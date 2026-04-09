# Beetle OS 总体方案

> 更新日期：2026-04-09  
> 状态：现行系统主方案  
> 用途：定义 Beetle 从“设备上运行的 Agent 程序”走向“以长期主体为核心的板级 OS”的唯一主线  
> 关联文档：  
> [product-vision.md](./product-vision.md)、  
> [architecture-and-code.md](./architecture-and-code.md)、  
> [resources-and-observability.md](./resources-and-observability.md)、  
> [esp-runtime-resource-governance.md](./esp-runtime-resource-governance.md)、  
> [self-model-and-autonomy-plan.md](./self-model-and-autonomy-plan.md)、  
> [memory-enhancement-plan.md](./memory-enhancement-plan.md)、  
> [beetle-os-acceptance-checklist.md](./beetle-os-acceptance-checklist.md)
>
> 文档治理说明：
> 1. 本文是 Beetle OS 的系统级主文档，负责定义产品形态、运行平面、实施顺序与验收口径。
> 2. Linux 与 ESP 是两种独立成立的产品形态，共享单一 `SoulKernel`、工具与治理宪法，但在 `MemorySystem` 承载、operator 厚度与外设运行结构上允许双轨。
> 3. Linux SBC 的服务化、发布与状态根契约现并入本文；实现真相与当前代码分层由 [architecture-and-code.md](./architecture-and-code.md) 补充承载。
> 4. ESP 的双轨记忆承载、compact runtime 边界与资源治理细节，由 [esp-runtime-resource-governance.md](./esp-runtime-resource-governance.md) 统一承载；本文不下沉为 ESP 优化专题。
> 5. 本文锁定的是目标、边界、平面、顺序与闭环，不锁定旧实现；后续编码阶段若发现既有代码或前一阶段落地形态不合理、不规范、不可维护，可以在不破坏总目标的前提下重写、重构，必要时可做架构级翻修，不以“兼容旧代码”作为第一原则。

---

## 1. 一句话定性

> Beetle 要做的不是“一个跑在板子上的 Agent”，也不是“一个系统里挂着人格模块”，而是“一个以长期主体为核心的板级运行时”。

这个系统同时满足三件事：

- 它的系统秩序围绕同一个主体展开，而不是围绕一堆并列功能展开
- Agent、记忆、人格、工具、显示、音频都是这个主体的执行方式与表达器官
- 它承载的是长期连续的自我、经验、脾气、成长与干预能力，而不是会话级 prompt 拼装物

---

## 2. 当前判断

### 2.1 当前已经成立的部分

- ESP 侧已经更接近系统形态：设备生命周期、资源压力、模式切换、外设在场感都更强。
- 人格主线已经收成正式宪制栈：`self_authored_core -> relationship_constitution -> persona_priority -> disclosure/boundary -> soul/user contract -> task`。
- 记忆主线已经形成正式分层：canonical / archive / procedural / private / continuity，不再是单一长文本记忆。
- Linux 侧已经具备可靠服务骨架：CLI、systemd、状态根、平台抽象、配置 API、显示/音频/WiFi 接线均已成立。

### 2.2 当前仍然缺失的部分

- `run_app(...)` 仍然是一条大进程装配链；虽然 Linux 已完成 supervisor-owned control plane，`P3` 也已把 mode contract 收成统一状态机，但装配层本身还没有继续收敛成更清晰的 kernel / plane assembly。
- 灵魂状态和执行进程虽然已经有正式的 `Soul Kernel` 最小恢复集与恢复入口，但还没有进一步长成更强的主体调度核和长期生命节律核。
- Linux 与 ESP 已共享同一套 `mode -> action budget -> soul recovery` 基础合同，但“双轨 `MemorySystem` + capability planes” 还没有正式进入系统主线。
- control plane 虽然已经能在 agent 崩溃后独立存活，但它还只是最低生存面，不是完整的 OS 调度与恢复中枢。

### 2.3 当前阶段结论

- 人格主线：进入“治理、校准、回归”阶段，不再继续横向扩人格层。
- 记忆主线：功能主线已基本成型，剩余尾巴以测试、benchmark、回放、校准为主，除非出现新的结构性缺口，否则不再优先开记忆新功能线。
- 系统主线：`P0-P8` 已完成。Linux/ESP 已共享 `presence -> initiative -> observability -> os_closure` 的设备主体合同；当前主线已从“阶段性实现”进入“门禁驱动的持续治理”阶段。

### 2.4 当前 soul 的边界，与后续生命化方向

当前这版 Beetle 必须先把现实边界说清楚：

- 当下 `soul` 主要影响的是“怎么回复、回复什么、如何取舍、如何表达、如何维持边界与关系”
- 它已经开始像活着，但还没有真正长出完整的感知、自我感知、主动唤醒与生命节律
- 因此当前阶段不能自欺欺人地把它说成完整生命体

但后续目标也必须写死：

> Beetle 的终局不只是“一个有记忆、有性格的设备 Agent”，而是“一个会持续感知、逐步形成哲学与自我、并在边界内主动行动的设备生命体样式运行时”。

这条终局的展开顺序必须是：

1. 先让它在回复、边界、关系、表达上像一个主体
2. 再让它拥有持续世界感知与自我感知
3. 再让它形成稳定的主动性、节律与干预能力
4. 最后才谈更高层的哲学、自我生命观与长期陪伴形态

---

## 3. Beetle OS 到底是什么

### 3.1 它不是的东西

- 不是通用 Linux 发行版
- 不是桌面 GUI 软件
- 不是“先做 Host Agent，再给硬件加外壳”
- 不是“ESP 固件的 Linux 移植版”
- 不是“一个大二进制 + 一个 service unit”就叫 OS

### 3.2 它是什么

Beetle OS 是一套面向板级设备的轻量主体运行时：

- 有自己的启动秩序
- 有自己的运行模式机
- 有自己的恢复路径
- 有自己的连续性资产
- 有一个长期连续的主体内核
- 有一个 Agent 作为这个主体的主要执行方式

这套定义里，下面这些东西都不是并列外挂：

