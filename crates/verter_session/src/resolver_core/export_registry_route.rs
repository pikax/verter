use rustc_hash::{FxHashMap, FxHashSet};
use verter_workspace::ResolveRequestKind;

use crate::resolver_core::ResolverHash16;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RegistryExportEntry {
    Defined,
    Alias {
        source_specifier: String,
        original_name: String,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExportRegistryView {
    pub source_hash: ResolverHash16,
    pub named: FxHashMap<String, RegistryExportEntry>,
    pub wildcard_edges: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RegistryResolvedTarget {
    pub final_canonical_id: String,
    pub exported_name: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RegistryRoute {
    pub target: Option<RegistryResolvedTarget>,
    pub tracked_deps: Vec<String>,
    pub route_hashes: Vec<(String, ResolverHash16)>,
}

pub trait RegistryRouteResolver {
    fn ensure_export_registry(&self, canonical: &str) -> Option<ExportRegistryView>;

    fn resolve_loaded_dependency_canonical(
        &self,
        canonical: &str,
        source_specifier: &str,
        kind: ResolveRequestKind,
    ) -> Option<String>;

    fn note_barrel_fact_reuse(&self) {}
}

struct RegistryRouteState<'a> {
    visited: &'a mut FxHashSet<(String, String)>,
    tracked_deps: &'a mut Vec<String>,
    route_hashes: &'a mut Vec<(String, ResolverHash16)>,
}

pub fn resolve_type_via_registry<R: RegistryRouteResolver>(
    resolver: &R,
    canonical: &str,
    type_name: &str,
    kind: ResolveRequestKind,
    visited: &mut FxHashSet<(String, String)>,
) -> RegistryRoute {
    let mut tracked_deps = Vec::new();
    let mut route_hashes = Vec::new();
    let mut state = RegistryRouteState {
        visited,
        tracked_deps: &mut tracked_deps,
        route_hashes: &mut route_hashes,
    };

    let target = resolve_type_via_registry_inner(resolver, canonical, type_name, kind, &mut state);

    RegistryRoute {
        target,
        tracked_deps,
        route_hashes,
    }
}

fn resolve_registry_entry_target<R: RegistryRouteResolver>(
    resolver: &R,
    canonical: &str,
    requested_name: &str,
    entry: &RegistryExportEntry,
    kind: ResolveRequestKind,
    state: &mut RegistryRouteState<'_>,
) -> Option<RegistryResolvedTarget> {
    match entry {
        RegistryExportEntry::Defined => Some(RegistryResolvedTarget {
            final_canonical_id: canonical.to_string(),
            exported_name: requested_name.to_string(),
        }),
        RegistryExportEntry::Alias {
            source_specifier,
            original_name,
        } => {
            let source_canonical =
                resolver.resolve_loaded_dependency_canonical(canonical, source_specifier, kind)?;
            resolve_type_via_registry_inner(resolver, &source_canonical, original_name, kind, state)
        }
    }
}

fn resolve_type_via_registry_inner<R: RegistryRouteResolver>(
    resolver: &R,
    canonical: &str,
    type_name: &str,
    kind: ResolveRequestKind,
    state: &mut RegistryRouteState<'_>,
) -> Option<RegistryResolvedTarget> {
    if !state
        .visited
        .insert((canonical.to_string(), type_name.to_string()))
    {
        return None;
    }

    let registry = resolver.ensure_export_registry(canonical)?;
    state.tracked_deps.push(canonical.to_string());
    state
        .route_hashes
        .push((canonical.to_string(), registry.source_hash));
    if !registry.wildcard_edges.is_empty() {
        resolver.note_barrel_fact_reuse();
    }

    if let Some(entry) = registry.named.get(type_name) {
        return resolve_registry_entry_target(resolver, canonical, type_name, entry, kind, state);
    }

    let requested_name = type_name.to_string();
    let mut current_level = Vec::new();
    let mut current_level_seen = FxHashSet::default();
    for specifier in &registry.wildcard_edges {
        let Some(child_canonical) =
            resolver.resolve_loaded_dependency_canonical(canonical, specifier, kind)
        else {
            continue;
        };
        let visit_key = (child_canonical.clone(), requested_name.clone());
        if !state.visited.contains(&visit_key) && current_level_seen.insert(child_canonical.clone())
        {
            state.visited.insert(visit_key);
            current_level.push(child_canonical);
        }
    }

    while !current_level.is_empty() {
        let mut next_level = Vec::new();
        let mut next_level_seen = FxHashSet::default();

        for child_canonical in current_level {
            let Some(child_registry) = resolver.ensure_export_registry(&child_canonical) else {
                continue;
            };
            state.tracked_deps.push(child_canonical.clone());
            state
                .route_hashes
                .push((child_canonical.clone(), child_registry.source_hash));
            if !child_registry.wildcard_edges.is_empty() {
                resolver.note_barrel_fact_reuse();
            }
            if let Some(entry) = child_registry.named.get(type_name) {
                return resolve_registry_entry_target(
                    resolver,
                    &child_canonical,
                    type_name,
                    entry,
                    kind,
                    state,
                );
            }

            for specifier in &child_registry.wildcard_edges {
                if let Some(grandchild_canonical) =
                    resolver.resolve_loaded_dependency_canonical(&child_canonical, specifier, kind)
                {
                    let visit_key = (grandchild_canonical.clone(), requested_name.clone());
                    if !state.visited.contains(&visit_key)
                        && next_level_seen.insert(grandchild_canonical.clone())
                    {
                        state.visited.insert(visit_key);
                        next_level.push(grandchild_canonical);
                    }
                }
            }
        }

        current_level = next_level;
    }

    None
}

#[cfg(test)]
mod tests {
    use super::{
        resolve_type_via_registry, ExportRegistryView, RegistryExportEntry, RegistryResolvedTarget,
        RegistryRouteResolver,
    };
    use rustc_hash::{FxHashMap, FxHashSet};
    use std::cell::RefCell;
    use std::collections::BTreeMap;
    use verter_workspace::ResolveRequestKind;

    #[derive(Default)]
    struct TestResolver {
        registries: BTreeMap<String, ExportRegistryView>,
        routes: BTreeMap<(String, String), String>,
        registry_lookups: RefCell<Vec<String>>,
    }

    impl RegistryRouteResolver for TestResolver {
        fn ensure_export_registry(&self, canonical: &str) -> Option<ExportRegistryView> {
            self.registry_lookups
                .borrow_mut()
                .push(canonical.to_string());
            self.registries.get(canonical).cloned()
        }

        fn resolve_loaded_dependency_canonical(
            &self,
            canonical: &str,
            source_specifier: &str,
            _kind: ResolveRequestKind,
        ) -> Option<String> {
            self.routes
                .get(&(canonical.to_string(), source_specifier.to_string()))
                .cloned()
        }
    }

    #[test]
    fn resolve_type_via_registry_follows_alias_chain() {
        let mut resolver = TestResolver::default();
        resolver.registries.insert(
            "/src/index.ts".to_string(),
            ExportRegistryView {
                source_hash: [1; 16],
                named: FxHashMap::from_iter([(
                    "Props".to_string(),
                    RegistryExportEntry::Alias {
                        source_specifier: "./types".to_string(),
                        original_name: "Props".to_string(),
                    },
                )]),
                wildcard_edges: Vec::new(),
            },
        );
        resolver.registries.insert(
            "/src/types.ts".to_string(),
            ExportRegistryView {
                source_hash: [2; 16],
                named: FxHashMap::from_iter([("Props".to_string(), RegistryExportEntry::Defined)]),
                wildcard_edges: Vec::new(),
            },
        );
        resolver.routes.insert(
            ("/src/index.ts".to_string(), "./types".to_string()),
            "/src/types.ts".to_string(),
        );

        let mut visited = FxHashSet::default();
        let route = resolve_type_via_registry(
            &resolver,
            "/src/index.ts",
            "Props",
            ResolveRequestKind::TypeImport,
            &mut visited,
        );

        assert_eq!(
            route.target,
            Some(RegistryResolvedTarget {
                final_canonical_id: "/src/types.ts".to_string(),
                exported_name: "Props".to_string(),
            })
        );
        assert_eq!(route.tracked_deps, vec!["/src/index.ts", "/src/types.ts"]);
    }

    #[test]
    fn resolve_type_via_registry_walks_wildcard_bfs() {
        let mut resolver = TestResolver::default();
        resolver.registries.insert(
            "/src/index.ts".to_string(),
            ExportRegistryView {
                source_hash: [1; 16],
                named: FxHashMap::default(),
                wildcard_edges: vec!["./a".to_string(), "./b".to_string()],
            },
        );
        resolver.registries.insert(
            "/src/a.ts".to_string(),
            ExportRegistryView {
                source_hash: [2; 16],
                named: FxHashMap::default(),
                wildcard_edges: vec!["./deep".to_string()],
            },
        );
        resolver.registries.insert(
            "/src/b.ts".to_string(),
            ExportRegistryView {
                source_hash: [3; 16],
                named: FxHashMap::from_iter([("Props".to_string(), RegistryExportEntry::Defined)]),
                wildcard_edges: Vec::new(),
            },
        );
        resolver.registries.insert(
            "/src/deep.ts".to_string(),
            ExportRegistryView {
                source_hash: [4; 16],
                named: FxHashMap::from_iter([("Props".to_string(), RegistryExportEntry::Defined)]),
                wildcard_edges: Vec::new(),
            },
        );
        resolver.routes.insert(
            ("/src/index.ts".to_string(), "./a".to_string()),
            "/src/a.ts".to_string(),
        );
        resolver.routes.insert(
            ("/src/index.ts".to_string(), "./b".to_string()),
            "/src/b.ts".to_string(),
        );
        resolver.routes.insert(
            ("/src/a.ts".to_string(), "./deep".to_string()),
            "/src/deep.ts".to_string(),
        );

        let mut visited = FxHashSet::default();
        let route = resolve_type_via_registry(
            &resolver,
            "/src/index.ts",
            "Props",
            ResolveRequestKind::TypeImport,
            &mut visited,
        );

        assert_eq!(
            route.target,
            Some(RegistryResolvedTarget {
                final_canonical_id: "/src/b.ts".to_string(),
                exported_name: "Props".to_string(),
            })
        );
    }

    #[test]
    fn resolve_type_via_registry_preserves_wildcard_source_order_for_duplicate_exports() {
        let mut resolver = TestResolver::default();
        resolver.registries.insert(
            "/src/types/index.ts".to_string(),
            ExportRegistryView {
                source_hash: [1; 16],
                named: FxHashMap::default(),
                wildcard_edges: vec!["./legacy".to_string(), "./Button.vue".to_string()],
            },
        );
        resolver.registries.insert(
            "/src/components/Legacy.ts".to_string(),
            ExportRegistryView {
                source_hash: [2; 16],
                named: FxHashMap::from_iter([(
                    "ButtonProps".to_string(),
                    RegistryExportEntry::Defined,
                )]),
                wildcard_edges: Vec::new(),
            },
        );
        resolver.registries.insert(
            "/src/components/Button.vue".to_string(),
            ExportRegistryView {
                source_hash: [3; 16],
                named: FxHashMap::from_iter([(
                    "ButtonProps".to_string(),
                    RegistryExportEntry::Defined,
                )]),
                wildcard_edges: Vec::new(),
            },
        );
        resolver.routes.insert(
            ("/src/types/index.ts".to_string(), "./legacy".to_string()),
            "/src/components/Legacy.ts".to_string(),
        );
        resolver.routes.insert(
            (
                "/src/types/index.ts".to_string(),
                "./Button.vue".to_string(),
            ),
            "/src/components/Button.vue".to_string(),
        );

        let mut visited = FxHashSet::default();
        let route = resolve_type_via_registry(
            &resolver,
            "/src/types/index.ts",
            "ButtonProps",
            ResolveRequestKind::TypeImport,
            &mut visited,
        );

        assert_eq!(
            route.target,
            Some(RegistryResolvedTarget {
                final_canonical_id: "/src/components/Legacy.ts".to_string(),
                exported_name: "ButtonProps".to_string(),
            })
        );
    }

    #[test]
    fn resolve_type_via_registry_stops_after_first_same_level_match() {
        let mut resolver = TestResolver::default();
        resolver.registries.insert(
            "/src/index.ts".to_string(),
            ExportRegistryView {
                source_hash: [1; 16],
                named: FxHashMap::default(),
                wildcard_edges: vec!["./a".to_string(), "./b".to_string(), "./c".to_string()],
            },
        );
        resolver.registries.insert(
            "/src/a.ts".to_string(),
            ExportRegistryView {
                source_hash: [2; 16],
                named: FxHashMap::from_iter([("Props".to_string(), RegistryExportEntry::Defined)]),
                wildcard_edges: vec!["./deep".to_string()],
            },
        );
        resolver.registries.insert(
            "/src/b.ts".to_string(),
            ExportRegistryView {
                source_hash: [3; 16],
                named: FxHashMap::default(),
                wildcard_edges: Vec::new(),
            },
        );
        resolver.registries.insert(
            "/src/c.ts".to_string(),
            ExportRegistryView {
                source_hash: [4; 16],
                named: FxHashMap::default(),
                wildcard_edges: Vec::new(),
            },
        );
        resolver.registries.insert(
            "/src/deep.ts".to_string(),
            ExportRegistryView {
                source_hash: [5; 16],
                named: FxHashMap::from_iter([("Props".to_string(), RegistryExportEntry::Defined)]),
                wildcard_edges: Vec::new(),
            },
        );
        resolver.routes.insert(
            ("/src/index.ts".to_string(), "./a".to_string()),
            "/src/a.ts".to_string(),
        );
        resolver.routes.insert(
            ("/src/index.ts".to_string(), "./b".to_string()),
            "/src/b.ts".to_string(),
        );
        resolver.routes.insert(
            ("/src/index.ts".to_string(), "./c".to_string()),
            "/src/c.ts".to_string(),
        );
        resolver.routes.insert(
            ("/src/a.ts".to_string(), "./deep".to_string()),
            "/src/deep.ts".to_string(),
        );

        let mut visited = FxHashSet::default();
        let route = resolve_type_via_registry(
            &resolver,
            "/src/index.ts",
            "Props",
            ResolveRequestKind::TypeImport,
            &mut visited,
        );

        assert_eq!(
            route.target,
            Some(RegistryResolvedTarget {
                final_canonical_id: "/src/a.ts".to_string(),
                exported_name: "Props".to_string(),
            }),
            "the first same-level match should still win",
        );
        assert_eq!(
            resolver.registry_lookups.borrow().clone(),
            vec!["/src/index.ts".to_string(), "/src/a.ts".to_string(),],
            "BFS should stop once the first same-level match is found",
        );
        assert_eq!(
            route.tracked_deps,
            vec!["/src/index.ts".to_string(), "/src/a.ts".to_string(),],
            "tracked deps should reflect only the inspected winning frontier",
        );
        assert!(
            !route.tracked_deps.contains(&"/src/deep.ts".to_string()),
            "queued descendants below the winning level must not be tracked",
        );
    }

    #[test]
    fn resolve_type_via_registry_tracks_only_inspected_frontier_for_matching_level() {
        let mut resolver = TestResolver::default();
        resolver.registries.insert(
            "/src/index.ts".to_string(),
            ExportRegistryView {
                source_hash: [1; 16],
                named: FxHashMap::default(),
                wildcard_edges: vec!["./a".to_string(), "./b".to_string()],
            },
        );
        resolver.registries.insert(
            "/src/a.ts".to_string(),
            ExportRegistryView {
                source_hash: [2; 16],
                named: FxHashMap::default(),
                wildcard_edges: vec!["./deep-a".to_string()],
            },
        );
        resolver.registries.insert(
            "/src/b.ts".to_string(),
            ExportRegistryView {
                source_hash: [3; 16],
                named: FxHashMap::from_iter([("Props".to_string(), RegistryExportEntry::Defined)]),
                wildcard_edges: vec!["./deep-b".to_string()],
            },
        );
        resolver.registries.insert(
            "/src/deep-a.ts".to_string(),
            ExportRegistryView {
                source_hash: [4; 16],
                named: FxHashMap::default(),
                wildcard_edges: Vec::new(),
            },
        );
        resolver.registries.insert(
            "/src/deep-b.ts".to_string(),
            ExportRegistryView {
                source_hash: [5; 16],
                named: FxHashMap::default(),
                wildcard_edges: Vec::new(),
            },
        );
        resolver.routes.insert(
            ("/src/index.ts".to_string(), "./a".to_string()),
            "/src/a.ts".to_string(),
        );
        resolver.routes.insert(
            ("/src/index.ts".to_string(), "./b".to_string()),
            "/src/b.ts".to_string(),
        );
        resolver.routes.insert(
            ("/src/a.ts".to_string(), "./deep-a".to_string()),
            "/src/deep-a.ts".to_string(),
        );
        resolver.routes.insert(
            ("/src/b.ts".to_string(), "./deep-b".to_string()),
            "/src/deep-b.ts".to_string(),
        );

        let mut visited = FxHashSet::default();
        let route = resolve_type_via_registry(
            &resolver,
            "/src/index.ts",
            "Props",
            ResolveRequestKind::TypeImport,
            &mut visited,
        );

        assert_eq!(
            route.target,
            Some(RegistryResolvedTarget {
                final_canonical_id: "/src/b.ts".to_string(),
                exported_name: "Props".to_string(),
            }),
        );
        assert_eq!(
            route.tracked_deps,
            vec![
                "/src/index.ts".to_string(),
                "/src/a.ts".to_string(),
                "/src/b.ts".to_string(),
            ],
            "matching-level tracking should include only the inspected frontier and exclude queued descendants",
        );
        assert!(
            !route
                .tracked_deps
                .iter()
                .any(|dep| dep == "/src/deep-a.ts" || dep == "/src/deep-b.ts"),
            "queued descendants below the winning level must stay out of tracked deps",
        );
    }
}
