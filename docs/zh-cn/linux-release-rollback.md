# Linux 部署与回滚

[English](../en-us/linux-release-rollback.md) | **中文** | [文档索引](../README.md)

这页只讲当前 Linux 路径里真实存在的部署和回滚方式。

## 部署后常见目录

当前部署脚本会维护这些路径：

- `/opt/beetle/releases/<release>/`
- `/opt/beetle/current`
- `/opt/beetle/rollback`
- `/usr/local/bin/beetle`
- `/usr/bin/beetle`（当目标 shell 默认 `PATH` 不含 `/usr/local/bin` 时作为兼容入口）
- `/var/lib/beetle`

其中：

- `current` 指向当前版本
- `rollback` 指向上一个可回退版本
- `/usr/local/bin/beetle` 是全局命令入口
- 某些嵌入式 shell 不搜 `/usr/local/bin` 时，会额外创建 `/usr/bin/beetle`

## 运行入口

Linux 服务入口现在是：

- `beetle run`

控制面（HTTP API）与 agent 运行在同一个进程内，无需再派生子进程。

如果你用 `systemd`，`ExecStart` 应该指向：

- `/opt/beetle/current/beetle run`

## 三种部署模式

`./build.sh --deploy-linux` 当前有三种模式：

1. `Quick deploy`
   只换二进制，不动服务
2. `Full deploy`
   刷新二进制和服务安装内容
3. `Smart update`
   替换二进制，并在合适的时候重启现有服务

如果这次改动涉及服务入口或服务文件，不要只用 `Smart update`。

## 回滚是怎么工作的

新版本部署后，会先进入：

- `pending_validation`

如果新版本稳定运行，会转成稳定状态。
如果在验证窗口里连续快速失败，会优先回退到 `rollback`。

## 你手工排查时最常看的地方

- 当前版本： `/opt/beetle/current`
- 回滚版本： `/opt/beetle/rollback`
- 发布状态： `/var/lib/beetle/runtime/linux_release/state.json`
- 全局命令： `/usr/local/bin/beetle`
- 兼容命令： `/usr/bin/beetle`（如果存在）

## 直接命令

- 看发布状态： `beetle release status`
- 请求回滚： `beetle release rollback`
- 重启托管服务： `beetle restart`
- 停止托管服务： `beetle stop`

## 几个直接结论

- Linux 版现在是常驻服务形态
- 真正的启动入口是 `beetle run`（单进程）
- 回滚不只是换一个文件，而是切回上一版发布目录
- 如果你要配硬件，不要参考旧式的 Linux 硬件示例口径，直接看 [hardware-device-config.md](hardware-device-config.md)
