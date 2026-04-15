#!/usr/bin/env bash
# 从 microsoft/fluentui-emoji（MIT）拉取 3D PNG 到 public/icons。
# Fluent 单库即有数千枚 3D 资产，可按语义为各工具分配不同图标，避免「一张图到处用」。
# 可选第二来源（需单独评估风格）：googlefonts/noto-emoji（Apache 2.0 / OFL），与本项目拟物 3D 可能不一致，默认不混用。
set -euo pipefail
BASE="https://raw.githubusercontent.com/microsoft/fluentui-emoji/main"
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
OUT="$ROOT/public/icons"

fetch() {
  local rel="$1"
  local dest="$2"
  echo "fetch $dest"
  curl -fsSL "$BASE/$rel" -o "$OUT/$dest"
}

mkdir -p "$OUT"

# —— 主导航 + 壳层 ——
fetch "assets/House/3D/house_3d.png" "home_3d.png"
fetch "assets/Robot/3D/robot_3d.png" "bot_3d.png"
fetch "assets/Speech%20balloon/3D/speech_balloon_3d.png" "chat_3d.png"
fetch "assets/Artist%20palette/3D/artist_palette_3d.png" "theme_3d.png"
fetch "assets/Puzzle%20piece/3D/puzzle_piece_3d.png" "puzzle_3d.png"
fetch "assets/Hammer%20and%20wrench/3D/hammer_and_wrench_3d.png" "tools_3d.png"
fetch "assets/Mobile%20phone/3D/mobile_phone_3d.png" "devices_3d.png"
fetch "assets/Memo/3D/memo_3d.png" "history_3d.png"
fetch "assets/Gear/3D/gear_3d.png" "settings_3d.png"
fetch "assets/Chart%20increasing/3D/chart_increasing_3d.png" "strategy_3d.png"
fetch "assets/Link/3D/link_3d.png" "link_3d.png"
fetch "assets/Desktop%20computer/3D/desktop_computer_3d.png" "device_info_3d.png"
fetch "assets/Computer%20disk/3D/computer_disk_3d.png" "storage_3d.png"
fetch "assets/Brain/3D/brain_3d.png" "memory_3d.png"
fetch "assets/Counterclockwise%20arrows%20button/3D/counterclockwise_arrows_button_3d.png" "runtime_3d.png"
fetch "assets/Double%20exclamation%20mark/3D/double_exclamation_mark_3d.png" "faults_3d.png"
fetch "assets/Electric%20plug/3D/electric_plug_3d.png" "power_3d.png"

# —— 仪表盘磁贴（与主导航刻意区分，避免同页重复感）——
fetch "assets/Wireless/3D/wireless_3d.png" "dash_connection_3d.png"
fetch "assets/Antenna%20bars/3D/antenna_bars_3d.png" "dash_channels_3d.png"
# 设备离线缓存蒙层：与「正常无线连接」成对的否定语义（Fluent Cross mark · 红叉）
fetch "assets/Cross%20mark/3D/cross_mark_3d.png" "device_unreachable_3d.png"
# 未保存修改弹窗：剪贴板草稿未写入（Fluent Clipboard，与错误/离线图标区分）
fetch "assets/Clipboard/3D/clipboard_3d.png" "unsaved_changes_3d.png"

# —— 通用工具基底 ——
fetch "assets/Stopwatch/3D/stopwatch_3d.png" "time_3d.png"
fetch "assets/Calendar/3D/calendar_3d.png" "calendar_3d.png"
fetch "assets/Alarm%20clock/3D/alarm_clock_3d.png" "alarm_3d.png"
fetch "assets/Envelope/3D/envelope_3d.png" "mail_3d.png"
fetch "assets/Busts%20in%20silhouette/3D/busts_in_silhouette_3d.png" "contacts_3d.png"
# 主导航「账户管理」：密码箱 / 凭据保管（与通讯录 capability 的 contacts_3d 区分）
fetch "assets/Locked%20with%20key/3D/locked_with_key_3d.png" "safe_3d.png"
fetch "assets/Microphone/3D/microphone_3d.png" "microphone_3d.png"
fetch "assets/Speaker%20high%20volume/3D/speaker_high_volume_3d.png" "speaker_3d.png"
fetch "assets/Camera/3D/camera_3d.png" "camera_3d.png"
fetch "assets/Potted%20plant/3D/potted_plant_3d.png" "garden_3d.png"
fetch "assets/File%20folder/3D/file_folder_3d.png" "folder_3d.png"
fetch "assets/Stethoscope/3D/stethoscope_3d.png" "diagnose_3d.png"
fetch "assets/Globe%20with%20meridians/3D/globe_with_meridians_3d.png" "globe_3d.png"
fetch "assets/Magnifying%20glass%20tilted%20right/3D/magnifying_glass_tilted_right_3d.png" "search_3d.png"
fetch "assets/Keyboard/3D/keyboard_3d.png" "keyboard_3d.png"

