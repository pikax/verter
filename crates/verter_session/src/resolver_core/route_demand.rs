//! Shared route-demand model for routed-symbol resolution.
//!
//! This module owns the `RouteDemand` type that represents how much of
//! an exported symbol's dependency graph is needed. It replaces the
//! former `ExportedRoute` in `shallow_file_state.rs` and is consumed by
//! `ShallowFileState`, `ExternalTypeFrontier`, `meta_resolve`,
//! `component_meta_query_engine`, and fallthrough routing.
//!
//! See architectural rule 2: "Route demand is a shared resolver-core type."

use std::hash::{Hash, Hasher};

/// How much of an exported symbol's dependency graph is needed.
///
/// `RouteDemand` is the single authority for route shape in the resolver.
/// All consumers (frontier, shallow state, component-meta, fallthrough)
/// must use this type — no consumer-local route-shape types allowed.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub enum RouteDemand {
    /// Full export — all dependencies.
    #[default]
    Whole,
    /// Indexed member path: `Type['a']['b']`.
    /// Each element is one path segment. Must not be collapsed to a
    /// shorter prefix during routing, caching, or materialization.
    MemberPath(Vec<String>),
    /// Pick subset: `Pick<Type, 'a' | 'b'>`.
    Pick(Vec<String>),
    /// Omit subset: `Omit<Type, 'a' | 'b'>`.
    Omit(Vec<String>),
}

impl Hash for RouteDemand {
    fn hash<H: Hasher>(&self, state: &mut H) {
        core::mem::discriminant(self).hash(state);
        match self {
            RouteDemand::Whole => {}
            RouteDemand::MemberPath(segments) => segments.hash(state),
            RouteDemand::Pick(members) => {
                let mut sorted = members.clone();
                sorted.sort();
                sorted.hash(state);
            }
            RouteDemand::Omit(members) => {
                let mut sorted = members.clone();
                sorted.sort();
                sorted.hash(state);
            }
        }
    }
}

/// Symbol space for routed resolution — type-space vs value-space.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash)]
pub enum SymbolSpace {
    /// Type-space import: `import type { Foo }` or generic type references.
    #[default]
    Type,
    /// Value-space import: `import { Foo }` where Foo is a value (component, function, etc.).
    Value,
}

/// Bounded status of a routed-symbol resolution.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RoutedSymbolStatus {
    /// Successfully resolved to a final canonical symbol.
    Resolved,
    /// Symbol not found through any route.
    NotFound,
    /// Partially resolved — some dependencies could not be followed.
    PartialUnknown {
        /// Descriptions of what could not be resolved.
        reasons: Vec<String>,
    },
    /// Resolution exceeded a budget (frontier depth, symbol visits, etc.).
    BudgetExceeded { domain: String },
    /// Cycle detected during resolution.
    CycleDetected,
}

/// Result of a routed-symbol resolution — rich enough to avoid a second pass.
///
/// See architectural rule 6: "A routed-symbol result must be rich enough
/// to avoid a second pass."
#[derive(Debug, Clone)]
pub struct RoutedSymbolResult {
    /// Final canonical file where the symbol is defined.
    pub final_canonical_id: String,
    /// Final exported name in the defining file.
    pub final_exported_name: String,
    /// Bounded resolution status.
    pub status: RoutedSymbolStatus,
    /// The route demand as normalized during resolution (may differ from
    /// the requested demand if narrowing occurred).
    pub normalized_route: RouteDemand,
    /// Provider/wildcard provenance chain for diagnostics and invalidation.
    pub provenance: Vec<RouteProvenance>,
    /// Route-local external dependency closure needed for materialization.
    /// Maps local alias → (canonical_id, exported_name) of the dependency.
    pub external_dependency_closure: Vec<RoutedExternalDep>,
}

/// One hop in the route provenance chain.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RouteProvenance {
    /// Canonical ID of the file at this hop.
    pub canonical_id: String,
    /// How this hop was discovered.
    pub kind: RouteProvenanceKind,
}

/// How a route hop was discovered.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RouteProvenanceKind {
    /// Direct local export.
    Direct,
    /// Aliased re-export (`export { X as Y }`).
    Alias,
    /// Wildcard re-export (`export * from './source'`).
    Wildcard {
        /// Source order of the wildcard in the barrel file's declaration list.
        source_order: usize,
    },
}

/// An external dependency discovered during routed resolution.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RoutedExternalDep {
    /// Local alias name in the defining file.
    pub local_alias: String,
    /// Canonical ID of the dependency file.
    pub canonical_id: String,
    /// Exported name in the dependency file.
    pub exported_name: String,
}

