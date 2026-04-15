# 配置接口说明

[English](../en-us/config-api.md) | **中文** | [文档索引](../README.md)

这页给会直接调用 Beetle 接口的人看，比如自己写配置页、脚本，或者要把别的系统接进来的人。

如果你只是想完成首次配网和基础设置，先看 [configuration.md](configuration.md) 就够了。

这是一份技术参考，不是普通用户的入门文档。

这页重点回答三类问题：

- 什么时候要配对码，什么时候不用
- 每个接口读什么、写什么
- 如果自己写页面或脚本，应该怎么调

## 网络与访问

- ESP 固件首次上电后会开一个名为 **Beetle** 的热点，不设密码。连上后用 **http://192.168.4.1** 访问。
- Linux 小板如果系统当前已经连上一个有效的 WiFi，Beetle 会直接继承这条连接；这时请直接使用设备当前的局域网 IP。
- Linux 小板只有在当前没有有效 WiFi 连接时，才会进入 Beetle 自带热点 / 配网兜底路径。
- 接口支持浏览器跨域调用。`/api/*` 和 `GET /` 会带 `Access-Control-Allow-Origin: *`；`OPTIONS` 预检返回 200，并带 `Access-Control-Allow-Methods: GET, POST, DELETE, OPTIONS`、`Access-Control-Allow-Headers: Content-Type, X-Pairing-Code, X-CSRF-Token` 等头，方便外部配置页直接访问。

## 配对码和访问规则

### 术语

- **未激活**：设备里还没有保存有效的 6 位配对码。
- **已激活**：已成功执行过 `POST /api/pairing_code`。
- **仅已激活**：设备必须已经激活，但这次请求本身不要求在 query 或 header 里再带配对码。
- **写操作**：已经激活后，凡是会改配置、改状态或删除内容的接口，通常都需要 **配对码 + CSRF**。

### 未激活时可用的请求

- 任意 **OPTIONS**。
- **GET /**：302 至配对相关页面（`Location: /pairing`），**不**返回 JSON。
- **GET /wifi**：设备内嵌配置页 HTML。
- **GET /pairing**、**GET /common.css**、**GET /common.js**。
- **GET /api/pairing_code**、**POST /api/pairing_code**（仅首次写入配对码）。
- **GET /api/wifi/scan**、**GET /api/csrf_token**（未激活也可调用）。
- 通道平台回调：**POST /api/feishu/event**、**POST /api/dingtalk/webhook**、**GET/POST /api/wecom/webhook**、**POST /api/webhook/qq**（QQ 依赖构建/环境开关）。

除了上面这些，其他路径在未激活时通常都会返回 401。不同语言环境下，错误文案可能会略有区别。

### 已激活、只读 API（请求中不必带配对码）

下面这些接口在**设备已经激活**后，请求里**不用再带** `?code=` 或 `X-Pairing-Code`。这里说的“无需配对码”，是指这次请求不用附带，不是指设备没激活也能访问。

**GET /**、**GET /api/config**、**GET /api/config/accounts**、**GET /api/config/hardware**、**GET /api/config/audio**、**GET /api/config/display**、**GET /api/health**、**GET /api/metrics**、**GET /api/resource**、**GET /api/tools**、**GET /api/diagnose**、**GET /api/system_info**、**GET /api/channel_connectivity**、**GET /api/sessions**、**GET /api/memory/status**、**GET /api/skills**、**GET /api/soul**、**GET /api/user**；启用 `ota` 时另有 **GET /api/ota/check**。

如果设备还没激活，访问上述接口就是 401。

### 会改动内容的接口：配对码 + CSRF

设备已经激活后，下面这类 **POST** 和 **DELETE** 请求还需要带两样东西：

1. **配对码**：`?code=<6位数字>` 和/或 `X-Pairing-Code: <6位数字>`。
2. **CSRF**：`X-CSRF-Token`（或 `x-csrf-token`）为 **GET /api/csrf_token** 返回体中的 `csrf_token`；缺失或无效 → **403**。

包括但不限于：`POST /api/config/wifi`、`/api/config/llm`、`/api/config/channels`、`/api/config/system`、`/api/config/accounts`、`/api/config/hardware`、`/api/config/audio`、`/api/config/display`；**POST**/**DELETE /api/skills**、**POST /api/skills/import**；**POST /api/soul**、**POST /api/user**；**DELETE /api/sessions**；**POST /api/restart**、**POST /api/config_reset**、**POST /api/webhook**；以及启用 OTA 时的 **POST /api/ota**。

