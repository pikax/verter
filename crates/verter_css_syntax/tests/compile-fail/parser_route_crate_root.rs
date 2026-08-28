use std::sync::Arc;

use verter_css_syntax::{
    CssDialect, CssEntryPoint, CssParseMode, CssSource, CssStructureTooLarge, ParseEvent,
    ParseEventSink, Parser,
};

struct Sink;

impl ParseEventSink for Sink {
    fn event(&mut self, _event: ParseEvent) -> Result<(), CssStructureTooLarge> {
        Ok(())
    }
}

fn main() {
    let source = CssSource::new(Arc::from("a {}"), 0).unwrap();
    let mut sink = Sink;
    let _ = Parser::new(
        &source,
        CssDialect::Css,
        CssEntryPoint::Stylesheet,
        CssParseMode::Strict,
    )
    .parse(&mut sink);
}
