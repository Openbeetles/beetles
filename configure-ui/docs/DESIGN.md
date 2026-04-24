# Beetle OS 配置页 · 设计约束

本文档面向**参与配置页 UI 开发与样式修改的开发者**，约定视觉与布局的单源（Token 化）及必须遵守的约束，避免硬编码色值、失控阴影、脏材质混色与重边框。产品与设计说明见本目录上级 README。

## 项目定位

- **产品**：**Beetle OS**（固件侧项目名可仍为 beetle / 甲壳虫，配置页对用户的系统语义统一为 **Beetle OS**）配置前端，用于连接设备后配置 WiFi、LLM、通道（飞书/钉钉/企微/QQ/Telegram）、系统与技能等。
- **风格**：现代、精致、带克制拟物感，偏工具型与可信赖感。不是网页式扁平，也不是厚重玩具感；目标是 **干净的 3D 壳层**：材质分层清楚、阴影精确、主副光源统一。壳层采用 **桌面 OS 隐喻**：顶栏为标题栏、**底部任务栏** 承载主导航；Beetle OS 徽标（SVG）作为 **「开始」徽标** 打开开始菜单（完整列表与连接摘要），任务栏中部为固定快捷方式（图标），右侧为连接状态托盘区。详见下文「Shell 布局」。

## Token 化（单源）

**禁止在组件、theme 的 styleOverrides 内硬编码色值、圆角、动效时长、焦点环尺寸等。** 单源来自：

- **颜色 / 语义**：`src/config/themeTokens.ts` 的 `ThemeTokens`（per mode × brand），通过 `createAppTheme` 注入到 `:root` 的 `--background`、`--foreground`、`--primary`、`--border`、`--muted`、`--card`、`--surface`、`--primary-soft`、`--primary-fg`、`--accent`、`--border-subtle`、`--overlay`、`--backdrop-overlay`、`--glass-blur`、`--shell-chrome-blur`（壳层磨砂 blur，来自 `LAYOUT_TOKENS.shellChromeBackdropBlurPx`）、`--shadow-shell-start-flyout`（开始菜单：`none`）、`--transition-duration`、`--foreground-soft` 等。顶栏/任务栏 **`--shadow-shell-titlebar` / `--shadow-shell-taskbar` 为 `none`**，不靠外投阴影分层。
- **文字色阶（文案/辅助信息必读）**：`:root` 同时提供 **`--text-primary`、`--text-secondary`、`--text-tertiary`**，与 `foreground` / `foreground-soft` / `muted` 对齐。页面与组件中的**正文层级、说明、helper、次要标签**应优先使用 **`var(--text-*)`** 或 `src/theme/panelStyles.ts` 中的 **`TEXT_*_SX` 预设**（如 `TEXT_BODY_TERTIARY_SX`、`TEXT_SECTION_TITLE_SX`），避免在业务代码里散落 `fontSize`/`color`。**不要用 `var(--muted)` 充当「第三级文案」的长期口径**（该变量仍服务于 palette / 旧引用；新代码以 `--text-tertiary` 为准）。
- **布局 / 动效**：`themeTokens.ts` 的 `LAYOUT_TOKENS`（`radiusControl`、`radiusCard`、`radiusChip`、`easeEmphasized`、`easeOutSmooth`、`durationImageHoverMs`、按钮高度、padding 等），并注入 `:root` 的 `--radius-control`、`--radius-card`、`--radius-chip`、`--ease-emphasized`、`--ease-out-smooth`、`--focus-ring-width`、`--focus-ring-offset`。
- **宽度**：`src/config/layout.ts` 的 `CONTENT_MAX_WIDTH`、`SETTINGS_DRAWER_WIDTH`、`TASKBAR_HEIGHT` 等，与 theme breakpoints 一致。

组件与 theme 中一律使用 `var(--xxx)` 或从 token/常量引用，不写死 `#hex`、`12px`、`200ms` 等（除 token 定义文件本身）。

## 视觉原则

### 必须遵守

- **克制拟物**：允许明确的 3D 壳层、台座、磨砂与蜡光，但必须读起来像同一套工业材质，而不是堆效果
- **精致**：细节克制，间距与字号统一，动效引用 `var(--transition-duration)` 或 `LAYOUT_TOKENS`
- **阴影要分层，不要发糊**：允许多层阴影，但每层都要服务体积感；禁止大面积脏灰糊影
- **禁止重边框**：分割用 `var(--border)` / `var(--border-subtle)` 的细线，轮廓优先靠材质明暗与薄描边共同成立

