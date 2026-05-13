//! storage 实现的 SessionStore。会话文件为 {sessions_dir}/{short_id}.jsonl，有界 ring。
//! 短 id：chat_id 若不超过 MAX_CHAT_ID_FILENAME_LEN 则直接用，否则用 8 字符哈希，文件首行存 "# chat_id: <真实 id>"。

use crate::error::{Error, Result};
use crate::memory::{
    synthesize_session_message_records, SessionMessage, SessionMessageRecord, SessionStore,
    MAX_SESSION_ENTRIES, MAX_SESSION_MESSAGE_LEN, REL_PATH_SESSIONS_DIR,
};
use serde_json;
use std::collections::HashMap;
use std::collections::VecDeque;
#[cfg(not(any(target_arch = "xtensa", target_arch = "riscv32")))]
use std::fs::OpenOptions;
use std::io::{Read, Write};
use std::path::Path;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::Mutex;

use crate::platform::psram_vec::PsramVec;
use crate::platform::state_root::state_mount_path;

#[cfg(not(any(target_arch = "xtensa", target_arch = "riscv32")))]
use super::finish_file_after_write;
use super::{list_dir, read_file, with_fs_lock_stage, WriteDurability, MAX_WRITE_SIZE};

const TAG: &str = "platform::storage::session";

/// 文件名中 chat_id 部分最大长度（不含 .jsonl），超出则用 hash 短名，满足 ESP-IDF 路径/文件名限制。
const MAX_CHAT_ID_FILENAME_LEN: usize = 20;
const SESSION_FILE_EXT: &str = ".jsonl";
const CHAT_ID_HEADER_PREFIX: &str = "# chat_id: ";
const SESSION_MESSAGE_ID_PREFIX: &str = "msg_";
#[cfg(any(target_arch = "xtensa", target_arch = "riscv32"))]
const SESSION_TAIL_READ_MAX_BYTES: usize = 16 * 1024;
// xtensa/riscv32 targets do not expose AtomicU64; the counter only needs to
// disambiguate same-timestamp writes within one process, so AtomicU32 is enough.
static SESSION_MESSAGE_ID_COUNTER: AtomicU32 = AtomicU32::new(1);

fn fnv1a_hash(s: &str) -> u32 {
    let mut h: u32 = 2166136261;
    for b in s.bytes() {
        h ^= b as u32;
        h = h.wrapping_mul(16777619);
    }
    h
}

/// 返回 (路径, 是否在文件中写入 chat_id 首行)。长 chat_id 用 8 位 hex 哈希作文件名，首行存真实 id。
fn session_path(chat_id: &str) -> Result<(PathBuf, bool)> {
    if chat_id.is_empty() {
        return Err(Error::config("session_path", "chat_id empty"));
    }
    if !chat_id
        .chars()
        .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-' || c == '.' || c == ':')
    {
        return Err(Error::config(
            "session_path",
            "chat_id contains invalid chars",
        ));
    }
    let mut p = state_mount_path();
    p.push(REL_PATH_SESSIONS_DIR);
    let (filename, write_header) = if chat_id.len() <= MAX_CHAT_ID_FILENAME_LEN {
        (format!("{}{}", chat_id, SESSION_FILE_EXT), false)
    } else {
        let h = fnv1a_hash(chat_id);
        (format!("{:08x}{}", h, SESSION_FILE_EXT), true)
    };
    p.push(&filename);
    if p.as_os_str().len() > 56 {
        return Err(Error::config(
            "session_path",
            format!("path too long ({})", p.as_os_str().len()),
        ));
    }
    Ok((p, write_header))
}

#[derive(Clone, Debug, serde::Serialize, serde::Deserialize, PartialEq, Eq)]
struct StoredSessionMessage {
    #[serde(default)]
    message_id: String,
    role: String,
    content: String,
}

impl StoredSessionMessage {
    fn new(role: &str, content: &str) -> Self {
        Self {
            message_id: next_session_message_id(),
            role: role.to_string(),
            content: content.to_string(),
        }
    }

    fn from_session_message(message: &SessionMessage) -> Self {
        Self::new(message.role.as_str(), message.content.as_str())
    }

    fn to_session_message(&self) -> SessionMessage {
        SessionMessage {
            role: self.role.clone(),
            content: self.content.clone(),
        }
    }

    fn to_session_record(&self) -> SessionMessageRecord {
        SessionMessageRecord {
            message_id: self.message_id.clone(),
            role: self.role.clone(),
            content: self.content.clone(),
        }
    }
}

fn next_session_message_id() -> String {
    let now_nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|value| value.as_nanos().min(u64::MAX as u128) as u64)
        .unwrap_or(0);
    let counter = SESSION_MESSAGE_ID_COUNTER.fetch_add(1, Ordering::Relaxed);
    format!("{SESSION_MESSAGE_ID_PREFIX}{now_nanos:016x}{counter:08x}")
}

enum ParsedJsonlLine {
    Ignored,
    Message(StoredSessionMessage),
    RepairedMessage(StoredSessionMessage),
    Invalid,
}

struct SessionFileSnapshot {
    messages: VecDeque<StoredSessionMessage>,
    message_count: usize,
    malformed_lines: usize,
    has_data: bool,
    ends_with_newline: bool,
    needs_repair: bool,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
struct SessionAppendState {
    message_count: usize,
    has_data: bool,
    ends_with_newline: bool,
}

impl SessionAppendState {
    fn from_observed_snapshot(snapshot: &SessionFileSnapshot) -> Self {
        Self {
            message_count: snapshot.message_count,
            has_data: snapshot.has_data,
            ends_with_newline: snapshot.ends_with_newline,
        }
    }

    fn from_written_messages(message_count: usize, has_data: bool) -> Self {
        Self {
            message_count,
            has_data,
            ends_with_newline: has_data,
        }
    }

