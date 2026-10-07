use super::*;

#[test]
fn resolved_meta_reuses_resolver_cache_after_legacy_slot_is_cleared() {
    let project = make_project();
    project
        .upsert_base("Comp.vue", &sfc("count: number"))
        .unwrap();

    let _ = project
        .host()
        .resolve_component_meta(
            "Comp.vue",
            verter_type_engine::semantic_query::ProjectionMode::Expanded,
        )
        .expect("initial resolve should succeed");
    let first_cache = cached_resolved_state(
        &project,
        "Comp.vue",
        verter_type_engine::semantic_query::ProjectionMode::Expanded,
    )
    .expect("initial resolve should populate legacy cache mirror");

    clear_legacy_cached_resolved_state(
        &project,
        "Comp.vue",
        verter_type_engine::semantic_query::ProjectionMode::Expanded,
    );
    assert!(
        cached_resolved_state(
            &project,
            "Comp.vue",
            verter_type_engine::semantic_query::ProjectionMode::Expanded
        )
        .is_none(),
        "legacy cache slot should be cleared before the second lookup"
    );

    project.host().provenance().reset();
    let _ = project
        .host()
        .resolve_component_meta(
            "Comp.vue",
            verter_type_engine::semantic_query::ProjectionMode::Expanded,
        )
        .expect("second resolve should succeed from resolver-owned cache");
    let second_cache = cached_resolved_state(
        &project,
        "Comp.vue",
        verter_type_engine::semantic_query::ProjectionMode::Expanded,
    )
    .expect("resolver-owned cache hit should mirror back into the legacy slot");

    assert!(Arc::ptr_eq(&first_cache, &second_cache));
    assert_eq!(
        provenance(&project).component_meta_resolved_state_recomputes,
        0,
        "resolver-owned cache hit should avoid a recompute after the legacy slot is cleared"
    );
    assert_eq!(
        provenance(&project).resolver_node_cache_hits,
        1,
        "second lookup should be served from the resolver-owned cache"
    );
    assert_eq!(
        provenance(&project).resolver_node_cache_misses,
        0,
        "second lookup should not miss the resolver-owned cache after the legacy slot is cleared"
    );
    assert_eq!(
        provenance(&project).resolver_singleflight_coalesced,
        0,
        "single-threaded cache reuse should not require singleflight coalescing"
    );
}

#[test]
fn resolved_meta_partial_direct_publish_refuses_validated_and_legacy_slots() {
    let project = make_project();
    project
        .upsert_base("Comp.vue", &sfc("count: number"))
        .unwrap();
    let host = project.host();
    let mode = verter_type_engine::semantic_query::ProjectionMode::Expanded;
    let mut partial = host
        .resolve_component_meta("Comp.vue", mode)
        .expect("control resolve must produce a complete state to mutate into a partial fixture");

    let key =
        crate::host_manage::component_meta_request_impl::resolved_meta_cache_key("Comp.vue", mode);
    host.resolver_runtime().component_meta.remove(&key);
    clear_legacy_cached_resolved_state(&project, "Comp.vue", mode);

    partial.completeness = verter_type_engine::semantic_query::ResultCompleteness::partial(
        verter_type_engine::semantic_query::PartialReasonSet::PROPAGATED,
    );
    partial.synthesis_should_suppress = true;
    let facts = partial.fact_versions.clone();
    host.store_cached_resolved_meta("Comp.vue", mode, &partial, &facts);

    assert!(
        cached_resolved_state(&project, "Comp.vue", mode).is_none(),
        "a direct partial publish must not write the legacy resolved-meta mirror"
    );
    let view = host.resolver_store_view_read().into_owned_view();
    assert!(
        host.resolver_runtime()
            .component_meta
            .get_if_valid(&key, &view)
            .is_none(),
        "a direct partial publish must not write the validated resolved-meta cache"
    );
}

/// PUBLIC BOUNDARY — a `defineSlots` surface with an UNRESOLVABLE slot value
/// never publishes as COMPLETE and never warms. Three spellings of the same
/// drop, each with a different producer that used to spell the failure as
/// silence:
///
/// 1. a slot member typed DIRECTLY by an unresolvable import — the slot
///    cannot be classified callable at all;
/// 2. a slot member typed by a LOCAL alias whose body is the unresolvable
///    import — the alias demand-validates but the callable realization walks
///    into the unresolved body;
/// 3. a resolvable slot CALLABLE whose first-param (binding) type is the
///    unresolvable import — the slot publishes, its binding surface cannot.
///
/// In every row: no fabricated slots/bindings, `synthesis_should_suppress`,
/// and no `ComponentMetaResultDb` warm on replay.
#[cfg(not(target_arch = "wasm32"))]
#[test]
fn an_unresolvable_slot_surface_never_publishes_complete_or_warm() {
    use std::sync::atomic::Ordering::Relaxed;

    const ROWS: &[(&str, &str)] = &[
        (
            "/src/SlotValueMissing.vue",
            r#"<script setup lang="ts">
import type { SlotFn } from './missing'
defineSlots<{ default: SlotFn }>()
</script>
<template><div /></template>"#,
        ),
        (
            "/src/SlotAliasMissing.vue",
            r#"<script setup lang="ts">
import type { Missing } from './missing'
type SlotFn = Missing
defineSlots<{ default: SlotFn }>()
</script>
<template><div /></template>"#,
        ),
        (
            "/src/SlotBindingMissing.vue",
            r#"<script setup lang="ts">
import type { MissingProps } from './missing'
defineSlots<{ default: (props: MissingProps) => any }>()
</script>
<template><div /></template>"#,
        ),
    ];

    for (canonical, source) in ROWS {
        let project = make_project();
        project.upsert_base(canonical, source).unwrap();
        let host = project.host();
        let meta = get_meta(&project, canonical);
        for slot in &meta.slots {
            assert!(
                slot.bindings.is_empty(),
                "{canonical}: an unresolvable slot surface must not fabricate \
                 bindings; slot `{}` got {:?}",
                slot.name,
                slot.bindings
            );
        }

        let (_, resolved) = host
            .get_component_meta_with_resolution(canonical)
            .expect("the resolve must still return metadata");
        assert!(
            resolved.synthesis_should_suppress,
            "{canonical}: a slots surface built around an unresolvable value \
             must NOT report COMPLETE"
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
            "{canonical}: an unresolvable slot surface must NOT warm \
             `ComponentMetaResultDb`"
        );
    }

    // THE DISCRIMINATION CONTROL: a resolvable slot whose binding object is
    // genuinely EMPTY publishes the slot, zero bindings, COMPLETE, and WARMS.
    let project = make_project();
    project
        .upsert_base(
            "/src/SlotEmptyBindings.vue",
            r#"<script setup lang="ts">
defineSlots<{ default: (props: {}) => any }>()
</script>
<template><div /></template>"#,
        )
        .unwrap();
    let host = project.host();
    let meta = get_meta(&project, "/src/SlotEmptyBindings.vue");
    assert!(
        meta.slots.iter().any(|slot| slot.name == "default"),
        "the control publishes its declared slot"
    );
    let (_, resolved) = host
        .get_component_meta_with_resolution("/src/SlotEmptyBindings.vue")
        .expect("the control resolve returns metadata");
    assert!(
        !resolved.synthesis_should_suppress,
        "a resolvable slot with a genuinely empty binding object is COMPLETE"
    );
    let hits_before = host
        .provenance()
        .component_meta_result_cache_hits
        .load(Relaxed);
    let _ = get_meta(&project, "/src/SlotEmptyBindings.vue");
    let hits_after = host
        .provenance()
        .component_meta_result_cache_hits
        .load(Relaxed);
    assert_eq!(hits_after, hits_before + 1, "the control WARMS on replay");
}

/// A construct-signature slot value (`new (p) => object`) is NOT a
/// callable slot shape: TS cannot invoke it as a render callback, so
/// slot-binding synthesis must refuse it (no bindings), exactly as a
/// non-callable value. The sibling call-signature slot is the positive
/// control proving the binding pipeline is live in the same fixture.
#[cfg(not(target_arch = "wasm32"))]
#[test]
fn construct_signature_slot_value_publishes_no_callable_bindings() {
    let project = make_project();
    project
        .upsert_base(
            "/src/Comp.vue",
            r#"<script setup lang="ts">
defineSlots<{
  default: new (p: { x: string }) => object
  named: (p: { y: string }) => any
}>()
</script>
<template><div /></template>"#,
        )
        .unwrap();
    let meta = get_meta(&project, "/src/Comp.vue");

    // Positive control: the CALL-signature slot binds its first-param
    // member.
    let named = meta
        .slots
        .iter()
        .find(|slot| slot.name == "named")
        .expect("the call-signature slot `named` must publish");
    let named_bindings: Vec<&str> = named.bindings.iter().map(|b| b.name.as_str()).collect();
    assert_eq!(
        named_bindings,
        vec!["y"],
        "the call-signature slot must bind `y`, got {named_bindings:?}"
    );

    // The construct-signature slot is NOT callable: no bindings may be
    // synthesized from its first parameter.
    let default_slot = meta.slots.iter().find(|slot| slot.name == "default");
    if let Some(slot) = default_slot {
        let bindings: Vec<&str> = slot.bindings.iter().map(|b| b.name.as_str()).collect();
        assert!(
            bindings.is_empty(),
            "a construct-signature slot value is not a callable slot shape — \
             no bindings may be synthesized from `new (p: {{ x: string }}) => object`, \
             got {bindings:?}"
        );
    }
}

#[test]
fn evaluate_types_skip_irrelevant_transitive_slot_value_dependencies() {
    let project = make_project();
    project
        .upsert_base(
            "/leaf.ts",
            r#"export interface LeafValue {
  class: string
}"#,
        )
        .unwrap();
    project
        .upsert_base(
            "/tv.ts",
            r#"import type { LeafValue } from './leaf'

type ComponentSlots<T extends { slots?: Record<string, any> }> = {
  [K in keyof T['slots']]?: LeafValue
}

export type ComponentConfig<T extends { slots?: Record<string, any> }> = {
  slots: ComponentSlots<T>
}"#,
        )
        .unwrap();
    project
        .upsert_base(
            "/theme.ts",
            r#"export default {
  slots: {
    item: 'item',
    body: 'body'
  }
}"#,
        )
        .unwrap();
    project
        .upsert_base(
            "/App.vue",
            r#"<script setup lang="ts">
import type { ComponentConfig } from './tv'
import theme from './theme'

type Accordion = ComponentConfig<typeof theme>

defineProps<{
  ui: Accordion['slots']
}>()
</script>
<template><div /></template>"#,
        )
        .unwrap();

    let session = project.open_session_batch().unwrap();
    let evaluated = session.evaluate_types("/App.vue").unwrap().unwrap();

    match &evaluated_prop_type(&project, "/App.vue", &evaluated, "ui") {
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
        other => panic!("expected ui slots object, got {other:?}"),
    }

    // Dependency tracking assertions removed — the legacy walker is deleted.
}

