# HTTP Config API

[中文](../zh-cn/config-api.md) | **English** | [Doc index](../README.md)

This page is for developers calling Beetle over HTTP.

Use it when you need:

- the pairing and CSRF rules
- the exact read/write API behavior
- the endpoint list for your own frontend, script, or integration

If you only want to bring up a device and use the built-in configuration flow, read [configuration.md](configuration.md) first.

## Network and access

- **SoftAP**: After power-on the device starts a hotspot; SSID is fixed as **Beetle** (no password). When connected to this hotspot, use **http://192.168.4.1** (matches firmware).
- **STA**: If WiFi is configured and the device is connected to the user’s router, the same HTTP service is reachable on the STA network; use the LAN address assigned by the router.
- **CORS**: All responses for `/api/*` and `GET /` must include `Access-Control-Allow-Origin: *`. OPTIONS returns 200 with `Access-Control-Allow-Methods: GET, POST, DELETE, OPTIONS` and `Access-Control-Allow-Headers: Content-Type, X-Pairing-Code, X-CSRF-Token` (case-insensitive matches apply in practice), so the external config UI can call the API cross-origin.

## Pairing code and auth

### Terms

- **Not activated**: No valid 6-digit pairing code stored in NVS.
- **Activated**: `POST /api/pairing_code` has succeeded at least once.
- **Activated only** (`require_activated`): The device must already be activated, but the request itself does **not** need `?code=` or `X-Pairing-Code`.
- **Writes**: After activation, state-changing APIs normally require **pairing code + CSRF**. The exact list and exceptions are below.

### Allowed when not activated

- Any **OPTIONS**.
- **GET /**: **302** to the pairing flow (`Location: /pairing`); no JSON list.
- **GET /wifi**: Built-in config page HTML.
- **GET /pairing**, **GET /common.css**, **GET /common.js**.
- **GET /api/pairing_code**, **POST /api/pairing_code** (initial pairing only).
- **GET /api/wifi/scan**, **GET /api/csrf_token** (callable before activation).
- Channel callbacks: **POST /api/feishu/event**, **POST /api/dingtalk/webhook**, **GET/POST /api/wecom/webhook**, **POST /api/webhook/qq** (QQ may 404 if disabled).

Other paths return **401** when not activated (wording may vary by locale).

### Read-only APIs after activation (no pairing code in request)

These require **activation** but **not** `?code=` or `X-Pairing-Code` (this is what “no pairing code” means in user docs: **not** “works before activation”):

**GET /**, **GET /api/config**, **GET /api/config/hardware**, **GET /api/config/audio**, **GET /api/config/display**, **GET /api/health**, **GET /api/metrics**, **GET /api/resource**, **GET /api/tools**, **GET /api/diagnose**, **GET /api/system_info**, **GET /api/channel_connectivity**, **GET /api/sessions**, **GET /api/memory/status**, **GET /api/skills**, **GET /api/soul**, **GET /api/user**; with `ota` feature, **GET /api/ota/check**.

### Writes: pairing code + CSRF

After activation, these **POST**/**DELETE** calls also need:

1. **Pairing code**: `?code=<6 digits>` and/or `X-Pairing-Code: <6 digits>`.
2. **CSRF**: Header `X-CSRF-Token` or `x-csrf-token` with the `csrf_token` value from **GET /api/csrf_token**; missing/invalid → **403**.

Includes: `POST /api/config/wifi`, `/api/config/llm`, `/api/config/channels`, `/api/config/system`, `/api/config/hardware`, `/api/config/audio`, `/api/config/display`; **POST/DELETE /api/skills**, **POST /api/skills/import**; **POST /api/soul**, **POST /api/user**; **DELETE /api/sessions**; **POST /api/restart**, **POST /api/config_reset**, **POST /api/webhook**; **POST /api/ota** (if built with `ota`).

**Exceptions**: **POST /api/pairing_code** (not activated only) needs neither; **channel webhooks** use platform-specific verification, not pairing/CSRF.

### GET /api/csrf_token

- **Auth**: None.
- **Response**: 200, `{"csrf_token":"<token>"}`.

### Factory reset

