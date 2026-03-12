//! SSR code actions: wrap client-only code in `onMounted()` or `<ClientOnly>`.
//!
//! Handles: `no-client-only-lifecycle-in-setup`, `no-dom-query-in-setup`,
//! `no-browser-globals-in-setup`, `no-template-ref-in-setup`, `require-client-only-wrapper`

use crate::provider::{ActionContext, ActionProvider};
use crate::types::{ActionKind, AutofixSafety, CodeAction, FileEdit};
use verter_diagnostics::LintDiagnostic;

pub struct SsrWrap;

impl ActionProvider for SsrWrap {
    fn name(&self) -> &str {
        "ssr-wrap"
    }

    fn fixes_for_diagnostic(&self, diag: &LintDiagnostic, ctx: &ActionContext) -> Vec<CodeAction> {
        match diag.rule.as_str() {
            // Script-level SSR rules: suggest wrapping in onMounted()
            "no-client-only-lifecycle-in-setup"
            | "no-dom-query-in-setup"
            | "no-browser-globals-in-setup"
            | "no-template-ref-in-setup"
            | "no-side-effects-in-setup-for-ssr" => wrap_in_on_mounted(diag, ctx),
            // Template-level: suggest wrapping in <ClientOnly>
            "require-client-only-wrapper" => wrap_in_client_only(diag, ctx),
            // Suggest adding onServerPrefetch for data fetching
            "prefer-server-prefetch" => add_server_prefetch(diag, ctx),
            _ => vec![],
        }
    }
}

/// Suggest wrapping the flagged code in `onMounted(() => { ... })`.
fn wrap_in_on_mounted(diag: &LintDiagnostic, ctx: &ActionContext) -> Vec<CodeAction> {
    let start = diag.span.start as usize;
    let end = diag.span.end as usize;
    if end > ctx.source.len() {
        return vec![];
    }

    let original = &ctx.source[start..end];
    let replacement = format!("onMounted(() => {{ {original} }})");

    vec![CodeAction {
        title: "Wrap in onMounted()".to_string(),
        kind: ActionKind::QuickFix,
        edits: vec![FileEdit {
            file_id: None,
            replacement,
            span: verter_span::Span::new(diag.span.start, diag.span.end),
        }],
        is_preferred: false,
        diagnostic_rule: Some(diag.rule.clone()),
        safety: AutofixSafety::Safe,
    }]
}

/// Suggest wrapping a component in `<ClientOnly>`.
fn wrap_in_client_only(diag: &LintDiagnostic, ctx: &ActionContext) -> Vec<CodeAction> {
    let start = diag.span.start as usize;
    let end = diag.span.end as usize;
    if end > ctx.source.len() {
        return vec![];
    }

    let original = &ctx.source[start..end];
    let replacement = format!("<ClientOnly>{original}</ClientOnly>");

    vec![CodeAction {
        title: "Wrap in <ClientOnly>".to_string(),
        kind: ActionKind::QuickFix,
        edits: vec![FileEdit {
            file_id: None,
            replacement,
            span: verter_span::Span::new(diag.span.start, diag.span.end),
        }],
        is_preferred: false,
        diagnostic_rule: Some(diag.rule.clone()),
        safety: AutofixSafety::Safe,
    }]
}

/// Suggest adding `onServerPrefetch()` for data fetching.
fn add_server_prefetch(diag: &LintDiagnostic, ctx: &ActionContext) -> Vec<CodeAction> {
    let start = diag.span.start as usize;
    let end = diag.span.end as usize;
    if end > ctx.source.len() {
        return vec![];
    }

    // Insert onServerPrefetch before the onMounted call
    let insert = "onServerPrefetch(async () => {\n  // TODO: fetch data here\n})\n";

    vec![CodeAction {
        title: "Add onServerPrefetch()".to_string(),
        kind: ActionKind::QuickFix,
        edits: vec![FileEdit {
            file_id: None,
            replacement: format!("{insert}{}", &ctx.source[start..end]),
            span: verter_span::Span::new(diag.span.start, diag.span.end),
        }],
        is_preferred: false,
        diagnostic_rule: Some(diag.rule.clone()),
        safety: AutofixSafety::Safe,
    }]
}

#[cfg(test)]
mod tests {
    use super::*;
    use verter_diagnostics::LintDiagnostic;
    use verter_span::Span;

    fn make_diag(rule: &str, start: u32, end: u32) -> LintDiagnostic {
        LintDiagnostic {
            rule: rule.to_string(),
            category: "ssr".to_string(),
            message: "test".to_string(),
            severity: verter_diagnostics::Severity::Warning,
            span: Span::new(start, end),
            span_kind: verter_diagnostics::DiagnosticSpanKind::Attribute,
            certainty: verter_diagnostics::Certainty::Definite,
            evidence: vec![],
            tags: vec![],
            related_files: vec![],
        }
    }

    fn run_fix(rule: &str, source: &str) -> Vec<CodeAction> {
        let diags = verter_diagnostics::DiagnosticSet::new();
        let ctx = ActionContext {
            source,
            file_id: "test.vue",
            diagnostics: &diags,
            template: None,
            script: None,
            styles: &[],
        };
        let diag = make_diag(rule, 0, source.len() as u32);
        SsrWrap.fixes_for_diagnostic(&diag, &ctx)
    }

    #[test]
    fn wrap_in_on_mounted_for_lifecycle() {
        let source = "onMounted(() => { document.getElementById('x') })";
        let actions = run_fix("no-client-only-lifecycle-in-setup", source);
        assert_eq!(actions.len(), 1);
        assert_eq!(actions[0].title, "Wrap in onMounted()");
        assert!(
            actions[0].edits[0].replacement.starts_with("onMounted("),
            "replacement should wrap in onMounted"
        );
    }

    #[test]
    fn wrap_in_client_only_for_component() {
        let source = "<GoogleMap />";
        let actions = run_fix("require-client-only-wrapper", source);
        assert_eq!(actions.len(), 1);
        assert_eq!(actions[0].title, "Wrap in <ClientOnly>");
        assert!(
            actions[0].edits[0].replacement.contains("<ClientOnly>"),
            "replacement should wrap in ClientOnly"
        );
        assert!(
            actions[0].edits[0].replacement.contains("</ClientOnly>"),
            "replacement should have closing ClientOnly"
        );
    }

    #[test]
    fn add_server_prefetch_for_data_fetching() {
        let source = "onMounted(async () => { await fetch('/api') })";
        let actions = run_fix("prefer-server-prefetch", source);
        assert_eq!(actions.len(), 1);
        assert_eq!(actions[0].title, "Add onServerPrefetch()");
        assert!(
            actions[0].edits[0].replacement.contains("onServerPrefetch"),
            "replacement should add onServerPrefetch"
        );
    }

    #[test]
    fn no_action_for_unrelated_rule() {
        let source = "const x = 1";
        let actions = run_fix("no-v-html", source);
        assert!(
            actions.is_empty(),
            "unrelated rule should produce no actions"
        );
    }
}
