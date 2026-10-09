use super::*;

#[test]
fn prepared_decl_bundle_without_store_view_reuses_stable_cache() {
    let host = make_host();
    upsert_non_sfc(&host, "/src/dep.ts", "export interface Base { id: string }");
    upsert_non_sfc(
        &host,
        "/src/types.ts",
        "import type { Base } from './dep'\nexport interface Props extends Base {}\n",
    );
    host.set_import_dependencies(
        "/src/types.ts",
        vec![exact_dependency("./dep", "/src/dep.ts")],
    );

    let _ = host
        .ensure_indexed_ready("/src/types.ts")
        .expect("types dependency should materialize");
    host.provenance().reset();

    let first = host
        .prepared_type_decl("/src/types.ts", "Props")
        .expect("first lookup should materialize a prepared bundle");
    let after_first = host.provenance().snapshot();
    assert_eq!(
        after_first.bundle_materializations, 1,
        "first lookup without a store view should materialize exactly one bundle"
    );
    assert_eq!(
        after_first.dep_resolution_calls, 0,
        "this fixture carries exact import targets already, so first lookup should not need dependency-resolution recomputation"
    );

    let second = host
        .prepared_type_decl("/src/types.ts", "Props")
        .expect("second lookup should reuse the prepared bundle");
    let after_second = host.provenance().snapshot();

    assert_eq!(
        first
            .name_resolution
            .get("Base")
            .map(|identity| identity.canonical_id.as_ref()),
        Some("/src/dep.ts"),
    );
    assert_eq!(
        second
            .name_resolution
            .get("Base")
            .map(|identity| identity.canonical_id.as_ref()),
        Some("/src/dep.ts"),
    );
    assert_eq!(
        after_second.bundle_materializations, 1,
        "warm lookup without a store view should reuse the stable bundle cache instead of rematerializing"
    );
    assert_eq!(
        after_second.dep_resolution_calls, 0,
        "warm lookup without a store view should not recompute dependency resolutions"
    );
    assert!(
        after_second.bundle_cache_hits >= 1,
        "warm lookup without a store view should register at least one bundle cache hit, got {:?}",
        after_second
    );
}

#[test]
fn prepared_decl_bundle_with_store_view_reuses_cache_for_structural_exact_resolutions() {
    let ws = Arc::new(verter_workspace::MemoryWorkspace::new(
        verter_workspace::MemoryOptions::default(),
    ));
    ws.inject_file(
        "/workspace/dep.ts".to_string(),
        Arc::from("export interface Base { id: string }\n"),
    );
    ws.inject_file(
        "/workspace/types.ts".to_string(),
        Arc::from("import type { Base } from './dep'\nexport interface Props extends Base {}\n"),
    );

    let host = VerterHost::new(HostConfig::default(), ws);
    host.set_import_dependencies(
        "/workspace/types.ts",
        vec![exact_dependency("./dep", "/workspace/dep.ts")],
    );

    let _view = host.resolver_store_view_read().into_owned_view();
    host.provenance().reset();

    let first = host
        .prepared_type_decl("/workspace/types.ts", "Props")
        .expect("first lookup should materialize a prepared bundle");
    let after_first = host.provenance().snapshot();
    assert_eq!(
        after_first.bundle_materializations, 1,
        "first lookup with a captured store view should materialize exactly one bundle"
    );

    let second = host
        .prepared_type_decl("/workspace/types.ts", "Props")
        .expect("second lookup should reuse the prepared bundle even with the same captured view");
    let after_second = host.provenance().snapshot();

    assert_eq!(
        first
            .name_resolution
            .get("Base")
            .map(|identity| identity.canonical_id.as_ref()),
        Some("/workspace/dep.ts"),
    );
    assert_eq!(
        second
            .name_resolution
            .get("Base")
            .map(|identity| identity.canonical_id.as_ref()),
        Some("/workspace/dep.ts"),
    );
    assert_eq!(
        after_second.bundle_materializations, 1,
        "warm lookup with the same captured store view should reuse the stable bundle cache"
    );
    assert!(
        after_second.bundle_cache_hits >= 1,
        "warm lookup with the same captured store view should register a bundle cache hit, got {:?}",
        after_second
    );
    // Verify the dependency resolution is persisted on DerivedRawState
    // (D48 split — import_routes is the sub-mirror of
    // IndexedReady.import_routes).
    let derived_entry = host
        .derived_raw_cache()
        .get("/workspace/types.ts")
        .expect("types file should have a derived_raw_cache entry");
    assert_eq!(
        derived_entry
            .import_routes
            .get("./dep")
            .and_then(|dep| dep.resolved_canonical_id.as_deref()),
        Some("/workspace/dep.ts"),
        "structurally derived exact resolutions should be persisted onto the DerivedRawState entry",
    );
}

