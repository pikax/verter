//! Rule: no-deprecated-v-is
//!
//! The `v-is` directive was removed in Vue 3. Use `is="vue:component-name"`
//! instead. Detect usage of the `v-is` directive.

// @ai-generated

use crate::context::LintContext;
use crate::diagnostic::{DiagnosticSpanKind, DiagnosticTag, Severity};
use crate::rules::{LintRule, RuleCategory};
use verter_analysis::template::{TemplateDirective, TemplateElement};

pub struct NoDeprecatedVIs;

impl LintRule for NoDeprecatedVIs {
    fn name(&self) -> &'static str {
        "no-deprecated-v-is"
    }

    fn category(&self) -> RuleCategory {
        RuleCategory::VueEssential
    }

    fn default_severity(&self) -> Severity {
        Severity::Warning
    }

    fn check_directive(
        &self,
        dir: &TemplateDirective,
        _el: &TemplateElement,
        ctx: &mut LintContext,
    ) {
        if dir.name != "is" {
            return;
        }
        ctx.report_with_tags(
            self.name(),
            self.category().as_str(),
            "The 'v-is' directive has been removed in Vue 3. Use 'is=\"vue:component-name\"' instead.".to_string(),
            dir.span.start,
            dir.span.end,
            self.default_severity(),
            vec![DiagnosticTag::Deprecated],
            DiagnosticSpanKind::Directive,
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    use verter_analysis::template::*;
    use verter_span::Span;

    fn run(template: &TemplateAnalysisSnapshot) -> Vec<crate::diagnostic::LintDiagnostic> {
        crate::test_support::run_template_rule(NoDeprecatedVIs, template)
    }

    #[test]
    fn v_is_directive_reports() {
        let template = TemplateAnalysisSnapshot {
            elements: vec![TemplateElement {
                tag: "div".to_string(),
                directives: vec![TemplateDirective {
                    name: "is".to_string(),
                    raw_name: "v-is".to_string(),
                    argument: None,
                    modifiers: vec![],
                    expression: Some("'MyComponent'".to_string()),
                    span: Span::new(5, 25),
                    name_end: 0,
                    arg_span: None,
                    expression_span: None,
                    modifier_spans: Vec::new(),
                }],
                content_end: 0,
                ..Default::default()
            }],
            ..Default::default()
        };
        let diags = run(&template);
        assert!(!diags.is_empty(), "v-is directive should trigger");
        assert!(diags.iter().any(|d| d.rule == "no-deprecated-v-is"));
        assert!(
            diags[0].tags.contains(&DiagnosticTag::Deprecated),
            "should have Deprecated tag"
        );
        assert!(
            diags[0].message.contains("vue:"),
            "message should suggest vue: prefix"
        );
        assert!(
            !diags.iter().any(|d| d.rule == "no-v-html"),
            "must not trigger unrelated rule"
        );
    }

    #[test]
    fn v_bind_passes() {
        let template = TemplateAnalysisSnapshot {
            elements: vec![TemplateElement {
                tag: "div".to_string(),
                directives: vec![TemplateDirective {
                    name: "bind".to_string(),
                    raw_name: "v-bind:class".to_string(),
                    argument: Some("class".to_string()),
                    modifiers: vec![],
                    expression: Some("active".to_string()),
                    span: Span::new(5, 25),
                    name_end: 0,
                    arg_span: None,
                    expression_span: None,
                    modifier_spans: Vec::new(),
                }],
                content_end: 0,
                ..Default::default()
            }],
            ..Default::default()
        };
        let diags = run(&template);
        assert!(diags.is_empty(), "v-bind should pass");
    }
}
