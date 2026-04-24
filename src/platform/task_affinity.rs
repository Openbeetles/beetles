//! ESP 线程绑核执行面：统一封装 pthread cfg，避免各处直接操作底层 API。
//! Unified ESP thread affinity surface for safe/centralized pthread cfg handling.

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TaskCore {
    Core0,
    Core1,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "snake_case")]
/// 线程底层执行面：标准线程层或 ESP 原生任务。
pub enum TaskSpawnSurface {
    /// Rust std-compatible pthread surface. `std::sync` blocking primitives are allowed here.
    StdThreadCompat,
    /// Raw ESP/FreeRTOS task surface. Only FreeRTOS-native blocking primitives are allowed here.
    EspNativeTask,
}

#[cfg(any(target_arch = "xtensa", target_arch = "riscv32", test))]
#[derive(Clone, Copy, Debug)]
struct NativeTaskPolicy {
    name: &'static str,
    reason: &'static str,
}

#[cfg(any(target_arch = "xtensa", target_arch = "riscv32", test))]
const ESP_NATIVE_TASK_ALLOWLIST: &[NativeTaskPolicy] = &[
    // Intentionally empty until a worker is proven to use FreeRTOS-native
    // blocking primitives only. Rust closures with Mutex/Condvar/mpsc stay on
    // `StdThreadCompat` even when pinned to a core.
];

#[cfg(any(target_arch = "xtensa", target_arch = "riscv32", test))]
fn native_task_policy(name: &str) -> Option<&'static NativeTaskPolicy> {
    ESP_NATIVE_TASK_ALLOWLIST
        .iter()
        .find(|policy| policy.name == name)
}

#[cfg(any(target_arch = "xtensa", target_arch = "riscv32", test))]
pub fn native_task_allowlist_hit(name: &str) -> bool {
    native_task_policy(name).is_some()
}

#[cfg(any(target_arch = "xtensa", target_arch = "riscv32", test))]
fn native_task_name_policy(name: &str) -> bool {
    native_task_allowlist_hit(name)
}

#[cfg(all(not(any(target_arch = "xtensa", target_arch = "riscv32")), not(test)))]
pub fn native_task_allowlist_hit(_name: &str) -> bool {
    false
}

#[cfg(any(target_arch = "xtensa", target_arch = "riscv32"))]
mod imp {
    use super::{native_task_policy, TaskCore, TaskSpawnSurface};
    use core::ffi::c_void;
    use esp_idf_hal::cpu::Core;
    use esp_idf_hal::task;
    use esp_idf_hal::task::thread::ThreadSpawnConfiguration;
    use std::ffi::CString;
    use std::io;
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::sync::{Arc, Condvar, Mutex};

    static PTHREAD_CFG_LOCK: Mutex<()> = Mutex::new(());
    type TaskBody = Box<dyn FnOnce() + Send + 'static>;

    struct NativeTaskCompletion {
        finished: AtomicBool,
        lock: Mutex<bool>,
        cv: Condvar,
    }

    impl NativeTaskCompletion {
        fn new() -> Self {
            Self {
                finished: AtomicBool::new(false),
                lock: Mutex::new(false),
                cv: Condvar::new(),
            }
        }

        fn mark_finished(&self) {
            self.finished.store(true, Ordering::Release);
            let mut guard = self.lock.lock().unwrap_or_else(|e| e.into_inner());
            *guard = true;
            self.cv.notify_all();
        }

        fn is_finished(&self) -> bool {
            self.finished.load(Ordering::Acquire)
        }

        fn wait(&self) {
            if self.is_finished() {
                return;
            }
            let mut guard = self.lock.lock().unwrap_or_else(|e| e.into_inner());
            while !*guard {
                guard = self.cv.wait(guard).unwrap_or_else(|e| e.into_inner());
            }
        }
    }

    struct NativeTaskBootstrap {
        body: Option<TaskBody>,
        completion: Arc<NativeTaskCompletion>,
    }