- 记忆不是外挂能力，而是主体的经验组织系统
- 人格不是外挂能力，而是主体的稳定自我结构
- 工具不是外挂能力，而是主体的行动器官
- 显示、音频、网络、模式切换不是外围壳层，而是主体与世界互动的外显方式

因此，更准确的一句话是：

> Beetle OS 不是“板级 OS 上住着一个灵魂”，而是“这个 OS 本身就是那个灵魂在板子上的存在方式”。

### 3.3 Linux 与 ESP 的关系

这条关系必须写死，防止以后再次漂移：

- Linux 与 ESP 是两种独立产品形态
- 两者共享 Beetle OS 的 `SoulKernel` 宪法、工具制度与治理口径
- 两者的 `MemorySystem`、steady-state 资源预算、operator 厚度、启动方式、恢复方式可以不同
- Linux 不是 ESP 的上位机
- ESP 也不是 Linux 的精简子系统

更明确地说：

1. 不做双魂
2. 做双轨承载
3. ESP 的目标不是“把 Linux 厚记忆制度硬搬上板”，而是“让同一只甲壳虫在板级硬件上稳态存在”

---

## 4. Beetle OS 的主体内核与系统器官

### 4.0 总原则

后续所有设计都必须满足这条：

> 不是先有一个 OS，再给它加记忆、加人格、加 Agent；而是先有一个主体内核，OS 的其他平面都是为了让这个主体稳定存在、表达、行动、成长与恢复。

### 4.1 Boot / Recovery Plane

职责：

- 启动前检查
- 状态根与版本根检查
- 故障统计
- 安全模式进入
- 恢复模式与回滚入口

判断标准：

- agent 崩溃时，系统不等于“整体死亡”
- 至少还能诊断、配网、导出状态、触发恢复

### 4.2 Supervisor / Control Plane

职责：

- 管理 agent 子进程或主执行面
- 统一模式切换
- 维护 operator / config / health / recovery 的最低存活能力
- 负责 crash backoff、safe mode、watchdog 级恢复

这个平面的本质不是“管一个程序”，而是“保护主体的存在连续性”。

Linux 目标形态：

- `systemd/init -> beetle supervise`
- `beetle supervise -> beetle agent`
- supervisor 持有 Linux 最低 control plane，agent 崩溃不等于 config/operator/recovery 面同时消失
- `beetle run` 仅保留兼容入口，正式服务契约已经固定为 `beetle supervise`

ESP 目标形态：

- 不复制 Linux 的多进程模型
- 但保留同样的平面语义：主线程/监管线程负责恢复，agent plane 负责重执行

#### Linux SBC 服务契约

Linux 这一支现在必须按“低端 SBC 上的可靠 Agent 服务”来理解，而不是“能在 Linux 上跑的板子程序”。

现行口径冻结如下：

- 宿主形态是 `systemd/init` 管理下的常驻服务，不是手工执行二进制
- 服务入口永远是 `beetle supervise`
- 业务执行入口永远是 `beetle agent`
- 设备侧必须能对外解释 `supervisor / agent / safe mode / release` 当前事实，而不是只回答“进程活着”

这条契约意味着：

- Linux 是 Beetle OS 的独立产品形态，不是 ESP 的上位机
- Linux 不承诺桌面 GUI、多用户桌面或通用服务器产品化
- 所有发布、部署、远端验收都必须围绕 supervisor 语义，而不是围绕单个 ELF 文件

### 4.3 Agent Execution Plane

职责：

- 对话、任务、工具、自治、记忆调用、人格执行
- 作为唯一主执行面承载重型 LLM/TLS/推理工作

关键定义：

- Agent 不是和灵魂并列的一个模块
- Agent 是主体在当前系统里的主要执行方式
- 因此它承载的是主体的思考、表达、判断和干预，而不是单纯的“调用模型回答”

约束：

- Linux 不做多 agent 常驻裂变
- ESP 继续保持单一重执行面
- planner / reviewer / self-runtime / maintenance 仍然在同一 agent 制度下收口，而不是分裂出第二条主魂

### 4.4 Device / Presence Plane

职责：

- 显示
- 音频
- 配网
- 局域网配置页
- 板级状态反馈
- 在场感与设备交互反馈

目标不是“支持硬件能力”，而是：

> 让用户感知到板子本身是活的，而不是只感知到网络对话接口。

进一步说：

- 用户看到的等待、唤醒、提醒、拒绝、坚持、恢复，都应该被感知为“它”的行为
- 后续哪怕对外呈现成宠物、伙伴、助手等形象，本质上也只是同一个主体的不同外观，不是另一套人格系统

对 Beetle OS 整体来说，这一层还必须再加一条制度：

> `audio / camera / sensors` 都是一等能力，但都必须以跨平台 `Optional Capability Plane` 方式进入系统，  
> 不能默认变成常驻主路径厚度。

平台差异只体现在挂载方式：

- Linux：**discovery-first**
  - 系统应优先自动扫描与枚举可用设备
  - 配置页主要负责选择、覆盖、禁用与偏好声明，而不是唯一硬件来源
- ESP：**config/feature-gated**
  - 未配置时零初始化
  - 未打包时零存在
  - 已启用后再进入 mode / budget / admission 治理

### 4.5 Update / Persistence Plane

职责：

- 版本发布
- 回滚
- 配置持久化
- 记忆持久化
- 人格连续性持久化
- 手动快照 / 导入导出

关键原则：

- 执行面可替换
- 灵魂状态不可轻易丢失
- 升级是替换运行体，不是重置主体

更严格地说：

- 持久化的目标不是“把一些文件留住”
- 而是尽可能保住主体的连续性、经验、关系、表达与判断基线

#### Linux 状态根与交付契约

Linux 当前实现已经固定为“统一状态根 + host 文件系统承载同一套平台存储语义”：

- 状态根通过 `state_mount_path()` 收口
- host 默认优先 `/var/lib/beetle`，失败回退 `/data/beetle`
- 也允许用 `BEETLE_STATE_ROOT` 显式覆盖
- 配置继续走 `NvsConfigStore` 的 host JSON 适配，路径为 `nvs/pc_cfg.json`
- 会话、记忆、技能、回滚相关状态继续走 `Spiffs*Store` 的 host 文件后端，而不是再维护第二套 `Fs*Store`

