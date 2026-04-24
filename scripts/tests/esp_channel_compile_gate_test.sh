#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
ROOT_DIR="$(cd "$SCRIPT_DIR/../.." && pwd)"
CARGO_TOML="$ROOT_DIR/Cargo.toml"
BUILD_SH="$ROOT_DIR/build.sh"
FEATURE_EXPANDER="$ROOT_DIR/scripts/expand_cargo_features.py"
CHANNELS_MOD_RS="$ROOT_DIR/src/channels/mod.rs"
CHANNEL_CATALOG_RS="$ROOT_DIR/src/channel_catalog.rs"
CHANNELS_DISPATCH_RS="$ROOT_DIR/src/channels/dispatch.rs"
CHANNELS_CONNECTIVITY_RS="$ROOT_DIR/src/channels/connectivity.rs"
LIB_RS="$ROOT_DIR/src/lib.rs"
MAIN_RS="$ROOT_DIR/src/main.rs"

assert_contains() {
  local file="$1"
  local pattern="$2"
  local message="$3"
  if ! rg -n "$pattern" "$file" >/dev/null 2>&1; then
    echo "FAIL: $message" >&2
    echo "  file: $file" >&2
    echo "  missing pattern: $pattern" >&2
    exit 1
  fi
}

assert_absent() {
  local file="$1"
  local pattern="$2"
  local message="$3"
  if rg -n "$pattern" "$file" >/dev/null 2>&1; then
    echo "FAIL: $message" >&2
    echo "  file: $file" >&2
    rg -n "$pattern" "$file" >&2 || true
    exit 1
  fi
}

assert_line_guarded() {
  local file="$1"
  local literal="$2"
  local guard="$3"
  local message="$4"
  local line
  line="$(rg -n -F "$literal" "$file" | head -n1 | cut -d: -f1 || true)"
  if [[ -z "$line" ]]; then
    echo "FAIL: missing item for guard check: $literal" >&2
    echo "  file: $file" >&2
    exit 1
  fi
  if ! sed -n "$((line-6)),$((line-1))p" "$file" | rg "$guard" >/dev/null 2>&1; then
    echo "FAIL: $message" >&2
    echo "  file: $file" >&2
    echo "  item: $literal" >&2
    exit 1
  fi
}

feature_block_line() {
  local file="$1"
  local block_name="$2"
  rg -n "^${block_name} = \\[$" "$file" | head -n1 | cut -d: -f1 || true
}

assert_feature_block_contains() {
  local file="$1"
  local block_name="$2"
  local feature_name="$3"
  local message="$4"
  local line
  line="$(feature_block_line "$file" "$block_name")"
  if [[ -z "$line" ]]; then
    echo "FAIL: missing feature block $block_name" >&2
    exit 1
  fi
  if ! sed -n "${line},$((line+10))p" "$file" | rg "^\s*\"${feature_name}\",\s*$" >/dev/null 2>&1; then
    echo "FAIL: $message" >&2
    exit 1
  fi
}

assert_feature_block_absent() {
  local file="$1"
  local block_name="$2"
  local feature_name="$3"
  local message="$4"
  local line
  line="$(feature_block_line "$file" "$block_name")"
  if [[ -z "$line" ]]; then
    echo "FAIL: missing feature block $block_name" >&2
    exit 1
  fi
  if sed -n "${line},$((line+10))p" "$file" | rg "^\s*\"${feature_name}\",\s*$" >/dev/null 2>&1; then
    echo "FAIL: $message" >&2
    exit 1
  fi
}

assert_csv_contains() {
  local csv="$1"
  local feature_name="$2"
  local message="$3"
  if ! printf '%s\n' "$csv" | rg "(^|,)${feature_name}(,|$)" >/dev/null 2>&1; then
    echo "FAIL: $message" >&2
    echo "  csv: $csv" >&2
    exit 1
  fi
}

assert_feature_block_absent "$CARGO_TOML" "default_runtime" "telegram" \
  "default_runtime must not include telegram"
assert_feature_block_absent "$CARGO_TOML" "default_runtime" "feishu" \
  "default_runtime must not include feishu"
assert_feature_block_absent "$CARGO_TOML" "default_runtime" "wecom" \
  "default_runtime must not include wecom"