**例外**：**POST /api/pairing_code** 只在未激活时可用，不需要配对码或 CSRF；各聊天平台的回调接口走各自平台的签名或 token 规则，也不走这里这一套。

### GET /api/csrf_token

- **鉴权**：无。
- **响应**：200，`{"csrf_token":"<token>"}`。

### 恢复出厂

**POST /api/config_reset** 需要配对码 + CSRF。成功后会清掉配置和配对码，设备回到未激活状态。实现上会删除 `config/skills_meta.json`、`config/llm.json`、`config/channels.json`、`config/accounts.json`、`config/office_credentials.json`、`config/hardware.json`、`config/audio.json`、`config/display.json`，并清理 `runtime/office_runtime_status.json` 等派生状态文件。

---

## 根路径与探测

### GET /

- **未激活**：302，`Location: /pairing`。
- **已激活**：200 JSON，`name` 固定为 **`beetle`**，`version` 为固件版本，`endpoints` 为字符串数组。
- **说明**：`endpoints` 适合用来做简单探测，但不要把它当成完整接口清单。真正对接时，以这份文档为准。

**示例**（字段与顺序以程序实际返回为准）：

```json
{
  "name": "beetle",
  "version": "0.1.0",
  "endpoints": ["GET /pairing", "GET /wifi", "GET /api/pairing_code"]
}
```

### GET /api/pairing_code

- **用途**：查询是否已设置配对码（不返回明文）；并返回当前界面语言。
- **响应**：200，`{"code_set":true|false,"locale":"zh"|"en"}`（`locale` 以固件为准）。
- **鉴权**：未激活亦可调用。

### POST /api/pairing_code

- **用途**：首次设置 6 位配对码（仅未激活）。
- **请求**：`Content-Type: application/json`，Body `{"code": "123456"}`。
- **响应**：成功 200 `{"ok": true}`；已设置过或格式错误 400。
- **鉴权**：不要求配对码/CSRF。

### GET /pairing

- **用途**：配对页 HTML。
- **响应**：200，`Content-Type: text/html; charset=utf-8`。

### GET /wifi

- **用途**：配置页 HTML（设备内嵌）。
- **响应**：200，`Content-Type: text/html; charset=utf-8`。

## 配置读写

配置不会只放在一个地方。比较小的项目，比如 WiFi、代理、会话条数、群组触发和界面语言，放在 NVS；大模型、通道、办公账户、硬件、音频、显示和技能元信息分别放在 `config/*.json`。`GET /api/config` 会把这些内容合并后一次性返回。

### GET /api/wifi/scan

- **用途**：由设备扫描周边 WiFi，返回 SSID 列表供配置页下拉选择。
- **鉴权**：未激活亦可调用；请求**不必**附带配对码。
- **响应**：200，JSON 数组 `[{ "ssid": "MyWiFi", "rssi": -50 }, ...]`，按信号强度（rssi）降序。扫描不可用（非 ESP 或 WiFi 未就绪）时 503。

### GET /api/config

- **用途**：读取当前完整配置。注意，返回内容里会包含真实密钥。
- **鉴权**：已激活；GET **不必**附带配对码。
- **响应**：200，JSON 为 `AppConfig` 序列化，各字段为实际存储值。
- **多大模型来源**：`llm_sources` 是数组，每一项包含 `provider`、`api_key`、`model`、`api_url`、`max_tokens`。如果是旧配置，加载时会自动补成单来源格式。可选的 **`llm_router_source_index`** 和 **`llm_worker_source_index`** 用来调整尝试顺序；如果设置有效，就先试这两个，再按列表顺序试其他来源。全局流式开关是 `llm_stream`。

### POST /api/config/llm

- **用途**：只更新大模型配置这一段，写入 `config/llm.json`。
- **鉴权**：已激活 + 配对码 + CSRF（要求同本节「写操作：配对码 + CSRF」）。
- **请求**：`Content-Type: application/json`。请求体要把这一整段完整传上来，例如 `{ "llm_sources": [...], "llm_stream": false, "llm_router_source_index": null, "llm_worker_source_index": null }`。后两项可选，省略等同 `null`。`llm_sources` 不能为空，每一项都必须带 `api_key`。每项可包含：
  - `provider`（必填，非空字符串；长度等校验见 `config` 模块。常见值含 `anthropic`、`openai`、`openai_compatible`、`gemini`、`glm`、`qwen`、`deepseek`、`moonshot`、`ollama` 等；完整说明见 [大模型服务商](llm-providers.md)）
  - `api_key`（必填）
  - `model`（必填）
  - `api_url`（必填字段；若 `provider` 属于 OpenAI 兼容族且留空，则由客户端使用各厂商默认 base URL，见 `build_llm_clients`）
  - `max_tokens`（可选 u32，默认 null；null 时各客户端使用内置默认值 1024）
