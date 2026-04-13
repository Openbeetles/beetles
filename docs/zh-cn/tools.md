# 工具说明

[English](../en-us/tools.md) | **中文** | [文档索引](../README.md)

这页列的是 Beetle 现在能用到的工具。

补充说明：

- 普通聊天里，用户不需要手动输入工具名
- 模型会在需要时自动调用这些工具
- 实际可见的工具列表会受平台、功能开关和配置影响

## 基础工具

| 工具 | 用途 |
|------|------|
| `get_time` | 获取当前 UTC 时间 |
| `env` | 读取环境变量，主要用于配置和调试 |
| `message` | 发送一条由程序接管的消息 |
| `task` | 持久任务管理 |
| `calendar` | 持久日历事件 |
| `mail` | 通过共享 office 账户权威读取、查看和发送邮件 |
| `documents` | 通过共享 office 账户权威查看远端文档库状态、目录、正文和搜索结果 |
| `contacts_directory` | 本地联系人目录，供 Agent 做 people lookup 和后续办公组合 |
| `office_config` | 以结构化操作检查、草拟、校验、提交和撤销 office 配置 |
| `office_status` | 查看 office 账户、默认绑定、凭证存在性和运行态探测状态 |
| `files` | 列出或读取设备存储中的文件 |
| `file_edit` | 对设备存储中的文本文件做局部修改 |
| `remind_at` | 创建提醒 |
| `remind_list` | 列出当前对话的提醒 |
| `board_info` | 查看设备型号、运行时间、WiFi 和存储等基础信息 |
| `kv_store` | 持久键值存储 |
| `private_garden` | 当前对话的私有空间 |
| `memory_search` | 搜索聊天记录、每日记录、回合记录中的档案内容 |
| `memory_get` | 读取一条档案记录 |
| `factual_memory` | 读取设备保存的稳定事实 |
| `continuity_snapshot` | 导出、保存、列出或导入连续性数据 |
| `file_write` | 向允许写入的设备文件写内容 |

## 重点补充

### `calendar`

- 默认使用本地日历。
- 接入外部日历账户后，也可以查看、创建、更新和删除外部事件。
- 如果同类外部账户有多个，可以显式传 `account_key`，也可以先在 office 配置里设默认账户。
- `provider_status` 返回的是当前可用日历能力、已配置账户状态，以及默认账户和运行状态摘要。

### `mail`

- `mail` 是外部办公邮件能力，不是本地私有邮箱实现。
- 当前支持：
  - `provider_status`
  - `list`
  - `get`
  - `send`
- 如果已经配置默认邮件账户，或当前只有一个可用账户，可以省略 `provider` / `account_key`。
- `send` 属于显式对外发送动作，要求 `confirm=true`。
- `send` 既支持直接邮箱数组 `to` / `cc` / `bcc`，也支持通过 `contacts_directory` 解析的联系人查询数组 `to_lookup` / `cc_lookup` / `bcc_lookup`。
- `provider_status` 返回的是当前可用邮件能力、已配置账户状态，以及默认账户和运行状态摘要。

### `documents`

- `documents` 是外部办公文档库/文件库能力，不替代本地文档读取工具。
- 当前支持：
  - `provider_status`
  - `list`
  - `read`
  - `search`
- 如果已经配置默认文档账户，或当前只有一个可用账户，可以省略 `provider` / `account_key`。
- `provider_status` 返回的是当前可用文档能力、已配置账户状态，以及默认账户和运行状态摘要。
- `documents` 读取的是“办公文档库/文件库”能力；本地设备存储里的文件检索仍然继续使用 `document_search`、`document_read`、`document_extract`

### `contacts_directory`

- `contacts_directory` 是本地联系人支撑层，不是外部通讯录 provider 的壳子。
- 当前支持：
  - `status`
  - `list`
  - `lookup`
  - `upsert`
  - `delete`
- 它的目标是把“人”的稳定资料沉淀下来，比如姓名、邮箱、别名、组织和备注。
- 现在 `mail send` 已经会消费这里的 people lookup；后续 `calendar` 的参会人路由也应该继续复用这一层，而不是再造一套联系人逻辑。

### `office_config`

- 这是 office 域的统一配置工具，不是某个单独服务的私有控制面。
- 当前支持：
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
- `draft_*` / `validate_*` 只处理结构化草案，不会直接写盘。
- `commit_*` 和 `revoke` 属于显式配置写操作，要求 `confirm=true`。
- `probe` 只返回真实状态，不会假装成功；缺配置、不可探测或当前不可用都会明确说明原因。

### `office_status`

- 读取 office 域的统一状态，而不是某个工具自己的私有状态。
- 可以用来查看账户、默认绑定、凭证是否存在，以及最近一次运行状态。
- 可选传 `capability`，只看某一个能力，比如 `calendar`、`mail`、`documents` 或 `contacts_directory`。

## `tools_network_extra` 工具

| 工具 | 用途 |
|------|------|
| `web_search` | 网页搜索 |
| `analyze_image` | 分析图片链接 |
| `http_request` | 发起公网 HTTP 请求 |
| `proxy_config` | 读写代理配置 |
| `model_config` | 读写模型配置项 |

## 非 ESP 构建下的额外网络工具

以下工具仅在非 ESP 构建中提供。

| 工具 | 用途 |
|------|------|
| `document_search` | 搜索已保存的文档 |
| `document_read` | 读取公网链接或本地文档 |
| `document_extract` | 提取指定行、章节或字段 |
| `web_fetch` | 把公网网页抓成可读文本 |
| `pdf_read` | 读取公网 PDF 文档 |

## `tools_diagnostics` 工具

| 工具 | 用途 |
|------|------|
| `memory_manage` | 管理长期记忆及相关文本内容 |
| `session_manage` | 查看、清理或删除会话 |
| `system_control` | 重启和存储相关系统操作 |
| `cron_manage` | 持久定时任务 |
| `network_scan` | WiFi 和网络连通性检查 |

启用 `tools_diagnostics` 后，以下工具会按配置出现：

| 工具 | 出现条件 |
|------|----------|
| `device_control` | 配置了 `hardware_devices` |
| `sensor_watch` | 配置了可监控硬件或 I2C 传感器 |
| `i2c_device` | 配置了 I2C 总线和 I2C 设备 |
| `i2c_sensor` | 配置了 I2C 总线和 I2C 传感器 |

## 音频相关工具

只有在音频配置和凭证齐全时才会出现：

| 工具 | 用途 |
|------|------|
| `voice_input` | 语音转文字 |
| `voice_output` | 文字转语音 |

## Linux / 非 ESP 专属工具

以下工具只在非 ESP 构建中可用：

| 工具 | 用途 |
|------|------|
| `shell` | 受限 shell 执行 |
| `process` | 本地进程操作 |
| `network` | 本地网络诊断 |

## 使用限制

- `files` 只读；`file_write` 和 `file_edit` 只能操作允许写入的路径。
- `private_garden` 按当前对话隔离，不会和别的会话混在一起。
- `memory_search` 和 `memory_get` 返回的是历史内容，未必等于最终结论。
- `factual_memory` 更适合查“已经确认过”的稳定信息。
- `continuity_snapshot` 主要用于备份、迁移和恢复。
- `http_request`、`web_fetch`、`pdf_read` 会拒绝内网和本机目标。

相关文档：

- [hardware-device-config.md](hardware-device-config.md)
- [config-api.md](config-api.md)
