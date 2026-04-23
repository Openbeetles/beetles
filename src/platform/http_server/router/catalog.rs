//! Shared HTTP route catalog used by transport registration and dispatch.

#[cfg(any(test, target_arch = "xtensa", target_arch = "riscv32"))]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum RouteMethod {
    Get,
    Post,
    Delete,
    Options,
}

#[cfg(any(test, target_arch = "xtensa", target_arch = "riscv32"))]
impl RouteMethod {
    pub(crate) const fn as_str(self) -> &'static str {
        match self {
            Self::Get => "GET",
            Self::Post => "POST",
            Self::Delete => "DELETE",
            Self::Options => "OPTIONS",
        }
    }
}

#[cfg(any(test, target_arch = "xtensa", target_arch = "riscv32"))]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum RouteBodyMode {
    None,
    Utf8(usize),
}

#[cfg(any(test, target_arch = "xtensa", target_arch = "riscv32"))]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum RouteDispatchMode {
    Direct,
    Worker,
}

#[cfg(any(test, target_arch = "xtensa", target_arch = "riscv32"))]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum OperatorRouteAccess {
    Hidden,
    AlwaysOn,
    Windowed,
}

#[cfg(any(test, target_arch = "xtensa", target_arch = "riscv32"))]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct HttpRouteSpec {
    pub(crate) path: &'static str,
    pub(crate) method: RouteMethod,
    pub(crate) body_mode: RouteBodyMode,
    pub(crate) dispatch_mode: RouteDispatchMode,
    pub(crate) operator_access: OperatorRouteAccess,
}

#[cfg(any(test, target_arch = "xtensa", target_arch = "riscv32"))]
impl HttpRouteSpec {
    pub(crate) const fn direct(
        path: &'static str,
        method: RouteMethod,
        body_mode: RouteBodyMode,
    ) -> Self {
        Self {
            path,
            method,
            body_mode,
            dispatch_mode: RouteDispatchMode::Direct,
            operator_access: OperatorRouteAccess::Hidden,
        }
    }

    pub(crate) const fn worker(
        path: &'static str,
        method: RouteMethod,
        body_mode: RouteBodyMode,
    ) -> Self {
        Self {
            path,
            method,
            body_mode,
            dispatch_mode: RouteDispatchMode::Worker,
            operator_access: OperatorRouteAccess::Hidden,
        }
    }

    pub(crate) const fn direct_operator(
        path: &'static str,
        method: RouteMethod,
        body_mode: RouteBodyMode,
        operator_access: OperatorRouteAccess,
    ) -> Self {
        Self {
            path,
            method,
            body_mode,
            dispatch_mode: RouteDispatchMode::Direct,
            operator_access,
        }
    }

    pub(crate) const fn worker_operator(
        path: &'static str,
        method: RouteMethod,
        body_mode: RouteBodyMode,
        operator_access: OperatorRouteAccess,
    ) -> Self {
        Self {
            path,
            method,
            body_mode,
            dispatch_mode: RouteDispatchMode::Worker,
            operator_access,
        }
    }
}

