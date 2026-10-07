use super::*;

#[cfg(not(target_arch = "wasm32"))]
#[test]
fn fallthrough_runtime_cache_remains_authoritative_after_legacy_slot_is_cleared() {
    let project = make_project();
    project
        .upsert_base("/Child.vue", r#"<template><div>child</div></template>"#)
        .unwrap();
    project
        .upsert_base(
            "/App.vue",
            r#"<script setup lang="ts">
import Child from './Child.vue'
</script>
<template><Child /></template>"#,
        )
        .unwrap();

    project.host().resolver_runtime().reset_counters();
    let _ = get_meta(&project, "/App.vue");
    // get_meta does not populate cached_fallthrough; use resolve_fallthrough_surface
    let _ = project.host().resolve_fallthrough_surface("/App.vue");
    let after_first = project.host().resolver_runtime().counter_snapshot();
    let first_cache = cached_fallthrough_state(&project, "/App.vue")
        .expect("initial lookup should populate the legacy fallthrough mirror");

    clear_legacy_cached_fallthrough_state(&project, "/App.vue");
    assert!(
        cached_fallthrough_state(&project, "/App.vue").is_none(),
        "legacy fallthrough cache slot should be cleared before the second lookup"
    );

    project.host().provenance.reset();

    let second = project
        .host()
        .resolve_fallthrough_surface("/App.vue")
        .expect("second fallthrough resolve should succeed from resolver-owned cache");
    let after_second = project.host().resolver_runtime().counter_snapshot();
    assert!(
        cached_fallthrough_state(&project, "/App.vue").is_none(),
        "a warm runtime read is read-only and must not repopulate the legacy mirror"
    );

    let first_prop_names: Vec<_> = first_cache
        .accepted_props
        .iter()
        .map(|prop| prop.name.as_str())
        .collect();
    let second_prop_names: Vec<_> = second
        .accepted_props
        .iter()
        .map(|prop| prop.name.as_str())
        .collect();
    assert_eq!(first_prop_names, second_prop_names);
    assert_eq!(
        first_cache.accepted_surface_completeness,
        second.accepted_surface_completeness
    );
    assert_eq!(
        first_cache.fact_versions.len(),
        second.fact_versions.len(),
        "the authoritative runtime result preserves dependency fact coverage"
    );
    assert!(
        after_first.node_cache_misses > 0,
        "first fallthrough resolve should populate runtime fallthrough nodes, got {:?}",
        after_first
    );
    assert!(
        after_second.node_cache_hits > after_first.node_cache_hits,
        "clearing only the legacy mirror should now reuse the runtime top-level cache directly, before={:?} after={:?}",
        after_first,
        after_second
    );
    assert_eq!(
        provenance(&project).resolver_node_cache_hits,
        1,
        "second fallthrough lookup should be served from the authoritative runtime cache"
    );
    assert_eq!(
        provenance(&project).resolver_node_cache_misses,
        0,
        "second fallthrough lookup should not miss once the runtime cache is consulted after the legacy slot is cleared"
    );
}

#[cfg(not(target_arch = "wasm32"))]
#[test]
fn budget_exhausted_no_override_fallthrough_is_not_cached() {
    // No-poison: a no-override fallthrough whose spread walker trips the shared
    // projection budget MID-WALK is a PARTIAL — the trip folds into the active
    // per-cold-compute completeness scope, so it must NOT warm the runtime node
    // cache NOR the legacy `cached_fallthrough` mirror. Both admission sites gate
    // on the typed `current_cold_compute_completeness`; without the mirror gate
    // the partial warms `DerivedRawState.cached_fallthrough` and a later request
    // replays it.
    use crate::resolver_core::FallthroughRequestHost;

    // A `v-bind` of a union of distinct-keyed objects on a native root drives the
    // fallthrough spread walker, which is the budget consumer that trips the cap.
    let project = make_project();
    project
        .upsert_base(
            "/obj.ts",
            r#"export declare const obj: { a: string } | { b: string } | { c: string };"#,
        )
        .unwrap();
    project
        .upsert_base(
            "/App.vue",
            r#"<script setup lang="ts">
import { obj } from './obj'
</script>
<template><div v-bind="obj" /></template>"#,
        )
        .unwrap();
    project.host().set_import_dependencies(
        "/App.vue",
        vec![crate::types::DependencyResolution {
            specifier: "./obj".to_string(),
            resolved_canonical_id: Some("/obj.ts".to_string()),
            possible_canonical_ids: Vec::new(),
        }],
    );

    // A low projection budget: the spread walker trips it mid-walk, folding a
    // partial into the cold-compute scope — the resolved fallthrough is a partial
    // that must not warm any cache.
    let rctx =
        verter_type_engine::request_context::RequestContext::with_kind_timing_and_projection_budget(
            1,
            Arc::from("/App.vue"),
            verter_audit::RequestKind::ComponentMeta,
            false,
            false,
            None,
            1,
        );
    let guard =
        verter_type_engine::request_context::RequestContextGuard::install(Arc::clone(&rctx));

    let _ = project.host().resolve_fallthrough_surface("/App.vue");

    drop(guard);

    // The legacy mirror must be empty (THE Part 3 discriminator: without the
    // mirror gate, the budget-exhausted partial warms it here).
    assert!(
        cached_fallthrough_state(&project, "/App.vue").is_none(),
        "a budget-exhausted no-override fallthrough must NOT warm the legacy cached_fallthrough mirror"
    );
    // The runtime top-level node cache must also be empty (store_node self-gate).
    let key = crate::resolver_core::fallthrough_cache_key(
        "/App.vue",
        project.host().config.generic_root_propagation,
        None,
    );
    let view = FallthroughRequestHost::snapshot_store_view(project.host());
    assert!(
        project
            .host()
            .resolver_runtime()
            .fallthrough
            .get_cached_node(&key, &view)
            .is_none(),
        "a budget-exhausted no-override fallthrough must NOT warm the runtime node cache"
    );

    // Positive control: the SAME scenario WITHOUT a tripped budget DOES warm the
    // mirror (the direct entry installs the default budget, which the small spread
    // walk completes under), proving the gate suppresses only the genuine partial.
    let _ = project.host().resolve_fallthrough_surface("/App.vue");
    assert!(
        cached_fallthrough_state(&project, "/App.vue").is_some(),
        "without a tripped budget the no-override fallthrough DOES warm the mirror"
    );
}

/// PUBLIC BOUNDARY, RENDERED BYTES — a flow-return degradation at the ROOT
/// refuses; it never publishes an empty `props` object.
///
/// A marker at an interior POSITION leaves a member set the runtime lane can
/// publish. A marker at the ROOT does not: there are no members at all, so
/// the derived surface is empty and emitting it declares a props-less
/// component for a component that declares props. Every listener and bound
/// attribute then falls through to `$attrs` — silently wrong at runtime, and
/// strictly worse than refusing, because refusing is loud and the TSX lane
/// still type-checks the file.
///
/// Two families reach the root: a NO-VALUE outcome (`R2Invoked`: an
/// invoked closure writing a captured binding, which the substrate does not
/// model) and an object literal
/// whose SPREAD SOURCE the substrate cannot type — directly
/// (`S5UndeclaredSpread`) or one call away (`S6MarkerSpread`, whose
/// callee's own frame is what cannot type its return). Both sources call
/// `notDeclared`, a name declared nowhere (TS2304), whose error type is
/// recovery the flow-return lane does not model. An unknown source makes
/// the literal's KEY SET unknown, and an object surface has no way to say
/// "and an unknown number of further keys". tsgo types both as `any` — the
/// spread of its error type — so there is no key set to publish, and
/// `{ n }` alone would declare one.
///
/// `S5UndeclaredSpread` is the row that discriminates the evaluator's
/// spread fail-closed rail: dropping an unevaluable spread source instead
/// of failing the literal closed publishes `props: { n: { type: Number } }`
/// for it. `S6MarkerSpread` reaches the same verdict one rail earlier
/// (the callee's frame failure propagates) and is coverage for that
/// authored shape rather than a second discriminator for the same arm.
///
/// A spread whose source IS modelled is not in that family: the literal
/// lowers structurally, the source rides the same call sink every other
/// call position rides, and the surface publishes with its real
/// constructors. That is the `SPREADS` block below.
///
/// Oracle (TypeScript 7.0.2 `tsc`, `--noEmit --strict --ignoreConfig`):
/// every spread row's `ReturnType<typeof makeProps>` is an ordinary object
/// type — `{ label: string; n: number }` for S1/S3/S4/C1, `{ label: "x"; n:
/// number }` for S7, `{ label: string }` for S2 — which is exactly why
/// publishing `props: {}` for them is wrong
/// rather than merely conservative.
///
/// The controls are the discrimination: a blanket "never publish an empty
/// surface" regression passes every refusal row and fails every publish row,
/// whose props must carry their real constructors.
#[cfg(not(target_arch = "wasm32"))]
#[test]
fn a_root_position_flow_degradation_refuses_instead_of_publishing_empty_props() {
    /// `(canonical, script)` — the runtime lane must REFUSE.
    const REFUSES: &[(&str, &str)] = &[
        (
            "/src/R2Invoked.vue",
            "function makeProps() { let label = \"x\"; (() => { label = \"y\" })(); return { label } }",
        ),
        (
            "/src/S5UndeclaredSpread.vue",
            "function makeProps() { return { ...notDeclared(), n: 1 } }",
        ),
        // The same fact one CALL away — the spread source is a modelled
        // direct call whose own frame is the one that cannot type its
        // return. It exercises a different rail from S5 (whose spread
        // source is the unresolvable call itself) and reaches the same
        // verdict: publishing `{ n }` here would declare a key set the
        // checker does not have.
        (
            "/src/S6MarkerSpread.vue",
            "function base() { return notDeclared() }\nfunction makeProps() { return { ...base(), n: 1 } }",
        ),
    ];

    for (canonical, script) in REFUSES {
        match render_runtime_props(canonical, script) {
            RenderedRuntime::Refused => {}
            RenderedRuntime::Props(props) => panic!(
                "{canonical}: the substrate could not type the ROOT, so there is no member set \
                 — the runtime lane must refuse rather than declare this component's props to \
                 be `{props}`"
            ),
        }
    }

    /// `(canonical, script)` — the runtime lane must PUBLISH exactly this.
    const PUBLISHES: &[(&str, &str, &[&str])] = &[
        (
            "/src/S1Spread.vue",
            "function base() { return { label: \"x\" } }\nfunction makeProps() { return { ...base(), n: 1 } }",
            &["label: { type: String", "n: { type: Number"],
        ),
        (
            "/src/S2SpreadOnly.vue",
            "function base() { return { label: \"x\" } }\nfunction makeProps() { return { ...base() } }",
            &["label: { type: String"],
        ),
        (
            "/src/S3TwoSpreads.vue",
            "function a() { return { label: \"x\" } }\nfunction b() { return { n: 1 } }\nfunction makeProps() { return { ...a(), ...b() } }",
            &["label: { type: String", "n: { type: Number"],
        ),
        (
            "/src/S4ArrowSpread.vue",
            "const arrowConst = () => ({ label: \"x\" })\nfunction makeProps() { return { ...arrowConst(), n: 1 } }",
            &["label: { type: String", "n: { type: Number"],
        ),
        (
            "/src/C1ModuleConst.vue",
            "const mc = { label: \"x\" }\nfunction makeProps() { return { ...mc, n: 1 } }",
            &["label: { type: String", "n: { type: Number"],
        ),
        (
            "/src/C2Plain.vue",
            "function makeProps() { return { label: \"x\", n: 1 } }",
            &["label: { type: String", "n: { type: Number"],
        ),
        // A `new` spread source is modelled: the construction resolves to
        // the class instance, whose `label` spreads in.
        (
            "/src/S7NewSpread.vue",
            "class Box { readonly label = \"x\" }\nfunction makeProps() { return { ...new Box(), n: 1 } }",
            &["label: { type: String", "n: { type: Number"],
        ),
    ];

    for (canonical, script, expected) in PUBLISHES {
        let RenderedRuntime::Props(props) = render_runtime_props(canonical, script) else {
            panic!("{canonical}: a fully modelled surface must still compile and publish");
        };
        for needle in *expected {
            assert!(
                props.contains(needle),
                "{canonical}: expected `{needle}` in the emitted props object:\n{props}"
            );
        }
        assert!(
            !props.contains("type: null"),
            "{canonical}: every member of a fully modelled spread surface has a real \
             constructor — `type: null` is the erasure this row exists to forbid:\n{props}"
        );
    }

    // `defineEmits` is the same rule on the same evidence, and the failure is
    // not a milder one: `emits: []` for a component that declares emits sends
    // every `@evA` listener silently through to `$attrs` instead of the
    // declared emit.
    let RenderedRuntime::Props(emits) = render_runtime_emits(
        "/src/E1Spread.vue",
        "function base() { return { evA: (p: string) => true } }\nfunction makeEmits() { return { ...base(), evB: (n: number) => true } }",
    ) else {
        panic!("/src/E1Spread.vue: a modelled spread source leaves a complete event set");
    };
    assert!(
        emits.contains("\"evA\"") && emits.contains("\"evB\""),
        "/src/E1Spread.vue: the spread-contributed event and the direct one must BOTH \
         survive:\n{emits}"
    );

    let RenderedRuntime::Props(emits) = render_runtime_emits(
        "/src/E2Plain.vue",
        "function makeEmits() { return { evA: (p: string) => true, evB: (n: number) => true } }",
    ) else {
        panic!("/src/E2Plain.vue: a fully modelled emits surface must still compile and publish");
    };
    assert!(
        emits.contains("\"evA\"") && emits.contains("\"evB\""),
        "/src/E2Plain.vue: both declared events must survive:\n{emits}"
    );
}

#[cfg(not(target_arch = "wasm32"))]
#[test]
fn fallthrough_only_budget_partial_does_not_warm_component_meta_result_db() {
    use crate::resolver_core::FallthroughRequestHost;
    use std::sync::atomic::Ordering::Relaxed;

    // Low projection budget: resolve (no macros) stays at zero ops, the
    // fallthrough spread walker trips mid-walk.
    let project = make_project_with_config(HostConfig {
        analysis_level: crate::types::AnalysisLevel::Full,
        projection_op_budget: 3,
        ..HostConfig::default()
    });
    upsert_fallthrough_spread_owner(&project);
    let host = project.host();
    let canonical = "/src/App.vue";

    // First (cold) resolve: produces the fallthrough partial.
    let (meta1, resolved) = host
        .get_component_meta_with_resolution(canonical)
        .expect("a fallthrough-tripped resolve must still return partial metadata");

    // The resolve is clean — the partial is fallthrough-only (the exact shape
    // this fix targets: too late for `resolved.synthesis_should_suppress`).
    assert!(
        !resolved.synthesis_should_suppress,
        "the no-macro owner's resolve must complete cleanly so the partial is fallthrough-only \
         (synthesis_should_suppress reflects resolve, not the later fallthrough trip)"
    );
    assert!(
        matches!(
            meta1.accepted_surface_completeness,
            verter_session_query::analysis::component_meta::AcceptedSurfaceCompleteness::LowerBound
        ),
        "the budget-tripped fallthrough surface is a lower bound"
    );

    // The fallthrough partial must NOT have warmed `ComponentMetaResultDb`: a
    // second resolve is NOT a warm hit.
    let hits_before = host
        .provenance()
        .component_meta_result_cache_hits
        .load(Relaxed);
    let _ = host
        .get_component_meta_with_resolution(canonical)
        .expect("second resolve must still succeed");
    let hits_after = host
        .provenance()
        .component_meta_result_cache_hits
        .load(Relaxed);
    assert_eq!(
        hits_after, hits_before,
        "a fallthrough-only budget partial MUST NOT warm `ComponentMetaResultDb` — the replay must \
         be cold (hits_before={hits_before}, hits_after={hits_after}); pre-fix the publish gate saw \
         only resolved.synthesis_should_suppress=false and warmed the partial"
    );

    // The runtime node cache and the legacy mirror also stay empty for the
    // partial (no fallthrough-cache leak).
    let key = crate::resolver_core::fallthrough_cache_key(
        canonical,
        host.config.generic_root_propagation,
        None,
    );
    let view = FallthroughRequestHost::snapshot_store_view(host);
    assert!(
        host.resolver_runtime()
            .fallthrough
            .get_cached_node(&key, &view)
            .is_none(),
        "the fallthrough-partial top-level node must NOT be warm in the runtime node cache"
    );
    assert!(
        cached_fallthrough_state(&project, canonical).is_none(),
        "the fallthrough-partial must NOT warm the legacy cached_fallthrough mirror"
    );
}

/// The PUBLIC direct entry `resolve_fallthrough_surface` installs a
/// `RequestContext` when none is ambient, so the projection budget and the
/// completeness gate are LIVE on that path (no manually-installed guard). With a
/// low `projection_op_budget` and NO ambient context, the spread walker trips
/// and the partial gates the mirror.
///
/// Without the context install the direct entry has no `RequestContext`, so
/// `current_request_budget()` is `None`, the walker never trips, the fallthrough
/// completes, and the mirror is warmed. With it, the auto-installed context's
/// budget trips and the completeness gate refuses the mirror.
#[cfg(not(target_arch = "wasm32"))]
#[test]
fn direct_resolve_fallthrough_surface_installs_context_so_budget_gate_is_live() {
    let project = make_project_with_config(HostConfig {
        analysis_level: crate::types::AnalysisLevel::Full,
        projection_op_budget: 3,
        ..HostConfig::default()
    });
    upsert_fallthrough_spread_owner(&project);

    // No ambient RequestContext is installed: the direct entry must install its
    // own (with `config.projection_op_budget`) for the gate to be live.
    assert!(
        verter_type_engine::request_context::current_request_context().is_none(),
        "test precondition: no ambient request context"
    );

    let _ = project.host().resolve_fallthrough_surface("/src/App.vue");

    assert!(
        cached_fallthrough_state(&project, "/src/App.vue").is_none(),
        "with no ambient context, the direct entry MUST install one so the low projection budget \
         trips the spread walker and the partial-completeness gate refuses the mirror; pre-fix no \
         context is installed, the walker never trips, and the mirror is warmed"
    );
}

/// DON'T-OVER-GATE positive control: a `LowerBound` SURFACE whose COMPUTE is
/// COMPLETE is still cacheable. The same spread owner under a generous budget
/// produces a lower-bound accepted surface (the union spread is inexact) WITHOUT
/// any budget/fuse trip, so the fallthrough completeness is `Complete` and the
/// result warms `ComponentMetaResultDb`. A regression that wrongly gated on the
/// surface-shape `accepted_surface_completeness == LowerBound` would refuse this
/// and the replay would be cold.
#[cfg(not(target_arch = "wasm32"))]
#[test]
fn lower_bound_complete_fallthrough_surface_still_warms_component_meta_result_db() {
    use std::sync::atomic::Ordering::Relaxed;

    // Default (generous) budget: the spread walk COMPLETES, so the compute is
    // Complete even though the surface is a lower bound.
    let project = make_project();
    upsert_fallthrough_spread_owner(&project);
    let host = project.host();
    let canonical = "/src/App.vue";

    let (meta1, resolved) = host
        .get_component_meta_with_resolution(canonical)
        .expect("a clean resolve must return metadata");
    assert!(
        !resolved.synthesis_should_suppress,
        "the clean resolve must not suppress"
    );
    assert!(
        matches!(
            meta1.accepted_surface_completeness,
            verter_session_query::analysis::component_meta::AcceptedSurfaceCompleteness::LowerBound
        ),
        "the inexact union spread yields a lower-bound SURFACE (distinct from compute completeness)"
    );

    // The LowerBound-surface / Complete-compute result MUST warm: a second
    // resolve IS a warm hit.
    let hits_before = host
        .provenance()
        .component_meta_result_cache_hits
        .load(Relaxed);
    let _ = host
        .get_component_meta_with_resolution(canonical)
        .expect("second resolve must succeed");
    let hits_after = host
        .provenance()
        .component_meta_result_cache_hits
        .load(Relaxed);
    assert!(
        hits_after > hits_before,
        "a LowerBound SURFACE with COMPLETE compute MUST stay cacheable — the replay must warm-hit \
         `ComponentMetaResultDb` (hits_before={hits_before}, hits_after={hits_after}); gating on the \
         surface shape instead of compute completeness would over-gate this"
    );
}

/// Regression lock (payload-surface fallthrough-only-partial no-poison): a
/// fallthrough-only budget partial reached through the BARE scalar payload
/// surface must NOT warm `cached_meta_payload`. `resolve_one_payload_item`
/// installs ONE `RequestContext` (with `config.projection_op_budget`) spanning
/// its cold resolve AND its fallthrough extract, so the projection-op budget
/// fuse is LIVE during the payload fallthrough exactly as on the analysis
/// surface. The no-macro owner's resolve completes cleanly
/// (`synthesis_should_suppress == false`) — the only partial comes from the
/// spread walker tripping the low budget DURING the payload extract. The
/// payload-write gate merges the threaded fallthrough completeness and refuses
/// to warm the payload (it is still RETURNED). Without the spanning install the
/// payload extract runs context-free, the budget never trips, the compute is
/// Complete, and the payload warms — RED at the `is_none()` replay assertion.
#[cfg(not(target_arch = "wasm32"))]
#[test]
fn fallthrough_only_budget_partial_does_not_warm_cached_meta_payload() {
    // Low projection budget: resolve (no macros) stays at zero ops; the
    // fallthrough spread walker trips mid-walk during the PAYLOAD extract.
    let project = make_project_with_config(HostConfig {
        analysis_level: crate::types::AnalysisLevel::Full,
        projection_op_budget: 3,
        ..HostConfig::default()
    });
    upsert_fallthrough_spread_owner(&project);
    let host = project.host();
    let canonical = "/src/App.vue";

    // The resolve itself is clean — the partial is fallthrough-only (too late
    // for `resolved.synthesis_should_suppress`, the exact shape this gate
    // targets on the payload surface).
    let (_meta, resolved) = host
        .get_component_meta_with_resolution(canonical)
        .expect("a fallthrough-tripped resolve must still return partial metadata");
    assert!(
        !resolved.synthesis_should_suppress,
        "the no-macro owner's resolve completes cleanly so the partial is fallthrough-only \
         (synthesis_should_suppress reflects resolve, not the later fallthrough trip)"
    );

    // Drive the BARE scalar payload surface with NO ambient context: the
    // install inside `resolve_one_payload_item` is what makes the budget live
    // during the extract (the discriminator is the FIX, not the fixture).
    assert!(
        verter_type_engine::request_context::current_request_context().is_none(),
        "test precondition: no ambient request context — the payload path must install its own"
    );
    let session = project.open_session_batch().unwrap();
    let payload = session
        .get_component_meta_payload(canonical, test_encode_fn)
        .expect("the payload request must succeed")
        .expect("a partial payload is still RETURNED to the caller, just not warmed");
    assert!(
        !payload.is_empty(),
        "the partial payload is returned to the caller"
    );

    // The fallthrough-only budget partial must NOT have warmed the payload
    // cache: a fresh warm read finds nothing.
    assert!(
        host.try_get_cached_meta_payload(canonical).is_none(),
        "a fallthrough-only budget partial reached through the payload surface MUST NOT warm \
         cached_meta_payload — without the spanning install the extract runs context-free (no trip \
         → Complete compute → warmed); the fix installs a RequestContext spanning resolve+extract \
         so the budget trips → partial → the completeness gate refuses the warm"
    );
}

/// DON'T-OVER-GATE positive control (payload surface): a `LowerBound` SURFACE
/// whose COMPUTE is COMPLETE must still warm `cached_meta_payload`. The same
/// spread owner under the default (generous) budget produces a lower-bound
/// accepted surface WITHOUT any budget/fuse trip, so the fallthrough
/// completeness is `Complete` and the payload warms. A regression that wrongly
/// gated on the surface-shape `accepted_surface_completeness == LowerBound`
/// would refuse this and the warm read would miss.
#[cfg(not(target_arch = "wasm32"))]
#[test]
fn lower_bound_complete_fallthrough_surface_still_warms_cached_meta_payload() {
    // Default (generous) budget: the spread walk COMPLETES, so the compute is
    // Complete even though the surface is a lower bound.
    let project = make_project();
    upsert_fallthrough_spread_owner(&project);
    let host = project.host();
    let canonical = "/src/App.vue";

    let (meta1, resolved) = host
        .get_component_meta_with_resolution(canonical)
        .expect("a clean resolve must return metadata");
    assert!(
        !resolved.synthesis_should_suppress,
        "the clean resolve must not suppress"
    );
    assert!(
        matches!(
            meta1.accepted_surface_completeness,
            verter_session_query::analysis::component_meta::AcceptedSurfaceCompleteness::LowerBound
        ),
        "the inexact union spread yields a lower-bound SURFACE (distinct from compute completeness)"
    );

    // The LowerBound-surface / Complete-compute payload MUST warm.
    let session = project.open_session_batch().unwrap();
    let _payload = session
        .get_component_meta_payload(canonical, test_encode_fn)
        .expect("the payload request must succeed")
        .expect("a clean payload is returned");
    assert!(
        host.try_get_cached_meta_payload(canonical).is_some(),
        "a LowerBound SURFACE with COMPLETE compute MUST still warm cached_meta_payload — gating on \
         the surface shape instead of compute completeness would over-gate this"
    );
}

/// BUDGET-PARITY (D4): the projection-budget fuse + the no-poison completeness
/// gate must be LIVE on the SESSION-VIEW surfaces — the view-aware
/// `MetaSession::get_component_meta` (`get_component_meta_via_view`) AND the
/// session `MetaSession::get_component_meta_with_resolution`
/// (`get_component_meta_with_resolution_via_view`) — not only on the
/// direct-analysis / audited / payload surfaces (Shared Optimized Codebase).
///
/// Pre-fix both view paths ran the fallthrough extract CONTEXT-FREE: the inner
/// `resolve_component_meta_with_*` install-if-none dropped its `RequestContext`
/// before the extract, so `current_request_budget()` was `None` during the
/// fallthrough, the spread walker never tripped, the compute was `Complete`,
/// and the partial-as-complete warmed downstream caches. The D4 install-if-none
/// spans the FULL cold body (resolve AND extract) on each surface.
///
/// Discriminating observables (each surface keys its own gate):
/// - view-aware: the ComponentMetaResultDb publish is gated on the threaded
///   `fallthrough_completeness` — a partial REFUSES admission, so a replay is a
///   cold miss (`component_meta_result_cache_hits` unchanged). RED pre-fix
///   (Complete → warmed → replay warm-hits).
/// - session with-resolution: it discards the ComponentMetaResultDb publish,
///   but its bounded fallthrough extract folds the budget trip into the
///   cold-compute scope the runtime-node `store_node` gate reads, so the
///   top-level fallthrough node is NOT warmed. RED pre-fix (no trip → Complete
///   → `store_node` warms the runtime node).
///
/// The partial is fallthrough-ONLY: the no-macro owner's Expanded resolve
/// completes cleanly (`synthesis_should_suppress == false`), isolating the
/// budget fuse from the synthesis gate (the exact shape D4 targets).
#[cfg(not(target_arch = "wasm32"))]
#[test]
fn fallthrough_only_budget_partial_not_warmed_through_session_view_surfaces() {
    use crate::resolver_core::FallthroughRequestHost;
    use std::sync::atomic::Ordering::Relaxed;

    // Low projection budget: resolve (no macros) stays at zero ops; the
    // fallthrough spread walker trips mid-walk DURING the extract.
    let project = make_project_with_config(HostConfig {
        analysis_level: crate::types::AnalysisLevel::Full,
        projection_op_budget: 3,
        ..HostConfig::default()
    });
    upsert_fallthrough_spread_owner(&project);
    let host = project.host();
    let canonical = "/src/App.vue";

    // Precondition: the partial is fallthrough-only (resolve completes clean).
    // The audited base entry already spans resolve+extract, so it does NOT
    // warm the partial — it only establishes the fallthrough-only shape.
    let (_meta, resolved) = host
        .get_component_meta_with_resolution(canonical)
        .expect("a fallthrough-tripped resolve must still return partial metadata");
    assert!(
        !resolved.synthesis_should_suppress,
        "the no-macro owner's resolve must complete cleanly so the partial is fallthrough-only \
         (synthesis_should_suppress reflects resolve, not the later fallthrough trip)"
    );

    // ── Surface 1: view-aware `MetaSession::get_component_meta` (site 430).
    // No ambient context — the via-view path must install its own spanning the
    // extract (the discriminator is the FIX, not the fixture).
    assert!(
        verter_type_engine::request_context::current_request_context().is_none(),
        "test precondition: no ambient request context"
    );
    let session = project.open_session_batch().unwrap();
    let _ = session
        .get_component_meta(canonical)
        .expect("view-aware meta request must succeed")
        .expect("a partial analysis is still RETURNED to the caller, just not warmed");
    let hits_before = host
        .provenance()
        .component_meta_result_cache_hits
        .load(Relaxed);
    let _ = session
        .get_component_meta(canonical)
        .expect("second view-aware request must succeed")
        .expect("the replay is still RETURNED");
    let hits_after = host
        .provenance()
        .component_meta_result_cache_hits
        .load(Relaxed);
    assert_eq!(
        hits_after, hits_before,
        "the view-aware surface's fallthrough-only budget partial MUST NOT warm \
         `ComponentMetaResultDb` — the replay must be cold (hits_before={hits_before}, \
         hits_after={hits_after}); pre-fix the extract ran context-free, the budget never tripped, \
         the compute was Complete, and the publish gate warmed the partial"
    );

    // ── Surface 2: session `MetaSession::get_component_meta_with_resolution`
    // (site 276). It discards the ComponentMetaResultDb publish, so the gated
    // runtime-node `store_node` is the observable: the bounded extract folds
    // the budget trip into the cold-compute scope the gate reads, refusing the
    // top-level fallthrough node.
    let key = crate::resolver_core::fallthrough_cache_key(
        canonical,
        host.config.generic_root_propagation,
        None,
    );
    clear_runtime_top_level_fallthrough_node(&project, canonical);
    {
        let view = FallthroughRequestHost::snapshot_store_view(host);
        assert!(
            host.resolver_runtime()
                .fallthrough
                .get_cached_node(&key, &view)
                .is_none(),
            "test precondition: the runtime fallthrough node is cleared before the session call"
        );
    }
    let session_wr = project.open_session_batch().unwrap();
    let _ = session_wr
        .get_component_meta_with_resolution(canonical)
        .expect("session with-resolution request must succeed")
        .expect("a partial result is still RETURNED to the caller, just not warmed");
    let view = FallthroughRequestHost::snapshot_store_view(host);
    assert!(
        host.resolver_runtime()
            .fallthrough
            .get_cached_node(&key, &view)
            .is_none(),
        "the session with-resolution surface's bounded fallthrough extract MUST NOT warm the \
         runtime fallthrough node — pre-fix the extract ran context-free, the budget never tripped, \
         the compute was Complete, and `store_node` warmed the top-level node"
    );
}

/// BUDGET-PARITY (D4) — the VIEW-AWARE OUTER full-request install is
/// load-bearing: the projection budget must span resolve AND fallthrough as
/// ONE budget, not two per-phase budgets the fallthrough choke alone would
/// arm.
///
/// The sibling `fallthrough_only_budget_partial_not_warmed_through_session_view_surfaces`
/// uses a fallthrough-ONLY trip — the fallthrough choke backstop
/// (`compute_fallthrough_surface_from_resolved_state`'s install-if-none)
/// arms a fresh per-fallthrough budget and trips it, so that test stays
/// GREEN even with the OUTER installs deleted. This test closes that gap
/// with a MIXED fixture whose RESOLVE phase (a `defineProps<Partial<…>>()`
/// macro) AND fallthrough phase (a `v-bind` spread) each charge projection
/// ops over DISJOINT types: neither phase alone exceeds the budget, but their
/// COMBINED work does.
///
/// View-aware (`get_component_meta`, `component_meta_entry.rs`): the resolve
/// and the fallthrough extract share ONE resolver ctx, so the choke's
/// re-resolution of the owner's declared props hits the resolve's WARM cache
/// (the choke charges only the fallthrough's ops). Only the OUTER install
/// makes the resolve ops and the fallthrough ops share one budget — so the
/// combined work trips ONLY with the outer install. Revert it and the work
/// splits into an inner-resolve budget + a fresh-choke budget, neither trips,
/// the compute is Complete, and the partial-as-complete warms
/// `ComponentMetaResultDb` (the replay warm-hits) — RED. This surface is the
/// DISCRIMINATING half.
///
/// Session (`get_component_meta_with_resolution`,
/// `component_meta_entry_resolution.rs`): for THIS combined fixture the budget
/// trips in the FALLTHROUGH, and the session path rebuilds the resolver ctx
/// with a COLD-SEED view between resolve and extract, so the choke's
/// `extract_component_meta` re-resolves the full surface COLD within the
/// choke's own budget — the choke alone already bounds this combined work, so
/// Surface 2 is a corroborating combined-budget-partial guard here, not the
/// discriminating half for this fixture. The session outer install is still
/// load-bearing in general: it bounds the PRE-CHOKE macro-DTO extraction
/// (`extract_component_meta_from_resolved` → `component_meta_resolved_macros` →
/// `vue_macro_dtos_with_ctx`), which the fallthrough choke does NOT cover —
/// discriminated by
/// `session_pre_choke_macro_dto_budget_partial_not_admitted_to_vue_surface_store`.
///
/// The discriminating budget is MEASURED, not hard-coded: `R` (resolve-only)
/// and `S = C - R` (fallthrough) are read from the shared projection-op
/// counter under a generous ambient context (the session via-view entry is
/// install-if-none, so it inherits and charges that context's budget). The
/// budget is set to `max(R, S)`: resolve alone (`R <= K`) and fallthrough
/// alone (`S <= K`) each stay within it, but the combined cold work
/// (`C = R + S > K`) trips a SHARED budget.
#[cfg(not(target_arch = "wasm32"))]
#[test]
fn combined_resolve_plus_fallthrough_budget_partial_not_warmed_through_session_view_surfaces() {
    use crate::resolver_core::FallthroughRequestHost;
    use std::sync::atomic::Ordering::Relaxed;

    let canonical = "/src/App.vue";

    // Measure a fixture's cold combined projection-op count under a GENEROUS
    // budget: install an OUTER request context (install-if-none in the
    // session via-view entry inherits it, charging ITS budget Arc — shared
    // across any worker that propagates it) and read the counter back.
    let measure = |upsert: &dyn Fn(&Arc<MetaProject>)| -> usize {
        let project = make_project_with_config(HostConfig {
            analysis_level: crate::types::AnalysisLevel::Full,
            projection_op_budget: 0, // generous (effective 2000)
            ..HostConfig::default()
        });
        upsert(&project);
        let host = project.host();
        let ctx = verter_type_engine::request_context::RequestContext::with_kind_timing_and_projection_budget(
            host.next_request_id(),
            Arc::<str>::from(canonical),
            verter_audit::RequestKind::ComponentMeta,
            false,
            false,
            None,
            0,
        );
        let budget = Arc::clone(&ctx.projection_budget);
        let _guard = verter_type_engine::request_context::RequestContextGuard::install(ctx);
        let session = project.open_session_batch().unwrap();
        let _ = session
            .get_component_meta_with_resolution(canonical)
            .expect("the generous-budget measurement request must succeed");
        budget.projection_ops_executed_count()
    };

    let r_ops = measure(&upsert_mixed_budget_resolve_only_owner);
    let c_ops = measure(&upsert_mixed_budget_owner);
    let s_ops = c_ops.saturating_sub(r_ops);

    assert!(
        r_ops >= 1,
        "the resolve phase must charge >=1 projection op — else the fixture is fallthrough-only \
         and the OUTER install is not load-bearing (the choke alone would suffice); got R={r_ops}"
    );
    assert!(
        s_ops >= 1,
        "the fallthrough phase must charge >=1 projection op; got C={c_ops} R={r_ops} S={s_ops}"
    );

    // K = max(R, S): neither phase alone exceeds it, but the SHARED combined
    // work (C = R + S) does. Each surface runs on its OWN fresh host so a
    // partial from the other surface cannot confound it (each cold run
    // re-charges the full combined work).
    let k = r_ops.max(s_ops);
    assert!(
        c_ops > k,
        "the combined cold work (C={c_ops}) must exceed the shared budget K={k} so a SHARED \
         budget trips while neither phase alone does (R={r_ops} S={s_ops})"
    );

    // ── Surface 1: view-aware `MetaSession::get_component_meta`
    // (`component_meta_entry.rs` outer install). The combined budget partial
    // must NOT warm `ComponentMetaResultDb`: the replay is a cold miss.
    {
        let project = make_project_with_config(HostConfig {
            analysis_level: crate::types::AnalysisLevel::Full,
            projection_op_budget: k,
            ..HostConfig::default()
        });
        upsert_mixed_budget_owner(&project);
        let host = project.host();
        assert!(
            verter_type_engine::request_context::current_request_context().is_none(),
            "test precondition: no ambient request context — the entry installs its own spanning one"
        );
        let session = project.open_session_batch().unwrap();
        let _ = session
            .get_component_meta(canonical)
            .expect("view-aware meta request must succeed")
            .expect("a partial analysis is still RETURNED to the caller, just not warmed");
        let hits_before = host
            .provenance()
            .component_meta_result_cache_hits
            .load(Relaxed);
        let _ = session
            .get_component_meta(canonical)
            .expect("second view-aware request must succeed")
            .expect("the replay is still RETURNED");
        let hits_after = host
            .provenance()
            .component_meta_result_cache_hits
            .load(Relaxed);
        assert_eq!(
            hits_after, hits_before,
            "the view-aware surface's COMBINED resolve+fallthrough budget partial MUST NOT warm \
             `ComponentMetaResultDb` — the replay must be cold (hits_before={hits_before}, \
             hits_after={hits_after}); reverting the OUTER install at `component_meta_entry.rs` \
             splits the work into an inner-resolve budget + a fresh-choke budget (the choke sees the \
             resolve's WARM props), neither trips, the compute is Complete, and the publish gate \
             warms the partial"
        );
    }

    // ── Surface 2 (corroborating): session
    // `MetaSession::get_component_meta_with_resolution`
    // (`component_meta_entry_resolution.rs`). It discards the
    // `ComponentMetaResultDb` publish, so the gated runtime-node `store_node`
    // is the observable. The combined budget partial must NOT warm the
    // top-level fallthrough node. The session path rebuilds the resolver ctx
    // with a cold-seed between resolve and extract, so the choke's
    // `extract_component_meta` re-resolves the full surface COLD within the
    // choke's own budget — the choke alone already bounds this combined work,
    // so Surface 2 corroborates the session path's combined-budget bounding
    // for this fixture; the view-aware Surface 1 is the discriminating half
    // here. The session outer install is independently load-bearing for the
    // PRE-CHOKE macro-DTO extraction the choke does NOT cover, discriminated by
    // `session_pre_choke_macro_dto_budget_partial_not_admitted_to_vue_surface_store`.
    {
        let project = make_project_with_config(HostConfig {
            analysis_level: crate::types::AnalysisLevel::Full,
            projection_op_budget: k,
            ..HostConfig::default()
        });
        upsert_mixed_budget_owner(&project);
        let host = project.host();
        let key = crate::resolver_core::fallthrough_cache_key(
            canonical,
            host.config.generic_root_propagation,
            None,
        );
        let session_wr = project.open_session_batch().unwrap();
        let _ = session_wr
            .get_component_meta_with_resolution(canonical)
            .expect("session with-resolution request must succeed")
            .expect("a partial result is still RETURNED to the caller, just not warmed");
        let view = FallthroughRequestHost::snapshot_store_view(host);
        let s2_node_warmed = host
            .resolver_runtime()
            .fallthrough
            .get_cached_node(&key, &view)
            .is_some();
        assert!(
            !s2_node_warmed,
            "the session with-resolution surface's COMBINED budget partial MUST NOT warm the \
             runtime fallthrough node — the session path bounds the combined resolve+fallthrough \
             work (here via the choke, which re-runs the extract cold under its own budget) and \
             `store_node` refuses the partial"
        );
    }
}

#[cfg(not(target_arch = "wasm32"))]
#[test]
fn fallthrough_runtime_reuse_survives_host_cache_clear() {
    let project = make_project();
    project
        .upsert_base("/Child.vue", r#"<template><input /></template>"#)
        .unwrap();
    project
        .upsert_base(
            "/App.vue",
            r#"<script setup lang="ts">
import Child from './Child.vue'
</script>
<template><Child /></template>"#,
        )
        .unwrap();

    let first = project
        .host()
        .resolve_fallthrough_surface("/App.vue")
        .expect("initial fallthrough resolve should succeed");
    assert!(
        first.accepted_props.iter().any(|prop| prop.name == "value"),
        "initial fallthrough resolve should inherit input attrs from the child"
    );

    clear_legacy_cached_fallthrough_state(&project, "/App.vue");
    project.host().provenance.reset();
    project.host().resolver_runtime().reset_counters();

    let second = project
        .host()
        .resolve_fallthrough_surface("/App.vue")
        .expect("second fallthrough resolve should succeed from runtime-owned cache");
    let runtime = project.host().resolver_runtime().counter_snapshot();
    let provenance = provenance(&project);

    assert!(
        second
            .accepted_props
            .iter()
            .any(|prop| prop.name == "value"),
        "runtime-owned top-level fallthrough should preserve inherited input attrs"
    );
    assert!(
        runtime.node_cache_hits > 0,
        "runtime branch-union nodes should satisfy the top-level lookup after host cache clear, got {:?}",
        runtime
    );
    assert_eq!(
        provenance.resolver_node_cache_hits,
        1,
        "top-level fallthrough should be served from the runtime-owned cache once host caches are cleared"
    );
    assert_eq!(
        provenance.resolver_node_cache_misses,
        0,
        "runtime-owned top-level fallthrough should avoid a host-side miss after host caches are cleared, got provenance={:?}",
        provenance
    );
}

#[cfg(not(target_arch = "wasm32"))]
#[test]
fn top_level_fallthrough_lives_in_runtime_not_host_wrapper_cache() {
    let project = make_project();
    project
        .upsert_base("/Child.vue", r#"<template><input /></template>"#)
        .unwrap();
    project
        .upsert_base(
            "/App.vue",
            r#"<script setup lang="ts">
import Child from './Child.vue'
</script>
<template><Child /></template>"#,
        )
        .unwrap();

    let result = project
        .host()
        .resolve_fallthrough_surface("/App.vue")
        .expect("fallthrough resolve should succeed");
    let key = crate::resolver_core::fallthrough_cache_key(
        "/App.vue",
        project.host().config.generic_root_propagation,
        None,
    );
    assert!(
        result
            .accepted_props
            .iter()
            .any(|prop| prop.name == "value"),
        "resolved fallthrough should inherit input attrs from the child"
    );
    assert!(
        cached_fallthrough_state(&project, "/App.vue").is_some(),
        "legacy compile-cache mirror should still be populated"
    );
    assert!(
        project
            .host()
            .resolver_runtime()
            .fallthrough
            .get_cached_node(&key, &project.host().resolver_store_view_read().into_owned_view())
            .is_some(),
        "top-level fallthrough should live only in runtime nodes once runtime owns top-level authority"
    );
}

#[cfg(not(target_arch = "wasm32"))]
#[test]
fn fallthrough_recomputes_and_reuses_runtime_subnode_after_top_level_node_clear() {
    let project = make_project();
    project
        .upsert_base(
            "/App.vue",
            r#"<script setup lang="ts">
const attrs = { id: 'hero', title: 'Hello' }
</script>
<template><div v-bind="attrs" /></template>"#,
        )
        .unwrap();

    let first = project
        .host()
        .resolve_fallthrough_surface("/App.vue")
        .expect("initial fallthrough resolve should succeed");
    assert!(
        first
            .accepted_props
            .iter()
            .any(|prop| prop.name == "placeholder"),
        "initial fallthrough resolve should include remaining div attrs"
    );
    assert!(
        !first.accepted_props.iter().any(|prop| prop.name == "id"),
        "consumed spread attrs must not leak into inherited attrs"
    );

    clear_legacy_cached_fallthrough_state(&project, "/App.vue");
    clear_runtime_top_level_fallthrough_node(&project, "/App.vue");
    clear_runtime_root_follow_node(&project, "/App.vue");
    project.host().provenance.reset();
    project.host().resolver_runtime().reset_counters();

    let second = project
        .host()
        .resolve_fallthrough_surface("/App.vue")
        .expect("second fallthrough resolve should rebuild from runtime subnodes");
    let runtime = project.host().resolver_runtime().counter_snapshot();

    assert!(
        second
            .accepted_props
            .iter()
            .any(|prop| prop.name == "placeholder"),
        "recomputed fallthrough should preserve remaining div attrs"
    );
    assert!(
        !second.accepted_props.iter().any(|prop| prop.name == "id"),
        "recomputed fallthrough must still treat spread attrs as consumed"
    );
    assert!(
        runtime.node_cache_hits >= 1,
        "recomputing after evicting the top-level and root-follow nodes should reuse an available deeper runtime subnode, got {:?}",
        runtime
    );
}

#[cfg(not(target_arch = "wasm32"))]
#[test]
fn fallthrough_reuses_root_follow_after_branch_union_node_clear() {
    let project = make_project();
    project
        .upsert_base("/App.vue", r#"<template><UnknownRoot /></template>"#)
        .unwrap();

    let first = project
        .host()
        .resolve_fallthrough_surface("/App.vue")
        .expect("initial fallthrough resolve should succeed");
    assert!(
        first.accepted_props.is_empty(),
        "unresolved root should not fabricate inherited attrs"
    );

    clear_legacy_cached_fallthrough_state(&project, "/App.vue");
    clear_runtime_top_level_fallthrough_node(&project, "/App.vue");
    project.host().provenance.reset();
    project.host().resolver_runtime().reset_counters();

    let second = project
        .host()
        .resolve_fallthrough_surface("/App.vue")
        .expect("second fallthrough resolve should rebuild from root-follow and consumed-binding runtime nodes");
    let runtime = project.host().resolver_runtime().counter_snapshot();

    assert!(
        second.accepted_props.is_empty(),
        "recomputed unresolved root should not fabricate inherited attrs"
    );
    assert!(
        runtime.node_cache_hits >= 1,
        "evicting only the branch-union node should still reuse the cached root-follow node, got {:?}",
        runtime
    );
    assert_eq!(
        runtime.node_cache_misses,
        1,
        "only the missing branch-union node should miss once root-follow is runtime-owned, got {:?}",
        runtime
    );
}

/// `type Props = { [k: string]: string }; defineProps<Props>()` — an
/// OWNER-LOCAL NAMED props root whose body is index-signature-only. This
/// exercises a DISTINCT lowering from the inline-literal case
/// (`evaluate_types_define_props_preserves_index_signature_only_surface`): the
/// macro type argument is a `Ref` to a named alias resolved through
/// `ResolveDecl`, not an inline `Object`. The published `define_props` shape
/// MUST still carry the named root's index signature.
///
/// Discriminating: pre-fix `define_props_shape` hardcoded `index_signatures:
/// Vec::new()`, dropping the named root's index signature too; post-fix the
/// DTO's `prop_index_signatures` (raised from the resolved-alias surface)
/// surfaces. Proves the index-sig publication is not specific to the inline
/// object shape.
#[test]
fn evaluate_types_owner_local_index_signature_only_props_root_resolves() {
    let project = make_project();
    project
        .upsert_base(
            "/OwnerLocalIndex.vue",
            r#"<script setup lang="ts">
type Props = { [key: string]: number }
defineProps<Props>()
</script>
<template><div /></template>"#,
        )
        .unwrap();

    let session = project.open_session_batch().unwrap();
    let evaluated = session
        .evaluate_types("/OwnerLocalIndex.vue")
        .unwrap()
        .unwrap();

    let shape = evaluated
        .define_props
        .iter()
        .map(|entry| &entry.result.value)
        .next()
        .expect("an owner-local index-signature-only props root must publish a define_props shape");

    assert_eq!(
        shape.index_signatures.len(),
        1,
        "owner-local `type Props = {{ [k: string]: number }}` must publish its \
         index signature, got {} (a dropped index-sig-only root means the \
         surface-shape projector gated it out before the owner-local gate)",
        shape.index_signatures.len(),
    );
    let sig = &shape.index_signatures[0];
    let value_ty = demand_published_type(
        project.host(),
        "/OwnerLocalIndex.vue",
        sig.value_type.present(),
        "index signature value",
    );
    assert!(
        matches!(value_ty, TypeExpr::Primitive(PrimitiveName::Number)),
        "index signature value type is `number`, got {value_ty:?}",
    );
}

/// Directly discriminates the surface-shape projector gate fix: the cold
/// resolver's owner-local authority gate (`owner_local_macro_root_has_surface`)
/// resolves the named root through the shared dispatch surface projection
/// and returns `false` when that projection yields no shape. The projector
/// previously gated on `properties / call_signatures` only and returned `None`
/// for an index-signature-only surface, so the gate reported "no surface" for
/// an index-sig-only owner-local props root — dropping its authoritative
/// `ResolvedMacroMeta` entry (slot-binding / Rule-5 PublishedField provenance).
///
/// Discriminating: with the projector gating on index signatures too, the gate
/// returns `true` for `type Props = { [k: string]: string }`; reverting that
/// `|| !shape.index_signatures.is_empty()` admission makes this assertion fail
/// (the gate reports no surface and the authoritative entry is skipped).
#[test]
fn owner_local_index_signature_only_props_root_passes_authority_gate() {
    use crate::host_manage::jsdoc_resolve::HostComponentMetaResolver;
    use crate::resolver_core::component_meta::ComponentMetaResolverHost;
    use verter_session_query::analysis::types::AnalyzedMacroKind;

    let project = make_project();
    project
        .upsert_base(
            "/GateIndex.vue",
            r#"<script setup lang="ts">
type Props = { [key: string]: string }
defineProps<Props>()
</script>
<template><div /></template>"#,
        )
        .unwrap();
    // Prime the SFC's IndexedReady so the owner-local lowering can resolve the
    // local `Props` alias.
    let _ = project
        .open_session_batch()
        .unwrap()
        .evaluate_types("/GateIndex.vue")
        .unwrap()
        .unwrap();

    let host = project.host();
    let dispatch =
        verter_type_engine::project_semantic_dispatch::ProjectSemanticDispatch::new(host);
    let resolver_host = HostComponentMetaResolver {
        host,
        ctx: host,
        engine: crate::host_manage::jsdoc_resolve::ComponentMetaSemanticServices {
            dispatch: &dispatch,
            session_view: None,
        },
    };
    assert!(
        resolver_host.owner_local_macro_root_has_surface(
            "/GateIndex.vue",
            verter_type_expr::TopLevelOwnerId::instance(0),
            "Props",
            AnalyzedMacroKind::DefineProps,
        ),
        "the owner-local authority gate MUST report a surface for an \
         index-signature-only props root `type Props = {{ [k: string]: string }}`; \
         a `false` here means the surface-shape projector dropped the \
         index-sig-only shape before the gate counted its index signatures",
    );
}

/// TRAP 1 (construct-signature fold) for the node-domain owner-local presence
/// gate: a root whose ONLY surface member is a CONSTRUCT signature
/// (`type Props = { new (): { x: number } }`) is a non-empty props surface.
///
/// The prior materialised reader gated on `ExpandedObjectShape.call_signatures`,
/// into which `type_expr_to_object_shape` FOLDS construct signatures — so a
/// construct-signature-only root read non-empty. The node-domain `SurfaceView`
/// keeps `call_signatures` and `construct_signatures` SEPARATE, so the props gate
/// must OR `construct_signatures` to preserve that semantics.
///
/// Discriminating: removing the `|| !view.construct_signatures.is_empty()` clause
/// from `owner_local_macro_root_surface_presence` makes this gate report `false`
/// for a construct-signature-only root (proven by mutation — the construct sig is
/// the surface's only member, so dropping the clause leaves every OR term false).
#[test]
fn owner_local_macro_root_construct_signature_only_passes_props_gate() {
    use crate::host_manage::jsdoc_resolve::HostComponentMetaResolver;
    use crate::resolver_core::component_meta::ComponentMetaResolverHost;
    use verter_session_query::analysis::types::AnalyzedMacroKind;

    let project = make_project();
    project
        .upsert_base(
            "/CtorRoot.vue",
            r#"<script setup lang="ts">
type Props = { new (): { x: number } }
defineProps<Props>()
</script>
<template><div /></template>"#,
        )
        .unwrap();
    let _ = project
        .open_session_batch()
        .unwrap()
        .evaluate_types("/CtorRoot.vue")
        .unwrap()
        .unwrap();

    let host = project.host();
    let dispatch =
        verter_type_engine::project_semantic_dispatch::ProjectSemanticDispatch::new(host);
    let resolver_host = HostComponentMetaResolver {
        host,
        ctx: host,
        engine: crate::host_manage::jsdoc_resolve::ComponentMetaSemanticServices {
            dispatch: &dispatch,
            session_view: None,
        },
    };
    assert!(
        resolver_host.owner_local_macro_root_has_surface(
            "/CtorRoot.vue",
            verter_type_expr::TopLevelOwnerId::instance(0),
            "Props",
            AnalyzedMacroKind::DefineProps,
        ),
        "a construct-signature-only props root `type Props = {{ new (): {{ x: number }} }}` MUST \
         read as a non-empty macro surface; a `false` here means the node-domain gate dropped \
         the construct signature the materialised reader folded into `call_signatures`",
    );
}

/// Behaviour-preservation for the node-domain owner-local presence gate: an
/// index-signature-only root (`type Root = { [k: string]: string }`) counts as a
/// non-empty PROPS surface but NOT a non-empty EMITS surface — exactly as the
/// prior materialised reader did (props OR-ed `index_signatures`; emits did not).
///
/// Discriminating: the SAME named root yields `true` for `DefineProps` and
/// `false` for `DefineEmits`; folding `index_signatures` into the emits gate (a
/// narrowing bug in the opposite direction) would flip the emits assertion.
#[test]
fn owner_local_macro_root_index_signature_counts_for_props_not_emits() {
    use crate::host_manage::jsdoc_resolve::HostComponentMetaResolver;
    use crate::resolver_core::component_meta::ComponentMetaResolverHost;
    use verter_session_query::analysis::types::AnalyzedMacroKind;

    let project = make_project();
    project
        .upsert_base(
            "/IdxKind.vue",
            r#"<script setup lang="ts">
type Root = { [key: string]: string }
defineProps<Root>()
</script>
<template><div /></template>"#,
        )
        .unwrap();
    let _ = project
        .open_session_batch()
        .unwrap()
        .evaluate_types("/IdxKind.vue")
        .unwrap()
        .unwrap();

    let host = project.host();
    let dispatch =
        verter_type_engine::project_semantic_dispatch::ProjectSemanticDispatch::new(host);
    let resolver_host = HostComponentMetaResolver {
        host,
        ctx: host,
        engine: crate::host_manage::jsdoc_resolve::ComponentMetaSemanticServices {
            dispatch: &dispatch,
            session_view: None,
        },
    };
    assert!(
        resolver_host.owner_local_macro_root_has_surface(
            "/IdxKind.vue",
            verter_type_expr::TopLevelOwnerId::instance(0),
            "Root",
            AnalyzedMacroKind::DefineProps,
        ),
        "an index-signature-only root MUST read as a non-empty props surface (props ORs \
         index signatures)",
    );
    assert!(
        !resolver_host.owner_local_macro_root_has_surface(
            "/IdxKind.vue",
            verter_type_expr::TopLevelOwnerId::instance(0),
            "Root",
            AnalyzedMacroKind::DefineEmits,
        ),
        "an index-signature-only root MUST NOT read as a non-empty emits surface — the emits \
         gate counts members / call / construct signatures only, never index signatures",
    );
}

/// Behaviour-preservation for the node-domain owner-local presence gate: a
/// call-signature-only root (`type Root = { (): void }`) is a non-empty PROPS /
/// SLOTS surface (props ORs call signatures) but NOT a non-empty EXPOSE surface
/// (expose publishes named members only — `exposed_from_typeinfo_surface`).
///
/// Discriminating: the SAME named root yields `true` for `DefineProps` and
/// `false` for `DefineExpose`; ORing `call_signatures` into the expose gate would
/// flip the expose assertion.
#[test]
fn owner_local_macro_root_call_signature_only_props_yes_expose_no() {
    use crate::host_manage::jsdoc_resolve::HostComponentMetaResolver;
    use crate::resolver_core::component_meta::ComponentMetaResolverHost;
    use verter_session_query::analysis::types::AnalyzedMacroKind;

    let project = make_project();
    project
        .upsert_base(
            "/CallRoot.vue",
            r#"<script setup lang="ts">
type Root = { (): void }
defineProps<Root>()
</script>
<template><div /></template>"#,
        )
        .unwrap();
    let _ = project
        .open_session_batch()
        .unwrap()
        .evaluate_types("/CallRoot.vue")
        .unwrap()
        .unwrap();

    let host = project.host();
    let dispatch =
        verter_type_engine::project_semantic_dispatch::ProjectSemanticDispatch::new(host);
    let resolver_host = HostComponentMetaResolver {
        host,
        ctx: host,
        engine: crate::host_manage::jsdoc_resolve::ComponentMetaSemanticServices {
            dispatch: &dispatch,
            session_view: None,
        },
    };
    assert!(
        resolver_host.owner_local_macro_root_has_surface(
            "/CallRoot.vue",
            verter_type_expr::TopLevelOwnerId::instance(0),
            "Root",
            AnalyzedMacroKind::DefineProps,
        ),
        "a call-signature-only root MUST read as a non-empty props surface (props ORs call \
         signatures)",
    );
    assert!(
        !resolver_host.owner_local_macro_root_has_surface(
            "/CallRoot.vue",
            verter_type_expr::TopLevelOwnerId::instance(0),
            "Root",
            AnalyzedMacroKind::DefineExpose,
        ),
        "a call-signature-only root MUST NOT read as a non-empty expose surface — expose \
         publishes named members only",
    );
}

#[test]
fn owner_local_macro_root_authority_gate_isolates_same_name_module_and_instance_roots() {
    use crate::host_manage::jsdoc_resolve::HostComponentMetaResolver;
    use crate::resolver_core::component_meta::ComponentMetaResolverHost;
    use verter_session_query::analysis::types::AnalyzedMacroKind;

    let project = make_project();
    project
        .upsert_base(
            "/OwnerGateIsolation.vue",
            r#"<script lang="ts">
type Root = { moduleOnly: string }
</script>
<script setup lang="ts">
type Root = { (): void }
defineProps<Root>()
</script>
<template><div /></template>"#,
        )
        .unwrap();
    let _ = project
        .open_session_batch()
        .unwrap()
        .evaluate_types("/OwnerGateIsolation.vue")
        .unwrap()
        .unwrap();

    let host = project.host();
    let dispatch =
        verter_type_engine::project_semantic_dispatch::ProjectSemanticDispatch::new(host);
    let resolver_host = HostComponentMetaResolver {
        host,
        ctx: host,
        engine: crate::host_manage::jsdoc_resolve::ComponentMetaSemanticServices {
            dispatch: &dispatch,
            session_view: None,
        },
    };
    assert!(resolver_host.owner_local_macro_root_has_surface(
        "/OwnerGateIsolation.vue",
        verter_type_expr::TopLevelOwnerId::module(0),
        "Root",
        AnalyzedMacroKind::DefineExpose,
    ));
    assert!(resolver_host.owner_local_macro_root_has_surface(
        "/OwnerGateIsolation.vue",
        verter_type_expr::TopLevelOwnerId::instance(0),
        "Root",
        AnalyzedMacroKind::DefineProps,
    ));
    assert!(
        !resolver_host.owner_local_macro_root_has_surface(
            "/OwnerGateIsolation.vue",
            verter_type_expr::TopLevelOwnerId::instance(0),
            "Root",
            AnalyzedMacroKind::DefineExpose,
        ),
        "the instance gate must not read the same-name module member surface",
    );
}

/// Constructor demand and expose demand share the `.bindings` lane, so a
/// module-owned constructor-bound `String` and an instance-owned `String`
/// exposure coexist there. End-to-end: the exposure publishes the
/// instance-owned authored body, and the prop keeps the constructor's type.
///
/// This is a positive end-to-end check, NOT the negative control for the
/// `(owner, name)` join. The demand vector's order is not a contract (it
/// reaches the `evaluateTypes` payload verbatim), so this fixture must not
/// force the wrong row to sort first to manufacture a name-only failure.
/// `extract_exposed_matches_resolved_binding_owner_not_first_name`
/// (verter_semantic) builds the colliding lane directly with the module row
/// first and is what actually discriminates a first-name join.
#[test]
fn expose_does_not_inherit_same_spelling_module_constructor_type() {
    let project = make_project();
    project
        .upsert_base(
            "/CtorExposeCollision.vue",
            r#"<script lang="ts">
const String: string = 'module-ctor-shadow'
</script>
<script setup lang="ts">
defineProps({ label: String })
const String = 1
defineExpose({ String })
</script>
<template><div /></template>"#,
        )
        .unwrap();

    let evaluated = project
        .host()
        .evaluate_types("/CtorExposeCollision.vue")
        .expect("evaluated types should exist");
    let string_owners: Vec<_> = evaluated
        .bindings
        .iter()
        .filter(|field| field.name == "String")
        .map(|field| field.owner)
        .collect();
    assert!(
        string_owners.contains(&verter_type_expr::TopLevelOwnerId::module(0))
            && string_owners.contains(&verter_type_expr::TopLevelOwnerId::instance(0)),
        "fixture premise: `.bindings` must contain BOTH same-spelling \
         owners so the join has two lanes to tell apart; got {string_owners:?}"
    );

    let meta = project
        .host()
        .get_component_meta("/CtorExposeCollision.vue")
        .expect("component meta resolves");
    let exposed = meta
        .exposed
        .iter()
        .find(|field| field.name == "String")
        .expect("instance String must still be exposed under its property key");
    match &exposed.type_source {
        verter_type_expr::facts::SourcePosition::Present(
            verter_type_expr::facts::SemanticTypeSource::Authored(
                verter_type_expr::locators::AuthoredBodyLocator::DeclBody(slot),
            ),
        ) => {
            assert_eq!(
                slot.anchor.owner,
                verter_type_expr::TopLevelOwnerId::instance(0),
                "expose must resolve the instance-owned String, not the \
                 module constructor of the same spelling"
            );
            assert_eq!(slot.anchor.symbol.as_ref(), "String");
        }
        other => panic!(
            "expose must publish the instance-owned authored String body, \
             never the module constructor's string type; got {other:?}"
        ),
    }
    assert!(
        !matches!(
            exposed.type_source.present(),
            Some(verter_type_expr::facts::SemanticTypeSource::Closed(
                verter_type_expr::facts::ClosedTypeFact::Leaf(
                    verter_type_expr::facts::LeafTypeFact::Primitive(PrimitiveName::String)
                )
            ))
        ),
        "the module constructor's closed string type must not type the \
         instance-owned exposure, got {:?}",
        exposed.type_source
    );

    let (analysis, _resolution, types) = project
        .host()
        .get_component_meta_output("/CtorExposeCollision.vue")
        .expect("component-meta output should materialize")
        .expect("component should resolve")
        .into_parts();
    let lanes = types.into_lanes();
    let label_idx = analysis
        .props
        .iter()
        .position(|prop| prop.name == "label")
        .expect("constructor-bound label prop must still publish");
    assert_eq!(
        published_type(&lanes.props[label_idx]),
        &TypeExpr::Primitive(PrimitiveName::String),
        "the module constructor still types the prop — the expose join \
         must not have stolen or inverted that lane"
    );
}

// @ai-generated - Reproduces imported Pick<VueButtonHTMLAttributes, ...> heritage surviving through generic wrapper Omit chains.
#[test]
fn get_component_meta_keeps_imported_picked_button_form_attrs_through_generic_wrapper_omits() {
    let ws = Arc::new(verter_workspace::MemoryWorkspace::new(
        verter_workspace::MemoryOptions::default(),
    ));
    ws.inject_file(
        "/workspace/src/runtime/types/index.ts".to_string(),
        Arc::from("export * from '../components/SelectMenu.vue'\nexport * from '../icons'\nexport * from './input'\n"),
    );
    ws.inject_file(
        "/workspace/src/runtime/icons.ts".to_string(),
        Arc::from(
            r#"export interface UseComponentIconsProps {
  icon?: string
  loading?: boolean
}"#,
        ),
    );
    ws.inject_file(
        "/workspace/src/runtime/vue-dom.ts".to_string(),
        Arc::from(
            r#"export interface VueButtonHTMLAttributes {
  autofocus?: boolean
  disabled?: boolean
  form?: string
  formaction?: string
  formenctype?: string
  formmethod?: string
  formnovalidate?: boolean
  formtarget?: string
  name?: string
  type?: 'button' | 'submit'
}"#,
        ),
    );
    ws.inject_file(
        "/workspace/src/runtime/types/html.ts".to_string(),
        Arc::from(
            r#"import type { VueButtonHTMLAttributes } from '../vue-dom'

export type ButtonHTMLAttributes = Pick<VueButtonHTMLAttributes, 'autofocus' | 'disabled' | 'form' | 'formaction' | 'formenctype' | 'formmethod' | 'formnovalidate' | 'formtarget' | 'name' | 'type'>
"#,
        ),
    );
    ws.inject_file(
        "/workspace/src/runtime/types/utils.ts".to_string(),
        Arc::from(
            r#"export type ArrayOrNested<T> = T[]
export type GetItemKeys<T> = string
"#,
        ),
    );
    ws.inject_file(
        "/workspace/src/runtime/types/input.ts".to_string(),
        Arc::from(
            r#"export interface InputProps {
  modelValue?: string
  placeholder?: string
}"#,
        ),
    );
    ws.inject_file(
        "/workspace/src/runtime/components/SelectMenu.vue".to_string(),
        Arc::from(
            r#"<script lang="ts">
import type { InputProps, UseComponentIconsProps } from '../types'
import type { ButtonHTMLAttributes } from '../types/html'
import type { ArrayOrNested, GetItemKeys } from '../types/utils'

export type SelectMenuItem = {
  label?: string
  value?: string
}

export interface SelectMenuProps<
  T extends ArrayOrNested<SelectMenuItem> = ArrayOrNested<SelectMenuItem>,
  VK extends GetItemKeys<T> | undefined = undefined,
  M extends boolean = false
> extends UseComponentIconsProps, Omit<ButtonHTMLAttributes, 'type' | 'disabled' | 'name'> {
  disabled?: boolean
  name?: string
  open?: boolean
  searchInput?: boolean | Omit<InputProps, 'modelValue'>
  valueKey?: VK
  items?: T
  modelValue?: M extends true ? T : SelectMenuItem
}
</script>
<template><div /></template>"#,
        ),
    );
    ws.inject_file(
        "/workspace/src/runtime/components/ColorModeSelect.vue".to_string(),
        Arc::from(
            r#"<script lang="ts">
import type { SelectMenuProps, SelectMenuItem } from '../types'

export interface ColorModeSelectProps extends Omit<SelectMenuProps<SelectMenuItem[]>, 'icon' | 'items' | 'modelValue'> {
}
</script>

<script setup lang="ts">
defineProps<ColorModeSelectProps>()
</script>
<template><div /></template>"#,
        ),
    );

    let project = make_workspace_project(Arc::clone(&ws));
    assert!(
        project
            .ensure_loaded("/workspace/src/runtime/components/ColorModeSelect.vue")
            .unwrap(),
        "workspace owner should load into the shared base project"
    );

    let meta = get_meta(
        &project,
        "/workspace/src/runtime/components/ColorModeSelect.vue",
    );
    let prop_names: Vec<&str> = meta.props.iter().map(|prop| prop.name.as_str()).collect();
    assert!(
        prop_names.contains(&"form")
            && prop_names.contains(&"formaction")
            && prop_names.contains(&"formenctype")
            && prop_names.contains(&"formmethod")
            && prop_names.contains(&"formnovalidate")
            && prop_names.contains(&"formtarget"),
        "picked button form attrs should survive generic wrapper omits, got: {prop_names:?}"
    );
}

// @ai-generated - Reproduces Pick<VueButtonHTMLAttributes, ...> form attrs disappearing when the source alias comes from a package import.
#[test]
fn get_component_meta_keeps_picked_package_button_form_attrs_through_generic_wrapper_omits() {
    let project = make_project();
    project
        .upsert_base(
            "/node_modules/vue/index.d.ts",
            r#"export interface ButtonHTMLAttributes {
  autofocus?: boolean
  disabled?: boolean
  form?: string
  formaction?: string
  formenctype?: string
  formmethod?: string
  formnovalidate?: boolean
  formtarget?: string
  name?: string
  type?: 'button' | 'submit'
}"#,
        )
        .unwrap();
    project
        .upsert_base(
            "/src/runtime/types/index.ts",
            "export * from '../components/SelectMenu.vue'\nexport * from '../icons'\nexport * from './input'\n",
        )
        .unwrap();
    project
        .upsert_base(
            "/src/runtime/icons.ts",
            r#"export interface UseComponentIconsProps {
  icon?: string
  loading?: boolean
}"#,
        )
        .unwrap();
    project
        .upsert_base(
            "/src/runtime/types/input.ts",
            r#"export interface InputProps {
  modelValue?: string
  placeholder?: string
}"#,
        )
        .unwrap();
    project
        .upsert_base(
            "/src/runtime/types/html.ts",
            r#"import type { ButtonHTMLAttributes as VueButtonHTMLAttributes } from 'vue'

export type ButtonHTMLAttributes = Pick<VueButtonHTMLAttributes, 'autofocus' | 'disabled' | 'form' | 'formaction' | 'formenctype' | 'formmethod' | 'formnovalidate' | 'formtarget' | 'name' | 'type'>
"#,
        )
        .unwrap();
    project
        .upsert_base(
            "/src/runtime/types/utils.ts",
            r#"export type ArrayOrNested<T> = T[]
export type GetItemKeys<T> = string
"#,
        )
        .unwrap();
    project
        .upsert_base(
            "/src/runtime/components/SelectMenu.vue",
            r#"<script lang="ts">
import type { InputProps, UseComponentIconsProps } from '../types'
import type { ButtonHTMLAttributes } from '../types/html'
import type { ArrayOrNested, GetItemKeys } from '../types/utils'

export type SelectMenuItem = {
  label?: string
  value?: string
}

export interface SelectMenuProps<
  T extends ArrayOrNested<SelectMenuItem> = ArrayOrNested<SelectMenuItem>,
  VK extends GetItemKeys<T> | undefined = undefined,
  M extends boolean = false
> extends UseComponentIconsProps, Omit<ButtonHTMLAttributes, 'type' | 'disabled' | 'name'> {
  disabled?: boolean
  name?: string
  open?: boolean
  searchInput?: boolean | Omit<InputProps, 'modelValue'>
  valueKey?: VK
  items?: T
  modelValue?: M extends true ? T : SelectMenuItem
}
</script>
<template><div /></template>"#,
        )
        .unwrap();
    project
        .upsert_base(
            "/src/runtime/components/color-mode/ColorModeSelect.vue",
            r#"<script lang="ts">
import type { SelectMenuProps, SelectMenuItem } from '../../types'

export interface ColorModeSelectProps extends Omit<SelectMenuProps<SelectMenuItem[]>, 'icon' | 'items' | 'modelValue'> {
}
</script>

<script setup lang="ts">
defineProps<ColorModeSelectProps>()
</script>
<template><div /></template>"#,
        )
        .unwrap();

    project.host().set_import_dependencies(
        "/src/runtime/types/html.ts",
        vec![crate::types::DependencyResolution {
            specifier: "vue".to_string(),
            resolved_canonical_id: Some("/node_modules/vue/index.d.ts".to_string()),
            possible_canonical_ids: Vec::new(),
        }],
    );

    let meta = get_meta(
        &project,
        "/src/runtime/components/color-mode/ColorModeSelect.vue",
    );
    let prop_names: Vec<&str> = meta.props.iter().map(|prop| prop.name.as_str()).collect();
    assert!(
        prop_names.contains(&"loading")
            && prop_names.contains(&"disabled")
            && prop_names.contains(&"name")
            && prop_names.contains(&"open")
            && prop_names.contains(&"searchInput")
            && prop_names.contains(&"valueKey"),
        "package wrapper should preserve declared props, got: {prop_names:?}"
    );
}