assert_feature_block_contains "$CARGO_TOML" "default" "telegram" \
  "default cargo feature set should still include telegram for host/dev builds"
assert_feature_block_contains "$CARGO_TOML" "default" "feishu" \
  "default cargo feature set should still include feishu for host/dev builds"
assert_feature_block_contains "$CARGO_TOML" "default" "wecom" \
  "default cargo feature set should still include wecom for host/dev builds"
assert_feature_block_contains "$CARGO_TOML" "default" "qq_channel" \
  "default cargo feature set should still include qq_channel for host/dev builds"
assert_feature_block_absent "$CARGO_TOML" "default" "dingtalk" \
  "default cargo feature set must not include dingtalk"
assert_feature_block_absent "$CARGO_TOML" "default" "websocket" \
  "default cargo feature set must not include websocket"
assert_absent "$CARGO_TOML" 'ed25519-dalek' \
  "QQ webhook Ed25519 dependency must not remain after webhook removal"
assert_absent "$CARGO_TOML" '^hmac = ' \
  "legacy webhook HMAC dependency must not remain after webhook removal"
assert_absent "$CARGO_TOML" '^sha2 = ' \
  "legacy webhook SHA-2 dependency must not remain after webhook removal"
assert_absent "$CARGO_TOML" '^aes = ' \
  "legacy webhook AES dependency must not remain after webhook removal"
assert_absent "$CARGO_TOML" '^cbc = ' \
  "legacy webhook CBC dependency must not remain after webhook removal"
assert_absent "$CARGO_TOML" '^hex = ' \
  "legacy webhook hex dependency must not remain after webhook removal"
assert_contains "$CARGO_TOML" '^\[package\.metadata\.beetle\.package_profiles\.defaults\]$' \
  "Cargo.toml must define package-profile defaults in package metadata"
assert_contains "$CARGO_TOML" '^esp = "voice\+vision\+sensor"$' \
  "ESP default package profile must resolve from Cargo metadata"
assert_contains "$CARGO_TOML" '^linux = "linux-full"$' \
  "Linux default package profile must resolve from Cargo metadata"
assert_contains "$CARGO_TOML" '^"voice\+vision\+sensor" = \[$' \
  "Cargo.toml must define the shared ESP package-profile roots"

assert_contains "$BUILD_SH" 'scripts/expand_cargo_features\.py' \
  "build.sh must resolve package profiles from the Cargo feature graph helper"
assert_contains "$BUILD_SH" 'package_profile_features\(\)' \
  "build.sh must keep a dedicated named package-profile resolver helper"
assert_contains "$BUILD_SH" 'default_package_profile_for_target\(\)' \
  "build.sh must keep a dedicated default package-profile resolver helper"
assert_contains "$BUILD_SH" 'beetle_package_profile_query[[:space:]]*\\' \
  "build.sh must query named package profiles through the Cargo metadata helper"
assert_contains "$BUILD_SH" 'default_package_profile_for_target\(\)' \
  "build.sh must query default package profiles through the Cargo metadata helper"
assert_absent "$BUILD_SH" "roots_csv='default,capability_office,dingtalk'" \
  "build.sh must not hard-code package-profile roots after the Cargo metadata migration"

linux_full_features="$(python3 "$FEATURE_EXPANDER" --manifest "$CARGO_TOML" --package-profile 'linux-full' --format csv)"
assert_csv_contains "$linux_full_features" "default" \
  "linux-full expansion must retain the Cargo default root feature"
assert_csv_contains "$linux_full_features" "telegram" \
  "linux-full expansion must inherit Telegram from Cargo default"
assert_csv_contains "$linux_full_features" "feishu" \
  "linux-full expansion must inherit Feishu from Cargo default"
assert_csv_contains "$linux_full_features" "wecom" \
  "linux-full expansion must inherit WeCom from Cargo default"
assert_csv_contains "$linux_full_features" "qq_channel" \
  "linux-full expansion must inherit QQ from Cargo default"
assert_csv_contains "$linux_full_features" "dingtalk" \
  "linux-full expansion must keep DingTalk explicitly enabled"
assert_csv_contains "$linux_full_features" "capability_office" \
  "linux-full expansion must keep capability_office"
