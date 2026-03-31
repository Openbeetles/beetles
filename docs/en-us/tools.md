# Agent tools

[中文](../zh-cn/tools.md) | **English** | [Doc index](../README.md)

Tools the on-device **Agent may invoke automatically** during chat (you do not call them manually). On failure, the Agent explains in plain language.

Authoritative registration is [`build_default_registry`](../../src/tools/registry.rs). Below we group by **Cargo features** and **runtime config**; default features are in the repo root [`Cargo.toml`](../../Cargo.toml) (currently **`tools_diagnostics`** and **`tools_network_extra`** are default-on).

---

## Always registered (no feature gate)

| Tool | Summary | When the Agent might use it |
|------|---------|----------------------------|
| **get_time** | Current UTC time (date, weekday, time). | “What time is it?”, dates. |
| **env** | Process environment: `get` / `list`. | Debugging or reading runtime env (avoid leaking secrets into chat). |
| **files** | List or **read** files under storage root; no `..`. | List/read skills, notes, etc. (read-only). |
| **remind_at** | Schedule a reminder (ISO8601 or Unix seconds + text); fires on the same channel. | “Remind me at …”. |
| **remind_list** | Upcoming reminders for the current chat (optional limit). | “What reminders did I set?”. |
| **board_info** | Chip, heap/PSRAM, uptime, pressure, WiFi, SPIFFS, etc. | “Device status”, memory, storage. |
| **kv_store** | Persistent KV: `get`/`set`/`delete`/`list_keys`; caps on keys/values/count. | “Remember …”, “what keys are stored?”. |
| **file_write** | **Write** under storage root (overwrite/append); **protected paths** (e.g. `config/llm.json`, `config/SOUL.md`) cannot be written. | User notes and other non-protected paths. |

---

## Requires Cargo feature `tools_network_extra`

If this feature is disabled, these tools are **not** registered.

| Tool | Summary | When the Agent might use it |
|------|---------|----------------------------|
| **web_search** | Web search with a short summary. | Recent facts, “search for …”. |
| **analyze_image** | Vision model over an image URL. | Describe what’s in a linked image. |
| **http_request** | HTTP **GET/POST/PUT/DELETE/PATCH** with optional headers/body. **Private/internal URLs are blocked** (SSRF). | Public APIs, webhooks, integrations. |
| **proxy_config** | Get/set/clear HTTP proxy in NVS; **effective after reboot**. | Change proxy when allowed. |
| **model_config** | Read/update model-related fields in `config/llm.json` (**api_key not shown**); **effective after reboot**. | Switch model/URL when allowed. |

---

## Requires Cargo feature `tools_diagnostics`

If this feature is disabled, these tools are **not** registered (hardware/I2C tools also need config as below).

| Tool | Summary | When the Agent might use it |
|------|---------|----------------------------|
| **memory_manage** | Long-term memory, soul/user text, daily notes: `get_memory`/`set_memory`, soul/user ops, daily note CRUD, etc. | Managing memory and notes (distinct from config-UI SOUL/USER flows). |
| **session_manage** | Sessions: `list`/`info`/`clear`/`delete`. | Inspect or clear session history. |
| **system_control** | `restart` (needs `confirm=true`), `spiffs_usage`. | Restart, storage usage (dangerous ops need confirmation). |
| **cron_manage** | Persistent scheduled tasks (cron + action); evaluated by the device cron loop. | Recurring automated messages. |
| **network_scan** | `wifi_scan`, `wifi_status`, `connectivity_check`; scans are **rate-limited**. | WiFi / basic connectivity checks. |

### Conditional (with `tools_diagnostics` enabled)

| Tool | When registered | Summary |
|------|-----------------|---------|
| **device_control** | Non-empty `hardware_devices` | GPIO/PWM/ADC/buzzer by configured `device_id`; see [Hardware device config](hardware-device-config.md). |
| **sensor_watch** | Non-empty `hardware_devices` **or** non-empty `i2c_sensors` | Threshold watches: `add`/`list`/`remove`/`update`; tied to the cron loop. |
| **i2c_device** | `i2c_bus` set and non-empty `i2c_devices` | I2C register read/write per configured devices. |
| **i2c_sensor** | `i2c_bus` set and non-empty `i2c_sensors` | I2C sensor reads (works with `sensor_watch`). |

---

## Audio: voice I/O (conditional)

When `config/audio.json` exists, **`audio.enabled`**, and Baidu STT/TTS credentials plus mic/speaker checks in [`registry.rs`](../../src/tools/registry.rs) pass:

| Tool | Rough condition |
|------|-----------------|
| **voice_input** | `stt.provider == "baidu"`, STT key/secret set, `microphone.enabled` |
| **voice_output** | `tts.provider == "baidu"` with same credential path, `speaker.enabled` |

---

## Host-only (non-ESP targets, e.g. Linux)

These are **not** registered on **`xtensa` / `riscv32`** firmware builds:

| Tool | Summary |
|------|---------|
| **shell** | Restricted shell execution (host debugging). |
| **process** | Process-related operations. |
| **network** | Host network diagnostics. |

---

## Limits and behavior

- **Time**: Accurate after NTP/RTC sync; use **get_time** to verify.
- **files**: Read-only; paths must stay under the storage root; list/read limits apply (see code constants).
- **Reminders**: Stored on device, capped count; delivered on the **same channel/session**.
- **Network tools**: May be deferred under resource pressure (orchestrator gating).
- **http_request**: **RFC1918 / local targets are rejected**—do not use for LAN probing.
- **GET /api/tools** may **not** list every registered tool; trust the runtime registry and this page.

For JSON-driven onboard hardware and `device_control`, see [Hardware device config](hardware-device-config.md) and the hardware section of the config UI.
