# 资源与可观测 / Resources and Observability

> 状态说明（2026-03-30）：本文以当前 `src/constants.rs` 与在用调用链为准；若与实现冲突，以代码为准并及时回写文档。
> 2026-04-05 补充：realtime 语音新增 duplex capability 口径、turn fencing 与 interrupt/timeout/stale-drop 指标，排障时优先看 heartbeat `metrics` 行。
> 2026-04-07 补充：关于“不削功能、不削 UX、不破坏 OS 完整性”的 ESP 运行态资源治理主线，见 [esp-runtime-resource-governance.md](./esp-runtime-resource-governance.md)。
> 2026-04-10 补充：`network.outbound_http` 已改为“transport readiness + 真实出站 HTTP 成功/失败观察”双源收口；sender / token / gateway / direct send / stream editor 链路的成功失败都会统一推进 capability 与 metrics。transport 错误不再在 LLM / token 刷新等上游链路被拍平成泛化 IO 文本，而是保留嵌套根因；`tls_admission` 现在会穿透包装层直达 heartbeat `err_tls_admission`。dispatch cooldown buffer 也已改为 idle tick 周期重放，不再依赖“刚好有下一条新消息”才 replay。QQ / 钉钉群入站的 `PcMsg::is_group` 语义已收回到统一构造辅助，避免同类语义丢失。`SessionStore` 热路径追加缓存也已显式追踪 `has_data / ends_with_newline`，避免反复出现 `repaired session ... bad_lines=1` 的 JSONL 自修复循环。
> 2026-04-10 再补充：ESP 资源口径新增显式 `tls_fragmentation_risk`（由 `heap_largest_block_internal` + TLS 连续块阈值推导）。heartbeat `resource` 基线与 `/api/resource` 现在直接暴露该状态；`GET /api/channel_connectivity` 在 WiFi 未 settle 或 `tls_fragmentation_risk` 进入 `cautious/critical` 时返回 stale snapshot，不再为了诊断探测强行创建新的 outbound HTTP/TLS 路径。
> 2026-04-11 补充：高频成功路径 trace（`agent_prepare` 分阶段日志、wake-word feed diag、`[latency][dispatch]` / `[latency][qq_*]` 成功延迟）已统一下调到 `debug`；`info` 口径继续保留给启动关键节点、heartbeat/resource 基线、以及 warn/error 级故障信号。
> 2026-04-11 再补充：runtime/resource 治理已进一步统一。`orchestrator::snapshot()` 新增 `storage_contention_risk`，`self_runtime` idle 作业在 SPIFFS contention 非 Healthy 时必须让行；spawned-thread TWDT 注册统一收口到 `spawn_guarded_with_profile*`；runtime mode source 改为读取真实 owner 状态，不再由 `presence` 等展示层补写。`scripts/check_network_governance.sh` / `scripts/check_runtime_governance.sh` 已进入 CI，直接拦截 transport/raw-state/watchdog/display 绕路。
> 2026-04-11 再补充二：steady-state idle 线程若通过 `spawn_guarded_with_profile*` 被纳入 TWDT，线程体必须在等待/睡眠期间周期性 `feed_current_task()`；当前已覆盖 `bg_timer`、`display`、`wifi_worker` 与 ESP `config_plane_watch`。`voice_session` 对 transient `voice_realtime` worker 的 spawn 失败现已加冷却闸门，避免 `ENOMEM` 下的 100ms 级重试风暴；`runtime::write_back` 对 `spiffs_write` 的 `ENOSPC` 也改为 non-retryable，避免满盘/伪满盘时反复刷盘告警。

---

## 1. 资源上界 / Resource Bounds

### 1.1 队列与消息

| 项目 | 常量 / 位置 | 值 |
|------|-------------|-----|
| 入站/出站队列容量 | `constants::DEFAULT_CAPACITY` | ESP: 16 条；host/Linux: 64 条 |
| 单条消息 content 最大长度 | `bus::MAX_CONTENT_LEN`（同 `constants::MAX_CONTENT_LEN`） | **ESP/RISC-V 嵌入式**：64 KiB；**host/Linux**：256 KiB |
| WebSocket 单条消息 | `channels::websocket::MAX_WS_MESSAGE_LEN` | 同 MAX_CONTENT_LEN |
| WebSocket 最大连接数 | `channels::websocket::MAX_WS_CONNECTIONS` | 4 |

### 1.2 记忆与会话

| 项目 | 常量 / 位置 | 值 |
|------|-------------|-----|
| MEMORY 单次写入最大 | `memory::MAX_MEMORY_CONTENT_LEN` | 256 KiB |
| 单条会话消息最大长度 | `memory::MAX_SESSION_MESSAGE_LEN` | 4 KiB |
| 单会话最大条数（ring） | `memory::MAX_SESSION_ENTRIES` | 128 |

### 1.3 网络与 HTTP

| 项目 | 常量 / 位置 | 值 |
|------|-------------|-----|
| HTTP 客户端 | 由 main 统一注入 HTTP 工厂到各调用链（agent / sender / poll / ws / streaming）。 | — |
| HTTP 请求超时 | `platform::http_client` (REQUEST_TIMEOUT_MS) | 30 s |
| HTTP 响应体最大 | `constants::MAX_RESPONSE_BODY_LEN` | **ESP/RISC-V**：512 KiB；**host/Linux**：2 MiB |
| Handler 内拉取 URL | `HandlerContext::fetch_url` → `platform::fetch_url::fetch_url_with_client` | OTA manifest、`skills` import 等：每次调用 `Platform::create_http_client` 再 GET；**非**主链路注入的共享 `EspHttpClient` 槽位 |
| WiFi 连接超时 | `platform::wifi` (CONNECT_TIMEOUT_SECS) | 15 s |
| OTA 请求超时 | `ota` (OTA_TIMEOUT_MS) | 120 s |
| **HTTP 配置 API 端口** | `platform::http_server` 监听 | 80 |
| **HTTP 配置 API 最大连接数** | `platform::http_server` (MAX_OPEN_SOCKETS) | 4 |
| **POST body 最大** | `platform::http_server` (POST_BODY_MAX_LEN)，按段接口等 | 4 KiB |

