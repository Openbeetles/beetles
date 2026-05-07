//! Configure UI chat stream broker.
//!
//! This is a transport-neutral handoff between the HTTP session route and the
//! single agent loop. It does not execute LLM work; it only carries bounded SSE
//! frames for a request that is already in the normal inbound queue.

use std::collections::{HashMap, VecDeque};
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::{Arc, Condvar, Mutex, Weak};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

pub const CHANNEL_CONFIGURE_UI_CHAT: &str = "configure_ui_chat";
#[cfg(any(target_arch = "xtensa", target_arch = "riscv32"))]
const CHAT_STREAM_QUEUE_CAPACITY: usize = 16;
#[cfg(not(any(target_arch = "xtensa", target_arch = "riscv32")))]
const CHAT_STREAM_QUEUE_CAPACITY: usize = 64;
const CHAT_STREAM_FRAME_MAX_BYTES: usize = crate::constants::SSE_LINE_BUF_SIZE;

#[cfg(any(target_arch = "xtensa", target_arch = "riscv32"))]
const DEFAULT_CHAT_STREAM_MAX_ACTIVE: usize = 1;
#[cfg(not(any(target_arch = "xtensa", target_arch = "riscv32")))]
const DEFAULT_CHAT_STREAM_MAX_ACTIVE: usize = 4;

enum QueueRecv {
    Frame(Vec<u8>),
    Closed,
    Timeout,
}

#[derive(Clone)]
struct StreamEntry {
    queue: Arc<ChatStreamQueue>,
}

struct QueuedFrame {
    bytes: Vec<u8>,
    terminal: bool,
}

struct QueueState {
    frames: VecDeque<QueuedFrame>,
    closed: bool,
    sent_content_bytes: usize,
}

struct ChatStreamQueue {
    state: Mutex<QueueState>,
    ready: Condvar,
}

impl ChatStreamQueue {
    fn new() -> Self {
        Self {
            state: Mutex::new(QueueState {
                frames: VecDeque::with_capacity(CHAT_STREAM_QUEUE_CAPACITY),
                closed: false,
                sent_content_bytes: 0,
            }),
            ready: Condvar::new(),
        }
    }

    fn recv(&self, timeout: Duration) -> QueueRecv {
        let mut state = self.state.lock().unwrap_or_else(|error| error.into_inner());
        loop {
            if let Some(frame) = state.frames.pop_front() {
                return QueueRecv::Frame(frame.bytes);
            }
            if state.closed {
                return QueueRecv::Closed;
            }
            let (next_state, wait_result) = self
                .ready
                .wait_timeout(state, timeout)
                .unwrap_or_else(|error| error.into_inner());
            state = next_state;
            if wait_result.timed_out() {
                return QueueRecv::Timeout;
            }
        }
    }

    fn push_nonterminal(&self, bytes: Vec<u8>) -> bool {
        if bytes.len() > CHAT_STREAM_FRAME_MAX_BYTES {
            log::warn!(
                "[chat_stream] oversized nonterminal frame dropped bytes={} limit={}",
                bytes.len(),
                CHAT_STREAM_FRAME_MAX_BYTES
            );
            return false;
        }
        let mut state = self.state.lock().unwrap_or_else(|error| error.into_inner());
        if state.closed {
            return false;
        }
        if state.frames.len() >= CHAT_STREAM_QUEUE_CAPACITY {
            log::warn!(
                "[chat_stream] nonterminal queue full capacity={}",
                CHAT_STREAM_QUEUE_CAPACITY
            );
            return false;
        }
        state.frames.push_back(QueuedFrame {
            bytes,
            terminal: false,
        });
        self.ready.notify_one();
        true
    }

