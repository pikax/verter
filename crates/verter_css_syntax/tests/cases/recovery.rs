use std::sync::Arc;

use verter_css_syntax::{
    parse_lossless, CssDiagnosticKind, CssDialect, CssEntryPoint, CssParseFailure, CssParseMode,
    CssSource, CssSourceTooLarge, RecoveryKind, SourceSize, SyntaxKind,
};

#[test]
fn strict_fails_where_recover_emits_explicit_nodes() {
    let input = ".a { color: rgb(1, 2; broken: [x; }\n.b { ok: yes; }";
    let strict_source = CssSource::new(Arc::from(input), 0).unwrap();
    let strict = parse_lossless(
        strict_source,
        CssDialect::Css,
        CssEntryPoint::Stylesheet,
        CssParseMode::Strict,
    );
    let recover_source = CssSource::new(Arc::from(input), 0).unwrap();
    let recovered = parse_lossless(
        recover_source,
        CssDialect::Css,
        CssEntryPoint::Stylesheet,
        CssParseMode::Recover,
    )
    .unwrap();

    assert!(matches!(
        strict,
        Err(CssParseFailure::Diagnostic(diagnostic))
            if diagnostic.kind == CssDiagnosticKind::MismatchedDelimiter
    ));
    assert!(recovered
        .nodes()
        .iter()
        .any(|node| node.kind() == SyntaxKind::Recovery));
    assert!(recovered
        .diagnostics()
        .iter()
        .any(|diagnostic| diagnostic.recovery == RecoveryKind::AdvanceToBoundary));
    assert!(
        recovered
            .nodes()
            .iter()
            .filter(|node| node.kind() == SyntaxKind::QualifiedRule)
            .count()
            >= 2
    );
    assert_eq!(recovered.reconstruct(), input);
}

#[test]
fn source_domain_overflow_is_typed() {
    assert_eq!(
        SourceSize::new(0, u32::MAX as usize + 1),
        Err(CssSourceTooLarge {
            origin: 0,
            source_len: u32::MAX as u64 + 1,
        })
    );
    assert_eq!(
        SourceSize::new(u32::MAX, 1),
        Err(CssSourceTooLarge {
            origin: u32::MAX,
            source_len: 1,
        })
    );
    assert!(CssSource::new(Arc::from("x"), u32::MAX).is_err());
}

#[test]
fn syntax_element_index_overflow_is_typed() {
    let error = verter_css_syntax::SyntaxElement::try_token(u32::MAX).unwrap_err();
    assert_eq!(
        error.kind,
        verter_css_syntax::StructureOverflowKind::TokenIndex
    );
}

#[test]
fn excessive_nesting_returns_typed_structure_overflow() {
    let input = format!("{}x{}", "(".repeat(129), ")".repeat(129));
    let source = CssSource::new(Arc::from(input), 0).unwrap();
    let result = parse_lossless(
        source,
        CssDialect::Css,
        CssEntryPoint::ComponentValueList,
        CssParseMode::Strict,
    );

    assert!(matches!(
        result,
        Err(CssParseFailure::Structure(error))
            if error.kind == verter_css_syntax::StructureOverflowKind::NestingDepth
    ));

    for input in [
        format!("{}x{}", ".a{".repeat(129), "}".repeat(129)),
        format!("{}x{}", "@media x{".repeat(129), "}".repeat(129)),
    ] {
        let source = CssSource::new(Arc::from(input), 0).unwrap();
        let result = parse_lossless(
            source,
            CssDialect::Css,
            CssEntryPoint::Stylesheet,
            CssParseMode::Strict,
        );
        assert!(matches!(
            result,
            Err(CssParseFailure::Structure(error))
                if error.kind == verter_css_syntax::StructureOverflowKind::NestingDepth
        ));
    }
}

