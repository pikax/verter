use std::borrow::Cow;
use std::collections::BTreeSet;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use crate::selector::parse_selector_structure;
use verter_css_syntax::{
    decode_css_identifier, parse_style_ir, parse_with_sink, CssDialect, CssEntryPoint,
    CssParseMode, CssSource, Lexer, NodeFlags, ParseEvent, ParseEventSink, SelectorComponentKind,
    StyleCompleteness, StyleStatement, SyntaxKind, SyntaxToken, TokenFlags, TokenKind,
};

use crate::cst::parse_lossless;

fn tokens(source: &CssSource, dialect: CssDialect) -> Vec<(TokenKind, u16, u32, u32)> {
    Lexer::new(source, dialect)
        .map(|token| (token.kind(), token.flags, token.start, token.end))
        .collect()
}

#[derive(Default)]
struct RecordingSink {
    events: Vec<ParseEvent>,
}

impl ParseEventSink for RecordingSink {
    fn event(&mut self, event: ParseEvent) -> Result<(), verter_css_syntax::CssStructureTooLarge> {
        self.events.push(event);
        Ok(())
    }
}

#[test]
fn css_syntax_wpt_tokenization_is_exact() {
    let text: Arc<str> = "--> --foo { color: 1.0px; bad: \"x\n; u: url(a b); }\r\n".into();
    let source = CssSource::new(text, 7).unwrap();
    let actual = tokens(&source, CssDialect::Css);

    assert_eq!(
        actual,
        vec![
            (TokenKind::Cdc, 0, 7, 10),
            (TokenKind::Whitespace, TokenFlags::TRIVIA, 10, 11),
            (TokenKind::Ident, 0, 11, 16),
            (TokenKind::Whitespace, TokenFlags::TRIVIA, 16, 17),
            (TokenKind::LeftBrace, 0, 17, 18),
            (TokenKind::Whitespace, TokenFlags::TRIVIA, 18, 19),
            (TokenKind::Ident, 0, 19, 24),
            (TokenKind::Colon, 0, 24, 25),
            (TokenKind::Whitespace, TokenFlags::TRIVIA, 25, 26),
            (TokenKind::Dimension, 0, 26, 31),
            (TokenKind::Semicolon, 0, 31, 32),
            (TokenKind::Whitespace, TokenFlags::TRIVIA, 32, 33),
            (TokenKind::Ident, 0, 33, 36),
            (TokenKind::Colon, 0, 36, 37),
            (TokenKind::Whitespace, TokenFlags::TRIVIA, 37, 38),
            (TokenKind::BadString, TokenFlags::UNTERMINATED, 38, 40),
            (
                TokenKind::Whitespace,
                TokenFlags::TRIVIA | TokenFlags::CONTAINS_NEWLINE,
                40,
                41,
            ),
            (TokenKind::Semicolon, 0, 41, 42),
            (TokenKind::Whitespace, TokenFlags::TRIVIA, 42, 43),
            (TokenKind::Ident, 0, 43, 44),
            (TokenKind::Colon, 0, 44, 45),
            (TokenKind::Whitespace, TokenFlags::TRIVIA, 45, 46),
            (TokenKind::BadUrl, 0, 46, 54),
            (TokenKind::Semicolon, 0, 54, 55),
            (TokenKind::Whitespace, TokenFlags::TRIVIA, 55, 56),
            (TokenKind::RightBrace, 0, 56, 57),
            (
                TokenKind::Whitespace,
                TokenFlags::TRIVIA | TokenFlags::CONTAINS_NEWLINE,
                57,
                59,
            ),
        ]
    );
    assert_eq!(
        source.slice_tokens(Lexer::new(&source, CssDialect::Css)),
        source.text()
    );
}

