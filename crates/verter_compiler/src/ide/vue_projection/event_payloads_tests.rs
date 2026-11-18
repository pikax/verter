//! Unit tests for the event payloads module.

use verter_audit::observe::Uncaptured;

use super::attribute_operations::project_attribute_operations;
use super::branches::{project_template_branches, TemplateBranchProjection};
use super::callback_reentry::project_callback_reentry;
use super::checking_fragment::{CheckingPiece, ObligationFamily, StatementPhase, UseHost};
use super::component_use::{project_component_uses, ComponentUseProjection};
use super::event_keys::project_event_transport;
use super::event_payloads::*;
use crate::framework_common::projection_plan::{
    build_projection_plan, HandlerShape, PlanInput, ProjectionPlan,
};
use crate::framework_common::vue_bridge::parse_sfc;
use crate::template::topology::{build_template_topology, TopologyRecoveryFacts};
use crate::work_ledger::CompilerWorkLedger;

struct Carrier {
    source: String,
    plan: ProjectionPlan,
    uses: ComponentUseProjection,
    projection: EventPayloadProjection,
}

fn carrier(template: &str) -> Carrier {
    let source = format!(
        "<script setup lang=\"ts\">\nimport Child from './Child.vue';\n</script>\n<template>\n{template}\n</template>\n"
    );
    let parsed = parse_sfc(&source, None, None);
    let plan = build_projection_plan(PlanInput {
        canonical_id: "file:///src/Payloads.vue",
        source: &source,
        parsed: &parsed,
        parse_key: None,
        syntax_profile: None,
        custom_elements: None,
    });
    let attributes = project_attribute_operations(&plan, &parsed, &source);
    let uses = project_component_uses(&plan, &attributes);
    let transport = project_event_transport(&plan, &attributes);
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
    let projection = project_event_payloads(&plan, &branches, &uses, &transport, &reentry);
    Carrier {
        source,
        plan,
        uses,
        projection,
    }
}

impl Carrier {
    fn binding(&self) -> &str {
        &self.uses.witnesses[0].binding
    }

    fn contract(&self, key: &str) -> String {
        format!("__VerterUseListener<typeof {}, \"{key}\">", self.binding())
    }

    fn keys(&self) -> Vec<(&str, &ListenerRoute)> {
        self.projection
            .obligations
            .iter()
            .map(|obligation| (obligation.listener_key.as_str(), &obligation.route))
            .collect()
    }
}

#[test]
fn a_named_listener_is_one_obligation_read_against_the_whole_contract() {
    let carrier = carrier("  <Child @save=\"onSave\" />");
    let projection = &carrier.projection;
    assert!(projection.complete);
    assert!(!projection.pending);
    assert_eq!(projection.obligations.len(), 1);
    let obligation = &projection.obligations[0];
    assert_eq!(obligation.listener_key, "onSave");
    assert_eq!(obligation.route, ListenerRoute::Construction);
    assert_eq!(obligation.shape, Some(HandlerShape::Reference));
    assert!(matches!(obligation.host, UseHost::Component(_)));
    assert_eq!(projection.relations.len(), 1);
    let relation = &projection.relations[0];
    assert_eq!(relation.event, "save");
    assert_eq!(relation.listener_key, "onSave");
    assert_eq!(relation.contract_type, carrier.contract("onSave"));
}

#[test]
fn a_relation_never_narrows_the_contract_to_one_signature() {
    let carrier = carrier(
        "  <Child @save=\"onSave\" v-on:save=\"onOther\" @close.once=\"onClose\" />\n  <Child @change=\"onChange\" />",
    );
    assert!(!carrier.projection.relations.is_empty());
    for relation in &carrier.projection.relations {
        assert!(
            relation
                .contract_type
                .starts_with("__VerterUseListener<typeof "),
            "{}",
            relation.contract_type
        );
        assert!(!relation.contract_type.contains("Parameters<"));
    }
}

#[test]
fn accumulated_listeners_are_each_an_obligation_on_their_own_value() {
    let carrier = carrier("  <Child @save=\"first\" v-on:save=\"second\" @save=\"third\" />");
    assert_eq!(
        carrier.keys(),
        vec![
            ("onSave", &ListenerRoute::Construction),
            ("onSave", &ListenerRoute::Validation),
            ("onSave", &ListenerRoute::Validation),
        ]
    );
    let values: Vec<_> = carrier
        .projection
        .obligations
        .iter()
        .map(|obligation| &obligation.value)
        .collect();
    assert_ne!(values[0], values[1]);
    assert_ne!(values[1], values[2]);
    assert_eq!(
        carrier.projection.relations.len(),
        1,
        "one contract for the key"
    );
}

