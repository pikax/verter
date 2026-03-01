//! Static analysis utilities for Vue Single File Components.
//!
//! Provides import/export extraction, Vue API classification, script and style
//! analysis, and a project-wide component graph. Used by `verter_host` for
//! smart invalidation and LSP features.
//!
//! Depends on `verter_core` (indirectly via OXC) for AST parsing.
//!
//! # Key exports
//!
//! - [`build_script_analysis`] — Parses a script block and produces a
//!   [`ScriptAnalysisSnapshot`] with imports, bindings, macros, and flags.
//! - [`build_export_signatures`] — Extracts per-export hashes for
//!   change-detection in dependency files.
//! - [`classify_vue_api`] — Maps a Vue import name to a
//!   [`VueApiClassification`] variant.
//! - [`ProjectIndex`] — Aggregates file-level usage into cross-file indexes
//!   (provide/inject validation, component graph, CSS class tracking).
//! - [`build_css_style_analysis`] / [`build_preprocessor_style_analysis`] —
//!   Analyse CSS style blocks for selectors, specificity, and Vue-specific
//!   features (`:deep`, `:global`, `v-bind()`).

mod analysis;
mod classify;
mod exports;
pub mod file_usage;
mod imports;
mod macros;
pub mod project_index;
pub mod scope;
pub mod selector_match;
pub mod style;
pub mod template;
pub mod types;

pub use analysis::{
    build_export_signatures, build_script_analysis, build_script_analysis_with_scope,
};
pub use classify::{classify_vue_api, is_lifecycle_api, is_reactivity_api, is_watcher_api};
pub use exports::extract_export_signatures;
pub use file_usage::{
    ComponentUsageOwned, FileUsageFlags, FileUsageInfoOwned, ImportInfoOwned, InjectUsageOwned,
    MacroInfoOwned, ProvideUsageOwned, StyleUsageInfoOwned,
};
pub use imports::extract_import_sources;
pub use macros::collect_type_references;
pub use project_index::{
    ComponentEdge, ComponentUsageSummary, DynamicInjectEntry, FileInjectValidation,
    InjectValidation, InjectValidationEntry, ProjectIndex, ProjectStats, ProvideInjectSummary,
};
pub use scope::AnalysisScope;
pub use selector_match::{match_selector, MatchResult};
pub use style::{
    build_css_style_analysis, build_preprocessor_style_analysis, compute_structured_specificity,
    parse_selector, AnalyzedSelector, AttributeOperator, AttributeSelector, CompoundSelector,
    SelectorCombinator, SelectorPseudoClass, SpecialPseudoInput, SpecialPseudoKind,
    StructuredSelector, StyleAnalysisFlags, StyleAnalysisLang, StyleBlockAnalysis, VBindInput,
    VueStyleInput,
};
pub use template::{
    extract_dynamic_class_names, extract_dynamic_class_names_rich, AnalyzedEmitDefinition,
    AnalyzedMacroUsage, AnalyzedPropDefinition, BindingUsageKind, CommentDirective,
    CommentDirectiveKind, DefinedSlot, DynamicClassName, ElementNamespace, IfChain, MacroKind,
    PropValueConstness, TemplateAnalysisSnapshot, TemplateAttribute, TemplateBindingOccurrence,
    TemplateComponentUsage, TemplateDirective, TemplateElement, TemplateEventHandler,
    TemplatePropUsage, TemplateRef, TemplateTypeEnhancements, TypeMismatch, UnresolvedBinding,
    VForDirective, VModelDirective,
};
pub use types::hash_16;
pub use types::{
    AnalysisFlags, AnalyzedBinding, AnalyzedBindingKind, AnalyzedExportedFunction, AnalyzedImport,
    AnalyzedImportBinding, AnalyzedMacro, AnalyzedMacroKind, BindingInitializer, ComposableInfo,
    ComposableReturn, ComposableReturnField, DomQueryCallSite, DomQueryKind, ExportSignature,
    FunctionParam, Hash16, ImportSourceInfo, LiteralKind, MacroTypeDep, ReactivityKind,
    ResolvedTypeInfo, ReturnReactivity, ScriptAnalysisSnapshot, ScriptTypeEnhancements,
    VueApiClassification,
};
