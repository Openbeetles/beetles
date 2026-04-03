//! Lightweight delayed task queue polled by existing runtime threads.
//! 轻量延迟任务队列；由已有运行线程轮询执行，不额外创建后台线程。

use std::sync::{Mutex, OnceLock};
use std::time::{Duration, Instant};

type DelayedTask = Box<dyn FnOnce() + Send + 'static>;

struct DelayedTaskJob {
    due_at: Instant,
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

pub fn schedule_delayed_task(due_at: Instant, task: DelayedTask) {
    let mut pending = state().pending.lock().unwrap_or_else(|e| e.into_inner());
    pending.push(DelayedTaskJob {
        due_at,
        task: Some(task),
    });
}

pub fn service_delayed_tasks() {
    let mut due = Vec::new();
    {
        let mut pending = state().pending.lock().unwrap_or_else(|e| e.into_inner());
        let now = Instant::now();
        let mut index = 0usize;
        while index < pending.len() {
            if pending[index].due_at <= now {
                due.push(pending.swap_remove(index));
            } else {
                index += 1;
            }
        }
    }
    due.sort_by_key(|job| job.due_at);
    for mut job in due {
        if let Some(task) = job.task.take() {
            task();
        }
    }
}

pub fn next_delayed_task_wait(max_wait: Duration) -> Duration {
    let pending = state().pending.lock().unwrap_or_else(|e| e.into_inner());
    let now = Instant::now();
    pending
        .iter()
        .map(|job| job.due_at.saturating_duration_since(now))
        .min()
        .map(|wait| wait.min(max_wait))
        .unwrap_or(max_wait)
}

#[cfg(test)]
pub fn reset_delayed_tasks_for_tests() {
    state()
        .pending
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .clear();
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::{Arc, Mutex};

    #[test]
    fn service_delayed_tasks_runs_due_jobs_in_due_order() {
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
        reset_delayed_tasks_for_tests();
        let now = Instant::now();
        schedule_delayed_task(now + Duration::from_millis(15), Box::new(|| {}));
        schedule_delayed_task(now + Duration::from_millis(40), Box::new(|| {}));

        let wait = next_delayed_task_wait(Duration::from_millis(100));
        assert!(wait <= Duration::from_millis(20));
    }
}