/// Route/mode-INDEPENDENT L1 for the MAPPED family: a hermetic SFC with
/// `defineSlots<OpenMappedSlots<T>>()` where `OpenMappedSlots<T>` is a
/// `{ [K in keyof BaseSlots]?: (props: { message: MessageBase<T> }) =>
/// VNode[] }` mapped type — the `ChatMessagesSlots<T>` / `TableSlots<T>`
/// family. The keys are enumerable (`keyof BaseSlots` is a finite closed
/// surface) but the per-key value body reaches the OPEN outer generic `T`
/// (NOT the bound mapper binder `K`) through a function-parameter object
/// member.
///
/// **Scope honesty.** A conditional-bodied mapped value (the exact real
/// `ChatMessagesSlots<T>` shape) yields `semanticMiss` downstream at
/// hermetic scale (the separate conditional-reduction gap), so this
/// fixture uses a PLAIN function value to make the per-key-value carrier
/// rule discriminating; the conditional-bodied open mapped storm is
/// reproduced and gated by the external-corpus oracle (the real
/// ChatMessages.vue / Table.vue surface).
///
/// **Discriminating.** The empty-path Shallow surface enumerator gates on
/// the KEY-PRODUCTION axis only: a CLOSED key domain (`keyof BaseSlots`)
/// ENUMERATES its keys even when the per-key value body reaches the open
/// outer `T` — but each enumerated binding's published VALUE stays the
/// deferred CARRIER (`MessageBase<T>` stays a `Ref` reference, never a
/// flattened object of MessageBase's members, never a both-branch Union,
/// never a budget sentinel). An eager per-key-value materialiser flattens
/// `items` / `current` into the binding's type and fails the carrier
/// assertions.
#[test]
fn get_component_meta_open_mapped_slots_surface_carrier_stops_no_sentinel() {
    let project = make_project();
    project
        .upsert_base(
            "/OpenMappedSlots.vue",
            r#"<script lang="ts">
type VNode = { __isVNode: true }

interface BaseSlots {
  header(props: { title: string }): VNode[]
  footer(props: { note: string }): VNode[]
}

interface MessageBase<T> {
  items: T
  current: T
}

// Open mapped slots surface: the keys (`header` / `footer`) are
// enumerable from the closed `BaseSlots`, but each per-key value reaches
// the OPEN outer generic `T` through a function-parameter object member.
export type OpenMappedSlots<T> = {
  [K in keyof BaseSlots]?: (props: { message: MessageBase<T> }) => VNode[]
}
</script>

<script setup lang="ts" generic="T">
defineSlots<OpenMappedSlots<T>>()
</script>
<template><div /></template>"#,
        )
        .unwrap();

    // The resolution must TERMINATE and publish a COMPLETE result.
    let meta = get_meta(&project, "/OpenMappedSlots.vue");

    // Typed no-leak guard (replaces the raw-spelling scans over props and
    // slot bindings): the resolved meta was ADMITTED to the resolved-meta
    // cache — admission is complete-only, so a budget-tripped partial would
    // have been refused.
    assert!(
        cached_resolved_state(
            &project,
            "/OpenMappedSlots.vue",
            verter_type_engine::semantic_query::ProjectionMode::Expanded
        )
        .is_some(),
        "the OpenMappedSlots.vue resolution must be admitted complete — a leaked budget partial would refuse admission"
    );
    assert_no_degraded_props(project.host(), "/OpenMappedSlots.vue", &meta);

    // CLOSED key domain ⇒ the keys ENUMERATE: `header` / `footer` publish
    // as named slots (key enumeration is path-precise even when the value
    // body is open).
    let slot_names: Vec<&str> = meta.slots.iter().map(|s| s.name.as_str()).collect();
    for key in ["header", "footer"] {
        assert!(
            slot_names.contains(&key),
            "the closed-key mapped slots surface must enumerate `{key}`, got {slot_names:?}"
        );
    }

    // The open per-key VALUE stays a deferred CARRIER: each enumerated
    // slot's `message` binding publishes the `MessageBase<T>` reference
    // carrier — structurally NOT a flattened `TypeExpr::Object`
    // materialising MessageBase's members (`items` / `current`) and NOT a
    // both-branch Union.
    for slot in &meta.slots {
        let message = slot
            .bindings
            .iter()
            .find(|b| b.name == "message")
            .unwrap_or_else(|| {
                panic!(
                    "slot `{}` must carry the `message` binding from the per-key \
                     function value's first-parameter object, got bindings: {:?}",
                    slot.name,
                    slot.bindings.iter().map(|b| &b.name).collect::<Vec<_>>()
                )
            });
        match &shallow_published_type(
            project.host(),
            "/OpenMappedSlots.vue",
            message.publication.result().selected_source(),
            "message binding",
        ) {
            verter_type_expr::TypeExpr::Object(obj) => panic!(
                "slot `{}`'s `message` binding must stay the MessageBase<T> carrier — it \
                 flattened into an object surface: {obj:?}",
                slot.name
            ),
            verter_type_expr::TypeExpr::Union(arms) => panic!(
                "slot `{}`'s `message` binding must stay the MessageBase<T> carrier — it \
                 widened into a Union: {arms:?}",
                slot.name
            ),
            verter_type_expr::TypeExpr::Ref { name, .. } => assert_eq!(
                name.as_ref(),
                "MessageBase",
                "slot `{}`'s `message` binding must publish the MessageBase reference carrier",
                slot.name
            ),
            other => panic!(
                "slot `{}`'s `message` binding must publish a reference carrier, got {other:?}",
                slot.name
            ),
        }
    }

    // POSITIVE witness — the carrier-stop is not a DROPPED surface. The
    // empty/shallow assertions above would also pass on a broken pipeline
    // that simply lost the slots payload, so three positive facts pin the
    // consumed-and-carried shape:
    //
    //  (a) the analyzer consumed `defineSlots<OpenMappedSlots<T>>()`;
    //  (b) the slots macro-payload expansion ran to COMPLETION and
    //      published its (shallow) shape — a dropped payload has no
    //      `define_slots` entry, a stormed one is not `Completed`;
    //  (c) the shared dispatch route the registry / macro-payload
    //      consumers resolve through yields the PRESERVED `Mapped`
    //      carrier for `OpenMappedSlots<T>` (the carrier IS the published
    //      value and stays re-resolvable on demand).
    let resolved = project
        .host()
        .resolve_component_meta(
            "/OpenMappedSlots.vue",
            verter_type_engine::semantic_query::ProjectionMode::Expanded,
        )
        .expect("resolved component meta should exist");
    assert!(
        resolved.snapshot.macros.iter().any(|m| {
            matches!(
                m.kind,
                verter_session_query::analysis::types::AnalyzedMacroKind::DefineSlots
            ) && m.type_references.iter().any(|r| r == "OpenMappedSlots")
        }),
        "the analyzer must consume `defineSlots<OpenMappedSlots<T>>()`, got macros: {:?}",
        resolved.snapshot.macros
    );
    let evaluated = resolved
        .evaluated_types
        .as_ref()
        .expect("Expanded-mode resolution must carry evaluated types");
    assert_eq!(
        evaluated.define_slots.len(),
        1,
        "the slots macro payload must publish exactly one expanded shape (a missing entry \
         means the slots surface was DROPPED, not carrier-stopped), got {:?}",
        evaluated.define_slots
    );
    assert!(
        matches!(
            evaluated.define_slots[0].result.execution_status,
            verter_session_query::analysis::type_expand::ExpansionExecutionStatus::Completed
        ),
        "the carrier-stopped slots payload must complete (not storm / trip a budget), got {:?}",
        evaluated.define_slots[0].result.execution_status
    );

    // (c) — the preserved `Mapped` carrier is reachable through the shared
    // dispatch (the same route the registry materialiser and the macro
    // payload resolve through): instantiating `OpenMappedSlots<T>` with an
    // open `T` yields the deferred `Mapped` shell, not an enumerated
    // Object and not an Opaque miss.
    let dispatch =
        verter_type_engine::project_semantic_dispatch::ProjectSemanticDispatch::new(project.host());
    let graph = Arc::clone(project.host().project_type_store().semantic_graph());
    let t_param = graph.intern_node(
        verter_type_engine::semantic_query::SemanticNodeData::TypeParam {
            decl: verter_type_engine::semantic_query::DeclIdentity::synthetic("T"),
            param_index: 0,
            constraint: None,
            default: None,
            display_name: Arc::from("T"),
        },
    );
    let carrier = match dispatch
        .execute_read(verter_type_engine::semantic_query::SemanticQueryKey::Instantiate(
            verter_type_engine::semantic_query::InstantiateKey::new(
                verter_type_engine::semantic_query::ResolvedDeclSlotIdentity::type_slot_unscoped(
                    Arc::from("/OpenMappedSlots.vue"),
                    verter_type_expr::TopLevelOwnerId::module(0),
                    Arc::from("OpenMappedSlots"),
                ),
                Arc::from(vec![t_param].into_boxed_slice()),
                verter_type_engine::semantic_query::InstantiateContext::non_file(
                    verter_type_engine::semantic_query::ProjectionReductionContext::published(
                        verter_type_engine::semantic_query::ProjectionMode::Navigate,
                    ),
                    Default::default(),
                    verter_type_engine::project_semantic_dispatch::BodySourceWitness::mint_for_unit_tests(),
                ),
            ),
        ))
        .value
    {
        verter_type_engine::semantic_query::QueryResult::Value(node) => node,
        other => panic!("OpenMappedSlots<T> must resolve to a Value carrier, got {other:?}"),
    };
    let mut node = carrier;
    for _ in 0..4 {
        match graph.node_data(node).as_deref() {
            Some(verter_type_engine::semantic_query::SemanticNodeData::Alias(inner)) => {
                node = *inner
            }
            _ => break,
        }
    }
    assert!(
        matches!(
            graph.node_data(node).as_deref(),
            Some(verter_type_engine::semantic_query::SemanticNodeData::Mapped { .. })
        ),
        "instantiating OpenMappedSlots<T> through the shared dispatch must preserve the \
         `Mapped` carrier shell (the published value), got {:?}",
        graph.node_data(node)
    );
}

#[test]
fn get_component_meta_keeps_local_slot_surface_without_imported_helper_pollution() {
    let project = make_project();
    project
        .upsert_base(
            "/tv.ts",
            r#"export type DynamicSlots<T extends Record<string, any>> = {
  [K in keyof T]?: (props: {}) => any
}

export type ComponentSlots<T extends { slots?: Record<string, any> }> = {
  [K in keyof T['slots']]?: (props: {}) => any
}

export type ComponentConfig<T extends { slots?: Record<string, any> }, A extends Record<string, any>> = {
  appConfig: A,
  slots: ComponentSlots<T>
}"#,
        )
        .unwrap();
    project
        .upsert_base(
            "/schema.ts",
            r#"export interface AppConfig {
  ui?: { variant: string }
}"#,
        )
        .unwrap();
    project
        .upsert_base(
            "/theme.ts",
            r#"export default {
  slots: {
    leading: 'leading',
    trailing: 'trailing'
  }
}"#,
        )
        .unwrap();
    project
        .upsert_base(
            "/App.vue",
            r#"<script setup lang="ts">
import type { ComponentConfig, DynamicSlots } from './tv'
import type { AppConfig } from './schema'
import theme from './theme'

type Accordion = ComponentConfig<typeof theme, AppConfig>

interface Slots extends DynamicSlots<Accordion['slots']> {
  default(props: { item: string }): any
  leading?(): any
  trailing?(): any
}

defineSlots<Slots>()
</script>
<template><div /></template>"#,
        )
        .unwrap();

    project.host().provenance().reset();
    let meta = get_meta(&project, "/App.vue");
    let slot_names: Vec<&str> = meta.slots.iter().map(|slot| slot.name.as_str()).collect();
    assert_eq!(slot_names, vec!["default", "leading", "trailing"]);
    assert!(
        !slot_names.contains(&"appConfig") && !slot_names.contains(&"slots"),
        "defineSlots output should not be polluted by imported helper object members: {slot_names:?}"
    );
}

