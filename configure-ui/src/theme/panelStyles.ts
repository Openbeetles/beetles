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
 * 沉浸式等大编辑对话框底栏：sm 略宽于主 gutter，避免双按钮+提示贴边。
 * Wide dialog footer horizontal padding (immersive editor, etc.).
 */
export const DIALOG_FOOTER_GUTTER_WIDE_SX = {
  px: { xs: 2, sm: 4 },
} as const

/**
 * 固件 / 设备配置类面板：无边框，仅靠底色与轻阴影与主表面区分。
 * SettingsSection、个性配置等共用。
 */
export const CONFIG_PANEL_SX = {
  borderRadius: "var(--radius-card)",
  bgcolor: "var(--card)",
  border: "none",
  /** 极轻顶边 + 内顶高光 */
  boxShadow: [
    "var(--shadow-subtle)",
    "inset 0 1px 0 color-mix(in srgb, var(--foreground) 5%, transparent)",
  ].join(", "),
} as const

/**
 * 设备首页仪表盘卡片：Gateway 卡与 `DashboardCard` 共用（无 hover 态）。
 */
export const DASHBOARD_CARD_SURFACE_SX = {
  bgcolor: "var(--card)",
  borderRadius: "var(--radius-card)",
  border: "none",
  boxShadow: [
    "var(--shadow-subtle)",
    "inset 0 1px 0 color-mix(in srgb, var(--foreground) 6%, transparent)",
  ].join(", "),
  overflow: "hidden",
} as const

/**
 * 仪表盘卡内「浅坑」中性底（数字块、运行策略行为三栏、通道状态 pill 等）— 统一 2% 混色，避免与 2.5% 等并排发花。
 */
export const DASHBOARD_INSET_WELL_BG =
  "color-mix(in srgb, var(--foreground) 2%, transparent)" as const

/** 设备首页主网格 gap（MUI spacing，与卡片正文 padding 同阶） */
export const DASHBOARD_HOME_GRID_GAP = 2.5

/** 卡片正文区内块间距（数字栅格、故障子栅格等） */
export const DASHBOARD_BLOCK_GAP = 1.5

/** 卡片内主要区块纵向间距（运行策略：表盘区 / 行为 / 预算） */
export const DASHBOARD_SECTION_STACK_GAP = 2

/** 卡片正文区：与顶栏左右 padding 对齐 */
export const DASHBOARD_CARD_BODY_SX = {
  p: 2.5,
  flex: 1,
  display: "flex",
  flexDirection: "column",
  minHeight: 0,
} as const

/** 仪表盘卡片顶栏（与 Gateway 首行对齐） */
export const DASHBOARD_CARD_HEADER_ROW_SX = {
  display: "flex",
  alignItems: "center",
  justifyContent: "space-between",
  px: 2.5,
  py: 2,
  bgcolor: "color-mix(in srgb, var(--foreground) 2.5%, transparent)",
} as const

/** 次级标签：降噪（相对全大写 caption），用于表盘下钻、LED 条等 */
export const UI_LABEL_SECONDARY_SX = {
  fontSize: "var(--font-size-label)",
  fontWeight: 500,
  letterSpacing: "0.02em",
  textTransform: "none" as const,
  color: "var(--foreground-soft)",
} as const