// @ai-generated - Reproduces package-imported Pick<VueButtonHTMLAttributes, ...> heritage surviving through a cyclic barrel that also re-exports the wrapper component.
#[test]
fn get_component_meta_keeps_picked_package_button_form_attrs_through_cyclic_barrel_wrapper_omits() {
    let project = make_project();
    project
        .upsert_base(
            "/node_modules/vue/index.d.ts",
            r#"export interface ButtonHTMLAttributes {
  autofocus?: boolean
  disabled?: boolean
  form?: string
  formaction?: string
  formenctype?: string
  formmethod?: string
  formnovalidate?: boolean
  formtarget?: string
  name?: string
  type?: 'button' | 'submit'
}"#,
        )
        .unwrap();
    project
        .upsert_base(
            "/src/runtime/types/index.ts",
            r#"export * from '../components/SelectMenu.vue'
export * from '../components/color-mode/ColorModeSelect.vue'
export * from '../icons'
export * from './input'
"#,
        )
        .unwrap();
    project
        .upsert_base(
            "/src/runtime/icons.ts",
            r#"export interface UseComponentIconsProps {
  icon?: string
  loading?: boolean
}"#,
        )
        .unwrap();
    project
        .upsert_base(
            "/src/runtime/types/input.ts",
            r#"export interface InputProps {
  modelValue?: string
  placeholder?: string
}"#,
        )
        .unwrap();
    project
        .upsert_base(
            "/src/runtime/types/html.ts",
            r#"import type { ButtonHTMLAttributes as VueButtonHTMLAttributes } from 'vue'

export type ButtonHTMLAttributes = Pick<VueButtonHTMLAttributes, 'autofocus' | 'disabled' | 'form' | 'formaction' | 'formenctype' | 'formmethod' | 'formnovalidate' | 'formtarget' | 'name' | 'type'>
"#,
        )
        .unwrap();
    project
        .upsert_base(
            "/src/runtime/types/utils.ts",
            r#"export type ArrayOrNested<T> = T[]
export type GetItemKeys<T> = string
"#,
        )
        .unwrap();
    project
        .upsert_base(
            "/src/runtime/components/SelectMenu.vue",
            r#"<script lang="ts">
import type { InputProps, UseComponentIconsProps } from '../types'
import type { ButtonHTMLAttributes } from '../types/html'
import type { ArrayOrNested, GetItemKeys } from '../types/utils'

export type SelectMenuItem = {
  label?: string
  value?: string
}

export interface SelectMenuProps<
  T extends ArrayOrNested<SelectMenuItem> = ArrayOrNested<SelectMenuItem>,
  VK extends GetItemKeys<T> | undefined = undefined,
  M extends boolean = false
> extends UseComponentIconsProps, Omit<ButtonHTMLAttributes, 'type' | 'disabled' | 'name'> {
  disabled?: boolean
  name?: string
  open?: boolean
  searchInput?: boolean | Omit<InputProps, 'modelValue'>
  valueKey?: VK
  items?: T
  modelValue?: M extends true ? T : SelectMenuItem
}
</script>
<template><div /></template>"#,
        )
        .unwrap();
    project
        .upsert_base(
            "/src/runtime/components/color-mode/ColorModeSelect.vue",
            r#"<script lang="ts">
import type { SelectMenuProps, SelectMenuItem } from '../../types'

export interface ColorModeSelectProps extends Omit<SelectMenuProps<SelectMenuItem[]>, 'icon' | 'items' | 'modelValue'> {
}
</script>

<script setup lang="ts">
defineProps<ColorModeSelectProps>()
</script>
<template><div /></template>"#,
        )
        .unwrap();

    project.host().set_import_dependencies(
        "/src/runtime/types/html.ts",
        vec![crate::types::DependencyResolution {
            specifier: "vue".to_string(),
            resolved_canonical_id: Some("/node_modules/vue/index.d.ts".to_string()),
            possible_canonical_ids: Vec::new(),
        }],
    );
    project.host().set_import_dependencies(
        "/src/runtime/components/color-mode/ColorModeSelect.vue",
        vec![crate::types::DependencyResolution {
            specifier: "../../types".to_string(),
            resolved_canonical_id: Some("/src/runtime/types/index.ts".to_string()),
            possible_canonical_ids: Vec::new(),
        }],
    );
    project.host().set_import_dependencies(
        "/src/runtime/components/SelectMenu.vue",
        vec![
            crate::types::DependencyResolution {
                specifier: "../types".to_string(),
                resolved_canonical_id: Some("/src/runtime/types/index.ts".to_string()),
                possible_canonical_ids: Vec::new(),
            },
            crate::types::DependencyResolution {
                specifier: "../types/html".to_string(),
                resolved_canonical_id: Some("/src/runtime/types/html.ts".to_string()),
                possible_canonical_ids: Vec::new(),
            },
            crate::types::DependencyResolution {
                specifier: "../types/utils".to_string(),
                resolved_canonical_id: Some("/src/runtime/types/utils.ts".to_string()),
                possible_canonical_ids: Vec::new(),
            },
        ],
    );
    project.host().set_import_dependencies(
        "/src/runtime/types/index.ts",
        vec![
            crate::types::DependencyResolution {
                specifier: "../components/SelectMenu.vue".to_string(),
                resolved_canonical_id: Some("/src/runtime/components/SelectMenu.vue".to_string()),
                possible_canonical_ids: Vec::new(),
            },
            crate::types::DependencyResolution {
                specifier: "../components/color-mode/ColorModeSelect.vue".to_string(),
                resolved_canonical_id: Some(
                    "/src/runtime/components/color-mode/ColorModeSelect.vue".to_string(),
                ),
                possible_canonical_ids: Vec::new(),
            },
            crate::types::DependencyResolution {
                specifier: "../icons".to_string(),
                resolved_canonical_id: Some("/src/runtime/icons.ts".to_string()),
                possible_canonical_ids: Vec::new(),
            },
            crate::types::DependencyResolution {
                specifier: "./input".to_string(),
                resolved_canonical_id: Some("/src/runtime/types/input.ts".to_string()),
                possible_canonical_ids: Vec::new(),
            },
        ],
    );

    let meta = get_meta(
        &project,
        "/src/runtime/components/color-mode/ColorModeSelect.vue",
    );
    let prop_names: Vec<&str> = meta.props.iter().map(|prop| prop.name.as_str()).collect();
    assert!(
        prop_names.contains(&"loading")
            && prop_names.contains(&"disabled")
            && prop_names.contains(&"name")
            && prop_names.contains(&"open")
            && prop_names.contains(&"searchInput")
            && prop_names.contains(&"valueKey"),
        "cyclic barrel wrapper should preserve declared props, got: {prop_names:?}"
    );
}