#[test]
fn malformed_at_rule_stops_at_its_owners_right_brace() {
    let input = ".a { @media x } .b { color:red }";
    let source = CssSource::new(Arc::from(input), 41).unwrap();
    let cst = parse_lossless(
        source.clone(),
        CssDialect::Css,
        CssEntryPoint::Stylesheet,
        CssParseMode::Recover,
    )
    .unwrap();
    let qualified: Vec<_> = cst
        .nodes()
        .iter()
        .filter(|node| node.kind() == SyntaxKind::QualifiedRule)
        .map(|node| source.slice(node.span()).to_owned())
        .collect();

    assert_eq!(qualified, vec![".a { @media x }", ".b { color:red }"]);
    assert!(cst.diagnostics().iter().any(|diagnostic| {
        diagnostic.kind == CssDiagnosticKind::ExpectedAtRuleTerminator
            && diagnostic.recovery == RecoveryKind::AdvanceToBoundary
    }));
    assert_eq!(cst.reconstruct(), input);
}

#[test]
// @ai-generated - Verifies nested qualified-rule recovery transfers the owner brace.
fn malformed_qualified_rule_stops_at_its_owners_right_brace() {
    let input = ".a { broken } .b { color:red }";
    let source = CssSource::new(Arc::from(input), 41).unwrap();
    let cst = parse_lossless(
        source.clone(),
        CssDialect::Css,
        CssEntryPoint::Stylesheet,
        CssParseMode::Recover,
    )
    .unwrap();
    let qualified: Vec<_> = cst
        .nodes()
        .iter()
        .filter(|node| node.kind() == SyntaxKind::QualifiedRule)
        .map(|node| source.slice(node.span()).to_owned())
        .collect();

    assert_eq!(
        qualified,
        vec!["broken ", ".a { broken }", ".b { color:red }"]
    );
    assert!(cst.diagnostics().iter().any(|diagnostic| {
        diagnostic.kind == CssDiagnosticKind::ExpectedRuleBlock
            && diagnostic.span.start == 53
            && diagnostic.span.end == 53
            && diagnostic.recovery == RecoveryKind::AdvanceToBoundary
    }));
    assert!(cst.nodes().iter().any(|node| {
        node.kind() == SyntaxKind::Recovery && node.span().start == 53 && node.span().end == 53
    }));
    assert_eq!(cst.reconstruct(), input);
}

// @ai-generated - Verifies an unterminated selector-list pseudo transfers the owner brace.
#[test]
fn unterminated_selector_list_pseudo_stops_at_its_owners_right_brace() {
    let input = ".a { :is(.b } .c { color:red }";
    let source = CssSource::new(Arc::from(input), 41).unwrap();
    let cst = parse_lossless(
        source.clone(),
        CssDialect::Css,
        CssEntryPoint::Stylesheet,
        CssParseMode::Recover,
    )
    .unwrap();
    let qualified: Vec<_> = cst
        .nodes()
        .iter()
        .filter(|node| node.kind() == SyntaxKind::QualifiedRule)
        .map(|node| source.slice(node.span()).to_owned())
        .collect();

    assert_eq!(
        qualified,
        vec![":is(.b ", ".a { :is(.b }", ".c { color:red }"]
    );
    assert_eq!(
        cst.nodes()
            .iter()
            .find(|node| node.kind() == SyntaxKind::PseudoSelectorList)
            .unwrap()
            .span(),
        verter_span::Span::new(46, 53)
    );
    let rule_blocks: Vec<_> = cst
        .nodes()
        .iter()
        .filter(|node| node.kind() == SyntaxKind::RuleBlock)
        .map(|node| node.span())
        .collect();
    assert_eq!(
        rule_blocks,
        vec![
            verter_span::Span::new(44, 54),
            verter_span::Span::new(58, 71)
        ]
    );
    let diagnostics: Vec<_> = cst
        .diagnostics()
        .iter()
        .map(|diagnostic| (diagnostic.kind, diagnostic.span, diagnostic.recovery))
        .collect();
    assert_eq!(
        diagnostics,
        vec![
            (
                CssDiagnosticKind::UnterminatedBlock,
                verter_span::Span::new(53, 53),
                RecoveryKind::AdvanceToBoundary,
            ),
            (
                CssDiagnosticKind::ExpectedRuleBlock,
                verter_span::Span::new(53, 53),
                RecoveryKind::AdvanceToBoundary,
            ),
        ]
    );
    let recovery_spans: Vec<_> = cst
        .nodes()
        .iter()
        .filter(|node| node.kind() == SyntaxKind::Recovery)
        .map(|node| node.span())
        .collect();
    assert_eq!(
        recovery_spans,
        vec![
            verter_span::Span::new(53, 53),
            verter_span::Span::new(53, 53)
        ]
    );
    assert_eq!(cst.reconstruct(), input);
}

