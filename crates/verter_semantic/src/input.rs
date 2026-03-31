//! Input type re-exports from the analysis module.
//!
//! These re-exports allow consumers to import analysis input types from
//! `verter_semantic::input` instead of the analysis module directly.
//! The analysis module is now owned by verter_semantic, these types will be moved
//! here natively.

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

// Analysis scope (transitional — will be replaced by QueryProfile)
pub use crate::analysis::AnalysisScope;

// Template component usage
pub use crate::analysis::TemplateComponentUsage;
pub use crate::analysis::TemplatePropUsage;

// Prop constness
pub use crate::analysis::template::PropValueConstness;

// Prop/emit/slot field types (used in tests)
pub use crate::analysis::types::AnalyzedDefaultValue;
pub use crate::analysis::types::AnalyzedEmitField;
pub use crate::analysis::types::AnalyzedExposeField;
pub use crate::analysis::types::AnalyzedPropField;
pub use crate::analysis::types::AnalyzedSlotField;
pub use crate::analysis::types::AnalyzedSlotFieldBinding;
pub use crate::analysis::types::TypeResolutionSource;

// Template component usage
pub use crate::analysis::template::TemplateComponentVModel;
