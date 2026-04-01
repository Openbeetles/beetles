/**
 * 固件工具名（GET /api/tools 的 name）→ 语义化图标。
 * 未收录的新工具回退到 ExtensionOutlined。
 */
import type { SvgIconProps } from "@mui/material/SvgIcon";
import type { ElementType } from "react";
import AccessTimeOutlined from "@mui/icons-material/AccessTimeOutlined";
import AdminPanelSettingsOutlined from "@mui/icons-material/AdminPanelSettingsOutlined";
import AlarmOutlined from "@mui/icons-material/AlarmOutlined";
import CalendarMonthOutlined from "@mui/icons-material/CalendarMonthOutlined";
import CellTowerOutlined from "@mui/icons-material/CellTowerOutlined";
import DescriptionOutlined from "@mui/icons-material/DescriptionOutlined";
import EditNoteOutlined from "@mui/icons-material/EditNoteOutlined";
import ExtensionOutlined from "@mui/icons-material/ExtensionOutlined";
import FileDownloadOutlined from "@mui/icons-material/FileDownloadOutlined";
import FindInPageOutlined from "@mui/icons-material/FindInPageOutlined";
import FolderOpenOutlined from "@mui/icons-material/FolderOpenOutlined";
import FormatListBulletedOutlined from "@mui/icons-material/FormatListBulletedOutlined";
import ForumOutlined from "@mui/icons-material/ForumOutlined";
import HubOutlined from "@mui/icons-material/HubOutlined";
import ImageSearchOutlined from "@mui/icons-material/ImageSearchOutlined";
import LanOutlined from "@mui/icons-material/LanOutlined";
import MemoryOutlined from "@mui/icons-material/MemoryOutlined";
import MonitorHeartOutlined from "@mui/icons-material/MonitorHeartOutlined";
import NoteAddOutlined from "@mui/icons-material/NoteAddOutlined";
import PsychologyOutlined from "@mui/icons-material/PsychologyOutlined";
import ScheduleOutlined from "@mui/icons-material/ScheduleOutlined";
import SensorsOutlined from "@mui/icons-material/SensorsOutlined";
import SettingsRemoteOutlined from "@mui/icons-material/SettingsRemoteOutlined";
import StorageOutlined from "@mui/icons-material/StorageOutlined";
import TaskAltOutlined from "@mui/icons-material/TaskAltOutlined";
import TerminalOutlined from "@mui/icons-material/TerminalOutlined";
import TravelExploreOutlined from "@mui/icons-material/TravelExploreOutlined";

const TOOL_ICONS: Record<string, ElementType<SvgIconProps>> = {
  get_time: AccessTimeOutlined,
  task: TaskAltOutlined,
  calendar: CalendarMonthOutlined,
  files: FolderOpenOutlined,
  file_write: NoteAddOutlined,
  file_edit: EditNoteOutlined,
  remind_at: AlarmOutlined,
  remind_list: FormatListBulletedOutlined,
  board_info: MemoryOutlined,
  kv_store: StorageOutlined,
  document_search: FindInPageOutlined,
  document_read: DescriptionOutlined,
  document_extract: FileDownloadOutlined,
  web_search: TravelExploreOutlined,
  analyze_image: ImageSearchOutlined,
  device_control: SettingsRemoteOutlined,
  i2c_device: HubOutlined,
  i2c_sensor: SensorsOutlined,
  memory_manage: PsychologyOutlined,
  session_manage: ForumOutlined,
  system_control: AdminPanelSettingsOutlined,
  cron_manage: ScheduleOutlined,
  sensor_watch: MonitorHeartOutlined,
  network_scan: CellTowerOutlined,
  process: TerminalOutlined,
  network: LanOutlined,
};

export function ToolGlyph({
  name,
  ...props
}: { name: string } & SvgIconProps) {
  const Icon = TOOL_ICONS[name] ?? ExtensionOutlined;
  return <Icon {...props} />;
}
