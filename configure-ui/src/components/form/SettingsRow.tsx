import Box from "@mui/material/Box";
import Stack from "@mui/material/Stack";
import Typography from "@mui/material/Typography";
import type { ReactNode } from "react";

export interface SettingsRowProps {
  /** 行左侧主标签 */
  label: ReactNode;
  /** 可选说明（小号、muted） */
  description?: ReactNode;
  /** 右侧控件（输入、Switch 等） */
  children: ReactNode;
  /** 为 true 时不画底部分割线（例如最后一行） */
  divider?: boolean;
}

/**
 * 系统设置式「左标签 + 右控件」行（窄屏纵向堆叠）。
 * Settings-style row: label stack left, control right; stacks on xs.
 */
export function SettingsRow({
  label,
  description,
  children,
  divider = true,
}: SettingsRowProps) {
  return (
    <Stack
      direction={{ xs: "column", sm: "row" }}
      spacing={{ xs: 1, sm: 2 }}
      alignItems={{ xs: "stretch", sm: "flex-start" }}
      sx={{
        py: 2,
        borderBottom: divider ? "var(--divider-row)" : "none",
        gap: { sm: 3 },
      }}
    >
      <Box
        sx={{
          flex: { sm: "0 0 38%" },
          minWidth: 0,
          pt: { sm: 0.75 },
        }}
      >
        <Typography
          component="div"
          sx={{
            fontSize: "var(--font-size-body)",
            fontWeight: 600,
            color: "var(--foreground)",
            lineHeight: "var(--line-height-snug)",
          }}
        >
          {label}
        </Typography>
        {description ? (
          <Typography
            component="div"
            sx={{
              mt: 0.5,
              fontSize: "var(--font-size-caption)",
              color: "var(--muted)",
              lineHeight: "var(--line-height-normal)",
              maxWidth: "48ch",
            }}
          >
            {description}
          </Typography>
        ) : null}
      </Box>
      <Box sx={{ flex: 1, minWidth: 0 }}>{children}</Box>
    </Stack>
  );
}
