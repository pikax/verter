//! Rule: prefer-import-from-vue
//!
//! Prefer importing from `vue` instead of internal packages like
//! `@vue/runtime-core`, `@vue/runtime-dom`, `@vue/reactivity`, `@vue/shared`.

use crate::context::LintContext;
use crate::diagnostic::{DiagnosticSpanKind, Severity};
use crate::rules::{LintRule, RuleCategory};
use verter_analysis::types::ScriptAnalysisSnapshot;

const INTERNAL_VUE_PACKAGES: &[&str] = &[
    "@vue/runtime-core",
    "@vue/runtime-dom",
    "@vue/reactivity",
    "@vue/shared",
    "@vue/composition-api",
];

pub struct PreferImportFromVue;

impl LintRule for PreferImportFromVue {
    fn name(&self) -> &'static str {
        "prefer-import-from-vue"
    }

    fn category(&self) -> RuleCategory {
        RuleCategory::Script
    }

    fn default_severity(&self) -> Severity {
        Severity::Warning
    }

    fn check_script(&self, script: &ScriptAnalysisSnapshot, ctx: &mut LintContext) {
        for import in &script.imports {
            if INTERNAL_VUE_PACKAGES.contains(&import.source.as_str()) {
                ctx.report_with_severity(
                    self.name(),
                    self.category().as_str(),
                    format!(
                        "Import from 'vue' instead of '{}'. Vue re-exports all public APIs from the main package.",
                        import.source
                    ),
                    import.span.start,
                    import.span.end,
                    self.default_severity(),
                    DiagnosticSpanKind::ScriptCallSite,
                );
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    use verter_analysis::types::*;
    use verter_span::Span;

    fn run_rule(script: &ScriptAnalysisSnapshot) -> Vec<crate::diagnostic::LintDiagnostic> {
        crate::test_support::run_script_rule(PreferImportFromVue, script)
    }

    #[test]
    fn internal_vue_import_reports() {
        let script = ScriptAnalysisSnapshot {
            imports: vec![AnalyzedImport {
                source: "@vue/runtime-core".to_string(),
                is_type_only: false,
                bindings: vec![],
                span: Span::new(0, 40),
                resolved_canonical_id: None,
            }],
            ..Default::default()
        };
        let diags = run_rule(&script);
        assert!(!diags.is_empty(), "internal Vue import should trigger");
        assert!(diags[0].rule == "prefer-import-from-vue");
        assert!(
            !diags.iter().any(|d| d.rule == "no-reserved-keys"),
            "must not trigger unrelated rule"
        );
    }

    #[test]
    fn vue_import_passes() {
        let script = ScriptAnalysisSnapshot {
            imports: vec![AnalyzedImport {
                source: "vue".to_string(),
                is_type_only: false,
                bindings: vec![],
                span: Span::new(0, 25),
                resolved_canonical_id: None,
            }],
            ..Default::default()
        };
        let diags = run_rule(&script);
        assert!(diags.is_empty(), "vue import should pass");
    }
}