    fn after_appending(self, appended_messages: usize) -> Self {
        let has_data = self.has_data || appended_messages > 0;
        Self {
            message_count: self
                .message_count
                .saturating_add(appended_messages)
                .min(MAX_SESSION_ENTRIES),
            has_data,
            ends_with_newline: has_data,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum SessionRepairMode {
    Deferred,
    Immediate,
}

fn parse_jsonl_line(line: &str) -> ParsedJsonlLine {
    let line = line.trim();
    if line.is_empty() || line.starts_with('#') {
        return ParsedJsonlLine::Ignored;
    }
    match serde_json::from_str::<StoredSessionMessage>(line) {
        Ok(message) => normalize_parsed_message(message),
        Err(_) => {
            let mut iter =
                serde_json::Deserializer::from_str(line).into_iter::<StoredSessionMessage>();
            match iter.next() {
                Some(Ok(message)) => normalize_parsed_message(message),
                Some(Err(_)) => ParsedJsonlLine::Invalid,
                None => ParsedJsonlLine::Ignored,
            }
        }
    }
}

fn normalize_parsed_message(mut message: StoredSessionMessage) -> ParsedJsonlLine {
    if message.role.trim().is_empty() && message.content.trim().is_empty() {
        return ParsedJsonlLine::Ignored;
    }
    if message.message_id.trim().is_empty() {
        message.message_id = next_session_message_id();
        return ParsedJsonlLine::RepairedMessage(message);
    }
    ParsedJsonlLine::Message(message)
}

/// 从文件首行解析 "# chat_id: <id>"，非该格式返回 None。
fn parse_chat_id_header(line: &str) -> Option<String> {
    let line = line.trim();
    if let Some(stripped) = line.strip_prefix(CHAT_ID_HEADER_PREFIX) {
        let id = stripped.trim();
        if !id.is_empty() {
            return Some(id.to_string());
        }
    }
    None
}

fn scan_session_file(buf: &[u8]) -> SessionFileSnapshot {
    let mut messages = VecDeque::with_capacity(MAX_SESSION_ENTRIES);
    let mut message_count = 0usize;
    let mut malformed_lines = 0usize;
    let mut first = true;
    let mut needs_repair = !buf.is_empty() && !buf.ends_with(b"\n");

    for raw_line in buf.split(|&b| b == b'\n') {
        if raw_line.is_empty() {
            continue;
        }
        let Ok(line) = std::str::from_utf8(raw_line) else {
            malformed_lines = malformed_lines.saturating_add(1);
            needs_repair = true;
            first = false;
            continue;
        };
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }
        if first && parse_chat_id_header(trimmed).is_some() {
            first = false;
            continue;
        }
        first = false;
        let parsed = parse_jsonl_line(trimmed);
        let message = match parsed {
            ParsedJsonlLine::Ignored => None,
            ParsedJsonlLine::Message(message) => Some(message),
            ParsedJsonlLine::RepairedMessage(message) => {
                malformed_lines = malformed_lines.saturating_add(1);
                needs_repair = true;
                Some(message)
            }
            ParsedJsonlLine::Invalid => {
                malformed_lines = malformed_lines.saturating_add(1);
                needs_repair = true;
                None
            }
        };
        let Some(message) = message else {
            continue;
        };
        if messages.len() == MAX_SESSION_ENTRIES {
            messages.pop_front();
            needs_repair = true;
        }
        messages.push_back(message);
        message_count = messages.len();
    }

    SessionFileSnapshot {
        messages,
        message_count,
        malformed_lines,
        has_data: !buf.is_empty(),
        ends_with_newline: buf.ends_with(b"\n"),
        needs_repair,
    }
}

fn scan_session_tail(buf: &[u8], limit: usize, tail_truncated: bool) -> SessionFileSnapshot {
    let cap = limit.clamp(1, MAX_SESSION_ENTRIES);
    let mut messages = VecDeque::with_capacity(cap);
    let mut message_count = 0usize;
    let mut malformed_lines = 0usize;
    let mut first = !tail_truncated;
    let mut first_tail_line = tail_truncated;
    let mut needs_repair = !buf.is_empty() && !buf.ends_with(b"\n");

    for raw_line in buf.split(|&b| b == b'\n') {
        if first_tail_line {
            first_tail_line = false;
            if !raw_line.is_empty() {
                continue;
            }
        }
        if raw_line.is_empty() {
            continue;
        }
        let Ok(line) = std::str::from_utf8(raw_line) else {
            malformed_lines = malformed_lines.saturating_add(1);
            needs_repair = true;
            first = false;
            continue;
        };
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }
        if first && parse_chat_id_header(trimmed).is_some() {
            first = false;
            continue;
        }
        first = false;
        let parsed = parse_jsonl_line(trimmed);
        let message = match parsed {
            ParsedJsonlLine::Ignored => None,
            ParsedJsonlLine::Message(message) => Some(message),
            ParsedJsonlLine::RepairedMessage(message) => {
                malformed_lines = malformed_lines.saturating_add(1);
                needs_repair = true;
                Some(message)
            }
            ParsedJsonlLine::Invalid => {
                malformed_lines = malformed_lines.saturating_add(1);
                needs_repair = true;
                None
            }
        };
        let Some(message) = message else {
            continue;
        };
        message_count = message_count.saturating_add(1).min(MAX_SESSION_ENTRIES);
        if messages.len() == cap {
            messages.pop_front();
        }
        messages.push_back(message);
    }

    let message_count = if tail_truncated && message_count > 0 {
        MAX_SESSION_ENTRIES
    } else {
        message_count
    };
    SessionFileSnapshot {
        messages,
        message_count,
        malformed_lines,
        has_data: !buf.is_empty(),
        ends_with_newline: buf.ends_with(b"\n"),
        needs_repair,
    }
}

fn cleanup_legacy_count_sidecar(path: &Path) {
    let mut legacy_path = path.as_os_str().to_os_string();
    legacy_path.push(".c");
    let _ = super::remove_file(PathBuf::from(legacy_path));
}

#[cfg(not(any(target_arch = "xtensa", target_arch = "riscv32")))]
fn ensure_session_parent_dir(path: &Path, stage: &'static str) -> Result<()> {
    if let Some(parent) = path.parent().filter(|p| !p.as_os_str().is_empty()) {
        std::fs::create_dir_all(parent).map_err(|e| Error::io(stage, e))?;
    }
    Ok(())
}

#[cfg(any(target_arch = "xtensa", target_arch = "riscv32"))]
fn ensure_session_parent_dir(path: &Path, stage: &'static str) -> Result<()> {
    if let Some(parent) = path.parent().filter(|p| !p.as_os_str().is_empty()) {
        super::ensure_esp_storage_dir(parent, stage)?;
    }
    Ok(())
}

fn ensure_sessions_dir_exists(stage: &'static str) -> Result<()> {
    let mut dir = state_mount_path();
    dir.push(REL_PATH_SESSIONS_DIR);
    #[cfg(any(target_arch = "xtensa", target_arch = "riscv32"))]
    {
        super::ensure_esp_storage_dir(&dir, stage)?;
    }
    #[cfg(not(any(target_arch = "xtensa", target_arch = "riscv32")))]
    {
        std::fs::create_dir_all(&dir).map_err(|e| Error::io(stage, e))?;
    }
    Ok(())
}

fn read_existing_file_unlocked(path: &Path) -> Result<PsramVec<u8>> {
    let path_str = path
        .to_str()
        .ok_or_else(|| Error::config("session_read", "invalid path"))?;
    let mut file = std::fs::File::open(path_str).map_err(|e| Error::io("session_read", e))?;
    let capacity = file
        .metadata()
        .ok()
        .and_then(|meta| usize::try_from(meta.len()).ok())
        .map(|len| len.min(MAX_WRITE_SIZE))
        .unwrap_or(0);
    let mut buf = if capacity >= super::PSRAM_FILE_THRESHOLD {
        super::psram_vec_with_capacity(capacity)
    } else if capacity > 0 {
        PsramVec::from(Vec::with_capacity(capacity))
    } else {
        PsramVec::from(Vec::new())
    };
    let mut chunk = [0u8; 1024];
    loop {
        let n = file
            .read(&mut chunk)
            .map_err(|e| Error::io("session_read", e))?;
        if n == 0 {
            break;
        }
        buf.write_all(&chunk[..n])
            .map_err(|e| Error::io("session_read", e))?;
    }
    Ok(buf)
}

#[cfg(any(target_arch = "xtensa", target_arch = "riscv32"))]
fn read_file_tail_unlocked(path: &Path, max_bytes: usize) -> Result<(PsramVec<u8>, bool)> {
    use std::io::Seek as _;

    let path_str = path
        .to_str()
        .ok_or_else(|| Error::config("session_read", "invalid path"))?;
    let mut file = std::fs::File::open(path_str).map_err(|e| Error::io("session_read", e))?;
    let len = file
        .metadata()
        .ok()
        .and_then(|meta| usize::try_from(meta.len()).ok())
        .unwrap_or(0);
    if len == 0 {
        return Ok((PsramVec::from(Vec::new()), false));
    }
    let read_len = len.min(max_bytes.max(1));
    let offset = len.saturating_sub(read_len);
    file.seek(std::io::SeekFrom::Start(offset as u64))
        .map_err(|e| Error::io("session_read", e))?;
    let mut buf = if read_len >= super::PSRAM_FILE_THRESHOLD {
        super::psram_vec_with_capacity(read_len)
    } else {
        PsramVec::from(Vec::with_capacity(read_len))
    };
    let mut remaining = read_len;
    let mut chunk = [0u8; 1024];
    while remaining > 0 {
        let want = remaining.min(chunk.len());
        let n = file
            .read(&mut chunk[..want])
            .map_err(|e| Error::io("session_read", e))?;
        if n == 0 {
            break;
        }
        buf.write_all(&chunk[..n])
            .map_err(|e| Error::io("session_read", e))?;
        remaining -= n;
    }
    Ok((buf, offset > 0))
}

#[cfg(any(target_arch = "xtensa", target_arch = "riscv32"))]
fn read_session_recent_bytes_unlocked(path: &Path) -> Result<(PsramVec<u8>, bool)> {
    read_file_tail_unlocked(path, SESSION_TAIL_READ_MAX_BYTES)
}

#[cfg(not(any(target_arch = "xtensa", target_arch = "riscv32")))]
fn read_session_recent_bytes_unlocked(path: &Path) -> Result<(PsramVec<u8>, bool)> {
    read_existing_file_unlocked(path).map(|buf| (buf, false))
}

fn observe_session_append_state_unlocked(path: &Path) -> Result<SessionAppendState> {
    let len = match std::fs::metadata(path) {
        Ok(meta) => usize::try_from(meta.len()).unwrap_or(usize::MAX),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            return Ok(SessionAppendState::default());
        }
        Err(error) => return Err(Error::io("session_read", error)),
    };
    if len == 0 {
        return Ok(SessionAppendState::default());
    }
    let (buf, tail_truncated) = read_session_recent_bytes_unlocked(path)?;
    let snapshot = scan_session_tail(&buf, MAX_SESSION_ENTRIES, tail_truncated);
    Ok(SessionAppendState {
        message_count: snapshot.message_count,
        has_data: snapshot.has_data,
        ends_with_newline: snapshot.ends_with_newline,
    })
}

#[cfg(not(any(target_arch = "xtensa", target_arch = "riscv32")))]
fn write_session_body_unlocked(path: &Path, data: &[u8]) -> Result<()> {
    ensure_session_parent_dir(path, "session_write")?;
    super::write_file_unlocked(path, data, WriteDurability::Durable, "session_write")
}

fn write_session_messages_to_sink<'a>(
    chat_id: &str,
    write_header: bool,
    messages: impl IntoIterator<Item = &'a StoredSessionMessage>,
    mut write_chunk: impl FnMut(&[u8]) -> Result<()>,
) -> Result<SessionAppendState> {
    let mut written = 0usize;
    let mut message_count = 0usize;

    if write_header {
        write_chunk(CHAT_ID_HEADER_PREFIX.as_bytes())?;
        write_chunk(chat_id.as_bytes())?;
        write_chunk(b"\n")?;
        written = written
            .saturating_add(CHAT_ID_HEADER_PREFIX.len())
            .saturating_add(chat_id.len())
            .saturating_add(1);
    }

    for message in messages {
        let line = serde_json::to_string(message)
            .map_err(|e| Error::config("session_write", e.to_string()))?;
        if line.len() > MAX_SESSION_MESSAGE_LEN {
            return Err(Error::config(
                "session_write",
                format!(
                    "message serialized len {} exceeds {}",
                    line.len(),
                    MAX_SESSION_MESSAGE_LEN
                ),
            ));
        }
        write_chunk(line.as_bytes())?;
        write_chunk(b"\n")?;
        written = written.saturating_add(line.len()).saturating_add(1);
        message_count = message_count.saturating_add(1);
    }

    Ok(SessionAppendState::from_written_messages(
        message_count,
        written > 0,
    ))
}

