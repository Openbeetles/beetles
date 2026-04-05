//! Linux USB speaker runtime backed by ALSA playback.
//! 基于 ALSA playback 的 Linux USB 喇叭运行时。

use crate::config::AudioSegment;
use crate::error::{Error, Result};

const STAGE: &str = "audio_init";

#[cfg(target_os = "linux")]
mod imp {
    use super::{AudioSegment, Error, Result, STAGE};
    use crate::util::{HttpThreadRole, SpawnCore, TaskHandle, spawn_guarded_with_profile_handle};
    use alsa::pcm::{Access, Format, HwParams, PCM, State};
    use alsa::{Direction, ValueOr};
    use std::sync::Arc;
    use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
    use std::sync::mpsc::{self, Receiver, SyncSender};

    use super::super::hardware_discovery::resolve_usb_audio_output_device;

    const SPEAKER_QUEUE_CAPACITY: usize = 32;
    const STACK_LINUX_AUDIO_SPEAKER: usize = 8192;

    enum SpeakerWorkerMsg {
        Samples {
            generation: usize,
            samples: Vec<i16>,
        },
        Clear {
            generation: usize,
        },
        Stop,
    }

    pub(crate) struct LinuxSpeakerRuntime {
        tx: SyncSender<SpeakerWorkerMsg>,
        buffered_samples: Arc<AtomicUsize>,
        generation: Arc<AtomicUsize>,
        ready: Arc<AtomicBool>,
        selected_label: String,
        worker: Option<TaskHandle>,
    }

    impl LinuxSpeakerRuntime {
        pub fn from_config(config: &AudioSegment) -> Result<Self> {
            if !config.enabled || !config.speaker.enabled {
                return Err(Error::config(STAGE, "speaker is disabled"));
            }
            if config.speaker.device_type != "usb" {
                return Err(Error::config(
                    STAGE,
                    format!(
                        "Linux speaker only supports device_type 'usb' (got '{}')",
                        config.speaker.device_type
                    ),
                ));
            }
            if config.speaker.bits_per_sample != 16 {
                return Err(Error::config(
                    STAGE,
                    format!(
                        "Linux USB speaker currently requires 16-bit PCM output (got {})",
                        config.speaker.bits_per_sample
                    ),
                ));
            }

            let resolved = resolve_usb_audio_output_device(config.speaker.device_ref.as_deref())?;
            let pcm_name = resolved
                .playback_pcm
                .clone()
                .ok_or_else(|| Error::config(STAGE, "resolved USB device has no playback PCM"))?;
            let sample_rate = config.speaker.sample_rate.max(8_000);
            let selected_label = resolved.label.clone();

            let (tx, rx) = mpsc::sync_channel(SPEAKER_QUEUE_CAPACITY);
            let (init_tx, init_rx) = mpsc::sync_channel(1);
            let buffered_samples = Arc::new(AtomicUsize::new(0));
            let generation = Arc::new(AtomicUsize::new(0));
            let ready = Arc::new(AtomicBool::new(false));
            let worker_buffered = Arc::clone(&buffered_samples);
            let worker_generation = Arc::clone(&generation);
            let worker_ready = Arc::clone(&ready);
            let worker_name = format!("linux_audio_speaker({})", selected_label);

            let worker = spawn_guarded_with_profile_handle(
                "linux_audio_speaker",
                STACK_LINUX_AUDIO_SPEAKER,
                Some(SpawnCore::Core0),
                HttpThreadRole::Background,
                move || {
                    speaker_worker_loop(
                        worker_name.as_str(),
                        pcm_name.as_str(),
                        sample_rate,
                        init_tx,
                        rx,
                        worker_buffered,
                        worker_generation,
                        worker_ready,
                    )
                },
            )
            .map_err(|e| {
                Error::config(STAGE, format!("spawn linux audio speaker failed: {}", e))
            })?;

            match init_rx.recv() {
                Ok(Ok(())) => {}
                Ok(Err(message)) => {
                    let _ = worker.join();
                    return Err(Error::config(STAGE, message));
                }
                Err(error) => {
                    let _ = worker.join();
                    return Err(Error::config(
                        STAGE,
                        format!("linux speaker init handshake failed: {}", error),
                    ));
                }
            }

            Ok(Self {
                tx,
                buffered_samples,
                generation,
                ready,
                selected_label,
                worker: Some(worker),
            })
        }

        pub fn ready(&self) -> bool {
            self.ready.load(Ordering::Relaxed)
        }

        pub fn selected_label(&self) -> &str {
            self.selected_label.as_str()
        }

        pub fn buffered_samples(&self) -> usize {
            self.buffered_samples.load(Ordering::Relaxed)
        }

