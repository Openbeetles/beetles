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
  sed -n '/fn should_trigger_playback_interrupt/,/^}/p' |
  rg -n 'audio_cfg\.vad\.threshold|REALTIME_INTERRUPT_THRESHOLD_MIN' >/dev/null; then
  prod_source src/audio/realtime.rs |
    sed -n '/fn should_trigger_playback_interrupt/,/^}/p' |
    rg -n 'audio_cfg\.vad\.threshold|REALTIME_INTERRUPT_THRESHOLD_MIN' >&2
  fail "playback interrupt must consume VoiceEndpointProfile instead of global VAD constants"
fi
rg -n 'fn handle_local_endpoint_event' src/audio/realtime.rs >/dev/null ||
  fail "server-VAD providers must route local endpoint activity through a provider-aware owner helper"
rg -n 'requires_client_commit' src/audio/realtime.rs >/dev/null ||
  fail "local endpoint commit/response-wait ownership must follow RealtimeProviderTurnContract"
rg -n 'fn should_force_close_local_speech_window' src/audio/realtime.rs >/dev/null ||
  fail "local speech hard caps must be centralized behind provider turn ownership"
LOCAL_WINDOW_CAP_BLOCK="$(sed -n '/fn should_force_close_local_speech_window/,/^}/p' src/audio/realtime.rs)"
printf '%s\n' "$LOCAL_WINDOW_CAP_BLOCK" |
  rg -n 'requires_client_commit' >/dev/null ||
  fail "server-VAD providers must not be force-closed by the client-commit local speech hard cap"
printf '%s\n' "$LOCAL_WINDOW_CAP_BLOCK" |
  rg -n 'REALTIME_LOCAL_SPEECH_WINDOW_MAX_MS' >/dev/null ||
  fail "client-commit providers must retain the local speech hard cap"
if prod_source src/audio/realtime.rs | rg -n 'force closing long local speech window' >/dev/null; then
  prod_source src/audio/realtime.rs | rg -n 'force closing long local speech window' >&2
  fail "server-VAD local activity must not use the old provider-agnostic 12s forced-close log path"
fi
rg -n 'fn should_hold_local_capture_for_server_response' src/audio/realtime.rs >/dev/null ||
  fail "half-duplex realtime must hold local capture as soon as a server response starts"
rg -n 'fn should_suppress_server_turn_during_half_duplex_output' src/audio/realtime.rs >/dev/null ||
  fail "half-duplex realtime must suppress stale server turns while prior output is still playing"
if prod_source src/platform/abstraction.rs |
  sed -n '/pub const fn duplex_with_input_reference/,/^    }/p' |
  rg -n 'barge_in:\s*true' >/dev/null; then
  prod_source src/platform/abstraction.rs |
    sed -n '/pub const fn duplex_with_input_reference/,/^    }/p' |
    rg -n 'barge_in:\s*true' >&2
  fail "InputReference without platform AEC must not claim safe local barge-in"
fi
if ! prod_source src/platform/abstraction.rs |
  sed -n '/pub fn normalized/,/pub const fn has_microphone_input/p' |
  rg -n 'AudioReferenceCapability::InputReference' >/dev/null; then
  fail "duplex capability normalization must explicitly guard InputReference without platform AEC"
fi

rg -n 'REALTIME_SERVER_VAD_POST_RESPONSE_IDLE_TIMEOUT_MS' src/audio/realtime.rs >/dev/null ||
  fail "server-VAD realtime providers must have a longer post-response local idle fallback"
rg -n 'realtime server event type=' src/audio/realtime.rs >/dev/null ||
  fail "realtime voice must log key provider server events without audio payloads"
rg -n 'realtime audio downlink summary' src/audio/realtime.rs >/dev/null ||
  fail "realtime voice must summarize downlink delta/queue playback evidence"
rg -n 'struct RealtimeOutputTurn' src/audio/realtime.rs >/dev/null ||
  fail "realtime output audio must have an explicit RealtimeOutputTurn owner"
rg -n 'active_output_turn' src/audio/realtime.rs >/dev/null ||
  fail "realtime output generation fence must be tied to an active output turn"
rg -n 'stale_server_audio_drop_total' src/audio/realtime.rs >/dev/null ||
  fail "stale realtime output audio drops must be counted for analyzer visibility"
rg -n 'fn should_accept_server_audio_event' src/audio/realtime.rs >/dev/null ||
  fail "server audio deltas must pass through a response generation fence before queueing"
rg -n 'fn can_begin_output_turn' src/audio/realtime.rs >/dev/null ||
  fail "new realtime responses must be fenced until the previous output turn is drained or cancelled"
rg -n 'fn has_pending_output_turn' src/audio/realtime.rs >/dev/null ||
  fail "half-duplex server VAD suppression must treat unfinished output turns as pending even across playback underruns"