// @ai-generated - Reproduces package-imported Pick<VueButtonHTMLAttributes, ...> heritage surviving through a cyclic barrel when defineProps is wrapped in withDefaults().
#[test]
fn get_component_meta_keeps_picked_package_button_form_attrs_through_cyclic_barrel_with_defaults() {
    let project = make_project();
    project
        .upsert_base(
            "/node_modules/vue/index.d.ts",
            r#"export interface ButtonHTMLAttributes {
  autofocus?: boolean
  disabled?: boolean
  form?: string
  formaction?: string
  formenctype?: string
  formmethod?: string
  formnovalidate?: boolean
  formtarget?: string
  name?: string
  type?: 'button' | 'submit'
}

export declare function withDefaults<T, D>(props: T, defaults: D): T & D
"#,
        )
        .unwrap();
    project
        .upsert_base(
            "/src/runtime/types/index.ts",
            r#"export * from '../components/SelectMenu.vue'
export * from '../components/color-mode/ColorModeSelect.vue'
export * from '../icons'
export * from './input'
"#,
        )
        .unwrap();
    project
        .upsert_base(
            "/src/runtime/icons.ts",
            r#"export interface UseComponentIconsProps {
  icon?: string
  loading?: boolean
}"#,
        )
        .unwrap();
    project
        .upsert_base(
            "/src/runtime/types/input.ts",
            r#"export interface InputProps {
  modelValue?: string
  placeholder?: string
}"#,
        )
        .unwrap();
    project
        .upsert_base(
            "/src/runtime/types/html.ts",
            r#"import type { ButtonHTMLAttributes as VueButtonHTMLAttributes } from 'vue'

export type ButtonHTMLAttributes = Pick<VueButtonHTMLAttributes, 'autofocus' | 'disabled' | 'form' | 'formaction' | 'formenctype' | 'formmethod' | 'formnovalidate' | 'formtarget' | 'name' | 'type'>
"#,
        )
        .unwrap();
    project
        .upsert_base(
            "/src/runtime/types/utils.ts",
            r#"export type ArrayOrNested<T> = T[]
export type GetItemKeys<T> = string
"#,
        )
        .unwrap();
    project
        .upsert_base(
            "/src/runtime/components/SelectMenu.vue",
            r#"<script lang="ts">
import type { InputProps, UseComponentIconsProps } from '../types'
import type { ButtonHTMLAttributes } from '../types/html'
import type { ArrayOrNested, GetItemKeys } from '../types/utils'

export type SelectMenuItem = {
  label?: string
  value?: string
}

export interface SelectMenuProps<
  T extends ArrayOrNested<SelectMenuItem> = ArrayOrNested<SelectMenuItem>,
  VK extends GetItemKeys<T> | undefined = undefined,
  M extends boolean = false
> extends UseComponentIconsProps, Omit<ButtonHTMLAttributes, 'type' | 'disabled' | 'name'> {
  disabled?: boolean
  name?: string
  open?: boolean
  searchInput?: boolean | Omit<InputProps, 'modelValue'>
  valueKey?: VK
  items?: T
  modelValue?: M extends true ? T : SelectMenuItem
}
</script>
<template><div /></template>"#,
        )
        .unwrap();
    project
        .upsert_base(
            "/src/runtime/components/color-mode/ColorModeSelect.vue",
            r#"<script lang="ts">
import type { SelectMenuProps, SelectMenuItem } from '../../types'

export interface ColorModeSelectProps extends Omit<SelectMenuProps<SelectMenuItem[]>, 'icon' | 'items' | 'modelValue'> {
}
</script>

<script setup lang="ts">
import { withDefaults } from 'vue'

const props = withDefaults(defineProps<ColorModeSelectProps>(), {
  searchInput: false
})
</script>
<template><div /></template>"#,
        )
        .unwrap();

    project.host().set_import_dependencies(
        "/src/runtime/types/html.ts",
        vec![crate::types::DependencyResolution {
            specifier: "vue".to_string(),
            resolved_canonical_id: Some("/node_modules/vue/index.d.ts".to_string()),
            possible_canonical_ids: Vec::new(),
        }],
    );
    project.host().set_import_dependencies(
        "/src/runtime/components/color-mode/ColorModeSelect.vue",
        vec![
            crate::types::DependencyResolution {
                specifier: "../../types".to_string(),
                resolved_canonical_id: Some("/src/runtime/types/index.ts".to_string()),
                possible_canonical_ids: Vec::new(),
            },
            crate::types::DependencyResolution {
                specifier: "vue".to_string(),
                resolved_canonical_id: Some("/node_modules/vue/index.d.ts".to_string()),
                possible_canonical_ids: Vec::new(),
            },
        ],
    );
    project.host().set_import_dependencies(
        "/src/runtime/components/SelectMenu.vue",
        vec![
            crate::types::DependencyResolution {
                specifier: "../types".to_string(),
                resolved_canonical_id: Some("/src/runtime/types/index.ts".to_string()),
                possible_canonical_ids: Vec::new(),
            },
            crate::types::DependencyResolution {
                specifier: "../types/html".to_string(),
                resolved_canonical_id: Some("/src/runtime/types/html.ts".to_string()),
                possible_canonical_ids: Vec::new(),
            },
            crate::types::DependencyResolution {
                specifier: "../types/utils".to_string(),
                resolved_canonical_id: Some("/src/runtime/types/utils.ts".to_string()),
                possible_canonical_ids: Vec::new(),
            },
        ],
    );
    project.host().set_import_dependencies(
        "/src/runtime/types/index.ts",
        vec![
            crate::types::DependencyResolution {
                specifier: "../components/SelectMenu.vue".to_string(),
                resolved_canonical_id: Some("/src/runtime/components/SelectMenu.vue".to_string()),
                possible_canonical_ids: Vec::new(),
            },
            crate::types::DependencyResolution {
                specifier: "../components/color-mode/ColorModeSelect.vue".to_string(),
                resolved_canonical_id: Some(
                    "/src/runtime/components/color-mode/ColorModeSelect.vue".to_string(),
                ),
                possible_canonical_ids: Vec::new(),
            },
            crate::types::DependencyResolution {
                specifier: "../icons".to_string(),
                resolved_canonical_id: Some("/src/runtime/icons.ts".to_string()),
                possible_canonical_ids: Vec::new(),
            },
            crate::types::DependencyResolution {
                specifier: "./input".to_string(),
                resolved_canonical_id: Some("/src/runtime/types/input.ts".to_string()),
                possible_canonical_ids: Vec::new(),
            },
        ],
    );

    let meta = get_meta(
        &project,
        "/src/runtime/components/color-mode/ColorModeSelect.vue",
    );
    let prop_names: Vec<&str> = meta.props.iter().map(|prop| prop.name.as_str()).collect();
    assert!(
        prop_names.contains(&"loading")
            && prop_names.contains(&"disabled")
            && prop_names.contains(&"name")
            && prop_names.contains(&"open")
            && prop_names.contains(&"searchInput")
            && prop_names.contains(&"valueKey"),
        "cyclic barrel withDefaults wrapper should preserve declared props, got: {prop_names:?}"
    );
}

