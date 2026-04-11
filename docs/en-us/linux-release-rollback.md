# Linux Agent OS and Package Status

**English** | [中文](../zh-cn/linux-release-rollback.md) | [Doc index](../README.md)

This page describes how Beetle Agent OS is installed, packaged, and rolled back on Linux.

This page is for Linux deployment, packaging, and rollback work, not for first-time setup.

It is relevant if you are:

- running Beetle on Linux
- packaging Beetle for Linux
- managing Linux deployment and rollback

## Current State

- the Linux path already has an independent runtime chain and service-style CLI entrypoints
- its correct positioning is "a long-running Agent service on low-end Linux SBCs", not a desktop app and not merely a board helper program
- packaging, service templates, and rollback layout already exist
- but it should not yet be described as a fully general Linux product shape; the current target is an embedded Linux service lifecycle, not broad distro integration

## If You Are Deploying Manually

Release tarballs include:

- `README.txt`
- a sample `beetle.service`
- a sample `beetle.init` with Debian/LSB headers for SysV compatibility paths

A common manual layout is:

- `/opt/beetle/releases/<version>/`
- a `current` symlink pointing to the active version
- state stored under `BEETLE_STATE_ROOT` or the default Linux state path

Follow the `README.txt` shipped inside the bundle.
Today, `./build.sh --deploy-linux` maintains this layout on the target:

- `/opt/beetle/releases/<release>/`
- `/opt/beetle/current` pointing to the active release
- `/opt/beetle/beetle` as a compatibility shortcut to the active binary
- `/var/lib/beetle` as the default state directory

For non-root deploy accounts:

- the default remote build directory lives under that user's home, for example `/home/beetle/beetle-build`
- writing `/opt/beetle`, `/var/lib/beetle`, and `/etc/systemd/system` requires remote `sudo`

### Service entrypoint and release contract

The Linux service entrypoint is:

- `beetle supervise`

The execution-plane entrypoint is:

- `beetle agent`

That means:

- manual foreground supervisor start should use `beetle supervise`
- the final `systemd` `ExecStart` should point to `.../beetle supervise`
- `/opt/beetle/current/beetle` is always the active release binary

If a device still has an old unit file or an entrypoint without `supervise`, that is not a broken binary. It is **drift between the service template and the CLI contract**.

### `current`, `rollback`, and `pending_validation`

`./build.sh --deploy-linux` now maintains:

- `/opt/beetle/current`: the active release
- `/opt/beetle/rollback`: the previous rollback candidate
- `/var/lib/beetle/runtime/linux_release/state.json`: Linux rollout state
- `/var/lib/beetle/runtime/state_schema.json`: state-root schema version

Each newly deployed release first enters:

- `pending_validation`

That means:

- the new release has become `current`
- the supervisor watches it through the quick-failure window
- if it survives, the release is marked `steady`
- if it fails repeatedly during validation, the supervisor flips `current` back to `rollback`, then exits so the outer service manager restarts Beetle from the rolled-back symlink

### About `smart update`

The deployment modes should be understood as follows:

- Quick deploy: replace the binary only; do not touch service state
- Smart update: replace the binary and restart or preserve an existing service when appropriate; **it does not proactively refresh an old unit file**
- Full deploy: refresh the binary, service/init templates, and install metadata together

Therefore:

- if a release changes the service entrypoint, unit contents, or environment-file contract, do not rely on `smart update` alone
- use **Full deploy**, or refresh `/etc/systemd/system/beetle.service` explicitly

## CI and Rollback

- releases include checksums and provenance
- installation and rollback on the target machine are your responsibility
- CI does not simulate on-machine rollback behavior

On Linux, what really needs rollback is not just one ELF file, but one runtime unit:

- the binary
- the service/unit template
- the state-directory contract
- the `current` / `rollback` symlink targets
- the rollout state

Only when those move together does Beetle behave like a first-class Linux service instead of "a board program copied onto Linux".