#[test]
fn ensure_indexed_ready_reuses_cached_vue_entry_arc() {
    let host = make_host();
    upsert_non_sfc(
        &host,
        "/src/base.ts",
        "export interface Base { id: string }\n",
    );
    upsert_vue(
        &host,
        "/src/types.vue",
        r#"<script lang="ts">
import type { Base } from './base'

export interface Props extends Base {
  label: string
}
</script>
<template><div /></template>"#,
    );
    host.set_import_dependencies(
        "/src/types.vue",
        vec![exact_dependency("./base", "/src/base.ts")],
    );

    let first = host
        .ensure_indexed_ready("/src/types.vue")
        .expect("first Vue imported dependency state should be built");
    let second = host
        .ensure_indexed_ready("/src/types.vue")
        .expect("second Vue imported dependency state should reuse the cached entry");

    assert_eq!(
        first.whole_hash, second.whole_hash,
        "repeated Vue imported dependency state lookups should produce equivalent entries",
    );
    // In IndexedReady, snapshot is Arc<FileAnalysisSnapshot> (non-optional).
    assert!(
        first.framework_parse.is_some(),
        "cached Vue imported dependency entry should retain parse state",
    );
    assert!(
        first.script_analysis.is_some() && first.export_signatures.is_some(),
        "cached Vue imported dependency entry should retain script facts alongside the full snapshot for later export-graph reuse",
    );
    assert!(
        first.route_inventory.counts.top_level_statement_count > 0,
        "cached Vue imported dependency entry should retain routes so later resolver lookups do not reparse",
    );
}

