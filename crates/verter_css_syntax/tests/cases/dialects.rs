use std::sync::Arc;

use verter_css_syntax::{
    parse_style_ir, ComponentValue, CssDiagnosticKind, CssDialect, CssParseMode, CssSource,
    SelectorComponentKind, StyleBlockKind, StyleStatement, UnknownStatementKind,
};

fn ir(input: &str, dialect: CssDialect) -> verter_css_syntax::StyleSyntaxIr {
    parse_style_ir(
        CssSource::new(Arc::from(input), 0).unwrap(),
        dialect,
        CssParseMode::Recover,
    )
    .unwrap()
}

fn concrete_classes(ir: &verter_css_syntax::StyleSyntaxIr) -> Vec<String> {
    ir.complete_static_classes()
        .map(|class| ir.source().slice(class.name_span()).to_owned())
        .collect()
}

// @ai-generated - Covers structural and opaque containment for all five dialects.
#[test]
fn five_dialects_share_one_structural_authority() {
    let cases = [
        (CssDialect::Css, ".css { color: calc(1px); }"),
        (CssDialect::Scss, "$tone: red\n.scss\n  color: #{$tone}\n"),
        (CssDialect::Less, "@tone: red;\n.less\n  color: @{tone}\n"),
        (CssDialect::Sass, "$tone: red\n.sass\n  color: #{$tone}\n"),
        (CssDialect::Stylus, "tone = red\n.stylus\n  color ${tone}\n"),
    ];

    for (dialect, input) in cases {
        let parsed = ir(input, dialect);
        let classes = concrete_classes(&parsed);
        assert_eq!(classes.len(), 1, "{dialect:?}: {classes:?}");
        assert!(classes[0].starts_with(dialect_class_prefix(dialect)));
        let rule = parsed
            .statements()
            .iter()
            .find_map(|statement| match statement {
                StyleStatement::Rule(rule) => Some(rule),
                _ => None,
            })
            .expect("dialect rule");
        if dialect != CssDialect::Css {
            assert_eq!(rule.body().kind(), StyleBlockKind::Indented);
        }
        assert!(!rule.body().statements().is_empty(), "{dialect:?}");
    }
}

// @ai-generated - Pins dialect statement containment while expressions and guards stay opaque.
#[test]
fn dialect_mixin_function_and_directive_headers_are_contained_not_evaluated() {
    let css = ir(
        "@future balanced(fn(x)) { .css { color: red; } }",
        CssDialect::Css,
    );
    assert!(matches!(css.statements()[0], StyleStatement::AtRule(_)));

    for (dialect, input) in [
        (
            CssDialect::Scss,
            "@mixin pad($x) { .scss { padding: $x; } }",
        ),
        (
            CssDialect::Less,
            ".pad(@x) when (@x > 0) { .less { padding: @x; } }",
        ),
        (CssDialect::Sass, "@function size($x)\n  @return $x\n"),
        (CssDialect::Stylus, "pad(x)\n  width x\n"),
    ] {
        let parsed = ir(input, dialect);
        assert!(
            matches!(parsed.statements()[0], StyleStatement::MixinOrFunction(_)),
            "{dialect:?}: {:#?}",
            parsed.statements()
        );
    }

    for dialect in [
        CssDialect::Css,
        CssDialect::Scss,
        CssDialect::Less,
        CssDialect::Sass,
        CssDialect::Stylus,
    ] {
        let parsed = ir("@import \"theme\";", dialect);
        assert!(parsed.imports_unresolved(), "{dialect:?}");
    }
}

fn dialect_class_prefix(dialect: CssDialect) -> &'static str {
    match dialect {
        CssDialect::Css => "css",
        CssDialect::Scss => "scss",
        CssDialect::Less => "less",
        CssDialect::Sass => "sass",
        CssDialect::Stylus => "stylus",
    }
}

