//! The Vue macro producer's fact footprint must reach the CONSUMING thread.
//!
//! `produce_vue_macro_codegen_with_ctx` runs its build closure through
//! `Scheduler::execute_scoped_cache_node`, which dispatches onto a scheduler
//! CPU-pool worker. The producer installs its own fact tracer inside that
//! closure, and the tracer stack is thread-local with no cross-thread bridge —
//! so nothing the producer observes is visible to the enclosing `Session`
//! compile's tracer on the submitting thread.
//!
//! Two consequences, both live production defects before the footprint carrier
//! existed:
//!
//! 1. Every fact rooted at a TRANSITIVELY reached file (anything past the SFC's
//!    direct imports) was missing from the compile slot's `ReadSetSignature`,
//!    so editing such a file left the warm slot validating and the bundler
//!    emitted stale runtime prop validators.
//! 2. A non-cacheable read consumed inside macro resolution never reached the
//!    compile's `non_cacheable_read_observed()` gate, so the entry was admitted
//!    anyway.
//!
//! `VueMacroCodegenOutput.fact_footprint` carries the producer's finalised
//! observation set plus its refusal across the worker boundary, and the call
//! site replays both on the consuming thread — for the leader arm (which ran
//! the closure elsewhere) and the follower arm (which ran no closure at all).
//!
//! | Test | Discriminating assertion |
//! | ---- | ------------------------ |
//! | `nested_macro_type_dep_edit_invalidates_warm_runtime_props` | Editing a TRANSITIVELY reached file re-emits the runtime validator; the stale constructor is absent. |
//! | `direct_macro_type_dep_edit_invalidates_warm_runtime_props` | Control: the direct-dep edit path already worked, so the fixture/harness itself discriminates. |
//! | `compile_slot_signature_roots_the_transitively_reached_file` | The published compile signature names the transitive canonical, not just the direct ones. |
//! | `both_flight_arms_replay_the_producer_footprint` | The follower — which returns from the flight terminal having run NO closure — records the producer's facts in its own tracer, as does the leader. |
//! | `warm_producer_run_still_roots_the_transitively_reached_file` | A producer re-run over memos an earlier run of the same owner populated still seals the transitive footprint. |
//! | `second_owner_over_fully_warm_memos_roots_the_transitive_file` | A producer that NEVER computed the shared memos itself — pure warm reads — still seals them. |
//! | `factless_footprints_refuse_rooting_instead_of_replaying_nothing` | All FOUR factless footprints (`Overflowed`, `MutationUnstable`, `Unobserved`, `RootedNonCacheable([])`) taint instead of replaying an empty set. |
//! | `cancelled_producer_handoff_refuses_the_consumer_rooting` | The `Unobserved` arm on the real path: a cancelled handoff refuses the CONSUMER's tracer, with an uncancelled control that does not. |
//! | `non_cacheable_footprint_replays_facts_and_the_refusal` | The `NonCacheable` arm bubbles its facts AND its refusal. |
//! | `rooted_footprint_replays_facts_without_refusing` | A publishable footprint replays its facts and leaves the consumer able to root. |
//!
//! ## Mutation recipes
//!
//! Each recipe is a single edit against the landed tree, verified to apply
//! uniquely and to be caught. Counts are out of the 10 tests above. A recipe
//! that applies but leaves the suite green means the suite has regressed —
//! `perl`/`sed` exit 0 on a non-match, so confirm the marker is present exactly
//! once before trusting a green run.
//!
//! All five live in `MacroFactFootprint` (`typeinfo/vue_macro_codegen.rs`),
//! four of them inside `replay()`:
//!
//! - **R1 — drop the publishable replay.** `Self::Rooted(facts) =>
//!   observe_fan_out_borrowed(facts)` becomes a no-op. This is the original bug.
//!   Caught: 6 fail.
//! - **R2 — overflow replays nothing.** Split the factless arm so
//!   `Self::Overflowed => {}`. Caught: 1 fails, naming `["Overflowed"]`.
//! - **R3 — empty non-cacheable replays nothing.** Insert a guarded arm
//!   `Self::RootedNonCacheable(facts) if facts.is_empty() => {}` above the real
//!   one. Caught: 1 fails, naming `["RootedNonCacheable(empty)"]`. Reachable in
//!   production: a refusal raised before any observation finalises exactly here.
//! - **R4 — the non-computed handoff replays nothing.** Split the factless arm
//!   so `Self::Unobserved => {}`. Caught: 2 fail — the unit test naming
//!   `["Unobserved"]` and the real-path cancelled handoff.
//! - **R5 — replay on the PRODUCING thread.** Delete
//!   `output.fact_footprint.replay()` from
//!   `produce_vue_macro_codegen_with_ctx` and call `fact_footprint.replay()`
//!   inside `compute_vue_macro_codegen_output` instead. Every arm is still
//!   matched and the carrier is still built correctly — it is the original bug
//!   wearing the fix's clothes, and the shape a well-intentioned refactor would
//!   produce. Caught: 6 fail.