assert_csv_contains "$linux_full_features" "websocket" \
  "linux-full expansion must now keep websocket so Linux default builds are not runtime-trimmed"

esp_default_profile="$(python3 "$FEATURE_EXPANDER" --manifest "$CARGO_TOML" --default-target-kind esp --format value)"
if [[ "$esp_default_profile" != "voice+vision+sensor" ]]; then
  echo "FAIL: ESP default package profile must remain voice+vision+sensor" >&2
  echo "  actual: $esp_default_profile" >&2
  exit 1
fi

esp_default_features="$(python3 "$FEATURE_EXPANDER" --manifest "$CARGO_TOML" --default-target-kind esp --format csv)"
assert_csv_contains "$esp_default_features" "qq_channel" \
  "default ESP package expansion must now include QQ from Cargo package-profile metadata"

assert_line_guarded "$CHANNEL_CATALOG_RS" 'const TELEGRAM_ENTRY: CompiledChannelEntry = CompiledChannelEntry {' 'feature = "telegram"' \
  "channel_catalog.rs must gate Telegram catalog entries"
assert_line_guarded "$CHANNEL_CATALOG_RS" 'const FEISHU_ENTRY: CompiledChannelEntry = CompiledChannelEntry {' 'feature = "feishu"' \
  "channel_catalog.rs must gate Feishu catalog entries"
assert_line_guarded "$CHANNEL_CATALOG_RS" 'const DINGTALK_ENTRY: CompiledChannelEntry = CompiledChannelEntry {' 'feature = "dingtalk"' \
  "channel_catalog.rs must gate DingTalk catalog entries"
assert_line_guarded "$CHANNEL_CATALOG_RS" 'const WECOM_ENTRY: CompiledChannelEntry = CompiledChannelEntry {' 'feature = "wecom"' \
  "channel_catalog.rs must gate WeCom catalog entries"
assert_line_guarded "$CHANNEL_CATALOG_RS" 'const QQ_CHANNEL_ENTRY: CompiledChannelEntry = CompiledChannelEntry {' 'feature = "qq_channel"' \
  "channel_catalog.rs must gate QQ catalog entries"
assert_line_guarded "$CHANNEL_CATALOG_RS" 'const WEBSOCKET_ENTRY: CompiledChannelEntry = CompiledChannelEntry {' 'feature = "websocket"' \
  "channel_catalog.rs must gate websocket catalog entries"

assert_line_guarded "$CHANNELS_MOD_RS" 'pub(crate) mod telegram;' 'feature = "telegram"' \
  "channels/mod.rs must compile-gate the Telegram module"
assert_line_guarded "$CHANNELS_MOD_RS" 'pub(crate) mod feishu;' 'feature = "feishu"' \
  "channels/mod.rs must compile-gate the Feishu module"
assert_line_guarded "$CHANNELS_MOD_RS" 'pub(crate) mod dingtalk;' 'feature = "dingtalk"' \
  "channels/mod.rs must compile-gate the DingTalk module"
assert_line_guarded "$CHANNELS_MOD_RS" 'pub(crate) mod wecom;' 'feature = "wecom"' \
  "channels/mod.rs must compile-gate the WeCom module"
assert_line_guarded "$CHANNELS_MOD_RS" 'mod qq;' 'feature = "qq_channel"' \
  "channels/mod.rs must compile-gate the QQ module"
assert_line_guarded "$CHANNELS_MOD_RS" 'mod websocket;' 'feature = "websocket"' \
  "channels/mod.rs must compile-gate the websocket module"
assert_line_guarded "$CHANNELS_MOD_RS" 'edit_message_text as tg_edit_message_text' 'feature = "telegram"' \
  "channels/mod.rs must compile-gate Telegram re-exports"
assert_line_guarded "$CHANNELS_MOD_RS" 'acquire_tenant_token as feishu_acquire_token' 'feature = "feishu"' \
  "channels/mod.rs must compile-gate Feishu re-exports"
assert_line_guarded "$CHANNELS_MOD_RS" 'pub use dingtalk::{flush_dingtalk_sends, run_dingtalk_sender_loop};' 'feature = "dingtalk"' \
  "channels/mod.rs must compile-gate DingTalk re-exports"
