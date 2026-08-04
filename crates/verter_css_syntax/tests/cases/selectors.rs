use std::sync::Arc;

use verter_css_syntax::{
    parse_lossless, parse_selector_structure, AttributeMatcher, CombinatorKind, CssDialect,
    CssEntryPoint, CssParseMode, CssSource, NthExpression, PseudoFunctionKind,
    SelectorComponentKind, TokenFlags, TokenKind,
};

#[test]
fn wpt_selector_structure_and_spans() {
    let input = r#"svg|a#hero.button\? > & + [data-label="x,.fake}"][lang|=en]::before:is(.a, :where(.b,.c)):has(> img), *|x:not(.z)"#;
    let source = CssSource::new(Arc::from(input), 13).unwrap();
    let structure = parse_selector_structure(&source, CssDialect::Css).unwrap();

    assert_eq!(structure.span().start, 13);
    assert_eq!(structure.span().end, 13 + input.len() as u32);
    assert_eq!(structure.top_level_selector_count(), 2);
    assert!(structure
        .components()
        .iter()
        .any(|component| component.kind() == SelectorComponentKind::Namespace));
    assert!(structure
        .components()
        .iter()
        .any(|component| component.kind() == SelectorComponentKind::Nesting));
    assert!(structure
        .components()
        .iter()
        .any(|component| component.kind() == SelectorComponentKind::PseudoElement));
    assert!(structure
        .attributes()
        .iter()
        .any(|attribute| attribute.matcher() == Some(AttributeMatcher::DashMatch)));
    assert!(structure
        .combinators()
        .iter()
        .any(|value| value.kind() == CombinatorKind::Child));
    assert!(structure
        .pseudos()
        .iter()
        .any(|pseudo| pseudo.kind() == PseudoFunctionKind::Is && pseudo.selector_count() == 2));
    assert!(structure
        .pseudos()
        .iter()
        .any(|pseudo| pseudo.kind() == PseudoFunctionKind::Has));
    assert_eq!(
        source.slice(structure.attributes()[0].span()),
        r#"[data-label="x,.fake}"]"#
    );
}

// @ai-generated - Proves list/complex/compound hierarchy and exact selector-name spans.
#[test]
fn selector_structure_exposes_hierarchical_type_class_and_id_components() {
    let input = "svg.card#hero > .label, button.primary";
    let source = CssSource::new(Arc::from(input), 9).unwrap();
    let structure = parse_selector_structure(&source, CssDialect::Css).unwrap();

    assert_eq!(structure.list().selectors().len(), 2);
    let first = &structure.list().selectors()[0];
    assert_eq!(first.compounds().len(), 2);
    assert_eq!(first.combinators().len(), 1);

    let components: Vec<_> = first
        .compounds()
        .iter()
        .flat_map(|compound| compound.components())
        .collect();
    assert_eq!(components[0].kind(), SelectorComponentKind::Type);
    assert_eq!(source.slice(components[0].name_span().unwrap()), "svg");
    assert_eq!(components[1].kind(), SelectorComponentKind::Class);
    assert_eq!(source.slice(components[1].name_span().unwrap()), "card");
    assert_eq!(components[2].kind(), SelectorComponentKind::Id);
    assert_eq!(source.slice(components[2].name_span().unwrap()), "hero");
    assert!(structure.facts().is_complete_static());
}

// @ai-generated - Proves interpolation keeps fragments but never publishes a concrete class.
#[test]
fn interpolated_class_is_dynamic_with_positioned_fragments() {
    let input = ".icon-#{tone}-active, .safe";
    let source = CssSource::new(Arc::from(input), 20).unwrap();
    let structure = parse_selector_structure(&source, CssDialect::Scss).unwrap();
    let dynamic = structure.list().selectors()[0].compounds()[0]
        .components()
        .iter()
        .find(|component| component.kind() == SelectorComponentKind::DynamicClass)
        .expect("interpolated class is typed dynamic");

    assert_eq!(dynamic.name_span(), None);
    let fragments: Vec<_> = dynamic
        .static_fragments()
        .iter()
        .map(|span| source.slice(*span))
        .collect();
    assert_eq!(fragments, vec!["icon-", "-active"]);
    assert_eq!(dynamic.interpolations().len(), 1);
    assert_eq!(
        source.slice(dynamic.interpolations()[0].full_span()),
        "#{tone}"
    );
    assert!(structure.facts().is_dynamic());

    let safe = &structure.list().selectors()[1].compounds()[0].components()[0];
    assert_eq!(safe.kind(), SelectorComponentKind::Class);
    assert_eq!(source.slice(safe.name_span().unwrap()), "safe");
}

