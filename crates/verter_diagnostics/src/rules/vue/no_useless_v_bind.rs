//! Rule: no-useless-v-bind
//!
//! Disallow unnecessary `v-bind` with a string literal value.
//! `:prop="'literal'"` can be simplified to `prop="literal"`.

use crate::context::LintContext;
use crate::diagnostic::{DiagnosticSpanKind, Severity};
use crate::rules::{LintRule, RuleCategory};
use verter_analysis::template::{TemplateDirective, TemplateElement};

pub struct NoUselessVBind;

/// Check if expression is a simple string literal (`'...'` or `"..."`).
fn is_simple_string_literal(expr: &str) -> bool {
    let trimmed = expr.trim();
    if trimmed.len() < 2 {
        return false;
    }
    let bytes = trimmed.as_bytes();
    let first = bytes[0];
    let last = bytes[bytes.len() - 1];
    (first == b'\'' || first == b'"') && first == last
}

impl LintRule for NoUselessVBind {
    fn name(&self) -> &'static str {
        "no-useless-v-bind"
    }

    fn category(&self) -> RuleCategory {
        RuleCategory::VueRecommended
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
        if dir.name != "bind" {
            return;
        }
        if let Some(ref expr) = dir.expression {
            if is_simple_string_literal(expr) {
                let arg_display = dir
                    .argument
                    .as_deref()
                    .map(|a| format!("'{a}'"))
                    .unwrap_or_else(|| "prop".to_string());
                ctx.report_with_severity(
                    self.name(),
                    self.category().as_str(),
                    format!(
                        "Unnecessary 'v-bind' with a string literal value. Use {arg_display}=\"{}\" instead.",
                        expr.trim().trim_matches(|c| c == '\'' || c == '"')
                    ),
                    dir.span.start,
                    dir.span.end,
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

    use verter_analysis::template::*;
    use verter_span::Span;

    fn run(template: &TemplateAnalysisSnapshot) -> Vec<crate::diagnostic::LintDiagnostic> {
        crate::test_support::run_template_rule(NoUselessVBind, template)
    }

    #[test]
    fn v_bind_with_string_literal_reports() {
        let template = TemplateAnalysisSnapshot {
            elements: vec![TemplateElement {
                tag: "div".to_string(),
                directives: vec![TemplateDirective {
                    name: "bind".to_string(),
                    raw_name: ":title".to_string(),
                    argument: Some("title".to_string()),
                    modifiers: vec![],
                    expression: Some("'hello'".to_string()),
                    span: Span::new(5, 22),
                    name_end: 0,
                    arg_span: None,
                    expression_span: None,
                    modifier_spans: Vec::new(),
                }],
                span: Span::new(0, 30),
                tag_span_end: 25,
                content_end: 0,
                ..Default::default()
            }],
            ..Default::default()
        };
        let diags = run(&template);
        assert!(!diags.is_empty(), ":title=\"'hello'\" should trigger");
        assert!(diags.iter().any(|d| d.rule == "no-useless-v-bind"));
        assert!(
            diags[0].message.contains("title"),
            "message should include the attribute name"
        );
        assert!(
            !diags.iter().any(|d| d.rule == "valid-v-bind"),
            "must not trigger unrelated rule"
        );
    }

    #[test]
    fn v_bind_with_variable_passes() {
        let template = TemplateAnalysisSnapshot {
            elements: vec![TemplateElement {
                tag: "div".to_string(),
                directives: vec![TemplateDirective {
                    name: "bind".to_string(),
                    raw_name: ":title".to_string(),
                    argument: Some("title".to_string()),
                    modifiers: vec![],
                    expression: Some("title".to_string()),
                    span: Span::new(5, 18),
                    name_end: 0,
                    arg_span: None,
                    expression_span: None,
                    modifier_spans: Vec::new(),
                }],
                span: Span::new(0, 30),
                tag_span_end: 22,
                content_end: 0,
                ..Default::default()
            }],
            ..Default::default()
        };
        let diags = run(&template);
        assert!(diags.is_empty(), ":title=\"title\" should pass");
    }

    #[test]
    fn v_bind_with_number_passes() {
        let template = TemplateAnalysisSnapshot {
            elements: vec![TemplateElement {
                tag: "div".to_string(),
                directives: vec![TemplateDirective {
                    name: "bind".to_string(),
                    raw_name: ":max".to_string(),
                    argument: Some("max".to_string()),
                    modifiers: vec![],
                    expression: Some("100".to_string()),
                    span: Span::new(5, 15),
                    name_end: 0,
                    arg_span: None,
                    expression_span: None,
                    modifier_spans: Vec::new(),
                }],
                span: Span::new(0, 30),
                tag_span_end: 20,
                content_end: 0,
                ..Default::default()
            }],
            ..Default::default()
        };
        let diags = run(&template);
        assert!(
            diags.is_empty(),
            ":max=\"100\" should pass (number binding is useful)"
        );
    }
}
