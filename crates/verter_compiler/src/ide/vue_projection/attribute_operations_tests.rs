use super::attribute_operations::{
    project_attribute_operations, AttributeOperationsProjection, AttributeSyntax, Certainty,
    ConsumerChannel, MergeRule, RuntimeKey, SpreadKind, WriteValue,
};
use crate::framework_common::projection_plan::{build_projection_plan, PlanInput};

fn sfc(template: &str) -> String {
    format!(
        "<script setup lang=\"ts\">\nconst h = () => {{}};\n</script>\n<template>\n{template}\n</template>\n"
    )
}

fn project(template: &str) -> AttributeOperationsProjection {
    let source = sfc(template);
    let parsed = crate::compile::parse_sfc(&source, None, None);
    let plan = build_projection_plan(PlanInput {
        canonical_id: "file:///attrs.vue",
        source: &source,
        parsed: &parsed,
        parse_key: None,
        syntax_profile: None,
    });
    project_attribute_operations(&plan, &parsed, &source)
}

fn channel_events(channels: &[ConsumerChannel]) -> Option<&Vec<String>> {
    channels.iter().find_map(|channel| match channel {
        ConsumerChannel::EmittedEvent { events, .. } => Some(events),
        _ => None,
    })
}

fn declared_prop(channels: &[ConsumerChannel]) -> Option<&str> {
    channels.iter().find_map(|channel| match channel {
        ConsumerChannel::DeclaredProp { name } => Some(name.as_str()),
        _ => None,
    })
}

/// `@save`, `:onSave`, `:on-save` and static `onSave` keep their raw
/// spelling while resolving to separately correct runtime keys: the first
/// two share the `onSave` listener key reachable by `emit('save')`;
/// `:on-save` normalizes only as a declared prop and never becomes the
/// `save` listener; static `onSave` carries literal text, not a handler.
#[test]
fn attribute_ops_spellings_keep_raw_runtime_prop_and_event_lookup_distinct() {
    let projection = project(
        "  <Foo @save=\"h\" />\n  <Foo :onSave=\"h\" />\n  <Foo :on-save=\"h\" />\n  <Foo onSave=\"h\" />",
    );
    assert!(projection.complete);
    assert_eq!(projection.sequences.len(), 4);
    let expect = [
        (
            "@save",
            AttributeSyntax::On,
            "onSave",
            WriteValue::Expression,
        ),
        (
            ":onSave",
            AttributeSyntax::Bind,
            "onSave",
            WriteValue::Expression,
        ),
        (
            ":on-save",
            AttributeSyntax::Bind,
            "on-save",
            WriteValue::Expression,
        ),
        (
            "onSave",
            AttributeSyntax::Static,
            "onSave",
            WriteValue::StaticText,
        ),
    ];
    for ((raw, syntax, key, value), (sequence, plan)) in expect
        .into_iter()
        .zip(projection.sequences.iter().zip(&projection.key_plans))
    {
        let op = &sequence.operations[0];
        assert_eq!(op.raw_spelling, raw);
        assert_eq!(op.syntax, syntax);
        assert_eq!(plan.writes[0].key, RuntimeKey::Static(key.to_string()));
        assert_eq!(plan.writes[0].value, value);
        let relation = projection
            .relation(&sequence.use_id, key)
            .expect("relation");
        assert_eq!(declared_prop(&relation.channels), Some("onSave"), "{raw}");
        let events = channel_events(&relation.channels);
        if key == "onSave" {
            assert!(
                events.expect("event channel").contains(&"save".to_string()),
                "{raw}"
            );
        } else {
            assert!(
                !events.is_some_and(|events| events.iter().any(|e| e == "save")),
                "`{raw}` must not become the `save` listener: {events:?}"
            );
        }
        assert!(relation
            .channels
            .contains(&ConsumerChannel::FallthroughAttr {
                key: key.to_string()
            }));
    }
    let static_relation = projection
        .relation(&projection.sequences[3].use_id, "onSave")
        .expect("static relation");
    assert!(static_relation
        .obligations
        .iter()
        .all(|obligation| obligation.value == WriteValue::StaticText
            && obligation.expression.is_none()));
    assert_eq!(
        projection.sequences[3].operations[0].static_text.as_deref(),
        Some("h")
    );
}

