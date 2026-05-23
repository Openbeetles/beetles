/**
 * @file beetle_wakenet.h
 * @brief Stable C ABI for Beetle ESP-SR AFE WakeNet wake-word detection.
 *
 * This wrapper keeps ESP-SR headers inside the C component. Rust callers only
 * bind this small ABI and must serialize feed/reset/destroy access externally.
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
    BEETLE_WN_ERR_ARG = -4,
    BEETLE_WN_ERR_UNSUPPORTED = -5,
} beetle_wn_err_t;

typedef enum {
    BEETLE_WN_NO = 0,
    BEETLE_WN_DETECTED = 1,
} beetle_wn_result_t;

/**
 * Initialize AFE WakeNet with a model from the "model" partition.
 *
 * Supported input sample rates are 16000 and 24000 Hz. The 24000 Hz path is
 * normalized to AFE's 16000 Hz detector input inside this component.
 */
beetle_wn_err_t beetle_wakenet_init(const char *model_name, int input_sample_rate_hz, int use_reference);

/**
 * Feed mono signed 16-bit PCM samples and optional playback-reference samples at the sample rate passed to init().
 *
 * This call only fills AFE feed chunks and never polls detection synchronously.
 */
beetle_wn_result_t beetle_wakenet_feed(const int16_t *mic, const int16_t *reference, int samples);

/**
 * Non-blockingly consume one pending WakeNet detection from the detection task.
 */
beetle_wn_result_t beetle_wakenet_take_event(void);

/**
 * Apply the ESP-SR AFE WakeNet threshold for the selected model index.
 *
 * The threshold range follows the official ESP-SR AFE ABI: 0.4..0.9999.
 * This controls the WakeNet detector inside AFE; it is not the Linux/host
 * acoustic wake fallback threshold.
 */
beetle_wn_err_t beetle_wakenet_set_threshold(int index, float threshold);

/**
 * Reset the ESP-SR AFE WakeNet threshold for the selected model index.
 */
beetle_wn_err_t beetle_wakenet_reset_threshold(int index);

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
