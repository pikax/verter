//! Unit tests for the public instance module.

use super::attribute_operations::project_attribute_operations;
use super::branches::project_template_branches;
use super::callback_reentry::project_callback_reentry;
use super::component_use::{project_component_uses, ComponentUseProjection, ObservationKind};
use super::native_elements::project_native_elements;
use super::public_constructor::project_public_constructor;
use super::public_instance::*;
use super::script_setup::{project_script_pair, ScriptBlockInput};
use crate::cursor::ScriptLanguage;
use crate::framework_common::projection_plan::{build_projection_plan, PlanInput, ProjectionPlan};
use crate::framework_common::vue_bridge::parse_sfc;
use crate::template::topology::{build_template_topology, TopologyRecoveryFacts};
use crate::work_ledger::CompilerWorkLedger;
use verter_audit::observe::Uncaptured;

fn project(
    setup: &str,
    template: &str,
) -> (
    String,
    ProjectionPlan,
    ComponentUseProjection,
    PublicInstanceRefProjection,
) {
    let opening = "<script setup lang=\"ts\">";
    let source = format!("{opening}{setup}</script><template>{template}</template>");
    let parsed = parse_sfc(&source, None, None);
    let plan = build_projection_plan(PlanInput {
        canonical_id: "file:///Parent.vue",
        source: &source,
        parsed: &parsed,
        parse_key: None,
        syntax_profile: None,
        custom_elements: None,
    });
    let topology = build_template_topology(
        &source,
        parsed.template_ast().unwrap(),
        TopologyRecoveryFacts::default(),
        &mut CompilerWorkLedger::<Uncaptured>::new(),
    )
    .unwrap();
    let branches = project_template_branches(&plan, &topology);
    let reentry = project_callback_reentry(&plan, &branches);
    let attributes = project_attribute_operations(&plan, &parsed, &source);
    let uses = project_component_uses(&plan, &attributes);
    let native = project_native_elements(&plan, &branches, &reentry, None);
    let block = ScriptBlockInput {
        content: setup,
        content_start: opening.len() as u32,
        lang: Some(ScriptLanguage::TypeScript),
    };
    let script = project_script_pair(None, Some(block), None).unwrap();
    let public = project_public_constructor(None, Some(block), None).unwrap();
    let refs = project_public_instance(&plan, &branches, &uses, &native, &public, &script);
    (source, plan, uses, refs)
}

fn text(source: &str, plan: &ProjectionPlan, refs: &PublicInstanceRefProjection) -> String {
    let statements: String = refs
        .statements()
        .iter()
        .map(|statement| {
            statement
                .fragment
                .flatten(source, &|id| plan.expression(id).unwrap().spelling.clone())
        })
        .collect();
    let output = format!("{}{statements}", TEMPLATE_REF_PRELUDE.text);
    let alloc = oxc_allocator::Allocator::default();
    let parsed =
        verter_parser::oxc_parse::Parser::new(&alloc, &output, oxc_span::SourceType::ts()).parse();
    assert!(
        parsed.diagnostics.is_empty(),
        "{:?}\n{output}",
        parsed.diagnostics
    );
    statements
}

#[test]
fn exposed_members_and_aliased_component_refs_use_the_public_instance() {
    let (source, plan, uses, refs) = project(
        "import Child from './Child.vue'; import { ref, useTemplateRef as templateRef } from 'vue'; const direct = ref<InstanceType<typeof Child> | null>(null); const target = templateRef<InstanceType<typeof Child>>('panel'); const hidden = 1; function focus() {} defineExpose({ focus });",
        "<Child ref=\"direct\" /><Child ref=\"panel\" />",
    );
    assert!(refs.complete);
    assert!(!refs.pending);
    assert_eq!(refs.exposed.members, ["focus"]);
    assert!(!refs.exposed.open);
    assert_eq!(
        refs.refs
            .iter()
            .map(|r| r.binding.as_deref())
            .collect::<Vec<_>>(),
        [Some("direct"), Some("target")]
    );
    let text = text(&source, &plan, &refs);
    for witness in &uses.witnesses {
        let instance = witness
            .observations
            .iter()
            .find(|o| o.kind == ObservationKind::Instance)
            .unwrap();
        assert!(
            text.contains(&format!("({}) | null", instance.type_text)),
            "{text}"
        );
    }
    assert!(text.contains("const __VerterRef_1 = target;"), "{text}");
}

