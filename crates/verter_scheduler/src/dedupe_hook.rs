//! Caller-side pre-admission singleflight hook.
//!
//! [`DedupeHook`] lets the *calling* crate collapse duplicate in-flight
//! submissions BEFORE they reach the DAG. It is distinct from the
//! scheduler-internal [`crate::dag::DedupJoinerEvent`]: that one is the
//! waiter-notify fired (after the DAG lock is released) once admission
//! has already joined a request onto an existing waiter group; this one
//! runs on the *caller's* side, before a submission is even constructed,
//! so a caller that already has an equivalent flight live in its own
//! in-flight table can skip the scheduler round-trip entirely and attach
//! as a joiner.
//!
//! # Leaf boundary (H20)
//!
//! The trait has NO `verter_session` / cache-runtime path in any method
//! signature or field. The calling crate implements `DedupeHook` over its
//! own in-flight table; the scheduler stays unaware of that substrate.
//! The probe key is the scheduler's own dedupe identity,
//! [`crate::dag::WorkNodeIdentity`] — the single dedupe-identity
//! authority. Any public dedupe key is a thin wrapper/derivation of
//! `WorkNodeIdentity`, never a parallel key type, so there is a single
//! source of truth for dedupe identity.
//!
//! A caller wires this in by passing a `&dyn DedupeHook` to the
//! submission entry points. When the hook's `probe` returns `Some`, the
//! caller blocks on the existing flight and the scheduler skips enqueue;
//! when it returns `None`, the submission proceeds to admission as usual.

use crate::dag::WorkNodeIdentity;

/// Caller-side pre-admission singleflight hook.
///
/// `Send + Sync` so a `&dyn DedupeHook` / `Arc<dyn DedupeHook>` can be
/// handed to the scheduler and shared across the worker pool.
pub trait DedupeHook: Send + Sync {
    /// Probes whether `identity` is already known to the caller's
    /// in-flight table. If `Some`, the caller blocks on the existing
    /// flight and the scheduler skips enqueue; if `None`, the submission
    /// proceeds to admission as usual.
    fn probe(&self, identity: &WorkNodeIdentity) -> Option<DedupeJoiner>;
}

/// Opaque handle the caller uses to attach a completion as a joiner on an
/// in-flight flight.
///
/// It is intentionally opaque (no public fields): the concrete attach
/// mechanism is the caller's in-flight-table internal. The scheduler only
/// observes its presence/absence as the `Option` discriminant returned
/// from [`DedupeHook::probe`].
#[derive(Debug)]
pub struct DedupeJoiner {
    _opaque: (),
}

impl DedupeJoiner {
    /// Constructs an opaque joiner handle. The caller's `DedupeHook` impl
    /// returns this when a probe matches an in-flight flight.
    #[must_use]
    pub fn new() -> Self {
        Self { _opaque: () }
    }
}

impl Default for DedupeJoiner {
    fn default() -> Self {
        Self::new()
    }
}

/// The genuine no-op `DedupeHook`: it never collapses a submission.
///
/// This is the value used wherever a caller supplies no in-flight table.
/// It is NOT a stub that advertises behaviour it lacks — its contract IS
/// "never deduplicate", and `probe` always returns `None`. Callers that
/// want real singleflight install their own impl.
#[derive(Debug, Clone, Copy, Default)]
pub struct NoDedupeHook;

impl DedupeHook for NoDedupeHook {
    fn probe(&self, _identity: &WorkNodeIdentity) -> Option<DedupeJoiner> {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::dag::FileStageKey;
    use std::sync::Arc;

    fn identity(canonical: &str, generation: u64) -> WorkNodeIdentity {
        WorkNodeIdentity::FileStage {
            canonical: Arc::from(canonical),
            generation,
            stage: FileStageKey::Source,
        }
    }

    #[test]
    fn no_dedupe_hook_always_returns_none() {
        let hook = NoDedupeHook;
        assert!(hook.probe(&identity("/a.vue", 1)).is_none());
        assert!(hook.probe(&identity("/b.vue", 2)).is_none());
    }

    #[test]
    fn trait_object_routes_to_impl() {
        let hook: &dyn DedupeHook = &NoDedupeHook;
        assert!(hook.probe(&identity("/c.vue", 0)).is_none());
    }

    #[test]
    fn joiner_constructs() {
        let _j = DedupeJoiner::new();
        let _d = DedupeJoiner::default();
    }
}
