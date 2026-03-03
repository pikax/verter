//! Rule: no-deprecated-inline-template
//!
//! The `inline-template` attribute was removed in Vue 3.
//! Use `<slot>` or `<script>` setup for child content instead.
//! This is the Vue deprecation version (separate from the Vapor compatibility rule).

// @ai-generated

use crate::context::LintContext;
use crate::diagnostic::{DiagnosticSpanKind, DiagnosticTag, Severity};
use crate::rules::{LintRule, RuleCategory};
use verter_analysis::template::TemplateElement;

pub struct NoDeprecatedInlineTemplate;

impl LintRule for NoDeprecatedInlineTemplate {
    fn name(&self) -> &'static str {
        "no-deprecated-inline-template"
    }

    fn category(&self) -> RuleCategory {
        RuleCategory::VueEssential
    }

    fn default_severity(&self) -> Severity {
        Severity::Warning
    }

    fn check_element(&self, el: &TemplateElement, ctx: &mut LintContext) {
        for attr in &el.attributes {
            if attr.name == "inline-template" {
                ctx.report_with_tags(
                    self.name(),
                    self.category().as_str(),
                    "The 'inline-template' attribute has been removed in Vue 3. Use '<slot>' or script setup instead.".to_string(),
                    attr.span.start,
                    attr.span.end,
                    self.default_severity(),
                    vec![DiagnosticTag::Deprecated],
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
        let rules: Vec<Box<dyn LintRule>> = vec![Box::new(NoDeprecatedInlineTemplate)];
        let visitor = LintVisitor::new(&rules);
        let config = LintConfig::default();
        let mut ctx = LintContext::new(&config);
        visitor.visit_template(template, &mut ctx);
        ctx.into_diagnostics()
    }

    #[test]
    fn inline_template_reports() {
        let template = TemplateAnalysisSnapshot {
            elements: vec![TemplateElement {
                tag: "MyComp".to_string(),
                is_component: true,
                attributes: vec![TemplateAttribute {
                    name: "inline-template".to_string(),
                    value: None,
                    is_dynamic: false,
                    span: Span::new(8, 23),
                }],
                ..Default::default()
            }],
            ..Default::default()
        };
        let diags = run(&template);
        assert!(
            !diags.is_empty(),
            "inline-template attribute should trigger"
        );
        assert!(diags
            .iter()
            .any(|d| d.rule == "no-deprecated-inline-template"));
        assert!(
            diags[0].tags.contains(&DiagnosticTag::Deprecated),
            "should have Deprecated tag"
        );
        assert!(
            !diags.iter().any(|d| d.rule == "no-v-html"),
            "must not trigger unrelated rule"
        );
    }

    #[test]
    fn no_inline_template_passes() {
        let template = TemplateAnalysisSnapshot {
            elements: vec![TemplateElement {
                tag: "MyComp".to_string(),
                is_component: true,
                attributes: vec![TemplateAttribute {
                    name: "class".to_string(),
                    value: Some("foo".to_string()),
                    is_dynamic: false,
                    span: Span::new(8, 19),
                }],
                ..Default::default()
            }],
            ..Default::default()
        };
        let diags = run(&template);
        assert!(
            diags.is_empty(),
            "element without inline-template should pass"
        );
    }
}
