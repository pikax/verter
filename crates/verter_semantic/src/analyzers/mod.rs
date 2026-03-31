//! Semantic analyzers — derived reports from the fact model.
//!
//! Each analyzer consumes semantic facts and produces reports suitable for
//! diagnostics, MCP explanations, or code actions. Analyzers do not own
//! semantic logic — they compose facts from the semantic DB.

pub mod boundary;
pub mod class_flow;
pub mod corender;
pub mod css_bleed;
pub mod reactive_flow;
pub mod route;
pub mod ssr;
