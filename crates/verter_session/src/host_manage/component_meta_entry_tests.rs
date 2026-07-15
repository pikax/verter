//! Discriminating tests for `strip_owner_route_fact` — the narrow
//! owner-Route fact filter applied at the `ComponentMetaResultDb`
//! cache-admission site.
//!
//! ## What is under test
//!
//! An earlier change made the component-meta cache signature
//! tracer-owned and deleted the route-fact filter, on the premise that the
//! finalised tracer read set never observes the owner's own
//! `DerivedFactHash{Route}` fact. That premise is false: the cold
//! compute's macro-root route walk observes the owner's Route fact
//! whenever the owner is a route participant (see
//! `tests/cases/g_component/component_meta_route_facts_flow_into_signature.rs`, which
//! characterises that the owner's Route fact reaches the tracer).
//!
//! The owner's own Route fact is non-round-tripping on warm
//! validation: a missing `(owner, Route)` entry on a later live
//! `HostStoreView` rejects the published signature, so a repeated
//! IDENTICAL `get_component_meta` query can miss the final-result
//! cache with no edit — a steady-state warm-cache miss / perf
//! regression. The owner's own export route is not a dependency
//! of the owner's own component-meta result in the first place (the
//! owner's `FileWholeHash` fact already covers owner-content edits),
//! so the fix drops the fact unconditionally.
//!
//! `strip_owner_route_fact` drops exactly the owner's own
//! `DerivedFactHash{Route}` fact before cache admission. Cross-file
//! route facts (Route facts for OTHER canonicals the cold compute
//! walked) round-trip correctly and stay.
//!
//! ## Discrimination
//!
//! `repeated_query_after_route_only_indexed_read_is_warm_hit`
//! materialises the owner's `IndexedReady` via a genuine route-only
//! read BEFORE `get_component_meta` runs (the precondition: an owner
//! already has an `IndexedReady` from an earlier route-only read),
//! then runs `get_component_meta` and asserts:
//!
//! 1. **Discriminator.** The published `ComponentMetaResultEntry`
//!    signature does NOT contain `DerivedFactHash { canonical_id ==
//!    owner, kind: Route }`. Without the owner-Route filter this fact IS
//!    present (the publish site admits the raw tracer set verbatim) and
//!    this assertion FAILS; `strip_owner_route_fact` removes it.
//! 2. **Narrowness guard.** The published signature DOES still contain
//!    the cross-file route dep's `DerivedFactHash{Route}` fact —
//!    proving the filter is NARROW, not the broad route-fact removal.
//! 3. **Behavioural non-regression.** A repeated IDENTICAL query is a
//!    warm-cache HIT with no new miss. The fix must not break warm
//!    reuse for the route-only-read-first scenario.

use std::sync::atomic::Ordering::Relaxed;
use std::sync::Arc;

use crate::resolver_core::{DerivedFactKind, FactVersionRef};
use crate::types::{FileLanguage, HostConfig, UpsertRequest};
use crate::VerterHost;

/// `/src/types.ts` — a cross-file dep imported by the owner. Its
/// export route is a genuine cross-file route dependency of the
/// owner's component-meta result.
const TYPES_TS: &str = "export interface RProps { a: number; b: string; }\n";

/// Owner SFC: `defineProps<RProps>()` over the imported `RProps`.
/// Resolving the macro root walks the named-type export route; the
/// route walk observes `DerivedFactHash{Route}` participant facts —
/// including the owner's own, as the importer is a route participant.
const OWNER_VUE: &str = "<script setup lang=\"ts\">\n\
     import type { RProps } from './types';\n\
     defineProps<RProps>();\n\
     </script>\n\
     <template><div /></template>\n";

fn build_host() -> Arc<VerterHost> {
    Arc::new(VerterHost::new_standalone(HostConfig::default()))
}

fn upsert(host: &VerterHost, id: &str, src: &str, kind: FileLanguage) {
    let _ = host
        .upsert(UpsertRequest {
            canonical_id: None,
            input_id: id.to_string(),
            source: Arc::from(src),
            file_language: kind,
            aliases: Vec::new(),
        })
        .expect("upsert");
}

/// Read the published `ComponentMetaResultEntry` signature `facts`
/// rail for `owner` — composed exactly as
/// `publish_component_meta_cache_entry` composes the key (owner
/// canonical + current `IndexedReady` whole-hash + default
/// `ComponentMetaOptions` fingerprint).
fn published_facts(host: &VerterHost, owner: &str) -> Vec<FactVersionRef> {
    let whole_hash = host
        .ensure_indexed_ready(owner)
        .map(|ir| ir.whole_hash)
        .expect("owner must have an IndexedReady entry");
    let key =
        host.component_meta_result_key(owner, &crate::host_manage::ComponentMetaOptions::default());
    host.project_type_store()
        .component_meta_results()
        .get(&key, whole_hash)
        .expect("a ComponentMetaResultEntry must be published for the owner")
        .read_set_signature
        .facts
        .to_vec()
}

fn has_owner_route_fact(facts: &[FactVersionRef], owner: &str) -> bool {
    facts.iter().any(|f| {
        matches!(
            f,
            FactVersionRef::DerivedFactHash {
                canonical_id,
                kind: DerivedFactKind::Route,
                ..
            } if canonical_id == owner
        )
    })
}

