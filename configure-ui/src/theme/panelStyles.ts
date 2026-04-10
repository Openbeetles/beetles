/**
 * 固件 / 设备配置类面板：全宽、扁平、细边框（挂载模块感）。
 * SettingsSection、个性配置等共用，保证风格一致。
 */
export const CONFIG_PANEL_SX = {
  borderRadius: "var(--radius-card)",
  bgcolor: "var(--card)",
  border: "1px solid color-mix(in srgb, var(--border) 28%, transparent)",
  /** 极轻顶边 + 内顶高光，接近系统设置面板/窗口内嵌块 */
  boxShadow: [
    "var(--shadow-subtle)",
    "inset 0 1px 0 color-mix(in srgb, var(--foreground) 5%, transparent)",
  ].join(", "),
} as const

/**
 * 设备首页仪表盘卡片：略抬描边与主表面区分；hover 统一用 `--shadow-card-hover`。
 * Gateway 卡与 `DashboardCard` 共用，避免双层 PCB 与卡片边框糊成一片。
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
  transition: "box-shadow var(--transition-duration) var(--ease-out-smooth)",
  "&:hover": {
    boxShadow: [
      "var(--shadow-card-hover)",
      "inset 0 1px 0 color-mix(in srgb, var(--foreground) 7%, transparent)",
    ].join(", "),
  },
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
  fontSize: "0.7rem",
  fontWeight: 500,
  letterSpacing: "0.02em",
  textTransform: "none" as const,
  color: "var(--foreground-soft)",
} as const