pub(crate) const ROUTE_ROOT: &str = "/";
pub(crate) const ROUTE_PAIRING_CODE: &str = "/api/pairing_code";
pub(crate) const ROUTE_CONFIG_LLM: &str = "/api/config/llm";
pub(crate) const ROUTE_CONFIG_CHANNELS: &str = "/api/config/channels";
pub(crate) const ROUTE_CONFIG_SYSTEM: &str = "/api/config/system";
pub(crate) const ROUTE_CONFIG_HARDWARE: &str = "/api/config/hardware";
pub(crate) const ROUTE_CONFIG_AUDIO: &str = "/api/config/audio";
pub(crate) const ROUTE_CONFIG_DISPLAY: &str = "/api/config/display";
#[cfg(all(
    feature = "capability_office",
    not(any(target_arch = "xtensa", target_arch = "riscv32"))
))]
pub(crate) const ROUTE_CONFIG_ACCOUNTS: &str = "/api/config/accounts";
#[cfg(all(
    feature = "capability_office",
    not(any(target_arch = "xtensa", target_arch = "riscv32"))
))]
pub(crate) const ROUTE_CONFIG_ACCOUNTS_PREFIX: &str = "/api/config/accounts/";
#[cfg(all(
    feature = "capability_office",
    not(any(target_arch = "xtensa", target_arch = "riscv32"))
))]
pub(crate) const ROUTE_CONFIG_CAPABILITIES: &str = "/api/config/capabilities";
#[cfg(all(
    feature = "capability_office",
    not(any(target_arch = "xtensa", target_arch = "riscv32"))
))]
pub(crate) const ROUTE_CONFIG_CAPABILITIES_PREFIX: &str = "/api/config/capabilities/";
#[cfg(all(
    feature = "capability_office",
    not(any(target_arch = "xtensa", target_arch = "riscv32"))
))]
pub(crate) const ROUTE_CONFIG_PROVIDERS: &str = "/api/config/providers";
pub(crate) const ROUTE_WIFI_SCAN: &str = "/api/wifi/scan";
pub(crate) const ROUTE_HARDWARE_DISCOVERY: &str = "/api/hardware/discovery";
pub(crate) const ROUTE_CSRF_TOKEN: &str = "/api/csrf_token";
pub(crate) const ROUTE_HEALTH: &str = "/api/health";
pub(crate) const ROUTE_OPERATOR_STATUS: &str = "/api/operator/status";
pub(crate) const ROUTE_OPERATOR_WINDOW: &str = "/api/operator/window";
pub(crate) const ROUTE_METRICS: &str = "/api/metrics";
pub(crate) const ROUTE_RESOURCE: &str = "/api/resource";
pub(crate) const ROUTE_DIAGNOSE: &str = "/api/diagnose";
pub(crate) const ROUTE_SYSTEM_INFO: &str = "/api/system_info";
pub(crate) const ROUTE_CHANNEL_CONNECTIVITY: &str = "/api/channel_connectivity";
pub(crate) const ROUTE_TOOLS: &str = "/api/tools";
pub(crate) const ROUTE_SESSIONS: &str = "/api/sessions";
pub(crate) const ROUTE_MEMORY_STATUS: &str = "/api/memory/status";
pub(crate) const ROUTE_MEMORY_MAINTENANCE: &str = "/api/memory/maintenance";
pub(crate) const ROUTE_CAPABILITY_PACKAGES: &str = "/api/capability_packages";
pub(crate) const ROUTE_SKILLS: &str = "/api/skills";
pub(crate) const ROUTE_SKILLS_IMPORT: &str = "/api/skills/import";
pub(crate) const ROUTE_RESTART: &str = "/api/restart";
pub(crate) const ROUTE_CONFIG_RESET: &str = "/api/config_reset";
pub(crate) const ROUTE_WEBHOOK: &str = "/api/webhook";
#[cfg(all(
    feature = "feishu",
    not(any(target_arch = "xtensa", target_arch = "riscv32"))
))]
pub(crate) const ROUTE_FEISHU_EVENT: &str = "/api/feishu/event";
#[cfg(all(
    feature = "dingtalk",
    not(any(target_arch = "xtensa", target_arch = "riscv32"))
))]
pub(crate) const ROUTE_DINGTALK_WEBHOOK: &str = "/api/dingtalk/webhook";
#[cfg(all(
    feature = "wecom",
    not(any(target_arch = "xtensa", target_arch = "riscv32"))
))]
pub(crate) const ROUTE_WECOM_WEBHOOK: &str = "/api/wecom/webhook";
#[cfg(all(
    feature = "qq_channel",
    not(any(target_arch = "xtensa", target_arch = "riscv32"))
))]
pub(crate) const ROUTE_WEBHOOK_QQ: &str = "/api/webhook/qq";
#[cfg(feature = "ota")]
pub(crate) const ROUTE_OTA_CHECK: &str = "/api/ota/check";
#[cfg(feature = "ota")]
pub(crate) const ROUTE_OTA: &str = "/api/ota";

