# Configuration API

[中文](../zh-cn/config-api.md) | **English** | [Doc index](../README.md)

This page is for people building their own frontend, script, or integration. Each endpoint is described in terms of purpose, request, and response.

## Request rules

- Base address: during first setup, the common entry is `http://192.168.4.1`; once the device is on your network, use its current address.
- CORS: `/api/*` supports cross-origin access, and `OPTIONS` can be called directly.
- Response format: everything is JSON except `GET /api/soul`, `GET /api/user`, `GET /api/skills?name=...`, and `GET /api/metrics?format=prometheus`.
- Error format: common errors return `{"error":"..."}`.
- Pairing code: send it through `?code=` or `X-Pairing-Code`.
- CSRF: send it through `X-CSRF-Token`; fetch it from `GET /api/csrf_token`.
- Config-save routes expect the full object, not a partial patch:
  `POST /api/config/llm`, `POST /api/config/channels`, `POST /api/config/system`,
  `POST /api/config/hardware`, `POST /api/config/audio`, `POST /api/config/display`.

### Auth levels

- `Public`: no activation and no pairing code required.
- `Activated`: the device must already have a pairing code, but this request does not need to send it again.
- `Pairing code`: this request must send the pairing code.
- `Pairing code + CSRF`: this request must send both pairing code and CSRF.

### Common status codes

- `200`: request completed.
- `202`: request accepted and continues asynchronously.
- `400`: invalid parameter or request body.
- `401`: device is not activated, or pairing code is wrong.
- `403`: CSRF failed, webhook token failed, or the route currently requires an operator window.
- `404`: resource not found.
- `500`: server-side failure.
- `503`: temporarily unavailable, such as scanner not ready or queue unavailable.

## Activation and security

**GET /api/pairing_code**

Purpose: check whether the device already has a pairing code and return the current locale.

Auth: `Public`

Success response: `200 application/json`

```json
{
  "code_set": true,
  "locale": "zh"
}
```

**POST /api/pairing_code**

Purpose: set the pairing code for the first time. It can only be set once.

Auth: `Public`

Request body: `application/json`

```json
{
  "code": "123456"
}
```

Success response: `200 application/json`

```json
{
  "ok": true
}
```

Common failures:

- `400`: a pairing code already exists.
- `400`: `code` is not a 6-digit number.

**GET /api/csrf_token**

Purpose: get the CSRF token required by write routes.

Auth: `Public`

Success response: `200 application/json`

```json
{
  "csrf_token": "..."
}
```

### Recommended activation flow

1. Call `GET /api/pairing_code` to see whether the device is already activated.
2. If not, call `POST /api/pairing_code` to set the pairing code.
3. Call `GET /api/csrf_token` to get the CSRF token.
4. After that, send pairing code and CSRF with every write route.

## Configuration routes

**GET /api/config**

Purpose: read the current full config.

Auth: `Pairing code`

Success response: `200 application/json`

The response is the full config object, plus `locale` and `build_package`. It includes sensitive fields and should not be exposed to unauthenticated pages.

**POST /api/config/wifi**

Purpose: save network settings.

Auth: `Pairing code + CSRF`

Request body: `application/json`

```json
{
  "wifi_ssid": "MyWiFi",
  "wifi_pass": "secret"
}
```

Optional query parameter: `restart=1`
When present, the device restarts automatically after a successful save.

Success response: `200 application/json`

```json
{
  "ok": true,
  "restart_required": true
}
```

**POST /api/config/system**

Purpose: save the system segment.

Auth: `Pairing code + CSRF`

Request body: `application/json`

Fields:

- `wifi_ssid`
- `wifi_pass`
- `proxy_url`
- `session_max_messages`
- `tg_group_activation`
- `locale`

Success response: `200 application/json`

```json
{
  "ok": true
}
```

**POST /api/config/llm**

Purpose: save LLM settings.

Auth: `Pairing code + CSRF`

Request body: `application/json`

Fields:

- `llm_sources`
- `llm_router_source_index`
- `llm_worker_source_index`

Each item in `llm_sources` contains:

- `provider`
- `api_key`
- `model`
- `api_url`
- `max_tokens`

Example:

