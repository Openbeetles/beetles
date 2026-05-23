/**
 * @file beetle_wakenet.c
 * @brief Minimal C wrapper over ESP-SR AFE WakeNet.
 *
 * The component owns one AFE WakeNet instance and a PSRAM feed-frame
 * accumulator. The feed hot path only feeds full AFE chunks; a separate
 * detection task blocks on AFE fetch and exposes detections through a tiny
 * non-blocking pending-event ABI consumed by Rust.
 */

#include "beetle_wakenet.h"

#include "esp_afe_sr_models.h"
#include "esp_err.h"
#include "esp_heap_caps.h"
#include "esp_log.h"
#include "esp_wn_iface.h"
#include "esp_wn_models.h"
#include "freertos/FreeRTOS.h"
#include "freertos/task.h"
#include "model_path.h"

#include <math.h>
#include <stdbool.h>
#include <stdint.h>
#include <string.h>

#define TAG "beetle_wakenet"
#define BEETLE_WN_DETECTION_TASK_STACK 4096
#define BEETLE_WN_DETECTION_TASK_PRIORITY 3
#define BEETLE_WN_PENDING_POLL_DELAY_TICKS pdMS_TO_TICKS(10)
#define BEETLE_WN_INPUT_FORMAT_MONO "M"
#define BEETLE_WN_INPUT_FORMAT_WITH_SECONDARY "MMR"
#define BEETLE_WN_DIAG_CHANNELS 3
#define BEETLE_WN_DIAG_WINDOW_CHUNKS 16
#define BEETLE_WN_THRESHOLD_MIN 0.4f
#define BEETLE_WN_THRESHOLD_MAX 0.9999f
#define BEETLE_WN_DETECTION_MODE_LABEL "DET_MODE_95"

typedef struct {
    const esp_afe_sr_iface_t *afe;
    esp_afe_sr_data_t *afe_data;
    int feed_chunk_size;
    int feed_channels;
    int frame_pos;
    int input_sample_rate_hz;
    bool use_reference;
    int16_t *feed_frame;
    int16_t resample_mic_tail[2];
    int16_t resample_ref_tail[2];
    int resample_tail_len;
    uint32_t feed_diag_window_chunks;
    uint64_t feed_diag_window_frames;
    uint64_t feed_diag_window_abs_sum[BEETLE_WN_DIAG_CHANNELS];
    uint32_t feed_diag_window_peak[BEETLE_WN_DIAG_CHANNELS];
    TaskHandle_t detection_task;
    volatile bool destroy_requested;
    volatile bool pending_detection;
} beetle_wn_ctx_t;

static beetle_wn_ctx_t *s_ctx = NULL;
static srmodel_list_t *s_models = NULL;

static uint32_t sample_abs_i16(int16_t sample) {
    int32_t value = sample;
    return value < 0 ? (uint32_t)-value : (uint32_t)value;
}

static uint32_t abs_sum_permille(uint64_t abs_sum, uint64_t samples) {
    if (samples == 0) {
        return 0;
    }
    return (uint32_t)((abs_sum * 1000ULL) / (samples * 32768ULL));
}

static uint32_t sample_permille(uint32_t magnitude) {
    return (uint32_t)(((uint64_t)magnitude * 1000ULL) / 32768ULL);
}

static void reset_feed_diag_window(beetle_wn_ctx_t *ctx) {
    if (ctx == NULL) {
        return;
    }
    ctx->feed_diag_window_chunks = 0;
    ctx->feed_diag_window_frames = 0;
    for (int ch = 0; ch < BEETLE_WN_DIAG_CHANNELS; ++ch) {
        ctx->feed_diag_window_abs_sum[ch] = 0;
        ctx->feed_diag_window_peak[ch] = 0;
    }
}

static bool valid_wakenet_index(int index) {
    return index == 1 || index == 2;
}

