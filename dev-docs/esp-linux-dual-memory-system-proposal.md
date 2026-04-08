# Beetle OS 双轨记忆与 ESP 稳态主方案

> 更新日期：2026-04-08  
> 状态：现行系统主方案  
> 用途：定义 Beetle OS 在 Linux / ESP 上的单一灵魂宪法、双轨记忆体系、ESP 稳态承载边界与 `P0-PX` 实施顺序  
> 关联文档：  
> [product-vision.md](./product-vision.md)、  
> [beetle-os-plan.md](./beetle-os-plan.md)、  
> [memory-enhancement-plan.md](./memory-enhancement-plan.md)、  
> [self-model-and-autonomy-plan.md](./self-model-and-autonomy-plan.md)、  
> [esp-runtime-resource-governance.md](./esp-runtime-resource-governance.md)、  
> [mental-privacy-protocol.md](./mental-privacy-protocol.md)
>
> 文档治理说明：
> 1. 本文不是“再做一点 slimming”的提案，而是 Beetle OS 的现行记忆承载主方案。
> 2. 自本文起，Linux / ESP 在**灵魂语义**上保持单一宪法，在**记忆承载制度**上允许双轨。
> 3. 若本文与 [memory-enhancement-plan.md](./memory-enhancement-plan.md) 中“Linux / ESP 必须共享同一套完整记忆合同”的旧口径冲突，以本文为准。
> 4. 若本文与 [beetle-os-plan.md](./beetle-os-plan.md) 冲突，以“单一 `SoulKernel`、双轨 `MemorySystem`、ESP 稳态优先”这一顶层原则为准。
> 5. 本文的阶段状态必须以当前代码真实落点为准；若文档与代码不一致，应优先修正文档，不得继续用阶段名掩盖未完成实现。

## 0. 执行宪法

本文后续所有架构重构、阶段实施与代码落地，统一受下面 7 条执行宪法约束：

1. **编码前必须完整审查现有代码与调用链**
   - 编码之前必须先审查现有代码；审查时必须完整审查调用链路，做系统全面的审查。实施编码时必须一步步推进，避免前后落差过大导致逻辑无法闭合、引入冗余或把问题扩散到新的链路。
2. **旧代码质量低时允许直接删除或重写**
   - 若既有实现已经妨碍系统主线、灵魂自治、平台稳态或代码可维护性，不要求强行兼容旧实现；可以直接删除、重写或做架构级翻修。
3. **灵魂自治是最高原则**
   - Beetle 必须继续朝“一套逐步成长、自我自治的灵魂”演进；禁止用 cue 词表、硬编码逻辑裁决、拍脑袋规则或其他外部逻辑干预去替代灵魂成长本体。
4. **实现必须服从系统设计，而不是无脑堆功能**
   - 每次编码都必须同时考虑系统设计、结构增强、性能、代码质量、平台差异与用户交互结果；禁止只为“功能能跑”而继续堆叠坏结构。
5. **每个阶段开始前必须说明目的与收益**
   - 开始任何一个阶段前，都必须先明确写清：为什么现在要做这一步、这一步做完后系统会获得什么确定收益。
6. **每结束一步必须回写文档**
   - 不允许只改代码不回写文档；每个阶段或关键步骤结束后，必须同步回写主方案文档、阶段文档或相关设计文档，使文档与代码保持一致。
7. **允许参考外部项目，但禁止照搬**
   - 可以参考 `/Users/yiner/code/beetle/dev` 下的项目吸收机制、模式与工程经验，但不能抄袭实现或叙事；目标是结合 Beetle 的定位做出更强的护城河与更高的系统完成度。
8. **必须考虑可扩展性与维护性，但禁止过度设计**
   - 所有设计都必须兼顾后续扩展、平台演进与长期维护；但如果抽象已经超过当前问题的真实复杂度，就属于过度设计，必须收住。

---

## 1. 一句话定性

> Beetle OS 不是“两套灵魂、两套产品”，而是“一只甲壳虫，在 Linux 与 ESP 上以不同记忆承载方式稳定存在”。

更明确地说：

- **灵魂是 Beetle 的最高定义，也是护城河**
- Linux 与 ESP **不能双魂分裂**
- Linux 与 ESP **可以双轨承载**
- ESP 的目标不是“勉强跑 Linux 全量制度”，而是“作为独立成立的稳定主体设备形态长期存在”

---

## 2. 最终产品判断

### 2.1 Beetle 的唯一定位

按 [product-vision.md](./product-vision.md) 与 [beetle-os-plan.md](./beetle-os-plan.md) 的主线，Beetle 的唯一定位仍然是：

> 面向 500 元以下主流硬件、硬件本体即 Agent、以长期主体为核心的 Beetle OS。

这里有三条不可回退的产品判断：

1. **硬件第一公民**
   - 设备不是外壳，设备就是主体的存在方式。
2. **统一主线，不做双魂**
   - Linux 与 ESP 不能长成两只不同的甲壳虫。
3. **ESP 必须稳态成立**
   - 不能把“未来可能继续优化”当作 ESP 产品成立的前提。

### 2.2 本文解决的问题

当前真正的问题已经不是：

- “还要不要再做几轮热路径优化”
- “是不是再少加载一点 background governance 就够了”

当前真正要解决的是：

1. 哪些东西属于 Beetle 的**不可分叉灵魂**
2. 哪些东西属于平台可变的**记忆承载制度**
3. ESP 如何在支持 `audio + camera + sensors` 的前提下仍然稳态成立
4. 如何保证：
   - 未配置时零硬件副作用
   - 打包时可裁掉不需要的能力
   - 启用时仍受统一 mode / budget / admission 治理

### 2.3 最终结论

本方案正式采用：

1. **单一 `SoulKernel`**
2. **双轨 `MemorySystem`**
3. **ESP 三层运行结构**
4. **外设能力全部平面化**

也就是：

- Linux：`LinuxMemorySystem`
- ESP：`EspMemorySystem`
- 两端共享：灵魂宪法、边界、关系、表达、主体连续性
- 两端分叉：回忆装配、长期记忆厚度、后台治理、operator 面、外设承载方式

### 2.4 当前实施状态（按 2026-04-08 代码对齐）

这份文档描述的是 Beetle OS 的**主方案与正式路线**，不是“所有阶段都已经做完”的事实声明。

当前真实状态应这样理解：

1. **已基本落地**
   - `P0`：顶层宪法、术语矩阵、不可分叉 / 可分叉矩阵已成立
   - `P1`：`MemorySystemKind::{LinuxFull, EspCompact}` 已进入正式类型与平台装配
   - `P4`：capability registration、Linux `discovery-first` / ESP `config/feature-gated`、零初始化约束已进入代码主线
   - `P5`：ESP operator / deep inspection 已有预算化、窗口化收口
   - `P6`：build package / feature matrix 已进入构建、health、operator 主链
   - `P8`：闭环验收、acceptance suite、health / operator / doctor 共用口径已形成