// @ai-generated - Verifies a pseudo-name probe cannot consume its owner's right brace.
#[test]
fn pseudo_probe_preserves_owning_right_brace_and_sibling_rule() {
    for (input, owner) in [
        (".a { color junk:} .b { width:1px; }", ".a { color junk:}"),
        (
            ".a { color junk:/**/} .b { width:1px; }",
            ".a { color junk:/**/}",
        ),
    ] {
        let source = CssSource::new(Arc::from(input), 0).unwrap();
        let cst = parse_lossless(
            source.clone(),
            CssDialect::Css,
            CssEntryPoint::Stylesheet,
            CssParseMode::Recover,
        )
        .unwrap();
        let qualified: Vec<_> = cst
            .nodes()
            .iter()
            .filter(|node| node.kind() == SyntaxKind::QualifiedRule)
            .map(|node| source.slice(node.span()))
            .collect();
        let pseudo = cst
            .nodes()
            .iter()
            .find(|node| node.kind() == SyntaxKind::PseudoClass)
            .unwrap();

        assert!(qualified.contains(&owner));
        assert!(qualified.contains(&".b { width:1px; }"));
        assert_eq!(source.slice(pseudo.span()), ":");
        assert!(cst.diagnostics().iter().any(|diagnostic| {
            diagnostic.kind == CssDiagnosticKind::ExpectedRuleBlock
                && diagnostic.recovery == RecoveryKind::AdvanceToBoundary
        }));
        assert_eq!(cst.reconstruct(), input);
    }
}

// @ai-generated - Verifies a pseudo-name probe cannot consume declaration recovery's semicolon.
#[test]
fn pseudo_probe_preserves_semicolon_and_resumes_declarations() {
    for input in [
        ".a { color junk:; width:1px; }",
        ".a { color junk:/**/; width:1px; }",
    ] {
        let source = CssSource::new(Arc::from(input), 0).unwrap();
        let cst = parse_lossless(
            source.clone(),
            CssDialect::Css,
            CssEntryPoint::Stylesheet,
            CssParseMode::Recover,
        )
        .unwrap();
        let declarations: Vec<_> = cst
            .nodes()
            .iter()
            .filter(|node| node.kind() == SyntaxKind::Declaration)
            .map(|node| source.slice(node.span()))
            .collect();
        let pseudo = cst
            .nodes()
            .iter()
            .find(|node| node.kind() == SyntaxKind::PseudoClass)
            .unwrap();

        assert_eq!(declarations, vec!["width:1px;"]);
        assert_eq!(source.slice(pseudo.span()), ":");
        assert!(cst.diagnostics().iter().any(|diagnostic| {
            diagnostic.kind == CssDiagnosticKind::ExpectedRuleBlock
                && diagnostic.recovery == RecoveryKind::AdvanceToBoundary
        }));
        assert_eq!(cst.reconstruct(), input);
    }
}

// @ai-generated - Verifies the second pseudo-element probe cannot consume its owner's brace.
#[test]
fn pseudo_element_probe_preserves_owning_right_brace_and_sibling_rule() {
    for (input, owner, pseudo_text) in [
        (
            ".a { color junk::} .b { width:1px; }",
            ".a { color junk::}",
            "::",
        ),
        (
            ".a { color junk:/**/:/**/} .b { width:1px; }",
            ".a { color junk:/**/:/**/}",
            ":/**/:",
        ),
    ] {
        let source = CssSource::new(Arc::from(input), 0).unwrap();
        let cst = parse_lossless(
            source.clone(),
            CssDialect::Css,
            CssEntryPoint::Stylesheet,
            CssParseMode::Recover,
        )
        .unwrap();
        let qualified: Vec<_> = cst
            .nodes()
            .iter()
            .filter(|node| node.kind() == SyntaxKind::QualifiedRule)
            .map(|node| source.slice(node.span()))
            .collect();
        let pseudo = cst
            .nodes()
            .iter()
            .find(|node| node.kind() == SyntaxKind::PseudoElement)
            .unwrap();

        assert!(qualified.contains(&owner));
        assert!(qualified.contains(&".b { width:1px; }"));
        assert_eq!(source.slice(pseudo.span()), pseudo_text);
        assert!(cst.diagnostics().iter().any(|diagnostic| {
            diagnostic.kind == CssDiagnosticKind::ExpectedRuleBlock
                && diagnostic.recovery == RecoveryKind::AdvanceToBoundary
        }));
        assert_eq!(cst.reconstruct(), input);
    }
}

