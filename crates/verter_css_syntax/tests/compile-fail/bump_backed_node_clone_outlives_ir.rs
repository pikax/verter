//! Old UAF helper shape: clone a bump-backed node out of the IR, drop the IR,
//! then read the node. Must not compile.

use std::sync::Arc;
use verter_css_syntax::{
    parse_style_ir, CssDialect, CssParseMode, CssSource, SelectorCompound, StyleStatement,
    StyleSyntaxIr,
};

fn first_rule_compound(css: &str) -> (StyleSyntaxIr, SelectorCompound) {
    let source = CssSource::new(Arc::from(css), 0).unwrap();
    let ir = parse_style_ir(source, CssDialect::Css, CssParseMode::Recover).unwrap();
    let StyleStatement::Rule(rule) = &ir.statements()[0] else {
        panic!("expected a rule");
    };
    let compound = rule.selector_list().selectors()[0].compounds()[0].clone();
    (ir, compound)
}

fn main() {
    let (_ir, compound) = first_rule_compound("p:nth-child(2n+1) { color: red; }");
    let _ = compound.components();
}
