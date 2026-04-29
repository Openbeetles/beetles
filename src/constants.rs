//! 资源上界单源；仅支持带 PSRAM 的 ESP32-S3。
//! Single source for resource bounds; only ESP32-S3 with PSRAM supported.

/// 入站/出站队列固定容量（条数）。
#[cfg(any(target_arch = "xtensa", target_arch = "riscv32"))]
pub const DEFAULT_CAPACITY: usize = 16;
#[cfg(not(any(target_arch = "xtensa", target_arch = "riscv32")))]
pub const DEFAULT_CAPACITY: usize = 64;

/// Per-channel outbound sender queue capacity.
#[cfg(any(target_arch = "xtensa", target_arch = "riscv32"))]
pub const CHANNEL_SENDER_QUEUE_DEPTH: usize = 8;
#[cfg(not(any(target_arch = "xtensa", target_arch = "riscv32")))]
pub const CHANNEL_SENDER_QUEUE_DEPTH: usize = 32;

/// Voice event queue capacity between wake/sink producers and the voice session scheduler.
pub const VOICE_EVENT_QUEUE_CAPACITY: usize = 4;

/// 单条消息 content 最大长度（字节）。
#[cfg(any(target_arch = "xtensa", target_arch = "riscv32"))]
pub const MAX_CONTENT_LEN: usize = 64 * 1024;
#[cfg(not(any(target_arch = "xtensa", target_arch = "riscv32")))]
pub const MAX_CONTENT_LEN: usize = 256 * 1024;

/// HTTP 响应体最大读取字节数。
#[cfg(any(target_arch = "xtensa", target_arch = "riscv32"))]
pub const MAX_RESPONSE_BODY_LEN: usize = 512 * 1024;
#[cfg(not(any(target_arch = "xtensa", target_arch = "riscv32")))]
pub const MAX_RESPONSE_BODY_LEN: usize = 2 * 1024 * 1024;

/// LLM 请求体最大字节数，与 MAX_RESPONSE_BODY_LEN 一致。
pub const MAX_REQUEST_BODY_LEN: usize = MAX_RESPONSE_BODY_LEN;

/// 系统提示总长度上界（字符）。
#[cfg(any(target_arch = "xtensa", target_arch = "riscv32"))]
pub const DEFAULT_SYSTEM_MAX_LEN: usize = 32 * 1024;
#[cfg(not(any(target_arch = "xtensa", target_arch = "riscv32")))]
pub const DEFAULT_SYSTEM_MAX_LEN: usize = 64 * 1024;

/// 会话 messages 总长度上界（字符）。
#[cfg(any(target_arch = "xtensa", target_arch = "riscv32"))]
pub const DEFAULT_MESSAGES_MAX_LEN: usize = 24 * 1024;
#[cfg(not(any(target_arch = "xtensa", target_arch = "riscv32")))]
pub const DEFAULT_MESSAGES_MAX_LEN: usize = 128 * 1024;

/// TLS 准入：有 PSRAM 时允许发起单次 TLS（HTTP/WSS）要求的最小 internal 空闲（字节）。
/// 有 PSRAM 时 mbedTLS 大部分分配走 SPIRAM，internal 仅需 ~15KB 给硬件加密/DMA。
/// 实测稳态 internal ~47KB，38KB 阈值留 ~18KB 给硬件加密+DMA，避免边缘误拒。
pub const TLS_ADMISSION_MIN_INTERNAL_BYTES: usize = 38 * 1024;
/// TLS 准入：要求 internal 最大连续块不低于此值，避免碎片化导致 mbedTLS 分配失败。
pub const TLS_ADMISSION_MIN_LARGEST_BLOCK_BYTES: usize = 24 * 1024;
/// 在 TLS 最大连续块硬门槛之上保留一小段 headroom，便于 operator/config 面在碎片化前主动退让。
pub const TLS_FRAGMENTATION_CAUTION_HEADROOM_BYTES: usize = 4 * 1024;
/// TLS 准入：无 PSRAM 时 internal 堆空闲下限（字节），mbedTLS 全部走 internal 需更多空间。
pub const TLS_ADMISSION_NO_PSRAM_MIN_BYTES: usize = 72 * 1024;
/// ESP 稳态传输余量：低于该 internal 空闲值时不再视作 Normal，即便 TLS 硬门槛尚未触发。
pub const ESP_TRANSPORT_STEADY_INTERNAL_CAUTION_BYTES: usize =
    TLS_ADMISSION_MIN_INTERNAL_BYTES + 16 * 1024;

