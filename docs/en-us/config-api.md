# HTTP Config API

[中文](../zh-cn/config-api.md) | **English** | [Doc index](../README.md)

This document defines the public Beetle HTTP configuration contract. It is intended for developers building:

- external configuration UIs
- scripts and automation
- third-party integrations against Beetle devices

It covers:

- base access and auth rules
- request and response contracts
- important status codes and side effects

If you only need the built-in provisioning flow, read [configuration.md](configuration.md) first.

## Basics

- **ESP SoftAP address**: on first boot, ESP firmware starts a hotspot named **Beetle**. Use `http://192.168.4.1` after connecting.
- **Linux device address**: if the system already has a valid WiFi connection, Beetle serves HTTP on the device's current LAN IP.
- **CORS**: `/api/*` and `GET /` include `Access-Control-Allow-Origin: *`. `OPTIONS` returns `200 OK` with standard CORS headers.

## Auth model

### Terms

- **Not activated**: no valid 6-digit pairing code has been stored yet.
- **Activated**: `POST /api/pairing_code` has succeeded at least once.
- **Pairing code**: sent via query `?code=` or header `X-Pairing-Code`.
- **CSRF token**: sent via `X-CSRF-Token`, with the value returned by `GET /api/csrf_token`.

### Routes callable before activation

- any `OPTIONS`
- `GET /`
- `GET /wifi`
- `GET /pairing`
- `GET /common.css`
- `GET /common.js`
- `GET /api/pairing_code`
- `POST /api/pairing_code`
- `GET /api/wifi/scan`
- `GET /api/csrf_token`
- channel callbacks:
  - `POST /api/feishu/event`
  - `POST /api/dingtalk/webhook`
  - `GET /api/wecom/webhook`
  - `POST /api/wecom/webhook`
  - `POST /api/webhook/qq`

All other routes normally return `401 Unauthorized` before activation.

### General write rule

After activation, state-changing `POST` and `DELETE` routes require:

- pairing code
- CSRF token

Exceptions:

- `POST /api/pairing_code` is only available before activation and requires neither
- channel callbacks use vendor-specific verification instead of Beetle pairing and CSRF

## Discovery and pairing

### GET /

- **Auth**: callable before activation; after activation, no pairing code is required in the request
- **Response**:
  - not activated: `302 Found`, `Location: /pairing`
  - activated: `200 OK`, JSON
- **Body**:
  - `name`
  - `version`
  - `endpoints`

Example:

```json
{
  "name": "beetle",
  "version": "0.1.0",
  "endpoints": ["GET /pairing", "GET /wifi", "GET /api/pairing_code"]
}
```

### GET /api/pairing_code

- **Auth**: none
- **Response**: `200 OK`
- **Body**:
  - `code_set`
  - `locale`

### POST /api/pairing_code

- **Auth**: none; available only before activation
- **Headers**: `Content-Type: application/json`
- **Body**:

```json
{ "code": "123456" }
```

- **Response**:
  - success: `200 OK`, `{"ok": true}`
  - invalid request or already set: `400 Bad Request`

### GET /api/csrf_token

- **Auth**: none
- **Response**: `200 OK`

```json
{ "csrf_token": "<token>" }
```

### GET /pairing

- **Auth**: none
- **Response**: `200 OK`
- **Content-Type**: `text/html; charset=utf-8`

### GET /wifi

- **Auth**: none
- **Response**: `200 OK`
- **Content-Type**: `text/html; charset=utf-8`

## Config overview

### GET /api/config

- **Auth**: activated + pairing code
- **Response**: `200 OK`
- **Body**: full `AppConfig` JSON with current stored values

### GET /api/wifi/scan

- **Auth**: none
- **Response**:
  - success: `200 OK`
  - unavailable: `503 Service Unavailable`
- **Body**: WiFi scan results sorted by signal strength

```json
[
  { "ssid": "MyWiFi", "rssi": -50 }
]
```

### POST /api/config/wifi

- **Auth**: activated + pairing code + CSRF
- **Headers**: `Content-Type: application/json`
- **Body**:

