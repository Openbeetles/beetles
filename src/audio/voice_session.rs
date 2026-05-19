//! 语音会话调度：主线程消费事件并串行执行语音任务。
//! Voice session scheduler: event intake stays responsive while voice tasks run one at a time.
//!
//! Architecture:
//! - `wake::feed_pcm_i16` pushes `WakeTriggered`
//! - `VoiceSink` pushes `Speak(text)`
//! - `run_voice_session` coalesces events and dispatches one task at a time
//! - Realtime wake interactions run on a dedicated transient `voice_realtime`
//!   worker so the always-on scheduler thread stays shallow and stable
//! - Non-realtime STT/TTS fallback still uses `voice_session_worker`

use crate::audio::baidu_token::BaiduTokenCache;
use crate::audio::pipeline::{
    acquire_audio_lease, capture_and_transcribe, speak_text, AudioLeaseGuard, AudioLeaseOwner,
};
use crate::audio::realtime::{
    connect_realtime_session, run_connected_realtime_session, ConnectedRealtimeSession,
};
use crate::bus::{PcMsg, UserInboundTx};
use crate::config::{audio_realtime_enabled, AudioSegment};
use crate::constants::{AUDIO_CAPTURE_MAX_MS, VOICE_CHANNEL_NAME, VOICE_DEVICE_CHAT_ID};
use crate::network::{HttpClientClass, NetworkGovernor, VoiceExclusiveTransportGuard};
use crate::platform::PlatformHttpClient;
use crate::util::{
    spawn_guarded_with_profile_handle, HttpThreadRole, SpawnCore, TaskHandle, STACK_VOICE_REALTIME,
    STACK_VOICE_REALTIME_CONNECT, STACK_VOICE_SESSION,
};
use crate::Platform;
use std::sync::mpsc::{self, Receiver, TryRecvError};
use std::sync::Arc;
use std::time::{Duration, Instant};

const TAG: &str = "voice_session";
const WORKER_IDLE_POLL_MS: u64 = 100;
const WORKER_SPAWN_FAILURE_COOLDOWN_MS: u64 = 5_000;
const MAX_PENDING_SPEAK_CHARS: usize = 512;

/// Events consumed by the voice session thread.
#[derive(Debug)]
pub enum VoiceEvent {
    /// Wake backend triggered — start capture + STT + inject to agent.
    WakeTriggered,
    /// Agent reply to speak aloud via TTS.
    Speak(String),
}

#[derive(Clone)]
enum VoiceWorkerTask {
    WakeInteraction,
    Speak(String),
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum VoiceWorkerStartKind {
    SpawnWorker,
    PrepareRealtimeTransportThenSpawnConnect,
}

enum VoiceWorkerStartResult {
    Started(TaskHandle),
    Deferred { retry_after_ms: u64 },
    Dropped,
}

#[cfg(test)]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum VoiceWorkerStartDisposition {
    RetryLater,
}

struct RealtimeSessionOwnership {
    _foreground_ticket: crate::runtime::RuntimeForegroundTicket,
    _audio_input_call: crate::orchestrator::RuntimeCapabilityCallGuard,
    _audio_output_call: crate::orchestrator::RuntimeCapabilityCallGuard,
    _audio_input_lease: AudioLeaseGuard,
    _audio_output_lease: AudioLeaseGuard,
    _voice_transport: VoiceExclusiveTransportGuard,
    _wake_reset: WakeSessionResetGuard,
}

struct PreparedRealtimeSession {
    connected: ConnectedRealtimeSession,
    _ownership: RealtimeSessionOwnership,
}

enum VoiceWorkerMessage {
    Done,
    RealtimePrepared(crate::Result<PreparedRealtimeSession>),
}

#[derive(Default)]
struct PendingVoiceEvents {
    wake_requested: bool,
    pending_speak: Option<String>,
}

#[derive(Default)]
struct VoiceWorkerRetryGate {
    retry_after: Option<Instant>,
}

impl VoiceWorkerRetryGate {
    fn can_retry(&self, now: Instant) -> bool {
        self.retry_after
            .is_none_or(|retry_after| retry_after <= now)
    }

    fn record_spawn_failure(&mut self, now: Instant) {
        self.retry_after = Some(now + Duration::from_millis(WORKER_SPAWN_FAILURE_COOLDOWN_MS));
    }

    fn record_scheduler_defer(&mut self, now: Instant, retry_after_ms: u64) {
        self.retry_after = Some(now + Duration::from_millis(retry_after_ms.max(1)));
    }

