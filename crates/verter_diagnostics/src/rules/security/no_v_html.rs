//! Rule: no-v-html
//!
//! Disallow use of v-html to prevent XSS attacks.

use crate::context::LintContext;
use crate::diagnostic::{DiagnosticSpanKind, Severity};
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
            // Try to find the v-html directive for precise span
            let (span_start, span_end) =
                if let Some(dir) = el.directives.iter().find(|d| d.name == "html") {
                    (dir.span.start, dir.span.end)
                } else {
                    (el.span.start, el.tag_span_end)
                };

            ctx.report_with_severity(
                self.name(),
                self.category().as_str(),
                "'v-html' directive can lead to XSS attack.".to_string(),
                span_start,
                span_end,
                self.default_severity(),
                DiagnosticSpanKind::Directive,
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
            has_text_content: false,
            has_bare_text: false,

            has_element_children: false,
            nesting_depth: 0,
            parent_tag: None,
            parent_index: None,
            dynamic_classes: vec![],
            span: Span::new(0, 50),
            tag_span_end: 50,
            content_end: 0,
            text_children: Vec::new(),
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
