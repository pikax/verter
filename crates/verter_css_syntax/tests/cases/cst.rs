use std::mem::size_of;
use std::process::Command;
use std::sync::Arc;

use verter_css_syntax::{
    parse_lossless, parse_with_sink, CssDiagnosticKind, CssDialect, CssEntryPoint, CssParseFailure,
    CssParseMode, CssSource, CssSyntaxGrammarVersion, LosslessCst, LosslessCstSink, NodeFlags,
    ParseEvent, ParseEventSink, SyntaxElement, SyntaxKind, SyntaxNode, SyntaxToken, TokenKind,
};

use crate::measure_allocations;

#[derive(Default)]
struct RuntimeSink {
    fingerprint: u64,
    starts: usize,
    finishes: usize,
    tokens: usize,
    declarations: usize,
    custom_properties: usize,
    qualified_rules: usize,
    unicode_ranges: usize,
}

// @ai-generated - Pins the cache-visible grammar identity and the extended single taxonomy.
#[test]
fn grammar_version_and_extended_syntax_kinds_are_public_and_total() {
    assert!(CssSyntaxGrammarVersion::CURRENT.as_u32() > 0);
    for kind in [
        SyntaxKind::ClassSelector,
        SyntaxKind::IdSelector,
        SyntaxKind::TypeSelector,
        SyntaxKind::Interpolation,
        SyntaxKind::IndentedBlock,
        SyntaxKind::AmbiguousStatement,
        SyntaxKind::VariableDeclaration,
        SyntaxKind::MixinOrFunctionHeader,
        SyntaxKind::ControlDirective,
    ] {
        assert_eq!(SyntaxKind::from_raw(kind as u16), kind);
        assert_ne!(kind, SyntaxKind::Recovery);
    }
}

impl ParseEventSink for RuntimeSink {
    fn event(&mut self, event: ParseEvent) -> Result<(), verter_css_syntax::CssStructureTooLarge> {
        self.fingerprint = event.fold_fingerprint(self.fingerprint);
        match event {
            ParseEvent::StartNode { kind, .. } => {
                self.starts += 1;
                if kind == SyntaxKind::Declaration {
                    self.declarations += 1;
                }
                if kind == SyntaxKind::CustomPropertyDeclaration {
                    self.custom_properties += 1;
                }
                if kind == SyntaxKind::QualifiedRule {
                    self.qualified_rules += 1;
                }
            }
            ParseEvent::FinishNode { .. } => self.finishes += 1,
            ParseEvent::Token(token) => {
                self.tokens += 1;
                if token.kind() == TokenKind::UnicodeRange {
                    self.unicode_ranges += 1;
                }
            }
            ParseEvent::Diagnostic(_) => {}
        }
        Ok(())
    }
}

#[test]
fn runtime_and_cst_sinks_observe_one_balanced_event_stream() {
    let source = CssSource::new(
        Arc::from("@media screen { .a:is(.b,.c) { color: red; } }"),
        0,
    )
    .unwrap();
    let mut runtime = RuntimeSink::default();
    let runtime_summary = parse_with_sink(
        &source,
        CssDialect::Css,
        CssEntryPoint::Stylesheet,
        CssParseMode::Strict,
        &mut runtime,
    )
    .unwrap();
    let mut cst_sink = LosslessCstSink::new(source.clone());
    let cst_summary = parse_with_sink(
        &source,
        CssDialect::Css,
        CssEntryPoint::Stylesheet,
        CssParseMode::Strict,
        &mut cst_sink,
    )
    .unwrap();
    let cst = cst_sink.finish().unwrap();

    assert_eq!(runtime.starts, runtime.finishes);
    assert!(runtime.starts >= 8);
    assert!(runtime.tokens >= 20);
    assert_eq!(runtime_summary, cst_summary);
    assert_eq!(runtime.fingerprint, cst.event_fingerprint());
    assert_eq!(cst.reconstruct(), source.text());
}

// @ai-generated - Verifies unicode-range descriptor context keeps one allocation-free token authority.
#[test]
fn contextual_unicode_ranges_have_one_lossless_event_authority() {
    let input = concat!(
        r#"@f\6f nt-face {"#,
        " UnIcOdE-RaNgE: U+00A0-00FF, U+4??;",
        r#" unic\6f de-range: U+1?2, U+?A, U+1234567;"#,
        " src: U+4??;",
        " }",
        " @property { unicode-range: U+4??; }",
        " .host { --x: U+4??; }",
    );
    let source = CssSource::new(Arc::from(input), 17).unwrap();
    let mut runtime = RuntimeSink::default();
    let (runtime_result, allocations, bytes) = measure_allocations(|| {
        parse_with_sink(
            &source,
            CssDialect::Css,
            CssEntryPoint::Stylesheet,
            CssParseMode::Strict,
            &mut runtime,
        )
    });
    let runtime_summary = runtime_result.unwrap();
    assert_eq!(allocations, 0, "contextual lexing allocated {bytes} bytes");
    assert_eq!(bytes, 0);
    assert_eq!(runtime.unicode_ranges, 5);

    let mut cst_sink = LosslessCstSink::new(source.clone());
    let cst_summary = parse_with_sink(
        &source,
        CssDialect::Css,
        CssEntryPoint::Stylesheet,
        CssParseMode::Strict,
        &mut cst_sink,
    )
    .unwrap();
    let cst = cst_sink.finish().unwrap();
    assert_eq!(runtime_summary, cst_summary);
    assert_eq!(runtime.fingerprint, cst.event_fingerprint());

    let ranges: Vec<_> = cst
        .tokens()
        .iter()
        .filter(|token| token.kind() == TokenKind::UnicodeRange)
        .map(|token| source.token_text(*token))
        .collect();
    assert_eq!(
        ranges,
        vec!["U+00A0-00FF", "U+4??", "U+1?", "U+?", "U+123456",]
    );

    for marker in ["src:", "@property { unicode-range:", "--x:"] {
        let value_start = input.find(marker).unwrap() + marker.len();
        let value_end = value_start + input[value_start..].find(';').unwrap() + 1;
        let value_tokens: Vec<_> = cst
            .tokens()
            .iter()
            .filter(|token| {
                let start = usize::try_from(token.start - source.origin()).unwrap();
                let end = usize::try_from(token.end - source.origin()).unwrap();
                start >= value_start && end <= value_end && !token.kind().is_trivia()
            })
            .map(|token| (token.kind(), source.token_text(*token)))
            .collect();
        assert_eq!(
            value_tokens,
            vec![
                (TokenKind::Ident, "U"),
                (TokenKind::Number, "+4"),
                (TokenKind::Delim, "?"),
                (TokenKind::Delim, "?"),
                (TokenKind::Semicolon, ";"),
            ],
            "{marker} must retain normal tokenization"
        );
    }
    assert!(cst.diagnostics().is_empty());
    assert_eq!(cst.reconstruct(), input);
}

