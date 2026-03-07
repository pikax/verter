//! Rule: no-unused-refs
//!
//! Reports template refs that are not referenced in the script bindings.
//! A `ref="foo"` in the template should have a corresponding binding in the script.

// @ai-generated

use crate::context::LintContext;
use crate::diagnostic::{DiagnosticSpanKind, Severity};
use crate::rules::{FileContext, LintRule, RuleCategory};

pub struct NoUnusedRefs;

impl LintRule for NoUnusedRefs {
    fn name(&self) -> &'static str {
        "no-unused-refs"
    }

    fn category(&self) -> RuleCategory {
        RuleCategory::VueRecommended
    }

    fn default_severity(&self) -> Severity {
        Severity::Warning
    }

    fn check_file(&self, file: &FileContext<'_>, ctx: &mut LintContext) {
        let Some(template) = file.template else {
            return;
        };

        if template.template_refs.is_empty() {
            return;
        }

        let Some(script) = file.script else {
            // Template refs exist but no script — report all
            for tref in &template.template_refs {
                if tref.is_dynamic {
                    continue;
                }
                ctx.report_with_severity(
                    self.name(),
                    self.category().as_str(),
                    format!(
                        "Template ref '{}' is not referenced in the script.",
                        tref.name
                    ),
                    0,
                    0,
                    self.default_severity(),
                    DiagnosticSpanKind::Directive,
                );
            }
            return;
        };

        // Collect all binding names from script
        let binding_names: std::collections::HashSet<&str> =
            script.bindings.iter().map(|b| b.name.as_str()).collect();

        // Also collect ref names from useTemplateRef() calls
        let use_template_ref_names: std::collections::HashSet<&str> = script
            .vue_api_calls
            .iter()
            .filter(|c| c.api == verter_analysis::types::VueApiClassification::UseTemplateRef)
            .filter_map(|c| c.arg_value.as_deref())
            .collect();

        for tref in &template.template_refs {
            if tref.is_dynamic {
                continue;
            }

            // Check if there's a script binding or useTemplateRef call matching the ref name
            if !binding_names.contains(tref.name.as_str())
                && !use_template_ref_names.contains(tref.name.as_str())
            {
                ctx.report_with_severity(
                    self.name(),
                    self.category().as_str(),
                    format!(
                        "Template ref '{}' is not referenced in the script.",
                        tref.name
                    ),
                    0,
                    0,
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
    use verter_analysis::template::*;
    use verter_analysis::types::*;
    use verter_span::Span;

    fn run_rule_with_file(
        template: &TemplateAnalysisSnapshot,
        script: &ScriptAnalysisSnapshot,
    ) -> Vec<crate::diagnostic::LintDiagnostic> {
        let rules: Vec<Box<dyn LintRule>> = vec![Box::new(NoUnusedRefs)];
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

    fn make_template_ref(name: &str) -> TemplateRef {
        TemplateRef {
            name: name.to_string(),
            is_dynamic: false,
            target_tag: "div".to_string(),
        }
    }

    fn make_binding(name: &str) -> AnalyzedBinding {
        AnalyzedBinding {
            name: name.to_string(),
            kind: AnalyzedBindingKind::Let,
            is_reactive: false,
            reactivity_kind: ReactivityKind::None,
            type_annotation: None,
            initializer: None,
            span: Span::new(0, 10),
        }
    }

    #[test]
    fn ref_without_binding_reports() {
        let template = TemplateAnalysisSnapshot {
            template_refs: vec![make_template_ref("myEl")],
            ..Default::default()
        };
        let script = ScriptAnalysisSnapshot::default();
        let diags = run_rule_with_file(&template, &script);
        assert!(!diags.is_empty(), "ref without binding should trigger");
        assert!(diags.iter().any(|d| d.rule == "no-unused-refs"));
        assert!(
            diags[0].message.contains("myEl"),
            "message should mention ref name"
        );
        assert!(
            !diags.iter().any(|d| d.rule == "no-v-html"),
            "must not trigger unrelated rule"
        );
    }

    #[test]
    fn ref_with_matching_binding_passes() {
        let template = TemplateAnalysisSnapshot {
            template_refs: vec![make_template_ref("myEl")],
            ..Default::default()
        };
        let script = ScriptAnalysisSnapshot {
            bindings: vec![make_binding("myEl")],
            ..Default::default()
        };
        let diags = run_rule_with_file(&template, &script);
        assert!(diags.is_empty(), "ref with matching binding should pass");
    }

    #[test]
    fn no_refs_passes() {
        let template = TemplateAnalysisSnapshot::default();
        let script = ScriptAnalysisSnapshot::default();
        let diags = run_rule_with_file(&template, &script);
        assert!(diags.is_empty(), "no refs should pass");
    }

    #[test]
    fn ref_with_use_template_ref_passes() {
        // FP3: useTemplateRef('myEl') should count as a valid ref binding,
        // even if there's no matching script binding name.
        let template = TemplateAnalysisSnapshot {
            template_refs: vec![make_template_ref("myEl")],
            ..Default::default()
        };
        let script = ScriptAnalysisSnapshot {
            vue_api_calls: vec![VueApiCallSite {
                api: VueApiClassification::UseTemplateRef,
                span: Span::new(0, 30),
                arg_value: Some("myEl".to_string()),
                is_async_callback: false,
                callback_params: vec![],
            }],
            ..Default::default()
        };
        let diags = run_rule_with_file(&template, &script);
        assert!(
            diags.is_empty(),
            "ref covered by useTemplateRef should NOT be reported unused"
        );
    }

    #[test]
    fn dynamic_ref_passes() {
        let template = TemplateAnalysisSnapshot {
            template_refs: vec![TemplateRef {
                name: "expr".to_string(),
                is_dynamic: true,
                target_tag: "div".to_string(),
            }],
            ..Default::default()
        };
        let script = ScriptAnalysisSnapshot::default();
        let diags = run_rule_with_file(&template, &script);
        assert!(diags.is_empty(), "dynamic ref should pass");
    }
}
