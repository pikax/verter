use std::cell::RefCell;
use std::future::Future;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Mutex, OnceLock};
use std::time::{Duration, Instant};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TypeRuntimeTraceEvent {
    Start,
    End,
    Point,
}
impl TypeRuntimeTraceEvent {
    fn as_str(self) -> &'static str {
        match self {
            Self::Start => "start",
            Self::End => "end",
            Self::Point => "point",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TypeRuntimeTraceContext {
    pub request_id: u64,
    pub parent_span_id: u64,
    pub base_depth: usize,
}

#[derive(Debug, Clone, Copy)]
struct RuntimeTraceRoot {
    request_id: u64,
    parent_span_id: u64,
    base_depth: usize,
}

#[derive(Debug, Clone, Copy)]
struct RuntimeTraceStackContext {
    trace_id: u64,
    span_id: u64,
}

/// A trace span-stack scope.
///
/// The same nesting state used by both the synchronous thread-local fallback
/// and the async task-local scopes. Holding the root + LIFO span stack together
/// in one value lets an async scope own its own copy, so guards held across
/// `.await` push/pop against the state that is active during *their own* poll —
/// never a sibling task's stack interleaved on the same OS thread.
#[derive(Debug)]
struct RuntimeTraceState {
    root: Option<RuntimeTraceRoot>,
    stack: Vec<RuntimeTraceStackContext>,
    /// Identity of this state, recorded on every guard so `Drop` can assert it
    /// pops from the same state it pushed onto (a cross-state pop is a bug, not
    /// a recoverable condition).
    state_id: u64,
}

impl RuntimeTraceState {
    fn new(root: Option<RuntimeTraceRoot>) -> Self {
        Self {
            root,
            stack: Vec::new(),
            state_id: type_runtime_next_state_id(),
        }
    }
}

/// Which storage backs the active trace state a guard pushed onto.
///
/// A guard must pop from the exact same storage it pushed onto. Synchronous
/// scopes use the thread-local fallback; await-crossing scopes use a
/// task-local state that travels with the future poll rather than the OS
/// thread.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum TraceStateStorage {
    Sync,
    Async,
}

thread_local! {
    /// Synchronous fallback state for true synchronous code and tests. Async
    /// scopes never touch this; they install their own task-local state.
    static TYPE_RUNTIME_SYNC_TRACE_STATE: RefCell<RuntimeTraceState> =
        RefCell::new(RuntimeTraceState::new(None));
}

tokio::task_local! {
    /// Async scopes install their own state here so the span stack is local to
    /// the future being polled, not the OS thread. Tokio swaps this value
    /// around every poll boundary, so interleaved sibling futures on a
    /// single-threaded runtime each see their own stack.
    static TYPE_RUNTIME_ASYNC_TRACE_STATE: RefCell<RuntimeTraceState>;
}

/// Run `f` with mutable access to whichever trace state is currently active.
///
/// The async task-local takes precedence when present (we are inside an async
/// scope); otherwise the synchronous thread-local fallback is used. The
/// `TraceStateStorage` reported is the one a guard must later pop from.
///
/// `f` is threaded through an `Option` so it is consumed exactly once: when the
/// async task-local is unset, `try_with` does not invoke its closure, leaving
/// `f` available for the synchronous fallback (a plain move would have it
/// captured-but-unused, which the borrow checker rejects).
fn with_active_trace_state<R>(f: impl FnOnce(TraceStateStorage, &mut RuntimeTraceState) -> R) -> R {
    let mut f = Some(f);
    let async_result = TYPE_RUNTIME_ASYNC_TRACE_STATE.try_with(|state| {
        let f = f.take().expect("trace state closure consumed once");
        f(TraceStateStorage::Async, &mut state.borrow_mut())
    });
    match async_result {
        Ok(result) => result,
        Err(_) => TYPE_RUNTIME_SYNC_TRACE_STATE.with(|state| {
            let f = f.take().expect("trace state closure consumed once");
            f(TraceStateStorage::Sync, &mut state.borrow_mut())
        }),
    }
}

/// Run `f` against the state identified by `(storage, state_id)`.
///
/// Used by guard `Drop` to pop from the exact state it pushed onto. Returns
/// `None` when that state is no longer active (e.g. the async scope future was
/// already torn down) so `Drop` can stay fault-contained rather than asserting
/// against an unrelated state.
fn with_state_by_identity<R>(
    storage: TraceStateStorage,
    state_id: u64,
    f: impl FnOnce(&mut RuntimeTraceState) -> R,
) -> Option<R> {
    match storage {
        TraceStateStorage::Async => TYPE_RUNTIME_ASYNC_TRACE_STATE
            .try_with(|state| {
                let mut state = state.borrow_mut();
                (state.state_id == state_id).then(|| f(&mut state))
            })
            .ok()
            .flatten(),
        TraceStateStorage::Sync => TYPE_RUNTIME_SYNC_TRACE_STATE.with(|state| {
            let mut state = state.borrow_mut();
            (state.state_id == state_id).then(|| f(&mut state))
        }),
    }
}

fn type_runtime_trace_output_lock() -> &'static Mutex<()> {
    static LOCK: OnceLock<Mutex<()>> = OnceLock::new();
    LOCK.get_or_init(|| Mutex::new(()))
}

