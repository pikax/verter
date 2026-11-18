//! Unit tests for the native elements module.
use super::native_elements::listener_modifier;
use crate::template::vue_syntax::EventModifierKind;
use verter_parser::utils::vue::tag::{is_mathml_tag, is_svg_tag};

use super::branches::project_template_branches;
use super::callback_reentry::project_callback_reentry;
use super::native_elements::{
    project_native_elements, NativeElementProjection, NATIVE_ELEMENT_PRELUDE,
};
use crate::framework_common::projection_plan::{
    build_projection_plan, ElementNamespace, PlanInput, ProjectionPlan,
};
use crate::framework_common::vue_bridge::parse_sfc;
use crate::template::topology::{build_template_topology, TopologyRecoveryFacts};
use crate::work_ledger::CompilerWorkLedger;
use verter_audit::observe::Uncaptured;

fn project(
    source: &str,
    custom_elements: Option<&[String]>,
) -> (ProjectionPlan, NativeElementProjection) {
    let parsed = parse_sfc(source, None, None);
    let plan = build_projection_plan(PlanInput {
        canonical_id: "file:///Native.vue",
        source,
        parsed: &parsed,
        parse_key: None,
        syntax_profile: None,
        custom_elements,
    });
    let topology = build_template_topology(
        source,
        parsed.template_ast().unwrap(),
        TopologyRecoveryFacts::default(),
        &mut CompilerWorkLedger::<Uncaptured>::new(),
    )
    .unwrap();
    let branches = project_template_branches(&plan, &topology);
    let reentry = project_callback_reentry(&plan, &branches);
    let native = project_native_elements(&plan, &branches, &reentry, custom_elements);
    (plan, native)
}

fn text(source: &str, plan: &ProjectionPlan, native: &NativeElementProjection) -> String {
    let text: String = native
        .statements()
        .iter()
        .map(|statement| {
            statement
                .fragment
                .flatten(source, &|id| plan.expression(id).unwrap().spelling.clone())
        })
        .collect();
    let output = format!("{}{text}", NATIVE_ELEMENT_PRELUDE.text);
    let alloc = oxc_allocator::Allocator::default();
    let parsed =
        verter_parser::oxc_parse::Parser::new(&alloc, &output, oxc_span::SourceType::ts()).parse();
    assert!(
        parsed.diagnostics.is_empty(),
        "{:?}\n{output}",
        parsed.diagnostics
    );
    text
}

#[test]
fn native_attributes_and_inline_listeners_contribute_checked_statements() {
    let source = "<template><input :disabled=\"flag\" @input=\"accept($event)\" /></template>";
    let (plan, native) = project(source, None);
    assert!(native.complete);
    assert!(!native.pending);
    assert_eq!(native.contracts.len(), 1);
    assert_eq!(native.events.len(), 1);
    assert_eq!(native.events[0].listener_key, "onInput");
    let text = text(source, &plan, &native);
    assert!(text.contains("\"disabled\": (flag)"), "{text}");
    assert!(
        text.contains("\"onInput\": ($event) => (void $event, accept($event))"),
        "{text}"
    );
    assert!(!native.preludes().is_empty());
}

#[test]
fn keyboard_mouse_and_input_handlers_keep_shapes_options_and_guard_reentry() {
    let source = concat!(
        "<template><section v-if=\"user\">",
        "<input @input=\"user.accept($event)\" @keydown.enter.capture.once=\"key\" ",
        "@click.right.prevent=\"(event) => use(event)\" @keyup=\"a(); b($event)\" />",
        "</section></template>",
    );
    let (plan, native) = project(source, None);
    assert!(native.complete);
    assert_eq!(
        native
            .events
            .iter()
            .map(|event| event.listener_key.as_str())
            .collect::<Vec<_>>(),
        [
            "onInput",
            "onKeydownCaptureOnce",
            "onContextmenu",
            "onKeyup"
        ]
    );
    let text = text(source, &plan, &native);
    assert!(text.contains("if (!(user)) throw undefined;"), "{text}");
    assert!(text.contains("return (user.accept($event))"), "{text}");
    assert!(text.contains("\"onKeydownCaptureOnce\": (key)"), "{text}");
    assert!(
        text.contains("\"onContextmenu\": ((event) => use(event))"),
        "{text}"
    );
    assert!(text.contains("a(); b($event);"), "{text}");
}

