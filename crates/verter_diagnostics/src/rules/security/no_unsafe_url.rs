//! Rule: no-unsafe-url
//!
//! Disallows `javascript:` protocol in URL attributes (`href`, `src`, `action`,
//! `formaction`). These can be used for XSS attacks.

// @ai-generated

use crate::context::LintContext;
use crate::diagnostic::{DiagnosticSpanKind, Severity};
use crate::rules::{LintRule, RuleCategory};
use verter_analysis::template::TemplateElement;

const URL_ATTRS: &[&str] = &["href", "src", "action", "formaction"];

pub struct NoUnsafeUrl;

impl LintRule for NoUnsafeUrl {
    fn name(&self) -> &'static str {
        "no-unsafe-url"
    }

    fn category(&self) -> RuleCategory {
        RuleCategory::Security
    }

    fn default_severity(&self) -> Severity {
        Severity::Error
    }

    fn check_element(&self, el: &TemplateElement, ctx: &mut LintContext) {
        for attr in &el.attributes {
            if !URL_ATTRS.contains(&attr.name.as_str()) {
                continue;
            }
            // Only check static (non-dynamic) attributes — dynamic values can't be statically analyzed
            if attr.is_dynamic {
                continue;
            }
            let Some(value) = &attr.value else {
                continue;
            };
            if value
                .trim_start()
                .to_ascii_lowercase()
                .starts_with("javascript:")
            {
                ctx.report_with_severity(
                    self.name(),
                    self.category().as_str(),
                    format!(
                        "Attribute '{}' contains a 'javascript:' URL which may lead to XSS attacks.",
                        attr.name
                    ),
                    attr.span.start,
                    attr.span.end,
                    self.default_severity(),
                    DiagnosticSpanKind::Directive,
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

    fn run_rule(template: &TemplateAnalysisSnapshot) -> Vec<crate::diagnostic::LintDiagnostic> {
        let rules: Vec<Box<dyn LintRule>> = vec![Box::new(NoUnsafeUrl)];
        let visitor = LintVisitor::new(&rules);
        let config = LintConfig::default();
        let mut ctx = LintContext::new(&config);
        visitor.visit_template(template, &mut ctx);
        ctx.into_diagnostics()
    }

    fn make_anchor(href: &str, is_dynamic: bool) -> TemplateElement {
        TemplateElement {
            tag: "a".to_string(),
            is_component: false,
            is_self_closing: false,
            namespace: ElementNamespace::Html,
            attributes: vec![TemplateAttribute {
                name: "href".to_string(),
                value: Some(href.to_string()),
                is_dynamic,
                span: Span::new(3, 3 + href.len() as u32 + 7),
            }],
            directives: vec![],
            v_for: None,
            v_model: None,
            has_v_if: false,
            has_v_else: false,
            has_v_else_if: false,
            has_v_show: false,
            has_v_html: false,
            has_v_text: false,
            has_text_content: true,
            has_element_children: false,
            nesting_depth: 0,
            parent_tag: None,
            parent_index: None,
            dynamic_classes: vec![],
            span: Span::new(0, 50),
            tag_span_end: 50,
        }
    }

    #[test]
    fn javascript_href_reports() {
        let template = TemplateAnalysisSnapshot {
            elements: vec![make_anchor("javascript:void(0)", false)],
            ..Default::default()
        };
        let diags = run_rule(&template);
        assert!(!diags.is_empty(), "javascript: href should trigger");
        assert!(diags.iter().any(|d| d.rule == "no-unsafe-url"));
        assert!(
            diags[0].message.contains("href"),
            "message should mention href"
        );
        assert!(
            !diags.iter().any(|d| d.rule == "no-v-html"),
            "must not trigger no-v-html"
        );
    }

    #[test]
    fn javascript_href_case_insensitive_reports() {
        let template = TemplateAnalysisSnapshot {
            elements: vec![make_anchor("JavaScript:alert(1)", false)],
            ..Default::default()
        };
        let diags = run_rule(&template);
        assert!(!diags.is_empty(), "JavaScript: (mixed case) should trigger");
    }

    #[test]
    fn safe_href_passes() {
        let template = TemplateAnalysisSnapshot {
            elements: vec![make_anchor("/safe/path", false)],
            ..Default::default()
        };
        let diags = run_rule(&template);
        assert!(diags.is_empty(), "safe href should pass");
    }

    #[test]
    fn dynamic_href_not_checked() {
        // :href="expr" — dynamic, can't statically analyze
        let template = TemplateAnalysisSnapshot {
            elements: vec![make_anchor("javascript:void(0)", true)],
            ..Default::default()
        };
        let diags = run_rule(&template);
        assert!(
            diags.is_empty(),
            "dynamic :href must not be flagged by static analysis"
        );
    }

    #[test]
    fn hash_href_passes() {
        let template = TemplateAnalysisSnapshot {
            elements: vec![make_anchor("#section", false)],
            ..Default::default()
        };
        let diags = run_rule(&template);
        assert!(diags.is_empty(), "# href should pass");
    }
}
