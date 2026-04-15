import { OS_ICON_NAV } from "./osIcons";

export interface NavItem {
  path: string;
  labelKey: string;
  iconSrc: string;
}

/** 主导航项（任务栏 / 开始菜单共用） / Primary nav (taskbar + Start menu) */
export const NAV_ITEMS: NavItem[] = [
  {
    path: "/device",
    labelKey: "nav.device",
    iconSrc: OS_ICON_NAV["/device"],
  },
  {
    path: "/ai-config",
    labelKey: "nav.aiConfig",
    iconSrc: OS_ICON_NAV["/ai-config"],
  },
  {
    path: "/channels-config",
    labelKey: "nav.channelsConfig",
    iconSrc: OS_ICON_NAV["/channels-config"],
  },
  {
    path: "/accounts",
    labelKey: "nav.accounts",
    iconSrc: OS_ICON_NAV["/accounts"],
  },
  {
    path: "/soul-user",
    labelKey: "nav.soulUser",
    iconSrc: OS_ICON_NAV["/soul-user"],
  },
  {
    path: "/skills",
    labelKey: "nav.skills",
    iconSrc: OS_ICON_NAV["/skills"],
  },
  {
    path: "/tools",
    labelKey: "nav.tools",
    iconSrc: OS_ICON_NAV["/tools"],
  },
  {
    path: "/device-config",
    labelKey: "nav.deviceConfig",
    iconSrc: OS_ICON_NAV["/device-config"],
  },
  {
    path: "/system-logs",
    labelKey: "nav.systemLogs",
    iconSrc: OS_ICON_NAV["/system-logs"],
  },
  {
    path: "/system-config",
    labelKey: "nav.systemConfig",
    iconSrc: OS_ICON_NAV["/system-config"],
  },
];
