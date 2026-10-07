use super::*;

/// `get_component_meta_batch` performs a **single** scheduler
/// submission per batch dispatch. `submit_count` increases by exactly
/// one, independent of `jobs.len()` (R7 / R8 — one view, one
/// scheduler context, shared admissions). Every per-id result resolves
/// to the same shape the synchronous `get_component_meta` path returns,
/// so callers can rely on observable equivalence between the two
/// execution modes while only the fan-out characteristic differs.
#[test]
fn get_component_meta_batch_dispatches_through_scheduler() {
    use std::sync::atomic::Ordering;
    let project = make_project();
    project
        .upsert_base(
            "/src/A.vue",
            r#"<script setup lang="ts">defineProps<{ a: string }>()</script><template><div /></template>"#,
        )
        .unwrap();
    project
        .upsert_base(
            "/src/B.vue",
            r#"<script setup lang="ts">defineProps<{ b: number }>()</script><template><div /></template>"#,
        )
        .unwrap();
    project
        .upsert_base(
            "/src/C.vue",
            r#"<script setup lang="ts">defineProps<{ c: boolean }>()</script><template><div /></template>"#,
        )
        .unwrap();

    let session = project.open_session_batch().expect("batch session");
    let scheduler = project.host().scheduler();
    let baseline_submit = scheduler.counters().submit_count.load(Ordering::Relaxed);
    let canonical_ids = vec![
        "/src/A.vue".to_string(),
        "/src/B.vue".to_string(),
        "/src/C.vue".to_string(),
    ];
    let results = session
        .get_component_meta_batch(&canonical_ids)
        .expect("batch dispatch should complete");
    assert_eq!(results.len(), 3, "one result per submitted job");
    for (canonical, result) in canonical_ids.iter().zip(results.iter()) {
        let analysis = result
            .as_ref()
            .unwrap_or_else(|err| panic!("batch result for {canonical} failed: {err:?}"))
            .as_ref()
            .unwrap_or_else(|| panic!("batch result for {canonical} missing analysis"));
        assert!(
            !analysis.props.is_empty(),
            "batch result for {canonical} should carry its own defineProps shape",
        );
    }
    let after_submit = scheduler.counters().submit_count.load(Ordering::Relaxed);
    assert_eq!(
        after_submit - baseline_submit,
        1,
        "batch dispatch is O(1) per batch: \
         scheduler.counters.submit_count MUST bump by exactly 1 \
         regardless of N=3 jobs (baseline={baseline_submit} after={after_submit})",
    );
}

/// Route/mode-INDEPENDENT L1: a Table.vue-shaped HERMETIC SFC. A generic
/// SFC (`generic="T"`) whose props interface `extends Omit<CoreOptions<T>,
/// 'data'>` — an open object-filter over the OPEN `CoreOptions<T>` heritage,
/// the structural decl-body-lowering route that ran away on the real
/// Table.vue. The carrier-stop keeps the open `Omit` a shallow carrier while
/// the enclosing decl still materialises its OWN closed members.
///
/// The source's KEY DOMAIN is genuinely open: `CoreOptions<T>` intersects
/// the unbound outer `T` (`{ … } & T`), so the enumerable member-name set
/// depends on `T`. A fixed-key generic body with `T` confined to member
/// VALUE positions is the CLOSED key-domain class — it materialises
/// path-precisely (`Omit<Foo<T>, 'items'>` publishes `label`) and is pinned
/// by `omit_wrapped_sfc_generic_param_via_wildcard_resolves`; this fixture
/// pins the complementary OPEN class on the structural route.
///
/// **Discriminating.** Pre-change the structural heritage `Omit<CoreOptions<T>,
/// 'data'>` MATERIALISES its source, flattening `columns`/`rowCount` into the
/// published surface (and the runaway budget can leak a sentinel). Post-change
/// it carrier-stops: the open-domain members do NOT flatten, the enclosing
/// `caption`/`sticky` still publish, and the named `options` field publishes as
/// an `Omit` carrier `Ref`. The `columns`/`rowCount`-absent + no-sentinel
/// assertions FAIL pre-change and PASS post-change.
#[test]
fn get_component_meta_table_shaped_open_omit_heritage_carrier_stops_complete() {
    let project = make_project();
    project
        .upsert_base(
            "/Table.vue",
            r#"<script lang="ts">
export type CoreOptions<T> = {
  data: T
  columns: number
  rowCount: number
} & T

export interface TableProps<T> extends Omit<CoreOptions<T>, 'data'> {
  caption?: string
  sticky?: boolean
  options?: Omit<CoreOptions<T>, 'data'>
}
</script>

<script setup lang="ts" generic="T">
defineProps<TableProps<T>>()
</script>
<template><div /></template>"#,
        )
        .unwrap();

    let meta = get_meta(&project, "/Table.vue");
    let prop_names: Vec<&str> = meta.props.iter().map(|p| p.name.as_str()).collect();

    // Own closed members of the enclosing decl materialise.
    assert!(
        prop_names.contains(&"caption"),
        "the enclosing decl's own closed `caption` prop must publish, got: {prop_names:?}"
    );
    assert!(
        prop_names.contains(&"sticky"),
        "the enclosing decl's own closed `sticky` prop must publish, got: {prop_names:?}"
    );

    // The open `Omit<CoreOptions<T>, 'data'>` heritage carrier-stops: its
    // members (`columns`, `rowCount`) do NOT flatten into the surface
    // (shallow-by-default over an OPEN enumeration domain).
    assert!(
        !prop_names.contains(&"columns") && !prop_names.contains(&"rowCount"),
        "the OPEN `Omit<CoreOptions<T>, 'data'>` heritage must carrier-stop — its members \
         must NOT flatten into the published surface, got: {prop_names:?}"
    );
    // `data` is omitted by the filter and is never reachable regardless.
    assert!(
        !prop_names.contains(&"data"),
        "`data` is omitted by the Omit filter and must never publish, got: {prop_names:?}"
    );

    // The named `options` field publishes as a shallow `Omit` carrier `Ref`.
    let options = meta
        .props
        .iter()
        .find(|p| p.name == "options")
        .expect("`options` prop must publish");
    match &shallow_published_type(
        project.host(),
        "/Table.vue",
        options.publication.result().selected_source(),
        "options prop",
    ) {
        TypeExpr::Ref { name, .. } => assert_eq!(
            name.as_ref(),
            "Omit",
            "the open `options: Omit<CoreOptions<T>, 'data'>` field must publish as an \
             `Omit` carrier Ref"
        ),
        other => panic!("`options` must be an `Omit` carrier Ref, got {other:?}"),
    }

    // Typed no-leak guard (replaces the raw-spelling scan): the resolved meta
    // was ADMITTED to the resolved-meta cache — admission is complete-only, so
    // a budget-tripped partial surface would have been refused (the negative
    // class is pinned by the partial-fixture / budget-exhaustion admission
    // tests).
    assert!(
        cached_resolved_state(&project, "/Table.vue", verter_type_engine::semantic_query::ProjectionMode::Expanded)
            .is_some(),
        "the Table.vue resolution must be admitted complete — a leaked budget partial would refuse admission"
    );
    assert_no_degraded_props(project.host(), "/Table.vue", &meta);
}

/// Route/mode-INDEPENDENT L1: a ChatMessages.vue-shaped HERMETIC SFC. A
/// generic SFC whose named props interface `extends Pick<MessageProps<T>, …>`
/// — an open object-filter (`Pick`) over the OPEN generic source
/// `MessageProps<T>` imported cross-file. The heritage flows through the
/// structural materialise (Expanded) route — the same route the real
/// ChatMessages.vue storms on — NOT the inline-object Navigate projector
/// (which already carrier-stops named-member open Picks at hermetic scale).
///
/// **Scope honesty.** The real ChatMessages.vue Pick source is a *chained
/// conditional* (`PropsBase<T> = MessageBase<T> extends … ? … : never`); a
/// hermetic conditional source yields `semanticMiss` downstream pre-change
/// (the separate conditional-reduction gap), so it does not flatten and
/// cannot serve as a clean RED→GREEN discriminator. This fixture therefore
/// uses a PLAIN (non-conditional) Pick source whose KEY DOMAIN is genuinely
/// open — `MessageProps<T>` intersects the unbound outer `T`, so the
/// enumerable member-name set depends on `T` (a fixed-key body with `T`
/// confined to VALUE positions is the CLOSED class and materialises
/// path-precisely instead). The conditional-source Expanded registry-route
/// storm is reproduced and gated by the external-corpus gate (the real
/// oracle).
///
/// **Discriminating.** The open-domain source is NEVER whole-materialised:
/// the un-picked `side` member must NOT flatten into the published surface
/// (whole-source flattening — the storm class — would publish it), and no
/// published field carries a budget sentinel. `Pick`'s OUTPUT keys are the
/// CLOSED key-selection `K` even over the open source, so the SURFACE
/// demand (a heritage arm IS the published props surface) enumerates
/// exactly the picked `icon`/`avatar` from the source's ENUMERABLE object
/// arm (values shallow; the open `& T` arm contributes nothing) — dropping
/// them was the nuxt-ui ContentSearch/DropdownMenuContent zero-member
/// collapse. VALUE-position publication of an open Pick stays a shallow
/// carrier (`chatmessages_resolvable_barrel_publishes_open_pick_as_shallow_carrier`).
#[test]
fn get_component_meta_chat_messages_shaped_open_pick_heritage_enumerates_picked_keys_only() {
    let project = make_project();
    project
        .upsert_base(
            "/chat-types.ts",
            r#"export type MessageProps<T> = {
  icon?: string
  avatar?: string
  side?: 'left' | 'right'
} & T

// A NAMED generic props interface whose HERITAGE is an open Pick over the
// open generic source. `defineProps<ChatProps<T>>()` resolves this through
// the structural materialise (Expanded) route.
export interface ChatProps<T> extends Pick<MessageProps<T>, 'icon' | 'avatar'> {
  caption?: string
}
"#,
        )
        .unwrap();
    project
        .upsert_base(
            "/ChatMessages.vue",
            r#"<script setup lang="ts" generic="T extends unknown[]">
import type { ChatProps } from './chat-types'

defineProps<ChatProps<T>>()
</script>
<template><div /></template>"#,
        )
        .unwrap();

    let meta = get_meta(&project, "/ChatMessages.vue");
    let prop_names: Vec<&str> = meta.props.iter().map(|p| p.name.as_str()).collect();

    // The enclosing decl's own closed member materialises.
    assert!(
        prop_names.contains(&"caption"),
        "the enclosing decl's own `caption` prop must publish, got: {prop_names:?}"
    );

    // `Pick`'s OUTPUT keys are its CLOSED key-selection even over the OPEN
    // source (`& T` widens the SOURCE's key domain, not the Pick's output
    // set), so the surface-position heritage enumerates exactly the picked
    // members from the source's enumerable object arm.
    assert!(
        prop_names.contains(&"icon") && prop_names.contains(&"avatar"),
        "the `Pick<MessageProps<T>, 'icon' | 'avatar'>` heritage must publish its picked \
         members from the source's enumerable arm, got: {prop_names:?}"
    );

    // The un-picked `side` member must NOT flatten — whole-source
    // materialisation of the open generic source (the ChatMessages.vue
    // storm class) would publish it; the filtered surface-position
    // enumeration must not.
    assert!(
        !prop_names.contains(&"side"),
        "the un-picked `side` member must NOT flatten into the published surface — the OPEN \
         source must never whole-materialise, got: {prop_names:?}"
    );

    // Typed no-leak guard (replaces the raw-spelling scan): the resolved meta
    // was ADMITTED to the resolved-meta cache — admission is complete-only.
    assert!(
        cached_resolved_state(&project, "/ChatMessages.vue", verter_type_engine::semantic_query::ProjectionMode::Expanded)
            .is_some(),
        "the ChatMessages.vue resolution must be admitted complete — a leaked budget partial would refuse admission"
    );
    assert_no_degraded_props(project.host(), "/ChatMessages.vue", &meta);
}