#[test]
fn wpt_inputs_reconstruct_byte_for_byte() {
    let inputs = [
        "/*lead*/\r\n.a, [data-x=\"x,.fake}\"] { color: url(\"a,b\"); }\n",
        "@unknown foo(bar, {baz});\n@keyframes k { from { opacity: 0 } }\n",
        ".astral-😀 > .café\\? { --theme: { fg: #fff; nested: [a,b] }; }\n",
    ];

    for input in inputs {
        let source = CssSource::new(Arc::from(input), 19).unwrap();
        let cst = parse_lossless(
            source.clone(),
            CssDialect::Css,
            CssEntryPoint::Stylesheet,
            CssParseMode::Recover,
        )
        .unwrap();
        let spans: Vec<_> = cst
            .tokens()
            .iter()
            .map(|token| (token.start, token.end))
            .collect();
        assert_eq!(spans.first().map(|span| span.0), Some(19));
        assert_eq!(
            spans.last().map(|span| span.1),
            Some(19 + input.len() as u32)
        );
        for pair in spans.windows(2) {
            assert_eq!(pair[0].1, pair[1].0, "token coverage must be gap-free");
            assert!(pair[0].0 <= pair[0].1);
        }
        assert_eq!(cst.reconstruct(), input);
    }
}

#[test]
fn custom_properties_remain_balanced_lossless_values() {
    let input = ".a { --theme: { fg: #fff; nested: [a,b] }; color: red; }";
    let source = CssSource::new(Arc::from(input), 0).unwrap();
    let cst = parse_lossless(
        source,
        CssDialect::Css,
        CssEntryPoint::Stylesheet,
        CssParseMode::Strict,
    )
    .unwrap();
    let kinds: Vec<_> = cst.nodes().iter().map(SyntaxNode::kind).collect();

    assert!(kinds.contains(&SyntaxKind::CustomPropertyDeclaration));
    assert!(kinds.contains(&SyntaxKind::ComponentValueBlock));
    assert_eq!(
        kinds
            .iter()
            .filter(|kind| **kind == SyntaxKind::QualifiedRule)
            .count(),
        1,
        "the custom-property block must not become a nested rule"
    );
    assert_eq!(cst.reconstruct(), input);
}

// @ai-generated - Verifies nested conditional blocks retain style-block declaration grammar.
#[test]
fn nested_group_at_rules_accept_declarations_and_nested_rules() {
    let input = concat!(
        ".card {",
        " @media (width > 40rem) {",
        " color: red;",
        " & .child { display: block; }",
        " }",
        " }",
    );
    let source = CssSource::new(Arc::from(input), 0).unwrap();
    let cst = parse_lossless(
        source,
        CssDialect::Css,
        CssEntryPoint::Stylesheet,
        CssParseMode::Strict,
    )
    .unwrap();
    let group = cst
        .nodes()
        .iter()
        .find(|node| node.kind() == SyntaxKind::GroupAtRule)
        .copied()
        .unwrap();
    let group_block = direct_child_nodes(&cst, group)
        .into_iter()
        .find(|node| node.kind() == SyntaxKind::AtRuleBlock)
        .unwrap();
    let block_kinds: Vec<_> = direct_child_nodes(&cst, group_block)
        .into_iter()
        .map(|node| node.kind())
        .collect();

    assert_eq!(
        block_kinds
            .iter()
            .filter(|kind| **kind == SyntaxKind::Declaration)
            .count(),
        1,
        "the conditional block must retain its direct declaration"
    );
    assert_eq!(
        block_kinds
            .iter()
            .filter(|kind| **kind == SyntaxKind::QualifiedRule)
            .count(),
        1,
        "the conditional block must retain its nested rule"
    );
    assert!(
        !block_kinds.contains(&SyntaxKind::Recovery),
        "valid nested conditional contents must not recover"
    );
    assert!(cst.diagnostics().is_empty());
    assert_eq!(cst.reconstruct(), input);
}

// @ai-generated - Verifies style-block context does not leak into top-level groups or keyframes.
#[test]
fn conditional_style_context_does_not_change_rule_list_grammar() {
    for input in [
        "@media (width > 40rem) { color: red; }",
        ".card { @keyframes fade { opacity: 0; } }",
    ] {
        let source = CssSource::new(Arc::from(input), 0).unwrap();
        let failure = match parse_lossless(
            source,
            CssDialect::Css,
            CssEntryPoint::Stylesheet,
            CssParseMode::Strict,
        ) {
            Ok(_) => panic!("rule-list declaration unexpectedly parsed as valid"),
            Err(failure) => failure,
        };

        assert!(matches!(
            failure,
            CssParseFailure::Diagnostic(diagnostic)
                if diagnostic.kind == CssDiagnosticKind::ExpectedRuleBlock
        ));
    }
}

// @ai-generated - Verifies custom-property identity follows decoded CSS identifier semantics.
#[test]
fn escaped_custom_property_names_use_decoded_identifier_semantics() {
    let input = r#".host { \-\-theme: { foreground: red }; }"#;
    let source = CssSource::new(Arc::from(input), 0).unwrap();
    let cst = parse_lossless(
        source,
        CssDialect::Css,
        CssEntryPoint::Stylesheet,
        CssParseMode::Strict,
    )
    .unwrap();
    let kinds: Vec<_> = cst.nodes().iter().map(SyntaxNode::kind).collect();

    assert_eq!(
        kinds
            .iter()
            .filter(|kind| **kind == SyntaxKind::CustomPropertyDeclaration)
            .count(),
        1
    );
    assert!(!kinds.contains(&SyntaxKind::Declaration));
    assert!(kinds.contains(&SyntaxKind::ComponentValueBlock));
    assert!(cst.diagnostics().is_empty());
    assert_eq!(cst.reconstruct(), input);
}

