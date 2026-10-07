use super::*;

// The import-route witness emits only when the owner's authored
// specifiers resolve through an admitting transaction; the live-host
// path has no equivalent to "under store view" semantics. Stale deps
// are rejected at warm-read time by
// `StoreView::validates_fact_signature` revalidating the recorded fact
// signature against the live host.
#[test]
fn current_dependency_fact_versions_emits_import_route_witness_when_cache_populated() {
    let project = make_project();
    project
        .upsert_base("/theme.ts", r#"export default { color: "red" }"#)
        .expect("theme upsert");
    project
        .upsert_base(
            "/Comp.vue",
            r#"<script setup lang="ts">
import theme from './theme'
defineProps<{ ui: typeof theme }>()
</script>"#,
        )
        .expect("upsert should succeed");

    // Exercise the shallow file state pipeline so import_routes are populated
    // on the compile cache.
    let _ = project.host().resolve_component_meta(
        "/Comp.vue",
        verter_type_engine::semantic_query::ProjectionMode::Identity,
    );

    let facts = project
        .host()
        .current_dependency_fact_versions("/Comp.vue", &std::collections::BTreeSet::new());

    assert!(
        facts.iter().any(|fact| matches!(
            fact,
            verter_session_query::facts::fact_cache::FactVersionRef::ResolveImports(inner)
                if inner.resolution_fact().is_some()
        )) || facts.iter().any(|fact| matches!(
            fact,
            verter_session_query::facts::fact_cache::FactVersionRef::FileWholeHash { .. }
        )),
        "live-host fact capture should emit either an import-route resolution \
         witness or a FileWholeHash fact for tracked dependencies",
    );
}

/// PUBLIC BOUNDARY — a call-signature `defineEmits` whose EVENT-NAME position
/// cannot be fully enumerated never publishes the emit surface as COMPLETE and
/// never warms. Two spellings of the same drop:
///
/// 1. the event-name parameter typed by an unresolvable import — the whole
///    name enumeration fails;
/// 2. a union name position with one authored RESOLVABLE literal beside an
///    unresolvable arm — fail-closed-whole still refuses the enumeration, but
///    the refusal must be a typed partial: the authored `'a'` contributor is
///    dropped, which is only sound when the result says it is incomplete.
///
/// The SAME unresolvable import on the props lane fails closed; the emit lane
/// must not be the one producer that spells the identical failure as an empty
/// complete surface.
///
/// CONTROL: a resolvable literal event name publishes, stays COMPLETE, and
/// WARMS.
#[cfg(not(target_arch = "wasm32"))]
#[test]
fn an_unresolvable_emit_event_name_never_publishes_complete_or_warm() {
    use std::sync::atomic::Ordering::Relaxed;

    // 1. The whole name position behind an unresolvable import.
    let project = make_project();
    project
        .upsert_base(
            "/src/EmitNameMissing.vue",
            r#"<script setup lang="ts">
import type { E } from './missing'
defineEmits<{ (e: E, v: number): void }>()
</script>
<template><div /></template>"#,
        )
        .unwrap();
    let host = project.host();
    let _ = get_meta(&project, "/src/EmitNameMissing.vue");
    let (_, resolved) = host
        .get_component_meta_with_resolution("/src/EmitNameMissing.vue")
        .expect("the resolve must still return metadata");
    assert!(
        resolved.synthesis_should_suppress,
        "an emit surface whose event-name position is unresolvable must NOT \
         report COMPLETE — the empty emit set is a failed enumeration, not an \
         authored empty surface"
    );
    let hits_before = host
        .provenance()
        .component_meta_result_cache_hits
        .load(Relaxed);
    let _ = get_meta(&project, "/src/EmitNameMissing.vue");
    let hits_after = host
        .provenance()
        .component_meta_result_cache_hits
        .load(Relaxed);
    assert_eq!(
        hits_after, hits_before,
        "an unresolvable emit event-name surface must NOT warm"
    );

    // 2. A resolvable authored literal beside an unresolvable union arm: the
    // fail-closed-whole enumeration drops the authored `'a'`, so the result
    // must say PARTIAL — never complete-and-warm.
    let project = make_project();
    project
        .upsert_base(
            "/src/EmitNameUnionMissing.vue",
            r#"<script setup lang="ts">
import type { Unknown1 } from './missing'
defineEmits<{ (e: 'a' | Unknown1, v: number): void }>()
</script>
<template><div /></template>"#,
        )
        .unwrap();
    let host = project.host();
    let _ = get_meta(&project, "/src/EmitNameUnionMissing.vue");
    let (_, resolved) = host
        .get_component_meta_with_resolution("/src/EmitNameUnionMissing.vue")
        .expect("the resolve must still return metadata");
    assert!(
        resolved.synthesis_should_suppress,
        "dropping the authored resolvable 'a' over an unresolvable union arm \
         must publish a typed PARTIAL, never an empty complete emit set"
    );
    let hits_before = host
        .provenance()
        .component_meta_result_cache_hits
        .load(Relaxed);
    let _ = get_meta(&project, "/src/EmitNameUnionMissing.vue");
    let hits_after = host
        .provenance()
        .component_meta_result_cache_hits
        .load(Relaxed);
    assert_eq!(
        hits_after, hits_before,
        "a partially-enumerable emit name union must NOT warm"
    );

    // CONTROL: the resolvable literal name publishes complete and warms.
    let project = make_project();
    project
        .upsert_base(
            "/src/EmitNameOk.vue",
            r#"<script setup lang="ts">
defineEmits<{ (e: 'save', v: number): void }>()
</script>
<template><div /></template>"#,
        )
        .unwrap();
    let host = project.host();
    let meta = get_meta(&project, "/src/EmitNameOk.vue");
    assert!(
        meta.events.iter().any(|event| event.name == "save"),
        "the control publishes the authored 'save' event; got {:?}",
        meta.events.iter().map(|e| &e.name).collect::<Vec<_>>()
    );
    let (_, resolved) = host
        .get_component_meta_with_resolution("/src/EmitNameOk.vue")
        .expect("the control resolve returns metadata");
    assert!(
        !resolved.synthesis_should_suppress,
        "the resolvable-name control is COMPLETE"
    );
    let hits_before = host
        .provenance()
        .component_meta_result_cache_hits
        .load(Relaxed);
    let _ = get_meta(&project, "/src/EmitNameOk.vue");
    let hits_after = host
        .provenance()
        .component_meta_result_cache_hits
        .load(Relaxed);
    assert_eq!(hits_after, hits_before + 1, "the control WARMS on replay");
}

/// PUBLIC BOUNDARY, RENDERED BYTES — the TSX lane emits for EVERY flow-return
/// degradation class, including the two the runtime lane refuses.
///
/// The TSC projection splices the AUTHORED declaration and the authored type
/// argument into the generated TSX and lets the external checker compute the
/// member types. A body-derived return THIS substrate could not infer, could
/// not verify, or could not produce at all therefore says nothing about
/// whether the TSX is the full surface — faulting on any of them deleted the
/// whole file's type-check surface for programs tsgo types without
/// difficulty.
///
/// The lane is driven through `ensure_ide_compiled` + `get_ide`, which is the
/// only way to reach the `CachedTsx` projection: `get_virtual_file` with
/// `VirtualNodeKind::Main` and `CompileTarget::IDE` returns the RUNTIME module
/// under a names-only demand, so a test written that way measures the runtime
/// lane twice and reports the TSX lane healthy no matter what it does.
///
/// Oracle (TypeScript 7.0.2 `tsc`, `--noEmit --strict --ignoreConfig`):
/// every row's `ReturnType<typeof makeProps>` is an ordinary object type, so
/// the emitted TSX type-checks in four cases; `R1Helper` calls `notDeclared`,
/// a name declared nowhere, so its TSX carries exactly that TS2304 and types
/// `made` as the checker's error type — the unmodelled position the
/// flow-return lane leaves to the checker by design.
///
/// Discrimination: making the TSX lane fault on any flow-return class fails
/// the row for that class — `R1Helper` for the uninferred class, `R4Write` for
/// the unverified class, `R2Invoked` for the no-value class, `S1Spread` for a
/// root-position marker. `R3Clean` fails nothing on its own and is the
/// control that a blanket "always emit" change is not what passed.
#[cfg(not(target_arch = "wasm32"))]
#[test]
fn the_tsx_lane_emits_for_every_flow_return_degradation_class() {
    /// `(canonical, script, the binding the projected TSX must name)`
    const ROWS: &[(&str, &str)] = &[
        // A FAITHFUL degraded surface — a marker at one interior position.
        (
            "/src/R1Helper.vue",
            "function makeProps() { const f = () => notDeclared(); return { label: \"x\", made: f() } }",
        ),
        // A NO-VALUE outcome.
        (
            "/src/R2Invoked.vue",
            "function makeProps() { let label = \"x\"; (() => { label = \"y\" })(); return { label } }",
        ),
        // The clean control.
        (
            "/src/R3Clean.vue",
            "function makeProps() { return { label: \"x\", n: 1 } }",
        ),
        // An UNVERIFIED value.
        (
            "/src/R4Write.vue",
            "function makeProps(seed: string) { seed = \"y\"; return { label: seed } }",
        ),
        // A ROOT-position marker.
        (
            "/src/S1Spread.vue",
            "function base() { return { label: \"x\" } }\nfunction makeProps() { return { ...base(), n: 1 } }",
        ),
    ];

    for (canonical, script) in ROWS {
        let project = make_project();
        project
            .upsert_base(
                canonical,
                &format!(
                    "<script setup lang=\"ts\">\n{script}\ndefineProps<ReturnType<typeof makeProps>>()\n</script>\n<template><div /></template>"
                ),
            )
            .unwrap();
        let host = project.host();
        // The LSP's own IDE profile (`Documents::tsx_profile`). IDE
        // normalization strips STYLE/SCRIPT/TEMPLATE so a leftover bundler
        // target cannot admit runtime-render; this test still uses the LSP
        // profile so it measures the hover/TSX lane.
        let profile = crate::types::CompileProfile {
            source_map: true,
            target: crate::CompileTarget::IDE | crate::CompileTarget::TEMPLATE_DATA,
            ..crate::types::CompileProfile::default()
        };
        let compiled = host
            .ensure_ide_compiled(canonical, &profile)
            .unwrap_or_else(|err| {
                panic!(
                    "{canonical}: the TSX lane splices the authored declaration and is \
                     unaffected by a body-derived degradation — it must never delete the \
                     file's type-check surface; got {err:?}"
                )
            });
        assert!(
            compiled,
            "{canonical}: a Vue carrier always has an IDE projection surface"
        );
        let ide = host
            .get_ide(canonical, &profile)
            .unwrap_or_else(|| panic!("{canonical}: no TSX projection was cached"));
        assert!(
            ide.code.contains("makeProps"),
            "{canonical}: the projected TSX must splice the authored script:\n{}",
            ide.code
        );
        assert!(
            ide.code.contains("defineProps"),
            "{canonical}: the projected TSX must carry the authored macro call:\n{}",
            ide.code
        );
    }
}

/// `defineEmits<{ [event: string]: [v: number] }>()` — an emits type argument
/// that is an index-signature-only object literal. The emits object is `events +
/// index signatures`, so the published `define_emits` shape MUST carry the index
/// signature even though there is NO named event.
///
/// PARITY: the retired materialiser surfaced this index signature; the dispatch
/// `define_emits_shape` hardcoded `index_signatures: Vec::new()`, dropping it —
/// a reroute REGRESSION.
///
/// Discriminating: reverting `define_emits_shape` to `index_signatures:
/// Vec::new()` (or dropping the DTO `emit_index_signatures` capture) makes the
/// published shape carry zero index signatures and this test FAILS; the fix
/// publishes the DTO's `emit_index_signatures` so the `[event: string]: [v:
/// number]` signature surfaces. (The `properties` list legitimately stays empty
/// — the surface has no named event.)
#[test]
fn evaluate_types_define_emits_preserves_index_signature_only_surface() {
    let project = make_project();
    project
        .upsert_base(
            "/IndexEmits.vue",
            r#"<script setup lang="ts">
defineEmits<{ [event: string]: [v: number] }>()
</script>
<template><div /></template>"#,
        )
        .unwrap();

    let session = project.open_session_batch().unwrap();
    let evaluated = session.evaluate_types("/IndexEmits.vue").unwrap().unwrap();

    let shape = evaluated
        .define_emits
        .iter()
        .map(|entry| &entry.result.value)
        .next()
        .expect("an index-signature-only defineEmits must still publish a define_emits shape");

    assert_eq!(
        shape.index_signatures.len(),
        1,
        "defineEmits<{{ [event: string]: [v: number] }}> must publish exactly its \
         index signature, got {} index signatures (a `Vec::new()` here means the \
         emit index signature was dropped — emits = events + index signatures; the \
         retired materialiser surfaced it)",
        shape.index_signatures.len(),
    );
    let sig = &shape.index_signatures[0];
    let key_ty = demand_published_type(
        project.host(),
        "/IndexEmits.vue",
        sig.key_type.present(),
        "emit index signature key",
    );
    assert!(
        matches!(key_ty, TypeExpr::Primitive(PrimitiveName::String)),
        "emit index signature key type is `string`, got {key_ty:?}",
    );
    // The value `[v: number]` is the emit payload tuple — a concrete typed form,
    // not an opaque/unknown carrier.
    let value_ty = demand_published_type(
        project.host(),
        "/IndexEmits.vue",
        sig.value_type.present(),
        "emit index signature value",
    );
    assert!(
        matches!(value_ty, TypeExpr::Tuple { .. }),
        "emit index signature value type is the `[v: number]` payload tuple, got {value_ty:?}",
    );
}

#[test]
fn get_component_meta_includes_imported_define_emits_members() {
    let project = make_project();
    project
        .upsert_base(
            "/types.ts",
            r#"export type ExternalEmits = {
  change: [event: Event]
  "update:modelValue": [value: string]
}"#,
        )
        .unwrap();
    project
        .upsert_base(
            "/App.vue",
            r#"<script setup lang="ts">
import type { ExternalEmits } from './types'

defineEmits<ExternalEmits>()
</script>
<template><div /></template>"#,
        )
        .unwrap();

    let meta = project
        .host()
        .get_component_meta("/App.vue")
        .expect("full meta should resolve");
    let event_names: Vec<&str> = meta
        .events
        .iter()
        .map(|event| event.name.as_str())
        .collect();

    assert!(
        event_names.contains(&"change"),
        "full meta should keep direct emit members, got: {event_names:?}"
    );
    assert!(
        event_names.contains(&"update:modelValue"),
        "full meta should include imported emit members from the resolved macro surface, got: {event_names:?}"
    );
}

