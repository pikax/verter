//! Quick fix: remove import or export statements.
//!
//! Handles: `no-import-compiler-macros`, `no-export-in-script-setup`

// @ai-generated

use crate::provider::{ActionContext, ActionProvider};
use crate::types::{ActionKind, AutofixSafety, CodeAction, FileEdit};
use verter_diagnostics::LintDiagnostic;

pub struct RemoveImport;

impl ActionProvider for RemoveImport {
    fn name(&self) -> &str {
        "remove-import"
    }

    fn fixes_for_diagnostic(&self, diag: &LintDiagnostic, ctx: &ActionContext) -> Vec<CodeAction> {
        let title = match diag.rule.as_str() {
            "no-import-compiler-macros" => "Remove compiler macro import",
            "no-export-in-script-setup" => "Remove export statement",
            _ => return vec![],
        };

        let source = ctx.source;
        let start = diag.span.start as usize;
        let end = diag.span.end as usize;

        if end > source.len() {
            return vec![];
        }

        // Extend to include trailing newline if present
        let remove_end = if end < source.len() && source.as_bytes()[end] == b'\n' {
            end + 1
        } else {
            end
        };

        vec![CodeAction {
            title: title.to_string(),
            kind: ActionKind::QuickFix,
            edits: vec![FileEdit {
                file_id: None,
                replacement: String::new(),
                span: verter_span::Span::new(start as u32, remove_end as u32),
            }],
            is_preferred: true,
            diagnostic_rule: Some(diag.rule.clone()),
            safety: AutofixSafety::Safe,
        }]
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::provider::ActionContext;
    use verter_diagnostics::{DiagnosticSet, DiagnosticSpanKind, LintDiagnostic, Severity};

    #[test]
    fn removes_compiler_macro_import() {
        let source = "import { defineProps } from 'vue'\nconst props = defineProps()";
        let end = source.find('\n').unwrap();
        let diag = LintDiagnostic {
            rule: "no-import-compiler-macros".to_string(),
            category: "script".to_string(),
            severity: Severity::Warning,
            message: "do not import compiler macros".to_string(),
            span: verter_span::Span::new(0, end as u32),
            tags: vec![],
            span_kind: DiagnosticSpanKind::ScriptCallSite,
            certainty: verter_diagnostics::Certainty::Definite,
            evidence: Vec::new(),
            related_files: Vec::new(),
        };
        let set = DiagnosticSet::new();
        let ctx = ActionContext {
            source,
            file_id: "/src/App.vue",
            diagnostics: &set,
            template: None,
            script: None,
            styles: &[],
        };
        let actions = RemoveImport.fixes_for_diagnostic(&diag, &ctx);
        assert_eq!(actions.len(), 1);
        assert!(
            actions[0].edits[0].replacement.is_empty(),
            "should delete the import"
        );
    }

    #[test]
    fn ignores_unrelated_rule() {
        let source = "import { ref } from 'vue'";
        let diag = LintDiagnostic {
            rule: "other-rule".to_string(),
            category: "test".to_string(),
            severity: Severity::Warning,
            message: "other".to_string(),
            span: verter_span::Span::new(0, 10),
            tags: vec![],
            span_kind: DiagnosticSpanKind::ScriptCallSite,
            certainty: verter_diagnostics::Certainty::Definite,
            evidence: Vec::new(),
            related_files: Vec::new(),
        };
        let set = DiagnosticSet::new();
        let ctx = ActionContext {
            source,
            file_id: "/src/App.vue",
            diagnostics: &set,
            template: None,
            script: None,
            styles: &[],
        };
        assert!(RemoveImport.fixes_for_diagnostic(&diag, &ctx).is_empty());
    }
}
