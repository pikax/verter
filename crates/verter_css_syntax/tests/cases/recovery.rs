use std::sync::Arc;

use verter_css_syntax::{
    CssDiagnosticKind, CssDialect, CssEntryPoint, CssParseFailure, CssParseMode, CssSource,
    CssSourceTooLarge, RecoveryKind, SourceSize, StructureOverflowKind, SyntaxKind,
};

use crate::cst::parse_lossless;

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

// @ai-generated - dialect-coverage parity for Sass/Stylus in the recovery suite.
//
// `CssEntryPoint::ComponentValueList` and `CssEntryPoint::SelectorList` never route through the
// indentation-aware layout parser regardless of dialect (only `Stylesheet` can), so lexical and
// component-value recovery is dialect-neutral there by construction; this pins that neutrality
// for Sass/Stylus explicitly instead of leaving it CSS/Less-only. The `Stylesheet`-entry case
// covers the one construct layout-parser statement classification must still get right on its
// own: a comment sitting between a pseudo-selector's colon and its name must not be mistaken for
// a declaration's "value follows the colon" shape.
#[test]
fn sass_and_stylus_recovery_coverage_parity_with_css_and_less() {
    const DIALECTS: [CssDialect; 5] = [
        CssDialect::Css,
        CssDialect::Scss,
        CssDialect::Less,
        CssDialect::Sass,
        CssDialect::Stylus,
    ];

    // ── the CLOSED recovery outcome set ──
    //
    // Everything below this block is a hand-selected fixture, i.e. a SAMPLE:
    // a failure mode nobody wrote a fixture for — or one whose Sass/Stylus
    // typing silently diverges from Css/Less outside the sampled shapes —
    // coexists with a green run. This block replaces the sample with the
    // ENUMERATED universe. `CssDiagnosticKind`, `RecoveryKind` and
    // `StructureOverflowKind` are the closed sets of recovery outcomes the
    // parser can report, and each `disposition` function below matches over
    // its enum EXHAUSTIVELY, so a new variant is a compile error (`E0004`)
    // until dispositioned here.
    //
    // Three dispositions:
    //
    //   `Symmetric`   — every dialect must report the SAME diagnostic kind
    //                   and the SAME recovery kind from one fixture.
    //   `PerDialect`  — the failure is typed differently by the direct parser
    //                   and the indentation-aware layout parser (or exists
    //                   only in one of them). Every dialect that reports it
    //                   names its own fixture, AND every dialect NOT named
    //                   must NOT report it from those fixtures — so this
    //                   cannot wave an asymmetry through, it pins its shape.
    //   `Unreachable` — no input this suite can express reaches the outcome.
    //                   The reason is recorded, and the whole inventory is
    //                   re-checked to confirm nothing produces it, so the
    //                   disposition cannot silently rot once it becomes
    //                   reachable.

    type DiagCase = (CssDialect, &'static str, CssEntryPoint);

    enum Disposition {
        Symmetric(&'static str, CssEntryPoint, RecoveryKind),
        PerDialect(&'static str, RecoveryKind, &'static [DiagCase]),
        Unreachable(&'static str),
    }

    const UNTERMINATED_BLOCK: &str = ".a { color: red;";
    const NO_RULE_BLOCK: &str = ".a";
    const STRAY_AT_RULE: &str = ".a { @import \"x\" }";
    const INCONSISTENT_INDENT: &str = ".a\n\tcolor: red\n  x: y\n";
    const LEADING_INDENT: &str = "  .a\n";
    const SASS_AMBIGUOUS: &str = "$tone junk: red;";
    const STYLUS_AMBIGUOUS: &str = "foo bar baz\n";
    const UNTERMINATED_INTERP_HASH: &str = ".a-#{$x\n  color: red\n";
    const UNTERMINATED_INTERP_AT: &str = ".a-@{x\n  color: red\n";
    const UNTERMINATED_INTERP_DOLLAR: &str = ".a-${x\n  color: red\n";

    // Exhaustive over `CssDiagnosticKind`.
    fn disposition(kind: CssDiagnosticKind) -> Disposition {
        use CssDialect::{Css, Less, Sass, Scss, Stylus};
        use CssEntryPoint::{ComponentValueList, Stylesheet};

        match kind {
            CssDiagnosticKind::UnexpectedClosingDelimiter => {
                Disposition::Symmetric(")", ComponentValueList, RecoveryKind::AdvanceOneToken)
            }
            CssDiagnosticKind::MismatchedDelimiter => {
                Disposition::Symmetric("(]", ComponentValueList, RecoveryKind::AdvanceOneToken)
            }
            CssDiagnosticKind::UnterminatedBlock => Disposition::Symmetric(
                UNTERMINATED_BLOCK,
                Stylesheet,
                RecoveryKind::CloseAtEndOfInput,
            ),
            CssDiagnosticKind::UnterminatedComment => {
                Disposition::Symmetric("/*", ComponentValueList, RecoveryKind::AdvanceOneToken)
            }
            CssDiagnosticKind::UnterminatedString => {
                Disposition::Symmetric("\"x", ComponentValueList, RecoveryKind::AdvanceOneToken)
            }
            CssDiagnosticKind::BadString => {
                Disposition::Symmetric("\"x\n", ComponentValueList, RecoveryKind::AdvanceOneToken)
            }
            CssDiagnosticKind::UnterminatedUrl => {
                Disposition::Symmetric("url(foo", ComponentValueList, RecoveryKind::AdvanceOneToken)
            }
            CssDiagnosticKind::BadUrl => Disposition::Symmetric(
                "url(a b)",
                ComponentValueList,
                RecoveryKind::AdvanceOneToken,
            ),

            CssDiagnosticKind::ExpectedRuleBlock => Disposition::PerDialect(
                "a headless prelude is the DIRECT parser's failure; the layout parser types \
                 the same bytes as its own ambiguous statement",
                RecoveryKind::AdvanceToBoundary,
                &[
                    (Css, NO_RULE_BLOCK, Stylesheet),
                    (Scss, NO_RULE_BLOCK, Stylesheet),
                    (Less, NO_RULE_BLOCK, Stylesheet),
                ],
            ),
            CssDiagnosticKind::ExpectedAtRuleTerminator => Disposition::PerDialect(
                "an unterminated at-rule statement is the DIRECT parser's failure; the layout \
                 parser ends the statement at its own line boundary instead",
                RecoveryKind::AdvanceToBoundary,
                &[
                    (Css, STRAY_AT_RULE, Stylesheet),
                    (Scss, STRAY_AT_RULE, Stylesheet),
                    (Less, STRAY_AT_RULE, Stylesheet),
                ],
            ),
            CssDiagnosticKind::AmbiguousStatement => Disposition::PerDialect(
                "`AmbiguousStatement` is the LAYOUT parser's own vocabulary for a statement it \
                 cannot classify; the direct parser has no notion of it",
                RecoveryKind::None,
                &[
                    (Sass, SASS_AMBIGUOUS, Stylesheet),
                    (Stylus, STYLUS_AMBIGUOUS, Stylesheet),
                ],
            ),
            CssDiagnosticKind::InconsistentIndentation => Disposition::PerDialect(
                "indentation diagnostics belong to the layout parser, which plain CSS never \
                 reaches; the other four dialects all route brace-free indented input to it",
                RecoveryKind::AdvanceToBoundary,
                &[
                    (Scss, INCONSISTENT_INDENT, Stylesheet),
                    (Less, INCONSISTENT_INDENT, Stylesheet),
                    (Sass, INCONSISTENT_INDENT, Stylesheet),
                    (Stylus, INCONSISTENT_INDENT, Stylesheet),
                ],
            ),
            CssDiagnosticKind::UnexpectedIndentation => Disposition::PerDialect(
                "indentation diagnostics belong to the layout parser, which plain CSS never \
                 reaches; Sass and Stylus reach it for any input, Scss and Less for brace-free \
                 indented input",
                RecoveryKind::AdvanceToBoundary,
                &[
                    (Sass, LEADING_INDENT, Stylesheet),
                    (Stylus, LEADING_INDENT, Stylesheet),
                    (Scss, INCONSISTENT_INDENT, Stylesheet),
                    (Less, INCONSISTENT_INDENT, Stylesheet),
                ],
            ),
            CssDiagnosticKind::UnterminatedInterpolation => Disposition::PerDialect(
                "each preprocessor spells interpolation differently, and plain CSS has none",
                RecoveryKind::AdvanceToBoundary,
                &[
                    (Scss, UNTERMINATED_INTERP_HASH, Stylesheet),
                    (Sass, UNTERMINATED_INTERP_HASH, Stylesheet),
                    (Less, UNTERMINATED_INTERP_AT, Stylesheet),
                    (Stylus, UNTERMINATED_INTERP_DOLLAR, Stylesheet),
                ],
            ),

            CssDiagnosticKind::ExpectedDeclarationColon => Disposition::Unreachable(
                "both rule-body classifiers require the declaration colon BEFORE committing to \
                 a declaration (`looks_like_declaration` scans for it, the layout parser's \
                 boundary classifier keys on it), so the colon-missing arms inside \
                 `parse_declaration` are defensive and no input reaches them",
            ),
        }
    }

    fn diagnostics(
        input: &str,
        dialect: CssDialect,
        entry: CssEntryPoint,
    ) -> Vec<(CssDiagnosticKind, RecoveryKind)> {
        let Ok(source) = CssSource::new(Arc::from(input), 0) else {
            return Vec::new();
        };
        parse_lossless(source, dialect, entry, CssParseMode::Recover).map_or_else(
            |_| Vec::new(),
            |cst| {
                cst.diagnostics()
                    .iter()
                    .map(|diagnostic| (diagnostic.kind, diagnostic.recovery))
                    .collect()
            },
        )
    }

    // Walk `CssDiagnosticKind`'s discriminants the same way the CST suite
    // walks `SyntaxKind`'s. A hand-maintained array would silently omit a
    // new variant; `from_raw` + `kind as u8 != raw` is the derived walk,
    // and the exhaustive `disposition` match forces a new variant to be
    // dispositioned.
    let every_diagnostic: Vec<CssDiagnosticKind> = {
        let mut kinds = Vec::new();
        for raw in 0u8.. {
            let kind = CssDiagnosticKind::from_raw(raw);
            if kind as u8 != raw {
                break;
            }
            kinds.push(kind);
        }
        kinds
    };
    assert!(
        every_diagnostic.len() >= 15,
        "the discriminant walk must have found the real variant set, got {}",
        every_diagnostic.len()
    );

    // Every fixture named anywhere in the inventory, so `Unreachable` can be
    // checked against the whole inventory rather than trusted.
    let mut every_fixture: Vec<(&'static str, CssEntryPoint)> = Vec::new();
    let mut observed_recoveries: Vec<RecoveryKind> = Vec::new();

    for kind in every_diagnostic.iter().copied() {
        match disposition(kind) {
            Disposition::Symmetric(source, entry, recovery) => {
                every_fixture.push((source, entry));
                observed_recoveries.push(recovery);
                for dialect in DIALECTS {
                    let found = diagnostics(source, dialect, entry);
                    assert!(
                        found.contains(&(kind, recovery)),
                        "{kind:?} is dispositioned Symmetric with recovery {recovery:?}, but \
                         {dialect:?} reports {found:?} for {source:?}"
                    );
                }
            }
            Disposition::PerDialect(reason, recovery, cases) => {
                assert!(!reason.is_empty() && !cases.is_empty(), "{kind:?}");
                observed_recoveries.push(recovery);
                let reporting: Vec<CssDialect> =
                    cases.iter().map(|(dialect, ..)| *dialect).collect();
                for (dialect, source, entry) in cases.iter().copied() {
                    every_fixture.push((source, entry));
                    let found = diagnostics(source, dialect, entry);
                    assert!(
                        found.contains(&(kind, recovery)),
                        "{kind:?} is dispositioned PerDialect with a {dialect:?} fixture \
                         {source:?} that does not report it with recovery {recovery:?}: {found:?}"
                    );
                    for other in DIALECTS.into_iter().filter(|d| !reporting.contains(d)) {
                        assert!(
                            !diagnostics(source, other, entry)
                                .iter()
                                .any(|(found, _)| *found == kind),
                            "{other:?} is not listed as reporting {kind:?}, yet it reports one \
                             for {source:?} — the disposition is stale"
                        );
                    }
                }
            }
            Disposition::Unreachable(reason) => {
                assert!(!reason.is_empty(), "{kind:?}");
            }
        }
    }

    // `Unreachable` is checked against the WHOLE inventory, not trusted.
    for kind in every_diagnostic.iter().copied() {
        if !matches!(disposition(kind), Disposition::Unreachable(_)) {
            continue;
        }
        for (source, entry) in every_fixture.iter().copied() {
            for dialect in DIALECTS {
                assert!(
                    !diagnostics(source, dialect, entry)
                        .iter()
                        .any(|(found, _)| *found == kind),
                    "{kind:?} is dispositioned Unreachable, yet {dialect:?} reports one for \
                     {source:?} — the disposition is stale"
                );
            }
        }
    }

    // `RecoveryKind` is its own closed set: every variant must actually be
    // exercised by the inventory above, so a recovery strategy cannot be
    // added (or silently stop being produced) without a fixture reaching it.
    for recovery in [
        RecoveryKind::None,
        RecoveryKind::AdvanceOneToken,
        RecoveryKind::AdvanceToBoundary,
        RecoveryKind::CloseAtEndOfInput,
    ] {
        // Exhaustive: a new variant fails to compile here until dispositioned.
        let must_be_observed = match recovery {
            RecoveryKind::None
            | RecoveryKind::AdvanceOneToken
            | RecoveryKind::AdvanceToBoundary
            | RecoveryKind::CloseAtEndOfInput => true,
        };
        assert_eq!(
            must_be_observed,
            observed_recoveries.contains(&recovery),
            "{recovery:?} is not exercised by the diagnostic inventory"
        );
    }

    // `StructureOverflowKind` closes the third recovery surface.
    for overflow in [
        StructureOverflowKind::TokenIndex,
        StructureOverflowKind::NodeIndex,
        StructureOverflowKind::ElementIndex,
        StructureOverflowKind::ChildRange,
        StructureOverflowKind::NestingDepth,
    ] {
        // Exhaustive: a new variant fails to compile here until dispositioned.
        let fixture: Option<&str> = match overflow {
            StructureOverflowKind::NestingDepth => Some("nesting"),
            // The index/range overflows need a source with more than `u32`
            // tokens, nodes, elements or children — orders of magnitude
            // beyond anything a unit test can allocate. Recorded as a known
            // hole rather than left unstated.
            StructureOverflowKind::TokenIndex
            | StructureOverflowKind::NodeIndex
            | StructureOverflowKind::ElementIndex
            | StructureOverflowKind::ChildRange => None,
        };
        if fixture.is_none() {
            continue;
        }
        let deep = format!("{}x{}", "(".repeat(129), ")".repeat(129));
        for dialect in DIALECTS {
            let source = CssSource::new(Arc::from(deep.as_str()), 0).unwrap();
            assert!(
                matches!(
                    parse_lossless(
                        source,
                        dialect,
                        CssEntryPoint::ComponentValueList,
                        CssParseMode::Strict,
                    ),
                    Err(CssParseFailure::Structure(error)) if error.kind == overflow
                ),
                "{dialect:?} must report {overflow:?}"
            );
        }
    }

    // Lexical corruption (unterminated comment/string/url) is typed identically in strict mode
    // and explicit identically in recover mode for every dialect.
    for (input, expected) in [
        ("/*", CssDiagnosticKind::UnterminatedComment),
        ("\"x", CssDiagnosticKind::UnterminatedString),
        ("\"x\n", CssDiagnosticKind::BadString),
        ("url(foo", CssDiagnosticKind::UnterminatedUrl),
        ("url(a b)", CssDiagnosticKind::BadUrl),
    ] {
        for dialect in DIALECTS {
            let strict = parse_lossless(
                CssSource::new(Arc::from(input), 5).unwrap(),
                dialect,
                CssEntryPoint::ComponentValueList,
                CssParseMode::Strict,
            );
            assert!(
                matches!(
                    strict,
                    Err(CssParseFailure::Diagnostic(diagnostic)) if diagnostic.kind == expected
                ),
                "{dialect:?}: {input}"
            );

            let recovered = parse_lossless(
                CssSource::new(Arc::from(input), 5).unwrap(),
                dialect,
                CssEntryPoint::ComponentValueList,
                CssParseMode::Recover,
            )
            .unwrap();
            assert_eq!(
                recovered.diagnostics()[0].kind,
                expected,
                "{dialect:?}: {input}"
            );
            assert_eq!(
                recovered.diagnostics()[0].recovery,
                RecoveryKind::AdvanceOneToken,
                "{dialect:?}: {input}"
            );
            assert!(
                recovered
                    .nodes()
                    .iter()
                    .any(|node| node.kind() == SyntaxKind::Recovery),
                "{dialect:?}: {input}"
            );
            assert_eq!(recovered.reconstruct(), input, "{dialect:?}: {input}");
        }
    }

    // Excessive component-value nesting (`(((...)))`) is a typed structure overflow, not a stack
    // hazard or a plain diagnostic, in every dialect.
    let deep = format!("{}x{}", "(".repeat(129), ")".repeat(129));
    for dialect in DIALECTS {
        let result = parse_lossless(
            CssSource::new(Arc::from(deep.as_str()), 0).unwrap(),
            dialect,
            CssEntryPoint::ComponentValueList,
            CssParseMode::Strict,
        );
        assert!(
            matches!(
                result,
                Err(CssParseFailure::Structure(error))
                    if error.kind == StructureOverflowKind::NestingDepth
            ),
            "{dialect:?}"
        );
    }

    // A comment between a selector's pseudo-class colon and its name must not be misread as a
    // declaration's "colon, then some intervening trivia, then the value" shape: the four
    // comment-transparent pseudo forms Css already covers stay nested QualifiedRules in every
    // dialect. Before this parity fix, Sass/Stylus's layout-parser statement classifier used raw
    // byte adjacency (`next.start == separator.end`) to detect "touching colon = pseudo", so a
    // comment between the colon and the pseudo name (which is still touching in every real
    // sense) broke the heuristic and misclassified each case as a declaration with an opaque
    // block value instead of a nested rule.
    let input = concat!(
        ".host {",
        " item:/**/hover { color:red; }",
        " widget:/**/is(.x) { color:blue; }",
        " thing:/**/:/**/before { color:green; }",
        " slot:/**/:/**/part(foo) { color:black; }",
        " }",
    );
    for dialect in DIALECTS {
        let source = CssSource::new(Arc::from(input), 0).unwrap();
        let cst = parse_lossless(
            source.clone(),
            dialect,
            CssEntryPoint::Stylesheet,
            CssParseMode::Strict,
        )
        .unwrap_or_else(|error| panic!("{dialect:?} failed to parse: {error:?}"));
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
            ],
            "{dialect:?}"
        );
        assert_eq!(
            cst.nodes()
                .iter()
                .filter(|node| node.kind() == SyntaxKind::QualifiedRule)
                .count(),
            5,
            "{dialect:?}"
        );
        assert_eq!(
            cst.nodes()
                .iter()
                .filter(|node| node.kind() == SyntaxKind::Declaration)
                .count(),
            4,
            "{dialect:?}"
        );
        assert!(
            !cst.nodes()
                .iter()
                .any(|node| node.kind() == SyntaxKind::Recovery),
            "{dialect:?}"
        );
        assert!(cst.diagnostics().is_empty(), "{dialect:?}");
        assert_eq!(cst.reconstruct(), input, "{dialect:?}");
    }

    // A malformed rule never swallows a clean, newline-separated sibling: each dialect's own
    // statement boundary between the two rules is real, even though *how* the malformed rule
    // itself is classified is dialect-appropriate rather than byte-identical (Sass/Stylus flag
    // the headless `broken` prelude as one ambiguous statement instead of Css/Less/Scss's
    // attempted nested-rule reparse of the bare word).
    let sibling_input = ".a { broken }\n.b { color:red; }\n";
    for dialect in DIALECTS {
        let source = CssSource::new(Arc::from(sibling_input), 0).unwrap();
        let recovered = parse_lossless(
            source.clone(),
            dialect,
            CssEntryPoint::Stylesheet,
            CssParseMode::Recover,
        )
        .unwrap();
        assert!(!recovered.diagnostics().is_empty(), "{dialect:?}");
        assert!(
            recovered
                .nodes()
                .iter()
                .any(|node| node.kind() == SyntaxKind::QualifiedRule
                    && source.slice(node.span()) == ".b { color:red; }"),
            "{dialect:?}: sibling rule must survive intact: {:#?}",
            recovered
                .nodes()
                .iter()
                .map(|node| node.kind())
                .collect::<Vec<_>>()
        );
        assert_eq!(recovered.reconstruct(), sibling_input, "{dialect:?}");
    }
}
