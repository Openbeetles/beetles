# Self-Learning Closure P4-C2 C4 Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Close the remaining self-learning loop by recording runtime-skill reuse outcomes, feeding validated procedures back into recall ranking, and exposing a unified learning summary in `/api/memory/status` and recall inspection.

**Architecture:** Reuse the existing `task_learning -> runtime_skill -> recall report -> operator surface` chain. Carry prompt-time selected runtime skill / task-learning ids through the worker telemetry into post-reply maintenance, write outcome-backed evidence onto `RuntimeSkillRecord`, then let recall scoring and operator surfaces consume those persisted fields instead of introducing a second learning plane.

**Tech Stack:** Rust, serde, existing `SkillStorage`, `TaskLearningStore`, prompt recall router/rerank, `/api/memory/status` handler, cargo test/check.

---

### Task 1: Runtime Skill Outcome Ledger

**Files:**
- Modify: `src/skills/runtime.rs`
- Modify: `src/skills/mod.rs`
- Modify: `src/memory/maintenance.rs`
- Modify: `src/agent/loop.rs`
- Modify: `src/agent/loop/background_jobs.rs`
- Modify: `src/agent/loop/turn_finalize.rs`
- Modify: `src/memory/prompt_context.rs`
- Test: `src/skills/runtime.rs`
- Test: `src/memory/maintenance.rs`

- [ ] **Step 1: Write the failing tests**

Add tests that prove the missing behavior:

```rust
#[test]
fn record_runtime_skill_outcome_marks_validated_and_revision_pending_states() {
    let storage = StubSkillStorage::default();
    upsert_runtime_skill(
        &storage,
        &RuntimeSkillWrite {
            name: String::new(),
            topic: "release_patch_flow".to_string(),
            title: "Release patch flow".to_string(),
            summary: "Apply the release patch safely.".to_string(),
            content: "1. inspect diff\n2. patch\n3. verify".to_string(),
            citations: Vec::new(),
            source_chat_id: Some("chat-1".to_string()),
            observed_at: 100,
        },
    )
    .unwrap();

    record_runtime_skill_outcomes(
        &storage,
        &[String::from("runtime_skill__release_patch_flow")],
        RuntimeSkillReuseOutcome::Succeeded,
        200,
        "final_answer",
    )
    .unwrap();
    record_runtime_skill_outcomes(
        &storage,
        &[String::from("runtime_skill__release_patch_flow")],
        RuntimeSkillReuseOutcome::Mismatch,
        260,
        "final_recovery",
    )
    .unwrap();

    let record = parse_runtime_skill_record(
        "runtime_skill__release_patch_flow",
        &get_skill_content(&storage, "runtime_skill__release_patch_flow").unwrap(),
    )
    .unwrap();
    assert_eq!(record.validated_success_count, 1);
    assert_eq!(record.mismatch_count, 1);
    assert!(record.revision_pending);
    assert_eq!(record.last_outcome_at, Some(260));
}
```

```rust
#[test]
fn post_reply_maintenance_records_runtime_skill_success_and_mismatch() {
    // Build a stub context with one promoted skill, then run post-reply maintenance twice:
    // once with RuntimeSkillReuseOutcome::Succeeded and once with ::Mismatch.
    // Expect the persisted skill record to reflect both counters.
}
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test --lib record_runtime_skill_outcome_marks_validated_and_revision_pending_states post_reply_maintenance_records_runtime_skill_success_and_mismatch`

Expected: FAIL with missing runtime skill outcome fields/functions or missing maintenance plumbing.

- [ ] **Step 3: Write minimal implementation**

Implement:

```rust
pub enum RuntimeSkillReuseOutcome {
    Neutral,
    Succeeded,
    Mismatch,
}

pub fn record_runtime_skill_outcomes(
    storage: &dyn SkillStorage,
    skill_names: &[String],
    outcome: RuntimeSkillReuseOutcome,
    now_secs: u64,
    outcome_note: &str,
) -> crate::error::Result<usize> { /* update persisted RuntimeSkillRecord fields */ }
```

Add persisted runtime-skill fields:

```rust
pub struct RuntimeSkillRecord {
    // existing fields...
    pub validated_success_count: u32,
    pub mismatch_count: u32,
    pub revision_count: u32,
    pub revision_pending: bool,
    pub last_outcome_at: Option<u64>,
    pub last_outcome_note: String,
}
```

Carry selected prompt recall ids into post-reply maintenance:

```rust
pub struct PromptRuntimeCarry {
    pub summary_text: Option<String>,
    pub long_term_memory_text: Option<String>,
    pub recent_messages: Vec<SessionMessage>,
    pub prompt_recall_intent: PromptRecallIntent,
    pub runtime_skill_selected_ids: Vec<String>,
    pub task_recall_selected_ids: Vec<String>,
}
```

```rust
struct PostReplyMaintenanceJobPayload {
    // existing fields...
    prompt_recall_intent: crate::memory::PromptRecallIntent,
    runtime_skill_selected_ids: Vec<String>,
    task_learning_selected_ids: Vec<String>,
    reuse_outcome: crate::memory::RuntimeSkillReuseOutcome,
    reuse_outcome_note: String,
}
```

- [ ] **Step 4: Run tests to verify they pass**

Run:
- `cargo test --lib record_runtime_skill_outcome_marks_validated_and_revision_pending_states`
- `cargo test --lib post_reply_maintenance_records_runtime_skill_success_and_mismatch`

Expected: PASS

- [ ] **Step 5: Commit**

```bash
git add src/skills/runtime.rs src/skills/mod.rs src/memory/maintenance.rs src/agent/loop.rs src/agent/loop/background_jobs.rs src/agent/loop/turn_finalize.rs src/memory/prompt_context.rs
git commit -m "Add runtime skill outcome tracking"
```

### Task 2: Query-Aware Learning Reuse

**Files:**
- Modify: `src/skills/runtime.rs`
- Modify: `src/memory/recall_contract.rs`
- Modify: `src/memory/recall_rerank.rs`
- Modify: `src/memory/recall_inspection.rs`
- Modify: `src/task_execution/learning.rs`
- Test: `src/skills/runtime.rs`
- Test: `src/memory/recall_inspection.rs`
- Test: `src/memory/recall_router.rs`

- [ ] **Step 1: Write the failing tests**

Add tests that prove validated procedures get a real, explainable preference:

```rust
#[test]
fn runtime_skill_recall_prefers_validated_skill_over_fresh_promoted_skill() {
    // Two similar runtime skills, one validated_success_count > 0 and one untouched.
    // Expect validated skill to rank first.
}
```

```rust
#[test]
fn inspect_runtime_skill_recall_surfaces_stable_and_fallback_selection_notes() {
    // Validated top hit => selection_note == stable_validated_runtime_skill
    // Unvalidated promoted hit => selection_note == fallback_promoted_runtime_skill
}
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test --lib runtime_skill_recall_prefers_validated_skill_over_fresh_promoted_skill inspect_runtime_skill_recall_surfaces_stable_and_fallback_selection_notes`

Expected: FAIL because validated outcome evidence is not used in recall scoring or inspection notes.

- [ ] **Step 3: Write minimal implementation**

Adjust runtime skill scoring and inspection:

```rust
fn runtime_skill_learning_bonus(record: &RuntimeSkillRecord) -> (u32, Vec<String>) {
    // validated_success_count => small positive boost
    // revision_pending / mismatch_count => small negative or governance drag
}
```

```rust
selection_note: Some(
    if top_record.revision_pending {
        "fallback_revision_pending_runtime_skill".to_string()
    } else if top_record.validated_success_count > 0 {
        "stable_validated_runtime_skill".to_string()
    } else {
        "fallback_promoted_runtime_skill".to_string()
    }
)
```

Also let task-learning inspection / operator text expose whether a promoted procedure is backed by a validated runtime skill when that can be resolved from the deterministic skill name for the topic.

- [ ] **Step 4: Run tests to verify they pass**

Run:
- `cargo test --lib runtime_skill_recall_prefers_validated_skill_over_fresh_promoted_skill`
- `cargo test --lib inspect_runtime_skill_recall_surfaces_stable_and_fallback_selection_notes`