#[test]
fn repeated_query_after_route_only_indexed_read_is_warm_hit() {
    let host = build_host();
    upsert(&host, "/src/types.ts", TYPES_TS, FileLanguage::script_ts());
    upsert(&host, "/src/Comp.vue", OWNER_VUE, FileLanguage::vue());

    // Materialise the OWNER's `IndexedReady` via a genuine route-only
    // read BEFORE `get_component_meta` runs. This reproduces the
    // precondition exactly: "an owner already has an `IndexedReady`
    // from an earlier route-only read".
    let indexed = host.ensure_indexed_ready("/src/Comp.vue");
    assert!(
        indexed.is_some(),
        "route-only read must materialise an IndexedReady artifact \
         for the owner SFC — the route-only-read precondition",
    );
    assert!(
        host.project_type_store()
            .indexed()
            .get_any("/src/Comp.vue")
            .is_some(),
        "the FileArtifactStore must hold the owner artifact after \
         the route-only read",
    );

    // Cold `get_component_meta` — runs the cold resolver and publishes
    // the `ComponentMetaResultEntry` over the pre-existing
    // `IndexedReady`.
    let prime = host.get_component_meta("/src/Comp.vue");
    assert!(prime.is_some(), "cold get_component_meta must resolve");

    // Discriminator 1 — the published signature must NOT carry the
    // owner's own `DerivedFactHash{Route}` fact. Without the owner-Route
    // filter the publish site admits the finalised tracer set verbatim, so
    // the owner's Route fact (observed by the macro-root route walk) IS
    // present and this assertion FAILS; `strip_owner_route_fact` removes
    // exactly this fact.
    let facts = published_facts(&host, "/src/Comp.vue");
    assert!(
        !has_owner_route_fact(&facts, "/src/Comp.vue"),
        "the owner's own `DerivedFactHash{{Route}}` fact MUST be \
         filtered from the published signature — it is dual-sourced \
         on `HostStoreView::derived_hashes` and does not round-trip \
         on warm validation. facts = {facts:#?}",
    );

    // Discriminator 2 (narrowness) — the cross-file route dep's
    // `DerivedFactHash{Route}` fact MUST still be present. A broad
    // route-fact removal would strip this too.
    assert!(
        facts.iter().any(|f| matches!(
            f,
            FactVersionRef::DerivedFactHash {
                canonical_id,
                kind: DerivedFactKind::Route,
                ..
            } if canonical_id == "/src/types.ts"
        )),
        "the cross-file route dep `/src/types.ts` `DerivedFactHash{{Route}}` \
         fact MUST remain in the published signature — `strip_owner_route_fact` \
         filters ONLY the owner's own Route fact, not cross-file route \
         facts. facts = {facts:#?}",
    );

    // Discriminator 3 (behavioural non-regression) — a repeated
    // IDENTICAL query is a warm-cache HIT with no new miss. With the
    // owner-Route fact filtered, the published signature round-trips,
    // so the second query reuses the warm result. (The hard
    // discriminator is Discriminator 1. This assertion guards that the
    // filter does not regress warm reuse.)
    let prov = host.provenance();
    let hits_before = prov.component_meta_result_cache_hits.load(Relaxed);
    let misses_before = prov.component_meta_result_cache_misses.load(Relaxed);

    let second = host.get_component_meta("/src/Comp.vue");
    assert!(second.is_some(), "second identical query must resolve");

    let hits_after = prov.component_meta_result_cache_hits.load(Relaxed);
    let misses_after = prov.component_meta_result_cache_misses.load(Relaxed);
    assert!(
        hits_after > hits_before,
        "a repeated IDENTICAL `get_component_meta` query MUST be a \
         warm-cache HIT even when the owner's IndexedReady came from an \
         earlier route-only read — the published signature must round-trip. \
         hits {hits_before} -> {hits_after}, misses {misses_before} -> {misses_after}",
    );
    assert_eq!(
        misses_before, misses_after,
        "a repeated IDENTICAL query MUST NOT register a new \
         `ComponentMetaResultDb` miss — a miss here means the warm \
         validation rejected the published signature. \
         misses {misses_before} -> {misses_after}",
    );

    // The warm-hit payload must be the genuine component meta — the
    // owner's `RProps` props surface.
    let second_meta = second.unwrap();
    let prop_names: Vec<&str> = second_meta.props.iter().map(|p| p.name.as_str()).collect();
    assert!(
        prop_names.contains(&"a") && prop_names.contains(&"b"),
        "the warm-hit payload must carry the owner's `RProps` props \
         (a + b) — got {prop_names:?}",
    );
}

#[test]
fn editing_route_dep_still_invalidates_after_route_only_indexed_read() {
    // The filter is narrow — it removes ONLY the owner's own Route
    // fact. Cross-file route facts stay, so editing a route dep MUST
    // still invalidate the owner's warm hit. This guards against the
    // filter being widened into the broad route-fact removal that
    // would silently drop cross-file route invalidation.
    let host = build_host();
    upsert(&host, "/src/types.ts", TYPES_TS, FileLanguage::script_ts());
    upsert(&host, "/src/Comp.vue", OWNER_VUE, FileLanguage::vue());

    // Materialise the owner's IndexedReady first (the route-only-read
    // precondition), then prime the component-meta cache.
    let _ = host.ensure_indexed_ready("/src/Comp.vue");
    let prime = host.get_component_meta("/src/Comp.vue");
    assert!(prime.is_some(), "prime call must resolve");

    let prov = host.provenance();
    let misses_before = prov.component_meta_result_cache_misses.load(Relaxed);

    // Edit the route source type — `RProps` loses `b`.
    upsert(
        &host,
        "/src/types.ts",
        "export interface RProps { a: number; }\n",
        FileLanguage::script_ts(),
    );

    let after = host.get_component_meta("/src/Comp.vue");
    assert!(after.is_some(), "post-edit call must still resolve");
    let misses_after = prov.component_meta_result_cache_misses.load(Relaxed);
    assert!(
        misses_after > misses_before,
        "editing the cross-file route dep MUST invalidate the owner's \
         warm hit — the dep's `DerivedFactHash{{Route}}` fact survives \
         `strip_owner_route_fact` (only the OWNER's own Route fact is \
         filtered). misses {misses_before} -> {misses_after}",
    );

    // The post-edit result reflects the NEW shape: `RProps` now has
    // only `a`. A stale warm hit would still report `b`.
    let after_meta = after.unwrap();
    let prop_names: Vec<&str> = after_meta.props.iter().map(|p| p.name.as_str()).collect();
    assert!(
        prop_names.contains(&"a") && !prop_names.contains(&"b"),
        "post-edit component-meta MUST reflect the new `RProps` shape \
         (a, no b) — got {prop_names:?}",
    );
}

