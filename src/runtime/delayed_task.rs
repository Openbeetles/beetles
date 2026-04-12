//! Lightweight delayed task queue polled by existing runtime threads.
//! 轻量延迟任务队列；由已有运行线程轮询执行，不额外创建后台线程。

use std::sync::{Mutex, OnceLock};
use std::time::{Duration, Instant};

type DelayedTask = Box<dyn FnOnce() + Send + 'static>;

#[cfg(any(target_arch = "xtensa", target_arch = "riscv32"))]
const DELAYED_TASK_BEST_EFFORT_MAX: usize = 16;
#[cfg(not(any(target_arch = "xtensa", target_arch = "riscv32")))]
const DELAYED_TASK_BEST_EFFORT_MAX: usize = 64;
#[cfg(any(target_arch = "xtensa", target_arch = "riscv32"))]
const DELAYED_TASK_CRITICAL_RESERVED: usize = 4;
#[cfg(not(any(target_arch = "xtensa", target_arch = "riscv32")))]
const DELAYED_TASK_CRITICAL_RESERVED: usize = 8;
const DELAYED_TASK_TOTAL_MAX: usize = DELAYED_TASK_BEST_EFFORT_MAX + DELAYED_TASK_CRITICAL_RESERVED;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum DelayedTaskPriority {
    BestEffort,
    Critical,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum DelayedTaskServiceScope {
    AllEligible,
    CriticalOnly,
}

struct DelayedTaskJob {
    due_at: Instant,
    priority: DelayedTaskPriority,
    task: Option<DelayedTask>,
}

#[derive(Default)]
struct DelayedTaskState {
    pending: Mutex<Vec<DelayedTaskJob>>,
}

fn state() -> &'static DelayedTaskState {
    static STATE: OnceLock<DelayedTaskState> = OnceLock::new();
    STATE.get_or_init(DelayedTaskState::default)
}

fn execute_jobs(mut due: Vec<DelayedTaskJob>) {
    due.sort_by_key(|job| job.due_at);
    for mut job in due {
        if let Some(task) = job.task.take() {
            task();
        }
    }
}

fn take_due_jobs_locked(pending: &mut Vec<DelayedTaskJob>, now: Instant) -> Vec<DelayedTaskJob> {
    let mut due = Vec::new();
    let mut index = 0usize;
    while index < pending.len() {
        if pending[index].due_at <= now {
            due.push(pending.swap_remove(index));
        } else {
            index += 1;
        }
    }
    due
}

fn take_due_jobs_locked_with_policy(
    pending: &mut Vec<DelayedTaskJob>,
    now: Instant,
    scope: DelayedTaskServiceScope,
    allow_best_effort: bool,
) -> Vec<DelayedTaskJob> {
    if scope == DelayedTaskServiceScope::AllEligible && allow_best_effort {
        return take_due_jobs_locked(pending, now);
    }
    let mut due = Vec::new();
    let mut index = 0usize;
    while index < pending.len() {
        let job = &pending[index];
        if job.due_at <= now && job.priority == DelayedTaskPriority::Critical {
            due.push(pending.swap_remove(index));
        } else {
            index += 1;
        }
    }
    due
}

fn oldest_best_effort_index(pending: &[DelayedTaskJob]) -> Option<usize> {
    pending
        .iter()
        .enumerate()
        .filter(|(_, job)| job.priority == DelayedTaskPriority::BestEffort)
        .min_by_key(|(_, job)| job.due_at)
        .map(|(idx, _)| idx)
}

fn schedule_delayed_task_with_priority(
    due_at: Instant,
    priority: DelayedTaskPriority,
    task: DelayedTask,
) -> std::result::Result<(), DelayedTask> {
    let mut dropped_best_effort = false;
    let mut notify_deadline_changed = false;
    let mut task = Some(task);
    {
        let mut pending = state().pending.lock().unwrap_or_else(|e| e.into_inner());
        match priority {
            DelayedTaskPriority::BestEffort => {
                let best_effort_count = pending
                    .iter()
                    .filter(|job| job.priority == DelayedTaskPriority::BestEffort)
                    .count();
                if best_effort_count < DELAYED_TASK_BEST_EFFORT_MAX
                    && pending.len() < DELAYED_TASK_TOTAL_MAX
                {
                    pending.push(DelayedTaskJob {
                        due_at,
                        priority,
                        task: task.take(),
                    });
                    notify_deadline_changed = true;
                }
            }
            DelayedTaskPriority::Critical => {
                if pending.len() >= DELAYED_TASK_TOTAL_MAX {
                    if let Some(idx) = oldest_best_effort_index(&pending) {
                        pending.swap_remove(idx);
                        dropped_best_effort = true;
                    }
                }
                if pending.len() < DELAYED_TASK_TOTAL_MAX {
                    pending.push(DelayedTaskJob {
                        due_at,
                        priority,
                        task: task.take(),
                    });
                    notify_deadline_changed = true;
                }
            }
        }
    }
    if notify_deadline_changed {
        crate::bg_timer::notify_deadline_changed();
    }
    if dropped_best_effort {
        log::warn!("[delayed_task] dropped oldest best-effort job to reserve critical capacity");
    }
    match task {
        Some(task) => Err(task),
        None => Ok(()),
    }
}

