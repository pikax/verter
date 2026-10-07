use super::*;

#[cfg(not(target_arch = "wasm32"))]
#[test]
fn fact_versions_match_uses_derived_fact_kind_specific_validation() {
    let project = make_project();
    project
        .upsert_base("/index.ts", "export * from './inner'")
        .unwrap();
    project
        .upsert_base("/inner.ts", "export interface Inner {}")
        .unwrap();

    // Materialise the owner so it publishes a route surface.
    let _ = project
        .host()
        .ensure_indexed_ready_serve("/index.ts")
        .expect("the wildcard reexporter must materialise");

    let route_hash = project
        .host()
        .current_derived_fact_hash(
            "/index.ts",
            verter_session_query::facts::fact_cache::DerivedFactKind::Route,
        )
        .expect("a materialised wildcard reexporter must publish a Route fact");

    assert!(project.host().fact_versions_match(&[
        verter_session_query::facts::fact_cache::FactVersionRef::DerivedFactHash {
            canonical_id: "/index.ts".to_string(),
            kind: verter_session_query::facts::fact_cache::DerivedFactKind::Route,
            hash: route_hash,
        },
    ]));

    assert!(!project.host().fact_versions_match(&[
        verter_session_query::facts::fact_cache::FactVersionRef::DerivedFactHash {
            canonical_id: "/index.ts".to_string(),
            kind: verter_session_query::facts::fact_cache::DerivedFactKind::Route,
            hash: [9; 16],
        },
    ]));
}

#[test]
fn snapshot_view_is_stale_but_coherent_after_host_changes() {
    let project = make_project();
    project
        .upsert_base("/types.ts", "export interface Props { label: string }")
        .unwrap();

    let before_hash = project
        .host()
        .get_whole_hash("/types.ts")
        .expect("whole hash should exist before mutation");
    let before_view = project.host().snapshot_view();
    let before_epoch = before_view.mutation_epoch();
    let fact = verter_session_query::facts::fact_cache::FactVersionRef::FileWholeHash {
        canonical_id: "/types.ts".to_string(),
        hash: before_hash,
    };

    assert!(before_view.validates(&fact));

    project
        .upsert_base("/types.ts", "export interface Props { disabled: boolean }")
        .unwrap();

    let after_view = project.host().snapshot_view();
    let after_epoch = after_view.mutation_epoch();

    assert!(
        before_view.validates(&fact),
        "a captured store view should keep validating against the snapshot it was created from"
    );
    assert!(
        !after_view.validates(&fact),
        "a fresh store view should reject stale facts after the host changes"
    );
    assert_ne!(before_epoch, after_epoch);
    assert_ne!(before_view.compat_token(), after_view.compat_token());
}

#[cfg(not(target_arch = "wasm32"))]
#[test]
fn ensure_loaded_first_time_advances_the_validation_token() {
    let ws = Arc::new(verter_workspace::MemoryWorkspace::new(
        verter_workspace::MemoryOptions::default(),
    ));
    ws.inject_file(
        "/workspace/App.vue".to_string(),
        Arc::from(sfc("msg: string")),
    );

    let project = make_workspace_project(ws.clone());
    let before_token = project.host().current_validation_token();

    assert!(
        project.ensure_loaded("/workspace/App.vue").unwrap(),
        "ensure_loaded should load the workspace file into the host"
    );

    let after_token = project.host().current_validation_token();
    // SOUNDNESS: a first-time additive load adds a
    // scheduler node + `whole_hashes` entry and populates
    // `derived_raw_cache` — all of which `HostStoreView::build` snapshots
    // BY VALUE. A `StoreViewManager`-cached base snapshot built before the
    // load does NOT track the new canonical, so the FULL reuse token MUST
    // advance or the manager would hand a stale pre-load snapshot to the
    // next caller and the untracked-file optimistic-accept would fossilize
    // against it. `integrate_scheduler_snapshot` does not publish into
    // `FileArtifactStore`, so the DEDICATED `load_generation` dimension is
    // what moves (NOT `store_view_epoch`, which the publish fence checks —
    // a cold compute's own dependency loads must not self-fence promotion).
    assert_ne!(
        before_token, after_token,
        "first-time load MUST advance the full validation token (a manager-cached \
         base view must be invalidated)"
    );
    assert_ne!(
        before_token.load_generation, after_token.load_generation,
        "first-time load MUST advance the dedicated load_generation token dimension"
    );
    assert_eq!(
        before_token.store_view_epoch, after_token.store_view_epoch,
        "first-time load MUST NOT bump store_view_epoch — it is the compute's own \
         additive work, excluded from the publish fence's external-supersession check"
    );

    // Reloading the same file via evict + ensure_loaded with CHANGED content IS
    // a content-change boundary that must advance the store_view_epoch.
    project.host().evict("/workspace/App.vue");
    ws.inject_file(
        "/workspace/App.vue".to_string(),
        Arc::from(sfc("msg: number")),
    );
    let post_evict_epoch = project.host().current_store_view_epoch();
    assert!(
        project.ensure_loaded("/workspace/App.vue").unwrap(),
        "evicted file must reload via ensure_loaded"
    );
    let reload_view = project.host().snapshot_view();
    assert_ne!(
        post_evict_epoch,
        reload_view.mutation_epoch(),
        "reload after evict with changed content must advance the mutation epoch"
    );
}

#[test]
fn store_view_epoch_advances_on_upsert() {
    let project = make_project();
    project
        .upsert_base("/App.vue", &sfc("msg: string"))
        .expect("upsert should succeed");
    let epoch_after_first = project.host().current_store_view_epoch();

    project
        .upsert_base("/App.vue", &sfc("msg: number"))
        .expect("re-upsert should succeed");
    let epoch_after_second = project.host().current_store_view_epoch();

    assert_ne!(
        epoch_after_first, epoch_after_second,
        "mutation epoch must advance on re-upsert so compat tokens distinguish views"
    );
}

#[test]
fn store_view_epoch_advances_on_evict() {
    let project = make_project();
    project
        .upsert_base("/App.vue", &sfc("msg: string"))
        .expect("upsert should succeed");
    let epoch_before = project.host().current_store_view_epoch();

    project.host().evict("/App.vue");
    let epoch_after = project.host().current_store_view_epoch();

    assert_ne!(
        epoch_before, epoch_after,
        "mutation epoch must advance on evict so compat tokens distinguish views"
    );
}

// ---------------------------------------------------------------------------
// Basic project lifecycle
// ---------------------------------------------------------------------------

#[test]
fn open_session_returns_unique_ids() {
    let project = make_project();
    let s1 = project.open_session_batch().unwrap();
    let s2 = project.open_session_batch().unwrap();
    assert_ne!(s1.id(), s2.id());
    assert_eq!(project.session_count(), 2);
}

#[test]
fn close_session_is_idempotent() {
    let project = make_project();
    let s = project.open_session_batch().unwrap();
    s.close();
    s.close(); // second close is a no-op
    assert!(s.is_closed());
    assert_eq!(project.session_count(), 0);
}

#[test]
fn session_drop_auto_closes() {
    let project = make_project();
    {
        let _s = project.open_session_batch().unwrap();
        assert_eq!(project.session_count(), 1);
    }
    assert_eq!(project.session_count(), 0);
}

#[test]
fn ensure_loaded_populates_shared_base_from_workspace() {
    let ws = Arc::new(verter_workspace::MemoryWorkspace::new(
        verter_workspace::MemoryOptions::default(),
    ));
    ws.inject_file(
        "/workspace/App.vue".to_string(),
        Arc::from(sfc("msg: string")),
    );

    let project = make_workspace_project(Arc::clone(&ws));

    assert!(
        project.ensure_loaded("/workspace/App.vue").unwrap(),
        "ensure_loaded should materialize the workspace file into the shared base project"
    );
    assert!(
        project.base_file_ids().contains("/workspace/App.vue"),
        "base index should include the loaded workspace file"
    );

    let session = project.open_session_batch().unwrap();
    assert!(session.has_file("/workspace/App.vue").unwrap());
    let source = session
        .get_effective_source("/workspace/App.vue")
        .unwrap()
        .expect("session should see the loaded base source");
    assert!(source.contains("msg: string"));
}

#[test]
fn refresh_base_reloads_workspace_source_into_shared_base() {
    let ws = Arc::new(verter_workspace::MemoryWorkspace::new(
        verter_workspace::MemoryOptions::default(),
    ));
    ws.inject_file(
        "/workspace/App.vue".to_string(),
        Arc::from(sfc("msg: string")),
    );

    let project = make_workspace_project(Arc::clone(&ws));
    assert!(project.ensure_loaded("/workspace/App.vue").unwrap());

    ws.inject_file(
        "/workspace/App.vue".to_string(),
        Arc::from(sfc("count: number")),
    );

    assert!(
        project.refresh_base("/workspace/App.vue").unwrap(),
        "refresh_base should reload the latest workspace content into shared base state"
    );

    let session = project.open_session_batch().unwrap();
    let source = session
        .get_effective_source("/workspace/App.vue")
        .unwrap()
        .expect("session should see the refreshed base source");
    assert!(source.contains("count: number"));
    assert!(!source.contains("msg: string"));
}

#[test]
fn methods_fail_after_close() {
    let project = make_project();
    let s = project.open_session_batch().unwrap();
    s.close();
    assert!(matches!(
        s.upsert("Comp.vue", "source".into()),
        Err(MetaError::SessionClosed)
    ));
    assert!(matches!(
        s.delete("Comp.vue"),
        Err(MetaError::SessionClosed)
    ));
    assert!(matches!(
        s.get_analysis("Comp.vue"),
        Err(MetaError::SessionClosed)
    ));
}

// ---------------------------------------------------------------------------
// Overlay isolation: two sessions don't see each other's overlays
// ---------------------------------------------------------------------------

#[test]
fn two_sessions_dont_see_each_others_upserts() {
    let project = make_project();
    let base = sfc("msg: string");
    project.upsert_base("Comp.vue", &base).unwrap();

    let s1 = project.open_session_batch().unwrap();
    let s2 = project.open_session_batch().unwrap();

    // Session 1 updates the file
    let modified = sfc("msg: string; count: number");
    s1.upsert("Comp.vue", modified.clone()).unwrap();

    // Session 1 sees the modified source
    let src1 = s1.get_effective_source("Comp.vue").unwrap().unwrap();
    assert!(
        src1.contains("count: number"),
        "session 1 should see its own overlay"
    );

    // Session 2 sees the original base source
    let src2 = s2.get_effective_source("Comp.vue").unwrap().unwrap();
    assert!(
        !src2.contains("count: number"),
        "session 2 must NOT see session 1's overlay"
    );
    assert!(
        src2.contains("msg: string"),
        "session 2 should see base source"
    );
}

#[test]
fn delete_in_session_a_does_not_hide_from_session_b() {
    let project = make_project();
    let base = sfc("msg: string");
    project.upsert_base("Comp.vue", &base).unwrap();

    let s1 = project.open_session_batch().unwrap();
    let s2 = project.open_session_batch().unwrap();

    // Session 1 deletes the file
    s1.delete("Comp.vue").unwrap();

    // Session 1 doesn't see the file
    assert!(!s1.has_file("Comp.vue").unwrap());
    assert!(s1.get_effective_source("Comp.vue").unwrap().is_none());

    // Session 2 still sees the file
    assert!(s2.has_file("Comp.vue").unwrap());
    let src2 = s2.get_effective_source("Comp.vue").unwrap();
    assert!(src2.is_some(), "session 2 should still see the base file");
}

// ---------------------------------------------------------------------------
// Analysis through overlay
// ---------------------------------------------------------------------------

