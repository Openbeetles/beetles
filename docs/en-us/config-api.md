# Configuration API

[中文](../zh-cn/config-api.md) | **English** | [Doc index](../README.md)

The configuration API is the reference for custom frontends, scripts, and integrations.
If basic setup is not finished yet, start with [configuration.md](configuration.md) first.

Start with the activation flow, then the configuration routes.
Come back to the later sections only when you need office integration, maintenance, or callback details.

Each endpoint is described in terms of purpose, request, and response.

## Request rules

- Base address: during first setup, the common entry is `http://192.168.4.1`; if this is a Linux embedded device and its STA side already uses `192.168.4.0/24`, Beetle moves the hotspot to `http://172.16.42.1`; once the device is on your network, use its current address.
- CORS: `/api/*` supports cross-origin access, and `OPTIONS` can be called directly.
- Response format: everything is JSON except `GET /api/skills?name=...` and `GET /api/metrics?format=prometheus`.
- The legacy `/api/soul` and `/api/user` content endpoints are retired and are not part of this contract.
- Error format:
  - Product and official configuration-surface APIs now return `{"error_key":"..."}` as the stable system-generated error contract.
  - If the failure comes from an upstream third-party provider, the body may also include `upstream_error`, `upstream_status`, `error_stage`, and `provider_kind`.
  - Debug / operator / protocol-compatibility routes are exempt and may still return raw English text or protocol-native bodies.
- Pairing code: send it through `?code=` or `X-Pairing-Code`.
- CSRF: send it through `X-CSRF-Token`; fetch it from `GET /api/csrf_token`.
- Config-save routes expect the full object, not a partial patch:
  `POST /api/config/llm`, `POST /api/config/channels`, `POST /api/config/system`,
  `POST /api/config/hardware`, `POST /api/config/audio`, `POST /api/config/display`.
- Custom frontends targeting ESP devices should serialize `/api/*` calls for the same device. First-screen loads should stay limited to activation, security, and lightweight status requests; slow diagnostics such as `/api/channel_connectivity`, `/api/wifi/scan`, and hardware discovery should be user-triggered. Do not add or depend on a catch-all `/api/device_snapshot` aggregate.

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
- `403`: CSRF failed, webhook token failed, or the route currently requires a temporary maintenance window.
- `404`: resource not found.
- `500`: server-side failure.
- `503`: temporarily unavailable, such as scanner not ready or queue unavailable.

### Product API error contract

Non-debug/operator product and official configuration-surface `/api/*` routes now follow:

```json
{
  "error_key": "common.not_found"
}
```

If the failure comes from an upstream provider, the response may also include:

```json
{
  "error_key": "office.provider_error",
  "provider_kind": "microsoft365_mail",
  "error_stage": "office_probe",
  "upstream_status": 401,
  "upstream_error": "AADSTS7000215: Invalid client secret is provided."
}
```

Notes:

- `error_key` is the stable semantic contract that frontends and other clients translate.
- `upstream_error` is raw troubleshooting text and is not translated.
- Only debug/operator routes and protocol-compatibility routes are outside this contract.

Currently emitted infrastructure/runtime keys include:

- `http.route_worker_busy`: the device is temporarily busy processing configuration or diagnostic work; retry later.
- `http.route_worker_memory_low`: the device does not have enough memory headroom to start the requested route worker task.
- `runtime.config_blocked_by_voice`: configuration activity is rejected while realtime voice is active; retry after the reported delay.

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

**GET /api/config/system**

Purpose: read the current system segment.

Auth: `Pairing code`

Success response: `200 application/json`

Fields:

- `wifi_ssid`
- `wifi_pass`
- `proxy_url`
- `locale`
  Only `zh` and `en` are accepted. Invalid values now return `400 application/json` instead of being silently ignored.

**GET /api/config/llm**

Purpose: read only the LLM config segment, so the AI settings page does not need to fetch the full config payload.

Auth: `Pairing code`

Success response: `200 application/json`

Fields:

- `llm_sources`
- `llm_router_source_index`
- `llm_worker_source_index`

This route does not return `locale`, `build_package`, or any other config segments.

**POST /api/config/system**

Purpose: save the system segment.

Auth: `Pairing code + CSRF`

Request body: `application/json`

