import Box from "@mui/material/Box";
import type { ReactNode } from "react";

/** 表单内分组卡片：浅底 + 细边框，用于 LLM 源、可折叠区块等。 */
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
        bgcolor: "var(--form-group-well)",
        border: "1px solid var(--form-outline-rest)",
        transition: "border-color var(--transition-duration) ease",
        "&:focus-within": {
          borderColor: "color-mix(in srgb, var(--primary) 28%, var(--border))",
        },
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