// Overlay-visible semantics depend on the consumer paths
// (`get_analysis`, `get_component_meta`, `evaluate_types`) routing
// host reads through the overlay-aware
// `SessionResolverContext::resolver_store_view`, which wraps the base
// `HostStoreView` via `with_session_overlay` so cross-file dep facts
// pinned to BASE content miss whenever a session overlays or
// tombstones a dep. The R20 multi-candidate cache substrate plus that
// view wiring is what these tests exercise end-to-end.
#[test]
fn get_analysis_sees_overlay_content() {
    let project = make_project();
    let base = sfc("msg: string");
    project.upsert_base("Comp.vue", &base).unwrap();

    let s = project.open_session_batch().unwrap();
    let modified = sfc("msg: string; count: number");
    s.upsert("Comp.vue", modified).unwrap();

    let analysis = s.get_analysis("Comp.vue").unwrap();
    assert!(
        analysis.is_some(),
        "should return analysis for overlayed file"
    );

    let snapshot = analysis.unwrap();
    let names = prop_names(&snapshot);
    assert!(
        names.contains(&"count".to_string()),
        "analysis should reflect overlay content with 'count' prop, got: {:?}",
        names
    );
}

#[test]
fn get_analysis_without_overlay_uses_base() {
    let project = make_project();
    let base = sfc("msg: string");
    project.upsert_base("Comp.vue", &base).unwrap();

    let s = project.open_session_batch().unwrap();

    // No overlay — should see base analysis
    let analysis = s.get_analysis("Comp.vue").unwrap();
    assert!(analysis.is_some());

    let snapshot = analysis.unwrap();
    let names = prop_names(&snapshot);
    assert!(
        names.contains(&"msg".to_string()),
        "should see base 'msg' prop, got: {:?}",
        names
    );
    assert!(
        !names.contains(&"count".to_string()),
        "should NOT see 'count' prop from base"
    );
}

#[test]
fn get_analysis_for_deleted_file_returns_none() {
    // R17 — overlay-Delete short-circuits the consumer path. The
    // session's `get_analysis` consults its overlay map via the
    // view substrate (`OverlaidViewRef` constructed at call time)
    // and returns `None` for tombstoned canonicals without reading
    // the base host.
    let project = make_project();
    let base = sfc("msg: string");
    project.upsert_base("Comp.vue", &base).unwrap();

    let s = project.open_session_batch().unwrap();
    s.delete("Comp.vue").unwrap();

    let analysis = s.get_analysis("Comp.vue").unwrap();
    assert!(
        analysis.is_none(),
        "analysis for tombstoned file should be None"
    );
}

// ---------------------------------------------------------------------------
// Overlay isolation for analysis
// ---------------------------------------------------------------------------

#[test]
fn analysis_isolation_between_sessions() {
    let project = make_project();
    let base = sfc("msg: string");
    project.upsert_base("Comp.vue", &base).unwrap();

    let s1 = project.open_session_batch().unwrap();
    let s2 = project.open_session_batch().unwrap();

    // Session 1 modifies the file
    s1.upsert("Comp.vue", sfc("count: number")).unwrap();

    // Session 1 sees count
    let snap1 = s1.get_analysis("Comp.vue").unwrap().unwrap();
    let names1 = prop_names(&snap1);
    assert!(
        names1.contains(&"count".to_string()),
        "session 1 should see 'count', got: {:?}",
        names1
    );
    assert!(
        !names1.contains(&"msg".to_string()),
        "session 1 should NOT see 'msg'"
    );

    // Session 2 sees msg (base)
    let snap2 = s2.get_analysis("Comp.vue").unwrap().unwrap();
    let names2 = prop_names(&snap2);
    assert!(
        names2.contains(&"msg".to_string()),
        "session 2 should see base 'msg', got: {:?}",
        names2
    );
    assert!(
        !names2.contains(&"count".to_string()),
        "session 2 should NOT see session 1's 'count'"
    );
}

// ---------------------------------------------------------------------------
// Shutdown
// ---------------------------------------------------------------------------

#[test]
fn shutdown_marks_project_dead() {
    let project = make_project();
    let s = project.open_session_batch().unwrap();

    project.shutdown();

    assert!(project.is_shutdown());
    assert!(matches!(
        s.upsert("Comp.vue", "x".into()),
        Err(MetaError::Shutdown)
    ));
    assert!(matches!(
        project.open_session_batch(),
        Err(MetaError::Shutdown)
    ));
}

#[test]
fn shutdown_is_idempotent() {
    let project = make_project();
    project.shutdown();
    project.shutdown(); // no panic
}

// ---------------------------------------------------------------------------
// Overlay generation tracking
// ---------------------------------------------------------------------------

#[test]
fn overlay_generation_bumps_on_mutations() {
    let project = make_project();
    let s = project.open_session_batch().unwrap();

    assert_eq!(s.overlay_generation(), 0);
    s.upsert("A.vue", "a".into()).unwrap();
    assert_eq!(s.overlay_generation(), 1);
    s.delete("B.vue").unwrap();
    assert_eq!(s.overlay_generation(), 2);
}

#[test]
fn reset_restores_base_state_and_drops_overlay_only_files() {
    let project = make_project();
    let base = sfc("label: string");
    let modified = sfc("count: number");
    project.upsert_base("A.vue", &base).unwrap();

    let s = project.open_session_batch().unwrap();
    s.upsert("A.vue", modified.clone()).unwrap();
    s.upsert("Temp.vue", sfc("temp: boolean")).unwrap();

    assert!(s
        .get_effective_source("A.vue")
        .unwrap()
        .unwrap()
        .contains("count: number"));
    assert!(s.has_file("Temp.vue").unwrap());

    s.reset("A.vue").unwrap();
    s.reset("Temp.vue").unwrap();

    let restored = s.get_effective_source("A.vue").unwrap().unwrap();
    assert!(restored.contains("label: string"));
    assert!(!restored.contains("count: number"));
    assert!(!s.has_file("Temp.vue").unwrap());
    assert!(s.get_effective_source("Temp.vue").unwrap().is_none());
    assert_eq!(s.overlay_generation(), 4);
}

#[test]
fn reset_reverts_an_active_overlay_from_the_shared_host() {
    let project = make_project();
    let base = sfc("label: string");
    let modified = sfc("count: number");
    project.upsert_base("A.vue", &base).unwrap();

    let s = project.open_session_batch().unwrap();
    s.upsert("A.vue", modified).unwrap();

    let analysis = s.get_analysis("A.vue").unwrap().unwrap();
    assert!(
        prop_names(&analysis).contains(&"count".to_string()),
        "active overlay should be visible before reset"
    );

    s.reset("A.vue").unwrap();

    let analysis = s.get_analysis("A.vue").unwrap().unwrap();
    let names = prop_names(&analysis);
    assert!(
        names.contains(&"label".to_string()),
        "base props should be visible after reset, got: {names:?}"
    );
    assert!(
        !names.contains(&"count".to_string()),
        "overlay props must be removed after reset, got: {names:?}"
    );
}

// ---------------------------------------------------------------------------
// visible_file_ids
// ---------------------------------------------------------------------------

#[test]
fn visible_file_ids_reflects_overlays() {
    let project = make_project();
    project.upsert_base("A.vue", &sfc("a: string")).unwrap();
    project.upsert_base("B.vue", &sfc("b: string")).unwrap();

    let s = project.open_session_batch().unwrap();
    s.delete("A.vue").unwrap();
    s.upsert("C.vue", sfc("c: string")).unwrap();

    let ids = s.visible_file_ids().unwrap();
    assert!(!ids.contains(&"A.vue".to_string()), "A.vue was deleted");
    assert!(ids.contains(&"B.vue".to_string()), "B.vue is in base");
    assert!(
        ids.contains(&"C.vue".to_string()),
        "C.vue was added by overlay"
    );
}

// ---------------------------------------------------------------------------
// Concurrent session activity (sequential in this test, but isolated)
// ---------------------------------------------------------------------------

#[test]
fn concurrent_sessions_on_different_files() {
    let project = make_project();
    project.upsert_base("A.vue", &sfc("a: string")).unwrap();
    project.upsert_base("B.vue", &sfc("b: string")).unwrap();

    let s1 = project.open_session_batch().unwrap();
    let s2 = project.open_session_batch().unwrap();

    // Session 1 modifies A
    s1.upsert("A.vue", sfc("a_modified: number")).unwrap();

    // Session 2 modifies B
    s2.upsert("B.vue", sfc("b_modified: number")).unwrap();

    // Session 1 queries its files
    let snap_a1 = s1.get_analysis("A.vue").unwrap().unwrap();
    let names_a1 = prop_names(&snap_a1);
    assert!(
        names_a1.contains(&"a_modified".to_string()),
        "s1 should see its overlay on A, got: {:?}",
        names_a1
    );
    let snap_b1 = s1.get_analysis("B.vue").unwrap().unwrap();
    let names_b1 = prop_names(&snap_b1);
    assert!(
        names_b1.contains(&"b".to_string()),
        "s1 should see base B (not s2's overlay), got: {:?}",
        names_b1
    );

    // Session 2 queries its files
    let snap_b2 = s2.get_analysis("B.vue").unwrap().unwrap();
    let names_b2 = prop_names(&snap_b2);
    assert!(
        names_b2.contains(&"b_modified".to_string()),
        "s2 should see its overlay on B, got: {:?}",
        names_b2
    );
    let snap_a2 = s2.get_analysis("A.vue").unwrap().unwrap();
    let names_a2 = prop_names(&snap_a2);
    assert!(
        names_a2.contains(&"a".to_string()),
        "s2 should see base A (not s1's overlay), got: {:?}",
        names_a2
    );
}