/// 低内存且非 cron 时，重入队后休眠毫秒数，避免忙等、给 internal 恢复时间。
pub const LOW_MEM_DEFER_SLEEP_MS: u64 = 1800;

/// 入站 defer 最大重试次数；超过后降级回复"设备忙碌"，不再重入队。
/// Max defer retries for the same inbound message before degraded reply.
pub const MAX_DEFER_RETRIES: u8 = 3;

/// 工具结果拼成一条 user 消息时，user_content 部分的字节数上限（4 KiB）。
pub const MAX_TOOL_RESULTS_USER_MESSAGE_LEN: usize = 4 * 1024;

/// Agent 结构化输出：固件将当轮 user 消息标为截断时优先保留。
pub const AGENT_MARKER_MARK_IMPORTANT: &str = "[MARK_IMPORTANT]";
/// Agent 结构化输出：固件在下轮 build_context 时注入情绪提示，随后清除。
pub const AGENT_MARKER_SIGNAL_COMFORT: &str = "[SIGNAL:comfort]";

/// remind_at 存储条目数上界；超过时实现应拒绝或淘汰最旧。
pub const REMIND_AT_MAX_ENTRIES: usize = 32;
/// remind_at 单条 context 最大字节数；实现应截断或拒绝超长。
pub const REMIND_AT_MAX_CONTEXT_LEN: usize = 512;

/// 会话摘要存贮与注入时截断上限（字符）。
pub const SESSION_SUMMARY_MAX_LEN: usize = 1024;

// ---------- 可靠性：超时与退避（须小于 TWDT 超时，避免静默复位） ----------
/// Agent 入站 recv 超时（秒）；超时后喂狗再继续等待。
pub const INBOUND_RECV_TIMEOUT_SECS: u64 = 30;
/// Agent 同一消息重试时退避基数（毫秒）；第 n 次重试 sleep(base * 2^n)，上限 AGENT_RETRY_MAX_MS。
pub const AGENT_RETRY_BASE_MS: u64 = 100;
/// Agent 重试退避上限（毫秒）。
pub const AGENT_RETRY_MAX_MS: u64 = 500;
/// Cautious 压力下 LLM RetryLater 的等待毫秒数，避免固定 3s 造成体感卡顿。
pub const LLM_RETRY_LATER_DELAY_MS: u64 = 700;
/// pending_retry 重放次数上限；超过则清除不再注入，避免重复饥饿。
pub const PENDING_RETRY_MAX_REPLAY: u32 = 3;
/// Dispatch 单通道连续失败后熔断冷却时间（秒）；冷却期内不再向该通道发送。
pub const CHANNEL_FAIL_COOLDOWN_SECS: u64 = 60;
/// Dispatch 熔断阈值：连续失败此次数后进入冷却。
pub const CHANNEL_FAIL_THRESHOLD: u32 = 3;

/// SSE 流式响应行缓冲区大小（字节）；单行 SSE data 不应超此值。
#[cfg(any(target_arch = "xtensa", target_arch = "riscv32"))]
pub const SSE_LINE_BUF_SIZE: usize = 6 * 1024;
#[cfg(not(any(target_arch = "xtensa", target_arch = "riscv32")))]
pub const SSE_LINE_BUF_SIZE: usize = 16 * 1024;

/// HTTP 最大并发连接数（含 TLS）。lwIP ~10 socket，预留给 WSS/HTTP 服务器后可用 ~6，但 TLS 内存限制更紧。
#[cfg(any(target_arch = "xtensa", target_arch = "riscv32"))]
pub const MAX_CONCURRENT_HTTP: usize = 3;
#[cfg(not(any(target_arch = "xtensa", target_arch = "riscv32")))]
pub const MAX_CONCURRENT_HTTP: usize = 16;