#[test]
fn nth_child_and_nth_last_child_of_are_structural() {
    let input = ":nth-child(2n + 1 of .item, [hidden=\"a,b\"]), :nth-last-child(odd of :is(.a,.b))";
    let source = CssSource::new(Arc::from(input), 0).unwrap();
    let structure = parse_selector_structure(&source, CssDialect::Css).unwrap();
    let nth: Vec<_> = structure
        .pseudos()
        .into_iter()
        .filter(|pseudo| {
            matches!(
                pseudo.kind(),
                PseudoFunctionKind::NthChild | PseudoFunctionKind::NthLastChild
            )
        })
        .collect();

    assert_eq!(nth.len(), 2);
    assert!(nth.iter().all(|pseudo| pseudo.nth().is_some()));
    assert_eq!(nth[0].nth().unwrap().a, 2);
    assert_eq!(nth[0].nth().unwrap().b, 1);
    assert_eq!(nth[0].selector_count(), 2);
    assert_eq!(nth[1].nth().unwrap().a, 2);
    assert_eq!(nth[1].nth().unwrap().b, 1);
    assert_eq!(nth[1].selector_count(), 1);
}

// @ai-generated - Verifies matcher facts use top-level, whitespace-sensitive tokens.
#[test]
fn attribute_matchers_require_top_level_comment_transparent_adjacency() {
    for (operator, matcher) in [
        ("~", AttributeMatcher::Includes),
        ("|", AttributeMatcher::DashMatch),
        ("^", AttributeMatcher::Prefix),
        ("$", AttributeMatcher::Suffix),
        ("*", AttributeMatcher::Substring),
    ] {
        for (input, expected) in [
            (format!("[x{operator}=v]"), Some(matcher)),
            (format!("[x{operator}/**/=v]"), Some(matcher)),
            (format!("[x{operator} =v]"), None),
            (format!("[x fn(a{operator}=b)]"), None),
        ] {
            let source = CssSource::new(Arc::<str>::from(input.as_str()), 13).unwrap();
            let structure = parse_selector_structure(&source, CssDialect::Css).unwrap();
            assert_eq!(structure.attributes().len(), 1, "{input}");
            assert_eq!(
                structure.attributes()[0].span(),
                verter_span::Span::new(13, 13 + input.len() as u32),
                "{input}"
            );
            assert_eq!(structure.attributes()[0].matcher(), expected, "{input}");

            let cst = parse_lossless(
                source.clone(),
                CssDialect::Css,
                CssEntryPoint::SelectorList,
                CssParseMode::Strict,
            )
            .unwrap();
            if input.contains("/**/") {
                let comments: Vec<_> = cst
                    .tokens()
                    .iter()
                    .filter(|token| token.kind() == TokenKind::Comment)
                    .map(|token| {
                        (
                            token.flags,
                            token.start,
                            token.end,
                            source.token_text(*token),
                        )
                    })
                    .collect();
                assert_eq!(
                    comments,
                    vec![(TokenFlags::TRIVIA, 16, 20, "/**/")],
                    "{input}"
                );
            }
            assert_eq!(cst.reconstruct(), input, "{input}");
        }
    }

    for (input, expected) in [
        ("[x=v]", Some(AttributeMatcher::Exact)),
        ("[x = v]", Some(AttributeMatcher::Exact)),
        ("[ns|x|=v]", Some(AttributeMatcher::DashMatch)),
        ("[|x^=v]", Some(AttributeMatcher::Prefix)),
        ("[*|x$=v]", Some(AttributeMatcher::Suffix)),
        ("[x fn(a=b)]", None),
        ("[x {a=b}]", None),
    ] {
        let source = CssSource::new(Arc::from(input), 23).unwrap();
        let structure = parse_selector_structure(&source, CssDialect::Css).unwrap();
        assert_eq!(structure.attributes().len(), 1, "{input}");
        assert_eq!(structure.attributes()[0].matcher(), expected, "{input}");
        assert_eq!(
            structure.attributes()[0].span(),
            verter_span::Span::new(23, 23 + input.len() as u32),
            "{input}"
        );
        let cst = parse_lossless(
            source,
            CssDialect::Css,
            CssEntryPoint::SelectorList,
            CssParseMode::Strict,
        )
        .unwrap();
        assert_eq!(cst.reconstruct(), input, "{input}");
    }
}

