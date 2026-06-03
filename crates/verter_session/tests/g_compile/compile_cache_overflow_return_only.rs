//! Discriminating test: the compile-tier producer refuses
//! `compile_slots.insert` when the finalised fact tracer reports
//! `Overflow`.
//!
//! Pre-fix (`finalise_signature_or_empty` collapses overflow → empty
//! signature → publish anyway): the warm-hit oracle's `is_empty()`
//! short-circuit validates the empty signature trivially, so the
//! compile slot stays "warm" indefinitely and downstream cross-file
//! edits are masked. The integration test pins the carrier invariant
//! `present in compile_slots ⇒ admitted cache entry`: on overflow, no
//! slot lands.
//!
//! Post-fix (typed `SignatureAdmission`): the cold-build path matches
//! on `SignatureAdmission::from_finalise(...)` and skips the
//! `compile_slots.insert` on the `NonCacheable` arm. The freshly
//! computed virtual file is still returned to the caller; only the
//! cache admission is refused.
//!
//! Discrimination: against the pre-change tree (commit `7b84404b` or
//! earlier), this test would FAIL because the slot is admitted with
//! an empty signature. Against the post-change tree, the assertion
//! holds.

use std::sync::Mutex;

use verter_session::for_tests::{
    compile_force_overflow_observations_for_tests, compile_scheduler_artifact_present_for_tests,
    compile_scheduler_last_known_good_artifact_present_for_tests,
};
use verter_session::{
    CompileProfile, FileKind, HostConfig, UpsertRequest, VerterHost, VirtualNodeKind, VirtualQuery,
};

// `COMPILE_TEST_FORCE_OVERFLOW_OBSERVATIONS` is a process-global atomic;
// concurrent tests in this file (and any other file in this crate) that
// arm the knob would race on its value. Serialize across this file so
// each test owns the global state for its duration.
static FORCE_OVERFLOW_MUTEX: Mutex<()> = Mutex::new(());

fn upsert_vue(host: &VerterHost, canonical: &str, source: &str) {
    let _ = host
        .upsert(UpsertRequest {
            canonical_id: Some(canonical.to_string()),
            input_id: canonical.to_string(),
            source: source.into(),
            file_kind: FileKind::VueSfc,
            aliases: Vec::new(),
        })
        .expect("upsert vue");
}

fn prime_compile(host: &VerterHost, canonical: &str) {
    let _ = host.get_virtual_file(VirtualQuery {
        raw_id: None,
        canonical_id: Some(canonical.to_string()),
        node_kind: Some(VirtualNodeKind::Script),
        compile_profile: CompileProfile::default(),
    });
}

/// Discriminator: when the compile cold-compute tracer overflows
/// (forced by `compile_force_overflow_observations_for_tests`), the
/// resulting `CompileSlot` MUST NOT be published into `compile_slots`.
///
/// Pre-fix `finalise_signature_or_empty` collapsed overflow into an
/// empty signature and the slot was published anyway with an empty
/// fact rail that trivially validated forever — a stale-cacheable
/// state. Post-fix the producer refuses the insert on overflow.
#[test]
fn compile_fact_signature_overflow_does_not_publish_compile_slot() {
    let _serial = FORCE_OVERFLOW_MUTEX
        .lock()
        .unwrap_or_else(|e| e.into_inner());
    let host = VerterHost::new_standalone(HostConfig::default());
    upsert_vue(
        &host,
        "/src/Comp.vue",
        "<script setup lang=\"ts\">\n\
         const n = 1;\n\
         </script>\n\
         <template><div>{{ n }}</div></template>\n",
    );

    // Force the compile-tier tracer past FACT_SIGNATURE_CAP (1024)
    // by injecting 1100 synthetic `FileWholeHash` observations into
    // the tracer scope. The finalised tracer returns `Overflow`.
    let _guard = compile_force_overflow_observations_for_tests(1100);

    prime_compile(&host, "/src/Comp.vue");

    let profile = CompileProfile::default();
    // The slot MUST NOT be present in `compile_slots`. The carrier
    // invariant is: `present in compile_slots ⇒ admitted cache entry`,
    // and an overflowed signature is non-cacheable.
    let slot_present = host
        .compile_slot_fact_dep_signature("/src/Comp.vue", &profile)
        .is_some();
    assert!(
        !slot_present,
        "carrier invariant: compile cold-build MUST refuse `compile_slots.insert` when \
         the finalised fact tracer reports `Overflow`. A published slot here means \
         the producer collapsed overflow into an empty signature and published \
         anyway — the pre-fix `finalise_signature_or_empty` collapse defect."
    );
    // The warm-hit predicate must also report false, since no slot
    // exists — discriminator half-check that the producer's refusal
    // wasn't masked by some downstream backfill path.
    assert!(
        !host.compile_slot_is_warm("/src/Comp.vue", &profile),
        "no slot means `compile_slot_is_warm` is false; an overflowed publish \
         that snuck through would leave `compile_slot_is_warm` true with an \
         empty signature that validates vacuously."
    );
}

