import Box from "@mui/material/Box";
import type { ReactNode } from "react";

/** 表单内分组卡片：浅底无边框，用于 LLM 源、可折叠区块等。 */
export function FormCard({
  children,
  header,
  action,
}: {
  children: ReactNode;
  header?: ReactNode;
  action?: ReactNode;
}) {
  return (
    <Box
      sx={{
        p: 2,
        borderRadius: "var(--radius-control)",
        bgcolor: "color-mix(in srgb, var(--card) 88%, var(--form-group-well))",
        backgroundImage:
          "linear-gradient(180deg, color-mix(in srgb, var(--surface) 54%, transparent) 0%, transparent 100%)",
        border: "1px solid color-mix(in srgb, var(--border) 16%, transparent)",
        boxShadow:
          "0 1px 2px color-mix(in srgb, var(--foreground) 5%, transparent), inset 0 1px 0 color-mix(in srgb, var(--surface) 58%, transparent)",
      }}
    >
      {(header || action) && (
        <Box
          sx={{
            display: "flex",
            alignItems: "center",
            justifyContent: "space-between",
            mb: 1.5,
            gap: 1,
          }}
        >
          {header}
          {action}
        </Box>
      )}
      {children}
    </Box>
  );
}
