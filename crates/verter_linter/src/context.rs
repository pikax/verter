//! Lint context: scope tracking, diagnostics accumulator, disabled ranges.

use crate::config::LintConfig;
use crate::diagnostic::{LintDiagnostic, LintFix, Severity};

/// Context passed to lint rules during traversal.
/// Accumulates diagnostics and tracks disabled ranges from comment directives.
pub struct LintContext<'a> {
    /// Accumulated diagnostics.
    diagnostics: Vec<LintDiagnostic>,
    /// Lint configuration (for severity lookups).
    config: &'a LintConfig,
    /// Disabled ranges: `(rule_name, start_offset, end_offset)`.
    /// A `None` rule_name means all rules are disabled in that range.
    disabled_ranges: Vec<(Option<String>, u32, u32)>,
    /// Per-line disables: `(rule_name, line_offset)`.
    disabled_next_lines: Vec<(Option<String>, u32)>,
}

impl<'a> LintContext<'a> {
    /// Create a new lint context with the given configuration.
    pub fn new(config: &'a LintConfig) -> Self {
        Self {
            diagnostics: Vec::new(),
            config,
            disabled_ranges: Vec::new(),
            disabled_next_lines: Vec::new(),
        }
    }

    /// Report a diagnostic. The diagnostic is only added if the rule is not
    /// disabled at the given span.
    pub fn report(
        &mut self,
        rule: &str,
        category: &str,
        message: String,
        span_start: u32,
        span_end: u32,
    ) {
        self.report_with_severity(
            rule,
            category,
            message,
            span_start,
            span_end,
            Severity::Warning,
        );
    }

    /// Report a diagnostic with a specific default severity.
    pub fn report_with_severity(
        &mut self,
        rule: &str,
        category: &str,
        message: String,
        span_start: u32,
        span_end: u32,
        default_severity: Severity,
    ) {
        if self.is_disabled(rule, span_start) {
            return;
        }
        let severity = match self.config.effective_severity(rule, default_severity) {
            Some(s) => s,
            None => return, // Rule disabled via config
        };
        self.diagnostics.push(LintDiagnostic {
            rule: rule.to_string(),
            category: category.to_string(),
            severity,
            message,
            span_start,
            span_end,
            fix: None,
        });
    }

    /// Report a diagnostic with a suggested fix.
    pub fn report_with_fix(
        &mut self,
        rule: &str,
        category: &str,
        message: String,
        span_start: u32,
        span_end: u32,
        fix: LintFix,
    ) {
        if self.is_disabled(rule, span_start) {
            return;
        }
        let severity = match self.config.effective_severity(rule, Severity::Warning) {
            Some(s) => s,
            None => return,
        };
        self.diagnostics.push(LintDiagnostic {
            rule: rule.to_string(),
            category: category.to_string(),
            severity,
            message,
            span_start,
            span_end,
            fix: Some(fix),
        });
    }

    /// Add a disabled range (from `@verter:ignore-start` to `@verter:ignore-end`).
    pub fn add_disabled_range(&mut self, rule: Option<String>, start: u32, end: u32) {
        self.disabled_ranges.push((rule, start, end));
    }

    /// Add a disabled next-line directive.
    pub fn add_disabled_next_line(&mut self, rule: Option<String>, line_offset: u32) {
        self.disabled_next_lines.push((rule, line_offset));
    }

    /// Check if a rule is disabled at a given byte offset.
    fn is_disabled(&self, rule: &str, offset: u32) -> bool {
        // Check range disables
        for (disabled_rule, start, end) in &self.disabled_ranges {
            if offset >= *start
                && offset <= *end
                && (disabled_rule.is_none() || disabled_rule.as_deref() == Some(rule))
            {
                return true;
            }
        }
        // Check next-line disables
        for (disabled_rule, line_offset) in &self.disabled_next_lines {
            if offset >= *line_offset
                && offset <= *line_offset + 1000
                && (disabled_rule.is_none() || disabled_rule.as_deref() == Some(rule))
            {
                return true;
            }
        }
        false
    }

    /// Consume the context and return all accumulated diagnostics.
    pub fn into_diagnostics(self) -> Vec<LintDiagnostic> {
        self.diagnostics
    }

    /// Get a reference to the lint configuration.
    pub fn config(&self) -> &LintConfig {
        self.config
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::LintConfig;

    #[test]
    fn report_adds_diagnostic() {
        let config = LintConfig::default();
        let mut ctx = LintContext::new(&config);
        ctx.report("no-v-html", "security", "Avoid v-html".to_string(), 10, 20);
        let diags = ctx.into_diagnostics();
        assert_eq!(diags.len(), 1);
        assert_eq!(diags[0].rule, "no-v-html");
    }

    #[test]
    fn disabled_rule_not_reported() {
        let mut config = LintConfig::default();
        config.rules.insert("no-v-html".to_string(), None);
        let mut ctx = LintContext::new(&config);
        ctx.report("no-v-html", "security", "Avoid v-html".to_string(), 10, 20);
        assert!(ctx.into_diagnostics().is_empty());
    }

    #[test]
    fn disabled_range_suppresses_diagnostic() {
        let config = LintConfig::default();
        let mut ctx = LintContext::new(&config);
        ctx.add_disabled_range(Some("no-v-html".to_string()), 5, 25);
        ctx.report("no-v-html", "security", "Avoid v-html".to_string(), 10, 20);
        assert!(ctx.into_diagnostics().is_empty());
    }

    #[test]
    fn disabled_range_all_rules_suppresses() {
        let config = LintConfig::default();
        let mut ctx = LintContext::new(&config);
        ctx.add_disabled_range(None, 5, 25);
        ctx.report("no-v-html", "security", "Avoid v-html".to_string(), 10, 20);
        ctx.report(
            "require-v-for-key",
            "vue-essential",
            "Missing key".to_string(),
            15,
            25,
        );
        assert!(ctx.into_diagnostics().is_empty());
    }

    #[test]
    fn report_with_fix() {
        let config = LintConfig::default();
        let mut ctx = LintContext::new(&config);
        ctx.report_with_fix(
            "v-bind-style",
            "vue-recommended",
            "Use shorthand".to_string(),
            5,
            20,
            LintFix {
                description: "Use ':'".to_string(),
                replacement: ":class".to_string(),
                span_start: 5,
                span_end: 17,
            },
        );
        let diags = ctx.into_diagnostics();
        assert_eq!(diags.len(), 1);
        assert!(diags[0].fix.is_some());
    }
}
