//! 轻量工具，避免热路径堆分配；敏感信息脱敏供日志安全。
//! Lightweight helpers; secret redaction for safe logging.

use std::path::Path;
use std::sync::OnceLock;

/// 按字符边界截断内容至最多 max 个字符；不截断时零分配返回借用。
/// Truncate to at most `max` chars; returns `Cow::Borrowed` (zero alloc) when no truncation needed.
pub fn truncate_content_to_max(s: &str, max: usize) -> std::borrow::Cow<'_, str> {
    // Fast path: ASCII-dominant messages where byte len ≤ max guarantees char count ≤ max.
    if s.len() <= max {
        return std::borrow::Cow::Borrowed(s);
    }
    // Slow path: find the byte offset of the max-th char boundary in a single pass.
    match s.char_indices().nth(max) {
        Some((byte_offset, _)) => std::borrow::Cow::Owned(s[..byte_offset].to_string()),
        None => std::borrow::Cow::Borrowed(s), // fewer than max chars despite byte len > max
    }
}

/// 规范化状态根相对路径：trim、去前导 `/`、禁止 `..` 与绝对路径。
/// Normalize state-root relative path: trim, strip leading `/`, reject `..` and absolute path.
pub fn normalize_state_rel_path(path_arg: &str) -> crate::Result<String> {
    let s = path_arg.trim().trim_start_matches('/');
    if s.contains("..") {
        return Err(crate::Error::config("state_rel_path", "invalid path"));
    }
    if Path::new(s).is_absolute() {
        return Err(crate::Error::config("state_rel_path", "invalid path"));
    }
    Ok(s.to_string())
}

/// 移除 `s` 中所有非重叠的 `needle` 子串（`needle` 按字节匹配；模型标记为 ASCII）。
/// Remove all non-overlapping occurrences of `needle` without `replace` + `trim` 的多次分配。
pub fn remove_substring_all(s: &str, needle: &str) -> String {
    if needle.is_empty() {
        return s.to_string();
    }
    let mut out = String::with_capacity(s.len());
    let mut rest = s;
    while let Some(pos) = rest.find(needle) {
        out.push_str(&rest[..pos]);
        rest = &rest[pos + needle.len()..];
    }
    out.push_str(rest);
    out
}

/// 反复移除「最早出现」的任一 `needles` 子串，直到无法再匹配；再原地 trim。
/// Repeatedly remove the earliest match among `needles`, then trim in place (one allocation for body).
pub fn remove_substrings_all_trim(s: &str, needles: &[&str]) -> String {
    let mut out = remove_substrings_all_untrimmed(s, needles);
    trim_string_inplace(&mut out);
    out
}

fn remove_substrings_all_untrimmed(s: &str, needles: &[&str]) -> String {
    let mut out = String::with_capacity(s.len());
    let mut rest = s;
    loop {
        let mut best: Option<(usize, usize)> = None;
        for needle in needles {
            if needle.is_empty() {
                continue;
            }
            if let Some(pos) = rest.find(needle) {
                match best {
                    None => best = Some((pos, needle.len())),
                    Some((bp, _)) if pos < bp => best = Some((pos, needle.len())),
                    _ => {}
                }
            }
        }
        match best {
            Some((pos, len)) => {
                out.push_str(&rest[..pos]);
                rest = &rest[pos + len..];
            }
            None => {
                out.push_str(rest);
                break;
            }
        }
    }
    out
}

fn trim_string_inplace(s: &mut String) {
    let trimmed = s.trim();
    if trimmed.len() == s.len() {
        return;
    }
    if trimmed.is_empty() {
        s.clear();
        return;
    }
    let start = trimmed.as_ptr() as usize - s.as_ptr() as usize;
    let len = trimmed.len();
    if start > 0 {
        s.drain(..start);
    }
    s.truncate(len);
}

/// 按 UTF-8 字符边界截断至最多 max_bytes 字节；若发生截断则末尾追加 "…"（3 字节）。保证返回值 len() <= max_bytes。
pub fn truncate_to_byte_len(s: &str, max_bytes: usize) -> String {
    if s.len() <= max_bytes {
        return s.to_string();
    }
    const ELLIPSIS: &str = "…";
    let cap = max_bytes.saturating_sub(ELLIPSIS.len());
    // Find the largest char-aligned position ≤ cap using is_char_boundary (O(1)~O(3)).
    let mut end = cap;
    while end > 0 && !s.is_char_boundary(end) {
        end -= 1;
    }
    let mut out = String::with_capacity(end + ELLIPSIS.len());
    out.push_str(&s[..end]);
    out.push_str(ELLIPSIS);
    out
}

/// URL 查询参数 percent-encode：保留字母数字与 -_.~，其余按 UTF-8 字节编码为 %XX。供 web_search 等使用。
pub fn percent_encode_query(s: &str) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    fn is_unreserved(b: u8) -> bool {
        matches!(b, b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'.' | b'_' | b'~')
    }
    let mut out = String::with_capacity(s.len());
    for &b in s.as_bytes() {
        if is_unreserved(b) {
            out.push(b as char);
        } else if b == b' ' {
            out.push_str("%20");
        } else {
            out.push('%');
            out.push(HEX[(b >> 4) as usize] as char);
            out.push(HEX[(b & 0x0f) as usize] as char);
        }
    }
    out
}

/// 向现有 `String` 追加一个 JSON string literal（含外层双引号）。
/// 适合固定结构 JSON 手写拼装，避免 `json!` / `Value` 热路径开销。
pub fn push_json_string_escaped(out: &mut String, s: &str) {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    out.push('"');
    for ch in s.chars() {
        match ch {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            '\u{08}' => out.push_str("\\b"),
            '\u{0c}' => out.push_str("\\f"),
            c if c <= '\u{1f}' => {
                let code = c as u32 as u8;
                out.push_str("\\u00");
                out.push(HEX[(code >> 4) as usize] as char);
                out.push(HEX[(code & 0x0f) as usize] as char);
            }
            c => out.push(c),
        }
    }
    out.push('"');
}