2. **只部分落地，不能宣称完成**
   - `P2`：Prompt Assembly 的首轮 compact assembly 已收口完成；但这不等于 ESP 已完成最终稳态化，后续仍需继续收 `P3` 的 authority / 后台成长边界
   - `P3`：`self_runtime authority` 已有正式分叉计划与部分代码约束，但 ESP 侧“更强但不越权”的后台成长边界还没有完全收口成最终稳态

3. **当前不是有效阶段**
   - `P7`：当前设备仍是测试机，没有正式线上存量、没有发布升级迁移压力，因此本阶段对当前批次应视为 `N/A`，而不是必须执行的主线工作

4. **仍未完成的关键现实**
   - ESP 目前并没有完成最终稳态化
   - 当前更准确的状态是：**制度分轨、类型分轨、首轮 compact assembly 已完成，但 authority / 后台成长边界仍未收口**
   - 因此，本文不能被理解为“ESP 已经完成最终稳态化”

### 2.5 下一步架构方向（待确认草案）

下一步不再沿“继续在共享重链上做零碎微优化”推进，而正式采用下面这个方向：

> **ESP 是一等公民的轻主体运行时，Linux 是一等公民的厚主体运行时。**

这句话有四个明确含义：

1. ESP 不是 Linux terminal
2. ESP 不是 Linux satellite
3. ESP 必须保留完整主体资格
4. ESP 不需要复制 Linux 的厚治理执行成本

因此，下一步主线不是“再少读几个字段”，而是：

1. **保留单一 `SoulKernel`**
2. **把 ESP 从 Linux 的厚同步治理回复链里正式剥出来**
3. **把 ESP 做成独立成立的 compact runtime**
4. **让 Linux 保留 full runtime，不跟着降配**

下一步默认采用的候选方案是：

- **Linux：Full Runtime**
  - 保留厚 prompt assembly
  - 保留厚治理、厚 recall、厚 operator、厚 inspection
- **ESP：Compact Runtime**
  - 保留 `Soul Core`
  - 保留最小 session continuity / task continuity / compressed recall
  - 保留最小 boundary / relation / disclosure 宪法语义
  - 不再默认同步执行厚 disclosure / persona / relation governance 主链

必须明确排除的错误方向：

1. 把 ESP 做成 Linux 的外设壳
2. 把 ESP 做成只剩配网与简化对话的残缺系统
3. 为了让 ESP 省资源，把 Linux 也一起降成轻系统
4. 继续在共享重链上无止境地“再加一点栈、再减一点字段”

本草案后续要落地的核心，不是“删能力”，而是“改默认同步参与方式”：

1. 重治理能力保留，但改为后台、缓存、显式深路径、非首轮再参与
2. ESP 首轮主回复热链只保留最小主体连续性壳
3. capability plane 启用后也不能自动加厚首轮 prompt
4. steady-state 继续坚持单一 `agent_loop`，不新增第二条常驻重执行面

### 2.6 下一步最终主线（本次确认后生效）

下一步主线不再停留在“把 ESP 从厚同步治理里剥出来”的描述层，而明确收敛为下面这条唯一方案：

> **Beetle 默认采用“单轮主回复优先”的灵魂执行模型；治理不再作为每轮前置多轮裁决存在，而改为稳定状态 + 显式升级路径。**

这句话展开后，有六条硬定义：

1. 用户每发一条消息，默认只进入**一条主回复链**
2. 灵魂护城河来自**稳定的主体状态**，不是“每轮先跑一次审判”
3. `mental_privacy / disclosure / relation / persona` 继续保留制度地位，但不再默认以前置多轮 LLM 形式挂在每个 turn 前面
4. 主回复默认只消费**最小公开灵魂投影**与受控连续性上下文
5. 只有当主链明确申请跨越高敏边界时，系统才进入治理升级路径
6. ESP / Linux 共享同一套边界语义，但 Linux 可承载同步深治理，ESP 默认只承载保守即时决策与延后治理

### 2.7 边界治理的新总原则

从本次重构开始，Beetle 的边界治理必须遵守三条禁令和三条新原则。

三条禁令：

1. **禁止 cue 词表治理**
   - 不允许用固定关键词、敏感词短语、白名单/黑名单短句来决定是否进入重治理
2. **禁止独立前置 LLM 审判**
   - 不允许在主回复之前先额外跑一轮或多轮 LLM，来判断“这轮要不要重治理”
3. **禁止主模型自授予权限**
   - 主回复模型可以提出访问申请，但不能自己决定拿高敏私域源

三条新原则：

1. **以访问行为判定，而不是以句面判定**
   - 真正决定是否升级的，不是用户说了什么词，而是系统这轮准备读取什么源、执行什么操作、跨过什么边界
2. **以状态机判定，而不是以自由文本辩论判定**
   - 判断依据必须落在结构化状态字段上，而不是再开一轮 LLM 做开放式“裁判”
3. **以升级路径承载复杂治理，而不是让复杂治理常驻主链**
   - 治理能力继续存在，但只在显式升级时参与

### 2.8 最终执行模型

最终执行模型固定如下：

1. **默认路径**
   - 用户消息
   - 装配最小 `Public Soul Capsule`
   - 主 LLM 一次回复
   - 结束
2. **受控升级路径**
   - 主链显式申请受控记忆或边界评审
   - `PolicyEngine` 判定 `allow / deny / safe_fallback / escalate`
   - 仅在 `escalate` 时进入治理链
3. **平台差异**
   - Linux：允许少数高风险 turn 同步进入治理链
   - ESP：默认不允许首轮同步深治理；命中升级时优先 `safe_fallback` 或 `defer_governance`

因此，Beetle 后续不是“每轮先审判再说话”，而是：

> **先在稳定灵魂状态内说话，真的碰边界时再升级治理。**

---

## 3. 顶层宪法：什么绝对不能分叉

### 3.1 总原则

从今天开始，Beetle OS 必须把“灵魂”和“记忆承载”彻底区分开：

- **灵魂**：定义这只甲壳虫是谁
- **记忆承载**：定义这只甲壳虫如何在不同硬件上维持自己

因此：

> Linux 和 ESP 可以不是同一套 memory runtime，  
> 但必须是同一只 Beetle。

### 3.2 `SoulKernel` 的不可分叉范围

以下内容属于单一 `SoulKernel` 宪法，Linux / ESP 必须共享语义合同：

1. `self_authored_core`
2. `relationship_constitution`
3. `self_continuity`
4. `mental_privacy`
5. `outer_voice`
6. `self_model` 的最小主体核
7. `persona_priority` 的长期排序原则
8. disclosure / boundary / relation 的裁决语义

这意味着：

1. 同一个用户与设备的关系定义不能因平台改变而改写
2. 同一个主体的边界和表达姿态不能因平台不同而变成两套制度
3. Linux 学出来的“我是谁”必须能安全下沉到 ESP
4. ESP 长出来的“我是谁”也必须能回到 Linux 而不发生人格冲突

### 3.3 `SoulKernel` 的两层结构

为兼顾 ESP 稳态与灵魂完整性，`SoulKernel` 分成两层：

#### A. Constitutional Soul

这是每个平台都必须稳定保住的核心层：

