# 配置接口说明

[English](../en-us/config-api.md) | **中文** | [文档索引](../README.md)

本文面向直接调用 Beetle HTTP 接口的开发者。内容只定义公开接口合同，包括：

- 访问地址与鉴权规则
- 请求方法、参数、请求体与响应体
- 重要的状态码与副作用

如果只是完成首次配网或使用设备自带页面，优先阅读 [configuration.md](configuration.md)。

## 基础信息

- **ESP SoftAP 地址**：首次上电后，ESP 固件会启动名为 **Beetle** 的热点。连接后使用 `http://192.168.4.1`。
- **Linux 设备地址**：如果系统已有可用 WiFi，Beetle 直接复用该网络，请使用设备当前局域网 IP。
- **跨域**：`/api/*` 与 `GET /` 返回 `Access-Control-Allow-Origin: *`。`OPTIONS` 预检返回 200，并带标准 CORS 头。

## 鉴权规则

### 术语

- **未激活**：设备尚未保存有效的 6 位配对码。
- **已激活**：`POST /api/pairing_code` 已成功执行过。
- **配对码**：通过 query `?code=` 或请求头 `X-Pairing-Code` 传递。
- **CSRF**：请求头 `X-CSRF-Token`，值来自 `GET /api/csrf_token`。

### 未激活时可调用的接口

- 任意 `OPTIONS`
- `GET /`
- `GET /wifi`
- `GET /pairing`
- `GET /common.css`
- `GET /common.js`
- `GET /api/pairing_code`
- `POST /api/pairing_code`
- `GET /api/wifi/scan`
- `GET /api/csrf_token`
- 通道回调：
  - `POST /api/feishu/event`
  - `POST /api/dingtalk/webhook`
  - `GET /api/wecom/webhook`
  - `POST /api/wecom/webhook`
  - `POST /api/webhook/qq`

除上述接口外，未激活状态通常返回 `401 Unauthorized`。

### 写接口的通用规则

激活后，所有会修改配置、运行态或内容的 `POST` / `DELETE` 接口默认都要求：

- 配对码
- CSRF Token

例外：

- `POST /api/pairing_code` 只在未激活时可用，不需要配对码和 CSRF
- 通道回调接口使用各平台自己的签名或 token 规则

## 发现与配对

### GET /

- **鉴权**：未激活可调用；激活后不要求在请求里再次附带配对码。
- **响应**：
  - 未激活：`302 Found`，`Location: /pairing`
  - 已激活：`200 OK`，JSON
- **响应体**：
  - `name`
  - `version`
  - `endpoints`

示例：

```json
{
  "name": "beetle",
  "version": "0.1.0",
  "endpoints": ["GET /pairing", "GET /wifi", "GET /api/pairing_code"]
}
```

### GET /api/pairing_code

- **鉴权**：无
- **响应**：`200 OK`
- **响应体**：
  - `code_set`
  - `locale`

### POST /api/pairing_code

- **鉴权**：无；仅未激活时可用
- **请求头**：`Content-Type: application/json`
- **请求体**：

```json
{ "code": "123456" }
```

- **响应**：
  - 成功：`200 OK`，`{"ok": true}`
  - 请求不合法或重复设置：`400 Bad Request`

### GET /api/csrf_token

- **鉴权**：无
- **响应**：`200 OK`

```json
{ "csrf_token": "<token>" }
```

### GET /pairing

- **鉴权**：无
- **响应**：`200 OK`
- **Content-Type**：`text/html; charset=utf-8`

### GET /wifi

- **鉴权**：无
- **响应**：`200 OK`
- **Content-Type**：`text/html; charset=utf-8`

## 配置总览

### GET /api/config

- **鉴权**：已激活 + 配对码
- **响应**：`200 OK`
- **响应体**：完整 `AppConfig` JSON，包含当前真实配置值

### GET /api/wifi/scan

- **鉴权**：无
- **响应**：
  - 成功：`200 OK`
  - 扫描不可用：`503 Service Unavailable`
- **响应体**：按信号强度降序排列的 WiFi 列表

```json
[
  { "ssid": "MyWiFi", "rssi": -50 }
]
```

### POST /api/config/wifi

- **鉴权**：已激活 + 配对码 + CSRF
- **请求头**：`Content-Type: application/json`
- **请求体**：

```json
{
  "wifi_ssid": "MyWiFi",
  "wifi_pass": "secret"
}
```

- **响应**：
  - 成功：`200 OK`
  - 校验失败：`400 Bad Request`
- **成功响应**：

```json
{ "ok": true, "restart_required": true }
```

### POST /api/config/system

