//! Rule: no-export-in-script-setup
//!
//! Disallows `export` statements in `<script setup>`.
//! Only `export default` is allowed (which is the component itself).

use crate::context::LintContext;
use crate::diagnostic::{DiagnosticSpanKind, Severity};
use crate::rules::{FileContext, LintRule, RuleCategory};

pub struct NoExportInScriptSetup;

impl LintRule for NoExportInScriptSetup {
    fn name(&self) -> &'static str {
        "no-export-in-script-setup"
    }

    fn category(&self) -> RuleCategory {
        RuleCategory::VueEssential
    }

    fn default_severity(&self) -> Option<Severity> {
        Some(Severity::Error)
    }

    fn check_file(&self, file: &FileContext<'_>, ctx: &mut LintContext) {
        let source = match file.source {
            Some(s) => s,
            None => return,
        };
        let script = match file.script {
            Some(s) => s,
            None => return,
        };

        if !script
            .flags
            .contains(verter_analysis::types::AnalysisFlags::ASYNC_SETUP)
            && script.macros.is_empty()
            && script.vue_api_calls.is_empty()
        {
            // Heuristic: if no setup indicators, it's likely a normal script
            return;
        }

        // Check for named exports in imports (re-exports)
        for import in &script.imports {
            let import_text = source.get(import.span.start as usize..import.span.end as usize);
            if let Some(text) = import_text {
                if text.starts_with("export") {
                    ctx.report_with_severity(
                        self.name(),
                        self.category().as_str(),
                        "`<script setup>` does not support named exports. Move exports to a separate `<script>` block.".to_string(),
                        import.span.start,
                        import.span.end,
                        self.default_severity(),
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

    #[test]
    fn struct_compiles() {
        let _rule = NoExportInScriptSetup;
        assert_eq!(NoExportInScriptSetup.name(), "no-export-in-script-setup");
    }
}
