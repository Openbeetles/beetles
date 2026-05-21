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

grep -q '#define BEETLE_AUDIO_CODEC_TDM_INPUT_SLOTS 4U' "$codec_c" \
  || fail "ES7210 hardware TDM slot count must stay explicit"

grep -q '#define BEETLE_AUDIO_CODEC_INPUT_REFERENCE_OUTPUT_CHANNELS 2U' "$codec_c" \
  || fail "ES7210 input-reference runtime output must stay a mic/secondary pair"

grep -q '.channel = config->input_reference ? BEETLE_AUDIO_CODEC_TDM_INPUT_SLOTS : 1' "$codec_c" \
  || fail "ES7210 input-reference open must keep the hardware TDM frame width at 4 slots"

grep -q 'beetle_audio_codec_make_channel_mask(BEETLE_AUDIO_CODEC_TDM_INPUT_SLOTS)' "$codec_c" \
  || fail "ES7210 input-reference capture must keep all TDM slots observable"

grep -q 'codec->input_channels = config->input_reference ? BEETLE_AUDIO_CODEC_TDM_INPUT_SLOTS : 1' "$codec_c" \
  || fail "ES7210 input-reference read buffer must match the full TDM capture width"

grep -q 'beetle_audio_codec_make_channel_mask(codec->input_channels)' "$codec_c" \
  || fail "ES7210 input-reference gain must cover every captured TDM slot"

grep -q 'sample_count > SIZE_MAX / input_channels / sizeof(int16_t)' "$codec_c" \
  || fail "input-reference read buffer sizing must guard multiplication overflow"

grep -q 'buffered_samples = sample_count \* input_channels' "$codec_c" \
  || fail "input-reference read buffer must scale by the opened input channel count"

grep -q 'beetle_audio_codec_select_primary_pair' "$codec_c" \
  || fail "input-reference capture must select a primary mic and secondary input from observed ES7210 slots"

grep -Fq 'out_samples[i] = codec->input_read_buf[i * input_channels + selected_mic];' "$codec_c" \
  || fail "input-reference deinterleave must emit the selected primary mic"

grep -Fq 'out_reference[i] = codec->input_read_buf[i * input_channels + selected_secondary];' "$codec_c" \
  || fail "input-reference deinterleave must emit the selected secondary ES7210 input"

grep -q 'beetle_audio_codec_read_mic_reference_pcm16' "$codec_h" \
  || fail "codec C ABI must expose a mic/reference read entry point"

grep -q 'fn beetle_audio_codec_read_mic_reference_pcm16' "$codec_rs" \
  || fail "Rust codec backend must bind the mic/reference C ABI"

grep -q 'fn read_mic_reference_frame_pcm16' "$audio_driver_rs" \
  || fail "audio worker backend contract must support mic/reference frame reads"

grep -q 'if !input_reference_capture' "$audio_driver_rs" \
  || fail "input-reference codec mode must not push playback monitor frames into the reference ring"

grep -q 'BEETLE_AUDIO_CODEC_CHANNEL_SWITCH_NUM' "$codec_c" \
  || fail "input-reference primary channel selection must include hysteresis to avoid slot flapping"

grep -q 'beetle_audio_codec_select_loudest_channel' "$codec_c" \
  && fail "input-reference selection must choose a mic/secondary pair, not a single acoustic-era channel"

grep -q 'input reference diag selected_mic=.*selected_secondary=.*ch0_abs_pm=.*ch1_abs_pm=.*ch2_abs_pm=.*ch3_abs_pm=' "$codec_c" \
  || fail "input-reference diagnostics must report selected pair and all ES7210 TDM slot levels"

printf 'PASS: esp audio codec ES7210 input-reference contract\n'
