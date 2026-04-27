# 打包方案与 Cargo Feature

[English](../en-us/package-profiles-and-features.md) | **中文** | [文档索引](../README.md)

## 1. 范围

本文说明 Beetle OS 当前的打包裁剪合同：

- `Cargo.toml` `[features]` 的分组与语义
- `build.sh --package-profile` 的包型合同
- `package profile` 到 Cargo feature 的映射
- 直接使用 cargo 时的参数写法

## 2. 合同层次

当前打包链分为三层：

1. `Cargo.toml` `[features]`
   - 编译期真源
   - 决定能力域、通道和运行时面是否进入产物
2. `Cargo.toml` `[package.metadata.beetle.package_profiles]`
   - package profile 与 target 默认值真源
   - 决定 `build.sh` / `build.ps1` 的默认包型和每个包型对应的根 feature
3. `build.sh` / `build.ps1`
   - 用户与发布流程入口
   - 只解析 Cargo 真源，不再手写 profile 合同
4. `scripts/expand_cargo_features.py`
   - feature / package-profile 查询工具
   - 用于查看某个根 feature 集或 package profile 最终会展开成哪些本地 feature

## 3. 默认映射

| 入口 | 默认值 |
|------|--------|
| `cargo build` | `default` |
| `build.sh` 的默认 ESP 包型 | `voice+vision+sensor` |
| `build.sh` 的默认 Linux 包型 | `linux-full` |

说明：

- `default` 是 host/developer 默认构建，不是默认 ESP 包
- 默认包型名由 `Cargo.toml [package.metadata.beetle.package_profiles.defaults]` 决定
- 默认 ESP 官方包当前是 `voice+vision+sensor`
- 默认 Linux 官方包当前是 `linux-full`

固件更新说明：

当前 Beetle 主线功能较多，系统包体较大，无法在同时保留现有功能与体验的前提下继续提供官方 OTA 升级能力。如果你需要 OTA，可以自行裁剪功能、重新规划分区表，或联系我们做定制方案。

主线 package profile 当前面向浏览器 USB 烧录、串口烧录和工厂重刷，不再宣称官方 OTA 包型。

## 4. Feature 分组

### 4.1 Runtime bundle

| Feature | 含义 | 默认状态 |
|---------|------|----------|
| `default` | host/developer 默认构建：`default_runtime + 默认通道 + voice/vision/sensor` | `cargo build` 默认开启 |
| `default_runtime` | 基线运行时：`config_api + tools_diagnostics + tools_network_extra + thread_panic_catch` | 被 `default` 和 ESP package profile 复用 |

### 4.2 Capability domain

| Feature | 含义 | 默认状态 |
|---------|------|----------|
| `capability_office` | office 能力域 | 默认关闭，需显式开启 |
| `capability_voice` | 语音能力域 | `default` 开启 |
| `capability_vision` | 视觉能力域 | `default` 开启 |
| `capability_sensor` | 传感器能力域 | `default` 开启 |

### 4.3 Channel surface

| Feature | 含义 | 默认状态 |
|---------|------|----------|
| `telegram` | Telegram 通道 | `default` 开启 |
| `feishu` | 飞书通道 | `default` 开启 |
| `wecom` | 企微通道 | `default` 开启 |
| `qq_channel` | QQ 通道；附带 Ed25519 验签依赖 | `default` 开启 |
| `dingtalk` | 钉钉通道 | 默认关闭；`linux-full` 显式补回 |
| `websocket` | websocket 通道预留面 | 默认关闭；`linux-full` 显式补回 |

### 4.4 Runtime surface

| Feature | 含义 | 默认状态 |
|---------|------|----------|
| `config_api` | 设备配置与控制面 HTTP API | 被 `default_runtime` 带入 |
| `tools_diagnostics` | 诊断工具面 | 被 `default_runtime` 带入 |
| `tools_network_extra` | 扩展联网工具面 | 被 `default_runtime` 带入 |
| `thread_panic_catch` | 线程 panic 收口保护 | 被 `default_runtime` 带入 |