assert_line_guarded "$CHANNELS_MOD_RS" 'new_wecom_aibot_route_store, run_wecom_aibot_loop, WecomAibotRouteStore, WECOM_AIBOT_WS_URL,' 'feature = "wecom"' \
  "channels/mod.rs must compile-gate WeCom AI Bot re-exports"
assert_line_guarded "$CHANNELS_MOD_RS" 'flush_qq_channel_sends, is_ws_online, new_shared_qq_token_cache, new_shared_qq_ws_status,' 'feature = "qq_channel"' \
  "channels/mod.rs must compile-gate QQ re-exports"
assert_line_guarded "$CHANNELS_MOD_RS" 'pub use websocket::{WebSocketSink, MAX_WS_CONNECTIONS, MAX_WS_MESSAGE_LEN};' 'feature = "websocket"' \
  "channels/mod.rs must compile-gate websocket re-exports"

assert_line_guarded "$CHANNELS_DISPATCH_RS" 'let telegram = if enabled == "telegram" && !config.tg_token.trim().is_empty() {' 'feature = "telegram"' \
  "dispatch.rs must compile-gate Telegram sink assembly"
assert_line_guarded "$CHANNELS_DISPATCH_RS" 'if let Some(tg_rx) = rx_set.telegram.take() {' 'feature = "telegram"' \
  "dispatch.rs must compile-gate Telegram sender spawn"
assert_line_guarded "$CHANNELS_DISPATCH_RS" 'let feishu = if enabled == "feishu"' 'feature = "feishu"' \
  "dispatch.rs must compile-gate Feishu sink assembly"
assert_line_guarded "$CHANNELS_DISPATCH_RS" 'if let Some(c) = rx_set.feishu.take() {' 'feature = "feishu"' \
  "dispatch.rs must compile-gate Feishu sender spawn"
assert_line_guarded "$CHANNELS_DISPATCH_RS" 'let dingtalk = if enabled == "dingtalk" {' 'feature = "dingtalk"' \
  "dispatch.rs must compile-gate DingTalk sink assembly"
assert_line_guarded "$CHANNELS_DISPATCH_RS" 'if let Some(c) = rx_set.dingtalk.take() {' 'feature = "dingtalk"' \
  "dispatch.rs must compile-gate DingTalk sender spawn"
assert_line_guarded "$CHANNELS_DISPATCH_RS" 'let wecom = if enabled == "wecom"' 'feature = "wecom"' \
  "dispatch.rs must compile-gate WeCom sink assembly"
assert_line_guarded "$CHANNELS_DISPATCH_RS" 'let qq_channel = if enabled == "qq_channel"' 'feature = "qq_channel"' \
  "dispatch.rs must compile-gate QQ sink assembly"
assert_line_guarded "$CHANNELS_DISPATCH_RS" 'if let Some(c) = rx_set.qq_channel.take() {' 'feature = "qq_channel"' \
  "dispatch.rs must compile-gate QQ sender spawn"
assert_line_guarded "$CHANNELS_DISPATCH_RS" 'sinks.register("websocket", Box::new(super::WebSocketSink::new("ws")));' 'feature = "websocket"' \
  "dispatch.rs must compile-gate websocket sink assembly"

assert_line_guarded "$CHANNELS_CONNECTIVITY_RS" '"telegram" => Some(crate::channels::telegram::check_connectivity(config, http)),' 'feature = "telegram"' \
  "connectivity.rs must compile-gate Telegram probes"
assert_line_guarded "$CHANNELS_CONNECTIVITY_RS" '"feishu" => Some(crate::channels::feishu::check_connectivity(config, http)),' 'feature = "feishu"' \
  "connectivity.rs must compile-gate Feishu probes"
assert_line_guarded "$CHANNELS_CONNECTIVITY_RS" '"dingtalk" => Some(crate::channels::dingtalk::check_connectivity(config, http)),' 'feature = "dingtalk"' \
  "connectivity.rs must compile-gate DingTalk probes"
assert_line_guarded "$CHANNELS_CONNECTIVITY_RS" '"wecom" => Some(crate::channels::wecom::check_connectivity(config, http)),' 'feature = "wecom"' \
  "connectivity.rs must compile-gate WeCom probes"