**POST /api/config_reset** requires pairing code + CSRF; on success the device is not activated again (implementation removes SPIFFS files such as `config/skills_meta.json`, `config/llm.json`, `config/channels.json`, `config/hardware.json`, `config/audio.json`, `config/display.json`—see [`config_reset` handler](../../src/platform/http_server/handlers/config_reset.rs)).

---

## Root and discovery

### GET /

- **Not activated**: **302**, `Location: /pairing`.
- **Activated**: 200 JSON; `name` is **`beetle`**, `version` is firmware version, `endpoints` is a string array.
- **Note**: `endpoints` is useful for discovery, but do not treat it as the only API list. Use the routes documented on this page when building integrations.

**Example** (exact fields and order depend on the device build):

```json
{
  "name": "beetle",
  "version": "0.1.0",
  "endpoints": ["GET /pairing", "GET /wifi", "GET /api/pairing_code"]
}
```

### GET /api/pairing_code

- **Purpose**: Whether a code is set (never returns the secret) and current UI locale.
- **Response**: 200, `{"code_set":true|false,"locale":"zh"|"en"}` (locale per firmware).
- **Auth**: Callable when not activated.

### POST /api/pairing_code

- **Purpose**: Set the 6-digit code once (not activated only).
- **Request**: `Content-Type: application/json`, body `{"code": "123456"}`.
- **Response**: 200 `{"ok": true}`; 400 if already set or invalid.
- **Auth**: No pairing/CSRF.

### GET /pairing

- **Purpose**: Pairing page HTML.
- **Response**: 200, `text/html; charset=utf-8`.

### GET /wifi

- **Purpose**: Built-in config page HTML.
- **Response**: 200, `text/html; charset=utf-8`.

## Config read/write

**Storage**: NVS holds only 6 small keys (WiFi SSID/password, proxy, session count, group trigger, UI locale). LLM multi-source and channel config are on SPIFFS (`config/llm.json`, `config/channels.json`); hardware device config on `config/hardware.json`; audio on `config/audio.json`; display on `config/display.json`; skill enable/order on `config/skills_meta.json`. GET /api/config merges NVS and SPIFFS and returns the full config.

### GET /api/wifi/scan

- **Purpose**: Device scans nearby WiFi and returns SSID list for the config UI dropdown.
- **Auth**: Works when not activated; request does **not** need a pairing code.
- **Response**: 200, JSON array `[{ "ssid": "MyWiFi", "rssi": -50 }, ...]`, sorted by signal strength (rssi) descending. 503 when scan is unavailable (non-ESP or WiFi not ready).

### GET /api/config

- **Purpose**: Get the current full config (real values for all fields, including secrets).
- **Auth**: Activated; GET does **not** need a pairing code in the request.
- **Response**: 200, JSON is `AppConfig` serialization; all fields are stored values.
- **Multi-LLM**: `llm_sources` is an array; each item has `provider`, `api_key`, `model`, `api_url`, optional `max_tokens` (u32; null means clients use built-in default 1024). When empty, load builds a single source from legacy fields. Optional **`llm_router_source_index`** and **`llm_worker_source_index`** (`u32`, indices into `llm_sources`) reorder the worker’s [`FallbackLlmClient`](../../src/llm/fallback.rs) chain: if a valid `llm_router_source_index` is set, that source is tried **first**, then (if set and valid) `llm_worker_source_index`, then remaining valid sources; if neither is set, order follows valid sources in list order. Global streaming: top-level `llm_stream` in the LLM segment.

### POST /api/config/llm

- **Purpose**: Write only the LLM segment (multi-source + global streaming flag) to SPIFFS (`config/llm.json`); request body is the full segment; backend validates and writes.
- **Auth**: Activated + pairing code + CSRF (same rules as **Writes: pairing code + CSRF** in this doc).
- **Request**: `Content-Type: application/json`, Body `{ "llm_sources": [...], "llm_stream": false, "llm_router_source_index": null, "llm_worker_source_index": null }` (last two optional; omit = null). `llm_sources` must be non-empty; each item must have `api_key`. Each item may include:
  - `provider` (required non-empty string; length checks in `config`. Runtime client selection: [`llm/mod.rs`](../../src/llm/mod.rs)—common values include `anthropic`, `openai`, `openai_compatible`, `gemini`, `glm`, `qwen`, `deepseek`, `moonshot`, `ollama`; see [LLM providers](llm-providers.md))
  - `api_key` (required)
  - `model` (required)
  - `api_url` (field required; if empty for OpenAI-compatible providers, clients use vendor default base URLs per `build_llm_clients`)
  - `max_tokens` (optional u32, default null; null means clients use built-in default 1024)
