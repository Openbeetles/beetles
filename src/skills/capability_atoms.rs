use super::{
    get_skill_content, list_skill_names, runtime::list_runtime_skill_records,
    runtime::runtime_skill_last_transition_at, runtime_skill_name_for_topic, write_skill,
    RuntimeSkillRecord, RuntimeSkillStatus, MAX_SKILL_CONTENT_LEN,
};
use crate::error::{Error, Result};
use crate::platform::SkillStorage;
use crate::util::truncate_content_to_max;
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};

const CAPABILITY_ATOM_MARKER: &str = "<!-- beetle:capability-atom -->";
const CAPABILITY_ATOM_PREFIX: &str = "capability_atom__";
const CAPABILITY_ATOM_EXCHANGE_VERSION: u8 = 1;
const MAX_CAPABILITY_ATOM_SUMMARY_CHARS: usize = 220;
const MAX_CAPABILITY_ATOM_TITLE_CHARS: usize = 96;
const MAX_CAPABILITY_ATOM_MACRO_STEPS: usize = 8;
const MAX_CAPABILITY_ATOM_MACRO_STEP_CHARS: usize = 180;
const MAX_CAPABILITY_ATOM_COMPONENTS: usize = 8;
const MAX_CAPABILITY_ATOM_COMPONENT_ROLE_CHARS: usize = 48;
const MAX_CAPABILITY_ATOM_RECENT_RECORDS: usize = 6;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CapabilityAtomTrustLevel {
    #[default]
    LocalVerified,
    ImportedPendingAdjudication,
    ImportedAdopted,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CapabilityAtomSourceKind {
    #[default]
    RuntimeSkill,
    ImportedAtom,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CapabilityAtomComponentKind {
    #[default]
    CapabilityAtom,
    RuntimeSkill,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct CapabilityAtomComponentRef {
    pub kind: CapabilityAtomComponentKind,
    pub name: String,
    pub role: String,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct CapabilityAtomProvenance {
    pub source_kind: CapabilityAtomSourceKind,
    pub source_name: String,
    pub strategy_digest: String,
    pub lineage_depth: usize,
    pub diff_events: usize,
    pub validated_success_count: u32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source_chat_id: Option<String>,
    pub observed_at: u64,
    pub updated_at: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub imported_at: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub exported_at: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub upstream_trust_hint: Option<CapabilityAtomTrustLevel>,
    pub requires_local_adjudication: bool,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct CapabilityAtomRecord {
    pub name: String,
    pub atom_id: String,
    pub title: String,
    pub topic: String,
    pub summary: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub macro_steps: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub components: Vec<CapabilityAtomComponentRef>,
    pub trust: CapabilityAtomTrustLevel,
    pub provenance: CapabilityAtomProvenance,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct CapabilityAtomExchangeEnvelope {
    pub format_version: u8,
    pub atom: CapabilityAtomRecord,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct CapabilityAtomImportOutcome {
    pub name: String,
    pub changed: bool,
    pub trust: CapabilityAtomTrustLevel,
    pub detail: String,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct CapabilityAtomSyncOutcome {
    pub upserted: usize,
    pub adopted: usize,
    pub removed: usize,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct CapabilityAtomOperatorRecord {
    pub name: String,
    pub topic: String,
    pub title: String,
    pub trust: CapabilityAtomTrustLevel,
    pub component_count: usize,
    pub updated_at: u64,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct CapabilityAtomOperatorSummary {
    pub total: usize,
    pub local_verified: usize,
    pub imported_pending_adjudication: usize,
    pub imported_adopted: usize,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub recent_records: Vec<CapabilityAtomOperatorRecord>,
}

pub fn is_capability_atom_name(name: &str) -> bool {
    name.starts_with(CAPABILITY_ATOM_PREFIX)
}

pub fn build_capability_atom_operator_summary(
    storage: &dyn SkillStorage,
) -> CapabilityAtomOperatorSummary {
    let mut records = list_capability_atom_records(storage);
    let mut summary = CapabilityAtomOperatorSummary {
        total: records.len(),
        ..CapabilityAtomOperatorSummary::default()
    };
    for record in &records {
        match record.trust {
            CapabilityAtomTrustLevel::LocalVerified => {
                summary.local_verified = summary.local_verified.saturating_add(1);
            }
            CapabilityAtomTrustLevel::ImportedPendingAdjudication => {
                summary.imported_pending_adjudication =
                    summary.imported_pending_adjudication.saturating_add(1);
            }
            CapabilityAtomTrustLevel::ImportedAdopted => {
                summary.imported_adopted = summary.imported_adopted.saturating_add(1);
            }
        }
    }
    records.sort_by(|a, b| {
        b.provenance
            .updated_at
            .cmp(&a.provenance.updated_at)
            .then_with(|| a.name.cmp(&b.name))
    });
    summary.recent_records = records
        .into_iter()
        .take(MAX_CAPABILITY_ATOM_RECENT_RECORDS)
        .map(|record| CapabilityAtomOperatorRecord {
            name: record.name,
            topic: record.topic,
            title: record.title,
            trust: record.trust,
            component_count: record.components.len(),
            updated_at: record.provenance.updated_at,
        })
        .collect();
    summary
}

pub fn sync_capability_atoms_from_runtime_skills(
    storage: &dyn SkillStorage,
    _now_secs: u64,
) -> Result<CapabilityAtomSyncOutcome> {
    let runtime_records = list_runtime_skill_records(storage);
    let eligible_records = runtime_records
        .into_iter()
        .filter(capability_atom_runtime_skill_eligible)
        .collect::<Vec<_>>();
    let eligible_names = eligible_records
        .iter()
        .map(|record| capability_atom_name_for_topic(&record.topic))
        .collect::<HashSet<_>>();
    let mut existing_atoms = list_capability_atom_records(storage)
        .into_iter()
        .map(|record| (record.name.clone(), record))
        .collect::<HashMap<_, _>>();
    let mut outcome = CapabilityAtomSyncOutcome::default();

    for record in &eligible_records {
        let atom_name = capability_atom_name_for_topic(&record.topic);
        let existing = existing_atoms.remove(&atom_name);
        let next =
            build_capability_atom_from_runtime_skill(record, existing.as_ref(), &eligible_names)?;
        let rendered = render_capability_atom_record(&next)?;
        let changed = get_skill_content(storage, &atom_name)
            .map(|current| current.trim() != rendered.trim())
            .unwrap_or(true);
        let adopted = existing.as_ref().is_some_and(|current| {
            current.trust == CapabilityAtomTrustLevel::ImportedPendingAdjudication
                && current.provenance.strategy_digest == next.provenance.strategy_digest
                && next.trust == CapabilityAtomTrustLevel::ImportedAdopted
        });
        if changed {
            write_skill(storage, &atom_name, &rendered)?;
            outcome.upserted = outcome.upserted.saturating_add(1);
        }
        if adopted {
            outcome.adopted = outcome.adopted.saturating_add(1);
        }
    }

    for (name, record) in existing_atoms {
        if record.provenance.source_kind != CapabilityAtomSourceKind::RuntimeSkill {
            continue;
        }
        if eligible_names.contains(&name) {
            continue;
        }
        storage.remove(&name)?;
        outcome.removed = outcome.removed.saturating_add(1);
    }

    Ok(outcome)
}

pub fn export_capability_atom_exchange_envelope(
    storage: &dyn SkillStorage,
    atom_name: &str,
    exported_at: u64,
) -> Result<String> {
    let Some(content) = get_skill_content(storage, atom_name) else {
        return Err(Error::config(
            "capability_atom_export",
            format!("missing capability atom: {atom_name}"),
        ));
    };
    let mut record = parse_capability_atom_record(atom_name, &content).ok_or_else(|| {
        Error::config(
            "capability_atom_export",
            format!("invalid capability atom payload: {atom_name}"),
        )
    })?;
    record.provenance.exported_at = Some(exported_at);
    let rendered = render_capability_atom_record(&record)?;
    write_skill(storage, &record.name, &rendered)?;
    serde_json::to_string_pretty(&CapabilityAtomExchangeEnvelope {
        format_version: CAPABILITY_ATOM_EXCHANGE_VERSION,
        atom: record,
    })
    .map_err(|error| Error::config("capability_atom_export", error.to_string()))
}

pub fn import_capability_atom_exchange_envelope(
    storage: &dyn SkillStorage,
    envelope_json: &str,
    imported_at: u64,
) -> Result<CapabilityAtomImportOutcome> {
    let envelope: CapabilityAtomExchangeEnvelope = serde_json::from_str(envelope_json)
        .map_err(|error| Error::config("capability_atom_import", error.to_string()))?;
    if envelope.format_version != CAPABILITY_ATOM_EXCHANGE_VERSION {
        return Err(Error::config(
            "capability_atom_import",
            format!(
                "unsupported capability atom format_version {}",
                envelope.format_version
            ),
        ));
    }
    let claimed = normalize_capability_atom_record(envelope.atom)?;
    let atom_name = capability_atom_name_for_topic(&claimed.topic);
    let claimed_strategy_digest = if claimed.provenance.strategy_digest.trim().is_empty() {
        capability_atom_strategy_digest(&claimed.summary, &claimed.macro_steps)
    } else {
        claimed.provenance.strategy_digest.clone()
    };
    if let Some(existing) = get_capability_atom_record(storage, &atom_name) {
        if matches!(
            existing.trust,
            CapabilityAtomTrustLevel::LocalVerified | CapabilityAtomTrustLevel::ImportedAdopted
        ) {
            let detail = if existing.provenance.strategy_digest == claimed_strategy_digest {
                "trusted local atom already matches imported payload".to_string()
            } else {
                "trusted local atom preserved over imported payload".to_string()
            };
            return Ok(CapabilityAtomImportOutcome {
                name: existing.name,
                changed: false,
                trust: existing.trust,
                detail,
            });
        }
    }

    let imported = CapabilityAtomRecord {
        name: atom_name.clone(),
        atom_id: capability_atom_id_for_topic(&claimed.topic),
        title: claimed.title,
        topic: claimed.topic,
        summary: claimed.summary,
        macro_steps: claimed.macro_steps,
        components: normalize_capability_atom_components(claimed.components, &atom_name),
        trust: CapabilityAtomTrustLevel::ImportedPendingAdjudication,
        provenance: CapabilityAtomProvenance {
            source_kind: CapabilityAtomSourceKind::ImportedAtom,
            source_name: if claimed.atom_id.trim().is_empty() {
                claimed.name
            } else {
                claimed.atom_id
            },
            strategy_digest: claimed_strategy_digest,
            lineage_depth: claimed.provenance.lineage_depth,
            diff_events: claimed.provenance.diff_events,
            validated_success_count: claimed.provenance.validated_success_count,
            source_chat_id: claimed.provenance.source_chat_id,
            observed_at: claimed
                .provenance
                .observed_at
                .max(claimed.provenance.updated_at)
                .max(imported_at),
            updated_at: claimed.provenance.updated_at.max(imported_at),
            imported_at: Some(imported_at),
            exported_at: claimed.provenance.exported_at,
            upstream_trust_hint: Some(claimed.trust),
            requires_local_adjudication: true,
        },
    };
    let rendered = render_capability_atom_record(&imported)?;
    let changed = get_skill_content(storage, &atom_name)
        .map(|current| current.trim() != rendered.trim())
        .unwrap_or(true);
    if changed {
        write_skill(storage, &atom_name, &rendered)?;
    }
    Ok(CapabilityAtomImportOutcome {
        name: atom_name,
        changed,
        trust: imported.trust,
        detail: if changed {
            "imported capability atom now requires local adjudication".to_string()
        } else {
            "imported capability atom already matched local pending record".to_string()
        },
    })
}

fn capability_atom_runtime_skill_eligible(record: &RuntimeSkillRecord) -> bool {
    record.validated_success_count > 0
        && !matches!(record.status, RuntimeSkillStatus::LowValue)
        && !extract_capability_atom_macro_steps(&record.procedure).is_empty()
}

fn build_capability_atom_from_runtime_skill(
    record: &RuntimeSkillRecord,
    existing: Option<&CapabilityAtomRecord>,
    eligible_atom_names: &HashSet<String>,
) -> Result<CapabilityAtomRecord> {
    let name = capability_atom_name_for_topic(&record.topic);
    let macro_steps = extract_capability_atom_macro_steps(&record.procedure);
    if macro_steps.is_empty() {
        return Err(Error::config(
            "capability_atom_sync",
            format!("runtime skill {} lacks reusable macro steps", record.name),
        ));
    }
    let strategy_digest = capability_atom_strategy_digest(&record.summary, &macro_steps);
    let existing_same_digest =
        existing.filter(|current| current.provenance.strategy_digest == strategy_digest);
    let trust = match existing_same_digest.map(|current| current.trust) {
        Some(CapabilityAtomTrustLevel::ImportedPendingAdjudication) => {
            CapabilityAtomTrustLevel::ImportedAdopted
        }
        Some(CapabilityAtomTrustLevel::ImportedAdopted) => {
            CapabilityAtomTrustLevel::ImportedAdopted
        }
        _ => CapabilityAtomTrustLevel::LocalVerified,
    };
    let imported_at = existing_same_digest.and_then(|current| current.provenance.imported_at);
    let exported_at = existing_same_digest.and_then(|current| current.provenance.exported_at);
    let upstream_trust_hint = existing_same_digest.and_then(|current| {
        current
            .provenance
            .upstream_trust_hint
            .or(Some(CapabilityAtomTrustLevel::ImportedPendingAdjudication))
    });
    let lifecycle_event_at = runtime_skill_last_transition_at(record)
        .unwrap_or(record.updated_at.max(record.observed_at))
        .max(imported_at.unwrap_or(0));
    normalize_capability_atom_record(CapabilityAtomRecord {
        name: name.clone(),
        atom_id: capability_atom_id_for_topic(&record.topic),
        title: truncate_content_to_max(record.title.trim(), MAX_CAPABILITY_ATOM_TITLE_CHARS)
            .into_owned(),
        topic: record.topic.clone(),
        summary: truncate_content_to_max(record.summary.trim(), MAX_CAPABILITY_ATOM_SUMMARY_CHARS)
            .into_owned(),
        macro_steps,
        components: collect_capability_atom_components(record, eligible_atom_names, &name),
        trust,
        provenance: CapabilityAtomProvenance {
            source_kind: CapabilityAtomSourceKind::RuntimeSkill,
            source_name: record.name.clone(),
            strategy_digest,
            lineage_depth: record.genome_lineage.len().max(1),
            diff_events: record.strategy_diffs.len(),
            validated_success_count: record.validated_success_count,
            source_chat_id: record.source_chat_id.clone(),
            observed_at: record.observed_at,
            updated_at: lifecycle_event_at,
            imported_at,
            exported_at,
            upstream_trust_hint,
            requires_local_adjudication: false,
        },
    })
}

fn collect_capability_atom_components(
    record: &RuntimeSkillRecord,
    eligible_atom_names: &HashSet<String>,
    atom_name: &str,
) -> Vec<CapabilityAtomComponentRef> {
    let mut refs = Vec::new();
    for topic in &record.component_topics {
        let topic = topic.trim();
        if topic.is_empty() {
            continue;
        }
        let component_atom_name = capability_atom_name_for_topic(topic);
        if component_atom_name == atom_name {
            continue;
        }
        if eligible_atom_names.contains(&component_atom_name) {
            refs.push(CapabilityAtomComponentRef {
                kind: CapabilityAtomComponentKind::CapabilityAtom,
                name: component_atom_name,
                role: "component_atom".to_string(),
            });
        } else {
            refs.push(CapabilityAtomComponentRef {
                kind: CapabilityAtomComponentKind::RuntimeSkill,
                name: runtime_skill_name_for_topic(topic),
                role: "component_runtime_skill".to_string(),
            });
        }
    }
    for superseded in &record.supersedes {
        let superseded = superseded.trim();
        if superseded.is_empty() || superseded == record.name {
            continue;
        }
        refs.push(CapabilityAtomComponentRef {
            kind: CapabilityAtomComponentKind::RuntimeSkill,
            name: superseded.to_string(),
            role: "superseded_runtime_skill".to_string(),
        });
    }
    normalize_capability_atom_components(refs, atom_name)
}

pub(crate) fn list_capability_atom_records(
    storage: &dyn SkillStorage,
) -> Vec<CapabilityAtomRecord> {
    let mut out = Vec::new();
    for name in list_skill_names(storage) {
        if !is_capability_atom_name(&name) {
            continue;
        }
        let Some(content) = get_skill_content(storage, &name) else {
            continue;
        };
        let Some(record) = parse_capability_atom_record(&name, &content) else {
            continue;
        };
        out.push(record);
    }
    out
}

pub(crate) fn capability_atom_lifecycle_event_at(record: &CapabilityAtomRecord) -> Option<u64> {
    let event_at = match record.trust {
        CapabilityAtomTrustLevel::ImportedPendingAdjudication => record
            .provenance
            .imported_at
            .or(Some(record.provenance.observed_at).filter(|value| *value > 0))
            .or(Some(record.provenance.updated_at).filter(|value| *value > 0)),
        CapabilityAtomTrustLevel::ImportedAdopted => Some(
            record
                .provenance
                .updated_at
                .max(record.provenance.imported_at.unwrap_or(0))
                .max(record.provenance.observed_at),
        ),
        CapabilityAtomTrustLevel::LocalVerified => Some(
            record
                .provenance
                .updated_at
                .max(record.provenance.observed_at),
        ),
    };
    event_at.filter(|value| *value > 0)
}

fn get_capability_atom_record(
    storage: &dyn SkillStorage,
    name: &str,
) -> Option<CapabilityAtomRecord> {
    let content = get_skill_content(storage, name)?;
    parse_capability_atom_record(name, &content)
}

fn parse_capability_atom_record(name: &str, content: &str) -> Option<CapabilityAtomRecord> {
    let trimmed = content.trim();
    let payload = trimmed.strip_prefix(CAPABILITY_ATOM_MARKER)?.trim();
    let mut record: CapabilityAtomRecord = serde_json::from_str(payload).ok()?;
    if record.name.trim().is_empty() {
        record.name = name.to_string();
    }
    normalize_capability_atom_record(record).ok()
}

fn render_capability_atom_record(record: &CapabilityAtomRecord) -> Result<String> {
    let payload = serde_json::to_string_pretty(record)
        .map_err(|error| Error::config("capability_atom_render", error.to_string()))?;
    let rendered = format!("{CAPABILITY_ATOM_MARKER}\n{payload}\n");
    if rendered.len() > MAX_SKILL_CONTENT_LEN {
        return Err(Error::config(
            "capability_atom_render",
            format!(
                "capability atom content length {} exceeds {}",
                rendered.len(),
                MAX_SKILL_CONTENT_LEN
            ),
        ));
    }
    Ok(rendered)
}

fn normalize_capability_atom_record(
    mut record: CapabilityAtomRecord,
) -> Result<CapabilityAtomRecord> {
    let topic =
        truncate_content_to_max(record.topic.trim(), MAX_CAPABILITY_ATOM_TITLE_CHARS).into_owned();
    if topic.is_empty() {
        return Err(Error::config(
            "capability_atom_normalize",
            "capability atom requires non-empty topic",
        ));
    }
    let name = capability_atom_name_for_topic(&topic);
    let atom_id = capability_atom_id_for_topic(&topic);
    let title = if record.title.trim().is_empty() {
        topic.replace('_', " ")
    } else {
        truncate_content_to_max(record.title.trim(), MAX_CAPABILITY_ATOM_TITLE_CHARS).into_owned()
    };
    let summary = truncate_content_to_max(
        if record.summary.trim().is_empty() {
            &title
        } else {
            record.summary.trim()
        },
        MAX_CAPABILITY_ATOM_SUMMARY_CHARS,
    )
    .into_owned();
    let macro_steps = normalize_capability_atom_macro_steps(record.macro_steps);
    if macro_steps.is_empty() {
        return Err(Error::config(
            "capability_atom_normalize",
            "capability atom requires reusable macro steps",
        ));
    }
    record.provenance.source_name = truncate_content_to_max(
        record.provenance.source_name.trim(),
        MAX_CAPABILITY_ATOM_TITLE_CHARS,
    )
    .into_owned();
    if record.provenance.strategy_digest.trim().is_empty() {
        record.provenance.strategy_digest = capability_atom_strategy_digest(&summary, &macro_steps);
    }
    if record.provenance.observed_at == 0 {
        record.provenance.observed_at = record.provenance.updated_at;
    }
    if record.provenance.updated_at == 0 {
        record.provenance.updated_at = record.provenance.observed_at;
    }
    record.provenance.updated_at = record
        .provenance
        .updated_at
        .max(record.provenance.observed_at);
    record.components = normalize_capability_atom_components(record.components, &name);
    Ok(CapabilityAtomRecord {
        name,
        atom_id,
        title,
        topic,
        summary,
        macro_steps,
        components: record.components,
        trust: record.trust,
        provenance: record.provenance,
    })
}

fn capability_atom_name_for_topic(topic: &str) -> String {
    runtime_skill_name_for_topic(topic).replacen("runtime_skill__", CAPABILITY_ATOM_PREFIX, 1)
}

fn capability_atom_id_for_topic(topic: &str) -> String {
    format!(
        "capability_atom::{}",
        capability_atom_name_for_topic(topic).trim_start_matches(CAPABILITY_ATOM_PREFIX)
    )
}

fn normalize_capability_atom_macro_steps(steps: Vec<String>) -> Vec<String> {
    let mut out = Vec::new();
    let mut seen = HashSet::<String>::new();
    for step in steps {
        let normalized = normalize_capability_atom_step(step.trim());
        if normalized.is_empty() {
            continue;
        }
        if seen.insert(normalized.clone()) {
            out.push(normalized);
        }
        if out.len() >= MAX_CAPABILITY_ATOM_MACRO_STEPS {
            break;
        }
    }
    out
}

fn extract_capability_atom_macro_steps(procedure: &str) -> Vec<String> {
    let mut steps = procedure
        .lines()
        .map(normalize_capability_atom_step)
        .filter(|step| !step.is_empty())
        .collect::<Vec<_>>();
    if steps.is_empty() {
        let single = normalize_capability_atom_step(procedure.trim());
        if !single.is_empty() {
            steps.push(single);
        }
    }
    steps.truncate(MAX_CAPABILITY_ATOM_MACRO_STEPS);
    steps
}

fn normalize_capability_atom_step(step: &str) -> String {
    let mut trimmed = step.trim();
    if trimmed.is_empty() {
        return String::new();
    }
    if let Some(rest) = trimmed.strip_prefix("- ") {
        trimmed = rest.trim();
    } else if let Some(rest) = trimmed.strip_prefix("* ") {
        trimmed = rest.trim();
    } else if let Some((prefix, rest)) = trimmed.split_once('.') {
        if prefix.chars().all(|ch| ch.is_ascii_digit()) {
            trimmed = rest.trim();
        }
    }
    truncate_content_to_max(trimmed, MAX_CAPABILITY_ATOM_MACRO_STEP_CHARS).into_owned()
}

fn normalize_capability_atom_components(
    components: Vec<CapabilityAtomComponentRef>,
    atom_name: &str,
) -> Vec<CapabilityAtomComponentRef> {
    let mut out = Vec::new();
    let mut seen = HashSet::<(CapabilityAtomComponentKind, String, String)>::new();
    for component in components {
        let name = truncate_content_to_max(component.name.trim(), MAX_CAPABILITY_ATOM_TITLE_CHARS)
            .into_owned();
        if name.is_empty() || name == atom_name {
            continue;
        }
        let role = truncate_content_to_max(
            if component.role.trim().is_empty() {
                "composed_component"
            } else {
                component.role.trim()
            },
            MAX_CAPABILITY_ATOM_COMPONENT_ROLE_CHARS,
        )
        .into_owned();
        let key = (component.kind, name.clone(), role.clone());
        if seen.insert(key) {
            out.push(CapabilityAtomComponentRef {
                kind: component.kind,
                name,
                role,
            });
        }
        if out.len() >= MAX_CAPABILITY_ATOM_COMPONENTS {
            break;
        }
    }
    out
}

fn capability_atom_strategy_digest(summary: &str, macro_steps: &[String]) -> String {
    use std::collections::hash_map::DefaultHasher;
    use std::hash::{Hash, Hasher};

    let mut hasher = DefaultHasher::new();
    summary.trim().to_ascii_lowercase().hash(&mut hasher);
    for step in macro_steps {
        step.trim().to_ascii_lowercase().hash(&mut hasher);
    }
    format!("{:016x}", hasher.finish())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::platform::SkillStorage;
    use crate::skills::{
        build_skill_descriptions_for_system_prompt, get_skill_content,
        record_runtime_skill_outcomes, runtime_skill_name_for_topic, set_skill_enabled,
        write_governed_runtime_skills, RuntimeSkillReuseOutcome, RuntimeSkillWrite,
        RuntimeSkillWriteSource,
    };
    use std::collections::HashMap;
    use std::sync::Mutex;

    #[derive(Default)]
    struct TestSkillStorage {
        files: Mutex<HashMap<String, Vec<u8>>>,
    }

    impl SkillStorage for TestSkillStorage {
        fn list_names(&self) -> Result<Vec<String>> {
            Ok(self.files.lock().unwrap().keys().cloned().collect())
        }

        fn read(&self, name: &str) -> Result<Vec<u8>> {
            self.files
                .lock()
                .unwrap()
                .get(name)
                .cloned()
                .ok_or_else(|| Error::config("test_skill_storage_read", "missing"))
        }

        fn write(&self, name: &str, content: &[u8]) -> Result<()> {
            self.files
                .lock()
                .unwrap()
                .insert(name.to_string(), content.to_vec());
            Ok(())
        }

        fn remove(&self, name: &str) -> Result<()> {
            self.files.lock().unwrap().remove(name);
            Ok(())
        }
    }

    #[derive(Default)]
    struct TestSkillMetaStore {
        order: Mutex<Vec<String>>,
        disabled: Mutex<Vec<String>>,
    }

    impl crate::platform::SkillMetaStore for TestSkillMetaStore {
        fn read_meta(&self) -> Result<(Vec<String>, Vec<String>)> {
            Ok((
                self.order.lock().unwrap().clone(),
                self.disabled.lock().unwrap().clone(),
            ))
        }

        fn write_meta(&self, order: &[String], disabled: &[String]) -> Result<()> {
            *self.order.lock().unwrap() = order.to_vec();
            *self.disabled.lock().unwrap() = disabled.to_vec();
            Ok(())
        }
    }

    fn seed_validated_runtime_skill(storage: &TestSkillStorage) -> String {
        let write = RuntimeSkillWrite {
            name: runtime_skill_name_for_topic("serial framing"),
            topic: "serial framing".to_string(),
            title: "Serial Framing".to_string(),
            summary: "Recover frame boundaries before validating checksums.".to_string(),
            content:
                "1. Scan for sync bytes.\n2. Validate frame length.\n3. Reject corrupted frames."
                    .to_string(),
            citations: vec!["task://run-1".to_string()],
            source_chat_id: Some("chat-1".to_string()),
            observed_at: 1_700_000_000,
        };
        write_governed_runtime_skills(storage, &[write], RuntimeSkillWriteSource::TaskLearning)
            .expect("runtime skill write should succeed");
        let skill_name = runtime_skill_name_for_topic("serial framing");
        record_runtime_skill_outcomes(
            storage,
            std::slice::from_ref(&skill_name),
            RuntimeSkillReuseOutcome::Succeeded,
            1_700_000_100,
            "validated in task execution",
        )
        .expect("runtime skill outcome should record");
        skill_name
    }

    #[test]
    fn sync_promotes_validated_runtime_skill_into_local_verified_atom() {
        let storage = TestSkillStorage::default();
        let skill_name = seed_validated_runtime_skill(&storage);

        let outcome = sync_capability_atoms_from_runtime_skills(&storage, 1_700_000_200)
            .expect("sync should succeed");
        assert_eq!(outcome.upserted, 1);
        assert_eq!(outcome.adopted, 0);

        let atom_name = "capability_atom__serial_framing";
        let rendered = get_skill_content(&storage, atom_name).expect("atom should exist");
        assert!(rendered.contains(CAPABILITY_ATOM_MARKER));
        assert!(rendered.contains("local_verified"));
        assert!(rendered.contains(&skill_name));

        let summary = build_capability_atom_operator_summary(&storage);
        assert_eq!(summary.total, 1);
        assert_eq!(summary.local_verified, 1);
        assert_eq!(summary.imported_pending_adjudication, 0);
    }

    #[test]
    fn import_export_roundtrip_marks_atom_pending_local_adjudication() {
        let storage = TestSkillStorage::default();
        seed_validated_runtime_skill(&storage);
        sync_capability_atoms_from_runtime_skills(&storage, 1_700_000_200)
            .expect("sync should succeed");

        let exported = export_capability_atom_exchange_envelope(
            &storage,
            "capability_atom__serial_framing",
            1_700_000_250,
        )
        .expect("export should succeed");

        let remote = TestSkillStorage::default();
        let import_outcome =
            import_capability_atom_exchange_envelope(&remote, &exported, 1_700_000_300)
                .expect("import should succeed");
        assert!(import_outcome.changed);
        assert_eq!(
            import_outcome.trust,
            CapabilityAtomTrustLevel::ImportedPendingAdjudication
        );

        let summary = build_capability_atom_operator_summary(&remote);
        assert_eq!(summary.total, 1);
        assert_eq!(summary.imported_pending_adjudication, 1);
    }

    #[test]
    fn local_validation_adopts_imported_atom_instead_of_leaving_it_pending() {
        let origin = TestSkillStorage::default();
        seed_validated_runtime_skill(&origin);
        sync_capability_atoms_from_runtime_skills(&origin, 1_700_000_200)
            .expect("sync should succeed");
        let exported = export_capability_atom_exchange_envelope(
            &origin,
            "capability_atom__serial_framing",
            1_700_000_250,
        )
        .expect("export should succeed");

        let receiver = TestSkillStorage::default();
        import_capability_atom_exchange_envelope(&receiver, &exported, 1_700_000_300)
            .expect("import should succeed");
        seed_validated_runtime_skill(&receiver);
        sync_capability_atoms_from_runtime_skills(&receiver, 1_700_000_400)
            .expect("sync should succeed");

        let atom = get_skill_content(&receiver, "capability_atom__serial_framing")
            .expect("atom should exist");
        assert!(atom.contains("imported_adopted"));
        assert!(atom.contains("upstream_trust_hint"));
    }

    #[test]
    fn prompt_assembly_skips_capability_atom_assets() {
        let storage = TestSkillStorage::default();
        storage
            .write("general_skill", b"General skill prompt content.")
            .expect("general skill should write");
        let origin = TestSkillStorage::default();
        seed_validated_runtime_skill(&origin);
        sync_capability_atoms_from_runtime_skills(&origin, 1_700_000_200)
            .expect("sync should succeed");
        let exported = export_capability_atom_exchange_envelope(
            &origin,
            "capability_atom__serial_framing",
            1_700_000_250,
        )
        .expect("export should succeed");
        import_capability_atom_exchange_envelope(&storage, &exported, 1_700_000_300)
            .expect("import should succeed");

        let meta = TestSkillMetaStore::default();
        set_skill_enabled(&meta, "general_skill", true).expect("meta write should succeed");
        let prompt = build_skill_descriptions_for_system_prompt(&meta, &storage, 2_000);
        assert!(prompt.contains("general_skill"));
        assert!(!prompt.contains("capability_atom__serial_framing"));
        assert!(!prompt.contains("Recover frame boundaries"));
    }
}
