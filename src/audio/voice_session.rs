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
    RealtimeSessionExitReason,
};
use crate::audio::voice_conversation::{NoSpeechExitReason, VoiceConversationController};
use crate::audio::wake_handoff::WakeAudioHandoff;
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
    WakeTriggered(WakeAudioHandoff),
    /// Agent reply to speak aloud via TTS.
    Speak(String),
}

#[derive(Clone)]
enum VoiceWorkerTask {
    WakeInteraction(WakeAudioHandoff),
    Speak(String),
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum VoiceWorkerStartKind {
    SpawnWorker,
    SpawnRealtimeConnectWorker,
}

impl VoiceWorkerStartKind {
    #[cfg(test)]
    fn spawns_realtime_connect_worker_before_transport_ownership(self) -> bool {
        matches!(self, Self::SpawnRealtimeConnectWorker)
    }
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
    conversation: VoiceConversationController,
    _foreground_ticket: VoiceForegroundTicketGuard,
    _audio_input_call: crate::orchestrator::RuntimeCapabilityCallGuard,
    _audio_output_call: crate::orchestrator::RuntimeCapabilityCallGuard,
    _audio_input_lease: AudioLeaseGuard,
    _audio_output_lease: AudioLeaseGuard,
    _voice_transport: VoiceExclusiveTransportGuard,
    _wake_reset: WakeSessionResetGuard,
}

struct PreparedRealtimeSession {
    connected: ConnectedRealtimeSession,
    handoff: WakeAudioHandoff,
    _ownership: RealtimeSessionOwnership,
}

struct VoiceForegroundTicketGuard {
    ticket: Option<crate::runtime::RuntimeForegroundTicket>,
}

impl VoiceForegroundTicketGuard {
    fn new(ticket: crate::runtime::RuntimeForegroundTicket) -> Self {
        Self {
            ticket: Some(ticket),
        }
    }

    fn renew_now(&mut self) {
        if let Some(ticket) = self.ticket {
            self.ticket = Some(crate::runtime::renew_runtime_foreground_now(ticket.source));
        }
    }

