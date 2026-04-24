# 3D 图标资源说明

## 当前 shipped 资源

当前 `configure-ui/public/icons/*.png` 的 shipped 集合为 **Beetle 自研 Industrial OS3D** 位图资产，不再直接出自第三方图标库。

- **输出格式**：`512x512` RGBA PNG，透明底
- **生成质量**：内部以 `1024x1024` supersample 场景渲染后下采样导出，优先保证小尺寸边缘干净、放大时不易发糊或出现明显锯齿
- **当前规模**：`77` 个 `*_3d.png`
- **目标风格**：对象本体优先、克制高光、稳定伪 3D 深度；任务栏、开始菜单、仪表盘与工具列表共用同一资产，但 PNG 内部不再自带二级 UI 容器

## 生成单源

当前资源的单一生成入口是：

```bash
python3 configure-ui/scripts/generate_industrial_os3d_icons.py
```

图标分组定义位于：

- `configure-ui/scripts/industrial_os3d/icon_groups/nav_shell.py`
- `configure-ui/scripts/industrial_os3d/icon_groups/dashboard_diag.py`
- `configure-ui/scripts/industrial_os3d/icon_groups/device_runtime.py`
- `configure-ui/scripts/industrial_os3d/icon_groups/tools_general.py`
- `configure-ui/scripts/industrial_os3d/icon_groups/tools_special.py`

共享底盘、调色与基础绘制原语位于：

- `configure-ui/scripts/industrial_os3d/icon_groups/common.py`

常用命令：

```bash
# 校验 77 个图标定义是否闭合、输出是否为 512x512
python3 configure-ui/scripts/generate_industrial_os3d_icons.py --check

# 重渲染全部 shipped 图标
python3 configure-ui/scripts/generate_industrial_os3d_icons.py

# 仅重渲染某个图标分组
python3 configure-ui/scripts/generate_industrial_os3d_icons.py --group tools_general

# 仅查看分组归属
python3 configure-ui/scripts/generate_industrial_os3d_icons.py --list

# 输出逐图审查总览与几何指标
python3 configure-ui/scripts/generate_industrial_os3d_icons.py --audit-dir /tmp/beetle-icon-audit
```

## 路径与消费口径

路径与语义仍只在 `src/config/osIcons.ts` 维护：

- `OS_ICON_NAV`
- `OS_ICON_DEVICE_CONFIG`
- `OS_ICON_DASHBOARD`
- `OS_ICON_SHELL`
- `OS_ICON_DIALOG`
- `OS_ICON_PREFERENCES`
- `OS_ICON_TOOL`

展示统一使用 `src/components/Os3dIcon.tsx`，由现有滤镜语言和尺寸容器消费，不单独在业务页面硬编码图标样式。

## 历史参考

仓库仍保留 `configure-ui/scripts/fetch_fluent_3d_icons.sh`，但它现在是 **历史参考 / 迁移前素材脚本**，不再代表当前 shipped 资源来源。

保留它的原因：

- 作为旧版 Fluent 资产映射的历史记录
- 在需要对照旧隐喻时，可回看原先文件名到第三方资源的对应关系

如果未来再次改版图标，不应重新把 Fluent 资源直接回贴到 `public/icons/`，而应沿用或扩展当前 Industrial OS3D 生成管线。

## 风格约束

- 同一界面内保持一套 Industrial OS3D 光影，不与扁平图标或其它 3D 渲染风格混排
- 优先保持字面隐喻可读，再追求个性化造型
- 高频入口图标应优先保证主体饱满、轮廓稳定、dock 小尺寸可识别；明显窄长或线稿化的图标需要单独做对象几何修正
- 如果消费语义与现有文件名不一致，新增语义命名资产，例如 `system_logs_3d.png`，不要把错名资源重画成另一种含义
- 新增图标时优先复用现有 palette / primitive，而不是另起一套表现体系

## Ubuntu / GNOME 说明

Yaru / Adwaita 以扁平 SVG 为主，与当前 Beetle Industrial OS3D 壳层不一致；系统级隐喻若需扩展，应继续沿用本仓库自研同管线资源。
