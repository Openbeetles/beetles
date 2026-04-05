import { useContext, useEffect, useRef, useState } from "react";
import { Outlet, useLocation, useNavigate } from "react-router-dom";
import { useTranslation } from "react-i18next";
import Box from "@mui/material/Box";
import Tab from "@mui/material/Tab";
import Tabs from "@mui/material/Tabs";
import WarningAmberRounded from "@mui/icons-material/WarningAmberRounded";
import { ConfirmDialog } from "../components/ConfirmDialog";
import { NavBlockerContext } from "../contexts/NavBlockerContext";

/** 与路由 `/device-config/:tab` 对齐；新增设备子模块时在此追加 */
const TAB_SEGMENTS = ["display", "audio", "hardware"] as const;
type TabSegment = (typeof TAB_SEGMENTS)[number];

function tabFromPathname(pathname: string): TabSegment {
  const seg = pathname.split("/").filter(Boolean)[1] as TabSegment | undefined;
  if (seg && (TAB_SEGMENTS as readonly string[]).includes(seg)) return seg;
  return "display";
}

/**
 * 设备配置壳层：顶部 Tab 固定，子路由内容在下方独立滚动。
 * /device-config/display — 显示；/device-config/audio — 音频；/device-config/hardware — GPIO 等硬件设备
 */
export function DeviceConfigLayout() {
  const { t } = useTranslation();
  const navigate = useNavigate();
  const { pathname } = useLocation();
  const scrollRef = useRef<HTMLDivElement>(null);
  const prevPathRef = useRef<string | null>(null);
  const [disclaimerOpen, setDisclaimerOpen] = useState(false);
  const tab = tabFromPathname(pathname);
  const navBlocker = useContext(NavBlockerContext);

  useEffect(() => {
    let cancelled = false;
    const enteredFromOutside =
      prevPathRef.current === null ||
      !prevPathRef.current.startsWith("/device-config");
    if (pathname.startsWith("/device-config") && enteredFromOutside) {
      queueMicrotask(() => {
        if (!cancelled) setDisclaimerOpen(true);
      });
    }
    prevPathRef.current = pathname;
    return () => {
      cancelled = true;
    };
  }, [pathname]);

  useEffect(() => {
    scrollRef.current?.scrollTo({ top: 0, left: 0 });
  }, [pathname]);

  const goToTab = (segment: TabSegment) => {
    const next = `/device-config/${segment}`;
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
      <ConfirmDialog
        open={disclaimerOpen}
        onClose={() => setDisclaimerOpen(false)}
        onCancel={() => {
          navigate("/device");
          setDisclaimerOpen(false);
        }}
        requireExplicitAction
        wide
        title={t("deviceConfig.disclaimerTitle")}
        description={t("deviceConfig.disclaimerDesc")}
        icon={<WarningAmberRounded />}
        confirmColor="error"
        cancelLabel={t("deviceConfig.disclaimerLeave")}
        confirmLabel={t("deviceConfig.disclaimerContinue")}
        onConfirm={() => {}}
      />
      <Box
        sx={{
          flexShrink: 0,
          borderBottom: "none",
          backgroundColor: "var(--card)",
        }}
      >
        <Tabs
          value={tab}
          onChange={(_, v) => goToTab(v as TabSegment)}
          sx={{
            minHeight: 48,
            "& .MuiTab-root": {
              fontSize: "var(--font-size-body-sm)",
            },
          }}
        >
          <Tab value="display" label={t("deviceConfig.tabDisplay")} />
          <Tab value="audio" label={t("deviceConfig.tabAudio")} />
          <Tab value="hardware" label={t("deviceConfig.tabGpioDevices")} />
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
