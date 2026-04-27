//! 按接口域拆分的 handler 逻辑；mod.rs 只做路由注册与配对检查，具体响应体由各子模块生成。

use crate::config::{AppConfig, ConfigFileStore};
use crate::platform::fetch_url::fetch_url_with_client;
use crate::platform::{ConfigStore, Platform, SkillMetaStore, SkillStorage};
use crate::CapabilityPackageRuntimeCapabilities;
use std::sync::atomic::AtomicUsize;
use std::sync::{Arc, RwLock};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ControlPlaneRouteContract {
    pub inbound_webhooks_enabled: bool,
}

impl ControlPlaneRouteContract {
    pub const FULL: Self = Self {
        inbound_webhooks_enabled: true,
    };
}

/// 各 handler 共享的上下文，由 run() 构建后以 Arc 传入闭包。
/// `cached_config` 缓存最新配置：读路径零 `AppConfig::load()`；SPIFFS 写路径仍可 `reload_config()`，
/// NVS-only 小改动则原地投影到缓存，避免整包重载。
#[allow(dead_code)]
pub struct HandlerContext {
    pub config_store: Arc<dyn ConfigStore + Send + Sync>,
    pub config_file_store: Arc<dyn ConfigFileStore + Send + Sync>,
    pub platform: Arc<dyn Platform>,
    pub memory_store: Arc<dyn crate::memory::MemoryStore + Send + Sync>,
    pub session_store: Arc<dyn crate::memory::SessionStore + Send + Sync>,
    pub skill_storage: Arc<dyn SkillStorage + Send + Sync>,
    pub skill_meta_store: Arc<dyn SkillMetaStore + Send + Sync>,
    pub skill_prompt_cache: Arc<crate::skills::SkillPromptCache>,
    pub tool_registry: Arc<crate::tools::ToolRegistry>,
    pub channel_capability_registry: Arc<crate::ChannelCapabilityRegistry>,
    pub capability_package_runtime_capabilities: Arc<CapabilityPackageRuntimeCapabilities>,
    pub inbound_depth: Arc<AtomicUsize>,
    pub outbound_depth: Arc<AtomicUsize>,
    pub system_inbound_tx: Option<crate::bus::SystemInboundTx>,
    pub version: Arc<str>,
    pub board_id: Arc<str>,
    pub cached_config: Arc<RwLock<AppConfig>>,
    pub route_contract: ControlPlaneRouteContract,
    #[cfg(all(
        test,
        feature = "capability_office",
        not(any(target_arch = "xtensa", target_arch = "riscv32"))
    ))]
    pub office_probe_adapters:
        Option<Vec<Arc<dyn crate::office::OfficeProbeAdapter + Send + Sync>>>,
}

impl HandlerContext {
    /// 借用缓存配置的读锁。
    pub fn config(&self) -> std::sync::RwLockReadGuard<'_, AppConfig> {
        self.cached_config.read().unwrap_or_else(|e| e.into_inner())
    }

    /// 保存操作后调用，从 stores 重新加载配置到缓存。
    pub fn reload_config(&self) {
        let new = AppConfig::load(
            self.config_store.as_ref(),
            Some(self.config_file_store.as_ref()),
        );
        *self
            .cached_config
            .write()
            .unwrap_or_else(|e| e.into_inner()) = new;
    }

    pub fn update_cached_config(&self, apply: impl FnOnce(&mut AppConfig)) {
        apply(
            &mut self
                .cached_config
                .write()
                .unwrap_or_else(|error| error.into_inner()),
        );
    }

    pub fn fetch_url(&self, url: &str, max_len: usize) -> crate::error::Result<Vec<u8>> {
        let cfg = self.config();
        let mut client = crate::network::create_http_client_with_config(
            self.platform.as_ref(),
            &cfg,
            crate::network::HttpClientClass::Background,
        )?;
        drop(cfg);
        fetch_url_with_client(client.as_mut(), url, max_len)
    }
}

