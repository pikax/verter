//! Opaque request-context carriers + TLS accessor for the scheduler.
//!
//! The scheduler is domain-agnostic and must not depend on `verter_session`
//! (the cycle would run the other way — session depends on scheduler). So
//! the session-side `RequestContext` is surfaced through an opaque trait
//! ([`RequestContextLike`]) that the scheduler can store in `Request`,
//! install into worker TLS, and call back into on dedup / cache-event
//! boundaries.
#![deny(missing_docs)]
//!
//! Lifecycle:
//!
//! 1. The session wraps its concrete `RequestContext` in an
//!    [`OpaqueRequestContext`] and attaches it to a [`crate::scheduler::Request`].
//! 2. When the request admits, the driver installs the context into the
//!    worker's TLS via [`OpaqueContextGuard::install`] before running the
//!    stage closure. The guard's RAII `Drop` clears TLS on both the
//!    normal-return and panic-unwind paths.
//! 3. Worker-thread code (VFS reads, scheduler metrics, etc.) can read
//!    the active request id via [`current_request_id`] without needing a
//!    session-crate dependency.
//! 4. Session-side code receives the context directly via
//!    `verter_session::request_context::current_request_context()` (a
//!    separate accessor that downcasts to the concrete session type).

use std::cell::RefCell;
use std::sync::{Arc, OnceLock};

/// Discriminator for cache-event hooks routed through
/// [`RequestContextLike::record_cache_event`]. The session-side
/// implementation dispatches each variant to the matching per-context
/// atomic counter.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CacheEventKind {
    /// Warm memo hit — the callee returned an immutable shared value.
    Hit,
    /// Cache miss — the logical call had to run a cold build.
    Miss,
    /// Joiner woke on a cooperative wait.
    JoinedWait,
    /// Same-path recursion sentinel returned instead of self-await.
    Sentinel,
    /// Cold winner ran a build to completion.
    ColdBuild,
    /// Joiner observed `aborted = true` and re-entered dispatch.
    InflightAbortedRetry,
    /// Cold winner's publish was aborted by a concurrent invalidation.
    ColdAbortSwept,
}

/// RAII handle returned by [`RequestContextLike::install_tls`]. Dropping
/// the box un-installs the TLS entry planted by the concrete context.
pub trait TlsUninstall: Send {
    /// Consume the handle and restore the previous TLS value.
    fn uninstall(self: Box<Self>);
}

/// Type of the host-registered "clear all TLS slots planted by an
/// install" hook. When invoked, the hook clears every TLS slot that
/// the host's [`RequestContextLike::install_tls`] would have planted,
/// and returns an RAII handle whose `uninstall` restores all prior
/// values. Used by the cooperative inline-execute path when the
/// dispatched job has no `winner_ctx`: clearing must be symmetric
/// with the install path so the inline branch never observes the
/// outer request's TLS in any slot.
///
/// The scheduler cannot reach into `verter_session` directly (the
/// crate cycle runs the other way), so the session registers its
/// concrete clear at process startup via [`register_clear_tls_hook`].
pub type ClearTlsHook = fn() -> Box<dyn TlsUninstall + Send>;

static CLEAR_TLS_HOOK: OnceLock<ClearTlsHook> = OnceLock::new();

/// Register the host's concrete "clear all install_tls slots" hook.
/// Called once at process startup by the session crate. Returns
/// `Err(existing)` if a hook has already been registered.
///
/// This is the substrate-level counterpart to
/// [`RequestContextLike::install_tls`]. `install_tls` plants every
/// TLS slot the host owns (scheduler opaque, session request
/// context, audit observer); the hook here clears every one of them
/// symmetrically. Without the hook the inline-execute None-winner_ctx
/// path would only clear the scheduler's opaque slot and the outer
/// request's session + audit TLS would bleed into the inner stage.
pub fn register_clear_tls_hook(hook: ClearTlsHook) -> Result<(), ClearTlsHook> {
    CLEAR_TLS_HOOK.set(hook)
}

/// Invoke the registered "clear all install_tls slots" hook, returning
/// its RAII handle. Falls back to a no-op handle if no hook is
/// registered — the scheduler-side opaque clear is the minimum
/// guarantee; the session-side audit + request-context clears require
/// the host to have wired its hook (any binary that ever installs a
/// session-level context via `RequestContextGuard::install` is
/// expected to register the hook at startup).
pub(crate) fn invoke_clear_tls_hook() -> Box<dyn TlsUninstall + Send> {
    match CLEAR_TLS_HOOK.get() {
        Some(hook) => hook(),
        None => {
            struct NoopUninstall;
            impl TlsUninstall for NoopUninstall {
                fn uninstall(self: Box<Self>) {}
            }
            Box::new(NoopUninstall)
        }
    }
}

