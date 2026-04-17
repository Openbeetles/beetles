//! Linux-only capability atom inspection and exchange tools.

use crate::error::{Error, Result};
use crate::skills::{
    build_capability_atom_operator_summary, export_capability_atom_exchange_envelope,
    import_capability_atom_exchange_envelope, list_capability_atom_records,
    CapabilityAtomImportOutcome, CapabilityAtomOperatorSummary, CapabilityAtomRecord,
    CapabilityAtomSourceKind, CapabilityAtomTrustLevel,
};
use crate::tools::{
    parse_tool_args, serialize_tool_output, Tool, ToolApprovalMode, ToolContext, ToolEffectClass,
    ToolMetadata, ToolRiskLevel, ToolRollbackKind, TOOL_CAPABILITY_ATOMS_EXCHANGE,
    TOOL_CAPABILITY_ATOMS_INSPECT,
};
use crate::util::current_unix_secs;
use crate::SkillStorage;
use serde::Serialize;
use serde_json::Value;
use std::sync::Arc;

const EXCHANGE_PLANE: &str = TOOL_CAPABILITY_ATOMS_EXCHANGE;
const INSPECT_PLANE: &str = TOOL_CAPABILITY_ATOMS_INSPECT;
const DEFAULT_INSPECT_LIMIT: usize = 12;
const MAX_INSPECT_LIMIT: usize = 24;

pub struct CapabilityAtomsExchangeTool {
    skill_storage: Arc<dyn SkillStorage + Send + Sync>,
}

pub struct CapabilityAtomsInspectTool {
    skill_storage: Arc<dyn SkillStorage + Send + Sync>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
struct CapabilityAtomExchangeRecordView {
    name: String,
    topic: String,
    title: String,
    trust: String,
    source_kind: String,
    requires_local_adjudication: bool,
    component_count: usize,
    updated_at: u64,
    #[serde(skip_serializing_if = "Option::is_none")]
    imported_at: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    exported_at: Option<u64>,
    exchange_ready: bool,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
struct CapabilityAtomsExchangeResponse {
    ok: bool,
    plane: &'static str,
    op: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    summary: Option<CapabilityAtomOperatorSummary>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    records: Vec<CapabilityAtomExchangeRecordView>,
    #[serde(skip_serializing_if = "Option::is_none")]
    atom: Option<CapabilityAtomExchangeRecordView>,
    #[serde(skip_serializing_if = "Option::is_none")]
    envelope: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    import_outcome: Option<CapabilityAtomImportOutcome>,
}

impl CapabilityAtomsExchangeTool {
    pub fn new(skill_storage: Arc<dyn SkillStorage + Send + Sync>) -> Self {
        Self { skill_storage }
    }
}

impl CapabilityAtomsInspectTool {
    pub fn new(skill_storage: Arc<dyn SkillStorage + Send + Sync>) -> Self {
        Self { skill_storage }
    }
}

impl Tool for CapabilityAtomsExchangeTool {
    fn name(&self) -> &'static str {
        TOOL_CAPABILITY_ATOMS_EXCHANGE
    }

    fn description(&self) -> &str {
        "Export and import Linux-only capability atom exchange envelopes. Imports land as pending local adjudication until the runtime skill chain re-validates them."
    }

    fn schema(&self) -> &str {
        r#"{"type":"object","properties":{"op":{"type":"string","enum":["export","import"],"description":"Export one atom as an exchange envelope, or import an exchange envelope into the local pending-adjudication set."},"atom_name":{"type":"string","description":"Capability atom name for export."},"topic":{"type":"string","description":"Capability atom topic for export. Used when atom_name is omitted."},"confirm":{"type":"boolean","description":"Must be true to approve the governed capability atom state change."},"envelope":{"description":"Capability atom exchange envelope as a JSON string or embedded JSON object when op=import."}},"required":["op","confirm"],"allOf":[{"if":{"properties":{"op":{"const":"export"}}},"then":{"anyOf":[{"required":["atom_name"]},{"required":["topic"]}]}},{"if":{"properties":{"op":{"const":"import"}}},"then":{"required":["envelope"]}}]}"#
    }

