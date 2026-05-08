#![deny(missing_docs)]
//! Public host typeinfo substrate.
//!
//! Three audited / non-audited host methods that expose the
//! shallow-state symbol inventory and the dispatch-backed type
//! evaluation surface to downstream consumers (the `@verter/typeinfo`
//! TS package, MCP tools, IDE integrations).
//!
//! Public methods (per §5 of the typeinfo plan):
//!
//! 1. [`crate::VerterHost::list_file_symbols`] — shallow read; no
//!    audit.
//! 2. [`crate::VerterHost::resolve_named_symbol`] /
//!    [`crate::VerterHost::resolve_named_symbol_with_audit`] —
//!    audited resolution of a named declaration with optional
//!    generic instantiation.
//! 3. [`crate::VerterHost::evaluate_type_expression_with_audit`] —
//!    audited evaluation of a synthetic type expression in a
//!    file scope, with an optional host-owned scratch cache.
//!
//! Sub-modules:
//! - [`types`] — public DTOs (`SymbolEntry`, `EvaluateTypeExpressionRequest`, …).
//! - [`symbol_inventory`] — `list_file_symbols` impl.
//! - [`resolve_named_symbol`] — `resolve_named_symbol_with_audit` impl.
//! - [`evaluate_type_expression`] — `evaluate_type_expression_with_audit` impl.
//! - [`scratch_cache`] — host-owned LRU for typeinfo scratch URIs.

pub mod evaluate_type_expression;
pub mod raise;
pub mod resolve_named_symbol;
pub(crate) mod scratch_cache;
pub mod symbol_inventory;
pub mod types;

pub use resolve_named_symbol::ResolveMode;
pub use types::{
    EvaluateTypeExpressionRequest, ImportSpec, NamedImport, SymbolEntry, SymbolKind, TypeArgList,
};

#[cfg(test)]
mod tests;