/// Session-owned request context surfaced to the scheduler as an opaque
/// trait object. The concrete `Arc<RequestContext>` (in
/// `verter_session::request_context`) implements this.
pub trait RequestContextLike: Send + Sync + 'static {
    /// Monotonic request identifier — stable across the request's
    /// lifetime. Zero is reserved for "no request".
    fn request_id(&self) -> u64;

    /// Whether this request is capturing its semantic footprint (audit
    /// accumulator attached). Consumers of dedup hooks use this to
    /// decide whether a joiner can expect a full audit record from the
    /// winner or must fall back to a "winner unaudited" rendering.
    fn capture_enabled(&self) -> bool;

    /// Whether per-file timing capture is enabled for this request.
    /// Producers gate `Instant::now()` calls and other timing-only
    /// instrumentation behind this flag so the zero-cost path is
    /// preserved when the host's `audit_timing_capture` is `false`.
    /// Default is `false` so adapters and trivial fakes need not
    /// override unless they want timing.
    fn timing_enabled(&self) -> bool {
        false
    }

    /// Per-request cancellation token. Session-owned contexts return one
    /// stable token for their entire lifetime; lightweight adapters may return
    /// `None` to represent an uncancellable owner.
    fn cancellation_token(&self) -> Option<crate::cancellation::CancellationToken> {
        None
    }

    /// Called at the scheduler-dedup join point: this request is a
    /// joiner joining an already-admitted group whose winner is
    /// identified by `winner_request_id` (zero if the winner has no
    /// context). Session-side impl pushes a `SharedLoadReuseRecord`
    /// into the joiner's accumulator, keyed by `canonical_id`.
    fn on_dedup_joiner(&self, canonical_id: Arc<str>, winner_request_id: u64, winner_audited: bool);

    /// Route one cache-event increment through the per-context
    /// counters. Session-side impl dispatches to the matching atomic.
    fn record_cache_event(&self, event: CacheEventKind);

    /// Install `self` into the scheduler's TLS and return an RAII
    /// handle. Dropping the handle restores whatever was in TLS before
    /// the install — never panics (see `OpaqueContextGuard::install`).
    fn install_tls(self: Arc<Self>) -> Box<dyn TlsUninstall + Send>;
}

/// Thin newtype owning an `Arc<dyn RequestContextLike>`. The wrapper
/// exists so downstream crates can pattern-match on `&ctx.0` without
/// juggling the trait-object syntax in every call site.
#[derive(Clone)]
pub struct OpaqueRequestContext(pub Arc<dyn RequestContextLike>);

impl std::fmt::Debug for OpaqueRequestContext {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("OpaqueRequestContext")
            .field("request_id", &self.0.request_id())
            .field("capture_enabled", &self.0.capture_enabled())
            .finish()
    }
}

impl OpaqueRequestContext {
    /// Test-only constructor producing an opaque context tagged by
    /// `request_id`. The returned context's TLS install is a no-op
    /// (the underlying impl never installs anything), and its
    /// `on_dedup_joiner` / `record_cache_event` callbacks are
    /// no-ops. Used by tests that need to assert which submitter's
    /// context survived a dedup join.
    #[cfg(test)]
    pub(crate) fn test_only(request_id: u64) -> Self {
        struct TestOnlyCtx {
            id: u64,
        }
        impl RequestContextLike for TestOnlyCtx {
            fn request_id(&self) -> u64 {
                self.id
            }
            fn capture_enabled(&self) -> bool {
                false
            }
            fn on_dedup_joiner(&self, _c: Arc<str>, _w: u64, _a: bool) {}
            fn record_cache_event(&self, _event: CacheEventKind) {}
            fn install_tls(self: Arc<Self>) -> Box<dyn TlsUninstall + Send> {
                struct NoopUninstall;
                impl TlsUninstall for NoopUninstall {
                    fn uninstall(self: Box<Self>) {}
                }
                Box::new(NoopUninstall)
            }
        }
        OpaqueRequestContext(Arc::new(TestOnlyCtx { id: request_id }) as Arc<dyn RequestContextLike>)
    }
}

thread_local! {
    /// Worker-thread TLS slot. Populated by [`OpaqueContextGuard::install`]
    /// around each stage-closure, cleared by the guard's `Drop`. `replace`
    /// is used to swap atomically — never borrow_mut — so nested installs
    /// never panic.
    static CURRENT_OPAQUE_CONTEXT: RefCell<Option<OpaqueRequestContext>> =
        const { RefCell::new(None) };
}

