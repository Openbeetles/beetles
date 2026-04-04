import { useContext, useEffect, useRef } from "react";
import { Outlet, useLocation, useNavigate } from "react-router-dom";
import { useTranslation } from "react-i18next";
import Box from "@mui/material/Box";
import Tab from "@mui/material/Tab";
import Tabs from "@mui/material/Tabs";
import { NavBlockerContext } from "../../contexts/NavBlockerContext";
import { SoulUserConfigProvider } from "../../contexts/SoulUserConfigProvider";

/** 与路由 `/soul-user/:tab` 对齐 */
const TAB_SEGMENTS = ["soul", "user"] as const;
type TabSegment = (typeof TAB_SEGMENTS)[number];

function tabFromPathname(pathname: string): TabSegment {
  const parts = pathname.split("/").filter(Boolean);
  const seg = parts[1] as TabSegment | undefined;
  if (seg && (TAB_SEGMENTS as readonly string[]).includes(seg)) return seg;
  return "soul";
}

function SoulUserLayoutShell() {
  const { t } = useTranslation();
  const navigate = useNavigate();
  const { pathname } = useLocation();
  const scrollRef = useRef<HTMLDivElement>(null);
  const navBlocker = useContext(NavBlockerContext);
  const tab = tabFromPathname(pathname);

  useEffect(() => {
    scrollRef.current?.scrollTo({ top: 0, left: 0 });
  }, [pathname]);

  const goToTab = (segment: TabSegment) => {
    const next = `/soul-user/${segment}`;
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
        height: "100%",
        display: "flex",
        flexDirection: "column",
        minHeight: 0,
        width: "100%",
      }}
    >
      <Box
        sx={{
          flexShrink: 0,
          borderBottom: "none",
          backgroundColor: "var(--surface)",
        }}
      >
        <Tabs
          value={tab}
          onChange={(_, v) => goToTab(v as TabSegment)}
          sx={{
            minHeight: 48,
            "& .MuiTab-root": {
              textTransform: "none",
              fontWeight: 600,
              fontSize: "var(--font-size-body-sm)",
            },
          }}
        >
          <Tab value="soul" label={t("soulUser.tabSoul")} />
          <Tab value="user" label={t("soulUser.tabUser")} />
        </Tabs>
      </Box>
      <Box
        ref={scrollRef}
        sx={{
          flex: 1,
          minHeight: 0,
          overflow: "auto",
          pt: 2,
          pb: 4,
        }}
      >
        <Outlet />
      </Box>
    </Box>
  );
}

/**
 * 个性配置壳层：顶部 Tab + 子路由滚动区（布局与设备配置 `/device-config` 一致）。
 */
export function SoulUserLayout() {
  return (
    <SoulUserConfigProvider>
      <SoulUserLayoutShell />
    </SoulUserConfigProvider>
  );
}