/// Q10 arg-preserving publication for a PAYLOAD-LESS resolved-surface member:
/// `defineProps<PropsBase>()` where the imported `PropsBase` declares
/// `message: MessageBase<string>`. The member has NO flat authored
/// macro-payload position (`shallow_payload` is `None`), so its publication
/// previously took the LOSSY `InstantiationRef` upgrade
/// (`Synthesized(Ref(SymbolBodyLocator))` — scope kept, type arguments
/// DROPPED). The published source must instead be the authored USE-SITE
/// body slot (`PropsBase.message`'s member-value position): content-free,
/// non-executed, and arg-preserving — its deref through the one shared
/// dispatch replays `MessageBase<string>` with the substitution intact.
///
/// Discriminating: shell-materializing the published source must yield
/// `Ref { name: "MessageBase", type_arguments: [string] }` — the lossy
/// carrier shell-materializes with EMPTY type_arguments and fails.
#[test]
fn get_component_meta_payloadless_member_publishes_arg_preserving_instantiation_source() {
    let project = make_project();
    project
        .upsert_base(
            "/types.ts",
            r#"
export interface MessageBase<T> { content: T }
export interface PropsBase { message: MessageBase<string> }
"#,
        )
        .unwrap();
    project
        .upsert_base(
            "/Comp.vue",
            r#"<script setup lang="ts">
import type { PropsBase } from './types'
defineProps<PropsBase>()
</script>
<template><div /></template>"#,
        )
        .unwrap();

    let meta = get_meta(&project, "/Comp.vue");
    let prop = meta
        .props
        .iter()
        .find(|p| p.name == "message")
        .expect("the imported PropsBase surface must publish the `message` prop");
    let source = prop
        .publication
        .result()
        .selected_source()
        .expect("`message` must publish a typed source");

    // Canonical identity: the authored use-site slot anchors on the
    // DECLARING file + symbol (never a bare name re-resolved in the owner
    // scope, never an argument-less symbol ref).
    let verter_type_expr::facts::SemanticTypeSource::Authored(
        verter_type_expr::locators::AuthoredBodyLocator::DeclBody(slot),
    ) = source
    else {
        panic!(
            "a payload-less instantiation-valued member must publish the authored \
             use-site DeclBody carrier (arg-preserving, re-resolvable); observed \
             {source:?}",
        );
    };
    assert_eq!(
        slot.anchor.canonical_id.as_ref(),
        "/types.ts",
        "the use-site slot must anchor on the declaring canonical",
    );
    assert_eq!(
        slot.anchor.symbol.as_ref(),
        "PropsBase",
        "the use-site slot must anchor on the declaring symbol",
    );

    // Arg preservation: the shallow (non-demanded) shape replays the
    // authored instantiation with its concrete `string` argument.
    let shallow = shallow_published_type(project.host(), "/Comp.vue", Some(source), "message");
    let TypeExpr::Ref {
        name,
        type_arguments,
    } = &shallow
    else {
        panic!(
            "`message` must shell-materialize to the MessageBase reference carrier; \
             observed {shallow:?}",
        );
    };
    assert_eq!(name.as_ref(), "MessageBase");
    assert_eq!(
        type_arguments.len(),
        1,
        "the instantiation's type argument must be PRESERVED on the published \
         source (the lossy Synthesized(Ref) upgrade drops it); observed \
         {type_arguments:?}",
    );
    assert!(
        matches!(
            &type_arguments[0],
            TypeExpr::Primitive(PrimitiveName::String)
        ),
        "the preserved argument must be the concrete authored `string`; observed {:?}",
        type_arguments[0],
    );
}

#[test]
fn get_component_meta_uses_single_native_query_path() {
    let project = make_project();
    project
        .upsert_base("/App.vue", &sfc("msg: string"))
        .unwrap();

    project.host().provenance().reset();
    let session = project.open_session_batch().unwrap();

    let _meta = session
        .get_component_meta("/App.vue")
        .unwrap()
        .expect("get_component_meta should succeed");

    let p = provenance(&project);

    // Assert+: the new query was called
    assert_eq!(
        p.get_component_meta_calls, 1,
        "get_component_meta should record one call"
    );

    // Assert+: resolved state was computed at most once
    assert!(
        p.component_meta_resolved_state_recomputes <= 1,
        "get_component_meta should compute resolved state at most once, got: {}",
        p.component_meta_resolved_state_recomputes
    );
}

#[test]
fn get_component_meta_returns_consistent_results_on_repeated_calls() {
    let project = make_project();
    project
        .upsert_base("/App.vue", &sfc("msg: string"))
        .unwrap();

    let session = project.open_session_batch().unwrap();

    // First call
    let first = session
        .get_component_meta("/App.vue")
        .unwrap()
        .expect("first call should return metadata");

    // Second call — should return consistent results
    let second = session
        .get_component_meta("/App.vue")
        .unwrap()
        .expect("second call should return metadata");

    // Assert+: both calls return the same props
    assert_eq!(
        first.props.len(),
        second.props.len(),
        "repeated calls should return the same number of props"
    );
    assert_eq!(
        first.props[0].name, second.props[0].name,
        "repeated calls should return the same prop names"
    );

    // Assert-: no extra events/models introduced
    assert!(
        first.events.is_empty() && second.events.is_empty(),
        "no defineEmits means no events on either call"
    );
    assert!(
        first.models.is_empty() && second.models.is_empty(),
        "no defineModel means no models on either call"
    );
}

#[test]
fn get_component_meta_does_not_call_public_evaluate_types_workflow() {
    let project = make_project();
    project
        .upsert_base("/App.vue", &sfc("msg: string"))
        .unwrap();

    project.host().provenance().reset();
    let session = project.open_session_batch().unwrap();

    let _meta = session.get_component_meta("/App.vue").unwrap().unwrap();
    let p = provenance(&project);

    assert_eq!(
        p.evaluate_types_calls, 0,
        "native get_component_meta must not route through the public evaluate_types workflow"
    );
}

#[test]
fn get_component_meta_cold_path_does_not_call_public_get_analysis_workflow() {
    let project = make_project();
    project
        .upsert_base("/App.vue", &sfc("msg: string"))
        .unwrap();

    project.host().provenance().reset();
    let session = project.open_session_batch().unwrap();

    let _meta = session
        .get_component_meta("/App.vue")
        .unwrap()
        .expect("get_component_meta should succeed");
    let p = provenance(&project);

    assert_eq!(
        p.get_analysis_calls, 0,
        "native get_component_meta must not route through the public get_analysis workflow",
    );
}

#[test]
fn get_component_meta_returns_full_native_metadata_contract() {
    let project = make_project();
    project
        .upsert_base(
            "/FancyButton.vue",
            r#"<script setup lang="ts">
defineProps<{ label: string; modelValue: number }>()
</script>
<template><button><slot /></button></template>"#,
        )
        .unwrap();
    project
        .upsert_base(
            "/App.vue",
            r#"<script setup lang="ts">
import { computed, onMounted, ref } from 'vue'
import FancyButton from './FancyButton.vue'

const count = ref(0)
const accentColor = "red"
const doubled = computed(() => count.value * 2)

onMounted(() => {
  console.log(count.value)
})
</script>
<template>
  <FancyButton
    id="wrapper"
    ref="button"
    :label="`${doubled}`"
    class="primary"
    :class="{ active: count > 0 }"
    v-model="count"
  >
    <template #default>{{ count }}</template>
  </FancyButton>
</template>
<style scoped module="theme">
#wrapper .primary {
  color: v-bind(accentColor);
  --accent: red;
}
</style>"#,
        )
        .unwrap();

    let session = project.open_session_batch().unwrap();
    let meta = session
        .get_component_meta("/App.vue")
        .unwrap()
        .expect("get_component_meta should return metadata");

    assert_eq!(
        meta.components.len(),
        1,
        "template component usage should be present"
    );
    assert_eq!(meta.components[0].name, "FancyButton");
    assert_eq!(
        meta.components[0].import_source.as_deref(),
        Some("./FancyButton.vue")
    );
    assert!(!meta.components[0].has_spread);
    assert!(meta.components[0].has_dynamic_class);
    assert_eq!(meta.components[0].v_models, vec!["modelValue".to_string()]);
    assert_eq!(
        meta.components[0]
            .v_model_entries
            .iter()
            .map(|entry| entry.binding_name.as_str())
            .collect::<Vec<_>>(),
        vec!["modelValue"]
    );
    let label_prop = meta.components[0]
        .props
        .iter()
        .find(|prop| prop.name == "label")
        .expect("label prop usage should be present");
    assert_eq!(label_prop.expression.as_deref(), Some("`${doubled}`"));
    assert_eq!(label_prop.referenced_bindings, vec!["doubled".to_string()]);
    assert!(!label_prop.from_spread);
    assert!(!label_prop.is_shorthand);

    assert_eq!(
        meta.template_refs.len(),
        1,
        "template refs should be present"
    );
    assert_eq!(meta.template_refs[0].name, "button");
    assert_eq!(meta.template_refs[0].target_tag, "FancyButton");

    let child_meta = session
        .get_component_meta("/FancyButton.vue")
        .unwrap()
        .expect("child component meta should be available");
    let public_instance = child_meta
        .public_instance
        .as_ref()
        .expect("host should provide a public-instance sidecar");
    let public_member_names: Vec<_> = public_instance
        .members
        .iter()
        .map(|member| member.name.as_str())
        .collect();
    assert!(
        public_member_names.contains(&"label"),
        "public instance should expose declared props, got {:?}",
        public_member_names
    );
    assert!(
        public_member_names.contains(&"modelValue"),
        "public instance should expose model props, got {:?}",
        public_member_names
    );
    assert!(
        public_member_names.contains(&"$slots"),
        "public instance should expose $slots, got {:?}",
        public_member_names
    );
    assert!(
        public_instance.members.iter().any(|member| {
            member.name == "$slots"
                && matches!(
                    member.kind,
                    verter_session_query::analysis::component_meta::PublicInstanceMemberKind::SlotContainer,
                )
        }),
        "$slots should be tagged as a public-instance slot container"
    );

    assert!(
        meta.imports.iter().any(|import| import.source == "vue"),
        "script imports should be preserved"
    );
    assert!(
        meta.bindings
            .iter()
            .any(|binding| binding.name == "count" && binding.used_in_template),
        "bindings should preserve template usage information"
    );
    assert!(
        meta.vue_api_calls.iter().any(|call| matches!(
            call.api,
            verter_session_query::analysis::types::VueApiClassification::OnMounted,
        )),
        "Vue API calls should be preserved"
    );
    assert_eq!(meta.styles.len(), 1, "style metadata should be present");
    assert_eq!(meta.styles[0].classes, vec!["primary".to_string()]);
    assert_eq!(meta.styles[0].ids, vec!["wrapper".to_string()]);
    assert_eq!(
        meta.styles[0].custom_properties,
        vec!["--accent".to_string()]
    );
    assert_eq!(meta.styles[0].v_binds, vec!["accentColor".to_string()]);
    assert!(
        meta.styles[0]
            .selectors
            .iter()
            .any(|selector| selector.text == "#wrapper .primary"),
        "style selectors should be preserved"
    );
}

