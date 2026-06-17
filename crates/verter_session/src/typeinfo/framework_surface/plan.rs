#![deny(missing_docs)]
//! The closed framework-surface PLAN + RESOLVED vocabulary.
//!
//! An adapter PLANS its surfaces as typed [`PlannedDemand`] — a CLOSED four-arm
//! taxonomy carrying ONLY typed demand data (a typed macro selector, a stable
//! node handle, a path + projection mode, a Svelte source family). No arm carries
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
/// [`PlannedDemand::MacroPayload`], so the handle is the substrate the
/// path/shallow demands need for later framework verticals; it never carries a
/// live generation-scoped id.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TypeNodeHandle {
    /// The canonical id of the file owning the referenced node.
    pub owner_canonical: Arc<str>,
    /// A stable symbol name identifying the node within its owner.
    pub symbol_name: Arc<str>,
}

/// The CLOSED Svelte source-family discriminant (D-bc).
///
/// A Svelte component has at most ONE declaration site per source family
/// (derived from the §9 mapping), so the family alone is the minimal structural
/// remainder — no index column. It is the [`PlannedDemand::SvelteSurface`] demand
/// discriminant AND the Svelte adapter's
/// [`crate::framework::surface_store::FullKey`] key remainder. SLOTS is composed
/// from TWO families ([`SvelteSurfaceSource::SnippetProps`] +
/// [`SvelteSurfaceSource::LegacySlotInventory`]) merged at normalise time, so
/// each cached bundle stays single-source and collision-free.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum SvelteSurfaceSource {
    /// Runes `$props()` type members → PROPS.
    RunesProps,
    /// Legacy `export let` props → PROPS.
    LegacyExportLet,
    /// `$bindable()` props → MODEL.
    Bindable,
    /// Snippet-typed `$props()` members → SLOTS (validated `Snippet` members).
    SnippetProps,
    /// Legacy `<slot>` inventory → SLOTS.
    LegacySlotInventory,
    /// Legacy `createEventDispatcher<E>` event map → EMITS.
    LegacyDispatcher,
    /// Modern callback-prop events (`onEvent` props whose value is function-like)
    /// → EMITS. A DERIVED, NON-AUTHORITATIVE compatibility index: Svelte 5's
    /// `Component` type carries no Events generic — callback props replaced
    /// dispatcher events — so `$props` stays the authoritative surface for modern
    /// event correctness. This source structurally enumerates the `$props` object
    /// surface, keeps the static keys matching the `on${E}` callback convention
    /// (NON-EMPTY suffix + function-like value), and surfaces each as an EMITS
    /// event whose payload is the callback's PARAMETERS directly (NO event-name
    /// strip — that strip is dispatcher-only). It NEVER mines an arbitrary
    /// non-`on` function prop. The legacy `LegacyDispatcher` source stays the
    /// authoritative EMITS source for dispatcher-based components.
    CallbackPropEvents,
    /// Exported instance-script members → EXPOSE.
    InstanceExports,
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
    /// Resolve one Svelte source family's surface (D-bh).
    ///
    /// The executor's resolve arm reads the owner's typed Svelte facts for
    /// `source`, dispatches the captured `TypeExpr`(s) through the SHARED
    /// resolver, and produces a single-source [`ResolvedMacroPayload`]. NOT the
    /// Vue-coupled [`PlannedDemand::MacroPayload`] arm — Svelte surfaces are not
    /// Vue macros.
    SvelteSurface {
        /// The owner component's canonical id.
        owner: Arc<str>,
        /// The Svelte source family this demand resolves.
        source: SvelteSurfaceSource,
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
    /// A resolved macro payload.
    MacroPayload(ResolvedMacroPayload),
    /// A resolved path projection.
    PathProjection(ResolvedOutcome<TypeInfoSurface>),
    /// A resolved shallow surface.
    ShallowSurface(ResolvedOutcome<TypeInfoSurface>),
    /// A resolved Svelte source-family surface (single-source DTO bundle).
    SvelteSurface(ResolvedMacroPayload),
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
            PlannedDemand::SvelteSurface {
                owner: Arc::from("/App.svelte"),
                source: SvelteSurfaceSource::RunesProps,
            },
        ];
        for d in &demands {
            // Exhaustive match, no wildcard — adding a variant breaks this.
            let tag = match d {
                PlannedDemand::MacroPayload { .. } => 0,
                PlannedDemand::PathProjection { .. } => 1,
                PlannedDemand::ShallowSurface { .. } => 2,
                PlannedDemand::SvelteSurface { .. } => 3,
            };
            assert!(tag < 4);
        }
    }

    /// The Svelte source-family discriminant is CLOSED and `Eq + Hash` (the
    /// D-bc store-key remainder + the demand discriminant). Every variant is
    /// matched explicitly here so a new family forces an acknowledgement.
    #[test]
    fn svelte_surface_source_is_closed_and_hashable() {
        use std::collections::HashSet;
        let all = [
            SvelteSurfaceSource::RunesProps,
            SvelteSurfaceSource::LegacyExportLet,
            SvelteSurfaceSource::Bindable,
            SvelteSurfaceSource::SnippetProps,
            SvelteSurfaceSource::LegacySlotInventory,
            SvelteSurfaceSource::LegacyDispatcher,
            SvelteSurfaceSource::CallbackPropEvents,
            SvelteSurfaceSource::InstanceExports,
        ];
        // Distinct families never alias under Hash/Eq.
        let set: HashSet<_> = all.iter().copied().collect();
        assert_eq!(set.len(), 8);
        for source in &all {
            // Exhaustive match — adding a family breaks this.
            let tag = match source {
                SvelteSurfaceSource::RunesProps => 0,
                SvelteSurfaceSource::LegacyExportLet => 1,
                SvelteSurfaceSource::Bindable => 2,
                SvelteSurfaceSource::SnippetProps => 3,
                SvelteSurfaceSource::LegacySlotInventory => 4,
                SvelteSurfaceSource::LegacyDispatcher => 5,
                SvelteSurfaceSource::CallbackPropEvents => 6,
                SvelteSurfaceSource::InstanceExports => 7,
            };
            assert!(tag < 8);
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
