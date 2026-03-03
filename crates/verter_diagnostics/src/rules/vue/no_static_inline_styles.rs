//! Rule: no-static-inline-styles
//!
//! Disallow static `style="..."` attributes on elements. Prefer CSS classes
//! or `:style` bindings for maintainability.

use crate::context::LintContext;
use crate::diagnostic::{DiagnosticSpanKind, Severity};
use crate::rules::{LintRule, RuleCategory};
use verter_analysis::template::TemplateElement;

pub struct NoStaticInlineStyles;

impl LintRule for NoStaticInlineStyles {
    fn name(&self) -> &'static str {
        "no-static-inline-styles"
    }

    fn category(&self) -> RuleCategory {
        RuleCategory::VueRecommended
    }

    fn default_severity(&self) -> Severity {
        Severity::Warning
    }

    fn check_element(&self, el: &TemplateElement, ctx: &mut LintContext) {
        for attr in &el.attributes {
            if attr.name == "style" && !attr.is_dynamic {
                ctx.report_with_severity(
                    self.name(),
                    self.category().as_str(),
                    "Avoid using static inline 'style'. Use CSS classes or ':style' bindings instead."
                        .to_string(),
                    attr.span.start,
                    attr.span.end,
                    self.default_severity(),
                    DiagnosticSpanKind::Attribute,
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
        let rules: Vec<Box<dyn LintRule>> = vec![Box::new(NoStaticInlineStyles)];
        let visitor = LintVisitor::new(&rules);
        let config = LintConfig::default();
        let mut ctx = LintContext::new(&config);
        visitor.visit_template(template, &mut ctx);
        ctx.into_diagnostics()
    }

    #[test]
    fn static_style_reports() {
        let template = TemplateAnalysisSnapshot {
            elements: vec![TemplateElement {
                tag: "div".to_string(),
                attributes: vec![TemplateAttribute {
                    name: "style".to_string(),
                    value: Some("color: red".to_string()),
                    is_dynamic: false,
                    span: Span::new(5, 22),
                }],
                span: Span::new(0, 30),
                tag_span_end: 25,
                ..Default::default()
            }],
            ..Default::default()
        };
        let diags = run(&template);
        assert!(!diags.is_empty(), "static style should trigger");
        assert!(diags.iter().any(|d| d.rule == "no-static-inline-styles"));
        assert!(
            diags[0].message.contains("style"),
            "message should mention style"
        );
        assert!(
            !diags.iter().any(|d| d.rule == "no-v-html"),
            "must not trigger unrelated rule"
        );
    }

    #[test]
    fn dynamic_style_passes() {
        let template = TemplateAnalysisSnapshot {
            elements: vec![TemplateElement {
                tag: "div".to_string(),
                attributes: vec![TemplateAttribute {
                    name: "style".to_string(),
                    value: Some("{ color: myColor }".to_string()),
                    is_dynamic: true,
                    span: Span::new(5, 30),
                }],
                span: Span::new(0, 35),
                tag_span_end: 32,
                ..Default::default()
            }],
            ..Default::default()
        };
        let diags = run(&template);
        assert!(diags.is_empty(), "dynamic :style should pass");
    }

    #[test]
    fn class_attribute_passes() {
        let template = TemplateAnalysisSnapshot {
            elements: vec![TemplateElement {
                tag: "div".to_string(),
                attributes: vec![TemplateAttribute {
                    name: "class".to_string(),
                    value: Some("container".to_string()),
                    is_dynamic: false,
                    span: Span::new(5, 22),
                }],
                span: Span::new(0, 30),
                tag_span_end: 25,
                ..Default::default()
            }],
            ..Default::default()
        };
        let diags = run(&template);
        assert!(diags.is_empty(), "class attribute should pass");
    }
}
