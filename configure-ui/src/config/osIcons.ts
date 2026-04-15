/**
 * 拟物 3D 图标路径单源（`/public/icons/*.png`）。
 * 导航与仪表盘卡片共用同一套视觉；来源与版权见 `public/icons/README.md`。
 *
 * Single source for glossy 3D PNG paths; see `public/icons/README.md` for attribution.
 */

/** 主导航路径 → 图标（任务栏 / 开始菜单） */
export const OS_ICON_NAV: Record<string, string> = {
  "/device": "/icons/home_3d.png",
  "/ai-config": "/icons/bot_3d.png",
  "/channels-config": "/icons/chat_3d.png",
  /** 账户管理（密码箱隐喻：Fluent Locked with key） */
  "/accounts": "/icons/safe_3d.png",
  "/soul-user": "/icons/theme_3d.png",
  "/skills": "/icons/puzzle_3d.png",
  "/tools": "/icons/tools_3d.png",
  /** 设备配置入口（子页见 `OS_ICON_DEVICE_CONFIG`） */
  "/device-config": "/icons/devices_3d.png",
  "/system-logs": "/icons/history_3d.png",
  "/system-config": "/icons/settings_3d.png",
};

/**
 * 设备配置子路由专用 3D 图标（显示 / 音频 / GPIO·硬件），与主导航入口 `devices_3d` 区分粒度。
 * Per-tab icons for /device-config/* — distinct from the parent nav glyph.
 */
export const OS_ICON_DEVICE_CONFIG = {
  /** 显示 / 画面与屏相关 */
  display: "/icons/camera_3d.png",
  /** 音频 / 扬声器侧（面板内可再区分麦与回放） */
  audio: "/icons/speaker_3d.png",
  /** GPIO / 外设与设备控制 */
  hardware: "/icons/tool_device_ctrl_3d.png",
} as const;

/** 设备首页仪表盘卡片（Fluent 3D 补充项） */
export const OS_ICON_DASHBOARD = {
  /** 连接 / 配对（与导航「链接」图标区分：无线；正常态用） */
  connection: "/icons/dash_connection_3d.png",
  /** 故障与恢复（仪表盘磁贴） */
  faults: "/icons/faults_3d.png",
  /**
   * 设备不可达 / 离线缓存全屏蒙层专用（Fluent **Cross mark** 3D，红叉；与正常无线 `dash_connection` 成对）。
   * Dedicated overlay when device is unreachable — Cross mark, paired with healthy wireless tile.
   */
  deviceUnreachable: "/icons/device_unreachable_3d.png",
  /** 设备信息摘要 */
  deviceInfo: "/icons/device_info_3d.png",
  /** 通道连通性（与主导航「对话气泡」区分：信号格） */
  channels: "/icons/dash_channels_3d.png",
  /** 存储占用 */
  storage: "/icons/storage_3d.png",
  /** 内存 / 堆（隐喻：脑 — 可换其它 Fluent 资源） */
  memory: "/icons/memory_3d.png",
  /** 运行策略 */
  strategy: "/icons/strategy_3d.png",
  /** 运行时 / 交换 */
  runtime: "/icons/runtime_3d.png",
} as const;

export type OsDashboardIconKey = keyof typeof OS_ICON_DASHBOARD;

/** 壳层：开始菜单标题栏「重启」（Fluent 3D · Electric plug） */
export const OS_ICON_SHELL = {
  power: "/icons/power_3d.png",
  /**
   * 顶栏「壳层偏好」抽屉入口：语言 / 明暗 / 强调色（与任务栏「系统设置」`settings_3d` 区分）。
   * Shell preferences (locale & appearance) — distinct from device System Config gear.
   */
  preferences: "/icons/globe_3d.png",
} as const;

/**
 * 全站级确认弹窗专用 3D 隐喻（与主导航 / 仪表盘磁贴解耦，避免复用）。
 * Dedicated 3D glyphs for app-wide `ConfirmDialog` — not shared with nav or dashboard tiles.
 */
export const OS_ICON_DIALOG = {
  /**
   * 未保存修改 / 放弃编辑（Fluent **Clipboard** — 剪贴区待落盘草稿，非错误叉号、非设备离线）。
   * Unsaved changes: clipboard as pending edits not yet persisted.
   */
  unsavedChanges: "/icons/unsaved_changes_3d.png",
} as const;

/**
 * 设置抽屉内分区图标（与导航/仪表盘路径解耦，专用隐喻）。
 * Section icons inside the preferences drawer.
 */
