//! Rule: valid-v-on
//!
//! Ensures `v-on` directives have at least a valid argument or expression.
//! A bare `v-on` with no argument and no expression is invalid.

// @ai-generated

use crate::context::LintContext;
use crate::diagnostic::{DiagnosticSpanKind, Severity};
use crate::rules::{LintRule, RuleCategory};
use verter_analysis::template::{TemplateDirective, TemplateElement};

pub struct ValidVOn;

impl LintRule for ValidVOn {
    fn name(&self) -> &'static str {
        "valid-v-on"
    }

    fn category(&self) -> RuleCategory {
        RuleCategory::VueEssential
    }

    fn default_severity(&self) -> Severity {
        Severity::Error
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

        // `v-on` with no argument (event name) AND no expression AND no modifiers is invalid.
        // Note: `@click.stop` without an expression is valid — modifiers alone work on native elements.
        let has_argument = dir
            .argument
            .as_deref()
            .map(|a| !a.is_empty())
            .unwrap_or(false);
        let has_expression = dir
            .expression
            .as_deref()
            .map(|e| !e.is_empty())
            .unwrap_or(false);
        let has_modifiers = !dir.modifiers.is_empty();

        if !has_argument && !has_expression && !has_modifiers {
            ctx.report_with_severity(
                self.name(),
                self.category().as_str(),
                "'v-on' directives require at least an event name or expression.".to_string(),
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
    use crate::config::LintConfig;
    use crate::visitor::LintVisitor;
    use verter_analysis::template::*;
    use verter_span::Span;

    fn run_rule(template: &TemplateAnalysisSnapshot) -> Vec<crate::diagnostic::LintDiagnostic> {
        let rules: Vec<Box<dyn LintRule>> = vec![Box::new(ValidVOn)];
        let visitor = LintVisitor::new(&rules);
        let config = LintConfig::default();
        let mut ctx = LintContext::new(&config);
        visitor.visit_template(template, &mut ctx);
        ctx.into_diagnostics()
    }

    fn make_element_with_v_on(
        argument: Option<&str>,
        expression: Option<&str>,
        modifiers: Vec<&str>,
    ) -> TemplateElement {
        TemplateElement {
            tag: "div".to_string(),
            is_component: false,
            is_self_closing: false,
            namespace: ElementNamespace::Html,
            attributes: vec![],
            directives: vec![TemplateDirective {
                name: "on".to_string(),
                raw_name: "v-on".to_string(),
                argument: argument.map(|s| s.to_string()),
                modifiers: modifiers.iter().map(|s| s.to_string()).collect(),
                expression: expression.map(|s| s.to_string()),
                span: Span::new(5, 20),
                name_end: 0,
                arg_span: None,
                expression_span: None,
                modifier_spans: Vec::new(),
            }],
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
            span: Span::new(0, 50),
            tag_span_end: 50,
            content_end: 0,
            ..Default::default()
        }
    }

    #[test]
    fn bare_v_on_no_arg_no_expr_reports() {
        let template = TemplateAnalysisSnapshot {
            elements: vec![make_element_with_v_on(None, None, vec![])],
            ..Default::default()
        };
        let diags = run_rule(&template);
        assert!(!diags.is_empty(), "bare v-on should trigger");
        assert!(diags.iter().any(|d| d.rule == "valid-v-on"));
        assert!(
            !diags.iter().any(|d| d.rule == "valid-v-if"),
            "must not trigger valid-v-if"
        );
    }

    #[test]
    fn v_on_with_argument_and_handler_passes() {
        // @click="fn"
        let template = TemplateAnalysisSnapshot {
            elements: vec![make_element_with_v_on(
                Some("click"),
                Some("handleClick"),
                vec![],
            )],
            ..Default::default()
        };
        let diags = run_rule(&template);
        assert!(
            diags.is_empty(),
            "v-on with argument + expression should pass"
        );
    }

    #[test]
    fn v_on_with_modifiers_only_passes() {
        // @click.stop is valid (modifier without expression on native element)
        let template = TemplateAnalysisSnapshot {
            elements: vec![make_element_with_v_on(Some("click"), None, vec!["stop"])],
            ..Default::default()
        };
        let diags = run_rule(&template);
        assert!(
            diags.is_empty(),
            "v-on with argument + modifier should pass"
        );
    }

    #[test]
    fn v_on_with_object_syntax_passes() {
        // v-on="{ click: fn }" — no argument but has expression
        let template = TemplateAnalysisSnapshot {
            elements: vec![make_element_with_v_on(None, Some("{ click: fn }"), vec![])],
            ..Default::default()
        };
        let diags = run_rule(&template);
        assert!(diags.is_empty(), "v-on object syntax should pass");
    }
}
