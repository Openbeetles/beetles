# beetle Rust 编码约束与架构约定

## 编码前必做

- **完整审查**：每次编码前必须完整审查并了解当前项目状态——现有模块、已实现调用链、本阶段目标与依赖关系。
- **明确调用链**：编码前厘清「谁创建、谁调用、谁消费」；只实现当前调用链上的节点，不提前抽象未使用路径。
- **分步实现**：按「定义类型 → 实现构造/发送/接收 → main 或组装点接线 → 单条链路验证」顺序推进；每步可编译、可验证后再进入下一步。
- **禁止冗余**：不引入本阶段未使用的类型、trait 或模块；不复制已有逻辑（如错误处理复用 `beetle::Error`）；同一职责只保留一处实现。

## 项目命名

- **中文名**：甲壳虫
- **英文名**：beetle
- **Crate 名**：`beetle`
- 对外类型与日志可带 "beetle" 或 "甲壳虫"；注释与文档保持中英双语。

## 架构原则

- **依赖倒置**：核心模块（agent、bus、llm、tools、memory）只依赖 trait，不依赖 `platform` 或 `channels`。具体实现由 `main` / `run_app` 注入。
- **平台隔离**：`esp_idf_svc` 及任何硬件专有调用**仅**出现在 `platform/` 目录或带 `#[cfg(any(target_arch = "xtensa", target_arch = "riscv32"))]` 守卫的文件/块中。业务层（agent、bus、llm、tools、channels、config、error、memory、doctor、skills、orchestrator、heartbeat、cron、state、cli）**禁止**直接引用 `esp_idf_svc`。新增功能涉及硬件时，必须在 `Platform` trait 上扩展方法（默认 no-op 或返回错误），由 `Esp32Platform` 实现，业务层通过 `platform.xxx()` 调用。
- **单一错误类型**：公共 API 返回 `Result<T, beetle::Error>`，使用 `?` 传播；错误带上下文（stage、status_code 等），禁止在生产路径使用 `unwrap()`/`expect()`。
- **配置显式下传**：通过参数传递 `AppConfig`，禁止全局可变静态保存配置或密钥。
- **通过通信共享**：任务间用 channel 传递 `PcMsg`，避免不必要的 `Arc<Mutex<T>>`；队列与缓冲区有固定容量与背压约定。
- **禁止 cue 词表设计**：记忆保存、长期记忆召回、用户偏好识别等能力，禁止依赖固定触发词、硬编码关键词列表、短语白名单/黑名单或同类 cue 词表实现。应优先使用结构化记忆、程序性状态/阈值、摘要 grounding、检索归一化与必要的语义检索；不得用 cue 词表替代通用多语言语义判断。

### 平台隔离门禁（合并前） / Platform isolation gate

