//! # no-unused-store-import
//!
//! Warns when a store is imported but the binding is never used in script or template.
//!
//! ## Bad
//! ```vue
//! <script setup>
//! import { useUserStore } from '@/stores/user'; // imported but never called
//! </script>
//! ```

use crate::context::LintContext;
use crate::diagnostic::{DiagnosticSpanKind, Severity};
use crate::rules::{LintRule, RuleCategory};
use verter_semantic::analysis::types::{AnalysisFlags, ScriptAnalysisSnapshot};

pub struct NoUnusedStoreImport;

impl LintRule for NoUnusedStoreImport {
    fn name(&self) -> &'static str {
        "no-unused-store-import"
    }

    fn category(&self) -> RuleCategory {
        RuleCategory::Script
    }

    fn default_severity(&self) -> Option<Severity> {
        Some(Severity::Warning)
    }

    fn check_script(&self, script: &ScriptAnalysisSnapshot, ctx: &mut LintContext) {
        if !script.flags.contains(AnalysisFlags::HAS_STORE_USAGE)
            && !script.flags.contains(AnalysisFlags::HAS_STORE_DEFINITION)
        {
            return;
        }

        // Collect all store callee names that were actually called
        let used_callees: std::collections::HashSet<&str> = script
            .store_usages
            .iter()
            .map(|u| u.callee.as_str())
            .chain(
                script
                    .store_definitions
                    .iter()
                    .map(|d| d.export_name.as_str()),
            )
            .collect();

        // Check imports for store-like imports that are never used
        for import in &script.imports {
            if import.is_type_only {
                continue;
            }
            let source = &import.source;
            // Only check imports from store-related sources
            if !is_store_source(source) {
                continue;
            }
            for binding in &import.bindings {
                if binding.is_type_only {
                    continue;
                }
                // Check if this binding is used in any store call or definition
                if !used_callees.contains(binding.name.as_str()) {
                    ctx.report_with_severity(
                        self.name(),
                        self.category().as_str(),
                        format!(
                            "Store import `{}` from `{}` is never used.",
                            binding.name, source
                        ),
                        binding.span.start,
                        binding.span.end,
                        self.default_severity(),
                        DiagnosticSpanKind::ScriptCallSite,
                    );
                }
            }
        }
    }
}

/// Check if an import source looks like a store module.
fn is_store_source(source: &str) -> bool {
    source == "pinia"
        || source == "vuex"
        || source.contains("/store")
        || source.contains("/stores")
        || source.contains("\\store")
        || source.contains("\\stores")
}

#[cfg(test)]
mod tests {
    use super::*;
    use verter_semantic::analysis::types::{
        AnalyzedImport, AnalyzedImportBinding, ImportBindingKind, StoreApiClassification,
        StoreUsage,
    };
    use verter_span::Span;

    fn run_rule(script: &ScriptAnalysisSnapshot) -> Vec<crate::diagnostic::LintDiagnostic> {
        crate::test_support::run_script_rule(NoUnusedStoreImport, script)
    }

    #[test]
    fn empty_script_no_diagnostics() {
        let script = ScriptAnalysisSnapshot::default();
        assert!(run_rule(&script).is_empty());
    }

    #[test]
    fn used_store_import_no_warning() {
        let script = ScriptAnalysisSnapshot {
            imports: vec![AnalyzedImport {
                source: "@/stores/user".to_string(),
                is_type_only: false,
                bindings: vec![AnalyzedImportBinding {
                    name: "useUserStore".to_string(),
                    kind: ImportBindingKind::Named,
                    imported_name: None,
                    is_type_only: false,
                    vue_api: None,
                    span: Span::new(10, 30),
                }],
                span: Span::new(0, 50),
                resolved_canonical_id: None,
            }],
            store_usages: vec![StoreUsage {
                binding_name: "store".to_string(),
                callee: "useUserStore".to_string(),
                import_source: "@/stores/user".to_string(),
                store_api: StoreApiClassification::StoreComposable,
                span: Span::new(60, 80),
                has_store_to_refs: false,
                destructured_props: Vec::new(),
                destructured_without_store_to_refs: false,
            }],
            flags: AnalysisFlags::HAS_STORE_USAGE,
            ..Default::default()
        };
        let diags = run_rule(&script);
        assert!(diags.is_empty(), "used store should not trigger warning");
    }

    #[test]
    fn non_store_import_not_flagged() {
        let script = ScriptAnalysisSnapshot {
            imports: vec![AnalyzedImport {
                source: "vue-router".to_string(),
                is_type_only: false,
                bindings: vec![AnalyzedImportBinding {
                    name: "useRouter".to_string(),
                    kind: ImportBindingKind::Named,
                    imported_name: None,
                    is_type_only: false,
                    vue_api: None,
                    span: Span::new(10, 30),
                }],
                span: Span::new(0, 50),
                resolved_canonical_id: None,
            }],
            flags: AnalysisFlags::HAS_STORE_USAGE,
            ..Default::default()
        };
        let diags = run_rule(&script);
        assert!(diags.is_empty(), "non-store imports should not be checked");
    }
}
