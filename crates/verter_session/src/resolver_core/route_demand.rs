//! Shared route-demand model for routed-symbol resolution.
//!
//! The `RouteDemand` type itself is the ONE canonical
//! `verter_type_expr::RouteDemand` (re-exported here for the resolver's
//! consumers): `Pick`/`Omit` carry the normalized `RouteKeySet`
//! (order-independent `Eq` + `Hash`), `MemberPath` stays an ordered
//! sequence. `merge_route_demands` lives beside it in `verter_type_expr`
//! (pure over the canonical type) and is re-exported the same way. This
//! module keeps the session-only routed-resolution carriers
//! (`SymbolSpace`, `RoutedSymbolStatus`, `RoutedSymbolResult`,
//! `RouteProvenance*`, `RoutedExternalDep`).
//!
//! See architectural rule 2: "Route demand is a shared resolver-core type."

pub use verter_type_expr::{merge_route_demands, RouteDemand, RouteKeySet};

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

#[cfg(test)]
mod tests {
    use super::*;

    // The `RouteDemand` shape/merge unit tests moved to
    // `verter_type_expr::fact_witnesses` with the canonical type; only the
    // session-owned carriers are tested here.

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
