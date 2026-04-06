# Linux 版 Agent OS 与发布包说明

[English](../en-us/linux-release-rollback.md) | **中文** | [文档索引](../README.md)

这页说明 Linux 版 Beetle Agent OS 的安装、打包和回滚。

适用场景：

- 直接在 Linux 上运行 Beetle 的人
- 直接处理 Linux 发布包的人
- 需要手工安装或回滚的运维人员

## 当前状态

- Linux 版已经具备独立运行主链路与服务化入口
- 它的正确定位是“低端 Linux SBC 上的常驻 Agent 服务”，不是桌面软件，也不是单纯的板子程序
- Linux 打包路径、服务模板和回滚目录已经存在
- 但它还不能被描述为“完全通用的 Linux 产品形态”：当前重点仍是嵌入式板端服务闭环，而不是泛 Linux 发行物生态

## 手工安装

发布包里通常会带：

- `README.txt`
- `beetle.service` 示例

常见目录结构：

- `/opt/beetle/releases/<version>/`
- 用 `current` 符号链接指向当前运行版本
- 状态目录使用 `BEETLE_STATE_ROOT` 或程序默认路径

真正安装时，以发布包内的 `README.txt` 为准。
当前 `./build.sh --deploy-linux` 会在目标机上维护：

- `/opt/beetle/releases/<release>/`
- `/opt/beetle/current` 指向当前版本
- `/opt/beetle/beetle` 作为当前二进制的兼容快捷路径
- `/var/lib/beetle` 作为默认状态目录

### 服务入口与一个容易踩的坑

当前 Linux CLI 的服务主入口是：

- `beetle run`

也就是说：

- 手工前台启动：`beetle run`
- `systemd` unit 的 `ExecStart` 也必须最终指向 `.../beetle run`

这点非常重要，因为设备上若残留旧版 unit（例如 `ExecStart=/opt/beetle/current/beetle`），服务会直接打印 CLI 帮助并退出，看起来像“二进制坏了”，本质上其实是 **service 模板与 CLI 契约漂移**。

### 关于 `smart update`

当前部署脚本的三种模式语义应按下面理解：

- Quick deploy：只换二进制，不碰服务
- Smart update：默认替换二进制，并在合适时重启现有服务；**不会主动刷新旧 unit 模板**
- Full deploy：刷新二进制、service/init 模板和相关安装文件

因此：

- 如果这次升级涉及服务入口、unit 内容、环境文件格式等契约变化，不要只做 `smart update`
- 这类升级应使用 **Full deploy**，或手工刷新 `/etc/systemd/system/beetle.service`

## CI 和回滚

- Release 会附带校验和与构建出处信息
- 目标机器上的安装与回滚由你自己负责
- CI 不会模拟机器上的实际回滚过程

当前 Linux 侧真正需要回滚的不是“一个二进制文件”，而是一个运行单元：

- 二进制
- service/unit 模板
- 状态目录契约
- 当前软链接指向

只有把这些一起看待，Linux 版 Beetle 才算是“一等服务进程”的回滚，而不是“板子上换了个程序文件”。
