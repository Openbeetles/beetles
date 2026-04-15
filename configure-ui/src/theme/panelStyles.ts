import { LAYOUT_TOKENS } from "../config/themeTokens";

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
 * 弹窗内长表单滚动区：与同为 `var(--card)` 的表单项卡片区分的一层浅坑底，避免白底贴白底。
 * Scroll region behind stacked CONFIG_PANEL blocks in dialogs (add-account, etc.).
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
  bgcolor: "var(--form-group-well)",
  borderRadius: "var(--radius-card)",
  boxShadow: "var(--os3d-micro-well-stack)",
} as const

/** 与滚动坑底分离的底栏（主按钮区），回到卡片面亮度；无顶部分割线，靠底色区分。 */
export const DIALOG_FORM_SUBMIT_BAR_SX = {
  flexShrink: 0,
  pt: 2.5,
  px: { xs: 2, sm: 2.5 },
  pb: 1,
  bgcolor: "var(--card)",
} as const

/**
 * 固件 / 设备配置类面板：与官网 OsPanel 一致，**边界靠 `--os3d-content-plate-stack` 多层光照**，无描边。
 */
export const CONFIG_PANEL_SX = {
  borderRadius: "var(--radius-card)",
  bgcolor: "var(--card)",
  border: "none",
  boxShadow: "var(--os3d-content-plate-stack)",
  isolation: "isolate",
} as const

/**
 * 设备首页仪表盘卡片：Gateway 卡与 `DashboardCard` 共用。
 */
export const DASHBOARD_CARD_SURFACE_SX = {
  bgcolor: "var(--card)",
  borderRadius: "var(--radius-card)",
  border: "none",
  boxShadow: "var(--os3d-content-plate-stack)",
  isolation: "isolate",
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

/** 卡片正文区：与顶栏左右 padding 对齐；相对标题栏略「沉」入屏坑 */
export const DASHBOARD_CARD_BODY_SX = {
  p: 2.5,
  flex: 1,
  display: "flex",
  flexDirection: "column",
  minHeight: 0,
  boxShadow: "var(--os3d-dashboard-body-recess)",
} as const

/** 仪表盘卡片顶栏（与 Gateway 首行对齐） */
export const DASHBOARD_CARD_HEADER_ROW_SX = {
  display: "flex",
  alignItems: "center",
  justifyContent: "space-between",
  px: 2.5,
  py: 2,
  bgcolor: "color-mix(in srgb, var(--foreground) 2.5%, transparent)",
  /** 与正文区分界：内阴影代替 hairline border */
  borderBottom: "none",
  boxShadow: "var(--os3d-dashboard-card-header-lip)",
} as const

/** 次级标签：降噪（相对全大写 caption），用于表盘下钻、LED 条等 */
export const UI_LABEL_SECONDARY_SX = {
  fontSize: "var(--font-size-label)",
  fontWeight: 500,
  letterSpacing: "0.02em",
  textTransform: "none" as const,
  color: "var(--text-secondary)",
} as const

// ---------- 文字层级预设（配合 `--text-*`，减少页面内硬编码字号/颜色）----------

/** 区块主标题（与 SettingsSection 标题同阶） */
export const TEXT_SECTION_TITLE_SX = {
  fontSize: "var(--font-size-h4)",
  fontWeight: 700,
  letterSpacing: "-0.02em",
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

/** 仪表盘卡片顶栏标题（略小于正文小，偏系统设置列表） */
export const TEXT_DASHBOARD_CARD_TITLE_SX = {
  fontSize: "var(--font-size-overline)",
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
