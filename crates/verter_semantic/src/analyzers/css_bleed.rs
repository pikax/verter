//! CSS bleed analyzer — detects non-scoped selectors that may affect
//! other components.
//!
//! Severity follows the plan's policy:
//! - Definite bleed + Definite/Possible co-render → warning
//! - Likely bleed + Possible co-render → hint
//! - Possible bleed or Unknown → trace only

use serde::{Deserialize, Serialize};
use verter_span::Span;

use crate::facts::css::{CssBleedIssue, CssBleedLikelihood, StyleScopeKind};

/// Severity assigned by the diagnostic policy.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum BleedSeverity {
    Warning,
    Hint,
    TraceOnly,
}

/// CSS bleed report for a component.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CssBleedReport {
    pub issues: Vec<CssBleedDiagnostic>,
}

/// A CSS bleed finding with assigned severity.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CssBleedDiagnostic {
    pub issue: CssBleedIssue,
    pub severity: BleedSeverity,
}

/// Classify CSS bleed issues into diagnostic severities.
///
/// Applies the plan's severity policy based on bleed likelihood and
/// co-renderability context.
pub fn classify_bleed_issues(
    issues: &[CssBleedIssue],
    co_render_is_possible: bool,
) -> CssBleedReport {
    let diagnostics = issues
        .iter()
        .map(|issue| {
            let severity = match (issue.likelihood, co_render_is_possible) {
                (CssBleedLikelihood::Definite, true) => BleedSeverity::Warning,
                (CssBleedLikelihood::Definite, false) => BleedSeverity::Hint,
                (CssBleedLikelihood::Likely, true) => BleedSeverity::Hint,
                (CssBleedLikelihood::Likely, false) => BleedSeverity::TraceOnly,
                (CssBleedLikelihood::Possible, _) => BleedSeverity::TraceOnly,
                (CssBleedLikelihood::Unknown, _) => BleedSeverity::TraceOnly,
            };
            CssBleedDiagnostic {
                issue: issue.clone(),
                severity,
            }
        })
        .collect();

    CssBleedReport {
        issues: diagnostics,
    }
}

