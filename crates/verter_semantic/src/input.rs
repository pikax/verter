//! Convenience re-exports of commonly-used analysis types.
//!
//! Provides a flat import surface for the most-used types from
//! `verter_semantic::analysis`, avoiding deep import paths in
//! extraction and test code.

// Script analysis input
pub use crate::analysis::ScriptAnalysisSnapshot;

// Template analysis input
pub use crate::analysis::TemplateAnalysisSnapshot;

// Macro types

// Binding types

// Import types

// Binding initializer

// Vue API classification

// Analysis scope
pub use crate::analysis::AnalysisScope;

// Template component usage
pub use crate::analysis::TemplateComponentUsage;
pub use crate::analysis::TemplatePropUsage;

// Prop constness
pub use crate::analysis::template::PropValueConstness;

// Prop/emit/slot field types

// Template component v-model
pub use crate::analysis::template::TemplateComponentVModel;