```json
{
  "llm_sources": [
    {
      "provider": "provider_name",
      "api_key": "your_key",
      "model": "model_name",
      "api_url": "https://example.com/v1/chat/completions",
      "max_tokens": 1024
    }
  ],
  "llm_router_source_index": 0,
  "llm_worker_source_index": 0
}
```

Success response: `200 application/json`

```json
{
  "ok": true
}
```

Related guide: [LLM providers](llm-providers.md).

**POST /api/config/channels**

Purpose: save chat-channel settings.

Auth: `Pairing code + CSRF`

Request body: `application/json`

Field groups:

- Common: `enabled_channel`
- Telegram: `tg_token`, `tg_allowed_chat_ids`
- Feishu: `feishu_app_id`, `feishu_app_secret`, `feishu_verification_token`, `feishu_encrypt_key`, `feishu_allowed_chat_ids`
- DingTalk: `dingtalk_webhook_url`, `dingtalk_app_secret`
- WeCom: `wecom_corp_id`, `wecom_corp_secret`, `wecom_agent_id`, `wecom_default_touser`, `wecom_token`, `wecom_encoding_aes_key`
- QQ Channel: `qq_channel_app_id`, `qq_channel_secret`
- Custom webhook: `webhook_enabled`, `webhook_token`

Field notes:

- `feishu_verification_token`: Feishu HTTP event-subscription Verification Token; validated for both plaintext and decrypted event payloads.
- `feishu_encrypt_key`: Feishu HTTP event-subscription Encrypt Key; when configured, `/api/feishu/event` verifies `X-Lark-Signature` and decrypts `encrypt` using Feishu's official scheme.
- `dingtalk_webhook_url`: DingTalk custom-robot webhook for proactive sends outside the current conversation; when empty, `enabled_channel=dingtalk` still works in session-reply-only mode via callback `sessionWebhook`.
- `dingtalk_app_secret`: DingTalk custom-robot signing secret; only used for proactive sends to `dingtalk_webhook_url`.
- `wecom_token`: WeCom callback Token; `GET/POST /api/wecom/webhook` require it and verify signatures with it.
- `wecom_encoding_aes_key`: WeCom secure-mode EncodingAESKey; when configured, GET verification decrypts `echostr` and POST decrypts the XML `Encrypt` payload.

Allowed `enabled_channel` values:

- empty string
- `telegram`
- `feishu`
- `dingtalk`
- `wecom`
- `qq_channel`

Minimal example:

```json
{
  "enabled_channel": "telegram",
  "tg_token": "bot_token",
  "tg_allowed_chat_ids": "123456",
  "webhook_enabled": false,
  "webhook_token": ""
}
```

Success response: `200 application/json`

```json
{
  "ok": true
}
```

**GET /api/config/hardware**

Purpose: read the current hardware config.

Auth: `Pairing code`

Success response: `200 application/json`

If nothing has been saved yet, the default response is:

```json
{
  "hardware_devices": []
}
```

**POST /api/config/hardware**

Purpose: save hardware config.

Auth: `Pairing code + CSRF`

Request body: `application/json`

Top-level fields:

- `hardware_devices`
- `i2c_bus`
- `i2c_devices`
- `i2c_sensors`

Minimal example:

```json
{
  "hardware_devices": [],
  "i2c_bus": null,
  "i2c_devices": [],
  "i2c_sensors": []
}
```

Success response: `200 application/json`

```json
{
  "ok": true
}
```

Field guide: [Hardware device config](hardware-device-config.md).

**GET /api/config/audio**

Purpose: read the current audio config.

Auth: `Pairing code`

Success response: `200 application/json`

The response is the full audio config object. If no config has been saved yet, the route returns a disabled default object.

**POST /api/config/audio**

Purpose: save audio config.

Auth: `Pairing code + CSRF`

Request body: `application/json`

Top-level fields:

- `version`
- `enabled`
- `service_provider`
- `microphone`
- `speaker`
- `vad`
- `wake_word`
- `speech`
- `tts`
- `realtime`
- `ambient_listening`
- `led_indicator`

Optional query parameter: `restart=1`

Success response: `200 application/json`

```json
{
  "ok": true,
  "restart_required": true
}
```

**GET /api/config/display**

Purpose: read the current display config.

Auth: `Pairing code`

Success response: `200 application/json`

The response is the full display config object. If no config has been saved yet, the route returns a disabled default object.

**POST /api/config/display**

