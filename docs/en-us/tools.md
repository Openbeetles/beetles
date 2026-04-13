# Tools

[中文](../zh-cn/tools.md) | **English** | [Doc index](../README.md)

This page lists the tools Beetle Agent OS can use.

Keep three things in mind:

- users do not manually type tool names in normal chat
- the model may call these tools automatically
- the visible tool set depends on target, enabled features, and config

## Base Tools

| Tool | What it is for |
|------|----------------|
| `get_time` | current UTC time |
| `env` | read environment values for config and debugging |
| `message` | send an outbound message managed by Beetle |
| `task` | persistent task management |
| `calendar` | persistent calendar events |
| `mail` | read, inspect, and send mail through linked office accounts |
| `documents` | browse and read linked office document libraries |
| `contacts_directory` | local people directory for agent lookup and later office composition |
| `office_config` | inspect, draft, validate, commit, and revoke office config |
| `office_status` | view office accounts, defaults, credential presence, and runtime status |
| `files` | list or read files in device storage |
| `file_edit` | patch an existing text file in device storage |
| `remind_at` | create a reminder |
| `remind_list` | list reminders in the current chat |
| `board_info` | device model, uptime, WiFi, storage, and other basic device info |
| `kv_store` | persistent key-value storage |
| `private_garden` | current-chat private workspace |
| `memory_search` | search archive evidence from transcripts, daily notes, and turn logs |
| `memory_get` | fetch one archive evidence record |
| `factual_memory` | read stable facts saved on the device |
| `continuity_snapshot` | export, save, list, or import continuity data |
| `file_write` | write or append to allowed device files |

## Key Notes

### `calendar`

- Uses the local calendar by default.
- After you link external calendar accounts, the same tool can also list, create, update, and delete remote events.
- If more than one external account is available, pass `account_key` explicitly or set a default office account first.
- `provider_status` returns the current calendar capability view, configured account status, and default-account/runtime summary.

### `mail`

- `mail` is the external office mail capability, not a local mailbox implementation.
- Supported operations:
  - `provider_status`
  - `list`
  - `get`
  - `send`
- If a default mail account is configured, or only one mail account is available, you can omit `provider` / `account_key`.
- `send` is an explicit outbound action and requires `confirm=true`.
- `send` can use direct email arrays (`to` / `cc` / `bcc`) and contact-query arrays (`to_lookup` / `cc_lookup` / `bcc_lookup`) resolved through `contacts_directory`.
- `provider_status` returns the current mail capability view, configured account status, and default-account/runtime summary.

### `documents`

- `documents` is the external office document-library capability. It does not replace the local/public document-reading tools.
- Supported operations:
  - `provider_status`
  - `list`
  - `read`
  - `search`
- If a default documents account is configured, or only one documents account is available, you can omit `provider` / `account_key`.
- `provider_status` returns the current documents capability view, configured account status, and default-account/runtime summary.

### `contacts_directory`

- `contacts_directory` is the local people-support layer, not a shell around an external directory provider.
- Supported operations:
  - `status`
  - `list`
  - `lookup`
  - `upsert`
  - `delete`
- Use it to persist stable person data such as names, emails, aliases, organizations, and short notes.
- `mail send` already consumes this shared people lookup through `*_lookup` recipient fields; later calendar attendee routing should reuse the same layer instead of inventing a separate contact model.

### `office_config`

- This is the unified office configuration tool, not a private control plane for one service.
- Supported operations:
  - `inspect`
  - `resolve_account`
  - `draft_accounts`
  - `draft_credentials`
  - `validate_accounts`
  - `validate_credentials`
  - `commit_accounts`
  - `commit_credentials`
  - `revoke`
  - `probe`
- `draft_*` / `validate_*` work on structured drafts only and do not write state.
- `commit_*` and `revoke` are explicit write actions and require `confirm=true`.
- `probe` reports only real status. Missing config, unsupported probe paths, or current unavailability are returned explicitly.

### `office_status`

- Reads the unified office state instead of any one tool's private status.
- Use it to inspect accounts, defaults, whether credentials exist, and the latest runtime status.
- Optional `capability` lets you scope the result to `calendar`, `mail`, `documents`, or `contacts_directory`.

## Tools Behind `tools_network_extra`

| Tool | What it is for |
|------|----------------|
| `web_search` | web search |
| `analyze_image` | vision analysis for an image URL |
| `http_request` | public HTTP requests |
| `proxy_config` | read/write proxy config |
| `model_config` | read/write model config fields |

## Extra Non-ESP Network Tools

These appear only on non-ESP builds:

| Tool | What it is for |
|------|----------------|
| `document_search` | search stored documents |
| `document_read` | read a public URL or stored document |
| `document_extract` | extract lines, sections, or specific fields |
| `web_fetch` | fetch a public web page as readable text |
| `pdf_read` | fetch and read a public PDF |

## Tools Behind `tools_diagnostics`

| Tool | What it is for |
|------|----------------|
| `memory_manage` | manage long-term memory and related text stores |
| `session_manage` | list, inspect, clear, or delete sessions |
| `system_control` | restart and storage-related system actions |
| `cron_manage` | persistent scheduled tasks |
| `network_scan` | WiFi and connectivity checks |

With diagnostics enabled, these appear only when the related hardware/config is present:

| Tool | When it appears |
|------|------------------|
| `device_control` | when `hardware_devices` is configured |
| `sensor_watch` | when watchable hardware or I2C sensors exist |
| `i2c_device` | when I2C bus and I2C devices are configured |
| `i2c_sensor` | when I2C bus and I2C sensors are configured |

## Audio Tools

These appear only when audio config and required credentials are present:

| Tool | What it is for |
|------|----------------|
| `voice_input` | speech-to-text |
| `voice_output` | text-to-speech |

## Linux / Non-ESP Utilities

These are available only on non-ESP builds:

| Tool | What it is for |
|------|----------------|
| `shell` | restricted shell execution |
| `process` | local process operations |
| `network` | local network diagnostics |

## Usage Notes

- `files` is read-only; `file_write` and `file_edit` are limited to allowed mutable paths.
- `private_garden` is scoped to the current chat.
- `memory_search` and `memory_get` return historical material, not necessarily the final answer.
- `factual_memory` is better for stable, already-confirmed information.
- `continuity_snapshot` is mainly for backup, migration, and restore.
- `http_request`, `web_fetch`, and `pdf_read` reject private/internal targets.

Related docs:

- [hardware-device-config.md](hardware-device-config.md)
- [config-api.md](config-api.md)
