# Linux Operations

**English** | [中文](../zh-cn/linux-release-rollback.md) | [Doc index](../README.md)

On Linux, release management follows the deploy, restart, stop, and rollback flow below.

## The Short Path

For a normal Linux release cycle:

1. build the Linux artifact with [build-script.md](build-script.md), or produce a distributable tarball with `TARGET=linux ./build.sh --package-linux`
2. deploy it with `./build.sh --deploy-linux`
3. check release state with `beetle release status`
4. use `beetle restart`, `beetle stop`, or `beetle release rollback` only when needed

## Common Paths After Deploy

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
- `/usr/local/bin/beetle` is the main global command entry
- some embedded shells also receive `/usr/bin/beetle` as a compatibility entry

## Runtime Entry Point

The Linux service entrypoint is:

- `beetle run`

The control plane (HTTP API) and the agent run inside the same process.
No child process is spawned for the main runtime.

If you use `systemd`, `ExecStart` should point to:

- `/opt/beetle/current/beetle run`

## The Three Deploy Modes

`./build.sh --deploy-linux` currently offers three modes:

1. `Quick deploy`
   Replace the binary only
2. `Full deploy`
   Refresh the binary and service install content
3. `Smart update`
   Replace the binary and restart the existing service when appropriate

If a release changes the service entrypoint or service files, do not rely on `Smart update` alone.

## How Rollback Works

A new release first enters:

- `pending_validation`

If it stays healthy, it becomes the stable release.
If it fails quickly and repeatedly during the validation window, the runtime prefers to roll back to `rollback`.

## Where To Look During Manual Checks

- active release: `/opt/beetle/current`
- rollback candidate: `/opt/beetle/rollback`
- release state: `/var/lib/beetle/runtime/linux_release/state.json`
- global command: `/usr/local/bin/beetle`
- compatibility command: `/usr/bin/beetle` (when present)

## Direct Commands

- show release status: `beetle release status`
- request rollback: `beetle release rollback`
- restart the managed service: `beetle restart`
- stop the active runtime: `beetle stop` (prefer the managed service; when unmanaged, request the live `beetle run` process to exit gracefully)

## Read Next

- To get Beetls OS running on Linux the first time: [getting-started-linux.md](getting-started-linux.md)
- To build or deploy Linux artifacts: [build-script.md](build-script.md)
- To complete browser setup after deployment: [configuration.md](configuration.md)
- To configure hardware on Linux-capable installs: [hardware-device-config.md](hardware-device-config.md)