/// URL query percent-decode：`%XX` → 单字节，`+` → 空格，其余保留。与 `percent_encode_query` 对称。
pub fn percent_decode_query(s: &str) -> String {
    let bytes = s.as_bytes();
    let len = bytes.len();
    let mut out = Vec::with_capacity(len);
    let mut i = 0;
    while i < len {
        if bytes[i] == b'%' && i + 2 < len {
            if let (Some(hi), Some(lo)) = (hex_val(bytes[i + 1]), hex_val(bytes[i + 2])) {
                out.push(hi << 4 | lo);
                i += 3;
                continue;
            }
        }
        if bytes[i] == b'+' {
            out.push(b' ');
        } else {
            out.push(bytes[i]);
        }
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

fn hex_val(b: u8) -> Option<u8> {
    match b {
        b'0'..=b'9' => Some(b - b'0'),
        b'A'..=b'F' => Some(b - b'A' + 10),
        b'a'..=b'f' => Some(b - b'a' + 10),
        _ => None,
    }
}

/// 检测常见 CJK 统一表意文字范围，供多语言 retrieval 归一化与窗口切词复用。
#[inline]
pub fn is_cjk(ch: char) -> bool {
    matches!(
        ch as u32,
        0x4E00..=0x9FFF
            | 0x3400..=0x4DBF
            | 0x20000..=0x2A6DF
            | 0x2A700..=0x2B73F
            | 0x2B740..=0x2B81F
            | 0x2B820..=0x2CEAF
            | 0xF900..=0xFAFF
            | 0x2F800..=0x2FA1F
    )
}

/// 统一 retrieval 文本归一化：保留字母数字与 CJK，其他字符折叠为空格，再压缩空白。
pub fn normalize_retrieval_text(input: &str) -> String {
    let mut out = String::with_capacity(input.len());
    let mut prev_space = false;
    for ch in input.chars() {
        if ch.is_alphanumeric() || is_cjk(ch) {
            for lower in ch.to_lowercase() {
                out.push(lower);
            }
            prev_space = false;
        } else if !prev_space {
            out.push(' ');
            prev_space = true;
        }
    }
    out.split_whitespace().collect::<Vec<_>>().join(" ")
}

fn push_unique_retrieval_term(
    out: &mut Vec<String>,
    term: &str,
    min_term_chars: usize,
    max_terms: usize,
) {
    if out.len() >= max_terms {
        return;
    }
    let trimmed = term.trim();
    if trimmed.is_empty()
        || trimmed.chars().count() < min_term_chars
        || out.iter().any(|existing| existing == trimmed)
    {
        return;
    }
    out.push(trimmed.to_string());
}

/// 统一 retrieval query 切词：支持 ASCII term 去重与 CJK 2/3-gram 窗口。
pub fn collect_retrieval_terms(
    query: &str,
    min_term_chars: usize,
    max_terms: usize,
    cjk_windows: &[usize],
) -> Vec<String> {
    let normalized = normalize_retrieval_text(query);
    if normalized.is_empty() || max_terms == 0 {
        return Vec::new();
    }
    let mut out = Vec::new();
    let mut current = String::new();
    let mut current_has_cjk = false;
    let flush_run = |out: &mut Vec<String>, run: &mut String, has_cjk: &mut bool| {
        if run.is_empty() {
            return;
        }
        let term = run.clone();
        if *has_cjk {
            push_unique_retrieval_term(out, &term, min_term_chars, max_terms);
            let chars: Vec<char> = term.chars().collect();
            for window in cjk_windows {
                if chars.len() < *window || out.len() >= max_terms {
                    continue;
                }
                for slice in chars.windows(*window) {
                    let candidate: String = slice.iter().collect();
                    push_unique_retrieval_term(out, &candidate, min_term_chars, max_terms);
                    if out.len() >= max_terms {
                        break;
                    }
                }
            }
        } else if term.chars().count() >= min_term_chars {
            push_unique_retrieval_term(out, &term, min_term_chars, max_terms);
        }
        run.clear();
        *has_cjk = false;
    };

    for ch in normalized.chars() {
        if ch.is_ascii_alphanumeric() {
            current.push(ch);
        } else if is_cjk(ch) {
            current.push(ch);
            current_has_cjk = true;
        } else {
            flush_run(&mut out, &mut current, &mut current_has_cjk);
            if out.len() >= max_terms {
                break;
            }
        }
    }
    flush_run(&mut out, &mut current, &mut current_has_cjk);
    if out.is_empty() {
        push_unique_retrieval_term(&mut out, &normalized, min_term_chars, max_terms);
    }
    out
}

fn retrieval_trigrams(value: &str) -> Vec<String> {
    let compact: Vec<char> = value.chars().filter(|ch| !ch.is_whitespace()).collect();
    if compact.is_empty() {
        return Vec::new();
    }
    if compact.len() < 3 {
        return vec![compact.iter().collect()];
    }
    let mut grams = Vec::new();
    for slice in compact.windows(3) {
        let gram: String = slice.iter().collect();
        if !grams.iter().any(|existing| existing == &gram) {
            grams.push(gram);
        }
    }
    grams
}

/// 统一 semantic-lite 重叠分：基于 char trigram overlap，适合 Linux/ESP 共享合同。
pub fn trigram_overlap_score(left: &str, right: &str, max_score: u32) -> u32 {
    let left = retrieval_trigrams(left);
    let right = retrieval_trigrams(right);
    if left.is_empty() || right.is_empty() || max_score == 0 {
        return 0;
    }
    let overlap = left
        .iter()
        .filter(|gram| right.iter().any(|candidate| candidate == *gram))
        .count();
    ((overlap as f32 / left.len().max(right.len()) as f32) * max_score as f32)
        .round()
        .max(0.0) as u32
}

/// 粗判是否像原始日志 / payload / 结构化转储，而不是可治理记忆文本。
pub fn looks_like_raw_payload_text(content: &str) -> bool {
    let trimmed = content.trim();
    if trimmed.is_empty() {
        return true;
    }
    let line_count = trimmed.lines().count();
    let bracket_like = trimmed
        .chars()
        .filter(|ch| matches!(ch, '{' | '}' | '[' | ']' | ':' | '=' | '|'))
        .count();
    let punctuation_ratio = bracket_like as f32 / trimmed.chars().count().max(1) as f32;
    let has_log_shape = trimmed
        .lines()
        .filter(|line| line.contains('[') && line.contains(']'))
        .count();
    (line_count >= 3 && punctuation_ratio > 0.18 && has_log_shape >= 2)
        || (trimmed.starts_with('{') && trimmed.ends_with('}') && punctuation_ratio > 0.12)
}

/// 程序性文本形态信号。返回值越高，越像可复用 procedure / playbook。
pub fn procedural_text_signal_count(content: &str) -> u32 {
    let trimmed = content.trim();
    if trimmed.is_empty() {
        return 0;
    }
    let line_count = trimmed.lines().count();
    let bullet_lines = trimmed
        .lines()
        .filter(|line| {
            let trimmed = line.trim_start();
            trimmed.starts_with("- ")
                || trimmed.starts_with("* ")
                || trimmed.starts_with("1.")
                || trimmed.starts_with("2.")
                || trimmed.starts_with("3.")
        })
        .count();
    let short_line_count = trimmed
        .lines()
        .filter(|line| {
            let trimmed = line.trim();
            !trimmed.is_empty() && trimmed.chars().count() <= 72
        })
        .count();
    let shellish_tokens = trimmed
        .split_whitespace()
        .filter(|token| {
            token.starts_with("--")
                || token.starts_with('/')
                || token.starts_with("./")
                || token.contains("://")
                || token.contains("::")
        })
        .count();
    let arrow_like = trimmed.matches("->").count() + trimmed.matches("=>").count();
    let newline_density = u32::from(line_count >= 3 && short_line_count >= 2);
    let bullet_density = u32::from(bullet_lines >= 2);
    let tool_shape = u32::from(shellish_tokens >= 2 || arrow_like >= 1 || trimmed.contains("```"));
    newline_density + bullet_density + tool_shape
}

// ---------- 时间/日期（与 cron、remind_at、get_time 共用） ----------

/// 闰年判定。
#[inline]
pub fn is_leap_year(y: i32) -> bool {
    (y % 4 == 0 && y % 100 != 0) || (y % 400 == 0)
}

/// 自 1970-01-01 起的天数（1970-01-01 为 0）。用于 Unix 秒换算。
pub fn days_from_epoch(year: i32, month: u32, day: u32) -> i64 {
    const MONTH_DAYS: [u32; 12] = [31, 28, 31, 30, 31, 30, 31, 31, 30, 31, 30, 31];
    let mut d = 0i64;
    for y in 1970..year {
        d += if is_leap_year(y) { 366 } else { 365 };
    }
    let mut month_days = MONTH_DAYS;
    if is_leap_year(year) {
        month_days[1] = 29;
    }
    for md in month_days.iter().take((month as usize).saturating_sub(1)) {
        d += *md as i64;
    }
    d + (day as i64) - 1
}

/// Unix 秒 → (year, month 1-12, day 1-31, hour, min, sec) UTC。
pub fn epoch_to_ymdhms(mut secs: u64) -> (i32, u32, u32, u32, u32, u32) {
    let sec = (secs % 60) as u32;
    secs /= 60;
    let min = (secs % 60) as u32;
    secs /= 60;
    let hour = (secs % 24) as u32;
    secs /= 24;
    let days = secs as i64;
    let mut year = 1970i32;
    let days_in_year = |y: i32| if is_leap_year(y) { 366i64 } else { 365i64 };
    let mut d = days;
    while d >= days_in_year(year) {
        d -= days_in_year(year);
        year += 1;
    }
    let day_of_year = d as u32;
    const MONTH_DAYS: [u32; 12] = [31, 28, 31, 30, 31, 30, 31, 31, 30, 31, 30, 31];
    let mut month_days = MONTH_DAYS;
    if is_leap_year(year) {
        month_days[1] = 29;
    }
    let mut m = 0usize;
    let mut acc = 0u32;
    while m < 12 && acc + month_days[m] <= day_of_year {
        acc += month_days[m];
        m += 1;
    }
    let month = (m as u32) + 1;
    let day = day_of_year - acc + 1;
    (year, month, day, hour, min, sec)
}

/// (y,m,d,h,min,sec) UTC → Unix 秒。
pub fn ymdhms_to_epoch(year: i32, month: u32, day: u32, hour: u32, min: u32, sec: u32) -> u64 {
    let d = days_from_epoch(year, month, day);
    (d as u64) * 86400 + (hour as u64) * 3600 + (min as u64) * 60 + (sec as u64)
}

/// 解析 ISO8601 简式或纯数字 Unix 秒字符串。
/// 支持格式：纯数字、YYYY-MM-DDTHH:MM:SS、带 Z、带时区偏移（+HH:MM / -HH:MM）、带小数秒。
pub fn parse_iso8601(s: &str) -> Option<u64> {
    let s = s.trim();
    if let Ok(n) = s.parse::<u64>() {
        return Some(n);
    }
    // Strip trailing Z or timezone offset (+HH:MM / -HH:MM)
    let s = s.trim_end_matches('Z');
    let s = if let Some(pos) = s.rfind('+') {
        // Ensure it's a timezone offset (after the T), not part of the date
        if pos > 10 {
            &s[..pos]
        } else {
            s
        }
    } else if let Some(pos) = s.rfind('-') {
        // Only treat as tz offset if after time part (pos > 16 means after HH:MM:SS)
        if pos > 16 {
            &s[..pos]
        } else {
            s
        }
    } else {
        s
    };
    let (date, time) = s.split_once('T')?;
    let mut d = date.split('-');
    let y: i32 = d.next()?.parse().ok()?;
    let m: u32 = d.next()?.parse().ok()?;
    let day: u32 = d.next()?.parse().ok()?;
    let mut t = time.split(':');
    let h: u32 = t.next()?.parse().ok()?;
    let min: u32 = t.next()?.parse().ok()?;
    // Strip fractional seconds (e.g. "00.123")
    let sec_str = t.next()?;
    let sec_str = sec_str.split('.').next()?;
    let sec: u32 = sec_str.parse().ok()?;
    if !(1..=12).contains(&m) || !(1..=31).contains(&day) || h > 23 || min > 59 || sec > 59 {
        return None;
    }
    Some(ymdhms_to_epoch(y, m, day, h, min, sec))
}

/// 当前 Unix 秒。Host 与 ESP 都通过 `SystemTime` 获取；
/// ESP 在 SNTP 同步后同样由系统时钟提供正确 Unix 时间。
pub fn current_unix_secs() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

/// 星期几名称。days = Unix 秒 / 86400，1970-01-01 (days=0) 为 Thursday。
pub fn weekday_name(days_since_epoch: u64) -> &'static str {
    const WEEKDAY: [&str; 7] = [
        "Thursday",
        "Friday",
        "Saturday",
        "Sunday",
        "Monday",
        "Tuesday",
        "Wednesday",
    ];
    WEEKDAY[(days_since_epoch % 7) as usize]
}

// ---------- 脱敏 ----------

/// 脱敏工具输出中的凭证键值对。逐行扫描，对含敏感关键字的行将 value 部分替换为 `[REDACTED]` 前缀提示。
/// 不引入 regex 依赖，适合嵌入式环境。
/// Scrub credential-like key/value lines in tool output; no regex dependency for embedded builds.
pub fn scrub_credentials(input: &str) -> String {
    if input.is_empty() {
        return String::new();
    }
    let mut out = String::with_capacity(input.len());
    let mut first = true;
    for line in input.split('\n') {
        if !first {
            out.push('\n');
        }
        first = false;
        if line_has_sensitive_kv(line) {
            out.push_str(&scrub_kv_line(line));
        } else {
            out.push_str(line);
        }
    }
    out
}

/// 对配置文本中的敏感 JSON 字符串字段做正则脱敏，确保进入 LLM 前不暴露真实秘钥。
/// Regex-based redaction for sensitive JSON string fields in config text before LLM exposure.
pub fn redact_sensitive_config_text(input: &str) -> String {
    sensitive_json_string_field_regex()
        .replace_all(input, |caps: &regex::Captures<'_>| {
            let key = caps.name("key").map(|m| m.as_str()).unwrap_or_default();
            if !is_sensitive_config_key_regex(key) {
                return caps
                    .get(0)
                    .map(|m| m.as_str().to_string())
                    .unwrap_or_default();
            }
            let value = caps.name("value").map(|m| m.as_str()).unwrap_or_default();
            if value.is_empty() {
                return caps
                    .get(0)
                    .map(|m| m.as_str().to_string())
                    .unwrap_or_default();
            }
            let decoded = decode_json_string_fragment(value);
            let redacted = redact_config_secret_value(&decoded);
            let encoded =
                serde_json::to_string(&redacted).unwrap_or_else(|_| "\"[REDACTED]\"".into());
            let prefix = caps.name("prefix").map(|m| m.as_str()).unwrap_or_default();
            format!("{prefix}{encoded}")
        })
        .into_owned()
}

