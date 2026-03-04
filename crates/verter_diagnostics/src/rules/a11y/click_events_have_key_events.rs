//! Rule: click-events-have-key-events
//!
//! Elements with a `@click` handler should also have a keyboard event handler
//! (`@keydown`, `@keyup`, or `@keypress`) to ensure keyboard accessibility.

use crate::context::LintContext;
use crate::diagnostic::{DiagnosticSpanKind, Severity};
use crate::rules::{LintRule, RuleCategory};
use verter_analysis::template::TemplateElement;

pub struct ClickEventsHaveKeyEvents;

impl LintRule for ClickEventsHaveKeyEvents {
    fn name(&self) -> &'static str {
        "click-events-have-key-events"
    }
    fn category(&self) -> RuleCategory {
        RuleCategory::Accessibility
    }
    fn default_severity(&self) -> Severity {
        Severity::Warning
    }
    fn check_element(&self, el: &TemplateElement, ctx: &mut LintContext) {
        let click_dir = el
            .directives
            .iter()
            .find(|d| d.name == "on" && d.argument.as_deref() == Some("click"));
        let click_dir = match click_dir {
            Some(d) => d,
            None => return,
        };
        let has_key_event = el.directives.iter().any(|d| {
            d.name == "on"
                && matches!(
                    d.argument.as_deref(),
                    Some("keydown") | Some("keyup") | Some("keypress")
                )
        });
        if !has_key_event {
            ctx.report_with_severity(
                self.name(),
                self.category().as_str(),
                "Elements with @click must also have a keyboard event handler (@keydown, @keyup, or @keypress).".to_string(),
                click_dir.span.start,
                click_dir.span.end,
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

    fn run(elements: Vec<TemplateElement>) -> Vec<crate::diagnostic::LintDiagnostic> {
        let rules: Vec<Box<dyn LintRule>> = vec![Box::new(ClickEventsHaveKeyEvents)];
        let visitor = LintVisitor::new(&rules);
        let config = LintConfig::default();
        let mut ctx = LintContext::new(&config);
        visitor.visit_template(
            &TemplateAnalysisSnapshot {
                elements,
                ..Default::default()
            },
            &mut ctx,
        );
        ctx.into_diagnostics()
    }

    fn dir(name: &str, argument: Option<&str>) -> TemplateDirective {
        TemplateDirective {
            name: name.to_string(),
            raw_name: format!("v-{}", name),
            argument: argument.map(|s| s.to_string()),
            modifiers: vec![],
            expression: None,
            span: Span::new(0, 10),
            name_end: 0,
            arg_span: None,
            expression_span: None,
            modifier_spans: Vec::new(),
        }
    }

    fn el_with_directives(directives: Vec<TemplateDirective>) -> TemplateElement {
        TemplateElement {
            tag: "div".to_string(),
            is_component: false,
            is_self_closing: false,
            namespace: ElementNamespace::Html,
            attributes: vec![],
            directives,
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
            span: Span::new(0, 30),
            tag_span_end: 30,
            content_end: 0,
            text_children: Vec::new(),
        }
    }

    #[test]
    fn click_without_key_event_reports() {
        let diags = run(vec![el_with_directives(vec![dir("on", Some("click"))])]);
        assert_eq!(diags.len(), 1);
    }

    #[test]
    fn click_with_keydown_passes() {
        assert!(run(vec![el_with_directives(vec![
            dir("on", Some("click")),
            dir("on", Some("keydown")),
        ])])
        .is_empty());
    }

    #[test]
    fn click_with_keyup_passes() {
        assert!(run(vec![el_with_directives(vec![
            dir("on", Some("click")),
            dir("on", Some("keyup")),
        ])])
        .is_empty());
    }

    #[test]
    fn no_click_passes() {
        assert!(run(vec![el_with_directives(vec![dir(
            "on",
            Some("mouseenter")
        )])])
        .is_empty());
    }
}
