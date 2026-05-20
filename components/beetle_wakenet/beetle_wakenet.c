/**
 * @file beetle_wakenet.c
 * @brief Minimal C wrapper over ESP-SR AFE WakeNet.
 *
 * The component owns one AFE WakeNet instance and a PSRAM feed-frame
 * accumulator. The Rust side is responsible for locking, cooldown, lifecycle
 * policy, and event handoff; this file only performs model initialization,
 * optional playback-reference formatting, and non-blocking detection polling.
 */

#include "beetle_wakenet.h"

#include "esp_afe_sr_models.h"
#include "esp_err.h"
#include "esp_heap_caps.h"
#include "esp_wn_iface.h"
#include "esp_wn_models.h"
#include "freertos/FreeRTOS.h"
#include "model_path.h"

#include <stdbool.h>
#include <string.h>

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
} beetle_wn_ctx_t;

static beetle_wn_ctx_t *s_ctx = NULL;
static srmodel_list_t *s_models = NULL;

static void ctx_destroy(void) {
    if (s_ctx == NULL) {
        return;
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

static beetle_wn_result_t poll_detection(void) {
    if (s_ctx == NULL || s_ctx->afe == NULL || s_ctx->afe_data == NULL) {
        return BEETLE_WN_NO;
    }

    afe_fetch_result_t *res = s_ctx->afe->fetch_with_delay(s_ctx->afe_data, 0);
    if (res == NULL || res->ret_value == ESP_FAIL) {
        return BEETLE_WN_NO;
    }
    if (res->wakeup_state == WAKENET_DETECTED) {
        s_ctx->afe->reset_buffer(s_ctx->afe_data);
        s_ctx->frame_pos = 0;
        s_ctx->resample_tail_len = 0;
        return BEETLE_WN_DETECTED;
    }
    return BEETLE_WN_NO;
}

static beetle_wn_result_t feed_16k_sample(int16_t mic, int16_t reference) {
    int index = s_ctx->frame_pos * s_ctx->feed_channels;
    s_ctx->feed_frame[index] = mic;
    if (s_ctx->feed_channels > 1) {
        s_ctx->feed_frame[index + 1] = reference;
    }
    s_ctx->frame_pos++;

    if (s_ctx->frame_pos < s_ctx->feed_chunk_size) {
        return poll_detection();
    }

    s_ctx->afe->feed(s_ctx->afe_data, s_ctx->feed_frame);
    s_ctx->frame_pos = 0;
    return poll_detection();
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

    const char *input_format = use_reference ? "MR" : "M";
    afe_config_t *afe_config = afe_config_init(input_format, s_models, AFE_TYPE_SR, AFE_MODE_HIGH_PERF);
    if (afe_config == NULL) {
        return BEETLE_WN_ERR_NOMEM;
    }
    afe_config->aec_init = use_reference ? true : false;
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

    s_ctx = (beetle_wn_ctx_t *)heap_caps_malloc(
        sizeof(beetle_wn_ctx_t), MALLOC_CAP_INTERNAL | MALLOC_CAP_8BIT);
    if (s_ctx == NULL) {
        heap_caps_free(feed_frame);
        afe->destroy(afe_data);
        return BEETLE_WN_ERR_NOMEM;
    }

    s_ctx->afe = afe;
    s_ctx->afe_data = afe_data;
    s_ctx->feed_chunk_size = feed_chunk_size;
    s_ctx->feed_channels = feed_channels;
    s_ctx->frame_pos = 0;
    s_ctx->input_sample_rate_hz = input_sample_rate_hz;
    s_ctx->use_reference = use_reference ? true : false;
    s_ctx->feed_frame = feed_frame;
    s_ctx->resample_tail_len = 0;

    return BEETLE_WN_OK;
}

beetle_wn_result_t beetle_wakenet_feed(const int16_t *mic, const int16_t *reference, int samples) {
    if (s_ctx == NULL || mic == NULL || samples <= 0) {
        return BEETLE_WN_NO;
    }

    if (s_ctx->input_sample_rate_hz == 16000) {
        for (int i = 0; i < samples; ++i) {
            int16_t ref_sample = (s_ctx->use_reference && reference != NULL) ? reference[i] : 0;
            if (feed_16k_sample(mic[i], ref_sample) == BEETLE_WN_DETECTED) {
                return BEETLE_WN_DETECTED;
            }
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

        if (feed_16k_sample(mic_triple[0], ref_triple[0]) == BEETLE_WN_DETECTED) {
            return BEETLE_WN_DETECTED;
        }

        int32_t mic_blended = ((int32_t)mic_triple[1] + (int32_t)mic_triple[2]) / 2;
        int32_t ref_blended = ((int32_t)ref_triple[1] + (int32_t)ref_triple[2]) / 2;
        if (feed_16k_sample((int16_t)mic_blended, (int16_t)ref_blended) == BEETLE_WN_DETECTED) {
            return BEETLE_WN_DETECTED;
        }

        triple_len = 0;
    }

    s_ctx->resample_tail_len = triple_len;
    if (triple_len > 0) {
        memcpy(s_ctx->resample_mic_tail, mic_triple, (size_t)triple_len * sizeof(int16_t));
        memcpy(s_ctx->resample_ref_tail, ref_triple, (size_t)triple_len * sizeof(int16_t));
    }
    return BEETLE_WN_NO;
}

void beetle_wakenet_reset(void) {
    if (s_ctx != NULL) {
        if (s_ctx->afe != NULL && s_ctx->afe_data != NULL) {
            s_ctx->afe->reset_buffer(s_ctx->afe_data);
        }
        s_ctx->frame_pos = 0;
        s_ctx->resample_tail_len = 0;
    }
}

void beetle_wakenet_destroy(void) {
    ctx_destroy();
    models_destroy();
}