/// Regression lock (fallthrough-only-partial no-poison): a fallthrough-only
/// budget partial must NOT warm the OUTER `ComponentMetaResultDb`. The owner's
/// resolve completes cleanly
/// (`synthesis_should_suppress == false`) — the ONLY partial comes from the
/// fallthrough spread walker tripping the low projection budget AFTER `resolved`
/// was produced. Without the fix the publish gate consults only
/// `resolved.synthesis_should_suppress` (false), so it warms the partial and a
/// replay warm-hits — RED. Post-fix the carrier merges the fallthrough
/// completeness into the gate, refusing admission — GREEN. The runtime node
/// cache and the legacy mirror also stay empty for the partial.
/// PUBLIC BOUNDARY — a props type derived from a helper whose return has
/// ONE unmodelled member publishes the MODELLED members, marks the
/// unmodelled one, reports partial, and warms NOTHING.
///
/// `defineProps<ReturnType<typeof makeProps>>()` over
/// `{ label: "x", made: notDeclared() }`. `notDeclared` is declared
/// nowhere, so the call is TS2304 and the checker types it with its error
/// type — recovery for a program that does not type-check, which the
/// flow-return lane does not model. But `label` is fully known, and a props
/// surface is a COMPOSITE: routing one unmodelled member to a whole-frame
/// failure published `[]`, complete and warm, where the checker publishes
/// `{ label: string; made: any }`. A wrong value at a public boundary,
/// entering `ComponentMetaResultDb`.
///
/// Four independent assertions, because three of them pass on the wrong
/// implementation:
///
///  1. `label` IS published (fails on the collapse);
///  2. `made` IS published and is NOT a usable type — it is the typed
///     unresolved marker, never a fabricated `any` (a fabricated `any` is
///     indistinguishable from an authored one at every downstream gate,
///     so this is what discriminates "marked" from "silently guessed");
///  3. the result is reported PARTIAL;
///  4. nothing warms `ComponentMetaResultDb` — a replay is cold.
///
/// Oracle (TypeScript 7.0.2 `tsc`, `--noEmit --strict
/// --ignoreConfig`): `ReturnType<typeof makeProps>` is
/// `{ label: string; made: any }`, with TS2304 at the call.
///
/// Mutation recipe: restoring the whole-frame `Err` for a positional
/// non-modelling drops BOTH props and assertion 1 fails (verbatim: "the
/// modelled sibling `label` MUST be published … got []" — which is B-F1
/// reproduced at the boundary); publishing a fabricated `any` for `made`
/// leaves 1/3/4 green and fails 2.
///
/// Assertions 3 and 4 are NOT gated by the consumer-entry cache-read fold
/// on this fixture — the unresolved marker suppresses admission through
/// the `FlowReturn` query's own `cache_suppress` regardless. The fold's
/// discriminator is
/// [`a_degraded_success_with_a_usable_value_still_gates_the_enclosing_result`],
/// where the degraded value carries no marker at all.
/// PUBLIC BOUNDARY — a DEGRADED SUCCESS whose value is fully usable still
/// gates the ENCLOSING result: partial, and nothing warms.
///
/// This is the arm the marker cannot cover. `makeProps` logically assigns
/// to its own parameter before returning — a plain `=` or compound write
/// at statement position is APPLIED by the evaluator and stays clean, so
/// the degradation fixture rides the operator form nobody applies —
/// so the evaluation carries the typed `UnappliedWriteEffect` degradation
/// — but its VALUE is a perfectly ordinary `{ label: string }` with no
/// miss carrier anywhere in it. So every downstream "is this value
/// known" test passes, the props surface publishes `label: string`, and
/// the ONLY thing that says the answer is not complete is the
/// degradation channel itself.
///
/// The `FlowReturn` build itself carries that fact outward: the
/// finalizer-outcome adapter translates the degraded verdict once into
/// the build's own partial rails, and the universal read funnel folds
/// them into every enclosing composition at the read boundary. Without
/// that propagation the enclosing composition reports COMPLETE and WARMS
/// around a degraded interior — the defect this row exists to
/// discriminate.
///
/// Mutation recipe: clearing the partial rails on the degraded arm of
/// `build_flow_return` leaves the whole rest of the suite green and
/// fails exactly this test — first on `synthesis_should_suppress`, and
/// on the warm replay if that assertion is removed.
#[cfg(not(target_arch = "wasm32"))]
#[test]
fn a_degraded_success_with_a_usable_value_still_gates_the_enclosing_result() {
    use std::sync::atomic::Ordering::Relaxed;

    let project = make_project();
    project
        .upsert_base(
            "/src/C2.vue",
            r#"<script setup lang="ts">
function makeProps(seed: string) {
  while ((seed = "y")) { break; }
  return { label: seed }
}
defineProps<ReturnType<typeof makeProps>>()
</script>
<template><div /></template>"#,
        )
        .unwrap();

    let host = project.host();
    let canonical = "/src/C2.vue";
    let meta = get_meta(&project, canonical);

    // The value is USABLE and fully known: `label` publishes normally and
    // carries NO marker. That is what makes this row discriminating — the
    // degradation is the only signal there is.
    let label = meta
        .props
        .iter()
        .find(|prop| prop.name == "label")
        .expect("the `label` prop");
    let label_type = demand_published_type(
        host,
        canonical,
        label.publication.result().selected_source(),
        "label",
    );
    assert!(
        matches!(&label_type, TypeExpr::Primitive(PrimitiveName::String)),
        "the degraded evaluation's VALUE is ordinary and fully known; got {label_type:?}"
    );

    // The enclosing result is nevertheless PARTIAL, and warms nothing.
    let (_, resolved) = host
        .get_component_meta_with_resolution(canonical)
        .expect("the resolve must still return metadata");
    assert!(
        resolved.synthesis_should_suppress,
        "a degraded flow-return interior makes the ENCLOSING result partial — the sealed \
         consumer entry's cache-read fold is the only thing that carries that fact outward"
    );

    let hits_before = host
        .provenance()
        .component_meta_result_cache_hits
        .load(Relaxed);
    let _ = get_meta(&project, canonical);
    let hits_after = host
        .provenance()
        .component_meta_result_cache_hits
        .load(Relaxed);
    assert_eq!(
        hits_after, hits_before,
        "a result composed around a degraded interior MUST NOT warm \
         `ComponentMetaResultDb` (hits_before={hits_before}, hits_after={hits_after})"
    );
}

/// #4-CLOSE — the full-extract `ColdComputeCompletenessScope` captures a
/// pre-choke macro-DTO partial that the resolve phase did NOT, feeding the merged
/// admission signal so the result is refused warm promotion.
///
/// `extract_component_meta_from_resolved` reads the macro-DTO surface
/// (`component_meta_resolved_macros` → `vue_macro_dtos_with_ctx`) BEFORE the
/// fallthrough cold compute. A budget-tripped DTO is returned partial and REFUSED
/// its own `vue_surface_store` (the sibling
/// `session_pre_choke_macro_dto_budget_partial_not_admitted_to_vue_surface_store`
/// pins that refusal). In production this macro-DTO partial is COUPLING-PREVENTED:
/// in a single cold request the resolve-phase props projector (`define_shapes` →
/// `vue_macro_dtos_with_ctx` → `observe_partial`) ALSO reads the DTO, so a
/// budget-tripped DTO already folds into `resolved.completeness`, which the former
/// `synthesis_should_suppress` gate term already caught — so it is NOT a reachable
/// pre-fix production poison hole. This test ARTIFICIALLY DECOUPLES the macro-DTO
/// to exercise the extract-scope operand in isolation (defense-by-construction of
/// the convergent gate, not proof of a reachable pre-fix hole); the
/// genuinely-reachable operand divergence the merge closes is a fallthrough-only
/// extract partial (extract scope) versus a synthesis-suppress resolve partial
/// (`resolved.completeness`). The fix enters ONE
/// full-extract scope spanning the macro-DTO read and gates on the single
/// `final_completeness = resolved.completeness.merge(extract_scope_completeness)`.
///
/// DECOUPLING the macro-DTO partial from `resolved.completeness` is what makes the
/// extract-scope term observable in isolation: in a single cold request the
/// RESOLVE-phase props projector (`define_shapes` → `vue_macro_dtos_with_ctx` →
/// `observe_partial`) ALSO reads the DTO, so a budget-tripped DTO folds into
/// `resolved.completeness` too. Here the macro-DTO is warmed by a GENEROUS resolve
/// (so `resolved.completeness == Complete`); then the imported helper is EDITED so
/// the DTO's validated `vue_surface_store` entry fails re-validation; then the
/// extract re-resolves the DTO COLD under a pre-exhausted budget. The fallthrough
/// is skipped (`include_fallthrough = false`) to remove the only other
/// extract-phase partiality source. Now `resolved.completeness` is Complete and the
/// ONLY partiality lives in the full-extract scope — so a partial `extract`
/// completeness proves the scope captured the macro-DTO, exactly the source the
/// pre-fix fallthrough-only carrier missed.
///
/// RED proof: in `extract_component_meta_from_resolved`, move
/// `ColdComputeCompletenessScope::enter()` to AFTER the macro-DTO read (so the
/// macro-DTO partial escapes the captured scope) → `extract.completeness` is
/// Complete → `final_completeness` is Complete → both assertions FAIL.
///
/// Scope: the merged gate protects the FINAL `ComponentMetaResultDb` + payload
/// admission; the intermediate resolved-meta scalar-lane cache is a SEPARATE
/// pre-existing latent poison bug, tracked as `RESOLVED_META_SCALAR_NO_POISON`.
///
/// Discrimination robustness: the RED proof relies on the extract AFTER the
/// macro-DTO read charging ZERO additional budget under the tripped / prop-less /
/// `include_fallthrough = false` setup (reasoned, not guarded by an assertion), so
/// the captured partiality is attributable to the pre-choke macro-DTO read alone.
#[cfg(not(target_arch = "wasm32"))]
#[test]
fn extract_scope_captures_cold_macro_dto_partial_into_merged_gate_signal() {
    use std::sync::Arc;
    let canonical = "/src/App.vue";

    // (1) GENEROUS resolve → `resolved` COMPLETE, and the resolve warms the
    // macro-DTO's host-global type memo.
    let project = make_project_with_config(HostConfig {
        analysis_level: crate::types::AnalysisLevel::Full,
        projection_op_budget: 0, // generous (effective 2000)
        ..HostConfig::default()
    });
    upsert_macro_dto_budget_owner(&project);
    let host = project.host();
    let resolved = host
        .resolve_component_meta(
            canonical,
            verter_type_engine::semantic_query::ProjectionMode::Expanded,
        )
        .expect("the generous resolve produces a complete resolved state");
    assert!(
        !resolved.synthesis_should_suppress && !resolved.completeness.is_partial(),
        "the generous resolve must be COMPLETE so the macro-DTO partial cannot ride \
         `resolved.completeness` — only the extract scope can carry it"
    );

    // (2) Edit the imported helper → its content hash changes, so the macro-DTO's
    // validated `vue_surface_store` entry FAILS re-validation and the extract must
    // re-resolve the DTO COLD. The owned `resolved` value is retained (it carries
    // the owner's snapshot/macros), and `resolved.completeness` stays the COMPLETE
    // value the generous resolve produced.
    {
        use std::fmt::Write as _;
        let mut helper = String::from("// invalidated\n");
        for n in 1..=32u32 {
            let _ = writeln!(
                helper,
                "export interface S{n:02} {{ a{n:02}: string; b{n:02}: number; c{n:02}: boolean }}"
            );
        }
        project.upsert_base("/src/dto_helper.ts", &helper).unwrap();
    }

    // (3) Extract WITHOUT fallthrough (`include_fallthrough = false`) under a
    // PRE-EXHAUSTED budget. Skipping the fallthrough removes the only other
    // extract-phase partiality source (so the scope's partiality is attributable
    // to the pre-choke macro-DTO read alone), and the policy over the tripped
    // (prop-less) meta does no budget-charged work. The cold macro-DTO re-resolves,
    // trips the fuse on its first charge, returns partial, and folds into the
    // full-extract scope.
    let extract = {
        let ctx = verter_type_engine::request_context::RequestContext::with_kind_timing_and_projection_budget(
            host.next_request_id(),
            Arc::<str>::from(canonical),
            verter_audit::RequestKind::ComponentMeta,
            false,
            false,
            None,
            1,
        );
        // Pre-exhaust: drive the counter strictly past the cap so the very next
        // `check_projection_op_count` (the cold macro-DTO's first charge) trips.
        ctx.projection_budget.check_projection_op_count();
        ctx.projection_budget.check_projection_op_count();
        let _guard = verter_type_engine::request_context::RequestContextGuard::install(ctx);
        crate::resolver_core::with_bare_host_ctx_for_test(host, |rc| {
            let fixture_dispatch_0 =
                verter_type_engine::project_semantic_dispatch::ProjectSemanticDispatch::new(rc);
            crate::host_manage::extract_component_meta_from_resolved(
                host,
                canonical,
                &resolved,
                false,
                rc,
                &fixture_dispatch_0,
            )
        })
    };

    // THE #4 DISCRIMINATOR: `resolved` is COMPLETE and the fallthrough is skipped,
    // so the macro-DTO partiality can reach the merged signal ONLY through the
    // full-extract scope spanning the pre-choke macro-DTO read. A partial
    // `extract.completeness` proves the scope captured it; the merged
    // `final_completeness` is therefore partial, which is EXACTLY the publish gate's
    // refusal condition (`if final_completeness.is_partial() { skip }`).
    assert!(
        extract.completeness.is_partial(),
        "the full-extract scope MUST capture the cold pre-choke macro-DTO partial (RED: with the \
         scope entered AFTER the macro-DTO read, `extract.completeness` stays Complete — the \
         pre-fix fallthrough-only carrier never spanned the macro-DTO read)"
    );
    let final_completeness = resolved.completeness.merge(extract.completeness);
    assert!(
        final_completeness.is_partial(),
        "the merged gate signal is partial SOLELY because the extract scope captured the macro DTO \
         (`resolved.completeness` is Complete) → the publish gate refuses warm admission"
    );
}