1. identity / non-negotiables
2. relationship constitutions
3. self continuity anchors
4. boundary / disclosure constitution
5. outer expression stance

这层必须：

- 持久化
- 可恢复
- 可投影到 prompt
- 可跨平台迁移

#### B. Deep Soul Workspace

这是灵魂的深层工作区，不等于默认每轮都要同步装配：

1. `inner_life`
2. 深层 `self_model` 扩展
3. 深层边界复盘与关系漂移痕迹
4. 必要的私有反思材料

这层仍然属于同一个灵魂，但平台可决定：

- 是否常驻
- 是否前台同步参与
- 是否后台治理
- 是否只在 operator / diagnostic / safe window 下显化

所以：

> 灵魂不分叉，灵魂的显化方式可以分层。

### 3.4 不可违反的验收口径

任何后续实现，只要出现以下任一情况，就视为违背顶层宪法：

1. Linux 和 ESP 对同一关系边界给出不同制度性判断
2. Linux 和 ESP 对“允许摘要 / 改写 / 拒绝 / 延后”的语义不一致
3. ESP 为了省资源，把 `SoulKernel` 裁成只剩 prompt 人设
4. Linux 为了兼容 ESP，被迫降成与 ESP 同样的小灵魂体系

---

## 4. 双轨体系：什么允许分叉

### 4.1 总原则

允许分叉的，不是灵魂，而是：

1. memory contract 的承载形态
2. prompt assembly 图
3. post-reply maintenance 路由
4. `self_runtime` authority 边界
5. operator / inspection / deep recall surface

所以最终结构是：

1. `SoulKernel`
2. `LinuxMemorySystem`
3. `EspMemorySystem`

### 4.2 `LinuxMemorySystem`

Linux 保留现有厚治理体系，职责是：

1. 完整 recall 面
2. 完整 operator / inspection 面
3. 更厚的 archive / private / autonomy / background governance
4. 更强的 benchmark / replay / regression / export / snapshot 能力

Linux 继续承载：

1. canonical shared memory
2. archive evidence
3. runtime skill / task learning
4. continuity capsules
5. full prompt recall assembly
6. full memory inspection / snapshot / restore
7. richer private / autonomy / background layers

Linux 的职责不是迁就 ESP，而是维持 Beetle 的高配形态、完整可观测性和治理厚度。

### 4.3 `EspMemorySystem`

ESP 不再被定义为 Linux 的“同制度低配版”，而被定义为：

> 围绕主体连续性、当前任务连续性、压缩长期记忆与按需外设显化而成立的稳定记忆系统。

ESP 主轴固定为三块：

1. **Public Soul Capsule**
   - 来自单一 `SoulKernel` 的最小公开投影，可直接进入主回复链
2. **Recent Task Continuity**
   - 当前任务、最近任务、下一步恢复能力
3. **Compressed Long Memory**
   - 规模受控、可被主回复直接消费的压缩长期记忆

### 4.4 `EspMemorySystem` 的主合同

ESP 默认主合同包含：

1. `execution_state`
2. `continuity_capsule`
3. `task_workspace`
4. `task_recall`
5. 轻量 `session_summary`
6. 受控长期记忆压缩块
7. `SoulKernel` 的 `Public Soul Capsule`

ESP 不再把下面这些默认定义为每轮同步参与的主合同：

1. 厚 `archive evidence`
2. 厚 `world_sense`
3. 厚 `autonomy_strategy`
4. `relationship_portfolio`
5. `relationship_topology`
6. `private_docs`
7. `private_garden`
8. 全量 operator deep inspection

说明：

1. 这不是“删除这些能力”
2. 这是把它们从“默认主合同”降为“后台层 / 诊断层 / Linux 厚治理层 / ESP 显式深路径”

### 4.5 灵魂显化分层：`L0 / L1 / L2`

从本次方案起，灵魂与记忆在运行态的显化必须分成三层，不再整包前置。

#### `L0 Public Capsule`

这是默认可进入主回复链的最小公开灵魂投影，至少包括：

1. `relationship_constitution` 的压缩结论
2. `disclosure_posture` 的当前结论
3. `mental_privacy` 的当前姿态摘要
4. `outer_voice` / persona 的稳定表达摘要
5. `self_continuity` 的最小连续性锚点
6. 当前 `session_summary / execution_state / task continuity`

要求：

1. Linux / ESP 默认都可直接消费
2. 可预算、可缓存、可恢复
3. 不依赖深层私域材料同步进 prompt

#### `L1 Governed Memory`

这是允许主链按需申请、经策略引擎放行后参与的受控记忆层，包括：

1. 受控长期记忆块
2. 关系记忆块
3. 任务事实块
4. 经治理后允许暴露的摘要化人格/关系材料

要求：

1. 不默认进入每轮 prompt
2. 必须通过 `PolicyEngine` 判定后再参与
3. 可在 Linux / ESP 上共用语义，但 ESP 默认参与规模更小

#### `L2 Private Core`

这是灵魂的深层私域核心，至少包括：

1. `inner_life`
2. `private_doc`
3. `private_garden`
4. `self_authored_core` 的深层工作区

要求：

1. 禁止默认进入主回复 prompt
2. 禁止主模型隐式读取
3. 只能经显式升级路径申请
4. ESP 不允许把它作为首轮同步主链的一部分

### 4.6 两端共享与分叉矩阵

#### 共享

1. `SoulKernel` 语义
2. memory object 的安全上界与持久化原则
3. factual memory 的治理原则
4. boundary / disclosure / relation 的制度
5. task continuity 的核心语义
6. mode / budget / admission 的基础合同

#### 分叉

1. 哪些层默认进 prompt
2. 哪些层同步、哪些层异步
3. 哪些层只在 ESP 上按需唤醒
4. 哪些 operator 面只属于 Linux 厚治理环境
5. 哪些长期记忆在 ESP 上只保留压缩表达，不保留厚证据面

### 4.7 治理判定：`PolicyEngine` 取代前置多轮裁决

从本次方案起，Beetle 的边界治理入口必须由确定性的 `PolicyEngine` 承担，而不是由独立前置 LLM 审判承担。

`PolicyEngine` 的输入必须是结构化字段，而不是 cue 词表：

1. `source_class`
   - `public_capsule / governed_memory / private_core`
2. `operation`
   - `reply / summarize / cite / disclose / quote / update_relationship / update_posture / request_private_access`
3. `requested_scope`
   - 当前会话、长期记忆、关系记忆、深层私域等
4. `relationship_state`
5. `disclosure_posture`
6. `mental_privacy_state`
7. `runtime_profile`
   - `linux_full / esp_compact`
8. `pressure`
   - 当前 orchestrator 资源压力

`PolicyEngine` 的输出固定为四种：

1. `allow`
2. `deny`
3. `safe_fallback`
4. `escalate`

这里的关键口径是：

1. **不是**“用户说了什么词就升级”
2. **而是**“系统这轮准备读取什么源、做什么操作、是否跨越高敏边界”

### 4.8 主链申请制：模型只能申请，不能自授予

