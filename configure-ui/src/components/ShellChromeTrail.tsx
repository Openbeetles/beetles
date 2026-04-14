import Box from "@mui/material/Box";
import Typography from "@mui/material/Typography";
import ChevronRightRounded from "@mui/icons-material/ChevronRightRounded";
import { useTranslation } from "react-i18next";
import { useLocation } from "react-router-dom";
import { NAV_ITEMS } from "../config/navItems";

/** 取最长匹配的导航项，使 /device-config/display 归类到「设备配置」 */
function navPathForLocation(pathname: string): string {
  const p = pathname || "/device";
  let best: string | null = null;
  let bestLen = -1;
  for (const item of NAV_ITEMS) {
    if (item.path === "/device") {
      if (p === "/device" || p === "/") {
        return "/device";
      }
      continue;
    }
    if (p === item.path || p.startsWith(`${item.path}/`)) {
      if (item.path.length > bestLen) {
        best = item.path;
        bestLen = item.path.length;
      }
    }
  }
  return best ?? "/device";
}

/** 顶栏面包屑：根 › 当前页（与任务栏路由一致） */
export function ShellBreadcrumb() {
  const { t } = useTranslation();
  const { pathname } = useLocation();
  const path = navPathForLocation(pathname);
  const item = NAV_ITEMS.find((n) => n.path === path);
  const currentLabel = item ? t(item.labelKey) : pathname;

  return (
    <Box
      component="nav"
      aria-label="breadcrumb"
      sx={{
        display: "flex",
        alignItems: "center",
        gap: 0.5,
        minWidth: 0,
        flexWrap: "wrap",
      }}
    >
      <Typography
        component="span"
        sx={{
          fontSize: "var(--font-size-caption)",
          fontWeight: 500,
          color: "var(--foreground-soft)",
          letterSpacing: "0.01em",
        }}
      >
        {t("shell.breadcrumbRoot")}
      </Typography>
      <ChevronRightRounded
        sx={{
          fontSize: "1rem",
          color: "var(--text-tertiary)",
          opacity: 0.85,
          flexShrink: 0,
        }}
        aria-hidden
      />
      <Typography
        component="span"
        title={typeof currentLabel === "string" ? currentLabel : undefined}
        sx={{
          fontSize: "var(--font-size-caption)",
          fontWeight: 600,
          letterSpacing: "0.01em",
          color: "var(--foreground)",
          overflow: "hidden",
          textOverflow: "ellipsis",
          whiteSpace: "nowrap",
          maxWidth: "min(52vw, 280px)",
        }}
      >
        {currentLabel}
      </Typography>
    </Box>
  );
}
