//! Sibling of the SelectorCompound UAF lock: clone a bump-backed SelectorList
//! out of the IR, drop the IR, then read the list.

use std::sync::Arc;
use verter_css_syntax::{
    parse_style_ir, CssDialect, CssParseMode, CssSource, SelectorList, StyleStatement, StyleSyntaxIr,
};

fn first_list(css: &str) -> (StyleSyntaxIr, SelectorList) {
    let source = CssSource::new(Arc::from(css), 0).unwrap();
    let ir = parse_style_ir(source, CssDialect::Css, CssParseMode::Recover).unwrap();
    let StyleStatement::Rule(rule) = &ir.statements()[0] else {
        panic!("expected a rule");
    };
    (ir, rule.selector_list().clone())
}

fn main() {
    let (_ir, list) = first_list(".card { color: red; }");
    let _ = list.selectors();
}