fn type_runtime_next_span_id() -> u64 {
    static NEXT_SPAN_ID: AtomicU64 = AtomicU64::new(1);
    NEXT_SPAN_ID.fetch_add(1, Ordering::Relaxed)
}

fn type_runtime_next_state_id() -> u64 {
    static NEXT_STATE_ID: AtomicU64 = AtomicU64::new(1);
    NEXT_STATE_ID.fetch_add(1, Ordering::Relaxed)
}

pub fn type_runtime_trace_enabled() -> bool {
    // The type-runtime trace responds to its own
    // `VERTER_TYPE_RUNTIME_TRACE` env var plus the shared
    // `VERTER_META_TRACE` name. The legacy `VERTER_COMPONENT_META_TRACE*`
    // surface has been retired workspace-wide.
    std::env::var_os("VERTER_META_TRACE").is_some()
        || std::env::var_os("VERTER_TYPE_RUNTIME_TRACE").is_some()
}

fn type_runtime_trace_output_path() -> Option<std::path::PathBuf> {
    std::env::var_os("VERTER_META_TRACE_PATH")
        .or_else(|| std::env::var_os("VERTER_TYPE_RUNTIME_TRACE_PATH"))
        .map(std::path::PathBuf::from)
}

#[allow(clippy::too_many_arguments)]
pub fn format_type_runtime_trace_line(
    event: TypeRuntimeTraceEvent,
    trace_id: u64,
    span_id: u64,
    parent_span_id: Option<u64>,
    depth: usize,
    name: &str,
    detail: &str,
    duration: Option<Duration>,
) -> String {
    let parent = parent_span_id
        .map(|id| id.to_string())
        .unwrap_or_else(|| "-".to_string());
    let mut line = format!(
        "[verter-meta-trace] event={} trace={} span={} parent={} request={} subrequest={} caller={} depth={} thread={:?} name={:?} detail={:?}",
        event.as_str(),
        trace_id,
        span_id,
        parent,
        trace_id,
        span_id,
        parent,
        depth,
        std::thread::current().id(),
        name,
        detail,
    );
    if let Some(duration) = duration {
        line.push_str(&format!(" dur_ms={:.3}", duration.as_secs_f64() * 1000.0));
    }
    line
}