// @ai-generated - Verifies wildcard admission is limited to namespace prefixes.
#[test]
fn attribute_wildcards_cannot_be_local_names_for_any_matcher() {
    for local in ["*", "|*", "*|*"] {
        for operator in ["=", "~=", "|=", "^=", "$=", "*="] {
            let input = format!("[{local}{operator}v]");
            let source = CssSource::new(Arc::<str>::from(input.as_str()), 17).unwrap();
            let structure = parse_selector_structure(&source, CssDialect::Css).unwrap();
            assert_eq!(structure.attributes().len(), 1, "{input}");
            assert_eq!(
                structure.attributes()[0].span(),
                verter_span::Span::new(17, 17 + input.len() as u32),
                "{input}"
            );
            assert_eq!(structure.attributes()[0].matcher(), None, "{input}");

            let cst = parse_lossless(
                source,
                CssDialect::Css,
                CssEntryPoint::SelectorList,
                CssParseMode::Strict,
            )
            .unwrap();
            assert_eq!(cst.reconstruct(), input, "{input}");
        }
    }

    for (input, matcher) in [
        ("[*|x=v]", AttributeMatcher::Exact),
        ("[*|x$=v]", AttributeMatcher::Suffix),
        ("[*/**/|/**/x$/**/=v]", AttributeMatcher::Suffix),
    ] {
        let source = CssSource::new(Arc::from(input), 31).unwrap();
        let structure = parse_selector_structure(&source, CssDialect::Css).unwrap();
        assert_eq!(structure.attributes().len(), 1, "{input}");
        assert_eq!(
            structure.attributes()[0].matcher(),
            Some(matcher),
            "{input}"
        );
        assert_eq!(
            structure.attributes()[0].span(),
            verter_span::Span::new(31, 31 + input.len() as u32),
            "{input}"
        );
        let cst = parse_lossless(
            source,
            CssDialect::Css,
            CssEntryPoint::SelectorList,
            CssParseMode::Strict,
        )
        .unwrap();
        assert_eq!(cst.reconstruct(), input, "{input}");
    }
}

// @ai-generated - Verifies escaped dimension units retain numeric An+B facts.
#[test]
fn escaped_n_dimension_units_preserve_nth_formulas_and_of_selectors() {
    let input = concat!(
        r":nth-child(2\6e + 1 of .a, .b), ",
        ":nth-child(2n + 1 of .a, .b), ",
        r":nth-last-child(-3\6e + 2 of :is(.x,.y), .z), ",
        ":nth-last-child(-3n + 2 of :is(.x,.y), .z)"
    );
    let source = CssSource::new(Arc::from(input), 29).unwrap();
    let structure = parse_selector_structure(&source, CssDialect::Css).unwrap();
    let nth: Vec<_> = structure
        .pseudos()
        .iter()
        .filter(|pseudo| {
            matches!(
                pseudo.kind(),
                PseudoFunctionKind::NthChild | PseudoFunctionKind::NthLastChild
            )
        })
        .map(|pseudo| {
            (
                pseudo.kind(),
                pseudo.nth(),
                pseudo.selector_count(),
                source.slice(pseudo.span()).to_owned(),
                source.slice(pseudo.argument_span()).to_owned(),
            )
        })
        .collect();

    assert_eq!(structure.top_level_selector_count(), 4);
    assert_eq!(
        nth,
        vec![
            (
                PseudoFunctionKind::NthChild,
                Some(NthExpression { a: 2, b: 1 }),
                2,
                r":nth-child(2\6e + 1 of .a, .b)".to_owned(),
                r"2\6e + 1 of .a, .b".to_owned(),
            ),
            (
                PseudoFunctionKind::NthChild,
                Some(NthExpression { a: 2, b: 1 }),
                2,
                ":nth-child(2n + 1 of .a, .b)".to_owned(),
                "2n + 1 of .a, .b".to_owned(),
            ),
            (
                PseudoFunctionKind::NthLastChild,
                Some(NthExpression { a: -3, b: 2 }),
                2,
                r":nth-last-child(-3\6e + 2 of :is(.x,.y), .z)".to_owned(),
                r"-3\6e + 2 of :is(.x,.y), .z".to_owned(),
            ),
            (
                PseudoFunctionKind::NthLastChild,
                Some(NthExpression { a: -3, b: 2 }),
                2,
                ":nth-last-child(-3n + 2 of :is(.x,.y), .z)".to_owned(),
                "-3n + 2 of :is(.x,.y), .z".to_owned(),
            ),
        ]
    );
    let nested_is_counts: Vec<_> = structure
        .pseudos()
        .iter()
        .filter(|pseudo| pseudo.kind() == PseudoFunctionKind::Is)
        .map(|pseudo| pseudo.selector_count())
        .collect();
    assert_eq!(nested_is_counts, vec![2, 2]);

    let cst = parse_lossless(
        source.clone(),
        CssDialect::Css,
        CssEntryPoint::SelectorList,
        CssParseMode::Strict,
    )
    .unwrap();
    let dimensions: Vec<_> = cst
        .tokens()
        .iter()
        .filter(|token| token.kind() == TokenKind::Dimension)
        .map(|token| {
            (
                source.token_text(*token).to_owned(),
                token.flags,
                token.start,
                token.end,
            )
        })
        .collect();
    assert_eq!(
        dimensions,
        vec![
            (
                r"2\6e ".to_owned(),
                TokenFlags::NUMBER_INTEGER | TokenFlags::CONTAINS_ESCAPE,
                40,
                45,
            ),
            ("2n".to_owned(), TokenFlags::NUMBER_INTEGER, 72, 74),
            (
                r"-3\6e ".to_owned(),
                TokenFlags::NUMBER_INTEGER | TokenFlags::CONTAINS_ESCAPE,
                107,
                113,
            ),
            ("-3n".to_owned(), TokenFlags::NUMBER_INTEGER, 153, 156),
        ]
    );
    assert_eq!(cst.reconstruct(), input);
}