    fn push_terminal_pair(&self, terminal: Vec<u8>, done: Vec<u8>) -> bool {
        if terminal.len() > CHAT_STREAM_FRAME_MAX_BYTES || done.len() > CHAT_STREAM_FRAME_MAX_BYTES
        {
            log::warn!(
                "[chat_stream] oversized terminal frame dropped terminal_bytes={} done_bytes={} limit={}",
                terminal.len(),
                done.len(),
                CHAT_STREAM_FRAME_MAX_BYTES
            );
            return false;
        }
        let mut state = self.state.lock().unwrap_or_else(|error| error.into_inner());
        if state.closed {
            return false;
        }
        while state.frames.len().saturating_add(2) > CHAT_STREAM_QUEUE_CAPACITY {
            if let Some(index) = state.frames.iter().position(|frame| !frame.terminal) {
                state.frames.remove(index);
            } else {
                break;
            }
        }
        state.frames.push_back(QueuedFrame {
            bytes: terminal,
            terminal: true,
        });
        state.frames.push_back(QueuedFrame {
            bytes: done,
            terminal: true,
        });
        state.closed = true;
        self.ready.notify_all();
        true
    }

    fn close(&self) {
        let mut state = self.state.lock().unwrap_or_else(|error| error.into_inner());
        state.closed = true;
        self.ready.notify_all();
    }

    fn sent_content_bytes(&self) -> usize {
        self.state
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .sent_content_bytes
    }

    fn mark_sent_content_bytes(&self, bytes: usize) {
        let mut state = self.state.lock().unwrap_or_else(|error| error.into_inner());
        state.sent_content_bytes = state.sent_content_bytes.max(bytes);
    }
}

/// Open SSE stream receiver owned by the HTTP transport.
pub struct ChatStreamReceiver {
    stream_id: String,
    queue: Arc<ChatStreamQueue>,
    broker: Weak<ChatStreamBroker>,
}

impl ChatStreamReceiver {
    pub fn recv(&self) -> Option<Vec<u8>> {
        self.recv_with_timeout(Duration::from_secs(
            crate::constants::CHAT_STREAM_RECV_TIMEOUT_SECS,
        ))
    }

    pub fn recv_with_timeout(&self, timeout: Duration) -> Option<Vec<u8>> {
        match self.queue.recv(timeout) {
            QueueRecv::Frame(bytes) => Some(bytes),
            QueueRecv::Closed => None,
            QueueRecv::Timeout => {
                if let Some(broker) = self.broker.upgrade() {
                    broker.emit_error(&self.stream_id, "chat.stream_timeout", None);
                    match self.queue.recv(timeout) {
                        QueueRecv::Frame(bytes) => Some(bytes),
                        QueueRecv::Closed | QueueRecv::Timeout => None,
                    }
                } else {
                    None
                }
            }
        }
    }
}

impl Iterator for ChatStreamReceiver {
    type Item = Vec<u8>;

    fn next(&mut self) -> Option<Self::Item> {
        self.recv()
    }
}

impl Drop for ChatStreamReceiver {
    fn drop(&mut self) {
        if let Some(broker) = self.broker.upgrade() {
            broker.unregister(&self.stream_id);
        }
    }
}

impl std::fmt::Debug for ChatStreamReceiver {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ChatStreamReceiver")
            .field("stream_id", &self.stream_id)
            .finish_non_exhaustive()
    }
}

/// Registered stream returned to the session POST handler.
pub struct ChatStreamOpen {
    pub stream_id: String,
    pub receiver: ChatStreamReceiver,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ChatStreamOpenError {
    Busy,
}

/// Bounded fan-out for Configure UI chat SSE events.
pub struct ChatStreamBroker {
    sequence: AtomicU32,
    max_active: usize,
    streams: Mutex<HashMap<String, StreamEntry>>,
}

impl ChatStreamBroker {
    pub fn new() -> Self {
        Self {
            sequence: AtomicU32::new(1),
            max_active: DEFAULT_CHAT_STREAM_MAX_ACTIVE,
            streams: Mutex::new(HashMap::new()),
        }
    }

    #[cfg(test)]
    pub fn new_with_max_active_for_test(max_active: usize) -> Self {
        Self {
            sequence: AtomicU32::new(1),
            max_active,
            streams: Mutex::new(HashMap::new()),
        }
    }