    fn execute(&self, args: &str, _ctx: &mut dyn ToolContext) -> Result<String> {
        let obj = parse_tool_args(args, "tool_capability_atoms_exchange")?;
        let op = obj
            .get("op")
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .ok_or_else(|| Error::config("tool_capability_atoms_exchange", "missing op"))?;

        let response = match op {
            "export" => {
                require_confirm(&obj, "tool_capability_atoms_exchange", "export")?;
                self.export(&obj)?
            }
            "import" => {
                require_confirm(&obj, "tool_capability_atoms_exchange", "import")?;
                self.import(&obj)?
            }
            _ => {
                return Err(Error::config(
                    "tool_capability_atoms_exchange",
                    format!("unknown op '{op}'"),
                ));
            }
        };
        serialize_tool_output("tool_capability_atoms_exchange", &response)
    }

    fn metadata(&self) -> ToolMetadata {
        ToolMetadata::stateful().with_approval_mode(ToolApprovalMode::ExplicitIntent)
    }

    fn execution_shape(&self, args: &str) -> Result<crate::tools::ToolExecutionShape> {
        let obj = parse_tool_args(args, "tool_capability_atoms_exchange_governance")?;
        let op = obj.get("op").and_then(Value::as_str).unwrap_or("export");
        let confirm = obj.get("confirm").and_then(Value::as_bool).unwrap_or(false);
        Ok(match op {
            "import" => self
                .metadata()
                .default_execution_shape(op)
                .with_effect_class(ToolEffectClass::PersistentStateWrite)
                .with_risk_level(ToolRiskLevel::Medium)
                .with_approval_mode(ToolApprovalMode::ExplicitIntent)
                .with_approval_granted(confirm)
                .with_rollback_kind(ToolRollbackKind::CompensatingWrite),
            "export" => self
                .metadata()
                .default_execution_shape(op)
                .with_effect_class(ToolEffectClass::PersistentStateWrite)
                .with_risk_level(ToolRiskLevel::Low)
                .with_approval_mode(ToolApprovalMode::ExplicitIntent)
                .with_approval_granted(confirm)
                .with_rollback_kind(ToolRollbackKind::CompensatingWrite),
            _ => self.metadata().default_execution_shape(op),
        })
    }

    fn governance_examples(&self) -> &'static [&'static str] {
        &[
            r#"{"op":"export","atom_name":"demo"}"#,
            r#"{"op":"import","envelope":{"version":1}}"#,
        ]
    }
}

impl Tool for CapabilityAtomsInspectTool {
    fn name(&self) -> &'static str {
        TOOL_CAPABILITY_ATOMS_INSPECT
    }

    fn description(&self) -> &str {
        "Inspect Linux-only capability atoms and exchange readiness without mutating local state."
    }

    fn schema(&self) -> &str {
        r#"{"type":"object","properties":{"atom_name":{"type":"string","description":"Capability atom name for focused inspection."},"topic":{"type":"string","description":"Capability atom topic for focused inspection. Used when atom_name is omitted."},"limit":{"type":"integer","description":"Optional inspect limit. Defaults to 12 and caps at 24."}}}"#
    }

    fn execute(&self, args: &str, _ctx: &mut dyn ToolContext) -> Result<String> {
        let obj = parse_tool_args(args, "tool_capability_atoms_inspect")?;
        let response = inspect_capability_atoms(self.skill_storage.as_ref(), &obj)?;
        serialize_tool_output("tool_capability_atoms_inspect", &response)
    }

    fn metadata(&self) -> ToolMetadata {
        ToolMetadata::task()
    }
}

