//! Action provider trait and context.

use verter_analysis::template::TemplateAnalysisSnapshot;
use verter_analysis::types::ScriptAnalysisSnapshot;
use verter_analysis::StyleBlockAnalysis;
use verter_diagnostics::{DiagnosticSet, LintDiagnostic};

use crate::types::CodeAction;

/// Context for action providers, containing analysis data and diagnostics.
pub struct ActionContext<'a> {
    /// Full SFC source text.
    pub source: &'a str,
    /// File identifier (canonical path).
    pub file_id: &'a str,
    /// Diagnostics produced by the diagnostic engine.
    pub diagnostics: &'a DiagnosticSet,
    /// Template analysis (if available).
    pub template: Option<&'a TemplateAnalysisSnapshot>,
    /// Script analysis (if available).
    pub script: Option<&'a ScriptAnalysisSnapshot>,
    /// Style block analyses.
    pub styles: &'a [StyleBlockAnalysis],
}

/// Trait for action providers. Each provider handles one or more diagnostic rules
/// or refactoring patterns.
pub trait ActionProvider: Send + Sync {
    /// Provider name (for debugging).
    fn name(&self) -> &str;

    /// Quick fixes for a specific diagnostic.
    ///
    /// Called when the user requests code actions at a diagnostic's span.
    fn fixes_for_diagnostic(
        &self,
        _diag: &LintDiagnostic,
        _ctx: &ActionContext,
    ) -> Vec<CodeAction> {
        vec![]
    }

    /// Actions available at a byte offset (refactoring, source actions).
    ///
    /// Called when the user requests code actions at a cursor position.
    fn actions_at(&self, _offset: u32, _ctx: &ActionContext) -> Vec<CodeAction> {
        vec![]
    }
}