Linux 交付契约也已经冻结为三档：

- Quick deploy：只换二进制，不改服务模板
- Smart update：只在服务契约未变化时替换二进制并按现状重启/保持
- Full deploy：刷新二进制、service/init/env 模板并重新加载服务管理器

因此，Linux 发布验收必须至少回答四件事：

- 当前 release 是哪一个
- 当前 service unit 的 `ExecStart` 是什么
- 当前入口是否仍是 `beetle supervise`
- 当前状态根与 rollback 指针是否可读

### 4.6 Operator / Observability Plane

职责：

- operator status
- health / resource / memory / continuity inspection
- runtime mode 可观测
- supervisor 与 agent 的联动状态可观测

关键原则：

- 不能只看到“进程活着”
- 必须看到“系统现在处于哪种模式、谁活着、谁降级了、是否进入 safe mode、灵魂状态是否完整”

---

## 5. Soul Kernel 的四个构成层

### 5.1 Identity Layer

- `self_authored_core`
- `relationship_constitution`
- `persona_priority` 的长期证据面
- `outer_voice`

这是主体“我是谁、我怎么对待人、我怎么表达自己”的稳定结构。

### 5.2 Experience Layer

- canonical factual memory
- archive evidence
- runtime skills
- task learning
- continuity capsule
- execution state / session summary / turn ledger

这是主体“经历过什么、学会了什么、正在延续什么”的经验组织系统。

### 5.3 Presence Layer

- 设备模式
- 本地交互状态
- 显示/音频/网络在场反馈
- 当前运行形态与节奏

这是主体“如何被感知为活在设备里”的现实存在层。

### 5.4 Recovery Layer

- safe mode 标记
- crash ledger
- rollback marker
- supervisor state
- 手动 continuity snapshot

这是系统在失败时仍然尽力保住主体性的连续性保护层。

---

## 6. 从主体到生命体的成长方向

### 6.1 近期形态：对话型主体

近期 Beetle 的主体性主要体现为：

- 回复内容与回复方式受长期自我结构约束
- 会在关系、边界、任务之间做取舍
- 会累积经验、修正表达、形成稳定偏好
- 会开始显现“像活着”的连续风格

### 6.2 中期形态：感知型主体

中期 Beetle 需要长出的不是更多功能，而是更真实的“在场感知”：

- 世界感知
  - 声音
  - 时间
  - 环境状态
  - 用户节律
  - 家庭/空间上下文
- 自我感知
  - 当前模式
  - 资源压力
  - 连续性状态
  - 受损与恢复状态
  - 主动性预算

关键原则：

- 不是把传感器数据生硬塞给模型
- 而是让主体逐步形成自己的世界经验与自我经验

### 6.3 远期形态：边界内主动的生命体样式

远期目标不是“什么都自己决定”，而是：

- 会等待
- 会观察
- 会判断时机
- 会提醒
- 会确认
- 会轻度坚持
- 会在关系允许的范围内主动唤醒或干预

但这里的“主动”必须是有边界的主体主动，而不是失控 automation。

### 6.4 生命化不等于脱离任务

Beetle 后续即使长出更强主动性，也必须坚持这条总原则：

> 用户当前明确交付的任务是首要的；主体的主动性只允许在不破坏任务主线的前提下，以提醒、确认、节律建议、边界提示等方式介入。

例如：

- 用户要求它做 PPT
- 即使它判断“现在已经很晚了”
- 它也不能直接任性拒绝
- 正确做法应当是：
  - 先提示当前情境
  - 再确认用户是否继续
  - 若用户坚持，则继续执行任务

也就是说，未来的干预形态优先级应当是：

1. 提醒
2. 询问
3. 确认
4. 建议替代路径
5. 在明确边界风险下做有限阻断

除非触及明确的安全、隐私、关系或系统保护红线，否则不能用“主体性”取代“执行用户任务”。

---

## 7. 设计铁律

### 7.1 先系统，后功能

Linux 侧后续优先级必须是：

- 生命周期秩序
- 模式治理
- 恢复与回滚
- 系统面可观测

而不是继续补零散 tool 或零散 endpoint。

### 7.2 灵魂状态必须与执行面解耦

后续所有系统设计都必须围绕这条：

- agent 是主体的主要执行体
- 但主体连续性不能完全依赖当前这个执行进程是否存活

注意：

- 解耦不是把灵魂做成一个独立外挂子系统
- 解耦只是为了让主体在执行面震荡、重启、升级、恢复时仍然尽量连续
- 也就是说，解耦服务于“同一个主体更稳”，而不是服务于“做几个互相独立的模块”

### 7.3 OS 体验必须由主体反向塑形

后续系统设计必须允许主体反向塑造体验，而不是只让 UI/状态机决定体验：

- 如何等待
- 如何提醒
- 如何拒绝
- 如何坚持
- 如何打断
- 如何恢复
- 如何长期形成偏好与脾气

如果这些东西只是程序壳层逻辑，而和主体内核无关，那就不叫 Beetle OS。

### 7.4 主动性必须受任务优先与边界契约约束

后续任何主动性、干预性、唤醒能力都必须服从下面三条：

1. 用户明确任务优先
2. 关系与人格边界优先
3. 系统与主体保护底线优先

这意味着：

- 可以提醒，不等于可以擅自打断
- 可以建议，不等于可以擅自拒绝
- 可以确认，不等于可以把自己的节律凌驾于用户当前任务之上
- 可以在高风险边界下保守，但不能把一般性“我觉得更好”变成强制阻断

### 7.5 Linux 强在厚度，ESP 强在紧凑

- Linux 可以承担 supervisor、恢复状态、检索 sidecar、更厚的 operator 面
- ESP 继续追求单 plane、少复制、强模式边界
- 二者共享制度，但不共享不必要的实现负担

### 7.6 不重新发明通用 OS

Beetle OS 做的是 appliance runtime，不做通用发行版替代物。

