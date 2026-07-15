//! Rule: no-deprecated-router-link-tag-prop
//!
//! The `tag` prop on `<router-link>` was removed in Vue Router 4.
//! Use scoped slots and a custom component instead.
//! Detect `tag` attribute on `<router-link>` or `<RouterLink>`.

// @ai-generated

use crate::context::LintContext;
use crate::diagnostic::{DiagnosticSpanKind, DiagnosticTag, Severity};
use crate::rules::{LintRule, RuleCategory};
use verter_semantic::analysis::template::TemplateElement;

pub struct NoDeprecatedRouterLinkTagProp;

impl LintRule for NoDeprecatedRouterLinkTagProp {
    fn name(&self) -> &'static str {
        "no-deprecated-router-link-tag-prop"
    }

    fn category(&self) -> RuleCategory {
        RuleCategory::VueEssential
    }

    fn default_severity(&self) -> Option<Severity> {
        Some(Severity::Warning)
    }

    fn check_element(&self, el: &TemplateElement, ctx: &mut LintContext) {
        if el.tag != "router-link" && el.tag != "RouterLink" {
            return;
        }

        for attr in &el.attributes {
            if attr.name == "tag" {
                ctx.report_with_tags(
                    self.name(),
                    self.category().as_str(),
                    "The 'tag' prop on '<router-link>' has been removed in Vue Router 4. Use scoped slots instead.".to_string(),
                    attr.span.start,
                    attr.span.end,
                    self.default_severity(),
                    vec![DiagnosticTag::Deprecated],
                    DiagnosticSpanKind::Attribute,
                );
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    use verter_semantic::analysis::template::*;
    use verter_span::Span;

    fn run(template: &TemplateAnalysisSnapshot) -> Vec<crate::diagnostic::LintDiagnostic> {
        crate::test_support::run_template_rule(NoDeprecatedRouterLinkTagProp, template)
    }

    #[test]
    fn router_link_tag_prop_reports() {
        let template = TemplateAnalysisSnapshot {
            elements: vec![TemplateElement {
                tag: "router-link".to_string(),
                is_component: true,
                attributes: vec![
                    TemplateAttribute {
                        name: "to".to_string(),
                        value: Some("/home".to_string()),
                        is_dynamic: false,
                        span: Span::new(13, 23),
                        name_end: 0,
                        value_span: None,
                    },
                    TemplateAttribute {
                        name: "tag".to_string(),
                        value: Some("button".to_string()),
                        is_dynamic: false,
                        span: Span::new(24, 36),
                        name_end: 0,
                        value_span: None,
                    },
                ],
                content_end: 0,
                ..Default::default()
            }],
            ..Default::default()
        };
        let diags = run(&template);
        assert!(
            !diags.is_empty(),
            "tag prop on <router-link> should trigger"
        );
        assert!(diags
            .iter()
            .any(|d| d.rule == "no-deprecated-router-link-tag-prop"));
        assert!(
            diags[0].tags.contains(&DiagnosticTag::Deprecated),
            "should have Deprecated tag"
        );
        assert!(
            !diags.iter().any(|d| d.rule == "no-v-html"),
            "must not trigger unrelated rule"
        );
    }

    #[test]
    fn router_link_pascal_case_tag_prop_reports() {
        let template = TemplateAnalysisSnapshot {
            elements: vec![TemplateElement {
                tag: "RouterLink".to_string(),
                is_component: true,
                attributes: vec![TemplateAttribute {
                    name: "tag".to_string(),
                    value: Some("li".to_string()),
                    is_dynamic: false,
                    span: Span::new(12, 20),
                    name_end: 0,
                    value_span: None,
                }],
                content_end: 0,
                ..Default::default()
            }],
            ..Default::default()
        };
        let diags = run(&template);
        assert!(!diags.is_empty(), "tag prop on <RouterLink> should trigger");
    }

    #[test]
    fn router_link_without_tag_passes() {
        let template = TemplateAnalysisSnapshot {
            elements: vec![TemplateElement {
                tag: "router-link".to_string(),
                is_component: true,
                attributes: vec![TemplateAttribute {
                    name: "to".to_string(),
                    value: Some("/home".to_string()),
                    is_dynamic: false,
                    span: Span::new(13, 23),
                    name_end: 0,
                    value_span: None,
                }],
                content_end: 0,
                ..Default::default()
            }],
            ..Default::default()
        };
        let diags = run(&template);
        assert!(diags.is_empty(), "router-link without tag prop should pass");
    }

    #[test]
    fn non_router_tag_attr_passes() {
        let template = TemplateAnalysisSnapshot {
            elements: vec![TemplateElement {
                tag: "MyComp".to_string(),
                is_component: true,
                attributes: vec![TemplateAttribute {
                    name: "tag".to_string(),
                    value: Some("div".to_string()),
                    is_dynamic: false,
                    span: Span::new(8, 18),
                    name_end: 0,
                    value_span: None,
                }],
                content_end: 0,
                ..Default::default()
            }],
            ..Default::default()
        };
        let diags = run(&template);
        assert!(
            diags.is_empty(),
            "tag on non-router-link component should pass"
        );
    }
}
