# 配置 API 契约 / Config API Contract

[English](../en-us/config-api.md) | **中文** | [文档索引](../README.md)

这篇文档是写给要直接调用设备 HTTP 接口的开发者的。

你一般会在下面这些场景里用到它：

- 想搞清楚配对码和 CSRF 规则
- 想确认每个读写接口到底怎么工作
- 想自己写前端、脚本或第三方集成

如果你只是想先把设备跑起来、打开配置页、完成配网，那先看 [configuration.md](configuration.md) 会更合适。

## 网络与访问

- **SoftAP**：设备上电后开启热点，SSID 固定为 **Beetle**（无密码）。连接该热点后使用 **http://192.168.4.1**（与固件一致）。
- **STA**：若已配置 WiFi 并连接用户路由器，同一 HTTP 服务在 STA 网段也可访问，使用路由器分配的 LAN 地址。
- **CORS**：所有 `/api/*` 及 `GET /` 的响应应带 `Access-Control-Allow-Origin: *`；OPTIONS 预检返回 200，并带 `Access-Control-Allow-Methods: GET, POST, DELETE, OPTIONS`、`Access-Control-Allow-Headers: Content-Type, X-Pairing-Code, X-CSRF-Token`（及大小写变体），以便外置配置页跨域调用。

## 配对码与鉴权

权威实现：[`dispatch.rs`](../../src/platform/http_server/router/dispatch.rs)、[`auth.rs`](../../src/platform/http_server/router/auth.rs)。

### 术语

- **未激活**：NVS 中尚未保存有效 6 位配对码。
- **已激活**：已成功执行过 `POST /api/pairing_code`。
- **仅已激活**：设备必须已经激活，但这次请求本身**不要求**在 query/header 中带配对码（实现为 `require_activated`）。
- **写操作**：设备激活后，变更状态或配置的接口通常都需要 **配对码 + CSRF**；具体哪些接口算写操作、哪些是例外，后面会列出来。

### 未激活时可用的请求

- 任意 **OPTIONS**。
- **GET /**：302 至配对相关页面（`Location: /pairing`），**不**返回 JSON。
- **GET /wifi**：设备内嵌配置页 HTML（路径历史原因；**没有** `GET /config`）。
- **GET /pairing**、**GET /common.css**、**GET /common.js**。
- **GET /api/pairing_code**、**POST /api/pairing_code**（仅首次写入配对码）。
- **GET /api/wifi/scan**、**GET /api/csrf_token**（未激活也可调用）。
- 通道平台回调：**POST /api/feishu/event**、**POST /api/dingtalk/webhook**、**GET/POST /api/wecom/webhook**、**POST /api/webhook/qq**（QQ 依赖构建/环境开关）。

其余路径在未激活下通常都会返回 401。至于具体 JSON 文案，会随固件 locale 略有差异。

### 已激活、只读 API（请求中不必带配对码）

以下接口在**已激活**后，**无需** `?code=` 或 `X-Pairing-Code`（与部分用户文档中「无需配对码」同义：**指请求不必附带码**，不是指未激活即可访问）：

**GET /**、**GET /api/config**、**GET /api/config/hardware**、**GET /api/config/audio**、**GET /api/config/display**、**GET /api/health**、**GET /api/metrics**、**GET /api/resource**、**GET /api/tools**、**GET /api/diagnose**、**GET /api/system_info**、**GET /api/channel_connectivity**、**GET /api/sessions**、**GET /api/memory/status**、**GET /api/skills**、**GET /api/soul**、**GET /api/user**；启用 `ota` 时另有 **GET /api/ota/check**。

如果设备还没激活，访问上述接口就是 401。

### 写操作：配对码 + CSRF

在已激活前提下，下列 **POST**/**DELETE** 还须：

1. **配对码**：`?code=<6位数字>` 和/或 `X-Pairing-Code: <6位数字>`。
2. **CSRF**：`X-CSRF-Token`（或 `x-csrf-token`）为 **GET /api/csrf_token** 返回体中的 `csrf_token`；缺失或无效 → **403**。