static void log_and_reset_feed_diag_window(beetle_wn_ctx_t *ctx) {
    if (ctx == NULL || ctx->feed_diag_window_chunks == 0 ||
        ctx->feed_diag_window_frames == 0) {
        return;
    }

    ESP_LOGI(
        TAG,
        "feed16k window chunks=%u frames=%llu mic0_avg_pm=%u mic0_peak_pm=%u mic1_avg_pm=%u mic1_peak_pm=%u ref_avg_pm=%u ref_peak_pm=%u",
        (unsigned int)ctx->feed_diag_window_chunks,
        (unsigned long long)ctx->feed_diag_window_frames,
        (unsigned int)abs_sum_permille(ctx->feed_diag_window_abs_sum[0], ctx->feed_diag_window_frames),
        (unsigned int)sample_permille(ctx->feed_diag_window_peak[0]),
        (unsigned int)abs_sum_permille(ctx->feed_diag_window_abs_sum[1], ctx->feed_diag_window_frames),
        (unsigned int)sample_permille(ctx->feed_diag_window_peak[1]),
        (unsigned int)abs_sum_permille(ctx->feed_diag_window_abs_sum[2], ctx->feed_diag_window_frames),
        (unsigned int)sample_permille(ctx->feed_diag_window_peak[2])
    );

    reset_feed_diag_window(ctx);
}

static void accumulate_feed_diag_sample(
    beetle_wn_ctx_t *ctx,
    int16_t mic0,
    int16_t mic1,
    int16_t playback_ref
) {
    if (ctx == NULL) {
        return;
    }

    const int16_t values[BEETLE_WN_DIAG_CHANNELS] = {mic0, mic1, playback_ref};
    ctx->feed_diag_window_frames++;
    for (int ch = 0; ch < BEETLE_WN_DIAG_CHANNELS; ++ch) {
        uint32_t magnitude = sample_abs_i16(values[ch]);
        ctx->feed_diag_window_abs_sum[ch] += magnitude;
        if (magnitude > ctx->feed_diag_window_peak[ch]) {
            ctx->feed_diag_window_peak[ch] = magnitude;
        }
    }
}

static void finish_feed_diag_chunk(beetle_wn_ctx_t *ctx) {
    if (ctx == NULL) {
        return;
    }

    ctx->feed_diag_window_chunks++;
    if (ctx->feed_diag_window_chunks >= BEETLE_WN_DIAG_WINDOW_CHUNKS) {
        log_and_reset_feed_diag_window(ctx);
    }
}

static void detection_task_main(void *arg) {
    beetle_wn_ctx_t *ctx = (beetle_wn_ctx_t *)arg;
    if (ctx == NULL || ctx->afe == NULL || ctx->afe_data == NULL) {
        vTaskDelete(NULL);
        return;
    }

    int fetch_chunk_size = ctx->afe->get_fetch_chunksize(ctx->afe_data);
    ESP_LOGI(TAG, "WakeNet detection task started, feed size: %d fetch size: %d",
             ctx->feed_chunk_size, fetch_chunk_size);

    while (!ctx->destroy_requested) {
        if (ctx->pending_detection) {
            vTaskDelay(BEETLE_WN_PENDING_POLL_DELAY_TICKS);
            continue;
        }

        afe_fetch_result_t *res = ctx->afe->fetch_with_delay(ctx->afe_data, portMAX_DELAY);
        if (ctx->destroy_requested) {
            break;
        }
        if (res == NULL || res->ret_value == ESP_FAIL) {
            continue;
        }
        if (res->wakeup_state == WAKENET_DETECTED) {
            ESP_LOGI(
                TAG,
                "WakeNet fetch detected wakeup_state=%d wake_word_index=%d wakenet_model_index=%d trigger_channel_id=%d wake_word_length=%d data_volume=%.2f ringbuff_free_pct=%.3f",
                (int)res->wakeup_state,
                res->wake_word_index,
                res->wakenet_model_index,
                res->trigger_channel_id,
                res->wake_word_length,
                (double)res->data_volume,
                (double)res->ringbuff_free_pct
            );
            ctx->pending_detection = true;
        }
    }

    vTaskDelete(NULL);
}

static void ctx_destroy(void) {
    if (s_ctx == NULL) {
        return;
    }

    s_ctx->destroy_requested = true;
    if (s_ctx->detection_task != NULL) {
        vTaskDelete(s_ctx->detection_task);
        s_ctx->detection_task = NULL;
    }

    if (s_ctx->afe != NULL && s_ctx->afe_data != NULL) {
        s_ctx->afe->destroy(s_ctx->afe_data);
        s_ctx->afe_data = NULL;
    }
    if (s_ctx->feed_frame != NULL) {
        heap_caps_free(s_ctx->feed_frame);
        s_ctx->feed_frame = NULL;
    }
    heap_caps_free(s_ctx);
    s_ctx = NULL;
}

