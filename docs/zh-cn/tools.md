# 工具说明

[English](../en-us/tools.md) | **中文** | [文档索引](../README.md)

这页列的是 Beetle 现在能用到的工具。

补充说明：

- 普通聊天里，用户不需要手动输入工具名
- 模型会在需要时自动调用这些工具
- 实际可见的工具列表会受平台、编译选项、配置和策略影响

## 基础工具

| 工具 | 用途 |
|------|------|
| `get_time` | 获取当前 UTC 时间 |
| `env` | 读取环境变量，主要用于配置和调试 |
| `message` | 发送一条由程序接管的消息 |
| `task` | 持久任务管理 |
| `calendar` | 持久日历事件 |
| `files` | 列出或读取状态根下的文件 |
| `file_edit` | 对状态根中的文本文件做局部修改 |
| `remind_at` | 创建提醒 |
| `remind_list` | 列出当前对话的提醒 |
| `board_info` | 查看芯片、internal heap、总可用内存、PSRAM、最大连续 internal 空闲块、TLS 碎片风险、运行时间、WiFi 和存储信息 |
| `kv_store` | 持久键值存储 |
| `private_garden` | 当前对话的私有空间 |
| `memory_search` | 搜索聊天记录、每日记录、回合记录中的档案内容 |
| `memory_get` | 读取一条档案记录 |
| `factual_memory` | 读取 canonical shared factual plane，支持精确 slot 查询和证据态返回 |
| `continuity_snapshot` | 导出、保存、列出或导入连续性快照 |
| `file_write` | 向允许写入的状态根文件写内容 |

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
| `document_extract` | 提取行、章节或 JSON 字段 |
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
- `memory_search` 和 `memory_get` 返回的是档案记录，不是最终结论。
- `factual_memory` 读取的是 canonical shared factual plane。精确事实、稳定槽位、项目/任务/约束优先走它；结果会带 evidence posture、provenance，以及 miss 时的 nearby candidates。
- `continuity_snapshot` 支持 `export`、`import`、`list_saved`；`export` 可带 `save_name` 落盘，`import` 既可直接吃 JSON，也可按 `save_name` 从状态根加载。
- `http_request`、`web_fetch`、`pdf_read` 会拒绝内网和本机目标。
- `GET /api/tools` 可能不会列出全部工具；完整列表以注册表为准。

相关文档：

- [hardware-device-config.md](hardware-device-config.md)
- [config-api.md](config-api.md)
