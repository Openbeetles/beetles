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
| `remind_at` | 管理提醒 |
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

### `task`

- `task` 支持可选日历联动。
- 默认联动本地日历；如果已经接好办公日历，也可以把任务同步到外部日历。
- 如果你接了多个外部日历，Beetle 会优先按当前语境判断；仍然不够明确时，它会直接追问你要用哪一个，不要求你记内部标识。
- 任务完成后，已联动的日历事项会同步收口；删除任务或清除日历联动时，对应日历事项也会一起移除。

### `calendar`

- 默认使用本地日历。
- 接入外部日历账户后，也可以查看、创建、更新和删除外部事件。
- 现在外部日历账户既可以接 CalDAV，也可以接飞书日历、微软 365 日历或 Google 日历。
- 如果你接了多个外部日历，Beetle 会尽量根据上下文判断是工作日历还是私人日历；判断不稳时会直接追问，而不是悄悄选错。
- 当你围绕某个人或某个团队创建、修改会议时，Beetle 也会复用统一联系人目录里的线索，一起判断更合适的是哪一个日历，而不是把“找参会人”和“选日历”拆成两套逻辑。
- 飞书日历账户适合直接接团队日历；接好后，还是继续用同一个 `calendar` 工具管理事件，不需要换一套工具。
- `provider_status` 会告诉你当前有哪些日历账户、默认走哪个账户，以及每个账户现在是可用、还需要补配置，还是最近连接/使用出过问题。
- 远端 `list` / `get` / `create` / `update` / `delete` 后，后续状态查询会显示最近一次使用是否成功，方便排查问题。

### `mail`

- `mail` 是外部办公邮件能力，不是本地私有邮箱实现。
- 当前支持：
  - `provider_status`
  - `list`
  - `search`
  - `get`
  - `send`
  - `draft`
  - `reply`
  - `forward`
- 如果你只接了一个邮箱，Beetle 会直接用它；如果你接了多个邮箱，Beetle 会先按上下文判断是工作邮箱还是私人邮箱，仍然不稳时再追问。
- 当你发信或存草稿时引用的是远端工作联系人目录里的联系人，Beetle 会把这条线索一起考虑进去；如果它已经能明确对应到某一套工作邮箱，就会直接选那一套，而不是先落回默认邮箱再来追问。
- `search` 可以按关键词在指定邮箱里找邮件，返回的结果可直接继续交给 `get`、`reply`、`forward` 等后续动作使用。
- `send`、`draft`、`reply`、`forward` 都属于显式远端变更动作，执行前会要求明确确认。
- 发信时既可以直接给出邮箱地址，也可以直接说“发给某某”，让 Beetle 先去联系人目录里找人；回复邮件时，Beetle 会保留原始发件人作为基础收件人，并允许继续补充其他收件人。
- `provider_status` 会直接告诉你每个邮件账户现在能不能用，以及最近有没有连接或发送失败。
- 现在邮件账户既可以接通用 `imap_smtp`，也可以把飞书邮箱接成独立 `feishu_mail`，把企业微信邮箱接成独立 `wecom_mail`，或把微软 365 / Google 邮箱接成独立办公邮箱账户；它们都走同一套收发信合同。
- 如果 `mail` 因为账户没配好、凭证失效，或最近连接失败而不能使用，返回结果会直接说明原因。

### `documents`

- `documents` 是外部办公文档库/文件库能力，不替代本地文档读取工具。
- 现在文档账户既可以接 WebDAV，也可以接飞书文档库、企业微信微盘文档库，或微软 365 / Google 文档库。
- 当前支持：
  - `provider_status`
  - `list`
  - `read`
  - `summarize`
  - `search`
