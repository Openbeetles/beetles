import type { ReactElement } from "react";
import ChatBubbleOutlineRounded from "@mui/icons-material/ChatBubbleOutlineRounded";
import ExtensionOutlined from "@mui/icons-material/ExtensionOutlined";
import HandymanOutlined from "@mui/icons-material/HandymanOutlined";
import HomeRounded from "@mui/icons-material/HomeRounded";
import HistoryOutlined from "@mui/icons-material/HistoryOutlined";
import PaletteOutlined from "@mui/icons-material/PaletteOutlined";
import SettingsOutlined from "@mui/icons-material/SettingsOutlined";
import DevicesOtherOutlined from "@mui/icons-material/DevicesOtherOutlined";
import SmartToyOutlined from "@mui/icons-material/SmartToyOutlined";

/** 主导航项（任务栏 / 开始菜单共用） / Primary nav (taskbar + Start menu) */
export const NAV_ITEMS: {
  path: string;
  labelKey: string;
  icon: ReactElement;
}[] = [
  { path: "/device", labelKey: "nav.device", icon: <HomeRounded /> },
  { path: "/ai-config", labelKey: "nav.aiConfig", icon: <SmartToyOutlined /> },
  {
    path: "/channels-config",
    labelKey: "nav.channelsConfig",
    icon: <ChatBubbleOutlineRounded />,
  },
  { path: "/soul-user", labelKey: "nav.soulUser", icon: <PaletteOutlined /> },
  { path: "/skills", labelKey: "nav.skills", icon: <ExtensionOutlined /> },
  { path: "/tools", labelKey: "nav.tools", icon: <HandymanOutlined /> },
  {
    path: "/device-config",
    labelKey: "nav.deviceConfig",
    icon: <DevicesOtherOutlined />,
  },
  {
    path: "/system-logs",
    labelKey: "nav.systemLogs",
    icon: <HistoryOutlined />,
  },
  {
    path: "/system-config",
    labelKey: "nav.systemConfig",
    icon: <SettingsOutlined />,
  },
];
