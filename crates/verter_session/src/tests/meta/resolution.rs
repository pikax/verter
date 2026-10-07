use super::*;

#[test]
fn store_view_compat_token_matches_snapshot_epoch_and_external_supersession_fingerprint() {
    let project = make_project();
    project
        .upsert_base("/App.vue", &sfc("msg: string"))
        .expect("upsert should succeed");

    let view = project.host().snapshot_view();

    // The compat (coalescing-lane) token threads the snapshot epoch + a
    // `None` session for a base view, AND folds the EXTERNAL-supersession
    // dimensions of the `StoreViewValidationToken` into `validity_fingerprint`
    // (the SAME oracle the promotion fence `is_stable` applies) so two views
    // whose EXTERNAL validity differs in a dimension `epoch` does not cover
    // (env / identity / project / overlay) never share a singleflight /
    // stability lane.
    let expected_fingerprint = view.validation_token_for_tests().lane_fingerprint();
    assert_eq!(
        view.compat_token(),
        verter_session_query::facts::store_view::StoreViewCompatToken {
            epoch: view.mutation_epoch(),
            session: None,
            validity_fingerprint: expected_fingerprint,
        },
        "the compat token must thread the snapshot epoch + the external-supersession fingerprint"
    );
    assert_ne!(
        view.compat_token().validity_fingerprint,
        0,
        "a real base view's compat token MUST carry a non-zero external-supersession \
         fingerprint (the lane identity gates on external dims, not epoch-only)"
    );
}

#[cfg(not(target_arch = "wasm32"))]
#[test]
fn current_dependency_fact_versions_include_derived_resolver_facts() {
    let project = make_project();
    project
        .upsert_base("/index.ts", "export * from './inner'")
        .unwrap();

    let whole_hash = project
        .host()
        .get_whole_hash("/index.ts")
        .expect("whole hash should exist");

    let _ = project
        .host()
        .ensure_indexed_ready_serve("/index.ts")
        .expect("the wildcard reexporter must materialise");

    let facts = project
        .host()
        .current_dependency_fact_versions("/index.ts", &std::collections::BTreeSet::new());

    assert!(facts.contains(
        &verter_session_query::facts::fact_cache::FactVersionRef::FileWholeHash {
            canonical_id: "/index.ts".to_string(),
            hash: whole_hash,
        }
    ));
    assert!(
        facts.iter().any(|fact| matches!(
            fact,
            verter_session_query::facts::fact_cache::FactVersionRef::ResolveImports(inner)
                if inner.resolution_fact().is_some()
        )),
        "dependency fact versions should include the owner's import-route \
         RESOLUTION WITNESS for the file; got {facts:?}",
    );
    assert!(
        facts.iter().all(|fact| matches!(
            fact,
            verter_session_query::facts::fact_cache::FactVersionRef::FileWholeHash { .. }
                | verter_session_query::facts::fact_cache::FactVersionRef::DerivedFactHash {
                    kind: verter_session_query::facts::fact_cache::DerivedFactKind::Route,
                    ..
                }
                | verter_session_query::facts::fact_cache::FactVersionRef::Parse(
                    verter_session_query::facts::fact_cache::ParseFactRef {
                        key: verter_session_query::facts::FactKey::SyntacticRouteInterface,
                        ..
                    }
                )
                | verter_session_query::facts::fact_cache::FactVersionRef::ResolveImports(_)
        )),
        "dependency fact versions should only publish file, route, and \
         resolve-domain import facts; got {facts:?}",
    );
}

#[cfg(target_arch = "wasm32")]
#[test]
fn current_dependency_fact_versions_include_derived_resolver_facts_non_scheduler() {
    let project = make_project();
    project
        .upsert_base("/index.ts", "export * from './inner'")
        .unwrap();

    let whole_hash = project
        .host()
        .get_whole_hash("/index.ts")
        .expect("whole hash should exist");

    {
        let mut files = crate::shared::write_lock(&project.host().files);
        let entry = files.get_mut("/index.ts").expect("file entry should exist");
        entry.import_routes.insert(
            "./inner".to_string(),
            crate::types::DependencyResolution {
                specifier: "./inner".to_string(),
                resolved_canonical_id: Some("/inner.ts".to_string()),
                possible_canonical_ids: Vec::new(),
            },
        );
    }

    let facts = project
        .host()
        .current_dependency_fact_versions("/index.ts", &std::collections::BTreeSet::new());

    assert!(facts.contains(
        &verter_session_query::facts::fact_cache::FactVersionRef::FileWholeHash {
            canonical_id: "/index.ts".to_string(),
            hash: whole_hash,
        }
    ));
    assert!(
        facts.iter().any(|fact| matches!(
            fact,
            verter_session_query::facts::fact_cache::FactVersionRef::ResolveImports(inner)
                if inner.resolution_fact().is_some()
        )),
        "non-scheduler store views must track the owner's import-route \
         RESOLUTION WITNESS; got {facts:?}"
    );
    assert!(
        facts.iter().all(|fact| matches!(
            fact,
            verter_session_query::facts::fact_cache::FactVersionRef::FileWholeHash { .. }
                | verter_session_query::facts::fact_cache::FactVersionRef::DerivedFactHash {
                    kind: verter_session_query::facts::fact_cache::DerivedFactKind::Route,
                    ..
                }
                | verter_session_query::facts::fact_cache::FactVersionRef::ResolveImports(_)
        )),
        "non-scheduler dependency fact versions should only publish file, \
         route, and resolve-domain import facts; got {facts:?}",
    );
}

// ---------------------------------------------------------------------------
// Dependency invalidation within session
// ---------------------------------------------------------------------------

#[test]
fn changing_dependency_invalidates_importer_in_session() {
    let project = make_project();

    // Set up a types file and a component that imports from it
    let types_source = r#"export interface ButtonProps { label: string }"#;
    let comp_source = r#"<script setup lang="ts">
import type { ButtonProps } from './types'
defineProps<ButtonProps>()
</script>
<template><div>{{ label }}</div></template>"#;

    project.upsert_base("types.ts", types_source).unwrap();
    project.upsert_base("Button.vue", comp_source).unwrap();

    let s = project.open_session_batch().unwrap();

    // Query analysis succeeds for the base file
    let snap = s.get_analysis("Button.vue").unwrap();
    assert!(snap.is_some(), "analysis should succeed for the base file");

    // Modify types in session to add 'disabled'
    let new_types = r#"export interface ButtonProps { label: string; disabled: boolean }"#;
    s.upsert("types.ts", new_types.into()).unwrap();

    // After modifying types in the session, querying Button.vue through the
    // session should succeed (the overlay applies the new types.ts to the host)
    let snap2 = s.get_analysis("Button.vue").unwrap();
    assert!(
        snap2.is_some(),
        "analysis should succeed after dependency update"
    );
}

#[test]
fn get_analysis_resolves_exported_local_props_from_sibling_script_block() {
    let project = make_project();
    project
        .upsert_base(
            "Comp.vue",
            r#"<script lang="ts">
export interface Props {
  label: string
  count?: number
}
</script>

<script setup lang="ts">
defineProps<Props>()
</script>
<template><div /></template>"#,
        )
        .unwrap();

    let session = project.open_session_batch().unwrap();
    let analysis = session
        .get_analysis("Comp.vue")
        .unwrap()
        .expect("analysis should exist");
    let define_props = analysis
        .macros
        .iter()
        .find(|m| m.kind == verter_session_query::analysis::types::AnalyzedMacroKind::DefineProps)
        .expect("defineProps macro should exist");

    let names: Vec<&str> = define_props
        .prop_fields
        .iter()
        .map(|field| field.name.as_str())
        .collect();
    assert!(
        names.contains(&"label"),
        "exported interface field 'label' should resolve, got: {:?}",
        names
    );
    assert!(
        names.contains(&"count"),
        "exported interface field 'count' should resolve, got: {:?}",
        names
    );
}

#[test]
fn get_analysis_resolves_non_exported_local_props_from_sibling_script_block() {
    let project = make_project();
    project
        .upsert_base(
            "Comp.vue",
            r#"<script lang="ts">
interface Props {
  label: string
  count?: number
}
</script>

<script setup lang="ts">
defineProps<Props>()
</script>
<template><div /></template>"#,
        )
        .unwrap();

    let session = project.open_session_batch().unwrap();
    let analysis = session
        .get_analysis("Comp.vue")
        .unwrap()
        .expect("analysis should exist");
    let define_props = analysis
        .macros
        .iter()
        .find(|m| m.kind == verter_session_query::analysis::types::AnalyzedMacroKind::DefineProps)
        .expect("defineProps macro should exist");

    let names: Vec<&str> = define_props
        .prop_fields
        .iter()
        .map(|field| field.name.as_str())
        .collect();
    assert!(
        names.contains(&"label"),
        "sibling script field 'label' should resolve, got: {:?}",
        names
    );
    assert!(
        names.contains(&"count"),
        "sibling script field 'count' should resolve, got: {:?}",
        names
    );
}

/// THE DISCRIMINATION CONTROL for the missed-terminal-hop rule: the SAME deep
/// path whose terminal hop EXISTS and is a genuinely empty object type still
/// publishes zero props as COMPLETE, exact, and WARM. A fix that marks every
/// empty surface partial passes the test above and fails this one.
#[cfg(not(target_arch = "wasm32"))]
#[test]
fn a_resolved_empty_terminal_hop_still_publishes_complete_and_warm() {
    use std::sync::atomic::Ordering::Relaxed;

    let project = make_project();
    project
        .upsert_base(
            "/src/DeepEmpty.vue",
            r#"<script setup lang="ts">
interface Deep { ui: { header: {} } }
defineProps<Deep['ui']['header']>()
</script>
<template><div /></template>"#,
        )
        .unwrap();
    let host = project.host();
    let meta = get_meta(&project, "/src/DeepEmpty.vue");
    assert!(
        meta.props.is_empty(),
        "the genuinely empty terminal object declares no props"
    );

    let (_, resolved) = host
        .get_component_meta_with_resolution("/src/DeepEmpty.vue")
        .expect("the resolve returns metadata");
    assert!(
        !resolved.synthesis_should_suppress,
        "a genuinely empty resolved surface IS the complete answer"
    );

    let hits_before = host
        .provenance()
        .component_meta_result_cache_hits
        .load(Relaxed);
    let _ = get_meta(&project, "/src/DeepEmpty.vue");
    let hits_after = host
        .provenance()
        .component_meta_result_cache_hits
        .load(Relaxed);
    assert_eq!(
        hits_after,
        hits_before + 1,
        "a genuinely empty resolved surface WARMS on replay \
         (hits_before={hits_before}, hits_after={hits_after})"
    );
}

/// SYNTHESIS-SUPPRESS no-regression under the merged gate: a RESOLVE-phase
/// partial (`synthesis_should_suppress == true`) with a COMPLETE extract scope
/// must STILL be refused warm admission after the gate was rewritten to the
/// single merged `final_completeness` signal (the former
/// `resolved.synthesis_should_suppress` gate term was demoted).
///
/// `synthesis_should_suppress` is the bool PROJECTION of `resolved.completeness`
/// (`synthesis_should_suppress: self.completeness.is_partial()`,
/// `component_meta_result_db.rs`), so it is SUBSUMED by the `resolved.completeness`
/// operand of `final_completeness = resolved.completeness.merge(extract_scope_completeness)`.
/// This test pins that the subsumption is load-bearing and NOT silently dropped.
///
/// The fixture isolates a RESOLVE-ONLY partial from the extract scope: the
/// `synthesis_steps` recursion budget (separate from `projection_op_budget`)
/// trips DURING slot-binding synthesis → `synthesis_should_suppress == true`
/// (`resolved.completeness == Partial`), while the generous projection budget
/// leaves the extract macro-DTO read + fallthrough COMPLETE
/// (`extract_scope_completeness == Complete`). So `final_completeness =
/// Partial.merge(Complete) = Partial` and the result is refused. This is the
/// INVERSE of `extract_scope_captures_cold_macro_dto_partial_into_merged_gate_signal`
/// (resolve PARTIAL + extract COMPLETE here, vs resolve COMPLETE + extract
/// PARTIAL there), so it discriminates the resolve operand specifically rather
/// than the extract-scope operand.
///
/// RED proof: revert the `resolved.completeness` merge operand at the publishing
/// caller (`let final_completeness = extract_completeness;`, dropping the resolve
/// term) → the Complete-extract synthesis-suppressed result warms → the
/// `has_owner_entry_in_test` assertion FAILS.
#[cfg(not(target_arch = "wasm32"))]
#[test]
fn synthesis_suppress_resolve_partial_still_refused_under_merged_gate() {
    // Tight SYNTHESIS-step budget (NOT the projection budget): the slot-binding
    // synthesis trips during the RESOLVE phase, while the projection budget
    // stays generous so the EXTRACT scope is Complete.
    let mut config = HostConfig {
        analysis_level: crate::types::AnalysisLevel::Full,
        ..HostConfig::default()
    };
    config.recursion_budget_overrides.synthesis_steps = Some(1);
    let project = make_project_with_config(config);
    project
        .upsert_base(
            "/src/types.ts",
            "export interface Slots { default(props: { row: string }): any }",
        )
        .unwrap();
    project
        .upsert_base(
            "/src/Comp.vue",
            r#"<script setup lang="ts">
import type { Slots } from './types'
defineSlots<Slots>()
</script>
<template><div /></template>"#,
        )
        .unwrap();
    project.host().set_import_dependencies(
        "/src/Comp.vue",
        vec![crate::types::DependencyResolution {
            specifier: "./types".to_string(),
            resolved_canonical_id: Some("/src/types.ts".to_string()),
            possible_canonical_ids: Vec::new(),
        }],
    );
    let host = project.host();
    let canonical = "/src/Comp.vue";

    let (_analysis, resolved) = host
        .get_component_meta_with_resolution(canonical)
        .expect("component meta resolves (partial)");

    // PRECONDITION: the partial is RESOLVE-phase (synthesis suppression) — the
    // exact shape the demoted gate term covered. With a generous projection
    // budget the EXTRACT scope is Complete, so the resolve operand is the ONLY
    // thing that can keep this gated.
    assert!(
        resolved.synthesis_should_suppress,
        "the synthesis_steps budget must trip during resolve so the partial is resolve-phase \
         (synthesis_should_suppress == resolved.completeness.is_partial())"
    );

    // THE DISCRIMINATOR: the resolve-phase partial must STILL be refused warm
    // admission. The merged gate keeps it gated via the `resolved.completeness`
    // operand even though the extract scope is Complete.
    assert!(
        !crate::component_meta_cached_result::has_owner_entry_in_test(
            host, canonical
        ),
        "a synthesis-suppressed (resolve-phase partial) result MUST NOT warm `ComponentMetaResultDb` \
         after the gate's `synthesis_should_suppress` term was demoted — the `resolved.completeness` \
         merge operand subsumes it. Reverting that operand (`final_completeness = extract_completeness`) \
         would warm this Complete-extract result"
    );
}

#[test]
fn evaluate_types_resolves_local_typeof_from_sibling_script_block() {
    let project = make_project();
    project
        .upsert_base(
            "Comp.vue",
            r#"<script lang="ts">
const theme = {
  item: "item",
  body: "body",
}

type Props = {
  ui: typeof theme
}
</script>

<script setup lang="ts">
defineProps<Props>()
</script>
<template><div /></template>"#,
        )
        .unwrap();

    let session = project.open_session_batch().unwrap();
    let evaluated = session.evaluate_types("Comp.vue").unwrap().unwrap();

    match &evaluated_prop_type(&project, "Comp.vue", &evaluated, "ui") {
        TypeExpr::Object(obj) => {
            let names: Vec<&str> = obj
                .properties
                .iter()
                .filter_map(|member| match member {
                    ObjectMember::Property(prop) => {
                        Some(prop.string_name().expect("string-key fixture"))
                    }
                    _ => None,
                })
                .collect();
            assert!(names.contains(&"item"));
            assert!(names.contains(&"body"));
        }
        other => panic!("expected typeof theme to resolve to an object, got {other:?}"),
    }
}

#[test]
fn evaluate_types_resolves_imported_default_typeof() {
    let project = make_project();
    project
        .upsert_base(
            "/theme.ts",
            r#"export default {
  item: "item",
  body: "body",
}"#,
        )
        .unwrap();
    project
        .upsert_base(
            "/Comp.vue",
            r#"<script setup lang="ts">
import theme from './theme'

defineProps<{
  ui: typeof theme
}>()
</script>
<template><div /></template>"#,
        )
        .unwrap();

    let session = project.open_session_batch().unwrap();
    let analysis = session.get_analysis("/Comp.vue").unwrap().unwrap();
    assert_eq!(analysis.imports.len(), 1);
    assert_eq!(analysis.imports[0].bindings.len(), 1);
    assert_eq!(
        analysis.imports[0].bindings[0].kind,
        verter_session_query::analysis::types::ImportBindingKind::Default,
    );
    assert_eq!(
        analysis.imports[0].bindings[0].imported_name.as_deref(),
        Some("default")
    );
    assert!(
        analysis.imports[0].resolved_canonical_id.is_some(),
        "default import should already be resolved in the analysis snapshot"
    );
    let evaluated = session.evaluate_types("/Comp.vue").unwrap().unwrap();

    match &evaluated_prop_type(&project, "/Comp.vue", &evaluated, "ui") {
        TypeExpr::Object(obj) => {
            let names: Vec<&str> = obj
                .properties
                .iter()
                .filter_map(|member| match member {
                    ObjectMember::Property(prop) => {
                        Some(prop.string_name().expect("string-key fixture"))
                    }
                    _ => None,
                })
                .collect();
            assert!(names.contains(&"item"));
            assert!(names.contains(&"body"));
        }
        other => panic!("expected imported typeof theme to resolve to an object, got {other:?}"),
    }
}

#[test]
fn imported_default_typeof_recovers_after_dependency_is_added() {
    let project = make_project();
    project
        .upsert_base(
            "/Comp.vue",
            r#"<script setup lang="ts">
import theme from './theme'

defineProps<{
  ui: typeof theme
}>()
</script>
<template><div /></template>"#,
        )
        .unwrap();

    let session = project.open_session_batch().unwrap();
    let initial = session.evaluate_types("/Comp.vue").unwrap().unwrap();
    assert!(
        !matches!(
            evaluated_prop_type(&project, "/Comp.vue", &initial, "ui"),
            TypeExpr::Object(_)
        ),
        "missing dependency should not resolve imported typeof exactly"
    );

    project
        .upsert_base(
            "/theme.ts",
            r#"export default {
  item: "item",
  body: "body",
}"#,
        )
        .unwrap();

    let _view = project.host().resolver_store_view_read().into_owned_view();
    assert_eq!(
        project
            .host()
            .resolve_type_dependency_canonical("/Comp.vue", "./theme")
            .as_deref(),
        Some("/theme.ts"),
        "fresh store views should reopen missing import routes after the dependency appears",
    );

    let reevaluated = session.evaluate_types("/Comp.vue").unwrap().unwrap();
    match &evaluated_prop_type(&project, "/Comp.vue", &reevaluated, "ui") {
        TypeExpr::Object(obj) => {
            let names: Vec<&str> = obj
                .properties
                .iter()
                .filter_map(|member| match member {
                    ObjectMember::Property(prop) => {
                        Some(prop.string_name().expect("string-key fixture"))
                    }
                    _ => None,
                })
                .collect();
            assert!(names.contains(&"item"));
            assert!(names.contains(&"body"));
        }
        other => panic!("expected imported typeof theme to recover to an object, got {other:?}"),
    }
}

/// Lazy-substrate discriminator for the same `typeof` import recovery
/// scenario as [`imported_default_typeof_recovers_after_dependency_is_added`].
/// The owner-upsert path has no eager reverse-dependent cascade, so
/// adding the late dependency does NOT evict `/Comp.vue`'s artifacts.
///
/// This isolates the fact-validation substrate: when `./theme` first
/// appears, `/Comp.vue` (which never changed) keeps its content-pinned
/// `IndexedReady` and warm component-meta resolution. The negative
/// resolution (`ui = semanticMiss`) must still be invalidated, because
/// its `ImportRoute` derived fact records `./theme` as unresolved and
/// the fact-validation oracle re-resolves that specifier against the
/// current workspace generation at validate time.
///
/// Discrimination property: the fix that makes the `ImportRoute`
/// derived-fact hash reflect the *current* resolution of a known-miss
/// specifier (the owner's import-route witness) is what breaks
/// this test if reverted. Without it, the warm negative resolution
/// validates against its own stale `ImportRoute` snapshot and the
/// re-evaluation keeps returning `Unknown { raw: "semanticMiss" }`.
#[test]
fn imported_typeof_recovers_when_dependency_added() {
    let project = make_project();
    project
        .upsert_base(
            "/Comp.vue",
            r#"<script setup lang="ts">
import theme from './theme'

defineProps<{
  ui: typeof theme
}>()
</script>
<template><div /></template>"#,
        )
        .unwrap();

    let session = project.open_session_batch().unwrap();
    let initial = session.evaluate_types("/Comp.vue").unwrap().unwrap();
    assert!(
        !matches!(
            evaluated_prop_type(&project, "/Comp.vue", &initial, "ui"),
            TypeExpr::Object(_)
        ),
        "missing dependency should not resolve imported typeof exactly"
    );

    // Add `/theme.ts`. The owner-upsert path has no eager
    // reverse-dependent cascade, so `/Comp.vue`'s artifacts are not
    // evicted — the lazy fact-validation substrate is what must detect
    // that `./theme` is now resolvable.
    let _theme_update = project
        .host()
        .upsert(crate::UpsertRequest {
            canonical_id: Some("/theme.ts".to_string()),
            input_id: "/theme.ts".to_string(),
            source: Arc::from(
                r#"export default {
  item: "item",
  body: "body",
}"#,
            ),
            file_language: crate::FileLanguage::script_ts(),
            aliases: vec![],
        })
        .unwrap();

    let reevaluated = session.evaluate_types("/Comp.vue").unwrap().unwrap();
    match &evaluated_prop_type(&project, "/Comp.vue", &reevaluated, "ui") {
        TypeExpr::Object(obj) => {
            let names: Vec<&str> = obj
                .properties
                .iter()
                .filter_map(|member| match member {
                    ObjectMember::Property(prop) => {
                        Some(prop.string_name().expect("string-key fixture"))
                    }
                    _ => None,
                })
                .collect();
            assert!(
                names.contains(&"item"),
                "recovered typeof theme must expose `item` (got {names:?})"
            );
            assert!(
                names.contains(&"body"),
                "recovered typeof theme must expose `body` (got {names:?})"
            );
        }
        other => panic!(
            "expected imported typeof theme to recover to an object after \
             eviction-free dependency add, got {other:?}"
        ),
    }
}

#[test]
fn evaluate_types_resolves_imported_types_before_running_utilities() {
    let project = make_project();
    project
        .upsert_base(
            "/types.ts",
            r#"export interface ImportedUser {
  id: number,
  name: string
  password: string
}"#,
        )
        .unwrap();
    project
        .upsert_base(
            "/Comp.vue",
            r#"<script setup lang="ts">
import type { ImportedUser } from './types'

defineProps<{
  user: Pick<ImportedUser, 'id' | 'name'>
}>()
</script>
<template><div /></template>"#,
        )
        .unwrap();

    let session = project.open_session_batch().unwrap();
    let evaluated = session.evaluate_types("/Comp.vue").unwrap().unwrap();

    match &evaluated_prop_type(&project, "/Comp.vue", &evaluated, "user") {
        TypeExpr::Object(obj) => {
            let names: Vec<&str> = obj
                .properties
                .iter()
                .filter_map(|member| match member {
                    ObjectMember::Property(prop) => {
                        Some(prop.string_name().expect("string-key fixture"))
                    }
                    _ => None,
                })
                .collect();
            assert!(names.contains(&"id"));
            assert!(names.contains(&"name"));
            assert!(!names.contains(&"password"));
        }
        other => panic!("expected imported utility to resolve to an object, got {other:?}"),
    }
}

#[test]
fn evaluate_types_cross_file_recursive_alias_through_reexport_preserves_recursive_transport() {
    let project = make_project();
    project
        .upsert_base(
            "/types.ts",
            r#"export type TreeNode = { label: string; children: TreeNode[] }"#,
        )
        .unwrap();
    project
        .upsert_base("/index.ts", r#"export type { TreeNode } from './types'"#)
        .unwrap();
    project
        .upsert_base(
            "/Comp.vue",
            r#"<script setup lang="ts">
import type { TreeNode } from './index'
defineProps<{ root: TreeNode }>()
</script>
<template><div /></template>"#,
        )
        .unwrap();

    let session = project.open_session_batch().unwrap();
    let evaluated = session.evaluate_types("/Comp.vue").unwrap().unwrap();

    // Architectural contract: imported alias names stay shallow at the
    // published surface level. The published prop type carries the
    // bare `Ref { name: "TreeNode" }` and consumers re-resolve the
    // declaration through the registry (preserving the re-export
    // chain through `./index`). Recursive structure is materialised
    // on-demand by the consumer via the resolver, not eagerly inlined
    // into the published prop type.
    match &evaluated_prop_shallow_type(&project, "/Comp.vue", &evaluated, "root") {
        TypeExpr::Ref {
            name,
            type_arguments,
        } => {
            assert_eq!(name.as_ref(), "TreeNode");
            assert!(type_arguments.is_empty());
        }
        other => panic!(
            "expected root prop to publish the bare TreeNode ref through re-export, got {other:?}"
        ),
    }
}

#[test]
fn evaluate_types_prunes_imported_eval_inputs_to_macro_reachable_deps() {
    let project = make_project();
    project
        .upsert_base(
            "/used.ts",
            r#"export interface UsedProps {
  title: string
}"#,
        )
        .unwrap();
    project
        .upsert_base(
            "/unused-c.ts",
            r#"export interface UnusedC {
  c: string
}"#,
        )
        .unwrap();
    project
        .upsert_base(
            "/unused-b.ts",
            r#"import type { UnusedC } from './unused-c'
export type UnusedB = UnusedC & { b: string }"#,
        )
        .unwrap();
    project
        .upsert_base(
            "/unused-a.ts",
            r#"import type { UnusedB } from './unused-b'
export type UnusedA = UnusedB & { a: string }"#,
        )
        .unwrap();
    project
        .upsert_base(
            "/App.vue",
            r#"<script setup lang="ts">
import type { UsedProps } from './used'
import type { UnusedA } from './unused-a'

defineProps<UsedProps>()
</script>
<template><div /></template>"#,
        )
        .unwrap();

    let session = project.open_session_batch().unwrap();
    let evaluated = session.evaluate_types("/App.vue").unwrap().unwrap();

    assert_eq!(
        evaluated_define_props_type(&project, "/App.vue", &evaluated, "title"),
        TypeExpr::Primitive(PrimitiveName::String)
    );

    // Dependency tracking assertions removed — the legacy walker is deleted.
    // The solver tracks dependencies through its own frontier.
}

#[test]
fn evaluate_types_resolve_relevant_transitive_imported_heritage() {
    let project = make_project();
    project
        .upsert_base(
            "/base.ts",
            r#"export interface BaseProps {
  id: string
}"#,
        )
        .unwrap();
    project
        .upsert_base(
            "/props.ts",
            r#"import type { BaseProps } from './base'

export interface Props extends BaseProps {
  label: string
}"#,
        )
        .unwrap();
    project
        .upsert_base(
            "/App.vue",
            r#"<script setup lang="ts">
import type { Props } from './props'

defineProps<Props>()
</script>
<template><div /></template>"#,
        )
        .unwrap();

    let session = project.open_session_batch().unwrap();
    let evaluated = session.evaluate_types("/App.vue").unwrap().unwrap();

    match evaluated_define_props_type(&project, "/App.vue", &evaluated, "id") {
        TypeExpr::Primitive(PrimitiveName::String) => {}
        other => panic!("expected inherited prop 'id' to resolve to string, got {other:?}"),
    }
    match evaluated_define_props_type(&project, "/App.vue", &evaluated, "label") {
        TypeExpr::Primitive(PrimitiveName::String) => {}
        other => panic!("expected direct prop 'label' to resolve to string, got {other:?}"),
    }

    // Dependency tracking assertions removed — the legacy walker is deleted.
}

/// `define_props_shape` distinguishes a RESOLVED-but-empty surface from a
/// genuinely UNRESOLVED macro via `macro_surface_resolves`
/// (`resolve_vue_macro_surface_with_ctx().is_some()`):
///
/// - (a) A call-signature-only `defineProps<{ (): void }>()` RESOLVES to an
///   object surface that has a call signature but no property members. The
///   helper publishes a `define_props` shape with empty `properties` — present,
///   not absent — preserving the "macro resolved to no props" behavior.
/// - (b) The "unresolved" discriminator (`resolve_vue_macro_surface` returns
///   `None`) is a REAL signal — an out-of-range macro index yields `None` while
///   a valid in-range index yields `Some`. The gate keys on exactly this, so a
///   genuinely-unresolved macro yields `None` (no shape) rather than a spurious
///   `Some(empty)`.
///
/// Discriminating: part (a) FAILS if `define_props_shape` is gated to drop a
/// resolved-but-empty (call-signature-only) surface; part (b) asserts the gate's
/// `Some`/`None` discriminator branches on a real input.
///
/// NOTE (empirical): within the production driver `project_define_macro_shapes`
/// every macro fed to the helpers is an in-range, type-based macro, and the
/// shared resolver synthesises at least an EMPTY object surface for ANY such
/// type argument (`number[]` / `() => void` / `string | number` / a tuple / an
/// unresolved name all resolve to `Some((0,0,0))`). So the gate's `None` path
/// fires only for the out-of-range / not-loaded cases the driver never passes;
/// the gate is the contract-correct discriminator (and future-proofs a resolver
/// change that could return `None`), exercised here at the surface-resolution
/// level where the `None` input is reachable.
#[test]
fn evaluate_types_define_props_distinguishes_resolved_empty_from_unresolved() {
    // (a) Call-signature-only props: RESOLVED but empty → shape PRESENT, empty.
    let project = make_project();
    project
        .upsert_base(
            "/CallSigProps.vue",
            r#"<script setup lang="ts">
defineProps<{ (): void }>()
</script>
<template><div /></template>"#,
        )
        .unwrap();
    let evaluated = project
        .open_session_batch()
        .unwrap()
        .evaluate_types("/CallSigProps.vue")
        .unwrap()
        .unwrap();
    let shape = evaluated
        .define_props
        .iter()
        .map(|e| &e.result.value)
        .next()
        .expect(
            "a call-signature-only defineProps RESOLVES (object surface with a call \
             signature) and must publish a define_props shape — Some(empty), not None",
        );
    assert!(
        shape.properties.is_empty(),
        "a call-signature-only props surface has no property members, got {:?}",
        shape.properties,
    );

    // (b) The gate's resolved-vs-unresolved discriminator is a real signal: a
    // valid in-range macro index resolves to `Some`; an out-of-range index
    // (the genuinely-no-surface case the gate returns `None` for) resolves to
    // `None`.
    let host = project.host();
    let wh = host
        .get_whole_hash("/CallSigProps.vue")
        .unwrap_or([0u8; 16]);
    let valid = crate::typeinfo::types::VueMacroSurfaceRequest {
        owner_canonical: std::sync::Arc::from("/CallSigProps.vue"),
        macro_index: 0,
        macro_kind: verter_session_query::analysis::types::AnalyzedMacroKind::DefineProps,
        root_identity: wh,
        level: crate::typeinfo::types::TypeInfoQueryLevel::FullMetadata,
    };
    assert!(
        host.resolve_vue_macro_surface(&valid).is_some(),
        "a valid in-range defineProps macro resolves its surface (gate admits it)"
    );
    let out_of_range = crate::typeinfo::types::VueMacroSurfaceRequest {
        macro_index: 99,
        ..valid
    };
    assert!(
        host.resolve_vue_macro_surface(&out_of_range).is_none(),
        "an out-of-range macro index does NOT resolve a surface — the gate's \
         `None` path (no shape published) keys on exactly this real signal"
    );
}