use std::sync::Arc;

use crate::resolver_core::{FactReadSetFinalise, FactVersionRef};
use crate::typeinfo::vue_macro_codegen::{MacroFactFootprint, VueMacroCodegenDemand};
use crate::types::{CompileProfile, HostConfig, UpsertRequest, VirtualNodeKind, VirtualQuery};
use crate::VerterHost;

const INNER: &str = "/proj/inner.ts";
const PROPS: &str = "/proj/props.ts";
const OWNER: &str = "/proj/App.vue";

/// `defineProps<Props>()` where `Props` lives in a DIRECT import and its member
/// type `Inner` lives one hop further out. `inner.ts` is reachable only through
/// the macro producer's own cross-file type traversal, so it is exactly the
/// class of dependency the worker-local tracer used to swallow.
const OWNER_SOURCE: &str = "<script setup lang=\"ts\">\nimport type { Props } from './props';\ndefineProps<Props>()\n</script>\n<template><div /></template>";

const PROPS_SOURCE: &str =
    "import type { Inner } from './inner';\nexport interface Props { a: Inner }\n";

fn make_host() -> Arc<VerterHost> {
    let workspace = Arc::new(verter_workspace::MemoryWorkspace::new(
        verter_workspace::MemoryOptions::default(),
    ));
    Arc::new(VerterHost::new(HostConfig::default(), workspace))
}

fn upsert(host: &VerterHost, canonical_id: &str, source: &str) {
    let _ = host
        .upsert(UpsertRequest {
            canonical_id: Some(canonical_id.to_string()),
            input_id: canonical_id.to_string(),
            source: Arc::from(source),
            file_language: verter_language::LanguageRegistry::global()
                .classify_static(canonical_id)
                .static_resolution(),
            aliases: Vec::new(),
        })
        .expect("upsert must succeed");
}

/// Drive the producer through its real entry point for the seeded owner.
fn produce_runtime(host: &VerterHost) -> crate::typeinfo::vue_macro_codegen::VueMacroCodegenOutput {
    crate::resolver_core::with_bare_host_ctx_for_test(host, |ctx| {
        host.produce_vue_macro_codegen_with_ctx(ctx, OWNER, VueMacroCodegenDemand::Runtime)
    })
}

fn seed(host: &VerterHost, inner: &str, props: &str) {
    upsert(host, INNER, inner);
    upsert(host, PROPS, props);
    upsert(host, OWNER, OWNER_SOURCE);
}

/// The bundler-facing production read: the compiled `Main` module carrying the
/// runtime `props` option object.
fn compile_main(host: &VerterHost) -> String {
    host.get_virtual_file(VirtualQuery {
        raw_id: None,
        canonical_id: Some(OWNER.to_string()),
        node_kind: Some(VirtualNodeKind::Main),
        compile_profile: CompileProfile::default(),
    })
    .expect("compile must serve")
    .code
    .to_string()
}

