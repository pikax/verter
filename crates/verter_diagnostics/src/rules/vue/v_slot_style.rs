//! Rule: v-slot-style
//!
//! Enforces the shorthand `#slot` syntax instead of `v-slot:slot`.

// @ai-generated

use crate::context::LintContext;
use crate::diagnostic::{DiagnosticSpanKind, Severity};
use crate::rules::{LintRule, RuleCategory};
use verter_analysis::template::{TemplateDirective, TemplateElement};

pub struct VSlotStyle;

impl LintRule for VSlotStyle {
    fn name(&self) -> &'static str {
        "v-slot-style"
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
        if dir.name != "slot" {
            return;
        }

        // `v-slot:header` should be written as `#header`
        if dir.raw_name.starts_with("v-slot:") {
            let slot_name = dir.raw_name.trim_start_matches("v-slot:");
            ctx.report_with_severity(
                self.name(),
                self.category().as_str(),
                format!(
                    "Use '#{slot_name}' shorthand instead of '{}'.",
                    dir.raw_name
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
    use crate::config::LintConfig;
    use crate::visitor::LintVisitor;
    use verter_analysis::template::*;
    use verter_span::Span;

    fn run_rule(template: &TemplateAnalysisSnapshot) -> Vec<crate::diagnostic::LintDiagnostic> {
        let rules: Vec<Box<dyn LintRule>> = vec![Box::new(VSlotStyle)];
        let visitor = LintVisitor::new(&rules);
        let config = LintConfig::default();
        let mut ctx = LintContext::new(&config);
        visitor.visit_template(template, &mut ctx);
        ctx.into_diagnostics()
    }

    fn make_template_with_slot(raw_name: &str) -> TemplateElement {
        TemplateElement {
            tag: "template".to_string(),
            is_component: false,
            is_self_closing: false,
            namespace: ElementNamespace::Html,
            attributes: vec![],
            directives: vec![TemplateDirective {
                name: "slot".to_string(),
                raw_name: raw_name.to_string(),
                argument: Some("header".to_string()),
                modifiers: vec![],
                expression: None,
                span: Span::new(10, 30),
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
            nesting_depth: 1,
            parent_tag: Some("MyComp".to_string()),
            parent_index: None,
            dynamic_classes: vec![],
            span: Span::new(0, 50),
            tag_span_end: 50,
            content_end: 0,
            ..Default::default()
        }
    }

    #[test]
    fn v_slot_longform_reports() {
        let template = TemplateAnalysisSnapshot {
            elements: vec![make_template_with_slot("v-slot:header")],
            ..Default::default()
        };
        let diags = run_rule(&template);
        assert!(
            !diags.is_empty(),
            "v-slot:header should trigger v-slot-style"
        );
        assert!(diags.iter().any(|d| d.rule == "v-slot-style"));
        assert!(
            diags[0].message.contains("#header"),
            "message should suggest #header"
        );
        assert!(
            !diags.iter().any(|d| d.rule == "v-bind-style"),
            "must not trigger v-bind-style"
        );
    }

    #[test]
    fn shorthand_slot_passes() {
        let template = TemplateAnalysisSnapshot {
            elements: vec![make_template_with_slot("#header")],
            ..Default::default()
        };
        let diags = run_rule(&template);
        assert!(diags.is_empty(), "#header shorthand should pass");
    }

    #[test]
    fn bare_v_slot_passes() {
        // v-slot (default slot, no argument)
        let template = TemplateAnalysisSnapshot {
            elements: vec![make_template_with_slot("v-slot")],
            ..Default::default()
        };
        let diags = run_rule(&template);
        assert!(
            diags.is_empty(),
            "bare v-slot should pass (no named argument to shorten)"
        );
    }
}