- 如果你只接了一个文档空间，Beetle 会直接用它；如果你接了多个文档空间，Beetle 会优先按当前语境判断工作/私人归属，不够明确时会继续问你。
- 当你提到某个人、某个团队或某个组织上下文时，Beetle 也会复用统一联系人目录里的线索，先收窄更合理的文档空间，再决定是否需要继续追问。
- 飞书文档账户适合接一个已经共享给 Beetle 的飞书文件夹；企业微信文档账户则需要提供微盘 `space_id` 和共享根目录 id，之后也继续用同一个 `documents` 工具查看目录、读取文档和搜索内容。
- `provider_status` 会告诉你当前有哪些文档账户、默认走哪个账户，以及每个账户现在是否可用。
- `summarize` 会把文档整理成简短摘要、关键点、待办，以及可直接交给邮件或任务流程继续使用的内容。
- 远端 `list` / `read` / `search` 后，后续状态查询会显示最近一次使用是否成功，方便排查问题。
- 如果 `documents` 因为账户没配好、凭证失效，或最近连接失败而不能使用，返回结果会直接说明原因。
- `documents` 读取的是“办公文档库/文件库”能力；本地设备存储里的文件检索仍然继续使用 `document_search`、`document_read`、`document_extract`

### `contacts_directory`

- `contacts_directory` 仍然是 Beetle 的统一联系人支撑层；本地联系人和接入的办公目录都会从这里汇合到同一套 people lookup。
- 当前支持：
  - `status`
  - `provider_status`
  - `list`
  - `lookup`
  - `upsert`
  - `delete`
- 它的目标是把“人”的稳定资料沉淀下来，比如姓名、邮箱、别名、组织和备注。
- 如果已经接入飞书、企业微信、微软 365 或 Google 联系人目录账户，`lookup` 会在本地联系人之外继续补充远端目录结果。
- 如果你接了多个联系人目录，Beetle 会尽量按当前语境判断应查工作目录还是私人目录；仍然不稳时会直接追问。
- `provider_status` 会告诉你当前有哪些联系人目录账户、默认走哪个账户，以及每个账户现在是否可用。
- 现在 `mail send` 和 `draft` 已经会消费这里的 people lookup；如果远端目录已经明确指向某一套办公邮箱，Beetle 也会顺着这条线索直接收窄发件侧选择。
- `calendar` 现在也会复用这层 people lookup：当你围绕某个人或某个团队安排会议时，Beetle 会把同一条目录线索继续用到日历侧选择。
- `documents` 也会复用这里的人员/组织线索：当你在多个文档空间之间切换时，Beetle 会先用这些上下文帮助判断更合理的空间归属。

### `remind_at`

- `remind_at` 不只是新增提醒，现在也能查看、修改和删除已保存的提醒。
- 如果提醒已经联动到本地日历或外部办公日历，改时间、改说明、删除提醒时，对应的日历事项也会一起更新或移除，不需要手工清理两边。
- 你仍然可以只说“明天下午三点提醒我……”，把它当普通提醒来用；只有你明确要联动日历时，Beetle 才会把它同步到日历侧。

### `office_config`

- 这是 office 域的统一配置工具，不是某个单独服务的私有控制面。
- 当前支持：
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
- `provider_schema` 用来查看不同办公服务需要准备哪些配置项，适合接新服务时先确认需要填什么。
- `draft_*` / `validate_*` 只会整理和检查配置草稿，不会直接写入设备。
- 凭证相关操作会自动做基础清洗和校验，比如补默认值、拦住缺失必填项、拒绝不属于该服务的额外字段。
- `commit_*` 和 `revoke` 属于显式配置写操作，执行前会要求明确确认。
- `assess` 和 `office_status` 会直接告诉你“还缺什么、哪里不对、下一步该补什么”，不要求自己猜字段名。
- `probe` 只返回真实状态，不会假装成功；缺配置、不可探测或当前不可用都会明确说明原因。

### `office_status`

- 读取 office 域的统一状态，而不是某个工具自己的私有状态。
- 可以用来查看账户、默认绑定、凭证是否存在，以及最近一次运行状态。
- 现在会直接说明每个账户当前是可用、缺少登录信息、需要重新检查连接，还是最近一次使用失败。
- 也可以只看某一类办公能力，比如日历、邮件、文档或联系人目录。

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