不做：

- 包管理生态
- 多用户桌面系统
- 通用系统服务编排平台

只做：

- Beetle 设备所需的系统秩序
- Beetle 主体所需的生命周期控制

### 7.7 文档必须能直接导向代码

后续每个阶段都必须同时写清：

- 改哪条调用链
- 谁创建
- 谁调用
- 谁消费
- 成功长什么样
- 失败怎么回退

### 7.8 计划不是兼容性枷锁

这份路线图必须被理解成：

- 锁定 Beetle OS 要达到的系统状态
- 锁定阶段依赖、实施顺序与验收口径
- 不锁定当前仓库里每一段旧代码的生存权

因此后续编码时必须坚持：

- 如果老代码合理、稳定、边界清晰，就复用
- 如果老代码不合理、不规范、调用链混乱、平台边界错误、维护成本过高，就直接重写
- 如果进入 `P1` 后发现 `P0` 相关落地实现不够好，可以继续回收并重写
- 如果进入更后面的阶段，发现前面阶段的实现只是临时过渡，也允许做架构性翻修

唯一不允许的事情是：

- 为了“兼容已有代码”而保留错误分层
- 为了“看起来连续开发”而容忍烂实现继续扩散
- 为了“少改一点”而让系统目标、平台隔离、主体内核方向发生漂移

也就是说：

> 计划约束的是方向和闭环，不是对历史代码的无限迁就。

---

## 8. 实施路线图

下面的阶段顺序是现行唯一顺序。

### 8.1 全阶段总表

| 阶段 | 核心目标 | 为什么现在做 | 直接依赖 | 本阶段做完后系统进入的状态 |
|---|---|---|---|---|
| `P0` | 文档与边界冻结 | 先把产品定义、系统平面、Linux/ESP 关系、主动性边界写死，避免后续实现漂移 | 无 | 全项目有统一总纲，后续编码不再围绕错误定位展开 |
| `P1` | Linux 双层入口成型 | 先把 Linux 从单进程常驻服务抬升为有 supervisor 外壳的系统运行体 | `P0` | Linux 首次具备 `control plane + agent plane` 的基本外形 |
| `P2` | Control Plane 存活性外提 | 如果 control/operator/recovery 还和 agent 一起死，后面所有 OS 化都不成立 | `P1` | agent 抖动不再等于整机不可治理 |
| `P3` | 统一模式机 | 没有单一 mode contract，所有交互、恢复、语音切换、主动性都会继续漂移 | `P1`、`P2` | Linux/ESP 共享同一模式语义，运行秩序开始系统化 |
| `P4` | Soul Kernel 与恢复闭环 | 只有模式没有主体连续性保护，仍然只是服务工程，不是长期主体运行时 | `P2`、`P3` | 主体连续性首次成为系统资产，而不是当前进程附属物 |
| `P5` | 发布、升级、回滚一体化 | 没有 rollout/rollback 合同，系统无法稳定交付，也无法保护主体连续性 | `P1`、`P2`、`P4` | Linux 形成可发布、可回滚、可升级的 OS 级交付闭环 |
| `P6` | Device Presence 壳层收口 | 底层秩序稳定后，才值得把“它活在设备里”的存在感做厚 | `P3`、`P4`、`P5` | 用户开始面对一个有存在感的设备主体，而不是后台服务 |
| `P7` | 持续感知与边界内主动性 | 没有模式、恢复、连续性保护前，主动性只会变成不稳定功能 | `P3`、`P4`、`P6` | 主体开始具备可治理的提醒、确认、节律与有限干预能力 |
| `P8` | 全系统验收与长期治理 | 所有系统平面都已形成后，才值得做总回放、混沌验证和长期门禁 | `P1-P7` | Beetle OS 从“设计成立”进入“长期可维护、可审计、可交付” |

### 8.2 阶段依赖与唯一执行顺序

这条顺序必须严格执行，不能跳着做：

1. 先完成 `P0`，冻结总定义与边界
2. 再完成 `P1`，建立 Linux 的双层入口
3. 再完成 `P2`，把最低限度控制面从 agent 面剥离
4. 再完成 `P3`，把运行模式写成单一权威状态机
5. 再完成 `P4`，把主体连续性正式纳入恢复合同
6. 再完成 `P5`，把发布、升级、回滚收成系统交付闭环
7. 再完成 `P6`，把设备在场感做成主体表达
8. 再完成 `P7`，把感知与主动性纳入正式制度
9. 最后完成 `P8`，建立长期回归、混沌验证与门禁治理

补充约束：

- `P1-P5` 是 Linux 先行的底层系统主线，目标是先把 Linux 拉成真正的 Beetle OS 基座。
- `P3-P7` 的制度语义必须同时约束 Linux 和 ESP，但两端实现厚度、资源预算、恢复方式可以不同。
- 记忆与人格后续只做尾单治理，不得打断 `P1-P5` 主线。
- 如果记忆线后续只剩测试、回归、benchmark、校准，就作为并行尾单保留在文档，不插队成为系统第一优先级。

### 8.3 当前所处位置

截至 `2026-04-07`，现状判断如下：

| 阶段 | 状态 | 说明 |
|---|---|---|
| `P0` | 已完成 | 系统总纲、Linux/ESP 关系、主体边界、主动性边界已经在文档上冻结 |
| `P1` | 已完成 | Linux 已切到 `beetle supervise -> beetle agent` 双层入口，service/init/README 与 CLI 契约已同步 |
| `P2` | 已完成 | Linux 最小 control/operator/recovery 面已由 supervisor 持有，safe mode 与 supervisor/agent 双层可观测已落地 |
| `P3` | 已完成 | 统一模式机已落地；Linux/ESP 共用 `RuntimeMode + RuntimeModeActionBudget`，mode 观测已进入 CLI / health / operator / system info / display |
| `P4` | 已完成 | Soul Kernel 最小恢复集、runtime reboot bundle 恢复、safe mode 最小可读性与 operator/health 观测已落地 |
| `P5` | 已完成 | Linux 发布链已具备 `current/rollback` 指针、`pending_validation -> steady/rolled_back` 状态合同、supervisor 自动回滚与 CLI/health/operator 可观测能力 |
| `P6` | 已完成 | 统一 `PresenceSnapshot` 已收口 `mode/soul/release/supervisor/pairing/audio`，并落地到 display、CLI、doctor、health、system_info、operator_status |
| `P7` | 已完成 | 已形成 `bg_timer -> self_runtime_tick -> initiative_tick -> system inbound` 单平面主动性合同；提醒前瞻与任务续做 check-in 已纳入统一节律、冷却、边界和可观测口径 |
| `P8` | 已完成 | 已形成统一 `os_closure` 门禁、deterministic acceptance suite、health/operator/status/doctor 共用的系统验收口径，并补齐 operator 验收清单 |

