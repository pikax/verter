//! Lint diagnostic types.

/// Severity level for a lint diagnostic.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Severity {
    /// Error: must fix.
    Error,
    /// Warning: should fix.
    Warning,
    /// Informational: style suggestion.
    Info,
}

/// A single lint diagnostic emitted by a rule.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LintDiagnostic {
    /// Rule name (e.g., `"require-v-for-key"`).
    pub rule: String,
    /// Rule category for grouping.
    pub category: String,
    /// Severity level.
    pub severity: Severity,
    /// Human-readable message.
    pub message: String,
    /// Byte offset start in the SFC source.
    pub span_start: u32,
    /// Byte offset end in the SFC source.
    pub span_end: u32,
    /// Optional fix suggestion.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub fix: Option<LintFix>,
}

/// A suggested fix for a lint diagnostic.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LintFix {
    /// Description of the fix.
    pub description: String,
    /// Replacement text.
    pub replacement: String,
    /// Byte offset start of the range to replace.
    pub span_start: u32,
    /// Byte offset end of the range to replace.
    pub span_end: u32,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn diagnostic_serde_roundtrip() {
        let diag = LintDiagnostic {
            rule: "require-v-for-key".to_string(),
            category: "vue-essential".to_string(),
            severity: Severity::Error,
            message: "Elements in iteration expect to have 'v-bind:key' directives.".to_string(),
            span_start: 10,
            span_end: 40,
            fix: None,
        };

        let json = serde_json::to_string(&diag).expect("serialize");
        let roundtrip: LintDiagnostic = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(diag, roundtrip);
    }

    #[test]
    fn diagnostic_with_fix_serde_roundtrip() {
        let diag = LintDiagnostic {
            rule: "v-bind-style".to_string(),
            category: "vue-recommended".to_string(),
            severity: Severity::Warning,
            message: "Unexpected 'v-bind' before ':'".to_string(),
            span_start: 5,
            span_end: 20,
            fix: Some(LintFix {
                description: "Replace 'v-bind:' with ':'".to_string(),
                replacement: ":class".to_string(),
                span_start: 5,
                span_end: 17,
            }),
        };

        let json = serde_json::to_string(&diag).expect("serialize");
        let roundtrip: LintDiagnostic = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(diag, roundtrip);
    }
}
