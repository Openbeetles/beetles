/**
 * SettingsSection 内多行列表：浅底无边框（Tools / Skills 等共用）。
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
