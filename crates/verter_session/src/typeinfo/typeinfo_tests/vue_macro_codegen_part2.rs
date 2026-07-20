//! `vue_macro_codegen` typeinfo tests — second partition.
//!
//! Split from `vue_macro_codegen.rs` (same test module, sibling file) to keep
//! each production `.rs` under the module-size budget. Runs as a child module
//! of the parent test module and reaches its shared fixtures/helpers
//! (`upsert`, `produce`, `constructors`, `props_projection`, the imported
//! DTO types) through `use super::*`.

use super::*;

/// Mutation recipe: omit the prepared declaration's file fact from the
/// runtime-filtered candidate, or admit the filtered value outside the shared
/// fact-validated family memo. Removing only the directive then reuses the old
/// filtered surface and `base` remains absent after the edit.
#[test]
fn vue_ignore_runtime_surface_invalidates_when_only_the_directive_changes() {
    let host = VerterHost::new_standalone(HostConfig::default());
    let canonical = "/src/VueIgnoreEdit.vue";
    upsert(
        &host,
        canonical,
        r#"<script setup lang="ts">
interface Base { base: string }
interface Props extends /* @vue-ignore */ Base { own: number }
defineProps<Props>()
</script>"#,
    );

    let filtered = produce(&host, canonical, VueMacroCodegenDemand::RuntimeAndTsc);
    assert_eq!(
        filtered.completeness,
        crate::semantic_query::ResultCompleteness::Complete
    );
    assert!(filtered.facts_cacheable);
    let filtered_hash = filtered.origin_whole_hash.expect("first content hash");
    let filtered_runtime = filtered.runtime.as_ref().expect("runtime bundle");
    let MacroRuntimeOutcome::Complete(MacroRuntimeShape::Props(filtered_props)) =
        &filtered_runtime.entries[0].outcome
    else {
        panic!("expected filtered runtime props: {filtered_runtime:?}");
    };
    assert_eq!(
        filtered_props
            .props
            .iter()
            .map(|prop| prop.name.as_str())
            .collect::<Vec<_>>(),
        ["own"]
    );
    let filtered_tsc = filtered.tsc.as_ref().expect("TSC bundle");
    let MacroTscOutcome::Complete(MacroTscProjection::Props(filtered_tsc_props)) =
        &filtered_tsc.entries[0].outcome
    else {
        panic!("expected complete TSC props: {filtered_tsc:?}");
    };
    let mut filtered_tsc_names: Vec<_> = filtered_tsc_props
        .testing_rows
        .iter()
        .map(|row| row.name.as_str())
        .collect();
    filtered_tsc_names.sort_unstable();
    assert_eq!(filtered_tsc_names, ["base", "own"]);

    // The declaration topology is unchanged; only the producer fact vanishes.
    upsert(
        &host,
        canonical,
        r#"<script setup lang="ts">
interface Base { base: string }
interface Props extends Base { own: number }
defineProps<Props>()
</script>"#,
    );
    let unfiltered = produce(&host, canonical, VueMacroCodegenDemand::RuntimeAndTsc);
    assert_ne!(
        unfiltered.origin_whole_hash,
        Some(filtered_hash),
        "the edit must advance the value-side file version"
    );
    assert_eq!(
        unfiltered.completeness,
        crate::semantic_query::ResultCompleteness::Complete
    );
    assert!(unfiltered.facts_cacheable);
    let unfiltered_runtime = unfiltered.runtime.as_ref().expect("runtime bundle");
    let MacroRuntimeOutcome::Complete(MacroRuntimeShape::Props(unfiltered_props)) =
        &unfiltered_runtime.entries[0].outcome
    else {
        panic!("expected unfiltered runtime props: {unfiltered_runtime:?}");
    };
    let mut unfiltered_names: Vec<_> = unfiltered_props
        .props
        .iter()
        .map(|prop| prop.name.as_str())
        .collect();
    unfiltered_names.sort_unstable();
    assert_eq!(unfiltered_names, ["base", "own"]);
}