/// Read the published `ComponentMetaResultEntry` for `owner`, or `None`
/// when no entry was promoted. Composes the key exactly as
/// `publish_component_meta_cache_entry` does.
fn published_entry_present(host: &VerterHost, owner: &str) -> bool {
    let Some(whole_hash) = host.ensure_indexed_ready(owner).map(|ir| ir.whole_hash) else {
        return false;
    };
    let key =
        host.component_meta_result_key(owner, &crate::host_manage::ComponentMetaOptions::default());
    host.project_type_store()
        .component_meta_results()
        .get(&key, whole_hash)
        .is_some()
}

/// Publish fence: a cold result computed under
/// a validation token that is superseded before promotion MUST NOT be
/// promoted into the final-result cache — but MUST still be returned to
/// the caller (return-only semantics). Discriminates against the
/// pre-change tree, which had no token recheck and would have promoted
/// the result unconditionally.
#[test]
fn publish_fence_skips_promotion_under_superseded_token() {
    use crate::host_manage::component_meta_entry::PUBLISH_FENCE_FORCE_SUPERSEDE;

    let host = build_host();
    upsert(&host, "/src/types.ts", TYPES_TS, FileLanguage::script_ts());
    upsert(&host, "/src/Comp.vue", OWNER_VUE, FileLanguage::vue());

    // Force the fence to observe a superseded token on the next publish.
    PUBLISH_FENCE_FORCE_SUPERSEDE.with(|c| c.set(true));

    // Cold compute: the result is produced and RETURNED, but the fence
    // refuses to promote it.
    let meta = host.get_component_meta("/src/Comp.vue");
    assert!(
        meta.is_some(),
        "the cold result MUST still be returned to the caller even when \
         the publish fence skips promotion (return-only semantics)",
    );
    assert!(
        !published_entry_present(&host, "/src/Comp.vue"),
        "a result computed under a SUPERSEDED token MUST NOT be promoted \
         into the final-result cache (publish fence)",
    );

    // The knob is one-shot (consumed). A subsequent cold query under a
    // live token DOES promote — proving the fence is a transient gate,
    // not a permanent block.
    let meta2 = host.get_component_meta("/src/Comp.vue");
    assert!(meta2.is_some(), "the second query must resolve");
    assert!(
        published_entry_present(&host, "/src/Comp.vue"),
        "a result computed under a LIVE token MUST be promoted — the \
         publish fence only gates superseded-token promotions",
    );
}

/// Publish fence (NON-CURRENT seed, MATCHING token): a cold result whose
/// seed store view the manager could NOT prove current MUST NOT be
/// promoted into the final-result cache — EVEN WHEN the seed's external
/// validation token still matches the live host. This isolates the
/// publish fence's SEED-CURRENTNESS gate from its token gate, and is
/// exactly the isolated gap: a stale ReturnOnly seed while the
/// live token still matches.
///
/// The seed is forced non-current through the RESET-fence decline path,
/// which declines the manager's build WITHOUT advancing any token
/// dimension (epoch / project / env / identity). So `seed.token` is NOT
/// externally superseded relative to the live host — the token gate would
/// PASS — and only the seed-currentness gate can refuse promotion. (The
/// `FORCE_SUPERSEDE_*` knobs bump the epoch, so their ReturnOnly seeds
/// carry a drifted external token a token-only fence already rejects;
/// they cannot discriminate THIS gate.)
///
/// DISCRIMINATES: against a tree whose publish fence checks only the
/// validation token (the pre-change cold paths sampled
/// `current_validation_token()` before the seed-view read and never
/// gated on currentness), this non-current-but-token-matching seed
/// PROMOTES (`published_entry_present == true`) — so the
/// `!published_entry_present` assertion FAILS there. With the
/// seed-currentness gate the result is return-only.
///
/// Runs on a watchdog thread: the knob is thread-local and the
/// store-view manager's retry loop is bounded, so a regression hang
/// surfaces as a test failure (timeout), never a CI hang.
#[test]
fn publish_fence_skips_promotion_when_cold_seed_is_non_current() {
    use std::sync::mpsc;
    use std::time::Duration;

    let host = build_host();
    upsert(&host, "/src/types.ts", TYPES_TS, FileLanguage::script_ts());
    upsert(&host, "/src/Comp.vue", OWNER_VUE, FileLanguage::vue());

    // Confirm the owner is NOT yet published (no prior cold compute).
    assert!(
        !published_entry_present(&host, "/src/Comp.vue"),
        "precondition: no published entry before the first cold compute",
    );

    let host_for_watchdog = Arc::clone(&host);
    let (tx, rx) = mpsc::channel::<(bool, bool)>();
    let watchdog = std::thread::spawn(move || {
        // Force every `base_view` publish to decline through the RESET
        // fence WITHOUT bumping any token dimension, so the manager
        // exhausts its bounded retry and hands back a `ReturnOnly` seed
        // whose validation token STILL EQUALS the live host token. The
        // cold component-meta compute thus runs under a non-current seed
        // that the token gate alone would accept.
        crate::resolver_store::HostStoreView::arm_reset_fence_decline_always_for_tests();
        let meta = host_for_watchdog.get_component_meta("/src/Comp.vue");
        crate::resolver_store::HostStoreView::disarm_reset_fence_decline_always_for_tests();
        let promoted = published_entry_present(&host_for_watchdog, "/src/Comp.vue");
        let _ = tx.send((meta.is_some(), promoted));
    });

    let (resolved, promoted) = rx
        .recv_timeout(Duration::from_secs(10))
        .expect("cold compute under a non-current seed must return in bounded time");
    watchdog.join().expect("watchdog thread must not panic");

    assert!(
        resolved,
        "the cold result MUST still be returned to the caller even when its \
         seed view is non-current (return-only semantics)",
    );
    assert!(
        !promoted,
        "a result computed under a NON-CURRENT (ReturnOnly) seed view MUST NOT \
         be promoted into the final-result cache EVEN when the seed's external \
         token still matches the live host — the publish fence's seed-\
         currentness gate (a token-only fence would wrongly promote here)",
    );

    // Corroboration: with the knob disarmed, a fresh cold query under a
    // genuinely-current seed DOES promote — proving the gate is the seed's
    // currentness, not a permanent block. No spurious refusal.
    let meta2 = host
        .get_component_meta("/src/Comp.vue")
        .expect("post-quiescence query must resolve");
    let _ = meta2;
    assert!(
        published_entry_present(&host, "/src/Comp.vue"),
        "a result computed under a CURRENT seed MUST be promoted — the fence \
         gates only non-current / superseded seeds, never a fresh current one",
    );
}

