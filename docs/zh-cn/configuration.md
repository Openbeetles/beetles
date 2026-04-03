# 配置指南

[English](../en-us/configuration.md) | **中文** | [文档索引](../README.md)

这篇文档是写给“想尽快把 Beetle 设备跑起来的人”的。

你看完它，基本就能把第一次配置走通。它主要解决四件事：

- 第一次怎么连上设备
- 怎么设置配对码
- 怎么配置 WiFi、LLM 和聊天通道
- 常见设置到底是干什么的

如果你后面还要自己写前端、脚本或者集成系统，看完这篇再去看 [config-api.md](config-api.md)。

## 首次配置

### 第一步：连接设备热点

设备第一次上电时，会开启一个名为 **Beetle** 的热点。你可以把它理解成设备给你的“第一次登录入口”。

1. 用手机或电脑连接这个热点
2. 浏览器打开 **http://192.168.4.1**
3. 进入配对和配置流程

### 第二步：设置配对码

配对码用于保护写操作。最常见的几类是：

- 保存配置
- 重启设备
- 恢复出厂
- 执行 OTA

说明：

- 第一次打开配置页时就要设置
- 通过配置页写入的密钥会进入 NVS
- 密钥不会直接写到 SPIFFS，也不应被打印到日志里

### 第三步：配置 WiFi

保存 WiFi 后，设备会尝试连接你的路由器。

一旦连上成功，只要浏览器和设备处于同一局域网，你就可以改用设备的局域网 IP 访问配置页，不用再回到热点模式。

### 第四步：配置运行时

通常最少要完成这三项：

1. WiFi
2. 一个 LLM 源
3. 一个聊天通道

## 配置页怎么打开

实际使用里，通常就是下面两种方式：

### 方式 A：直接访问设备

- 连着设备热点时：打开 **http://192.168.4.1**
- 和设备同局域网时：打开设备的局域网 IP

### 方式 B：使用外部 Web UI

仓库里自带 `configure-ui`，它本质上也是通过 HTTP API 和设备通信。

但前提仍然是：

- 设备已经烧录
- 浏览器和设备在同一个网络
- 你知道设备地址

## 你会配置哪些内容

| 区域 | 作用 |
|------|------|
| WiFi | 路由器名称和密码 |
| LLM | provider、模型、API Key、API URL、多源回退顺序 |
| 通道 | 各聊天通道的凭证和开关 |
| 代理 / 搜索 | 代理地址和搜索相关 key |
| 硬件 | `hardware.json` 驱动的外设配置 |
| 显示 | SPI TFT 仪表板 |
| 系统 | 重启、恢复、诊断、OTA 等 |

## 常见配置键

下面这些名字会频繁出现在代码、配置文件和 API 里，先有个印象会省很多事：

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

LLM 的真实运行时配置主要看 `config/llm.json`。支持哪些 provider、`api_url` 怎么填、多源怎么回退，请看 [llm-providers.md](llm-providers.md)。

## 配对码和激活状态

设备设置过配对码之后，规则就会变成这样：

- 大多数只读 API 要求设备已经激活
- 写操作 API 需要“配对码 + CSRF”

如果你用的是配置页，这些通常都会自动处理。
如果你自己调接口，就继续看 [config-api.md](config-api.md)。

## 常用检查方式

- `GET /api/health`：快速看整体状态
- `GET /api/resource`：看运行时资源快照
- 串口日志：看启动信息和 heartbeat

精确返回结构见 [config-api.md](config-api.md)。

## 常见问题

- 打不开设备：先重新连接热点 **Beetle**，再试 `http://192.168.4.1`
- 保存失败：大多数时候是配对码或 CSRF 失效
- 设备在线但通道不工作：优先检查凭证和允许的 chat id
- 启动了但没有硬件控制：检查 `hardware.json` 是否正确，以及 `device_control` 是否真的被注册