#[test]
fn css_and_dialect_extensions_are_distinct_and_lossless() {
    let source = CssSource::new(
        Arc::<str>::from("$tone: #{$base}; @tone: @{base}; ~\"calc\"; // note\r\n"),
        0,
    )
    .unwrap();

    let css = tokens(&source, CssDialect::Css);
    let scss = tokens(&source, CssDialect::Scss);
    let less = tokens(&source, CssDialect::Less);

    assert!(!css.iter().any(|token| token.0 == TokenKind::LineComment));
    assert!(scss.iter().any(|token| token.0 == TokenKind::ScssVariable));
    assert!(scss
        .iter()
        .any(|token| token.0 == TokenKind::ScssInterpolationStart));
    assert!(less.iter().any(|token| token.0 == TokenKind::LessVariable));
    assert!(less
        .iter()
        .any(|token| token.0 == TokenKind::LessInterpolationStart));
    assert!(less
        .iter()
        .any(|token| token.0 == TokenKind::LessEscapedString));
    assert!(scss.iter().any(|token| token.0 == TokenKind::LineComment));
    assert!(less.iter().any(|token| token.0 == TokenKind::LineComment));
    assert_eq!(
        source.slice_tokens(Lexer::new(&source, CssDialect::Scss)),
        source.text()
    );
    assert_eq!(
        source.slice_tokens(Lexer::new(&source, CssDialect::Less)),
        source.text()
    );
}

// @ai-generated - Verifies layout facts and Stylus interpolation remain lexical and lossless.
#[test]
fn layout_newlines_and_stylus_interpolation_are_lexical_facts() {
    let input = ".icon-{name}\n  color red\n.value-${tone}\n";
    let source = CssSource::new(Arc::from(input), 11).unwrap();
    let stylus: Vec<_> = Lexer::new(&source, CssDialect::Stylus).collect();

    let newline_tokens: Vec<_> = stylus
        .iter()
        .filter(|token| token.flags & TokenFlags::CONTAINS_NEWLINE != 0)
        .map(|token| source.token_text(*token))
        .collect();
    assert_eq!(newline_tokens, vec!["\n  ", "\n", "\n"]);

    let interpolations: Vec<_> = stylus
        .iter()
        .filter(|token| token.kind() == TokenKind::StylusInterpolationStart)
        .map(|token| source.token_text(*token))
        .collect();
    assert_eq!(interpolations, vec!["{", "${"]);
    assert_eq!(source.slice_tokens(stylus), input);
}

// @ai-generated - Regression for adjacent Stylus rule braces being mistaken for interpolation.
#[test]
fn adjacent_stylus_rule_braces_are_plain_braces() {
    for input in [
        ".btn{\n  color: red;\n}",
        "#hero{\n  color: red;\n}",
        "foo(){\n  color: red;\n}",
    ] {
        let source = CssSource::new(Arc::from(input), 0).unwrap();
        let tokens: Vec<_> = Lexer::new(&source, CssDialect::Stylus).collect();
        assert!(
            tokens
                .iter()
                .any(|token| token.kind() == TokenKind::LeftBrace),
            "{input}"
        );
        assert!(
            !tokens
                .iter()
                .any(|token| token.kind() == TokenKind::StylusInterpolationStart),
            "{input}"
        );
        let parsed = parse_style_ir(source, CssDialect::Stylus, CssParseMode::Recover).unwrap();
        assert!(
            parsed.diagnostics().is_empty(),
            "{input}: {:#?}",
            parsed.diagnostics()
        );
        assert!(
            matches!(
                parsed.statements().first(),
                Some(StyleStatement::Rule(rule))
                    if rule.completeness() == StyleCompleteness::Complete
            ) | matches!(
                parsed.statements().first(),
                Some(StyleStatement::MixinOrFunction(value))
                    if value.completeness() == StyleCompleteness::Complete
            ),
            "{input}: {:#?}",
            parsed.statements()
        );
    }

    let compact = CssSource::new(Arc::from(".btn{color:red}"), 0).unwrap();
    assert!(
        Lexer::new(&compact, CssDialect::Stylus).any(|token| token.kind() == TokenKind::LeftBrace)
    );

    for input in [".btn{ color red }", ".btn{color red}", ".btn{ }"] {
        let source = CssSource::new(Arc::from(input), 0).unwrap();
        let tokens: Vec<_> = Lexer::new(&source, CssDialect::Stylus).collect();
        assert!(
            tokens
                .iter()
                .any(|token| token.kind() == TokenKind::LeftBrace),
            "{input}"
        );
        assert!(
            !tokens
                .iter()
                .any(|token| token.kind() == TokenKind::StylusInterpolationStart),
            "{input}"
        );
        let parsed = parse_style_ir(source, CssDialect::Stylus, CssParseMode::Recover).unwrap();
        assert!(
            matches!(parsed.statements().first(), Some(StyleStatement::Rule(_))),
            "{input}: {:#?}",
            parsed.statements()
        );
        let classes: Vec<_> = parsed
            .complete_static_classes()
            .map(|class| parsed.source().slice(class.name_span()).to_owned())
            .collect();
        assert_eq!(classes, vec!["btn"], "{input}");
    }
}

