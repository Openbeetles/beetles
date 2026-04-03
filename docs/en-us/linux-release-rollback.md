# Linux Package Status

**English** | [中文](../zh-cn/linux-release-rollback.md) | [Doc index](../README.md)

This page is not for normal firmware users.

It is only for people who are dealing with the Linux release bundles directly.

## Current State

- Linux packaging exists
- it is aimed at integrators and operators
- it is not yet a polished one-click install flow

## If You Are Deploying Manually

Release tarballs include:

- `README.txt`
- a sample `beetle.service`

A common manual layout is:

- `/opt/beetle/releases/<version>/`
- a `current` symlink pointing to the active version
- state stored under `BEETLE_STATE_ROOT` or the runtime default

Follow the instructions shipped inside the bundle. That is the authoritative source for the package you are installing.

## CI and Rollback

- releases include checksums and provenance
- installation and rollback on the target machine are your responsibility
- CI does not simulate on-machine rollback behavior