- **校验**：这里只校验大模型配置这一段。要求包括：`llm_sources` 非空；字段长度满足限制（provider/api_key/model ≤ 64，api_url ≤ 256）；`llm_stream` 必须是布尔值；`llm_router_source_index` 和 `llm_worker_source_index` 如果存在，必须落在 `llm_sources` 的下标范围内。
- **响应**：成功 200 `{"ok": true}`；校验失败 400。

### POST /api/config/channels

- **用途**：只更新通道配置，写入 `config/channels.json`。
- **鉴权**：已激活 + 配对码 + CSRF。
- **请求**：`Content-Type: application/json`。请求体要把通道配置整段一次性传完整，格式和 `ChannelsSegment` 一致。除了常见字段，也支持 **`wecom_token`**、**`wecom_encoding_aes_key`**、**`dingtalk_app_secret`** 等。完整键名以 `config.rs` 里的 `ChannelsSegment` 为准。
- **校验**：这里只校验通道配置这一段的字段长度，例如 tg/feishu/wecom/qq 等 ≤ 64，`dingtalk_webhook_url` ≤ 512，`wecom_default_touser` ≤ 128。
- **响应**：成功 200 `{"ok": true}`；校验失败 400。

### POST /api/config/system

- **用途**：只更新系统配置这一段，包括 WiFi、代理、会话条数、群组触发和界面语言。
- **鉴权**：已激活 + 配对码 + CSRF。
- **请求**：`Content-Type: application/json`。请求体包含 `wifi_ssid`、`wifi_pass`、`proxy_url`、`session_max_messages`（1～128）、`tg_group_activation`（`"mention"` 或 `"always"`）、`locale`（可选，`"zh"` 或 `"en"`）。
- **校验**：这里只校验系统配置这一段。WiFi 字段长度 ≤ 64；`proxy_url` 可以为空，或者写成 `http://host:port`；`session_max_messages` 和 `tg_group_activation` 也要符合上面的范围和取值。
- **响应**：成功 200 `{"ok": true}`；校验失败 400。
- **说明**：WiFi 写入后需重启生效。

### GET /api/config/accounts

- **用途**：读取办公账户配置，也就是 `config/accounts.json` 的内容。
- **鉴权**：已激活；GET **不必**附带配对码。
- **响应**：200，JSON 为 `OfficeAccountsSegment`。文件不存在时返回空默认值：
  - `registry.accounts`：以 `account_key` 为稳定主键的账户注册表
  - `binding.capability_defaults`：各 capability 的默认账户
  - `policy`：全局默认账户、歧义策略、身份偏好
- **说明**：这是原始配置段接口，适合配置页和脚本整段读写；如果是 Agent 要代用户完成 inspect / draft / validate / commit / revoke / probe，应优先走 `office_config` 工具，而不是直接编辑这段 JSON。
- **补充**：provider 需要哪些字段，不应由前端或调用方硬编码猜测；应该通过 `office_config {"op":"provider_schema", ...}` 查询结构化合同。
- **补充**：通过 `office_config` 走受控配置路径时，后端会按 provider schema 自动裁剪字段、补默认值并拒绝合同外 metadata key；这个原始段接口不承担这层受控归一化。

### POST /api/config/accounts

- **用途**：写入办公账户配置，保存到 `config/accounts.json`。请求体要把这一整段完整传上来。
- **鉴权**：已激活 + 配对码 + CSRF。
- **请求**：`Content-Type: application/json`，Body 为 `OfficeAccountsSegment`。
- **校验**（仅本段）：
  - 账户总数不能超过实现限定上限
  - 每个账户都必须有非空 `account_key`、`provider_kind`
  - 每个账户必须声明至少一个 `enabled_capabilities`
  - `binding.capability_defaults` 指向的账户必须存在，并且该账户必须启用对应 capability
  - `policy.global_default_account_key` 如果存在，必须能在注册表中找到
- **响应**：成功 200 `{"ok": true}`；校验失败 400。

### GET /api/config/office_credentials

