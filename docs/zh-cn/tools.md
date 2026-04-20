# Beetle 工具清单

[English](../en-us/tools.md) | **中文** | [文档索引](../README.md)

本页列的是当前代码里真实存在的工具名。
你平时不用背名字，但这里会完整展示它们做什么、什么时候会出现。

同名工具只列一次。
比如 `task` 和 `remind_at` 默认就有；接入更多能力后，它们会在原来的名字上增加联动能力。

## 基础工具

| 工具名 | 做什么 | 什么时候会出现 |
|--------|--------|----------------|
| `get_time` | 查看当前时间 | 默认 |
| `message` | 主动发一条可见消息到当前或指定会话 | 默认 |
| `task` | 管理任务，可带截止时间；接入办公能力后也能联动日历 | 默认 |
| `remind_at` | 新建、查看、修改、删除提醒 | 默认 |
| `remind_list` | 查看当前会话的提醒列表 | 默认 |
| `files` | 列出、读取、删除存储区文件 | 默认 |
| `file_edit` | 对现有文本文件做局部修改 | 默认 |
| `file_write` | 直接写入或追加文件 | 默认 |
| `kv_store` | 保存简单键值信息 | 默认 |

## 记忆与历史

| 工具名 | 做什么 | 什么时候会出现 |
|--------|--------|----------------|
| `private_garden` | 管理 Beetle 自己的私有笔记区、草稿和整理材料 | 默认 |
| `factual_memory` | 查看已经沉淀下来的稳定事实 | 默认 |
| `memory_search` | 在历史记录、日记和回合记录里搜索线索 | 默认 |
| `memory_get` | 打开一条历史记录详情 | 默认 |
| `continuity_snapshot` | 导出、载入或检查记忆恢复快照，偏管理用途 | 默认 |
| `memory_manage` | 管理持久记忆、每日笔记和长期记忆条目 | 诊断能力 |
| `session_manage` | 查看、清空或删除会话 | 诊断能力 |

## 状态、诊断与运维

| 工具名 | 做什么 | 什么时候会出现 |
|--------|--------|----------------|
| `board_info` | 查看整机状态摘要 | 默认 |
| `diagnose` | 通过 `op` 选择一个运行时平面进行诊断（`system`、`network`、`memory`、`delivery`、`voice`） | 默认 |
| `network_scan` | 扫描 Wi-Fi、看 Wi-Fi 状态、做连通性检查 | 诊断能力 |
| `system_control` | 查看系统状态、存储占用，或执行受控的重启与紧急停用 | 诊断能力 |
| `cron_manage` | 管理持久化定时任务 | 诊断能力 |

## 网页、文档与外部请求

| 工具名 | 做什么 | 什么时候会出现 |
|--------|--------|----------------|
| `web_search` | 搜网页，返回标题、链接和摘要 | 网页/文档能力 |
| `web_fetch` | 打开网页并提取可读正文 | 网页/文档能力；主机侧部署 |
| `document_search` | 在存储区文档里按关键词搜索 | 网页/文档能力；主机侧部署 |
| `document_read` | 读取网页或存储区文档内容 | 网页/文档能力；主机侧部署 |
| `document_extract` | 从网页或文档里抽取某一段、某几行或某个字段 | 网页/文档能力；主机侧部署 |
| `pdf_read` | 打开公开 PDF 并提取文字 | 网页/文档能力；主机侧部署 |
| `analyze_image` | 分析图片内容并回答问题 | 网页/文档能力；主机侧部署 |
| `http_request` | 发起通用 HTTP 请求 | 网页/文档能力；主机侧部署 |
| `proxy_config` | 查看、设置或清空代理配置 | 网页/文档能力 |
| `model_config` | 查看或修改模型配置 | 网页/文档能力 |

## 办公与协作

| 工具名 | 做什么 | 什么时候会出现 |
|--------|--------|----------------|
| `calendar` | 管理日历事件；办公账号状态/可用性先看 `office_status`，接入或修复走 `office_config` | 办公能力；主机侧部署 |
| `mail` | 查看、搜索、发送、回复和转发邮件；办公账号状态/可用性先看 `office_status`，接入或修复走 `office_config` | 办公能力；主机侧部署 |
| `contacts_directory` | 管理联系人目录，供邮件和日历查人；远端办公账号状态先看 `office_status`，接入或修复走 `office_config` | 办公能力；主机侧部署 |
| `documents` | 查看、读取、搜索和总结文档库内容；办公账号状态/可用性先看 `office_status`，接入或修复走 `office_config` | 办公能力；主机侧部署 |
| `office_config` | 完整的办公账号管理入口；主线路径是 `provider_schema`，再 `apply_account`，账号歧义时用 `resolve_account` | 办公能力；主机侧部署 |
| `office_status` | LLM 侧唯一的办公账号状态/可用性/诊断入口 | 办公能力；主机侧部署 |

## 硬件与语音

| 工具名 | 做什么 | 什么时候会出现 |
|--------|--------|----------------|
| `device_control` | 控制硬件设备，或读取已接入的基础传感器 | 诊断能力 + 已接硬件 |
| `sensor_watch` | 持续监控传感器并在到达阈值时提醒 | 诊断能力 + 已接硬件 |
| `i2c_device` | 读写已配置的 I2C 设备 | 诊断能力 + 已接 I2C 设备 |
| `i2c_sensor` | 读取已配置的 I2C 传感器 | 诊断能力 + 已接 I2C 传感器 |
| `voice_input` | 通过麦克风收音并转成文字 | 音频已配置 |
| `voice_output` | 把文字播报出来 | 音频已配置 |

## 主机侧扩展工具

| 工具名 | 做什么 | 什么时候会出现 |
|--------|--------|----------------|
| `shell` | 运行少量白名单系统命令 | 主机侧部署 |
| `process` | 查看当前进程列表或某个进程详情 | 主机侧部署 |
| `network` | 查看网络接口、DNS、路由，或做解析、Ping、HTTP 探测 | 主机侧部署 |
| `lua_query` | 用 Lua 脚本处理一段输入数据 | Linux 部署 |
| `lua_datasheet_distill` | 用 Lua 脚本整理硬件参考资料 | Linux 部署 |
| `lua_protocol_frame_helper` | 用 Lua 脚本整理协议帧草案 | Linux 部署 |
| `lua_register_table_helper` | 用 Lua 脚本整理寄存器表草案 | Linux 部署 |
| `lua_state_machine_checker` | 用 Lua 脚本检查状态机说明 | Linux 部署 |
| `lua_memory_query` | 用 Lua 脚本查看一份记忆快照 | Linux 部署 |
| `lua_tool_bridge` | 用 Lua 脚本根据工具清单生成调用建议，不直接执行工具 | Linux 部署 |
| `capability_atoms_exchange` | 导出或导入能力条目交换数据 | Linux 部署 |
| `capability_atoms_inspect` | 查看本地能力条目列表和交换准备情况 | Linux 部署 |

## 什么时候看别的文档

- 要先把 Beetle 跑起来：看 [configuration.md](configuration.md)
- 要配置大模型：看 [llm-providers.md](llm-providers.md)
- 要接硬件或传感器：看 [hardware.md](hardware.md) 和 [hardware-device-config.md](hardware-device-config.md)
- 要自己写页面或脚本：看 [config-api.md](config-api.md)
