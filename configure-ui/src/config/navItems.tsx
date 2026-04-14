import type { ReactElement } from "react";
import Box from "@mui/material/Box";

const OS_ICONS: Record<string, string> = {
  "/device": "/icons/home_3d.png",
  "/ai-config": "/icons/bot_3d.png",
  "/channels-config": "/icons/chat_3d.png",
  "/soul-user": "/icons/theme_3d.png",
  "/skills": "/icons/puzzle_3d.png",
  "/tools": "/icons/tools_3d.png",
  "/device-config": "/icons/devices_3d.png",
  "/system-logs": "/icons/history_3d.png",
  "/system-config": "/icons/settings_3d.png",
};

export const OsIcon = ({ path }: { path: string }) => {
  return (
    <Box
      component="img"
      src={OS_ICONS[path]}
      alt=""
      draggable="false"
      sx={{
        width: "100%",
        height: "100%",
        objectFit: "contain",
        // 给真实拟物图片增加一点柔和的投影，让它更有立体感并仿佛浮在底座上
        filter: "drop-shadow(0 4px 6px rgba(0, 0, 0, 0.15)) drop-shadow(0 1px 3px rgba(0, 0, 0, 0.1))",
        transition: "transform 0.25s cubic-bezier(0.34, 1.56, 0.64, 1)",
      }}
    />
  );
};

/** 主导航项（任务栏 / 开始菜单共用） / Primary nav (taskbar + Start menu) */
export const NAV_ITEMS: {
  path: string;
  labelKey: string;
  icon: ReactElement;
}[] = [
  { path: "/device", labelKey: "nav.device", icon: <OsIcon path="/device" /> },
  { path: "/ai-config", labelKey: "nav.aiConfig", icon: <OsIcon path="/ai-config" /> },
  {
    path: "/channels-config",
    labelKey: "nav.channelsConfig",
    icon: <OsIcon path="/channels-config" />,
  },
  { path: "/soul-user", labelKey: "nav.soulUser", icon: <OsIcon path="/soul-user" /> },
  { path: "/skills", labelKey: "nav.skills", icon: <OsIcon path="/skills" /> },
  { path: "/tools", labelKey: "nav.tools", icon: <OsIcon path="/tools" /> },
  {
    path: "/device-config",
    labelKey: "nav.deviceConfig",
    icon: <OsIcon path="/device-config" />,
  },
  {
    path: "/system-logs",
    labelKey: "nav.systemLogs",
    icon: <OsIcon path="/system-logs" />,
  },
  {
    path: "/system-config",
    labelKey: "nav.systemConfig",
    icon: <OsIcon path="/system-config" />,
  },
];