- **用途**：读取办公凭证权威层，也就是 `config/office_credentials.json` 的内容。
- **鉴权**：已激活；GET **不必**附带配对码。
- **响应**：200，JSON 为 `OfficeCredentialsSegment`：
  - `items[].account_key`：账户主键，和 `/api/config/accounts` 里的注册表对齐
  - `items[].access_token` / `refresh_token` / `token_endpoint`：受控凭证字段
  - `items[].metadata`：账户补充元数据；provider-specific key 仍然落在这里，但调用方不应该把这份文档当成所有 provider 字段合同的唯一真源。具体字段要求请通过 `office_config {"op":"provider_schema", ...}` 查询。
- **说明**：这是 office 域共享凭证层，不再由 `calendar` 私有维护自己的 credential 真相。
- **补充**：`mail`、`calendar`、`documents` 都消费这层共享凭证；调用方不需要再为每个能力维护一份单独的私有凭证文件。
- **补充**：运行态 probe/错误状态不在这个接口里，运行派生真相由 `runtime/office_runtime_status.json` 承载，并通过 `office_status` / `office_config probe` 这类上层能力消费。
- **补充**：如果调用方需要 provider-aware 的受控配置合同，请不要把这里的 `metadata` 当成完整字段字典，而是通过 `office_config {"op":"provider_schema", ...}` 获取。

### POST /api/config/office_credentials

- **用途**：写入办公凭证权威层，保存到 `config/office_credentials.json`。请求体要把这一整段完整传上来。
- **鉴权**：已激活 + 配对码 + CSRF。
- **请求**：`Content-Type: application/json`，Body 为 `OfficeCredentialsSegment`。
- **校验**（仅本段）：
  - 每个 `items[].account_key` 都必须非空
  - 不允许重复 `account_key`
  - 采用严格 JSON 解析；尾随垃圾、半截 JSON、重复记录都直接报错，不做兼容性兜底
- **响应**：成功 200 `{"ok": true}`；校验失败 400。

### GET /api/config/hardware

- **用途**：读取当前硬件配置，也就是 `config/hardware.json` 的内容。
- **鉴权**：已激活；GET **不必**附带配对码。
- **响应**：200，JSON 为 `HardwareSegment`：`{ "hardware_devices": [...] }`。文件不存在时返回 `{ "hardware_devices": [] }`。
- **说明**：GET 返回的是文件里的原始内容。如果启动时校验没过，程序会退回空设备列表，同时 `load_errors` 里会出现 `hardware_validation_failed`。

### POST /api/config/hardware

- **用途**：写入硬件配置，保存到 `config/hardware.json`。请求体要把这一整段完整传上来。改完后重启生效。如果重启后校验失败，`load_errors` 会包含 `hardware_validation_failed`。详细规则见 [硬件设备配置](hardware-device-config.md)。
- **鉴权**：已激活 + 配对码 + CSRF。
- **请求**：`Content-Type: application/json`，Body 为 `HardwareSegment`：
  ```json
  {
    "hardware_devices": [
      {
        "id": "板载LED",
        "device_type": "gpio_out",
        "pins": { "pin": 2 },
        "what": "板载指示灯，可开关",
        "how": "传 value：1=亮，0=灭"
      }
    ]
  }
  ```
  每项 `DeviceEntry` 包含 `id`、`device_type`、`pins`、`what`、`how`，以及可选的 `options`。
- **校验**（只针对硬件配置这一段）：
  - 设备总数 ≤ 8
  - `id` 非空且 ≤ 32 字节，不得重复
  - `device_type` 须为 `gpio_out` / `gpio_in` / `pwm_out` / `adc_in` / `buzzer` 之一
  - `what` ≤ 128 字节，`how` ≤ 256 字节
  - `pins` 须含 `"pin"` 键；引脚值 1–48，不得为 strapping 引脚（0, 3, 46），不得跨设备冲突
  - `adc_in` 引脚须在 ADC1 范围（GPIO 1–10）
  - `pwm_out` 设备总数 ≤ 4；`options.frequency_hz` 若存在须在 1–40000
- **响应**：成功 200 `{"ok": true}`；校验失败 400。

### GET /api/config/audio

- **用途**：读取当前音频配置，也就是 `config/audio.json` 的内容。
- **鉴权**：已激活；GET **不必**附带配对码。
- **响应**：200，JSON 为 `AudioSegment`。文件不存在时返回 disabled 默认配置（`enabled=false`）。

### POST /api/config/audio