#[test]
fn namespace_and_custom_element_contracts_use_the_admitted_population() {
    let source = concat!(
        "<template><svg :viewBox=\"view\"><a ref=\"link\" :href=\"url\" /></svg>",
        "<math :display=\"display\" /><x-meter :reading=\"reading\" @ready=\"ready\" />",
        "<slot name=\"default\" /><template v-show=\"visible\" /></template>",
    );
    let custom = vec!["x-".to_string()];
    let (plan, native) = project(source, Some(&custom));
    assert!(native.complete);
    assert_eq!(native.policy.custom_elements, custom);
    assert_eq!(
        native
            .contracts
            .iter()
            .map(|contract| (contract.tag.as_str(), contract.namespace))
            .collect::<Vec<_>>(),
        [
            ("svg", ElementNamespace::Svg),
            ("a", ElementNamespace::Svg),
            ("math", ElementNamespace::MathMl),
            ("x-meter", ElementNamespace::Html)
        ]
    );
    let text = text(source, &plan, &native);
    assert!(text.contains("__VerterNative(\"a\", \"svg\")"), "{text}");
    assert!(
        text.contains("__VerterNative(\"x-meter\", \"html\")"),
        "{text}"
    );
    assert!(text.contains("\"reading\": (reading)"), "{text}");
    assert!(
        NATIVE_ELEMENT_PRELUDE
            .text
            .contains("import('vue/jsx-runtime').JSX.IntrinsicElements"),
        "configured custom-element contracts must retain declared JSX members and events"
    );
}

#[test]
fn static_text_spreads_class_style_and_open_attribute_families_are_checked() {
    let source = concat!(
        "<template><button disabled autofocus=\"\" title=\"a &amp; b\" class=\"base\" :class=\"classes\" ",
        "style=\"color:red\" :style=\"styles\" :aria-checked=\"checked\" :data-id=\"id\" ",
        "v-bind=\"attrs\" v-on=\"listeners\" /></template>",
    );
    let (plan, native) = project(source, None);
    assert!(native.complete);
    let text = text(source, &plan, &native);
    for member in [
        "\"disabled\": true",
        "\"autofocus\": \"\"",
        "\"title\": \"a & b\"",
        "\"class\": \"base\"",
        "\"class\": (classes)",
        "\"style\": (styles)",
        "\"aria-checked\": (checked)",
        "\"data-id\": (id)",
        "...(attrs)",
        "((listeners) satisfies __VerterNativeListeners<",
    ] {
        assert!(text.contains(member), "{member}\n{text}");
    }
    let static_value = native
        .statements()
        .into_iter()
        .flat_map(|statement| statement.fragment.pieces)
        .find_map(|piece| match piece {
            super::checking_fragment::CheckingPiece::Alias { text, source: span }
                if text == "\"a & b\"" =>
            {
                Some(span)
            }
            _ => None,
        })
        .unwrap();
    assert_eq!(
        &source[static_value.start as usize..static_value.end as usize],
        "a &amp; b"
    );
}

#[test]
fn conditional_listener_objects_and_entity_values_keep_their_whole_contract() {
    let source = concat!(
        "<template><button type=\"sub&#109;it\" v-on=\"flag ? { click: 123 } : { click: () => {} }\" />",
        "<x-meter :reading=\"42\" :unit=\"'C'\" /></template>",
    );
    let custom = vec!["x-".to_string()];
    let (plan, native) = project(source, Some(&custom));
    assert!(native.complete);
    let text = text(source, &plan, &native);
    assert!(text.contains("\"type\": \"submit\""), "{text}");
    assert!(
        text.contains(
            "((flag ? { click: 123 } : { click: () => {} }) satisfies __VerterNativeListeners<"
        ),
        "{text}"
    );
    let binding = &native.contracts[1].binding;
    assert!(
        text.contains(&format!(
            "satisfies __VerterNativeWrite<typeof {binding}, \"unit\">"
        )),
        "{text}"
    );
}