assert_line_guarded "$CHANNELS_CONNECTIVITY_RS" '"qq_channel" => Some(crate::channels::qq::check_connectivity(config, http)),' 'feature = "qq_channel"' \
  "connectivity.rs must compile-gate QQ probes"

assert_line_guarded "$LIB_RS" 'flush_telegram_sends, get_bot_username, poll_telegram_once, run_telegram_poll_loop,' 'feature = "telegram"' \
  "lib.rs must compile-gate Telegram public re-exports"
assert_line_guarded "$LIB_RS" 'pub use channels::{flush_dingtalk_sends, run_dingtalk_sender_loop};' 'feature = "dingtalk"' \
  "lib.rs must compile-gate DingTalk public re-exports"
assert_line_guarded "$LIB_RS" 'new_wecom_aibot_route_store, run_wecom_aibot_loop, WecomAibotRouteStore, WECOM_AIBOT_WS_URL,' 'feature = "wecom"' \
  "lib.rs must compile-gate WeCom AI Bot public re-exports"
assert_line_guarded "$LIB_RS" 'pub use channels::{flush_qq_channel_sends, run_qq_sender_loop};' 'feature = "qq_channel"' \
  "lib.rs must compile-gate QQ public re-exports"
assert_line_guarded "$LIB_RS" 'pub use channels::WebSocketSink;' 'feature = "websocket"' \
  "lib.rs must compile-gate websocket public re-exports"
assert_line_guarded "$MAIN_RS" 'struct TelegramTypingNotifier {' 'feature = "telegram"' \
  "main.rs must compile-gate Telegram typing notifier"
assert_line_guarded "$MAIN_RS" 'if enabled_channel == "telegram" && !assembly.config.tg_token.trim().is_empty() {' 'feature = "telegram"' \
  "main.rs must compile-gate Telegram poll ingress"
assert_line_guarded "$MAIN_RS" 'if enabled_channel == "wecom" {' 'feature = "wecom"' \
  "main.rs must compile-gate WeCom AI Bot WSS ingress"
assert_contains "$ROOT_DIR/src/orchestrator/state.rs" 'pub fn channel_to_index\(channel: &str\) -> Option<usize>' \
  "orchestrator state must derive channel health indexes from the compiled channel catalog"
assert_contains "$ROOT_DIR/src/capability_package.rs" 'if channel != "\*" && !crate::channel_catalog::channel_is_compiled\(channel\) \{' \
  "capability package validation must reject channel compatibility entries for uncompiled channels"
assert_absent "$ROOT_DIR/src/platform/http_server/handlers/mod.rs" 'pub mod wecom_webhook;' \
  "HTTP handlers must not re-register the removed WeCom callback handler"
assert_absent "$ROOT_DIR/src/platform/http_server/handlers/mod.rs" 'pub mod dingtalk_webhook;' \
  "HTTP handlers must not re-register the removed DingTalk callback handler"
assert_absent "$ROOT_DIR/src/platform/http_server/handlers/mod.rs" 'pub mod qq_webhook;' \
  "HTTP handlers must not re-register the removed QQ callback handler"
assert_absent "$ROOT_DIR/src/platform/http_server/handlers/mod.rs" 'pub mod feishu_event;' \
  "HTTP handlers must not re-register the removed Feishu callback handler"
assert_contains "$ROOT_DIR/src/platform/http_server/router/dispatch.rs" 'social_channel_webhook_routes_are_not_registered' \
  "HTTP router must keep a regression test proving removed social webhook routes return 404"
assert_absent "$ROOT_DIR/src/platform/operator_surface.rs" '/api/wecom/webhook' \
  "operator surface inventory must not list removed WeCom callback routes"
assert_absent "$ROOT_DIR/src/platform/operator_surface.rs" '/api/dingtalk/webhook' \
  "operator surface inventory must not list removed DingTalk callback routes"
assert_absent "$ROOT_DIR/src/platform/operator_surface.rs" '/api/webhook/qq' \
  "operator surface inventory must not list removed QQ callback routes"
assert_absent "$ROOT_DIR/src/platform/operator_surface.rs" '/api/feishu/event' \
  "operator surface inventory must not list removed Feishu callback routes"

echo "esp_channel_compile_gate_test: ok"
