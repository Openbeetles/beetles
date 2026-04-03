# Linux Agent OS and Package Status

**English** | [中文](../zh-cn/linux-release-rollback.md) | [Doc index](../README.md)

This page describes how Beetle Agent OS is installed, packaged, and rolled back on Linux.

It is relevant if you are:

- running Beetle on Linux
- packaging Beetle for Linux
- managing Linux deployment and rollback

## Current State

- the Linux Agent OS path is stable
- it is the better fit for fuller Agent OS capabilities, longer tasks, and complex integrations
- packaging exists

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

## CI and Rollback

- releases include checksums and provenance
- installation and rollback on the target machine are your responsibility
- CI does not simulate on-machine rollback behavior
