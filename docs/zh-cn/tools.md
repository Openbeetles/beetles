# Agent 工具说明

[English](../en-us/tools.md) | **中文** | [文档索引](../README.md)

这篇文档讲的是 Beetle 在运行时到底有哪些工具可用。

先把最容易误解的三点说清楚：

- 普通聊天里，用户不需要手动输入工具名
- 模型会在需要时自动调用这些工具
- 真正能看到哪些工具，还会受 target、feature、配置和策略影响

权威来源是 [`build_default_registry`](../../src/tools/registry.rs)。

## 始终注册的工具

| 工具 | 用途 |
|------|------|
| `get_time` | 获取当前 UTC 时间 |
| `env` | 读取环境变量，主要用于运行时/调试场景 |
| `message` | 发送运行时管理的出站消息 |
| `task` | 持久任务管理 |
| `calendar` | 持久日历事件 |
| `files` | 列出或读取状态根下的文件 |
| `file_edit` | 对状态根中的文本文件做局部修改 |
| `remind_at` | 创建提醒 |
| `remind_list` | 列出当前 chat 的提醒 |
| `board_info` | 查看芯片、堆、PSRAM、运行时间、WiFi、SPIFFS |
| `kv_store` | 持久键值存储 |
| `private_garden` | 当前 chat 私有工作区 |
| `memory_search` | 搜索 transcript、daily note、turn log 中的档案证据 |
| `memory_get` | 读取一条档案证据记录 |
| `continuity_snapshot` | 导出或导入连续性状态 |
| `file_write` | 向允许写入的状态根文件写内容 |

## `tools_network_extra` 下的工具

| 工具 | 用途 |
|------|------|
| `web_search` | 网页搜索 |
| `analyze_image` | 分析图片 URL |
| `http_request` | 发起公网 HTTP 请求 |
| `proxy_config` | 读写代理配置 |
| `model_config` | 读写模型配置字段 |

## 非 ESP 且启用 `tools_network_extra` 时额外出现的工具

这些工具只会在非 ESP 构建里出现。对固件用户来说，通常可以先忽略这一组。

| 工具 | 用途 |
|------|------|
| `document_search` | 搜索存储中的文档 |
| `document_read` | 读取公网 URL 或本地文档 |
| `document_extract` | 抽取行、章节或 JSON 字段 |
| `web_fetch` | 把公网网页抓成可读文本 |
| `pdf_read` | 读取公网 PDF |

## `tools_diagnostics` 下的工具

| 工具 | 用途 |
|------|------|
| `memory_manage` | 管理长期记忆及相关文本存储 |
| `session_manage` | 查看、清理或删除会话 |
| `system_control` | 重启和存储相关系统操作 |
| `cron_manage` | 持久定时任务 |
| `network_scan` | WiFi 和连通性检查 |

`tools_diagnostics` 打开后，下面这些工具会按条件出现：

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

## 仅宿主侧可用的工具

这些工具只在非 ESP 构建里可用，主要是给宿主侧开发和调试准备的：

| 工具 | 用途 |
|------|------|
| `shell` | 受限 shell 执行 |
| `process` | 宿主进程操作 |
| `network` | 宿主网络诊断 |

## 使用说明

- `files` 只读；`file_write` 和 `file_edit` 只能操作允许写入的路径。
- `private_garden` 按当前 chat 隔离，不会和别的会话混在一起。
- `memory_search` 和 `memory_get` 返回的是档案证据，不是最终事实层。
- `http_request`、`web_fetch`、`pdf_read` 会拒绝内网和本机目标。
- `GET /api/tools` 可能列不全全部运行时工具，真正的准绳还是注册表。

相关文档：

- [hardware-device-config.md](hardware-device-config.md)
- [config-api.md](config-api.md)