#[cfg(any(test, target_arch = "xtensa", target_arch = "riscv32"))]
pub(crate) const ROOT_ROUTE_SPECS: &[HttpRouteSpec] = &[
    HttpRouteSpec::direct(ROUTE_ROOT, RouteMethod::Get, RouteBodyMode::None),
    HttpRouteSpec::direct(ROUTE_ROOT, RouteMethod::Options, RouteBodyMode::None),
];

#[cfg(any(test, target_arch = "xtensa", target_arch = "riscv32"))]
pub(crate) const PAIRING_AND_CONFIG_ROUTE_SPECS: &[HttpRouteSpec] = &[
    HttpRouteSpec::direct_operator(
        ROUTE_PAIRING_CODE,
        RouteMethod::Get,
        RouteBodyMode::None,
        OperatorRouteAccess::AlwaysOn,
    ),
    HttpRouteSpec::direct_operator(
        ROUTE_PAIRING_CODE,
        RouteMethod::Post,
        RouteBodyMode::Utf8(crate::platform::http_server::common::POST_BODY_MAX_LEN),
        OperatorRouteAccess::AlwaysOn,
    ),
    HttpRouteSpec::direct(
        ROUTE_PAIRING_CODE,
        RouteMethod::Options,
        RouteBodyMode::None,
    ),
    HttpRouteSpec::direct_operator(
        ROUTE_CONFIG_LLM,
        RouteMethod::Get,
        RouteBodyMode::None,
        OperatorRouteAccess::AlwaysOn,
    ),
    HttpRouteSpec::direct(ROUTE_CONFIG_LLM, RouteMethod::Options, RouteBodyMode::None),
    HttpRouteSpec::direct_operator(
        ROUTE_CONFIG_LLM,
        RouteMethod::Post,
        RouteBodyMode::Utf8(crate::platform::http_server::common::POST_BODY_MAX_LEN),
        OperatorRouteAccess::AlwaysOn,
    ),
    HttpRouteSpec::direct_operator(
        ROUTE_CONFIG_CHANNELS,
        RouteMethod::Get,
        RouteBodyMode::None,
        OperatorRouteAccess::AlwaysOn,
    ),
    HttpRouteSpec::direct(
        ROUTE_CONFIG_CHANNELS,
        RouteMethod::Options,
        RouteBodyMode::None,
    ),
    HttpRouteSpec::direct_operator(
        ROUTE_CONFIG_CHANNELS,
        RouteMethod::Post,
        RouteBodyMode::Utf8(crate::platform::http_server::common::POST_BODY_MAX_LEN),
        OperatorRouteAccess::AlwaysOn,
    ),
    HttpRouteSpec::direct_operator(
        ROUTE_CONFIG_SYSTEM,
        RouteMethod::Get,
        RouteBodyMode::None,
        OperatorRouteAccess::AlwaysOn,
    ),
    HttpRouteSpec::direct(
        ROUTE_CONFIG_SYSTEM,
        RouteMethod::Options,
        RouteBodyMode::None,
    ),
    HttpRouteSpec::direct_operator(
        ROUTE_CONFIG_SYSTEM,
        RouteMethod::Post,
        RouteBodyMode::Utf8(crate::platform::http_server::common::POST_BODY_MAX_LEN),
        OperatorRouteAccess::AlwaysOn,
    ),
    HttpRouteSpec::direct_operator(
        ROUTE_CONFIG_HARDWARE,
        RouteMethod::Get,
        RouteBodyMode::None,
        OperatorRouteAccess::AlwaysOn,
    ),
    HttpRouteSpec::direct(
        ROUTE_CONFIG_HARDWARE,
        RouteMethod::Options,
        RouteBodyMode::None,
    ),
    HttpRouteSpec::direct_operator(
        ROUTE_CONFIG_HARDWARE,
        RouteMethod::Post,
        RouteBodyMode::Utf8(crate::platform::http_server::common::POST_BODY_MAX_LEN),
        OperatorRouteAccess::AlwaysOn,
    ),
    HttpRouteSpec::direct_operator(
        ROUTE_CONFIG_AUDIO,
        RouteMethod::Get,
        RouteBodyMode::None,
        OperatorRouteAccess::AlwaysOn,
    ),
    HttpRouteSpec::direct(
        ROUTE_CONFIG_AUDIO,
        RouteMethod::Options,
        RouteBodyMode::None,
    ),
    HttpRouteSpec::direct_operator(
        ROUTE_CONFIG_AUDIO,
        RouteMethod::Post,
        RouteBodyMode::Utf8(crate::platform::http_server::common::POST_BODY_MAX_LEN),
        OperatorRouteAccess::AlwaysOn,
    ),
    HttpRouteSpec::direct_operator(
        ROUTE_CONFIG_DISPLAY,
        RouteMethod::Get,
        RouteBodyMode::None,
        OperatorRouteAccess::AlwaysOn,
    ),
    HttpRouteSpec::direct(
        ROUTE_CONFIG_DISPLAY,
        RouteMethod::Options,
        RouteBodyMode::None,
    ),
    HttpRouteSpec::direct_operator(
        ROUTE_CONFIG_DISPLAY,
        RouteMethod::Post,
        RouteBodyMode::Utf8(crate::platform::http_server::common::POST_BODY_MAX_LEN),
        OperatorRouteAccess::AlwaysOn,
    ),
    HttpRouteSpec::worker_operator(
        ROUTE_WIFI_SCAN,
        RouteMethod::Get,
        RouteBodyMode::None,
        OperatorRouteAccess::AlwaysOn,
    ),
    HttpRouteSpec::worker(ROUTE_WIFI_SCAN, RouteMethod::Options, RouteBodyMode::None),
    HttpRouteSpec::worker_operator(
        ROUTE_HARDWARE_DISCOVERY,
        RouteMethod::Get,
        RouteBodyMode::None,
        OperatorRouteAccess::AlwaysOn,
    ),
    HttpRouteSpec::worker(
        ROUTE_HARDWARE_DISCOVERY,
        RouteMethod::Options,
        RouteBodyMode::None,
    ),
    HttpRouteSpec::direct_operator(
        ROUTE_CSRF_TOKEN,
        RouteMethod::Get,
        RouteBodyMode::None,
        OperatorRouteAccess::AlwaysOn,
    ),
    HttpRouteSpec::direct(ROUTE_CSRF_TOKEN, RouteMethod::Options, RouteBodyMode::None),
];

