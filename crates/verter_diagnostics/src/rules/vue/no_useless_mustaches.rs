//! Rule: no-useless-mustaches
//!
//! Disallow unnecessary mustache interpolations: `{{ "literal" }}` or `{{ 'literal' }}`
//! where a plain text node would suffice.

use crate::context::LintContext;
use crate::diagnostic::{DiagnosticSpanKind, Severity};
use crate::rules::{LintRule, RuleCategory};
use verter_analysis::template::TemplateBindingOccurrence;

pub struct NoUselessMustaches;

/// Check if an expression is a string literal (`"..."` or `'...'` or `` `...` `` without interpolation).
fn is_string_literal(expr: &str) -> bool {
    let trimmed = expr.trim();
    if trimmed.len() < 2 {
        return false;
    }
    let bytes = trimmed.as_bytes();
    let first = bytes[0];
    let last = bytes[bytes.len() - 1];
    if (first == b'\'' || first == b'"') && first == last {
        return true;
    }
    // Template literal without `${` interpolation
    if first == b'`' && last == b'`' && !trimmed.contains("${") {
        return true;
    }
    false
}

impl LintRule for NoUselessMustaches {
    fn name(&self) -> &'static str {
        "no-useless-mustaches"
    }

    fn category(&self) -> RuleCategory {
        RuleCategory::VueRecommended
    }

    fn default_severity(&self) -> Severity {
        Severity::Warning
    }

    fn check_interpolation(&self, occ: &TemplateBindingOccurrence, ctx: &mut LintContext) {
        // The binding occurrence name is the full expression inside {{ }}
        if is_string_literal(&occ.name) {
            ctx.report_with_severity(
                self.name(),
                self.category().as_str(),
                format!(
                    "Unnecessary mustache interpolation with a string literal. Use plain text instead of '{{{{ {} }}}}'.",
                    occ.name
                ),
                occ.span.start,
                occ.span.end,
                self.default_severity(),
                DiagnosticSpanKind::Interpolation,
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::LintConfig;
    use crate::visitor::LintVisitor;
    use verter_analysis::template::*;
    use verter_span::Span;

    fn run(template: &TemplateAnalysisSnapshot) -> Vec<crate::diagnostic::LintDiagnostic> {
        let rules: Vec<Box<dyn LintRule>> = vec![Box::new(NoUselessMustaches)];
        let visitor = LintVisitor::new(&rules);
        let config = LintConfig::default();
        let mut ctx = LintContext::new(&config);
        visitor.visit_template(template, &mut ctx);
        ctx.into_diagnostics()
    }

    #[test]
    fn string_literal_in_mustache_reports() {
        let template = TemplateAnalysisSnapshot {
            binding_occurrences: vec![TemplateBindingOccurrence {
                name: "\"hello\"".to_string(),
                span: Span::new(5, 20),
                usage_kind: BindingUsageKind::Interpolation,
            }],
            ..Default::default()
        };
        let diags = run(&template);
        assert!(
            !diags.is_empty(),
            "string literal in mustache should trigger"
        );
        assert!(diags.iter().any(|d| d.rule == "no-useless-mustaches"));
        assert!(
            !diags.iter().any(|d| d.rule == "valid-v-once"),
            "must not trigger unrelated rule"
        );
    }

    #[test]
    fn variable_in_mustache_passes() {
        let template = TemplateAnalysisSnapshot {
            binding_occurrences: vec![TemplateBindingOccurrence {
                name: "message".to_string(),
                span: Span::new(5, 18),
                usage_kind: BindingUsageKind::Interpolation,
            }],
            ..Default::default()
        };
        let diags = run(&template);
        assert!(diags.is_empty(), "variable in mustache should pass");
    }

    #[test]
    fn single_quoted_literal_reports() {
        let template = TemplateAnalysisSnapshot {
            binding_occurrences: vec![TemplateBindingOccurrence {
                name: "'hello'".to_string(),
                span: Span::new(5, 20),
                usage_kind: BindingUsageKind::Interpolation,
            }],
            ..Default::default()
        };
        let diags = run(&template);
        assert!(
            !diags.is_empty(),
            "single-quoted literal in mustache should trigger"
        );
    }

    #[test]
    fn template_literal_with_interpolation_passes() {
        let template = TemplateAnalysisSnapshot {
            binding_occurrences: vec![TemplateBindingOccurrence {
                name: "`hello ${name}`".to_string(),
                span: Span::new(5, 25),
                usage_kind: BindingUsageKind::Interpolation,
            }],
            ..Default::default()
        };
        let diags = run(&template);
        assert!(
            diags.is_empty(),
            "template literal with interpolation should pass"
        );
    }
}