    #[cfg(test)]
    fn renew_at_for_test(&mut self, now_ms: u64) {
        if let Some(ticket) = self.ticket {
            self.ticket = Some(crate::runtime::renew_runtime_foreground(
                ticket.source,
                now_ms,
            ));
        }
    }
}

impl Drop for VoiceForegroundTicketGuard {
    fn drop(&mut self) {
        if let Some(ticket) = self.ticket.take() {
            let _ = crate::runtime::finish_runtime_foreground(ticket);
        }
    }
}

enum VoiceWorkerMessage {
    Done,
    RealtimePrepared(Box<crate::Result<PreparedRealtimeSession>>),
}

#[derive(Default)]
struct PendingVoiceEvents {
    wake_handoff: Option<WakeAudioHandoff>,
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
pub struct VoiceSessionConfig {
    pub platform: Arc<dyn Platform>,
    pub network: Arc<NetworkGovernor>,
    pub audio_cfg: AudioSegment,
    pub baidu_token: Option<Arc<BaiduTokenCache>>,
    pub inbound_tx: UserInboundTx,
    pub wake_prompt: String,
}

/// Entry point for the voice session scheduler thread. Blocks on `rx` until the channel closes.
pub fn run_voice_session(cfg: Arc<VoiceSessionConfig>, rx: Receiver<VoiceEvent>) {
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
    cfg: Arc<VoiceSessionConfig>,
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
        VoiceWorkerStartKind::SpawnRealtimeConnectWorker => {
            let VoiceWorkerTask::WakeInteraction(handoff) = task else {
                return Ok(VoiceWorkerStartResult::Dropped);
            };
            if let Some(retry_after_ms) = voice_realtime_startup_defer_ms(
                crate::runtime::runtime_startup_readiness_snapshot(),
            ) {
                return Ok(VoiceWorkerStartResult::Deferred { retry_after_ms });
            }
            if let Some(retry_after_ms) =
                voice_realtime_connect_spawn_reserve_defer_ms(cfg.platform.as_ref())
            {
                return Ok(VoiceWorkerStartResult::Deferred { retry_after_ms });
            }
            spawn_guarded_with_profile_handle(
                "voice_realtime_connect",
                STACK_VOICE_REALTIME_CONNECT,
                Some(SpawnCore::Core1),
                HttpThreadRole::Background,
                move || run_realtime_connect_worker(cfg, handoff, worker_tx),
            )
            .map(VoiceWorkerStartResult::Started)
            .map_err(|error| crate::Error::io("voice_realtime_connect_spawn", error))
        }
    }
}

fn voice_realtime_connect_spawn_reserve_defer_ms(platform: &dyn Platform) -> Option<u64> {
    #[cfg(any(target_arch = "xtensa", target_arch = "riscv32"))]
    {
        let snap = platform.memory_snapshot();
        let min_internal = if snap.heap_free_spiram > 0 {
            crate::constants::TLS_ADMISSION_MIN_INTERNAL_BYTES
        } else {
            crate::constants::TLS_ADMISSION_NO_PSRAM_MIN_BYTES
        }
        .saturating_add(STACK_VOICE_REALTIME_CONNECT);
        let min_largest = crate::constants::TLS_ADMISSION_MIN_LARGEST_BLOCK_BYTES
            .saturating_add(STACK_VOICE_REALTIME_CONNECT);
        let enough_internal = snap.heap_free_internal >= min_internal as u32;
        let enough_largest =
            snap.heap_free_spiram == 0 || snap.heap_largest_block >= min_largest as u32;
        if enough_internal && enough_largest {
            return None;
        }
        log::warn!(
            "[{}] defer realtime voice connect spawn: free={} free_min={} largest={} largest_min={} spiram={} connect_stack={}",
            TAG,
            snap.heap_free_internal,
            min_internal,
            snap.heap_largest_block,
            min_largest,
            snap.heap_free_spiram,
            STACK_VOICE_REALTIME_CONNECT
        );
        Some(1_000)
    }
    #[cfg(not(any(target_arch = "xtensa", target_arch = "riscv32")))]
    {
        let _ = platform;
        None
    }
}

fn voice_realtime_startup_defer_ms(
    readiness: crate::runtime::RuntimeStartupReadiness,
) -> Option<u64> {
    if readiness.allow_voice_realtime_connect {
        None
    } else {
        log::debug!(
            "[{}] defer realtime voice worker start: {}",
            TAG,
            readiness.worker_block_reason()
        );
        Some(1_000)
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
        VoiceWorkerTask::WakeInteraction(_) if realtime_enabled => Some((
            crate::runtime::RuntimeWorkClass::RealtimeVoiceSession,
            crate::runtime::RuntimeWorkSource::Background,
        )),
        VoiceWorkerTask::WakeInteraction(_) => Some((
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
    if realtime_enabled && matches!(task, VoiceWorkerTask::WakeInteraction(_)) {
        VoiceWorkerStartKind::SpawnRealtimeConnectWorker
    } else {
        VoiceWorkerStartKind::SpawnWorker
    }
}

fn voice_worker_spawn_profile(task: &VoiceWorkerTask) -> (&'static str, usize) {
    match task {
        VoiceWorkerTask::WakeInteraction(_) | VoiceWorkerTask::Speak(_) => {
            ("voice_session_worker", STACK_VOICE_SESSION)
        }
    }
}

fn run_voice_session_worker(
    cfg: Arc<VoiceSessionConfig>,
    task: VoiceWorkerTask,
    worker_tx: mpsc::Sender<VoiceWorkerMessage>,
) {
    run_voice_task(&cfg, task);
    let _ = worker_tx.send(VoiceWorkerMessage::Done);
    log::info!("[{}] worker stopped", TAG);
}

fn run_realtime_connect_worker(
    cfg: Arc<VoiceSessionConfig>,
    handoff: WakeAudioHandoff,
    worker_tx: mpsc::Sender<VoiceWorkerMessage>,
) {
    let result = prepare_realtime_session_ownership(&cfg, &handoff)
        .and_then(|ownership| connect_prepared_realtime_session(&cfg, ownership, handoff));
    if let Err(error) = &result {
        log::warn!("[{}] realtime voice connect failed: {}", TAG, error);
        crate::metrics::record_voice_tool_failure("voice_session_realtime");
    }
    if worker_tx
        .send(VoiceWorkerMessage::RealtimePrepared(Box::new(result)))
        .is_err()
    {
        log::warn!("[{}] realtime prepare result receiver dropped", TAG);
    }
    log::info!("[{}] realtime connect worker stopped", TAG);
}

fn spawn_prepared_realtime_session_worker(
    cfg: Arc<VoiceSessionConfig>,
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
        VoiceWorkerTask::WakeInteraction(handoff) => {
            handle_wake_interaction(cfg, &mut http, &ensure_http, handoff);
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
    handoff: &WakeAudioHandoff,
) -> crate::Result<RealtimeSessionOwnership> {
    let wake_reset = WakeSessionResetGuard;
    let mut conversation = VoiceConversationController::wake_primed(handoff);
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
    conversation.mark_connecting();

    Ok(RealtimeSessionOwnership {
        conversation,
        _foreground_ticket: VoiceForegroundTicketGuard::new(foreground_ticket),
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
    mut ownership: RealtimeSessionOwnership,
    handoff: WakeAudioHandoff,
) -> crate::Result<PreparedRealtimeSession> {
    let connected = connect_realtime_session(cfg.platform.as_ref(), &cfg.audio_cfg, TAG)?;
    ownership.conversation.mark_handoff_uploaded(&handoff);

    Ok(PreparedRealtimeSession {
        connected,
        handoff,
        _ownership: ownership,
    })
}

fn run_prepared_realtime_session(cfg: &VoiceSessionConfig, prepared: PreparedRealtimeSession) {
    let PreparedRealtimeSession {
        connected,
        handoff,
        _ownership: mut ownership,
    } = prepared;
    let result = {
        let foreground_ticket = &mut ownership._foreground_ticket;
        run_connected_realtime_session(
            cfg.platform.as_ref(),
            &cfg.audio_cfg,
            connected,
            handoff,
            || foreground_ticket.renew_now(),
        )
    };
    match result {
        Ok(session) => {
            let steady_completed =
                record_realtime_conversation_result(&mut ownership.conversation, &session);
            if steady_completed {
                log::info!(
                    "[{}] realtime session finished turns={} input_ms={} output_ms={} duration_ms={} exit_reason={}",
                    TAG,
                    session.turns_completed,
                    session.input_audio_ms,
                    session.output_audio_ms,
                    session.session_ms,
                    session.exit_reason.as_str()
                );
            } else {
                log::warn!(
                    "[{}] realtime session ended non-steady turns={} input_ms={} output_ms={} duration_ms={} exit_reason={} interrupted_active_turn={} partial_output_pending_at_exit={}",
                    TAG,
                    session.turns_completed,
                    session.input_audio_ms,
                    session.output_audio_ms,
                    session.session_ms,
                    session.exit_reason.as_str(),
                    session.interrupted_active_turn,
                    session.partial_output_pending_at_exit
                );
                crate::metrics::record_voice_tool_failure("voice_session_realtime");
            }
            log_realtime_turn_summary(&ownership.conversation);
            if session.output_audio_ms > 0 {
                crate::metrics::record_voice_output_play_ms(session.output_audio_ms);
            }
        }
        Err(error) => {
            ownership
                .conversation
                .finish_no_speech(NoSpeechExitReason::RealtimeSessionError);
            record_realtime_conversation_summary(&ownership.conversation);
            log_realtime_turn_summary(&ownership.conversation);
            log::warn!("[{}] realtime voice session failed: {}", TAG, error);
            crate::metrics::record_voice_tool_failure("voice_session_realtime");
        }
    }
}

fn record_realtime_conversation_result(
    conversation: &mut VoiceConversationController,
    session: &crate::audio::realtime::RealtimeSessionResult,
) -> bool {
    conversation.record_input_audio_ms(session.input_audio_ms);
    if session.server_speech_started {
        conversation.mark_server_speech_started();
    }
    if session.server_speech_stopped {
        conversation.mark_server_speech_stopped();
    }
    if session.response_created {
        conversation.mark_response_created();
    }
    if session.output_audio_ms > 0 {
        conversation.record_output_audio_ms(session.output_audio_ms);
    }
    if session.turns_completed > 0 {
        for _ in 0..session.turns_completed {
            conversation.mark_turn_completed();
        }
    }

    let steady_completed = session.exit_reason.is_normal_dialogue_exit()
        && session.turns_completed > 0
        && !session.interrupted_active_turn
        && !session.partial_output_pending_at_exit;
    if steady_completed {
        conversation.finish_completed();
    } else if let Some(reason) = realtime_exit_reason_for_conversation(session) {
        conversation.finish_no_speech(reason);
    } else {
        conversation.classify_no_speech(session.turns_completed);
        if conversation.no_speech_reason().is_none() {
            conversation.finish_no_speech(if session.input_audio_ms == 0 {
                NoSpeechExitReason::NoHandoffAudio
            } else {
                NoSpeechExitReason::ProviderNoTurnEvents
            });
        }
    }
    record_realtime_conversation_summary(conversation);
    steady_completed
}

fn realtime_exit_reason_for_conversation(
    session: &crate::audio::realtime::RealtimeSessionResult,
) -> Option<NoSpeechExitReason> {
    match session.exit_reason {
        RealtimeSessionExitReason::PostPlaybackIdle => None,
        RealtimeSessionExitReason::NoLocalSpeechAfterSessionReady => None,
        RealtimeSessionExitReason::ResponseWait => Some(NoSpeechExitReason::RealtimeResponseWait),
        RealtimeSessionExitReason::TransportDisconnected => {
            if session.interrupted_active_turn || session.partial_output_pending_at_exit {
                Some(NoSpeechExitReason::RealtimeTurnInterrupted)
            } else {
                Some(NoSpeechExitReason::RealtimeTransportDisconnected)
            }
        }
        RealtimeSessionExitReason::PeerClosed => {
            if session.interrupted_active_turn || session.partial_output_pending_at_exit {
                Some(NoSpeechExitReason::RealtimeTurnInterrupted)
            } else {
                Some(NoSpeechExitReason::RealtimePeerClosed)
            }
        }
    }
}

fn record_realtime_conversation_summary(conversation: &VoiceConversationController) {
    let summary = conversation.summary();
    crate::metrics::record_voice_realtime_handoff_ms(summary.wake_pre_roll_ms as u128);
    if let Some(reason) = summary.no_speech_reason {
        crate::metrics::record_voice_realtime_no_speech_reason(reason.as_str());
    }
}

fn log_realtime_turn_summary(conversation: &VoiceConversationController) {
    log::info!(
        "[{}] realtime turn summary {}",
        TAG,
        conversation.summary().to_log_fields()
    );
}

fn handle_wake_interaction<F>(
    cfg: &VoiceSessionConfig,
    http: &mut Option<Box<dyn PlatformHttpClient>>,
    ensure_http: &F,
    handoff: WakeAudioHandoff,
) where
    F: Fn(
        &mut Option<Box<dyn PlatformHttpClient>>,
        &dyn Fn() -> crate::error::Result<Box<dyn PlatformHttpClient>>,
    ) -> bool,
{
    if audio_realtime_enabled(&cfg.audio_cfg) {
        match prepare_realtime_session_ownership(cfg, &handoff)
            .and_then(|ownership| connect_prepared_realtime_session(cfg, ownership, handoff))
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
    let _foreground_ticket =
        VoiceForegroundTicketGuard::new(crate::runtime::renew_runtime_foreground_now(
            crate::runtime::RuntimeForegroundSource::VoiceFallbackInteraction,
        ));
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
    cfg: &Arc<VoiceSessionConfig>,
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
                match *result {
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
    if let Some(handoff) = pending.wake_handoff.take() {
        return Some(VoiceWorkerTask::WakeInteraction(handoff));
    }
    pending.pending_speak.take().map(VoiceWorkerTask::Speak)
}

fn restore_pending_voice_task(pending: &mut PendingVoiceEvents, task: VoiceWorkerTask) {
    match task {
        VoiceWorkerTask::WakeInteraction(handoff) => {
            pending.wake_handoff = Some(handoff);
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
        VoiceEvent::WakeTriggered(handoff) => {
            pending.wake_handoff = Some(handoff);
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
    use crate::audio::wake_handoff::{WakeAcousticSnapshot, WakeAudioHandoff};
    use crate::platform::byte_buffer::ByteBuffer;
    use std::sync::Arc;

    fn test_handoff() -> WakeAudioHandoff {
        WakeAudioHandoff {
            id: 1,
            sample_rate_hz: 16_000,
            channels: 1,
            pre_roll_ms: 20,
            post_wake_ms: 0,
            pcm_le_bytes: Arc::new(ByteBuffer::zeroed(64)),
            acoustic: WakeAcousticSnapshot::default(),
        }
    }

    fn realtime_result(
        exit_reason: RealtimeSessionExitReason,
        turns_completed: u32,
        interrupted_active_turn: bool,
        partial_output_pending_at_exit: bool,
    ) -> crate::audio::realtime::RealtimeSessionResult {
        crate::audio::realtime::RealtimeSessionResult {
            turns_completed,
            input_audio_ms: 12_000,
            output_audio_ms: 4_000,
            session_ms: 20_000,
            server_speech_started: true,
            server_speech_stopped: true,
            response_created: true,
            exit_reason,
            interrupted_active_turn,
            partial_output_pending_at_exit,
        }
    }

    #[test]
    fn wake_clears_pending_speak() {
        let mut pending = PendingVoiceEvents {
            wake_handoff: None,
            pending_speak: Some("old reply".to_string()),
        };
        handle_voice_event(VoiceEvent::WakeTriggered(test_handoff()), &mut pending);
        assert!(pending.wake_handoff.is_some());
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
    fn realtime_result_preserves_interrupted_transport_exit() {
        let mut conversation = VoiceConversationController::wake_primed(&test_handoff());
        conversation.mark_handoff_uploaded(&test_handoff());
        let session = realtime_result(
            RealtimeSessionExitReason::TransportDisconnected,
            3,
            true,
            true,
        );

        let steady = record_realtime_conversation_result(&mut conversation, &session);

        assert!(!steady);
        let summary = conversation.summary();
        assert_eq!(summary.turns, 3);
        assert_eq!(
            summary.no_speech_reason,
            Some(NoSpeechExitReason::RealtimeTurnInterrupted)
        );
    }

    #[test]
    fn realtime_result_marks_post_playback_idle_as_steady_completion() {
        let mut conversation = VoiceConversationController::wake_primed(&test_handoff());
        conversation.mark_handoff_uploaded(&test_handoff());
        let session = realtime_result(RealtimeSessionExitReason::PostPlaybackIdle, 2, false, false);

        let steady = record_realtime_conversation_result(&mut conversation, &session);

        assert!(steady);
        let summary = conversation.summary();
        assert_eq!(summary.turns, 2);
        assert_eq!(summary.no_speech_reason, None);
    }

    #[test]
    fn realtime_wake_does_not_prepare_transport_ownership_on_control_thread() {
        assert!(
            voice_worker_start_kind(true, &VoiceWorkerTask::WakeInteraction(test_handoff()))
                .spawns_realtime_connect_worker_before_transport_ownership(),
            "voice_session control thread must stay a light event owner; realtime transport ownership belongs inside the transient connect worker"
        );
        assert_eq!(
            voice_worker_start_kind(false, &VoiceWorkerTask::WakeInteraction(test_handoff())),
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
            voice_worker_runtime_work(true, &VoiceWorkerTask::WakeInteraction(test_handoff())),
            Some((
                crate::runtime::RuntimeWorkClass::RealtimeVoiceSession,
                crate::runtime::RuntimeWorkSource::Background
            ))
        );
        assert_eq!(
            voice_worker_runtime_work(false, &VoiceWorkerTask::WakeInteraction(test_handoff())),
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
    fn voice_foreground_ticket_guard_finishes_ticket_on_drop() {
        let _guard = crate::runtime::foreground::runtime_foreground_test_guard();
        crate::runtime::foreground::reset_runtime_foreground_for_tests();
        let ticket = crate::runtime::foreground::renew_runtime_foreground(
            crate::runtime::RuntimeForegroundSource::RealtimeVoiceSession,
            1_000,
        );

        {
            let _ticket_guard = VoiceForegroundTicketGuard::new(ticket);
        }

        let snapshot = crate::runtime::foreground::runtime_foreground_snapshot_at(1_001);
        assert!(!snapshot.active);
        assert_eq!(snapshot.records.len(), 1);
        assert_eq!(
            snapshot.records[0].state,
            crate::runtime::foreground::RuntimeForegroundTicketState::Finished
        );
    }

    #[test]
    fn voice_foreground_ticket_guard_renews_long_realtime_sessions() {
        let _guard = crate::runtime::foreground::runtime_foreground_test_guard();
        crate::runtime::foreground::reset_runtime_foreground_for_tests();
        let ticket = crate::runtime::foreground::renew_runtime_foreground(
            crate::runtime::RuntimeForegroundSource::RealtimeVoiceSession,
            1_000,
        );
        let mut ticket_guard = VoiceForegroundTicketGuard::new(ticket);

        ticket_guard.renew_at_for_test(25_000);

        let snapshot = crate::runtime::foreground::runtime_foreground_snapshot_at(30_500);
        assert!(snapshot.active);
        assert_eq!(snapshot.active_count, 1);
        assert_eq!(snapshot.records[0].ticket, ticket);
        assert_eq!(snapshot.records[0].renewed_at_ms, 25_000);
        assert_eq!(snapshot.records[0].expires_at_ms, 55_000);
    }

    #[test]
    fn auto_voice_wake_scheduler_defer_keeps_task_pending() {
        let now = Instant::now();
        let (outcome, retry_gate, pending) = apply_voice_worker_start_result_for_test(
            VoiceWorkerStartResult::Deferred {
                retry_after_ms: 1_500,
            },
            VoiceWorkerTask::WakeInteraction(test_handoff()),
            now,
        );

        assert_eq!(outcome, VoiceWorkerStartDisposition::RetryLater);
        assert!(!retry_gate.can_retry(now + Duration::from_millis(100)));
        assert!(
            pending.wake_handoff.is_some(),
            "scheduler defer must retain auto wake instead of dropping the voice interaction"
        );
    }

    #[test]
    fn realtime_voice_startup_gate_defers_before_transport_ownership() {
        let retry_after_ms =
            voice_realtime_startup_defer_ms(crate::runtime::RuntimeStartupReadiness {
                phase: crate::runtime::RuntimeStartupPhase::LocalRuntimeAssembled,
                reason: "wifi_not_ready",
                network_reason: crate::runtime::RuntimeStartupNetworkReason::WifiNotReady,
                allow_config_recovery_routes: true,
                allow_default_status_routes: true,
                allow_external_wss_worker: false,
                allow_agent_heavy_execution: false,
                allow_channel_outbound_worker: false,
                allow_voice_realtime_connect: false,
                allow_write_back_worker: false,
                allow_display_status_surface: true,
                allow_display_heavy_refresh: true,
                config_worker_floor_available: true,
            });

        assert_eq!(retry_after_ms, Some(1_000));
    }

    #[test]
    fn auto_realtime_voice_wake_consumes_scheduler_decision_before_connect() {
        let decision = voice_worker_scheduler_decision_for_context(
            true,
            &VoiceWorkerTask::WakeInteraction(test_handoff()),
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
                    ..crate::runtime::RuntimeForegroundOverlay::default()
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
            &VoiceWorkerTask::WakeInteraction(test_handoff()),
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
                    ..crate::runtime::RuntimeForegroundOverlay::default()
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