/// The same tree compiled by a FRESH host — the cold truth the warm read must
/// agree with.
fn cold_truth(inner: &str, props: &str) -> String {
    let host = make_host();
    seed(&host, inner, props);
    compile_main(&host)
}

const INNER_STRING: &str = "export type Inner = string;\n";
const INNER_NUMBER: &str = "export type Inner = number;\n";

/// Editing a file reached only TRANSITIVELY through the macro type graph must
/// invalidate the warm `Session` compile slot.
///
/// Discrimination: before the producer's footprint crossed the worker boundary,
/// no fact in the compile slot's signature was rooted at `inner.ts`, so the warm
/// slot validated and the bundler kept emitting `type: String` after `Inner`
/// became `number`.
#[test]
fn nested_macro_type_dep_edit_invalidates_warm_runtime_props() {
    let host = make_host();
    seed(&host, INNER_STRING, PROPS_SOURCE);

    let before = compile_main(&host);
    assert!(
        before.contains("a: { type: String, required: true }"),
        "baseline must classify `a` from the transitive `Inner = string`: {before}"
    );

    upsert(&host, INNER, INNER_NUMBER);
    let after = compile_main(&host);

    assert!(
        after.contains("a: { type: Number, required: true }"),
        "editing the transitively reached `inner.ts` must re-emit the runtime \
         validator: {after}"
    );
    assert!(
        !after.contains("type: String"),
        "the superseded constructor must be gone, not merely joined: {after}"
    );
    assert_eq!(
        after,
        cold_truth(INNER_NUMBER, PROPS_SOURCE),
        "the warm read must be byte-identical to the same tree compiled cold"
    );
}

/// Control: the DIRECT-dependency edit path was already correct. If this ever
/// fails, the fixture is not exercising cross-file macro resolution at all and
/// the nested test above proves nothing.
#[test]
fn direct_macro_type_dep_edit_invalidates_warm_runtime_props() {
    let host = make_host();
    seed(&host, INNER_STRING, PROPS_SOURCE);

    let before = compile_main(&host);
    assert!(
        before.contains("a: { type: String, required: true }"),
        "baseline: {before}"
    );

    const PROPS_INLINE_NUMBER: &str =
        "import type { Inner } from './inner';\nexport interface Props { a: number }\n";
    upsert(&host, PROPS, PROPS_INLINE_NUMBER);
    let after = compile_main(&host);

    assert!(
        after.contains("a: { type: Number, required: true }"),
        "editing the direct dep must re-emit the runtime validator: {after}"
    );
    assert!(
        !after.contains("type: String"),
        "the superseded constructor must be gone: {after}"
    );
}

/// The mechanism behind the two behavioural tests: the published compile slot's
/// fact signature must NAME the transitively reached canonical. A signature that
/// only roots the owner and its direct imports is precisely the stale-serve bug.
#[test]
fn compile_slot_signature_roots_the_transitively_reached_file() {
    let host = make_host();
    seed(&host, INNER_STRING, PROPS_SOURCE);
    let _ = compile_main(&host);

    let signature = host
        .compile_slot_fact_dep_signature(OWNER, &CompileProfile::default())
        .expect("the Session compile must publish a fact-validated slot");

    let rooted: Vec<&str> = signature
        .facts
        .iter()
        .filter_map(FactVersionRef::canonical_id)
        .collect();
    assert!(
        rooted.contains(&INNER),
        "the compile signature must root the transitively reached `inner.ts`; \
         rooted canonicals were {rooted:?}"
    );
    assert!(
        rooted.contains(&PROPS),
        "sanity: the direct dep must also be rooted; rooted canonicals were {rooted:?}"
    );
}

