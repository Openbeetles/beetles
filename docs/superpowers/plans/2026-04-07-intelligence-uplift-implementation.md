# Intelligence Uplift Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Turn `dev-docs/intelligence-uplift-plan.md` into a staged, code-first workstream that can be implemented on the current Beetle codebase without conflicting with the Beetle OS master plan in `dev-docs/beetle-os-plan.md`.

**Architecture:** This is a subordinate capability workstream, not a replacement for Beetle OS `P0-P8`. The Beetle OS master plan has already completed system `P0-P6` and is entering `P7-P8`; this implementation plan therefore treats intelligence uplift as a local `P0-P6` sequence that lands inside the existing single-agent-plane runtime, memory, and replay architecture. Each stage must produce a working, testable slice on the current main path before the next stage starts.

**Tech Stack:** Rust, serde, existing `agent` / `memory` / `llm` modules, current SPIFFS/Linux stores, existing CLI and replay/inspection tooling.

---

## Scope And Stage Alignment

This plan deliberately follows the Beetle OS master-plan discipline:

- It does **not** introduce a second agent plane, second personality authority, or second memory authority.
- It treats Linux and ESP as one semantic species with different implementation thickness.
- It assumes system `P0-P6` from `dev-docs/beetle-os-plan.md` are already done and positions this workstream mainly as an intelligence sub-plan for Beetle OS `P7` and `P8`.

To avoid confusion with Beetle OS master numbering, the stages below are named `Intelligence P0-P6`. They are still a true `P0-Px` implementation sequence, but they are local to the intelligence-uplift workstream.

## File Map

### New Files

- `src/agent/subject_state.rs`
  Responsibility: deterministic `SubjectState` and `SubjectStateDigest` definitions plus compiler logic over existing persona/relationship/runtime inputs.
- `src/cli/intelligence_eval.rs`
  Responsibility: Linux-side replay/eval entrypoints for first-useful-answer, react-round, tool-convergence, and continuity-sensitive regressions.

### Existing Files To Modify

- `src/agent/mod.rs`
  Responsibility: expose any new `subject_state` module APIs needed by the rest of the agent layer.
- `src/agent/context.rs`
  Responsibility: consume `SubjectState` and keep compact-state injection on the current main prompt path.
- `src/agent/request_plan.rs`
  Responsibility: later-stage integration point if `TurnExecutionClass` must influence tool policy after the control point is moved earlier.
- `src/agent/tool_guidance.rs`
  Responsibility: extend current successful tool-round observations toward typed observation summaries and convergence guidance.
- `src/agent/loop.rs`
  Responsibility: own top-level worker-path sequencing, turn-level execution class, replay-write order, and background-job linkage.
- `src/agent/loop/worker_context.rs`
  Responsibility: compile and inject `SubjectState` inside `prepare_worker_conversation`.
- `src/agent/loop/tool_round.rs`
  Responsibility: emit synchronous tool-round observations and replay-ready tool-path summaries.
- `src/agent/loop/turn_finalize.rs`
  Responsibility: persist synchronous replay fields and enqueue post-turn append/update work for asynchronous observations.
- `src/memory/turn_ledger.rs`
  Responsibility: extend the current turn-ledger schema/history with replay-oriented synchronous and appendable fields.
- `src/memory/prompt_context.rs`
  Responsibility: consume minimal working-set summaries without turning `ExecutionState` into an unbounded second store.
- `src/memory/execution_state.rs`
  Responsibility: hold the minimal working-set summary that directly improves next-turn decisions.
- `src/memory/maintenance.rs`
  Responsibility: refresh working-set summaries and later append asynchronous replay observations.
- `src/memory/self_runtime.rs`
  Responsibility: emit appendable self-runtime observations rather than silently mutating state with no replay linkage.
- `src/memory/recall_inspection.rs`
  Responsibility: expose measurable recall/replay inspection data for the new evaluation loop.
- `src/memory/recall_rerank.rs`
  Responsibility: optional later-stage rerank hook; must remain Linux-only and non-authoritative.
- `src/memory/archive_search.rs`
  Responsibility: expose replay/eval-friendly archive traces and candidate metadata.
- `src/memory/continuity_capsule.rs`
  Responsibility: surface continuity-sensitive outcomes into replay/eval without creating a second continuity authority.
- `src/llm/compat.rs`
  Responsibility: hold compatibility helpers if execution-class-aware dispatch needs model capability checks.
- `src/llm/mod.rs`
  Responsibility: later-stage execution-class-aware dispatch and optional stronger reasoning lane selection.