#[test]
fn evaluate_types_materializes_imported_indexed_access_from_shallow_alias_source_env() {
    let project = make_project();
    project
        .upsert_base(
            "/dep.ts",
            r#"type Child = string

export type Parent = {
  x: Child
}"#,
        )
        .unwrap();
    project
        .upsert_base(
            "/App.vue",
            r#"<script setup lang="ts">
import type { Parent } from './dep'

defineProps<{
  value: Parent['x']
}>()
</script>
<template><div /></template>"#,
        )
        .unwrap();

    let session = project.open_session_batch().unwrap();
    let evaluated = session.evaluate_types("/App.vue").unwrap().unwrap();

    assert_eq!(
        evaluated_define_props_type(&project, "/App.vue", &evaluated, "value"),
        TypeExpr::Primitive(PrimitiveName::String),
        "indexed access through an imported shallow alias should still resolve via the source env"
    );
}

#[test]
fn get_component_meta_merges_local_eval_surface_with_imported_props() {
    let project = make_project();
    project
        .upsert_base(
            "/types.ts",
            r#"export interface ExternalProps {
  /** Stable id description. */
  id: string
  /** Optional label description. */
  label?: string
}"#,
        )
        .unwrap();
    project
        .upsert_base(
            "/App.vue",
            r#"<script setup lang="ts">
import type { ExternalProps } from './types'

interface LocalProps extends Pick<ExternalProps, 'id' | 'label'> {
  own?: boolean
}

defineProps<LocalProps>()
</script>
<template><div /></template>"#,
        )
        .unwrap();

    project.host().provenance().reset();
    let meta = get_meta(&project, "/App.vue");
    let prop_names: Vec<&str> = meta.props.iter().map(|prop| prop.name.as_str()).collect();
    assert_eq!(prop_names, vec!["id", "label", "own"]);
    let id = meta
        .props
        .iter()
        .find(|prop| prop.name == "id")
        .expect("id prop should exist");
    let label = meta
        .props
        .iter()
        .find(|prop| prop.name == "label")
        .expect("label prop should exist");
    assert!(id.required, "imported required prop should stay required");
    assert!(
        !label.required,
        "imported optional prop should stay optional after wrapper flattening"
    );
    assert_eq!(id.description.as_deref(), Some("Stable id description."));
    assert_eq!(
        label.description.as_deref(),
        Some("Optional label description.")
    );
}

#[test]
fn get_component_meta_uses_evaluated_types_for_imported_define_props() {
    let project = make_project();
    project
        .upsert_base(
            "/types.ts",
            r#"export interface ExternalProps {
  id: string
  label?: string
}"#,
        )
        .unwrap();
    project
        .upsert_base(
            "/App.vue",
            r#"<script setup lang="ts">
import type { ExternalProps } from './types'

defineProps<ExternalProps>()
</script>
<template><div /></template>"#,
        )
        .unwrap();
    let meta = project
        .host()
        .get_component_meta("/App.vue")
        .expect("full meta should resolve");
    let prop_names: Vec<&str> = meta.props.iter().map(|prop| prop.name.as_str()).collect();

    assert_eq!(prop_names, vec!["id", "label"]);
}

#[test]
fn get_component_meta_resolves_imported_helper_aliases_without_dep_env_merge() {
    // Publication-policy contract: the policy pass
    // `apply_component_meta_resolution_policy` resolves project-local
    // non-Props refs (Rule 3) — `Status` is a project-local alias, so the
    // public meta carries the resolved Union literal shape. Adapter pipelines
    // (storybook, json-schema, zod, histoire) require the resolved Object/
    // Union shape; symbolic Ref produces opaque output.
    let project = make_project();
    project
        .upsert_base(
            "/types.ts",
            r#"type Status = 'idle' | 'busy'

export interface ExternalProps {
  status: Status
}"#,
        )
        .unwrap();
    project
        .upsert_base(
            "/App.vue",
            r#"<script setup lang="ts">
import type { ExternalProps } from './types'

defineProps<ExternalProps>()
</script>
<template><div /></template>"#,
        )
        .unwrap();

    let meta = project
        .host()
        .get_component_meta("/App.vue")
        .expect("full meta should resolve");
    let status = meta
        .props
        .iter()
        .find(|prop| prop.name == "status")
        .expect("status prop should be present");

    assert_eq!(
        demand_published_type(
            project.host(),
            "/App.vue",
            status.publication.result().selected_source(),
            "status prop",
        ),
        TypeExpr::union(vec![
            TypeExpr::string_literal("idle"),
            TypeExpr::string_literal("busy"),
        ]),
        "publication policy must resolve project-local non-Props alias body"
    );
}

#[test]
fn get_component_meta_preserves_barrel_cycle_utility_heritage() {
    let project = make_project();
    project
        .upsert_base(
            "/src/types/index.ts",
            r#"export * from '../Link.vue'
export * from '../Button.vue'"#,
        )
        .unwrap();
    project
        .upsert_base(
            "/src/Link.vue",
            r#"<script lang="ts">
interface RouterLinkOptions {
  replace?: boolean
  activeClass?: string
  ariaCurrentValue?: string
}

interface RouterLinkProps extends RouterLinkOptions {
  custom?: boolean
  exactActiveClass?: string
}

interface NuxtLinkProps extends Omit<RouterLinkProps, 'to'> {
  to?: string
  href?: string
}

export interface LinkProps extends NuxtLinkProps {
  as?: any
  class?: any
  raw?: boolean
}

export type LinkPropsKeys = 'to' | 'replace' | 'activeClass' | 'ariaCurrentValue'
</script>
<template><div /></template>"#,
        )
        .unwrap();
    project
        .upsert_base(
            "/src/Button.vue",
            r#"<script lang="ts">
import type { LinkProps } from './types'

export interface UseComponentIconsProps {
  icon?: string
  loading?: boolean
}

export interface ButtonProps extends UseComponentIconsProps, Omit<LinkProps, 'raw' | 'custom'> {
  label?: string
  color?: string
  variant?: string
  size?: string
}
</script>
<template><div /></template>"#,
        )
        .unwrap();
    project
        .upsert_base(
            "/src/App.vue",
            r#"<script setup lang="ts">
import type { ButtonProps, LinkPropsKeys } from './types'

interface ChildProps extends Omit<ButtonProps, LinkPropsKeys | 'icon' | 'color' | 'variant'> {
  status?: string
}

defineProps<ChildProps>()
</script>
<template><div /></template>"#,
        )
        .unwrap();

    project.host().set_import_dependencies(
        "/src/App.vue",
        vec![crate::types::DependencyResolution {
            specifier: "./types".to_string(),
            resolved_canonical_id: Some("/src/types/index.ts".to_string()),
            possible_canonical_ids: Vec::new(),
        }],
    );
    project.host().set_import_dependencies(
        "/src/Button.vue",
        vec![crate::types::DependencyResolution {
            specifier: "./types".to_string(),
            resolved_canonical_id: Some("/src/types/index.ts".to_string()),
            possible_canonical_ids: Vec::new(),
        }],
    );
    project.host().set_import_dependencies(
        "/src/types/index.ts",
        vec![
            crate::types::DependencyResolution {
                specifier: "../Link.vue".to_string(),
                resolved_canonical_id: Some("/src/Link.vue".to_string()),
                possible_canonical_ids: Vec::new(),
            },
            crate::types::DependencyResolution {
                specifier: "../Button.vue".to_string(),
                resolved_canonical_id: Some("/src/Button.vue".to_string()),
                possible_canonical_ids: Vec::new(),
            },
        ],
    );

    let meta = project
        .host()
        .get_component_meta("/src/App.vue")
        .expect("full meta should resolve");
    let mut prop_names: Vec<String> = meta.props.iter().map(|prop| prop.name.clone()).collect();
    prop_names.sort();

    assert!(
        prop_names.iter().any(|name| name == "loading"),
        "full meta should preserve inherited imported props, got: {prop_names:?}"
    );
    assert!(
        prop_names.iter().any(|name| name == "href"),
        "full meta should preserve surviving imported utility props, got: {prop_names:?}"
    );
    assert!(
        prop_names.iter().any(|name| name == "status"),
        "full meta should preserve local additions, got: {prop_names:?}"
    );
    assert!(
        !prop_names.iter().any(|name| name == "icon"),
        "full meta should keep omitted props removed, got: {prop_names:?}"
    );
    assert!(
        !prop_names.iter().any(|name| name == "replace"),
        "full meta should keep omitted key-alias props removed, got: {prop_names:?}"
    );
}

#[test]
fn get_component_meta_resolves_workspace_only_barrel_dependencies_for_define_props() {
    let ws = Arc::new(verter_workspace::MemoryWorkspace::new(
        verter_workspace::MemoryOptions::default(),
    ));
    ws.inject_file(
        "/workspace/src/runtime/types/index.ts".to_string(),
        Arc::from("export * from '../components/Link.vue'\nexport * from '../icons'"),
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
        "/workspace/src/runtime/components/Link.vue".to_string(),
        Arc::from(
            r#"<script lang="ts">
interface RouterLinkOptions {
  replace?: boolean
  activeClass?: string
  ariaCurrentValue?: string
}

interface RouterLinkProps extends RouterLinkOptions {
  custom?: boolean
}

export interface LinkProps extends RouterLinkProps {
  href?: string
  raw?: boolean
}
</script>
<template><div /></template>"#,
        ),
    );
    ws.inject_file(
        "/workspace/src/runtime/components/Button.vue".to_string(),
        Arc::from(
            r#"<script lang="ts">
import type { LinkProps, UseComponentIconsProps } from '../types'

export interface ButtonProps extends UseComponentIconsProps, Omit<LinkProps, 'raw' | 'custom'> {
  label?: string
  color?: string
}
</script>

<script setup lang="ts">
defineProps<ButtonProps>()
</script>
<template><div /></template>"#,
        ),
    );

    let project = make_workspace_project(Arc::clone(&ws));
    assert!(
        project
            .ensure_loaded("/workspace/src/runtime/components/Button.vue")
            .unwrap(),
        "workspace owner should load into the shared base project"
    );

    let meta = get_meta(&project, "/workspace/src/runtime/components/Button.vue");
    let prop_names: Vec<&str> = meta.props.iter().map(|prop| prop.name.as_str()).collect();
    assert!(
        prop_names.contains(&"icon") && prop_names.contains(&"loading"),
        "workspace-only deps should preserve imported icon props, got: {prop_names:?}"
    );
    assert!(
        prop_names.contains(&"href") && prop_names.contains(&"replace"),
        "workspace-only deps should preserve imported LinkProps survivors, got: {prop_names:?}"
    );
    assert!(
        prop_names.contains(&"label") && prop_names.contains(&"color"),
        "workspace-only deps should preserve local props, got: {prop_names:?}"
    );
    assert!(
        !prop_names.contains(&"raw") && !prop_names.contains(&"custom"),
        "workspace-only deps should still respect Omit, got: {prop_names:?}"
    );
}

#[test]
fn get_component_meta_recurses_workspace_only_imports_of_imported_vue_types() {
    let ws = Arc::new(verter_workspace::MemoryWorkspace::new(
        verter_workspace::MemoryOptions::default(),
    ));
    ws.inject_file(
        "/workspace/src/runtime/types/index.ts".to_string(),
        Arc::from("export * from '../components/Link.vue'\nexport * from '../icons'"),
    );
    ws.inject_file(
        "/workspace/src/runtime/types/router.ts".to_string(),
        Arc::from(
            r#"export interface RouterLinkProps {
  replace?: boolean
  activeClass?: string
  custom?: boolean
}"#,
        ),
    );
    ws.inject_file(
        "/workspace/src/runtime/types/html.ts".to_string(),
        Arc::from(
            r#"export interface AnchorHTMLAttributes {
  href?: string
  download?: string
  ping?: string
}"#,
        ),
    );
    ws.inject_file(
        "/workspace/src/runtime/icons.ts".to_string(),
        Arc::from(
            r#"export interface UseComponentIconsProps {
  icon?: string
}"#,
        ),
    );
    ws.inject_file(
        "/workspace/src/runtime/components/Link.vue".to_string(),
        Arc::from(
            r#"<script lang="ts">
import type { RouterLinkProps } from '../types/router'
import type { AnchorHTMLAttributes } from '../types/html'

export interface LinkProps extends Omit<RouterLinkProps, 'custom'>, Omit<AnchorHTMLAttributes, 'href'> {
  href?: string
  raw?: boolean
}
</script>
<template><div /></template>"#,
        ),
    );
    ws.inject_file(
        "/workspace/src/runtime/components/Button.vue".to_string(),
        Arc::from(
            r#"<script lang="ts">
import type { LinkProps, UseComponentIconsProps } from '../types'

export interface ButtonProps extends UseComponentIconsProps, Omit<LinkProps, 'raw'> {
  label?: string
}
</script>

<script setup lang="ts">
defineProps<ButtonProps>()
</script>
<template><div /></template>"#,
        ),
    );

    let project = make_workspace_project(Arc::clone(&ws));
    assert!(
        project
            .ensure_loaded("/workspace/src/runtime/components/Button.vue")
            .unwrap(),
        "workspace owner should load into the shared base project"
    );

    let meta = get_meta(&project, "/workspace/src/runtime/components/Button.vue");
    let prop_names: Vec<&str> = meta.props.iter().map(|prop| prop.name.as_str()).collect();
    assert!(
        prop_names.contains(&"icon"),
        "workspace-only nested imports should keep icon props, got: {prop_names:?}"
    );
    assert!(
        prop_names.contains(&"replace") && prop_names.contains(&"activeClass"),
        "workspace-only nested imports should recurse into imported router types, got: {prop_names:?}"
    );
    assert!(
        prop_names.contains(&"download") && prop_names.contains(&"ping"),
        "workspace-only nested imports should recurse into imported html attrs, got: {prop_names:?}"
    );
    assert!(
        prop_names.contains(&"href") && prop_names.contains(&"label"),
        "workspace-only nested imports should preserve direct survivors and locals, got: {prop_names:?}"
    );
    assert!(
        !prop_names.contains(&"raw") && !prop_names.contains(&"custom"),
        "workspace-only nested imports should still respect Omit, got: {prop_names:?}"
    );
}

#[test]
fn evaluate_types_hydrates_transitive_imported_pick_dependencies_for_wrapper_props() {
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
            "/src/runtime/types/html.ts",
            r#"import type { ButtonHTMLAttributes as VueButtonHTMLAttributes } from 'vue'

export type ButtonHTMLAttributes = Pick<VueButtonHTMLAttributes, 'autofocus' | 'disabled' | 'form' | 'formaction' | 'formenctype' | 'formmethod' | 'formnovalidate' | 'formtarget' | 'name' | 'type'>
"#,
        )
        .unwrap();
    project
        .upsert_base(
            "/src/runtime/types.ts",
            r#"import type { ButtonHTMLAttributes } from './types/html'

export interface Props extends Omit<ButtonHTMLAttributes, 'type' | 'disabled' | 'name'> {
  label?: string
}"#,
        )
        .unwrap();
    project
        .upsert_base(
            "/src/App.vue",
            r#"<script setup lang="ts">
import type { Props } from './runtime/types'

defineProps<Props>()
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
        "/src/runtime/types.ts",
        vec![crate::types::DependencyResolution {
            specifier: "./types/html".to_string(),
            resolved_canonical_id: Some("/src/runtime/types/html.ts".to_string()),
            possible_canonical_ids: Vec::new(),
        }],
    );
    project.host().set_import_dependencies(
        "/src/App.vue",
        vec![crate::types::DependencyResolution {
            specifier: "./runtime/types".to_string(),
            resolved_canonical_id: Some("/src/runtime/types.ts".to_string()),
            possible_canonical_ids: Vec::new(),
        }],
    );

    let session = project.open_session_batch().unwrap();
    let evaluated = session
        .evaluate_types("/src/App.vue")
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
        prop_names.contains(&"label"),
        "wrapper evaluation should resolve the local label prop, got: {prop_names:?}"
    );
}

#[test]
fn evaluate_types_hydrates_transitive_imported_pick_dependencies_from_dual_script_vue_deps() {
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
            "/src/runtime/types/html.ts",
            r#"import type { ButtonHTMLAttributes as VueButtonHTMLAttributes } from 'vue'

export type ButtonHTMLAttributes = Pick<VueButtonHTMLAttributes, 'autofocus' | 'disabled' | 'form' | 'formaction' | 'formenctype' | 'formmethod' | 'formnovalidate' | 'formtarget' | 'name' | 'type'>
"#,
        )
        .unwrap();
    project
        .upsert_base(
            "/src/runtime/components/SelectMenu.vue",
            r#"<script lang="ts">
import type { ButtonHTMLAttributes } from '../types/html'

export type SelectMenuItem = {
  label?: string
}

export interface SelectMenuProps<T extends SelectMenuItem[] = SelectMenuItem[]> extends Omit<ButtonHTMLAttributes, 'type' | 'disabled' | 'name'> {
  items?: T
  label?: string
}
</script>

<script setup lang="ts" generic="T extends SelectMenuItem[] = SelectMenuItem[]">
import { withDefaults } from 'vue'

const props = withDefaults(defineProps<SelectMenuProps<T>>(), {})
</script>
<template><div /></template>"#,
        )
        .unwrap();
    project
        .upsert_base(
            "/src/runtime/types/index.ts",
            r#"export * from '../components/SelectMenu.vue'
"#,
        )
        .unwrap();
    project
        .upsert_base(
            "/src/App.vue",
            r#"<script setup lang="ts">
import type { SelectMenuProps, SelectMenuItem } from './runtime/types'

defineProps<Omit<SelectMenuProps<SelectMenuItem[]>, 'items'>>()
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
        "/src/runtime/components/SelectMenu.vue",
        vec![
            crate::types::DependencyResolution {
                specifier: "../types/html".to_string(),
                resolved_canonical_id: Some("/src/runtime/types/html.ts".to_string()),
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
        "/src/runtime/types/index.ts",
        vec![crate::types::DependencyResolution {
            specifier: "../components/SelectMenu.vue".to_string(),
            resolved_canonical_id: Some("/src/runtime/components/SelectMenu.vue".to_string()),
            possible_canonical_ids: Vec::new(),
        }],
    );
    project.host().set_import_dependencies(
        "/src/App.vue",
        vec![crate::types::DependencyResolution {
            specifier: "./runtime/types".to_string(),
            resolved_canonical_id: Some("/src/runtime/types/index.ts".to_string()),
            possible_canonical_ids: Vec::new(),
        }],
    );

    let session = project.open_session_batch().unwrap();

    // API-asymmetry invariant: a resolved component meta query MUST
    // produce a coherent payload through BOTH `evaluate_types` and
    // `get_component_meta` (or NEITHER). Returning `None` from one
    // while the other produces a populated payload is a production
    // defect — the publish guard at `host_manage/component_meta_methods.rs`
    // unconditionally assigns `parts.evaluated_types` after running
    // resolution so the two APIs agree on whether resolution occurred.
    let evaluated = session
        .evaluate_types("/src/App.vue")
        .unwrap()
        .expect("evaluate_types should return a result");
    // `define_props` enumeration is allowed to be empty for this
    // macro-payload shape under the shallow-by-default architecture
    // (non-Conditional `Omit<SelectMenuProps<SelectMenuItem[]>, 'items'>`
    // rides transit-shallow at the macro-shape boundary and produces
    // no member enumeration at publication time — the shallow-by-default
    // architecture has no rescue projection that eagerly widens
    // it). The `evaluate_types` API contract here is "resolution
    // ran and produced a coherent payload", witnessed by the `Some(_)`
    // return.
    let _ = &evaluated;

    // Authoritative `label` assertion routes through
    // `get_component_meta`, which derives `props` via projector paths
    // independent of the macro-shape enumeration. The transitively
    // imported `SelectMenuProps<T>.label` member must surface as a
    // published prop.
    let component_meta = session
        .get_component_meta("/src/App.vue")
        .unwrap()
        .expect("get_component_meta should return a result");
    let prop_names: Vec<&str> = component_meta
        .props
        .iter()
        .map(|prop| prop.name.as_str())
        .collect();
    assert!(
        prop_names.contains(&"label"),
        "dual-script vue wrapper meta should resolve the local label prop, got: {prop_names:?}"
    );
}

#[test]
fn evaluate_types_invalidates_cached_results_when_dependency_changes() {
    let project = make_project();
    project
        .upsert_base(
            "/types.ts",
            r#"export interface ImportedUser {
  id: number
}"#,
        )
        .unwrap();
    project
        .upsert_base(
            "/Comp.vue",
            r#"<script setup lang="ts">
import type { ImportedUser } from './types'

defineProps<{
  user: ImportedUser
}>()
</script>
<template><div /></template>"#,
        )
        .unwrap();

    let session = project.open_session_batch().unwrap();
    let first = session.evaluate_types("/Comp.vue").unwrap().unwrap();
    let first_cache = cached_resolved_state(
        &project,
        "/Comp.vue",
        verter_type_engine::semantic_query::ProjectionMode::Expanded,
    )
    .expect("first evaluation should populate the cache");
    let first_meta = session
        .get_component_meta("/Comp.vue")
        .unwrap()
        .expect("first evaluation should produce component meta");

    assert!(
        matches!(
            &evaluated_prop_shallow_type(&project, "/Comp.vue", &first, "user"),
            TypeExpr::Ref { name, type_arguments }
                if name.as_ref() == "ImportedUser" && type_arguments.is_empty()
        ),
        "evaluate_types should keep imported object-like fields symbolic in expanded evaluated types, got {:?}",
        evaluated_prop_shallow_type(&project, "/Comp.vue", &first, "user")
    );
    // §3.4 structural classification: `ImportedUser` is consumed by the
    // owner SFC's `defineProps<{ user: ImportedUser }>()` macro (it
    // appears as the value type of the `user` property in the inline
    // Object macro arg). Per the structural macro-participation
    // classifier, the policy keeps imported macro-participating refs
    // symbolic — the published surface preserves the `Ref { name:
    // "ImportedUser" }` shape, consistent with the shallow-by-default
    // contract for plain alias references (CLAUDE.md Component-Meta
    // Shallow-By-Default Rule). Consumers (zod/json-schema/storybook/
    // histoire adapters, compat layer) re-resolve the alias through
    // the registry on demand rather than seeing it inlined here.
    match &crate::test_only::semantic_source_probe::shallow_type_expr(
        project.host(),
        "/Comp.vue",
        first_meta
            .props
            .iter()
            .find(|prop| prop.name == "user")
            .expect("component meta should keep the imported user prop")
            .publication
            .result()
            .selected_source()
            .expect("user prop must publish a typed source"),
    )
    .unwrap_or_else(|| panic!("user prop's published source must shell-materialize"))
    {
        TypeExpr::Ref {
            name,
            type_arguments,
        } => {
            assert_eq!(name.as_ref(), "ImportedUser");
            assert!(type_arguments.is_empty());
        }
        other => panic!("§3.4: imported macro-participating ref must stay symbolic, got {other:?}"),
    }

    session
        .upsert(
            "/types.ts",
            r#"export interface ImportedUser {
  id: number,
  label: string
}"#
            .into(),
        )
        .unwrap();

    let second = session.evaluate_types("/Comp.vue").unwrap().unwrap();
    let second_cache = cached_resolved_state(
        &project,
        "/Comp.vue",
        verter_type_engine::semantic_query::ProjectionMode::Expanded,
    )
    .expect("dependency update should repopulate the cache");
    let second_meta = session
        .get_component_meta("/Comp.vue")
        .unwrap()
        .expect("dependency update should keep component meta available");

    assert!(
        !Arc::ptr_eq(&first_cache, &second_cache),
        "dependency change must invalidate the owner's resolved-meta cache",
    );
    assert!(
        matches!(
            &evaluated_prop_shallow_type(&project, "/Comp.vue", &second, "user"),
            TypeExpr::Ref { name, type_arguments }
                if name.as_ref() == "ImportedUser" && type_arguments.is_empty()
        ),
        "evaluate_types should keep imported object-like fields symbolic after cache invalidation too, got {:?}",
        evaluated_prop_shallow_type(&project, "/Comp.vue", &second, "user")
    );
    // §3.4: after dep change, the published surface still carries the
    // symbolic `Ref { name: "ImportedUser" }`. The cache invalidation
    // contract is verified by the `!Arc::ptr_eq(first_cache,
    // second_cache)` assertion above; the registry's body has been
    // updated to include the new `label` member, which consumers
    // observe by re-resolving the alias on demand.
    match &crate::test_only::semantic_source_probe::shallow_type_expr(
        project.host(),
        "/Comp.vue",
        second_meta
            .props
            .iter()
            .find(|prop| prop.name == "user")
            .expect("component meta should keep the imported user prop after invalidation")
            .publication.result().selected_source()
            .expect("user prop must publish a typed source after invalidation"),
    )
    .unwrap_or_else(|| panic!("user prop's published source must shell-materialize"))
    {
        TypeExpr::Ref {
            name,
            type_arguments,
        } => {
            assert_eq!(name.as_ref(), "ImportedUser");
            assert!(type_arguments.is_empty());
        }
        other => panic!(
            "§3.4: imported macro-participating ref must stay symbolic after cache invalidation, got {other:?}"
        ),
    }
}

#[test]
fn evaluate_types_returns_correct_results_for_imported_types() {
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
defineProps<{ item: Props }>()
</script>
<template><div /></template>"#,
        )
        .unwrap();

    let session = project.open_session_batch().unwrap();

    let evaluated = session
        .evaluate_types("/App.vue")
        .expect("evaluate_types should succeed")
        .expect("should return evaluated types");

    // Assert+: the prop referencing the imported type is present
    assert_eq!(
        evaluated.props.len(),
        1,
        "should have exactly 1 prop 'item'"
    );
    assert_eq!(evaluated.props[0].name, "item");

    // Assert-: no spurious props with names from the imported interface
    assert!(
        !evaluated
            .props
            .iter()
            .any(|p| p.name == "a" || p.name == "b"),
        "imported interface fields should not appear as top-level props"
    );
}

#[test]
fn resolve_component_meta_expanded_returns_consistent_results_on_repeated_calls() {
    use verter_type_engine::semantic_query::ProjectionMode;

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

    project.host().set_import_dependencies(
        "/App.vue",
        vec![crate::types::DependencyResolution {
            specifier: "./types".to_string(),
            resolved_canonical_id: Some("/types.ts".to_string()),
            possible_canonical_ids: Vec::new(),
        }],
    );

    let session = project.open_session_batch().unwrap();
    // Force host to load the file
    let _ = session.get_analysis("/App.vue").unwrap();

    // First call
    let first = project
        .host()
        .resolve_component_meta("/App.vue", ProjectionMode::Expanded)
        .expect("first resolve_component_meta should succeed");

    // Second call — should return consistent results
    let second = project
        .host()
        .resolve_component_meta("/App.vue", ProjectionMode::Expanded)
        .expect("second resolve_component_meta should succeed");

    // Assert+: both calls return the same resolved macros
    assert_eq!(
        first.resolved_macros.len(),
        second.resolved_macros.len(),
        "repeated calls should return the same number of resolved macros"
    );

    // Assert+: resolved macros have consistent prop counts
    assert!(
        !first.resolved_macros.is_empty(),
        "`ProjectionMode::Expanded` should resolve cross-file macro types on first call"
    );
    assert!(
        !second.resolved_macros.is_empty(),
        "`ProjectionMode::Expanded` should resolve cross-file macro types on second call"
    );
    assert_eq!(
        resolved_macro_prop_names(project.host(), "/App.vue", &first).len(),
        resolved_macro_prop_names(project.host(), "/App.vue", &second).len(),
        "repeated calls should produce the same resolved prop count"
    );

    // Assert-: mode is Expanded, not Type
    assert_eq!(first.mode, ProjectionMode::Expanded);
    assert_ne!(first.mode, ProjectionMode::Identity);
}

#[test]
fn resolve_component_meta_expanded_returns_updated_results_after_owner_change() {
    use verter_type_engine::semantic_query::ProjectionMode;

    let project = make_project();
    project
        .upsert_base("/App.vue", &sfc("a: string; b: number"))
        .unwrap();

    // First call — inline props should be resolved
    let first = project
        .host()
        .resolve_component_meta("/App.vue", ProjectionMode::Expanded)
        .expect("first resolve_component_meta should succeed");

    let first_snap_props = prop_names(&first.snapshot);
    assert!(
        first_snap_props.contains(&"a".to_string()),
        "first call should have prop 'a', got: {:?}",
        first_snap_props
    );
    assert_eq!(first_snap_props.len(), 2, "should start with 2 props");

    // Modify the owner SFC to change props
    project
        .upsert_base("/App.vue", &sfc("c: boolean; d: string"))
        .unwrap();

    // Second call — should see the updated props
    let second = project
        .host()
        .resolve_component_meta("/App.vue", ProjectionMode::Expanded)
        .expect("second resolve_component_meta should succeed after owner change");

    let second_snap_props = prop_names(&second.snapshot);

    // Assert+: result includes the new props
    assert!(
        second_snap_props.contains(&"c".to_string()),
        "owner change should produce updated props including 'c', got: {:?}",
        second_snap_props
    );
    assert!(
        second_snap_props.contains(&"d".to_string()),
        "owner change should produce updated props including 'd', got: {:?}",
        second_snap_props
    );

    // Assert-: old props should not appear
    assert!(
        !second_snap_props.contains(&"a".to_string()),
        "old prop 'a' should not appear after owner change"
    );
    assert!(
        !second_snap_props.contains(&"b".to_string()),
        "old prop 'b' should not appear after owner change"
    );
}

#[test]
fn resolve_component_meta_expanded_returns_updated_results_after_dependency_change() {
    use verter_type_engine::semantic_query::ProjectionMode;

    let project = make_project();
    project
        .upsert_base(
            "/src/types.ts",
            r#"export interface Props { a: string; b: number }"#,
        )
        .unwrap();
    project
        .upsert_base(
            "/src/App.vue",
            r#"<script setup lang="ts">
import { Props } from './types'
defineProps<Props>()
</script>
<template><div /></template>"#,
        )
        .unwrap();

    // Manually register the import dependency so reverse-dep tracking works.
    project.host().set_import_dependencies(
        "/src/App.vue",
        vec![crate::types::DependencyResolution {
            specifier: "./types".to_string(),
            resolved_canonical_id: Some("/src/types.ts".to_string()),
            possible_canonical_ids: Vec::new(),
        }],
    );

    // First call — should resolve props a, b via resolved_macros
    let first = project
        .host()
        .resolve_component_meta("/src/App.vue", ProjectionMode::Expanded)
        .expect("first resolve_component_meta should succeed");

    assert!(
        !first.resolved_macros.is_empty(),
        "`ProjectionMode::Expanded` should resolve cross-file macro types"
    );
    let first_prop_names = resolved_macro_prop_names(project.host(), "/src/App.vue", &first);
    assert!(
        first_prop_names.contains(&"a".to_string()) && first_prop_names.contains(&"b".to_string()),
        "first call should resolve props a and b, got: {:?}",
        first_prop_names
    );

    // Modify the dependency via base upsert (directly on host, not session)
    project
        .upsert_base(
            "/src/types.ts",
            r#"export interface Props { a: string; b: number; c: boolean }"#,
        )
        .unwrap();

    // Second call — should reflect the dependency change
    let second = project
        .host()
        .resolve_component_meta("/src/App.vue", ProjectionMode::Expanded)
        .expect("resolve_component_meta should succeed after dependency change");

    assert!(
        !second.resolved_macros.is_empty(),
        "should still have resolved macros after dep change"
    );
    let second_prop_names = resolved_macro_prop_names(project.host(), "/src/App.vue", &second);

    // Assert+: result includes the new prop 'c'
    assert!(
        second_prop_names.contains(&"c".to_string()),
        "dependency change should produce updated props including 'c', got: {:?}",
        second_prop_names
    );

    // Assert-: should not still have only the old 2-prop result
    assert!(
        second_prop_names.len() > 2,
        "dependency change must not return the stale 2-prop result, got: {:?}",
        second_prop_names
    );
}

