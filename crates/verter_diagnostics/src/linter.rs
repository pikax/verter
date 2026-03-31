//! Linter engine: runs rules against analysis data.

use crate::comment_directives::parse_comment_directives;
use crate::config::LintConfig;
use crate::context::LintContext;
use crate::cross_file::CrossFileSnapshot;
use crate::diagnostic_set::DiagnosticSet;
use crate::rules::{FileContext, RuleRegistry};
use crate::visitor::LintVisitor;
use verter_semantic::analysis::template::TemplateAnalysisSnapshot;
use verter_semantic::analysis::types::ScriptAnalysisSnapshot;
use verter_semantic::analysis::StyleBlockAnalysis;

/// Main linter engine. Holds the rule registry and configuration.
pub struct Linter {
    registry: RuleRegistry,
    config: LintConfig,
}

impl Linter {
    /// Create a new linter with the given configuration.
    pub fn new(config: LintConfig) -> Self {
        Self {
            registry: RuleRegistry::builtin(),
            config,
        }
    }

    /// Create a linter with a custom rule registry.
    pub fn with_registry(config: LintConfig, registry: RuleRegistry) -> Self {
        Self { registry, config }
    }

    /// Lint a file given its analysis data.
    ///
    /// Accepts optional script, template, and style analysis snapshots,
    /// plus an optional source string for rules that need byte-level access.
    /// Returns a [`DiagnosticSet`] that can be enriched before consumption.
    pub fn lint(
        &self,
        script: Option<&ScriptAnalysisSnapshot>,
        template: Option<&TemplateAnalysisSnapshot>,
        styles: &[StyleBlockAnalysis],
    ) -> DiagnosticSet {
        self.lint_inner(script, template, styles, None, None)
    }

    /// Lint a file with the full SFC source available.
    ///
    /// Same as [`lint`](Self::lint) but provides source text for rules that
    /// need byte-level access (e.g., CSS class extraction).
    pub fn lint_with_source(
        &self,
        script: Option<&ScriptAnalysisSnapshot>,
        template: Option<&TemplateAnalysisSnapshot>,
        styles: &[StyleBlockAnalysis],
        source: Option<&str>,
    ) -> DiagnosticSet {
        self.lint_inner(script, template, styles, source, None)
    }

    /// Lint a file with cross-file analysis data.
    ///
    /// Same as [`lint`](Self::lint) but also runs cross-file rules using the
    /// pre-computed [`CrossFileSnapshot`].
    pub fn lint_with_cross_file(
        &self,
        script: Option<&ScriptAnalysisSnapshot>,
        template: Option<&TemplateAnalysisSnapshot>,
        styles: &[StyleBlockAnalysis],
        cross_file: Option<&CrossFileSnapshot>,
    ) -> DiagnosticSet {
        self.lint_inner(script, template, styles, None, cross_file)
    }

    /// Full lint pipeline.
    #[cfg_attr(feature = "hotpath", hotpath::measure)]
    fn lint_inner(
        &self,
        script: Option<&ScriptAnalysisSnapshot>,
        template: Option<&TemplateAnalysisSnapshot>,
        styles: &[StyleBlockAnalysis],
        source: Option<&str>,
        cross_file: Option<&CrossFileSnapshot>,
    ) -> DiagnosticSet {
        let rules = self.registry.rules();
        let mut ctx = LintContext::new(&self.config);
        let visitor = LintVisitor::new(rules);

        // Process comment directives from template first
        if let Some(tpl) = template {
            parse_comment_directives(&tpl.comment_directives, &mut ctx, source);
        }

        // Visit all analysis data
        if let Some(tpl) = template {
            visitor.visit_template(tpl, &mut ctx);
        }
        if let Some(s) = script {
            visitor.visit_script(s, &mut ctx);
        }
        visitor.visit_styles(styles, &mut ctx);

        // Visit file-level context (for rules that need cross-block reasoning)
        let file_ctx = FileContext {
            template,
            script,
            styles,
            source,
        };
        visitor.visit_file(&file_ctx, &mut ctx);

        // Visit cross-file data
        if let Some(cf) = cross_file {
            visitor.visit_cross_file(cf, &mut ctx);
        }

        ctx.into_diagnostic_set()
    }

    /// Get a reference to the configuration.
    pub fn config(&self) -> &LintConfig {
        &self.config
    }

    /// Get a mutable reference to the configuration.
    pub fn config_mut(&mut self) -> &mut LintConfig {
        &mut self.config
    }
}

impl Default for Linter {
    fn default() -> Self {
        Self::new(LintConfig::default())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn linter_with_no_data_returns_empty() {
        let linter = Linter::new(LintConfig::default());
        let set = linter.lint(None, None, &[]);
        assert!(set.is_empty());
    }

    #[test]
    fn linter_with_empty_analysis_returns_empty() {
        let mut config = LintConfig::default();
        // Disable valid-template-root since an empty snapshot has no elements
        config.rules.insert("valid-template-root".to_string(), None);
        let linter = Linter::new(config);
        let template = TemplateAnalysisSnapshot::default();
        let set = linter.lint(None, Some(&template), &[]);
        assert!(set.is_empty());
    }

    #[test]
    fn linter_returns_diagnostic_set_with_enrichment_api() {
        let linter = Linter::new(LintConfig::default());
        let mut set = linter.lint(None, None, &[]);
        assert_eq!(set.len(), 0);
        // DiagnosticSet supports enrichment: add + enhance
        set.add(crate::diagnostic::LintDiagnostic {
            rule: "test".to_string(),
            category: "test".to_string(),
            severity: crate::diagnostic::Severity::Warning,
            message: "test".to_string(),
            span: verter_span::Span::new(0, 10),
            tags: vec![],
            span_kind: crate::diagnostic::DiagnosticSpanKind::ElementOpenTag,
            certainty: crate::diagnostic::Certainty::Definite,
            evidence: Vec::new(),
            related_files: Vec::new(),
        });
        assert_eq!(set.len(), 1);
        set.enhance(0, |d| d.message = "enriched".to_string());
        let diags = set.into_diagnostics();
        assert_eq!(diags[0].message, "enriched");
    }
}
