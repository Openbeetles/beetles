import type { ReactElement } from "react";
import { Os3dIcon } from "../components/Os3dIcon";
import { OS_ICON_NAV } from "./osIcons";

export const OsIcon = ({ path }: { path: string }) => {
  const src = OS_ICON_NAV[path];
  if (!src) return null;
  return <Os3dIcon src={src} />;
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