fn inspect_capability_atoms(
    storage: &dyn SkillStorage,
    obj: &serde_json::Map<String, Value>,
) -> Result<CapabilityAtomsExchangeResponse> {
    let limit = obj
        .get("limit")
        .and_then(Value::as_u64)
        .map(|value| value as usize)
        .map(|value| value.clamp(1, MAX_INSPECT_LIMIT))
        .unwrap_or(DEFAULT_INSPECT_LIMIT);
    let summary = build_capability_atom_operator_summary(storage);
    let mut records = sorted_capability_atom_records(storage);
    if let Some(filter) = atom_filter(obj) {
        records.retain(|record| filter.matches(record));
    }
    let views = records
        .into_iter()
        .take(limit)
        .map(capability_atom_exchange_view)
        .collect::<Vec<_>>();
    Ok(CapabilityAtomsExchangeResponse {
        ok: true,
        plane: INSPECT_PLANE,
        op: "inspect".to_string(),
        summary: Some(summary),
        records: views,
        atom: None,
        envelope: None,
        import_outcome: None,
    })
}

impl CapabilityAtomsExchangeTool {
    fn export(
        &self,
        obj: &serde_json::Map<String, Value>,
    ) -> Result<CapabilityAtomsExchangeResponse> {
        let atom = select_capability_atom(self.skill_storage.as_ref(), obj)?;
        let envelope = export_capability_atom_exchange_envelope(
            self.skill_storage.as_ref(),
            &atom.name,
            current_unix_secs(),
        )?;
        let exported =
            find_capability_atom(self.skill_storage.as_ref(), &atom.name).unwrap_or(atom);
        Ok(CapabilityAtomsExchangeResponse {
            ok: true,
            plane: EXCHANGE_PLANE,
            op: "export".to_string(),
            summary: None,
            records: Vec::new(),
            atom: Some(capability_atom_exchange_view(exported)),
            envelope: Some(envelope),
            import_outcome: None,
        })
    }

    fn import(
        &self,
        obj: &serde_json::Map<String, Value>,
    ) -> Result<CapabilityAtomsExchangeResponse> {
        let envelope = obj
            .get("envelope")
            .ok_or_else(|| Error::config("tool_capability_atoms_exchange", "missing envelope"))?;
        let envelope_json = match envelope {
            Value::String(raw) => raw.trim().to_string(),
            other => serde_json::to_string(other).map_err(|error| {
                Error::config("tool_capability_atoms_exchange", error.to_string())
            })?,
        };
        if envelope_json.is_empty() {
            return Err(Error::config(
                "tool_capability_atoms_exchange",
                "empty envelope payload",
            ));
        }
        let import_outcome = import_capability_atom_exchange_envelope(
            self.skill_storage.as_ref(),
            &envelope_json,
            current_unix_secs(),
        )?;
        let atom = find_capability_atom(self.skill_storage.as_ref(), &import_outcome.name)
            .map(capability_atom_exchange_view);
        Ok(CapabilityAtomsExchangeResponse {
            ok: true,
            plane: EXCHANGE_PLANE,
            op: "import".to_string(),
            summary: None,
            records: Vec::new(),
            atom,
            envelope: None,
            import_outcome: Some(import_outcome),
        })
    }
}

fn require_confirm(
    obj: &serde_json::Map<String, Value>,
    stage: &'static str,
    op: &str,
) -> Result<()> {
    if obj.get("confirm").and_then(Value::as_bool).unwrap_or(false) {
        Ok(())
    } else {
        Err(Error::config(stage, format!("{op} requires confirm=true")))
    }
}

#[derive(Clone, Debug)]
struct CapabilityAtomFilter {
    atom_name: Option<String>,
    topic: Option<String>,
}

impl CapabilityAtomFilter {
    fn matches(&self, record: &CapabilityAtomRecord) -> bool {
        self.atom_name
            .as_ref()
            .is_none_or(|value| value == &record.name)
            && self
                .topic
                .as_ref()
                .is_none_or(|value| value == &record.topic)
    }
}

fn atom_filter(obj: &serde_json::Map<String, Value>) -> Option<CapabilityAtomFilter> {
    let atom_name = obj
        .get("atom_name")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_string);
    let topic = obj
        .get("topic")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_string);
    (atom_name.is_some() || topic.is_some()).then_some(CapabilityAtomFilter { atom_name, topic })
}

