//! Lint context: scope tracking, diagnostics accumulator, disabled ranges.

use crate::config::LintConfig;
use crate::diagnostic::{
    Certainty, DiagnosticSpanKind, DiagnosticTag, LintDiagnostic, RelatedFile, Severity,
};
use crate::diagnostic_set::DiagnosticSet;

/// Context passed to lint rules during traversal.
/// Accumulates diagnostics into a [`DiagnosticSet`] and tracks disabled ranges
/// from comment directives.
pub struct LintContext<'a> {
    /// Accumulated diagnostics.
    set: DiagnosticSet,
    /// Lint configuration (for severity lookups).
    config: &'a LintConfig,
    /// Disabled ranges: `(rule_name, start_offset, end_offset)`.
    /// A `None` rule_name means all rules are disabled in that range.
    /// Both range disables and next-line disables are stored here
    /// (next-line directives are converted to ranges at parse time).
    disabled_ranges: Vec<(Option<String>, u32, u32)>,
    /// Severity overrides from `@verter:level()` directives.
    /// Each entry: `(rule_name, severity, start_offset, end_offset)`.
    /// `None` rule_name = all rules; `None` severity = suppress (off).
    severity_overrides: Vec<(Option<String>, Option<Severity>, u32, u32)>,
    /// Rules that are off by default (opt-in). Only fired when explicitly
    /// enabled in the config `rules` map or under the Strict preset.
    default_off_rules: std::collections::HashSet<&'static str>,
}

impl<'a> LintContext<'a> {
    /// Create a new lint context with the given configuration.
    pub fn new(config: &'a LintConfig) -> Self {
        Self {
            set: DiagnosticSet::new(),
            config,
            disabled_ranges: Vec::new(),
            severity_overrides: Vec::new(),
            default_off_rules: std::collections::HashSet::new(),
        }
    }