// @ai-generated - Verifies custom-property-like identifiers remain valid nested type selectors.
#[test]
fn custom_property_like_names_with_rule_blocks_are_nested_selectors() {
    let input = r#".host { --widget { color: red; } \-\-escaped { color: blue; } }"#;
    let source = CssSource::new(Arc::from(input), 0).unwrap();
    let cst = parse_lossless(
        source,
        CssDialect::Css,
        CssEntryPoint::Stylesheet,
        CssParseMode::Strict,
    )
    .unwrap();
    let kinds: Vec<_> = cst.nodes().iter().map(SyntaxNode::kind).collect();

    assert_eq!(
        kinds
            .iter()
            .filter(|kind| **kind == SyntaxKind::QualifiedRule)
            .count(),
        3,
        "the host rule and both custom-property-like type selectors are rules"
    );
    assert!(
        !kinds.contains(&SyntaxKind::CustomPropertyDeclaration),
        "a block without a declaration colon must not be forced into custom-property grammar"
    );
    assert_eq!(
        kinds
            .iter()
            .filter(|kind| **kind == SyntaxKind::Declaration)
            .count(),
        2
    );
    assert!(cst.diagnostics().is_empty());
    assert_eq!(cst.reconstruct(), input);
}

// @ai-generated - Verifies only an immediate declaration colon triggers custom-property subtraction.
#[test]
fn custom_property_like_selectors_with_later_colons_remain_qualified_rules() {
    let input = concat!(
        ".host {",
        " --widget > .child:hover { color: red; }",
        r#" \-\-escaped > .child:hover { color: orange; }"#,
        " --widget.active:hover { color: yellow; }",
        r#" \-\-escaped.active:hover { color: green; }"#,
        " --widget.active:where(.enabled) { color: blue; }",
        r#" \-\-escaped.active:where(.enabled) { color: violet; }"#,
        " }",
    );
    let source = CssSource::new(Arc::from(input), 0).unwrap();
    let cst = parse_lossless(
        source,
        CssDialect::Css,
        CssEntryPoint::Stylesheet,
        CssParseMode::Strict,
    )
    .unwrap();
    let kinds: Vec<_> = cst.nodes().iter().map(SyntaxNode::kind).collect();

    assert_eq!(
        kinds
            .iter()
            .filter(|kind| **kind == SyntaxKind::QualifiedRule)
            .count(),
        7,
        "the host and all raw/escaped selector variants must remain rules"
    );
    assert_eq!(
        kinds
            .iter()
            .filter(|kind| **kind == SyntaxKind::Declaration)
            .count(),
        6
    );
    assert!(
        !kinds.contains(&SyntaxKind::CustomPropertyDeclaration),
        "a later pseudo colon must not retroactively make the leading identifier a custom property"
    );
    assert!(!kinds.contains(&SyntaxKind::Recovery));
    assert!(cst.diagnostics().is_empty());
    assert_eq!(cst.reconstruct(), input);
}

// @ai-generated - Verifies trivia does not break an immediate custom-property declaration colon.
#[test]
fn immediate_custom_property_colons_ignore_whitespace_and_comments() {
    let input = concat!(
        ".host {",
        " --raw-function /**/ : /**/ var(--fallback);",
        r#" \-\-escaped-function/**/:rgb(1 2 3);"#,
        " --raw-block /* before colon */ : /**/ { foreground: red };",
        r#" \-\-escaped-block/**/ :{ foreground: blue };"#,
        " }",
    );
    let source = CssSource::new(Arc::from(input), 0).unwrap();
    let cst = parse_lossless(
        source,
        CssDialect::Css,
        CssEntryPoint::Stylesheet,
        CssParseMode::Strict,
    )
    .unwrap();
    let kinds: Vec<_> = cst.nodes().iter().map(SyntaxNode::kind).collect();

    assert_eq!(
        kinds
            .iter()
            .filter(|kind| **kind == SyntaxKind::CustomPropertyDeclaration)
            .count(),
        4
    );
    assert_eq!(
        kinds
            .iter()
            .filter(|kind| **kind == SyntaxKind::QualifiedRule)
            .count(),
        1,
        "all immediate-colon cases must remain declarations"
    );
    assert!(!kinds.contains(&SyntaxKind::Declaration));
    assert!(kinds.contains(&SyntaxKind::Function));
    assert!(kinds.contains(&SyntaxKind::ComponentValueBlock));
    assert!(!kinds.contains(&SyntaxKind::Recovery));
    assert!(cst.diagnostics().is_empty());
    assert_eq!(cst.reconstruct(), input);
}

// @ai-generated - Verifies ordinary CSS declaration names require trivia-transparent colon adjacency.
#[test]
fn immediate_css_declaration_colons_ignore_whitespace_and_comments() {
    let input = r#".host { color /**/ : red; c\6flor/**/: blue; }"#;
    let source = CssSource::new(Arc::from(input), 0).unwrap();
    let cst = parse_lossless(
        source,
        CssDialect::Css,
        CssEntryPoint::Stylesheet,
        CssParseMode::Strict,
    )
    .unwrap();
    let kinds: Vec<_> = cst.nodes().iter().map(SyntaxNode::kind).collect();

    assert_eq!(
        kinds
            .iter()
            .filter(|kind| **kind == SyntaxKind::Declaration)
            .count(),
        2
    );
    assert!(!kinds.contains(&SyntaxKind::CustomPropertyDeclaration));
    assert!(!kinds.contains(&SyntaxKind::Recovery));
    assert!(cst.diagnostics().is_empty());
    assert_eq!(cst.reconstruct(), input);
}

// @ai-generated - Verifies non-trivia between a CSS declaration name and colon is rejected.
#[test]
fn css_declaration_names_reject_intervening_non_trivia() {
    for input in [".host { color junk: red; }", ".host { color[bad]: red; }"] {
        let strict = match parse_lossless(
            CssSource::new(Arc::from(input), 0).unwrap(),
            CssDialect::Css,
            CssEntryPoint::Stylesheet,
            CssParseMode::Strict,
        ) {
            Ok(_) => panic!("malformed declaration unexpectedly passed strict parsing"),
            Err(failure) => failure,
        };
        assert!(matches!(
            strict,
            CssParseFailure::Diagnostic(diagnostic)
                if diagnostic.kind == CssDiagnosticKind::ExpectedRuleBlock
        ));

        let recovered = parse_lossless(
            CssSource::new(Arc::from(input), 0).unwrap(),
            CssDialect::Css,
            CssEntryPoint::Stylesheet,
            CssParseMode::Recover,
        )
        .unwrap();
        assert!(recovered
            .diagnostics()
            .iter()
            .any(|diagnostic| diagnostic.kind == CssDiagnosticKind::ExpectedRuleBlock));
        assert!(recovered
            .nodes()
            .iter()
            .any(|node| node.kind() == SyntaxKind::Recovery));
        assert!(!recovered.nodes().iter().any(|node| matches!(
            node.kind(),
            SyntaxKind::Declaration | SyntaxKind::CustomPropertyDeclaration
        )));
        assert_eq!(recovered.reconstruct(), input);
    }
}