- `src/cli/mod.rs`
  Responsibility: register any new intelligence-eval command.

### Existing Tests To Extend

- `src/agent/request_plan.rs`
- `src/agent/loop.rs`
- `src/memory/execution_state.rs`
- `src/memory/prompt_context.rs`
- `src/memory/maintenance.rs`
- `src/memory/turn_ledger.rs`
- `src/llm/fallback.rs`

## Stage Table

| Stage | Core Goal | Depends On | Primary Output | Exit Gate |
| --- | --- | --- | --- | --- |
| `Intelligence P0` | Freeze metrics, contracts, and stage boundaries | none | baseline eval cases + replay contract doc-in-code | team can measure improvement before changing behavior |
| `Intelligence P1` | Land deterministic `SubjectState` on the current prompt path | `Intelligence P0` | compact subject-state compiler + digest | main reply path consumes `SubjectState` without changing agent-plane topology |
| `Intelligence P2` | Land synchronous replay skeleton and typed tool observations | `Intelligence P0`, `Intelligence P1` | replay-ready `TurnLedger` fields + sync observation extraction | turn history explains what happened in one turn without async ambiguity |
| `Intelligence P3` | Upgrade `ExecutionState` into minimal working-set summary | `Intelligence P1`, `Intelligence P2` | tighter active-task context | next-turn decisions improve without exploding `ExecutionState` scope |
| `Intelligence P4` | Introduce gated hard-turn execution class and recovery-aware policy inputs | `Intelligence P1`, `Intelligence P2`, `Intelligence P3` | deterministic `TurnExecutionClass` + policy wiring | hard turns can be identified without adding a second agent loop |
| `Intelligence P5` | Add asynchronous replay append path and continuous eval loop | `Intelligence P2`, `Intelligence P3`, `Intelligence P4` | maintenance/self-runtime replay append + CLI eval | sync and async intelligence signals are auditable and replayable |
| `Intelligence P6` | Add optional Linux-only rerank/calibration gain | `Intelligence P5` | opt-in remote rerank + regression suite | Linux gains extra retrieval sharpness without changing shared semantic contracts |

## Implementation Order

1. Finish `Intelligence P0` and freeze metric definitions.
2. Land `Intelligence P1` first so all later stages can reference `SubjectStateDigest`.
3. Land `Intelligence P2` before any async replay work; replay must first be coherent for synchronous turn data.
4. Land `Intelligence P3` before `Intelligence P4`; execution-class policy should read a better working set than today’s minimal summary.
5. Land `Intelligence P4` before `Intelligence P5`; replay append and eval are more useful once execution class and recovery-aware behavior exist.
6. Land `Intelligence P6` last and keep it optional, Linux-only, and non-authoritative.

## Intelligence P0: Baseline And Contract Freeze

**Why now:** The current `dev-docs/intelligence-uplift-plan.md` has the right direction but is still partly strategic prose. Before changing behavior, freeze the measurable contract and the exact definition of "better".

**Files:**

- Modify: `dev-docs/intelligence-uplift-plan.md`
- Modify: `src/memory/recall_inspection.rs`
- Modify: `src/agent/loop.rs`
- Create: `src/cli/intelligence_eval.rs`
- Modify: `src/cli/mod.rs`

**Deliverables:**

- A local definition of the intelligence workstream stages and their relationship to Beetle OS `P7-P8`.
- Baseline metrics:
  - `first_useful_answer_rate`
  - `mean_react_rounds`
  - `tool_after_success_generic_answer_rate`
  - `blocker_explanation_quality`
  - `continuity_correctness`
  - `recovery_sensitive_correctness`
- A Linux CLI path that can replay or inspect baseline cases against current logic.

**Implementation Steps:**

- [ ] Add a short "implementation workstream" note to `dev-docs/intelligence-uplift-plan.md` clarifying that the document is strategic and that this plan file is the executable stage plan.
- [ ] Add metric structs and markdown/json render helpers in `src/memory/recall_inspection.rs` for the six baseline metrics.
- [ ] Add a Linux-only intelligence-eval command in `src/cli/intelligence_eval.rs` that reads replay/inspection inputs and prints baseline metrics.
- [ ] Register the command in `src/cli/mod.rs`.
- [ ] Add a small agent-loop hook in `src/agent/loop.rs` to expose any raw values already needed by the CLI metrics.

**Verification:**

- Run: `cargo test recall_inspection -- --nocapture`
- Run: `cargo test agent::loop -- --nocapture`
- Run: `cargo test cli -- --nocapture`
- Run: `cargo check`