/// Mutation recipe: address ignore facts by declaration name without the
/// prepared declaration's exact owner. The module-side `Props` directive then
/// suppresses setup-side `Props` heritage despite the two owner scopes being
/// distinct.
#[test]
fn vue_ignore_facts_do_not_cross_same_name_owner_scopes() {
    let host = VerterHost::new_standalone(HostConfig::default());
    upsert(
        &host,
        "/src/VueIgnoreOwners.vue",
        r#"<script lang="ts">
interface ModuleBase { moduleInherited: string }
interface Props extends /* @vue-ignore */ ModuleBase { moduleOwn: number }
</script>
<script setup lang="ts">
interface SetupBase { setupInherited: boolean }
interface Props extends SetupBase { setupOwn: number }
defineProps<Props>()
</script>"#,
    );

    let output = produce(
        &host,
        "/src/VueIgnoreOwners.vue",
        VueMacroCodegenDemand::Runtime,
    );
    assert_eq!(
        output.completeness,
        crate::semantic_query::ResultCompleteness::Complete
    );
    let runtime = output.runtime.expect("runtime bundle");
    let MacroRuntimeOutcome::Complete(MacroRuntimeShape::Props(props)) =
        &runtime.entries[0].outcome
    else {
        panic!("expected complete setup props: {runtime:?}");
    };
    assert_eq!(
        props
            .props
            .iter()
            .map(|prop| prop.name.as_str())
            .collect::<Vec<_>>(),
        ["setupInherited", "setupOwn"]
    );
}

/// Mutation recipe: apply the ignore filter after mapped/intersection heritage
/// has already resolved and merged, or fail to propagate the runtime demand
/// through the imported generic carrier. `importedIgnored` then survives in
/// the runtime props surface instead of being cut off at its heritage head.
#[test]
fn imported_vue_ignore_survives_mapped_runtime_projection() {
    let host = VerterHost::new_standalone(HostConfig::default());
    upsert_ts(
        &host,
        "/src/imported-ignore.ts",
        r#"
export interface ImportedBase { importedIgnored: string }
export interface ImportedProps extends /* @vue-ignore */ ImportedBase {
  importedOwn: number
}
"#,
    );
    upsert(
        &host,
        "/src/VueIgnoreImported.vue",
        r#"<script setup lang="ts">
import type { ImportedProps } from './imported-ignore'
type Copy<T> = { [K in keyof T]: T[K] }
defineProps<Copy<ImportedProps>>()
</script>"#,
    );

    let output = produce(
        &host,
        "/src/VueIgnoreImported.vue",
        VueMacroCodegenDemand::Runtime,
    );
    assert_eq!(
        output.completeness,
        crate::semantic_query::ResultCompleteness::Complete
    );
    let runtime = output.runtime.expect("runtime bundle");
    let MacroRuntimeOutcome::Complete(MacroRuntimeShape::Props(props)) =
        &runtime.entries[0].outcome
    else {
        panic!("expected complete imported props: {runtime:?}");
    };
    assert_eq!(
        props
            .props
            .iter()
            .map(|prop| prop.name.as_str())
            .collect::<Vec<_>>(),
        ["importedOwn"]
    );
}

#[test]
fn with_defaults_uses_outer_identity_and_inner_props_payload() {
    let host = VerterHost::new_standalone(HostConfig::default());
    upsert(
        &host,
        "/src/Defaults.vue",
        r#"<script setup lang="ts">
withDefaults(defineProps<{ label?: string; count: number }>(), { label: 'ok' })
</script>"#,
    );

    let output = produce(&host, "/src/Defaults.vue", VueMacroCodegenDemand::Runtime);
    let runtime = output.runtime.expect("runtime bundle");
    assert_eq!(
        runtime.entries.len(),
        1,
        "inner and outer calls form one effective macro"
    );
    assert_eq!(
        runtime.entries[0].macro_index, 1,
        "outer withDefaults owns identity"
    );
    assert_eq!(
        runtime.entries[0].syntax_index, 0,
        "nested analyzer rows must join the single top-level compiler macro"
    );
    let MacroRuntimeOutcome::Complete(MacroRuntimeShape::Props(props)) =
        &runtime.entries[0].outcome
    else {
        panic!("expected complete props runtime shape: {runtime:?}");
    };
    assert_eq!(
        props.defaults,
        PropsDefaultsAssociation::WithDefaults {
            payload_macro_index: 0,
            defaults_macro_index: 1,
        }
    );
    assert_eq!(
        props
            .props
            .iter()
            .map(|prop| prop.anchor)
            .collect::<Vec<_>>(),
        [
            MacroAnchor::Authored {
                macro_index: 0,
                member_ordinal: verter_macro_dto::AuthoredMemberOrdinal::new(0),
            },
            MacroAnchor::Authored {
                macro_index: 0,
                member_ordinal: verter_macro_dto::AuthoredMemberOrdinal::new(1),
            },
        ]
    );
    assert_eq!(output.counters.root_shallow_demands, 1);
    assert_eq!(output.counters.runtime_classifier_calls, 2);
}

