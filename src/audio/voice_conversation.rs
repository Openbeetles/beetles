//! Realtime voice conversation state and diagnostics.
//! 只记录语音会话状态与观测，不拥有 WSS、硬件或调度资源。

use crate::audio::wake_handoff::WakeAudioHandoff;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum VoiceConversationState {
    Idle,
    WakePrimed,
    Connecting,
    Listening,
    AwaitingResponse,
    Speaking,
    Cooldown,
}

impl VoiceConversationState {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Idle => "idle",
            Self::WakePrimed => "wake_primed",
            Self::Connecting => "connecting",
            Self::Listening => "listening",
            Self::AwaitingResponse => "awaiting_response",
            Self::Speaking => "speaking",
            Self::Cooldown => "cooldown",
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum NoSpeechExitReason {
    NoHandoffAudio,
    HandoffUploadedNoLocalOrServerSpeech,
    LocalSpeechBelowProfile,
    ServerSpeechNoResponse,
    ResponseWithoutAudio,
    ProviderNoTurnEvents,
}

impl NoSpeechExitReason {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::NoHandoffAudio => "no_handoff_audio",
            Self::HandoffUploadedNoLocalOrServerSpeech => {
                "handoff_uploaded_no_local_or_server_speech"
            }
            Self::LocalSpeechBelowProfile => "local_speech_below_profile",
            Self::ServerSpeechNoResponse => "server_speech_no_response",
            Self::ResponseWithoutAudio => "response_without_audio",
            Self::ProviderNoTurnEvents => "provider_no_turn_events",
        }
    }
}

#[derive(Clone, Debug, Default)]
pub struct VoiceTurnState {
    pub wake_handoff_id: u32,
    pub pre_roll_ms: u32,
    pub post_wake_ms: u32,
    pub local_speech_ms: u32,
    pub server_speech_started: bool,
    pub server_speech_stopped: bool,
    pub response_created: bool,
    pub output_audio_ms: u128,
}

#[derive(Clone, Debug)]
pub struct VoiceConversationController {
    state: VoiceConversationState,
    turn: VoiceTurnState,
    handoff_uploaded: bool,
    turns: u32,
    input_ms: u128,
    no_speech_reason: Option<NoSpeechExitReason>,
}

impl Default for VoiceConversationController {
    fn default() -> Self {
        Self::new()
    }
}

impl VoiceConversationController {
    pub fn new() -> Self {
        Self {
            state: VoiceConversationState::Idle,
            turn: VoiceTurnState::default(),
            handoff_uploaded: false,
            turns: 0,
            input_ms: 0,
            no_speech_reason: None,
        }
    }

    pub fn new_without_handoff() -> Self {
        Self::new()
    }

    pub fn wake_primed(handoff: &WakeAudioHandoff) -> Self {
        let mut controller = Self::new();
        controller.state = VoiceConversationState::WakePrimed;
        controller.turn.wake_handoff_id = handoff.id;
        controller.turn.pre_roll_ms = handoff.pre_roll_ms;
        controller.turn.post_wake_ms = handoff.post_wake_ms;
        if handoff.is_empty() {
            controller.no_speech_reason = Some(NoSpeechExitReason::NoHandoffAudio);
        }
        controller
    }

    pub fn mark_wake_primed(&mut self) {
        self.state = VoiceConversationState::WakePrimed;
    }

    pub fn mark_connecting(&mut self) {
        self.state = VoiceConversationState::Connecting;
    }

    pub fn mark_listening(&mut self) {
        self.state = VoiceConversationState::Listening;
    }

    pub fn mark_handoff_uploaded(&mut self, handoff: &WakeAudioHandoff) {
        self.handoff_uploaded = !handoff.is_empty();
        self.turn.wake_handoff_id = handoff.id;
        self.turn.pre_roll_ms = handoff.pre_roll_ms;
        self.turn.post_wake_ms = handoff.post_wake_ms;
        self.state = VoiceConversationState::Listening;
    }

    pub fn mark_local_speech_ms(&mut self, ms: u32) {
        self.turn.local_speech_ms = self.turn.local_speech_ms.max(ms);
        self.input_ms = self.input_ms.max(ms as u128);
    }

    pub fn record_local_speech_ms(&mut self, ms: u32) {
        self.mark_local_speech_ms(ms);
    }

    pub fn record_input_audio_ms(&mut self, ms: u128) {
        self.input_ms = ms;
        self.turn.local_speech_ms = ms.min(u32::MAX as u128) as u32;
    }

    pub fn mark_server_speech_started(&mut self) {
        self.turn.server_speech_started = true;
        self.state = VoiceConversationState::AwaitingResponse;
    }

    pub fn mark_server_speech_stopped(&mut self) {
        self.turn.server_speech_stopped = true;
        self.state = VoiceConversationState::AwaitingResponse;
    }

    pub fn mark_response_created(&mut self) {
        self.turn.response_created = true;
        self.state = VoiceConversationState::AwaitingResponse;
    }

    pub fn mark_output_audio_ms(&mut self, output_audio_ms: u128) {
        self.turn.output_audio_ms = self.turn.output_audio_ms.max(output_audio_ms);
        if output_audio_ms > 0 {
            self.state = VoiceConversationState::Speaking;
        }
    }

    pub fn record_output_audio_ms(&mut self, output_audio_ms: u128) {
        self.mark_output_audio_ms(output_audio_ms);
    }

    pub fn mark_turn_completed(&mut self) {
        self.turns = self.turns.saturating_add(1);
    }

    pub fn mark_cooldown(&mut self) {
        self.state = VoiceConversationState::Cooldown;
    }

    pub fn finish_completed(&mut self) {
        self.no_speech_reason = None;
        self.mark_cooldown();
    }

