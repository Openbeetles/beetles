import { LAYOUT_TOKENS } from "../config/themeTokens.ts";

/**
 * 主内容区内层与顶栏/横幅水平 gutter 对齐（MUI spacing：xs=16px, sm=24px）。
 * Aligns main scroll content with title bar and device banner.
 */
export const MAIN_CONTENT_INNER_SX = {
  px: { xs: 2, sm: 3 },
  maxWidth: "100%",
  boxSizing: "border-box" as const,
} as const

/** SettingsSection / CONFIG_PANEL 内边距（与仪表盘卡正文 p 同阶） */
export const PANEL_SECTION_PADDING = 2.5

/**
 * 主工作区占满壳层剩余高度（与 Layout `MainSurface` 内 flex 链配合），禁止整页滚动。
 * Fill remaining viewport under shell chrome; pair with inner `overflow: auto` scroll regions.
 */
export const PAGE_COLUMN_FILL_SX = {
  display: "flex",
  flexDirection: "column",
  alignSelf: "stretch",
  flex: "1 1 0",
  flexBasis: 0,
  minHeight: 0,
  width: "100%",
  overflow: "hidden",
} as const

/**
 * 配置页根容器：占满主列 + 页级纵向节奏（Alert / Section 间距与 `LAYOUT_TOKENS.spacingPageStack` 一致）。
 */
export const PAGE_STACK_OUTER_SX = {
  ...PAGE_COLUMN_FILL_SX,
  gap: LAYOUT_TOKENS.spacingPageStack,
} as const

/**
 * 整页内容区纵向滚动（仪表盘、列表页等无「卡片内 pinHeader」时使用）。
 */
/**
 * 整页纵向滚动区：与 Layout `MainSurface` 内层 flex 链配合。
 * `flex: 1 1 0` + `height: 100%` 避免嵌套 flex 下子项高度塌成内容高，导致「主区被压扁」。
 */
export const PAGE_SCROLL_CANVAS_SX = {
  display: "flex",
  flexDirection: "column",
  alignSelf: "stretch",
  flex: "1 1 0",
  flexBasis: 0,
  minHeight: 0,
  width: "100%",
  height: "100%",
  maxHeight: "100%",
  overflow: "auto",
  WebkitOverflowScrolling: "touch",
} as const

/**
 * 整页可滚动内容区 + 与页根相同的纵向 gap（列表/仪表盘等）。
 */
export const PAGE_SCROLL_STACK_SX = {
  ...PAGE_SCROLL_CANVAS_SX,
  gap: LAYOUT_TOKENS.spacingPageStack,
} as const

/**
 * 沉浸式等大编辑对话框底栏：sm 略宽于主 gutter，避免双按钮+提示贴边。
 * Wide dialog footer horizontal padding (immersive editor, etc.).
 */
export const DIALOG_FOOTER_GUTTER_WIDE_SX = {
  px: { xs: 2, sm: 4 },
} as const

/**
 * 弹窗内长表单滚动区：极淡透明坑底，与上层玻璃弹窗互不干扰。
 */
export const DIALOG_FORM_SCROLL_WELL_SX = {
  flex: 1,
  minHeight: 0,
  overflow: "auto" as const,
  width: "100%",
  boxSizing: "border-box" as const,
  px: { xs: 2, sm: 2.5 },
  py: 2.5,
  pb: 1.5,
  bgcolor: "color-mix(in srgb, var(--foreground) 2%, transparent)",
  borderRadius: "var(--radius-card)",
  boxShadow: "none",
} as const

/** 与滚动区分离的底栏（主按钮区）：透明继承弹窗玻璃底。 */
export const DIALOG_FORM_SUBMIT_BAR_SX = {
  flexShrink: 0,
  pt: 2.5,
  px: { xs: 2, sm: 2.5 },
  pb: 1,
  bgcolor: "color-mix(in srgb, #fff 8%, transparent)",
} as const

/**
 * 配置/设置面板：与仪表盘卡同一套毛玻璃材质，但稍高透明度（设置区背景更稳定）。
 */
export const CONFIG_PANEL_SX = {
  borderRadius: "var(--radius-card)",
  bgcolor: "var(--card-glass)",
  border: "1px solid var(--card-glass-border)",
  backgroundImage:
    "linear-gradient(135deg, color-mix(in srgb, #fff 14%, transparent) 0%, color-mix(in srgb, var(--primary) 2%, transparent) 50%, transparent 100%)",
  boxShadow: "var(--card-glass-shadow)",
  backdropFilter: "blur(32px) saturate(1.7)",
  WebkitBackdropFilter: "blur(32px) saturate(1.7)",
  isolation: "isolate",
} as const