#[test]
fn runtime_and_tsc_demands_are_independent() {
    let host = VerterHost::new_standalone(HostConfig::default());
    upsert(
        &host,
        "/src/Isolated.vue",
        r#"<script setup lang="ts">
defineProps<{ name: string; config: { enabled: boolean } }>()
</script>"#,
    );

    let runtime = produce(&host, "/src/Isolated.vue", VueMacroCodegenDemand::Runtime);
    assert!(runtime.runtime.is_some());
    assert!(runtime.tsc.is_none());
    assert_eq!(runtime.counters.root_shallow_demands, 1);
    assert_eq!(runtime.counters.runtime_classifier_calls, 2);
    assert_eq!(runtime.counters.tsc_materializations, 0);

    let tsc = produce(&host, "/src/Isolated.vue", VueMacroCodegenDemand::Tsc);
    assert!(tsc.runtime.is_none());
    let tsc_bundle = tsc.tsc.expect("TSC demand must produce a bundle");
    assert_eq!(tsc.counters.root_shallow_demands, 1);
    assert_eq!(tsc.counters.runtime_classifier_calls, 0);
    assert_eq!(
        tsc.counters.tsc_materializations, 2,
        "only the two testing rows are terminally materialized; public props syntax is compiler-owned"
    );
    let MacroTscOutcome::Complete(MacroTscProjection::Props(props)) =
        &tsc_bundle.entries[0].outcome
    else {
        panic!("expected complete props TSC splice: {tsc_bundle:?}");
    };
    assert_eq!(
        props.public,
        verter_macro_dto::TscPublicPropsProjection::AuthoredArgument {
            anchor: verter_macro_dto::MacroAnchor::MacroArgument { macro_index: 0 },
        },
        "public props syntax remains compiler-owned and source-stable"
    );
    assert_eq!(
        props
            .testing_rows
            .iter()
            .map(|row| (row.name.as_str(), row.optional, row.type_text.as_str()))
            .collect::<Vec<_>>(),
        [
            ("name", false, "string"),
            ("config", false, "{ enabled: boolean }")
        ]
    );

    let both = produce(
        &host,
        "/src/Isolated.vue",
        VueMacroCodegenDemand::RuntimeAndTsc,
    );
    assert!(both.runtime.is_some() && both.tsc.is_some());
    assert_eq!(both.counters.root_shallow_demands, 2);
    assert_eq!(both.counters.runtime_classifier_calls, 2);
    assert_eq!(
        both.counters.tsc_materializations, 2,
        "runtime demand must not add TSC materializations"
    );
}

#[test]
fn emit_names_and_model_rows_do_not_classify_payloads_beyond_the_model_prop() {
    let host = VerterHost::new_standalone(HostConfig::default());
    upsert(
        &host,
        "/src/Events.vue",
        r#"<script setup lang="ts">
defineEmits<{
  save: [id: number]
  (event: 'cancel' | 'close'): void
}>()
defineModel<string>('title')
</script>"#,
    );

    let output = produce(&host, "/src/Events.vue", VueMacroCodegenDemand::Runtime);
    let runtime = output.runtime.expect("runtime bundle");
    assert_eq!(runtime.entries.len(), 2);
    assert_eq!(
        output.counters.root_shallow_demands, 1,
        "only emits needs a root surface"
    );
    assert_eq!(
        output.counters.runtime_classifier_calls, 1,
        "emit payloads are names-only and model classifies only its prop"
    );

    let MacroRuntimeOutcome::Complete(MacroRuntimeShape::Emits(emits)) =
        &runtime.entries[0].outcome
    else {
        panic!("expected emit shape: {runtime:?}");
    };
    assert_eq!(
        emits
            .iter()
            .map(|event| event.name.as_str())
            .collect::<Vec<_>>(),
        ["save", "cancel", "close"]
    );

    let tsc_output = produce(&host, "/src/Events.vue", VueMacroCodegenDemand::Tsc);
    let tsc = tsc_output.tsc.expect("TSC bundle");
    let MacroTscOutcome::Complete(MacroTscProjection::Emits(emits)) = &tsc.entries[0].outcome
    else {
        panic!("expected TSC emit shape: {tsc:?}");
    };
    assert_eq!(
        emits
            .events
            .iter()
            .map(|event| event.name.as_str())
            .collect::<Vec<_>>(),
        ["save", "cancel", "close"],
        "runtime and TSC projections must preserve one authored event order"
    );

    let MacroRuntimeOutcome::Complete(MacroRuntimeShape::Model(model)) =
        &runtime.entries[1].outcome
    else {
        panic!("expected model shape: {runtime:?}");
    };
    assert_eq!(model.prop.name, "title");
    assert_eq!(constructors(&model.prop), &[RuntimeConstructor::String]);
    assert_eq!(model.update_event.name, "update:title");
    assert_eq!(model.modifiers_prop.name, "titleModifiers");
    assert_eq!(
        model.prop.anchor,
        MacroAnchor::Synthesized {
            macro_index: 1,
            row: SynthesizedRowKind::ModelProp,
        }
    );
}

