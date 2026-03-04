//! Rule: tabindex-no-positive
//!
//! Disallow positive `tabindex` values. A positive tabindex disrupts the natural
//! tab order and makes keyboard navigation unpredictable. Use `0` (natural order)
//! or `-1` (programmatic focus only) instead.

use crate::context::LintContext;
use crate::diagnostic::{DiagnosticSpanKind, Severity};
use crate::rules::{LintRule, RuleCategory};
use verter_analysis::template::TemplateElement;

pub struct TabindexNoPositive;

impl LintRule for TabindexNoPositive {
    fn name(&self) -> &'static str {
        "tabindex-no-positive"
    }
    fn category(&self) -> RuleCategory {
        RuleCategory::Accessibility
    }
    fn default_severity(&self) -> Severity {
        Severity::Warning
    }
    fn check_element(&self, el: &TemplateElement, ctx: &mut LintContext) {
        for attr in &el.attributes {
            if attr.name == "tabindex" {
                if let Some(ref value) = attr.value {
                    if let Ok(n) = value.parse::<i32>() {
                        if n > 0 {
                            ctx.report_with_severity(
                                self.name(),
                                self.category().as_str(),
                                format!("Avoid positive tabindex ({}). Use 0 or -1 instead.", n),
                                attr.span.start,
                                attr.span.end,
                                self.default_severity(),
                                DiagnosticSpanKind::Attribute,
                            );
                        }
                    }
                }
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

    fn run(elements: Vec<TemplateElement>) -> Vec<crate::diagnostic::LintDiagnostic> {
        let rules: Vec<Box<dyn LintRule>> = vec![Box::new(TabindexNoPositive)];
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

    fn el(attrs: Vec<(&str, Option<&str>)>) -> TemplateElement {
        TemplateElement {
            tag: "div".to_string(),
            is_component: false,
            is_self_closing: false,
            namespace: ElementNamespace::Html,
            attributes: attrs
                .into_iter()
                .map(|(n, v)| TemplateAttribute {
                    name: n.to_string(),
                    value: v.map(|s| s.to_string()),
                    is_dynamic: false,
                    span: Span::new(0, 10),
                    name_end: 0,
                    value_span: None,
                })
                .collect(),
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
            text_children: Vec::new(),
        }
    }

    #[test]
    fn positive_tabindex_reports() {
        let diags = run(vec![el(vec![("tabindex", Some("5"))])]);
        assert_eq!(diags.len(), 1);
        assert!(diags[0].message.contains("5"));
    }

    #[test]
    fn zero_tabindex_passes() {
        assert!(run(vec![el(vec![("tabindex", Some("0"))])]).is_empty());
    }

    #[test]
    fn negative_tabindex_passes() {
        assert!(run(vec![el(vec![("tabindex", Some("-1"))])]).is_empty());
    }

    #[test]
    fn no_tabindex_passes() {
        assert!(run(vec![el(vec![])]).is_empty());
    }

    #[test]
    fn non_numeric_tabindex_passes() {
        assert!(run(vec![el(vec![("tabindex", Some("abc"))])]).is_empty());
    }
}
