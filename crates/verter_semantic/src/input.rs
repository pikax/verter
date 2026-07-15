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
pub use crate::analysis::types::AnalyzedMacro;
pub use crate::analysis::types::AnalyzedMacroKind;

// Binding types
pub use crate::analysis::types::AnalyzedBinding;
pub use crate::analysis::types::AnalyzedBindingKind;
pub use crate::analysis::types::ReactivityKind;

// Import types
pub use crate::analysis::types::AnalyzedImport;
pub use crate::analysis::types::AnalyzedImportBinding;
pub use crate::analysis::types::ImportBindingKind;

// Binding initializer
pub use crate::analysis::types::BindingInitializer;

// Vue API classification
pub use crate::analysis::VueApiClassification;

// Analysis scope
pub use crate::analysis::AnalysisScope;

// Template component usage
pub use crate::analysis::TemplateComponentUsage;
pub use crate::analysis::TemplatePropUsage;

// Prop constness
pub use crate::analysis::template::PropValueConstness;

// Prop/emit/slot field types
pub use crate::analysis::types::AnalyzedDefaultValue;
pub use crate::analysis::types::AnalyzedEmitField;
pub use crate::analysis::types::AnalyzedExposeField;
pub use crate::analysis::types::AnalyzedPropField;
pub use crate::analysis::types::AnalyzedSlotField;
pub use crate::analysis::types::AnalyzedSlotFieldBinding;
pub use crate::analysis::types::TypeResolutionSource;

// Template component v-model
pub use crate::analysis::template::TemplateComponentVModel;