/// 压力判级（ESP32）：Normal 阈值的 internal 空闲下限（字节）。
#[cfg(any(target_arch = "xtensa", target_arch = "riscv32"))]
pub const PRESSURE_NORMAL_INTERNAL_MIN_BYTES: usize = 60 * 1024;
/// 压力判级（Linux）：Normal 阈值的可用内存下限（字节）。64MB 以下视为紧张。
#[cfg(not(any(target_arch = "xtensa", target_arch = "riscv32")))]
pub const PRESSURE_NORMAL_INTERNAL_MIN_BYTES: usize = 64 * 1024 * 1024;

/// 压力判级：Normal 阈值的 PSRAM 空闲下限（字节）。Linux 无 PSRAM，此值不参与判定。
pub const PRESSURE_NORMAL_PSRAM_MIN_BYTES: usize = 3 * 1024 * 1024;

/// 压力判级（ESP32）：Cautious 阈值的 internal 空闲下限（字节）。
#[cfg(any(target_arch = "xtensa", target_arch = "riscv32"))]
pub const PRESSURE_CAUTIOUS_INTERNAL_MIN_BYTES: usize = 48 * 1024;
/// 压力判级（Linux）：Cautious 阈值的可用内存下限（字节）。32MB 以下视为严重。
#[cfg(not(any(target_arch = "xtensa", target_arch = "riscv32")))]
pub const PRESSURE_CAUTIOUS_INTERNAL_MIN_BYTES: usize = 32 * 1024 * 1024;

/// 压力判级：Cautious 阈值的 PSRAM 空闲下限（字节）。Linux 无 PSRAM，此值不参与判定。
pub const PRESSURE_CAUTIOUS_PSRAM_MIN_BYTES: usize = 768 * 1024;
/// 压力判级：队列拥塞阈值（入站+出站总深度），默认容量下为 75%。
pub const PRESSURE_QUEUE_CONGESTION_THRESHOLD: u32 = (DEFAULT_CAPACITY as u32) * 2 * 3 / 4;

/// kv_store 工具：最多允许存储的条目数。
pub const KV_STORE_MAX_ENTRIES: usize = 64;
/// kv_store 工具：key 最大字节数；只允许 [a-zA-Z0-9_\-.] 字符。
pub const KV_STORE_MAX_KEY_LEN: usize = 64;
/// kv_store 工具：value 最大字节数；适合偏好/状态等小型数据。
pub const KV_STORE_MAX_VALUE_LEN: usize = 512;

/// 会话 GC：过期时间（秒），7 天。
pub const SESSION_GC_MAX_AGE_SECS: u64 = 604_800;
/// 会话 GC：每 N 轮 heartbeat 执行一次（~50 分钟 @ 30s 间隔）。
pub const SESSION_GC_INTERVAL_ROUNDS: u32 = 100;

/// 会话/存储指标采集频率：每 N 轮 heartbeat 执行一次（~5 分钟 @ 30s 间隔）。
/// Session/storage metrics collection interval (heartbeat rounds).
pub const SESSION_METRICS_INTERVAL_ROUNDS: u32 = 10;

/// 出站门禁 Critical 压力时延迟毫秒数。
/// Outbound admission defer delay under Critical pressure.
pub const OUTBOUND_DEFER_DELAY_MS: u64 = 1400;
/// 出站门禁 Cautious 压力时轻量退避毫秒数（短于 Critical，与队列拥塞→Cautious 闭环）。
/// Light outbound defer under Cautious pressure (shorter than Critical).
pub const OUTBOUND_DEFER_DELAY_MS_CAUTIOUS: u64 = 350;
/// QQ sender 第一次重试前等待毫秒数（attempt=2）。
pub const QQ_SEND_RETRY_DELAY_MS_STEP1: u64 = 300;
/// QQ sender 第二次重试前等待毫秒数（attempt=3）。
pub const QQ_SEND_RETRY_DELAY_MS_STEP2: u64 = 550;
/// post-reply 后台维护进入主 lane 前要求的最小静默窗口（秒）。
/// Prevents post-reply governance work from immediately stealing the next conversational turn.
pub const POST_REPLY_BACKGROUND_QUIET_WINDOW_SECS: u64 = 15;
/// post-reply 后台治理在连续入站/低 largest-block 下的最大延后窗口（毫秒）。
/// After this window, embedded profile may only run lightweight/segmented maintenance.
pub const POST_REPLY_BACKGROUND_MAX_DEFER_MS: u64 = 30_000;

