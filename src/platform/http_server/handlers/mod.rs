//! 按接口域拆分的 handler 逻辑；mod.rs 只做路由注册与配对检查，具体响应体由各子模块生成。

use crate::config::{AppConfig, ConfigFileStore};
use crate::platform::fetch_url::fetch_url_with_client;
use crate::platform::{ConfigStore, Platform, SkillMetaStore, SkillStorage};
use crate::CapabilityPackageRuntimeCapabilities;
use std::sync::atomic::AtomicUsize;
use std::sync::{Arc, RwLock};

/// 各 handler 共享的上下文，由 run() 构建后以 Arc 传入闭包。
/// `cached_config` 缓存最新配置：读路径零 `AppConfig::load()`，写路径保存后 `reload_config()` 刷新。
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
    pub version: Arc<str>,
    pub board_id: Arc<str>,
    pub cached_config: Arc<RwLock<AppConfig>>,
    pub llm_stream_enabled: bool,
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

    pub fn fetch_url(&self, url: &str, max_len: usize) -> crate::error::Result<Vec<u8>> {
        let cfg = self.config();
        let mut client = self.platform.create_http_client(&cfg)?;
        drop(cfg);
        fetch_url_with_client(client.as_mut(), url, max_len)
    }
}

pub mod capability_packages;
pub mod channel_connectivity;
pub mod config;
pub mod config_page;
pub mod config_reset;
pub mod csrf_token;
pub mod diagnose;
pub mod hardware_discovery;
pub mod health;
pub mod memory;
pub mod metrics;
pub mod operator_status;
pub mod operator_window;
pub mod pairing;
pub mod resource;
pub mod restart;
pub mod root;
pub mod sessions;
pub mod skills;
pub mod soul;
pub mod system_info;
pub mod tools;
pub mod user;
pub mod webhook;
pub mod wifi_scan;

#[cfg(not(any(target_arch = "xtensa", target_arch = "riscv32")))]
pub mod dingtalk_webhook;
#[cfg(not(any(target_arch = "xtensa", target_arch = "riscv32")))]
pub mod feishu_event;
#[cfg(not(any(target_arch = "xtensa", target_arch = "riscv32")))]
pub mod qq_webhook;
#[cfg(not(any(target_arch = "xtensa", target_arch = "riscv32")))]
pub mod wecom_webhook;

#[cfg(feature = "ota")]
pub mod ota;