Purpose: save display config.

Auth: `Pairing code + CSRF`

Request body: `application/json`

Common fields:

- `enabled`
- `driver`
- `bus`
- `width`
- `height`
- `rotation`
- `color_order`
- `invert_colors`
- `offset_x`
- `offset_y`
- `spi`
- `fb_device`
- `backlight_sysfs`
- `sleep_timeout_secs`

Optional query parameter: `restart=1`

Success response: `200 application/json`

```json
{
  "ok": true,
  "restart_required": true
}
```

Field guide: [Display config](display.md).

**GET /api/wifi/scan**

Purpose: scan nearby WiFi networks for the config page or an external frontend.

Auth: `Public`

Success response: `200 application/json`

```json
[
  {
    "ssid": "MyWiFi",
    "rssi": -50
  }
]
```

Common failures:

- `503`: scanning is not currently available.
- `500`: scanning failed.

**GET /api/hardware/discovery**

Purpose: discover attachable external hardware.

Auth: `Pairing code`

Query parameters:

- `bus`: the current public value is `usb`
- `capability`: `audio_input`, `audio_output`, `camera`, `serial`, `hid`

Example:

```text
GET /api/hardware/discovery?bus=usb&capability=audio_output
```

Success response: `200 application/json`

Response fields:

- `bus`
- `capability`
- `items`

Each item in `items` contains:

- `device_ref`
- `label`
- `kind`
- `capabilities`
- `is_default`
- `metadata`

Common failures:

- `400`: `bus` or `capability` is missing or invalid.
- `503`: discovery is not currently available for that capability.

## Accounts and office-capability routes

**GET /api/config/providers**

Purpose: read the provider catalog for account creation.

Auth: `Pairing code`

Optional query parameter: `capability`

Success response: `200 application/json`

Response structure:

- `count`
- `items`

Each item in `items` contains:

- `provider_kind`
- `capabilities`
- `account_fields`
- `config_fields`

**GET /api/config/capabilities**

Purpose: read the current status of office capabilities.

Auth: `Pairing code`

Success response: `200 application/json`

Response structure:

- `count`
- `items`

Each item in `items` contains:

- `capability`
- `default_account_key`
- `selection_status`
- `selected_account_key`
- `ready`
- `next_action`
- `accounts`

**GET /api/config/capabilities/:capability**

Purpose: read the status of one capability.

Auth: `Pairing code`

Path parameter: `capability`

Current public values include:

- `mail`
- `calendar`
- `documents`
- `contacts_directory`

Success response: `200 application/json`

The response uses the same shape as one item from `GET /api/config/capabilities`.

**GET /api/config/accounts**

Purpose: list connected accounts.

This HTTP API remains the complete public account-management surface. It is not narrowed to match the official UI or the LLM-facing office tool surface. In the LLM tool path, `office_status` is the status/readiness entrypoint and `office_config` is the management entrypoint.

Auth: `Pairing code`

Optional query parameters:

- `provider_kind`
- `capability`

Success response: `200 application/json`

Response structure:

- `count`
- `items`

Each item in `items` contains:

- `account_key`
- `provider_kind`
- `account_label`
- `identity_class`
- `enabled_capabilities`
- `selected_for_capabilities`
- `readiness`
- `next_action`
- `missing_fields_count`
- `has_runtime_error`

**POST /api/config/accounts**

Purpose: create an account or update its base information.

This API keeps the complete public account-management contract for external consumers. The official UI is only one consumer; the LLM-facing office tools intentionally use a narrower mainline path on top of the same underlying office authority.

Auth: `Pairing code + CSRF`

Request body: `application/json`

Top-level fields:

- `account`
- `set_defaults`
- `clear_defaults`
- `policy_patch`
- `config`

`account` fields:

- `account_key`
- `provider_kind`
- `external_account_id`
- `account_label`
- `identity_class`
- `enabled_capabilities`

The `config` field uses the same body shape as `POST /api/config/accounts/:account_key/config`.

Success response: `200 application/json`

The response is the account detail object.

**GET /api/config/accounts/:account_key**

Purpose: read one account with editable fields.

Auth: `Pairing code`

Success response: `200 application/json`

Response structure:

- `account`
- `assessment`
- `fields`

**POST /api/config/accounts/:account_key/config**

Purpose: save provider-specific config fields for one account.