/// SOUNDNESS: a warm component-meta cache hit must NEVER be served against
/// a known-stale store view.
///
/// `try_component_meta_cache_hit` validates the cached entry's
/// `read_set_signature.facts` against the store view returned by the
/// shared chokepoint. Before the typed-currentness split, the chokepoint
/// could hand back a known-stale view on retry-budget exhaustion: that
/// stale view holds a dependency's OLD whole-hash, so a cache entry
/// referencing the SAME old hash validates `old == old` — a FALSE-POSITIVE
/// that returns stale component-meta to the caller.
///
/// With the typed split, the warm validator accepts ONLY a
/// `StoreViewRead::Current` view. When the manager cannot prove currentness
/// (sustained mid-build token churn, armed here via the persistent
/// supersede knob), the read is `StoreViewRead::ReturnOnly` — the warm
/// validator treats it as a cache MISS and falls to the cold recompute,
/// which produces a fresh result against the LIVE content.
///
/// Discrimination: with the supersede knob armed, the
/// `component_meta_result_cache_hits` counter must NOT increase (the warm
/// path missed) and the `component_meta_result_cache_misses` counter MUST
/// increase (it fell to cold). Against a tree whose chokepoint returns the
/// stale view as validation-capable, the warm validator would validate the
/// planted entry and the HIT counter WOULD increase — so this assertion
/// fails against such a tree. The corroborating arm disarms the churn,
/// mutates the dep, and asserts the recompute reflects the NEW dep shape —
/// never the stale cached payload.
#[test]
fn warm_component_meta_hit_is_suppressed_when_store_view_is_not_current() {
    let host = build_host();
    upsert(&host, "/src/types.ts", TYPES_TS, FileLanguage::script_ts());
    upsert(&host, "/src/Comp.vue", OWNER_VUE, FileLanguage::vue());

    // Cold prime: publishes a `ComponentMetaResultEntry` whose signature
    // references the dep `/src/types.ts` at its CURRENT (old) whole-hash.
    let prime = host.get_component_meta("/src/Comp.vue");
    assert!(prime.is_some(), "prime call must resolve");

    let prov = host.provenance();

    // Sanity arm: a quiescent repeat is a genuine warm HIT — proving the
    // planted entry is valid and warm-hittable when the view IS current.
    let quiescent_hits_before = prov.component_meta_result_cache_hits.load(Relaxed);
    let quiescent_second = host.get_component_meta("/src/Comp.vue");
    assert!(quiescent_second.is_some(), "quiescent repeat must resolve");
    assert!(
        prov.component_meta_result_cache_hits.load(Relaxed) > quiescent_hits_before,
        "a quiescent repeat must be a warm-cache HIT (precondition: the \
         entry is valid and warm-hittable under a current view)",
    );

    // Now force the store-view read to be NON-CURRENT for the next request:
    // every `build_coherent` attempt churns the token mid-build, so
    // `base_view` exhausts its retry budget and returns `ReturnOnly`. The
    // run happens on a watchdog thread (the knob is thread-local and the
    // bounded loop always terminates) so a regression hang surfaces as a
    // test failure, not a CI hang.
    use std::sync::mpsc;
    use std::time::Duration;
    let host_for_watchdog = Arc::clone(&host);
    let (tx, rx) = mpsc::channel::<(u64, u64, bool)>();
    let watchdog = std::thread::spawn(move || {
        let prov = host_for_watchdog.provenance();
        let hits_before = prov.component_meta_result_cache_hits.load(Relaxed);
        let misses_before = prov.component_meta_result_cache_misses.load(Relaxed);
        // Bump the store-view epoch so the manager's cached base view
        // false-misses and the next `base_view` must CLAIM A BUILD (the
        // path the persistent supersede knob engages). The epoch is not a
        // fact in the component-meta signature — the planted entry still
        // validates against the view's unchanged file whole-hashes, which
        // is exactly the stale-but-validating window this test closes.
        host_for_watchdog.bump_store_view_epoch();
        crate::resolver_store::HostStoreView::arm_supersede_always_for_tests();
        let churn = host_for_watchdog.get_component_meta("/src/Comp.vue");
        crate::resolver_store::HostStoreView::disarm_supersede_always_for_tests();
        let hits_after = prov.component_meta_result_cache_hits.load(Relaxed);
        let misses_after = prov.component_meta_result_cache_misses.load(Relaxed);
        let _ = tx.send((
            hits_after - hits_before,
            misses_after - misses_before,
            churn.is_some(),
        ));
    });

    let (hit_delta, miss_delta, churn_resolved) = rx
        .recv_timeout(Duration::from_secs(10))
        .expect("get_component_meta under sustained churn must return in bounded time");
    watchdog.join().expect("watchdog thread must not panic");

    assert!(
        churn_resolved,
        "the request under churn must still RESOLVE (return-only cold result)",
    );
    assert_eq!(
        hit_delta, 0,
        "SOUNDNESS REGRESSION: a warm component-meta cache HIT was served \
         against a known-stale store view — the chokepoint handed a \
         non-current view to the warm validator, which false-validated the \
         planted entry (hit delta = {hit_delta})",
    );
    assert!(
        miss_delta >= 1,
        "the warm validator must MISS on a non-current view and fall to the \
         cold recompute path (miss delta = {miss_delta})",
    );

    // Corroboration: with the churn disarmed, mutate the dep so `RProps`
    // loses `b`. A subsequent query must reflect the NEW dep shape — never
    // the stale cached payload that still carried `b`.
    upsert(
        &host,
        "/src/types.ts",
        "export interface RProps { a: number; }\n",
        FileLanguage::script_ts(),
    );
    let recomputed = host
        .get_component_meta("/src/Comp.vue")
        .expect("post-edit query must resolve");
    let prop_names: Vec<&str> = recomputed.props.iter().map(|p| p.name.as_str()).collect();
    assert!(
        prop_names.contains(&"a") && !prop_names.contains(&"b"),
        "the recompute against the mutated dep MUST reflect the new `RProps` \
         shape (a, no b) — a stale warm hit would still report `b`. \
         got {prop_names:?}",
    );
}

