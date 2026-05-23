#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
WAKENET_C="$ROOT_DIR/components/beetle_wakenet/beetle_wakenet.c"
WAKENET_H="$ROOT_DIR/components/beetle_wakenet/include/beetle_wakenet.h"
ESP_SR_RS="$ROOT_DIR/src/wake/esp_sr.rs"
SDKCONFIG_S3="$ROOT_DIR/sdkconfig.defaults.esp32s3"
SDKCONFIG_P4="$ROOT_DIR/sdkconfig.defaults.esp32p4"

fail() {
  echo "esp_wakenet_afe_contract_test: $*" >&2
  exit 1
}

require_contains() {
  local file="$1"
  local needle="$2"
  local message="$3"
  grep -Fq "$needle" "$file" || fail "$message"
}

require_not_contains() {
  local file="$1"
  local needle="$2"
  local message="$3"
  if grep -Fq "$needle" "$file"; then
    fail "$message"
  fi
}

require_contains "$WAKENET_H" \
  "beetle_wakenet_take_event" \
  "WakeNet C ABI must expose a nonblocking take-event entry point"

require_contains "$WAKENET_H" \
  "beetle_wakenet_set_threshold" \
  "WakeNet C ABI must expose the ESP-SR AFE threshold setter"

require_contains "$WAKENET_H" \
  "beetle_wakenet_reset_threshold" \
  "WakeNet C ABI must expose the ESP-SR AFE threshold reset entry point"

require_contains "$ESP_SR_RS" \
  "beetle_wakenet_take_event" \
  "Rust WakeNet backend must consume C-side pending detection via nonblocking take-event"

require_contains "$ESP_SR_RS" \
  "beetle_wakenet_set_threshold" \
  "Rust WakeNet backend must bind the ESP-SR AFE threshold setter"

require_contains "$ESP_SR_RS" \
  "ESP_SR_WAKENET_THRESHOLD_PROFILE" \
  "Rust WakeNet backend must define an ESP-SR threshold profile instead of using acoustic fallback thresholds"

require_contains "$ESP_SR_RS" \
  "pub const ESP_SR_WAKE_PHRASE: &str = \"Hi 乐鑫\"" \
  "ESP-SR WakeNet phrase must be fixed to Hi Lexin for the current A/B firmware"

require_contains "$ESP_SR_RS" \
  "pub const ESP_SR_WAKENET_MODEL: &str = \"wn9_hilexin\"" \
  "ESP-SR WakeNet model must be fixed to wn9_hilexin for the current A/B firmware"

require_contains "$WAKENET_C" \
  "fetch_with_delay(ctx->afe_data, portMAX_DELAY)" \
  "WakeNet detection must use a blocking AFE fetch task instead of polling in the feed hot path"

require_contains "$WAKENET_C" \
  "BEETLE_WN_INPUT_FORMAT_WITH_SECONDARY \"MMR\"" \
  "ESP-SR AFE wake input with a secondary codec mic must use two mic inputs plus a zero playback reference"

require_contains "$WAKENET_C" \
  "afe_config->aec_init = false" \
  "WakeNet arming must not treat the secondary ES7210 input as an AEC playback reference"

require_contains "$WAKENET_C" \
  "afe_config->wakenet_mode = DET_MODE_95" \
  "ESP-SR AFE WakeNet must keep the last empirically triggered aggressive detection mode until the secondary-mic feed shape is proven"

require_contains "$WAKENET_C" \
  "s_ctx->feed_frame[index + 2] = 0" \
  "WakeNet feed must zero-fill the playback reference slot in MMR mode"

require_contains "$WAKENET_C" \
  "feed16k window chunks=%u frames=%llu mic0_avg_pm=%u mic0_peak_pm=%u mic1_avg_pm=%u mic1_peak_pm=%u ref_avg_pm=%u ref_peak_pm=%u" \
  "WakeNet feed diagnostics must report pre-AFE window average and peak levels"

require_contains "$WAKENET_C" \
  "s_ctx->afe->set_wakenet_threshold(s_ctx->afe_data, index, threshold)" \
  "WakeNet threshold setter must call the official ESP-SR AFE ABI"

require_contains "$WAKENET_C" \
  "s_ctx->afe->reset_wakenet_threshold(s_ctx->afe_data, index)" \
  "WakeNet threshold reset must call the official ESP-SR AFE ABI"