Auth: `Pairing code + CSRF`

Request body: `application/json`

```json
{
  "fields": {
    "tenant_id": "xxx",
    "client_id": "xxx"
  },
  "clear_fields": [
    "old_secret"
  ]
}
```

Success response: `200 application/json`

The response is the updated account detail object.

**POST /api/config/accounts/:account_key/probe**

Purpose: check whether the account is usable right now.

Auth: `Pairing code + CSRF`

Request body: none.

Success response: `200 application/json`

Response fields:

- `account_key`
- `provider_kind`
- `configured`
- `disposition`
- `reason`

**POST /api/config/accounts/:account_key/revoke**

Purpose: revoke the account and optionally clear related current state.

Auth: `Pairing code + CSRF`

Request body: `application/json`

```json
{
  "clear_runtime_status": true
}
```

The body can also be empty. When empty, it behaves the same as `true`.

Success response: `200 application/json`

```json
{
  "ok": true,
  "account_key": "mail-main",
  "cleared_runtime_status": true
}
```

**DELETE /api/config/accounts/:account_key**

Purpose: delete the account.

Auth: `Pairing code + CSRF`

Success response: `200 application/json`

```json
{
  "ok": true,
  "account_key": "mail-main",
  "deleted": true
}
```

## Content, sessions, skills, and maintenance

**GET /api/soul**

Purpose: read the system text.

Auth: `Activated`

Success response: `200 text/plain`

The response body is the raw text content.

**POST /api/soul**

Purpose: save the system text.

Auth: `Pairing code + CSRF`

Two request forms are accepted:

- `text/plain`: send the raw text directly
- `application/json`: `{"content":"..."}`

Success response: `200 application/json`

```json
{
  "ok": true
}
```

**GET /api/user**

Purpose: read the user text.

Auth: `Activated`

Success response: `200 text/plain`

**POST /api/user**

Purpose: save the user text.

Auth: `Pairing code + CSRF`

The request format is the same as `POST /api/soul`.

Success response: `200 application/json`

```json
{
  "ok": true
}
```

**GET /api/sessions**

Purpose: list sessions, or read recent messages for one session.

Auth: `Activated`

Query parameters:

- List mode: `page`, `limit`
- Single-session mode: `chat_id`

List-mode success response: `200 application/json`

```json
{
  "items": [
    "chat-1",
    "chat-2"
  ],
  "total": 2,
  "page": 1,
  "limit": 20,
  "total_pages": 1
}
```

Single-session success response: `200 application/json`

The response body is an array of recent messages.

**DELETE /api/sessions**

Purpose: delete one session.

Auth: `Pairing code + CSRF`

Query parameter: `chat_id`

Success response: `200 application/json`

```json
{
  "ok": true
}
```

**GET /api/memory/status**

Purpose: read memory status and, when needed, run a targeted deep inspection.

Auth: `Activated`

Common query parameters:

- `chat_id`
- `channel`
- `query`
- `run_id`
- `deep=1`
- `snapshot_mode=full_restore`
- `memory_system_kind=esp_compact`
- `memory_system_kind=linux_full`

Default-mode success response: `200 application/json`

Top-level fields:

- `memory_system_kind`
- `memory_len`
- `soul_len`
- `user_len`
- `long_term_count`
- `continuity_capsule_count`
- `stores`
- `personality`
- `continuity_tooling`
- `continuity_capsules`
- `task_execution`
- `learning`
- `diagnosis`
- `operator_surface`

When `deep=1` is present together with `chat_id`, the response also includes `inspection`.

Special case:

- `403`: this deep inspection currently requires `POST /api/operator/window` first.

**POST /api/memory/maintenance**

Purpose: submit a memory maintenance job.

Auth: `Pairing code + CSRF`

Request body: `application/json`

```json
{
  "action": "run_repair_plan",
  "chat_id": "chat-1",
  "channel": "qq_channel"
}
```

Current public `action` values:

- `run_repair_plan`
- `rebuild_continuity_snapshot`
- `reconcile_relationship_governance`
- `replay_recovery`
- `refresh_operator_digest`

Success response: `202 application/json`

The response includes at least:

- `accepted`
- `delivery`

**GET /api/tools**

Purpose: read the currently available tool list.

Auth: `Activated`

Success response: `200 application/json`