### 1.4 LLM 与 Agent

| 项目 | 常量 / 位置 | 值 |
|------|-------------|-----|
| 请求体最大 | `llm::types::MAX_REQUEST_BODY_LEN`（同 `constants::MAX_REQUEST_BODY_LEN`） | **ESP/RISC-V**：512 KiB；**host/Linux**：2 MiB |
| 单条消息内容最大 | `llm::types::MAX_MESSAGE_CONTENT_LEN` | 64 KiB（与 `MAX_CONTENT_LEN` 对齐） |
| 系统提示最大长度 | `agent::context::DEFAULT_SYSTEM_MAX_LEN` | **ESP/RISC-V**：32 KiB；**host/Linux**：64 KiB |
| 消息列表最大长度 | `agent::context::DEFAULT_MESSAGES_MAX_LEN` | **ESP/RISC-V**：24 KiB；**host/Linux**：128 KiB |
| **进 agent 轮前资源门控** | `orchestrator::admission` | 由 orchestrator 四维门禁统一管理：入站消息按压力等级 Accept/Defer/Reject；LLM 调用前检查压力与堆碎片；工具执行前按网络/本地分类门控 |
| **HTTP 准入并发上限** | `constants::MAX_CONCURRENT_HTTP` | 3（TLS 单并发 Mutex + active_http_count 限制） |
| **工具结果回写消息上限** | `constants::MAX_TOOL_RESULTS_USER_MESSAGE_LEN` | 4 KiB |

**运行态能力治理（2026-04 已落地）**：运行态子能力治理已从专项计划转为现行实现，口径统一收口到 `orchestrator::runtime_capability`。当前已落地的首批 sub-capability 为 `audio_output`、`audio_input`、`network.outbound_http`、`storage.state_fs`；`ToolRegistry::tool_specs_for_llm*()` 会按 `ToolCapabilityContract` 对 required capability 做 LLM 暴露过滤，执行前还会再次做 stale gate；agent tool round 对这类拒绝统一返回 `failure_kind=capability` 的结构化 blocker，而不是混成普通 config/error。对外观测上，`/api/health`、heartbeat、board_info、operator status 都从同一个 authority 输出 `runtime_capabilities` 快照或摘要，排障时若四处口径不一致，应视为实现缺陷。

**待重试存储**（阶段七 7-2.2）：两种场景落盘——① 低内存且 inbound 队满时；② LLM 连续失败重试时 inbound 队满时。SPIFFS 单文件 `memory/pending_retry.json`，存一条 PcMsg JSON；循环开始前 drain 一次重试。

**任务延续存储**（阶段七 7-3）：多轮延续状态；SPIFFS 单文件 `memory/task_continuation.json`，单设备单任务，含 chat_id、round、last_output（last_output 上限 4 KiB，由 `TASK_CONTINUATION_MAX_OUTPUT_LEN` 截断）。

**ImportantMessageStore**（阶段六 6-2）：重要消息偏移；SPIFFS 单文件 `memory/important_message.json`，单 chat 单条记录（offset_from_end=1 表示最后一条 user 消息）。当模型回复含 `[MARK_IMPORTANT]` 时固件写入；build_context 截断 messages 时优先保留该条，用后清除。

**EmotionSignalStore**（阶段六 6-17）：情绪信号；当前为内存实现（`MemoryEmotionSignalStore`，HashMap），无持久化。当模型回复含 `[SIGNAL:comfort]` 时固件 set(chat_id, "comfort")；下一轮 build_context 时 get_then_clear 生成 system 后缀（如「用户可能需安慰…」）注入后清除，仅生效一次。

**RemindAtStore**（阶段六 6-10）：到点提醒；SPIFFS 单文件 `memory/remind_at.json`，JSON 数组，按 at_unix_secs 排序。条数上界 `REMIND_AT_MAX_ENTRIES`（32），单条 context 上界 `REMIND_AT_MAX_CONTEXT_LEN`（512 字节）。remind_at 工具写入；独立线程每 60s 调用 pop_due(now)，到点项以 PcMsg「提醒：{context}」注入 inbound_tx 再跑 agent。

**SessionSummaryStore**（阶段六 6-1）：会话摘要；SPIFFS 单文件 `memory/session_summaries.json`，JSON 对象 chat_id -> { summary, last_summary_at_count }，chat 数上界 32，摘要长度上界 `SESSION_SUMMARY_MAX_LEN`（1024 字符）。agent 在回复成功后按消息条数阈值程序性触发摘要更新；build_context 将已有摘要作为独立上下文消息注入，再与 recent messages 一起参与截断。

**LongTermMemoryStore / ExtractionStateStore**（2026-04 主链）：长期记忆与提取状态；SPIFFS 单文件 `memory/long_term_memories.json` 与 `memory/long_term_extraction_states.json`。长期记忆按结构化 `kind/topic/content/keywords/source_chat_id` 存储，带 TTL 治理、slot 复用、冲突清理与 post-reply 程序性提取；提取状态记录 dirty/pending/last_requested_at_count，用于避免每轮都打 LLM。

**2026-04 热路径持久化收口**：`SessionStore` 已支持 `append_batch(...)`，ESP/Linux 平台装配点会统一用 `runtime::write_back` 包裹 `SessionStore / TurnLedgerStore / ExecutionStateStore / SelfModelStore / WorldSenseStore / OuterVoiceStore / AutonomyStrategyStore / InnerLifeStore / SelfContinuityStore / MentalPrivacyStore / LongTermMemoryExtractionStateStore / SessionSummaryStore / ImportantMessageStore`。这些 store 的热路径 mutation 不再立即打 SPIFFS，而是复用现有 delayed task + `bg_timer` 做延迟合批刷盘，**不新增常驻后台线程**。`write_back` 现已把 `ENOSPC`（`os error 28`）视为 non-retryable，防止存储耗尽时 pending map 无限回灌。

