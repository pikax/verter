//! `DedupeHook` — caller-side pre-admission singleflight contract.
//!
//! `DedupeHook` is the *caller-side* pre-admission singleflight hook,
//! distinct from the scheduler-internal post-unlock `DedupJoinerEvent`.
//! A caller (the session `cache_runtime`) implements `DedupeHook` over
//! its own in-flight table to collapse duplicate in-flight submissions
//! BEFORE they reach the DAG — the scheduler crate stays unaware of the
//! cache-runtime substrate (H20).
//!
//! The contract is the narrowest sound shape: `probe(&WorkNodeIdentity)
//! -> Option<DedupeJoiner>`. The scheduler dedupe identity is
//! `WorkNodeIdentity` (the single-authority decision). It also ships a
//! genuine no-op default (`NoDedupeHook`) that always returns `None`
//! (never collapses) so the "no hook supplied" path is a real,
//! exercised value, not an advertised behaviour it lacks.
//!
//! These tests EXERCISE the contract (they are not satisfied by an empty
//! trait):
//!
//! 1. A real `DedupeHook` impl backed by an in-flight set returns
//!    `Some(joiner)` for a known identity and `None` for an unknown one —
//!    proving the probe routes the identity through caller state.
//! 2. The shipped `NoDedupeHook` default is invoked and always returns
//!    `None` (it never advertises a collapse).
//! 3. The contract is object-safe (`&dyn DedupeHook`) and `Send + Sync`,
//!    so it can be stored behind a trait object on a work node.

use std::collections::HashSet;
use std::sync::Arc;

use verter_scheduler::dag::{FileStageKey, WorkNodeIdentity};
use verter_scheduler::dedupe_hook::{DedupeHook, DedupeJoiner, NoDedupeHook};

fn file_stage(canonical: &str, generation: u64) -> WorkNodeIdentity {
    WorkNodeIdentity::FileStage {
        canonical: Arc::from(canonical),
        generation,
        stage: FileStageKey::Source,
    }
}

/// A real consumer-side hook: it owns an in-flight identity set and
/// returns a joiner when the probed identity is already in flight. This is
/// the shape the session `cache_runtime` implements.
struct InflightSetHook {
    inflight: HashSet<WorkNodeIdentity>,
}

impl DedupeHook for InflightSetHook {
    fn probe(&self, identity: &WorkNodeIdentity) -> Option<DedupeJoiner> {
        if self.inflight.contains(identity) {
            Some(DedupeJoiner::new())
        } else {
            None
        }
    }
}

/// The probe routes the identity through real caller state: a known
/// in-flight identity yields `Some`, an unknown one yields `None`.
#[test]
fn probe_collapses_known_inflight_identity() {
    let mut inflight = HashSet::new();
    inflight.insert(file_stage("/src/A.vue", 1));
    let hook = InflightSetHook { inflight };

    // Known in-flight identity → collapse (joiner returned).
    assert!(
        hook.probe(&file_stage("/src/A.vue", 1)).is_some(),
        "a known in-flight identity must collapse to a joiner",
    );

    // Different generation → distinct identity → no collapse.
    assert!(
        hook.probe(&file_stage("/src/A.vue", 2)).is_none(),
        "a different generation is a distinct identity — must NOT collapse",
    );

    // Different canonical → no collapse.
    assert!(
        hook.probe(&file_stage("/src/B.vue", 1)).is_none(),
        "a different canonical is a distinct identity — must NOT collapse",
    );
}

/// The shipped no-op default is genuinely invoked and always returns
/// `None`: the "no caller hook" path never fabricates a collapse.
#[test]
fn no_dedupe_hook_never_collapses() {
    let hook = NoDedupeHook;
    assert!(
        hook.probe(&file_stage("/src/A.vue", 1)).is_none(),
        "NoDedupeHook must never collapse — it advertises no behaviour it lacks",
    );
    // Probing a second, different identity must also yield None (the
    // default is a genuine no-op, not a one-shot).
    assert!(
        hook.probe(&file_stage("/src/C.vue", 9)).is_none(),
        "NoDedupeHook stays a genuine no-op across distinct probes",
    );
}

/// The contract is object-safe + `Send + Sync` so a `&dyn DedupeHook`
/// can be handed to `submit_request` / `try_submit_dag`. We exercise
/// it through a trait object to prove object-safety at the type level.
#[test]
fn hook_is_object_safe_and_thread_safe() {
    fn assert_send_sync<T: Send + Sync + ?Sized>() {}
    assert_send_sync::<dyn DedupeHook>();

    let hook: Arc<dyn DedupeHook> = Arc::new(NoDedupeHook);
    let probed = hook.probe(&file_stage("/src/D.vue", 0));
    assert!(probed.is_none(), "trait-object probe routes to the impl");
}