```json
{
  "wifi_ssid": "MyWiFi",
  "wifi_pass": "secret"
}
```

- **Response**:
  - success: `200 OK`
  - validation failure: `400 Bad Request`
- **Success body**:

```json
{ "ok": true, "restart_required": true }
```

### POST /api/config/system

- **Auth**: activated + pairing code + CSRF
- **Headers**: `Content-Type: application/json`
- **Body**: system config segment JSON
- **Primary fields**:
  - `wifi_ssid`
  - `wifi_pass`
  - `proxy_url`
  - `session_max_messages`
  - `tg_group_activation`
  - `locale`
- **Response**:
  - success: `200 OK`, `{"ok": true}`
  - validation failure: `400 Bad Request`

### POST /api/config/llm

- **Auth**: activated + pairing code + CSRF
- **Headers**: `Content-Type: application/json`
- **Body**: full LLM config segment
- **Primary fields**:
  - `llm_sources[]`
  - `llm_stream`
  - `llm_router_source_index`
  - `llm_worker_source_index`
- **Response**:
  - success: `200 OK`, `{"ok": true}`
  - validation failure: `400 Bad Request`

### POST /api/config/channels

- **Auth**: activated + pairing code + CSRF
- **Headers**: `Content-Type: application/json`
- **Body**: full channels config segment
- **Response**:
  - success: `200 OK`, `{"ok": true}`
  - validation failure: `400 Bad Request`

## Account configuration

### GET /api/config/providers

- **Auth**: activated + pairing code
- **Query parameters**:
  - `capability`: optional, `mail|calendar|documents|contacts_directory`
- **Response**: `200 OK`
- **Body**:
  - `count`
  - `items[]`
    - `provider_kind`
    - `display_name`
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

- **Auth**: activated + pairing code
- **Response**: `200 OK`
- **Body**:
  - `count`
  - `items[]`
    - `capability`
    - `default_account_key`
    - `selection_status`
    - `selected_account_key`
    - `ready`
    - `next_action`
    - `accounts[]`

### GET /api/config/capabilities/:capability

- **Auth**: activated + pairing code
- **Path parameter**:
  - `capability`: `mail|calendar|documents|contacts_directory`
- **Response**: `200 OK`
- **Body**: one capability status object with the same fields as one `items[]` entry from `GET /api/config/capabilities`

### GET /api/config/accounts

- **Auth**: activated + pairing code
- **Query parameters**:
  - `capability`: optional, `mail|calendar|documents|contacts_directory`
- **Response**: `200 OK`
- **Body**:
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

- **Auth**: activated + pairing code + CSRF
- **Headers**: `Content-Type: application/json`
- **Body**: one account upsert payload
- **Primary fields**:
  - `account`
    - `account_key`
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
- **Response**:
  - success: `200 OK`, returns refreshed account detail
  - validation failure: `400 Bad Request`

### GET /api/config/accounts/:account_key

- **Auth**: activated + pairing code
- **Path parameter**:
  - `account_key`
- **Response**: `200 OK`
- **Body**:
  - `account`
  - `assessment`
  - `provider_display_name`
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

- **Auth**: activated + pairing code + CSRF
- **Headers**: `Content-Type: application/json`
- **Body**:

```json
{
  "fields": {
    "field_key": "value"
  },
  "clear_fields": ["another_field"]
}
```

- **Response**:
  - success: `200 OK`, returns refreshed account detail
  - validation failure: `400 Bad Request`

### POST /api/config/accounts/:account_key/probe

- **Auth**: activated + pairing code + CSRF
- **Path parameter**:
  - `account_key`
- **Response**:
  - success: `200 OK`
  - failure: `400 Bad Request`
- **Body**:
  - `account_key`
  - `provider_kind`
  - `configured`
  - `disposition`
  - `reason`

### POST /api/config/accounts/:account_key/revoke

- **Auth**: activated + pairing code + CSRF
- **Headers**: `Content-Type: application/json`
- **Body**: optional; if present:

```json
{
  "clear_runtime_status": true
}
```

- **Response**:
  - success: `200 OK`
  - failure: `400 Bad Request`

