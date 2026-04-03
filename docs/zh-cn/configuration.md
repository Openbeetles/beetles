# 配置指南

[English](../en-us/configuration.md) | **中文** | [文档索引](../README.md)

Beetle 第一次配置时，按下面的顺序操作即可。

1. 连上设备热点
2. 设置配对码
3. 配置 WiFi
4. 配置大模型和聊天通道

如果需要自己调用接口，请阅读 [config-api.md](config-api.md)。

## 首次配置

### 1. 连接设备热点

设备第一次上电时，会开启一个名为 **Beetle** 的热点。

1. 用手机或电脑连接这个热点
2. 浏览器打开 **http://192.168.4.1**
3. 进入配对和配置流程

### 2. 设置配对码

配对码用于保护写操作，包括：

- 保存配置
- 重启设备
- 恢复出厂
- 在线升级固件

注意：

- 第一次打开配置页时就要设置
- 通过配置页写入的密钥会进入 NVS
- 密钥不会直接写到 SPIFFS，也不应被打印到日志里

### 3. 配置 WiFi

保存 WiFi 后，设备会尝试连接你的路由器。

连接成功后，只要浏览器和设备在同一局域网，就可以改用设备的局域网 IP 访问配置页。

### 4. 配置大模型和聊天通道

至少需要完成以下三项：

1. WiFi
2. 一个大模型来源
3. 一个聊天通道

## 访问配置页

### 方式 A：直接访问设备

- 连着设备热点时：打开 **http://192.168.4.1**
- 和设备同局域网时：打开设备的局域网 IP

### 方式 B：使用外部配置页面

仓库里自带 `configure-ui`，也可以用它来连接设备。

使用条件：

- 设备已经烧录
- 浏览器和设备在同一个网络
- 你知道设备地址

## 配置项

| 区域 | 作用 |
|------|------|
| WiFi | 路由器名称和密码 |
| 大模型 | 服务商、模型、密钥、接口地址、回退顺序 |
| 通道 | 各聊天通道的凭证和开关 |
| 代理 / 搜索 | 代理地址和搜索相关 key |
| 硬件 | `hardware.json` 驱动的外设配置 |
| 显示 | SPI TFT 仪表板 |
| 系统 | 重启、恢复、诊断、在线升级等 |

## 主要配置键

下列键名会出现在配置文件和接口里：

| 类别 | 键 | 含义 |
|------|----|------|
| WiFi | `WIFI_SSID`、`WIFI_PASS` | 路由器账号和密码 |
| Telegram | `TG_TOKEN`、`TG_ALLOWED_CHAT_IDS` | Telegram 机器人凭证与允许会话 |
| 飞书 | `FEISHU_APP_ID`、`FEISHU_APP_SECRET`、`FEISHU_ALLOWED_CHAT_IDS` | 飞书应用凭证 |
| 钉钉 | `DINGTALK_WEBHOOK_URL` | 钉钉 Webhook |
| 企微 | `WECOM_CORP_ID`、`WECOM_CORP_SECRET`、`WECOM_AGENT_ID`、`WECOM_DEFAULT_TOUSER` | 企微应用设置 |
| QQ 频道 | `QQ_CHANNEL_APP_ID`、`QQ_CHANNEL_SECRET` | QQ 频道凭证 |
| 代理 | `PROXY_URL` | 出站 HTTP 代理 |
| 搜索 | `SEARCH_KEY`、`TAVILY_KEY` | 搜索服务 key |

大模型相关配置主要在 `config/llm.json`。支持哪些服务商、`api_url` 怎么填、多个来源怎么切换，请看 [llm-providers.md](llm-providers.md)。

## 配对码与激活状态

设备完成配对后：

- 大多数只读接口要求设备已经激活
- 写操作接口需要“配对码 + CSRF”

内置配置页会自动处理这些校验。
手动调用接口时，请参阅 [config-api.md](config-api.md)。

## 状态检查

- `GET /api/health`：快速看整体状态
- `GET /api/resource`：查看资源占用情况
- 串口日志：看启动信息和 heartbeat

精确返回结构见 [config-api.md](config-api.md)。

## 常见问题

- 打不开设备：先重新连接热点 **Beetle**，再试 `http://192.168.4.1`
- 保存失败：大多数时候是配对码或 CSRF 失效
- 设备在线但通道不工作：优先检查凭证和允许的会话 ID
- 启动了但没有硬件控制：检查 `hardware.json` 是否正确，以及 `device_control` 是否真的被注册
