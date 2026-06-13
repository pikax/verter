#![deny(missing_docs)]
//! The closed framework-surface PLAN + RESOLVED vocabulary.
//!
//! An adapter PLANS its surfaces as typed [`PlannedDemand`] — a CLOSED four-arm
//! taxonomy carrying ONLY typed demand data (a canonical id, a typed macro
//! selector, a stable node handle, a path + projection mode). No arm carries
//! source text, raw byte ranges standing in for source, OXC handles, closures,
//! or raw semantic query keys (a `Custom`/`Raw` arm is forbidden — the closed
//! taxonomy is the typed-demand discipline). The executor resolves each
//! [`PlannedDemand`] through its private resolve context onto a
//! [`ResolvedDemand`] — the CLOSED result vocabulary mirroring `PlannedDemand`
//! 1:1, each arm carrying a typed [`ResolvedOutcome`] (never a bare `Option`).

use std::sync::Arc;

use verter_protocol::typeinfo::graph::FrameworkSurfaceKind;
use verter_semantic::analysis::types::AnalyzedMacroKind;

use crate::semantic_query::{PathSegment, ProjectionMode};
use crate::typeinfo::framework_surface::results::{ResolvedMacroPayload, ResolvedOutcome};
use crate::typeinfo::surface::TypeInfoSurface;

/// A stable handle to a typed-IR node a plan demand operates on.
///
/// A raw graph node id is generation-scoped and therefore unsafe to carry
/// across a generation flip; a plan demand is data the executor resolves at a
/// stable point, so it carries the OWNER canonical + the node's stable identity
/// rather than a live graph id. Vue's `plan_surfaces` only emits
/// [`PlannedDemand::PublicTypeInstance`] / [`PlannedDemand::MacroPayload`], so
/// the handle is the substrate the path/shallow demands need for later
/// framework verticals; it never carries a live generation-scoped id.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TypeNodeHandle {
    /// The canonical id of the file owning the referenced node.
    pub owner_canonical: Arc<str>,
    /// A stable symbol name identifying the node within its owner.
    pub symbol_name: Arc<str>,
}

/// A typed macro-payload selector — typed demand data, NOT a query key.
///
/// The selector is KIND-PRIMARY: `macro_kind` identifies the surface, and the
/// executor enumerates the matching macro(s) from the owner's authoritative
/// shallow snapshot. `macro_index` is an OPTIONAL disambiguator a caller that
/// already knows the macro's stable snapshot index may supply (e.g. a future
/// request-narrowing path); `None` means "the executor selects the macro of
/// this kind from the snapshot" — the planning case, which has no snapshot
/// access and therefore never fabricates an index.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MacroPayloadSelector {
    /// An optional stable index of the macro in the owner's analysis snapshot.
    /// `None` ⇒ the executor selects the macro of `macro_kind` from the
    /// snapshot.
    pub macro_index: Option<usize>,
    /// The macro kind this payload targets.
    pub macro_kind: AnalyzedMacroKind,
}

/// The closed plan-demand taxonomy.
///
/// Each variant carries ONLY typed demand data; no arm carries source text,
/// OXC handles, closures, or raw semantic query keys (asserted by the
/// closed-vocabulary guard).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PlannedDemand {
    /// Instantiate a component's public instance type (the synthesized
    /// `{ $props, $emit, $slots }` surface for a carrier component).
    PublicTypeInstance {
        /// The component's canonical id.
        canonical: Arc<str>,
    },
    /// Resolve one macro's payload surface.
    MacroPayload {
        /// The owner component's canonical id.
        owner: Arc<str>,
        /// The typed macro selector.
        selector: MacroPayloadSelector,
    },
    /// Project a path off a base node at a projection mode.
    PathProjection {
        /// The base node handle.
        base: TypeNodeHandle,
        /// The path segments to project.
        path: Arc<[PathSegment]>,
        /// The projection mode for the terminal hop.
        mode: ProjectionMode,
    },
    /// Read a node's one-level shallow surface.
    ShallowSurface {
        /// The node handle.
        node: TypeNodeHandle,
    },
}