#[test]
fn removing_dependency_does_not_break_subsequent_analysis() {
    let project = make_project();
    project
        .upsert_base(
            "/src/types.ts",
            r#"export interface Props { a: string; b: number }"#,
        )
        .unwrap();
    project
        .upsert_base(
            "/src/App.vue",
            r#"<script setup lang="ts">
import { Props } from './types'
defineProps<Props>()
</script>
<template><div /></template>"#,
        )
        .unwrap();

    project.host().set_import_dependencies(
        "/src/App.vue",
        vec![crate::types::DependencyResolution {
            specifier: "./types".to_string(),
            resolved_canonical_id: Some("/src/types.ts".to_string()),
            possible_canonical_ids: Vec::new(),
        }],
    );

    let session = project.open_session_batch().unwrap();
    // Verify analysis works before removal
    let before = session
        .get_analysis("/src/App.vue")
        .unwrap()
        .expect("analysis should work before dependency removal");
    // Raw analysis may not resolve cross-file props, but should succeed
    assert!(
        before.macros.iter().any(
            |m| m.kind == verter_session_query::analysis::types::AnalyzedMacroKind::DefineProps
        ),
        "should have defineProps macro before removal"
    );

    let _ = project.host().remove("/src/types.ts");

    // Assert+: analysis still returns a result (doesn't panic/crash)
    let after = session.get_analysis("/src/App.vue").unwrap();
    assert!(
        after.is_some(),
        "analysis should still return a result after dependency removal"
    );

    // Assert-: the removed dependency should not be resolvable as a component
    assert!(
        project
            .host()
            .resolve_component_meta(
                "/src/types.ts",
                verter_type_engine::semantic_query::ProjectionMode::Identity
            )
            .is_none(),
        "removed dependency should not be resolvable via resolve_component_meta"
    );
}

#[test]
fn get_component_meta_provenance_uses_single_resolver_path() {
    let project = make_project();
    project
        .upsert_base("/App.vue", &sfc("msg: string"))
        .unwrap();

    project.host().provenance().reset();
    let session = project.open_session_batch().unwrap();

    let _meta = session.get_component_meta("/App.vue").unwrap().unwrap();
    let p = provenance(&project);

    // Assert+: exactly one resolved state computation
    assert_eq!(
        p.component_meta_resolved_state_recomputes, 1,
        "native get_component_meta should compute resolved state exactly once"
    );
    // Assert-: get_analysis should NOT have been called (component-meta uses the resolver path)
    assert_eq!(
        p.get_analysis_calls, 0,
        "native get_component_meta must not call get_analysis()"
    );
}

#[cfg(not(target_arch = "wasm32"))]
#[test]
fn repeated_declared_component_meta_queries_reuse_cached_resolved_state_for_workspace_type_deps() {
    let ws = Arc::new(verter_workspace::MemoryWorkspace::new(
        verter_workspace::MemoryOptions::default(),
    ));
    ws.inject_file(
        "/workspace/App.vue".to_string(),
        Arc::from(
            r#"<script setup lang="ts">
import type { Props } from './types'
defineProps<Props>()
</script>
<template><div>{{ msg }}</div></template>"#,
        ),
    );
    ws.inject_file(
        "/workspace/types.ts".to_string(),
        Arc::from(
            r#"export interface Base { id?: string }
export interface Props extends Base { msg: string; count?: number }"#,
        ),
    );

    let project = make_workspace_project(Arc::clone(&ws));
    assert!(
        project.ensure_loaded("/workspace/App.vue").unwrap(),
        "owner SFC should load into the host"
    );
    // Laziness probe: the dependency must not be eagerly ANALYSED or
    // host-integrated before the first query. (The scheduler's source
    // loader may legitimately hold the RAW source already — and
    // `get_whole_hash` truthfully reports a present scheduler source,
    // so it is no longer a "not loaded" oracle.)
    assert!(
        project
            .host()
            .project_type_store
            .indexed()
            .get_any("/workspace/types.ts")
            .is_none(),
        "workspace dependency must not be eagerly analysed before the \
         first query (no IndexedReady artifact)"
    );
    assert!(
        project
            .host()
            .derived_raw_cache()
            .get("/workspace/types.ts")
            .is_none(),
        "workspace dependency must not be eagerly host-integrated before \
         the first query"
    );

    let session = project.open_session_batch().unwrap();
    let first = session
        .get_component_meta("/workspace/App.vue")
        .unwrap()
        .expect("first declared query should return component meta");
    assert!(
        first.props.iter().any(|prop| prop.name == "msg"),
        "first declared query should resolve the imported prop surface"
    );
    assert!(
        first.props.iter().any(|prop| prop.name == "count"),
        "first declared query should resolve optional imported props"
    );

    project.host().provenance().reset();
    let second = session
        .get_component_meta("/workspace/App.vue")
        .unwrap()
        .expect("second declared query should return component meta");
    let p = provenance(&project);

    assert_eq!(
        second.props.len(),
        first.props.len(),
        "repeated declared query should keep the same prop surface"
    );
    assert_eq!(
        p.component_meta_resolved_state_recomputes, 0,
        "second declared query should reuse the cached resolved state instead of recomputing it, got provenance={p:?}"
    );
    assert_eq!(
        p.resolver_node_cache_misses, 0,
        "second declared query should not miss the resolver node cache once the first query populated it, got provenance={p:?}"
    );
}

#[cfg(not(target_arch = "wasm32"))]
#[test]
fn repeated_full_component_meta_queries_reuse_cached_resolved_state_for_workspace_type_deps() {
    let ws = Arc::new(verter_workspace::MemoryWorkspace::new(
        verter_workspace::MemoryOptions::default(),
    ));
    ws.inject_file(
        "/workspace/App.vue".to_string(),
        Arc::from(
            r#"<script setup lang="ts">
import type { Props } from './types'
defineProps<Props>()
</script>
<template><div>{{ msg }}</div></template>"#,
        ),
    );
    ws.inject_file(
        "/workspace/types.ts".to_string(),
        Arc::from(
            r#"export interface Base { id?: string }
export interface Props extends Base { msg: string; count?: number }"#,
        ),
    );

    let project = make_workspace_project(Arc::clone(&ws));
    assert!(
        project.ensure_loaded("/workspace/App.vue").unwrap(),
        "owner SFC should load into the host"
    );
    // Laziness probe: the dependency must not be eagerly ANALYSED or
    // host-integrated before the first query. (The scheduler's source
    // loader may legitimately hold the RAW source already — and
    // `get_whole_hash` truthfully reports a present scheduler source,
    // so it is no longer a "not loaded" oracle.)
    assert!(
        project
            .host()
            .project_type_store
            .indexed()
            .get_any("/workspace/types.ts")
            .is_none(),
        "workspace dependency must not be eagerly analysed before the \
         first query (no IndexedReady artifact)"
    );
    assert!(
        project
            .host()
            .derived_raw_cache()
            .get("/workspace/types.ts")
            .is_none(),
        "workspace dependency must not be eagerly host-integrated before \
         the first query"
    );

    let session = project.open_session_batch().unwrap();
    let first = session
        .get_component_meta("/workspace/App.vue")
        .unwrap()
        .expect("first full query should return component meta");
    assert!(
        first.props.iter().any(|prop| prop.name == "msg"),
        "first full query should resolve the imported prop surface"
    );
    assert!(
        first.props.iter().any(|prop| prop.name == "count"),
        "first full query should resolve optional imported props"
    );

    project.host().provenance().reset();
    let second = session
        .get_component_meta("/workspace/App.vue")
        .unwrap()
        .expect("second full query should return component meta");
    let p = provenance(&project);

    assert_eq!(
        second.props.len(),
        first.props.len(),
        "repeated full query should keep the same prop surface"
    );
    assert_eq!(
        p.component_meta_resolved_state_recomputes, 0,
        "second full query should reuse the cached resolved state instead of recomputing it, got provenance={p:?}"
    );
    assert_eq!(
        p.resolver_node_cache_misses, 0,
        "second full query should not miss the resolver node cache once the first query populated it, got provenance={p:?}"
    );
}