**Agent 轮前资源门控**：由 `orchestrator` 模块统一管理。agent loop 收到消息后调用 `orchestrator::should_accept_inbound_pub(channel, chat_id)`，返回 `Accept`（正常处理）、`Defer`（发固定人话出站、重入队或 pending_retry，休眠后继续）或 `Reject`（直接丢弃，如 Critical 压力下的 cron 消息）。LLM 调用前调用 `orchestrator::can_call_llm_pub()`，Critical 压力下降级返回简单响应；Cautious 时检查堆最大连续块是否足够 TLS 握手，不足则延迟重试。工具执行前调用 `orchestrator::can_execute_tool_pub(tool_name)`，Critical 压力下仅允许本地工具（get_time、kv_store 等），拒绝网络工具（web_search、http_post、fetch_url）。

**TLS 碎片风险（2026-04-10）**：`orchestrator::pressure` 现在会把 `heap_largest_block_internal` 折算为 `tls_fragmentation_risk`。当 internal 最大连续块跌破 TLS 线时，pressure 不再继续显示 `Normal`；即便 `heap_free_internal` 还有余量，也视为真实运行态风险。排障时优先联看 heartbeat `resource ... tls_fragmentation=... heap_largest=...`、`/api/resource` 的 `tls_fragmentation_risk` 与 `heap_largest_block_internal`、`metrics.err_tls_admission`，以及 `runtime_capabilities.offline_ids` 中是否出现 `network.outbound_http`。

**HTTP 准入与 TLS 单并发**：所有 HTTP 外出请求最终都经 `network` owner 进入同一条 transport admission 链；低层实现仍通过 `orchestrator::request_http_permit(priority, timeout)` 获取 RAII 令牌。准入流程：① 检查压力等级，Critical 下低优先级直接拒绝；② 检查 active_http_count（上限 `MAX_CONCURRENT_HTTP=3`），超限时低优先级拒绝；③ TLS 单并发 Mutex（try_lock 循环 + 超时 + 喂看门狗）；④ **实时堆检查**（通过 `memory_snapshot_live()` 读取装配期注入的 `Platform::memory_snapshot`，非原子缓存值）。令牌 Drop 时自动递减连接计数。优先级分四级：Low（cron）、Normal（sender）、High（agent LLM）、Critical（健康检查）。出站门禁延迟常量以 `constants` 为准：Critical=1400ms，Cautious=350ms。  
2026-03 多核治理补充：准入前增加线程角色（`Interactive` / `Io` / `Background`）降噪，背景线程在有交互任务时会先短让行，再进入 TLS 互斥竞争，减少“多核并发抢同一锁”导致的体感抖动。
2026-04-10 再补充：`GET /api/channel_connectivity` 属 operator/control-plane 诊断面，ESP 上若 WiFi STA 尚未 settle，或 `tls_fragmentation_risk` 已到 `cautious/critical`，handler 会直接返回 stale/unavailable snapshot；这类“主动跳过 live probe”的结果不应把 `network.outbound_http` 打成 offline。只有真实 sender / tool / channel HTTP 失败才推进 runtime capability failure。
2026-04-11 再补充：业务域不得直接调用 `Platform::create_http_client*`、`orchestrator::current_http_thread_role()`、`orchestrator::request_http_permit()` 或 `begin_wss_session()`；这些 primitive/facade 现在只允许存在于 `src/network/mod.rs`、`src/platform/http_client/**`、`src/channels/wss_gateway/*_conn.rs` 以及统一 spawn wrapper。防回退由 [`scripts/check_network_governance.sh`](../scripts/check_network_governance.sh) 强制执行。

**单轮 agent 峰值 RAM**（估算）：主要贡献项为 HTTP 响应体 1 份（`MAX_RESPONSE_BODY_LEN`，PSRAM 或堆二选一）、build_context 的 system（≤ `DEFAULT_SYSTEM_MAX_LEN`）与 messages（≤ `DEFAULT_MESSAGES_MAX_LEN`）、load_recent 的会话文件 buf + 最近 n 条 `Vec<SessionMessage>`、LLM 请求体（≤ `MAX_REQUEST_BODY_LEN`）、工具结果与 final_content 等。优化后响应体与会话均仅单份（无 PSRAM→堆双缓冲、无整文件 String 与全量 128 条 Vec），数值以 `constants.rs` 与 `memory` 常量为准。

**语音链路补充**：`voice_input` / `voice_output` 不再在工具内部 `create_http_client()`，统一通过 `ToolContext` 复用现有 HTTP/TLS 准入链；执行前后会打印 orchestrator 快照，便于核对语音场景下 `heap_free_internal`、`heap_free_spiram`、`heap_largest_block` 与 `pressure` 的变化。

**realtime 下行播放链路**：当前 ESP realtime 语音播放的真实调用链为
`voice_session::handle_wake_interaction` → `audio::realtime::run_realtime_session` →
`handle_*_server_message` → `queue_output_audio / flush_output_audio / update_playback_state` →
`Platform::write_speaker_pcm_i16` → `platform::audio_drivers::AudioPipelineState` speaker ring →
`audio_io_worker` I2S TX。  
2026-04-11 补充：`voice_session` 现在只保留 scheduler 责任，`external WSS suspend -> realtime WSS connect -> voice-exclusive session` 已迁到 transient `voice_realtime` worker。
当前稳定口径为：`STACK_VOICE_CONTROL=8KB`（scheduler only），`STACK_VOICE_REALTIME=32KB`（ESP）/ `64KB`（Linux rustls）。
2026-04 这一轮之后，这条链还同时带上了两条控制支路：

