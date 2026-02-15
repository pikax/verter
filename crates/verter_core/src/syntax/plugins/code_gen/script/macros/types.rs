use crate::common::Span;

/// A diagnostic message from macro processing.
pub struct MacroDiagnostic {
    pub message: String,
    pub span: Span,
}

pub struct MacroProcessReturn {
    pub move_span: Option<Span>,
    pub overwrite_span: Option<(Span, String)>,
    pub remove: Option<Span>,
    /// Optional diagnostic (e.g., "Unresolvable type reference").
    pub diagnostic: Option<MacroDiagnostic>,
}
