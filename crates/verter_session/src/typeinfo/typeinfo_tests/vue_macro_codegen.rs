use std::sync::Arc;

use verter_macro_dto::{
    MacroAnchor, MacroRuntimeOutcome, MacroRuntimeShape, MacroTscOutcome, MacroTscProjection,
    PropsDefaultsAssociation, RuntimeConstructor, RuntimeProp, SynthesizedRowKind,
    TscDeclarationFailureReason, TscInferredClassTypePosition, TscScriptOwner,
    TscSemanticInferenceUnavailableReason, UnsupportedReason,
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

fn constructors(prop: &RuntimeProp) -> &[RuntimeConstructor] {
    prop.type_shape
        .constructors()
        .expect("fixture prop classification must be complete")
        .as_slice()
}

fn props_projection(
    output: &crate::typeinfo::vue_macro_codegen::VueMacroCodegenOutput,
) -> &verter_macro_dto::TscPropsProjection {
    let bundle = output.tsc.as_ref().expect("TSC bundle");
    let MacroTscOutcome::Complete(MacroTscProjection::Props(props)) = &bundle.entries[0].outcome
    else {
        panic!("complete props projection expected: {bundle:?}");
    };
    props
}

#[test]
fn tsc_class_scope_carries_declaration_dependencies_and_contextual_inference() {
    let host = VerterHost::new_standalone(HostConfig::default());
    upsert(
        &host,
        "/src/external.ts",
        "export interface External { value: string }",
    );
    upsert(
        &host,
        "/src/Class.vue",
        r#"<script setup lang="ts">
import type { External } from './external'
interface Base<T> {}
class Payload<T extends string> implements Base<T> {
  static external: External
  readonly literal = 1
  value = 1
  constructor(public id?: number, protected name = "x", external: External) {}
  method(input = 1) { return input }
  get label() { return "x" }
  set label(value: string) {}
}
defineProps<{ payload: Payload<"x"> }>()
</script>"#,
    );

    let output = produce(&host, "/src/Class.vue", VueMacroCodegenDemand::Tsc);
    let props = props_projection(&output);
    assert_eq!(
        props
            .scope
            .retained_bindings
            .iter()
            .map(|binding| binding.local_name.as_str())
            .collect::<Vec<_>>(),
        ["External"]
    );
    assert_eq!(
        props
            .scope
            .dependency_declarations
            .iter()
            .map(|dependency| dependency.name.as_str())
            .collect::<Vec<_>>(),
        ["Base", "Payload"]
    );
    let payload = props
        .scope
        .dependency_declarations
        .iter()
        .find(|dependency| dependency.name == "Payload")
        .expect("Payload dependency");
    let rows = payload
        .inferred_class_members
        .iter()
        .map(|row| {
            (
                row.name.as_str(),
                row.is_static,
                row.position,
                row.type_text.as_str(),
            )
        })
        .collect::<Vec<_>>();
    assert!(rows.contains(&(
        "literal",
        false,
        TscInferredClassTypePosition::Property,
        "1"
    )));
    for expected in [
        (
            "value",
            false,
            TscInferredClassTypePosition::Property,
            "number",
        ),
        (
            "name",
            false,
            TscInferredClassTypePosition::Property,
            "string",
        ),
        (
            "input",
            false,
            TscInferredClassTypePosition::Parameter,
            "number",
        ),
        (
            "method",
            false,
            TscInferredClassTypePosition::Return,
            "number",
        ),
        (
            "label",
            false,
            TscInferredClassTypePosition::Return,
            "string",
        ),
    ] {
        assert!(
            rows.contains(&expected),
            "missing {expected:?}; rows={rows:?}"
        );
    }
}

#[test]
fn tsc_class_inference_keeps_same_name_static_and_instance_methods_disjoint() {
    let host = VerterHost::new_standalone(HostConfig::default());
    upsert(
        &host,
        "/src/StaticInstanceCollision.vue",
        r#"<script setup lang="ts">
class Payload {
  collide() { return 1 }
  static collide() { return "static" }
}
defineProps<{ payload: Payload }>()
</script>"#,
    );

    let output = produce(
        &host,
        "/src/StaticInstanceCollision.vue",
        VueMacroCodegenDemand::Tsc,
    );
    let payload = props_projection(&output)
        .scope
        .dependency_declarations
        .iter()
        .find(|dependency| dependency.name == "Payload")
        .expect("Payload dependency");

    assert_eq!(payload.declaration_failure, None);
    assert_eq!(
        payload
            .inferred_class_members
            .iter()
            .filter(|member| {
                member.name == "collide"
                    && member.position == TscInferredClassTypePosition::Return
            })
            .map(|member| (member.is_static, member.type_text.as_str()))
            .collect::<Vec<_>>(),
        [(false, "number"), (true, "string")],
        "same-name methods must select return-inference facts from their exact instance/static surface",
    );
}

#[test]
fn tsc_value_dependency_facts_distinguish_carried_and_owner_only_values() {
    let host = VerterHost::new_standalone(HostConfig::default());
    upsert(
        &host,
        "/src/Values.vue",
        r#"<script setup lang="ts">
const seed = { value: "x" }
class Base {}
type Props = { seed: typeof seed; ctor: typeof Base }
defineProps<Props>()
</script>"#,
    );
    let output = produce(&host, "/src/Values.vue", VueMacroCodegenDemand::Tsc);
    let props = props_projection(&output);
    let props_dependency = props
        .scope
        .dependency_declarations
        .iter()
        .find(|dependency| dependency.name == "Props")
        .expect("Props dependency");
    assert_eq!(props_dependency.owner_value_dependencies.len(), 1);
    assert_eq!(props_dependency.owner_value_dependencies[0].name, "seed");
    assert_eq!(
        props_dependency.retained_value_carriers,
        [verter_macro_dto::TscRetainedValueCarrier {
            owner: TscScriptOwner::Setup,
            name: "Base".to_owned(),
            contributor_ordinal: 0,
        }]
    );
    assert!(props
        .scope
        .dependency_declarations
        .iter()
        .any(|dependency| dependency.name == "Base"));

    upsert(
        &host,
        "/src/Self.vue",
        r#"<script setup lang="ts">
class Payload { peer!: typeof Payload }
defineProps<Payload>()
</script>"#,
    );
    let self_output = produce(&host, "/src/Self.vue", VueMacroCodegenDemand::Tsc);
    let payload = props_projection(&self_output)
        .scope
        .dependency_declarations
        .iter()
        .find(|dependency| dependency.name == "Payload")
        .expect("Payload dependency");
    assert!(payload.owner_value_dependencies.is_empty());
    assert_eq!(
        payload.retained_value_carriers,
        [verter_macro_dto::TscRetainedValueCarrier {
            owner: TscScriptOwner::Setup,
            name: "Payload".to_owned(),
            contributor_ordinal: 0,
        }]
    );

    upsert(
        &host,
        "/src/DualSpace.vue",
        r#"<script setup lang="ts">
class Base {}
enum Kind { Ready }
class Payload {
  ctor = Base
  kind = Kind
}
defineProps<Payload>()
</script>"#,
    );
    let dual_space = produce(&host, "/src/DualSpace.vue", VueMacroCodegenDemand::Tsc);
    let props = props_projection(&dual_space);
    assert_eq!(
        props
            .scope
            .dependency_declarations
            .iter()
            .map(|dependency| dependency.name.as_str())
            .collect::<Vec<_>>(),
        ["Base", "Kind", "Payload"]
    );
    let payload = props
        .scope
        .dependency_declarations
        .iter()
        .find(|dependency| dependency.name == "Payload")
        .expect("Payload dependency");
    assert!(
        payload.owner_value_dependencies.is_empty(),
        "dual-space declaration carriers satisfy exact inferred value roots"
    );

    upsert(
        &host,
        "/src/OwnerDiscriminator.vue",
        r#"<script lang="ts">
class Base { companion = true }
class Payload { ctor = Base }
</script>
<script setup lang="ts">
class Base { setup = true }
defineProps<Payload>()
</script>"#,
    );
    let owner_discriminator = produce(
        &host,
        "/src/OwnerDiscriminator.vue",
        VueMacroCodegenDemand::Tsc,
    );
    let props = props_projection(&owner_discriminator);
    assert_eq!(
        props
            .scope
            .dependency_declarations
            .iter()
            .map(|dependency| (dependency.owner, dependency.name.as_str()))
            .collect::<Vec<_>>(),
        [
            (TscScriptOwner::Companion, "Base"),
            (TscScriptOwner::Companion, "Payload"),
        ]
    );
    let payload = props
        .scope
        .dependency_declarations
        .iter()
        .find(|dependency| dependency.name == "Payload")
        .expect("Payload dependency");
    assert_eq!(
        payload.retained_value_carriers,
        [verter_macro_dto::TscRetainedValueCarrier {
            owner: TscScriptOwner::Companion,
            name: "Base".to_owned(),
            contributor_ordinal: 0,
        }],
        "same-name setup carrier must not satisfy a companion-owned inferred value root"
    );
}

#[test]
fn tsc_overload_projection_hides_the_implementation_inference_sites() {
    let host = VerterHost::new_standalone(HostConfig::default());
    upsert(
        &host,
        "/src/Overload.vue",
        r#"<script setup lang="ts">
class Payload {
  method(value: string): string
  method(value: number): number
  method(value) { return value }
}
defineProps<Payload>()
</script>"#,
    );
    let output = produce(&host, "/src/Overload.vue", VueMacroCodegenDemand::Tsc);
    let payload = props_projection(&output)
        .scope
        .dependency_declarations
        .iter()
        .find(|dependency| dependency.name == "Payload")
        .expect("Payload dependency");
    assert!(
        payload.inferred_class_members.is_empty(),
        "implementation-only inference rows must be hidden: {:?}",
        payload.inferred_class_members
    );
}

#[test]
fn tsc_class_return_replay_fails_closed_for_unsupported_and_nested_unsafe_inference() {
    for (file, method) in [
        (
            "/src/UnsupportedReturn.vue",
            "method(flag: boolean) { while (flag) { return 1 } return 0 }",
        ),
        (
            "/src/NestedUnsafeReturn.vue",
            "method() { return [] as any[] }",
        ),
    ] {
        let host = VerterHost::new_standalone(HostConfig::default());
        upsert(
            &host,
            file,
            &format!(
                r#"<script setup lang="ts">
class Payload {{ {method} }}
defineProps<Payload>()
</script>"#
            ),
        );

        let output = produce(&host, file, VueMacroCodegenDemand::Tsc);
        let payload = props_projection(&output)
            .scope
            .dependency_declarations
            .iter()
            .find(|dependency| dependency.name == "Payload")
            .expect("Payload dependency");
        assert_eq!(
            payload.declaration_failure,
            Some(TscDeclarationFailureReason::Unsupported(
                UnsupportedReason::SemanticConstruct,
            )),
            "file={file}"
        );
        assert!(payload.inferred_class_members.is_empty(), "file={file}");
    }
}

#[test]
fn tsc_class_inference_budget_is_exact_partial_and_non_cacheable() {
    let host = VerterHost::new_standalone(HostConfig::default());
    let mut inferred = "0".to_owned();
    for _ in 0..80 {
        inferred = format!("[{inferred}]");
    }
    upsert(
        &host,
        "/src/InferenceBudget.vue",
        &format!(
            r#"<script setup lang="ts">
class Payload {{ method() {{ return {inferred} }} }}
defineProps<Payload>()
</script>"#
        ),
    );

    let output = produce(
        &host,
        "/src/InferenceBudget.vue",
        VueMacroCodegenDemand::Tsc,
    );
    let payload = props_projection(&output)
        .scope
        .dependency_declarations
        .iter()
        .find(|dependency| dependency.name == "Payload")
        .expect("Payload dependency");
    assert_eq!(
        payload.declaration_failure,
        Some(TscDeclarationFailureReason::SemanticInferenceUnavailable(
            TscSemanticInferenceUnavailableReason::DepthBudgetExceeded,
        ))
    );
    assert!(
        output.completeness.is_partial(),
        "budget exhaustion must make the TypeInfo result partial"
    );
    assert!(
        !output.facts_cacheable,
        "budget exhaustion must refuse TypeInfo fact-footprint admission"
    );
}

#[test]
fn producer_origin_hash_tracks_each_exact_snapshot_across_aba_edits() {
    let host = VerterHost::new_standalone(HostConfig::default());
    const FILE: &str = "/src/Revision.vue";
    let v1 = r#"<script setup lang="ts">defineProps<{ value: string }>()</script>"#;
    let v2 = r#"<script setup lang="ts">defineProps<{ value: number }>()</script>"#;

    upsert(&host, FILE, v1);
    let first = produce(&host, FILE, VueMacroCodegenDemand::Tsc);
    upsert(&host, FILE, v2);
    let middle = produce(&host, FILE, VueMacroCodegenDemand::Tsc);
    assert_ne!(first.origin_whole_hash, middle.origin_whole_hash);
    assert!(crate::host_resolve::vue_macro_output_matches_revision(
        &first,
        first.origin_whole_hash.expect("v1 origin")
    ));
    assert!(!crate::host_resolve::vue_macro_output_matches_revision(
        &middle,
        first.origin_whole_hash.expect("v1 origin")
    ));
    assert_eq!(
        props_projection(&middle).testing_rows[0].type_text.as_str(),
        "number"
    );

    upsert(&host, FILE, v1);
    let final_output = produce(&host, FILE, VueMacroCodegenDemand::Tsc);
    assert_eq!(first.origin_whole_hash, final_output.origin_whole_hash);
    assert_ne!(middle.origin_whole_hash, final_output.origin_whole_hash);
    assert_eq!(
        props_projection(&final_output).testing_rows[0]
            .type_text
            .as_str(),
        "string"
    );
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
        constructors(&props.props[2]),
        &[RuntimeConstructor::Boolean, RuntimeConstructor::Object]
    );
    assert_eq!(
        constructors(&props.props[3]),
        &[RuntimeConstructor::Object],
        "nested objects stop at Object; their child surface is never enumerated"
    );
    assert_eq!(
        constructors(&props.props[4]),
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
    assert_eq!(tsc.counters.tsc_materializations, 3);
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
    assert_eq!(both.counters.tsc_materializations, 3);
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

#[test]
fn tsc_emits_models_and_scope_are_explicit_role_rows() {
    let host = VerterHost::new_standalone(HostConfig::default());
    upsert(
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
        [("cancel", "reason?: string"), ("save", "value: Local")]
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
fn scheduler_submission_counter_is_a_request_scoped_witness() {
    use std::sync::atomic::Ordering;

    let host = VerterHost::new_standalone(HostConfig::default());
    upsert(
        &host,
        "/src/SchedulerWitness.vue",
        r#"<script setup lang="ts">defineProps<{ value: string }>()</script>"#,
    );
    host.test_force
        .vue_macro_codegen_scheduler_submission_for_tests
        .store(true, Ordering::Relaxed);

    let output = produce(
        &host,
        "/src/SchedulerWitness.vue",
        VueMacroCodegenDemand::Runtime,
    );
    assert_eq!(
        output.counters.scheduler_submissions, 1,
        "the producer counter must report submissions occurring inside its request scope"
    );
}