    enum TaskHandleInner {
        Std(std::thread::JoinHandle<()>),
        Native {
            completion: Arc<NativeTaskCompletion>,
        },
    }

    pub struct TaskHandle(TaskHandleInner);

    impl TaskHandle {
        pub fn is_finished(&self) -> bool {
            match &self.0 {
                TaskHandleInner::Std(handle) => handle.is_finished(),
                TaskHandleInner::Native { completion } => completion.is_finished(),
            }
        }

        pub fn join(self) -> std::thread::Result<()> {
            match self.0 {
                TaskHandleInner::Std(handle) => handle.join(),
                TaskHandleInner::Native { completion } => {
                    completion.wait();
                    Ok(())
                }
            }
        }
    }

    fn map_core(core: TaskCore) -> Core {
        match core {
            TaskCore::Core0 => Core::Core0,
            TaskCore::Core1 => Core::Core1,
        }
    }

    fn io_other(msg: impl Into<String>) -> io::Error {
        io::Error::other(msg.into())
    }

    fn default_task_priority() -> u8 {
        ThreadSpawnConfiguration::default().priority
    }

    fn should_use_native_task(name: &str) -> bool {
        super::native_task_name_policy(name)
    }

    fn spawn_std_thread<F>(
        name: String,
        stack_size: usize,
        core: Option<TaskCore>,
        f: F,
    ) -> io::Result<TaskHandle>
    where
        F: FnOnce() + Send + 'static,
    {
        if core.is_none() {
            let handle = std::thread::Builder::new()
                .name(name)
                .stack_size(stack_size)
                .spawn(f)?;
            return Ok(TaskHandle(TaskHandleInner::Std(handle)));
        }

        let _guard = PTHREAD_CFG_LOCK
            .lock()
            .map_err(|e| io_other(format!("task_affinity lock poisoned: {}", e)))?;
        let mut cfg = ThreadSpawnConfiguration::get().unwrap_or_default();
        cfg.pin_to_core = core.map(map_core);
        cfg.inherit = false;
        cfg.set()
            .map_err(|e| io_other(format!("task_affinity set cfg failed: {}", e)))?;

        let spawn_result = std::thread::Builder::new()
            .name(name)
            .stack_size(stack_size)
            .spawn(f)
            .map(|handle| TaskHandle(TaskHandleInner::Std(handle)));

        let restore_result = ThreadSpawnConfiguration::default().set();
        if let Err(e) = restore_result {
            log::warn!("[task_affinity] restore pthread cfg failed: {}", e);
        }

        spawn_result
    }

    extern "C" fn native_task_entry(arg: *mut c_void) {
        let mut bootstrap: Box<NativeTaskBootstrap> =
            unsafe { Box::from_raw(arg.cast::<NativeTaskBootstrap>()) };
        if let Some(body) = bootstrap.body.take() {
            body();
        }
        bootstrap.completion.mark_finished();
        unsafe {
            task::destroy(core::ptr::null_mut());
        }
    }

    fn spawn_native_task<F>(
        name: String,
        stack_size: usize,
        core: Option<TaskCore>,
        f: F,
    ) -> io::Result<TaskHandle>
    where
        F: FnOnce() + Send + 'static,
    {
        let task_name = CString::new(name.as_str())
            .map_err(|e| io_other(format!("task_affinity invalid task name '{}': {}", name, e)))?;
        let completion = Arc::new(NativeTaskCompletion::new());
        let bootstrap = Box::new(NativeTaskBootstrap {
            body: Some(Box::new(f) as TaskBody),
            completion: Arc::clone(&completion),
        });
        let raw = Box::into_raw(bootstrap);

        let result = unsafe {
            task::create(
                native_task_entry,
                task_name.as_c_str(),
                stack_size,
                raw.cast::<c_void>(),
                default_task_priority(),
                core.map(map_core),
            )
        };

        if let Err(err) = result {
            unsafe {
                drop(Box::from_raw(raw));
            }
            return Err(io_other(format!(
                "task_affinity create native task '{}' failed: {}",
                name, err
            )));
        }

        Ok(TaskHandle(TaskHandleInner::Native { completion }))
    }