### 推荐做法

- 颜色只用 `var(--primary)`、`var(--border)`、`var(--card)` 等；圆角用 `var(--radius-control)` / `var(--radius-card)`；动效用 `var(--transition-duration)`、`var(--ease-emphasized)` 等
- 导航与主体宽度用 `Container maxWidth="lg"`（即 `CONTENT_MAX_WIDTH`）

## 排版与表单（与「系统设置」一致）

- **页级纵向节奏**：配置页根容器优先使用 `PAGE_STACK_OUTER_SX` / `PAGE_SCROLL_STACK_SX`（`src/theme/panelStyles.ts`），**gap** 引用 **`LAYOUT_TOKENS.spacingPageStack`** 等同源常量，避免页面根上手写零散 `gap`。
- **区块内间距**：小节之间、表单项之间使用 **`LAYOUT_TOKENS`** 中的 **`spacingSectionStack`、`spacingFormFields`** 等，与 `SettingsSection` / `SettingsRow` 节奏一致。
- **Settings 行**：`SettingsRow` **始终纵向**（标签在上、控件在下）；**说明/helper 不要用窄 `maxWidth`/`ch` 人为过早换行**，保持与正文同宽或自然换行。
- **表单布局**：**禁止**为「留白」而做左右分栏拉空一栏；字段矩阵统一使用 `FormGrid`，禁止页面自定义 `fieldGridSx` / `gridTemplateColumns` 复制品。需要分组时用 `SettingsSection`、`FormSectionSub`、`FormSectionSubCollapsible`、`FormFieldStack` 等现有结构，而不是空列。
- **二元设置行**：Switch / Checkbox 这类二元设置优先使用 `FormSwitchRow`，保持标题、说明与控件在同一基线；不要直接把裸 `FormControlLabel` 散落在复杂表单里。
- **表单视觉基线**：0.1.0 表单采用 **Modern Workstation** 方向：中性工作站面板、轻量子模块、清晰字段井、低厚度阴影。避免旧式奶油色厚卡片、过大圆角、过多嵌套光泽；Glass Console 只适合状态/诊断面板，不作为全站表单基线。
- **卡片**：配置区、仪表盘等允许使用 **校准过的 3D 投影栈**（如 `--os3d-content-plate-stack`），但必须配合细描边与材质高光；不要重新发明另一套重影。

## Shell 布局（桌面隐喻）

- **顶栏 `TopBar`**：**窗口标题栏**隐喻——`SHELL_TITLEBAR_CHROME_SX` 为 **半透明哑光底 + `blur(var(--shell-chrome-blur))` + 轻微品牌侧光**；允许极浅材质渐变，但不允许脏色叠加。与主内容以 `border-subtle` 底边分隔。左侧 **Beetle OS 窗口图标**（点击回首页）、中间为 **轻量品牌标签 + 当前页标题胶囊**、右侧为 **标题栏按钮区**；页标题本身保持高可读性，不用展示字体抢层级。**macOS Tauri overlay 窗口例外**：顶栏仅保留透明拖拽安全区，APP 内不再重复渲染白色标题面板或首页图标，界面里只留系统红绿灯。
- **底部任务栏 `Taskbar`**：高度 `TASKBAR_HEIGHT`（60px），`SHELL_TASKBAR_CHROME_SX` 同为 **磨砂哑光台面**，允许浅品牌辉光与底部承托影；目标是“有体积，但不脏”。左侧 **开始** 打开菜单；**开始菜单为磁贴布局**：**品牌条**内左侧为 **Beetle OS** 徽标 + 应用名，**右侧**为 **重启**（仅已连接时显示：**仅 3D 图标**、无正文，`OS_ICON_SHELL.power`，`Tooltip` 与 `aria-label` 承载文案，确认对话框在 `Taskbar`）；其下 **宽幅连接磁贴**；再下 **响应式磁贴网格**，各路由为 **竖向 3D 磁贴**（图标上、标题下，材质高光与台座统一）；**`sm`+** 中部任务栏快捷方式；**`xs`** 仅开始 + 托盘。
- **导航数据单源**：`src/config/navItems.tsx` 的 `NAV_ITEMS`，任务栏快捷方式与开始菜单共用，避免分叉。

## 组件与布局

