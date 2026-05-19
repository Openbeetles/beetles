#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
cd "$ROOT"

if ! command -v rg >/dev/null 2>&1; then
  echo "esp_realtime_voice_contract_test: ripgrep (rg) is required" >&2
  exit 1
fi

prod_source() {
  sed '/^#\[cfg(test)\]/,$d' "$1"
}

fail() {
  echo "FAIL: $*" >&2
  exit 1
}

require_file() {
  [[ -f "$1" ]] || fail "$1 is required by the realtime voice closure contract"
}

require_file src/audio/wake_handoff.rs
require_file src/audio/input_profile.rs
require_file src/audio/endpoint_profile.rs
require_file src/audio/realtime_provider.rs
require_file src/audio/voice_conversation.rs

rg -n 'pub struct WakeAudioHandoff' src/audio/wake_handoff.rs >/dev/null ||
  fail "WakeAudioHandoff must be a concrete handoff type"
rg -n 'pub struct AudioInputHardwareProfile' src/audio/input_profile.rs >/dev/null ||
  fail "AudioInputHardwareProfile must be the input hardware truth source"
rg -n 'pub struct VoiceEndpointProfile' src/audio/endpoint_profile.rs >/dev/null ||
  fail "VoiceEndpointProfile must be the endpoint truth source"
rg -n 'pub struct RealtimeProviderTurnContract' src/audio/realtime_provider.rs >/dev/null ||
  fail "RealtimeProviderTurnContract must centralize provider turn semantics"
rg -n 'pub struct VoiceConversationController' src/audio/voice_conversation.rs >/dev/null ||
  fail "VoiceConversationController must own realtime voice conversation observability"
rg -n 'pub enum NoSpeechExitReason' src/audio/voice_conversation.rs >/dev/null ||
  fail "NoSpeechExitReason must classify realtime voice no-speech exits"

if prod_source src/audio/voice_session.rs | rg -n 'WakeTriggered\s*,' >/dev/null; then
  prod_source src/audio/voice_session.rs | rg -n 'WakeTriggered\s*,' >&2
  fail "VoiceEvent::WakeTriggered must carry WakeAudioHandoff, not remain a unit variant"
fi

rg -n 'VoiceEvent::WakeTriggered\(' src/wake/mod.rs src/audio/voice_session.rs >/dev/null ||
  fail "wake handoff must be passed through VoiceEvent::WakeTriggered(handoff)"

if prod_source src/audio/realtime.rs |
  sed -n '/fn build_turn_detection/,/^}/p' |
  rg -n 'audio_cfg\.vad\.threshold' >/dev/null; then
  prod_source src/audio/realtime.rs |
    sed -n '/fn build_turn_detection/,/^}/p' |
    rg -n 'audio_cfg\.vad\.threshold' >&2
  fail "build_turn_detection must consume VoiceEndpointProfile instead of reading audio_cfg.vad.threshold directly"
fi

if prod_source src/audio/realtime.rs |
  sed -n '/"response\.created"/,/"input_audio_buffer\.speech_started"/p' |
  rg -n 'has_committed_local_turn' >/dev/null; then
  prod_source src/audio/realtime.rs |
    sed -n '/"response\.created"/,/"input_audio_buffer\.speech_started"/p' |
    rg -n 'has_committed_local_turn' >&2
  fail "server VAD providers must not hard-gate response.created on local commit"
fi

if prod_source src/audio/realtime.rs |
  sed -n '/"response\.output_audio\.delta" \| "response\.audio\.delta"/,/"response\.done"/p' |
  rg -n 'has_committed_local_turn' >/dev/null; then
  prod_source src/audio/realtime.rs |
    sed -n '/"response\.output_audio\.delta" \| "response\.audio\.delta"/,/"response\.done"/p' |
    rg -n 'has_committed_local_turn' >&2
  fail "server audio deltas must be accepted according to RealtimeProviderTurnContract, not a global local-commit gate"
fi

VOICE_PROD_FILES="src/audio/voice_session.rs src/audio/realtime.rs src/audio/realtime_provider.rs"
if rg -n 'ESP_BOX|esp_box|esp-box|EspBox|esp32s3|esp32-s3|esp32_s3|board_name|BOARD_' $VOICE_PROD_FILES >/dev/null; then
  rg -n 'ESP_BOX|esp_box|esp-box|EspBox|esp32s3|esp32-s3|esp32_s3|board_name|BOARD_' $VOICE_PROD_FILES >&2
  fail "voice session/realtime/provider contract must not hard-code concrete board names"
fi

if rg -n 'porcupine|picovoice|snowboy|webrtcvad|external_voice_frontend|external_wake_runtime' Cargo.toml src/audio src/wake >/dev/null; then
  rg -n 'porcupine|picovoice|snowboy|webrtcvad|external_voice_frontend|external_wake_runtime' Cargo.toml src/audio src/wake >&2
  fail "realtime voice closure must not add an external wake/audio frontend runtime"
fi

rg -n 'realtime turn summary' src/audio/voice_session.rs src/audio/voice_conversation.rs >/dev/null ||
  fail "voice session must emit a realtime turn summary with NoSpeechExitReason"

echo "OK: ESP realtime voice contract checks passed"