    /// Create a new lint context, populating default-off rules from the registry.
    pub fn with_rules(config: &'a LintConfig, rules: &[Box<dyn crate::rules::LintRule>]) -> Self {
        let default_off_rules = rules
            .iter()
            .filter(|r| r.is_default_off())
            .map(|r| r.name())
            .collect();
        Self {
            set: DiagnosticSet::new(),
            config,
            disabled_ranges: Vec::new(),
            severity_overrides: Vec::new(),
            default_off_rules,
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
        span_kind: DiagnosticSpanKind,
    ) {
        self.report_with_severity(
            rule,
            category,
            message,
            span_start,
            span_end,
            Severity::Warning,
            span_kind,
        );
    }

    /// Report a diagnostic with a specific default severity.
    #[allow(clippy::too_many_arguments)]
    pub fn report_with_severity(
        &mut self,
        rule: &str,
        category: &str,
        message: String,
        span_start: u32,
        span_end: u32,
        default_severity: Severity,
        span_kind: DiagnosticSpanKind,
    ) {
        if self.is_disabled(rule, span_start) {
            return;
        }
        let severity = match self.config.effective_severity(
            rule,
            default_severity,
            self.default_off_rules.contains(rule),
        ) {
            Some(s) => s,
            None => return, // Rule disabled via config
        };
        // Apply @verter:level() overrides (last matching wins)
        let severity = match self.severity_override(rule, span_start) {
            Some(Some(s)) => s,   // Override to specific severity
            Some(None) => return, // off — suppress
            None => severity,     // No override
        };
        self.set.add(LintDiagnostic {
            rule: rule.to_string(),
            category: category.to_string(),
            severity,
            message,
            span: verter_span::Span::new(span_start, span_end),
            tags: vec![],
            span_kind,
            certainty: Certainty::Definite,
            evidence: Vec::new(),
            related_files: Vec::new(),
        });
    }

    /// Report a hint-severity diagnostic (faded out, lowest priority).
    pub fn report_hint(
        &mut self,
        rule: &str,
        category: &str,
        message: String,
        span_start: u32,
        span_end: u32,
        span_kind: DiagnosticSpanKind,
    ) {
        self.report_with_severity(
            rule,
            category,
            message,
            span_start,
            span_end,
            Severity::Hint,
            span_kind,
        );
    }

    /// Report a diagnostic with tags (e.g., `Unnecessary`, `Deprecated`).
    #[allow(clippy::too_many_arguments)]
    pub fn report_with_tags(
        &mut self,
        rule: &str,
        category: &str,
        message: String,
        span_start: u32,
        span_end: u32,
        default_severity: Severity,
        tags: Vec<DiagnosticTag>,
        span_kind: DiagnosticSpanKind,
    ) {
        if self.is_disabled(rule, span_start) {
            return;
        }
        let severity = match self.config.effective_severity(
            rule,
            default_severity,
            self.default_off_rules.contains(rule),
        ) {
            Some(s) => s,
            None => return,
        };
        let severity = match self.severity_override(rule, span_start) {
            Some(Some(s)) => s,
            Some(None) => return,
            None => severity,
        };
        self.set.add(LintDiagnostic {
            rule: rule.to_string(),
            category: category.to_string(),
            severity,
            message,
            span: verter_span::Span::new(span_start, span_end),
            tags,
            span_kind,
            certainty: Certainty::Definite,
            evidence: Vec::new(),
            related_files: Vec::new(),
        });
    }

    /// Report a diagnostic with `Partial` certainty and related files.
    #[allow(clippy::too_many_arguments)]
    pub fn report_partial(
        &mut self,
        rule: &str,
        category: &str,
        message: String,
        span_start: u32,
        span_end: u32,
        default_severity: Severity,
        span_kind: DiagnosticSpanKind,
        related_files: Vec<RelatedFile>,
    ) {
        if self.is_disabled(rule, span_start) {
            return;
        }
        let severity = match self.config.effective_severity(
            rule,
            default_severity,
            self.default_off_rules.contains(rule),
        ) {
            Some(s) => s,
            None => return,
        };
        let severity = match self.severity_override(rule, span_start) {
            Some(Some(s)) => s,
            Some(None) => return,
            None => severity,
        };
        self.set.add(LintDiagnostic {
            rule: rule.to_string(),
            category: category.to_string(),
            severity,
            message,
            span: verter_span::Span::new(span_start, span_end),
            tags: vec![],
            span_kind,
            certainty: Certainty::Partial,
            evidence: Vec::new(),
            related_files,
        });
    }

    /// Add a disabled range (from `@verter:ignore-start` to `@verter:ignore-end`).
    pub fn add_disabled_range(&mut self, rule: Option<String>, start: u32, end: u32) {
        self.disabled_ranges.push((rule, start, end));
    }

    /// Add a disabled next-line directive as a range.
    ///
    /// `start` is the byte offset where the directive ends (typically the end of the comment).
    /// `end` is the byte offset of the end of the next line (computed from source text).
    pub fn add_disabled_next_line(&mut self, rule: Option<String>, start: u32, end: u32) {
        self.disabled_ranges.push((rule, start, end));
    }

    /// Add a severity override for a rule in a given range.
    /// `None` severity means "off" (suppress).
    pub fn add_severity_override(
        &mut self,
        rule: Option<String>,
        severity: Option<Severity>,
        start: u32,
        end: u32,
    ) {
        self.severity_overrides.push((rule, severity, start, end));
    }

    /// Check if a rule has a severity override at a given byte offset.
    /// Returns `Some(Some(s))` for a severity override, `Some(None)` for "off", `None` for no override.
    fn severity_override(&self, rule: &str, offset: u32) -> Option<Option<Severity>> {
        let mut result = None;
        for (override_rule, severity, start, end) in &self.severity_overrides {
            if offset >= *start
                && offset <= *end
                && (override_rule.is_none() || override_rule.as_deref() == Some(rule))
            {
                result = Some(*severity); // last matching wins
            }
        }
        result
    }

    /// Check if a rule is disabled at a given byte offset.
    fn is_disabled(&self, rule: &str, offset: u32) -> bool {
        for (disabled_rule, start, end) in &self.disabled_ranges {
            if offset >= *start
                && offset <= *end
                && (disabled_rule.is_none() || disabled_rule.as_deref() == Some(rule))
            {
                return true;
            }
        }
        false
    }

    /// Consume the context and return the [`DiagnosticSet`].
    pub fn into_diagnostic_set(self) -> DiagnosticSet {
        self.set
    }

    /// Consume the context and return all accumulated diagnostics as a vec.
    pub fn into_diagnostics(self) -> Vec<LintDiagnostic> {
        self.set.into_diagnostics()
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
        ctx.report(
            "no-v-html",
            "security",
            "Avoid v-html".to_string(),
            10,
            20,
            DiagnosticSpanKind::Directive,
        );
        let diags = ctx.into_diagnostics();
        assert_eq!(diags.len(), 1);
        assert_eq!(diags[0].rule, "no-v-html");
        assert!(
            diags[0].tags.is_empty(),
            "default report should have no tags"
        );
    }

    #[test]
    fn disabled_rule_not_reported() {
        let mut config = LintConfig::default();
        config.rules.insert("no-v-html".to_string(), None);
        let mut ctx = LintContext::new(&config);
        ctx.report(
            "no-v-html",
            "security",
            "Avoid v-html".to_string(),
            10,
            20,
            DiagnosticSpanKind::Directive,
        );
        assert!(ctx.into_diagnostics().is_empty());
    }

    #[test]
    fn disabled_range_suppresses_diagnostic() {
        let config = LintConfig::default();
        let mut ctx = LintContext::new(&config);
        ctx.add_disabled_range(Some("no-v-html".to_string()), 5, 25);
        ctx.report(
            "no-v-html",
            "security",
            "Avoid v-html".to_string(),
            10,
            20,
            DiagnosticSpanKind::Directive,
        );
        assert!(ctx.into_diagnostics().is_empty());
    }

    #[test]
    fn disabled_range_all_rules_suppresses() {
        let config = LintConfig::default();
        let mut ctx = LintContext::new(&config);
        ctx.add_disabled_range(None, 5, 25);
        ctx.report(
            "no-v-html",
            "security",
            "Avoid v-html".to_string(),
            10,
            20,
            DiagnosticSpanKind::Directive,
        );
        ctx.report(
            "require-v-for-key",
            "vue-essential",
            "Missing key".to_string(),
            15,
            25,
            DiagnosticSpanKind::Directive,
        );
        assert!(ctx.into_diagnostics().is_empty());
    }

    #[test]
    fn report_hint_uses_hint_severity() {
        let config = LintConfig::default();
        let mut ctx = LintContext::new(&config);
        ctx.report_hint(
            "unused-css-selector",
            "css",
            "Unused selector".to_string(),
            5,
            20,
            DiagnosticSpanKind::CssSelector,
        );
        let diags = ctx.into_diagnostics();
        assert_eq!(diags.len(), 1);
        assert_eq!(diags[0].severity, Severity::Hint);
    }

    #[test]
    fn report_with_tags_adds_tags() {
        let config = LintConfig::default();
        let mut ctx = LintContext::new(&config);
        ctx.report_with_tags(
            "unused-css-selector",
            "css",
            "Unused selector".to_string(),
            5,
            20,
            Severity::Hint,
            vec![DiagnosticTag::Unnecessary],
            DiagnosticSpanKind::CssSelector,
        );
        let diags = ctx.into_diagnostics();
        assert_eq!(diags.len(), 1);
        assert_eq!(diags[0].tags, vec![DiagnosticTag::Unnecessary]);
        assert_eq!(diags[0].severity, Severity::Hint);
    }
}