require_contains "$WAKENET_C" \
  "BEETLE_WN_THRESHOLD_MIN 0.4f" \
  "WakeNet threshold range must include the official lower bound"

require_contains "$WAKENET_C" \
  "BEETLE_WN_THRESHOLD_MAX 0.9999f" \
  "WakeNet threshold range must include the official upper bound"

require_contains "$WAKENET_C" \
  "!isfinite(threshold)" \
  "WakeNet threshold validation must reject NaN values"

require_contains "$WAKENET_C" \
  "WakeNet threshold apply failed" \
  "WakeNet threshold failures must be logged for analyzer diagnosis"

require_contains "$WAKENET_C" \
  "WakeNet fetch detected wakeup_state=%d wake_word_index=%d wakenet_model_index=%d trigger_channel_id=%d wake_word_length=%d data_volume=%.2f ringbuff_free_pct=%.3f" \
  "WakeNet fetch diagnostics must expose the ESP-SR detection result fields"

require_contains "$ESP_SR_RS" \
  "crate::config::audio_uses_es7210_codec_input(audio)" \
  "Rust WakeNet backend must derive secondary ES7210 input from codec topology, not speaker.enabled"

require_contains "$ESP_SR_RS" \
  "record_wake_word_acoustic_frame" \
  "ESP-SR WakeNet backend must update audio_wake PCM metrics instead of leaving mic_level_pm at the default"

require_not_contains "$WAKENET_C" \
  "fetch_with_delay(s_ctx->afe_data, 0)" \
  "WakeNet feed path must not poll AFE with zero-delay fetch"

require_not_contains "$WAKENET_C" \
  "return poll_detection()" \
  "WakeNet feed path must not call detection polling while filling feed chunks"

require_not_contains "$WAKENET_C" \
  "== BEETLE_WN_DETECTED" \
  "beetle_wakenet_feed must not synchronously return detection from feed hot path"

require_not_contains "$WAKENET_C" \
  "afe_config->wakenet_mode = DET_MODE_2CH_90" \
  "ESP-SR AFE WakeNet must not use the normal two-channel mode after missed-wake evidence"

require_not_contains "$WAKENET_C" \
  "afe_config->wakenet_mode = DET_MODE_2CH_95" \
  "ESP-SR AFE WakeNet must not use the two-channel aggressive mode after missed-wake evidence"

require_not_contains "$ESP_SR_RS" \
  "AudioWakeWordConfig" \
  "ESP-SR WakeNet threshold must not reuse the acoustic wake config type"

require_not_contains "$ESP_SR_RS" \
  "enter_threshold" \
  "ESP-SR WakeNet threshold must not reuse acoustic enter_threshold"

require_contains "$SDKCONFIG_S3" \
  "CONFIG_SR_WN_WN9_HILEXIN=y" \
  "S3 sdkconfig must package only the Hi Lexin WakeNet model"

require_contains "$SDKCONFIG_P4" \
  "CONFIG_SR_WN_WN9_HILEXIN=y" \
  "P4 sdkconfig must package only the Hi Lexin WakeNet model"

require_not_contains "$SDKCONFIG_S3" \
  "CONFIG_SR_WN_WN9_HIESP" \
  "S3 sdkconfig must not drift back to the Hi ESP WakeNet model"

require_not_contains "$SDKCONFIG_P4" \
  "CONFIG_SR_WN_WN9_HIESP" \
  "P4 sdkconfig must not drift back to the Hi ESP WakeNet model"

require_not_contains "$SDKCONFIG_S3" \
  "CONFIG_SR_WN_WN9_XIAOAITONGXUE" \
  "S3 sdkconfig must not drift back to the XiaoAiTongXue A/B model"

require_not_contains "$SDKCONFIG_P4" \
  "CONFIG_SR_WN_WN9_XIAOAITONGXUE" \
  "P4 sdkconfig must not drift back to the XiaoAiTongXue A/B model"

require_not_contains "$ESP_SR_RS" \
  "wn9_hiesp" \
  "Rust WakeNet constants must not drift back to the Hi ESP model"

require_not_contains "$ESP_SR_RS" \
  "wn9_xiaoaitongxue" \
  "Rust WakeNet constants must not drift back to the XiaoAiTongXue A/B model"

echo "esp_wakenet_afe_contract_test: ok"
