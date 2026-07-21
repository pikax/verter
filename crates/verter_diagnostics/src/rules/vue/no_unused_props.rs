//! Rule: no-unused-props
//!
//! Warns when a prop is declared but never used in script or template.

use crate::context::LintContext;
use crate::diagnostic::{DiagnosticSpanKind, DiagnosticTag, Severity};
use crate::rules::{LintRule, RuleCategory};
use verter_semantic::analysis::template::TemplateAnalysisSnapshot;

/// Disallow declared props that are provably never read.
///
/// `TemplateAnalysisSnapshot::prop_definitions` is populated FAIL-OPEN by the
/// shared unused-declaration pipeline: members appear only when script AND
/// template usage could be statically bounded (no whole-object escape, no
/// destructured `defineProps` — that is provider-owned TS6133 — no `$props`,
/// no style `v-bind()` on the props root, no expression parse errors).
///
/// Known accepted false positive (documented, hint-severity): a prop declared
/// solely to STRIP it from `$attrs` fallthrough is genuinely unread and WILL
/// be flagged.
pub struct NoUnusedProps;

impl LintRule for NoUnusedProps {
    fn name(&self) -> &'static str {
        "no-unused-props"
    }

    fn category(&self) -> RuleCategory {
        RuleCategory::VueRecommended
    }

    fn default_severity(&self) -> Option<Severity> {
        Some(Severity::Hint)
    }

    fn check_template(&self, tpl: &TemplateAnalysisSnapshot, ctx: &mut LintContext) {
        for prop in &tpl.prop_definitions {
            if prop.used_in_template || prop.used_in_script {
                continue;
            }

            ctx.report_with_tags(
                self.name(),
                self.category().as_str(),
                format!("Prop '{}' is declared but never used.", prop.name),
                prop.span.start,
                prop.span.end,
                self.default_severity(),
                vec![DiagnosticTag::Unnecessary],
                DiagnosticSpanKind::PropDefinition,
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    use verter_semantic::analysis::template::{AnalyzedPropDefinition, TemplateAnalysisSnapshot};

    fn run_rule(template: &TemplateAnalysisSnapshot) -> Vec<crate::diagnostic::LintDiagnostic> {
        crate::test_support::run_template_rule(NoUnusedProps, template)
    }

    fn make_prop(
        name: &str,
        used_in_template: bool,
        used_in_script: bool,
    ) -> AnalyzedPropDefinition {
        AnalyzedPropDefinition {
            name: name.to_string(),
            type_annotation: None,
            has_default: false,
            is_required: false,
            is_boolean: false,
            used_in_template,
            used_in_script,
            span: verter_span::Span::new(10, 20),
        }
    }

    #[test]
    fn reports_unused_props() {
        let template = TemplateAnalysisSnapshot {
            prop_definitions: vec![make_prop("msg", false, false)],
            ..Default::default()
        };

        let diags = run_rule(&template);
        assert_eq!(diags.len(), 1);
        assert_eq!(diags[0].rule, "no-unused-props");
        assert!(diags[0].message.contains("msg"));
        // Faded TS-unused look: Unnecessary tag + low-stakes hint severity on
        // the authored declaration span.
        assert_eq!(diags[0].tags, vec![DiagnosticTag::Unnecessary]);
        assert_eq!(diags[0].severity, Severity::Hint);
        assert_eq!(diags[0].span, verter_span::Span::new(10, 20));
    }

    #[test]
    fn ignores_props_used_in_template() {
        let template = TemplateAnalysisSnapshot {
            prop_definitions: vec![make_prop("msg", true, false)],
            ..Default::default()
        };

        let diags = run_rule(&template);
        assert!(diags.is_empty());
    }

    #[test]
    fn ignores_props_used_in_script() {
        let template = TemplateAnalysisSnapshot {
            prop_definitions: vec![make_prop("msg", false, true)],
            ..Default::default()
        };

        let diags = run_rule(&template);
        assert!(diags.is_empty());
    }
}
