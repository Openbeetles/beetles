# Linux 发布包现状

[English](../en-us/linux-release-rollback.md) | **中文** | [文档索引](../README.md)

这篇文档不是写给普通固件用户的。

它主要面向两类人：

- 直接处理 Linux 发布包的集成方
- 需要手工部署或回滚的运维人员

## 当前状态

- Linux 打包路径已经存在
- 主要面向集成和运维
- 现在还不是一键安装体验

## 如果你要手工部署

发布 tarball 里通常会带：

- `README.txt`
- `beetle.service` 示例

常见手工部署目录结构：

- `/opt/beetle/releases/<version>/`
- 用 `current` 符号链接指向当前运行版本
- 状态目录使用 `BEETLE_STATE_ROOT` 或运行时默认路径

真正安装时，以发布包里附带的说明为准。那份说明会比仓库文档更接近你手上的实际产物。

## CI 和回滚

- Release 会附带校验和与构建出处信息
- 目标机器上的安装与回滚由你自己负责
- CI 不会模拟机器上的实际回滚过程
