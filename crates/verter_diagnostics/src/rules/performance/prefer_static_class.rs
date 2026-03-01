//! Rule: prefer-static-class
//!
//! Prefer static `class` over dynamic `:class` when the bound expression is
//! a plain string literal (e.g., `:class="'container'"` should be `class="container"`).
//! Dynamic class bindings with string literals add unnecessary runtime overhead
//! because the reactivity system still tracks them even though the value never changes.

use crate::context::LintContext;
use crate::diagnostic::{DiagnosticSpanKind, Severity};
use crate::rules::{LintRule, RuleCategory};
use verter_analysis::template::TemplateElement;

/// Prefer static `class` over dynamic `:class` for string literal values.
pub struct PreferStaticClass;

impl LintRule for PreferStaticClass {
    fn name(&self) -> &'static str {
        "prefer-static-class"
    }

    fn category(&self) -> RuleCategory {
        RuleCategory::Performance
    }

    fn default_severity(&self) -> Severity {
        Severity::Warning
    }

    fn check_element(&self, el: &TemplateElement, ctx: &mut LintContext) {
        for attr in &el.attributes {
            // Dynamic class binding: `:class` is represented as
            // a dynamic attribute with name "class".
            if attr.is_dynamic && attr.name == "class" {
                if let Some(value) = &attr.value {
                    // Check if the value is a plain string literal:
                    // e.g., `'container'` or `"container"`.
                    let trimmed = value.trim();
                    let is_string_literal = (trimmed.starts_with('\'')
                        && trimmed.ends_with('\'')
                        && trimmed.len() >= 2)
                        || (trimmed.starts_with('"')
                            && trimmed.ends_with('"')
                            && trimmed.len() >= 2);

                    if is_string_literal {
                        ctx.report_with_severity(
                            self.name(),
                            self.category().as_str(),
                            "Prefer static `class` over dynamic `:class` when the value is a string literal.".to_string(),
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::LintConfig;
    use crate::visitor::LintVisitor;
    use verter_analysis::template::*;
    use verter_span::Span;

    fn run_rule(template: &TemplateAnalysisSnapshot) -> Vec<crate::diagnostic::LintDiagnostic> {
        let rules: Vec<Box<dyn LintRule>> = vec![Box::new(PreferStaticClass)];
        let visitor = LintVisitor::new(&rules);
        let config = LintConfig::default();
        let mut ctx = LintContext::new(&config);
        visitor.visit_template(template, &mut ctx);
        ctx.into_diagnostics()
    }

    fn make_element_with_attrs(attrs: Vec<TemplateAttribute>) -> TemplateElement {
        TemplateElement {
            tag: "div".to_string(),
            is_component: false,
            is_self_closing: false,
            namespace: ElementNamespace::Html,
            attributes: attrs,
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

            has_element_children: false,
            nesting_depth: 0,
            parent_tag: None,
            parent_index: None,
            dynamic_classes: vec![],
            span: Span::new(0, 50),
            tag_span_end: 50,
        }
    }

    /// @ai-generated - Rule metadata is correct
    #[test]
    fn rule_metadata() {
        let rule = PreferStaticClass;
        assert_eq!(rule.name(), "prefer-static-class");
        assert_eq!(rule.category(), RuleCategory::Performance);
        assert_eq!(rule.default_severity(), Severity::Warning);
    }

    /// @ai-generated - Dynamic class with string literal reports
    #[test]
    fn dynamic_class_string_literal_reports() {
        let template = TemplateAnalysisSnapshot {
            elements: vec![make_element_with_attrs(vec![TemplateAttribute {
                name: "class".to_string(),
                value: Some("'container'".to_string()),
                is_dynamic: true,
                span: Span::new(5, 25),
            }])],
            ..Default::default()
        };
        let diags = run_rule(&template);
        assert_eq!(diags.len(), 1);
        assert_eq!(diags[0].rule, "prefer-static-class");
        assert!(diags[0].message.contains("static"));
    }

    /// @ai-generated - Dynamic class with double-quoted string literal reports
    #[test]
    fn dynamic_class_double_quoted_string_literal_reports() {
        let template = TemplateAnalysisSnapshot {
            elements: vec![make_element_with_attrs(vec![TemplateAttribute {
                name: "class".to_string(),
                value: Some("\"container\"".to_string()),
                is_dynamic: true,
                span: Span::new(5, 25),
            }])],
            ..Default::default()
        };
        let diags = run_rule(&template);
        assert_eq!(diags.len(), 1);
    }

    /// @ai-generated - Dynamic class with expression does not report
    #[test]
    fn dynamic_class_expression_passes() {
        let template = TemplateAnalysisSnapshot {
            elements: vec![make_element_with_attrs(vec![TemplateAttribute {
                name: "class".to_string(),
                value: Some("isActive ? 'active' : ''".to_string()),
                is_dynamic: true,
                span: Span::new(5, 40),
            }])],
            ..Default::default()
        };
        let diags = run_rule(&template);
        assert!(diags.is_empty());
    }

    /// @ai-generated - Static class attribute does not report
    #[test]
    fn static_class_passes() {
        let template = TemplateAnalysisSnapshot {
            elements: vec![make_element_with_attrs(vec![TemplateAttribute {
                name: "class".to_string(),
                value: Some("container".to_string()),
                is_dynamic: false,
                span: Span::new(5, 25),
            }])],
            ..Default::default()
        };
        let diags = run_rule(&template);
        assert!(diags.is_empty());
    }

    /// @ai-generated - Dynamic non-class attribute does not report
    #[test]
    fn dynamic_non_class_passes() {
        let template = TemplateAnalysisSnapshot {
            elements: vec![make_element_with_attrs(vec![TemplateAttribute {
                name: "id".to_string(),
                value: Some("'my-id'".to_string()),
                is_dynamic: true,
                span: Span::new(5, 25),
            }])],
            ..Default::default()
        };
        let diags = run_rule(&template);
        assert!(diags.is_empty());
    }

    /// @ai-generated - Empty template produces no diagnostic
    #[test]
    fn empty_template_passes() {
        let template = TemplateAnalysisSnapshot::default();
        let diags = run_rule(&template);
        assert!(diags.is_empty());
    }
}