// @ai-generated - Reproduces package-imported Pick<VueButtonHTMLAttributes, ...> heritage surviving when the imported generic interface also extends a picked external generic package interface.
#[test]
fn get_component_meta_keeps_picked_package_button_form_attrs_through_external_generic_pick_and_cyclic_barrel(
) {
    let project = make_project();
    project
        .upsert_base(
            "/node_modules/vue/index.d.ts",
            r#"export interface ButtonHTMLAttributes {
  autofocus?: boolean
  disabled?: boolean
  form?: string
  formaction?: string
  formenctype?: string
  formmethod?: string
  formnovalidate?: boolean
  formtarget?: string
  name?: string
  type?: 'button' | 'submit'
}"#,
        )
        .unwrap();
    project
        .upsert_base(
            "/node_modules/reka-ui/index.d.ts",
            r#"export interface ComboboxRootProps<T> {
  open?: boolean
  defaultOpen?: boolean
  disabled?: boolean
  name?: string
  by?: string
  items?: T
}"#,
        )
        .unwrap();
    project
        .upsert_base(
            "/src/runtime/types/index.ts",
            r#"export * from '../components/SelectMenu.vue'
export * from '../components/color-mode/ColorModeSelect.vue'
export * from '../icons'
export * from './input'
"#,
        )
        .unwrap();
    project
        .upsert_base(
            "/src/runtime/icons.ts",
            r#"export interface UseComponentIconsProps {
  icon?: string
  loading?: boolean
}"#,
        )
        .unwrap();
    project
        .upsert_base(
            "/src/runtime/types/input.ts",
            r#"export interface InputProps {
  modelValue?: string
  placeholder?: string
}"#,
        )
        .unwrap();
    project
        .upsert_base(
            "/src/runtime/types/html.ts",
            r#"import type { ButtonHTMLAttributes as VueButtonHTMLAttributes } from 'vue'

export type ButtonHTMLAttributes = Pick<VueButtonHTMLAttributes, 'autofocus' | 'disabled' | 'form' | 'formaction' | 'formenctype' | 'formmethod' | 'formnovalidate' | 'formtarget' | 'name' | 'type'>
"#,
        )
        .unwrap();
    project
        .upsert_base(
            "/src/runtime/types/utils.ts",
            r#"export type ArrayOrNested<T> = T[]
export type GetItemKeys<T> = string
"#,
        )
        .unwrap();
    project
        .upsert_base(
            "/src/runtime/components/SelectMenu.vue",
            r#"<script lang="ts">
import type { ComboboxRootProps } from 'reka-ui'
import type { InputProps, UseComponentIconsProps } from '../types'
import type { ButtonHTMLAttributes } from '../types/html'
import type { ArrayOrNested, GetItemKeys } from '../types/utils'

export type SelectMenuItem = {
  label?: string
  value?: string
}

export interface SelectMenuProps<
  T extends ArrayOrNested<SelectMenuItem> = ArrayOrNested<SelectMenuItem>,
  VK extends GetItemKeys<T> | undefined = undefined,
  M extends boolean = false
> extends Pick<ComboboxRootProps<T>, 'open' | 'defaultOpen' | 'disabled' | 'name' | 'by'>,
    UseComponentIconsProps,
    Omit<ButtonHTMLAttributes, 'type' | 'disabled' | 'name'> {
  searchInput?: boolean | Omit<InputProps, 'modelValue'>
  valueKey?: VK
  items?: T
  modelValue?: M extends true ? T : SelectMenuItem
}
</script>
<template><div /></template>"#,
        )
        .unwrap();
    project
        .upsert_base(
            "/src/runtime/components/color-mode/ColorModeSelect.vue",
            r#"<script lang="ts">
import type { SelectMenuProps, SelectMenuItem } from '../../types'

export interface ColorModeSelectProps extends Omit<SelectMenuProps<SelectMenuItem[]>, 'icon' | 'items' | 'modelValue'> {
}
</script>

<script setup lang="ts">
import { withDefaults } from 'vue'

const props = withDefaults(defineProps<ColorModeSelectProps>(), {
  searchInput: false
})
</script>
<template><div /></template>"#,
        )
        .unwrap();

    project.host().set_import_dependencies(
        "/src/runtime/types/html.ts",
        vec![crate::types::DependencyResolution {
            specifier: "vue".to_string(),
            resolved_canonical_id: Some("/node_modules/vue/index.d.ts".to_string()),
            possible_canonical_ids: Vec::new(),
        }],
    );
    project.host().set_import_dependencies(
        "/src/runtime/components/color-mode/ColorModeSelect.vue",
        vec![
            crate::types::DependencyResolution {
                specifier: "../../types".to_string(),
                resolved_canonical_id: Some("/src/runtime/types/index.ts".to_string()),
                possible_canonical_ids: Vec::new(),
            },
            crate::types::DependencyResolution {
                specifier: "vue".to_string(),
                resolved_canonical_id: Some("/node_modules/vue/index.d.ts".to_string()),
                possible_canonical_ids: Vec::new(),
            },
        ],
    );
    project.host().set_import_dependencies(
        "/src/runtime/components/SelectMenu.vue",
        vec![
            crate::types::DependencyResolution {
                specifier: "reka-ui".to_string(),
                resolved_canonical_id: Some("/node_modules/reka-ui/index.d.ts".to_string()),
                possible_canonical_ids: Vec::new(),
            },
            crate::types::DependencyResolution {
                specifier: "../types".to_string(),
                resolved_canonical_id: Some("/src/runtime/types/index.ts".to_string()),
                possible_canonical_ids: Vec::new(),
            },
            crate::types::DependencyResolution {
                specifier: "../types/html".to_string(),
                resolved_canonical_id: Some("/src/runtime/types/html.ts".to_string()),
                possible_canonical_ids: Vec::new(),
            },
            crate::types::DependencyResolution {
                specifier: "../types/utils".to_string(),
                resolved_canonical_id: Some("/src/runtime/types/utils.ts".to_string()),
                possible_canonical_ids: Vec::new(),
            },
        ],
    );
    project.host().set_import_dependencies(
        "/src/runtime/types/index.ts",
        vec![
            crate::types::DependencyResolution {
                specifier: "../components/SelectMenu.vue".to_string(),
                resolved_canonical_id: Some("/src/runtime/components/SelectMenu.vue".to_string()),
                possible_canonical_ids: Vec::new(),
            },
            crate::types::DependencyResolution {
                specifier: "../components/color-mode/ColorModeSelect.vue".to_string(),
                resolved_canonical_id: Some(
                    "/src/runtime/components/color-mode/ColorModeSelect.vue".to_string(),
                ),
                possible_canonical_ids: Vec::new(),
            },
            crate::types::DependencyResolution {
                specifier: "../icons".to_string(),
                resolved_canonical_id: Some("/src/runtime/icons.ts".to_string()),
                possible_canonical_ids: Vec::new(),
            },
            crate::types::DependencyResolution {
                specifier: "./input".to_string(),
                resolved_canonical_id: Some("/src/runtime/types/input.ts".to_string()),
                possible_canonical_ids: Vec::new(),
            },
        ],
    );

    let meta = get_meta(
        &project,
        "/src/runtime/components/color-mode/ColorModeSelect.vue",
    );
    let prop_names: Vec<&str> = meta.props.iter().map(|prop| prop.name.as_str()).collect();
    assert!(
        prop_names.contains(&"open")
            && prop_names.contains(&"defaultOpen")
            && prop_names.contains(&"disabled")
            && prop_names.contains(&"name")
            && prop_names.contains(&"by")
            && prop_names.contains(&"loading")
            && prop_names.contains(&"searchInput")
            && prop_names.contains(&"valueKey"),
        "external generic pick + cyclic barrel wrapper should preserve declared props, got: {prop_names:?}"
    );
}