**Exit Criteria:**

- The team can run one command and get a reproducible baseline for intelligence-related regressions before any behavior change lands.

## Intelligence P1: Subject State Compiler

**Why now:** This is the smallest deterministic algorithmic core that sharpens first response quality without adding a second authority plane.

**Files:**

- Create: `src/agent/subject_state.rs`
- Modify: `src/agent/mod.rs`
- Modify: `src/agent/loop/worker_context.rs`
- Modify: `src/agent/context.rs`
- Modify: `src/memory/turn_ledger.rs`
- Test: `src/agent/context.rs`
- Test: `src/agent/loop.rs`

**Deliverables:**

- `SubjectState`
- `SubjectStateDigest`
- `compile_subject_state(...)`
- Prompt-path consumption in `prepare_worker_conversation -> build_context`
- Replay digest persistence in turn ledger

**Implementation Steps:**

- [ ] Define `SubjectState` and `SubjectStateDigest` in `src/agent/subject_state.rs` with only the first-stage fields from the strategic doc: `identity_stance`, `relationship_posture`, `boundary_posture`, `task_posture`, `resource_posture`, `continuity_risk`, `active_non_negotiables`.
- [ ] Keep the compiler deterministic and feed it only from current prompt-memory and runtime inputs already available inside `prepare_worker_conversation`.
- [ ] Wire `SubjectState` into `src/agent/loop/worker_context.rs` so it is compiled before `build_context(...)` is called.
- [ ] Teach `src/agent/context.rs` to render a compact subject-state block ahead of the current wider constitutional stack.
- [ ] Extend `src/memory/turn_ledger.rs` with a compact digest field so every terminal turn can record which subject-state posture was active.

**Verification:**

- Run: `cargo test subject_state -- --nocapture`
- Run: `cargo test agent::context -- --nocapture`
- Run: `cargo test agent::loop -- --nocapture`
- Run: `cargo check`

**Exit Criteria:**

- The main prompt path consumes a compact deterministic subject state.
- Turn history can explain which subject-state digest was active for each terminal turn.

## Intelligence P2: Synchronous Replay And Tool Observations

**Why now:** Replay must first cover the data that already exists inside one turn before the codebase can safely append asynchronous intelligence signals later.

**Files:**

- Modify: `src/memory/turn_ledger.rs`
- Modify: `src/agent/tool_guidance.rs`
- Modify: `src/agent/loop/tool_round.rs`
- Modify: `src/agent/loop/turn_finalize.rs`
- Test: `src/memory/turn_ledger.rs`
- Test: `src/agent/strategy.rs`
- Test: `src/agent/loop.rs`

**Deliverables:**

- Synchronous replay fields:
  - `execution_class`
  - `subject_state_digest`
  - `tool_path`
  - `blockers`
  - `final_outcome`
  - `mode_snapshot`
  - `pressure_snapshot`
- Typed synchronous tool observations extracted from successful tool results

**Implementation Steps:**

- [ ] Extend `TurnLedger` with replay-oriented fields that are definitely available by `turn_finalize`.
- [ ] Keep history compatibility: defaults must preserve loading of existing stored ledgers.
- [ ] Refactor `src/agent/tool_guidance.rs` so current ad-hoc observations become replay-friendly typed summaries instead of only prompt hints.
- [ ] Wire those summaries out of `src/agent/loop/tool_round.rs`.
- [ ] Persist only synchronous observations in `src/agent/loop/turn_finalize.rs`; do not mix in maintenance or self-runtime data yet.

**Verification:**

- Run: `cargo test turn_ledger -- --nocapture`
- Run: `cargo test agent::loop -- --nocapture`
- Run: `cargo test agent::strategy -- --nocapture`
- Run: `cargo check`

**Exit Criteria:**

- One finished turn can answer, from replay alone: what the turn tried to do, which tools it used, what blockers it hit, what outcome it produced, and under what pressure/mode.

## Intelligence P3: Minimal Working-Set Upgrade

**Why now:** Better execution-class policy and better post-tool convergence both need a tighter active-task summary than the current `goal/progress/blocker/next_action/last_output`.

**Files:**

- Modify: `src/memory/execution_state.rs`
- Modify: `src/memory/prompt_context.rs`
- Modify: `src/memory/maintenance.rs`
- Modify: `src/agent/loop/tool_round.rs`
- Modify: `src/agent/loop/turn_finalize.rs`
- Test: `src/memory/execution_state.rs`
- Test: `src/memory/prompt_context.rs`
- Test: `src/memory/maintenance.rs`

**Deliverables:**

