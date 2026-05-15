import { LAYOUT_TOKENS } from "../config/themeTokens.ts";

export const CONFIG_PANEL_TAB_INDICATOR_SX = { display: "none" } as const;

export type ConfigPanelTabsSxOptions = {
  minTabWidth?: number;
};

export function createConfigPanelTabsSx({
  minTabWidth = LAYOUT_TOKENS.configPanelTabMinWidthPx,
}: ConfigPanelTabsSxOptions = {}) {
  return {
    width: "fit-content",
    maxWidth: "100%",
    minHeight:
      LAYOUT_TOKENS.configPanelTabHeightPx +
      LAYOUT_TOKENS.configPanelTabsInsetPx * 2,
    p: `${LAYOUT_TOKENS.configPanelTabsInsetPx}px`,
    borderRadius: "var(--radius-control)",
    border: "1px solid color-mix(in srgb, var(--border) 10%, transparent)",
    bgcolor: "color-mix(in srgb, var(--surface) 78%, var(--card))",
    backgroundImage:
      "linear-gradient(180deg, color-mix(in srgb, #fff 10%, transparent) 0%, transparent 58%)",
    boxShadow: "var(--os3d-subnav-track-recess)",
    isolation: "isolate",
    "&:hover": {
      borderColor: "color-mix(in srgb, var(--border) 18%, transparent)",
    },
    "& .MuiTabs-scroller": {
      minHeight: LAYOUT_TOKENS.configPanelTabHeightPx,
    },
    "& .MuiTabs-flexContainer": {
      alignItems: "center",
      gap: `${LAYOUT_TOKENS.configPanelTabsInsetPx}px`,
    },
    "& .MuiTab-root": {
      minHeight: LAYOUT_TOKENS.configPanelTabHeightPx,
      minWidth: minTabWidth,
      px: 1.25,
      py: 0,
      mx: 0,
      border: "1px solid transparent",
      borderRadius: "calc(var(--radius-control) - 4px)",
      color: "var(--text-secondary)",
      fontSize: "var(--font-size-body-sm)",
      fontWeight: 700,
      letterSpacing: 0,
      lineHeight: 1,
      textTransform: "none",
      transition:
        "color var(--transition-duration) ease, background-color var(--transition-duration) ease, box-shadow var(--transition-duration) var(--ease-emphasized), border-color var(--transition-duration) ease, transform var(--transition-duration) var(--ease-emphasized)",
    },
    "& .MuiTab-root:hover": {
      bgcolor: "color-mix(in srgb, var(--foreground) 3%, var(--card))",
      color: "var(--text-primary)",
    },
    "& .MuiTab-root.Mui-selected": {
      borderColor: "color-mix(in srgb, var(--primary) 18%, var(--border))",
      bgcolor: "color-mix(in srgb, var(--primary) 6%, var(--card))",
      backgroundImage:
        "linear-gradient(180deg, color-mix(in srgb, #fff 14%, transparent) 0%, color-mix(in srgb, var(--primary) 4%, transparent) 62%, transparent 100%)",
      boxShadow: "var(--os3d-control-soft-lift-stack)",
      color: "var(--primary)",
    },
    "& .MuiTab-root.Mui-selected:hover": {
      bgcolor: "color-mix(in srgb, var(--primary) 7%, var(--card))",
      color: "var(--primary)",
    },
    "@media (prefers-reduced-motion: reduce)": {
      "& .MuiTab-root": {
        transition: "none",
      },
    },
  } as const;
}
