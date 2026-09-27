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
        EventName::Dynamic { candidates, open, .. }
            if candidates == &vec!["save".to_string(), "cancel".to_string()] && !*open
    )));
    let open: Vec<_> = projection
        .listeners
        .iter()
        .filter(|set| set.key.is_none())
        .collect();
    assert_eq!(
        open.len(),
        1,
        "a closed name union does not join the open domain"
    );
    assert_eq!(open[0].consumers.len(), 1);
    assert!(!open[0].collision);
    assert!(matches!(
        open[0].consumers[0],
        ListenerConsumer::Object { .. }
    ));
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

#[test]
fn event_transport_excludes_static_on_save_text() {
    let projection = project("<Foo onSave=\"handler\" @cancel=\"h\" />");
    assert!(projection.complete);
    assert!(
        projection
            .listeners
            .iter()
            .any(|set| set.key.as_deref() == Some("onCancel")),
        "a real listener is still recorded"
    );
    assert!(
        projection
            .listeners
            .iter()
            .all(|set| set.key.as_deref() != Some("onSave")),
        "static onSave=\"handler\" is text, not a listener binding"
    );
    assert!(projection.aliases.iter().all(|alias| {
        !matches!(&alias.event, EventName::Static(name) if name == "onSave" || name == "handler")
    }));
}

#[test]
fn event_transport_merges_open_listener_domains() {
    let objects = project("<Foo v-on=\"first\" v-on=\"second\" />");
    let open: Vec<_> = objects
        .listeners
        .iter()
        .filter(|set| set.key.is_none())
        .collect();
    assert_eq!(open.len(), 1);
    assert_eq!(open[0].consumers.len(), 2);
    assert!(open[0].collision);

    let names = project("<Foo @[first]=\"h\" @[second]=\"h\" />");
    let open: Vec<_> = names
        .listeners
        .iter()
        .filter(|set| set.key.is_none())
        .collect();
    assert_eq!(open.len(), 1);
    assert_eq!(open[0].consumers.len(), 2);
    assert!(open[0].collision);
    assert!(names.listeners.iter().all(|set| set.key.is_none()));
}

#[test]
fn event_transport_dynamic_model_argument_is_not_model_value() {
    let dynamic = project("<Foo v-model:[name]=\"value\" />");
    assert!(dynamic.complete);
    assert!(dynamic.aliases.iter().any(|alias| {
        alias.synthesized
            && matches!(
                &alias.event,
                EventName::Dynamic { candidates, open, .. } if candidates.is_empty() && *open
            )
            && alias.listener_key == ListenerKey::Dynamic
    }));
    assert!(dynamic
        .aliases
        .iter()
        .all(|alias| { alias.event != EventName::Static("update:modelValue".into()) }));
    assert!(dynamic.listeners.iter().all(|set| {
        set.key.as_deref() != Some("onUpdate:modelValue")
            && set.consumers.iter().all(|consumer| {
                !matches!(
                    consumer,
                    ListenerConsumer::ModelUpdate { event: EventName::Static(name), .. }
                        if name == "update:modelValue"
                )
            })
    }));

    let finite = project("<Foo v-model:[('title' as 'title' | 'name')]=\"value\" />");
    assert!(finite.complete);
    assert!(finite.aliases.iter().any(|alias| matches!(
        &alias.event,
        EventName::Dynamic { candidates, open, .. }
            if candidates == &["update:title".to_string(), "update:name".to_string()] && !*open
    )));
    assert!(finite
        .listeners
        .iter()
        .any(|set| set.key.as_deref() == Some("onUpdate:title")));
    assert!(finite
        .listeners
        .iter()
        .any(|set| set.key.as_deref() == Some("onUpdate:name")));
    assert!(finite.listeners.iter().all(|set| set.key.is_some()));
}

#[test]
fn event_transport_keeps_non_literal_branches_in_the_open_domain() {
    let projection = project("<Foo @[cond ? 'save' : otherEvent]=\"h\" />");
    assert!(projection.complete);
    assert!(projection.aliases.iter().any(|alias| matches!(
        &alias.event,
        EventName::Dynamic { candidates, open, .. }
            if candidates == &["save".to_string()] && *open
    )));
    let save = projection
        .listeners
        .iter()
        .find(|set| set.key.as_deref() == Some("onSave"))
        .expect("definite save listener");
    assert_eq!(save.consumers.len(), 1);
    assert!(!save.collision);
    let open = projection
        .listeners
        .iter()
        .find(|set| set.key.is_none())
        .expect("open listener domain");
    assert_eq!(open.consumers.len(), 1);
    assert!(!open.collision);
}

#[test]
fn event_transport_keeps_modifiers_on_finite_dynamic_listener_keys() {
    let projection = project("<Foo @[('save' as 'save' | 'cancel')].once=\"h\" />");
    assert!(projection.complete);
    assert!(projection.aliases.iter().any(|alias| {
        matches!(
            &alias.event,
            EventName::Dynamic { candidates, open, .. }
                if candidates == &["save".to_string(), "cancel".to_string()] && !*open
        ) && alias.modifiers == ["once"]
            && alias.listener_key
                == ListenerKey::Finite(vec!["onSaveOnce".into(), "onCancelOnce".into()])
    }));
    assert!(projection
        .listeners
        .iter()
        .any(|set| set.key.as_deref() == Some("onSaveOnce")));
    assert!(projection
        .listeners
        .iter()
        .any(|set| set.key.as_deref() == Some("onCancelOnce")));
    assert!(projection.listeners.iter().all(|set| set.key.is_some()));
}
