//! Rule: no-deprecated-filter
//!
//! Filters (`{{ value | filterName }}`) were removed in Vue 3.
//! Use computed properties or method calls instead.
//! Detects pipe `|` in template binding expressions (excluding `||`).

// @ai-generated

use crate::context::LintContext;
use crate::diagnostic::{DiagnosticSpanKind, DiagnosticTag, Severity};
use crate::rules::{LintRule, RuleCategory};
use verter_analysis::template::TemplateAnalysisSnapshot;

pub struct NoDeprecatedFilter;

impl LintRule for NoDeprecatedFilter {
    fn name(&self) -> &'static str {
        "no-deprecated-filter"
    }

    fn category(&self) -> RuleCategory {
        RuleCategory::VueEssential
    }

    fn default_severity(&self) -> Option<Severity> {
        Some(Severity::Error)
    }

    fn check_template(&self, tpl: &TemplateAnalysisSnapshot, ctx: &mut LintContext) {
        for occ in &tpl.binding_occurrences {
            if has_filter_pipe(&occ.name) {
                ctx.report_with_tags(
                    self.name(),
                    self.category().as_str(),
                    "Filters have been removed in Vue 3. Use computed properties or method calls instead.".to_string(),
                    occ.span.start,
                    occ.span.end,
                    self.default_severity(),
                    vec![DiagnosticTag::Deprecated],
                    DiagnosticSpanKind::Interpolation,
                );
            }
        }
    }
}

/// Check if the expression contains a filter pipe `|` that is not part of `||`.
fn has_filter_pipe(expr: &str) -> bool {
    let bytes = expr.as_bytes();
    let len = bytes.len();
    let mut i = 0;
    while i < len {
        if bytes[i] == b'|' {
            // Check it's not `||` (logical OR)
            let prev_is_pipe = i > 0 && bytes[i - 1] == b'|';
            let next_is_pipe = i + 1 < len && bytes[i + 1] == b'|';
            if !prev_is_pipe && !next_is_pipe {
                return true;
            }
            // Skip the second `|` in `||`
            if next_is_pipe {
                i += 1;
            }
        }
        i += 1;
    }
    false
}

#[cfg(test)]
mod tests {
    use super::*;

    use verter_analysis::template::*;
    use verter_span::Span;

    fn run(template: &TemplateAnalysisSnapshot) -> Vec<crate::diagnostic::LintDiagnostic> {
        crate::test_support::run_template_rule(NoDeprecatedFilter, template)
    }

    #[test]
    fn filter_pipe_reports() {
        let template = TemplateAnalysisSnapshot {
            binding_occurrences: vec![TemplateBindingOccurrence {
                name: "value | capitalize".to_string(),
                span: Span::new(5, 23),
                usage_kind: BindingUsageKind::Interpolation,
            }],
            ..Default::default()
        };
        let diags = run(&template);
        assert!(!diags.is_empty(), "filter pipe should trigger");
        assert!(diags.iter().any(|d| d.rule == "no-deprecated-filter"));
        assert!(
            diags[0].tags.contains(&DiagnosticTag::Deprecated),
            "should have Deprecated tag"
        );
        assert!(
            !diags.iter().any(|d| d.rule == "no-v-html"),
            "must not trigger unrelated rule"
        );
    }

    #[test]
    fn logical_or_passes() {
        let template = TemplateAnalysisSnapshot {
            binding_occurrences: vec![TemplateBindingOccurrence {
                name: "a || b".to_string(),
                span: Span::new(5, 11),
                usage_kind: BindingUsageKind::Interpolation,
            }],
            ..Default::default()
        };
        let diags = run(&template);
        assert!(diags.is_empty(), "logical OR (||) should not trigger");
    }

    #[test]
    fn plain_binding_passes() {
        let template = TemplateAnalysisSnapshot {
            binding_occurrences: vec![TemplateBindingOccurrence {
                name: "message".to_string(),
                span: Span::new(5, 12),
                usage_kind: BindingUsageKind::Interpolation,
            }],
            ..Default::default()
        };
        let diags = run(&template);
        assert!(diags.is_empty(), "plain binding should pass");
    }

    #[test]
    fn has_filter_pipe_unit_tests() {
        assert!(has_filter_pipe("value | capitalize"));
        assert!(has_filter_pipe("a | b | c"));
        assert!(!has_filter_pipe("a || b"));
        assert!(!has_filter_pipe("message"));
        assert!(!has_filter_pipe(""));
    }
}