- **鉴权**：已激活 + 配对码 + CSRF
- **请求头**：`Content-Type: application/json`
- **请求体**：系统配置段 JSON
- **主要字段**：
  - `wifi_ssid`
  - `wifi_pass`
  - `proxy_url`
  - `session_max_messages`
  - `tg_group_activation`
  - `locale`
- **响应**：
  - 成功：`200 OK`，`{"ok": true}`
  - 校验失败：`400 Bad Request`

### POST /api/config/llm

- **鉴权**：已激活 + 配对码 + CSRF
- **请求头**：`Content-Type: application/json`
- **请求体**：完整 LLM 配置段
- **主要字段**：
  - `llm_sources[]`
  - `llm_stream`
  - `llm_router_source_index`
  - `llm_worker_source_index`
- **响应**：
  - 成功：`200 OK`，`{"ok": true}`
  - 校验失败：`400 Bad Request`

### POST /api/config/channels

- **鉴权**：已激活 + 配对码 + CSRF
- **请求头**：`Content-Type: application/json`
- **请求体**：完整通道配置段
- **响应**：
  - 成功：`200 OK`，`{"ok": true}`
  - 校验失败：`400 Bad Request`

## 账户配置

### GET /api/config/providers

- **鉴权**：已激活 + 配对码
- **查询参数**：
  - `capability`：可选，`mail|calendar|documents|contacts_directory`
- **响应**：`200 OK`
- **响应体**：
  - `count`
  - `items[]`
    - `provider_kind`
    - `capabilities`
    - `account_fields[]`
      - `key`
      - `label`
      - `description`
      - `value_kind`
      - `required`
      - `secret`
      - `multiple`
      - `default_value`
      - `default_values`
      - `options[]`
    - `config_fields[]`
      - `key`
      - `label`
      - `description`
      - `location`
      - `value_kind`
      - `required`
      - `secret`
      - `default_value`

### GET /api/config/capabilities

- **鉴权**：已激活 + 配对码
- **响应**：`200 OK`
- **响应体**：
  - `count`
  - `items[]`
    - `capability`
    - `default_account_key`
    - `selection_status`
    - `selected_account_key`
    - `ready`
    - `next_action`
    - `accounts[]`
      - `account_key`
      - `provider_kind`
      - `account_label`
      - `identity_class`
      - `enabled_capabilities`
      - `selected_for_capabilities`
      - `readiness`
      - `next_action`
      - `missing_fields_count`
      - `has_runtime_error`

### GET /api/config/capabilities/:capability

- **鉴权**：已激活 + 配对码
- **路径参数**：
  - `capability`：`mail|calendar|documents|contacts_directory`
- **响应**：`200 OK`
- **响应体**：单个 capability 状态对象，字段与 `GET /api/config/capabilities` 的 `items[]` 一致

### GET /api/config/accounts

- **鉴权**：已激活 + 配对码
- **查询参数**：
  - `capability`：可选，`mail|calendar|documents|contacts_directory`
- `provider_kind`：可选，精确 provider kind，例如 `imap_smtp`、`feishu_mail`、`microsoft365_mail`、`google_mail`
- **响应**：`200 OK`
- **响应体**：
  - `count`
  - `items[]`
    - `account_key`
    - `provider_kind`
    - `account_label`
    - `identity_class`
    - `enabled_capabilities`
    - `selected_for_capabilities`
    - `readiness`
    - `next_action`
    - `missing_fields_count`
    - `has_runtime_error`

### POST /api/config/accounts

- **鉴权**：已激活 + 配对码 + CSRF
- **请求头**：`Content-Type: application/json`
- **请求体**：单个账户 create / upsert payload
- **主要字段**：
  - `account`
    - `provider_kind`
    - `external_account_id`
    - `account_label`
    - `identity_class`
    - `enabled_capabilities`
  - `set_defaults[]`
  - `clear_defaults[]`
  - `policy_patch`
  - `config`
    - `fields`
    - `clear_fields[]`
- **响应**：
  - 成功：`200 OK`，返回更新后的账户详情
  - 校验失败：`400 Bad Request`
- **补充**：
  - 如果 `account.account_key` 省略或为空，由服务端自动生成稳定的账户标识。
  - 响应体会返回生成后的 `account_key`，后续详情、配置、探测、撤销和删除都继续使用这个路径键。

### GET /api/config/accounts/:account_key

- **鉴权**：已激活 + 配对码
- **路径参数**：
  - `account_key`
- **响应**：`200 OK`
- **响应体**：
  - `account`
  - `assessment`
  - `fields[]`
    - `key`
    - `label`
    - `description`
    - `location`
    - `value_kind`
    - `required`
    - `secret`
    - `default_value`
    - `configured`
    - `current_value`

### POST /api/config/accounts/:account_key/config

