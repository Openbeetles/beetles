# Pocket Crayfish 配置页 · 设计约束

本文档面向**参与配置页 UI 开发与样式修改的开发者**，约定视觉与布局的单源（Token 化）及必须遵守的约束，避免硬编码色值、重阴影、重边框。产品与设计说明见本目录上级 README。

## 项目定位

- **产品**：随身小龙虾（Pocket Crayfish）固件的配置前端，用于连接设备后配置 WiFi、LLM、通道（飞书/钉钉/企微/QQ/Telegram）、系统与技能等。
- **风格**：现代、扁平化、精致，偏工具型与可信赖感。壳层采用 **桌面 OS 隐喻**：顶栏为标题栏、**底部任务栏** 承载主导航；甲壳虫 SVG 作为 **「开始」徽标** 打开开始菜单（完整列表与连接摘要），任务栏中部为固定快捷方式（图标），右侧为连接状态托盘区。详见下文「Shell 布局」。

## Token 化（单源）

**禁止在组件、theme 的 styleOverrides 内硬编码色值、圆角、动效时长、焦点环尺寸等。** 单源来自：

- **颜色 / 语义**：`src/config/themeTokens.ts` 的 `ThemeTokens`（per mode × brand），通过 `createAppTheme` 注入到 `:root` 的 `--background`、`--foreground`、`--primary`、`--border`、`--muted`、`--card`、`--surface`、`--primary-soft`、`--primary-fg`、`--accent`、`--border-subtle`、`--overlay`、`--backdrop-overlay`、`--glass-blur`、`--shell-chrome-blur`（壳层磨砂 blur，来自 `LAYOUT_TOKENS.shellChromeBackdropBlurPx`）、`--shadow-shell-start-flyout`（开始菜单：`none`）、`--transition-duration`、`--foreground-soft` 等。顶栏/任务栏 **`--shadow-shell-titlebar` / `--shadow-shell-taskbar` 为 `none`**，不靠外投阴影分层。
- **布局 / 动效**：`themeTokens.ts` 的 `LAYOUT_TOKENS`（`radiusControl`、`radiusCard`、`radiusChip`、`easeEmphasized`、`easeOutSmooth`、`durationImageHoverMs`、按钮高度、padding 等），并注入 `:root` 的 `--radius-control`、`--radius-card`、`--radius-chip`、`--ease-emphasized`、`--ease-out-smooth`、`--focus-ring-width`、`--focus-ring-offset`。
- **宽度**：`src/config/layout.ts` 的 `CONTENT_MAX_WIDTH`、`SETTINGS_DRAWER_WIDTH`、`TASKBAR_HEIGHT` 等，与 theme breakpoints 一致。

组件与 theme 中一律使用 `var(--xxx)` 或从 token/常量引用，不写死 `#hex`、`12px`、`200ms` 等（除 token 定义文件本身）。

## 视觉原则

### 必须遵守

- **扁平化**：界面层次通过留白、色块与轻微对比区分，不依赖立体感
- **精致**：细节克制，间距与字号统一，动效引用 `var(--transition-duration)` 或 `LAYOUT_TOKENS`
- **禁止重阴影**：若有阴影仅限极轻级别
- **禁止重边框**：分割用 `var(--border)` / `var(--border-subtle)` 的细线

### 推荐做法

- 颜色只用 `var(--primary)`、`var(--border)`、`var(--card)` 等；圆角用 `var(--radius-control)` / `var(--radius-card)`；动效用 `var(--transition-duration)`、`var(--ease-emphasized)` 等
- 导航与主体宽度用 `Container maxWidth="lg"`（即 `CONTENT_MAX_WIDTH`）

## Shell 布局（桌面隐喻）

- **顶栏 `TopBar`**：**窗口标题栏**隐喻——`SHELL_TITLEBAR_CHROME_SX` 为 **半透明哑光底 + `blur(var(--shell-chrome-blur))`**，**无背景渐变**；与主内容以 `border-subtle` 底边分隔。左侧甲壳虫 **窗口图标**（点击回首页）、中间标题与说明（窄屏隐藏说明行）、右侧 **标题栏按钮区**（带过渡）。
- **底部任务栏 `Taskbar`**：高度 `TASKBAR_HEIGHT`（60px），`SHELL_TASKBAR_CHROME_SX` 同为 **扁平哑光 + 磨砂 blur**，**无渐变**；**顶边 `border-subtle`**，无外投阴影。左侧 **开始** 打开菜单；**开始菜单为磁贴布局**：顶部品牌条；其下 **宽幅连接磁贴**（语义色扁平底）；再下 **响应式磁贴网格**（`xs` 单列、`sm`+ 12 列 mosaic），各路由为 **Metro 式竖向磁贴**（图标上、标题下，主色/强调色/中性三色轮换底 + 选中主色强调），纸面 **无边框、无外投阴影、无圆角**（磁贴与连接条同为直角），半透明 + `blur(var(--shell-chrome-blur))`。**`sm`+** 中部任务栏快捷方式；**`xs`** 仅开始 + 托盘。
- **导航数据单源**：`src/config/navItems.tsx` 的 `NAV_ITEMS`，任务栏快捷方式与开始菜单共用，避免分叉。

