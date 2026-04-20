Beetle Linux bundle (musl)
==========================

Audience
--------
This tarball is for integrators and manual trials. **End-user one-click / SSH install is not the story yet**—that will follow in a separate product flow. Optional context: docs/en-us/linux-release-rollback.md (or docs/zh-cn/linux-release-rollback.md).

Binary
------
- `beetle`: statically linked (musl). `./build.sh --deploy-linux` installs to `/opt/beetle/releases/<release>/`, updates `/opt/beetle/current`, maintains `/opt/beetle/rollback` for the previous release, and creates `/usr/local/bin/beetle -> /opt/beetle/current/beetle` as the global command entry. Start the Linux runtime with `beetle supervise`; `beetle agent` is the execution-plane entry used by the supervisor.
- The deploy flow writes `/var/lib/beetle/runtime/linux_release/state.json` with rollout state `pending_validation`. The supervisor marks the release `steady` after it survives the validation window, or flips back to `rollback` on repeated quick failures.
- The deploy flow also syncs shipped official runtime skills from `spiffs_data/skills/*.md` into the remote state root `skills/` directory. Existing user-created runtime skills on the device are left in place unless a shipped file has the same name.
- **WiFi addressing**: Beetle sets AP/STA addresses via **rtnetlink** in-process; the **`ip` utility is not required** for those steps (you still need `wpa_supplicant` / `hostapd` / `dnsmasq` / `iw` where the code invokes them).

Config API (optional)
---------------------
- Set `BEETLE_CONFIG_HTTP_LISTEN` to e.g. `127.0.0.1:8080` to enable the config HTTP server on Linux.
- Default state root: `/var/lib/beetle` or `/data/beetle`, or override with `BEETLE_STATE_ROOT`.

Hardware JSON
-------------
- Copy `hardware.json.example` to your config path (e.g. under the state root `config/hardware.json`) and adjust `backlight_path` / `backlight_max` for your board.
- Some boards use `backlight0` and `backlight_max` 100; adjust the example values to match your hardware.

systemd
-------
- Edit `beetle.service`: set `User=`/`Group=`; optional hardening: `ProtectSystem=` + `ReadWritePaths=` only if every listed path exists (missing paths cause systemd 226/NAMESPACE on start).
- Install: copy unit to `/etc/systemd/system/`, optionally create `/etc/default/beetle`, then run `systemctl daemon-reload` and `systemctl enable --now beetle`. The unit starts Beetle with `ExecStart=/opt/beetle/current/beetle supervise`.
- The bundle also ships `beetle.init` as a SysV example. It now carries Debian/LSB headers too, so `systemctl enable beetle` no longer trips over `update-rc.d` when that init script is installed alongside the unit.
- **Startup order**: the unit uses `After=local-fs.target` and `Wants=network-pre.target` only — **not** `network-online.target`. Beetle manages `wpa_supplicant` / `hostapd` itself; waiting for “full internet” can deadlock with `NetworkManager-wait-online` on devices where the wlan is not yet up at that point.
- **NetworkManager conflict**: if NetworkManager (or another manager) **owns the same wlan interface**, pick one — either disable NM for that iface or do not run Beetle’s Linux WiFi stack on it. Two controllers on one radio will race.

Environment
-------------
- Optional `/etc/default/beetle` can set e.g. `BEETLE_STATE_ROOT=/var/lib/beetle` and `BEETLE_CONFIG_HTTP_LISTEN=127.0.0.1:8080`.

Permissions
-----------
- Prefer a dedicated user; config/secrets `0600` or `0640`; state directory `0700` per project baseline.