/// OVERLAY MUST NOT LAUNDER CURRENTNESS (Q4): the view-aware warm path
/// (`try_component_meta_cache_hit_with_view_inner`) re-roots the base store
/// view through the session overlay. The overlay must NOT convert a
/// known-stale (`StoreViewRead::ReturnOnly`) base view into a validating one.
///
/// The warm validator reads the base as a typed `StoreViewRead` and applies
/// the overlay ONLY to a proven-`Current` base; a non-current base misses to
/// cold BEFORE the overlay is applied, so the overlay can never re-root a
/// stale snapshot's per-canonical hashes into a fingerprint that looks
/// current.
///
/// Discrimination: prime a warm view-aware entry, then drive
/// `get_component_meta_via_view` under sustained token churn (the base read
/// is `ReturnOnly`). The `component_meta_result_cache_hits` counter must NOT
/// increase. Against a tree where the overlay re-rooting ran on a non-current
/// base and recomputed a validating fingerprint, the warm validator would
/// hit — so the no-hit assertion fails against such a tree.
#[test]
fn overlay_does_not_launder_non_current_base_into_warm_hit() {
    use rustc_hash::FxHashMap;
    use std::sync::mpsc;
    use std::time::Duration;

    let host = build_host();
    upsert(&host, "/src/types.ts", TYPES_TS, FileLanguage::script_ts());
    upsert(&host, "/src/Comp.vue", OWNER_VUE, FileLanguage::vue());

    // A no-overlay session view over the owner: the warm path still routes
    // through `with_session_overlay` (recomputing the coalescing
    // fingerprint), so the laundering risk is exercised even with an empty
    // overlay set.
    let make_view = || {
        crate::session_view::OverlaidView::new(
            Arc::clone(&host),
            FxHashMap::<String, Arc<str>>::default(),
        )
    };

    // Cold prime through the view-aware path so a warm view-aware entry
    // exists, then confirm a quiescent repeat is a genuine warm HIT.
    let view = make_view();
    let prime = host.get_component_meta_via_view("/src/Comp.vue", &view);
    assert!(prime.is_some(), "view-aware prime must resolve");
    let prov = host.provenance();
    let q_hits_before = prov.component_meta_result_cache_hits.load(Relaxed);
    let view_q = make_view();
    let _ = host.get_component_meta_via_view("/src/Comp.vue", &view_q);
    assert!(
        prov.component_meta_result_cache_hits.load(Relaxed) > q_hits_before,
        "a quiescent view-aware repeat must be a warm-cache HIT (precondition)",
    );

    // Drive the view-aware warm path under sustained churn so the base read
    // is `ReturnOnly`. The overlay must NOT launder it into a validating
    // view; the warm hit counter must stay flat.
    let host_for_watchdog = Arc::clone(&host);
    let (tx, rx) = mpsc::channel::<(u64, bool)>();
    let watchdog = std::thread::spawn(move || {
        let prov = host_for_watchdog.provenance();
        let hits_before = prov.component_meta_result_cache_hits.load(Relaxed);
        let churn_view = crate::session_view::OverlaidView::new(
            Arc::clone(&host_for_watchdog),
            FxHashMap::<String, Arc<str>>::default(),
        );
        // Bump the store-view epoch so the manager's cached base view
        // false-misses and the next `base_view` must claim a build (where
        // the persistent supersede knob engages → `ReturnOnly`). The epoch
        // is not a component-meta signature fact, so the planted entry
        // still validates against the unchanged file whole-hashes — the
        // overlay must not launder that stale-but-validating base.
        host_for_watchdog.bump_store_view_epoch();
        crate::resolver_store::HostStoreView::arm_supersede_always_for_tests();
        let churn = host_for_watchdog.get_component_meta_via_view("/src/Comp.vue", &churn_view);
        crate::resolver_store::HostStoreView::disarm_supersede_always_for_tests();
        let hits_after = prov.component_meta_result_cache_hits.load(Relaxed);
        let _ = tx.send((hits_after - hits_before, churn.is_some()));
    });

    let (hit_delta, churn_resolved) = rx
        .recv_timeout(Duration::from_secs(10))
        .expect("view-aware query under churn must return in bounded time");
    watchdog.join().expect("watchdog thread must not panic");

    assert!(
        churn_resolved,
        "the view-aware request under churn must still RESOLVE (return-only cold result)",
    );
    assert_eq!(
        hit_delta, 0,
        "OVERLAY-LAUNDERING REGRESSION: the session overlay converted a \
         known-stale (ReturnOnly) base view into a validating view and the \
         view-aware warm validator served a HIT (hit delta = {hit_delta}). \
         The overlay must apply only to a proven-Current base.",
    );
}