Expected: PASS

- [ ] **Step 5: Commit**

```bash
git add src/skills/runtime.rs src/memory/recall_contract.rs src/memory/recall_rerank.rs src/memory/recall_inspection.rs src/task_execution/learning.rs
git commit -m "Bias recall toward validated learning reuse"
```

### Task 3: Learning Operator Summary and Minimal Metrics

**Files:**
- Modify: `src/skills/runtime.rs`
- Modify: `src/skills/mod.rs`
- Modify: `src/platform/http_server/handlers/memory.rs`
- Modify: `src/memory/recall_benchmark.rs`
- Test: `src/platform/http_server/handlers/memory.rs`
- Test: `src/memory/recall_benchmark.rs`

- [ ] **Step 1: Write the failing tests**

Add tests that prove `/api/memory/status` exposes a unified learning summary and metrics:

```rust
#[test]
fn memory_status_api_includes_learning_summary_and_metrics() {
    let ctx = build_test_context();
    let payload = body(&ctx, "/api/memory/status").unwrap();
    let parsed: serde_json::Value = serde_json::from_str(&payload).unwrap();
    assert!(parsed["learning"]["runtime_skills"].get("validated").is_some());
    assert!(parsed["learning"]["runtime_skills"].get("revision_pending").is_some());
    assert!(parsed["learning"]["metrics"].get("runtime_skill_success_events").is_some());
    assert!(parsed["learning"]["metrics"].get("runtime_skill_mismatch_events").is_some());
}
```

```rust
#[test]
fn recall_benchmark_reports_learning_aware_runtime_skill_reasoning() {
    // Expect runtime recall benchmark report to include the new stable/fallback selection note.
}
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test --lib memory_status_api_includes_learning_summary_and_metrics recall_benchmark_reports_learning_aware_runtime_skill_reasoning`

Expected: FAIL with missing `learning` section / metrics / selection note assertions.

- [ ] **Step 3: Write minimal implementation**

Add a runtime-skill operator summary and combine it with task candidate data:

```rust
pub struct RuntimeSkillOperatorSnapshot {
    pub total: usize,
    pub validated: usize,
    pub revision_pending: usize,
    pub stale_or_low_value: usize,
    pub success_events: u32,
    pub mismatch_events: u32,
    pub recent_records: Vec<RuntimeSkillOperatorRecord>,
}
```

Expose it in `/api/memory/status`:

```rust
struct MemoryLearningStatus {
    task_candidates: crate::task_execution::TaskLearningOperatorSnapshot,
    runtime_skills: crate::skills::RuntimeSkillOperatorSnapshot,
    metrics: MemoryLearningMetrics,
}
```

- [ ] **Step 4: Run tests to verify they pass**

Run:
- `cargo test --lib memory_status_api_includes_learning_summary_and_metrics`
- `cargo test --lib recall_benchmark_reports_learning_aware_runtime_skill_reasoning`

Expected: PASS

- [ ] **Step 5: Commit**

```bash
git add src/skills/runtime.rs src/skills/mod.rs src/platform/http_server/handlers/memory.rs src/memory/recall_benchmark.rs
git commit -m "Expose learning closure operator summary"
```

### Task 4: Final Verification

**Files:**
- Modify: `docs/superpowers/plans/2026-04-09-self-learning-closure-p4c2-c4.md`

- [ ] **Step 1: Run focused regression suite**

Run: `cargo test --lib runtime_skill_`

Expected: PASS with the new runtime skill outcome and recall tests included.

- [ ] **Step 2: Run memory/operator regression suite**

Run: `cargo test --lib memory_status_api_ recall_`

Expected: PASS with `/api/memory/status` and recall inspection/benchmark coverage green.

- [ ] **Step 3: Run compile verification**

Run: `cargo check`

Expected: PASS

- [ ] **Step 4: Commit plan status update**

```bash
git add docs/superpowers/plans/2026-04-09-self-learning-closure-p4c2-c4.md
git commit -m "Record self-learning closure execution plan"
```
