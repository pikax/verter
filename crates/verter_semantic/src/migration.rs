//! Migration notes for WS9: verter_semantic::analysis → verter_semantic.
//!
//! This module documents which `verter_semantic::analysis` types are consumed by
//! `verter_semantic` through the extraction layer, and where they will
//! land when `verter_semantic::analysis` is deleted in WS9.
//!
//! ## Types consumed by extraction (extract.rs)
//!
//! | verter_semantic::analysis type           | Destination          | Status     |
//! |-------------------------------|----------------------|------------|
//! | `ScriptAnalysisSnapshot`       | verter_semantic input | Consumed via extract |
//! | `AnalyzedMacro`                | verter_semantic input | Consumed via extract |
//! | `AnalyzedMacroKind`            | verter_semantic input | Consumed via extract |
//! | `AnalyzedPropField`            | verter_semantic input | Consumed via extract |
//! | `AnalyzedEmitField`            | verter_semantic input | Consumed via extract |
//! | `AnalyzedSlotField`            | verter_semantic input | Consumed via extract |
//! | `AnalyzedBinding`              | verter_semantic input | Consumed via extract |
//! | `AnalyzedBindingKind`          | verter_semantic input | Consumed via extract |
//! | `ReactivityKind`               | verter_semantic input | Consumed via extract |
//! | `TemplateAnalysisSnapshot`     | verter_semantic input | Consumed via extract |
//! | `TemplateComponentUsage`       | verter_semantic input | Consumed via extract |
//! | `TemplatePropUsage`            | verter_semantic input | Consumed via extract |
//! | `PropValueConstness`           | verter_semantic input | Consumed via extract |
//! | `AnalyzedImport`               | verter_semantic input | Consumed via extract |
//! | `AnalyzedImportBinding`        | verter_semantic input | Consumed via extract |
//! | `ImportBindingKind`            | verter_semantic input | Consumed via extract |
//! | `VueApiClassification`         | verter_semantic input | Consumed via extract |
//! | `AnalysisScope`                | verter_semantic profile | Bridged via QueryProfile |
//!
//! ## WS9 plan
//!
//! When `verter_semantic::analysis` is deleted:
//! 1. Script/template analysis types become inputs owned by verter_parser or
//!    verter_compiler (whichever owns the analysis pass)
//! 2. The extract layer continues to bridge those inputs → semantic facts
//! 3. No semantic logic changes — only import paths update