fn type_runtime_trace_write_line(line: &str) {
    use std::io::Write;

    let _lock = type_runtime_trace_output_lock().lock();
    if let Some(path) = type_runtime_trace_output_path() {
        if let Ok(mut file) = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(&path)
        {
            let _ = writeln!(file, "{line}");
            let _ = file.flush();
            return;
        }
    }

    let mut stderr = std::io::stderr().lock();
    let _ = writeln!(stderr, "{line}");
    let _ = stderr.flush();
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct TypeRuntimeTraceGuardState {
    trace_id: u64,
    span_id: u64,
    parent_span_id: Option<u64>,
    depth: usize,
    name: &'static str,
    detail: String,
    started: Instant,
    /// Storage + identity of the trace state this guard pushed onto. `Drop`
    /// pops from this exact state, never whichever state happens to be active
    /// at drop time.
    storage: TraceStateStorage,
    state_id: u64,
}

pub struct TypeRuntimeTraceGuard {
    state: Option<TypeRuntimeTraceGuardState>,
}

impl TypeRuntimeTraceGuard {
    pub fn noop() -> Self {
        Self { state: None }
    }
}

impl Drop for TypeRuntimeTraceGuard {
    fn drop(&mut self) {
        let Some(state) = self.state.take() else {
            return;
        };

        // Pop from the exact state this guard pushed onto, identified by
        // `(storage, state_id)`. Because each async scope owns its own
        // task-local state and tokio swaps that state around every poll, a
        // guard held across `.await` pops against its own LIFO — sibling
        // tasks interleaved on the same OS thread never corrupt it. A missing
        // state (the scope future was already torn down) is handled as a
        // no-op pop rather than a panic, keeping `Drop` fault-contained.
        let popped = with_state_by_identity(state.storage, state.state_id, |trace_state| {
            trace_state.stack.pop()
        });
        debug_assert_eq!(
            popped.map(|ctx| ctx.map(|ctx| ctx.span_id)),
            Some(Some(state.span_id)),
            "trace guard must pop its own span from its own state",
        );

        type_runtime_trace_write_line(&format_type_runtime_trace_line(
            TypeRuntimeTraceEvent::End,
            state.trace_id,
            state.span_id,
            state.parent_span_id,
            state.depth,
            state.name,
            &state.detail,
            Some(state.started.elapsed()),
        ));
    }
}

/// Run a synchronous closure under a trace root context.
///
/// Seeds the synchronous thread-local fallback state with `context` and a fresh
/// empty span stack, restoring the prior state afterwards. For await-crossing
/// work use [`with_type_runtime_trace_context_async`] instead — it installs a
/// task-local state that travels with the future poll.
pub fn with_type_runtime_trace_context<T>(
    context: Option<TypeRuntimeTraceContext>,
    f: impl FnOnce() -> T,
) -> T {
    let root = context.map(|context| RuntimeTraceRoot {
        request_id: context.request_id,
        parent_span_id: context.parent_span_id,
        base_depth: context.base_depth,
    });
    let previous = TYPE_RUNTIME_SYNC_TRACE_STATE
        .with(|state| std::mem::replace(&mut *state.borrow_mut(), RuntimeTraceState::new(root)));

    let result = f();

    TYPE_RUNTIME_SYNC_TRACE_STATE.with(|state| {
        *state.borrow_mut() = previous;
    });

    result
}

/// Compute `(trace_id, parent_span_id, depth)` for a new span pushed onto
/// `state`, and push it. Shared by the sync and async scope entry points so
/// both derive nesting identically.
fn push_trace_span(state: &mut RuntimeTraceState, span_id: u64) -> (u64, Option<u64>, usize) {
    if let Some(parent) = state.stack.last().copied() {
        let depth = state
            .root
            .map(|root| root.base_depth + state.stack.len())
            .unwrap_or(state.stack.len());
        state.stack.push(RuntimeTraceStackContext {
            trace_id: parent.trace_id,
            span_id,
        });
        return (parent.trace_id, Some(parent.span_id), depth);
    }

    if let Some(root) = state.root {
        state.stack.push(RuntimeTraceStackContext {
            trace_id: root.request_id,
            span_id,
        });
        return (root.request_id, Some(root.parent_span_id), root.base_depth);
    }

    state.stack.push(RuntimeTraceStackContext {
        trace_id: span_id,
        span_id,
    });
    (span_id, None, 0)
}

/// Open a trace span on whichever state is currently active (the async
/// task-local scope when inside one, else the synchronous fallback) and return
/// a guard that closes it on drop.
///
/// The returned guard records the storage + state identity it pushed onto, so
/// it pops from that exact state — this is what makes guards held across
/// `.await` sound under [`type_runtime_trace_scope_async`].
pub fn type_runtime_trace_scope(
    name: &'static str,
    detail: impl Into<String>,
) -> TypeRuntimeTraceGuard {
    if !type_runtime_trace_enabled() {
        return TypeRuntimeTraceGuard { state: None };
    }

    let detail = detail.into();
    let span_id = type_runtime_next_span_id();
    let (storage, state_id, trace_id, parent_span_id, depth) =
        with_active_trace_state(|storage, state| {
            let state_id = state.state_id;
            let (trace_id, parent_span_id, depth) = push_trace_span(state, span_id);
            (storage, state_id, trace_id, parent_span_id, depth)
        });

    type_runtime_trace_write_line(&format_type_runtime_trace_line(
        TypeRuntimeTraceEvent::Start,
        trace_id,
        span_id,
        parent_span_id,
        depth,
        name,
        &detail,
        None,
    ));

    TypeRuntimeTraceGuard {
        state: Some(TypeRuntimeTraceGuardState {
            trace_id,
            span_id,
            parent_span_id,
            depth,
            name,
            detail,
            started: Instant::now(),
            storage,
            state_id,
        }),
    }
}

/// Snapshot the currently active trace position as a propagatable context.
///
/// Captures the top span (if any) as the parent, so spawned child work can be
/// re-parented under it via [`with_type_runtime_trace_context_async`]. Returns
/// `None` when there is no active span/root to inherit.
pub fn current_type_runtime_trace_context() -> Option<TypeRuntimeTraceContext> {
    with_active_trace_state(|_storage, state| {
        if let Some(top) = state.stack.last().copied() {
            let depth = state
                .root
                .map(|root| root.base_depth + state.stack.len())
                .unwrap_or(state.stack.len());
            return Some(TypeRuntimeTraceContext {
                request_id: top.trace_id,
                parent_span_id: top.span_id,
                base_depth: depth,
            });
        }
        state.root.map(|root| TypeRuntimeTraceContext {
            request_id: root.request_id,
            parent_span_id: root.parent_span_id,
            base_depth: root.base_depth,
        })
    })
}

/// Run `future` under a fresh async trace state seeded from `context`.
///
/// The state lives in a task-local that tokio scopes to the future poll, so
/// guards opened inside `future` (including ones held across `.await`) push and
/// pop against this state rather than the OS thread — sound even when sibling
/// futures interleave on a single-threaded runtime. Nested calls stack
/// naturally: the inner scope seeds from the outer's active position.
pub async fn with_type_runtime_trace_context_async<F>(
    context: Option<TypeRuntimeTraceContext>,
    future: F,
) -> F::Output
where
    F: Future,
{
    let root = context.map(|context| RuntimeTraceRoot {
        request_id: context.request_id,
        parent_span_id: context.parent_span_id,
        base_depth: context.base_depth,
    });
    TYPE_RUNTIME_ASYNC_TRACE_STATE
        .scope(RefCell::new(RuntimeTraceState::new(root)), future)
        .await
}

/// Open an await-crossing trace span around `future`.
///
/// Equivalent to opening a [`type_runtime_trace_scope`] guard and holding it
/// across the future, but the span lives inside a per-future task-local state
/// (seeded from the current top span) so it is correct under concurrency. The
/// guard is created and dropped inside the scoped future, so both its push and
/// its pop run while this future's state is the active task-local.
///
/// `detail` is `None` when tracing is disabled (the caller's
/// [`type_runtime_trace_scope_async!`] macro skips building it), so the
/// disabled path stays a zero-allocation passthrough that just awaits `future`.
/// Passing `Some(_)` while tracing is disabled still degrades to a passthrough.
pub async fn type_runtime_trace_scope_async<F>(
    name: &'static str,
    detail: Option<String>,
    future: F,
) -> F::Output
where
    F: Future,
{
    let Some(detail) = detail.filter(|_| type_runtime_trace_enabled()) else {
        return future.await;
    };

    let context = current_type_runtime_trace_context();
    with_type_runtime_trace_context_async(context, async move {
        let _trace = type_runtime_trace_scope(name, detail);
        future.await
    })
    .await
}

pub fn type_runtime_trace_event(name: &'static str, detail: impl Into<String>) {
    if !type_runtime_trace_enabled() {
        return;
    }

    let detail_string = detail.into();
    let (trace_id, parent_span_id, depth) = with_active_trace_state(|_storage, state| {
        if let Some(parent) = state.stack.last().copied() {
            let depth = state
                .root
                .map(|root| root.base_depth + state.stack.len())
                .unwrap_or(state.stack.len());
            return (parent.trace_id, Some(parent.span_id), depth);
        }

        if let Some(root) = state.root {
            return (root.request_id, Some(root.parent_span_id), root.base_depth);
        }

        (0, None, 0)
    });
    let span_id = type_runtime_next_span_id();

    type_runtime_trace_write_line(&format_type_runtime_trace_line(
        TypeRuntimeTraceEvent::Point,
        if trace_id == 0 { span_id } else { trace_id },
        span_id,
        parent_span_id,
        depth,
        name,
        &detail_string,
        None,
    ));
}

#[macro_export]
macro_rules! type_runtime_trace_scope {
    ($name:expr, $detail:expr $(,)?) => {{
        if $crate::trace::type_runtime_trace_enabled() {
            $crate::trace::type_runtime_trace_scope($name, $detail)
        } else {
            $crate::trace::TypeRuntimeTraceGuard::noop()
        }
    }};
}

/// Open an await-crossing trace span around `$future`.
///
/// Use this instead of [`type_runtime_trace_scope!`] whenever the span must be
/// held across `.await`: the span lives in a per-future task-local state, so
/// interleaved sibling futures on a single-threaded runtime cannot corrupt each
/// other's span stack. Awaiting the result yields the future's output.
///
/// `$detail` is only built when tracing is enabled (disabled-mode laziness),
/// and it is materialised to an owned `String` *before* `$future` is
/// constructed — so a detail expression and the future may both reference the
/// same local even when the future moves it (the detail's borrow is a
/// temporary that ends before the move).
#[macro_export]
macro_rules! type_runtime_trace_scope_async {
    ($name:expr, $detail:expr, $future:expr $(,)?) => {
        $crate::trace::type_runtime_trace_scope_async(
            $name,
            if $crate::trace::type_runtime_trace_enabled() {
                ::core::option::Option::Some($detail)
            } else {
                ::core::option::Option::None
            },
            $future,
        )
    };
}

#[macro_export]
macro_rules! type_runtime_trace_event {
    ($name:expr, $detail:expr $(,)?) => {{
        if $crate::trace::type_runtime_trace_enabled() {
            $crate::trace::type_runtime_trace_event($name, $detail);
        }
    }};
}

/// Process-wide serialization point for tests that mutate the `VERTER_*` trace
/// environment variables. Those variables are global, so every test that flips
/// them — in this module AND in the transport `ipc_tests` — must hold this one
/// lock or they race. Acquisition is poison-tolerant: a panic in one env-test
/// must not cascade-fail unrelated env-tests via `PoisonError`.
#[cfg(test)]
pub(crate) fn test_trace_env_guard() -> std::sync::MutexGuard<'static, ()> {
    static LOCK: OnceLock<Mutex<()>> = OnceLock::new();
    LOCK.get_or_init(|| Mutex::new(()))
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::Cell;

    #[test]
    fn format_runtime_trace_line_uses_component_meta_shape() {
        let line = format_type_runtime_trace_line(
            TypeRuntimeTraceEvent::Point,
            11,
            22,
            Some(7),
            3,
            "runtime_hover",
            "backend=tsgo",
            Some(Duration::from_millis(5)),
        );
        assert!(line.contains("request=11"));
        assert!(line.contains("subrequest=22"));
        assert!(line.contains("caller=7"));
        assert!(line.contains("name=\"runtime_hover\""));
        assert!(line.contains("dur_ms=5.000"));
    }

    #[test]
    fn runtime_trace_scope_inherits_host_request_context() {
        let _guard = test_trace_env_guard();
        let dir = std::env::temp_dir();
        let path = dir.join(format!(
            "verter-type-runtime-trace-{}-{}.log",
            std::process::id(),
            type_runtime_next_span_id()
        ));
        let _ = std::fs::remove_file(&path);

        unsafe {
            std::env::set_var("VERTER_TYPE_RUNTIME_TRACE", "1");
            std::env::set_var("VERTER_TYPE_RUNTIME_TRACE_PATH", &path);
        }

        with_type_runtime_trace_context(
            Some(TypeRuntimeTraceContext {
                request_id: 41,
                parent_span_id: 17,
                base_depth: 4,
            }),
            || {
                let _trace = crate::type_runtime_trace_scope!("runtime_sync", "backend=tsserver");
                crate::type_runtime_trace_event!("runtime_sync_result", "cache_hit=false");
            },
        );

        let contents = std::fs::read_to_string(&path).expect("trace file should exist");
        assert!(contents.contains("request=41"));
        assert!(contents.contains("parent=17"));
        assert!(contents.contains("name=\"runtime_sync\""));
        assert!(contents.contains("name=\"runtime_sync_result\""));

        let _ = std::fs::remove_file(path);
        unsafe {
            std::env::remove_var("VERTER_TYPE_RUNTIME_TRACE");
            std::env::remove_var("VERTER_TYPE_RUNTIME_TRACE_PATH");
        }
    }

    #[test]
    fn runtime_trace_macros_skip_detail_evaluation_when_disabled() {
        let _guard = test_trace_env_guard();
        unsafe {
            std::env::remove_var("VERTER_META_TRACE");
            std::env::remove_var("VERTER_TYPE_RUNTIME_TRACE");
            std::env::remove_var("VERTER_TYPE_RUNTIME_TRACE_PATH");
        }

        let scope_detail_evaluated = Cell::new(false);
        let _trace = crate::type_runtime_trace_scope!("runtime_disabled", {
            scope_detail_evaluated.set(true);
            "disabled scope detail".to_string()
        });
        assert!(!scope_detail_evaluated.get());

        let event_detail_evaluated = Cell::new(false);
        crate::type_runtime_trace_event!("runtime_disabled_result", {
            event_detail_evaluated.set(true);
            "disabled event detail".to_string()
        });
        assert!(!event_detail_evaluated.get());
    }

    #[test]
    fn async_trace_scope_skips_detail_evaluation_when_disabled() {
        let _guard = test_trace_env_guard();
        unsafe {
            std::env::remove_var("VERTER_META_TRACE");
            std::env::remove_var("VERTER_TYPE_RUNTIME_TRACE");
            std::env::remove_var("VERTER_TYPE_RUNTIME_TRACE_PATH");
        }

        let detail_evaluated = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
        let detail_flag = std::sync::Arc::clone(&detail_evaluated);
        let rt = tokio::runtime::Builder::new_current_thread()
            .build()
            .unwrap();
        let value = rt.block_on(async move {
            crate::type_runtime_trace_scope_async!(
                "runtime_disabled_async",
                {
                    detail_flag.store(true, std::sync::atomic::Ordering::SeqCst);
                    "disabled async detail".to_string()
                },
                async { 7u32 }
            )
            .await
        });
        assert_eq!(value, 7);
        assert!(
            !detail_evaluated.load(std::sync::atomic::Ordering::SeqCst),
            "async scope detail must not be evaluated when tracing is disabled"
        );
    }

    /// The core discriminator at the trace-module level: two await-crossing
    /// trace scopes interleaved on one current-thread runtime. Under the old
    /// thread-local LIFO this tripped the `debug_assert_eq!` span-id invariant
    /// when one task's `.await` let another task push/pop in between. With the
    /// per-future task-local state each scope pops its own span, so the run
    /// completes without panicking even with tracing active.
    #[test]
    fn interleaved_async_trace_scopes_do_not_corrupt_span_stack() {
        let _guard = test_trace_env_guard();
        let dir = std::env::temp_dir();
        let path = dir.join(format!(
            "verter-type-runtime-trace-interleave-{}-{}.log",
            std::process::id(),
            type_runtime_next_span_id()
        ));
        let _ = std::fs::remove_file(&path);
        unsafe {
            std::env::set_var("VERTER_TYPE_RUNTIME_TRACE", "1");
            std::env::set_var("VERTER_TYPE_RUNTIME_TRACE_PATH", &path);
        }

        let rt = tokio::runtime::Builder::new_current_thread()
            .enable_time()
            .build()
            .unwrap();
        rt.block_on(async {
            // Each future holds an await-crossing scope guard while yielding,
            // forcing the runtime to interleave their push/pop sequences.
            let one = crate::type_runtime_trace_scope_async!(
                "scope_one",
                "task=one".to_string(),
                async {
                    tokio::task::yield_now().await;
                    let _inner = crate::type_runtime_trace_scope!("scope_one_inner", "depth=2");
                    tokio::task::yield_now().await;
                    1u32
                }
            );
            let two = crate::type_runtime_trace_scope_async!(
                "scope_two",
                "task=two".to_string(),
                async {
                    tokio::task::yield_now().await;
                    let _inner = crate::type_runtime_trace_scope!("scope_two_inner", "depth=2");
                    tokio::task::yield_now().await;
                    2u32
                }
            );
            let (a, b) = tokio::join!(one, two);
            assert_eq!((a, b), (1, 2));
        });

        // Both scopes emitted balanced start/end pairs without tripping the
        // span-id assertion (a trip would have panicked the run above).
        let contents = std::fs::read_to_string(&path).expect("trace file should exist");
        assert!(contents.contains("name=\"scope_one\""));
        assert!(contents.contains("name=\"scope_two\""));

        let _ = std::fs::remove_file(path);
        unsafe {
            std::env::remove_var("VERTER_TYPE_RUNTIME_TRACE");
            std::env::remove_var("VERTER_TYPE_RUNTIME_TRACE_PATH");
        }
    }

    /// A nested async scope inherits the enclosing scope's span as its parent,
    /// proving the task-local state is seeded from the active context rather
    /// than starting detached.
    #[test]
    fn nested_async_trace_scope_parents_under_outer_span() {
        let _guard = test_trace_env_guard();
        let dir = std::env::temp_dir();
        let path = dir.join(format!(
            "verter-type-runtime-trace-nested-{}-{}.log",
            std::process::id(),
            type_runtime_next_span_id()
        ));
        let _ = std::fs::remove_file(&path);
        unsafe {
            std::env::set_var("VERTER_TYPE_RUNTIME_TRACE", "1");
            std::env::set_var("VERTER_TYPE_RUNTIME_TRACE_PATH", &path);
        }

        let rt = tokio::runtime::Builder::new_current_thread()
            .build()
            .unwrap();
        let observed = rt.block_on(async {
            crate::type_runtime_trace_scope_async!("outer", "level=1".to_string(), async {
                let outer = current_type_runtime_trace_context().expect("outer context");
                let inner =
                    crate::type_runtime_trace_scope_async!("inner", "level=2".to_string(), async {
                        current_type_runtime_trace_context().expect("inner context")
                    })
                    .await;
                (outer, inner)
            })
            .await
        });

        let (outer, inner) = observed;
        // The inner scope inherits the outer's logical request tree (same
        // `request_id`) and nests strictly deeper. A detached inner scope
        // would mint a fresh `request_id` equal to its own span id and reset
        // depth — both assertions discriminate that regression.
        assert_eq!(
            inner.request_id, outer.request_id,
            "nested async scope must share the outer scope's trace tree"
        );
        assert!(
            inner.base_depth > outer.base_depth,
            "nested async scope must nest deeper than its parent (outer={}, inner={})",
            outer.base_depth,
            inner.base_depth
        );
        // The inner's parent is the inner scope's own span (the active top
        // span at the point of capture), distinct from the outer's span.
        assert_ne!(inner.parent_span_id, outer.parent_span_id);

        let _ = std::fs::remove_file(path);
        unsafe {
            std::env::remove_var("VERTER_TYPE_RUNTIME_TRACE");
            std::env::remove_var("VERTER_TYPE_RUNTIME_TRACE_PATH");
        }
    }
}