- **用途**：写入音频配置，保存到 `config/audio.json`。请求体要把这一整段完整传上来。改完后重启生效。
- **鉴权**：已激活 + 配对码 + CSRF。
- **请求**：`Content-Type: application/json`，Body 为 `AudioSegment`，结构见 [`voice-interaction-plan.md`](../../dev-docs/voice-interaction-plan.md) 的 `config/audio.json` 示例（含 `microphone`、`speaker`、`vad`、`wake_word`、`speech`、`tts`、`realtime`、`ambient_listening`、`led_indicator`）。
- **校验**（仅本段）：
  - `version` 必须为 `1`
  - 启用的 microphone/speaker 引脚需在 1～48；采样率 8000～48000；位深 16/24/32
  - microphone `buffer_size` 需在 256～16384
  - 当启用唤醒词链路且未配置 realtime voice 时，当前回退语音服务商若为 `baidu`，则 `speech.api_key` 与 `speech.api_secret` 必填
  - realtime voice 的 `realtime.provider` 当前仅支持 `openai_compatible`、`qwen`、`doubao`
  - realtime voice 启用时，`realtime.api_key`、`realtime.model`、`realtime.voice`、`realtime.ws_url` 必填，且麦克风/喇叭采样率都必须为 `24000`
  - 固件只校验字段存在性和基础格式，不会把服务商音色目录硬编码成保存拦截；后续若厂商调整可用音色，应通过前端默认值或用户配置覆盖
  - `vad.threshold` 需在 [0,1]；`silence_duration_ms` 需在 1～60000
  - `ambient_listening.sound_events` 最多 16 项，每项 1～32 字符；`check_interval_seconds` 需在 1～86400
- **响应**：成功 200，`{"ok": true, "restart_required": true}`；校验失败 400。

### POST /api/config/wifi

- **用途**：仅将 WiFi SSID/密码写入 NVS，供配置页单独配网场景。
- **鉴权**：已激活 + 配对码 + CSRF。
- **请求**：`Content-Type: application/json`，Body `{"wifi_ssid":"...","wifi_pass":"..."}`；字段长度 ≤ 64。
- **响应**：成功 200，`{"ok": true, "restart_required": true}`；校验失败 400。
- **说明**：WiFi 写入后要重启才会真正生效。也可以在请求里带 `?restart=1`，保存成功后自动重启。

### GET /api/soul

- **用途**：获取当前 SOUL（人格）配置内容，供外置配置页回显或编辑。
- **鉴权**：已激活；GET **不必**附带配对码。
- **响应**：200，`Content-Type: text/plain`，Body 为 SOUL 文件全文（UTF-8）；读失败 500，`{"error":"..."}`。

### POST /api/soul

- **用途**：提交 SOUL 内容并写入 SPIFFS（config/SOUL.md）。
- **鉴权**：已激活 + 配对码 + CSRF。
- **请求**：Body 为纯文本或 JSON `{"content": "..."}`；长度 ≤ 32KB（MAX_SOUL_USER_LEN）。
- **响应**：成功 200，`{"ok": true}`；超长或非法 UTF-8 返回 400；写入失败 500。

### GET /api/user

- **用途**：获取当前 USER（用户信息）配置内容。
- **鉴权**：已激活；GET **不必**附带配对码。
- **响应**：200，`Content-Type: text/plain`，Body 为 USER 文件全文；读失败 500。

### POST /api/user

- **用途**：提交 USER 内容并写入 SPIFFS（config/USER.md）。
- **鉴权**：已激活 + 配对码 + CSRF。
- **请求**：同 POST /api/soul（纯文本或 `{"content":"..."}`，≤ 32KB）。
- **响应**：同 POST /api/soul。

### GET /api/sessions

- **用途**：分页列出会话 ID，或者查看某一个会话最近的消息。
- **鉴权**：已激活；GET **不必**附带配对码。
- **查询参数**：
  - 无 `chat_id`：分页列表。支持 **`page`**（默认 1）、**`limit`**（默认 20，最大 100）。响应 200，JSON：`{"items":["chat_id1",...],"total":N,"page":1,"limit":20,"total_pages":...}`。
  - query 中带 **`chat_id`** 或 **`name`**（二者任一，值为会话 id）：返回该会话最近消息 JSON，最多约 50 条；失败 500，`{"error":"..."}`。

### GET /api/memory/status