- **顶栏**：与主体同宽逻辑一致，样式遵循壳层 token；允许轻台座与眉题胶囊，但不能再另起材质体系
- **卡片 / 列表**：优先用主题提供的 Card、Paper 等组件样式，不额外加第二套重影或粗边框
- **按钮 / 输入框**：使用主题已定制的 MUI 组件，保持明确 3D 触感，但 hover / active 位移必须克制

### 列表：静态行 vs 可点击导航

- **静态多行列表**（仅展示、非导航）：`src/theme/listItemStyles.ts` 的 **`SETTINGS_SECTION_LIST_ROW_SX` / `SETTINGS_SECTION_LIST_EMPTY_SX`**（如 Tools / Skills 区块内列表），浅底、圆角与输入 well 一致。
- **可点击侧栏 / 子导航行**：使用 **`ListItemButton`**，**选中 / hover / 无 ripple** 由 **`appTheme` 的 `MuiListItemButton`** 统一，调用方（如 `ConfigSubNavLayout`）**只写布局类 `sx`**（`py` / `px` / `width` / `whiteSpace` 等），**禁止**在页面重复粘贴 `&.Mui-selected` 色块与圆角。
- **窄屏子导航**：横向子导航的 `ListItem` 必须按内容宽度排布，禁止继承默认 `width: 100%` 把每个分区撑成一屏；当前路由与可见高亮必须同步。

### Toast（Snackbar）

- **单一路径**：全局反馈使用 **`ToastProvider` + `useToast`**（基于 MUI **`Snackbar`**）。
- **职责划分**：**结构样式**（圆角、字重、字号、无边框阴影、`1px solid` 边框骨架）在 **`MuiSnackbarContent`** 的 `styleOverrides`；**成功 / 警告 / 错误** 的底/边/字色仅在 **`ToastProvider`** 中按 variant 注入，避免在业务里复制一整段 Snackbar 样式。

### Slider

- **轨道 / 滑块 / 圆角** 等在 **`MuiSlider`** 的 `styleOverrides` 中统一；业务页面 **禁止** 再写一套 `& .MuiSlider-thumb/track/rail` 重复主题。
- **页面内仅保留布局型 `sx`**（例如 **`maxWidth`、`mt`** 等与表单排版相关的约束）。

### 拟物 3D 图标（导航与仪表盘）

- **路径单源**：`src/config/osIcons.ts`（`OS_ICON_NAV`、`OS_ICON_DASHBOARD`、`OS_ICON_SHELL`）；展示统一用 **`Os3dIcon`**（与任务栏 `OsIcon` 同滤镜，父级给足尺寸）。
- **页内尺寸口径**：工具管理页列表图标尺寸与图标槽位使用 `LAYOUT_TOKENS.toolsListIconPx` / `LAYOUT_TOKENS.toolsListIconSlotPx`，不要在页面里重复写死 `24px`、`40px` 一类 magic number。
- **新增或替换资源**：优先扩展当前 **Industrial OS3D** 生成管线（`scripts/generate_industrial_os3d_icons.py` + `scripts/industrial_os3d/icon_groups/*`），保持统一 palette、primitive 与对象优先的构图口径；若参考第三方资源，也只能作为语义参考，不应直接回贴到 shipped `public/icons/`。更新后用 `--audit-dir` 生成总览板与几何指标，检查主体饱满度、dock 小尺寸识别和是否出现线稿化。

## 反馈分层与语义色

- **语义色单源**：状态表达只使用 `--semantic-success`、`--semantic-warning`、`--semantic-danger`，禁止继续使用评分色表达成功/错误。
- **分层规则**：
  - 配置保存（LLM/通道/系统等带标题行「保存」的区块）仅用页内 `SaveFeedback`，经 `SettingsSection` 的 `belowTitleRow` 放在标题行下方全宽（标题行仍为单行 flex），`SaveFeedback placement="belowTitle"`，禁止重复 Toast。
  - 设备连接/配对前置条件使用 `DeviceBanner` + 侧栏禁用提示；点击禁用导航可用 warning Toast。
  - 全局生命周期事件（如重启完成/超时）与无锚点操作（如技能导入/删除）使用 Toast。
  - 页面加载失败使用 `InlineAlert` + 重试，不用 Toast 抢焦点；`InlineAlert` 必须把 `Failed to fetch` 这类底层 transport 文案归一化为产品语义，并使用轻奶玻璃状态条表达语义色，禁止左侧粗强调线和浏览器式整条红色错误横幅。
  - 页面主体状态必须互斥：`loading`、阻塞性 `error`、`empty` / `unsupported` / `connect-first`、正式内容四者只能出现一种；只有在“已有旧数据、刷新失败”时，才允许在正式内容上方叠一条 `InlineAlert`。
  - `PanelStateBlock`、`DeviceBanner`、面板内 warning / danger notice 统一使用柔和毛玻璃语义面，不再用左侧竖向强调条。
  - 局部操作失败（如 WiFi 扫描）优先在操作区就地展示错误并提供重试。