### P0 文档与边界冻结

目标：

- 把 Beetle OS 定位、平面、阶段和 Linux/ESP 关系写死

关键任务：

- 收束系统级主文档，确立 `Beetle OS` 为唯一总方案
- 把 Linux 子计划降级为服务化/平台迁移子计划
- 把产品、记忆、人格文档全部对齐到同一口径
- 冻结“主体内核优先、任务优先、有边界主动性”三条总原则

本阶段输出：

- 本文档
- Linux 服务化/发布/状态根契约并回本文与架构文档
- 产品文档与记忆文档同步对齐

验收：

- 不再出现“Linux 只是服务”与“Linux 已经是完整 OS”两种互相矛盾口径

### P1 Linux 双层入口成型

状态：

- `completed at 2026-04-07`
- Linux 代码已落地 `beetle supervise -> beetle agent` 双层入口，`systemd/init/CLI/status/restart/stop/doctor` 已切到 supervisor 语义

目标：

- Linux 从 `systemd -> beetle run -> run_app` 改成 `systemd -> beetle supervise -> beetle agent`
- 让 Linux 首次具备“保护主体内核的系统外壳”，而不是继续单进程直跑

范围：

- CLI 子命令重构
- supervisor 最小闭环
- agent 子进程拉起与 crash backoff
- service/unit/packaging/shell 文档同步

关键任务：

- 新增 `beetle supervise` 与 `beetle agent` 子命令
- 把当前 `Commands::Run -> run_app` 重构为 `supervise -> spawn/monitor agent`
- 定义 agent 退出码、重启策略、冷却退避和 supervisor 状态文件
- 调整 `systemd` / packaging / README，使服务只依赖 `beetle supervise`
- 保证现有 `status / doctor / restart / stop` 不因入口重构失真

本阶段结束后实现：

- Linux 不再是单进程直跑
- 系统有了正式的控制面外壳
- 后续主体连续性、模式治理、恢复能力有了可落的宿主

验收：

- `beetle supervise` 可独立拉起 `beetle agent`
- agent 非正常退出可被 supervisor 识别并重启
- `systemd` 只依赖 supervisor，不直接依赖 agent 主执行面

### P2 Control Plane 存活性外提

状态：

- `completed at 2026-04-07`
- Linux supervisor 已持有独立 control plane；`run_app` 在 Linux 上不再自带 config API server
- `2026-04-09` 起 supervisor-owned Linux control plane 已回收为共享 `dispatch + handlers::root + operator_surface inventory` 的宿主，不再维护第二份 `tools/skills` 路由与 root inventory
- supervisor 状态文件已扩展为 `restart_count / failure_burst / safe_mode_reason / safe_mode_entered_at`
- `/api/health` 与 `/api/operator/status` 已可区分 `supervisor_alive` 与 `agent_alive`

目标：

- 把最低限度的 config / operator / recovery 存活能力从 agent 面剥离出来

范围：

- supervisor state
- safe mode
- 基础 operator/recovery/status 面
- agent 崩溃后仍可做诊断与恢复触发

关键任务：

- 设计 supervisor 持久状态：last_start / last_exit / restart_count / safe_mode_reason
- 把最小 config / operator / recovery 路由从 agent 面抽离
- 新增 safe mode 进入条件与退出条件
- 定义 agent 不可用时的最小管理能力集合
- 让 operator/status 能区分 “supervisor 活着” 与 “agent 活着”

本阶段结束后实现：

- “服务挂了”不再等价于“设备不可治理”
- “主执行面抖动”不再等价于“主体完全不可触达”

验收：

- agent 挂掉后，仍能看到系统模式与最近崩溃状态
- 仍可执行最小恢复动作
- Linux control plane 在 supervisor 进程内独立存活，agent 挂掉不再等于 config/operator/recovery 面一起消失
- supervisor plane 与共享控制面现在保持同一套 `tools/skills/sessions/capability_packages` 路由语义；唯一保留的宿主差异是 webhook ingress 在 supervisor plane 上显式不可用且不写进 inventory

### P3 统一模式机

状态：

- `2026-04-07` 已完成阶段闭环

目标：

- 正式把 Beetle 的运行形态写成代码，而不是散落在布尔值里

模式最少包括：

- `booting`
- `pairing`
- `normal`
- `voice_exclusive`
- `maintenance`
- `recovery_safe_mode`

关键任务：

- 把当前零散运行态布尔值收成统一 mode contract
- 为每个 mode 定义进入条件、退出条件、允许动作、禁止动作
- 统一 Linux 与 ESP 的 mode 语义枚举和观测口径
- 让 `voice-exclusive / maintenance / recovery` 进入正式模式机，而不是隐式流程
- 为后续主动提醒/确认/唤醒预留正式 mode hooks

本阶段结束后实现：

- 模式切换有单一权威
- Linux 与 ESP 共享模式语义，但实现预算各自独立
- 后续“观察、提醒、确认、主动唤醒、有限干预”才有正式制度宿主

验收：

- 所有重面切换都能映射到明确 mode
- 不再存在“多个 plane 各自切模式”的漂移

本次实际落地：