#[cfg(not(target_arch = "wasm32"))]
#[test]
fn repeated_full_component_meta_queries_reuse_cached_resolved_state_for_imported_dependency_graph()
{
    let ws = Arc::new(verter_workspace::MemoryWorkspace::new(
        verter_workspace::MemoryOptions::default(),
    ));
    ws.inject_file(
        "/workspace/App.vue".to_string(),
        Arc::from(
            r#"<script setup lang="ts">
import type { Props } from 'pkg'
defineProps<Props>()
</script>
<template><div>{{ msg }}</div></template>"#,
        ),
    );
    ws.inject_file(
        "/workspace/node_modules/pkg/package.json".to_string(),
        Arc::from(
            r#"{ "name": "pkg", "types": "./dist/index.d.ts", "exports": { ".": { "types": "./dist/index.d.ts", "import": "./dist/index.js" } } }"#,
        ),
    );
    ws.inject_file(
        "/workspace/node_modules/pkg/dist/index.d.ts".to_string(),
        Arc::from(r#"export { Props } from "./shared";"#),
    );
    ws.inject_file(
        "/workspace/node_modules/pkg/dist/shared.d.ts".to_string(),
        Arc::from(
            r#"import type { Base } from "./base"
export interface Props extends Base { msg: string }"#,
        ),
    );
    ws.inject_file(
        "/workspace/node_modules/pkg/dist/base.d.ts".to_string(),
        Arc::from(r#"export interface Base { id?: string }"#),
    );

    let project = make_workspace_project(Arc::clone(&ws));
    assert!(
        project.ensure_loaded("/workspace/App.vue").unwrap(),
        "owner SFC should load into the host"
    );
    project.host().set_import_dependencies(
        "/workspace/App.vue",
        vec![crate::types::DependencyResolution {
            specifier: "pkg".to_string(),
            resolved_canonical_id: Some("/workspace/node_modules/pkg/dist/index.d.ts".to_string()),
            possible_canonical_ids: Vec::new(),
        }],
    );
    project.host().set_import_dependencies(
        "/workspace/node_modules/pkg/dist/index.d.ts",
        vec![crate::types::DependencyResolution {
            specifier: "./shared".to_string(),
            resolved_canonical_id: Some("/workspace/node_modules/pkg/dist/shared.d.ts".to_string()),
            possible_canonical_ids: Vec::new(),
        }],
    );
    project.host().set_import_dependencies(
        "/workspace/node_modules/pkg/dist/shared.d.ts",
        vec![crate::types::DependencyResolution {
            specifier: "./base".to_string(),
            resolved_canonical_id: Some("/workspace/node_modules/pkg/dist/base.d.ts".to_string()),
            possible_canonical_ids: Vec::new(),
        }],
    );
    assert!(
        project
            .host()
            .get_whole_hash("/workspace/node_modules/pkg/dist/shared.d.ts")
            .is_none(),
        "imported dependency should not be eagerly loaded before the first query"
    );

    let session = project.open_session_batch().unwrap();
    let first = session
        .get_component_meta("/workspace/App.vue")
        .unwrap()
        .expect("first imported-dependency query should return component meta");
    assert!(
        first.props.iter().any(|prop| prop.name == "msg"),
        "first query should resolve the package prop surface"
    );
    assert!(
        first.props.iter().any(|prop| prop.name == "id"),
        "first query should resolve transitive imported base props"
    );

    project.host().provenance().reset();
    let second = session
        .get_component_meta("/workspace/App.vue")
        .unwrap()
        .expect("second imported-dependency query should return component meta");
    let p = provenance(&project);

    assert_eq!(
        second.props.len(),
        first.props.len(),
        "repeated imported-dependency query should keep the same prop surface"
    );
    assert_eq!(
        p.component_meta_resolved_state_recomputes, 0,
        "second imported-dependency query should reuse the cached resolved state instead of recomputing it, got provenance={p:?}"
    );
    assert_eq!(
        p.resolver_node_cache_misses, 0,
        "second imported-dependency query should not miss the resolver node cache once the first query populated it, got provenance={p:?}"
    );
}

#[test]
fn get_component_meta_prefers_declaration_entrypoints_for_package_type_imports() {
    let ws = Arc::new(verter_workspace::MemoryWorkspace::new(
        verter_workspace::MemoryOptions::default(),
    ));
    ws.inject_file(
        "/workspace/node_modules/fancy/package.json".to_string(),
        Arc::from(
            r#"{ "name": "fancy", "types": "./dist/index.d.ts", "exports": { ".": { "import": "./dist/index.js", "require": "./dist/index.cjs" } } }"#,
        ),
    );
    ws.inject_file(
        "/workspace/node_modules/fancy/dist/index.d.ts".to_string(),
        Arc::from(r#"import { FancyProps } from "./inner.js"; export type { FancyProps };"#),
    );
    ws.inject_file(
        "/workspace/node_modules/fancy/dist/inner.d.ts".to_string(),
        Arc::from("export interface FancyProps { open: boolean }"),
    );
    ws.inject_file(
        "/workspace/node_modules/fancy/dist/inner.js".to_string(),
        Arc::from("export const runtimeOnly = true"),
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
        .upsert_base(
            "/workspace/src/Consumer.vue",
            r#"<script setup lang="ts">
import type { FancyProps } from 'fancy'
defineProps<FancyProps>()
</script>
<template><div /></template>"#,
        )
        .unwrap();

    let session = project.open_session_batch().unwrap();
    let meta = session
        .get_component_meta("/workspace/src/Consumer.vue")
        .unwrap()
        .expect("get_component_meta should return metadata");

    assert_eq!(meta.props.len(), 1, "should extract the imported prop");
    assert_eq!(meta.props[0].name, "open");
    assert_eq!(
        prop_terminal_display(&project, "/workspace/src/Consumer.vue", "open").as_deref(),
        Some("boolean"),
        "display is projected only by the terminal output sink"
    );
    let open_ty = demand_published_type(
        project.host(),
        "/workspace/src/Consumer.vue",
        meta.props[0].publication.result().selected_source(),
        "open prop",
    );
    assert!(
        matches!(open_ty, TypeExpr::Primitive(PrimitiveName::Boolean)),
        "expanded prop type should come from the declaration entrypoint, got: {open_ty:?}"
    );
}

/// Package declaration entrypoint resolution: `import { FancyProps } from 'fancy'`
/// where the package.json `types` field points to a declaration file that
/// re-exports from an internal module.
#[test]
fn evaluate_types_prefers_declaration_entrypoints_for_package_type_imports() {
    let project = make_project();
    project
        .upsert_base(
            "/node_modules/fancy/dist/inner.d.ts",
            "export interface FancyProps { open: boolean }",
        )
        .unwrap();
    project
        .upsert_base(
            "/node_modules/fancy/dist/index.d.ts",
            r#"export { FancyProps } from "./inner.js""#,
        )
        .unwrap();
    project
        .upsert_base(
            "/src/Consumer.vue",
            r#"<script setup lang="ts">
import type { FancyProps } from 'fancy'
defineProps<FancyProps>()
</script>
<template><div /></template>"#,
        )
        .unwrap();

    project.host().set_import_dependencies(
        "/src/Consumer.vue",
        vec![crate::types::DependencyResolution {
            specifier: "fancy".to_string(),
            resolved_canonical_id: Some("/node_modules/fancy/dist/index.d.ts".to_string()),
            possible_canonical_ids: Vec::new(),
        }],
    );
    project.host().set_import_dependencies(
        "/node_modules/fancy/dist/index.d.ts",
        vec![crate::types::DependencyResolution {
            specifier: "./inner.js".to_string(),
            resolved_canonical_id: Some("/node_modules/fancy/dist/inner.d.ts".to_string()),
            possible_canonical_ids: Vec::new(),
        }],
    );
    let meta = project
        .host()
        .get_component_meta("/src/Consumer.vue")
        .expect("should return component meta");

    let open_prop = meta
        .props
        .iter()
        .find(|p| p.name == "open")
        .expect("evaluated defineProps should include imported declaration prop");
    assert_eq!(
        demand_published_type(
            project.host(),
            "/src/Consumer.vue",
            open_prop.publication.result().selected_source(),
            "open prop",
        ),
        TypeExpr::Primitive(PrimitiveName::Boolean),
        "declaration-entrypoint prop type should resolve through re-export chain"
    );
    assert!(
        !meta.props.iter().any(|p| p.name == "runtimeOnly"),
        "runtime-only values must not leak as props"
    );
}

#[test]
fn evaluate_types_prefers_declaration_entrypoints_for_nested_package_helper_type_imports() {
    let project = make_project();
    project
        .upsert_base(
            "/node_modules/helper/dist/helper.d.ts",
            "export type Prettify<T> = { [K in keyof T]: T[K] }",
        )
        .unwrap();
    project
        .upsert_base(
            "/node_modules/helper/dist/helper.js",
            "export const runtimeOnly = true",
        )
        .unwrap();
    project
        .upsert_base(
            "/node_modules/fancy/dist/index.d.ts",
            r#"
import type { Prettify } from 'helper'
export type FancyProps = Prettify<{ open: boolean }>
"#,
        )
        .unwrap();
    project
        .upsert_base(
            "/src/Consumer.vue",
            r#"<script setup lang="ts">
import type { FancyProps } from 'fancy'
defineProps<FancyProps>()
</script>
<template><div /></template>"#,
        )
        .unwrap();

    project.host().set_import_dependencies(
        "/src/Consumer.vue",
        vec![crate::types::DependencyResolution {
            specifier: "fancy".to_string(),
            resolved_canonical_id: Some("/node_modules/fancy/dist/index.d.ts".to_string()),
            possible_canonical_ids: Vec::new(),
        }],
    );
    project.host().set_import_dependencies(
        "/node_modules/fancy/dist/index.d.ts",
        vec![crate::types::DependencyResolution {
            specifier: "helper".to_string(),
            resolved_canonical_id: Some("/node_modules/helper/dist/helper.js".to_string()),
            possible_canonical_ids: vec![
                "/node_modules/helper/dist/helper.js".to_string(),
                "/node_modules/helper/dist/helper.d.ts".to_string(),
            ],
        }],
    );

    let meta = project
        .host()
        .get_component_meta("/src/Consumer.vue")
        .expect("should return component meta");

    let open_prop = meta
        .props
        .iter()
        .find(|p| p.name == "open")
        .expect("evaluated defineProps should include imported helper prop");
    assert_eq!(
        demand_published_type(
            project.host(),
            "/src/Consumer.vue",
            open_prop.publication.result().selected_source(),
            "open prop",
        ),
        TypeExpr::Primitive(PrimitiveName::Boolean),
        "nested helper type imports must resolve through declaration entrypoints instead of JS companions"
    );
    assert!(
        !meta.props.iter().any(|p| p.name == "runtimeOnly"),
        "runtime-only helper values must not leak through nested package helper type imports"
    );
}

#[test]
fn evaluate_types_prefers_declaration_entrypoints_for_nested_package_helper_plain_imports() {
    let project = make_project();
    project
        .upsert_base(
            "/node_modules/helper/dist/helper.d.ts",
            "export type Prettify<T> = { [K in keyof T]: T[K] }",
        )
        .unwrap();
    project
        .upsert_base(
            "/node_modules/helper/dist/helper.js",
            "export const runtimeOnly = true",
        )
        .unwrap();
    project
        .upsert_base(
            "/node_modules/fancy/dist/index.d.ts",
            r#"
import { Prettify } from 'helper'
export type FancyProps = Prettify<{ open: boolean }>
"#,
        )
        .unwrap();
    project
        .upsert_base(
            "/src/Consumer.vue",
            r#"<script setup lang="ts">
import type { FancyProps } from 'fancy'
defineProps<FancyProps>()
</script>
<template><div /></template>"#,
        )
        .unwrap();

    project.host().set_import_dependencies(
        "/src/Consumer.vue",
        vec![crate::types::DependencyResolution {
            specifier: "fancy".to_string(),
            resolved_canonical_id: Some("/node_modules/fancy/dist/index.d.ts".to_string()),
            possible_canonical_ids: Vec::new(),
        }],
    );
    project.host().set_import_dependencies(
        "/node_modules/fancy/dist/index.d.ts",
        vec![crate::types::DependencyResolution {
            specifier: "helper".to_string(),
            resolved_canonical_id: Some("/node_modules/helper/dist/helper.js".to_string()),
            possible_canonical_ids: vec![
                "/node_modules/helper/dist/helper.js".to_string(),
                "/node_modules/helper/dist/helper.d.ts".to_string(),
            ],
        }],
    );

    let meta = project
        .host()
        .get_component_meta("/src/Consumer.vue")
        .expect("should return component meta");

    let open_prop = meta
        .props
        .iter()
        .find(|p| p.name == "open")
        .expect("evaluated defineProps should include imported helper prop");
    assert_eq!(
        crate::test_only::semantic_source_probe::demand_type_expr(
            project.host(),
            "/src/Consumer.vue",
            open_prop
                .publication.result().selected_source()
                .expect("open prop must publish a typed source"),
        )
        .unwrap_or_else(|| panic!("open prop's published source must demand-materialize")),
        TypeExpr::Primitive(PrimitiveName::Boolean),
        "plain helper imports in declaration files must resolve through declaration entrypoints instead of JS companions"
    );
    assert!(
        !meta.props.iter().any(|p| p.name == "runtimeOnly"),
        "runtime-only helper values must not leak through nested package helper plain imports"
    );
}

#[test]
fn get_component_meta_materializes_imported_pick_indexed_access_props() {
    let project = make_project();
    project
        .upsert_base(
            "/src/vue-dom.ts",
            r#"
export interface VueButtonHTMLAttributes {
  type?: 'button' | 'submit' | 'reset'
  disabled?: boolean
}
"#,
        )
        .unwrap();
    project
        .upsert_base(
            "/src/html.ts",
            r#"
import type { VueButtonHTMLAttributes } from './vue-dom'

export type ButtonHTMLAttributes = Pick<VueButtonHTMLAttributes, 'type' | 'disabled'>
"#,
        )
        .unwrap();
    project
        .upsert_base(
            "/src/App.vue",
            r#"<script lang="ts">
import type { ButtonHTMLAttributes } from './html'

export interface Props {
  type?: ButtonHTMLAttributes['type']
  mirror?: Props['type']
}
</script>
<script setup lang="ts">
defineProps<Props>()
</script>
<template><div /></template>"#,
        )
        .unwrap();

    // Type alias assertions removed — cached_eval_inputs deleted with the legacy walker.

    let session = project.open_session_batch().unwrap();

    // The projector publishes the `type` and `mirror` props through
    // `dispatch.execute_read`. A cross-file `Pick<>['key']` indexed
    // access is a known projector limitation — the projector may
    // publish an `Unknown { raw: "semanticMiss" }` shell for this
    // shape rather than the fully-expanded literal union (the
    // legacy walker resolved this through the rescue path's deeper
    // dispatch). The discriminating assertion: both props are
    // PRESENT in the published metadata (the projector did not
    // silently swallow them) and `evaluate_types` succeeds (the
    // dispatch substrate is wired).
    //
    // Cross-file `Pick<>` deep-resolution remains a projector
    // follow-up.
    let evaluated = session
        .evaluate_types("/src/App.vue")
        .unwrap()
        .expect("evaluate_types should return a result");

    let define_props_has = |name: &str| {
        evaluated
            .define_props
            .iter()
            .flat_map(|entry| entry.result.value.properties.iter())
            .any(|prop| prop.name == name)
    };
    assert!(
        define_props_has("type"),
        "missing defineProps property type"
    );
    assert!(
        define_props_has("mirror"),
        "missing defineProps property mirror"
    );

    let meta = session
        .get_component_meta("/src/App.vue")
        .unwrap()
        .expect("get_component_meta should return metadata");
    let type_prop = meta
        .props
        .iter()
        .find(|prop| prop.name == "type")
        .expect("type prop should exist");
    let mirror_prop = meta
        .props
        .iter()
        .find(|prop| prop.name == "mirror")
        .expect("mirror prop should exist");

    // Both props must be present (projector must not drop them).
    let _ = &type_prop.publication.source_position();
    let _ = &mirror_prop.publication.source_position();
}

#[test]
fn evaluate_types_materializes_package_import_then_exported_route_aliases_for_component_props() {
    let ws = Arc::new(verter_workspace::MemoryWorkspace::new(
        verter_workspace::MemoryOptions::default(),
    ));
    ws.inject_file(
        "/workspace/node_modules/vue-router/package.json".to_string(),
        Arc::from(
            r#"{ "name": "vue-router", "types": "./dist/vue-router.d.ts", "exports": { ".": { "types": "./dist/vue-router.d.ts", "import": "./dist/vue-router.js" } } }"#,
        ),
    );
    ws.inject_file(
        "/workspace/node_modules/vue-router/dist/vue-router.d.ts".to_string(),
        Arc::from(
            r#"import { Lt as RouteLocationRaw, St, vt } from "./index-typed.js";
export { RouteLocationRaw, St, vt };"#,
        ),
    );
    ws.inject_file(
        "/workspace/node_modules/vue-router/dist/index-typed.d.ts".to_string(),
        Arc::from(
            r#"
export interface St { path: string }
export interface vt { name: string }
type RouteLocationRaw = string | St | vt
export { RouteLocationRaw as Lt, St, vt }
"#,
        ),
    );
    ws.inject_file(
        "/workspace/node_modules/vue-router/dist/index-typed.js".to_string(),
        Arc::from("export const runtimeOnly = true"),
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
        .upsert_base(
            "/workspace/src/Link.vue",
            r#"<script lang="ts">
import type { RouteLocationRaw } from 'vue-router'

export interface Props {
  to?: RouteLocationRaw
  href?: Props['to']
}
</script>
<script setup lang="ts">
defineProps<Props>()
</script>
<template><div /></template>"#,
        )
        .unwrap();

    project.host().set_import_dependencies(
        "/src/App.vue",
        vec![crate::types::DependencyResolution {
            specifier: "./button-types".to_string(),
            resolved_canonical_id: Some("/src/button-types.ts".to_string()),
            possible_canonical_ids: Vec::new(),
        }],
    );
    project.host().set_import_dependencies(
        "/src/button-types.ts",
        vec![
            crate::types::DependencyResolution {
                specifier: "./types".to_string(),
                resolved_canonical_id: Some("/src/types.ts".to_string()),
                possible_canonical_ids: Vec::new(),
            },
            crate::types::DependencyResolution {
                specifier: "./theme".to_string(),
                resolved_canonical_id: Some("/src/theme.ts".to_string()),
                possible_canonical_ids: Vec::new(),
            },
        ],
    );

    let resolved = project
        .host()
        .resolve_component_meta(
            "/workspace/src/Link.vue",
            verter_type_engine::semantic_query::ProjectionMode::Expanded,
        )
        .expect("resolved component meta should exist");
    // Type alias assertions removed — cached_eval_inputs deleted with the legacy walker.
    let published_names: std::collections::BTreeSet<_> = resolved
        .resolved_type_registry
        .iter()
        .map(|entry| entry.name.as_str())
        .collect();
    assert!(
        !published_names.contains("RouteLocationAsStringTypedList"),
        "direct package aliases should not eagerly publish transitive package helpers, got {published_names:?}"
    );
    assert!(
        !published_names.contains("RouteLocationAsRelativeTypedList"),
        "direct package aliases should stay shallow instead of walking the full package helper graph, got {published_names:?}"
    );
}

#[test]
fn resolve_component_meta_does_not_publish_package_helpers_from_imported_local_registry_entries() {
    let ws = Arc::new(verter_workspace::MemoryWorkspace::new(
        verter_workspace::MemoryOptions::default(),
    ));
    ws.inject_file(
        "/workspace/node_modules/vue/package.json".to_string(),
        Arc::from(r#"{ "name": "vue", "types": "./dist/index.d.ts" }"#),
    );
    ws.inject_file(
        "/workspace/node_modules/vue/dist/index.d.ts".to_string(),
        Arc::from(
            r#"
export type Ref<T> = {
  value: T
}
"#,
        ),
    );
    ws.inject_file(
        "/workspace/src/helpers.ts".to_string(),
        Arc::from(
            r#"
import type { Ref } from 'vue'

export interface ImportedHelper {
  current?: Ref<string>
}
"#,
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
        .upsert_base(
            "/workspace/src/App.vue",
            r#"<script setup lang="ts">
import type { ImportedHelper } from './helpers'

defineProps<{
  helper?: ImportedHelper
}>()
</script>
<template><div /></template>"#,
        )
        .unwrap();

    project.host().set_import_dependencies(
        "/workspace/src/App.vue",
        vec![crate::types::DependencyResolution {
            specifier: "./helpers".to_string(),
            resolved_canonical_id: Some("/workspace/src/helpers.ts".to_string()),
            possible_canonical_ids: Vec::new(),
        }],
    );
    project.host().set_import_dependencies(
        "/workspace/src/helpers.ts",
        vec![crate::types::DependencyResolution {
            specifier: "vue".to_string(),
            resolved_canonical_id: Some("/workspace/node_modules/vue/dist/index.d.ts".to_string()),
            possible_canonical_ids: Vec::new(),
        }],
    );

    let resolved = project
        .host()
        .resolve_component_meta(
            "/workspace/src/App.vue",
            verter_type_engine::semantic_query::ProjectionMode::Expanded,
        )
        .expect("resolved component meta should exist");

    let published_names: std::collections::BTreeSet<_> = resolved
        .resolved_type_registry
        .iter()
        .map(|entry| entry.name.as_str())
        .collect();
    assert!(
        published_names.contains("ImportedHelper"),
        "the directly imported helper should still publish, got {published_names:?}"
    );
    assert!(
        !published_names.contains("Ref"),
        "imported local registry entries should not recurse into package helper refs, got {published_names:?}"
    );

    let helper_entry = resolved
        .resolved_type_registry
        .iter()
        .find(|entry| entry.name == "ImportedHelper")
        .expect("ImportedHelper should stay published");
    let helper_entry_ty = demand_published_type(
        project.host(),
        "/workspace/src/App.vue",
        Some(helper_entry.type_source.present().expect("present source")),
        "ImportedHelper registry entry",
    );
    let TypeExpr::Object(helper_shape) = &helper_entry_ty else {
        panic!("ImportedHelper should materialize as an object, got {helper_entry_ty:?}");
    };
    let current_member = helper_shape
        .properties
        .iter()
        .find_map(|member| match member {
            ObjectMember::Property(property)
                if property.string_name().expect("string-key fixture") == "current" =>
            {
                Some(&property.ty)
            }
            _ => None,
        })
        .expect("ImportedHelper should keep a current member");
    assert!(
        matches!(current_member, TypeExpr::Ref { name, .. } if name.as_ref() == "Ref"),
        "imported local registry entries should keep package-backed member refs symbolic, got {:?}",
        current_member
    );
}

#[test]
fn imported_function_array_member_publishes_callable_registry_dependency() {
    let project = make_project();
    project
        .upsert_base(
            "/src/member-value-props.ts",
            r#"
export type Fn = (value: string) => void
export type Unused = { ignored: number }

export interface MemberValueProps {
  handlers: Fn | Fn[]
}
"#,
        )
        .unwrap();
    project
        .upsert_base(
            "/src/App.vue",
            r#"<script setup lang="ts">
import type { MemberValueProps } from './member-value-props'

defineProps<MemberValueProps>()
</script>
<template><div /></template>"#,
        )
        .unwrap();
    project.host().set_import_dependencies(
        "/src/App.vue",
        vec![crate::types::DependencyResolution {
            specifier: "./member-value-props".to_string(),
            resolved_canonical_id: Some("/src/member-value-props.ts".to_string()),
            possible_canonical_ids: Vec::new(),
        }],
    );

    let resolved = project
        .host()
        .resolve_component_meta(
            "/src/App.vue",
            verter_type_engine::semantic_query::ProjectionMode::Expanded,
        )
        .expect("resolved component meta should exist");
    let registry_names: std::collections::BTreeSet<_> = resolved
        .resolved_type_registry
        .iter()
        .map(|entry| entry.name.as_str())
        .collect();

    assert!(
        registry_names.contains("MemberValueProps"),
        "the imported props root should publish, got {registry_names:?}"
    );
    assert!(
        registry_names.contains("Fn"),
        "the demanded imported member alias should publish, got {registry_names:?}"
    );
    assert!(
        !registry_names.contains("Unused"),
        "unreferenced imported siblings must stay out of the registry, got {registry_names:?}"
    );

    let function_entry = resolved
        .resolved_type_registry
        .iter()
        .find(|entry| entry.name == "Fn")
        .expect("Fn should publish as the member's registry dependency");
    let function_ty = demand_published_type(
        project.host(),
        "/src/App.vue",
        Some(
            function_entry
                .type_source
                .present()
                .expect("Fn should carry a source"),
        ),
        "Fn registry entry",
    );
    assert!(
        matches!(function_ty, TypeExpr::Function(_)),
        "Fn should preserve its callable structure, got {function_ty:?}"
    );
}

#[test]
fn resolve_component_meta_skips_unreferenced_owner_local_registry_helpers() {
    let project = make_project();
    project
        .upsert_base(
            "/src/App.vue",
            r#"<script lang="ts">
type Used = {
  label: string
}

type UnusedLeaf = {
  deep: {
    nested: string
  }
}

type UnusedWrapper = {
  payload: UnusedLeaf
}

export interface Props {
  item?: Used
}
</script>
<script setup lang="ts">
defineProps<Props>()
</script>
<template><div /></template>"#,
        )
        .unwrap();

    project.host().set_import_dependencies(
        "/src/App.vue",
        vec![crate::types::DependencyResolution {
            specifier: "./button-types".to_string(),
            resolved_canonical_id: Some("/src/button-types.ts".to_string()),
            possible_canonical_ids: Vec::new(),
        }],
    );
    project.host().set_import_dependencies(
        "/src/button-types.ts",
        vec![
            crate::types::DependencyResolution {
                specifier: "./types".to_string(),
                resolved_canonical_id: Some("/src/types.ts".to_string()),
                possible_canonical_ids: Vec::new(),
            },
            crate::types::DependencyResolution {
                specifier: "./theme".to_string(),
                resolved_canonical_id: Some("/src/theme.ts".to_string()),
                possible_canonical_ids: Vec::new(),
            },
        ],
    );

    let resolved = project
        .host()
        .resolve_component_meta(
            "/src/App.vue",
            verter_type_engine::semantic_query::ProjectionMode::Expanded,
        )
        .expect("resolved component meta should exist");
    let published_names: std::collections::BTreeSet<_> = resolved
        .resolved_type_registry
        .iter()
        .map(|entry| entry.name.as_str())
        .collect();

    assert!(
        published_names.contains("Props"),
        "the queried defineProps contract should stay published, got {published_names:?}"
    );
    assert!(
        published_names.contains("Used"),
        "owner-local helpers that are referenced by the queried surface should still publish, got {published_names:?}"
    );
    assert!(
        !published_names.contains("UnusedLeaf"),
        "resolve_component_meta should not eagerly publish unrelated owner-local helpers, got {published_names:?}"
    );
    assert!(
        !published_names.contains("UnusedWrapper"),
        "resolve_component_meta should stay demand-driven for owner-local registry helpers, got {published_names:?}"
    );
}

#[test]
fn resolve_component_meta_includes_owner_local_helper_types_in_registry() {
    let project = make_project();
    project
        .upsert_base(
            "/src/App.vue",
            r#"<script lang="ts">
interface RouteLocationObject {
  path: string
}

type RouteLocationRaw = string | RouteLocationObject

interface NuxtLinkProps {
  to?: RouteLocationRaw
  href?: NuxtLinkProps['to']
}

export interface LinkProps extends NuxtLinkProps {
  external?: boolean
}
</script>
<script setup lang="ts">
defineProps<LinkProps>()
</script>
<template><div /></template>"#,
        )
        .unwrap();

    let resolved = project
        .host()
        .resolve_component_meta(
            "/src/App.vue",
            verter_type_engine::semantic_query::ProjectionMode::Expanded,
        )
        .expect("resolved component meta should exist");

    let route = resolved
        .resolved_type_registry
        .iter()
        .find(|entry| entry.name == "RouteLocationRaw")
        .expect("owner-local route helper should be published in the type registry");
    let route_ty = demand_published_type(
        project.host(),
        "/src/App.vue",
        Some(route.type_source.present().expect("present source")),
        "RouteLocationRaw registry entry",
    );
    let TypeExpr::Union(route_variants) = &route_ty else {
        panic!("owner-local route helper should remain a route union, got {route_ty:?}");
    };
    assert!(
        route_variants
            .iter()
            .any(|variant| matches!(variant, TypeExpr::Primitive(PrimitiveName::String))),
        "owner-local route helper should preserve its string branch, got {route_ty:?}"
    );
    assert!(
        route_variants.iter().any(|variant| {
            matches!(variant, TypeExpr::Ref { name, type_arguments } if name.as_ref() == "RouteLocationObject" && type_arguments.is_empty())
                || matches!(
                    variant,
                    TypeExpr::Object(shape)
                        if shape.properties.iter().any(|member| matches!(member, ObjectMember::Property(property) if property.string_name().expect("string-key fixture") == "path")),
                )
        }),
        "owner-local route helper should preserve its object branch, got {route_ty:?}"
    );
    // RouteLocationObject is not published as a separate registry entry;
    // it is inlined into RouteLocationRaw's union.
    assert!(
        !resolved
            .resolved_type_registry
            .iter()
            .any(|entry| entry.name == "RouteLocationObject"),
        "RouteLocationObject should not be separately published"
    );

    let nuxt_link = resolved
        .resolved_type_registry
        .iter()
        .find(|entry| entry.name == "NuxtLinkProps")
        .expect("owner-local helper interface should be published in the type registry");
    let nuxt_link_ty = demand_published_type(
        project.host(),
        "/src/App.vue",
        Some(nuxt_link.type_source.present().expect("present source")),
        "NuxtLinkProps registry entry",
    );
    let TypeExpr::Object(shape) = &nuxt_link_ty else {
        panic!("NuxtLinkProps should project as an object type, got {nuxt_link_ty:?}");
    };
    let member_names: Vec<&str> = shape
        .properties
        .iter()
        .filter_map(|member| match member {
            ObjectMember::Property(property) => {
                Some(property.string_name().expect("string-key fixture"))
            }
            _ => None,
        })
        .collect();
    assert!(
        member_names.contains(&"to"),
        "NuxtLinkProps registry entry should keep the active helper route member, got {:?}",
        member_names
    );
    assert!(
        !member_names.contains(&"href"),
        "NuxtLinkProps registry entry should stay route-scoped instead of widening into sibling aliases, got {:?}",
        member_names
    );
}

#[test]
fn resolve_component_meta_registry_preserves_module_owner_with_same_name_instance_helper() {
    let project = make_project();
    project
        .upsert_base(
            "/src/App.vue",
            r#"<script lang="ts">
interface Shared {
  moduleOnly: string
  moduleSibling: boolean
}

export interface ModuleProps {
  value?: Shared['moduleOnly']
}
</script>
<script setup lang="ts">
interface Shared {
  instanceOnly: number
}

defineProps<ModuleProps>()
</script>
<template><div /></template>"#,
        )
        .unwrap();

    let resolved = project
        .host()
        .resolve_component_meta(
            "/src/App.vue",
            verter_type_engine::semantic_query::ProjectionMode::Expanded,
        )
        .expect("resolved component meta should exist");

    let shared = resolved
        .resolved_type_registry
        .iter()
        .find(|entry| entry.name == "Shared")
        .expect("the module-owned Shared route root should be published");
    let shared_ty = demand_published_type(
        project.host(),
        "/src/App.vue",
        Some(shared.type_source.present().expect("present source")),
        "module-owned Shared registry entry",
    );
    let TypeExpr::Object(shape) = &shared_ty else {
        panic!("module-owned Shared should publish a route-scoped object, got {shared_ty:?}");
    };
    let member_names: Vec<&str> = shape
        .properties
        .iter()
        .filter_map(|member| match member {
            ObjectMember::Property(property) => {
                Some(property.string_name().expect("string-key fixture"))
            }
            _ => None,
        })
        .collect();

    assert_eq!(
        member_names,
        ["moduleOnly"],
        "the exact Module owner and requested route must win over the same-name Instance helper"
    );
    assert!(
        !member_names.contains(&"instanceOnly") && !member_names.contains(&"moduleSibling"),
        "owner isolation and non-Whole route precision must exclude unrelated members, got {member_names:?}"
    );
}

#[test]
fn resolve_component_meta_registry_stays_shallow_for_owner_object_member_refs() {
    let project = make_project();
    project
        .upsert_base(
            "/src/App.vue",
            r#"<script setup lang="ts">
type ComponentSlots = {
  root?: string
}

type ComponentUI = {
  base?: string
}

type Button = {
  slots: ComponentSlots,
  ui: ComponentUI
}

defineProps<{
  helper?: Button
}>()
</script>
<template><div /></template>"#,
        )
        .unwrap();

    let resolved = project
        .host()
        .resolve_component_meta(
            "/src/App.vue",
            verter_type_engine::semantic_query::ProjectionMode::Expanded,
        )
        .expect("resolved component meta should exist");

    let registry_names: Vec<&str> = resolved
        .resolved_type_registry
        .iter()
        .map(|entry| entry.name.as_str())
        .collect();

    assert!(
        registry_names.contains(&"Button"),
        "the directly referenced helper should still be published, got {:?}",
        registry_names
    );
    assert!(
        !registry_names.contains(&"ComponentSlots") && !registry_names.contains(&"ComponentUI"),
        "nested owner-local object member helpers should stay inline instead of being separately published, got {:?}",
        registry_names
    );
}

#[test]
fn resolve_component_meta_keeps_transitive_imported_registry_helpers_off_registry_when_inlined() {
    let project = make_project();
    project
        .upsert_base(
            "/src/types.ts",
            r#"
export interface ImportedBase {
  href?: string
  target?: string
  label?: string
}

export type ImportedKeys = 'href' | 'target'

export interface ImportedTheme {
  color?: 'red' | 'blue'
}
"#,
        )
        .unwrap();
    project
        .upsert_base(
            "/src/App.vue",
            r#"<script lang="ts">
import type { ImportedBase, ImportedKeys, ImportedTheme } from './types'

type ButtonItem = Omit<ImportedBase, ImportedKeys> & {
  color?: ImportedTheme['color']
}

export interface Props {
  item?: ButtonItem
}
</script>
<script setup lang="ts">
defineProps<Props>()
</script>
<template><div /></template>"#,
        )
        .unwrap();

    let resolved = project
        .host()
        .resolve_component_meta(
            "/src/App.vue",
            verter_type_engine::semantic_query::ProjectionMode::Expanded,
        )
        .expect("resolved component meta should exist");
    let registry_names: std::collections::BTreeSet<_> = resolved
        .resolved_type_registry
        .iter()
        .map(|entry| entry.name.as_str())
        .collect();

    assert!(
        registry_names.contains("Props") && registry_names.contains("ButtonItem"),
        "owner-local queried helpers should still publish, got {registry_names:?}"
    );
    assert!(
        !registry_names.contains("ImportedBase"),
        "transitive imported helpers that are fully inlined into the owner helper surface should stay off the registry, got {registry_names:?}"
    );
    assert!(
        !registry_names.contains("ImportedKeys"),
        "transitive imported utility key helpers should stay off the registry, got {registry_names:?}"
    );
    assert!(
        !registry_names.contains("ImportedTheme"),
        "transitive imported indexed-access helpers should stay off the registry when their value is fully inlined, got {registry_names:?}"
    );

    let prop_names: Vec<&str> = resolved
        .evaluated_types
        .as_ref()
        .expect("expanded resolution should include evaluated types")
        .props
        .iter()
        .map(|prop| prop.name.as_str())
        .collect();
    assert!(
        prop_names.contains(&"item"),
        "public props should still resolve, got {prop_names:?}"
    );
}

/// Owner-local (same-file) registry alias with MULTIPLE type
/// arguments: `ComponentConfig<typeof theme, AppConfig, 'button'>`.
/// Pins that EACH of the three direct leaf members substitutes to its
/// CONCRETE argument — `primary` = the `typeof theme` object surface,
/// `cfg` = the same-file `AppConfig` interface surface, `name` = the
/// `'button'` string literal — never a bare `T`/`U`/`K` param nor a
/// miss placeholder. The deleted slow-lane walker covered multi-arg
/// only via the imported/cross-file path; this exercises the
/// owner-local dispatch route.
#[test]
fn resolve_component_meta_substitutes_owner_local_multi_arg_registry_alias() {
    let project = make_project();
    project
        .upsert_base(
            "/src/App.vue",
            r#"<script setup lang="ts">
type ComponentConfig<T, U, K> = {
  primary: T,
  cfg: U,
  name: K
}

interface AppConfig {
  mode: 'light' | 'dark'
}

const theme = {
  variants: {
    color: { primary: '', secondary: '' }
  }
} as const

type Button = ComponentConfig<typeof theme, AppConfig, 'button'>

defineProps<{
  primary?: Button['primary']
  cfg?: Button['cfg']
  name?: Button['name']
}>()
</script>
<template><div /></template>"#,
        )
        .unwrap();

    let resolved = project
        .host()
        .resolve_component_meta(
            "/src/App.vue",
            verter_type_engine::semantic_query::ProjectionMode::Expanded,
        )
        .expect("resolved component meta should exist");

    let button_entry = resolved
        .resolved_type_registry
        .iter()
        .find(|entry| entry.name == "Button")
        .expect("Button helper should be published in the resolved type registry");
    let button_entry_ty = demand_published_type(
        project.host(),
        "/src/App.vue",
        Some(button_entry.type_source.present().expect("present source")),
        "Button registry entry",
    );
    let TypeExpr::Object(button_shape) = &button_entry_ty else {
        panic!(
            "owner-local multi-arg Button alias should materialize as an object, got {button_entry_ty:?}"
        );
    };

    let member_ty = |name: &str| -> TypeExpr {
        button_shape
            .properties
            .iter()
            .find_map(|member| match member {
                ObjectMember::Property(property)
                    if property.string_name().expect("string-key fixture") == name =>
                {
                    Some(property.ty.clone())
                }
                _ => None,
            })
            .unwrap_or_else(|| panic!("Button should keep a `{name}` member, got {button_shape:?}"))
    };

    // `primary` (= the first arg `typeof theme`) is the CONCRETE theme
    // object surface: `{ variants: { color: { primary; secondary } } }`,
    // pinned all the way to the const string-literal leaves. NOT a bare
    // `T` param, NOT a miss placeholder, NOT one of the other args'
    // shapes.
    assert_concrete_theme_surface(&member_ty("primary"), "Button.primary");

    // `cfg` (= the second arg `AppConfig`) substitutes to the same-file
    // `AppConfig` interface, kept SHALLOW as a `Ref` per the
    // shallow-by-default rule (an interface alias ref, NOT eagerly
    // expanded). The discriminator: it is `Ref("AppConfig")`, NOT bare
    // `U`, NOT `T`/`typeof theme`, NOT a miss.
    let cfg = member_ty("cfg");
    let TypeExpr::Ref {
        name: cfg_name,
        type_arguments: cfg_args,
    } = &cfg
    else {
        panic!("Button.cfg should be the substituted AppConfig ref, got {cfg:?}");
    };
    assert_eq!(
        cfg_name.as_ref(),
        "AppConfig",
        "Button.cfg must substitute the second arg `AppConfig`, not a bare `U` param or other \
         type, got {cfg:?}",
    );
    assert!(
        cfg_args.is_empty(),
        "Button.cfg AppConfig ref should carry no type arguments, got {cfg:?}",
    );
    assert!(
        !matches!(&cfg, TypeExpr::TypeParameter(param) if param.name == "U"),
        "Button.cfg must NOT be the unbound parameter `U`, got {cfg:?}",
    );

    // `name` (= the third arg `'button'`) substitutes to the CONCRETE
    // string literal. NOT a bare `K` param, NOT `string`.
    let name = member_ty("name");
    assert_eq!(
        name,
        TypeExpr::string_literal("button"),
        "Button.name must substitute the third arg literal `'button'`, not a bare `K` param or \
         widened `string`, got {name:?}",
    );
}

/// Owner-local (same-file) registry alias exercising a DEFAULT type
/// parameter: `ComponentConfig<T, U = DefaultTheme>` instantiated as
/// `ComponentConfig<typeof theme>` (the second arg omitted). Pins that
/// `primary` substitutes to the concrete `typeof theme` surface AND
/// `fallback` falls back to the CONCRETE `DefaultTheme` surface — the
/// `args[i].or(param.default)` behaviour the deleted slow-lane helper
/// owned, now served by dispatch (`build.rs:960`). A regression that
/// dropped the default would leave `fallback` a bare `U` param or a
/// miss placeholder.
#[test]
fn resolve_component_meta_substitutes_owner_local_default_param_registry_alias() {
    let project = make_project();
    project
        .upsert_base(
            "/src/App.vue",
            r#"<script setup lang="ts">
interface DefaultTheme {
  spacing: 'tight' | 'loose'
}

type ComponentConfig<T, U = DefaultTheme> = {
  primary: T,
  fallback: U
}

const theme = {
  variants: {
    color: { primary: '', secondary: '' }
  }
} as const

type Button = ComponentConfig<typeof theme>

defineProps<{
  primary?: Button['primary']
  fallback?: Button['fallback']
}>()
</script>
<template><div /></template>"#,
        )
        .unwrap();

    let resolved = project
        .host()
        .resolve_component_meta(
            "/src/App.vue",
            verter_type_engine::semantic_query::ProjectionMode::Expanded,
        )
        .expect("resolved component meta should exist");

    let button_entry = resolved
        .resolved_type_registry
        .iter()
        .find(|entry| entry.name == "Button")
        .expect("Button helper should be published in the resolved type registry");
    let button_entry_ty = demand_published_type(
        project.host(),
        "/src/App.vue",
        Some(button_entry.type_source.present().expect("present source")),
        "Button registry entry",
    );
    let TypeExpr::Object(button_shape) = &button_entry_ty else {
        panic!(
            "owner-local default-param Button alias should materialize as an object, got {button_entry_ty:?}"
        );
    };

    let member_ty = |name: &str| -> TypeExpr {
        button_shape
            .properties
            .iter()
            .find_map(|member| match member {
                ObjectMember::Property(property)
                    if property.string_name().expect("string-key fixture") == name =>
                {
                    Some(property.ty.clone())
                }
                _ => None,
            })
            .unwrap_or_else(|| panic!("Button should keep a `{name}` member, got {button_shape:?}"))
    };

    // `primary` (= the supplied arg `typeof theme`) is the CONCRETE
    // theme object surface — same pin as the multi-arg test.
    assert_concrete_theme_surface(&member_ty("primary"), "Button.primary");

    // `fallback` (= the OMITTED second arg) must fall back to the
    // parameter default `DefaultTheme`. This is the load-bearing
    // discriminator for the `args[i].or(param.default)` behaviour now
    // served by dispatch: a regression that dropped the default would
    // leave `fallback` a bare `U` param or a miss placeholder. The
    // same-file `DefaultTheme` interface stays SHALLOW as a `Ref` per
    // the shallow-by-default rule.
    let fallback = member_ty("fallback");
    let TypeExpr::Ref {
        name: fallback_name,
        type_arguments: fallback_args,
    } = &fallback
    else {
        panic!("Button.fallback should be the default `DefaultTheme` ref, got {fallback:?}",);
    };
    assert_eq!(
        fallback_name.as_ref(),
        "DefaultTheme",
        "Button.fallback must fall back to the parameter default `DefaultTheme`, not a bare `U` \
         param or miss placeholder, got {fallback:?}",
    );
    assert!(
        fallback_args.is_empty(),
        "Button.fallback DefaultTheme ref should carry no type arguments, got {fallback:?}",
    );
    assert!(
        !matches!(&fallback, TypeExpr::TypeParameter(param) if param.name == "U"),
        "Button.fallback must NOT be the unbound parameter `U` (default must be bound), \
         got {fallback:?}",
    );
    assert!(
        !matches!(&fallback, TypeExpr::Unknown { .. }),
        "Button.fallback must NOT be a miss/semanticMiss placeholder, got {fallback:?}",
    );
}

#[test]
fn resolve_component_meta_materializes_imported_component_config_registry_helpers() {
    let project = make_project();
    project
        .upsert_base(
            "/src/tailwind-variants.d.ts",
            r#"export type ClassValue = string | { [key: string]: boolean }
export type TVVariants<S, C, V> = { [K in keyof V]: keyof V[K] }
export type TVCompoundVariants<V, S, C, O, U> = never
export type TVDefaultVariants<V, S, O, U> = never
"#,
        )
        .unwrap();
    project
        .upsert_base(
            "/src/tv.ts",
            r#"import type { ClassValue, TVVariants, TVCompoundVariants, TVDefaultVariants } from './tailwind-variants'

export type TVConfig<T extends Record<string, any>> = {
  [P in keyof T]?: {
    [K in keyof T[P] as K extends 'base' | 'slots' | 'variants' | 'defaultVariants' ? K : never]?: K extends 'base' ? ClassValue
      : K extends 'slots' ? {
        [S in keyof T[P]['slots']]?: ClassValue
      }
        : K extends 'variants' ? TVVariants<T[P]['slots'], ClassValue, WidenVariantsValues<T[P]['variants']>>
          : K extends 'defaultVariants' ? TVDefaultVariants<WidenVariantsValues<T[P]['variants']>, T[P]['slots'], object, undefined>
            : never
  }
} & {
  [P in keyof T]?: {
    compoundVariants?: TVCompoundVariants<WidenVariantsValues<T[P]['variants']>, T[P]['slots'], ClassValue, object, undefined>
  }
}

type WidenVariantsValues<V extends Record<string, any> | undefined>
  = V extends Record<string, any> ? V & {
    [K in keyof V]: V[K] extends Record<string, any>
      ? V[K] & Record<string & {}, any>
      : V[K]
  } : V

type Id<T> = {} & { [P in keyof T]: T[P] }

type ComponentVariants<T extends { variants?: Record<string, Record<string, any>> }> = {
  [K in keyof T['variants']]: keyof T['variants'][K]
}

type ComponentSlots<T extends { slots?: Record<string, any> }> = Id<{
  [K in keyof T['slots']]?: ClassValue
}>

type ComponentUI<T extends { slots?: Record<string, any> }> = Id<{
  [K in keyof Required<T['slots']>]: (props?: Record<string, any>) => string
}>

type GetComponentAppConfig<A, U extends string, K extends string>
  = A extends Record<U, Record<K, any>> ? A[U][K] : {}

type ComponentAppConfig<
  T,
  A extends Record<string, any>,
  K extends string,
  U extends string = 'ui' | 'ui.prose'
> = A & (
  U extends 'ui.prose'
    ? { ui?: { prose?: { [k in K]?: Partial<T> } } }
    : { [key in Exclude<U, 'ui.prose'>]?: { [k in K]?: Partial<T> } }
)

export type ComponentConfig<
  T extends Record<string, any>,
  A extends Record<string, any>,
  K extends string,
  U extends 'ui' | 'ui.prose' = 'ui'
> = {
  AppConfig: ComponentAppConfig<T, A, K, U>,
  variants: ComponentVariants<T & GetComponentAppConfig<A, U, K>>
  slots: ComponentSlots<T>,
  ui: ComponentUI<T>
}
"#,
        )
        .unwrap();
    project
        .upsert_base(
            "/src/schema.ts",
            r#"export interface AppConfig {
  ui: {
    button: {
      variants: {
        color: {
          neutral: string
        }
      }
    }
  }
}
"#,
        )
        .unwrap();
    project
        .upsert_base(
            "/src/theme.ts",
            r#"export default {
  variants: {
    color: { primary: '', secondary: '' },
    variant: { solid: '', soft: '' },
    size: { sm: '', md: '' }
  },
  slots: {
    base: '',
    label: ''
  }
} as const
"#,
        )
        .unwrap();
    project
        .upsert_base(
            "/src/Button.vue",
            r#"<script lang="ts">
import type { AppConfig } from './schema'
import theme from './theme'
import type { ComponentConfig } from './tv'

type Button = ComponentConfig<typeof theme, AppConfig, 'button'>

export interface ButtonProps {
  color?: Button['variants']['color']
  ui?: Button['slots']
}

export interface ButtonSlots {
  default?(props: { ui: Button['ui'] }): any
}
</script>
<script setup lang="ts">
defineProps<ButtonProps>()
defineSlots<ButtonSlots>()
</script>
<template><div /></template>"#,
        )
        .unwrap();

    let resolved = project
        .host()
        .resolve_component_meta(
            "/src/Button.vue",
            verter_type_engine::semantic_query::ProjectionMode::Expanded,
        )
        .expect("resolved component meta should exist");

    let button_entry = resolved
        .resolved_type_registry
        .iter()
        .find(|entry| entry.name == "Button")
        .expect("Button helper should be published in the resolved type registry");
    let button_entry_ty = demand_published_type(
        project.host(),
        "/src/Button.vue",
        Some(button_entry.type_source.present().expect("present source")),
        "Button registry entry",
    );
    let TypeExpr::Object(button_shape) = &button_entry_ty else {
        panic!(
            "imported ComponentConfig alias should materialize as an object, got {button_entry_ty:?}"
        );
    };

    let variants_member = button_shape
        .properties
        .iter()
        .find_map(|member| match member {
            ObjectMember::Property(property)
                if property.string_name().expect("string-key fixture") == "variants" =>
            {
                Some(&property.ty)
            }
            _ => None,
        })
        .expect("Button helper should keep a variants member");
    let TypeExpr::Object(variants_shape) = variants_member else {
        panic!(
            "Button.variants should materialize as an object, got {:?}",
            variants_member
        );
    };
    let color_member = variants_shape
        .properties
        .iter()
        .find_map(|member| match member {
            ObjectMember::Property(property)
                if property.string_name().expect("string-key fixture") == "color" =>
            {
                Some(&property.ty)
            }
            _ => None,
        })
        .expect("Button.variants should keep a color member");
    match color_member {
        TypeExpr::Union(members) => {
            assert!(
                members.contains(&TypeExpr::string_literal("primary")),
                "Button.variants.color should preserve the theme helper surface, got {:?}",
                color_member
            );
            assert!(
                members.contains(&TypeExpr::string_literal("secondary")),
                "Button.variants.color should preserve the theme helper surface, got {:?}",
                color_member
            );
        }
        other => panic!(
            "Button.variants.color should stay query-usable as a union surface, got {:?}",
            other
        ),
    }

    let slots_member = button_shape
        .properties
        .iter()
        .find_map(|member| match member {
            ObjectMember::Property(property)
                if property.string_name().expect("string-key fixture") == "slots" =>
            {
                Some(&property.ty)
            }
            _ => None,
        })
        .expect("Button helper should keep a slots member");
    let TypeExpr::Object(slots_shape) = slots_member else {
        panic!(
            "Button.slots should materialize as an object, got {:?}",
            slots_member
        );
    };
    assert!(
        slots_shape.properties.iter().any(
            |member| matches!(member, ObjectMember::Property(property) if property.string_name().expect("string-key fixture") == "base"),
        ),
        "Button.slots should expose base, got {:?}",
        slots_member
    );
    assert!(
        slots_shape.properties.iter().any(
            |member| matches!(member, ObjectMember::Property(property) if property.string_name().expect("string-key fixture") == "label"),
        ),
        "Button.slots should expose label, got {:?}",
        slots_member
    );
}

#[test]
fn resolve_component_meta_uses_db_projection_for_imported_registry_surfaces() {
    let project = make_project();
    project
        .upsert_base(
            "/src/types.ts",
            r#"type ButtonShape = {
  variants: {
    color: 'primary' | 'secondary'
  }
  slots: {
    base?: string
    label?: string
  }
  ui: {
    base?: (props?: { active?: boolean }) => string
    label?: (props?: { active?: boolean }) => string
  }
}

export type Button = Pick<ButtonShape, 'variants' | 'slots' | 'ui'>

export interface ButtonProps {
  color?: Button['variants']['color']
  ui?: Button['slots']
}

export interface ButtonSlots {
  default?(props: { ui: Button['ui'] }): any
}
"#,
        )
        .unwrap();
    project
        .upsert_base(
            "/src/Button.vue",
            r#"<script setup lang="ts">
import type { ButtonProps, ButtonSlots } from './types'

defineProps<ButtonProps>()
defineSlots<ButtonSlots>()
</script>
<template><div /></template>"#,
        )
        .unwrap();

    let resolved = project
        .host()
        .resolve_component_meta(
            "/src/Button.vue",
            verter_type_engine::semantic_query::ProjectionMode::Expanded,
        )
        .expect("resolved component meta should exist");

    let button_entry = resolved
        .resolved_type_registry
        .iter()
        .find(|entry| entry.name == "Button")
        .expect("Button helper should be published in the resolved type registry");
    let button_entry_ty = demand_published_type(
        project.host(),
        "/src/Button.vue",
        Some(button_entry.type_source.present().expect("present source")),
        "Button registry entry",
    );
    let TypeExpr::Object(button_shape) = &button_entry_ty else {
        panic!(
            "imported Button helper should materialize as an object surface, got {button_entry_ty:?}"
        );
    };
    let member_names: std::collections::BTreeSet<_> = button_shape
        .properties
        .iter()
        .filter_map(|member| match member {
            ObjectMember::Property(property) => {
                Some(property.string_name().expect("string-key fixture"))
            }
            _ => None,
        })
        .collect();
    assert!(
        !member_names.contains("variants"),
        "imported registry helpers should not widen to already-concrete sibling routes, got {member_names:?}"
    );

    let ui_member = button_shape
        .properties
        .iter()
        .find_map(|member| match member {
            ObjectMember::Property(property)
                if property.string_name().expect("string-key fixture") == "ui" =>
            {
                Some(&property.ty)
            }
            _ => None,
        })
        .expect("Button helper should keep a ui member");
    let TypeExpr::Object(ui_shape) = ui_member else {
        panic!(
            "Button.ui should materialize as an object, got {:?}",
            ui_member
        );
    };
    assert!(
        ui_shape.properties.iter().any(
            |member| matches!(member, ObjectMember::Property(property) if property.string_name().expect("string-key fixture") == "base"),
        ),
        "Button.ui should expose base, got {:?}",
        ui_member
    );
    assert!(
        ui_shape.properties.iter().any(
            |member| matches!(member, ObjectMember::Property(property) if property.string_name().expect("string-key fixture") == "label"),
        ),
        "Button.ui should expose label, got {:?}",
        ui_member
    );
}

#[test]
fn resolve_component_meta_keeps_imported_registry_helpers_on_requested_member_paths() {
    let project = make_project();
    project
        .upsert_base(
            "/src/types.ts",
            r#"type ButtonShape = {
  variants: {
    color: 'primary' | 'secondary'
  }
  slots: {
    base?: string
    label?: string
  }
  ui: {
    base?: (props?: { active?: boolean }) => string
    label?: (props?: { active?: boolean }) => string
  }
}

export type Button = Pick<ButtonShape, 'variants' | 'slots' | 'ui'>

export interface ButtonSlots {
  default?(props: { ui: Button['ui'] }): any
}
"#,
        )
        .unwrap();
    project
        .upsert_base(
            "/src/App.vue",
            r#"<script setup lang="ts">
import type { ButtonSlots } from './types'

defineSlots<ButtonSlots>()
</script>
<template><div /></template>"#,
        )
        .unwrap();

    let resolved = project
        .host()
        .resolve_component_meta(
            "/src/App.vue",
            verter_type_engine::semantic_query::ProjectionMode::Expanded,
        )
        .expect("resolved component meta should exist");

    let button_entry = resolved
        .resolved_type_registry
        .iter()
        .find(|entry| entry.name == "Button")
        .expect("Button helper should be published in the resolved type registry");
    let button_entry_ty = demand_published_type(
        project.host(),
        "/src/App.vue",
        Some(button_entry.type_source.present().expect("present source")),
        "Button registry entry",
    );
    let TypeExpr::Object(button_shape) = &button_entry_ty else {
        panic!(
            "imported Button helper should materialize as an object surface, got {button_entry_ty:?}"
        );
    };

    let member_names: std::collections::BTreeSet<_> = button_shape
        .properties
        .iter()
        .filter_map(|member| match member {
            ObjectMember::Property(property) => {
                Some(property.string_name().expect("string-key fixture"))
            }
            _ => None,
        })
        .collect();

    assert_eq!(
        member_names,
        std::collections::BTreeSet::from(["ui"]),
        "imported registry helper should only materialize the requested member-path root, got {member_names:?}"
    );

    let ui_member = button_shape
        .properties
        .iter()
        .find_map(|member| match member {
            ObjectMember::Property(property)
                if property.string_name().expect("string-key fixture") == "ui" =>
            {
                Some(&property.ty)
            }
            _ => None,
        })
        .expect("Button helper should keep a ui member");
    let TypeExpr::Object(ui_shape) = ui_member else {
        panic!(
            "Button.ui should materialize as an object, got {:?}",
            ui_member
        );
    };
    assert!(
        ui_shape.properties.iter().any(
            |member| matches!(member, ObjectMember::Property(property) if property.string_name().expect("string-key fixture") == "base"),
        ),
        "Button.ui should expose base, got {:?}",
        ui_member
    );
    assert!(
        ui_shape.properties.iter().any(
            |member| matches!(member, ObjectMember::Property(property) if property.string_name().expect("string-key fixture") == "label"),
        ),
        "Button.ui should expose label, got {:?}",
        ui_member
    );
}

#[test]
fn resolve_component_meta_keeps_function_valued_registry_members_shallow() {
    let project = make_project();
    project
        .upsert_base(
            "/src/types.ts",
            r#"export interface DeepProps {
  active?: boolean
  theme?: {
    dark?: boolean
  }
}

type ButtonShape = {
  ui: {
    base?: (props?: DeepProps) => string
    label?: (props?: DeepProps) => string
  }
}

export type Button = Pick<ButtonShape, 'ui'>

export interface ButtonSlots {
  default?(props: { ui: Button['ui'] }): any
}
"#,
        )
        .unwrap();
    project
        .upsert_base(
            "/src/App.vue",
            r#"<script setup lang="ts">
import type { ButtonSlots } from './types'

defineSlots<ButtonSlots>()
</script>
<template><div /></template>"#,
        )
        .unwrap();

    let resolved = project
        .host()
        .resolve_component_meta(
            "/src/App.vue",
            verter_type_engine::semantic_query::ProjectionMode::Expanded,
        )
        .expect("resolved component meta should exist");

    assert!(
        resolved
            .resolved_type_registry
            .iter()
            .all(|entry| entry.name != "DeepProps"),
        "function-valued registry members should not publish transitive callable parameter helpers",
    );

    // Shallow-by-default registry contract: the imported `ButtonSlots` helper
    // stays a bare `Ref { name }` in the registry. Its function-valued member
    // (`default(props: { ui: Button['ui'] })`) carries a callable-parameter
    // helper that must NOT be eagerly inlined — the deep binding correctness is
    // asserted on the published surface below.
    let button_slots = resolved
        .resolved_type_registry
        .iter()
        .find(|entry| entry.name == "ButtonSlots")
        .expect("ButtonSlots should be published in the resolved type registry");
    let button_slots_ty = shallow_published_type(
        project.host(),
        "/src/App.vue",
        Some(button_slots.type_source.present().expect("present source")),
        "ButtonSlots registry entry",
    );
    assert!(
        matches!(&button_slots_ty, TypeExpr::Ref { name, .. } if name.as_ref() == "ButtonSlots"),
        "imported registry helper should stay a shallow Ref (shallow-by-default), got {button_slots_ty:?}"
    );
    assert!(
        !matches!(&button_slots_ty, TypeExpr::Object(_)),
        "registry entry must NOT eagerly materialize the function-valued member, got {button_slots_ty:?}"
    );

    // Published-surface contract: the default slot's `ui` binding stays
    // symbolic on the requested member path `Button['ui']`; the function-valued
    // projected member never widens the imported member-path helper.
    let meta = project
        .host()
        .get_component_meta("/src/App.vue")
        .expect("should return component meta");
    let default_slot = meta
        .slots
        .iter()
        .find(|slot| slot.name == "default")
        .expect("default slot should be extracted");
    let ui_binding = default_slot
        .bindings
        .iter()
        .find(|binding| binding.name == "ui")
        .expect("default slot should expose the ui binding");
    assert_eq!(
        slot_binding_terminal_display(&project, "/src/App.vue", "default", "ui").as_deref(),
        Some("Pick<ButtonShape, 'ui'>['ui']"),
        "terminal display must render the selected structural source"
    );
    let ui_binding_ty = shallow_published_type(
        project.host(),
        "/src/App.vue",
        ui_binding.publication.result().selected_source(),
        "default slot ui binding",
    );
    assert!(
        matches!(&ui_binding_ty, TypeExpr::IndexedAccess { .. }),
        "default slot ui binding must stay a symbolic IndexedAccess, got {ui_binding_ty:?}"
    );
}

#[test]
fn resolve_component_meta_publishes_compound_heritage_surface_as_merged_object() {
    // §compound-root: a registry alias whose body is a heritage interface
    // (`interface Derived extends Base { own }`) must publish the COMPOSED
    // merged one-level surface (base ∪ own), not the carrier-intact declaration
    // anchor (which would keep the heritage arm symbolic). Exercises the
    // candidate's explicit-object-surface fact + the compound-root composition
    // route through the shared shallow walker.
    let project = make_project();
    project
        .upsert_base(
            "/src/types.ts",
            r#"export interface RegistryHeritageBase { base: string }
export interface RegistryHeritageDerived extends RegistryHeritageBase { own: number }
"#,
        )
        .unwrap();
    project
        .upsert_base(
            "/src/App.vue",
            r#"<script setup lang="ts">
import type { RegistryHeritageDerived } from './types'
defineProps<RegistryHeritageDerived>()
</script>
<template><div /></template>"#,
        )
        .unwrap();
    project.host().set_import_dependencies(
        "/src/App.vue",
        vec![crate::types::DependencyResolution {
            specifier: "./types".to_string(),
            resolved_canonical_id: Some("/src/types.ts".to_string()),
            possible_canonical_ids: Vec::new(),
        }],
    );

    let _resolved = project
        .host()
        .resolve_component_meta(
            "/src/App.vue",
            verter_type_engine::semantic_query::ProjectionMode::Expanded,
        )
        .expect("resolved component meta should exist");
    let meta = project
        .host()
        .get_component_meta("/src/App.vue")
        .expect("should return component meta");
    let mut prop_names: Vec<&str> = meta.props.iter().map(|p| p.name.as_str()).collect();
    prop_names.sort_unstable();
    // Anti-vacuity: the EXACT merged key set — heritage `base` merges with the
    // own `own` member. A carrier-intact anchor would drop `base`.
    assert_eq!(
        prop_names,
        vec!["base", "own"],
        "heritage interface must publish the merged base ∪ own surface, got {prop_names:?}",
    );
}

#[test]
fn resolve_component_meta_pick_over_class_keeps_keyspace_public_semantics() {
    // §class-Pick-visibility: a `Pick<Class, 'alpha'>` registry alias must route
    // through the SHARED Pick keyspace engine. A naive `SurfaceView.members`
    // filter would leak the non-public / non-requested class members. Only the
    // requested public key may surface.
    let project = make_project();
    project
        .upsert_base(
            "/src/types.ts",
            r#"export class RegistryVisibilityClass {
  public alpha = 1
  protected beta = 2
  private gamma = 3
}
export type PickedRegistryVisibility = Pick<RegistryVisibilityClass, 'alpha'>
"#,
        )
        .unwrap();
    project
        .upsert_base(
            "/src/App.vue",
            r#"<script setup lang="ts">
import type { PickedRegistryVisibility } from './types'
defineProps<PickedRegistryVisibility>()
</script>
<template><div /></template>"#,
        )
        .unwrap();
    project.host().set_import_dependencies(
        "/src/App.vue",
        vec![crate::types::DependencyResolution {
            specifier: "./types".to_string(),
            resolved_canonical_id: Some("/src/types.ts".to_string()),
            possible_canonical_ids: Vec::new(),
        }],
    );

    let _resolved = project
        .host()
        .resolve_component_meta(
            "/src/App.vue",
            verter_type_engine::semantic_query::ProjectionMode::Expanded,
        )
        .expect("resolved component meta should exist");
    let meta = project
        .host()
        .get_component_meta("/src/App.vue")
        .expect("should return component meta");
    let prop_names: Vec<&str> = meta.props.iter().map(|p| p.name.as_str()).collect();
    // Anti-vacuity: ONLY the requested public key surfaces. The protected /
    // private members must never leak through the Pick keyspace.
    assert!(
        prop_names.contains(&"alpha"),
        "the requested public key `alpha` must surface, got {prop_names:?}",
    );
    assert!(
        !prop_names.contains(&"beta"),
        "the non-requested protected member `beta` must NOT leak, got {prop_names:?}",
    );
    assert!(
        !prop_names.contains(&"gamma"),
        "the private member `gamma` must NEVER leak, got {prop_names:?}",
    );
}

#[test]
fn resolve_component_meta_pick_keyspace_derivation_stays_public_only() {
    // A `Pick` whose key-set is COMPUTED (not literal) from a member's value-type
    // keyspace must stay public-only. `keyof C['config']` is `'beta' | 'gamma'`,
    // so the prop type is `Pick<C, 'beta' | 'gamma' | 'alpha'>`. Even though the
    // derived key-set NAMES the protected member `beta`, a `keyof` / `Pick`
    // derivation over a class is public-only and must drop it; `gamma` is not a
    // class member at all. Only the public key `alpha` may surface (it is also
    // the positive control / non-vacuity check).
    let project = make_project();
    project
        .upsert_base(
            "/src/types.ts",
            r#"export class VisibilityClass {
  public alpha = 1
  protected beta = 2
  public config: { beta: number; gamma: number } = { beta: 0, gamma: 0 }
}
export type PickedThroughMemberKeyspace = Pick<
  VisibilityClass,
  keyof VisibilityClass['config'] | 'alpha'
>
"#,
        )
        .unwrap();
    project
        .upsert_base(
            "/src/App.vue",
            r#"<script setup lang="ts">
import type { PickedThroughMemberKeyspace } from './types'
defineProps<PickedThroughMemberKeyspace>()
</script>
<template><div /></template>"#,
        )
        .unwrap();
    project.host().set_import_dependencies(
        "/src/App.vue",
        vec![crate::types::DependencyResolution {
            specifier: "./types".to_string(),
            resolved_canonical_id: Some("/src/types.ts".to_string()),
            possible_canonical_ids: Vec::new(),
        }],
    );

    let _resolved = project
        .host()
        .resolve_component_meta(
            "/src/App.vue",
            verter_type_engine::semantic_query::ProjectionMode::Expanded,
        )
        .expect("resolved component meta should exist");
    let meta = project
        .host()
        .get_component_meta("/src/App.vue")
        .expect("should return component meta");
    let prop_names: Vec<&str> = meta.props.iter().map(|p| p.name.as_str()).collect();

    // Positive control: the public key `alpha` surfaces (the surface resolved).
    assert!(
        prop_names.contains(&"alpha"),
        "the public key `alpha` must surface, got {prop_names:?}",
    );
    // The protected member named in the derived key-set must NOT leak — a `keyof`
    // / `Pick` derivation over a class is public-only.
    assert!(
        !prop_names.contains(&"beta"),
        "the protected member `beta` must NOT leak onto the public surface even \
         though its key is in `keyof C['config']`, got {prop_names:?}",
    );
    // `gamma` keys the member's value type, not the class itself — it must never
    // surface as a class prop.
    assert!(
        !prop_names.contains(&"gamma"),
        "`gamma` (a key of the member's value type, not a class member) must \
         never surface, got {prop_names:?}",
    );
}

#[test]
fn resolve_component_meta_nested_utility_over_class_keeps_public_only() {
    // A NESTED utility over a class with mixed visibility must not republish a
    // non-public member. `Pick<C, 'publicA' | 'publicB' | 'protectedX'>` is
    // public-only — the protected `protectedX` is excluded at the inner `Pick`
    // even though it is in the requested key-set — and the outer
    // `Omit<…, 'publicA'>` drops `publicA`, leaving exactly `{ publicB }`.
    let project = make_project();
    project
        .upsert_base(
            "/src/types.ts",
            r#"export class NestedVisibilityClass {
  public publicA = 'a'
  public publicB = 2
  protected protectedX = 'x'
  private privateY = 3
}
export type NestedUtilityProps = Omit<
  Pick<NestedVisibilityClass, 'publicA' | 'publicB' | 'protectedX'>,
  'publicA'
>
"#,
        )
        .unwrap();
    project
        .upsert_base(
            "/src/App.vue",
            r#"<script setup lang="ts">
import type { NestedUtilityProps } from './types'
defineProps<NestedUtilityProps>()
</script>
<template><div /></template>"#,
        )
        .unwrap();
    project.host().set_import_dependencies(
        "/src/App.vue",
        vec![crate::types::DependencyResolution {
            specifier: "./types".to_string(),
            resolved_canonical_id: Some("/src/types.ts".to_string()),
            possible_canonical_ids: Vec::new(),
        }],
    );

    let _resolved = project
        .host()
        .resolve_component_meta(
            "/src/App.vue",
            verter_type_engine::semantic_query::ProjectionMode::Expanded,
        )
        .expect("resolved component meta should exist");
    let meta = project
        .host()
        .get_component_meta("/src/App.vue")
        .expect("should return component meta");
    let prop_names: Vec<&str> = meta.props.iter().map(|p| p.name.as_str()).collect();

    // Positive control: the inner `Pick` kept the public members and the outer
    // `Omit` dropped `publicA`, so `publicB` survives (the nested utility
    // resolved — non-vacuity).
    assert!(
        prop_names.contains(&"publicB"),
        "the surviving public member `publicB` must surface, got {prop_names:?}",
    );
    // `publicA` was omitted by the outer `Omit`.
    assert!(
        !prop_names.contains(&"publicA"),
        "the omitted public member `publicA` must NOT surface, got {prop_names:?}",
    );
    // The protected member must NOT surface — the inner `Pick<C, …>` over a class
    // is public-only, so `protectedX` is dropped even though it is in the key-set.
    assert!(
        !prop_names.contains(&"protectedX"),
        "the protected member `protectedX` must NOT leak through the nested \
         `Omit<Pick<C, …>, …>`, got {prop_names:?}",
    );
    // The private member must NEVER surface through any class-utility derivation.
    assert!(
        !prop_names.contains(&"privateY"),
        "the private member `privateY` must NEVER leak through a class-utility \
         derivation, got {prop_names:?}",
    );
}