/// Mutation recipe: flatten tuple members in `render_tuple_parameters` while
/// leaving function-form signatures unchanged. The `save` row loses its
/// terminal rest-tuple form while the `cancel` control stays direct.
#[test]
fn tsc_emits_models_and_scope_are_explicit_role_rows() {
    let host = VerterHost::new_standalone(HostConfig::default());
    upsert_ts(
        &host,
        "/src/types.ts",
        "export interface Payload { id: number }",
    );
    upsert(
        &host,
        "/src/TscRows.vue",
        r#"<script setup lang="ts">
import type { Payload } from './types'
interface Local { payload: Payload }
defineEmits<{
  save: [value: Local]
  (event: 'cancel', reason?: string): void
}>()
defineModel<Payload>('selected')
</script>"#,
    );

    let output = produce(&host, "/src/TscRows.vue", VueMacroCodegenDemand::Tsc);
    let bundle = output.tsc.expect("TSC bundle");
    assert_eq!(bundle.entries.len(), 2);

    let MacroTscOutcome::Complete(MacroTscProjection::Emits(emits)) = &bundle.entries[0].outcome
    else {
        panic!("expected explicit emits projection: {bundle:?}");
    };
    assert_eq!(
        emits
            .events
            .iter()
            .map(|event| (event.name.as_str(), event.emit_parameters.as_str()))
            .collect::<Vec<_>>(),
        [
            ("save", "...args: [value: Local]"),
            ("cancel", "reason?: string"),
        ]
    );
    assert!(emits
        .scope
        .retained_bindings
        .iter()
        .any(|binding| binding.local_name == "Payload"));
    assert!(emits
        .scope
        .dependency_declarations
        .iter()
        .any(|declaration| declaration.name == "Local"));

    let MacroTscOutcome::Complete(MacroTscProjection::Model(model)) = &bundle.entries[1].outcome
    else {
        panic!("expected explicit model projection: {bundle:?}");
    };
    assert_eq!(model.name, "selected");
    assert_eq!(model.value_type.as_str(), "Payload");
}

#[test]
fn resolved_empty_props_are_not_collapsed_into_unavailable() {
    let host = VerterHost::new_standalone(HostConfig::default());
    upsert(
        &host,
        "/src/Empty.vue",
        r#"<script setup lang="ts">defineProps<{}>()</script>"#,
    );
    upsert(
        &host,
        "/src/Missing.vue",
        r#"<script setup lang="ts">defineProps<MissingType>()</script>"#,
    );

    let empty = produce(&host, "/src/Empty.vue", VueMacroCodegenDemand::Runtime);
    let empty_bundle = empty.runtime.expect("empty runtime bundle");
    let MacroRuntimeOutcome::Complete(MacroRuntimeShape::Props(props)) =
        &empty_bundle.entries[0].outcome
    else {
        panic!("resolved empty object must be complete: {empty_bundle:?}");
    };
    assert!(props.props.is_empty());

    let missing = produce(&host, "/src/Missing.vue", VueMacroCodegenDemand::Runtime);
    let missing_bundle = missing.runtime.expect("missing runtime bundle");
    assert!(
        !matches!(
            missing_bundle.entries[0].outcome,
            MacroRuntimeOutcome::Complete(_)
        ),
        "an unavailable type argument must not masquerade as resolved-empty"
    );
}

#[test]
fn resolved_non_object_props_are_invalid_not_complete_empty() {
    let host = VerterHost::new_standalone(HostConfig::default());
    upsert(
        &host,
        "/src/Invalid.vue",
        r#"<script setup lang="ts">defineProps<string>()</script>"#,
    );

    let output = produce(&host, "/src/Invalid.vue", VueMacroCodegenDemand::Runtime);
    let bundle = output.runtime.expect("runtime bundle");
    assert!(
        matches!(
            bundle.entries[0].outcome,
            MacroRuntimeOutcome::Invalid(ref failure)
                if failure.reason == verter_macro_dto::MacroInvalidReason::NonObjectRoot
        ),
        "a resolved primitive root must retain the invalid-root policy: {bundle:?}"
    );
}