    fn clear(&mut self) {
        self.retry_after = None;
    }
}

/// All dependencies for the voice session thread, injected by `main`.
#[derive(Clone)]
pub struct VoiceSessionConfig {
    pub platform: Arc<dyn Platform>,
    pub network: Arc<NetworkGovernor>,
    pub audio_cfg: AudioSegment,
    pub baidu_token: Option<Arc<BaiduTokenCache>>,
    pub inbound_tx: UserInboundTx,
    pub wake_prompt: String,
}

/// Entry point for the voice session scheduler thread. Blocks on `rx` until the channel closes.
pub fn run_voice_session(cfg: VoiceSessionConfig, rx: Receiver<VoiceEvent>) {
    log::info!(
        "[{}] started realtime_enabled={}",
        TAG,
        audio_realtime_enabled(&cfg.audio_cfg)
    );

    let (worker_tx, worker_rx) = mpsc::channel::<VoiceWorkerMessage>();
    let mut worker_busy = false;
    let mut worker_handle: Option<TaskHandle> = None;
    let mut pending = PendingVoiceEvents::default();
    let mut retry_gate = VoiceWorkerRetryGate::default();

    loop {
        crate::platform::task_wdt::feed_current_task();
        drain_worker_messages(
            &cfg,
            &worker_tx,
            &mut worker_handle,
            &worker_rx,
            &mut worker_busy,
        );
        let now = Instant::now();
        if !worker_busy && retry_gate.can_retry(now) {
            if let Some(task) = take_pending_voice_task(&mut pending) {
                match spawn_voice_session_worker(cfg.clone(), task.clone(), worker_tx.clone()) {
                    Ok(VoiceWorkerStartResult::Started(handle)) => {
                        retry_gate.clear();
                        worker_handle = Some(handle);
                        worker_busy = true;
                        continue;
                    }
                    Ok(VoiceWorkerStartResult::Deferred { retry_after_ms }) => {
                        retry_gate.record_scheduler_defer(now, retry_after_ms);
                        restore_pending_voice_task(&mut pending, task);
                        continue;
                    }
                    Ok(VoiceWorkerStartResult::Dropped) => {
                        retry_gate.clear();
                        continue;
                    }
                    Err(error) => {
                        retry_gate.record_spawn_failure(now);
                        log::error!(
                            "[{}] failed to start worker: {}; retry suppressed for {}ms",
                            TAG,
                            error,
                            WORKER_SPAWN_FAILURE_COOLDOWN_MS
                        );
                        restore_pending_voice_task(&mut pending, task);
                    }
                }
            }
        }

        let next_event = match rx.try_recv() {
            Ok(event) => Some(event),
            Err(TryRecvError::Empty) => {
                std::thread::sleep(Duration::from_millis(WORKER_IDLE_POLL_MS));
                None
            }
            Err(TryRecvError::Disconnected) => break,
        };

        let Some(event) = next_event else {
            continue;
        };
        handle_voice_event(event, &mut pending);
    }

    if let Some(handle) = worker_handle.take() {
        let _ = handle.join();
    }
    log::info!("[{}] scheduler stopped", TAG);
}