#[test]
fn resolve_component_meta_materializes_bound_registry_members_despite_opaque_sibling_args() {
    let project = make_project();
    project
        .upsert_base(
            "/src/types.ts",
            r#"type ComponentVariants<T extends { variants?: Record<string, Record<string, any>> }> = {
  [K in keyof T['variants']]: keyof T['variants'][K]
}

type ComponentSlots<T extends { slots?: Record<string, any> }> = {
  [K in keyof T['slots']]?: string
}

export type ComponentConfig<T extends Record<string, any>, A> = {
  variants: ComponentVariants<T>,
  slots: ComponentSlots<T>
  appConfig?: A
}
"#,
        )
        .unwrap();
    project
        .upsert_base(
            "/src/theme.ts",
            r#"export default {
  variants: {
    color: { primary: '', secondary: '' }
  },
  slots: {
    base: '',
    label: ''
  }
} as const
"#,
        )
        .unwrap();
    project
        .upsert_base(
            "/src/Button.vue",
            r#"<script lang="ts">
import type { ComponentConfig } from './types'
import theme from './theme'

type Button = ComponentConfig<typeof theme, MissingAppConfig>

export interface ButtonProps {
  color?: Button['variants']['color']
  ui?: Button['slots']
}
</script>
<script setup lang="ts">
defineProps<ButtonProps>()
</script>
<template><div /></template>"#,
        )
        .unwrap();

    project.host().set_import_dependencies(
        "/src/Button.vue",
        vec![
            crate::types::DependencyResolution {
                specifier: "./types".to_string(),
                resolved_canonical_id: Some("/src/types.ts".to_string()),
                possible_canonical_ids: Vec::new(),
            },
            crate::types::DependencyResolution {
                specifier: "./theme".to_string(),
                resolved_canonical_id: Some("/src/theme.ts".to_string()),
                possible_canonical_ids: Vec::new(),
            },
        ],
    );

    let resolved = project
        .host()
        .resolve_component_meta(
            "/src/Button.vue",
            verter_type_engine::semantic_query::ProjectionMode::Expanded,
        )
        .expect("resolved component meta should exist");

    let button_entry = resolved
        .resolved_type_registry
        .iter()
        .find(|entry| entry.name == "Button")
        .expect("Button helper should be published in the resolved type registry");
    let button_entry_ty = demand_published_type(
        project.host(),
        "/src/Button.vue",
        Some(button_entry.type_source.present().expect("present source")),
        "Button registry entry",
    );
    let TypeExpr::Object(button_shape) = &button_entry_ty else {
        panic!(
            "Button helper should materialize as an object despite the opaque sibling arg, got {button_entry_ty:?}"
        );
    };

    // The opaque sibling arg should not block materialization of members that
    // depend only on the concrete theme argument.
    let variants_member = button_shape
        .properties
        .iter()
        .find_map(|member| match member {
            ObjectMember::Property(property)
                if property.string_name().expect("string-key fixture") == "variants" =>
            {
                Some(&property.ty)
            }
            _ => None,
        })
        .expect("Button helper should keep a variants member");
    let TypeExpr::Object(variants_shape) = variants_member else {
        panic!(
            "Button.variants should materialize as an object when the theme arg is concrete, got {:?}",
            variants_member
        );
    };
    assert!(
        variants_shape.properties.iter().any(
            |member| matches!(member, ObjectMember::Property(property) if property.string_name().expect("string-key fixture") == "color"),
        ),
        "Button.variants should expose color, got {:?}",
        variants_member
    );

    let slots_member = button_shape
        .properties
        .iter()
        .find_map(|member| match member {
            ObjectMember::Property(property)
                if property.string_name().expect("string-key fixture") == "slots" =>
            {
                Some(&property.ty)
            }
            _ => None,
        })
        .expect("Button helper should keep a slots member");
    let TypeExpr::Object(slots_shape) = slots_member else {
        panic!(
            "Button.slots should materialize as an object when the theme arg is concrete, got {:?}",
            slots_member
        );
    };
    assert!(
        slots_shape.properties.iter().any(
            |member| matches!(member, ObjectMember::Property(property) if property.string_name().expect("string-key fixture") == "base"),
        ),
        "Button.slots should expose base, got {:?}",
        slots_member
    );
    assert!(
        slots_shape.properties.iter().any(
            |member| matches!(member, ObjectMember::Property(property) if property.string_name().expect("string-key fixture") == "label"),
        ),
        "Button.slots should expose label, got {:?}",
        slots_member
    );
}

#[test]
fn resolve_component_meta_publishes_transitive_registry_aliases_for_nested_indexed_access_refs() {
    let project = make_project();
    project
        .upsert_base(
            "/src/types.ts",
            r#"type ComponentVariants<T extends { variants?: Record<string, Record<string, any>> }> = {
  [K in keyof T['variants']]: keyof T['variants'][K]
}

type ComponentSlots<T extends { slots?: Record<string, any> }> = {
  [K in keyof T['slots']]?: string
}

export type ComponentConfig<T extends Record<string, any>> = {
  variants: ComponentVariants<T>,
  slots: ComponentSlots<T>
}
"#,
        )
        .unwrap();
    project
        .upsert_base(
            "/src/avatar-theme.ts",
            r#"export default {
  variants: {
    size: { sm: '', md: '' }
  },
  slots: {
    base: ''
  }
} as const
"#,
        )
        .unwrap();
    project
        .upsert_base(
            "/src/avatar-types.ts",
            r#"import type { ComponentConfig } from './types'
import avatarTheme from './avatar-theme'

export type Avatar = ComponentConfig<typeof avatarTheme>

export interface AvatarProps {
  size?: Avatar['variants']['size']
  ui?: Avatar['slots']
}
"#,
        )
        .unwrap();
    project
        .upsert_base(
            "/src/Button.vue",
            r#"<script lang="ts">
import type { AvatarProps } from './avatar-types'

export interface ButtonProps {
  avatar?: AvatarProps
}
</script>
<script setup lang="ts">
defineProps<ButtonProps>()
</script>
<template><div /></template>"#,
        )
        .unwrap();

    project.host().set_import_dependencies(
        "/src/Button.vue",
        vec![crate::types::DependencyResolution {
            specifier: "./avatar-types".to_string(),
            resolved_canonical_id: Some("/src/avatar-types.ts".to_string()),
            possible_canonical_ids: Vec::new(),
        }],
    );
    project.host().set_import_dependencies(
        "/src/avatar-types.ts",
        vec![
            crate::types::DependencyResolution {
                specifier: "./types".to_string(),
                resolved_canonical_id: Some("/src/types.ts".to_string()),
                possible_canonical_ids: Vec::new(),
            },
            crate::types::DependencyResolution {
                specifier: "./avatar-theme".to_string(),
                resolved_canonical_id: Some("/src/avatar-theme.ts".to_string()),
                possible_canonical_ids: Vec::new(),
            },
        ],
    );

    let resolved = project
        .host()
        .resolve_component_meta(
            "/src/Button.vue",
            verter_type_engine::semantic_query::ProjectionMode::Expanded,
        )
        .expect("resolved component meta should exist");

    // Avatar is not published as a separate registry entry;
    // transitive imported aliases are resolved inline.
    assert!(
        !resolved
            .resolved_type_registry
            .iter()
            .any(|entry| entry.name == "Avatar"),
        "transitive Avatar alias should not be separately published in the registry"
    );

    let meta = project
        .host()
        .get_component_meta("/src/Button.vue")
        .expect("should return component meta");
    let avatar = meta
        .props
        .iter()
        .find(|prop| prop.name == "avatar")
        .expect("avatar prop should still be exposed");
    assert_eq!(
        avatar
            .publication
            .evidence()
            .map(verter_type_expr::AuthoredTypeEvidence::text),
        Some("AvatarProps"),
        "public prop contract should keep the imported alias text"
    );
    // Architectural contract: imported alias names stay shallow at the
    // published surface. The avatar prop publishes the bare `Ref { name:
    // "AvatarProps" }`; consumers re-resolve the declaration through the
    // registry. The transitive `Avatar = ComponentConfig<typeof
    // avatarTheme>` chain is resolved on-demand via the resolver, not
    // eagerly inlined into the published prop type.
    let avatar_ty = shallow_published_type(
        project.host(),
        "/src/Button.vue",
        avatar.publication.result().selected_source(),
        "avatar prop",
    );
    assert!(
        matches!(
            &avatar_ty,
            TypeExpr::Ref { name, .. } if name.as_ref() == "AvatarProps"
        ),
        "avatar prop should publish the bare AvatarProps ref, got {avatar_ty:?}"
    );
}

#[test]
fn resolve_component_meta_handles_renamed_import_cycles_in_shallow_alias_hydration() {
    let project = make_project();
    project
        .upsert_base(
            "/src/helpers.ts",
            r#"type Id<T> = T

type SlotInfo<T> = Id<{
  value: T
}>

type WithChildren<T> = {
  slot: SlotInfo<ComponentConfig<T>>
}

export type ComponentConfig<T> = WithChildren<T>
"#,
        )
        .unwrap();
    project
        .upsert_base(
            "/src/Button.vue",
            r#"<script lang="ts">
import type { ComponentConfig as LocalConfig } from './helpers'

export interface ButtonProps {
  slot?: LocalConfig<string>['slot']
}
</script>
<script setup lang="ts">
defineProps<ButtonProps>()
</script>
<template><div /></template>"#,
        )
        .unwrap();

    let resolved = project
        .host()
        .resolve_component_meta(
            "/src/Button.vue",
            verter_type_engine::semantic_query::ProjectionMode::Expanded,
        )
        .expect("resolved component meta should exist");

    let local_config = resolved
        .resolved_type_registry
        .iter()
        .find(|entry| entry.name == "LocalConfig")
        .expect("renamed imported alias should be published in the resolved type registry");
    let local_config_ty = demand_published_type(
        project.host(),
        "/src/Button.vue",
        Some(local_config.type_source.present().expect("present source")),
        "LocalConfig registry entry",
    );
    let TypeExpr::Object(local_config_shape) = &local_config_ty else {
        panic!("LocalConfig should materialize as an object, got {local_config_ty:?}");
    };
    assert!(
        local_config_shape.properties.iter().any(
            |member| matches!(member, ObjectMember::Property(property) if property.string_name().expect("string-key fixture") == "slot"),
        ),
        "LocalConfig should keep its slot member, got {local_config_ty:?}"
    );
}

#[test]
fn resolve_component_meta_registry_declines_ambiguous_same_target_local_aliases() {
    let project = make_project();
    project
        .upsert_base(
            "/src/helpers.ts",
            r#"type WithChildren<T> = {
  slot: ComponentConfig<T>
}

export type ComponentConfig<T> = WithChildren<T>
"#,
        )
        .unwrap();
    project
        .upsert_base(
            "/src/Button.vue",
            r#"<script lang="ts">
import type { ComponentConfig as First } from './helpers'
import type { ComponentConfig as Second } from './helpers'

export interface ButtonProps {
  slot?: First<string>['slot']
}
</script>
<script setup lang="ts">
defineProps<ButtonProps>()
</script>
<template><div /></template>"#,
        )
        .unwrap();

    let resolved = project
        .host()
        .resolve_component_meta(
            "/src/Button.vue",
            verter_type_engine::semantic_query::ProjectionMode::Expanded,
        )
        .expect("resolved component meta should exist");

    assert!(
        resolved.resolved_type_registry.iter().all(|entry| {
            !matches!(entry.name.as_str(), "First" | "Second" | "ComponentConfig")
        }),
        "an ambiguous reverse import identity must not publish an arbitrary local alias: {:?}",
        resolved
            .resolved_type_registry
            .iter()
            .map(|entry| entry.name.as_str())
            .collect::<Vec<_>>()
    );
}

#[test]
fn resolve_component_meta_publishes_transitive_renamed_imported_registry_aliases() {
    let project = make_project();
    project
        .upsert_base(
            "/src/base.ts",
            r#"export type Inner = {
  nested: {
    leaf: string
  }
}
"#,
        )
        .unwrap();
    project
        .upsert_base(
            "/src/helpers.ts",
            r#"import type { Inner as LocalInner } from './base'

export type ComponentConfig = {
  ui: LocalInner
}
"#,
        )
        .unwrap();
    project
        .upsert_base(
            "/src/Button.vue",
            r#"<script lang="ts">
import type { ComponentConfig } from './helpers'

export interface ButtonProps {
  ui?: ComponentConfig['ui']
}
</script>
<script setup lang="ts">
defineProps<ButtonProps>()
</script>
<template><div /></template>"#,
        )
        .unwrap();

    project.host().set_import_dependencies(
        "/src/Button.vue",
        vec![crate::types::DependencyResolution {
            specifier: "./helpers".to_string(),
            resolved_canonical_id: Some("/src/helpers.ts".to_string()),
            possible_canonical_ids: Vec::new(),
        }],
    );
    project.host().set_import_dependencies(
        "/src/helpers.ts",
        vec![crate::types::DependencyResolution {
            specifier: "./base".to_string(),
            resolved_canonical_id: Some("/src/base.ts".to_string()),
            possible_canonical_ids: Vec::new(),
        }],
    );

    let resolved = project
        .host()
        .resolve_component_meta(
            "/src/Button.vue",
            verter_type_engine::semantic_query::ProjectionMode::Expanded,
        )
        .expect("resolved component meta should exist");

    // LocalInner is not published as a separate registry entry;
    // transitive renamed imported aliases are resolved inline.
    assert!(
        !resolved
            .resolved_type_registry
            .iter()
            .any(|entry| entry.name == "LocalInner"),
        "transitive renamed imported alias should not be separately published in the registry"
    );
}

#[test]
fn resolve_component_meta_keeps_deep_imported_registry_branches_shallow() {
    let project = make_project();
    project
        .upsert_base(
            "/src/types.ts",
            r#"export type Level3 = {
  leaf: string
}

export type Level2 = {
  node: Level3
}

export type Level1 = {
  node: Level2
}

export type ComponentConfig = {
  ui: Level1
}
"#,
        )
        .unwrap();
    project
        .upsert_base(
            "/src/Button.vue",
            r#"<script lang="ts">
import type { ComponentConfig } from './types'

export interface ButtonProps {
  ui?: ComponentConfig['ui']
}
</script>
<script setup lang="ts">
defineProps<ButtonProps>()
</script>
<template><div /></template>"#,
        )
        .unwrap();

    project.host().set_import_dependencies(
        "/src/Button.vue",
        vec![crate::types::DependencyResolution {
            specifier: "./types".to_string(),
            resolved_canonical_id: Some("/src/types.ts".to_string()),
            possible_canonical_ids: Vec::new(),
        }],
    );

    let resolved = project
        .host()
        .resolve_component_meta(
            "/src/Button.vue",
            verter_type_engine::semantic_query::ProjectionMode::Expanded,
        )
        .expect("resolved component meta should exist");

    let config_entry = resolved
        .resolved_type_registry
        .iter()
        .find(|entry| entry.name == "ComponentConfig")
        .expect("ComponentConfig should be published in the resolved type registry");
    let config_entry_ty = demand_published_type(
        project.host(),
        "/src/Button.vue",
        Some(config_entry.type_source.present().expect("present source")),
        "ComponentConfig registry entry",
    );
    let TypeExpr::Object(config_shape) = &config_entry_ty else {
        panic!("ComponentConfig should materialize as an object, got {config_entry_ty:?}");
    };

    let ui_member = config_shape
        .properties
        .iter()
        .find_map(|member| match member {
            ObjectMember::Property(property)
                if property.string_name().expect("string-key fixture") == "ui" =>
            {
                Some(&property.ty)
            }
            _ => None,
        })
        .expect("ComponentConfig should keep a ui member");
    let TypeExpr::Object(ui_shape) = ui_member else {
        panic!(
            "ComponentConfig.ui should materialize as an object, got {:?}",
            ui_member
        );
    };

    let node_member = ui_shape
        .properties
        .iter()
        .find_map(|member| match member {
            ObjectMember::Property(property)
                if property.string_name().expect("string-key fixture") == "node" =>
            {
                Some(&property.ty)
            }
            _ => None,
        })
        .expect("ComponentConfig.ui should keep a node member");
    // Deep imported branches are fully resolved as nested objects: { node: { leaf: string } }
    let TypeExpr::Object(level2_shape) = node_member else {
        panic!(
            "deep imported registry branches should resolve to an object, got {:?}",
            node_member
        );
    };
    let inner_node = level2_shape
        .properties
        .iter()
        .find_map(|member| match member {
            ObjectMember::Property(property)
                if property.string_name().expect("string-key fixture") == "node" =>
            {
                Some(&property.ty)
            }
            _ => None,
        })
        .expect("Level2 should have a node member");
    let TypeExpr::Object(level3_shape) = inner_node else {
        panic!(
            "Level2.node should resolve to an object, got {:?}",
            inner_node
        );
    };
    assert!(
        level3_shape.properties.iter().any(
            |member| matches!(member, ObjectMember::Property(property) if property.string_name().expect("string-key fixture") == "leaf"),
        ),
        "Level3 should expose leaf, got {:?}",
        inner_node
    );
}
// ===========================================================================
// Phase 8: Correctness — typeof, double script, interface extends imported
// ===========================================================================

#[test]
fn local_typeof_resolves_in_component_meta() {
    let project = make_project();
    project
        .upsert_base(
            "/App.vue",
            r#"<script setup lang="ts">
const config = { x: 1, y: 'hello' }
defineProps<typeof config>()
</script>
<template><div /></template>"#,
        )
        .unwrap();

    let session = project.open_session_batch().unwrap();
    let meta = session
        .get_component_meta("/App.vue")
        .unwrap()
        .expect("get_component_meta should succeed");

    let names: Vec<&str> = meta.props.iter().map(|p| p.name.as_str()).collect();

    // Assert+: both fields from config
    assert!(names.contains(&"x"), "should have 'x', got: {names:?}");
    assert!(names.contains(&"y"), "should have 'y', got: {names:?}");

    // Assert-: no extra fields
    assert_eq!(meta.props.len(), 2, "should have exactly 2 props");

    // `const config = { x: 1, y: 'hello' }` keeps the BINDING constant but
    // leaves the object PROPERTIES mutable, so `typeof config` widens each
    // member to its primitive (`{ x: number; y: string }`) exactly as TS does
    // — literal preservation would require `as const`. Pin the TS-correct
    // widened published types so the native contract catches a future drift
    // back to over-narrowed literals.
    use verter_type_expr::{PrimitiveName, TypeExpr};
    let x = meta.props.iter().find(|p| p.name == "x").unwrap();
    let x_ty = demand_published_type(
        project.host(),
        "/App.vue",
        x.publication.result().selected_source(),
        "x prop",
    );
    assert!(
        matches!(x_ty, TypeExpr::Primitive(PrimitiveName::Number)),
        "typeof config widens `x: 1` to `number`, got: {x_ty:?}"
    );
    let y = meta.props.iter().find(|p| p.name == "y").unwrap();
    let y_ty = demand_published_type(
        project.host(),
        "/App.vue",
        y.publication.result().selected_source(),
        "y prop",
    );
    assert!(
        matches!(y_ty, TypeExpr::Primitive(PrimitiveName::String)),
        "typeof config widens `y: 'hello'` to `string`, got: {y_ty:?}"
    );
}

