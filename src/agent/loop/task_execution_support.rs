use super::*;

pub(super) fn should_consider_task_execution(
    msg: &crate::bus::PcMsg,
    has_tools: bool,
    pressure: crate::orchestrator::PressureLevel,
    request_semantics: crate::agent::request_semantics::RequestSemantics,
) -> bool {
    if msg.ingress != IngressKind::User || msg.is_group {
        return false;
    }
    if matches!(pressure, crate::orchestrator::PressureLevel::Critical) {
        return false;
    }
    if request_semantics.action_family
        == crate::agent::request_semantics::ActionFamily::TaskExecution
        || request_semantics.resume_relation
            == crate::agent::request_semantics::ResumeRelation::ResumeActiveTaskRun
    {
        return true;
    }
    if !matches!(
        request_semantics.execution_preference,
        crate::agent::request_semantics::ExecutionPreference::ToolFirst
    ) {
        return false;
    }
    if !has_durable_run_shape(msg, has_tools) {
        return false;
    }
    matches!(
        (
            request_semantics.action_family,
            request_semantics.resume_relation,
        ),
        (
            crate::agent::request_semantics::ActionFamily::ActionRequest,
            crate::agent::request_semantics::ResumeRelation::IndependentTurn
                | crate::agent::request_semantics::ResumeRelation::SwitchToNewRequest,
        )
    )
}

