//! Minimal lazy-initialized executor holder for HTTP internal workers.

use std::sync::{Arc, Mutex, MutexGuard};

pub(crate) struct LazyExecutor<T, E> {
    state: Arc<LazyExecutorState<T, E>>,
}

struct LazyExecutorState<T, E> {
    init: Box<dyn Fn() -> Result<T, E> + Send + Sync>,
    value: Mutex<Option<Arc<T>>>,
}

impl<T, E> Clone for LazyExecutor<T, E> {
    fn clone(&self) -> Self {
        Self {
            state: Arc::clone(&self.state),
        }
    }
}

impl<T, E> LazyExecutor<T, E> {
    pub(crate) fn new(init: impl Fn() -> Result<T, E> + Send + Sync + 'static) -> Self {
        Self {
            state: Arc::new(LazyExecutorState {
                init: Box::new(init),
                value: Mutex::new(None),
            }),
        }
    }

    pub(crate) fn get(&self) -> Result<Arc<T>, E> {
        {
            let slot = lock_ignore_poison(&self.state.value);
            if let Some(value) = slot.as_ref() {
                return Ok(Arc::clone(value));
            }
        }

        let mut slot = lock_ignore_poison(&self.state.value);
        if let Some(value) = slot.as_ref() {
            return Ok(Arc::clone(value));
        }

        let value = Arc::new((self.state.init)()?);
        *slot = Some(Arc::clone(&value));
        Ok(value)
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
            move || -> Result<usize, &'static str> {
                init_calls.fetch_add(1, Ordering::SeqCst);
                Ok(7usize)
            }
        });

        let first = executor.get().expect("lazy init should succeed");
        let second = executor.get().expect("cached lazy init should succeed");

        assert_eq!(*first, 7);
        assert!(Arc::ptr_eq(&first, &second));
        assert_eq!(init_calls.load(Ordering::SeqCst), 1);
    }

    #[test]
    fn lazy_executor_initializes_only_once_under_concurrency() {
        let init_calls = Arc::new(AtomicUsize::new(0));
        let executor = Arc::new(LazyExecutor::new({
            let init_calls = Arc::clone(&init_calls);
            move || -> Result<usize, &'static str> {
                init_calls.fetch_add(1, Ordering::SeqCst);
                Ok(9usize)
            }
        }));
        let barrier = Arc::new(Barrier::new(8));
        let mut threads = Vec::new();

        for _ in 0..8 {
            let executor = Arc::clone(&executor);
            let barrier = Arc::clone(&barrier);
            threads.push(std::thread::spawn(move || {
                barrier.wait();
                executor.get().expect("lazy init should succeed")
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
            move || -> Result<usize, &'static str> {
                Ok(init_calls.fetch_add(1, Ordering::SeqCst) + 1)
            }
        });

        let first = executor.get().expect("first init should succeed");
        assert_eq!(*first, 1);
        assert!(executor.clear_if(&first));

        let second = executor.get().expect("re-init should succeed");
        assert_eq!(*second, 2);
        assert!(!Arc::ptr_eq(&first, &second));
        assert_eq!(init_calls.load(Ordering::SeqCst), 2);
    }

    #[test]
    fn lazy_executor_does_not_clear_newer_value_with_stale_handle() {
        let init_calls = Arc::new(AtomicUsize::new(0));
        let executor = LazyExecutor::new({
            let init_calls = Arc::clone(&init_calls);
            move || -> Result<usize, &'static str> {
                Ok(init_calls.fetch_add(1, Ordering::SeqCst) + 1)
            }
        });

        let first = executor.get().expect("first init should succeed");
        assert!(executor.clear_if(&first));
        let second = executor.get().expect("second init should succeed");

        assert!(!executor.clear_if(&first));
        assert!(executor.clear_if(&second));
        assert_eq!(init_calls.load(Ordering::SeqCst), 2);
    }

    #[test]
    fn lazy_executor_does_not_cache_failed_initialization() {
        let init_calls = Arc::new(AtomicUsize::new(0));
        let executor = LazyExecutor::new({
            let init_calls = Arc::clone(&init_calls);
            move || -> Result<usize, &'static str> {
                let call = init_calls.fetch_add(1, Ordering::SeqCst);
                if call == 0 {
                    Err("boom")
                } else {
                    Ok(11usize)
                }
            }
        });

        assert_eq!(executor.get().expect_err("first init should fail"), "boom");
        let recovered = executor
            .get()
            .expect("second init should retry instead of reusing the failure");

        assert_eq!(*recovered, 11);
        assert_eq!(init_calls.load(Ordering::SeqCst), 2);
    }
}
