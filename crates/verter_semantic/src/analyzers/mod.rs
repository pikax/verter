//! Semantic analyzers — derived reports from the fact model.
//!
//! Each analyzer consumes semantic facts and produces reports suitable for
//! diagnostics, MCP explanations, or code actions. Analyzers do not own
//! semantic logic — they compose facts from the semantic DB.

pub mod boundary;
