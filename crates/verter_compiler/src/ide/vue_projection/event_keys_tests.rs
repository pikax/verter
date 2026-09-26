use super::attribute_operations::project_attribute_operations;
use super::event_keys::{project_event_transport, EventName, ListenerConsumer, ListenerKey};
use crate::framework_common::projection_plan::{build_projection_plan, PlanInput};

fn project(template: &str) -> super::event_keys::EventTransportPlan {
    let source = format!(
        "<script setup lang=\"ts\">\nconst h = () => {{}};\n</script>\n<template>\n{template}\n</template>\n"
    );
    let parsed = crate::compile::parse_sfc(&source, None, None);
    let plan = build_projection_plan(PlanInput {
        canonical_id: "file:///events.vue",
        source: &source,
        parsed: &parsed,
        parse_key: None,
        syntax_profile: None,
    });
    let attributes = project_attribute_operations(&plan, &parsed, &source);
    project_event_transport(&plan, &attributes)
}

#[test]
fn event_transport_keeps_event_identity_separate_from_listener_key_aliases() {
    let projection = project("<Foo @save-item=\"h\" @saveItem=\"h\" :on-save=\"h\" />");
    assert!(projection.complete);
    assert_eq!(projection.aliases.len(), 2);
    assert!(projection
        .aliases
        .iter()
        .all(|alias| alias.listener_key == ListenerKey::Static("onSaveItem".into())));
    assert_eq!(
        projection.aliases[0].event,
        EventName::Static("save-item".into())
    );
    assert_eq!(
        projection.aliases[1].event,
        EventName::Static("saveItem".into())
    );
    assert!(
        projection
            .listeners
            .iter()
            .all(|set| set.key.as_deref() != Some("on-save")),
        ":on-save is a bound prop, not an event"
    );
}

#[test]
fn event_transport_preserves_model_modifiers_dynamic_unions_and_listener_objects() {
    let projection = project(
        "<Foo v-model:model-value.trim=\"value\" @save.once.capture.passive.prevent=\"h\" @[('save' as 'save' | 'cancel')]=\"h\" v-on=\"listeners\" />",
    );
    assert!(projection.complete);
    assert!(projection.aliases.iter().any(|alias| {
        alias.event == EventName::Static("update:modelValue".into())
            && alias.listener_key == ListenerKey::Static("onUpdate:modelValue".into())
            && alias.synthesized
    }));
    assert!(projection.aliases.iter().any(|alias| {
        alias.event == EventName::Static("save".into())
            && alias.listener_key == ListenerKey::Static("onSaveOnceCapturePassive".into())
            && alias.modifiers == ["once", "capture", "passive", "prevent"]
    }));
    assert!(projection.aliases.iter().any(|alias| matches!(
        &alias.event,
        EventName::Dynamic { candidates, .. } if candidates == &vec!["save", "cancel"]
    )));
    assert!(projection.listeners.iter().any(|set| set
        .consumers
        .iter()
        .any(|consumer| matches!(consumer, ListenerConsumer::Object { .. }))));
}

#[test]
fn event_transport_keeps_all_colliding_listener_consumers() {
    let projection = project("<Foo @save=\"first\" @save-item=\"second\" v-on:save=\"third\" />");
    let save = projection
        .listeners
        .iter()
        .find(|set| set.key.as_deref() == Some("onSave"))
        .expect("save listener set");
    assert_eq!(save.consumers.len(), 2);
    assert!(save.collision);
}