/// Both flight arms must replay the producer's footprint.
///
/// The leader runs the build closure on a scheduler CPU-pool worker, where its
/// tracer sits on a different thread's stack. The follower returns straight from
/// the flight terminal having run NO closure at all, so it observes nothing
/// unless the footprint is replayed for it too. Each thread here brackets its
/// own tracer and finalises it, and BOTH must contain a fact rooted at the
/// transitively reached `inner.ts`.
#[cfg(not(target_arch = "wasm32"))]
#[test]
fn both_flight_arms_replay_the_producer_footprint() {
    let host = make_host();
    seed(&host, INNER_STRING, PROPS_SOURCE);

    let rendezvous = Arc::new((std::sync::Barrier::new(2), std::sync::Barrier::new(2)));
    *host.test_force.vue_macro_codegen_build_rendezvous.lock() = Some(Arc::clone(&rendezvous));
    let submissions_before = host
        .scheduler()
        .counters()
        .submit_count
        .load(std::sync::atomic::Ordering::Acquire);

    /// Produce the bundle inside a fresh fact tracer and return the sealed
    /// observation set. `FactReadSet` is `!Send`, so the seal happens on the
    /// producing thread and only the `Arc`-backed finalise crosses back.
    fn traced_produce(host: &VerterHost) -> FactReadSetFinalise {
        let ((), read_set) =
            host.with_fact_tracer(verter_workspace::AggregateBasisSeed::Unvouched, || {
                crate::resolver_core::with_bare_host_ctx_for_test(host, |ctx| {
                    let _ = host.produce_vue_macro_codegen_with_ctx(
                        ctx,
                        OWNER,
                        VueMacroCodegenDemand::Runtime,
                    );
                });
            });
        read_set.finalise()
    }

    let leader = {
        let host = Arc::clone(&host);
        std::thread::spawn(move || traced_produce(host.as_ref()))
    };
    rendezvous.0.wait();

    let follower = {
        let host = Arc::clone(&host);
        std::thread::spawn(move || traced_produce(host.as_ref()))
    };

    // Hold the leader inside its build closure until the follower has joined
    // the same scoped flight, so the follower genuinely takes the terminal arm.
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
    while host
        .scheduler()
        .counters()
        .submit_count
        .load(std::sync::atomic::Ordering::Acquire)
        < submissions_before + 2
        && std::time::Instant::now() < deadline
    {
        std::thread::yield_now();
    }
    assert_eq!(
        host.scheduler()
            .counters()
            .submit_count
            .load(std::sync::atomic::Ordering::Acquire),
        submissions_before + 2,
        "the follower must join the scoped flight before the leader is released"
    );
    rendezvous.1.wait();

    let leader = leader.join().expect("leader thread");
    let follower = follower.join().expect("follower thread");
    *host.test_force.vue_macro_codegen_build_rendezvous.lock() = None;

    // Report BOTH arms from one assertion rather than short-circuiting on the
    // first: a sequential assert would let a regression that loses the replay
    // entirely be attributed to the leader alone, leaving the follower arm
    // untested by the very failure that breaks it.
    let arms = [("leader", &leader), ("follower", &follower)];
    let mut rooted_by_arm = Vec::new();
    for (arm, finalise) in arms {
        let FactReadSetFinalise::Ok(facts) = finalise else {
            panic!("{arm} tracer must seal a bounded, publishable set: {finalise:?}");
        };
        let rooted: Vec<&str> = facts
            .iter()
            .filter_map(FactVersionRef::canonical_id)
            .collect();
        rooted_by_arm.push((arm, rooted));
    }
    let lost: Vec<&str> = rooted_by_arm
        .iter()
        .filter(|(_, rooted)| !rooted.contains(&INNER))
        .map(|(arm, _)| *arm)
        .collect();
    assert!(
        lost.is_empty(),
        "these flight arms lost the producer's transitive footprint: {lost:?} \
         (per-arm rooted canonicals: {rooted_by_arm:?})"
    );
}