- **能力支路**：`Platform::audio_duplex_capabilities()` 先给出当前平台真实口径：
  - ESP 当前为 `duplex_without_aec`
  - Linux 当前为 `speaker_only`
  这意味着 Beetle 已有本地 barge-in 控制链，但**还没有** reference/AEC 级回声治理
- **打断支路**：`audio_io_worker` 在 `audio_interrupt_listening=true` 时继续读 mic →
  `wake_word::feed_pcm_i16` 或 realtime 本地能量阈值命中 →
  `orchestrator::request_audio_interrupt()` →
  `run_realtime_session` 本地 `cancel / clear_speaker_buffer / resume listening`
- **退出支路**：`run_realtime_session` 本地维护 `session_ready`、`SpeechEnd`、`post-playback idle` 三组时钟，
  处理“用户没开口”“说完了服务端迟迟不回”“回复播完后还不退出”
- **回合支路**：`run_realtime_session` 现在还维护“有效本地语音提交 + 本地回合代次”：
  - 本地语音未达到 `REALTIME_LOCAL_SPEECH_COMMIT_MIN_MS` 前，不算真正 turn
  - provider 回包 / 音频若未对齐当前本地回合，直接记为 stale drop，不再漏播到 speaker

这一链路的 P0 观测字段不是 `llm_*`，而是 heartbeat `metrics` 行中的：

- `voice_out_tts_http_ms`：TTS / realtime 下行 HTTP 或会话端到可播音频的最近耗时
- `voice_out_play_ms`：最近一次本地播放时长
- `voice_interrupt_req` / `voice_interrupt_accept`：本地打断请求数 / 真正接受数
- `voice_cancel_sent`：向 provider 发出 realtime cancel 的成功次数
- `voice_stale_drop`：因旧回合或未形成有效本地 turn 而被丢弃的 provider 音频/回包次数
- `voice_no_speech_to` / `voice_resp_wait_to` / `voice_post_play_to`：三类退出原因计数
- `audio_spk_write_last_us`：最近一次 speaker I2S 写耗时
- `audio_spk_depth_last`：speaker ring 当前深度
- `audio_spk_depth_min`：本轮播放期间最小深度
- `audio_spk_underrun`：播放中 speaker ring 见底次数

排查口径：

- 若 `audio_spk_depth_last=0` 且 `audio_spk_underrun>0`，优先判定为**下行播放供给/补水策略问题**，不是 LLM 主循环问题。
- 若同时 `spiffs_contention` / `spiffs_hold_last_us` 很高，则要结合 2026-04 write-back 收口后的新日志再看，避免把 SPIFFS 热写误判成播放链唯一根因。
- 若本地已经完成 interrupt 并立刻静音，但服务端迟迟不切到新回合，优先排查 provider cancel/turn 边界，而不是回头再猜 speaker ring。

**工具结果回写上限**：tool use 轮结束后，agent 会把结果以 `"Tool results:\n"` 前缀加结构化 block 的形式回写到下一轮消息中，内部 block 包括 `<tool_result ...>`，LinuxEnhanced 成功轮还可追加 `<tool_evidence_summary>` 与 `<tool_round_guidance>`；若本轮已加载 summary / long-term memory，还会追加紧凑的 `<memory_grounding>`，避免多轮 tool use 后记忆锚点漂走。整条回写消息仍受 `MAX_TOOL_RESULTS_USER_MESSAGE_LEN`（4 KiB）约束；超过上限时按 UTF-8 边界截断，并在尾部补 `[truncated]`，避免单轮 context 被工具结果撑爆。

### 1.5 工具

| 项目 | 常量 / 位置 | 值 |
|------|-------------|-----|
| 工具参数最大长度 | `tools::MAX_TOOL_ARGS_LEN` | 8 KiB |
| 工具返回值最大长度 | `tools::MAX_TOOL_RESULT_LEN` | 16 KiB |

### 1.6 配置存储（NVS + SPIFFS）

| 项目 | 说明 |
|------|------|
| NVS | 仅存 6 个小键（`wifi_ssid`、`wifi_pass`、`proxy_url`、`session_max_messages`、`tg_group_activation`、`locale`）；配对码等仍用 NVS。减少单次写入量与键数以降低 ESP_ERR_NVS_INVALID_STATE (4361) 风险；遇 4361 时单次 recover+重试。 |
| SPIFFS 配置 | `config/llm.json`（LLM 多源与路由/worker 下标；每项可含 `stream: bool` 与 `max_tokens: u32`）、`config/channels.json`（通道相关）、`config/skills_meta.json`（技能顺序与禁用列表）。GET /api/config 合并 NVS 与上述文件后返回；config_reset 会删除这三文件。首次启动时文件不存在属正常情况，**不会**产生 `load_errors`；仅 IO 错误（文件存在但读失败）才记为错误。供 LLM 工具链读取时，`config/llm.json` / `config/channels.json` / `config/wifi.json` 允许访问，但敏感字段值会在工具返回前用正则脱敏，模型永远看不到真实秘钥。 |

### 1.7 其他

| 项目 | 说明 |
|------|------|
| PSRAM / 栈 | 与 C 版量级接近；具体以 sdkconfig 与目标板为准。大 buffer 使用 PSRAM。 |
| Skills | 见 `src/skills/mod.rs`：单条内容 ≤ 32 KiB，最多 64 条。 |

### 1.8 大 buffer 与 PSRAM（ESP32-S3）

HTTP 响应体在有 PSRAM 时由 `ResponseBody` 持有、仅存于 PSRAM（无堆拷贝）；LLM 请求/响应等其余大块内存由默认分配器分配。是否使用 PSRAM 由 **sdkconfig** 控制（见 `sdkconfig.defaults.esp32s3`）：