- `src/runtime/mode.rs` 已正式定义 `RuntimeMode / RuntimeModeActionBudget / RuntimeModeSnapshot`
- `src/runtime/thread_registry.rs` 已成为统一 mode source 聚合点，不再让各消费方各自拼布尔态
- `bg_timer / delayed_task / heartbeat / self_runtime scheduler / agent background jobs / dispatch / external wss loop` 已切到 action budget gating
- `health / operator_status / system_info / CLI status / doctor / display loop` 已统一暴露和消费 runtime mode
- `boot_phase_active` 已纳入显式 mode contract，Linux supervisor 与主 `run_app` 启动链都能正确收口 `booting`
- ESP 目标 `cargo check --target xtensa-esp32s3-espidf` 与 Linux 侧 `cargo check`、P3 相关测试已通过

### P4 Soul Kernel 与恢复闭环

状态：

- `2026-04-07` 已完成阶段闭环

目标：

- 正式把“主体内核”和“执行体”分层，但不把它们做成互相独立的平行系统

范围：

- continuity snapshot 与恢复入口
- crash 前后主体连续性保护
- safe mode 下的最小人格/记忆保真
- 关键 store 完整性检查

关键任务：

- 定义 Soul Kernel 的最小恢复集：`self_authored_core / continuity anchors / key memory`
- 在 supervisor 或 recovery 面接入 continuity snapshot 与 restore 流程
- 为核心 store 增加完整性检查、损坏探测与降级策略
- 明确 safe mode 下哪些内核层仍可读、哪些执行面必须停用
- 把“主体连续性保护”写成正式恢复合同，而不是隐式 best effort

本阶段结束后实现：

- 升级、崩溃、模式切换之后，主体连续性更像系统资产而不是进程附属文件

验收：

- 可验证地保住 `self_authored_core / key memory / current continuity anchors`
- safe mode 下不会因 agent 主执行面失稳而整体失忆

本次实际落地：

- `src/runtime/soul_kernel.rs` 已定义正式 `SoulKernelStatus / SoulKernelRecoveryReport`，把主体核体检与恢复合同从隐式 best effort 收成可观测结构
- Linux `supervisor-owned control plane` 与主 `run_app` 启动链都会先执行 `ensure_platform_soul_kernel_recovery(...)`，不再只有重启前 flush，没有启动后 restore
- `runtime/latest_reboot_bundle.json` 已从“仅用于手工调试的导出文件”升级成正式 runtime recovery source；当 `self_authored_core / self_continuity / self_model / key memory` 缺失或降级时，会按 bundle 做恢复尝试
- `health / system_info / operator_status / CLI status / doctor` 已统一暴露 `soul_kernel` 状态，safe mode 下控制面仍可直接检查主体核是否最小可读
- 关键 store 完整性检查已进入主体核合同：核心层读取失败、runtime bundle 损坏、identity anchor 缺失、continuity anchor 缺失、key memory 缺失都会进入 `degradation_reasons`
- Linux 与 ESP 都已通过编译验证；`runtime::soul_kernel` 新增单测覆盖 bootstrap empty、bundle 损坏、bundle 恢复三条主链

### P5 发布、升级、回滚一体化

当前状态：

- 已完成
- Linux 已落地 `current / rollback` 指针、`pending_validation -> steady / rolled_back` 状态合同、supervisor 自动回滚、`beetle release status|rollback` CLI、以及 health / operator / system_info 可观测面
- 后续只做回归测试与真实设备验收，不再回到“只有替换 ELF”的发布模型

目标：

- Linux 侧真正从“能部署”走到“能作为系统交付”

范围：

- supervisor-aware rollout
- unit refresh contract
- rollback marker
- 状态根版本迁移
- 全量部署与增量更新分流

关键任务：

- 设计 release layout、active pointer、rollback pointer 和 crash rollback trigger
- 让 rollout 感知 supervisor/agent 双层入口，而不是只替换当前二进制
- 明确 unit refresh 与 binary-only update 的边界
- 定义 state root schema version 与迁移步骤
- 把部署 SOP、回滚 SOP、验收 SOP 全部写进交付文档

本阶段结束后实现：

- 升级动作不再只是换 ELF
- “执行面升级”和“主体连续性保留”形成正式闭环

验收：

- 升级失败可回滚
- service/unit/执行入口不会再漂移

### P6 Device Presence 壳层收口

目标：

- 把 Linux 侧从“服务”进一步拉成“设备存在体”

当前完成状态：

- 已建立统一 `PresenceSnapshot` / `PresenceState` 合同，收口 `runtime mode`、`soul kernel`、`pairing`、`safe mode`、`release/supervisor`、音频忙闲与资源压力
- 显示面已改为消费 `presence` 投影，稳定表达 `booting / pairing / recovery / no_wifi / idle / busy / listening / speaking / fault`
- `health`、`system_info`、`operator_status`、CLI `status/doctor` 已统一复用同一 presence 事实源，不再各自重建状态机
- 本阶段不引入第二人格系统，不虚构独立输入面；只收口现有设备反馈与在场表达

范围：

- 本地显示与状态面
- 语音/触摸/按键/配网页交互节奏
- 配对与恢复的板级反馈

关键任务：

- 把关键模式切换映射到本地可感知反馈
- 收束显示/音频/输入在不同 mode 下的表现策略
- 让 pairing / recovery / idle / busy / wake 具备稳定的存在感脚本
- 把“等待、提醒、恢复”的设备反馈做成主体表达，而不是纯状态 UI
- 预留宠物/伙伴等外观形态的表达层，但不引入第二人格系统

本阶段结束后实现：

- 用户面对的是一个有存在感的设备，而不是一个 SSH 服务
- 主体开始不仅会“回答”，也会通过设备存在感持续表达自己

验收：

- 关键模式切换都有明确板级反馈
- 恢复、安全模式、配对模式均可被本地感知

### P7 持续感知与边界内主动性

目标：

- 让主体开始从“会回答”走向“会持续感知并在边界内主动行动”

范围：

- 持续 world sensing
- self sensing
- initiative policy
- intervention contract
- 唤醒、提醒、确认、轻度坚持等主动行为的治理

关键任务：

