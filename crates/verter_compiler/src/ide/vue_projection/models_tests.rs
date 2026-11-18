//! Unit tests for the models module.

use verter_audit::observe::Uncaptured;

use super::attribute_operations::project_attribute_operations;
use super::binding_views::{project_binding_views, WriteDomain, WriteRejection};
use super::branches::{project_template_branches, TemplateBranchProjection, TemplateItem};
use super::callback_reentry::project_callback_reentry;
use super::checking_fragment::{
    ObligationFamily, PlacedStatement, StatementAnchor, StatementPhase, UseHost,
};
use super::checking_module::setup_binding_kinds;
use super::component_use::{project_component_uses, ComponentUseProjection};
use super::models::*;
use super::script_setup::ScriptBlockInput;
use crate::framework_common::projection_plan::{build_projection_plan, PlanInput, ProjectionPlan};
use crate::framework_common::vue_bridge::parse_sfc;
use crate::template::topology::{build_template_topology, TopologyRecoveryFacts};
use crate::work_ledger::CompilerWorkLedger;

const SETUP: &str = concat!(
    "import Child from './Child.vue';\n",
    "import { queryField } from './query';\n",
    "import { computed, ref } from 'vue';\n",
    "const search = queryField('q', '');\n",
    "const text = ref('');\n",
    "const user = ref<{ name: string } | null>(null);\n",
    "const total = computed(() => 1);\n",
    "const frozen = 1;\n",
);

struct Carrier {
    source: String,
    plan: ProjectionPlan,
    branches: TemplateBranchProjection,
    uses: ComponentUseProjection,
    models: ModelProjection,
}

fn carrier(template: &str) -> Carrier {
    carrier_over(SETUP, template)
}

fn carrier_over(setup: &str, template: &str) -> Carrier {
    let source = format!(
        "<script setup lang=\"ts\">\n{setup}</script>\n<template>\n{template}</template>\n"
    );
    let parsed = parse_sfc(&source, None, None);
    let plan = build_projection_plan(PlanInput {
        canonical_id: "file:///src/Models.vue",
        source: &source,
        parsed: &parsed,
        parse_key: None,
        syntax_profile: None,
        custom_elements: None,
    });
    let ast = parsed.template_ast().expect("the carrier has a template");
    let topology = build_template_topology(
        &source,
        ast,
        TopologyRecoveryFacts::default(),
        &mut CompilerWorkLedger::<Uncaptured>::new(),
    )
    .expect("the topology builds");
    let branches = project_template_branches(&plan, &topology);
    let reentry = project_callback_reentry(&plan, &branches);
    let attributes = project_attribute_operations(&plan, &parsed, &source);
    let uses = project_component_uses(&plan, &attributes);
    let setup = parsed.script_setup().expect("a setup block");
    let content = setup.content.expect("setup content");
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
    .expect("views project");
    let models = project_models(&plan, &branches, &uses, &views, &reentry);
    Carrier {
        source,
        plan,
        branches,
        uses,
        models,
    }
}

impl Carrier {
    /// The statement text of every placed statement, expressions spelled.
    fn texts(&self) -> Vec<String> {
        self.models
            .statements()
            .iter()
            .map(|statement| {
                statement.fragment.flatten(&self.source, &|id| {
                    self.plan.expression(id).expect("admitted").spelling.clone()
                })
            })
            .collect()
    }

    /// The witness binding of the `index`th use.
    fn witness(&self, index: usize) -> &str {
        &self.uses.witnesses[index].binding
    }

    fn payload(&self, index: usize, key: &str) -> String {
        format!(
            "__VerterUseModel<typeof {}, \"{key}\">",
            self.witness(index)
        )
    }
}

#[test]
fn a_model_on_a_const_holding_a_call_result_is_written_through_the_context() {
    let carrier = carrier("  <Child v-model=\"search\" />\n");
    assert!(carrier.models.complete);
    assert!(!carrier.models.pending);
    assert_eq!(carrier.models.writes.len(), 1);
    assert_eq!(
        carrier.models.writes[0].target_kind,
        ModelTarget::Binding {
            name: "search".to_string(),
            domain: WriteDomain::RefValue,
        },
        "the write goes through the ref, never to the const"
    );
    assert_eq!(
        carrier.texts(),
        vec![format!(
            "(() => {{\nsearch = null! as {};\n}});\n",
            carrier.payload(0, "onUpdate:modelValue")
        )]
    );
}

