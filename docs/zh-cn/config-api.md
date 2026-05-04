# 配置接口

[English](../en-us/config-api.md) | **中文** | [文档索引](../README.md)

配置接口是给自定义前端、脚本和集成用的参考页。
如果基础配置还没跑通，先看 [configuration.md](configuration.md)。

先看激活流程，再看配置接口。
办公集成、维护接口和回调接口，等你真的需要时再往后读。

每个接口都按“做什么、怎么传、会返回什么”来写。

## 调用约定

- 基础地址：首次配置通常用 `http://192.168.4.1`；如果这是 Linux 嵌入式设备，且其 STA 侧已经落在 `192.168.4.0/24`，热点会避让到 `http://172.16.42.1`；设备入网后用设备当前地址。
- CORS：`/api/*` 支持跨域，`OPTIONS` 可直接调用。
- 返回格式：除 `GET /api/skills?name=...`、`GET /api/metrics?format=prometheus` 外，默认返回 JSON。
- 历史 `/api/soul`、`/api/user` 文本接口已退役，不属于当前合同。
- 错误格式：
  - 产品面与正式配置面 API 统一返回 `{"error_key":"..."}`，作为系统生成错误的稳定合同。
  - 若失败来自第三方上游，还会附带 `upstream_error`，并可能带 `upstream_status`、`error_stage`、`provider_kind`。
  - debug / operator / 协议兼容接口不受这条约束，可继续返回英文原文或协议要求的原始 body。
- 配对码：用查询参数 `?code=`，或请求头 `X-Pairing-Code`。
- CSRF：用请求头 `X-CSRF-Token`；先调用 `GET /api/csrf_token` 获取。
- 保存配置类接口提交完整对象，不支持只传要改的单个字段：
  `POST /api/config/llm`、`POST /api/config/channels`、`POST /api/config/system`、
  `POST /api/config/hardware`、`POST /api/config/audio`、`POST /api/config/display`。
- ESP 设备上的自定义前端应把同一设备的 `/api/*` 请求串行化；首屏只做必要的激活、安全和轻量状态请求，`/api/channel_connectivity`、`/api/wifi/scan`、硬件发现等慢诊断接口应由用户显式触发。不要新增或依赖 `/api/device_snapshot` 这类大全局聚合接口。

### 鉴权级别

- `公开`：不需要激活，也不需要配对码。
- `已激活`：设备已经设置过配对码，但本次请求不用再传配对码。
- `配对码`：本次请求要带配对码。
- `配对码 + CSRF`：本次请求要同时带配对码和 CSRF。

### 常见状态码

- `200`：请求成功。
- `202`：任务已接收，异步继续处理。
- `400`：参数或请求体不对。
- `401`：设备未激活，或配对码不对。
- `403`：CSRF 不通过、Webhook token 不通过，或当前操作需要先打开临时维护窗口。
- `404`：资源不存在。
- `500`：服务端处理失败。
- `503`：当前不可用，例如扫描器未就绪、队列不可用。

### 产品 API 错误合同

除 debug/operator / 协议兼容这两类例外外，产品面与正式配置面 `/api/*` 统一遵循：

```json
{
  "error_key": "common.not_found"
}
```

如果失败来自第三方上游，可同时返回：

```json
{
  "error_key": "office.provider_error",
  "provider_kind": "microsoft365_mail",
  "error_stage": "office_probe",
  "upstream_status": 401,
  "upstream_error": "AADSTS7000215: Invalid client secret is provided."
}
```

说明：

- `error_key` 是稳定语义合同，供前端或其他调用方翻译。
- `upstream_error` 是原始排障文本，不做翻译，也不保证语言统一。
- 只有第三方协议兼容接口和 debug/operator 接口不受这条合同约束。

当前仍会出现的基础设施 / 运行态 key 包括：

- `http.route_worker_busy`：设备正在处理配置或诊断任务，请稍后重试。
- `http.route_worker_memory_low`：设备当前内存余量不足，无法启动本次请求对应的路由 worker 任务。
- `runtime.config_blocked_by_voice`：实时语音会话活跃时拒绝配置操作，请按返回的等待时间重试。

## 激活与安全

**GET /api/pairing_code**

用途：查看设备是否已经设置配对码，并返回当前语言。

鉴权：`公开`

成功响应：`200 application/json`

