//! cron 辅助函数：解析 5 字段 cron 表达式，计算下次触发时间（UTC）。
//! cron helpers: parse 5-field cron expression, compute next trigger time (UTC).

use crate::error::{Error, Result};
use crate::util::epoch_to_ymdhms;

/// 解析单个 cron 字段为允许的值集合。支持 *、N、N-M、*/S、N-M/S、N,M,P。max 为允许的最大值（含）。
pub(crate) fn parse_cron_field(s: &str, min_val: u32, max_val: u32) -> Result<Vec<u32>> {
    let s = s.trim();
    if s.is_empty() {
        return Err(Error::config("tool_cron", "empty cron field"));
    }
    if s == "*" {
        return Ok((min_val..=max_val).collect());
    }
    let mut out = Vec::new();
    for part in s.split(',') {
        let part = part.trim();
        let (range, step) = if let Some((r, st)) = part.split_once('/') {
            (
                r.trim(),
                Some(
                    st.trim()
                        .parse::<u32>()
                        .map_err(|_| Error::config("tool_cron", "invalid step"))?,
                ),
            )
        } else {
            (part, None)
        };
        if range == "*" {
            let step = step.unwrap_or(1);
            for v in (min_val..=max_val).step_by(step as usize) {
                out.push(v);
            }
            continue;
        }
        let (lo, hi) = if let Some((a, b)) = range.split_once('-') {
            let a = a
                .trim()
                .parse::<u32>()
                .map_err(|_| Error::config("tool_cron", "invalid range"))?;
            let b = b
                .trim()
                .parse::<u32>()
                .map_err(|_| Error::config("tool_cron", "invalid range"))?;
            if a > b || b > max_val || a < min_val {
                return Err(Error::config("tool_cron", "range out of bounds"));
            }
            (a, b)
        } else {
            let v = range
                .parse::<u32>()
                .map_err(|_| Error::config("tool_cron", "invalid number"))?;
            if v < min_val || v > max_val {
                return Err(Error::config("tool_cron", "value out of bounds"));
            }
            (v, v)
        };
        let step = step.unwrap_or(1);
        for v in (lo..=hi).step_by(step as usize) {
            if v >= min_val && v <= max_val {
                out.push(v);
            }
        }
    }
    out.sort_unstable();
    out.dedup();
    if out.is_empty() {
        return Err(Error::config("tool_cron", "field has no valid value"));
    }
    Ok(out)
}

fn unix_to_iso(secs: u64) -> String {
    let (y, mo, d, h, min, s) = epoch_to_ymdhms(secs);
    format!("{:04}-{:02}-{:02}T{:02}:{:02}:{:02}Z", y, mo, d, h, min, s)
}

/// 计算 cron 表达式的下次触发时间。返回 (next_unix_secs, next_iso_string)。
/// Compute next trigger time for a 5-field cron expression. Returns (unix_secs, iso_string).
pub(crate) fn next_cron_trigger(expr: &str, now_secs: u64) -> Result<(u64, String)> {
    let parts: Vec<&str> = expr.split_whitespace().collect();
    if parts.len() != 5 {
        return Err(Error::config(
            "tool_cron",
            "expr must have exactly 5 fields: min hour dom month dow",
        ));
    }
    let minutes = parse_cron_field(parts[0], 0, 59)?;
    let hours = parse_cron_field(parts[1], 0, 23)?;
    let dom = parse_cron_field(parts[2], 1, 31)?;
    let month = parse_cron_field(parts[3], 1, 12)?;
    let dow = parse_cron_field(parts[4], 0, 6)?; // 0=Sunday

    // Start from next minute so we don't return "now" if now matches.
    let mut secs = (now_secs / 60).saturating_add(1) * 60;
    const ONE_YEAR_SECS: u64 = 366 * 86400;
    let limit = now_secs.saturating_add(ONE_YEAR_SECS);

    while secs < limit {
        let (_y, mo, d, h, min, _s) = epoch_to_ymdhms(secs);
        let dow_actual = ((secs / 86400) as u32 + 4) % 7; // 0=Sunday, 1970-01-01=Thursday

        let month_ok = month.contains(&mo);
        let dom_ok = dom.contains(&d);
        let dow_ok = dow.contains(&dow_actual);
        let day_ok = dom_ok || dow_ok; // cron: dom or dow (either matches)
        let hour_ok = hours.contains(&h);
        let min_ok = minutes.contains(&min);

        if month_ok && day_ok && hour_ok && min_ok {
            let next_iso = unix_to_iso(secs);
            return Ok((secs, next_iso));
        }
        secs += 60;
    }
    Err(Error::config("tool_cron", "no matching time within 1 year"))
}