/// A `v-bind` spread of a member read on a call's result — directly, and
/// through a member call on the result of the call before it — consumes the
/// member's attributes: the spread's value is the member of the call's value.
#[cfg(not(target_arch = "wasm32"))]
#[test]
fn a_spread_of_a_member_of_a_call_result_consumes_its_attributes() {
    for spread in ["make().attrs", "make().next().attrs"] {
        let project = make_project();
        project
            .upsert_base(
                "/App.vue",
                &format!(
                    r#"<script setup lang="ts">
interface Made {{ attrs: {{ id: string; title: string }}; next(): Made }}
declare function make(): Made
</script>
<template><div v-bind="{spread}" /></template>"#
                ),
            )
            .unwrap();
        let surface = project
            .host()
            .resolve_fallthrough_surface("/App.vue")
            .expect("the fallthrough surface resolves");
        let names: Vec<&str> = surface
            .accepted_props
            .iter()
            .map(|prop| prop.name.as_ref())
            .collect();
        assert!(
            names.contains(&"placeholder"),
            "{spread}: the remaining div attributes stay: {names:?}"
        );
        assert!(
            !names.contains(&"id") && !names.contains(&"title"),
            "{spread}: the spread consumes its attributes: {names:?}"
        );
    }
}

#[test]
fn evaluate_types_only_expands_surface_requested_bindings() {
    let project = make_project();
    project
        .upsert_base(
            "/types.ts",
            r#"export interface HiddenPayload {
  deep: string
}"#,
        )
        .unwrap();
    project
        .upsert_base(
            "/App.vue",
            r#"<script setup lang="ts">
import type { HiddenPayload } from './types'

const hidden: HiddenPayload = { deep: 'x' }
const shown: number = 1

defineProps<{ label: string }>()
defineExpose({ shown })
</script>
<template><div /></template>"#,
        )
        .unwrap();

    let evaluated = project
        .host()
        .evaluate_types("/App.vue")
        .expect("evaluated types should exist");

    let binding_names: Vec<&str> = evaluated
        .bindings
        .iter()
        .map(|binding| binding.name.as_str())
        .collect();

    assert_eq!(
        binding_names,
        vec!["shown"],
        "only bindings requested by the component surface should be expanded"
    );
}

#[test]
fn evaluate_types_cold_path_does_not_call_public_get_analysis_workflow() {
    let project = make_project();
    project
        .upsert_base(
            "/types.ts",
            r#"export interface Props { a: string; b: number }"#,
        )
        .unwrap();
    project
        .upsert_base(
            "/App.vue",
            r#"<script setup lang="ts">
import { Props } from './types'
defineProps<Props>()
</script>
<template><div /></template>"#,
        )
        .unwrap();

    project.host().provenance().reset();
    let session = project.open_session_batch().unwrap();

    let _ = session
        .evaluate_types("/App.vue")
        .expect("evaluate_types should succeed on a cold path");

    let p = provenance(&project);
    assert_eq!(
        p.get_analysis_calls, 0,
        "evaluate_types should use the private resolved-state helper instead of the public get_analysis workflow",
    );
}

#[test]
fn evaluate_types_works_independently_of_prior_get_analysis_call() {
    let project = make_project();
    project
        .upsert_base("/App.vue", &sfc("count: number; label: string"))
        .unwrap();

    let session = project.open_session_batch().unwrap();

    // Call get_analysis first (raw, no enrichment)
    let analysis = session
        .get_analysis("/App.vue")
        .unwrap()
        .expect("get_analysis should return raw analysis");

    // get_analysis returns raw props
    let raw_names = prop_names(&analysis);
    assert!(
        raw_names.contains(&"count".to_string()),
        "raw analysis should have 'count' prop"
    );

    // evaluate_types should still work correctly regardless of prior get_analysis
    let evaluated = session
        .evaluate_types("/App.vue")
        .expect("evaluate_types should succeed")
        .expect("should return evaluated types");

    // Assert+: types are properly resolved
    assert_eq!(
        evaluated_prop_type(&project, "/App.vue", &evaluated, "count"),
        TypeExpr::Primitive(PrimitiveName::Number),
    );
    assert_eq!(
        evaluated_prop_type(&project, "/App.vue", &evaluated, "label"),
        TypeExpr::Primitive(PrimitiveName::String),
    );

    // Assert-: only the expected props
    assert_eq!(evaluated.props.len(), 2);
}

#[test]
fn evaluate_types_returns_consistent_results_for_repeated_calls() {
    let project = make_project();
    project
        .upsert_base("/App.vue", &sfc("a: string; b: number"))
        .unwrap();

    let session = project.open_session_batch().unwrap();

    // First call
    let first = session
        .evaluate_types("/App.vue")
        .expect("first evaluate_types should succeed")
        .expect("should return evaluated types");

    // Second call — should return identical results
    let second = session
        .evaluate_types("/App.vue")
        .expect("second evaluate_types should succeed")
        .expect("should return evaluated types");

    // Assert+: both calls return the same prop count and types
    assert_eq!(
        first.props.len(),
        second.props.len(),
        "repeated evaluate_types calls should return the same number of props"
    );
    let published_source_of = |types: &ExpandedComponentTypes, name: &str| {
        types
            .props
            .iter()
            .find(|field| field.name == name)
            .unwrap_or_else(|| panic!("missing evaluated prop {name}"))
            .authority
            .source_position()
            .clone()
    };
    assert_eq!(
        published_source_of(&first, "a"),
        published_source_of(&second, "a"),
        "repeated calls should return the same type for prop 'a'"
    );

    // Assert-: no extra props introduced
    assert_eq!(first.props.len(), 2, "should have exactly 2 props");
}

#[cfg(target_arch = "wasm32")]
#[test]
fn non_scheduler_upsert_reflects_updated_source_in_subsequent_analysis() {
    let project = make_project();
    project
        .upsert_base("/App.vue", &sfc("msg: string"))
        .unwrap();

    let session = project.open_session_batch().unwrap();
    let before = session
        .get_analysis("/App.vue")
        .unwrap()
        .expect("analysis should exist before upsert");
    let before_names = prop_names(&before);
    assert!(
        before_names.contains(&"msg".to_string()),
        "should see 'msg' before upsert"
    );

    let updated = sfc("msg: string; count: number");
    let _ = project
        .host()
        .upsert(crate::types::UpsertRequest {
            canonical_id: Some("/App.vue".to_string()),
            input_id: "/App.vue".to_string(),
            source: Arc::from(updated.as_str()),
            file_language: crate::LanguageRegistry::global()
                .classify_static("/App.vue")
                .static_resolution(),
            aliases: Vec::new(),
        })
        .unwrap();

    // Assert+: subsequent analysis reflects updated content
    let after = session
        .get_analysis("/App.vue")
        .unwrap()
        .expect("analysis should work after upsert");
    let after_names = prop_names(&after);
    assert!(
        after_names.contains(&"count".to_string()),
        "should see 'count' after upsert, got: {:?}",
        after_names
    );

    // Assert-: should not lose the original prop
    assert!(
        after_names.contains(&"msg".to_string()),
        "should still see 'msg' after upsert"
    );
}

#[test]
fn project_local_intrinsics_load_from_vue_type_entrypoints() {
    let ws = Arc::new(verter_workspace::MemoryWorkspace::new(
        verter_workspace::MemoryOptions::default(),
    ));
    ws.inject_file(
        "/workspace/node_modules/vue/package.json".to_string(),
        Arc::from(
            r#"{
  "name": "vue",
  "types": "./index.d.ts",
  "exports": {
    ".": { "types": "./index.d.ts", "import": "./index.js" },
    "./jsx": { "types": "./jsx.d.ts", "import": "./jsx.js" }
  }
}"#,
        ),
    );
    ws.inject_file(
        "/workspace/node_modules/vue/index.d.ts".to_string(),
        Arc::from(
            r#"export interface HTMLAttributes {
  fallbackOnly?: string
  onProjectClick?: ProjectClickEvent
}

export interface ProjectClickEvent {
  source: 'project'
}"#,
        ),
    );
    ws.inject_file(
        "/workspace/node_modules/vue/jsx.d.ts".to_string(),
        Arc::from(
            r#"import type { NativeElements } from "./jsx-runtime"

export namespace JSX {
  export interface IntrinsicElements extends NativeElements {}
}"#,
        ),
    );
    ws.inject_file(
        "/workspace/node_modules/vue/jsx-runtime.d.ts".to_string(),
        Arc::from(
            r#"import type { HTMLAttributes } from "./index"

export interface NativeElements {
  div: HTMLAttributes & { projectOnly?: string }
}"#,
        ),
    );

    let host = VerterHost::new(
        HostConfig {
            analysis_level: crate::types::AnalysisLevel::Full,
            ..HostConfig::default()
        },
        ws,
    );
    host.configure_projects(vec![verter_workspace::ide_project_config(
        "/workspace".to_string(),
        "/workspace".to_string(),
        Some("/workspace/tsconfig.json".to_string()),
    )]);
    let project = MetaProject::new(host);
    project
        .upsert_base("/workspace/src/App.vue", r#"<template><div /></template>"#)
        .unwrap();

    let meta = get_meta(&project, "/workspace/src/App.vue");

    assert!(
        meta.accepted_props
            .iter()
            .any(|prop| prop.name == "projectOnly"),
        "native intrinsics loading should surface tag-specific members from vue/jsx"
    );
    assert!(
        meta.accepted_props
            .iter()
            .any(|prop| prop.name == "fallbackOnly"),
        "native intrinsics loading should surface fallback HTMLAttributes members from vue"
    );
    assert!(
        meta.accepted_events
            .iter()
            .any(|event| event.name == "projectClick"),
        "native intrinsics loading should expose listeners derived from the project-local HTMLAttributes surface"
    );
    assert!(
        !meta.accepted_props.iter().any(|prop| prop.name == "id"),
        "project-local intrinsic surfaces should replace the generated built-in tag surface when vue entrypoints resolve"
    );
}

