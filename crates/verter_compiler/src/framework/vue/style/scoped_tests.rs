use verter_css_syntax::CssDialect;

use super::scoped::transform_vue_scoped_css;
use crate::style_planner::{PlainCssInput, StyleRewriteOutcome};

const SCOPE: &str = "X";

fn scoped(source: &str) -> String {
    let input = PlainCssInput::try_new(
        source,
        CssDialect::Css,
        "probe.css",
        "space:probe",
        "artifact:probe",
    )
    .expect("plain css");
    match transform_vue_scoped_css(input, SCOPE).expect("selectors are trusted") {
        StyleRewriteOutcome::Rewritten { code, .. } => code,
        StyleRewriteOutcome::Unchanged { .. } => panic!("expected a rewrite"),
    }
}

#[test]
fn a_class_named_like_a_scoping_pseudo_is_scoped_as_a_class() {
    assert_eq!(scoped(".where{color:red}"), ".where[data-v-X]{color:red}");
    assert_eq!(scoped(".is{color:red}"), ".is[data-v-X]{color:red}");
}

#[test]
fn a_class_list_with_pseudo_names_scopes_every_class() {
    assert_eq!(
        scoped(".tags{display:flex}.where,.position{color:red}"),
        ".tags[data-v-X]{display:flex}.where[data-v-X],.position[data-v-X]{color:red}"
    );
    assert_eq!(
        scoped(".where,.position{color:red}"),
        ".where[data-v-X],.position[data-v-X]{color:red}"
    );
}

#[test]
fn classes_named_like_vue_pseudos_are_plain_classes() {
    for name in [
        "deep", "global", "slotted", "has", "not", "matches", "any", "where2",
    ] {
        assert_eq!(
            scoped(&format!(".{name}{{color:red}}")),
            format!(".{name}[data-v-X]{{color:red}}"),
            "class named {name}"
        );
    }
}
