use crate::platform::SkillStorage;
use crate::skills::{
    get_skill_content, runtime_skill_name_for_topic, write_skill, RuntimeSkillWrite,
    MAX_SKILL_CONTENT_LEN,
};
use crate::util::{
    collect_retrieval_terms, normalize_retrieval_text, trigram_overlap_score,
    truncate_content_to_max,
};

const RUNTIME_SKILL_MARKER: &str = "<!-- beetle:runtime-skill -->";
const MAX_RUNTIME_SKILL_HITS: usize = 4;
const MAX_RUNTIME_SKILL_CITATIONS: usize = 8;
const MIN_RUNTIME_SKILL_BLOCK_LEN: usize = 180;
const RUNTIME_SKILL_TOUCH_INTERVAL_SECS: u64 = 6 * 60 * 60;
const RUNTIME_SKILL_STALE_AFTER_SECS: u64 = 90 * 86_400;
const RUNTIME_SKILL_DUPLICATE_SIMILARITY: u32 = 16;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RuntimeSkillStatus {
    Active,
    Stale,
    LowValue,
}

impl RuntimeSkillStatus {
    fn label(self) -> &'static str {
        match self {
            Self::Active => "active",
            Self::Stale => "stale",
            Self::LowValue => "low_value",
        }
    }

    fn parse(value: &str) -> Self {
        match value.trim().to_ascii_lowercase().as_str() {
            "stale" => Self::Stale,
            "low_value" | "low-value" | "low value" => Self::LowValue,
            _ => Self::Active,
        }
    }
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct RuntimeSkillGovernanceOutcome {
    pub merged: usize,
    pub pruned: usize,
    pub stale_marked: usize,
    pub low_value_marked: usize,
}

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
    pub status: RuntimeSkillStatus,
    pub supersedes: Vec<String>,
    pub component_topics: Vec<String>,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct RuntimeSkillRecallScoreBreakdown {
    pub lexical_score: u32,
    pub semantic_score: u32,
    pub exact_match_score: u32,
    pub recency_score: u32,
    pub confidence_score: u32,
    pub importance_score: u32,
    pub scope_affinity_score: u32,
    pub governance_score: u32,
    pub source_score: u32,
    pub total_score: u32,
    pub reason_fragments: Vec<String>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RuntimeSkillHit {
    pub record: RuntimeSkillRecord,
    pub score: u32,
    pub reasons: Vec<String>,
    pub score_breakdown: RuntimeSkillRecallScoreBreakdown,
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
    if normalized_query.is_empty() || normalized_query.chars().count() < 2 {
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
            .then_with(|| {
                b.score_breakdown
                    .semantic_score
                    .cmp(&a.score_breakdown.semantic_score)
            })
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
        record.status = RuntimeSkillStatus::Active;
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
    if let Some(composition_line) = build_runtime_skill_composition_line(&hits, query, max_chars) {
        if out
            .len()
            .saturating_add(composition_line.len())
            .saturating_add(1)
            <= max_chars
        {
            out.push_str(&composition_line);
            out.push('\n');
        }
    }
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
                "- [{}] {} (topic: {}; why: {}; quality={}; reused={}; status={})",
                hit.record.title,
                truncate_content_to_max(hit.record.summary.trim(), 120),
                hit.record.topic,
                reasons,
                hit.record.quality_score,
                hit.record.use_count,
                hit.record.status.label(),
            )
        } else {
            format!(
                "- [{}] {} (topic: {}; why: {}; quality={}; reused={}; status={}; provenance={})",
                hit.record.title,
                truncate_content_to_max(hit.record.summary.trim(), 120),
                hit.record.topic,
                reasons,
                hit.record.quality_score,
                hit.record.use_count,
                hit.record.status.label(),
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

pub fn govern_runtime_skills(
    storage: &dyn SkillStorage,
    now_secs: u64,
) -> crate::error::Result<RuntimeSkillGovernanceOutcome> {
    let mut records = list_runtime_skill_records(storage);
    if records.is_empty() {
        return Ok(RuntimeSkillGovernanceOutcome::default());
    }
    let mut outcome = RuntimeSkillGovernanceOutcome::default();
    let mut changed_records = std::collections::HashMap::<String, RuntimeSkillRecord>::new();
    let mut removed_names = Vec::new();

    records.sort_by(|a, b| a.name.cmp(&b.name));
    let mut consumed = vec![false; records.len()];
    for idx in 0..records.len() {
        if consumed[idx] {
            continue;
        }
        let mut group = vec![records[idx].clone()];
        consumed[idx] = true;
        for other_idx in (idx + 1)..records.len() {
            if consumed[other_idx] {
                continue;
            }
            if runtime_skill_similarity(&records[idx], &records[other_idx])
                < RUNTIME_SKILL_DUPLICATE_SIMILARITY
            {
                continue;
            }
            group.push(records[other_idx].clone());
            consumed[other_idx] = true;
        }
        let canonical = select_canonical_runtime_skill_index(&group);
        let merged = merge_runtime_skill_group(group, canonical);
        for superseded in &merged.supersedes {
            if superseded != &merged.name {
                removed_names.push(superseded.clone());
                outcome.merged = outcome.merged.saturating_add(1);
            }
        }
        let governed = apply_runtime_skill_status(merged, now_secs, &mut outcome);
        if should_prune_runtime_skill(&governed, now_secs) {
            removed_names.push(governed.name.clone());
            outcome.pruned = outcome.pruned.saturating_add(1);
            continue;
        }
        changed_records.insert(governed.name.clone(), governed);
    }

    for record in changed_records.values() {
        write_runtime_skill_record(storage, record)?;
    }
    removed_names.sort();
    removed_names.dedup();
    for name in removed_names {
        let _ = storage.remove(&name);
    }
    Ok(outcome)
}

fn fallback_runtime_skill_hits(
    storage: &dyn SkillStorage,
    preferred_chat_id: Option<&str>,
    now_secs: u64,
    limit: usize,
) -> Vec<RuntimeSkillHit> {
    let mut hits = list_runtime_skill_records(storage)
        .into_iter()
        .filter_map(|record| {
            if should_prune_runtime_skill(&record, now_secs) {
                return None;
            }
            let mut reasons = vec!["fallback procedural memory".to_string()];
            let scope_affinity_score = preferred_chat_id
                .filter(|chat_id| record.source_chat_id.as_deref() == Some(*chat_id))
                .map(|_| 4)
                .unwrap_or(0);
            if scope_affinity_score > 0 {
                reasons.push("same-chat provenance".to_string());
            }
            let recency_score = record
                .last_used_at
                .filter(|last_used_at| now_secs.saturating_sub(*last_used_at) <= 30 * 86_400)
                .map(|_| 4)
                .unwrap_or(0);
            if recency_score > 0 {
                reasons.push("recently reused".to_string());
            }
            if runtime_skill_is_stale(&record, now_secs) {
                reasons.push("stale".to_string());
            }
            if matches!(record.status, RuntimeSkillStatus::LowValue) {
                reasons.push("low-value".to_string());
            }
            let confidence_score = (record.quality_score / 8) as u32;
            let importance_score = record.use_count.min(6).saturating_mul(2);
            let governance_score = match record.status {
                RuntimeSkillStatus::Active => 4,
                RuntimeSkillStatus::Stale => 1,
                RuntimeSkillStatus::LowValue => 0,
            };
            let source_score = record.citations.len().min(3) as u32 * 2;
            let breakdown = RuntimeSkillRecallScoreBreakdown {
                lexical_score: 0,
                semantic_score: 0,
                exact_match_score: 0,
                recency_score,
                confidence_score,
                importance_score,
                scope_affinity_score,
                governance_score,
                source_score,
                total_score: recency_score
                    .saturating_add(confidence_score)
                    .saturating_add(importance_score)
                    .saturating_add(scope_affinity_score)
                    .saturating_add(governance_score)
                    .saturating_add(source_score),
                reason_fragments: reasons.clone(),
            };
            Some(RuntimeSkillHit {
                record,
                score: breakdown.total_score,
                reasons,
                score_breakdown: breakdown,
            })
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
    let all_records = list_runtime_skill_records(storage);
    let canonical_name = find_canonical_runtime_skill_name(&all_records, &input);
    let existing = canonical_name
        .as_deref()
        .and_then(|name| {
            get_skill_content(storage, name)
                .and_then(|content| parse_runtime_skill_record(name, &content))
        })
        .or(existing);
    if let Some(canonical_name) = canonical_name {
        input.name = canonical_name;
    }
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
                status: RuntimeSkillStatus::Active,
                supersedes: Vec::new(),
                component_topics: vec![topic.clone()],
            })
        });
    let mut component_topics = meta
        .get("components")
        .or_else(|| meta.get("component_topics"))
        .map(|value| parse_list_field(value))
        .unwrap_or_default();
    if !component_topics.iter().any(|candidate| candidate == &topic) {
        component_topics.push(topic.clone());
    }
    component_topics.sort();
    component_topics.dedup();
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
        status: meta
            .get("status")
            .map(|value| RuntimeSkillStatus::parse(value))
            .unwrap_or(RuntimeSkillStatus::Active),
        supersedes: meta
            .get("supersedes")
            .map(|value| parse_list_field(value))
            .unwrap_or_default(),
        component_topics,
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
    let RuntimeSkillUpsertInput {
        name,
        title,
        topic,
        summary,
        procedure,
        citations: input_citations,
        source_chat_id,
        observed_at,
        updated_at,
    } = input;
    let mut citations = existing
        .map(|record| record.citations.clone())
        .unwrap_or_default();
    for citation in input_citations {
        if citations.iter().any(|existing| existing == &citation) {
            continue;
        }
        citations.push(citation);
        if citations.len() >= MAX_RUNTIME_SKILL_CITATIONS {
            break;
        }
    }
    let mut record = RuntimeSkillRecord {
        name,
        title,
        topic: topic.clone(),
        summary,
        procedure,
        citations,
        source_chat_id,
        observed_at,
        updated_at,
        last_used_at: existing.and_then(|record| record.last_used_at),
        use_count: existing.map(|record| record.use_count).unwrap_or(0),
        quality_score: 0,
        status: existing
            .map(|record| record.status)
            .unwrap_or(RuntimeSkillStatus::Active),
        supersedes: existing
            .map(|record| record.supersedes.clone())
            .unwrap_or_default(),
        component_topics: existing
            .map(|record| record.component_topics.clone())
            .unwrap_or_else(|| vec![topic]),
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
        for topic in &existing.component_topics {
            if !record
                .component_topics
                .iter()
                .any(|candidate| candidate == topic)
            {
                record.component_topics.push(topic.clone());
            }
        }
        if !record
            .component_topics
            .iter()
            .any(|candidate| candidate == &record.topic)
        {
            record.component_topics.push(record.topic.clone());
        }
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
    if !record
        .component_topics
        .iter()
        .any(|candidate| candidate == &record.topic)
    {
        record.component_topics.push(record.topic.clone());
    }
    record.component_topics.sort();
    record.component_topics.dedup();
    record.supersedes.sort();
    record.supersedes.dedup();
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
    out.push_str("Status: ");
    out.push_str(record.status.label());
    out.push('\n');
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
    if !record.supersedes.is_empty() {
        out.push_str("Supersedes: ");
        out.push_str(&record.supersedes.join(", "));
        out.push('\n');
    }
    if !record.component_topics.is_empty() {
        out.push_str("Components: ");
        out.push_str(&record.component_topics.join(", "));
        out.push('\n');
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
    let breakdown = score_runtime_skill_record_breakdown(
        &record,
        normalized_query,
        terms,
        preferred_chat_id,
        now_secs,
    )?;
    Some(RuntimeSkillHit {
        record,
        score: breakdown.total_score,
        reasons: breakdown.reason_fragments.clone(),
        score_breakdown: breakdown,
    })
}

pub(crate) fn score_runtime_skill_record_breakdown(
    record: &RuntimeSkillRecord,
    normalized_query: &str,
    terms: &[String],
    preferred_chat_id: Option<&str>,
    now_secs: u64,
) -> Option<RuntimeSkillRecallScoreBreakdown> {
    if matches!(record.status, RuntimeSkillStatus::LowValue)
        && runtime_skill_is_stale(record, now_secs)
        && record.use_count == 0
    {
        return None;
    }
    let haystack = normalize_runtime_skill_text(&format!(
        "{} {} {}",
        record.topic, record.summary, record.procedure
    ));
    if haystack.is_empty() {
        return None;
    }
    let normalized_title = normalize_runtime_skill_text(&record.title);
    let normalized_topic = normalize_runtime_skill_text(&record.topic);
    let normalized_summary = normalize_runtime_skill_text(&record.summary);
    let mut lexical_score = 0u32;
    let mut exact_match_score = 0u32;
    let mut reasons = Vec::new();
    if normalized_query == normalized_topic {
        exact_match_score = exact_match_score.saturating_add(14);
        reasons.push("exact topic overlap".to_string());
    }
    if normalized_query == normalized_title {
        exact_match_score = exact_match_score.saturating_add(10);
        reasons.push("exact title overlap".to_string());
    }
    for term in terms {
        if normalized_topic.contains(term) {
            lexical_score = lexical_score.saturating_add(10);
        }
        if normalized_title.contains(term) {
            lexical_score = lexical_score.saturating_add(8);
        }
        if normalized_summary.contains(term) {
            lexical_score = lexical_score.saturating_add(5);
        }
        if haystack.contains(term) {
            lexical_score = lexical_score.saturating_add(3);
        }
    }
    if lexical_score > 0 {
        reasons.push("term overlap".to_string());
    }
    let semantic_score = trigram_overlap_score(normalized_query, &haystack, 18);
    if semantic_score > 0 {
        reasons.push("semantic overlap".to_string());
    }
    let scope_affinity_score = preferred_chat_id
        .filter(|chat_id| record.source_chat_id.as_deref() == Some(*chat_id))
        .map(|_| 6)
        .unwrap_or(0);
    if scope_affinity_score > 0 {
        reasons.push("same-chat provenance".to_string());
    }
    let recency_score = record
        .last_used_at
        .map(|last_used_at| {
            let age = now_secs.saturating_sub(last_used_at);
            if age <= 7 * 86_400 {
                6
            } else if age <= 30 * 86_400 {
                3
            } else {
                0
            }
        })
        .unwrap_or(0);
    if recency_score > 0 {
        reasons.push("recently reused".to_string());
    }
    let confidence_score = (record.quality_score / 8) as u32;
    let importance_score = record.use_count.min(6).saturating_mul(2);
    let source_score = if record.citations.is_empty() {
        0
    } else {
        record.citations.len().min(3) as u32 * 2
    };
    if source_score > 0 {
        reasons.push(format!("{} provenance refs", record.citations.len()));
    }
    let governance_score = match record.status {
        RuntimeSkillStatus::Active => 6,
        RuntimeSkillStatus::Stale => 1,
        RuntimeSkillStatus::LowValue => 0,
    };
    if runtime_skill_is_stale(record, now_secs) {
        reasons.push("stale".to_string());
    }
    if matches!(record.status, RuntimeSkillStatus::LowValue) {
        reasons.push("low-value".to_string());
    }
    let total_score = lexical_score
        .saturating_add(semantic_score)
        .saturating_add(exact_match_score)
        .saturating_add(recency_score)
        .saturating_add(confidence_score)
        .saturating_add(importance_score)
        .saturating_add(scope_affinity_score)
        .saturating_add(governance_score)
        .saturating_add(source_score);
    (total_score > 0).then_some(RuntimeSkillRecallScoreBreakdown {
        lexical_score,
        semantic_score,
        exact_match_score,
        recency_score,
        confidence_score,
        importance_score,
        scope_affinity_score,
        governance_score,
        source_score,
        total_score,
        reason_fragments: reasons,
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

fn parse_list_field(value: &str) -> Vec<String> {
    value
        .split(',')
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_string)
        .collect()
}

fn find_canonical_runtime_skill_name(
    records: &[RuntimeSkillRecord],
    input: &RuntimeSkillUpsertInput,
) -> Option<String> {
    let probe = RuntimeSkillRecord {
        name: input.name.clone(),
        title: input.title.clone(),
        topic: input.topic.clone(),
        summary: input.summary.clone(),
        procedure: input.procedure.clone(),
        citations: input.citations.clone(),
        source_chat_id: input.source_chat_id.clone(),
        observed_at: input.observed_at,
        updated_at: input.updated_at,
        last_used_at: None,
        use_count: 0,
        quality_score: 0,
        status: RuntimeSkillStatus::Active,
        supersedes: Vec::new(),
        component_topics: vec![input.topic.clone()],
    };
    records
        .iter()
        .filter(|record| {
            runtime_skill_similarity(record, &probe) >= RUNTIME_SKILL_DUPLICATE_SIMILARITY
        })
        .max_by_key(|record| runtime_skill_rank(record))
        .map(|record| record.name.clone())
}

fn runtime_skill_similarity(left: &RuntimeSkillRecord, right: &RuntimeSkillRecord) -> u32 {
    let left_id = normalize_runtime_skill_text(&format!("{} {}", left.topic, left.title));
    let right_id = normalize_runtime_skill_text(&format!("{} {}", right.topic, right.title));
    if left_id == right_id {
        return 32;
    }
    trigram_overlap_score(&left_id, &right_id, 24)
        .saturating_add(u32::from(left_id.contains(&right_id) || right_id.contains(&left_id)) * 12)
}

fn runtime_skill_rank(record: &RuntimeSkillRecord) -> u32 {
    (record.quality_score as u32)
        .saturating_add(record.use_count.saturating_mul(3))
        .saturating_add(record.citations.len() as u32 * 2)
        .saturating_add(u32::from(record.status == RuntimeSkillStatus::Active) * 6)
        .saturating_add((record.updated_at / 86_400) as u32)
}

fn select_canonical_runtime_skill_index(group: &[RuntimeSkillRecord]) -> usize {
    let mut best_idx = 0usize;
    let mut best_rank = 0u32;
    for (idx, record) in group.iter().enumerate() {
        let rank = runtime_skill_rank(record);
        if idx == 0 || rank > best_rank {
            best_idx = idx;
            best_rank = rank;
        }
    }
    best_idx
}

fn merge_runtime_skill_group(
    mut group: Vec<RuntimeSkillRecord>,
    canonical_idx: usize,
) -> RuntimeSkillRecord {
    let mut canonical = group.swap_remove(canonical_idx);
    for duplicate in group {
        if duplicate.name != canonical.name
            && !canonical
                .supersedes
                .iter()
                .any(|existing| existing == &duplicate.name)
        {
            canonical.supersedes.push(duplicate.name.clone());
        }
        for topic in duplicate
            .component_topics
            .iter()
            .chain(std::iter::once(&duplicate.topic))
        {
            if !canonical
                .component_topics
                .iter()
                .any(|existing| existing == topic)
            {
                canonical.component_topics.push(topic.clone());
            }
        }
        for citation in duplicate.citations {
            if !canonical
                .citations
                .iter()
                .any(|existing| existing == &citation)
            {
                canonical.citations.push(citation);
                canonical.citations.truncate(MAX_RUNTIME_SKILL_CITATIONS);
            }
        }
        canonical.use_count = canonical.use_count.saturating_add(duplicate.use_count);
        canonical.last_used_at = canonical.last_used_at.max(duplicate.last_used_at);
        canonical.observed_at = canonical.observed_at.max(duplicate.observed_at);
        canonical.updated_at = canonical.updated_at.max(duplicate.updated_at);
        if duplicate.quality_score > canonical.quality_score
            && duplicate.summary.len() > canonical.summary.len()
        {
            canonical.summary = duplicate.summary;
        }
        if duplicate.procedure.lines().count() > canonical.procedure.lines().count() {
            canonical.procedure = duplicate.procedure;
        }
        if canonical.source_chat_id.is_none() {
            canonical.source_chat_id = duplicate.source_chat_id;
        }
    }
    canonical.component_topics.sort();
    canonical.component_topics.dedup();
    canonical.supersedes.sort();
    canonical.supersedes.dedup();
    canonical.quality_score = compute_runtime_skill_quality(&canonical);
    canonical
}

fn apply_runtime_skill_status(
    mut record: RuntimeSkillRecord,
    now_secs: u64,
    outcome: &mut RuntimeSkillGovernanceOutcome,
) -> RuntimeSkillRecord {
    let stale = runtime_skill_is_stale(&record, now_secs);
    let next_status = if stale && record.quality_score < 45 && record.use_count == 0 {
        RuntimeSkillStatus::LowValue
    } else if stale {
        RuntimeSkillStatus::Stale
    } else if record.quality_score < 32 && record.use_count == 0 {
        RuntimeSkillStatus::LowValue
    } else {
        RuntimeSkillStatus::Active
    };
    if next_status != record.status {
        match next_status {
            RuntimeSkillStatus::Stale => {
                outcome.stale_marked = outcome.stale_marked.saturating_add(1)
            }
            RuntimeSkillStatus::LowValue => {
                outcome.low_value_marked = outcome.low_value_marked.saturating_add(1)
            }
            RuntimeSkillStatus::Active => {}
        }
    }
    record.status = next_status;
    record
}

fn should_prune_runtime_skill(record: &RuntimeSkillRecord, now_secs: u64) -> bool {
    matches!(record.status, RuntimeSkillStatus::LowValue)
        && runtime_skill_is_stale(record, now_secs)
        && record.use_count == 0
        && record.citations.len() <= 1
}

fn build_runtime_skill_composition_line(
    hits: &[RuntimeSkillHit],
    query: &str,
    max_chars: usize,
) -> Option<String> {
    if hits.len() < 2 {
        return None;
    }
    let normalized_query = normalize_runtime_skill_text(query);
    let first = &hits[0];
    let second = hits
        .iter()
        .skip(1)
        .find(|candidate| candidate.record.topic != first.record.topic)?;
    let query_terms = collect_runtime_skill_terms(&normalized_query);
    let first_overlap = query_terms
        .iter()
        .filter(|term| normalize_runtime_skill_text(&first.record.summary).contains(term.as_str()))
        .count();
    let second_overlap = query_terms
        .iter()
        .filter(|term| normalize_runtime_skill_text(&second.record.summary).contains(term.as_str()))
        .count();
    if first_overlap == 0 || second_overlap == 0 {
        return None;
    }
    let line = format!(
        "- [Composition] Combine {} then {} for this turn when both setup and verification are needed.",
        first.record.title, second.record.title
    );
    (line.len() <= max_chars / 2).then_some(line)
}

fn normalize_runtime_skill_text(input: &str) -> String {
    normalize_retrieval_text(input)
}

fn collect_runtime_skill_terms(normalized_query: &str) -> Vec<String> {
    collect_retrieval_terms(normalized_query, 2, 24, &[2, 3])
}

fn write_runtime_skill_record(
    storage: &dyn SkillStorage,
    record: &RuntimeSkillRecord,
) -> crate::error::Result<()> {
    write_skill(storage, &record.name, &render_runtime_skill_record(record))
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

    fn runtime_skill_record(
        name: &str,
        topic: &str,
        title: &str,
        summary: &str,
        procedure: &str,
        observed_at: u64,
    ) -> RuntimeSkillRecord {
        RuntimeSkillRecord {
            name: name.to_string(),
            title: title.to_string(),
            topic: topic.to_string(),
            summary: summary.to_string(),
            procedure: procedure.to_string(),
            citations: Vec::new(),
            source_chat_id: Some("chat-1".to_string()),
            observed_at,
            updated_at: observed_at,
            last_used_at: None,
            use_count: 0,
            quality_score: 0,
            status: RuntimeSkillStatus::Active,
            supersedes: Vec::new(),
            component_topics: vec![topic.to_string()],
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

    #[test]
    fn governance_merges_duplicate_runtime_skills_into_canonical_record() {
        let storage = StubSkillStorage::default();
        let mut canonical = runtime_skill_record(
            "runtime_skill__wifi_setup",
            "wifi_setup",
            "Wi-Fi setup",
            "Bring Wi-Fi up before verification.",
            "- connect wifi\n- verify connectivity",
            100,
        );
        canonical.citations = vec!["transcript:chat-1#message=1".to_string()];
        canonical.quality_score = compute_runtime_skill_quality(&canonical);
        write_runtime_skill_record(&storage, &canonical).unwrap();

        let mut duplicate = runtime_skill_record(
            "runtime_skill__wifi_verification",
            "wifi setup",
            "Wi-Fi setup flow",
            "Bring Wi-Fi up before verification.",
            "- connect wifi\n- verify connectivity",
            120,
        );
        duplicate.citations = vec!["turn_log:chat-1#req=req-1".to_string()];
        duplicate.quality_score = compute_runtime_skill_quality(&duplicate);
        write_runtime_skill_record(&storage, &duplicate).unwrap();

        let outcome = govern_runtime_skills(&storage, 200).unwrap();
        assert_eq!(outcome.merged, 1);
        assert!(get_skill_content(&storage, "runtime_skill__wifi_verification").is_none());
        let merged = parse_runtime_skill_record(
            "runtime_skill__wifi_setup",
            &get_skill_content(&storage, "runtime_skill__wifi_setup").unwrap(),
        )
        .unwrap();
        assert!(merged
            .supersedes
            .iter()
            .any(|name| name == "runtime_skill__wifi_verification"));
        assert!(merged
            .component_topics
            .iter()
            .any(|topic| topic == "wifi_setup"));
        assert!(merged
            .component_topics
            .iter()
            .any(|topic| topic == "wifi setup"));
        assert_eq!(merged.citations.len(), 2);
    }

    #[test]
    fn governance_prunes_stale_low_value_runtime_skills() {
        let storage = StubSkillStorage::default();
        let mut low_value = runtime_skill_record(
            "runtime_skill__temp_probe",
            "temp_probe",
            "Temp probe",
            "",
            "probe",
            1,
        );
        low_value.quality_score = compute_runtime_skill_quality(&low_value);
        write_runtime_skill_record(&storage, &low_value).unwrap();

        let outcome =
            govern_runtime_skills(&storage, RUNTIME_SKILL_STALE_AFTER_SECS.saturating_add(10))
                .unwrap();
        assert_eq!(outcome.pruned, 1);
        assert!(get_skill_content(&storage, "runtime_skill__temp_probe").is_none());
    }

    #[test]
    fn runtime_skill_recall_can_suggest_composition() {
        let storage = StubSkillStorage::default();
        upsert_runtime_skill(
            &storage,
            &RuntimeSkillWrite {
                name: String::new(),
                topic: "network_setup".to_string(),
                title: "Network setup".to_string(),
                summary: "Network setup checklist for bring-up.".to_string(),
                content: "- connect wifi\n- collect link status".to_string(),
                citations: vec!["transcript:chat-1#message=1".to_string()],
                source_chat_id: Some("chat-1".to_string()),
                observed_at: 100,
            },
        )
        .unwrap();
        upsert_runtime_skill(
            &storage,
            &RuntimeSkillWrite {
                name: String::new(),
                topic: "network_verification".to_string(),
                title: "Network verification".to_string(),
                summary: "Verification pass for setup and connectivity.".to_string(),
                content: "- inspect retrieval trace\n- verify connectivity".to_string(),
                citations: vec!["turn_log:chat-1#req=req-1".to_string()],
                source_chat_id: Some("chat-1".to_string()),
                observed_at: 120,
            },
        )
        .unwrap();

        let block = build_runtime_skill_recall_block(
            &storage,
            "network setup verification",
            Some("chat-1"),
            200,
            480,
        )
        .unwrap();
        assert!(block.contains("[Composition]"));
    }
}
