//! Rule: valid-v-model
//!
//! Ensures `v-model` is used correctly:
//! - Empty expression is invalid
//! - `<input type="file">` with v-model is invalid
//! - Native elements must not use v-model with an argument (v-model:foo)

// @ai-generated

use crate::context::LintContext;
use crate::diagnostic::{DiagnosticSpanKind, Severity};
use crate::rules::{LintRule, RuleCategory};
use verter_analysis::template::TemplateElement;

pub struct ValidVModel;

impl LintRule for ValidVModel {
    fn name(&self) -> &'static str {
        "valid-v-model"
    }

    fn category(&self) -> RuleCategory {
        RuleCategory::VueEssential
    }

    fn default_severity(&self) -> Severity {
        Severity::Error
    }

    fn check_element(&self, el: &TemplateElement, ctx: &mut LintContext) {
        let Some(v_model) = &el.v_model else {
            return;
        };

        // v-model on <input type="file"> is not supported (file inputs are read-only)
        if el.tag.eq_ignore_ascii_case("input") {
            let is_file = el
                .attributes
                .iter()
                .any(|a| a.name == "type" && a.value.as_deref() == Some("file"));
            if is_file {
                ctx.report_with_severity(
                    self.name(),
                    self.category().as_str(),
                    "'v-model' cannot be used on file inputs. Use a change event listener instead."
                        .to_string(),
                    v_model.span.start,
                    v_model.span.end,
                    self.default_severity(),
                    DiagnosticSpanKind::Directive,
                );
                return;
            }
        }

        // Native elements cannot use v-model with a named argument (v-model:foo)
        if !el.is_component && v_model.binding_name != "modelValue" {
            ctx.report_with_severity(
                self.name(),
                self.category().as_str(),
                format!(
                    "'v-model:{}' with a named argument can only be used on components.",
                    v_model.binding_name
                ),
                v_model.span.start,
                v_model.span.end,
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
        let rules: Vec<Box<dyn LintRule>> = vec![Box::new(ValidVModel)];
        let visitor = LintVisitor::new(&rules);
        let config = LintConfig::default();
        let mut ctx = LintContext::new(&config);
        visitor.visit_template(template, &mut ctx);
        ctx.into_diagnostics()
    }

    fn make_input_with_v_model(input_type: Option<&str>) -> TemplateElement {
        let mut attrs = vec![];
        if let Some(t) = input_type {
            attrs.push(TemplateAttribute {
                name: "type".to_string(),
                value: Some(t.to_string()),
                is_dynamic: false,
                span: Span::new(7, 20),
                name_end: 0,
                value_span: None,
            });
        }
        TemplateElement {
            tag: "input".to_string(),
            is_component: false,
            is_self_closing: true,
            namespace: ElementNamespace::Html,
            attributes: attrs,
            directives: vec![],
            v_for: None,
            v_model: Some(VModelDirective {
                binding_name: "modelValue".to_string(),
                modifiers: vec![],
                target_is_component: false,
                target_tag: "input".to_string(),
                span: Span::new(20, 40),
            }),
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
    fn v_model_on_file_input_reports() {
        let template = TemplateAnalysisSnapshot {
            elements: vec![make_input_with_v_model(Some("file"))],
            ..Default::default()
        };
        let diags = run_rule(&template);
        assert!(!diags.is_empty(), "v-model on file input should trigger");
        assert!(diags.iter().any(|d| d.rule == "valid-v-model"));
        assert!(
            diags[0].message.contains("file"),
            "message should mention file input"
        );
        assert!(
            !diags.iter().any(|d| d.rule == "valid-v-if"),
            "must not trigger valid-v-if"
        );
    }

    #[test]
    fn v_model_on_text_input_passes() {
        let template = TemplateAnalysisSnapshot {
            elements: vec![make_input_with_v_model(Some("text"))],
            ..Default::default()
        };
        let diags = run_rule(&template);
        assert!(diags.is_empty(), "v-model on text input should pass");
    }

    #[test]
    fn v_model_on_input_no_type_passes() {
        let template = TemplateAnalysisSnapshot {
            elements: vec![make_input_with_v_model(None)],
            ..Default::default()
        };
        let diags = run_rule(&template);
        assert!(
            diags.is_empty(),
            "v-model on input without type should pass"
        );
    }

    #[test]
    fn v_model_with_argument_on_native_reports() {
        // <div v-model:foo="x"> — named argument on native element
        let element = TemplateElement {
            tag: "div".to_string(),
            is_component: false,
            is_self_closing: false,
            namespace: ElementNamespace::Html,
            attributes: vec![],
            directives: vec![],
            v_for: None,
            v_model: Some(VModelDirective {
                binding_name: "foo".to_string(),
                modifiers: vec![],
                target_is_component: false,
                target_tag: "div".to_string(),
                span: Span::new(5, 25),
            }),
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
        };
        let template = TemplateAnalysisSnapshot {
            elements: vec![element],
            ..Default::default()
        };
        let diags = run_rule(&template);
        assert!(
            !diags.is_empty(),
            "v-model:foo on native element should trigger"
        );
        assert!(
            diags[0].message.contains("components"),
            "message should mention components"
        );
    }

    #[test]
    fn v_model_with_argument_on_component_passes() {
        // <MyComp v-model:title="x"> — named argument on component is valid
        let element = TemplateElement {
            tag: "MyComp".to_string(),
            is_component: true,
            is_self_closing: false,
            namespace: ElementNamespace::Html,
            attributes: vec![],
            directives: vec![],
            v_for: None,
            v_model: Some(VModelDirective {
                binding_name: "title".to_string(),
                modifiers: vec![],
                target_is_component: true,
                target_tag: "MyComp".to_string(),
                span: Span::new(7, 30),
            }),
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
        };
        let template = TemplateAnalysisSnapshot {
            elements: vec![element],
            ..Default::default()
        };
        let diags = run_rule(&template);
        assert!(diags.is_empty(), "v-model:title on component should pass");
    }
}
