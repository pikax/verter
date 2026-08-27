//! Sibling of the SelectorCompound UAF lock: clone a bump-backed StyleRule
//! out of the IR, drop the IR, then read the rule.

use std::sync::Arc;
use verter_css_syntax::{parse_style_ir, CssDialect, CssParseMode, CssSource, StyleStatement, StyleSyntaxIr};

fn first_rule(css: &str) -> (StyleSyntaxIr, verter_css_syntax::StyleRule) {
    let source = CssSource::new(Arc::from(css), 0).unwrap();
    let ir = parse_style_ir(source, CssDialect::Css, CssParseMode::Recover).unwrap();
    let StyleStatement::Rule(rule) = &ir.statements()[0] else {
        panic!("expected a rule");
    };
    (ir, rule.clone())
}

fn main() {
    let (_ir, rule) = first_rule(".card { color: red; }");
    let _ = rule.selector_list();
}
