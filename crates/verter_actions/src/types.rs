//! Core action types: `CodeAction`, `FileEdit`, `ActionKind`.

/// A code fix or refactoring action.
#[derive(Debug, Clone)]
pub struct CodeAction {
    /// Human-readable title shown in the IDE.
    pub title: String,
    /// Kind of action (quick fix, refactor, source).
    pub kind: ActionKind,
    /// Text edits to apply.
    pub edits: Vec<FileEdit>,
    /// Whether this is the preferred action for the diagnostic.
    pub is_preferred: bool,
    /// The diagnostic rule this fixes, if any.
    pub diagnostic_rule: Option<String>,
}

/// Kind of code action.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ActionKind {
    /// Fix for a specific diagnostic.
    QuickFix,
    /// Code transformation (extract, inline, rename).
    Refactor,
    /// File-level (organize imports, format).
    Source,
}

/// A single text edit, possibly cross-file.
#[derive(Debug, Clone)]
pub struct FileEdit {
    /// File identifier. `None` = current file.
    pub file_id: Option<String>,
    /// Replacement text.
    pub replacement: String,
    /// SFC-absolute byte offset span.
    pub span: verter_span::Span,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn code_action_construction() {
        let action = CodeAction {
            title: "Remove unused selector".to_string(),
            kind: ActionKind::QuickFix,
            edits: vec![FileEdit {
                file_id: None,
                replacement: String::new(),
                span: verter_span::Span::new(10, 30),
            }],
            is_preferred: true,
            diagnostic_rule: Some("unused-css-selector".to_string()),
        };

        assert_eq!(action.kind, ActionKind::QuickFix);
        assert!(action.is_preferred);
        assert_eq!(action.edits.len(), 1);
        assert!(
            action.edits[0].file_id.is_none(),
            "current file edit should have None file_id"
        );
        assert_eq!(
            action.diagnostic_rule.as_deref(),
            Some("unused-css-selector")
        );
    }

    #[test]
    fn cross_file_edit() {
        let edit = FileEdit {
            file_id: Some("/src/Child.vue".to_string()),
            replacement: "newProp".to_string(),
            span: verter_span::Span::new(50, 60),
        };

        assert!(
            edit.file_id.is_some(),
            "cross-file edit should have file_id"
        );
    }
}