fn spawn_voice_session_worker(
    cfg: VoiceSessionConfig,
    task: VoiceWorkerTask,
    worker_tx: mpsc::Sender<VoiceWorkerMessage>,
) -> crate::Result<VoiceWorkerStartResult> {
    let realtime_enabled = audio_realtime_enabled(&cfg.audio_cfg);
    match voice_worker_scheduler_decision(realtime_enabled, &task) {
        crate::runtime::RuntimeWorkDecision::Proceed
        | crate::runtime::RuntimeWorkDecision::Degrade { .. } => {}
        crate::runtime::RuntimeWorkDecision::Defer { retry_after_ms, .. }
        | crate::runtime::RuntimeWorkDecision::DrainAndResume { retry_after_ms, .. } => {
            return Ok(VoiceWorkerStartResult::Deferred { retry_after_ms });
        }
        crate::runtime::RuntimeWorkDecision::Suspend { .. } => {
            return Ok(VoiceWorkerStartResult::Deferred {
                retry_after_ms: 1_000,
            });
        }
        crate::runtime::RuntimeWorkDecision::RejectWithStableKey { reason, .. }
        | crate::runtime::RuntimeWorkDecision::RejectWithUserVisibleReason { reason } => {
            log::warn!("[{}] voice worker rejected by scheduler: {}", TAG, reason);
            crate::metrics::record_voice_tool_failure("voice_session_scheduler");
            return Ok(VoiceWorkerStartResult::Dropped);
        }
    }

    match voice_worker_start_kind(realtime_enabled, &task) {
        VoiceWorkerStartKind::SpawnWorker => {
            let (name, stack_size) = voice_worker_spawn_profile(&task);
            spawn_guarded_with_profile_handle(
                name,
                stack_size,
                Some(SpawnCore::Core1),
                HttpThreadRole::Background,
                move || run_voice_session_worker(cfg, task, worker_tx),
            )
            .map(VoiceWorkerStartResult::Started)
            .map_err(|error| crate::Error::io("voice_session_worker_spawn", error))
        }
        VoiceWorkerStartKind::PrepareRealtimeTransportThenSpawnConnect => {
            let ownership = match prepare_realtime_session_ownership(&cfg) {
                Ok(ownership) => ownership,
                Err(error) => {
                    log::warn!(
                        "[{}] realtime voice transport admission failed: {}",
                        TAG,
                        error
                    );
                    crate::metrics::record_voice_tool_failure("voice_session_realtime");
                    return Ok(VoiceWorkerStartResult::Dropped);
                }
            };
            spawn_guarded_with_profile_handle(
                "voice_realtime_connect",
                STACK_VOICE_REALTIME_CONNECT,
                Some(SpawnCore::Core1),
                HttpThreadRole::Background,
                move || run_realtime_connect_worker(cfg, ownership, worker_tx),
            )
            .map(VoiceWorkerStartResult::Started)
            .map_err(|error| crate::Error::io("voice_realtime_connect_spawn", error))
        }
    }
}

fn voice_worker_scheduler_decision(
    realtime_enabled: bool,
    task: &VoiceWorkerTask,
) -> crate::runtime::RuntimeWorkDecision {
    let Some((class, source)) = voice_worker_runtime_work(realtime_enabled, task) else {
        return crate::runtime::RuntimeWorkDecision::Proceed;
    };
    let pressure = crate::orchestrator::snapshot().pressure;
    crate::runtime::admit_current_runtime_work(
        class,
        source,
        crate::runtime::default_runtime_scheduler_profile(),
        pressure,
    )
}

#[cfg(test)]
fn voice_worker_scheduler_decision_for_context(
    realtime_enabled: bool,
    task: &VoiceWorkerTask,
    context: crate::runtime::RuntimeSchedulerContext,
) -> crate::runtime::RuntimeWorkDecision {
    let Some((class, source)) = voice_worker_runtime_work(realtime_enabled, task) else {
        return crate::runtime::RuntimeWorkDecision::Proceed;
    };
    crate::runtime::admit_runtime_work(
        crate::runtime::RuntimeWorkRequest::new(class, source),
        context,
    )
}

fn voice_worker_runtime_work(
    realtime_enabled: bool,
    task: &VoiceWorkerTask,
) -> Option<(
    crate::runtime::RuntimeWorkClass,
    crate::runtime::RuntimeWorkSource,
)> {
    match task {
        VoiceWorkerTask::WakeInteraction if realtime_enabled => Some((
            crate::runtime::RuntimeWorkClass::RealtimeVoiceSession,
            crate::runtime::RuntimeWorkSource::Background,
        )),
        VoiceWorkerTask::WakeInteraction => Some((
            crate::runtime::RuntimeWorkClass::VoiceFallbackInteraction,
            crate::runtime::RuntimeWorkSource::Background,
        )),
        VoiceWorkerTask::Speak(_) => None,
    }
}

#[cfg(test)]
fn apply_voice_worker_start_result_for_test(
    result: VoiceWorkerStartResult,
    task: VoiceWorkerTask,
    now: Instant,
) -> (
    VoiceWorkerStartDisposition,
    VoiceWorkerRetryGate,
    PendingVoiceEvents,
) {
    let mut retry_gate = VoiceWorkerRetryGate::default();
    let mut pending = PendingVoiceEvents::default();
    match result {
        VoiceWorkerStartResult::Deferred { retry_after_ms } => {
            retry_gate.record_scheduler_defer(now, retry_after_ms);
            restore_pending_voice_task(&mut pending, task);
            (VoiceWorkerStartDisposition::RetryLater, retry_gate, pending)
        }
        VoiceWorkerStartResult::Started(_) | VoiceWorkerStartResult::Dropped => {
            unreachable!("test helper only models scheduler defer")
        }
    }
}

