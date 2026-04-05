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
      <Box sx={{ mb: 1.5 }}>
        <SectionSubTitleRow title={title} />
      </Box>
      <Box sx={{ display: "flex", flexDirection: "column", gap: 2 }}>
        {children}
      </Box>
    </Box>
  );
}
