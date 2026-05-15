import Box from "@mui/material/Box";
import ToggleButton from "@mui/material/ToggleButton";
import ToggleButtonGroup from "@mui/material/ToggleButtonGroup";
import Stack from "@mui/material/Stack";
import Typography from "@mui/material/Typography";
import type { ReactNode } from "react";
import { LAYOUT_TOKENS } from "../../config/themeTokens";
import {
  FORM_SWITCH_ROW_SX,
  TEXT_BODY_TERTIARY_SX,
  TEXT_FIELD_LABEL_SX,
} from "../../theme/panelStyles";

const FORM_SEGMENTED_INSET_PX = 4;
const FORM_SEGMENTED_BUTTON_HEIGHT_PX =
  LAYOUT_TOKENS.formControlHeight - FORM_SEGMENTED_INSET_PX * 2;
const FORM_SEGMENTED_DOT_LEFT_PX = 10;
const FORM_SEGMENTED_SELECTED_PADDING_LEFT_PX =
  LAYOUT_TOKENS.toggleButtonPaddingX +
  FORM_SEGMENTED_DOT_LEFT_PX +
  LAYOUT_TOKENS.dotSizePx +
  4;

export interface FormSegmentedRowOption<T extends string> {
  value: T;
  label: ReactNode;
  disabled?: boolean;
}

export interface FormSegmentedControlProps<T extends string> {
  value: T;
  options: readonly FormSegmentedRowOption<T>[];
  onChange: (value: T) => void;
  disabled?: boolean;
  minButtonWidth?: number;
}

export interface FormSegmentedRowProps<T extends string>
  extends FormSegmentedControlProps<T> {
  title: ReactNode;
  description?: ReactNode;
  divider?: boolean;
}