// @ai-generated - Verifies ordinary declarations reject mixed top-level block values.
#[test]
fn ordinary_declaration_block_values_reject_mixed_top_level_values() {
    for candidate in [
        "color: {} tail;",
        "color: head {};",
        "color: head {} tail;",
        "color: {} {};",
        "color:/**/{} tail;",
        "color: fn() {};",
        "color: () {};",
        "color: [] {};",
    ] {
        let input = format!(".host {{ {candidate} width: 1px; }}");
        let strict = parse_lossless(
            CssSource::new(Arc::from(input.as_str()), 0).unwrap(),
            CssDialect::Css,
            CssEntryPoint::Stylesheet,
            CssParseMode::Strict,
        );
        if let Ok(cst) = strict {
            let declarations: Vec<_> = cst
                .nodes()
                .iter()
                .filter(|node| node.kind() == SyntaxKind::Declaration)
                .map(|node| cst.source().slice(node.span()))
                .collect();
            assert_eq!(
                declarations,
                vec!["width: 1px;"],
                "{candidate} must not publish declaration structure in strict mode"
            );
        }

        let recovered = parse_lossless(
            CssSource::new(Arc::from(input.as_str()), 0).unwrap(),
            CssDialect::Css,
            CssEntryPoint::Stylesheet,
            CssParseMode::Recover,
        )
        .unwrap();
        let declarations: Vec<_> = recovered
            .nodes()
            .iter()
            .filter(|node| node.kind() == SyntaxKind::Declaration)
            .map(|node| recovered.source().slice(node.span()))
            .collect();
        assert_eq!(
            declarations,
            vec!["width: 1px;"],
            "{candidate} must not publish declaration structure in recover mode"
        );
        assert_eq!(recovered.reconstruct(), input);
    }
}

// @ai-generated - Verifies semicolons remain prelude content outside declaration-bearing blocks.
#[test]
fn rule_list_qualified_rule_preludes_retain_semicolons() {
    for input in [
        "foo;bar { color: red; }",
        "@media screen { foo;bar { color: red; } }",
    ] {
        let cst = parse_lossless(
            CssSource::new(Arc::from(input), 0).unwrap(),
            CssDialect::Css,
            CssEntryPoint::Stylesheet,
            CssParseMode::Strict,
        )
        .unwrap();

        assert_eq!(
            cst.nodes()
                .iter()
                .filter(|node| node.kind() == SyntaxKind::QualifiedRule)
                .count(),
            1
        );
        assert_eq!(
            cst.nodes()
                .iter()
                .filter(|node| node.kind() == SyntaxKind::Declaration)
                .count(),
            1
        );
        assert!(!cst
            .nodes()
            .iter()
            .any(|node| node.kind() == SyntaxKind::Recovery));
        assert!(cst.diagnostics().is_empty());
        assert_eq!(cst.reconstruct(), input);
    }
}

// @ai-generated - Verifies a sole block and terminal !important remain ordinary values.
#[test]
fn ordinary_declaration_values_allow_sole_block_and_important() {
    let input = concat!(
        ".host {",
        " color:/**/{};",
        r#" background: /**/ { foreground: red } !/**/\69mportant;"#,
        " border: fn({});",
        " }",
    );
    let cst = parse_lossless(
        CssSource::new(Arc::from(input), 0).unwrap(),
        CssDialect::Css,
        CssEntryPoint::Stylesheet,
        CssParseMode::Strict,
    )
    .unwrap();

    assert_eq!(
        cst.nodes()
            .iter()
            .filter(|node| node.kind() == SyntaxKind::Declaration)
            .count(),
        3
    );
    assert_eq!(
        cst.nodes()
            .iter()
            .filter(|node| node.kind() == SyntaxKind::ComponentValueBlock)
            .count(),
        3
    );
    assert!(!cst
        .nodes()
        .iter()
        .any(|node| node.kind() == SyntaxKind::Recovery));
    assert!(cst.diagnostics().is_empty());
    assert_eq!(cst.reconstruct(), input);
}

// @ai-generated - Verifies custom properties retain arbitrary mixed balanced block values.
#[test]
fn custom_properties_allow_mixed_top_level_block_values() {
    let input = concat!(
        ".host {",
        " --raw: head {} tail [];",
        r#" \-\-escaped: {} tail {} !not-important;"#,
        " }",
    );
    let cst = parse_lossless(
        CssSource::new(Arc::from(input), 0).unwrap(),
        CssDialect::Css,
        CssEntryPoint::Stylesheet,
        CssParseMode::Strict,
    )
    .unwrap();

    assert_eq!(
        cst.nodes()
            .iter()
            .filter(|node| node.kind() == SyntaxKind::CustomPropertyDeclaration)
            .count(),
        2
    );
    assert!(!cst
        .nodes()
        .iter()
        .any(|node| node.kind() == SyntaxKind::Declaration));
    assert_eq!(
        cst.nodes()
            .iter()
            .filter(|node| node.kind() == SyntaxKind::ComponentValueBlock)
            .count(),
        4
    );
    assert!(!cst
        .nodes()
        .iter()
        .any(|node| node.kind() == SyntaxKind::Recovery));
    assert!(cst.diagnostics().is_empty());
    assert_eq!(cst.reconstruct(), input);
}

// @ai-generated - Verifies explicit SCSS/Less variable declaration names retain their grammar.
#[test]
fn dialect_variable_declaration_names_retain_trivia_before_colons() {
    for (dialect, input) in [
        (CssDialect::Scss, ".host { $tone/**/: red; }"),
        (CssDialect::Less, ".host { @tone/**/: blue; }"),
    ] {
        let cst = parse_lossless(
            CssSource::new(Arc::from(input), 0).unwrap(),
            dialect,
            CssEntryPoint::Stylesheet,
            CssParseMode::Strict,
        )
        .unwrap();
        assert_eq!(
            cst.nodes()
                .iter()
                .filter(|node| node.kind() == SyntaxKind::Declaration)
                .count(),
            1
        );
        assert!(!cst
            .nodes()
            .iter()
            .any(|node| node.kind() == SyntaxKind::Recovery));
        assert!(cst.diagnostics().is_empty());
        assert_eq!(cst.reconstruct(), input);
    }
}

