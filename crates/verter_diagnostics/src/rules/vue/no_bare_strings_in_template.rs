//! Rule: no-bare-strings-in-template
//!
//! Disallow raw text content in template elements for i18n readiness.
//! Elements with `has_text_content` flag suggest hardcoded strings that should
//! be wrapped in a translation function like `$t()` or `t()`.

// @ai-generated

use crate::context::LintContext;
use crate::diagnostic::{DiagnosticSpanKind, Severity};
use crate::rules::{LintRule, RuleCategory};
use verter_analysis::template::TemplateAnalysisSnapshot;

pub struct NoBareStringsInTemplate;

impl LintRule for NoBareStringsInTemplate {
    fn name(&self) -> &'static str {
        "no-bare-strings-in-template"
    }

    fn category(&self) -> RuleCategory {
        RuleCategory::VueRecommended
    }

    fn default_severity(&self) -> Severity {
        // This is a very noisy rule — default to Hint
        Severity::Hint
    }

    fn check_template(&self, tpl: &TemplateAnalysisSnapshot, ctx: &mut LintContext) {
        for el in &tpl.elements {
            if !el.has_bare_text {
                continue;
            }

            // Skip elements that typically have raw text (script, style, pre, code)
            let skip_tags = ["script", "style", "pre", "code", "textarea"];
            if skip_tags.contains(&el.tag.as_str()) {
                continue;
            }

            ctx.report_with_severity(
                self.name(),
                self.category().as_str(),
                format!(
                    "Raw text content in <{}>. Consider using a translation function \
                     (e.g., `$t()`) for i18n support.",
                    el.tag
                ),
                el.tag_span_end,
                el.content_end,
                self.default_severity(),
                DiagnosticSpanKind::ElementContent,
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

    fn run(template: &TemplateAnalysisSnapshot) -> Vec<crate::diagnostic::LintDiagnostic> {
        let rules: Vec<Box<dyn LintRule>> = vec![Box::new(NoBareStringsInTemplate)];
        let visitor = LintVisitor::new(&rules);
        let config = LintConfig::default();
        let mut ctx = LintContext::new(&config);
        visitor.visit_template(template, &mut ctx);
        ctx.into_diagnostics()
    }

    #[test]
    fn bare_string_reports() {
        let template = TemplateAnalysisSnapshot {
            elements: vec![TemplateElement {
                tag: "p".to_string(),
                has_bare_text: true,
                has_text_content: true,
                span: Span::new(0, 30),
                tag_span_end: 3,
                content_end: 25,
                text_children: Vec::new(),
                ..Default::default()
            }],
            ..Default::default()
        };
        let diags = run(&template);
        assert!(!diags.is_empty(), "bare string should trigger");
        assert!(diags
            .iter()
            .any(|d| d.rule == "no-bare-strings-in-template"));
        assert!(
            diags[0].message.contains("translation"),
            "message should suggest translation"
        );
        // Span should cover the content area, not the open tag
        assert_eq!(diags[0].span.start, 3, "span start should be tag_span_end");
        assert_eq!(diags[0].span.end, 25, "span end should be content_end");
        assert_eq!(
            diags[0].span_kind,
            DiagnosticSpanKind::ElementContent,
            "span kind should be ElementContent"
        );
        assert!(
            !diags.iter().any(|d| d.rule == "no-v-html"),
            "must not trigger unrelated rule"
        );
    }

    #[test]
    fn interpolation_only_passes() {
        // {{ message }} is NOT a bare string — should not trigger
        let template = TemplateAnalysisSnapshot {
            elements: vec![TemplateElement {
                tag: "div".to_string(),
                has_text_content: true,
                has_bare_text: false,
                span: Span::new(0, 40),
                tag_span_end: 5,
                content_end: 0,
                text_children: Vec::new(),
                ..Default::default()
            }],
            ..Default::default()
        };
        let diags = run(&template);
        assert!(
            diags.is_empty(),
            "interpolation-only content should NOT trigger no-bare-strings: {:?}",
            diags
        );
    }

    #[test]
    fn no_text_content_passes() {
        let template = TemplateAnalysisSnapshot {
            elements: vec![TemplateElement {
                tag: "div".to_string(),
                has_text_content: false,
                span: Span::new(0, 20),
                tag_span_end: 5,
                content_end: 0,
                text_children: Vec::new(),
                ..Default::default()
            }],
            ..Default::default()
        };
        let diags = run(&template);
        assert!(diags.is_empty(), "element without text content should pass");
    }

    #[test]
    fn pre_tag_passes() {
        let template = TemplateAnalysisSnapshot {
            elements: vec![TemplateElement {
                tag: "pre".to_string(),
                has_text_content: true,
                span: Span::new(0, 30),
                tag_span_end: 5,
                content_end: 0,
                text_children: Vec::new(),
                ..Default::default()
            }],
            ..Default::default()
        };
        let diags = run(&template);
        assert!(diags.is_empty(), "<pre> with text should be skipped");
    }

    #[test]
    fn code_tag_passes() {
        let template = TemplateAnalysisSnapshot {
            elements: vec![TemplateElement {
                tag: "code".to_string(),
                has_text_content: true,
                span: Span::new(0, 30),
                tag_span_end: 6,
                content_end: 0,
                text_children: Vec::new(),
                ..Default::default()
            }],
            ..Default::default()
        };
        let diags = run(&template);
        assert!(diags.is_empty(), "<code> with text should be skipped");
    }
}
