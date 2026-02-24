//! Rule: no-v-html
//!
//! Disallow use of v-html to prevent XSS attacks.

use crate::context::LintContext;
use crate::diagnostic::Severity;
use crate::rules::{LintRule, RuleCategory};
use verter_analysis::template::TemplateElement;

/// Disallow use of v-html to prevent XSS attacks.
pub struct NoVHtml;

impl LintRule for NoVHtml {
    fn name(&self) -> &'static str {
        "no-v-html"
    }

    fn category(&self) -> RuleCategory {
        RuleCategory::Security
    }

    fn default_severity(&self) -> Severity {
        Severity::Warning
    }

    fn check_element(&self, el: &TemplateElement, ctx: &mut LintContext) {
        if el.has_v_html {
            ctx.report_with_severity(
                self.name(),
                self.category().as_str(),
                "'v-html' directive can lead to XSS attack.".to_string(),
                el.span_start,
                el.span_end,
                self.default_severity(),
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

    fn run_rule(template: &TemplateAnalysisSnapshot) -> Vec<crate::diagnostic::LintDiagnostic> {
        let rules: Vec<Box<dyn LintRule>> = vec![Box::new(NoVHtml)];
        let visitor = LintVisitor::new(&rules);
        let config = LintConfig::default();
        let mut ctx = LintContext::new(&config);
        visitor.visit_template(template, &mut ctx);
        ctx.into_diagnostics()
    }

    fn make_element(has_v_html: bool) -> TemplateElement {
        TemplateElement {
            tag: "div".to_string(),
            is_component: false,
            is_self_closing: false,
            namespace: ElementNamespace::Html,
            attributes: vec![],
            directives: vec![],
            v_for: None,
            v_model: None,
            has_v_if: false,
            has_v_else: false,
            has_v_else_if: false,
            has_v_show: false,
            has_v_html,
            has_v_text: false,
            nesting_depth: 0,
            parent_tag: None,
            span_start: 0,
            span_end: 50,
        }
    }

    #[test]
    fn v_html_reports() {
        let template = TemplateAnalysisSnapshot {
            elements: vec![make_element(true)],
            ..Default::default()
        };
        let diags = run_rule(&template);
        assert_eq!(diags.len(), 1);
        assert!(diags[0].message.contains("XSS"));
    }

    #[test]
    fn no_v_html_passes() {
        let template = TemplateAnalysisSnapshot {
            elements: vec![make_element(false)],
            ..Default::default()
        };
        let diags = run_rule(&template);
        assert!(diags.is_empty());
    }
}