// @ai-generated - Verifies root SCSS variables share the established Less declaration path.
#[test]
fn root_dialect_variables_share_declaration_grammar() {
    for (dialect, input, expected_declarations) in [
        (
            CssDialect::Scss,
            "$tone /**/ : red; $tokens: head {} tail [];",
            ["$tone /**/ : red;", "$tokens: head {} tail [];"],
        ),
        (
            CssDialect::Less,
            "@tone /**/ : blue; @tokens: head {} tail [];",
            ["@tone /**/ : blue;", "@tokens: head {} tail [];"],
        ),
    ] {
        for mode in [CssParseMode::Strict, CssParseMode::Recover] {
            let cst = parse_lossless(
                CssSource::new(Arc::from(input), 0).unwrap(),
                dialect,
                CssEntryPoint::Stylesheet,
                mode,
            )
            .unwrap();
            let declarations: Vec<_> = cst
                .nodes()
                .iter()
                .filter(|node| node.kind() == SyntaxKind::Declaration)
                .map(|node| cst.source().slice(node.span()))
                .collect();

            assert_eq!(declarations, expected_declarations);
            assert_eq!(
                cst.nodes()
                    .iter()
                    .filter(|node| node.kind() == SyntaxKind::ComponentValueBlock)
                    .count(),
                2
            );
            assert!(!cst
                .nodes()
                .iter()
                .any(|node| node.kind() == SyntaxKind::QualifiedRule));
            assert!(!cst
                .nodes()
                .iter()
                .any(|node| node.kind() == SyntaxKind::Recovery));
            assert!(cst.diagnostics().is_empty());
            assert_eq!(cst.reconstruct(), input);
        }
    }
}

// @ai-generated - Verifies root declaration routing remains token-, dialect-, and adjacency-bounded.
#[test]
fn root_variable_declarations_reject_intervening_non_trivia_and_css_idents() {
    for (dialect, input) in [
        (CssDialect::Scss, "$tone junk: red;"),
        (CssDialect::Css, "color: red;"),
        (CssDialect::Css, "$tone: red;"),
        (CssDialect::Less, "$tone: red;"),
    ] {
        let strict = parse_lossless(
            CssSource::new(Arc::from(input), 0).unwrap(),
            dialect,
            CssEntryPoint::Stylesheet,
            CssParseMode::Strict,
        );
        assert!(matches!(
            strict,
            Err(CssParseFailure::Diagnostic(diagnostic))
                if diagnostic.kind == CssDiagnosticKind::ExpectedRuleBlock
        ));

        let recovered = parse_lossless(
            CssSource::new(Arc::from(input), 0).unwrap(),
            dialect,
            CssEntryPoint::Stylesheet,
            CssParseMode::Recover,
        )
        .unwrap();
        assert!(!recovered
            .nodes()
            .iter()
            .any(|node| node.kind() == SyntaxKind::Declaration));
        assert!(recovered
            .nodes()
            .iter()
            .any(|node| node.kind() == SyntaxKind::Recovery));
        assert!(recovered
            .diagnostics()
            .iter()
            .any(|diagnostic| diagnostic.kind == CssDiagnosticKind::ExpectedRuleBlock));
        assert_eq!(recovered.reconstruct(), input);
    }
}

// @ai-generated - Verifies SCSS variables in shared rule lists resume following rules.
#[test]
fn scss_variables_in_group_and_keyframe_rule_lists_resume_following_rules() {
    for (input, expected_declarations, expected_rule) in [
        (
            "@media screen { $tone: red; .after { color:red; } }",
            ["$tone: red;", "color:red;"],
            ".after { color:red; }",
        ),
        (
            "@keyframes fade { $tone: blue; from { opacity:0; } }",
            ["$tone: blue;", "opacity:0;"],
            "from { opacity:0; }",
        ),
    ] {
        for mode in [CssParseMode::Strict, CssParseMode::Recover] {
            let cst = parse_lossless(
                CssSource::new(Arc::from(input), 0).unwrap(),
                CssDialect::Scss,
                CssEntryPoint::Stylesheet,
                mode,
            )
            .unwrap();
            let declarations: Vec<_> = cst
                .nodes()
                .iter()
                .filter(|node| node.kind() == SyntaxKind::Declaration)
                .map(|node| cst.source().slice(node.span()))
                .collect();
            let qualified_rules: Vec<_> = cst
                .nodes()
                .iter()
                .filter(|node| node.kind() == SyntaxKind::QualifiedRule)
                .map(|node| cst.source().slice(node.span()))
                .collect();

            assert_eq!(declarations, expected_declarations);
            assert_eq!(qualified_rules, vec![expected_rule]);
            assert!(!cst
                .nodes()
                .iter()
                .any(|node| node.kind() == SyntaxKind::Recovery));
            assert!(cst.diagnostics().is_empty());
            assert_eq!(cst.reconstruct(), input);
        }
    }
}

// @ai-generated - Verifies functional pseudos disambiguate nested rules from declarations.
#[test]
fn nested_functional_pseudo_selectors_remain_qualified_rules() {
    let input = concat!(
        ".host {",
        " article:/**/is(.a, .b) { color: red; }",
        " section:where(.active) { color: blue; }",
        " main:has(> .child) { display: block; }",
        " li:nth-child(2n + 1 of .item) { opacity: .5; }",
        " pane:state(foo) { visibility: hidden; }",
        " }",
    );
    let source = CssSource::new(Arc::from(input), 0).unwrap();
    let cst = parse_lossless(
        source,
        CssDialect::Css,
        CssEntryPoint::Stylesheet,
        CssParseMode::Strict,
    )
    .unwrap();
    let kinds: Vec<_> = cst.nodes().iter().map(SyntaxNode::kind).collect();

    assert_eq!(
        kinds
            .iter()
            .filter(|kind| **kind == SyntaxKind::QualifiedRule)
            .count(),
        6,
        "the host and all five functional-pseudo selectors must remain rules"
    );
    assert_eq!(
        kinds
            .iter()
            .filter(|kind| **kind == SyntaxKind::Declaration)
            .count(),
        5
    );
    assert!(!kinds.contains(&SyntaxKind::CustomPropertyDeclaration));
    assert_eq!(
        kinds
            .iter()
            .filter(|kind| **kind == SyntaxKind::PseudoSelectorList)
            .count(),
        3
    );
    assert!(kinds.contains(&SyntaxKind::NthSelector));
    assert!(kinds.contains(&SyntaxKind::UnknownPseudoFunction));
    assert!(!kinds.contains(&SyntaxKind::Recovery));
    assert!(cst.diagnostics().is_empty());
    assert_eq!(cst.reconstruct(), input);
}

