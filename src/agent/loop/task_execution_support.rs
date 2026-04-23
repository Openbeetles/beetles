use super::*;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum FormalTaskAdmission {
    None,
    ConsiderNewRun,
}

pub(super) fn decide_formal_task_admission(
    msg: &crate::bus::PcMsg,
    has_tools: bool,
    pressure: crate::orchestrator::PressureLevel,
    deliberation_class: crate::memory::TurnDeliberationClass,
    request_semantics: crate::agent::request_semantics::RequestSemantics,
    reply_surface: crate::agent::reply_surface::ReplySurface,
    has_active_run: bool,
) -> FormalTaskAdmission {
    if msg.ingress != IngressKind::User || msg.is_group {
        return FormalTaskAdmission::None;
    }
    if matches!(pressure, crate::orchestrator::PressureLevel::Critical) {
        return FormalTaskAdmission::None;
    }
    if has_active_run {
        return FormalTaskAdmission::ConsiderNewRun;
    }
    if reply_surface != crate::agent::reply_surface::ReplySurface::GovernedConversation {
        return FormalTaskAdmission::None;
    }
    if request_semantics.action_family
        == crate::agent::request_semantics::ActionFamily::ActiveAction
    {
        return FormalTaskAdmission::None;
    }
    if deliberation_class == crate::memory::TurnDeliberationClass::HardReasoning
        && has_durable_run_shape(msg, has_tools)
    {
        FormalTaskAdmission::ConsiderNewRun
    } else {
        FormalTaskAdmission::None
    }
}

