# Beetle Tool List

[中文](../zh-cn/tools.md) | **English** | [Doc index](../README.md)

This page lists the tool names that actually exist in the current codebase.
In normal use, you do not need to memorize them, but this page shows what each tool does and when it appears.

Tools with the same name are listed once.
For example, `task` and `remind_at` already exist by default, and gain more integrations when more capabilities are connected.

## Core Tools

| Tool | What it does | When it appears |
|------|--------------|-----------------|
| `get_time` | Check the current time | Default |
| `env` | View environment variables | Default |
| `message` | Send a visible message to the current or a specific chat | Default |
| `task` | Manage tasks and due times; can also sync with calendars when office capability is connected | Default |
| `remind_at` | Create, view, update, and delete reminders | Default |
| `remind_list` | Show upcoming reminders for the current chat | Default |
| `files` | List, read, and delete files under storage | Default |
| `file_edit` | Make localized edits to an existing text file | Default |
| `file_write` | Write or append file content directly | Default |
| `kv_store` | Save simple key-value data | Default |

## Memory And History

| Tool | What it does | When it appears |
|------|--------------|-----------------|
| `private_garden` | Manage Beetle's own private notes, drafts, and working material | Default |
| `factual_memory` | Read stable facts that have already been retained | Default |
| `memory_search` | Search past records, notes, and turn logs for evidence | Default |
| `memory_get` | Open one specific record from history | Default |
| `continuity_snapshot` | Export, load, or inspect memory restore snapshots; mostly for operator use | Default |
| `memory_manage` | Manage persistent memory, daily notes, and long-term memory entries | Diagnostics capability |
| `session_manage` | View, clear, or delete chat sessions | Diagnostics capability |

## Status, Diagnostics, And Operations

| Tool | What it does | When it appears |
|------|--------------|-----------------|
| `board_info` | Show a device or host status summary | Default |
| `diagnose_delivery` | Investigate failed sends, delays, or reply handoff issues | Default |
| `diagnose_system` | Investigate overall runtime health, resource pressure, and degraded capability | Default |
| `diagnose_network_path` | Investigate network path, DNS, proxy, and upstream reachability | Default |
| `diagnose_memory_runtime` | Investigate whether memory runtime is behaving abnormally | Default |
| `diagnose_voice_path` | Investigate voice input, voice output, and related setup | Default |
| `network_scan` | Scan Wi-Fi, view Wi-Fi status, and run connectivity checks | Diagnostics capability |
| `system_control` | View system status and storage usage, or perform controlled restart and emergency stop actions | Diagnostics capability |
| `cron_manage` | Manage persistent scheduled tasks | Diagnostics capability |

## Web, Documents, And External Requests

| Tool | What it does | When it appears |
|------|--------------|-----------------|
| `web_search` | Search the web and return titles, links, and summaries | Web/document capability |
| `web_fetch` | Open a web page and return cleaned readable text | Web/document capability; host deployments |
| `document_search` | Search stored documents by keyword | Web/document capability; host deployments |
| `document_read` | Read a web page or a stored document | Web/document capability; host deployments |
| `document_extract` | Extract a section, matching lines, or a field from a page or document | Web/document capability; host deployments |
| `pdf_read` | Open a public PDF and extract text | Web/document capability; host deployments |
| `analyze_image` | Analyze an image and answer questions about it | Web/document capability; host deployments |
| `http_request` | Make a general HTTP request | Web/document capability; host deployments |
| `proxy_config` | View, set, or clear proxy configuration | Web/document capability |
| `model_config` | View or update model configuration | Web/document capability |

## Office And Collaboration

| Tool | What it does | When it appears |
|------|--------------|-----------------|
| `calendar` | Manage calendar events | Office capability; host deployments |
| `mail` | View, search, send, reply to, and forward mail | Office capability; host deployments |
| `contacts_directory` | Manage the contacts directory used by mail and calendar flows | Office capability; host deployments |
| `documents` | List, read, search, and summarize document libraries | Office capability; host deployments |
| `office_config` | Manage office accounts and related configuration | Office capability; host deployments |
| `office_status` | Check whether office accounts are configured and usable | Office capability; host deployments |

## Hardware And Voice

| Tool | What it does | When it appears |
|------|--------------|-----------------|
| `device_control` | Control hardware devices or read connected basic sensors | Diagnostics capability + hardware connected |
| `sensor_watch` | Watch sensors continuously and alert when a threshold is hit | Diagnostics capability + hardware connected |
| `i2c_device` | Read from and write to configured I2C devices | Diagnostics capability + configured I2C devices |
| `i2c_sensor` | Read configured I2C sensors | Diagnostics capability + configured I2C sensors |
| `voice_input` | Listen through the microphone and turn speech into text | Audio configured |
| `voice_output` | Speak text aloud through the speaker | Audio configured |

## Host Extra Tools

| Tool | What it does | When it appears |
|------|--------------|-----------------|
| `shell` | Run a small allowlist of system commands | Host deployments |
| `process` | View the running process list or inspect one process | Host deployments |
| `network` | View interfaces, DNS, and routes, or run resolve, ping, and HTTP probes | Host deployments |
| `lua_query` | Run a Lua script against provided input data | Linux deployments |
| `lua_datasheet_distill` | Use Lua to distill hardware reference text | Linux deployments |
| `lua_protocol_frame_helper` | Use Lua to prepare protocol frame drafts | Linux deployments |
| `lua_register_table_helper` | Use Lua to prepare register table drafts | Linux deployments |
| `lua_state_machine_checker` | Use Lua to check state-machine descriptions | Linux deployments |
| `lua_memory_query` | Use Lua to inspect a memory snapshot | Linux deployments |
| `lua_tool_bridge` | Use Lua to generate tool-call proposals from the tool list without executing tools directly | Linux deployments |
| `capability_atoms_exchange` | Export or import capability item exchange data | Linux deployments |
| `capability_atoms_inspect` | Inspect local capability items and exchange readiness | Linux deployments |

## Read These Next When Needed

- To get Beetle running first: [configuration.md](configuration.md)
- To configure model providers: [llm-providers.md](llm-providers.md)
- To connect hardware or sensors: [hardware.md](hardware.md) and [hardware-device-config.md](hardware-device-config.md)
- To build your own frontend or script: [config-api.md](config-api.md)