#[test]
fn svelte_component_meta_carries_template_usage_facts() {
    // THE discriminating public E2E: a `.svelte` file resolved through
    // `get_component_meta` carries its template component-USAGE facts on the
    // published `ComponentMetaBody.components`. It is RED if Svelte's
    // `template_data` is an empty stub OR template-data ingestion is Vue-gated
    // (Svelte never reaches the public surface); GREEN with registry-dispatched
    // ingestion + the typed-IR producer. A producer alone (a populated
    // `template_data` not yet wired into the public surface) is insufficient.
    let project = make_project();
    project
        .upsert_base(
            "/App.svelte",
            r#"<script lang="ts">
  import Button from './Button.svelte';
  let value = 0;
  function handler() {}
</script>
<Button size="sm" online={value} onclick={handler} bind:value on:focus={handler}>
  {#snippet icon()}x{/snippet}
</Button>
"#,
        )
        .unwrap();

    let session = project.open_session_batch().unwrap();
    let meta = session
        .get_component_meta("/App.svelte")
        .unwrap()
        .expect("get_component_meta should return metadata for a .svelte file");

    assert_eq!(
        meta.components.len(),
        1,
        "the Svelte template component usage must reach the public meta surface"
    );
    let usage = &meta.components[0];
    assert_eq!(usage.name, "Button");
    assert!(!usage.is_dynamic);

    // The `size`, `online`, and `onclick` PROPS are published. `online` and
    // `onclick` are PLAIN `on*` attributes — they are PROPS, never fabricated as
    // events by a name guess (the P1 regression this discriminates).
    let prop_names: Vec<&str> = usage.props.iter().map(|p| p.name.as_str()).collect();
    assert_eq!(
        prop_names,
        vec!["size", "online", "onclick"],
        "plain attrs (incl. `online`/`onclick`) reach the surface as PROPS"
    );
    // `bind:value` is NOT a prop / NOT a v_model — it is a neutral BINDING.
    assert!(!prop_names.contains(&"value"));

    let binding_names: Vec<&str> = usage.bindings.iter().map(|b| b.name.as_str()).collect();
    assert_eq!(
        binding_names,
        vec!["value"],
        "`bind:value` reaches the surface"
    );

    // Only the LEGACY `on:focus` directive is a neutral EVENT — the plain `on*`
    // attributes (`online`, `onclick`) are NOT events.
    let event_names: Vec<&str> = usage.events.iter().map(|e| e.name.as_str()).collect();
    assert_eq!(
        event_names,
        vec!["focus"],
        "only the legacy `on:` directive reaches the surface as an event"
    );
    // HARD INVARIANT: no plain `on*` attribute is misclassified as an event.
    assert!(!event_names.contains(&"online"));
    assert!(!event_names.contains(&"onclick"));

    // The passed `{#snippet icon}` is recorded in slots_used.
    assert_eq!(usage.slots_used, vec!["icon".to_string()]);
}

#[test]
fn get_component_meta_surfaces_registered_ordered_structure() {
    let project = make_project();
    project
        .upsert_base(
            "/Button.vue",
            r#"<script lang="ts">
export const legacy = true
</script>
<script setup lang="ts" generic="T extends string = string" attrs="ButtonAttrs">
defineProps<{ label: string }>()
defineSlots<{
  default(props: { item: number }): any
}>()
defineExpose({
  focus() {}
})
</script>
<template lang="html" data-layout="stack">
  <button>{{ label }}</button>
  <slot :item="1" />
</template>
<style scoped module="theme" lang="scss">
.primary { color: red; }
</style>
<i18n lang="json">
{ "label": "Button" }
</i18n>"#,
        )
        .unwrap();

    let session = project.open_session_batch().unwrap();
    let meta = session
        .get_component_meta("/Button.vue")
        .unwrap()
        .expect("component meta should be available");

    let blocks = meta
        .ordered_sfc_structure
        .as_ref()
        .expect("host should surface registered structure");
    assert_eq!(blocks.schema_version, 1);
    assert_eq!(blocks.artifact_token.len(), 43);
    assert_eq!(blocks.block_tokens.len(), blocks.inventory.blocks().len());
    assert_eq!(
        blocks.markup_node_tokens.len(),
        blocks.inventory.markup().nodes().len()
    );
    assert!(blocks
        .block_tokens
        .iter()
        .all(|token| token.len() == 43 && token != &blocks.artifact_token));
    let has_data_layout = blocks.inventory.blocks().iter().any(|block| {
        let verter_language::parse_artifact::carrier_inventory::CarrierBlock::Section {
            syntax,
            ..
        } = block
        else {
            return false;
        };
        syntax.attributes.iter().any(|attribute| match attribute {
            verter_language::parse_artifact::carrier_inventory::CarrierAttribute::Named {
                name,
                ..
            } => blocks.inventory.slice(name.authored).ok() == Some("data-layout"),
            _ => false,
        })
    });
    assert!(has_data_layout, "inventory preserves arbitrary attributes");
}

#[test]
fn get_component_meta_preserves_component_spread_usage() {
    let project = make_project();
    project
        .upsert_base(
            "/FancyButton.vue",
            r#"<script setup lang="ts">
defineProps<{ label?: string }>()
</script>
<template><button><slot /></button></template>"#,
        )
        .unwrap();
    project
        .upsert_base(
            "/App.vue",
            r#"<script setup lang="ts">
import FancyButton from './FancyButton.vue'

const attrs = { label: 'Hello' }
</script>
<template>
  <FancyButton v-bind="attrs" />
</template>"#,
        )
        .unwrap();

    let session = project.open_session_batch().unwrap();
    let meta = session
        .get_component_meta("/App.vue")
        .unwrap()
        .expect("get_component_meta should return metadata");

    assert_eq!(meta.components.len(), 1);
    assert!(
        meta.components[0].has_spread,
        "component usage should preserve v-bind spread markers"
    );
}

#[test]
fn double_script_same_file_visibility_in_component_meta() {
    let project = make_project();
    project
        .upsert_base(
            "/App.vue",
            r#"<script lang="ts">
export interface SharedProps { shared: boolean }
</script>
<script setup lang="ts">
defineProps<SharedProps>()
</script>
<template><div /></template>"#,
        )
        .unwrap();

    let session = project.open_session_batch().unwrap();
    let meta = session
        .get_component_meta("/App.vue")
        .unwrap()
        .expect("get_component_meta should succeed");

    // Assert+: prop from sibling script block
    assert_eq!(
        meta.props.len(),
        1,
        "should have 1 prop from sibling script"
    );
    assert_eq!(meta.props[0].name, "shared");

    // Assert-: no unresolved types or errors — prop should be fully resolved
    assert!(
        meta.props[0]
            .publication
            .evidence()
            .map(|evidence| evidence.text().to_string())
            .is_some(),
        "shared prop should have a resolved raw type"
    );
}

#[test]
fn package_pick_heritage_survives_local_indexed_access_helpers_in_component_meta() {
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
            "/src/tv.ts",
            r#"
export type ComponentConfig<TTheme> = {
  variants: {
    color: 'primary' | 'secondary'
    size: 'sm' | 'md'
  }
  slots: {
    root?: string
    list?: string
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
    size: { sm: '', md: '' }
  },
  slots: {
    root: '',
    list: ''
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
import theme from './theme'

type Tabs = ComponentConfig<typeof theme>

export interface Props extends Pick<TabsRootProps<string | number>, 'defaultValue' | 'modelValue' | 'activationMode' | 'unmountOnHide'> {
  color?: Tabs['variants']['color']
  ui?: Tabs['slots']
}

export interface Emits extends TabsRootEmits<string | number> {}
</script>
<script setup lang="ts">
defineProps<Props>()
defineEmits<Emits>()
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
        let fixture_dispatch_1 =
            verter_type_engine::project_semantic_dispatch::ProjectSemanticDispatch::new(ctx);

        crate::host_manage::extract_component_meta_from_resolved(
            project.host(),
            "/src/App.vue",
            &resolved,
            true,
            ctx,
            &fixture_dispatch_1,
        )
    })
    .analysis;
    let prop_names: Vec<&str> = meta.props.iter().map(|prop| prop.name.as_str()).collect();

    assert!(
        prop_names.contains(&"activationMode")
            && prop_names.contains(&"defaultValue")
            && prop_names.contains(&"modelValue")
            && prop_names.contains(&"unmountOnHide"),
        "package-backed Pick heritage should survive alongside local indexed-access helpers, got {prop_names:?}"
    );
    assert!(
        prop_names.contains(&"color") && prop_names.contains(&"ui"),
        "local indexed-access helper props should still be present, got {prop_names:?}"
    );

    let color = meta
        .props
        .iter()
        .find(|prop| prop.name == "color")
        .expect("color prop should exist");
    let color_ty = demand_published_type(
        project.host(),
        "/src/App.vue",
        color.publication.result().selected_source(),
        "color prop",
    );
    assert!(
        !matches!(
            color_ty,
            TypeExpr::Unknown { .. } | TypeExpr::IndexedAccess { .. }
        ),
        "component-config indexed access should not stay symbolic in component meta, got {color_ty:?}"
    );
    assert_union_string_literals(&color_ty, &["primary", "secondary"]);

    let ui = meta
        .props
        .iter()
        .find(|prop| prop.name == "ui")
        .expect("ui prop should exist");
    let ui_ty = demand_published_type(
        project.host(),
        "/src/App.vue",
        ui.publication.result().selected_source(),
        "ui prop",
    );
    let TypeExpr::Object(ui_shape) = &ui_ty else {
        panic!("component-config slots helper should materialize as an object, got {ui_ty:?}");
    };
    assert!(
        ui_shape.properties.iter().any(
            |member| matches!(member, ObjectMember::Property(property) if property.string_name().expect("string-key fixture") == "root"),
        ),
        "ui helper should keep root, got {ui_ty:?}"
    );
    assert!(
        ui_shape.properties.iter().any(
            |member| matches!(member, ObjectMember::Property(property) if property.string_name().expect("string-key fixture") == "list"),
        ),
        "ui helper should keep list, got {ui_ty:?}"
    );

    let event = meta
        .events
        .iter()
        .find(|event| event.name == "update:modelValue")
        .expect("update:modelValue event should exist");
    let event_payload_ty = demand_published_type(
        project.host(),
        "/src/App.vue",
        event.payload.present(),
        "update:modelValue payload",
    );
    let TypeExpr::Tuple { elements, .. } = &event_payload_ty else {
        panic!(
            "package-backed emits should materialize as a tuple payload, got {event_payload_ty:?}"
        );
    };
    assert_eq!(
        elements.len(),
        1,
        "model update should have a single payload"
    );
    match &elements[0].ty {
        TypeExpr::Union(members) => {
            assert!(
                members.contains(&TypeExpr::Primitive(PrimitiveName::String)),
                "event payload should include string, got {event_payload_ty:?}"
            );
            assert!(
                members.contains(&TypeExpr::Primitive(PrimitiveName::Number)),
                "event payload should include number, got {event_payload_ty:?}"
            );
        }
        other => panic!(
            "package-backed emits should instantiate the generic payload, got {:?}",
            other
        ),
    }
    assert_eq!(
        event.raw_signature.as_deref(),
        Some("[payload: string | number]"),
        "event display should not fall back to the uninstantiated type parameter"
    );
}

/// The GLOBAL-registration direction of the same authority: a static
/// `is="GlobalWidget"` names no import and no HTML tag, so it is a COMPONENT
/// target — exactly what the IDE template rewrite decides through the same
/// `is_html_tag` authority when it mints a `GlobalComponents` fallback const
/// for that name. Verter's project side owns no `GlobalComponents`
/// resolution, so the branch is UNRESOLVED (fail-closed) rather than an
/// intrinsic attribute surface fabricated for a component; the genuine
/// native tag in the same fixture still resolves as native.
///
/// Mutation recipe: classify a non-import static `is=` as a string literal
/// again and `GlobalWidget` reappears as `NativeTag`; classify every static
/// `is=` as a `typeof` reference and `div` stops resolving as native.
#[test]
fn static_is_global_component_name_is_a_component_target_not_a_native_tag() {
    let project = make_project();
    project
        .upsert_base(
            "/Global.vue",
            r#"<script setup lang="ts">
</script>
<template><component is="GlobalWidget" /></template>"#,
        )
        .unwrap();
    project
        .upsert_base(
            "/NativeToo.vue",
            r#"<script setup lang="ts">
</script>
<template><component is="div" /></template>"#,
        )
        .unwrap();

    let global_meta = get_meta(&project, "/Global.vue");
    let global_steps = root_chain_steps(&global_meta);
    assert!(
        !global_steps
            .iter()
            .any(|step| matches!(step, ResolvedRootStep::NativeTag { .. })),
        "a globally-registered is=\"GlobalWidget\" is never a native tag: {:?}",
        global_meta.fallthrough_surface
    );
    assert!(
        global_steps.iter().any(|step| matches!(
            step,
            ResolvedRootStep::Unresolved {
                reason: UnresolvedBranchReason::DynamicComponentIs,
                ..
            }
        )),
        "a component target this project cannot resolve fails closed as an \
         unresolved branch: {:?}",
        global_meta.fallthrough_surface
    );

    let native_meta = get_meta(&project, "/NativeToo.vue");
    assert!(
        root_chain_steps(&native_meta)
            .iter()
            .any(|step| matches!(step, ResolvedRootStep::NativeTag { tag } if tag == "div")),
        "a genuine native tag still resolves as native: {:?}",
        native_meta.fallthrough_surface
    );
}

#[test]
fn component_meta_budget_error_detects_symbolic_budget_exceeded() {
    let types = ExpandedComponentTypes {
        props: vec![
            verter_session_query::analysis::type_expand::ExpandedField::from_source_position(
                "label".to_string(),
                verter_type_expr::TopLevelOwnerId::ordinary_file(),
                verter_type_expr::facts::SourcePosition::Present(
                    verter_type_expr::facts::SemanticTypeSource::Closed(
                        verter_type_expr::facts::ClosedTypeFact::Leaf(
                            verter_type_expr::facts::LeafTypeFact::Primitive(PrimitiveName::String),
                        ),
                    ),
                ),
                None,
                false,
                verter_session_query::analysis::type_expand::ExpansionExactness::Incomplete,
                verter_session_query::analysis::type_expand::ExpansionExecutionStatus::Completed,
                vec![
                    verter_session_query::analysis::type_expand::ExpansionDiagnostic {
                        reason: verter_session_query::analysis::type_expand::ExpansionStopReason::BudgetExceeded,
                        context: "symbolic work limit reached".to_string(),
                        property_name: None,
                    },
                ],
                false,
                verter_type_expr::ResolutionProvenance::SemanticEvaluator,
            ),
        ],
        ..ExpandedComponentTypes::default()
    };

    assert!(
        component_meta_expansion_budget_exceeded(&types),
        "budget-exceeded diagnostics should force an explicit component-meta error"
    );
}

#[test]
fn symbolic_budget_is_not_fatal_when_component_surface_exists() {
    let analysis = verter_session_query::analysis::component_meta::ComponentMetaAnalysis {
        props: vec![
            verter_session_query::analysis::component_meta::PropAnalysis {
                name: "label".to_string(),
                callable_role: verter_type_expr::PropCallableRole::default(),
                publication: crate::test_only::type_publication_fixture(
                    verter_type_expr::facts::SourcePosition::Present(
                        verter_type_expr::facts::SemanticTypeSource::Closed(
                            verter_type_expr::facts::ClosedTypeFact::Leaf(
                                verter_type_expr::facts::LeafTypeFact::Primitive(
                                    PrimitiveName::String,
                                ),
                            ),
                        ),
                    ),
                    verter_type_expr::ResolutionExactness::ExactConcrete,
                    Some("string".to_string()),
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
        ],
        events: Vec::new(),
        slots: Vec::new(),
        models: Vec::new(),
        exposed: Vec::new(),
        public_instance: None,
        ordered_sfc_structure: None,
        type_registry: Vec::new(),
        components: Vec::new(),
        template_refs: Vec::new(),
        imports: Vec::new(),
        bindings: Vec::new(),
        vue_api_calls: Vec::new(),
        styles: Vec::new(),
        flags: verter_session_query::analysis::component_meta::ComponentMetaFlags::default(),
        root_reachability:
            verter_session_query::analysis::component_meta::RootReachability::NoFallthrough {
                reason:
                    verter_session_query::analysis::component_meta::NoFallthroughReason::NoTemplate,
            },
        accepted_props: Vec::new(),
        accepted_events: Vec::new(),
        accepted_surface_completeness:
            verter_session_query::analysis::component_meta::AcceptedSurfaceCompleteness::Exact,
        fallthrough_surface:
            verter_session_query::analysis::component_meta::FallthroughSurface::None {
                reason:
                    verter_session_query::analysis::component_meta::NoFallthroughReason::NoTemplate,
            },
        macro_expansion_diagnostics: Vec::new(),
        options_api: false,
        file_path: "/src/App.vue".to_string(),
    };

    assert!(!component_meta_symbolic_budget_is_fatal(Some(&analysis)));
    assert!(component_meta_symbolic_budget_is_fatal(None));
}

/// A local object shape (`interface Props { p0..pN: string }`) must
/// materialise its FULL `N`-member prop surface through the native
/// graph — the symbolic-carrier scoring and per-member shape reducer
/// must enumerate every declared member, not truncate to a partial.
///
/// Sized small (`60` props) instead of the historical `2400`-prop
/// corpus: the materialisation path is per-member, so a small object
/// exercises the identical full-enumeration invariant in a fraction
/// of the time. The `props.len() == prop_count` assertion is the
/// discriminating gate — if the per-member reducer dropped any
/// declared member the count falls short and the test goes RED.
#[test]
fn get_component_meta_retries_symbolic_budget_for_large_local_object_shapes() {
    let project = make_project();

    let prop_count = 60usize;
    let mut props_body = String::new();
    for index in 0..prop_count {
        props_body.push_str(&format!("  p{index}: string\n"));
    }

    project
        .upsert_base(
            "/src/App.vue",
            &format!(
                r#"<script setup lang="ts">
interface Props {{
{props_body}}}

defineProps<Props>()
</script>
<template><div /></template>"#
            ),
        )
        .unwrap();

    let session = project.open_session_batch().unwrap();
    let meta = session
        .get_component_meta("/src/App.vue")
        .unwrap()
        .expect("large local object shape should succeed after budget retry");

    assert_eq!(
        meta.props.len(),
        prop_count,
        "retry path should materialize the full local prop surface"
    );
    assert!(meta.props.iter().any(|prop| prop.name == "p0"));
    assert!(meta
        .props
        .iter()
        .any(|prop| prop.name == format!("p{}", prop_count - 1)));
}

/// ColorModeSelect regression: cross-file generic SelectMenuProps + Omit +
/// ButtonHTMLAttributes. Must complete without HardStop/timeout.
#[test]
fn get_component_meta_color_mode_select_completion_regression() {
    let project = make_project();
    project
        .upsert_base(
            "/types.ts",
            r#"export interface UseComponentIconsProps {
  loading?: boolean
  leadingIcon?: string
  trailingIcon?: string
}

export interface InputProps {
  modelValue?: string
  placeholder?: string
}

export type GetItemKeys<T> = T extends readonly (infer U)[]
  ? U extends Record<string, any> ? keyof U : never
  : T extends Record<string, any> ? keyof T : never

export interface SelectMenuItem {
  label?: string
  value?: string | number
  icon?: string
  disabled?: boolean
}

export interface SelectMenuProps<
  T extends SelectMenuItem | SelectMenuItem[] = SelectMenuItem[],
  VK extends GetItemKeys<T> | undefined = undefined,
  M extends boolean = false
> extends UseComponentIconsProps, Omit<ButtonHTMLAttributes, 'name'> {
  open?: boolean
  disabled?: boolean
  name?: string
  searchInput?: boolean | Omit<InputProps, 'modelValue'>
  valueKey?: VK
  items?: T
  modelValue?: M extends true ? T : SelectMenuItem
}

interface ButtonHTMLAttributes {
  autofocus?: boolean
  disabled?: boolean
  form?: string
  formaction?: string
  formenctype?: string
  formmethod?: string
  formnovalidate?: boolean
  formtarget?: string
  name?: string
  type?: 'submit' | 'reset' | 'button'
  value?: string
}"#,
        )
        .unwrap();
    project
        .upsert_base(
            "/ColorModeSelect.vue",
            r#"<script lang="ts">
import type { SelectMenuProps, SelectMenuItem } from './types'

export interface ColorModeSelectProps extends Omit<SelectMenuProps<SelectMenuItem[]>, 'icon' | 'items' | 'modelValue'> {
}
</script>

<script setup lang="ts">
defineProps<ColorModeSelectProps>()
</script>
<template><div /></template>"#,
        )
        .unwrap();

    let meta = get_meta(&project, "/ColorModeSelect.vue");
    let prop_names: Vec<&str> = meta.props.iter().map(|prop| prop.name.as_str()).collect();

    // Core props from SelectMenuProps should survive through Omit + extends.
    assert!(
        prop_names.contains(&"open")
            && prop_names.contains(&"disabled")
            && prop_names.contains(&"name"),
        "ColorModeSelect must keep direct generic survivors, got: {prop_names:?}"
    );
    assert!(
        prop_names.contains(&"loading"),
        "ColorModeSelect must keep inherited UseComponentIconsProps members, got: {prop_names:?}"
    );
    // Omitted props should NOT appear.
    assert!(
        !prop_names.contains(&"icon")
            && !prop_names.contains(&"items")
            && !prop_names.contains(&"modelValue"),
        "ColorModeSelect must respect wrapper Omit, got: {prop_names:?}"
    );
    // ButtonHTMLAttributes survivors (after Omit<..., 'name'>).
    assert!(
        prop_names.contains(&"formaction") && prop_names.contains(&"formtarget"),
        "ColorModeSelect must keep ButtonHTMLAttributes heritage, got: {prop_names:?}"
    );
}

#[test]
fn get_component_meta_toolbar_items_do_not_flatten_nested_button_helpers() {
    let project = make_project();
    project
        .upsert_base(
            "/types.ts",
            r#"export interface LinkProps {
  href?: string
  target?: string
  rel?: string
}

export type LinkPropsKeys = 'href' | 'target' | 'rel'

export interface ButtonProps extends Omit<LinkProps, 'href'> {
  icon?: string
  avatar?: string
  color?: 'primary' | 'neutral'
  variant?: 'solid' | 'ghost'
}"#,
        )
        .unwrap();
    project
        .upsert_base(
            "/Toolbar.vue",
            r#"<script lang="ts">
import type { ButtonProps, LinkPropsKeys } from './types'

type ButtonItem = Omit<ButtonProps, LinkPropsKeys | 'color' | 'variant'> & {
  slot?: string
}

type ToolbarItem = ButtonItem | {
  label?: string
}

export interface ToolbarProps {
  color?: ButtonProps['color']
  items?: ToolbarItem[]
}
</script>

<script setup lang="ts">
defineProps<ToolbarProps>()
</script>
<template><div /></template>"#,
        )
        .unwrap();

    let meta = get_meta(&project, "/Toolbar.vue");
    let prop_names: Vec<&str> = meta.props.iter().map(|prop| prop.name.as_str()).collect();

    assert!(
        prop_names.contains(&"color") && prop_names.contains(&"items"),
        "Toolbar wrapper should keep its declared top-level props, got: {prop_names:?}"
    );
    assert!(
        !prop_names.contains(&"icon")
            && !prop_names.contains(&"avatar")
            && !prop_names.contains(&"href")
            && !prop_names.contains(&"target")
            && !prop_names.contains(&"rel")
            && !prop_names.contains(&"slot")
            && !prop_names.contains(&"variant"),
        "nested toolbar item helpers must stay nested instead of leaking to top level: {prop_names:?}"
    );
}

/// The ALL-13-LANES sentinel fixture: one component populating every wire
/// type lane — props (distinct middle element), DUPLICATE event names,
/// nested slot bindings (repeated binding name across slots), models,
/// exposed, public-instance members, merged type-registry rows, accepted
/// props/events (declared + inherited), and MULTIPLE fallthrough branches —
/// resolved through the audited output entry and asserted POSITIONALLY.
///
/// Discrimination: a name-keyed (map) lane loses the duplicate `dup` event
/// row and collapses the repeated `row` binding across slots; an internal
/// positional swap moves the DISTINCT middle prop sentinel (`middle:
/// number`) onto a neighbour. Both fail the exact positional assertions.
#[test]
fn component_meta_output_materializes_all_thirteen_lanes_positionally() {
    let project = make_project();
    project
        .upsert_base(
            "/ChildA.vue",
            r#"<script setup lang="ts">
defineProps<{ inheritedA: string }>()
defineEmits<{ childEventA: [flag: boolean] }>()
</script>
<template><div /></template>"#,
        )
        .unwrap();
    project
        .upsert_base(
            "/ChildB.vue",
            r#"<script setup lang="ts">
defineProps<{ inheritedB: number }>()
defineEmits<{ childEventB: [tag: string] }>()
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
type Named = { x: number }
defineProps<{ first: string; middle: number; last: boolean; named: Named }>()
defineEmits<{ first: [id: number]; dup: [a: string]; dup: [b: boolean] }>()
defineSlots<{ default(props: { row: string; other: number }): number; second(props: { row: boolean }): string }>()
const modelSentinel = defineModel<boolean>('modelSentinel')
const exposedVal: string = 'x'
defineExpose({ exposedVal })
const cond = true
</script>
<template>
  <ChildA v-if="cond" />
  <ChildB v-else />
</template>"#,
        )
        .unwrap();

    let (output, _request_id) = {
        let (output, request_id) = project
            .host()
            .get_component_meta_output_with_resolution("/App.vue")
            .expect("all-lanes output materialization must succeed");
        (output.expect("component must resolve"), request_id)
    };
    let (analysis, resolution, types) = output.into_parts();
    let lanes = types.into_lanes();

    // ── Lane 1: props (positional; DISTINCT middle sentinel). ──
    assert_eq!(analysis.props.len(), lanes.props.len(), "props lane 1:1");
    let prop_names: Vec<&str> = analysis.props.iter().map(|p| p.name.as_str()).collect();
    let first_idx = prop_names.iter().position(|n| *n == "first").unwrap();
    let middle_idx = prop_names.iter().position(|n| *n == "middle").unwrap();
    let last_idx = prop_names.iter().position(|n| *n == "last").unwrap();
    assert_eq!(
        published_type(&lanes.props[first_idx]),
        &TypeExpr::Primitive(PrimitiveName::String),
        "props[first] materializes its own sentinel type"
    );
    assert_eq!(
        published_type(&lanes.props[middle_idx]),
        &TypeExpr::Primitive(PrimitiveName::Number),
        "the DISTINCT middle element keeps its own positional value (a swap moves it)"
    );
    assert_eq!(
        published_type(&lanes.props[last_idx]),
        &TypeExpr::Primitive(PrimitiveName::Boolean)
    );

    // ── Lane 2: event payloads (DUPLICATE names preserved positionally). ──
    assert_eq!(
        analysis.events.len(),
        materialized_event_types(&lanes).len()
    );
    let event_names: Vec<&str> = analysis.events.iter().map(|e| e.name.as_str()).collect();
    let dup_positions: Vec<usize> = event_names
        .iter()
        .enumerate()
        .filter_map(|(i, n)| (*n == "dup").then_some(i))
        .collect();
    assert_eq!(
        dup_positions.len(),
        2,
        "duplicate event names are positional rows, never name-collapsed; got {event_names:?}"
    );
    // Both authored `dup` overloads survive positionally with their distinct
    // payload sources.
    assert_eq!(
        materialized_event_types(&lanes)[dup_positions[0]],
        labeled_tuple(&[("a", TypeExpr::Primitive(PrimitiveName::String))]),
    );
    assert_eq!(
        materialized_event_types(&lanes)[dup_positions[1]],
        labeled_tuple(&[("b", TypeExpr::Primitive(PrimitiveName::Boolean))]),
        "the second duplicate row transports its own authored payload"
    );
    // The DISTINCT `first` event keeps its own payload — an internal
    // positional swap moves it onto a `dup` row.
    let first_event = event_names.iter().position(|n| *n == "first").unwrap();
    assert_eq!(
        materialized_event_types(&lanes)[first_event],
        labeled_tuple(&[("id", TypeExpr::Primitive(PrimitiveName::Number))]),
        "the distinct event keeps its own positional payload"
    );
    assert_eq!(
        analysis.events.len(),
        event_occurrence_publications(&lanes).len(),
        "A1 event publications are positionally aligned"
    );
    assert_eq!(
        published_type(&event_occurrence_publications(&lanes)[dup_positions[0]]),
        &labeled_tuple(&[("a", TypeExpr::Primitive(PrimitiveName::String))])
    );
    assert_eq!(
        published_type(&event_occurrence_publications(&lanes)[dup_positions[1]]),
        &labeled_tuple(&[("b", TypeExpr::Primitive(PrimitiveName::Boolean))])
    );

    // ── Lanes 4-5: slot bindings and typed returns. ──
    assert_eq!(analysis.slots.len(), lanes.slot_bindings.len());
    assert_eq!(analysis.slots.len(), lanes.slot_returns.len());
    for (slot, binding_lane) in analysis.slots.iter().zip(lanes.slot_bindings.iter()) {
        assert_eq!(
            slot.bindings.len(),
            binding_lane.len(),
            "slot `{}` bindings inner-align 1:1",
            slot.name
        );
    }
    let default_idx = analysis
        .slots
        .iter()
        .position(|s| s.name == "default")
        .expect("default slot");
    let second_idx = analysis
        .slots
        .iter()
        .position(|s| s.name == "second")
        .expect("second slot");
    let default_row = analysis.slots[default_idx]
        .bindings
        .iter()
        .position(|b| b.name == "row")
        .expect("default.row binding");
    let second_row = analysis.slots[second_idx]
        .bindings
        .iter()
        .position(|b| b.name == "row")
        .expect("second.row binding");
    // Binding rows publish their complete closed leaf facts: the repeated
    // `row` name under two slots keeps each slot's OWN declared type —
    // `default(props: { row: string })` vs `second(props: { row: boolean })`
    // — so a cross-slot collapse of the repeated name or a positional swap
    // moves `string` onto the boolean row and fails.
    assert_eq!(
        lanes.slot_bindings[default_idx][default_row]
            .materialized_type()
            .expect("published type"),
        &TypeExpr::Primitive(PrimitiveName::String),
        "default.row publishes its own slot's declared `string` leaf fact"
    );
    assert_eq!(
        lanes.slot_bindings[second_idx][second_row]
            .materialized_type()
            .expect("published type"),
        &TypeExpr::Primitive(PrimitiveName::Boolean),
        "second.row keeps ITS OWN slot's declared `boolean` leaf fact — the \
         repeated name never collapses across slots"
    );
    assert_eq!(
        published_type(
            lanes.slot_returns[default_idx]
                .as_ref()
                .expect("default slot has a typed return")
        ),
        &TypeExpr::Primitive(PrimitiveName::Number)
    );
    assert_eq!(
        published_type(
            lanes.slot_returns[second_idx]
                .as_ref()
                .expect("second slot has a typed return")
        ),
        &TypeExpr::Primitive(PrimitiveName::String)
    );

    // ── Lane 4: models — exact sentinel VALUE (a deleted model
    // materializer empties the lane; an Unknown-returning one fails the
    // exact equality). ──
    assert_eq!(analysis.models.len(), lanes.models.len());
    let model_idx = analysis
        .models
        .iter()
        .position(|m| m.name == "modelSentinel")
        .expect("fixture premise: defineModel populates the models lane");
    assert_eq!(
        lanes.models[model_idx],
        TypeExpr::Primitive(PrimitiveName::Boolean),
        "the model lane materializes the defineModel type argument exactly"
    );

    // ── Lane 5: exposed — exact sentinel VALUE. ──
    assert_eq!(analysis.exposed.len(), lanes.exposed.len());
    let exposed_idx = analysis
        .exposed
        .iter()
        .position(|x| x.name == "exposedVal")
        .expect("fixture premise: defineExpose populates the exposed lane");
    assert_eq!(
        lanes.exposed[exposed_idx],
        TypeExpr::Primitive(PrimitiveName::String),
        "the exposed lane materializes the exposed binding's type exactly"
    );

    // ── Lane 6: public-instance members — exact sentinel VALUES (the
    // exposed member AND the distinct middle prop, so a cross-lane or
    // positional swap moves a wrong value here). ──
    let public_instance = analysis
        .public_instance
        .as_ref()
        .expect("fixture premise: props+slots+exposed populate the public-instance sidecar");
    assert_eq!(
        public_instance.members.len(),
        lanes.public_instance_members.len()
    );
    let pi_exposed = public_instance
        .members
        .iter()
        .position(|m| m.name == "exposedVal")
        .expect("public-instance carries the exposed member");
    assert_eq!(
        lanes.public_instance_members[pi_exposed],
        TypeExpr::Primitive(PrimitiveName::String),
    );
    let pi_middle = public_instance
        .members
        .iter()
        .position(|m| m.name == "middle")
        .expect("public-instance carries the middle prop member");
    assert_eq!(
        lanes.public_instance_members[pi_middle],
        TypeExpr::Primitive(PrimitiveName::Number),
        "the DISTINCT middle member keeps its own positional value"
    );

    // ── Lane 7: merged type-registry rows (overlay finalize is session-owned). ──
    assert!(
        resolution.is_some(),
        "the audited output entry carries the narrowed resolution sidecar"
    );
    assert_eq!(
        analysis.type_registry.len(),
        lanes.type_registry_entries.len()
    );
    let named_idx = analysis
        .type_registry
        .iter()
        .position(|e| e.name == "Named")
        .unwrap_or_else(|| {
            panic!(
                "fixture premise: the Named macro type reference enters the registry; got {:?}",
                analysis
                    .type_registry
                    .iter()
                    .map(|e| e.name.as_str())
                    .collect::<Vec<_>>()
            )
        });
    match &lanes.type_registry_entries[named_idx] {
        TypeExpr::Object(object) => {
            assert_eq!(object.properties.len(), 1, "Named = {{ x: number }}");
            match &object.properties[0] {
                verter_type_expr::ObjectMember::Property(prop) => {
                    assert_eq!(prop.string_name().expect("string-key fixture"), "x");
                    assert_eq!(
                        prop.ty,
                        TypeExpr::Primitive(PrimitiveName::Number),
                        "the registry lane materializes the declaration body exactly"
                    );
                }
                other => panic!("Named.x is a property; got {other:?}"),
            }
        }
        other => panic!(
            "the registry lane materializes Named's object body (never Unknown); got {other:?}"
        ),
    }

    // ── Lanes 8-9: accepted props/events (declared + inherited). ──
    assert_eq!(analysis.accepted_props.len(), lanes.accepted_props.len());
    assert_eq!(
        analysis.accepted_events.len(),
        lanes.accepted_event_payloads.len()
    );
    let accepted_names: Vec<&str> = analysis
        .accepted_props
        .iter()
        .map(|p| p.name.as_str())
        .collect();
    assert!(
        accepted_names.contains(&"first") && accepted_names.contains(&"inheritedA"),
        "accepted props span declared AND inherited rows; got {accepted_names:?}"
    );
    let inherited_a = accepted_names
        .iter()
        .position(|n| *n == "inheritedA")
        .unwrap();
    assert_eq!(
        published_type(&lanes.accepted_props[inherited_a]),
        &TypeExpr::Primitive(PrimitiveName::String),
        "the INHERITED accepted prop materializes the child's own type under the child scope"
    );
    let declared_first = accepted_names.iter().position(|n| *n == "first").unwrap();
    assert_eq!(
        published_type(&lanes.accepted_props[declared_first]),
        &TypeExpr::Primitive(PrimitiveName::String),
        "the DECLARED accepted prop materializes its own type exactly"
    );
    // Accepted-event payloads: BOTH children's inherited events carry their
    // exact payload tuples (an Unknown-returning or deleted materializer
    // fails these; a cross-child swap moves flag/tag onto the wrong row).
    let accepted_event_names: Vec<&str> = analysis
        .accepted_events
        .iter()
        .map(|e| e.name.as_str())
        .collect();
    let child_a_event = accepted_event_names
        .iter()
        .position(|n| *n == "childEventA")
        .expect("accepted events span the inherited child events");
    assert_eq!(
        lanes.accepted_event_payloads[child_a_event],
        labeled_tuple(&[("flag", TypeExpr::Primitive(PrimitiveName::Boolean))]),
        "childEventA's accepted payload materializes ChildA's exact tuple"
    );
    let child_b_event = accepted_event_names
        .iter()
        .position(|n| *n == "childEventB")
        .expect("accepted events span BOTH children's inherited events");
    assert_eq!(
        lanes.accepted_event_payloads[child_b_event],
        labeled_tuple(&[("tag", TypeExpr::Primitive(PrimitiveName::String))]),
        "childEventB's accepted payload materializes ChildB's exact tuple (never ChildA's)"
    );

    // ── Lanes 10-11: MULTIPLE fallthrough branches, per-branch rows. ──
    let verter_session_query::analysis::component_meta::FallthroughSurface::Branches { branches } =
        &analysis.fallthrough_surface
    else {
        panic!("fixture premise: v-if/v-else roots produce fallthrough branches");
    };
    assert_eq!(branches.len(), 2, "two conditional root branches");
    assert_eq!(branches.len(), lanes.fallthrough_props.len());
    assert_eq!(branches.len(), lanes.fallthrough_event_payloads.len());
    for (branch, (prop_lane, event_lane)) in branches.iter().zip(
        lanes
            .fallthrough_props
            .iter()
            .zip(lanes.fallthrough_event_payloads.iter()),
    ) {
        assert_eq!(branch.props.len(), prop_lane.len());
        assert_eq!(branch.events.len(), event_lane.len());
    }
    // Branch 0 inherits from ChildA (string), branch 1 from ChildB (number)
    // — a branch swap or a parent-scoped mis-raise moves/loses these.
    let a_branch = branches
        .iter()
        .position(|b| b.props.iter().any(|p| p.name == "inheritedA"))
        .expect("a branch inherits ChildA's prop");
    let b_branch = branches
        .iter()
        .position(|b| b.props.iter().any(|p| p.name == "inheritedB"))
        .expect("a branch inherits ChildB's prop");
    assert_ne!(a_branch, b_branch, "each child contributes its OWN branch");
    let a_row = branches[a_branch]
        .props
        .iter()
        .position(|p| p.name == "inheritedA")
        .unwrap();
    assert_eq!(
        published_type(&lanes.fallthrough_props[a_branch][a_row]),
        &TypeExpr::Primitive(PrimitiveName::String),
    );
    let b_row = branches[b_branch]
        .props
        .iter()
        .position(|p| p.name == "inheritedB")
        .unwrap();
    assert_eq!(
        published_type(&lanes.fallthrough_props[b_branch][b_row]),
        &TypeExpr::Primitive(PrimitiveName::Number),
        "branch B keeps ITS child's sentinel type (never ChildA's)"
    );
    // Per-branch EVENT payload rows: each branch carries ITS child's exact
    // event tuple (a deleted fallthrough-event materializer empties the
    // lane; an Unknown-returning one or a branch swap fails the equality).
    let a_event_row = branches[a_branch]
        .events
        .iter()
        .position(|e| e.name == "childEventA")
        .expect("branch A inherits ChildA's event");
    assert_eq!(
        lanes.fallthrough_event_payloads[a_branch][a_event_row],
        labeled_tuple(&[("flag", TypeExpr::Primitive(PrimitiveName::Boolean))]),
        "branch A's event payload materializes ChildA's exact tuple"
    );
    let b_event_row = branches[b_branch]
        .events
        .iter()
        .position(|e| e.name == "childEventB")
        .expect("branch B inherits ChildB's event");
    assert_eq!(
        lanes.fallthrough_event_payloads[b_branch][b_event_row],
        labeled_tuple(&[("tag", TypeExpr::Primitive(PrimitiveName::String))]),
        "branch B's event payload materializes ChildB's exact tuple (never ChildA's)"
    );
}

/// `None` sources on EVERY optional-source lane follow the ONE centralized
/// missing-source policy — the canonical typed `TypeExpr::Unknown` with an
/// empty raw. Text without a locator is not promoted into authored evidence.
/// (The registry lane is structurally excluded: its source is non-optional.)
#[test]
fn component_meta_output_missing_sources_follow_central_policy_on_every_lane() {
    use verter_session_query::analysis::component_meta as cm;
    let project = make_project();
    project
        .upsert_base("/App.vue", "<template><div /></template>")
        .unwrap();
    let host = project.host();

    let mut analysis = blank_output_analysis();
    analysis.props.push(cm::PropAnalysis {
        name: "p".to_string(),
        callable_role: verter_type_expr::PropCallableRole::default(),
        publication: crate::test_only::type_publication_fixture(
            verter_type_expr::facts::SourcePosition::unannotated(),
            verter_type_expr::ResolutionExactness::ExactConcrete,
            Some("RawPropText".to_string()),
            None,
        ),
        type_expansion: None,
        required: true,
        has_default: false,
        default_value: None,
        description: None,
        tags: Vec::new(),
        declared_in_macro_type_arg: false,
    });
    analysis.events.push(cm::EventAnalysis {
        name: "e".to_string(),
        payload: verter_type_expr::facts::SourcePosition::unannotated(),
        publication: crate::test_only::type_publication_fixture(
            verter_type_expr::facts::SourcePosition::unannotated(),
            verter_type_expr::ResolutionExactness::ExactConcrete,
            None,
            None,
        ),
        return_publication: None,
        return_publication_scope: None,
        payload_expansion: None,
        raw_signature: Some("(e: 'e') => void".to_string()),
        description: None,
        tags: Vec::new(),
    });
    analysis.slots.push(cm::SlotAnalysis {
        name: "s".to_string(),
        is_scoped: true,
        bindings: vec![cm::SlotBindingAnalysis {
            name: "b".to_string(),
            publication: crate::test_only::type_publication_fixture(
                verter_type_expr::facts::SourcePosition::unannotated(),
                verter_type_expr::ResolutionExactness::ExactConcrete,
                Some("RawBindingText".to_string()),
                None,
            ),
            type_expansion: None,
        }],
        is_required: false,
        return_type: None,
        return_publication: None,
        return_publication_scope: None,
        description: None,
        tags: Vec::new(),
        declared_in_macro_type_arg: true,
    });
    analysis.models.push(cm::ModelAnalysis {
        name: "m".to_string(),
        type_source: verter_type_expr::facts::SourcePosition::unannotated(),
    });
    analysis.exposed.push(cm::ExposedAnalysis {
        name: "x".to_string(),
        type_source: verter_type_expr::facts::SourcePosition::unannotated(),
        type_expansion: None,
        description: None,
        tags: Vec::new(),
    });
    analysis.public_instance = Some(cm::PublicInstanceAnalysis {
        members: vec![cm::PublicInstanceMemberAnalysis {
            name: "pi".to_string(),
            kind: cm::PublicInstanceMemberKind::Prop,
            type_source: verter_type_expr::facts::SourcePosition::unannotated(),
            type_expansion: None,
            raw_type: Some("RawMemberText".to_string()),
            description: None,
            tags: Vec::new(),
        }],
        completeness: cm::PublicInstanceCompleteness::Exact,
    });
    analysis.accepted_props.push(cm::AcceptedPropAnalysis {
        name: "ap".to_string(),
        callable_role: verter_type_expr::PropCallableRole::default(),
        publication: crate::test_only::type_publication_fixture(
            verter_type_expr::facts::SourcePosition::unannotated(),
            verter_type_expr::ResolutionExactness::ExactConcrete,
            Some("RawAcceptedText".to_string()),
            None,
        ),
        type_source_scope: None,
        required: false,
        provenance: cm::MemberProvenance::Declared,
        availability: cm::MemberAvailability::Always,
        kind: cm::AcceptedPropKind::DeclaredProp,
    });
    analysis.accepted_events.push(cm::AcceptedEventAnalysis {
        name: "ae".to_string(),
        payload: verter_type_expr::facts::SourcePosition::unannotated(),
        payload_scope: None,
        raw_signature: Some("(e: 'ae') => void".to_string()),
        provenance: cm::MemberProvenance::Declared,
        availability: cm::MemberAvailability::Always,
        kind: cm::AcceptedEventKind::DeclaredEmit,
    });
    analysis.fallthrough_surface = cm::FallthroughSurface::Branches {
        branches: vec![cm::FallthroughBranch {
            branch_key: "0".to_string(),
            condition_text: None,
            props: vec![cm::FallthroughPropEntry {
                name: "fp".to_string(),
                callable_role: verter_type_expr::PropCallableRole::default(),
                publication: crate::test_only::type_publication_fixture(
                    verter_type_expr::facts::SourcePosition::unannotated(),
                    verter_type_expr::ResolutionExactness::ExactConcrete,
                    Some("RawFallthroughText".to_string()),
                    None,
                ),
                type_source_scope: None,
                sources: vec![cm::InheritedSource::NativeTag {
                    tag: "div".to_string(),
                }],
            }],
            events: vec![cm::FallthroughEventEntry {
                name: "fe".to_string(),
                payload: verter_type_expr::facts::SourcePosition::unannotated(),
                payload_scope: None,
                raw_signature: None,
                sources: vec![cm::InheritedSource::NativeTag {
                    tag: "div".to_string(),
                }],
            }],
            root_chain: Vec::new(),
            status: cm::BranchStatus::Resolved,
        }],
    };

    let fixture_dispatch_13 =
        verter_type_engine::project_semantic_dispatch::ProjectSemanticDispatch::new(host);
    let output = crate::meta_resolve::projectors::build_component_meta_output(
        host,
        &fixture_dispatch_13,
        "/App.vue",
        analysis,
        None,
        crate::meta_resolve::PublishedCompleteness::COMPLETE,
    )
    .expect("None sources never fail materialization");
    let (analysis, _resolution, types) = output.into_parts();
    let lanes = types.into_lanes();

    let unknown = TypeExpr::Unknown(UnknownValue::missing_output());
    assert_eq!(
        published_type(&lanes.props[0]),
        &unknown,
        "props None-policy golden"
    );
    assert_eq!(materialized_event_types(&lanes), vec![unknown.clone()]);
    assert_eq!(published_type(&lanes.slot_bindings[0][0]), &unknown);
    assert_eq!(lanes.models, vec![unknown.clone()]);
    assert_eq!(lanes.exposed, vec![unknown.clone()]);
    assert_eq!(lanes.public_instance_members, vec![unknown.clone()]);
    assert_eq!(published_type(&lanes.accepted_props[0]), &unknown);
    assert_eq!(lanes.accepted_event_payloads, vec![unknown.clone()]);
    assert_eq!(published_type(&lanes.fallthrough_props[0][0]), &unknown);
    assert_eq!(lanes.fallthrough_event_payloads, vec![vec![unknown]]);

    assert!(
        analysis.props[0].publication.evidence().is_none()
            && analysis.slots[0].bindings[0]
                .publication
                .evidence()
                .is_none()
            && analysis.accepted_props[0].publication.evidence().is_none(),
        "display text without a locator must not mint authored evidence"
    );
}

/// A present-but-UNRAISABLE source on a NESTED lane fails the whole output
/// with the typed error carrying the lane + BOTH positional indices — never
/// a silent `Unknown`.
#[test]
fn component_meta_output_unraisable_nested_sources_fail_typed_with_inner_index() {
    let project = make_project();
    project
        .upsert_base("/App.vue", "<template><div /></template>")
        .unwrap();
    let host = project.host();
    let bad_source = authored_decl_body_source("/definitely-missing.ts", "NoSuchType");

    // Nested lane 1: slot bindings (outer = slot index, inner = binding row).
    let mut analysis = blank_output_analysis();
    analysis.slots.push(
        verter_session_query::analysis::component_meta::SlotAnalysis {
            name: "s".to_string(),
            is_scoped: true,
            bindings: vec![
                verter_session_query::analysis::component_meta::SlotBindingAnalysis {
                    name: "ok".to_string(),
                    publication: crate::test_only::type_publication_fixture(
                        verter_type_expr::facts::SourcePosition::unannotated(),
                        verter_type_expr::ResolutionExactness::ExactConcrete,
                        None,
                        None,
                    ),
                    type_expansion: None,
                },
                verter_session_query::analysis::component_meta::SlotBindingAnalysis {
                    name: "bad".to_string(),
                    publication: crate::test_only::type_publication_fixture(
                        verter_type_expr::facts::SourcePosition::Present(bad_source.clone()),
                        verter_type_expr::ResolutionExactness::ExactConcrete,
                        None,
                        None,
                    ),
                    type_expansion: None,
                },
            ],
            is_required: false,
            return_type: None,
            return_publication: None,
            return_publication_scope: None,
            description: None,
            tags: Vec::new(),
            declared_in_macro_type_arg: true,
        },
    );

    let fixture_dispatch_16 =
        verter_type_engine::project_semantic_dispatch::ProjectSemanticDispatch::new(host);
    let err = crate::meta_resolve::projectors::build_component_meta_output(
        host,
        &fixture_dispatch_16,
        "/App.vue",
        analysis,
        None,
        crate::meta_resolve::PublishedCompleteness::COMPLETE,
    )
    .expect_err("an unraisable present source must FAIL the output");
    assert_eq!(
        err.lane,
        crate::meta_resolve::ComponentMetaOutputLane::SlotBinding
    );
    assert_eq!(err.index, 0, "outer index = slot position");
    assert_eq!(err.inner_index, Some(1), "inner index = binding row");
    assert_eq!(
        *err.position,
        verter_type_expr::facts::SourcePosition::Present(bad_source.clone())
    );
    assert_eq!(
        err.failure,
        crate::meta_resolve::ComponentMetaOutputFailure::UnraisableSource
    );

    // Nested lane 2: fallthrough props (outer = branch, inner = row).
    let mut analysis = blank_output_analysis();
    analysis.fallthrough_surface =
        verter_session_query::analysis::component_meta::FallthroughSurface::Branches {
            branches: vec![
                verter_session_query::analysis::component_meta::FallthroughBranch {
                    branch_key: "0".to_string(),
                    condition_text: None,
                    props: Vec::new(),
                    events: Vec::new(),
                    root_chain: Vec::new(),
                    status: verter_session_query::analysis::component_meta::BranchStatus::Resolved,
                },
                verter_session_query::analysis::component_meta::FallthroughBranch {
                    branch_key: "1".to_string(),
                    condition_text: None,
                    props: vec![
                        verter_session_query::analysis::component_meta::FallthroughPropEntry {
                            name: "bad".to_string(),
                            callable_role: verter_type_expr::PropCallableRole::default(),
                            publication: crate::test_only::type_publication_fixture(
                                verter_type_expr::facts::SourcePosition::Present(
                                    bad_source.clone(),
                                ),
                                verter_type_expr::ResolutionExactness::ExactConcrete,
                                None,
                                None,
                            ),
                            type_source_scope: None,
                            sources: Vec::new(),
                        },
                    ],
                    events: Vec::new(),
                    root_chain: Vec::new(),
                    status: verter_session_query::analysis::component_meta::BranchStatus::Resolved,
                },
            ],
        };
    let err = crate::meta_resolve::projectors::build_component_meta_output(
        host,
        &fixture_dispatch_16,
        "/App.vue",
        analysis,
        None,
        crate::meta_resolve::PublishedCompleteness::COMPLETE,
    )
    .expect_err("an unraisable fallthrough source must FAIL the output");
    assert_eq!(
        err.lane,
        crate::meta_resolve::ComponentMetaOutputLane::FallthroughProp
    );
    assert_eq!(
        err.index, 1,
        "outer index = the FAILING branch, not branch 0"
    );
    assert_eq!(err.inner_index, Some(0));
}

/// FIX-3 regression: a failed INTERIOR required locator inside a
/// successfully-composed root FAILS the output with the typed
/// `InteriorSourceMiss` (carrying the nested position path) — never an
/// `Ok` whose lane silently renders the interior miss as `Unknown`.
/// Covers all four composite source families: object member, function
/// parameter, tuple element, index-signature value.
#[test]
fn component_meta_output_failed_interior_locator_fails_closed_per_source_family() {
    use verter_type_expr::facts as tf;
    use verter_type_expr::span_origins::SourceSynthetic;
    let project = make_project();
    project
        .upsert_base("/App.vue", "<template><div /></template>")
        .unwrap();
    let host = project.host();
    let bad = missing_interior_slot();

    // (1) Closed OBJECT with a member whose value slot cannot deref.
    let object_source =
        tf::SemanticTypeSource::Closed(tf::ClosedTypeFact::Object(tf::ObjectShapeFact {
            members: Arc::from(
                vec![tf::ObjectMemberFact::Property(tf::ObjectPropertyFact {
                    key: "member".into(),
                    optional: false,
                    readonly: false,
                    visibility: verter_type_expr::MemberVisibility::Public,
                    ty: bad.clone(),
                    span_origin: verter_type_expr::span_origins::MemberSpansOrigin::Synthetic(
                        SourceSynthetic,
                    ),
                })]
                .into_boxed_slice(),
            ),
        }));
    let err = build_output_with_prop_source(host, object_source)
        .expect_err("a failed interior OBJECT-member locator must fail the output");
    assert_eq!(err.lane, crate::meta_resolve::ComponentMetaOutputLane::Prop);
    match &err.failure {
        crate::meta_resolve::ComponentMetaOutputFailure::InteriorSourceMiss { path } => {
            assert_eq!(
                path.as_ref(),
                &[
                    verter_type_engine::project_semantic_dispatch::interior_source::InteriorSourceStep::Member(
                        "member".into(),
                    )
                ],
                "the typed failure names the exact nested member position"
            );
        }
        other => panic!("expected InteriorSourceMiss with the member path; got {other:?}"),
    }

    // (2) Closed FUNCTION with a PRESENT (annotated) parameter slot that
    // cannot deref.
    let function_source =
        tf::SemanticTypeSource::Closed(tf::ClosedTypeFact::Function(tf::FunctionSignatureFact {
            type_parameters: Arc::from(Vec::new().into_boxed_slice()),
            parameters: Arc::from(
                vec![tf::FunctionParamFact {
                    name: Some("arg".to_string()),
                    optional: false,
                    rest: false,
                    has_ts_annotation: true,
                    ty: Some(bad.clone()),
                    span_origin: verter_type_expr::span_origins::FunctionParamSpanOrigin {
                        function: verter_type_expr::span_origins::FunctionSpansOrigin::Synthetic(
                            SourceSynthetic,
                        ),
                        param: verter_type_expr::span_origins::FunctionParamSelector::Positional {
                            ordinal: 0,
                        },
                    },
                }]
                .into_boxed_slice(),
            ),
            return_source: tf::FunctionReturnSource::Absent,
            return_reference_head: tf::AuthoredReferenceHeadFact::Unavailable,
            has_implementation_body: false,
            spans_origin: verter_type_expr::span_origins::FunctionSpansOrigin::Synthetic(
                SourceSynthetic,
            ),
        }));
    let err = build_output_with_prop_source(host, function_source)
        .expect_err("a failed interior function-PARAMETER locator must fail the output");
    match &err.failure {
        crate::meta_resolve::ComponentMetaOutputFailure::InteriorSourceMiss { path } => {
            assert_eq!(
                path.as_ref(),
                &[verter_type_engine::project_semantic_dispatch::interior_source::InteriorSourceStep::Parameter { ordinal: 0 }],
            );
        }
        other => panic!("expected InteriorSourceMiss with the parameter path; got {other:?}"),
    }

    // (3) Closed TUPLE with an element locator that cannot deref.
    let tuple_source =
        tf::SemanticTypeSource::Closed(tf::ClosedTypeFact::Tuple(tf::TuplePayloadFact {
            readonly: false,
            elements: Arc::from(
                vec![tf::TupleElementFact {
                    label: Some("payload".to_string()),
                    optional: false,
                    rest: false,
                    ty: tf::FactOrLocator::Locator(bad.clone()),
                }]
                .into_boxed_slice(),
            ),
        }));
    let err = build_output_with_prop_source(host, tuple_source)
        .expect_err("a failed interior TUPLE-element locator must fail the output");
    match &err.failure {
        crate::meta_resolve::ComponentMetaOutputFailure::InteriorSourceMiss { path } => {
            assert_eq!(
                path.as_ref(),
                &[verter_type_engine::project_semantic_dispatch::interior_source::InteriorSourceStep::TupleElement { ordinal: 0 }],
            );
        }
        other => panic!("expected InteriorSourceMiss with the tuple path; got {other:?}"),
    }

    // (4) Closed OBJECT with an INDEX-SIGNATURE value slot that cannot deref.
    let index_source =
        tf::SemanticTypeSource::Closed(tf::ClosedTypeFact::Object(tf::ObjectShapeFact {
            members: Arc::from(
                vec![tf::ObjectMemberFact::IndexSignature(
                    tf::IndexSignatureFact {
                        key_name: "k".to_string(),
                        key_type: tf::KeyTypeShape::String,
                        value_type: bad.clone(),
                        readonly: false,
                        span_origin:
                            verter_type_expr::span_origins::IndexSignatureSpansOrigin::Synthetic(
                                SourceSynthetic,
                            ),
                    },
                )]
                .into_boxed_slice(),
            ),
        }));
    let err = build_output_with_prop_source(host, index_source)
        .expect_err("a failed interior INDEX-SIGNATURE value locator must fail the output");
    match &err.failure {
        crate::meta_resolve::ComponentMetaOutputFailure::InteriorSourceMiss { path } => {
            assert_eq!(
                path.as_ref(),
                &[verter_type_engine::project_semantic_dispatch::interior_source::InteriorSourceStep::IndexSignatureValue { ordinal: 0 }],
            );
        }
        other => panic!("expected InteriorSourceMiss with the index-signature path; got {other:?}"),
    }
}

/// FIX-3 non-conflation control: a genuinely ABSENT schema position (an
/// unannotated parameter, a deliberately slot-less signature return — `None`
/// by SCHEMA) is NOT
/// a failed dereference: the output SUCCEEDS and the position renders as
/// typed `Unknown`, exactly as before. The absent-vs-failed split is the
/// schema `Option`, never a heuristic over the materialized `TypeExpr`.
#[test]
fn component_meta_output_genuinely_absent_positions_stay_typed_unknown_not_failure() {
    use verter_type_expr::facts as tf;
    use verter_type_expr::span_origins::SourceSynthetic;
    let project = make_project();
    project
        .upsert_base("/App.vue", "<template><div /></template>")
        .unwrap();
    let host = project.host();

    let unannotated_fn =
        tf::SemanticTypeSource::Closed(tf::ClosedTypeFact::Function(tf::FunctionSignatureFact {
            type_parameters: Arc::from(Vec::new().into_boxed_slice()),
            parameters: Arc::from(
                vec![tf::FunctionParamFact {
                    name: Some("arg".to_string()),
                    optional: false,
                    rest: false,
                    has_ts_annotation: false,
                    ty: None, // genuinely absent by schema
                    span_origin: verter_type_expr::span_origins::FunctionParamSpanOrigin {
                        function: verter_type_expr::span_origins::FunctionSpansOrigin::Synthetic(
                            SourceSynthetic,
                        ),
                        param: verter_type_expr::span_origins::FunctionParamSelector::Positional {
                            ordinal: 0,
                        },
                    },
                }]
                .into_boxed_slice(),
            ),
            return_source: tf::FunctionReturnSource::Absent, // deliberately absent synthetic return
            return_reference_head: tf::AuthoredReferenceHeadFact::Unavailable,
            has_implementation_body: false,
            spans_origin: verter_type_expr::span_origins::FunctionSpansOrigin::Synthetic(
                SourceSynthetic,
            ),
        }));
    let output = build_output_with_prop_source(host, unannotated_fn)
        .expect("a genuinely-absent schema position is NOT a materialization failure");
    let lanes = output.into_parts().2.into_lanes();
    match lanes.props[0].materialized_type().expect("published type") {
        TypeExpr::Function(function) => {
            let param = function.parameters.first().expect("one parameter");
            assert!(
                matches!(param.ty, TypeExpr::Unknown { .. }),
                "the unannotated parameter stays typed Unknown; got {:?}",
                param.ty
            );
        }
        other => panic!("the function source materializes as a function; got {other:?}"),
    }
}
