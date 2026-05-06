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
  /** Stable DOM id seed when title is user-editable or can duplicate. */
  idBase?: string;
  /** 标题行右侧操作（如删除），点击不触发展开/收起 */
  action?: ReactNode;
}

function toDomIdPart(value: string): string {
  return (
    value.trim().replace(/[^A-Za-z0-9_-]+/g, "-").replace(/^-+|-+$/g, "") ||
    "section"
  );
}

export function FormSectionSubCollapsible({
  title,
  children,
  defaultOpen = true,
  idBase,
  action,
}: FormSectionSubCollapsibleProps) {
  const { t } = useTranslation();
  const [open, setOpen] = useState(defaultOpen);
  const domIdPart = toDomIdPart(idBase ?? title);
  const headerId = `header-${domIdPart}`;
  const collapseId = `collapse-${domIdPart}`;

  return (
    <Box
      sx={{
        "&:not(:first-of-type)": { mt: 2.5 },
        ...FORM_SECTION_MODULE_SX,
      }}
    >
      <Box
        sx={{
          ...FORM_SECTION_MODULE_HEADER_SX,
          display: "flex",
          alignItems: "center",
          justifyContent: "space-between",
          width: "100%",
          gap: 1,
        }}
      >
        <Box
          component="button"
          type="button"
          onClick={() => setOpen((o) => !o)}
          sx={{
            display: "flex",
            alignItems: "center",
            justifyContent: "space-between",
            flex: "1 1 auto",
            minWidth: 0,
            gap: 1,
            p: 0,
            border: 0,
            boxShadow: "none",
            bgcolor: "transparent",
            cursor: "pointer",
            color: "var(--foreground)",
            font: "inherit",
            textAlign: "left",
            "&:focus-visible": {
              outline:
                "var(--focus-ring-width) solid color-mix(in srgb, var(--primary) 55%, transparent)",
              outlineOffset: "var(--focus-ring-offset)",
              borderRadius: "var(--radius-control)",
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
          <Box
            component="span"
            sx={{
              display: "inline-flex",
              alignItems: "center",
              justifyContent: "center",
              width: "var(--icon-container-md)",
              height: "var(--icon-container-md)",
              borderRadius: "var(--radius-control)",
              flexShrink: 0,
              color: "var(--primary)",
              bgcolor: "color-mix(in srgb, var(--primary) 7%, var(--card))",
              border:
                "1px solid color-mix(in srgb, var(--primary) 16%, transparent)",
              boxShadow: "none",
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
        {action != null ? (
          <Box component="span" sx={{ display: "flex", flexShrink: 0 }}>
            {action}
          </Box>
        ) : null}
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