/// Discriminator: a successful compile publishes a slot, then a
/// subsequent re-compile of the same `(canonical, profile)` that
/// overflows MUST remove the prior slot. The carrier invariant
/// strengthens from "present in compile_slots ⇒ admitted cache entry"
/// to "present in compile_slots ⇒ admitted cache entry for the
/// current version (any prior slot whose re-compute overflowed is
/// removed)".
///
/// Pre-fix the refusal-only branch skipped `compile_slots.insert` but
/// did NOT remove any prior slot, so a stale-cacheable entry from the
/// earlier successful compile satisfied warm-hit reads after the
/// re-compute overflowed. Post-fix the producer's `NonCacheable` arm
/// calls `compile_slots.remove(&profile_hash)` first.
#[test]
fn overflow_recompile_removes_prior_slot_for_same_key() {
    let _serial = FORCE_OVERFLOW_MUTEX
        .lock()
        .unwrap_or_else(|e| e.into_inner());
    let host = VerterHost::new_standalone(HostConfig::default());
    upsert_vue(
        &host,
        "/src/Comp.vue",
        "<script setup lang=\"ts\">\n\
         const n = 1;\n\
         </script>\n\
         <template><div>{{ n }}</div></template>\n",
    );

    let profile = CompileProfile::default();

    // Phase 1: prime a successful compile so a CompileSlot lands.
    prime_compile(&host, "/src/Comp.vue");
    assert!(
        host.compile_slot_fact_dep_signature("/src/Comp.vue", &profile)
            .is_some(),
        "fixture invariant: the first compile must publish a slot — \
         otherwise the discriminating prior-slot-removal assertion is \
         vacuous."
    );

    // Phase 2: force overflow on the next compile. The producer
    // observes 1100 synthetic facts → tracer finalises with `Overflow`.
    let _guard = compile_force_overflow_observations_for_tests(1100);

    // Re-prime: same `(canonical, profile)` cold-recomputes (the
    // upsert tick bump invalidates the warm-hit fast path, so the
    // cold path runs again).
    upsert_vue(
        &host,
        "/src/Comp.vue",
        "<script setup lang=\"ts\">\n\
         const n = 2;\n\
         </script>\n\
         <template><div>{{ n }}</div></template>\n",
    );
    prime_compile(&host, "/src/Comp.vue");

    // Post-fix: the prior slot MUST be removed. Pre-fix this returns
    // `Some(...)` (the stale-cacheable slot from phase 1 with the
    // first compile's `fact_dep_signature`).
    assert!(
        host.compile_slot_fact_dep_signature("/src/Comp.vue", &profile)
            .is_none(),
        "carrier invariant: a re-compute that overflows MUST remove \
         any prior slot for the same `(canonical, profile)`. Pre-fix \
         the refusal branch skipped the insert but did not remove the \
         prior slot, so stale data survived an overflowed re-compute."
    );
}

/// Discriminator: an overflowed compile MUST NOT commit a scheduler
/// artifact snapshot. The artifact substrate (scheduler-backed
/// `try_get_artifact` and pending Artifact requests) is the second
/// observable warm-hit substrate — refusing only the `compile_slots`
/// insert leaks the overflowed result via the scheduler artifact
/// path.
///
/// Pre-fix the `scheduler.commit_artifact(...)` block ran
/// unconditionally for both `Cacheable` and `NonCacheable` admission,
/// so `try_get_artifact(canonical, profile_hash)` returned
/// `Some(snapshot)` after an overflowed compile. Post-fix the artifact
/// commit is gated on `Cacheable` admission.
#[test]
fn overflow_skips_scheduler_artifact_commit() {
    let _serial = FORCE_OVERFLOW_MUTEX
        .lock()
        .unwrap_or_else(|e| e.into_inner());
    let host = VerterHost::new_standalone(HostConfig::default());
    upsert_vue(
        &host,
        "/src/Comp.vue",
        "<script setup lang=\"ts\">\n\
         const n = 1;\n\
         </script>\n\
         <template><div>{{ n }}</div></template>\n",
    );

    let profile = CompileProfile::default();

    // Force the tracer past FACT_SIGNATURE_CAP. The finalised tracer
    // returns `Overflow`.
    let _guard = compile_force_overflow_observations_for_tests(1100);
    prime_compile(&host, "/src/Comp.vue");

    // The compile_slots refusal already pins the per-slot invariant.
    // This assertion pins the COMPANION invariant at the scheduler
    // artifact layer.
    assert!(
        !compile_scheduler_artifact_present_for_tests(&host, "/src/Comp.vue", &profile),
        "carrier invariant: an overflowed compile MUST NOT commit a \
         scheduler artifact snapshot. The artifact substrate is the \
         second warm-hit observation path; refusing only the \
         `compile_slots.insert` (pre-fix) left `try_get_artifact` \
         returning the overflowed result. Post-fix the artifact \
         commit is gated on `Cacheable` admission."
    );
}