#[cfg(not(any(target_arch = "xtensa", target_arch = "riscv32")))]
fn write_session_messages_unlocked<'a>(
    path: &Path,
    chat_id: &str,
    write_header: bool,
    messages: impl IntoIterator<Item = &'a StoredSessionMessage>,
) -> Result<SessionAppendState> {
    ensure_session_parent_dir(path, "session_write")?;
    let mut body = String::new();
    let state = write_session_messages_to_sink(chat_id, write_header, messages, |chunk| {
        let part = std::str::from_utf8(chunk)
            .map_err(|e| Error::config("session_write", e.to_string()))?;
        body.push_str(part);
        Ok(())
    })?;
    write_session_body_unlocked(path, body.as_bytes())?;
    Ok(state)
}

#[cfg(any(target_arch = "xtensa", target_arch = "riscv32"))]
fn write_session_messages_unlocked<'a>(
    path: &Path,
    chat_id: &str,
    write_header: bool,
    messages: impl IntoIterator<Item = &'a StoredSessionMessage>,
) -> Result<SessionAppendState> {
    ensure_session_parent_dir(path, "session_write")?;
    let path_str = path
        .to_str()
        .ok_or_else(|| Error::config("session_write", "invalid path"))?;
    let file = super::esp_stdio_open(path_str, "wb", "session_write_open")?;
    let write_result = write_session_messages_to_sink(chat_id, write_header, messages, |chunk| {
        super::esp_stdio_write_all(file, chunk, "session_write_body")
    })
    .and_then(|state| {
        super::esp_stdio_finish(file, "session_write_sync", WriteDurability::Durable)?;
        Ok(state)
    });
    match write_result {
        Ok(state) => {
            super::esp_stdio_close(file, "session_write_close")?;
            Ok(state)
        }
        Err(error) => {
            super::esp_stdio_close_suppress(file);
            Err(error)
        }
    }
}

fn append_session_lines_unlocked(
    path: &Path,
    write_header: bool,
    chat_id: &str,
    prepend_newline: bool,
    lines: &[String],
) -> Result<()> {
    if lines.is_empty() {
        return Ok(());
    }
    ensure_session_parent_dir(path, "session_append")?;
    let path_str = path
        .to_str()
        .ok_or_else(|| Error::config("session_append", "invalid path"))?;
    let file_len = std::fs::metadata(path_str)
        .ok()
        .and_then(|meta| usize::try_from(meta.len()).ok())
        .unwrap_or(0);
    #[cfg(any(target_arch = "xtensa", target_arch = "riscv32"))]
    {
        let mut chunks: Vec<&[u8]> = Vec::with_capacity(
            lines.len().saturating_mul(2)
                + usize::from(file_len == 0 && write_header).saturating_mul(3)
                + usize::from(file_len > 0 && prepend_newline),
        );
        if file_len == 0 && write_header {
            chunks.push(CHAT_ID_HEADER_PREFIX.as_bytes());
            chunks.push(chat_id.as_bytes());
            chunks.push(b"\n");
        }
        if file_len > 0 && prepend_newline {
            chunks.push(b"\n");
        }
        for line in lines {
            chunks.push(line.as_bytes());
            chunks.push(b"\n");
        }
        super::esp_append_file_chunks_stdio(
            path_str,
            &chunks,
            "session_append",
            WriteDurability::Durable,
        )?;
        Ok(())
    }
    #[cfg(not(any(target_arch = "xtensa", target_arch = "riscv32")))]
    {
        let open_result = OpenOptions::new().create(true).append(true).open(path_str);
        let mut file = match open_result {
            Ok(file) => file,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                let mut body = String::with_capacity(
                    CHAT_ID_HEADER_PREFIX.len()
                        + chat_id.len()
                        + lines.iter().map(|line| line.len() + 1).sum::<usize>()
                        + if write_header { 2 } else { 0 },
                );
                if write_header {
                    body.push_str(CHAT_ID_HEADER_PREFIX);
                    body.push_str(chat_id);
                    body.push('\n');
                }
                for line in lines {
                    body.push_str(line);
                    body.push('\n');
                }
                return write_session_body_unlocked(path, body.as_bytes());
            }
            Err(error) => return Err(Error::io("session_append", error)),
        };
        if file_len == 0 && write_header {
            file.write_all(CHAT_ID_HEADER_PREFIX.as_bytes())
                .and_then(|_| file.write_all(chat_id.as_bytes()))
                .and_then(|_| file.write_all(b"\n"))
                .map_err(|e| Error::io("session_append", e))?;
        }
        if file_len > 0 && prepend_newline {
            file.write_all(b"\n")
                .map_err(|e| Error::io("session_append", e))?;
        }
        for line in lines {
            file.write_all(line.as_bytes())
                .and_then(|_| file.write_all(b"\n"))
                .map_err(|e| Error::io("session_append", e))?;
        }
        finish_file_after_write(&mut file, "session_append", WriteDurability::Durable)?;
        Ok(())
    }
}

