//! Wrapped formal parameters enter the same file-relative coordinate domain.
use oxc_ast::ast::FormalParameters;
use oxc_ast_visit::VisitMut;

/// Strip the synthetic arrow-wrapper prefix and add the file offset in one walk.
/// Defaults and nested callbacks receive the same shift as binding sites.
/// Annotation nodes keep the published wrapper-prefix convention.
pub fn shift_formal_parameters_spans(
    params: &mut FormalParameters<'_>,
    wrapper_offset: u32,
    file_offset: u32,
) {
    if wrapper_offset != 0 || file_offset != 0 {
        super::span::SpanShift {
            wrapper: wrapper_offset,
            file: file_offset,
        }
        .visit_formal_parameters(params);
    }
}
