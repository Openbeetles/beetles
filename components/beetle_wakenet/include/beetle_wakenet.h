/**
 * @file beetle_wakenet.h
 * @brief Stable C ABI for Beetle WakeNet wake-word detection.
 *
 * This wrapper keeps ESP-SR headers inside the C component. Rust callers only
 * bind this small ABI and must serialize access externally.
 */
#pragma once

#include <stdint.h>

#ifdef __cplusplus
extern "C" {
#endif

typedef enum {
    BEETLE_WN_OK = 0,
    BEETLE_WN_ERR_MODEL = -1,
    BEETLE_WN_ERR_NOMEM = -2,
    BEETLE_WN_ERR_STATE = -3,
} beetle_wn_err_t;

typedef enum {
    BEETLE_WN_NO = 0,
    BEETLE_WN_DETECTED = 1,
} beetle_wn_result_t;

/**
 * Initialize WakeNet with a model from the "model" partition.
 *
 * Supported input sample rates are 16000 and 24000 Hz. The 24000 Hz path is
 * normalized to WakeNet's 16000 Hz detector input inside this component.
 */
beetle_wn_err_t beetle_wakenet_init(const char *model_name, int input_sample_rate_hz);

/**
 * Feed mono signed 16-bit PCM samples at the sample rate passed to init().
 */
beetle_wn_result_t beetle_wakenet_feed(const int16_t *pcm, int samples);

/**
 * Reset the internal chunk and resampling accumulators without destroying the model.
 */
void beetle_wakenet_reset(void);

/**
 * Destroy the WakeNet instance. Safe to call before successful initialization.
 */
void beetle_wakenet_destroy(void);

#ifdef __cplusplus
}
#endif