// @ai-generated - Discriminates next-line lookahead and whitespace adjacency without evaluation.
#[test]
fn stylus_and_old_sass_ambiguity_is_local_and_fail_closed() {
    let stylus = ir(
        "foo bar\n  .child\n    color red\nborder-radius 5px\n.safe\n  color blue\n",
        CssDialect::Stylus,
    );
    assert!(matches!(stylus.statements()[0], StyleStatement::Rule(_)));
    assert!(matches!(
        stylus.statements()[1],
        StyleStatement::Unknown(ref value)
            if value.kind() == UnknownStatementKind::Ambiguous
    ));
    assert_eq!(concrete_classes(&stylus), vec!["child", "safe"]);

    let sass = ir(
        "+mix\n  color: red\n+ mix\n  color: blue\n",
        CssDialect::Sass,
    );
    assert!(matches!(
        sass.statements()[0],
        StyleStatement::MixinOrFunction(_)
    ));
    assert!(matches!(
        sass.statements()[1],
        StyleStatement::Unknown(ref value)
            if value.kind() == UnknownStatementKind::Ambiguous
    ));
}

// @ai-generated - Pins byte-prefix indentation, brace dominance, and local recovery.
#[test]
fn layout_recovery_preserves_dedented_siblings_and_mid_edit_classes() {
    let input = concat!(
        ".broken\n",
        "\tcolor: #{$tone\n",
        "  .inconsistent\n",
        ".braced {\n",
        " color: red\n",
        "}\n",
        ".after\n",
        "  color: blue\n",
    );
    let parsed = ir(input, CssDialect::Sass);
    assert!(parsed
        .diagnostics()
        .iter()
        .any(|diagnostic| diagnostic.kind == CssDiagnosticKind::InconsistentIndentation));
    assert!(parsed
        .diagnostics()
        .iter()
        .any(|diagnostic| diagnostic.kind == CssDiagnosticKind::UnterminatedInterpolation));
    assert!(concrete_classes(&parsed).contains(&"after".to_owned()));
    assert!(parsed.statements().iter().any(|statement| {
        matches!(statement, StyleStatement::Rule(rule) if rule.body().kind() == StyleBlockKind::Braced)
    }), "{:#?}", parsed.statements());
}

// @ai-generated - An orphan indentation is local ambiguity and cannot steal a sibling.
#[test]
fn unexpected_root_indent_is_ambiguous_and_does_not_own_following_lines() {
    let parsed = ir("  orphan value\n.safe\n  color blue\n", CssDialect::Stylus);
    assert!(parsed
        .diagnostics()
        .iter()
        .any(|diagnostic| diagnostic.kind == CssDiagnosticKind::UnexpectedIndentation));
    assert!(matches!(
        parsed.statements()[0],
        StyleStatement::Unknown(ref value)
            if value.kind() == UnknownStatementKind::Ambiguous
    ));
    assert!(concrete_classes(&parsed).contains(&"safe".to_owned()));
}

// @ai-generated - Stage-one consumers can reach complete v-bind functions in ambiguous values.
#[test]
fn ambiguous_component_values_retain_balanced_v_bind_functions() {
    let parsed = ir("border-radius v-bind(color)\n", CssDialect::Stylus);
    let StyleStatement::Unknown(statement) = &parsed.statements()[0] else {
        panic!("optional Stylus syntax stays ambiguous");
    };
    let values = statement
        .opaque_values()
        .expect("ambiguous component-value region remains reachable");
    assert!(values.values().iter().any(|value| {
        matches!(value, ComponentValue::Function(function)
            if function.is_complete() && parsed.source().slice(function.name_span()) == "v-bind")
    }));
}

// @ai-generated - Dynamic selectors never become concrete class facts while disjoint facts survive.
#[test]
fn dynamic_selector_trust_is_typed_and_disjoint_static_classes_survive() {
    let parsed = ir(
        ".icon-#{tone}\n  color: red\n.safe\n  color: blue\n",
        CssDialect::Sass,
    );
    assert_eq!(concrete_classes(&parsed), vec!["safe"]);
    let dynamic = parsed
        .selector_components()
        .find(|component| component.kind() == SelectorComponentKind::DynamicClass)
        .expect("dynamic class fact");
    assert_eq!(dynamic.name_span(), None);
    assert!(parsed.has_dynamic_selectors());
}
