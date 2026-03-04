//! Rule: no-distracting-elements
//!
//! Disallow `<marquee>` and `<blink>` elements. These elements are visually
//! distracting and can cause accessibility issues, especially for users with
//! attention or vestibular disorders.

use crate::context::LintContext;
use crate::diagnostic::{DiagnosticSpanKind, Severity};
use crate::rules::{LintRule, RuleCategory};
use verter_analysis::template::TemplateElement;

pub struct NoDistractingElements;

impl LintRule for NoDistractingElements {
    fn name(&self) -> &'static str {
        "no-distracting-elements"
    }
    fn category(&self) -> RuleCategory {
        RuleCategory::Accessibility
    }
    fn default_severity(&self) -> Severity {
        Severity::Warning
    }
    fn check_element(&self, el: &TemplateElement, ctx: &mut LintContext) {
        if el.is_component {
            return;
        }
        if matches!(el.tag.as_str(), "marquee" | "blink") {
            ctx.report_with_severity(
                self.name(),
                self.category().as_str(),
                format!(
                    "<{}> elements are distracting and should not be used.",
                    el.tag
                ),
                el.span.start,
                el.tag_span_end,
                self.default_severity(),
                DiagnosticSpanKind::ElementOpenTag,
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

    fn run(elements: Vec<TemplateElement>) -> Vec<crate::diagnostic::LintDiagnostic> {
        let rules: Vec<Box<dyn LintRule>> = vec![Box::new(NoDistractingElements)];
        let visitor = LintVisitor::new(&rules);
        let config = LintConfig::default();
        let mut ctx = LintContext::new(&config);
        visitor.visit_template(
            &TemplateAnalysisSnapshot {
                elements,
                ..Default::default()
            },
            &mut ctx,
        );
        ctx.into_diagnostics()
    }

    fn el(tag: &str) -> TemplateElement {
        TemplateElement {
            tag: tag.to_string(),
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
            has_v_html: false,
            has_v_text: false,
            has_text_content: false,
            has_bare_text: false,

            has_element_children: false,
            nesting_depth: 0,
            parent_tag: None,
            parent_index: None,
            dynamic_classes: vec![],
            span: Span::new(0, 30),
            tag_span_end: 30,
            content_end: 0,
            ..Default::default()
        }
    }

    #[test]
    fn marquee_reports() {
        let diags = run(vec![el("marquee")]);
        assert_eq!(diags.len(), 1);
        assert!(diags[0].message.contains("<marquee>"));
    }

    #[test]
    fn blink_reports() {
        let diags = run(vec![el("blink")]);
        assert_eq!(diags.len(), 1);
        assert!(diags[0].message.contains("<blink>"));
    }

    #[test]
    fn div_passes() {
        assert!(run(vec![el("div")]).is_empty());
    }

    #[test]
    fn span_passes() {
        assert!(run(vec![el("span")]).is_empty());
    }
}