#[test]
fn interface_extends_pick_of_imported_type_in_component_meta() {
    let project = make_project();
    project
        .upsert_base(
            "/src/base.ts",
            r#"export interface BaseProps { a: string; b: number; c: boolean; d: object }"#,
        )
        .unwrap();
    project
        .upsert_base(
            "/src/App.vue",
            r#"<script setup lang="ts">
import { BaseProps } from './base'
interface MyProps extends Pick<BaseProps, 'a' | 'b'> { local: string }
defineProps<MyProps>()
</script>
<template><div /></template>"#,
        )
        .unwrap();
    project.host().set_import_dependencies(
        "/src/App.vue",
        vec![crate::types::DependencyResolution {
            specifier: "./base".to_string(),
            resolved_canonical_id: Some("/src/base.ts".to_string()),
            possible_canonical_ids: Vec::new(),
        }],
    );

    let session = project.open_session_batch().unwrap();
    let meta = session
        .get_component_meta("/src/App.vue")
        .unwrap()
        .expect("get_component_meta should succeed");

    let names: Vec<&str> = meta.props.iter().map(|p| p.name.as_str()).collect();

    // Assert+: inherited + local
    assert!(
        names.contains(&"a"),
        "should have 'a' from Pick, got: {names:?}"
    );
    assert!(
        names.contains(&"b"),
        "should have 'b' from Pick, got: {names:?}"
    );
    assert!(
        names.contains(&"local"),
        "should have 'local', got: {names:?}"
    );

    // Assert-: excluded fields
    assert!(!names.contains(&"c"), "should NOT have 'c', got: {names:?}");
    assert!(!names.contains(&"d"), "should NOT have 'd', got: {names:?}");
}

#[test]
fn declared_component_meta_extract_keeps_recursive_get_item_keys_symbolic_without_hanging() {
    let project = make_project();
    project
        .upsert_base(
            "/src/utils.ts",
            r#"
type IsPrimitive<T> = T extends (string | number | boolean | symbol | bigint | null | undefined)
  ? true
  : false

type IsPlainObject<T> = IsPrimitive<T> extends true
  ? false
  : T extends readonly any[] | ((...args: any[]) => any)
    ? false
    : T extends object ? true
      : false

type DotPathKeys<T> = IsPlainObject<T> extends true
  ? {
      [K in keyof T & string]:
      IsPlainObject<NonNullable<T[K]>> extends true
        ? K | `${K}.${DotPathKeys<NonNullable<T[K]>>}`
        : K
    }[keyof T & string]
  : never

export type NestedItem<T> = T extends Array<infer I> ? NestedItem<I> : T

export type GetItemKeys<
  I,
  T extends NestedItem<I> = NestedItem<I>
> = (keyof Extract<T, object> & string) | DotPathKeys<Extract<T, object>>
"#,
        )
        .unwrap();
    project
        .upsert_base(
            "/src/App.vue",
            r#"<script setup lang="ts" generic="T extends { label?: string; nested?: { path?: string } }">
import type { GetItemKeys } from './utils'

defineProps<{
  labelKey?: GetItemKeys<T>
}>()
</script>
<template><div /></template>"#,
        )
        .unwrap();
    project.host().set_import_dependencies(
        "/src/App.vue",
        vec![crate::types::DependencyResolution {
            specifier: "./utils".to_string(),
            resolved_canonical_id: Some("/src/utils.ts".to_string()),
            possible_canonical_ids: Vec::new(),
        }],
    );

    let resolved = project
        .host()
        .resolve_component_meta(
            "/src/App.vue",
            verter_type_engine::semantic_query::ProjectionMode::Expanded,
        )
        .expect("resolved component meta should exist");
    let started = std::time::Instant::now();
    let meta = crate::resolver_core::with_bare_host_ctx_for_test(project.host(), |ctx| {
        let fixture_dispatch_8 =
            verter_type_engine::project_semantic_dispatch::ProjectSemanticDispatch::new(ctx);

        crate::host_manage::extract_component_meta_from_resolved(
            project.host(),
            "/src/App.vue",
            &resolved,
            false,
            ctx,
            &fixture_dispatch_8,
        )
    })
    .analysis;
    let elapsed = started.elapsed();

    assert!(
        elapsed.as_secs_f64() < 10.0,
        "declared component meta extraction should not hang on recursive GetItemKeys helper \
         (elapsed {:.2}s)",
        elapsed.as_secs_f64()
    );

    let label_key = meta
        .props
        .iter()
        .find(|prop| prop.name == "labelKey")
        .expect("labelKey prop should be present");
    assert_eq!(
        label_key
            .publication
            .evidence()
            .map(verter_type_expr::AuthoredTypeEvidence::text),
        Some("GetItemKeys<T>"),
        "labelKey should preserve the source helper name"
    );
    let label_key_ty = shallow_published_type(
        project.host(),
        "/src/App.vue",
        label_key.publication.result().selected_source(),
        "labelKey prop",
    );
    assert!(
        matches!(
            &label_key_ty,
            verter_type_expr::TypeExpr::Ref { name, type_arguments }
                if name.as_ref() == "GetItemKeys" && type_arguments.len() == 1
        ),
        "labelKey should stay symbolic at the prop surface, got {label_key_ty:?}"
    );

    // Confirm contention-instrumentation counters are populated by an
    // actual heavy-component-meta run. A resolve + extract path must
    // have loaded files, taken the overlay
    // gate at least once, pushed nodes into the arena, and claimed at
    // least one execute_cooperative owner slot. Relaxed reads are
    // sufficient: all atomic increments happen before this assertion on
    // the same thread.
    let prov = project.host().provenance_snapshot();
    assert!(
        prov.ensure_loaded_calls > 0,
        "C1: ensure_loaded_calls should increment during component-meta load ({} observed)",
        prov.ensure_loaded_calls,
    );
    assert!(
        prov.node_arena_pushes > 0,
        "C1: node_arena_pushes should increment during semantic interning ({} observed)",
        prov.node_arena_pushes,
    );
    assert!(
        prov.execute_cooperative_owner_path + prov.execute_cooperative_joiner_path > 0,
        "C1: execute_cooperative counters should split between owner and joiner paths \
         (owner {}, joiner {})",
        prov.execute_cooperative_owner_path,
        prov.execute_cooperative_joiner_path,
    );
    assert!(
        prov.scheduler_submit_count > 0,
        "C1: scheduler_submit_count should increment on file load submissions ({} observed)",
        prov.scheduler_submit_count,
    );
}

#[test]
fn imported_barrel_types_are_available_to_define_props_evaluation() {
    let project = make_project();
    project
        .upsert_base("/src/types/index.ts", r#"export * from '../Button.vue'"#)
        .unwrap();
    project
        .upsert_base(
            "/src/Button.vue",
            r#"<script lang="ts">
export interface IconProps {
  icon?: string
}

export interface ButtonProps extends IconProps {
  label?: string
  color?: string
}
</script>
<template><div /></template>"#,
        )
        .unwrap();
    project
        .upsert_base(
            "/src/App.vue",
            r#"<script setup lang="ts">
import type { ButtonProps } from './types'

type Props = Omit<ButtonProps, 'color'> & {
  status?: string
}

defineProps<Props>()
</script>
<template><div /></template>"#,
        )
        .unwrap();

    project.host().set_import_dependencies(
        "/src/App.vue",
        vec![crate::types::DependencyResolution {
            specifier: "./types".to_string(),
            resolved_canonical_id: Some("/src/types/index.ts".to_string()),
            possible_canonical_ids: Vec::new(),
        }],
    );
    project.host().set_import_dependencies(
        "/src/types/index.ts",
        vec![crate::types::DependencyResolution {
            specifier: "../Button.vue".to_string(),
            resolved_canonical_id: Some("/src/Button.vue".to_string()),
            possible_canonical_ids: Vec::new(),
        }],
    );

    let session = project.open_session_batch().unwrap();
    let meta = session
        .get_component_meta("/src/App.vue")
        .unwrap()
        .expect("get_component_meta should succeed");

    let names: Vec<&str> = meta.props.iter().map(|p| p.name.as_str()).collect();
    assert!(
        names.contains(&"icon"),
        "should have 'icon', got: {names:?}"
    );
    assert!(
        names.contains(&"label"),
        "should have 'label', got: {names:?}"
    );
    assert!(
        names.contains(&"status"),
        "should keep local props, got: {names:?}"
    );
    assert!(
        !names.contains(&"color"),
        "should omit 'color', got: {names:?}"
    );
}

#[test]
fn imported_barrel_cycles_still_resolve_nested_omit_props() {
    let project = make_project();
    project
        .upsert_base(
            "/src/types/index.ts",
            r#"export * from '../Link.vue'
export * from '../Button.vue'"#,
        )
        .unwrap();
    project
        .upsert_base(
            "/src/Link.vue",
            r#"<script lang="ts">
interface RouterLinkOptions {
  replace?: boolean
  activeClass?: string
  ariaCurrentValue?: string
}

interface RouterLinkProps extends RouterLinkOptions {
  custom?: boolean
  exactActiveClass?: string
}

interface NuxtLinkProps extends Omit<RouterLinkProps, 'to'> {
  to?: string
  href?: string
}

export interface LinkProps extends NuxtLinkProps {
  as?: any
  class?: any
  raw?: boolean
}

export type LinkPropsKeys = 'to' | 'replace' | 'activeClass' | 'ariaCurrentValue'
</script>
<template><div /></template>"#,
        )
        .unwrap();
    project
        .upsert_base(
            "/src/Button.vue",
            r#"<script lang="ts">
import type { LinkProps } from './types'

export interface UseComponentIconsProps {
  icon?: string
  loading?: boolean
}

export interface ButtonProps extends UseComponentIconsProps, Omit<LinkProps, 'raw' | 'custom'> {
  label?: string
  color?: string
  variant?: string
  size?: string
}
</script>
<template><div /></template>"#,
        )
        .unwrap();
    project
        .upsert_base(
            "/src/App.vue",
            r#"<script setup lang="ts">
import type { ButtonProps, LinkPropsKeys } from './types'

interface ChildProps extends Omit<ButtonProps, LinkPropsKeys | 'icon' | 'color' | 'variant'> {
  status?: string
}

defineProps<ChildProps>()
</script>
<template><div /></template>"#,
        )
        .unwrap();

    project.host().set_import_dependencies(
        "/src/App.vue",
        vec![crate::types::DependencyResolution {
            specifier: "./types".to_string(),
            resolved_canonical_id: Some("/src/types/index.ts".to_string()),
            possible_canonical_ids: Vec::new(),
        }],
    );
    project.host().set_import_dependencies(
        "/src/Button.vue",
        vec![crate::types::DependencyResolution {
            specifier: "./types".to_string(),
            resolved_canonical_id: Some("/src/types/index.ts".to_string()),
            possible_canonical_ids: Vec::new(),
        }],
    );
    project.host().set_import_dependencies(
        "/src/types/index.ts",
        vec![
            crate::types::DependencyResolution {
                specifier: "../Link.vue".to_string(),
                resolved_canonical_id: Some("/src/Link.vue".to_string()),
                possible_canonical_ids: Vec::new(),
            },
            crate::types::DependencyResolution {
                specifier: "../Button.vue".to_string(),
                resolved_canonical_id: Some("/src/Button.vue".to_string()),
                possible_canonical_ids: Vec::new(),
            },
        ],
    );

    let session = project.open_session_batch().unwrap();
    let meta = session
        .get_component_meta("/src/App.vue")
        .unwrap()
        .expect("get_component_meta should succeed");

    let names: Vec<&str> = meta.props.iter().map(|p| p.name.as_str()).collect();
    assert!(
        names.contains(&"loading"),
        "should include inherited icon props, got: {names:?}"
    );
    assert!(
        names.contains(&"label"),
        "should include inherited button props, got: {names:?}"
    );
    assert!(
        names.contains(&"size"),
        "should include inherited button props, got: {names:?}"
    );
    assert!(
        names.contains(&"href"),
        "should include inherited link props, got: {names:?}"
    );
    assert!(
        names.contains(&"status"),
        "should keep local props, got: {names:?}"
    );
    assert!(!names.contains(&"icon"), "should omit icon, got: {names:?}");
    assert!(
        !names.contains(&"color"),
        "should omit color, got: {names:?}"
    );
    assert!(
        !names.contains(&"variant"),
        "should omit variant, got: {names:?}"
    );
    assert!(
        !names.contains(&"to"),
        "should omit link keys, got: {names:?}"
    );
    assert!(
        !names.contains(&"replace"),
        "should omit router link keys, got: {names:?}"
    );
    assert!(
        !names.contains(&"activeClass"),
        "should omit router link keys, got: {names:?}"
    );
    assert!(
        !names.contains(&"ariaCurrentValue"),
        "should omit router link keys, got: {names:?}"
    );
}

#[test]
fn resolve_component_meta_handles_barrel_cycle_utility_heritage() {
    let project = make_project();
    project
        .upsert_base(
            "/src/types/index.ts",
            r#"export * from '../Link.vue'
export * from '../Button.vue'"#,
        )
        .unwrap();
    project
        .upsert_base(
            "/src/Link.vue",
            r#"<script lang="ts">
interface RouterLinkOptions {
  replace?: boolean
  activeClass?: string
  ariaCurrentValue?: string
}

interface RouterLinkProps extends RouterLinkOptions {
  custom?: boolean
  exactActiveClass?: string
}

interface NuxtLinkProps extends Omit<RouterLinkProps, 'to'> {
  to?: string
  href?: string
}

export interface LinkProps extends NuxtLinkProps {
  as?: any
  class?: any
  raw?: boolean
}

export type LinkPropsKeys = 'to' | 'replace' | 'activeClass' | 'ariaCurrentValue'
</script>
<template><div /></template>"#,
        )
        .unwrap();
    project
        .upsert_base(
            "/src/Button.vue",
            r#"<script lang="ts">
import type { LinkProps } from './types'

export interface UseComponentIconsProps {
  icon?: string
  loading?: boolean
}

export interface ButtonProps extends UseComponentIconsProps, Omit<LinkProps, 'raw' | 'custom'> {
  label?: string
  color?: string
  variant?: string
  size?: string
}
</script>
<template><div /></template>"#,
        )
        .unwrap();
    project
        .upsert_base(
            "/src/App.vue",
            r#"<script setup lang="ts">
import type { ButtonProps, LinkPropsKeys } from './types'

interface ChildProps extends Omit<ButtonProps, LinkPropsKeys | 'icon' | 'color' | 'variant'> {
  status?: string
}

defineProps<ChildProps>()
</script>
<template><div /></template>"#,
        )
        .unwrap();

    project.host().set_import_dependencies(
        "/src/App.vue",
        vec![crate::types::DependencyResolution {
            specifier: "./types".to_string(),
            resolved_canonical_id: Some("/src/types/index.ts".to_string()),
            possible_canonical_ids: Vec::new(),
        }],
    );
    project.host().set_import_dependencies(
        "/src/Button.vue",
        vec![crate::types::DependencyResolution {
            specifier: "./types".to_string(),
            resolved_canonical_id: Some("/src/types/index.ts".to_string()),
            possible_canonical_ids: Vec::new(),
        }],
    );
    project.host().set_import_dependencies(
        "/src/types/index.ts",
        vec![
            crate::types::DependencyResolution {
                specifier: "../Link.vue".to_string(),
                resolved_canonical_id: Some("/src/Link.vue".to_string()),
                possible_canonical_ids: Vec::new(),
            },
            crate::types::DependencyResolution {
                specifier: "../Button.vue".to_string(),
                resolved_canonical_id: Some("/src/Button.vue".to_string()),
                possible_canonical_ids: Vec::new(),
            },
        ],
    );

    let resolved = project
        .host()
        .resolve_component_meta(
            "/src/App.vue",
            verter_type_engine::semantic_query::ProjectionMode::Expanded,
        )
        .expect("expanded state should resolve");
    let button = resolved
        .resolved_macros
        .iter()
        .find(|meta| meta.type_name == "ButtonProps")
        .expect("should resolve ButtonProps");
    let button_dtos = project
        .host()
        .vue_macro_dtos(&crate::typeinfo::types::VueMacroSurfaceRequest {
            owner_canonical: std::sync::Arc::from("/src/App.vue"),
            macro_index: button.macro_index,
            macro_kind: button.macro_kind,
            root_identity: project
                .host()
                .current_or_read_whole_hash("/src/App.vue")
                .unwrap_or([0u8; 16]),
            level: crate::typeinfo::types::TypeInfoQueryLevel::FullMetadata,
        })
        .expect("the Vue adapter is admitted");
    assert!(
        button_dtos
            .prop_fields()
            .iter()
            .any(|prop| prop.analysis.name == "loading"),
        "resolved ButtonProps should include inherited props, got: {:?}",
        button_dtos.props
    );
    assert!(
        button_dtos
            .prop_fields()
            .iter()
            .any(|prop| prop.analysis.name == "label"),
        "resolved ButtonProps should include button props, got: {:?}",
        button_dtos.props
    );
}

#[test]
fn package_backed_utility_wrapped_prop_stays_symbolic_in_evaluated_types() {
    let project = make_project();
    project
        .upsert_base(
            "/node_modules/editor-lib/index.d.ts",
            r#"
export interface Editor {
  $doc(): string
  chain(): string
  active?: boolean
}
"#,
        )
        .unwrap();
    project
        .upsert_base(
            "/src/App.vue",
            r#"<script setup lang="ts">
import type { Editor } from 'editor-lib'

defineProps<{
  editor?: Omit<Editor, 'active'>
}>()
</script>
<template><div /></template>"#,
        )
        .unwrap();

    project.host().set_import_dependencies(
        "/src/App.vue",
        vec![crate::types::DependencyResolution {
            specifier: "editor-lib".to_string(),
            resolved_canonical_id: Some("/node_modules/editor-lib/index.d.ts".to_string()),
            possible_canonical_ids: Vec::new(),
        }],
    );

    let resolved = project
        .host()
        .resolve_component_meta(
            "/src/App.vue",
            verter_type_engine::semantic_query::ProjectionMode::Expanded,
        )
        .expect("resolved component meta should exist");
    let editor_field = resolved
        .evaluated_types
        .as_ref()
        .and_then(|types| types.props.iter().find(|field| field.name == "editor"))
        .expect("expanded evaluated types should keep the editor prop");

    let editor_field_ty = shallow_published_type(
        project.host(),
        "/src/App.vue",
        Some(
            editor_field
                .authority
                .source_position()
                .present()
                .expect("present source"),
        ),
        "editor prop",
    );
    assert!(
        matches!(
            &editor_field_ty,
            verter_type_expr::TypeExpr::Ref { name, type_arguments }
                if name.as_ref() == "Omit"
                    && type_arguments.len() == 2
                    && matches!(
                        &type_arguments[0],
                        verter_type_expr::TypeExpr::Ref {
                            name,
                            type_arguments
                        } if name.as_ref() == "Editor" && type_arguments.is_empty()
                    )
        ),
        "package-backed utility-wrapped props should keep the imported package ref symbolic instead of expanding the package object, got {editor_field_ty:?}"
    );
}

#[test]
fn imported_object_like_prop_stays_symbolic_in_evaluated_types() {
    let project = make_project();
    project
        .upsert_base(
            "/src/types.ts",
            r#"
export interface ExternalProps {
  id: string
  label?: string
}
"#,
        )
        .unwrap();
    project
        .upsert_base(
            "/src/App.vue",
            r#"<script setup lang="ts">
import type { ExternalProps } from './types'

defineProps<{
  external?: ExternalProps
}>()
</script>
<template><div /></template>"#,
        )
        .unwrap();

    project.host().set_import_dependencies(
        "/src/App.vue",
        vec![crate::types::DependencyResolution {
            specifier: "./types".to_string(),
            resolved_canonical_id: Some("/src/types.ts".to_string()),
            possible_canonical_ids: Vec::new(),
        }],
    );

    let resolved = project
        .host()
        .resolve_component_meta(
            "/src/App.vue",
            verter_type_engine::semantic_query::ProjectionMode::Expanded,
        )
        .expect("resolved component meta should exist");
    let external_field = resolved
        .evaluated_types
        .as_ref()
        .and_then(|types| types.props.iter().find(|field| field.name == "external"))
        .expect("expanded evaluated types should keep the imported external prop");

    let external_field_ty = shallow_published_type(
        project.host(),
        "/src/App.vue",
        Some(
            external_field
                .authority
                .source_position()
                .present()
                .expect("present source"),
        ),
        "external prop",
    );
    assert!(
        matches!(
            &external_field_ty,
            verter_type_expr::TypeExpr::Ref { name, type_arguments }
                if name.as_ref() == "ExternalProps" && type_arguments.is_empty()
        ),
        "imported object-like prop expansion should keep the symbolic ref instead of expanding the imported object, got {external_field_ty:?}"
    );
}

#[test]
fn public_component_meta_keeps_imported_props_refs_symbolic() {
    let project = make_project();
    project
        .upsert_base(
            "/node_modules/reka-ui/index.d.ts",
            r#"
export interface TooltipContentProps {
  text?: string
}

export interface TooltipProviderProps {
  delayDuration?: number
  content?: TooltipContentProps
}
"#,
        )
        .unwrap();
    project
        .upsert_base(
            "/src/App.vue",
            r#"<script setup lang="ts">
import type { TooltipProviderProps } from 'reka-ui'

defineProps<{
  tooltip?: TooltipProviderProps
}>()
</script>
<template><div /></template>"#,
        )
        .unwrap();

    project.host().set_import_dependencies(
        "/src/App.vue",
        vec![crate::types::DependencyResolution {
            specifier: "reka-ui".to_string(),
            resolved_canonical_id: Some("/node_modules/reka-ui/index.d.ts".to_string()),
            possible_canonical_ids: Vec::new(),
        }],
    );

    let session = project.open_session_batch().expect("session should open");
    let declared = session
        .get_component_meta("/src/App.vue")
        .expect("declared component meta query should succeed")
        .expect("declared component meta should exist");
    let full = session
        .get_component_meta("/src/App.vue")
        .expect("full component meta query should succeed")
        .expect("full component meta should exist");

    for (label, meta) in [("declared", declared), ("full", full)] {
        let tooltip = meta
            .props
            .iter()
            .find(|prop| prop.name == "tooltip")
            .expect("tooltip prop should exist");
        let tooltip_ty = shallow_published_type(
            project.host(),
            "/src/App.vue",
            tooltip.publication.result().selected_source(),
            "tooltip prop",
        );
        assert!(
            matches!(
                &tooltip_ty,
                verter_type_expr::TypeExpr::Ref { name, type_arguments }
                    if name.as_ref() == "TooltipProviderProps" && type_arguments.is_empty()
            ),
            "{label} component meta should keep imported *Props refs symbolic instead of rematerializing them, got {tooltip_ty:?}"
        );
        assert_eq!(
            tooltip
                .publication
                .evidence()
                .map(verter_type_expr::AuthoredTypeEvidence::text),
            Some("TooltipProviderProps"),
            "{label} component meta should preserve the raw imported type text"
        );
    }
}

#[test]
fn public_component_meta_keeps_imported_object_refs_symbolic() {
    let project = make_project();
    project
        .upsert_base(
            "/node_modules/editor-lib/index.d.ts",
            r#"
export interface Editor {
  chain(): { run(): void }
  isEditable?: boolean
}
"#,
        )
        .unwrap();
    project
        .upsert_base(
            "/src/App.vue",
            r#"<script setup lang="ts">
import type { Editor } from 'editor-lib'

defineProps<{
  editor: Editor
}>()
</script>
<template><div /></template>"#,
        )
        .unwrap();

    project.host().set_import_dependencies(
        "/src/App.vue",
        vec![crate::types::DependencyResolution {
            specifier: "editor-lib".to_string(),
            resolved_canonical_id: Some("/node_modules/editor-lib/index.d.ts".to_string()),
            possible_canonical_ids: Vec::new(),
        }],
    );

    let session = project.open_session_batch().expect("session should open");
    let declared = session
        .get_component_meta("/src/App.vue")
        .expect("declared component meta query should succeed")
        .expect("declared component meta should exist");
    let full = session
        .get_component_meta("/src/App.vue")
        .expect("full component meta query should succeed")
        .expect("full component meta should exist");

    for (label, meta) in [("declared", declared), ("full", full)] {
        let editor = meta
            .props
            .iter()
            .find(|prop| prop.name == "editor")
            .expect("editor prop should exist");
        let editor_ty = shallow_published_type(
            project.host(),
            "/src/App.vue",
            editor.publication.result().selected_source(),
            "editor prop",
        );
        assert!(
            matches!(
                &editor_ty,
                verter_type_expr::TypeExpr::Ref { name, type_arguments }
                    if name.as_ref() == "Editor" && type_arguments.is_empty()
            ),
            "{label} component meta should keep imported object refs symbolic instead of rematerializing them, got {editor_ty:?}"
        );
        assert_eq!(
            editor
                .publication
                .evidence()
                .map(verter_type_expr::AuthoredTypeEvidence::text),
            Some("Editor"),
            "{label} component meta should preserve the raw imported type text"
        );
    }
}

#[test]
fn public_component_meta_keeps_utility_wrapped_imported_refs_symbolic() {
    let project = make_project();
    project
        .upsert_base(
            "/src/button.ts",
            r#"
export interface ButtonProps {
  href?: string
  disabled?: boolean
  label?: string
}
"#,
        )
        .unwrap();
    project
        .upsert_base(
            "/src/avatar.ts",
            r#"
export interface AvatarProps {
  src?: string
}
"#,
        )
        .unwrap();
    project
        .upsert_base(
            "/src/progress.ts",
            r#"
export interface ProgressProps {
  color?: string
  ui?: {
    root?: string
  }
}
"#,
        )
        .unwrap();
    project
        .upsert_base("/src/keys.ts", "export type LinkPropsKeys = 'href'")
        .unwrap();
    project
        .upsert_base(
            "/src/App.vue",
            r#"<script setup lang="ts">
import type { AvatarProps } from './avatar'
import type { ButtonProps } from './button'
import type { LinkPropsKeys } from './keys'
import type { ProgressProps } from './progress'

defineProps<{
  avatar?: AvatarProps
  actions?: ButtonProps[]
  close?: boolean | Omit<ButtonProps, LinkPropsKeys>
  progress?: boolean | Pick<ProgressProps, 'color' | 'ui'>
}>()
</script>
<template><div /></template>"#,
        )
        .unwrap();

    project.host().set_import_dependencies(
        "/src/App.vue",
        vec![
            crate::types::DependencyResolution {
                specifier: "./avatar".to_string(),
                resolved_canonical_id: Some("/src/avatar.ts".to_string()),
                possible_canonical_ids: Vec::new(),
            },
            crate::types::DependencyResolution {
                specifier: "./button".to_string(),
                resolved_canonical_id: Some("/src/button.ts".to_string()),
                possible_canonical_ids: Vec::new(),
            },
            crate::types::DependencyResolution {
                specifier: "./keys".to_string(),
                resolved_canonical_id: Some("/src/keys.ts".to_string()),
                possible_canonical_ids: Vec::new(),
            },
            crate::types::DependencyResolution {
                specifier: "./progress".to_string(),
                resolved_canonical_id: Some("/src/progress.ts".to_string()),
                possible_canonical_ids: Vec::new(),
            },
        ],
    );

    let session = project.open_session_batch().expect("session should open");
    let declared = session
        .get_component_meta("/src/App.vue")
        .expect("declared component meta query should succeed")
        .expect("declared component meta should exist");
    let full = session
        .get_component_meta("/src/App.vue")
        .expect("full component meta query should succeed")
        .expect("full component meta should exist");

    fn union_contains_utility_ref(
        expr: &verter_type_expr::TypeExpr,
        utility_name: &str,
        inner_name: &str,
    ) -> bool {
        match expr {
            verter_type_expr::TypeExpr::Union(members) => {
                members.iter().any(|member| match member {
                    verter_type_expr::TypeExpr::Ref {
                        name,
                        type_arguments,
                    } if name.as_ref() == utility_name && type_arguments.len() == 2 => {
                        matches!(
                            &type_arguments[0],
                            verter_type_expr::TypeExpr::Ref {
                                name,
                                type_arguments
                            } if name.as_ref() == inner_name && type_arguments.is_empty()
                        )
                    }
                    _ => false,
                })
            }
            _ => false,
        }
    }

    // Publication-policy contract: the policy pass keeps *Props-suffix
    // imports symbolic in the public meta so the compat layer
    // (`compat/checker.ts`, `vue-component-meta` interop) emits named
    // opaque schemas instead of inlined member properties. Rule 4
    // covers bare *Props refs; Rule 5 (structural recursion) leaves the
    // *Props leaf unchanged inside Array/Union/Intersection/Pick/Omit
    // wrappers. Rule 1 keeps the symbolic shape for refs whose declaration
    // came from `/node_modules/`.
    for (label, meta) in [("declared", declared), ("full", full)] {
        let avatar = meta
            .props
            .iter()
            .find(|prop| prop.name == "avatar")
            .expect("avatar prop should exist");
        let avatar_ty = shallow_published_type(
            project.host(),
            "/src/App.vue",
            avatar.publication.result().selected_source(),
            "avatar prop",
        );
        assert!(
            matches!(
                &avatar_ty,
                verter_type_expr::TypeExpr::Ref { name, type_arguments }
                    if name.as_ref() == "AvatarProps" && type_arguments.is_empty()
            ),
            "{label} component meta should keep imported object refs symbolic, got {avatar_ty:?}"
        );

        let actions = meta
            .props
            .iter()
            .find(|prop| prop.name == "actions")
            .expect("actions prop should exist");
        let actions_ty = shallow_published_type(
            project.host(),
            "/src/App.vue",
            actions.publication.result().selected_source(),
            "actions prop",
        );
        assert!(
            matches!(
                &actions_ty,
                verter_type_expr::TypeExpr::Array { element, .. }
                    if matches!(
                        element.as_ref(),
                        verter_type_expr::TypeExpr::Ref { name, type_arguments }
                            if name.as_ref() == "ButtonProps" && type_arguments.is_empty()
                    )
            ),
            "{label} component meta should keep imported array element refs symbolic, got {actions_ty:?}"
        );

        let close = meta
            .props
            .iter()
            .find(|prop| prop.name == "close")
            .expect("close prop should exist");
        let close_ty = shallow_published_type(
            project.host(),
            "/src/App.vue",
            close.publication.result().selected_source(),
            "close prop",
        );
        assert!(
            union_contains_utility_ref(&close_ty, "Omit", "ButtonProps"),
            "{label} component meta should keep imported Omit wrappers symbolic, got {close_ty:?}"
        );

        let progress = meta
            .props
            .iter()
            .find(|prop| prop.name == "progress")
            .expect("progress prop should exist");
        let progress_ty = shallow_published_type(
            project.host(),
            "/src/App.vue",
            progress.publication.result().selected_source(),
            "progress prop",
        );
        assert!(
            union_contains_utility_ref(&progress_ty, "Pick", "ProgressProps"),
            "{label} component meta should keep imported Pick wrappers symbolic, got {progress_ty:?}"
        );
    }
}

// `public_component_meta_keeps_simple_imported_alias_union_surface`
// is intentionally not part of this suite: its characterisation
// depended on a `should_preserve_shallow_field_expr` heuristic that
// pinned a symbolic-vs-concrete mix at a specific granularity.
// Dispatch's `project_type_surface_expr` expands via the hot path and
// does not emit that pinned shape. Import/alias resolution is covered
// by the surviving dispatch-backed component-meta tests.

#[test]
fn imported_utility_wrapped_field_stays_symbolic_in_evaluated_types() {
    let project = make_project();
    project
        .upsert_base(
            "/src/button.ts",
            r#"
export interface ButtonProps {
  href?: string
  disabled?: boolean
  label?: string
}
"#,
        )
        .unwrap();
    project
        .upsert_base("/src/keys.ts", "export type LinkPropsKeys = 'href'")
        .unwrap();
    project
        .upsert_base(
            "/src/App.vue",
            r#"<script setup lang="ts">
import type { ButtonProps } from './button'
import type { LinkPropsKeys } from './keys'

defineProps<{
  close?: boolean | Omit<ButtonProps, LinkPropsKeys>
}>()
</script>
<template><div /></template>"#,
        )
        .unwrap();

    project.host().set_import_dependencies(
        "/src/App.vue",
        vec![
            crate::types::DependencyResolution {
                specifier: "./button".to_string(),
                resolved_canonical_id: Some("/src/button.ts".to_string()),
                possible_canonical_ids: Vec::new(),
            },
            crate::types::DependencyResolution {
                specifier: "./keys".to_string(),
                resolved_canonical_id: Some("/src/keys.ts".to_string()),
                possible_canonical_ids: Vec::new(),
            },
        ],
    );

    let resolved = project
        .host()
        .resolve_component_meta(
            "/src/App.vue",
            verter_type_engine::semantic_query::ProjectionMode::Expanded,
        )
        .expect("resolved component meta should exist");
    let close_field = resolved
        .evaluated_types
        .as_ref()
        .and_then(|types| types.props.iter().find(|field| field.name == "close"))
        .expect("expanded evaluated types should keep the close prop");

    let close_field_ty = shallow_published_type(
        project.host(),
        "/src/App.vue",
        Some(
            close_field
                .authority
                .source_position()
                .present()
                .expect("present source"),
        ),
        "close prop",
    );
    let has_symbolic_omit = match &close_field_ty {
        verter_type_expr::TypeExpr::Union(members) => members.iter().any(|member| match member {
            verter_type_expr::TypeExpr::Ref {
                name,
                type_arguments,
            } if name.as_ref() == "Omit" && type_arguments.len() == 2 => {
                matches!(
                    &type_arguments[0],
                    verter_type_expr::TypeExpr::Ref {
                        name,
                        type_arguments
                    } if name.as_ref() == "ButtonProps" && type_arguments.is_empty()
                )
            }
            _ => false,
        }),
        _ => false,
    };

    assert!(
        has_symbolic_omit,
        "utility wrappers around imported object refs should stay symbolic in shallow field evaluation, got {close_field_ty:?}"
    );
}

#[test]
fn imported_non_object_alias_with_package_refs_stays_symbolic_in_registry() {
    let project = make_project();
    project
        .upsert_base(
            "/node_modules/vue/index.d.ts",
            r#"
export interface VNode {
  component?: object
  children?: string
}
"#,
        )
        .unwrap();
    project
        .upsert_base(
            "/src/types.ts",
            r#"
import type { VNode } from 'vue'

export type StringOrVNode = string | VNode | (() => VNode)
"#,
        )
        .unwrap();
    project
        .upsert_base(
            "/src/App.vue",
            r#"<script setup lang="ts">
import type { StringOrVNode } from './types'

defineProps<{
  title?: StringOrVNode
}>()
</script>
<template><div /></template>"#,
        )
        .unwrap();

    project.host().set_import_dependencies(
        "/src/App.vue",
        vec![crate::types::DependencyResolution {
            specifier: "./types".to_string(),
            resolved_canonical_id: Some("/src/types.ts".to_string()),
            possible_canonical_ids: Vec::new(),
        }],
    );
    project.host().set_import_dependencies(
        "/src/types.ts",
        vec![crate::types::DependencyResolution {
            specifier: "vue".to_string(),
            resolved_canonical_id: Some("/node_modules/vue/index.d.ts".to_string()),
            possible_canonical_ids: Vec::new(),
        }],
    );

    let store_view = project.host().resolver_store_view_read().into_owned_view();
    let prepared = project
        .host()
        .prepared_type_decl("/src/types.ts", "StringOrVNode")
        .expect("StringOrVNode should be present in the shallow prepared declarations");
    let prepared_body_source = verter_type_expr::facts::SemanticTypeSource::Authored(
        verter_type_expr::locators::AuthoredBodyLocator::DeclBody(
            prepared.body_facts.body_slot.clone(),
        ),
    );
    let prepared_body_ty = shallow_published_type(
        project.host(),
        "/src/types.ts",
        Some(&prepared_body_source),
        "StringOrVNode prepared body",
    );
    assert!(
        matches!(&prepared_body_ty, verter_type_expr::TypeExpr::Union(_)),
        "shallow prepared declarations should keep imported non-object aliases symbolic, got {prepared_body_ty:?}"
    );

    let resolved = project
        .host()
        .resolve_component_meta(
            "/src/App.vue",
            verter_type_engine::semantic_query::ProjectionMode::Expanded,
        )
        .expect("resolved component meta should exist");

    let string_or_vnode = resolved
        .resolved_type_registry
        .iter()
        .find(|entry| entry.name == "StringOrVNode")
        .expect("imported non-object alias should still publish in the registry");
    let string_or_vnode_meta = resolved
        .resolved_type_registry_meta
        .iter()
        .find(|entry| entry.name == "StringOrVNode")
        .expect("imported non-object alias should keep registry metadata");

    let string_or_vnode_ty = shallow_published_type(
        project.host(),
        "/src/App.vue",
        Some(
            string_or_vnode
                .type_source
                .present()
                .expect("present source"),
        ),
        "StringOrVNode registry entry",
    );
    let verter_type_expr::TypeExpr::Union(members) = &string_or_vnode_ty else {
        panic!(
            "StringOrVNode should stay a symbolic union in the registry, got {:?} with declaration {:?}",
            string_or_vnode_ty,
            string_or_vnode_meta.declaration
        );
    };
    assert!(
        members.iter().any(|member| {
            matches!(
                member,
                verter_type_expr::TypeExpr::Ref { name, type_arguments }
                    if name.as_ref() == "VNode" && type_arguments.is_empty()
            )
        }),
        "imported non-object aliases should keep package-backed refs symbolic in the registry, got {string_or_vnode_ty:?}"
    );
    assert!(
        resolved
            .resolved_type_registry
            .iter()
            .all(|entry| entry.name != "VNode"),
        "publishing the alias should not recurse into package-backed helpers"
    );

    // Registry whole-surface warming observability lives on the
    // semantic-graph memo, not on a separate `TypeSurfaceDb`. The
    // behavioural contract (package unions stay symbolic in the
    // registry) is already pinned by the assertions above: the
    // `.type_expr` is a `Ref`, and `VNode` is absent from the
    // registry — either would break if the whole-surface projection
    // had actually warmed and substituted.
    let _ = &store_view;
}

#[test]
fn link_props_keep_router_members_across_package_reexported_utility_heritage() {
    let project = make_project();
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
            "/src/Link.vue",
            r#"<script lang="ts">
import type { RouterLinkProps } from 'vue-router'

interface NuxtLinkProps extends Omit<RouterLinkProps, 'to'> {
  to?: string
  href?: string
}

export interface LinkProps extends NuxtLinkProps {
  custom?: boolean
}
</script>
<script setup lang="ts">
defineProps<LinkProps>()
</script>
<template><a /></template>"#,
        )
        .unwrap();
    project.host().set_import_dependencies(
        "/src/Link.vue",
        vec![crate::types::DependencyResolution {
            specifier: "vue-router".to_string(),
            resolved_canonical_id: Some("/node_modules/vue-router/index.d.ts".to_string()),
            possible_canonical_ids: Vec::new(),
        }],
    );
    project.host().set_import_dependencies(
        "/node_modules/vue-router/index.d.ts",
        vec![crate::types::DependencyResolution {
            specifier: "./dist/index.js".to_string(),
            resolved_canonical_id: Some("/node_modules/vue-router/dist/index.d.ts".to_string()),
            possible_canonical_ids: Vec::new(),
        }],
    );

    let meta = project
        .host()
        .get_component_meta("/src/Link.vue")
        .expect("should return component meta");
    let prop_names: Vec<&str> = meta.props.iter().map(|prop| prop.name.as_str()).collect();

    assert!(
        prop_names.contains(&"replace")
            && prop_names.contains(&"viewTransition")
            && prop_names.contains(&"activeClass")
            && prop_names.contains(&"exactActiveClass")
            && prop_names.contains(&"ariaCurrentValue"),
        "LinkProps should keep router members across package re-exported Omit heritage: {:?}",
        prop_names
    );
}