rg -n 'not_before_drained_at' src/audio/realtime.rs >/dev/null ||
  fail "realtime output turn drain must include an accepted-PCM duration tail, not only software queue counters"
rg -n 'fn samples_to_duration' src/audio/realtime.rs >/dev/null ||
  fail "realtime output drain must convert accepted PCM samples into a playback-duration fence"
SUPPRESS_TURN_BLOCK="$(
  sed -n '/fn should_suppress_server_turn_during_half_duplex_output/,/^}/p' src/audio/realtime.rs
)"
printf '%s\n' "$SUPPRESS_TURN_BLOCK" |
  rg -n 'has_pending_output_turn\(now\)' >/dev/null ||
  fail "half-duplex server VAD suppression must not rely only on audio_playing; active output turns remain pending until the accepted PCM duration drains"
REFRESH_DRAIN_BLOCK="$(
  sed -n '/fn refresh_output_drain_state/,/fn append_audio_frame/p' src/audio/realtime.rs
)"
printf '%s\n' "$REFRESH_DRAIN_BLOCK" |
  rg -n 'active_output_duration_pending\(now\)' >/dev/null ||
  fail "queue-empty refresh must not finish playback before the accepted PCM duration tail has elapsed"
DROP_STALE_BLOCK="$(sed -n '/fn drop_server_audio_for_stale_turn/,/fn should_accept_server_audio_event/p' src/audio/realtime.rs)"
printf '%s\n' "$DROP_STALE_BLOCK" |
  rg -n 'stale_server_audio_drop_total' >/dev/null ||
  fail "drop_server_audio_for_stale_turn must count stale response/audio drops"
AUDIO_DELTA_BLOCK="$(
  sed -n '/"response\.output_audio\.delta" \| "response\.audio\.delta"/,/"response\.output_audio\.done" \| "response\.audio\.done"/p' src/audio/realtime.rs
)"
printf '%s\n' "$AUDIO_DELTA_BLOCK" |
  rg -n 'should_accept_server_audio_event' >/dev/null ||
  fail "response audio deltas must be discarded unless they match the active output generation"
AUDIO_DONE_BLOCK="$(
  sed -n '/"response\.output_audio\.done" \| "response\.audio\.done"/,/"response\.done"/p' src/audio/realtime.rs
)"
printf '%s\n' "$AUDIO_DONE_BLOCK" |
  rg -n 'force_empty_summary|true' >/dev/null ||
  fail "response.audio.done must force a downlink summary even when no audio delta arrived"
RESPONSE_DONE_BLOCK="$(
  sed -n '/"response\.done"/,/"error"/p' src/audio/realtime.rs
)"
if printf '%s\n' "$RESPONSE_DONE_BLOCK" |
  rg -n 'suppress_server_audio_until_turn_end\s*=\s*false' >/dev/null; then
  printf '%s\n' "$RESPONSE_DONE_BLOCK" |
    rg -n 'suppress_server_audio_until_turn_end\s*=\s*false' >&2
  fail "stale response.done must not clear half-duplex suppression before the active output turn drains"
fi
QUEUE_OUTPUT_BLOCK="$(
  sed -n '/fn queue_output_audio/,/fn start_realtime_playback_before_queue_write/p' src/audio/realtime.rs
)"
if rg -n 'try_write_speaker_pcm_i16' <<<"$QUEUE_OUTPUT_BLOCK" >/dev/null; then
  rg -n 'try_write_speaker_pcm_i16' <<<"$QUEUE_OUTPUT_BLOCK" >&2
  fail "realtime downlink must apply speaker backpressure instead of dropping PCM when queues are full"
fi
if rg -n 'write_speaker_pcm_i16' <<<"$QUEUE_OUTPUT_BLOCK" >/dev/null; then
  rg -n 'write_speaker_pcm_i16' <<<"$QUEUE_OUTPUT_BLOCK" >&2
  fail "realtime downlink must keep a single FIFO path through staging instead of bypassing directly to speaker"
fi
rg -n 'push_output_staging_with_backpressure' src/audio/realtime.rs >/dev/null ||
  fail "realtime downlink must backpressure on the staging FIFO instead of bypassing it"
STAGING_TRANSFER_BLOCK="$(
  sed -n '/staging .* speaker transfer/,/pop_speaker_frame_for_output/p' src/platform/audio_drivers.rs
)"
printf '%s\n' "$STAGING_TRANSFER_BLOCK" |
  rg -n 'spk\.available\(\)' >/dev/null ||
  fail "audio worker must check speaker-ring capacity before popping staging PCM"
printf '%s\n' "$STAGING_TRANSFER_BLOCK" |
  rg -n 'debug_assert_eq!\(pushed, staging_popped\)' >/dev/null ||
  fail "audio worker staging transfer must prove it never drops popped PCM when the speaker ring is full"
rg -n 'REALTIME_INTERRUPT_LOW_SNR_THRESHOLD_MIN: f32 = 0\.006' src/audio/realtime.rs >/dev/null ||
  fail "ES7210 low-SNR playback interrupt calibration must stay tied to observed ESP-BOX3 mic levels"