    pub fn finish_no_speech(&mut self, reason: NoSpeechExitReason) {
        self.no_speech_reason = Some(reason);
        self.mark_cooldown();
    }

    pub fn classify_no_speech(&mut self, turns_completed: u32) -> Option<NoSpeechExitReason> {
        if turns_completed > 0 {
            self.no_speech_reason = None;
            return None;
        }
        let reason = if !self.handoff_uploaded {
            NoSpeechExitReason::NoHandoffAudio
        } else if !self.turn.server_speech_started && self.turn.local_speech_ms == 0 {
            NoSpeechExitReason::HandoffUploadedNoLocalOrServerSpeech
        } else if self.turn.local_speech_ms > 0 && !self.turn.response_created {
            NoSpeechExitReason::LocalSpeechBelowProfile
        } else if self.turn.server_speech_started && !self.turn.response_created {
            NoSpeechExitReason::ServerSpeechNoResponse
        } else if self.turn.response_created && self.turn.output_audio_ms == 0 {
            NoSpeechExitReason::ResponseWithoutAudio
        } else {
            NoSpeechExitReason::ProviderNoTurnEvents
        };
        self.no_speech_reason = Some(reason);
        Some(reason)
    }

    pub fn no_speech_reason(&self) -> Option<NoSpeechExitReason> {
        self.no_speech_reason
    }

    pub fn summary_line(
        &self,
        turns: u32,
        input_ms: u128,
        output_ms: u128,
        duration_ms: u128,
    ) -> String {
        format!(
            "realtime turn summary state={} wake_handoff_id={} wake_pre_roll_ms={} post_wake_ms={} local_speech_ms={} server_speech_started={} server_speech_stopped={} response_created={} turns={} input_ms={} output_ms={} duration_ms={} no_speech_reason={}",
            self.state.as_str(),
            self.turn.wake_handoff_id,
            self.turn.pre_roll_ms,
            self.turn.post_wake_ms,
            self.turn.local_speech_ms,
            self.turn.server_speech_started,
            self.turn.server_speech_stopped,
            self.turn.response_created,
            turns,
            input_ms,
            output_ms,
            duration_ms,
            self.no_speech_reason
                .map(NoSpeechExitReason::as_str)
                .unwrap_or("none")
        )
    }

    pub fn summary(&self) -> VoiceTurnSummary {
        VoiceTurnSummary {
            state: self.state,
            wake_handoff_id: self.turn.wake_handoff_id,
            wake_pre_roll_ms: self.turn.pre_roll_ms,
            post_wake_ms: self.turn.post_wake_ms,
            local_speech_ms: self.turn.local_speech_ms,
            server_speech_started: self.turn.server_speech_started,
            server_speech_stopped: self.turn.server_speech_stopped,
            response_created: self.turn.response_created,
            turns: self.turns,
            input_ms: self.input_ms,
            output_ms: self.turn.output_audio_ms,
            no_speech_reason: self.no_speech_reason,
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct VoiceTurnSummary {
    pub state: VoiceConversationState,
    pub wake_handoff_id: u32,
    pub wake_pre_roll_ms: u32,
    pub post_wake_ms: u32,
    pub local_speech_ms: u32,
    pub server_speech_started: bool,
    pub server_speech_stopped: bool,
    pub response_created: bool,
    pub turns: u32,
    pub input_ms: u128,
    pub output_ms: u128,
    pub no_speech_reason: Option<NoSpeechExitReason>,
}

impl VoiceTurnSummary {
    pub fn to_log_fields(&self) -> String {
        format!(
            "state={} wake_handoff_id={} wake_pre_roll_ms={} post_wake_ms={} local_speech_ms={} server_speech_started={} server_speech_stopped={} response_created={} turns={} input_ms={} output_ms={} no_speech_reason={}",
            self.state.as_str(),
            self.wake_handoff_id,
            self.wake_pre_roll_ms,
            self.post_wake_ms,
            self.local_speech_ms,
            self.server_speech_started,
            self.server_speech_stopped,
            self.response_created,
            self.turns,
            self.input_ms,
            self.output_ms,
            self.no_speech_reason
                .map(NoSpeechExitReason::as_str)
                .unwrap_or("none")
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::audio::wake_handoff::{WakeAcousticSnapshot, WakeAudioHandoff};
    use crate::platform::byte_buffer::ByteBuffer;
    use std::sync::Arc;

    fn handoff(bytes: usize) -> WakeAudioHandoff {
        WakeAudioHandoff {
            id: 7,
            sample_rate_hz: 16_000,
            channels: 1,
            pre_roll_ms: 250,
            post_wake_ms: 0,
            pcm_le_bytes: Arc::new(ByteBuffer::zeroed(bytes)),
            acoustic: WakeAcousticSnapshot::default(),
        }
    }

    #[test]
    fn empty_handoff_classifies_no_handoff_audio() {
        let mut controller = VoiceConversationController::wake_primed(&handoff(0));
        controller.mark_handoff_uploaded(&handoff(0));

        assert_eq!(
            controller.classify_no_speech(0),
            Some(NoSpeechExitReason::NoHandoffAudio)
        );
    }

    #[test]
    fn server_speech_without_response_is_classified() {
        let mut controller = VoiceConversationController::wake_primed(&handoff(128));
        controller.mark_handoff_uploaded(&handoff(128));
        controller.mark_server_speech_started();

        assert_eq!(
            controller.classify_no_speech(0),
            Some(NoSpeechExitReason::ServerSpeechNoResponse)
        );
        assert!(controller
            .summary_line(0, 10, 0, 100)
            .contains("no_speech_reason=server_speech_no_response"));
    }
}
