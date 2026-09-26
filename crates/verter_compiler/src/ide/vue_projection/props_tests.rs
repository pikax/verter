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
    caller_and_setup_props(&contract)
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
}
