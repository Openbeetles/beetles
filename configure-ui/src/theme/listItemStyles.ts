/**
 * SettingsSection 内静态多行列表：浅底无边框（Tools / Skills 等共用）。
 * 可点击侧栏/子导航行由 `createSettingsNavItemButtonSx` 统一（如 ConfigSubNavLayout）。
 */
export const SETTINGS_SECTION_LIST_ROW_SX = {
  py: 1.5,
  px: 2.5,
  bgcolor: "var(--input-idle-well)",
  border: "none",
  borderRadius: "var(--radius-control)",
  alignItems: "center",
  boxSizing: "border-box" as const,
} as const;

/**
 * 与配置列表同源：`CONFIG_PANEL` 双层高光/环境渐变 + 极淡主色角向层。
 * 供账户卡、Tools/Skills 列表行等复用，避免各处各写一套。
 */
export const LIST_CARD_PLATE_BACKGROUND_IMAGE = [
  "linear-gradient(180deg, color-mix(in srgb, #fff 18%, transparent) 0%, color-mix(in srgb, #fff 6%, transparent) 34%, transparent 76%)",
  "linear-gradient(180deg, color-mix(in srgb, var(--surface) 40%, transparent) 0%, transparent 68%)",
  "linear-gradient(125deg, color-mix(in srgb, var(--primary) 4.5%, transparent) 0%, transparent 44%)",
].join(", ");

/**
 * 列表行「薄卡」：共享渐变 + `--os3d-content-plate-stack` + 顶内高光；hover 仅加深阴影（不改底色素块）。
 */
export const SETTINGS_LIST_ROW_PLATE_SX = {
  ...SETTINGS_SECTION_LIST_ROW_SX,
  bgcolor: "var(--card)",
  backgroundImage: LIST_CARD_PLATE_BACKGROUND_IMAGE,
  boxShadow: [
    "var(--os3d-content-plate-stack)",
    "inset 0 1px 0 color-mix(in srgb, #fff 48%, transparent)",
  ].join(", "),
  isolation: "isolate",
  overflow: "hidden",
  transition: "box-shadow var(--transition-duration) var(--ease-out-smooth)",
  "@media (hover: hover)": {
    "&:hover": {
      boxShadow: [
        "var(--os3d-content-plate-stack)",
        "0 12px 32px -24px color-mix(in srgb, var(--foreground) 9%, transparent)",
        "inset 0 1px 0 color-mix(in srgb, #fff 52%, transparent)",
      ].join(", "),
    },
  },
} as const;

/** 配置式左侧菜单 / 子导航行：统一选中态、hover、边框与 OS3D 阴影。 */
export function createSettingsNavItemButtonSx(selected: boolean) {
  return {
    borderRadius: "var(--radius-control)",
    border: "1px solid transparent",
    backgroundImage: selected
      ? "linear-gradient(180deg, color-mix(in srgb, #fff 12%, transparent) 0%, color-mix(in srgb, var(--primary) 7%, transparent) 100%)"
      : "linear-gradient(180deg, color-mix(in srgb, #fff 8%, transparent) 0%, transparent 100%)",
    transition:
      "background-color var(--transition-duration) var(--ease-out-smooth), box-shadow var(--transition-duration) var(--ease-out-smooth), border-color var(--transition-duration) var(--ease-out-smooth)",
    "&:hover": {
      backgroundColor:
        "color-mix(in srgb, var(--foreground) 3.5%, var(--card))",
      borderColor:
        "color-mix(in srgb, var(--border) 18%, transparent)",
      boxShadow: "var(--os3d-control-soft-lift-stack)",
    },
    "&.Mui-selected": {
      borderColor:
        "color-mix(in srgb, var(--primary) 22%, var(--border))",
      backgroundColor:
        "color-mix(in srgb, var(--primary) 11%, var(--card))",
      boxShadow: "var(--os3d-selection-pill-stack)",
    },
    "&.Mui-selected:hover": {
      backgroundColor:
        "color-mix(in srgb, var(--primary) 13%, var(--card))",
    },
    "@media (prefers-reduced-motion: reduce)": {
      transition: "none",
    },
  } as const;
}

export const SETTINGS_SECTION_LIST_EMPTY_SX = {
  py: 2,
  px: 2.5,
  bgcolor: "var(--input-idle-well)",
  border: "none",
  borderRadius: "var(--radius-control)",
  boxSizing: "border-box" as const,
} as const;
