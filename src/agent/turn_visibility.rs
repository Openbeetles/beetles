//! Typed execution-visibility facts for current-chat delivery projection.
//! 当前 chat 执行可见性的结构化事实真源。

/// Execution facts that delivery may project for the current chat.
/// 仅表达执行事实，不携带答复语义。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum TurnVisibilityFact<'a> {
    Acknowledged,
    Reasoning {
        round: u32,
    },
    RunningTool {
        tool: &'a str,
        index: usize,
        total: usize,
    },
    TaskPlanner,
    TaskStarted {
        resumed: bool,
    },
    TaskTerminal {
        status: TaskTerminalVisibilityStatus,
    },
    Finalizing,
}

/// Final task status that can be projected as a sticky terminal header.
/// 可作为 sticky header 呈现的任务终态。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum TaskTerminalVisibilityStatus {
    Completed,
    PartialComplete,
    Blocked,
    Aborted,
}
