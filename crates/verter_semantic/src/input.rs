//! Convenience re-exports of commonly-used analysis types.
//!
//! Provides a flat import surface for the most-used types from
//! `verter_semantic::analysis`, avoiding deep import paths in
//! extraction and test code.

// Script analysis input
pub use verter_session_query::analysis::script_snapshot::ScriptAnalysisSnapshot;

// Template analysis input
pub use verter_session_query::analysis::template::TemplateAnalysisSnapshot;

// Macro types

// Binding types

// Import types

// Binding initializer

// Vue API classification

// Analysis scope
pub use crate::analysis::AnalysisScope;

// Template component usage
pub use verter_session_query::analysis::template::TemplateComponentUsage;
pub use verter_session_query::analysis::template::TemplatePropUsage;

// Prop constness

// Prop/emit/slot field types

// Template component v-model