fn line_has_sensitive_kv(line: &str) -> bool {
    let Some(pos) = find_kv_separator(line) else {
        return false;
    };
    is_sensitive_key(&line[..pos])
}

fn scrub_kv_line(line: &str) -> String {
    match find_kv_separator(line) {
        Some(pos) => {
            let (key_part, val_part) = line.split_at(pos + 1);
            match redact_value_fragment(val_part) {
                Some(redacted) => {
                    let mut out = String::with_capacity(key_part.len() + redacted.len());
                    out.push_str(key_part);
                    out.push_str(&redacted);
                    out
                }
                None => line.to_string(),
            }
        }
        None => line.to_string(),
    }
}

fn find_kv_separator(line: &str) -> Option<usize> {
    let bytes = line.as_bytes();
    for (i, &b) in bytes.iter().enumerate() {
        if b == b':' && bytes.get(i + 1) == Some(&b'/') {
            continue;
        }
        if b == b'=' || b == b':' {
            return Some(i);
        }
    }
    None
}

fn is_sensitive_key(raw_key: &str) -> bool {
    let key = raw_key
        .trim()
        .trim_start_matches(|c: char| c.is_ascii_punctuation() && c != '_' && c != '-')
        .trim_matches(|c: char| matches!(c, '"' | '\'' | '{' | '}' | '[' | ']' | '(' | ')'))
        .trim();
    matches!(
        (),
        _ if key.eq_ignore_ascii_case("token")
            || key.eq_ignore_ascii_case("api_key")
            || key.eq_ignore_ascii_case("api-key")
            || key.eq_ignore_ascii_case("apikey")
            || key.eq_ignore_ascii_case("client_secret")
            || key.eq_ignore_ascii_case("client-secret")
            || key.eq_ignore_ascii_case("password")
            || key.eq_ignore_ascii_case("passwd")
            || key.eq_ignore_ascii_case("secret")
            || key.eq_ignore_ascii_case("secret_key")
            || key.eq_ignore_ascii_case("secret-key")
            || key.eq_ignore_ascii_case("credential")
            || key.eq_ignore_ascii_case("authorization")
            || key.eq_ignore_ascii_case("cookie")
            || key.eq_ignore_ascii_case("access_key")
            || key.eq_ignore_ascii_case("access_token")
            || key.eq_ignore_ascii_case("refresh_token")
            || key.eq_ignore_ascii_case("private_key")
    )
}

fn sensitive_json_string_field_regex() -> &'static regex::Regex {
    static RE: OnceLock<regex::Regex> = OnceLock::new();
    RE.get_or_init(|| {
        regex::Regex::new(
            r#"(?P<prefix>"(?P<key>(?:\\.|[^"\\])*)"[[:space:]]*:[[:space:]]*)(?P<quoted>"(?P<value>(?:\\.|[^"\\])*)")"#,
        )
        .expect("valid sensitive json string regex")
    })
}

fn sensitive_config_key_regex() -> &'static regex::Regex {
    static RE: OnceLock<regex::Regex> = OnceLock::new();
    RE.get_or_init(|| {
        regex::Regex::new(
            r"(?:^|_)(?:token|api_key|apikey|api_secret|client_secret|app_secret|corp_secret|password|passwd|secret|secret_key|credential|authorization|cookie|access_key|access_token|refresh_token|private_key|search_key)(?:$|_)",
        )
        .expect("valid sensitive key regex")
    })
}

fn is_sensitive_config_key_regex(raw_key: &str) -> bool {
    let normalized = raw_key
        .chars()
        .map(|ch| {
            if ch.is_ascii_alphanumeric() {
                ch.to_ascii_lowercase()
            } else {
                '_'
            }
        })
        .collect::<String>();
    sensitive_config_key_regex().is_match(&normalized)
}

fn decode_json_string_fragment(value: &str) -> String {
    serde_json::from_str::<String>(&format!("\"{value}\"")).unwrap_or_else(|_| value.to_string())
}