    pub fn try_open(self: &Arc<Self>) -> Result<ChatStreamOpen, ChatStreamOpenError> {
        let sequence = self.sequence.fetch_add(1, Ordering::Relaxed);
        let now_ms = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|duration| duration.as_millis())
            .unwrap_or(0);
        let stream_id = format!("chat_stream_{now_ms}_{sequence}");
        let queue = Arc::new(ChatStreamQueue::new());
        {
            let mut streams = self
                .streams
                .lock()
                .unwrap_or_else(|error| error.into_inner());
            if streams.len() >= self.max_active {
                return Err(ChatStreamOpenError::Busy);
            }
            streams.insert(
                stream_id.clone(),
                StreamEntry {
                    queue: Arc::clone(&queue),
                },
            );
        }
        Ok(ChatStreamOpen {
            stream_id: stream_id.clone(),
            receiver: ChatStreamReceiver {
                stream_id,
                queue,
                broker: Arc::downgrade(self),
            },
        })
    }

    pub fn has_active(&self, stream_id: &str) -> bool {
        self.streams
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .contains_key(stream_id)
    }

    pub fn emit_queued(&self, stream_id: &str, chat_id: &str) {
        log::info!(
            "[chat_stream] event=queued stream_id={} chat_id={}",
            stream_id,
            chat_id
        );
        self.send_event(
            stream_id,
            "queued",
            serde_json::json!({
                "stream_id": stream_id,
                "chat_id": chat_id,
            }),
        );
    }

    pub fn emit_delta(&self, stream_id: &str, delta: &str, accumulated: &str) {
        let Some(entry) = self.entry(stream_id) else {
            return;
        };
        let sent = self.send_delta_chunks(&entry, stream_id, delta);
        if sent {
            entry.queue.mark_sent_content_bytes(accumulated.len());
        }
    }

    pub fn emit_final(
        &self,
        stream_id: &str,
        content: &str,
        session_appended: bool,
        message_id: Option<&str>,
    ) {
        log::info!(
            "[chat_stream] event=final stream_id={} session_appended={} message_id_present={}",
            stream_id,
            session_appended,
            message_id.is_some()
        );
        let Some(entry) = self.entry(stream_id) else {
            return;
        };
        let sent = entry.queue.sent_content_bytes();
        let suffix_sent = if sent == 0 {
            self.send_delta_chunks(&entry, stream_id, content)
        } else if sent < content.len() {
            if let Some(suffix) = content.get(sent..) {
                self.send_delta_chunks(&entry, stream_id, suffix)
            } else {
                true
            }
        } else {
            true
        };
        if !suffix_sent {
            return;
        }
        let entry = {
            self.streams
                .lock()
                .unwrap_or_else(|error| error.into_inner())
                .remove(stream_id)
        };
        let Some(entry) = entry else {
            return;
        };
        self.push_terminal_events(
            entry,
            "final",
            serde_json::json!({
                "message_id": message_id,
                "turn_id": stream_id,
                "session_appended": session_appended,
            }),
        );
    }

    pub fn emit_error(&self, stream_id: &str, error_key: &str, error_stage: Option<&str>) {
        log::warn!(
            "[chat_stream] event=error stream_id={} error_key={} error_stage={}",
            stream_id,
            error_key,
            error_stage.unwrap_or("")
        );
        self.send_terminal_events(
            stream_id,
            "error",
            serde_json::json!({
                "error_key": error_key,
                "error_stage": error_stage,
                "meta": {},
            }),
        );
    }

    fn unregister(&self, stream_id: &str) {
        if let Some(entry) = self
            .streams
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .remove(stream_id)
        {
            entry.queue.close();
        }
    }

    fn entry(&self, stream_id: &str) -> Option<StreamEntry> {
        self.streams
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .get(stream_id)
            .cloned()
    }

    fn send_event(&self, stream_id: &str, event: &'static str, data: serde_json::Value) {
        let Some(entry) = self.entry(stream_id) else {
            return;
        };
        let frame = encode_sse_event(event, data);
        let _ = entry.queue.push_nonterminal(frame);
    }

    fn send_terminal_events(&self, stream_id: &str, event: &'static str, data: serde_json::Value) {
        let entry = {
            self.streams
                .lock()
                .unwrap_or_else(|error| error.into_inner())
                .remove(stream_id)
        };
        let Some(entry) = entry else {
            return;
        };
        self.push_terminal_events(entry, event, data);
    }

    fn push_terminal_events(
        &self,
        entry: StreamEntry,
        event: &'static str,
        data: serde_json::Value,
    ) {
        let _ = entry.queue.push_terminal_pair(
            encode_sse_event(event, data),
            encode_sse_event("done", serde_json::json!({})),
        );
    }

