/**
 * @file beetle_wakenet.h
 * @brief Stable C ABI for WakeNet wake-word detection (beetle project).
 *
 * Wraps ESP-SR WakeNet so that the Rust layer only needs to bind these three
 * functions and does not need to include the entire ESP-SR header tree.
 *
 * All functions are NOT thread-safe; the caller (Rust wake_word module) is
 * responsible for serialising access via a Mutex.
 */
#pragma once

#include <stdint.h>
#include <stddef.h>

#ifdef __cplusplus
extern "C" {
#endif

/** Return codes for beetle_wakenet_init(). */
typedef enum {
    BEETLE_WN_OK        =  0,  /**< Initialisation succeeded. */
    BEETLE_WN_ERR_MODEL = -1,  /**< Model not found in flash partition. */
    BEETLE_WN_ERR_NOMEM = -2,  /**< Heap allocation failed. */
    BEETLE_WN_ERR_STATE = -3,  /**< Called in invalid state. */
} beetle_wn_err_t;

/** Per-frame feed result. */
typedef enum {
    BEETLE_WN_NO       = 0,   /**< No wake word detected in this frame. */
    BEETLE_WN_DETECTED = 1,   /**< Wake word detected. */
} beetle_wn_result_t;

/**
 * @brief Initialise the WakeNet engine.
 *
 * Mounts the "model" flash partition (SPIFFS), locates @p model_name, creates
 * the WakeNet instance, and allocates an internal sample accumulator in PSRAM.
 *
 * Must be called before beetle_wakenet_feed().  Calling init again destroys the
 * previous instance first (safe reset path).
 *
 * @param model_name  Full WakeNet model name, e.g. "wn9_hiesp".
 *                    Must match a model present in the "model" flash partition.
 * @return BEETLE_WN_OK on success, negative error code on failure.
 */
beetle_wn_err_t beetle_wakenet_init(const char *model_name);

/**
 * @brief Feed a frame of 16-bit PCM samples.
 *
 * Internally accumulates samples until a full detection chunk (typically 480
 * samples at 16 kHz) is ready, then calls WakeNet detect().  Returns
 * BEETLE_WN_DETECTED as soon as the first triggered chunk is found; subsequent
 * samples in the same call are discarded (the caller should reset cooldown).
 *
 * Safe to call with n == 0 (no-op).
 *
 * @param pcm      Pointer to 16-bit signed PCM, mono, 16 kHz.
 * @param samples  Number of samples (NOT bytes).
 * @return BEETLE_WN_DETECTED if triggered, BEETLE_WN_NO otherwise.
 */
beetle_wn_result_t beetle_wakenet_feed(const int16_t *pcm, int samples);

/**
 * @brief Reset internal accumulator (e.g. after a triggered detection).
 *
 * Does not destroy the model instance; the engine remains ready for the next
 * feed() sequence.
 */
void beetle_wakenet_reset(void);

/**
 * @brief Destroy the WakeNet instance and free all resources.
 *
 * Safe to call even if init() was never called or failed.
 */
void beetle_wakenet_destroy(void);

#ifdef __cplusplus
}
#endif