#[test]
fn resolved_imported_non_object_props_are_invalid_on_both_codegen_rails() {
    let host = VerterHost::new_standalone(HostConfig::default());
    upsert_ts(&host, "/src/wrong.ts", "export type WrongProps = string\n");
    upsert(
        &host,
        "/src/InvalidImported.vue",
        r#"<script setup lang="ts">
import type { WrongProps } from './wrong'
defineProps<WrongProps>()
</script>"#,
    );

    let output = produce(
        &host,
        "/src/InvalidImported.vue",
        VueMacroCodegenDemand::RuntimeAndTsc,
    );
    let runtime = output.runtime.expect("runtime bundle");
    assert!(
        matches!(
            runtime.entries[0].outcome,
            MacroRuntimeOutcome::Invalid(ref failure)
                if failure.reason == verter_macro_dto::MacroInvalidReason::NonObjectRoot
        ),
        "a resolved imported primitive root must be runtime-invalid: {runtime:?}"
    );
    let tsc = output.tsc.expect("TSC bundle");
    assert!(
        matches!(
            tsc.entries[0].outcome,
            MacroTscOutcome::Invalid(ref failure)
                if failure.reason == verter_macro_dto::MacroInvalidReason::NonObjectRoot
        ),
        "a resolved imported primitive root must be TSC-invalid: {tsc:?}"
    );
}

fn assert_complete_emits_across_demands(
    host: &VerterHost,
    canonical_id: &str,
    expected: &[(&str, &str)],
) {
    for demand in [
        VueMacroCodegenDemand::Runtime,
        VueMacroCodegenDemand::Tsc,
        VueMacroCodegenDemand::RuntimeAndTsc,
    ] {
        let output = produce(host, canonical_id, demand);
        assert!(
            output.dependency_failures.is_empty(),
            "valid emit payloads must not create dependency failures: {output:?}"
        );
        if matches!(
            demand,
            VueMacroCodegenDemand::Runtime | VueMacroCodegenDemand::RuntimeAndTsc
        ) {
            let runtime = output.runtime.as_ref().expect("requested runtime bundle");
            let MacroRuntimeOutcome::Complete(MacroRuntimeShape::Emits(rows)) =
                &runtime.entries[0].outcome
            else {
                panic!("expected a complete runtime emits shape: {runtime:?}");
            };
            assert_eq!(
                rows.iter().map(|row| row.name.as_str()).collect::<Vec<_>>(),
                expected.iter().map(|(name, _)| *name).collect::<Vec<_>>()
            );
        } else {
            assert!(output.runtime.is_none());
        }
        if matches!(
            demand,
            VueMacroCodegenDemand::Tsc | VueMacroCodegenDemand::RuntimeAndTsc
        ) {
            let tsc = output.tsc.as_ref().expect("requested TSC bundle");
            let MacroTscOutcome::Complete(MacroTscProjection::Emits(projection)) =
                &tsc.entries[0].outcome
            else {
                panic!("expected a complete TSC emits projection: {tsc:?}");
            };
            assert_eq!(
                projection
                    .events
                    .iter()
                    .map(|row| {
                        (
                            row.name.as_str(),
                            row.emit_parameters.as_str(),
                            row.handler_parameters.as_str(),
                        )
                    })
                    .collect::<Vec<_>>(),
                expected
                    .iter()
                    .map(|(name, parameters)| (*name, *parameters, *parameters))
                    .collect::<Vec<_>>()
            );
        } else {
            assert!(output.tsc.is_none());
        }
    }
}

/// Mutation recipe: normalize member payloads with the enclosing runtime
/// surface's Shallow context instead of the terminal published Navigate
/// context, or flatten `render_tuple_parameters` to its inner parameter list.
/// Indexed-access tuples then either fail with `InvalidEmitsShape` or lose the
/// terminal `...args: [tuple]` contract asserted for both TSC roles.
#[test]
fn indexed_access_tuple_emits_are_complete_and_demand_invariant() {
    let host = VerterHost::new_standalone(HostConfig::default());
    upsert_ts(
        &host,
        "/src/indexed-emits.ts",
        r#"export type LayerEmits = {
  escapeKeydown: [event: KeyboardEvent]
  pointerdownOutside: [event: PointerEvent]
}
export type SharedEmits = {
  escapeKeydown: LayerEmits['escapeKeydown']
  pointerdownOutside: LayerEmits['pointerdownOutside']
}
"#,
    );
    const FILE: &str = "/src/IndexedEmits.vue";
    upsert(
        &host,
        FILE,
        r#"<script setup lang="ts">
import type { SharedEmits } from './indexed-emits'
defineEmits<SharedEmits>()
</script>"#,
    );

    assert_complete_emits_across_demands(
        &host,
        FILE,
        &[
            ("escapeKeydown", "...args: [event: KeyboardEvent]"),
            ("pointerdownOutside", "...args: [event: PointerEvent]"),
        ],
    );
}