#[test]
fn get_component_meta_keeps_imported_members_from_local_emit_aliases() {
    let project = make_project();
    project
        .upsert_base(
            "/types.ts",
            r#"export type ModelEmits<T = string> = {
  "update:modelValue": [value: T]
}"#,
        )
        .unwrap();
    project
        .upsert_base(
            "/App.vue",
            r#"<script setup lang="ts">
import type { ModelEmits } from './types'

type AppEmits = {
  change: [event: Event]
} & ModelEmits

defineEmits<AppEmits>()
</script>
<template><div /></template>"#,
        )
        .unwrap();
    let meta = project
        .host()
        .get_component_meta("/App.vue")
        .expect("full meta should resolve");
    let event_names: Vec<&str> = meta
        .events
        .iter()
        .map(|event| event.name.as_str())
        .collect();

    assert!(
        event_names.contains(&"change"),
        "full meta should keep direct emit members, got: {event_names:?}"
    );
    assert!(
        event_names.contains(&"update:modelValue"),
        "full meta should not drop imported emit members from local aliases, got: {event_names:?}"
    );
}

// ===========================================================================
// Phase 3: Native get_component_meta query
// ===========================================================================

#[test]
fn get_component_meta_returns_props_and_events() {
    let project = make_project();
    project
        .upsert_base(
            "/App.vue",
            r#"<script setup lang="ts">
defineProps<{ label: string; count?: number }>()
defineEmits<{ change: [value: string] }>()
</script>
<template><div /></template>"#,
        )
        .unwrap();

    let session = project.open_session_batch().unwrap();
    let meta = session
        .get_component_meta("/App.vue")
        .unwrap()
        .expect("get_component_meta should return metadata");

    // Assert+: props extracted
    assert_eq!(meta.props.len(), 2, "should extract 2 props");
    assert_eq!(meta.props[0].name, "label");
    assert!(meta.props[0].required, "label should be required");
    assert_eq!(meta.props[1].name, "count");
    assert!(!meta.props[1].required, "count should be optional");

    // Assert+: events extracted
    assert_eq!(meta.events.len(), 1, "should extract 1 event");
    assert_eq!(meta.events[0].name, "change");

    // Assert-: no models, no exposed
    assert!(meta.models.is_empty(), "no defineModel → no models");
    assert!(meta.exposed.is_empty(), "no defineExpose → no exposed");
}

#[test]
fn vue_component_meta_keeps_empty_bindings_events_no_regression() {
    // Vue NO-REGRESSION: the neutral per-usage `bindings` / `events` fields stay
    // EMPTY for Vue (Vue carries two-way bindings in `v_models` and events at the
    // template level), while the existing Vue fields are unchanged.
    let project = make_project();
    project
        .upsert_base(
            "/Child.vue",
            r#"<script setup lang="ts">
defineProps<{ label: string; modelValue: number }>()
</script>
<template><button>{{ label }}</button></template>"#,
        )
        .unwrap();
    project
        .upsert_base(
            "/Host.vue",
            r#"<script setup lang="ts">
import { ref } from 'vue'
import Child from './Child.vue'
const count = ref(0)
</script>
<template>
  <Child label="hi" :class="{ active: count > 0 }" v-model="count" />
</template>"#,
        )
        .unwrap();

    let session = project.open_session_batch().unwrap();
    let meta = session
        .get_component_meta("/Host.vue")
        .unwrap()
        .expect("get_component_meta should return metadata");

    assert_eq!(meta.components.len(), 1);
    let usage = &meta.components[0];
    assert_eq!(usage.name, "Child");
    // The Vue fields are byte-identical in shape.
    assert_eq!(usage.v_models, vec!["modelValue".to_string()]);
    assert!(usage.has_dynamic_class);
    assert!(usage.props.iter().any(|p| p.name == "label"));
    // The new neutral fields are EMPTY for Vue.
    assert!(
        usage.bindings.is_empty(),
        "Vue must not populate the neutral `bindings` field"
    );
    assert!(
        usage.events.is_empty(),
        "Vue must not populate the neutral `events` field"
    );
}

