#!/usr/bin/env bash
set -euo pipefail

root="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
codec_c="$root/components/beetle_audio_codec/beetle_audio_codec.c"

fail() {
  printf 'FAIL: %s\n' "$1" >&2
  exit 1
}

grep -q '#define BEETLE_AUDIO_CODEC_REFERENCE_INPUT_CHANNELS 4U' "$codec_c" \
  || fail "ES7210 input-reference path must read all four TDM slots before selecting the active mic channel"

grep -q 'beetle_audio_codec_make_channel_mask(BEETLE_AUDIO_CODEC_REFERENCE_INPUT_CHANNELS)' "$codec_c" \
  || fail "ES7210 input-reference channel mask must cover the full reference input slot set"

grep -q 'codec->input_channels = config->input_reference ? BEETLE_AUDIO_CODEC_REFERENCE_INPUT_CHANNELS : 1' "$codec_c" \
  || fail "ES7210 input-reference read/deinterleave channel count must match the opened slot mask"

grep -q 'sample_count > SIZE_MAX / input_channels / sizeof(int16_t)' "$codec_c" \
  || fail "input-reference read buffer sizing must guard multiplication overflow"

grep -q 'buffered_samples = sample_count \* input_channels' "$codec_c" \
  || fail "input-reference read buffer must scale by the opened input channel count"

grep -q 'out_samples\[i\] = codec->input_read_buf\[i \* input_channels + selected_channel\]' "$codec_c" \
  || fail "input-reference deinterleave must use the selected input channel"

grep -q 'BEETLE_AUDIO_CODEC_CHANNEL_SWITCH_NUM' "$codec_c" \
  || fail "input-reference active channel selection must include hysteresis to avoid frame-to-frame slot flapping"

grep -q 'ch0_abs_pm=.*ch1_abs_pm=.*ch2_abs_pm=.*ch3_abs_pm=' "$codec_c" \
  || fail "input channel diagnostics must report all ES7210 reference slots"

printf 'PASS: esp audio codec ES7210 input-reference contract\n'