```json
[
  {
    "name": "web_search",
    "i18n_key": "tools.web_search"
  }
]
```

**GET /api/capability_packages**

Purpose: read capability-package status.

Auth: `Activated`

Success response: `200 application/json`

Top-level fields:

- `installed`
- `enabled`
- `active_now`
- `workflow_count`
- `skill_fragment_count`
- `policy_overlay_count`
- `asset_count`
- `packages`

Each item in `packages` contains:

- `package_id`
- `version`
- `display_name`
- `enabled`
- `compatible_now`
- `requirements_satisfied`
- `workflow_count`
- `skill_fragment_count`
- `policy_count`
- `asset_count`
- `required_capabilities`
- `missing_capabilities`
- `channel_compatibility`
- `rollback_available`

**POST /api/capability_packages**

Purpose: install, enable, disable, uninstall, or roll back a capability package.

Auth: `Pairing code + CSRF`

Request body: `application/json`

Enable, disable, uninstall, rollback:

```json
{
  "op": "enable",
  "package_id": "package_id"
}
```

Install:

```json
{
  "op": "install",
  "payload": {
    "manifest": {
      "package_id": "package_id",
      "version": "1.0.0",
      "display_name": "Package name"
    },
    "skills": [],
    "workflows": [],
    "policies": [],
    "assets": [],
    "enable_on_install": true
  }
}
```

Current public `op` values:

- `install`
- `enable`
- `disable`
- `uninstall`
- `rollback`

Success response: `200 application/json`

```json
{
  "ok": true,
  "outcome": {}
}
```

**GET /api/skills**

Purpose: list skills, or read one skill file.

Auth: `Activated`

Query parameter:

- without `name`: returns the list
- with `name`: returns one skill file

List-mode success response: `200 application/json`

```json
{
  "skills": [
    {
      "name": "example",
      "enabled": true
    }
  ],
  "order": [
    "example"
  ]
}
```

Single-skill success response: `200 text/plain`

The response body is the skill file content.

**POST /api/skills**

Purpose: write skill content, enable or disable a skill, or reorder skills.

Auth: `Pairing code + CSRF`

Three request forms are supported:

Write content:

```json
{
  "name": "example",
  "content": "# Skill"
}
```

Enable or disable:

```json
{
  "name": "example",
  "enabled": false
}
```

Reorder:

```json
{
  "order": [
    "example",
    "another"
  ]
}
```

Success response: `200 application/json`

```json
{
  "ok": true
}
```

**DELETE /api/skills**

Purpose: delete one skill.

Auth: `Pairing code + CSRF`

Query parameter: `name`

Success response: `200 application/json`

```json
{
  "ok": true
}
```

Common failures:

- `404`: skill not found.

**POST /api/skills/import**

Purpose: import a skill from a URL.

Auth: `Pairing code + CSRF`

Request body: `application/json`

```json
{
  "url": "https://example.com/skill.md",
  "name": "imported-skill"
}
```

Success response: `200 application/json`

```json
{
  "ok": true
}
```

## Status and operations

**GET /api/health**

Purpose: read the lightweight status summary.

Auth: `Activated`

Success response: `200 application/json`

Top-level fields:

- `wifi`
- `last_error`
- `display`
- `audio`
- `workflow`

**GET /api/operator/status**

Purpose: read the full operator-facing status.

Auth: `Activated`

Success response: `200 application/json`

Top-level fields:

- `platform_contract`
- `build_package`
- `operator_surface`
- `reply_pipeline`
- `delivery_diagnosis`
- `system_diagnosis`
- `memory_operator_surface`
- `workflow`
- `programmable_reasoning`
- `os_closure`
- `initiative`
- `presence`
- `runtime_mode`
- `soul_kernel`
- `capability_planes`

**GET /api/metrics**

Purpose: read the metrics snapshot.

Auth: `Activated`

Success response: `200 application/json`

The response is a metrics object. Common fields include:

- `messages_in`
- `messages_out`
- `llm_calls`
- `llm_errors`
- `tool_calls`
- `tool_errors`
- `llm_last_ms`
- `e2e_last_ms`

**GET /api/metrics?format=prometheus**

Purpose: read metrics in Prometheus text format.

Auth: `Activated`

Success response: `200 text/plain`

**GET /api/resource**

