//! Minimal lazy-initialized executor holder for HTTP internal workers.

use std::sync::{Arc, Mutex, MutexGuard};

pub(crate) struct LazyExecutor<T> {
    state: Arc<LazyExecutorState<T>>,
}

struct LazyExecutorState<T> {
    init: Box<dyn Fn() -> T + Send + Sync>,
    value: Mutex<Option<Arc<T>>>,
}

impl<T> Clone for LazyExecutor<T> {
    fn clone(&self) -> Self {
        Self {
            state: Arc::clone(&self.state),
        }
    }
}

impl<T> LazyExecutor<T> {
    pub(crate) fn new(init: impl Fn() -> T + Send + Sync + 'static) -> Self {
        Self {
            state: Arc::new(LazyExecutorState {
                init: Box::new(init),
                value: Mutex::new(None),
            }),
        }
    }

    pub(crate) fn get(&self) -> Arc<T> {
        {
            let slot = lock_ignore_poison(&self.state.value);
            if let Some(value) = slot.as_ref() {
                return Arc::clone(value);
            }
        }

        let mut slot = lock_ignore_poison(&self.state.value);
        if let Some(value) = slot.as_ref() {
            return Arc::clone(value);
        }

        let value = Arc::new((self.state.init)());
        *slot = Some(Arc::clone(&value));
        value
    }

    pub(crate) fn clear_if(&self, current: &Arc<T>) -> bool {
        let mut slot = lock_ignore_poison(&self.state.value);
        match slot.as_ref() {
            Some(stored) if Arc::ptr_eq(stored, current) => {
                *slot = None;
                true
            }
            _ => false,
        }
    }
}

fn lock_ignore_poison<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

#[cfg(test)]
mod tests {
    use super::LazyExecutor;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::{Arc, Barrier};

    #[test]
    fn lazy_executor_initializes_only_once_for_repeated_gets() {
        let init_calls = Arc::new(AtomicUsize::new(0));
        let executor = LazyExecutor::new({
            let init_calls = Arc::clone(&init_calls);
            move || {
                init_calls.fetch_add(1, Ordering::SeqCst);
                7usize
            }
        });

        let first = executor.get();
        let second = executor.get();

        assert_eq!(*first, 7);
        assert!(Arc::ptr_eq(&first, &second));
        assert_eq!(init_calls.load(Ordering::SeqCst), 1);
    }

    #[test]
    fn lazy_executor_initializes_only_once_under_concurrency() {
        let init_calls = Arc::new(AtomicUsize::new(0));
        let executor = Arc::new(LazyExecutor::new({
            let init_calls = Arc::clone(&init_calls);
            move || {
                init_calls.fetch_add(1, Ordering::SeqCst);
                9usize
            }
        }));
        let barrier = Arc::new(Barrier::new(8));
        let mut threads = Vec::new();

        for _ in 0..8 {
            let executor = Arc::clone(&executor);
            let barrier = Arc::clone(&barrier);
            threads.push(std::thread::spawn(move || {
                barrier.wait();
                executor.get()
            }));
        }

        let first = threads.remove(0).join().expect("thread should succeed");
        for thread in threads {
            let value = thread.join().expect("thread should succeed");
            assert!(Arc::ptr_eq(&first, &value));
        }
        assert_eq!(init_calls.load(Ordering::SeqCst), 1);
    }

    #[test]
    fn lazy_executor_reinitializes_after_clearing_current_value() {
        let init_calls = Arc::new(AtomicUsize::new(0));
        let executor = LazyExecutor::new({
            let init_calls = Arc::clone(&init_calls);
            move || init_calls.fetch_add(1, Ordering::SeqCst) + 1
        });

        let first = executor.get();
        assert_eq!(*first, 1);
        assert!(executor.clear_if(&first));

        let second = executor.get();
        assert_eq!(*second, 2);
        assert!(!Arc::ptr_eq(&first, &second));
        assert_eq!(init_calls.load(Ordering::SeqCst), 2);
    }

    #[test]
    fn lazy_executor_does_not_clear_newer_value_with_stale_handle() {
        let init_calls = Arc::new(AtomicUsize::new(0));
        let executor = LazyExecutor::new({
            let init_calls = Arc::clone(&init_calls);
            move || init_calls.fetch_add(1, Ordering::SeqCst) + 1
        });

        let first = executor.get();
        assert!(executor.clear_if(&first));
        let second = executor.get();

        assert!(!executor.clear_if(&first));
        assert!(executor.clear_if(&second));
        assert_eq!(init_calls.load(Ordering::SeqCst), 2);
    }
}