/// POSITIVE source-level close for the two union-payload trackers above: an
/// evaluated `defineEmits` call-signature payload whose element instantiates
/// to the union `string | number` (the heritage instantiation
/// `TabsRootEmits<string | number>`) publishes the EXACT closed tuple source —
/// `Closed(Tuple([payload: LeafUnion([Primitive(String), Primitive(Number)])]))`
/// — and demands to `Tuple([Union(String, Number)])` through the one shared
/// dispatch.
///
/// Fail-closed negatives: the payload is NEVER the degraded Unknown leaf, a
/// fabricated authored locator (a position the author never wrote), a
/// synthesized shape, or a synthetic slot-binding carrier.
#[test]
fn evaluated_union_emit_payload_publishes_the_closed_tuple_leaf_union_source() {
    use verter_type_expr::facts::{
        ClosedTypeFact, FactOrLocator, LeafTypeFact, SemanticTypeSource,
    };

    let project = make_project();
    project
        .upsert_base(
            "/node_modules/reka-ui/index.d.ts",
            r#"
export interface TabsRootEmits<T> {
  (e: 'update:modelValue', payload: T): void
}
"#,
        )
        .unwrap();
    project
        .upsert_base(
            "/src/App.vue",
            r#"<script lang="ts">
import type { TabsRootEmits } from 'reka-ui'

export interface Emits extends TabsRootEmits<string | number> {}
</script>
<script setup lang="ts">
defineEmits<Emits>()
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

    let resolved = project
        .host()
        .resolve_component_meta(
            "/src/App.vue",
            verter_type_engine::semantic_query::ProjectionMode::Expanded,
        )
        .expect("resolved component meta should exist");
    let meta = crate::resolver_core::with_bare_host_ctx_for_test(project.host(), |ctx| {
        let fixture_dispatch_3 =
            verter_type_engine::project_semantic_dispatch::ProjectSemanticDispatch::new(ctx);

        crate::host_manage::extract_component_meta_from_resolved(
            project.host(),
            "/src/App.vue",
            &resolved,
            true,
            ctx,
            &fixture_dispatch_3,
        )
    })
    .analysis;

    let event = meta
        .events
        .iter()
        .find(|event| event.name == "update:modelValue")
        .expect("update:modelValue event should exist");
    let payload = event
        .payload
        .present()
        .expect("the union payload publishes a typed source");

    // The EXACT closed tuple source: one labelled required non-rest element
    // whose ty is the ORDERED leaf union `string | number`.
    let SemanticTypeSource::Closed(ClosedTypeFact::Tuple(tuple)) = payload else {
        panic!("the union emit payload must publish the closed tuple source, got {payload:?}");
    };
    assert!(!tuple.readonly, "the payload tuple is not readonly");
    assert_eq!(tuple.elements.len(), 1, "one post-event-name payload param");
    let element = &tuple.elements[0];
    assert_eq!(
        element.label.as_deref(),
        Some("payload"),
        "the param label must survive as the tuple element label"
    );
    assert!(!element.optional, "the payload param is required");
    assert!(!element.rest, "the payload param is not a rest param");
    let FactOrLocator::LeafUnion(leaves) = &element.ty else {
        panic!(
            "the tuple element must carry the closed leaf-union fact, got {:?}",
            element.ty
        );
    };
    assert_eq!(
        leaves.as_ref(),
        &[
            LeafTypeFact::Primitive(PrimitiveName::String),
            LeafTypeFact::Primitive(PrimitiveName::Number),
        ],
        "the leaf union must carry the ordered instantiated primitives"
    );

    // Fail-closed negatives: never the degraded Unknown leaf, never a
    // fabricated authored locator / synthesized shape / synthetic carrier.
    assert_ne!(
        payload,
        &SemanticTypeSource::Closed(ClosedTypeFact::Leaf(LeafTypeFact::Primitive(
            PrimitiveName::Unknown,
        ))),
        "the union payload must not degrade to the Unknown leaf"
    );
    assert!(
        !matches!(
            payload,
            SemanticTypeSource::Authored(_)
                | SemanticTypeSource::Synthesized(_)
                | SemanticTypeSource::SyntheticSlotBinding(_)
        ),
        "the union payload must not be a fabricated authored locator or synthetic source, \
         got {payload:?}"
    );

    // End-to-end: the demand materializes the tuple whose element is the
    // union of the two instantiated primitives — never Unknown.
    let demanded = demand_published_type(
        project.host(),
        "/src/App.vue",
        event.payload.present(),
        "update:modelValue payload",
    );
    let TypeExpr::Tuple { elements, readonly } = &demanded else {
        panic!("the union payload must demand to a tuple, got {demanded:?}");
    };
    assert!(!readonly);
    assert_eq!(elements.len(), 1, "one payload element");
    assert_eq!(
        elements[0].label.as_deref(),
        Some("payload"),
        "the demanded element keeps its label"
    );
    let TypeExpr::Union(members) = &elements[0].ty else {
        panic!(
            "the demanded element must be the union, got {:?}",
            elements[0].ty
        );
    };
    assert_eq!(members.len(), 2, "both union arms must materialize");
    assert!(
        members.contains(&TypeExpr::Primitive(PrimitiveName::String)),
        "the demanded union must include string, got {demanded:?}"
    );
    assert!(
        members.contains(&TypeExpr::Primitive(PrimitiveName::Number)),
        "the demanded union must include number, got {demanded:?}"
    );
    assert!(
        !members.contains(&TypeExpr::Primitive(PrimitiveName::Unknown)),
        "the demanded union must not contain Unknown, got {demanded:?}"
    );
}

#[test]
fn declared_props_and_events_take_precedence() {
    let project = make_project();
    project
        .upsert_base(
            "/App.vue",
            r#"<script setup lang="ts">
defineProps<{ id: number }>()
defineEmits<{ (e: 'click', value: string): void }>()
</script>
<template><div>hello</div></template>"#,
        )
        .unwrap();

    let meta = get_meta(&project, "/App.vue");

    // Assert+: 'id' should be declared, not inherited
    let id_prop = meta
        .accepted_props
        .iter()
        .find(|p| p.name == "id")
        .expect("should have 'id' in accepted_props");
    assert!(
        matches!(id_prop.provenance, MemberProvenance::Declared),
        "'id' should be declared, not inherited"
    );

    // Assert+: 'click' should be declared, not inherited
    let click_event = meta
        .accepted_events
        .iter()
        .find(|e| e.name == "click")
        .expect("should have 'click' in accepted_events");
    assert!(
        matches!(click_event.provenance, MemberProvenance::Declared),
        "'click' should be declared, not inherited"
    );

    // Assert-: should NOT have duplicate 'id' or 'click'
    assert_eq!(
        meta.accepted_props
            .iter()
            .filter(|p| p.name == "id")
            .count(),
        1,
        "'id' should appear exactly once"
    );
    assert_eq!(
        meta.accepted_events
            .iter()
            .filter(|e| e.name == "click")
            .count(),
        1,
        "'click' should appear exactly once"
    );
}

// `get_component_meta_real_nuxt_ui_editor_toolbar_keeps_base_and_plugin_props`
// retired: the hermetic
// `get_component_meta_editor_toolbar_union_keeps_base_and_plugin_props`
// above asserts the same 16-prop contract (as, color, variant,
// activeColor, activeVariant, size, items, editor, class, ui, layout,
// appendTo, pluginKey, shouldShow, updateDelay, options) against the
// same EditorToolbarProps union shape. The retired test inspected the
// same shape from a `.integration-tests/repos/nuxt-ui/` checkout via
// FilesystemWorkspace; per the user directive (unit tests must not
// rely on third-party code) and CLAUDE.md "Legacy Code Deletion"
// (do not preserve dual paths), the third-party-coupled duplicate
// was deleted rather than re-ported into a near-identical second
// hermetic copy.

/// defineEmits: call-signature / overload form. Projection must preserve event
/// names and payloads for callable emit declarations.
#[test]
fn get_component_meta_define_emits_call_signature_form() {
    let project = make_project();
    project
        .upsert_base(
            "/App.vue",
            r#"<script setup lang="ts">
const emit = defineEmits<{
  (e: 'change', value: string): void
  (e: 'update', id: number, force?: boolean): void
  (e: 'close'): void
}>()
</script>
<template><div /></template>"#,
        )
        .unwrap();

    let meta = get_meta(&project, "/App.vue");
    let event_names: Vec<&str> = meta.events.iter().map(|ev| ev.name.as_str()).collect();

    assert!(
        event_names.contains(&"change"),
        "call-signature emits must include 'change', got: {event_names:?}"
    );
    assert!(
        event_names.contains(&"update"),
        "call-signature emits must include 'update', got: {event_names:?}"
    );
    assert!(
        event_names.contains(&"close"),
        "call-signature emits must include 'close', got: {event_names:?}"
    );
}

/// defineEmits: object-literal form. Simpler shape must also work on the new
/// projection path.
#[test]
fn get_component_meta_define_emits_object_literal_form() {
    let project = make_project();
    project
        .upsert_base(
            "/App.vue",
            r#"<script setup lang="ts">
const emit = defineEmits<{
  change: [value: string],
  update: [id: number, force?: boolean]
  close: []
}>()
</script>
<template><div /></template>"#,
        )
        .unwrap();

    let meta = get_meta(&project, "/App.vue");
    let event_names: Vec<&str> = meta.events.iter().map(|ev| ev.name.as_str()).collect();

    assert!(
        event_names.contains(&"change"),
        "object-literal emits must include 'change', got: {event_names:?}"
    );
    assert!(
        event_names.contains(&"update"),
        "object-literal emits must include 'update', got: {event_names:?}"
    );
    assert!(
        event_names.contains(&"close"),
        "object-literal emits must include 'close', got: {event_names:?}"
    );
}

#[test]
fn inline_property_style_emit_jsdoc_publishes() {
    let project = make_project();
    project
        .upsert_base(
            "/src/Inline.vue",
            r#"<script setup lang="ts">
defineEmits<{
  /** Fired on click */
  click: []
  close: []
}>()
</script>
<template><div /></template>"#,
        )
        .unwrap();

    let meta = project
        .host()
        .get_component_meta("/src/Inline.vue")
        .expect("component meta resolves");

    let click = meta
        .events
        .iter()
        .find(|event| event.name == "click")
        .expect("click event must surface");
    assert_eq!(
        click.description.as_deref(),
        Some("Fired on click"),
        "inline property-style emit's JSDoc description must publish"
    );

    let close = meta
        .events
        .iter()
        .find(|event| event.name == "close")
        .expect("close event must surface");
    assert_eq!(
        close.description.as_deref(),
        None,
        "undocumented event must not gain a fabricated description"
    );
    assert!(
        close.tags.is_empty(),
        "undocumented event must not gain fabricated tags, got {:?}",
        close.tags
    );
}

#[test]
fn cross_file_call_signature_emit_jsdoc_publishes() {
    let project = make_project();
    project
        .upsert_base(
            "/src/emits.ts",
            r#"
export interface WidgetEmits {
  /**
   * Fires when the value changes.
   * @deprecated listen to input instead
   */
  (e: 'change', value: string): void
  /** Fires on focus. */
  (e: 'focus'): void
}
"#,
        )
        .unwrap();
    project
        .upsert_base(
            "/src/Widget.vue",
            r#"<script setup lang="ts">
import type { WidgetEmits } from './emits'

defineEmits<WidgetEmits>()
</script>
<template><div /></template>"#,
        )
        .unwrap();

    let meta = project
        .host()
        .get_component_meta("/src/Widget.vue")
        .expect("component meta resolves");

    let change = meta
        .events
        .iter()
        .find(|event| event.name == "change")
        .expect("change event must surface");
    assert_eq!(
        change.description.as_deref(),
        Some("Fires when the value changes."),
        "imported call-signature emit's JSDoc description must publish"
    );
    let deprecated = change
        .tags
        .iter()
        .find(|tag| tag.name == "deprecated")
        .expect("imported call-signature emit's @deprecated tag must publish");
    assert_eq!(deprecated.text.as_deref(), Some("listen to input instead"));

    let focus = meta
        .events
        .iter()
        .find(|event| event.name == "focus")
        .expect("focus event must surface");
    assert_eq!(focus.description.as_deref(), Some("Fires on focus."));
}

#[test]
fn cross_file_property_style_emit_jsdoc_publishes() {
    let project = make_project();
    project
        .upsert_base(
            "/src/emits.ts",
            r#"
export interface PanelEmits {
  /** Fires when the panel toggles. */
  toggle: [open: boolean]
  close: []
}
"#,
        )
        .unwrap();
    project
        .upsert_base(
            "/src/Panel.vue",
            r#"<script setup lang="ts">
import type { PanelEmits } from './emits'

defineEmits<PanelEmits>()
</script>
<template><div /></template>"#,
        )
        .unwrap();

    let meta = project
        .host()
        .get_component_meta("/src/Panel.vue")
        .expect("component meta resolves");

    let toggle = meta
        .events
        .iter()
        .find(|event| event.name == "toggle")
        .expect("toggle event must surface");
    assert_eq!(
        toggle.description.as_deref(),
        Some("Fires when the panel toggles."),
        "imported property-style emit's JSDoc description must publish"
    );

    // Negative: an undocumented event publishes NO description and NO
    // fabricated tags (the synthesized @defaultValue rail is props-only).
    let close = meta
        .events
        .iter()
        .find(|event| event.name == "close")
        .expect("close event must surface");
    assert_eq!(
        close.description.as_deref(),
        None,
        "undocumented event must not gain a fabricated description"
    );
    assert!(
        close.tags.is_empty(),
        "undocumented event must not gain fabricated tags, got {:?}",
        close.tags
    );
}

/// (a) An authored `defineEmits` payload materializes to its exact SHALLOW
/// `TypeExpr` in the output envelope's `events[].payload` lane.
#[test]
fn component_meta_output_materializes_authored_emit_payload_to_exact_shallow_tuple() {
    let project = make_project();
    project
        .upsert_base(
            "/App.vue",
            r#"<script setup lang="ts">
defineEmits<{ change: [value: number] }>()
</script>
<template><div /></template>"#,
        )
        .unwrap();

    let output = project
        .host()
        .get_component_meta_output("/App.vue")
        .expect("events-payload output materialization must succeed")
        .expect("component must resolve");
    let (analysis, resolution, types) = output.into_parts();

    assert!(
        resolution.is_none(),
        "the plain cold entry carries no resolution sidecar"
    );
    assert_eq!(analysis.events.len(), 1, "fixture declares exactly 1 event");
    assert_eq!(analysis.events[0].name, "change");
    assert!(
        analysis.events[0].payload.is_present(),
        "fixture premise: the authored tuple payload must publish a typed source"
    );

    let payloads = materialized_event_types(&types.into_lanes());
    assert_eq!(
        payloads.len(),
        1,
        "the events lane must align 1:1 with analysis.events"
    );
    let expected = labeled_tuple(&[("value", TypeExpr::Primitive(PrimitiveName::Number))]);
    assert_eq!(
        payloads[0], expected,
        "authored `change: [value: number]` must materialize to exactly \
         `[value: number]` (shallow shell), got {:?}",
        payloads[0]
    );
}

/// (b) A payload-less event (payload source `None`) materializes through the
/// ONE centralized missing-source policy to the canonical typed
/// `TypeExpr::Unknown` — decided in the session output sink, never at the
/// wire.
#[test]
fn component_meta_output_missing_event_payload_source_follows_central_unknown_policy() {
    let project = make_project();
    project
        .upsert_base(
            "/App.vue",
            r#"<script setup lang="ts">
defineEmits(['ping'])
</script>
<template><div /></template>"#,
        )
        .unwrap();

    let output = project
        .host()
        .get_component_meta_output("/App.vue")
        .expect("events-payload output materialization must succeed")
        .expect("component must resolve");
    let (analysis, _resolution, types) = output.into_parts();

    assert_eq!(analysis.events.len(), 1, "fixture declares exactly 1 event");
    assert_eq!(analysis.events[0].name, "ping");
    assert!(
        analysis.events[0].payload.present().is_none(),
        "fixture premise: a runtime-array emit carries no typed payload source, got {:?}",
        analysis.events[0].payload
    );

    let payloads = materialized_event_types(&types.into_lanes());
    assert_eq!(
        payloads.len(),
        1,
        "the events lane must align 1:1 with analysis.events"
    );
    assert_eq!(
        payloads[0],
        TypeExpr::Unknown(UnknownValue::missing_output()),
        "a None payload source must materialize to the canonical typed Unknown \
         via the central missing-source policy, got {:?}",
        payloads[0]
    );
}

/// (c) A PRESENT-but-unraisable payload source FAILS the output with the
/// strict typed `ComponentMetaOutputError` (carrying the lane, the lane
/// index, and the failed source) — it must NEVER silently materialize as
/// `Unknown`.
#[test]
fn component_meta_output_unraisable_event_payload_source_fails_typed_never_silent_unknown() {
    let project = make_project();
    project
        .upsert_base(
            "/App.vue",
            r#"<script setup lang="ts">
defineEmits<{ change: [value: number]; close: [] }>()
</script>
<template><div /></template>"#,
        )
        .unwrap();

    let host = project.host();
    let analysis = host
        .get_component_meta("/App.vue")
        .expect("component must resolve");
    assert_eq!(
        analysis.events.len(),
        2,
        "fixture declares exactly 2 events"
    );

    // Sanity: the UNTAMPERED analysis materializes cleanly — the typed
    // failure asserted below is not unconditional.

    let fixture_dispatch_11 =
        verter_type_engine::project_semantic_dispatch::ProjectSemanticDispatch::new(host);
    let ok = crate::meta_resolve::projectors::build_component_meta_output(
        host,
        &fixture_dispatch_11,
        "/App.vue",
        analysis.clone(),
        None,
        crate::meta_resolve::PublishedCompleteness::COMPLETE,
    )
    .expect("the untampered analysis must materialize");
    let (_a, _r, ok_types) = ok.into_parts();
    assert_eq!(materialized_event_types(&ok_types.into_lanes()).len(), 2);

    // A present-but-unraisable source: an authored macro-payload locator
    // addressing a macro ordinal that does not exist in the owner — the
    // shared raise has no live graph representation for it.
    let bad_source = verter_type_expr::facts::SemanticTypeSource::Authored(
        verter_type_expr::locators::AuthoredBodyLocator::MacroPayload(
            verter_type_expr::locators::MacroPayloadLocator {
                anchor: verter_type_expr::locators::AuthoredAnchor {
                    canonical_id: Arc::from("/App.vue"),
                    owner: verter_type_expr::TopLevelOwnerId::instance(0),
                    symbol: Arc::from("default"),
                    space: verter_type_expr::locators::LocatorSymbolSpace::Value,
                },
                macro_index: 99,
                payload: verter_type_expr::locators::MacroPayloadPosition::TypeArgument,
            },
        ),
    );
    let mut tampered = analysis;
    replace_event_payload(
        &mut tampered.events[1],
        verter_type_expr::facts::SourcePosition::Present(bad_source.clone()),
    );

    let err = crate::meta_resolve::projectors::build_component_meta_output(
        host,
        &fixture_dispatch_11,
        "/App.vue",
        tampered,
        None,
        crate::meta_resolve::PublishedCompleteness::COMPLETE,
    )
    .expect_err(
        "a present-but-unraisable payload source must FAIL the output with a typed \
         error — it must NOT silently materialize as Unknown",
    );
    assert_eq!(
        err.lane,
        crate::meta_resolve::ComponentMetaOutputLane::EventPayload,
        "the error must name the failed lane"
    );
    assert_eq!(err.index, 1, "the error must carry the failed lane index");
    assert_eq!(
        *err.position,
        verter_type_expr::facts::SourcePosition::Present(bad_source),
        "the error must carry the failed source"
    );
    assert_eq!(
        err.failure,
        crate::meta_resolve::ComponentMetaOutputFailure::UnraisableSource,
        "the failure class must be the raise miss, got {:?}",
        err.failure
    );
}

// @ai-generated - Discriminates the event-return output lane from a healthy
// event payload when only the producer-owned callable return cannot materialize.
#[test]
fn component_meta_output_return_only_failure_names_event_return_lane() {
    let project = make_project();
    project
        .upsert_base(
            "/App.vue",
            r#"<script setup lang="ts">
defineEmits<{ (event: 'change', value: number): boolean }>()
</script>
<template><div /></template>"#,
        )
        .unwrap();

    let host = project.host();
    let mut analysis = host
        .get_component_meta("/App.vue")
        .expect("component must resolve");
    assert_eq!(
        analysis.events.len(),
        1,
        "fixture declares one callable event"
    );
    assert!(
        analysis.events[0].return_publication.is_some(),
        "the callable producer must publish its return lane"
    );

    let bad_source = verter_type_expr::facts::SemanticTypeSource::Authored(
        verter_type_expr::locators::AuthoredBodyLocator::MacroPayload(
            verter_type_expr::locators::MacroPayloadLocator {
                anchor: verter_type_expr::locators::AuthoredAnchor {
                    canonical_id: Arc::from("/App.vue"),
                    owner: verter_type_expr::TopLevelOwnerId::instance(0),
                    symbol: Arc::from("default"),
                    space: verter_type_expr::locators::LocatorSymbolSpace::Value,
                },
                macro_index: 99,
                payload: verter_type_expr::locators::MacroPayloadPosition::TypeArgument,
            },
        ),
    );
    let failed_position = verter_type_expr::facts::SourcePosition::Present(bad_source.clone());
    analysis.events[0].return_publication =
        Some(verter_type_expr::TypePublication::from_source_position(
            &failed_position,
            verter_type_expr::ResolutionExactness::ExactConcrete,
            verter_type_expr::ResolutionProvenance::FrameworkSurface,
            Arc::from([]),
            None,
            &verter_type_expr::PublicationPolicy::exact_only(),
        ));

    let fixture_dispatch_12 =
        verter_type_engine::project_semantic_dispatch::ProjectSemanticDispatch::new(host);
    let err = crate::meta_resolve::projectors::build_component_meta_output(
        host,
        &fixture_dispatch_12,
        "/App.vue",
        analysis,
        None,
        crate::meta_resolve::PublishedCompleteness::COMPLETE,
    )
    .expect_err("a failed callable return must fail output materialization");
    assert_eq!(
        err.lane,
        crate::meta_resolve::ComponentMetaOutputLane::EventReturn,
        "the healthy payload lane must not be blamed for a return-only failure"
    );
    assert_eq!(err.index, 0);
    assert_eq!(err.inner_index, None);
    assert_eq!(*err.position, failed_position);
    assert_eq!(
        err.failure,
        crate::meta_resolve::ComponentMetaOutputFailure::UnraisableSource,
    );
}

/// (d) The `events[].payload` lane is POSITIONAL: entries align 1:1 with
/// `analysis.events` in order, and duplicate event names are preserved as
/// distinct positional rows — a name-keyed output would collapse the two
/// `dup` rows (len 3, not 4) and lose alignment.
#[test]
fn component_meta_output_event_occurrences_preserve_duplicate_names() {
    let project = make_project();
    project
        .upsert_base(
            "/App.vue",
            r#"<script setup lang="ts">
defineEmits<{ first: [id: number]; dup: [a: string]; dup: [b: boolean]; last: [] }>()
</script>
<template><div /></template>"#,
        )
        .unwrap();

    let output = project
        .host()
        .get_component_meta_output("/App.vue")
        .expect("events-payload output materialization must succeed")
        .expect("component must resolve");
    let (analysis, _resolution, types) = output.into_parts();

    let names: Vec<&str> = analysis.events.iter().map(|e| e.name.as_str()).collect();
    assert_eq!(
        names,
        vec!["first", "dup", "dup", "last"],
        "fixture premise: the analysis preserves the duplicate `dup` rows in source order"
    );

    let lanes = types.into_lanes();
    let payloads = &materialized_event_types(&lanes);
    assert_eq!(
        payloads.len(),
        4,
        "the lane must keep one entry PER analysis event — duplicate names \
         preserved positionally, never collapsed name-keyed"
    );
    assert_eq!(
        payloads[0],
        labeled_tuple(&[("id", TypeExpr::Primitive(PrimitiveName::Number))]),
        "index 0 must be `first`'s `[id: number]`, got {:?}",
        payloads[0]
    );
    assert_eq!(
        payloads[1],
        labeled_tuple(&[("a", TypeExpr::Primitive(PrimitiveName::String))]),
        "index 1 must be the first `dup` row, got {:?}",
        payloads[1]
    );
    assert_eq!(
        payloads[2],
        labeled_tuple(&[("b", TypeExpr::Primitive(PrimitiveName::Boolean))]),
        "index 2 must be the second authored `dup` overload, got {:?}",
        payloads[2]
    );
    assert_eq!(
        payloads[3],
        labeled_tuple(&[]),
        "index 3 must be `last`'s empty tuple `[]`, got {:?}",
        payloads[3]
    );
    assert_eq!(
        event_occurrence_publications(&lanes).len(),
        4,
        "the A1 publication lane aligns 1:1 with duplicate event rows"
    );
    assert_eq!(
        published_type(&event_occurrence_publications(&lanes)[1]),
        &labeled_tuple(&[("a", TypeExpr::Primitive(PrimitiveName::String))])
    );
    assert_eq!(
        published_type(&event_occurrence_publications(&lanes)[2]),
        &labeled_tuple(&[("b", TypeExpr::Primitive(PrimitiveName::Boolean))])
    );
}

// ═══════════════════════════════════════════════════════════════════════════
// Required-payload honesty: a REQUIRED emit payload whose faithful source
// cannot be constructed is a TYPED FAILURE at output materialization — never
// a Completed + `unknown` + zero-diagnostic success. Authored `unknown`,
// genuinely-open, and schema-absent positions stay valid successes.
// ═══════════════════════════════════════════════════════════════════════════

/// A CROSS-FILE call-signature emit (`defineEmits<Events>()` over an
/// imported `Events { (e: 'save', value: Row): void }`) publishes the REAL
/// payload tuple: the normalized row mints an exact callable-occurrence
/// replay route, output materialization replays it through the one
/// shared dispatch, and the published payload is `[value: Row]` — the
/// shallow named reference a consumer re-resolves on demand. The result is
/// a COMPLETE success (warm-admissible). The pre-fix behavior was the typed
/// `Failed(UnrepresentableRequiredPayload)` interim (and before that, a
/// fabricated `unknown` rendered as success).
#[test]
fn cross_file_call_signature_emit_payload_publishes_the_real_tuple() {
    let project = make_project();
    project
        .upsert_base(
            "/events.ts",
            "export interface Row { id: number }\nexport interface Events { (e: 'save', value: Row): void }\n",
        )
        .unwrap();
    project
        .upsert_base(
            "/App.vue",
            r#"<script setup lang="ts">
import type { Events } from './events'
defineEmits<Events>()
</script>
<template><div /></template>"#,
        )
        .unwrap();

    let output = project
        .host()
        .get_component_meta_output("/App.vue")
        .expect("a cross-file call-signature emit payload is representable and must succeed")
        .expect("component must resolve");
    let (analysis, _resolution, types) = output.into_parts();
    let lanes = types.into_lanes();
    let save = analysis
        .events
        .iter()
        .position(|event| event.name == "save")
        .expect("the save event publishes");
    // The published source is the projected callable-occurrence replay route —
    // never a fabricated closed fact and never a failure.
    assert!(
        matches!(
            analysis.events[save].payload.present(),
            Some(verter_type_expr::facts::SemanticTypeSource::Projected(
                verter_type_expr::facts::ProjectedTypeFact::CallableOccurrence {
                    projection: verter_type_expr::facts::CallableOccurrenceProjection::Parameters {
                        first_param: 1,
                    },
                    ..
                }
            ))
        ),
        "the cross-file call-signature payload publishes the callable-occurrence \
         replay source, got {:?}",
        analysis.events[save].payload
    );
    let TypeExpr::Tuple { elements, .. } = &materialized_event_types(&lanes)[save] else {
        panic!(
            "the replayed payload tuple renders; got {:?}",
            materialized_event_types(&lanes)[save]
        );
    };
    assert_eq!(elements.len(), 1, "one post-event-name payload param");
    assert_eq!(elements[0].label.as_deref(), Some("value"));
    assert!(
        matches!(&elements[0].ty, TypeExpr::Ref { name, .. } if name.as_ref() == "Row"),
        "the named payload param stays the shallow resolvable reference — \
         never a fabricated unknown; got {:?}",
        elements[0].ty
    );
    // Resolvability walk: demanding the published source through the one
    // shared dispatch materializes the REAL tuple and resolves the imported
    // `Row` reference — never an unknown and never a semantic miss.
    let demanded = demand_published_type(
        project.host(),
        "/App.vue",
        analysis.events[save].payload.present(),
        "save payload",
    );
    let TypeExpr::Tuple {
        elements: demanded_elements,
        ..
    } = &demanded
    else {
        panic!("the demanded payload source materializes the tuple, got {demanded:?}");
    };
    assert_eq!(demanded_elements.len(), 1);
    match &demanded_elements[0].ty {
        TypeExpr::Object(shape) => assert!(
            shape.properties.iter().any(|member| matches!(
                member,
                ObjectMember::Property(property) if property.string_name().expect("string-key fixture") == "id"
            )),
            "the demanded Row payload expands to its declared members, got {demanded:?}"
        ),
        TypeExpr::Ref { name, .. } if name.as_ref() == "Row" => {}
        other => panic!(
            "the demanded payload element must resolve (an expanded Row body \
             or the resolvable Row reference), got {other:?}"
        ),
    }

    // COMPLETE-success enforcement: the representable payload leaves the
    // result complete and warm-admissible — the fail-closed interim (a
    // partial, suppressed result) is gone for this class.
    let (_analysis, state) = project
        .host()
        .get_component_meta_with_resolution("/App.vue")
        .expect("resolves");
    assert!(
        !state.completeness.is_partial(),
        "a representable call-signature payload completes; got {:?}",
        state.completeness
    );
}

/// An IMPORTED property-style emit (`ImportedEmits { save: [id: number] }`)
/// publishes the REAL payload tuple, IDENTICAL to the local authored control
/// (`defineEmits<{ save: [id: number] }>()`): the normalized emit row's
/// faithful `Closed(Tuple)` source is the payload AUTHORITY — the flat
/// `evaluated_types.emits` lane's unrepresentable residue no longer shadows
/// it. Imported == local. The pre-fix behavior was the typed
/// `Failed(UnrepresentableRequiredPayload)` interim (and before that, a
/// fabricated `unknown` rendered as success).
#[test]
fn imported_property_style_emit_payload_matches_the_local_control() {
    let render_save_payload = |project: &MetaProject| {
        let output = project
            .host()
            .get_component_meta_output("/App.vue")
            .expect("a property-style emit payload tuple is representable and must succeed")
            .expect("component must resolve");
        let (analysis, _resolution, types) = output.into_parts();
        let lanes = types.into_lanes();
        let save = analysis
            .events
            .iter()
            .position(|event| event.name == "save")
            .expect("the save event publishes");
        materialized_event_types(&lanes)[save].clone()
    };

    // The IMPORTED property-style emit.
    let imported = make_project();
    imported
        .upsert_base(
            "/emits.ts",
            "export interface ImportedEmits { save: [id: number] }\n",
        )
        .unwrap();
    imported
        .upsert_base(
            "/App.vue",
            r#"<script setup lang="ts">
import type { ImportedEmits } from './emits'
defineEmits<ImportedEmits>()
</script>
<template><div /></template>"#,
        )
        .unwrap();
    let imported_payload = render_save_payload(&imported);

    // The LOCAL authored control — the same payload written inline.
    let local = make_project();
    local
        .upsert_base(
            "/App.vue",
            r#"<script setup lang="ts">
defineEmits<{ save: [id: number] }>()
</script>
<template><div /></template>"#,
        )
        .unwrap();
    let local_payload = render_save_payload(&local);

    // The imported payload is the REAL tuple…
    let TypeExpr::Tuple { elements, .. } = &imported_payload else {
        panic!("the imported property-style payload tuple renders; got {imported_payload:?}");
    };
    assert_eq!(elements.len(), 1);
    assert_eq!(elements[0].label.as_deref(), Some("id"));
    assert_eq!(
        elements[0].ty,
        TypeExpr::Primitive(PrimitiveName::Number),
        "the imported tuple element renders its real number type — never a \
         fabricated unknown"
    );
    // …IDENTICAL to the local authored control: imported == local.
    assert_eq!(
        imported_payload, local_payload,
        "an imported property-style emit publishes the SAME payload as the \
         local authored control"
    );
}

/// SESSION-OVERLAY parity for the callable-occurrence replay: the same
/// cross-file call-signature emit resolved through a SESSION (overlay
/// upserts + the fixed-view output path — the flow the native bindings
/// drive) publishes the same real payload tuple as the base-host scalar.
#[test]
fn cross_file_call_signature_emit_analysis_retains_the_session_overlay_context() {
    let project = make_project();
    let session = project.open_session_batch().expect("batch session");
    session
        .upsert(
            "/events.ts",
            "export interface Events { (e: 'save', value: number): void }\n".to_string(),
        )
        .unwrap();
    session
        .upsert(
            "/App.vue",
            r#"<script setup lang="ts">
import type { Events } from './events'
defineEmits<Events>()
</script>
<template><div /></template>"#
                .to_string(),
        )
        .unwrap();

    let analysis = session
        .get_component_meta("/App.vue")
        .expect("the session-overlay analysis query succeeds")
        .expect("the overlay-only component resolves");
    assert!(
        analysis.events.iter().any(|event| event.name == "save"),
        "macro DTO extraction must retain the session view while resolving the exact \
         overlay-only `/events.ts` `Events` declaration"
    );
}

/// OUTPUT parity for the same overlay-only callable declaration. The analysis
/// extraction and the later payload materialization must remain bound to the
/// same session view and fixed store capture.
#[test]
fn cross_file_call_signature_emit_payload_replays_under_a_session_overlay() {
    let project = make_project();
    let session = project.open_session_batch().expect("batch session");
    session
        .upsert(
            "/events.ts",
            "export interface Row { id: number }\nexport interface Events { (e: 'save', value: Row): void }\n"
                .to_string(),
        )
        .unwrap();
    session
        .upsert(
            "/App.vue",
            r#"<script setup lang="ts">
import type { Events } from './events'
defineEmits<Events>()
</script>
<template><div /></template>"#
                .to_string(),
        )
        .unwrap();

    let output = session
        .get_component_meta_output("/App.vue")
        .expect("the session-overlay call-signature emit payload replays and must succeed")
        .expect("component must resolve");
    let (analysis, _resolution, types) = output.into_parts();
    let lanes = types.into_lanes();
    let save = analysis
        .events
        .iter()
        .position(|event| event.name == "save")
        .expect("the save event publishes");
    let TypeExpr::Tuple { elements, .. } = &materialized_event_types(&lanes)[save] else {
        panic!(
            "the replayed payload tuple renders under the session view; got {:?}",
            materialized_event_types(&lanes)[save]
        );
    };
    assert_eq!(elements.len(), 1);
    assert_eq!(elements[0].label.as_deref(), Some("value"));
    assert!(
        matches!(&elements[0].ty, TypeExpr::Ref { name, .. } if name.as_ref() == "Row"),
        "the named payload param stays the shallow resolvable reference under \
         the session view; got {:?}",
        elements[0].ty
    );
}

/// A COMPOSITE call-signature emit param (`value: A | B`) publishes the real
/// union payload through the callable-occurrence replay: every arm is present in
/// the rendered tuple (order preserved), never a collapsed or fabricated
/// value.
#[test]
fn composite_call_signature_emit_payload_publishes_all_union_arms() {
    let project = make_project();
    project
        .upsert_base(
            "/events.ts",
            "export interface A { a: number }\nexport interface B { b: string }\nexport interface Events { (e: 'save', value: A | B): void }\n",
        )
        .unwrap();
    project
        .upsert_base(
            "/App.vue",
            r#"<script setup lang="ts">
import type { Events } from './events'
defineEmits<Events>()
</script>
<template><div /></template>"#,
        )
        .unwrap();

    let output = project
        .host()
        .get_component_meta_output("/App.vue")
        .expect("a composite call-signature emit payload is representable and must succeed")
        .expect("component must resolve");
    let (analysis, _resolution, types) = output.into_parts();
    let lanes = types.into_lanes();
    let save = analysis
        .events
        .iter()
        .position(|event| event.name == "save")
        .expect("the save event publishes");
    let TypeExpr::Tuple { elements, .. } = &materialized_event_types(&lanes)[save] else {
        panic!(
            "the replayed payload tuple renders; got {:?}",
            materialized_event_types(&lanes)[save]
        );
    };
    assert_eq!(elements.len(), 1);
    assert_eq!(elements[0].label.as_deref(), Some("value"));
    let TypeExpr::Union(arms) = &elements[0].ty else {
        panic!(
            "the composite payload param renders its union, got {:?}",
            elements[0].ty
        );
    };
    let arm_names: Vec<&str> = arms
        .iter()
        .map(|arm| match arm {
            TypeExpr::Ref { name, .. } => name.as_ref(),
            other => panic!("every union arm stays a shallow named reference, got {other:?}"),
        })
        .collect();
    assert_eq!(
        arm_names,
        vec!["A", "B"],
        "ALL composite arms are present, in authored order"
    );
}

/// A NESTED-OBJECT call-signature emit param (`value: {{ nested: Row }}`)
/// publishes the shallow object carrier — the nested reference stays a
/// resolvable `Ref` at publication, and a consumer WALK (demanding the
/// published source through the one shared dispatch) reaches the nested
/// leaf.
#[test]
fn nested_object_call_signature_emit_payload_stays_a_walkable_shallow_carrier() {
    let project = make_project();
    project
        .upsert_base(
            "/events.ts",
            "export interface Row { id: number }\nexport interface Events { (e: 'save', value: { nested: Row }): void }\n",
        )
        .unwrap();
    project
        .upsert_base(
            "/App.vue",
            r#"<script setup lang="ts">
import type { Events } from './events'
defineEmits<Events>()
</script>
<template><div /></template>"#,
        )
        .unwrap();

    let output = project
        .host()
        .get_component_meta_output("/App.vue")
        .expect("a nested-object call-signature emit payload is representable and must succeed")
        .expect("component must resolve");
    let (analysis, _resolution, types) = output.into_parts();
    let lanes = types.into_lanes();
    let save = analysis
        .events
        .iter()
        .position(|event| event.name == "save")
        .expect("the save event publishes");
    let TypeExpr::Tuple { elements, .. } = &materialized_event_types(&lanes)[save] else {
        panic!(
            "the replayed payload tuple renders; got {:?}",
            materialized_event_types(&lanes)[save]
        );
    };
    assert_eq!(elements.len(), 1);
    assert_eq!(elements[0].label.as_deref(), Some("value"));
    // Published SHALLOW: the object carrier surfaces with its nested member
    // as a resolvable reference — not eagerly flattened, never unknown.
    let TypeExpr::Object(shape) = &elements[0].ty else {
        panic!(
            "the nested-object payload param renders its object carrier, got {:?}",
            elements[0].ty
        );
    };
    let nested = shape
        .properties
        .iter()
        .find_map(|member| match member {
            ObjectMember::Property(property)
                if property.string_name().expect("string-key fixture") == "nested" =>
            {
                Some(property)
            }
            _ => None,
        })
        .expect("the object carrier keeps its nested member");
    assert!(
        matches!(&nested.ty, TypeExpr::Ref { name, .. } if name.as_ref() == "Row"),
        "the nested member stays a shallow resolvable reference, got {:?}",
        nested.ty
    );
    // Consumer WALK: demanding the published source reaches the nested leaf
    // (`value.nested` resolves through `Row` to `id: number`).
    let demanded = demand_published_type(
        project.host(),
        "/App.vue",
        analysis.events[save].payload.present(),
        "save payload",
    );
    let TypeExpr::Tuple {
        elements: demanded_elements,
        ..
    } = &demanded
    else {
        panic!("the demanded payload source materializes the tuple, got {demanded:?}");
    };
    let TypeExpr::Object(demanded_shape) = &demanded_elements[0].ty else {
        panic!(
            "the demanded payload element keeps its object surface, got {:?}",
            demanded_elements[0].ty
        );
    };
    let demanded_nested = demanded_shape
        .properties
        .iter()
        .find_map(|member| match member {
            ObjectMember::Property(property)
                if property.string_name().expect("string-key fixture") == "nested" =>
            {
                Some(property)
            }
            _ => None,
        })
        .expect("the demanded object keeps its nested member");
    match &demanded_nested.ty {
        TypeExpr::Object(row_shape) => assert!(
            row_shape.properties.iter().any(|member| matches!(
                member,
                ObjectMember::Property(property) if property.string_name().expect("string-key fixture") == "id"
            )),
            "the walked nested Row reaches its id leaf, got {demanded:?}"
        ),
        TypeExpr::Ref { name, .. } if name.as_ref() == "Row" => {}
        other => panic!(
            "the walked nested member must resolve (an expanded Row body or \
             the resolvable Row reference), got {other:?}"
        ),
    }
}

/// A RICH call-signature emit — an optional generic-instantiated param plus
/// a rest array param — preserves labels, optionality, rest, ORDER, and the
/// generic substitution through the callable-occurrence replay.
#[test]
fn rich_call_signature_emit_payload_preserves_labels_optionality_rest_and_substitutions() {
    let project = make_project();
    project
        .upsert_base(
            "/events.ts",
            "export interface Row { id: number }\nexport interface Box<T> { boxed: T }\nexport interface Events { (e: 'save', value?: Box<number>, ...rows: Row[]): void }\n",
        )
        .unwrap();
    project
        .upsert_base(
            "/App.vue",
            r#"<script setup lang="ts">
import type { Events } from './events'
defineEmits<Events>()
</script>
<template><div /></template>"#,
        )
        .unwrap();

    let output = project
        .host()
        .get_component_meta_output("/App.vue")
        .expect("a rich call-signature emit payload is representable and must succeed")
        .expect("component must resolve");
    let (analysis, _resolution, types) = output.into_parts();
    let lanes = types.into_lanes();
    let save = analysis
        .events
        .iter()
        .position(|event| event.name == "save")
        .expect("the save event publishes");
    let TypeExpr::Tuple { elements, .. } = &materialized_event_types(&lanes)[save] else {
        panic!(
            "the replayed payload tuple renders; got {:?}",
            materialized_event_types(&lanes)[save]
        );
    };
    assert_eq!(
        elements.len(),
        2,
        "both post-event-name params survive, in order"
    );
    // Element 0: the OPTIONAL generic-instantiated param, substitution
    // preserved (`Box<number>` keeps its type argument).
    assert_eq!(elements[0].label.as_deref(), Some("value"));
    assert!(elements[0].optional, "the authored `?` is preserved");
    assert!(!elements[0].rest);
    let TypeExpr::Ref {
        name,
        type_arguments,
    } = &elements[0].ty
    else {
        panic!(
            "the generic-instantiated param stays a shallow instantiation \
             reference, got {:?}",
            elements[0].ty
        );
    };
    assert_eq!(name.as_ref(), "Box");
    assert_eq!(
        type_arguments.as_ref(),
        &[TypeExpr::Primitive(PrimitiveName::Number)],
        "the generic substitution rides the replay"
    );
    // Element 1: the REST array param.
    assert_eq!(elements[1].label.as_deref(), Some("rows"));
    assert!(elements[1].rest, "the authored rest is preserved");
    assert!(!elements[1].optional);
    let TypeExpr::Array { element, .. } = &elements[1].ty else {
        panic!(
            "the rest param keeps its array shape, got {:?}",
            elements[1].ty
        );
    };
    assert!(
        matches!(element.as_ref(), TypeExpr::Ref { name, .. } if name.as_ref() == "Row"),
        "the rest element stays the shallow resolvable reference, got {element:?}"
    );
}

/// An authored unresolved call-signature parameter remains an explicit,
/// Complete `Ref` inside its payload tuple. Imported and local call signatures
/// share the same carrier-preserving behavior and emit no budget diagnostic.
#[test]
fn call_signature_emit_with_unresolved_param_stays_a_complete_carrier() {
    fn assert_complete_missing_payload(project: &MetaProject) {
        let output = project
            .host()
            .get_component_meta_output("/App.vue")
            .expect("a stable unresolved payload carrier must materialize")
            .expect("the component resolves");
        let (analysis, _resolution, types) = output.into_parts();
        assert!(
            analysis
                .macro_expansion_diagnostics
                .iter()
                .all(|expansion| expansion.diagnostics.is_empty()),
            "a stable unresolved reference must not receive an operational budget diagnostic: {:?}",
            analysis.macro_expansion_diagnostics
        );
        let save = analysis
            .events
            .iter()
            .position(|event| event.name == "save")
            .expect("the save event publishes");
        let lanes = types.into_lanes();
        let TypeExpr::Tuple { elements, .. } = &materialized_event_types(&lanes)[save] else {
            panic!(
                "the callable payload must remain a tuple carrier; got {:?}",
                materialized_event_types(&lanes)[save]
            );
        };
        assert_eq!(elements.len(), 1);
        assert_eq!(elements[0].label.as_deref(), Some("value"));
        assert!(matches!(
            &elements[0].ty,
            TypeExpr::Ref {
                name,
                type_arguments
            } if name.as_ref() == "Missing" && type_arguments.is_empty()
        ));

        let (_analysis, state) = project
            .host()
            .get_component_meta_with_resolution("/App.vue")
            .expect("the stable unresolved payload remains resolvable");
        assert!(
            !state.completeness.is_partial() && !state.synthesis_should_suppress,
            "a stable unresolved callable parameter is Complete and cacheable; got {:?}",
            state.completeness
        );
    }

    let project = make_project();
    project
        .upsert_base(
            "/events.ts",
            "export interface Events { (e: 'save', value: Missing): void }\n",
        )
        .unwrap();
    project
        .upsert_base(
            "/App.vue",
            r#"<script setup lang="ts">
import type { Events } from './events'
defineEmits<Events>()
</script>
<template><div /></template>"#,
        )
        .unwrap();

    assert_complete_missing_payload(&project);

    // The local form routes through the same replay and preserves the same
    // stable carrier.
    let local = make_project();
    local
        .upsert_base(
            "/App.vue",
            r#"<script setup lang="ts">
defineEmits<{ (e: 'save', value: Missing): void }>()
</script>
<template><div /></template>"#,
        )
        .unwrap();
    assert_complete_missing_payload(&local);
}

/// POSITIVE CONTROL: an AUTHORED `unknown` emit payload param is a PRESENT,
/// valid success — the component completes and the payload tuple renders
/// the authored `unknown` element. Authored `unknown` is never a failure.
#[test]
fn authored_unknown_emit_payload_completes_as_valid_success() {
    let project = make_project();
    project
        .upsert_base(
            "/App.vue",
            r#"<script setup lang="ts">
defineEmits<{ (e: 'save', value: unknown): void }>()
</script>
<template><div /></template>"#,
        )
        .unwrap();

    let output = project
        .host()
        .get_component_meta_output("/App.vue")
        .expect("an authored unknown payload is a valid success, never a failure")
        .expect("component must resolve");
    let (analysis, _resolution, types) = output.into_parts();
    let lanes = types.into_lanes();
    let save = analysis
        .events
        .iter()
        .position(|event| event.name == "save")
        .expect("the save event publishes");
    let TypeExpr::Tuple { elements, .. } = &materialized_event_types(&lanes)[save] else {
        panic!(
            "the authored payload tuple renders; got {:?}",
            materialized_event_types(&lanes)[save]
        );
    };
    assert_eq!(elements.len(), 1);
    assert_eq!(
        elements[0].ty,
        TypeExpr::Primitive(PrimitiveName::Unknown),
        "the authored unknown element renders as the author wrote it"
    );
}

/// POSITIVE CONTROL: a genuinely-open index-signature-only emits surface is
/// a valid success (the open position is semantic openness, not a failure).
#[test]
fn open_index_signature_emits_surface_completes_as_valid_success() {
    let project = make_project();
    project
        .upsert_base(
            "/App.vue",
            r#"<script setup lang="ts">
defineEmits<{ [event: string]: [v: number] }>()
</script>
<template><div /></template>"#,
        )
        .unwrap();

    let output = project.host().get_component_meta_output("/App.vue");
    assert!(
        output.is_ok(),
        "a genuinely-open emits index signature is a valid success; got {output:?}"
    );
}

/// POSITIVE CONTROL: a schema-ABSENT unannotated payload position (an
/// array-form runtime emit) renders the centralized typed `unknown` and
/// COMPLETES — honest absence, never a failure.
#[test]
fn unannotated_runtime_emit_renders_typed_unknown_and_completes() {
    let project = make_project();
    project
        .upsert_base(
            "/App.vue",
            r#"<script setup lang="ts">
defineEmits(['save'])
</script>
<template><div /></template>"#,
        )
        .unwrap();

    let output = project
        .host()
        .get_component_meta_output("/App.vue")
        .expect("an unannotated runtime emit is honest absence, never a failure")
        .expect("component must resolve");
    let (analysis, _resolution, types) = output.into_parts();
    let lanes = types.into_lanes();
    let save = analysis
        .events
        .iter()
        .position(|event| event.name == "save")
        .expect("the save event publishes");
    assert_eq!(
        materialized_event_types(&lanes)[save],
        TypeExpr::Unknown(UnknownValue::missing_output()),
        "the unannotated position renders the centralized typed unknown"
    );

    let (_analysis, state) = project
        .host()
        .get_component_meta_with_resolution("/App.vue")
        .expect("resolves");
    assert!(
        !state.completeness.is_partial(),
        "schema absence completes; it is never a failure state"
    );
}

/// POSITIVE CONTROL: a LOCAL call-signature emit with a named-reference
/// payload param keeps its faithful AUTHORED payload tuple — the fail-closed
/// rail must not overfire on locally-authored payloads.
#[test]
fn local_call_signature_emit_with_named_param_stays_a_real_tuple_success() {
    let project = make_project();
    project
        .upsert_base(
            "/App.vue",
            r#"<script setup lang="ts">
interface Row { id: number }
defineEmits<{ (e: 'save', value: Row): void }>()
</script>
<template><div /></template>"#,
        )
        .unwrap();

    let output = project
        .host()
        .get_component_meta_output("/App.vue")
        .expect("a locally-authored call-signature emit succeeds")
        .expect("component must resolve");
    let (analysis, _resolution, types) = output.into_parts();
    let lanes = types.into_lanes();
    let save = analysis
        .events
        .iter()
        .position(|event| event.name == "save")
        .expect("the save event publishes");
    let TypeExpr::Tuple { elements, .. } = &materialized_event_types(&lanes)[save] else {
        panic!(
            "the authored payload tuple renders; got {:?}",
            materialized_event_types(&lanes)[save]
        );
    };
    assert_eq!(elements.len(), 1);
    assert_eq!(elements[0].label.as_deref(), Some("value"));
    assert!(
        matches!(&elements[0].ty, TypeExpr::Ref { name, .. } if name.as_ref() == "Row"),
        "the named payload param stays the shallow authored reference; got {:?}",
        elements[0].ty
    );
}

/// EXHAUSTIVE-arm coverage for one occurrence-owned event publication:
/// `Absent` renders centralized `Unknown`, `Present` materializes its source,
/// and `Failed` produces a typed unsupported public contract at that exact
/// occurrence.
#[test]
fn output_event_occurrence_decides_absent_present_and_failed_arms_exhaustively() {
    let project = make_project();
    project
        .upsert_base(
            "/App.vue",
            r#"<script setup lang="ts">
defineEmits<{ save: [id: number] }>()
</script>
<template><div /></template>"#,
        )
        .unwrap();
    let host = project.host();
    let analysis = host
        .get_component_meta("/App.vue")
        .expect("component must resolve");
    assert_eq!(analysis.events.len(), 1, "fixture declares exactly 1 event");

    // PRESENT arm: the untampered analysis materializes the real source.

    let fixture_dispatch_24 =
        verter_type_engine::project_semantic_dispatch::ProjectSemanticDispatch::new(host);
    let output = crate::meta_resolve::projectors::build_component_meta_output(
        host,
        &fixture_dispatch_24,
        "/App.vue",
        analysis.clone(),
        None,
        crate::meta_resolve::PublishedCompleteness::COMPLETE,
    )
    .expect("a present source materializes");
    let (_analysis, _resolution, types) = output.into_parts();
    let TypeExpr::Tuple { .. } = &materialized_event_types(&types.into_lanes())[0] else {
        panic!("the present payload source materializes the authored tuple");
    };

    // ABSENT arm: a proven schema absence renders the centralized typed
    // `Unknown` and COMPLETES — absence is never a failure.
    let mut absent = analysis.clone();
    replace_event_payload(
        &mut absent.events[0],
        verter_type_expr::facts::SourcePosition::unannotated(),
    );
    let output = crate::meta_resolve::projectors::build_component_meta_output(
        host,
        &fixture_dispatch_24,
        "/App.vue",
        absent,
        None,
        crate::meta_resolve::PublishedCompleteness::COMPLETE,
    )
    .expect("schema absence keeps materializing the output");
    let (_analysis, _resolution, types) = output.into_parts();
    assert_eq!(
        materialized_event_types(&types.into_lanes())[0],
        TypeExpr::Unknown(UnknownValue::missing_output()),
        "the absent position renders the centralized typed Unknown"
    );

    // FAILED arm: the complete occurrence remains in the output envelope and
    // fails the public contract closed at its exact event-overload position.
    let mut failed = analysis;
    replace_event_payload(
        &mut failed.events[0],
        verter_type_expr::facts::SourcePosition::Failed(
            verter_type_expr::facts::SemanticSourceFailure::UnrepresentableRequiredPayload,
        ),
    );
    let output = crate::meta_resolve::projectors::build_component_meta_output(
        host,
        &fixture_dispatch_24,
        "/App.vue",
        failed,
        None,
        crate::meta_resolve::PublishedCompleteness::COMPLETE,
    )
    .expect("a typed failed publication remains an occurrence-owned output row");
    let (_analysis, _resolution, types, contract, _completeness) =
        output.into_parts_with_contract();
    let lanes = types.into_lanes();
    assert!(matches!(
        lanes.events[0].payload.publication(),
        verter_type_expr::PublicationResult::Failed {
            failure: verter_type_expr::TypedResolutionFailure::SourceConstruction(
                verter_type_expr::facts::SemanticSourceFailure::UnrepresentableRequiredPayload
            ),
            provenance: verter_type_expr::ResolutionProvenance::FrameworkSurface,
        }
    ));
    assert!(lanes.events[0].payload.materialized_type().is_none());
    let crate::framework::ComponentContractAvailability::Unsupported(unsupported) = contract else {
        panic!("a failed event occurrence must fail the public contract closed");
    };
    assert_eq!(
        unsupported.reason,
        crate::framework::ComponentContractUnsupportedReason::PublicationFailed {
            surface: crate::framework::ContractSurface::Event {
                name: Arc::from("save"),
                overload_index: 0,
            },
            failure: verter_type_expr::TypedResolutionFailure::SourceConstruction(
                verter_type_expr::facts::SemanticSourceFailure::UnrepresentableRequiredPayload,
            ),
            provenance: verter_type_expr::ResolutionProvenance::FrameworkSurface,
        },
    );
}

/// The EMITS property-style surface preserves a stable unresolved payload arm
/// beside the concrete tuple arm, rather than masking either contributor.
#[test]
fn same_name_intersection_emit_preserves_unresolved_carrier() {
    let project = make_project();
    project
        .upsert_base(
            "/bad-emits.ts",
            "export interface BadEmits { save: MissingType }\n",
        )
        .unwrap();
    project
        .upsert_base(
            "/App.vue",
            r#"<script setup lang="ts">
import type { BadEmits } from './bad-emits'
defineEmits<{ save: [id: number] } & BadEmits>()
</script>
<template><div /></template>"#,
        )
        .unwrap();

    let (analysis, _resolution, types) = project
        .host()
        .get_component_meta_output("/App.vue")
        .expect("the stable unresolved emit carrier materializes")
        .expect("the SFC resolves")
        .into_parts();
    let index = analysis
        .events
        .iter()
        .position(|event| event.name == "save")
        .expect("save event publishes");
    let lanes = types.into_lanes();
    let TypeExpr::Intersection(arms) = &materialized_event_types(&lanes)[index] else {
        panic!(
            "the merged payload preserves both contributors; got {:?}",
            materialized_event_types(&lanes)[index]
        );
    };
    assert!(
        arms.iter()
            .any(|arm| matches!(arm, TypeExpr::Ref { name, type_arguments }
                if name.as_ref() == "MissingType" && type_arguments.is_empty())),
        "the unresolved emit contributor remains an explicit carrier; got {arms:?}"
    );
    assert!(
        arms.iter()
            .any(|arm| matches!(arm, TypeExpr::Tuple { elements, .. }
            if elements.len() == 1
                && elements[0].label.as_deref() == Some("id")
                && matches!(elements[0].ty, TypeExpr::Primitive(PrimitiveName::Number)))),
        "the concrete tuple contributor remains present; got {arms:?}"
    );
    let (_analysis, state) = project
        .host()
        .get_component_meta_with_resolution("/App.vue")
        .expect("component resolves");
    assert!(
        !state.completeness.is_partial() && !state.synthesis_should_suppress,
        "stable unresolved emit carriers remain complete and cacheable; got {:?}",
        state.completeness
    );
}

/// A single stable unresolved EMITS payload remains an explicit Complete
/// carrier; it is not an operational projection miss.
#[test]
fn emit_payload_referencing_unresolved_type_stays_complete_carrier() {
    let project = make_project();
    project
        .upsert_base(
            "/bad-emits.ts",
            "export interface BadEmits { save: MissingType }\n",
        )
        .unwrap();
    project
        .upsert_base(
            "/App.vue",
            r#"<script setup lang="ts">
import type { BadEmits } from './bad-emits'
defineEmits<BadEmits>()
</script>
<template><div /></template>"#,
        )
        .unwrap();

    let (analysis, _resolution, types) = project
        .host()
        .get_component_meta_output("/App.vue")
        .expect("a stable unresolved emit carrier materializes")
        .expect("the SFC resolves")
        .into_parts();
    let index = analysis
        .events
        .iter()
        .position(|event| event.name == "save")
        .expect("save event publishes");
    assert!(
        matches!(
            &materialized_event_types(&types.into_lanes())[index],
            TypeExpr::Ref { name, type_arguments }
                if name.as_ref() == "MissingType" && type_arguments.is_empty()
        ),
        "the authored unresolved payload remains an explicit Ref carrier"
    );
    let (_analysis, state) = project
        .host()
        .get_component_meta_with_resolution("/App.vue")
        .expect("component resolves");
    assert!(
        !state.completeness.is_partial() && !state.synthesis_should_suppress,
        "the stable unresolved payload remains Complete and cacheable; got {:?}",
        state.completeness
    );
}

/// Distributive-conditional truth over a DEFAULTED union type argument
/// (the reka-ui `AccordionRootEmits` shape): the emit payload
/// `(T extends 'single' ? string : string[]) | undefined` with
/// `T extends SingleOrMultiple = SingleOrMultiple` (an ALIAS of
/// `'single' | 'multiple'`) must distribute over the default binding's
/// union constituents — `string | string[] | undefined` — not decide
/// the whole union against `'single'` and collapse to the false branch
/// (`string[] | undefined`, the arm-dropping wrong answer).
#[test]
fn distributive_conditional_over_aliased_defaulted_union_type_arg_distributes_emit_payload() {
    let project = make_project();
    project
        .upsert_base(
            "/types.ts",
            r#"export type SingleOrMultiple = 'single' | 'multiple'
export type RootEmits<T extends SingleOrMultiple = SingleOrMultiple> = {
  'update:modelValue': [value: (T extends 'single' ? string : string[]) | undefined]
}"#,
        )
        .unwrap();
    project
        .upsert_base(
            "/App.vue",
            r#"<script setup lang="ts">
import type { RootEmits } from './types'

export interface AppEmits extends RootEmits {}

defineEmits<AppEmits>()
</script>
<template><div /></template>"#,
        )
        .unwrap();

    let meta = project
        .host()
        .get_component_meta("/App.vue")
        .expect("full meta should resolve");
    let event = meta
        .events
        .iter()
        .find(|event| event.name == "update:modelValue")
        .expect("update:modelValue event should exist");
    let payload_ty = demand_published_type(
        project.host(),
        "/App.vue",
        event.payload.present(),
        "update:modelValue payload",
    );
    let TypeExpr::Tuple { elements, .. } = &payload_ty else {
        panic!("object-emits payload should materialize as a tuple, got {payload_ty:?}");
    };
    assert_eq!(elements.len(), 1, "model update has a single payload");
    let members = flatten_union_arms(&elements[0].ty);
    assert!(
        members.len() > 1,
        "the distributed conditional payload should be a union, got {:?}",
        elements[0].ty
    );
    assert!(
        members.contains(&&TypeExpr::Primitive(PrimitiveName::String)),
        "distribution over the defaulted union keeps the `'single'` (true-branch) arm `string`, got {members:?}"
    );
    assert!(
        members
            .iter()
            .any(|member| matches!(member, TypeExpr::Array { element, .. } if matches!(element.as_ref(), TypeExpr::Primitive(PrimitiveName::String)))),
        "distribution over the defaulted union keeps the `'multiple'` (false-branch) arm `string[]`, got {members:?}"
    );
    assert!(
        members.contains(&&TypeExpr::Primitive(PrimitiveName::Undefined)),
        "the authored `| undefined` arm survives, got {members:?}"
    );
}

/// Inline-default sibling of
/// [`distributive_conditional_over_aliased_defaulted_union_type_arg_distributes_emit_payload`]:
/// the default is the INLINE union `'a' | 'b'` (no alias indirection).
/// Distribution must produce `string | string[]` here too.
#[test]
fn distributive_conditional_over_inline_defaulted_union_type_arg_distributes_emit_payload() {
    let project = make_project();
    project
        .upsert_base(
            "/App.vue",
            r#"<script setup lang="ts">
type E<T = 'a' | 'b'> = { p: [payload: T extends 'a' ? string : string[]] }

defineEmits<E>()
</script>
<template><div /></template>"#,
        )
        .unwrap();

    let meta = project
        .host()
        .get_component_meta("/App.vue")
        .expect("full meta should resolve");
    let event = meta
        .events
        .iter()
        .find(|event| event.name == "p")
        .expect("p event should exist");
    let payload_ty = demand_published_type(
        project.host(),
        "/App.vue",
        event.payload.present(),
        "p payload",
    );
    let TypeExpr::Tuple { elements, .. } = &payload_ty else {
        panic!("object-emits payload should materialize as a tuple, got {payload_ty:?}");
    };
    let TypeExpr::Union(members) = &elements[0].ty else {
        panic!(
            "the distributed conditional payload should be a union, got {:?}",
            elements[0].ty
        );
    };
    assert!(
        members.contains(&TypeExpr::Primitive(PrimitiveName::String))
            && members
                .iter()
                .any(|member| matches!(member, TypeExpr::Array { element, .. } if matches!(element.as_ref(), TypeExpr::Primitive(PrimitiveName::String)))),
        "distribution over the inline defaulted union produces `string | string[]`, got {members:?}"
    );
}

/// PUBLIC BOUNDARY, RENDERED BYTES — the `defineEmits` twin of the
/// spread-source refusal.
///
/// A spread source the evaluator cannot produce a surface for is a fact
/// about the literal's KEY SET, so the literal fails closed and the emits
/// projection has no member set. Publishing `["evB"]` would declare an
/// event set the source's unknown keys can extend — an `evA` event it
/// carries routes every `@evA` listener to `$attrs` instead of the declared
/// emit. Refusing is the block's deliberate trade and it needs a landed
/// assertion of its own: the
/// props side is pinned by
/// `a_root_position_flow_degradation_refuses_instead_of_publishing_empty_props`,
/// and nothing pinned the emits side.
///
/// The spread source calls `notDeclared`, a name declared nowhere (TS2304),
/// whose error type is recovery the flow-return lane does not model.
///
/// Oracle (TypeScript 7.0.2 `tsc`, `--noEmit --strict --ignoreConfig`):
/// `ReturnType<typeof makeEmits>` is `any` — the spread of the call's error
/// type — so there is no event set to publish, and `["evB"]` would declare
/// one.
///
/// Discrimination: publishing `["evB"]` again fails the `Refused` match;
/// the control row (a MODELLED spread source) fails under a blanket
/// "any spread refuses" regression.
#[cfg(not(target_arch = "wasm32"))]
#[test]
fn an_unevaluable_emits_spread_source_refuses_rather_than_dropping_the_event() {
    match render_runtime_emits(
        "/src/E3UndeclaredSpread.vue",
        "function makeEmits() { return { ...notDeclared(), evB: (n: number) => true } }",
    ) {
        RenderedRuntime::Refused => {}
        RenderedRuntime::Props(emitted) => panic!(
            "/src/E3UndeclaredSpread.vue: the spread source has no evaluable surface, so the event \
             set is incomplete — publishing `{emitted}` drops `evA` and routes its listeners \
             to `$attrs`"
        ),
    }

    // CONTROL — a MODELLED spread source leaves a complete event set and
    // must still publish both events.
    let RenderedRuntime::Props(emitted) = render_runtime_emits(
        "/src/E4ModelledSpread.vue",
        "function base() { return { evA: (p: string) => true } }\n\
         function makeEmits() { return { ...base(), evB: (n: number) => true } }",
    ) else {
        panic!("/src/E4ModelledSpread.vue: a modelled spread source leaves a complete event set");
    };
    assert!(
        emitted.contains("\"evA\"") && emitted.contains("\"evB\""),
        "/src/E4ModelledSpread.vue: both events must survive:\n{emitted}"
    );
}