#[test]
fn conditional_and_loop_refs_keep_nullable_specific_element_types() {
    let (source, plan, _, refs) = project(
        "import { ref } from 'vue'; const input = ref<HTMLInputElement | null>(null); const circles = ref<SVGCircleElement[] | null>(null); const visible = true; const items = [1];",
        "<input v-if=\"visible\" ref=\"input\" /><svg><circle v-for=\"item in items\" ref=\"circles\" /></svg>",
    );
    assert!(refs.complete);
    assert!(refs.refs[0].conditional);
    assert!(!refs.refs[0].in_loop);
    assert!(refs.refs[1].in_loop);
    assert!(refs.policy.nullable_always && refs.policy.loop_collects);
    let text = text(&source, &plan, &refs);
    assert!(
        text.contains("(__VerterNativeElement<\"input\", \"html\">) | null"),
        "{text}"
    );
    assert!(
        text.contains("(__VerterNativeElement<\"circle\", \"svg\">)[] | null"),
        "{text}"
    );
}

#[test]
fn generic_component_refs_observe_each_uses_own_specialization() {
    let (source, plan, uses, refs) = project(
        "import Generic from './Generic.vue'; import { ref } from 'vue'; const first = ref<unknown>(null); const second = ref<unknown>(null); const numbers = [1]; const words = ['a'];",
        "<Generic :items=\"numbers\" ref=\"first\" /><Generic :items=\"words\" ref=\"second\" />",
    );
    assert!(refs.complete);
    assert_eq!(refs.refs.len(), 2);
    assert_ne!(
        uses.witnesses[0].specialization,
        uses.witnesses[1].specialization
    );
    let statements = refs.statements();
    for (statement, witness) in statements
        .iter()
        .filter(|statement| statement.host.is_some())
        .zip(&uses.witnesses)
    {
        let instance = witness
            .observations
            .iter()
            .find(|o| o.kind == ObservationKind::Instance)
            .unwrap();
        let output = statement
            .fragment
            .flatten(&source, &|id| plan.expression(id).unwrap().spelling.clone());
        assert!(output.contains(&instance.type_text), "{output}");
    }
    text(&source, &plan, &refs);
}

#[test]
fn function_refs_delegate_native_checks_and_check_component_instances() {
    let (source, plan, _, refs) = project(
        "function accept(el: HTMLInputElement | null) {}",
        "<input :ref=\"el => accept(el)\" />",
    );
    assert!(refs.complete);
    assert!(matches!(refs.refs[0].syntax, RefSyntax::Function(_)));
    assert!(refs.statements().is_empty());
    assert!(refs.preludes().is_empty());
    text(&source, &plan, &refs);
    let (source, plan, _, refs) = project("import Child from './Child.vue'; function accept(el: InstanceType<typeof Child> | null) {}", "<Child :ref=\"el => accept(el)\" />");
    assert!(refs.complete);
    assert_eq!(refs.statements().len(), 1);
    assert!(text(&source, &plan, &refs).contains("__VerterFunctionRef<typeof __VerterUse_"));
}

#[test]
fn static_receivers_are_captured_before_template_aliases_and_error_values_are_mapped() {
    let (source, plan, _, refs) = project(
        "import { useTemplateRef } from 'vue'; const target = useTemplateRef<HTMLInputElement[]>('rows'); const items = [1];",
        "<input v-for=\"target in items\" ref=\"rows\" />",
    );
    assert!(refs.complete);
    let statements = refs.statements();
    assert!(matches!(
        statements[0].anchor,
        super::checking_fragment::StatementAnchor::Item {
            block: super::checking_fragment::TemplateBlockId(0),
            item: 0
        }
    ));
    assert!(statements[0].host.is_none());
    let output = text(&source, &plan, &refs);
    assert!(output.contains("const __VerterRef_0 = target;"), "{output}");
    assert!(
        output.contains("__VerterTemplateRef(__VerterRef_0.value,"),
        "{output}"
    );
    let error_value = statements[1]
        .fragment
        .pieces
        .iter()
        .find_map(|piece| match piece {
            super::checking_fragment::CheckingPiece::Alias { text, source }
                if text.starts_with("null! as") =>
            {
                Some(source)
            }
            _ => None,
        })
        .expect("the argument receiving assignability errors has an authored origin");
    assert_eq!(
        &source[error_value.start as usize..error_value.end as usize],
        "'rows'"
    );
}
