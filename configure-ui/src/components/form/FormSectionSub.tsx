import Box from "@mui/material/Box";
import type { ReactNode } from "react";
import { SectionSubTitleRow } from "./SectionSubTitleRow";
import {
  FORM_SECTION_MODULE_BODY_SX,
  FORM_SECTION_MODULE_HEADER_SX,
  FORM_SECTION_MODULE_SX,
} from "../../theme/panelStyles";

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
      <Box sx={FORM_SECTION_MODULE_SX}>
        <Box sx={FORM_SECTION_MODULE_HEADER_SX}>
          <SectionSubTitleRow title={title} />
        </Box>
        <Box
          sx={{
            ...FORM_SECTION_MODULE_BODY_SX,
            display: "flex",
            flexDirection: "column",
            gap: 2,
          }}
        >
          {children}
        </Box>
      </Box>
    </Box>
  );
}