- **用途**：返回 memory operator 视图，聚合记忆存储、人格连续性、continuity tooling、task learning / execution，以及按会话下钻的 deep inspection。
- **鉴权**：已激活；GET **不必**附带配对码。
- **响应**：200，JSON 对象，顶层包含 `memory_system_kind`、`memory_len`、`soul_len`、`user_len`、`long_term_count`、`continuity_capsule_count`、`stores`、`personality`、`continuity_tooling`、`continuity_capsules`、`task_execution`、`learning`，以及可选的 `inspection`。
- **职责**：该接口返回内存与会话学习相关的 operator 视图；队列深度看 `GET /api/resource`，会话列表与明细看 `GET /api/sessions`。

### GET /api/tools

- **用途**：返回当前可以列举出来的工具名和简短说明，方便配置页或脚本探测能力。
- **鉴权**：已激活；GET **不必**附带配对码。
- **响应**：200，JSON 数组 `[{"name":"get_time","description":"..."}, ...]`。返回条目会随着 **`tools_network_extra`**、**`tools_diagnostics`** 等编译选项变化，也不一定等于程序最终实际加载的全部工具。完整说明见 [工具说明](tools.md)。

## Skills

### GET /api/skills

- **用途**：读取技能列表，或者读取某一个技能的内容。不给 query 时返回列表和顺序；带 `?name=xxx` 时返回对应技能的纯文本内容，方便配置页回显。
- **鉴权**：已激活；GET **不必**附带配对码。
- **响应（无 name）**：200，JSON `{"skills": [{"name": "x", "enabled": true}, ...], "order": ["a", "b"]}`。
- **响应（name=xxx）**：200，`Content-Type: text/plain`，Body 为 skill 内容；不存在 404。

### POST /api/skills

- **用途**：更新启用状态、写入内容，或者调整顺序。具体做哪件事，由请求体的形状决定。
- **鉴权**：已激活 + 配对码 + CSRF。
- **请求**：`Content-Type: application/json`。
  - 仅更新启用：`{"name": "x", "enabled": true|false}`。
  - 写入/覆盖 skill：`{"name": "x", "content": "..."}`；content 长度 ≤ 32KB。
  - 仅更新顺序：`{"order": ["a", "b", "c"]}`。
- **响应**：成功 200，`{"ok": true}`；失败 400/500。

### DELETE /api/skills?name=xxx

- **用途**：删除指定 skill 文件。query 必带 `name`。
- **鉴权**：已激活 + 配对码 + CSRF。
- **响应**：成功 200，`{"ok": true}`；name 非法 400；文件不存在 404。

### POST /api/skills/import

