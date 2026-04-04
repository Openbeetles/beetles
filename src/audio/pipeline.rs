//! 共享语音编排：收口采集+转写、TTS+播放两条主链，供 voice_session 与语音工具共用。
//! Shared voice pipeline helpers for capture/transcribe and TTS playback.

use crate::audio::baidu_token::BaiduTokenCache;
use crate::audio::capture::{capture_speech, AudioRecordingGuard};
use crate::audio::{stt_baidu, tts_baidu};
use crate::config::AudioSegment;
use crate::constants::AUDIO_TTS_WRITE_CHUNK_SAMPLES;
use crate::error::Result;
use crate::platform::PlatformHttpClient;
use crate::Platform;
use std::time::Instant;

pub struct VoicePlaybackStats {
    pub played_samples: usize,
    pub tts_http_ms: u128,
    pub play_ms: u128,
}

pub fn capture_and_transcribe(
    platform: &dyn Platform,
    audio_cfg: &AudioSegment,
    baidu_token: &BaiduTokenCache,
    http: &mut dyn PlatformHttpClient,
    max_ms: u32,
    log_tag: &'static str,
) -> Result<String> {
    let captured = {
        let _recording_guard = AudioRecordingGuard::new();
        capture_speech(platform, audio_cfg, max_ms, log_tag)?
    };
    let mic_sr = audio_cfg.microphone.sample_rate.max(8_000);
    let stt_start = Instant::now();
    let text = stt_baidu::transcribe_pcm16_samples(
        http,
        baidu_token,
        &audio_cfg.speech,
        captured.as_slice(),
        mic_sr,
    )?;
    crate::metrics::record_voice_input_stt_http_ms(stt_start.elapsed().as_millis());
    Ok(text)
}

pub fn speak_text(
    platform: &dyn Platform,
    audio_cfg: &AudioSegment,
    baidu_token: &BaiduTokenCache,
    http: &mut dyn PlatformHttpClient,
    text: &str,
) -> Result<VoicePlaybackStats> {
    let _recording_guard = AudioRecordingGuard::new();
    let tts_start = Instant::now();
    let mut first_pcm_at: Option<Instant> = None;
    crate::orchestrator::set_audio_playing(true);
    let result = (|| match tts_baidu::stream_wav_pcm16le(
        http,
        baidu_token,
        &audio_cfg.speech,
        &audio_cfg.tts,
        text,
        AUDIO_TTS_WRITE_CHUNK_SAMPLES,
        |chunk| {
            if first_pcm_at.is_none() {
                first_pcm_at = Some(Instant::now());
            }
            platform.write_speaker_pcm_i16(chunk)
        },
    ) {
        Ok(samples) => Ok(samples),
        Err(error) if error.stage() == "tts_baidu_wav" => {
            let wav = tts_baidu::synthesize_wav(
                http,
                baidu_token,
                &audio_cfg.speech,
                &audio_cfg.tts,
                text,
            )?;
            tts_baidu::play_wav_pcm16le_chunks(&wav, AUDIO_TTS_WRITE_CHUNK_SAMPLES, |chunk| {
                if first_pcm_at.is_none() {
                    first_pcm_at = Some(Instant::now());
                }
                platform.write_speaker_pcm_i16(chunk)
            })
        }
        Err(error) => Err(error),
    })();
    crate::orchestrator::set_audio_playing(false);
    let played_samples = result?;
    let total_ms = tts_start.elapsed().as_millis();
    let play_ms = first_pcm_at.map(|at| at.elapsed().as_millis()).unwrap_or(0);
    Ok(VoicePlaybackStats {
        played_samples,
        tts_http_ms: total_ms,
        play_ms,
    })
}
