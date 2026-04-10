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
  border: "1px solid color-mix(in srgb, var(--border) 48%, transparent)",
  boxShadow: [
    "var(--shadow-subtle)",
    "inset 0 1px 0 color-mix(in srgb, var(--foreground) 6%, transparent)",
  ].join(", "),
  overflow: "hidden",
  transition:
    "border-color var(--transition-duration) var(--ease-out-smooth), box-shadow var(--transition-duration) var(--ease-out-smooth)",
  "&:hover": {
    borderColor: "color-mix(in srgb, var(--border) 78%, transparent)",
    boxShadow: [
      "var(--shadow-card-hover)",
      "inset 0 1px 0 color-mix(in srgb, var(--foreground) 7%, transparent)",
    ].join(", "),
  },
} as const

/** 仪表盘卡片顶栏（与 Gateway 首行对齐） */
export const DASHBOARD_CARD_HEADER_ROW_SX = {
  display: "flex",
  alignItems: "center",
  justifyContent: "space-between",
  px: 2.5,
  py: 2,
  borderBottom: "1px solid color-mix(in srgb, var(--border) 14%, transparent)",
  bgcolor: "color-mix(in srgb, var(--foreground) 2%, transparent)",
} as const

/** 次级标签：降噪（相对全大写 caption），用于表盘下钻、LED 条等 */
export const UI_LABEL_SECONDARY_SX = {
  fontSize: "0.7rem",
  fontWeight: 500,
  letterSpacing: "0.02em",
  textTransform: "none" as const,
  color: "var(--foreground-soft)",
} as const