Fields:

- `wifi_ssid`
- `wifi_pass`
- `proxy_url`
- `locale`

The system segment does not accept channel fields. Save `tg_group_activation` through `POST /api/config/channels`.

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

**GET /api/config/channels**

Purpose: read chat-channel settings plus the channel catalog visible in the current build.

Auth: `Pairing code`

Success response: `200 application/json`

```json
{
  "available_channels": ["telegram", "qq_channel"],
  "unavailable_enabled_channel": "wecom",
  "enabled_channel": "wecom",
  "tg_token": "",
  "tg_allowed_chat_ids": "",
  "tg_group_activation": "mention",
  "feishu_app_id": "",
  "feishu_app_secret": "",
  "feishu_allowed_chat_ids": "",
  "dingtalk_client_id": "",
  "dingtalk_client_secret": "",
  "wecom_bot_id": "",
  "wecom_bot_secret": "",
  "wecom_ws_url": "",
  "qq_channel_app_id": "",
  "qq_channel_secret": "",
  "webhook_enabled": false,
  "webhook_token": ""
}
```

Response notes:

- `available_channels` lists the channel IDs compiled into this firmware or binary.
- `unavailable_enabled_channel` is present only when the saved `enabled_channel` is not compiled into the current build.
- The remaining fields are the flattened channels segment stored in `config/channels.json`.

**POST /api/config/channels**

Purpose: save chat-channel settings.

Auth: `Pairing code + CSRF`

Request body: `application/json`

Field groups:

- Common: `enabled_channel`
- Telegram: `tg_token`, `tg_allowed_chat_ids`, `tg_group_activation`
- Feishu: `feishu_app_id`, `feishu_app_secret`, `feishu_allowed_chat_ids`
- DingTalk: `dingtalk_client_id`, `dingtalk_client_secret`
- WeCom: `wecom_bot_id`, `wecom_bot_secret`, `wecom_ws_url`
- QQ Channel: `qq_channel_app_id`, `qq_channel_secret`
- Custom webhook: `webhook_enabled`, `webhook_token`

Field notes:

- `dingtalk_client_id` / `dingtalk_client_secret`: DingTalk Stream Mode connection credentials for subscribing to `/v1.0/im/bot/messages/get`.
- `wecom_bot_id` / `wecom_bot_secret`: WeCom AI Bot long-connection credentials.
- `wecom_ws_url`: WeCom AI Bot WebSocket URL; when empty, Beetle uses `wss://openws.work.weixin.qq.com`.
- Legacy social-platform HTTP callback / platform custom-robot fields have been removed from the config model; same-named unknown keys in old config files are ignored. User-owned `POST /api/webhook` remains controlled by `webhook_enabled` / `webhook_token`.

Save semantics:

- The server validates and writes `config/channels.json`; `tg_group_activation` belongs to the channels segment and is saved together with the Telegram channel fields.

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

Notes:

- Normal users should start from Configure UI: **Device Config -> GPIO Devices** or **Device Config -> I2C Sensors**
- Both pages still read and write the same `/api/config/hardware` route and `HardwareSegment`
- This section is the raw contract for scripts, custom frontends, and advanced integrations

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

Generic AHT20 example:

```json
{
  "hardware_devices": [],
  "i2c_bus": {
    "sda_pin": 21,
    "scl_pin": 22,
    "freq_hz": 100000
  },
  "i2c_devices": [],
  "i2c_sensors": [
    {
      "id": "aht20_env",
      "addr": 56,
      "model": "aht20",
      "what": "AHT20 temperature and humidity sensor",
      "how": "Read ambient temperature and humidity over I2C at address 0x38.",
      "options": {}
    }
  ]
}
```

Success response: `200 application/json`

```json
{
  "ok": true
}
```

