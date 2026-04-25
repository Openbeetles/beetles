//! 用户可见文案枚举与中英 `tr` 实现。
//! User-visible message ids and zh/en resolution.

use super::Locale;

/// 传感器阈值告警类型（用于 [`Message::SensorWatchAlert`]）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SensorWatchThresholdKind {
    Above,
    Below,
    Change,
}

/// 固件侧所有经 API / 通道展示给用户的中英文案（强类型 id）。
#[derive(Clone, Debug, PartialEq)]
pub enum Message {
    PairingRequired,
    PairingCodeWrong,
    OperationFailed,
    InvalidJson,
    CodeMustBe6Digits,
    PairingCodeAlreadySet,
    FailedToSaveCode,
    SaveFailed,
    ContentTooLong,
    InvalidUrl,
    BodyReadFailed,
    InvalidUtf8,
    BodyTooLarge,
    SkillNotFound,
    MissingNameQuery,
    WebhookDisabled,
    InvalidToken,
    QueueFull,
    MissingNameForWrite,
    MissingNameOrEnabled,
    MissingOrderNameContent,
    MissingUrl,
    MissingName,
    UrlBodyNotUtf8,
    OtaChannelNotConfigured,
    OtaCheckFail,
    OtaDownload,
    OtaValidate,
    OtaWrite,
    /// CSRF 校验失败（写操作）
    CsrfInvalidToken,
    CsrfTokenRequired,
    // --- tr_error 桶（阶段 1 粗粒度；阶段 3 细化 Config）---
    ErrorNvs,
    ErrorSpiffs,
    ErrorIo,
    ErrorEsp,
    ErrorHttpStatus {
        code: u16,
    },
    ErrorProxyUnsupported,
    // --- 阶段 2：system_info / 连通性 ---
    SystemStatusOk,
    SystemStatusWifiDisconnected,
    SystemStatusStorage,
    SystemStatusChannel,
    SystemStatusRunning,
    ChannelConnectivityUnavailable,
    ConnectivityNotConfigured,
    ConnectivityCheckFailed,
    ConnectivityTokenInvalid,
    ConnectivitySessionReplyOnly,
    // --- 阶段 4+：Agent / 提醒 ---
    LowMemoryUserDefer,
    NodeMaintenance,
    ReplyTruncated,
    StreamLowMemoryOmitted,
    RemindPrefix,
    TaskDue {
        title: String,
    },
    InitiativeUpcomingReminder {
        minutes: u64,
    },
    InitiativeResumeTaskCheckIn {
        idle_minutes: u64,
        high_priority: bool,
    },
    // --- 阶段 5：Telegram / 工具 ---
    TgActivationMention,
    TgActivationAlways,
    TgSessionCleared,
    /// Telegram /status：WiFi 与队列深度
    TelegramStatus {
        wifi_connected: bool,
        inbound: usize,
        outbound: usize,
    },
    BindHintEmpty,
    BindHintNotInList,
    RemindAtSetOk,
    SessionSummaryUpdated,
    SensorWatchAlert {
        id: String,
        label: String,
        value: f64,
        threshold: f64,
        threshold_kind: SensorWatchThresholdKind,
    },
    /// 持久化 cron 到点推入通道的展示前缀（后跟 `action` 原文）
    CronTaskFired {
        id: String,
        action: String,
    },
    /// HEARTBEAT.md 有待办时注入入站的用户提示
    HeartbeatPendingTasksReminder,
    /// 未配置有效 LLM 源时 Noop 客户端返回
    LlmNotConfigured,
    /// 配置校验未通过（细分见 tr_error 映射；兜底）
    ConfigRejected,
    LocaleMustBeZhOrEn,
    TgGroupActivationInvalid,
    ConfigFieldTooLong,
    ConfigEnabledChannelInvalid,
    ConfigLlmSourcesEmpty,
    ConfigLlmIndicesInvalid,
    ConfigLlmSourceFieldLen,
    ConfigChannelFieldLen,
    ConfigHardwareInvalid,
    ConfigDisplayInvalid,
}