- **Validation**: This segment only—`llm_sources` non-empty; field lengths (provider/api_key/model ≤ 64, api_url ≤ 256); `llm_stream` boolean; optional indices must be in range for `llm_sources` (`validate_llm_source_indices` in `config`).
- **Response**: Success 200 `{"ok": true}`; validation failure 400.

### POST /api/config/channels

- **Purpose**: Write only the channels segment (Telegram, Feishu, DingTalk, WeCom, QQ Channel, Webhook) to SPIFFS (`config/channels.json`); request body is the full segment; backend validates and writes.
- **Auth**: Activated + pairing code + CSRF.
- **Request**: `Content-Type: application/json`, full channels segment as `ChannelsSegment` in firmware `config.rs`—includes the usual Telegram/Feishu/DingTalk/WeCom/QQ/webhook fields plus optional **`wecom_token`**, **`wecom_encoding_aes_key`**, **`dingtalk_app_secret`**, etc.
- **Validation**: This segment’s field lengths (tg/feishu/wecom/qq etc. ≤ 64, dingtalk_webhook_url ≤ 512, wecom_default_touser ≤ 128).
- **Response**: Success 200 `{"ok": true}`; validation failure 400.

### POST /api/config/system

- **Purpose**: Write only the system segment (WiFi, proxy, session count, group trigger, UI locale); request body is the full segment; backend validates and writes.
- **Auth**: Activated + pairing code + CSRF.
- **Request**: `Content-Type: application/json`, Body includes `wifi_ssid`, `wifi_pass`, `proxy_url`, `session_max_messages` (1–128), `tg_group_activation` (`"mention"` or `"always"`), optional `locale` (`"zh"` or `"en"`).
- **Validation**: This segment only—wifi field length ≤ 64; `proxy_url` empty or like `http://host:port`; `session_max_messages`, `tg_group_activation` as above.
- **Response**: Success 200 `{"ok": true}`; validation failure 400.
- **Note**: WiFi changes take effect after reboot.

### GET /api/config/hardware

- **Purpose**: Get current hardware device config segment (`config/hardware.json` content).
- **Auth**: Activated; GET does **not** need a pairing code in the request.
- **Response**: 200, JSON is `HardwareSegment`: `{ "hardware_devices": [...] }`. Returns `{ "hardware_devices": [] }` when the file does not exist.
- **Note**: GET returns the raw file content. If validation failed at boot, Beetle falls back to an empty device list and `load_errors` will include `hardware_validation_failed`.

### POST /api/config/hardware

- **Purpose**: Write the hardware device config segment to SPIFFS (`config/hardware.json`); request body is the full segment; backend validates and writes. Takes effect after reboot. If validation fails when loading after reboot, `load_errors` will include `hardware_validation_failed`; full validation rules in [Hardware device config & LLM-driven control](hardware-device-config.md).
- **Auth**: Activated + pairing code + CSRF.
- **Request**: `Content-Type: application/json`, Body is `HardwareSegment`:
  ```json
  {
    "hardware_devices": [
      {
        "id": "onboard_led",
        "device_type": "gpio_out",
        "pins": { "pin": 2 },
        "what": "Onboard LED indicator, toggleable",
        "how": "Pass value: 1=on, 0=off"
      }
    ]
  }
  ```
  Each `DeviceEntry` has `id`, `device_type`, `pins`, `what`, `how`, optional `options`.
