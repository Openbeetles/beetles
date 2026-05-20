/**
 * @file beetle_wakenet.c
 * @brief Minimal C wrapper over ESP-SR WakeNet.
 *
 * The component owns one WakeNet instance and a PSRAM chunk accumulator. The
 * Rust side is responsible for locking, cooldown, lifecycle policy, and event
 * handoff; this file only performs model initialization and PCM detection.
 */

#include "beetle_wakenet.h"

#include "esp_heap_caps.h"
#include "esp_wn_iface.h"
#include "esp_wn_models.h"
#include "model_path.h"

#include <string.h>

typedef struct {
    esp_wn_iface_t *wakenet;
    model_iface_data_t *model_data;
    int chunk_size;
    int16_t *accumulator;
    int acc_pos;
    int input_sample_rate_hz;
    int16_t resample_tail[2];
    int resample_tail_len;
} beetle_wn_ctx_t;

static beetle_wn_ctx_t *s_ctx = NULL;
static srmodel_list_t *s_models = NULL;

static void ctx_destroy(void) {
    if (s_ctx == NULL) {
        return;
    }

    if (s_ctx->wakenet != NULL && s_ctx->model_data != NULL) {
        s_ctx->wakenet->destroy(s_ctx->model_data);
        s_ctx->model_data = NULL;
    }
    if (s_ctx->accumulator != NULL) {
        heap_caps_free(s_ctx->accumulator);
        s_ctx->accumulator = NULL;
    }
    heap_caps_free(s_ctx);
    s_ctx = NULL;
}

static beetle_wn_result_t feed_detect_sample(int16_t sample) {
    s_ctx->accumulator[s_ctx->acc_pos++] = sample;
    if (s_ctx->acc_pos < s_ctx->chunk_size) {
        return BEETLE_WN_NO;
    }

    int result = s_ctx->wakenet->detect(s_ctx->model_data, s_ctx->accumulator);
    s_ctx->acc_pos = 0;
    if (result > 0) {
        return BEETLE_WN_DETECTED;
    }
    return BEETLE_WN_NO;
}

beetle_wn_err_t beetle_wakenet_init(const char *model_name, int input_sample_rate_hz) {
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

    esp_wn_iface_t *wakenet = esp_wn_handle_from_name(wn_name);
    if (wakenet == NULL) {
        return BEETLE_WN_ERR_MODEL;
    }

    model_iface_data_t *model_data = wakenet->create(wn_name, DET_MODE_95);
    if (model_data == NULL) {
        return BEETLE_WN_ERR_NOMEM;
    }

    int chunk_size = wakenet->get_samp_chunksize(model_data);
    if (chunk_size <= 0) {
        wakenet->destroy(model_data);
        return BEETLE_WN_ERR_STATE;
    }

    int16_t *acc = (int16_t *)heap_caps_malloc(
        (size_t)chunk_size * sizeof(int16_t), MALLOC_CAP_SPIRAM | MALLOC_CAP_8BIT);
    if (acc == NULL) {
        wakenet->destroy(model_data);
        return BEETLE_WN_ERR_NOMEM;
    }

    s_ctx = (beetle_wn_ctx_t *)heap_caps_malloc(
        sizeof(beetle_wn_ctx_t), MALLOC_CAP_INTERNAL | MALLOC_CAP_8BIT);
    if (s_ctx == NULL) {
        heap_caps_free(acc);
        wakenet->destroy(model_data);
        return BEETLE_WN_ERR_NOMEM;
    }

    s_ctx->wakenet = wakenet;
    s_ctx->model_data = model_data;
    s_ctx->chunk_size = chunk_size;
    s_ctx->accumulator = acc;
    s_ctx->acc_pos = 0;
    s_ctx->input_sample_rate_hz = input_sample_rate_hz;
    s_ctx->resample_tail_len = 0;

    return BEETLE_WN_OK;
}

beetle_wn_result_t beetle_wakenet_feed(const int16_t *pcm, int samples) {
    if (s_ctx == NULL || pcm == NULL || samples <= 0) {
        return BEETLE_WN_NO;
    }

    if (s_ctx->input_sample_rate_hz == 16000) {
        for (int i = 0; i < samples; ++i) {
            if (feed_detect_sample(pcm[i]) == BEETLE_WN_DETECTED) {
                s_ctx->resample_tail_len = 0;
                return BEETLE_WN_DETECTED;
            }
        }
        return BEETLE_WN_NO;
    }

    int16_t triple[3];
    int triple_len = s_ctx->resample_tail_len;
    if (triple_len > 0) {
        memcpy(triple, s_ctx->resample_tail, (size_t)triple_len * sizeof(int16_t));
    }

    for (int i = 0; i < samples; ++i) {
        triple[triple_len++] = pcm[i];
        if (triple_len < 3) {
            continue;
        }

        if (feed_detect_sample(triple[0]) == BEETLE_WN_DETECTED) {
            s_ctx->resample_tail_len = 0;
            return BEETLE_WN_DETECTED;
        }

        int32_t blended = ((int32_t)triple[1] + (int32_t)triple[2]) / 2;
        if (feed_detect_sample((int16_t)blended) == BEETLE_WN_DETECTED) {
            s_ctx->resample_tail_len = 0;
            return BEETLE_WN_DETECTED;
        }

        triple_len = 0;
    }

    s_ctx->resample_tail_len = triple_len;
    if (triple_len > 0) {
        memcpy(s_ctx->resample_tail, triple, (size_t)triple_len * sizeof(int16_t));
    }
    return BEETLE_WN_NO;
}

void beetle_wakenet_reset(void) {
    if (s_ctx != NULL) {
        s_ctx->acc_pos = 0;
        s_ctx->resample_tail_len = 0;
    }
}

void beetle_wakenet_destroy(void) {
    ctx_destroy();
}