/// Mutation recipe: bypass indexed-access normalization, accept unresolved
/// member names through a fallback, or flatten `render_tuple_parameters` to
/// its inner parameter list. The paired TSC role assertions then degrade to
/// unknown payloads or lose their terminal `...args: [tuple]` contract even if
/// runtime event names happen to survive.
#[test]
fn omit_alias_of_indexed_access_tuple_emits_is_complete_and_demand_invariant() {
    let host = VerterHost::new_standalone(HostConfig::default());
    upsert_ts(
        &host,
        "/src/base-emits.ts",
        r#"export type BaseEmits = {
  close: []
  escapeKeydown: [event: KeyboardEvent]
  pointerdownOutside: [event: PointerEvent]
}
"#,
    );
    upsert_ts(
        &host,
        "/src/shared-emits.ts",
        r#"import type { BaseEmits } from './base-emits'
export type SharedEmits = {
  close: BaseEmits['close']
  escapeKeydown: BaseEmits['escapeKeydown']
  pointerdownOutside: BaseEmits['pointerdownOutside']
}
export type SubEmits = Omit<SharedEmits, 'close'>
"#,
    );
    upsert_ts(
        &host,
        "/src/alias-emits.ts",
        "import type { SubEmits } from './shared-emits'\nexport type PublicEmits = SubEmits\n",
    );
    const FILE: &str = "/src/OmitIndexedEmits.vue";
    upsert(
        &host,
        FILE,
        r#"<script setup lang="ts">
import type { PublicEmits } from './alias-emits'
defineEmits<PublicEmits>()
</script>"#,
    );

    assert_complete_emits_across_demands(
        &host,
        FILE,
        &[
            ("escapeKeydown", "...args: [event: KeyboardEvent]"),
            ("pointerdownOutside", "...args: [event: PointerEvent]"),
        ],
    );
}

/// Mutation recipe: treat `Any`/`Unknown` primitive carriers as definitely
/// incompatible instead of conservative open payloads. Both semantic rails
/// then close this valid fallback surface as `InvalidEmitsShape`.
#[test]
fn open_emit_member_payloads_remain_conservative_and_demand_invariant() {
    let host = VerterHost::new_standalone(HostConfig::default());
    const FILE: &str = "/src/OpenEmits.vue";
    upsert(
        &host,
        FILE,
        r#"<script setup lang="ts">
defineEmits<{
  opaque: any
  honestUnknown: unknown
}>()
</script>"#,
    );

    assert_complete_emits_across_demands(
        &host,
        FILE,
        &[
            ("opaque", "...args: unknown[]"),
            ("honestUnknown", "...args: unknown[]"),
        ],
    );
}

/// Mutation recipe: admit every public emits member by name without checking
/// its resolved payload node. The scalar member then reverts to a complete
/// runtime/TSC shape and this demand matrix fails.
#[test]
fn resolved_imported_invalid_emits_members_are_typed_and_demand_invariant() {
    let host = VerterHost::new_standalone(HostConfig::default());
    upsert_ts(
        &host,
        "/src/wrong-emits.ts",
        "export interface WrongEmits { broken: string }\n",
    );
    const FILE: &str = "/src/InvalidEmits.vue";
    upsert(
        &host,
        FILE,
        r#"<script setup lang="ts">
import type { WrongEmits } from './wrong-emits'
defineEmits<WrongEmits>()
</script>"#,
    );

    for demand in [
        VueMacroCodegenDemand::Runtime,
        VueMacroCodegenDemand::Tsc,
        VueMacroCodegenDemand::RuntimeAndTsc,
    ] {
        let output = produce(&host, FILE, demand);
        assert!(
            output.dependency_failures.is_empty(),
            "a resolved wrong shape must not masquerade as a dependency failure: {output:?}"
        );
        if matches!(
            demand,
            VueMacroCodegenDemand::Runtime | VueMacroCodegenDemand::RuntimeAndTsc
        ) {
            let runtime = output.runtime.as_ref().expect("requested runtime bundle");
            assert!(
                matches!(
                    runtime.entries[0].outcome,
                    MacroRuntimeOutcome::Invalid(ref failure)
                        if failure.reason == MacroInvalidReason::InvalidEmitsShape
                ),
                "runtime demand must retain the typed emits-shape failure: {runtime:?}"
            );
        } else {
            assert!(output.runtime.is_none());
        }
        if matches!(
            demand,
            VueMacroCodegenDemand::Tsc | VueMacroCodegenDemand::RuntimeAndTsc
        ) {
            let tsc = output.tsc.as_ref().expect("requested TSC bundle");
            assert!(
                matches!(
                    tsc.entries[0].outcome,
                    MacroTscOutcome::Invalid(ref failure)
                        if failure.reason == MacroInvalidReason::InvalidEmitsShape
                ),
                "TSC demand must retain the typed emits-shape failure: {tsc:?}"
            );
        } else {
            assert!(output.tsc.is_none());
        }
    }
}

