//! Realtime provider turn contracts.
//! provider 回合语义集中真源，避免 realtime loop 继续散落 provider 判断。

use crate::audio::endpoint_profile::VoiceEndpointOwner;
use crate::config::{
    AUDIO_REALTIME_PROVIDER_DOUBAO, AUDIO_REALTIME_PROVIDER_OPENAI_COMPATIBLE,
    AUDIO_REALTIME_PROVIDER_QWEN,
};
use crate::error::{Error, Result};

const REALTIME_TAG: &str = "audio::realtime";

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum RealtimeProvider {
    OpenAiCompatible,
    Qwen,
    Doubao,
}

impl RealtimeProvider {
    pub(crate) fn parse(raw: &str) -> Result<Self> {
        match raw {
            AUDIO_REALTIME_PROVIDER_OPENAI_COMPATIBLE => Ok(Self::OpenAiCompatible),
            AUDIO_REALTIME_PROVIDER_QWEN => Ok(Self::Qwen),
            AUDIO_REALTIME_PROVIDER_DOUBAO => Ok(Self::Doubao),
            _ => Err(Error::config(
                REALTIME_TAG,
                format!("unsupported realtime provider: {}", raw),
            )),
        }
    }

    pub(crate) fn input_audio_format(self) -> &'static str {
        match self {
            Self::OpenAiCompatible => "pcm16",
            Self::Qwen => "pcm",
            Self::Doubao => "pcm16",
        }
    }

    pub(crate) fn output_audio_format(self) -> &'static str {
        match self {
            Self::OpenAiCompatible => "pcm16",
            Self::Qwen => "pcm",
            Self::Doubao => "pcm16",
        }
    }

    pub(crate) fn requires_openai_beta_header(self) -> bool {
        matches!(self, Self::OpenAiCompatible)
    }

    pub(crate) fn session_created_is_ready(self) -> bool {
        matches!(self, Self::Doubao)
    }

    pub(crate) fn turn_contract(self) -> RealtimeProviderTurnContract {
        RealtimeProviderTurnContract::for_provider(self)
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct RealtimeProviderTurnContract {
    pub endpoint_owner: VoiceEndpointOwner,
    pub requires_client_commit: bool,
    pub accepts_server_speech_events_without_local_commit: bool,
    pub accepts_response_without_local_commit: bool,
    pub sends_response_create_on_client_commit: bool,
}

impl RealtimeProviderTurnContract {
    pub fn for_provider(provider: RealtimeProvider) -> Self {
        match provider {
            RealtimeProvider::OpenAiCompatible | RealtimeProvider::Qwen => Self {
                endpoint_owner: VoiceEndpointOwner::ServerVad,
                requires_client_commit: false,
                accepts_server_speech_events_without_local_commit: true,
                accepts_response_without_local_commit: true,
                sends_response_create_on_client_commit: false,
            },
            RealtimeProvider::Doubao => Self {
                endpoint_owner: VoiceEndpointOwner::ClientCommit,
                requires_client_commit: true,
                accepts_server_speech_events_without_local_commit: false,
                accepts_response_without_local_commit: false,
                sends_response_create_on_client_commit: true,
            },
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn server_vad_providers_accept_response_without_local_commit() {
        for provider in [RealtimeProvider::OpenAiCompatible, RealtimeProvider::Qwen] {
            let contract = RealtimeProviderTurnContract::for_provider(provider);

            assert_eq!(contract.endpoint_owner, VoiceEndpointOwner::ServerVad);
            assert!(!contract.requires_client_commit);
            assert!(contract.accepts_server_speech_events_without_local_commit);
            assert!(contract.accepts_response_without_local_commit);
        }
    }

    #[test]
    fn doubao_remains_client_commit_authoritative() {
        let contract = RealtimeProviderTurnContract::for_provider(RealtimeProvider::Doubao);

        assert_eq!(contract.endpoint_owner, VoiceEndpointOwner::ClientCommit);
        assert!(contract.requires_client_commit);
        assert!(contract.sends_response_create_on_client_commit);
    }
}