#[test]
fn invalidate_compile_slots_does_not_break_subsequent_analysis() {
    let project = make_project();
    project
        .upsert_base("/App.vue", &sfc("msg: string"))
        .unwrap();

    let session = project.open_session_batch().unwrap();
    let before = session
        .get_analysis("/App.vue")
        .unwrap()
        .expect("analysis should exist before invalidation");
    let before_names = prop_names(&before);
    assert!(
        before_names.contains(&"msg".to_string()),
        "should see 'msg' prop before invalidation"
    );

    project.host().invalidate_compile_slots("/App.vue");

    // Assert+: analysis still works after invalidation
    let after = session
        .get_analysis("/App.vue")
        .unwrap()
        .expect("analysis should still work after invalidate_compile_slots");
    let after_names = prop_names(&after);
    assert!(
        after_names.contains(&"msg".to_string()),
        "should still see 'msg' prop after invalidation"
    );

    // Assert-: no spurious props introduced
    assert_eq!(
        after_names.len(),
        1,
        "should have exactly 1 prop after invalidation, not more"
    );
}

#[test]
fn resolve_component_meta_keeps_package_registry_helpers_shallow_for_local_slot_types() {
    let ws = Arc::new(verter_workspace::MemoryWorkspace::new(
        verter_workspace::MemoryOptions::default(),
    ));
    ws.inject_file(
        "/workspace/node_modules/pkg/package.json".to_string(),
        Arc::from(r#"{ "name": "pkg", "types": "./dist/index.d.ts" }"#),
    );
    ws.inject_file(
        "/workspace/node_modules/pkg/dist/index.d.ts".to_string(),
        Arc::from(
            r#"
export interface InternalNode {
  leaf: string
}

export type PublicNode = InternalNode | {
  next: InternalNode
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
            "/workspace/src/slot-types.ts",
            r#"import type { PublicNode } from 'pkg'

export interface ButtonSlots {
  default?(): PublicNode
}
"#,
        )
        .unwrap();
    project
        .upsert_base(
            "/workspace/src/App.vue",
            r#"<script setup lang="ts">
import type { ButtonSlots } from './slot-types'

defineSlots<ButtonSlots>()
</script>
<template><div /></template>"#,
        )
        .unwrap();

    project.host().set_import_dependencies(
        "/workspace/src/App.vue",
        vec![crate::types::DependencyResolution {
            specifier: "./slot-types".to_string(),
            resolved_canonical_id: Some("/workspace/src/slot-types.ts".to_string()),
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
        published_names.contains("ButtonSlots"),
        "local slot helper should still be published, got {published_names:?}"
    );
    assert!(
        !published_names.contains("PublicNode"),
        "package registry publication should stay shallow for external package types, got {published_names:?}"
    );
    assert!(
        !published_names.contains("InternalNode"),
        "package registry publication should stay shallow instead of recursing into helper internals, got {published_names:?}"
    );
}

#[test]
fn resolve_component_meta_keeps_transitive_nested_slot_param_helpers_off_registry() {
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
        "requested member-path materialization should not publish transitive nested helper refs",
    );

    // Shallow-by-default registry contract: imported helper aliases are
    // published as a bare `Ref { name }` in the resolved type registry, NOT
    // an eagerly-materialized object. The deep slot-binding correctness is
    // asserted on the PUBLISHED surface below.
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
        "registry entry must NOT eagerly materialize to an object, got {button_slots_ty:?}"
    );

    // Published-surface contract (path-precise materialization): the default
    // slot's `ui` binding stays symbolic on the requested member path
    // `Button['ui']` — the nested `DeepProps` callable-parameter helper is
    // never widened into the binding.
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
fn component_meta_keeps_explicit_slot_bindings_through_dynamic_slots_intersection() {
    let project = make_project();
    project
        .upsert_base(
            "/node_modules/reka-ui/index.d.ts",
            r#"
export interface TabsRootProps<T> {
  defaultValue?: T
  modelValue?: T
  activationMode?: 'automatic' | 'manual'
  unmountOnHide?: boolean
}

export interface TabsRootEmits<T> {
  (e: 'update:modelValue', payload: T): void
}
"#,
        )
        .unwrap();
    project
        .upsert_base(
            "/src/utils.ts",
            r#"
export type DynamicSlotsKeys<Name extends string | undefined, Suffix extends string | undefined = undefined> = (
  Name extends string
    ? Suffix extends string
      ? Name | `${Name}-${Suffix}`
      : Name
    : never,
)

export type DynamicSlots<
  T extends { slot?: string },
  Suffix extends string | undefined = undefined,
  ExtraProps extends object = {}
> = {
  [K in DynamicSlotsKeys<T['slot'], Suffix>]?: (
    props: { item: Extract<T, { slot: K extends `${infer Base}-${Suffix}` ? Base : K }> } & ExtraProps,
  ) => any
}
"#,
        )
        .unwrap();
    project
        .upsert_base(
            "/src/tv.ts",
            r#"
type Id<T> = {} & { [P in keyof T]: T[P] }

type ComponentUI<T extends { slots?: Record<string, any> }> = Id<{
  [K in keyof Required<T['slots']>]: (props?: Record<string, any>) => string
}>

export type ComponentConfig<T extends Record<string, any>> = {
  ui: ComponentUI<T>
}
"#,
        )
        .unwrap();
    project
        .upsert_base(
            "/src/theme.ts",
            r#"export default {
  slots: {
    root: '',
    list: '',
    trigger: '',
    label: '',
    content: ''
  }
} as const"#,
        )
        .unwrap();
    project
        .upsert_base(
            "/src/App.vue",
            r#"<script lang="ts">
import type { TabsRootProps, TabsRootEmits } from 'reka-ui'
import type { ComponentConfig } from './tv'
import type { DynamicSlots } from './utils'
import theme from './theme'

type Tabs = ComponentConfig<typeof theme>

export interface TabsItem {
  label?: string
  value?: string | number
  slot?: string
}

export interface TabsProps<T extends TabsItem = TabsItem> extends Pick<TabsRootProps<string | number>, 'defaultValue' | 'modelValue' | 'activationMode' | 'unmountOnHide'> {
  items?: T[]
}

export interface TabsEmits extends TabsRootEmits<string | number> {}

type SlotProps<T extends TabsItem> = (props: { item: T, index: number, ui: Tabs['ui'] }) => any

export type TabsSlots<T extends TabsItem = TabsItem> = {
  leading?: SlotProps<T>
  content?: SlotProps<T>
} & DynamicSlots<T, undefined, { index: number, ui: Tabs['ui'] }>
</script>
<script setup lang="ts" generic="T extends TabsItem">
defineProps<TabsProps<T>>()
defineEmits<TabsEmits>()
defineSlots<TabsSlots<T>>()
</script>
<template><div /></template>"#,
        )
        .unwrap();

    project.host().set_import_dependencies(
        "/src/App.vue",
        vec![
            crate::types::DependencyResolution {
                specifier: "reka-ui".to_string(),
                resolved_canonical_id: Some("/node_modules/reka-ui/index.d.ts".to_string()),
                possible_canonical_ids: Vec::new(),
            },
            crate::types::DependencyResolution {
                specifier: "./tv".to_string(),
                resolved_canonical_id: Some("/src/tv.ts".to_string()),
                possible_canonical_ids: Vec::new(),
            },
            crate::types::DependencyResolution {
                specifier: "./utils".to_string(),
                resolved_canonical_id: Some("/src/utils.ts".to_string()),
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
    let meta = crate::resolver_core::with_bare_host_ctx_for_test(project.host(), |ctx| {
        let fixture_dispatch_5 =
            verter_type_engine::project_semantic_dispatch::ProjectSemanticDispatch::new(ctx);

        crate::host_manage::extract_component_meta_from_resolved(
            project.host(),
            "/src/App.vue",
            &resolved,
            true,
            ctx,
            &fixture_dispatch_5,
        )
    })
    .analysis;
    let content_slot = meta
        .slots
        .iter()
        .find(|slot| slot.name == "content")
        .expect("content slot should exist");
    let binding_names: Vec<_> = content_slot
        .bindings
        .iter()
        .map(|binding| binding.name.as_str())
        .collect();
    assert_eq!(
        binding_names,
        vec!["item", "index", "ui"],
        "explicit slots in DynamicSlots intersections should keep their bindings, got {:?}",
        binding_names
    );

    let leading_slot = meta
        .slots
        .iter()
        .find(|slot| slot.name == "leading")
        .expect("leading slot should exist");
    let leading_binding_names: Vec<_> = leading_slot
        .bindings
        .iter()
        .map(|binding| binding.name.as_str())
        .collect();
    assert_eq!(
        leading_binding_names,
        vec!["item", "index", "ui"],
        "sibling explicit slots should keep the same intersection bindings, got {:?}",
        leading_binding_names
    );
}

#[test]
fn component_meta_keeps_realistic_tabs_slot_bindings_with_dynamic_helper_intersection() {
    let project = make_project();
    project
        .upsert_base(
            "/node_modules/reka-ui/index.d.ts",
            r#"
export interface TabsRootProps<T> {
  defaultValue?: T
  modelValue?: T
  activationMode?: 'automatic' | 'manual'
  unmountOnHide?: boolean
}

export interface TabsRootEmits<T> {
  (e: 'update:modelValue', payload: T): void
}
"#,
        )
        .unwrap();
    project
        .upsert_base(
            "/src/utils.ts",
            r#"
export type DynamicSlotsKeys<Name extends string | undefined, Suffix extends string | undefined = undefined> = (
  Name extends string
    ? Suffix extends string
      ? Name | `${Name}-${Suffix}`
      : Name
    : never,
)

export type DynamicSlots<
  T extends { slot?: string },
  Suffix extends string | undefined = undefined,
  ExtraProps extends object = {}
> = {
  [K in DynamicSlotsKeys<T['slot'], Suffix>]?: (
    props: { item: Extract<T, { slot: K extends `${infer Base}-${Suffix}` ? Base : K }> } & ExtraProps,
  ) => any
}

export type NestedItem<T> = T extends Array<infer I> ? NestedItem<I> : T

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

export type GetItemKeys<
  I,
  T extends NestedItem<I> = NestedItem<I>
> = (keyof Extract<T, object> & string) | DotPathKeys<Extract<T, object>>
"#,
        )
        .unwrap();
    project
        .upsert_base(
            "/src/tv.ts",
            r#"
type Id<T> = {} & { [P in keyof T]: T[P] }

type ComponentVariants<T extends { variants?: Record<string, Record<string, any>> }> = {
  [K in keyof T['variants']]: keyof T['variants'][K]
}

type ComponentSlots<T extends { slots?: Record<string, any> }> = Id<{
  [K in keyof T['slots']]?: string
}>

type ComponentUI<T extends { slots?: Record<string, any> }> = Id<{
  [K in keyof Required<T['slots']>]: (props?: Record<string, any>) => string
}>

export type ComponentConfig<T extends Record<string, any>> = {
  variants: ComponentVariants<T>,
  slots: ComponentSlots<T>
  ui: ComponentUI<T>
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
    variant: { pill: '', link: '' },
    size: { sm: '', md: '' },
    orientation: { horizontal: '', vertical: '' }
  },
  slots: {
    root: '',
    list: '',
    trigger: '',
    label: '',
    content: ''
  }
} as const"#,
        )
        .unwrap();
    project
        .upsert_base(
            "/src/App.vue",
            r#"<script lang="ts">
import type { TabsRootProps, TabsRootEmits } from 'reka-ui'
import type { ComponentConfig } from './tv'
import type { DynamicSlots, GetItemKeys } from './utils'
import theme from './theme'

type Tabs = ComponentConfig<typeof theme>

export interface TabsItem {
  label?: string
  value?: string | number
  slot?: string
  nested?: {
    path?: string
  }
}

export interface TabsProps<T extends TabsItem = TabsItem> extends Pick<TabsRootProps<string | number>, 'defaultValue' | 'modelValue' | 'activationMode' | 'unmountOnHide'> {
  items?: T[]
  color?: Tabs['variants']['color']
  variant?: Tabs['variants']['variant']
  size?: Tabs['variants']['size']
  orientation?: Tabs['variants']['orientation']
  valueKey?: GetItemKeys<T>
  labelKey?: GetItemKeys<T>
  ui?: Tabs['slots']
}

export interface TabsEmits extends TabsRootEmits<string | number> {}

type SlotProps<T extends TabsItem> = (props: { item: T, index: number, ui: Tabs['ui'] }) => any

export type TabsSlots<T extends TabsItem = TabsItem> = {
  leading?: SlotProps<T>
  default?(props: { item: T, index: number }): any
  trailing?: SlotProps<T>
  content?: SlotProps<T>
} & DynamicSlots<T, undefined, { index: number, ui: Tabs['ui'] }>
</script>
<script setup lang="ts" generic="T extends TabsItem">
withDefaults(defineProps<TabsProps<T>>(), {
  defaultValue: '0',
  orientation: 'horizontal',
  unmountOnHide: true,
  valueKey: 'value',
  labelKey: 'label'
})
defineEmits<TabsEmits>()
defineSlots<TabsSlots<T>>()
</script>
<template><div /></template>"#,
        )
        .unwrap();

    project.host().set_import_dependencies(
        "/src/App.vue",
        vec![
            crate::types::DependencyResolution {
                specifier: "reka-ui".to_string(),
                resolved_canonical_id: Some("/node_modules/reka-ui/index.d.ts".to_string()),
                possible_canonical_ids: Vec::new(),
            },
            crate::types::DependencyResolution {
                specifier: "./tv".to_string(),
                resolved_canonical_id: Some("/src/tv.ts".to_string()),
                possible_canonical_ids: Vec::new(),
            },
            crate::types::DependencyResolution {
                specifier: "./utils".to_string(),
                resolved_canonical_id: Some("/src/utils.ts".to_string()),
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
    let meta = crate::resolver_core::with_bare_host_ctx_for_test(project.host(), |ctx| {
        let fixture_dispatch_6 =
            verter_type_engine::project_semantic_dispatch::ProjectSemanticDispatch::new(ctx);

        crate::host_manage::extract_component_meta_from_resolved(
            project.host(),
            "/src/App.vue",
            &resolved,
            true,
            ctx,
            &fixture_dispatch_6,
        )
    })
    .analysis;
    let content_slot = meta
        .slots
        .iter()
        .find(|slot| slot.name == "content")
        .expect("content slot should exist");
    let binding_names: Vec<_> = content_slot
        .bindings
        .iter()
        .map(|binding| binding.name.as_str())
        .collect();
    assert_eq!(
        binding_names,
        vec!["item", "index", "ui"],
        "realistic Tabs slot helpers should keep explicit content bindings, got {:?}",
        binding_names
    );
}

#[test]
fn component_meta_keeps_conditional_slot_helper_symbolic_without_hanging() {
    let project = make_project();
    project
        .upsert_base(
            "/src/App.vue",
            r#"<script lang="ts">
type Mode = 'click' | 'hover'

type SlotProps<M extends Mode = Mode> = [M] extends ['hover']
  ? { close: undefined }
  : { close: () => void }

interface Slots<M extends Mode = Mode> {
  default?(props: { open: boolean }): any
  content?(props: SlotProps<M>): any
  anchor?(props: SlotProps<M>): any
}
</script>
<script setup lang="ts" generic="M extends Mode">
defineSlots<Slots<M>>()
</script>
<template><div /></template>"#,
        )
        .unwrap();

    let started = std::time::Instant::now();
    let resolved = project
        .host()
        .resolve_component_meta(
            "/src/App.vue",
            verter_type_engine::semantic_query::ProjectionMode::Expanded,
        )
        .expect("resolved component meta should exist");
    let meta = crate::resolver_core::with_bare_host_ctx_for_test(project.host(), |ctx| {
        let fixture_dispatch_9 =
            verter_type_engine::project_semantic_dispatch::ProjectSemanticDispatch::new(ctx);

        crate::host_manage::extract_component_meta_from_resolved(
            project.host(),
            "/src/App.vue",
            &resolved,
            false,
            ctx,
            &fixture_dispatch_9,
        )
    })
    .analysis;
    let elapsed = started.elapsed();

    assert!(
        elapsed.as_secs_f64() < 10.0,
        "conditional slot helper should not hang component-meta resolution \
         (elapsed {:.2}s)",
        elapsed.as_secs_f64()
    );

    let content_slot = meta
        .slots
        .iter()
        .find(|slot| slot.name == "content")
        .expect("content slot should exist");
    let anchor_slot = meta
        .slots
        .iter()
        .find(|slot| slot.name == "anchor")
        .expect("anchor slot should exist");
    assert!(
        content_slot.bindings.is_empty(),
        "conditional content slot helper should stay symbolic, got bindings {:?}",
        content_slot
            .bindings
            .iter()
            .map(|binding| binding.name.as_str())
            .collect::<Vec<_>>()
    );
    assert!(
        anchor_slot.bindings.is_empty(),
        "conditional anchor slot helper should stay symbolic, got bindings {:?}",
        anchor_slot
            .bindings
            .iter()
            .map(|binding| binding.name.as_str())
            .collect::<Vec<_>>()
    );
}

#[test]
fn imported_pick_slot_bindings_keep_symbolic_terminal_display() {
    let project = make_project();
    project
        .upsert_base(
            "/src/reka-ui.ts",
            r#"
export interface CalendarCellTriggerProps {
  day: Date,
  month: number
}
"#,
        )
        .unwrap();
    project
        .upsert_base(
            "/src/slots.ts",
            r#"
import type { CalendarCellTriggerProps } from './reka-ui'

export interface CalendarSlots {
  day?: (props: Pick<CalendarCellTriggerProps, 'day'>) => any
}
"#,
        )
        .unwrap();
    project
        .upsert_base(
            "/src/App.vue",
            r#"<script setup lang="ts">
import type { CalendarSlots } from './slots'

defineSlots<CalendarSlots>()
</script>
<template><div /></template>"#,
        )
        .unwrap();

    let meta = project
        .host()
        .get_component_meta("/src/App.vue")
        .expect("should return component meta");

    let day_slot = meta
        .slots
        .iter()
        .find(|slot| slot.name == "day")
        .expect("should extract imported day slot");
    let day_binding = day_slot
        .bindings
        .iter()
        .find(|binding| binding.name == "day")
        .expect("day slot should expose the day binding");

    assert_eq!(
        slot_binding_terminal_display(&project, "/src/App.vue", "day", "day").as_deref(),
        Some("Date"),
        "exact concrete publication must drive terminal display"
    );
    let day_binding_ty = shallow_published_type(
        project.host(),
        "/src/App.vue",
        day_binding.publication.result().selected_source(),
        "day slot binding",
    );
    assert!(
        matches!(day_binding_ty, TypeExpr::Ref { ref name, .. } if name.as_ref() == "Date"),
        "exact concrete publication must retain the resolved Date carrier"
    );
}

/// Issue #1 (partial): a slot binding whose resolved type is
/// `IndexedAccess { object: <project-local Props>, index: <literal> }`
/// must stay symbolic when the underlying property body resolves
/// through to an imported declaration that carries an open
/// `[k: string]: any` index signature. Otherwise the evaluator
/// re-expands the indexed access through the index signature and the
/// public surface widens to `any`.
///
/// Fixture shape:
///   * `/src/avatar.ts` exports `interface ImportedProps { src: string }`.
///   * `/src/Comp.vue`'s script-setup declares
///     `interface AppProps { avatar: ImportedProps & { [k: string]: any } }`
///     and `defineSlots<{ leading(props: { avatar: AppProps['avatar'] }): any }>()`.
///
/// The slot binding `avatar` must publish an `IndexedAccess` whose selected
/// resolved source remains structural instead of widening through `any`.
#[test]
fn slot_binding_imported_props_with_any_index_signature_stays_symbolic() {
    let project = make_project();
    project
        .upsert_base(
            "/src/avatar.ts",
            r#"
export interface ImportedProps {
  src: string
}
"#,
        )
        .unwrap();
    project
        .upsert_base(
            "/src/Comp.vue",
            r#"<script setup lang="ts">
import type { ImportedProps } from './avatar'

interface AppProps {
  avatar: ImportedProps & { [k: string]: any }
}

defineSlots<{
  leading(props: { avatar: AppProps['avatar'] }): any
}>()
</script>
<template><div /></template>"#,
        )
        .unwrap();

    let meta = project
        .host()
        .get_component_meta("/src/Comp.vue")
        .expect("should return component meta for the slot fixture");
    let leading_slot = meta
        .slots
        .iter()
        .find(|slot| slot.name == "leading")
        .expect("leading slot should be extracted");
    let avatar_binding = leading_slot
        .bindings
        .iter()
        .find(|binding| binding.name == "avatar")
        .expect("leading slot should expose the avatar binding");

    // The terminal display mirrors the structural publication without becoming
    // a semantic input.
    assert_eq!(
        slot_binding_terminal_display(&project, "/src/Comp.vue", "leading", "avatar").as_deref(),
        Some("{ avatar: ImportedProps & { [key: string]: any } }['avatar']"),
        "terminal display must render the selected structural source"
    );
    // Public type_expr stays as the indexed access — no expansion
    // through the imported `[k: string]: any` index signature.
    let avatar_binding_ty = shallow_published_type(
        project.host(),
        "/src/Comp.vue",
        avatar_binding.publication.result().selected_source(),
        "avatar slot binding",
    );
    assert!(
        matches!(&avatar_binding_ty, TypeExpr::IndexedAccess { .. }),
        "slot binding type must stay IndexedAccess (no widening through the imported index \
         signature); got {avatar_binding_ty:?}"
    );
}

/// Counterfixture for the slot-binding indexed-access policy: when the
/// indexed root is workspace-local AND non-imported AND not in a
/// route-preservation context, the policy should NOT preserve
/// `IndexedAccess` symbolically — the evaluator's expanded shape is
/// the intended public surface.
///
/// Fixture: `interface AppProps { kind: 'a' | 'b' }` declared in the
/// owner SFC's script-setup, with no imported helpers in the binding
/// chain. `defineSlots<{ leading(props: { kind: AppProps['kind'] }): any }>()`
/// publishes the union literal `'a' | 'b'` as the slot binding's
/// `type_expr` — symbolic preservation would suppress information the
/// consumer expects.
#[test]
fn slot_binding_local_props_without_index_signature_takes_slow_path() {
    let project = make_project();
    project
        .upsert_base(
            "/src/Comp.vue",
            r#"<script setup lang="ts">
interface AppProps {
  kind: 'a' | 'b'
}

defineSlots<{
  leading(props: { kind: AppProps['kind'] }): any
}>()
</script>
<template><div /></template>"#,
        )
        .unwrap();

    let meta = project
        .host()
        .get_component_meta("/src/Comp.vue")
        .expect("should return component meta for the slot counterfixture");
    let leading_slot = meta
        .slots
        .iter()
        .find(|slot| slot.name == "leading")
        .expect("leading slot should be extracted");
    let kind_binding = leading_slot
        .bindings
        .iter()
        .find(|binding| binding.name == "kind")
        .expect("leading slot should expose the kind binding");

    // The slow path expands the indexed access into the literal union
    // — that is the intended public surface for purely-local props
    // with no imported index signature in the chain. Symbolic
    // preservation here would suppress the resolved literal union the
    // consumer expects.
    let kind_binding_ty = shallow_published_type(
        project.host(),
        "/src/Comp.vue",
        kind_binding.publication.result().selected_source(),
        "kind slot binding",
    );
    assert!(
        !matches!(&kind_binding_ty, TypeExpr::IndexedAccess { .. }),
        "purely-local slot binding without imported helpers must take the slow path \
         (no symbolic IndexedAccess preservation); got {kind_binding_ty:?}"
    );
}

#[test]
fn imported_slot_binding_indexed_access_stays_symbolic_member_path() {
    let project = make_project();
    project
        .upsert_base(
            "/src/types.ts",
            r#"
type Id<T> = {} & { [P in keyof T]: T[P] }

export type ComponentUI<T extends { slots?: Record<string, any> }> = Id<{
  [K in keyof Required<T['slots']>]: (props?: Record<string, any>) => string
}>

export type ComponentConfig<T extends Record<string, any>> = {
  ui: ComponentUI<T>
}
"#,
        )
        .unwrap();
    project
        .upsert_base(
            "/src/theme.ts",
            r#"
export const theme = {
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
            "/src/button-types.ts",
            r#"
import type { ComponentConfig } from './types'
import { theme } from './theme'

export type Button = ComponentConfig<typeof theme>

export interface ButtonSlots {
  default?(props: {
    ui: Button['ui']
  }): any
}
"#,
        )
        .unwrap();
    project
        .upsert_base(
            "/src/App.vue",
            r#"<script setup lang="ts">
import type { ButtonSlots } from './button-types'

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
        .expect("should resolve component meta state");

    // Shallow-by-default registry contract: the imported `ButtonSlots` helper
    // stays a bare `Ref { name }` in the registry. The indexed-access slot
    // binding (`ui: Button['ui']`) resolves PATH-PRECISELY on the published
    // surface below — only the requested `ui` member path, never the whole
    // `Button` shape, enters the published binding.
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
        "registry entry must NOT eagerly materialize to an object, got {button_slots_ty:?}"
    );

    // Published-surface contract: the default slot's `ui` binding resolves to
    // the requested member path `Button['ui']` — symbolic IndexedAccess, NOT
    // an eagerly-widened object of the whole `Button` shape.
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
        Some("ComponentConfig<typeof theme>['ui']"),
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
fn resolve_component_meta_keeps_imported_slot_param_member_paths_symbolic_in_registry() {
    let project = make_project();
    project
        .upsert_base(
            "/src/types.ts",
            r#"
type Id<T> = {} & { [P in keyof T]: T[P] }

export type ComponentUI<T extends { slots?: Record<string, any> }> = Id<{
  [K in keyof Required<T['slots']>]: (props?: Record<string, any>) => string
}>

export type ComponentConfig<T extends Record<string, any>> = {
  ui: ComponentUI<T>
}
"#,
        )
        .unwrap();
    project
        .upsert_base(
            "/src/theme.ts",
            r#"
export const theme = {
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
            "/src/button-types.ts",
            r#"
import type { ComponentConfig } from './types'
import { theme } from './theme'

export type Button = ComponentConfig<typeof theme>

export interface ButtonSlots {
  default?(props: {
    ui: Button['ui']
  }): any
}
"#,
        )
        .unwrap();
    project
        .upsert_base(
            "/src/App.vue",
            r#"<script setup lang="ts">
import type { ButtonSlots } from './button-types'

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
        .expect("should resolve component meta state");

    // Shallow-by-default registry contract: the imported `ButtonSlots` helper
    // stays a bare `Ref { name }` in the registry; the deep slot-binding
    // correctness is asserted on the published surface below.
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
        "registry entry must NOT eagerly materialize to an object, got {button_slots_ty:?}"
    );

    let meta = project
        .host()
        .get_component_meta("/src/App.vue")
        .expect("should return component meta");
    let default_slot = meta
        .slots
        .iter()
        .find(|slot| slot.name == "default")
        .expect("default slot should still be extracted");
    let ui_binding = default_slot
        .bindings
        .iter()
        .find(|binding| binding.name == "ui")
        .expect("default slot should still expose the ui binding");
    assert_eq!(
        slot_binding_terminal_display(&project, "/src/App.vue", "default", "ui").as_deref(),
        Some("ComponentConfig<typeof theme>['ui']"),
        "terminal display must render the selected structural source"
    );
    // Resolution-authority contract: compute is the single resolution
    // authority. The meta carries the selected structural indexed-access
    // form; consumers navigate it via dispatch when they need the
    // resolved members (e.g. `base`, `label`). A regression that re-
    // introduces eager indexed-access resolution through imported
    // helper aliases would inline the members and fail this guard.
    // The binding type stays symbolic — same shape as the resolved
    // registry's `ButtonSlots.default` params asserted above.
    let ui_binding_ty = shallow_published_type(
        project.host(),
        "/src/App.vue",
        ui_binding.publication.result().selected_source(),
        "default slot ui binding",
    );
    assert!(
        matches!(&ui_binding_ty, TypeExpr::IndexedAccess { .. }),
        "post-Outcome-3: slot binding stays symbolic IndexedAccess, got {ui_binding_ty:?}"
    );
}

#[test]
fn resolve_component_meta_keeps_imported_intersection_slot_helpers_symbolic() {
    let project = make_project();
    project
        .upsert_base(
            "/src/types.ts",
            r#"
export interface Item {
  label?: string
}

export type DynamicSlots<T> = {
  [name: string]: (props: { item: T }) => any
}

export type MergeTypes<T> = T & {
  extra?: boolean
}

export type MenuSlots<T = Item> = {
  default?(props?: {}): any
  item?(props: { item: T }): any
} & DynamicSlots<MergeTypes<T>>
"#,
        )
        .unwrap();
    project
        .upsert_base(
            "/src/App.vue",
            r#"<script setup lang="ts">
import type { MenuSlots } from './types'

defineSlots<MenuSlots>()
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
        !registry_names.contains("DynamicSlots") && !registry_names.contains("MergeTypes"),
        "imported utility helpers should stay off the published registry, got {registry_names:?}"
    );
    // Shallow-by-default registry contract: the imported `MenuSlots` helper —
    // whose body is an intersection mixing explicit slot members with imported
    // utility helpers (`DynamicSlots<MergeTypes<T>>`) — stays a bare
    // `Ref { name }` in the registry. The explicit slot members are asserted on
    // the published surface below.
    let menu_slots = resolved
        .resolved_type_registry
        .iter()
        .find(|entry| entry.name == "MenuSlots")
        .expect("MenuSlots should be published in the resolved type registry");
    let menu_slots_ty = shallow_published_type(
        project.host(),
        "/src/App.vue",
        Some(menu_slots.type_source.present().expect("present source")),
        "MenuSlots registry entry",
    );
    assert!(
        matches!(&menu_slots_ty, TypeExpr::Ref { name, .. } if name.as_ref() == "MenuSlots"),
        "imported intersection slot helper should stay a shallow Ref (shallow-by-default), got {menu_slots_ty:?}"
    );
    assert!(
        !matches!(&menu_slots_ty, TypeExpr::Object(_)),
        "registry entry must NOT eagerly materialize the intersection surface, got {menu_slots_ty:?}"
    );

    // Published-surface contract: the explicit `default` and `item` slot
    // members survive on the published surface; the imported utility helpers
    // (`DynamicSlots` / `MergeTypes`) stay off the registry (asserted above).
    let meta = project
        .host()
        .get_component_meta("/src/App.vue")
        .expect("should return component meta");
    let slot_names: std::collections::BTreeSet<_> =
        meta.slots.iter().map(|slot| slot.name.as_str()).collect();
    assert!(
        slot_names.contains("default") && slot_names.contains("item"),
        "explicit imported slot members should still be exposed, got {slot_names:?}"
    );
}

#[test]
fn imported_slot_binding_prepared_decls_expose_generic_params_and_theme_value_decl() {
    let project = make_project();
    project
        .upsert_base(
            "/src/types.ts",
            r#"
type Id<T> = {} & { [P in keyof T]: T[P] }

export type ComponentUI<T extends { slots?: Record<string, any> }> = Id<{
  [K in keyof Required<T['slots']>]: (props?: Record<string, any>) => string
}>

export type ComponentConfig<T extends Record<string, any>> = {
  ui: ComponentUI<T>
}
"#,
        )
        .unwrap();
    project
        .upsert_base(
            "/src/theme.ts",
            r#"
export const theme = {
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
            "/src/button-types.ts",
            r#"
import type { ComponentConfig } from './types'
import { theme } from './theme'

export type Button = ComponentConfig<typeof theme>

export interface ButtonSlots {
  default?(props: {
    ui: Button['ui']
  }): any
}
"#,
        )
        .unwrap();
    project
        .upsert_base(
            "/src/App.vue",
            r#"<script setup lang="ts">
import type { ButtonSlots } from './button-types'

defineSlots<ButtonSlots>()
</script>
<template><div /></template>"#,
        )
        .unwrap();

    project
        .host()
        .resolve_component_meta(
            "/src/App.vue",
            verter_type_engine::semantic_query::ProjectionMode::Expanded,
        )
        .expect("component meta resolution should warm the prepared-decl route");

    let _store_view = project.host().resolver_store_view_read().into_owned_view();
    let component_config = project
        .host()
        .prepared_type_decl("/src/types.ts", "ComponentConfig")
        .expect("ComponentConfig should have a prepared declaration");
    assert_eq!(
        component_config
            .type_parameters
            .iter()
            .map(|param| param.name.as_str())
            .collect::<Vec<_>>(),
        vec!["T"]
    );

    let theme = project
        .host()
        .prepared_value_decl_in(
            "/src/theme.ts",
            verter_type_expr::TopLevelOwnerId::ordinary_file(),
            "theme",
        )
        .expect("theme should have a prepared value declaration");
    assert!(
        theme.type_annotation.classification
            != verter_type_expr::facts::ValueAnnotationClass::Absent
            || theme.object_shape.is_some(),
        "theme prepared value decl should expose an object surface for typeof"
    );
}

#[test]
fn local_pick_slot_bindings_keep_symbolic_terminal_display() {
    let project = make_project();
    project
        .upsert_base(
            "/src/App.vue",
            r#"<script lang="ts">
interface CalendarCellTriggerProps {
  day: Date,
  month: number
}

export interface CalendarSlots {
  day?: (props: Pick<CalendarCellTriggerProps, 'day'>) => any
}
</script>
<script setup lang="ts">
defineSlots<CalendarSlots>()
</script>
<template><div /></template>"#,
        )
        .unwrap();

    let meta = project
        .host()
        .get_component_meta("/src/App.vue")
        .expect("should return component meta");

    let day_slot = meta
        .slots
        .iter()
        .find(|slot| slot.name == "day")
        .expect("should extract local day slot");
    let day_binding = day_slot
        .bindings
        .iter()
        .find(|binding| binding.name == "day")
        .expect("day slot should expose the day binding");

    assert_eq!(
        slot_binding_terminal_display(&project, "/src/App.vue", "day", "day").as_deref(),
        Some("Date"),
        "exact concrete publication must drive terminal display"
    );
    let day_binding_ty = shallow_published_type(
        project.host(),
        "/src/App.vue",
        day_binding.publication.result().selected_source(),
        "day slot binding",
    );
    assert!(
        matches!(day_binding_ty, TypeExpr::Ref { ref name, .. } if name.as_ref() == "Date"),
        "exact concrete publication must retain the resolved Date carrier"
    );
}

#[test]
fn public_component_meta_materializes_local_component_config_variant_and_slot_helpers() {
    let project = make_project();
    project
        .upsert_base(
            "/src/tv.ts",
            r#"
type Id<T> = {} & { [P in keyof T]: T[P] }

type ComponentVariants<T extends { variants?: Record<string, Record<string, any>> }> = {
  [K in keyof T['variants']]: keyof T['variants'][K]
}

type ComponentSlots<T extends { slots?: Record<string, any> }> = Id<{
  [K in keyof T['slots']]?: string
}>

type ComponentUI<T extends { slots?: Record<string, any> }> = Id<{
  [K in keyof Required<T['slots']>]: (props?: Record<string, any>) => string
}>

export type ComponentConfig<T extends Record<string, any>> = {
  variants: ComponentVariants<T>,
  slots: ComponentSlots<T>
  ui: ComponentUI<T>
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
    variant: { solid: '', soft: '' }
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
import type { ComponentConfig } from './tv'
import theme from './theme'

type Button = ComponentConfig<typeof theme>

export interface ButtonProps {
  color?: Button['variants']['color']
  activeColor?: Button['variants']['color']
  ui?: Button['slots']
}

type ButtonSlots = {
  default?: (props: { ui: Button['ui'] }) => any
}
</script>
<script setup lang="ts">
defineProps<ButtonProps>()
defineSlots<ButtonSlots>()
</script>
<template><div /></template>"#,
        )
        .unwrap();

    project.host().set_import_dependencies(
        "/src/Button.vue",
        vec![
            crate::types::DependencyResolution {
                specifier: "./tv".to_string(),
                resolved_canonical_id: Some("/src/tv.ts".to_string()),
                possible_canonical_ids: Vec::new(),
            },
            crate::types::DependencyResolution {
                specifier: "./theme".to_string(),
                resolved_canonical_id: Some("/src/theme.ts".to_string()),
                possible_canonical_ids: Vec::new(),
            },
        ],
    );

    let session = project.open_session_batch().expect("session should open");
    let meta = session
        .get_component_meta("/src/Button.vue")
        .expect("component meta query should succeed")
        .expect("component meta should exist");

    assert_eq!(
        meta.props.len(),
        3,
        "should have exactly 3 props (color, activeColor, ui), got {:?}",
        meta.props.iter().map(|p| &p.name).collect::<Vec<_>>()
    );
    assert!(
        meta.events.is_empty(),
        "should have no events, got {:?}",
        meta.events.iter().map(|e| &e.name).collect::<Vec<_>>()
    );

    for prop_name in ["color", "activeColor"] {
        let prop = meta
            .props
            .iter()
            .find(|prop| prop.name == prop_name)
            .expect("variant prop should exist");
        let prop_ty = demand_published_type(
            project.host(),
            "/src/Button.vue",
            prop.publication.result().selected_source(),
            prop_name,
        );
        assert_union_string_literals(&prop_ty, &["primary", "secondary"]);
        assert!(
            !matches!(&prop_ty, TypeExpr::Unknown { .. }),
            "variant prop should not degrade to Unknown"
        );
    }

    let ui = meta
        .props
        .iter()
        .find(|prop| prop.name == "ui")
        .expect("ui prop should exist");
    let ui_ty = demand_published_type(
        project.host(),
        "/src/Button.vue",
        ui.publication.result().selected_source(),
        "ui prop",
    );
    let TypeExpr::Object(ui_shape) = &ui_ty else {
        panic!("component-config slots helper should materialize as an object, got {ui_ty:?}");
    };
    assert_eq!(
        ui_shape.properties.len(),
        2,
        "ui prop should have exactly 2 properties (base, label)"
    );
    assert!(
        ui_shape.properties.iter().any(
            |member| matches!(member, ObjectMember::Property(property) if property.string_name().expect("string-key fixture") == "base"),
        ),
        "ui helper should expose base, got {ui_ty:?}"
    );
    assert!(
        ui_shape.properties.iter().any(
            |member| matches!(member, ObjectMember::Property(property) if property.string_name().expect("string-key fixture") == "label"),
        ),
        "ui helper should expose label, got {ui_ty:?}"
    );

    assert_eq!(meta.slots.len(), 1, "should have exactly 1 slot (default)");
    let default_slot = meta
        .slots
        .iter()
        .find(|slot| slot.name == "default")
        .expect("default slot should exist");
    assert_eq!(
        default_slot.bindings.len(),
        1,
        "default slot should have exactly 1 binding (ui)"
    );
    let ui_binding = default_slot
        .bindings
        .iter()
        .find(|binding| binding.name == "ui")
        .expect("default slot should expose ui");
    let ui_binding_ty = demand_published_type(
        project.host(),
        "/src/Button.vue",
        ui_binding.publication.result().selected_source(),
        "default slot ui binding",
    );
    let TypeExpr::Object(binding_shape) = &ui_binding_ty else {
        panic!("slot ui binding should materialize as an object, got {ui_binding_ty:?}");
    };
    assert_eq!(
        binding_shape.properties.len(),
        2,
        "slot ui binding should have exactly 2 properties (base, label)"
    );
    assert!(
        binding_shape.properties.iter().any(
            |member| matches!(member, ObjectMember::Property(property) if property.string_name().expect("string-key fixture") == "base"),
        ),
        "slot ui binding should expose base, got {ui_binding_ty:?}"
    );
    assert!(
        binding_shape.properties.iter().any(
            |member| matches!(member, ObjectMember::Property(property) if property.string_name().expect("string-key fixture") == "label"),
        ),
        "slot ui binding should expose label, got {ui_binding_ty:?}"
    );
}

#[test]
fn public_component_meta_materializes_component_config_app_config_variant_and_slot_helpers() {
    let project = make_project();
    project
        .upsert_base(
            "/node_modules/@nuxt/schema/index.d.ts",
            r#"
export interface AppConfig {
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
            "/src/tv.ts",
            r#"
type Id<T> = {} & { [P in keyof T]: T[P] }

type ComponentVariants<T extends { variants?: Record<string, Record<string, any>> }> = {
  [K in keyof T['variants']]: keyof T['variants'][K]
}

type ComponentSlots<T extends { slots?: Record<string, any> }> = Id<{
  [K in keyof T['slots']]?: string
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
            "/src/theme.ts",
            r#"export default {
  variants: {
    color: { primary: '', secondary: '' },
    variant: { solid: '', soft: '' }
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
import type { AppConfig } from '@nuxt/schema'
import type { ComponentConfig } from './tv'
import theme from './theme'

type Button = ComponentConfig<typeof theme, AppConfig, 'button'>

export interface ButtonProps {
  color?: Button['variants']['color']
  activeColor?: Button['variants']['color']
  ui?: Button['slots']
}

type ButtonSlots = {
  default?: (props: { ui: Button['ui'] }) => any
}
</script>
<script setup lang="ts">
defineProps<ButtonProps>()
defineSlots<ButtonSlots>()
</script>
<template><div /></template>"#,
        )
        .unwrap();

    project.host().set_import_dependencies(
        "/src/Button.vue",
        vec![
            crate::types::DependencyResolution {
                specifier: "@nuxt/schema".to_string(),
                resolved_canonical_id: Some("/node_modules/@nuxt/schema/index.d.ts".to_string()),
                possible_canonical_ids: Vec::new(),
            },
            crate::types::DependencyResolution {
                specifier: "./tv".to_string(),
                resolved_canonical_id: Some("/src/tv.ts".to_string()),
                possible_canonical_ids: Vec::new(),
            },
            crate::types::DependencyResolution {
                specifier: "./theme".to_string(),
                resolved_canonical_id: Some("/src/theme.ts".to_string()),
                possible_canonical_ids: Vec::new(),
            },
        ],
    );

    let session = project.open_session_batch().expect("session should open");
    let meta = session
        .get_component_meta("/src/Button.vue")
        .expect("component meta query should succeed")
        .expect("component meta should exist");

    assert_eq!(
        meta.props.len(),
        3,
        "should have exactly 3 props (color, activeColor, ui), got {:?}",
        meta.props.iter().map(|p| &p.name).collect::<Vec<_>>()
    );
    assert!(
        meta.events.is_empty(),
        "should have no events, got {:?}",
        meta.events.iter().map(|e| &e.name).collect::<Vec<_>>()
    );

    for prop_name in ["color", "activeColor"] {
        let prop = meta
            .props
            .iter()
            .find(|prop| prop.name == prop_name)
            .expect("variant prop should exist");
        let prop_ty = demand_published_type(
            project.host(),
            "/src/Button.vue",
            prop.publication.result().selected_source(),
            prop_name,
        );
        assert_union_string_literals(&prop_ty, &["primary", "secondary", "neutral"]);
        assert!(
            !matches!(&prop_ty, TypeExpr::Unknown { .. }),
            "variant prop should not degrade to Unknown"
        );
    }

    let ui = meta
        .props
        .iter()
        .find(|prop| prop.name == "ui")
        .expect("ui prop should exist");
    let ui_ty = demand_published_type(
        project.host(),
        "/src/Button.vue",
        ui.publication.result().selected_source(),
        "ui prop",
    );
    let TypeExpr::Object(ui_shape) = &ui_ty else {
        panic!("component-config slots helper should materialize as an object, got {ui_ty:?}");
    };
    assert_eq!(
        ui_shape.properties.len(),
        2,
        "ui prop should have exactly 2 properties (base, label)"
    );
    assert!(
        ui_shape.properties.iter().any(
            |member| matches!(member, ObjectMember::Property(property) if property.string_name().expect("string-key fixture") == "base"),
        ),
        "ui helper should expose base, got {ui_ty:?}"
    );
    assert!(
        ui_shape.properties.iter().any(
            |member| matches!(member, ObjectMember::Property(property) if property.string_name().expect("string-key fixture") == "label"),
        ),
        "ui helper should expose label, got {ui_ty:?}"
    );

    assert_eq!(meta.slots.len(), 1, "should have exactly 1 slot (default)");
    let default_slot = meta
        .slots
        .iter()
        .find(|slot| slot.name == "default")
        .expect("default slot should exist");
    assert_eq!(
        default_slot.bindings.len(),
        1,
        "default slot should have exactly 1 binding (ui)"
    );
    let ui_binding = default_slot
        .bindings
        .iter()
        .find(|binding| binding.name == "ui")
        .expect("default slot should expose ui");
    let ui_binding_ty = demand_published_type(
        project.host(),
        "/src/Button.vue",
        ui_binding.publication.result().selected_source(),
        "default slot ui binding",
    );
    let TypeExpr::Object(binding_shape) = &ui_binding_ty else {
        panic!("slot ui binding should materialize as an object, got {ui_binding_ty:?}");
    };
    assert_eq!(
        binding_shape.properties.len(),
        2,
        "slot ui binding should have exactly 2 properties (base, label)"
    );
    assert!(
        binding_shape.properties.iter().any(
            |member| matches!(member, ObjectMember::Property(property) if property.string_name().expect("string-key fixture") == "base"),
        ),
        "slot ui binding should expose base, got {ui_binding_ty:?}"
    );
    assert!(
        binding_shape.properties.iter().any(
            |member| matches!(member, ObjectMember::Property(property) if property.string_name().expect("string-key fixture") == "label"),
        ),
        "slot ui binding should expose label, got {ui_binding_ty:?}"
    );
}

#[test]
fn payload_cache_get_resolved_reuses_full_slot() {
    let project = make_project();
    project
        .upsert_base(
            "/Comp.vue",
            r#"<script setup lang="ts">
defineProps<{ msg: string }>()
</script>
<template><div /></template>"#,
        )
        .unwrap();

    let session = project.open_session_batch().unwrap();

    // First call — full/resolved — miss.
    let p1 = session
        .get_component_meta_payload("/Comp.vue", test_encode_fn)
        .expect("should succeed")
        .expect("should return payload");

    let prov1 = provenance(&project);
    assert_eq!(prov1.payload_encodes, 1);
    assert_eq!(prov1.payload_cache_misses, 1);

    // Second call — same slot — hit.
    let p2 = session
        .get_component_meta_payload("/Comp.vue", test_encode_fn)
        .expect("should succeed")
        .expect("should return payload");

    let prov2 = provenance(&project);
    assert_eq!(p1, p2, "resolved reuses the full payload slot");
    assert_eq!(prov2.payload_cache_hits, 1);
    assert_eq!(prov2.payload_encodes, 1, "no new encode on warm hit");
}

// ---------------------------------------------------------------------------
// Real-shape regression tests for the semantic-DB resolver.
// ---------------------------------------------------------------------------

/// Real nuxt-ui DynamicSlots pattern with conditional template-literal keys
/// and `Extract` in the mapped value. This is the pattern that causes solver
/// explosion via O(N^2) conditional distribution.
#[test]
fn get_component_meta_dynamic_slots_real_shape_accordion() {
    let project = make_project();
    project
        .upsert_base(
            "/utils.ts",
            r#"export type DynamicSlotsKeys<
  Name extends string | undefined,
  Suffix extends string | undefined = undefined
> = (
  Name extends string
    ? Suffix extends string
      ? Name | `${Name}-${Suffix}`
      : Name
    : never,
)

export type DynamicSlots<
  T extends { slot?: string },
  Suffix extends string | undefined = undefined,
  ExtraProps extends object = {}
> = {
  [K in DynamicSlotsKeys<T['slot'], Suffix>]?: (
    props: { item: Extract<T, { slot: K extends `${infer Base}-${Suffix}` ? Base : K }> } & ExtraProps,
  ) => any[]
}"#,
        )
        .unwrap();
    project
        .upsert_base(
            "/Accordion.vue",
            r#"<script setup lang="ts">
import type { DynamicSlots } from './utils'

type AccordionItem = { slot?: 'default' | 'leading' | 'trailing' }

interface AccordionSlots extends DynamicSlots<AccordionItem, 'body', { index: number; open: boolean }> {
  default(props: { item: AccordionItem }): any
  leading?(): any
  trailing?(): any
}

defineSlots<AccordionSlots>()
</script>
<template><div /></template>"#,
        )
        .unwrap();

    let meta = get_meta(&project, "/Accordion.vue");
    let slot_names: Vec<&str> = meta.slots.iter().map(|slot| slot.name.as_str()).collect();

    // Named slots from the interface should survive.
    assert!(
        slot_names.contains(&"default"),
        "real-shape DynamicSlots accordion must keep 'default' slot, got: {slot_names:?}"
    );
    assert!(
        slot_names.contains(&"leading"),
        "real-shape DynamicSlots accordion must keep 'leading' slot, got: {slot_names:?}"
    );
    assert!(
        slot_names.contains(&"trailing"),
        "real-shape DynamicSlots accordion must keep 'trailing' slot, got: {slot_names:?}"
    );

    // Helper internals must NOT leak into the public surface.
    assert!(
        !slot_names.iter().any(|n| *n == "item" || *n == "slot"),
        "DynamicSlots helper internals must not leak into slot surface: {slot_names:?}"
    );
}

/// NO-CACHE-POISON characterization: the publication reduce path performs NO
/// TypeExpr-subject `ShapeCacheDb` admission at all — the former
/// `Leaf`-admit / `BareCarrier`-no-admit split is structurally gone with the
/// TypeExpr-start reducer, so the shared TypeExpr-start whole-subject slot can
/// no longer be poisoned from the publication loops. Discrimination: re-introducing ANY
/// admission call (`admit_type_expr_shape_if_possible` / `admit_computed`)
/// under `reduce_published_field_types` or `reduce_field_value_node` FAILS
/// here (both idents were present on the pre-change tree's reduce path).
#[test]
fn publication_reduce_path_admits_nothing_into_the_shared_type_expr_slot() {
    const OUTPUT_SINK_SRC: &str = include_str!("../../meta_resolve/projectors/output_sink.rs");
    const PUBLISHED_FINALIZE_SRC: &str =
        include_str!("../../meta_resolve/projectors/output_sink/published_finalize.rs");

    for (source, target) in [
        (PUBLISHED_FINALIZE_SRC, "reduce_published_field_types"),
        (OUTPUT_SINK_SRC, "reduce_field_value_node"),
    ] {
        let calls = output_sink_calls_in(source, target);
        for forbidden in ["admit_type_expr_shape_if_possible", "admit_computed"] {
            assert!(
                !calls.contains(forbidden),
                "`{target}` must NOT admit into a shared cache slot via `{forbidden}` — the \
                 publication reduce path is admission-free (no-poison); calls seen: {calls:?}"
            );
        }
    }
}

/// End-to-end (`get_component_meta`) member-visibility leak guard for slot
/// bindings whose first parameter is a CLASS carrying non-public members. This
/// drives the FULL component-meta pipeline, so it covers BOTH the typeinfo
/// adapter binding path (`binding_fields_from_param_node`) AND the graph-native
/// binding path (`slot_binding_graph::compute_bindings_via_graph` ->
/// `publish_merged_bindings`); both must apply the Public-only publication
/// filter so a navigated class param's `private` / `protected` member never
/// reaches the published slot binding surface.
///
/// Discriminating: against the tree without the graph-native + adapter
/// slot-binding filters, `protectedBinding` / `privateBinding` appear in the
/// published bindings and the `does-not-contain` assertions FAIL.
#[test]
fn slot_binding_navigated_class_param_excludes_non_public_members_end_to_end() {
    let project = make_project();
    project
        .upsert_base(
            "/slot-props.ts",
            r#"export class SlotProps {
  publicBinding: string
  protected protectedBinding: number
  private privateBinding: boolean
}"#,
        )
        .unwrap();
    project
        .upsert_base(
            "/App.vue",
            r#"<script setup lang="ts">
import type { SlotProps } from './slot-props'
defineSlots<{ default(props: SlotProps): any }>()
</script>
<template><div /></template>"#,
        )
        .unwrap();

    let meta = project
        .host()
        .get_component_meta("/App.vue")
        .expect("component meta with class-param slot resolves");

    let default_slot = meta
        .slots
        .iter()
        .find(|slot| slot.name == "default")
        .expect("default slot must be published");
    let binding_names: Vec<&str> = default_slot
        .bindings
        .iter()
        .map(|binding| binding.name.as_str())
        .collect();

    assert!(
        binding_names.contains(&"publicBinding"),
        "the public class-param member must publish as a slot binding; got {binding_names:?}",
    );
    assert!(
        !binding_names.contains(&"protectedBinding"),
        "a protected class-param member must NOT leak into published slot bindings \
         (adapter OR graph-native path); got {binding_names:?}",
    );
    assert!(
        !binding_names.contains(&"privateBinding"),
        "a private class-param member must NOT leak into published slot bindings \
         (adapter OR graph-native path); got {binding_names:?}",
    );
}

#[test]
fn cross_file_slot_jsdoc_publishes() {
    let project = make_project();
    project
        .upsert_base(
            "/src/slots.ts",
            r#"
export interface CardSlots {
  /** Main card body content. */
  default(props: { item: string }): any
  footer?(props: {}): any
}
"#,
        )
        .unwrap();
    project
        .upsert_base(
            "/src/Card.vue",
            r#"<script setup lang="ts">
import type { CardSlots } from './slots'

defineSlots<CardSlots>()
</script>
<template><div><slot /></div></template>"#,
        )
        .unwrap();

    let meta = project
        .host()
        .get_component_meta("/src/Card.vue")
        .expect("component meta resolves");

    let default_slot = meta
        .slots
        .iter()
        .find(|slot| slot.name == "default")
        .expect("default slot must surface");
    assert_eq!(
        default_slot.description.as_deref(),
        Some("Main card body content."),
        "imported slot's JSDoc description must publish"
    );

    // Negative: an undocumented slot publishes NO description.
    let footer = meta
        .slots
        .iter()
        .find(|slot| slot.name == "footer")
        .expect("footer slot must surface");
    assert_eq!(
        footer.description.as_deref(),
        None,
        "undocumented slot must not gain a fabricated description"
    );
}

/// REQUEST-LOCAL MEMO WORK canary: for a LARGE source repeated across
/// lanes the memo performs EXACTLY ONE full-source hash traversal per
/// populated lane slot (the single-hash borrowed-key `entry` route) while
/// still materializing the source ONCE — the per-slot hash/clone work is
/// O(lanes × 1 traversal), never a get-then-insert SECOND traversal per
/// miss and never a per-lookup owned key clone (structurally impossible:
/// the memo key borrows the scope + source; there is no owned source in
/// the key type to clone).
///
/// Discriminating: a get-then-insert memo route re-hashes every NON-EMPTY
/// miss on the insert side (with two distinct sources the second source's
/// miss pays get + insert = 2 traversals) and fails the exact-equality
/// assert.
#[test]
fn output_memo_hash_work_is_one_traversal_per_lane_slot() {
    use verter_type_expr::facts::{FactOrLocator, LeafTypeFact, ResolvedLocalShape};
    let project = make_project();
    project
        .upsert_base("/App.vue", "<template><div /></template>")
        .unwrap();
    let host = project.host();

    // ONE large composite source (64 members) repeated across half the
    // lane rows — the worst case for per-lookup deep hash/clone work —
    // plus a second DISTINCT source on the other half, so an insert-side
    // re-hash of a non-empty-map miss cannot hide behind the empty-map
    // fast path.
    let members: Vec<verter_type_expr::facts::SynthesizedMemberFact> = (0..64)
        .map(|i| verter_type_expr::facts::SynthesizedMemberFact {
            name: format!("member{i}"),
            optional: false,
            ty: FactOrLocator::Leaf(LeafTypeFact::Primitive(
                verter_type_expr::PrimitiveName::String,
            )),
            span_origin: verter_type_expr::span_origins::MemberSpansOrigin::Synthetic(
                verter_type_expr::span_origins::SourceSynthetic,
            ),
        })
        .collect();
    let large = verter_type_expr::facts::SemanticTypeSource::Synthesized(
        ResolvedLocalShape::Object(Arc::from(members)),
    );
    let distinct = closed_ref_source("DistinctAlias");

    const LANES: usize = 6;
    let mut analysis = blank_output_analysis();
    for i in 0..LANES {
        let source = if i < LANES / 2 { &large } else { &distinct };
        analysis.props.push(
            verter_session_query::analysis::component_meta::PropAnalysis {
                name: format!("p{i}"),
                callable_role: verter_type_expr::PropCallableRole::default(),
                publication: crate::test_only::type_publication_fixture(
                    verter_type_expr::facts::SourcePosition::Present(source.clone()),
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
    }

    let fixture_dispatch_21 =
        verter_type_engine::project_semantic_dispatch::ProjectSemanticDispatch::new(host);
    let output = crate::meta_resolve::projectors::build_component_meta_output(
        host,
        &fixture_dispatch_21,
        "/App.vue",
        analysis,
        None,
        crate::meta_resolve::PublishedCompleteness::COMPLETE,
    )
    .expect("the repeated composite source materializes");
    let calls =
        crate::meta_resolve::projectors::LAST_OUTPUT_MATERIALIZE_CALLS.with(std::cell::Cell::get);
    assert_eq!(
        calls, 2,
        "{LANES} lane slots over TWO source identities materialize exactly twice"
    );
    let hash_ops =
        crate::meta_resolve::projectors::LAST_OUTPUT_MEMO_HASH_OPS.with(std::cell::Cell::get);
    assert_eq!(
        hash_ops, LANES as u64,
        "each populated lane slot performs EXACTLY ONE full-source hash \
         traversal — no insert-side second hash, no per-lookup key clone \
         (structurally impossible: the memo key borrows the source)"
    );
    let lanes = output.into_parts().2.into_lanes();
    assert_eq!(lanes.props.len(), LANES);
    assert_eq!(lanes.props[0], lanes.props[LANES / 2 - 1]);
}

/// A stable unresolved SLOTS member remains Complete but does not become a
/// fabricated callable slot.
#[test]
fn slot_member_referencing_unresolved_type_is_complete_but_not_callable() {
    let project = make_project();
    project
        .upsert_base(
            "/bad-slots.ts",
            "export interface BadSlots { item: MissingType }\n",
        )
        .unwrap();
    project
        .upsert_base(
            "/App.vue",
            r#"<script setup lang="ts">
import type { BadSlots } from './bad-slots'
defineSlots<BadSlots>()
</script>
<template><div /></template>"#,
        )
        .unwrap();

    let (analysis, state) = project
        .host()
        .get_component_meta_with_resolution("/App.vue")
        .expect("the analysis resolves");
    assert!(
        !analysis.slots.iter().any(|slot| slot.name == "item"),
        "a bare unresolved carrier is not fabricated into a callable slot"
    );
    assert!(
        !state.completeness.is_partial(),
        "a stable unresolved slot carrier is Complete; got {:?}",
        state.completeness
    );
    assert!(
        !state.synthesis_should_suppress,
        "a stable unresolved slot carrier does not suppress warm admission"
    );
}

/// The SLOTS callable view may retain the known callable arm as its best safe
/// projection. A stable unresolved non-callable arm does not turn that view
/// into an operational partial or suppress cache admission.
#[test]
fn same_name_intersection_slot_keeps_callable_projection_without_partiality() {
    let project = make_project();
    project
        .upsert_base(
            "/bad-slots.ts",
            "export interface BadSlots { item: MissingType }\n",
        )
        .unwrap();
    project
        .upsert_base(
            "/App.vue",
            r#"<script setup lang="ts">
import type { BadSlots } from './bad-slots'
defineSlots<{ item(props: { a: string }): any } & BadSlots>()
</script>
<template><div /></template>"#,
        )
        .unwrap();

    let (analysis, state) = project
        .host()
        .get_component_meta_with_resolution("/App.vue")
        .expect("component resolves");
    let slot = analysis
        .slots
        .iter()
        .find(|slot| slot.name == "item")
        .expect("the known callable contributor remains available as the best safe slot view");
    assert!(
        slot.bindings.iter().any(|binding| binding.name == "a"),
        "the callable projection retains its authored binding; got {:?}",
        slot.bindings
    );
    assert!(
        !state.completeness.is_partial(),
        "a stable unresolved non-callable arm does not make the callable slot projection partial; got {:?}",
        state.completeness
    );
    assert!(
        !state.synthesis_should_suppress,
        "stable unresolved carriers do not suppress warm admission"
    );
}

#[test]
fn normal_script_type_alias_wins_over_setup_runtime_import_in_slot_binding() {
    let project = make_project();
    project
        .upsert_base("/runtime.ts", "export const Separator = {}\n")
        .unwrap();
    project
        .upsert_base(
            "/Separator.vue",
            r#"<script lang="ts">
type Separator = { ui: { root: string } }
export interface SeparatorSlots {
  default?(props: { ui: Separator['ui'] }): unknown
}
</script>
<script setup lang="ts">
import { Separator } from './runtime'
defineSlots<SeparatorSlots>()
</script>
<template><Separator><slot :ui="{}" /></Separator></template>"#,
        )
        .unwrap();

    let (analysis, _resolution, types) = project
        .host()
        .get_component_meta_output("/Separator.vue")
        .expect("the slot binding resolves in the normal script's type namespace")
        .expect("component resolves")
        .into_parts();
    let slot_index = analysis
        .slots
        .iter()
        .position(|slot| slot.name == "default")
        .expect("default slot publishes");
    let binding_index = analysis.slots[slot_index]
        .bindings
        .iter()
        .position(|binding| binding.name == "ui")
        .expect("ui binding publishes");
    let lanes = types.into_lanes();
    assert!(
        matches!(
            lanes.slot_bindings[slot_index][binding_index].materialized_type().expect("published type"),
            TypeExpr::SyntheticSlotBinding(carrier)
                if carrier.slot_name.as_deref() == Some("default")
                    && carrier.binding_name.as_ref() == "ui"
        ),
        "the public lane retains the shallow, re-demandable slot-binding carrier; got {:?}",
        lanes.slot_bindings[slot_index][binding_index]
    );
    let demanded = demand_published_type(
        project.host(),
        "/Separator.vue",
        analysis.slots[slot_index].bindings[binding_index]
            .publication
            .result()
            .selected_source(),
        "default.ui slot binding",
    );
    let TypeExpr::Object(object) = &demanded else {
        panic!("the local Separator['ui'] binding resolves to its object; got {demanded:?}");
    };
    assert!(
        object.properties.iter().any(|property| {
            matches!(
                property,
                verter_type_expr::ObjectMember::Property(property)
                    if property.string_name().expect("string-key fixture") == "root"
                        && matches!(property.ty, TypeExpr::Primitive(PrimitiveName::String))
            )
        }),
        "the local ui object retains its root:string member; got {object:?}"
    );
}

/// The Popover.vue corpus shape: `anchor` is declared on the component's
/// own `defineSlots` surface (a referenced interface's own body). The
/// producer fact `declared_in_macro_type_arg` rides the analysis so the
/// compat slot blocklist can exempt author-declared VNode-transport
/// names (an undeclared `anchor` would still be suppressed).
#[test]
fn define_slots_surface_slots_carry_declared_in_macro_type_arg() {
    let project = make_project();
    project
        .upsert_base(
            "/App.vue",
            r#"<script lang="ts">
export interface AppSlots {
  default?(props: { open: boolean }): any
  anchor?(props: { open: boolean }): any
}
</script>
<script setup lang="ts">
defineSlots<AppSlots>()
</script>
<template><div /></template>"#,
        )
        .unwrap();

    let meta = project
        .host()
        .get_component_meta("/App.vue")
        .expect("full meta should resolve");
    let anchor = meta
        .slots
        .iter()
        .find(|slot| slot.name == "anchor")
        .expect("the declared `anchor` slot publishes natively");
    assert!(
        anchor.declared_in_macro_type_arg,
        "a slot declared on the component's own defineSlots surface carries \
         the producer fact — the compat blocklist must never suppress it"
    );
    let default_slot = meta
        .slots
        .iter()
        .find(|slot| slot.name == "default")
        .expect("default slot publishes");
    assert!(
        default_slot.declared_in_macro_type_arg,
        "every authored defineSlots member carries the fact"
    );
}

/// Template-declared sibling: an authored `<slot name=...>` element is an
/// author declaration too — the fact holds without any `defineSlots`.
#[test]
fn template_slots_carry_declared_in_macro_type_arg() {
    let project = make_project();
    project
        .upsert_base(
            "/App.vue",
            r#"<script setup lang="ts">
</script>
<template><slot name="anchor" /></template>"#,
        )
        .unwrap();

    let meta = project
        .host()
        .get_component_meta("/App.vue")
        .expect("full meta should resolve");
    let anchor = meta
        .slots
        .iter()
        .find(|slot| slot.name == "anchor")
        .expect("template-declared slot publishes");
    assert!(
        anchor.declared_in_macro_type_arg,
        "an authored template `<slot>` element declares the name"
    );
}