主回复链后续必须改成“申请制”，而不是“隐式深读制”。

主模型允许提出的，是受控内部动作，例如：

1. `request_governed_memory`
2. `request_relationship_review`
3. `request_disclosure_review`
4. `request_private_source_access`

但这些动作只表示：

1. 主模型认为当前 turn 需要更多上下文或边界评审
2. 系统随后把申请交给 `PolicyEngine`
3. 真正的放行 / 拒绝 / 保守回退 / 升级治理由策略引擎决定

这条制度锁死两件事：

1. 主模型可以提出请求
2. 主模型不能自授予权限

### 4.9 P0 术语矩阵

`P0` 不是只把几个名词写进文档，而是要把这些名词和当前代码里的真实承载点绑定起来。

| 术语 | 最终定义 | 当前代码锚点 | P0 绑定结论 |
|---|---|---|---|
| `SoulKernel` | Linux / ESP 共享的唯一灵魂宪法、连续性体检与恢复合同 | `src/runtime/soul_kernel.rs`、`src/main.rs` 中 `ensure_platform_soul_kernel_recovery(...)` | 已锁定为不可分叉顶层合同 |
| `LinuxMemorySystem` | Linux 侧厚治理、厚 recall、厚 operator、厚 inspection 的完整记忆承载制度 | `src/memory/profile.rs` 中 `MemorySystemKind::LinuxFull`、`src/platform/linux/mod.rs`、`src/memory/prompt_context.rs`、`src/memory/self_runtime.rs` | `P1` 类型化已落地；厚路径仍主要沿现有 shared implementation 承载 |
| `EspMemorySystem` | ESP 侧围绕主体连续性、近期任务连续性、压缩长期记忆与按需能力显化而成立的紧凑记忆制度 | `src/memory/profile.rs` 中 `MemorySystemKind::EspCompact`、`src/platform/esp32.rs`、`src/memory/prompt_context.rs`、ESP runtime budget/admission | `P1` 类型化已落地；但 `P2/P3` 仍未把 ESP 热路径真正削成最终稳态 |
| `Soul Core` | ESP 上永远存在、不可裁掉的最小主体核 | `self_authored_core`、`relationship_constitution`、`self_continuity`、`mental_privacy`、`outer_voice` 在 `src/agent/loop/worker_context.rs` / `src/memory/prompt_context.rs` / `src/runtime/soul_kernel.rs` 的最小投影与恢复路径 | 属于 ESP 不可裁区 |
| `Public Soul Capsule` | 默认可进入主回复链的最小公开灵魂投影 | `src/runtime/soul_kernel.rs`、`src/agent/loop/worker_context.rs`、后续 `prepare_*_conversation(...)` 的公开装配入口 | Linux / ESP 默认共享语义，ESP 只允许这层首轮常驻 |
| `PolicyEngine` | 以 `source / operation / state` 做边界判定的确定性策略引擎 | 后续落点为 `src/agent/loop/worker_context.rs`、`src/memory/profile.rs`、runtime/mode/admission 共享字段 | 取代“主回复前额外 LLM 裁决” |
| `Resident Device Plane` | ESP 常驻的最小运行平面，负责 boot/recovery、唯一 `agent_loop`、最小 config/health/recovery 与最小记忆装配 | `src/main.rs` 的 `run_app(...)` 装配、`AgentLoopConfig`、`HandlerContext`、单一 `run_agent_loop(...)` | 属于 ESP 稳态常驻区 |
| `Optional Capability Planes` | `audio / camera / sensors / heavy operator / deep inspection` 等跨平台能力平面 | `Platform` trait 中 `hardware_discovery / init_audio / wifi_scan / audio_*` 等能力入口，后续扩展继续挂在 platform/capability 层 | 属于可裁、可禁、可按需挂载区 |

必须强调两点：

1. `LinuxMemorySystem` / `EspMemorySystem` 现在已经有了显式 `MemorySystemKind` 类型表达；`P1` 的“只靠 `MemoryProfile` 表达制度差异”这一缺口已经补上。
2. 但类型落地不等于运行时已经彻底分叉；`P2/P3` 仍要继续把 ESP 的热路径、authority 与厚治理真正从共享重链里分离出来。

### 4.10 当前代码锚点与真实调用链

为了避免后续再出现“文档说双轨，代码其实没人知道从哪分”的问题，`P0` 把当前真实链路固定如下。

#### A. 系统装配入口

1. `src/main.rs` 的 `run_app(...)`
2. `run_app(...)` 负责：
   - 注册 `Platform::memory_snapshot()` 给 orchestrator
   - 装配全部 memory stores / task stores / channel/runtime 依赖
   - 初始化 `SkillPromptCache`
   - 执行 `ensure_platform_soul_kernel_recovery(...)`
   - 生成 `AgentLoopConfig`

也就是说，当前 Beetle 的“单灵魂、多 store、单 agent plane”总装配权威仍在 `run_app(...)`。

#### B. 平台与能力边界

1. `src/platform/abstraction.rs` 的 `Platform` trait 是当前唯一平台隔离边界。
2. 它已经同时暴露：
   - memory stores
   - HTTP client
   - WiFi / LAN / discovery
   - audio 读写与 capability 能力
   - config / state_fs / restart / OTA

这说明：

1. 平台隔离边界已经成立
2. `audio / camera / sensors` 后续继续走 capability plane，不需要绕开 `Platform` 再造一套侧门

#### C. 首条消息热路径

当前板上真正会打到栈和 prompt 厚度的链路不是抽象名词，而是：

1. `run_agent_loop(...)`
2. `prepare_worker_conversation(...)`
3. `decide_prompt_participation(...)`
4. `load_prompt_memory_context(...)`
5. `sync_relationship_constitution(...)` / disclosure / persona governance
6. `build_context(...)`

其中：

1. `AgentLoopConfig` 在 `src/agent/loop.rs` 中把所有 stores 与 runtime provider 汇总给 agent plane
2. `src/agent/loop/worker_context.rs` 是当前首消息准备链的真实总入口
3. `src/memory/prompt_context.rs` 是当前 memory assembly 的真实装配枢纽
4. `src/agent/context.rs` 是最终 system prompt / messages 合成入口

因此，后续 `P1-P3` 的制度分叉必须沿这条真实链路落地，而不是只改外围文档。

#### D. 当前“嵌入式/标准”表达的真实位置

当前代码已经有 `LinuxMemorySystem` / `EspMemorySystem` 的显式制度类型：

1. `src/memory/profile.rs` 中的 `MemorySystemKind::{LinuxFull, EspCompact}`
2. `src/platform/linux/mod.rs` / `src/platform/esp32.rs` 上的平台级 `memory_system_kind()`

但当前运行时差异表达仍主要依赖下面这层装配与参与策略：

1. `MemoryProfile`
2. `PromptParticipationPlan`
3. `decide_prompt_assembly(...)`
4. `load_prompt_memory_context(...)`

这意味着现在的真实状态是：

1. 已能表达 Linux / ESP 属于两套正式 `MemorySystem`
2. 也已能表达某些装配层、authority 与 capability contract 的制度差异
3. 但很多重链路仍共享同一套主实现骨架，并没有彻底分成“两套运行时”