// @ai-generated - Exact r2 ternary interpolation repro cannot become a concrete braced rule.
#[test]
fn stylus_ternary_brace_remains_interpolation() {
    let input = "p{a ? b : c}\n  color red";
    let source = CssSource::new(Arc::from(input), 0).unwrap();
    let tokens: Vec<_> = Lexer::new(&source, CssDialect::Stylus).collect();
    assert!(tokens
        .iter()
        .any(|token| token.kind() == TokenKind::StylusInterpolationStart));
    assert!(!tokens
        .iter()
        .any(|token| token.kind() == TokenKind::LeftBrace));
    let parsed = parse_style_ir(source, CssDialect::Stylus, CssParseMode::Recover).unwrap();
    assert!(parsed.has_dynamic_selectors());
}

#[test]
fn hash_classification_and_default_unicode_range_context_follow_css_decision_order() {
    let source = CssSource::new(Arc::from("#1 #--id U+00A0-00FF U+4??"), 0).unwrap();
    let actual = tokens(&source, CssDialect::Css);

    assert_eq!(
        actual,
        vec![
            (TokenKind::Hash, 0, 0, 2),
            (TokenKind::Whitespace, TokenFlags::TRIVIA, 2, 3),
            (TokenKind::Hash, TokenFlags::ID_HASH, 3, 8),
            (TokenKind::Whitespace, TokenFlags::TRIVIA, 8, 9),
            (TokenKind::Ident, 0, 9, 10),
            (TokenKind::Dimension, TokenFlags::NUMBER_INTEGER, 10, 20,),
            (TokenKind::Whitespace, TokenFlags::TRIVIA, 20, 21),
            (TokenKind::Ident, 0, 21, 22),
            (TokenKind::Number, TokenFlags::NUMBER_INTEGER, 22, 24),
            (TokenKind::Delim, 0, 24, 25),
            (TokenKind::Delim, 0, 25, 26),
        ]
    );
    assert_eq!(
        source.slice_tokens(Lexer::new(&source, CssDialect::Css)),
        source.text()
    );
}

#[test]
// @ai-generated - Verifies direct lexing keeps ordered unicode-range text as ordinary CSS tokens.
fn direct_lexer_never_enables_unicode_range_tokenization() {
    let source = CssSource::new(Arc::from("U+1?2 U+?A U+1234567 U+00A0-00FF"), 11).unwrap();
    assert_eq!(
        tokens(&source, CssDialect::Css),
        vec![
            (TokenKind::Ident, 0, 11, 12),
            (TokenKind::Number, TokenFlags::NUMBER_INTEGER, 12, 14),
            (TokenKind::Delim, 0, 14, 15),
            (TokenKind::Number, TokenFlags::NUMBER_INTEGER, 15, 16),
            (TokenKind::Whitespace, TokenFlags::TRIVIA, 16, 17),
            (TokenKind::Ident, 0, 17, 18),
            (TokenKind::Delim, 0, 18, 19),
            (TokenKind::Delim, 0, 19, 20),
            (TokenKind::Ident, 0, 20, 21),
            (TokenKind::Whitespace, TokenFlags::TRIVIA, 21, 22),
            (TokenKind::Ident, 0, 22, 23),
            (TokenKind::Number, TokenFlags::NUMBER_INTEGER, 23, 31),
            (TokenKind::Whitespace, TokenFlags::TRIVIA, 31, 32),
            (TokenKind::Ident, 0, 32, 33),
            (TokenKind::Dimension, TokenFlags::NUMBER_INTEGER, 33, 43,),
        ]
    );
    assert_eq!(
        source.slice_tokens(Lexer::new(&source, CssDialect::Css)),
        source.text()
    );
    assert!(
        !Lexer::new(&source, CssDialect::Css)
            .clone()
            .any(|token| token.kind() == TokenKind::UnicodeRange),
        "a clone of a public default lexer must retain disabled unicode ranges"
    );
}