#[test]
fn listeners_that_differ_only_in_key_modifiers_are_all_checked() {
    let carrier =
        carrier("  <Child @keydown.enter.prevent=\"submit\" @keydown.space.prevent=\"toggle\" />");
    assert_eq!(
        carrier.keys(),
        vec![
            ("onKeydown", &ListenerRoute::Construction),
            ("onKeydown", &ListenerRoute::Validation),
        ]
    );
    let constructed = carrier.uses.witnesses[0]
        .transaction
        .members
        .iter()
        .filter(|member| member.key() == Some("onKeydown"))
        .count();
    assert_eq!(constructed, 1, "the key reaches the construction once");
}

#[test]
fn event_option_modifiers_select_their_own_listener_key() {
    let carrier = carrier("  <Child @save=\"first\" @save.once=\"second\" />");
    let keys: Vec<_> = carrier.keys().into_iter().map(|(key, _)| key).collect();
    assert_eq!(keys, ["onSave", "onSaveOnce"]);
    let relations: Vec<_> = carrier
        .projection
        .relations
        .iter()
        .map(|relation| (relation.event.as_str(), relation.listener_key.as_str()))
        .collect();
    assert_eq!(relations, [("save", "onSave"), ("save", "onSaveOnce")]);
}

#[test]
fn an_inline_handler_observes_the_event_it_is_checked_in() {
    let carrier = carrier("  <Child @save=\"total += $event\" @close=\"onClose\" />");
    let projection = &carrier.projection;
    let inline = projection
        .obligations
        .iter()
        .find(|obligation| obligation.listener_key == "onSave")
        .expect("the inline listener");
    assert_eq!(inline.shape, Some(HandlerShape::Inline));
    assert_eq!(inline.route, ListenerRoute::Validation);
    assert_eq!(projection.contexts.len(), 1, "a reference has no context");
    let context = &projection.contexts[0];
    assert_eq!(context.value, inline.value);
    assert_eq!(
        context.event_type,
        format!("__VerterListenerEvent<{}>", carrier.contract("onSave"))
    );
    assert_eq!(projection.preludes(), vec![EVENT_PAYLOAD_PRELUDE]);
}

#[test]
fn a_statement_handler_is_an_inline_context() {
    let carrier = carrier("  <Child @save=\"seen = true; total += 1\" />");
    assert_eq!(
        carrier.projection.obligations[0].shape,
        Some(HandlerShape::Statements)
    );
    assert_eq!(carrier.projection.contexts.len(), 1);
}

#[test]
fn a_listener_object_is_checked_key_by_key_after_its_use() {
    let carrier = carrier("  <Child v-on=\"handlers\" />");
    let projection = &carrier.projection;
    assert!(projection.complete);
    assert_eq!(carrier.keys(), vec![("", &ListenerRoute::ListenerObject)]);
    let statements = projection.statements();
    assert_eq!(statements.len(), 1);
    let statement = &statements[0];
    assert_eq!(statement.phase, StatementPhase::After);
    assert_eq!(statement.family, ObligationFamily::Listener);
    assert!(matches!(statement.host, Some(UseHost::Component(_))));
    let flat = statement.fragment.flatten(&carrier.source, &|id| {
        carrier
            .plan
            .expression(id)
            .expect("admitted")
            .spelling
            .clone()
    });
    assert_eq!(
        flat,
        format!("__VerterListeners({}, (handlers));\n", carrier.binding())
    );
    let mapped = statement
        .fragment
        .pieces
        .iter()
        .filter(|piece| matches!(piece, CheckingPiece::Expression(_)))
        .count();
    assert_eq!(mapped, 1, "the object is mapped once");
    assert_eq!(projection.preludes(), vec![EVENT_PAYLOAD_PRELUDE]);
}

#[test]
fn obligations_follow_authored_order_across_listeners_and_objects() {
    let carrier = carrier("  <Child @first=\"a\" v-on=\"handlers\" @second=\"b\" />");
    let indexes: Vec<_> = carrier
        .projection
        .obligations
        .iter()
        .map(|obligation| obligation.op_index)
        .collect();
    let mut sorted = indexes.clone();
    sorted.sort_unstable();
    assert_eq!(indexes, sorted);
    assert_eq!(carrier.projection.obligations.len(), 3);
}

#[test]
fn a_computed_event_name_and_a_model_update_are_not_authored_listeners() {
    let carrier = carrier("  <Child @[name]=\"onAny\" v-model=\"value\" />");
    assert!(carrier.projection.obligations.is_empty());
    assert!(carrier.projection.relations.is_empty());
}