```json
{
  "code_set": true,
  "locale": "zh"
}
```

**POST /api/pairing_code**

用途：第一次设置配对码。只能设置一次。

鉴权：`公开`

请求体：`application/json`

```json
{
  "code": "123456"
}
```

成功响应：`200 application/json`

```json
{
  "ok": true
}
```

常见失败：

- `400`：设备已经设置过配对码。
- `400`：`code` 不是 6 位数字。

**GET /api/csrf_token**

用途：获取写接口要用的 CSRF token。

鉴权：`公开`

成功响应：`200 application/json`

```json
{
  "csrf_token": "..."
}
```

### 建议的激活顺序

1. 调 `GET /api/pairing_code` 看是否已激活。
2. 如果还没激活，调 `POST /api/pairing_code` 设置配对码。
3. 调 `GET /api/csrf_token` 拿到 CSRF token。
4. 之后所有写接口都带上配对码和 CSRF。

## 配置接口

**GET /api/config/system**

用途：读取当前系统配置段。

鉴权：`配对码`

成功响应：`200 application/json`

字段：

- `wifi_ssid`
- `wifi_pass`
- `proxy_url`
- `locale`
  仅接受 `zh` 或 `en`；非法值会直接返回 `400 application/json`，不会被静默忽略。

**GET /api/config/llm**

用途：只读取大模型配置段，避免为了 AI 配置页加载整包总配置。

鉴权：`配对码`

成功响应：`200 application/json`

字段：

- `llm_sources`
- `llm_router_source_index`
- `llm_worker_source_index`

这个接口不返回 `locale`、`build_package`，也不携带其他配置段。

**POST /api/config/system**

用途：保存系统段配置。

鉴权：`配对码 + CSRF`

请求体：`application/json`

字段：

- `wifi_ssid`
- `wifi_pass`
- `proxy_url`
- `locale`

系统配置段不接收通道字段；`tg_group_activation` 必须通过 `POST /api/config/channels` 保存。

成功响应：`200 application/json`

```json
{
  "ok": true
}
```

**POST /api/config/llm**

用途：保存大模型配置。

鉴权：`配对码 + CSRF`

请求体：`application/json`

字段：

- `llm_sources`
- `llm_router_source_index`
- `llm_worker_source_index`

`llm_sources` 中每个来源包含这些字段：

- `provider`
- `api_key`
- `model`
- `api_url`
- `max_tokens`

示例：

```json
{
  "llm_sources": [
    {
      "provider": "provider_name",
      "api_key": "your_key",
      "model": "model_name",
      "api_url": "https://example.com/v1/chat/completions",
      "max_tokens": 1024
    }
  ],
  "llm_router_source_index": 0,
  "llm_worker_source_index": 0
}
```

成功响应：`200 application/json`

```json
{
  "ok": true
}
```

相关说明见 [LLM 服务配置](llm-providers.md)。

**GET /api/config/channels**

用途：读取聊天通道配置，并返回当前构建实际可用的通道目录。

鉴权：`配对码`

成功响应：`200 application/json`

```json
{
  "available_channels": ["telegram", "qq_channel"],
  "unavailable_enabled_channel": "wecom",
  "enabled_channel": "wecom",
  "tg_token": "",
  "tg_allowed_chat_ids": "",
  "tg_group_activation": "mention",
  "feishu_app_id": "",
  "feishu_app_secret": "",
  "feishu_allowed_chat_ids": "",
  "dingtalk_client_id": "",
  "dingtalk_client_secret": "",
  "wecom_bot_id": "",
  "wecom_bot_secret": "",
  "wecom_ws_url": "",
  "qq_channel_app_id": "",
  "qq_channel_secret": "",
  "webhook_enabled": false,
  "webhook_token": ""
}
```

返回字段说明：

- `available_channels`：当前固件或二进制实际编译进去的通道 ID。
- `unavailable_enabled_channel`：仅当已保存的 `enabled_channel` 没有编译进当前构建时返回。
- 其余字段是 `config/channels.json` 中 channels 配置段的扁平字段。

**POST /api/config/channels**

用途：保存聊天通道配置。

鉴权：`配对码 + CSRF`

请求体：`application/json`

字段分组：