        pub fn write_pcm_i16(&self, buf: &[i16]) -> Result<()> {
            if !self.ready() {
                return Err(Error::config(
                    "audio_speaker",
                    "Linux USB speaker not ready",
                ));
            }
            let owned = buf.to_vec();
            let generation = self.generation.load(Ordering::Relaxed);
            self.buffered_samples
                .fetch_add(owned.len(), Ordering::Relaxed);
            if let Err(error) = self.tx.send(SpeakerWorkerMsg::Samples {
                generation,
                samples: owned,
            }) {
                subtract_buffered_samples(self.buffered_samples.as_ref(), buf.len());
                self.ready.store(false, Ordering::Relaxed);
                return Err(Error::Other {
                    source: Box::new(error),
                    stage: "audio_speaker",
                });
            }
            Ok(())
        }

        pub fn try_write_pcm_i16(&self, buf: &[i16]) -> Result<usize> {
            if !self.ready() {
                return Err(Error::config(
                    "audio_speaker",
                    "Linux USB speaker not ready",
                ));
            }
            if buf.is_empty() {
                return Ok(0);
            }
            let owned = buf.to_vec();
            let generation = self.generation.load(Ordering::Relaxed);
            match self.tx.try_send(SpeakerWorkerMsg::Samples {
                generation,
                samples: owned,
            }) {
                Ok(()) => {
                    self.buffered_samples
                        .fetch_add(buf.len(), Ordering::Relaxed);
                    Ok(buf.len())
                }
                Err(std::sync::mpsc::TrySendError::Full(_)) => Ok(0),
                Err(std::sync::mpsc::TrySendError::Disconnected(_)) => {
                    self.ready.store(false, Ordering::Relaxed);
                    Err(Error::config(
                        "audio_speaker",
                        "Linux USB speaker worker disconnected",
                    ))
                }
            }
        }

        pub fn clear_buffer(&self) -> Result<()> {
            if !self.ready() {
                return Err(Error::config(
                    "audio_speaker",
                    "Linux USB speaker not ready",
                ));
            }
            let generation = self.generation.fetch_add(1, Ordering::Relaxed) + 1;
            self.buffered_samples.store(0, Ordering::Relaxed);
            self.tx
                .send(SpeakerWorkerMsg::Clear { generation })
                .map_err(|error| Error::Other {
                    source: Box::new(error),
                    stage: "audio_speaker",
                })
        }
    }

    impl Drop for LinuxSpeakerRuntime {
        fn drop(&mut self) {
            let _ = self.tx.send(SpeakerWorkerMsg::Stop);
            self.ready.store(false, Ordering::Relaxed);
            if let Some(handle) = self.worker.take() {
                let _ = handle.join();
            }
        }
    }

    fn speaker_worker_loop(
        worker_name: &str,
        pcm_name: &str,
        sample_rate: u32,
        init_tx: SyncSender<std::result::Result<(), String>>,
        rx: Receiver<SpeakerWorkerMsg>,
        buffered_samples: Arc<AtomicUsize>,
        generation: Arc<AtomicUsize>,
        ready: Arc<AtomicBool>,
    ) {
        let result = (|| -> Result<()> {
            let (pcm, channels) = match open_playback_pcm(pcm_name, sample_rate) {
                Ok(v) => {
                    let _ = init_tx.send(Ok(()));
                    ready.store(true, Ordering::Relaxed);
                    v
                }
                Err(error) => {
                    let _ = init_tx.send(Err(error.to_string()));
                    return Err(error);
                }
            };
            let io = pcm.io_i16().map_err(map_other("audio_speaker"))?;
            let mut stereo_buf = Vec::<i16>::new();
            let mut active_generation = generation.load(Ordering::Relaxed);

            while let Ok(msg) = rx.recv() {
                match msg {
                    SpeakerWorkerMsg::Samples {
                        generation,
                        samples,
                    } => {
                        if generation != active_generation {
                            continue;
                        }
                        let sample_count = samples.len();
                        let write_buf: &[i16];
                        if channels == 1 {
                            write_buf = samples.as_slice();
                        } else {
                            stereo_buf.clear();
                            stereo_buf.reserve(samples.len().saturating_mul(channels));
                            for sample in &samples {
                                for _ in 0..channels {
                                    stereo_buf.push(*sample);
                                }
                            }
                            write_buf = stereo_buf.as_slice();
                        }
                        if let Err(error) = write_all_frames(&pcm, &io, write_buf, channels) {
                            ready.store(false, Ordering::Relaxed);
                            subtract_buffered_samples(buffered_samples.as_ref(), sample_count);
                            return Err(error);
                        }
                        subtract_buffered_samples(buffered_samples.as_ref(), sample_count);
                    }
                    SpeakerWorkerMsg::Clear { generation } => {
                        active_generation = generation;
                        buffered_samples.store(0, Ordering::Relaxed);
                        pcm.drop().map_err(map_other("audio_speaker"))?;
                        pcm.prepare().map_err(map_other("audio_speaker"))?;
                    }
                    SpeakerWorkerMsg::Stop => break,
                }
            }
            Ok(())
        })();

        if let Err(error) = result {
            ready.store(false, Ordering::Relaxed);
            log::warn!(
                "[{}] linux speaker worker stopped with error: {}",
                worker_name,
                error
            );
        } else {
            ready.store(false, Ordering::Relaxed);
            log::info!("[{}] linux speaker worker stopped", worker_name);
        }
    }

