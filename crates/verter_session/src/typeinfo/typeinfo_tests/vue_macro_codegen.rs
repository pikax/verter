use std::sync::Arc;

use verter_macro_dto::{
    MacroAnchor, MacroRuntimeOutcome, MacroRuntimeShape, MacroTscOutcome, MacroTscProjection,
    PropsDefaultsAssociation, RuntimeConstructor, RuntimeRootShape, SynthesizedRowKind,
};

use crate::typeinfo::vue_macro_codegen::VueMacroCodegenDemand;
use crate::{HostConfig, UpsertRequest, VerterHost};

fn upsert(host: &VerterHost, canonical_id: &str, source: &str) {
    let _ = host
        .upsert(UpsertRequest {
            canonical_id: Some(canonical_id.to_owned()),
            input_id: canonical_id.to_owned(),
            source: Arc::from(source),
            file_language: crate::FileLanguage::vue(),
            aliases: Vec::new(),
        })
        .expect("Vue fixture must upsert");
}

fn produce(
    host: &VerterHost,
    canonical_id: &str,
    demand: VueMacroCodegenDemand,
) -> crate::typeinfo::vue_macro_codegen::VueMacroCodegenOutput {
    crate::resolver_core::with_bare_host_ctx_for_test(host, |ctx| {
        host.produce_vue_macro_codegen_with_ctx(ctx, canonical_id, demand)
    })
}

#[test]
fn runtime_props_are_one_level_and_classified_once_per_public_member() {
    let host = VerterHost::new_standalone(HostConfig::default());
    upsert(
        &host,
        "/src/Props.vue",
        r#"<script setup lang="ts">
type Nested = { child: { leaf: string } }
type EmitMap = { save: (value: number) => void }
defineProps<{
  title?: string
  count: number
  mixed: boolean | { nested: string }
  options: Nested
  onChange: EmitMap['save']
}>()
</script>"#,
    );

    let output = produce(&host, "/src/Props.vue", VueMacroCodegenDemand::Runtime);
    let runtime = output
        .runtime
        .expect("runtime demand must produce a bundle");
    assert!(
        output.tsc.is_none(),
        "runtime demand must not materialize TSC text"
    );
    assert_eq!(output.counters.producer_invocations, 1);
    assert_eq!(output.counters.root_shallow_demands, 1);
    assert_eq!(output.counters.runtime_classifier_calls, 5);
    assert_eq!(output.counters.tsc_materializations, 0);
    assert_eq!(output.counters.scheduler_submissions, 0);
    assert!(
        output
            .transitive_canonicals
            .iter()
            .any(|id| id == "/src/Props.vue"),
        "the per-call fact footprint must retain the owner canonical"
    );
    assert!(output.facts_cacheable);
    assert_eq!(
        output.completeness,
        crate::semantic_query::ResultCompleteness::Complete
    );

    let MacroRuntimeOutcome::Complete(MacroRuntimeShape::Props(props)) =
        &runtime.entries[0].outcome
    else {
        panic!("expected complete props runtime shape: {runtime:?}");
    };
    assert_eq!(props.root_shape, RuntimeRootShape::ObjectLike);
    assert_eq!(
        props
            .props
            .iter()
            .map(|prop| prop.name.as_str())
            .collect::<Vec<_>>(),
        ["title", "count", "mixed", "options", "onChange"]
    );
    assert!(props.props[0].optional);
    assert_eq!(
        props.props[2].constructors.as_slice(),
        &[RuntimeConstructor::Boolean, RuntimeConstructor::Object]
    );
    assert_eq!(
        props.props[3].constructors.as_slice(),
        &[RuntimeConstructor::Object],
        "nested objects stop at Object; their child surface is never enumerated"
    );
    assert_eq!(
        props.props[4].constructors.as_slice(),
        &[RuntimeConstructor::Function]
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
    let MacroRuntimeOutcome::Complete(MacroRuntimeShape::Props(props)) =
        &runtime.entries[0].outcome
    else {
        panic!("expected complete props runtime shape: {runtime:?}");
    };
    assert_eq!(
        props.defaults,
        PropsDefaultsAssociation::WithDefaults {
            defaults_macro_index: 1,
        }
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
    assert_eq!(tsc.counters.root_shallow_demands, 0);
    assert_eq!(tsc.counters.runtime_classifier_calls, 0);
    assert_eq!(tsc.counters.tsc_materializations, 1);
    let MacroTscOutcome::Complete(MacroTscProjection::Props { splice }) =
        &tsc_bundle.entries[0].outcome
    else {
        panic!("expected complete props TSC splice: {tsc_bundle:?}");
    };
    assert!(splice.as_str().contains("name"));
    assert!(splice.as_str().contains("config"));

    let both = produce(
        &host,
        "/src/Isolated.vue",
        VueMacroCodegenDemand::RuntimeAndTsc,
    );
    assert!(both.runtime.is_some() && both.tsc.is_some());
    assert_eq!(both.counters.root_shallow_demands, 1);
    assert_eq!(both.counters.runtime_classifier_calls, 2);
    assert_eq!(both.counters.tsc_materializations, 1);
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

    let MacroRuntimeOutcome::Complete(MacroRuntimeShape::Model(model)) =
        &runtime.entries[1].outcome
    else {
        panic!("expected model shape: {runtime:?}");
    };
    assert_eq!(model.prop.name, "title");
    assert_eq!(
        model.prop.constructors.as_slice(),
        &[RuntimeConstructor::String]
    );
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
    assert_eq!(props.root_shape, RuntimeRootShape::ObjectLike);
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
fn slots_are_typed_unsupported_without_runtime_classification() {
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
    assert!(matches!(
        runtime.entries[0].outcome,
        MacroRuntimeOutcome::Unsupported(_)
    ));
    assert_eq!(output.counters.root_shallow_demands, 0);
    assert_eq!(output.counters.runtime_classifier_calls, 0);
}