#[test]
// @ai-generated - Verifies shared CSS escape consumption for CSS and Less strings.
fn strings_consume_shared_css_escapes_and_escaped_newlines() {
    for (input, dialect, expected_kind, expected_flags) in [
        (
            "\"\\41 B\"",
            CssDialect::Css,
            TokenKind::String,
            TokenFlags::CONTAINS_ESCAPE,
        ),
        (
            "\"\\a\nx\"",
            CssDialect::Css,
            TokenKind::String,
            TokenFlags::CONTAINS_ESCAPE | TokenFlags::CONTAINS_NEWLINE,
        ),
        (
            "\"\\123456x\"",
            CssDialect::Css,
            TokenKind::String,
            TokenFlags::CONTAINS_ESCAPE,
        ),
        (
            "\"x\\\r\ny\"",
            CssDialect::Css,
            TokenKind::String,
            TokenFlags::CONTAINS_ESCAPE | TokenFlags::CONTAINS_NEWLINE,
        ),
        (
            "~\"\\41 B\"",
            CssDialect::Less,
            TokenKind::LessEscapedString,
            TokenFlags::DIALECT_EXTENSION | TokenFlags::CONTAINS_ESCAPE,
        ),
    ] {
        let source = CssSource::new(Arc::from(input), 23).unwrap();
        assert_eq!(
            tokens(&source, dialect),
            vec![(expected_kind, expected_flags, 23, 23 + input.len() as u32)],
            "{input:?}"
        );
    }
}

#[test]
fn identifier_decoding_borrows_when_unescaped() {
    let plain = decode_css_identifier("button").unwrap();
    let escaped = decode_css_identifier(r"but\74 on").unwrap();

    assert!(matches!(plain, Cow::Borrowed("button")));
    assert!(matches!(escaped, Cow::Owned(ref value) if value == "button"));
    assert_ne!(escaped.as_ref(), r"but\74 on");
}

#[test]
fn escaped_url_heads_bodies_and_nul_preprocessing_are_exact() {
    let input = "u\\72l(foo) url(foo\\20 bar) url(foo\\000020bar) a\0b url(foo\"\\20 tail)";
    let source = CssSource::new(Arc::from(input), 17).unwrap();
    let actual = tokens(&source, CssDialect::Css);

    assert_eq!(
        actual,
        vec![
            (TokenKind::Url, TokenFlags::CONTAINS_ESCAPE, 17, 27),
            (TokenKind::Whitespace, TokenFlags::TRIVIA, 27, 28),
            (TokenKind::Url, TokenFlags::CONTAINS_ESCAPE, 28, 43),
            (TokenKind::Whitespace, TokenFlags::TRIVIA, 43, 44),
            (TokenKind::Url, TokenFlags::CONTAINS_ESCAPE, 44, 62),
            (TokenKind::Whitespace, TokenFlags::TRIVIA, 62, 63),
            (TokenKind::Ident, 0, 63, 66),
            (TokenKind::Whitespace, TokenFlags::TRIVIA, 66, 67),
            (TokenKind::BadUrl, TokenFlags::CONTAINS_ESCAPE, 67, 84),
        ]
    );
    assert_eq!(
        source.slice_tokens(Lexer::new(&source, CssDialect::Css)),
        input
    );
    assert_eq!(decode_css_identifier("a\0b").unwrap(), "a\u{fffd}b");
}

// @ai-generated - Verifies URL whitespace at EOF differs from whitespace before junk.
#[test]
fn url_whitespace_at_eof_and_before_junk_have_exact_kinds_and_spans() {
    for (input, expected) in [
        (
            "url(foo   ",
            (TokenKind::Url, TokenFlags::UNTERMINATED, 17, 27),
        ),
        ("url(foo   x)", (TokenKind::BadUrl, 0, 17, 29)),
    ] {
        let source = CssSource::new(Arc::from(input), 17).unwrap();
        assert_eq!(tokens(&source, CssDialect::Css), vec![expected]);
        assert_eq!(
            source.slice_tokens(Lexer::new(&source, CssDialect::Css)),
            input
        );
    }
}