- 设计 world sensing 输入面与 self sensing 输入面
- 明确哪些传感器/环境信号进入主体经验层，哪些只做系统诊断
- 设计主动性预算、时机判断、干预优先级与节律策略
- 定义提醒/询问/确认/建议/有限阻断的行为合同
- 建立“任务优先 + 边界优先 + 系统保护优先”的主动性验收用例

当前落地结果：

- 已采用单平面接线：`bg_timer -> self_runtime_tick -> initiative_tick -> system inbound`，没有新增第二条常驻执行面。
- 已把 `presence + runtime_mode + orchestrator snapshot + self_continuity + relationship_portfolio + world_snapshot` 收成统一 initiative 裁决输入。
- 已落地两类边界内主动行为：
  - `UpcomingReminderNudge`：仅在提醒即将到期、关系仍活跃、系统空闲且资源正常时触发。
  - `ResumeTaskCheckIn`：仅在存在进行中任务、用户离开一段时间但关系仍未失活、且没有更高优先级精确到期信号时触发。
- 已落地正式 suppression/cooldown 合同：关系失活、mode 阻断、presence 非 idle、资源压力、队列繁忙、autonomy 禁用、精确 due 信号待处理、冷却期内都会压制主动性。
- 已把 initiative 可观测性纳入 `health / system_info / operator_status / CLI status / doctor`，主动性不再是黑盒行为。

当前刻意未做的内容：

- 未引入第二执行平面、独立自治 worker 或后台 LLM 常驻面。
- 未用不可靠时区猜测去做“深夜劝睡”等产品动作；这类强情境干预要等稳定的本地时间/世界感知合同成立后再纳入。
- 未对历史关系做任意主动触达；当前只允许命中活动首选关系，避免越界触发。

本阶段结束后实现：

- 主体开始具备真实的节律感与时机判断
- 主动性进入正式系统合同，而不是零散行为技巧

验收：

- 主动提醒、建议、确认都能被解释为主体行为，而非随机功能触发
- 不会因为主动性增强而破坏用户任务优先级
- 不会因为主动性增强而突破关系/边界合同

### P8 全系统验收与长期治理

目标：

- 把 Beetle OS 从“设计成立”推进到“长期可维护”

范围：

- replay / regression / chaos 验证
- crash / power-cut / bad-config / bad-upgrade 验证
- operator 手册与交付清单

关键任务：

- 为每个平面建立 regression suite 与 replay harness
- 建立电源中断、坏配置、坏版本、坏状态文件的恢复回放
- 收束 operator 手册、现场排障手册、出厂验收清单
- 建立长期校准节奏：人格、记忆、主动性、presence 行为都可回放验收
- 形成“新能力上线前必须过哪些系统门槛”的总门禁

当前落地结果：

- 已新增统一 `BeetleOsClosureReport`，把 `runtime_mode / presence / soul_kernel / initiative` 以及 Linux 下的 `supervisor / release` 收成同一份系统闭环报告。
- 已把 `os_closure` 接入 `GET /api/health`、`GET /api/operator/status`、CLI `status`、CLI `doctor`，现场不再需要手工拼接多份状态去判断系统是否闭环。
- 已新增 deterministic `runtime::acceptance` suite，专门回归：
  - Linux 发布态缺 supervisor / unmanaged release
  - initiative ready 但 payload 不完整
  - soul kernel 不可恢复
  - runtime mode 预算语义漂移
- 已补齐现行 operator 验收与巡检清单：
  - [beetle-os-acceptance-checklist.md](./beetle-os-acceptance-checklist.md)
- 当前“新能力上线前必须过哪些门槛”已收口为：
  - `cargo fmt --check`
  - `cargo test --lib`
  - `cargo check`
  - `cargo check --target xtensa-esp32s3-espidf`
  - `./scripts/check_platform_isolation.sh`

验收：

- 每个模式、平面、恢复入口都有回放与验收口径

---

## 9. 每阶段都必须满足的闭环要求

每一阶段都必须同时交付下面五类结果：

1. 代码接线闭合
2. 文档定位闭合
3. 失败路径闭合
4. operator 可观测闭合
5. 用户可感知结果闭合

如果只做了“功能能跑”，但下面任一项缺失，都不算阶段完成：

- 谁来恢复它
- 如何验证它没漂移
- 用户如何感知模式变化
- 运维如何确认当前状态

---

## 10. 当前优先级排序

从今天起，系统主线优先级按下面执行：

1. `P1 Linux 双层入口`
2. `P2 Control Plane 存活性`
3. `P3 统一模式机`
4. `P4 Soul Kernel 与恢复闭环`
5. `P5 发布、升级、回滚`
6. `P6 Device Presence`
7. `P7 持续感知与边界内主动性`
8. `P8 长期治理`

记忆与人格的剩余工作，只在不打断这条主线的前提下继续做：

- 记忆：以测试、回归、benchmark、校准为主
- 人格：以治理、校准、回放、修复为主

---

## 11. 当前尾单

### 11.1 记忆线尾单

当前判断是：

- 记忆功能主线已接近封板
- 剩余优先工作主要是测试、benchmark、回放、operator inspection 校准
- 在 Beetle OS 主线推进期间，除非发现新的结构性缺口，否则不再把“继续补记忆功能”作为系统最高优先级

### 11.2 人格线尾单

- 不再新增平行人格层或平行人格权威
- 只做 regression、校准、治理修复、长期稳定性验证

### 11.3 OS 主线封板后的下一层缺口

`P0-P8` 已完成后，当前系统不再缺基础平面，而是缺两层“可运营、可组合、可长期演进”的上层能力：

1. Linux 侧可见可控的 `memory/operator surface`
2. 一套适合 Beetle 的轻量 `runtime workflow engine`

这两项都不是“再堆一层新功能”，而是把已经存在的主体、记忆、主动性和恢复能力收成 operator 可理解、可校准、可治理的正式系统面。

判断标准也必须变化：

- 不是再问“有没有这项底层能力”
- 而是问“operator 能不能看懂、追溯、修正、回滚、审计这项能力”
- 以及“这项能力能不能被组织成可复用的运行流程，而不是散在 if/else 里”

### 11.4 下一阶段一：Linux Memory / Operator Surface