#[test]
fn project_local_intrinsics_tag_members_value_intersect_conflicting_fallback() {
    let ws = Arc::new(verter_workspace::MemoryWorkspace::new(
        verter_workspace::MemoryOptions::default(),
    ));
    ws.inject_file(
        "/workspace/node_modules/vue/package.json".to_string(),
        Arc::from(
            r#"{
  "name": "vue",
  "types": "./index.d.ts",
  "exports": {
    ".": { "types": "./index.d.ts", "import": "./index.js" },
    "./jsx": { "types": "./jsx.d.ts", "import": "./jsx.js" }
  }
}"#,
        ),
    );
    ws.inject_file(
        "/workspace/node_modules/vue/index.d.ts".to_string(),
        Arc::from(
            r#"export interface HTMLAttributes {
  projectOnly?: number
  onClick?: (payload: FallbackClickEvent) => void
}

export interface FallbackClickEvent {
  source: 'fallback'
}"#,
        ),
    );
    ws.inject_file(
        "/workspace/node_modules/vue/jsx.d.ts".to_string(),
        Arc::from(
            r#"import type { NativeElements } from "./jsx-runtime"

export namespace JSX {
  export interface IntrinsicElements extends NativeElements {}
}"#,
        ),
    );
    ws.inject_file(
        "/workspace/node_modules/vue/jsx-runtime.d.ts".to_string(),
        Arc::from(
            r#"import type { HTMLAttributes } from "./index"

export interface NativeElements {
  div: HTMLAttributes & {
    projectOnly?: string
    onClick?: (payload: ProjectClickEvent) => void
  }
}

export interface ProjectClickEvent {
  source: 'project'
}"#,
        ),
    );

    let host = VerterHost::new(
        HostConfig {
            analysis_level: crate::types::AnalysisLevel::Full,
            ..HostConfig::default()
        },
        ws,
    );
    host.configure_projects(vec![verter_workspace::ide_project_config(
        "/workspace".to_string(),
        "/workspace".to_string(),
        Some("/workspace/tsconfig.json".to_string()),
    )]);
    let project = MetaProject::new(host);
    project
        .upsert_base("/workspace/src/App.vue", r#"<template><div /></template>"#)
        .unwrap();

    let meta = get_meta(&project, "/workspace/src/App.vue");

    // `div = HTMLAttributes & { projectOnly?: string; onClick?: (payload:
    // ProjectClickEvent) => void }` over `HTMLAttributes.projectOnly?: number`
    // and `HTMLAttributes.onClick?: (payload: FallbackClickEvent) => void`. An
    // anonymous `A & B` object intersection VALUE-INTERSECTS conflicting
    // same-named members — `projectOnly` is `number & string`, NOT the last-arm
    // override `string`. This is the TS-correct merge (the same authored-`&`
    // semantics `authored_intersection_duplicate_does_not_shadow` pins) and is
    // distinct from interface heritage, which DOES shadow.

    let project_only = meta
        .accepted_props
        .iter()
        .find(|prop| prop.name == "projectOnly")
        .expect("project-local tag members must still be present");
    let project_only_ty = demand_published_type(
        project.host(),
        "/workspace/src/App.vue",
        project_only.publication.result().selected_source(),
        "projectOnly accepted prop",
    );
    // POSITIVE: the conflicting member is the value-intersection of both
    // arms; `number & string` is PROVABLY disjoint at tag level, so the
    // canonical intersection reduces it to `never` (checker-confirmed:
    // `IsNever<number & string>` is `true`) — the value-intersect rule
    // applied, never a last-arm override.
    assert!(
        matches!(&project_only_ty, TypeExpr::Primitive(PrimitiveName::Never)),
        "projectOnly must value-intersect the conflicting fallback (`number & \
         string` reduces to `never`), not override; got: {project_only_ty:?}"
    );
    // NEGATIVE: it must NOT have collapsed to the old last-arm override `string`.
    assert!(
        !matches!(project_only_ty, TypeExpr::Primitive(PrimitiveName::String)),
        "projectOnly must NOT collapse to the last-arm override `string` — that \
         was the bug; got: {project_only_ty:?}"
    );

    // The conflicting listener `onClick` is likewise the intersection of the two
    // handler types — NOT the last-arm override. Its presence on the accepted
    // event surface is preserved; its payload is the value-intersection.
    let click = meta
        .accepted_events
        .iter()
        .find(|event| event.name == "click")
        .expect("tag-specific listeners must still appear on the accepted event surface");
    let click_payload_ty = demand_published_type(
        project.host(),
        "/workspace/src/App.vue",
        click.payload.present(),
        "click accepted event payload",
    );
    // POSITIVE: the listener payload intersects both handlers' parameter types.
    let payload_intersects = matches!(&click_payload_ty, TypeExpr::Intersection(arms)
        if arms.iter().any(|arm| payload_param_references(arm, "FallbackClickEvent"))
            && arms.iter().any(|arm| payload_param_references(arm, "ProjectClickEvent")));
    assert!(
        payload_intersects,
        "click listener payload must value-intersect both handler types \
         (fallback + project), not override; got: {click_payload_ty:?}"
    );
    // NEGATIVE: it must NOT be a single function overriding to the project-only
    // handler (the old override bug).
    assert!(
        !matches!(
            &click_payload_ty,
            TypeExpr::Function(function)
                if function.parameters.len() == 1
                    && payload_ty_references(&function.parameters[0].ty, "ProjectClickEvent")
        ),
        "click listener payload must NOT be the last-arm override handler \
         (`(payload: ProjectClickEvent) => void`) alone; got: {click_payload_ty:?}"
    );
}

#[test]
fn cycle_terminates_without_invented_members() {
    let project = make_project();

    // A imports B, B imports A — create a cycle
    project
        .upsert_base(
            "/A.vue",
            r#"<script setup lang="ts">
import B from './B.vue'
defineProps<{ aProp: string }>()
</script>
<template><B /></template>"#,
        )
        .unwrap();

    project
        .upsert_base(
            "/B.vue",
            r#"<script setup lang="ts">
import A from './A.vue'
defineProps<{ bProp: string }>()
</script>
<template><A /></template>"#,
        )
        .unwrap();

    // Should not panic or infinite loop
    let meta = get_meta(&project, "/A.vue");

    // Assert+: declared props are present
    assert!(
        meta.accepted_props.iter().any(|p| p.name == "aProp"),
        "should have declared 'aProp'"
    );

    // Assert+: surface completeness should be LowerBound due to cycle
    assert_eq!(
        meta.accepted_surface_completeness,
        AcceptedSurfaceCompleteness::LowerBound,
        "cycle should produce LowerBound completeness"
    );

    // Assert-: no invented members from the cycle
    assert!(
        !meta.accepted_props.iter().any(|p| p.name == "bProp"),
        "should NOT inherit 'bProp' through a cycle"
    );
}

#[test]
fn accepted_surface_member_order_is_deterministic() {
    let project = make_project();
    project
        .upsert_base(
            "/App.vue",
            r#"<script setup lang="ts">
defineProps<{ z: string; a: number }>()
</script>
<template><div>test</div></template>"#,
        )
        .unwrap();

    let meta = get_meta(&project, "/App.vue");

    // Assert+: declared props come first in declared source order
    let declared_props: Vec<&str> = meta
        .accepted_props
        .iter()
        .filter(|p| matches!(p.provenance, MemberProvenance::Declared))
        .map(|p| p.name.as_str())
        .collect();
    assert_eq!(
        declared_props,
        vec!["z", "a"],
        "declared props should keep source order"
    );

    // Assert+: inherited props come after declared, sorted lexicographically
    let inherited_props: Vec<&str> = meta
        .accepted_props
        .iter()
        .filter(|p| matches!(p.provenance, MemberProvenance::Inherited { .. }))
        .map(|p| p.name.as_str())
        .collect();
    let mut sorted = inherited_props.clone();
    sorted.sort();
    assert_eq!(
        inherited_props, sorted,
        "inherited props should be sorted lexicographically"
    );
}

#[test]
fn type_reachable_count_zero_falls_back_to_all_sources() {
    // Component with inline defineProps (no macro_type_deps) should still
    // resolve locally without any cross-file imported-eval work.
    let project = make_project();
    let session = project.open_session_batch().unwrap();

    session
        .upsert(
            "/src/App.vue",
            r#"<script setup lang="ts">
defineProps<{ msg: string }>()
</script>
<template><div>{{ msg }}</div></template>"#
                .to_string(),
        )
        .unwrap();

    let meta = session
        .get_component_meta("/src/App.vue")
        .unwrap()
        .expect("should get component meta");

    // Type eval should still work with inline types
    assert_eq!(meta.props.len(), 1, "should resolve inline prop");
    assert_eq!(meta.props[0].name, "msg");
}

#[test]
fn depth_limit_does_not_hang_on_extreme_chain() {
    // Create a chain of 40 barrel files, each re-exporting from the next.
    // Verifies the resolver terminates on long chains without stack overflow.
    // (135 caused stack overflow in tests; 40 is safe and still exercises the chain.)
    let project = make_project();

    for i in 0..40 {
        let source = format!("export * from './barrel_{}'", i + 1);
        project
            .upsert_base(&format!("/src/barrel_{i}.ts"), &source)
            .unwrap();
        project.host().set_import_dependencies(
            &format!("/src/barrel_{i}.ts"),
            vec![crate::types::DependencyResolution {
                specifier: format!("./barrel_{}", i + 1),
                resolved_canonical_id: Some(format!("/src/barrel_{}.ts", i + 1)),
                possible_canonical_ids: Vec::new(),
            }],
        );
    }
    // Terminal file
    project
        .upsert_base(
            "/src/barrel_40.ts",
            r#"export interface FinalType { done: boolean }"#,
        )
        .unwrap();

    project
        .upsert_base(
            "/src/App.vue",
            r#"<script setup lang="ts">
import type { FinalType } from './barrel_0'
defineProps<FinalType>()
</script>
<template><div /></template>"#,
        )
        .unwrap();
    project.host().set_import_dependencies(
        "/src/App.vue",
        vec![crate::types::DependencyResolution {
            specifier: "./barrel_0".to_string(),
            resolved_canonical_id: Some("/src/barrel_0.ts".to_string()),
            possible_canonical_ids: Vec::new(),
        }],
    );

    let session = project.open_session_batch().unwrap();
    // Should complete without hanging — depth limit terminates the chain
    let meta = session
        .get_component_meta("/src/App.vue")
        .unwrap()
        .expect("get_component_meta should return a result");

    // The type won't be found (depth exceeded), but the call must not hang
    // It's OK if props is empty — the important thing is termination.
    assert!(
        meta.props.len() <= 1,
        "depth-limited chain should produce 0-1 props (not hang): {:?}",
        meta.props.iter().map(|p| &p.name).collect::<Vec<_>>()
    );
}

/// Counter-positive: the projector reduces a Pick<Foo, 'a'>
/// indexed-access chain — operator-shape inputs DO reduce, even
/// though bare alias references stay shallow.
///
/// `defineProps<{ k: Pick<Foo, 'a'>['a'] }>` where
/// `type Foo = { a: string; b: number }` lives in the same file
/// MUST publish `k` as the literal `Primitive(String)` — the
/// terminal hop's resolved value. The consumer explicitly walked
/// the path (`Pick<...>['a']` carries an `IndexedAccess` operator
/// node), so the projector reduces it.
///
/// Pairs with [`published_same_file_alias_stays_shallow`]: the
/// bare same-file reference stays as `Ref { "Foo" }` (alias names
/// are shallow), but a Pick/IndexedAccess chain that explicitly
/// walks `Foo`'s `'a'` key materialises that key. Together the two
/// pin the projector's contract: alias references stay shallow, but
/// explicit walks (operator-shape inputs) self-reduce path-precisely.
#[test]
fn projector_reduces_same_file_alias_via_pick_indexed_access() {
    let project = make_project();
    project
        .upsert_base(
            "/Comp.vue",
            r#"<script setup lang="ts">
type Foo = { a: string; b: number }

defineProps<{
  k: Pick<Foo, 'a'>['a']
}>()
</script>
<template><div /></template>"#,
        )
        .unwrap();

    let session = project.open_session_batch().unwrap();
    let evaluated = session.evaluate_types("/Comp.vue").unwrap().unwrap();

    let k_ty = evaluated_prop_type(&project, "/Comp.vue", &evaluated, "k");
    match &k_ty {
        TypeExpr::Primitive(PrimitiveName::String) => {}
        TypeExpr::Ref { name, .. } if name.as_ref() == "Foo" => panic!(
            "FAIL (architectural rule): Pick<Foo,'a'>['a'] must reduce to \
             the terminal `string` primitive — leaving it as a bare `Ref` \
             over `Foo` is the bare-alias preservation rule, which does \
             not apply to a structural Pick/IndexedAccess chain. Got {k_ty:?}"
        ),
        TypeExpr::IndexedAccess { .. } => panic!(
            "FAIL (architectural rule): Pick<Foo,'a'>['a'] must self-reduce \
             through the projector path; a symbolic IndexedAccess proves the \
             projector did not reduce the chain. Got {k_ty:?}"
        ),
        other => panic!(
            "FAIL: same-file Pick<Foo,'a'>['a'] must reduce to Primitive(String); \
             got {other:?}"
        ),
    }
}

