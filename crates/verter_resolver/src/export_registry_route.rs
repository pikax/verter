use rustc_hash::{FxHashMap, FxHashSet};
use verter_vfs::ResolveRequestKind;

use crate::ResolverHash16;

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

    let target = resolve_type_via_registry_inner(
        resolver,
        canonical,
        type_name,
        kind,
        visited,
        &mut tracked_deps,
        &mut route_hashes,
    );

    RegistryRoute {
        target,
        tracked_deps,
        route_hashes,
    }
}

fn resolve_type_via_registry_inner<R: RegistryRouteResolver>(
    resolver: &R,
    canonical: &str,
    type_name: &str,
    kind: ResolveRequestKind,
    visited: &mut FxHashSet<(String, String)>,
    tracked_deps: &mut Vec<String>,
    route_hashes: &mut Vec<(String, ResolverHash16)>,
) -> Option<RegistryResolvedTarget> {
    if !visited.insert((canonical.to_string(), type_name.to_string())) {
        return None;
    }

    let registry = resolver.ensure_export_registry(canonical)?;
    tracked_deps.push(canonical.to_string());
    route_hashes.push((canonical.to_string(), registry.source_hash));

    if let Some(entry) = registry.named.get(type_name) {
        return match entry {
            RegistryExportEntry::Defined => Some(RegistryResolvedTarget {
                final_canonical_id: canonical.to_string(),
                exported_name: type_name.to_string(),
            }),
            RegistryExportEntry::Alias {
                source_specifier,
                original_name,
            } => {
                let source_canonical = resolver.resolve_loaded_dependency_canonical(
                    canonical,
                    source_specifier,
                    kind,
                )?;
                resolve_type_via_registry_inner(
                    resolver,
                    &source_canonical,
                    original_name,
                    kind,
                    visited,
                    tracked_deps,
                    route_hashes,
                )
            }
        };
    }

    let mut bfs_queue = std::collections::VecDeque::new();

    for specifier in &registry.wildcard_edges {
        let Some(child_canonical) =
            resolver.resolve_loaded_dependency_canonical(canonical, specifier, kind)
        else {
            continue;
        };

        let Some(child_registry) = resolver.ensure_export_registry(&child_canonical) else {
            continue;
        };

        tracked_deps.push(child_canonical.clone());
        route_hashes.push((child_canonical.clone(), child_registry.source_hash));

        if let Some(entry) = child_registry.named.get(type_name) {
            visited.insert((child_canonical.clone(), type_name.to_string()));
            return match entry {
                RegistryExportEntry::Defined => Some(RegistryResolvedTarget {
                    final_canonical_id: child_canonical,
                    exported_name: type_name.to_string(),
                }),
                RegistryExportEntry::Alias {
                    source_specifier,
                    original_name,
                } => {
                    let alias_canonical = resolver.resolve_loaded_dependency_canonical(
                        &child_canonical,
                        source_specifier,
                        kind,
                    )?;
                    resolve_type_via_registry_inner(
                        resolver,
                        &alias_canonical,
                        original_name,
                        kind,
                        visited,
                        tracked_deps,
                        route_hashes,
                    )
                }
            };
        }

        for child_specifier in &child_registry.wildcard_edges {
            if let Some(grandchild_canonical) = resolver.resolve_loaded_dependency_canonical(
                &child_canonical,
                child_specifier,
                kind,
            ) {
                if !visited.contains(&(grandchild_canonical.clone(), type_name.to_string())) {
                    bfs_queue.push_back(grandchild_canonical);
                }
            }
        }
    }

    while let Some(next_canonical) = bfs_queue.pop_front() {
        if !visited.insert((next_canonical.clone(), type_name.to_string())) {
            continue;
        }

        let Some(next_registry) = resolver.ensure_export_registry(&next_canonical) else {
            continue;
        };

        tracked_deps.push(next_canonical.clone());
        route_hashes.push((next_canonical.clone(), next_registry.source_hash));

        if let Some(entry) = next_registry.named.get(type_name) {
            return match entry {
                RegistryExportEntry::Defined => Some(RegistryResolvedTarget {
                    final_canonical_id: next_canonical,
                    exported_name: type_name.to_string(),
                }),
                RegistryExportEntry::Alias {
                    source_specifier,
                    original_name,
                } => {
                    let alias_canonical = resolver.resolve_loaded_dependency_canonical(
                        &next_canonical,
                        source_specifier,
                        kind,
                    )?;
                    resolve_type_via_registry_inner(
                        resolver,
                        &alias_canonical,
                        original_name,
                        kind,
                        visited,
                        tracked_deps,
                        route_hashes,
                    )
                }
            };
        }

        for specifier in &next_registry.wildcard_edges {
            if let Some(grandchild_canonical) =
                resolver.resolve_loaded_dependency_canonical(&next_canonical, specifier, kind)
            {
                if !visited.contains(&(grandchild_canonical.clone(), type_name.to_string())) {
                    bfs_queue.push_back(grandchild_canonical);
                }
            }
        }
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
    use std::collections::BTreeMap;
    use verter_vfs::ResolveRequestKind;

    #[derive(Default)]
    struct TestResolver {
        registries: BTreeMap<String, ExportRegistryView>,
        routes: BTreeMap<(String, String), String>,
    }

    impl RegistryRouteResolver for TestResolver {
        fn ensure_export_registry(&self, canonical: &str) -> Option<ExportRegistryView> {
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
}
