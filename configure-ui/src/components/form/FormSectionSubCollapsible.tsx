import { useState } from "react";
import Box from "@mui/material/Box";
import Collapse from "@mui/material/Collapse";
import Typography from "@mui/material/Typography";
import ExpandMore from "@mui/icons-material/ExpandMore";
import ExpandLess from "@mui/icons-material/ExpandLess";
import type { ReactNode } from "react";

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
  const [open, setOpen] = useState(defaultOpen);
  const headerId = `header-${title.replace(/\s/g, "-")}`;
  const collapseId = `collapse-${title.replace(/\s/g, "-")}`;

  return (
    <Box
      sx={{
        "&:not(:first-of-type)": { mt: 2 },
        border: "1px solid color-mix(in srgb, var(--border) 18%, transparent)",
        borderRadius: "var(--radius-control)",
        overflow: "hidden",
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
          width: "100%",
          p: 1.5,
          border: 0,
          background: "transparent",
          cursor: "pointer",
          color: "var(--foreground)",
          font: "inherit",
          textAlign: "left",
          transition: "background-color var(--transition-duration) ease",
          "&:hover": {
            bgcolor: "color-mix(in srgb, var(--primary) 4%, transparent)",
          },
          "&:focus-visible": {
            outline: "2px solid var(--primary)",
            outlineOffset: 2,
          },
        }}
        aria-expanded={open}
        aria-controls={collapseId}
        id={headerId}
      >
        <Box sx={{ display: "flex", alignItems: "center" }}>
          <Typography
            variant="caption"
            sx={{
              fontWeight: 600,
              letterSpacing: "var(--letter-spacing-label)",
              color: "var(--muted)",
              textTransform: "uppercase",
              fontSize: "var(--font-size-overline)",
            }}
          >
            {title}
          </Typography>
        </Box>
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
              color: "var(--muted)",
              p: 0.5,
              display: "inline-flex",
              alignItems: "center",
            }}
            aria-hidden
          >
            {open ? (
              <ExpandLess fontSize="small" />
            ) : (
              <ExpandMore fontSize="small" />
            )}
          </Box>
        </Box>
      </Box>
      <Collapse in={open}>
        <Box
          id={collapseId}
          sx={{
            px: 2,
            pt: 2,
            pb: 2,
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