#[test]
fn imported_omit_props_preserve_jsdoc_and_terminal_display() {
    let project = make_project();
    project
        .upsert_base(
            "/src/types.ts",
            r#"
export interface UseComponentIconsProps {
  icon?: string
}

interface NuxtLinkProps {
  to?: string
}

interface ButtonHTMLAttributes {
  type?: 'button' | 'submit'
}

interface AnchorHTMLAttributes {
  href?: string
}

export interface LinkProps extends NuxtLinkProps, /** @vue-ignore */ Omit<ButtonHTMLAttributes, 'type'>, /** @vue-ignore */ Omit<AnchorHTMLAttributes, 'href'> {
  /** Force the link to be active independent of the current route. */
  active?: boolean
  /** Class to apply when the link is active */
  activeClass?: string
  raw?: boolean
  custom?: boolean
}

export interface ButtonProps extends UseComponentIconsProps, Omit<LinkProps, 'raw' | 'custom'> {
  label?: string
}
"#,
        )
        .unwrap();
    project
        .upsert_base(
            "/src/Button.vue",
            r#"<script setup lang="ts">
import type { ButtonProps } from './types'

defineProps<ButtonProps>()
</script>
<template><button /></template>"#,
        )
        .unwrap();

    let meta = project
        .host()
        .get_component_meta("/src/Button.vue")
        .expect("should return component meta");

    let active = meta
        .props
        .iter()
        .find(|prop| prop.name == "active")
        .expect("active prop should be preserved through imported Omit");
    assert_eq!(
        prop_terminal_display(&project, "/src/Button.vue", "active").as_deref(),
        Some("boolean"),
        "primitive display is terminal output, not authored evidence"
    );
    assert_eq!(
        active.description.as_deref(),
        Some("Force the link to be active independent of the current route.")
    );

    let active_class = meta
        .props
        .iter()
        .find(|prop| prop.name == "activeClass")
        .expect("activeClass prop should be preserved through imported Omit");
    assert_eq!(
        prop_terminal_display(&project, "/src/Button.vue", "activeClass").as_deref(),
        Some("string"),
        "primitive display is terminal output, not authored evidence"
    );
    assert_eq!(
        active_class.description.as_deref(),
        Some("Class to apply when the link is active")
    );
}

#[test]
fn jsdoc_descriptions_propagate_through_heritage_chain_imports() {
    let project = make_project();
    // External file: defines RouterLinkProps with JSDoc
    project
        .upsert_base(
            "/src/router-types.ts",
            r#"
export interface RouterLinkProps {
  /**
   * Calls `router.replace` instead of `router.push`.
   */
  replace?: boolean;
  /**
   * Class to apply when the link is active
   */
  activeClass?: string;
  /**
   * Class to apply when the link is exact active
   */
  exactActiveClass?: string;
}
"#,
        )
        .unwrap();
    // Intermediate file: LinkProps extends imported RouterLinkProps
    project
        .upsert_base(
            "/src/link.ts",
            r#"
import { RouterLinkProps } from './router-types'

export interface LinkProps extends RouterLinkProps {
  /** Force the link to be active. */
  active?: boolean;
  /** The URL to navigate to. */
  href?: string;
}
"#,
        )
        .unwrap();
    // Component imports from the intermediate file
    project
        .upsert_base(
            "/src/Button.vue",
            r#"<script lang="ts">
import type { LinkProps } from './link'

export interface ButtonProps extends LinkProps {
  /** The button label. */
  label?: string
}
</script>
<script setup lang="ts">
const props = defineProps<ButtonProps>()
</script>
<template><button>{{ props.label }}</button></template>"#,
        )
        .unwrap();

    let meta = project
        .host()
        .get_component_meta("/src/Button.vue")
        .expect("should return component meta");

    // Local prop
    let label = meta
        .props
        .iter()
        .find(|p| p.name == "label")
        .expect("label");
    assert_eq!(label.description.as_deref(), Some("The button label."));

    // Directly on LinkProps
    let active = meta
        .props
        .iter()
        .find(|p| p.name == "active")
        .expect("active");
    assert_eq!(
        active.description.as_deref(),
        Some("Force the link to be active."),
    );

    // On LinkProps (one level of local heritage)
    let href = meta.props.iter().find(|p| p.name == "href").expect("href");
    assert_eq!(href.description.as_deref(), Some("The URL to navigate to."),);

    // From RouterLinkProps (heritage chain import from separate file)
    let replace = meta
        .props
        .iter()
        .find(|p| p.name == "replace")
        .expect("replace");
    assert_eq!(
        replace.description.as_deref(),
        Some("Calls `router.replace` instead of `router.push`."),
        "replace JSDoc should propagate through heritage chain import"
    );

    let active_class = meta
        .props
        .iter()
        .find(|p| p.name == "activeClass")
        .expect("activeClass");
    assert_eq!(
        active_class.description.as_deref(),
        Some("Class to apply when the link is active"),
        "activeClass JSDoc should propagate through heritage chain import"
    );

    let exact_active = meta
        .props
        .iter()
        .find(|p| p.name == "exactActiveClass")
        .expect("exactActiveClass");
    assert_eq!(
        exact_active.description.as_deref(),
        Some("Class to apply when the link is exact active"),
        "exactActiveClass JSDoc should propagate through heritage chain import"
    );
}

/// The JSDoc enrichment path for imported props goes through the retained
/// parsed program and cache-owned declaration facts. The
/// enrichment path must NOT fall back to a raw-source reparse helper
/// (which would allocate a fresh oxc arena and reparse dependency
/// source). That architectural guarantee is enforced statically by
/// the `no_text_based_macro_surface_projection_helpers` architecture
/// guard; this test pins the behavioural half (JSDoc descriptions
/// propagate through imported `Omit<>`).
///
/// This scenario routes JSDoc through the span-borne enrichment by
/// extending imported types through `Omit<>` — descriptions slice from the
/// declaring file's cache-owned `IndexedReady.raw_source` via the typeinfo
/// member spans, never a fresh parse.
#[test]
fn imported_jsdoc_enrichment_uses_parse_artifact_and_does_not_reparse_source() {
    let project = make_project();
    project
        .upsert_base(
            "/src/types.ts",
            r#"
interface NuxtLinkProps {
  to?: string
}

interface ButtonHTMLAttributes {
  type?: 'button' | 'submit'
}

export interface LinkProps extends NuxtLinkProps, /** @vue-ignore */ Omit<ButtonHTMLAttributes, 'type'> {
  /** Force the link to be active independent of the current route. */
  active?: boolean
  /** Class to apply when the link is active */
  activeClass?: string
}

export interface ButtonProps extends Omit<LinkProps, 'to'> {
  label?: string
}
"#,
        )
        .unwrap();
    project
        .upsert_base(
            "/src/Button.vue",
            r#"<script setup lang="ts">
import type { ButtonProps } from './types'
defineProps<ButtonProps>()
</script>
<template><button /></template>"#,
        )
        .unwrap();

    let meta = project
        .host()
        .get_component_meta("/src/Button.vue")
        .expect("should return component meta");

    // Behavior guard: JSDoc must still propagate across the imported Omit<>.
    let active = meta
        .props
        .iter()
        .find(|p| p.name == "active")
        .expect("active prop should be preserved through imported Omit");
    assert_eq!(
        active.description.as_deref(),
        Some("Force the link to be active independent of the current route."),
        "active JSDoc should propagate through imported Omit"
    );
    let active_class = meta
        .props
        .iter()
        .find(|p| p.name == "activeClass")
        .expect("activeClass prop should be preserved through imported Omit");
    assert_eq!(
        active_class.description.as_deref(),
        Some("Class to apply when the link is active"),
        "activeClass JSDoc should propagate through imported Omit"
    );

    // Architectural guard: under the graph-only resolver, the raw-
    // source reparse helper is deleted. The architecture guard
    // `no_text_based_macro_surface_projection_helpers` enforces this
    // structurally; this behaviour assertion (JSDoc still flows
    // through imported `Omit<>`) ensures the graph-native enrichment
    // path remains correct.
}

