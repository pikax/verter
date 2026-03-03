//! Rule: no-deprecated-delete-set
//!
//! `Vue.delete()` and `Vue.set()` were removed in Vue 3. Reactivity is
//! handled natively via Proxies. Detect imports of `set` or `delete` from 'vue'.

// @ai-generated

use crate::context::LintContext;
use crate::diagnostic::{DiagnosticSpanKind, DiagnosticTag, Severity};
use crate::rules::{LintRule, RuleCategory};
use verter_analysis::types::ScriptAnalysisSnapshot;

pub struct NoDeprecatedDeleteSet;

impl LintRule for NoDeprecatedDeleteSet {
    fn name(&self) -> &'static str {
        "no-deprecated-delete-set"
    }

    fn category(&self) -> RuleCategory {
        RuleCategory::VueEssential
    }

    fn default_severity(&self) -> Severity {
        Severity::Warning
    }

    fn check_script(&self, script: &ScriptAnalysisSnapshot, ctx: &mut LintContext) {
        for import in &script.imports {
            if import.source != "vue" {
                continue;
            }
            for binding in &import.bindings {
                if binding.name == "set" || binding.name == "delete" {
                    ctx.report_with_tags(
                        self.name(),
                        self.category().as_str(),
                        format!(
                            "'Vue.{}()' has been removed in Vue 3. Use native JavaScript operations instead.",
                            binding.name
                        ),
                        binding.span.start,
                        binding.span.end,
                        self.default_severity(),
                        vec![DiagnosticTag::Deprecated],
                        DiagnosticSpanKind::ScriptCallSite,
                    );
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::LintConfig;
    use crate::visitor::LintVisitor;
    use verter_analysis::types::*;
    use verter_span::Span;

    fn run_rule(script: &ScriptAnalysisSnapshot) -> Vec<crate::diagnostic::LintDiagnostic> {
        let rules: Vec<Box<dyn LintRule>> = vec![Box::new(NoDeprecatedDeleteSet)];
        let visitor = LintVisitor::new(&rules);
        let config = LintConfig::default();
        let mut ctx = LintContext::new(&config);
        visitor.visit_script(script, &mut ctx);
        ctx.into_diagnostics()
    }

    #[test]
    fn vue_set_import_reports() {
        let script = ScriptAnalysisSnapshot {
            imports: vec![AnalyzedImport {
                source: "vue".to_string(),
                is_type_only: false,
                bindings: vec![AnalyzedImportBinding {
                    name: "set".to_string(),
                    is_type_only: false,
                    vue_api: None,
                    span: Span::new(15, 18),
                }],
                span: Span::new(0, 30),
                resolved_canonical_id: None,
            }],
            ..Default::default()
        };
        let diags = run_rule(&script);
        assert!(!diags.is_empty(), "import set from 'vue' should trigger");
        assert!(diags.iter().any(|d| d.rule == "no-deprecated-delete-set"));
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
    fn vue_delete_import_reports() {
        let script = ScriptAnalysisSnapshot {
            imports: vec![AnalyzedImport {
                source: "vue".to_string(),
                is_type_only: false,
                bindings: vec![AnalyzedImportBinding {
                    name: "delete".to_string(),
                    is_type_only: false,
                    vue_api: None,
                    span: Span::new(15, 21),
                }],
                span: Span::new(0, 35),
                resolved_canonical_id: None,
            }],
            ..Default::default()
        };
        let diags = run_rule(&script);
        assert!(!diags.is_empty(), "import delete from 'vue' should trigger");
    }

    #[test]
    fn vue_ref_import_passes() {
        let script = ScriptAnalysisSnapshot {
            imports: vec![AnalyzedImport {
                source: "vue".to_string(),
                is_type_only: false,
                bindings: vec![AnalyzedImportBinding {
                    name: "ref".to_string(),
                    is_type_only: false,
                    vue_api: Some(VueApiClassification::Ref),
                    span: Span::new(15, 18),
                }],
                span: Span::new(0, 30),
                resolved_canonical_id: None,
            }],
            ..Default::default()
        };
        let diags = run_rule(&script);
        assert!(diags.is_empty(), "import ref from 'vue' should pass");
    }
}
