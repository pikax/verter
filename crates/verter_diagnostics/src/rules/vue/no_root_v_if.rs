//! Rule: no-root-v-if
//!
//! Disallow `v-if` on root elements. If the condition is false, the component
//! renders nothing, which may cause unexpected behavior.

use crate::context::LintContext;
use crate::diagnostic::{DiagnosticSpanKind, Severity};
use crate::rules::{LintRule, RuleCategory};
use verter_analysis::template::TemplateAnalysisSnapshot;

pub struct NoRootVIf;

impl LintRule for NoRootVIf {
    fn name(&self) -> &'static str {
        "no-root-v-if"
    }

    fn category(&self) -> RuleCategory {
        RuleCategory::VueEssential
    }

    fn default_severity(&self) -> Severity {
        Severity::Warning
    }

    fn check_template(&self, tpl: &TemplateAnalysisSnapshot, ctx: &mut LintContext) {
        for el in &tpl.elements {
            // Root elements have no parent
            if el.parent_index.is_some() {
                continue;
            }
            if el.has_v_if {
                ctx.report_with_severity(
                    self.name(),
                    self.category().as_str(),
                    "'v-if' should not be used on the root element — the component may render nothing."
                        .to_string(),
                    el.span.start,
                    el.tag_span_end,
                    self.default_severity(),
                    DiagnosticSpanKind::ElementOpenTag,
                );
            }
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
        let rules: Vec<Box<dyn LintRule>> = vec![Box::new(NoRootVIf)];
        let visitor = LintVisitor::new(&rules);
        let config = LintConfig::default();
        let mut ctx = LintContext::new(&config);
        visitor.visit_template(template, &mut ctx);
        ctx.into_diagnostics()
    }

    #[test]
    fn root_v_if_reports() {
        let template = TemplateAnalysisSnapshot {
            elements: vec![TemplateElement {
                tag: "div".to_string(),
                has_v_if: true,
                parent_index: None,
                span: Span::new(0, 40),
                tag_span_end: 20,
                ..Default::default()
            }],
            ..Default::default()
        };
        let diags = run(&template);
        assert!(!diags.is_empty(), "root v-if should trigger");
        assert!(diags.iter().any(|d| d.rule == "no-root-v-if"));
        assert!(
            diags[0].message.contains("v-if"),
            "message should mention v-if"
        );
        assert!(
            !diags.iter().any(|d| d.rule == "valid-v-if"),
            "must not trigger unrelated rule"
        );
    }

    #[test]
    fn nested_v_if_passes() {
        let template = TemplateAnalysisSnapshot {
            elements: vec![TemplateElement {
                tag: "div".to_string(),
                has_v_if: true,
                parent_index: Some(0),
                parent_tag: Some("div".to_string()),
                span: Span::new(10, 40),
                tag_span_end: 25,
                ..Default::default()
            }],
            ..Default::default()
        };
        let diags = run(&template);
        assert!(diags.is_empty(), "nested v-if should pass");
    }

    #[test]
    fn root_without_v_if_passes() {
        let template = TemplateAnalysisSnapshot {
            elements: vec![TemplateElement {
                tag: "div".to_string(),
                parent_index: None,
                span: Span::new(0, 40),
                tag_span_end: 5,
                ..Default::default()
            }],
            ..Default::default()
        };
        let diags = run(&template);
        assert!(diags.is_empty(), "root without v-if should pass");
    }
}