/// Architectural rule: `Pick<Foo, "bar">` materialises ONLY `bar`.
///
/// The projector path resolves the indexed-access / utility chain
/// to the requested keys' value types. Other Foo properties (the
/// ones NOT picked) stay shallow — the consumer never observes them
/// through this surface.
#[test]
fn pick_materialises_only_named_keys_others_stay_shallow() {
    use verter_type_expr::ObjectMember;
    let project = make_project();
    project
        .upsert_base(
            "/types.ts",
            r#"export interface Foo {
  a: string,
  b: number,
  c: boolean,
  d: 'd'
}"#,
        )
        .unwrap();
    project
        .upsert_base(
            "/Comp.vue",
            r#"<script setup lang="ts">
import type { Foo } from './types'

defineProps<{
  picked: Pick<Foo, 'a' | 'b'>
}>()
</script>
<template><div /></template>"#,
        )
        .unwrap();

    let session = project.open_session_batch().unwrap();
    let evaluated = session.evaluate_types("/Comp.vue").unwrap().unwrap();
    let picked_ty = evaluated_prop_type(&project, "/Comp.vue", &evaluated, "picked");

    let TypeExpr::Object(obj) = &picked_ty else {
        panic!("Pick<Foo, 'a' | 'b'> must materialise to an Object surface, got {picked_ty:?}");
    };
    let names: Vec<&str> = obj
        .properties
        .iter()
        .filter_map(|m| match m {
            ObjectMember::Property(p) => Some(p.string_name().expect("string-key fixture")),
            _ => None,
        })
        .collect();

    // Picked keys are present.
    assert!(
        names.contains(&"a"),
        "Pick must include `a` (got {names:?})"
    );
    assert!(
        names.contains(&"b"),
        "Pick must include `b` (got {names:?})"
    );
    // Architectural rule: the picked surface MUST NOT include `c` or
    // `d` (they were not picked, so the consumer never observes
    // them through this surface).
    assert!(
        !names.contains(&"c"),
        "FAIL (architectural rule): picked surface must NOT include `c` \
         (got {names:?}) — Pick<Foo, 'a' | 'b'> is path-precise."
    );
    assert!(
        !names.contains(&"d"),
        "FAIL (architectural rule): picked surface must NOT include `d` \
         (got {names:?}) — Pick<Foo, 'a' | 'b'> is path-precise."
    );
}

/// Architectural rule: `Omit<Foo, "bar">` keeps `bar` shallow and
/// materialises the others.
///
/// Omit is the dual of Pick: the named key is EXCLUDED, all other
/// keys land on the surface. The excluded key never appears on the
/// surface so the consumer cannot observe it through this projection.
#[test]
fn omit_excludes_named_keys_others_materialise() {
    use verter_type_expr::ObjectMember;
    let project = make_project();
    project
        .upsert_base(
            "/types.ts",
            r#"export interface Foo {
  a: string,
  b: number,
  c: boolean
}"#,
        )
        .unwrap();
    project
        .upsert_base(
            "/Comp.vue",
            r#"<script setup lang="ts">
import type { Foo } from './types'

defineProps<{
  trimmed: Omit<Foo, 'b'>
}>()
</script>
<template><div /></template>"#,
        )
        .unwrap();

    let session = project.open_session_batch().unwrap();
    let evaluated = session.evaluate_types("/Comp.vue").unwrap().unwrap();
    let trimmed_ty = evaluated_prop_type(&project, "/Comp.vue", &evaluated, "trimmed");

    let TypeExpr::Object(obj) = &trimmed_ty else {
        panic!("Omit<Foo, 'b'> must materialise to an Object surface, got {trimmed_ty:?}");
    };
    let names: Vec<&str> = obj
        .properties
        .iter()
        .filter_map(|m| match m {
            ObjectMember::Property(p) => Some(p.string_name().expect("string-key fixture")),
            _ => None,
        })
        .collect();

    // Architectural rule: the omitted key MUST NOT be present.
    assert!(
        !names.contains(&"b"),
        "FAIL (architectural rule): omitted surface must NOT include `b` \
         (got {names:?}) — Omit<Foo, 'b'> excludes `b` and materialises \
         the others."
    );
    // The other keys land on the surface.
    assert!(
        names.contains(&"a"),
        "Omit<Foo, 'b'> must include `a` (got {names:?})"
    );
    assert!(
        names.contains(&"c"),
        "Omit<Foo, 'b'> must include `c` (got {names:?})"
    );
}

/// DISCRIMINATION: the live routed Pick/Omit surface owner
/// (`dispatch_routed_pick_omit_via_shared_engine_node`, registry_decl.rs) projects
/// in NODE DOMAIN — it routes the `Pick` / `Omit` route through the SHARED semantic
/// builtin engine (`builtin_type_slot` + the `Instantiate` query) and admits the
/// resulting NODE through the node-domain `admit_materialized` gate, NEVER by
/// materialising the whole object `TypeExpr` and calling `type_expr_to_object_shape`.
///
/// Discrimination: re-introducing a materialize-then-`type_expr_to_object_shape`
/// shape derivation on this live path FAILS the forbidden-call assertion; dropping
/// the shared-builtin-engine routing FAILS the required-call assertion; a
/// rename/removal of the owner FAILS the anti-vacuity `found` assertion.
#[test]
fn routed_pick_omit_projects_in_node_domain_via_shared_engine() {
    use std::collections::BTreeSet;
    use syn::visit::Visit;

    /// Collect every call ident (free/assoc-fn last path segment + method-call
    /// name) syntactically reachable inside the fn named `target` (a nested
    /// `fn` or an impl method), with depth tracking so sibling fns never leak.
    #[derive(Default)]
    struct CallCollector {
        target: String,
        depth: usize,
        found: bool,
        calls: BTreeSet<String>,
    }
    impl<'ast> Visit<'ast> for CallCollector {
        fn visit_item_fn(&mut self, f: &'ast syn::ItemFn) {
            let hit = f.sig.ident == self.target;
            if hit {
                self.found = true;
                self.depth += 1;
            }
            syn::visit::visit_item_fn(self, f);
            if hit {
                self.depth -= 1;
            }
        }
        fn visit_impl_item_fn(&mut self, f: &'ast syn::ImplItemFn) {
            let hit = f.sig.ident == self.target;
            if hit {
                self.found = true;
                self.depth += 1;
            }
            syn::visit::visit_impl_item_fn(self, f);
            if hit {
                self.depth -= 1;
            }
        }
        fn visit_expr_call(&mut self, c: &'ast syn::ExprCall) {
            if self.depth > 0 {
                if let syn::Expr::Path(p) = c.func.as_ref() {
                    if let Some(seg) = p.path.segments.last() {
                        self.calls.insert(seg.ident.to_string());
                    }
                }
            }
            syn::visit::visit_expr_call(self, c);
        }
        fn visit_expr_method_call(&mut self, m: &'ast syn::ExprMethodCall) {
            if self.depth > 0 {
                self.calls.insert(m.method.to_string());
            }
            syn::visit::visit_expr_method_call(self, m);
        }
    }

    fn collect_calls(src: &str, target: &str) -> BTreeSet<String> {
        let file = syn::parse_file(src).expect("source must parse");
        let mut collector = CallCollector {
            target: target.to_string(),
            ..Default::default()
        };
        collector.visit_file(&file);
        assert!(
            collector.found,
            "target fn `{target}` not found (renamed/removed?) — the \
             characterization must not vacuously pass"
        );
        collector.calls
    }

    const REGISTRY_DECL_SRC: &str =
        include_str!("../../resolver_core/component_meta_query_engine/registry_decl.rs");

    let calls = collect_calls(
        REGISTRY_DECL_SRC,
        "dispatch_routed_pick_omit_via_shared_engine_node",
    );
    // FORBIDDEN: a materialize-then-shape derivation on the live Pick/Omit path.
    assert!(
        !calls.contains("type_expr_to_object_shape"),
        "dispatch_routed_pick_omit_via_shared_engine_node must NOT call \
         `type_expr_to_object_shape` (materialize-then-shape on the live Pick/Omit path); \
         calls seen: {calls:?}"
    );
    // REQUIRED: route through the SHARED builtin engine + the node-domain
    // admission gate — never a materialised-value decide.
    for required in ["builtin_type_slot", "admit_materialized"] {
        assert!(
            calls.contains(required),
            "dispatch_routed_pick_omit_via_shared_engine_node must call `{required}` (shared \
             builtin engine routing / node-domain admission); calls seen: {calls:?}"
        );
    }
}

/// The dispatch macro-surface projection equals the surface the former
/// eager parser expander produced for the same fixture: prop set,
/// per-prop requiredness, and the emit payload all match the expander's
/// known-good output (hardcoded below from that engine's behaviour on
/// this exact fixture). If a second engine were still live and drifted
/// on any of these facts, this pin diverges.
#[test]
fn dispatch_macro_surface_matches_former_expander_output_on_real_fixture() {
    let project = make_project();
    project
        .upsert_base(
            "/shared.ts",
            r#"export interface Base { id: string }
export interface SharedProps extends Base {
  label?: string
  count: number
}
export interface SharedEmits {
  (e: 'save', id: number): void
  close: []
}"#,
        )
        .unwrap();
    project
        .upsert_base(
            "/App.vue",
            r#"<script setup lang="ts">
import type { SharedProps, SharedEmits } from './shared'
defineProps<SharedProps>()
defineEmits<SharedEmits>()
</script>
<template><div /></template>"#,
        )
        .unwrap();

    let meta = get_meta(&project, "/App.vue");

    // Former-expander known-good facts for this fixture: the heritage
    // closure flattens `Base.id` onto the surface; own-body members keep
    // their authored optionality.
    let mut props: Vec<(String, bool)> = meta
        .props
        .iter()
        .map(|p| (p.name.clone(), p.required))
        .collect();
    props.sort();
    assert_eq!(
        props,
        vec![
            ("count".to_string(), true),
            ("id".to_string(), true),
            ("label".to_string(), false),
        ],
        "dispatch projection must equal the former eager-expander surface \
         (heritage-flattened, authored optionality preserved)"
    );

    // Emits: both the call-signature form and the tuple shorthand form
    // surface with their event names — byte-identical to the former
    // expander's named-call-signature output for this fixture.
    let mut emit_names: Vec<&str> = meta.events.iter().map(|e| e.name.as_str()).collect();
    emit_names.sort();
    assert_eq!(
        emit_names,
        vec!["close", "save"],
        "dispatch emit projection must equal the former eager-expander emit set"
    );
}