#[test]
fn a_native_element_listener_is_not_a_component_obligation() {
    let carrier = carrier("  <input @input=\"onInput\" />");
    assert!(carrier.projection.obligations.is_empty());
}

#[test]
fn a_template_without_listeners_contributes_nothing() {
    let carrier = carrier("  <Child />");
    let projection = &carrier.projection;
    assert!(projection.complete);
    assert!(!projection.pending);
    assert!(projection.obligations.is_empty());
    assert!(projection.statements().is_empty());
    assert!(projection.preludes().is_empty());
}

/// A template with no component use has no listener to check. With no
/// expression anywhere the template also has no block, and the product is
/// still complete rather than pending.
#[test]
fn a_template_without_component_uses_has_nothing_to_check() {
    for template in ["  <div>child</div>", "  <div/><span/>", "  <a>link</a>"] {
        let carrier = carrier(template);
        let projection = &carrier.projection;
        assert!(carrier.plan.uses.is_empty(), "{template}");
        assert!(!projection.pending, "{template}: {projection:?}");
        assert!(projection.complete, "{template}: {projection:?}");
        assert!(projection.obligations.is_empty(), "{template}");
        assert!(projection.statements().is_empty(), "{template}");
        assert!(projection.preludes().is_empty(), "{template}");
    }
}

#[test]
fn without_template_blocks_the_product_stays_pending() {
    let source = "<template>\n<Child @save=\"onSave\" />\n</template>\n";
    let parsed = parse_sfc(source, None, None);
    let plan = build_projection_plan(PlanInput {
        canonical_id: "file:///src/Payloads.vue",
        source,
        parsed: &parsed,
        parse_key: None,
        syntax_profile: None,
        custom_elements: None,
    });
    let attributes = project_attribute_operations(&plan, &parsed, source);
    let uses = project_component_uses(&plan, &attributes);
    let transport = project_event_transport(&plan, &attributes);
    let branches = TemplateBranchProjection::pending(plan.snapshot.clone());
    let reentry = project_callback_reentry(&plan, &branches);
    let projection = project_event_payloads(&plan, &branches, &uses, &transport, &reentry);
    assert!(projection.pending);
    assert!(!projection.complete);
    assert!(projection.statements().is_empty());
}

#[test]
fn a_guarded_inline_handler_is_checked_in_its_own_context() {
    let carrier = carrier("  <Child v-if=\"user\" @save=\"user.name.trim()\" />");
    let projection = &carrier.projection;
    assert!(projection.complete);
    assert_eq!(projection.obligations.len(), 1);
    assert_eq!(projection.obligations[0].route, ListenerRoute::Validation);
    assert_eq!(projection.contexts.len(), 1);
    assert_eq!(
        projection.contexts[0].value,
        projection.obligations[0].value
    );
}

#[test]
fn a_listener_object_inside_a_branch_is_placed_with_its_use() {
    let carrier = carrier("  <Child v-if=\"user\" v-on=\"handlers\" />");
    assert!(carrier.projection.complete);
    let statements = carrier.projection.statements();
    assert_eq!(statements.len(), 1);
    assert!(matches!(
        statements[0].anchor,
        super::checking_fragment::StatementAnchor::Item { block, .. } if block.0 != 0
    ));
}

#[test]
fn a_listener_object_with_once_keys_uses_the_named_listener_fallback() {
    let carrier = carrier("<Child v-on=\"{ saveOnce: (id: string) => id.toUpperCase() }\" />");
    assert!(carrier.projection.complete);
    let statements = carrier.projection.statements();
    assert_eq!(statements.len(), 1);
    let check = statements[0].fragment.flatten(&carrier.source, &|id| {
        carrier
            .plan
            .expression(id)
            .expect("admitted")
            .spelling
            .clone()
    });
    assert_eq!(
        check,
        format!(
            "__VerterListeners({}, ({{ saveOnce: (id: string) => id.toUpperCase() }}));\n",
            carrier.binding()
        )
    );
    assert!(EVENT_PAYLOAD_PRELUDE.text.contains(
        "type __VerterListenerFallback<K extends string> = K extends `${infer B}Once` ? B : K;"
    ));
    assert!(EVENT_PAYLOAD_PRELUDE
        .text
        .contains("= __VerterUseListener<I, `on${Capitalize<E & string>}`, __VerterListenerFallback<`on${Capitalize<E & string>}`>>;"));
    assert!(
        !EVENT_PAYLOAD_PRELUDE.text.contains("infer P"),
        "no second lookup of the instance's props"
    );
}
