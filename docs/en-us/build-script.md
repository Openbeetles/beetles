# Build, Flash, and Deploy

[中文](../zh-cn/build-script.md) | **English** | [Doc index](../README.md)

`build.sh` is the terminal-first path for building, flashing, and deploying Beetle OS.

## Common Tasks

| What you want to do | Starting command |
|---------------------|-----------------|
| Flash Beetle OS to an ESP board | `./build.sh --flash` |
| Build Linux x86_64 | `TARGET=linux ./build.sh` |
| Build Linux armv7 | `TARGET=linux-armv7 ./build.sh` |
| Build Linux aarch64 | `TARGET=linux-aarch64 ./build.sh` |
| Build and package a Linux bundle | `TARGET=linux ./build.sh --package-linux` |
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
- `--flash-update` skips the erase choice and uses update-style flashing directly; it refreshes bootloader, partition table, otadata, and app in place without a full-chip erase. NVS is kept, but SPIFFS config is safe only when the SPIFFS partition offset and size are unchanged.
- `--no-monitor` means do not open the serial monitor after flashing
- if the serial port is obvious, the script usually picks it; otherwise it asks

## ESP Panic Attribution And Artifact Identity

After every ESP build, `build.sh` archives the files needed for symbolization under:

```bash
target/esp-artifacts/<git-sha>-<elf-sha>/
```

The directory contains `beetle.elf` as the default Rust symbolization ELF, plus `libespidf.elf`, `libespidf.map`, `partition-table.bin`, and `artifact.env`. Startup logs print the matching build `git_sha`, build time, app ELF SHA, runtime partition-table SHA, and parsed partition layout summary.

When a Guru Meditation / panic happens, record the artifact identity from the startup log first, then symbolize addresses with the matching artifact directory:

```bash
scripts/esp_symbolize_panic.sh target/esp-artifacts/<artifact-id> 0x4037f815
```

Do not guess final addresses with an ELF/map from another build. ESP panic attribution must start from the matching artifact id.

`--flash-update` refreshes bootloader, the compiled partition table, otadata, and the app while preserving data partitions such as NVS/SPIFFS only when their offset and size stay unchanged. This prevents a new app from running against an old partition table during bring-up, but changing the SPIFFS extent can make ESP-IDF format the filesystem. The default 16MB S3 layout removes the old `model` partition and keeps the current post-migration SPIFFS extent at `0xA20000/0x5D0000`; do not change that extent again unless you are intentionally migrating or reformatting user configuration.

## Common Linux Workflows

```bash
TARGET=linux ./build.sh
TARGET=linux-armv7 ./build.sh
TARGET=linux-aarch64 ./build.sh
TARGET=linux ./build.sh --package-linux
./build.sh --deploy-linux
```

What matters most:

- on an interactive terminal, a successful Linux build asks whether to deploy over SSH right away
- `--package-linux` builds the selected Linux target and writes a release tarball to `dist/`
- `--package-linux` derives the bundle version from `Cargo.toml package.version`, so the public release path no longer needs a second manual packaging command
- `BUILD_METHOD=auto` is now non-interactive: on macOS it prefers Docker, then a saved remote Linux host, and only then falls back to local cross-build
- `--deploy-linux` does not compile; it deploys an existing artifact
- `--deploy-linux` also syncs shipped official runtime skills from `spiffs_data/skills/*.md` into the remote Beetle OS state root `skills/` directory
- `./build.sh` is the main Linux build and deploy entry; Docker helper scripts are internal helpers behind `BUILD_METHOD=docker`
- `TARGET=linux BUILD_METHOD=docker` builds the GNU target inside an amd64 Linux container, avoiding a fake musl cross sysroot for normal Linux system-library dependencies
- for ARM Linux targets, `BUILD_METHOD=docker` automatically boots the matching host-architecture GNU build container

## Package Profiles

`build.sh` package profiles and `Cargo.toml` feature/metadata are separate contract layers:

- package profile: user-facing and release-facing entrypoint, but names and roots now resolve from `Cargo.toml [package.metadata.beetle.package_profiles]`
- feature closure: expanded dynamically from `Cargo.toml [features]`

For the full mapping, direct cargo forms, and default contract, see:

- [package-profiles-and-features.md](package-profiles-and-features.md)

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
- both defaults now come from `Cargo.toml [package.metadata.beetle.package_profiles.defaults]`
- `linux-full` currently expands from `default + capability_office + dingtalk + websocket`, so Linux defaults no longer trim hosted runtime channels
- ESP profiles currently all start from `default_runtime`; the default ESP package also carries `qq_channel`

## Linux Build Methods

`BUILD_METHOD` matters only for Linux targets.

| Value | What it means |
|-------|---------------|
| `auto` | default; non-interactive backend selection |
| `local` | build on the current machine |
| `docker` | build Linux targets inside Docker |
| `remote` | sync the repo to a remote Linux host and build there |

Common examples:

```bash
BUILD_METHOD=local TARGET=linux ./build.sh
BUILD_METHOD=docker TARGET=linux-aarch64 ./build.sh
BUILD_METHOD=remote TARGET=linux ./build.sh
```

`auto` currently resolves like this:

- on Linux hosts: local build
- on macOS: Docker when the daemon is reachable
- otherwise on macOS: a saved remote Linux host, if one was already configured
- otherwise: local cross-build as the last fallback

## Interactive Prompt And Non-Interactive Use

By default, after a successful build the script asks what to do next when all of these are true:

- you are in an interactive terminal
- you did not pass `--no-deploy`
- `BEETLE_SKIP_DEPLOY_PROMPT=1` is not set
- you did not already use `--flash`
- you did not already use `--package-linux`

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
- Linux release bundles written by `--package-linux` go to `dist/beetle-v<version>-linux-<arch>.tar.gz`
- ESP artifacts go to `target/<target>/release-size/beetle`
- ESP builds also emit `target/<target>/release-size/beetle.bin` for later flashing
- that image is produced through `espflash save-image`, so build-only runs do not depend on Python `esptool` imports
- after a successful build, the script prints the exact artifact path

If you plan to use `--deploy-linux`, make sure the matching Linux artifact already exists. If you want a distributable tarball instead of an SSH deploy, use `--package-linux`.

## Read Next

- To get started on ESP32: [getting-started-esp.md](getting-started-esp.md)
- To get started on Linux: [getting-started-linux.md](getting-started-linux.md)
- For Linux deploy modes and rollback: [linux-release-rollback.md](linux-release-rollback.md)
- To check boards and hardware direction: [hardware.md](hardware.md)
- For package profiles and Cargo features: [package-profiles-and-features.md](package-profiles-and-features.md)
