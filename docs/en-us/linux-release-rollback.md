# Linux Agent OS and Package Status

**English** | [中文](../zh-cn/linux-release-rollback.md) | [Doc index](../README.md)

This page describes how Beetle Agent OS is installed, packaged, and rolled back on Linux.

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

### Service entrypoint and one common failure mode

The current Linux service entrypoint is:

- `beetle run`

That means:

- manual foreground start should use `beetle run`
- the final `systemd` `ExecStart` should also point to `.../beetle run`

This matters because a device with an older unit file such as `ExecStart=/opt/beetle/current/beetle` will print CLI help and exit. That can look like a broken binary, but the real issue is **drift between the service template and the CLI contract**.

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
- the active symlink target

Only when those move together does Beetle behave like a first-class Linux service instead of "a board program copied onto Linux".