- `CONFIG_SPIRAM=y`：启用 PSRAM。
- `CONFIG_SPIRAM_MALLOC_ALWAYSINTERNAL=32`：仅 ≤32B 的最小普通分配优先内部 SRAM；更大的默认堆分配更倾向走 PSRAM，尽量把 internal 连续块留给 TLS / WSS / DMA。
- `CONFIG_SPIRAM_MALLOC_RESERVE_INTERNAL=102400`：预留 100KB internal 给 internal-only / DMA / TLS 相关分配，避免 PSRAM 板子把关键 internal 池提前吃穿。

**所有权约束（2026-04-10 修订）**：`heap_caps_malloc(MALLOC_CAP_SPIRAM)` 返回的原始指针**不得**直接交给标准 `Vec::from_raw_parts(...)` 托管；SPIFFS 读缓冲应通过 `PsramVec<u8>` 保持显式释放，HTTP 响应体应通过 `ResponseBody::PSRAM` 持有。若需要 `Vec<u8>`，必须走显式复制/转换边界，而不是把 PSRAM 指针伪装成默认分配器对象。

**建议**：当前默认回到 `ALWAYSINTERNAL=32B` 基线，只让最小对象留在 internal，把连续大块优先保给 TLS / WSS / DMA。若后续仍观察到启动峰值碎片化，优先收窄常驻线程栈或启动次序，而不是继续把 `ALWAYSINTERNAL` 往上抬。

**路线约束**：当前生产方案固定为 `mbedTLS + esp-tls`，本轮优化不迁移 wolfSSL。优先优化语音路径中的内存复制与连接复建，保持现有 orchestrator/TLS 准入闭环。

### 1.9 低内存预设说明

当前仓库以 `src/constants.rs` 为单一真值，未维护独立 `small_memory` feature 分支常量。若后续引入低内存构建档位，需先在 `constants` 与本文件同步定义并给出验证基线，避免文档口径漂移。

### 1.10 看门狗与长请求

LLM/HTTP 单次请求可能长达数十秒。当前口径：**凡是通过 `spawn_guarded_with_profile*` 拉起的线程，都会在 wrapper 内统一注册/反注册 TWDT**，包括 `agent_main_loop`、WSS 网关循环、sender、voice worker 等；主线程只保留少数非 wrapper owner 的显式注册点。**每次 HTTP 请求前**仍会喂任务看门狗（仅 ESP 目标），API 由 build 时自动选择：**build.rs** 根据 `IDF_PATH/version.txt` 解析主版本，IDF 4.x 用 `esp_task_wdt_feed()`，IDF 5.x 用 `esp_task_wdt_reset()`。若启用 TWDT，建议超时 ≥「HTTP 超时 + 重试」总时间（例如 ≥ 60s）。

- 2026-04-11 补充：steady-state 非 HTTP 线程也不能“只注册不喂狗”。凡是可能 `sleep` / `recv_timeout` / `Condvar::wait_timeout` 的长生命周期线程，必须在等待窗口里周期性喂狗；`bg_timer` 现以 5s slice 分段等待，`display` / `wifi_worker` / ESP `config_plane_watch` 在各自 idle loop 中固定 feed。
- 2026-04-11 补充：`voice_session` scheduler 不再在 `voice_realtime` spawn 失败后每 100ms 重试；当前会进入固定冷却窗口，只保留一个 pending wake 任务，避免内存不足时刷爆日志与 pthread 创建路径。

### 1.11 启动自检

- **必须**：storage 可读（`get_memory` 或 `get_soul` 至少其一成功），否则不启动业务任务。
- **首次/空存储**：若两者均失败，启动时先写入占位 memory/soul（空内容），再执行自检，通过后进入正常流程（可引导用户配置或后续写入真实数据）。
- **可选（仅打日志）**：自检通过后打一条汇总：`wifi=` connected/disconnected，`spiffs=` 剩余字节或 N/A（`platform::spiffs_usage()`）。失败不阻塞启动。

### 1.12 HTTP 配置 API（feature = config_api）

- **ESP**：IDF `httpd`；**Linux/host**：`tiny_http`，监听地址由环境变量 **`BEETLE_CONFIG_HTTP_LISTEN`** 覆盖，默认 **`0.0.0.0:80`**（与 ESP 默认口径一致）。
- **端口**：默认 80（特权端口需部署侧权限）；SoftAP 与 STA 下均可访问（以实际绑定地址为准）。
- **最大连接数**：2～4（`MAX_OPEN_SOCKETS`），与 1.3 表一致。
- **POST body 最大长度**：4 KiB（按段保存等 POST 请求）。
- **仅局域网**：不暴露到公网，用户连设备热点或同 LAN 访问。
- **CORS**：所有 `/api/*` 及 GET / 的响应应带 `Access-Control-Allow-Origin: *`；OPTIONS 预检对约定路径返回 200 并带 `Access-Control-Allow-Methods`、`Access-Control-Allow-Headers: Content-Type`。详见 `src/platform/http_server/` 相关 handler 与路由实现。
- **无内嵌/SPIFFS 静态资源**：固件不提供 HTML/JS/CSS；配置页外置。

### 1.13 NVS 配置

- **命名空间**：`config::NVS_NAMESPACE` = `pc_cfg`。
- **键名长度**：ESP-IDF 规定 NVS 键名最多 **15 字符**，`platform::nvs::NVS_KEY_MAX_LEN` 与之一致；键名过长的配置项使用缩写（如 `sess_max_msg`、`tg_grp_act`）。
- **NVS 仅 6 键**：`NVS_ALL_KEYS` 为 wifi_ssid、wifi_pass、proxy_url、sess_max_msg、tg_grp_act、locale；LLM 与通道配置已迁至 SPIFFS（见 1.6），减少写放大与 4361 风险。
- **单值最大长度**：与 NVS 底层约定一致（典型 512 字节）；见 `platform::nvs::write_string`。遇 `ESP_ERR_NVS_INVALID_STATE` (4361) 时单次 recover+重试。
- **安全**：配置 API 与 CLI 返回完整配置（含密钥），仅在受控环境使用；GET /api/config 与 `config_show` 输出完整 config。

