//! 按接口域拆分的 handler 逻辑；mod.rs 只做路由注册与配对检查，具体响应体由各子模块生成。

use crate::config::{AppConfig, ConfigFileStore};
use crate::platform::fetch_url::fetch_url_with_client;
use crate::platform::{ConfigStore, Platform, SkillMetaStore, SkillStorage};
use std::sync::atomic::AtomicUsize;
use std::sync::Arc;

/// 各 handler 共享的只读上下文，由 run() 构建后以 Arc 传入闭包。配置以 config_store 为唯一源，按需 load。
#[allow(dead_code)]
pub struct HandlerContext {
    pub config_store: Arc<dyn ConfigStore + Send + Sync>,
    pub config_file_store: Arc<dyn ConfigFileStore + Send + Sync>,
    pub platform: Arc<dyn Platform>,
    pub memory_store: Arc<dyn crate::memory::MemoryStore + Send + Sync>,
    pub session_store: Arc<dyn crate::memory::SessionStore + Send + Sync>,
    pub skill_storage: Arc<dyn SkillStorage + Send + Sync>,
    pub skill_meta_store: Arc<dyn SkillMetaStore + Send + Sync>,
    pub inbound_depth: Arc<AtomicUsize>,
    pub outbound_depth: Arc<AtomicUsize>,
    pub version: Arc<str>,
    /// 板型 ID，与 board_presets.toml 一致，用于 OTA 渠道查 manifest。
    pub board_id: Arc<str>,
}

impl HandlerContext {
    /// GET url，返回 body 截断至 max_len。经 `config_store` 加载代理等配置后建 HTTP 客户端。
    pub fn fetch_url(&self, url: &str, max_len: usize) -> crate::error::Result<Vec<u8>> {
        let config = AppConfig::load(self.config_store.as_ref(), None);
        let mut client = self.platform.create_http_client(&config)?;
        fetch_url_with_client(client.as_mut(), url, max_len)
    }
}

pub mod channel_connectivity;
pub mod config;
pub mod config_page;
pub mod config_reset;
pub mod csrf_token;
pub mod diagnose;
pub mod dingtalk_webhook;
pub mod feishu_event;
pub mod health;
pub mod memory;
pub mod metrics;
pub mod pairing;
pub mod qq_webhook;
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
pub mod wecom_webhook;
pub mod wifi_scan;

#[cfg(feature = "ota")]
pub mod ota;