- Minimal working-set fields added to `ExecutionState` without turning it into a second large store.
- Active-task-context rendering that prioritizes the new fields when they exist.

**Implementation Steps:**

- [ ] Add only the minimal extra fields that improve next-turn reasoning directly:
  - `primary_goal`
  - `done_criteria`
  - `confirmed_facts`
  - `open_questions`
  - `active_constraints`
  - `current_blockers`
  - `latest_observations`
  - `next_best_actions`
  - `confidence_posture`
- [ ] Preserve existing compact semantics: cap counts and rendered lengths aggressively.
- [ ] Keep the current refresh contract explicit; if `ExecutionState` still refreshes only for user/non-cron/normal-pressure turns, document and test that behavior rather than hiding it.
- [ ] Update `prompt_context` rendering so the working-set block is concise and decision-oriented.
- [ ] Teach maintenance/finalize paths to refresh only the minimal working-set summary, not a full observation archive.

**Verification:**

- Run: `cargo test execution_state -- --nocapture`
- Run: `cargo test prompt_context -- --nocapture`
- Run: `cargo test maintenance -- --nocapture`
- Run: `cargo check`

**Exit Criteria:**

- `ExecutionState` becomes a meaningfully better active-task summary while remaining compact and bounded.

## Intelligence P4: Hard-Turn Gate And Recovery-Aware Policy

**Why now:** Once subject state, replay, and working set are in place, the runtime can start allocating more reasoning budget only to hard turns and only within the current single-agent-plane contract.

**Files:**

- Modify: `src/agent/loop.rs`
- Modify: `src/agent/loop/worker_context.rs`
- Modify: `src/agent/request_plan.rs`
- Modify: `src/llm/compat.rs`
- Modify: `src/llm/mod.rs`
- Modify: `src/agent/tool_guidance.rs`
- Test: `src/agent/request_plan.rs`
- Test: `src/llm/fallback.rs`
- Test: `src/agent/loop.rs`

**Deliverables:**

- `TurnExecutionClass` with:
  - `FastInteractive`
  - `Standard`
  - `HardReasoning`
  - `LongHorizonAction`
- Deterministic first-stage classification
- Recovery-aware inputs incorporated into policy decisions

**Implementation Steps:**

- [ ] Define `TurnExecutionClass` in the agent layer where `run_worker_path` can own it.
- [ ] Classify hard turns deterministically from already-available evidence: repeated stalls, explicit complex-user asks, weak recall convergence, or strong blocker persistence.
- [ ] First wire execution class into `build_context`, reply budget, replay fields, and LLM dispatch choice.
- [ ] Do **not** yet assume it can influence `AgentRequestPlan::build(...)`; only move that control point once the earlier stages are stable.
- [ ] Feed recovery-aware inputs from current runtime mode, orchestrator pressure, and continuity-sensitive state into policy summaries and convergence guidance.

**Verification:**

- Run: `cargo test request_plan -- --nocapture`
- Run: `cargo test llm::fallback -- --nocapture`
- Run: `cargo test agent::loop -- --nocapture`
- Run: `cargo check`

**Exit Criteria:**

- Hard turns can be detected and handled differently without adding a second loop, a second agent, or a second personality kernel.

## Intelligence P5: Async Replay Append And Continuous Eval

**Why now:** Maintenance, self-runtime, and other delayed jobs should stop being invisible intelligence inputs; they must become replayable without lying about turn-finalize timing.

**Files:**

- Modify: `src/agent/loop/background_jobs.rs`
- Modify: `src/agent/loop.rs`
- Modify: `src/agent/loop/turn_finalize.rs`
- Modify: `src/memory/maintenance.rs`
- Modify: `src/memory/self_runtime.rs`
- Modify: `src/memory/turn_ledger.rs`
- Modify: `src/memory/recall_inspection.rs`
- Modify: `src/cli/intelligence_eval.rs`
- Test: `src/agent/loop.rs`
- Test: `src/memory/maintenance.rs`
- Test: `src/memory/self_runtime.rs`

**Deliverables:**

- An append/update path for replay entries after `turn_finalize`
- Req-id or equivalent turn-level linkage for maintenance/self-runtime observations
- Continuous evaluation that can consume both synchronous and asynchronous replay signals

**Implementation Steps:**