fn redact_config_secret_value(value: &str) -> String {
    if value.is_empty() {
        String::new()
    } else {
        format!("{}…[REDACTED]", secret_prefix(value))
    }
}

fn redact_value_fragment(fragment: &str) -> Option<String> {
    let leading_ws_len = fragment.len().saturating_sub(fragment.trim_start().len());
    let leading_ws = &fragment[..leading_ws_len];
    let trimmed = &fragment[leading_ws_len..];
    if trimmed.is_empty() {
        return None;
    }
    let mut out = String::with_capacity(fragment.len().max(24));
    out.push_str(leading_ws);
    let mut chars = trimmed.chars();
    if let Some(quote) = chars.next().filter(|c| *c == '"' || *c == '\'') {
        let quote_len = quote.len_utf8();
        let rest = &trimmed[quote_len..];
        if let Some(close_rel) = rest.find(quote) {
            let value = &rest[..close_rel];
            let redacted = redact_secret_value(value)?;
            out.push(quote);
            out.push_str(&redacted);
            out.push(quote);
            out.push_str(&rest[close_rel + quote_len..]);
            return Some(out);
        }
        let redacted = redact_secret_value(rest)?;
        out.push(quote);
        out.push_str(&redacted);
        return Some(out);
    }

    if let Some((scheme, token)) = split_auth_scheme_value(trimmed) {
        let redacted = redact_secret_value(&format!("{} {}", scheme, token))?;
        out.push_str(&redacted);
        out.push_str(&trimmed[scheme.len() + 1 + token.len()..]);
        return Some(out);
    }

    let value_end = trimmed
        .find(|c: char| c.is_whitespace() || matches!(c, ',' | '}' | ']' | ';'))
        .unwrap_or(trimmed.len());
    let value = &trimmed[..value_end];
    let redacted = redact_secret_value(value)?;
    out.push_str(&redacted);
    out.push_str(&trimmed[value_end..]);
    Some(out)
}

fn redact_secret_value(value: &str) -> Option<String> {
    let trimmed = value.trim();
    if trimmed.len() < 8 {
        return None;
    }
    if let Some(rest) = strip_ascii_prefix(trimmed, "bearer ") {
        let prefix = secret_prefix(rest);
        return Some(format!("Bearer {}…[REDACTED]", prefix));
    }
    if let Some(rest) = strip_ascii_prefix(trimmed, "basic ") {
        let prefix = secret_prefix(rest);
        return Some(format!("Basic {}…[REDACTED]", prefix));
    }
    Some(format!("{}…[REDACTED]", secret_prefix(trimmed)))
}

fn split_auth_scheme_value(s: &str) -> Option<(&str, &str)> {
    let space = s.find(' ')?;
    let scheme = &s[..space];
    if !(scheme.eq_ignore_ascii_case("bearer") || scheme.eq_ignore_ascii_case("basic")) {
        return None;
    }
    let rest = &s[space + 1..];
    let token_end = rest
        .find(|c: char| c.is_whitespace() || matches!(c, ',' | '}' | ']' | ';'))
        .unwrap_or(rest.len());
    let token = &rest[..token_end];
    if token.is_empty() {
        None
    } else {
        Some((scheme, token))
    }
}

fn strip_ascii_prefix<'a>(s: &'a str, prefix_lower: &str) -> Option<&'a str> {
    let prefix_len = prefix_lower.len();
    if s.len() < prefix_len {
        return None;
    }
    let head = s.get(..prefix_len)?;
    if head.eq_ignore_ascii_case(prefix_lower) {
        s.get(prefix_len..)
    } else {
        None
    }
}

fn secret_prefix(s: &str) -> &str {
    let mut end = 0usize;
    for (idx, ch) in s.char_indices().take(4) {
        end = idx + ch.len_utf8();
    }
    if end == 0 {
        s
    } else {
        &s[..end]
    }
}

/// 常量时间比较，避免 token 时序侧信道。
/// Constant-time string comparison to prevent timing side-channel attacks.
pub fn constant_time_eq(a: &str, b: &str) -> bool {
    let a = a.as_bytes();
    let b = b.as_bytes();
    if a.len() != b.len() {
        return false;
    }
    a.iter()
        .zip(b.iter())
        .fold(0u8, |acc, (x, y)| acc | (x ^ y))
        == 0
}

/// 将 usize 十进制写入缓冲区，返回有效区间的 &str。调用方应传入至少 20 字节（如 `[0u8; 20]`）。
/// 供 content-length 等 header 使用，避免 format! 堆分配。
/// Caller must only write ASCII digits (b'0'+n%10); buf[i..max] is therefore valid UTF-8.
#[inline]
pub fn usize_to_decimal_buf(buf: &mut [u8], n: usize) -> &str {
    let max = buf.len().min(20);
    if max == 0 {
        // SAFETY: empty slice is trivially valid UTF-8.
        return unsafe { std::str::from_utf8_unchecked(&[]) };
    }
    if n == 0 {
        buf[0] = b'0';
        // SAFETY: single ASCII digit byte is valid UTF-8.
        return unsafe { std::str::from_utf8_unchecked(&buf[..1]) };
    }
    let mut i = max;
    let mut n = n as u64;
    while n > 0 && i > 0 {
        i -= 1;
        buf[i] = b'0' + (n % 10) as u8;
        n /= 10;
    }
    // SAFETY: all bytes in buf[i..max] are ASCII digits (0x30..0x39), which is valid UTF-8.
    unsafe { std::str::from_utf8_unchecked(&buf[i..max]) }
}

// ---------- SSRF 防护 ----------

/// 检查 URL 的 host 部分是否指向私有/本地网络地址，用于 SSRF 防护。
/// Returns true if the URL host appears to be a private/loopback address.
pub fn is_private_url(url: &str) -> bool {
    // Strip scheme
    let after_scheme = url
        .strip_prefix("https://")
        .or_else(|| url.strip_prefix("http://"))
        .unwrap_or(url);
    // Extract host (before '/' or ':' port)
    let host = after_scheme
        .split('/')
        .next()
        .unwrap_or("")
        .split(':')
        .next()
        .unwrap_or("");
    let host = host.trim();
    if host.is_empty() {
        return true; // empty host is invalid, block it
    }
    // Loopback & special
    if host == "localhost"
        || host == "0.0.0.0"
        || host.starts_with("127.")
        || host == "[::1]"
        || host.starts_with("[::ffff:127.")
    {
        return true;
    }
    // RFC 1918 private ranges
    if host.starts_with("10.") || host.starts_with("192.168.") {
        return true;
    }
    // 172.16.0.0/12 — 172.16.x.x through 172.31.x.x
    if host.starts_with("172.") {
        if let Some(second) = host.split('.').nth(1).and_then(|s| s.parse::<u8>().ok()) {
            if (16..=31).contains(&second) {
                return true;
            }
        }
    }
    // Link-local
    if host.starts_with("169.254.") || host.starts_with("[fe80:") {
        return true;
    }
    false
}