fn voice_worker_start_kind(realtime_enabled: bool, task: &VoiceWorkerTask) -> VoiceWorkerStartKind {
    if realtime_enabled && matches!(task, VoiceWorkerTask::WakeInteraction) {
        VoiceWorkerStartKind::PrepareRealtimeTransportThenSpawnConnect
    } else {
        VoiceWorkerStartKind::SpawnWorker
    }
}

fn voice_worker_spawn_profile(task: &VoiceWorkerTask) -> (&'static str, usize) {
    match task {
        VoiceWorkerTask::WakeInteraction | VoiceWorkerTask::Speak(_) => {
            ("voice_session_worker", STACK_VOICE_SESSION)
        }
    }
}

fn run_voice_session_worker(
    cfg: VoiceSessionConfig,
    task: VoiceWorkerTask,
    worker_tx: mpsc::Sender<VoiceWorkerMessage>,
) {
    run_voice_task(&cfg, task);
    let _ = worker_tx.send(VoiceWorkerMessage::Done);
    log::info!("[{}] worker stopped", TAG);
}

fn run_realtime_connect_worker(
    cfg: VoiceSessionConfig,
    ownership: RealtimeSessionOwnership,
    worker_tx: mpsc::Sender<VoiceWorkerMessage>,
) {
    let result = connect_prepared_realtime_session(&cfg, ownership);
    if let Err(error) = &result {
        log::warn!("[{}] realtime voice connect failed: {}", TAG, error);
        crate::metrics::record_voice_tool_failure("voice_session_realtime");
    }
    if worker_tx
        .send(VoiceWorkerMessage::RealtimePrepared(result))
        .is_err()
    {
        log::warn!("[{}] realtime prepare result receiver dropped", TAG);
    }
    log::info!("[{}] realtime connect worker stopped", TAG);
}

fn spawn_prepared_realtime_session_worker(
    cfg: VoiceSessionConfig,
    prepared: PreparedRealtimeSession,
    worker_tx: mpsc::Sender<VoiceWorkerMessage>,
) -> std::io::Result<TaskHandle> {
    spawn_guarded_with_profile_handle(
        "voice_realtime",
        STACK_VOICE_REALTIME,
        Some(SpawnCore::Core1),
        HttpThreadRole::Background,
        move || {
            run_prepared_realtime_session(&cfg, prepared);
            let _ = worker_tx.send(VoiceWorkerMessage::Done);
            log::info!("[{}] worker stopped", TAG);
        },
    )
}

fn run_voice_task(cfg: &VoiceSessionConfig, task: VoiceWorkerTask) {
    let mut http: Option<Box<dyn PlatformHttpClient>> = None;

    let ensure_http =
        |h: &mut Option<Box<dyn PlatformHttpClient>>,
         make: &dyn Fn() -> crate::error::Result<Box<dyn PlatformHttpClient>>| {
            if h.is_none() {
                match make() {
                    Ok(client) => *h = Some(client),
                    Err(error) => log::error!("[{}] create_http_client failed: {}", TAG, error),
                }
            }
            h.is_some()
        };

    match task {
        VoiceWorkerTask::WakeInteraction => {
            handle_wake_interaction(cfg, &mut http, &ensure_http);
        }
        VoiceWorkerTask::Speak(text) => {
            handle_speak(cfg, &mut http, &ensure_http, &text);
        }
    }
}

struct WakeSessionResetGuard;

impl Drop for WakeSessionResetGuard {
    fn drop(&mut self) {
        crate::wake::reset_after_session();
    }
}

