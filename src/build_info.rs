//! 编译期构建元数据（`option_env!`）；板型键由 `platform::runtime_board::resolved_board_id()` 运行期拼装。
//! Build-time metadata; board id is composed at runtime (see `platform::runtime_board`).

/// Git commit captured by `build.sh`; `unknown` means the build did not run through the release script.
#[inline(always)]
pub fn build_git_sha() -> &'static str {
    option_env!("BEETLE_BUILD_GIT_SHA").unwrap_or("unknown")
}

/// Whether the working tree was dirty when `build.sh` invoked cargo.
#[inline(always)]
pub fn build_git_dirty() -> &'static str {
    option_env!("BEETLE_BUILD_GIT_DIRTY").unwrap_or("unknown")
}

/// UTC build timestamp captured by `build.sh`.
#[inline(always)]
pub fn build_time_utc() -> &'static str {
    option_env!("BEETLE_BUILD_TIME_UTC").unwrap_or("unknown")
}

/// SHA-256 of the source partition CSV used for the ESP build.
#[inline(always)]
pub fn partition_csv_sha256() -> &'static str {
    option_env!("BEETLE_PARTITION_CSV_SHA256").unwrap_or("unknown")
}
