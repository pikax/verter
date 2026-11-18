//! Minimized single-file components for Vue template and script-setup compilation.
//!
//! Each fixture is one component, named by the behaviour it exercises.

use verter_compiler::framework_common::{CarrierFrontend, VueCarrierFrontend};
use verter_language::{LanguageDiagnosticSeverity, ParseOptions};

const FIXTURES: &[(&str, &str)] = &[
    (
        "force_js_define_props_boolean",
        include_str!("force_js_define_props_boolean.vue"),
    ),
    (
        "scoped_class_named_where_in_list",
        include_str!("scoped_class_named_where_in_list.vue"),
    ),
    (
        "scoped_class_named_where",
        include_str!("scoped_class_named_where.vue"),
    ),
    (
        "scoped_class_named_where_first_in_list",
        include_str!("scoped_class_named_where_first_in_list.vue"),
    ),
    (
        "scoped_class_named_is",
        include_str!("scoped_class_named_is.vue"),
    ),
    (
        "scoped_class_named_where2",
        include_str!("scoped_class_named_where2.vue"),
    ),
    (
        "scoped_class_named_deep",
        include_str!("scoped_class_named_deep.vue"),
    ),
    (
        "scoped_class_named_global",
        include_str!("scoped_class_named_global.vue"),
    ),
    (
        "scoped_class_named_slotted",
        include_str!("scoped_class_named_slotted.vue"),
    ),
    (
        "scoped_class_named_has",
        include_str!("scoped_class_named_has.vue"),
    ),
    (
        "scoped_class_named_not",
        include_str!("scoped_class_named_not.vue"),
    ),
    (
        "scoped_class_named_matches",
        include_str!("scoped_class_named_matches.vue"),
    ),
    (
        "scoped_class_named_any",
        include_str!("scoped_class_named_any.vue"),
    ),
    (
        "server_scoped_style_root",
        include_str!("server_scoped_style_root.vue"),
    ),
    (
        "server_conditional_slot_from_slots",
        include_str!("server_conditional_slot_from_slots.vue"),
    ),
    (
        "server_conditional_slot_from_binding",
        include_str!("server_conditional_slot_from_binding.vue"),
    ),
    (
        "server_slot_handler_two_statements",
        include_str!("server_slot_handler_two_statements.vue"),
    ),
    (
        "server_handler_two_statements",
        include_str!("server_handler_two_statements.vue"),
    ),
    (
        "server_slot_handler_one_statement",
        include_str!("server_slot_handler_one_statement.vue"),
    ),
    (
        "server_scoped_slot_if_else",
        include_str!("server_scoped_slot_if_else.vue"),
    ),
    (
        "server_comment_only_template_backticks",
        include_str!("server_comment_only_template_backticks.vue"),
    ),
    (
        "server_nested_comment_backticks",
        include_str!("server_nested_comment_backticks.vue"),
    ),
    (
        "server_comment_only_template",
        include_str!("server_comment_only_template.vue"),
    ),
    (
        "server_comment_only_template_with_script",
        include_str!("server_comment_only_template_with_script.vue"),
    ),
];

/// Returns the source of the fixture called `name`.
pub fn fixture(name: &str) -> &'static str {
    FIXTURES
        .iter()
        .find(|(fixture_name, _)| *fixture_name == name)
        .map(|(_, source)| *source)
        .unwrap_or_else(|| panic!("no Vue route fixture named {name}"))
}

fn error_messages(source: &str) -> Vec<String> {
    let artifact = VueCarrierFrontend
        .parse(source, &ParseOptions::vue_standard())
        .expect("the standard Vue parse options are accepted");
    artifact
        .diagnostics
        .iter()
        .filter(|diagnostic| diagnostic.severity == LanguageDiagnosticSeverity::Error)
        .map(|diagnostic| diagnostic.code.to_string())
        .collect()
}

#[test]
fn every_fixture_parses() {
    for (name, _) in FIXTURES {
        let source = fixture(name);
        let errors = error_messages(source);
        assert!(
            errors.is_empty(),
            "fixture {name} has fatal parse diagnostics: {errors:?}"
        );
    }
}