/// Return the request id of the context currently active on this
/// worker thread, or `None` when no request context is installed.
/// Called from VFS / workspace code that wants to attribute its work
/// to the caller's request without pulling in `verter_session`.
#[must_use]
pub fn current_request_id() -> Option<u64> {
    CURRENT_OPAQUE_CONTEXT.with(|slot| slot.borrow().as_ref().map(|c| c.0.request_id()))
}

/// Return the per-file timing-capture flag for the context currently
/// active on this worker thread, or `false` when no request context
/// is installed. Called from VFS / workspace code that wants to gate
/// `Instant::now()` instrumentation on the host's
/// `audit_timing_capture` flag without pulling in `verter_session`.
#[must_use]
pub fn current_timing_enabled() -> bool {
    CURRENT_OPAQUE_CONTEXT
        .with(|slot| slot.borrow().as_ref().map(|c| c.0.timing_enabled()))
        .unwrap_or(false)
}

/// Return a clone of the currently installed `OpaqueRequestContext`,
/// or `None` when no context is installed. Consumers hold the Arc
/// across suspension points; never borrow the TLS across user code.
#[must_use]
pub fn current_context() -> Option<OpaqueRequestContext> {
    CURRENT_OPAQUE_CONTEXT.with(|slot| slot.borrow().as_ref().cloned())
}

/// RAII guard that installs `ctx` into TLS and restores the prior value
/// on drop. Uses `RefCell::replace` (never `borrow_mut`) so even a
/// nested install on an already-occupied slot cannot panic — the
/// previous slot value is captured and restored unconditionally.
pub struct OpaqueContextGuard {
    prev: Option<OpaqueRequestContext>,
}

impl OpaqueContextGuard {
    /// Install `ctx` into the worker's TLS. The returned guard restores
    /// whatever was previously in the slot when it drops (including
    /// `None`, which is the common case on a fresh worker).
    pub fn install(ctx: OpaqueRequestContext) -> Self {
        let prev = CURRENT_OPAQUE_CONTEXT.with(|slot| slot.replace(Some(ctx)));
        Self { prev }
    }

    /// Clear the worker's opaque TLS slot and capture the prior
    /// value for restoration on drop. The empty-slot mirror of
    /// [`Self::install`].
    ///
    /// Pool-spawn paths run inside an outer `install_tls` guard
    /// whose `Drop` resets the slot — sequential jobs on the same
    /// pool worker observe `None` between jobs, so no explicit
    /// clear is needed there. (Pool workers are persistent threads
    /// reused across jobs, not fresh threads — what protects them
    /// is the install guard's RAII Drop, not thread freshness.)
    ///
    /// Inline-execute paths run ON the calling worker's thread
    /// without a fresh `install_tls`, so when `winner_ctx == None`
    /// the outer stage's TLS would bleed into the inner stage.
    /// The inline-execute clear arm explicitly resets the scheduler
    /// opaque slot for the duration of inline execution; the
    /// session request context and audit observer slots are
    /// cleared in concert via [`AllSlotsClearGuard::clear_all`].
    pub fn clear() -> Self {
        let prev = CURRENT_OPAQUE_CONTEXT.with(|slot| slot.replace(None));
        Self { prev }
    }
}

impl Drop for OpaqueContextGuard {
    fn drop(&mut self) {
        let prev = self.prev.take();
        // `replace` never panics — it swaps values atomically.
        CURRENT_OPAQUE_CONTEXT.with(|slot| {
            slot.replace(prev);
        });
    }
}

/// RAII handle that clears EVERY TLS slot a `RequestContextLike::install_tls`
/// would have planted (scheduler opaque, session request context, audit
/// observer) and restores them all on drop. Used by the cooperative
/// inline-execute path when the dispatched job has no `winner_ctx`.
///
/// The handle is two-part: an in-crate clear of the scheduler's opaque
/// slot, plus the host's registered cross-crate clear (session +
/// audit). Both restore in field-declaration order on drop.
///
/// Field ORDER is load-bearing — Rust drops struct fields in
/// declaration order, so `cross_crate` is declared FIRST to make
/// it drop FIRST. The drop sequence is therefore:
///   1. `cross_crate.drop()` — restores session + audit slots.
///   2. `opaque.drop()`      — restores scheduler opaque slot.
///
/// This is the reverse of
/// `verter_session::request_context::RequestContextGuard::install`
/// (which plants `opaque` first then `audit_observer`), so the
/// install/clear pair un-stacks in matching directions.
pub struct AllSlotsClearGuard {
    // Drops FIRST (declaration-order drop) — restores session
    // request-context + accumulator + audit observer.
    #[allow(dead_code)]
    cross_crate: Box<dyn TlsUninstall + Send>,
    // Drops LAST — restores scheduler opaque slot.
    #[allow(dead_code)]
    opaque: OpaqueContextGuard,
}