### DELETE /api/config/accounts/:account_key

- **Auth**: activated + pairing code + CSRF
- **Path parameter**:
  - `account_key`
- **Response**:
  - success: `200 OK`
  - failure: `400 Bad Request`
- **Success body**:

```json
{
  "ok": true,
  "account_key": "mail-work",
  "deleted": true
}
```

## Raw config segment endpoints

### GET /api/config/office_credentials

- **Auth**: activated + pairing code
- **Response**: `200 OK`
- **Body**: `OfficeCredentialsSegment`

### POST /api/config/office_credentials

- **Auth**: activated + pairing code + CSRF
- **Headers**: `Content-Type: application/json`
- **Body**: full `OfficeCredentialsSegment`
- **Response**:
  - success: `200 OK`, `{"ok": true}`
  - validation failure: `400 Bad Request`

### GET /api/config/hardware

- **Auth**: activated
- **Response**: `200 OK`
- **Body**: `HardwareSegment`

### POST /api/config/hardware

- **Auth**: activated + pairing code + CSRF
- **Headers**: `Content-Type: application/json`
- **Body**: full `HardwareSegment`
- **Response**:
  - success: `200 OK`, `{"ok": true}`
  - validation failure: `400 Bad Request`

### GET /api/config/audio

- **Auth**: activated
- **Response**: `200 OK`
- **Body**: `AudioSegment`

### POST /api/config/audio

- **Auth**: activated + pairing code + CSRF
- **Headers**: `Content-Type: application/json`
- **Body**: full `AudioSegment`
- **Response**:
  - success: `200 OK`
  - validation failure: `400 Bad Request`
- **Success body**:

```json
{ "ok": true, "restart_required": true }
```

### GET /api/config/display

- **Auth**: activated
- **Response**: `200 OK`

### POST /api/config/display

- **Auth**: activated + pairing code + CSRF
- **Headers**: `Content-Type: application/json`
- **Response**:
  - success: `200 OK`
  - validation failure: `400 Bad Request`

## Text content configuration

### GET /api/soul

- **Auth**: activated
- **Response**: `200 OK`
- **Content-Type**: `text/plain`

### POST /api/soul

- **Auth**: activated + pairing code + CSRF
- **Body**: plain text, or JSON `{"content":"..."}`, max length 32KB
- **Response**:
  - success: `200 OK`
  - invalid request: `400 Bad Request`
  - write failure: `500 Internal Server Error`

### GET /api/user

- **Auth**: activated
- **Response**: `200 OK`
- **Content-Type**: `text/plain`

### POST /api/user

- **Auth**: activated + pairing code + CSRF
- **Body**: plain text, or JSON `{"content":"..."}`, max length 32KB
- **Response**:
  - success: `200 OK`
  - invalid request: `400 Bad Request`
  - write failure: `500 Internal Server Error`

## Sessions, memory, and tools

### GET /api/sessions

- **Auth**: activated
- **Query parameters**:
  - list mode: `page`, `limit`
  - detail mode: `chat_id` or `name`
- **Response**: `200 OK`

### DELETE /api/sessions?chat_id=...

- **Auth**: activated + pairing code + CSRF
- **Query parameter**:
  - `chat_id`: required
- **Response**:
  - success: `200 OK`
  - invalid request: `400 Bad Request`

### GET /api/memory/status

- **Auth**: activated
- **Response**: `200 OK`
- **Body**: memory operator status object

### GET /api/tools

- **Auth**: activated
- **Response**: `200 OK`
- **Body**: tool list array

## Skills

### GET /api/skills

- **Auth**: activated
- **Query parameter**:
  - `name`: optional
- **Response**:
  - without `name`: `200 OK`, returns skill list and order
  - with `name`: `200 OK`, returns skill text; `404 Not Found` if missing

### POST /api/skills

- **Auth**: activated + pairing code + CSRF
- **Headers**: `Content-Type: application/json`
- **Body**:
  - enable or disable: `{"name":"x","enabled":true}`
  - write content: `{"name":"x","content":"..."}`
  - update order: `{"order":["a","b"]}`
