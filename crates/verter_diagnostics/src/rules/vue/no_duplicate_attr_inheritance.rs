//! Rule: no-duplicate-attr-inheritance
//!
//! If `inheritAttrs: false` is set, the component should not also manually
//! spread `$attrs` via `v-bind="$attrs"` — that defeats the purpose of
//! disabling inheritance and can cause confusing double-attribute behavior.

// @ai-generated

use crate::context::LintContext;
use crate::diagnostic::{DiagnosticSpanKind, Severity};
use crate::rules::{FileContext, LintRule, RuleCategory};
use verter_semantic::analysis::types::AnalysisFlags;

pub struct NoDuplicateAttrInheritance;

impl LintRule for NoDuplicateAttrInheritance {
    fn name(&self) -> &'static str {
        "no-duplicate-attr-inheritance"
    }

    fn category(&self) -> RuleCategory {
        RuleCategory::VueRecommended
    }

    fn default_severity(&self) -> Option<Severity> {
        Some(Severity::Warning)
    }

    fn check_file(&self, file: &FileContext<'_>, ctx: &mut LintContext) {
        let script = match file.script {
            Some(s) => s,
            None => return,
        };

        // Check if inheritAttrs: false is set
        if !script
            .flags
            .contains(AnalysisFlags::HAS_INHERIT_ATTRS_FALSE)
        {
            return;
        }

        let template = match file.template {
            Some(t) => t,
            None => return,
        };

        // Check if $attrs is used in template binding occurrences
        for occ in &template.binding_occurrences {
            if occ.name == "$attrs" {
                ctx.report_with_severity(
                    self.name(),
                    self.category().as_str(),
                    "Component has `inheritAttrs: false` but also uses `$attrs` in the template. \
                     This can cause confusing double-attribute inheritance. Remove one or the other."
                        .to_string(),
                    occ.span.start,
                    occ.span.end,
                    self.default_severity(),
                    DiagnosticSpanKind::Directive,
                );
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::LintConfig;
    use crate::rules::FileContext;
    use crate::visitor::LintVisitor;
    use verter_semantic::analysis::template::*;
    use verter_semantic::analysis::types::*;
    use verter_span::Span;

    fn run_file(
        script: &ScriptAnalysisSnapshot,
        template: &TemplateAnalysisSnapshot,
    ) -> Vec<crate::diagnostic::LintDiagnostic> {
        let rules: Vec<Box<dyn LintRule>> = vec![Box::new(NoDuplicateAttrInheritance)];
        let visitor = LintVisitor::new(&rules);
        let config = LintConfig::default();
        let mut ctx = LintContext::new(&config);
        let file = FileContext {
            template: Some(template),
            script: Some(script),
            styles: &[],
            source: None,
        };
        visitor.visit_file(&file, &mut ctx);
        ctx.into_diagnostics()
    }

    #[test]
    fn inherit_attrs_false_with_attrs_usage_reports() {
        let script = ScriptAnalysisSnapshot {
            flags: AnalysisFlags::HAS_INHERIT_ATTRS_FALSE,
            ..Default::default()
        };
        let template = TemplateAnalysisSnapshot {
            binding_occurrences: vec![TemplateBindingOccurrence {
                name: "$attrs".to_string(),
                span: Span::new(20, 26),
                usage_kind: BindingUsageKind::DirectiveValue,
            }],
            ..Default::default()
        };
        let diags = run_file(&script, &template);
        assert!(
            !diags.is_empty(),
            "should report duplicate attr inheritance"
        );
        assert!(diags
            .iter()
            .any(|d| d.rule == "no-duplicate-attr-inheritance"));
        assert!(
            diags[0].message.contains("inheritAttrs"),
            "message should mention inheritAttrs"
        );
        assert!(
            !diags.iter().any(|d| d.rule == "no-v-html"),
            "must not trigger unrelated rule"
        );
    }

    #[test]
    fn inherit_attrs_false_without_attrs_passes() {
        let script = ScriptAnalysisSnapshot {
            flags: AnalysisFlags::HAS_INHERIT_ATTRS_FALSE,
            ..Default::default()
        };
        let template = TemplateAnalysisSnapshot {
            binding_occurrences: vec![TemplateBindingOccurrence {
                name: "count".to_string(),
                span: Span::new(20, 25),
                usage_kind: BindingUsageKind::Interpolation,
            }],
            ..Default::default()
        };
        let diags = run_file(&script, &template);
        assert!(
            diags.is_empty(),
            "inheritAttrs: false without $attrs usage should pass"
        );
    }

    #[test]
    fn no_inherit_attrs_false_passes() {
        let script = ScriptAnalysisSnapshot {
            flags: AnalysisFlags::empty(),
            ..Default::default()
        };
        let template = TemplateAnalysisSnapshot {
            binding_occurrences: vec![TemplateBindingOccurrence {
                name: "$attrs".to_string(),
                span: Span::new(20, 26),
                usage_kind: BindingUsageKind::DirectiveValue,
            }],
            ..Default::default()
        };
        let diags = run_file(&script, &template);
        assert!(
            diags.is_empty(),
            "without inheritAttrs: false, $attrs usage is fine"
        );
    }
}