fn prepare_realtime_session_ownership(
    cfg: &VoiceSessionConfig,
) -> crate::Result<RealtimeSessionOwnership> {
    let wake_reset = WakeSessionResetGuard;
    log::info!("[{}] wake triggered, starting voice interaction", TAG);
    let duplex_caps = cfg.platform.audio_duplex_capabilities();

    if !duplex_caps.can_run_realtime_session() {
        return Err(crate::Error::config(
            "voice_realtime_audio_contract",
            format!(
                "realtime session unavailable under audio contract profile={}",
                duplex_caps.profile().as_str()
            ),
        ));
    }
    let runtime_mode = crate::runtime::thread_registry::runtime_mode_snapshot();
    if !runtime_mode.action_budget.allow_realtime_voice_connect {
        return Err(crate::Error::config(
            "voice_realtime_mode_budget",
            format!(
                "skip realtime voice connect under runtime_mode={}",
                runtime_mode.current_mode.as_str()
            ),
        ));
    }
    let foreground_ticket = crate::runtime::renew_runtime_foreground_now(
        crate::runtime::RuntimeForegroundSource::RealtimeVoiceSession,
    );
    let audio_input_call = crate::orchestrator::try_begin_runtime_capability_call(
        crate::orchestrator::RUNTIME_CAPABILITY_AUDIO_INPUT,
    )?;
    let audio_output_call = crate::orchestrator::try_begin_runtime_capability_call(
        crate::orchestrator::RUNTIME_CAPABILITY_AUDIO_OUTPUT,
    )?;
    let audio_input_lease = acquire_audio_lease(
        crate::runtime::lease::LeaseKind::AudioInput,
        AudioLeaseOwner::VoiceRealtime,
    )?;
    let audio_output_lease = acquire_audio_lease(
        crate::runtime::lease::LeaseKind::AudioOutput,
        AudioLeaseOwner::VoiceRealtime,
    )?;
    let voice_transport = VoiceExclusiveTransportGuard::enter(cfg.platform.as_ref(), TAG)?;

    Ok(RealtimeSessionOwnership {
        _foreground_ticket: foreground_ticket,
        _audio_input_call: audio_input_call,
        _audio_output_call: audio_output_call,
        _audio_input_lease: audio_input_lease,
        _audio_output_lease: audio_output_lease,
        _voice_transport: voice_transport,
        _wake_reset: wake_reset,
    })
}

fn connect_prepared_realtime_session(
    cfg: &VoiceSessionConfig,
    ownership: RealtimeSessionOwnership,
) -> crate::Result<PreparedRealtimeSession> {
    let connected = connect_realtime_session(cfg.platform.as_ref(), &cfg.audio_cfg, TAG)?;

    Ok(PreparedRealtimeSession {
        connected,
        _ownership: ownership,
    })
}

fn run_prepared_realtime_session(cfg: &VoiceSessionConfig, prepared: PreparedRealtimeSession) {
    let PreparedRealtimeSession {
        connected,
        _ownership,
    } = prepared;
    match run_connected_realtime_session(cfg.platform.as_ref(), &cfg.audio_cfg, connected) {
        Ok(session) => {
            log::info!(
                "[{}] realtime session finished turns={} input_ms={} output_ms={} duration_ms={}",
                TAG,
                session.turns_completed,
                session.input_audio_ms,
                session.output_audio_ms,
                session.session_ms
            );
            if session.output_audio_ms > 0 {
                crate::metrics::record_voice_output_play_ms(session.output_audio_ms);
            }
        }
        Err(error) => {
            log::warn!("[{}] realtime voice session failed: {}", TAG, error);
            crate::metrics::record_voice_tool_failure("voice_session_realtime");
        }
    }
}