// @ai-generated - Verifies raw and escaped custom properties retain value grammar.
#[test]
fn functional_and_block_custom_property_values_remain_declarations() {
    let input = concat!(
        r#".host {"#,
        r#" --theme:var(--fallback){ foreground: red };"#,
        r#" \-\-accent:rgb(1 2 3);"#,
        r#" --palette: { foreground: red };"#,
        r#" --widget:where(.active){ color: blue };"#,
        r#" }"#,
    );
    let source = CssSource::new(Arc::from(input), 0).unwrap();
    let cst = parse_lossless(
        source,
        CssDialect::Css,
        CssEntryPoint::Stylesheet,
        CssParseMode::Strict,
    )
    .unwrap();
    let kinds: Vec<_> = cst.nodes().iter().map(SyntaxNode::kind).collect();

    assert_eq!(
        kinds
            .iter()
            .filter(|kind| **kind == SyntaxKind::CustomPropertyDeclaration)
            .count(),
        4
    );
    assert_eq!(
        kinds
            .iter()
            .filter(|kind| **kind == SyntaxKind::QualifiedRule)
            .count(),
        1,
        "custom-property values must not be reclassified as nested rules"
    );
    assert!(!kinds.contains(&SyntaxKind::Declaration));
    assert!(kinds.contains(&SyntaxKind::Function));
    assert!(kinds.contains(&SyntaxKind::ComponentValueBlock));
    assert!(cst.diagnostics().is_empty());
    assert_eq!(cst.reconstruct(), input);
}

#[test]
fn rule_blocks_own_both_delimiters_at_nonzero_origins() {
    for (input, rule_kind, block_kind, expected) in [
        (
            ".a{}",
            SyntaxKind::QualifiedRule,
            SyntaxKind::RuleBlock,
            "{}",
        ),
        (
            ".a{color:red}",
            SyntaxKind::QualifiedRule,
            SyntaxKind::RuleBlock,
            "{color:red}",
        ),
        (
            "@media x{}",
            SyntaxKind::GroupAtRule,
            SyntaxKind::AtRuleBlock,
            "{}",
        ),
        (
            "@media x{.a{}}",
            SyntaxKind::GroupAtRule,
            SyntaxKind::AtRuleBlock,
            "{.a{}}",
        ),
    ] {
        let source = CssSource::new(Arc::from(input), 37).unwrap();
        let cst = parse_lossless(
            source.clone(),
            CssDialect::Css,
            CssEntryPoint::Stylesheet,
            CssParseMode::Strict,
        )
        .unwrap();
        assert!(cst.nodes().iter().any(|node| node.kind() == rule_kind));
        let block = cst
            .nodes()
            .iter()
            .find(|node| node.kind() == block_kind)
            .copied()
            .unwrap();
        assert_eq!(source.slice(block.span()), expected);
        let children = cst.children(block);
        let first = children.first().copied().unwrap();
        let last = children.last().copied().unwrap();
        assert!(!first.is_node());
        assert!(!last.is_node());
        assert_eq!(source.token_text(cst.tokens()[first.index() as usize]), "{");
        assert_eq!(source.token_text(cst.tokens()[last.index() as usize]), "}");
    }
}

#[test]
fn stylesheet_and_component_value_nodes_are_typed_and_lossless() {
    let input = concat!(
        "@media screen { .a { color: rgb(1 2 3 / .5); } }",
        "@font-face { font-family: \"x\"; src: url(x.woff2); }",
        "@keyframes fade { from { opacity: 0 } to { opacity: 1 } }",
        "@future foo(bar, {raw:value});",
        ".parent { & > .child { data: [a,b]; } article:hover { color: blue; } }",
    );
    let source = CssSource::new(Arc::from(input), 0).unwrap();
    let cst = parse_lossless(
        source,
        CssDialect::Css,
        CssEntryPoint::Stylesheet,
        CssParseMode::Strict,
    )
    .unwrap();
    let kinds: Vec<_> = cst.nodes().iter().map(SyntaxNode::kind).collect();

    for required in [
        SyntaxKind::GroupAtRule,
        SyntaxKind::DescriptorAtRule,
        SyntaxKind::KeyframesAtRule,
        SyntaxKind::UnknownAtRule,
        SyntaxKind::Declaration,
        SyntaxKind::Function,
        SyntaxKind::ComponentValueBlock,
        SyntaxKind::QualifiedRule,
    ] {
        assert!(
            kinds.contains(&required),
            "missing structural node {required:?}"
        );
    }
    assert!(
        kinds
            .iter()
            .filter(|kind| **kind == SyntaxKind::QualifiedRule)
            .count()
            >= 5,
        "type-selector nesting must remain a nested qualified rule"
    );
    assert!(cst
        .tokens()
        .iter()
        .any(|token| source_text(&cst, *token) == "\"x\""));
    assert_eq!(cst.reconstruct(), input);
}

