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

### `task`

- `task` supports optional calendar sync.
- It uses the local calendar by default. If office calendar accounts are already configured, you can also point a task at a remote calendar provider when creating or updating it.
- If more than one remote calendar account is available for that provider, pass `calendar_account_key` explicitly or set a default office account first.
- When a task is completed, the linked calendar item is closed with it. If you delete the task or clear calendar sync, the linked calendar item is removed as well.

### `calendar`

- Uses the local calendar by default.
- After you link external calendar accounts, the same tool can also list, create, update, and delete remote events.
- External calendar accounts can now come from either CalDAV or Feishu Calendar.
- If more than one external account is available, pass `account_key` explicitly or set a default office account first.
- A Feishu calendar account is meant for direct team-calendar access, but you still keep using the same `calendar` tool for the actual event work.
- `provider_status` shows which calendar accounts are available, which one is the default, and whether each account is ready, still needs setup, or recently failed.

### `mail`

- `mail` is the external office mail capability, not a local mailbox implementation.
- Supported operations:
  - `provider_status`
  - `list`
  - `search`
  - `get`
  - `send`
  - `draft`
  - `reply`
  - `forward`
- If a default mail account is configured, or only one mail account is available, you can omit `provider` / `account_key`.
- `search` finds messages in a mailbox by keyword and returns ids you can pass straight into follow-up actions like `get`, `reply`, or `forward`.
- `send`, `draft`, `reply`, and `forward` are explicit remote mutations and require `confirm=true`.
- `send`, `draft`, and `forward` can use direct email arrays (`to` / `cc` / `bcc`) and contact-query arrays (`to_lookup` / `cc_lookup` / `bcc_lookup`) resolved through `contacts_directory`; `reply` keeps the original message sender as the base recipient and can still merge extra recipients.
- `provider_status` shows whether each mail account is ready to use and whether it has recently had connection or send problems.
- Mail accounts can now use generic `imap_smtp`, dedicated `feishu_mail`, or dedicated `wecom_mail`; all three follow the same mail tool contract.
- If `mail` cannot run because an account is incomplete, credentials no longer work, or the latest connection failed, the result now explains that directly.

### `documents`

- `documents` is the external office document-library capability. It does not replace the local/public document-reading tools.
- Documents accounts can now come from WebDAV, Feishu document libraries, or WeCom Wedrive libraries.
- Supported operations:
  - `provider_status`
  - `list`
  - `read`
  - `summarize`
  - `search`
- If a default documents account is configured, or only one documents account is available, you can omit `provider` / `account_key`.
- A Feishu documents account works well when you share one Feishu folder with Beetle; a WeCom documents account works the same way once you provide the Wedrive `space_id` plus the shared root folder id.
- `provider_status` shows which document-library accounts are available, which one is the default, and whether each account is ready to use.
- `summarize` turns a document into a short brief with key points, action items, and handoff content you can reuse in mail or task follow-up.
- If `documents` cannot run because an account is incomplete, credentials no longer work, or the latest connection failed, the result now explains that directly.

### `contacts_directory`

- `contacts_directory` remains Beetle's unified people-support layer; local contacts and connected office directories now feed into the same people lookup surface.
- Supported operations:
  - `status`
  - `provider_status`
  - `list`
  - `lookup`
  - `upsert`
  - `delete`
- Use it to persist stable person data such as names, emails, aliases, organizations, and short notes.
- If a Feishu or WeCom contacts account is connected, `lookup` can supplement local contacts with directory matches; you can also pass `provider` / `account_key` to target a specific directory account.
- `provider_status` shows which contacts-directory accounts are available, which one is the default, and whether each account is ready to use.
- `mail send` already consumes this shared people lookup through `*_lookup` recipient fields; later calendar attendee routing should reuse the same layer instead of inventing a separate contact model.

### `office_config`

- This is the unified office configuration tool, not a private control plane for one service.
- Supported operations:
  - `inspect`
  - `assess`
  - `provider_schema`
  - `resolve_account`
  - `draft_accounts`
  - `draft_credentials`
  - `validate_accounts`
  - `validate_credentials`
  - `commit_accounts`
  - `commit_credentials`
  - `revoke`
  - `probe`
- `provider_schema` is the backend truth source for provider onboarding. It returns the structured field contract for a specific `provider_kind`, or for every provider under one `capability`.
- `draft_*` / `validate_*` work on structured drafts only and do not write state.
- `draft_credentials` / `validate_credentials` / `commit_credentials` are now schema-driven: they trim credential values, apply provider defaults, reject metadata keys outside the provider contract, and block missing required fields before write.
- `commit_*` and `revoke` are explicit write actions and require `confirm=true`.
- `assess` and `office_status` now include `missing_field_details`, so callers do not need to guess raw metadata keys.
- `probe` reports only real status. Missing config, unsupported probe paths, or current unavailability are returned explicitly.

### `office_status`

- Reads the unified office state instead of any one tool's private status.
- Use it to inspect accounts, defaults, whether credentials exist, and the latest runtime status.
- It now directly tells you whether each account is ready, missing sign-in info, needs a connection check, or recently failed.
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
