//! VoiceSink：将 dispatch 出站消息路由到 voice_session 线程进行 TTS 播报。
//! Routes outbound messages on the "voice" channel to the voice session thread for TTS playback.

use crate::audio::voice_session::VoiceEvent;
use crate::error::{Error, Result};
use std::sync::mpsc::SyncSender;

use super::MessageSink;

pub struct VoiceSink {
    tx: SyncSender<VoiceEvent>,
}

impl VoiceSink {
    pub fn new(tx: SyncSender<VoiceEvent>) -> Self {
        Self { tx }
    }
}

impl MessageSink for VoiceSink {
    fn send(&self, _chat_id: &str, content: &str) -> Result<()> {
        self.tx
            .try_send(VoiceEvent::Speak(content.to_string()))
            .map_err(|e| Error::Other {
                source: Box::new(e),
                stage: "voice_sink",
            })
    }
}