fn load_session_snapshot_unlocked(
    path: &Path,
    chat_id: &str,
    write_header: bool,
    repair_mode: SessionRepairMode,
) -> Result<SessionFileSnapshot> {
    let existing_buf = if path.exists() {
        read_existing_file_unlocked(path)?
    } else {
        PsramVec::from(Vec::new())
    };
    let snapshot = scan_session_file(&existing_buf);
    if snapshot.needs_repair {
        match repair_mode {
            SessionRepairMode::Deferred => {
                log::warn!(
                    "[{}] session needs repair chat_id={} bad_lines={} kept_messages={} repair=deferred",
                    TAG,
                    chat_id,
                    snapshot.malformed_lines,
                    snapshot.messages.len()
                );
            }
            SessionRepairMode::Immediate => {
                write_session_messages_unlocked(
                    path,
                    chat_id,
                    write_header,
                    snapshot.messages.iter(),
                )?;
                log::warn!(
                    "[{}] repaired session chat_id={} bad_lines={} kept_messages={}",
                    TAG,
                    chat_id,
                    snapshot.malformed_lines,
                    snapshot.messages.len()
                );
            }
        }
    }
    Ok(snapshot)
}

fn load_session_tail_snapshot_unlocked(
    path: &Path,
    chat_id: &str,
    limit: usize,
) -> Result<SessionFileSnapshot> {
    let (existing_buf, tail_truncated) = if path.exists() {
        read_session_recent_bytes_unlocked(path)?
    } else {
        (PsramVec::from(Vec::new()), false)
    };
    let snapshot = scan_session_tail(&existing_buf, limit, tail_truncated);
    if snapshot.needs_repair {
        log::warn!(
            "[{}] session needs repair chat_id={} bad_lines={} kept_messages={} repair=deferred",
            TAG,
            chat_id,
            snapshot.malformed_lines,
            snapshot.messages.len()
        );
    }
    Ok(snapshot)
}

fn resolve_chat_id_from_session_filename(dir: &mut PathBuf, name: &str) -> Option<String> {
    if !name.ends_with(SESSION_FILE_EXT) {
        return None;
    }
    let stem = name.trim_end_matches(SESSION_FILE_EXT);
    if stem.len() != 8 || !stem.chars().all(|c| c.is_ascii_hexdigit()) {
        return (!stem.is_empty()).then(|| stem.to_string());
    }
    dir.push(name);
    let resolved = read_file(dir.as_path())
        .ok()
        .and_then(|buf| {
            buf.split(|&b| b == b'\n')
                .next()
                .and_then(|line| std::str::from_utf8(line).ok())
                .and_then(parse_chat_id_header)
        })
        .or_else(|| (!stem.is_empty()).then(|| stem.to_string()));
    dir.pop();
    resolved
}

/// 列举 chat_id 数量上界（与 MAX_SESSION_ENTRIES 同量级）。
const MAX_LIST_CHAT_IDS: usize = 128;
/// 活跃会话 recent-cache 的 chat 数量上界，避免在 ESP 上无限放大 RAM 占用。
const RECENT_CACHE_CHAT_LIMIT: usize = 16;

/// SessionStore 的 storage 实现；单会话最多 MAX_SESSION_ENTRIES 条，超限淘汰最旧。
/// Counts are cached in-process so the hot append path only writes the JSONL body.
pub struct StorageSessionStore {
    counts: Mutex<HashMap<String, SessionAppendState>>,
    chat_ids: Mutex<Option<Vec<String>>>,
    recent: Mutex<HashMap<String, VecDeque<StoredSessionMessage>>>,
    defer_compact_on_append: bool,
}

impl Default for StorageSessionStore {
    fn default() -> Self {
        Self::new()
    }
}

impl StorageSessionStore {
    pub fn new() -> Self {
        Self {
            counts: Mutex::new(HashMap::new()),
            chat_ids: Mutex::new(None),
            recent: Mutex::new(HashMap::new()),
            defer_compact_on_append: cfg!(any(target_arch = "xtensa", target_arch = "riscv32")),
        }
    }

    #[cfg(test)]
    fn new_with_deferred_compact_for_test() -> Self {
        Self {
            counts: Mutex::new(HashMap::new()),
            chat_ids: Mutex::new(None),
            recent: Mutex::new(HashMap::new()),
            defer_compact_on_append: true,
        }
    }

    fn ensure_chat_ids_loaded(&self) -> Result<Vec<String>> {
        if let Some(cached) = self
            .chat_ids
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .as_ref()
            .cloned()
        {
            return Ok(cached);
        }

        let mut p = state_mount_path();
        p.push(REL_PATH_SESSIONS_DIR);
        ensure_sessions_dir_exists("session_list")?;
        let names = match list_dir(&p) {
            Ok(n) => n,
            Err(error) => return Err(error.with_stage("session_list")),
        };
        let mut resolved: Vec<String> = Vec::with_capacity(MAX_LIST_CHAT_IDS.min(names.len()));
        for name in names {
            let Some(chat_id) = resolve_chat_id_from_session_filename(&mut p, &name) else {
                continue;
            };
            if !chat_id.is_empty() {
                resolved.push(chat_id);
                if resolved.len() >= MAX_LIST_CHAT_IDS {
                    break;
                }
            }
        }
        resolved.sort();

        let mut guard = self.chat_ids.lock().unwrap_or_else(|e| e.into_inner());
        let cached = guard.get_or_insert_with(|| resolved.clone());
        Ok(cached.clone())
    }

    fn note_chat_id_present(&self, chat_id: &str) {
        let mut guard = self.chat_ids.lock().unwrap_or_else(|e| e.into_inner());
        if let Some(ids) = guard.as_mut() {
            let exists = ids.iter().any(|id| id == chat_id);
            if !exists && ids.len() < MAX_LIST_CHAT_IDS {
                ids.push(chat_id.to_string());
                ids.sort();
            }
        }
    }

    fn note_chat_id_removed(&self, chat_id: &str) {
        let mut guard = self.chat_ids.lock().unwrap_or_else(|e| e.into_inner());
        if let Some(ids) = guard.as_mut() {
            ids.retain(|id| id != chat_id);
        }
    }

    fn upsert_recent_cache(
        recent_cache: &mut HashMap<String, VecDeque<StoredSessionMessage>>,
        chat_id: &str,
        recent: VecDeque<StoredSessionMessage>,
    ) {
        if !recent_cache.contains_key(chat_id) && recent_cache.len() >= RECENT_CACHE_CHAT_LIMIT {
            if let Some(evict_key) = recent_cache.keys().next().cloned() {
                recent_cache.remove(&evict_key);
            }
        }
        recent_cache.insert(chat_id.to_string(), recent);
    }

    fn should_defer_compact_on_append(&self) -> bool {
        self.defer_compact_on_append
    }
}

impl SessionStore for StorageSessionStore {
    fn append(&self, chat_id: &str, role: &str, content: &str) -> Result<()> {
        self.append_batch(
            chat_id,
            &[SessionMessage {
                role: role.to_string(),
                content: content.to_string(),
            }],
        )
    }

