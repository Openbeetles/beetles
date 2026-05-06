//! Configure UI chat stream broker.
//!
//! This is a transport-neutral handoff between the HTTP session route and the
//! single agent loop. It does not execute LLM work; it only carries bounded SSE
//! frames for a request that is already in the normal inbound queue.

use std::collections::{HashMap, VecDeque};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Condvar, Mutex, Weak};
use std::time::{SystemTime, UNIX_EPOCH};

pub const CHANNEL_CONFIGURE_UI_CHAT: &str = "configure_ui_chat";
const CHAT_STREAM_QUEUE_CAPACITY: usize = 16;
const CHAT_STREAM_MAX_ACTIVE: usize = 1;

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
            }),
            ready: Condvar::new(),
        }
    }

    fn recv(&self) -> Option<Vec<u8>> {
        let mut state = self.state.lock().unwrap_or_else(|error| error.into_inner());
        loop {
            if let Some(frame) = state.frames.pop_front() {
                return Some(frame.bytes);
            }
            if state.closed {
                return None;
            }
            state = self
                .ready
                .wait(state)
                .unwrap_or_else(|error| error.into_inner());
        }
    }

    fn push_nonterminal(&self, bytes: Vec<u8>) -> bool {
        let mut state = self.state.lock().unwrap_or_else(|error| error.into_inner());
        if state.closed {
            return false;
        }
        if state.frames.len() >= CHAT_STREAM_QUEUE_CAPACITY {
            if let Some(index) = state.frames.iter().position(|frame| !frame.terminal) {
                state.frames.remove(index);
            } else {
                return true;
            }
        }
        state.frames.push_back(QueuedFrame {
            bytes,
            terminal: false,
        });
        self.ready.notify_one();
        true
    }

    fn push_terminal_pair(&self, terminal: Vec<u8>, done: Vec<u8>) -> bool {
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
}

/// Open SSE stream receiver owned by the HTTP transport.
pub struct ChatStreamReceiver {
    stream_id: String,
    queue: Arc<ChatStreamQueue>,
    broker: Weak<ChatStreamBroker>,
}

impl ChatStreamReceiver {
    pub fn recv(&self) -> Option<Vec<u8>> {
        self.queue.recv()
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
    sequence: AtomicU64,
    streams: Mutex<HashMap<String, StreamEntry>>,
}

impl ChatStreamBroker {
    pub fn new() -> Self {
        Self {
            sequence: AtomicU64::new(1),
            streams: Mutex::new(HashMap::new()),
        }
    }

    pub fn try_open(self: &Arc<Self>) -> Result<ChatStreamOpen, ChatStreamOpenError> {
        {
            let streams = self
                .streams
                .lock()
                .unwrap_or_else(|error| error.into_inner());
            if streams.len() >= CHAT_STREAM_MAX_ACTIVE {
                return Err(ChatStreamOpenError::Busy);
            }
        }
        let sequence = self.sequence.fetch_add(1, Ordering::Relaxed);
        let now_ms = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|duration| duration.as_millis())
            .unwrap_or(0);
        let stream_id = format!("chat_stream_{now_ms}_{sequence}");
        let queue = Arc::new(ChatStreamQueue::new());
        self.streams
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .insert(
                stream_id.clone(),
                StreamEntry {
                    queue: Arc::clone(&queue),
                },
            );
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
        self.send_event(
            stream_id,
            "delta",
            serde_json::json!({
                "delta": delta,
                "accumulated": accumulated,
            }),
        );
    }

    pub fn emit_final(&self, stream_id: &str, content: &str, session_appended: bool) {
        self.send_terminal_events(
            stream_id,
            "final",
            serde_json::json!({
                "content": content,
                "session_appended": session_appended,
            }),
        );
    }

    pub fn emit_error(&self, stream_id: &str, error_key: &str, upstream_error: Option<&str>) {
        self.send_terminal_events(
            stream_id,
            "error",
            serde_json::json!({
                "error_key": error_key,
                "upstream_error": upstream_error,
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
        let _ = entry.queue.push_terminal_pair(
            encode_sse_event(event, data),
            encode_sse_event("done", serde_json::json!({})),
        );
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
        let broker = Arc::new(ChatStreamBroker::new());
        let opened = broker.try_open().expect("open stream");
        let stream_id = opened.stream_id.clone();

        broker.emit_queued(&stream_id, "configure-ui:default");
        broker.emit_delta(&stream_id, "he", "he");
        broker.emit_final(&stream_id, "hello", true);

        assert!(frame_text(opened.receiver.recv().expect("queued")).contains("event: queued"));
        assert!(frame_text(opened.receiver.recv().expect("delta")).contains("event: delta"));
        let final_frame = frame_text(opened.receiver.recv().expect("final"));
        assert!(final_frame.contains("event: final"));
        assert!(final_frame.contains("\"session_appended\":true"));
        assert!(frame_text(opened.receiver.recv().expect("done")).contains("event: done"));
        assert!(!broker.has_active(&stream_id));
    }

    #[test]
    fn broker_preserves_terminal_events_when_delta_queue_is_full() {
        let broker = Arc::new(ChatStreamBroker::new());
        let opened = broker.try_open().expect("open stream");
        let stream_id = opened.stream_id.clone();

        for index in 0..(CHAT_STREAM_QUEUE_CAPACITY * 2) {
            broker.emit_delta(&stream_id, "x", &format!("delta-{index}"));
        }
        broker.emit_final(&stream_id, "done", true);

        let frames = std::iter::from_fn(|| opened.receiver.recv())
            .map(frame_text)
            .collect::<Vec<_>>();
        assert!(
            frames.iter().any(|frame| frame.contains("event: final")),
            "final frame must survive delta backpressure"
        );
        assert!(
            frames.iter().any(|frame| frame.contains("event: done")),
            "done frame must survive delta backpressure"
        );
        assert!(!broker.has_active(&stream_id));
    }

    #[test]
    fn broker_rejects_second_active_stream() {
        let broker = Arc::new(ChatStreamBroker::new());
        let _opened = broker.try_open().expect("first stream");
        assert_eq!(broker.try_open().err(), Some(ChatStreamOpenError::Busy));
    }
}
