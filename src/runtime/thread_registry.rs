use crate::orchestrator::HttpThreadRole;
use crate::util::SpawnCore;
use std::sync::{Mutex, OnceLock};

#[derive(Clone)]
struct ThreadEntry {
    name: String,
    stack_size: usize,
    core_target: Option<SpawnCore>,
    role: HttpThreadRole,
    starts: u32,
    alive: bool,
}

#[derive(Clone, serde::Serialize)]
pub struct ThreadRegistrySnapshot {
    pub alive_threads: usize,
    pub registered_threads: usize,
    pub total_stack_bytes: usize,
    pub io_threads: usize,
    pub interactive_threads: usize,
    pub background_threads: usize,
    pub core0_threads: usize,
    pub core1_threads: usize,
    pub unpinned_threads: usize,
}

static THREADS: OnceLock<Mutex<Vec<ThreadEntry>>> = OnceLock::new();

fn registry() -> &'static Mutex<Vec<ThreadEntry>> {
    THREADS.get_or_init(|| Mutex::new(Vec::new()))
}

pub fn register_thread(
    name: &str,
    stack_size: usize,
    core_target: Option<SpawnCore>,
    role: HttpThreadRole,
) {
    let mut guard = registry().lock().unwrap_or_else(|e| e.into_inner());
    if let Some(entry) = guard.iter_mut().find(|entry| entry.name == name) {
        entry.stack_size = stack_size;
        entry.core_target = core_target;
        entry.role = role;
        entry.starts = entry.starts.saturating_add(1);
        entry.alive = true;
        return;
    }
    guard.push(ThreadEntry {
        name: name.to_string(),
        stack_size,
        core_target,
        role,
        starts: 1,
        alive: true,
    });
}

pub fn mark_thread_stopped(name: &str) {
    let mut guard = registry().lock().unwrap_or_else(|e| e.into_inner());
    if let Some(entry) = guard.iter_mut().find(|entry| entry.name == name) {
        entry.alive = false;
    }
}

pub fn snapshot() -> ThreadRegistrySnapshot {
    let guard = registry().lock().unwrap_or_else(|e| e.into_inner());
    let mut alive_threads = 0usize;
    let mut total_stack_bytes = 0usize;
    let mut io_threads = 0usize;
    let mut interactive_threads = 0usize;
    let mut background_threads = 0usize;
    let mut core0_threads = 0usize;
    let mut core1_threads = 0usize;
    let mut unpinned_threads = 0usize;

    for entry in guard.iter().filter(|entry| entry.alive) {
        alive_threads += 1;
        total_stack_bytes = total_stack_bytes.saturating_add(entry.stack_size);
        match entry.role {
            HttpThreadRole::Io => io_threads += 1,
            HttpThreadRole::Interactive => interactive_threads += 1,
            HttpThreadRole::Background => background_threads += 1,
        }
        match entry.core_target {
            Some(SpawnCore::Core0) => core0_threads += 1,
            Some(SpawnCore::Core1) => core1_threads += 1,
            None => unpinned_threads += 1,
        }
    }

    ThreadRegistrySnapshot {
        alive_threads,
        registered_threads: guard.len(),
        total_stack_bytes,
        io_threads,
        interactive_threads,
        background_threads,
        core0_threads,
        core1_threads,
        unpinned_threads,
    }
}

pub fn format_baseline_log_line() -> String {
    let snapshot = snapshot();
    format!(
        "threads alive={} registered={} stack_total={} io={} interactive={} background={} core0={} core1={} unpinned={}",
        snapshot.alive_threads,
        snapshot.registered_threads,
        snapshot.total_stack_bytes,
        snapshot.io_threads,
        snapshot.interactive_threads,
        snapshot.background_threads,
        snapshot.core0_threads,
        snapshot.core1_threads,
        snapshot.unpinned_threads,
    )
}
