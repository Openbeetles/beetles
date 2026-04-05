//! Linux CLI command tree implemented with clap.

use clap::{Parser, Subcommand};

#[derive(Parser)]
#[command(name = "beetle")]
#[command(version, about = "Beetle AI Agent", long_about = None)]
pub struct Cli {
    #[command(subcommand)]
    pub command: Commands,
}

#[derive(Subcommand)]
pub enum Commands {
    /// Start the agent service in the foreground
    Run {
        /// Optional config file path
        #[arg(short, long)]
        config: Option<String>,
    },

    /// Manage configuration values
    Config {
        #[command(subcommand)]
        action: ConfigAction,
    },

    /// Show system status
    Status {
        /// Output JSON instead of plain text
        #[arg(long)]
        json: bool,
        /// Include the latest turn ledger for the specified chat_id
        #[arg(long)]
        chat_id: Option<String>,
    },

    /// Restart the Beetle service or process
    Restart,

    /// Run diagnostic checks
    Doctor,

    /// Print version information
    Version,
}

#[derive(Subcommand)]
pub enum ConfigAction {
    /// Get a configuration value
    Get {
        /// Config key, for example `llm.provider`
        key: String,
    },

    /// Set a configuration value
    Set {
        /// Config key
        key: String,
        /// Config value
        value: String,
    },

    /// List all configuration values
    List,
}