#[test]
fn empty_emits_object_remains_a_valid_complete_shape_on_both_rails() {
    let host = VerterHost::new_standalone(HostConfig::default());
    const FILE: &str = "/src/EmptyEmits.vue";
    upsert(
        &host,
        FILE,
        r#"<script setup lang="ts">defineEmits<{}>()</script>"#,
    );

    let output = produce(&host, FILE, VueMacroCodegenDemand::RuntimeAndTsc);
    assert!(matches!(
        output.runtime.as_ref().expect("runtime bundle").entries[0].outcome,
        MacroRuntimeOutcome::Complete(MacroRuntimeShape::Emits(ref rows)) if rows.is_empty()
    ));
    assert!(matches!(
        output.tsc.as_ref().expect("TSC bundle").entries[0].outcome,
        MacroTscOutcome::Complete(MacroTscProjection::Emits(ref projection))
            if projection.events.is_empty()
    ));
}

#[test]
fn type_only_slots_stay_out_of_runtime_bundle_without_losing_meta_surface() {
    let host = VerterHost::new_standalone(HostConfig::default());
    upsert(
        &host,
        "/src/Slots.vue",
        r#"<script setup lang="ts">
defineSlots<{ default(props: { deep: { value: string } }): unknown }>()
</script>"#,
    );

    let output = produce(&host, "/src/Slots.vue", VueMacroCodegenDemand::Runtime);
    let runtime = output.runtime.expect("runtime bundle");
    assert!(
        runtime.entries.is_empty(),
        "defineSlots has no runtime semantic output and must not enter the runtime join: {runtime:?}"
    );
    assert_eq!(output.counters.root_shallow_demands, 0);
    assert_eq!(output.counters.runtime_classifier_calls, 0);

    let meta = host
        .get_component_meta("/src/Slots.vue")
        .expect("defineSlots component-meta surface");
    let default_slot = meta
        .slots
        .iter()
        .find(|slot| slot.name == "default")
        .expect("the type-only slot remains published outside runtime codegen");
    assert!(
        default_slot
            .bindings
            .iter()
            .any(|binding| binding.name == "deep"),
        "the defineSlots binding surface remains available: {default_slot:?}"
    );
}

#[test]
fn unknown_plus_number_collapses_to_no_runtime_constructors() {
    let host = VerterHost::new_standalone(HostConfig::default());
    upsert(
        &host,
        "/src/UnknownNumber.vue",
        r#"<script setup lang="ts">
defineProps<{ value: number | bigint }>()
</script>"#,
    );

    let output = produce(
        &host,
        "/src/UnknownNumber.vue",
        VueMacroCodegenDemand::Runtime,
    );
    let runtime = output.runtime.expect("runtime bundle");
    let MacroRuntimeOutcome::Complete(MacroRuntimeShape::Props(props)) =
        &runtime.entries[0].outcome
    else {
        panic!("expected complete props shape: {runtime:?}");
    };
    assert!(
        constructors(&props.props[0]).is_empty(),
        "Unknown mixed with Number must collapse to Vue's no-constructor runtime shape: {:?}",
        constructors(&props.props[0])
    );
    assert!(!props.props[0].type_shape.skip_check());
}

#[test]
fn scheduler_submits_once_for_multi_macro_sfc_without_member_fanout() {
    let host = VerterHost::new_standalone(HostConfig::default());
    upsert(
        &host,
        "/src/SchedulerWitness.vue",
        r#"<script lang="ts">
export interface CompanionProps { companion: string }
</script>
<script setup lang="ts">
defineProps<{
  first: string
  second?: number
  third: boolean
}>()
defineEmits<{
  save: [value: string]
  cancel: []
}>()
defineModel<string>('selected')
</script>"#,
    );

    let submissions_before = host
        .scheduler()
        .counters()
        .submit_count
        .load(std::sync::atomic::Ordering::Relaxed);
    let output = produce(
        &host,
        "/src/SchedulerWitness.vue",
        VueMacroCodegenDemand::RuntimeAndTsc,
    );
    let submissions_after = host
        .scheduler()
        .counters()
        .submit_count
        .load(std::sync::atomic::Ordering::Relaxed);

    assert_eq!(
        output.counters.scheduler_submissions, 1,
        "one SFC demand must report exactly one scoped scheduler submission"
    );
    assert_eq!(
        submissions_after - submissions_before,
        1,
        "props, emits, models, companion scope, and public members must remain inside one scheduled closure"
    );
    assert_eq!(
        output
            .runtime
            .as_ref()
            .expect("runtime bundle")
            .entries
            .len(),
        3
    );
    assert_eq!(output.tsc.as_ref().expect("TSC bundle").entries.len(), 3);
}