#[cfg(any(test, target_arch = "xtensa", target_arch = "riscv32"))]
pub(crate) const OBSERVABILITY_ROUTE_SPECS: &[HttpRouteSpec] = &[
    HttpRouteSpec::direct_operator(
        ROUTE_HEALTH,
        RouteMethod::Get,
        RouteBodyMode::None,
        OperatorRouteAccess::AlwaysOn,
    ),
    HttpRouteSpec::direct(ROUTE_HEALTH, RouteMethod::Options, RouteBodyMode::None),
    // Keep the HTTPD callback thread on lightweight summaries only.
    // Routes that inspect runtime/storage/memory state run on http_route_exec.
    HttpRouteSpec::worker_operator(
        ROUTE_OPERATOR_STATUS,
        RouteMethod::Get,
        RouteBodyMode::None,
        OperatorRouteAccess::AlwaysOn,
    ),
    HttpRouteSpec::direct(
        ROUTE_OPERATOR_STATUS,
        RouteMethod::Options,
        RouteBodyMode::None,
    ),
    HttpRouteSpec::direct_operator(
        ROUTE_OPERATOR_WINDOW,
        RouteMethod::Post,
        RouteBodyMode::None,
        OperatorRouteAccess::AlwaysOn,
    ),
    HttpRouteSpec::direct(
        ROUTE_OPERATOR_WINDOW,
        RouteMethod::Options,
        RouteBodyMode::None,
    ),
    HttpRouteSpec::direct_operator(
        ROUTE_METRICS,
        RouteMethod::Get,
        RouteBodyMode::None,
        OperatorRouteAccess::Windowed,
    ),
    HttpRouteSpec::direct(ROUTE_METRICS, RouteMethod::Options, RouteBodyMode::None),
    HttpRouteSpec::worker_operator(
        ROUTE_RESOURCE,
        RouteMethod::Get,
        RouteBodyMode::None,
        OperatorRouteAccess::Windowed,
    ),
    HttpRouteSpec::direct(ROUTE_RESOURCE, RouteMethod::Options, RouteBodyMode::None),
    HttpRouteSpec::worker_operator(
        ROUTE_DIAGNOSE,
        RouteMethod::Get,
        RouteBodyMode::None,
        OperatorRouteAccess::AlwaysOn,
    ),
    HttpRouteSpec::direct(ROUTE_DIAGNOSE, RouteMethod::Options, RouteBodyMode::None),
    HttpRouteSpec::worker_operator(
        ROUTE_SYSTEM_INFO,
        RouteMethod::Get,
        RouteBodyMode::None,
        OperatorRouteAccess::AlwaysOn,
    ),
    HttpRouteSpec::direct(ROUTE_SYSTEM_INFO, RouteMethod::Options, RouteBodyMode::None),
    HttpRouteSpec::worker_operator(
        ROUTE_CHANNEL_CONNECTIVITY,
        RouteMethod::Get,
        RouteBodyMode::None,
        OperatorRouteAccess::Windowed,
    ),
    HttpRouteSpec::worker(
        ROUTE_CHANNEL_CONNECTIVITY,
        RouteMethod::Options,
        RouteBodyMode::None,
    ),
];

