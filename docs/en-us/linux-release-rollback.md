# Linux Deploy and Rollback

**English** | [中文](../zh-cn/linux-release-rollback.md) | [Doc index](../README.md)

This page keeps to the deployment and rollback flow that actually exists on Linux today.

## Common paths after deploy

The current deploy script maintains these paths:

- `/opt/beetle/releases/<release>/`
- `/opt/beetle/current`
- `/opt/beetle/rollback`
- `/usr/local/bin/beetle`
- `/usr/bin/beetle` (fallback on shells whose default `PATH` omits `/usr/local/bin`)
- `/var/lib/beetle`

In practice:

- `current` points to the active release
- `rollback` points to the previous rollback candidate
- `/usr/local/bin/beetle` is the global command entry
- some embedded shells also receive `/usr/bin/beetle` as a compatibility entry

## Runtime entrypoints

The Linux service entrypoint is:

- `beetle run`

The control plane (HTTP API) and the agent run inside the same process — no child process is spawned.

If you use `systemd`, `ExecStart` should point to:

- `/opt/beetle/current/beetle run`

## The three deploy modes

`./build.sh --deploy-linux` currently offers three modes:

1. `Quick deploy`
   Replace the binary only
2. `Full deploy`
   Refresh the binary and service install content
3. `Smart update`
   Replace the binary and restart the existing service when appropriate

If a release changes the service entrypoint or service files, do not rely on `Smart update` alone.

## How rollback works

A new release first enters:

- `pending_validation`

If it stays healthy, it becomes the stable release.
If it fails quickly and repeatedly during the validation window, Beetle prefers to roll back to `rollback`.

## Where to look during manual checks

- active release: `/opt/beetle/current`
- rollback candidate: `/opt/beetle/rollback`
- release state: `/var/lib/beetle/runtime/linux_release/state.json`
- global command: `/usr/local/bin/beetle`
- compatibility command: `/usr/bin/beetle` (when present)

## Direct commands

- show release status: `beetle release status`
- request rollback: `beetle release rollback`
- restart the managed service: `beetle restart`
- stop the active Beetle runtime: `beetle stop` (prefer the managed service; when unmanaged, request the live `beetle run` process to exit gracefully)

## Direct takeaways

- The Linux path now runs as a long-running service
- The real startup entrypoint is `beetle run` (single process)
- Rollback is not just swapping one file; it switches back to the previous release layout
- For hardware setup, do not follow old Linux-only hardware examples; go straight to [hardware-device-config.md](hardware-device-config.md)