- [ ] Add explicit replay-link identifiers to post-reply maintenance payloads and any self-runtime job payloads that currently lack them.
- [ ] Add append/update APIs or semantics in `TurnLedgerStore` and its implementations without breaking current history behavior.
- [ ] Make maintenance emit replay append records for working-set refresh, continuity-sensitive outcomes, and factual-governance conclusions.
- [ ] Make self-runtime emit replay append records for governance actions that materially changed the turn’s longer-term continuity posture.
- [ ] Extend the CLI eval tool so it can read the enriched replay stream rather than only terminal-turn synchronous fields.

**Verification:**

- Run: `cargo test agent::loop -- --nocapture`
- Run: `cargo test maintenance -- --nocapture`
- Run: `cargo test self_runtime -- --nocapture`
- Run: `cargo check`

**Exit Criteria:**

- Replay is honest about timing: synchronous fields are written at finalize, asynchronous fields are appended later, and evaluation can consume both.

## Intelligence P6: Optional Linux-Only Rerank And Calibration

**Why now:** Retrieval/rerank gain is only worth adding after the core algorithmic contract exists; otherwise it just hides missing structure behind a stronger search stack.

**Files:**

- Create: `src/memory/remote_rerank.rs`
- Modify: `src/memory/mod.rs`
- Modify: `src/memory/recall_rerank.rs`
- Modify: `src/memory/archive_search.rs`
- Modify: `src/memory/continuity_capsule.rs`
- Modify: `src/config.rs`
- Modify: `src/cli/intelligence_eval.rs`
- Test: `src/memory/recall_rerank.rs`
- Test: `src/memory/archive_search.rs`
- Test: `src/cli/intelligence_eval.rs`

**Deliverables:**

- Optional, default-off, Linux-only remote rerank path
- Eval comparison mode for local-vs-reranked retrieval
- No change to authoritative memory semantics

**Implementation Steps:**

- [ ] Introduce a dedicated `remote_rerank` module instead of burying network calls inside existing local rerank code.
- [ ] Gate the feature in config and keep it disabled by default.
- [ ] Apply rerank only to shortlist reordering for archive and continuity candidates; never let it become a second memory authority.
- [ ] Extend the eval CLI to compare baseline/local-rerank/remote-rerank outcomes on the same cases.

**Verification:**

- Run: `cargo test recall_rerank -- --nocapture`
- Run: `cargo test archive_search -- --nocapture`
- Run: `cargo test cli -- --nocapture`
- Run: `cargo check`

**Exit Criteria:**

- Linux can optionally gain retrieval sharpness, but the shared Linux/ESP semantic contract remains identical.

## Global Constraints

- No second always-on agent loop.
- No new personality authority parallel to `self_authored_core -> relationship_constitution -> persona_priority`.
- No new memory authority parallel to `shared factual / archive evidence / continuity / private`.
- No ESP-only semantic branch; only implementation-thickness differences are allowed.
- No replay field may pretend to be synchronous if it is only known after background jobs run.
- No stage is complete without tests, replay evidence, and a clear exit gate.

## Verification Matrix

- Core compile/test:
  - `cargo check`
  - `cargo test`
- Linux-only replay/eval smoke:
  - `cargo run --features cli -- intelligence-eval --help`
- Optional isolation gate before merge:
  - `./scripts/check_platform_isolation.sh`

## Risks And Countermeasures

- **Risk:** `ExecutionState` balloons into an unbounded shadow store.
  Countermeasure: cap list lengths, cap render sizes, and keep P3 limited to minimal working-set summary fields.
- **Risk:** replay append path creates contradictory sync/async state.
  Countermeasure: keep explicit sync-vs-async field ownership and test append ordering.
- **Risk:** execution class silently forks Linux/ESP semantics.
  Countermeasure: execution class changes budget and routing only; semantic behavior and contracts remain shared.
- **Risk:** stronger reasoning lane hides missing deterministic structure.
  Countermeasure: land deterministic `SubjectState`, replay, and working set before any optional stronger lane.

## Self-Review

- Spec coverage: the plan covers `Subject State`, `Replay`, `Observation`, `Working Set`, `Hard Turn Gate`, `Recovery-aware policy`, and optional rerank/eval.
- Placeholder scan: no `TODO` / `TBD` placeholders remain.
- Type consistency: the same names are used throughout: `SubjectState`, `SubjectStateDigest`, `TurnExecutionClass`, `ExecutionState`, `TurnLedger`.

## Execution Handoff

Plan complete and saved to `docs/superpowers/plans/2026-04-07-intelligence-uplift-implementation.md`. Two execution options:

**1. Subagent-Driven (recommended)** - I dispatch a fresh subagent per task, review between tasks, fast iteration

**2. Inline Execution** - Execute tasks in this session using executing-plans, batch execution with checkpoints

Which approach?
