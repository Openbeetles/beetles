import Box from "@mui/material/Box";
import Stack from "@mui/material/Stack";
import Typography from "@mui/material/Typography";
import type { PropsWithChildren, ReactNode } from "react";
import { CONFIG_PANEL_SX } from "../theme/panelStyles";

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
}

export function SettingsSection({
  icon,
  label,
  description,
  accessory,
  belowTitleRow,
  children,
}: PropsWithChildren<SettingsSectionProps>) {
  const titleRowMb = belowTitleRow ? 1 : description ? 1 : 2;
  const belowRowMb = description ? 1 : 2;

  return (
    <Box
      sx={{
        ...CONFIG_PANEL_SX,
        p: 2.5,
        transition: "border-color var(--transition-duration) ease",
        "&:hover": {
          borderColor: "color-mix(in srgb, var(--border) 32%, transparent)",
        },
      }}
    >
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
              color: "color-mix(in srgb, var(--primary) 55%, var(--muted))",
              display: "flex",
              alignItems: "center",
            }}
          >
            {icon}
          </Box>
          <Typography
            component="span"
            sx={{
              fontSize: "var(--font-size-body-sm)",
              fontWeight: 700,
              letterSpacing: "var(--letter-spacing-label)",
              lineHeight: "var(--line-height-tight)",
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
      {children}
    </Box>
  );
}