### 1.14 通道能力矩阵 / Channel Capabilities

与 OpenClaw 对齐：群组触发、ACK reaction、typing、长回复分片、SILENT 不发送。**SILENT** 由 `channels::dispatch::run_dispatch` 统一处理（`content.trim() == "SILENT"` 则不调用 sink），所有通道一致。

| 能力 | Telegram | Feishu | 钉钉 | 企业微信 | WebSocket |
|------|----------|--------|------|----------|-----------|
| 入站 | 有（long poll） | **降级**：入站未实现，待日后实现 | **降级**：无 | **降级**：无 | **降级**：无 |
| 出站 | 有 | 有 | 有（自定义机器人 Webhook，按 4096 分片） | 有（应用消息，按 2048 字节分片） | 有（当前仅打日志，可扩展真实 WS） |
| 长消息分片 | 4096 字符，后续条 reply 首条 | 4096 字符，顺序多条 | 4096 字符，顺序多条 | 2048 字节，顺序多条 | **降级**：按 `MAX_WS_MESSAGE_LEN` 单条，不分片 |
| ACK reaction | 入站后 setMessageReaction | **降级**：无（依赖入站与 message_id） | **降级**：无 | **降级**：无 | **降级**：无 |
| Typing | sendChatAction(typing)，401 退避 | **降级**：无（依赖入站） | **降级**：无 | **降级**：无 | **降级**：无 |
| 群组 mention/always | 有，system 注入 + 门控 | **降级**：入站未实现 | **降级**：无 | **降级**：无 | **降级**：无 |

- **Feishu**：出站已按 4096 字符分片；入站、ACK、typing 待入站实现时再补。
- **钉钉**：出站为自定义机器人 Webhook，按 4096 字符分片；首版不加签；入站无。
- **企业微信**：出站为应用消息（gettoken + message/send），按 2048 字节 UTF-8 安全分片；入站无。
- **WebSocket**：协议可自定义，当前无 reaction、无 typing；长消息不分片，由上层 `MAX_CONTENT_LEN` 限制单条。

### 1.15 ESP32-S3 多核线程拓扑（固定口径）

`main.rs` 中线程规划为单一权威：

- **Core0（网络 IO）**：`wifi_worker`、`feishu_ws`、`qq_ws`、`tg_poll`、`dispatch`、`*_sender`、`http_server`
- **Core1（agent + background）**：`agent_main_loop`、`display`、`cron`、`heartbeat*`、`remind`、`cli_repl`

角色口径：仅 `agent_main_loop` 使用 `Interactive`；其余 Core1 线程统一 `Background`，Core0 网络线程统一 `Io`。主线程职责为监管 `agent_main_loop` 生命周期，异常退出时触发统一恢复（记录 + `request_restart`）。

### 1.16 ESP32 运行态治理口径（吸收 xiaozhi 后）

与 `dev/xiaozhi-esp32_AEC_xiaolong/main/application.cc`、`main/background_task.cc`、`main/protocols/websocket_protocol.cc` 对照后，Beetle 的下一层治理重点不再是“再省几 KB”，而是**运行态拓扑收口**：

- **模式切换优先于资源争抢**：新增 ESP 交互/联网能力时，先定义 steady-state mode、切换前置条件、挂起/恢复边界，再考虑 `orchestrator` admission；不得只靠 permit、重试、延时去兼容并发共存。
- **常驻重执行面预算固定**：ESP 运行态只允许一个 `agent_loop` 作为常驻 LLM/TLS 重执行面；后台自治、维护、`self_runtime` 等作业必须在同一 agent plane 串行消费，不再恢复双 agent lane。
- **控制面不是长期主面**：`config_plane` 属 bootstrap / recovery plane；默认不应扩成长期常驻重服务。新增配置/诊断平面必须证明其 steady-state 成本、可暂停性或可驱逐性。
- **语音优先级高于外部长连**：外部 WSS / IM 长连是从属 plane；进入 `voice-exclusive` 前先完成 suspend，退出后再 resume，禁止边切模式边抢 TLS。
- **编译期预算服务于运行态拓扑**：`sdkconfig` 中 internal reserve、task stack、WiFi/LwIP 预算的调整，必须对应到具体 plane / 线程 / 模式切换收口；不得用更大的保守值长期掩盖拓扑过宽。
- **2026-04-11 运行态补充**：`voice_realtime` 的 `ENOMEM` 需要优先对照 heartbeat 的 `heap_largest`，不是只看 `heap_internal` 总量。实机出现 `heap_largest=31744` 而 `STACK_VOICE_REALTIME=32768` 时，先收紧常驻 WSS 线程的共享栈预算（当前 `STACK_CHANNEL_WS` 已从 16KB 收到 12KB），而不是为单一通道再开特判常量。

---

## 2. 超时与重试默认值 / Timeouts and Retry Defaults

| 场景 | 默认值 | 说明 |
|------|--------|------|
| WiFi 连接 | 15 s | 超时后返回 Error，不阻塞 |
| HTTP 单次请求 | 30 s | GET/POST 共用 |
| OTA 下载 | 120 s | esp_https_ota 用；失败不写当前分区，可重试或换 URL |
| Cron 推送间隔 | 60 s | `cron::DEFAULT_CRON_INTERVAL_SECS` |
| Heartbeat 周期 | 30 s | main 中传入 |
| LLM | 随 HTTP 客户端 | 可配置重试与退避（Anthropic 实现） |
| 通道轮询 | 实现内 | 失败带退避，不 panic |

---

## 3. 长时间运行 / Long-Run Stability

建议在目标板上长时间运行（如 24h+）验证无 OOM、栈溢出与异常重启。可结合 heartbeat 日志与 `cli health` 观察队列与最近错误。

---

## 4. 可观测清单 / Observability Checklist

与 `cli health` 及 MIGRATION_PLAN 2.6 对应。

### 4.1 CLI `health` 命令输出（feature = cli）