// @ai-generated - Verifies comment-transparent pseudo names still accept identifiers and functions.
#[test]
fn comment_transparent_pseudo_names_accept_identifiers_and_functions() {
    let input = concat!(
        ".host {",
        " item:/**/hover { color:red; }",
        " widget:/**/is(.x) { color:blue; }",
        " thing:/**/:/**/before { color:green; }",
        " slot:/**/:/**/part(foo) { color:black; }",
        " }",
    );
    let source = CssSource::new(Arc::from(input), 0).unwrap();
    let cst = parse_lossless(
        source.clone(),
        CssDialect::Css,
        CssEntryPoint::Stylesheet,
        CssParseMode::Strict,
    )
    .unwrap();
    let pseudos: Vec<_> = cst
        .nodes()
        .iter()
        .filter(|node| {
            matches!(
                node.kind(),
                SyntaxKind::PseudoClass
                    | SyntaxKind::PseudoElement
                    | SyntaxKind::PseudoSelectorList
            )
        })
        .map(|node| source.slice(node.span()))
        .collect();

    assert_eq!(
        pseudos,
        vec![
            ":/**/hover",
            ":/**/is(.x)",
            ":/**/:/**/before",
            ":/**/:/**/part(foo)",
        ]
    );
    assert_eq!(
        cst.nodes()
            .iter()
            .filter(|node| node.kind() == SyntaxKind::QualifiedRule)
            .count(),
        5
    );
    assert_eq!(
        cst.nodes()
            .iter()
            .filter(|node| node.kind() == SyntaxKind::Declaration)
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

#[test]
fn standalone_selector_recovery_consumes_every_byte_and_reaches_eof() {
    let input = ":is(.a])tail";
    let source = CssSource::new(Arc::from(input), 7).unwrap();
    let cst = parse_lossless(
        source,
        CssDialect::Css,
        CssEntryPoint::SelectorList,
        CssParseMode::Recover,
    )
    .unwrap();

    assert!(!cst.diagnostics().is_empty());
    assert!(cst
        .nodes()
        .iter()
        .any(|node| node.kind() == SyntaxKind::Recovery && node.span().start < node.span().end));
    assert_eq!(cst.tokens().last().unwrap().end, 7 + input.len() as u32);
    assert_eq!(cst.reconstruct(), input);
}

#[test]
fn lexical_corruption_is_typed_in_strict_and_explicit_in_recover() {
    for (input, expected) in [
        ("/*", CssDiagnosticKind::UnterminatedComment),
        ("\"x", CssDiagnosticKind::UnterminatedString),
        ("\"x\n", CssDiagnosticKind::BadString),
        ("url(foo", CssDiagnosticKind::UnterminatedUrl),
        ("url(a b)", CssDiagnosticKind::BadUrl),
    ] {
        let strict_source = CssSource::new(Arc::from(input), 5).unwrap();
        let strict = parse_lossless(
            strict_source,
            CssDialect::Css,
            CssEntryPoint::ComponentValueList,
            CssParseMode::Strict,
        );
        assert!(matches!(
            strict,
            Err(CssParseFailure::Diagnostic(diagnostic)) if diagnostic.kind == expected
        ));

        let recover_source = CssSource::new(Arc::from(input), 5).unwrap();
        let recovered = parse_lossless(
            recover_source,
            CssDialect::Css,
            CssEntryPoint::ComponentValueList,
            CssParseMode::Recover,
        )
        .unwrap();
        assert_eq!(recovered.diagnostics()[0].kind, expected);
        assert_eq!(
            recovered.diagnostics()[0].recovery,
            RecoveryKind::AdvanceOneToken
        );
        assert!(recovered
            .nodes()
            .iter()
            .any(|node| node.kind() == SyntaxKind::Recovery));
        assert_eq!(recovered.reconstruct(), input);
    }
}

#[test]
// @ai-generated - Verifies EOF and newline diagnostics for Less escaped strings.
fn unterminated_less_escaped_strings_share_typed_string_recovery() {
    for (input, expected) in [
        ("~\"x", CssDiagnosticKind::UnterminatedString),
        ("~\"x\n", CssDiagnosticKind::BadString),
    ] {
        let strict_source = CssSource::new(Arc::from(input), 17).unwrap();
        let strict = parse_lossless(
            strict_source,
            CssDialect::Less,
            CssEntryPoint::ComponentValueList,
            CssParseMode::Strict,
        );
        assert!(matches!(
            strict,
            Err(CssParseFailure::Diagnostic(diagnostic))
                if diagnostic.kind == expected
                    && diagnostic.span.start == 17
                    && diagnostic.span.end == 17 + input.trim_end_matches('\n').len() as u32
        ));

        let recover_source = CssSource::new(Arc::from(input), 17).unwrap();
        let recovered = parse_lossless(
            recover_source,
            CssDialect::Less,
            CssEntryPoint::ComponentValueList,
            CssParseMode::Recover,
        )
        .unwrap();
        assert_eq!(recovered.diagnostics()[0].kind, expected);
        assert_eq!(
            recovered.diagnostics()[0].recovery,
            RecoveryKind::AdvanceOneToken
        );
        assert!(recovered.nodes().iter().any(|node| {
            node.kind() == SyntaxKind::Recovery
                && node.span().start == 17
                && node.span().end == 17 + input.trim_end_matches('\n').len() as u32
        }));
        assert_eq!(recovered.reconstruct(), input);
    }
}

// @ai-generated - Verifies URL whitespace at EOF shares unterminated-URL recovery.
#[test]
fn url_content_then_whitespace_at_eof_is_typed_as_unterminated_url() {
    let input = "url(foo   ";
    let strict_source = CssSource::new(Arc::from(input), 17).unwrap();
    let strict = parse_lossless(
        strict_source,
        CssDialect::Css,
        CssEntryPoint::ComponentValueList,
        CssParseMode::Strict,
    );
    assert!(matches!(
        strict,
        Err(CssParseFailure::Diagnostic(diagnostic))
            if diagnostic.kind == CssDiagnosticKind::UnterminatedUrl
                && diagnostic.span.start == 17
                && diagnostic.span.end == 27
                && diagnostic.recovery == RecoveryKind::None
    ));

    let recover_source = CssSource::new(Arc::from(input), 17).unwrap();
    let recovered = parse_lossless(
        recover_source,
        CssDialect::Css,
        CssEntryPoint::ComponentValueList,
        CssParseMode::Recover,
    )
    .unwrap();
    assert_eq!(recovered.diagnostics().len(), 1);
    assert_eq!(
        recovered.diagnostics()[0].kind,
        CssDiagnosticKind::UnterminatedUrl
    );
    assert_eq!(
        recovered.diagnostics()[0].span,
        verter_span::Span::new(17, 27)
    );
    assert_eq!(
        recovered.diagnostics()[0].recovery,
        RecoveryKind::AdvanceOneToken
    );
    assert!(recovered.nodes().iter().any(|node| {
        node.kind() == SyntaxKind::Recovery && node.span() == verter_span::Span::new(17, 27)
    }));
    assert_eq!(recovered.tokens().len(), 1);
    assert_eq!(
        recovered.tokens()[0].kind(),
        verter_css_syntax::TokenKind::Url
    );
    assert_eq!(
        recovered.tokens()[0].flags,
        verter_css_syntax::TokenFlags::UNTERMINATED
    );
    assert_eq!(recovered.reconstruct(), input);
}
