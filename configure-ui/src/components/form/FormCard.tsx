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
        borderRadius: "calc(var(--radius-card) - 6px)",
        bgcolor: "color-mix(in srgb, var(--card) 84%, var(--form-group-well))",
        backgroundImage:
          "linear-gradient(180deg, color-mix(in srgb, #fff 12%, transparent) 0%, transparent 100%)",
        border: "1px solid color-mix(in srgb, var(--border) 12%, transparent)",
        boxShadow: "var(--os3d-section-module-stack)",
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
