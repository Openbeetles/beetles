# 3D 图标资源说明

## 许可与来源

### 主库（当前全部使用）

- **Microsoft Fluent Emoji（3D PNG）**  
  仓库：<https://github.com/microsoft/fluentui-emoji>，**MIT License**。  
  单库内含 **上千枚** 3D 资源（`assets/<EmojiName>/3D/<name>_3d.png`），可为不同工具分配**不同**隐喻，避免「一张图贴全场」。

### 其它开源库（未混入 UI，仅作备选）

- **Google Noto Emoji**：<https://github.com/googlefonts/noto-emoji>（字体与资源多为 **Apache 2.0** / **SIL OFL 1.1**）。色块风格与 Fluent 拟物不一致，**默认不混用**；若将来需要第二套风格，再单独立项评审。
- **OpenMoji**（CC BY-SA 4.0）等偏 **扁平矢量**，与本项目 3D 壳层不搭，不建议直接贴进顶栏/工具列表。

### 批量拉取

在 `configure-ui` 目录执行：

```bash
./scripts/fetch_fluent_3d_icons.sh
```

需已安装 `curl`、可访问 GitHub raw。脚本内按行注释了 **本地文件名 → Fluent 目录名**；当前约 **70** 个 PNG（主导航、仪表盘、工具专用扩展）。

## 映射单源

路径与语义只在 **`src/config/osIcons.ts`** 维护：`OS_ICON_NAV`、`OS_ICON_DASHBOARD`、`OS_ICON_TOOL`、`OS_ICON_SHELL`。  
展示统一用 **`src/components/Os3dIcon.tsx`**（与任务栏 `OsIcon` 滤镜一致）。

## 风格约定

- **同一界面内**保持 Fluent 3D 一套光影，避免与扁平/其它 3D 渲染器混排。  
- 建议上游 **256px 量级、透明底**；若发糊，优先调父级尺寸，而非强行拉伸位图。

## 仪表盘与主导航的区分

设备首页「连接 / 通道」磁贴与任务栏图标刻意使用不同 Fluent 资源（如 **Wireless**、**Antenna bars** 与 **Speech balloon**），减少同屏重复感；映射见 `OS_ICON_DASHBOARD`。

## Ubuntu / GNOME 说明

Yaru / Adwaita 以扁平为主，与拟物 3D 不一致；系统级隐喻请继续用 **Fluent MIT** 或自研同管线资源。
