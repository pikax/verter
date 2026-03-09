//! Rule: v-on-handler-style
//!
//! Prefer method handler over inline handler for `v-on` / `@` event bindings.
//! Inline handlers like `@click="doSomething()"` with parentheses suggest a
//! function call rather than a method reference like `@click="doSomething"`.

// @ai-generated

use crate::context::LintContext;
use crate::diagnostic::{DiagnosticSpanKind, Severity};
use crate::rules::{LintRule, RuleCategory};
use verter_analysis::template::{TemplateDirective, TemplateElement};

pub struct VOnHandlerStyle;

impl LintRule for VOnHandlerStyle {
    fn name(&self) -> &'static str {
        "v-on-handler-style"
    }

    fn category(&self) -> RuleCategory {
        RuleCategory::VueRecommended
    }

    fn default_severity(&self) -> Severity {
        Severity::Hint
    }

    fn check_directive(
        &self,
        dir: &TemplateDirective,
        _el: &TemplateElement,
        ctx: &mut LintContext,
    ) {
        if dir.name != "on" {
            return;
        }

        let expr = match &dir.expression {
            Some(e) => e.trim(),
            None => return,
        };

        // Skip multi-statement handlers (contain `;`) — those are intentionally inline
        if expr.contains(';') {
            return;
        }

        // Skip arrow functions (`=>`) — those are intentionally inline
        if expr.contains("=>") {
            return;
        }

        // Detect simple inline calls: expression ends with `)` and contains `(`
        // but is not a ternary or complex expression
        if expr.ends_with(')') && expr.contains('(') && !expr.contains('?') {
            ctx.report_with_severity(
                self.name(),
                self.category().as_str(),
                format!(
                    "Prefer method handler over inline handler. \
                     Use `{}` instead of `{}`.",
                    expr.split('(').next().unwrap_or(expr),
                    expr,
                ),
                dir.span.start,
                dir.span.end,
                self.default_severity(),
                DiagnosticSpanKind::Directive,
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    use verter_analysis::template::*;
    use verter_span::Span;

    fn run(template: &TemplateAnalysisSnapshot) -> Vec<crate::diagnostic::LintDiagnostic> {
        crate::test_support::run_template_rule(VOnHandlerStyle, template)
    }

    fn make_event_directive(expression: &str) -> TemplateElement {
        TemplateElement {
            tag: "button".to_string(),
            directives: vec![TemplateDirective {
                name: "on".to_string(),
                raw_name: "@click".to_string(),
                argument: Some("click".to_string()),
                modifiers: vec![],
                expression: Some(expression.to_string()),
                span: Span::new(8, 30),
                name_end: 0,
                arg_span: None,
                expression_span: None,
                modifier_spans: Vec::new(),
            }],
            content_end: 0,
            ..Default::default()
        }
    }

    #[test]
    fn inline_call_reports() {
        let template = TemplateAnalysisSnapshot {
            elements: vec![make_event_directive("doSomething()")],
            ..Default::default()
        };
        let diags = run(&template);
        assert!(!diags.is_empty(), "inline call should trigger");
        assert!(diags.iter().any(|d| d.rule == "v-on-handler-style"));
        assert!(
            diags[0].message.contains("doSomething"),
            "message should suggest method handler"
        );
        assert!(
            !diags.iter().any(|d| d.rule == "no-v-html"),
            "must not trigger unrelated rule"
        );
    }

    #[test]
    fn method_reference_passes() {
        let template = TemplateAnalysisSnapshot {
            elements: vec![make_event_directive("doSomething")],
            ..Default::default()
        };
        let diags = run(&template);
        assert!(diags.is_empty(), "method reference should pass");
    }

    #[test]
    fn arrow_function_passes() {
        let template = TemplateAnalysisSnapshot {
            elements: vec![make_event_directive("() => doSomething()")],
            ..Default::default()
        };
        let diags = run(&template);
        assert!(
            diags.is_empty(),
            "arrow function should pass (intentionally inline)"
        );
    }

    #[test]
    fn multi_statement_passes() {
        let template = TemplateAnalysisSnapshot {
            elements: vec![make_event_directive("a(); b()")],
            ..Default::default()
        };
        let diags = run(&template);
        assert!(diags.is_empty(), "multi-statement handler should pass");
    }

    #[test]
    fn ternary_passes() {
        let template = TemplateAnalysisSnapshot {
            elements: vec![make_event_directive("ok ? a() : b()")],
            ..Default::default()
        };
        let diags = run(&template);
        assert!(diags.is_empty(), "ternary expression should pass");
    }
}