// ---------- 显示自适应刷新频率 ----------
/// 显示刷新间隔：Busy 状态（秒）。
pub const DISPLAY_REFRESH_BUSY_SECS: u64 = 2;
/// 显示刷新间隔：Idle 状态（秒）。
pub const DISPLAY_REFRESH_IDLE_SECS: u64 = 5;
/// 显示刷新间隔：长时间 Idle（秒）。
pub const DISPLAY_REFRESH_IDLE_LONG_SECS: u64 = 10;
/// 显示刷新间隔：熄屏/睡眠状态（秒）。
pub const DISPLAY_REFRESH_SLEEP_SECS: u64 = 30;
/// 进入长时间 Idle 刷新的阈值（秒）。
pub const DISPLAY_IDLE_LONG_THRESHOLD_SECS: u64 = 30;

/// cron_manage 工具：持久化定时任务最大条目数。
pub const CRON_TASKS_MAX_ENTRIES: usize = 16;
/// cron_manage 工具：单条任务 action 最大字节数。
pub const CRON_TASK_MAX_ACTION_LEN: usize = 512;
/// file_write 工具：写入内容最大字节数。
pub const FILE_WRITE_MAX_CONTENT_LEN: usize = 16 * 1024;
/// daily_note 工具：list 操作最大返回条数。
pub const DAILY_NOTE_MAX_LIST: usize = 30;

// ---------- SoftAP（设备热点）固定地址 ----------
/// SoftAP 网关 IPv4（点分十进制）；与 `platform::softap_ip`、文档、`configure-ui` 一致。
/// Fixed SoftAP gateway IPv4; must match firmware, docs, and config UI.
pub const SOFTAP_DEFAULT_IPV4: &str = "192.168.4.1";
/// 浏览器访问设备配置页的默认基址（HTTP，端口 80）。
/// Default base URL for the on-device config UI (HTTP port 80).
pub const SOFTAP_DEFAULT_BASE_URL: &str = "http://192.168.4.1";
/// 当 STA 已占用 `192.168.4.0/24` 时，SoftAP 避让使用的备用网关（与文档 §10.3 一致）。
pub const SOFTAP_FALLBACK_IPV4: &str = "172.16.42.1";

// ---------- WiFi（跨平台统一超时/退避） ----------
/// WiFi 首轮连接判定超时（秒），用于 Linux `wpa` 轮询等 STA 墙钟判定（非 ESP 主线程）。
pub const WIFI_CONNECT_TIMEOUT_SECS: u64 = 15;
/// ESP 上 `wifi::connect` **主线程**等待 WiFi 子线程首包 `Ok(())` 的墙钟上限（秒）。
/// 须 ≥ STA 慢握手场景，与 [`WIFI_CONNECT_TIMEOUT_SECS`] 解耦，避免与 Linux 共用 15s 导致误杀。
pub const WIFI_ESP_CONNECT_MAIN_WAIT_SECS: u64 = 45;
/// WiFi 扫描请求等待超时（秒），用于 `wifi_scan.request_scan`。
pub const WIFI_SCAN_TIMEOUT_SECS: u64 = 15;
/// WiFi 重连退避序列（秒）：用于 Linux/host 后台重试与状态机。
pub const WIFI_RETRY_BACKOFF_SECS: [u64; 3] = [5, 10, 20];
/// Linux 嵌入式：`hostapd` / `dnsmasq` /（可选）`wpa_supplicant` 存活检查周期（秒）。
pub const WIFI_LINUX_DAEMON_WATCH_INTERVAL_SECS: u64 = 15;
/// 并发 STA+AP 时 hostapd 使用的虚拟 AP 接口名（`iw dev <phy> interface add` 创建）。
pub const WIFI_LINUX_AP_VIRT_IFACE: &str = "ap0";