// @ai-generated - Verifies An+B facts preserve canonical token boundaries.
#[test]
fn nth_formulas_respect_token_boundaries_signs_comments_and_of_lists() {
    for (input, expected, selector_count) in [
        (":nth-child(2n)", Some(NthExpression { a: 2, b: 0 }), 0),
        (":nth-child(n + 1)", Some(NthExpression { a: 1, b: 1 }), 0),
        (
            ":nth-child(n/**/+/**/1)",
            Some(NthExpression { a: 1, b: 1 }),
            0,
        ),
        (
            r":nth-child(2\6e /**/-/**/1 of .a,.b)",
            Some(NthExpression { a: 2, b: -1 }),
            2,
        ),
        (
            ":nth-last-child(+n - 2 of .x)",
            Some(NthExpression { a: 1, b: -2 }),
            1,
        ),
        (
            ":nth-last-child(-n+2 of .x)",
            Some(NthExpression { a: -1, b: 2 }),
            1,
        ),
        (":nth-child(n-3)", Some(NthExpression { a: 1, b: -3 }), 0),
        (":nth-child(2n-3)", Some(NthExpression { a: 2, b: -3 }), 0),
        (":nth-child(2 n)", None, 0),
        (":nth-child(n 1)", None, 0),
        (":nth-child(2/**/n)", None, 0),
        (":nth-child(n/**/1)", None, 0),
        (":nth-child(+ n)", None, 0),
        (":nth-child(n + +1)", None, 0),
        (":nth-child(n - -1)", None, 0),
        (":nth-child(2n 1)", None, 0),
        (":nth-child(2.5n)", None, 0),
        (":nth-child(n +)", None, 0),
    ] {
        let source = CssSource::new(Arc::from(input), 43).unwrap();
        let structure = parse_selector_structure(&source, CssDialect::Css).unwrap();
        let pseudo = structure
            .pseudos()
            .into_iter()
            .find(|pseudo| {
                matches!(
                    pseudo.kind(),
                    PseudoFunctionKind::NthChild | PseudoFunctionKind::NthLastChild
                )
            })
            .unwrap();
        assert_eq!(pseudo.nth(), expected, "{input}");
        assert_eq!(pseudo.selector_count(), selector_count, "{input}");
        assert_eq!(source.slice(pseudo.span()), input, "{input}");

        let cst = parse_lossless(
            source,
            CssDialect::Css,
            CssEntryPoint::SelectorList,
            CssParseMode::Strict,
        )
        .unwrap();
        assert_eq!(cst.reconstruct(), input, "{input}");
    }

    for (input, expected) in [
        (
            ":nth-child(2 n)",
            vec![
                (TokenKind::Number, "2"),
                (TokenKind::Whitespace, " "),
                (TokenKind::Ident, "n"),
            ],
        ),
        (
            ":nth-child(n 1)",
            vec![
                (TokenKind::Ident, "n"),
                (TokenKind::Whitespace, " "),
                (TokenKind::Number, "1"),
            ],
        ),
        (
            ":nth-child(n/**/+/**/1)",
            vec![
                (TokenKind::Ident, "n"),
                (TokenKind::Comment, "/**/"),
                (TokenKind::Delim, "+"),
                (TokenKind::Comment, "/**/"),
                (TokenKind::Number, "1"),
            ],
        ),
    ] {
        let source = CssSource::new(Arc::from(input), 67).unwrap();
        let cst = parse_lossless(
            source.clone(),
            CssDialect::Css,
            CssEntryPoint::SelectorList,
            CssParseMode::Strict,
        )
        .unwrap();
        let function = cst
            .tokens()
            .iter()
            .position(|token| token.kind() == TokenKind::Function)
            .unwrap();
        let arguments: Vec<_> = cst.tokens()[function + 1..]
            .iter()
            .take_while(|token| token.kind() != TokenKind::RightParen)
            .map(|token| (token.kind(), source.token_text(*token)))
            .collect();
        assert_eq!(arguments, expected, "{input}");
        assert_eq!(cst.reconstruct(), input, "{input}");
    }
}

