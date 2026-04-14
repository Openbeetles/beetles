import Box from "@mui/material/Box";
import type { ReactNode } from "react";
import { SectionSubTitleRow } from "./SectionSubTitleRow";

/** 表单内子区块标题：与 FormSectionSubCollapsible 共用标题行样式。 */
export function FormSectionSub({
  title,
  children,
}: {
  title: string;
  children: ReactNode;
}) {
  return (
    <Box
      sx={{
        "&:not(:first-of-type)": { mt: 3 },
      }}
    >
      <Box
        sx={{
          position: "sticky",
          top: 0,
          zIndex: 2,
          mb: 1.5,
          py: 0.75,
          px: 0.5,
          mx: -0.5,
          bgcolor: "color-mix(in srgb, var(--card) 88%, transparent)",
          backdropFilter: "blur(var(--overlay-backdrop-blur)) saturate(1.15)",
          WebkitBackdropFilter: "blur(var(--overlay-backdrop-blur)) saturate(1.15)",
          borderBottom: "var(--divider-row)",
          "@media (prefers-reduced-motion: reduce)": {
            backdropFilter: "none",
            WebkitBackdropFilter: "none",
            bgcolor: "var(--card)",
          },
        }}
      >
        <SectionSubTitleRow title={title} />
      </Box>
      <Box sx={{ display: "flex", flexDirection: "column", gap: 2 }}>
        {children}
      </Box>
    </Box>
  );
}
