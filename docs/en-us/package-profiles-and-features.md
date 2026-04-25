# Package Profiles and Cargo Features

[中文](../zh-cn/package-profiles-and-features.md) | **English** | [Doc index](../README.md)

## 1. Scope

This document defines the current Beetle OS packaging contract:

- the grouping and meaning of `Cargo.toml` `[features]`
- the package contract exposed by `build.sh --package-profile`
- the mapping from package profiles to Cargo features
- the direct cargo command forms

## 2. Contract Layers

The packaging path has three layers:

1. `Cargo.toml` `[features]`
   - compile-time source of truth
   - controls whether a capability domain, channel, or runtime surface is compiled in
2. `Cargo.toml` `[package.metadata.beetle.package_profiles]`
   - source of truth for package profiles and target defaults
   - decides which roots each `build.sh` / `build.ps1` package profile uses
3. `build.sh` / `build.ps1`
   - user-facing and release-facing package entrypoints
   - only resolve package metadata from Cargo; they no longer hand-maintain profile contracts
4. `scripts/expand_cargo_features.py`
   - feature / package-profile inspection tool
   - expands a root feature set or a package profile into the local Cargo feature closure

## 3. Default Mapping

| Entry | Default |
|------|---------|
| `cargo build` | `default` |
| default ESP package in `build.sh` | `voice+vision+sensor` |
| default Linux package in `build.sh` | `linux-full` |

Notes:

- `default` is the host/developer default build, not the default ESP package
- default package-profile names come from `Cargo.toml [package.metadata.beetle.package_profiles.defaults]`
- the current default official ESP package is `voice+vision+sensor`
- the current default official Linux package is `linux-full`

## 4. Feature Groups

### 4.1 Runtime bundle

| Feature | Meaning | Default status |
|---------|---------|----------------|
| `default` | host/developer default build: `default_runtime + default channels + voice/vision/sensor` | enabled by default for `cargo build` |
| `default_runtime` | baseline runtime: `config_api + tools_diagnostics + tools_network_extra + thread_panic_catch` | reused by `default` and ESP package profiles |

### 4.2 Capability domain

| Feature | Meaning | Default status |
|---------|---------|----------------|
| `capability_office` | office capability domain | off by default; must be enabled explicitly |
| `capability_voice` | voice capability domain | enabled by `default` |
| `capability_vision` | vision capability domain | enabled by `default` |
| `capability_sensor` | sensor capability domain | enabled by `default` |

### 4.3 Channel surface

| Feature | Meaning | Default status |
|---------|---------|----------------|
| `telegram` | Telegram channel | enabled by `default` |
| `feishu` | Feishu channel | enabled by `default` |
| `wecom` | WeCom channel | enabled by `default` |
| `qq_channel` | QQ channel; includes Ed25519 verification dependency | enabled by `default` |
| `dingtalk` | DingTalk channel | off by default; added explicitly by `linux-full` |
| `websocket` | reserved websocket channel surface | off by default; added explicitly by `linux-full` |

### 4.4 Runtime surface

| Feature | Meaning | Default status |
|---------|---------|----------------|
| `config_api` | device configuration and control-plane HTTP API | included by `default_runtime` |
| `tools_diagnostics` | diagnostics tool surface | included by `default_runtime` |
| `tools_network_extra` | extra network tool surface | included by `default_runtime` |
| `thread_panic_catch` | thread panic containment | included by `default_runtime` |

### 4.5 Misc

| Feature | Meaning | Default status |
|---------|---------|----------------|
| `cli` | CLI surface | off by default |
| `ota` | OTA surface | off by default |

## 5. `build.sh` Package Profile Mapping

`build.sh` / `build.ps1` do not hand-maintain package-profile roots or a fully expanded feature string. The current contract is:

- resolve package-profile roots from `Cargo.toml [package.metadata.beetle.package_profiles]`
- expand the local feature closure from `Cargo.toml [features]`

Current package profiles:

| package profile | root features | Meaning |
|-----------------|--------------|---------|
| `core-only` | `default_runtime` | baseline runtime without voice/vision/sensor capability domains |
| `voice` | `default_runtime + capability_voice` | voice package |
| `vision` | `default_runtime + capability_vision` | vision package |
| `sensor` | `default_runtime + capability_sensor` | sensor package |
| `voice+vision` | `default_runtime + capability_voice + capability_vision` | voice + vision |
| `voice+sensor` | `default_runtime + capability_voice + capability_sensor` | voice + sensor |
| `vision+sensor` | `default_runtime + capability_vision + capability_sensor` | vision + sensor |
| `voice+vision+sensor` | `default_runtime + capability_voice + capability_vision + capability_sensor + qq_channel` | default official ESP package |
| `linux-full` | `default + capability_office + dingtalk + websocket` | default official Linux package |

Defaults:

- Linux targets: `linux-full`
- ESP targets: `voice+vision+sensor`

## 6. Inspecting the Feature Closure

Inspect the official Linux package:

```bash
python3 scripts/expand_cargo_features.py \
  --manifest Cargo.toml \
  --package-profile linux-full \
  --format csv
```

Inspect the shell args for the default ESP package:

```bash
python3 scripts/expand_cargo_features.py \
  --manifest Cargo.toml \
  --default-target-kind esp \
  --format shell-args
```

## 7. Direct Cargo Forms

### 7.1 Default ESP feature shape

```bash
cargo check --bin beetle --no-default-features \
  --features default_runtime,capability_voice,capability_vision,capability_sensor
```

### 7.2 Default Linux feature shape

```bash
cargo check --bin beetle --no-default-features \
  --features default,capability_office,dingtalk,websocket
```

### 7.3 Validate one optional channel

```bash
cargo check --bin beetle --no-default-features \
  --features default_runtime,websocket,capability_voice,capability_vision,capability_sensor
```

## 8. `build.sh` Examples

### 8.1 Default official ESP package

```bash
./build.sh
```

Equivalent explicit form:

```bash
./build.sh --package-profile voice+vision+sensor
```

### 8.2 Voice-only ESP package

```bash
./build.sh --package-profile voice
```

### 8.3 Default official Linux package

```bash
TARGET=linux ./build.sh
```

Equivalent explicit form:

```bash
TARGET=linux ./build.sh --package-profile linux-full
```

## 9. Constraints

- `default` is not the default ESP package
- `core-only` still includes `default_runtime`
- the current `linux-full` contract is `default + capability_office + dingtalk + websocket`
- `capability_office` is default-off; that does not prohibit explicit re-enablement in future ESP packages
- if the expanded full feature set or a default package profile is needed, inspect it through `scripts/expand_cargo_features.py`; do not maintain a hand-written list in another script or document

## 10. Related Documents

- Build, flash, and deploy: [build-script.md](build-script.md)
- First-time ESP setup: [getting-started-esp.md](getting-started-esp.md)
- First-time Linux setup: [getting-started-linux.md](getting-started-linux.md)
