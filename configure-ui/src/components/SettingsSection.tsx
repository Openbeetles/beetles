import Box from "@mui/material/Box";
import Stack from "@mui/material/Stack";
import Typography from "@mui/material/Typography";
import type { SxProps, Theme } from "@mui/material/styles";
import type { PropsWithChildren, ReactNode } from "react";
import { CONFIG_PANEL_SX, PANEL_SECTION_PADDING } from "../theme/panelStyles";

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
}

export function SettingsSection({
  icon,
  label,
  description,
  accessory,
  belowTitleRow,
  pinHeader = false,
  sx: sxProp,
  children,
}: PropsWithChildren<SettingsSectionProps>) {
  const titleRowMb = belowTitleRow ? 1 : description ? 1 : 2;
  const belowRowMb = description ? 1 : 2;

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
              width: "var(--icon-container-sm)",
              height: "var(--icon-container-sm)",
              borderRadius: "var(--radius-control)",
              display: "flex",
              alignItems: "center",
              justifyContent: "center",
              flexShrink: 0,
              color: "color-mix(in srgb, var(--primary) 55%, var(--muted))",
              bgcolor: "color-mix(in srgb, var(--primary) 5%, transparent)",
              transition:
                "background-color var(--transition-duration) ease, color var(--transition-duration) ease",
            }}
          >
            {icon}
          </Box>
          <Typography
            component="span"
            sx={{
              fontSize: "var(--font-size-h4)",
              fontWeight: 700,
              letterSpacing: "-0.02em",
              lineHeight: "var(--line-height-snug)",
              color: "var(--foreground)",
            }}
          >
            {label}
          </Typography>
        </Stack>
        {accessory}
      </Stack>
      {belowTitleRow ? (
        <Box sx={{ mb: belowRowMb }}>{belowTitleRow}</Box>
      ) : null}
      {description && (
        <Typography
          variant="body2"
          sx={{
            color: "var(--muted)",
            mb: 2,
            fontSize: "var(--font-size-caption)",
            lineHeight: "var(--line-height-normal)",
            maxWidth: "52ch",
          }}
        >
          {description}
        </Typography>
      )}
    </>
  );

  if (pinHeader) {
    return (
      <Box
        sx={{
          ...CONFIG_PANEL_SX,
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
        ...CONFIG_PANEL_SX,
        p: PANEL_SECTION_PADDING,
        ...sxProp,
      }}
    >
      {headerBlock}
      {children}
    </Box>
  );
}