| 项 | 说明 |
|----|------|
| wifi | connected / disconnected（由 main 传入的 wifi_connected） |
| inbound_depth | 入站队列深度近似值（bus 不暴露时为 N/A） |
| outbound_depth | 出站队列深度近似值（同上） |
| last_error | 最近一次错误摘要（无密钥；由 `state::set_last_error` 设置，CLI 与 GET /api/health 共用） |

### 4.2 审计日志（无敏感信息）

| 命令/操作 | 日志内容 |
|-----------|----------|
| session_clear | AUDIT: session_clear chat_id=xxx |
| memory_write | AUDIT: memory_write len=xxx |

### 4.3 周期日志

| 来源 | 内容 |
|------|------|
| heartbeat | HEARTBEAT version=… uptime_secs=… heap_free=…（ESP32） |
| cron | 每间隔推送一条入站消息；发送失败打 warn 并退避 |

基线日志中的 `metrics` 行包含 `err_tls_admission`，用于直接观测 TLS 准入失败趋势并与 `resource.heap_internal/heap_largest` 对照调参。

### 4.3.0 LLM Turn Pipeline 延迟口径

2026-04 起，LLM 从入站到回复的主链延迟不再只看一条总耗时，而按五段固定口径拆分：

- `queue_wait_ms` / `admission_ms`：属于 `IngressAdmission`
- `worker_prepare_ms`：属于 `TurnPrepare`
- `context_ms + llm_round_total_ms + tool_exec_ms`：属于 `TurnExecution`
- `session_write_ms` 与 `TurnLedger` / `mental_privacy_review` 收尾：属于 `ReplyFinalize`
- `outbound_enqueue_ms` / `reply_handoff_ms`：属于 `DeliveryHandoff`

`ReplyFinalize` 是 canonical final reply 的唯一权威；delivery report 只记录交付行为，不拥有最终回复语义。排查慢回合时，必须先判断卡在 admission、prepare、execution、finalize 还是 handoff，而不是把所有尾延迟都混成 “agent 慢”。

### 4.3.1 Runtime Capability 观测口径

2026-04 起，运行态能力治理已经进入现行真源，不再单独依赖计划文档。当前对外观测口径固定如下：

- `GET /api/health`：返回 `runtime_capabilities` 全量快照
- `operator_status`：返回 `runtime_capabilities`，并给出 offline / degraded 摘要
- `board_info`：返回 `runtime_capabilities` 摘要，供设备级快速排障
- heartbeat：输出单独的 `runtime_capabilities offline=... degraded=...` 基线行

正确实现要求这些口径全部来自同一 authority 快照，而不是在各处重新 probe。若同一 sub-capability 在四个面上状态不一致，优先检查 `src/orchestrator/runtime_capability.rs` 的状态推进与调用链接线。

2026-04-10 之后，`network.outbound_http` 的推进口径固定为：

- 启动 / heartbeat 只负责更新 transport readiness（LinuxFull / ESP Wi-Fi STA 就绪）
- 真实 HTTP 成功通过 `observe_runtime_capability_success([network.outbound_http])` 把 capability 拉回 `Online/Nominal`
- 真实 HTTP 失败若属于 `tls_admission`、连接失败或 retryable upstream，则通过 `observe_runtime_capability_failure(..., UpstreamUnavailable)` 保持 offline，直到下一次真实成功
- 因此若日志里已经出现 sender / token / gateway 的 HTTP 成功，但 heartbeat 仍把 `network.outbound_http` 打成 offline，应直接视为实现缺陷

2026-04-10 这一轮再补四条硬口径，后续排障与新代码都按这个口径审查：

- 根因分类必须看 `Error::metrics_stage()`，不是只看最外层 `stage()`；凡是 wrapped `tls_admission` / connect error / retryable upstream 被错误归类成普通 `other`，都视为观测链断裂。
- 统一出站 HTTP 观测必须覆盖 sender、gateway、token refresh、typing/reaction、stream editor、direct send/edit 等所有真实 transport 路径；只要有一条旁路没接 capability/metrics，就会把 runtime capability 打回“看起来在线、实际盲飞”的假稳态。
- cooldown / retry buffer 的 replay 必须依赖调度时钟，不依赖下一条新消息刺激；否则系统处于“有 backlog 但无新流量”时会卡成假死。
- 入站群聊语义必须在 `PcMsg` 构造点保真，不能在上层解析后又退回 `PcMsg::new(...)` 丢掉 `is_group`；QQ group 和钉钉 `conversationId` 已按这个口径收口。

### 4.3.2 音频播放诊断字段

语音断续阶段，优先看下面几项组合，而不是先猜网络或先猜 LLM：

| 字段 | 含义 | 判断方式 |
|------|------|----------|
| `audio_spk_depth_last` | 当前 speaker ring 深度（samples） | 长期接近 0 说明下行补给跟不上消费 |
| `audio_spk_depth_min` | 本轮播放期间最小深度 | 为 0 基本等价于播放中见底 |
| `audio_spk_underrun` | speaker ring 见底累计次数 | 递增说明存在可感知断续 |
| `audio_spk_write_last_us` | 最近一次 I2S TX 写耗时 | 若持续异常大，再看 I2S / worker 调度 |
| `voice_out_play_ms` | 最近一次播放时长 | 为 0 或长期过短，说明音频没有稳定下到本地播放面 |
| `voice_interrupt_req/accept` | 本地打断请求 / 接受次数 | `req` 增长但 `accept` 不动，优先排查 interrupt 链是否被能力口径或状态机挡住 |
| `voice_cancel_sent` | realtime cancel 成功次数 | `accept` 增长但 `cancel` 不动，优先查 provider cancel 发送链 |
| `voice_stale_drop` | stale provider 音频/回包丢弃次数 | 增长本身不一定是 bug，但能直接证明本地 turn fencing 在工作 |
| `voice_no_speech_to` | 无有效首句语音退出次数 | 用户没说话仍挂着不退时先看它是否增长 |
| `voice_resp_wait_to` | 等服务端首个有效响应超时次数 | 说完后 provider 迟迟不回时应增长 |
| `voice_post_play_to` | 打断后或播完后的本地 idle 退出次数 | 打断后没继续说话却不退出时先查它 |
| `wake_feed_busy_skip` | 非 interrupt window 下播放/录音冲突时 wake feed 被跳过次数 | 上升本身不等于 bug，但说明系统在主动做 mode 隔离 |

