import Box from "@mui/material/Box";
import Stack from "@mui/material/Stack";
import Typography from "@mui/material/Typography";
import type { ReactNode } from "react";
import {
  TEXT_BODY_TERTIARY_SX,
  TEXT_FIELD_LABEL_SX,
} from "../../theme/panelStyles";

export interface SettingsRowProps {
  /** 主标签（控件上方） */
  label: ReactNode;
  /** 可选说明（小号、muted，标签下方） */
  description?: ReactNode;
  /** 控件（输入、Switch 等，全宽） */
  children: ReactNode;
  /** 为 true 时不画底部分割线（例如最后一行） */
  divider?: boolean;
}

/**
 * 表单行：标签与说明在上、控件在下全宽（与 MUI TextField `label` 页内一致，避免宽屏左右分栏拉空）。
 */
export function SettingsRow({
  label,
  description,
  children,
  divider = true,
}: SettingsRowProps) {
  return (
    <Stack
      direction="column"
      spacing={1}
      alignItems="stretch"
      sx={{
        py: 1.5,
        borderBottom: divider ? "var(--divider-row)" : "none",
      }}
    >
      <Box sx={{ minWidth: 0 }}>
        <Typography component="div" sx={TEXT_FIELD_LABEL_SX}>
          {label}
        </Typography>
        {description ? (
          <Typography
            component="div"
            sx={{ mt: 0.5, ...TEXT_BODY_TERTIARY_SX }}
          >
            {description}
          </Typography>
        ) : null}
      </Box>
      <Box sx={{ minWidth: 0, width: "100%" }}>{children}</Box>
    </Stack>
  );
}