/// Two listeners for one key survive an interleaved `v-bind` object: the
/// spread opens a `mergeProps` argument and listeners accumulate across it.
#[test]
fn attribute_ops_merge_listeners_accumulate_across_interleaved_v_bind() {
    let projection =
        project("  <Foo @save=\"h\" v-bind=\"attrs\" :onSave=\"h\" class=\"a\" :class=\"c\" />");
    assert!(projection.complete);
    let plan = &projection.key_plans[0];
    assert_eq!(plan.spreads.len(), 1);
    assert_eq!(plan.spreads[0].kind, SpreadKind::BindObject);
    let groups: Vec<u32> = plan.writes.iter().map(|write| write.group).collect();
    assert!(groups[0] < plan.spreads[0].group && plan.spreads[0].group < groups[1]);
    let save = plan.effective("onSave").expect("onSave");
    assert_eq!(save.rule, MergeRule::Accumulate);
    let definite: Vec<u32> = save
        .contributors
        .iter()
        .filter(|c| c.certainty == Certainty::Definite)
        .map(|c| c.op_index)
        .collect();
    assert_eq!(
        definite,
        vec![0, 2],
        "both authored listeners reach the component"
    );
    assert!(save.overridden.is_empty());
    let class = plan.effective("class").expect("class");
    assert_eq!(class.rule, MergeRule::Combine);
    assert_eq!(
        class
            .contributors
            .iter()
            .filter(|c| c.certainty == Certainty::Definite)
            .count(),
        2
    );
    let relation = projection
        .relation(&projection.sequences[0].use_id, "onSave")
        .expect("relation");
    assert_eq!(relation.inference_inputs, save.contributors);
}

/// A known later prop overrides an earlier value by Vue semantics: a later
/// `mergeProps` argument overwrites, while inside one literal group the
/// runtime compiler keeps the first static write.
#[test]
fn attribute_ops_overwrite_later_definite_prop_wins() {
    let projection = project(
        "  <Foo title=\"a\" v-bind=\"attrs\" :title=\"t\" />\n  <Foo title=\"a\" :title=\"t\" />",
    );
    assert!(projection.complete);
    let across = projection.key_plans[0].effective("title").expect("title");
    assert_eq!(across.rule, MergeRule::Overwrite);
    assert_eq!(across.contributors.len(), 1);
    assert_eq!(across.contributors[0].op_index, 2);
    assert_eq!(across.contributors[0].certainty, Certainty::Definite);
    assert_eq!(
        across.overridden,
        vec![0, 1],
        "static title and the spread are overwritten"
    );
    let within = projection.key_plans[1].effective("title").expect("title");
    assert_eq!(within.contributors.len(), 1);
    assert_eq!(
        within.contributors[0].op_index, 0,
        "literal dedupe keeps the first write"
    );
    assert_eq!(within.overridden, vec![1]);
    let relation = projection
        .relation(&projection.sequences[0].use_id, "title")
        .expect("relation");
    let effective: Vec<(u32, bool)> = relation
        .obligations
        .iter()
        .filter(|o| matches!(o.channel, ConsumerChannel::DeclaredProp { .. }))
        .map(|o| (o.op_index, o.effective))
        .collect();
    assert_eq!(effective, vec![(0, false), (2, true)]);
}

/// A spread whose keys are opaque (an optional `title` included) is never
/// an unconditional overwrite: the earlier definite `title` stays a
/// contributor next to the spread's possible write.
#[test]
fn attribute_ops_optional_spread_keeps_earlier_key_possible() {
    let projection = project("  <Foo :title=\"t\" v-bind=\"maybe\" />");
    assert!(projection.complete);
    let title = projection.key_plans[0].effective("title").expect("title");
    let contributors: Vec<(u32, Certainty)> = title
        .contributors
        .iter()
        .map(|c| (c.op_index, c.certainty))
        .collect();
    assert_eq!(
        contributors,
        vec![(0, Certainty::Definite), (1, Certainty::Possible)]
    );
    assert!(title.overridden.is_empty());
    let relation = projection
        .relation(&projection.sequences[0].use_id, "title")
        .expect("relation");
    assert!(relation.obligations.iter().all(|o| o.effective));
}

