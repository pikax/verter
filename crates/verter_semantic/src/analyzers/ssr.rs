//! SSR readiness analyzer.
//!
//! Classifies components as SSR-compatible, incompatible, or conditional
//! based on browser-only API usage in their script blocks.

use serde::{Deserialize, Serialize};
use verter_span::Span;

use crate::facts::binding::BindingDeclaration;
use crate::facts::reactivity::ReactivityFact;
use crate::facts::route::SsrReadinessStatus;
use crate::facts::symbol::FileImportGraph;

/// Known browser-only globals that break SSR.
const BROWSER_ONLY_GLOBALS: &[&str] = &[
    "window",
    "document",
    "navigator",
    "location",
    "localStorage",
    "sessionStorage",
    "history",
    "alert",
    "confirm",
    "prompt",
    "XMLHttpRequest",
    "IntersectionObserver",
    "MutationObserver",
    "ResizeObserver",
    "requestAnimationFrame",
    "cancelAnimationFrame",
    "getComputedStyle",
    "matchMedia",
];

/// SSR readiness report for a component.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SsrReadinessReport {
    pub status: SsrReadinessStatus,
    pub issues: Vec<SsrIssue>,
}

/// A specific SSR compatibility issue.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SsrIssue {
    pub binding_name: String,
    pub reason: String,
    pub span: Span,
}

/// Analyze a component's SSR readiness.
///
/// Examines bindings for references to browser-only globals. A component
/// that accesses `document`, `window`, etc. at the top level of script
/// setup is SSR-incompatible.
pub fn analyze_ssr_readiness(
    bindings: &[(BindingDeclaration, ReactivityFact)],
    import_graph: &FileImportGraph,
) -> SsrReadinessReport {
    let mut issues = Vec::new();

    // Check bindings for browser-only global references
    for (decl, _) in bindings {
        if BROWSER_ONLY_GLOBALS.contains(&decl.name.as_str()) {
            issues.push(SsrIssue {
                binding_name: decl.name.clone(),
                reason: format!("`{}` is a browser-only global", decl.name),
                span: decl.span,
            });
        }
    }

    // Check imports for known browser-only packages
    for sym in &import_graph.imports {
        if is_browser_only_import(&sym.source_specifier) {
            issues.push(SsrIssue {
                binding_name: sym.local_name.clone(),
                reason: format!("import from `{}` is browser-only", sym.source_specifier),
                span: sym.span,
            });
        }
    }

    let status = if issues.is_empty() {
        SsrReadinessStatus::Compatible
    } else {
        SsrReadinessStatus::Incompatible
    };

    SsrReadinessReport { status, issues }
}

fn is_browser_only_import(specifier: &str) -> bool {
    // Common browser-only packages
    specifier == "intersection-observer"
        || specifier.starts_with("@vueuse/") && specifier.contains("browser")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::facts::binding::BindingKind;

    fn make_binding(name: &str) -> (BindingDeclaration, ReactivityFact) {
        (
            BindingDeclaration {
                name: name.into(),
                kind: BindingKind::Const,
                span: Span::new(0, 10),
                usages: vec![],
            },
            ReactivityFact::non_reactive(),
        )
    }

    #[test]
    fn compatible_when_no_browser_apis() {
        let bindings = vec![make_binding("count"), make_binding("message")];
        let graph = FileImportGraph::default();
        let report = analyze_ssr_readiness(&bindings, &graph);

        assert_eq!(report.status, SsrReadinessStatus::Compatible);
        assert!(report.issues.is_empty());
    }

    #[test]
    fn incompatible_when_uses_document() {
        let bindings = vec![make_binding("document")];
        let graph = FileImportGraph::default();
        let report = analyze_ssr_readiness(&bindings, &graph);

        assert_eq!(report.status, SsrReadinessStatus::Incompatible);
        assert_eq!(report.issues.len(), 1);
        assert!(report.issues[0].reason.contains("browser-only global"));
    }

    #[test]
    fn incompatible_when_uses_window() {
        let bindings = vec![make_binding("window"), make_binding("count")];
        let graph = FileImportGraph::default();
        let report = analyze_ssr_readiness(&bindings, &graph);

        // Positive: detects window
        assert_eq!(report.status, SsrReadinessStatus::Incompatible);
        assert_eq!(report.issues.len(), 1);
        assert_eq!(report.issues[0].binding_name, "window");

        // Negative: count is not flagged
        assert!(!report.issues.iter().any(|i| i.binding_name == "count"));
    }

    #[test]
    fn normal_bindings_not_flagged() {
        let bindings = vec![
            make_binding("ref"),
            make_binding("computed"),
            make_binding("onMounted"),
        ];
        let graph = FileImportGraph::default();
        let report = analyze_ssr_readiness(&bindings, &graph);

        assert_eq!(report.status, SsrReadinessStatus::Compatible);
    }

    #[test]
    fn empty_file_is_compatible() {
        let report = analyze_ssr_readiness(&[], &FileImportGraph::default());
        assert_eq!(report.status, SsrReadinessStatus::Compatible);
        assert!(report.issues.is_empty());
    }

    #[test]
    fn multiple_browser_globals_all_reported() {
        let bindings = vec![
            make_binding("window"),
            make_binding("document"),
            make_binding("localStorage"),
        ];
        let report = analyze_ssr_readiness(&bindings, &FileImportGraph::default());

        assert_eq!(report.status, SsrReadinessStatus::Incompatible);
        assert_eq!(report.issues.len(), 3);
        let names: Vec<_> = report
            .issues
            .iter()
            .map(|i| i.binding_name.as_str())
            .collect();
        assert!(names.contains(&"window"));
        assert!(names.contains(&"document"));
        assert!(names.contains(&"localStorage"));
    }

    #[test]
    fn navigator_and_history_detected() {
        let bindings = vec![make_binding("navigator"), make_binding("history")];
        let report = analyze_ssr_readiness(&bindings, &FileImportGraph::default());
        assert_eq!(report.status, SsrReadinessStatus::Incompatible);
        assert_eq!(report.issues.len(), 2);
    }

    #[test]
    fn request_animation_frame_detected() {
        let bindings = vec![make_binding("requestAnimationFrame")];
        let report = analyze_ssr_readiness(&bindings, &FileImportGraph::default());
        assert_eq!(report.status, SsrReadinessStatus::Incompatible);
    }

    #[test]
    fn intersection_observer_detected() {
        let bindings = vec![make_binding("IntersectionObserver")];
        let report = analyze_ssr_readiness(&bindings, &FileImportGraph::default());
        assert_eq!(report.status, SsrReadinessStatus::Incompatible);
    }
}
