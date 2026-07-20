use std::sync::Arc;

use verter_macro_dto::{
    MacroAnchor, MacroMemberReason, MacroPartialReason, MacroRuntimeOutcome, MacroRuntimeShape,
    MacroTscOutcome, MacroTscProjection, PropsDefaultsAssociation, RuntimeConstructor, RuntimeProp,
    RuntimePropType, SynthesizedRowKind, TscDeclarationFailureReason, TscInferredClassTypePosition,
    TscScriptOwner, TscSemanticInferenceUnavailableReason, UnresolvedReason, UnsupportedReason,
};

use crate::typeinfo::vue_macro_codegen::{VueMacroCodegenDemand, VueMacroDependencyFailure};
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

fn upsert_ts(host: &VerterHost, canonical_id: &str, source: &str) {
    let _ = host
        .upsert(UpsertRequest {
            canonical_id: Some(canonical_id.to_owned()),
            input_id: canonical_id.to_owned(),
            source: Arc::from(source),
            file_language: crate::FileLanguage::script_ts(),
            aliases: Vec::new(),
        })
        .expect("TypeScript fixture must upsert");
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

#[test]
fn missing_root_dependency_is_typed_and_demand_invariant() {
    let host = VerterHost::new_standalone(HostConfig::default());
    const FILE: &str = "/src/MissingRoot.vue";
    upsert(
        &host,
        FILE,
        r#"<script setup lang="ts">
import type { Props } from './missing'
defineProps<Props>()
</script>"#,
    );

    let outputs = [
        VueMacroCodegenDemand::Runtime,
        VueMacroCodegenDemand::Tsc,
        VueMacroCodegenDemand::RuntimeAndTsc,
    ]
    .map(|demand| produce(&host, FILE, demand));
    for output in &outputs {
        assert_eq!(
            output.dependency_failures,
            [VueMacroDependencyFailure::MissingRoot {
                macro_index: 0,
                owner: verter_type_expr::TopLevelOwnerId::instance(0),
                import_source: "./missing".to_string(),
                type_name: "Props".to_string(),
            }],
            "the missing root must ride as one typed, deterministic failure"
        );
    }
}

#[test]
fn unresolved_surface_arm_is_typed_and_demand_invariant() {
    let host = VerterHost::new_standalone(HostConfig::default());
    upsert_ts(
        &host,
        "/src/types.ts",
        "import type { MissingBase } from './missing'\n\
         export interface Props extends MissingBase { own?: string }",
    );
    const FILE: &str = "/src/MissingArm.vue";
    upsert(
        &host,
        FILE,
        r#"<script setup lang="ts">
import type { Props } from './types'
defineProps<Props>()
</script>"#,
    );

    let outputs = [
        VueMacroCodegenDemand::Runtime,
        VueMacroCodegenDemand::Tsc,
        VueMacroCodegenDemand::RuntimeAndTsc,
    ]
    .map(|demand| produce(&host, FILE, demand));
    for output in &outputs {
        assert_eq!(
            output.dependency_failures,
            [VueMacroDependencyFailure::UnresolvedSurfaceArm {
                macro_index: 0,
                macro_owner: verter_type_expr::TopLevelOwnerId::instance(0),
                name: Arc::from("MissingBase"),
                owner_canonical: Arc::from("/src/types.ts"),
                owner: verter_type_expr::TopLevelOwnerId::ordinary_file(),
            }],
            "the dropped surface arm must ride as one typed, deterministic failure"
        );
    }
}

#[test]
fn unrelated_runtime_import_does_not_create_macro_dependency_failure() {
    let host = VerterHost::new_standalone(HostConfig::default());
    const FILE: &str = "/src/UnrelatedRuntime.vue";
    upsert(
        &host,
        FILE,
        r#"<script setup lang="ts">
import { runtimeOnly } from './missing-runtime'
defineProps<{ own?: string }>()
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
            "ordinary runtime imports are outside the macro dependency channel: {:?}",
            output.dependency_failures
        );
    }
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
    upsert_ts(
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
fn tsc_value_dependency_fixture_preserves_exact_shallow_boundary_facts() {
    let host = VerterHost::new_standalone(HostConfig::default());
    let canonical = "/src/ValuesBoundary.vue";
    upsert(
        &host,
        canonical,
        r#"<script setup lang="ts">
const seed = { value: "x" }
class Base {}
type Props = { seed: typeof seed; ctor: typeof Base }
defineProps<Props>()
</script>"#,
    );

    let indexed = host
        .ensure_indexed_ready(canonical)
        .expect("fixture must publish an indexed artifact");
    let analysis = indexed
        .script_analysis
        .as_ref()
        .expect("fixture must publish script analysis");
    let (macro_index, mac) = analysis
        .macros
        .iter()
        .enumerate()
        .find(|(_, mac)| mac.kind == verter_semantic::analysis::AnalyzedMacroKind::DefineProps)
        .expect("fixture must publish defineProps");
    let owner = verter_type_expr::TopLevelOwnerId::instance(0);
    assert_eq!(mac.owner, owner);
    assert!(indexed.route_inventory.imports.is_empty());
    assert!(indexed.route_inventory.reexports.is_empty());
    assert!(indexed.shallow_state.has_type_symbol_in(owner, "Props"));
    assert!(indexed.shallow_state.has_type_symbol_in(owner, "Base"));
    assert!(indexed.shallow_state.has_value_symbol_in(owner, "Base"));
    assert!(indexed.shallow_state.has_value_symbol_in(owner, "seed"));
    let resolved = crate::resolver_core::bare_name_resolve::resolve_bare_name_in_scope(
        &host, canonical, owner, None, "Props",
    )
    .expect("exact-owner header facts must resolve the local Props root");
    assert_eq!(resolved.canonical_id.as_ref(), canonical);
    assert_eq!(resolved.owner, owner);
    assert_eq!(resolved.symbol_name.as_ref(), "Props");

    let deps = indexed
        .shallow_state
        .type_deps_in(owner, "Props")
        .expect("memo-backed Props dependency facts must be present");
    assert_eq!(deps.owner_value_deps, ["seed"]);
    assert_eq!(deps.retained_value_carrier_deps, ["Base"]);

    let hot =
        crate::structural_carrier_producer::macro_type_arg_hot_ref(&host, canonical, macro_index)
            .expect("macro payload carrier must be present");
    let data = crate::project_semantic_dispatch::node_data_for(&host, hot.node())
        .expect("macro payload carrier node must be interned");
    let (name, scope) = data
        .bare_ref_head()
        .expect("defineProps<Props> must preserve its unresolved bare carrier");
    assert_eq!(name.as_ref(), "Props");
    assert!(matches!(
        scope,
        crate::semantic_query::NodeScopeId::File {
            canonical_id,
            owner: scope_owner,
            ..
        } if canonical_id.as_ref() == canonical && *scope_owner == owner
    ));

    crate::resolver_core::with_bare_host_ctx_for_test(&host, |ctx| {
        let dispatch = crate::project_semantic_dispatch::ProjectSemanticDispatch::new(ctx);
        let resolved = dispatch.resolve_carrier_subject_node(
            hot.node(),
            crate::semantic_query::ProjectionReductionContext::structural_transit_with_mode(
                crate::semantic_query::ProjectionMode::Navigate,
            ),
        );
        let data = crate::project_semantic_dispatch::node_data_for(ctx, resolved)
            .expect("direct carrier resolution must preserve exact declaration identity");
        let crate::semantic_query::SemanticNodeData::DeclRef { identity } = data.as_ref() else {
            panic!("local Props carrier must resolve to an exact DeclRef: {data:?}");
        };
        assert_eq!(identity.canonical_id.as_ref(), canonical);
        assert_eq!(identity.owner, owner);
        assert_eq!(identity.decl_name.as_ref(), "Props");
    });
}

#[test]
fn sfc_setup_scope_resolves_unique_companion_owner_without_reverse_visibility() {
    let host = VerterHost::new_standalone(HostConfig::default());
    let canonical = "/src/OwnerChain.vue";
    upsert(
        &host,
        canonical,
        r#"<script lang="ts">
type CompanionOnly = { companion: true }
type Shared = { source: "companion" }
</script>
<script setup lang="ts">
export type SetupOnly = { setup: true }
type Shared = { source: "setup" }
defineProps<CompanionOnly & Shared>()
</script>"#,
    );

    let module = verter_type_expr::TopLevelOwnerId::ordinary_file();
    let instance = verter_type_expr::TopLevelOwnerId::instance(0);
    crate::resolver_core::with_bare_host_ctx_for_test(&host, |ctx| {
        let companion = crate::resolver_core::bare_name_resolve::resolve_bare_name_in_scope(
            ctx,
            canonical,
            instance,
            None,
            "CompanionOnly",
        )
        .expect("setup must see the unique validated companion owner");
        assert_eq!(companion.owner, module);
        assert_eq!(companion.symbol_name.as_ref(), "CompanionOnly");

        let shadowed = crate::resolver_core::bare_name_resolve::resolve_bare_name_in_scope(
            ctx, canonical, instance, None, "Shared",
        )
        .expect("the exact setup declaration must win before companion lookup");
        assert_eq!(shadowed.owner, instance);

        assert!(
            crate::resolver_core::bare_name_resolve::resolve_bare_name_in_scope(
                ctx,
                canonical,
                module,
                None,
                "SetupOnly",
            )
            .is_none(),
            "module scope must never see instance declarations"
        );
    });
}

#[test]
fn sfc_setup_companion_owner_resolution_warms_and_invalidates_on_exact_header_edit() {
    let host = VerterHost::new_standalone(HostConfig::default());
    let canonical = "/src/OwnerChainEdit.vue";
    upsert(
        &host,
        canonical,
        r#"<script lang="ts">
type Before = { value: string }
</script>
<script setup lang="ts">
defineProps<Before>()
</script>"#,
    );

    let module = verter_type_expr::TopLevelOwnerId::ordinary_file();
    let instance = verter_type_expr::TopLevelOwnerId::instance(0);
    crate::resolver_core::with_bare_host_ctx_for_test(&host, |ctx| {
        for _ in 0..2 {
            let resolved = crate::resolver_core::bare_name_resolve::resolve_bare_name_in_scope(
                ctx, canonical, instance, None, "Before",
            )
            .expect("cold and warm lookup must retain the exact companion owner");
            assert_eq!(resolved.owner, module);
        }
    });

    upsert(
        &host,
        canonical,
        r#"<script lang="ts">
type After = { value: number }
</script>
<script setup lang="ts">
defineProps<After>()
</script>"#,
    );
    crate::resolver_core::with_bare_host_ctx_for_test(&host, |ctx| {
        assert!(
            crate::resolver_core::bare_name_resolve::resolve_bare_name_in_scope(
                ctx, canonical, instance, None, "Before",
            )
            .is_none(),
            "the removed companion header must not survive the edit"
        );
        let resolved = crate::resolver_core::bare_name_resolve::resolve_bare_name_in_scope(
            ctx, canonical, instance, None, "After",
        )
        .expect("the replacement companion header must resolve after invalidation");
        assert_eq!(resolved.owner, module);
    });
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
fn tsc_inference_partial_is_entry_scoped_and_complete_sibling_continues() {
    let host = VerterHost::new_standalone(HostConfig::default());
    let mut inferred = "0".to_owned();
    for _ in 0..80 {
        inferred = format!("[{inferred}]");
    }
    upsert(
        &host,
        "/src/InferenceBudgetSibling.vue",
        &format!(
            r#"<script setup lang="ts">
class Payload {{ method() {{ return {inferred} }} }}
defineProps<{{ payload: Payload }}>()
defineModel<string>('selected')
</script>"#
        ),
    );

    let output = produce(
        &host,
        "/src/InferenceBudgetSibling.vue",
        VueMacroCodegenDemand::Tsc,
    );
    let bundle = output.tsc.as_ref().expect("TSC bundle");
    assert_eq!(bundle.entries.len(), 2);

    let MacroTscOutcome::Complete(MacroTscProjection::Props(props)) = &bundle.entries[0].outcome
    else {
        panic!("budgeted props projection must retain its typed declaration: {bundle:?}");
    };
    let payload = props
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

    let MacroTscOutcome::Complete(MacroTscProjection::Model(model)) = &bundle.entries[1].outcome
    else {
        panic!("independent model projection must continue after a partial sibling: {bundle:?}");
    };
    assert_eq!(model.name, "selected");
    assert_eq!(model.value_type.as_str(), "string");
    assert!(output.completeness.is_partial());
    assert!(!output.facts_cacheable);
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
fn scheduler_key_is_content_free_but_input_pin_moves_across_edit() {
    let host = VerterHost::new_standalone(HostConfig::default());
    const FILE: &str = "/src/SchedulerIdentity.vue";
    upsert(
        &host,
        FILE,
        r#"<script setup lang="ts">defineProps<{ value: string }>()</script>"#,
    );
    let first = crate::resolver_core::with_bare_host_ctx_for_test(&host, |ctx| {
        crate::typeinfo::vue_macro_codegen::vue_macro_codegen_schedule_identity(
            ctx,
            FILE,
            VueMacroCodegenDemand::Runtime,
        )
    });

    upsert(
        &host,
        FILE,
        r#"<script setup lang="ts">defineProps<{ value: number }>()</script>"#,
    );
    let edited = crate::resolver_core::with_bare_host_ctx_for_test(&host, |ctx| {
        crate::typeinfo::vue_macro_codegen::vue_macro_codegen_schedule_identity(
            ctx,
            FILE,
            VueMacroCodegenDemand::Runtime,
        )
    });

    assert_eq!(
        first.key_hash, edited.key_hash,
        "content, version, graph, and node hashes must not enter the R6 semantic key"
    );
    assert_ne!(
        first.input_pin, edited.input_pin,
        "an edit must move the epoch/validity input pin even though the semantic key stays stable"
    );
}

#[test]
fn scheduler_identity_isolates_exact_demand_and_session() {
    use crate::resolver_core::StoreViewCompatToken;

    let base = StoreViewCompatToken {
        epoch: 7,
        session: Some(41),
        validity_fingerprint: 11,
    };
    let identity = |demand, compat| {
        crate::typeinfo::vue_macro_codegen::vue_macro_codegen_schedule_identity_from_compat(
            "/src/Isolation.vue",
            demand,
            compat,
        )
    };
    let runtime = identity(VueMacroCodegenDemand::Runtime, base);
    let tsc = identity(VueMacroCodegenDemand::Tsc, base);
    let both = identity(VueMacroCodegenDemand::RuntimeAndTsc, base);
    assert_ne!(runtime.key_hash, tsc.key_hash);
    assert_ne!(runtime.key_hash, both.key_hash);
    assert_ne!(tsc.key_hash, both.key_hash);
    assert_eq!(runtime.input_pin, tsc.input_pin);

    let other_session = identity(
        VueMacroCodegenDemand::Runtime,
        StoreViewCompatToken {
            session: Some(42),
            ..base
        },
    );
    assert_ne!(runtime.key_hash, other_session.key_hash);

    let overlay_edit = identity(
        VueMacroCodegenDemand::Runtime,
        StoreViewCompatToken {
            epoch: 8,
            validity_fingerprint: 12,
            ..base
        },
    );
    assert_eq!(runtime.key_hash, overlay_edit.key_hash);
    assert_ne!(runtime.input_pin, overlay_edit.input_pin);
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
    assert_eq!(output.counters.scheduler_submissions, 1);
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
fn runtime_props_degrade_only_a_direct_missing_member_dependency() {
    let host = VerterHost::new_standalone(HostConfig::default());
    upsert_ts(
        &host,
        "/src/member-types.ts",
        "export type ExternalUnknown = unknown",
    );
    upsert(
        &host,
        "/src/MemberDependency.vue",
        r#"<script setup lang="ts">
import type { Missing } from './missing'
import type { ExternalUnknown } from './member-types'
defineProps<{
  direct: Missing
  nested: { value: Missing }
  resolvedUnknown: ExternalUnknown
  honestUnknown: unknown
}>()
</script>"#,
    );

    let output = produce(
        &host,
        "/src/MemberDependency.vue",
        VueMacroCodegenDemand::Runtime,
    );
    let runtime = output.runtime.expect("runtime bundle");
    let MacroRuntimeOutcome::Complete(MacroRuntimeShape::Props(props)) =
        &runtime.entries[0].outcome
    else {
        panic!("a member dependency miss must preserve the complete prop surface: {runtime:?}");
    };
    assert_eq!(
        props
            .props
            .iter()
            .map(|prop| prop.name.as_str())
            .collect::<Vec<_>>(),
        ["direct", "nested", "resolvedUnknown", "honestUnknown"]
    );
    assert!(matches!(
        props.props[0].type_shape,
        RuntimePropType::Degraded(ref failure)
            if failure.reason
                == MacroMemberReason::Unresolved(UnresolvedReason::MissingDependency)
    ));
    assert_eq!(
        constructors(&props.props[1]),
        [RuntimeConstructor::Object],
        "nested references are outside runtime constructor demand"
    );
    assert!(
        constructors(&props.props[2]).is_empty(),
        "a resolved imported `unknown` is complete and must not masquerade as degradation"
    );
    assert!(
        constructors(&props.props[3]).is_empty(),
        "an authored `unknown` is complete and must not masquerade as degradation"
    );
}

/// Mutation recipe: route runtime props/emits through ordinary
/// `MacroObjectSurface`, or suppress every heritage arm once any directive is
/// present. The ignored names then leak into runtime (or the non-ignored names
/// disappear), while TSC no longer preserves the full TypeScript surface.
#[test]
fn vue_ignore_suppresses_only_selected_runtime_heritage_and_preserves_tsc() {
    let host = VerterHost::new_standalone(HostConfig::default());
    upsert(
        &host,
        "/src/VueIgnore.vue",
        r#"<script setup lang="ts">
interface IgnoredProps { ignored: string }
interface KeptProps { kept: boolean }
interface Props extends /* @vue-ignore */ IgnoredProps, KeptProps { own: number }

interface IgnoredEmits { ignoredEvent: [value: string] }
interface KeptEmits { keptEvent: [value: boolean] }
interface Emits extends /* @vue-ignore */ IgnoredEmits, KeptEmits { ownEvent: [] }

defineProps<Props>()
defineEmits<Emits>()
</script>"#,
    );

    let output = produce(
        &host,
        "/src/VueIgnore.vue",
        VueMacroCodegenDemand::RuntimeAndTsc,
    );
    assert_eq!(
        output.completeness,
        crate::semantic_query::ResultCompleteness::Complete
    );

    let runtime = output.runtime.as_ref().expect("runtime bundle");
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
        ["kept", "own"]
    );
    let MacroRuntimeOutcome::Complete(MacroRuntimeShape::Emits(emits)) =
        &runtime.entries[1].outcome
    else {
        panic!("expected complete emits runtime shape: {runtime:?}");
    };
    assert_eq!(
        emits
            .iter()
            .map(|event| event.name.as_str())
            .collect::<Vec<_>>(),
        ["keptEvent", "ownEvent"]
    );

    let tsc = output.tsc.as_ref().expect("TSC bundle");
    let MacroTscOutcome::Complete(MacroTscProjection::Props(tsc_props)) = &tsc.entries[0].outcome
    else {
        panic!("expected complete props TSC projection: {tsc:?}");
    };
    let mut tsc_prop_names: Vec<_> = tsc_props
        .testing_rows
        .iter()
        .map(|row| row.name.as_str())
        .collect();
    tsc_prop_names.sort_unstable();
    assert_eq!(tsc_prop_names, ["ignored", "kept", "own"]);
    let MacroTscOutcome::Complete(MacroTscProjection::Emits(tsc_emits)) = &tsc.entries[1].outcome
    else {
        panic!("expected complete emits TSC projection: {tsc:?}");
    };
    let mut tsc_emit_names: Vec<_> = tsc_emits
        .events
        .iter()
        .map(|event| event.name.as_str())
        .collect();
    tsc_emit_names.sort_unstable();
    assert_eq!(tsc_emit_names, ["ignoredEvent", "keptEvent", "ownEvent"]);
}

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
        [("save", "value: Local"), ("cancel", "reason?: string")]
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