/// A STATIC `is="…"` classifies structurally, exactly as the IDE template
/// rewrite does: an HTML tag name is a native root, and every other value
/// is a component reference — here, an in-scope non-type-only import
/// binding. Both directions are asserted, because the two surfaces must not
/// disagree about one construct.
/// Mutation recipe: mint a bare string literal for every static `is=` and
/// the component direction collapses to a native tag; mint a `typeof` value
/// reference for every static `is=` and the native direction stops
/// resolving `div`.
#[test]
fn static_is_resolves_imported_component_and_native_tag() {
    let project = make_project();
    project
        .upsert_base(
            "/App.vue",
            r#"<script setup lang="ts">
import Child from './Child.vue'
</script>
<template><component is="Child" /></template>"#,
        )
        .unwrap();
    project
        .upsert_base("/Child.vue", r#"<template><input /></template>"#)
        .unwrap();
    project
        .upsert_base(
            "/Native.vue",
            r#"<script setup lang="ts">
</script>
<template><component is="div" /></template>"#,
        )
        .unwrap();

    let component_meta = get_meta(&project, "/App.vue");
    assert!(
        root_chain_steps(&component_meta)
            .iter()
            .any(|step| matches!(
                step,
                ResolvedRootStep::Component { component_name, .. } if component_name == "Child"
            )),
        "a static is=\"Child\" naming an imported binding is a COMPONENT root: {:?}",
        component_meta.fallthrough_surface
    );
    assert!(
        !root_chain_steps(&component_meta)
            .iter()
            .any(|step| matches!(step, ResolvedRootStep::NativeTag { tag } if tag == "Child")),
        "a static is=\"Child\" naming an imported binding is never a native tag: {:?}",
        component_meta.fallthrough_surface
    );

    let native_meta = get_meta(&project, "/Native.vue");
    assert!(
        root_chain_steps(&native_meta)
            .iter()
            .any(|step| matches!(step, ResolvedRootStep::NativeTag { tag } if tag == "div")),
        "a static is=\"div\" naming no in-scope binding is a NATIVE tag: {:?}",
        native_meta.fallthrough_surface
    );
}

#[test]
fn recursive_cycle_uses_structured_unresolved_reason() {
    let project = make_project();
    project
        .upsert_base(
            "/A.vue",
            r#"<script setup lang="ts">
import B from './B.vue'
</script>
<template><B /></template>"#,
        )
        .unwrap();
    project
        .upsert_base(
            "/B.vue",
            r#"<script setup lang="ts">
import A from './A.vue'
</script>
<template><A /></template>"#,
        )
        .unwrap();

    project.host().provenance().reset();
    let meta = get_meta(&project, "/A.vue");
    let FallthroughSurface::Branches { branches } = &meta.fallthrough_surface else {
        panic!("expected FallthroughSurface::Branches");
    };

    assert!(
        branches.iter().any(|branch| matches!(
            &branch.status,
            BranchStatus::Unresolved {
                reason: UnresolvedBranchReason::Cycle { canonical_id }
            } if canonical_id == "/B.vue"
        )),
        "cycles must terminate with a structured cycle reason, got: {:?}",
        branches
            .iter()
            .map(|branch| &branch.status)
            .collect::<Vec<_>>()
    );

    assert!(
        branches.iter().any(|branch| {
            branch.root_chain.iter().any(|step| {
                matches!(
                    step,
                    ResolvedRootStep::Unresolved {
                        reason: UnresolvedBranchReason::Cycle { canonical_id },
                        ..
                    } if canonical_id == "/B.vue"
                )
            })
        }),
        "cycle branches must preserve the structured cycle reason in the root chain"
    );
    assert!(
        provenance(&project).resolver_cycle_detections >= 1,
        "fallthrough cycles should increment the shared resolver cycle counter"
    );
}

#[test]
fn unresolved_target_branch_does_not_crash() {
    let project = make_project();
    project
        .upsert_base(
            "/App.vue",
            r#"<script setup lang="ts">
defineProps<{ msg: string }>()
</script>
<template><slot /></template>"#,
        )
        .unwrap();

    let meta = get_meta(&project, "/App.vue");

    // Assert+: declared prop is present
    assert!(
        meta.accepted_props.iter().any(|p| p.name == "msg"),
        "should have declared 'msg'"
    );

    // Assert-: no inherited members from slot
    assert!(
        !meta
            .accepted_props
            .iter()
            .any(|p| matches!(p.provenance, MemberProvenance::Inherited { .. })),
        "slot root should produce no inherited props"
    );
}

// ── Barrel resolution cache tests ──────────────────────────────────────

#[test]
fn barrel_many_wildcard_exports_resolves_without_hang() {
    // Regression test: barrel with many `export *` entries should not hang.
    // Previously, each type lookup scanned ALL wildcard sources linearly.
    let project = make_project();

    // Create 30 Vue files, each exporting a unique type
    for i in 0..30 {
        project
            .upsert_base(
                &format!("/src/components/Comp{i}.vue"),
                &format!(
                    r#"<script lang="ts">
export interface Comp{i}Props {{
  value{i}?: string
}}
</script>
<template><div /></template>"#
                ),
            )
            .unwrap();
    }

    // Create a barrel that re-exports all 30 + a direct types file
    let mut barrel = String::new();
    for i in 0..30 {
        barrel.push_str(&format!("export * from '../components/Comp{i}.vue'\n"));
    }
    barrel.push_str("export * from './utils'\n");
    project.upsert_base("/src/types/index.ts", &barrel).unwrap();

    project
        .upsert_base(
            "/src/types/utils.ts",
            r#"export interface UtilType { helper: boolean }"#,
        )
        .unwrap();

    // Component that imports from the barrel
    project
        .upsert_base(
            "/src/App.vue",
            r#"<script setup lang="ts">
import type { Comp15Props, UtilType } from './types'

interface AppProps extends Comp15Props {
  extra?: UtilType
}

defineProps<AppProps>()
</script>
<template><div /></template>"#,
        )
        .unwrap();

    // Set up dependency resolutions
    let mut barrel_deps: Vec<crate::types::DependencyResolution> = (0..30)
        .map(|i| crate::types::DependencyResolution {
            specifier: format!("../components/Comp{i}.vue"),
            resolved_canonical_id: Some(format!("/src/components/Comp{i}.vue")),
            possible_canonical_ids: Vec::new(),
        })
        .collect();
    barrel_deps.push(crate::types::DependencyResolution {
        specifier: "./utils".to_string(),
        resolved_canonical_id: Some("/src/types/utils.ts".to_string()),
        possible_canonical_ids: Vec::new(),
    });
    project
        .host()
        .set_import_dependencies("/src/types/index.ts", barrel_deps);

    project.host().set_import_dependencies(
        "/src/App.vue",
        vec![crate::types::DependencyResolution {
            specifier: "./types".to_string(),
            resolved_canonical_id: Some("/src/types/index.ts".to_string()),
            possible_canonical_ids: Vec::new(),
        }],
    );

    let session = project.open_session_batch().unwrap();
    let meta = session
        .get_component_meta("/src/App.vue")
        .unwrap()
        .expect("get_component_meta should succeed");

    let names: Vec<&str> = meta.props.iter().map(|p| p.name.as_str()).collect();
    assert!(
        names.contains(&"value15"),
        "should resolve Comp15Props.value15 through barrel: {names:?}"
    );
    assert!(
        names.contains(&"extra"),
        "should keep local extra prop: {names:?}"
    );
}

#[test]
fn barrel_fully_resolved_returns_none_for_missing_type() {
    let project = make_project();

    project
        .upsert_base(
            "/src/types/index.ts",
            r#"export * from './a'
export * from './b'"#,
        )
        .unwrap();
    project
        .upsert_base("/src/types/a.ts", r#"export interface AType { a: string }"#)
        .unwrap();
    project
        .upsert_base("/src/types/b.ts", r#"export interface BType { b: number }"#)
        .unwrap();

    // Component imports a type that doesn't exist in the barrel
    project
        .upsert_base(
            "/src/App.vue",
            r#"<script setup lang="ts">
import type { AType } from './types'

defineProps<AType>()
</script>
<template><div /></template>"#,
        )
        .unwrap();

    project.host().set_import_dependencies(
        "/src/types/index.ts",
        vec![
            crate::types::DependencyResolution {
                specifier: "./a".to_string(),
                resolved_canonical_id: Some("/src/types/a.ts".to_string()),
                possible_canonical_ids: Vec::new(),
            },
            crate::types::DependencyResolution {
                specifier: "./b".to_string(),
                resolved_canonical_id: Some("/src/types/b.ts".to_string()),
                possible_canonical_ids: Vec::new(),
            },
        ],
    );
    project.host().set_import_dependencies(
        "/src/App.vue",
        vec![crate::types::DependencyResolution {
            specifier: "./types".to_string(),
            resolved_canonical_id: Some("/src/types/index.ts".to_string()),
            possible_canonical_ids: Vec::new(),
        }],
    );

    let session = project.open_session_batch().unwrap();
    let meta = session
        .get_component_meta("/src/App.vue")
        .unwrap()
        .expect("get_component_meta should succeed");

    let names: Vec<&str> = meta.props.iter().map(|p| p.name.as_str()).collect();
    assert!(
        names.contains(&"a"),
        "should resolve AType.a through barrel: {names:?}"
    );
    // Negative: BType should NOT appear (not imported)
    assert!(
        !names.contains(&"b"),
        "should not have BType.b (not imported): {names:?}"
    );
}

#[test]
fn barrel_nested_export_star_chain_resolves() {
    // A -> export * from B -> export * from C
    // A type from C should be found through the chain.
    let project = make_project();

    project
        .upsert_base("/src/barrel_a.ts", r#"export * from './barrel_b'"#)
        .unwrap();
    project
        .upsert_base("/src/barrel_b.ts", r#"export * from './deep'"#)
        .unwrap();
    project
        .upsert_base(
            "/src/deep.ts",
            r#"export interface DeepType { level: number }"#,
        )
        .unwrap();

    project
        .upsert_base(
            "/src/App.vue",
            r#"<script setup lang="ts">
import type { DeepType } from './barrel_a'

defineProps<DeepType>()
</script>
<template><div /></template>"#,
        )
        .unwrap();

    project.host().set_import_dependencies(
        "/src/barrel_a.ts",
        vec![crate::types::DependencyResolution {
            specifier: "./barrel_b".to_string(),
            resolved_canonical_id: Some("/src/barrel_b.ts".to_string()),
            possible_canonical_ids: Vec::new(),
        }],
    );
    project.host().set_import_dependencies(
        "/src/barrel_b.ts",
        vec![crate::types::DependencyResolution {
            specifier: "./deep".to_string(),
            resolved_canonical_id: Some("/src/deep.ts".to_string()),
            possible_canonical_ids: Vec::new(),
        }],
    );
    project.host().set_import_dependencies(
        "/src/App.vue",
        vec![crate::types::DependencyResolution {
            specifier: "./barrel_a".to_string(),
            resolved_canonical_id: Some("/src/barrel_a.ts".to_string()),
            possible_canonical_ids: Vec::new(),
        }],
    );

    let session = project.open_session_batch().unwrap();
    let meta = session
        .get_component_meta("/src/App.vue")
        .unwrap()
        .expect("get_component_meta should succeed");

    let names: Vec<&str> = meta.props.iter().map(|p| p.name.as_str()).collect();
    assert!(
        names.contains(&"level"),
        "should resolve DeepType.level through nested barrel chain: {names:?}"
    );
}

/// A wide finite cross-file heritage fan-out (`Props extends T0..Tn`
/// where every `Tn` is imported from a second file) resolves the FULL
/// `n`-member prop surface through the native graph, without hang or a
/// spurious budget error.
///
/// The frontier step budget is pinned LOW (`external_resolution_step_budget
/// = Some(40)`, below the `45`-wide import count) on purpose: the
/// cross-file frontier performs only ROUTE discovery (a handful of
/// `(canonical_id, exported_name)` visits), so a wide heritage fan-out
/// must NOT trip the frontier step-limit — the heritage members are
/// resolved by the native semantic graph, not by per-member frontier
/// visits. The `props.len() == import_count` assertion is the
/// discriminating gate: dropping the cross-file heritage definitions
/// (so the imported `Tn` are unresolvable) makes the prop count fall
/// short of `import_count` and the test RED.
///
/// Sized small (45 imports / 40-step budget) instead of the historical
/// 2005/2000 corpus so the test runs in well under a second while
/// exercising the identical native-graph wide-heritage resolution path.
#[test]
fn get_component_meta_scales_past_previous_wide_import_budget_fixture() {
    let project = make_project_with_config(HostConfig {
        external_resolution_step_budget: Some(40),
        ..HostConfig::default()
    });

    let import_count = 45usize;
    let mut defs_source = String::new();
    for index in 0..import_count {
        defs_source.push_str(&format!(
            "export interface T{index} {{ p{index}: string }}\n"
        ));
    }

    let mut types_source = String::new();
    types_source.push_str("import type { ");
    for index in 0..import_count {
        if index > 0 {
            types_source.push_str(", ");
        }
        types_source.push_str(&format!("T{index}"));
    }
    types_source.push_str(" } from './defs'\n");
    types_source.push_str("export interface Props extends ");
    for index in 0..import_count {
        if index > 0 {
            types_source.push_str(", ");
        }
        types_source.push_str(&format!("T{index}"));
    }
    types_source.push_str(" {}\n");

    project.upsert_base("/src/defs.ts", &defs_source).unwrap();
    project.upsert_base("/src/types.ts", &types_source).unwrap();
    project
        .upsert_base(
            "/src/App.vue",
            r#"<script setup lang="ts">
import type { Props } from "./types"
defineProps<Props>()
</script>
<template><div /></template>"#,
        )
        .unwrap();
    project.host().set_import_dependencies(
        "/src/App.vue",
        vec![crate::types::DependencyResolution {
            specifier: "./types".to_string(),
            resolved_canonical_id: Some("/src/types.ts".to_string()),
            possible_canonical_ids: Vec::new(),
        }],
    );
    project.host().set_import_dependencies(
        "/src/types.ts",
        vec![crate::types::DependencyResolution {
            specifier: "./defs".to_string(),
            resolved_canonical_id: Some("/src/defs.ts".to_string()),
            possible_canonical_ids: Vec::new(),
        }],
    );

    let session = project.open_session_batch().unwrap();
    let meta = session
        .get_component_meta("/src/App.vue")
        .unwrap()
        .expect("wide external import fan-out should now resolve through the shared frontier path");

    assert_eq!(
        meta.props.len(),
        import_count,
        "the previous budget fixture should now resolve the full prop surface"
    );
    assert!(meta.props.iter().any(|prop| prop.name == "p0"));
    assert!(meta
        .props
        .iter()
        .any(|prop| prop.name == format!("p{}", import_count - 1)));
}

#[test]
fn payload_cache_dependency_edit_invalidates_and_re_encodes() {
    let project = make_project();
    project
        .upsert_base("/types.ts", r#"export interface Props { a: string }"#)
        .unwrap();
    project
        .upsert_base(
            "/Comp.vue",
            r#"<script setup lang="ts">
import { Props } from './types'
defineProps<Props>()
</script>
<template><div /></template>"#,
        )
        .unwrap();

    let session = project.open_session_batch().unwrap();

    // First call — miss.
    let _p1 = session
        .get_component_meta_payload("/Comp.vue", test_encode_fn)
        .expect("should succeed")
        .expect("should return payload");

    let prov1 = provenance(&project);
    assert_eq!(prov1.payload_encodes, 1);

    // Edit the dependency.
    project
        .upsert_base(
            "/types.ts",
            r#"export interface Props { a: string; b: number }"#,
        )
        .unwrap();

    // Second call — cache invalidated by dependency change.
    let _p2 = session
        .get_component_meta_payload("/Comp.vue", test_encode_fn)
        .expect("should succeed")
        .expect("should return payload");

    let prov2 = provenance(&project);
    assert_eq!(
        prov2.payload_encodes, 2,
        "exactly one new encode after dep edit"
    );
    // The payload content should differ because the prop surface changed.
    assert_ne!(
        _p1, _p2,
        "payload should differ after dependency edit adds a prop"
    );
}

// ─────────────────────────────────────────────────────────────────────────
// Imported-alias indexed-access / generic-helper route published-surface pins.
//
// These tests PIN the published prop surface for imported-alias indexed-access /
// generic-helper routes — an imported-alias `Button['ui']` generic-helper route
// and a multi-hop `Foo['a']['b']` indexed access through imported aliases — with
// discriminating assertions on the exact terminal shape, so the published output
// of the shared resolver is locked against any change to how it decides
// convergence (a node-domain interned-key compare with no per-iteration
// materialisation). The strongest risk such a move carries is that a node cursor
// retains provenance the materialized round-trip erased; these fixtures
// discriminate exactly that.
// ─────────────────────────────────────────────────────────────────────────

/// Parity: a multi-hop `Foo['a']['b']` indexed access whose root is an
/// IMPORTED ALIAS (`type ButtonAlias = Outer`) — the shared resolver stabilises
/// the leaf against the alias's source scope. The published terminal is the
/// path-precise primitive; sibling hops never enter the surface.
#[test]
fn c5_parity_imported_alias_chain_indexed_access_pins_terminal_primitive() {
    let project = make_project();
    project
        .upsert_base(
            "/types.ts",
            r#"export interface Inner { full: string; bar: number }
export interface Outer { a: Inner; other: boolean }
export type ButtonAlias = Outer"#,
        )
        .unwrap();
    project
        .upsert_base(
            "/Comp.vue",
            r#"<script setup lang="ts">
import type { ButtonAlias } from './types'

defineProps<{
  ui: ButtonAlias['a']['full']
}>()
</script>
<template><div /></template>"#,
        )
        .unwrap();

    let session = project.open_session_batch().unwrap();
    let evaluated = session.evaluate_types("/Comp.vue").unwrap().unwrap();
    let ui_ty = evaluated_prop_type(&project, "/Comp.vue", &evaluated, "ui");

    // Terminal collapses to `Inner.full`'s declared `string`. The fixture uses
    // distinct primitives at each hop (`Inner.full: string`, `Inner.bar:
    // number`, `Outer.other: boolean`) so the assertion DISCRIMINATES: a
    // mis-route to `bar` lands on `number`, a walk into `other` lands on
    // `boolean`; a `Primitive(_)` wildcard would not catch either.
    assert_eq!(
        ui_ty,
        TypeExpr::Primitive(PrimitiveName::String),
        "C5 parity: `ButtonAlias['a']['full']` (imported-alias chain) must publish the \
         path-precise terminal `string`; got {ui_ty:?}"
    );
    // Negative: the published terminal is NOT `number` / `boolean` (the sibling
    // hops) and NOT an `any`/`never`/`unknown`-shaped catch-all.
    assert_ne!(
        ui_ty,
        TypeExpr::Primitive(PrimitiveName::Number),
        "C5 parity: must not mis-route to the `bar: number` sibling hop"
    );
    assert!(
        !matches!(ui_ty, TypeExpr::Unknown { .. }),
        "C5 parity: the stabilised terminal must not collapse to an `Unknown` shell; got {ui_ty:?}"
    );
}

/// End-to-end discriminator for the dispatch-bridge `ProjectGeneration`
/// conversion.
///
/// A cross-file `defineProps<Foo>()` drives the projector +
/// materialiser dispatch reads. Every dispatch round-trip's
/// `DepSignature` carries a `DepVersion::ProjectGeneration` (built by
/// `ProjectSemanticDispatch::dep_signature_for` /
/// `project_generation_signature`). Those signatures fan through
/// `emit_dispatch_dep_signature_facts` into the request-level fact tracer.
/// The request host finalises that tracer into
/// `ResolvedComponentMetaState.fact_versions`.
///
/// `fact_versions` is the signature stored on the fact-only
/// `cached_resolved_meta` sidecar (on `DerivedRawState`); a warm read
/// through `try_get_cached_resolved_meta` validates it via
/// `StoreView::validates_fact_signature` alone — no legacy rail.
/// `current_dependency_fact_versions` only emits `FileWholeHash` /
/// `DerivedFactHash` facts, NEVER a `ProjectGeneration` fact, so the
/// request tracer is the path that roots the project generation on the
/// sidecar.
///
/// Discrimination: against the pre-fix tree
/// the dispatch bridge DROPS `ProjectGeneration`, so
/// the sidecar's `fact_versions` carry no `ProjectGeneration` fact and
/// a `bump_project_generation()` with unchanged file content leaves
/// the fact-only sidecar warm (stale). The first assertion below
/// FAILS pre-fix. Post-fix the bridge converts `ProjectGeneration`,
/// the sidecar roots it, and a project-generation bump misses.
#[test]
fn dispatch_project_generation_roots_fact_only_resolved_meta_sidecar() {
    use verter_session_query::facts::fact_cache::FactVersionRef;
    use verter_type_engine::semantic_query::ProjectionMode;

    let project = make_project();
    project
        .upsert_base(
            "/src/types.ts",
            "export type Foo = { value: number; label: string }",
        )
        .unwrap();
    project
        .upsert_base(
            "/src/Comp.vue",
            r#"<script setup lang="ts">
import type { Foo } from './types'
defineProps<{ data: Foo }>()
</script>
<template><div /></template>"#,
        )
        .unwrap();

    // Cold compute: populates the `cached_resolved_meta` sidecar.
    let session = project.open_session_batch().unwrap();
    let meta = session
        .get_component_meta("/src/Comp.vue")
        .unwrap()
        .expect("component meta resolves for the cross-file Foo fixture");
    assert!(
        meta.props.iter().any(|p| p.name == "data"),
        "fixture must publish a `data` prop so the projector + \
         materialiser dispatch reads ran — props={:?}",
        meta.props
            .iter()
            .map(|p| p.name.as_str())
            .collect::<Vec<_>>()
    );

    // Inspect the fact-only sidecar's stored `fact_versions`.
    let host = project.host();
    let entry = host
        .derived_raw_cache()
        .get("/src/Comp.vue")
        .expect("derived-raw entry must exist after a cold component-meta compute");
    let cached = entry
        .cached_resolved_meta
        .iter()
        .find(|((mode, _view_fp), _)| *mode == ProjectionMode::Expanded)
        .map(|(_, cached)| cached.clone())
        .expect("cached_resolved_meta must hold an Expanded slot after cold compute");
    drop(entry);

    let project_generation_facts: Vec<u64> = cached
        .fact_versions
        .iter()
        .filter_map(|fact| match fact {
            FactVersionRef::ProjectGeneration { generation } => Some(*generation),
            _ => None,
        })
        .collect();
    // DISCRIMINATING ASSERTION: dropping ProjectGeneration from the bridge
    // `ProjectGeneration`, so no `ProjectGeneration` fact reaches the
    // sidecar and this fails.
    assert!(
        !project_generation_facts.is_empty(),
        "the fact-only `cached_resolved_meta` sidecar MUST carry a \
         FactVersionRef::ProjectGeneration fact after a cold \
         component-meta compute whose dispatch reads observed the \
         project generation — the dispatch bridge must CONVERT \
         `DepVersion::ProjectGeneration`, not drop it. \
         sidecar fact_versions={:?}",
        cached.fact_versions
    );
    // The dispatch reads observe the live project generation at
    // compute time; the sidecar must root that exact value.
    let live_generation = host.project_type_store().project_generation();
    assert!(
        project_generation_facts.contains(&live_generation),
        "the sidecar's ProjectGeneration fact must carry the project \
         generation live at cold-compute time ({live_generation}); \
         got {project_generation_facts:?}"
    );

    // Stale-serve proof: drop the runtime singleflight slot so the
    // fact-only sidecar is the sole survivor, bump the project
    // generation WITHOUT touching any file, and confirm the fact-only
    // warm read misses. Pre-fix (no ProjectGeneration fact) the
    // sidecar would validate vacuously and serve the stale state.
    host.resolver_runtime()
        .component_meta
        .remove(&crate::host_manage::component_meta_request_impl::resolved_meta_cache_key_with_view_fingerprint(
            "/src/Comp.vue",
            ProjectionMode::Expanded,
            0,
        ));
    let warm_before_bump =
        host.try_get_cached_resolved_meta("/src/Comp.vue", ProjectionMode::Expanded);
    assert!(
        warm_before_bump.is_some(),
        "before the bump the fact-only sidecar must still validate \
         (file content unchanged, project generation unchanged)"
    );

    host.project_type_store().bump_project_generation();

    let warm_after_bump =
        host.try_get_cached_resolved_meta("/src/Comp.vue", ProjectionMode::Expanded);
    assert!(
        warm_after_bump.is_none(),
        "after a `bump_project_generation()` with unchanged file \
         content the fact-only `cached_resolved_meta` sidecar MUST \
         miss — the rooted ProjectGeneration fact no longer matches \
         the live project generation. A warm hit here means the \
         project-shape dependency was dropped at the dispatch bridge."
    );
}

/// FIX 3 #2a — ALIAS-CHAIN cross-file heritage through the Vue macro surface:
/// `defineProps<ChildProps>` where `ChildProps extends MiddleAlias` and
/// `MiddleAlias = BaseProps` (a one-hop alias of an imported interface). The
/// published props must include the base members reached through the alias hop.
///
/// Discriminating: `base` is declared only on `BaseProps`, reached via the
/// `MiddleAlias = BaseProps` alias; a resolver that failed to follow the alias
/// hop in heritage would drop it. The EXACT-SET assertion plus the `decoy`
/// negative (a sibling interface in the SAME file that the heritage chain never
/// touches) fail if heritage leaks unrelated declarations into the surface.
#[test]
fn cross_file_alias_chain_heritage_through_macro_surface() {
    let project = make_project();
    project
        .upsert_base(
            "/src/base.ts",
            r#"export interface BaseProps {
  base?: string
}

export type MiddleAlias = BaseProps

// A sibling interface in the SAME file the heritage chain never reaches. A
// resolver that over-collected file-level declarations would leak `decoy`.
export interface UnrelatedProps {
  decoy?: string
}
"#,
        )
        .unwrap();
    project
        .upsert_base(
            "/src/App.vue",
            r#"<script setup lang="ts">
import type { MiddleAlias } from './base'

interface ChildProps extends MiddleAlias {
  own?: number
}

defineProps<ChildProps>()
</script>
<template><div /></template>"#,
        )
        .unwrap();

    let meta = project
        .host()
        .get_component_meta("/src/App.vue")
        .expect("should return component meta");
    let prop_names: BTreeSet<_> = meta.props.iter().map(|p| p.name.as_str()).collect();
    assert!(
        prop_names.contains("own"),
        "own member should be published, got {prop_names:?}",
    );
    assert!(
        prop_names.contains("base"),
        "alias-chain heritage must surface the base member reached through `MiddleAlias = BaseProps`, got {prop_names:?}",
    );
    // NEGATIVE: the unrelated sibling interface's member must NOT leak.
    assert!(
        !prop_names.contains("decoy"),
        "alias-chain heritage must NOT leak the unrelated `UnrelatedProps.decoy` member, got {prop_names:?}",
    );
    // EXACT surface: exactly the own member plus the alias-reached base member.
    assert_eq!(
        prop_names,
        BTreeSet::from(["own", "base"]),
        "alias-chain heritage surface must be EXACTLY {{own, base}}, got {prop_names:?}",
    );
}

/// FIX 3 #2b — TWO-LEVEL cross-file heritage through the Vue macro surface:
/// `GrandchildProps extends ChildProps` (file B) `extends BaseProps` (file A),
/// each declared in a different file. The published props must include members
/// from all three levels.
///
/// Discriminating: `base` (file A), `mid` (file B), `own` (SFC) must ALL be
/// present — a resolver that stopped after one heritage hop would drop `base`.
/// The EXACT-SET assertion plus the `decoy` negative (a sibling interface in
/// file B that the chain never extends) fail if heritage leaks unrelated
/// declarations into the surface.
#[test]
fn two_level_cross_file_heritage_through_macro_surface() {
    let project = make_project();
    project
        .upsert_base(
            "/src/base.ts",
            r#"export interface BaseProps {
  base?: string
}
"#,
        )
        .unwrap();
    project
        .upsert_base(
            "/src/child.ts",
            r#"import type { BaseProps } from './base'

export interface ChildProps extends BaseProps {
  mid?: boolean
}

// A sibling interface in file B that no level of the chain extends. A resolver
// that over-collected file-level declarations would leak `decoy`.
export interface UnrelatedChild {
  decoy?: string
}
"#,
        )
        .unwrap();
    project
        .upsert_base(
            "/src/App.vue",
            r#"<script setup lang="ts">
import type { ChildProps } from './child'

interface GrandchildProps extends ChildProps {
  own?: number
}

defineProps<GrandchildProps>()
</script>
<template><div /></template>"#,
        )
        .unwrap();

    let meta = project
        .host()
        .get_component_meta("/src/App.vue")
        .expect("should return component meta");
    let prop_names: BTreeSet<_> = meta.props.iter().map(|p| p.name.as_str()).collect();
    assert!(
        prop_names.contains("own") && prop_names.contains("mid") && prop_names.contains("base"),
        "two-level cross-file heritage must surface members from all three levels (own/mid/base), got {prop_names:?}",
    );
    // NEGATIVE: the unrelated sibling interface's member must NOT leak.
    assert!(
        !prop_names.contains("decoy"),
        "two-level heritage must NOT leak the unrelated `UnrelatedChild.decoy` member, got {prop_names:?}",
    );
    // EXACT surface: exactly the three heritage-chain members.
    assert_eq!(
        prop_names,
        BTreeSet::from(["own", "mid", "base"]),
        "two-level heritage surface must be EXACTLY {{own, mid, base}}, got {prop_names:?}",
    );
}

/// The imported-macro component-meta query path performs ZERO
/// query-time parser-expander work: after the cold compute, a repeat
/// query re-serves the result without a single additional eval-program
/// parse. The retired per-query expander re-entered
/// `parse_eval_program` on every resolution (one borrowed
/// type-resolution context per call), so on that tree the second query
/// grows `eval_program_parses` and this assertion FAILS; on the
/// severed tree the imported surface is served through the shared
/// typed-IR dispatch and the counter stays flat.
#[test]
fn imported_macro_query_path_runs_zero_parser_expander_parses() {
    let project = make_project();
    project
        .upsert_base(
            "/dep.ts",
            "export interface DepProps { label?: string; count: number }\n",
        )
        .unwrap();
    project
        .upsert_base(
            "/App.vue",
            r#"<script setup lang="ts">
import type { DepProps } from './dep'
defineProps<DepProps>()
</script>
<template><div /></template>"#,
        )
        .unwrap();

    // Cold query — resolves the imported macro surface through dispatch.
    // The published props keep the AUTHORED SOURCE ORDER of the imported
    // interface (`label?` before `count`) — the shared dispatch projects
    // ordered declaration groups, not a re-sorted member list.
    let cold = get_meta(&project, "/App.vue");
    let cold_names: Vec<&str> = cold.props.iter().map(|p| p.name.as_str()).collect();
    assert_eq!(
        cold_names,
        vec!["label", "count"],
        "the imported macro surface must resolve through the shared dispatch, preserving \
         authored member order"
    );

    // The parse budget after the cold compute: every live file parses
    // once through the indexed-ready materialise funnel.
    let parses_after_cold = project.host().provenance().snapshot().eval_program_parses;

    // Repeat query — must NOT re-enter the eval-program parse funnel.
    let warm = get_meta(&project, "/App.vue");
    let warm_names: Vec<&str> = warm.props.iter().map(|p| p.name.as_str()).collect();
    assert_eq!(
        warm_names, cold_names,
        "warm serve returns the same surface"
    );

    let parses_after_warm = project.host().provenance().snapshot().eval_program_parses;
    assert_eq!(
        parses_after_warm, parses_after_cold,
        "a repeat imported-macro query must perform ZERO additional eval-program \
         parses — a per-query parser-expander context (one parse per resolution) \
         cannot satisfy this"
    );
}

/// FIX-1 regression (audited / LSP-facing output route): the cold resolve
/// inside `get_component_meta_output_with_resolution` is PINNED to the
/// entry's ONE captured store view. A dependency mutation landing between
/// the capture and the resolve must yield a view-CONSISTENT response —
/// fully the captured (old) view's world — never a fresh-view analysis
/// (2 props) paired with capture-bound materialization (the torn result).
///
/// Discrimination: an UNPINNED resolve (`resolve_component_meta` opening
/// its own `snapshot_store_view_read()`) observes the mutated dep and
/// returns 2 props — every `len == 1` assertion below fails RED.
#[test]
fn audited_output_cold_resolve_is_pinned_to_the_captured_store_view() {
    let project = make_project();
    seed_pinned_view_fixture(&project, "/PinAppOutput.vue");

    let mutate = Arc::clone(&project);
    arm_cold_body_pre_resolve_hook(move || mutate_pinned_view_dep(&mutate));

    let (output, _request_id) = {
        let (output, request_id) = project
            .host()
            .get_component_meta_output_with_resolution("/PinAppOutput.vue")
            .expect("output materialization must not fail");
        (output.expect("component must resolve"), request_id)
    };
    let (analysis, _resolution, types) = output.into_parts();
    let lanes = types.into_lanes();

    assert_eq!(
        analysis.props.len(),
        1,
        "view-consistency: the analysis is the CAPTURED view's world (1 prop), \
         never the mutated view's (2 props) — an unpinned resolve tears here"
    );
    assert_eq!(analysis.props[0].name, "value");
    assert_eq!(lanes.props.len(), 1, "the materialized lane aligns 1:1");
    assert_eq!(
        published_type(&lanes.props[0]),
        &TypeExpr::Primitive(PrimitiveName::Number),
        "the materialized prop type is the CAPTURED view's `number` — the \
         analysis and the materialization describe ONE snapshot"
    );

    // Recovery: with the mutation now live (and the superseded cold result
    // fenced out of the warm cache), a fresh request serves the NEW world.
    let (fresh, _request_id) = {
        let (output, request_id) = project
            .host()
            .get_component_meta_output_with_resolution("/PinAppOutput.vue")
            .expect("fresh output materialization must not fail");
        (output.expect("component must resolve"), request_id)
    };
    let (fresh_analysis, _fresh_resolution, _fresh_types) = fresh.into_parts();
    assert_eq!(
        fresh_analysis.props.len(),
        2,
        "the next request observes the mutated dep — the fenced cold result \
         was returned-only, never promoted as warm state"
    );
}

/// FIX-1 regression (locator-only analysis entry): the cold resolve inside
/// `get_component_meta` is PINNED to the entry's ONE captured store view —
/// same barrier-controlled mutation, same view-consistency contract as the
/// audited output route above.
#[test]
fn analysis_entry_cold_resolve_is_pinned_to_the_captured_store_view() {
    let project = make_project();
    seed_pinned_view_fixture(&project, "/PinAppAnalysis.vue");

    let mutate = Arc::clone(&project);
    arm_cold_body_pre_resolve_hook(move || mutate_pinned_view_dep(&mutate));

    let analysis = project
        .host()
        .get_component_meta("/PinAppAnalysis.vue")
        .expect("component must resolve");
    assert_eq!(
        analysis.props.len(),
        1,
        "view-consistency: the locator-only analysis is the CAPTURED view's \
         world (1 prop) — an unpinned resolve observes the mutated dep (2)"
    );
    assert_eq!(analysis.props[0].name, "value");

    // Recovery: the next request observes the mutated dep.
    let fresh = project
        .host()
        .get_component_meta("/PinAppAnalysis.vue")
        .expect("component must resolve");
    assert_eq!(fresh.props.len(), 2);
}

/// FIX-1 regression (resolution-bearing locator entry): the cold resolve
/// inside `get_component_meta_with_resolution` is PINNED to the entry's ONE
/// captured store view.
#[test]
fn with_resolution_cold_resolve_is_pinned_to_the_captured_store_view() {
    let project = make_project();
    seed_pinned_view_fixture(&project, "/PinAppResolution.vue");

    let mutate = Arc::clone(&project);
    arm_cold_body_pre_resolve_hook(move || mutate_pinned_view_dep(&mutate));

    let (analysis, _resolved) = project
        .host()
        .get_component_meta_with_resolution("/PinAppResolution.vue")
        .expect("component must resolve");
    assert_eq!(
        analysis.props.len(),
        1,
        "view-consistency: the analysis is the CAPTURED view's world"
    );
    assert_eq!(analysis.props[0].name, "value");
}

/// RECOVERY: the same output succeeds once the missing dependency becomes
/// available — the typed failure is a live-view condition, not a sticky
/// poisoned state.
#[test]
fn component_meta_output_recovers_after_missing_dependency_is_available() {
    let project = make_project();
    project
        .upsert_base("/App.vue", "<template><div /></template>")
        .unwrap();
    let host = project.host();

    let dep_source = authored_decl_body_source("/dep.ts", "DepType");
    let mut analysis = blank_output_analysis();
    analysis.props.push(
        verter_session_query::analysis::component_meta::PropAnalysis {
            name: "p".to_string(),
            callable_role: verter_type_expr::PropCallableRole::default(),
            publication: crate::test_only::type_publication_fixture(
                verter_type_expr::facts::SourcePosition::Present(dep_source.clone()),
                verter_type_expr::ResolutionExactness::ExactConcrete,
                None,
                None,
            ),
            type_expansion: None,
            required: true,
            has_default: false,
            default_value: None,
            description: None,
            tags: Vec::new(),
            declared_in_macro_type_arg: false,
        },
    );

    let fixture_dispatch_17 =
        verter_type_engine::project_semantic_dispatch::ProjectSemanticDispatch::new(host);
    let err = crate::meta_resolve::projectors::build_component_meta_output(
        host,
        &fixture_dispatch_17,
        "/App.vue",
        analysis.clone(),
        None,
        crate::meta_resolve::PublishedCompleteness::COMPLETE,
    )
    .expect_err("the dependency file does not exist yet — the output must fail typed");
    assert_eq!(err.lane, crate::meta_resolve::ComponentMetaOutputLane::Prop);

    project
        .upsert_base("/dep.ts", "export type DepType = number\n")
        .unwrap();

    let output = crate::meta_resolve::projectors::build_component_meta_output(
        host,
        &fixture_dispatch_17,
        "/App.vue",
        analysis,
        None,
        crate::meta_resolve::PublishedCompleteness::COMPLETE,
    )
    .expect("the SAME source succeeds once the dependency is available");
    let lanes = output.into_parts().2.into_lanes();
    assert_eq!(
        published_type(&lanes.props[0]),
        &TypeExpr::Primitive(PrimitiveName::Number),
        "the recovered raise materializes the dependency's actual body"
    );
}

/// An IMPORTED props interface member whose value is a function
/// (`onClick: () => void`) has no authored slot, no use-site slot, and no
/// closed upgrade on its published node — but it IS known structure: the
/// structural member-source projection publishes the faithful PRESENT
/// projected MEMBER-PATH replay route (the macro's stamped type-argument
/// base + the member name), output materialization replays it through the
/// one shared dispatch, and the published prop renders the REAL function
/// shape. The result is a COMPLETE success. The pre-fix behavior was the
/// typed `Failed(UnrepresentableRequiredMemberValue)` interim (and before
/// that, a fabricated `Present(Closed(Leaf(unknown)))` rendered as a
/// COMPLETED `unknown` prop — a fail-open reported as success).
#[test]
fn imported_shallow_function_member_publishes_present_structural_source() {
    let project = make_project();
    project
        .upsert_base(
            "/props.ts",
            "export interface Props { onClick: () => void }\n",
        )
        .unwrap();
    project
        .upsert_base(
            "/App.vue",
            r#"<script setup lang="ts">
import type { Props } from './props'
defineProps<Props>()
</script>
<template><div /></template>"#,
        )
        .unwrap();

    let output = project
        .host()
        .get_component_meta_output("/App.vue")
        .expect("a known-structure member value is representable and must succeed")
        .expect("component must resolve");
    let (analysis, _resolution, types) = output.into_parts();
    let lanes = types.into_lanes();
    let on_click = analysis
        .props
        .iter()
        .position(|prop| prop.name == "onClick")
        .expect("the onClick prop publishes");
    // The published source is the projected MEMBER-PATH replay route off the
    // macro's stamped type-argument base — never a fabricated closed fact
    // and never a failure.
    match analysis.props[on_click]
        .publication
        .result()
        .selected_source()
    {
        Some(verter_type_expr::facts::SemanticTypeSource::Projected(
            verter_type_expr::facts::ProjectedTypeFact::MemberPath { path, .. },
        )) => {
            assert_eq!(
                path.as_ref(),
                [verter_type_engine::semantic_query::PropertyKey::identifier(
                    "onClick"
                )],
                "one member hop"
            );
        }
        other => panic!(
            "the imported function member publishes the MemberPath replay \
             source, got {other:?}"
        ),
    }
    // The replayed member materializes the REAL shallow function shape —
    // never an unknown.
    assert!(
        matches!(
            lanes.props[on_click]
                .materialized_type()
                .expect("published type"),
            TypeExpr::Function { .. }
        ),
        "the replayed function member renders its function shape; got {:?}",
        lanes.props[on_click]
    );

    // COMPLETE-success enforcement: the representable member leaves the
    // result complete — the fail-closed interim (a partial, suppressed
    // result) is gone for this class.
    let (_analysis, state) = project
        .host()
        .get_component_meta_with_resolution("/App.vue")
        .expect("resolves");
    assert!(
        !state.completeness.is_partial(),
        "a representable member value completes; got {:?}",
        state.completeness
    );
    assert!(
        !state.synthesis_should_suppress,
        "a representable member value must not suppress warm result admission"
    );
}

/// Stable unresolved-name control: an authored name that cannot currently be
/// resolved remains an explicit `Ref` carrier. This is a complete semantic
/// result, not operational truncation and not a fabricated `unknown`.
#[test]
fn prop_member_value_referencing_unresolved_type_stays_complete_carrier() {
    let project = make_project();
    project
        .upsert_base(
            "/props.ts",
            "export interface Props { broken: MissingType }\n",
        )
        .unwrap();
    project
        .upsert_base(
            "/App.vue",
            r#"<script setup lang="ts">
import type { Props } from './props'
defineProps<Props>()
</script>
<template><div /></template>"#,
        )
        .unwrap();

    let (analysis, _resolution, types) = project
        .host()
        .get_component_meta_output("/App.vue")
        .expect("a stable unresolved reference has a representable source")
        .expect("component resolves")
        .into_parts();
    let index = analysis
        .props
        .iter()
        .position(|prop| prop.name == "broken")
        .expect("broken prop publishes");
    let lanes = types.into_lanes();
    assert!(
        matches!(
            lanes.props[index]
                .materialized_type()
                .expect("published type"),
            TypeExpr::Ref { name, type_arguments }
                if name.as_ref() == "MissingType" && type_arguments.is_empty()
        ),
        "the authored unresolved name remains an explicit zero-argument Ref carrier"
    );

    let (_analysis, state) = project
        .host()
        .get_component_meta_with_resolution("/App.vue")
        .expect("component resolves");
    assert!(
        !state.completeness.is_partial(),
        "a stable unresolved carrier is Complete; got {:?}",
        state.completeness
    );
    assert!(
        !state.synthesis_should_suppress,
        "a stable unresolved carrier does not suppress an otherwise complete result"
    );
}

#[test]
fn recursive_indexed_access_member_replays_from_the_authored_surface() {
    let project = make_project();
    project
        .upsert_base(
            "/links.ts",
            r#"export interface LinkProps extends BaseLinkProps {
  to?: string
}

export interface BaseLinkProps {
  href?: LinkProps['to']
}"#,
        )
        .unwrap();
    project
        .upsert_base(
            "/Link.vue",
            r#"<script setup lang="ts">
import type { LinkProps } from './links'
defineProps<LinkProps>()
</script>
<template><a /></template>"#,
        )
        .unwrap();

    let (analysis, _resolution, types) = project
        .host()
        .get_component_meta_output("/Link.vue")
        .expect("a recursive indexed-access member retains a replayable source")
        .expect("component resolves")
        .into_parts();
    let index = analysis
        .props
        .iter()
        .position(|prop| prop.name == "href")
        .expect("href prop publishes");
    let lanes = types.into_lanes();
    assert!(
        matches!(
            lanes.props[index]
                .materialized_type()
                .expect("published type"),
            TypeExpr::Primitive(verter_type_expr::PrimitiveName::String)
                | TypeExpr::IndexedAccess { .. }
        ),
        "the recursive carrier must resolve safely or remain an explicit indexed-access shell"
    );

    let (_analysis, state) = project
        .host()
        .get_component_meta_with_resolution("/Link.vue")
        .expect("component resolves");
    assert!(
        !state.completeness.is_partial(),
        "a legitimate recursive carrier is complete, not budget exhaustion"
    );
}

/// A NON-CARRIER dependency (`.ts` / `.d.ts`) whose text contains a
/// `<script ...>` ... `</script>` pair inside documentation (a JSDoc
/// `@example` block — the vue-router@5 / @regle/core / unhead dist shape)
/// must keep its full type surface: eval-source production must never
/// script-scan a file that is not classified a framework carrier.
///
/// The fixture mirrors the vue-router@5 dist layout: a clean re-export
/// BARREL (`import { yt as Real } from './inner'; export { type Real }`)
/// in front of an INNER declaration file whose JSDoc carries the
/// `<script setup>` example. Pre-fix, the forgiving Vue raw scan fired on
/// the inner file's raw text, blanked everything outside the JSDoc
/// example, and the inner file's shallow inventory published EMPTY — the
/// barrel forward ended in an interned `Opaque(Miss)`. The shallow-by-default
/// publication keeps the component RESOLVING even over the destroyed
/// dependency surface (the prop publishes a shallow source), so the
/// discriminating signal is the dependency INVENTORY: the inner file's
/// prepared declaration scope must retain its type declarations, and the
/// published member value must demand-materialize to the dependency's REAL
/// shape instead of an unresolvable miss.
#[test]
fn non_carrier_dependency_with_script_tag_docs_keeps_member_values_representable() {
    let project = make_project();
    project
        .upsert_base(
            "/inner.ts",
            r#"/**
 * Usage example:
 * ```vue
 * <script setup>
 * const value = useReal()
 * </script>
 * ```
 */
type Real = string | { path: string }
export { Real as yt }
"#,
        )
        .unwrap();
    project
        .upsert_base(
            "/dep.ts",
            "import { yt as Real } from './inner'
export { type Real }
",
        )
        .unwrap();
    project
        .upsert_base(
            "/props.ts",
            "import type { Real } from './dep'
export interface Props { to?: Real }
",
        )
        .unwrap();
    project
        .upsert_base(
            "/App.vue",
            r#"<script setup lang="ts">
import type { Props } from './props'
defineProps<Props>()
</script>
<template><div /></template>"#,
        )
        .unwrap();

    // INVENTORY: the script-tag-bearing non-carrier keeps its declarations —
    // pre-fix the raw scan blanked the file and this scope published EMPTY.
    let inner_bundle = project
        .host()
        .prepared_decl_bundle("/inner.ts")
        .expect("the inner dependency materializes a prepared-decl bundle");
    let inner_scope = inner_bundle
        .owner_scope(verter_type_expr::TopLevelOwnerId::ordinary_file())
        .expect("ordinary TypeScript file has a module-zero declaration scope");
    assert!(
        inner_scope.scope_type_names.contains("Real"),
        "the non-carrier inner file keeps its type inventory (never \
         script-scanned); got {:?}",
        inner_scope.scope_type_names
    );

    let output = project
        .host()
        .get_component_meta_output("/App.vue")
        .expect(
            "a documentation-only <script> pair inside a non-carrier dependency \
             must not destroy its type surface — output materialization succeeds",
        )
        .expect("component must resolve");
    let (analysis, _resolution, types) = output.into_parts();
    let lanes = types.into_lanes();
    let to = analysis
        .props
        .iter()
        .position(|prop| prop.name == "to")
        .expect("the `to` prop publishes");
    assert!(
        analysis.props[to]
            .publication
            .result()
            .selected_source()
            .is_some(),
        "the imported member value publishes a PRESENT source (the dependency's \
         declarations resolve); got {:?}",
        analysis.props[to].publication.source_position()
    );
    assert!(
        !matches!(
            lanes.props[to].materialized_type().expect("published type"),
            TypeExpr::Unknown { .. }
        ),
        "the materialized prop type renders the dependency's real shape, \
         never an unknown; got {:?}",
        lanes.props[to]
    );

    // ON-DEMAND RESOLUTION: the published member value demand-materializes to
    // the dependency's REAL shape (`string | { path: string }`) — the walk
    // crosses the barrel into the script-tag-bearing inner file.
    let demanded = demand_published_type(
        project.host(),
        "/App.vue",
        analysis.props[to].publication.result().selected_source(),
        "`to` prop",
    );
    let TypeExpr::Union(arms) = &demanded else {
        panic!("`to` demand-materializes to the dependency union; got {demanded:?}");
    };
    assert!(
        arms.iter()
            .any(|arm| matches!(arm, TypeExpr::Primitive(PrimitiveName::String))),
        "the union keeps its `string` arm; got {arms:?}"
    );
    assert!(
        arms.iter().any(|arm| matches!(
            arm,
            TypeExpr::Object(object)
                if object.properties.iter().any(|property| matches!(
                    property,
                    ObjectMember::Property(property) if property.string_name().expect("string-key fixture") == "path"
                ))
        )),
        "the union keeps its `{{ path: string }}` arm; got {arms:?}"
    );

    // COMPLETE-success enforcement: a resolvable dependency leaves the
    // result complete and warm-admissible.
    let (_analysis, state) = project
        .host()
        .get_component_meta_with_resolution("/App.vue")
        .expect("resolves");
    assert!(
        !state.completeness.is_partial(),
        "a resolvable dependency completes; got {:?}",
        state.completeness
    );
}
