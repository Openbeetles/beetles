/**
 * SettingsSection 内静态多行列表：浅底无边框（Tools / Skills 等共用）。
 * 可点击侧栏/子导航行由 `appTheme` 的 `MuiListItemButton` 统一（如 ConfigSubNavLayout）。
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

export const SETTINGS_SECTION_LIST_EMPTY_SX = {
  py: 2,
  px: 2.5,
  bgcolor: "var(--input-idle-well)",
  border: "none",
  borderRadius: "var(--radius-control)",
  boxSizing: "border-box" as const,
} as const;
