use crate::context::LintContext;
use crate::diagnostic::{DiagnosticSpanKind, Severity};
use crate::rules::{LintRule, RuleCategory};
use verter_semantic::analysis::template::TemplateAnalysisSnapshot;

/// Warns about nondeterministic expressions in template interpolations
/// (`Date.now()`, `Math.random()`, `new Date()`). These produce different
/// values on server vs client, causing hydration mismatches.
pub struct NoNondeterministicInTemplate;

const NONDETERMINISTIC_NAMES: &[&str] = &["Date", "Math"];

impl LintRule for NoNondeterministicInTemplate {
    fn name(&self) -> &'static str {
        "no-nondeterministic-in-template"
    }

    fn category(&self) -> RuleCategory {
        RuleCategory::Ssr
    }

    fn default_severity(&self) -> Option<Severity> {
        Some(Severity::Warning)
    }

    fn check_template(&self, tpl: &TemplateAnalysisSnapshot, ctx: &mut LintContext) {
        if !ctx.config().ssr_mode {
            return;
        }

        // Check unresolved bindings for Date/Math references
        for binding in &tpl.unresolved_bindings {
            if NONDETERMINISTIC_NAMES.contains(&binding.name.as_str()) {
                ctx.report_with_severity(
                    self.name(),
                    self.category().as_str(),
                    format!(
                        "`{}` produces different values on server vs client, causing hydration mismatch. Use a computed property or `useId()` instead.",
                        binding.name
                    ),
                    binding.span.start,
                    binding.span.end,
                    self.default_severity(),
                    DiagnosticSpanKind::Interpolation,
                );
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::{run_template_rule, run_template_rule_ssr};
    use verter_semantic::analysis::template::{TemplateAnalysisSnapshot, UnresolvedBinding};
    use verter_span::Span;

    fn unresolved(name: &str) -> UnresolvedBinding {
        UnresolvedBinding {
            name: name.to_string(),
            span: Span::new(10, 10 + name.len() as u32),
        }
    }

    #[test]
    fn no_report_without_ssr_mode() {
        let tpl = TemplateAnalysisSnapshot {
            unresolved_bindings: vec![unresolved("Date")],
            ..Default::default()
        };
        let diags = run_template_rule(NoNondeterministicInTemplate, &tpl);
        assert!(diags.is_empty());
    }

    #[test]
    fn reports_date_in_ssr() {
        let tpl = TemplateAnalysisSnapshot {
            unresolved_bindings: vec![unresolved("Date")],
            ..Default::default()
        };
        let diags = run_template_rule_ssr(NoNondeterministicInTemplate, &tpl);
        assert_eq!(diags.len(), 1);
        assert!(diags[0].message.contains("Date"));
        assert!(diags[0].message.contains("hydration mismatch"));
    }

    #[test]
    fn reports_math_in_ssr() {
        let tpl = TemplateAnalysisSnapshot {
            unresolved_bindings: vec![unresolved("Math")],
            ..Default::default()
        };
        let diags = run_template_rule_ssr(NoNondeterministicInTemplate, &tpl);
        assert_eq!(diags.len(), 1);
    }

    #[test]
    fn ignores_safe_globals() {
        let tpl = TemplateAnalysisSnapshot {
            unresolved_bindings: vec![unresolved("JSON"), unresolved("Array")],
            ..Default::default()
        };
        let diags = run_template_rule_ssr(NoNondeterministicInTemplate, &tpl);
        assert!(diags.is_empty());
    }
}