// ---------------------------------------------------------------------------
// Thread stack budget constants
// ---------------------------------------------------------------------------
//
// On ESP32/RISC-V, TLS runs inside the IDF task that owns the socket, so
// our Rust threads only need small stacks (8–16 KB).  On Linux (including
// embedded boards like Luckfox), `rustls` executes *inside* the thread that
// calls `create_http_client` / `connect_wss`, making the stack requirement
// roughly 4–8× larger.
//
// Single tuning knob: `LINUX_RUSTLS_THREAD_STACK` (non-ESP targets).
// Raise this constant if a board still overflows; all dependent constants
// follow automatically.  Verified floor: >16 KB required; 64 KB was still
// insufficient on Luckfox-class Linux embedded boards once the current
// agent/user-turn stack matured, so the host baseline is now 96 KB.  Do NOT
// write 8192 / 16384 inline in main.rs
// for any thread that calls `create_http_client` or `connect_wss` on Linux.
//
// Thread → constant mapping (Linux column):
//
// | Thread(s)                             | Constant               | ESP   | Linux |
// |---------------------------------------|------------------------|-------|-------|
// | http_config_worker_*                  | DEFAULT_GUARD_STACK_SIZE (spawn_guarded) | 8 KB | 96 KB |
// | qq_ws, feishu_ws                      | STACK_CHANNEL_WS       | 9 KB  | 96 KB |
// | agent_loop                            | STACK_AGENT_LOOP       | 48 KB | 96 KB |
// | os_outbound                           | STACK_OS_OUTBOUND      | 20 KB | 96 KB |
// | tg_sender, qq_sender, fs/dt/wc_sender | STACK_CHANNEL_SENDER   | 8 KB  | 96 KB |
// | tg_poll                               | STACK_CHANNEL_SENDER   | 8 KB  | 96 KB |
// | runtime_bootstrap                     | STACK_ESP_RUNTIME_BOOT | 32 KB | n/a   | ← ESP-only: moves Rust-heavy SPIFFS/registry/audio startup off IDF main_task
// | agent guard (bg_timer)                | n/a                    | 0 KB  | n/a   | ← ESP-only: supervised by bg_timer, no dedicated long-lived stack
// | display                               | STACK_DISPLAY          | 8 KB  | 8 KB  | ← no TLS; recover 4KB internal SRAM while keeping a safer floor above the old 6 KB budget
// | audio_io_worker                       | STACK_AUDIO_IO_STD_COMPAT | 8 KB  | 8 KB  | ← no TLS, I2S + acoustic wake state; std-compatible surface
// | config_plane_watch                    | STACK_CONFIG_PLANE_WATCH | 6 KB | 6 KB  | ← wrapper thread owns config-plane lifecycle; 4KB S3 test hit low-margin
// | wifi_worker                           | STACK_WIFI_WORKER      | 8 KB  | n/a   | ← ESP WiFi driver + scan + STA keepalive owner
// | http_snapshot_exec                     | STACK_HTTP_SNAPSHOT_WORKER | 24 KB | 24 KB | ← local read-only snapshots; P4 smoke exposed >20 KB use, S3 soak remains the ESP baseline gate
// | http_config_exec                       | STACK_HTTP_CONFIG_WORKER | 28 KB | 32 KB | ← config writes must fit normal post-startup largest-block budget
// | http_diag_exec                         | STACK_HTTP_DIAG_WORKER   | 28 KB | 32 KB | ← scan/diagnostic lane after first-screen fan-out was moved off this worker
// | dispatch                              | STACK_DISPATCH         | 6 KB  | 6 KB  | ← 常驻逻辑只做 admission/retry/cooldown，不承接重执行链
// | bg_timer                              | STACK_BG_TIMER         | 16 KB | 96 KB | ← heartbeat + cron + delayed-task wake; no SPIFFS/serde flush closures execute on this plane
// | write_back                            | runtime local          | 24 KB | 24 KB | ← governed lazy SPIFFS/serde flush worker; separate from bg_timer
// | sntp                                  | STACK_SNTP_WORKER      | 8 KB  | 96 KB | ← default guarded worker, no direct TLS call
// | cli_repl                              | STACK_CLI_REPL         | 8 KB  | 8 KB  | ← no TLS
// | voice_session                         | STACK_VOICE_CONTROL    | 8 KB  | 8 KB  | ← scheduler only; realtime WSS moved off this always-on thread
// | voice_session_worker                  | STACK_VOICE_SESSION    | 16 KB | 96 KB | ← STT + TTS HTTPS
// | voice_realtime_connect                | STACK_VOICE_REALTIME_CONNECT | 12 KB | 96 KB | ← transient realtime WSS/TLS connect budget
// | voice_realtime                        | STACK_VOICE_REALTIME   | 16 KB | 96 KB | ← steady-state realtime session owner after connect handoff
// ---------------------------------------------------------------------------

/// Linux（含嵌入式）：TLS 栈远大于 ESP 的 16KB，但不必拉到桌面级上百 KB；
/// 单一调节点，所有下游常量跟随。Luckfox Linux 实机在 64KB 预算下仍会把
/// `agent_loop` 首条真实 user turn 打爆，因此当前基线提升到 96KB；若仍溢出再试 128KB。
#[cfg(not(any(target_arch = "xtensa", target_arch = "riscv32")))]
const LINUX_RUSTLS_THREAD_STACK: usize = 96 * 1024;

/// `spawn_guarded` 默认栈：ESP 维持 8KB；Linux 与 TLS 线程同档。
#[cfg(any(target_arch = "xtensa", target_arch = "riscv32"))]
const DEFAULT_GUARD_STACK_SIZE: usize = 8192;
#[cfg(not(any(target_arch = "xtensa", target_arch = "riscv32")))]
const DEFAULT_GUARD_STACK_SIZE: usize = LINUX_RUSTLS_THREAD_STACK;

/// ESP runtime bootstrap：承接 SPIFFS recovery、runtime assembly、registry、audio init
/// 与后续 guard loop。不能继续跑在 ESP-IDF `main_task` 的窄栈上。
#[cfg(any(target_arch = "xtensa", target_arch = "riscv32"))]
pub const STACK_ESP_RUNTIME_BOOT: usize = 32 * 1024;
#[cfg(not(any(target_arch = "xtensa", target_arch = "riscv32")))]
pub const STACK_ESP_RUNTIME_BOOT: usize = 32 * 1024;

/// `qq_ws` / `feishu_ws`：外部消息通道的常驻 WSS 握手 + 帧处理。
/// 2026-05-03 S3 QQ 发布前复测显示，10KB 预算下 `qq_ws` high-water 仍保留
/// 约 4.0KB，而单轮 QQ 回复 + write-back drain 后 `heap_largest_internal=27648`，
/// 距离 TLS fragmentation Healthy headroom 只差 1KB。这里继续只收常驻
/// channel WSS 到 9KB，保留约 3KB WSS 余量，同时把最后 1KB continuous
/// internal block 还给低内存交互提示与下一轮 outbound TLS。
#[cfg(any(target_arch = "xtensa", target_arch = "riscv32"))]
pub const STACK_CHANNEL_WS: usize = 9 * 1024;
#[cfg(not(any(target_arch = "xtensa", target_arch = "riscv32")))]
pub const STACK_CHANNEL_WS: usize = LINUX_RUSTLS_THREAD_STACK;

/// 语音 realtime 建连是瞬时 TLS/WSS 连接面，不跟外部长连常驻 WSS 一起压栈。
/// 这保留实时语音能力的连接峰值余量，同时让常驻消息通道按自己的 high-water
/// 证据收窄。
#[cfg(any(target_arch = "xtensa", target_arch = "riscv32"))]
pub const STACK_VOICE_REALTIME_CONNECT: usize = 12 * 1024;
#[cfg(not(any(target_arch = "xtensa", target_arch = "riscv32")))]
pub const STACK_VOICE_REALTIME_CONNECT: usize = LINUX_RUSTLS_THREAD_STACK;

/// ESP `agent_loop` 栈预算。
///
/// 2026-04-24 实机符号化显示，QQ 入站首条真实消息在
/// `execute_turn -> prompt_context -> turn-ledger SPIFFS read` 路径上已把
/// 40KB 预算推到危险边缘；2026-04-25 首条 QQ 回复完成后又触发 pthread
/// stack overflow，说明未 boxed 的回复收尾/ledger 结算峰值不能压在 48KB 内。
/// 当前生产路径已把 heavy turn state 改为 boxed handoff；2026-05-03 S3
/// QQ 实机完整首轮消息后 `agent_loop` high-water 仍保留约 32KB，实际栈使用
/// 约 32KB。48KB 给当前路径保留约 16KB 余量，同时释放 16KB 常驻 internal SRAM
/// 给下一轮 LLM/QQ 出站 TLS。若实机 high-water 低于 8KB，必须回到拆分
/// agent hot path，而不是再盲目抬常驻栈。
pub const ESP_AGENT_LOOP_STACK_BUDGET: usize = 48 * 1024;

/// `agent_loop`：统一 agent 主执行面，承接用户消息与自治/system 作业。
#[cfg(any(target_arch = "xtensa", target_arch = "riscv32"))]
pub const STACK_AGENT_LOOP: usize = ESP_AGENT_LOOP_STACK_BUDGET;
#[cfg(not(any(target_arch = "xtensa", target_arch = "riscv32")))]
pub const STACK_AGENT_LOOP: usize = LINUX_RUSTLS_THREAD_STACK;