    fn append_batch(&self, chat_id: &str, new_messages: &[SessionMessage]) -> Result<()> {
        if new_messages.is_empty() {
            return Ok(());
        }
        let stored_messages = new_messages
            .iter()
            .map(StoredSessionMessage::from_session_message)
            .collect::<Vec<_>>();
        let mut lines = Vec::with_capacity(stored_messages.len());
        for message in &stored_messages {
            let line = serde_json::to_string(message)
                .map_err(|e| Error::config("session_append", e.to_string()))?;
            if line.len() > MAX_SESSION_MESSAGE_LEN {
                return Err(Error::config(
                    "session_append",
                    format!(
                        "message serialized len {} exceeds {}",
                        line.len(),
                        MAX_SESSION_MESSAGE_LEN
                    ),
                ));
            }
            lines.push(line);
        }

        let (path, write_header) = session_path(chat_id)?;
        with_fs_lock_stage("session_append", || {
            let mut counts = self.counts.lock().unwrap_or_else(|e| e.into_inner());
            let mut recent_cache = self.recent.lock().unwrap_or_else(|e| e.into_inner());
            let (msg_count, existing_has_data, existing_ends_with_newline, existing_messages) =
                match counts.get(chat_id).copied() {
                    Some(state) => (
                        state.message_count,
                        state.has_data,
                        state.ends_with_newline,
                        None,
                    ),
                    None if self.should_defer_compact_on_append() => {
                        let state = observe_session_append_state_unlocked(&path)?;
                        counts.insert(chat_id.to_string(), state);
                        (
                            state.message_count,
                            state.has_data,
                            state.ends_with_newline,
                            None,
                        )
                    }
                    None => {
                        let snapshot = load_session_snapshot_unlocked(
                            &path,
                            chat_id,
                            write_header,
                            SessionRepairMode::Deferred,
                        )?;
                        let state = SessionAppendState::from_observed_snapshot(&snapshot);
                        let messages = snapshot.messages;
                        counts.insert(chat_id.to_string(), state);
                        (
                            state.message_count,
                            state.has_data,
                            state.ends_with_newline,
                            Some(messages),
                        )
                    }
                };
            if msg_count.saturating_add(new_messages.len()) <= MAX_SESSION_ENTRIES {
                let prepend_newline = existing_has_data && !existing_ends_with_newline;
                append_session_lines_unlocked(
                    &path,
                    write_header,
                    chat_id,
                    prepend_newline,
                    &lines,
                )?;
                if let Some(recent) = recent_cache.get_mut(chat_id) {
                    for message in &stored_messages {
                        if recent.len() == MAX_SESSION_ENTRIES {
                            recent.pop_front();
                        }
                        recent.push_back(message.clone());
                    }
                }
                counts.insert(
                    chat_id.to_string(),
                    SessionAppendState {
                        message_count: msg_count,
                        has_data: existing_has_data,
                        ends_with_newline: existing_ends_with_newline,
                    }
                    .after_appending(new_messages.len()),
                );
                drop(recent_cache);
                drop(counts);
                self.note_chat_id_present(chat_id);
                return Ok(());
            }

            if self.should_defer_compact_on_append() {
                let prepend_newline = existing_has_data && !existing_ends_with_newline;
                append_session_lines_unlocked(
                    &path,
                    write_header,
                    chat_id,
                    prepend_newline,
                    &lines,
                )?;
                if let Some(recent) = recent_cache.get_mut(chat_id) {
                    for message in &stored_messages {
                        if recent.len() == MAX_SESSION_ENTRIES {
                            recent.pop_front();
                        }
                        recent.push_back(message.clone());
                    }
                }
                counts.insert(
                    chat_id.to_string(),
                    SessionAppendState {
                        message_count: msg_count,
                        has_data: existing_has_data,
                        ends_with_newline: existing_ends_with_newline,
                    }
                    .after_appending(new_messages.len()),
                );
                log::warn!(
                    "[{}] session compact deferred chat_id={} messages_cached={} appended={}",
                    TAG,
                    chat_id,
                    msg_count,
                    new_messages.len()
                );
                drop(recent_cache);
                drop(counts);
                self.note_chat_id_present(chat_id);
                return Ok(());
            }

            // Slow path: 触顶时优先走 recent-cache，避免每次都全文件解析。
            let mut messages = if let Some(recent) = recent_cache.get(chat_id) {
                recent.clone()
            } else {
                let loaded = if let Some(messages) = existing_messages {
                    messages
                } else {
                    load_session_snapshot_unlocked(
                        &path,
                        chat_id,
                        write_header,
                        SessionRepairMode::Immediate,
                    )?
                    .messages
                };
                Self::upsert_recent_cache(&mut recent_cache, chat_id, loaded.clone());
                loaded
            };
            for msg in new_messages {
                messages.push_back(StoredSessionMessage::from_session_message(msg));
            }
            while messages.len() > MAX_SESSION_ENTRIES {
                messages.pop_front();
            }

            let state =
                write_session_messages_unlocked(&path, chat_id, write_header, messages.iter())?;
            Self::upsert_recent_cache(&mut recent_cache, chat_id, messages.clone());
            counts.insert(chat_id.to_string(), state);
            drop(recent_cache);
            drop(counts);
            self.note_chat_id_present(chat_id);
            Ok(())
        })
    }

    fn load_recent(&self, chat_id: &str, n: usize) -> Result<Vec<SessionMessage>> {
        let (path, write_header) = session_path(chat_id)?;
        let cap = n.min(MAX_SESSION_ENTRIES);
        if cap == 0 {
            return Ok(Vec::new());
        }
        if let Some(recent) = self
            .recent
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .get(chat_id)
            .cloned()
        {
            let start = recent.len().saturating_sub(cap);
            return Ok(recent
                .into_iter()
                .skip(start)
                .map(|message| message.to_session_message())
                .collect());
        }
        let (recent, append_state) = with_fs_lock_stage("session_read_recent", || {
            let _ = write_header;
            let snapshot = load_session_tail_snapshot_unlocked(&path, chat_id, cap)?;
            let start = snapshot.messages.len().saturating_sub(cap);
            let append_state = SessionAppendState::from_observed_snapshot(&snapshot);
            Ok((
                snapshot
                    .messages
                    .into_iter()
                    .skip(start)
                    .collect::<VecDeque<_>>(),
                append_state,
            ))
        })?;
        if self.should_defer_compact_on_append() {
            self.counts
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .insert(chat_id.to_string(), append_state);
        }
        if cap == MAX_SESSION_ENTRIES || self.should_defer_compact_on_append() {
            let mut recent_cache = self.recent.lock().unwrap_or_else(|e| e.into_inner());
            Self::upsert_recent_cache(&mut recent_cache, chat_id, recent.clone());
        }
        Ok(recent
            .into_iter()
            .map(|message| message.to_session_message())
            .collect())
    }

    fn load_recent_records(&self, chat_id: &str, n: usize) -> Result<Vec<SessionMessageRecord>> {
        let (path, write_header) = session_path(chat_id)?;
        let cap = n.min(MAX_SESSION_ENTRIES);
        if cap == 0 {
            return Ok(Vec::new());
        }
        let (recent, needs_repair) = with_fs_lock_stage("session_read_recent", || {
            let _ = write_header;
            let snapshot = load_session_tail_snapshot_unlocked(&path, chat_id, cap)?;
            let start = snapshot.messages.len().saturating_sub(cap);
            let needs_repair = snapshot.needs_repair;
            Ok((
                snapshot
                    .messages
                    .into_iter()
                    .skip(start)
                    .collect::<VecDeque<_>>(),
                needs_repair,
            ))
        })?;
        if cap == MAX_SESSION_ENTRIES || self.should_defer_compact_on_append() {
            let mut recent_cache = self.recent.lock().unwrap_or_else(|e| e.into_inner());
            Self::upsert_recent_cache(&mut recent_cache, chat_id, recent.clone());
        }
        if needs_repair {
            return Ok(synthesize_session_message_records(
                chat_id,
                recent
                    .into_iter()
                    .map(|message| message.to_session_message())
                    .collect(),
            ));
        }
        Ok(recent
            .into_iter()
            .map(|message| message.to_session_record())
            .collect())
    }