    pub fn planned_spawn_surface(name: &str) -> TaskSpawnSurface {
        if should_use_native_task(name) {
            if let Some(policy) = native_task_policy(name) {
                log::info!(
                    "[task_affinity] native task allowlist hit name={} reason={}",
                    policy.name,
                    policy.reason
                );
            }
            TaskSpawnSurface::EspNativeTask
        } else {
            TaskSpawnSurface::StdThreadCompat
        }
    }

    pub fn current_task_handle_key() -> usize {
        unsafe { esp_idf_hal::sys::xTaskGetCurrentTaskHandle() as usize }
    }

    pub fn spawn_named_with_affinity<F>(
        name: String,
        stack_size: usize,
        core: Option<TaskCore>,
        f: F,
    ) -> io::Result<TaskHandle>
    where
        F: FnOnce() + Send + 'static,
    {
        match planned_spawn_surface(name.as_str()) {
            TaskSpawnSurface::StdThreadCompat => spawn_std_thread(name, stack_size, core, f),
            TaskSpawnSurface::EspNativeTask => spawn_native_task(name, stack_size, core, f),
        }
    }
}

#[cfg(not(any(target_arch = "xtensa", target_arch = "riscv32")))]
mod imp {
    use super::{TaskCore, TaskSpawnSurface};
    use std::io;

    pub struct TaskHandle {
        inner: std::thread::JoinHandle<()>,
    }

    impl TaskHandle {
        pub fn is_finished(&self) -> bool {
            self.inner.is_finished()
        }

        pub fn join(self) -> std::thread::Result<()> {
            self.inner.join()
        }
    }

    pub fn planned_spawn_surface(_name: &str) -> TaskSpawnSurface {
        TaskSpawnSurface::StdThreadCompat
    }

    pub fn spawn_named_with_affinity<F>(
        name: String,
        stack_size: usize,
        _core: Option<TaskCore>,
        f: F,
    ) -> io::Result<TaskHandle>
    where
        F: FnOnce() + Send + 'static,
    {
        let handle = std::thread::Builder::new()
            .name(name)
            .stack_size(stack_size)
            .spawn(f)?;
        Ok(TaskHandle { inner: handle })
    }
}

pub use imp::planned_spawn_surface;
pub use imp::spawn_named_with_affinity;
pub use imp::TaskHandle;

#[cfg(any(target_arch = "xtensa", target_arch = "riscv32"))]
pub fn current_task_handle_key() -> usize {
    imp::current_task_handle_key()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rust_std_blocking_workers_stay_on_std_thread_compat_surface() {
        assert_eq!(
            planned_spawn_surface("voice_realtime"),
            TaskSpawnSurface::StdThreadCompat
        );
        assert_eq!(
            planned_spawn_surface("voice_realtime_connect"),
            TaskSpawnSurface::StdThreadCompat
        );
        assert_eq!(
            planned_spawn_surface("agent_loop"),
            TaskSpawnSurface::StdThreadCompat
        );
        assert_eq!(
            planned_spawn_surface("audio_io_worker"),
            TaskSpawnSurface::StdThreadCompat
        );
    }

    #[test]
    fn native_task_allowlist_is_explicit_and_empty_until_freertos_native_worker_exists() {
        assert!(ESP_NATIVE_TASK_ALLOWLIST.is_empty());
        let reason_bytes: usize = ESP_NATIVE_TASK_ALLOWLIST
            .iter()
            .map(|policy| policy.reason.len())
            .sum();
        assert_eq!(reason_bytes, 0);
        for name in [
            "http_config_exec",
            "http_diag_exec",
            "http_ota_exec",
            "http_snapshot_exec",
        ] {
            assert!(!native_task_name_policy(name));
        }
    }
}
