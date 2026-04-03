# Linux 版程序与发布包说明

[English](../en-us/linux-release-rollback.md) | **中文** | [文档索引](../README.md)

Linux 版 Beetle 的运行、打包和回滚方式如下。

适用场景：

- 直接在 Linux 上运行 Beetle 的人
- 直接处理 Linux 发布包的集成方
- 需要手工部署或回滚的运维人员

## 当前状态

- Linux 主程序已经能稳定运行
- 适合运行完整 Agent 工作流、做集成和部署
- Linux 打包路径已经存在

## 手工部署

发布 tarball 里通常会带：

- `README.txt`
- `beetle.service` 示例

常见手工部署目录结构：

- `/opt/beetle/releases/<version>/`
- 用 `current` 符号链接指向当前运行版本
- 状态目录使用 `BEETLE_STATE_ROOT` 或程序默认路径

真正安装时，以发布包内的 `README.txt` 为准。

## CI 和回滚

- Release 会附带校验和与构建出处信息
- 目标机器上的安装与回滚由你自己负责
- CI 不会模拟机器上的实际回滚过程