/// A6-04 — cycle, missing dependency, an unsupported head and a budget-clamped
/// host each publish the EXACT typed reason with no provenance, and re-demanding
/// never promotes a partial into an exact role (nothing warm was admitted).
/// Collapsing the reasons onto one value REDs; promoting a partial REDs.
#[test]
fn return_wrapper_role_degrades_typed_and_is_never_warmed() {
    let host = make_host();
    upsert_non_sfc(
        &host,
        "/workspace/node_modules/vue/index.d.ts",
        RETURN_WRAPPER_VUE_DTS,
    );

    // Cycle: a mutually recursive local alias pair on the return head.
    let cycle = "/workspace/src/cycle.ts";
    upsert_ts(
        &host,
        cycle,
        "type A<T> = B<T>\n\
         type B<T> = A<T>\n\
         export function getValue(): A<number> { return null as never; }\n",
    );
    let (role, provenance) = return_wrapper_role_for(&host, cycle, "getValue");
    assert_eq!(
        role,
        verter_type_expr::ReactiveWrapperRole::Unresolved {
            reason: verter_type_expr::ReactiveWrapperUnresolvedReason::Cycle
        }
    );
    assert!(provenance.is_none());
    // Re-demand: still the same typed cycle, never a promoted exact role.
    assert_eq!(
        return_wrapper_role_for(&host, cycle, "getValue").0,
        role,
        "a partial must not be promoted warm into an exact role"
    );
    // (Non-warmth is proven by RECOVERY at the end of this test, after the
    // reason-distinctness control has read the degraded roles.)

    // Missing dependency: the import edge resolves to a canonical the host has
    // no state for, so the routed terminal cannot be reached.
    let missing = "/workspace/src/missing.ts";
    upsert_ts(
        &host,
        missing,
        "import type { Ref } from './gone'\n\
         export function getValue(): Ref<number> { return null as never; }\n",
    );
    host.set_import_dependencies(
        missing,
        vec![exact_dependency("./gone", "/workspace/src/gone.ts")],
    );
    let (role, provenance) = return_wrapper_role_for(&host, missing, "getValue");
    assert_eq!(
        role,
        verter_type_expr::ReactiveWrapperRole::Unresolved {
            reason: verter_type_expr::ReactiveWrapperUnresolvedReason::MissingDependency
        }
    );
    assert!(provenance.is_none());
    assert_eq!(return_wrapper_role_for(&host, missing, "getValue").0, role);
    // (Arrival recovery is asserted at the end of this test.)

    // A wholly UNRESOLVABLE import specifier: `resolve_type_dependency_canonical`
    // reports the same `MissingDependency` reason for "no route at all" as for
    // "a known route to an unloaded file" (`missing` above) —
    // `resolve_authored_reference_route`'s import-type-specifier arm has no
    // separate outcome for the two. The declaration itself still PREPARES (an
    // unrelated unresolvable import must not poison every sibling declaration's
    // admission — see `insert_value_space_import_resolutions`), so the demand
    // reaches this reason rather than failing earlier at the preparation step.
    let unprepared = "/workspace/src/unprepared.ts";
    upsert_ts(
        &host,
        unprepared,
        "import type { Ref } from 'nowhere'\n\
         export function getValue(): Ref<number> { return null as never; }\n",
    );
    let (unprepared_role, provenance) = return_wrapper_role_for(&host, unprepared, "getValue");
    assert_eq!(
        unprepared_role,
        verter_type_expr::ReactiveWrapperRole::Unresolved {
            reason: verter_type_expr::ReactiveWrapperUnresolvedReason::MissingDependency
        }
    );
    assert!(provenance.is_none());

    // Unsupported: an INFERRED return has no authored head at all, so the
    // demand has nothing to resolve. This is the mint gate surfacing at the
    // demand boundary — it must never become a complete non-wrapper proof.
    let inferred = "/workspace/src/inferred.ts";
    upsert_ts(
        &host,
        inferred,
        "import type { Ref } from 'vue'\n\
         export function getValue() { return null as unknown as Ref<number>; }\n",
    );
    host.set_import_dependencies(
        inferred,
        vec![exact_dependency(
            "vue",
            "/workspace/node_modules/vue/index.d.ts",
        )],
    );
    let (role, provenance) = return_wrapper_role_for(&host, inferred, "getValue");
    assert_eq!(
        role,
        verter_type_expr::ReactiveWrapperRole::Unresolved {
            reason: verter_type_expr::ReactiveWrapperUnresolvedReason::Unsupported
        },
        "an inferred return publishes no authored head, so the demand fails closed"
    );
    assert!(provenance.is_none());
    assert_ne!(
        role,
        verter_type_expr::ReactiveWrapperRole::None,
        "an inferred return is NOT a complete non-wrapper proof"
    );

    // The connected-work / projection ENVELOPE arm is proven in the demand
    // entry's own module, where the envelope limit is settable: see
    // `project_semantic_dispatch::reactive_wrapper::tests::
    // clamped_connected_work_envelope_degrades_the_return_role_typed`. Faking a
    // trip here would assert a state this fixture cannot actually reach — the
    // routes are one hop long and the armed projection budget is nowhere near
    // exhausted, so the envelope class is proven where it is reachable.

    // Control: the degradations are DISTINCT typed values — an implementation
    // that collapsed them onto one reason fails here. `unprepared_role` is
    // DELIBERATELY excluded from the pair against `missing_role`: both are the
    // SAME `MissingDependency` reason by design — `resolve_authored_reference_route`'s
    // import-type-specifier arm reports `MissingDependency` for "no route at
    // all" identically to "a known route to an unloaded file" (see the comment
    // on `unprepared` above); they are the same typed value, not a collapse bug.
    let cycle_role = return_wrapper_role_for(&host, cycle, "getValue").0;
    let missing_role = return_wrapper_role_for(&host, missing, "getValue").0;
    let inferred_role = return_wrapper_role_for(&host, inferred, "getValue").0;
    for (a, b) in [
        (&cycle_role, &missing_role),
        (&missing_role, &inferred_role),
        (&cycle_role, &inferred_role),
        (&cycle_role, &unprepared_role),
        (&inferred_role, &unprepared_role),
    ] {
        assert_ne!(a, b, "each failure class keeps its own typed reason");
    }
    assert_eq!(
        missing_role, unprepared_role,
        "an unresolvable import specifier and a known-but-unloaded dependency \
         report the SAME MissingDependency reason — resolve_authored_reference_route \
         has no separate outcome for the two"
    );

    // A re-demand equality alone cannot distinguish a cold recompute from a
    // WARMED partial (both answer the same reason), so prove non-warmth by
    // RECOVERY at the public boundary: fix each degraded input and re-demand —
    // a warmed partial would keep answering its `Unresolved` reason forever,
    // while a recomputing demand resolves exactly. (A TLS cold-compute-scope
    // probe is the wrong instrument here: the cycle stop is the route
    // resolver's own visited-set, not a folding query, so no scope partiality
    // manifests at this boundary.)
    //
    // Cycle recovery: break the recursion.
    upsert_ts(
        &host,
        cycle,
        "import type { Ref } from 'vue'\n\
         type A<T> = Ref<T>\n\
         type B<T> = A<T>\n\
         export function getValue(): A<number> { return null as never; }\n",
    );
    host.set_import_dependencies(
        cycle,
        vec![exact_dependency(
            "vue",
            "/workspace/node_modules/vue/index.d.ts",
        )],
    );
    let (recovered_role, recovered_provenance) = return_wrapper_role_for(&host, cycle, "getValue");
    assert_eq!(
        recovered_role,
        verter_type_expr::ReactiveWrapperRole::Ref,
        "breaking the cycle must recover the exact role — a warmed partial cannot"
    );
    assert!(
        recovered_provenance.is_some(),
        "the recovered route must carry exact provenance"
    );
    // Missing-dependency ARRIVAL (route-provenance required test #10): once
    // the dependency exists, the demand must resolve exactly.
    upsert_non_sfc(
        &host,
        "/workspace/src/gone.ts",
        "export type { Ref } from 'vue'\n",
    );
    host.set_import_dependencies(
        "/workspace/src/gone.ts",
        vec![exact_dependency(
            "vue",
            "/workspace/node_modules/vue/index.d.ts",
        )],
    );
    let (arrived_role, arrived_provenance) = return_wrapper_role_for(&host, missing, "getValue");
    assert_eq!(
        arrived_role,
        verter_type_expr::ReactiveWrapperRole::Ref,
        "after the missing dependency arrives, the demand must resolve exactly — \
         a warmed partial cannot recover"
    );
    assert!(
        arrived_provenance.is_some(),
        "the arrived route must carry exact provenance"
    );
}