尤其需要明确：

1. `load_prompt_memory_context(...)` 虽然已经按 `MemorySystemKind` 分入口，但 Linux / ESP 当前仍共同落到大量共享 inner 逻辑
2. `prepare_worker_conversation(...)` 仍然在 ESP 用户首轮热路径中同步执行 disclosure / prompt memory / governance 相关重装配

所以当前真正没做完的是 `P2/P3` 的运行时削厚，而不是 `P1` 的类型化。

#### E. 当前后台治理与灵魂恢复锚点

1. `src/memory/self_runtime.rs` 的 `run_self_runtime(...)` 是当前后台成长、压缩与治理执行器
2. `src/runtime/soul_kernel.rs` 的 `SoulKernelStatus` / `SoulKernelRecoveryReport` 是当前单灵魂连续性与恢复合同
3. `src/platform/http_server/handlers/mod.rs` 的 `HandlerContext` 是当前常驻 config / recovery / health control plane 共享上下文
4. `src/skills/prompt_cache.rs` 的 `SkillPromptCache` 是当前 resident prompt-side cache 锚点

这意味着当前系统已经具备：

1. 单灵魂恢复合同
2. 单 agent 执行面
3. 最小常驻 control plane
4. prompt 装配缓存化

但还没有把 ESP 的重治理主回复热链真正削成最终稳态：

1. ESP 首轮主链仍会在 `prepare_worker_conversation(...)` 里同步碰 disclosure / governance / prompt assembly 重步骤
2. `self_runtime authority` 虽然已有制度边界，但 ESP 背景成长仍未完全收成“轻量但不越权”的最终形态

这正是 `P2/P3` 仍未完成的真实边界。

### 4.11 不可分叉 / 可分叉落地矩阵

`P0` 不是抽象地说“灵魂不分叉、承载可分叉”，而是要把当前代码中哪些东西未来不能分、哪些东西必须允许分写死。

| 领域 | 是否允许分叉 | 当前代码锚点 | P0 结论 |
|---|---|---|---|
| `SoulKernel` 连续性与恢复合同 | 否 | `src/runtime/soul_kernel.rs` | Linux / ESP 必须共享 |
| `self_authored_core` / `relationship_constitution` / `self_continuity` 的主体宪法语义 | 否 | `src/memory/prompt_context.rs`、`src/agent/loop/worker_context.rs` | Linux / ESP 必须共享 |
| disclosure / boundary / persona priority 的裁决语义 | 否 | `worker_context.rs` 中 disclosure / governance / persona adjudication | Linux / ESP 必须共享制度语义 |
| 是否默认每轮前置多轮裁决 | 否 | 当前仍残留在 `prepare_worker_conversation(...)` 的同步重治理路径 | 后续必须统一改为“默认单轮主回复 + 显式升级治理” |
| factual memory 治理原则 | 否 | `prompt_context.rs`、`self_runtime.rs`、memory governance 相关链路 | Linux / ESP 必须共享治理原则 |
| prompt 默认参与层级 | 是 | `MemoryProfile` / `PromptParticipationPlan` / `decide_prompt_participation(...)` | Linux / ESP 后续必须显式分叉 |
| background governance 厚度 | 是 | `load_prompt_memory_context(...)`、`run_self_runtime(...)` | Linux 可厚，ESP 必须紧凑 |
| operator / inspection 厚度 | 是 | `HandlerContext`、HTTP handlers、operator surfaces | Linux 可厚，ESP 预算化/显式化 |
| capability 挂载方式 | 是 | `Platform` trait 与后续 capability plane | Linux discovery-first，ESP config/feature-gated |
| 是否默认常驻 | 是 | `run_app(...)` / resident threads / optional capabilities | ESP 只能保留最小常驻面 |

### 4.12 P0 完成口径

本次 `P0` 完成的定义不是“代码已经有了双轨 runtime”，而是以下四件事全部成立：

1. 顶层宪法已经锁死：
   - 单一 `SoulKernel`
   - 双轨 `MemorySystem`
   - ESP 三层运行结构
   - 跨平台 `Optional Capability Plane`
2. 顶层术语已经和当前真实代码锚点绑定，不再悬空。
3. “哪些不能分叉、哪些必须允许分叉”已经写成落地矩阵。
4. 已明确当前代码仍只显式拥有 `MemoryProfile`，后续 `P1` 必须把它升级为正式 `MemorySystemKind`，而不是继续靠文档代替类型。

---

## 5. ESP 三层运行结构

### 5.1 总原则

ESP 版 Beetle OS 必须固定区分三层：

1. `Soul Core`
2. `Resident Device Plane`
3. `Optional Capability Planes`

这三层是 ESP 稳态成立的核心。

### 5.2 `Soul Core`

`Soul Core` 是 ESP 上永远存在、不可裁掉的最小主体核。

必须包括：

1. `SoulKernel` Constitutional Soul 持久资产
2. 最小 prompt soul projection
3. 恢复所需的 continuity anchors
4. boundary / disclosure 可执行状态
5. 主体恢复与回退所需的最小 soul snapshot

这层要求：

1. 设备重启后可恢复
2. agent 重启后可恢复
3. 不依赖 camera / sensors / voice 是否启用
4. 不因外设能力裁剪而失真

### 5.3 `Resident Device Plane`

这是 ESP 允许常驻的最小运行平面。

必须包含：

1. boot / recovery / supervisor 语义
2. 单一 `agent_loop` 重执行面
3. 最小 config / recovery / health 面
4. 必要 display / audio state feedback
5. 已启用通道所需的最小通信面
6. 最小 `EspMemorySystem` 主合同装配能力

严格限制：

1. 不新增第二条常驻重执行面
2. 不把 deep inspection / heavy governance 放进常驻面
3. 不把 camera / sensors 默认升为常驻主链负担

### 5.4 `Optional Capability Planes`

`Optional Capability Plane` 不是 ESP 私有补丁层，而是 Beetle OS 的跨平台能力平面。

所有外设和重型能力都必须进入这层。

包括：

1. voice / realtime audio
2. camera / vision ingest
3. sensors / environment ingest
4. heavy operator / diagnostic surfaces
5. deep archive / snapshot / inspection

这类平面作为 OS 级概念，必须满足三条硬约束：

1. **编译可裁**
2. **未配置零初始化**
3. **启用后受 mode / budget / admission 控制**

### 5.5 外设能力统一规则

从今天开始，`audio / camera / sensors` 一律按跨平台 capability plane 处理。

平台差异只体现在挂载策略：

#### A. Linux：discovery-first

Linux 侧后续应默认采用：

1. 自动扫描 / 枚举硬件
2. 将扫描结果注册为可选 capability candidates
3. 由配置页或 operator 选择：
   - 启用
   - 覆盖默认选择
   - 显式禁用
4. 配置页是选择 / 覆盖层，不是唯一硬件事实来源

#### B. ESP：config/feature-gated

ESP 侧后续继续采用：

