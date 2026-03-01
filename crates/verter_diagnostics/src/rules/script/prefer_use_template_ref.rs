//! Rule: prefer-use-template-ref
//!
//! Suggests using `useTemplateRef()` for template refs instead of `ref()`.
//! When a template has `ref="foo"` but the script uses `ref()` rather than
//! `useTemplateRef('foo')`, this rule fires.

// @ai-generated

use crate::context::LintContext;
use crate::diagnostic::{DiagnosticSpanKind, Severity};
use crate::rules::{FileContext, LintRule, RuleCategory};

pub struct PreferUseTemplateRef;

impl LintRule for PreferUseTemplateRef {
    fn name(&self) -> &'static str {
        "prefer-use-template-ref"
    }

    fn category(&self) -> RuleCategory {
        RuleCategory::Script
    }

    fn default_severity(&self) -> Severity {
        Severity::Warning
    }

    fn check_file(&self, file: &FileContext<'_>, ctx: &mut LintContext) {
        use verter_analysis::types::VueApiClassification;

        let Some(template) = file.template else {
            return;
        };

        if template.template_refs.is_empty() {
            return;
        }

        let Some(script) = file.script else {
            return;
        };

        // Collect all ref names already covered by useTemplateRef
        let template_ref_calls: Vec<&str> = script
            .vue_api_calls
            .iter()
            .filter(|c| c.api == VueApiClassification::UseTemplateRef)
            .filter_map(|c| c.arg_value.as_deref())
            .collect();

        // For each template ref, check if useTemplateRef is used
        for tref in &template.template_refs {
            if tref.is_dynamic {
                continue; // dynamic :ref, skip
            }
            if !template_ref_calls.contains(&tref.name.as_str()) {
                ctx.report_with_severity(
                    self.name(),
                    self.category().as_str(),
                    format!(
                        "Template ref '{}' should use useTemplateRef('{}') instead of ref() for better type inference.",
                        tref.name, tref.name
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
        let rules: Vec<Box<dyn LintRule>> = vec![Box::new(PreferUseTemplateRef)];
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

    fn make_use_template_ref_call(ref_name: &str) -> VueApiCallSite {
        VueApiCallSite {
            api: VueApiClassification::UseTemplateRef,
            span: Span::new(0, 30),
            arg_value: Some(ref_name.to_string()),
            is_async_callback: false,
        }
    }

    #[test]
    fn template_ref_without_use_template_ref_reports() {
        let template = TemplateAnalysisSnapshot {
            template_refs: vec![make_template_ref("myEl")],
            ..Default::default()
        };
        let script = ScriptAnalysisSnapshot::default();
        let diags = run_rule_with_file(&template, &script);
        assert!(
            !diags.is_empty(),
            "template ref without useTemplateRef should trigger"
        );
        assert!(diags.iter().any(|d| d.rule == "prefer-use-template-ref"));
        assert!(
            diags[0].message.contains("myEl"),
            "message should mention the ref name"
        );
        assert!(
            !diags.iter().any(|d| d.rule == "no-v-html"),
            "must not trigger no-v-html"
        );
    }

    #[test]
    fn template_ref_with_use_template_ref_passes() {
        let template = TemplateAnalysisSnapshot {
            template_refs: vec![make_template_ref("myEl")],
            ..Default::default()
        };
        let script = ScriptAnalysisSnapshot {
            vue_api_calls: vec![make_use_template_ref_call("myEl")],
            ..Default::default()
        };
        let diags = run_rule_with_file(&template, &script);
        assert!(
            diags.is_empty(),
            "template ref covered by useTemplateRef should pass"
        );
    }

    #[test]
    fn no_template_refs_passes() {
        let template = TemplateAnalysisSnapshot::default();
        let script = ScriptAnalysisSnapshot::default();
        let diags = run_rule_with_file(&template, &script);
        assert!(
            diags.is_empty(),
            "no template refs should produce no diagnostics"
        );
    }
}