    fn send_delta_chunks(&self, entry: &StreamEntry, stream_id: &str, text: &str) -> bool {
        for chunk in delta_chunks(text, stream_id) {
            let frame = encode_sse_event(
                "delta",
                serde_json::json!({
                    "delta": chunk,
                    "message_id": stream_id,
                }),
            );
            if !entry.queue.push_nonterminal(frame) {
                self.emit_error(stream_id, "chat.stream_backpressure", None);
                return false;
            }
        }
        true
    }
}

impl Default for ChatStreamBroker {
    fn default() -> Self {
        Self::new()
    }
}

fn encode_sse_event(event: &str, data: serde_json::Value) -> Vec<u8> {
    let data = serde_json::to_string(&data).unwrap_or_else(|_| "{}".to_string());
    format!("event: {event}\ndata: {data}\n\n").into_bytes()
}

fn delta_chunks(text: &str, stream_id: &str) -> Vec<String> {
    if text.is_empty() {
        return vec![String::new()];
    }
    let mut chunks = Vec::new();
    let mut current = String::new();
    for ch in text.chars() {
        current.push(ch);
        let frame = encode_sse_event(
            "delta",
            serde_json::json!({
                "delta": current,
                "message_id": stream_id,
            }),
        );
        if frame.len() <= CHAT_STREAM_FRAME_MAX_BYTES {
            continue;
        }
        let last = current
            .pop()
            .expect("current contains the just-pushed char");
        if !current.is_empty() {
            chunks.push(std::mem::take(&mut current));
        }
        current.push(last);
    }
    if !current.is_empty() {
        chunks.push(current);
    }
    chunks
}