- 通用：`enabled_channel`
- Telegram：`tg_token`、`tg_allowed_chat_ids`、`tg_group_activation`
- 飞书：`feishu_app_id`、`feishu_app_secret`、`feishu_allowed_chat_ids`
- 钉钉：`dingtalk_client_id`、`dingtalk_client_secret`
- 企业微信：`wecom_bot_id`、`wecom_bot_secret`、`wecom_ws_url`
- QQ 频道：`qq_channel_app_id`、`qq_channel_secret`
- 自定义 Webhook：`webhook_enabled`、`webhook_token`

字段说明补充：

- `dingtalk_client_id` / `dingtalk_client_secret`：钉钉 Stream Mode 注册连接凭证，用于订阅 `/v1.0/im/bot/messages/get`。
- `wecom_bot_id` / `wecom_bot_secret`：企业微信 AI Bot 长连接凭证。
- `wecom_ws_url`：企业微信 AI Bot 长连接地址；为空时使用官方默认 `wss://openws.work.weixin.qq.com`。
- 旧社交平台 HTTP callback / 平台自定义机器人字段已从配置模型删除；旧配置文件中的同名未知键会被忽略。用户自建 `POST /api/webhook` 仍由 `webhook_enabled` / `webhook_token` 控制。

保存语义补充：

- 服务端会校验并写入 `config/channels.json`；`tg_group_activation` 属于 channels 配置段，与 Telegram 通道字段一起保存。

`enabled_channel` 允许值：

- 空字符串
- `telegram`
- `feishu`
- `dingtalk`
- `wecom`
- `qq_channel`

最小示例：

```json
{
  "enabled_channel": "telegram",
  "tg_token": "bot_token",
  "tg_allowed_chat_ids": "123456",
  "webhook_enabled": false,
  "webhook_token": ""
}
```

成功响应：`200 application/json`

```json
{
  "ok": true
}
```

**GET /api/config/hardware**

用途：读取当前硬件配置。

鉴权：`配对码`

成功响应：`200 application/json`

如果还没有保存过，默认返回：

```json
{
  "hardware_devices": []
}
```

**POST /api/config/hardware**

用途：保存硬件配置。

鉴权：`配对码 + CSRF`

请求体：`application/json`

说明：

- 普通用户优先走 Configure UI 的 **设备配置 -> GPIO 设备** 或 **设备配置 -> I2C 传感器**
- 这两个页面底层仍读写同一个 `/api/config/hardware` 与 `HardwareSegment`
- 这里是给脚本、自定义前端和进阶集成看的原始接口合同

顶层字段：

- `hardware_devices`
- `i2c_bus`
- `i2c_devices`
- `i2c_sensors`

最小示例：

```json
{
  "hardware_devices": [],
  "i2c_bus": null,
  "i2c_devices": [],
  "i2c_sensors": []
}
```

通用 AHT20 示例：

```json
{
  "hardware_devices": [],
  "i2c_bus": {
    "sda_pin": 21,
    "scl_pin": 22,
    "freq_hz": 100000
  },
  "i2c_devices": [],
  "i2c_sensors": [
    {
      "id": "aht20_env",
      "addr": 56,
      "model": "aht20",
      "what": "AHT20 温湿度传感器",
      "how": "通过 I2C 读取环境温度与湿度；地址 0x38。",
      "options": {}
    }
  ]
}
```

成功响应：`200 application/json`

```json
{
  "ok": true
}
```

用户操作路径见 [硬件设备配置](hardware-device-config.md)；原始字段合同以本页和接口返回为准。

**GET /api/config/audio**

用途：读取当前音频配置。

鉴权：`配对码`

成功响应：`200 application/json`

返回体是完整音频配置对象。没有保存过时，会返回一份关闭状态的默认对象。

**POST /api/config/audio**

用途：保存音频配置。

鉴权：`配对码 + CSRF`

请求体：`application/json`

顶层字段：

- `version`
- `enabled`
- `service_provider`
- `microphone`
- `speaker`
- `vad`
- `wake_word`（当前配置 Beetle 内建的 acoustic 唤醒后端；外部 wake backend 只是未来扩展点，本轮不开放配置）
- `speech`
- `tts`
- `realtime`
- `ambient_listening`
- `led_indicator`