## 组件与布局

- **顶栏**：与主体同宽逻辑一致，样式遵循壳层 token（无重阴影、无粗边框）
- **卡片 / 列表**：优先用主题提供的 Card、Paper 等组件样式，不额外加重阴影或边框
- **按钮 / 输入框**：使用主题已定制的 MUI 组件，保持扁平、无强立体感

## 反馈分层与语义色

- **语义色单源**：状态表达只使用 `--semantic-success`、`--semantic-warning`、`--semantic-danger`，禁止继续使用评分色表达成功/错误。
- **分层规则**：
  - 配置保存（LLM/通道/系统等带标题行「保存」的区块）仅用页内 `SaveFeedback`，经 `SettingsSection` 的 `belowTitleRow` 放在标题行下方全宽（标题行仍为单行 flex），`SaveFeedback placement="belowTitle"`，禁止重复 Toast。
  - 设备连接/配对前置条件使用 `DeviceBanner` + 侧栏禁用提示；点击禁用导航可用 warning Toast。
  - 全局生命周期事件（如重启完成/超时）与无锚点操作（如技能导入/删除）使用 Toast。
  - 页面加载失败使用 `InlineAlert` + 重试，不用 Toast 抢焦点。
  - 局部操作失败（如 WiFi 扫描）优先在操作区就地展示错误并提供重试。
- **可访问性**：错误 Toast 使用 `role=\"alert\"` + `aria-live=\"assertive\"`；成功/警告使用 `aria-live=\"polite\"`。

## 主题品牌（ThemeBrand）

- **单源**：`src/config/themeTokens.ts` 的 `ThemeBrand`、`THEME_BRAND_KEYS`、`tokenMap`；默认偏好见 `appPreferencesContext.ts`。
- **`logo`（默认）**：与 `public/logo.png` 对齐——浅色模式主色深紫 `#6d28d9`、强调电青 `#0891b2`；深色模式主色 `#a78bfa`、强调 `#22d3ee`，背景带轻微紫基调。
- **已移除**：原「爱马仕橙」`orange` 品牌；本地若仍存 `themeBrand: "orange"` 的偏好会被视为无效并回退到默认 `logo`。

## 设计约束清单（写 UI/样式时必须遵守）

以下为 `.cursor/rules/design-constraints.mdc` 的完整内容来源，AI 与开发写样式时以此为准：

- **Token 化**：禁止在组件和 theme 的 styleOverrides 中硬编码色值、圆角、动效时长、焦点环等。颜色只用 `var(--primary)`、`var(--border)`、`var(--card)`、`var(--muted)`、`var(--foreground)` 等；圆角用 `var(--radius-control)`、`var(--radius-card)`、`var(--radius-chip)`；动效用 `var(--transition-duration)`、`var(--ease-emphasized)`；焦点环用 `var(--focus-ring-width)`、`var(--focus-ring-offset)`。单源为 `src/config/themeTokens.ts`（ThemeTokens + LAYOUT_TOKENS）和 `appTheme` 注入的 `:root` 变量。
- **风格**：现代、扁平化、精致；禁止重阴影、粗/重边框。
- **阴影**：仅允许极轻级别；禁止大面积、高模糊、深色强阴影。
- **边框**：分割用 `var(--border)` 或 `var(--border-subtle)` 的细线，不写死色值。
- **布局**：内容宽度用 `CONTENT_MAX_WIDTH` / `maxWidth="lg"`，不写死 1200 等数字。
- **组件**：优先用主题已定制的 MUI 组件，不额外加重阴影或边框。
- **新增 token**：在 themeTokens 中定义，并在 appTheme 的 `:root` 中注入对应 CSS 变量。

## 变更与扩展

- 新增页面或组件前请对照本文档（含上节约束清单），确保不引入重阴影、重边框与硬编码色值/尺寸/动效。
- 新增颜色或尺寸 token 时：颜色放入 `ThemeTokens`（按 mode/brand），布局/动效放入 `LAYOUT_TOKENS`，并在 `appTheme` 的 `:root` 中注入对应 CSS 变量。
- 状态管理与请求入口评审基线：`docs/STATE_MANAGEMENT.md`（状态分层、统一 API 入口、统一保存反馈）。