#[test]
fn evaluate_types_keeps_reexported_vue_button_form_attrs_through_workspace_generic_wrapper() {
    let ws = Arc::new(verter_workspace::MemoryWorkspace::new(
        verter_workspace::MemoryOptions::default(),
    ));
    ws.inject_file(
        "/workspace/node_modules/vue/package.json".to_string(),
        Arc::from(
            r#"{ "name": "vue", "types": "./dist/vue.d.ts", "exports": { ".": { "types": "./dist/vue.d.ts", "import": "./dist/vue.js" } } }"#,
        ),
    );
    ws.inject_file(
        "/workspace/node_modules/vue/dist/vue.d.ts".to_string(),
        Arc::from("export * from '@vue/runtime-dom'"),
    );
    ws.inject_file(
        "/workspace/node_modules/vue/dist/vue.js".to_string(),
        Arc::from("export const runtimeOnly = true"),
    );
    ws.inject_file(
        "/workspace/node_modules/@vue/runtime-dom/package.json".to_string(),
        Arc::from(
            r#"{ "name": "@vue/runtime-dom", "types": "./dist/runtime-dom.d.ts", "exports": { ".": { "types": "./dist/runtime-dom.d.ts", "import": "./dist/runtime-dom.js" } } }"#,
        ),
    );
    ws.inject_file(
        "/workspace/node_modules/@vue/runtime-dom/dist/runtime-dom.d.ts".to_string(),
        Arc::from(
            r#"export interface HTMLAttributes {
  class?: any
}

export interface ButtonHTMLAttributes extends HTMLAttributes {
  autofocus?: boolean
  disabled?: boolean
  form?: string
  formaction?: string
  formenctype?: string
  formmethod?: string
  formnovalidate?: boolean
  formtarget?: string
  name?: string
  type?: 'button' | 'submit'
}"#,
        ),
    );
    ws.inject_file(
        "/workspace/node_modules/@vue/runtime-dom/dist/runtime-dom.js".to_string(),
        Arc::from("export const runtimeOnly = true"),
    );
    ws.inject_file(
        "/workspace/node_modules/reka-ui/package.json".to_string(),
        Arc::from(
            r#"{ "name": "reka-ui", "types": "./dist/index.d.ts", "exports": { ".": { "types": "./dist/index.d.ts", "import": "./dist/index.js" } } }"#,
        ),
    );
    ws.inject_file(
        "/workspace/node_modules/reka-ui/dist/index.d.ts".to_string(),
        Arc::from(
            r#"export interface ComboboxRootProps<T> {
  open?: boolean
  defaultOpen?: boolean
  disabled?: boolean
  name?: string
  by?: string
  items?: T
}"#,
        ),
    );
    ws.inject_file(
        "/workspace/node_modules/reka-ui/dist/index.js".to_string(),
        Arc::from("export const runtimeOnly = true"),
    );
    ws.inject_file(
        "/workspace/src/runtime/types/index.ts".to_string(),
        Arc::from(
            r#"export * from '../components/SelectMenu.vue'
export * from '../components/color-mode/ColorModeSelect.vue'
export * from '../icons'
export * from './input'
"#,
        ),
    );
    ws.inject_file(
        "/workspace/src/runtime/icons.ts".to_string(),
        Arc::from(
            r#"export interface UseComponentIconsProps {
  icon?: string
  loading?: boolean
}"#,
        ),
    );
    ws.inject_file(
        "/workspace/src/runtime/types/input.ts".to_string(),
        Arc::from(
            r#"export interface InputProps {
  modelValue?: string
  placeholder?: string
}"#,
        ),
    );
    ws.inject_file(
        "/workspace/src/runtime/types/html.ts".to_string(),
        Arc::from(
            r#"import type { ButtonHTMLAttributes as VueButtonHTMLAttributes } from 'vue'

export type ButtonHTMLAttributes = Pick<VueButtonHTMLAttributes, 'autofocus' | 'disabled' | 'form' | 'formaction' | 'formenctype' | 'formmethod' | 'formnovalidate' | 'formtarget' | 'name' | 'type'>
"#,
        ),
    );
    ws.inject_file(
        "/workspace/src/runtime/types/utils.ts".to_string(),
        Arc::from(
            r#"export type ArrayOrNested<T> = T[]
export type GetItemKeys<T> = string
"#,
        ),
    );
    ws.inject_file(
        "/workspace/src/runtime/components/SelectMenu.vue".to_string(),
        Arc::from(
            r#"<script lang="ts">
import type { ComboboxRootProps } from 'reka-ui'
import type { InputProps, UseComponentIconsProps } from '../types'
import type { ButtonHTMLAttributes } from '../types/html'
import type { ArrayOrNested, GetItemKeys } from '../types/utils'

export type SelectMenuItem = {
  label?: string
  value?: string
}

export interface SelectMenuProps<
  T extends ArrayOrNested<SelectMenuItem> = ArrayOrNested<SelectMenuItem>,
  VK extends GetItemKeys<T> | undefined = undefined,
  M extends boolean = false
> extends Pick<ComboboxRootProps<T>, 'open' | 'defaultOpen' | 'disabled' | 'name' | 'by'>,
    UseComponentIconsProps,
    Omit<ButtonHTMLAttributes, 'type' | 'disabled' | 'name'> {
  searchInput?: boolean | Omit<InputProps, 'modelValue'>
  valueKey?: VK
  items?: T
  modelValue?: M extends true ? T : SelectMenuItem
}
</script>
<template><div /></template>"#,
        ),
    );
    ws.inject_file(
        "/workspace/src/runtime/components/color-mode/ColorModeSelect.vue".to_string(),
        Arc::from(
            r#"<script lang="ts">
import type { SelectMenuProps, SelectMenuItem } from '../../types'

export interface ColorModeSelectProps extends Omit<SelectMenuProps<SelectMenuItem[]>, 'icon' | 'items' | 'modelValue'> {
}
</script>

<script setup lang="ts">
import { withDefaults } from 'vue'

const props = withDefaults(defineProps<ColorModeSelectProps>(), {
  searchInput: false
})
</script>
<template><div /></template>"#,
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
    assert!(
        project
            .ensure_loaded("/workspace/src/runtime/components/color-mode/ColorModeSelect.vue")
            .unwrap(),
        "workspace owner should load the wrapper component"
    );

    let session = project.open_session_batch().unwrap();
    let evaluated = session
        .evaluate_types("/workspace/src/runtime/components/color-mode/ColorModeSelect.vue")
        .unwrap()
        .expect("evaluate_types should return a result");

    let define_props = evaluated
        .define_props
        .first()
        .expect("wrapper should produce a defineProps expansion");
    let prop_names: Vec<&str> = define_props
        .result
        .value
        .properties
        .iter()
        .map(|prop| prop.name.as_str())
        .collect();
    assert!(
        prop_names.contains(&"open")
            && prop_names.contains(&"defaultOpen")
            && prop_names.contains(&"disabled")
            && prop_names.contains(&"name")
            && prop_names.contains(&"by")
            && prop_names.contains(&"loading")
            && prop_names.contains(&"searchInput")
            && prop_names.contains(&"valueKey"),
        "workspace evaluate_types should preserve declared wrapper props, got: {prop_names:?}"
    );
}

#[test]
fn evaluate_types_keeps_complex_nuxt_ui_form_attrs_through_wrapper_omits() {
    let ws = Arc::new(verter_workspace::MemoryWorkspace::new(
        verter_workspace::MemoryOptions::default(),
    ));
    ws.inject_file(
        "/workspace/tsconfig.json".to_string(),
        Arc::from(
            r#"{ "compilerOptions": { "module": "esnext", "moduleResolution": "bundler" } }"#,
        ),
    );
    ws.inject_file(
        "/workspace/node_modules/vue/package.json".to_string(),
        Arc::from(
            r#"{ "name": "vue", "type": "module", "exports": { ".": { "types": "./dist/vue.d.mts", "import": "./dist/vue.runtime.mjs" } } }"#,
        ),
    );
    ws.inject_file(
        "/workspace/node_modules/vue/dist/vue.d.mts".to_string(),
        Arc::from(
            r#"export * from '@vue/runtime-dom'
export type VNode = any
export declare function withDefaults<T, D>(props: T, defaults: D): T & D
"#,
        ),
    );
    ws.inject_file(
        "/workspace/node_modules/vue/dist/vue.runtime.mjs".to_string(),
        Arc::from("export const runtimeOnly = true"),
    );
    ws.inject_file(
        "/workspace/node_modules/@vue/runtime-dom/package.json".to_string(),
        Arc::from(
            r#"{ "name": "@vue/runtime-dom", "type": "module", "exports": { ".": { "types": "./dist/runtime-dom.d.ts", "import": "./dist/runtime-dom.mjs" } } }"#,
        ),
    );
    ws.inject_file(
        "/workspace/node_modules/@vue/runtime-dom/dist/runtime-dom.d.ts".to_string(),
        Arc::from(
            r#"export interface HTMLAttributes {
  class?: any
}

export interface ButtonHTMLAttributes extends HTMLAttributes {
  autofocus?: boolean
  disabled?: boolean
  form?: string
  formaction?: string
  formenctype?: string
  formmethod?: string
  formnovalidate?: boolean
  formtarget?: string
  name?: string
  type?: 'button' | 'submit'
}"#,
        ),
    );
    ws.inject_file(
        "/workspace/node_modules/@vue/runtime-dom/dist/runtime-dom.mjs".to_string(),
        Arc::from("export const runtimeOnly = true"),
    );
    ws.inject_file(
        "/workspace/node_modules/reka-ui/package.json".to_string(),
        Arc::from(
            r#"{ "name": "reka-ui", "type": "module", "exports": { ".": { "types": "./dist/index.d.ts", "import": "./dist/index.mjs" } } }"#,
        ),
    );
    ws.inject_file(
        "/workspace/node_modules/reka-ui/dist/index.d.ts".to_string(),
        Arc::from(
            r#"export interface ComboboxRootProps<T> {
  open?: boolean
  defaultOpen?: boolean
  disabled?: boolean
  name?: string
  resetSearchTermOnBlur?: boolean
  resetSearchTermOnSelect?: boolean
  resetModelValueOnClear?: boolean
  highlightOnHover?: boolean
  by?: string
  items?: T
}

export interface ComboboxRootEmits {
  'update:open': [value: boolean]
}

export interface ComboboxContentProps {
  side?: 'bottom' | 'top'
  sideOffset?: number
  collisionPadding?: number
  position?: 'popper' | 'item-aligned'
  as?: string
  asChild?: boolean
  forceMount?: boolean
}

export interface ComboboxContentEmits {
  escapeKeyDown?: [event: KeyboardEvent]
}

export interface ComboboxArrowProps {
  width?: number
  height?: number
  as?: string
  asChild?: boolean
}"#,
        ),
    );
    ws.inject_file(
        "/workspace/node_modules/reka-ui/dist/index.mjs".to_string(),
        Arc::from("export const runtimeOnly = true"),
    );
    ws.inject_file(
        "/workspace/src/runtime/types/input.ts".to_string(),
        Arc::from(
            r#"export interface ModelModifiers {
  trim?: boolean
  number?: boolean
  lazy?: boolean
}

export type ApplyModifiers<T, _Mod> = T
"#,
        ),
    );
    ws.inject_file(
        "/workspace/src/runtime/types/utils.ts".to_string(),
        Arc::from(
            r#"export type AcceptableValue = string | number
export type ArrayOrNested<T> = T[] | T[][]
export type GetItemKeys<T> = string
export type GetItemValue<T, VK> = VK extends string ? string : T
export type GetModelValue<T, VK, M, ExcludeItem> = M extends true
  ? Array<GetItemValue<T, VK>>
  : GetItemValue<T, VK> | ExcludeItem
export type NestedItem<A> = A extends Array<infer U> ? U : never
export type EmitsToProps<T> = T extends object ? { [K in keyof T as K extends string ? `on${Capitalize<K>}` : never]?: T[K] } : {}
"#,
        ),
    );
    ws.inject_file(
        "/workspace/src/runtime/types/tv.ts".to_string(),
        Arc::from(
            r#"export type ComponentConfig<_Theme, _AppConfig, _Name extends string> = {
  variants: {
    color: 'primary' | 'neutral'
    variant: 'outline' | 'ghost'
    size: 'sm' | 'md'
  }
  slots: Record<string, any>,
  ui: Record<string, any>
}
"#,
        ),
    );
    ws.inject_file(
        "/workspace/src/runtime/types/html.ts".to_string(),
        Arc::from(
            r#"import type { ButtonHTMLAttributes as VueButtonHTMLAttributes } from 'vue'

export type ButtonHTMLAttributes = Pick<VueButtonHTMLAttributes, 'autofocus' | 'disabled' | 'form' | 'formaction' | 'formenctype' | 'formmethod' | 'formnovalidate' | 'formtarget' | 'name' | 'type'>
"#,
        ),
    );
    ws.inject_file(
        "/workspace/src/runtime/types/index.ts".to_string(),
        Arc::from(
            r#"export interface UseComponentIconsProps {
  icon?: string
  loading?: boolean
}

export interface AvatarProps {
  src?: string
}

export interface ButtonProps {
  color?: string
  variant?: string
  icon?: string
}

export interface ChipProps {
  color?: string
}

export interface IconProps {
  name?: string
}

export interface InputProps {
  modelValue?: string
  defaultValue?: string
  placeholder?: string
  variant?: string
}

export type LinkPropsKeys = 'href' | 'to'

export * from '../components/SelectMenu.vue'
export * from '../components/color-mode/ColorModeSelect.vue'
export * from './input'
export * from './tv'
export * from './utils'
"#,
        ),
    );
    ws.inject_file(
        "/workspace/src/runtime/components/SelectMenu.vue".to_string(),
        Arc::from(
            r#"<script lang="ts">
import type { ComboboxRootProps, ComboboxRootEmits, ComboboxContentProps, ComboboxContentEmits, ComboboxArrowProps } from 'reka-ui'
import type { VNode } from 'vue'
import type { UseComponentIconsProps } from '../types'
import type { AvatarProps, ButtonProps, ChipProps, IconProps, InputProps, LinkPropsKeys } from '../types'
import type { ModelModifiers, ApplyModifiers } from '../types/input'
import type { ButtonHTMLAttributes } from '../types/html'
import type { AcceptableValue, ArrayOrNested, GetItemKeys, GetModelValue, NestedItem, EmitsToProps } from '../types/utils'
import type { ComponentConfig } from '../types/tv'

type SelectMenu = ComponentConfig<unknown, {}, 'selectMenu'>

export type SelectMenuValue = AcceptableValue

export type SelectMenuItem = SelectMenuValue | {
  label?: string
  description?: string
  icon?: IconProps['name']
  avatar?: AvatarProps
  chip?: ChipProps
  type?: 'label' | 'separator' | 'item'
  disabled?: boolean
  onSelect?: (e: Event) => void
  class?: any
  ui?: Pick<SelectMenu['slots'], 'label' | 'separator' | 'item'>
  [key: string]: any
}

type ExcludeItem = { type: 'label' | 'separator' }
type IsClearUsed<M extends boolean, C extends boolean | object> = M extends false
  ? (C extends true ? null : C extends object ? null : never)
  : never

export interface SelectMenuProps<T extends ArrayOrNested<SelectMenuItem> = ArrayOrNested<SelectMenuItem>, VK extends GetItemKeys<T> | undefined = undefined, M extends boolean = false, Mod extends Omit<ModelModifiers, 'lazy'> = Omit<ModelModifiers, 'lazy'>, C extends boolean | object = false> extends Pick<ComboboxRootProps<T>, 'open' | 'defaultOpen' | 'disabled' | 'name' | 'resetSearchTermOnBlur' | 'resetSearchTermOnSelect' | 'resetModelValueOnClear' | 'highlightOnHover' | 'by'>, UseComponentIconsProps, Omit<ButtonHTMLAttributes, 'type' | 'disabled' | 'name'> {
  id?: string
  placeholder?: string
  searchInput?: boolean | Omit<InputProps, 'modelValue' | 'defaultValue'>
  color?: SelectMenu['variants']['color']
  variant?: SelectMenu['variants']['variant']
  size?: SelectMenu['variants']['size']
  required?: boolean
  trailingIcon?: IconProps['name']
  selectedIcon?: IconProps['name']
  clear?: (C & boolean) | (C & Partial<Omit<ButtonProps, LinkPropsKeys>>)
  clearIcon?: IconProps['name']
  content?: Omit<ComboboxContentProps, 'as' | 'asChild' | 'forceMount'> & Partial<EmitsToProps<ComboboxContentEmits>>
  arrow?: boolean | Omit<ComboboxArrowProps, 'as' | 'asChild'>
  portal?: boolean | string | HTMLElement
  virtualize?: boolean | {
    overscan?: number
    estimateSize?: number | ((index: number) => number)
  }
  valueKey?: VK
  labelKey?: GetItemKeys<T>
  descriptionKey?: GetItemKeys<T>
  items?: T
  defaultValue?: ApplyModifiers<GetModelValue<T, VK, M, ExcludeItem>, Mod> | IsClearUsed<M, C>
  modelValue?: ApplyModifiers<GetModelValue<T, VK, M, ExcludeItem>, Mod> | IsClearUsed<M, C>
  modelModifiers?: Mod
  multiple?: M & boolean
  highlight?: boolean
  createItem?: boolean | 'always' | { position?: 'top' | 'bottom', when?: 'empty' | 'always' }
  filterFields?: string[]
  ignoreFilter?: boolean
  autofocus?: boolean
  autofocusDelay?: number
  class?: any
  ui?: SelectMenu['slots']
}

export interface SelectMenuEmits<
  A extends ArrayOrNested<SelectMenuItem>,
  VK extends GetItemKeys<A> | undefined,
  M extends boolean,
  Mod extends Omit<ModelModifiers, 'lazy'> = Omit<ModelModifiers, 'lazy'>,
  C extends boolean | object = false
> extends Pick<ComboboxRootEmits, 'update:open'> {
  'change': [event: Event]
  'blur': [event: FocusEvent]
  'focus': [event: FocusEvent]
  'create': [item: string]
  'clear': []
  'highlight': [payload: {
    ref: HTMLElement,
    value: ApplyModifiers<GetModelValue<A, VK, M, ExcludeItem>, Mod> | IsClearUsed<M, C>
  } | undefined]
  'update:modelValue': [value: ApplyModifiers<GetModelValue<A, VK, M, ExcludeItem>, Mod> | IsClearUsed<M, C>]
}

type SlotProps<T extends SelectMenuItem> = (props: { item: T, index: number, ui: SelectMenu['ui'] }) => VNode[]

export interface SelectMenuSlots<
  A extends ArrayOrNested<SelectMenuItem> = ArrayOrNested<SelectMenuItem>,
  VK extends GetItemKeys<A> | undefined = undefined,
  M extends boolean = false,
  Mod extends Omit<ModelModifiers, 'lazy'> = Omit<ModelModifiers, 'lazy'>,
  C extends boolean | object = false,
  T extends NestedItem<A> = NestedItem<A>
> {
  'default'?(props: {
    modelValue: ApplyModifiers<GetModelValue<A, VK, M, ExcludeItem>, Mod> | IsClearUsed<M, C>,
    open: boolean
    ui: SelectMenu['ui']
  }): VNode[]
  'item'?: SlotProps<T>
}
</script>

<script setup lang="ts" generic="T extends ArrayOrNested<SelectMenuItem>, VK extends GetItemKeys<T> | undefined = undefined, M extends boolean = false, Mod extends Omit<ModelModifiers, 'lazy'> = Omit<ModelModifiers, 'lazy'>, C extends boolean | object = false">
import { withDefaults } from 'vue'

const props = withDefaults(defineProps<SelectMenuProps<T, VK, M, Mod, C>>(), {
  portal: true,
  searchInput: true,
  labelKey: 'label',
  descriptionKey: 'description',
  resetSearchTermOnBlur: true,
  resetSearchTermOnSelect: true,
  resetModelValueOnClear: true,
  autofocusDelay: 0,
  virtualize: false
})
</script>
<template><div /></template>"#,
        ),
    );
    ws.inject_file(
        "/workspace/src/runtime/components/color-mode/ColorModeSelect.vue".to_string(),
        Arc::from(
            r#"<script lang="ts">
import type { SelectMenuProps, SelectMenuItem } from '../../types'

export interface ColorModeSelectProps extends Omit<SelectMenuProps<SelectMenuItem[]>, 'icon' | 'items' | 'modelValue'> {
}
</script>

<script setup lang="ts">
import { withDefaults } from 'vue'

const props = withDefaults(defineProps<ColorModeSelectProps>(), {
  searchInput: false
})
</script>
<template><div /></template>"#,
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
    assert!(
        project
            .ensure_loaded("/workspace/src/runtime/components/color-mode/ColorModeSelect.vue")
            .unwrap(),
        "workspace owner should load the complex wrapper component"
    );

    let session = project.open_session_batch().unwrap();
    let evaluated = session
        .evaluate_types("/workspace/src/runtime/components/color-mode/ColorModeSelect.vue")
        .unwrap()
        .expect("evaluate_types should return a result");

    let define_props = evaluated
        .define_props
        .first()
        .expect("wrapper should produce a defineProps expansion");
    let prop_names: Vec<&str> = define_props
        .result
        .value
        .properties
        .iter()
        .map(|prop| prop.name.as_str())
        .collect();
    assert!(
        prop_names.contains(&"open")
            && prop_names.contains(&"defaultOpen")
            && prop_names.contains(&"disabled")
            && prop_names.contains(&"name")
            && prop_names.contains(&"loading")
            && prop_names.contains(&"searchInput")
            && prop_names.contains(&"valueKey"),
        "complex Nuxt UI wrapper should preserve declared wrapper props, got: {prop_names:?}"
    );
}

#[test]
fn nested_imported_omit_preserves_html_attrs_and_omits_link_only_keys() {
    let project = make_project();
    project
        .upsert_base(
            "/src/types/html.ts",
            r#"
export interface ButtonHTMLAttributes {
  autofocus?: boolean
  disabled?: boolean
  form?: string
  formaction?: string
  name?: string
  type?: 'button' | 'submit'
}

export interface AnchorHTMLAttributes {
  download?: boolean
  href?: string
  hreflang?: string
  media?: string
  ping?: string
  referrerpolicy?: string
  rel?: string
  target?: string
}
"#,
        )
        .unwrap();
    project
        .upsert_base(
            "/src/Link.vue",
            r#"<script lang="ts">
import type { ButtonHTMLAttributes, AnchorHTMLAttributes } from './types/html'

interface RouterLinkProps {
  replace?: boolean
  exactActiveClass?: string
  viewTransition?: boolean
}

interface NuxtLinkProps extends Omit<RouterLinkProps, 'to'> {
  to?: string
  href?: string
  external?: boolean
  target?: string | null
  rel?: string | null
  noRel?: boolean
  prefetchedClass?: string
  prefetch?: boolean
  prefetchOn?: 'visibility' | 'interaction'
  noPrefetch?: boolean
  trailingSlash?: 'append' | 'remove'
}

export interface LinkProps extends NuxtLinkProps, Omit<ButtonHTMLAttributes, 'type' | 'disabled'>, Omit<AnchorHTMLAttributes, 'href' | 'target' | 'rel' | 'type'> {
  as?: any
  type?: ButtonHTMLAttributes['type']
  disabled?: boolean
  active?: boolean
  exact?: boolean
  exactQuery?: boolean | 'partial'
  exactHash?: boolean
  inactiveClass?: string
  custom?: boolean
  raw?: boolean
  class?: any
}

export type LinkPropsKeys =
  | 'to'
  | 'href'
  | 'target'
  | 'rel'
  | 'noRel'
  | 'external'
  | 'prefetch'
  | 'prefetchOn'
  | 'prefetchedClass'
  | 'noPrefetch'
  | 'trailingSlash'
  | 'replace'
  | 'active'
  | 'exact'
  | 'exactQuery'
  | 'exactHash'
  | 'inactiveClass'
  | 'download'
  | 'ping'
  | 'referrerpolicy'
  | 'hreflang'
  | 'media'
  | 'viewTransition'
</script>
<template><a /></template>"#,
        )
        .unwrap();
    project
        .upsert_base(
            "/src/Button.vue",
            r#"<script lang="ts">
import type { LinkProps } from './types'

export interface UseComponentIconsProps {
  icon?: string
  leading?: boolean
}

export interface ButtonProps extends UseComponentIconsProps, Omit<LinkProps, 'raw' | 'custom'> {
  label?: string
  color?: string
  variant?: string
  size?: 'sm' | 'md'
  square?: boolean
  block?: boolean
  class?: any
  ui?: object
}
</script>
<template><button /></template>"#,
        )
        .unwrap();
    project
        .upsert_base(
            "/src/types/index.ts",
            "export * from '../Link.vue'\nexport * from '../Button.vue'",
        )
        .unwrap();
    project
        .upsert_base(
            "/src/App.vue",
            r#"<script setup lang="ts">
import type { ButtonProps, LinkPropsKeys } from './types'

interface Props extends Omit<ButtonProps, LinkPropsKeys | 'icon' | 'color' | 'variant'> {
  color?: ButtonProps['color']
  variant?: ButtonProps['variant']
  side?: 'left' | 'right'
}

defineProps<Props>()
</script>
<template><div /></template>"#,
        )
        .unwrap();

    let meta = project
        .host()
        .get_component_meta("/src/App.vue")
        .expect("should return component meta");
    let prop_names: Vec<&str> = meta.props.iter().map(|prop| prop.name.as_str()).collect();

    assert!(
        prop_names.contains(&"autofocus")
            && prop_names.contains(&"form")
            && prop_names.contains(&"formaction")
            && prop_names.contains(&"name"),
        "nested imported Omit should preserve inherited button attrs: {:?}",
        prop_names
    );
    assert!(
        !prop_names.contains(&"to")
            && !prop_names.contains(&"href")
            && !prop_names.contains(&"target")
            && !prop_names.contains(&"rel")
            && !prop_names.contains(&"prefetch")
            && !prop_names.contains(&"prefetchOn")
            && !prop_names.contains(&"external")
            && !prop_names.contains(&"viewTransition"),
        "nested imported Omit should exclude link-only keys: {:?}",
        prop_names
    );
}

#[test]
fn dual_heritage_omit_keeps_button_attrs_without_leaking_link_keys() {
    let project = make_project();
    project
        .upsert_base(
            "/src/types/html.ts",
            r#"
export interface ButtonHTMLAttributes {
  autofocus?: boolean
  disabled?: boolean
  form?: string
  formaction?: string
  name?: string
  type?: 'button' | 'submit'
}

export interface AnchorHTMLAttributes {
  download?: boolean
  href?: string
  hreflang?: string
  media?: string
  ping?: string
  referrerpolicy?: string
  rel?: string
  target?: string
}
"#,
        )
        .unwrap();
    project
        .upsert_base(
            "/src/drag.ts",
            r#"
export interface DragHandleProps {
  class?: any
  computePositionConfig?: unknown
  editor?: object
  element?: object
  getReferencedVirtualElement?: () => unknown
  nested?: boolean
  nestedOptions?: object
  onElementDragEnd?: () => void
  onElementDragStart?: () => void
  onNodeChange?: () => void
  pluginKey?: string
}
"#,
        )
        .unwrap();
    project
        .upsert_base(
            "/src/Link.vue",
            r#"<script lang="ts">
import type { ButtonHTMLAttributes, AnchorHTMLAttributes } from './types/html'

interface RouterLinkProps {
  replace?: boolean
  exactActiveClass?: string
  viewTransition?: boolean
}

interface NuxtLinkProps extends Omit<RouterLinkProps, 'to'> {
  to?: string
  href?: string
  external?: boolean
  target?: string | null
  rel?: string | null
  noRel?: boolean
  prefetchedClass?: string
  prefetch?: boolean
  prefetchOn?: 'visibility' | 'interaction'
  noPrefetch?: boolean
  trailingSlash?: 'append' | 'remove'
}

export interface LinkProps extends NuxtLinkProps, Omit<ButtonHTMLAttributes, 'type' | 'disabled'>, Omit<AnchorHTMLAttributes, 'href' | 'target' | 'rel' | 'type'> {
  as?: any
  type?: ButtonHTMLAttributes['type']
  disabled?: boolean
  active?: boolean
  exact?: boolean
  exactQuery?: boolean | 'partial'
  exactHash?: boolean
  inactiveClass?: string
  custom?: boolean
  raw?: boolean
  class?: any
}

export type LinkPropsKeys =
  | 'to'
  | 'href'
  | 'target'
  | 'rel'
  | 'noRel'
  | 'external'
  | 'prefetch'
  | 'prefetchOn'
  | 'prefetchedClass'
  | 'noPrefetch'
  | 'trailingSlash'
  | 'replace'
  | 'active'
  | 'exact'
  | 'exactQuery'
  | 'exactHash'
  | 'inactiveClass'
  | 'download'
  | 'ping'
  | 'referrerpolicy'
  | 'hreflang'
  | 'media'
  | 'viewTransition'
</script>
<template><a /></template>"#,
        )
        .unwrap();
    project
        .upsert_base(
            "/src/Button.vue",
            r#"<script lang="ts">
import type { LinkProps } from './types'

export interface UseComponentIconsProps {
  icon?: string
  leading?: boolean
}

export interface ButtonProps extends UseComponentIconsProps, Omit<LinkProps, 'raw' | 'custom'> {
  label?: string
  color?: string
  variant?: string
  size?: 'sm' | 'md'
  square?: boolean
  block?: boolean
  class?: any
  ui?: object
}
</script>
<template><button /></template>"#,
        )
        .unwrap();
    project
        .upsert_base(
            "/src/types/index.ts",
            "export * from '../Link.vue'\nexport * from '../Button.vue'",
        )
        .unwrap();
    project
        .upsert_base(
            "/src/App.vue",
            r#"<script setup lang="ts">
import type { DragHandleProps } from './drag'
import type { ButtonProps, LinkPropsKeys } from './types'

interface Props extends Omit<DragHandleProps, 'editor' | 'element' | 'onNodeChange' | 'computePositionConfig' | 'class'>, Omit<ButtonProps, LinkPropsKeys | 'icon' | 'color' | 'variant'> {
  color?: ButtonProps['color']
  variant?: ButtonProps['variant']
  options?: object
  editor: object
  ui?: ButtonProps['ui']
}

defineProps<Props>()
</script>
<template><div /></template>"#,
        )
        .unwrap();

    let meta = project
        .host()
        .get_component_meta("/src/App.vue")
        .expect("should return component meta");
    let prop_names: Vec<&str> = meta.props.iter().map(|prop| prop.name.as_str()).collect();

    assert!(
        prop_names.contains(&"autofocus")
            && prop_names.contains(&"form")
            && prop_names.contains(&"formaction")
            && prop_names.contains(&"name"),
        "dual-heritage Omit should preserve inherited button attrs: {:?}",
        prop_names
    );
    assert!(
        !prop_names.contains(&"to")
            && !prop_names.contains(&"href")
            && !prop_names.contains(&"target")
            && !prop_names.contains(&"rel")
            && !prop_names.contains(&"prefetch")
            && !prop_names.contains(&"prefetchOn")
            && !prop_names.contains(&"external")
            && !prop_names.contains(&"viewTransition"),
        "dual-heritage Omit should exclude link-only keys: {:?}",
        prop_names
    );
}

#[test]
fn link_props_keep_inherited_html_attrs_across_vue_ignore_utility_heritage() {
    let project = make_project();
    project.host().notify_upsert(
        "/node_modules/vue-router/package.json",
        Arc::from(r#"{"name":"vue-router","types":"./index.d.ts"}"#),
    );
    project
        .upsert_base(
            "/node_modules/vue-router/index.d.ts",
            r#"
export { R as RouterLinkProps } from './dist/index.js'
"#,
        )
        .unwrap();
    project
        .upsert_base(
            "/node_modules/vue-router/dist/index.d.ts",
            r#"
export interface RouterLinkOptions {
  to?: string
  replace?: boolean
  viewTransition?: boolean
}

export interface R extends RouterLinkOptions {
  activeClass?: string
  exactActiveClass?: string
  ariaCurrentValue?: 'page'
}
"#,
        )
        .unwrap();
    project
        .upsert_base(
            "/src/types/html.ts",
            r#"
export interface ButtonHTMLAttributes {
  autofocus?: boolean
  disabled?: boolean
  form?: string
  formaction?: string
  name?: string
  type?: 'button' | 'submit'
}

export interface AnchorHTMLAttributes {
  download?: boolean
  href?: string
  hreflang?: string
  media?: string
  ping?: string
  referrerpolicy?: string
  rel?: string
  target?: string
}
"#,
        )
        .unwrap();
    project
        .upsert_base(
            "/src/Link.vue",
            r#"<script lang="ts">
import type { ButtonHTMLAttributes, AnchorHTMLAttributes } from './types/html'

interface RouterLinkProps {
  replace?: boolean
}

interface NuxtLinkProps extends Omit<RouterLinkProps, 'to'> {
  to?: string
  href?: string
}

export interface LinkProps extends NuxtLinkProps, /** @vue-ignore */ Omit<ButtonHTMLAttributes, 'type' | 'disabled'>, /** @vue-ignore */ Omit<AnchorHTMLAttributes, 'href' | 'target' | 'rel' | 'type'> {
  as?: any
  type?: ButtonHTMLAttributes['type']
  disabled?: boolean
}
</script>
<script setup lang="ts">
defineProps<LinkProps>()
</script>
<template><a /></template>"#,
        )
        .unwrap();

    let export = project
        .host()
        .resolve_named_export(
            "/node_modules/vue-router/index.d.ts",
            "RouterLinkProps",
            None,
        )
        .expect("package re-export should resolve RouterLinkProps");
    assert_eq!(
        export.source_canonical_id.as_deref(),
        Some("/node_modules/vue-router/dist/index.d.ts")
    );
    assert_eq!(export.source_name, "R");
    let decl = crate::meta_resolve::resolve_type_declaration(
        project.host(),
        "/node_modules/vue-router/index.d.ts",
        "RouterLinkProps",
    );
    assert_eq!(
        decl.canonical_source,
        "/node_modules/vue-router/dist/index.d.ts"
    );
    assert_eq!(decl.resolved_name, "R");

    let meta = project
        .host()
        .get_component_meta("/src/Link.vue")
        .expect("should return component meta");
    let prop_names: Vec<&str> = meta.props.iter().map(|prop| prop.name.as_str()).collect();

    assert!(
        prop_names.contains(&"autofocus")
            && prop_names.contains(&"form")
            && prop_names.contains(&"formaction")
            && prop_names.contains(&"name")
            && prop_names.contains(&"download")
            && prop_names.contains(&"hreflang"),
        "LinkProps should keep inherited HTML attrs across vue-ignore utility heritage: {:?}",
        prop_names
    );
}

#[test]
fn single_native_root_inherits_intrinsic_surface() {
    let project = make_project();
    project
        .upsert_base(
            "/App.vue",
            r#"<script setup lang="ts">
defineProps<{ msg: string }>()
</script>
<template><div>{{ msg }}</div></template>"#,
        )
        .unwrap();

    let meta = get_meta(&project, "/App.vue");

    // Assert+: declared prop is in accepted_props
    assert!(
        meta.accepted_props.iter().any(|p| p.name == "msg"
            && matches!(p.provenance, MemberProvenance::Declared)
            && matches!(p.kind, AcceptedPropKind::DeclaredProp)),
        "accepted_props should contain declared 'msg' prop, got: {:?}",
        meta.accepted_props
            .iter()
            .map(|p| &p.name)
            .collect::<Vec<_>>()
    );

    // Assert+: inherited attrs from div should be present
    assert!(
        meta.accepted_props.iter().any(|p| p.name == "id"
            && matches!(p.provenance, MemberProvenance::Inherited { .. })
            && matches!(p.kind, AcceptedPropKind::Attr)),
        "accepted_props should contain inherited 'id' attr from <div>, got: {:?}",
        meta.accepted_props
            .iter()
            .map(|p| &p.name)
            .collect::<Vec<_>>()
    );

    // Assert+: inherited events from div
    assert!(
        meta.accepted_events.iter().any(|e| e.name == "click"
            && matches!(e.provenance, MemberProvenance::Inherited { .. })
            && matches!(e.kind, AcceptedEventKind::Listener)),
        "accepted_events should contain inherited 'click' listener from <div>, got: {:?}",
        meta.accepted_events
            .iter()
            .map(|e| &e.name)
            .collect::<Vec<_>>()
    );

    // Assert+: surface completeness should be Exact
    assert_eq!(
        meta.accepted_surface_completeness,
        AcceptedSurfaceCompleteness::Exact,
        "completeness should be Exact for a simple native root"
    );

    // Assert+: fallthrough_surface should have branches
    assert!(
        matches!(
            meta.fallthrough_surface,
            FallthroughSurface::Branches { .. }
        ),
        "fallthrough_surface should be Branches, got: {:?}",
        meta.fallthrough_surface
    );

    // Assert-: declared props should NOT appear in fallthrough_surface
    if let FallthroughSurface::Branches { ref branches } = meta.fallthrough_surface {
        assert_eq!(branches.len(), 1, "should have one branch");
        assert!(
            !branches[0].props.iter().any(|p| p.name == "msg"),
            "fallthrough_surface should NOT contain declared 'msg' prop"
        );
        assert_eq!(
            branches[0].status,
            BranchStatus::Resolved,
            "branch status should be Resolved"
        );
        assert!(
            matches!(&branches[0].root_chain[0], ResolvedRootStep::NativeTag { tag } if tag == "div"),
            "root_chain should show NativeTag div"
        );
    }
}

#[test]
fn explicit_root_bindings_are_subtracted() {
    let project = make_project();
    project
        .upsert_base(
            "/App.vue",
            r#"<script setup lang="ts">
defineProps<{ msg: string }>()
</script>
<template><div id="root" @click="() => {}">{{ msg }}</div></template>"#,
        )
        .unwrap();

    let meta = get_meta(&project, "/App.vue");

    // Assert-: explicitly bound 'id' attr should NOT be inherited
    if let FallthroughSurface::Branches { ref branches } = meta.fallthrough_surface {
        assert!(
            !branches[0].props.iter().any(|p| p.name == "id"),
            "consumed 'id' attr should be subtracted from inherited props"
        );
    }

    // Assert-: explicitly bound 'click' listener should NOT be inherited
    if let FallthroughSurface::Branches { ref branches } = meta.fallthrough_surface {
        assert!(
            !branches[0].events.iter().any(|e| e.name == "click"),
            "consumed 'click' listener should be subtracted from inherited events"
        );
    }

    // Assert+: other attrs should still be inherited
    assert!(
        meta.accepted_props.iter().any(
            |p| p.name == "title" && matches!(p.provenance, MemberProvenance::Inherited { .. }),
        ),
        "non-consumed 'title' attr should still be inherited"
    );
}

#[test]
fn declared_on_listener_alias_prop_blocks_inherited_click_listener() {
    let project = make_project();
    project
        .upsert_base(
            "/App.vue",
            r#"<script setup lang="ts">
defineProps<{ onClick?: () => void }>()
</script>
<template><div>hello</div></template>"#,
        )
        .unwrap();

    let meta = get_meta(&project, "/App.vue");

    assert!(
        meta.accepted_props
            .iter()
            .any(|p| p.name == "onClick" && matches!(p.provenance, MemberProvenance::Declared)),
        "declared onClick prop must remain on the accepted prop surface"
    );
    assert!(
        !meta.accepted_events.iter().any(|e| e.name == "click"),
        "declared onClick prop must block the inherited click listener alias"
    );

    if let FallthroughSurface::Branches { ref branches } = meta.fallthrough_surface {
        assert!(
            branches
                .iter()
                .all(|branch| branch.events.iter().all(|event| event.name != "click")),
            "fallthrough branches must not leak click when a declared onClick prop shadows it"
        );
    }
}

#[test]
fn inherit_attrs_false_returns_declared_only_surface() {
    let project = make_project();
    project
        .upsert_base(
            "/App.vue",
            r#"<script setup lang="ts">
defineOptions({ inheritAttrs: false })
defineProps<{ msg: string }>()
</script>
<template><div>{{ msg }}</div></template>"#,
        )
        .unwrap();

    let meta = get_meta(&project, "/App.vue");

    // Assert+: declared prop is present
    assert!(
        meta.accepted_props.iter().any(|p| p.name == "msg"),
        "should have declared 'msg'"
    );

    // Assert-: no inherited members
    assert!(
        !meta
            .accepted_props
            .iter()
            .any(|p| matches!(p.provenance, MemberProvenance::Inherited { .. })),
        "should have no inherited props when inheritAttrs: false"
    );
    assert!(
        !meta
            .accepted_events
            .iter()
            .any(|e| matches!(e.provenance, MemberProvenance::Inherited { .. })),
        "should have no inherited events when inheritAttrs: false"
    );

    // Assert+: fallthrough_surface is None
    assert!(
        matches!(meta.fallthrough_surface, FallthroughSurface::None { .. }),
        "fallthrough_surface should be None when inheritAttrs: false"
    );
}

#[test]
fn unconditional_multi_root_returns_declared_only_surface() {
    let project = make_project();
    project
        .upsert_base(
            "/App.vue",
            r#"<script setup lang="ts">
defineProps<{ msg: string }>()
</script>
<template><div>a</div><span>b</span></template>"#,
        )
        .unwrap();

    let meta = get_meta(&project, "/App.vue");

    // Assert+: declared prop is present
    assert!(
        meta.accepted_props.iter().any(|p| p.name == "msg"),
        "should have declared 'msg'"
    );

    // Assert-: no inherited members
    assert!(
        !meta
            .accepted_props
            .iter()
            .any(|p| matches!(p.provenance, MemberProvenance::Inherited { .. })),
        "multi-root should have no inherited props"
    );

    // Assert+: fallthrough_surface is None
    assert!(
        matches!(meta.fallthrough_surface, FallthroughSurface::None { .. }),
        "fallthrough_surface should be None for multi-root"
    );
}

#[test]
fn conditional_single_root_returns_exact_branches() {
    let project = make_project();
    project
        .upsert_base(
            "/App.vue",
            r#"<script setup lang="ts">
const show = true
defineProps<{ msg: string }>()
</script>
<template>
  <div v-if="show">a</div>
  <input v-else />
</template>"#,
        )
        .unwrap();

    let meta = get_meta(&project, "/App.vue");

    // Assert+: should have branches
    if let FallthroughSurface::Branches { ref branches } = meta.fallthrough_surface {
        assert_eq!(branches.len(), 2, "should have 2 branches (div, input)");

        // Branch 0: div
        assert_eq!(branches[0].branch_key, "0");
        assert!(
            matches!(&branches[0].root_chain[0], ResolvedRootStep::NativeTag { tag } if tag == "div"),
            "first branch should be div"
        );

        // Branch 1: input
        assert_eq!(branches[1].branch_key, "1");
        assert!(
            matches!(&branches[1].root_chain[0], ResolvedRootStep::NativeTag { tag } if tag == "input"),
            "second branch should be input"
        );

        // Assert+: input-specific attrs should be conditional
        // (only in branch 1, not branch 0)
        let input_specific = meta.accepted_props.iter().find(|p| p.name == "type");
        if let Some(p) = input_specific {
            assert!(
                matches!(p.availability, MemberAvailability::Conditional { .. }),
                "'type' attr should be conditional (only in input branch)"
            );
        }
    } else {
        panic!("expected FallthroughSurface::Branches");
    }
}

#[test]
fn static_dynamic_is_root_resolves_native_candidates() {
    let project = make_project();
    project
        .upsert_base(
            "/App.vue",
            r#"<script setup lang="ts">
import Child from './Child.vue'
const showNative = true
</script>
<template><component :is="showNative ? 'div' : Child" /></template>"#,
        )
        .unwrap();
    project
        .upsert_base("/Child.vue", r#"<template><input /></template>"#)
        .unwrap();

    let meta = get_meta(&project, "/App.vue");

    let value_prop = meta
        .accepted_props
        .iter()
        .find(|p| p.name == "value")
        .expect("dynamic :is should propagate the input branch's accepted attrs");
    assert!(
        matches!(
            value_prop.availability,
            MemberAvailability::Conditional { .. }
        ),
        "input-only attrs from dynamic :is candidates must stay conditional"
    );

    if let FallthroughSurface::Branches { ref branches } = meta.fallthrough_surface {
        assert!(
            branches
                .iter()
                .any(|branch| matches!(&branch.root_chain[0], ResolvedRootStep::NativeTag { tag } if tag == "div")),
            "dynamic :is should produce a native div branch"
        );
        assert!(
            branches.iter().any(|branch| {
                branch
                    .root_chain
                    .iter()
                    .any(|step| matches!(step, ResolvedRootStep::Component { component_name, .. } if component_name == "Child"))
            }),
            "dynamic :is should also preserve the imported component branch"
        );
    } else {
        panic!("expected FallthroughSurface::Branches");
    }
}

#[test]
fn root_v_bind_known_object_shape_is_consumed_exactly() {
    let project = make_project();
    project
        .upsert_base(
            "/App.vue",
            r#"<script setup lang="ts">
const rootAttrs = {
  id: 'root',
  onClick: () => {},
}
</script>
<template><div v-bind="rootAttrs" /></template>"#,
        )
        .unwrap();

    let meta = get_meta(&project, "/App.vue");

    assert!(
        !meta.accepted_props.iter().any(|p| p.name == "id"),
        "exact root spread keys must be subtracted from inherited attrs"
    );
    assert!(
        !meta.accepted_events.iter().any(|e| e.name == "click"),
        "exact root spread listener aliases must be subtracted from inherited listeners"
    );
    assert_eq!(
        meta.accepted_surface_completeness,
        AcceptedSurfaceCompleteness::Exact,
        "resolvable root spreads should not force a lower-bound surface"
    );

    if let FallthroughSurface::Branches { ref branches } = meta.fallthrough_surface {
        assert!(
            branches
                .iter()
                .all(|branch| branch.props.iter().all(|prop| prop.name != "id")),
            "spread-consumed attrs must not leak back into fallthrough branches"
        );
        assert!(
            branches
                .iter()
                .all(|branch| branch.events.iter().all(|event| event.name != "click")),
            "spread-consumed listeners must not leak back into fallthrough branches"
        );
        assert!(
            branches
                .iter()
                .all(|branch| matches!(branch.status, BranchStatus::Resolved)),
            "an exact root spread should keep the branch resolved"
        );
    } else {
        panic!("expected FallthroughSurface::Branches");
    }
}

#[test]
fn root_v_bind_unknown_shape_uses_structured_partial_reason() {
    let project = make_project();
    project
        .upsert_base(
            "/App.vue",
            r#"<script setup lang="ts">
const rootAttrs: Record<string, unknown> = {}
</script>
<template><div v-bind="rootAttrs" /></template>"#,
        )
        .unwrap();

    let meta = get_meta(&project, "/App.vue");

    assert_eq!(
        meta.accepted_surface_completeness,
        AcceptedSurfaceCompleteness::LowerBound,
        "unknown root spreads must lower accepted-surface completeness"
    );

    let FallthroughSurface::Branches { branches } = &meta.fallthrough_surface else {
        panic!("expected FallthroughSurface::Branches");
    };

    assert!(
        branches.iter().any(|branch| matches!(
            &branch.status,
            BranchStatus::PartiallyUnresolved { reasons }
                if reasons == &vec![PartialBranchReason::UnknownSpread]
        )),
        "unknown root spreads must surface a structured UnknownSpread reason, got: {:?}",
        branches
            .iter()
            .map(|branch| &branch.status)
            .collect::<Vec<_>>()
    );
}

#[test]
fn generic_root_propagation_off_stays_sound() {
    let project = make_project();
    project
        .upsert_base(
            "/Poly.vue",
            r#"<script setup lang="ts" generic="T extends 'button' | 'input'">
defineProps<{ as: T }>()
</script>
<template><component :is="as" /></template>"#,
        )
        .unwrap();
    project
        .upsert_base(
            "/App.vue",
            r#"<script setup lang="ts">
import Poly from './Poly.vue'
</script>
<template><Poly as="input" /></template>"#,
        )
        .unwrap();

    let meta = get_meta(&project, "/App.vue");

    assert!(
        !meta.accepted_props.iter().any(|prop| prop.name == "value"),
        "generic root propagation disabled must not invent input-only attrs"
    );
    assert_eq!(
        meta.accepted_surface_completeness,
        AcceptedSurfaceCompleteness::LowerBound,
        "an unresolved generic root must remain a lower-bound surface"
    );

    let FallthroughSurface::Branches { branches } = &meta.fallthrough_surface else {
        panic!("expected FallthroughSurface::Branches");
    };
    assert!(
        branches.iter().any(|branch| {
            matches!(
                &branch.status,
                BranchStatus::Unresolved {
                    reason: UnresolvedBranchReason::DynamicComponentIs
                }
            )
        }),
        "without propagation the generic child root should remain unresolved, got: {:?}",
        branches
            .iter()
            .map(|branch| &branch.status)
            .collect::<Vec<_>>()
    );
}

#[test]
fn generic_root_propagation_specializes_dynamic_is_when_enabled() {
    let project = make_project_with_config(HostConfig {
        generic_root_propagation: true,
        ..HostConfig::default()
    });
    project
        .upsert_base(
            "/Poly.vue",
            r#"<script setup lang="ts" generic="T extends 'button' | 'input'">
defineProps<{ as: T }>()
</script>
<template><component :is="as" /></template>"#,
        )
        .unwrap();
    project
        .upsert_base(
            "/App.vue",
            r#"<script setup lang="ts">
import Poly from './Poly.vue'
</script>
<template><Poly as="input" /></template>"#,
        )
        .unwrap();

    let meta = get_meta(&project, "/App.vue");

    let value_prop = meta
        .accepted_props
        .iter()
        .find(|prop| prop.name == "value")
        .expect("generic propagation should specialize the child root to input");
    assert!(
        matches!(value_prop.availability, MemberAvailability::Always),
        "single specialized generic roots should yield always-available attrs"
    );

    let FallthroughSurface::Branches { branches } = &meta.fallthrough_surface else {
        panic!("expected FallthroughSurface::Branches");
    };
    assert!(
        branches.iter().any(|branch| {
            matches!(
                branch.root_chain.as_slice(),
                [
                    ResolvedRootStep::Component { component_name, .. },
                    ResolvedRootStep::NativeTag { tag }
                ] if component_name == "Poly" && tag == "input"
            )
        }),
        "generic propagation should resolve the child root chain to Poly -> input, got: {:?}",
        branches
            .iter()
            .map(|branch| &branch.root_chain)
            .collect::<Vec<_>>()
    );
}

#[test]
fn generic_root_propagation_recurses_through_component_chain() {
    let project = make_project_with_config(HostConfig {
        generic_root_propagation: true,
        ..HostConfig::default()
    });
    project
        .upsert_base(
            "/Poly.vue",
            r#"<script setup lang="ts" generic="T extends 'button' | 'input'">
defineProps<{ as: T }>()
</script>
<template><component :is="as" /></template>"#,
        )
        .unwrap();
    project
        .upsert_base(
            "/Wrapper.vue",
            r#"<script setup lang="ts" generic="T extends 'button' | 'input'">
import Poly from './Poly.vue'
defineProps<{ as: T }>()
</script>
<template><Poly :as="as" /></template>"#,
        )
        .unwrap();
    project
        .upsert_base(
            "/App.vue",
            r#"<script setup lang="ts">
import Wrapper from './Wrapper.vue'
</script>
<template><Wrapper as="input" /></template>"#,
        )
        .unwrap();

    let meta = get_meta(&project, "/App.vue");

    assert!(
        meta.accepted_props.iter().any(|prop| prop.name == "value"),
        "recursive generic propagation should preserve the specialized input attrs through Wrapper"
    );

    let FallthroughSurface::Branches { branches } = &meta.fallthrough_surface else {
        panic!("expected FallthroughSurface::Branches");
    };
    assert!(
        branches.iter().any(|branch| {
            matches!(
                branch.root_chain.as_slice(),
                [
                    ResolvedRootStep::Component { component_name: wrapper_name, .. },
                    ResolvedRootStep::Component { component_name: poly_name, .. },
                    ResolvedRootStep::NativeTag { tag }
                ] if wrapper_name == "Wrapper" && poly_name == "Poly" && tag == "input"
            )
        }),
        "recursive generic propagation should resolve Wrapper -> Poly -> input, got: {:?}",
        branches
            .iter()
            .map(|branch| &branch.root_chain)
            .collect::<Vec<_>>()
    );
}

#[test]
fn recursive_component_propagates_inherited_surface() {
    let project = make_project();

    // Child component with <div> root
    project
        .upsert_base(
            "/Child.vue",
            r#"<script setup lang="ts">
defineProps<{ childProp: string }>()
</script>
<template><div>child</div></template>"#,
        )
        .unwrap();

    // Parent with component root
    project
        .upsert_base(
            "/App.vue",
            r#"<script setup lang="ts">
import Child from './Child.vue'
defineProps<{ parentProp: string }>()
</script>
<template><Child :childProp="parentProp" /></template>"#,
        )
        .unwrap();

    let meta = get_meta(&project, "/App.vue");

    // Assert+: declared prop is present
    assert!(
        meta.accepted_props.iter().any(|p| p.name == "parentProp"),
        "should have declared 'parentProp'"
    );

    // Assert+: fallthrough_surface should have branches
    assert!(
        matches!(
            meta.fallthrough_surface,
            FallthroughSurface::Branches { .. }
        ),
        "fallthrough_surface should be Branches for component root"
    );

    // Assert+: root_chain should show Component step
    if let FallthroughSurface::Branches { ref branches } = meta.fallthrough_surface {
        assert!(!branches.is_empty(), "should have at least one branch");
        assert!(
            branches[0]
                .root_chain
                .iter()
                .any(|step| matches!(step, ResolvedRootStep::Component { .. })),
            "root_chain should contain a Component step, got: {:?}",
            branches[0].root_chain
        );
    }
}

#[test]
fn recursive_component_keeps_child_declared_surface_alongside_child_fallthrough() {
    let project = make_project();

    project
        .upsert_base(
            "/Child.vue",
            r#"<script setup lang="ts">
defineProps<{ childProp: string }>()
defineEmits<{ (e: 'childClick', value: number): void }>()
</script>
<template><div>child</div></template>"#,
        )
        .unwrap();

    project
        .upsert_base(
            "/App.vue",
            r#"<script setup lang="ts">
import Child from './Child.vue'
</script>
<template><Child /></template>"#,
        )
        .unwrap();

    let meta = get_meta(&project, "/App.vue");

    let child_prop = meta
        .accepted_props
        .iter()
        .find(|p| p.name == "childProp")
        .expect("parent must expose child's declared prop through component root recursion");
    assert!(
        matches!(child_prop.provenance, MemberProvenance::Inherited { .. }),
        "child declared prop must arrive as inherited acceptance on the parent"
    );
    assert!(
        matches!(child_prop.kind, AcceptedPropKind::Attr),
        "child declared prop should be exposed as an accepted attr on the parent"
    );

    let child_event = meta
        .accepted_events
        .iter()
        .find(|e| e.name == "childClick")
        .expect("parent must expose child's declared event through component root recursion");
    assert!(
        matches!(child_event.provenance, MemberProvenance::Inherited { .. }),
        "child declared event must arrive as inherited acceptance on the parent"
    );
    assert!(
        matches!(child_event.kind, AcceptedEventKind::Listener),
        "child declared event should be exposed as an accepted listener on the parent"
    );

    assert!(
        meta.accepted_props.iter().any(|p| p.name == "id"),
        "parent must still expose child's inherited native attrs, not just declared members"
    );
}

#[test]
fn non_vue_component_root_stops_fallthrough_recursion_at_the_boundary() {
    let project = make_project();

    project
        .upsert_base(
            "/Child.ts",
            r#"export default function Child() {
  return null
}"#,
        )
        .unwrap();
    project
        .upsert_base(
            "/App.vue",
            r#"<script setup lang="ts">
import Child from './Child'
defineProps<{ parentProp: string }>()
</script>
<template><Child /></template>"#,
        )
        .unwrap();

    let meta = get_meta(&project, "/App.vue");

    assert!(
        meta.accepted_props.iter().any(|p| p.name == "parentProp"),
        "declared props must remain on the accepted surface"
    );
    assert!(
        !meta.accepted_props.iter().any(|p| p.name == "id"),
        "non-Vue child roots must not invent inherited attrs"
    );
    assert_eq!(
        meta.accepted_surface_completeness,
        AcceptedSurfaceCompleteness::LowerBound,
        "non-Vue child roots should degrade completeness instead of recursing"
    );

    let FallthroughSurface::Branches { branches } = &meta.fallthrough_surface else {
        panic!("expected FallthroughSurface::Branches");
    };
    assert!(
        branches.iter().any(|branch| {
            matches!(
                &branch.status,
                BranchStatus::Unresolved {
                    reason: UnresolvedBranchReason::ChildResolutionFailed,
                }
            )
        }),
        "non-Vue child roots should stop at an unresolved branch"
    );
}

#[test]
fn package_component_root_prefers_declaration_companion_for_recursive_fallthrough() {
    let ws = Arc::new(verter_workspace::MemoryWorkspace::new(
        verter_workspace::MemoryOptions::default(),
    ));
    ws.inject_file(
        "/workspace/tsconfig.json".to_string(),
        Arc::from(
            r#"{ "compilerOptions": { "module": "esnext", "moduleResolution": "bundler" } }"#,
        ),
    );
    ws.inject_file(
        "/workspace/node_modules/reka-ui/package.json".to_string(),
        Arc::from(
            r#"{ "name": "reka-ui", "types": "./dist/index.d.ts", "exports": { ".": { "types": "./dist/index.d.ts", "import": "./dist/index.js" } } }"#,
        ),
    );
    ws.inject_file(
        "/workspace/node_modules/reka-ui/dist/index.d.ts".to_string(),
        Arc::from(r#"export { default as FancyRoot } from './FancyRoot.vue'"#),
    );
    ws.inject_file(
        "/workspace/node_modules/reka-ui/dist/index.js".to_string(),
        Arc::from("export const runtimeOnly = true"),
    );
    ws.inject_file(
        "/workspace/node_modules/reka-ui/dist/FancyRoot.vue".to_string(),
        Arc::from(
            r#"<script setup lang="ts">
defineProps<{ childProp?: string }>()
</script>
<template><div /></template>"#,
        ),
    );
    ws.inject_file(
        "/workspace/src/App.vue".to_string(),
        Arc::from(
            r#"<script setup lang="ts">
import { FancyRoot } from 'reka-ui'
</script>
<template><FancyRoot /></template>"#,
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
    assert!(
        project.ensure_loaded("/workspace/src/App.vue").unwrap(),
        "workspace owner should load the wrapper component"
    );

    let meta = get_meta(&project, "/workspace/src/App.vue");

    assert!(
        meta.accepted_props.iter().any(|p| p.name == "childProp"),
        "component root recursion should expose the child's declared props through the declaration companion"
    );
    assert!(
        meta.accepted_props.iter().any(|p| p.name == "id"),
        "component root recursion should still expose native attrs from the child root"
    );
    assert_eq!(
        meta.accepted_surface_completeness,
        AcceptedSurfaceCompleteness::Exact,
        "typed package component roots should recurse exactly through the declaration companion"
    );

    let FallthroughSurface::Branches { branches } = &meta.fallthrough_surface else {
        panic!("expected FallthroughSurface::Branches");
    };
    assert!(
        branches
            .iter()
            .all(|branch| !matches!(branch.status, BranchStatus::Unresolved { .. })),
        "typed package component roots should not stop at childResolutionFailed: {:?}",
        branches
            .iter()
            .map(|branch| &branch.status)
            .collect::<Vec<_>>()
    );
    assert!(
        branches
            .iter()
            .any(|branch| branch.root_chain.iter().any(|step| {
                matches!(
                    step,
                    ResolvedRootStep::Component { canonical_id, .. }
                        if canonical_id == "/workspace/node_modules/reka-ui/dist/FancyRoot.vue"
                )
            })),
        "root chain should recurse through the declaration-exported child component, got: {:?}",
        branches
            .iter()
            .map(|branch| &branch.root_chain)
            .collect::<Vec<_>>()
    );
}

#[test]
fn builtin_root_is_unresolved_branch() {
    let project = make_project();
    project
        .upsert_base(
            "/App.vue",
            r#"<script setup lang="ts">
defineProps<{ msg: string }>()
</script>
<template><Teleport to="body">{{ msg }}</Teleport></template>"#,
        )
        .unwrap();

    let meta = get_meta(&project, "/App.vue");

    // Assert+: declared prop is present
    assert!(
        meta.accepted_props.iter().any(|p| p.name == "msg"),
        "should have declared 'msg'"
    );

    // Assert-: no inherited members from Teleport
    assert!(
        !meta
            .accepted_props
            .iter()
            .any(|p| matches!(p.provenance, MemberProvenance::Inherited { .. })),
        "Teleport root should produce no inherited props"
    );
}

#[test]
fn child_change_invalidates_parent_fallthrough_cache() {
    let project = make_project();
    project
        .upsert_base("/Child.vue", r#"<template><div>child</div></template>"#)
        .unwrap();
    project
        .upsert_base(
            "/App.vue",
            r#"<script setup lang="ts">
import Child from './Child.vue'
</script>
<template><Child /></template>"#,
        )
        .unwrap();

    let first = get_meta(&project, "/App.vue");
    assert!(
        !first.accepted_props.iter().any(|p| p.name == "value"),
        "div-root child should not expose input-only attrs before the dependency changes"
    );

    #[cfg(not(target_arch = "wasm32"))]
    {
        // get_meta does not populate cached_fallthrough; use resolve_fallthrough_surface
        let _ = project.host().resolve_fallthrough_surface("/App.vue");
    }
    #[cfg(not(target_arch = "wasm32"))]
    let first_cache = cached_fallthrough_state(&project, "/App.vue")
        .expect("first query should cache fallthrough");

    project
        .upsert_base("/Child.vue", r#"<template><input /></template>"#)
        .unwrap();

    let second = get_meta(&project, "/App.vue");
    assert!(
        second.accepted_props.iter().any(|p| p.name == "value"),
        "parent fallthrough surface must refresh when the child root changes"
    );

    #[cfg(not(target_arch = "wasm32"))]
    {
        let _ = project.host().resolve_fallthrough_surface("/App.vue");
        let second_cache = cached_fallthrough_state(&project, "/App.vue")
            .expect("second query should repopulate the parent fallthrough cache");
        assert!(
            !Arc::ptr_eq(&first_cache, &second_cache),
            "dependency change must invalidate the parent's cached fallthrough surface"
        );
    }
}

#[cfg(not(target_arch = "wasm32"))]
#[test]
fn shared_child_fallthrough_reuses_runtime_child_surface_nodes() {
    let project = make_project();
    project
        .upsert_base("/Child.vue", r#"<template><input /></template>"#)
        .unwrap();
    project
        .upsert_base(
            "/ParentA.vue",
            r#"<script setup lang="ts">
import Child from './Child.vue'
</script>
<template><Child /></template>"#,
        )
        .unwrap();
    project
        .upsert_base(
            "/ParentB.vue",
            r#"<script setup lang="ts">
import Child from './Child.vue'
</script>
<template><Child /></template>"#,
        )
        .unwrap();

    project.host().resolver_runtime().reset_counters();

    let first = get_meta(&project, "/ParentA.vue");
    let after_first = project.host().resolver_runtime().counter_snapshot();
    let second = get_meta(&project, "/ParentB.vue");
    let after_second = project.host().resolver_runtime().counter_snapshot();

    assert!(
        first.accepted_props.iter().any(|prop| prop.name == "value"),
        "first parent should inherit input attrs from the shared child"
    );
    assert!(
        second
            .accepted_props
            .iter()
            .any(|prop| prop.name == "value"),
        "second parent should inherit input attrs from the shared child"
    );
    assert!(
        !second
            .accepted_props
            .iter()
            .any(|prop| prop.name == "missing"),
        "shared child reuse must not fabricate unrelated attrs"
    );
    assert!(
        after_first.node_cache_misses > 0,
        "first parent should populate runtime fallthrough child nodes, got {:?}",
        after_first
    );
    assert!(
        after_second.node_cache_hits > after_first.node_cache_hits,
        "second parent should reuse runtime child-surface nodes for the shared child, before={:?} after={:?}",
        after_first,
        after_second
    );
}

#[cfg(not(target_arch = "wasm32"))]
#[test]
fn cached_fallthrough_fact_versions_include_transitive_child_component_meta_dependencies() {
    let project = make_project();
    project
        .upsert_base(
            "/types.ts",
            "export interface ChildProps { msg?: string; count?: number }",
        )
        .unwrap();
    project
        .upsert_base(
            "/Child.vue",
            r#"<script setup lang="ts">
import type { ChildProps } from './types'
defineProps<ChildProps>()
</script>
<template><div>child</div></template>"#,
        )
        .unwrap();
    project
        .upsert_base(
            "/App.vue",
            r#"<script setup lang="ts">
import Child from './Child.vue'
</script>
<template><Child /></template>"#,
        )
        .unwrap();

    let _ = get_meta(&project, "/App.vue");
    // get_meta does not populate cached_fallthrough; use resolve_fallthrough_surface
    let _ = project.host().resolve_fallthrough_surface("/App.vue");
    let cached = cached_fallthrough_entry(&project, "/App.vue")
        .expect("parent fallthrough should be cached after meta extraction");

    assert!(
        cached.fact_versions.iter().any(|fact| matches!(
            fact,
            verter_session_query::facts::fact_cache::FactVersionRef::FileWholeHash { canonical_id, .. }
                if canonical_id == "/Child.vue"
        )),
        "cached fallthrough facts should include the child component file"
    );
    assert!(
        cached.fact_versions.iter().any(|fact| matches!(
            fact,
            verter_session_query::facts::fact_cache::FactVersionRef::FileWholeHash { canonical_id, .. }
                if canonical_id == "/types.ts"
        )),
        "cached fallthrough facts should include transitive child component-meta deps"
    );
}

#[test]
fn root_spread_with_cross_file_type_still_resolves_after_eval_caching() {
    // Regression test for Fix 3: when cached eval inputs are threaded through
    // to fallthrough resolution, root v-bind="importedObj" must still resolve
    // the spread keys correctly and not degrade to UnknownSpread.
    use verter_session_query::analysis::component_meta::AcceptedSurfaceCompleteness;

    let project = make_project();
    project
        .upsert_base(
            "/src/types.ts",
            r#"export interface WidgetProps { enabled: boolean }
export const rootAttrs = { id: 'root', onClick: () => {} }"#,
        )
        .unwrap();
    project
        .upsert_base(
            "/src/Widget.vue",
            r#"<script setup lang="ts">
import { WidgetProps, rootAttrs } from './types'
defineProps<WidgetProps>()
</script>
<template><div v-bind="rootAttrs">content</div></template>"#,
        )
        .unwrap();

    project.host().set_import_dependencies(
        "/src/Widget.vue",
        vec![crate::types::DependencyResolution {
            specifier: "./types".to_string(),
            resolved_canonical_id: Some("/src/types.ts".to_string()),
            possible_canonical_ids: Vec::new(),
        }],
    );

    let meta = get_meta(&project, "/src/Widget.vue");

    // The declared prop must be present
    assert!(
        meta.props.iter().any(|p| p.name == "enabled"),
        "should have the declared 'enabled' prop"
    );

    // The root spread keys ('id', 'click') must be consumed and subtracted
    // from the accepted surface. If the eval caching broke, the spread would
    // degrade to UnknownSpread and the surface would be LowerBound.
    assert!(
        !meta.accepted_props.iter().any(|p| p.name == "id"),
        "root spread key 'id' must be consumed and subtracted from accepted attrs"
    );
    assert!(
        !meta.accepted_events.iter().any(|e| e.name == "click"),
        "root spread listener 'click' must be consumed and subtracted from accepted listeners"
    );
    assert_eq!(
        meta.accepted_surface_completeness,
        AcceptedSurfaceCompleteness::Exact,
        "with resolvable root spreads, accepted surface should be Exact, not degraded to LowerBound"
    );
}

// ===========================================================================
// FIX 3 — restored discriminating coverage for behaviours the deleted
// equivalence harnesses owned, now driven through the typeinfo / Vue
// macro-surface publication path.
// ===========================================================================

/// FIX 3 #1 — an inherited METHOD-STYLE slot's JSDoc `description` survives
/// through cross-file heritage onto the published slot surface. Sibling slot
/// tests assert names / bindings / returns but not the inherited slot
/// description; this fills that gap.
///
/// Discriminating: the `header` slot is declared (as a method signature with a
/// leading JSDoc block) ONLY on the imported base `BaseSlots`; the component's
/// own `MySlots` adds `footer`. The published `header` slot's description must
/// be the base's JSDoc text — a resolver that dropped inherited-slot JSDoc, or
/// failed to follow the cross-file heritage, would leave it `None`.
#[test]
fn inherited_method_style_slot_jsdoc_description_propagates_through_heritage() {
    let project = make_project();
    project
        .upsert_base(
            "/src/base-slots.ts",
            r#"export interface BaseSlots {
  /** The header slot, rendered above the content. */
  header(props: { title: string }): any
}
"#,
        )
        .unwrap();
    project
        .upsert_base(
            "/src/App.vue",
            r#"<script setup lang="ts">
import type { BaseSlots } from './base-slots'

interface MySlots extends BaseSlots {
  /** The footer slot. */
  footer(props: {}): any
}

defineSlots<MySlots>()
</script>
<template><div /></template>"#,
        )
        .unwrap();

    let meta = project
        .host()
        .get_component_meta("/src/App.vue")
        .expect("should return component meta");

    let header = meta
        .slots
        .iter()
        .find(|slot| slot.name == "header")
        .expect("inherited method-style header slot should be published");
    assert_eq!(
        header.description.as_deref(),
        Some("The header slot, rendered above the content."),
        "inherited method-style slot must carry its base-declared JSDoc description",
    );
    // Negative guard: a resolver that mis-attributed the OWN footer slot's
    // description onto header would surface the wrong text.
    assert_ne!(
        header.description.as_deref(),
        Some("The footer slot."),
        "header slot must not pick up the own footer slot's description",
    );

    // The own footer slot keeps its own description (sanity that heritage merge
    // did not clobber own-body JSDoc).
    let footer = meta
        .slots
        .iter()
        .find(|slot| slot.name == "footer")
        .expect("own footer slot should be published");
    assert_eq!(footer.description.as_deref(), Some("The footer slot."));
}

/// A Keep-modifier homomorphic mapped type over an IMPORTED interface
/// (`{ [K in keyof ImportedProps]: ImportedProps[K] }`) inherits each
/// member's optionality from the source — `orientation?: string` stays
/// OPTIONAL through the identity mapping (TS `Keep` semantics). The
/// mapped enumeration must resolve the imported source's MEMBER surface,
/// not only its key names: a names-only enumeration would default `Keep`
/// to required/mutable and publish `orientation` as a required prop.
/// Pins both directions — the optional member stays optional (never
/// modifier-defaulted) AND both members publish (never a blanket
/// deferral that drops the surface).
#[test]
fn keep_modifier_homomorphic_mapped_over_import_inherits_optionality() {
    let project = make_project();
    project
        .upsert_base(
            "/src/types.ts",
            r#"
export interface ImportedProps {
  orientation?: string
  count: number
}
"#,
        )
        .unwrap();
    project
        .upsert_base(
            "/src/Comp.vue",
            r#"<script setup lang="ts">
import type { ImportedProps } from './types'

defineProps<{ [K in keyof ImportedProps]: ImportedProps[K] }>()
</script>
<template><div /></template>"#,
        )
        .unwrap();

    let meta = project
        .host()
        .get_component_meta("/src/Comp.vue")
        .expect("component meta resolves");

    let prop = |name: &str| {
        meta.props
            .iter()
            .find(|prop| prop.name == name)
            .unwrap_or_else(|| panic!("prop {name} must be published: {:?}", meta.props))
    };
    assert!(
        !prop("orientation").required,
        "Keep optionality inherits from the imported source member — \
         `orientation?` stays optional through the identity mapping"
    );
    assert!(
        prop("count").required,
        "a required source member stays required through the identity mapping"
    );
}

/// Key-remapped mapped props judge JSDoc inheritance PER PRODUCED NAME on
/// the published (Shallow) surface: a true rename (`as `x-${K}``)
/// publishes a name no source declaration declares and severs the
/// declaration site, so NO description and NO tags are fabricated for it;
/// an identity remap (`as K`) IS the source declaration's name-preserving
/// image and keeps the doc. Modifier inheritance (the `?` optionality)
/// survives the rename.
///
/// Discriminating: pre-fix the Shallow mapped synthesiser inherited the
/// source member's spans + declaration_origin for every produced name, so
/// `x-orientation` published `orientation`'s doc and @deprecated tag — the
/// severing assertions below fail on that implementation.
#[test]
fn key_remapped_mapped_prop_does_not_inherit_source_jsdoc() {
    let project = make_project();
    project
        .upsert_base(
            "/src/remap.ts",
            r#"
export interface SourceProps {
  /**
   * Visual orientation of the widget.
   * @deprecated use layout instead
   */
  orientation?: string
}
export type RenamedProps<T> = { [K in keyof T as `x-${K}`]: T[K] }
export type IdentityProps<T> = { [K in keyof T as K]: T[K] }
export type FanoutProps<T> = { [K in keyof T as K | `x-${K}`]: T[K] }
"#,
        )
        .unwrap();
    project
        .upsert_base(
            "/src/Renamed.vue",
            r#"<script setup lang="ts">
import type { SourceProps, RenamedProps } from './remap'

defineProps<RenamedProps<SourceProps>>()
</script>
<template><div /></template>"#,
        )
        .unwrap();
    project
        .upsert_base(
            "/src/Identity.vue",
            r#"<script setup lang="ts">
import type { SourceProps, IdentityProps } from './remap'

defineProps<IdentityProps<SourceProps>>()
</script>
<template><div /></template>"#,
        )
        .unwrap();

    let renamed_meta = project
        .host()
        .get_component_meta("/src/Renamed.vue")
        .expect("component meta resolves");

    // Renamed arm: `x-orientation` is declared by NO source declaration —
    // no description, no tags (negative: nothing fabricated from the
    // source member's declaration site).
    let renamed = renamed_meta
        .props
        .iter()
        .find(|prop| prop.name == "x-orientation")
        .expect("the renamed remap arm must surface");
    assert_eq!(
        renamed.description.as_deref(),
        None,
        "a key-remapped prop must NOT inherit the source member's JSDoc \
         description — its produced name has no source declaration site"
    );
    assert!(
        renamed.tags.is_empty(),
        "a key-remapped prop must NOT inherit the source member's tags, got {:?}",
        renamed.tags
    );
    // Over-sever guard: modifier parity survives the rename — the source
    // member's `?` still makes the renamed prop optional.
    assert!(
        !renamed.required,
        "the renamed arm must still inherit optionality from the source member"
    );

    // Identity remap (`as K`): the produced name equals the source key —
    // the declaration site is real and the doc publishes (guards against
    // an over-broad "any `as` clause severs" implementation).
    let identity_meta = project
        .host()
        .get_component_meta("/src/Identity.vue")
        .expect("component meta resolves");
    let orientation = identity_meta
        .props
        .iter()
        .find(|prop| prop.name == "orientation")
        .expect("the identity remap arm must surface");
    assert_eq!(
        orientation.description.as_deref(),
        Some("Visual orientation of the widget."),
        "an `as K` identity remap must keep the source JSDoc"
    );
    assert!(
        orientation.tags.iter().any(|tag| tag.name == "deprecated"),
        "an `as K` identity remap must keep the source @deprecated tag, got {:?}",
        orientation.tags
    );
    assert!(
        !orientation.required,
        "the identity arm must still inherit optionality from the source member"
    );

    // One-to-many remap (`as K | `x-${K}``): each produced arm is judged
    // independently — the verbatim `orientation` arm keeps the doc, the
    // renamed `x-orientation` arm severs it.
    project
        .upsert_base(
            "/src/Fanout.vue",
            r#"<script setup lang="ts">
import type { SourceProps, FanoutProps } from './remap'

defineProps<FanoutProps<SourceProps>>()
</script>
<template><div /></template>"#,
        )
        .unwrap();
    let fanout_meta = project
        .host()
        .get_component_meta("/src/Fanout.vue")
        .expect("component meta resolves");
    let fanout_verbatim = fanout_meta
        .props
        .iter()
        .find(|prop| prop.name == "orientation")
        .expect("the verbatim arm of a one-to-many remap must surface");
    assert_eq!(
        fanout_verbatim.description.as_deref(),
        Some("Visual orientation of the widget."),
        "the verbatim arm of a one-to-many remap must keep the source JSDoc"
    );
    let fanout_renamed = fanout_meta
        .props
        .iter()
        .find(|prop| prop.name == "x-orientation")
        .expect("the renamed arm of a one-to-many remap must surface");
    assert_eq!(
        fanout_renamed.description.as_deref(),
        None,
        "the renamed arm of a one-to-many remap must NOT inherit the source JSDoc"
    );
    assert!(
        fanout_renamed.tags.is_empty(),
        "the renamed arm of a one-to-many remap must NOT inherit tags, got {:?}",
        fanout_renamed.tags
    );
}

/// PRODUCER-SCOPE HYGIENE end-to-end: two conditional children each publish
/// the IDENTICAL fully-closed prop type (`shared?: string`); the parent's
/// REAL fallthrough branch rows (produced by the resolver-core clone
/// boundary, not hand-built) carry NO positional producer scope for the
/// closed source, so re-feeding exactly those two rows through the output
/// envelope materializes the shared source ONCE — the `(effective scope,
/// source identity)` memo entry is SHARED across the two children.
///
/// Discriminating: with the clone boundary allocating the producer scope
/// unconditionally, the two rows key `(/ChildA.vue, S)` / `(/ChildB.vue,
/// S)`, the scope asserts fail RED, and the memo probe materializes TWICE.
#[test]
fn closed_inherited_sources_share_one_output_memo_entry_across_children() {
    use verter_session_query::analysis::component_meta as cm;

    let project = make_project();
    project
        .upsert_base(
            "/ChildA.vue",
            r#"<script setup lang="ts">
defineProps<{ shared?: string }>()
</script>
<template><div /></template>"#,
        )
        .unwrap();
    project
        .upsert_base(
            "/ChildB.vue",
            r#"<script setup lang="ts">
defineProps<{ shared?: string }>()
</script>
<template><span /></template>"#,
        )
        .unwrap();
    project
        .upsert_base(
            "/App.vue",
            r#"<script setup lang="ts">
import ChildA from './ChildA.vue'
import ChildB from './ChildB.vue'
const cond = true
</script>
<template>
  <ChildA v-if="cond" />
  <ChildB v-else />
</template>"#,
        )
        .unwrap();
    let host = project.host();

    let meta = host
        .get_component_meta("/App.vue")
        .expect("two-child conditional parent resolves");
    let cm::FallthroughSurface::Branches { branches } = &meta.fallthrough_surface else {
        panic!("fixture premise: v-if/v-else roots produce fallthrough branches");
    };
    assert_eq!(branches.len(), 2, "two conditional root branches");
    let shared_rows: Vec<&cm::FallthroughPropEntry> = branches
        .iter()
        .map(|branch| {
            branch
                .props
                .iter()
                .find(|prop| prop.name == "shared")
                .expect("each branch inherits its child's `shared` prop")
        })
        .collect();

    // The REAL clone-boundary rows: identical fully-closed source, NO
    // positional producer scope (nothing scope-relative to resolve).
    let source_a = shared_rows[0]
        .publication
        .result()
        .selected_source()
        .expect("the inherited closed prop carries a typed source");
    assert!(
        !source_a.is_scope_relative(),
        "fixture premise: the child's `shared?: string` publishes a fully \
         anchored/closed source; got {source_a:?}"
    );
    assert_eq!(
        shared_rows[0].publication.source_position(),
        shared_rows[1].publication.source_position(),
        "the two children's identical closed prop types publish the \
         identical source value"
    );
    assert_eq!(
        shared_rows[0].type_source_scope, None,
        "a fully-closed inherited source carries NO producer scope (an \
         irrelevant scope fragments the output memo)"
    );
    assert_eq!(
        shared_rows[1].type_source_scope, None,
        "a fully-closed inherited source carries NO producer scope (an \
         irrelevant scope fragments the output memo)"
    );

    // Memo probe: re-feed EXACTLY those two real rows through the output
    // envelope — one shared memo entry, ONE materialize call.
    let mut probe = blank_output_analysis();
    probe.fallthrough_surface = cm::FallthroughSurface::Branches {
        branches: vec![
            cm::FallthroughBranch {
                branch_key: "0".to_string(),
                condition_text: None,
                props: vec![shared_rows[0].clone()],
                events: Vec::new(),
                root_chain: Vec::new(),
                status: cm::BranchStatus::Resolved,
            },
            cm::FallthroughBranch {
                branch_key: "1".to_string(),
                condition_text: None,
                props: vec![shared_rows[1].clone()],
                events: Vec::new(),
                root_chain: Vec::new(),
                status: cm::BranchStatus::Resolved,
            },
        ],
    };

    let fixture_dispatch_22 =
        verter_type_engine::project_semantic_dispatch::ProjectSemanticDispatch::new(host);
    let output = crate::meta_resolve::projectors::build_component_meta_output(
        host,
        &fixture_dispatch_22,
        "/App.vue",
        probe,
        None,
        crate::meta_resolve::PublishedCompleteness::COMPLETE,
    )
    .expect("the closed inherited source materializes");
    let calls =
        crate::meta_resolve::projectors::LAST_OUTPUT_MATERIALIZE_CALLS.with(std::cell::Cell::get);
    assert_eq!(
        calls, 1,
        "two branch rows carrying the identical closed source from two \
         different children must share ONE output memo entry — the \
         now-dropped producer scope was the only discriminator"
    );
    // Output-value invariance: both rows materialize the identical value.
    let lanes = output.into_parts().2.into_lanes();
    assert_eq!(lanes.fallthrough_props[0][0], lanes.fallthrough_props[1][0]);
    assert_eq!(
        published_type(&lanes.fallthrough_props[0][0]),
        &TypeExpr::Primitive(PrimitiveName::String),
        "the shared closed source materializes the exact child type"
    );
}