export const OS_ICON_PREFERENCES = {
  /** 语言 / locale */
  language: "/icons/globe_3d.png",
  /** 明暗主题（区块标题；与当前模式对应的 toggle 图标见 modeLight / modeDark） */
  themeMode: "/icons/theme_3d.png",
  /** 浅色模式 toggle */
  modeLight: "/icons/time_3d.png",
  /** 深色模式 toggle */
  modeDark: "/icons/runtime_3d.png",
  /** 强调色 / 品牌色 */
  accent: "/icons/garden_3d.png",
} as const;

/**
 * 工具列表项（GET /api/tools 的 `name`）→ 3D PNG。
 * 与既有 `public/icons` 资源语义对齐；未收录名称回退为 `OS_ICON_NAV["/tools"]`。
 */
export const OS_ICON_TOOL: Record<string, string> = {
  get_time: "/icons/time_3d.png",
  task: "/icons/tool_task_3d.png",
  calendar: "/icons/calendar_3d.png",
  files: "/icons/folder_3d.png",
  file_write: "/icons/tool_write_3d.png",
  file_edit: "/icons/tool_edit_3d.png",
  remind_at: "/icons/alarm_3d.png",
  remind_list: "/icons/tool_remind_list_3d.png",
  board_info: "/icons/device_info_3d.png",
  kv_store: "/icons/storage_3d.png",
  document_search: "/icons/tool_doc_search_3d.png",
  document_read: "/icons/tool_doc_read_3d.png",
  document_extract: "/icons/tool_doc_extract_3d.png",
  documents: "/icons/tool_documents_3d.png",
  pdf_read: "/icons/tool_pdf_3d.png",
  web_search: "/icons/search_3d.png",
  web_fetch: "/icons/tool_web_fetch_3d.png",
  http_request: "/icons/tool_http_3d.png",
  analyze_image: "/icons/camera_3d.png",
  device_control: "/icons/tool_device_ctrl_3d.png",
  i2c_device: "/icons/tool_i2c_dev_3d.png",
  i2c_sensor: "/icons/tool_sensor_3d.png",
  memory_manage: "/icons/tool_mem_manage_3d.png",
  memory_search: "/icons/tool_mem_search_3d.png",
  memory_get: "/icons/tool_mem_get_3d.png",
  factual_memory: "/icons/tool_factual_3d.png",
  private_garden: "/icons/garden_3d.png",
  continuity_snapshot: "/icons/bookmark_tabs_3d.png",
  session_manage: "/icons/chat_3d.png",
  message: "/icons/tool_message_3d.png",
  system_control: "/icons/tool_sys_ctrl_3d.png",
  cron_manage: "/icons/tool_cron_3d.png",
  sensor_watch: "/icons/tool_sensor_watch_3d.png",
  network_scan: "/icons/tool_net_scan_3d.png",
  process: "/icons/runtime_3d.png",
  network: "/icons/tool_network_3d.png",
  shell: "/icons/keyboard_3d.png",
  env: "/icons/tool_env_3d.png",
  proxy_config: "/icons/tool_proxy_3d.png",
  model_config: "/icons/bot_3d.png",
  voice_input: "/icons/microphone_3d.png",
  voice_output: "/icons/speaker_3d.png",
  /** 办公 / 通讯录 */
  office_config: "/icons/tool_office_cfg_3d.png",
  office_status: "/icons/tool_office_status_3d.png",
  contacts_directory: "/icons/contacts_3d.png",
  mail: "/icons/mail_3d.png",
  /** 诊断类（各一条目独立隐喻） */
  diagnose_delivery: "/icons/diag_delivery_3d.png",
  diagnose_system: "/icons/diagnose_3d.png",
  diagnose_network_path: "/icons/diag_network_3d.png",
  diagnose_voice_path: "/icons/diag_voice_3d.png",
  diagnose_memory_runtime: "/icons/diag_memory_3d.png",
  /** Lua 脚本桥 */
  lua_query: "/icons/lua_snake_3d.png",
  lua_memory_query: "/icons/lua_abacus_3d.png",
  lua_tool_bridge: "/icons/lua_hook_3d.png",
};

/** 工具名 → 3D 图标路径（未知工具用工具页主图标） */
export function osIconSrcForToolName(name: string): string {
  return OS_ICON_TOOL[name] ?? OS_ICON_NAV["/tools"];
}
