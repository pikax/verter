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

fn rules<'a>(statements: &'a [StyleStatement], output: &mut Vec<&'a verter_css_syntax::StyleRule>) {
    for statement in statements {
        match statement {
            StyleStatement::Rule(rule) => {
                output.push(rule);
                rules(rule.body().statements(), output);
            }
            StyleStatement::AtRule(value) => {
                if let Some(body) = value.body() {
                    rules(body.statements(), output);
                }
            }
            StyleStatement::MixinOrFunction(value) => {
                if let Some(body) = value.body() {
                    rules(body.statements(), output);
                }
            }
            StyleStatement::Unknown(value) => {
                if let Some(body) = value.body() {
                    rules(body.statements(), output);
                }
            }
            StyleStatement::Declaration(_) => {}
        }
    }
}

fn rule_texts(parsed: &verter_css_syntax::StyleSyntaxIr) -> Vec<String> {
    let mut found = Vec::new();
    rules(parsed.statements(), &mut found);
    found
        .into_iter()
        .map(|rule| {
            parsed
                .source()
                .slice(rule.selector_list().span())
                .trim()
                .to_owned()
        })
        .collect()
}

fn declaration_v_binds(parsed: &verter_css_syntax::StyleSyntaxIr) -> Vec<String> {
    fn visit_values(source: &CssSource, values: &[ComponentValue], output: &mut Vec<String>) {
        for value in values {
            match value {
                ComponentValue::Function(function) => {
                    if source.slice(function.name_span()) == "v-bind" {
                        output.push(source.slice(function.full_span()).to_owned());
                    }
                    visit_values(source, function.values(), output);
                }
                ComponentValue::Block(block) => visit_values(source, block.values(), output),
                ComponentValue::Interpolation(interpolation) => {
                    visit_values(source, interpolation.values(), output)
                }
                ComponentValue::Token(_)
                | ComponentValue::String(_)
                | ComponentValue::Comment(_) => {}
            }
        }
    }

    fn visit_statements(
        source: &CssSource,
        statements: &[StyleStatement],
        output: &mut Vec<String>,
    ) {
        for statement in statements {
            match statement {
                StyleStatement::Declaration(declaration) => {
                    visit_values(source, declaration.value().values(), output)
                }
                StyleStatement::Rule(rule) => {
                    visit_statements(source, rule.body().statements(), output)
                }
                StyleStatement::AtRule(value) => {
                    if let Some(body) = value.body() {
                        visit_statements(source, body.statements(), output);
                    }
                }
                StyleStatement::MixinOrFunction(value) => {
                    if let Some(body) = value.body() {
                        visit_statements(source, body.statements(), output);
                    }
                }
                StyleStatement::Unknown(value) => {
                    if let Some(body) = value.body() {
                        visit_statements(source, body.statements(), output);
                    }
                }
            }
        }
    }

    let mut output = Vec::new();
    visit_statements(parsed.source(), parsed.statements(), &mut output);
    output
}

// @ai-generated - Exact adversarial repros for pseudo selectors swallowed as declarations.
#[test]
fn braced_pseudo_selectors_remain_rules_in_every_supported_dialect() {
    for dialect in [CssDialect::Scss, CssDialect::Less] {
        let parsed = ir(".nav {\n  .link:hover {\n    color: red;\n  }\n}", dialect);
        assert_eq!(
            concrete_classes(&parsed),
            vec!["nav", "link"],
            "{dialect:?}"
        );
        assert!(rule_texts(&parsed).iter().any(|text| text == ".link:hover"));
        assert!(parsed.diagnostics().is_empty(), "{dialect:?}");
    }

    for (dialect, input, expected) in [
        (
            CssDialect::Scss,
            ".a { &::before { color: red; } }",
            "&::before",
        ),
        (
            CssDialect::Scss,
            ".a { :deep(.child) { color: red; } }",
            ":deep(.child)",
        ),
        (
            CssDialect::Stylus,
            ".link:hover { color: red; }",
            ".link:hover",
        ),
        (CssDialect::Sass, ".b:hover { color: red; }", ".b:hover"),
    ] {
        let parsed = ir(input, dialect);
        assert!(
            rule_texts(&parsed).iter().any(|text| text == expected),
            "{dialect:?}: {:#?}",
            parsed.statements()
        );
        assert!(parsed.diagnostics().is_empty(), "{dialect:?}");
    }
}

