//! board_info 工具：委托 `Platform::board_info_json`，载荷由 `platform/board_info` 按目标（ESP32 / Linux / 其它 OS 名）组装。
//! board_info tool: delegates to `Platform::board_info_json`; payload per target (ESP32 / Linux / other OS per `std::env::consts::OS`).

use crate::Platform;
use crate::error::Result;
use crate::tools::{Tool, ToolContext};
use std::sync::Arc;

pub struct BoardInfoTool {
    platform: Arc<dyn Platform>,
}

impl BoardInfoTool {
    pub fn new(platform: Arc<dyn Platform>) -> Self {
        Self { platform }
    }
}

impl Tool for BoardInfoTool {
    fn name(&self) -> &'static str {
        "board_info"
    }
    fn description(&self) -> &'static str {
        "Return a whole-device or whole-host status snapshot as JSON. ESP: chip, heap, IDF, SPIFFS. Linux: platform \"linux\" plus cpu_model, cpu_cores, mem_*, distro_pretty/distro_id, kernel_release, hostname, arch, storage, os (/proc/version), uptime, pressure, and WiFi STA state. Use this for overall system status, resource pressure, distro, CPU/RAM, and storage. For one specific process or detailed Linux network diagnostics, prefer the dedicated process or network tools."
    }
    fn schema(&self) -> &str {
        r#"{"type":"object","properties":{}}"#
    }
    fn execute(&self, _args: &str, _ctx: &mut dyn ToolContext) -> Result<String> {
        self.platform.board_info_json()
    }
}