fn has_durable_run_shape(msg: &crate::bus::PcMsg, has_tools: bool) -> bool {
    let content = msg.content.trim();
    if content.is_empty() {
        return false;
    }
    let metrics = crate::agent::request_semantics::request_shape_metrics_without_colons(content);
    if has_tools {
        metrics.char_count >= TASK_EXECUTION_MIN_CHARS
            || metrics.line_count >= TASK_EXECUTION_MIN_LINES
            || metrics.separator_count >= TASK_EXECUTION_MIN_SEPARATORS
    } else {
        metrics.char_count >= TASK_EXECUTION_MIN_CHARS.saturating_mul(2)
            || metrics.line_count >= TASK_EXECUTION_MIN_LINES
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::agent::reply_surface::ReplySurface;
    use crate::agent::request_semantics::{
        ActionFamily, DisclosureSurface, EvidenceNeed, ExecutionPreference, RequestKind,
        RequestSemantics,
    };
    use crate::memory::TurnDeliberationClass;

    fn semantics(
        action_family: ActionFamily,
        execution_preference: ExecutionPreference,
    ) -> RequestSemantics {
        RequestSemantics {
            request_kind: RequestKind::General,
            evidence_need: EvidenceNeed::HostTool,
            disclosure_surface: DisclosureSurface::Governed,
            execution_preference,
            action_family,
            confidence: 100,
        }
    }

    #[test]
    fn short_turn_with_active_action_resume_semantics_is_rejected() {
        let msg =
            crate::bus::PcMsg::new_inbound("qq_channel", "chat-1", "继续", false).expect("message");
        assert_eq!(
            decide_formal_task_admission(
                &msg,
                true,
                crate::orchestrator::PressureLevel::Normal,
                TurnDeliberationClass::Standard,
                semantics(ActionFamily::ActiveAction, ExecutionPreference::ToolFirst),
                ReplySurface::GovernedConversation,
                false,
            ),
            FormalTaskAdmission::None
        );
    }

    #[test]
    fn active_formal_run_always_enters_planner_consideration_even_for_short_followup_turns() {
        let msg =
            crate::bus::PcMsg::new_inbound("qq_channel", "chat-1", "继续", false).expect("message");
        let semantics = semantics(ActionFamily::TaskExecution, ExecutionPreference::ToolFirst);
        assert_eq!(
            decide_formal_task_admission(
                &msg,
                true,
                crate::orchestrator::PressureLevel::Normal,
                TurnDeliberationClass::Standard,
                semantics,
                ReplySurface::GovernedConversation,
                false,
            ),
            FormalTaskAdmission::None
        );
        assert_eq!(
            decide_formal_task_admission(
                &msg,
                true,
                crate::orchestrator::PressureLevel::Normal,
                TurnDeliberationClass::Standard,
                semantics,
                ReplySurface::GovernedConversation,
                true,
            ),
            FormalTaskAdmission::ConsiderNewRun
        );
    }

    #[test]
    fn structured_action_request_semantics_alone_no_longer_enters_formal_task_consideration() {
        let msg = crate::bus::PcMsg::new_inbound(
            "qq_channel",
            "chat-1",
            "帮我整理一套迁移方案：\n1. 盘点现有 QQ 邮箱配置\n2. 生成迁移步骤\n3. 记录风险和回滚办法",
            false,
        )
        .expect("message");
        assert_eq!(
            decide_formal_task_admission(
                &msg,
                true,
                crate::orchestrator::PressureLevel::Normal,
                TurnDeliberationClass::Standard,
                semantics(ActionFamily::ActiveAction, ExecutionPreference::ToolFirst),
                ReplySurface::GovernedConversation,
                false,
            ),
            FormalTaskAdmission::None
        );
    }

    #[test]
    fn medium_interactive_request_without_durable_multi_step_shape_stays_out_of_formal_task() {
        let msg = crate::bus::PcMsg::new_inbound(
            "qq_channel",
            "chat-1",
            "帮我看看邮箱里最新一封邮件",
            false,
        )
        .expect("message");
        assert_eq!(
            decide_formal_task_admission(
                &msg,
                true,
                crate::orchestrator::PressureLevel::Normal,
                TurnDeliberationClass::HardReasoning,
                semantics(ActionFamily::Conversation, ExecutionPreference::ToolFirst),
                ReplySurface::GovernedConversation,
                false,
            ),
            FormalTaskAdmission::None
        );
    }

    #[test]
    fn hard_reasoning_with_durable_shape_enters_durable_run_consideration() {
        let msg = crate::bus::PcMsg::new_inbound(
            "qq_channel",
            "chat-1",
            "帮我整理一套迁移方案：\n1. 盘点现有 QQ 邮箱配置\n2. 生成迁移步骤\n3. 记录风险和回滚办法",
            false,
        )
        .expect("message");
        assert_eq!(
            decide_formal_task_admission(
                &msg,
                true,
                crate::orchestrator::PressureLevel::Normal,
                TurnDeliberationClass::HardReasoning,
                semantics(
                    ActionFamily::Conversation,
                    ExecutionPreference::AnswerDirect
                ),
                ReplySurface::GovernedConversation,
                false,
            ),
            FormalTaskAdmission::ConsiderNewRun
        );
    }

    #[test]
    fn short_turn_even_with_hard_reasoning_is_rejected_without_durable_shape() {
        let msg = crate::bus::PcMsg::new_inbound("qq_channel", "chat-1", "配一下 QQ 邮箱", false)
            .expect("message");
        assert_eq!(
            decide_formal_task_admission(
                &msg,
                true,
                crate::orchestrator::PressureLevel::Normal,
                TurnDeliberationClass::HardReasoning,
                semantics(
                    ActionFamily::Conversation,
                    ExecutionPreference::AnswerDirect
                ),
                ReplySurface::GovernedConversation,
                false,
            ),
            FormalTaskAdmission::None
        );
    }

    #[test]
    fn short_turn_without_action_semantics_still_rejects() {
        let msg =
            crate::bus::PcMsg::new_inbound("qq_channel", "chat-1", "继续", false).expect("message");
        assert_eq!(
            decide_formal_task_admission(
                &msg,
                true,
                crate::orchestrator::PressureLevel::Normal,
                TurnDeliberationClass::Standard,
                semantics(
                    ActionFamily::Conversation,
                    ExecutionPreference::AnswerDirect
                ),
                ReplySurface::GovernedConversation,
                false,
            ),
            FormalTaskAdmission::None
        );
    }

    #[test]
    fn critical_pressure_blocks_new_durable_run_admission() {
        let msg = crate::bus::PcMsg::new_inbound(
            "qq_channel",
            "chat-1",
            "别配邮箱了，改成做一套 Telegram 配置迁移方案：\n1. 检查当前状态\n2. 列缺失项\n3. 形成执行计划",
            false,
        )
        .expect("message");
        assert_eq!(
            decide_formal_task_admission(
                &msg,
                true,
                crate::orchestrator::PressureLevel::Critical,
                TurnDeliberationClass::HardReasoning,
                semantics(
                    ActionFamily::Conversation,
                    ExecutionPreference::AnswerDirect
                ),
                ReplySurface::GovernedConversation,
                false,
            ),
            FormalTaskAdmission::None
        );
    }
}