#[cfg(any(test, target_arch = "xtensa", target_arch = "riscv32"))]
pub(crate) const MEMORY_AND_SKILL_ROUTE_SPECS: &[HttpRouteSpec] = &[
    HttpRouteSpec::worker_operator(
        ROUTE_TOOLS,
        RouteMethod::Get,
        RouteBodyMode::None,
        OperatorRouteAccess::Windowed,
    ),
    HttpRouteSpec::worker(ROUTE_TOOLS, RouteMethod::Options, RouteBodyMode::None),
    HttpRouteSpec::worker_operator(
        ROUTE_SESSIONS,
        RouteMethod::Get,
        RouteBodyMode::None,
        OperatorRouteAccess::Windowed,
    ),
    HttpRouteSpec::worker_operator(
        ROUTE_SESSIONS,
        RouteMethod::Delete,
        RouteBodyMode::None,
        OperatorRouteAccess::Windowed,
    ),
    HttpRouteSpec::worker(ROUTE_SESSIONS, RouteMethod::Options, RouteBodyMode::None),
    HttpRouteSpec::worker_operator(
        ROUTE_MEMORY_STATUS,
        RouteMethod::Get,
        RouteBodyMode::None,
        OperatorRouteAccess::Windowed,
    ),
    HttpRouteSpec::worker(
        ROUTE_MEMORY_STATUS,
        RouteMethod::Options,
        RouteBodyMode::None,
    ),
    HttpRouteSpec::worker_operator(
        ROUTE_CAPABILITY_PACKAGES,
        RouteMethod::Get,
        RouteBodyMode::None,
        OperatorRouteAccess::Windowed,
    ),
    HttpRouteSpec::worker_operator(
        ROUTE_CAPABILITY_PACKAGES,
        RouteMethod::Post,
        RouteBodyMode::Utf8(crate::capability_package::MAX_CAPABILITY_PACKAGE_HTTP_BODY_LEN),
        OperatorRouteAccess::Windowed,
    ),
    HttpRouteSpec::worker(
        ROUTE_CAPABILITY_PACKAGES,
        RouteMethod::Options,
        RouteBodyMode::None,
    ),
    HttpRouteSpec::worker_operator(
        ROUTE_SKILLS,
        RouteMethod::Get,
        RouteBodyMode::None,
        OperatorRouteAccess::Windowed,
    ),
    HttpRouteSpec::worker_operator(
        ROUTE_SKILLS,
        RouteMethod::Post,
        RouteBodyMode::Utf8(crate::platform::http_server::common::POST_BODY_MAX_LEN),
        OperatorRouteAccess::Windowed,
    ),
    HttpRouteSpec::worker_operator(
        ROUTE_SKILLS,
        RouteMethod::Delete,
        RouteBodyMode::None,
        OperatorRouteAccess::Windowed,
    ),
    HttpRouteSpec::worker(ROUTE_SKILLS, RouteMethod::Options, RouteBodyMode::None),
    HttpRouteSpec::worker_operator(
        ROUTE_SKILLS_IMPORT,
        RouteMethod::Post,
        RouteBodyMode::Utf8(crate::platform::http_server::common::POST_BODY_MAX_LEN),
        OperatorRouteAccess::Windowed,
    ),
    HttpRouteSpec::worker(
        ROUTE_SKILLS_IMPORT,
        RouteMethod::Options,
        RouteBodyMode::None,
    ),
];

