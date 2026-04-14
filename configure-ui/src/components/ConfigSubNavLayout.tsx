import { useContext } from "react";
import { Outlet, useLocation, useNavigate } from "react-router-dom";
import { useTranslation } from "react-i18next";
import Box from "@mui/material/Box";
import List from "@mui/material/List";
import ListItem from "@mui/material/ListItem";
import ListItemButton from "@mui/material/ListItemButton";
import ListItemText from "@mui/material/ListItemText";
import { NavBlockerContext } from "../contexts/NavBlockerContext";
import {
  CONFIG_PANEL_SX,
  PANEL_SECTION_PADDING,
} from "../theme/panelStyles";

export type ConfigSubNavItem = {
  segment: string;
  label: string;
};

type ConfigSubNavLayoutProps = {
  /** 路由前缀，如 `/device-config`、`/soul-user`（无尾部斜杠） */
  basePath: string;
  items: ConfigSubNavItem[];
};

function activeSegment(pathname: string, items: ConfigSubNavItem[]): string {
  const seg = pathname.split("/").filter(Boolean)[1];
  if (seg && items.some((i) => i.segment === seg)) return seg;
  return items[0]?.segment ?? "";
}

/**
 * 配置类子路由壳层：左侧（窄屏为顶栏横向）分区导航 + 右侧主体滚动区。
 * Settings-style sub-route shell: side nav + scrolling main (horizontal nav on xs).
 */
export function ConfigSubNavLayout({ basePath, items }: ConfigSubNavLayoutProps) {
  const { t } = useTranslation();
  const navigate = useNavigate();
  const { pathname } = useLocation();
  const navBlocker = useContext(NavBlockerContext);
  const tab = activeSegment(pathname, items);
  const base = basePath.replace(/\/$/, "");

  const goTo = (segment: string) => {
    const next = `${base}/${segment}`;
    if (pathname === next) return;
    if (navBlocker?.attemptNavigate) {
      navBlocker.attemptNavigate(next);
    } else {
      navigate(next);
    }
  };

  return (
    <Box
      sx={{
        flex: 1,
        minHeight: 0,
        height: "100%",
        display: "flex",
        flexDirection: { xs: "column", md: "row" },
        alignItems: { md: "stretch" },
        alignContent: { md: "stretch" },
        gap: { xs: 2, md: 2.5 },
        width: "100%",
        overflow: "hidden",
      }}
    >
      <Box
        component="nav"
        aria-label={t("common.configSectionNavAria")}
        sx={{
          ...CONFIG_PANEL_SX,
          flexShrink: 0,
          width: { xs: "100%", md: 236 },
          boxSizing: "border-box",
          /** 与 SettingsSection 主卡片一致，避免左右视觉「多一层 padding」 */
          p: PANEL_SECTION_PADDING,
          alignSelf: { md: "stretch" },
          height: { md: "100%" },
          minHeight: { md: 0 },
          display: "flex",
          flexDirection: "column",
        }}
      >
        <List
          disablePadding
          sx={{
            display: { xs: "flex", md: "block" },
            flexDirection: { xs: "row", md: "column" },
            flexWrap: { xs: "nowrap", md: "wrap" },
            overflowX: { xs: "auto", md: "visible" },
            gap: { xs: 0.5, md: 0.25 },
            flex: { md: 1 },
            minHeight: 0,
            scrollbarWidth: "thin",
          }}
        >
          {items.map((item) => (
            <ListItem key={item.segment} disablePadding sx={{ flexShrink: 0 }}>
              <ListItemButton
                selected={tab === item.segment}
                aria-current={tab === item.segment ? "page" : undefined}
                disableRipple
                onClick={() => goTo(item.segment)}
                sx={{
                  borderRadius: "var(--radius-control)",
                  py: { xs: 1, md: 1.125 },
                  px: { xs: 1.5, md: 1.25 },
                  width: { xs: "auto", md: "100%" },
                  whiteSpace: { xs: "nowrap", md: "normal" },
                  "&.Mui-selected": {
                    bgcolor: "color-mix(in srgb, var(--primary) 10%, transparent)",
                    color: "var(--primary)",
                    "&:hover": {
                      bgcolor:
                        "color-mix(in srgb, var(--primary) 14%, transparent)",
                    },
                  },
                }}
              >
                <ListItemText
                  primary={item.label}
                  primaryTypographyProps={{
                    fontWeight: tab === item.segment ? 700 : 600,
                    fontSize: "var(--font-size-body-sm)",
                  }}
                />
              </ListItemButton>
            </ListItem>
          ))}
        </List>
      </Box>
      <Box
        sx={{
          flex: 1,
          minHeight: 0,
          minWidth: 0,
          overflow: "hidden",
          display: "flex",
          flexDirection: "column",
          pb: 0,
        }}
      >
        <Outlet />
      </Box>
    </Box>
  );
}