fn select_capability_atom(
    storage: &dyn SkillStorage,
    obj: &serde_json::Map<String, Value>,
) -> Result<CapabilityAtomRecord> {
    let filter = atom_filter(obj).ok_or_else(|| {
        Error::config(
            "tool_capability_atoms_exchange",
            "export requires atom_name or topic",
        )
    })?;
    sorted_capability_atom_records(storage)
        .into_iter()
        .find(|record| filter.matches(record))
        .ok_or_else(|| {
            Error::config(
                "tool_capability_atoms_exchange",
                "capability atom not found",
            )
        })
}

fn find_capability_atom(
    storage: &dyn SkillStorage,
    atom_name: &str,
) -> Option<CapabilityAtomRecord> {
    list_capability_atom_records(storage)
        .into_iter()
        .find(|record| record.name == atom_name)
}

fn sorted_capability_atom_records(storage: &dyn SkillStorage) -> Vec<CapabilityAtomRecord> {
    let mut records = list_capability_atom_records(storage);
    records.sort_by(|left, right| {
        right
            .provenance
            .updated_at
            .cmp(&left.provenance.updated_at)
            .then_with(|| left.name.cmp(&right.name))
    });
    records
}

fn capability_atom_exchange_view(record: CapabilityAtomRecord) -> CapabilityAtomExchangeRecordView {
    CapabilityAtomExchangeRecordView {
        name: record.name,
        topic: record.topic,
        title: record.title,
        trust: capability_atom_trust_label(record.trust).to_string(),
        source_kind: capability_atom_source_kind_label(record.provenance.source_kind).to_string(),
        requires_local_adjudication: record.provenance.requires_local_adjudication,
        component_count: record.components.len(),
        updated_at: record.provenance.updated_at,
        imported_at: record.provenance.imported_at,
        exported_at: record.provenance.exported_at,
        exchange_ready: !record.provenance.requires_local_adjudication
            && !matches!(
                record.trust,
                CapabilityAtomTrustLevel::ImportedPendingAdjudication
            ),
    }
}

fn capability_atom_trust_label(trust: CapabilityAtomTrustLevel) -> &'static str {
    match trust {
        CapabilityAtomTrustLevel::LocalVerified => "local_verified",
        CapabilityAtomTrustLevel::ImportedPendingAdjudication => "imported_pending_adjudication",
        CapabilityAtomTrustLevel::ImportedAdopted => "imported_adopted",
    }
}

