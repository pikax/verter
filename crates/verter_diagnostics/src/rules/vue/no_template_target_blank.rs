//! Rule: no-template-target-blank
//!
//! `<a target="_blank">` without `rel="noopener noreferrer"` is a security risk
//! (reverse tabnapping / phishing). Always add `rel` when using `target="_blank"`.

// @ai-generated

use crate::context::LintContext;
use crate::diagnostic::{DiagnosticSpanKind, Severity};
use crate::rules::{LintRule, RuleCategory};
use verter_analysis::template::TemplateElement;

pub struct NoTemplateTargetBlank;

impl LintRule for NoTemplateTargetBlank {
    fn name(&self) -> &'static str {
        "no-template-target-blank"
    }

    fn category(&self) -> RuleCategory {
        RuleCategory::Security
    }

    fn default_severity(&self) -> Severity {
        Severity::Warning
    }

    fn check_element(&self, el: &TemplateElement, ctx: &mut LintContext) {
        if el.tag != "a" {
            return;
        }

        // Find static target="_blank"
        let target_blank_span = el
            .attributes
            .iter()
            .find(|a| !a.is_dynamic && a.name == "target" && a.value.as_deref() == Some("_blank"))
            .map(|a| a.span);

        let Some(span) = target_blank_span else {
            return;
        };

        // Check if rel contains noopener or noreferrer
        let has_safe_rel = el.attributes.iter().any(|a| {
            !a.is_dynamic
                && a.name == "rel"
                && a.value.as_deref().is_some_and(|v| {
                    v.split_whitespace()
                        .any(|part| part == "noopener" || part == "noreferrer")
                })
        });

        if has_safe_rel {
            return;
        }

        ctx.report_with_severity(
            self.name(),
            self.category().as_str(),
            "Using 'target=\"_blank\"' without 'rel=\"noopener noreferrer\"' is a security risk. \
             Add 'rel=\"noopener noreferrer\"' to prevent reverse tabnapping."
                .to_string(),
            span.start,
            span.end,
            self.default_severity(),
            DiagnosticSpanKind::Attribute,
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::LintConfig;
    use crate::visitor::LintVisitor;
    use verter_analysis::template::*;
    use verter_span::Span;

    fn run(template: &TemplateAnalysisSnapshot) -> Vec<crate::diagnostic::LintDiagnostic> {
        let rules: Vec<Box<dyn LintRule>> = vec![Box::new(NoTemplateTargetBlank)];
        let visitor = LintVisitor::new(&rules);
        let config = LintConfig::default();
        let mut ctx = LintContext::new(&config);
        visitor.visit_template(template, &mut ctx);
        ctx.into_diagnostics()
    }

    fn make_anchor(attrs: Vec<(&str, &str)>) -> TemplateElement {
        TemplateElement {
            tag: "a".to_string(),
            attributes: attrs
                .into_iter()
                .map(|(name, val)| TemplateAttribute {
                    name: name.to_string(),
                    value: Some(val.to_string()),
                    is_dynamic: false,
                    span: Span::new(3, 20),
                })
                .collect(),
            ..Default::default()
        }
    }

    #[test]
    fn target_blank_without_rel_reports() {
        let template = TemplateAnalysisSnapshot {
            elements: vec![make_anchor(vec![("target", "_blank")])],
            ..Default::default()
        };
        let diags = run(&template);
        assert!(
            !diags.is_empty(),
            "target=\"_blank\" without rel should trigger"
        );
        assert!(diags.iter().any(|d| d.rule == "no-template-target-blank"));
        assert!(
            !diags.iter().any(|d| d.rule == "no-v-html"),
            "must not trigger unrelated rule"
        );
    }

    #[test]
    fn target_blank_with_noopener_passes() {
        let template = TemplateAnalysisSnapshot {
            elements: vec![make_anchor(vec![
                ("target", "_blank"),
                ("rel", "noopener noreferrer"),
            ])],
            ..Default::default()
        };
        let diags = run(&template);
        assert!(
            diags.is_empty(),
            "with rel=\"noopener noreferrer\" should pass"
        );
    }

    #[test]
    fn target_self_passes() {
        let template = TemplateAnalysisSnapshot {
            elements: vec![make_anchor(vec![("target", "_self")])],
            ..Default::default()
        };
        let diags = run(&template);
        assert!(diags.is_empty(), "target=\"_self\" should pass");
    }

    #[test]
    fn non_anchor_target_blank_passes() {
        let template = TemplateAnalysisSnapshot {
            elements: vec![TemplateElement {
                tag: "button".to_string(),
                attributes: vec![TemplateAttribute {
                    name: "target".to_string(),
                    value: Some("_blank".to_string()),
                    is_dynamic: false,
                    span: Span::new(7, 22),
                }],
                ..Default::default()
            }],
            ..Default::default()
        };
        let diags = run(&template);
        assert!(diags.is_empty(), "non-anchor target=\"_blank\" should pass");
    }
}