`wake_word` 仍保留顶层对象名以兼容既有配置。当前字段形状为：
`enabled`、`enter_threshold`、`leave_threshold`、`reference_suppress_ratio`、`zcr_min`、`zcr_max`、`min_speech_band_ratio`、`min_active_ms`、`hangover_ms`、`cooldown_ms`、`keyword`（仅兼容旧配置读取/回写）和 `wake_prompt`。configure-ui 只暴露 acoustic 参数和 `wake_prompt`。

可选查询参数：`restart=1`

成功响应：`200 application/json`

```json
{
  "ok": true,
  "restart_required": true
}
```

**GET /api/config/display**

用途：读取当前显示配置。

鉴权：`配对码`

成功响应：`200 application/json`

返回体是完整显示配置对象。没有保存过时，会返回一份关闭状态的默认对象。

**POST /api/config/display**

用途：保存显示配置。

鉴权：`配对码 + CSRF`

请求体：`application/json`

常用字段：

- `enabled`
- `driver`
- `bus`
- `width`
- `height`
- `rotation`
- `color_order`
- `invert_colors`
- `offset_x`
- `offset_y`
- `spi`
- `fb_device`
- `backlight_sysfs`
- `sleep_timeout_secs`

可选查询参数：`restart=1`

成功响应：`200 application/json`

```json
{
  "ok": true,
  "restart_required": true
}
```

字段说明见 [显示配置](display.md)。

**GET /api/wifi/scan**

用途：扫描附近 WiFi，给配置页或外部前端做下拉列表。

鉴权：`公开`

成功响应：`200 application/json`

```json
[
  {
    "ssid": "MyWiFi",
    "rssi": -50
  }
]
```

常见失败：

- `503`：当前还不能扫描。
- `500`：扫描过程失败。

**GET /api/hardware/discovery**

用途：发现可直接接入的外部硬件。

鉴权：`配对码`

查询参数：

- `bus`：当前公开值只有 `usb`
- `capability`：`audio_input`、`audio_output`、`camera`、`serial`、`hid`

调用示例：

```text
GET /api/hardware/discovery?bus=usb&capability=audio_output
```

成功响应：`200 application/json`

返回体字段：

- `bus`
- `capability`
- `items`

`items` 中每一项包含：

- `device_ref`
- `label`
- `kind`
- `capabilities`
- `is_default`
- `metadata`

常见失败：

- `400`：`bus` 或 `capability` 缺失，或值不对。
- `503`：当前能力还不能发现设备。

## 账号与办公能力接口

**GET /api/config/providers**

用途：读取可创建账号的服务目录。

鉴权：`配对码`

可选查询参数：`capability`

成功响应：`200 application/json`

返回体结构：

- `count`
- `items`

`items` 中每一项包含：

- `provider_kind`
- `display_name_key`
- `capabilities`
- `account_fields`
- `config_fields`

说明：

- `display_name_key` 供前端翻译 provider 名称。
- `account_fields` / `config_fields` 中的展示语义统一使用 `label_key`、`description_key`、option `label_key`。
- 产品 API 不再返回系统生成的 `display_name`、`label`、`description` 原文。

**GET /api/config/capabilities**

用途：读取各类办公能力当前状态。

鉴权：`配对码`

成功响应：`200 application/json`

返回体结构：

- `count`
- `items`

`items` 中每一项包含：

- `capability`
- `default_account_key`
- `selection_status`
- `selected_account_key`
- `ready`
- `next_action`
- `accounts`

**GET /api/config/capabilities/:capability**

用途：读取某一个能力的状态。

鉴权：`配对码`

路径参数：`capability`

当前公开值包括：

- `mail`
- `calendar`
- `documents`
- `contacts_directory`

成功响应：`200 application/json`

返回体和 `GET /api/config/capabilities` 中单项对象一致。

**GET /api/config/accounts**

用途：列出已接入账号。

这组 HTTP API 继续保持完整的公开账号管理合同，不会为了官方 UI 或 LLM 工具面简化而缩窄。对 LLM 来说，`office_status` 是状态/可用性入口，`office_config` 是管理入口。

鉴权：`配对码`

可选查询参数：

- `provider_kind`
- `capability`

成功响应：`200 application/json`

返回体结构：

- `count`
- `items`

`items` 中每一项包含：

- `account_key`
- `provider_kind`
- `display_name_key`
- `account_label`
- `identity_class`
- `enabled_capabilities`
- `selected_for_capabilities`
- `readiness`
- `next_action`
- `missing_fields_count`
- `has_runtime_error`

