/**
 * @file beetle_audio_codec.h
 * @brief Minimal ESP codec backend shim for beetle i2s_codec topology.
 */
#pragma once

#include <stdbool.h>
#include <stddef.h>
#include <stdint.h>

#ifdef __cplusplus
extern "C" {
#endif

typedef struct beetle_audio_codec beetle_audio_codec_t;

typedef enum {
    BEETLE_AUDIO_CODEC_OK = 0,
    BEETLE_AUDIO_CODEC_ERR_INVALID_ARG = -1,
    BEETLE_AUDIO_CODEC_ERR_NOMEM = -2,
    BEETLE_AUDIO_CODEC_ERR_ESP = -3,
    BEETLE_AUDIO_CODEC_ERR_STATE = -4,
} beetle_audio_codec_status_t;

typedef struct {
    int input_sample_rate_hz;
    int output_sample_rate_hz;
    int i2c_sda_pin;
    int i2c_scl_pin;
    uint32_t i2c_freq_hz;
    int i2s_mclk_pin;
    int i2s_ws_pin;
    int i2s_bclk_pin;
    int i2s_din_pin;
    int i2s_dout_pin;
    int pa_pin;
    uint8_t input_addr;
    uint8_t output_addr;
    bool input_reference;
    bool mic_enabled;
    bool speaker_enabled;
} beetle_audio_codec_config_t;

beetle_audio_codec_status_t beetle_audio_codec_create(
    const beetle_audio_codec_config_t *config,
    beetle_audio_codec_t **out_codec);

void beetle_audio_codec_destroy(beetle_audio_codec_t *codec);

beetle_audio_codec_status_t beetle_audio_codec_read_mic_pcm16(
    beetle_audio_codec_t *codec,
    int16_t *out_samples,
    size_t sample_count,
    size_t *out_samples_read);

beetle_audio_codec_status_t beetle_audio_codec_read_mic_reference_pcm16(
    beetle_audio_codec_t *codec,
    int16_t *out_samples,
    int16_t *out_reference,
    size_t sample_count,
    size_t *out_samples_read);

beetle_audio_codec_status_t beetle_audio_codec_write_speaker_pcm16(
    beetle_audio_codec_t *codec,
    const int16_t *samples,
    size_t sample_count);

int beetle_audio_codec_last_esp_err(const beetle_audio_codec_t *codec);

#ifdef __cplusplus
}
#endif