/// EVERY footprint that carries zero facts must refuse rooting.
///
/// Replaying a factless footprint as an empty observation set silently
/// reproduces the stale-serve bug — the consumer roots on nothing and validates
/// forever. Four distinct footprints reach `replay()` with no facts, and all
/// four must taint:
///
/// - `Overflowed` — finalisation exceeded the cap, so the facts were dropped.
/// - `MutationUnstable` — the producer's aggregate basis moved mid-compute.
/// - `Unobserved` — no traced compute ran at all.
/// - `RootedNonCacheable([])` — a refusal raised before anything was observed.
///   This one is easy to miss because the arm *looks* covered by the populated
///   `NonCacheable` test; guarding `Overflowed` alone leaves it open.
///
/// The whole set is asserted from ONE verdict so a regression that drops the
/// taint names every arm it broke rather than short-circuiting on the first.
#[test]
fn factless_footprints_refuse_rooting_instead_of_replaying_nothing() {
    let host = make_host();
    let empty_non_cacheable =
        MacroFactFootprint::from_finalise(FactReadSetFinalise::NonCacheable(Arc::from(Vec::new())));
    let overflowed = MacroFactFootprint::from_finalise(FactReadSetFinalise::Overflow);
    let mutation_unstable =
        MacroFactFootprint::from_finalise(FactReadSetFinalise::MutationUnstable);

    assert!(
        matches!(overflowed.1, MacroFactFootprint::Overflowed),
        "overflow must keep its own typed arm: {:?}",
        overflowed.1
    );
    assert!(
        matches!(mutation_unstable.1, MacroFactFootprint::MutationUnstable),
        "mutation instability must keep its own typed arm: {:?}",
        mutation_unstable.1
    );
    assert!(
        matches!(
            empty_non_cacheable.1,
            MacroFactFootprint::RootedNonCacheable(_)
        ),
        "an empty non-cacheable set keeps the non-cacheable arm: {:?}",
        empty_non_cacheable.1
    );

    let cases = [
        ("Overflowed", overflowed.1, overflowed.0),
        ("MutationUnstable", mutation_unstable.1, mutation_unstable.0),
        (
            "RootedNonCacheable(empty)",
            empty_non_cacheable.1,
            empty_non_cacheable.0,
        ),
        ("Unobserved", MacroFactFootprint::Unobserved, Vec::new()),
    ];

    let mut rooted_without_refusing = Vec::new();
    for (arm, footprint, canonicals) in cases {
        assert!(
            canonicals.is_empty(),
            "the {arm} arm exposes no transitive canonicals: {canonicals:?}"
        );
        let ((), read_set) = host
            .with_fact_tracer(verter_workspace::AggregateBasisSeed::Unvouched, || {
                footprint.replay()
            });
        let refused = read_set.non_cacheable_read_observed();
        let sealed_empty = matches!(
            read_set.finalise(),
            FactReadSetFinalise::NonCacheable(facts) if facts.is_empty()
        );
        if !refused || !sealed_empty {
            rooted_without_refusing.push(arm);
        }
    }
    assert!(
        rooted_without_refusing.is_empty(),
        "these factless footprints let the consumer root on nothing: \
         {rooted_without_refusing:?}"
    );
}

/// The `Unobserved` arm on the REAL path, through the consumer's own tracer.
///
/// A pre-cancelled request short-circuits `execute_scoped_cache_node` before any
/// closure runs, so `produce_vue_macro_codegen_with_ctx` takes its
/// `Err(ScopedCacheNodeError::Cancelled)` arm and hands back the terminal
/// handoff. That handoff observed nothing, so the enclosing compute must be
/// refused the right to root — otherwise a compile whose macro bundle came from
/// a cancelled producer publishes a slot rooted only on its own direct reads.
///
/// This asserts on the CONSUMER's tracer, not on the output's enum tag: an
/// accessor that inspects the tag is satisfied no matter what `replay()` does.
#[test]
fn cancelled_producer_handoff_refuses_the_consumer_rooting() {
    let host = make_host();
    seed(&host, INNER_STRING, PROPS_SOURCE);

    let cancelled =
        crate::request_context::RequestContext::new(9101, Arc::from(OWNER), false, None);
    cancelled.cancel();

    let ((output, uncancelled_refused), read_set) =
        host.with_fact_tracer(verter_workspace::AggregateBasisSeed::Unvouched, || {
            // Control FIRST, inside the same tracer: an ordinary complete handoff
            // must NOT refuse, so the refusal asserted below is attributable to the
            // cancelled handoff rather than to anything the fixture reads.
            let uncancelled_refused = {
                let ((), probe) =
                    host.with_fact_tracer(verter_workspace::AggregateBasisSeed::Unvouched, || {
                        let _ = produce_runtime(&host);
                    });
                probe.non_cacheable_read_observed()
            };
            let _guard = crate::request_context::RequestContextGuard::install(cancelled);
            (produce_runtime(&host), uncancelled_refused)
        });

    assert!(
        !uncancelled_refused,
        "an ordinary complete producer handoff must not refuse its consumer"
    );
    assert!(
        output
            .completeness
            .reasons()
            .contains(crate::semantic_query::PartialReasonSet::CANCELLED),
        "the fixture must actually drive a cancelled terminal handoff, \
         not a complete one: {:?}",
        output.completeness
    );
    assert!(
        read_set.non_cacheable_read_observed(),
        "a producer handoff that observed nothing must refuse the consumer's \
         rooting"
    );
}

