//! Rule: v-bind-style
//!
//! Enforces the shorthand `:prop` syntax instead of `v-bind:prop`.

// @ai-generated

use crate::context::LintContext;
use crate::diagnostic::{DiagnosticSpanKind, Severity};
use crate::rules::{LintRule, RuleCategory};
use verter_semantic::analysis::template::{TemplateDirective, TemplateElement};

pub struct VBindStyle;

impl LintRule for VBindStyle {
    fn name(&self) -> &'static str {
        "v-bind-style"
    }

    fn category(&self) -> RuleCategory {
        RuleCategory::VueRecommended
    }

    fn default_severity(&self) -> Option<Severity> {
        Some(Severity::Warning)
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

        // `v-bind:foo` should be written as `:foo`
        // But `v-bind="obj"` (object spread, no argument) is fine as-is
        if dir.raw_name.starts_with("v-bind:") {
            let short = dir.raw_name.trim_start_matches("v-bind:");
            ctx.report_with_severity(
                self.name(),
                self.category().as_str(),
                format!("Use ':{}' shorthand instead of '{}'.", short, dir.raw_name),
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

    use verter_semantic::analysis::template::*;
    use verter_span::Span;

    fn run_rule(template: &TemplateAnalysisSnapshot) -> Vec<crate::diagnostic::LintDiagnostic> {
        crate::test_support::run_template_rule(VBindStyle, template)
    }

    fn make_el_with_bind(raw_name: &str, argument: Option<&str>) -> TemplateElement {
        TemplateElement {
            tag: "div".to_string(),
            is_component: false,
            is_self_closing: false,
            namespace: ElementNamespace::Html,
            attributes: vec![],
            directives: vec![TemplateDirective {
                name: "bind".to_string(),
                raw_name: raw_name.to_string(),
                argument: argument.map(|s| s.to_string()),
                modifiers: vec![],
                expression: Some("val".to_string()),
                span: Span::new(5, 25),
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
    fn v_bind_class_reports() {
        let template = TemplateAnalysisSnapshot {
            elements: vec![make_el_with_bind("v-bind:class", Some("class"))],
            ..Default::default()
        };
        let diags = run_rule(&template);
        assert!(
            !diags.is_empty(),
            "v-bind:class should trigger v-bind-style"
        );
        assert!(diags.iter().any(|d| d.rule == "v-bind-style"));
        assert!(
            diags[0].message.contains(":class"),
            "message should suggest :class"
        );
        assert!(
            !diags.iter().any(|d| d.rule == "v-on-style"),
            "must not trigger v-on-style"
        );
    }

    #[test]
    fn shorthand_class_passes() {
        let template = TemplateAnalysisSnapshot {
            elements: vec![make_el_with_bind(":class", Some("class"))],
            ..Default::default()
        };
        let diags = run_rule(&template);
        assert!(diags.is_empty(), ":class shorthand should pass");
    }

    #[test]
    fn v_bind_spread_passes() {
        // v-bind="obj" — no argument, object spread
        let template = TemplateAnalysisSnapshot {
            elements: vec![make_el_with_bind("v-bind", None)],
            ..Default::default()
        };
        let diags = run_rule(&template);
        assert!(diags.is_empty(), "v-bind spread should pass (no argument)");
    }
}
