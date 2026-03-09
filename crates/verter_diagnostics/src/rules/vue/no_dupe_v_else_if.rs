//! Rule: no-dupe-v-else-if
//!
//! Disallow duplicate conditions in v-if / v-else-if chains.

use crate::context::LintContext;
use crate::diagnostic::{DiagnosticSpanKind, Severity};
use crate::rules::{LintRule, RuleCategory};
use rustc_hash::FxHashSet;
use verter_analysis::template::TemplateAnalysisSnapshot;

/// Disallow duplicate conditions in v-if / v-else-if chains.
pub struct NoDupeVElseIf;

impl LintRule for NoDupeVElseIf {
    fn name(&self) -> &'static str {
        "no-dupe-v-else-if"
    }

    fn category(&self) -> RuleCategory {
        RuleCategory::VueEssential
    }

    fn default_severity(&self) -> Severity {
        Severity::Error
    }

    fn check_template(&self, tpl: &TemplateAnalysisSnapshot, ctx: &mut LintContext) {
        for chain in &tpl.if_chains {
            let mut seen = FxHashSet::default();
            for (expr, start, end) in &chain.conditions {
                if !seen.insert(expr.as_str()) {
                    ctx.report_with_severity(
                        self.name(),
                        self.category().as_str(),
                        format!(
                            "This branch can never execute. Its condition is a duplicate of a previous condition in the 'v-if' / 'v-else-if' chain: '{expr}'."
                        ),
                        *start,
                        *end,
                        self.default_severity(),
                        DiagnosticSpanKind::ConditionExpression,
                    );
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    use verter_analysis::template::*;

    fn run_rule(template: &TemplateAnalysisSnapshot) -> Vec<crate::diagnostic::LintDiagnostic> {
        crate::test_support::run_template_rule(NoDupeVElseIf, template)
    }

    #[test]
    fn duplicate_conditions_report() {
        let template = TemplateAnalysisSnapshot {
            if_chains: vec![IfChain {
                conditions: vec![
                    ("foo".to_string(), 0, 10),
                    ("bar".to_string(), 20, 30),
                    ("foo".to_string(), 40, 50), // duplicate
                ],
            }],
            ..Default::default()
        };
        let diags = run_rule(&template);
        assert_eq!(diags.len(), 1);
        assert!(diags[0].message.contains("foo"));
    }

    #[test]
    fn unique_conditions_pass() {
        let template = TemplateAnalysisSnapshot {
            if_chains: vec![IfChain {
                conditions: vec![
                    ("foo".to_string(), 0, 10),
                    ("bar".to_string(), 20, 30),
                    ("baz".to_string(), 40, 50),
                ],
            }],
            ..Default::default()
        };
        let diags = run_rule(&template);
        assert!(diags.is_empty());
    }
}