/// 将消息 id 转为当前语言的展示字符串。
pub fn tr(msg: Message, loc: Locale) -> String {
    let zh = |s: &'static str| s.to_string();
    let en = |s: &'static str| s.to_string();
    match loc {
        Locale::Zh => match msg {
            Message::PairingRequired => zh("请先设置配对码"),
            Message::PairingCodeWrong => zh("配对码错误"),
            Message::OperationFailed => zh("操作失败，请重试"),
            Message::InvalidJson => zh("请求体不是合法 JSON"),
            Message::CodeMustBe6Digits => zh("配对码须为 6 位数字"),
            Message::PairingCodeAlreadySet => zh("配对码已设置，无法修改"),
            Message::FailedToSaveCode => zh("保存配对码失败"),
            Message::SaveFailed => zh("保存失败"),
            Message::ContentTooLong => zh("内容过长"),
            Message::InvalidUrl => zh("无效的 URL"),
            Message::BodyReadFailed => zh("读取请求体失败"),
            Message::InvalidUtf8 => zh("请求体不是合法 UTF-8"),
            Message::BodyTooLarge => zh("请求体过大"),
            Message::SkillNotFound => zh("未找到该技能"),
            Message::MissingNameQuery => zh("缺少 name 参数"),
            Message::WebhookDisabled => zh("Webhook 未启用"),
            Message::InvalidToken => zh("Token 无效"),
            Message::QueueFull => zh("队列已满，请稍后重试"),
            Message::MissingNameForWrite => zh("缺少技能名称"),
            Message::MissingNameOrEnabled => zh("缺少 name 或 enabled"),
            Message::MissingOrderNameContent => zh("缺少 order、name+content 或 name+enabled"),
            Message::MissingUrl => zh("缺少 url"),
            Message::MissingName => zh("缺少 name"),
            Message::UrlBodyNotUtf8 => zh("URL 返回内容不是合法 UTF-8"),
            Message::OtaChannelNotConfigured => zh("渠道未配置"),
            Message::OtaCheckFail => zh("检查更新失败，请稍后重试"),
            Message::OtaDownload => zh("网络或下载失败，请检查网络后重试"),
            Message::OtaValidate => zh("固件校验失败，请更换固件来源"),
            Message::OtaWrite => zh("写入失败，请勿断电并重试"),
            Message::CsrfInvalidToken => zh("CSRF 令牌无效"),
            Message::CsrfTokenRequired => zh("需要 CSRF 令牌"),
            Message::ErrorNvs => zh("存储访问失败，请稍后重试"),
            Message::ErrorSpiffs => zh("文件存储异常，请稍后重试"),
            Message::ErrorIo => zh("读写失败，请稍后重试"),
            Message::ErrorEsp => zh("设备错误，请重启后重试"),
            Message::ErrorHttpStatus { code } => {
                format!("远程服务返回错误（HTTP {}）", code)
            }
            Message::ErrorProxyUnsupported => zh("已配置代理但当前固件未实现代理隧道"),
            Message::SystemStatusOk => zh("正常"),
            Message::SystemStatusWifiDisconnected => zh("WiFi 未连接"),
            Message::SystemStatusStorage => zh("存储异常"),
            Message::SystemStatusChannel => zh("通道异常"),
            Message::SystemStatusRunning => zh("运行中"),
            Message::ChannelConnectivityUnavailable => zh("无法检查通道连通性"),
            Message::ConnectivityNotConfigured => zh("未配置"),
            Message::ConnectivityCheckFailed => zh("检查失败，请查看网络"),
            Message::ConnectivityTokenInvalid => zh("凭证无效或已过期"),
            Message::ConnectivitySessionReplyOnly => {
                zh("仅支持会话回调回复，未配置主动 Webhook")
            }
            Message::LowMemoryUserDefer => zh("设备内存紧张，请稍后再试。"),
            Message::NodeMaintenance => zh("节点正在维护，请稍后..."),
            Message::ReplyTruncated => zh("（回复因长度限制被截断）"),
            Message::StreamLowMemoryOmitted => zh("（因设备内存不足，后续步骤已省略）"),
            Message::RemindPrefix => zh("提醒："),
            Message::TaskDue { ref title } => format!("任务到期：{}", title),
            Message::InitiativeUpcomingReminder { minutes } => format!(
                "有一个提醒将在约 {} 分钟后到点。不要打断当前方向；简短提示时间压力，并确认是否要先收尾或调整优先级。",
                minutes
            ),
            Message::InitiativeResumeTaskCheckIn {
                idle_minutes,
                high_priority,
            } => {
                if high_priority {
                    format!(
                        "当前有高优先级的进行中事项已停下约 {} 分钟。简短询问用户是否要继续推进；不要强推，也不要制造压力。",
                        idle_minutes
                    )
                } else {
                    format!(
                        "当前有进行中的事项已停下约 {} 分钟。简短询问用户是否要继续推进；不要强推，也不要制造压力。",
                        idle_minutes
                    )
                }
            }
            Message::TgActivationMention => zh("已切换为 mention"),
            Message::TgActivationAlways => zh("已切换为 always"),
            Message::TgSessionCleared => zh("会话已清空"),
            Message::TelegramStatus {
                wifi_connected,
                inbound,
                outbound,
            } => {
                let w = if wifi_connected {
                    "已连接"
                } else {
                    "未连接"
                };
                format!("WiFi: {}，入站: {}，出站: {}", w, inbound, outbound)
            }
            Message::BindHintEmpty => {
                zh("绑定：请设置 BEETLE_TG_ALLOWED_CHAT_IDS=<你的 chat_id> 后重新编译。")
            }
            Message::BindHintNotInList => {
                zh("绑定：请将你的 chat_id 加入 BEETLE_TG_ALLOWED_CHAT_IDS（逗号分隔）后重新编译。")
            }
            Message::RemindAtSetOk => zh("已设置提醒。"),
            Message::SessionSummaryUpdated => zh("已更新会话摘要。"),
            Message::SensorWatchAlert {
                ref id,
                ref label,
                value,
                threshold,
                threshold_kind,
            } => {
                let k = match threshold_kind {
                    SensorWatchThresholdKind::Above => "高于",
                    SensorWatchThresholdKind::Below => "低于",
                    SensorWatchThresholdKind::Change => "变化",
                };
                format!(
                    "传感器告警 [{}] {}: 当前值={:.2}, 阈值={:.2} ({})",
                    id, label, value, threshold, k
                )
            }
            Message::CronTaskFired { ref id, ref action } => {
                format!("定时任务 [{}]: {}", id, action)
            }
            Message::HeartbeatPendingTasksReminder => {
                zh("请根据 HEARTBEAT.md 中的待办事项执行并更新文件。")
            }
            Message::LlmNotConfigured => {
                zh("LLM 未配置或配置无效，请通过 Web UI / 配置 API 设置 llm_sources 或 api_key。")
            }
            Message::ConfigRejected => zh("配置无效，请检查字段后重试"),
            Message::LocaleMustBeZhOrEn => zh("语言须为 zh 或 en"),
            Message::TgGroupActivationInvalid => zh("群组激活模式须为 mention 或 always"),
            Message::ConfigFieldTooLong => zh("字段长度超出限制"),
            Message::ConfigEnabledChannelInvalid => zh("enabled_channel 取值无效"),
            Message::ConfigLlmSourcesEmpty => zh("llm_sources 不能为空"),
            Message::ConfigLlmIndicesInvalid => zh("llm_router / llm_worker 源下标无效"),
            Message::ConfigLlmSourceFieldLen => zh("某个 LLM 源字段过长"),
            Message::ConfigChannelFieldLen => zh("通道配置字段过长"),
            Message::ConfigHardwareInvalid => zh("硬件配置无效"),
            Message::ConfigDisplayInvalid => zh("显示配置无效"),
        },
        Locale::En => match msg {
            Message::PairingRequired => en("Please set pairing code first"),
            Message::PairingCodeWrong => en("Wrong pairing code"),
            Message::OperationFailed => en("Operation failed, please try again"),
            Message::InvalidJson => en("Request body is not valid JSON"),
            Message::CodeMustBe6Digits => en("Pairing code must be 6 digits"),
            Message::PairingCodeAlreadySet => en("Pairing code already set"),
            Message::FailedToSaveCode => en("Failed to save pairing code"),
            Message::SaveFailed => en("Save failed"),
            Message::ContentTooLong => en("Content too long"),
            Message::InvalidUrl => en("Invalid URL"),
            Message::BodyReadFailed => en("Failed to read request body"),
            Message::InvalidUtf8 => en("Request body is not valid UTF-8"),
            Message::BodyTooLarge => en("Request body too large"),
            Message::SkillNotFound => en("Skill not found"),
            Message::MissingNameQuery => en("Missing name parameter"),
            Message::WebhookDisabled => en("Webhook is disabled"),
            Message::InvalidToken => en("Invalid token"),
            Message::QueueFull => en("Queue full, try again later"),
            Message::MissingNameForWrite => en("Missing skill name for write"),
            Message::MissingNameOrEnabled => en("Missing name or enabled"),
            Message::MissingOrderNameContent => en("Missing order, name+content, or name+enabled"),
            Message::MissingUrl => en("Missing URL"),
            Message::MissingName => en("Missing name"),
            Message::UrlBodyNotUtf8 => en("URL body is not valid UTF-8"),
            Message::OtaChannelNotConfigured => en("Update channel not configured"),
            Message::OtaCheckFail => en("Check for update failed, try again later"),
            Message::OtaDownload => en("Network or download failed, check connection and retry"),
            Message::OtaValidate => en("Firmware verification failed, try another source"),
            Message::OtaWrite => en("Write failed, do not power off and retry"),
            Message::CsrfInvalidToken => en("Invalid CSRF token"),
            Message::CsrfTokenRequired => en("CSRF token required"),
            Message::ErrorNvs => en("Storage access failed, try again later"),
            Message::ErrorSpiffs => en("File storage error, try again later"),
            Message::ErrorIo => en("Read/write failed, try again later"),
            Message::ErrorEsp => en("Device error, restart and try again"),
            Message::ErrorHttpStatus { code } => {
                format!("Remote service error (HTTP {})", code)
            }
            Message::ErrorProxyUnsupported => {
                en("Proxy is configured but this firmware build does not support proxy tunnels")
            }
            Message::SystemStatusOk => en("OK"),
            Message::SystemStatusWifiDisconnected => en("WiFi disconnected"),
            Message::SystemStatusStorage => en("Storage error"),
            Message::SystemStatusChannel => en("Channel error"),
            Message::SystemStatusRunning => en("Running"),
            Message::ChannelConnectivityUnavailable => en("Channel connectivity check unavailable"),
            Message::ConnectivityNotConfigured => en("Not configured"),
            Message::ConnectivityCheckFailed => en("Check failed, verify network connection"),
            Message::ConnectivityTokenInvalid => en("Invalid or expired credentials"),
            Message::ConnectivitySessionReplyOnly => {
                en("Session reply mode only; proactive webhook is not configured")
            }
            Message::LowMemoryUserDefer => en("Device is low on memory, please try again later."),
            Message::NodeMaintenance => en("Node is under maintenance, please wait..."),
            Message::ReplyTruncated => en("(Reply truncated due to length limit)"),
            Message::StreamLowMemoryOmitted => {
                en("(Following steps omitted due to low device memory)")
            }
            Message::RemindPrefix => en("Reminder: "),
            Message::TaskDue { ref title } => format!("Task due: {}", title),
            Message::InitiativeUpcomingReminder { minutes } => format!(
                "A reminder is due in about {} minutes. Do not derail the current task; briefly flag the time pressure and ask whether to wrap up or adjust priority.",
                minutes
            ),
            Message::InitiativeResumeTaskCheckIn {
                idle_minutes,
                high_priority,
            } => {
                if high_priority {
                    format!(
                        "A high-priority in-progress task has been idle for about {} minutes. Briefly ask whether to continue, without pushing or creating pressure.",
                        idle_minutes
                    )
                } else {
                    format!(
                        "An in-progress task has been idle for about {} minutes. Briefly ask whether to continue, without pushing or creating pressure.",
                        idle_minutes
                    )
                }
            }
            Message::TgActivationMention => en("Switched to mention"),
            Message::TgActivationAlways => en("Switched to always"),
            Message::TgSessionCleared => en("Session cleared"),
            Message::TelegramStatus {
                wifi_connected,
                inbound,
                outbound,
            } => {
                let w = if wifi_connected {
                    "connected"
                } else {
                    "disconnected"
                };
                format!("WiFi: {}, inbound: {}, outbound: {}", w, inbound, outbound)
            }
            Message::BindHintEmpty => {
                en("Bind: set BEETLE_TG_ALLOWED_CHAT_IDS=<your_chat_id> and rebuild.")
            }
            Message::BindHintNotInList => en(
                "Bind: add your chat_id to BEETLE_TG_ALLOWED_CHAT_IDS (comma-separated) and rebuild.",
            ),
            Message::RemindAtSetOk => en("Reminder set."),
            Message::SessionSummaryUpdated => en("Session summary updated."),
            Message::SensorWatchAlert {
                ref id,
                ref label,
                value,
                threshold,
                threshold_kind,
            } => {
                let k = match threshold_kind {
                    SensorWatchThresholdKind::Above => "above",
                    SensorWatchThresholdKind::Below => "below",
                    SensorWatchThresholdKind::Change => "change",
                };
                format!(
                    "Sensor alert [{}] {}: current={:.2}, threshold={:.2} ({})",
                    id, label, value, threshold, k
                )
            }
            Message::CronTaskFired { ref id, ref action } => {
                format!("Scheduled task [{}]: {}", id, action)
            }
            Message::HeartbeatPendingTasksReminder => {
                en("Please follow pending tasks in HEARTBEAT.md and update the file.")
            }
            Message::LlmNotConfigured => en(
                "LLM is not configured or invalid. Set llm_sources or api_key via Web UI / config API.",
            ),
            Message::ConfigRejected => en("Invalid configuration, check fields and try again"),
            Message::LocaleMustBeZhOrEn => en("Locale must be zh or en"),
            Message::TgGroupActivationInvalid => {
                en("tg_group_activation must be mention or always")
            }
            Message::ConfigFieldTooLong => en("A field exceeds the maximum length"),
            Message::ConfigEnabledChannelInvalid => en("enabled_channel value is invalid"),
            Message::ConfigLlmSourcesEmpty => en("llm_sources must not be empty"),
            Message::ConfigLlmIndicesInvalid => {
                en("llm_router_source_index / llm_worker_source_index out of range")
            }
            Message::ConfigLlmSourceFieldLen => en("An LLM source field exceeds the length limit"),
            Message::ConfigChannelFieldLen => en("A channel field exceeds the length limit"),
            Message::ConfigHardwareInvalid => en("Invalid hardware configuration"),
            Message::ConfigDisplayInvalid => en("Invalid display configuration"),
        },
    }
}