pub fn schedule_delayed_task(due_at: Instant, task: DelayedTask) -> bool {
    schedule_delayed_task_with_priority(due_at, DelayedTaskPriority::BestEffort, task).is_ok()
}

pub fn schedule_critical_delayed_task(
    due_at: Instant,
    task: DelayedTask,
) -> std::result::Result<(), DelayedTask> {
    schedule_delayed_task_with_priority(due_at, DelayedTaskPriority::Critical, task)
}

pub fn service_delayed_tasks() {
    service_delayed_tasks_with_scope(DelayedTaskServiceScope::AllEligible);
}

pub fn service_critical_delayed_tasks() {
    service_delayed_tasks_with_scope(DelayedTaskServiceScope::CriticalOnly);
}

fn service_delayed_tasks_with_scope(scope: DelayedTaskServiceScope) {
    let allow_best_effort = crate::runtime::thread_registry::runtime_mode_snapshot()
        .action_budget
        .allow_best_effort_delayed_tasks;
    let due = {
        let mut pending = state().pending.lock().unwrap_or_else(|e| e.into_inner());
        take_due_jobs_locked_with_policy(&mut pending, Instant::now(), scope, allow_best_effort)
    };
    execute_jobs(due);
}

pub fn next_delayed_task_wait(max_wait: Duration) -> Duration {
    let pending = state().pending.lock().unwrap_or_else(|e| e.into_inner());
    let now = Instant::now();
    let allow_best_effort = crate::runtime::thread_registry::runtime_mode_snapshot()
        .action_budget
        .allow_best_effort_delayed_tasks;
    pending
        .iter()
        .filter(|job| allow_best_effort || job.priority == DelayedTaskPriority::Critical)
        .map(|job| job.due_at.saturating_duration_since(now))
        .min()
        .map(|wait| wait.min(max_wait))
        .unwrap_or(max_wait)
}

#[cfg(test)]
pub fn reset_delayed_tasks_for_tests() {
    crate::state::set_voice_exclusive_active(false);
    crate::state::set_background_maintenance_active(false);
    crate::state::set_config_plane_active(false);
    crate::state::set_boot_phase_active(false);
    crate::state::set_pairing_state_known(false);
    crate::state::set_pairing_required(false);
    crate::state::set_recovery_safe_mode_active(false);
    state()
        .pending
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .clear();
}

#[cfg(test)]
pub fn delayed_task_test_guard() -> std::sync::MutexGuard<'static, ()> {
    static LOCK: OnceLock<Mutex<()>> = OnceLock::new();
    LOCK.get_or_init(|| Mutex::new(()))
        .lock()
        .unwrap_or_else(|e| e.into_inner())
}

