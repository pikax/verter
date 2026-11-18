//! Unit tests for the native models module.

use super::binding_views::{project_binding_views, WriteDomain};
use super::branches::project_template_branches;
use super::callback_reentry::project_callback_reentry;
use super::checking_module::setup_binding_kinds;
use super::models::ModelTarget;
use super::native_elements::project_native_elements;
use super::native_models::*;
use super::script_setup::ScriptBlockInput;
use crate::framework_common::projection_plan::{build_projection_plan, PlanInput, ProjectionPlan};
use crate::framework_common::vue_bridge::parse_sfc;
use crate::template::topology::{build_template_topology, TopologyRecoveryFacts};
use crate::work_ledger::CompilerWorkLedger;
use verter_audit::observe::Uncaptured;

fn project(template: &str) -> (String, ProjectionPlan, NativeModelProjection) {
    let source = format!(
        "<script setup lang=\"ts\">import {{ queryField }} from './query'; import {{ ref, computed }} from 'vue'; const search = queryField('q', ''); const state = ref({{ text: '' }}); const total = computed(() => 1);</script><template>{template}</template>"
    );
    let parsed = parse_sfc(&source, None, None);
    let plan = build_projection_plan(PlanInput {
        canonical_id: "file:///NativeModel.vue",
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
    let native = project_native_elements(&plan, &branches, &reentry, None);
    let setup = parsed.script_setup().unwrap();
    let content = setup.content.unwrap();
    let views = project_binding_views(
        None,
        Some(ScriptBlockInput {
            content: &source[content.start as usize..content.end as usize],
            content_start: content.start,
            lang: setup.lang,
        }),
        None,
        &setup_binding_kinds(&source, &parsed),
    )
    .unwrap();
    let models = project_native_models(&plan, &branches, &native, &views);
    (source, plan, models)
}

#[test]
fn native_input_call_result_uses_the_shared_ref_write_domain() {
    let (_, _, models) = project("<input v-model=\"search\" />");
    assert!(models.complete && !models.pending);
    assert_eq!(models.contracts.len(), 1);
    assert_eq!(models.contracts[0].control, FormControl::Text);
    assert!(matches!(&models.obligations[0].target_kind,
        ModelTarget::Binding { name, domain: WriteDomain::RefValue } if name == "search"));
    assert!(!models.statements().is_empty());
}

fn text(source: &str, plan: &ProjectionPlan, models: &NativeModelProjection) -> String {
    let body: String = models
        .statements()
        .iter()
        .map(|statement| {
            statement
                .fragment
                .flatten(source, &|id| plan.expression(id).unwrap().spelling.clone())
        })
        .collect();
    let prelude: String = models
        .preludes()
        .iter()
        .map(|prelude| prelude.text)
        .collect();
    let output = format!("{prelude}{body}");
    let alloc = oxc_allocator::Allocator::default();
    let parsed =
        verter_parser::oxc_parse::Parser::new(&alloc, &output, oxc_span::SourceType::ts()).parse();
    assert!(
        parsed.diagnostics.is_empty(),
        "{:?}\n{output}",
        parsed.diagnostics
    );
    body
}

#[test]
fn form_controls_keep_collection_scalar_and_bound_value_contracts() {
    let template = concat!(
        "<input v-model=\"search\" /><textarea v-model=\"search\" />",
        "<select v-model=\"search\" /><select multiple v-model=\"search\" />",
        "<input type=\"checkbox\" :value=\"state\" :true-value=\"'yes'\" :false-value=\"'no'\" v-model=\"search\" />",
        "<input type=\"radio\" value=\"a&amp;b\" v-model=\"search\" />"
    );
    let (source, plan, models) = project(template);
    assert!(models.complete && !models.pending);
    assert_eq!(
        models
            .contracts
            .iter()
            .map(|c| c.control)
            .collect::<Vec<_>>(),
        [
            FormControl::Text,
            FormControl::Textarea,
            FormControl::SelectSingle,
            FormControl::SelectMultiple,
            FormControl::Checkbox,
            FormControl::Radio,
        ]
    );
    let checkbox = &models.contracts[4];
    assert_eq!(
        plan.expression(checkbox.true_value.as_ref().unwrap())
            .unwrap()
            .spelling,
        "'yes'"
    );
    assert_eq!(
        plan.expression(checkbox.false_value.as_ref().unwrap())
            .unwrap()
            .spelling,
        "'no'"
    );
    let output = text(&source, &plan, &models);
    assert!(
        output.contains("\"checkbox\", (search), (state), ('yes'), ('no')"),
        "{output}"
    );
    assert!(output.contains("\"radio\", (search), \"a&b\""), "{output}");
    assert_eq!(models.obligations.len(), 6);
}

#[test]
fn static_control_types_reuse_the_native_attribute_decoder() {
    let (source, plan, models) = project("<input type=\"check&#98;ox\" v-model=\"search\" />");
    assert!(models.complete);
    assert_eq!(models.contracts[0].control, FormControl::Checkbox);
    assert!(text(&source, &plan, &models).contains("\"checkbox\""));
}

#[test]
fn number_dynamic_type_trim_and_lazy_preserve_every_possible_write() {
    let (source, plan, models) = project(concat!(
        "<input v-model.number.trim.lazy=\"search\" />",
        "<input type=\"number\" v-model=\"search\" />",
        "<input :type=\"state.text\" v-model=\"search\" />",
        "<select :multiple=\"state.text\" v-model.number=\"search\" />"
    ));
    assert!(models.complete);
    assert_eq!(
        models.contracts[0].modifiers,
        NativeModelModifiers {
            number: true,
            trim: true,
            lazy: true
        }
    );
    assert_eq!(models.contracts[2].control, FormControl::DynamicInput);
    let output = text(&source, &plan, &models);
    assert!(output.contains("\"dynamic-input\""), "{output}");
    assert!(
        output.contains("number: true, trim: true, lazy: true"),
        "{output}"
    );
    assert!(output.contains("multiple: (state.text)"), "{output}");
    assert!(NATIVE_MODEL_PRELUDE.text.contains("number | string"));
}

#[test]
fn readonly_targets_and_nested_paths_use_shared_writes_and_guard_replay() {
    let (source, plan, models) = project(concat!(
        "<input v-model=\"total\" />",
        "<input v-if=\"state\" v-model=\"state.text\" />"
    ));
    assert!(models.complete);
    assert!(matches!(
        models.obligations[0].target_kind,
        ModelTarget::Refused { .. }
    ));
    assert_eq!(models.obligations[1].target_kind, ModelTarget::Path);
    let output = text(&source, &plan, &models);
    assert!(output.contains("__VerterModelWrite("), "{output}");
    assert!(
        output.contains("if (!(state)) throw undefined;"),
        "{output}"
    );
    assert!(output.contains("state.text = null! as"), "{output}");
}

#[test]
fn no_models_and_pending_products_contribute_nothing() {
    let (_, _, models) = project("<div />");
    assert!(models.complete && !models.pending);
    assert!(models.statements().is_empty());
    assert!(models.preludes().is_empty());
    let pending = NativeModelProjection::pending(models.snapshot);
    assert!(pending.statements().is_empty());
    assert!(pending.preludes().is_empty());
}

#[test]
fn opaque_attribute_writes_do_not_fabricate_default_model_payloads() {
    let (_, _, models) = project("<input v-bind=\"state\" v-model=\"search\" />");
    assert!(!models.pending);
    assert!(!models.complete);
    assert!(models.contracts.is_empty());
    assert!(models.statements().is_empty());
}

#[test]
fn forced_dom_routes_do_not_claim_the_ordinary_bound_model_payload() {
    for (control, attribute) in [
        ("radio", ":value.attr=\"42\""),
        ("checkbox", ":true-value.prop=\"42\""),
    ] {
        let (_, _, models) = project(&format!(
            "<input type=\"{control}\" {attribute} v-model=\"search\" />"
        ));
        assert!(
            !models.complete,
            "forced routing needs its actual DOM payload"
        );
        assert!(!models.pending);
        assert!(models.contracts.is_empty());
        assert!(models.statements().is_empty());
    }
    let (_, _, ordinary) = project("<input :disabled.attr=\"true\" v-model=\"search\" />");
    assert!(
        ordinary.complete,
        "unrelated routed attributes do not change the model payload"
    );
}
