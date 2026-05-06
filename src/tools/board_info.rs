//! board_info 工具：委托 `Platform::board_info_json`，载荷由 `platform/board_info` 按目标（ESP32 / Linux / 其它 OS 名）组装。
//! board_info tool: delegates to `Platform::board_info_json`; payload per target (ESP32 / Linux / other OS per `std::env::consts::OS`).

use crate::error::Result;
use crate::tools::{Tool, ToolContext};
use crate::Platform;
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
        "Return a whole-device or whole-host status snapshot as JSON. Includes firmware_version, display.available, audio.duplex_capabilities, runtime_capabilities, storage_media, uptime, pressure, and WiFi STA state. ESP also includes chip, internal heap free (`heap_free` / `heap_free_internal`), whole-memory free including PSRAM (`heap_free_total`), PSRAM free, largest internal free block, and TLS fragmentation risk. Linux also includes platform \"linux\", cpu_model, cpu_cores, mem_*, distro_pretty/distro_id, kernel_release, hostname, arch, storage, os (/proc/version), resource pressure, network interfaces, DNS, default route, and storage topology. Use this for version, display/audio availability, overall system status, resource pressure, distro, CPU/RAM, and storage topology. For deeper runtime diagnosis, use `diagnose` with `op=system` or `op=network`."
    }
    fn schema(&self) -> &str {
        r#"{"type":"object","properties":{}}"#
    }
    fn execute(&self, _args: &str, _ctx: &mut dyn ToolContext) -> Result<String> {
        self.platform.board_info_json()
    }
}