/**
 * 页面首屏/刷新中的 loading 壳层：保留版式与间距，但去掉正式卡片阴影，
 * 避免数据未就绪时整屏先闪出一层“空壳厚卡”。
 */
export const CONFIG_PANEL_LOADING_SX = {
  borderRadius: "var(--radius-card)",
  bgcolor: "transparent",
  border: "none",
  boxShadow: "none",
  isolation: "isolate",
} as const

/**
 * 设备首页仪表盘卡片：毛玻璃质感（iOS 风格）。
 * Dashboard card surface — frosted glass: semi-transparent background + backdrop blur.
 */
export const DASHBOARD_CARD_SURFACE_SX = {
  bgcolor: "var(--card-glass)",
  borderRadius: "var(--radius-card)",
  border: "1px solid var(--card-glass-border)",
  backgroundImage:
    "linear-gradient(135deg, color-mix(in srgb, #fff 12%, transparent) 0%, color-mix(in srgb, #fff 3%, transparent) 50%, transparent 100%)",
  boxShadow: "var(--card-glass-shadow)",
  backdropFilter: "blur(40px) saturate(1.8)",
  WebkitBackdropFilter: "blur(40px) saturate(1.8)",
  isolation: "isolate",
  overflow: "hidden",
} as const

/**
 * 表单/设置区内部的二级模块：比主面板更轻的玻璃层，保留层级但不喧宾夺主。
 */
export const FORM_SECTION_MODULE_SX = {
  borderRadius: "var(--radius-control)",
  bgcolor: "color-mix(in srgb, var(--card-glass) 80%, transparent)",
  backgroundImage:
    "linear-gradient(135deg, color-mix(in srgb, #fff 10%, transparent) 0%, transparent 60%)",
  boxShadow: "var(--card-glass-shadow)",
  border: "1px solid color-mix(in srgb, var(--card-glass-border) 70%, transparent)",
  backdropFilter: "blur(20px) saturate(1.5)",
  WebkitBackdropFilter: "blur(20px) saturate(1.5)",
  overflow: "hidden",
  isolation: "isolate",
} as const

/** 二级模块头：透明继承玻璃模块底色，仅用极淡分割线分组。 */
export const FORM_SECTION_MODULE_HEADER_SX = {
  px: 2,
  py: 1.25,
  bgcolor: "color-mix(in srgb, #fff 8%, transparent)",
  borderBottom: "1px solid color-mix(in srgb, var(--border) 8%, transparent)",
  boxShadow: "none",
} as const

/** 二级模块正文：完全透明，让上层玻璃材质贯穿显示。 */
export const FORM_SECTION_MODULE_BODY_SX = {
  px: 2,
  pt: 2,
  pb: 2,
  bgcolor: "transparent",
} as const

/** 表单内底部操作轨道：提交动作与保存反馈固定在同一基线。 */
export const FORM_ACTION_BAR_SX = {
  display: "flex",
  alignItems: "center",
  justifyContent: "space-between",
  gap: 1.5,
  mt: 2,
  pt: 1.5,
  borderTop: "1px solid color-mix(in srgb, var(--border) 16%, transparent)",
} as const

/** 二元设置行：Switch/Checkbox 在前，标题紧随其后，避免设置项被拉成左右分栏。 */
export const FORM_SWITCH_ROW_SX = {
  width: "fit-content",
  maxWidth: "100%",
  m: 0,
  px: 0,
  py: 0.25,
  alignItems: "center",
  justifyContent: "flex-start",
  columnGap: 1,
  "& .MuiFormControlLabel-label": {
    flex: "0 1 auto",
    minWidth: 0,
  },
} as const

/**
 * 仪表盘卡内「浅坑」chip 底色：白色混入（非深色 foreground），
 * 毛玻璃卡面下用纯白叠加而非深色，避免灰感。
 */
export const DASHBOARD_INSET_WELL_BG =
  "color-mix(in srgb, #fff 22%, transparent)" as const

/** 设备首页主网格 gap（MUI spacing，与卡片正文 padding 同阶） */
export const DASHBOARD_HOME_GRID_GAP = 2.5

/** 卡片正文区内块间距（数字栅格、故障子栅格等） */
export const DASHBOARD_BLOCK_GAP = 1.5

