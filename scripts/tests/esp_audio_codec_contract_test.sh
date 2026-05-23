#!/usr/bin/env bash
set -euo pipefail

root="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
codec_c="$root/components/beetle_audio_codec/beetle_audio_codec.c"
codec_h="$root/components/beetle_audio_codec/include/beetle_audio_codec.h"
codec_rs="$root/src/platform/audio_codec_backend.rs"
audio_driver_rs="$root/src/platform/audio_drivers.rs"

fail() {
  printf 'FAIL: %s\n' "$1" >&2
  exit 1
}

grep -q '#define BEETLE_AUDIO_CODEC_INPUT_REFERENCE_CHANNELS 2U' "$codec_c" \
  || fail "ESP-BOX3 ES7210 input-reference capture must expose MIC1/MIC2 as two app-visible channels"

grep -q '#define BEETLE_AUDIO_CODEC_INPUT_REFERENCE_OUTPUT_CHANNELS 2U' "$codec_c" \
  || fail "ES7210 input-reference runtime output must stay a mic/secondary pair"

grep -q '#define BEETLE_AUDIO_CODEC_DEFAULT_OUT_VOL 100' "$codec_c" \
  || fail "ESP-BOX3 speaker output volume must stay at max until runtime volume config exists"

grep -q 'i2s_channel_init_std_mode(codec->rx_handle, &rx_cfg)' "$codec_c" \
  || fail "ESP-BOX3 microphone RX must use official BSP-style STD I2S, not TDM"

if grep -q 'i2s_channel_init_tdm_mode(codec->rx_handle' "$codec_c"; then
  fail "ESP-BOX3 microphone RX must not initialize 4-slot TDM"
fi

if grep -q 'ES7210_SEL_MIC3' "$codec_c" || grep -q 'ES7210_SEL_MIC4' "$codec_c"; then
  fail "ESP-BOX3 ES7210 init must not force 4-mic TDM selection"
fi

grep -q '.channel = config->input_reference ? BEETLE_AUDIO_CODEC_INPUT_REFERENCE_CHANNELS : 1' "$codec_c" \
  || fail "ES7210 input-reference open must request two app-visible MIC channels"

grep -q 'codec->output_channels = config->input_reference ? BEETLE_AUDIO_CODEC_INPUT_REFERENCE_CHANNELS : 1' "$codec_c" \
  || fail "input-reference duplex mode must keep TX on a stereo STD clock for ES7210 MIC1/MIC2 RX"

grep -q 'beetle_audio_codec_make_channel_mask(BEETLE_AUDIO_CODEC_INPUT_REFERENCE_CHANNELS)' "$codec_c" \
  || fail "ES7210 input-reference capture must select MIC1/MIC2 channels"

grep -q 'codec->input_channels = config->input_reference ? BEETLE_AUDIO_CODEC_INPUT_REFERENCE_CHANNELS : 1' "$codec_c" \
  || fail "ES7210 input-reference read buffer must match the two app-visible input channels"

grep -q 'beetle_audio_codec_make_channel_mask(codec->input_channels)' "$codec_c" \
  || fail "ES7210 input-reference gain must cover every captured app-visible channel"

grep -q '#define BEETLE_AUDIO_CODEC_DEFAULT_IN_GAIN 37\.5f' "$codec_c" \
  || fail "ES7210 input gain must use the measured high-gain profile for ESP-BOX3 low input levels"

grep -q 'sample_count > SIZE_MAX / input_channels / sizeof(int16_t)' "$codec_c" \
  || fail "input-reference read buffer sizing must guard multiplication overflow"

grep -q 'buffered_samples = sample_count \* input_channels' "$codec_c" \
  || fail "input-reference read buffer must scale by the opened input channel count"

if grep -q 'beetle_audio_codec_select_primary_pair' "$codec_c"; then
  fail "ESP-BOX3 official MIC1/MIC2 capture must keep fixed channel order, not select a TDM pair"
fi

grep -Fq 'out_samples[i] = codec->input_read_buf[i * input_channels];' "$codec_c" \
  || fail "input-reference deinterleave must emit MIC1/channel0 as primary mic"

grep -Fq 'out_reference[i] = codec->input_read_buf[i * input_channels + 1U];' "$codec_c" \
  || fail "input-reference deinterleave must emit MIC2/channel1 as secondary input"

grep -Fq 'codec->output_write_buf[i * codec->output_channels + 1U] = samples[i];' "$codec_c" \
  || fail "stereo-clock speaker writes must duplicate mono samples into the second TX slot"

grep -q 'beetle_audio_codec_read_mic_reference_pcm16' "$codec_h" \
  || fail "codec C ABI must expose a mic/reference read entry point"

grep -q 'fn beetle_audio_codec_read_mic_reference_pcm16' "$codec_rs" \
  || fail "Rust codec backend must bind the mic/reference C ABI"

grep -q 'fn read_mic_reference_frame_pcm16' "$audio_driver_rs" \
  || fail "audio worker backend contract must support mic/reference frame reads"

grep -q 'if !input_reference_capture' "$audio_driver_rs" \
  || fail "input-reference codec mode must not push playback monitor frames into the reference ring"

if grep -q 'BEETLE_AUDIO_CODEC_CHANNEL_SWITCH_NUM' "$codec_c"; then
  fail "ESP-BOX3 official MIC1/MIC2 capture must not carry TDM slot-switch hysteresis"
fi

grep -q 'input reference diag ch0_abs_pm=.*ch1_abs_pm=' "$codec_c" \
  || fail "input-reference diagnostics must report MIC1/MIC2 levels"

grep -q 'input reference window reads=.*ch0_avg_pm=.*ch0_peak_pm=.*ch1_avg_pm=.*ch1_peak_pm=' "$codec_c" \
  || fail "input-reference diagnostics must report MIC1/MIC2 window average and peak levels"

printf 'PASS: esp audio codec ES7210 input-reference contract\n'