/// INVERTED-POLARITY successor to
/// `positive_route_stamp_is_caller_captured_not_live_at_record_time`.
///
/// That case pinned the capture-before-resolve discipline of the HOST
/// positive-route memo: a stamp taken after the resolve could forge
/// currency onto a retargeted answer. The memo is deleted — it duplicated
/// the workspace's own bounded owner-edge candidate slot, and being a
/// plain map with no witness it needed a global `content_generation`
/// equality to decide whether it was still true, which was the last
/// global-generation warm-resolution validity test in the session.
///
/// The successor asserts the DELETION: driving the host's own resolution
/// of an owner's import populates NO host-side route memo. Only a caller
/// push (`set_import_dependencies`) may write that table, and it is then
/// the caller's statement verbatim.
///
/// FAILS if any host resolution path re-introduces a memo write.
#[test]
fn host_resolution_populates_no_host_side_route_memo() {
    let host = make_host();
    host.configure_projects(vec![verter_workspace::ide_project_config(
        "/workspace".to_string(),
        "/workspace".to_string(),
        Some("/workspace/tsconfig.json".to_string()),
    )]);

    upsert_non_sfc(&host, "/workspace/dep.ts", "export const dep = 1;\n");
    upsert_non_sfc(
        &host,
        "/workspace/owner.ts",
        "import { dep } from './dep';\nexport const owner = dep;\n",
    );

    // Drive every host route lane over the owner's authored specifier.
    assert_eq!(
        host.resolve_type_dependency_canonical_shallow("/workspace/owner.ts", "./dep")
            .as_deref(),
        Some("/workspace/dep.ts"),
        "fixture invariant: the edge really does resolve, so the memo would \
         have had something to record"
    );
    let _ = host.ensure_indexed_ready("/workspace/owner.ts");
    let _ = host.prepared_decl_bundle("/workspace/owner.ts");
    let _ = host.owner_import_route_witness_for_tests("/workspace/owner.ts");

    assert!(
        host.derived_raw_cache()
            .get("/workspace/owner.ts")
            .is_none_or(|derived| derived.import_routes.is_empty()),
        "HOST ROUTE MEMO REINTRODUCED: `DerivedRawState.import_routes` is the \
         CALLER-SUPPLIED table only; a host-side memo duplicates the workspace \
         owner-edge slot and needs a global-generation validity stamp"
    );

    // Positive control: a CALLER push does land, so the assertion above is
    // not vacuously true for want of a writable table.
    host.set_import_dependencies(
        "/workspace/owner.ts",
        vec![crate::types::DependencyResolution {
            specifier: "./dep".to_string(),
            resolved_canonical_id: Some("/workspace/dep.ts".to_string()),
            possible_canonical_ids: vec!["/workspace/dep.ts".to_string()],
        }],
    );
    assert!(
        host.derived_raw_cache()
            .get("/workspace/owner.ts")
            .is_some_and(|derived| derived.import_routes.contains_key("./dep")),
        "the caller-supplied table must still be writable by its one writer"
    );
}