/// The `NonCacheable` arm keeps its observation set — it still bubbles into the
/// enclosing tracer — but must ALSO carry the refusal across, otherwise a fenced
/// serve or lease miss inside macro resolution never reaches the compile's
/// admission gate.
#[test]
fn non_cacheable_footprint_replays_facts_and_the_refusal() {
    let host = make_host();
    let fact = FactVersionRef::FileWholeHash {
        canonical_id: INNER.to_string(),
        hash: [3u8; 16],
    };
    let (canonicals, footprint) =
        MacroFactFootprint::from_finalise(FactReadSetFinalise::NonCacheable(Arc::from(vec![fact])));
    assert_eq!(canonicals, vec![INNER.to_string()]);
    assert!(
        matches!(footprint, MacroFactFootprint::RootedNonCacheable(_)),
        "a non-cacheable set keeps its own typed arm: {footprint:?}"
    );

    let ((), read_set) = host
        .with_fact_tracer(verter_workspace::AggregateBasisSeed::Unvouched, || {
            footprint.replay()
        });
    assert!(
        read_set.non_cacheable_read_observed(),
        "the producer's refusal must reach the consuming thread"
    );
    let FactReadSetFinalise::NonCacheable(facts) = read_set.finalise() else {
        panic!("a replayed refusal must seal as NonCacheable");
    };
    assert_eq!(
        facts
            .iter()
            .filter_map(FactVersionRef::canonical_id)
            .collect::<Vec<_>>(),
        vec![INNER],
        "the facts must bubble even though they cannot authorize admission"
    );
}

/// A publishable set replays its facts and leaves the consumer able to root.
#[test]
fn rooted_footprint_replays_facts_without_refusing() {
    let host = make_host();
    let fact = FactVersionRef::FileWholeHash {
        canonical_id: PROPS.to_string(),
        hash: [5u8; 16],
    };
    let (canonicals, footprint) =
        MacroFactFootprint::from_finalise(FactReadSetFinalise::Ok(Arc::from(vec![fact])));
    assert_eq!(canonicals, vec![PROPS.to_string()]);
    assert!(matches!(footprint, MacroFactFootprint::Rooted(_)));

    let ((), read_set) = host
        .with_fact_tracer(verter_workspace::AggregateBasisSeed::Unvouched, || {
            footprint.replay()
        });
    assert!(
        !read_set.non_cacheable_read_observed(),
        "a publishable producer footprint must not taint its consumer"
    );
    let FactReadSetFinalise::Ok(facts) = read_set.finalise() else {
        panic!("a publishable replay must seal as Ok");
    };
    assert_eq!(
        facts
            .iter()
            .filter_map(FactVersionRef::canonical_id)
            .collect::<Vec<_>>(),
        vec![PROPS]
    );
}