/// SOUNDNESS: the ENCODED-PAYLOAD warm cache (`try_get_cached_meta_payload`,
/// the surface the NAPI/WASM `get_component_meta_payload` /
/// `get_component_meta_payload_batch` consult FIRST) must NEVER serve a
/// cached payload against a known-stale store view.
///
/// The payload warm validator returns the cached encoded bytes directly to
/// the FFI consumer with NO outer publish / is_stable fence. Before the
/// typed-currentness split reached this surface, it built a RAW
/// `resolver_store_view()` and validated `view.validates_fact_signature(...)`
/// with no `.current()` gate: under sustained churn the shared chokepoint
/// could hand back a known-stale `StoreViewRead::ReturnOnly` view holding a
/// dependency's OLD whole-hash, so a cached payload referencing the SAME old
/// hash validates `old == old` — a FALSE-POSITIVE that hands the FFI
/// consumer a stale full-meta payload.
///
/// With the fix, `try_get_cached_meta_payload` reads the store view as a
/// typed `StoreViewRead` and serves a warm hit ONLY against a proven-
/// `Current` view; a `ReturnOnly` read returns `None` (cache miss).
///
/// Discrimination: prime the payload cache so a quiescent peek HITS
/// (returns `Some`), then under sustained token churn the SAME peek must
/// return `None`. Against a tree whose payload validator uses the raw view,
/// the stale payload validates and the peek returns `Some` — so the
/// `is_none()` assertion FAILS against such a tree. The corroborating arm
/// disarms the churn, mutates the dep, and asserts a fresh resolve reflects
/// the NEW dep shape — never the stale cached payload.
#[test]
fn warm_meta_payload_hit_is_suppressed_when_store_view_is_not_current() {
    use std::sync::mpsc;
    use std::time::Duration;

    let host = build_host();
    upsert(&host, "/src/types.ts", TYPES_TS, FileLanguage::script_ts());
    upsert(&host, "/src/Comp.vue", OWNER_VUE, FileLanguage::vue());

    // Cold prime the component-meta result so a published signature exists
    // referencing the dep `/src/types.ts` at its CURRENT (old) whole-hash.
    let prime = host.get_component_meta("/src/Comp.vue");
    assert!(prime.is_some(), "prime call must resolve");

    // Plant an encoded-payload cache entry whose fact signature is exactly
    // the published component-meta signature (it references the dep's
    // current whole-hash). The payload bytes are opaque to the validator —
    // their identity does not matter, only whether the entry is served.
    let facts = published_facts(&host, "/src/Comp.vue");
    assert!(
        !facts.is_empty(),
        "the published signature must be non-empty (the payload validator \
         only validates a non-empty fact rail)",
    );
    let planted_payload: Vec<u8> = vec![0xAB, 0xCD, 0xEF];
    host.store_meta_payload(
        "/src/Comp.vue",
        &facts,
        planted_payload.clone(),
        host.project_type_store.current_project_generation(),
    );

    // Sanity arm: a quiescent peek is a genuine HIT (the planted entry is
    // valid and warm-hittable when the view IS current).
    let quiescent = host.try_get_cached_meta_payload("/src/Comp.vue");
    assert_eq!(
        quiescent.as_deref(),
        Some(planted_payload.as_slice()),
        "a quiescent payload peek must HIT and return the planted payload \
         (precondition: the entry is valid under a current view)",
    );

    // Force the store-view read NON-CURRENT for the next peek: every
    // `build_coherent` attempt churns the token mid-build, so `base_view`
    // exhausts its retry budget and returns `ReturnOnly`. Run on a watchdog
    // thread (the knob is thread-local; the bounded loop always terminates)
    // so a regression hang surfaces as a failure, not a CI hang.
    let host_for_watchdog = Arc::clone(&host);
    let (tx, rx) = mpsc::channel::<Option<Vec<u8>>>();
    let watchdog = std::thread::spawn(move || {
        // Bump the store-view epoch so the manager's cached base view
        // false-misses and the next `base_view` must claim a build (where
        // the persistent supersede knob engages → `ReturnOnly`). The epoch
        // is not a fact in the payload signature — the planted entry still
        // validates against the unchanged file whole-hashes, which is
        // exactly the stale-but-validating window this test closes.
        host_for_watchdog.bump_store_view_epoch();
        crate::resolver_store::HostStoreView::arm_supersede_always_for_tests();
        let churn_peek = host_for_watchdog.try_get_cached_meta_payload("/src/Comp.vue");
        crate::resolver_store::HostStoreView::disarm_supersede_always_for_tests();
        let _ = tx.send(churn_peek);
    });

    let churn_peek = rx
        .recv_timeout(Duration::from_secs(10))
        .expect("try_get_cached_meta_payload under sustained churn must return in bounded time");
    watchdog.join().expect("watchdog thread must not panic");

    assert!(
        churn_peek.is_none(),
        "SOUNDNESS REGRESSION: the encoded-payload warm cache served a \
         cached payload against a known-stale (ReturnOnly) store view — the \
         payload validator validated against a raw, non-current view and \
         false-positived the planted entry (got {churn_peek:?}). A \
         non-current read MUST miss to cold.",
    );

    // Corroboration: with the churn disarmed, mutate the dep so `RProps`
    // loses `b`. A fresh resolve must reflect the NEW dep shape — never the
    // stale cached payload that still carried `b`.
    upsert(
        &host,
        "/src/types.ts",
        "export interface RProps { a: number; }\n",
        FileLanguage::script_ts(),
    );
    let recomputed = host
        .get_component_meta("/src/Comp.vue")
        .expect("post-edit query must resolve");
    let prop_names: Vec<&str> = recomputed.props.iter().map(|p| p.name.as_str()).collect();
    assert!(
        prop_names.contains(&"a") && !prop_names.contains(&"b"),
        "the resolve against the mutated dep MUST reflect the new `RProps` \
         shape (a, no b) — a stale warm hit would still report `b`. \
         got {prop_names:?}",
    );
}