### 4.5 Misc

| Feature | 含义 | 默认状态 |
|---------|------|----------|
| `cli` | CLI 能力 | 默认关闭 |

## 5. `build.sh` package profile 映射

`build.sh` / `build.ps1` 不手写 profile roots 或完整 feature 串。当前实现是：

- 先从 `Cargo.toml [package.metadata.beetle.package_profiles]` 读取包型 roots
- 再从 `Cargo.toml [features]` 动态展开本地 feature 闭包

当前包型合同如下：

| package profile | 根 feature | 说明 |
|-----------------|-----------|------|
| `core-only` | `default_runtime` | 基线 runtime，不带 voice/vision/sensor capability |
| `voice` | `default_runtime + capability_voice` | 语音包 |
| `vision` | `default_runtime + capability_vision` | 视觉包 |
| `sensor` | `default_runtime + capability_sensor` | 传感器包 |
| `voice+vision` | `default_runtime + capability_voice + capability_vision` | 语音 + 视觉 |
| `voice+sensor` | `default_runtime + capability_voice + capability_sensor` | 语音 + 传感器 |
| `vision+sensor` | `default_runtime + capability_vision + capability_sensor` | 视觉 + 传感器 |
| `voice+vision+sensor` | `default_runtime + capability_voice + capability_vision + capability_sensor + qq_channel` | 默认 ESP 官方包 |
| `linux-full` | `default + capability_office + dingtalk + websocket` | 默认 Linux 官方包 |

默认值：

- Linux 目标：`linux-full`
- ESP 目标：`voice+vision+sensor`

## 6. Feature 闭包查看

查看 Linux 官方包型：

```bash
python3 scripts/expand_cargo_features.py \
  --manifest Cargo.toml \
  --package-profile linux-full \
  --format csv
```

查看默认 ESP 包型对应的 shell 参数：

```bash
python3 scripts/expand_cargo_features.py \
  --manifest Cargo.toml \
  --default-target-kind esp \
  --format shell-args
```

## 7. 直接使用 cargo

### 7.1 默认 ESP 包型对应的 feature 组合

```bash
cargo check --bin beetle --no-default-features \
  --features default_runtime,capability_voice,capability_vision,capability_sensor
```

### 7.2 默认 Linux 包型对应的 feature 组合

```bash
cargo check --bin beetle --no-default-features \
  --features default,capability_office,dingtalk,websocket
```

### 7.3 单独验证可选通道

```bash
cargo check --bin beetle --no-default-features \
  --features default_runtime,websocket,capability_voice,capability_vision,capability_sensor
```

## 8. `build.sh` 使用示例

### 8.1 默认 ESP 官方包

```bash
./build.sh
```

等价显式写法：

```bash
./build.sh --package-profile voice+vision+sensor
```

### 8.2 voice-only ESP 包

```bash
./build.sh --package-profile voice
```

### 8.3 默认 Linux 官方包

```bash
TARGET=linux ./build.sh
```

等价显式写法：

```bash
TARGET=linux ./build.sh --package-profile linux-full
```

## 9. 约束

- `default` 不等于默认 ESP 包
- `core-only` 仍然包含 `default_runtime`
- `linux-full` 的当前合同是 `default + capability_office + dingtalk + websocket`
- `capability_office` 是默认关闭，不代表后续禁止在 ESP 包中显式开启
- 若需要查看展开后的完整 feature 集或默认 package profile，应使用 `scripts/expand_cargo_features.py`，不要在外部脚本或文档中复制一份手写列表

## 10. 相关文档

- 构建、烧录与部署：[build-script.md](build-script.md)
- ESP 首次上手：[getting-started-esp.md](getting-started-esp.md)
- Linux 首次上手：[getting-started-linux.md](getting-started-linux.md)