impl AllSlotsClearGuard {
    /// Clear every TLS slot the host's install path plants and return
    /// the RAII handle. The handle restores all prior values on drop.
    ///
    /// Always clears the scheduler-side opaque slot directly. Cross-
    /// crate slots (session request context, audit observer) are
    /// cleared via the host-registered hook installed by the session
    /// crate at startup via [`register_clear_tls_hook`]. Binaries
    /// that never install a session-level context observe a no-op
    /// cross-crate hook — the scheduler-only clear is the minimum
    /// guarantee.
    pub fn clear_all() -> Self {
        let opaque = OpaqueContextGuard::clear();
        let cross_crate = invoke_clear_tls_hook();
        Self {
            cross_crate,
            opaque,
        }
    }
}

impl Drop for AllSlotsClearGuard {
    fn drop(&mut self) {
        // Drop body is empty by design — Rust's field-declaration-
        // order drop is the load-bearing sequence:
        //   1. `cross_crate` drops first (declared first) →
        //      restores session + audit slots.
        //   2. `opaque` drops last → restores scheduler opaque slot.
        // The struct comment documents the rationale; touching field
        // order without re-reading that contract is the regression
        // class this layout guards against.
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicU64, Ordering};

    struct TestCtx {
        id: u64,
        captures: bool,
        joined: AtomicU64,
    }

    impl RequestContextLike for TestCtx {
        fn request_id(&self) -> u64 {
            self.id
        }
        fn capture_enabled(&self) -> bool {
            self.captures
        }
        fn on_dedup_joiner(&self, _c: Arc<str>, _w: u64, _a: bool) {
            self.joined.fetch_add(1, Ordering::Relaxed);
        }
        fn record_cache_event(&self, _event: CacheEventKind) {}
        fn install_tls(self: Arc<Self>) -> Box<dyn TlsUninstall + Send> {
            let guard = OpaqueContextGuard::install(OpaqueRequestContext(
                self as Arc<dyn RequestContextLike>,
            ));
            Box::new(GuardBox(guard))
        }
    }

    struct GuardBox(#[allow(dead_code)] OpaqueContextGuard);
    impl TlsUninstall for GuardBox {
        fn uninstall(self: Box<Self>) {}
    }

    #[test]
    fn install_and_current_id_round_trip() {
        let ctx = Arc::new(TestCtx {
            id: 42,
            captures: true,
            joined: AtomicU64::new(0),
        });
        assert_eq!(current_request_id(), None);
        let _guard = OpaqueContextGuard::install(OpaqueRequestContext(Arc::clone(&ctx) as _));
        assert_eq!(current_request_id(), Some(42));
        drop(_guard);
        assert_eq!(current_request_id(), None);
    }

    #[test]
    fn install_uses_replace_no_panic_on_nested() {
        // Two successive installs — the outer's Drop must restore the
        // outer's prior (None), NOT panic on the inner's live borrow.
        let a = Arc::new(TestCtx {
            id: 1,
            captures: false,
            joined: AtomicU64::new(0),
        });
        let b = Arc::new(TestCtx {
            id: 2,
            captures: false,
            joined: AtomicU64::new(0),
        });
        let g1 = OpaqueContextGuard::install(OpaqueRequestContext(Arc::clone(&a) as _));
        assert_eq!(current_request_id(), Some(1));
        let g2 = OpaqueContextGuard::install(OpaqueRequestContext(Arc::clone(&b) as _));
        assert_eq!(current_request_id(), Some(2));
        drop(g2);
        assert_eq!(current_request_id(), Some(1));
        drop(g1);
        assert_eq!(current_request_id(), None);
    }

    #[test]
    fn guard_restores_prior_on_panic_unwind() {
        use std::panic::{catch_unwind, AssertUnwindSafe};
        let ctx = Arc::new(TestCtx {
            id: 7,
            captures: true,
            joined: AtomicU64::new(0),
        });
        let result = catch_unwind(AssertUnwindSafe(|| {
            let _g = OpaqueContextGuard::install(OpaqueRequestContext(Arc::clone(&ctx) as _));
            assert_eq!(current_request_id(), Some(7));
            panic!("simulated worker panic");
        }));
        assert!(result.is_err(), "closure must have unwound");
        assert_eq!(
            current_request_id(),
            None,
            "TLS slot must be cleared after the unwinding guard drop",
        );
    }

    #[test]
    fn install_tls_trait_method_drives_tls_via_concrete_context() {
        let ctx = Arc::new(TestCtx {
            id: 99,
            captures: true,
            joined: AtomicU64::new(0),
        });
        assert_eq!(current_request_id(), None);
        let handle = Arc::clone(&ctx).install_tls();
        assert_eq!(current_request_id(), Some(99));
        handle.uninstall();
        assert_eq!(current_request_id(), None);
    }
}