fn has_durable_run_shape(msg: &crate::bus::PcMsg, has_tools: bool) -> bool {
    let content = msg.content.trim();
    if content.is_empty() {
        return false;
    }
    let char_count = content.chars().count();
    let line_count = content
        .lines()
        .filter(|line| !line.trim().is_empty())
        .count();
    let separator_count = content
        .chars()
        .filter(|ch| matches!(ch, '\n' | ',' | '，' | '.' | '。' | ';' | '；'))
        .count();
    if has_tools {
        char_count >= TASK_EXECUTION_MIN_CHARS
            || line_count >= TASK_EXECUTION_MIN_LINES
            || separator_count >= TASK_EXECUTION_MIN_SEPARATORS
    } else {
        char_count >= TASK_EXECUTION_MIN_CHARS.saturating_mul(2)
            || line_count >= TASK_EXECUTION_MIN_LINES
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::agent::request_semantics::{
        ActionFamily, DisclosureSurface, EvidenceNeed, ExecutionPreference, RequestKind,
        RequestSemantics, ResumeRelation,
    };

    fn semantics(
        action_family: ActionFamily,
        resume_relation: ResumeRelation,
        execution_preference: ExecutionPreference,
    ) -> RequestSemantics {
        RequestSemantics {
            request_kind: RequestKind::General,
            evidence_need: EvidenceNeed::HostTool,
            disclosure_surface: DisclosureSurface::Governed,
            execution_preference,
            action_family,
            resume_relation,
            confidence: 100,
        }
    }

    #[test]
    fn short_turn_with_active_action_resume_semantics_is_rejected() {
        let msg =
            crate::bus::PcMsg::new_inbound("qq_channel", "chat-1", "继续", false).expect("message");
        assert!(!should_consider_task_execution(
            &msg,
            true,
            crate::orchestrator::PressureLevel::Normal,
            semantics(
                ActionFamily::ActiveAction,
                ResumeRelation::ResumeActiveAction,
                ExecutionPreference::ToolFirst,
            ),
        ));
    }

    #[test]
    fn short_turn_with_task_execution_semantics_is_accepted() {
        let msg =
            crate::bus::PcMsg::new_inbound("qq_channel", "chat-1", "继续", false).expect("message");
        assert!(should_consider_task_execution(
            &msg,
            true,
            crate::orchestrator::PressureLevel::Normal,
            semantics(
                ActionFamily::TaskExecution,
                ResumeRelation::ResumeActiveTaskRun,
                ExecutionPreference::ToolFirst,
            ),
        ));
    }

    #[test]
    fn short_turn_with_action_request_semantics_is_rejected_without_durable_shape() {
        let msg = crate::bus::PcMsg::new_inbound("qq_channel", "chat-1", "配一下 QQ 邮箱", false)
            .expect("message");
        assert!(!should_consider_task_execution(
            &msg,
            true,
            crate::orchestrator::PressureLevel::Normal,
            semantics(
                ActionFamily::ActionRequest,
                ResumeRelation::IndependentTurn,
                ExecutionPreference::ToolFirst,
            ),
        ));
    }

    #[test]
    fn short_turn_with_confirm_active_action_semantics_is_rejected() {
        let msg =
            crate::bus::PcMsg::new_inbound("qq_channel", "chat-1", "行", false).expect("message");
        assert!(!should_consider_task_execution(
            &msg,
            true,
            crate::orchestrator::PressureLevel::Normal,
            semantics(
                ActionFamily::ActiveAction,
                ResumeRelation::ConfirmActiveAction,
                ExecutionPreference::ToolFirst,
            ),
        ));
    }

    #[test]
    fn short_turn_with_supply_active_action_input_semantics_is_rejected() {
        let msg = crate::bus::PcMsg::new_inbound(
            "qq_channel",
            "chat-1",
            "授权码是 hqvqcibpdvqgbdba",
            false,
        )
        .expect("message");
        assert!(!should_consider_task_execution(
            &msg,
            true,
            crate::orchestrator::PressureLevel::Normal,
            semantics(
                ActionFamily::ActiveAction,
                ResumeRelation::SupplyActiveActionInput,
                ExecutionPreference::ToolFirst,
            ),
        ));
    }

    #[test]
    fn short_turn_with_cancel_active_action_semantics_is_rejected() {
        let msg = crate::bus::PcMsg::new_inbound("qq_channel", "chat-1", "先别配了", false)
            .expect("message");
        assert!(!should_consider_task_execution(
            &msg,
            true,
            crate::orchestrator::PressureLevel::Normal,
            semantics(
                ActionFamily::ActiveAction,
                ResumeRelation::DenyOrCancelActiveAction,
                ExecutionPreference::AnswerDirect,
            ),
        ));
    }

    #[test]
    fn short_turn_with_switch_to_new_request_semantics_is_rejected_without_durable_shape() {
        let msg = crate::bus::PcMsg::new_inbound(
            "qq_channel",
            "chat-1",
            "别配邮箱了，改成配 Telegram",
            false,
        )
        .expect("message");
        assert!(!should_consider_task_execution(
            &msg,
            true,
            crate::orchestrator::PressureLevel::Normal,
            semantics(
                ActionFamily::ActionRequest,
                ResumeRelation::SwitchToNewRequest,
                ExecutionPreference::ToolFirst,
            ),
        ));
    }

    #[test]
    fn short_turn_without_action_semantics_still_rejects() {
        let msg =
            crate::bus::PcMsg::new_inbound("qq_channel", "chat-1", "继续", false).expect("message");
        assert!(!should_consider_task_execution(
            &msg,
            true,
            crate::orchestrator::PressureLevel::Normal,
            semantics(
                ActionFamily::Conversation,
                ResumeRelation::IndependentTurn,
                ExecutionPreference::AnswerDirect,
            ),
        ));
    }

    #[test]
    fn structured_action_request_semantics_enters_durable_run_consideration() {
        let msg = crate::bus::PcMsg::new_inbound(
            "qq_channel",
            "chat-1",
            "帮我整理一套迁移方案：\n1. 盘点现有 QQ 邮箱配置\n2. 生成迁移步骤\n3. 记录风险和回滚办法",
            false,
        )
        .expect("message");
        assert!(should_consider_task_execution(
            &msg,
            true,
            crate::orchestrator::PressureLevel::Normal,
            semantics(
                ActionFamily::ActionRequest,
                ResumeRelation::IndependentTurn,
                ExecutionPreference::ToolFirst,
            ),
        ));
    }

    #[test]
    fn structured_switch_request_semantics_enters_durable_run_consideration() {
        let msg = crate::bus::PcMsg::new_inbound(
            "qq_channel",
            "chat-1",
            "别配邮箱了，改成做一套 Telegram 配置迁移方案：\n1. 检查当前状态\n2. 列缺失项\n3. 形成执行计划",
            false,
        )
        .expect("message");
        assert!(should_consider_task_execution(
            &msg,
            true,
            crate::orchestrator::PressureLevel::Normal,
            semantics(
                ActionFamily::ActionRequest,
                ResumeRelation::SwitchToNewRequest,
                ExecutionPreference::ToolFirst,
            ),
        ));
    }
}

pub(super) fn parse_task_execution_json<T>(raw: &str, stage: &'static str) -> Result<T>
where
    T: for<'de> Deserialize<'de>,
{
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return Err(crate::error::Error::config(stage, "empty llm response"));
    }
    serde_json::from_str(trimmed)
        .or_else(|_| {
            let body = trimmed
                .split_once("```")
                .and_then(|(_, rest)| rest.split_once('\n'))
                .and_then(|(_, rest)| rest.split_once("```"))
                .map(|(json, _)| json.trim())
                .ok_or_else(|| serde_json::Error::io(std::io::Error::other("no fence body")))?;
            serde_json::from_str(body)
        })
        .or_else(|_| {
            let start = trimmed.find('{').ok_or_else(|| {
                serde_json::Error::io(std::io::Error::other("no json object start"))
            })?;
            let end = trimmed.rfind('}').ok_or_else(|| {
                serde_json::Error::io(std::io::Error::other("no json object end"))
            })?;
            serde_json::from_str(&trimmed[start..=end])
        })
        .map_err(|error| crate::error::Error::config(stage, error.to_string()))
}

pub(super) fn persist_task_run_record(
    store: &dyn TaskRunStore,
    record: &TaskRunRecord,
    stage: &str,
) {
    if let Err(error) = store.upsert(record) {
        log::warn!(
            "[task_execution] failed to persist run stage={} run_id={}: {}",
            stage,
            record.run.run_id,
            error
        );
    }
}

pub(super) fn persist_task_artifact_record(
    store: &dyn TaskArtifactStore,
    record: &TaskArtifactRecord,
    stage: &str,
) {
    if let Err(error) = store.put(record) {
        log::warn!(
            "[task_execution] failed to persist artifact stage={} run_id={} artifact_id={}: {}",
            stage,
            record.artifact.run_id,
            record.artifact.artifact_id,
            error
        );
    }
}

pub(super) fn append_task_execution_ledger_entry(
    store: &dyn TaskExecutionLedgerStore,
    entry: &TaskExecutionLedgerEntry,
    stage: &str,
) {
    if let Err(error) = store.append(&entry.run_id, entry) {
        log::warn!(
            "[task_execution] failed to append ledger stage={} run_id={} seq={}: {}",
            stage,
            entry.run_id,
            entry.sequence,
            error
        );
    }
}