包括但不限于：`POST /api/config/wifi`、`/api/config/llm`、`/api/config/channels`、`/api/config/system`、`/api/config/hardware`、`/api/config/audio`、`/api/config/display`；**POST**/**DELETE /api/skills**、**POST /api/skills/import**；**POST /api/soul**、**POST /api/user**；**DELETE /api/sessions**；**POST /api/restart**、**POST /api/config_reset**、**POST /api/webhook**；**POST /api/ota**（若编译启用）。

**例外**：**POST /api/pairing_code**（仅未激活）不要求配对码/CSRF；各**通道 webhook** 使用平台约定鉴权，也不要求配对码/CSRF。

### GET /api/csrf_token

- **鉴权**：无。
- **响应**：200，`{"csrf_token":"<token>"}`。

### 恢复出厂

**POST /api/config_reset** 须配对码 + CSRF；成功后清除配置与配对码，设备回到未激活状态（实现会删除 SPIFFS 上 `config/skills_meta.json`、`config/llm.json`、`config/channels.json`、`config/hardware.json`、`config/audio.json`、`config/display.json` 等，与 [`config_reset` handler](../../src/platform/http_server/handlers/config_reset.rs) 一致）。

---

## 根路径与探测

### GET /

- **未激活**：302，`Location: /pairing`。
- **已激活**：200 JSON，`name` 固定为 **`beetle`**，`version` 为固件版本，`endpoints` 为字符串数组。
- **权威列表**：以设备返回为准；由 [`handlers/root.rs`](../../src/platform/http_server/handlers/root.rs) 生成。该数组会随版本扩充，但仍可能**未穷尽** [`dispatch.rs`](../../src/platform/http_server/router/dispatch.rs) 中已实现的路由；集成时不应仅以 `GET /` 的 `endpoints` 为唯一依据。截至当前实现，下列路由仍常见于 dispatch 中而**未**列入 `endpoints`（以固件为准）：
  - `POST /api/config/wifi`
  - `GET` / `POST /api/config/display`
  - `GET /api/metrics`、`GET /api/resource`、`GET /api/csrf_token`、`GET /api/tools`
  - `DELETE /api/sessions`
  - `POST /api/dingtalk/webhook`、`GET`/`POST /api/wecom/webhook`、`POST /api/webhook/qq`
  - `GET /common.css`、`GET /common.js`

**示例**（字段与顺序以运行时为准）：

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

**存储策略**：NVS 仅存 6 个小键（WiFi SSID/密码、代理、会话条数、群组触发、界面语言）；LLM 多源与通道配置存 SPIFFS（`config/llm.json`、`config/channels.json`），硬件设备配置存 `config/hardware.json`，音频配置存 `config/audio.json`，显示配置存 `config/display.json`，技能启用/顺序存 `config/skills_meta.json`。GET /api/config 合并 NVS 与 SPIFFS 后返回完整配置。

### GET /api/wifi/scan

- **用途**：由设备扫描周边 WiFi，返回 SSID 列表供配置页下拉选择。
- **鉴权**：未激活亦可调用；请求**不必**附带配对码。
- **响应**：200，JSON 数组 `[{ "ssid": "MyWiFi", "rssi": -50 }, ...]`，按信号强度（rssi）降序。扫描不可用（非 ESP 或 WiFi 未就绪）时 503。

### GET /api/config

- **用途**：获取当前完整配置（含各字段真实值，含密钥类字段）。
- **鉴权**：已激活；GET **不必**附带配对码。
- **响应**：200，JSON 为 `AppConfig` 序列化，各字段为实际存储值。
- **多 LLM 源**：`llm_sources` 为数组，每项含 `provider`、`api_key`、`model`、`api_url`、`max_tokens`（可选 u32，null 时各客户端使用内置默认值 1024）；空时 load 从旧字段构造单源。可选 **`llm_router_source_index`**、**`llm_worker_source_index`**（`u32`，对应 `llm_sources` 下标）：用于调整 worker 内 [`FallbackLlmClient`](../../src/llm/fallback.rs) 的尝试顺序——若设置了有效的 `llm_router_source_index`，则**先该源**，再（若配置且有效）`llm_worker_source_index`，再其余校验通过的源；均未设置时按列表中**有效源**的原有顺序。全局流式开关为 `llm_stream`（LLM 段顶层）。

### POST /api/config/llm

- **用途**：仅写入 LLM 段（多源 + 全局流式开关）到 SPIFFS（`config/llm.json`）；请求体为 segment 全量，后端按 body 校验并写入。
- **鉴权**：已激活 + 配对码 + CSRF（要求同本节「写操作：配对码 + CSRF」）。
- **请求**：`Content-Type: application/json`，Body 为 `{ "llm_sources": [...], "llm_stream": false, "llm_router_source_index": null, "llm_worker_source_index": null }`（后两项可选，省略等同 null）。`llm_sources` 非空；每项 `api_key` 必填。每项可含：
  - `provider`（必填，非空字符串；长度等校验见 `config` 模块。运行时客户端分流见 [`llm/mod.rs`](../../src/llm/mod.rs)：常见值含 `anthropic` 与 `openai`、`openai_compatible`、`gemini`、`glm`、`qwen`、`deepseek`、`moonshot`、`ollama` 等；完整说明见 [LLM 提供商](llm-providers.md)）
  - `api_key`（必填）
  - `model`（必填）
  - `api_url`（必填字段；若 `provider` 属于 OpenAI 兼容族且留空，则由客户端使用各厂商默认 base URL，见 `build_llm_clients`）
  - `max_tokens`（可选 u32，默认 null；null 时各客户端使用内置默认值 1024）
- **校验**：仅本段——`llm_sources` 非空，各字段长度（provider/api_key/model ≤ 64，api_url ≤ 256）；`llm_stream` 为布尔值；`llm_router_source_index` / `llm_worker_source_index` 若存在须在 `llm_sources` 长度范围内（见 `config` 模块 `validate_llm_source_indices`）。
- **响应**：成功 200 `{"ok": true}`；校验失败 400。

### POST /api/config/channels

- **用途**：仅写入通道段（Telegram、飞书、钉钉、企微、QQ 频道、Webhook）到 SPIFFS（`config/channels.json`）；请求体为 segment 全量，后端按 body 校验并写入。
- **鉴权**：已激活 + 配对码 + CSRF。
- **请求**：`Content-Type: application/json`，Body 为通道段全量（与 `ChannelsSegment` 一致），除常见字段外还可含 **`wecom_token`**、**`wecom_encoding_aes_key`**、**`dingtalk_app_secret`** 等；完整键名以固件 `config.rs` 中 `ChannelsSegment` 为准。
- **校验**：仅本段字段长度（tg/feishu/wecom/qq 等 ≤ 64，dingtalk_webhook_url ≤ 512，wecom_default_touser ≤ 128）。
- **响应**：成功 200 `{"ok": true}`；校验失败 400。

### POST /api/config/system

- **用途**：仅写入系统段（WiFi、代理、会话条数、群组触发、界面语言）；请求体为 segment 全量，后端按 body 校验并写入。
- **鉴权**：已激活 + 配对码 + CSRF。
- **请求**：`Content-Type: application/json`，Body 含 `wifi_ssid`、`wifi_pass`、`proxy_url`、`session_max_messages`（1～128）、`tg_group_activation`（`"mention"` 或 `"always"`）、`locale`（可选，`"zh"` 或 `"en"`）。
- **校验**：仅本段——wifi 字段长度 ≤ 64；`proxy_url` 为空或形如 `http://host:port`；`session_max_messages`、`tg_group_activation` 同上。
- **响应**：成功 200 `{"ok": true}`；校验失败 400。
- **说明**：WiFi 写入后需重启生效。

### GET /api/config/hardware

- **用途**：获取当前硬件设备配置段（`config/hardware.json` 内容）。
- **鉴权**：已激活；GET **不必**附带配对码。
- **响应**：200，JSON 为 `HardwareSegment`：`{ "hardware_devices": [...] }`。文件不存在时返回 `{ "hardware_devices": [] }`。
- **说明**：GET 返回文件原始内容。若启动时该校验未通过，运行时使用空设备列表，且 `load_errors` 会包含 `hardware_validation_failed`。

### POST /api/config/hardware

- **用途**：写入硬件设备配置段到 SPIFFS（`config/hardware.json`）；请求体为 segment 全量，后端校验并写入。重启后生效。重启后加载时若校验失败，`load_errors` 将含 `hardware_validation_failed`；完整校验规则见 [硬件设备配置与 LLM 驱动设计](hardware-device-config.md)。
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
  每项 `DeviceEntry` 含 `id`、`device_type`、`pins`、`what`、`how`、可选 `options`。
- **校验**（仅本段）：
  - 设备总数 ≤ 8
  - `id` 非空且 ≤ 32 字节，不得重复
  - `device_type` 须为 `gpio_out` / `gpio_in` / `pwm_out` / `adc_in` / `buzzer` 之一
  - `what` ≤ 128 字节，`how` ≤ 256 字节
  - `pins` 须含 `"pin"` 键；引脚值 1–48，不得为 strapping 引脚（0, 3, 45, 46），不得跨设备冲突
  - `adc_in` 引脚须在 ADC1 范围（GPIO 1–10）
  - `pwm_out` 设备总数 ≤ 4；`options.frequency_hz` 若存在须在 1–40000
- **响应**：成功 200 `{"ok": true}`；校验失败 400。

### GET /api/config/audio

- **用途**：获取当前音频设备配置段（`config/audio.json` 内容）。
- **鉴权**：已激活；GET **不必**附带配对码。
- **响应**：200，JSON 为 `AudioSegment`。文件不存在时返回 disabled 默认配置（`enabled=false`）。

### POST /api/config/audio

- **用途**：写入音频设备配置段到 SPIFFS（`config/audio.json`）；请求体为 segment 全量，后端校验并写入。重启后生效。
- **鉴权**：已激活 + 配对码 + CSRF。
- **请求**：`Content-Type: application/json`，Body 为 `AudioSegment`，结构见 [`voice-interaction-plan.md`](../../dev-docs/voice-interaction-plan.md) 的 `config/audio.json` 示例（含 `microphone`、`speaker`、`vad`、`wake_word`、`stt`、`tts`、`ambient_listening`、`led_indicator`）。
- **校验**（仅本段）：
  - `version` 必须为 `1`
  - 启用的 microphone/speaker 引脚需在 1～48；采样率 8000～48000；位深 16/24/32
  - microphone `buffer_size` 需在 256～16384
  - 当 `enabled=true` 且 `microphone.enabled=true` 且 `stt.provider="baidu"` 时，`stt.api_key` 与 `stt.api_secret` 必填
  - `vad.threshold` 需在 [0,1]；`silence_duration_ms` 需在 1～60000
  - `ambient_listening.sound_events` 最多 16 项，每项 1～32 字符；`check_interval_seconds` 需在 1～86400
- **响应**：成功 200，`{"ok": true, "restart_required": true}`；校验失败 400。

### POST /api/config/wifi

- **用途**：仅将 WiFi SSID/密码写入 NVS，供配置页单独配网场景。
- **鉴权**：已激活 + 配对码 + CSRF。
- **请求**：`Content-Type: application/json`，Body `{"wifi_ssid":"...","wifi_pass":"..."}`；字段长度 ≤ 64。
- **响应**：成功 200，`{"ok": true, "restart_required": true}`；校验失败 400。
- **说明**：WiFi 写入 NVS 后需重启设备生效。可选：请求时带 query `?restart=1`，保存成功后将自动重启（与 `POST /api/restart` 相同逻辑）。

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

- **用途**：分页列出会话 chat_id，或查询单会话最近消息（只读）。
- **鉴权**：已激活；GET **不必**附带配对码。
- **查询参数**：
  - 无 `chat_id`：分页列表。支持 **`page`**（默认 1）、**`limit`**（默认 20，最大 100）。响应 200，JSON：`{"items":["chat_id1",...],"total":N,"page":1,"limit":20,"total_pages":...}`。
  - query 中带 **`chat_id`** 或 **`name`**（二者任一，值为会话 id）：返回该会话最近消息 JSON（最多约 50 条，见 [`sessions.rs`](../../src/platform/http_server/handlers/sessions.rs)）；失败 500，`{"error":"..."}`。

### GET /api/memory/status

- **用途**：获取 MEMORY、SOUL、USER 的字节数（只读）。
- **鉴权**：已激活；GET **不必**附带配对码。
- **响应**：200，JSON `{"memory_len": number, "soul_len": number, "user_len": number}`。

### GET /api/tools

- **用途**：返回固件侧为 HTTP 列举的「工具名 + 简短描述」JSON 数组，供配置页或脚本探测能力。
- **鉴权**：已激活；GET **不必**附带配对码。
- **响应**：200，JSON 数组 `[{"name":"get_time","description":"..."}, ...]`。实现见 [`handlers/tools.rs`](../../src/platform/http_server/handlers/tools.rs)：条目随 **`tools_network_extra`** / **`tools_diagnostics`** 等编译 feature 变化，且**不一定**与 Agent 运行时 [`build_default_registry`](../../src/tools/registry.rs) 完全一致（例如始终注册的 **env**、**file_write** 等可能未出现在该列表中）。**权威工具集合**以 Agent 注册表与 [Agent 工具说明](tools.md) 为准。

## Skills

### GET /api/skills

- **用途**：列表或单条 skill。无 query 时返回列表与顺序；带 `?name=xxx` 时返回该 skill 的纯文本内容（供编辑回显）。
- **鉴权**：已激活；GET **不必**附带配对码。
- **响应（无 name）**：200，JSON `{"skills": [{"name": "x", "enabled": true}, ...], "order": ["a", "b"]}`。
- **响应（name=xxx）**：200，`Content-Type: text/plain`，Body 为 skill 内容；不存在 404。

### POST /api/skills

- **用途**：更新启用状态、写入内容或更新顺序。Body 形状决定行为。
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

- **用途**：从 URL 拉取内容并写入为新 skill。
- **鉴权**：已激活 + 配对码 + CSRF。
- **请求**：`Content-Type: application/json`，Body `{"url": "https://...", "name": "xxx"}`。url 须为 http(s)；name 合法（无 `..`、`/`、`\`）。
- **响应**：成功 200，`{"ok": true}`；url 拉取失败 502/500；body 非 UTF-8 或超长 400。

### POST /api/webhook

- **用途**：外部 HTTP POST 触发一条入站消息，body 作为 content 推入入站队列，由 agent 处理。
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

- **用途**：与 CLI `health` 一致的结构化健康信息；含 `metrics` 与 `resource`（与 `GET /api/resource` 同源 orchestrator 快照）。
- **鉴权**：已激活；GET **不必**附带配对码。
- **响应**：200，JSON 示例（字段与 serde 结构体一致；`metrics` 计数器字段名为 `messages_in` 等）：
  ```json
  {
    "wifi": "connected",
    "inbound_depth": 0,
    "outbound_depth": 0,
    "last_error": "none",
    "metrics": { "messages_in": 0, "messages_out": 0 },
    "resource": { "pressure": "Normal", "heap_largest_block_internal": 12345 }
  }
  ```
  - `wifi`：`"connected"` | `"disconnected"`（**STA 已连上游路由器/有出站路径**；仅连设备 SoftAP 配网热点时仍可能为 `disconnected`）。
  - `inbound_depth` / `outbound_depth`：入站/出站队列深度（数字）。
  - `last_error`：最近一次错误摘要（仅 stage/message，无密钥）；无则为 `"none"`。
  - `metrics`：`metrics::MetricsSnapshot`（消息吞吐、LLM/tool、dispatch、按 stage 错误计数等；Linux 嵌入式另含 `wifi_reconnect_total`、`wifi_ap_restart_total`、`wifi_last_failure_stage`）。
  - `resource`：`orchestrator::ResourceSnapshot`（压力、堆、队列深度、通道健康等）；`pressure` 为字符串 `"Normal"` | `"Cautious"` | `"Critical"`。
  - **迁移**：若外部脚本曾依赖旧版手写 JSON 中的扁平 `msg_in` 等键名，请改为嵌套 `metrics.messages_in`。

### GET /api/diagnose

- **用途**：设备自检（Doctor 式），返回结构化结果列表，供外置配置页「设备状态」展示。
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
  - `category`：`"storage"`（存储可读、SPIFFS）| `"channel"`（队列深度、last_error）| `"config"`（NVS、WiFi）。
  - `message`：人类可读说明；`last_error` 摘要截断至 200 字符。

### GET /api/metrics

- **用途**：导出 metrics 快照 JSON（字段以固件 `metrics` 模块为准）。可选 query **`format=prometheus`** 返回 Prometheus 文本格式（`Content-Type` 为 `text/plain`）。
- **鉴权**：已激活；GET **不必**附带配对码。

### GET /api/resource

- **用途**：orchestrator 资源快照 JSON（与 `GET /api/health` 内 `resource` 同源）。
- **鉴权**：已激活；GET **不必**附带配对码。

### GET /api/system_info

- **用途**：系统/构建信息 JSON。
- **鉴权**：已激活；GET **不必**附带配对码。

### GET /api/channel_connectivity

- **用途**：通道连通性探测结果 JSON。
- **鉴权**：已激活；GET **不必**附带配对码。

### GET /api/config/display

- **用途**：读取显示相关配置段 JSON（若固件启用显示子系统）。
- **鉴权**：已激活；GET **不必**附带配对码。

### POST /api/config/display

- **用途**：写入显示配置段；部分变更可能支持 `?restart=1` 与 WiFi 段类似。
- **鉴权**：已激活 + 配对码 + CSRF。

### DELETE /api/sessions?chat_id=...

- **用途**：删除指定会话。
- **鉴权**：已激活 + 配对码 + CSRF；query **必须**含 `chat_id`。

### 通道回调（无配对码 / 无 CSRF）

供各 IM 平台服务器调用；鉴权为各 handler 内签名或 token 逻辑，**不适用**设备侧的配对码 + CSRF 流程：

- **POST /api/feishu/event**
- **POST /api/dingtalk/webhook**
- **GET /api/wecom/webhook**（URL 验证）、**POST /api/wecom/webhook**
- **POST /api/webhook/qq**（未启用 QQ 通道时可能 404）

### POST /api/restart

- **用途**：触发设备重启，使新配置生效。
- **鉴权**：已激活 + 配对码 + CSRF。
- **响应**：先返回 200，`{"ok": true}`，随后在约 100～500ms 内设备重启。
- **节流**：60 秒内仅允许一次有效重启。

### GET /api/ota/check

- **用途**：按当前板型与渠道查询是否有可用的固件更新（依赖编译期 `OTA_MANIFEST_URL` 与 manifest 清单）。仅当 feature `ota` 启用时存在。
- **鉴权**：已激活；GET **不必**附带配对码。
- **请求**：GET，可选 query `channel`（默认 `stable`）。
- **响应**：200 JSON。字段：`current_version`（当前固件版本）、`latest_version`（渠道最新版本，有渠道时）、`update_available`（是否有可升级版本）、`url`（有更新时的固件下载 URL）、`release_notes`（可选）、`error`（可选，人话提示，如渠道未配置或拉取失败）。manifest 未配置或拉取/解析失败时仍返回 200，`update_available: false`，可选带 `error` 人话。

### POST /api/ota

- **用途**：从指定 URL 拉取固件并执行 OTA 更新，成功后设备重启。仅当固件以 feature `ota` 编译时该接口存在（GET / 的 endpoints 中会包含 `"POST /api/ota"`）。
- **鉴权**：已激活 + 配对码 + CSRF。
- **请求**：`Content-Type: application/json`，Body 为 `{"url": "https://..."}`；url 须非空且为 `http://` 或 `https://` 开头。
- **响应**：成功 200，`{"ok": true}`，随后设备执行 OTA 并在完成后重启；无效或缺失 url 返回 400，`{"error": "invalid url"}`；OTA 下载、校验或写入失败返回 500，`{"error": "可读人话"}`（如「网络或下载失败，请检查网络后重试」「固件校验失败，请更换固件来源」「写入失败，请勿断电并重试」）。响应带 CORS 头。
- **说明**：更新失败时不会写入当前运行分区，设备可继续使用；可重试或更换 URL。

**OTA 渠道 manifest 格式**（由 CI/Release 产出并上传至 `OTA_MANIFEST_URL`）：JSON 根含 `boards`，键为板型 ID（`esp32-s3-8mb`、`esp32-s3-16mb`、`esp32-s3-32mb`），值为渠道对象；各渠道（如 `stable`）含 `version`、`url`（必填）、可选 `release_notes`。例：`{"boards":{"esp32-s3-16mb":{"stable":{"version":"0.2.0","url":"https://...","release_notes":"..."}}}}`。设备构建时通过 `BOARD`、`OTA_MANIFEST_URL` 指定板型与清单地址。

### POST /api/config_reset

- **用途**：恢复出厂（清空 NVS 配置区并删除 SPIFFS 上 `config/llm.json`、`config/channels.json`、`config/hardware.json`、`config/audio.json`、`config/display.json`、`config/skills_meta.json` 等），与 CLI `config_reset yes` 等价。
- **鉴权**：已激活 + 配对码 + CSRF。
- **响应**：成功 200，`{"ok": true}`；失败 500，`{"error": "reset failed"}`。
- **说明**：调用后建议用户重启设备，重启后 `AppConfig::load()` 仅来自环境变量；NVS 仅保留 6 个小键（wifi、proxy、session、tg_group、locale 等），其余配置存 SPIFFS。

## 如何获知板子 IP

- 连接设备热点 **Beetle** 时：使用 **http://192.168.4.1**（固件 SoftAP 固定地址）。
- 已连 STA 且与设备在同一 LAN 时：使用路由器分配给设备的 IP。

## 配置页归属

设备固件内嵌 **`GET /wifi`**（配置页 HTML）、**`GET /pairing`**、**`GET /common.css`**、**`GET /common.js`** 等（见上文路由表）。功能更全的界面也可由本仓库 **`configure-ui`** 或独立部署的静态站点提供，通过填写设备地址调用同一套 HTTP API。