// @ai-generated - Exact minimal pair proving formatting cannot choose the SCSS/Less parser.
#[test]
fn closing_brace_indentation_does_not_change_parser_mode() {
    let indented = ".nav {\n  .link:hover {\n    color: red;\n  }\n}";
    let flush = ".nav {\n  .link:hover {\n    color: red;\n}\n}";
    for dialect in [CssDialect::Scss, CssDialect::Less] {
        let indented = ir(indented, dialect);
        let flush = ir(flush, dialect);
        assert_eq!(
            concrete_classes(&indented),
            vec!["nav", "link"],
            "{dialect:?}"
        );
        assert_eq!(
            concrete_classes(&indented),
            concrete_classes(&flush),
            "{dialect:?}"
        );
        assert_eq!(rule_texts(&indented), rule_texts(&flush), "{dialect:?}");
    }
}

// @ai-generated - Exact trust-rule repros must not publish complete bogus facts.
#[test]
fn ambiguous_sass_forms_never_publish_complete_selector_or_declaration_facts() {
    let nested_property = ir(".a\n  font:\n    family: serif", CssDialect::Sass);
    assert!(
        !rule_texts(&nested_property)
            .iter()
            .any(|text| text == "font:"),
        "{:#?}",
        nested_property.statements()
    );
    let parent = match &nested_property.statements()[0] {
        StyleStatement::Rule(rule) => rule,
        other => panic!("expected .a rule, got {other:#?}"),
    };
    assert!(
        !parent.body().statements().is_empty(),
        "nested-property containment must survive"
    );

    let old_form = ir(".a\n  :color red", CssDialect::Sass);
    let parent = match &old_form.statements()[0] {
        StyleStatement::Rule(rule) => rule,
        other => panic!("expected .a rule, got {other:#?}"),
    };
    assert!(matches!(
        parent.body().statements()[0],
        StyleStatement::Unknown(ref value) if value.kind() == UnknownStatementKind::Ambiguous
    ));
    assert!(!old_form.diagnostics().is_empty());

    let old_form_with_body = ir(".a\n  :color red\n    x: y", CssDialect::Sass);
    let parent = match &old_form_with_body.statements()[0] {
        StyleStatement::Rule(rule) => rule,
        other => panic!("expected .a rule, got {other:#?}"),
    };
    assert!(matches!(
        parent.body().statements()[0],
        StyleStatement::Unknown(ref value) if value.kind() == UnknownStatementKind::Ambiguous
    ));
    assert!(old_form_with_body
        .diagnostics()
        .iter()
        .any(|diagnostic| diagnostic.kind == CssDiagnosticKind::AmbiguousStatement));

    let nested_pseudo = ir(".a\n  :hover\n    color: red", CssDialect::Sass);
    assert!(
        rule_texts(&nested_pseudo)
            .iter()
            .any(|text| text == ":hover"),
        "{:#?}",
        nested_pseudo.statements()
    );
}

// @ai-generated - Exact declaration-owned-block repro must retain its nested statements.
#[test]
fn declaration_classification_never_discards_an_indented_block() {
    let parsed = ir(
        ".a\n  input[type=\"text\"]\n    color red",
        CssDialect::Stylus,
    );
    let texts = rule_texts(&parsed);
    assert!(
        texts.iter().any(|text| text == "input[type=\"text\"]"),
        "{:#?}",
        parsed.statements()
    );
    let mut found = Vec::new();
    rules(parsed.statements(), &mut found);
    let input = found
        .into_iter()
        .find(|rule| {
            parsed.source().slice(rule.selector_list().span()).trim() == "input[type=\"text\"]"
        })
        .unwrap();
    assert!(!input.body().statements().is_empty());
}

// @ai-generated - Exact Stylus attribute-selector repros cannot classify as variables.
#[test]
fn stylus_attribute_selectors_with_equals_are_rules() {
    for selector in ["[data-role=\"nav\"]", "[class~=\"x\"]", "[type=\"text\"]"] {
        let input = format!("{selector}\n  color red");
        let parsed = ir(&input, CssDialect::Stylus);
        assert!(
            matches!(parsed.statements()[0], StyleStatement::Rule(_)),
            "{selector}: {:#?}",
            parsed.statements()
        );
        assert!(rule_texts(&parsed).iter().any(|text| text == selector));
    }
}