- **用途**：从 URL 拉取内容并保存成新的技能文件。
- **鉴权**：已激活 + 配对码 + CSRF。
- **请求**：`Content-Type: application/json`，Body `{"url": "https://...", "name": "xxx"}`。url 须为 http(s)；name 合法（无 `..`、`/`、`\`）。
- **响应**：成功 200，`{"ok": true}`；url 拉取失败 502/500；body 非 UTF-8 或超长 400。

### POST /api/webhook

- **用途**：让外部系统通过 HTTP POST 送一条新消息进来。请求体会作为消息内容进入入站队列，再按正常流程处理。
- **鉴权**：已激活 + 配对码 + CSRF；并须通过本条 **webhook token** 校验。
- **配置**：需在配置中设置 `webhook_enabled: true` 且 `webhook_token` 非空；否则返回 403。
- **校验**：在配对码与 CSRF 通过后，请求还须携带与配置一致的 token：Header `X-Webhook-Token` 或 query 参数 `token`；不匹配返回 401。
- **请求**：Body 为任意 UTF-8 文本，作为入站消息 content；上限 4KB。
- **响应**：
  - 成功：200，`{"ok": true}`。
  - webhook 未启用或 token 为空：403，`{"error": "webhook disabled"}`。
  - token 不匹配：401，`{"error": "invalid token"}`。
  - Body 非 UTF-8 或超长：400/413。
  - 入站队列满：503，`{"error": "queue full"}`。

## 健康与运维

### GET /api/health

- **用途**：返回轻量健康摘要，供首页与状态卡片直接读取。
- **鉴权**：已激活；GET **不必**附带配对码。
- **响应**：200，JSON 示例（字段与 serde 结构体一致）：
  ```json
  {
    "wifi": "connected",
    "last_error": "none",
    "display": { "available": true },
    "audio": {
      "duplex_profile": "FullDuplex",
      "duplex_capabilities": {
        "input_available": true,
        "output_available": true,
        "full_duplex_available": true
      }
    }
  }
  ```
  - `wifi`：`"connected"` 或 `"disconnected"`。这里指的是设备有没有真正连上上游网络；如果你只是连着设备自己的热点，也可能显示 `disconnected`。
  - `last_error`：最近一次错误摘要（仅 stage/message，无密钥）；无则为 `"none"`。
  - `display.available`：显示子系统是否可用。
  - `audio.duplex_profile` / `audio.duplex_capabilities`：音频收放能力摘要。
  - **职责边界**：该接口只保留轻量健康摘要；统计计数归 `GET /api/metrics`，资源/队列/预算归 `GET /api/resource`，设备身份摘要归 `GET /api/system_info`。

### GET /api/diagnose

- **用途**：执行一轮设备自检，返回结构化结果列表，方便配置页里的“设备状态”页面直接展示。
- **鉴权**：已激活；GET **不必**附带配对码。
- **响应**：200，JSON 数组，每项含 `severity`、`category`、`message`：
  ```json
  [
    { "severity": "ok", "category": "storage", "message": "storage readable" },
    { "severity": "ok", "category": "storage", "message": "spiffs total=... used=... free=..." },
    { "severity": "ok", "category": "config", "message": "nvs accessible" },
    { "severity": "warn", "category": "config", "message": "wifi disconnected" },
    { "severity": "ok", "category": "channel", "message": "inbound_depth=0 outbound_depth=0" },
    { "severity": "warn", "category": "channel", "message": "last_error: ..." }
  ]
  ```
  - `severity`：`"ok"` | `"warn"` | `"error"`。
  - `category`：`"storage"` 表示存储，`"channel"` 表示消息通道，`"config"` 表示基础配置和网络状态。
  - `message`：人类可读说明；`last_error` 摘要截断至 200 字符。

### GET /api/operator/status

- **用途**：返回 operator 面的宿主状态，用于观察 OS/runtime/host 合同、系统闭环、presence / initiative、runtime mode、soul kernel、runtime capabilities，以及 Linux 上的 supervisor / release。
- **鉴权**：已激活；GET **不必**附带配对码。
- **职责**：该接口返回 operator 面的运行态与宿主面信息；队列深度看 `GET /api/resource`，错误摘要看 `GET /api/health`，设备身份与存储介质看 `GET /api/system_info`。

### GET /api/metrics

- **用途**：导出统计快照。默认返回 JSON；如果带 query **`format=prometheus`**，就返回 Prometheus 文本格式。
- **鉴权**：已激活；GET **不必**附带配对码。

### GET /api/resource

- **用途**：返回当前资源状态，聚焦运行压力、队列、会话、存储占用与运行预算。
- **鉴权**：已激活；GET **不必**附带配对码。
- **职责**：该接口返回资源、压力、队列、会话与运行预算；通道现场状态看 `GET /api/channel_connectivity`。
- **关键字段**：
  - `pressure`：编排器给出的整体压力等级。
  - `tls_fragmentation_risk`：ESP 侧依据 internal heap 最大连续空闲块计算出的出站 TLS 碎片风险，取值为 `not_applicable`、`healthy`、`cautious`、`critical`。
  - `heap_largest_block_internal`：internal heap 最大连续空闲块字节数；Linux 上固定为 `0`，表示 `N/A`。

### GET /api/system_info

- **用途**：返回设备摘要与构建信息。
- **鉴权**：已激活；GET **不必**附带配对码。
- **职责**：该接口返回设备身份与设备摘要；`wifi` / `last_error` 看 `GET /api/health`，`pressure` / 队列看 `GET /api/resource`。

### GET /api/channel_connectivity

- **用途**：返回各聊天通道的连通性探测结果。
- **鉴权**：已激活；GET **不必**附带配对码。
- **ESP 行为**：当 WiFi STA 尚未稳定，或 orchestrator 判断 `tls_fragmentation_risk` 已到 `cautious` / `critical` 时，该接口返回一份 stale-unavailable 快照，而不是强行发起新的 HTTP/TLS live probe；这样诊断面不会和生产流量争抢同一份 TLS 准入预算。

### GET /api/config/display

- **用途**：读取显示配置。如果当前固件没有启用显示功能，这个接口不会提供有效内容。
- **鉴权**：已激活；GET **不必**附带配对码。

### POST /api/config/display

- **用途**：写入显示配置。部分修改和 WiFi 一样，也支持 `?restart=1`。
- **鉴权**：已激活 + 配对码 + CSRF。

### DELETE /api/sessions?chat_id=...

- **用途**：删除指定会话。
- **鉴权**：已激活 + 配对码 + CSRF；query **必须**含 `chat_id`。

### 通道回调（无配对码 / 无 CSRF）

下面这些接口是给聊天平台服务器回调用的。它们走各自平台的签名或 token 校验，**不走**设备自己的配对码 + CSRF：

- **POST /api/feishu/event**
- **POST /api/dingtalk/webhook**
- **GET /api/wecom/webhook**（URL 验证）、**POST /api/wecom/webhook**
- **POST /api/webhook/qq**（未启用 QQ 通道时可能 404）

### POST /api/restart

- **用途**：让设备重启，好让新配置真正生效。
- **鉴权**：已激活 + 配对码 + CSRF。
- **响应**：先返回 200，`{"ok": true}`，随后在约 100～500ms 内设备重启。
- **补充行为**：在真正重启前，运行时会先尝试把最近活跃会话刷成 continuity bundle，写入状态根下的 runtime continuity snapshot 文件，供 handoff / reboot 恢复使用；flush 失败不会阻止重启。
- **节流**：60 秒内仅允许一次有效重启。

### GET /api/ota/check

- **用途**：按当前板型和更新渠道，查询有没有可用的新固件。只有在启用了 `ota` 编译选项时，这个接口才存在。
- **鉴权**：已激活；GET **不必**附带配对码。
- **请求**：GET，可选 query `channel`（默认 `stable`）。
- **响应**：200 JSON。字段包括：`current_version`（当前固件版本）、`latest_version`（渠道最新版本，有渠道时）、`update_available`（是否有可升级版本）、`url`（有更新时的固件下载 URL）、`release_notes`（可选）、`error`（可选，面向用户的错误说明，比如渠道未配置或拉取失败）。如果清单未配置，或者拉取、解析失败，也仍然返回 200，并把 `update_available` 设为 `false`。

### POST /api/ota

- **用途**：从指定 URL 下载固件并执行 OTA 更新。成功后设备会自动重启。只有启用了 `ota` 编译选项时，这个接口才存在。
- **鉴权**：已激活 + 配对码 + CSRF。
- **请求**：`Content-Type: application/json`，Body 为 `{"url": "https://..."}`；url 须非空且为 `http://` 或 `https://` 开头。
- **响应**：成功 200，`{"ok": true}`，随后设备执行 OTA 并在完成后重启；无效或缺失 url 返回 400，`{"error": "invalid url"}`；OTA 下载、校验或写入失败返回 500，`{"error": "面向用户的错误说明"}`（如「网络或下载失败，请检查网络后重试」「固件校验失败，请更换固件来源」「写入失败，请勿断电并重试」）。响应带 CORS 头。
- **说明**：如果更新失败，不会破坏当前正在运行的固件，设备还能继续用。你可以重试，也可以换一个 URL。

