# Linux 版 Agent OS 与发布包说明

[English](../en-us/linux-release-rollback.md) | **中文** | [文档索引](../README.md)

这页说明 Linux 版 Beetle Agent OS 的安装、打包和回滚。

适用场景：

- 直接在 Linux 上运行 Beetle 的人
- 直接处理 Linux 发布包的人
- 需要手工安装或回滚的运维人员

## 当前状态

- Linux 版 Agent OS 已经能稳定运行
- 适合承载更完整的 Agent OS 能力、长任务和复杂集成
- Linux 打包路径已经存在

## 手工安装

发布包里通常会带：

- `README.txt`
- `beetle.service` 示例

常见目录结构：

- `/opt/beetle/releases/<version>/`
- 用 `current` 符号链接指向当前运行版本
- 状态目录使用 `BEETLE_STATE_ROOT` 或程序默认路径

真正安装时，以发布包内的 `README.txt` 为准。

## CI 和回滚

- Release 会附带校验和与构建出处信息
- 目标机器上的安装与回滚由你自己负责
- CI 不会模拟机器上的实际回滚过程
