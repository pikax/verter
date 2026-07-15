//! Route semantics analyzer.
//!
//! Classifies component route reachability based on the semantic DB's
//! knowledge of route configuration and file membership.

use serde::{Deserialize, Serialize};

use crate::facts::route::RouteReachabilityStatus;
use crate::facts::symbol::FileImportGraph;

/// Route reachability report for a component.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RouteReachabilityReport {
    pub file_id: String,
    pub status: RouteReachabilityStatus,
    pub reason: String,
}

/// Analyze route reachability for a component.
///
/// Checks whether the file is imported by any known route configuration.
/// This is a simplified check — full route analysis requires the route
/// analysis snapshot from verter_semantic::analysis::routes.
pub fn analyze_route_reachability(
    file_id: &str,
    import_graph: &FileImportGraph,
    known_route_component_ids: &[String],
) -> RouteReachabilityReport {
    if known_route_component_ids.contains(&file_id.to_string()) {
        return RouteReachabilityReport {
            file_id: file_id.to_string(),
            status: RouteReachabilityStatus::Reachable,
            reason: "directly referenced in route configuration".to_string(),
        };
    }

    // Check if any of this file's importers are route components
    // (simplified — full analysis would walk the import graph transitively)
    let is_imported_by_route = import_graph
        .import_sources
        .iter()
        .any(|source| known_route_component_ids.contains(source));

    if is_imported_by_route {
        RouteReachabilityReport {
            file_id: file_id.to_string(),
            status: RouteReachabilityStatus::Reachable,
            reason: "imported by a route component".to_string(),
        }
    } else if known_route_component_ids.is_empty() {
        RouteReachabilityReport {
            file_id: file_id.to_string(),
            status: RouteReachabilityStatus::Unknown,
            reason: "no route configuration available".to_string(),
        }
    } else {
        RouteReachabilityReport {
            file_id: file_id.to_string(),
            status: RouteReachabilityStatus::Unknown,
            reason: "not directly referenced in known routes".to_string(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn direct_route_component_is_reachable() {
        let graph = FileImportGraph::default();
        let routes = vec!["/src/Home.vue".to_string(), "/src/About.vue".to_string()];
        let report = analyze_route_reachability("/src/Home.vue", &graph, &routes);

        assert_eq!(report.status, RouteReachabilityStatus::Reachable);
        assert!(report.reason.contains("route configuration"));
    }

    #[test]
    fn no_routes_is_unknown() {
        let graph = FileImportGraph::default();
        let report = analyze_route_reachability("/src/App.vue", &graph, &[]);

        assert_eq!(report.status, RouteReachabilityStatus::Unknown);
        assert!(report.reason.contains("no route configuration"));
    }

    #[test]
    fn non_route_component_is_unknown() {
        let graph = FileImportGraph::default();
        let routes = vec!["/src/Home.vue".to_string()];
        let report = analyze_route_reachability("/src/Button.vue", &graph, &routes);

        assert_eq!(report.status, RouteReachabilityStatus::Unknown);
    }

    #[test]
    fn imported_by_route_component_is_reachable() {
        let graph = FileImportGraph {
            imports: vec![],
            import_sources: vec!["/src/Home.vue".to_string()],
        };
        let routes = vec!["/src/Home.vue".to_string()];
        let report = analyze_route_reachability("/src/HomeContent.vue", &graph, &routes);

        assert_eq!(report.status, RouteReachabilityStatus::Reachable);
        assert!(report.reason.contains("imported by"));
    }

    #[test]
    fn report_carries_file_id() {
        let graph = FileImportGraph::default();
        let report = analyze_route_reachability("/src/Foo.vue", &graph, &[]);
        assert_eq!(report.file_id, "/src/Foo.vue");
    }

    #[test]
    fn imported_by_non_route_is_unknown() {
        let graph = FileImportGraph {
            imports: vec![],
            import_sources: vec!["/src/Layout.vue".to_string()],
        };
        let routes = vec!["/src/Home.vue".to_string()];
        let report = analyze_route_reachability("/src/Widget.vue", &graph, &routes);

        // Negative: Layout.vue is not a route component
        assert_eq!(report.status, RouteReachabilityStatus::Unknown);
    }

    #[test]
    fn multiple_route_configs_checked() {
        let graph = FileImportGraph::default();
        let routes = vec![
            "/src/Home.vue".to_string(),
            "/src/About.vue".to_string(),
            "/src/Contact.vue".to_string(),
        ];
        let report = analyze_route_reachability("/src/Contact.vue", &graph, &routes);
        assert_eq!(report.status, RouteReachabilityStatus::Reachable);
    }

    // ── Plan-required route coverage ───────────────────────────────────────

    #[test]
    fn same_file_is_route_and_imported_by_route() {
        // A route component importing from another route component
        let graph = FileImportGraph {
            imports: vec![],
            import_sources: vec!["/src/About.vue".to_string()],
        };
        let routes = vec!["/src/Home.vue".to_string(), "/src/About.vue".to_string()];
        let report = analyze_route_reachability("/src/Home.vue", &graph, &routes);

        // Direct match takes priority
        assert_eq!(report.status, RouteReachabilityStatus::Reachable);
        assert!(report.reason.contains("route configuration"));
    }

    #[test]
    fn layout_component_not_in_routes_is_unknown() {
        // Plan: "layout relationships"
        let graph = FileImportGraph::default();
        let routes = vec!["/src/Home.vue".to_string()];
        let report = analyze_route_reachability("/src/layouts/Default.vue", &graph, &routes);

        assert_eq!(report.status, RouteReachabilityStatus::Unknown);
    }

    #[test]
    fn reason_differs_for_direct_vs_imported() {
        let graph = FileImportGraph {
            imports: vec![],
            import_sources: vec!["/src/Home.vue".to_string()],
        };
        let routes = vec!["/src/Home.vue".to_string()];

        let direct = analyze_route_reachability("/src/Home.vue", &graph, &routes);
        let imported = analyze_route_reachability("/src/HomeContent.vue", &graph, &routes);

        assert_ne!(direct.reason, imported.reason);
        assert!(direct.reason.contains("directly"));
        assert!(imported.reason.contains("imported"));
    }

    #[test]
    fn empty_import_sources_with_routes_is_unknown() {
        let graph = FileImportGraph {
            imports: vec![],
            import_sources: vec![],
        };
        let routes = vec!["/src/Home.vue".to_string()];
        let report = analyze_route_reachability("/src/Other.vue", &graph, &routes);

        assert_eq!(report.status, RouteReachabilityStatus::Unknown);
        // Negative: not Reachable even though routes exist
        assert_ne!(report.status, RouteReachabilityStatus::Reachable);
    }
}