fn handle_wake_interaction<F>(
    cfg: &VoiceSessionConfig,
    http: &mut Option<Box<dyn PlatformHttpClient>>,
    ensure_http: &F,
) where
    F: Fn(
        &mut Option<Box<dyn PlatformHttpClient>>,
        &dyn Fn() -> crate::error::Result<Box<dyn PlatformHttpClient>>,
    ) -> bool,
{
    if audio_realtime_enabled(&cfg.audio_cfg) {
        match prepare_realtime_session_ownership(cfg)
            .and_then(|ownership| connect_prepared_realtime_session(cfg, ownership))
        {
            Ok(prepared) => run_prepared_realtime_session(cfg, prepared),
            Err(error) => {
                log::warn!(
                    "[{}] realtime voice transport admission failed: {}",
                    TAG,
                    error
                );
                crate::metrics::record_voice_tool_failure("voice_session_realtime");
            }
        }
        return;
    }

    let _wake_reset = WakeSessionResetGuard;
    let _foreground_ticket = crate::runtime::renew_runtime_foreground_now(
        crate::runtime::RuntimeForegroundSource::VoiceFallbackInteraction,
    );
    log::info!("[{}] wake triggered, starting voice interaction", TAG);
    let duplex_caps = cfg.platform.audio_duplex_capabilities();

    let make_http = || cfg.network.open_http_client(HttpClientClass::Background);
    if !ensure_http(http, &make_http) {
        return;
    }
    let client = match http.as_mut() {
        Some(client) => client,
        None => return,
    };

    if should_play_wake_prompt(&cfg.audio_cfg)
        && !cfg.wake_prompt.is_empty()
        && duplex_caps.has_speaker_output()
    {
        let Some(baidu_token) = cfg.baidu_token.as_deref() else {
            log::warn!(
                "[{}] wake prompt requested but baidu token cache unavailable",
                TAG
            );
            crate::metrics::record_voice_tool_failure("voice_session_tts");
            return;
        };
        let tts_result = speak_text(
            AudioLeaseOwner::VoiceSession,
            cfg.platform.as_ref(),
            &cfg.audio_cfg,
            baidu_token,
            client.as_mut(),
            &cfg.wake_prompt,
        );
        match tts_result {
            Ok(playback) => {
                if playback.interrupted {
                    log::info!("[{}] wake prompt playback interrupted by local speech", TAG);
                }
            }
            Err(error) => {
                log::warn!("[{}] wake prompt TTS failed: {}", TAG, error);
            }
        }
    }

    if !duplex_caps.has_microphone_input() {
        log::warn!(
            "[{}] microphone unavailable under audio contract profile={}, skipping capture",
            TAG,
            duplex_caps.profile().as_str()
        );
        return;
    }

    let Some(baidu_token) = cfg.baidu_token.as_deref() else {
        log::warn!(
            "[{}] baidu speech fallback unavailable, skipping capture/transcribe",
            TAG
        );
        crate::metrics::record_voice_tool_failure("voice_session_stt");
        return;
    };

    let text = match capture_and_transcribe(
        AudioLeaseOwner::VoiceSession,
        cfg.platform.as_ref(),
        &cfg.audio_cfg,
        baidu_token,
        client.as_mut(),
        AUDIO_CAPTURE_MAX_MS,
        TAG,
    ) {
        Ok(text) => text,
        Err(error) => {
            log::info!("[{}] voice capture/transcribe skipped: {}", TAG, error);
            crate::metrics::record_voice_tool_failure("voice_session_stt");
            return;
        }
    };
    log::info!("[{}] transcribed: {:?}", TAG, text);

    match PcMsg::new_inbound(VOICE_CHANNEL_NAME, VOICE_DEVICE_CHAT_ID, &text, false) {
        Ok(msg) => {
            if let Err(error) = cfg.inbound_tx.try_submit_user(
                msg,
                crate::runtime::RuntimeForegroundSource::VoiceFallbackInteraction,
            ) {
                log::warn!("[{}] inbound queue full, voice msg dropped: {}", TAG, error);
            }
        }
        Err(error) => {
            log::warn!("[{}] PcMsg construction failed: {}", TAG, error);
        }
    }
}

fn handle_speak<F>(
    cfg: &VoiceSessionConfig,
    http: &mut Option<Box<dyn PlatformHttpClient>>,
    ensure_http: &F,
    text: &str,
) where
    F: Fn(
        &mut Option<Box<dyn PlatformHttpClient>>,
        &dyn Fn() -> crate::error::Result<Box<dyn PlatformHttpClient>>,
    ) -> bool,
{
    log::info!("[{}] speaking agent reply ({} chars)", TAG, text.len());
    let duplex_caps = cfg.platform.audio_duplex_capabilities();

    if !duplex_caps.has_speaker_output() {
        log::warn!(
            "[{}] speaker unavailable under audio contract profile={}, dropping TTS",
            TAG,
            duplex_caps.profile().as_str()
        );
        return;
    }
    let make_http = || cfg.network.open_http_client(HttpClientClass::Background);
    if !ensure_http(http, &make_http) {
        return;
    }
    let client = match http.as_mut() {
        Some(client) => client,
        None => return,
    };
    let Some(baidu_token) = cfg.baidu_token.as_deref() else {
        log::warn!("[{}] baidu speech fallback unavailable, dropping TTS", TAG);
        crate::metrics::record_voice_tool_failure("voice_session_tts");
        return;
    };

    let tts_result = speak_text(
        AudioLeaseOwner::VoiceSession,
        cfg.platform.as_ref(),
        &cfg.audio_cfg,
        baidu_token,
        client.as_mut(),
        text,
    );
    match tts_result {
        Ok(playback) => {
            crate::metrics::record_voice_output_tts_http_ms(playback.tts_http_ms);
            crate::metrics::record_voice_output_play_ms(playback.play_ms);
            if playback.interrupted {
                log::info!("[{}] TTS playback interrupted by local speech", TAG);
            }
        }
        Err(error) => {
            log::warn!("[{}] TTS playback failed: {}", TAG, error);
            crate::metrics::record_voice_tool_failure("voice_session_tts");
        }
    }
}

