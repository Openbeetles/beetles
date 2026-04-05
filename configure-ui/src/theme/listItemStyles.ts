/**
 * SettingsSection 内多行列表：与表单井、描边 token 一致（Tools / Skills 等共用）。
 */
export const SETTINGS_SECTION_LIST_ROW_SX = {
  py: 1.5,
  px: 2,
  bgcolor: "var(--input-idle-well)",
  border: "1px solid var(--form-outline-rest)",
  borderRadius: "var(--radius-control)",
  alignItems: "center",
  boxSizing: "border-box" as const,
  transition:
    "background-color var(--transition-duration) ease, border-color var(--transition-duration) ease",
  "&:focus-within": {
    backgroundColor:
      "color-mix(in srgb, var(--primary) 5%, var(--input-idle-well))",
    borderColor: "color-mix(in srgb, var(--primary) 22%, var(--border))",
  },
} as const;

export const SETTINGS_SECTION_LIST_EMPTY_SX = {
  py: 2,
  px: 2,
  bgcolor: "var(--input-idle-well)",
  border: "1px solid var(--form-outline-rest)",
  borderRadius: "var(--radius-control)",
  boxSizing: "border-box" as const,
} as const;