**POST /api/config/accounts**

用途：创建账号，或更新账号的基础信息。

这个 API 继续保留完整的公开账号管理能力，供第三方前端、脚本和其他 consumer 使用。官方 UI 只是其中一个 consumer；LLM 侧 office 工具则在同一套 office authority 之上走更窄的主线路径。

鉴权：`配对码 + CSRF`

请求体：`application/json`

顶层字段采用公开扁平合同，不再接受旧的嵌套 `account` / `credential` / `config` wrapper：

- `provider_kind`
- `provider`
- `capability`
- `identity_class`
- `account_label`
- `display_name`
- `external_account_id`
- `email`
- `account_id`
- `username`
- `password`
- `access_token`
- `refresh_token`
- `token_endpoint`
- `mail_username`
- `mail_from_address`
- `imap_host`
- `imap_port`
- `imap_tls`
- `smtp_host`
- `smtp_port`
- `smtp_tls`
- `metadata`

说明：

- `provider` 是 `provider_kind` 的公开别名。
- `display_name` / `label` 只作为输入别名参与归一化，不是产品响应合同字段。
- provider 自定义字段可直接放在顶层，或放进 `metadata`；后端会按 provider schema 归一化进配置字段。

成功响应：`200 application/json`

返回体是账号详情对象。

账号 summary/detail 的展示字段也遵循同一合同：

- `display_name_key` 用于翻译 provider / account 的展示名称。
- 产品响应不再返回系统生成的 `display_name`、`label`、`description` 原文。

若请求缺少用户必须补充的事实，接口返回 `400`，body 为结构化 onboarding 结果，例如：

```json
{
  "disposition": "needs_user_facts",
  "reason": "missing_user_facts",
  "missing_fields": ["identity_class"],
  "missing_field_details": [
    {
      "key": "identity_class",
      "label_key": "accounts.identityLabel",
      "description_key": "accounts.identityDescription",
      "options": [
        { "value": "work", "label_key": "accounts.identity.work" }
      ]
    }
  ]
}
```

**GET /api/config/accounts/:account_key**

用途：读取单个账号详情和可编辑字段。

鉴权：`配对码`

成功响应：`200 application/json`

返回体结构：

- `account`
- `assessment`
- `fields`

说明：

- `account.display_name_key` 用于翻译 provider / account 的展示名称。
- `fields` 与 `assessment.missing_field_details` 中的展示语义统一使用 `label_key` / `description_key` / option `label_key`。
- 产品 API 不返回系统生成的 `display_name` / `label` / `description` 原文。

**POST /api/config/accounts/:account_key/config**

用途：保存单个账号的配置字段。

鉴权：`配对码 + CSRF`

请求体：`application/json`

```json
{
  "fields": {
    "tenant_id": "xxx",
    "client_id": "xxx"
  },
  "clear_fields": [
    "old_secret"
  ]
}
```

成功响应：`200 application/json`

返回体是更新后的账号详情对象。

**POST /api/config/accounts/:account_key/probe**

用途：检查这个账号现在能不能正常使用。

鉴权：`配对码 + CSRF`

请求体：无。

成功响应：`200 application/json`

返回体字段：

- `account_key`
- `provider_kind`
- `configured`
- `disposition`
- `reason`

如果失败来自第三方上游，返回 `400`，body 为：

- `error_key`
- 可选 `provider_kind`
- 可选 `error_stage`
- 可选 `upstream_status`
- 可选 `upstream_error`

**POST /api/config/accounts/:account_key/revoke**

用途：撤销这个账号，并可选清理运行中的状态。

鉴权：`配对码 + CSRF`

请求体：`application/json`

```json
{
  "clear_runtime_status": true
}
```

请求体也可以留空；留空时默认等同于 `true`。

成功响应：`200 application/json`

```json
{
  "ok": true,
  "account_key": "mail-main",
  "cleared_runtime_status": true
}
```

**DELETE /api/config/accounts/:account_key**

用途：删除这个账号。

鉴权：`配对码 + CSRF`

成功响应：`200 application/json`

```json
{
  "ok": true,
  "account_key": "mail-main",
  "deleted": true
}
```

## 内容、会话、技能与维护接口