rg -n 'REALTIME_INTERRUPT_LOW_SNR_SPEECH_MIN_MS' src/audio/realtime.rs >/dev/null ||
  fail "ES7210 low-SNR playback interrupt calibration must have its own speech duration gate"
rg -n 'format_audio_speaker_baseline_line' src/metrics.rs src/heartbeat/mod.rs >/dev/null ||
  fail "heartbeat must expose speaker queue/underrun diagnostics for realtime choppiness"
rg -n 'start_realtime_playback_before_queue_write' src/audio/realtime.rs >/dev/null ||
  fail "realtime speaker output must mark playback active before queued PCM can hit hardware"
rg -n 'REALTIME_FOREGROUND_KEEPALIVE_MS' src/audio/realtime.rs >/dev/null ||
  fail "long realtime conversations must renew their runtime foreground ticket before it expires"
rg -n 'fn renew_now' src/audio/voice_session.rs >/dev/null ||
  fail "voice realtime foreground ownership must be renewable during long conversations"

rg -n 'pub const AUDIO_REALTIME_QWEN_PCM_SAMPLE_RATE: u32 = 16_000;' src/config.rs >/dev/null ||
  fail "Qwen realtime upload PCM target must remain 16kHz"
if prod_source src/config.rs |
  sed -n '/fn audio_realtime_required_sample_rate/,/^}/p' |
  rg -n 'AUDIO_REALTIME_QWEN_PCM_SAMPLE_RATE' >/dev/null; then
  prod_source src/config.rs |
    sed -n '/fn audio_realtime_required_sample_rate/,/^}/p' |
    rg -n 'AUDIO_REALTIME_QWEN_PCM_SAMPLE_RATE' >&2
  fail "hardware speaker/runtime sample-rate contract must not be collapsed to Qwen's 16k upload PCM target"
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

rg -n 'pub const STACK_VOICE_REALTIME_CONNECT: usize = 9 \* 1024;' src/util.rs >/dev/null ||
  fail "voice realtime connect stack must stay at the measured ESP WSS-class 9KB budget"
rg -n 'pub const STACK_VOICE_CONTROL: usize = 10 \* 1024;' src/util.rs >/dev/null ||
  fail "voice_session control stack must reserve the measured ESP-BOX3 low-margin headroom"

rg -n 'voice_realtime_connect_spawn_reserve_defer_ms' src/audio/voice_session.rs >/dev/null ||
  fail "voice realtime connect must run a real pre-spawn reserve gate before allocating its connect stack"
SPAWN_RESERVE_BLOCK="$(sed -n '/fn voice_realtime_connect_spawn_reserve_defer_ms/,/fn voice_realtime_startup_defer_ms/p' src/audio/voice_session.rs)"
printf '%s\n' "$SPAWN_RESERVE_BLOCK" |
  rg -n 'TLS_ADMISSION_MIN_LARGEST_BLOCK_BYTES' >/dev/null ||
  fail "voice realtime pre-spawn gate must reserve the connect stack plus TLS largest-block floor"
printf '%s\n' "$SPAWN_RESERVE_BLOCK" |
  rg -n 'saturating_add\(STACK_VOICE_REALTIME_CONNECT\)' >/dev/null ||
  fail "voice realtime pre-spawn gate must reserve the connect stack plus TLS largest-block floor"
printf '%s\n' "$SPAWN_RESERVE_BLOCK" |
  rg -n 'TLS_ADMISSION_MIN_INTERNAL_BYTES' >/dev/null ||
  fail "voice realtime pre-spawn gate must reserve the connect stack plus TLS internal-free floor"
printf '%s\n' "$SPAWN_RESERVE_BLOCK" |
  rg -n 'TLS_ADMISSION_NO_PSRAM_MIN_BYTES' >/dev/null ||
  fail "voice realtime pre-spawn gate must reserve the connect stack plus TLS internal-free floor"
printf '%s\n' "$SPAWN_RESERVE_BLOCK" |
  rg -n 'saturating_add\(STACK_VOICE_REALTIME_CONNECT\)' >/dev/null ||
  fail "voice realtime pre-spawn gate must reserve the connect stack plus TLS internal-free floor"

rg -n 'request_deferred_agent_loop_start' src/runtime/agent_supervision.rs src/runtime/mod.rs src/main.rs >/dev/null ||
  fail "ESP deferred agent loop must be startable by real inbound work instead of only startup readiness"
rg -n 'agent_loop_eager_start' src/main.rs >/dev/null ||
  fail "ESP agent loop startup must distinguish configured external channels from no-channel voice-only idle"
rg -n 'set_after_successful_send_hook' src/main.rs src/bus.rs >/dev/null ||
  fail "user/system inbound enqueue must wake deferred agent startup"

echo "OK: ESP realtime voice contract checks passed"