#[test]
fn less_variables_do_not_steal_css_at_rules() {
    let input = "@tone: red; @media screen { .a { color: @tone; } } @future x;";
    let source = CssSource::new(Arc::from(input), 0).unwrap();
    let significant: Vec<_> = Lexer::new(&source, CssDialect::Less)
        .filter(|token| !token.kind().is_trivia())
        .collect();
    assert_eq!(significant[0].kind(), TokenKind::LessVariable);
    let at_keywords: Vec<_> = significant
        .iter()
        .filter(|token| token.kind() == TokenKind::AtKeyword)
        .map(|token| source.token_text(*token))
        .collect();
    assert_eq!(at_keywords, vec!["@media", "@future"]);
    assert!(significant
        .iter()
        .filter(|token| token.kind() == TokenKind::LessVariable)
        .map(|token| source.token_text(*token))
        .eq(["@tone", "@tone"]));

    let cst = parse_lossless(
        source,
        CssDialect::Less,
        CssEntryPoint::Stylesheet,
        CssParseMode::Strict,
    )
    .unwrap();
    let at_rule_kinds: Vec<_> = cst
        .nodes()
        .iter()
        .map(|node| node.kind())
        .filter(|kind| matches!(kind, SyntaxKind::GroupAtRule | SyntaxKind::UnknownAtRule))
        .collect();
    assert_eq!(
        at_rule_kinds,
        vec![SyntaxKind::GroupAtRule, SyntaxKind::UnknownAtRule]
    );
    assert_eq!(cst.reconstruct(), input);
}