export function FormSegmentedControl<T extends string>({
  value,
  options,
  onChange,
  disabled = false,
  minButtonWidth = 72,
}: FormSegmentedControlProps<T>) {
  return (
    <ToggleButtonGroup
      exclusive
      value={value}
      disabled={disabled}
      onChange={(_, nextValue: T | null) => {
        if (nextValue != null) onChange(nextValue);
      }}
      sx={{
        flex: "0 0 auto",
        width: "fit-content",
        maxWidth: "100%",
        alignItems: "stretch",
        minHeight: LAYOUT_TOKENS.formControlHeight,
        height: LAYOUT_TOKENS.formControlHeight,
        p: `${FORM_SEGMENTED_INSET_PX}px`,
        gap: `${FORM_SEGMENTED_INSET_PX}px`,
        borderRadius: "var(--radius-control)",
        border: "1px solid var(--outlined-border-rest)",
        bgcolor: "var(--input-idle-well)",
        backgroundImage:
          "linear-gradient(180deg, color-mix(in srgb, #fff 18%, transparent) 0%, color-mix(in srgb, #fff 5%, transparent) 46%, transparent 100%)",
        boxShadow: "var(--os3d-micro-well-stack)",
        backdropFilter: "blur(12px) saturate(1.18)",
        WebkitBackdropFilter: "blur(12px) saturate(1.18)",
        isolation: "isolate",
        transition:
          "border-color var(--transition-duration) ease, box-shadow var(--transition-duration) var(--ease-out-smooth), background-color var(--transition-duration) ease",
        "&:hover": {
          borderColor: "var(--outlined-border-hover)",
        },
        "&.Mui-disabled": {
          opacity: 1,
          bgcolor: "color-mix(in srgb, var(--card) 92%, var(--foreground))",
          backgroundImage: "none",
          boxShadow: "none",
          borderColor: "color-mix(in srgb, var(--border) 12%, transparent)",
        },
        "& .MuiToggleButton-root": {
          position: "relative",
          minWidth: minButtonWidth,
          minHeight: FORM_SEGMENTED_BUTTON_HEIGHT_PX,
          height: FORM_SEGMENTED_BUTTON_HEIGHT_PX,
          paddingTop: 0,
          paddingBottom: 0,
          paddingLeft: `${LAYOUT_TOKENS.toggleButtonPaddingX}px`,
          paddingRight: `${LAYOUT_TOKENS.toggleButtonPaddingX}px`,
          border: "0 !important",
          borderRadius: "calc(var(--radius-control) - 4px) !important",
          fontSize: "var(--font-size-body-sm)",
          lineHeight: 1,
          fontWeight: 700,
          letterSpacing: 0,
          color: "var(--text-secondary)",
          bgcolor: "transparent",
          backgroundImage: "none",
          boxShadow: "none",
          textTransform: "none",
          overflow: "hidden",
          transition:
            "color var(--transition-duration) ease, background-color var(--transition-duration) ease, box-shadow var(--transition-duration) var(--ease-emphasized), transform var(--transition-duration) var(--ease-emphasized), padding-left var(--transition-duration) var(--ease-emphasized)",
          "&::before": {
            content: '""',
            position: "absolute",
            left: FORM_SEGMENTED_DOT_LEFT_PX,
            top: "50%",
            width: LAYOUT_TOKENS.dotSizePx,
            height: LAYOUT_TOKENS.dotSizePx,
            borderRadius: "var(--radius-full)",
            bgcolor: "var(--primary)",
            backgroundImage:
              "radial-gradient(circle at 32% 24%, color-mix(in srgb, #fff 72%, transparent) 0 22%, transparent 44%), linear-gradient(135deg, color-mix(in srgb, var(--primary) 82%, #fff) 0%, var(--primary-deep) 100%)",
            boxShadow:
              "0 1px 2px color-mix(in srgb, var(--primary-deep) 28%, transparent), inset 0 1px 1px color-mix(in srgb, #fff 64%, transparent)",
            opacity: 0,
            transform: "translateY(-50%) scale(0.66)",
            transition:
              "opacity var(--transition-duration) ease, transform var(--transition-duration) var(--ease-emphasized)",
          },
          "&:hover": {
            color: "var(--text-primary)",
            bgcolor: "color-mix(in srgb, var(--foreground) 3%, transparent)",
          },
          "&.Mui-selected": {
            color: "var(--primary)",
            bgcolor: "color-mix(in srgb, var(--primary) 7%, var(--card))",
            backgroundImage:
              "linear-gradient(180deg, color-mix(in srgb, #fff 24%, transparent) 0%, color-mix(in srgb, var(--primary) 5%, transparent) 58%, transparent 100%)",
            boxShadow: "var(--os3d-selection-pill-stack)",
            transform: "translateY(-0.5px)",
            paddingLeft: `${FORM_SEGMENTED_SELECTED_PADDING_LEFT_PX}px`,
            zIndex: 1,
            "&::before": {
              opacity: 1,
              transform: "translateY(-50%) scale(1)",
            },
            "&:hover": {
              color: "var(--primary)",
              bgcolor: "color-mix(in srgb, var(--primary) 8%, var(--card))",
            },
          },
          "&.Mui-disabled": {
            color: "var(--text-tertiary)",
            opacity: 1,
            transform: "none",
            "&.Mui-selected": {
              color: "color-mix(in srgb, var(--text-secondary) 78%, transparent)",
              bgcolor: "color-mix(in srgb, var(--card) 82%, var(--foreground))",
              backgroundImage: "none",
              boxShadow: "none",
              "&::before": {
                opacity: 0.52,
                backgroundImage: "none",
                bgcolor: "color-mix(in srgb, var(--text-tertiary) 64%, transparent)",
              },
            },
          },
        },
      }}
    >
      {options.map((option) => (
        <ToggleButton
          key={option.value}
          value={option.value}
          disabled={option.disabled}
        >
          {option.label}
        </ToggleButton>
      ))}
    </ToggleButtonGroup>
  );
}

/** Standard mutually-exclusive setting row with a compact leading segmented control. */
export function FormSegmentedRow<T extends string>({
  title,
  description,
  divider = true,
  ...controlProps
}: FormSegmentedRowProps<T>) {
  return (
    <Box
      sx={{
        ...FORM_SWITCH_ROW_SX,
        display: "flex",
        borderBottom: divider ? "var(--divider-row)" : "none",
      }}
    >
      <FormSegmentedControl {...controlProps} />
      <Stack spacing={0.35} sx={{ minWidth: 0, flex: "1 1 auto" }}>
        <Typography component="span" sx={TEXT_FIELD_LABEL_SX}>
          {title}
        </Typography>
        {description ? (
          <Typography component="span" sx={TEXT_BODY_TERTIARY_SX}>
            {description}
          </Typography>
        ) : null}
      </Stack>
    </Box>
  );
}
