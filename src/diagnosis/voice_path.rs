use crate::diagnosis::{
    DiagnosisAction, DiagnosisConfidence, DiagnosisDegradation, DiagnosisEvidence,
    DiagnosisFinding, DiagnosisKind, DiagnosisResult, DiagnosisRootCause,
};
use crate::metrics::MetricsSnapshot;
use crate::orchestrator::{RuntimeCapabilityState, RuntimeCapabilityStatus};
use crate::platform::AudioDuplexCapabilities;

pub struct VoicePathDiagnosisInput {
    pub voice_compiled: bool,
    pub audio_enabled: bool,
    pub wake_word_enabled: bool,
    pub realtime_enabled: bool,
    pub audio_duplex_capabilities: AudioDuplexCapabilities,
    pub runtime_capabilities: Vec<RuntimeCapabilityState>,
    pub metrics: MetricsSnapshot,
    pub audio_recording: bool,
    pub audio_playing: bool,
    pub voice_exclusive_active: bool,
}

pub fn build_voice_path_diagnosis(input: VoicePathDiagnosisInput) -> DiagnosisResult {
    let mut findings = Vec::new();
    let mut suspected_root_causes = Vec::new();
    let mut recommended_next_steps = Vec::new();
    let mut degraded_by = Vec::new();
    let profile = input.audio_duplex_capabilities.profile();
    let mut evidence = vec![
        DiagnosisEvidence::new("voice_compiled", input.voice_compiled.to_string()),
        DiagnosisEvidence::new("audio_enabled", input.audio_enabled.to_string()),
        DiagnosisEvidence::new("wake_word_enabled", input.wake_word_enabled.to_string()),
        DiagnosisEvidence::new("realtime_enabled", input.realtime_enabled.to_string()),
        DiagnosisEvidence::new("audio_profile", profile.as_str()),
        DiagnosisEvidence::new(
            "microphone_input",
            input.audio_duplex_capabilities.microphone_input.to_string(),
        ),
        DiagnosisEvidence::new(
            "speaker_output",
            input.audio_duplex_capabilities.speaker_output.to_string(),
        ),
        DiagnosisEvidence::new(
            "audio_recording",
            input.audio_recording.to_string(),
        ),
        DiagnosisEvidence::new("audio_playing", input.audio_playing.to_string()),
        DiagnosisEvidence::new(
            "voice_exclusive_active",
            input.voice_exclusive_active.to_string(),
        ),
        DiagnosisEvidence::new(
            "voice_input_fail_total",
            input.metrics.voice_input_fail_total.to_string(),
        ),
        DiagnosisEvidence::new(
            "voice_output_fail_total",
            input.metrics.voice_output_fail_total.to_string(),
        ),
        DiagnosisEvidence::new(
            "voice_input_stt_http_last_ms",
            input.metrics.voice_input_stt_http_last_ms.to_string(),
        ),
        DiagnosisEvidence::new(
            "voice_output_tts_http_last_ms",
            input.metrics.voice_output_tts_http_last_ms.to_string(),
        ),
        DiagnosisEvidence::new(
            "voice_interrupt_request_total",
            input.metrics.voice_interrupt_request_total.to_string(),
        ),
        DiagnosisEvidence::new(
            "voice_interrupt_accept_total",
            input.metrics.voice_interrupt_accept_total.to_string(),
        ),
        DiagnosisEvidence::new(
            "voice_no_speech_timeout_total",
            input.metrics.voice_no_speech_timeout_total.to_string(),
        ),
        DiagnosisEvidence::new(
            "voice_response_wait_timeout_total",
            input.metrics.voice_response_wait_timeout_total.to_string(),
        ),
        DiagnosisEvidence::new(
            "voice_post_playback_timeout_total",
            input.metrics.voice_post_playback_timeout_total.to_string(),
        ),
        DiagnosisEvidence::new(
            "wake_word_trigger_total",
            input.metrics.wake_word_trigger_total.to_string(),
        ),
    ];
    let mut confidence = DiagnosisConfidence::Medium;
    let mut summary = "The current voice path snapshot looks generally healthy.".to_string();

    if !input.voice_compiled {
        findings.push(DiagnosisFinding::observed(
            "voice capability is not compiled into the current runtime",
        ));
        suspected_root_causes.push(DiagnosisRootCause::new(
            "voice_capability_not_compiled",
            "voice features are unavailable because this build does not include voice capability",
            DiagnosisConfidence::High,
        ));
        recommended_next_steps.push(DiagnosisAction::new(
            "inspect_build_package",
            "inspect the current build package before expecting voice behavior",
        ));
        summary = "Voice capability is not compiled into the current runtime.".to_string();
        confidence = DiagnosisConfidence::High;
    } else if !input.audio_enabled {
        findings.push(DiagnosisFinding::observed(
            "audio is currently disabled in runtime configuration",
        ));
        suspected_root_causes.push(DiagnosisRootCause::new(
            "audio_disabled",
            "voice path is inactive because audio is disabled in configuration",
            DiagnosisConfidence::High,
        ));
        recommended_next_steps.push(DiagnosisAction::new(
            "inspect_audio_config",
            "inspect audio configuration before expecting wake-word or TTS behavior",
        ));
        summary = "Voice capability is compiled, but audio is disabled in configuration.".to_string();
        confidence = DiagnosisConfidence::High;
    }

    if input.realtime_enabled && !input.audio_duplex_capabilities.can_run_realtime_session() {
        findings.push(DiagnosisFinding::observed(
            "realtime voice is configured but the current audio contract cannot run a realtime session",
        ));
        suspected_root_causes.push(DiagnosisRootCause::new(
            "realtime_duplex_contract_missing",
            "realtime voice requires both microphone and speaker availability, but the current audio contract is incomplete",
            DiagnosisConfidence::High,
        ));
        recommended_next_steps.push(DiagnosisAction::new(
            "inspect_audio_contract",
            "inspect microphone and speaker runtime availability before retrying realtime voice",
        ));
        summary =
            "Realtime voice is configured, but the current audio contract cannot run a realtime session."
                .to_string();
        confidence = DiagnosisConfidence::High;
    }

    if input.audio_enabled && !input.audio_duplex_capabilities.microphone_input {
        findings.push(DiagnosisFinding::correlated(
            "the current audio contract has no microphone input",
        ));
        suspected_root_causes.push(DiagnosisRootCause::new(
            "audio_input_unavailable",
            "microphone input is unavailable, so capture and wake-word flows cannot run normally",
            DiagnosisConfidence::High,
        ));
        recommended_next_steps.push(DiagnosisAction::new(
            "inspect_audio_input",
            "inspect microphone runtime capability and hardware contract before blaming ASR or wake word",
        ));
        if confidence != DiagnosisConfidence::High {
            summary = "The current voice path is missing microphone input.".to_string();
            confidence = DiagnosisConfidence::High;
        }
    }

    if input.audio_enabled && !input.audio_duplex_capabilities.speaker_output {
        findings.push(DiagnosisFinding::correlated(
            "the current audio contract has no speaker output",
        ));
        suspected_root_causes.push(DiagnosisRootCause::new(
            "audio_output_unavailable",
            "speaker output is unavailable, so local playback and TTS delivery cannot complete normally",
            DiagnosisConfidence::High,
        ));
        recommended_next_steps.push(DiagnosisAction::new(
            "inspect_audio_output",
            "inspect speaker runtime capability and playback contract before blaming TTS generation",
        ));
        if confidence != DiagnosisConfidence::High {
            summary = "The current voice path is missing speaker output.".to_string();
            confidence = DiagnosisConfidence::High;
        }
    }

    if input.metrics.voice_input_fail_total > 0 || input.metrics.voice_output_fail_total > 0 {
        findings.push(DiagnosisFinding::observed(
            "voice input/output failures were recorded recently",
        ));
        suspected_root_causes.push(DiagnosisRootCause::new(
            "voice_io_failures",
            "recent voice input or output failures indicate instability somewhere in the speech pipeline",
            DiagnosisConfidence::High,
        ));
        recommended_next_steps.push(DiagnosisAction::new(
            "inspect_voice_metrics",
            "inspect voice metrics and recent input/output failure counters before retrying voice tasks",
        ));
        if summary == "The current voice path snapshot looks generally healthy." {
            summary = "Recent voice pipeline failures were recorded in the current runtime.".to_string();
            confidence = DiagnosisConfidence::High;
        }
    }

    if input.metrics.voice_no_speech_timeout_total > 0
        || input.metrics.voice_response_wait_timeout_total > 0
        || input.metrics.voice_post_playback_timeout_total > 0
    {
        findings.push(DiagnosisFinding::correlated(
            "voice session timeouts were recorded recently",
        ));
        suspected_root_causes.push(DiagnosisRootCause::new(
            "voice_session_timeouts",
            "recent timeout counters indicate that capture, response wait, or post-playback closure is stalling",
            DiagnosisConfidence::Medium,
        ));
        recommended_next_steps.push(DiagnosisAction::new(
            "inspect_voice_session_timeouts",
            "inspect voice timeout counters before assuming the model or audio hardware is the only problem",
        ));
    }

    if input.voice_exclusive_active {
        findings.push(DiagnosisFinding::observed(
            "runtime is currently in voice-exclusive mode",
        ));
    }

    for capability in input.runtime_capabilities {
        match capability.id {
            crate::orchestrator::RUNTIME_CAPABILITY_AUDIO_INPUT => match capability.status {
                RuntimeCapabilityStatus::Offline => {
                    degraded_by.push(DiagnosisDegradation::new(
                        capability.id,
                        "runtime capability audio_input is offline",
                    ));
                    suspected_root_causes.push(DiagnosisRootCause::new(
                        "runtime_audio_input_offline",
                        "the runtime has marked audio input as offline",
                        DiagnosisConfidence::High,
                    ));
                    evidence.push(DiagnosisEvidence::new(
                        "runtime_capability.audio_input.status",
                        "offline",
                    ));
                }
                RuntimeCapabilityStatus::Degraded => {
                    degraded_by.push(DiagnosisDegradation::new(
                        capability.id,
                        "runtime capability audio_input is degraded",
                    ));
                    suspected_root_causes.push(DiagnosisRootCause::new(
                        "runtime_audio_input_degraded",
                        "the runtime has marked audio input as degraded",
                        DiagnosisConfidence::Medium,
                    ));
                    evidence.push(DiagnosisEvidence::new(
                        "runtime_capability.audio_input.status",
                        "degraded",
                    ));
                }
                RuntimeCapabilityStatus::Online => {
                    evidence.push(DiagnosisEvidence::new(
                        "runtime_capability.audio_input.status",
                        "online",
                    ));
                }
            },
            crate::orchestrator::RUNTIME_CAPABILITY_AUDIO_OUTPUT => match capability.status {
                RuntimeCapabilityStatus::Offline => {
                    degraded_by.push(DiagnosisDegradation::new(
                        capability.id,
                        "runtime capability audio_output is offline",
                    ));
                    suspected_root_causes.push(DiagnosisRootCause::new(
                        "runtime_audio_output_offline",
                        "the runtime has marked audio output as offline",
                        DiagnosisConfidence::High,
                    ));
                    evidence.push(DiagnosisEvidence::new(
                        "runtime_capability.audio_output.status",
                        "offline",
                    ));
                }
                RuntimeCapabilityStatus::Degraded => {
                    degraded_by.push(DiagnosisDegradation::new(
                        capability.id,
                        "runtime capability audio_output is degraded",
                    ));
                    suspected_root_causes.push(DiagnosisRootCause::new(
                        "runtime_audio_output_degraded",
                        "the runtime has marked audio output as degraded",
                        DiagnosisConfidence::Medium,
                    ));
                    evidence.push(DiagnosisEvidence::new(
                        "runtime_capability.audio_output.status",
                        "degraded",
                    ));
                }
                RuntimeCapabilityStatus::Online => {
                    evidence.push(DiagnosisEvidence::new(
                        "runtime_capability.audio_output.status",
                        "online",
                    ));
                }
            },
            _ => {}
        }
    }

    DiagnosisResult {
        kind: DiagnosisKind::VoicePath,
        summary,
        findings,
        suspected_root_causes,
        recommended_next_steps,
        evidence,
        confidence,
        degraded_by,
        safe_actions_available: vec![
            "inspect_audio_config".to_string(),
            "inspect_audio_contract".to_string(),
            "inspect_audio_input".to_string(),
            "inspect_audio_output".to_string(),
            "inspect_voice_metrics".to_string(),
            "inspect_runtime_capabilities".to_string(),
        ],
    }
}

