# build.sh Guide

[中文](../zh-cn/build-script.md) | **English** | [Doc index](../README.md)

Read this page if you build, flash, or deploy Beetle from a terminal.

`build.sh` currently covers three jobs:

- building ESP firmware
- building Linux targets and optionally deploying them
- running the on-board C6 helper firmware flow

## Most Common Commands

| Command | What it does |
|---------|---------------|
| `./build.sh` | Interactive build; after success it asks whether to flash or deploy for the current target |
| `./build.sh --flash` | Build and flash the current ESP target |
| `./build.sh --flash-update` | Build and flash directly without entering the erase choice |
| `TARGET=linux ./build.sh` | Build Linux x86_64 |
| `TARGET=linux-armv7 ./build.sh` | Build Linux armv7 |
| `TARGET=linux-aarch64 ./build.sh` | Build Linux aarch64 |
| `./build.sh --deploy-linux` | Deploy an existing Linux build without compiling again |
| `./build.sh build-c6` | Build the on-board C6 helper firmware |
| `./build.sh flash-c6` | Flash the on-board C6 helper firmware |
| `./build.sh flash-all` | Flash C6 first, then flash the P4 main firmware |

## How Target Selection Works

The script decides the target in this order:

1. `TARGET`
2. `BOARD`
3. cargo `--target` passed through extra args
4. `--flash` or `--flash-update`, which default to ESP32-S3
5. the interactive menu

Current `TARGET` values:

- `esp`
- `esp32`
- `esp32s3`
- `p4`
- `esp32p4`
- `linux`
- `linux-armv7`
- `linux-aarch64`

Current `BOARD` presets:

- `esp32-s3-8mb`
- `esp32-s3-16mb`
- `esp32-s3-32mb`
- `esp32-p4-nano-16mb`

If you pass both `BOARD` and cargo `--target`, `--target` wins.

## Common ESP Workflows

```bash
./build.sh
BOARD=esp32-s3-16mb ./build.sh
BOARD=esp32-p4-nano-16mb ./build.sh
./build.sh --flash
BOARD=esp32-s3-16mb ./build.sh --flash
ESPFLASH_PORT=/dev/ttyUSB0 ./build.sh --flash
./build.sh --flash-update
./build.sh --flash --no-monitor
```

Direct takeaways:

- `--flash` enters the flash flow immediately after build
- `--flash` defaults to an update-style flash that keeps NVS; if you need a full erase, the script gives you that choice
- `--flash-update` skips the erase choice and uses update-style flashing directly
- `--no-monitor` means do not open the serial monitor after flashing
- If there is only one clear serial port, the script usually picks it; otherwise it asks

## Common Linux Workflows

```bash
TARGET=linux ./build.sh
TARGET=linux-armv7 ./build.sh
TARGET=linux-aarch64 ./build.sh
./build.sh --deploy-linux
./scripts/docker/linux_armv7_build_docker.sh
```

Direct takeaways:

- On an interactive terminal, a successful Linux build asks whether to deploy over SSH right away
- `--deploy-linux` does not compile; it only deploys an existing artifact
- `--deploy-linux` also syncs shipped official runtime skills from `spiffs_data/skills/*.md` into the remote Beetle state root `skills/` directory; it does not remove user-created skills already on the device
- `scripts/docker/linux_armv7_build_docker.sh` prepares the dedicated local armv7 GNU build container used for the Orange Pi class workflow
- Deployment modes, paths, and rollback live in [linux-release-rollback.md](linux-release-rollback.md)

## Package Profiles

Usage:

```bash
./build.sh --package-profile core-only
TARGET=linux ./build.sh --package-profile linux-full
```

Current package profiles:

| Name | Good fit |
|------|----------|
| `core-only` | the smallest runtime shape |
| `voice` | voice-focused builds |
| `vision` | vision-focused builds |
| `sensor` | sensor-focused builds |
| `voice+vision` | voice and vision only |
| `voice+sensor` | voice and sensor only |
| `vision+sensor` | vision and sensor only |
| `voice+vision+sensor` | the usual full ESP package set |
| `linux-full` | the usual full Linux package set |

Defaults:

- Linux targets default to `linux-full`
- ESP targets default to `voice+vision+sensor`

## Linux Build Methods

`BUILD_METHOD` matters only for Linux targets.

| Value | What it means |
|-------|----------------|
| `auto` | default; let the script choose |
| `local` | build on the current machine |
| `docker` | build Linux targets inside Docker |
| `remote` | sync the repo to a remote Linux host and build there |

Common examples:

```bash
BUILD_METHOD=local TARGET=linux ./build.sh
BUILD_METHOD=docker TARGET=linux-aarch64 ./build.sh
BUILD_METHOD=remote TARGET=linux ./build.sh
```

Direct takeaways:

- On Linux hosts, `auto` usually becomes a local build
- On macOS building Linux, `auto` prefers Docker; if the Docker daemon is not running, it falls back to local musl-cross
- `remote` is only for Linux targets; the script then asks for the remote host, remote directory, and what to do with the artifact after build
- If you only want a ready Linux aarch64 GNU build container, use `./scripts/docker/linux_aarch64_build_docker.sh`

## Interactive Prompt And Non-Interactive Use

By default, after a successful build the script asks what to do next when all of these are true:

- you are in an interactive terminal
- you did not pass `--no-deploy`
- `BEETLE_SKIP_DEPLOY_PROMPT=1` is not set
- you did not already use `--flash`

Common patterns:

```bash
./build.sh --no-deploy
BEETLE_SKIP_DEPLOY_PROMPT=1 ./build.sh
```

Both are useful for automation and CI.

## Common Environment Variables

| Variable | What it is for |
|----------|-----------------|
| `TARGET` | set the platform target directly |
| `BOARD` | choose a board preset |
| `BUILD_METHOD` | choose the Linux build method |
| `PACKAGE_PROFILE` | set the default package profile |
| `ESPFLASH_PORT` | choose the ESP serial port |
| `ESP_HOSTED_C6_PORT` | choose the on-board C6 serial port |
| `BEETLE_SKIP_DEPLOY_PROMPT` | skip the post-build flash/deploy prompt |

## Where Build Artifacts Go

- Linux artifacts go to `target/<target>/release/beetle`
- ESP artifacts go to `target/<target>/release-size/beetle`
- ESP builds also emit `target/<target>/release-size/beetle.bin` for later flashing
- That image is now produced through `espflash save-image`, so build-only runs do not depend on Python `esptool` module imports
- After a successful build, the script prints the exact artifact path

If you plan to use `--deploy-linux`, make sure the matching Linux artifact already exists.

## Read Next

- For Linux deploy modes and rollback: [linux-release-rollback.md](linux-release-rollback.md)
- To get Beetle running first: [configuration.md](configuration.md)
- To check boards and hardware direction: [hardware.md](hardware.md)