/// DISCRIMINATING regression (RouteDb stale-serve — generation-current
/// wildcard-edge route-surface production). A wildcard barrel
/// (`export * from './runtime'; export * from './present';`) first resolves
/// `Runtime` through `./runtime` when only `runtime.js` exists, caching a
/// `Route` fact whose indexed shallow surface bakes the wildcard edge
/// `./runtime → runtime.js`. When the `.d.ts` companion `runtime.d.ts` later
/// appears, TS-first priority retargets the effective edge to `runtime.d.ts`.
///
/// The barrel becomes scheduler-tracked once resolved, so the
/// owner-surface freshness gate's tier-1 (owner content hash) judges the
/// entry fresh after the retarget — the owner's content is unchanged. The
/// baked wildcard edge is nonetheless stale: it depends on the dependency
/// file set, which changed (a new file advanced `content_generation`). Before
/// the fix the warm host reproduces the stale `Route` hash (encoding
/// `runtime.js`) and validates the warm `RouteDb` entry, stale-serving
/// `runtime.js`.
///
/// The fix gates route-surface fact production AND indexed-surface
/// reuse on `route_surface_is_edge_current`: a wildcard-bearing surface is
/// edge-stale once `content_generation` advances past its baked-edge
/// generation, so no stale `Route` fact is produced and the edges are
/// rebuilt. The warm host then resolves the SAME target as a fresh
/// host.
///
/// FAILS pre-fix: the warm host keeps returning `runtime.js` after
/// `runtime.d.ts` appears. PASSES post-fix: the warm host returns
/// `runtime.d.ts`, identical to a fresh host built on the same workspace.
#[test]
fn route_fact_retargets_js_to_dts_on_warm_host() {
    let ws = Arc::new(CountingWorkspace::new());
    let index = "/workspace/index.ts";
    // Wildcard barrel + a resolvable sibling, injected straight into the
    // workspace.
    ws.inject_file(
        index,
        "export * from './runtime';\nexport * from './present';\n",
    );
    ws.inject_file("/workspace/present.ts", "export type Present = number;\n");
    let warm = VerterHost::new(HostConfig::default(), ws.clone());

    // `./runtime` absent → `Runtime` misses.
    let r0 = warm.resolve_named_type_export_target(index, "Runtime");
    assert_eq!(
        r0, None,
        "precondition: Runtime misses while ./runtime is absent"
    );

    // `runtime.js` appears: `./runtime` resolves to the runtime script (no
    // `.d.ts` companion yet), so `Runtime` resolves to runtime.js and the warm
    // host caches a Route fact encoding the `./runtime → runtime.js` edge.
    ws.inject_file("/workspace/runtime.js", "export const Runtime = true\n");
    let r1 = warm.resolve_named_type_export_target(index, "Runtime");
    assert_eq!(
        r1,
        Some(("/workspace/runtime.js".to_string(), "Runtime".to_string())),
        "precondition: Runtime resolves to runtime.js while only the .js exists"
    );

    // The `.d.ts` companion appears. TS-first priority retargets `./runtime`
    // to runtime.d.ts.
    ws.inject_file("/workspace/runtime.d.ts", "export type Runtime = boolean\n");

    // A FRESH host on the same workspace is the oracle for the retargeted edge.
    let fresh = VerterHost::new(HostConfig::default(), ws.clone());
    let fresh_result = fresh.resolve_named_type_export_target(index, "Runtime");
    assert_eq!(
        fresh_result,
        Some(("/workspace/runtime.d.ts".to_string(), "Runtime".to_string())),
        "precondition: a fresh host resolves Runtime to the .d.ts companion after retarget"
    );

    let warm_result = warm.resolve_named_type_export_target(index, "Runtime");
    assert_eq!(
        warm_result, fresh_result,
        "the WARM host MUST return the SAME retargeted target as a FRESH host once \
         runtime.d.ts appears — a stale wildcard-bearing indexed surface (its \
         baked ./runtime edge unchanged at owner-content level but edge-stale after \
         the dependency file set shifted) must NOT reproduce its Route fact. \
         Route-surface production + materializer reuse must be gated on the \
         wildcard-edge resolution generation"
    );
}

/// DISCRIMINATING regression (edge currency for NON-wildcard route edges):
/// ordinary `import_routes` are baked route edges exactly like wildcard
/// edges — a named reexport `export type { Runtime } from './runtime'`
/// bakes the resolved `./runtime → runtime/index.ts` target into the
/// content-pinned `IndexedReady` (`import_routes` / `import_route_hash` /
/// the `ExportTarget::Reexport` canonical). When the more-specific
/// `./runtime.ts` later appears the edge retargets, but the owner's own
/// content is unchanged — an edge-currency oracle that stales ONLY
/// wildcard-bearing surfaces reuses the stale baked route.
///
/// FAILS pre-fix: the warm host keeps serving runtime/index.ts after
/// runtime.ts appears. PASSES post-fix: warm == fresh == runtime.ts, and
/// the warm host takes exactly one cheap EDGE-REFRESH for the owner (route
/// surface rebuilt from the retained content payload — no eval-program
/// re-parse of the owner; the freshly resolved target's own cold
/// materialise is the only full build).
#[test]
fn non_wildcard_route_fact_retargets_via_edge_refresh_on_warm_host() {
    let ws = Arc::new(CountingWorkspace::new());
    let index = "/workspace/index.ts";
    ws.inject_file(index, "export type { Runtime } from './runtime';\n");
    ws.inject_file(
        "/workspace/runtime/index.ts",
        "export type Runtime = number;\n",
    );
    let warm = VerterHost::new(HostConfig::default(), ws.clone());

    // FORCE the indexed surface: bakes `./runtime → runtime/index.ts`.
    let before_refresh = warm
        .ensure_indexed_ready(index)
        .expect("initial route artifact must materialize");
    let r1 = warm.resolve_named_type_export_target(index, "Runtime");
    assert_eq!(
        r1,
        Some((
            "/workspace/runtime/index.ts".to_string(),
            "Runtime".to_string()
        )),
        "precondition: Runtime resolves to the directory-index file while it \
         is the only ./runtime target"
    );

    // The more-specific `./runtime.ts` appears — a dependency-set change;
    // the owner's content stays put.
    ws.inject_file("/workspace/runtime.ts", "export type Runtime = boolean;\n");

    let fresh = VerterHost::new(HostConfig::default(), ws.clone());
    let _ = fresh.ensure_indexed_ready(index);
    let fresh_result = fresh.resolve_named_type_export_target(index, "Runtime");
    assert_eq!(
        fresh_result,
        Some(("/workspace/runtime.ts".to_string(), "Runtime".to_string())),
        "precondition: a fresh host retargets Runtime to the more-specific \
         ./runtime.ts after it appears"
    );

    warm.provenance().reset();
    let warm_result = warm.resolve_named_type_export_target(index, "Runtime");
    assert_eq!(
        warm_result, fresh_result,
        "the WARM host MUST return the SAME retargeted target as a FRESH host \
         once ./runtime.ts appears — a NON-wildcard named-reexport edge is a \
         resolve-domain answer exactly like a wildcard edge, so the warm \
         host's route entry must root on the resolution witness and recompute"
    );
    // STRONGER than the deleted edge-refresh contract: the retarget costs
    // the OWNER's artifact nothing. Its named-reexport surface names the
    // AUTHORED specifier, so the dependency-set move is answered entirely
    // on the resolve domain. The per-host `indexed_ready_materializes`
    // counter cannot state that: the appearing dependency legitimately
    // materialises ITSELF in this window, so a host-wide zero would be
    // false. The owner-precise claim is the `Arc::ptr_eq` identity
    // assertion below.
    let after_refresh = warm
        .ensure_indexed_ready(index)
        .expect("the owner's route artifact must remain published");
    assert!(
        Arc::ptr_eq(&before_refresh, &after_refresh),
        "the retarget must leave the OWNER's artifact byte-for-byte in place \
         — the same Arc, never a rebuilt one (the per-host materialize \
         counter also sees the newly-appeared TARGET's own first build, so \
         artifact identity is the owner-scoped statement)",
    );
    assert!(Arc::ptr_eq(
        &after_refresh.route_inventory,
        &after_refresh.shallow_state.route_inventory
    ));
}

