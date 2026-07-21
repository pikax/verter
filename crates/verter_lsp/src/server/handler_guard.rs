// ── Handler tracking for freeze diagnosis ──────────────────────────────

/// Global counter of in-flight LSP request handlers. When this reaches the tokio
/// worker thread count, the runtime is saturated and timers/heartbeats can't fire.
pub(crate) static ACTIVE_HANDLERS: std::sync::atomic::AtomicU32 =
    std::sync::atomic::AtomicU32::new(0);

/// RAII guard that tracks handler lifecycle. Logs entry (with thread ID and active
/// handler count) on creation, logs exit (with duration) on drop.
pub(crate) struct HandlerGuard {
    name: &'static str,
    start: std::time::Instant,
    thread_id: std::thread::ThreadId,
}

impl HandlerGuard {
    pub(crate) fn new(name: &'static str) -> Self {
        let prev = ACTIVE_HANDLERS.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let thread_id = std::thread::current().id();
        // THROWAWAY DIAGNOSTIC (perf/inv-opus): anchor the stack-probe base for
        // this thread at the SHALLOWEST point a request reaches, so any later
        // native growth inside the handler is measured from the true entry.
        verter_session::stack_probe_public::probe(&name, 0);
        tracing::info!(
            "HANDLER_ENTER {name} active={} thread={thread_id:?}",
            prev + 1
        );
        Self {
            name,
            start: std::time::Instant::now(),
            thread_id,
        }
    }
}

impl Drop for HandlerGuard {
    fn drop(&mut self) {
        let remaining = ACTIVE_HANDLERS.fetch_sub(1, std::sync::atomic::Ordering::Relaxed) - 1;
        let elapsed = self.start.elapsed();
        tracing::info!(
            "HANDLER_EXIT {} active={remaining} elapsed={elapsed:?} thread={:?}",
            self.name,
            self.thread_id,
        );
    }
}

pub(crate) fn block_in_place_if_available<R>(f: impl FnOnce() -> R) -> R {
    match tokio::runtime::Handle::try_current() {
        Ok(handle) if handle.runtime_flavor() == tokio::runtime::RuntimeFlavor::MultiThread => {
            tokio::task::block_in_place(f)
        }
        _ => f(),
    }
}
