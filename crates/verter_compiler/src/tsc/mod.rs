//! TSC codegen — generates minimal TypeScript declaration files for Vue SFCs.
//!
//! This module is the entry point for tsc-mode compilation: it generates `.tsc.tsx`
//! files that TypeScript can use for type checking (replacing vue-tsc).
//!
//! Unlike the full compile pipeline, tsc codegen performs **macro extraction only**:
//! it OXC-parses `<script setup>` to extract `defineProps`, `defineEmits`,
//! `defineModel`, and `defineOptions`, then emits a minimal TypeScript declaration.

pub mod script;

#[cfg(test)]
mod tests;

pub use script::{
    extract_tsc_state, generate_tsc_from_state, generate_tsc_output,
    generate_tsc_output_with_options, ExtractedTscState, MacroTscInput, TscDeclarationShapeReason,
    TscExtractOptions, TscFailureSubject, TscGenOptions, TscGenerationError,
    TscInvalidAuthoredTypeReason, TscInvalidOutcome, TscMode, TscOutput, TscUnavailableOutcome,
};