/// INVERTED-POLARITY successor to
/// `edge_currency_oracle_stales_wildcard_surface_after_generation_advance`.
///
/// That case pinned the shared edge-currency oracle: ANY surface carrying a
/// cross-file edge went stale the moment the global `content_generation`
/// advanced, because the artifact baked resolved dependency canonicals that
/// a dependency-set move could retarget. The artifact bakes none now — it
/// is a content-addressed PARSE artifact — so the oracle, both of its
/// stamps, and the whole route-only edge-refresh materialise lane are
/// deleted.
///
/// This pins the replacement in BOTH directions:
///
/// * a `content_generation` advance stales NOTHING, for every edge shape
///   (a reintroduced global stamp fails here — it is the exact opposite of
///   the deleted assertions);
/// * a PARSE-ENV move still stales every surface, so the surviving gate is
///   not vacuously true.
#[test]
fn indexed_surface_reuse_is_parse_env_only_never_content_generation() {
    use crate::resolver_core::shallow_file_state::ShallowFileState;
    use rustc_hash::{FxHashMap, FxHashSet};
    use verter_session_query::inputs::shallow::{ExportTarget, ImportTarget, WildcardReexport};

    let ws = Arc::new(CountingWorkspace::new());
    ws.inject_file("/workspace/x.ts", "export const a = 1;\n");
    let host = VerterHost::new(HostConfig::default(), ws.clone());
    let baked_generation = host.ws().content_generation();

    enum EdgeShape {
        None,
        Wildcard,
        NamedReexport,
        ImportTarget,
    }
    let make_artifact = |shape: EdgeShape| {
        let routes = Arc::new(
            verter_session_query::analysis::route_inventory::ScriptRouteInventory::default(),
        );
        let mut exports = FxHashMap::default();
        let mut wildcard_reexports = Vec::new();
        let mut import_targets = FxHashMap::default();
        match shape {
            EdgeShape::None => {}
            EdgeShape::Wildcard => wildcard_reexports.push(WildcardReexport {
                owner: verter_type_expr::TopLevelOwnerId::ordinary_file(),
                source_specifier: "./dep".to_string(),
            }),
            EdgeShape::NamedReexport => {
                exports.insert(
                    "Foo".to_string(),
                    ExportTarget::Reexport {
                        source_specifier: "./dep".to_string(),
                        original_name: "Foo".to_string(),
                        is_type: true,
                    },
                );
            }
            EdgeShape::ImportTarget => {
                import_targets.insert(
                    "Foo".to_string(),
                    ImportTarget {
                        source_specifier: "./dep".to_string(),
                        imported_name: "Foo".to_string(),
                        is_namespace: false,
                    },
                );
            }
        }
        let shallow = ShallowFileState::routing_tables_only_for_test(
            [7u8; 16],
            exports,
            wildcard_reexports,
            FxHashSet::default(),
            import_targets,
            Arc::clone(&routes),
        );
        let mut artifact = crate::project_type_store::IndexedReady::new_for_test_with_state(
            [7u8; 16],
            Arc::new(shallow),
            Arc::from(""),
            Arc::from(""),
        );
        artifact.built_at_content_generation = baked_generation;
        artifact.parse_env_hash = host
            .host_view_env_hashes_for("/workspace/x.ts")
            .parse_env_hash;
        artifact
    };
    let shapes = [
        ("wildcard", make_artifact(EdgeShape::Wildcard)),
        ("named reexport", make_artifact(EdgeShape::NamedReexport)),
        ("import target", make_artifact(EdgeShape::ImportTarget)),
        ("no edges", make_artifact(EdgeShape::None)),
    ];

    for (label, artifact) in &shapes {
        assert!(
            host.indexed_surface_is_current("/workspace/x.ts", artifact),
            "precondition: the {label} surface starts current"
        );
    }

    // A dependency-set change advances content_generation past `baked`.
    ws.inject_file("/workspace/y.ts", "export const b = 2;\n");
    assert_ne!(host.ws().content_generation(), baked_generation);

    for (label, artifact) in &shapes {
        assert!(
            host.indexed_surface_is_current("/workspace/x.ts", artifact),
            "GLOBAL EDGE STAMP REINTRODUCED: the {label} surface is a PARSE \
             artifact — it bakes no resolved target — so a content-generation \
             advance anywhere in the workspace must NOT stale it. Staling here \
             is the O(published owners) re-index that was removed."
        );
    }

    // The surviving gate is real: a parse-env move DOES stale every surface.
    *host.parse_env_override.lock() = Some([0xABu8; 16]);
    for (label, artifact) in &shapes {
        assert!(
            !host.indexed_surface_is_current("/workspace/x.ts", artifact),
            "a moved parse environment MUST stale the {label} surface — its \
             framework_parse / shallow_state / decl_bodies were produced under \
             the superseded environment"
        );
    }
    *host.parse_env_override.lock() = None;
}