Purpose: read the resource snapshot.

Auth: `Activated`

Success response: `200 application/json`

Top-level fields:

- `pressure`
- `tls_fragmentation_risk`
- `storage_contention_risk`
- `heap_free_internal`
- `heap_free_spiram`
- `heap_largest_block_internal`
- `active_http_count`
- `active_wss_count`
- `active_agent_tasks`
- `inbound_depth`
- `outbound_depth`
- `budget`
- `session_count`
- `storage_used_kb`
- `storage_total_kb`

**GET /api/diagnose**

Purpose: read diagnostic results.

Auth: `Activated`

Success response: `200 application/json`

The response body is an array of diagnostic results.

**GET /api/system_info**

Purpose: read basic device information.

Auth: `Activated`

Success response: `200 application/json`

Common fields:

- `product_name`
- `current_time`
- `firmware_version`
- `board_id`
- `ota_available`
- `locale`
- `lan_ip`
- `workflow`
- `programmable_reasoning`
- `storage_media`

**GET /api/channel_connectivity**

Purpose: check current channel connectivity.

Auth: `Activated`

Success response: `200 application/json`

Top-level field:

- `channels`

**POST /api/operator/window**

Purpose: open a temporary operator window so protected operator routes can be accessed.

Auth: `Pairing code + CSRF`

Success response: `200 application/json`

Response fields:

- `opened`
- `operator_window`
- `windowed_endpoints`

**POST /api/restart**

Purpose: restart the device.

Auth: `Pairing code + CSRF`

Success response: `200 application/json`

```json
{
  "ok": true
}
```

**POST /api/config_reset**

Purpose: clear the current config and return the device to the unactivated state.

Auth: `Pairing code + CSRF`

Success response: `200 application/json`

```json
{
  "ok": true
}
```

**GET /api/ota/check**

Purpose: check whether an update is available.

Auth: `Activated`

Optional query parameter: `channel`
If omitted, the default is `stable`.

Success response: `200 application/json`

Response fields:

- `current_version`
- `update_available`
- `latest_version`
- `url`
- `release_notes`
- `error`

Different situations return different subsets of these fields.

**POST /api/ota**

Purpose: start an update.

Auth: `Pairing code + CSRF`

Request body: `application/json`

```json
{
  "url": "https://example.com/beetle.bin"
}
```

Success response: `200 application/json`

```json
{
  "ok": true
}
```

## Callback routes

**POST /api/webhook**

Purpose: receive a custom webhook message.

Auth: `Pairing code + CSRF`

Extra token check:

- `X-Webhook-Token`
- or query parameter `token`

Request body: raw text content.

Success response: `200 application/json`

```json
{
  "ok": true
}
```

Common failures:

- `401`: webhook token is wrong.
- `403`: webhook is disabled, or no webhook token is configured.
- `413`: content is too large.
- `503`: message queue is full.

**Platform callback routes**

These routes receive platform callback payloads directly. Request bodies, signatures, and validation follow the platform's own rules:

- `POST /api/feishu/event`
- `POST /api/dingtalk/webhook`
- `GET /api/wecom/webhook`
- `POST /api/wecom/webhook`
- `POST /api/webhook/qq`

Current behavior:

- Feishu: `/api/feishu/event` supports `url_verification` and `im.message.receive_v1`; it validates `feishu_verification_token`, verifies `X-Lark-Signature` and decrypts `encrypt` when `feishu_encrypt_key` is configured, and deduplicates HTTP webhook delivery by `message_id`.
- DingTalk: `/api/dingtalk/webhook` receives app-robot callbacks and caches `sessionWebhook` for in-session replies; proactive sends still use `dingtalk_webhook_url`, signed with `dingtalk_app_secret` when configured.
- WeCom: `GET /api/wecom/webhook` supports both plaintext and secure-mode URL verification; `POST /api/wecom/webhook` supports plaintext XML and secure-mode `Encrypt` XML, verifies `msg_signature`, and checks decrypted `receiveid == wecom_corp_id`; when no reply content is needed it returns HTTP 200 with an empty body.
- QQ: `POST /api/webhook/qq` still verifies signatures with QQ Bot's Ed25519 scheme; outbound group/C2C replies require an existing passive-reply `msg_id`, and connectivity now requires both access-token exchange and an online QQ WebSocket session.