// @ai-generated - Verifies Less declaration lookahead follows canonical trivia.
#[test]
fn less_variable_declaration_comments_have_exact_tokens_events_and_cst() {
    let input = "@tone/**/: red;";
    let source = CssSource::new(Arc::from(input), 23).unwrap();
    let expected_tokens = vec![
        SyntaxToken::new(
            TokenKind::LessVariable,
            TokenFlags::DIALECT_EXTENSION,
            23,
            28,
        ),
        SyntaxToken::new(TokenKind::Comment, TokenFlags::TRIVIA, 28, 32),
        SyntaxToken::new(TokenKind::Colon, 0, 32, 33),
        SyntaxToken::new(TokenKind::Whitespace, TokenFlags::TRIVIA, 33, 34),
        SyntaxToken::new(TokenKind::Ident, 0, 34, 37),
        SyntaxToken::new(TokenKind::Semicolon, 0, 37, 38),
    ];
    assert_eq!(
        Lexer::new(&source, CssDialect::Less).collect::<Vec<_>>(),
        expected_tokens
    );

    let mut sink = RecordingSink::default();
    parse_with_sink(
        &source,
        CssDialect::Less,
        CssEntryPoint::Stylesheet,
        CssParseMode::Strict,
        &mut sink,
    )
    .unwrap();
    assert_eq!(
        sink.events,
        vec![
            ParseEvent::StartNode {
                kind: SyntaxKind::Stylesheet,
                flags: NodeFlags::default(),
                start: 23,
            },
            ParseEvent::StartNode {
                kind: SyntaxKind::Declaration,
                flags: NodeFlags::default(),
                start: 23,
            },
            ParseEvent::Token(expected_tokens[0]),
            ParseEvent::Token(expected_tokens[1]),
            ParseEvent::Token(expected_tokens[2]),
            ParseEvent::StartNode {
                kind: SyntaxKind::ComponentValueList,
                flags: NodeFlags::default(),
                start: 33,
            },
            ParseEvent::Token(expected_tokens[3]),
            ParseEvent::Token(expected_tokens[4]),
            ParseEvent::FinishNode {
                kind: SyntaxKind::ComponentValueList,
                end: 37,
            },
            ParseEvent::Token(expected_tokens[5]),
            ParseEvent::FinishNode {
                kind: SyntaxKind::Declaration,
                end: 38,
            },
            ParseEvent::FinishNode {
                kind: SyntaxKind::Stylesheet,
                end: 38,
            },
        ]
    );

    let cst = parse_lossless(
        source.clone(),
        CssDialect::Less,
        CssEntryPoint::Stylesheet,
        CssParseMode::Strict,
    )
    .unwrap();
    let declaration = cst
        .nodes()
        .iter()
        .find(|node| node.kind() == SyntaxKind::Declaration)
        .unwrap();
    let value = cst
        .nodes()
        .iter()
        .find(|node| node.kind() == SyntaxKind::ComponentValueList)
        .unwrap();
    assert_eq!(declaration.span(), verter_span::Span::new(23, 38));
    assert_eq!(value.span(), verter_span::Span::new(33, 37));
    assert_eq!(source.slice(declaration.span()), input);
    assert!(cst.diagnostics().is_empty());
    assert_eq!(cst.reconstruct(), input);

    let controls = "@wrapped\n: blue; @line// stop\n: black; @media screen {} @future x;";
    let control_source = CssSource::new(Arc::from(controls), 41).unwrap();
    let heads: Vec<_> = Lexer::new(&control_source, CssDialect::Less)
        .filter(|token| matches!(token.kind(), TokenKind::LessVariable | TokenKind::AtKeyword))
        .map(|token| (token.kind(), control_source.token_text(token).to_owned()))
        .collect();
    assert_eq!(
        heads,
        vec![
            (TokenKind::LessVariable, "@wrapped".to_owned()),
            (TokenKind::AtKeyword, "@line".to_owned()),
            (TokenKind::AtKeyword, "@media".to_owned()),
            (TokenKind::AtKeyword, "@future".to_owned()),
        ]
    );
    let controls_cst = parse_lossless(
        control_source.clone(),
        CssDialect::Less,
        CssEntryPoint::Stylesheet,
        CssParseMode::Strict,
    )
    .unwrap();
    let mut structures: Vec<_> = controls_cst
        .nodes()
        .iter()
        .filter(|node| {
            matches!(
                node.kind(),
                SyntaxKind::Declaration | SyntaxKind::GroupAtRule | SyntaxKind::UnknownAtRule
            )
        })
        .map(|node| {
            (
                node.span().start,
                node.kind(),
                control_source.slice(node.span()).to_owned(),
            )
        })
        .collect();
    structures.sort_by_key(|structure| structure.0);
    assert_eq!(
        structures,
        vec![
            (41, SyntaxKind::Declaration, "@wrapped\n: blue;".to_owned()),
            (
                58,
                SyntaxKind::UnknownAtRule,
                "@line// stop\n: black;".to_owned()
            ),
            (80, SyntaxKind::GroupAtRule, "@media screen {}".to_owned()),
            (97, SyntaxKind::UnknownAtRule, "@future x;".to_owned()),
        ]
    );
    assert!(controls_cst.diagnostics().is_empty());
    assert_eq!(controls_cst.reconstruct(), controls);
}

#[test]
fn first_byte_dispatch_keeps_cdo_cdc_ident_and_left_angle_boundaries() {
    let source = CssSource::new(Arc::from("<!-- --> --foo < !"), 0).unwrap();
    let kinds: Vec<_> = tokens(&source, CssDialect::Css)
        .into_iter()
        .map(|token| token.0)
        .collect();
    assert_eq!(
        kinds,
        vec![
            TokenKind::Cdo,
            TokenKind::Whitespace,
            TokenKind::Cdc,
            TokenKind::Whitespace,
            TokenKind::Ident,
            TokenKind::Whitespace,
            TokenKind::Delim,
            TokenKind::Whitespace,
            TokenKind::Delim,
        ]
    );
    assert_eq!(
        source.slice_tokens(Lexer::new(&source, CssDialect::Css)),
        source.text()
    );
}

#[test]
fn unescaped_cr_or_form_feed_in_a_string_is_a_bad_string() {
    for input in ["\"x\ry\"", "\"x\u{c}y\""] {
        let source = CssSource::new(Arc::from(input), 0).unwrap();
        let kinds: Vec<_> = tokens(&source, CssDialect::Css)
            .into_iter()
            .map(|token| token.0)
            .collect();
        assert_eq!(
            kinds.first().copied(),
            Some(TokenKind::BadString),
            "{input:?}"
        );
        assert_ne!(
            kinds,
            vec![TokenKind::String],
            "an unescaped CSS newline must not stay inside a string: {input:?}"
        );
    }
}