// The `declaration_scoped_solver_applies_omit_to_barrel_imported_types`
// characterization scenario is intentionally not exercised here. It
// asserted `engine.solve(Omit<ButtonProps, 'color'> & { status })`
// expanded an imported barrel type through a declaration-scoped solver
// surface that is not part of the final design; dispatch owns that
// surface via `ComponentMetaQueryEngine::project_expr_surface_expr`,
// and the flat-property-union shape the previous bridge returned is
// not reproducible without reintroducing the bridge. The positive-
// direction coverage for barrel / Omit routes lives in
// `component_meta_query_engine::tests`.

#[cfg(not(target_arch = "wasm32"))]
#[test]
fn get_component_meta_reuses_barrel_routes_for_multiple_late_exports() {
    let ws = Arc::new(CountingWorkspace::new());
    ws.inject_file(
        "/src/Consumer.vue",
        r#"<script setup lang="ts">
import type { TargetProps, TargetEmits } from './types'

defineProps<TargetProps>()
defineEmits<TargetEmits>()
</script>
<template><div /></template>"#,
    );
    ws.inject_file(
        "/src/types/index.ts",
        "export * from './a'\nexport * from './b'\nexport * from './target'\n",
    );
    ws.inject_file(
        "/src/types/a.ts",
        "export interface AOnly { unused: string }\n",
    );
    ws.inject_file(
        "/src/types/b.ts",
        "export interface BOnly { unused: number }\n",
    );
    ws.inject_file(
        "/src/types/target.ts",
        r#"
export interface TargetProps { label: string }
export type TargetEmits = { change: [value: string] }
"#,
    );

    let host = VerterHost::new(
        HostConfig {
            analysis_level: AnalysisLevel::Full,
            ..HostConfig::default()
        },
        ws.clone(),
    );
    assert!(
        host.ensure_loaded("/src/Consumer.vue"),
        "consumer should load from the workspace",
    );

    host.set_import_dependencies(
        "/src/Consumer.vue",
        vec![exact_dependency("./types", "/src/types/index.ts")],
    );
    host.set_import_dependencies(
        "/src/types/index.ts",
        vec![
            exact_dependency("./a", "/src/types/a.ts"),
            exact_dependency("./b", "/src/types/b.ts"),
            exact_dependency("./target", "/src/types/target.ts"),
        ],
    );

    ws.reset_reads();
    let meta = host
        .get_component_meta("/src/Consumer.vue")
        .expect("component meta should resolve for repeated late barrel exports");

    assert!(
        meta.props.iter().any(|prop| prop.name == "label"),
        "resolved props should include TargetProps.label, got {:?}",
        meta.props,
    );
    assert!(
        meta.events.iter().any(|event| event.name == "change"),
        "resolved events should include TargetEmits.change, got {:?}",
        meta.events,
    );
    // BFS shallows same-layer barrel siblings. Each sibling may be read up to
    // twice per cold request (route discovery + materialization adapters).
    assert!(
        ws.read_count("/src/types/a.ts") <= 2,
        "same-layer sibling should be read at most twice per cold request, got {} for 'a'",
        ws.read_count("/src/types/a.ts"),
    );
    assert!(
        ws.read_count("/src/types/b.ts") <= 2,
        "same-layer sibling should be read at most twice per cold request, got {} for 'b'",
        ws.read_count("/src/types/b.ts"),
    );
}

