import { useState } from "react";
import Box from "@mui/material/Box";
import Collapse from "@mui/material/Collapse";
import ExpandMore from "@mui/icons-material/ExpandMore";
import ExpandLess from "@mui/icons-material/ExpandLess";
import type { ReactNode } from "react";
import { useTranslation } from "react-i18next";
import { SectionSubTitleRow } from "./SectionSubTitleRow";
import {
  FORM_SECTION_MODULE_BODY_SX,
  FORM_SECTION_MODULE_HEADER_SX,
  FORM_SECTION_MODULE_SX,
} from "../../theme/panelStyles";

interface FormSectionSubCollapsibleProps {
  title: string;
  children: ReactNode;
  defaultOpen?: boolean;
  /** 标题行右侧操作（如删除），点击不触发展开/收起 */
  action?: ReactNode;
}

export function FormSectionSubCollapsible({
  title,
  children,
  defaultOpen = true,
  action,
}: FormSectionSubCollapsibleProps) {
  const { t } = useTranslation();
  const [open, setOpen] = useState(defaultOpen);
  const headerId = `header-${title.replace(/\s/g, "-")}`;
  const collapseId = `collapse-${title.replace(/\s/g, "-")}`;

  return (
    <Box
      sx={{
        "&:not(:first-of-type)": { mt: 2.5 },
        ...FORM_SECTION_MODULE_SX,
      }}
    >
      <Box
        component="button"
        type="button"
        onClick={() => setOpen((o) => !o)}
        sx={{
          ...FORM_SECTION_MODULE_HEADER_SX,
          display: "flex",
          alignItems: "center",
          justifyContent: "space-between",
          width: "100%",
          gap: 1,
          border: 0,
          boxShadow: "var(--os3d-section-module-header-stack)",
          cursor: "pointer",
          color: "var(--foreground)",
          font: "inherit",
          textAlign: "left",
          "&:focus-visible": {
            outline:
              "var(--focus-ring-width) solid color-mix(in srgb, var(--primary) 55%, transparent)",
            outlineOffset: "var(--focus-ring-offset)",
          },
        }}
        aria-expanded={open}
        aria-controls={collapseId}
        id={headerId}
        aria-label={
          open
            ? t("form.collapseSection", { title })
            : t("form.expandSection", { title })
        }
      >
        <SectionSubTitleRow title={title} accentStretch />
        <Box sx={{ display: "flex", alignItems: "center", gap: 0.5 }}>
          {action != null ? (
            <Box
              component="span"
              onClick={(e) => e.stopPropagation()}
              sx={{ display: "flex" }}
            >
              {action}
            </Box>
          ) : null}
          <Box
            component="span"
            sx={{
              display: "inline-flex",
              alignItems: "center",
              justifyContent: "center",
              width: "var(--icon-container-md)",
              height: "var(--icon-container-md)",
              borderRadius: "var(--radius-chip)",
              flexShrink: 0,
              color: "var(--primary)",
              bgcolor: "color-mix(in srgb, var(--primary) 10%, var(--card))",
              boxShadow: "var(--os3d-control-soft-lift-stack)",
              transition:
                "background-color var(--transition-duration) ease, color var(--transition-duration) ease",
            }}
            aria-hidden
          >
            {open ? (
              <ExpandLess sx={{ fontSize: "var(--icon-size-md)" }} />
            ) : (
              <ExpandMore sx={{ fontSize: "var(--icon-size-md)" }} />
            )}
          </Box>
        </Box>
      </Box>
      <Collapse in={open}>
        <Box
          id={collapseId}
          sx={{
            ...FORM_SECTION_MODULE_BODY_SX,
            display: "flex",
            flexDirection: "column",
            gap: 2,
          }}
        >
          {children}
        </Box>
      </Collapse>
    </Box>
  );
}