**GET /api/sessions**

用途：列出会话，或读取单个会话最近消息。

鉴权：`已激活`

查询参数：

- 列表模式：`page`、`limit`
- 单会话模式：`chat_id`

列表模式成功响应：`200 application/json`

```json
{
  "items": [
    "chat-1",
    "chat-2"
  ],
  "total": 2,
  "page": 1,
  "limit": 20,
  "total_pages": 1
}
```

单会话模式成功响应：`200 application/json`

返回体是最近消息数组。

**DELETE /api/sessions**

用途：删除一个会话。

鉴权：`配对码 + CSRF`

查询参数：`chat_id`

成功响应：`200 application/json`

```json
{
  "ok": true
}
```

**GET /api/memory/status**

用途：读取记忆状态；需要时，也可以带目标会话做深度检查。

鉴权：`已激活`

常用查询参数：

- `chat_id`
- `channel`
- `query`
- `run_id`
- `deep=1`
- `snapshot_mode=full_restore`
- `memory_system_kind=esp_compact`
- `memory_system_kind=linux_full`

默认模式成功响应：`200 application/json`

返回体顶层字段：

- `memory_system_kind`
- `memory_len`
- `long_term_count`
- `continuity_capsule_count`
- `stores`
- `personality`
- `continuity_tooling`
- `continuity_capsules`
- `task_execution`
- `learning`
- `diagnosis`
- `operator_surface`

当带 `deep=1` 且给出 `chat_id` 时，还会多一个 `inspection` 字段。

特殊情况：

- `403`：当前这次深度检查要先调用 `POST /api/operator/window`。

**POST /api/memory/maintenance**

用途：提交记忆维护任务。

鉴权：`配对码 + CSRF`

请求体：`application/json`

```json
{
  "action": "run_repair_plan",
  "chat_id": "chat-1",
  "channel": "qq_channel"
}
```

`action` 当前公开值：

- `run_repair_plan`
- `rebuild_continuity_snapshot`
- `reconcile_relationship_governance`
- `replay_recovery`
- `refresh_operator_digest`

成功响应：`202 application/json`

返回体至少包含：

- `accepted`
- `delivery`

**GET /api/tools**

用途：读取当前可用工具列表。

鉴权：`已激活`

ESP 嵌入式说明：`/api/tools` 在激活后保持可用。它仍要求设备已配对，但不要求开启临时诊断会话。

成功响应：`200 application/json`

```json
[
  {
    "name": "web_search",
    "i18n_key": "tools.web_search"
  }
]
```

**GET /api/capability_packages**

用途：读取能力包状态。

鉴权：`已激活`

成功响应：`200 application/json`

返回体顶层字段：

- `installed`
- `enabled`
- `active_now`
- `workflow_count`
- `skill_fragment_count`
- `policy_overlay_count`
- `asset_count`
- `packages`

`packages` 中每一项包含：

- `package_id`
- `version`
- `display_name`
- `enabled`
- `compatible_now`
- `requirements_satisfied`
- `workflow_count`
- `skill_fragment_count`
- `policy_count`
- `asset_count`
- `required_capabilities`
- `missing_capabilities`
- `channel_compatibility`
- `rollback_available`

**POST /api/capability_packages**

用途：安装、启用、停用、卸载或回退能力包。

鉴权：`配对码 + CSRF`

请求体：`application/json`

启用、停用、卸载、回退：

```json
{
  "op": "enable",
  "package_id": "package_id"
}
```

安装：

```json
{
  "op": "install",
  "payload": {
    "manifest": {
      "package_id": "package_id",
      "version": "1.0.0",
      "display_name": "Package name"
    },
    "skills": [],
    "workflows": [],
    "policies": [],
    "assets": [],
    "enable_on_install": true
  }
}
```

`op` 当前公开值：

- `install`
- `enable`
- `disable`
- `uninstall`
- `rollback`

成功响应：`200 application/json`

```json
{
  "ok": true,
  "outcome": {}
}
```

**GET /api/skills**

用途：列出技能，或读取单个技能内容。

鉴权：`已激活`

查询参数：

- 不带 `name`：返回列表
- 带 `name`：返回单个技能内容

列表模式成功响应：`200 application/json`

```json
{
  "skills": [
    {
      "name": "example",
      "enabled": true
    }
  ],
  "order": [
    "example"
  ]
}
```