// @ai-generated - Verifies unknown at-rule blocks remain opaque balanced values.
#[test]
fn unknown_at_rule_blocks_are_opaque_balanced_component_values() {
    let input = "@future x { foo; {bar:baz} fn([q]); }";
    let expected_tokens = vec![
        (
            verter_css_syntax::TokenKind::AtKeyword,
            0,
            31,
            38,
            "@future",
        ),
        (verter_css_syntax::TokenKind::Whitespace, 1, 38, 39, " "),
        (verter_css_syntax::TokenKind::Ident, 0, 39, 40, "x"),
        (verter_css_syntax::TokenKind::Whitespace, 1, 40, 41, " "),
        (verter_css_syntax::TokenKind::LeftBrace, 0, 41, 42, "{"),
        (verter_css_syntax::TokenKind::Whitespace, 1, 42, 43, " "),
        (verter_css_syntax::TokenKind::Ident, 0, 43, 46, "foo"),
        (verter_css_syntax::TokenKind::Semicolon, 0, 46, 47, ";"),
        (verter_css_syntax::TokenKind::Whitespace, 1, 47, 48, " "),
        (verter_css_syntax::TokenKind::LeftBrace, 0, 48, 49, "{"),
        (verter_css_syntax::TokenKind::Ident, 0, 49, 52, "bar"),
        (verter_css_syntax::TokenKind::Colon, 0, 52, 53, ":"),
        (verter_css_syntax::TokenKind::Ident, 0, 53, 56, "baz"),
        (verter_css_syntax::TokenKind::RightBrace, 0, 56, 57, "}"),
        (verter_css_syntax::TokenKind::Whitespace, 1, 57, 58, " "),
        (verter_css_syntax::TokenKind::Function, 0, 58, 61, "fn("),
        (verter_css_syntax::TokenKind::LeftBracket, 0, 61, 62, "["),
        (verter_css_syntax::TokenKind::Ident, 0, 62, 63, "q"),
        (verter_css_syntax::TokenKind::RightBracket, 0, 63, 64, "]"),
        (verter_css_syntax::TokenKind::RightParen, 0, 64, 65, ")"),
        (verter_css_syntax::TokenKind::Semicolon, 0, 65, 66, ";"),
        (verter_css_syntax::TokenKind::Whitespace, 1, 66, 67, " "),
        (verter_css_syntax::TokenKind::RightBrace, 0, 67, 68, "}"),
    ];

    for mode in [CssParseMode::Strict, CssParseMode::Recover] {
        let source = CssSource::new(Arc::from(input), 31).unwrap();
        let cst = parse_lossless(
            source.clone(),
            CssDialect::Css,
            CssEntryPoint::Stylesheet,
            mode,
        )
        .unwrap();
        let actual_tokens: Vec<_> = cst
            .tokens()
            .iter()
            .map(|token| {
                (
                    token.kind(),
                    token.flags,
                    token.start,
                    token.end,
                    source.token_text(*token),
                )
            })
            .collect();
        let unknown = cst
            .nodes()
            .iter()
            .find(|node| node.kind() == SyntaxKind::UnknownAtRule)
            .unwrap();
        let block = cst
            .nodes()
            .iter()
            .find(|node| node.kind() == SyntaxKind::AtRuleBlock)
            .unwrap();
        let component_spans: Vec<_> = cst
            .nodes()
            .iter()
            .filter(|node| node.kind() == SyntaxKind::ComponentValueBlock)
            .map(|node| node.span())
            .collect();

        assert_eq!(actual_tokens, expected_tokens);
        assert_eq!(unknown.span(), verter_span::Span::new(31, 68));
        assert_eq!(block.span(), verter_span::Span::new(41, 68));
        assert_eq!(
            component_spans,
            vec![
                verter_span::Span::new(48, 57),
                verter_span::Span::new(61, 64),
            ]
        );
        assert!(cst.diagnostics().is_empty());
        assert!(!cst.nodes().iter().any(|node| {
            matches!(
                node.kind(),
                SyntaxKind::Declaration
                    | SyntaxKind::CustomPropertyDeclaration
                    | SyntaxKind::QualifiedRule
                    | SyntaxKind::Recovery
            )
        }));
        assert_eq!(cst.reconstruct(), input);
    }
}

fn source_text(cst: &verter_css_syntax::LosslessCst, token: SyntaxToken) -> &str {
    cst.source().token_text(token)
}

fn direct_child_nodes(cst: &LosslessCst, node: SyntaxNode) -> Vec<SyntaxNode> {
    cst.children(node)
        .iter()
        .filter(|element| element.is_node())
        .map(|element| cst.nodes()[element.index() as usize])
        .collect()
}

#[test]
fn record_layout_is_12_20_4_bytes() {
    assert_eq!(size_of::<SyntaxToken>(), 12);
    assert_eq!(size_of::<SyntaxNode>(), 20);
    assert_eq!(size_of::<SyntaxElement>(), 4);
    assert_eq!(size_of::<NodeFlags>(), 2);
}

#[test]
fn runtime_sink_shallow_parse_allocates_zero() {
    let source = CssSource::new(
        Arc::from(".a > b { color: rgb(1 2 3); margin: calc(1px + 2%); }"),
        0,
    )
    .unwrap();
    let mut sink = RuntimeSink::default();
    let (result, allocations, bytes) = measure_allocations(|| {
        parse_with_sink(
            &source,
            CssDialect::Css,
            CssEntryPoint::Stylesheet,
            CssParseMode::Strict,
            &mut sink,
        )
    });

    assert!(result.is_ok());
    assert_eq!(allocations, 0, "runtime parser allocated {bytes} bytes");
    assert_eq!(bytes, 0);
}

#[test]
fn runtime_sink_escaped_classification_allocates_zero() {
    let source = CssSource::new(
        Arc::from("@m\\65 dia x { .a:\\69s(.b,.c) { color: red; } }"),
        0,
    )
    .unwrap();
    let mut sink = RuntimeSink::default();
    let (result, allocations, bytes) = measure_allocations(|| {
        parse_with_sink(
            &source,
            CssDialect::Css,
            CssEntryPoint::Stylesheet,
            CssParseMode::Strict,
            &mut sink,
        )
    });

    assert!(result.is_ok());
    assert_eq!(
        allocations, 0,
        "escaped classification allocated {bytes} bytes"
    );
    assert_eq!(bytes, 0);
}