static void models_destroy(void) {
    if (s_models != NULL) {
        esp_srmodel_deinit(s_models);
        s_models = NULL;
    }
}

static void feed_16k_sample(int16_t mic, int16_t reference) {
    int index = s_ctx->frame_pos * s_ctx->feed_channels;
    int16_t mic1 = 0;
    int16_t playback_ref = 0;
    for (int ch = 0; ch < s_ctx->feed_channels; ++ch) {
        s_ctx->feed_frame[index + ch] = 0;
    }
    s_ctx->feed_frame[index] = mic;
    if (s_ctx->feed_channels > 1) {
        s_ctx->feed_frame[index + 1] = reference;
        mic1 = reference;
    }
    if (s_ctx->feed_channels > 2) {
        s_ctx->feed_frame[index + 2] = 0;
    }
    accumulate_feed_diag_sample(s_ctx, mic, mic1, playback_ref);
    s_ctx->frame_pos++;

    if (s_ctx->frame_pos < s_ctx->feed_chunk_size) {
        return;
    }

    s_ctx->afe->feed(s_ctx->afe_data, s_ctx->feed_frame);
    finish_feed_diag_chunk(s_ctx);
    s_ctx->frame_pos = 0;
}

beetle_wn_err_t beetle_wakenet_init(const char *model_name, int input_sample_rate_hz, int use_reference) {
    ctx_destroy();

    if (model_name == NULL || model_name[0] == '\0') {
        return BEETLE_WN_ERR_MODEL;
    }
    if (input_sample_rate_hz != 16000 && input_sample_rate_hz != 24000) {
        return BEETLE_WN_ERR_STATE;
    }

    if (s_models == NULL) {
        s_models = esp_srmodel_init("model");
    }
    if (s_models == NULL) {
        return BEETLE_WN_ERR_MODEL;
    }

    char *wn_name = esp_srmodel_filter(s_models, ESP_WN_PREFIX, (char *)model_name);
    if (wn_name == NULL) {
        return BEETLE_WN_ERR_MODEL;
    }

    const char *input_format = use_reference
        ? BEETLE_WN_INPUT_FORMAT_WITH_SECONDARY
        : BEETLE_WN_INPUT_FORMAT_MONO;
    afe_config_t *afe_config = afe_config_init(input_format, s_models, AFE_TYPE_SR, AFE_MODE_HIGH_PERF);
    if (afe_config == NULL) {
        return BEETLE_WN_ERR_NOMEM;
    }
    afe_config->aec_init = false;
    afe_config->aec_mode = AEC_MODE_SR_HIGH_PERF;
    afe_config->wakenet_init = true;
    afe_config->wakenet_model_name = wn_name;
    afe_config->wakenet_mode = DET_MODE_95;
    afe_config->afe_perferred_core = 1;
    afe_config->afe_perferred_priority = 1;
    afe_config->memory_alloc_mode = AFE_MEMORY_ALLOC_MORE_PSRAM;
    afe_config->fixed_first_channel = true;

    const esp_afe_sr_iface_t *afe = esp_afe_handle_from_config(afe_config);
    if (afe == NULL) {
        afe_config_free(afe_config);
        return BEETLE_WN_ERR_STATE;
    }
    esp_afe_sr_data_t *afe_data = afe->create_from_config(afe_config);
    afe_config_free(afe_config);
    if (afe_data == NULL) {
        return BEETLE_WN_ERR_NOMEM;
    }

    int feed_chunk_size = afe->get_feed_chunksize(afe_data);
    int feed_channels = afe->get_feed_channel_num(afe_data);
    if (feed_chunk_size <= 0 || feed_channels <= 0) {
        afe->destroy(afe_data);
        return BEETLE_WN_ERR_STATE;
    }

    int16_t *feed_frame = (int16_t *)heap_caps_malloc(
        (size_t)feed_chunk_size * (size_t)feed_channels * sizeof(int16_t),
        MALLOC_CAP_SPIRAM | MALLOC_CAP_8BIT);
    if (feed_frame == NULL) {
        afe->destroy(afe_data);
        return BEETLE_WN_ERR_NOMEM;
    }

    beetle_wn_ctx_t *ctx = (beetle_wn_ctx_t *)heap_caps_malloc(
        sizeof(beetle_wn_ctx_t), MALLOC_CAP_INTERNAL | MALLOC_CAP_8BIT);
    if (ctx == NULL) {
        heap_caps_free(feed_frame);
        afe->destroy(afe_data);
        return BEETLE_WN_ERR_NOMEM;
    }

    memset(ctx, 0, sizeof(*ctx));
    ctx->afe = afe;
    ctx->afe_data = afe_data;
    ctx->feed_chunk_size = feed_chunk_size;
    ctx->feed_channels = feed_channels;
    ctx->frame_pos = 0;
    ctx->input_sample_rate_hz = input_sample_rate_hz;
    ctx->use_reference = use_reference ? true : false;
    ctx->feed_frame = feed_frame;
    ctx->resample_tail_len = 0;

    BaseType_t task_created = xTaskCreate(
        detection_task_main,
        "wakenet_detect",
        BEETLE_WN_DETECTION_TASK_STACK,
        ctx,
        BEETLE_WN_DETECTION_TASK_PRIORITY,
        &ctx->detection_task);
    if (task_created != pdPASS) {
        heap_caps_free(feed_frame);
        afe->destroy(afe_data);
        heap_caps_free(ctx);
        return BEETLE_WN_ERR_NOMEM;
    }

    s_ctx = ctx;
    ESP_LOGI(
        TAG,
        "WakeNet init profile model=%s mode=%s threshold=default threshold_index=1 input_hz=%d feed_channels=%d feed_chunk=%d",
        wn_name,
        BEETLE_WN_DETECTION_MODE_LABEL,
        input_sample_rate_hz,
        feed_channels,
        feed_chunk_size
    );

    return BEETLE_WN_OK;
}