#[cfg(not(target_arch = "wasm32"))]
#[test]
fn get_component_meta_reuses_scheduler_snapshot_when_materializing_owner_indexed_ready() {
    let host = VerterHost::new_standalone(HostConfig {
        analysis_level: AnalysisLevel::Full,
        ..HostConfig::default()
    });
    upsert_vue(
        &host,
        "/src/Accordion.vue",
        r#"<script setup lang="ts">
import type { AccordionRootProps } from 'reka-ui'

const props = defineProps<AccordionRootProps>()
</script>
<template><div /></template>"#,
    );

    host.project_type_store()
        .indexed()
        .remove("/src/Accordion.vue");
    host.provenance().reset();
    let facts = host.ensure_indexed_ready("/src/Accordion.vue");
    let provenance = host.provenance_snapshot();

    assert!(
        facts.is_some(),
        "module-facts materialization should succeed for the scheduler-tracked owner",
    );
    assert!(
        provenance.indexed_ready_scheduler_snapshot_reuse > 0,
        "owner module-facts materialization should reuse the scheduler snapshot instead of rebuilding it from cached parse; saw {:?}",
        provenance,
    );
}

/// Test 8: cache ownership (cold/warm) — verifies that a second call to get_component_meta
/// on the same file hits cached state instead of re-resolving.
///
/// Full verification requires a CountingWorkspace to track file read counts,
/// which is not available in the standalone host test harness.
#[test]
fn solver_host_cache_ownership_cold_warm() {
    let host = VerterHost::new_standalone(HostConfig {
        analysis_level: crate::types::AnalysisLevel::Full,
        ..HostConfig::default()
    });
    upsert_ts(
        &host,
        "/types.ts",
        "export interface Props { label: string }",
    );
    upsert_vue(
        &host,
        "/App.vue",
        r#"<script setup lang="ts">
import type { Props } from './types'
defineProps<Props>()
</script>
<template><div /></template>"#,
    );

    // Cold call
    let meta1 = host
        .get_component_meta("/App.vue")
        .expect("cold call should produce component meta");
    let recomputes_after_cold = host
        .provenance
        .component_meta_resolved_state_recomputes
        .load(std::sync::atomic::Ordering::Relaxed);

    // Warm call — should reuse cached resolved state
    let meta2 = host
        .get_component_meta("/App.vue")
        .expect("warm call should produce component meta");
    let recomputes_after_warm = host
        .provenance
        .component_meta_resolved_state_recomputes
        .load(std::sync::atomic::Ordering::Relaxed);

    assert_eq!(
        meta1.props.len(),
        meta2.props.len(),
        "cold and warm calls should produce identical props"
    );
    assert_eq!(
        recomputes_after_cold, recomputes_after_warm,
        "warm call should not trigger additional resolved state recomputes"
    );
}

// ---------------------------------------------------------------------------
// Cross-owner file-level reuse tests (budget alignment plan)
// ---------------------------------------------------------------------------

/// Two different Vue files importing the same type from the same canonical
/// file should share the host-owned imported file state.
#[test]
fn same_canonical_file_reuses_state_across_two_owners() {
    let host = VerterHost::new_standalone(HostConfig {
        analysis_level: crate::types::AnalysisLevel::Full,
        ..HostConfig::default()
    });
    upsert_ts(
        &host,
        "/types.ts",
        "export interface Shared { label: string }",
    );
    upsert_vue(
        &host,
        "/OwnerA.vue",
        r#"<script setup lang="ts">
import type { Shared } from './types'
defineProps<Shared>()
</script>
<template><div /></template>"#,
    );
    upsert_vue(
        &host,
        "/OwnerB.vue",
        r#"<script setup lang="ts">
import type { Shared } from './types'
defineProps<Shared>()
</script>
<template><div /></template>"#,
    );

    let meta_a = host
        .get_component_meta("/OwnerA.vue")
        .expect("OwnerA should produce meta");
    let meta_b = host
        .get_component_meta("/OwnerB.vue")
        .expect("OwnerB should produce meta");

    // Both owners should see the same "label" prop from Shared
    assert_eq!(meta_a.props.len(), 1, "OwnerA should have one prop");
    assert_eq!(meta_b.props.len(), 1, "OwnerB should have one prop");
    assert_eq!(meta_a.props[0].name, "label");
    assert_eq!(meta_b.props[0].name, "label");

    // Warm call for OwnerA should not trigger recomputes
    host.provenance.reset();
    let meta_a_warm = host
        .get_component_meta("/OwnerA.vue")
        .expect("warm OwnerA should produce meta");
    let recomputes = host
        .provenance
        .component_meta_resolved_state_recomputes
        .load(std::sync::atomic::Ordering::Relaxed);
    assert_eq!(
        meta_a_warm.props.len(),
        meta_a.props.len(),
        "warm call should return identical meta"
    );
    assert_eq!(
        recomputes, 0,
        "warm call should not trigger resolved state recomputes"
    );
}
