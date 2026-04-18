import Box from "@mui/material/Box";
import Stack from "@mui/material/Stack";
import Typography from "@mui/material/Typography";
import type { SxProps, Theme } from "@mui/material/styles";
import type { PropsWithChildren, ReactNode } from "react";
import {
  CONFIG_PANEL_SX,
  CONFIG_PANEL_LOADING_SX,
  PANEL_SECTION_PADDING,
  TEXT_BODY_TERTIARY_SX,
  TEXT_SECTION_TITLE_SX,
} from "../theme/panelStyles";

type SettingsSectionSurfaceTone = "default" | "loading";

interface SettingsSectionProps {
  icon: ReactNode;
  label: string;
  /** 区块下方、内容区上方的简短说明 */
  description?: string;
  accessory?: ReactNode;
  /**
   * 标题行（图标+标题+accessory）下方的全宽区域，例如保存结果。
   * Keeps the title row a single-line flex; avoids a tall right column next to the title.
   */
  belowTitleRow?: ReactNode;
  /**
   * 为 true 时：标题、说明、保存等留在卡片顶缘固定，仅 children 在卡片内滚动。
   * When true, header stays fixed; only the form body scrolls inside the card.
   */
  pinHeader?: boolean;
  /** 合并到最外层卡片容器（便于与 `PAGE_COLUMN_FILL_SX` 等组合） */
  sx?: SxProps<Theme>;
  /** loading 刷新态不应复用正式卡片阴影。 */
  surfaceTone?: SettingsSectionSurfaceTone;
}

export function SettingsSection({
  icon,
  label,
  description,
  accessory,
  belowTitleRow,
  pinHeader = false,
  sx: sxProp,
  surfaceTone = "default",
  children,
}: PropsWithChildren<SettingsSectionProps>) {
  const titleRowMb = belowTitleRow ? 1 : description ? 1 : 2;
  const belowRowMb = description ? 1 : 2;
  const loadingSurface = surfaceTone === "loading";
  const surfaceSx =
    surfaceTone === "loading" ? CONFIG_PANEL_LOADING_SX : CONFIG_PANEL_SX;

  const headerBlock = (
    <>
      <Stack
        direction="row"
        alignItems="center"
        justifyContent="space-between"
        flexWrap="wrap"
        gap={1.5}
        sx={{ mb: titleRowMb }}
      >
        <Stack direction="row" alignItems="center" spacing={1.5}>
          <Box
            sx={{
              width: "var(--icon-size-lg)",
              height: "var(--icon-size-lg)",
              display: "flex",
              alignItems: "center",
              justifyContent: "center",
              flexShrink: 0,
              borderRadius: "var(--radius-chip)",
              backgroundColor: loadingSurface
                ? "transparent"
                : "color-mix(in srgb, var(--foreground) 4%, transparent)",
              boxShadow: loadingSurface
                ? "none"
                : "var(--os3d-pedestal-lift-stack)",
            }}
          >
            {icon}
          </Box>
          <Typography component="span" sx={TEXT_SECTION_TITLE_SX}>
            {label}
          </Typography>
        </Stack>
        {accessory}
      </Stack>
      {belowTitleRow ? (
        <Box sx={{ mb: belowRowMb }}>{belowTitleRow}</Box>
      ) : null}
      {description && (
        <Typography variant="body2" sx={{ ...TEXT_BODY_TERTIARY_SX, mb: 2 }}>
          {description}
        </Typography>
      )}
    </>
  );

  if (pinHeader) {
    return (
      <Box
        sx={{
          ...surfaceSx,
          p: PANEL_SECTION_PADDING,
          display: "flex",
          flexDirection: "column",
          flex: 1,
          minHeight: 0,
          overflow: "hidden",
          ...sxProp,
        }}
      >
        <Box sx={{ flexShrink: 0 }}>{headerBlock}</Box>
        <Box
          data-app-scroll-region
          sx={{
            flex: 1,
            minHeight: 0,
            width: "100%",
            boxSizing: "border-box",
            overflow: "auto",
            WebkitOverflowScrolling: "touch",
          }}
        >
          {children}
        </Box>
      </Box>
    );
  }

  return (
    <Box
      sx={{
        ...surfaceSx,
        p: PANEL_SECTION_PADDING,
        ...sxProp,
      }}
    >
      {headerBlock}
      {children}
    </Box>
  );
}
