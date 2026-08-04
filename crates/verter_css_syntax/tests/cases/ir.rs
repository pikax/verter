use std::sync::Arc;

use verter_css_syntax::{
    parse_with_sink, ComponentValue, CssDialect, CssEntryPoint, CssParseMode, CssSource,
    CssStructureTooLarge, CssSyntaxGrammarVersion, LosslessCstSink, ParseEvent, ParseEventSink,
    SelectorComponentKind, StyleStatement, StyleSyntaxIrSink,
};

// @ai-generated - Proves StyleSyntaxIr is projected directly from the same event stream as the CST.
#[test]
fn style_ir_and_lossless_cst_are_peer_event_sinks() {
    fn accepts_sink(_: &mut impl ParseEventSink) {}

    struct Peers<'a> {
        cst: &'a mut LosslessCstSink,
        ir: &'a mut StyleSyntaxIrSink,
    }

    impl ParseEventSink for Peers<'_> {
        fn event(&mut self, event: ParseEvent) -> Result<(), CssStructureTooLarge> {
            self.cst.event(event)?;
            self.ir.event(event)
        }
    }

    let input =
        ".card, #hero { color: calc(1px + var(--x)); content: \"x\"; } @import \"theme.css\";";
    let source = CssSource::new(Arc::from(input), 17).unwrap();
    let mut ir_sink = StyleSyntaxIrSink::new(source.clone(), CssDialect::Css);
    accepts_sink(&mut ir_sink);
    let mut cst_sink = LosslessCstSink::new(source.clone());
    let mut peers = Peers {
        cst: &mut cst_sink,
        ir: &mut ir_sink,
    };
    parse_with_sink(
        &source,
        CssDialect::Css,
        CssEntryPoint::Stylesheet,
        CssParseMode::Recover,
        &mut peers,
    )
    .unwrap();
    let ir = ir_sink.finish().unwrap();
    let cst = cst_sink.finish().unwrap();

    assert_eq!(cst.reconstruct(), input);
    assert_eq!(ir.source().text(), input);
    assert_eq!(ir.grammar_version(), CssSyntaxGrammarVersion::CURRENT);
    assert_eq!(ir.statements().len(), 2);
    assert!(ir.imports_unresolved());

    let StyleStatement::Rule(rule) = &ir.statements()[0] else {
        panic!("first statement must be a rule");
    };
    let components: Vec<_> = rule
        .selector_list()
        .selectors()
        .iter()
        .flat_map(|selector| selector.compounds())
        .flat_map(|compound| compound.components())
        .collect();
    assert!(components
        .iter()
        .any(|component| component.kind() == SelectorComponentKind::Class));
    assert!(components
        .iter()
        .any(|component| component.kind() == SelectorComponentKind::Id));

    let StyleStatement::Declaration(color) = &rule.body().statements()[0] else {
        panic!("rule body must contain a declaration");
    };
    assert_eq!(source.slice(color.name_span()), "color");
    assert!(color
        .value()
        .values()
        .iter()
        .any(|value| matches!(value, ComponentValue::Function(function) if source.slice(function.name_span()) == "calc")));
}
