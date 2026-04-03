use crate::platform::SkillStorage;
use crate::skills::{
    get_skill_content, runtime_skill_name_for_topic, write_skill, RuntimeSkillWrite,
    MAX_SKILL_CONTENT_LEN,
};
use crate::util::truncate_content_to_max;

const RUNTIME_SKILL_MARKER: &str = "<!-- beetle:runtime-skill -->";
const MAX_RUNTIME_SKILL_HITS: usize = 4;
const MAX_RUNTIME_SKILL_CITATIONS: usize = 8;
const MIN_RUNTIME_SKILL_BLOCK_LEN: usize = 180;
const RUNTIME_SKILL_TOUCH_INTERVAL_SECS: u64 = 6 * 60 * 60;
const RUNTIME_SKILL_STALE_AFTER_SECS: u64 = 90 * 86_400;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RuntimeSkillRecord {
    pub name: String,
    pub title: String,
    pub topic: String,
    pub summary: String,
    pub procedure: String,
    pub citations: Vec<String>,
    pub source_chat_id: Option<String>,
    pub observed_at: u64,
    pub updated_at: u64,
    pub last_used_at: Option<u64>,
    pub use_count: u32,
    pub quality_score: u8,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RuntimeSkillHit {
    pub record: RuntimeSkillRecord,
    pub score: u32,
    pub reasons: Vec<String>,
}

#[derive(Clone, Debug)]
struct RuntimeSkillUpsertInput {
    name: String,
    title: String,
    topic: String,
    summary: String,
    procedure: String,
    citations: Vec<String>,
    source_chat_id: Option<String>,
    observed_at: u64,
    updated_at: u64,
}

pub fn is_runtime_skill_name(name: &str) -> bool {
    name.starts_with("runtime_skill__")
}

pub fn retrieve_runtime_skill_hits(
    storage: &dyn SkillStorage,
    query: &str,
    preferred_chat_id: Option<&str>,
    now_secs: u64,
    limit: usize,
) -> Vec<RuntimeSkillHit> {
    let normalized_query = normalize_runtime_skill_text(query);
    if normalized_query.is_empty() {
        return Vec::new();
    }
    let terms = collect_runtime_skill_terms(&normalized_query);
    let mut hits = list_runtime_skill_records(storage)
        .into_iter()
        .filter_map(|record| {
            score_runtime_skill_record(
                record,
                &normalized_query,
                &terms,
                preferred_chat_id,
                now_secs,
            )
        })
        .collect::<Vec<_>>();
    hits.sort_by(|a, b| {
        b.score
            .cmp(&a.score)
            .then_with(|| b.record.quality_score.cmp(&a.record.quality_score))
            .then_with(|| b.record.last_used_at.cmp(&a.record.last_used_at))
            .then_with(|| b.record.updated_at.cmp(&a.record.updated_at))
            .then_with(|| a.record.name.cmp(&b.record.name))
    });
    let mut selected = Vec::with_capacity(limit.min(MAX_RUNTIME_SKILL_HITS));
    let mut seen_topics = Vec::new();
    for hit in hits {
        if selected.len() >= limit.min(MAX_RUNTIME_SKILL_HITS) {
            break;
        }
        let key =
            normalize_runtime_skill_text(&format!("{} {}", hit.record.topic, hit.record.summary));
        if !key.is_empty()
            && seen_topics.iter().any(|existing: &String| {
                existing == &key || existing.contains(&key) || key.contains(existing)
            })
        {
            continue;
        }
        if !key.is_empty() {
            seen_topics.push(key);
        }
        selected.push(hit);
    }
    selected
}

pub fn touch_runtime_skill_hits(
    storage: &dyn SkillStorage,
    hits: &[RuntimeSkillHit],
    now_secs: u64,
) -> usize {
    let mut changed = 0usize;
    for hit in hits {
        let mut record = hit.record.clone();
        if record.last_used_at.is_some_and(|previous| {
            now_secs.saturating_sub(previous) < RUNTIME_SKILL_TOUCH_INTERVAL_SECS
        }) {
            continue;
        }
        record.last_used_at = Some(now_secs);
        record.use_count = record.use_count.saturating_add(1);
        record.quality_score = compute_runtime_skill_quality(&record);
        if write_runtime_skill_record(storage, &record).is_ok() {
            changed = changed.saturating_add(1);
        }
    }
    changed
}

pub fn build_runtime_skill_recall_block(
    storage: &dyn SkillStorage,
    query: &str,
    preferred_chat_id: Option<&str>,
    now_secs: u64,
    max_chars: usize,
) -> Option<String> {
    if max_chars < MIN_RUNTIME_SKILL_BLOCK_LEN {
        return None;
    }
    let mut hits = retrieve_runtime_skill_hits(storage, query, preferred_chat_id, now_secs, 3);
    if hits.is_empty() {
        hits = fallback_runtime_skill_hits(storage, preferred_chat_id, now_secs, 2);
    }
    if hits.is_empty() {
        return None;
    }
    let mut out = String::from(
        "## Runtime skills\nProcedural memory distilled from proven prior operations. Reuse the method when it fits, but adapt it to current constraints instead of quoting it blindly.\n",
    );
    let mut appended = 0usize;
    for hit in &hits {
        let reasons_joined = hit.reasons.join(", ");
        let reasons = truncate_content_to_max(&reasons_joined, 140);
        let citations = hit
            .record
            .citations
            .iter()
            .take(2)
            .cloned()
            .collect::<Vec<_>>()
            .join(", ");
        let line = if citations.is_empty() {
            format!(
                "- [{}] {} (topic: {}; why: {}; quality={}; reused={})",
                hit.record.title,
                truncate_content_to_max(hit.record.summary.trim(), 120),
                hit.record.topic,
                reasons,
                hit.record.quality_score,
                hit.record.use_count,
            )
        } else {
            format!(
                "- [{}] {} (topic: {}; why: {}; quality={}; reused={}; provenance={})",
                hit.record.title,
                truncate_content_to_max(hit.record.summary.trim(), 120),
                hit.record.topic,
                reasons,
                hit.record.quality_score,
                hit.record.use_count,
                citations,
            )
        };
        let remaining = max_chars.saturating_sub(out.len()).saturating_sub(1);
        if line.len() > remaining {
            if remaining < 64 {
                break;
            }
            out.push_str(&truncate_content_to_max(&line, remaining));
            out.push('\n');
            appended = appended.saturating_add(1);
            break;
        }
        out.push_str(&line);
        out.push('\n');
        appended = appended.saturating_add(1);
    }
    if appended == 0 {
        None
    } else {
        let _ = touch_runtime_skill_hits(storage, &hits[..appended], now_secs);
        Some(out.trim_end().to_string())
    }
}

fn fallback_runtime_skill_hits(
    storage: &dyn SkillStorage,
    preferred_chat_id: Option<&str>,
    now_secs: u64,
    limit: usize,
) -> Vec<RuntimeSkillHit> {
    let mut hits = list_runtime_skill_records(storage)
        .into_iter()
        .map(|record| {
            let mut score = (record.quality_score / 8) as u32;
            let mut reasons = vec!["fallback procedural memory".to_string()];
            if let Some(chat_id) = preferred_chat_id {
                if record.source_chat_id.as_deref() == Some(chat_id) {
                    score = score.saturating_add(4);
                    reasons.push("same-chat provenance".to_string());
                }
            }
            if let Some(last_used_at) = record.last_used_at {
                if now_secs.saturating_sub(last_used_at) <= 30 * 86_400 {
                    score = score.saturating_add(4);
                    reasons.push("recently reused".to_string());
                }
            }
            RuntimeSkillHit {
                record,
                score,
                reasons,
            }
        })
        .collect::<Vec<_>>();
    hits.sort_by(|a, b| {
        b.score
            .cmp(&a.score)
            .then_with(|| b.record.quality_score.cmp(&a.record.quality_score))
            .then_with(|| b.record.updated_at.cmp(&a.record.updated_at))
            .then_with(|| a.record.name.cmp(&b.record.name))
    });
    hits.truncate(limit.min(MAX_RUNTIME_SKILL_HITS));
    hits
}

pub fn upsert_runtime_skill(
    storage: &dyn SkillStorage,
    write: &RuntimeSkillWrite,
) -> crate::error::Result<bool> {
    let mut input = RuntimeSkillUpsertInput {
        name: if write.name.trim().is_empty() {
            runtime_skill_name_for_topic(&write.topic)
        } else {
            write.name.trim().to_string()
        },
        title: if write.title.trim().is_empty() {
            write.topic.trim().replace('_', " ")
        } else {
            write.title.trim().to_string()
        },
        topic: write.topic.trim().to_string(),
        summary: write.summary.trim().to_string(),
        procedure: write.content.trim().to_string(),
        citations: normalize_citations(&write.citations),
        source_chat_id: write
            .source_chat_id
            .as_deref()
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(str::to_string),
        observed_at: write.observed_at,
        updated_at: write.observed_at.max(crate::util::current_unix_secs()),
    };
    if input.summary.is_empty() {
        input.summary = build_runtime_skill_summary(&input.procedure);
    }
    let existing = get_skill_content(storage, &input.name)
        .and_then(|content| parse_runtime_skill_record(&input.name, &content));
    let record = merge_runtime_skill_record(existing.as_ref(), input);
    let rendered = render_runtime_skill_record(&record);
    let changed = get_skill_content(storage, &record.name)
        .map(|existing| existing.trim() != rendered.trim())
        .unwrap_or(true);
    if !changed {
        return Ok(false);
    }
    if rendered.len() > MAX_SKILL_CONTENT_LEN {
        return Err(crate::error::Error::config(
            "runtime_skill",
            format!(
                "content length {} exceeds {}",
                rendered.len(),
                MAX_SKILL_CONTENT_LEN
            ),
        ));
    }
    write_skill(storage, &record.name, &rendered)?;
    Ok(true)
}

fn list_runtime_skill_records(storage: &dyn SkillStorage) -> Vec<RuntimeSkillRecord> {
    let mut out = Vec::new();
    for name in crate::skills::list_skill_names(storage) {
        if !is_runtime_skill_name(&name) {
            continue;
        }
        let Some(content) = get_skill_content(storage, &name) else {
            continue;
        };
        let Some(record) = parse_runtime_skill_record(&name, &content) else {
            continue;
        };
        out.push(record);
    }
    out
}

fn parse_runtime_skill_record(name: &str, content: &str) -> Option<RuntimeSkillRecord> {
    if !content.trim_start().starts_with(RUNTIME_SKILL_MARKER) {
        return None;
    }
    let mut lines = content.lines();
    let marker = lines.next()?;
    if marker.trim() != RUNTIME_SKILL_MARKER {
        return None;
    }
    let title = lines
        .next()
        .map(str::trim)
        .and_then(|line| line.strip_prefix('#'))
        .map(str::trim)
        .filter(|value| !value.is_empty())?
        .to_string();
    let mut meta = std::collections::HashMap::<String, String>::new();
    let mut section_lines = Vec::new();
    let mut seen_meta = false;
    for line in &mut lines {
        let trimmed = line.trim();
        if trimmed.is_empty() {
            if seen_meta {
                break;
            }
            continue;
        }
        if trimmed.starts_with("## ") {
            section_lines.push(line.to_string());
            break;
        }
        let Some((key, value)) = trimmed.split_once(':') else {
            continue;
        };
        seen_meta = true;
        meta.insert(key.trim().to_ascii_lowercase(), value.trim().to_string());
    }
    section_lines.extend(lines.map(str::to_string));
    let sections = collect_runtime_skill_sections(&section_lines.join("\n"));
    let topic = meta.get("topic")?.trim().to_string();
    let summary = sections
        .get("summary")
        .cloned()
        .unwrap_or_else(|| meta.get("summary").cloned().unwrap_or_default())
        .trim()
        .to_string();
    let procedure = sections
        .get("procedure")
        .cloned()
        .unwrap_or_default()
        .trim()
        .to_string();
    if topic.is_empty() || procedure.is_empty() {
        return None;
    }
    let citations = sections
        .get("provenance")
        .map(|value| {
            value
                .lines()
                .filter_map(|line| {
                    let trimmed = line.trim().trim_start_matches("- ").trim();
                    (!trimmed.is_empty()).then(|| trimmed.to_string())
                })
                .take(MAX_RUNTIME_SKILL_CITATIONS)
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();
    let source_chat_id = meta
        .get("source chat")
        .or_else(|| meta.get("source_chat"))
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty());
    let observed_at = meta
        .get("observed at")
        .or_else(|| meta.get("observed_at"))
        .and_then(|value| value.parse::<u64>().ok())
        .unwrap_or(0);
    let updated_at = meta
        .get("updated at")
        .or_else(|| meta.get("updated_at"))
        .and_then(|value| value.parse::<u64>().ok())
        .unwrap_or(observed_at);
    let last_used_at = meta
        .get("last used at")
        .or_else(|| meta.get("last_used_at"))
        .and_then(|value| value.parse::<u64>().ok())
        .filter(|value| *value > 0);
    let use_count = meta
        .get("use count")
        .or_else(|| meta.get("use_count"))
        .and_then(|value| value.parse::<u32>().ok())
        .unwrap_or(0);
    let quality_score = meta
        .get("quality")
        .and_then(|value| value.parse::<u8>().ok())
        .unwrap_or_else(|| {
            compute_runtime_skill_quality(&RuntimeSkillRecord {
                name: name.to_string(),
                title: title.clone(),
                topic: topic.clone(),
                summary: summary.clone(),
                procedure: procedure.clone(),
                citations: citations.clone(),
                source_chat_id: source_chat_id.clone(),
                observed_at,
                updated_at,
                last_used_at,
                use_count,
                quality_score: 0,
            })
        });
    Some(RuntimeSkillRecord {
        name: name.to_string(),
        title,
        topic,
        summary,
        procedure,
        citations,
        source_chat_id,
        observed_at,
        updated_at,
        last_used_at,
        use_count,
        quality_score,
    })
}

fn collect_runtime_skill_sections(content: &str) -> std::collections::HashMap<String, String> {
    let mut sections = std::collections::HashMap::new();
    let mut current_key: Option<String> = None;
    let mut buffer = String::new();
    for line in content.lines() {
        let trimmed = line.trim();
        if let Some(heading) = trimmed.strip_prefix("## ") {
            if let Some(key) = current_key.take() {
                sections.insert(key, buffer.trim().to_string());
                buffer.clear();
            }
            current_key = Some(heading.trim().to_ascii_lowercase());
            continue;
        }
        if !buffer.is_empty() {
            buffer.push('\n');
        }
        buffer.push_str(line);
    }
    if let Some(key) = current_key {
        sections.insert(key, buffer.trim().to_string());
    }
    sections
}

fn merge_runtime_skill_record(
    existing: Option<&RuntimeSkillRecord>,
    input: RuntimeSkillUpsertInput,
) -> RuntimeSkillRecord {
    let mut citations = existing
        .map(|record| record.citations.clone())
        .unwrap_or_default();
    for citation in input.citations {
        if citations.iter().any(|existing| existing == &citation) {
            continue;
        }
        citations.push(citation);
        if citations.len() >= MAX_RUNTIME_SKILL_CITATIONS {
            break;
        }
    }
    let mut record = RuntimeSkillRecord {
        name: input.name,
        title: input.title,
        topic: input.topic,
        summary: input.summary,
        procedure: input.procedure,
        citations,
        source_chat_id: input.source_chat_id,
        observed_at: input.observed_at,
        updated_at: input.updated_at,
        last_used_at: existing.and_then(|record| record.last_used_at),
        use_count: existing.map(|record| record.use_count).unwrap_or(0),
        quality_score: 0,
    };
    if let Some(existing) = existing {
        if record.summary.is_empty() {
            record.summary = existing.summary.clone();
        }
        if record.source_chat_id.is_none() {
            record.source_chat_id = existing.source_chat_id.clone();
        }
        if record.observed_at == 0 {
            record.observed_at = existing.observed_at;
        }
        record.updated_at = record.updated_at.max(existing.updated_at);
        if normalized_runtime_skill_identity(existing) == normalized_runtime_skill_identity(&record)
            && normalize_runtime_skill_text(&existing.procedure)
                == normalize_runtime_skill_text(&record.procedure)
        {
            record.summary = if record.summary.is_empty() {
                existing.summary.clone()
            } else {
                record.summary
            };
        }
    }
    record.quality_score = compute_runtime_skill_quality(&record);
    record
}

fn normalized_runtime_skill_identity(record: &RuntimeSkillRecord) -> String {
    normalize_runtime_skill_text(&format!("{} {}", record.topic, record.title))
}

fn render_runtime_skill_record(record: &RuntimeSkillRecord) -> String {
    let mut out = String::new();
    out.push_str(RUNTIME_SKILL_MARKER);
    out.push('\n');
    out.push_str("# ");
    out.push_str(record.title.trim());
    out.push_str("\n\n");
    out.push_str("Type: procedural_runtime_skill\n");
    out.push_str("Topic: ");
    out.push_str(record.topic.trim());
    out.push('\n');
    if let Some(chat_id) = record.source_chat_id.as_deref() {
        out.push_str("Source chat: ");
        out.push_str(chat_id);
        out.push('\n');
    }
    if record.observed_at > 0 {
        out.push_str("Observed at: ");
        out.push_str(&record.observed_at.to_string());
        out.push('\n');
    }
    if record.updated_at > 0 {
        out.push_str("Updated at: ");
        out.push_str(&record.updated_at.to_string());
        out.push('\n');
    }
    if let Some(last_used_at) = record.last_used_at.filter(|value| *value > 0) {
        out.push_str("Last used at: ");
        out.push_str(&last_used_at.to_string());
        out.push('\n');
    }
    out.push_str("Use count: ");
    out.push_str(&record.use_count.to_string());
    out.push('\n');
    out.push_str("Quality: ");
    out.push_str(&record.quality_score.to_string());
    out.push('\n');
    if runtime_skill_is_stale(record, record.updated_at.max(record.observed_at)) {
        out.push_str("Status: stale\n");
    }
    out.push_str("\n## Summary\n");
    out.push_str(record.summary.trim());
    out.push_str("\n\n## Procedure\n");
    out.push_str(record.procedure.trim());
    if !record.citations.is_empty() {
        out.push_str("\n\n## Provenance\n");
        for citation in &record.citations {
            out.push_str("- ");
            out.push_str(citation.trim());
            out.push('\n');
        }
    }
    out
}

fn compute_runtime_skill_quality(record: &RuntimeSkillRecord) -> u8 {
    let summary_signal = u8::from(!record.summary.trim().is_empty()) * 18;
    let provenance_signal = (record.citations.len().min(4) as u8).saturating_mul(10);
    let reuse_signal = (record.use_count.min(5) as u8).saturating_mul(6);
    let structure_signal = u8::from(record.procedure.lines().count() >= 2) * 14;
    let chat_signal = u8::from(record.source_chat_id.is_some()) * 6;
    20u8.saturating_add(summary_signal)
        .saturating_add(provenance_signal)
        .saturating_add(reuse_signal)
        .saturating_add(structure_signal)
        .saturating_add(chat_signal)
        .min(100)
}

fn score_runtime_skill_record(
    record: RuntimeSkillRecord,
    normalized_query: &str,
    terms: &[String],
    preferred_chat_id: Option<&str>,
    now_secs: u64,
) -> Option<RuntimeSkillHit> {
    let haystack = normalize_runtime_skill_text(&format!(
        "{} {} {}",
        record.topic, record.summary, record.procedure
    ));
    if haystack.is_empty() {
        return None;
    }
    let normalized_title = normalize_runtime_skill_text(&record.title);
    let normalized_topic = normalize_runtime_skill_text(&record.topic);
    let mut score = 0u32;
    let mut reasons = Vec::new();
    for term in terms {
        if normalized_topic.contains(term) {
            score = score.saturating_add(10);
        }
        if normalized_title.contains(term) {
            score = score.saturating_add(8);
        }
        if haystack.contains(term) {
            score = score.saturating_add(4);
        }
    }
    let trigram = trigram_overlap_score(normalized_query, &haystack);
    if trigram > 0 {
        score = score.saturating_add(trigram.min(18));
        reasons.push("semantic overlap".to_string());
    }
    if !normalized_topic.is_empty() && normalized_query.contains(&normalized_topic) {
        score = score.saturating_add(12);
        reasons.push("exact topic overlap".to_string());
    }
    if let Some(chat_id) = preferred_chat_id {
        if record.source_chat_id.as_deref() == Some(chat_id) {
            score = score.saturating_add(6);
            reasons.push("same-chat provenance".to_string());
        }
    }
    if let Some(last_used_at) = record.last_used_at {
        let age = now_secs.saturating_sub(last_used_at);
        if age <= 7 * 86_400 {
            score = score.saturating_add(6);
            reasons.push("recently reused".to_string());
        } else if age <= 30 * 86_400 {
            score = score.saturating_add(3);
        }
    }
    score = score
        .saturating_add(record.use_count.min(6).saturating_mul(2))
        .saturating_add((record.quality_score / 8) as u32);
    if !record.citations.is_empty() {
        score = score.saturating_add(record.citations.len().min(3) as u32 * 2);
        reasons.push(format!("{} provenance refs", record.citations.len()));
    }
    if runtime_skill_is_stale(&record, now_secs) {
        score = score.saturating_sub(8);
        reasons.push("stale".to_string());
    }
    (score > 0).then_some(RuntimeSkillHit {
        record,
        score,
        reasons,
    })
}

fn runtime_skill_is_stale(record: &RuntimeSkillRecord, now_secs: u64) -> bool {
    let freshness_anchor = record
        .last_used_at
        .unwrap_or(record.updated_at.max(record.observed_at));
    now_secs > 0 && now_secs.saturating_sub(freshness_anchor) > RUNTIME_SKILL_STALE_AFTER_SECS
}

fn build_runtime_skill_summary(procedure: &str) -> String {
    procedure
        .lines()
        .find_map(|line| {
            let trimmed = line.trim();
            (!trimmed.is_empty()).then(|| trimmed.to_string())
        })
        .unwrap_or_else(|| truncate_content_to_max(procedure.trim(), 96).to_string())
}

fn normalize_citations(citations: &[String]) -> Vec<String> {
    let mut out = Vec::new();
    for citation in citations {
        let trimmed = citation.trim();
        if trimmed.is_empty() || out.iter().any(|existing| existing == trimmed) {
            continue;
        }
        out.push(trimmed.to_string());
        if out.len() >= MAX_RUNTIME_SKILL_CITATIONS {
            break;
        }
    }
    out
}

fn normalize_runtime_skill_text(input: &str) -> String {
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

fn collect_runtime_skill_terms(normalized_query: &str) -> Vec<String> {
    let mut out = Vec::new();
    for part in normalized_query.split_whitespace() {
        if part.chars().count() >= 2 && !out.iter().any(|existing| existing == part) {
            out.push(part.to_string());
        }
    }
    if out.is_empty() {
        out.push(normalized_query.to_string());
    }
    out
}

fn trigram_overlap_score(left: &str, right: &str) -> u32 {
    let left = skill_trigrams(left);
    let right = skill_trigrams(right);
    if left.is_empty() || right.is_empty() {
        return 0;
    }
    let overlap = left
        .iter()
        .filter(|gram| right.iter().any(|candidate| candidate == *gram))
        .count();
    ((overlap as f32 / left.len().max(right.len()) as f32) * 24.0)
        .round()
        .max(0.0) as u32
}

fn skill_trigrams(value: &str) -> Vec<String> {
    let compact: Vec<char> = value.chars().filter(|ch| !ch.is_whitespace()).collect();
    if compact.len() < 3 {
        return if compact.is_empty() {
            Vec::new()
        } else {
            vec![compact.iter().collect()]
        };
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

fn write_runtime_skill_record(
    storage: &dyn SkillStorage,
    record: &RuntimeSkillRecord,
) -> crate::error::Result<()> {
    write_skill(storage, &record.name, &render_runtime_skill_record(record))
}

fn is_cjk(ch: char) -> bool {
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::error::{Error, Result};
    use std::collections::HashMap;
    use std::sync::Mutex;

    #[derive(Default)]
    struct StubSkillStorage {
        files: Mutex<HashMap<String, Vec<u8>>>,
    }

    impl SkillStorage for StubSkillStorage {
        fn list_names(&self) -> Result<Vec<String>> {
            Ok(self
                .files
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .keys()
                .cloned()
                .collect())
        }

        fn read(&self, name: &str) -> Result<Vec<u8>> {
            self.files
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .get(name)
                .cloned()
                .ok_or_else(|| Error::config("skill", "missing"))
        }

        fn write(&self, name: &str, content: &[u8]) -> Result<()> {
            self.files
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .insert(name.to_string(), content.to_vec());
            Ok(())
        }

        fn remove(&self, name: &str) -> Result<()> {
            self.files
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .remove(name);
            Ok(())
        }
    }

    #[test]
    fn runtime_skill_recall_prefers_exact_topic_and_provenance() {
        let storage = StubSkillStorage::default();
        let changed = upsert_runtime_skill(
            &storage,
            &RuntimeSkillWrite {
                name: String::new(),
                topic: "network_setup".to_string(),
                title: "Network setup".to_string(),
                summary: "Bring up Wi-Fi and verify logs".to_string(),
                content: "- connect wifi\n- check /tmp/log".to_string(),
                citations: vec!["transcript:chat-1#message=2".to_string()],
                source_chat_id: Some("chat-1".to_string()),
                observed_at: 100,
            },
        )
        .unwrap();
        assert!(changed);

        let hits =
            retrieve_runtime_skill_hits(&storage, "继续 network setup", Some("chat-1"), 200, 3);
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].record.topic, "network_setup");
        assert!(hits[0]
            .reasons
            .iter()
            .any(|reason| reason.contains("exact topic")));
    }

    #[test]
    fn build_runtime_skill_block_touches_usage() {
        let storage = StubSkillStorage::default();
        upsert_runtime_skill(
            &storage,
            &RuntimeSkillWrite {
                name: String::new(),
                topic: "archive_debug".to_string(),
                title: "Archive debug".to_string(),
                summary: "Inspect retrieval trace before trusting evidence.".to_string(),
                content: "- run memory_search\n- inspect retrieval_trace".to_string(),
                citations: vec!["turn_log:chat-1#req=req-1".to_string()],
                source_chat_id: Some("chat-1".to_string()),
                observed_at: 100,
            },
        )
        .unwrap();
        let block = build_runtime_skill_recall_block(
            &storage,
            "看看 archive debug",
            Some("chat-1"),
            1000,
            400,
        )
        .unwrap();
        assert!(block.contains("Runtime skills"));
        let record = parse_runtime_skill_record(
            "runtime_skill__archive_debug",
            &get_skill_content(&storage, "runtime_skill__archive_debug").unwrap(),
        )
        .unwrap();
        assert_eq!(record.use_count, 1);
        assert_eq!(record.last_used_at, Some(1000));
    }
}
