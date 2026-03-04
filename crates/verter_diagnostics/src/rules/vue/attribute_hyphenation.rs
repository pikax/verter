//! Rule: attribute-hyphenation
//!
//! Enforces kebab-case for non-dynamic attributes on component elements.
//! For example, `<MyComp myProp="val">` should be `<MyComp my-prop="val">`.
//! Only applies to component elements, not native HTML elements.

// @ai-generated

use crate::context::LintContext;
use crate::diagnostic::{DiagnosticSpanKind, Severity};
use crate::rules::{LintRule, RuleCategory};
use verter_analysis::template::TemplateElement;

pub struct AttributeHyphenation;

/// Returns true if a string contains uppercase letters (i.e., is camelCase or PascalCase).
fn has_uppercase(s: &str) -> bool {
    s.chars().any(|c| c.is_ascii_uppercase())
}

/// Converts a camelCase string to kebab-case.
fn to_kebab_case(s: &str) -> String {
    let mut result = String::with_capacity(s.len() + 4);
    for (i, c) in s.chars().enumerate() {
        if c.is_ascii_uppercase() {
            if i > 0 {
                result.push('-');
            }
            result.push(c.to_ascii_lowercase());
        } else {
            result.push(c);
        }
    }
    result
}

impl LintRule for AttributeHyphenation {
    fn name(&self) -> &'static str {
        "attribute-hyphenation"
    }

    fn category(&self) -> RuleCategory {
        RuleCategory::VueRecommended
    }

    fn default_severity(&self) -> Severity {
        Severity::Warning
    }

    fn check_element(&self, el: &TemplateElement, ctx: &mut LintContext) {
        if !el.is_component {
            return;
        }

        for attr in &el.attributes {
            // Skip dynamic attributes (`:prop`, `v-bind:prop`)
            if attr.is_dynamic {
                continue;
            }
            // Skip data-* and aria-* attributes (they're always kebab-case in HTML)
            if attr.name.starts_with("data-") || attr.name.starts_with("aria-") {
                continue;
            }
            // Check for camelCase attribute names
            if has_uppercase(&attr.name) {
                let kebab = to_kebab_case(&attr.name);
                ctx.report_with_severity(
                    self.name(),
                    self.category().as_str(),
                    format!(
                        "Attribute '{}' should be '{}'. Use kebab-case for attributes on components.",
                        attr.name, kebab
                    ),
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
        let rules: Vec<Box<dyn LintRule>> = vec![Box::new(AttributeHyphenation)];
        let visitor = LintVisitor::new(&rules);
        let config = LintConfig::default();
        let mut ctx = LintContext::new(&config);
        visitor.visit_template(template, &mut ctx);
        ctx.into_diagnostics()
    }

    fn make_component_with_attrs(attrs: Vec<TemplateAttribute>) -> TemplateElement {
        TemplateElement {
            tag: "MyComp".to_string(),
            is_component: true,
            attributes: attrs,
            span: Span::new(0, 50),
            tag_span_end: 45,
            content_end: 0,
            ..Default::default()
        }
    }

    #[test]
    fn camel_case_attr_on_component_reports() {
        let template = TemplateAnalysisSnapshot {
            elements: vec![make_component_with_attrs(vec![TemplateAttribute {
                name: "myProp".to_string(),
                value: Some("val".to_string()),
                is_dynamic: false,
                span: Span::new(8, 20),
                name_end: 0,
                value_span: None,
            }])],
            ..Default::default()
        };
        let diags = run(&template);
        assert!(
            !diags.is_empty(),
            "camelCase attribute on component should trigger"
        );
        assert!(diags.iter().any(|d| d.rule == "attribute-hyphenation"));
        assert!(
            diags[0].message.contains("my-prop"),
            "message should suggest kebab-case"
        );
        assert!(
            !diags.iter().any(|d| d.rule == "no-v-html"),
            "must not trigger unrelated rule"
        );
    }

    #[test]
    fn kebab_case_attr_passes() {
        let template = TemplateAnalysisSnapshot {
            elements: vec![make_component_with_attrs(vec![TemplateAttribute {
                name: "my-prop".to_string(),
                value: Some("val".to_string()),
                is_dynamic: false,
                span: Span::new(8, 22),
                name_end: 0,
                value_span: None,
            }])],
            ..Default::default()
        };
        let diags = run(&template);
        assert!(diags.is_empty(), "kebab-case attribute should pass");
    }

    #[test]
    fn dynamic_camel_case_passes() {
        let template = TemplateAnalysisSnapshot {
            elements: vec![make_component_with_attrs(vec![TemplateAttribute {
                name: "myProp".to_string(),
                value: Some("val".to_string()),
                is_dynamic: true,
                span: Span::new(8, 22),
                name_end: 0,
                value_span: None,
            }])],
            ..Default::default()
        };
        let diags = run(&template);
        assert!(diags.is_empty(), "dynamic camelCase attribute should pass");
    }

    #[test]
    fn native_element_camel_case_passes() {
        let template = TemplateAnalysisSnapshot {
            elements: vec![TemplateElement {
                tag: "div".to_string(),
                is_component: false,
                attributes: vec![TemplateAttribute {
                    name: "myAttr".to_string(),
                    value: Some("val".to_string()),
                    is_dynamic: false,
                    span: Span::new(5, 18),
                    name_end: 0,
                    value_span: None,
                }],
                span: Span::new(0, 30),
                tag_span_end: 25,
                content_end: 0,
                ..Default::default()
            }],
            ..Default::default()
        };
        let diags = run(&template);
        assert!(
            diags.is_empty(),
            "camelCase on native element should not trigger"
        );
    }

    #[test]
    fn data_attr_passes() {
        let template = TemplateAnalysisSnapshot {
            elements: vec![make_component_with_attrs(vec![TemplateAttribute {
                name: "data-testId".to_string(),
                value: Some("val".to_string()),
                is_dynamic: false,
                span: Span::new(8, 26),
                name_end: 0,
                value_span: None,
            }])],
            ..Default::default()
        };
        let diags = run(&template);
        assert!(diags.is_empty(), "data-* attribute should pass");
    }
}