/// `tg_sender` / `qq_sender` / `fs_sender` / `dt_sender` / `wc_sender` / `tg_poll`：
/// 各通道出站 HTTPS 与入站轮询。ESP 8KB；Linux 96KB。
#[cfg(any(target_arch = "xtensa", target_arch = "riscv32"))]
pub const STACK_CHANNEL_SENDER: usize = 8192;
#[cfg(not(any(target_arch = "xtensa", target_arch = "riscv32")))]
pub const STACK_CHANNEL_SENDER: usize = LINUX_RUSTLS_THREAD_STACK;

/// `os_outbound`：ESP 单 active-channel 出站 worker，合并 dispatch 与 HTTP sender。
///
/// 该 worker 少掉的是常驻线程数与二级队列边界，不是 sender 深调用栈本身。
/// QQ/Feishu/Telegram active driver 会在同一栈上执行 admission、capability projection、
/// token/cache、payload render 与 HTTP POST。2026-05-03 S3 发布前实机复测显示，
/// 16KB 在首条 QQ 私聊真实回复入站后仍触发 FreeRTOS `pthread` stack overflow；
/// 但完成 active HTTP release、LLM admission 与 write-back 收口后，QQ39 的
/// thread high-water 摘要中 `os_outbound` 已不在 High/Critical 前三，说明
/// 24KB 预算下的余量高于 `wifi_worker` 约 56% 的 free margin。当前收为
/// 20KB，仍明显高于旧 16KB failure point，同时返还 4KB steady internal
/// SRAM，继续保留单 active-channel 出站面，不恢复 ESP 常驻
/// `dispatch + *_sender` 双线程组合。
#[cfg(any(target_arch = "xtensa", target_arch = "riscv32"))]
pub const STACK_OS_OUTBOUND: usize = 20 * 1024;
#[cfg(not(any(target_arch = "xtensa", target_arch = "riscv32")))]
pub const STACK_OS_OUTBOUND: usize = LINUX_RUSTLS_THREAD_STACK;

/// `dispatch`：出站调度线程。
/// 仅承接 outbound admission、cooldown replay 与 send retry；
/// delayed-task 执行已收口到其他执行面，ESP 预算继续收回到 6KB。
pub const STACK_DISPATCH: usize = 6 * 1024;

/// `display`：显示刷新线程。
/// 6KB 已在 steady-state dashboard/render/SPI flush 路径上溢出；
/// 当前先收口到 8KB，优先回收 4KB internal SRAM 给 TLS/WSS，同时保留高于旧 6KB 的安全边际。
pub const STACK_DISPLAY: usize = 8 * 1024;

/// `audio_io_worker`：I2S + acoustic wake 常驻音频 owner。
/// 它使用 Rust `Mutex` / `Condvar` 协调软件 ring buffer，因此必须运行在
/// `StdThreadCompat` surface；该常量明确表达它不是 ESP native task 的省栈入口。
pub const STACK_AUDIO_IO_STD_COMPAT: usize = 8 * 1024;

/// `voice_session`：语音会话调度线程。
/// 常驻线程只做事件 intake / 合并 / worker 拉起；realtime WSS 已迁移到独立 transient worker。
#[cfg(any(target_arch = "xtensa", target_arch = "riscv32"))]
pub const STACK_VOICE_CONTROL: usize = 8 * 1024;
#[cfg(not(any(target_arch = "xtensa", target_arch = "riscv32")))]
pub const STACK_VOICE_CONTROL: usize = 8192;

/// `voice_session_worker`：非 realtime 的语音重活线程，STT + TTS 均需 HTTPS。
#[cfg(any(target_arch = "xtensa", target_arch = "riscv32"))]
pub const STACK_VOICE_SESSION: usize = 16 * 1024;
#[cfg(not(any(target_arch = "xtensa", target_arch = "riscv32")))]
pub const STACK_VOICE_SESSION: usize = LINUX_RUSTLS_THREAD_STACK;

/// `voice_realtime`：transient realtime voice worker，承接 voice-exclusive 模式与 steady-state session loop。
/// realtime WSS/TLS connect 峰值改由 `voice_realtime_connect` 复用独立 `STACK_VOICE_REALTIME_CONNECT` 预算承接，
/// 避免把 connect 峰值和整段 session loop 永久绑在同一栈预算上。
#[cfg(any(target_arch = "xtensa", target_arch = "riscv32"))]
pub const STACK_VOICE_REALTIME: usize = 16 * 1024;
#[cfg(not(any(target_arch = "xtensa", target_arch = "riscv32")))]
pub const STACK_VOICE_REALTIME: usize = LINUX_RUSTLS_THREAD_STACK;

/// HTTP snapshot route worker：承接本地只读 SPIFFS/config/resource 快照，避免压在
/// IDF HTTPD 回调线程上，同时不为这类非 TLS 路由预留诊断/TLS worker 余量。
///
/// P4 `/api/resource` 实机高水位暴露了 20KB 预算不足；common ESP 预算仍需以
/// S3 release-size soak 作为最低准入基线，优先拆路由深度而不是继续上调通用栈。
pub const STACK_HTTP_SNAPSHOT_WORKER: usize = 24 * 1024;

/// ESP HTTP config route worker：承接 NVS/SPIFFS/serde 配置写入，避免压在
/// IDF HTTPD 回调线程上。配置面必须能在 post-startup 约 31-32KB largest block
/// 下按需启动；不能再沿用一个 48KB 通用 worker 把产品配置入口永久 admission 掉。
#[cfg(any(target_arch = "xtensa", target_arch = "riscv32", test))]
pub const STACK_HTTP_CONFIG_WORKER: usize = 28 * 1024;
#[cfg(not(any(target_arch = "xtensa", target_arch = "riscv32", test)))]
pub const STACK_HTTP_CONFIG_WORKER: usize = 32 * 1024;

/// ESP HTTP diagnostic route worker：Wi-Fi scan、hardware discovery、diagnose 与
/// channel refresh。首屏 metrics/resource/system_info 已移回轻量 immediate 路径，
/// 因此这里按可启动性重新收口，而不是保留旧通用 worker 峰值。
#[cfg(any(target_arch = "xtensa", target_arch = "riscv32", test))]
pub const STACK_HTTP_DIAG_WORKER: usize = 28 * 1024;
#[cfg(not(any(target_arch = "xtensa", target_arch = "riscv32", test)))]
pub const STACK_HTTP_DIAG_WORKER: usize = 32 * 1024;

/// ESP `bg_timer` stack budget.
///
/// `bg_timer` is the scheduler/heartbeat owner. It may enqueue or wake the
/// governed write-back plane, but must not execute SPIFFS/session/serde flush
/// closures inline. Keeping the old 24KB storage-worker budget here permanently
/// consumes internal SRAM and makes post-write-back heartbeat sampling too thin.
pub const ESP_BG_TIMER_STACK_BUDGET: usize = 16 * 1024;

/// `bg_timer`：heartbeat + cron + remind/task + self-runtime 聚合线程。
/// ESP 侧只承接调度、heartbeat 与 delayed wake；真实 SPIFFS/session/serde flush
/// 必须继续由独立 lazy `write_back` worker 执行。非 ESP 目标保留 Linux TLS
/// 统一栈预算，避免 host / Linux embedded 路径回到 16KB 旧风险。
#[cfg(any(target_arch = "xtensa", target_arch = "riscv32"))]
pub const STACK_BG_TIMER: usize = ESP_BG_TIMER_STACK_BUDGET;
#[cfg(not(any(target_arch = "xtensa", target_arch = "riscv32")))]
pub const STACK_BG_TIMER: usize = LINUX_RUSTLS_THREAD_STACK;

/// `startup_recovery`：启动期 soul/runtime recovery 线程。
/// 该线程会做恢复状态扫描与 SPIFFS 读写，不能复用普通后台线程预算。
pub const STACK_STARTUP_RECOVERY: usize = 32 * 1024;

/// `sntp`：由默认 guarded worker 启动，保持与 `spawn_guarded` 隐式预算一致。
pub const STACK_SNTP_WORKER: usize = DEFAULT_GUARD_STACK_SIZE;

/// `cli_repl`：host 交互入口，不承接 TLS/HTTP deep path。
pub const STACK_CLI_REPL: usize = 8 * 1024;