- **鉴权**：已激活 + 配对码 + CSRF
- **请求头**：`Content-Type: application/json`
- **请求体**：

```json
{
  "fields": {
    "field_key": "value"
  },
  "clear_fields": ["another_field"]
}
```

- **响应**：
  - 成功：`200 OK`，返回保存后的账户详情
  - 校验失败：`400 Bad Request`

### POST /api/config/accounts/:account_key/probe

- **鉴权**：已激活 + 配对码 + CSRF
- **路径参数**：
  - `account_key`
- **响应**：
  - 成功：`200 OK`
  - 失败：`400 Bad Request`
- **响应体**：
  - `account_key`
  - `provider_kind`
  - `configured`
  - `disposition`
  - `reason`

### POST /api/config/accounts/:account_key/revoke

- **鉴权**：已激活 + 配对码 + CSRF
- **请求头**：`Content-Type: application/json`
- **请求体**：可为空；非空时支持：

```json
{
  "clear_runtime_status": true
}
```

- **响应**：
  - 成功：`200 OK`
  - 失败：`400 Bad Request`

### DELETE /api/config/accounts/:account_key

- **鉴权**：已激活 + 配对码 + CSRF
- **路径参数**：
  - `account_key`
- **响应**：
  - 成功：`200 OK`
  - 失败：`400 Bad Request`
- **成功响应**：

```json
{
  "ok": true,
  "account_key": "mail-work",
  "deleted": true
}
```

### GET /api/config/hardware

- **鉴权**：已激活
- **响应**：`200 OK`
- **响应体**：`HardwareSegment`

### POST /api/config/hardware

- **鉴权**：已激活 + 配对码 + CSRF
- **请求头**：`Content-Type: application/json`
- **请求体**：完整 `HardwareSegment`
- **响应**：
  - 成功：`200 OK`，`{"ok": true}`
  - 校验失败：`400 Bad Request`

### GET /api/config/audio

- **鉴权**：已激活
- **响应**：`200 OK`
- **响应体**：`AudioSegment`

### POST /api/config/audio

- **鉴权**：已激活 + 配对码 + CSRF
- **请求头**：`Content-Type: application/json`
- **请求体**：完整 `AudioSegment`
- **响应**：
  - 成功：`200 OK`
  - 校验失败：`400 Bad Request`
- **成功响应**：

```json
{ "ok": true, "restart_required": true }
```

### GET /api/config/display

- **鉴权**：已激活
- **响应**：`200 OK`

### POST /api/config/display

- **鉴权**：已激活 + 配对码 + CSRF
- **请求头**：`Content-Type: application/json`
- **响应**：
  - 成功：`200 OK`
  - 校验失败：`400 Bad Request`

## 文本配置

### GET /api/soul

- **鉴权**：已激活
- **响应**：`200 OK`
- **Content-Type**：`text/plain`

### POST /api/soul

- **鉴权**：已激活 + 配对码 + CSRF
- **请求体**：纯文本，或 JSON `{"content":"..."}`，长度上限 32KB
- **响应**：
  - 成功：`200 OK`
  - 参数错误：`400 Bad Request`
  - 写入失败：`500 Internal Server Error`

### GET /api/user

- **鉴权**：已激活
- **响应**：`200 OK`
- **Content-Type**：`text/plain`

### POST /api/user

- **鉴权**：已激活 + 配对码 + CSRF
- **请求体**：纯文本，或 JSON `{"content":"..."}`，长度上限 32KB
- **响应**：
  - 成功：`200 OK`
  - 参数错误：`400 Bad Request`
  - 写入失败：`500 Internal Server Error`

## 会话、记忆与工具

### GET /api/sessions

- **鉴权**：已激活
- **查询参数**：
  - 列表模式：`page`、`limit`
  - 详情模式：`chat_id` 或 `name`
- **响应**：`200 OK`

### DELETE /api/sessions?chat_id=...

- **鉴权**：已激活 + 配对码 + CSRF
- **查询参数**：
  - `chat_id`：必填
- **响应**：
  - 成功：`200 OK`
  - 参数错误：`400 Bad Request`

### GET /api/memory/status

- **鉴权**：已激活
- **响应**：`200 OK`
- **响应体**：memory operator 状态对象

### GET /api/tools

- **鉴权**：已激活
- **响应**：`200 OK`
- **响应体**：工具列表数组

## Skills

### GET /api/skills

- **鉴权**：已激活
- **查询参数**：
  - `name`：可选
- **响应**：
  - 无 `name`：`200 OK`，返回技能列表与顺序
  - 有 `name`：`200 OK`，返回技能文本；不存在时 `404 Not Found`

### POST /api/skills