- **Validation** (this segment only):
  - Total device count ≤ 8
  - `id` non-empty, ≤ 32 bytes, must be unique
  - `device_type` must be one of `gpio_out` / `gpio_in` / `pwm_out` / `adc_in` / `buzzer`
  - `what` ≤ 128 bytes, `how` ≤ 256 bytes
  - `pins` must have a `"pin"` key; pin value 1–48, must not be strapping pins (0, 3, 46), must not conflict across devices
  - `adc_in` pin must be in ADC1 range (GPIO 1–10)
  - `pwm_out` device count ≤ 4; `options.frequency_hz` if present must be 1–40000
- **Response**: Success 200 `{"ok": true}`; validation failure 400.

### GET /api/config/audio

- **Purpose**: Get current audio device config segment (`config/audio.json` content).
- **Auth**: Activated; GET does **not** need a pairing code in the request.
- **Response**: 200, JSON is `AudioSegment`. If the file does not exist, returns disabled defaults (`enabled=false`).

### POST /api/config/audio

- **Purpose**: Write the audio device config segment to SPIFFS (`config/audio.json`); request body is the full segment; backend validates and writes. Takes effect after reboot.
- **Auth**: Activated + pairing code + CSRF.
- **Request**: `Content-Type: application/json`, Body is `AudioSegment`, structure aligned with the `config/audio.json` example in [`voice-interaction-plan.md`](../../dev-docs/voice-interaction-plan.md) (`microphone`, `speaker`, `vad`, `wake_word`, `speech`, `tts`, `realtime`, `ambient_listening`, `led_indicator`).
- **Validation** (this segment only):
  - `version` must be `1`
  - For enabled microphone/speaker: pins in 1–48; sample rate 8000–48000; bits per sample in 16/24/32
  - microphone `buffer_size` must be 256–16384
  - When the wake-word path is enabled but realtime voice is not configured, the current fallback speech provider still requires its credentials; for `baidu`, both `speech.api_key` and `speech.api_secret` are required
  - `realtime.provider` currently supports only `openai_compatible`, `qwen`, and `doubao`
  - When realtime voice is enabled, `realtime.api_key`, `realtime.model`, `realtime.voice`, and `realtime.ws_url` are required, and both microphone/speaker sample rates must be `24000`
  - `vad.threshold` must be in [0,1], `silence_duration_ms` in 1–60000
  - `ambient_listening.sound_events` max 16 items, each 1–32 chars; `check_interval_seconds` in 1–86400
- **Response**: Success 200 `{"ok": true, "restart_required": true}`; validation failure 400.

### POST /api/config/wifi

- **Purpose**: Write only WiFi SSID/password to NVS for the “WiFi only” config flow.
- **Auth**: Activated + pairing code + CSRF.
- **Request**: `Content-Type: application/json`, Body `{"wifi_ssid":"...","wifi_pass":"..."}`; field length ≤ 64.
- **Response**: Success 200, `{"ok": true, "restart_required": true}`; validation failure 400.
- **Note**: After saving WiFi to NVS, device must restart to apply. Optional: send query `?restart=1` to trigger restart after save (same as `POST /api/restart`).

### GET /api/soul

- **Purpose**: Get current SOUL (persona) content for the external config UI to display or edit.
- **Auth**: Activated; GET does **not** need a pairing code in the request.
- **Response**: 200, `Content-Type: text/plain`, Body is SOUL file content (UTF-8); read failure 500, `{"error":"..."}`.

### POST /api/soul

- **Purpose**: Submit SOUL content and write to SPIFFS (config/SOUL.md).
- **Auth**: Activated + pairing code + CSRF.
- **Request**: Body is plain text or JSON `{"content": "..."}`; length ≤ 32KB (MAX_SOUL_USER_LEN).
- **Response**: Success 200, `{"ok": true}`; too long or invalid UTF-8 400; write failure 500.

### GET /api/user

- **Purpose**: Get current USER (user info) config content.
- **Auth**: Activated; GET does **not** need a pairing code in the request.
- **Response**: 200, `Content-Type: text/plain`, Body is USER file content; read failure 500.

### POST /api/user

- **Purpose**: Submit USER content and write to SPIFFS (config/USER.md).
- **Auth**: Activated + pairing code + CSRF.
- **Request**: Same as POST /api/soul (plain text or `{"content":"..."}`, ≤ 32KB).
- **Response**: Same as POST /api/soul.