/** 卡片内主要区块纵向间距（运行策略：表盘区 / 行为 / 预算） */
export const DASHBOARD_SECTION_STACK_GAP = 2

/** 卡片正文区：毛玻璃卡内无额外内陷（卡面已是透明层，再内陷反而破坏材质感） */
export const DASHBOARD_CARD_BODY_SX = {
  p: 2.5,
  flex: 1,
  display: "flex",
  flexDirection: "column",
  minHeight: 0,
  boxShadow: "none",
} as const

/** 仪表盘卡片顶栏 — 毛玻璃卡内轻高光条（无额外模糊，继承卡层） */
export const DASHBOARD_CARD_HEADER_ROW_SX = {
  display: "flex",
  alignItems: "center",
  justifyContent: "space-between",
  px: 2.5,
  py: 2,
  bgcolor: "color-mix(in srgb, #fff 6%, transparent)",
  borderBottom: "1px solid color-mix(in srgb, var(--border) 10%, transparent)",
  boxShadow: "none",
} as const

/** 次级标签：降噪（相对全大写 caption），用于表盘下钻、LED 条等 */
export const UI_LABEL_SECONDARY_SX = {
  fontSize: "var(--font-size-label)",
  fontWeight: 500,
  letterSpacing: "var(--letter-spacing-label)",
  textTransform: "none" as const,
  color: "var(--text-secondary)",
} as const

/**
 * Overline 标签：全大写 + 较宽字距 + caption 字号 + 三级文字色。
 * 用于仪表盘卡内分区标题（如「故障」「恢复」）、面板段落起始标注等。
 * 不用于主导航、正文或卡片主标题。
 */
export const TEXT_OVERLINE_SX = {
  fontSize: "var(--font-size-caption)",
  fontWeight: 600,
  letterSpacing: "var(--letter-spacing-small)",
  textTransform: "uppercase" as const,
  lineHeight: 1.35,
  color: "var(--text-tertiary)",
} as const

// ---------- 文字层级预设（配合 `--text-*`，减少页面内硬编码字号/颜色）----------

/** 区块主标题（与 SettingsSection 标题同阶） */
export const TEXT_SECTION_TITLE_SX = {
  fontSize: "var(--font-size-h4)",
  fontWeight: 700,
  letterSpacing: 0,
  lineHeight: "var(--line-height-snug)",
  color: "var(--text-primary)",
} as const

/** 小节标题 / 列表行主文 */
export const TEXT_SUBSECTION_TITLE_SX = {
  fontSize: "var(--font-size-body-sm)",
  fontWeight: 600,
  lineHeight: "var(--line-height-snug)",
  color: "var(--text-primary)",
} as const

/** 仪表盘卡片顶栏标题（caption 字号，保证在玻璃卡面上清晰可读） */
export const TEXT_DASHBOARD_CARD_TITLE_SX = {
  fontSize: "var(--font-size-caption)",
  fontWeight: 600,
  letterSpacing: "var(--letter-spacing-label)",
  lineHeight: 1.35,
  color: "var(--text-primary)",
  textTransform: "none" as const,
} as const

/** SettingsRow 等表单行主标签（非 Section 级标题） */
export const TEXT_FIELD_LABEL_SX = {
  fontSize: "var(--font-size-body)",
  fontWeight: 600,
  lineHeight: "var(--line-height-snug)",
  color: "var(--text-primary)",
} as const

/** 说明、helper、卡片顶栏描述 */
export const TEXT_BODY_TERTIARY_SX = {
  fontSize: "var(--font-size-caption)",
  fontWeight: 400,
  lineHeight: "var(--line-height-normal)",
  color: "var(--text-tertiary)",
} as const

/** 仅色阶（与其它 sx 合并用） */
export const TEXT_COLOR = {
  primary: "var(--text-primary)",
  secondary: "var(--text-secondary)",
  tertiary: "var(--text-tertiary)",
} as const

/**
 * SettingsSection 内「空数据 / 警告 / 错误」占位：居中、插画与正文权重与全屏状态卡一致。
 * Empty / warning / danger blocks inside sections (aligned with status overlay cards).
 */
export const PANEL_STATE_AREA_SX = {
  display: "flex",
  flexDirection: "column",
  alignItems: "stretch",
  justifyContent: "center",
  minHeight: { xs: 200, sm: 220 },
  width: "100%",
  boxSizing: "border-box" as const,
} as const
