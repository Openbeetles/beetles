# Linux 运维

[English](../en-us/linux-release-rollback.md) | **中文** | [文档索引](../README.md)

本页说明 Beetle OS 在 Linux 环境中的部署、重启、停止与回滚机制。

## 基础流程

常见 Linux 发布流程如下：

1. 先用 [build-script.md](build-script.md) 构建 Linux 产物，或用 `TARGET=linux ./build.sh --package-linux` 直接生成可分发 tarball
2. 用 `./build.sh --deploy-linux` 部署
3. 用 `beetle release status` 查看当前发布状态
4. 只有在需要时再用 `beetle restart`、`beetle stop` 或 `beetle release rollback`

## 部署后常见目录

当前部署脚本会维护这些路径：

- `/opt/beetle/releases/<release>/`
- `/opt/beetle/current`
- `/opt/beetle/rollback`
- `/usr/local/bin/beetle`
- `/usr/bin/beetle`（当目标 shell 默认 `PATH` 不含 `/usr/local/bin` 时作为兼容入口）
- `/var/lib/beetle`

其中：

- `current` 指向当前正在运行的版本
- `rollback` 指向上一个可回退版本
- `/usr/local/bin/beetle` 是主要的全局命令入口
- 某些嵌入式 shell 不搜 `/usr/local/bin` 时，会额外创建 `/usr/bin/beetle`

## 运行入口

Linux 服务入口是：

- `beetle run`

控制面（HTTP API）和 agent 运行在同一个进程内。
主运行时不会再额外派生子进程。

如果你用 `systemd`，`ExecStart` 应该指向：

- `/opt/beetle/current/beetle run`

## 三种部署模式

`./build.sh --deploy-linux` 当前有三种模式：

1. `Quick deploy`
   只换二进制
2. `Full deploy`
   刷新二进制和服务安装内容
3. `Smart update`
   替换二进制，并在合适的时候重启现有服务

如果这次改动涉及服务入口或服务文件，不要只靠 `Smart update`。

## 回滚机制

新版本部署后，会先进入：

- `pending_validation`

如果新版本稳定运行，就会变成稳定版本。
如果在验证窗口里连续快速失败，运行时会优先回退到 `rollback`。

## 排查位置

- 当前版本：`/opt/beetle/current`
- 回滚版本：`/opt/beetle/rollback`
- 发布状态：`/var/lib/beetle/runtime/linux_release/state.json`
- 全局命令：`/usr/local/bin/beetle`
- 兼容命令：`/usr/bin/beetle`（如果存在）

## 常用命令

- 看发布状态：`beetle release status`
- 请求回滚：`beetle release rollback`
- 重启托管服务：`beetle restart`
- 停止当前运行实例：`beetle stop`（优先走托管服务；若宿主未受管则直接请求活跃 `beetle run` 进程优雅退出）

## 相关文档

- Linux 首次部署：[getting-started-linux.md](getting-started-linux.md)
- 构建与部署入口：[build-script.md](build-script.md)
- 浏览器配置范围：[configuration.md](configuration.md)
- Linux 环境硬件配置：[hardware-device-config.md](hardware-device-config.md)
