//! Target-aware type expansion service.
//!
//! Replaces the old "evaluate everything, then project" model with
//! query-driven expansion that only does work relevant to the caller's
//! target (`ObjectShape` or `NormalizedExpr`).
//!
//! # Architecture
//!
//! - `request` — Request/result/budget/completeness types
//! - `object_shape` — `expand_object_shape()` implementation
//! - `normalized` — `expand_normalized_expr()` implementation

mod normalized;
mod object_shape;
mod request;

pub use normalized::expand_normalized_expr;
pub use object_shape::expand_object_shape;
pub use request::{
    ExpandedCallSignature, ExpandedComponentTypes, ExpandedField, ExpandedIndexSignature,
    ExpandedMacroObjectShape, ExpandedMacroProps, ExpandedNormalizedExpr, ExpandedObjectShape,
    ExpandedParameter, ExpandedProperty, ExpansionBudget, ExpansionCompleteness,
    ExpansionDiagnostic, ExpansionMetadata, ExpansionResult, ExpansionStopReason,
};

#[cfg(test)]
mod tests;