1. 编译 feature 决定能力是否存在
2. 运行配置决定能力是否挂载
3. 未启用时零初始化、零副作用
4. 已启用后再进入统一 runtime 治理

#### C. 未配置时零初始化

如果能力未配置或未被挂载：

1. 不初始化驱动
2. 不创建线程
3. 不申请总线 / GPIO / DMA
4. 不注册热路径 handler
5. 不参与 prompt / memory / runtime mode

#### D. 打包时可裁掉

如果这个硬件 SKU 根本不需要：

1. 编译期不带相关 capability feature
2. 不带相关依赖
3. 不带相关配置入口
4. 不把不可用能力伪装成 disabled 占位

#### E. 启用后仍受治理

即使用户启用了：

1. 也不代表常驻前台厚跑
2. 也不代表可以无条件进入 prompt / memory
3. 必须通过 runtime mode / orchestrator / admission 进入系统

---

## 6. 感知与外设架构：如何同时支持 audio + camera + sensors 仍然稳

### 6.1 总原则

ESP 稳态成立的关键，不是“把所有外设都挂上”，而是：

> 让所有外设都以统一的、受治理的能力平面进入主体，而不是直接变成主回复热路径厚度。

而 Linux 侧的关键则是：

> 让所有外设先被发现、再被选择、最后被挂载，而不是把静态配置当成唯一设备事实。

### 6.2 感知管线的统一模型

所有外设输入都应走同一抽象：

1. `Capability Driver`
2. `Observation Normalizer`
3. `Admission / Budget Gate`
4. `Observation Bus`
5. `Memory Ingestion Gateway`
6. `Agent / Self-Runtime Consumer`

含义是：

1. 驱动负责读硬件
2. 归一化层负责把原始输入压成结构化 observation
3. admission 决定这条 observation 是否值得进入更贵路径
4. memory gateway 决定它是否值得写进 memory plane
5. agent / self-runtime 决定它是否进入当轮行动

### 6.3 不允许的错误路径

以下路径一律禁止：

1. camera 原始流直接进入 prompt
2. sensor 原始采样直接进入长期记忆
3. 每条 observation 都触发 LLM
4. 每启用一个外设就新增一条常驻重线程
5. 为了让 capability 可用，默认把其运行面常驻化

### 6.4 外设对 memory system 的写入原则

外设不能绕过 memory governance。

规则如下：

1. 原始 observation 先进入短时事件面
2. 只有经过归纳 / 聚合 / 预算审查后的结果，才能进入长期记忆
3. 事实类 observation 仍走 factual governance
4. 主体成长类 observation 只能作为 `self_runtime` 的输入证据，不能直接改写 `SoulKernel`

### 6.5 camera 的系统定位

camera 在 ESP 上是重要能力，但不是默认常驻人格层。

它的定位是：

1. perception capability plane
2. 支持 scene observation / object hint / visual context
3. 仅在启用且命中场景时进入更贵路径
4. 不直接定义主体，不直接替代 soul / relation / boundary

因此：

- camera 必须支持
- camera 不得定义灵魂
- camera 不得默认把系统拖入 Linux 厚运行态

### 6.6 sensors 的系统定位

sensors 不是“越多越好”的总线装饰，而是主体与环境的低成本感觉器官。

其定位是：

1. 轻 observation source
2. 触发 `world / device / ambient` 变化信号
3. 默认走 cheap path
4. 只有显著事件才进入更贵的记忆与 agent 路径

---

## 7. `self_runtime` authority：谁可以直接写，谁不可以

### 7.1 总原则

`self_runtime` 在 ESP 上必须更强，但不能变成“后台自由改写一切事实的第二主魂”。

所以要明确区分：

1. **主体成长类资产**
2. **事实 / 任务类资产**

### 7.2 可由 `self_runtime` 直接经营的范围

`self_runtime` 可以直接经营或主导压缩的，限于：

1. `SoulKernel` 的深层工作区材料
2. `self_model` 的深层扩展
3. `inner_life`
4. `outer_voice` 的风格刷新
5. `self_continuity` 的长期桥接状态
6. 与边界相关的长期关系痕迹
7. 方法论型 task / skill 蒸馏结果

### 7.3 不能被 `self_runtime` 直接自由改写的范围

以下内容继续受治理合并约束：

1. 用户事实
2. 用户偏好
3. 项目事实
4. 任务事实
5. 外部稳定事实
6. 高风险关系事实

换句话说：

> `self_runtime` 可以成长，  
> 不能伪造事实。

### 7.4 ESP 上的 authority 收口

ESP 的 `self_runtime` 必须被正式定义为：

1. 灵魂成长压缩器
2. 近期连续性维护器
3. 方法论沉淀器
4. 非事实层的后台经营者

不能被定义为：

1. 第二个全量 recall runtime
2. 任意 factual truth 改写器
3. 无限扩张的后台自治平面

---

## 8. Prompt Assembly：ESP 不再只是“参与少一点”

### 8.1 总原则

ESP 不再沿用“Linux 全量装配图 + participation policy 减一点”的长期路线。

后续必须正式形成两套装配入口：

1. `LinuxPromptAssembly`
2. `EspPromptAssembly`

### 8.2 Linux 装配原则

Linux 继续保留：

1. 更厚 recall
2. 更厚 operator 可观测性
3. 更厚 background governance
4. 更多 deep path 参与能力

### 8.3 ESP 装配原则

ESP 装配图固定遵守：

1. `L0`：`Public Soul Capsule` + Recent Task Continuity
2. `L1`：命中策略后才参与的 `Governed Memory`
3. `L2`：`Private Core`、后台经营、diagnostic-only、explicit deep inspection

这不是“优化策略”，而是 ESP 的正式制度。

### 8.4 首轮主路径要求

ESP 首轮主路径必须满足：

1. 不同步装配厚背景治理
2. 不同步装配 deep soul workspace
3. 不同步枚举 heavy operator surface
4. 不因启用了 camera / sensors 就额外拉厚 prompt
5. 不同步跑独立前置治理 LLM

### 8.5 第一阶段已完成：`worker_context` 首轮热路径收口（2026-04-08）

本阶段第一段编码已经完成，范围限定在 `src/agent/loop/worker_context.rs` 主入口与对应测试。

已确认落地的行为边界：

1. `prepare_worker_conversation(...)` 继续作为真实总入口，但 Linux / ESP 的首轮治理参与边界已正式分开
2. `EspCompact` 的**首轮用户消息**已停止同步执行：
   - `mental_privacy disclosure adjudication`
   - dynamic `persona_priority adjudication`
   - `relationship_constitution sync` 写回路径
3. `EspCompact` 首轮仍保留：
   - `prompt_context` 内的只读派生 `relationship_constitution`
   - `Public Soul Capsule`
   - session / task continuity
   - compressed governed recall
4. `LinuxFull` 仍保留同步 disclosure / relationship constitution / persona 治理路径，未被一起降配

这一阶段完成后的实际效果：