当前建议的诊断顺序：

1. 先看 `audio_spk_depth_last/min` 与 `audio_spk_underrun` 是否直接证明 ring starvation。
2. 再看 `voice_out_play_ms` 与本地 interrupt 行为是否符合预期，区分“本地没播稳”还是“本地已静音但服务端 turn 还没切换”。
3. 若用户没说话或说完后迟迟不退出，优先检查 realtime 本地三段时钟：`session_ready -> first speech`、`SpeechEnd -> first server activity`、`playback finished -> post-response idle`。
4. 最后再看 `spiffs_*`、`voice_out_tts_http_ms`、`channel_http_*`、`active_wss`，判断是不是别的 plane 抢资源。

### 4.5 StreamEditor 连接复用口径

- `StreamEditor` 的 HTTP 客户端采用 **agent 线程内 `thread_local` 槽位**复用，禁止跨线程共享非 `Send` 客户端。
- 运行日志新增 `stream_http_stats` 行，字段包含：`ops`、`reuse_hits`、`creates`、`resets`、`invalidates`、`reuse_rate`。
- 触发策略：
  - 每 `STREAM_HTTP_STATS_LOG_EVERY` 次操作输出一次统计；
  - 连接失效时输出 `stream_http invalidate reason=...`；
  - 轻恢复时输出 `stream_http reset_for_retry reason=...`。
- 验收口径：同负载下 `creates` 相比“每次编辑新建连接”显著下降，且 `invalidates` 不持续攀升。

### 4.4 错误 stage 与排查 / Error Stages and Troubleshooting

所有公共 API 返回 `Result<T, Error>`；错误带 `stage`。下表列出当前代码中使用的 stage、出现场景与排查建议。通过串口执行 `health` 可获得 `last_error` 摘要，与下表对照排查。**不输出密钥**。

| stage | 出现位置/场景 | 排查建议 |
|-------|----------------|----------|
| `config` | 配置校验失败（如 wifi_ssid 空、长度超限、proxy 只填一端） | 检查环境变量或编译时配置；长度与格式见 config 校验逻辑 |
| `spiffs_read` | SPIFFS 读文件（open/read）失败 | 检查路径是否存在、挂载是否成功 |
| `spiffs_write` | SPIFFS 写文件失败或写入超 MAX_WRITE_SIZE | 检查分区空间、单次写入 ≤ 256KB |
| `spiffs_list` | SPIFFS 列目录失败 | 检查路径、挂载 |
| `spiffs_register` | esp_vfs_spiffs_register 失败（esp 变体） | 检查分区标签、NVS/SPIFFS 初始化顺序 |
| `wifi_connect` | WiFi 连接超时或失败 | 检查 SSID/密码、信号、validate_for_wifi |
| `wifi_peripherals` / `wifi_new` / `wifi_wrap` / `wifi_set_config` / `wifi_start` / `wifi_wait_netif` / `wifi_nvs` / `wifi_event_loop` | WiFi 底层初始化各步骤 | 检查 ESP-IDF 网络栈、NVS |
| `http_client_new` | EspHttpClient::new 失败 | 检查网络/依赖 |
| `tls_admission` | HTTP/WSS 准入被拒（压力、并发、TLS 锁超时、internal/largest_block 不足） | 对照 `/api/health` 的 `metrics.errors_tls_admission` 与 `resource.heap_*`，优先检查 internal 连续块与并发 |
| `proxy_connect` | 配置了代理但 CONNECT 未实现或失败 | 当前代理为占位；检查 proxy 配置或暂时清空 |
| `http_get_request` / `http_get_submit` / `http_post_request` / `http_post_write` / `http_post_flush` / `http_post_submit` | HTTP 请求各阶段失败 | 检查 URL、网络、超时 30s、响应体 ≤ 512KB |
| `http_read` | 读响应体失败 | 检查连接与大小限制 |
| `llm_request` | LLM 请求发送或 4xx/5xx | 检查 API URL、网络、key 是否有效（不打印 key） |
| `llm_parse` | LLM 响应 JSON 解析或 choices 缺失 | 检查响应格式是否与实现一致、body 是否截断 |
| `telegram_poll` | Telegram getUpdates 请求失败 | 检查 token、网络 |
| `telegram_parse` | getUpdates 响应 JSON 解析失败 | 检查 API 返回格式 |
| `telegram_send_queue` | 出站队列 send 失败 | 队列满或 channel 关闭；检查 dispatch 与容量 |
| `feishu_send_queue` | 飞书出站队列 send 失败 | 同上 |
| `dingtalk_send_queue` | 钉钉出站队列 send 失败 | 同上 |
| `wecom_send_queue` | 企业微信出站队列 send 失败 | 同上 |
| `tool_web_search` | Brave 搜索 API 调用失败 | 检查 search_key、URL、响应大小 |
| `tool_execute` | 工具执行失败（如 args 超限） | 检查参数长度 ≤ 8KB、工具实现 |
| `agent_session` | 会话追加或加载失败 | 检查 SessionStore、路径与长度限制 |
| `io` | 通用 IO 错误（From<std::io::Error>） | 见具体调用处的 stage |
| `esp` | ESP-IDF 返回错误码 | 见具体变体与 code |
| `http` | HTTP 状态码错误（带 status_code） | 见具体 stage（如 llm_request、telegram_poll） |

健康检查与审计**不输出密钥**；`last_error` 仅包含 stage 与安全摘要。
