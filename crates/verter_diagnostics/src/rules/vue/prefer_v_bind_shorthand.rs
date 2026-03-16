//! Rule: prefer-v-bind-shorthand
//!
//! Warns when `:foo="foo"` can be replaced with `:foo` (Vue 3.4+ same-name
//! shorthand). Also matches kebab-case arguments against camelCase expressions
//! (e.g., `:foo-bar="fooBar"`).

use crate::context::LintContext;
use crate::diagnostic::{DiagnosticSpanKind, Severity};
use crate::rules::{LintRule, RuleCategory};
use verter_analysis::template::{TemplateDirective, TemplateElement};

pub struct PreferVBindShorthand;

/// Convert a kebab-case string to camelCase.
///
/// `"foo-bar"` → `"fooBar"`, `"id"` → `"id"`
fn kebab_to_camel(s: &str) -> String {
    let mut result = String::with_capacity(s.len());
    let mut capitalize_next = false;
    for ch in s.chars() {
        if ch == '-' {
            capitalize_next = true;
        } else if capitalize_next {
            result.extend(ch.to_uppercase());
            capitalize_next = false;
        } else {
            result.push(ch);
        }
    }
    result
}

impl LintRule for PreferVBindShorthand {
    fn name(&self) -> &'static str {
        "prefer-v-bind-shorthand"
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

        // Must have an argument (`:foo`). Spread `v-bind="obj"` has no argument.
        let Some(arg) = dir.argument.as_deref() else {
            return;
        };

        // Dynamic arguments like `:[key]` — skip
        if arg.starts_with('[') {
            return;
        }

        // Must have an expression (`:foo="expr"`). Already-shorthand `:foo` has
        // no expression.
        let Some(expr) = dir.expression.as_deref() else {
            return;
        };

        let expr_trimmed = expr.trim();
        let arg_camel = kebab_to_camel(arg);

