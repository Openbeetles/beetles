//! get_time 工具：返回当前 UTC 时间字符串。Host 用 SystemTime；ESP 用系统时间（需 SNTP 或 RTC 同步后有效）。
//! get_time tool: return current UTC time string. Host uses SystemTime; ESP uses system time (valid after SNTP/RTC sync).

use crate::error::Result;
use crate::tools::{Tool, ToolContext};
use crate::util::{current_unix_secs, epoch_to_ymdhms, weekday_name};

pub struct GetTimeTool;

/// 将 Unix 秒转为 "YYYY-MM-DD Weekday HH:MM:SS UTC" 格式。
fn unix_secs_to_utc_string(secs: u64) -> String {
    let (year, month, day, hour, min, sec) = epoch_to_ymdhms(secs);
    let days = secs / 86400;
    let weekday = weekday_name(days);
    format!(
        "{:04}-{:02}-{:02} {} {:02}:{:02}:{:02} UTC",
        year, month, day, weekday, hour, min, sec
    )
}

impl Tool for GetTimeTool {
    fn name(&self) -> &'static str {
        "get_time"
    }
    fn description(&self) -> &'static str {
        "Get current UTC time in YYYY-MM-DD Weekday HH:MM:SS UTC format. On device, ensure SNTP or RTC is synced first."
    }
    fn schema(&self) -> &str {
        r#"{"type":"object","properties":{}}"#
    }
    fn execute(&self, _args: &str, _ctx: &mut dyn ToolContext) -> Result<String> {
        let secs = current_unix_secs();
        Ok(unix_secs_to_utc_string(secs))
    }
}