### GET /api/sessions

- **Purpose**: Paginated chat_id list or recent messages for one session (read-only).
- **Auth**: Activated; GET does **not** need a pairing code in the request.
- **Query**:
  - Without `chat_id` / `name`: paginated list. **`page`** (default 1), **`limit`** (default 20, max 100). Response 200 JSON: `{"items":["id1",...],"total":N,"page":1,"limit":20,"total_pages":...}`.
  - With **`chat_id`** or **`name`** (either query key, value = session id): recent messages JSON for that session (~50 messages max; see [`sessions.rs`](../../src/platform/http_server/handlers/sessions.rs)). Failure 500, `{"error":"..."}`.

### GET /api/memory/status

- **Purpose**: Get byte counts for MEMORY, SOUL, USER (read-only).
- **Auth**: Activated; GET does **not** need a pairing code in the request.
- **Response**: 200, JSON `{"memory_len": number, "soul_len": number, "user_len": number}`.

### GET /api/tools

- **Purpose**: JSON array of `{name, description}` for HTTP discovery of tool names.
- **Auth**: Activated; GET does **not** need a pairing code.
- **Response**: 200, `[{"name":"get_time","description":"..."}, ...]`. Built in [`handlers/tools.rs`](../../src/platform/http_server/handlers/tools.rs); entries depend on Cargo features such as **`tools_network_extra`** / **`tools_diagnostics`** and may **not** match every tool loaded by Beetle from [`build_default_registry`](../../src/tools/registry.rs) (for example **env** and **file_write** may be missing). **Authoritative tool set**: the tool registry and [tools](tools.md).

## Skills

### GET /api/skills

- **Purpose**: List skills or get a single skill. Without query, returns list and order; with `?name=xxx` returns that skill’s plain text (for edit).
- **Auth**: Activated; GET does **not** need a pairing code in the request.
- **Response (no name)**: 200, JSON `{"skills": [{"name": "x", "enabled": true}, ...], "order": ["a", "b"]}`.
- **Response (name=xxx)**: 200, `Content-Type: text/plain`, Body is skill content; not found 404.

### POST /api/skills

- **Purpose**: Update enabled state, write content, or update order. Body shape determines behavior.
- **Auth**: Activated + pairing code + CSRF.
- **Request**: `Content-Type: application/json`.
  - Enable only: `{"name": "x", "enabled": true|false}`.
  - Write/overwrite skill: `{"name": "x", "content": "..."}`; content length ≤ 32KB.
  - Order only: `{"order": ["a", "b", "c"]}`.
- **Response**: Success 200, `{"ok": true}`; failure 400/500.

### DELETE /api/skills?name=xxx

- **Purpose**: Delete the given skill file. Query must include `name`.
- **Auth**: Activated + pairing code + CSRF.
- **Response**: Success 200, `{"ok": true}`; invalid name 400; file not found 404.

### POST /api/skills/import

