# build.sh 使用说明

[English](../en-us/build-script.md) | **中文** | [文档索引](../README.md)

如果你想通过终端构建、烧录或部署 Beetle，看这页。

`build.sh` 现在负责三类事情：

- 构建 ESP 固件
- 构建 Linux 版本并按需部署
- 调用板载 C6 辅助固件流程

## 最常用命令

| 命令 | 用途 |
|------|------|
| `./build.sh` | 交互式构建；成功后会按当前目标询问是否继续烧录或部署 |
| `./build.sh --flash` | 构建并烧录当前 ESP 目标 |
| `./build.sh --flash-update` | 构建并直接更新烧录，不进入擦除选择 |
| `TARGET=linux ./build.sh` | 构建 Linux x86_64 |
| `TARGET=linux-armv7 ./build.sh` | 构建 Linux armv7 |
| `TARGET=linux-aarch64 ./build.sh` | 构建 Linux aarch64 |
| `./build.sh --deploy-linux` | 只部署已经编好的 Linux 产物，不重新编译 |
| `./build.sh build-c6` | 构建板载 C6 辅助固件 |
| `./build.sh flash-c6` | 烧录板载 C6 辅助固件 |
| `./build.sh flash-all` | 先烧录 C6，再烧录 P4 主固件 |

## 平台怎么选

脚本会按这个顺序决定目标：

1. 先看 `TARGET`
2. 再看 `BOARD`
3. 再看你额外传给 cargo 的 `--target`
4. 如果用了 `--flash` 或 `--flash-update`，默认走 ESP32-S3
5. 上面都没给时，进入交互菜单

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

## 常见 ESP 用法

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

直接结论：

- `--flash` 会在构建完成后直接进入烧录流程
- `--flash` 默认是“更新烧录”，保留 NVS；如果需要全擦，脚本会给你选项
- `--flash-update` 不进擦除选择，直接按更新方式烧录
- `--no-monitor` 表示烧录完成后不打开串口监视
- 如果串口只有一个，或者脚本能明确判断，通常会自动选口；否则会让你选

## 常见 Linux 用法

```bash
TARGET=linux ./build.sh
TARGET=linux-armv7 ./build.sh
TARGET=linux-aarch64 ./build.sh
./build.sh --deploy-linux
./scripts/docker/linux_armv7_build_docker.sh
```

直接结论：

- 交互式构建成功后，脚本会问你是否立刻通过 SSH 部署
- `--deploy-linux` 不会重新编译，只会拿现有产物去部署
- `--deploy-linux` 还会把 `spiffs_data/skills/*.md` 里的官方运行时技能同步到远端 Beetle state root 的 `skills/` 目录；不会删除设备上已有的用户自定义技能
- `scripts/docker/linux_armv7_build_docker.sh` 会准备 Orange Pi 这类 armv7 GNU 本地专用构建容器
- Linux 部署模式、目录和回滚，单独见 [linux-release-rollback.md](linux-release-rollback.md)

## 打包方案

用法：

```bash
./build.sh --package-profile core-only
TARGET=linux ./build.sh --package-profile linux-full
```

当前支持这些 `package profile`：

| 名称 | 适合什么时候用 |
|------|----------------|
| `core-only` | 只保留基础运行能力 |
| `voice` | 只要语音相关能力 |
| `vision` | 只要视觉相关能力 |
| `sensor` | 只要传感器相关能力 |
| `voice+vision` | 只要语音和视觉 |
| `voice+sensor` | 只要语音和传感器 |
| `vision+sensor` | 只要视觉和传感器 |
| `voice+vision+sensor` | ESP 常用的完整能力组合 |
| `linux-full` | Linux 常用的完整能力组合 |

默认值：

- Linux 目标默认用 `linux-full`
- ESP 目标默认用 `voice+vision+sensor`

## Linux 构建方式

`BUILD_METHOD` 只对 Linux 目标有意义。

| 值 | 用途 |
|----|------|
| `auto` | 默认值。脚本自己选 |
| `local` | 在当前机器本地构建 |
| `docker` | 在 Docker 里构建 Linux 目标 |
| `remote` | 把工程同步到远端 Linux 主机，在远端本机构建 |

常见写法：

```bash
BUILD_METHOD=local TARGET=linux ./build.sh
BUILD_METHOD=docker TARGET=linux-aarch64 ./build.sh
BUILD_METHOD=remote TARGET=linux ./build.sh
```

直接结论：

- 在 Linux 主机上，`auto` 通常就是本地构建
- 在 macOS 上构建 Linux，`auto` 会优先用 Docker；如果 Docker daemon 没启动，就退回本地 musl-cross
- `remote` 只适合 Linux 目标，脚本会继续问远端地址、远端目录，以及构建完成后怎么处理产物
- 如果你只是想先准备一个 Linux aarch64 GNU 构建容器，可以看 `./scripts/docker/linux_aarch64_build_docker.sh`

## 交互提示和非交互用法

默认情况下，只要满足这几个条件，构建成功后脚本就会追问下一步：

- 当前是交互终端
- 没有传 `--no-deploy`
- 没有设置 `BEETLE_SKIP_DEPLOY_PROMPT=1`
- 当前不是已经显式使用了 `--flash`

常见做法：

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

## 产物在哪里

- Linux 产物在 `target/<target>/release/beetle`
- ESP 产物在 `target/<target>/release-size/beetle`
- ESP 构建还会额外生成 `target/<target>/release-size/beetle.bin`，供后续烧录直接复用
- 这一步现在由 `espflash save-image` 完成，纯构建路径不再依赖 Python `esptool` 模块是否可导入
- 构建成功后，脚本会把实际产物路径直接打印出来

如果你打算用 `--deploy-linux`，先确认对应 Linux 产物已经存在。

## 接下来读什么

- 要看 Linux 部署模式和回滚： [linux-release-rollback.md](linux-release-rollback.md)
- 要先把 Beetle 跑起来： [configuration.md](configuration.md)
- 要看板型和硬件方向： [hardware.md](hardware.md)
