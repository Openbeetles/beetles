//! SPIFFS 实现的 SessionStore。会话文件为 {sessions_dir}/{short_id}.jsonl，有界 ring。
//! 短 id：chat_id 若不超过 MAX_CHAT_ID_FILENAME_LEN 则直接用，否则用 8 字符哈希，文件首行存 "# chat_id: <真实 id>"。

use crate::error::{Error, Result};
use crate::memory::{
    SessionMessage, SessionStore, MAX_SESSION_ENTRIES, MAX_SESSION_MESSAGE_LEN,
    REL_PATH_SESSIONS_DIR,
};
use serde_json;
use std::collections::HashMap;
use std::collections::VecDeque;
use std::fs::OpenOptions;
use std::io::{Read, Write};
use std::path::Path;
use std::path::PathBuf;
use std::sync::Mutex;

use crate::platform::psram_vec::PsramVec;
use crate::platform::state_root::state_mount_path;

use super::{list_dir, read_file, with_fs_lock, write_file, MAX_WRITE_SIZE};

const TAG: &str = "platform::spiffs::session";

/// 文件名中 chat_id 部分最大长度（不含 .jsonl），超出则用 hash 短名，满足 ESP-IDF 路径/文件名限制。
const MAX_CHAT_ID_FILENAME_LEN: usize = 20;
const SESSION_FILE_EXT: &str = ".jsonl";
const CHAT_ID_HEADER_PREFIX: &str = "# chat_id: ";

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

enum ParsedJsonlLine {
    Ignored,
    Message(SessionMessage),
    RepairedMessage(SessionMessage),
    Invalid,
}

struct SessionFileSnapshot {
    messages: VecDeque<SessionMessage>,
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
    fn from_snapshot(snapshot: &SessionFileSnapshot, write_header: bool) -> Self {
        if snapshot.needs_repair {
            let has_data = write_header || snapshot.message_count > 0;
            return Self {
                message_count: snapshot.message_count,
                has_data,
                ends_with_newline: has_data,
            };
        }
        Self {
            message_count: snapshot.message_count,
            has_data: snapshot.has_data,
            ends_with_newline: snapshot.ends_with_newline,
        }
    }

    fn from_rewritten_body(message_count: usize, body: &str) -> Self {
        let has_data = !body.is_empty();
        Self {
            message_count,
            has_data,
            ends_with_newline: has_data,
        }
    }

    fn after_appending(self, appended_messages: usize) -> Self {
        let has_data = self.has_data || appended_messages > 0;
        Self {
            message_count: self.message_count.saturating_add(appended_messages),
            has_data,
            ends_with_newline: has_data,
        }
    }
}