- **Purpose**: Fetch content from URL and save as a new skill.
- **Auth**: Activated + pairing code + CSRF.
- **Request**: `Content-Type: application/json`, Body `{"url": "https://...", "name": "xxx"}`. url must be http(s); name must be valid (no `..`, `/`, `\`).
- **Response**: Success 200, `{"ok": true}`; url fetch failure 502/500; body not UTF-8 or too long 400.

### POST /api/webhook

- **Purpose**: External HTTP POST to inject one inbound message; body is used as content and pushed to the inbound queue for the agent.
- **Auth**: Activated + pairing code + CSRF; then the **webhook token** check in the same subsection.
- **Config**: Requires `webhook_enabled: true` and non-empty `webhook_token` in config; otherwise 403.
- **Token check**: After pairing and CSRF, the request must include the configured token: Header `X-Webhook-Token` or query `token`; mismatch returns 401.
- **Request**: Body is arbitrary UTF-8 text as inbound message content; max 4KB.
- **Response**:
  - Success: 200, `{"ok": true}`.
  - Webhook disabled or token empty: 403, `{"error": "webhook disabled"}`.
  - Token mismatch: 401, `{"error": "invalid token"}`.
  - Body not UTF-8 or too long: 400/413.
  - Inbound queue full: 503, `{"error": "queue full"}`.

## Health and ops

### GET /api/health

- **Purpose**: Structured health info (same spirit as CLI `health`), including nested `metrics` and `resource` (same orchestrator snapshot as `GET /api/resource`).
- **Auth**: Activated; GET does **not** need a pairing code in the request.
- **Response**: 200, JSON example (field names match serde structs; metric fields use `messages_in`, etc.):
  ```json
  {
    "wifi": "connected",
    "inbound_depth": 0,
    "outbound_depth": 0,
    "last_error": "none",
    "metrics": { "messages_in": 0, "messages_out": 0 },
    "resource": { "pressure": "Normal", "heap_largest_block_internal": 12345 }
  }
  ```
  - `wifi`: `"connected"` | `"disconnected"` (**STA associated with upstream AP / outbound path**; may still be `disconnected` when only the device SoftAP is used for provisioning).
  - `inbound_depth` / `outbound_depth`: Queue depths (numbers).
  - `last_error`: Last error summary (stage/message only, no secrets); or `"none"`.
  - `metrics`: `metrics::MetricsSnapshot` (throughput, LLM/tool, dispatch, per-stage error counts; on Linux embedded also `wifi_reconnect_total`, `wifi_ap_restart_total`, `wifi_last_failure_stage`).
  - `resource`: `orchestrator::ResourceSnapshot` (pressure, heap, queue depths, channel health, etc.); `pressure` is `"Normal"` | `"Cautious"` | `"Critical"`.
  - **Migration**: External scripts that parsed legacy flat keys such as `msg_in` should use `metrics.messages_in` instead.

### GET /api/diagnose

- **Purpose**: Device self-check (Doctor-style), returns a list of structured results for the config UI “Device status”.
- **Auth**: Activated; GET does **not** need a pairing code in the request.
- **Response**: 200, JSON array; each item has `severity`, `category`, `message`:
  ```json
  [
    { "severity": "ok", "category": "storage", "message": "storage readable" },
    { "severity": "ok", "category": "storage", "message": "spiffs total=... used=... free=..." },
    { "severity": "ok", "category": "config", "message": "nvs accessible" },
    { "severity": "warn", "category": "config", "message": "wifi disconnected" },
    { "severity": "ok", "category": "channel", "message": "inbound_depth=0 outbound_depth=0" },
    { "severity": "warn", "category": "channel", "message": "last_error: ..." }
  ]
  ```
  - `severity`: `"ok"` | `"warn"` | `"error"`.
  - `category`: `"storage"` (readable, SPIFFS) | `"channel"` (queue depth, last_error) | `"config"` (NVS, WiFi).
  - `message`: Human-readable; `last_error` summary truncated to 200 chars.

### GET /api/metrics

- **Purpose**: Metrics snapshot JSON (fields per firmware `metrics` module). Optional query **`format=prometheus`** returns Prometheus text (`Content-Type: text/plain`).
- **Auth**: Activated; GET does **not** need a pairing code in the request.

### GET /api/resource

- **Purpose**: Orchestrator resource snapshot JSON (same `resource` object as inside `GET /api/health`).
- **Auth**: Activated; GET does **not** need a pairing code in the request.

### GET /api/system_info

- **Purpose**: System/build info JSON.
- **Auth**: Activated; GET does **not** need a pairing code in the request.

### GET /api/channel_connectivity

- **Purpose**: Channel connectivity probe JSON.
- **Auth**: Activated; GET does **not** need a pairing code in the request.

### GET /api/config/display

- **Purpose**: Read display-related config segment JSON (when display subsystem is enabled).
- **Auth**: Activated; GET does **not** need a pairing code in the request.

### POST /api/config/display

- **Purpose**: Write display segment; may support `?restart=1` similar to WiFi segment where applicable.
- **Auth**: Activated + pairing code + CSRF.

### DELETE /api/sessions?chat_id=...

- **Purpose**: Delete one session.
- **Auth**: Activated + pairing code + CSRF; query **must** include `chat_id`.

### Channel callbacks (no pairing / no CSRF)

Called by vendor servers; each handler applies its own signature/token rules—**not** the pairing+CSRF flow above:

- **POST /api/feishu/event**
- **POST /api/dingtalk/webhook**
- **GET /api/wecom/webhook** (URL verification), **POST /api/wecom/webhook**
- **POST /api/webhook/qq** (404 if QQ channel not enabled)

### POST /api/restart

- **Purpose**: Trigger device restart so new config takes effect.
- **Auth**: Activated + pairing code + CSRF.
- **Response**: Returns 200, `{"ok": true}`, then device restarts within ~100–500ms.
- **Extra behavior**: Before the actual restart, Beetle tries to flush a continuity bundle for recent active chats into the state root so reboot/handoff recovery has a fresh runtime snapshot. A flush failure is logged but does not block restart.
- **Throttle**: Only one successful restart allowed within 60 seconds.

### GET /api/ota/check

- **Purpose**: Check for available firmware update for current board and channel (uses build-time `OTA_MANIFEST_URL` and manifest). Only present when the `ota` feature is enabled.
- **Auth**: Activated; GET does **not** need a pairing code in the request.
- **Request**: GET, optional query `channel` (default `stable`).
- **Response**: 200 JSON. Fields: `current_version` (current firmware), `latest_version` (latest for channel, if any), `update_available` (whether an update exists), `url` (download URL when update available), optional `release_notes`, optional `error` (human message, e.g. channel not configured or fetch failed). If manifest is missing or fetch/parse fails, still 200 with `update_available: false` and optional `error`.

### POST /api/ota

- **Purpose**: Fetch firmware from the given URL and perform OTA update; on success the device restarts. Only present when the firmware is built with the `ota` feature (GET / endpoints will include `"POST /api/ota"`).
- **Auth**: Activated + pairing code + CSRF.
- **Request**: `Content-Type: application/json`, Body `{"url": "https://..."}`; url must be non-empty and start with `http://` or `https://`.
- **Response**: Success 200, `{"ok": true}`, then device runs OTA and restarts; invalid or missing url 400, `{"error": "invalid url"}`; OTA download, verify, or write failure 500, `{"error": "human-readable message"}` (e.g. “Network or download failed, check network and retry”, “Firmware verification failed, try another source”, “Write failed, do not power off and retry”). Response includes CORS headers.
- **Note**: On failure the current running partition is not overwritten; device keeps running; caller can retry or use another URL.

**OTA channel manifest format** (produced by CI/Release and served at `OTA_MANIFEST_URL`): JSON root has `boards`; keys are board IDs (`esp32-s3-8mb`, `esp32-s3-16mb`, `esp32-s3-32mb`), values are channel objects; each channel (e.g. `stable`) has `version`, `url` (required), optional `release_notes`. Example: `{"boards":{"esp32-s3-16mb":{"stable":{"version":"0.2.0","url":"https://...","release_notes":"..."}}}}`. Board and manifest URL are set at build time via `BOARD`, `OTA_MANIFEST_URL`.

### POST /api/config_reset

- **Purpose**: Factory reset (clear NVS config area and remove SPIFFS `config/llm.json`, `config/channels.json`, `config/hardware.json`, `config/audio.json`, `config/display.json`, `config/skills_meta.json`, etc.); same as CLI `config_reset yes`.
- **Auth**: Activated + pairing code + CSRF.
- **Response**: Success 200, `{"ok": true}`; failure 500, `{"error": "reset failed"}`.
- **Note**: After calling, user should restart; after restart `AppConfig::load()` uses only env; NVS keeps the 6 small keys (wifi, proxy, session, tg_group, locale, etc.); the rest is on SPIFFS.

## How to get the device IP

- When connected to hotspot **Beetle**: use **http://192.168.4.1** (firmware SoftAP fixed address).
- When on STA and device is on the same LAN: use the IP assigned by the router to the device.

## Config UI ownership

Firmware embeds **`GET /wifi`** (config HTML), **`GET /pairing`**, **`GET /common.css`**, **`GET /common.js`**, etc. (see route tables above). A richer experience can still be built with this repo’s **`configure-ui`** or any static host talking to the same HTTP API.
