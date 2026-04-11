# Configuration Guide

[中文](../zh-cn/configuration.md) | **English** | [Doc index](../README.md)

This page is for first-time Beetle setup.

In most cases, the setup order is:

1. connect to the device hotspot
2. set the pairing code
3. configure WiFi
4. configure an LLM and a chat channel

If you are building your own frontend or script, read [config-api.md](config-api.md) after this page.

## First-Time Setup

Linux SBC builds differ slightly from ESP:

- If the Linux system already has a valid WiFi connection, Beetle inherits that connection and you should use the device's current LAN IP.
- Beetle enters its own hotspot/provisioning fallback only when Linux does not currently have a valid WiFi connection.
- ESP firmware still follows the default hotspot-first flow.

### Step 1: connect to the hotspot

On first boot, the device opens a hotspot named **Beetle**.

1. Connect your phone or computer to that hotspot.
2. Open **http://192.168.4.1** in a browser.
3. You should see the pairing/config flow.

### Step 2: set the pairing code

The pairing code protects write operations such as:

- saving config
- restarting the device
- running factory reset
- starting OTA updates

Notes:

- Set it the first time you open the config page.
- Secrets written through the config UI go to NVS.
- Secrets are not supposed to be logged or written to SPIFFS.

### Step 3: configure WiFi

After you save WiFi settings, the device will try to join your router.

Once it is on the same LAN as your browser, you can open the config page again through the device's LAN IP instead of the hotspot address.

### Step 4: configure Beetle

The usual minimum setup is:

1. WiFi
2. one LLM source
3. one chat channel

## How To Open The Config UI

You have two common options:

### Option A: open the device directly

- While connected to the hotspot: use **http://192.168.4.1**
- While on the same LAN: use the device's router-assigned IP
- On Linux SBC builds that inherited system WiFi at boot, the LAN IP is the primary address; `192.168.4.1` is not guaranteed to exist

### Option B: use the external web UI

The repo includes `configure-ui`, which can talk to the device over the HTTP API.

You still need:

- a flashed device
- the browser and device on the same network
- the correct device address

## Common Config Areas

| Area | What it controls |
|------|------------------|
| WiFi | Router SSID and password |
| LLM | Provider, model, API key, API URL, fallback order |
| Channels | Credentials and channel-specific settings |
| Proxy / search | Proxy URL and search-related keys |
| Hardware | `hardware.json`-driven devices for `device_control` |
| Display | SPI TFT dashboard |
| System | Restart, reset, diagnostics, OTA if enabled |

## Common Config Keys

These names show up in files and API payloads:

| Category | Keys | Meaning |
|----------|------|---------|
| WiFi | `WIFI_SSID`, `WIFI_PASS` | Router credentials |
| Telegram | `TG_TOKEN`, `TG_ALLOWED_CHAT_IDS` | Telegram bot credentials and allowed chats |
| Feishu | `FEISHU_APP_ID`, `FEISHU_APP_SECRET`, `FEISHU_ALLOWED_CHAT_IDS` | Feishu app credentials |
| DingTalk | `DINGTALK_WEBHOOK_URL` | DingTalk webhook |
| WeCom | `WECOM_CORP_ID`, `WECOM_CORP_SECRET`, `WECOM_AGENT_ID`, `WECOM_DEFAULT_TOUSER` | WeCom app settings |
| QQ Channel | `QQ_CHANNEL_APP_ID`, `QQ_CHANNEL_SECRET` | QQ Channel credentials |
| Proxy | `PROXY_URL` | Outbound HTTP proxy |
| Search | `SEARCH_KEY`, `TAVILY_KEY` | Search service keys |

LLM settings are mainly stored in `config/llm.json`. Read [llm-providers.md](llm-providers.md) for supported provider IDs and fallback behavior.

## Pairing Code and Activation

After the device has been paired once:

- read-only APIs usually require the device to be activated
- write APIs require pairing code plus CSRF

The built-in config UI handles that for you. If you are calling APIs manually, read [config-api.md](config-api.md).

## Useful Checks

- `GET /api/health`: quick status snapshot
- `GET /api/resource`: resource snapshot
- serial logs: heartbeat and boot diagnostics

See [config-api.md](config-api.md) for exact response formats.

## Common Problems

- Cannot open the device: on Linux SBC builds, first confirm whether Beetle inherited an existing system WiFi connection; if not, reconnect to hotspot **Beetle** and retry `http://192.168.4.1`
- Cannot save config: pairing code or CSRF is missing/expired
- Device is online but channels do not work: check credentials and allowed chat IDs
- Device booted but hardware control is missing: check `hardware.json` and whether `device_control` was registered