#[test]
fn unknown_functional_pseudos_keep_lossless_component_values() {
    let input = r#".a:future(foo, [x="a,b"], {raw:value})"#;
    let source = CssSource::new(Arc::from(input), 0).unwrap();
    let structure = parse_selector_structure(&source, CssDialect::Css).unwrap();
    let pseudo = structure
        .pseudos()
        .into_iter()
        .find(|pseudo| pseudo.kind() == PseudoFunctionKind::Unknown)
        .unwrap();

    assert_eq!(
        source.slice(pseudo.span()),
        ":future(foo, [x=\"a,b\"], {raw:value})"
    );
    assert_eq!(
        source.slice(pseudo.argument_span()),
        "foo, [x=\"a,b\"], {raw:value}"
    );
    assert_eq!(pseudo.selector_count(), 0);
}

#[test]
fn combinators_are_exact_and_whitespace_is_only_descendant_between_compounds() {
    let input = "  .a > .b+.c ~ .d||.e, > .relative .tail, .x |element  ";
    let source = CssSource::new(Arc::from(input), 9).unwrap();
    let structure = parse_selector_structure(&source, CssDialect::Css).unwrap();
    let actual: Vec<_> = structure
        .combinators()
        .iter()
        .map(|value| (value.kind(), source.slice(value.span()).to_owned()))
        .collect();

    assert_eq!(
        actual,
        vec![
            (CombinatorKind::Child, ">".to_owned()),
            (CombinatorKind::NextSibling, "+".to_owned()),
            (CombinatorKind::LaterSibling, "~".to_owned()),
            (CombinatorKind::Column, "||".to_owned()),
            (CombinatorKind::Child, ">".to_owned()),
            (CombinatorKind::Descendant, " ".to_owned()),
            (CombinatorKind::Descendant, " ".to_owned()),
        ]
    );
}

#[test]
fn selector_facts_come_from_canonical_tokens_and_events() {
    let input = r#"|element, *|x, [|att], [*|att], :i\73(.a/*,*/, .b), :nth-child(2n/*x*/of.item), :future(ns|x)"#;
    let source = CssSource::new(Arc::from(input), 23).unwrap();
    let structure = parse_selector_structure(&source, CssDialect::Css).unwrap();
    let namespaces: Vec<_> = structure
        .components()
        .iter()
        .filter(|component| component.kind() == SelectorComponentKind::Namespace)
        .map(|component| source.slice(component.span()).to_owned())
        .collect();
    let mut pseudos: Vec<_> = structure.pseudos();
    pseudos.sort_by_key(|pseudo| pseudo.span().start);

    assert_eq!(namespaces, vec!["|element", "*|x", "|att", "*|att"]);
    assert_eq!(pseudos[0].kind(), PseudoFunctionKind::Is);
    assert_eq!(pseudos[0].selector_count(), 2);
    assert_eq!(pseudos[1].kind(), PseudoFunctionKind::NthChild);
    assert_eq!(pseudos[1].nth().unwrap().a, 2);
    assert_eq!(pseudos[1].selector_count(), 1);
    assert_eq!(pseudos[2].kind(), PseudoFunctionKind::Unknown);
    assert_eq!(
        structure
            .components()
            .iter()
            .filter(|component| component.kind() == SelectorComponentKind::Namespace)
            .count(),
        4,
        "unknown component-value arguments must not synthesize namespaces"
    );
}

