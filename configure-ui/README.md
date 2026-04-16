# Beetle Configure UI

[中文](README.zh-CN.md)

## What is this?

**Beetle Configure UI** is the browser-based setup page for Beetle devices.
Open it from the device address or the online page, connect to your Beetle, then configure WiFi, LLM, channels, and other options.

## What you can do here

After you connect to a Beetle device, this page lets you:

- Set or change the **pairing code** (required for saving config).
- Configure **WiFi** (scan and connect).
- Configure **channels**: Telegram, Feishu, DingTalk, WeCom, QQ Channel, Webhook (tokens, keys, toggles).
- Configure **LLM**: API key, model, provider, compatible API URL (e.g. Ollama).
- Set **proxy**, **search keys**, and related options.
- View **system info**, **restart**, **OTA** (if enabled), **factory reset**.

All write operations require the correct pairing code; the UI sends it for you.

## Who should read this page

- **End users**: read the next section for the fastest way to open the config page and set up a device.
- **Developers**: skip to [For developers: local work and release](#for-developers-local-work-and-release).

---

## For end users: how to use

### Prerequisites (required before using the config page)

You must have both of the following:

1. **A device with Beetle firmware flashed.**  
   The config page only talks to a device running **Beetle OS** firmware. If you have not flashed the firmware yet, build and flash it first (see the **parent repo’s README or docs** for build and flash instructions). This UI does not replace the need for a flashed device.

2. **The device powered on and reachable.**
   - **First use / not yet on your WiFi:** The device will open a **WiFi hotspot** with SSID **Beetle** (no password). Your phone or PC must **connect to this hotspot**; then open **http://192.168.4.1** (usually this address, in some cases may be a different address like 172.16.42.1).
   - **After WiFi is configured:** The device joins your router. Your phone or PC must be on the **same LAN** as the device; use the router-assigned device IP.

**Important:** Whether you open the config page from the device URL or an online URL, your browser must be on the same network as the device (Beetle hotspot or same LAN). Otherwise the page cannot talk to the device.

---

### Option A – Open the config page from the device (direct)

You use the device’s own address in the browser; the device serves the UI (or redirects to it).

**When the device is not yet on your WiFi (first use):**

1. Power on the device → it opens hotspot **Beetle** (no password).
2. On your phone or PC, **connect to the WiFi “Beetle”**.
3. In the browser open **http://192.168.4.1** (usually this address, in some cases may be a different address).

Only the device is on that hotspot.

**When the device is already on your WiFi:**

- From any device on the **same LAN**, use the router-assigned device IP.

**First time on the config page:** Set a **6-digit pairing code**. It protects all write operations; secrets stay on the device. If you forget it, use **Factory reset** from the config page (you must still be able to open the page).

---

### Option B – Open the config page from the online URL

You open the same setup page from the internet (for example **https://openbeetles.github.io/beetles/**). To actually configure a device, your browser still needs to reach that device on the same network.

**Step-by-step when using the online address:**

1. **Prepare the device**  
   - Flash Beetle firmware to your hardware (see parent repo docs if needed).  
   - Power on the device.

2. **Put your phone or PC on the same network as the device**  
   - **Not yet on WiFi:** Connect your phone/PC to the device’s hotspot **Beetle** (no password).  
   - **Already on WiFi:** Ensure your phone/PC and the device are on the same LAN (e.g. same home/office router).

3. **Open the online config page**  
   - In the browser go to: **https://openbeetles.github.io/beetles/**

4. **Enter the device address in the page**
   - In the config UI, find the **”Device URL”** (设备地址) field.
   - When connected to the device’s hotspot, enter **http://192.168.4.1** (usually this address, in some cases may be a different address); when on the same LAN, enter the router-assigned IP.
   - Save. The page will then talk to the device at that address.

5. **Set pairing code and configure**  
   - On first use, set a **6-digit pairing code** on the config page.  
   - After that, you can configure WiFi, channels, LLM, etc. All write operations use this code (the UI sends it for you).

**If the page says it cannot reach the device:** Check that (1) the device is powered on, (2) you are connected to the **Beetle** hotspot or the **same LAN** as the device, and (3) the device address you entered is correct—usually use **http://192.168.4.1** when on the hotspot (in some cases may be a different address like http://172.16.42.1 if the first doesn’t work), or the device’s LAN IP when on the same LAN.

**Using the online URL only to preview:** You can open the online page without a device to view the interface. To read or change config, you still need a device on the same network.

---

### First-time setup and pairing code

- **First access:** Set a **6-digit pairing code** on the config page. It protects save/restart/OTA/factory reset; secrets are stored on the device only.
- **Forgot the code:** Use **Factory reset** from the config page (you must still be able to open the page and run the action).

More detail: see the parent repo’s docs, especially `docs/en-us/configuration.md` and `docs/en-us/config-api.md`.

---

## For developers: local work and release

### Prerequisites

- Node.js 20+  
- npm
- Rust toolchain (`rustup`, `cargo`)
- Tauri desktop prerequisites for your OS: <https://v2.tauri.app/start/prerequisites/>

### Commands

| Command          | Description                |
|------------------|----------------------------|
| `npm ci`         | Install dependencies       |
| `npm run dev`    | Start dev server           |
| `npm run build`  | TypeScript + Vite build    |
| `npm run lint`   | Run ESLint                 |
| `npm run preview`| Preview production build   |
| `npm run tauri dev` | Run the same UI in a Tauri desktop shell |
| `npm run tauri build` | Build the desktop shell package |

### Local development

```bash
cd configure-ui
npm ci
npm run dev
```

During local development, open the page in a browser that can reach the target Beetle device.

### Desktop shell

`configure-ui` now also ships with a minimal Tauri desktop shell under `src-tauri/`.
The shell does **not** add or remove product features. It only wraps the existing React/Vite UI as a desktop app, so all current device URL, pairing code, config, and API flows stay unchanged.

```bash
cd configure-ui
npm ci
npm run tauri dev
```

For production packaging:

```bash
cd configure-ui
npm ci
npm run tauri build
```

### Design and style

- UI and style rules (tokens, layout, no hardcoded colors): **docs/DESIGN.md**.
- Follow the design constraints when adding or changing UI.
- Desktop shell baseline, versioning, and CI/release notes: **docs/DESKTOP_SHELL.md**.

### Deployment

A built version can be published to GitHub Pages.
If you maintain the web release, use the repository Pages settings and workflow for that deployment.