fn parse_jsonl_line(line: &str) -> ParsedJsonlLine {
    let line = line.trim();
    if line.is_empty() || line.starts_with('#') {
        return ParsedJsonlLine::Ignored;
    }
    match serde_json::from_str::<SessionMessage>(line) {
        Ok(m) => ParsedJsonlLine::Message(m),
        Err(_) => {
            let mut iter = serde_json::Deserializer::from_str(line).into_iter::<SessionMessage>();
            match iter.next() {
                Some(Ok(m)) => ParsedJsonlLine::RepairedMessage(m),
                Some(Err(_)) => ParsedJsonlLine::Invalid,
                None => ParsedJsonlLine::Ignored,
            }
        }
    }
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
fn ensure_session_parent_dir(_path: &Path, _stage: &'static str) -> Result<()> {
    Ok(())
}

fn ensure_sessions_dir_exists(stage: &'static str) -> Result<()> {
    let mut dir = state_mount_path();
    dir.push(REL_PATH_SESSIONS_DIR);
    #[cfg(any(target_arch = "xtensa", target_arch = "riscv32"))]
    let _ = stage;
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

fn write_session_body_unlocked(path: &Path, data: &[u8]) -> Result<()> {
    ensure_session_parent_dir(path, "session_write")?;
    let path_str = path
        .to_str()
        .ok_or_else(|| Error::config("session_write", "invalid path"))?;
    let _ = std::fs::remove_file(path_str);
    let mut file = std::fs::File::create(path_str).map_err(|e| Error::io("session_write", e))?;
    file.write_all(data)
        .map_err(|e| Error::io("session_write", e))?;
    file.sync_all().map_err(|e| Error::io("session_write", e))?;
    Ok(())
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
    file.sync_all()
        .map_err(|e| Error::io("session_append", e))?;
    Ok(())
}

fn build_session_body<'a>(
    chat_id: &str,
    write_header: bool,
    messages: impl IntoIterator<Item = &'a SessionMessage>,
) -> Result<String> {
    let mut body = String::new();
    if write_header {
        body.push_str(CHAT_ID_HEADER_PREFIX);
        body.push_str(chat_id);
        body.push('\n');
    }
    for message in messages {
        let line = serde_json::to_string(message)
            .map_err(|e| Error::config("session_write", e.to_string()))?;
        body.push_str(&line);
        body.push('\n');
    }
    Ok(body)
}

fn load_session_snapshot_unlocked(
    path: &Path,
    chat_id: &str,
    write_header: bool,
) -> Result<SessionFileSnapshot> {
    let existing_buf =
        read_existing_file_unlocked(path).unwrap_or_else(|_| PsramVec::from(Vec::new()));
    let snapshot = scan_session_file(&existing_buf);
    if snapshot.needs_repair {
        let body = build_session_body(chat_id, write_header, snapshot.messages.iter())?;
        write_session_body_unlocked(path, body.as_bytes())?;
        log::warn!(
            "[{}] repaired session chat_id={} bad_lines={} kept_messages={}",
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

/// SessionStore 的 SPIFFS 实现；单会话最多 MAX_SESSION_ENTRIES 条，超限淘汰最旧。
/// Counts are cached in-process so the hot append path only writes the JSONL body.
pub struct SpiffsSessionStore {
    counts: Mutex<HashMap<String, SessionAppendState>>,
    chat_ids: Mutex<Option<Vec<String>>>,
    recent: Mutex<HashMap<String, VecDeque<SessionMessage>>>,
}

impl Default for SpiffsSessionStore {
    fn default() -> Self {
        Self::new()
    }
}

impl SpiffsSessionStore {
    pub fn new() -> Self {
        Self {
            counts: Mutex::new(HashMap::new()),
            chat_ids: Mutex::new(None),
            recent: Mutex::new(HashMap::new()),
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
            Err(e) => {
                log::warn!("[{}] list_dir {:?} failed: {}", TAG, p, e);
                return Ok(Vec::new());
            }
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
        recent_cache: &mut HashMap<String, VecDeque<SessionMessage>>,
        chat_id: &str,
        recent: VecDeque<SessionMessage>,
    ) {
        if !recent_cache.contains_key(chat_id) && recent_cache.len() >= RECENT_CACHE_CHAT_LIMIT {
            if let Some(evict_key) = recent_cache.keys().next().cloned() {
                recent_cache.remove(&evict_key);
            }
        }
        recent_cache.insert(chat_id.to_string(), recent);
    }
}

impl SessionStore for SpiffsSessionStore {
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
        let mut lines = Vec::with_capacity(new_messages.len());
        for msg in new_messages {
            let line = serde_json::to_string(msg)
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
        with_fs_lock(|| {
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
                    None => {
                        let snapshot =
                            load_session_snapshot_unlocked(&path, chat_id, write_header)?;
                        let state = SessionAppendState::from_snapshot(&snapshot, write_header);
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
                    for msg in new_messages {
                        if recent.len() == MAX_SESSION_ENTRIES {
                            recent.pop_front();
                        }
                        recent.push_back(msg.clone());
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

            // Slow path: 触顶时优先走 recent-cache，避免每次都全文件解析。
            let mut messages = if let Some(recent) = recent_cache.get(chat_id) {
                recent.clone()
            } else {
                let loaded = if let Some(messages) = existing_messages {
                    messages
                } else {
                    load_session_snapshot_unlocked(&path, chat_id, write_header)?.messages
                };
                Self::upsert_recent_cache(&mut recent_cache, chat_id, loaded.clone());
                loaded
            };
            for msg in new_messages {
                messages.push_back(msg.clone());
            }
            while messages.len() > MAX_SESSION_ENTRIES {
                messages.pop_front();
            }

            let body = build_session_body(chat_id, write_header, messages.iter())?;
            write_session_body_unlocked(&path, body.as_bytes())?;
            Self::upsert_recent_cache(&mut recent_cache, chat_id, messages.clone());
            counts.insert(
                chat_id.to_string(),
                SessionAppendState::from_rewritten_body(messages.len(), &body),
            );
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
            return Ok(recent.into_iter().skip(start).collect());
        }
        let recent = with_fs_lock(|| {
            let snapshot = load_session_snapshot_unlocked(&path, chat_id, write_header)?;
            let start = snapshot.messages.len().saturating_sub(cap);
            Ok(snapshot
                .messages
                .into_iter()
                .skip(start)
                .collect::<VecDeque<_>>())
        })?;
        if cap == MAX_SESSION_ENTRIES {
            let mut recent_cache = self.recent.lock().unwrap_or_else(|e| e.into_inner());
            Self::upsert_recent_cache(&mut recent_cache, chat_id, recent.clone());
        }
        Ok(recent.into_iter().collect())
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
        with_fs_lock(|| {
            let mut counts = self.counts.lock().unwrap_or_else(|e| e.into_inner());
            let snapshot = load_session_snapshot_unlocked(&path, chat_id, write_header)?;
            counts.insert(
                chat_id.to_string(),
                SessionAppendState::from_snapshot(&snapshot, write_header),
            );
            Ok(snapshot.message_count)
        })
    }

    fn clear(&self, chat_id: &str) -> Result<()> {
        let (path, write_header) = session_path(chat_id)?;
        if write_header {
            let mut empty = String::from(CHAT_ID_HEADER_PREFIX);
            empty.push_str(chat_id);
            empty.push('\n');
            write_file(&path, empty.as_bytes())?;
        } else {
            write_file(&path, b"")?;
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
        if path.exists() {
            super::remove_file(&path)?;
        }
        cleanup_legacy_count_sidecar(&path);
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::{
        scan_session_file, session_path, write_session_body_unlocked, SessionAppendState,
        SpiffsSessionStore,
    };
    use crate::memory::{SessionMessage, SessionStore};

    #[test]
    fn counts_only_message_lines() {
        let raw = br#"# chat_id: demo
{"role":"user","content":"hello"}

{"role":"assistant","content":"world"}
"#;
        let snapshot = scan_session_file(raw);
        assert_eq!(snapshot.message_count, 2);
        assert!(!snapshot.needs_repair);
    }

    #[test]
    fn repairs_non_json_payload_lines() {
        let raw = b"note\n# chat_id: demo\n{\"role\":\"user\",\"content\":\"ok\"}\nnot-json\n";
        let snapshot = scan_session_file(raw);
        assert_eq!(snapshot.message_count, 1);
        assert!(snapshot.needs_repair);
        assert_eq!(snapshot.malformed_lines, 2);
    }

    #[test]
    fn append_batch_preserves_newline_when_fast_path_cache_is_already_warm() {
        let store = SpiffsSessionStore::new();
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
        let first_line = serde_json::to_string(&first).expect("line");
        let second_line = serde_json::to_string(&second).expect("line");

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
        assert_eq!(
            String::from_utf8_lossy(&raw),
            format!("{first_line}\n{second_line}\n")
        );
        let snapshot = scan_session_file(&raw);
        assert_eq!(snapshot.message_count, 2);
        assert_eq!(snapshot.malformed_lines, 0);

        let _ = std::fs::remove_file(&path);
    }
}
