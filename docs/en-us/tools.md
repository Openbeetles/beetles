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
| `remind_at` | manage reminders |
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
- It uses the local calendar by default. If office calendars are already linked, Beetle can also sync a task into an external calendar.
- If you have more than one external calendar, Beetle will first try to infer the right one from context. If that still is not clear, it will ask which calendar you mean instead of expecting you to know internal identifiers.
- When a task is completed, the linked calendar item is closed with it. If you delete the task or clear calendar sync, the linked calendar item is removed as well.

### `calendar`

- Uses the local calendar by default.
- After you link external calendar accounts, the same tool can also list, create, update, and delete remote events.
- External calendar accounts can now come from CalDAV, Feishu Calendar, Microsoft 365 Calendar, or Google Calendar.
- If more than one external calendar is linked, Beetle will first try to infer whether you mean a work or personal calendar. If it still is not confident, it will ask.
- When you create or update a meeting around named people or teams, Beetle can reuse the shared people directory to decide which calendar side fits that context, instead of treating attendee lookup and calendar choice as separate tasks.
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
- If only one mailbox is linked, Beetle will just use it. If multiple mailboxes are linked, Beetle will first try to infer whether the action belongs to work or personal mail, and only ask when that still is ambiguous.
- When you send mail or save a draft using someone found through a remote work contacts directory, Beetle uses that signal while choosing the sender mailbox too. If the directory already makes the right work mail suite clear, Beetle can jump straight there instead of falling back to the default mailbox and asking again.
- `search` finds messages in a mailbox by keyword and returns ids you can pass straight into follow-up actions like `get`, `reply`, or `forward`.
- `send`, `draft`, `reply`, and `forward` are explicit remote mutations, so Beetle will ask for clear confirmation before it performs them.
- When sending mail, you can give direct email addresses or simply name the person and let Beetle look them up through the contacts directory. `reply` keeps the original sender as the base recipient and can still merge extra recipients.
- `provider_status` shows whether each mail account is ready to use and whether it has recently had connection or send problems.
- Mail accounts can now use generic `imap_smtp`, dedicated `feishu_mail`, dedicated `wecom_mail`, Microsoft 365 mail, or Google Mail; all of them follow the same mail tool contract.
- If `mail` cannot run because an account is incomplete, credentials no longer work, or the latest connection failed, the result now explains that directly.

### `documents`

- `documents` is the external office document-library capability. It does not replace the local/public document-reading tools.
- Documents accounts can now come from WebDAV, Feishu document libraries, WeCom Wedrive libraries, Microsoft 365 document libraries, or Google document libraries.
- Supported operations:
  - `provider_status`
  - `list`
  - `read`
  - `summarize`
  - `search`
- If only one document space is linked, Beetle will just use it. If multiple spaces are linked, Beetle will first try to infer whether you mean a work or personal space, and ask only when that still is unclear.
- When you ask for a person- or team-related workspace, Beetle can reuse the shared people and organization directory to narrow the right document space before it asks follow-up questions.
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
- If a Feishu, WeCom, Microsoft 365, or Google contacts directory is connected, `lookup` can supplement local contacts with remote directory matches.
- If more than one directory is linked, Beetle will first try to infer whether you mean a work or personal directory, and ask only when that still is unclear.
- `provider_status` shows which contacts-directory accounts are available, which one is the default, and whether each account is ready to use.
- `mail send` and `draft` already consume this shared people lookup through recipient lookup fields. If a remote directory result clearly points to one office mail suite, Beetle can use that signal to narrow the sender side too.
- `calendar` can reuse the same shared people lookup when you schedule around named people or teams, so Beetle can narrow the right calendar from the same context.
- `documents` can also reuse people or organization context from the shared directory when choosing between multiple linked document spaces.

### `remind_at`

- `remind_at` no longer only creates reminders; it can now inspect, update, and delete saved reminders too.
- When a reminder is linked to a local calendar or a remote office calendar, changing or deleting the reminder also updates or removes the linked calendar event, so the two sides do not drift apart.
- You can still use it as a plain reminder by simply saying “remind me tomorrow at 3pm…”; Beetle only syncs it into a calendar when you explicitly ask for calendar linkage.

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
- `provider_schema` helps you inspect what a service needs before you try to connect it.
- `draft_*` / `validate_*` only prepare and check configuration drafts; they do not write device state.
- Credential operations now clean and validate values before write, including filling defaults, blocking missing required items, and rejecting fields that do not belong to that provider.
- `commit_*` and `revoke` are explicit write actions, so Beetle will ask for clear confirmation before it performs them.
- `assess` and `office_status` are meant to tell you what is missing, what is wrong, and what to fix next, without expecting you to know internal field names.
- `probe` reports only real status. Missing config, unsupported probe paths, or current unavailability are returned explicitly.

### `office_status`

- Reads the unified office state instead of any one tool's private status.
- Use it to inspect accounts, defaults, whether credentials exist, and the latest runtime status.
- It now directly tells you whether each account is ready, missing sign-in info, needs a connection check, or recently failed.
- You can also scope the result to one office area such as calendar, mail, documents, or contacts.

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
