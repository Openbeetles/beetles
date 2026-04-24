import Box from "@mui/material/Box";
import type { SxProps, Theme } from "@mui/material/styles";
import type { ReactNode } from "react";
import {
  createFormGridSx,
  type FormGridColumns,
  type FormGridGap,
} from "./formLayout";

export interface FormGridProps {
  children: ReactNode;
  columns?: FormGridColumns;
  gap?: FormGridGap;
  sx?: SxProps<Theme>;
}

/** Responsive field grid used by config forms. */
export function FormGrid({
  children,
  columns = 2,
  gap = "standard",
  sx,
}: FormGridProps) {
  return (
    <Box
      sx={[
        createFormGridSx({ columns, gap }),
        ...(Array.isArray(sx) ? sx : sx ? [sx] : []),
      ]}
    >
      {children}
    </Box>
  );
}