/// `config_plane_watch`：HTTP 配置/恢复面的外层 lifecycle wrapper。
///
/// 该线程启动并守住 config HTTPD lifecycle；真实 HTTPD callback 栈由
/// `ESP_HTTPD_CALLBACK_STACK` 管，deep route worker 由 route catalog 管。
/// 2026-05-03 S3 4KB 实机启动能进入 QQ WSS，但第一个 heartbeat 已出现
/// `low_margin=1`，因此 wrapper 不能继续低于 6KB。
pub const STACK_CONFIG_PLANE_WATCH: usize = 6 * 1024;

/// `wifi_worker`：ESP WiFi driver + scan + STA keepalive owner。
/// Host/Linux WiFi has a separate implementation; this value documents the ESP execution budget.
pub const STACK_WIFI_WORKER: usize = 8 * 1024;

/// 线程目标核心。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SpawnCore {
    Core0,
    Core1,
}

impl SpawnCore {
    fn as_task_core(self) -> crate::platform::task_affinity::TaskCore {
        match self {
            SpawnCore::Core0 => crate::platform::task_affinity::TaskCore::Core0,
            SpawnCore::Core1 => crate::platform::task_affinity::TaskCore::Core1,
        }
    }
}

/// 线程在 TLS 准入中的角色。
pub type HttpThreadRole = crate::orchestrator::HttpThreadRole;
/// 统一任务句柄：Linux/标准线程与 ESP 原生任务都走同一监管接口。
pub type TaskHandle = crate::platform::task_affinity::TaskHandle;

#[cfg(any(target_arch = "xtensa", target_arch = "riscv32", test))]
fn esp_should_auto_manage_task_wdt(
    name: &str,
    spawn_surface: crate::platform::task_affinity::TaskSpawnSurface,
) -> bool {
    let _ = spawn_surface;
    crate::platform::task_wdt::thread_policy_for_name(name)
        == crate::platform::task_wdt::TaskWdtThreadPolicy::Owner
}

fn should_auto_manage_task_wdt(
    name: &str,
    spawn_surface: crate::platform::task_affinity::TaskSpawnSurface,
) -> bool {
    #[cfg(any(target_arch = "xtensa", target_arch = "riscv32"))]
    {
        return esp_should_auto_manage_task_wdt(name, spawn_surface);
    }
    #[cfg(not(any(target_arch = "xtensa", target_arch = "riscv32")))]
    {
        let _ = name;
        let _ = spawn_surface;
        false
    }
}

/// Spawn a named thread with panic protection. If the closure panics, the panic is caught
/// and logged. This prevents silent thread death in long-running background loops.
/// 带 panic 保护的线程启动：闭包 panic 时捕获并记日志，避免后台线程静默消亡。
pub fn spawn_guarded<F>(name: &str, f: F)
where
    F: FnOnce() + Send + 'static,
{
    spawn_guarded_with_profile(
        name,
        DEFAULT_GUARD_STACK_SIZE,
        None,
        HttpThreadRole::Background,
        f,
    );
}

/// Spawn a named thread with custom stack size and panic protection.
/// 带自定义栈大小和 panic 保护的线程启动。
pub fn spawn_guarded_with_stack<F>(name: &str, stack_size: usize, f: F)
where
    F: FnOnce() + Send + 'static,
{
    spawn_guarded_with_profile(name, stack_size, None, HttpThreadRole::Background, f);
}

/// 带可选绑核 + TLS 准入角色 + panic 保护的线程启动。
pub fn spawn_guarded_with_profile<F>(
    name: &str,
    stack_size: usize,
    core: Option<SpawnCore>,
    role: HttpThreadRole,
    f: F,
) where
    F: FnOnce() + Send + 'static,
{
    let _ = spawn_guarded_with_profile_handle(name, stack_size, core, role, f);
}

/// 同 spawn_guarded_with_profile，但返回统一任务句柄供主线程监管。
pub fn spawn_guarded_with_profile_handle<F>(
    name: &str,
    stack_size: usize,
    core: Option<SpawnCore>,
    role: HttpThreadRole,
    f: F,
) -> std::io::Result<TaskHandle>
where
    F: FnOnce() + Send + 'static,
{
    let tag = name.to_string();
    let tag_for_spawn = tag.clone();
    let core_target = core;
    let spawn_surface = crate::platform::task_affinity::planned_spawn_surface(name);
    let auto_manage_task_wdt = should_auto_manage_task_wdt(name, spawn_surface);
    let native_std_sync_forbidden = matches!(
        spawn_surface,
        crate::platform::task_affinity::TaskSpawnSurface::EspNativeTask
    );
    let wrapped = move || {
        crate::orchestrator::set_current_http_thread_role(role);
        if auto_manage_task_wdt {
            crate::platform::task_wdt::register_current_task_to_task_wdt();
        }
        crate::runtime::thread_registry::register_thread(
            &tag,
            stack_size,
            core_target,
            role,
            spawn_surface,
        );
        log::info!(
            "[thread] started name={} core_target={:?} role={:?} surface={:?} native_std_sync_forbidden={}",
            tag,
            core_target,
            role,
            spawn_surface,
            native_std_sync_forbidden
        );
        #[cfg(feature = "thread_panic_catch")]
        {
            let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(f));
            if let Err(e) = result {
                let msg = if let Some(s) = e.downcast_ref::<&str>() {
                    (*s).to_string()
                } else if let Some(s) = e.downcast_ref::<String>() {
                    s.clone()
                } else {
                    "unknown panic".to_string()
                };
                log::error!("[{}] thread panicked: {}", tag, msg);
            }
        }
        #[cfg(not(feature = "thread_panic_catch"))]
        {
            f();
        }
        if auto_manage_task_wdt {
            crate::platform::task_wdt::unregister_current_task_from_task_wdt();
        }
        crate::runtime::thread_registry::mark_thread_stopped(&tag);
    };
    let spawn_res = crate::platform::task_affinity::spawn_named_with_affinity(
        tag_for_spawn,
        stack_size,
        core.map(SpawnCore::as_task_core),
        wrapped,
    );
    if let Err(e) = &spawn_res {
        crate::metrics::record_runtime_spawn_failure();
        log::error!(
            "[thread] spawn failed name={} core_target={:?} role={:?} surface={:?} err={}",
            name,
            core,
            role,
            spawn_surface,
            e
        );
    }
    spawn_res
}

/// 统一 transport HTTP permit facade，供非低层业务域通过批准 owner 请求网络 admission。
pub fn request_transport_http_permit(
    priority: crate::orchestrator::Priority,
    timeout: std::time::Duration,
) -> crate::Result<crate::orchestrator::HttpPermitGuard> {
    crate::orchestrator::request_http_permit(priority, timeout)
}

#[cfg(test)]
mod scrub_credentials_tests {
    use super::*;

    #[test]
    fn scrub_api_key() {
        let s = scrub_credentials("api_key: sk-1234abcdef");
        assert!(s.contains("[REDACTED]") && s.contains("sk-1"));
    }