#[test]
fn the_write_carries_the_payload_of_the_witness_and_sits_after_the_construction() {
    let carrier = carrier("  <Child v-model=\"text\" />\n");
    let write = &carrier.models.writes[0];
    assert_eq!(
        write.payload_type,
        carrier.payload(0, "onUpdate:modelValue")
    );
    assert_eq!(write.model, "modelValue");
    let place = carrier
        .branches
        .place_of(&TemplateItem::Component(write.use_id.clone()))
        .expect("the use is placed");
    let statements = carrier.models.statements();
    assert_eq!(
        statements,
        vec![PlacedStatement {
            anchor: StatementAnchor::Item {
                block: place.0,
                item: place.1,
            },
            phase: StatementPhase::After,
            family: ObligationFamily::Model,
            host: Some(UseHost::Component(write.use_id.clone())),
            fragment: statements[0].fragment.clone(),
        }]
    );
    assert_eq!(carrier.models.inputs.len(), 1);
    assert_eq!(carrier.models.inputs[0].value, write.target);
    assert_eq!(carrier.models.preludes(), vec![MODEL_PRELUDE]);
}

#[test]
fn a_target_the_views_refuse_is_assigned_through_a_readonly_view() {
    let carrier = carrier(concat!(
        "  <Child v-model=\"total\" />\n",
        "  <Child v-model=\"frozen\" />\n",
    ));
    let targets: Vec<&ModelTarget> = carrier
        .models
        .writes
        .iter()
        .map(|write| &write.target_kind)
        .collect();
    assert_eq!(
        targets,
        vec![
            &ModelTarget::Refused {
                name: "total".to_string(),
                rejection: WriteRejection::GetterOnlyComputed,
            },
            &ModelTarget::Refused {
                name: "frozen".to_string(),
                rejection: WriteRejection::ConstBinding,
            },
        ]
    );
    let texts = carrier.texts();
    assert_eq!(
        texts[0],
        format!(
            "__VerterModelWrite(__VerterCtx, (__VerterCtx) => {{\ntotal = null! as {};\n}});\n",
            carrier.payload(0, "onUpdate:modelValue")
        )
    );
    assert!(texts[1].starts_with("__VerterModelWrite(__VerterCtx, (__VerterCtx) => {\nfrozen = "));
}

#[test]
fn a_member_path_is_an_assignable_expression() {
    let carrier = carrier("  <Child v-model=\"user.name\" />\n");
    assert_eq!(carrier.models.writes[0].target_kind, ModelTarget::Path);
    assert!(carrier.texts()[0].starts_with("(() => {\nuser.name = null! as "));
}

#[test]
fn only_an_asserted_target_is_written_inside_parentheses() {
    let carrier = carrier(concat!(
        "  <Child v-model=\"user.name as string\" />\n",
        "  <Child v-model=\"user.name satisfies string\" />\n",
        "  <Child v-model=\"(user.name as string)\" />\n",
        "  <Child v-model=\"user.name!\" />\n",
    ));
    let texts = carrier.texts();
    assert!(texts[0].starts_with("(() => {\n(user.name as string) = null! as "));
    assert!(texts[1].starts_with("(() => {\n(user.name satisfies string) = null! as "));
    assert!(texts[2].starts_with("(() => {\n(user.name as string) = null! as "));
    assert!(texts[3].starts_with("(() => {\nuser.name! = null! as "));
}

#[test]
fn a_name_the_views_do_not_hold_is_left_to_the_type_checker() {
    let carrier = carrier("  <Child v-for=\"item in [1]\" v-model=\"item\" />\n");
    assert_eq!(carrier.models.writes[0].target_kind, ModelTarget::Path);
}