#[test]
fn comment_form_feed_sets_contains_newline_and_keeps_the_closer() {
    let input = "/*a\x0cb*/";
    let source = CssSource::new(Arc::from(input), 0).unwrap();
    let actual = tokens(&source, CssDialect::Css);
    assert_eq!(actual.len(), 1, "{actual:?}");
    assert_eq!(actual[0].0, TokenKind::Comment);
    assert_ne!(
        actual[0].1 & TokenFlags::CONTAINS_NEWLINE,
        0,
        "form feed is a CSS newline"
    );
    assert_eq!(
        source.slice_tokens(Lexer::new(&source, CssDialect::Css)),
        source.text()
    );
}

fn discover_css_files(root: &Path, current: &Path, out: &mut BTreeSet<String>) {
    for entry in fs::read_dir(current).unwrap() {
        let entry = entry.unwrap();
        let path = entry.path();
        if path.is_dir() {
            discover_css_files(root, &path, out);
        } else if path.extension().and_then(|value| value.to_str()) == Some("css") {
            out.insert(
                path.strip_prefix(root)
                    .unwrap()
                    .to_string_lossy()
                    .replace('\\', "/"),
            );
        }
    }
}

fn token_fixture_facts(source: &CssSource) -> String {
    Lexer::new(source, CssDialect::Css)
        .map(|token| {
            format!(
                "{:?}:{}:{}..{}",
                token.kind(),
                token.flags,
                token.start,
                token.end
            )
        })
        .collect::<Vec<_>>()
        .join(",")
}

fn recovery_fixture_facts(source: &CssSource) -> String {
    let tokens = token_fixture_facts(source);
    let cst = parse_lossless(
        source.clone(),
        CssDialect::Css,
        CssEntryPoint::Stylesheet,
        CssParseMode::Recover,
    )
    .unwrap();
    let diagnostics = cst
        .diagnostics()
        .iter()
        .map(|value| {
            format!(
                "{:?}@{}..{}:{:?}",
                value.kind, value.span.start, value.span.end, value.recovery
            )
        })
        .collect::<Vec<_>>()
        .join(",");
    let recoveries = cst
        .nodes()
        .iter()
        .filter(|node| node.kind() == SyntaxKind::Recovery)
        .map(|node| format!("{}..{}", node.start, node.end))
        .collect::<Vec<_>>()
        .join(",");
    let rules = cst
        .nodes()
        .iter()
        .filter(|node| node.kind() == SyntaxKind::QualifiedRule)
        .count();
    format!("tokens={tokens};diagnostics={diagnostics};recoveries={recoveries};rules={rules}")
}

fn selector_fixture_facts(source: &CssSource) -> String {
    let structure = parse_selector_structure(source, CssDialect::Css).unwrap();
    let mut namespaces: Vec<_> = structure
        .components()
        .iter()
        .filter(|value| value.kind() == SelectorComponentKind::Namespace)
        .map(|value| format!("{}..{}", value.span().start, value.span().end))
        .collect();
    namespaces.sort_by_key(|value| {
        value
            .split_once("..")
            .and_then(|(start, _)| start.parse::<u32>().ok())
            .unwrap()
    });
    let mut nesting: Vec<_> = structure
        .components()
        .iter()
        .filter(|value| value.kind() == SelectorComponentKind::Nesting)
        .map(|value| format!("{}..{}", value.span().start, value.span().end))
        .collect();
    nesting.sort_by_key(|value| {
        value
            .split_once("..")
            .and_then(|(start, _)| start.parse::<u32>().ok())
            .unwrap()
    });
    let mut combinators: Vec<_> = structure
        .combinators()
        .iter()
        .map(|value| {
            format!(
                "{:?}@{}..{}",
                value.kind(),
                value.span().start,
                value.span().end
            )
        })
        .collect();
    combinators.sort_by_key(|value| {
        value
            .split_once('@')
            .and_then(|(_, span)| span.split_once(".."))
            .and_then(|(start, _)| start.parse::<u32>().ok())
            .unwrap()
    });
    let mut attributes: Vec<_> = structure
        .attributes()
        .iter()
        .map(|value| {
            format!(
                "{:?}@{}..{}",
                value.matcher(),
                value.span().start,
                value.span().end
            )
        })
        .collect();
    attributes.sort_by_key(|value| {
        value
            .split_once('@')
            .and_then(|(_, span)| span.split_once(".."))
            .and_then(|(start, _)| start.parse::<u32>().ok())
            .unwrap()
    });
    let mut pseudos: Vec<_> = structure
        .pseudos()
        .iter()
        .map(|value| {
            format!(
                "{:?}:{}:{:?}@{}..{}",
                value.kind(),
                value.selector_count(),
                value.nth(),
                value.span().start,
                value.span().end
            )
        })
        .collect();
    pseudos.sort_by_key(|value| {
        value
            .rsplit_once('@')
            .and_then(|(_, span)| span.split_once(".."))
            .and_then(|(start, _)| start.parse::<u32>().ok())
            .unwrap()
    });
    format!(
        "top={};namespaces={};nesting={};combinators={};attributes={};pseudos={}",
        structure.top_level_selector_count(),
        namespaces.join(","),
        nesting.join(","),
        combinators.join(","),
        attributes.join(","),
        pseudos.join(",")
    )
}

