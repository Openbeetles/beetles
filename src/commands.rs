//! CLI 子命令系统（仅 Linux）
//! 使用 clap 实现完整的子命令结构

use clap::{Parser, Subcommand};

#[derive(Parser)]
#[command(name = "beetle")]
#[command(version, about = "甲壳虫 AI Agent", long_about = None)]
pub struct Cli {
    #[command(subcommand)]
    pub command: Commands,
}

#[derive(Subcommand)]
pub enum Commands {
    /// 启动 Agent 服务
    Run {
        /// 配置文件路径
        #[arg(short, long)]
        config: Option<String>,
    },

    /// 配置管理
    Config {
        #[command(subcommand)]
        action: ConfigAction,
    },

    /// 系统状态
    Status {
        /// 输出 JSON 格式
        #[arg(long)]
        json: bool,
        /// 附带输出指定 chat_id 的最近一轮执行账本
        #[arg(long)]
        chat_id: Option<String>,
    },

    /// 诊断工具
    Doctor,

    /// 版本信息
    Version,
}

#[derive(Subcommand)]
pub enum ConfigAction {
    /// 获取配置值
    Get {
        /// 配置键（如 llm.provider）
        key: String,
    },

    /// 设置配置值
    Set {
        /// 配置键
        key: String,
        /// 配置值
        value: String,
    },

    /// 列出所有配置
    List,
}
