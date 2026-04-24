import FormControlLabel from "@mui/material/FormControlLabel";
import Stack from "@mui/material/Stack";
import Typography from "@mui/material/Typography";
import type { ReactElement, ReactNode } from "react";
import {
  FORM_SWITCH_ROW_SX,
  TEXT_BODY_TERTIARY_SX,
  TEXT_FIELD_LABEL_SX,
} from "../../theme/panelStyles";

export interface FormSwitchRowProps {
  title: ReactNode;
  description?: ReactNode;
  control: ReactElement;
  divider?: boolean;
}

/** Standard binary setting row with label, helper text, and a right-aligned control. */
export function FormSwitchRow({
  title,
  description,
  control,
  divider = true,
}: FormSwitchRowProps) {
  return (
    <FormControlLabel
      labelPlacement="start"
      control={control}
      label={
        <Stack spacing={0.35} sx={{ minWidth: 0 }}>
          <Typography component="span" sx={TEXT_FIELD_LABEL_SX}>
            {title}
          </Typography>
          {description ? (
            <Typography component="span" sx={TEXT_BODY_TERTIARY_SX}>
              {description}
            </Typography>
          ) : null}
        </Stack>
      }
      sx={{
        ...FORM_SWITCH_ROW_SX,
        borderBottom: divider ? "var(--divider-row)" : "none",
      }}
    />
  );
}
