import FormControlLabel from "@mui/material/FormControlLabel";
import Stack from "@mui/material/Stack";
import Typography from "@mui/material/Typography";
import type { ReactElement, ReactNode } from "react";
import {
  FORM_SWITCH_ROW_SX,
  TEXT_BODY_TERTIARY_SX,
} from "../../theme/panelStyles";

export interface FormSwitchRowProps {
  title: ReactNode;
  description?: ReactNode;
  control: ReactElement;
  divider?: boolean;
}

/** Standard binary setting row with a compact leading control and adjacent label. */
export function FormSwitchRow({
  title,
  description,
  control,
  divider = true,
}: FormSwitchRowProps) {
  return (
    <FormControlLabel
      control={control}
      label={
        description ? (
          <Stack spacing={0.35} sx={{ minWidth: 0 }}>
            <Typography
              component="span"
              sx={{
                fontSize: "var(--font-size-body)",
                fontWeight: 400,
                lineHeight: "var(--line-height-relaxed)",
                color: "var(--text-primary)",
              }}
            >
              {title}
            </Typography>
            <Typography component="span" sx={TEXT_BODY_TERTIARY_SX}>
              {description}
            </Typography>
          </Stack>
        ) : (
          title
        )
      }
      sx={{
        ...FORM_SWITCH_ROW_SX,
        borderBottom: divider ? "var(--divider-row)" : "none",
      }}
    />
  );
}