beetle_wn_result_t beetle_wakenet_feed(const int16_t *mic, const int16_t *reference, int samples) {
    if (s_ctx == NULL || mic == NULL || samples <= 0) {
        return BEETLE_WN_NO;
    }

    if (s_ctx->input_sample_rate_hz == 16000) {
        for (int i = 0; i < samples; ++i) {
            int16_t ref_sample = (s_ctx->use_reference && reference != NULL) ? reference[i] : 0;
            feed_16k_sample(mic[i], ref_sample);
        }
        return BEETLE_WN_NO;
    }

    int16_t mic_triple[3];
    int16_t ref_triple[3];
    int triple_len = s_ctx->resample_tail_len;
    if (triple_len > 0) {
        memcpy(mic_triple, s_ctx->resample_mic_tail, (size_t)triple_len * sizeof(int16_t));
        memcpy(ref_triple, s_ctx->resample_ref_tail, (size_t)triple_len * sizeof(int16_t));
    }

    for (int i = 0; i < samples; ++i) {
        mic_triple[triple_len] = mic[i];
        ref_triple[triple_len] = (s_ctx->use_reference && reference != NULL) ? reference[i] : 0;
        triple_len++;
        if (triple_len < 3) {
            continue;
        }

        feed_16k_sample(mic_triple[0], ref_triple[0]);

        int32_t mic_blended = ((int32_t)mic_triple[1] + (int32_t)mic_triple[2]) / 2;
        int32_t ref_blended = ((int32_t)ref_triple[1] + (int32_t)ref_triple[2]) / 2;
        feed_16k_sample((int16_t)mic_blended, (int16_t)ref_blended);

        triple_len = 0;
    }

    s_ctx->resample_tail_len = triple_len;
    if (triple_len > 0) {
        memcpy(s_ctx->resample_mic_tail, mic_triple, (size_t)triple_len * sizeof(int16_t));
        memcpy(s_ctx->resample_ref_tail, ref_triple, (size_t)triple_len * sizeof(int16_t));
    }
    return BEETLE_WN_NO;
}

beetle_wn_result_t beetle_wakenet_take_event(void) {
    if (s_ctx == NULL || !s_ctx->pending_detection) {
        return BEETLE_WN_NO;
    }
    s_ctx->pending_detection = false;
    return BEETLE_WN_DETECTED;
}

