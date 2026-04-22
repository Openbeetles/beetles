# Build, Flash, and Deploy

[中文](../zh-cn/build-script.md) | **English** | [Doc index](../README.md)

`build.sh` is the terminal-first path for building, flashing, and deploying Beetls OS.

## Common Tasks

| What you want to do | Starting command |
|---------------------|-----------------|
| Flash Beetls OS to an ESP board | `./build.sh --flash` |
| Build Linux x86_64 | `TARGET=linux ./build.sh` |
| Build Linux armv7 | `TARGET=linux-armv7 ./build.sh` |
| Build Linux aarch64 | `TARGET=linux-aarch64 ./build.sh` |
| Deploy an existing Linux artifact | `./build.sh --deploy-linux` |
| Flash the on-board C6 helper firmware | `./build.sh flash-c6` |
| Flash C6 and then the P4 main firmware | `./build.sh flash-all` |

## What `build.sh` Covers

`build.sh` currently handles three main jobs:

- building ESP firmware
- building Linux targets and optionally deploying them
- running the on-board C6 helper firmware flow

## How Target Selection Works

The script chooses the target in this order:

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

What matters most:

- `--flash` goes straight into the flash flow after build
- `--flash` keeps NVS by default; if you need a full erase, the script offers that choice
- `--flash-update` skips the erase choice and uses update-style flashing directly
- `--no-monitor` means do not open the serial monitor after flashing
- if the serial port is obvious, the script usually picks it; otherwise it asks

## Common Linux Workflows

```bash
TARGET=linux ./build.sh
TARGET=linux-armv7 ./build.sh
TARGET=linux-aarch64 ./build.sh
./build.sh --deploy-linux
```

What matters most:

- on an interactive terminal, a successful Linux build asks whether to deploy over SSH right away
- `--deploy-linux` does not compile; it deploys an existing artifact
- `--deploy-linux` also syncs shipped official runtime skills from `spiffs_data/skills/*.md` into the remote Beetls OS state root `skills/` directory
- `./build.sh` is the main Linux build and deploy entry; Docker helper scripts are internal helpers behind `BUILD_METHOD=docker`
- for ARM Linux targets, `BUILD_METHOD=docker` automatically boots the matching host-architecture GNU build container

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
|-------|---------------|
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
- that image is produced through `espflash save-image`, so build-only runs do not depend on Python `esptool` imports
- after a successful build, the script prints the exact artifact path

If you plan to use `--deploy-linux`, make sure the matching Linux artifact already exists.

## Read Next

- To get started on ESP32: [getting-started-esp.md](getting-started-esp.md)
- To get started on Linux: [getting-started-linux.md](getting-started-linux.md)
- For Linux deploy modes and rollback: [linux-release-rollback.md](linux-release-rollback.md)
- To check boards and hardware direction: [hardware.md](hardware.md)