#[test]
fn every_wpt_fixture_is_manifested_and_executed() {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("fixtures")
        .join("wpt");
    let manifest = fs::read_to_string(root.join("MANIFEST.tsv")).unwrap();
    let license = fs::read_to_string(root.join("LICENSE.md")).unwrap();
    let readme = fs::read_to_string(root.join("README.md")).unwrap();
    assert!(license.contains("Copyright © web-platform-tests contributors"));
    assert!(license.contains("3-Clause BSD License"));
    assert!(readme.contains("7aed6630812b20e6eec2a2e40594f8dfda036e00"));
    let mut rows = BTreeSet::new();
    let mut executed = 0usize;
    let skipped = 0usize;
    let mut semantic_mismatches = Vec::new();

    for line in manifest
        .lines()
        .filter(|line| !line.is_empty() && !line.starts_with('#'))
    {
        let fields: Vec<_> = line.split('\t').collect();
        assert!(
            matches!(fields.len(), 7 | 8),
            "invalid manifest row: {line}"
        );
        assert_eq!(fields[5], "7aed6630812b20e6eec2a2e40594f8dfda036e00");
        assert!(
            rows.insert(fields[0].to_owned()),
            "duplicate row: {}",
            fields[0]
        );
        let fixture = fs::read_to_string(root.join(fields[0])).unwrap();
        let (boundary, expected) = if fields.len() == 8 {
            (fields[6], fields[7])
        } else {
            ("whole", fields[6])
        };
        let cases: Vec<&str> = match boundary {
            "whole" => vec![fixture.as_str()],
            "lf-eof-cases" => fixture.lines().collect(),
            boundary => panic!("unknown WPT case boundary {boundary}"),
        };
        assert!(
            !cases.is_empty(),
            "fixture has no authored cases: {}",
            fields[0]
        );
        let actual = cases
            .into_iter()
            .map(|case| {
                let source = CssSource::new(Arc::from(case), 0).unwrap();
                executed += 1;
                match fields[4] {
                    "token" => token_fixture_facts(&source),
                    "recovery" => recovery_fixture_facts(&source),
                    "selector" => selector_fixture_facts(&source),
                    family => panic!("unknown WPT fixture family {family}"),
                }
            })
            .collect::<Vec<_>>()
            .join(" || ");
        if actual != expected {
            semantic_mismatches.push(format!(
                "{}\nexpected={}\nactual={actual}",
                fields[0], expected
            ));
        }
    }

    let mut discovered = BTreeSet::new();
    discover_css_files(&root, &root, &mut discovered);
    assert_eq!(
        rows, discovered,
        "manifest and directory discovery diverged"
    );
    assert!(
        executed >= 21,
        "expected all curated WPT fixtures to execute"
    );
    assert_eq!(skipped, 0, "WPT fixtures may not be skipped");
    assert!(
        semantic_mismatches.is_empty(),
        "WPT semantic oracle mismatch:\n{}",
        semantic_mismatches.join("\n\n")
    );
}