#[cfg(any(test, target_arch = "xtensa", target_arch = "riscv32"))]
pub(crate) const ACTION_ROUTE_SPECS: &[HttpRouteSpec] = &[
    HttpRouteSpec::direct_operator(
        ROUTE_RESTART,
        RouteMethod::Post,
        RouteBodyMode::None,
        OperatorRouteAccess::AlwaysOn,
    ),
    HttpRouteSpec::direct(ROUTE_RESTART, RouteMethod::Options, RouteBodyMode::None),
    HttpRouteSpec::direct_operator(
        ROUTE_CONFIG_RESET,
        RouteMethod::Post,
        RouteBodyMode::None,
        OperatorRouteAccess::AlwaysOn,
    ),
    HttpRouteSpec::direct(
        ROUTE_CONFIG_RESET,
        RouteMethod::Options,
        RouteBodyMode::None,
    ),
    HttpRouteSpec::worker(
        ROUTE_WEBHOOK,
        RouteMethod::Post,
        RouteBodyMode::Utf8(crate::platform::http_server::common::POST_BODY_MAX_LEN),
    ),
    HttpRouteSpec::worker(ROUTE_WEBHOOK, RouteMethod::Options, RouteBodyMode::None),
];

#[cfg(all(
    feature = "ota",
    any(test, target_arch = "xtensa", target_arch = "riscv32")
))]
pub(crate) const OTA_ROUTE_SPECS: &[HttpRouteSpec] = &[
    HttpRouteSpec::worker_operator(
        ROUTE_OTA_CHECK,
        RouteMethod::Get,
        RouteBodyMode::None,
        OperatorRouteAccess::AlwaysOn,
    ),
    HttpRouteSpec::worker(ROUTE_OTA_CHECK, RouteMethod::Options, RouteBodyMode::None),
    HttpRouteSpec::worker_operator(
        ROUTE_OTA,
        RouteMethod::Post,
        RouteBodyMode::Utf8(crate::platform::http_server::common::POST_BODY_MAX_LEN),
        OperatorRouteAccess::AlwaysOn,
    ),
    HttpRouteSpec::worker(ROUTE_OTA, RouteMethod::Options, RouteBodyMode::None),
];