fn drain_worker_messages(
    cfg: &VoiceSessionConfig,
    worker_tx: &mpsc::Sender<VoiceWorkerMessage>,
    worker_handle: &mut Option<TaskHandle>,
    worker_rx: &mpsc::Receiver<VoiceWorkerMessage>,
    worker_busy: &mut bool,
) {
    while let Ok(message) = worker_rx.try_recv() {
        match message {
            VoiceWorkerMessage::Done => {
                *worker_busy = false;
                if let Some(handle) = worker_handle.take() {
                    let _ = handle.join();
                }
            }
            VoiceWorkerMessage::RealtimePrepared(result) => {
                if let Some(handle) = worker_handle.take() {
                    let _ = handle.join();
                }
                match result {
                    Ok(prepared) => match spawn_prepared_realtime_session_worker(
                        cfg.clone(),
                        prepared,
                        worker_tx.clone(),
                    ) {
                        Ok(handle) => {
                            *worker_busy = true;
                            *worker_handle = Some(handle);
                        }
                        Err(error) => {
                            *worker_busy = false;
                            log::error!(
                                "[{}] failed to start realtime session worker: {}",
                                TAG,
                                error
                            );
                            crate::metrics::record_voice_tool_failure("voice_session_realtime");
                        }
                    },
                    Err(_) => {
                        *worker_busy = false;
                    }
                }
            }
        }
    }
}

fn take_pending_voice_task(pending: &mut PendingVoiceEvents) -> Option<VoiceWorkerTask> {
    if pending.wake_requested {
        pending.wake_requested = false;
        return Some(VoiceWorkerTask::WakeInteraction);
    }
    pending.pending_speak.take().map(VoiceWorkerTask::Speak)
}

fn restore_pending_voice_task(pending: &mut PendingVoiceEvents, task: VoiceWorkerTask) {
    match task {
        VoiceWorkerTask::WakeInteraction => {
            pending.wake_requested = true;
            pending.pending_speak = None;
        }
        VoiceWorkerTask::Speak(text) => {
            if pending.pending_speak.is_none() {
                pending.pending_speak = Some(text);
            }
        }
    }
}

fn handle_voice_event(event: VoiceEvent, pending: &mut PendingVoiceEvents) {
    match event {
        VoiceEvent::WakeTriggered => {
            pending.wake_requested = true;
            pending.pending_speak = None;
        }
        VoiceEvent::Speak(text) => {
            let normalized = normalize_speak_text(&text);
            if normalized.is_empty() {
                return;
            }
            if pending.pending_speak.as_deref() == Some(normalized.as_str()) {
                return;
            }
            pending.pending_speak = Some(normalized);
        }
    }
}

fn normalize_speak_text(text: &str) -> String {
    crate::util::truncate_content_to_max(text.trim(), MAX_PENDING_SPEAK_CHARS).into_owned()
}