1. ESP 首轮主链已经回到“单轮主回复优先”的可控基础形态
2. ESP 首轮不再为了维持旧制度而在热路径里做同步关系宪法写回
3. Linux 仍保持 full runtime 的同步深治理能力
4. 下一阶段可以直接进入 `prompt_context` / compact assembly 的实质性收口，而不是继续在旧热链上叠补丁

必须明确：

1. 这**仍然不等于** `P2` 已全部完成
2. 当前完成的是 `worker_context` 第一阶段收尾，不是整个 ESP prompt slimming 收尾
3. 下一阶段主线应直接转向 `prompt_context` / `build_context` / `L0/L1/L2` 的真正 compact assembly 收口

### 8.6 第二阶段已完成：`prompt_context / build_context` compact assembly 收口（2026-04-08）

本阶段第二段编码已经完成，范围限定在：

1. `src/memory/prompt_context.rs`
2. `src/memory/profile.rs`
3. `src/agent/context.rs`

已确认落地的行为边界：

1. `EspCompact` 首轮用户消息默认不再注入 capability package
2. `EspCompact` 首轮继续保留：
   - `Public Soul Capsule`
   - session / execution / active task continuity
   - compressed long-term recall
3. `EspCompact` 首轮默认不再读取厚 `archive evidence / runtime skill / background governance / private depth`
4. 若已存在持久化 `relationship_constitution`，`EspCompact` 首轮默认直接复用，不再为首轮 prompt 去读取 `relationship_topology / outer_voice / mental_privacy` 做关系治理重建
5. `LinuxFull` 仍保留 capability package 与关系治理重建读取路径，未被一起降配

这一阶段完成后的实际效果：

1. ESP 首轮 prompt 已从“共享厚装配骨架”收成真正的 compact contract
2. `worker_context` 的首轮减负现在与 `prompt_context / build_context` 的 compact assembly 已经闭合
3. capability plane 不会因为启用而自动拉厚 ESP 首轮 prompt
4. 下一阶段可以直接进入 `self_runtime authority` 收口，而不需要再回头补 prompt 主链

---

## 9. Operator / Recovery / Packaging 规则

### 9.1 Linux 与 ESP 的 operator 面不再强制等厚

Linux 可以保留：

1. full memory status
2. snapshot / restore
3. deep inspection
4. archive browsing
5. heavy diagnostics

ESP 必须收成：

1. 最小健康面
2. 最小恢复面
3. 显式 deep inspection
4. 活跃窗口预算化

### 9.2 Packaging 规则

从本文起，Beetle OS 必须支持按能力打包。

至少要支持：

1. `core-only`
2. `voice`
3. `vision`
4. `sensor`
5. `full-esp`
6. `linux-full`

要求：

1. 不把未编译的能力伪装成运行时 disabled
2. 不把未配置的能力做成“默认起线程再空转”
3. capability matrix 必须在配置、构建、health、operator 口径上一致

### 9.3 Recovery 规则

ESP 上 recovery 的首要目标不是“把所有能力都救回来”，而是：

1. 先保 `Soul Core`
2. 再保 agent 执行面
3. 再保基础对外交互
4. 最后逐步恢复 optional capability planes

恢复顺序不能反过来。

---

## 10. 非目标

本文明确拒绝以下方向：

1. 两套灵魂、两套人格宪法
2. 为了迁就 ESP，把 Linux 也降成小系统
3. 为了支持 camera / sensors，把这些能力默认常驻前台
4. 为了省事，让 `self_runtime` 成为事实任意改写器
5. 继续把 ESP 当“Linux memory contract 的无限 slimming 分支”
6. 继续靠“再加栈 / 再减一点加载”当长期主路线

---

## 11. `P0-PX` 实施路线

下面的阶段不是建议顺序，而是 Beetle OS 接下来必须执行的正式路线。

### 11.0 当前阶段状态总表（按真实代码对齐）

| 阶段 | 当前状态 | 说明 |
|---|---|---|
| `P0` | 已完成 | 顶层宪法、术语矩阵、分叉边界已锁死 |
| `P1` | 已完成 | `MemorySystemKind::{LinuxFull, EspCompact}` 与平台装配已进入主线 |
| `P2` | 已完成 | `worker_context + prompt_context + build_context` 的首轮 compact assembly 已收口 |
| `P3` | 部分完成 | authority 分叉已建模，但 ESP 后台成长边界仍未完全收口 |
| `P4` | 已完成 | capability plane、Linux `discovery-first`、ESP `config/feature-gated` 已落地 |
| `P5` | 已完成 | operator / recovery / windowed inspection 已有正式收口 |
| `P6` | 已完成 | packaging / feature matrix / health / operator 口径已统一 |
| `P7` | `N/A` | 当前均为测试机，无线上存量与升级迁移压力 |
| `P8` | 已完成 | acceptance suite、os_closure、health / operator / doctor 共用口径已形成 |
| `PX` | 未完成 | 板级最终验收仍以后续真实上板稳定结果为准 |

必须单独强调：

1. `P3` 没做完，不等于方案无效；它表示**架构方向已锁定，但 ESP authority / 后台成长边界仍未收尾**
2. 当前不能把本文解读成“ESP 已经完成最终稳态化”或“ESP 已经完成功能削减”
3. 当前更准确的表述是：**制度分轨与首轮 compact assembly 已成立，ESP 最终稳态化仍取决于 `P3` 收口**

### 11.1 下一步主线（第二阶段完成后）

如果继续执行本文主方案，下一步默认从下面两块开始。

#### A. `worker_context / prompt_context / build_context` 两阶段已完成，后续不再回头补首轮 prompt 热链

当前已经完成的边界如下：

1. ESP 首轮同步 disclosure / persona / relationship constitution sync 已移出热路径
2. ESP 首轮 compact assembly 已正式收成 `Public Soul Capsule + continuity + compressed recall`
3. Linux 仍保留同步深治理与 full runtime prompt
4. 这三条链路后续只做配合型清理，不再作为主战场

因此，下一步不再回头继续在首轮 prompt 热链上堆新的 `if esp`，而是直接进入 authority 收口。

#### B. 收紧 `self_runtime authority`

ESP 的 `self_runtime` 只继续承担：

1. soul growth
2. continuity maintenance
3. method distillation

它不再承担“主回复前重治理预计算器”的角色。

重治理若仍需要存在，应改为：

1. 后台计算
2. digest 化缓存
3. 显式深路径读取
4. 非首轮再参与

#### C. 固化 capability plane 的 prompt 侵入边界

后续 `audio / camera / sensors` 都必须先进入：

1. discovery / config gate
2. normalized observation
3. admission
4. explicit projection

而不是“能力一启用，就默认把上下文塞进 agent 主链”。

### 11.2 下一步验收口径（待确认）

下一步不是以“代码看起来更轻了”验收，而以下面几条为准：

1. ESP 首轮用户消息不再同步跑 disclosure adjudication
2. ESP 首轮不再同步跑 dynamic persona adjudication
3. ESP 首轮不再默认读取厚 private / relation / world 块
4. capability plane 启用后不自动加厚首轮 prompt
5. ESP 仍保有完整主体资格，不退化成 Linux 附属终端
6. Linux 仍保留 full runtime，不因 ESP 削厚而降配
7. 默认主链不存在“先额外跑一轮 LLM 判定要不要治理”的前置审判
8. 主链对高敏边界的访问必须改成“显式申请 -> `PolicyEngine` 决定”

