//! Request-scoped observers: structured audit events, lock-wait and signature counters,
//! and the trace macros that feed the active request's accumulator.

use std::sync::OnceLock;

pub fn component_meta_debug_enabled() -> bool {
    static ENABLED: OnceLock<bool> = OnceLock::new();

    *ENABLED.get_or_init(|| {
        std::env::var_os("VERTER_COMPONENT_META_DEBUG").is_some()
            || std::env::var_os("VERTER_META_DEBUG").is_some()
    })
}

pub fn component_meta_debug(message: impl AsRef<str>) {
    if component_meta_debug_enabled() {
        use std::io::Write;

        let mut stderr = std::io::stderr().lock();
        let _ = writeln!(stderr, "[verter-meta] {}", message.as_ref());
        let _ = stderr.flush();
    }
}

/// Whether the active request asked for timing capture. Without
/// `semantic-observe` timing capture does not exist, so this is the
/// constant `false` and every timing branch behind it compiles away.
#[inline(always)]
#[must_use]
pub fn capture_timing_enabled() -> bool {
    #[cfg(feature = "semantic-observe")]
    {
        verter_execution::request_context::current_timing_enabled()
    }
    #[cfg(not(feature = "semantic-observe"))]
    {
        false
    }
}

/// Route one cache event to the request context the scheduler installed on
/// this thread. Optional capture: without `semantic-observe` it reads no
/// context and records nothing.
#[inline(always)]
pub fn record_cache_event(event: verter_execution::request_context::CacheEventKind) {
    #[cfg(feature = "semantic-observe")]
    if let Some(ctx) = verter_execution::request_context::current_context() {
        ctx.0.record_cache_event(event);
    }
    #[cfg(not(feature = "semantic-observe"))]
    let _ = event;
}

/// Push a structured event into the active request's accumulator.
/// No-op when no request context is installed. Optional capture: exists
/// only with `semantic-observe`; producers emit through
/// [`crate::component_meta_trace_structured!`], which does not build the
/// event in a default build.
#[cfg(feature = "semantic-observe")]
pub fn push_structured_event(event: verter_audit::structured_event::StructuredAuditEvent) {
    if let Some(acc) = crate::request_context::current_accumulator() {
        acc.push_structured_event(event);
    }
}

/// Push a typed `StructuredAuditEvent::CacheDrainedAtUpsert` into
/// the active request's accumulator. Emitted at every cache-cascade
/// drain site reached by the full `host.upsert(...)` path; the
/// quintuple-unchanged fast path does NOT emit this event (R1).
/// Tests observing the absence of these events prove the fast path
/// is a true cache-state no-op.
///
/// The `layer` argument is a static string identifying the cache
/// layer (e.g. `"dependency_cache"`, `"compile_slots"`); the
/// runtime value is stored as an `Arc<str>` so the event remains
/// serialisable.
#[inline(always)]
pub fn push_cache_drained_at_upsert(layer: &'static str, canonical_id: &str) {
    crate::component_meta_trace_structured!(
        verter_audit::structured_event::StructuredAuditEvent::CacheDrainedAtUpsert {
            layer: std::sync::Arc::<str>::from(layer),
            canonical_id: std::sync::Arc::<str>::from(canonical_id),
        }
    );
}

/// Bump `node_arena_lock_acquisitions` on the current request's
/// context AND feed the `WaitAudit` cross-cache aggregates with the
/// observed lock-acquire wait. No-op without a context. The `wait`
/// duration must already be `Duration::ZERO` when the active request's
/// `audit_timing_capture` flag is off — call sites short-circuit
/// `Instant::now()` at that point and pass `Duration::ZERO` here so
/// the zero-cost path is preserved.
#[inline(always)]
pub fn record_node_arena_lock_acquisition(wait: std::time::Duration) {
    #[cfg(not(feature = "semantic-observe"))]
    let _ = wait;
    #[cfg(feature = "semantic-observe")]
    if let Some(ctx) = crate::request_context::current_captured_request_context() {
        // Single mutation point: the observer-trait method on
        // `RequestContext` bumps the per-cache counter (matched on
        // the lock name) AND the cross-cache `WaitAudit` aggregates.
        let wait_ns = wait.as_nanos().min(u64::MAX as u128) as u64;
        <crate::request_context::RequestContext as verter_audit::AuditObserver>::record_lock_acquisition(
            ctx.as_ref(),
            "node_arena",
            wait_ns,
        );
    }
}