**OTA 清单格式**：CI/Release 会把更新清单发到 `OTA_MANIFEST_URL`。JSON 根对象里有 `boards`，键是板型 ID（`esp32-s3-8mb`、`esp32-s3-16mb`、`esp32-s3-32mb`），值是各更新渠道的信息；每个渠道（比如 `stable`）包含 `version`、`url`，以及可选的 `release_notes`。设备构建时通过 `BOARD` 和 `OTA_MANIFEST_URL` 来确定自己该查哪一份清单。

### POST /api/config_reset

- **用途**：恢复出厂。它会清空 NVS 里的配置，并删除 `config/llm.json`、`config/channels.json`、`config/hardware.json`、`config/audio.json`、`config/display.json`、`config/skills_meta.json` 等文件，效果和 CLI 的 `config_reset yes` 一样。
- **鉴权**：已激活 + 配对码 + CSRF。
- **响应**：成功 200，`{"ok": true}`；失败 500，`{"error": "reset failed"}`。
- **说明**：调用后建议立刻重启。重启后，程序只会从环境变量重新加载默认配置；NVS 里只保留少量基础键，其余配置都来自 `config/*.json`。

## 如何获知板子 IP

- 连接设备热点 **Beetle** 时：使用 **http://192.168.4.1**（固件 SoftAP 固定地址）。
- 已连 STA 且与设备在同一 LAN 时：使用路由器分配给设备的 IP。

## 配置页归属

设备自带的页面包括 **`GET /wifi`**（配置页 HTML）、**`GET /pairing`**、**`GET /common.css`**、**`GET /common.js`** 等。你也可以使用仓库里的 **`configure-ui`**，或者自己单独部署一个静态站点，只要调用的是同一套接口就可以。