fn capability_atom_source_kind_label(kind: CapabilityAtomSourceKind) -> &'static str {
    match kind {
        CapabilityAtomSourceKind::RuntimeSkill => "runtime_skill",
        CapabilityAtomSourceKind::ImportedAtom => "imported_atom",
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::skills::{
        record_runtime_skill_outcomes, runtime_skill_name_for_topic,
        sync_capability_atoms_from_runtime_skills, upsert_runtime_skill, RuntimeSkillReuseOutcome,
        RuntimeSkillWrite,
    };
    use std::collections::HashMap;
    use std::sync::Mutex;

    #[derive(Default)]
    struct TestSkillStorage {
        files: Mutex<HashMap<String, Vec<u8>>>,
    }

    impl SkillStorage for TestSkillStorage {
        fn list_names(&self) -> Result<Vec<String>> {
            Ok(self
                .files
                .lock()
                .unwrap_or_else(|error| error.into_inner())
                .keys()
                .cloned()
                .collect())
        }

        fn read(&self, name: &str) -> Result<Vec<u8>> {
            self.files
                .lock()
                .unwrap_or_else(|error| error.into_inner())
                .get(name)
                .cloned()
                .ok_or_else(|| Error::config("capability_atoms_exchange_test_read", "missing"))
        }

        fn write(&self, name: &str, content: &[u8]) -> Result<()> {
            self.files
                .lock()
                .unwrap_or_else(|error| error.into_inner())
                .insert(name.to_string(), content.to_vec());
            Ok(())
        }

        fn remove(&self, name: &str) -> Result<()> {
            self.files
                .lock()
                .unwrap_or_else(|error| error.into_inner())
                .remove(name);
            Ok(())
        }
    }

    struct DummyCtx;

    impl ToolContext for DummyCtx {
        fn get_with_headers(
            &mut self,
            _url: &str,
            _headers: &[(&str, &str)],
        ) -> Result<(u16, crate::platform::ResponseBody)> {
            Err(Error::config(
                "capability_atoms_exchange_test",
                "network unused",
            ))
        }

        fn post_with_headers(
            &mut self,
            _url: &str,
            _headers: &[(&str, &str)],
            _body: &[u8],
        ) -> Result<(u16, crate::platform::ResponseBody)> {
            Err(Error::config(
                "capability_atoms_exchange_test",
                "network unused",
            ))
        }

        fn user_locale(&self) -> crate::i18n::Locale {
            crate::i18n::Locale::Zh
        }
    }

    fn seed_capability_atom(storage: &dyn SkillStorage, topic: &str) -> String {
        let skill_name = runtime_skill_name_for_topic(topic);
        upsert_runtime_skill(
            storage,
            &RuntimeSkillWrite {
                name: skill_name.clone(),
                topic: topic.to_string(),
                title: "Serial framing".to_string(),
                summary: "Promote a reusable serial framing macro.".to_string(),
                content: "1. detect sync word\n2. confirm checksum\n3. emit frame".to_string(),
                citations: vec!["transcript:chat#message=1".to_string()],
                source_chat_id: Some("chat-1".to_string()),
                observed_at: 7_100_000_000,
            },
        )
        .expect("write runtime skill");
        record_runtime_skill_outcomes(
            storage,
            std::slice::from_ref(&skill_name),
            RuntimeSkillReuseOutcome::Succeeded,
            7_100_000_100,
            "validated reusable macro",
        )
        .expect("record runtime skill outcome");
        sync_capability_atoms_from_runtime_skills(storage, 7_100_000_200).expect("sync atoms");
        format!("capability_atom__{topic}")
    }

    #[test]
    fn inspect_reports_local_exchange_ready_atoms() {
        let storage: Arc<dyn SkillStorage + Send + Sync> = Arc::new(TestSkillStorage::default());
        let atom_name = seed_capability_atom(storage.as_ref(), "serial_framing");
        let tool = CapabilityAtomsInspectTool::new(Arc::clone(&storage));
        let mut ctx = DummyCtx;

        let output = tool.execute(r#"{}"#, &mut ctx).expect("inspect output");
        let parsed: Value = serde_json::from_str(&output).expect("inspect json");

        assert_eq!(parsed["ok"], Value::Bool(true));
        assert_eq!(parsed["plane"], Value::String(INSPECT_PLANE.to_string()));
        assert_eq!(parsed["summary"]["total"], Value::from(1));
        assert_eq!(parsed["records"][0]["name"], Value::String(atom_name));
        assert_eq!(
            parsed["records"][0]["trust"],
            Value::String("local_verified".to_string())
        );
        assert_eq!(parsed["records"][0]["exchange_ready"], Value::Bool(true));
    }

    #[test]
    fn export_and_import_round_trip_preserves_pending_adjudication_boundary() {
        let source_storage: Arc<dyn SkillStorage + Send + Sync> =
            Arc::new(TestSkillStorage::default());
        let atom_name = seed_capability_atom(source_storage.as_ref(), "protocol_bridge");
        let source_tool = CapabilityAtomsExchangeTool::new(Arc::clone(&source_storage));
        let mut ctx = DummyCtx;

        let export_output = source_tool
            .execute(
                &format!(r#"{{"op":"export","confirm":true,"atom_name":"{atom_name}"}}"#),
                &mut ctx,
            )
            .expect("export output");
        let export_parsed: Value = serde_json::from_str(&export_output).expect("export json");
        let envelope = export_parsed["envelope"]
            .as_str()
            .expect("envelope string")
            .to_string();

        let target_storage: Arc<dyn SkillStorage + Send + Sync> =
            Arc::new(TestSkillStorage::default());
        let target_tool = CapabilityAtomsExchangeTool::new(Arc::clone(&target_storage));
        let import_output = target_tool
            .execute(
                &serde_json::json!({
                    "op": "import",
                    "confirm": true,
                    "envelope": serde_json::from_str::<Value>(&envelope).expect("embedded envelope"),
                })
                .to_string(),
                &mut ctx,
            )
            .expect("import output");
        let import_parsed: Value = serde_json::from_str(&import_output).expect("import json");

        assert_eq!(
            import_parsed["import_outcome"]["trust"],
            Value::String("imported_pending_adjudication".to_string())
        );
        assert_eq!(
            import_parsed["atom"]["trust"],
            Value::String("imported_pending_adjudication".to_string())
        );
        assert_eq!(
            import_parsed["atom"]["requires_local_adjudication"],
            Value::Bool(true)
        );
        assert_eq!(import_parsed["atom"]["exchange_ready"], Value::Bool(false));
    }

    #[test]
    fn metadata_exposes_conservative_governed_write_contract() {
        let storage: Arc<dyn SkillStorage + Send + Sync> = Arc::new(TestSkillStorage::default());
        let tool = CapabilityAtomsExchangeTool::new(storage);
        let metadata = tool.metadata();

        assert_eq!(metadata.effect_class, ToolEffectClass::PersistentStateWrite);
        assert_eq!(metadata.risk_level, ToolRiskLevel::Medium);
        assert_eq!(metadata.approval_mode, ToolApprovalMode::ExplicitIntent);
        assert!(!metadata.allow_in_system_ingress);
    }

    #[test]
    fn inspect_tool_metadata_exposes_read_only_automatic_contract() {
        let storage: Arc<dyn SkillStorage + Send + Sync> = Arc::new(TestSkillStorage::default());
        let tool = CapabilityAtomsInspectTool::new(storage);
        let metadata = tool.metadata();

        assert_eq!(metadata.effect_class, ToolEffectClass::ReadOnly);
        assert_eq!(metadata.risk_level, ToolRiskLevel::Low);
        assert_eq!(metadata.approval_mode, ToolApprovalMode::Automatic);
        assert!(metadata.allow_in_system_ingress);
    }

    #[test]
    fn exchange_execution_shape_requires_confirm_for_export_and_import() {
        let storage: Arc<dyn SkillStorage + Send + Sync> = Arc::new(TestSkillStorage::default());
        let tool = CapabilityAtomsExchangeTool::new(storage);

        let export = tool
            .execution_shape(r#"{"op":"export","atom_name":"demo"}"#)
            .expect("export shape");
        assert_eq!(export.effect_class, ToolEffectClass::PersistentStateWrite);
        assert_eq!(export.approval_mode, ToolApprovalMode::ExplicitIntent);
        assert!(!export.approval_granted);

        let import = tool
            .execution_shape(r#"{"op":"import","envelope":"{}"}"#)
            .expect("import shape");
        assert_eq!(import.effect_class, ToolEffectClass::PersistentStateWrite);
        assert_eq!(import.approval_mode, ToolApprovalMode::ExplicitIntent);
        assert!(!import.approval_granted);

        let confirmed_import = tool
            .execution_shape(r#"{"op":"import","confirm":true,"envelope":"{}"}"#)
            .expect("confirmed import shape");
        assert_eq!(
            confirmed_import.approval_mode,
            ToolApprovalMode::ExplicitIntent
        );
        assert!(confirmed_import.approval_granted);
    }

    #[test]
    fn exchange_execute_rejects_export_and_import_without_confirm() {
        let storage: Arc<dyn SkillStorage + Send + Sync> = Arc::new(TestSkillStorage::default());
        let atom_name = seed_capability_atom(storage.as_ref(), "cap_exchange_guard");
        let tool = CapabilityAtomsExchangeTool::new(storage);
        let mut ctx = DummyCtx;

        let export_error = tool
            .execute(
                &format!(r#"{{"op":"export","atom_name":"{atom_name}"}}"#),
                &mut ctx,
            )
            .expect_err("export without confirm must fail");
        assert!(export_error
            .to_string()
            .contains("export requires confirm=true"));

        let import_error = tool
            .execute(r#"{"op":"import","envelope":{"version":1}}"#, &mut ctx)
            .expect_err("import without confirm must fail");
        assert!(import_error
            .to_string()
            .contains("import requires confirm=true"));
    }
}