pub fn is_configure_ui_stream_turn(msg: &crate::bus::PcMsg) -> bool {
    msg.channel.as_ref() == CHANNEL_CONFIGURE_UI_CHAT && msg.req_id.is_some()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn frame_text(bytes: Vec<u8>) -> String {
        String::from_utf8(bytes).expect("utf8 frame")
    }

    #[test]
    fn broker_emits_delta_final_done_and_unregisters_stream() {
        let broker = Arc::new(ChatStreamBroker::new_with_max_active_for_test(1));
        let opened = broker.try_open().expect("open stream");
        let stream_id = opened.stream_id.clone();

        broker.emit_queued(&stream_id, "configure-ui:default");
        broker.emit_delta(&stream_id, "he", "he");
        broker.emit_final(&stream_id, "hello", true, Some("msg_a1"));

        assert!(frame_text(opened.receiver.recv().expect("queued")).contains("event: queued"));
        assert!(frame_text(opened.receiver.recv().expect("delta")).contains("event: delta"));
        let suffix_frame = frame_text(opened.receiver.recv().expect("suffix delta"));
        assert!(suffix_frame.contains("event: delta"));
        assert!(suffix_frame.contains("llo"));
        let final_frame = frame_text(opened.receiver.recv().expect("final"));
        assert!(final_frame.contains("event: final"));
        assert!(final_frame.contains("\"message_id\":\"msg_a1\""));
        assert!(final_frame.contains("\"turn_id\":\"chat_stream_"));
        assert!(final_frame.contains("\"session_appended\":true"));
        assert!(!final_frame.contains("\"content\""));
        assert!(frame_text(opened.receiver.recv().expect("done")).contains("event: done"));
        assert!(!broker.has_active(&stream_id));
    }

    #[test]
    fn broker_closes_with_backpressure_when_delta_queue_is_full() {
        let broker = Arc::new(ChatStreamBroker::new_with_max_active_for_test(1));
        let opened = broker.try_open().expect("open stream");
        let stream_id = opened.stream_id.clone();

        for index in 0..(CHAT_STREAM_QUEUE_CAPACITY * 2) {
            broker.emit_delta(&stream_id, "x", &format!("delta-{index}"));
        }
        broker.emit_final(&stream_id, "done", true, None);

        let frames = std::iter::from_fn(|| opened.receiver.recv())
            .map(frame_text)
            .collect::<Vec<_>>();
        assert!(
            frames.iter().any(|frame| frame.contains("event: error")),
            "backpressure must terminate the stream explicitly"
        );
        assert!(
            frames.iter().any(|frame| frame.contains("event: done")),
            "done frame must survive delta backpressure"
        );
        assert!(
            frames
                .iter()
                .any(|frame| frame.contains("chat.stream_backpressure")),
            "error frame must identify stream backpressure"
        );
        assert!(!broker.has_active(&stream_id));
    }

    #[test]
    fn broker_splits_large_delta_frames_under_sse_frame_limit() {
        let broker = Arc::new(ChatStreamBroker::new_with_max_active_for_test(1));
        let opened = broker.try_open().expect("open stream");
        let stream_id = opened.stream_id.clone();
        let large_delta = "0123456789".repeat(CHAT_STREAM_FRAME_MAX_BYTES / 2);

        broker.emit_delta(&stream_id, &large_delta, &large_delta);
        broker.emit_final(&stream_id, &large_delta, true, None);

        let frames = std::iter::from_fn(|| opened.receiver.recv()).collect::<Vec<_>>();
        assert!(
            frames.len() > 2,
            "large delta should be split before final/done"
        );
        assert!(
            frames
                .iter()
                .all(|frame| frame.len() <= CHAT_STREAM_FRAME_MAX_BYTES),
            "all SSE frames must stay below the configured frame limit"
        );
        let text = frames
            .iter()
            .map(|frame| String::from_utf8(frame.clone()).expect("utf8 frame"))
            .collect::<String>();
        assert!(text.contains("event: final"));
        assert!(!text.contains("event: snapshot"));
        assert!(!broker.has_active(&stream_id));
    }

    #[test]
    fn broker_final_sends_content_as_delta_when_no_progress_arrived() {
        let broker = Arc::new(ChatStreamBroker::new_with_max_active_for_test(1));
        let opened = broker.try_open().expect("open stream");
        let stream_id = opened.stream_id.clone();

        broker.emit_final(&stream_id, "fallback reply", true, None);

        let delta_frame = frame_text(opened.receiver.recv().expect("delta"));
        let final_frame = frame_text(opened.receiver.recv().expect("final"));

        assert!(delta_frame.contains("event: delta"));
        assert!(delta_frame.contains("fallback reply"));
        assert!(final_frame.contains("event: final"));
        assert!(!final_frame.contains("fallback reply"));
        assert!(!broker.has_active(&stream_id));
    }

    #[test]
    fn broker_rejects_second_active_stream() {
        let broker = Arc::new(ChatStreamBroker::new_with_max_active_for_test(1));
        let _opened = broker.try_open().expect("first stream");
        assert_eq!(broker.try_open().err(), Some(ChatStreamOpenError::Busy));
    }

    #[test]
    fn broker_enforces_single_active_stream_under_concurrency() {
        let broker = Arc::new(ChatStreamBroker::new_with_max_active_for_test(1));
        let start = Arc::new(std::sync::Barrier::new(8));
        let finish = Arc::new(std::sync::Barrier::new(8));
        let mut threads = Vec::new();

        for _ in 0..8 {
            let broker = Arc::clone(&broker);
            let start = Arc::clone(&start);
            let finish = Arc::clone(&finish);
            threads.push(std::thread::spawn(move || {
                start.wait();
                let opened = broker.try_open().ok();
                let success = opened.is_some();
                finish.wait();
                success
            }));
        }

        let successes = threads
            .into_iter()
            .map(|thread| thread.join().expect("thread should finish"))
            .filter(|opened| *opened)
            .count();
        assert_eq!(successes, 1);
    }

    #[test]
    fn receiver_timeout_emits_terminal_error_and_releases_stream() {
        let broker = Arc::new(ChatStreamBroker::new_with_max_active_for_test(1));
        let opened = broker.try_open().expect("open stream");
        let stream_id = opened.stream_id.clone();

        let error_frame = frame_text(
            opened
                .receiver
                .recv_with_timeout(Duration::from_millis(1))
                .expect("timeout error frame"),
        );
        let done_frame = frame_text(opened.receiver.recv().expect("done frame"));

        assert!(error_frame.contains("event: error"));
        assert!(error_frame.contains("chat.stream_timeout"));
        assert!(done_frame.contains("event: done"));
        assert!(!broker.has_active(&stream_id));
    }
}