fn assert_cancelled_macro_output(
    output: &crate::typeinfo::vue_macro_codegen::VueMacroCodegenOutput,
) {
    assert!(output
        .completeness
        .reasons()
        .contains(crate::semantic_query::PartialReasonSet::CANCELLED));
    assert!(!output.facts_cacheable);
    let runtime = output.runtime.as_ref().expect("runtime bundle");
    assert!(!runtime.entries.is_empty());
    assert!(runtime.entries.iter().all(|entry| matches!(
        entry.outcome,
        MacroRuntimeOutcome::Partial(ref failure)
            if failure.reason == MacroPartialReason::Cancelled
    )));
    let tsc = output.tsc.as_ref().expect("TSC bundle");
    assert!(!tsc.entries.is_empty());
    assert!(tsc.entries.iter().all(|entry| matches!(
        entry.outcome,
        MacroTscOutcome::Partial(ref failure)
            if failure.reason == MacroPartialReason::Cancelled
    )));
}

#[test]
fn cancelled_request_returns_typed_partial_and_uncancelled_retry_completes() {
    let host = VerterHost::new_standalone(HostConfig::default());
    const FILE: &str = "/src/CancelledRetry.vue";
    upsert(
        &host,
        FILE,
        r#"<script setup lang="ts">defineProps<{ value: string }>()</script>"#,
    );

    let cancelled = crate::request_context::RequestContext::new(7001, Arc::from(FILE), false, None);
    cancelled.cancel();
    let cancelled_output = {
        let _guard = crate::request_context::RequestContextGuard::install(cancelled);
        produce(&host, FILE, VueMacroCodegenDemand::RuntimeAndTsc)
    };
    assert_cancelled_macro_output(&cancelled_output);

    let retry = produce(&host, FILE, VueMacroCodegenDemand::RuntimeAndTsc);
    assert_eq!(
        retry.completeness,
        crate::semantic_query::ResultCompleteness::Complete
    );
    assert!(retry.facts_cacheable);
    assert!(matches!(
        retry.runtime.as_ref().expect("runtime bundle").entries[0].outcome,
        MacroRuntimeOutcome::Complete(_)
    ));
    assert!(matches!(
        retry.tsc.as_ref().expect("TSC bundle").entries[0].outcome,
        MacroTscOutcome::Complete(_)
    ));
}

#[cfg(not(target_arch = "wasm32"))]
#[test]
fn cancelled_winner_does_not_abort_live_sibling() {
    let host = Arc::new(VerterHost::new_standalone(HostConfig::default()));
    const FILE: &str = "/src/CancelledWinner.vue";
    upsert(
        host.as_ref(),
        FILE,
        r#"<script setup lang="ts">defineProps<{ value: string }>()</script>"#,
    );
    let rendezvous = Arc::new((std::sync::Barrier::new(2), std::sync::Barrier::new(2)));
    *host.test_force.vue_macro_codegen_build_rendezvous.lock() = Some(Arc::clone(&rendezvous));
    let submissions_before = host
        .scheduler()
        .counters()
        .submit_count
        .load(std::sync::atomic::Ordering::Acquire);

    let winner_context =
        crate::request_context::RequestContext::new(7002, Arc::from(FILE), false, None);
    let winner = {
        let host = Arc::clone(&host);
        let winner_context = Arc::clone(&winner_context);
        std::thread::spawn(move || {
            let _guard = crate::request_context::RequestContextGuard::install(winner_context);
            produce(host.as_ref(), FILE, VueMacroCodegenDemand::RuntimeAndTsc)
        })
    };
    rendezvous.0.wait();

    let sibling = {
        let host = Arc::clone(&host);
        std::thread::spawn(move || {
            let context =
                crate::request_context::RequestContext::new(7003, Arc::from(FILE), false, None);
            let _guard = crate::request_context::RequestContextGuard::install(context);
            produce(host.as_ref(), FILE, VueMacroCodegenDemand::RuntimeAndTsc)
        })
    };

    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
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
        "the sibling must join the scoped flight before the winner is cancelled"
    );
    winner_context.cancel();
    rendezvous.1.wait();

    let winner_output = winner.join().expect("winner thread");
    let sibling_output = sibling.join().expect("sibling thread");
    *host.test_force.vue_macro_codegen_build_rendezvous.lock() = None;

    assert_cancelled_macro_output(&winner_output);
    assert_eq!(
        sibling_output.completeness,
        crate::semantic_query::ResultCompleteness::Complete
    );
    assert!(sibling_output.facts_cacheable);
    assert!(matches!(
        sibling_output
            .runtime
            .as_ref()
            .expect("runtime bundle")
            .entries[0]
            .outcome,
        MacroRuntimeOutcome::Complete(_)
    ));
}