fn should_play_wake_prompt(audio_cfg: &AudioSegment) -> bool {
    audio_cfg.service_provider == "baidu"
        && !audio_cfg.speech.api_key.trim().is_empty()
        && !audio_cfg.speech.api_secret.trim().is_empty()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn wake_clears_pending_speak() {
        let mut pending = PendingVoiceEvents {
            wake_requested: false,
            pending_speak: Some("old reply".to_string()),
        };
        handle_voice_event(VoiceEvent::WakeTriggered, &mut pending);
        assert!(pending.wake_requested);
        assert!(pending.pending_speak.is_none());
    }

    #[test]
    fn speak_keeps_latest_pending_text() {
        let mut pending = PendingVoiceEvents::default();
        handle_voice_event(VoiceEvent::Speak("first reply".to_string()), &mut pending);
        handle_voice_event(VoiceEvent::Speak("second reply".to_string()), &mut pending);
        assert_eq!(pending.pending_speak.as_deref(), Some("second reply"));
    }

    #[test]
    fn normalize_speak_text_trims_whitespace() {
        assert_eq!(normalize_speak_text("  hello  "), "hello");
    }

    #[test]
    fn worker_retry_gate_blocks_immediate_retry_after_spawn_failure() {
        let mut gate = VoiceWorkerRetryGate::default();
        let now = Instant::now();
        assert!(gate.can_retry(now));

        gate.record_spawn_failure(now);

        assert!(!gate.can_retry(now + Duration::from_millis(100)));
        assert!(gate.can_retry(now + Duration::from_millis(WORKER_SPAWN_FAILURE_COOLDOWN_MS + 1)));
    }

    #[test]
    fn worker_retry_gate_clears_after_success() {
        let mut gate = VoiceWorkerRetryGate::default();
        gate.record_spawn_failure(Instant::now());
        gate.clear();

        assert!(gate.can_retry(Instant::now()));
    }

    #[test]
    fn realtime_wake_prepares_transport_before_session_worker() {
        assert_eq!(
            voice_worker_start_kind(true, &VoiceWorkerTask::WakeInteraction),
            VoiceWorkerStartKind::PrepareRealtimeTransportThenSpawnConnect
        );
        assert_eq!(
            voice_worker_start_kind(false, &VoiceWorkerTask::WakeInteraction),
            VoiceWorkerStartKind::SpawnWorker
        );
        assert_eq!(
            voice_worker_start_kind(true, &VoiceWorkerTask::Speak("reply".to_string())),
            VoiceWorkerStartKind::SpawnWorker
        );
    }

    #[test]
    fn auto_voice_wake_maps_scheduler_work_class_and_source() {
        assert_eq!(
            voice_worker_runtime_work(true, &VoiceWorkerTask::WakeInteraction),
            Some((
                crate::runtime::RuntimeWorkClass::RealtimeVoiceSession,
                crate::runtime::RuntimeWorkSource::Background
            ))
        );
        assert_eq!(
            voice_worker_runtime_work(false, &VoiceWorkerTask::WakeInteraction),
            Some((
                crate::runtime::RuntimeWorkClass::VoiceFallbackInteraction,
                crate::runtime::RuntimeWorkSource::Background
            ))
        );
        assert_eq!(
            voice_worker_runtime_work(true, &VoiceWorkerTask::Speak("reply".to_string())),
            None
        );
    }

    #[test]
    fn auto_voice_wake_scheduler_defer_keeps_task_pending() {
        let now = Instant::now();
        let (outcome, retry_gate, pending) = apply_voice_worker_start_result_for_test(
            VoiceWorkerStartResult::Deferred {
                retry_after_ms: 1_500,
            },
            VoiceWorkerTask::WakeInteraction,
            now,
        );

        assert_eq!(outcome, VoiceWorkerStartDisposition::RetryLater);
        assert!(!retry_gate.can_retry(now + Duration::from_millis(100)));
        assert!(
            pending.wake_requested,
            "scheduler defer must retain auto wake instead of dropping the voice interaction"
        );
    }

    #[test]
    fn auto_realtime_voice_wake_consumes_scheduler_decision_before_connect() {
        let decision = voice_worker_scheduler_decision_for_context(
            true,
            &VoiceWorkerTask::WakeInteraction,
            crate::runtime::RuntimeSchedulerContext {
                profile: crate::runtime::RuntimePlanePolicyProfile::EspCompact,
                runtime_mode: crate::runtime::mode::snapshot_from_source(
                    crate::runtime::mode::RuntimeModeSource::default(),
                ),
                foreground: crate::runtime::RuntimeForegroundOverlay {
                    active: true,
                    active_count: 1,
                    primary_source: Some(crate::runtime::RuntimeForegroundSource::ConfigUiChat),
                    age_ms: Some(500),
                    resume_after_ms: Some(29_500),
                },
                pressure: crate::orchestrator::PressureLevel::Normal,
            },
        );

        assert!(matches!(
            decision,
            crate::runtime::RuntimeWorkDecision::Defer {
                reason: "foreground_active",
                ..
            }
        ));
    }

    #[test]
    fn auto_fallback_voice_wake_consumes_scheduler_decision_before_worker() {
        let decision = voice_worker_scheduler_decision_for_context(
            false,
            &VoiceWorkerTask::WakeInteraction,
            crate::runtime::RuntimeSchedulerContext {
                profile: crate::runtime::RuntimePlanePolicyProfile::EspCompact,
                runtime_mode: crate::runtime::mode::snapshot_from_source(
                    crate::runtime::mode::RuntimeModeSource::default(),
                ),
                foreground: crate::runtime::RuntimeForegroundOverlay {
                    active: true,
                    active_count: 1,
                    primary_source: Some(
                        crate::runtime::RuntimeForegroundSource::ExternalUserMessage,
                    ),
                    age_ms: Some(500),
                    resume_after_ms: Some(29_500),
                },
                pressure: crate::orchestrator::PressureLevel::Normal,
            },
        );

        assert!(matches!(
            decision,
            crate::runtime::RuntimeWorkDecision::Defer {
                reason: "foreground_active",
                ..
            }
        ));
    }
}