    #[test]
    fn scrub_json_token() {
        let s = scrub_credentials(r#"{"token": "eyJhbGciOiJ..."}"#);
        assert!(s.contains("[REDACTED]"));
    }

    #[test]
    fn no_scrub_normal() {
        let s = scrub_credentials("result: 42 items found");
        assert_eq!(s, "result: 42 items found");
    }

    #[test]
    fn scrub_multibyte_val_no_panic() {
        // Chinese chars (3 bytes each) as token value — must not panic on char boundary
        let s = scrub_credentials("token: 你好世界长密钥abc");
        assert!(s.contains("[REDACTED]"));
    }

    #[test]
    fn scrub_multiline() {
        let input = "result: ok\napi_key: sk-secret1234\nother: data";
        let s = scrub_credentials(input);
        assert!(s.contains("result: ok"));
        assert!(s.contains("[REDACTED]"));
        assert!(s.contains("other: data"));
        let lines: Vec<&str> = s.lines().collect();
        assert_eq!(lines.len(), 3);
    }

    #[test]
    fn no_scrub_url_scheme() {
        // "authorization" is a sensitive key, but ensure function doesn't crash on URL values
        let s = scrub_credentials("authorization: Bearer eyJhbGci0iJIUzI1NiJ9");
        assert!(s.contains("[REDACTED]"));
    }

    #[test]
    fn no_scrub_short_value() {
        // value shorter than 8 bytes: should NOT redact (likely not a real secret)
        let s = scrub_credentials("token: abc");
        assert!(!s.contains("[REDACTED]"));
    }

    #[test]
    fn scrub_preserves_json_suffix() {
        let s = scrub_credentials(r#"{"access_token":"abcdefghi123","ok":true}"#);
        assert!(s.contains(r#""access_token":"abcd…[REDACTED]""#));
        assert!(s.ends_with('}'));
    }

    #[test]
    fn scrub_bearer_keeps_scheme() {
        let s = scrub_credentials("authorization: Bearer eyJhbGciOiJIUzI1NiIsInR5cCI6IkpXVCJ9");
        assert!(s.contains("Bearer eyJh…[REDACTED]"));
    }

    #[test]
    fn scrub_cookie_like_value() {
        let s = scrub_credentials("cookie=sessionid=abc123456789; Path=/; HttpOnly");
        assert!(s.contains("[REDACTED]"));
        assert!(s.contains("Path=/"));
    }

    #[test]
    fn does_not_redact_plain_error_text_with_sensitive_words() {
        let s = scrub_credentials("error: authorization header missing");
        assert_eq!(s, "error: authorization header missing");
    }

    #[test]
    fn redact_sensitive_config_text_masks_multiple_secret_fields() {
        let input = r#"{"tg_token":"123456:live-secret","feishu_app_secret":"fs-secret","enabled":"telegram"}"#;
        let s = redact_sensitive_config_text(input);
        assert!(s.contains("[REDACTED]"));
        assert!(s.contains(r#""enabled":"telegram""#));
        assert!(!s.contains("123456:live-secret"));
        assert!(!s.contains("fs-secret"));
    }

    #[test]
    fn redact_sensitive_config_text_masks_nested_secret_fields() {
        let input = r#"{"audio":{"speech":{"api_secret":"baidu-secret","api_key":"baidu-key"}},"model":"x"}"#;
        let s = redact_sensitive_config_text(input);
        assert!(s.contains("[REDACTED]"));
        assert!(s.contains(r#""model":"x""#));
        assert!(!s.contains("baidu-secret"));
        assert!(!s.contains("baidu-key"));
    }

    #[test]
    fn push_json_string_escaped_handles_quotes_and_controls() {
        let mut out = String::new();
        push_json_string_escaped(&mut out, "a\"\n\t\\b");
        assert_eq!(out, "\"a\\\"\\n\\t\\\\b\"");
    }
}

#[cfg(test)]
mod thread_stack_budget_tests {
    use super::*;

    #[cfg(not(any(target_arch = "xtensa", target_arch = "riscv32")))]
    #[test]
    fn linux_tls_threads_share_host_stack_budget() {
        assert_eq!(STACK_BG_TIMER, LINUX_RUSTLS_THREAD_STACK);
        assert_eq!(STACK_AGENT_LOOP, LINUX_RUSTLS_THREAD_STACK);
        assert_eq!(STACK_CHANNEL_WS, LINUX_RUSTLS_THREAD_STACK);
        assert_eq!(STACK_VOICE_REALTIME_CONNECT, LINUX_RUSTLS_THREAD_STACK);
        assert_eq!(STACK_CHANNEL_SENDER, LINUX_RUSTLS_THREAD_STACK);
        assert_eq!(STACK_VOICE_SESSION, LINUX_RUSTLS_THREAD_STACK);
        assert_eq!(STACK_VOICE_REALTIME, LINUX_RUSTLS_THREAD_STACK);
        assert_eq!(STACK_AUDIO_IO_STD_COMPAT, 8 * 1024);
        assert_eq!(LINUX_RUSTLS_THREAD_STACK, 96 * 1024);
    }

    #[test]
    fn esp_agent_loop_stack_keeps_prompt_spiffs_headroom() {
        const {
            assert!(
                ESP_AGENT_LOOP_STACK_BUDGET >= 48 * 1024,
                "ESP agent_loop needs headroom for first real inbound prompt and reply settlement"
            );
            assert!(
                ESP_AGENT_LOOP_STACK_BUDGET <= 64 * 1024,
                "ESP agent_loop must not copy the Linux 96KB budget without first freeing resident SRAM"
            );
        }
    }

    #[test]
    fn esp_bg_timer_stack_is_scheduler_only_budget() {
        const {
            assert!(
                ESP_BG_TIMER_STACK_BUDGET == 16 * 1024,
                "bg_timer must not keep the old write-back worker stack budget"
            );
        }
    }

    #[test]
    fn config_plane_watch_keeps_recovery_wrapper_margin() {
        const {
            assert!(
                STACK_CONFIG_PLANE_WATCH == 6 * 1024,
                "config_plane_watch must keep the S3-proven 6KB recovery wrapper margin"
            );
        }
    }

    #[cfg(any(target_arch = "xtensa", target_arch = "riscv32"))]
    #[test]
    fn esp_agent_guard_does_not_allocate_a_dedicated_stack_after_bootstrap() {
        const {
            assert!(
                STACK_BG_TIMER <= ESP_BG_TIMER_STACK_BUDGET,
                "agent guard is serviced by bg_timer and must not restore a dedicated ESP stack"
            );
        }
    }

    #[test]
    fn os_outbound_stack_accounts_for_active_sender_call_depth_without_linux_budget() {
        const {
            assert!(
                STACK_OS_OUTBOUND >= 20 * 1024,
                "os_outbound runs dispatch admission plus measured active channel HTTP send depth on one stack"
            );
        }
        #[cfg(any(target_arch = "xtensa", target_arch = "riscv32"))]
        {
            const {
                assert!(
                    STACK_OS_OUTBOUND >= STACK_DISPATCH + STACK_CHANNEL_SENDER,
                    "merged ESP os_outbound must budget for the deepest legacy dispatch+sender call path"
                );
                assert!(
                    STACK_OS_OUTBOUND <= 24 * 1024,
                    "ESP os_outbound should stay below route/agent budgets while covering measured active-channel sender depth"
                );
            }
        }
    }

    #[test]
    fn http_snapshot_worker_stack_covers_resource_snapshot_high_water() {
        const OBSERVED_RESOURCE_SNAPSHOT_STACK_USED_BYTES_FROM_P4_SMOKE: usize = 20_300;
        const MIN_SNAPSHOT_STACK_HEADROOM_BYTES: usize = 4 * 1024;

        const {
            assert!(
                STACK_HTTP_SNAPSHOT_WORKER
                    >= OBSERVED_RESOURCE_SNAPSHOT_STACK_USED_BYTES_FROM_P4_SMOKE
                        + MIN_SNAPSHOT_STACK_HEADROOM_BYTES,
                "http_snapshot_exec must keep headroom over the P4 /api/resource risk sample"
            );
        }
    }
}

#[cfg(test)]
mod task_wdt_spawn_policy_tests {
    use super::*;

    #[test]
    fn esp_auto_task_wdt_management_is_limited_to_long_lived_owners() {
        assert!(esp_should_auto_manage_task_wdt(
            "agent_loop",
            crate::platform::task_affinity::TaskSpawnSurface::StdThreadCompat
        ));
        assert!(esp_should_auto_manage_task_wdt(
            "wifi_worker",
            crate::platform::task_affinity::TaskSpawnSurface::StdThreadCompat
        ));
        assert!(esp_should_auto_manage_task_wdt(
            "audio_io_worker",
            crate::platform::task_affinity::TaskSpawnSurface::StdThreadCompat
        ));
        assert!(!esp_should_auto_manage_task_wdt(
            "native_worker",
            crate::platform::task_affinity::TaskSpawnSurface::EspNativeTask
        ));
        assert!(!esp_should_auto_manage_task_wdt(
            "http_config_exec",
            crate::platform::task_affinity::TaskSpawnSurface::StdThreadCompat
        ));
        assert!(!esp_should_auto_manage_task_wdt(
            "voice_session_worker",
            crate::platform::task_affinity::TaskSpawnSurface::StdThreadCompat
        ));
    }
}

#[cfg(test)]
mod marker_string_tests {
    use super::*;
    use crate::constants::{AGENT_MARKER_MARK_IMPORTANT, AGENT_MARKER_SIGNAL_COMFORT};

    #[test]
    fn remove_both_markers_then_trim() {
        let s = remove_substrings_all_trim(
            "a [MARK_IMPORTANT] b [SIGNAL:comfort] c",
            &[AGENT_MARKER_MARK_IMPORTANT, AGENT_MARKER_SIGNAL_COMFORT],
        );
        assert_eq!(s, "a  b  c");
    }
}