/// The encoded-payload lane's value-side generation backstop (the same
/// discipline as the typed result caches' `validated_at_generation`
/// gate): an UNDER-RECORDED fact signature — the degenerate case being
/// the EMPTY signature, which `validates_fact_signature` accepts
/// trivially — must not keep validating across project-shape mutations
/// the missing facts would have caught. The payload lane has no outer
/// publish / `is_stable` fence, so without the stamp an under-recorded
/// entry is served to the FFI consumer PERMANENTLY.
#[test]
fn meta_payload_under_recorded_signature_misses_after_project_mutation() {
    let host = build_host();
    upsert(&host, "/src/types.ts", TYPES_TS, FileLanguage::script_ts());
    upsert(&host, "/src/Comp.vue", OWNER_VUE, FileLanguage::vue());

    // Plant a payload with the EMPTY (under-recorded) signature.
    let planted: Vec<u8> = vec![0x11, 0x22, 0x33];
    host.store_meta_payload(
        "/src/Comp.vue",
        &[],
        planted.clone(),
        host.project_type_store.current_project_generation(),
    );
    assert_eq!(
        host.try_get_cached_meta_payload("/src/Comp.vue").as_deref(),
        Some(planted.as_slice()),
        "sanity: the planted entry hits while the project shape is \
         unchanged",
    );

    // Land a project-shape mutation that bumps `project_generation`
    // WITHOUT the wide derived-raw evict (the stamp-only
    // `set_import_dependencies` route push on the DEP): the planted
    // payload entry survives physically, so only the generation
    // backstop can reject it.
    let pre = host.project_type_store().current_project_generation();
    host.set_import_dependencies(
        "/src/types.ts",
        vec![crate::types::DependencyResolution {
            specifier: "./somewhere".to_string(),
            resolved_canonical_id: Some("/src/elsewhere.ts".to_string()),
            possible_canonical_ids: Vec::new(),
        }],
    );
    assert!(
        host.project_type_store().current_project_generation() > pre,
        "anti-vacuity: the route push must have bumped project_generation",
    );

    assert!(
        host.try_get_cached_meta_payload("/src/Comp.vue").is_none(),
        "an under-recorded (empty) signature must NOT keep validating \
         across a project mutation — the generation backstop must miss \
         to cold",
    );
}

// ── Fenced-serve admission: ReturnOnly never publishes ───────────────
//
// A cold component-meta compute whose traced scope consumed a FENCED
// (ReturnOnly, `store_published == false`) `IndexedReady` serve derived
// its payload from a served-without-publication artifact while its fact
// stamps are read from the LIVE state — an entry the read-side fact
// rail cannot reject. Each producer must consult the tracer's by-value
// `fenced_serve_observed` flag and DECLINE the shared-cache publish.
//
// Discrimination: the seam hook below raises the flag WITHOUT moving
// any validation-token dimension (no mutation lands), so the publish
// fence's seed-token recheck PASSES and only the by-value consult can
// refuse. Pre-consult the entry LANDS; post-consult it is declined.

/// Arm the materialize seam to raise the by-value fenced-serve flag on
/// every tracer active on the flight's thread — the same chokepoint a
/// real fenced serve fans out through — while mutating NOTHING, so the
/// seed-fence token recheck cannot be what declines the publish.
fn arm_force_fenced_serve_flag(host: &VerterHost) {
    *host.materialize_seam_hook.lock() = Some(Arc::new(|| {
        crate::resolver_core::resolver_context::note_non_cacheable_read_fan_out(
            crate::resolver_core::resolver_context::NonCacheableReadReason::FencedServe,
        );
    }));
}