/// Detect potential CSS bleed issues from style scope analysis.
///
/// Any non-scoped, non-module style block with selectors is a potential bleed.
pub fn detect_bleed_from_scope(
    file_id: &str,
    scope: StyleScopeKind,
    selectors: &[(&str, Span)],
) -> Vec<CssBleedIssue> {
    match scope {
        StyleScopeKind::Global | StyleScopeKind::GlobalEscape => selectors
            .iter()
            .map(|(sel, span)| CssBleedIssue {
                selector: sel.to_string(),
                source_file_id: file_id.to_string(),
                likelihood: if scope == StyleScopeKind::Global {
                    CssBleedLikelihood::Definite
                } else {
                    CssBleedLikelihood::Likely
                },
                scope_kind: scope,
                span: *span,
            })
            .collect(),
        StyleScopeKind::Scoped | StyleScopeKind::Module => Vec::new(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn global_style_is_definite_bleed() {
        let issues = detect_bleed_from_scope(
            "app.vue",
            StyleScopeKind::Global,
            &[(".header", Span::new(10, 20))],
        );

        assert_eq!(issues.len(), 1);
        assert_eq!(issues[0].likelihood, CssBleedLikelihood::Definite);
        assert_eq!(issues[0].selector, ".header");
    }

    #[test]
    fn scoped_style_no_bleed() {
        let issues = detect_bleed_from_scope(
            "app.vue",
            StyleScopeKind::Scoped,
            &[(".header", Span::new(10, 20))],
        );

        assert!(issues.is_empty());
    }

    #[test]
    fn global_escape_is_likely_bleed() {
        let issues = detect_bleed_from_scope(
            "app.vue",
            StyleScopeKind::GlobalEscape,
            &[(".deep-child", Span::new(10, 20))],
        );

        assert_eq!(issues.len(), 1);
        assert_eq!(issues[0].likelihood, CssBleedLikelihood::Likely);
    }

    #[test]
    fn severity_definite_with_co_render_is_warning() {
        let issues = vec![CssBleedIssue {
            selector: ".foo".into(),
            source_file_id: "a.vue".into(),
            likelihood: CssBleedLikelihood::Definite,
            scope_kind: StyleScopeKind::Global,
            span: Span::new(0, 5),
        }];
        let report = classify_bleed_issues(&issues, true);

        assert_eq!(report.issues.len(), 1);
        assert_eq!(report.issues[0].severity, BleedSeverity::Warning);
    }

    #[test]
    fn severity_likely_without_co_render_is_trace() {
        let issues = vec![CssBleedIssue {
            selector: ".bar".into(),
            source_file_id: "b.vue".into(),
            likelihood: CssBleedLikelihood::Likely,
            scope_kind: StyleScopeKind::GlobalEscape,
            span: Span::new(0, 5),
        }];
        let report = classify_bleed_issues(&issues, false);

        assert_eq!(report.issues[0].severity, BleedSeverity::TraceOnly);
    }

    #[test]
    fn severity_possible_always_trace() {
        let issues = vec![CssBleedIssue {
            selector: ".baz".into(),
            source_file_id: "c.vue".into(),
            likelihood: CssBleedLikelihood::Possible,
            scope_kind: StyleScopeKind::Global,
            span: Span::new(0, 5),
        }];

        // Even with co-render possible, "Possible" bleed is trace-only
        let report = classify_bleed_issues(&issues, true);
        assert_eq!(report.issues[0].severity, BleedSeverity::TraceOnly);
    }

    #[test]
    fn module_style_no_bleed() {
        let issues = detect_bleed_from_scope(
            "app.vue",
            StyleScopeKind::Module,
            &[(".header", Span::new(10, 20))],
        );
        assert!(issues.is_empty());
    }

    #[test]
    fn multiple_selectors_all_reported() {
        let issues = detect_bleed_from_scope(
            "app.vue",
            StyleScopeKind::Global,
            &[
                (".a", Span::new(10, 12)),
                (".b", Span::new(20, 22)),
                ("#c", Span::new(30, 32)),
            ],
        );
        assert_eq!(issues.len(), 3);
        assert!(issues
            .iter()
            .all(|i| i.likelihood == CssBleedLikelihood::Definite));
    }

    #[test]
    fn empty_selectors_no_issues() {
        let issues = detect_bleed_from_scope("app.vue", StyleScopeKind::Global, &[]);
        assert!(issues.is_empty());
    }

    #[test]
    fn definite_without_co_render_is_hint() {
        let issues = vec![CssBleedIssue {
            selector: ".x".into(),
            source_file_id: "a.vue".into(),
            likelihood: CssBleedLikelihood::Definite,
            scope_kind: StyleScopeKind::Global,
            span: Span::new(0, 5),
        }];
        let report = classify_bleed_issues(&issues, false);
        assert_eq!(report.issues[0].severity, BleedSeverity::Hint);
    }

    #[test]
    fn likely_with_co_render_is_hint() {
        let issues = vec![CssBleedIssue {
            selector: ".y".into(),
            source_file_id: "a.vue".into(),
            likelihood: CssBleedLikelihood::Likely,
            scope_kind: StyleScopeKind::GlobalEscape,
            span: Span::new(0, 5),
        }];
        let report = classify_bleed_issues(&issues, true);
        assert_eq!(report.issues[0].severity, BleedSeverity::Hint);
    }

    #[test]
    fn unknown_likelihood_always_trace() {
        let issues = vec![CssBleedIssue {
            selector: ".z".into(),
            source_file_id: "a.vue".into(),
            likelihood: CssBleedLikelihood::Unknown,
            scope_kind: StyleScopeKind::Global,
            span: Span::new(0, 5),
        }];
        let report = classify_bleed_issues(&issues, true);
        assert_eq!(report.issues[0].severity, BleedSeverity::TraceOnly);
    }
}