    fn open_playback_pcm(pcm_name: &str, sample_rate: u32) -> Result<(PCM, usize)> {
        let pcm = PCM::new(pcm_name, Direction::Playback, false).map_err(map_other(STAGE))?;
        let channels = {
            let hwp = HwParams::any(&pcm).map_err(map_other(STAGE))?;
            hwp.set_access(Access::RWInterleaved)
                .map_err(map_other(STAGE))?;
            hwp.set_format(Format::s16()).map_err(map_other(STAGE))?;

            let channels = if hwp.set_channels(1).is_ok() { 1 } else { 2 };
            if channels == 2 {
                hwp.set_channels(2).map_err(map_other(STAGE))?;
            }
            hwp.set_rate(sample_rate, ValueOr::Nearest)
                .map_err(map_other(STAGE))?;
            pcm.hw_params(&hwp).map_err(map_other(STAGE))?;
            channels
        };
        pcm.prepare().map_err(map_other(STAGE))?;
        Ok((pcm, channels))
    }

    fn write_all_frames(
        pcm: &PCM,
        io: &alsa::pcm::IO<i16>,
        buf: &[i16],
        channels: usize,
    ) -> Result<()> {
        let mut offset = 0usize;
        while offset < buf.len() {
            let frames = &buf[offset..];
            match io.writei(frames) {
                Ok(written_frames) => {
                    if written_frames == 0 {
                        return Err(Error::config(
                            "audio_speaker",
                            "ALSA write returned 0 frames",
                        ));
                    }
                    offset = offset.saturating_add(written_frames.saturating_mul(channels));
                }
                Err(error) => {
                    let errno = error.errno();
                    if errno == libc::EPIPE || errno == libc::ESTRPIPE {
                        pcm.prepare().map_err(map_other("audio_speaker"))?;
                        continue;
                    }
                    if pcm.state() == State::XRun {
                        pcm.prepare().map_err(map_other("audio_speaker"))?;
                        continue;
                    }
                    return Err(Error::Other {
                        source: Box::new(error),
                        stage: "audio_speaker",
                    });
                }
            }
        }
        Ok(())
    }

    fn map_other<E>(stage: &'static str) -> impl Fn(E) -> Error
    where
        E: std::error::Error + Send + Sync + 'static,
    {
        move |error| Error::Other {
            source: Box::new(error),
            stage,
        }
    }

    fn subtract_buffered_samples(counter: &AtomicUsize, samples: usize) {
        let _ = counter.fetch_update(Ordering::Relaxed, Ordering::Relaxed, |current| {
            Some(current.saturating_sub(samples))
        });
    }
}

#[cfg(not(target_os = "linux"))]
mod imp {
    use super::{AudioSegment, Error, Result, STAGE};

    pub(crate) struct LinuxSpeakerRuntime;

    impl LinuxSpeakerRuntime {
        pub fn from_config(_config: &AudioSegment) -> Result<Self> {
            Err(Error::config(
                STAGE,
                "Linux USB speaker runtime is only available on target_os=linux",
            ))
        }

        pub fn ready(&self) -> bool {
            false
        }

        pub fn selected_label(&self) -> &str {
            "unsupported"
        }

        pub fn buffered_samples(&self) -> usize {
            0
        }

        pub fn write_pcm_i16(&self, _buf: &[i16]) -> Result<()> {
            Err(Error::config(
                "audio_speaker",
                "Linux USB speaker runtime is only available on target_os=linux",
            ))
        }

        pub fn try_write_pcm_i16(&self, _buf: &[i16]) -> Result<usize> {
            Err(Error::config(
                "audio_speaker",
                "Linux USB speaker runtime is only available on target_os=linux",
            ))
        }

        pub fn clear_buffer(&self) -> Result<()> {
            Err(Error::config(
                "audio_speaker",
                "Linux USB speaker runtime is only available on target_os=linux",
            ))
        }
    }
}

pub(crate) use imp::LinuxSpeakerRuntime;