pub fn build_voice_path_diagnosis_from_runtime(
    platform: &dyn crate::Platform,
    config: &crate::config::AppConfig,
) -> DiagnosisResult {
    let audio_config = config.audio.as_ref();
    let audio_enabled = audio_config.is_some_and(|audio| audio.enabled);
    let wake_word_enabled = audio_config.is_some_and(|audio| audio.enabled && audio.wake_word.enabled);
    let realtime_enabled = audio_config.is_some_and(crate::config::audio_realtime_enabled);
    let presence = crate::runtime::inspect_platform_presence(platform, crate::util::current_unix_secs());
    build_voice_path_diagnosis(VoicePathDiagnosisInput {
        voice_compiled: crate::compiled_voice_capability(),
        audio_enabled,
        wake_word_enabled,
        realtime_enabled,
        audio_duplex_capabilities: platform.audio_duplex_capabilities(),
        runtime_capabilities: crate::orchestrator::runtime_capability_snapshot(),
        metrics: crate::metrics::snapshot(),
        audio_recording: presence.audio_recording,
        audio_playing: presence.audio_playing,
        voice_exclusive_active: presence.runtime_mode.voice_exclusive_active,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::orchestrator::{RuntimeCapabilityReason, RuntimeCapabilityState};

    #[test]
    fn voice_path_diagnosis_marks_missing_realtime_duplex_contract() {
        let diagnosis = build_voice_path_diagnosis(VoicePathDiagnosisInput {
            voice_compiled: true,
            audio_enabled: true,
            wake_word_enabled: true,
            realtime_enabled: true,
            audio_duplex_capabilities: AudioDuplexCapabilities::speaker_only(),
            runtime_capabilities: Vec::new(),
            metrics: crate::metrics::snapshot(),
            audio_recording: false,
            audio_playing: false,
            voice_exclusive_active: false,
        });

        assert_eq!(diagnosis.kind, DiagnosisKind::VoicePath);
        assert!(diagnosis
            .suspected_root_causes
            .iter()
            .any(|cause| cause.code == "realtime_duplex_contract_missing"));
    }

    #[test]
    fn voice_path_diagnosis_reports_voice_failures_and_runtime_degradation() {
        let mut metrics = crate::metrics::snapshot();
        metrics.voice_input_fail_total = 1;
        metrics.voice_output_fail_total = 2;
        metrics.voice_no_speech_timeout_total = 1;
        let diagnosis = build_voice_path_diagnosis(VoicePathDiagnosisInput {
            voice_compiled: true,
            audio_enabled: true,
            wake_word_enabled: false,
            realtime_enabled: false,
            audio_duplex_capabilities: AudioDuplexCapabilities::duplex_without_aec(),
            runtime_capabilities: vec![
                RuntimeCapabilityState {
                    id: crate::orchestrator::RUNTIME_CAPABILITY_AUDIO_INPUT,
                    status: RuntimeCapabilityStatus::Degraded,
                    reason: RuntimeCapabilityReason::DeviceDisconnected,
                    epoch: 1,
                    changed_at_secs: 1,
                    observed_at_secs: 1,
                    recovery_hint: Some("wait_for_audio_input_recovery"),
                },
                RuntimeCapabilityState {
                    id: crate::orchestrator::RUNTIME_CAPABILITY_AUDIO_OUTPUT,
                    status: RuntimeCapabilityStatus::Offline,
                    reason: RuntimeCapabilityReason::DeviceDisconnected,
                    epoch: 1,
                    changed_at_secs: 1,
                    observed_at_secs: 1,
                    recovery_hint: Some("wait_for_audio_output_recovery"),
                },
            ],
            metrics,
            audio_recording: false,
            audio_playing: false,
            voice_exclusive_active: false,
        });

        assert!(diagnosis
            .suspected_root_causes
            .iter()
            .any(|cause| cause.code == "voice_io_failures"));
        assert!(diagnosis
            .suspected_root_causes
            .iter()
            .any(|cause| cause.code == "voice_session_timeouts"));
        assert!(!diagnosis.degraded_by.is_empty());
    }
}