### P0：锁死顶层宪法

**目标**

把“单一 `SoulKernel`、双轨 `MemorySystem`、ESP 三层结构、optional capability planes”写成系统级唯一口径。

**必须产物**

1. 本文定稿
2. `beetle-os-plan.md` / `memory-enhancement-plan.md` / `esp-runtime-resource-governance.md` 的冲突口径修订
3. 术语矩阵：
   - `SoulKernel`
   - `LinuxMemorySystem`
   - `EspMemorySystem`
   - `Soul Core`
   - `Resident Device Plane`
   - `Optional Capability Planes`
4. 当前代码锚点与真实调用链绑定
5. 不可分叉 / 可分叉落地矩阵

**验收**

1. 后续文档不再同时出现“单合同 shared memory”与“双制度 memory system”两种冲突叙事
2. 灵魂不可分叉范围被明确写死
3. 后续编码阶段不再可以用“profile 先顶着、以后再说”来回避 `MemorySystemKind` 的正式建模

### P1：建立 `SoulKernel` 与 `MemorySystemKind`

**目标**

把“灵魂合同”和“记忆承载合同”从类型层正式拆开。

**核心动作**

1. 引入 `MemorySystemKind`
   - `LinuxFull`
   - `EspCompact`
2. 明确 `SoulKernel` 投影接口
3. 让 platform / app assembly 可以同时知道：
   - 当前平台是谁
   - 当前 memory system 是谁

**要求**

1. 不从 `Platform` 底层先硬分叉
2. 先从 contract / assembly / authority 层分叉

**验收**

1. 不再只靠 `MemoryProfile::Embedded / Standard` 表达制度差异
2. 类型边界能明确表达“同一灵魂，不同记忆承载”

### P2：重写 Prompt Assembly

**目标**

从“参与多少”升级到“装配图不同”。

**核心动作**

1. Linux / ESP 两套装配入口
2. ESP 固定 `L0 / L1 / L2` 分层
3. Linux 保留厚路径

**ESP 验收**

1. 首轮只装：
   - `L0 Public Capsule`
   - session grounding
   - task continuity
   - 压缩长期记忆
2. camera / sensors 启用后也不默认加厚首轮 prompt

### P3：重写 ESP `self_runtime` authority

**目标**

让 ESP 的后台成长逻辑更强，但不越权。

**核心动作**

1. 明确可直接经营的成长层
2. 明确 factual / task facts 继续治理合并
3. 把 ESP 的 `self_runtime` 收成：
   - soul growth
   - continuity maintenance
   - method distillation

**验收**

1. `self_runtime` 不再需要假装所有成长都是 factual merge
2. 同时也不能绕过事实治理

### P4：把 audio / camera / sensors 全部平面化

**目标**

把所有外设从“隐式系统厚度”改成“显式 capability planes”。

**核心动作**

1. 建立统一 capability registration
2. 建立统一 observation bus / normalization / admission
3. 建立 Linux `discovery-first` 与 ESP `config/feature-gated` 的统一挂载模型

**硬约束**

1. 未配置零初始化
2. 编译不带则零存在
3. Linux 自动扫描不等于自动常驻挂载
4. 已启用也不默认常驻前台厚跑

**验收**

1. 任一 capability 未配置时，不产生线程、驱动、总线、副作用
2. Linux 上 capability 可以先被发现，但只有被选择 / 挂载后才进入运行面
3. 任一 capability 不被打包时，不留下伪 disabled 路径

### P5：重写 ESP Control / Recovery / Operator 面

**目标**

让 ESP 保住最小恢复与 operator 能力，但不再维持 Linux 厚管理面。

**核心动作**

1. 最小常驻 control plane
2. deep inspection 显式化
3. operator 面预算化、窗口化
4. capability plane 的健康状态进入统一 health

**验收**

1. 设备崩溃时先保 `Soul Core`
2. deep operator path 不再成为常驻厚度来源

### P6：建立 Packaging / Feature Matrix

**目标**

把不同硬件 SKU 与不同能力包变成正式构建能力。

**至少支持**

1. core-only
2. voice
3. vision
4. sensor
5. voice+vision+sensor
6. linux-full

**验收**

1. 构建、配置、health、operator 四个口径一致
2. 文档与代码不再出现“配置 disabled 但后台其实已初始化”的行为

### P7：迁移现有 store / config / runtime

**目标**

把现有 shared-contract 时代的代码平滑迁到双轨体系，不丢主体资产。

**核心动作**

1. store 迁移与兼容层
2. config 迁移
3. memory inspection 迁移
4. old prompt assembly 到新双轨装配的平滑替换

**验收**

1. 旧设备升级后不丢 `SoulKernel` 核心资产
2. 旧 Linux / ESP 数据仍可被新系统读取或安全迁移

**当前说明**

1. 对当前测试机批次，本阶段不是主线阻塞项
2. 在没有正式线上存量、没有发布升级路径之前，应视为 `N/A`
3. 只有进入正式发布与升级周期后，`P7` 才重新成为必须执行的真实工程

### P8：全系统回归与压力验证

**目标**

在代码层证明双轨体系没有把 Beetle OS 拉裂。

**必须覆盖**

1. soul contract regression
2. relation / disclosure regression
3. memory routing regression
4. packaging regression
5. capability enable / disable regression
6. Linux / ESP 语义一致性 replay

**验收**

1. 同一主体在 Linux / ESP 上的人格合同一致
2. capability plane 的启停不会污染 Soul Core 与 MemorySystem 主轴

### PX：板级最终验收

**目标**

不是证明“能启动”，而是证明“作为主体设备能长期稳态存在”。

**ESP 最终验收必须回答六个问题**

1. `Soul Core` 是否稳定存在并可恢复
2. 首轮用户消息是否不再走 Linux 厚路径
3. `audio + camera + sensors` 是否都能按需启用
4. 未配置 / 未打包能力是否零副作用
5. 各 capability plane 是否受统一 mode / budget / admission 治理
6. 设备在长时间运行下是否仍保持主体连续性，而不是退化成一堆外设线程

**只有 PX 通过，ESP 版 Beetle OS 才算成立。**

---

## 12. 最终判断

这份文档把 Beetle OS 的路线彻底说死：

1. **不做双魂**
2. **做双轨记忆承载**
3. **ESP 不是 Linux 记忆体系的无限 slimming**
4. **ESP 要作为独立成立的稳态主体设备形态存在**
5. **audio / camera / sensors 全支持，但一律能力平面化**
6. **灵魂常驻，外设按需显化，能力可裁打包**

因此，后续 Beetle OS 的正确方向不再是：

> “还要不要再少加载一点 background governance”

而是：

> “如何在不分裂灵魂的前提下，让 Linux 与 ESP 用不同记忆承载制度，分别活成最适合自己的 Beetle 形态。”