- **Response**:
  - success: `200 OK`
  - invalid request: `400 Bad Request`
  - processing failure: `500 Internal Server Error`

### DELETE /api/skills?name=xxx

- **Auth**: activated + pairing code + CSRF
- **Query parameter**:
  - `name`: required
- **Response**:
  - success: `200 OK`
  - invalid request: `400 Bad Request`
  - missing file: `404 Not Found`

### POST /api/skills/import

- **Auth**: activated + pairing code + CSRF
- **Headers**: `Content-Type: application/json`
- **Body**:

```json
{
  "url": "https://example.com/skill.md",
  "name": "skill-name"
}
```

- **Response**:
  - success: `200 OK`
  - invalid request: `400 Bad Request`
  - upstream fetch failure: `502 Bad Gateway` or `500 Internal Server Error`

## Health and operations

### GET /api/health

- **Auth**: activated
- **Response**: `200 OK`
- **Body**: lightweight health object, including fields such as `wifi`, `last_error`, `display`, and `audio`

### GET /api/diagnose

- **Auth**: activated
- **Response**: `200 OK`
- **Body**: diagnostic result array; each item includes:
  - `severity`
  - `category`
  - `message`

### GET /api/operator/status

- **Auth**: activated
- **Response**: `200 OK`
- **Body**: operator runtime status object

### GET /api/metrics

- **Auth**: activated
- **Query parameter**:
  - `format=prometheus`: optional
- **Response**:
  - default: `200 OK`, JSON
  - with `format=prometheus`: `200 OK`, Prometheus text

### GET /api/resource

- **Auth**: activated
- **Response**: `200 OK`
- **Body**: resource, queue, pressure, and budget status object

### GET /api/system_info

- **Auth**: activated
- **Response**: `200 OK`
- **Body**: device summary and build information

### GET /api/channel_connectivity

- **Auth**: activated
- **Response**: `200 OK`
- **Body**: channel connectivity status object

### POST /api/restart

- **Auth**: activated + pairing code + CSRF
- **Response**:
  - success: `200 OK`, then the device restarts
  - throttled or failed: `400` / `500`

### GET /api/ota/check

- **Auth**: activated
- **Precondition**: firmware built with `ota`
- **Query parameter**:
  - `channel`: optional, default `stable`
- **Response**: `200 OK`
- **Body**:
  - `current_version`
  - `latest_version`
  - `update_available`
  - `url`
  - `release_notes`
  - `error`

### POST /api/ota

- **Auth**: activated + pairing code + CSRF
- **Precondition**: firmware built with `ota`
- **Headers**: `Content-Type: application/json`
- **Body**:

```json
{ "url": "https://example.com/firmware.bin" }
```

- **Response**:
  - success: `200 OK`, then OTA starts and the device restarts
  - invalid request: `400 Bad Request`
  - download, verification, or write failure: `500 Internal Server Error`

### POST /api/config_reset

- **Auth**: activated + pairing code + CSRF
- **Response**:
  - success: `200 OK`, `{"ok": true}`
  - failure: `500 Internal Server Error`

## Webhook and platform callbacks

### POST /api/webhook

- **Auth**: activated + pairing code + CSRF; webhook token also required
- **Body**: UTF-8 text, max 4KB
- **Response**:
  - success: `200 OK`
  - token mismatch: `401 Unauthorized`
  - webhook disabled: `403 Forbidden`
  - invalid request: `400` / `413`
  - queue full: `503 Service Unavailable`

### Platform callback routes

These routes do not use Beetle pairing code and CSRF:

- `POST /api/feishu/event`
- `POST /api/dingtalk/webhook`
- `GET /api/wecom/webhook`
- `POST /api/wecom/webhook`
- `POST /api/webhook/qq`

## Device IP discovery

- When connected to the **Beetle** hotspot: use `http://192.168.4.1`
- When the device is already on the LAN: use the IP assigned by the router

## Built-in config pages

Firmware embeds:

- `GET /wifi`
- `GET /pairing`
- `GET /common.css`
- `GET /common.js`

You can also use the repo’s `configure-ui`, or any custom frontend calling the same HTTP API.