// @ai-generated - Verifies decoded custom-property classification stays allocation-free.
#[test]
fn runtime_sink_escaped_custom_property_classification_allocates_zero() {
    let source =
        CssSource::new(Arc::from(r#".host { \-\-theme: { foreground: red }; }"#), 0).unwrap();
    let mut sink = RuntimeSink::default();
    let (result, allocations, bytes) = measure_allocations(|| {
        parse_with_sink(
            &source,
            CssDialect::Css,
            CssEntryPoint::Stylesheet,
            CssParseMode::Strict,
            &mut sink,
        )
    });

    assert!(result.is_ok());
    assert_eq!(sink.custom_properties, 1);
    assert_eq!(
        allocations, 0,
        "escaped custom-property classification allocated {bytes} bytes"
    );
    assert_eq!(bytes, 0);
}

// @ai-generated - Verifies functional-pseudo disambiguation remains allocation-free.
#[test]
fn runtime_sink_functional_pseudo_disambiguation_allocates_zero() {
    let source = CssSource::new(
        Arc::from(concat!(
            r#".host {"#,
            r#" article:is(.a) { color: red; }"#,
            r#" widget:where(.active) { color: blue; }"#,
            r#" \-\-theme:var(--fallback){ foreground: red };"#,
            r#" }"#,
        )),
        0,
    )
    .unwrap();
    let mut sink = RuntimeSink::default();
    let (result, allocations, bytes) = measure_allocations(|| {
        parse_with_sink(
            &source,
            CssDialect::Css,
            CssEntryPoint::Stylesheet,
            CssParseMode::Strict,
            &mut sink,
        )
    });

    assert!(result.is_ok());
    assert_eq!(sink.qualified_rules, 3);
    assert_eq!(sink.custom_properties, 1);
    assert_eq!(
        allocations, 0,
        "functional-pseudo disambiguation allocated {bytes} bytes"
    );
    assert_eq!(bytes, 0);
}

// @ai-generated - Verifies mixed block-value admission and recovery stay allocation-free.
#[test]
fn runtime_sink_block_value_admission_allocates_zero() {
    let source = CssSource::new(
        Arc::from(concat!(
            ".host {",
            " color:/**/{} tail;",
            " width:{} !important;",
            " --x:head {} tail;",
            " height:1px;",
            " }",
        )),
        0,
    )
    .unwrap();
    let mut sink = RuntimeSink::default();
    let (result, allocations, bytes) = measure_allocations(|| {
        parse_with_sink(
            &source,
            CssDialect::Css,
            CssEntryPoint::Stylesheet,
            CssParseMode::Recover,
            &mut sink,
        )
    });

    assert!(result.is_ok());
    assert_eq!(sink.declarations, 2);
    assert_eq!(sink.custom_properties, 1);
    assert_eq!(
        allocations, 0,
        "block-value admission allocated {bytes} bytes"
    );
    assert_eq!(bytes, 0);
}

#[test]
fn flat_cst_allocations_stay_within_the_retained_table_budget() {
    let input = ".a,.b:is(.c,.d){--x:{a:[b,c]};color:rgb(1 2 3 / .5)}\n".repeat(32);
    let source = CssSource::new(Arc::from(input.clone()), 0).unwrap();
    let (result, allocations, bytes) = measure_allocations(|| {
        parse_lossless(
            source,
            CssDialect::Css,
            CssEntryPoint::Stylesheet,
            CssParseMode::Strict,
        )
    });
    let cst = result.unwrap();

    assert!(allocations <= 40, "flat CST made {allocations} allocations");
    assert!(
        bytes <= 40 * input.len() + 4096,
        "flat CST allocated {bytes} bytes for {} source bytes",
        input.len()
    );
    assert_eq!(cst.reconstruct(), input);
}

fn json_object_array<'a>(object: &'a str, field: &str) -> Vec<&'a str> {
    let marker = format!("\"{field}\":[");
    let array_start = object.find(&marker).unwrap() + marker.len();
    let bytes = object.as_bytes();
    let mut objects = Vec::new();
    let mut depth = 0usize;
    let mut object_start = None;
    let mut in_string = false;
    let mut escaped = false;
    for index in array_start..bytes.len() {
        let byte = bytes[index];
        if in_string {
            if escaped {
                escaped = false;
            } else if byte == b'\\' {
                escaped = true;
            } else if byte == b'"' {
                in_string = false;
            }
            continue;
        }
        match byte {
            b'"' => in_string = true,
            b'{' => {
                if depth == 0 {
                    object_start = Some(index);
                }
                depth += 1;
            }
            b'}' => {
                depth -= 1;
                if depth == 0 {
                    objects.push(&object[object_start.take().unwrap()..=index]);
                }
            }
            b']' if depth == 0 => break,
            _ => {}
        }
    }
    objects
}

fn json_optional_string_field<'a>(object: &'a str, field: &str) -> Option<&'a str> {
    let marker = format!("\"{field}\":");
    let value_start = object.find(&marker).unwrap() + marker.len();
    if object[value_start..].starts_with("null") {
        return None;
    }
    let string_start = value_start + 1;
    let mut escaped = false;
    for (offset, byte) in object.as_bytes()[string_start..]
        .iter()
        .copied()
        .enumerate()
    {
        if escaped {
            escaped = false;
        } else if byte == b'\\' {
            escaped = true;
        } else if byte == b'"' {
            return Some(&object[string_start..string_start + offset]);
        }
    }
    panic!("unterminated JSON string field {field}");
}

#[test]
fn production_dependency_closure_is_framework_neutral() {
    let manifest = concat!(env!("CARGO_MANIFEST_DIR"), "/Cargo.toml");
    let output = Command::new(env!("CARGO"))
        .args([
            "tree",
            "--manifest-path",
            manifest,
            "--package",
            "verter_css_syntax",
            "--all-features",
            "--edges",
            "normal,build",
            "--depth",
            "1",
            "--prefix",
            "none",
        ])
        .output()
        .unwrap();
    assert!(output.status.success(), "cargo tree failed");
    let tree = String::from_utf8(output.stdout).unwrap();

    let direct: std::collections::BTreeSet<_> = tree
        .lines()
        .skip(1)
        .filter_map(|line| line.split_whitespace().next())
        .map(str::to_owned)
        .collect();
    let expected = [
        "memchr",
        "rustc-hash",
        "smallvec",
        "verter_debug_assert",
        "verter_span",
    ]
    .into_iter()
    .map(str::to_owned)
    .collect();
    assert_eq!(
        direct, expected,
        "direct normal/build dependency drift\n{tree}"
    );

    let metadata = Command::new(env!("CARGO"))
        .args([
            "metadata",
            "--format-version",
            "1",
            "--no-deps",
            "--manifest-path",
            manifest,
        ])
        .output()
        .unwrap();
    assert!(metadata.status.success(), "cargo metadata failed");
    let metadata = String::from_utf8(metadata.stdout).unwrap();
    let package = json_object_array(&metadata, "packages")
        .into_iter()
        .find(|package| json_optional_string_field(package, "name") == Some("verter_css_syntax"))
        .unwrap();
    let declared: std::collections::BTreeSet<_> = json_object_array(package, "dependencies")
        .into_iter()
        .filter(|dependency| {
            matches!(
                json_optional_string_field(dependency, "kind"),
                None | Some("build")
            )
        })
        .map(|dependency| {
            (
                json_optional_string_field(dependency, "name")
                    .unwrap()
                    .to_owned(),
                json_optional_string_field(dependency, "kind")
                    .unwrap_or("normal")
                    .to_owned(),
                json_optional_string_field(dependency, "target").map(str::to_owned),
            )
        })
        .collect();
    let expected_declared = [
        ("memchr".to_owned(), "normal".to_owned(), None),
        ("rustc-hash".to_owned(), "normal".to_owned(), None),
        ("smallvec".to_owned(), "normal".to_owned(), None),
        ("verter_debug_assert".to_owned(), "normal".to_owned(), None),
        ("verter_span".to_owned(), "normal".to_owned(), None),
    ]
    .into_iter()
    .collect();
    assert_eq!(
        declared, expected_declared,
        "declared normal/build dependency drift across target predicates"
    );
}