        if expr_trimmed == arg_camel {
            ctx.report_with_severity(
                self.name(),
                self.category().as_str(),
                format!(
                    "Use same-name shorthand `:{}` instead of `:{}=\"{}\"`.",
                    arg, arg, expr_trimmed
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

    fn run_rule(template: &TemplateAnalysisSnapshot) -> Vec<crate::diagnostic::LintDiagnostic> {
        crate::test_support::run_template_rule(PreferVBindShorthand, template)
    }

    fn make_el(directives: Vec<TemplateDirective>) -> TemplateElement {
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
            span: Span::new(0, 50),
            tag_span_end: 50,
            content_end: 0,
            ..Default::default()
        }
    }

    fn bind_dir(
        raw_name: &str,
        argument: Option<&str>,
        expression: Option<&str>,
        span: Span,
    ) -> TemplateDirective {
        TemplateDirective {
            name: "bind".to_string(),
            raw_name: raw_name.to_string(),
            argument: argument.map(|s| s.to_string()),
            modifiers: vec![],
            expression: expression.map(|s| s.to_string()),
            span,
            name_end: 0,
            arg_span: None,
            expression_span: None,
            modifier_spans: Vec::new(),
        }
    }

    fn on_dir(raw_name: &str, argument: &str, expression: &str, span: Span) -> TemplateDirective {
        TemplateDirective {
            name: "on".to_string(),
            raw_name: raw_name.to_string(),
            argument: Some(argument.to_string()),
            modifiers: vec![],
            expression: Some(expression.to_string()),
            span,
            name_end: 0,
            arg_span: None,
            expression_span: None,
            modifier_spans: Vec::new(),
        }
    }

    // ── Detection tests ──

    #[test]
    fn shorthand_candidate_reports() {
        // :foo="foo" → warning
        let template = TemplateAnalysisSnapshot {
            elements: vec![make_el(vec![bind_dir(
                ":foo",
                Some("foo"),
                Some("foo"),
                Span::new(5, 15),
            )])],
            ..Default::default()
        };
        let diags = run_rule(&template);
        assert_eq!(diags.len(), 1, "should report one warning");
        assert_eq!(diags[0].rule, "prefer-v-bind-shorthand");
        assert!(
            diags[0].message.contains("`:foo`"),
            "message should suggest shorthand: {}",
            diags[0].message
        );
        assert!(!diags[0].message.contains("v-on"), "must not mention v-on");
    }

    #[test]
    fn shorthand_kebab_camel_reports() {
        // :foo-bar="fooBar" → warning
        let template = TemplateAnalysisSnapshot {
            elements: vec![make_el(vec![bind_dir(
                ":foo-bar",
                Some("foo-bar"),
                Some("fooBar"),
                Span::new(5, 24),
            )])],
            ..Default::default()
        };
        let diags = run_rule(&template);
        assert_eq!(diags.len(), 1, "kebab-camel match should trigger");
        assert!(diags[0].message.contains("`:foo-bar`"));
    }

    #[test]
    fn shorthand_single_word_same() {
        // :id="id" → warning
        let template = TemplateAnalysisSnapshot {
            elements: vec![make_el(vec![bind_dir(
                ":id",
                Some("id"),
                Some("id"),
                Span::new(5, 13),
            )])],
            ..Default::default()
        };
        let diags = run_rule(&template);
        assert_eq!(diags.len(), 1);
    }

    #[test]
    fn shorthand_multiple_directives_one_match() {
        // :foo="foo" :bar="baz" → exactly 1 warning on :foo
        let template = TemplateAnalysisSnapshot {
            elements: vec![make_el(vec![
                bind_dir(":foo", Some("foo"), Some("foo"), Span::new(5, 15)),
                bind_dir(":bar", Some("bar"), Some("baz"), Span::new(16, 26)),
            ])],
            ..Default::default()
        };
        let diags = run_rule(&template);
        assert_eq!(diags.len(), 1, "only :foo should trigger");
        assert!(diags[0].message.contains("`:foo`"));
    }

    // ── No false positives ──

    #[test]
    fn different_value_passes() {
        // :foo="bar" → no warning
        let template = TemplateAnalysisSnapshot {
            elements: vec![make_el(vec![bind_dir(
                ":foo",
                Some("foo"),
                Some("bar"),
                Span::new(5, 15),
            )])],
            ..Default::default()
        };
        let diags = run_rule(&template);
        assert!(diags.is_empty(), ":foo=\"bar\" must not trigger");
    }

    #[test]
    fn no_value_passes() {
        // :foo (already shorthand, no expression) → no warning
        let template = TemplateAnalysisSnapshot {
            elements: vec![make_el(vec![bind_dir(
                ":foo",
                Some("foo"),
                None,
                Span::new(5, 9),
            )])],
            ..Default::default()
        };
        let diags = run_rule(&template);
        assert!(diags.is_empty(), "already-shorthand :foo must not trigger");
    }

    #[test]
    fn v_bind_spread_passes() {
        // v-bind="obj" → no warning
        let template = TemplateAnalysisSnapshot {
            elements: vec![make_el(vec![bind_dir(
                "v-bind",
                None,
                Some("obj"),
                Span::new(5, 17),
            )])],
            ..Default::default()
        };
        let diags = run_rule(&template);
        assert!(diags.is_empty(), "v-bind spread must not trigger");
    }

    #[test]
    fn dynamic_arg_passes() {
        // :[key]="val" → no warning
        let template = TemplateAnalysisSnapshot {
            elements: vec![make_el(vec![bind_dir(
                ":[key]",
                Some("[key]"),
                Some("val"),
                Span::new(5, 17),
            )])],
            ..Default::default()
        };
        let diags = run_rule(&template);
        assert!(diags.is_empty(), "dynamic argument must not trigger");
    }

    #[test]
    fn expression_with_member_access() {
        // :foo="obj.foo" → no warning
        let template = TemplateAnalysisSnapshot {
            elements: vec![make_el(vec![bind_dir(
                ":foo",
                Some("foo"),
                Some("obj.foo"),
                Span::new(5, 19),
            )])],
            ..Default::default()
        };
        let diags = run_rule(&template);
        assert!(diags.is_empty(), "member access must not trigger");
    }

    #[test]
    fn expression_with_function_call() {
        // :foo="getFoo()" → no warning
        let template = TemplateAnalysisSnapshot {
            elements: vec![make_el(vec![bind_dir(
                ":foo",
                Some("foo"),
                Some("getFoo()"),
                Span::new(5, 20),
            )])],
            ..Default::default()
        };
        let diags = run_rule(&template);
        assert!(diags.is_empty(), "function call must not trigger");
    }

    #[test]
    fn expression_with_extra_whitespace() {
        // :foo=" foo " → warning (trimmed match)
        let template = TemplateAnalysisSnapshot {
            elements: vec![make_el(vec![bind_dir(
                ":foo",
                Some("foo"),
                Some(" foo "),
                Span::new(5, 17),
            )])],
            ..Default::default()
        };
        let diags = run_rule(&template);
        assert_eq!(diags.len(), 1, "whitespace-trimmed match should trigger");
    }

    #[test]
    fn v_on_not_affected() {
        // @click="click" → no warning (only v-bind)
        let template = TemplateAnalysisSnapshot {
            elements: vec![make_el(vec![on_dir(
                "@click",
                "click",
                "click",
                Span::new(5, 20),
            )])],
            ..Default::default()
        };
        let diags = run_rule(&template);
        assert!(diags.is_empty(), "v-on must not trigger v-bind rule");
    }

    #[test]
    fn component_prop_same_name() {
        // :title="title" on component → warning (works on both elements and components)
        let mut el = make_el(vec![bind_dir(
            ":title",
            Some("title"),
            Some("title"),
            Span::new(5, 20),
        )]);
        el.is_component = true;
        el.tag = "MyComp".to_string();
        let template = TemplateAnalysisSnapshot {
            elements: vec![el],
            ..Default::default()
        };
        let diags = run_rule(&template);
        assert_eq!(diags.len(), 1, "component prop same-name should trigger");
    }

    // ── Helper tests ──

    #[test]
    fn kebab_to_camel_works() {
        assert_eq!(kebab_to_camel("foo"), "foo");
        assert_eq!(kebab_to_camel("foo-bar"), "fooBar");
        assert_eq!(kebab_to_camel("foo-bar-baz"), "fooBarBaz");
        assert_eq!(kebab_to_camel("id"), "id");
    }
}