- **可访问性**：错误 Toast 使用 `role=\"alert\"` + `aria-live=\"assertive\"`；成功/警告使用 `aria-live=\"polite\"`。

## 主题品牌（ThemeBrand）

- **单源**：`src/config/themeTokens.ts` 的 `ThemeBrand`、`THEME_BRAND_KEYS`、`tokenMap`；默认偏好见 `appPreferencesContext.ts`。
- **`logo`（默认）**：与 `public/logo.png` 对齐，但背景收敛为更干净的控制台中性色。浅色模式主色 `#5f47d4`、强调 `#1398b6`；深色模式主色 `#8c79f3`、强调 `#29c7de`，避免整页泛紫发脏。
- **已移除**：原「爱马仕橙」`orange` 品牌；本地若仍存 `themeBrand: "orange"` 的偏好会被视为无效并回退到默认 `logo`。

## 设计约束清单（写 UI/样式时必须遵守）

以下为 `.cursor/rules/design-constraints.mdc` 的完整内容来源，AI 与开发写样式时以此为准：

- **Token 化**：禁止在组件和 theme 的 styleOverrides 中硬编码色值、圆角、动效时长、焦点环等。颜色用 `var(--primary)`、`var(--border)`、`var(--card)`、`var(--foreground)` 等；**文案层级优先 `var(--text-primary)` / `var(--text-secondary)` / `var(--text-tertiary)` 或 `TEXT_*_SX`**。圆角用 `var(--radius-control)`、`var(--radius-card)`、`var(--radius-chip)`；动效用 `var(--transition-duration)`、`var(--ease-emphasized)`；焦点环用 `var(--focus-ring-width)`、`var(--focus-ring-offset)`。单源为 `src/config/themeTokens.ts`（ThemeTokens + LAYOUT_TOKENS）、`appTheme` 注入的 `:root` 变量，以及 **`panelStyles` / `listItemStyles` 中的版面与列表预设**。
- **风格**：现代、精致、克制拟物；禁止把 3D 做成混乱的效果堆叠。
- **阴影**：允许多层阴影栈，但必须收敛、可解释；禁止大面积、高模糊、脏灰重影。
- **边框**：分割用 `var(--border)` 或 `var(--border-subtle)` 的细线，不写死色值；表单/面板描边优先 `var(--form-outline-rest)`。
- **布局**：内容宽度用 `CONTENT_MAX_WIDTH` / `maxWidth="lg"`，不写死 1200 等数字；页级/滚动区用 **`PAGE_STACK_OUTER_SX` / `PAGE_SCROLL_STACK_SX`**，纵向间距用 **`LAYOUT_TOKENS`**。
- **表单与说明**：`SettingsRow` 纵向；**禁止**左右分栏拉空；helper **不要**用窄 `maxWidth` 过早断行。
- **列表与导航**：静态列表行用 **`listItemStyles`**；侧栏/子导航 **`ListItemButton`** 交互样式只在 **`MuiListItemButton`**，页面不写重复 selected/hover。
- **Toast**：只用 **`useToast`**；外观分层遵守上文「Toast（Snackbar）」；业务代码不复制 Snackbar 全套样式。
- **Slider**：样式只在主题 **`MuiSlider`**；页面只保留布局型 `sx`。
- **组件**：优先用主题已定制的 MUI 组件，不额外加第二套重影或粗边框。
- **新增 token**：在 themeTokens 中定义，并在 appTheme 的 `:root` 中注入对应 CSS 变量。

## 变更与扩展

- 新增页面或组件前请对照本文档（含上节约束清单），确保不引入重阴影、重边框与硬编码色值/尺寸/动效。
- 新增颜色或尺寸 token 时：颜色放入 `ThemeTokens`（按 mode/brand），布局/动效放入 `LAYOUT_TOKENS`，并在 `appTheme` 的 `:root` 中注入对应 CSS 变量。
- 状态管理与请求入口评审基线：`docs/STATE_MANAGEMENT.md`（状态分层、统一 API 入口、统一保存反馈）。
