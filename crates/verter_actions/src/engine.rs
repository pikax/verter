//! Action engine: registry + dispatch.

use crate::provider::{ActionContext, ActionProvider};
use crate::types::CodeAction;
use verter_diagnostics::LintDiagnostic;

/// Central registry of action providers. Dispatches diagnostic-based and
/// position-based requests to all registered providers.
pub struct ActionEngine {
    providers: Vec<Box<dyn ActionProvider>>,
}

impl ActionEngine {
    /// Create a new empty action engine.
    pub fn new() -> Self {
        Self {
            providers: Vec::new(),
        }
    }

    /// Register an action provider.
    pub fn register(&mut self, provider: Box<dyn ActionProvider>) {
        self.providers.push(provider);
    }

    /// Get all quick fixes for a diagnostic.
    pub fn fixes_for(&self, diag: &LintDiagnostic, ctx: &ActionContext) -> Vec<CodeAction> {
        let mut actions = Vec::new();
        for provider in &self.providers {
            actions.extend(provider.fixes_for_diagnostic(diag, ctx));
        }
        actions
    }

    /// Get all actions at a byte offset.
    pub fn actions_at(&self, offset: u32, ctx: &ActionContext) -> Vec<CodeAction> {
        let mut actions = Vec::new();
        for provider in &self.providers {
            actions.extend(provider.actions_at(offset, ctx));
        }
        actions
    }

    /// Create an engine with all built-in providers registered.
    pub fn builtin() -> Self {
        let mut engine = Self::new();
        crate::providers::register_builtin_providers(&mut engine);
        engine
    }
}

impl Default for ActionEngine {
    fn default() -> Self {
        Self::builtin()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::provider::ActionContext;
    use crate::types::{ActionKind, FileEdit};
    use verter_diagnostics::{DiagnosticSet, DiagnosticSpanKind, LintDiagnostic, Severity};

    struct TestProvider;
    impl ActionProvider for TestProvider {
        fn name(&self) -> &str {
            "test-provider"
        }

        fn fixes_for_diagnostic(
            &self,
            diag: &LintDiagnostic,
            _ctx: &ActionContext,
        ) -> Vec<CodeAction> {
            if diag.rule == "test-rule" {
                vec![CodeAction {
                    title: "Fix test-rule".to_string(),
                    kind: ActionKind::QuickFix,
                    edits: vec![FileEdit {
                        file_id: None,
                        replacement: "fixed".to_string(),
                        span: diag.span,
                    }],
                    is_preferred: true,
                    diagnostic_rule: Some("test-rule".to_string()),
                }]
            } else {
                vec![]
            }
        }
    }

    fn make_diag(rule: &str) -> LintDiagnostic {
        LintDiagnostic {
            rule: rule.to_string(),
            category: "test".to_string(),
            severity: Severity::Warning,
            message: "test".to_string(),
            span: verter_span::Span::new(0, 10),
            tags: vec![],
            span_kind: DiagnosticSpanKind::ElementOpenTag,
        }
    }

    #[test]
    fn engine_dispatches_to_providers() {
        let mut engine = ActionEngine::new();
        engine.register(Box::new(TestProvider));

        let diag = make_diag("test-rule");
        let set = DiagnosticSet::new();
        let ctx = ActionContext {
            source: "some source",
            file_id: "/src/App.vue",
            diagnostics: &set,
            template: None,
            script: None,
            styles: &[],
        };

        let actions = engine.fixes_for(&diag, &ctx);
        assert_eq!(actions.len(), 1);
        assert_eq!(actions[0].title, "Fix test-rule");
        assert!(actions[0].is_preferred);
    }

    #[test]
    fn engine_returns_empty_for_unmatched_rule() {
        let mut engine = ActionEngine::new();
        engine.register(Box::new(TestProvider));

        let diag = make_diag("other-rule");
        let set = DiagnosticSet::new();
        let ctx = ActionContext {
            source: "some source",
            file_id: "/src/App.vue",
            diagnostics: &set,
            template: None,
            script: None,
            styles: &[],
        };

        let actions = engine.fixes_for(&diag, &ctx);
        assert!(
            actions.is_empty(),
            "unmatched rule should produce no actions"
        );
    }

    #[test]
    fn empty_engine_returns_no_actions() {
        let engine = ActionEngine::new();
        let diag = make_diag("any");
        let set = DiagnosticSet::new();
        let ctx = ActionContext {
            source: "source",
            file_id: "/src/App.vue",
            diagnostics: &set,
            template: None,
            script: None,
            styles: &[],
        };

        assert!(engine.fixes_for(&diag, &ctx).is_empty());
        assert!(engine.actions_at(5, &ctx).is_empty());
    }
}