#[allow(clippy::too_many_arguments)]
pub fn build_runtime_handler_context(
    platform: Arc<dyn Platform>,
    tool_registry: Arc<crate::tools::ToolRegistry>,
    channel_capability_registry: Arc<crate::ChannelCapabilityRegistry>,
    capability_package_runtime_capabilities: Arc<CapabilityPackageRuntimeCapabilities>,
    inbound_depth: Arc<AtomicUsize>,
    outbound_depth: Arc<AtomicUsize>,
    memory_store: Arc<dyn crate::memory::MemoryStore + Send + Sync>,
    session_store: Arc<dyn crate::memory::SessionStore + Send + Sync>,
    system_inbound_tx: Option<crate::bus::SystemInboundTx>,
    skill_prompt_cache: Arc<crate::skills::SkillPromptCache>,
    cached_config: Arc<RwLock<AppConfig>>,
    route_contract: ControlPlaneRouteContract,
) -> HandlerContext {
    let config_store = platform.config_store();
    let config_file_store = Arc::new(crate::config::PlatformConfigFileStore(Arc::clone(
        &platform,
    )));
    let skill_storage = platform.skill_storage();
    let skill_meta_store = platform.skill_meta_store();

    HandlerContext {
        config_store,
        config_file_store,
        platform,
        memory_store,
        session_store,
        skill_storage,
        skill_meta_store,
        skill_prompt_cache,
        tool_registry,
        channel_capability_registry,
        capability_package_runtime_capabilities,
        inbound_depth,
        outbound_depth,
        system_inbound_tx,
        version: Arc::from(env!("CARGO_PKG_VERSION")),
        board_id: Arc::from(crate::platform::runtime_board::resolved_board_id()),
        cached_config,
        route_contract,
        #[cfg(all(
            test,
            feature = "capability_office",
            not(any(target_arch = "xtensa", target_arch = "riscv32"))
        ))]
        office_probe_adapters: None,
    }
}

#[cfg(test)]
pub(crate) fn build_default_test_handler_context() -> HandlerContext {
    let config = AppConfig::load_from_env();
    let platform: Arc<dyn Platform> = Arc::new(crate::platform::LinuxPlatform::new());
    let config_store = platform.config_store();
    let skill_storage = platform.skill_storage();
    build_test_handler_context(
        config,
        platform,
        config_store,
        skill_storage,
        ControlPlaneRouteContract::FULL,
        "test-board",
    )
}

#[cfg(test)]
#[allow(clippy::too_many_arguments)]
pub(crate) fn build_test_handler_context(
    config: AppConfig,
    platform: Arc<dyn Platform>,
    config_store: Arc<dyn ConfigStore + Send + Sync>,
    skill_storage: Arc<dyn SkillStorage + Send + Sync>,
    route_contract: ControlPlaneRouteContract,
    board_id: &'static str,
) -> HandlerContext {
    let skill_meta_store = platform.skill_meta_store();
    let skill_prompt_cache = Arc::new(crate::skills::SkillPromptCache::new(
        Arc::clone(&skill_meta_store),
        Arc::clone(&skill_storage),
        8192,
    ));
    let runtime_services = crate::RuntimeServices::from_platform(Arc::clone(&platform));
    let (tool_registry, _) = crate::build_default_registry(&config, &runtime_services);
    let channel_capability_registry =
        Arc::new(crate::build_channel_capability_registry(&config, false));
    let capability_package_runtime_capabilities = Arc::new(
        crate::build_capability_package_runtime_capabilities(channel_capability_registry.as_ref()),
    );

    HandlerContext {
        config_store,
        config_file_store: Arc::new(crate::config::PlatformConfigFileStore(Arc::clone(
            &platform,
        ))),
        platform: Arc::clone(&platform),
        memory_store: platform.memory_store(),
        session_store: platform.session_store(),
        skill_storage,
        skill_meta_store,
        skill_prompt_cache,
        tool_registry: Arc::new(tool_registry),
        channel_capability_registry: Arc::clone(&channel_capability_registry),
        capability_package_runtime_capabilities,
        inbound_depth: Arc::new(AtomicUsize::new(0)),
        outbound_depth: Arc::new(AtomicUsize::new(0)),
        system_inbound_tx: None,
        version: Arc::from("0.0.0"),
        board_id: Arc::from(board_id),
        cached_config: Arc::new(RwLock::new(config)),
        route_contract,
        #[cfg(all(
            test,
            feature = "capability_office",
            not(any(target_arch = "xtensa", target_arch = "riscv32"))
        ))]
        office_probe_adapters: None,
    }
}

#[cfg(test)]
pub(crate) fn default_test_handler_context_guard() -> std::sync::MutexGuard<'static, ()> {
    use std::sync::{Mutex, OnceLock};

    static LOCK: OnceLock<Mutex<()>> = OnceLock::new();
    LOCK.get_or_init(|| Mutex::new(()))
        .lock()
        .unwrap_or_else(|error| error.into_inner())
}

pub mod capability_packages;
pub mod channel_connectivity;
pub mod config;
pub mod config_reset;
pub mod csrf_token;
pub mod diagnose;
pub mod hardware_discovery;
pub mod health;
pub mod memory;
pub mod memory_maintenance;
pub mod metrics;
pub mod operator_status;
pub mod operator_window;
pub mod pairing;
pub mod resource;
pub mod restart;
pub mod root;
pub mod sessions;
pub mod skills;
pub mod system_info;
pub mod tools;
pub mod webhook;
pub mod wifi_scan;