#[test]
fn unsupported_computed_keys_are_explicitly_incomplete() {
    let source = "<template><svg :[key]=\"value\" /></template>";
    let (plan, native) = project(source, None);
    assert!(!native.complete);
    assert!(!native.pending);
    assert_eq!(native.contracts.len(), 1);
    assert_eq!(native.statements().len(), 1);
    let _ = text(source, &plan, &native);
}

#[test]
fn bound_modifiers_select_the_effective_native_property() {
    let source = "<template><svg :view-box.camel=\"view\" /><input :value.prop=\"value\" :title.attr=\"title\" /></template>";
    let (plan, native) = project(source, None);
    assert!(native.complete);
    let text = text(source, &plan, &native);
    for key in ["viewBox", ".value", "^title"] {
        assert!(text.contains(&format!("\"{key}\":")), "{text}");
    }
}

#[test]
fn ref_only_svg_elements_keep_their_contract_and_callback() {
    let source =
        "<template><svg><a ref=\"link\" /><circle :ref=\"el => accept(el)\" /></svg></template>";
    let (plan, native) = project(source, None);
    assert!(native.complete);
    assert_eq!(native.contracts.len(), 2);
    assert!(native
        .contracts
        .iter()
        .all(|contract| contract.namespace == ElementNamespace::Svg));
    let text = text(source, &plan, &native);
    assert!(text.contains("\"ref\": (el => accept(el))"), "{text}");
}

#[test]
fn no_checked_elements_contribute_no_prelude() {
    let (plan, native) = project("<template><div /></template>", None);
    assert!(native.complete);
    assert!(native.contracts.is_empty());
    assert!(native.statements().is_empty());
    assert!(native.preludes().is_empty());
    let pending = NativeElementProjection::pending(plan.snapshot);
    assert!(pending.statements().is_empty());
    assert!(pending.preludes().is_empty());
}

#[test]
fn svg_tags_are_recognised_by_the_parser_sets() {
    for tag in ["svg", "path", "circle"] {
        assert!(is_svg_tag(tag.as_bytes()), "{tag}");
    }
    assert!(!is_svg_tag(b"div"));
}

#[test]
fn mathml_tags_are_recognised_by_the_parser_sets() {
    for tag in ["math", "mrow", "mi"] {
        assert!(is_mathml_tag(tag.as_bytes()), "{tag}");
    }
    assert!(!is_mathml_tag(b"span"));
}

#[test]
fn event_options_become_listener_key_suffixes() {
    for (modifier, suffix) in [
        ("capture", "Capture"),
        ("once", "Once"),
        ("passive", "Passive"),
    ] {
        let found = listener_modifier(modifier, "scroll").unwrap();
        assert_eq!(found.kind, EventModifierKind::Option);
        assert_eq!(found.key_suffix.as_deref(), Some(suffix));
    }
}

#[test]
fn guards_add_no_key_suffix() {
    for modifier in ["stop", "prevent", "ctrl", "exact"] {
        let found = listener_modifier(modifier, "click").unwrap();
        assert_eq!(found.kind, EventModifierKind::Runtime, "{modifier}");
        assert_eq!(found.key_suffix, None, "{modifier}");
    }
}

#[test]
fn key_names_filter_only_on_keyboard_events() {
    let on_keydown = listener_modifier("enter", "keydown").unwrap();
    assert_eq!(on_keydown.kind, EventModifierKind::Key);
    let on_click = listener_modifier("enter", "click").unwrap();
    assert_eq!(on_click.kind, EventModifierKind::Runtime);
}

#[test]
fn direction_modifiers_follow_the_event_class() {
    assert_eq!(
        listener_modifier("left", "keydown").unwrap().kind,
        EventModifierKind::Key
    );
    assert_eq!(
        listener_modifier("left", "mousedown").unwrap().kind,
        EventModifierKind::Runtime
    );
}

#[test]
fn retargeting_modifiers_leave_no_suffix() {
    assert_eq!(
        listener_modifier("right", "click").unwrap().key_suffix,
        None
    );
}

#[test]
fn empty_modifier_is_not_a_modifier() {
    assert_eq!(listener_modifier("", "click"), None);
}