// @ai-generated - Verifies comments preserve adjacency without becoming whitespace.
#[test]
fn comments_are_transparent_for_selector_adjacency_but_whitespace_is_not() {
    let input = ":/**/is(.a,.b), ns/**/|element, .a|/**/|.b";
    let source = CssSource::new(Arc::from(input), 17).unwrap();
    let structure = parse_selector_structure(&source, CssDialect::Css).unwrap();
    let pseudo = structure
        .pseudos()
        .into_iter()
        .find(|pseudo| pseudo.kind() == PseudoFunctionKind::Is)
        .unwrap();
    let namespaces: Vec<_> = structure
        .components()
        .iter()
        .filter(|component| component.kind() == SelectorComponentKind::Namespace)
        .map(|component| component.span())
        .collect();
    let columns: Vec<_> = structure
        .combinators()
        .iter()
        .filter(|combinator| combinator.kind() == CombinatorKind::Column)
        .map(|combinator| combinator.span())
        .collect();

    assert_eq!(pseudo.span(), verter_span::Span::new(17, 31));
    assert_eq!(pseudo.selector_count(), 2);
    assert_eq!(namespaces, vec![verter_span::Span::new(33, 47)]);
    assert_eq!(columns, vec![verter_span::Span::new(51, 57)]);
    assert_eq!(source.slice(namespaces[0]), "ns/**/|element");
    assert_eq!(source.slice(columns[0]), "|/**/|");

    let cst = parse_lossless(
        source.clone(),
        CssDialect::Css,
        CssEntryPoint::SelectorList,
        CssParseMode::Strict,
    )
    .unwrap();
    let comments: Vec<_> = cst
        .tokens()
        .iter()
        .filter(|token| token.kind() == TokenKind::Comment)
        .map(|token| (source.token_text(*token).to_owned(), token.start, token.end))
        .collect();
    assert_eq!(
        comments,
        vec![
            ("/**/".to_owned(), 18, 22),
            ("/**/".to_owned(), 35, 39),
            ("/**/".to_owned(), 52, 56),
        ]
    );
    assert_eq!(cst.reconstruct(), input);

    let pseudo_whitespace = CssSource::new(Arc::from(": /**/is(.a,.b)"), 0).unwrap();
    let pseudo_whitespace = parse_selector_structure(&pseudo_whitespace, CssDialect::Css).unwrap();
    assert!(!pseudo_whitespace
        .pseudos()
        .iter()
        .any(|pseudo| pseudo.kind() == PseudoFunctionKind::Is));

    let namespace_whitespace = CssSource::new(Arc::from("ns /**/|element"), 0).unwrap();
    let namespace_structure =
        parse_selector_structure(&namespace_whitespace, CssDialect::Css).unwrap();
    let namespace_texts: Vec<_> = namespace_structure
        .components()
        .iter()
        .filter(|component| component.kind() == SelectorComponentKind::Namespace)
        .map(|component| namespace_whitespace.slice(component.span()).to_owned())
        .collect();
    assert_eq!(namespace_texts, vec!["|element"]);

    let column_whitespace = CssSource::new(Arc::from(".a| /**/|.b"), 0).unwrap();
    let column_whitespace = parse_selector_structure(&column_whitespace, CssDialect::Css).unwrap();
    assert!(!column_whitespace
        .combinators()
        .iter()
        .any(|combinator| combinator.kind() == CombinatorKind::Column));
}

#[test]
fn dialect_interpolations_balance_in_selector_grammar() {
    for (dialect, input, expected) in [
        (CssDialect::Scss, ".item-#{$name}", "#{$name}"),
        (CssDialect::Less, ".item-@{name}", "@{name}"),
    ] {
        let source = CssSource::new(Arc::from(input), 31).unwrap();
        let cst = verter_css_syntax::parse_lossless(
            source.clone(),
            dialect,
            verter_css_syntax::CssEntryPoint::SelectorList,
            verter_css_syntax::CssParseMode::Strict,
        )
        .unwrap();
        let interpolation = cst
            .nodes()
            .iter()
            .find(|node| {
                node.kind() == verter_css_syntax::SyntaxKind::Interpolation
                    && node.flags & verter_css_syntax::NodeFlags::DIALECT_EXTENSION.0 != 0
            })
            .unwrap();
        assert_eq!(source.slice(interpolation.span()), expected);
        assert_eq!(cst.reconstruct(), input);
    }
}