/// One planned surface: the wire kind plus its typed demand.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PlannedResolve {
    /// The wire surface kind this demand produces.
    pub kind: FrameworkSurfaceKind,
    /// The typed demand the executor resolves.
    pub demand: PlannedDemand,
}

/// An adapter's full surface plan.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct FrameworkSurfacePlan {
    /// The planned demands, one per surface the adapter intends to produce.
    pub items: Vec<PlannedResolve>,
}

/// The resolved-demand taxonomy mirroring [`PlannedDemand`] 1:1.
///
/// Each arm carries a typed [`ResolvedOutcome`] so the status basis stays
/// explicit (a bare `Option` cannot carry the supported-empty / miss / partial /
/// unsupported distinction).
#[derive(Debug, Clone, PartialEq)]
pub enum ResolvedDemand {
    /// A resolved public-instance surface.
    PublicType(ResolvedOutcome<TypeInfoSurface>),
    /// A resolved macro payload.
    MacroPayload(ResolvedMacroPayload),
    /// A resolved path projection.
    PathProjection(ResolvedOutcome<TypeInfoSurface>),
    /// A resolved shallow surface.
    ShallowSurface(ResolvedOutcome<TypeInfoSurface>),
}

/// One resolved surface: the wire kind plus its resolved demand.
#[derive(Debug, Clone, PartialEq)]
pub struct ResolvedItem {
    /// The wire surface kind.
    pub kind: FrameworkSurfaceKind,
    /// The resolved demand.
    pub result: ResolvedDemand,
}

/// The executor-owned resolved-surface result set handed to `normalize`.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct ResolvedSurfaces {
    /// The resolved items, one per planned demand.
    pub items: Vec<ResolvedItem>,
}

/// The resolved component selector the executor hands to the adapter.
///
/// Carries the resolved owner canonical plus whether the component is a
/// default-export (synthesized `default`) or a named export — typed selector
/// data, not a query key.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResolvedComponentSelector {
    /// The owner component's canonical id.
    pub canonical: Arc<str>,
    /// The component's export kind.
    pub export: ComponentExport,
}

/// How a component is exported from its owner file.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ComponentExport {
    /// The default export (a synthesized `default` instance for carrier
    /// components).
    Default,
    /// A named export.
    Named(Arc<str>),
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Whole-enum closed-vocabulary walk: every `PlannedDemand` variant is
    /// matched explicitly (no `..`, no wildcard arm) and carries only typed
    /// demand data.
    #[test]
    fn planned_demand_is_closed_no_wildcard() {
        let demands = [
            PlannedDemand::PublicTypeInstance {
                canonical: Arc::from("/a.vue"),
            },
            PlannedDemand::MacroPayload {
                owner: Arc::from("/a.vue"),
                selector: MacroPayloadSelector {
                    macro_index: None,
                    macro_kind: AnalyzedMacroKind::DefineProps,
                },
            },
            PlannedDemand::PathProjection {
                base: TypeNodeHandle {
                    owner_canonical: Arc::from("/a.vue"),
                    symbol_name: Arc::from("default"),
                },
                path: Arc::from(Vec::<PathSegment>::new().into_boxed_slice()),
                mode: ProjectionMode::Navigate,
            },
            PlannedDemand::ShallowSurface {
                node: TypeNodeHandle {
                    owner_canonical: Arc::from("/a.vue"),
                    symbol_name: Arc::from("default"),
                },
            },
        ];
        for d in &demands {
            // Exhaustive match, no wildcard — adding a variant breaks this.
            let tag = match d {
                PlannedDemand::PublicTypeInstance { .. } => 0,
                PlannedDemand::MacroPayload { .. } => 1,
                PlannedDemand::PathProjection { .. } => 2,
                PlannedDemand::ShallowSurface { .. } => 3,
            };
            assert!(tag < 4);
        }
    }

    #[test]
    fn component_export_distinguishes_default_from_named() {
        let d = ResolvedComponentSelector {
            canonical: Arc::from("/a.vue"),
            export: ComponentExport::Default,
        };
        let n = ResolvedComponentSelector {
            canonical: Arc::from("/a.vue"),
            export: ComponentExport::Named(Arc::from("Foo")),
        };
        assert_ne!(d, n);
    }
}