/// CROSS-OWNER effective scope (invariant 16) — NESTED scope-relative
/// refs: a COMPOSITE inherited source containing a nested bare
/// `Ref("SharedAlias")` leaf (an anchor-FREE position
/// `absolutized_against` cannot pin) MUST raise under its PRODUCING
/// (child) scope. Child and parent both import the SAME alias spelling
/// resolving to DIFFERENT terminal declarations (renamed re-exports:
/// `ChildTerminal as SharedAlias` vs `ParentTerminal as SharedAlias`), so
/// a blind parent-scope raise resolves the nested ref to the WRONG
/// (parent) terminal declaration. Exercises BOTH inherited lane families:
/// the branch-structured fallthrough row and the flat accepted row.
///
/// Discriminating: with the envelope raising these lanes under the owner
/// scope instead of the row's positional producing scope, both lanes
/// materialize `ParentTerminal` and the child-identity asserts fail RED.
#[test]
fn cross_owner_nested_scope_relative_ref_raises_under_producing_scope() {
    use verter_session_query::analysis::component_meta as cm;
    use verter_type_expr::facts::{FactOrLocator, LeafTypeFact, ResolvedLocalShape};

    let project = make_project();
    project
        .upsert_base(
            "/child-types.ts",
            "type ChildTerminal = { tag: string }\nexport type { ChildTerminal as SharedAlias }\n",
        )
        .unwrap();
    project
        .upsert_base(
            "/parent-types.ts",
            "type ParentTerminal = { other: number }\nexport type { ParentTerminal as SharedAlias }\n",
        )
        .unwrap();
    // The child SFC imports `SharedAlias` from ITS types module — the
    // nested ref's spelling resolves in the CHILD's file scope.
    project
        .upsert_base(
            "/Child.vue",
            r#"<script setup lang="ts">
import type { SharedAlias } from './child-types'
defineProps<{ inheritedAlias: SharedAlias }>()
</script>
<template><div /></template>"#,
        )
        .unwrap();
    // The parent resolves the SAME spelling to a DIFFERENT terminal.
    project
        .upsert_base(
            "/Parent.vue",
            r#"<script setup lang="ts">
import type { SharedAlias } from './parent-types'
import Child from './Child.vue'
defineProps<{ own: SharedAlias }>()
</script>
<template><Child /></template>"#,
        )
        .unwrap();
    let host = project.host();

    // The composite inherited source: a synthesized object whose member
    // value is the nested bare `Ref("SharedAlias")` — the exact shape the
    // top-level closed-leaf fast path does NOT cover, so the envelope MUST
    // enter the raise path for it.
    let nested_composite =
        verter_type_expr::facts::SemanticTypeSource::Synthesized(ResolvedLocalShape::Object(
            Arc::from(vec![verter_type_expr::facts::SynthesizedMemberFact {
                name: "tag".to_string(),
                optional: false,
                ty: FactOrLocator::Leaf(LeafTypeFact::Ref("SharedAlias".to_string())),
                span_origin: verter_type_expr::span_origins::MemberSpansOrigin::Synthetic(
                    verter_type_expr::span_origins::SourceSynthetic,
                ),
            }]),
        ));

    let mut analysis = blank_output_analysis();
    analysis.file_path = "/Parent.vue".to_string();
    // Branch-structured fallthrough lane: the row carries its positional
    // PRODUCING scope (threaded by the clone boundary in production).
    analysis.fallthrough_surface = cm::FallthroughSurface::Branches {
        branches: vec![cm::FallthroughBranch {
            branch_key: "0".to_string(),
            condition_text: None,
            props: vec![cm::FallthroughPropEntry {
                name: "inherited".to_string(),
                callable_role: verter_type_expr::PropCallableRole::default(),
                publication: crate::test_only::type_publication_fixture(
                    verter_type_expr::facts::SourcePosition::Present(nested_composite.clone()),
                    verter_type_expr::ResolutionExactness::ExactConcrete,
                    None,
                    None,
                ),
                type_source_scope: Some("/Child.vue".to_string()),
                sources: vec![cm::InheritedSource::Component {
                    canonical_id: "/Child.vue".to_string(),
                }],
            }],
            events: Vec::new(),
            root_chain: Vec::new(),
            status: cm::BranchStatus::Resolved,
        }],
    };
    // Flat accepted lane: the merged row carries the SAME producing scope
    // (threaded by the cross-branch merge finalize in production).
    analysis.accepted_props.push(cm::AcceptedPropAnalysis {
        name: "inherited".to_string(),
        callable_role: verter_type_expr::PropCallableRole::default(),
        publication: crate::test_only::type_publication_fixture(
            verter_type_expr::facts::SourcePosition::Present(nested_composite),
            verter_type_expr::ResolutionExactness::ExactConcrete,
            None,
            None,
        ),
        type_source_scope: Some("/Child.vue".to_string()),
        required: false,
        provenance: cm::MemberProvenance::Inherited {
            sources: vec![cm::InheritedSource::Component {
                canonical_id: "/Child.vue".to_string(),
            }],
        },
        availability: cm::MemberAvailability::Always,
        kind: cm::AcceptedPropKind::Attr,
    });

    let fixture_dispatch_19 =
        verter_type_engine::project_semantic_dispatch::ProjectSemanticDispatch::new(host);
    let output = crate::meta_resolve::projectors::build_component_meta_output(
        host,
        &fixture_dispatch_19,
        "/Parent.vue",
        analysis,
        None,
        crate::meta_resolve::PublishedCompleteness::COMPLETE,
    )
    .expect("the nested composite inherited source materializes under its producing scope");
    let lanes = output.into_parts().2.into_lanes();

    let assert_child_identity = |materialized: &TypeExpr, lane: &str| {
        let TypeExpr::Object(object) = materialized else {
            panic!("{lane}: the composite source materializes an object; got {materialized:?}");
        };
        let tag = object
            .properties
            .iter()
            .find_map(|member| match member {
                ObjectMember::Property(property)
                    if property.string_name().expect("string-key fixture") == "tag" =>
                {
                    Some(&property.ty)
                }
                _ => None,
            })
            .expect("the nested member survives materialization");
        assert!(
            matches!(tag, TypeExpr::Ref { name, .. } if name.as_ref() == "ChildTerminal"),
            "{lane}: the nested scope-relative ref must resolve under the \
             PRODUCING (child) scope — `SharedAlias` names the CHILD's \
             terminal declaration `ChildTerminal`, never the parent's \
             `ParentTerminal` and never an unresolved parent-scope shell; \
             got {tag:?}"
        );
    };
    assert_child_identity(
        lanes.fallthrough_props[0][0]
            .materialized_type()
            .expect("published type"),
        "fallthrough lane",
    );
    assert_child_identity(
        lanes.accepted_props[0]
            .materialized_type()
            .expect("published type"),
        "flat accepted lane",
    );
}

/// SESSION-owned registry name-overlay finalize: resolved registry entries
/// REPLACE the first same-name analysis row IN PLACE (order preserved) and
/// APPEND new names — executed before materialization, so the registry lane
/// aligns with the MERGED registry.
#[test]
fn output_registry_overlay_finalize_replaces_in_place_and_appends() {
    let project = make_project();
    project
        .upsert_base("/App.vue", "<template><div /></template>")
        .unwrap();
    let host = project.host();

    let mut analysis = blank_output_analysis();
    for name in ["Alpha", "Beta"] {
        analysis.type_registry.push(
            verter_session_query::analysis::component_meta::ResolvedTypeAnalysis {
                name: name.to_string(),
                type_source: verter_type_expr::facts::SourcePosition::Present(closed_ref_source(
                    name,
                )),
                type_expansion: None,
            },
        );
    }
    let seed = crate::meta_resolve::output::ComponentMetaResolutionSeed {
        resolved_type_registry: vec![
            verter_session_query::analysis::component_meta::ResolvedTypeAnalysis {
                name: "Alpha".to_string(),
                type_source: verter_type_expr::facts::SourcePosition::Present(closed_ref_source(
                    "ResolvedAlpha",
                )),
                type_expansion: None,
            },
            verter_session_query::analysis::component_meta::ResolvedTypeAnalysis {
                name: "Gamma".to_string(),
                type_source: verter_type_expr::facts::SourcePosition::Present(closed_ref_source(
                    "ResolvedGamma",
                )),
                type_expansion: None,
            },
        ],
        output: crate::meta_resolve::ComponentMetaResolutionOutput {
            mode: verter_type_engine::semantic_query::ProjectionMode::Expanded,
            resolved_macros: Vec::new(),
            resolved_type_registry_meta: Vec::new(),
            origin_graph: None,
        },
    };

    let fixture_dispatch_23 =
        verter_type_engine::project_semantic_dispatch::ProjectSemanticDispatch::new(host);
    let output = crate::meta_resolve::projectors::build_component_meta_output(
        host,
        &fixture_dispatch_23,
        "/App.vue",
        analysis,
        Some(seed),
        crate::meta_resolve::PublishedCompleteness::COMPLETE,
    )
    .expect("closed registry sources materialize");
    let (analysis, resolution, types) = output.into_parts();
    let lanes = types.into_lanes();

    let names: Vec<&str> = analysis
        .type_registry
        .iter()
        .map(|e| e.name.as_str())
        .collect();
    assert_eq!(
        names,
        vec!["Alpha", "Beta", "Gamma"],
        "replace-in-place preserves order; new names append at the end"
    );
    assert_eq!(
        lanes.type_registry_entries.len(),
        3,
        "lane aligns with the MERGED registry"
    );
    assert!(
        matches!(&lanes.type_registry_entries[0], TypeExpr::Ref { name, .. } if name.as_ref() == "ResolvedAlpha"),
        "the resolved overlay's Alpha REPLACED the shallow analysis row"
    );
    assert!(
        matches!(&lanes.type_registry_entries[1], TypeExpr::Ref { name, .. } if name.as_ref() == "Beta"),
        "the untouched Beta row keeps its analysis value"
    );
    assert!(
        matches!(&lanes.type_registry_entries[2], TypeExpr::Ref { name, .. } if name.as_ref() == "ResolvedGamma"),
        "the appended Gamma row materializes the resolved value"
    );
    assert_eq!(
        resolution.expect("seeded resolution").mode,
        verter_type_engine::semantic_query::ProjectionMode::Expanded
    );
}

/// PUBLIC BOUNDARY, RENDERED BYTES — a producer that yielded NO SURFACE
/// refuses the module even when a SIBLING producer contributed members.
///
/// The refusal that protects the runtime lane has to be asked
/// per-CONTRIBUTION, not per-SURFACE. A no-value flow return
/// (`(() => { x = … })()` — an invoked closure writing a captured binding,
/// which the substrate does not model) produces no member set at all; when it is the macro's ONLY
/// producer the assembled surface is empty and a structural "is the surface
/// empty" check catches it. Compose it with ONE authored arm and that check
/// is defeated: the surface is non-empty, so the module publishes the
/// sibling's members alone and the no-value producer's members vanish.
///
/// That failure is quieter than the `props: {}` case it replaced, and
/// strictly worse. The IDE/TSX lane splices the AUTHORED macro call, so the
/// external checker types `props.label` as `string` and reports nothing,
/// while the runtime module Vue actually executes declares no `label` prop
/// at all — every `:label` binding falls through to `$attrs`. Types and
/// runtime disagree, silently, on two mainstream Vue idioms (an
/// intersection type argument and an `interface … extends` heritage
/// clause).
///
/// Oracle (TypeScript 7.0.2 `tsc`, `--noEmit --strict --ignoreConfig`):
/// for every row here the composed type is an ordinary object type whose
/// keys are `"label" | "extra"` (`"evA" | "evB"` for the emits row) —
/// verified with an `Eq<keyof T, …>` probe plus a negative control asserting
/// `Eq<keyof T, "extra">` which the checker REJECTS. So publishing the
/// sibling arm alone is a surface missing a declared member, not a
/// conservative answer.
///
/// Discrimination: restoring the per-surface `props.is_empty()` predicate
/// republishes every row here (the sibling arm makes the surface non-empty),
/// and the `PUBLISHES` control block below fails under a blanket "any
/// observed flow degradation refuses" regression — a FAITHFUL degraded
/// surface composed with an authored arm must still publish every one of its
/// members.
#[cfg(not(target_arch = "wasm32"))]
#[test]
fn a_no_surface_flow_return_refuses_even_when_a_sibling_arm_contributes() {
    /// A helper whose return the substrate cannot produce a surface for:
    /// an invoked closure writing a captured binding.
    const NO_SURFACE_PROPS: &str =
        "function makeProps() { let label = \"x\"; (() => { label = \"y\" })(); \
         return { label } }";
    const NO_SURFACE_EMITS: &str =
        "function makeEmits() { let ok = true; (() => { ok = false })(); \
         return { evA: (p: string) => ok } }";

    /// `(canonical, script, macro call, option key)` — the runtime lane must
    /// REFUSE, because one of the composed producers has no member set.
    const REFUSES: &[(&str, &str, &str, &str)] = &[
        // An INTERSECTION type argument: the authored arm contributes
        // `extra`, the no-surface arm contributes nothing.
        (
            "/src/X1InterProps.vue",
            NO_SURFACE_PROPS,
            "defineProps<ReturnType<typeof makeProps> & { extra: string }>()",
            "props: ",
        ),
        // The same composition on `defineEmits`: publishing `["evB"]` sends
        // every `@evA` listener silently through to `$attrs`.
        (
            "/src/X2InterEmits.vue",
            NO_SURFACE_EMITS,
            "defineEmits<ReturnType<typeof makeEmits> & { evB: (n: number) => void }>()",
            "emits: ",
        ),
        // `withDefaults` reaches the same projection through a wrapper, and
        // the defaults association must not make the incomplete surface look
        // deliberate.
        (
            "/src/X3WithDefaults.vue",
            NO_SURFACE_PROPS,
            "withDefaults(defineProps<ReturnType<typeof makeProps> & { extra?: string }>(), \
             { extra: \"z\" })",
            "props: ",
        ),
        // A HERITAGE clause composes the same two producers through a
        // declared interface rather than a type-argument intersection.
        (
            "/src/X4Heritage.vue",
            "function makeProps() { let label = \"x\"; (() => { label = \"y\" })(); \
             return { label } }\n\
             interface Props extends ReturnType<typeof makeProps> { extra: string }",
            "defineProps<Props>()",
            "props: ",
        ),
    ];

    for (canonical, script, macro_call, option_key) in REFUSES {
        match render_runtime_composed(canonical, script, macro_call, option_key) {
            RenderedRuntime::Refused => {}
            RenderedRuntime::Props(emitted) => panic!(
                "{canonical}: one composed producer yielded NO member set, so the assembled \
                 surface is missing declared members — the runtime lane must refuse rather \
                 than publish the sibling arm alone as `{emitted}`"
            ),
        }
    }

    /// `(canonical, script, macro call, option key, needles)` — a composed
    /// surface whose flow arm degraded FAITHFULLY (a marker at one position,
    /// every sibling exact) still has a complete member set, so it must
    /// publish it. The marker is the call of `notDeclared`, a name declared
    /// nowhere (TS2304), whose error type the flow-return lane does not
    /// model.
    const PUBLISHES: &[(&str, &str, &str, &str, &[&str])] = &[
        (
            "/src/X5FaithfulInter.vue",
            "function makeProps() { const f = () => notDeclared(); return { label: \"x\", made: f() } }",
            "defineProps<ReturnType<typeof makeProps> & { extra: string }>()",
            "props: ",
            &[
                "label: { type: String",
                "made: { type: null",
                "extra: { type: String",
            ],
        ),
        (
            "/src/X6ModelledInter.vue",
            "function makeProps() { return { label: \"x\" } }",
            "defineProps<ReturnType<typeof makeProps> & { extra: string }>()",
            "props: ",
            &["label: { type: String", "extra: { type: String"],
        ),
    ];

    for (canonical, script, macro_call, option_key, expected) in PUBLISHES {
        let RenderedRuntime::Props(emitted) =
            render_runtime_composed(canonical, script, macro_call, option_key)
        else {
            panic!(
                "{canonical}: every composed producer left a member set — refusing here would \
                 delete the module over a degradation that named no missing member"
            );
        };
        for needle in *expected {
            assert!(
                emitted.contains(needle),
                "{canonical}: expected `{needle}` in the emitted option:\n{emitted}"
            );
        }
    }
}