/// Discriminator: a successful compile commits an artifact, then a
/// subsequent re-compute that refuses cache admission (overflowed
/// fact tracer) MUST evict the prior artifact snapshot. Symmetric to
/// the `compile_slots.remove(...)` invariant at the per-slot
/// substrate, this pins the eviction at the SCHEDULER artifact
/// substrate.
///
/// `last_known_good_artifact` reads from the artifact map without
/// the generation-coherence filter that `try_get_artifact` applies,
/// so a stale artifact left in the map after a generation bump is
/// invisible to `try_get_artifact` but visible here. The
/// refusal arm calls
/// `scheduler.remove_artifact_if_not_newer_than(canonical,
/// profile_hash, compile_start_generation)`; without this call the
/// prior successful compile's artifact would survive in
/// `last_known_good_artifact` indefinitely. The generation gate
/// preserves a newer artifact when a slow refused compile races
/// against a fast successful compile at a later generation —
/// exercised by the dedicated scheduler-level test
/// `remove_artifact_if_not_newer_than_preserves_newer_generation_artifact`.
#[test]
fn overflow_recompile_evicts_prior_scheduler_artifact() {
    let _serial = FORCE_OVERFLOW_MUTEX
        .lock()
        .unwrap_or_else(|e| e.into_inner());
    let host = VerterHost::new_standalone(HostConfig::default());
    upsert_vue(
        &host,
        "/src/Comp.vue",
        "<script setup lang=\"ts\">\n\
         const n = 1;\n\
         </script>\n\
         <template><div>{{ n }}</div></template>\n",
    );

    let profile = CompileProfile::default();

    // Successful compile lands the artifact in the scheduler.
    prime_compile(&host, "/src/Comp.vue");
    assert!(
        compile_scheduler_last_known_good_artifact_present_for_tests(
            &host,
            "/src/Comp.vue",
            &profile,
        ),
        "fixture invariant: the first compile must commit a scheduler \
         artifact — otherwise the prior-artifact-eviction assertion is \
         vacuous."
    );

    // Re-upsert + forced overflow drives the producer's refusal arm
    // after a prior successful artifact was committed.
    let _guard = compile_force_overflow_observations_for_tests(1100);
    upsert_vue(
        &host,
        "/src/Comp.vue",
        "<script setup lang=\"ts\">\n\
         const n = 2;\n\
         </script>\n\
         <template><div>{{ n }}</div></template>\n",
    );
    prime_compile(&host, "/src/Comp.vue");

    // The prior artifact MUST be evicted from BOTH `try_get_artifact`
    // (visible via the generation coherence path) AND
    // `last_known_good_artifact` (visible without that filter). A
    // stale artifact in `last_known_good_artifact` is the
    // discriminator: a refusal arm that does NOT route through
    // `remove_artifact_if_not_newer_than(...)` leaves the prior
    // artifact in the scheduler indefinitely; the symmetric call
    // evicts it and both probes return `None`.
    assert!(
        !compile_scheduler_artifact_present_for_tests(&host, "/src/Comp.vue", &profile),
        "carrier invariant: an overflowed re-compile MUST NOT leave a \
         stale current-generation artifact visible via \
         `try_get_artifact`."
    );
    assert!(
        !compile_scheduler_last_known_good_artifact_present_for_tests(
            &host,
            "/src/Comp.vue",
            &profile,
        ),
        "carrier invariant: an overflowed re-compile MUST evict any \
         prior artifact from the scheduler's artifact map (visible via \
         `last_known_good_artifact`). A refusal arm that removes only \
         `compile_slots` leaves the prior artifact in the scheduler \
         indefinitely; the symmetric \
         `scheduler.remove_artifact_if_not_newer_than(...)` call \
         evicts it."
    );
}
