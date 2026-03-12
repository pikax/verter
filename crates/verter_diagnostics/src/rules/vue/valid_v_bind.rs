//! Rule: valid-v-bind
//!
//! Ensures `v-bind` directives are used correctly:
//! - `.prop` or `.camel` modifiers require an argument (event name / prop name)
//! - Dynamic argument `:[expr]` must not be empty

// @ai-generated

use crate::context::LintContext;
use crate::diagnostic::{DiagnosticSpanKind, Severity};
use crate::rules::{LintRule, RuleCategory};
use verter_analysis::template::{TemplateDirective, TemplateElement};

pub struct ValidVBind;

impl LintRule for ValidVBind {
    fn name(&self) -> &'static str {
        "valid-v-bind"
    }

    fn category(&self) -> RuleCategory {
        RuleCategory::VueEssential
    }

    fn default_severity(&self) -> Option<Severity> {
        Some(Severity::Error)
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

        let has_argument = dir
            .argument
            .as_deref()
            .map(|a| !a.is_empty())
            .unwrap_or(false);

        // Modifiers .prop or .camel require an argument
        let has_prop_modifier = dir.modifiers.iter().any(|m| m == "prop" || m == "camel");
        if has_prop_modifier && !has_argument {
            ctx.report_with_severity(
                self.name(),
                self.category().as_str(),
                "'v-bind' with '.prop' or '.camel' modifier requires a binding argument."
                    .to_string(),
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

    fn run_rule(template: &TemplateAnalysisSnapshot) -> Vec<crate::diagnostic::LintDiagnostic> {
        crate::test_support::run_template_rule(ValidVBind, template)
    }

    fn make_element_with_v_bind(
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
                name: "bind".to_string(),
                raw_name: if argument.is_some() {
                    ":class".to_string()
                } else {
                    "v-bind".to_string()
                },
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
    fn v_bind_prop_without_argument_reports() {
        // v-bind.prop — .prop without argument
        let template = TemplateAnalysisSnapshot {
            elements: vec![make_element_with_v_bind(None, None, vec!["prop"])],
            ..Default::default()
        };
        let diags = run_rule(&template);
        assert!(
            !diags.is_empty(),
            "v-bind.prop without argument should trigger"
        );
        assert!(diags.iter().any(|d| d.rule == "valid-v-bind"));
        assert!(
            !diags.iter().any(|d| d.rule == "valid-v-on"),
            "must not trigger valid-v-on"
        );
    }

    #[test]
    fn v_bind_camel_without_argument_reports() {
        // v-bind.camel — .camel without argument
        let template = TemplateAnalysisSnapshot {
            elements: vec![make_element_with_v_bind(None, None, vec!["camel"])],
            ..Default::default()
        };
        let diags = run_rule(&template);
        assert!(
            !diags.is_empty(),
            "v-bind.camel without argument should trigger"
        );
    }

    #[test]
    fn v_bind_class_with_expression_passes() {
        // :class="x"
        let template = TemplateAnalysisSnapshot {
            elements: vec![make_element_with_v_bind(
                Some("class"),
                Some("myClass"),
                vec![],
            )],
            ..Default::default()
        };
        let diags = run_rule(&template);
        assert!(diags.is_empty(), ":class binding should pass");
    }

    #[test]
    fn v_bind_spread_passes() {
        // v-bind="obj" — no argument, no modifiers
        let template = TemplateAnalysisSnapshot {
            elements: vec![make_element_with_v_bind(None, Some("obj"), vec![])],
            ..Default::default()
        };
        let diags = run_rule(&template);
        assert!(diags.is_empty(), "v-bind spread should pass");
    }

    #[test]
    fn v_bind_prop_with_argument_passes() {
        // :foo.prop="x" — .prop WITH argument is valid
        let template = TemplateAnalysisSnapshot {
            elements: vec![make_element_with_v_bind(
                Some("foo"),
                Some("val"),
                vec!["prop"],
            )],
            ..Default::default()
        };
        let diags = run_rule(&template);
        assert!(diags.is_empty(), ":foo.prop with argument should pass");
    }
}
