//! Rule: v-on-style
//!
//! Enforces the shorthand `@event` syntax instead of `v-on:event`.

// @ai-generated

use crate::context::LintContext;
use crate::diagnostic::{DiagnosticSpanKind, Severity};
use crate::rules::{LintRule, RuleCategory};
use verter_analysis::template::{TemplateDirective, TemplateElement};

pub struct VOnStyle;

impl LintRule for VOnStyle {
    fn name(&self) -> &'static str {
        "v-on-style"
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
        if dir.name != "on" {
            return;
        }

        // `v-on:click` should be written as `@click`
        // But `v-on="handlers"` (object syntax, no argument) is fine as-is
        if dir.raw_name.starts_with("v-on:") {
            let event = dir.raw_name.trim_start_matches("v-on:");
            ctx.report_with_severity(
                self.name(),
                self.category().as_str(),
                format!("Use '@{}' shorthand instead of '{}'.", event, dir.raw_name),
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
        let rules: Vec<Box<dyn LintRule>> = vec![Box::new(VOnStyle)];
        let visitor = LintVisitor::new(&rules);
        let config = LintConfig::default();
        let mut ctx = LintContext::new(&config);
        visitor.visit_template(template, &mut ctx);
        ctx.into_diagnostics()
    }

    fn make_el_with_on(raw_name: &str, argument: Option<&str>) -> TemplateElement {
        TemplateElement {
            tag: "button".to_string(),
            is_component: false,
            is_self_closing: false,
            namespace: ElementNamespace::Html,
            attributes: vec![],
            directives: vec![TemplateDirective {
                name: "on".to_string(),
                raw_name: raw_name.to_string(),
                argument: argument.map(|s| s.to_string()),
                modifiers: vec![],
                expression: Some("handleClick".to_string()),
                span: Span::new(8, 28),
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
    fn v_on_click_reports() {
        let template = TemplateAnalysisSnapshot {
            elements: vec![make_el_with_on("v-on:click", Some("click"))],
            ..Default::default()
        };
        let diags = run_rule(&template);
        assert!(!diags.is_empty(), "v-on:click should trigger v-on-style");
        assert!(diags.iter().any(|d| d.rule == "v-on-style"));
        assert!(
            diags[0].message.contains("@click"),
            "message should suggest @click"
        );
        assert!(
            !diags.iter().any(|d| d.rule == "v-bind-style"),
            "must not trigger v-bind-style"
        );
    }

    #[test]
    fn shorthand_click_passes() {
        let template = TemplateAnalysisSnapshot {
            elements: vec![make_el_with_on("@click", Some("click"))],
            ..Default::default()
        };
        let diags = run_rule(&template);
        assert!(diags.is_empty(), "@click shorthand should pass");
    }

    #[test]
    fn v_on_object_syntax_passes() {
        // v-on="{ click: fn }" — no specific event name
        let template = TemplateAnalysisSnapshot {
            elements: vec![make_el_with_on("v-on", None)],
            ..Default::default()
        };
        let diags = run_rule(&template);
        assert!(diags.is_empty(), "v-on object syntax should pass");
    }
}