/// XT-3, first shape: the footprint survives a producer re-run whose cross-file
/// memos were populated by an EARLIER run of the same owner.
///
/// The replay carries whatever the producer's tracer sealed. On any run past the
/// first, the producer's inner semantic queries hit warm memos, and a warm hit
/// that failed to bubble its recorded signature into the enclosing tracer would
/// leave the compile signature short of the transitive canonicals — the same
/// stale serve, one edit later. The producer re-runs here because the SFC's own
/// template changed (the scoped flight is removed at terminal state, so every
/// call rebuilds), while the `props.ts` -> `inner.ts` chain is untouched.
#[test]
fn warm_producer_run_still_roots_the_transitively_reached_file() {
    let host = make_host();
    seed(&host, INNER_STRING, PROPS_SOURCE);
    let _ = compile_main(&host);

    const OWNER_TOUCHED: &str = "<script setup lang=\"ts\">\nimport type { Props } from './props';\ndefineProps<Props>()\n</script>\n<template><div /><span /></template>";
    upsert(&host, OWNER, OWNER_TOUCHED);
    let warm = compile_main(&host);
    assert!(
        warm.contains("a: { type: String, required: true }"),
        "the re-compile must still classify `a`: {warm}"
    );

    let signature = host
        .compile_slot_fact_dep_signature(OWNER, &CompileProfile::default())
        .expect("the re-compile must publish a fact-validated slot");
    let rooted: Vec<&str> = signature
        .facts
        .iter()
        .filter_map(FactVersionRef::canonical_id)
        .collect();
    assert!(
        rooted.contains(&INNER),
        "a producer run over WARM inner memos must still root the transitively \
         reached `inner.ts`; rooted canonicals were {rooted:?}"
    );

    upsert(&host, INNER, INNER_NUMBER);
    let after = compile_main(&host);
    assert!(
        after.contains("a: { type: Number, required: true }"),
        "editing the transitive dep after a warm producer run must re-emit the \
         runtime validator: {after}"
    );
    assert!(!after.contains("type: String"), "{after}");
}

/// XT-3, second and stronger shape: a producer that NEVER computed the shared
/// cross-file memos itself.
///
/// `/proj/Second.vue` consumes the same `Props` -> `Inner` chain, but its
/// producer runs after `/proj/App.vue` has already populated every memo along
/// that chain. Its own tracer therefore observes those facts only if warm memo
/// hits bubble their recorded signatures — a pure warm-read path with no cold
/// compute of its own to fall back on. If they did not bubble, `Second.vue`'s
/// compile signature would carry only its direct import and the edit below would
/// serve stale.
#[test]
fn second_owner_over_fully_warm_memos_roots_the_transitive_file() {
    const SECOND: &str = "/proj/Second.vue";
    let host = make_host();
    seed(&host, INNER_STRING, PROPS_SOURCE);
    upsert(&host, SECOND, OWNER_SOURCE);

    // Warm the shared `props.ts` -> `inner.ts` memos through the first owner.
    let _ = compile_main(&host);

    let second = |host: &VerterHost| -> String {
        host.get_virtual_file(VirtualQuery {
            raw_id: None,
            canonical_id: Some(SECOND.to_string()),
            node_kind: Some(VirtualNodeKind::Main),
            compile_profile: CompileProfile::default(),
        })
        .expect("compile must serve")
        .code
        .to_string()
    };

    let before = second(&host);
    assert!(
        before.contains("a: { type: String, required: true }"),
        "baseline: {before}"
    );

    let signature = host
        .compile_slot_fact_dep_signature(SECOND, &CompileProfile::default())
        .expect("the second owner must publish a fact-validated slot");
    let rooted: Vec<&str> = signature
        .facts
        .iter()
        .filter_map(FactVersionRef::canonical_id)
        .collect();
    assert!(
        rooted.contains(&INNER),
        "a producer served entirely from warm memos must still root the \
         transitively reached `inner.ts`; rooted canonicals were {rooted:?}"
    );

    upsert(&host, INNER, INNER_NUMBER);
    let after = second(&host);
    assert!(
        after.contains("a: { type: Number, required: true }"),
        "the warm-memo-served owner must re-emit its runtime validator: {after}"
    );
    assert!(!after.contains("type: String"), "{after}");
}
