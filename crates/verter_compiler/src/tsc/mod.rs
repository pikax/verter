//! TSC codegen — generates minimal TypeScript declaration files for Vue SFCs.
//!
//! This module is the entry point for tsc-mode compilation: it generates `.tsc.tsx`
//! files that TypeScript can use for type checking (replacing vue-tsc).
//!
//! Unlike the full compile pipeline, tsc codegen performs **macro extraction only**:
//! it OXC-parses `<script setup>` to extract `defineProps`, `defineEmits`,
//! `defineModel`, and `defineOptions`, then emits a minimal TypeScript declaration.

pub mod module_specifiers;
pub mod script;

#[cfg(test)]
mod tests;

/// The four-way authored/carried script dialect every generated companion is
/// labelled with. Re-exported here so a consumer of [`TscOutput`] reaches the
/// classification through the same module.
pub use crate::parser::types::SfcScriptDialect;
pub use module_specifiers::{
    collect_module_specifier_spans, quote_module_specifier, ModuleSpecifierSpan,
};
pub use script::{
    extract_tsc_state, generate_tsc_from_state, generate_tsc_output,
    generate_tsc_output_with_options, ExtractedTscState, FallthroughArm,
    FallthroughPropsProjection, InheritedComponentProps, MacroTscInput, TscDeclarationShapeReason,
    TscExtractOptions, TscFailureSubject, TscGenOptions, TscGenerationError,
    TscInvalidAuthoredTypeReason, TscInvalidOutcome, TscMode, TscOutput, TscUnavailableOutcome,
};
