//! J1-A10c: no Native-supported construct in any of the 5 dialects depends on
//! a second parser or fallback. Every construct in the closed `SyntaxKind`
//! universe is resolved by exactly one [`parse_style_ir`] call — the sole
//! `StyleSyntaxIr` parse entry. Layout-parser subparses of Sass/Stylus are
//! the same grammar (`parse_with_sink`), not a second `parse_style_ir`.
//!
//! The construct universe is the same closed `SyntaxKind` set A3 enumerates.
//! A new variant is a compile error (`E0004`) until dispositioned here.

use std::sync::Arc;

use std::collections::HashSet;

use verter_css_syntax::{
    parse_style_ir, parse_style_ir_thread_invocations, ComplexSelectorPart, ComponentValue,
    CssDialect, CssParseMode, CssSource, SelectorComponentKind, StyleBlockKind, StyleStatement,
    StyleSyntaxIr, SyntaxKind, UnknownStatementKind,
};

const DIALECTS: [CssDialect; 5] = CssDialect::ALL;

type Case = (CssDialect, &'static str, CssParseMode);

enum Coverage {
    Symmetric(&'static str, CssParseMode),
    PerDialect(&'static str, &'static [Case]),
}

fn coverage(kind: SyntaxKind) -> Coverage {
    use CssDialect::{Less, Sass, Scss, Stylus};
    use CssParseMode::{Recover, Strict};

    const BASIC: &str = "div.a#b { color: red; }";
    const RICH_SELECTOR: &str = "svg|a > .b + [x=\"y\"] ~ .c { color: red; }";
    const PSEUDOS: &str =
        "a:hover::before:is(.x):nth-child(2n+1 of .y):unknownfn(z) { color: red; }";
    const INDENTED: &str = ".a\n  color: red\n";
    const MIXIN_BRACED: &str = "@mixin m($a) { color: $a; }";
    const CONTROL_BRACED: &str = "@if $x { .a { color: red; } }";
    const CONTROL_INDENTED: &str = "@if $x\n  .a\n    color: red\n";

    match kind {
        SyntaxKind::Stylesheet
        | SyntaxKind::QualifiedRule
        | SyntaxKind::RuleBlock
        | SyntaxKind::Declaration
        | SyntaxKind::ComponentValueList
        | SyntaxKind::Selector
        | SyntaxKind::CompoundSelector
        | SyntaxKind::ClassSelector
        | SyntaxKind::IdSelector
        | SyntaxKind::TypeSelector => Coverage::Symmetric(BASIC, Strict),
        // `parse_style_ir` is Stylesheet-entry only; wrap the A3 selector-list
        // fixture so the same construct is still demanded through the IR parse.
        SyntaxKind::SelectorList => Coverage::Symmetric(".a, .b { color: red; }", Strict),
        SyntaxKind::Combinator | SyntaxKind::NamespaceSelector | SyntaxKind::AttributeSelector => {
            Coverage::Symmetric(RICH_SELECTOR, Strict)
        }
        SyntaxKind::NestingSelector => {
            Coverage::Symmetric(".a { & .b { color: red; } }", Strict)
        }
        SyntaxKind::PseudoClass
        | SyntaxKind::PseudoElement
        | SyntaxKind::PseudoSelectorList
        | SyntaxKind::NthSelector
        | SyntaxKind::NthOfSelectorList
        | SyntaxKind::UnknownPseudoFunction => Coverage::Symmetric(PSEUDOS, Strict),
        SyntaxKind::CustomPropertyDeclaration => Coverage::Symmetric(".a { --x: 1; }", Strict),
        SyntaxKind::ComponentValueBlock | SyntaxKind::Function => {
            Coverage::Symmetric(".a { color: rgb(1,2,3); grid: { x: y }; }", Strict)
        }
        SyntaxKind::GroupAtRule | SyntaxKind::AtRulePrelude | SyntaxKind::AtRuleBlock => {
            Coverage::Symmetric("@media screen { .a { color: red; } }", Strict)
        }
        SyntaxKind::DescriptorAtRule => {
            Coverage::Symmetric("@property --x { syntax: \"*\"; }", Strict)
        }
        SyntaxKind::KeyframesAtRule => {
            Coverage::Symmetric("@keyframes k { from { opacity: 0; } }", Strict)
        }
        SyntaxKind::UnknownAtRule => Coverage::Symmetric("@future x { foo; }", Strict),
        SyntaxKind::Recovery => Coverage::Symmetric("/*", Recover),
        SyntaxKind::Interpolation => Coverage::PerDialect(
            "each preprocessor spells interpolation differently, and plain CSS has none",
            &[
                (Scss, ".a-#{$x} { color: red; }", Strict),
                (Sass, ".a-#{$x} { color: red; }", Strict),
                (Less, ".a-@{x} { color: red; }", Strict),
                (Stylus, ".a-${x} { color: red; }", Strict),
            ],
        ),
        SyntaxKind::IndentedBlock => Coverage::PerDialect(
            "plain CSS has no indentation-delimited block",
            &[
                (Scss, INDENTED, Strict),
                (Less, INDENTED, Strict),
                (Sass, INDENTED, Strict),
                (Stylus, INDENTED, Strict),
            ],
        ),
        SyntaxKind::VariableDeclaration => Coverage::PerDialect(
            "layout-parser variable assignment; the direct parser reads the same bytes as a declaration",
            &[(Sass, "$x: red;", Strict), (Stylus, "x = red;", Strict)],
        ),
        SyntaxKind::MixinOrFunctionHeader => Coverage::PerDialect(
            "mixin/function header is not a plain-CSS construct",
            &[
                (Scss, MIXIN_BRACED, Strict),
                (Sass, "@mixin m($a)\n  color: $a\n", Strict),
                (Less, "@mixin m($a)\n  color: $a\n", Strict),
                (Stylus, "m(a)\n  color: a\n", Strict),
            ],
        ),
        SyntaxKind::ControlDirective => Coverage::PerDialect(
            "control directive is not a plain-CSS construct",
            &[
                (Scss, CONTROL_BRACED, Strict),
                (Sass, CONTROL_INDENTED, Strict),
                (Less, CONTROL_INDENTED, Strict),
                (Stylus, CONTROL_INDENTED, Strict),
            ],
        ),
        SyntaxKind::AmbiguousStatement => Coverage::PerDialect(
            "`AmbiguousStatement` is the layout parser's own vocabulary",
            &[
                (Sass, "$tone junk: red;", Recover),
                (Stylus, "foo bar baz\n", Recover),
            ],
        ),
    }
}

fn parse_once(kind: SyntaxKind, source: &str, dialect: CssDialect, mode: CssParseMode) {
    let css = CssSource::new(Arc::from(source), 0)
        .unwrap_or_else(|error| panic!("{dialect:?} source {source:?}: {error:?}"));
    let before = parse_style_ir_thread_invocations();
    let result = parse_style_ir(css, dialect, mode);
    assert_eq!(
        parse_style_ir_thread_invocations(),
        before + 1,
        "{dialect:?} {source:?}: a Native construct must resolve through exactly one \
         StyleSyntaxIr parse; extra `parse_style_ir` calls are a second parser / fallback"
    );
    let ir = result.unwrap_or_else(|error| {
        panic!("{dialect:?} failed to parse Native construct {source:?}: {error:?}")
    });
    let kinds = collect_ir_kinds(&ir);
    assert!(
        kinds.contains(&kind),
        "{dialect:?} {source:?}: demanded {kind:?} missing from IR kinds {kinds:?}"
    );
}

fn collect_ir_kinds(ir: &StyleSyntaxIr) -> HashSet<SyntaxKind> {
    let mut kinds = HashSet::new();
    kinds.insert(SyntaxKind::Stylesheet);
    collect_statements(ir, ir.statements(), true, &mut kinds);
    if ir.statements().is_empty()
        || ir.diagnostics().iter().any(|diagnostic| {
            !matches!(diagnostic.recovery, verter_css_syntax::RecoveryKind::None)
                || matches!(
                    diagnostic.kind,
                    verter_css_syntax::CssDiagnosticKind::UnterminatedComment
                        | verter_css_syntax::CssDiagnosticKind::UnterminatedString
                        | verter_css_syntax::CssDiagnosticKind::UnterminatedBlock
                )
        })
    {
        kinds.insert(SyntaxKind::Recovery);
    }
    kinds
}

fn collect_statements(
    ir: &StyleSyntaxIr,
    statements: &[StyleStatement],
    top_level: bool,
    kinds: &mut HashSet<SyntaxKind>,
) {
    for statement in statements {
        match statement {
            StyleStatement::Rule(rule) => {
                kinds.insert(SyntaxKind::QualifiedRule);
                collect_selector_list(rule.selector_list(), kinds);
                collect_block(ir, rule.body(), kinds);
            }
            StyleStatement::Declaration(decl) => {
                let name = span_text(ir, decl.name_span());
                if name.starts_with("--") {
                    kinds.insert(SyntaxKind::CustomPropertyDeclaration);
                } else {
                    kinds.insert(SyntaxKind::Declaration);
                }
                if top_level && (name.starts_with('$') || ir.dialect() == CssDialect::Stylus) {
                    kinds.insert(SyntaxKind::VariableDeclaration);
                }
                collect_values(decl.value().values(), kinds);
                if let Some(body) = decl.body() {
                    collect_block(ir, body, kinds);
                }
            }
            StyleStatement::AtRule(directive) => {
                let head = span_text(ir, directive.head_span());
                let ident = head
                    .trim_start_matches('@')
                    .split(|c: char| !c.is_ascii_alphanumeric() && c != '-')
                    .next()
                    .unwrap_or("");
                match ident {
                    "media" | "supports" | "container" | "layer" => {
                        kinds.insert(SyntaxKind::GroupAtRule);
                    }
                    "property" => {
                        kinds.insert(SyntaxKind::DescriptorAtRule);
                    }
                    "keyframes" | "-webkit-keyframes" => {
                        kinds.insert(SyntaxKind::KeyframesAtRule);
                    }
                    "if" | "for" | "each" | "while" | "else" => {
                        kinds.insert(SyntaxKind::ControlDirective);
                    }
                    _ => {
                        kinds.insert(SyntaxKind::UnknownAtRule);
                    }
                }
                kinds.insert(SyntaxKind::AtRulePrelude);
                collect_values(directive.opaque_args().values(), kinds);
                if let Some(body) = directive.body() {
                    kinds.insert(SyntaxKind::AtRuleBlock);
                    collect_block(ir, body, kinds);
                }
            }
            StyleStatement::MixinOrFunction(_) => {
                kinds.insert(SyntaxKind::MixinOrFunctionHeader);
            }
            StyleStatement::Unknown(unknown) => match unknown.kind() {
                UnknownStatementKind::Ambiguous => {
                    kinds.insert(SyntaxKind::AmbiguousStatement);
                }
                UnknownStatementKind::Recovery => {
                    kinds.insert(SyntaxKind::Recovery);
                }
                UnknownStatementKind::Unknown => {
                    kinds.insert(SyntaxKind::UnknownAtRule);
                }
            },
        }
    }
}

fn collect_block(
    ir: &StyleSyntaxIr,
    block: &verter_css_syntax::StyleBlock,
    kinds: &mut HashSet<SyntaxKind>,
) {
    match block.kind() {
        StyleBlockKind::Indented => {
            kinds.insert(SyntaxKind::IndentedBlock);
        }
        StyleBlockKind::Braced => {
            kinds.insert(SyntaxKind::RuleBlock);
        }
    }
    collect_statements(ir, block.statements(), false, kinds);
}

fn collect_selector_list(list: &verter_css_syntax::SelectorList, kinds: &mut HashSet<SyntaxKind>) {
    kinds.insert(SyntaxKind::SelectorList);
    for selector in list.selectors() {
        kinds.insert(SyntaxKind::Selector);
        for part in selector.parts() {
            match part {
                ComplexSelectorPart::Compound(compound) => {
                    kinds.insert(SyntaxKind::CompoundSelector);
                    for component in compound.components() {
                        collect_component(component, kinds);
                    }
                }
                ComplexSelectorPart::Combinator(_) => {
                    kinds.insert(SyntaxKind::Combinator);
                }
            }
        }
    }
}

fn collect_component(
    component: &verter_css_syntax::SelectorComponent,
    kinds: &mut HashSet<SyntaxKind>,
) {
    match component.kind() {
        SelectorComponentKind::Type => {
            kinds.insert(SyntaxKind::TypeSelector);
        }
        SelectorComponentKind::Class | SelectorComponentKind::DynamicClass => {
            kinds.insert(SyntaxKind::ClassSelector);
        }
        SelectorComponentKind::Id => {
            kinds.insert(SyntaxKind::IdSelector);
        }
        SelectorComponentKind::Namespace => {
            kinds.insert(SyntaxKind::NamespaceSelector);
        }
        SelectorComponentKind::Attribute => {
            kinds.insert(SyntaxKind::AttributeSelector);
        }
        SelectorComponentKind::Nesting => {
            kinds.insert(SyntaxKind::NestingSelector);
        }
        SelectorComponentKind::PseudoClass => {
            kinds.insert(SyntaxKind::PseudoClass);
        }
        SelectorComponentKind::PseudoElement => {
            kinds.insert(SyntaxKind::PseudoElement);
        }
        SelectorComponentKind::FunctionalPseudo => {
            kinds.insert(SyntaxKind::PseudoClass);
            if let Some(pseudo) = component.pseudo() {
                match pseudo.kind() {
                    verter_css_syntax::PseudoFunctionKind::NthChild
                    | verter_css_syntax::PseudoFunctionKind::NthLastChild => {
                        kinds.insert(SyntaxKind::NthSelector);
                        if let Some(list) = pseudo.selector_list() {
                            kinds.insert(SyntaxKind::NthOfSelectorList);
                            collect_selector_list(list, kinds);
                        }
                    }
                    verter_css_syntax::PseudoFunctionKind::Unknown => {
                        kinds.insert(SyntaxKind::UnknownPseudoFunction);
                    }
                    _ => {
                        if let Some(list) = pseudo.selector_list() {
                            kinds.insert(SyntaxKind::PseudoSelectorList);
                            collect_selector_list(list, kinds);
                        }
                    }
                }
            }
        }
        SelectorComponentKind::Interpolation => {
            kinds.insert(SyntaxKind::Interpolation);
        }
    }
    if !component.interpolations().is_empty() {
        kinds.insert(SyntaxKind::Interpolation);
    }
    for nested in component.nested_components() {
        collect_component(nested, kinds);
    }
}

fn collect_values(values: &[ComponentValue], kinds: &mut HashSet<SyntaxKind>) {
    kinds.insert(SyntaxKind::ComponentValueList);
    for value in values {
        match value {
            ComponentValue::Function(function) => {
                kinds.insert(SyntaxKind::Function);
                collect_values(function.values(), kinds);
            }
            ComponentValue::Block(block) => {
                kinds.insert(SyntaxKind::ComponentValueBlock);
                collect_values(block.values(), kinds);
            }
            ComponentValue::Interpolation(interpolation) => {
                kinds.insert(SyntaxKind::Interpolation);
                collect_values(interpolation.values(), kinds);
            }
            ComponentValue::Token(_) | ComponentValue::String(_) | ComponentValue::Comment(_) => {}
        }
    }
}

fn span_text(ir: &StyleSyntaxIr, span: verter_span::Span) -> &str {
    let text = ir.source().text();
    let start = span.start as usize;
    let end = (span.end as usize).min(text.len());
    text.get(start..end).unwrap_or("")
}

#[test]
fn no_fallback_parser_per_construct() {
    let mut every_kind = Vec::new();
    for raw in 0u16.. {
        let kind = SyntaxKind::from_raw(raw);
        if kind as u16 != raw {
            break;
        }
        every_kind.push(kind);
    }
    assert!(
        every_kind.len() > 30,
        "the discriminant walk must have found the real variant set, got {}",
        every_kind.len()
    );

    for kind in every_kind {
        match coverage(kind) {
            Coverage::Symmetric(source, mode) => {
                for dialect in DIALECTS {
                    parse_once(kind, source, dialect, mode);
                }
            }
            Coverage::PerDialect(reason, cases) => {
                assert!(!reason.is_empty() && !cases.is_empty(), "{kind:?}");
                for (dialect, source, mode) in cases.iter().copied() {
                    parse_once(kind, source, dialect, mode);
                }
            }
        }
    }
}