#[cfg(test)]
fn pending_counts_for_tests() -> (usize, usize) {
    let pending = state().pending.lock().unwrap_or_else(|e| e.into_inner());
    let best_effort = pending
        .iter()
        .filter(|job| job.priority == DelayedTaskPriority::BestEffort)
        .count();
    let critical = pending
        .iter()
        .filter(|job| job.priority == DelayedTaskPriority::Critical)
        .count();
    (best_effort, critical)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::{Arc, Mutex};

    #[test]
    fn service_delayed_tasks_runs_due_jobs_in_due_order() {
        let _guard = delayed_task_test_guard();
        reset_delayed_tasks_for_tests();
        let executed = Arc::new(Mutex::new(Vec::new()));
        let now = Instant::now();

        for (idx, offset_ms) in [(1, 20_u64), (2, 5_u64), (3, 10_u64)] {
            let executed = Arc::clone(&executed);
            schedule_delayed_task(
                now + Duration::from_millis(offset_ms),
                Box::new(move || {
                    executed.lock().unwrap_or_else(|e| e.into_inner()).push(idx);
                }),
            );
        }

        std::thread::sleep(Duration::from_millis(30));
        service_delayed_tasks();

        let got = executed.lock().unwrap_or_else(|e| e.into_inner()).clone();
        assert_eq!(got, vec![2, 3, 1]);
    }

    #[test]
    fn next_delayed_task_wait_caps_to_soonest_due_job() {
        let _guard = delayed_task_test_guard();
        reset_delayed_tasks_for_tests();
        let now = Instant::now();
        schedule_delayed_task(now + Duration::from_millis(15), Box::new(|| {}));
        schedule_delayed_task(now + Duration::from_millis(40), Box::new(|| {}));

        let wait = next_delayed_task_wait(Duration::from_millis(100));
        assert!(wait <= Duration::from_millis(20));
    }

    #[test]
    fn best_effort_queue_has_hard_cap() {
        let _guard = delayed_task_test_guard();
        reset_delayed_tasks_for_tests();
        let due_at = Instant::now() + Duration::from_secs(60);
        for _ in 0..DELAYED_TASK_BEST_EFFORT_MAX {
            assert!(schedule_delayed_task(due_at, Box::new(|| {})));
        }
        assert!(!schedule_delayed_task(due_at, Box::new(|| {})));
        assert_eq!(
            pending_counts_for_tests(),
            (DELAYED_TASK_BEST_EFFORT_MAX, 0)
        );
    }

    #[test]
    fn critical_job_can_evict_oldest_best_effort_when_total_is_full() {
        let _guard = delayed_task_test_guard();
        reset_delayed_tasks_for_tests();
        let due_at = Instant::now() + Duration::from_secs(60);
        for _ in 0..DELAYED_TASK_BEST_EFFORT_MAX {
            assert!(schedule_delayed_task(due_at, Box::new(|| {})));
        }
        for _ in 0..DELAYED_TASK_CRITICAL_RESERVED {
            assert!(schedule_critical_delayed_task(due_at, Box::new(|| {})).is_ok());
        }

        assert!(schedule_critical_delayed_task(due_at, Box::new(|| {})).is_ok());
        assert_eq!(
            pending_counts_for_tests(),
            (
                DELAYED_TASK_BEST_EFFORT_MAX.saturating_sub(1),
                DELAYED_TASK_CRITICAL_RESERVED + 1
            )
        );
    }

    #[test]
    fn scheduling_new_job_does_not_run_overdue_jobs_inline() {
        let _guard = delayed_task_test_guard();
        reset_delayed_tasks_for_tests();
        let executed = Arc::new(Mutex::new(Vec::new()));
        let now = Instant::now();

        {
            let executed = Arc::clone(&executed);
            assert!(schedule_delayed_task(
                now,
                Box::new(move || {
                    executed.lock().unwrap_or_else(|e| e.into_inner()).push(1);
                }),
            ));
        }
        {
            let executed = Arc::clone(&executed);
            assert!(schedule_delayed_task(
                now + Duration::from_secs(1),
                Box::new(move || {
                    executed.lock().unwrap_or_else(|e| e.into_inner()).push(2);
                }),
            ));
        }

        assert!(executed
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .is_empty());

        service_delayed_tasks();
        let got = executed.lock().unwrap_or_else(|e| e.into_inner()).clone();
        assert_eq!(got, vec![1]);
    }

    #[test]
    fn critical_only_service_skips_best_effort_jobs() {
        let _guard = delayed_task_test_guard();
        reset_delayed_tasks_for_tests();
        let executed = Arc::new(Mutex::new(Vec::new()));
        let now = Instant::now();

        {
            let executed = Arc::clone(&executed);
            assert!(schedule_delayed_task(
                now,
                Box::new(move || {
                    executed.lock().unwrap_or_else(|e| e.into_inner()).push(1);
                }),
            ));
        }
        {
            let executed = Arc::clone(&executed);
            assert!(schedule_critical_delayed_task(
                now,
                Box::new(move || {
                    executed.lock().unwrap_or_else(|e| e.into_inner()).push(2);
                }),
            )
            .is_ok());
        }

        service_critical_delayed_tasks();
        let got = executed.lock().unwrap_or_else(|e| e.into_inner()).clone();
        assert_eq!(got, vec![2]);
        assert_eq!(pending_counts_for_tests(), (1, 0));

        service_delayed_tasks();
        let got = executed.lock().unwrap_or_else(|e| e.into_inner()).clone();
        assert_eq!(got, vec![2, 1]);
    }
}