- 须满足 [dev-docs/platform-isolation-plan.md](dev-docs/platform-isolation-plan.md) **§14.2**（业务域不得直引 `platform::spiffs` / `heap` / `hardware_drivers`，不得 `use crate::platform::*`）。合并前运行 `./scripts/check_platform_isolation.sh`（需安装 [ripgrep](https://github.com/BurntSushi/ripgrep) `rg`）；CI job `platform-isolation` 强制执行。可选：在本仓库执行 `git config core.hooksPath .githooks`，使提交前运行与 CI 相同检查。

## 安全

- 密钥与敏感字段**永不**打印、不写 SPIFFS 调试文件；配置加载后做必填与长度校验；启动期可调用 `config.validate_for_channels()` 校验当前 enabled_channel 对应凭证。
- 日志涉敏处使用 `util::redact_secret()` 脱敏；工具输入、会话与消息 content 有长度与格式上界，防止溢出与注入。
- **不搞伪需求**：在威胁模型已由现有手段覆盖时，不再叠加冗余安全措施；避免安全剧场式需求。

## 资源与弹性

- 队列、缓冲区、会话条数、单条消息/响应均有明确上界（见 `constants.rs`）；超时与退避常量集中（INBOUND*RECV_TIMEOUT_SECS、AGENT_RETRY*_、PENDING*RETRY_MAX_REPLAY、CHANNEL_FAIL*_）。
- 对外调用（LLM、HTTP、通道）需有超时与可配置重试/退避；失败返回 `Error` 而非 panic。dispatch 层单通道连续失败会熔断冷却，避免单通道拖垮全局。
- **资源可观测（orchestrator 为唯一权威）**：`GET /api/resource` 与 `orchestrator::snapshot()` / `format_resource_baseline_line()` 对齐；心跳在同周期内输出 orchestrator 单行基线 + `metrics::to_baseline_log_line`。**出站 Cautious**：`should_accept_outbound` 在 Cautious 下短延迟 `OUTBOUND_DEFER_DELAY_MS_CAUTIOUS`（500ms），Critical 仍用 `OUTBOUND_DEFER_DELAY_MS`；`GET /api/health` 嵌套 `metrics` 与 `resource` 快照（JSON 字段名与 serde 结构体一致）。

### ESP32 SRAM / PSRAM（嵌入式内存口径）

- **TLS / Transport**：生产路径为 **mbedTLS + esp-tls**；不得以「省 SRAM」为由擅自切换 **wolfSSL**，除非单独立项评审。**TLS stack stays mbedTLS + esp-tls; no wolfSSL migration for ad-hoc SRAM savings.**
- **HTTP 大响应体**：ESP 侧 HTTP 客户端已优先将响应体放入 **PSRAM**（如 `alloc_spiram_buffer`）；新功能勿再把「512KB 响应体迁 PSRAM」当作从零待办。**Large response bodies already prefer PSRAM on ESP HTTP path.**
- **语音与工具 HTTP**：在 **orchestrator 准入**内收口——减少语音链路上的整段复制与重复建连；语音相关出站应复用 **注入的 `ToolContext` / 既有 HTTP 客户端**，禁止为省内存再开一套并行 TLS 栈。**Voice/tools: fewer copies, no redundant per-tool HTTP clients outside the injected path.**
- **禁区（不放 PSRAM）**：**任务栈**、**DMA 描述符**、**Wi‑Fi / NVS 等 IDF 核心结构**须保留在 **internal DRAM**；实时或高频缓冲（如 SSE 行缓冲）按 `constants` 与现有设计留在 SRAM。**Stacks, DMA descriptors, Wi‑Fi/NVS internals stay internal; not SPIRAM.**

### ESP32 运行态进阶治理（吸收 xiaozhi 后）

- **先收模式，再做 admission**：新增 ESP 交互/联网能力时，先定义 steady-state mode、切换前置条件、挂起/恢复边界；不得只靠多开线程、补 permit、补重试来“允许共存”。
- **禁止恢复第二条常驻重执行面**：ESP 运行态只允许一个 `agent_loop` 作为常驻 LLM/TLS 重执行面；后台自治、维护、self-runtime 作业必须在同一 agent plane 串行消费。
- **管理面必须可驱逐**：`config_plane` 属 bootstrap / recovery plane；新增配置、诊断、对外服务若进入 ESP 主线，必须证明可暂停、可关闭，或其 steady-state 成本已被明确预算。
- **外部通道服从 voice-exclusive**：外部 WSS / IM 长连必须是从属 plane；进入 realtime voice 前先完成 suspend，退出后再 resume，禁止“边切模式边抢 TLS”。

## 代码风格

- 公共 API 必须有 rustdoc（中英均可）；新增模块在 `lib.rs` 或对应 `mod.rs` 中导出稳定接口。
- 遵循 `rustfmt` 与 `clippy`（`cargo clippy` 无警告）；嵌入式注意栈与堆使用，大 buffer 使用 PSRAM。

### 后台线程与资源

- **禁止重复线程**：同一职责（如显示刷新）只允许一个后台线程。若需要不同频率执行不同逻辑，在单一线程内用计数器区分，不得为此多开线程。ESP32 每个线程占 ~4KB 栈，资源宝贵。
- **禁止虚假度量**：显示、日志、API 返回的度量值（heap、CPU、温度等）必须来自真实数据源：对外口径以 `orchestrator::snapshot()` / `format_resource_baseline_line()` 等为权威；堆与内存维度由 `Platform::memory_snapshot()`（经 `run_app` 注册的 `register_memory_snapshot_provider` 闭包注入编排路径）与真实实现支撑。**禁止**业务域以直读 `platform::heap` 作为观测口径。禁止用固定映射或占位值冒充真实度量；暂时不可用的度量应传 0 或在 UI 标注 N/A。
- **绑核唯一入口**：多核/绑核改造只能通过 `platform/task_affinity.rs` 与 `util::spawn_guarded_with_profile*` 执行；业务域禁止直接使用 `esp_pthread_*`/`xTaskCreatePinnedToCore`。
- **线程角色一致性**：涉及 HTTP/TLS 的线程必须设置统一角色（`Interactive` / `Io` / `Background`），由 orchestrator 准入层统一消费；禁止各模块自行实现第二套 TLS 抢占策略。
- **Agent 独立线程**：`agent_main_loop` 必须在 Core1 独立线程运行并自行注册/喂 TWDT；主线程只负责监管与故障恢复（记录 + `request_restart`），不得再直接执行 agent 正文。
- **Linux / 嵌入式线程栈（必须遵守）**：在非 ESP/RISC-V 目标上，`rustls` 在调用方线程内完成 TLS 握手，栈深度远大于 ESP-IDF 路径。任何会调用 `create_http_client()` 或 `connect_wss()` 的线程，**禁止**写死 `8192` / `16384` 字节；必须使用 `src/util.rs` 中对应的 `STACK_*` 常量：
  - `STACK_CHANNEL_WS`：WS 长连接线程（`qq_ws`、`feishu_ws`）
  - `STACK_AGENT_LOOP`：Agent 主循环线程（`agent_user_loop`、`agent_system_loop`）
  - `STACK_CHANNEL_SENDER`：出站发送与入站轮询线程（`*_sender`、`tg_poll`）
  - 以上三者在 Linux 上统一由 `LINUX_RUSTLS_THREAD_STACK`（目前 **64KB**）驱动；若板子仍溢出，只改该单一常量。
  - 无 TLS 的纯调度/日志线程（`dispatch`、`http_server`、`bg_timer`、`heartbeat`）保持 8192 不变。

### 嵌入式字节序

- **ESP32 为 little-endian**：处理网络协议数据（IP 地址、端口等）时，注意网络字节序（大端）与平台字节序的转换。ESP-IDF 中 `esp_netif_ip_info_t.ip.addr` 以平台原生字节序存储，拆分字节应使用 `to_ne_bytes()` 而非 `to_be_bytes()`。

### 条件初始化

- **disabled 功能不初始化后端**：当功能被配置为 disabled 时（如 `display.enabled = false`），不应初始化对应的硬件后端或申请硬件资源。避免 disabled 配置的默认值变更导致意外初始化失败。

### 惯用 Rust

- 优先使用 `unwrap_or_else` 而非 `.or_else(|| Some(...))` + 后续 unwrap；避免不必要的 `Option` 包装。当 fallback 值确定时直接 `unwrap_or_else(|| default)` 得到内部类型。

### 显示与嵌入式性能规范（新增）

- **延时语义统一**：业务代码中的“毫秒级等待”统一使用 `std::thread::sleep(Duration::from_millis(...))`，禁止直接把毫秒值传给 `vTaskDelay`（tick 频率可配置，语义不稳定）。
- **热路径禁止无意义拷贝**：高频刷新路径（显示循环、队列消费）禁止 `clone()` 大缓冲（如行缓冲）；优先复用已分配内存并按引用发送。
- **disabled 必须零硬件副作用**：功能关闭时（如 `display.enabled=false`）不得初始化后端、不得申请总线/GPIO 资源。后端句柄使用 `Option`，执行时显式短路。
- **禁留伪检查死代码**：禁止 `_assets_sanity` 这类“只读取不校验”的占位代码；编译期可保证的事实（如 `include_bytes!` 文件存在）不再做运行时伪校验。
- **后台循环复用容器**：周期线程中禁止每轮新建 `Vec<String/...>`；循环外预分配，循环内 `clear + extend` 复用，降低堆碎片与抖动。
- **字节序先证实再改**：涉及网络/IP 字节序时，必须先确认 ESP-IDF/lwIP 字段存储语义再改实现；禁止“凭经验”把 `to_ne_bytes()` 直接改为 `to_be_bytes()`。

## Git

- **Commit messages**：必须使用英文撰写（subject 与 body 均英文），便于国际协作与历史检索。

## Linux 嵌入式 WiFi（P2）

- **Linux 嵌入式 WiFi P2** 指控制面（rtnetlink、`wpa_supplicant`/hostapd ctrl 套接字、systemd 单元、`metrics` WiFi 字段），与帧缓冲显示 / [linux-migration-plan.md](dev-docs/linux-migration-plan.md) Step 7 显示迁移**无关**。

## 扩展点

- 新通道：实现 `MessageSink` trait 并注册，不修改 bus/agent。
- 新工具：实现 `Tool` trait 并注册到 `ToolRegistry`。
- 新 LLM：实现 `LlmClient` trait 并注入。
- 新流式编辑通道：实现 `StreamEditor` trait（`send_initial` + `edit`），在 `main.rs` 根据 `enabled_channel` 创建并注入 `AgentLoopConfig.stream_editor`。实现方使用 **agent 线程内 `thread_local` 连接槽位**复用 HTTP 客户端，禁止跨线程共享非 `Send` 客户端；失败路径统一 `reset_connection_for_retry` 后重试，仍失败则失效槽位重建。
- 新平台：实现 `Platform` trait（含 `init`、`init_nvs`、`init_spiffs`、`connect_wifi`、`create_http_client`、`request_restart`、`init_sntp`、`ota_from_url` 等）及 `PlatformHttpClient`、`ChannelHttpClient`、`WssConnection` 等 trait，在 main 中替换平台实例与工厂闭包。业务层代码无需改动。

---

## 已实现模块的封装与约束

### 网络与 HTTP

- **WiFi**：`connect_wifi(config: &AppConfig) -> Result<()>`。调用前对 config 做 `validate_for_wifi()`；Linux STA 墙钟轮询用 `constants::WIFI_CONNECT_TIMEOUT_SECS`；ESP 主线程等待 WiFi 线程首包 `Ok` 用 **`WIFI_ESP_CONNECT_MAIN_WAIT_SECS`**（与 Linux 解耦）。错误 stage 为 `wifi_connect`。**P0 语义（ESP 与 Linux 嵌入式一致）**：只要 **SoftAP 已起且 `wifi_scan()` 可用**，`connect_wifi` 应返回 **`Ok(())`**；用户配置的 **STA 密码错误或暂时连不上** 时 **不得** 因 STA 失败而让整次 `connect_wifi` 失败（否则配网页/扫描不可用、设备等同失联）。**用户可见「WiFi 是否连上路由器」**（`/api/health` 的 `wifi`、CLI、Telegram `/status` 等）一律以 **`is_wifi_sta_connected()`** 为准；**不得** 与「`connect_wifi` 成功 / 仅配网栈就绪」混用。仅在 **AP/驱动无法启动** 等致命错误时返回 `Err`。
- **Linux 嵌入式 WiFi（补充）**：启动前 `iw dev … info` + `iw phy … info` 探测 **AP 能力** 与 **`valid interface combinations` 是否同时含 managed+AP**（启发式）；无 AP 能力则 `wifi_capability_check` 失败。未声明并发时 **只保 SoftAP**，不启 `wpa_supplicant` STA，`GET /api/wifi/scan` 走 **`iw dev <iface> scan`**；守护线程周期 **`WIFI_LINUX_DAEMON_WATCH_INTERVAL_SECS`**，用 PID 文件 + `kill -0` 检测 **hostapd/dnsmasq**，死则 `stop_ap`+`start_ap`；若存在 **wpa_supplicant** PID 文件且进程已死则 `ensure_daemon`。stage **`wifi_daemon_watch`** 仅用于看门狗路径。
- **HTTP 客户端**：由 **main 构造一次**，LLM、工具、通道均**注入**同一 `EspHttpClient`，不在各处再 `EspHttpClient::new()`。直连用 `new()`，走 proxy 用 `new_with_config(&config)`。响应体上限 512KB，请求超时 30s。**例外**：`StreamEditor` 与 sender 线程使用独立连接；`StreamEditor` 在 agent 线程内通过 `thread_local` 槽位复用，不允许新增跨线程共享路径。
- **Error stage**：网络/HTTP 错误带 stage（`wifi_connect`、`http_client_new`、`http_get_request`、`http_get_submit`、`proxy_connect`）；不新增未使用 Error 变体。
- **platform 边界**：platform 是唯一依赖 esp-idf-svc 的模块；核心域（llm、agent、tools、channels）不直接依赖 platform，通过 main 注入的客户端或 trait 使用。GPIO/PWM/ADC/Buzzer 等硬件能力仅通过 **`Platform` trait**（如 `drive_gpio_out`、`drive_adc_in` 等）暴露，由 `Esp32Platform` / `LinuxPlatform` 委托 `platform/hardware_drivers`；业务域（含 `tools`）**禁止** `use crate::platform::hardware_drivers`（见 §14.2 门禁脚本）。`lib.rs` 中 ESP 专有类型的 re-export 均有 `#[cfg(any(target_arch = "xtensa", target_arch = "riscv32"))]` 守卫。

### LLM

- **LlmClient**：Agent/main 只依赖此 trait。`chat(&self, http: &mut dyn LlmHttpClient, system, messages, tools) -> Result<LlmResponse>`；**llm 不依赖 platform**，HTTP 由调用方注入。
- **LLM 分层例外口径**：当前 `llm::LlmHttpClient` 仍复用 `platform::ResponseBody`，且 `fallback/retry` 会调用 `platform::task_wdt::feed_current_task()`；这是为嵌入式内存与 TWDT 稳定性的有意工程折中。若未来要求“llm 字面零 platform 依赖”，需单独立项引入 llm 侧自有响应体/喂狗抽象，再由 `lib` 桥接实现。
- **chat_with_progress**：可选流式回调方法；默认回退到 `chat`。`StreamProgressFn<'a> = &'a mut dyn FnMut(&str, &str)` 传递 `(delta, accumulated)`。`FallbackLlmClient` 仅第一源走 progress，后续降级普通 chat。
- **LlmHttpClient**：在 **lib** 中由 `EspHttpClient` 实现（`do_post(url, headers, body)`）；新增 LLM 厂商时仅新增实现 `LlmClient` 的类型，不修改 platform。
- **类型与常量**：`Message`、`LlmResponse`、`ToolSpec` 在 `llm::types`；`MAX_REQUEST_BODY_LEN`（512KB）、`MAX_MESSAGE_CONTENT_LEN`（64KB）；序列化超限用 `Error::Config`。
- **错误 stage**：LLM 用 `llm_request`、`llm_parse`。

### 工具

- **Tool**：`execute(&self, args: &str, ctx: &mut dyn ToolContext) -> Result<String>`；**tools 不依赖 platform**，HTTP 通过 ToolContext 注入。
- **ToolContext**：在 **lib** 中由 `EspHttpClient` 实现（`get` / `get_with_headers`）；需自定义 header 时用 `get_with_headers`。
- **常量**：`MAX_TOOL_ARGS_LEN`（8KB）、`MAX_TOOL_RESULT_LEN`（16KB）；args 在 `registry.execute` 内校验；错误 stage 如 `tool_web_search`、`tool_execute`。
- **ToolRegistry**：main 构造一次并注册所有工具；`build_default_registry(config, platform, …)` 需传入 `Arc<dyn Platform>`（如 `board_info`）；Agent 注入同一 Registry；`tool_specs_for_api(max_len)`、`format_descriptions_for_system_prompt(max_chars)` 总长度不超过参数。
- **新增工具**：实现 `Tool` 并在 main 中 `registry.register(Box::new(XxxTool::new(...)))`；不修改 platform。
