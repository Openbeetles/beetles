//! Shared HTTP route catalog used by transport registration and dispatch.

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum RouteMethod {
    Get,
    Post,
    Delete,
    Options,
}

impl RouteMethod {
    #[cfg(any(test, target_arch = "xtensa", target_arch = "riscv32"))]
    pub(crate) const fn as_str(self) -> &'static str {
        match self {
            Self::Get => "GET",
            Self::Post => "POST",
            Self::Delete => "DELETE",
            Self::Options => "OPTIONS",
        }
    }

    pub(crate) fn parse(method: &str) -> Option<Self> {
        match method {
            "GET" => Some(Self::Get),
            "POST" => Some(Self::Post),
            "DELETE" => Some(Self::Delete),
            "OPTIONS" => Some(Self::Options),
            _ => None,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum RouteBodyMode {
    None,
    Utf8(usize),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum RouteDispatchMode {
    Direct,
    Worker,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum OperatorRouteAccess {
    Hidden,
    AlwaysOn,
    Windowed,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct HttpRouteSpec {
    pub(crate) path: &'static str,
    pub(crate) method: RouteMethod,
    pub(crate) body_mode: RouteBodyMode,
    pub(crate) dispatch_mode: RouteDispatchMode,
    pub(crate) operator_access: OperatorRouteAccess,
}

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

fn route_spec_groups() -> &'static [&'static [HttpRouteSpec]] {
    &[
        ROOT_ROUTE_SPECS,
        PAIRING_AND_CONFIG_ROUTE_SPECS,
        OBSERVABILITY_ROUTE_SPECS,
        MEMORY_AND_SKILL_ROUTE_SPECS,
        ACTION_ROUTE_SPECS,
        #[cfg(feature = "ota")]
        OTA_ROUTE_SPECS,
    ]
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
#[cfg(feature = "ota")]
pub(crate) const ROUTE_OTA_CHECK: &str = "/api/ota/check";
#[cfg(feature = "ota")]
pub(crate) const ROUTE_OTA: &str = "/api/ota";

pub(crate) const ROOT_ROUTE_SPECS: &[HttpRouteSpec] = &[
    HttpRouteSpec::direct(ROUTE_ROOT, RouteMethod::Get, RouteBodyMode::None),
    HttpRouteSpec::direct(ROUTE_ROOT, RouteMethod::Options, RouteBodyMode::None),
];

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
        ROUTE_MEMORY_MAINTENANCE,
        RouteMethod::Post,
        RouteBodyMode::Utf8(crate::platform::http_server::common::POST_BODY_MAX_LEN),
        OperatorRouteAccess::Windowed,
    ),
    HttpRouteSpec::worker(
        ROUTE_MEMORY_MAINTENANCE,
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

#[cfg(feature = "ota")]
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

pub(crate) fn route_spec_for(method: &str, path: &str) -> Option<HttpRouteSpec> {
    let method = RouteMethod::parse(method)?;
    route_spec_for_method(method, path)
}

pub(crate) fn route_spec_for_method(method: RouteMethod, path: &str) -> Option<HttpRouteSpec> {
    for group in route_spec_groups() {
        if let Some(spec) = group
            .iter()
            .copied()
            .find(|spec| spec.method == method && spec.path == path)
        {
            return Some(spec);
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{compiled_enabled_channel_ids, selectable_channel_entries, CHANNEL_QQ_CHANNEL};

    #[test]
    fn compiled_enabled_channel_ids_follow_selectable_catalog_order() {
        let ids = compiled_enabled_channel_ids();
        assert_eq!(ids.first().copied(), Some(""));
        let selectable = selectable_channel_entries()
            .map(|entry| entry.id)
            .collect::<Vec<_>>();
        assert_eq!(&ids[1..], selectable.as_slice());
        #[cfg(feature = "dingtalk")]
        assert!(ids.contains(&crate::CHANNEL_DINGTALK));
        #[cfg(not(feature = "dingtalk"))]
        assert!(!ids.contains(&crate::channel_capability::CHANNEL_DINGTALK));
        #[cfg(feature = "qq_channel")]
        assert!(ids.contains(&CHANNEL_QQ_CHANNEL));
        #[cfg(not(feature = "qq_channel"))]
        assert!(!ids.contains(&crate::channel_capability::CHANNEL_QQ_CHANNEL));
    }

    #[test]
    fn route_lookup_returns_spec_metadata_from_catalog() {
        let spec = route_spec_for("POST", ROUTE_CONFIG_CHANNELS).expect("route spec");
        assert_eq!(spec.method.as_str(), "POST");
        assert_eq!(spec.path, ROUTE_CONFIG_CHANNELS);
        assert!(matches!(spec.body_mode, RouteBodyMode::Utf8(_)));
        assert_eq!(spec.dispatch_mode, RouteDispatchMode::Direct);
        assert_eq!(spec.operator_access, OperatorRouteAccess::AlwaysOn);
    }

    #[test]
    fn route_lookup_marks_windowed_worker_routes() {
        let spec = route_spec_for("GET", ROUTE_RESOURCE).expect("route spec");
        assert_eq!(spec.dispatch_mode, RouteDispatchMode::Worker);
        assert_eq!(spec.operator_access, OperatorRouteAccess::Windowed);
    }

    #[test]
    fn route_lookup_keeps_memory_maintenance_body_contract() {
        let spec = route_spec_for("POST", ROUTE_MEMORY_MAINTENANCE).expect("route spec");
        assert_eq!(spec.dispatch_mode, RouteDispatchMode::Worker);
        assert_eq!(spec.operator_access, OperatorRouteAccess::Windowed);
        assert!(matches!(spec.body_mode, RouteBodyMode::Utf8(_)));
    }
}