单技能模式成功响应：`200 text/plain`

返回体就是技能文件内容。

**POST /api/skills**

用途：写技能内容、启用或停用技能、调整技能顺序。

鉴权：`配对码 + CSRF`

请求体支持三种形式：

写内容：

```json
{
  "name": "example",
  "content": "# Skill"
}
```

启用或停用：

```json
{
  "name": "example",
  "enabled": false
}
```

调整顺序：

```json
{
  "order": [
    "example",
    "another"
  ]
}
```

成功响应：`200 application/json`

```json
{
  "ok": true
}
```

**DELETE /api/skills**

用途：删除一个技能。

鉴权：`配对码 + CSRF`

查询参数：`name`

成功响应：`200 application/json`

```json
{
  "ok": true
}
```

常见失败：

- `404`：技能不存在。

**POST /api/skills/import**

用途：从 URL 导入技能。

鉴权：`配对码 + CSRF`

请求体：`application/json`

```json
{
  "url": "https://example.com/skill.md",
  "name": "imported-skill"
}
```

成功响应：`200 application/json`

```json
{
  "ok": true
}
```

## 状态与运维接口

### 观测接口分层

- `/api/health` 是轻量生命体征，适合首屏和状态灯。不要把资源诊断、workflow、完整网络快照放进这里。
- `/api/resource` 是轻量资源压力快照，适合状态面板默认轮询。它不是设备总状态，也不返回健康总览或详细运行态内部对象。
- `/api/metrics` 是计数器和最近耗时。不要在这里放 heap/resource/network 对象。
- `/api/operator/status` 是面向人和 UI 的解释面，可聚合多个真源说明“为什么是这个状态”；不作为机器准入真源。
- `/api/diagnose` 是主动诊断结果和建议，输出诊断项，不是原始快照仓库。

旧字段 `health.wifi`、`health.network`、`health.workflow`、`resource.network`、`resource.firmware_identity` 已删除，不提供兼容。自定义前端不要依赖 `/api/health` 的诊断字段，也不要把 `/api/resource` 当作设备总状态或详细诊断接口。

**GET /api/health**

用途：读取轻量生命体征。

鉴权：`已激活`

成功响应：`200 application/json`

返回体顶层字段：

- `status`
- `network_status`
  - `stage`
  - `sta_connected`
  - `wall_clock_trusted`
- `last_error`
- `display`
- `audio`

**GET /api/operator/status**

用途：读取面向人和 UI 的运维解释状态。

鉴权：`已激活`

成功响应：`200 application/json`

返回体顶层字段：

- `platform_contract`
- `build_package`
- `operator_surface`
- `reply_pipeline`
- `delivery_diagnosis`
- `system_diagnosis`
- `memory_operator_surface`
- `workflow`
- `programmable_reasoning`
- `os_closure`
- `initiative`
- `presence`
- `runtime_mode`
- `soul_kernel`
- `capability_planes`

**GET /api/metrics**

用途：读取累计计数器和最近耗时。

鉴权：`已激活`

成功响应：`200 application/json`

返回体是指标对象，常见字段包括：

- `messages_in`（仅外部用户入站消息）
- `agent_messages_in`（agent 平面消费的全部消息，包含内部系统任务）
- `system_messages_in`（agent 平面消费的内部系统任务）
- `messages_out`
- `llm_calls`
- `llm_errors`
- `tool_calls`
- `tool_errors`
- `llm_last_ms`
- `e2e_last_ms`

**GET /api/metrics?format=prometheus**

用途：用 Prometheus 文本格式读取指标。

鉴权：`已激活`

成功响应：`200 text/plain`

**GET /api/resource**

用途：读取轻量资源压力快照。

鉴权：`已激活`

成功响应：`200 application/json`

返回体顶层字段：

- `pressure`
- `tls_fragmentation_risk`
- `storage_contention_risk`
- `heap_free_internal`
- `heap_min_free_internal`
- `heap_free_spiram`
- `heap_total_spiram`
- `heap_min_free_spiram`
- `heap_largest_block_spiram`
- `heap_used_spiram_est`
- `heap_largest_block_internal`
- `active_http_count`
- `active_wss_count`
- `active_agent_tasks`
- `inbound_depth`
- `outbound_depth`
- `budget`
- `governance_metrics`
- `session_count`
- `storage_used_kb`
- `storage_total_kb`
- `cpu_usage_percent`（仅 Linux / 宿主侧）
- `load_average`（仅 Linux / 宿主侧）
- `process_memory_kb`（仅 Linux / 宿主侧）