/// Merge two route demands conservatively.
///
/// Used when multiple consumers request the same symbol with different routes.
/// The result is the narrowest demand that satisfies both requests.
pub fn merge_route_demands(a: &RouteDemand, b: &RouteDemand) -> RouteDemand {
    if a == b {
        return a.clone();
    }
    match (a, b) {
        (RouteDemand::Whole, _) | (_, RouteDemand::Whole) => RouteDemand::Whole,
        (RouteDemand::MemberPath(pa), RouteDemand::MemberPath(pb)) => {
            let common_prefix = pa
                .iter()
                .zip(pb.iter())
                .take_while(|(left, right)| left == right)
                .map(|(segment, _)| segment.clone())
                .collect::<Vec<_>>();
            if !common_prefix.is_empty() {
                RouteDemand::MemberPath(common_prefix)
            } else {
                let mut members = Vec::new();
                if let Some(first) = pa.first() {
                    members.push(first.clone());
                }
                if let Some(first) = pb.first() {
                    members.push(first.clone());
                }
                members.sort();
                members.dedup();
                if members.is_empty() {
                    RouteDemand::Whole
                } else {
                    RouteDemand::Pick(members)
                }
            }
        }
        (RouteDemand::MemberPath(p), RouteDemand::Pick(ps))
        | (RouteDemand::Pick(ps), RouteDemand::MemberPath(p)) => {
            let mut merged = ps.clone();
            if let Some(first) = p.first() {
                merged.push(first.clone());
            }
            merged.sort();
            merged.dedup();
            if merged.is_empty() {
                RouteDemand::Whole
            } else {
                RouteDemand::Pick(merged)
            }
        }
        (RouteDemand::Pick(a), RouteDemand::Pick(b)) => {
            let mut merged = a.clone();
            merged.extend(b.iter().cloned());
            merged.sort();
            merged.dedup();
            RouteDemand::Pick(merged)
        }
        (RouteDemand::Omit(a_omit), RouteDemand::MemberPath(p)) => {
            // Omit + MemberPath: if the member is not omitted, it's still valid
            if p.first().is_some_and(|first| !a_omit.contains(first)) {
                RouteDemand::Omit(a_omit.clone())
            } else {
                RouteDemand::Whole
            }
        }
        (RouteDemand::MemberPath(p), RouteDemand::Omit(b_omit)) => {
            if p.first().is_some_and(|first| !b_omit.contains(first)) {
                RouteDemand::Omit(b_omit.clone())
            } else {
                RouteDemand::Whole
            }
        }
        // Omit + Pick, Omit + Omit: conservatively widen to Whole
        _ => RouteDemand::Whole,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn route_demand_whole_is_default() {
        assert_eq!(RouteDemand::default(), RouteDemand::Whole);
    }

    #[test]
    fn route_demand_member_path_preserves_full_depth() {
        let path = RouteDemand::MemberPath(vec!["variants".to_string(), "color".to_string()]);
        match &path {
            RouteDemand::MemberPath(segments) => {
                assert_eq!(segments.len(), 2);
                assert_eq!(segments[0], "variants");
                assert_eq!(segments[1], "color");
            }
            _ => panic!("expected MemberPath"),
        }
    }

    #[test]
    fn route_demand_pick_hash_is_order_independent() {
        use std::collections::hash_map::DefaultHasher;
        fn hash_demand(d: &RouteDemand) -> u64 {
            let mut h = DefaultHasher::new();
            d.hash(&mut h);
            h.finish()
        }
        let a = RouteDemand::Pick(vec!["b".to_string(), "a".to_string()]);
        let b = RouteDemand::Pick(vec!["a".to_string(), "b".to_string()]);
        assert_eq!(hash_demand(&a), hash_demand(&b));
    }

    #[test]
    fn merge_identical_demands_returns_same() {
        let d = RouteDemand::MemberPath(vec!["foo".to_string()]);
        assert_eq!(merge_route_demands(&d, &d), d);
    }

    #[test]
    fn merge_member_paths_produces_pick() {
        let a = RouteDemand::MemberPath(vec!["foo".to_string()]);
        let b = RouteDemand::MemberPath(vec!["bar".to_string()]);
        let merged = merge_route_demands(&a, &b);
        assert_eq!(
            merged,
            RouteDemand::Pick(vec!["bar".to_string(), "foo".to_string()])
        );
    }

    #[test]
    fn merge_member_paths_with_common_prefix_keeps_prefix() {
        let a = RouteDemand::MemberPath(vec!["variants".to_string(), "color".to_string()]);
        let b = RouteDemand::MemberPath(vec!["variants".to_string(), "size".to_string()]);
        let merged = merge_route_demands(&a, &b);
        assert_eq!(
            merged,
            RouteDemand::MemberPath(vec!["variants".to_string()])
        );
    }

    #[test]
    fn merge_with_whole_always_returns_whole() {
        let a = RouteDemand::Pick(vec!["x".to_string()]);
        assert_eq!(
            merge_route_demands(&a, &RouteDemand::Whole),
            RouteDemand::Whole
        );
        assert_eq!(
            merge_route_demands(&RouteDemand::Whole, &a),
            RouteDemand::Whole
        );
    }

    #[test]
    fn merge_pick_and_member_extends_pick() {
        let pick = RouteDemand::Pick(vec!["a".to_string(), "b".to_string()]);
        let member = RouteDemand::MemberPath(vec!["c".to_string()]);
        let merged = merge_route_demands(&pick, &member);
        assert_eq!(
            merged,
            RouteDemand::Pick(vec!["a".to_string(), "b".to_string(), "c".to_string()])
        );
    }

    #[test]
    fn symbol_space_defaults_to_type() {
        assert_eq!(SymbolSpace::default(), SymbolSpace::Type);
    }

    #[test]
    fn routed_symbol_result_carries_full_provenance_chain() {
        let result = RoutedSymbolResult {
            final_canonical_id: "/src/leaf.ts".to_string(),
            final_exported_name: "Props".to_string(),
            status: RoutedSymbolStatus::Resolved,
            normalized_route: RouteDemand::Whole,
            provenance: vec![
                RouteProvenance {
                    canonical_id: "/src/index.ts".to_string(),
                    kind: RouteProvenanceKind::Wildcard { source_order: 0 },
                },
                RouteProvenance {
                    canonical_id: "/src/types/index.ts".to_string(),
                    kind: RouteProvenanceKind::Wildcard { source_order: 2 },
                },
                RouteProvenance {
                    canonical_id: "/src/leaf.ts".to_string(),
                    kind: RouteProvenanceKind::Direct,
                },
            ],
            external_dependency_closure: vec![],
        };
        assert_eq!(result.provenance.len(), 3);
        assert_eq!(result.final_canonical_id, "/src/leaf.ts");
    }
}