    fn message_count(&self, chat_id: &str) -> Result<usize> {
        if let Some(recent) = self
            .recent
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .get(chat_id)
        {
            if recent.len() == MAX_SESSION_ENTRIES {
                return Ok(MAX_SESSION_ENTRIES);
            }
        }
        let (path, write_header) = session_path(chat_id)?;
        if let Some(count) = self
            .counts
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .get(chat_id)
            .copied()
        {
            return Ok(count.message_count);
        }
        with_fs_lock_stage("session_count", || {
            let mut counts = self.counts.lock().unwrap_or_else(|e| e.into_inner());
            let state = if self.should_defer_compact_on_append() {
                let _ = write_header;
                observe_session_append_state_unlocked(&path)?
            } else {
                let snapshot = load_session_snapshot_unlocked(
                    &path,
                    chat_id,
                    write_header,
                    SessionRepairMode::Deferred,
                )?;
                SessionAppendState::from_observed_snapshot(&snapshot)
            };
            counts.insert(chat_id.to_string(), state);
            Ok(state.message_count)
        })
    }

    fn clear(&self, chat_id: &str) -> Result<()> {
        let (path, write_header) = session_path(chat_id)?;
        if write_header {
            let mut empty = String::from(CHAT_ID_HEADER_PREFIX);
            empty.push_str(chat_id);
            empty.push('\n');
            super::write_line_file(&path, empty.as_bytes())?;
        } else {
            super::write_line_file(&path, b"")?;
        }
        self.counts
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .insert(
                chat_id.to_string(),
                SessionAppendState {
                    message_count: 0,
                    has_data: write_header,
                    ends_with_newline: write_header,
                },
            );
        self.recent
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .remove(chat_id);
        cleanup_legacy_count_sidecar(&path);
        Ok(())
    }

    fn list_chat_ids(&self) -> Result<Vec<String>> {
        self.ensure_chat_ids_loaded()
    }

    fn gc_stale(&self, max_age_secs: u64) -> Result<usize> {
        let mut p = state_mount_path();
        p.push(REL_PATH_SESSIONS_DIR);
        ensure_sessions_dir_exists("session_gc")?;
        let names = match list_dir(&p) {
            Ok(n) => n,
            Err(_) => return Ok(0),
        };
        let now = std::time::SystemTime::now();
        let mut removed = 0usize;
        for name in &names {
            if !name.ends_with(SESSION_FILE_EXT) {
                continue;
            }
            p.push(name);
            let stale = match std::fs::metadata(p.as_path()) {
                Ok(meta) => match meta.modified() {
                    Ok(mtime) => now
                        .duration_since(mtime)
                        .map(|d| d.as_secs() > max_age_secs)
                        .unwrap_or(false),
                    Err(_) => false,
                },
                Err(_) => false,
            };
            if stale {
                let chat_id = resolve_chat_id_from_session_filename(&mut p, name);
                if super::remove_file(&p).is_err() {
                    p.pop();
                    continue;
                }
                if let Some(chat_id) = chat_id {
                    self.counts
                        .lock()
                        .unwrap_or_else(|e| e.into_inner())
                        .remove(chat_id.as_str());
                    self.recent
                        .lock()
                        .unwrap_or_else(|e| e.into_inner())
                        .remove(chat_id.as_str());
                    self.note_chat_id_removed(&chat_id);
                }
                cleanup_legacy_count_sidecar(&p);
                removed += 1;
                log::info!("[{}] gc: removed stale session file {:?}", TAG, name);
            }
            p.pop();
        }
        if removed > 0 {
            log::info!("[{}] gc: cleaned {} stale session files", TAG, removed);
        }
        Ok(removed)
    }

    fn delete(&self, chat_id: &str) -> Result<()> {
        let (path, _) = session_path(chat_id)?;
        self.counts
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .remove(chat_id);
        self.recent
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .remove(chat_id);
        self.note_chat_id_removed(chat_id);
        match super::remove_file(&path) {
            Ok(()) => {}
            Err(Error::Io { source, .. }) if source.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => return Err(error),
        }
        cleanup_legacy_count_sidecar(&path);
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::{
        load_session_snapshot_unlocked, scan_session_file, session_path,
        write_session_body_unlocked, write_session_messages_to_sink, SessionAppendState,
        SessionRepairMode, StorageSessionStore, StoredSessionMessage, SESSION_MESSAGE_ID_PREFIX,
    };
    use crate::memory::{
        SessionMessage, SessionStore, MAX_SESSION_ENTRIES, MAX_SESSION_MESSAGE_LEN,
        REL_PATH_SESSIONS_DIR,
    };
    use crate::platform::state_root::state_mount_path;

    #[test]
    fn counts_only_message_lines() {
        let raw = br#"# chat_id: demo
{"message_id":"msg_seed_user","role":"user","content":"hello"}

{"message_id":"msg_seed_assistant","role":"assistant","content":"world"}
"#;
        let snapshot = scan_session_file(raw);
        assert_eq!(snapshot.message_count, 2);
        assert!(!snapshot.needs_repair);
    }

    #[test]
    fn repairs_non_json_payload_lines() {
        let raw = b"note\n# chat_id: demo\n{\"message_id\":\"msg_seed\",\"role\":\"user\",\"content\":\"ok\"}\nnot-json\n";
        let snapshot = scan_session_file(raw);
        assert_eq!(snapshot.message_count, 1);
        assert!(snapshot.needs_repair);
        assert_eq!(snapshot.malformed_lines, 2);
    }

    #[test]
    fn append_batch_preserves_newline_when_fast_path_cache_is_already_warm() {
        let store = StorageSessionStore::new();
        let chat_id = format!("append-cache-{}", std::process::id());
        let (path, _) = session_path(&chat_id).expect("path");
        let _ = std::fs::remove_file(&path);
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).expect("sessions dir");
        }

        let first = SessionMessage {
            role: "user".to_string(),
            content: "hello".to_string(),
        };
        let second = SessionMessage {
            role: "assistant".to_string(),
            content: "world".to_string(),
        };
        let seeded_first = StoredSessionMessage {
            message_id: "msg_seeded_first".to_string(),
            role: first.role.clone(),
            content: first.content.clone(),
        };
        let first_line = serde_json::to_string(&seeded_first).expect("line");

        write_session_body_unlocked(&path, first_line.as_bytes()).expect("seed file");
        store
            .counts
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .insert(
                chat_id.clone(),
                SessionAppendState {
                    message_count: 1,
                    has_data: true,
                    ends_with_newline: false,
                },
            );

        store
            .append_batch(&chat_id, std::slice::from_ref(&second))
            .expect("append");

