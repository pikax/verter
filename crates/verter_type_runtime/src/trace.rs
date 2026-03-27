use std::cell::RefCell;
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

thread_local! {
    static TYPE_RUNTIME_TRACE_ROOT: RefCell<Option<RuntimeTraceRoot>> = const { RefCell::new(None) };
    static TYPE_RUNTIME_TRACE_STACK: RefCell<Vec<RuntimeTraceStackContext>> = const { RefCell::new(Vec::new()) };
}

fn type_runtime_trace_output_lock() -> &'static Mutex<()> {
    static LOCK: OnceLock<Mutex<()>> = OnceLock::new();
    LOCK.get_or_init(|| Mutex::new(()))
}

fn type_runtime_next_span_id() -> u64 {
    static NEXT_SPAN_ID: AtomicU64 = AtomicU64::new(1);
    NEXT_SPAN_ID.fetch_add(1, Ordering::Relaxed)
}

pub fn type_runtime_trace_enabled() -> bool {
    std::env::var_os("VERTER_COMPONENT_META_TRACE").is_some()
        || std::env::var_os("VERTER_META_TRACE").is_some()
        || std::env::var_os("VERTER_TYPE_RUNTIME_TRACE").is_some()
}

fn type_runtime_trace_output_path() -> Option<std::path::PathBuf> {
    std::env::var_os("VERTER_COMPONENT_META_TRACE_PATH")
        .or_else(|| std::env::var_os("VERTER_META_TRACE_PATH"))
        .or_else(|| std::env::var_os("VERTER_TYPE_RUNTIME_TRACE_PATH"))
        .map(std::path::PathBuf::from)
}

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
}

pub struct TypeRuntimeTraceGuard {
    state: Option<TypeRuntimeTraceGuardState>,
}

impl Drop for TypeRuntimeTraceGuard {
    fn drop(&mut self) {
        let Some(state) = self.state.take() else {
            return;
        };

        TYPE_RUNTIME_TRACE_STACK.with(|stack| {
            let mut stack = stack.borrow_mut();
            let popped = stack.pop();
            debug_assert_eq!(popped.map(|ctx| ctx.span_id), Some(state.span_id));
        });

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

pub fn with_type_runtime_trace_context<T>(
    context: Option<TypeRuntimeTraceContext>,
    f: impl FnOnce() -> T,
) -> T {
    let previous_root = TYPE_RUNTIME_TRACE_ROOT.with(|root| {
        root.replace(context.map(|context| RuntimeTraceRoot {
            request_id: context.request_id,
            parent_span_id: context.parent_span_id,
            base_depth: context.base_depth,
        }))
    });
    let previous_stack =
        TYPE_RUNTIME_TRACE_STACK.with(|stack| std::mem::take(&mut *stack.borrow_mut()));

    let result = f();

    TYPE_RUNTIME_TRACE_STACK.with(|stack| {
        *stack.borrow_mut() = previous_stack;
    });
    TYPE_RUNTIME_TRACE_ROOT.with(|root| {
        root.replace(previous_root);
    });

    result
}

pub fn type_runtime_trace_scope(
    name: &'static str,
    detail: impl Into<String>,
) -> TypeRuntimeTraceGuard {
    if !type_runtime_trace_enabled() {
        return TypeRuntimeTraceGuard { state: None };
    }

    let detail = detail.into();
    let span_id = type_runtime_next_span_id();
    let (trace_id, parent_span_id, depth) = TYPE_RUNTIME_TRACE_STACK.with(|stack| {
        let mut stack = stack.borrow_mut();
        if let Some(parent) = stack.last().copied() {
            let depth = TYPE_RUNTIME_TRACE_ROOT.with(|root| {
                root.borrow()
                    .map(|root| root.base_depth + stack.len())
                    .unwrap_or(stack.len())
            });
            stack.push(RuntimeTraceStackContext {
                trace_id: parent.trace_id,
                span_id,
            });
            return (parent.trace_id, Some(parent.span_id), depth);
        }

        if let Some(root) = TYPE_RUNTIME_TRACE_ROOT.with(|root| *root.borrow()) {
            stack.push(RuntimeTraceStackContext {
                trace_id: root.request_id,
                span_id,
            });
            return (root.request_id, Some(root.parent_span_id), root.base_depth);
        }

        stack.push(RuntimeTraceStackContext {
            trace_id: span_id,
            span_id,
        });
        (span_id, None, 0)
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
        }),
    }
}

pub fn type_runtime_trace_event(name: &'static str, detail: impl Into<String>) {
    if !type_runtime_trace_enabled() {
        return;
    }

    let detail_string = detail.into();
    let (trace_id, parent_span_id, depth) = TYPE_RUNTIME_TRACE_STACK.with(|stack| {
        let stack = stack.borrow();
        if let Some(parent) = stack.last().copied() {
            let depth = TYPE_RUNTIME_TRACE_ROOT.with(|root| {
                root.borrow()
                    .map(|root| root.base_depth + stack.len())
                    .unwrap_or(stack.len())
            });
            return (parent.trace_id, Some(parent.span_id), depth);
        }

        if let Some(root) = TYPE_RUNTIME_TRACE_ROOT.with(|root| *root.borrow()) {
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

#[cfg(test)]
mod tests {
    use super::*;

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
                let _trace = type_runtime_trace_scope("runtime_sync", "backend=tsserver");
                type_runtime_trace_event("runtime_sync_result", "cache_hit=false");
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
}