资源端点属于默认轮询契约，必须保持轻量。`heap_free_spiram` 表示 PSRAM 空闲量，不是已用量；`heap_used_spiram_est = heap_total_spiram - heap_free_spiram`，仅用于帮助判读 PSRAM 是否被实际消耗。`governance_metrics` 提供精简的压力和拒绝计数；客户端应允许未知字段存在，但不要期待这里返回健康总览、详细运行态内部对象、crash 证据或固件身份。

**GET /api/diagnose**

用途：读取主动诊断结果和建议。

鉴权：`已激活`

成功响应：`200 application/json`

返回体是诊断结果数组。

**GET /api/system_info**

用途：读取设备基础信息。

鉴权：`已激活`

成功响应：`200 application/json`

常见字段：

- `product_name`
- `current_time`
- `firmware_version`
- `board_id`
- `locale`
- `lan_ip`
- `programmable_reasoning`
- `storage_media`

**GET /api/channel_connectivity**

用途：读取当前通道连接状态。ESP 默认返回被动快照，不做外部 live probe；显式刷新由 `POST /api/channel_connectivity/refresh` 执行。

鉴权：`已激活`

成功响应：`200 application/json`

返回体顶层字段：

- `channels`
- `checked_at_unix_secs`
- `stale`

`channels` 项字段：

- `id`：通道 ID。
- `configured`：是否已经配置。
- `ok`：最近一次显式连通性探测是否成功。
- `message_key`：前端可翻译的状态或错误键。
- `runtime_status`：运行态状态，例如 `connected`、`connecting`、`waiting_network`、`disabled`。
- `runtime_reason`：运行态原因键，可为空。

展示建议：若 `runtime_status=connected`，可把常驻 WSS/消息通道显示为在线；`stale=true` 或 `message_key=network.channel_connectivity_unavailable` 只表示当前被动快照没有 live probe 结果，不应直接覆盖已在线的运行态。

**POST /api/operator/window**

用途：打开临时维护窗口，让受保护的维护接口可访问。

鉴权：`配对码 + CSRF`

成功响应：`200 application/json`

返回体字段：

- `opened`
- `operator_window`
- `windowed_endpoints`

**POST /api/restart**

用途：重启设备。

鉴权：`配对码 + CSRF`

成功响应：`200 application/json`

```json
{
  "ok": true
}
```

**POST /api/config_reset**

用途：清空当前配置并回到未激活状态。

鉴权：`配对码 + CSRF`

成功响应：`200 application/json`

```json
{
  "ok": true
}
```

固件更新说明：

当前 Beetle 主线功能较多，系统包体较大，无法在同时保留现有功能与体验的前提下继续提供官方 OTA 升级能力。如果你需要 OTA，可以自行裁剪功能、重新规划分区表，或联系我们做定制方案。

主线现行换固件方式是浏览器 USB 烧录、串口烧录或工厂重刷。

## 回调接口

**POST /api/webhook**

用途：接收自定义 webhook 消息。

鉴权：`配对码 + CSRF`

额外校验：

- `X-Webhook-Token`
- 或查询参数 `token`

请求体：原始文本内容。

成功响应：`200 application/json`

```json
{
  "ok": true
}
```

常见失败：

- `401`：Webhook token 不对。
- `403`：Webhook 没开启，或没有配置 token。
- `413`：内容太长。
- `503`：消息队列已满。

**社交通道传输口径**

- 飞书：入站走事件订阅长连接；出站走 Feishu IM OpenAPI。
- 钉钉：入站走 Stream Mode WSS；出站只使用活动会话的 `sessionWebhook`。
- 企业微信：入站/出站走 AI Bot WSS，默认 `wss://openws.work.weixin.qq.com`。
- QQ：入站走 Gateway WSS；出站群聊/单聊回复要求已有被动回复 `msg_id`。
- Telegram：入站走 `getUpdates` long polling；启动轮询前会调用 `deleteWebhook(drop_pending_updates=false)` 清理平台侧 webhook 配置。
