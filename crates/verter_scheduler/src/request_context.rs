//! Opaque request-context carriers + TLS accessor for the scheduler.
//!
//! The scheduler is domain-agnostic and must not depend on `verter_session`
//! (the cycle would run the other way — session depends on scheduler). So
//! the session-side `RequestContext` is surfaced through an opaque trait
//! ([`RequestContextLike`]) that the scheduler can store in `Request`,
//! install into worker TLS, and call back into on dedup / cache-event
//! boundaries.
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
use std::sync::Arc;

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