beetle_wn_err_t beetle_wakenet_set_threshold(int index, float threshold) {
    if (s_ctx == NULL || s_ctx->afe == NULL || s_ctx->afe_data == NULL) {
        ESP_LOGE(
            TAG,
            "WakeNet threshold apply failed reason=not_initialized mode=%s index=%d threshold=%.4f",
            BEETLE_WN_DETECTION_MODE_LABEL,
            index,
            (double)threshold
        );
        return BEETLE_WN_ERR_STATE;
    }
    if (!valid_wakenet_index(index) ||
        !isfinite(threshold) ||
        threshold < BEETLE_WN_THRESHOLD_MIN ||
        threshold > BEETLE_WN_THRESHOLD_MAX) {
        ESP_LOGE(
            TAG,
            "WakeNet threshold apply failed reason=invalid_arg mode=%s index=%d threshold=%.4f allowed=0.4..0.9999",
            BEETLE_WN_DETECTION_MODE_LABEL,
            index,
            (double)threshold
        );
        return BEETLE_WN_ERR_ARG;
    }
    if (s_ctx->afe->set_wakenet_threshold == NULL) {
        ESP_LOGE(
            TAG,
            "WakeNet threshold apply failed reason=unsupported_abi mode=%s index=%d threshold=%.4f",
            BEETLE_WN_DETECTION_MODE_LABEL,
            index,
            (double)threshold
        );
        return BEETLE_WN_ERR_UNSUPPORTED;
    }

    int rc = s_ctx->afe->set_wakenet_threshold(s_ctx->afe_data, index, threshold);
    if (rc != 1) {
        ESP_LOGE(
            TAG,
            "WakeNet threshold apply failed reason=esp_sr_rejected mode=%s index=%d threshold=%.4f rc=%d",
            BEETLE_WN_DETECTION_MODE_LABEL,
            index,
            (double)threshold,
            rc
        );
        return BEETLE_WN_ERR_STATE;
    }

    ESP_LOGI(
        TAG,
        "WakeNet threshold apply ok mode=%s index=%d threshold=%.4f",
        BEETLE_WN_DETECTION_MODE_LABEL,
        index,
        (double)threshold
    );
    return BEETLE_WN_OK;
}

beetle_wn_err_t beetle_wakenet_reset_threshold(int index) {
    if (s_ctx == NULL || s_ctx->afe == NULL || s_ctx->afe_data == NULL) {
        ESP_LOGE(
            TAG,
            "WakeNet threshold reset failed reason=not_initialized mode=%s index=%d",
            BEETLE_WN_DETECTION_MODE_LABEL,
            index
        );
        return BEETLE_WN_ERR_STATE;
    }
    if (!valid_wakenet_index(index)) {
        ESP_LOGE(
            TAG,
            "WakeNet threshold reset failed reason=invalid_arg mode=%s index=%d",
            BEETLE_WN_DETECTION_MODE_LABEL,
            index
        );
        return BEETLE_WN_ERR_ARG;
    }
    if (s_ctx->afe->reset_wakenet_threshold == NULL) {
        ESP_LOGE(
            TAG,
            "WakeNet threshold reset failed reason=unsupported_abi mode=%s index=%d",
            BEETLE_WN_DETECTION_MODE_LABEL,
            index
        );
        return BEETLE_WN_ERR_UNSUPPORTED;
    }

    int rc = s_ctx->afe->reset_wakenet_threshold(s_ctx->afe_data, index);
    if (rc != 1) {
        ESP_LOGE(
            TAG,
            "WakeNet threshold reset failed reason=esp_sr_rejected mode=%s index=%d rc=%d",
            BEETLE_WN_DETECTION_MODE_LABEL,
            index,
            rc
        );
        return BEETLE_WN_ERR_STATE;
    }

    ESP_LOGI(
        TAG,
        "WakeNet threshold reset ok mode=%s index=%d threshold=default",
        BEETLE_WN_DETECTION_MODE_LABEL,
        index
    );
    return BEETLE_WN_OK;
}

void beetle_wakenet_reset(void) {
    if (s_ctx != NULL) {
        if (s_ctx->afe != NULL && s_ctx->afe_data != NULL) {
            s_ctx->afe->reset_buffer(s_ctx->afe_data);
        }
        s_ctx->frame_pos = 0;
        s_ctx->resample_tail_len = 0;
        s_ctx->pending_detection = false;
        reset_feed_diag_window(s_ctx);
    }
}

void beetle_wakenet_destroy(void) {
    ctx_destroy();
    models_destroy();
}