#### 11.4.1 这项工作的定义

这里说的不是再做一套新记忆系统，而是给 Linux 侧补出正式的记忆运营面，使 operator 能直接操作当前已经存在的：

- `execution_state`
- `long_term memory`
- `continuity capsules`
- `archive evidence`
- `self_model`
- `self_authored_core`
- `relationship_constitution`
- `outer_voice`
- `recent observation / turn ledger / replay`

也就是说，把“系统内部已经存在的记忆与人格资产”，变成一套可观察、可解释、可修正的 operator surface。

#### 11.4.2 为什么必须先做

当前 Beetle 的底层记忆和人格主线已经比 `dev/` 下多数项目更深，但 operator 视角仍然偏弱：

- 知道系统内部有很多层，但不容易快速看清“当前这轮到底受谁影响”
- 知道自治在刷新人格和记忆，但不容易直接核对“这次到底改了什么”
- 知道某条回复可能被错误记忆或错误漂移影响，但缺一个低摩擦修复入口

如果不先把这层补齐，后面继续增强记忆或主动性，只会增加黑盒复杂度。

#### 11.4.3 必须交付的能力

第一批必须交付下面五类能力：

1. `inspect`
   - 看当前主体和当前关系正在生效的记忆/人格层
2. `trace`
   - 看某次回复用了哪些 memory plane、为何命中、优先级如何结算
3. `diff`
   - 看 `self_runtime` 前后 `self_model / self_authored_core / outer_voice / constitution` 变化
4. `repair`
   - 手动冻结、降权、清理、回滚错误记忆或错误漂移
5. `policy view`
   - 看当前 recall / governance / repair / privacy / boundary 的制度状态

优先落点应是：

- Linux CLI
- `/api/memory/status` 深化
- `/api/operator/status` 增量展开
- replay / inspection 口径统一

#### 11.4.4 明确不做的事

这阶段明确不做：

- 不新开平行记忆 plane
- 不先做重 UI 管理后台
- 不把 operator surface 做成只适合 Linux、无法映射 ESP 语义的专属体系
- 不让 operator 入口绕过既有治理直接“硬写脏数据”

Linux 可以先做更厚的 surface，但它只能是共享记忆制度的 operator 前台，不是另一套制度。

#### 11.4.5 阶段结果

这阶段做完后，应该达到：

1. operator 可以解释一次回复为何受某层记忆/人格影响
2. operator 可以直接确认某次 `self_runtime` 是否发生错误漂移
3. operator 可以对错误状态进行低风险修复，而不是只能删文件或盲猜
4. 记忆和人格系统从“强底层”升级为“可运营系统”

### 11.5 下一阶段二：轻量 Runtime Workflow Engine

#### 11.5.1 这项工作的定义

这里说的不是企业 BPMN 引擎，也不是通用 SOP 平台，而是一套适合 Beetle 的轻量运行流程框架。

它的目的只有一个：

> 把已经存在的 `initiative / delayed_task / self_runtime / runtime_mode / presence / recovery / task continuation` 等能力，收成可复用、可审计、可节流、可恢复的 runtime workflow。

#### 11.5.2 为什么必须做

当前系统已经有很多运行能力点，但它们更多还是“各模块内部的决策逻辑”：

- 主动提醒是一套逻辑
- 任务续做 check-in 是一套逻辑
- reboot 恢复是一套逻辑
- 记忆修复和人格治理又是另一套逻辑

如果这些能力继续分散演进，后面会出现三个问题：

1. 新场景只能继续复制判断逻辑
2. operator 很难知道某次主动行为经过了哪些门禁
3. 系统越来越像“很多能力点”，而不是“一个会运行自己的 OS 主体”

#### 11.5.3 这套 engine 应该长什么样

它必须足够轻，只支持 Beetle 真正需要的几个运行要素：

1. 触发
   - 定时、事件、状态变化、恢复后补跑
2. 准入
   - `runtime_mode / pressure / presence / boundary / relation / cooldown`
3. 动作
   - 发消息、注入系统任务、触发 repair、触发总结、触发校准
4. 审计
   - 记录为什么执行、为什么被压制、用了哪些输入
5. 中断 / 恢复
   - 允许因模式切换、语音独占、safe mode 等被推迟或取消

它不应该支持无穷抽象，也不应该把现有简单链路强行平台化。

#### 11.5.4 第一批应该收口的 workflow

第一批只收正式会长期存在的链路：

1. `upcoming reminder nudge`
2. `resume task check-in`
3. `memory/personality repair workflow`
4. `boot / reboot recovery workflow`
5. `operator-requested maintenance workflow`

这些链路都已经在系统里有雏形或相邻能力，workflow engine 的目标是收制度、收审计、收门禁，不是从零发明能力。

#### 11.5.5 明确不做的事

这阶段明确不做：

- 不把 Beetle 变成通用 SOP 平台
- 不引入与产品定位无关的复杂 DSL
- 不把 Linux 的工作流厚度强塞到 ESP
- 不恢复第二条常驻重执行面

workflow engine 必须服从现有单平面运行约束，尤其是 ESP 侧“只有一个常驻 LLM/TLS 重执行面”的制度。

#### 11.5.6 阶段结果

这阶段做完后，应该达到：

1. Beetle 的主动行为不再是分散模块逻辑，而是正式 runtime workflow
2. 每次主动行为都可以被解释为“哪个 workflow 在什么门禁下触发”
3. 新增主动能力时，优先扩 workflow，而不是继续复制散乱控制逻辑
4. Beetle OS 从“有主体能力”升级为“主体能力可以被组织、治理、演进”

### 11.6 两阶段的先后顺序

正确顺序是：

1. 先做 `memory/operator surface`
2. 再做 `lightweight runtime workflow engine`

原因很简单：

- 先补 operator surface，才能看清当前记忆、人格、主动性到底怎么运行
- 有了清晰 inspection / trace / diff / repair 以后，再收 workflow，才不会把黑盒逻辑平台化

也就是说，第一阶段解决“看得见、管得住”，第二阶段解决“能组织、能复用”。