/// A callback key serving both a declared prop and an emitted event
/// validates every reachable channel, not only the easier one.
#[test]
fn attribute_ops_collision_validates_every_reachable_channel() {
    let projection = project("  <Foo :onSave=\"h\" @save=\"h\" />");
    assert!(projection.complete);
    let relation = projection
        .relation(&projection.sequences[0].use_id, "onSave")
        .expect("relation");
    for op_index in [0, 1] {
        let channels: Vec<&ConsumerChannel> = relation
            .obligations
            .iter()
            .filter(|o| o.op_index == op_index)
            .map(|o| &o.channel)
            .collect();
        assert!(
            channels
                .iter()
                .any(|c| matches!(c, ConsumerChannel::DeclaredProp { name } if name == "onSave")),
            "op {op_index} must validate the declared prop: {channels:?}"
        );
        assert!(
            channels.iter().any(|c| matches!(
                c,
                ConsumerChannel::EmittedEvent { events, once: false } if events.contains(&"save".to_string())
            )),
            "op {op_index} must validate the emitted event: {channels:?}"
        );
    }
}

/// Models, directives and modifiers stay in authored order with their
/// runtime keys: `v-model:first-name` writes the raw prop key plus the
/// camelized update listener and its modifiers object, `.once` extends the
/// listener key, `.camel`/`.prop` rewrite the bound key, directives write no
/// property and reserved keys reach no prop or attr.
#[test]
fn attribute_ops_sequence_keeps_models_directives_and_modifiers_ordered() {
    let projection = project(
        "  <Foo v-focus v-model:first-name.trim=\"n\" @save.once=\"h\" :item-id.camel=\"i\" key=\"k\" @[evt]=\"h\" />",
    );
    assert!(projection.complete);
    let sequence = &projection.sequences[0];
    let syntaxes: Vec<AttributeSyntax> = sequence.operations.iter().map(|op| op.syntax).collect();
    assert_eq!(
        syntaxes,
        vec![
            AttributeSyntax::Directive,
            AttributeSyntax::Model,
            AttributeSyntax::On,
            AttributeSyntax::Bind,
            AttributeSyntax::Static,
            AttributeSyntax::On,
        ]
    );
    assert_eq!(sequence.operations[1].modifiers, vec!["trim".to_string()]);
    assert!(sequence.operations[5].dynamic_argument.is_some());
    let plan = &projection.key_plans[0];
    assert_eq!(plan.no_property, vec![0]);
    let keys: Vec<RuntimeKey> = plan.writes.iter().map(|w| w.key.clone()).collect();
    let key = |k: &str| RuntimeKey::Static(k.to_string());
    assert_eq!(
        keys,
        vec![
            key("first-name"),
            key("onUpdate:firstName"),
            key("first-nameModifiers"),
            key("onSaveOnce"),
            key("itemId"),
            key("key"),
            RuntimeKey::Dynamic,
        ]
    );
    let once = projection
        .relation(&sequence.use_id, "onSaveOnce")
        .expect("once relation");
    assert!(once.channels.contains(&ConsumerChannel::EmittedEvent {
        events: vec!["save".to_string(), "Save".to_string()],
        once: true,
    }));
    let model = projection
        .relation(&sequence.use_id, "first-name")
        .expect("model relation");
    assert_eq!(declared_prop(&model.channels), Some("firstName"));
    let reserved = projection.relation(&sequence.use_id, "key").expect("key");
    assert_eq!(reserved.channels, vec![ConsumerChannel::Reserved]);
    // The dynamic `@[evt]` write may add a listener to `onSaveOnce` but not
    // to the ordinary `itemId` key.
    let dynamic = plan.writes.last().expect("dynamic").op_index;
    assert!(plan
        .effective("onSaveOnce")
        .unwrap()
        .contributors
        .iter()
        .any(|c| c.op_index == dynamic && c.certainty == Certainty::Possible));
    assert!(plan
        .effective("itemId")
        .unwrap()
        .contributors
        .iter()
        .all(|c| c.op_index != dynamic));
}

/// `<component :is>` selects the component; the `is` operation writes no
/// property while ordinary operations on the same use still do.
#[test]
fn attribute_ops_dynamic_component_is_selects_instead_of_writing() {
    let projection = project("  <component :is=\"comp\" :title=\"t\" />");
    assert!(projection.complete);
    let plan = &projection.key_plans[0];
    assert_eq!(plan.no_property, vec![0]);
    assert!(plan.effective("is").is_none());
    assert!(plan.effective("title").is_some());
}

/// Products are a pure function of the admitted plan: an unchanged
/// snapshot yields identical products, and an incomplete plan never
/// publishes a complete product.
#[test]
fn attribute_ops_products_are_deterministic_and_incomplete_plans_stay_incomplete() {
    let template = "  <Foo @save=\"h\" v-bind=\"attrs\" :title=\"t\" />";
    assert_eq!(project(template), project(template));
    let broken = project("  <Foo :title=\"t +\" />");
    assert!(!broken.complete);
}