# —— 工具列表：一工具一图（尽量不重复）——
fetch "assets/Check%20mark%20button/3D/check_mark_button_3d.png" "tool_task_3d.png"
fetch "assets/Fountain%20pen/3D/fountain_pen_3d.png" "tool_write_3d.png"
fetch "assets/Pencil/3D/pencil_3d.png" "tool_edit_3d.png"
fetch "assets/Spiral%20notepad/3D/spiral_notepad_3d.png" "tool_remind_list_3d.png"
fetch "assets/Microscope/3D/microscope_3d.png" "tool_doc_search_3d.png"
fetch "assets/Open%20book/3D/open_book_3d.png" "tool_doc_read_3d.png"
fetch "assets/Outbox%20tray/3D/outbox_tray_3d.png" "tool_doc_extract_3d.png"
fetch "assets/Briefcase/3D/briefcase_3d.png" "tool_documents_3d.png"
fetch "assets/Page%20with%20curl/3D/page_with_curl_3d.png" "tool_pdf_3d.png"
fetch "assets/Spider%20web/3D/spider_web_3d.png" "tool_web_fetch_3d.png"
fetch "assets/Envelope%20with%20arrow/3D/envelope_with_arrow_3d.png" "tool_http_3d.png"
fetch "assets/Joystick/3D/joystick_3d.png" "tool_device_ctrl_3d.png"
fetch "assets/Nut%20and%20bolt/3D/nut_and_bolt_3d.png" "tool_i2c_dev_3d.png"
fetch "assets/Thermometer/3D/thermometer_3d.png" "tool_sensor_3d.png"
fetch "assets/Books/3D/books_3d.png" "tool_mem_manage_3d.png"
fetch "assets/Magnifying%20glass%20tilted%20left/3D/magnifying_glass_tilted_left_3d.png" "tool_mem_search_3d.png"
fetch "assets/Card%20file%20box/3D/card_file_box_3d.png" "tool_mem_get_3d.png"
fetch "assets/Balance%20scale/3D/balance_scale_3d.png" "tool_factual_3d.png"
fetch "assets/Placard/3D/placard_3d.png" "tool_message_3d.png"
fetch "assets/Control%20knobs/3D/control_knobs_3d.png" "tool_sys_ctrl_3d.png"
fetch "assets/Hourglass%20done/3D/hourglass_done_3d.png" "tool_cron_3d.png"
fetch "assets/Eye%20in%20speech%20bubble/3D/eye_in_speech_bubble_3d.png" "tool_sensor_watch_3d.png"
fetch "assets/Satellite%20antenna/3D/satellite_antenna_3d.png" "tool_net_scan_3d.png"
fetch "assets/Ringed%20planet/3D/ringed_planet_3d.png" "tool_network_3d.png"
fetch "assets/Identification%20card/3D/identification_card_3d.png" "tool_env_3d.png"
fetch "assets/Bridge%20at%20night/3D/bridge_at_night_3d.png" "tool_proxy_3d.png"
fetch "assets/Office%20building/3D/office_building_3d.png" "tool_office_cfg_3d.png"
fetch "assets/Newspaper/3D/newspaper_3d.png" "tool_office_status_3d.png"
fetch "assets/Snake/3D/snake_3d.png" "lua_snake_3d.png"
fetch "assets/Abacus/3D/abacus_3d.png" "lua_abacus_3d.png"
fetch "assets/Hook/3D/hook_3d.png" "lua_hook_3d.png"
fetch "assets/Rocket/3D/rocket_3d.png" "diag_delivery_3d.png"
fetch "assets/Compass/3D/compass_3d.png" "diag_network_3d.png"
fetch "assets/Studio%20microphone/3D/studio_microphone_3d.png" "diag_voice_3d.png"
fetch "assets/Floppy%20disk/3D/floppy_disk_3d.png" "diag_memory_3d.png"
fetch "assets/Bookmark%20tabs/3D/bookmark_tabs_3d.png" "bookmark_tabs_3d.png"

echo "OK -> $OUT ($(ls -1 "$OUT"/*.png 2>/dev/null | wc -l | tr -d ' ') PNG)"
