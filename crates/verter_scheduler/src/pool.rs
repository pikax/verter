//! Bounded I/O thread pool for file reads.
//!
//! Separate from the rayon CPU pool so I/O storms (dependency-heavy miss
//! storms loading many files from disk) cannot starve CPU-bound parse/
//! analyze/compile work.

use crossbeam_channel::{bounded, Sender};

/// Bounded I/O thread pool for file reads and directory walks.
///
/// Fixed-size pool (default: 4 threads) with crossbeam channel dispatch.
/// Separate from the rayon CPU pool to prevent I/O starvation.
///
/// Not available on WASM — I/O runs inline on the calling thread.
#[cfg(not(target_arch = "wasm32"))]
pub struct IoPool {
    sender: Sender<Box<dyn FnOnce() + Send>>,
    _threads: Vec<std::thread::JoinHandle<()>>,
}

#[cfg(not(target_arch = "wasm32"))]
impl IoPool {
    /// Create a new I/O pool with the specified number of threads.
    pub fn new(size: usize) -> Self {
        let size = size.max(1);
        let (sender, receiver) = bounded::<Box<dyn FnOnce() + Send>>(size * 4);
        let mut threads = Vec::with_capacity(size);

        for i in 0..size {
            let rx = receiver.clone();
            threads.push(
                std::thread::Builder::new()
                    .name(format!("verter-io-{i}"))
                    .spawn(move || {
                        while let Ok(task) = rx.recv() {
                            task();
                        }
                    })
                    .expect("failed to spawn I/O pool thread"),
            );
        }

        Self {
            sender,
            _threads: threads,
        }
    }

    /// Submit a task to the I/O pool.
    ///
    /// If the pool is saturated (channel full), this blocks until a slot opens.
    pub fn execute(&self, f: impl FnOnce() + Send + 'static) {
        // Ignore send errors (pool shutting down)
        let _ = self.sender.send(Box::new(f));
    }

    /// Submit a task and get a handle to wait for the result.
    pub fn submit<T: Send + 'static>(&self, f: impl FnOnce() -> T + Send + 'static) -> IoHandle<T> {
        let (tx, rx) = crossbeam_channel::bounded(1);
        self.execute(move || {
            let result = f();
            let _ = tx.send(result);
        });
        IoHandle { rx }
    }

    /// Submit a task with an optional request context installed into
    /// TLS for the duration of the closure. Equivalent to
    /// [`submit`](Self::submit) when `context` is `None`.
    ///
    /// The guard is dropped on both normal-return and panic-unwind
    /// paths, restoring the worker thread's prior TLS slot.
    pub fn submit_with_context<T: Send + 'static>(
        &self,
        context: Option<crate::request_context::OpaqueRequestContext>,
        f: impl FnOnce() -> T + Send + 'static,
    ) -> IoHandle<T> {
        self.submit(move || {
            let _guard = context.map(crate::request_context::OpaqueContextGuard::install);
            f()
        })
    }
}

/// Handle for waiting on an I/O pool task result.
#[cfg(not(target_arch = "wasm32"))]
pub struct IoHandle<T> {
    rx: crossbeam_channel::Receiver<T>,
}

#[cfg(not(target_arch = "wasm32"))]
impl<T> IoHandle<T> {
    /// Block until the task completes and return the result.
    pub fn wait(self) -> T {
        self.rx.recv().expect("I/O task sender dropped")
    }

    /// Non-blocking check.
    pub fn try_get(&self) -> Option<T> {
        self.rx.try_recv().ok()
    }
}

#[cfg(test)]
#[cfg(not(target_arch = "wasm32"))]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::Arc;

    #[test]
    fn io_pool_executes_tasks() {
        let pool = IoPool::new(2);
        let counter = Arc::new(AtomicUsize::new(0));

        let handles: Vec<_> = (0..10)
            .map(|_| {
                let c = Arc::clone(&counter);
                pool.submit(move || {
                    c.fetch_add(1, Ordering::SeqCst);
                    42
                })
            })
            .collect();

        for h in handles {
            assert_eq!(h.wait(), 42);
        }
        assert_eq!(counter.load(Ordering::SeqCst), 10);
    }

    #[test]
    fn io_pool_bounded_concurrency() {
        let pool = IoPool::new(2);
        let active = Arc::new(AtomicUsize::new(0));
        let max_active = Arc::new(AtomicUsize::new(0));

        let handles: Vec<_> = (0..20)
            .map(|_| {
                let a = Arc::clone(&active);
                let m = Arc::clone(&max_active);
                pool.submit(move || {
                    let current = a.fetch_add(1, Ordering::SeqCst) + 1;
                    m.fetch_max(current, Ordering::SeqCst);
                    std::thread::sleep(std::time::Duration::from_millis(5));
                    a.fetch_sub(1, Ordering::SeqCst);
                })
            })
            .collect();

        for h in handles {
            h.wait();
        }

        // Max concurrency should be at most pool size (2)
        assert!(max_active.load(Ordering::SeqCst) <= 2);
    }

    #[test]
    fn io_pool_submit_returns_result() {
        let pool = IoPool::new(1);
        let handle = pool.submit(|| "hello".to_string());
        assert_eq!(handle.wait(), "hello");
    }

    #[test]
    fn io_pool_drop_terminates() {
        let counter = Arc::new(AtomicUsize::new(0));
        {
            let pool = IoPool::new(2);
            let c = Arc::clone(&counter);
            let h = pool.submit(move || {
                c.fetch_add(1, Ordering::SeqCst);
            });
            h.wait();
        }
        // Pool dropped — threads should terminate
        assert_eq!(counter.load(Ordering::SeqCst), 1);
    }
}
