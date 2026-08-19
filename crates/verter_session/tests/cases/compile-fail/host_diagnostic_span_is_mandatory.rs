// `HostDiagnostic.span` is a mandatory `verter_span::Span`, not
// `Option<verter_span::Span>` — a producer that cannot derive a real mapped
// location has no way to construct this type at all; it must fail closed at
// its own seam instead. This fixture omits the field and must fail to
// compile with a missing-field error, proving the constraint is structural
// (a compile error), not a convention a producer could opt out of.
fn main() {
    let _ = verter_session::HostDiagnostic {
        severity: verter_session::HostSeverity::Error,
        code: "X".to_string(),
        message: "x".to_string(),
        arguments: Vec::new(),
    };
}
