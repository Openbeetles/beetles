import Box from "@mui/material/Box";
import Typography from "@mui/material/Typography";
import type { ReactNode } from "react";
import {
  FORM_ACTION_BAR_SX,
  TEXT_BODY_TERTIARY_SX,
} from "../../theme/panelStyles";

export interface FormActionBarProps {
  feedback?: ReactNode;
  actions: ReactNode;
}

/** Bottom action rail for durable form commits inside a form body. */
export function FormActionBar({ feedback, actions }: FormActionBarProps) {
  return (
    <Box sx={FORM_ACTION_BAR_SX}>
      <Typography component="div" sx={TEXT_BODY_TERTIARY_SX}>
        {feedback}
      </Typography>
      <Box sx={{ display: "flex", alignItems: "center", gap: 1, flexShrink: 0 }}>
        {actions}
      </Box>
    </Box>
  );
}
