//! Every special-pseudo name in the shared authority parses with a typed
//! selector list in every dialect. `:foo(...)` is the in-run control that
//! must violate the same predicate.

use std::sync::Arc;

use verter_css_syntax::{
    css_identifier_eq_ignore_ascii_case, parse_style_ir, ComplexSelector, ComplexSelectorPart,
    CssDialect, CssParseMode, CssSource, SelectorComponentKind, SpecialSelectorListPseudo,
    StyleStatement, StyleSyntaxIr,
};

fn has_typed_list_for(ir: &StyleSyntaxIr, name: &str) -> bool {
    fn walk(selectors: &[ComplexSelector], ir: &StyleSyntaxIr, name: &str) -> bool {
        for selector in selectors {
            for part in selector.parts() {
                let ComplexSelectorPart::Compound(compound) = part else {
                    continue;
                };
                for component in compound.components() {
                    if matches!(
                        component.kind(),
                        SelectorComponentKind::PseudoClass
                            | SelectorComponentKind::FunctionalPseudo
                    ) {
                        if let Some(name_span) = component.name_span() {
                            let ident = ir.source().slice(name_span).trim_start_matches(':');
                            if css_identifier_eq_ignore_ascii_case(ident, name) {
                                return component
                                    .pseudo()
                                    .and_then(|p| p.selector_list())
                                    .is_some();
                            }
                        }
                    }
                    if let Some(list) = component.pseudo().and_then(|p| p.selector_list()) {
                        if walk(list.selectors(), ir, name) {
                            return true;
                        }
                    }
                }
            }
        }
        false
    }
    for statement in ir.statements() {
        if let StyleStatement::Rule(rule) = statement {
            if walk(rule.selector_list().selectors(), ir, name) {
                return true;
            }
        }
    }
    false
}

fn parse(css: &str, dialect: CssDialect) -> StyleSyntaxIr {
    let source = CssSource::new(Arc::from(css), 0).unwrap();
    parse_style_ir(source, dialect, CssParseMode::Recover).expect("stylesheet parses")
}

#[test]
fn every_special_pseudo_name_parses_with_typed_selector_list_in_all_dialects() {
    for dialect in CssDialect::ALL {
        for kind in SpecialSelectorListPseudo::ALL {
            let css = format!(".a:{}(.b) {{ color: red; }}", kind.ident());
            let ir = parse(&css, dialect);
            assert!(
                has_typed_list_for(&ir, kind.ident()),
                "{dialect:?} :{}(.b) must retain a typed selector list",
                kind.ident()
            );
        }
        let control = parse(".a:foo(.a .b) { color: red; }", dialect);
        assert!(
            !has_typed_list_for(&control, "foo"),
            "{dialect:?} :foo(.a .b) must NOT retain a typed selector list"
        );
    }
}
