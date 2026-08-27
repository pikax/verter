//! J1-A10c: no Native-supported construct in any of the 5 dialects depends on
//! a second parser or fallback. Every construct in the closed `SyntaxKind`
//! universe is resolved by exactly one [`parse_style_ir`] call — the sole
//! `StyleSyntaxIr` parse entry. Layout-parser subparses of Sass/Stylus are
//! the same grammar (`parse_with_sink`), not a second `parse_style_ir`.
//!
//! The construct universe is the same closed `SyntaxKind` set A3 enumerates.
//! A new variant is a compile error (`E0004`) until dispositioned here.

use std::sync::Arc;

use verter_css_syntax::{
    parse_style_ir, parse_style_ir_thread_invocations, CssDialect, CssParseMode, CssSource,
    SyntaxKind,
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

fn parse_once(source: &str, dialect: CssDialect, mode: CssParseMode) {
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
    result.unwrap_or_else(|error| {
        panic!("{dialect:?} failed to parse Native construct {source:?}: {error:?}")
    });
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
                    parse_once(source, dialect, mode);
                }
            }
            Coverage::PerDialect(reason, cases) => {
                assert!(!reason.is_empty() && !cases.is_empty(), "{kind:?}");
                for (dialect, source, mode) in cases.iter().copied() {
                    parse_once(source, dialect, mode);
                }
            }
        }
    }
}