/// PUBLIC BOUNDARY, RENDERED BYTES — a no-surface producer at a MEMBER
/// VALUE position degrades that member, and only that member.
///
/// The no-surface class faults the option-rendering runtime lane, and the
/// precision of that fault is the whole claim: it says "a producer this
/// derivation asked for yielded no member set", and the member SET is what
/// the fault protects. A producer that yielded no value for ONE member's
/// TYPE has not removed that member from the surface — the name is
/// authored right there in the macro's own type argument — so the honest
/// emit is the complete name set with that member's validation off, the
/// same encoding the lane already uses for any member it could not
/// resolve.
///
/// This is the discrimination between "the class faults the lane" and "the
/// class deletes the module". Faulting the whole projection here would
/// delete every byte over one member's return type, for a component whose
/// other props the same tree resolves exactly.
///
/// Oracle (TypeScript 7.0.2 `tsc`, `--noEmit --strict --ignoreConfig`):
/// the type argument's keys are `"a" | "b"`, and `b` is `string`.
///
/// Discrimination: refusing fails the `Props` destructure; a fabricated
/// constructor for `a` fails the `type: null` assertion; collapsing every
/// member on the frame-level observation fails the `b: { type: String`
/// assertion.
#[cfg(not(target_arch = "wasm32"))]
#[test]
fn a_no_surface_producer_at_a_member_value_degrades_only_that_member() {
    let RenderedRuntime::Props(props) = render_runtime_composed(
        "/src/X7MemberNoSurface.vue",
        "function makeProps() { let q = \"x\"; (() => { q = \"y\" })(); \
         return { q } }",
        "defineProps<{ a: ReturnType<typeof makeProps>; b: string }>()",
        "props: ",
    ) else {
        panic!(
            "/src/X7MemberNoSurface.vue: the member SET is authored in the macro's own type \
             argument and is complete — a producer that could not type ONE member's value has \
             not removed a member, so the lane must publish rather than delete the module"
        );
    };
    assert!(
        props.contains("a: { type: null"),
        "the member whose value has no surface publishes with validation off:\n{props}"
    );
    assert!(
        props.contains("b: { type: String"),
        "its exactly-typed sibling keeps its real constructor:\n{props}"
    );

    // `defineModel` reaches the classifier with the WHOLE model type as
    // its subject. The binding is authored, so the honest emit is the
    // binding with validation off — not a deleted module.
    let RenderedRuntime::Props(model) = render_runtime_composed(
        "/src/X8ModelNoSurface.vue",
        "function makeProps() { let q = \"x\"; (() => { q = \"y\" })(); \
         return { q } }",
        "defineModel<ReturnType<typeof makeProps>>()",
        "props: ",
    ) else {
        panic!("/src/X8ModelNoSurface.vue: the model binding is authored and present");
    };
    assert!(
        model.contains("modelValue: { type: null"),
        "the model binding publishes with validation off:\n{model}"
    );

    // `defineEmits` carries NAMES, not member types, so a member-value
    // position that has no surface cannot shorten the event set — and
    // must not be read as if it had.
    let RenderedRuntime::Props(emits) = render_runtime_composed(
        "/src/X9EmitsMemberNoSurface.vue",
        "function makeProps() { let q = \"x\"; (() => { q = \"y\" })(); \
         return { q } }",
        "defineEmits<{ evA: [p: ReturnType<typeof makeProps>]; evB: [n: number] }>()",
        "emits: ",
    ) else {
        panic!("/src/X9EmitsMemberNoSurface.vue: the event NAMES are authored and complete");
    };
    assert!(
        emits.contains("\"evA\"") && emits.contains("\"evB\""),
        "both authored events survive a member-value position with no surface:\n{emits}"
    );
}

/// The completeness an OUTPUT ENVELOPE publishes must be the SAME merged
/// signal the result-cache admission gate refuses on
/// (`resolved.completeness.merge(extract_scope_completeness)`), never the
/// resolve-phase term alone.
///
/// Extract-phase partiality has two sources the resolve phase cannot observe:
/// the pre-choke macro-DTO read and the fallthrough cold compute. A component
/// whose own macro surface resolves cleanly but whose fallthrough walk over a
/// wide child root trips the projection fuse is partial ONLY in the extract
/// scope. Publishing the resolve term alone makes that payload serialize as
/// `Complete` while the very same compute permanently refuses to warm it — the
/// wrong-complete outcome the wire field exists to prevent.
///
/// The two premise assertions pin the fixture in that exact state (own resolve
/// Complete + admission refused), so a drift in the measured budget window
/// fails loudly here instead of silently un-discriminating the test. The
/// generous-budget control proves the merge does not blanket-degrade.
///
/// RED proof: pass `resolved.completeness` at the cold output entry instead of
/// the merged signal and the envelope reports Complete while
/// `has_owner_entry_in_test` stays false — the discriminating assertion fails.
#[cfg(not(target_arch = "wasm32"))]
#[test]
fn output_envelope_completeness_carries_extract_phase_partiality() {
    let project = extract_only_partial_project(EXTRACT_ONLY_PARTIAL_BUDGET);
    let host = project.host();

    // PREMISE 1 — the parent's OWN resolve is Complete, so the resolve term
    // alone cannot carry the partiality and the test discriminates.
    let resolved = host
        .resolve_component_meta(
            "/src/WideParent.vue",
            verter_type_engine::semantic_query::ProjectionMode::Expanded,
        )
        .expect("the parent resolves");
    assert!(
        !resolved.completeness.is_partial(),
        "the parent's own macro surface must resolve COMPLETE at this budget — otherwise the \
         resolve-phase term already carries the partiality and this test proves nothing"
    );

    let (.., completeness) = host
        .get_component_meta_output("/src/WideParent.vue")
        .expect("the output envelope materializes")
        .expect("the component resolves")
        .into_parts_with_contract();

    // PREMISE 2 — the merged gate permanently refuses this result warm
    // admission. That refusal is what makes a `Complete` envelope wrong.
    assert!(
        !crate::component_meta_cached_result::has_owner_entry_in_test(host, "/src/WideParent.vue"),
        "the merged admission gate must refuse the extract-partial parent (the fixture's premise)"
    );

    // THE DISCRIMINATOR.
    assert!(
        completeness.is_partial(),
        "the published envelope must report the MERGED completeness: this result's partiality \
         lives entirely in the extract scope, so a `Complete` envelope is wrong-complete on a \
         payload the same compute refuses to warm"
    );

    // CONTROL: a generous budget trips nothing → Complete envelope AND a warm
    // entry, proving the merge does not blanket-degrade and the fixture's
    // admission path is live.
    let control = extract_only_partial_project(0);
    let control_host = control.host();
    let (.., control_completeness) = control_host
        .get_component_meta_output("/src/WideParent.vue")
        .expect("the control envelope materializes")
        .expect("the control component resolves")
        .into_parts_with_contract();
    assert!(
        !control_completeness.is_partial(),
        "the generous-budget control must still publish Complete"
    );
    assert!(
        crate::component_meta_cached_result::has_owner_entry_in_test(
            control_host,
            "/src/WideParent.vue"
        ),
        "the control must warm, proving the admission path this fixture exercises is live"
    );
}

/// The RESOLUTION-bearing output entry
/// (`get_component_meta_output_with_resolution` — the audited NAPI / WASM /
/// LSP surface) is a SEPARATE cold body with its own envelope-build call site.
/// It must publish the same merged completeness: a per-entry fix that misses
/// this lane leaves the audited surface wrong-complete while the plain entry
/// is correct.
#[cfg(not(target_arch = "wasm32"))]
#[test]
fn resolution_bearing_output_envelope_carries_extract_phase_partiality() {
    let project = extract_only_partial_project(EXTRACT_ONLY_PARTIAL_BUDGET);
    let host = project.host();
    let (output, _request_id) = host
        .get_component_meta_output_with_resolution("/src/WideParent.vue")
        .expect("the resolution-bearing envelope materializes");
    let (.., completeness) = output
        .expect("the component resolves")
        .into_parts_with_contract();
    assert!(
        !crate::component_meta_cached_result::has_owner_entry_in_test(host, "/src/WideParent.vue"),
        "the merged admission gate must refuse the extract-partial parent (the fixture's premise)"
    );
    assert!(
        completeness.is_partial(),
        "the resolution-bearing envelope must report the MERGED completeness"
    );

    let control = extract_only_partial_project(0);
    let (control_output, _) = control
        .host()
        .get_component_meta_output_with_resolution("/src/WideParent.vue")
        .expect("the control envelope materializes");
    let (.., control_completeness) = control_output
        .expect("the control component resolves")
        .into_parts_with_contract();
    assert!(
        !control_completeness.is_partial(),
        "the generous-budget control must still publish Complete on the resolution-bearing entry"
    );
}
