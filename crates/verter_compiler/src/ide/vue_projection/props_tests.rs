use super::attribute_operations::project_attribute_operations;
use super::props::{caller_and_setup_props, project_props};
use super::public_constructor::project_public_constructor;
use super::script_setup::ScriptBlockInput;
use crate::cursor::ScriptLanguage;
use crate::framework_common::projection_plan::{build_projection_plan, PlanInput};

fn project(template: &str) -> super::props::PropsProjection {
    let source = format!("<template>{template}</template>");
    let parsed = crate::compile::parse_sfc(&source, None, None);
    let plan = build_projection_plan(PlanInput {
        canonical_id: "file:///props.vue",
        source: &source,
        parsed: &parsed,
        parse_key: None,
        syntax_profile: None,
    });
    project_props(&project_attribute_operations(&plan, &parsed, &source))
}

fn caller_contract(setup: &str) -> super::props::CallerAndSetupPropsContract {
    let contract = project_public_constructor(
        None,
        Some(ScriptBlockInput {
            content: setup,
            content_start: 0,
            lang: Some(ScriptLanguage::TypeScript),
        }),
        None,
    )
    .expect("projects");
    caller_and_setup_props(&contract, [setup])
}

#[test]
fn known_spreads_are_checked_but_open_spreads_remain_framework_legal() {
    let projection = project(
        r#"<Child v-bind="{ title: 1, titel: 2 }" :title="title" />
           <Child v-bind="open" />"#,
    );
    assert!(projection.complete);
    assert_eq!(projection.obligations.len(), 2);
    assert_eq!(projection.obligations[0].overwritten_keys, vec!["title"]);
    assert!(projection.obligations[0].checks_known_keys());
    assert!(projection.obligations[1].checks_known_keys());
}

#[test]
fn a_later_possible_spread_does_not_revive_a_definitely_overwritten_key() {
    let projection = project(r#"<Child v-bind="{ title: 1 }" :title="ok" v-bind="rest" />"#);
    assert!(projection.complete);
    assert_eq!(projection.obligations.len(), 2);
    assert_eq!(projection.obligations[0].overwritten_keys, vec!["title"]);
    assert!(projection.obligations[0].checks_known_keys());
    assert!(projection.obligations[1].overwritten_keys.is_empty());
}

#[test]
fn defaults_make_callers_optional_but_setup_values_defined_only_when_named() {
    let named = caller_contract(
        "const props = withDefaults(defineProps<{ title: string; count?: number }>(), { title: 'fallback' });",
    );
    assert_eq!(named.caller_optional_keys, ["title"]);
    assert_eq!(named.setup_defined_keys, ["title"]);
    assert!(named.defaults_are_static);

    let open = caller_contract(
        "const props = withDefaults(defineProps<{ title: string }>(), { ...fallbacks });",
    );
    assert!(open.caller_optional_keys.is_empty());
    assert!(open.setup_defined_keys.is_empty());
    assert!(!open.defaults_are_static);
    assert_eq!(open.caller_required_keys, ["title"]);
    assert!(open.required_keys_are_static);
}

#[test]
fn reactive_destructure_defaults_and_boolean_casts_split_caller_from_setup() {
    let destructured = caller_contract(
        "const { title = \"fallback\" } = defineProps<{ title: string; count: number; flag?: boolean }>();",
    );
    assert_eq!(destructured.reactive_default_keys, ["title"]);
    assert_eq!(destructured.caller_optional_keys, ["title", "flag"]);
    assert_eq!(destructured.setup_defined_keys, ["title", "flag"]);
    assert_eq!(destructured.caller_required_keys, ["count"]);
    assert_eq!(destructured.boolean_cast_keys, ["flag"]);
    assert!(destructured.boolean_empty_string_keys.is_empty());
    assert!(destructured.validator_keys.is_empty());
    assert!(destructured.defaults_are_static);
    assert!(destructured.required_keys_are_static);
    let probe = include_str!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../tests/sfc-projection/STP20/probes/positive.ts"
    ));
    assert!(
        probe
            .replace("\r\n", "\n")
            .contains(&destructured.witness_types()),
        "positive probe must contain the rendered caller contract:\n{}",
        destructured.witness_types()
    );

    // `withDefaults` disables reactive destructure. The static defaults
    // object is the only caller default.
    let wrapped = caller_contract(
        "const { title = \"ignored\" } = withDefaults(defineProps<{ title: string; count: number }>(), { count: 1 });",
    );
    assert!(wrapped.reactive_default_keys.is_empty());
    assert_eq!(wrapped.caller_optional_keys, ["count"]);
    assert_eq!(wrapped.caller_required_keys, ["title"]);
}

#[test]
fn runtime_boolean_cast_validators_and_required_props_are_separate_facts() {
    let runtime = caller_contract(
        "defineProps({ disabled: Boolean, label: { type: String, default: \"x\" }, title: { type: String, required: true, validator: (value: string) => value.length > 0 }, mixed: { type: [String, Boolean] }, strict: { type: [Boolean, String], required: true } });",
    );
    assert_eq!(runtime.boolean_cast_keys, ["disabled", "mixed", "strict"]);
    assert_eq!(runtime.boolean_empty_string_keys, ["mixed"]);
    assert_eq!(runtime.validator_keys, ["title"]);
    assert_eq!(runtime.caller_required_keys, ["title", "strict"]);
    assert_eq!(runtime.caller_optional_keys, ["label", "disabled", "mixed"]);
    assert_eq!(runtime.setup_defined_keys, ["label", "disabled", "mixed"]);
    assert!(runtime.required_keys_are_static);

    let named =
        caller_contract("interface Props { title: string; flag?: boolean }\ndefineProps<Props>();");
    assert_eq!(named.caller_required_keys, ["title"]);
    assert_eq!(named.boolean_cast_keys, ["flag"]);
    assert_eq!(named.caller_optional_keys, ["flag"]);
    assert!(named.required_keys_are_static);
}