        let raw = std::fs::read(&path).expect("read");
        let snapshot = scan_session_file(&raw);
        assert_eq!(snapshot.message_count, 2);
        assert_eq!(snapshot.malformed_lines, 0);
        let raw_text = String::from_utf8_lossy(&raw);
        let lines = raw_text
            .lines()
            .filter(|line| !line.trim().is_empty())
            .collect::<Vec<_>>();
        assert_eq!(lines.len(), 2);
        assert_eq!(lines[0], first_line);
        let appended: StoredSessionMessage =
            serde_json::from_str(lines[1]).expect("appended session line");
        assert_eq!(appended.role, "assistant");
        assert_eq!(appended.content, "world");
        assert!(appended.message_id.starts_with(SESSION_MESSAGE_ID_PREFIX));

        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn load_recent_repairs_malformed_session_without_rewriting_file() {
        let store = StorageSessionStore::new();
        let chat_id = format!("load-repair-{}", std::process::id());
        let (path, write_header) = session_path(&chat_id).expect("path");
        let _ = std::fs::remove_file(&path);
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).expect("sessions dir");
        }
        let malformed =
            b"{\"role\":\"user\",\"content\":\"ok\"}\nnot-json\n{\"role\":\"assistant\",\"content\":\"still-ok\"}\n";
        write_session_body_unlocked(&path, malformed).expect("seed malformed file");

        let recent = store.load_recent(&chat_id, 8).expect("load recent");
        assert_eq!(recent.len(), 2);
        let raw = std::fs::read(&path).expect("read after load");
        assert_eq!(raw, malformed);

        let snapshot = load_session_snapshot_unlocked(
            &path,
            &chat_id,
            write_header,
            SessionRepairMode::Deferred,
        )
        .expect("snapshot");
        assert!(snapshot.needs_repair);

        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn load_recent_deferred_compact_caches_append_state_for_followup_append() {
        let store = StorageSessionStore::new_with_deferred_compact_for_test();
        let chat_id = format!("load-cache-append-state-{}", std::process::id());
        let (path, _) = session_path(&chat_id).expect("path");
        let _ = std::fs::remove_file(&path);
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).expect("sessions dir");
        }
        let malformed =
            b"{\"role\":\"user\",\"content\":\"ok\"}\nnot-json\n{\"role\":\"assistant\",\"content\":\"still-ok\"}\n";
        write_session_body_unlocked(&path, malformed).expect("seed malformed file");

        let recent = store.load_recent(&chat_id, 8).expect("load recent");
        assert_eq!(recent.len(), 2);

        let cached = store
            .counts
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .get(&chat_id)
            .copied();
        assert_eq!(
            cached,
            Some(SessionAppendState {
                message_count: 2,
                has_data: true,
                ends_with_newline: true,
            }),
            "deferred compact mode must reuse the prompt-session tail observation for append"
        );

        let appended = SessionMessage {
            role: "user".to_string(),
            content: "new turn".to_string(),
        };
        store
            .append_batch(&chat_id, std::slice::from_ref(&appended))
            .expect("append");
        let raw = std::fs::read(&path).expect("read after append");
        let raw_text = String::from_utf8_lossy(&raw);
        assert!(raw_text.contains("\nnot-json\n"));
        assert_eq!(
            raw_text
                .lines()
                .filter(|line| !line.trim().is_empty())
                .count(),
            4,
            "follow-up append must remain append-only instead of repairing on the hot path"
        );

        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn load_recent_deferred_compact_preserves_full_append_count_for_bounded_window() {
        let store = StorageSessionStore::new_with_deferred_compact_for_test();
        let chat_id = format!("load-cache-bounded-count-{}", std::process::id());
        let (path, _) = session_path(&chat_id).expect("path");
        let _ = std::fs::remove_file(&path);
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).expect("sessions dir");
        }
        let mut seeded = String::new();
        for index in 0..6 {
            let message = StoredSessionMessage {
                message_id: format!("msg_seed_{index:03}"),
                role: "user".to_string(),
                content: format!("seed {index}"),
            };
            seeded.push_str(&serde_json::to_string(&message).expect("seed line"));
            seeded.push('\n');
        }
        write_session_body_unlocked(&path, seeded.as_bytes()).expect("seed session");

        let recent = store.load_recent(&chat_id, 2).expect("load recent");
        assert_eq!(recent.len(), 2);
        assert_eq!(
            store.message_count(&chat_id).expect("message count"),
            6,
            "bounded prompt reads must not poison the append count cache"
        );

        store
            .append_batch(
                &chat_id,
                &[SessionMessage {
                    role: "assistant".to_string(),
                    content: "reply".to_string(),
                }],
            )
            .expect("append");
        assert_eq!(store.message_count(&chat_id).expect("count"), 7);

        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn append_batch_defers_cold_malformed_session_repair() {
        let store = StorageSessionStore::new();
        let chat_id = format!("append-deferred-repair-{}", std::process::id());
        let (path, _) = session_path(&chat_id).expect("path");
        let _ = std::fs::remove_file(&path);
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).expect("sessions dir");
        }
        let legacy =
            b"{\"role\":\"user\",\"content\":\"ok\"}\nnot-json\n{\"role\":\"assistant\",\"content\":\"still-ok\"}\n";
        write_session_body_unlocked(&path, legacy).expect("seed legacy file");

        let appended = SessionMessage {
            role: "user".to_string(),
            content: "new turn".to_string(),
        };
        store
            .append_batch(&chat_id, std::slice::from_ref(&appended))
            .expect("append");

        let raw = std::fs::read(&path).expect("read after append");
        let raw_text = String::from_utf8_lossy(&raw);
        assert!(
            raw_text.starts_with("{\"role\":\"user\",\"content\":\"ok\"}\nnot-json\n"),
            "cold append must not rewrite legacy lines on the agent hot path"
        );
        assert!(raw_text.contains("\nnot-json\n"));
        let lines = raw_text
            .lines()
            .filter(|line| !line.trim().is_empty())
            .collect::<Vec<_>>();
        assert_eq!(lines.len(), 4);
        let last: StoredSessionMessage =
            serde_json::from_str(lines[3]).expect("appended session line");
        assert_eq!(last.role, "user");
        assert_eq!(last.content, "new turn");
        assert!(last.message_id.starts_with(SESSION_MESSAGE_ID_PREFIX));

        let snapshot = scan_session_file(&raw);
        assert_eq!(snapshot.message_count, 3);
        assert!(snapshot.needs_repair);

        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn append_batch_preserves_newline_after_cold_append_state_observation() {
        let store = StorageSessionStore::new_with_deferred_compact_for_test();
        let chat_id = format!("append-count-cache-{}", std::process::id());
        let (path, _) = session_path(&chat_id).expect("path");
        let _ = std::fs::remove_file(&path);
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).expect("sessions dir");
        }
        let legacy_without_newline = b"{\"role\":\"user\",\"content\":\"ok\"}";
        write_session_body_unlocked(&path, legacy_without_newline).expect("seed legacy file");

        assert_eq!(
            store.message_count(&chat_id).expect("message count"),
            1,
            "cold append state must preserve the actual count while observing newline status"
        );
        store
            .append_batch(
                &chat_id,
                &[SessionMessage {
                    role: "assistant".to_string(),
                    content: "reply".to_string(),
                }],
            )
            .expect("append");

        let raw = std::fs::read(&path).expect("read after append");
        let raw_text = String::from_utf8_lossy(&raw);
        assert!(
            raw_text.starts_with("{\"role\":\"user\",\"content\":\"ok\"}\n{"),
            "deferred cache state must preserve the observed missing newline"
        );
        let snapshot = scan_session_file(&raw);
        assert_eq!(snapshot.message_count, 2);
        assert!(snapshot.needs_repair);

        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn append_batch_deferred_compact_keeps_overflow_append_only() {
        let store = StorageSessionStore::new_with_deferred_compact_for_test();
        let chat_id = format!("append-overflow-{}", std::process::id());
        let (path, _) = session_path(&chat_id).expect("path");
        let _ = std::fs::remove_file(&path);
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).expect("sessions dir");
        }
        let mut seeded = String::new();
        for index in 0..MAX_SESSION_ENTRIES {
            let message = StoredSessionMessage {
                message_id: format!("msg_seed_{index:03}"),
                role: "user".to_string(),
                content: format!("seed {index}"),
            };
            seeded.push_str(&serde_json::to_string(&message).expect("seed line"));
            seeded.push('\n');
        }
        write_session_body_unlocked(&path, seeded.as_bytes()).expect("seed full session");

        store
            .append_batch(
                &chat_id,
                &[SessionMessage {
                    role: "assistant".to_string(),
                    content: "overflow reply".to_string(),
                }],
            )
            .expect("append overflow");

        let raw = std::fs::read_to_string(&path).expect("read after overflow append");
        let lines = raw
            .lines()
            .filter(|line| !line.trim().is_empty())
            .collect::<Vec<_>>();
        assert_eq!(
            lines.len(),
            MAX_SESSION_ENTRIES + 1,
            "deferred compact mode must not rewrite the session during append"
        );
        let last: StoredSessionMessage =
            serde_json::from_str(lines.last().expect("last line")).expect("appended line");
        assert_eq!(last.content, "overflow reply");
        assert_eq!(
            store.message_count(&chat_id).expect("count"),
            MAX_SESSION_ENTRIES
        );
        let recent = store
            .load_recent(&chat_id, MAX_SESSION_ENTRIES)
            .expect("recent");
        assert_eq!(recent.len(), MAX_SESSION_ENTRIES);
        assert_eq!(
            recent.last().expect("last recent").content,
            "overflow reply"
        );

        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn partial_record_reads_do_not_poison_compacting_append_cache() {
        let store = StorageSessionStore::new();
        let chat_id = format!("partial-record-cache-{}", std::process::id());
        let (path, _) = session_path(&chat_id).expect("path");
        let _ = std::fs::remove_file(&path);
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).expect("sessions dir");
        }
        let mut seeded = String::new();
        for index in 0..MAX_SESSION_ENTRIES {
            let message = StoredSessionMessage {
                message_id: format!("msg_seed_{index:03}"),
                role: "user".to_string(),
                content: format!("seed {index:03}"),
            };
            seeded.push_str(&serde_json::to_string(&message).expect("seed line"));
            seeded.push('\n');
        }
        write_session_body_unlocked(&path, seeded.as_bytes()).expect("seed full session");

        let summary_records = store
            .load_recent_records(&chat_id, 8)
            .expect("partial record read");
        assert_eq!(summary_records.len(), 8);

        store
            .append_batch(
                &chat_id,
                &[SessionMessage {
                    role: "assistant".to_string(),
                    content: "new reply".to_string(),
                }],
            )
            .expect("append after partial record read");

        let raw = std::fs::read(&path).expect("read after compact append");
        let snapshot = scan_session_file(&raw);
        assert_eq!(
            snapshot.message_count, MAX_SESSION_ENTRIES,
            "partial /api/sessions reads must not shrink the compacted ring"
        );
        assert_eq!(
            snapshot.messages.front().expect("first compacted").content,
            "seed 001"
        );
        assert_eq!(
            snapshot.messages.back().expect("last compacted").content,
            "new reply"
        );

        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn session_rewrite_streams_full_ring_beyond_esp_single_write_cap() {
        const ESP_SINGLE_WRITE_CAP: usize = 256 * 1024;

        let content = "x".repeat(MAX_SESSION_MESSAGE_LEN - 256);
        let mut messages = Vec::with_capacity(MAX_SESSION_ENTRIES);
        for index in 0..MAX_SESSION_ENTRIES {
            messages.push(StoredSessionMessage {
                message_id: format!("msg_seed_{index:03}"),
                role: "user".to_string(),
                content: content.clone(),
            });
        }

        let mut total = 0usize;
        let mut max_chunk = 0usize;
        let state =
            write_session_messages_to_sink("stream-rewrite", false, messages.iter(), |chunk| {
                total = total.saturating_add(chunk.len());
                max_chunk = max_chunk.max(chunk.len());
                Ok(())
            })
            .expect("stream rewrite");

        assert_eq!(state.message_count, MAX_SESSION_ENTRIES);
        assert!(
            total > ESP_SINGLE_WRITE_CAP,
            "fixture must exceed the ESP generic single-write cap"
        );
        assert!(
            max_chunk <= MAX_SESSION_MESSAGE_LEN,
            "session rewrite must emit per-line chunks instead of one large body"
        );
    }

    #[test]
    fn ensure_session_parent_dir_recreates_missing_parent() {
        let parent = state_mount_path()
            .join(REL_PATH_SESSIONS_DIR)
            .join(format!("ensure-parent-{}", std::process::id()));
        let path = parent.join("session.jsonl");
        let _ = std::fs::remove_dir_all(&parent);

        super::ensure_session_parent_dir(&path, "session_test").expect("ensure parent");
        assert!(parent.is_dir());

        let _ = std::fs::remove_dir_all(&parent);
    }

    #[cfg(not(any(target_arch = "xtensa", target_arch = "riscv32")))]
    #[test]
    fn linux_load_recent_reads_full_recent_ring_beyond_legacy_tail_window() {
        let store = StorageSessionStore::new();
        let chat_id = format!("linux-full-recent-ring-{}", std::process::id());
        let (path, _) = session_path(&chat_id).expect("path");
        let _ = std::fs::remove_file(&path);
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).expect("sessions dir");
        }

        let mut seeded = String::new();
        for index in 0..MAX_SESSION_ENTRIES {
            let message = StoredSessionMessage {
                message_id: format!("msg_seed_{index:03}"),
                role: "user".to_string(),
                content: format!("seed {index:03} {}", "x".repeat(900)),
            };
            seeded.push_str(&serde_json::to_string(&message).expect("seed line"));
            seeded.push('\n');
        }
        assert!(
            seeded.len() > 64 * 1024,
            "fixture must exceed the legacy non-ESP tail read window"
        );
        write_session_body_unlocked(&path, seeded.as_bytes()).expect("seed full session");

        let recent = store
            .load_recent(&chat_id, MAX_SESSION_ENTRIES)
            .expect("recent");
        assert_eq!(recent.len(), MAX_SESSION_ENTRIES);
        assert!(recent
            .first()
            .expect("first recent")
            .content
            .starts_with("seed 000 "));
        assert!(recent
            .last()
            .expect("last recent")
            .content
            .starts_with("seed 127 "));

        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn load_recent_records_defers_repair_and_synthesizes_stable_ids() {
        let store = StorageSessionStore::new();
        let chat_id = format!("load-records-repair-{}", std::process::id());
        let (path, _) = session_path(&chat_id).expect("path");
        let _ = std::fs::remove_file(&path);
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).expect("sessions dir");
        }
        let legacy =
            b"{\"role\":\"user\",\"content\":\"ok\"}\n{\"role\":\"assistant\",\"content\":\"still-ok\"}\n";
        write_session_body_unlocked(&path, legacy).expect("seed legacy file");

        let first = store
            .load_recent_records(&chat_id, 8)
            .expect("first load recent records");
        let second = store
            .load_recent_records(&chat_id, 8)
            .expect("second load recent records");

        assert_eq!(first.len(), 2);
        assert_eq!(first, second);
        assert!(first
            .iter()
            .all(|record| record.message_id.starts_with("legacy_")));

        let raw = std::fs::read(&path).expect("read session");
        assert_eq!(raw, legacy);

        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn generated_session_message_ids_keep_fixed_prefix_and_width() {
        let first = super::next_session_message_id();
        let second = super::next_session_message_id();

        assert!(first.starts_with(SESSION_MESSAGE_ID_PREFIX));
        assert!(second.starts_with(SESSION_MESSAGE_ID_PREFIX));
        assert_eq!(
            first.len(),
            SESSION_MESSAGE_ID_PREFIX.len() + 16 + 8,
            "id format should remain msg_<16 hex nanos><8 hex counter>",
        );
        assert_eq!(second.len(), first.len());
        assert_ne!(first, second);
    }
}
