# 配置接口

[English](../en-us/config-api.md) | **中文** | [文档索引](../README.md)

这页给自己写前端、脚本或集成的人看。每个接口都按“做什么、怎么传、会返回什么”来写。

## 调用约定

- 基础地址：首次配置常用 `http://192.168.4.1`；设备入网后用设备当前地址。
- CORS：`/api/*` 支持跨域，`OPTIONS` 可直接调用。
- 返回格式：除 `GET /api/soul`、`GET /api/user`、`GET /api/skills?name=...`、`GET /api/metrics?format=prometheus` 外，默认返回 JSON。
- 错误格式：常见错误返回 `{"error":"..."}`。
- 配对码：用查询参数 `?code=`，或请求头 `X-Pairing-Code`。
- CSRF：用请求头 `X-CSRF-Token`；先调用 `GET /api/csrf_token` 获取。
- 保存配置类接口提交完整对象，不支持只传要改的单个字段：
  `POST /api/config/llm`、`POST /api/config/channels`、`POST /api/config/system`、
  `POST /api/config/hardware`、`POST /api/config/audio`、`POST /api/config/display`。

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
- `403`：CSRF 不通过、Webhook token 不通过，或当前操作需要先打开运维窗口。
- `404`：资源不存在。
- `500`：服务端处理失败。
- `503`：当前不可用，例如扫描器未就绪、队列不可用。

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

**GET /api/config**

用途：读取当前总配置。

鉴权：`配对码`

成功响应：`200 application/json`

返回体是完整配置对象，并额外带上 `locale` 和 `build_package`。这个结果包含敏感字段，不能直接暴露给无鉴权页面。

**POST /api/config/wifi**

用途：保存网络配置。

鉴权：`配对码 + CSRF`

请求体：`application/json`

```json
{
  "wifi_ssid": "MyWiFi",
  "wifi_pass": "secret"
}
```

可选查询参数：`restart=1`
保存成功后如果带了这个参数，设备会自动重启。

成功响应：`200 application/json`

```json
{
  "ok": true,
  "restart_required": true
}
```

**POST /api/config/system**

用途：保存系统段配置。

鉴权：`配对码 + CSRF`

请求体：`application/json`

字段：

- `wifi_ssid`
- `wifi_pass`
- `proxy_url`
- `session_max_messages`
- `tg_group_activation`
- `locale`

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

**POST /api/config/channels**

用途：保存聊天通道配置。

鉴权：`配对码 + CSRF`

请求体：`application/json`

字段分组：

- 通用：`enabled_channel`
- Telegram：`tg_token`、`tg_allowed_chat_ids`
- 飞书：`feishu_app_id`、`feishu_app_secret`、`feishu_verification_token`、`feishu_encrypt_key`、`feishu_allowed_chat_ids`
- 钉钉：`dingtalk_webhook_url`、`dingtalk_app_secret`
- 企业微信：`wecom_corp_id`、`wecom_corp_secret`、`wecom_agent_id`、`wecom_default_touser`、`wecom_token`、`wecom_encoding_aes_key`
- QQ 频道：`qq_channel_app_id`、`qq_channel_secret`
- 自定义 Webhook：`webhook_enabled`、`webhook_token`

字段说明补充：

- `feishu_verification_token`：飞书 HTTP 事件订阅的 Verification Token；明文与解密后的事件体都会校验。
- `feishu_encrypt_key`：飞书 HTTP 事件订阅 Encrypt Key；配置后 `/api/feishu/event` 会按飞书官方规则校验 `X-Lark-Signature` 并解密 `encrypt`。
- `dingtalk_webhook_url`：钉钉自定义机器人 Webhook；用于会话外主动发送，留空时 `enabled_channel=dingtalk` 仍可工作，但仅支持会话回调 `sessionWebhook` 回复。
- `dingtalk_app_secret`：钉钉自定义机器人加签 secret；仅主动发送到 `dingtalk_webhook_url` 时使用。
- `wecom_token`：企业微信回调 Token；`GET/POST /api/wecom/webhook` 都要求非空并做签名校验。
- `wecom_encoding_aes_key`：企业微信安全模式回调的 EncodingAESKey；配置后 GET 验证会解密 `echostr`，POST 会解密 XML 中的 `Encrypt`。

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

成功响应：`200 application/json`

```json
{
  "ok": true
}
```

字段说明见 [硬件设备配置](hardware-device-config.md)。

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
- `wake_word`
- `speech`
- `tts`
- `realtime`
- `ambient_listening`
- `led_indicator`

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
- `capabilities`
- `account_fields`
- `config_fields`

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

顶层字段：

- `account`
- `set_defaults`
- `clear_defaults`
- `policy_patch`
- `config`

`account` 字段：

- `account_key`
- `provider_kind`
- `external_account_id`
- `account_label`
- `identity_class`
- `enabled_capabilities`

