# 构建、烧录与部署

[English](../en-us/build-script.md) | **中文** | [文档索引](../README.md)

`build.sh` 是项目的终端构建入口，用于 ESP 烧录、Linux 构建和 Linux 部署。

## 常见任务

| 任务 | 命令 |
|------|------|
| ESP 固件烧录 | `./build.sh --flash` |
| Linux x86_64 构建 | `TARGET=linux ./build.sh` |
| Linux armv7 构建 | `TARGET=linux-armv7 ./build.sh` |
| Linux aarch64 构建 | `TARGET=linux-aarch64 ./build.sh` |
| Linux 发布包构建 | `TARGET=linux ./build.sh --package-linux` |
| 已构建 Linux 产物部署 | `./build.sh --deploy-linux` |
| 板载 C6 辅助固件烧录 | `./build.sh flash-c6` |
| C6 与 P4 固件顺序烧录 | `./build.sh flash-all` |

## `build.sh` 的职责范围

`build.sh` 现在负责三类主要用法：

- 构建 ESP 固件
- 构建 Linux 目标并按需部署
- 调用板载 C6 辅助固件流程

## 目标选择顺序

脚本会按这个顺序决定目标：

1. `TARGET`
2. `BOARD`
3. 你额外传给 cargo 的 `--target`
4. `--flash` 或 `--flash-update`，默认走 ESP32-S3
5. 上面都没给时进入交互菜单

当前支持这些 `TARGET`：

- `esp`
- `esp32`
- `esp32s3`
- `p4`
- `esp32p4`
- `linux`
- `linux-armv7`
- `linux-aarch64`

当前板型预设有这些 `BOARD`：

- `esp32-s3-8mb`
- `esp32-s3-16mb`
- `esp32-s3-32mb`
- `esp32-p4-nano-16mb`

如果你同时给了 `BOARD` 和 cargo `--target`，最后以 `--target` 为准。

## ESP 示例

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

说明：

- `--flash` 会在构建完成后直接进入烧录流程
- `--flash` 默认保留 NVS；如果你需要全擦，脚本会给你选项
- `--flash-update` 不进擦除选择，直接按更新方式烧录；会原地刷新 bootloader、分区表、otadata 和 app，但不会整片擦除。NVS 会保留；只有 SPIFFS 分区 offset 和 size 都不变时，SPIFFS 配置才可认为安全保留。
- `--no-monitor` 表示烧录完成后不打开串口监视
- 若串口可唯一识别，脚本自动选择该串口；否则进入选择流程

## ESP panic 定责与产物身份

每次 ESP 构建完成后，`build.sh` 会把符号化所需产物归档到：

```bash
target/esp-artifacts/<git-sha>-<elf-sha>/
```

目录内包含默认用于 Rust 地址符号化的 `beetle.elf`，以及 `libespidf.elf`、`libespidf.map`、`partition-table.bin` 和 `artifact.env`。启动日志会打印同一轮构建的 `git_sha`、构建时间、app ELF SHA、运行中分区表 SHA 与分区布局摘要。

发生 Guru Meditation / panic 时，先记录启动日志里的 artifact identity，再用同一目录符号化地址：

```bash
scripts/esp_symbolize_panic.sh target/esp-artifacts/<artifact-id> 0x4037f815
```

禁止用其他构建轮次的 ELF/map 猜地址；ESP panic 定责必须以匹配的 artifact id 为准。

`--flash-update` 会刷新 bootloader、编译后的分区表、otadata 和 app；只有当 NVS/SPIFFS 的 offset 与 size 不变时，数据区才可被保留，避免新 app 搭配旧分区表污染排查结论；如果改变 SPIFFS extent，ESP-IDF 可能会格式化文件系统。默认 16MB S3 分区表删除旧 `model` 分区，并保持当前迁移后的 SPIFFS extent 为 `0xA20000/0x5D0000`；除非明确要迁移或格式化用户配置，否则不要再次改变这个 extent。

## Linux 示例

```bash
TARGET=linux ./build.sh
TARGET=linux-armv7 ./build.sh
TARGET=linux-aarch64 ./build.sh
TARGET=linux ./build.sh --package-linux
./build.sh --deploy-linux
```

说明：

- 交互式构建成功后，脚本会问你是否立刻通过 SSH 部署
- `--package-linux` 会构建当前 Linux 目标，并把发布 tarball 写到 `dist/`
- `--package-linux` 会直接从 `Cargo.toml package.version` 生成 bundle 版本，不再要求额外手工跑第二条打包命令
- `BUILD_METHOD=auto` 现在不再弹构建方式菜单：macOS 下优先 Docker，其次已保存的远端 Linux 主机，最后才回落到本地交叉构建
- `--deploy-linux` 不重新编译，只部署现有产物
- `--deploy-linux` 还会把 `spiffs_data/skills/*.md` 里的官方运行时技能同步到远端 Beetle OS state root 的 `skills/` 目录
- `./build.sh` 是 Linux 构建和部署的主入口；Docker helper 脚本只是 `BUILD_METHOD=docker` 背后的内部帮手
- `TARGET=linux BUILD_METHOD=docker` 在 amd64 Linux 容器内构建 GNU 目标，避免把 Linux 系统库依赖强行变成 musl 交叉 sysroot 问题
- ARM Linux 目标在 `BUILD_METHOD=docker` 下会自动拉起对应的 GNU 构建容器