fn disarm_materialize_seam(host: &VerterHost) {
    *host.materialize_seam_hook.lock() = None;
}

/// ReturnOnly never publishes — `get_component_meta` (base path). The
/// caller is still served the freshly computed meta; only the
/// `ComponentMetaResultDb` promotion is declined. With the seam
/// disarmed, the next cold compute publishes — the refusal was the
/// admission gate acting, not a broken publish path.
#[test]
fn fenced_serve_inside_get_component_meta_declines_the_publish() {
    let host = build_host();
    upsert(&host, "/src/types.ts", TYPES_TS, FileLanguage::script_ts());
    upsert(&host, "/src/Comp.vue", OWNER_VUE, FileLanguage::vue());

    arm_force_fenced_serve_flag(&host);
    let fenced = host.get_component_meta("/src/Comp.vue");
    disarm_materialize_seam(&host);

    assert!(
        fenced.is_some(),
        "the declined publish must still serve the freshly computed meta \
         (return-only semantics)",
    );
    // THE PIN: the traced scope consumed a fenced serve, so the
    // promotion must DECLINE — by value, even though the seed-fence
    // token recheck passes (nothing mutated).
    assert!(
        !published_entry_present(&host, "/src/Comp.vue"),
        "a component-meta cold compute whose traced scope consumed a \
         FENCED (ReturnOnly) IndexedReady serve must DECLINE the \
         final-result-cache publish — its fact stamps validate against \
         the live view while its payload was computed from the \
         superseded artifact, an entry the read-side fact rail cannot \
         reject",
    );

    // Recovery: a quiescent recompute publishes and serves warm.
    let recomputed = host.get_component_meta("/src/Comp.vue");
    assert!(recomputed.is_some(), "quiescent recompute must resolve");
    assert!(
        published_entry_present(&host, "/src/Comp.vue"),
        "a quiescent recompute must publish the final-result entry",
    );
}

/// ReturnOnly never publishes — `get_component_meta_with_resolution`
/// path (its own `with_fact_tracer` producer + publish site).
#[test]
fn fenced_serve_inside_with_resolution_declines_the_publish() {
    let host = build_host();
    upsert(&host, "/src/types.ts", TYPES_TS, FileLanguage::script_ts());
    upsert(&host, "/src/Comp.vue", OWNER_VUE, FileLanguage::vue());

    arm_force_fenced_serve_flag(&host);
    let fenced = host.get_component_meta_with_resolution("/src/Comp.vue");
    disarm_materialize_seam(&host);

    assert!(
        fenced.is_some(),
        "the declined publish must still serve the freshly computed \
         (meta, resolution) pair",
    );
    assert!(
        !published_entry_present(&host, "/src/Comp.vue"),
        "the with-resolution producer must decline the publish by value \
         when its traced scope consumed a fenced serve",
    );

    let recomputed = host.get_component_meta_with_resolution("/src/Comp.vue");
    assert!(recomputed.is_some(), "quiescent recompute must resolve");
    assert!(
        published_entry_present(&host, "/src/Comp.vue"),
        "a quiescent recompute must publish the final-result entry",
    );
}

/// ReturnOnly never publishes — `get_component_meta_via_view` path
/// (the view-aware `with_fact_tracer` producer publishing through
/// `publish_component_meta_cache_entry_with_view`). A base
/// `HostViewRef` keys the same slot as the base path, so the base
/// probe observes the view-aware publish decision.
#[test]
fn fenced_serve_inside_via_view_declines_the_publish() {
    let host = build_host();
    upsert(&host, "/src/types.ts", TYPES_TS, FileLanguage::script_ts());
    upsert(&host, "/src/Comp.vue", OWNER_VUE, FileLanguage::vue());

    let view = crate::session_view::HostViewRef::new(&host);
    arm_force_fenced_serve_flag(&host);
    let fenced = host.get_component_meta_via_view("/src/Comp.vue", &view);
    disarm_materialize_seam(&host);

    assert!(
        fenced.is_some(),
        "the declined publish must still serve the freshly computed meta",
    );
    assert!(
        !published_entry_present(&host, "/src/Comp.vue"),
        "the view-aware producer must decline the publish by value when \
         its traced scope consumed a fenced serve",
    );

    let recomputed = host.get_component_meta_via_view("/src/Comp.vue", &view);
    assert!(recomputed.is_some(), "quiescent recompute must resolve");
    assert!(
        published_entry_present(&host, "/src/Comp.vue"),
        "a quiescent recompute must publish the final-result entry",
    );
}

/// Negative control: the seam armed but raising NOTHING must not trip
/// the consult — no fenced serve is consumed, the publish lands. Proves
/// the producers consult the fenced-serve flag rather than declining
/// whenever the seam fires.
#[test]
fn unfenced_cold_compute_still_publishes_through_the_seam() {
    let host = build_host();
    upsert(&host, "/src/types.ts", TYPES_TS, FileLanguage::script_ts());
    upsert(&host, "/src/Comp.vue", OWNER_VUE, FileLanguage::vue());

    *host.materialize_seam_hook.lock() = Some(Arc::new(|| {}));
    let cold = host.get_component_meta("/src/Comp.vue");
    disarm_materialize_seam(&host);

    assert!(cold.is_some(), "cold compute must resolve");
    assert!(
        published_entry_present(&host, "/src/Comp.vue"),
        "an un-fenced cold compute must publish the final-result entry",
    );
}
