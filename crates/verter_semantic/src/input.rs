//! Input type re-exports from `verter_analysis`.
//!
//! These re-exports allow consumers to import analysis input types from
//! `verter_semantic::input` instead of `verter_analysis` directly.
//! When `verter_analysis` is deleted in WS9, these types will be moved
//! here natively.

// Script analysis input
pub use verter_analysis::ScriptAnalysisSnapshot;

// Template analysis input
pub use verter_analysis::TemplateAnalysisSnapshot;

// Macro types
pub use verter_analysis::types::AnalyzedMacro;
pub use verter_analysis::types::AnalyzedMacroKind;

// Binding types
pub use verter_analysis::types::AnalyzedBinding;
pub use verter_analysis::types::AnalyzedBindingKind;
pub use verter_analysis::types::ReactivityKind;

// Import types
pub use verter_analysis::types::AnalyzedImport;
pub use verter_analysis::types::AnalyzedImportBinding;
pub use verter_analysis::types::ImportBindingKind;

// Binding initializer
pub use verter_analysis::types::BindingInitializer;

// Vue API classification
pub use verter_analysis::VueApiClassification;

// Analysis scope (transitional — will be replaced by QueryProfile)
pub use verter_analysis::AnalysisScope;

// Template component usage
pub use verter_analysis::TemplateComponentUsage;
pub use verter_analysis::TemplatePropUsage;

// Prop constness
pub use verter_analysis::template::PropValueConstness;

// Prop/emit/slot field types (used in tests)
pub use verter_analysis::types::AnalyzedDefaultValue;
pub use verter_analysis::types::AnalyzedEmitField;
pub use verter_analysis::types::AnalyzedExposeField;
pub use verter_analysis::types::AnalyzedPropField;
pub use verter_analysis::types::AnalyzedSlotField;
pub use verter_analysis::types::AnalyzedSlotFieldBinding;
pub use verter_analysis::types::TypeResolutionSource;

// Template component usage
pub use verter_analysis::template::TemplateComponentVModel;
