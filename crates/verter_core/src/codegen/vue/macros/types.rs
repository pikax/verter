use crate::common::Span;

pub struct MacroProcessReturn {
    pub move_span: Option<Span>,
    pub overwrite_span: Option<(Span, String)>,
    pub remove: Option<Span>,
}