`config` 字段和 `POST /api/config/accounts/:account_key/config` 的请求体结构相同。

成功响应：`200 application/json`

返回体是账号详情对象。

**GET /api/config/accounts/:account_key**

用途：读取单个账号详情和可编辑字段。

鉴权：`配对码`

成功响应：`200 application/json`

返回体结构：

- `account`
- `assessment`
- `fields`

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

**GET /api/soul**

用途：读取系统文本。

鉴权：`已激活`

成功响应：`200 text/plain`

返回体就是原始文本内容。

**POST /api/soul**

用途：保存系统文本。

鉴权：`配对码 + CSRF`

请求体支持两种形式：

- `text/plain`：直接传原始文本
- `application/json`：`{"content":"..."}`

成功响应：`200 application/json`

```json
{
  "ok": true
}
```

**GET /api/user**

用途：读取用户文本。

鉴权：`已激活`

成功响应：`200 text/plain`

**POST /api/user**

用途：保存用户文本。

鉴权：`配对码 + CSRF`

请求体和 `POST /api/soul` 相同。

成功响应：`200 application/json`

```json
{
  "ok": true
}
```

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
- `soul_len`
- `user_len`
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

**GET /api/health**

用途：读取轻量状态摘要。

鉴权：`已激活`

成功响应：`200 application/json`

返回体顶层字段：

- `wifi`
- `last_error`
- `display`
- `audio`
- `workflow`

**GET /api/operator/status**

用途：读取完整运维状态。

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

用途：读取指标快照。

鉴权：`已激活`

成功响应：`200 application/json`

返回体是指标对象，常见字段包括：

- `messages_in`
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

用途：读取资源快照。

鉴权：`已激活`

成功响应：`200 application/json`

返回体顶层字段：

- `pressure`
- `tls_fragmentation_risk`
- `storage_contention_risk`
- `heap_free_internal`
- `heap_free_spiram`
- `heap_largest_block_internal`
- `active_http_count`
- `active_wss_count`
- `active_agent_tasks`
- `inbound_depth`
- `outbound_depth`
- `budget`
- `session_count`
- `storage_used_kb`
- `storage_total_kb`

**GET /api/diagnose**

用途：读取诊断结果。

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
- `ota_available`
- `locale`
- `lan_ip`
- `workflow`
- `programmable_reasoning`
- `storage_media`

**GET /api/channel_connectivity**

用途：检查当前通道连接状态。

鉴权：`已激活`

成功响应：`200 application/json`

返回体顶层字段：

- `channels`

**POST /api/operator/window**

用途：打开临时运维窗口，让受保护的运维接口可访问。

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

**GET /api/ota/check**

用途：检查是否有更新。

鉴权：`已激活`

可选查询参数：`channel`
不传时默认用 `stable`。

成功响应：`200 application/json`

返回体字段：

- `current_version`
- `update_available`
- `latest_version`
- `url`
- `release_notes`
- `error`

不同情况下，不一定会同时出现所有字段。

**POST /api/ota**

用途：开始更新。

鉴权：`配对码 + CSRF`

请求体：`application/json`

```json
{
  "url": "https://example.com/beetle.bin"
}
```

成功响应：`200 application/json`

```json
{
  "ok": true
}
```

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

**平台回调接口**

这些接口直接接收平台回调请求。请求体、签名和校验规则以各平台要求为准：

- `POST /api/feishu/event`
- `POST /api/dingtalk/webhook`
- `GET /api/wecom/webhook`
- `POST /api/wecom/webhook`
- `POST /api/webhook/qq`

当前实现口径：

- 飞书：`/api/feishu/event` 支持 `url_verification` 与 `im.message.receive_v1`；会校验 `feishu_verification_token`，配置 `feishu_encrypt_key` 时会校验 `X-Lark-Signature` 并解密 `encrypt`，并按 `message_id` 做 HTTP webhook 幂等。
- 钉钉：`/api/dingtalk/webhook` 接收应用机器人回调，缓存 `sessionWebhook` 作为会话内回复通道；主动消息仍走 `dingtalk_webhook_url`，若配置了 `dingtalk_app_secret` 会按钉钉自定义机器人规则附带签名。
- 企业微信：`GET /api/wecom/webhook` 同时支持明文与安全模式 URL 校验；`POST /api/wecom/webhook` 同时支持明文 XML 与安全模式 `Encrypt` XML，安全模式会校验 `msg_signature` 并验证解密后的 `receiveid == wecom_corp_id`；无回复内容时返回 HTTP 200 空响应体。
- QQ：`POST /api/webhook/qq` 继续按 QQ 机器人官方 Ed25519 规则验签；出站群聊/单聊回复要求已有被动回复 `msg_id`，频道连通性同时要求 access token 可用且 QQ WebSocket 在线。