// ---------- network_scan 工具 ----------
/// WiFi 扫描最小间隔（毫秒）。
pub const NETWORK_SCAN_MIN_INTERVAL_MS: u64 = 2000;

// ---------- sensor_watch 工具 ----------
/// 传感器监控最大条目数。
pub const SENSOR_WATCH_MAX_ENTRIES: usize = 8;
/// 传感器监控最小检查间隔（秒）。
pub const SENSOR_WATCH_MIN_INTERVAL_SECS: u64 = 60;
/// 传感器监控告警消息最大长度（字节）。
pub const SENSOR_WATCH_MAX_ALERT_LEN: usize = 512;

// ---------- i2c_device 工具 ----------
/// I2C 读操作最小间隔（毫秒）。
pub const I2C_READ_MIN_INTERVAL_MS: u64 = 500;
/// I2C 写操作最小间隔（毫秒）。
pub const I2C_WRITE_MIN_INTERVAL_MS: u64 = 2000;
/// I2C 单次读取最大字节数。
pub const I2C_MAX_READ_LEN: usize = 32;
/// I2C 单次写入最大字节数。
pub const I2C_MAX_WRITE_LEN: usize = 32;
/// I2C 最大设备数。
pub const I2C_MAX_DEVICES: usize = 8;
/// I2C 默认频率（Hz）。
pub const I2C_DEFAULT_FREQ_HZ: u32 = 100_000;

// ---------- i2c_sensor 工具 / drive_i2c_sensor ----------
/// I2C 传感器配置最大条目数。
pub const I2C_SENSOR_MAX_ENTRIES: usize = 8;
/// I2C 传感器 `id` 最大长度（字节）。
pub const I2C_SENSOR_ID_MAX_LEN: usize = 64;
/// I2C 传感器读取最小间隔（毫秒）。
pub const I2C_SENSOR_RATE_LIMIT_MS: u64 = 2_000;
// ---------- voice_input / voice_output ----------
/// 语音采集每帧样本数（16kHz 下约 20ms）。
pub const AUDIO_CAPTURE_FRAME_SAMPLES: usize = 320;
/// 单次语音采集最长时长（毫秒）。
pub const AUDIO_CAPTURE_MAX_MS: u32 = 12_000;
/// 语音工具可发送到 STT 的最大 PCM 字节数（约 30 秒 16k 单声道 16-bit）。
pub const AUDIO_STT_MAX_PCM_BYTES: usize = 960_000;
/// TTS 输入文本最大长度（UTF-8 字节）。
pub const AUDIO_TTS_MAX_TEXT_LEN: usize = 512;
/// TTS 播放写喇叭时的分块样本数。
pub const AUDIO_TTS_WRITE_CHUNK_SAMPLES: usize = 1024;
/// 等待喇叭播放队列排空时的轮询间隔（毫秒）。
pub const AUDIO_SPEAKER_DRAIN_POLL_MS: u64 = 10;
/// 在预估播放时长之上追加的排空宽限（毫秒）。
pub const AUDIO_SPEAKER_DRAIN_GRACE_MS: u64 = 1_500;
/// `raw` 模型 `options.init_cmd` 最大长度（字节）。
pub const I2C_SENSOR_MAX_CMD_LEN: usize = 4;
// ---------- wake_word / voice session ----------
/// 唤醒词检测后的冷却时长（毫秒）。防止误触连续灌爆事件队列。
/// Cooldown after a wake-word detection (ms). Prevents false triggers flooding the event queue.
pub const WAKE_WORD_COOLDOWN_MS: u64 = 2_000;
/// 语音虚拟通道名称（dispatch 按此路由到 VoiceSink）。
pub const VOICE_CHANNEL_NAME: &str = "voice";
/// 语音交互的固定 chat_id（本地设备只有一个用户）。
pub const VOICE_DEVICE_CHAT_ID: &str = "device";