/// Bump `family_map_lock_acquisitions` on the current request's
/// context AND feed the `WaitAudit` cross-cache aggregates with the
/// observed lock-acquire wait. No-op without a context. See
/// [`record_node_arena_lock_acquisition`] for the timing-flag
/// contract on the `wait` argument.
#[inline(always)]
pub fn record_family_map_lock_acquisition(wait: std::time::Duration) {
    #[cfg(not(feature = "semantic-observe"))]
    let _ = wait;
    #[cfg(feature = "semantic-observe")]
    if let Some(ctx) = crate::request_context::current_captured_request_context() {
        // Single mutation point: the observer-trait method on
        // `RequestContext` bumps the per-cache counter (matched on
        // the lock name) AND the cross-cache `WaitAudit` aggregates.
        let wait_ns = wait.as_nanos().min(u64::MAX as u128) as u64;
        <crate::request_context::RequestContext as verter_audit::AuditObserver>::record_lock_acquisition(
            ctx.as_ref(),
            "family_map",
            wait_ns,
        );
    }
}

/// Bump `dep_signature_merges` on the current request's context.
/// No-op without a context.
#[inline(always)]
pub fn record_dep_signature_merge() {
    #[cfg(feature = "semantic-observe")]
    if let Some(ctx) = crate::request_context::current_captured_request_context() {
        ctx.dep_signature_merges
            .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    }
}

/// Bump `dep_signature_intern_hits` on the current request's context.
/// No-op without a context.
#[inline(always)]
pub fn record_dep_signature_intern_hit() {
    #[cfg(feature = "semantic-observe")]
    if let Some(ctx) = crate::request_context::current_captured_request_context() {
        ctx.dep_signature_intern_hits
            .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    }
}

/// Construct and push a `StructuredAuditEvent::Custom` into
/// the active request's accumulator. Single in-tree construction
/// site for the `Custom` variant — the
/// `every_custom_variant_construction_site_has_justification_comment`
/// grep test checks each `Custom {` literal
/// has a preceding `// Custom justified:` comment; the rationale
/// below covers every call routed through this helper.
#[cfg(feature = "semantic-observe")]
pub fn push_structured_custom(name: &'static str, detail: impl Into<String>) {
    let name = std::sync::Arc::<str>::from(name);
    let detail = std::sync::Arc::<str>::from(detail.into());
    // Custom justified: debug/trace sites across host_manage,
    // host_resolve, meta_resolve, component_meta_host, and
    // component_meta_audit do not map to typed variants of
    // `StructuredAuditEvent` (RequestStart / VfsRead /
    // MaterializeMemberRoute{Start,End} / etc.). The `Custom`
    // variant exists precisely for ad-hoc structured logging; every
    // call site funnels through this single helper so the
    // justification is centralised and the grep gate has one place
    // to inspect.
    // Custom justified: single construction site for `Custom`
    // across the session crate — see the rationale in the
    // `push_structured_custom` doc comment above.
    push_structured_event(
        verter_audit::structured_event::StructuredAuditEvent::Custom { name, detail },
    );
}

/// Push a typed `StructuredAuditEvent` variant into the
/// current accumulator. — preferred for any call site
/// that maps to a named variant (`IndexedReadyBuilt`, `VfsRead`,
/// `MaterializeMemberRouteStart`, …).
///
/// Optional capture: the event is built only when an accumulator is
/// installed, and without `semantic-observe` it is type-checked but never
/// built.
#[cfg(feature = "semantic-observe")]
#[macro_export]
macro_rules! component_meta_trace_structured {
    ($event:expr $(,)?) => {{
        if $crate::request_context::current_accumulator().is_some() {
            $crate::request_observers::push_structured_event($event);
        }
    }};
}

/// Push a typed `StructuredAuditEvent` variant into the current
/// accumulator. In this default build the event is type-checked but never
/// built.
#[cfg(not(feature = "semantic-observe"))]
#[macro_export]
macro_rules! component_meta_trace_structured {
    ($event:expr $(,)?) => {{
        #[allow(unreachable_code, unused_variables, clippy::redundant_closure_call)]
        if false {
            let _ = || $event;
        }
    }};
}

/// Convenience macro for debug/trace call-sites that don't fit a
/// typed `StructuredAuditEvent` variant — the successor to
/// the deleted `component_meta_trace_scope!` /
/// `component_meta_trace_event!` macros. Expands to a single call
/// into [`push_structured_custom`].
#[cfg(feature = "semantic-observe")]
#[macro_export]
#[doc(hidden)]
macro_rules! component_meta_trace_custom {
    ($name:expr, $detail:expr $(,)?) => {{
        // The accumulator check gates the $detail expression so its
        // allocations (typically a format!) are skipped when no audit
        // run is in flight. Hot-path call sites depend on this.
        if $crate::request_context::current_accumulator().is_some() {
            $crate::request_observers::push_structured_custom($name, $detail);
        }
    }};
}

/// Debug/trace call-site emission. In this default build the name and
/// detail are type-checked but never evaluated.
#[cfg(not(feature = "semantic-observe"))]
#[macro_export]
#[doc(hidden)]
macro_rules! component_meta_trace_custom {
    ($name:expr, $detail:expr $(,)?) => {{
        #[allow(unreachable_code, unused_variables, clippy::redundant_closure_call)]
        if false {
            let _ = || ($name, $detail);
        }
    }};
}
