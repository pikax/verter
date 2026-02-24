//! Linter engine: runs rules against analysis data.

use crate::comment_directives::parse_comment_directives;
use crate::config::LintConfig;
use crate::context::LintContext;
use crate::diagnostic::LintDiagnostic;
use crate::rules::RuleRegistry;
use crate::visitor::LintVisitor;
use verter_analysis::template::TemplateAnalysisSnapshot;
use verter_analysis::types::ScriptAnalysisSnapshot;
use verter_analysis::StyleBlockAnalysis;

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
    /// Accepts optional script, template, and style analysis snapshots.
    /// Returns all diagnostics produced by active rules.
    pub fn lint(
        &self,
        script: Option<&ScriptAnalysisSnapshot>,
        template: Option<&TemplateAnalysisSnapshot>,
        styles: &[StyleBlockAnalysis],
    ) -> Vec<LintDiagnostic> {
        let mut ctx = LintContext::new(&self.config);
        let visitor = LintVisitor::new(self.registry.rules());

        // Process comment directives from template first
        if let Some(tpl) = template {
            parse_comment_directives(&tpl.comment_directives, &mut ctx);
        }

        // Visit all analysis data
        if let Some(tpl) = template {
            visitor.visit_template(tpl, &mut ctx);
        }
        if let Some(script) = script {
            visitor.visit_script(script, &mut ctx);
        }
        visitor.visit_styles(styles, &mut ctx);

        ctx.into_diagnostics()
    }

    /// Get a reference to the configuration.
    pub fn config(&self) -> &LintConfig {
        &self.config
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn linter_with_no_data_returns_empty() {
        let linter = Linter::new(LintConfig::default());
        let diags = linter.lint(None, None, &[]);
        assert!(diags.is_empty());
    }

    #[test]
    fn linter_with_empty_analysis_returns_empty() {
        let linter = Linter::new(LintConfig::default());
        let template = TemplateAnalysisSnapshot::default();
        let diags = linter.lint(None, Some(&template), &[]);
        assert!(diags.is_empty());
    }
}