## 打包方案

`build.sh` 的 package profile 与 `Cargo.toml` feature/metadata 是两层合同：

- `package profile`：用户与发布流程入口，但 profile 名和 roots 都从 `Cargo.toml [package.metadata.beetle.package_profiles]` 解析
- feature 闭包：从 `Cargo.toml [features]` 动态展开

详细映射、直接 cargo 写法和默认合同见：

- [package-profiles-and-features.md](package-profiles-and-features.md)

用法：

```bash
./build.sh --package-profile core-only
TARGET=linux ./build.sh --package-profile linux-full
```

当前支持这些 `package profile`：

| 名称 | 适合什么时候用 |
|------|----------------|
| `core-only` | 只保留最小运行能力 |
| `voice` | 只要语音相关能力 |
| `vision` | 只要视觉相关能力 |
| `sensor` | 只要传感器相关能力 |
| `voice+vision` | 只要语音和视觉 |
| `voice+sensor` | 只要语音和传感器 |
| `vision+sensor` | 只要视觉和传感器 |
| `voice+vision+sensor` | ESP 常用的完整组合 |
| `linux-full` | Linux 常用的完整组合 |

默认值：

- Linux 目标默认用 `linux-full`
- ESP 目标默认用 `voice+vision+sensor`
- 这两个默认值现在由 `Cargo.toml [package.metadata.beetle.package_profiles.defaults]` 决定
- `linux-full` 当前固定从 `default + capability_office + dingtalk + websocket` 展开，作为 Linux 默认不裁剪运行面
- ESP 各 profile 当前都从 `default_runtime` 起步；默认 ESP 包额外显式带入 `qq_channel`

## Linux 构建方式

`BUILD_METHOD` 只对 Linux 目标有意义。

| 值 | 用途 |
|----|------|
| `auto` | 默认值，非交互自动选后端 |
| `local` | 在当前机器本地构建 |
| `docker` | 在 Docker 里构建 Linux 目标 |
| `remote` | 把工程同步到远端 Linux 主机，在远端构建 |

常见写法：

```bash
BUILD_METHOD=local TARGET=linux ./build.sh
BUILD_METHOD=docker TARGET=linux-aarch64 ./build.sh
BUILD_METHOD=remote TARGET=linux ./build.sh
```

`auto` 当前的决策顺序是：

- Linux 主机上直接本地构建
- macOS 上优先用可用的 Docker daemon
- macOS 如果 Docker 不可用，则优先复用已保存的远端 Linux 主机
- 再不行才回落到本地交叉构建

## 交互行为与非交互执行

默认情况下，构建成功后脚本会继续追问下一步，只要满足这些条件：

- 当前是交互终端
- 没有传 `--no-deploy`
- 没有设置 `BEETLE_SKIP_DEPLOY_PROMPT=1`
- 当前不是已经显式用了 `--flash`
- 当前不是已经显式用了 `--package-linux`

示例：

```bash
./build.sh --no-deploy
BEETLE_SKIP_DEPLOY_PROMPT=1 ./build.sh
```

这两种都适合自动化或 CI。

## 常用环境变量

| 变量 | 用途 |
|------|------|
| `TARGET` | 直接指定平台目标 |
| `BOARD` | 选板型预设 |
| `BUILD_METHOD` | 选择 Linux 构建方式 |
| `PACKAGE_PROFILE` | 设默认打包方案 |
| `ESPFLASH_PORT` | 指定 ESP 烧录串口 |
| `ESP_HOSTED_C6_PORT` | 指定板载 C6 烧录串口 |
| `BEETLE_SKIP_DEPLOY_PROMPT` | 跳过构建后的部署/烧录追问 |

## 产物路径

- Linux 产物在 `target/<target>/release/beetle`
- `--package-linux` 生成的 Linux 发布包在 `dist/beetle-v<version>-linux-<arch>.tar.gz`
- ESP 产物在 `target/<target>/release-size/beetle`
- ESP 构建还会额外生成 `target/<target>/release-size/beetle.bin`
- 这个镜像由 `espflash save-image` 生成，所以纯构建路径不再依赖 Python `esptool` 导入
- 构建成功后，脚本会把实际产物路径直接打印出来

如果你打算用 `--deploy-linux`，先确认对应 Linux 产物已经存在。如果你需要可分发 tarball，而不是直接 SSH 部署，就用 `--package-linux`。

## 相关文档

- ESP32 首次部署：[getting-started-esp.md](getting-started-esp.md)
- Linux 首次部署：[getting-started-linux.md](getting-started-linux.md)
- Linux 运维与回滚：[linux-release-rollback.md](linux-release-rollback.md)
- 板型与硬件范围：[hardware.md](hardware.md)
- 打包方案与 Cargo Feature：[package-profiles-and-features.md](package-profiles-and-features.md)