// @ai-generated - Exact multiline-value repro keeps continuation values in declarations.
#[test]
fn multiline_declaration_values_remain_one_value_tree() {
    let family = ir(
        ".a {\n  font-family:\n    Helvetica,\n    Arial;\n}",
        CssDialect::Scss,
    );
    assert!(
        family.diagnostics().is_empty(),
        "{:#?}",
        family.diagnostics()
    );
    let StyleStatement::Rule(rule) = &family.statements()[0] else {
        panic!("expected .a rule");
    };
    assert_eq!(rule.body().statements().len(), 1);
    assert!(matches!(
        rule.body().statements()[0],
        StyleStatement::Declaration(_)
    ));

    let bindings = ir(
        ".a {\n  color:\n    v-bind(tone);\n  box-shadow: 0 0 v-bind(blur) red;\n}",
        CssDialect::Scss,
    );
    assert_eq!(
        declaration_v_binds(&bindings),
        vec!["v-bind(tone)", "v-bind(blur)"]
    );
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

// @ai-generated - Pins the IR collector's per-component descent through functional pseudo lists.
#[test]
fn functional_pseudo_class_collection_is_per_component() {
    let parsed = ir(".x:is(.a, .#{$y}) { color: red; }", CssDialect::Scss);
    assert_eq!(concrete_classes(&parsed), vec!["x", "a"]);
    let component_classes: Vec<_> = parsed
        .selector_components()
        .filter(|component| {
            component.kind() == SelectorComponentKind::Class
                && component.facts().is_complete_static()
        })
        .filter_map(|component| component.name_span())
        .map(|span| parsed.source().slice(span).to_owned())
        .collect();
    assert_eq!(component_classes, vec!["x", "a"]);
}

/// `StyleSyntaxIrSink` hands every event inside a `SelectorList` to its nested `SelectorSink` and
/// then stops processing that event itself. A diagnostic raised inside that window is still the
/// stylesheet's diagnostic, so it must be recorded before the selector-sink hand-off — not
/// dropped with the rest of the event. `.a[ {}` is the discriminating input: `UnterminatedBlock`
/// is raised while the selector sink owns events and `ExpectedRuleBlock` after it closes, so a
/// hand-off that swallows in-window diagnostics keeps the second and loses the first.
#[test]
fn diagnostics_raised_inside_a_selector_list_still_reach_the_style_ir() {
    for input in ["[ {}", ".a[ {}", ":global( {}", ".a:nth-child(-2n {}"] {
        assert_eq!(
            ir(input, CssDialect::Css)
                .diagnostics()
                .iter()
                .map(|diagnostic| diagnostic.kind)
                .collect::<Vec<_>>(),
            vec![
                CssDiagnosticKind::UnterminatedBlock,
                CssDiagnosticKind::ExpectedRuleBlock,
            ],
            "{input}"
        );
    }

    // The same window on the LAYOUT path. Sass and Stylus reach the IR sink through the
    // indentation-aware parser, which surrounds the selector window with its own statement
    // classification, so their full sequence differs from Css's — but the selector-window
    // diagnostic itself must survive on every path, which is what the hand-off is about.
    for dialect in [
        CssDialect::Css,
        CssDialect::Scss,
        CssDialect::Less,
        CssDialect::Sass,
        CssDialect::Stylus,
    ] {
        let kinds: Vec<_> = ir(".a[ {}", dialect)
            .diagnostics()
            .iter()
            .map(|diagnostic| diagnostic.kind)
            .collect();
        assert!(
            kinds.contains(&CssDiagnosticKind::UnterminatedBlock),
            "{dialect:?}: selector-window diagnostic lost, got {kinds:?}"
        );
    }

    // Negative control: a well-formed stylesheet records no diagnostics at all, so the assertion
    // above is not satisfied by a sink that indiscriminately records everything.
    for dialect in [CssDialect::Css, CssDialect::Sass, CssDialect::Stylus] {
        assert!(
            ir(".a { color: red }", dialect).diagnostics().is_empty(),
            "{dialect:?}"
        );
    }
}