#[test]
fn a_model_update_opens_with_the_guards_that_dominate_it() {
    let carrier = carrier("  <Child v-if=\"user\" v-model=\"user.name\" />\n");
    assert_eq!(
        carrier.texts(),
        vec![format!(
            "(() => {{\nif (!(user)) throw undefined;\nuser.name = null! as {};\n}});\n",
            carrier.payload(0, "onUpdate:modelValue")
        )]
    );
}

#[test]
fn each_named_model_writes_its_own_payload() {
    let carrier = carrier("  <Child v-model:first=\"search\" v-model:second-name=\"text\" />\n");
    let writes: Vec<(&str, &str)> = carrier
        .models
        .writes
        .iter()
        .map(|write| (write.model.as_str(), write.payload_type.as_str()))
        .collect();
    let first = carrier.payload(0, "onUpdate:first");
    let second = carrier.payload(0, "onUpdate:secondName");
    assert_eq!(
        writes,
        vec![("first", first.as_str()), ("secondName", second.as_str())]
    );
    let texts = carrier.texts();
    assert_eq!(
        texts[0],
        format!("(() => {{\nsearch = null! as {first};\n}});\n")
    );
    assert_eq!(
        texts[1],
        format!("(() => {{\ntext = null! as {second};\n}});\n")
    );
}

#[test]
fn declared_modifier_keys_are_checked_as_literals_under_the_runtime_key() {
    let carrier = carrier(concat!(
        "  <Child v-model.trim.number=\"search\" />\n",
        "  <Child v-model:title.capitalize=\"text\" />\n",
    ));
    assert_eq!(carrier.models.modifiers.len(), 2);
    assert_eq!(carrier.models.modifiers[0].modifiers, ["trim", "number"]);
    assert_eq!(carrier.models.modifiers[1].model, "title");
    let texts = carrier.texts();
    assert_eq!(
        texts[1],
        format!(
            "__VerterModelModifiers<__VerterUseProp<typeof {}, \"modelModifiers\">>({{ \"trim\": true, \"number\": true }});\n",
            carrier.witness(0)
        )
    );
    assert_eq!(
        texts[3],
        format!(
            "__VerterModelModifiers<__VerterUseProp<typeof {}, \"titleModifiers\">>({{ \"capitalize\": true }});\n",
            carrier.witness(1)
        )
    );
}

#[test]
fn a_model_without_modifiers_declares_no_modifier_contract() {
    let carrier = carrier("  <Child v-model=\"search\" />\n");
    assert!(carrier.models.modifiers.is_empty());
    assert_eq!(carrier.texts().len(), 1);
}

#[test]
fn a_computed_argument_names_no_model() {
    let carrier = carrier("  <Child v-model:[which]=\"search\" />\n");
    assert!(carrier.models.writes.is_empty());
    assert!(carrier.models.statements().is_empty());
    assert!(carrier.models.preludes().is_empty());
    assert!(carrier.models.complete);
}

#[test]
fn a_template_without_models_contributes_nothing() {
    let carrier = carrier("  <Child :label=\"search\" />\n");
    assert!(carrier.models.complete);
    assert!(carrier.models.writes.is_empty());
    assert!(carrier.models.preludes().is_empty());
}

#[test]
fn the_pending_product_contributes_nothing() {
    let snapshot = carrier("  <Child />\n").plan.snapshot.clone();
    let pending = ModelProjection::pending(snapshot);
    assert!(pending.pending && !pending.complete);
    assert!(pending.statements().is_empty());
    assert!(pending.preludes().is_empty());
}

#[test]
fn a_writable_computed_is_checked_against_its_setter_domain() {
    let setup = SETUP.replace(
        "const total = computed(() => 1);",
        "const total = computed({ get: () => 1, set: (v: string) => {} });\nconst loose = computed({ get: () => 1, set: (v) => {} });",
    );
    let carrier = carrier_over(
        &setup,
        "  <Child v-model=\"total\" />\n  <Child v-model=\"loose\" />\n",
    );
    assert_eq!(carrier.models.writes.len(), 2);
    assert_eq!(
        carrier.texts(),
        vec![format!(
            "(() => {{\nnull! as {} satisfies string;\n}});\n",
            carrier.payload(0, "onUpdate:modelValue")
        )],
        "an unannotated setter declares no domain to check"
    );
}
