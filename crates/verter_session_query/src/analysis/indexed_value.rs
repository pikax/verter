//! The root of an indexed value read.

/// Exact source authority for an indexed value's whole binding input.
/// Spans use the input AST's coordinate system; composite values have no root.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum IndexedValueReadRoot {
    /// No whole identifier read supplied this result. This does not certify
    /// that names inside a composite or asserted type are free/module names.
    NonBinding,
    Identifier(verter_span::Span),
    /// The result is an authored whole `typeof name` type query. This is
    /// lexical type authority, never an operand read or freshness signal.
    SourceTypeQuery(verter_span::Span),
}