- **鉴权**：已激活 + 配对码 + CSRF
- **请求头**：`Content-Type: application/json`
- **请求体**：
  - 启用/禁用：`{"name":"x","enabled":true}`
  - 写入内容：`{"name":"x","content":"..."}`
  - 更新顺序：`{"order":["a","b"]}`
- **响应**：
  - 成功：`200 OK`
  - 参数错误：`400 Bad Request`
  - 处理失败：`500 Internal Server Error`

### DELETE /api/skills?name=xxx

- **鉴权**：已激活 + 配对码 + CSRF
- **查询参数**：
  - `name`：必填
- **响应**：
  - 成功：`200 OK`
  - 参数错误：`400 Bad Request`
  - 不存在：`404 Not Found`

### POST /api/skills/import

- **鉴权**：已激活 + 配对码 + CSRF
- **请求头**：`Content-Type: application/json`
- **请求体**：

```json
{
  "url": "https://example.com/skill.md",
  "name": "skill-name"
}
```

- **响应**：
  - 成功：`200 OK`
  - 参数错误：`400 Bad Request`
  - 拉取失败：`502 Bad Gateway` 或 `500 Internal Server Error`

## 健康与运维

### GET /api/health

- **鉴权**：已激活
- **响应**：`200 OK`
- **响应体**：轻量健康状态对象，包含 `wifi`、`last_error`、`display`、`audio` 等字段

### GET /api/diagnose

- **鉴权**：已激活
- **响应**：`200 OK`
- **响应体**：诊断结果数组，每项包含：
  - `severity`
  - `category`
  - `message`

### GET /api/operator/status

- **鉴权**：已激活
- **响应**：`200 OK`
- **响应体**：operator 运行态状态对象

### GET /api/metrics

- **鉴权**：已激活
- **查询参数**：
  - `format=prometheus`：可选
- **响应**：
  - 默认：`200 OK`，JSON
  - `format=prometheus`：`200 OK`，Prometheus 文本

### GET /api/resource

- **鉴权**：已激活
- **响应**：`200 OK`
- **响应体**：资源、队列、压力、预算相关状态对象

### GET /api/system_info

- **鉴权**：已激活
- **响应**：`200 OK`
- **响应体**：设备与构建信息摘要

### GET /api/channel_connectivity

- **鉴权**：已激活
- **响应**：`200 OK`
- **响应体**：通道连通性状态对象

### POST /api/restart

- **鉴权**：已激活 + 配对码 + CSRF
- **响应**：
  - 成功：`200 OK`，随后设备重启
  - 节流或错误：`400` / `500`

### GET /api/ota/check

- **鉴权**：已激活
- **前提**：固件启用了 `ota`
- **查询参数**：
  - `channel`：可选，默认 `stable`
- **响应**：`200 OK`
- **响应体**：
  - `current_version`
  - `latest_version`
  - `update_available`
  - `url`
  - `release_notes`
  - `error`

### POST /api/ota

- **鉴权**：已激活 + 配对码 + CSRF
- **前提**：固件启用了 `ota`
- **请求头**：`Content-Type: application/json`
- **请求体**：

```json
{ "url": "https://example.com/firmware.bin" }
```

- **响应**：
  - 成功：`200 OK`，随后执行 OTA 并重启
  - 参数错误：`400 Bad Request`
  - 下载、校验或写入失败：`500 Internal Server Error`

### POST /api/config_reset

- **鉴权**：已激活 + 配对码 + CSRF
- **响应**：
  - 成功：`200 OK`，`{"ok": true}`
  - 失败：`500 Internal Server Error`

## Webhook 与平台回调

### POST /api/webhook

- **鉴权**：已激活 + 配对码 + CSRF；并要求 webhook token
- **请求体**：UTF-8 文本，上限 4KB
- **响应**：
  - 成功：`200 OK`
  - token 错误：`401 Unauthorized`
  - webhook 未启用：`403 Forbidden`
  - 参数错误：`400` / `413`
  - 队列满：`503 Service Unavailable`

### 平台回调接口

以下接口不使用设备自己的配对码与 CSRF：

- `POST /api/feishu/event`
- `POST /api/dingtalk/webhook`
- `GET /api/wecom/webhook`
- `POST /api/wecom/webhook`
- `POST /api/webhook/qq`

## 板子 IP 获取

- 连接设备热点 **Beetle**：使用 `http://192.168.4.1`
- 设备已连入局域网：使用路由器分配给设备的 IP

## 配置页归属

设备固件自带：

- `GET /wifi`
- `GET /pairing`
- `GET /common.css`
- `GET /common.js`

也可以使用仓库中的 `configure-ui`，或自定义前端调用同一套 HTTP 接口。