For the user-facing setup flow, see [Hardware device config](hardware-device-config.md). The raw field contract is defined by this page and the API response itself.

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
- `wake_word` (currently Beetle's built-in acoustic wake backend; external wake backends are future extension points and are not configured in this round)
- `speech`
- `tts`
- `realtime`
- `ambient_listening`
- `led_indicator`

`wake_word` currently keeps the top-level object name for compatibility. Its current shape is:
`enabled`, `enter_threshold`, `leave_threshold`, `reference_suppress_ratio`, `zcr_min`, `zcr_max`, `min_speech_band_ratio`, `min_active_ms`, `hangover_ms`, `cooldown_ms`, `keyword` (legacy read-only compatibility), and `wake_prompt`. The configure-ui only exposes the acoustic parameters and `wake_prompt`.

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
- `display_name_key`
- `capabilities`
- `account_fields`
- `config_fields`

Notes:

- `display_name_key` is the provider-name translation key.
- Display semantics inside `account_fields` and `config_fields` use `label_key`, `description_key`, and option `label_key`.
- Product APIs no longer return system-generated `display_name`, `label`, or `description` prose.

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
- `display_name_key`
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

The request uses the public flat upsert contract and no longer accepts legacy nested `account` / `credential` / `config` wrappers:

- `provider_kind`
- `provider`
- `capability`
- `identity_class`
- `account_label`
- `display_name`
- `external_account_id`
- `email`
- `account_id`
- `username`
- `password`
- `access_token`
- `refresh_token`
- `token_endpoint`
- `mail_username`
- `mail_from_address`
- `imap_host`
- `imap_port`
- `imap_tls`
- `smtp_host`
- `smtp_port`
- `smtp_tls`
- `metadata`

Notes:

- `provider` is a public alias for `provider_kind`.
- `display_name` / `label` are accepted only as input aliases during normalization; they are not product-response contract fields.
- Provider-specific factual fields can be sent directly at the top level or inside `metadata`; the server normalizes them into provider config fields.

Success response: `200 application/json`

The response is the account detail object.

Account summary/detail display semantics follow the same contract:

- `display_name_key` is the translation key for the provider/account display name.
- Product responses do not return system-generated `display_name`, `label`, or `description` prose.

If the request is missing user facts that must be supplied explicitly, the route returns `400` with a structured onboarding result, for example:

```json
{
  "disposition": "needs_user_facts",
  "reason": "missing_user_facts",
  "missing_fields": ["identity_class"],
  "missing_field_details": [
    {
      "key": "identity_class",
      "label_key": "accounts.identityLabel",
      "description_key": "accounts.identityDescription",
      "options": [
        { "value": "work", "label_key": "accounts.identity.work" }
      ]
    }
  ]
}
```

**GET /api/config/accounts/:account_key**

Purpose: read one account with editable fields.

Auth: `Pairing code`

Success response: `200 application/json`

Response structure:

- `account`
- `assessment`
- `fields`

Notes:

- `account.display_name_key` is the translation key for the provider/account display name.
- Display semantics in `fields` and `assessment.missing_field_details` use `label_key`, `description_key`, and option `label_key`.
- Product APIs do not return system-generated `display_name` / `label` / `description` prose.

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

If the failure comes from an upstream provider, the route returns `400` with:

- `error_key`
- optional `provider_kind`
- optional `error_stage`
- optional `upstream_status`
- optional `upstream_error`

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

## Sessions, skills, and maintenance

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

Embedded ESP note: `/api/tools` remains available after activation. It still requires the device to be paired, but it does not require a temporary diagnostic session.

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

### Observability API Layers

- `/api/health` is lightweight liveness for first-screen status and LEDs. Do not put resource diagnostics, workflow data, or full network snapshots here.
- `/api/resource` is the lightweight resource-pressure snapshot for default status polling. It is not the device-wide status endpoint and does not return health overview or detailed runtime internals.
- `/api/metrics` carries counters and recent latency values. Do not put heap/resource/network objects here.
- `/api/operator/status` is the explanation surface for humans and UI. It may aggregate several sources to explain why the device is in its current state, but it is not a machine-admission source.
- `/api/diagnose` returns active diagnosis results and suggestions. It returns diagnosis items, not a raw snapshot warehouse.

The old fields `health.wifi`, `health.network`, `health.workflow`, `resource.network`, and `resource.firmware_identity` have been removed with no compatibility layer. Custom frontends must not depend on diagnostic fields from `/api/health`, and must not treat `/api/resource` as the device-wide status or detailed diagnostic endpoint.

**GET /api/health**

Purpose: read lightweight liveness.

Auth: `Activated`

Success response: `200 application/json`

Top-level fields:

- `status`
- `network_status`
  - `stage`
  - `sta_connected`
  - `wall_clock_trusted`
- `last_error`
- `display`
- `audio`

**GET /api/operator/status**

Purpose: read the human/UI operator explanation status.

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

Purpose: read counters and recent latency values.

Auth: `Activated`

Success response: `200 application/json`

The response is a metrics object. Common fields include:

- `messages_in` (user/external inbound messages only)
- `agent_messages_in` (all agent-plane messages, including internal system work)
- `system_messages_in` (internal system work consumed by the agent plane)
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

Purpose: read the lightweight resource-pressure snapshot.

Auth: `Activated`

Success response: `200 application/json`

Top-level fields:

- `pressure`
- `tls_fragmentation_risk`
- `storage_contention_risk`
- `heap_free_internal`
- `heap_min_free_internal`
- `heap_free_spiram`
- `heap_total_spiram`
- `heap_min_free_spiram`
- `heap_largest_block_spiram`
- `heap_used_spiram_est`
- `heap_largest_block_internal`
- `active_http_count`
- `active_wss_count`
- `active_agent_tasks`
- `inbound_depth`
- `outbound_depth`
- `budget`
- `governance_metrics`
- `session_count`
- `storage_used_kb`
- `storage_total_kb`
- `cpu_usage_percent` (Linux/host-only)
- `load_average` (Linux/host-only)
- `process_memory_kb` (Linux/host-only)

The resource endpoint is a default polling contract and is intentionally lightweight. `heap_free_spiram` is free PSRAM, not used PSRAM; `heap_used_spiram_est = heap_total_spiram - heap_free_spiram` is only an interpretation aid. `governance_metrics` carries compact pressure and rejection counters. Clients should tolerate unknown fields, but should not expect health overview, detailed runtime internals, crash evidence, or firmware identity here.

**GET /api/diagnose**

Purpose: read active diagnosis results and suggestions.

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
- `locale`
- `lan_ip`
- `programmable_reasoning`
- `storage_media`

**GET /api/channel_connectivity**

Purpose: read current channel connectivity. On ESP, the default response is a passive snapshot and does not run an external live probe; explicit refresh is handled by `POST /api/channel_connectivity/refresh`.

Auth: `Activated`

Success response: `200 application/json`

Top-level field:

- `channels`
- `checked_at_unix_secs`
- `stale`

`channels` item fields:

- `id`: channel ID.
- `configured`: whether the channel is configured.
- `ok`: whether the latest explicit connectivity probe succeeded.
- `message_key`: frontend-translatable status or error key.
- `runtime_status`: runtime status, for example `connected`, `connecting`, `waiting_network`, or `disabled`.
- `runtime_reason`: runtime reason key, nullable.

Display guidance: when `runtime_status=connected`, a persistent WSS/message channel can be shown as online. `stale=true` or `message_key=network.channel_connectivity_unavailable` only means the passive snapshot has no live probe result and should not override a connected runtime state.

**POST /api/operator/window**

Purpose: open a temporary maintenance window so protected maintenance routes can be accessed.

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

The route returns `200` only after the pairing code is cleared and reset-owned config/runtime files are removed successfully.
After the reset succeeds, subsequent control-plane reads reflect the default config immediately.

Auth: `Pairing code + CSRF`

Success response: `200 application/json`

```json
{
  "ok": true
}
```

Firmware update note:

Current Beetle mainline carries a large feature set and firmware package. We cannot keep the current functionality and user experience while also providing official OTA upgrade support. If you need OTA, you can slim the feature set, redesign the partition table, or contact us for a custom solution.

The supported mainline update paths are browser USB flashing, serial flashing, and factory reflash.

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

**Social-channel transport**

- Feishu: inbound uses event-subscription long connection; outbound uses Feishu IM OpenAPI.
- DingTalk: inbound uses Stream Mode WSS; outbound only uses active-session `sessionWebhook`.
- WeCom: inbound and outbound use AI Bot WSS, defaulting to `wss://openws.work.weixin.qq.com`.
- QQ: inbound uses Gateway WSS; group/C2C replies still require an existing passive-reply `msg_id`.
- Telegram: inbound uses `getUpdates` long polling; polling startup calls `deleteWebhook(drop_pending_updates=false)` to clear remote webhook configuration.
