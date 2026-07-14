//! Session-owned component-meta OUTPUT envelope: the fully-materialized,
//! context-free payload a wire (FFI) converter consumes by value.
//!
//! The component-meta `*Analysis` carriers hold content-free
//! [`SemanticTypeSource`] locators, not `TypeExpr`s. The wire boundary needs
//! materialized `TypeExpr`s, so the session materializes them ONCE at the
//! terminal output sink (`meta_resolve::projectors::output_sink`), under the
//! request-bound validated view, and hands the converter this envelope.
//!
//! Ownership rules (all compiler-fenced or structural):
//!
//! - **Request-local only.** The envelope is NEVER stored on
//!   `ResolvedComponentMetaState`, `ComponentMetaAnalysis`,
//!   `ComponentMetaResultDb`, or any warm semantic cache — wire `TypeExpr`s in
//!   cached semantic state would relaunder output IR into semantic authority.
//!   It is NON-`Clone` and transported by value.
//! - **Sink-only construction.** Every constructor takes the sink-mintable
//!   [`MetaResolveProjectorsOutputCap`] capability as proof. Only the terminal
//!   output sink can mint that capability (its `new` is private to the sink —
//!   a planted mint elsewhere is `E0624`), so arbitrary session code cannot
//!   assemble an envelope.
//! - **Positional topology.** Materialized lanes are positional vectors
//!   order-aligned with the analysis — never name-keyed maps (names repeat
//!   across duplicate events, slots, fallthrough branches, registry rows).

use verter_semantic::analysis::component_meta::{ComponentMetaAnalysis, ResolvedTypeAnalysis};
use verter_type_expr::facts::{SemanticSourceFailure, SourcePosition};
use verter_type_expr::TypeExpr;

use crate::meta_resolve::projectors::MetaResolveProjectorsOutputCap;

/// The session-owned, fully-materialized component-meta output envelope.
///
/// PRIVATE fields; NON-`Clone`; constructible only by the terminal output
/// sink (the constructor requires the sink-mintable capability). The single
/// way to read it is the DESTRUCTIVE terminal transfer accessor
/// [`Self::into_parts`], which the wire converter consumes by value.
#[derive(Debug)]
pub struct ComponentMetaOutput {
    /// The final component-meta analysis the output was materialized from.
    /// When the envelope carries a resolution sidecar, the analysis'
    /// `type_registry` already has the resolved-registry name-overlay
    /// finalize applied (session-owned; the wire converter never merges).
    analysis: ComponentMetaAnalysis,
    /// Narrowed output-only resolution sidecar (`None` on the
    /// sidecar-less output-envelope entries; the payload and audited
    /// entries seed it).
    resolution: Option<ComponentMetaResolutionOutput>,
    /// The materialized positional type lanes.
    types: MaterializedComponentMetaTypes,
}

impl ComponentMetaOutput {
    /// Assemble the envelope. Requires the terminal output sink's capability
    /// — only `meta_resolve::projectors::output_sink` can mint it, so only
    /// the sink can construct an envelope.
    pub(crate) fn from_parts(
        _cap: &MetaResolveProjectorsOutputCap<'_, '_>,
        analysis: ComponentMetaAnalysis,
        resolution: Option<ComponentMetaResolutionOutput>,
        types: MaterializedComponentMetaTypes,
    ) -> Self {
        Self {
            analysis,
            resolution,
            types,
        }
    }

    /// The single DESTRUCTIVE terminal transfer accessor: consume the
    /// envelope and yield the materialized parts a wire converter reads.
    pub fn into_parts(
        self,
    ) -> (
        ComponentMetaAnalysis,
        Option<ComponentMetaResolutionOutput>,
        MaterializedComponentMetaTypes,
    ) {
        (self.analysis, self.resolution, self.types)
    }
}

/// The materialized component-meta type lanes: a NESTED POSITIONAL topology,
/// order-aligned with the analysis. Positional vectors — never name-keyed
/// maps — because names repeat (duplicate event names, slot bindings across
/// slots, fallthrough branches, registry rows).
///
/// All 11 wire type lanes are materialized:
/// props, event payloads, slot bindings, models, exposed members,
/// public-instance members, merged type-registry entries, accepted props,
/// accepted event payloads, fallthrough props, fallthrough event payloads.
#[derive(Debug)]
pub struct MaterializedComponentMetaTypes {
    lanes: MaterializedComponentMetaTypeLanes,
}

impl MaterializedComponentMetaTypes {
    /// Assemble all 11 lanes. Requires the terminal output sink's
    /// capability — only the sink materializes output lanes.
    pub(crate) fn from_lanes(
        _cap: &MetaResolveProjectorsOutputCap<'_, '_>,
        lanes: MaterializedComponentMetaTypeLanes,
    ) -> Self {
        Self { lanes }
    }

    /// Destructive transfer of the 11 positional lanes, each order-aligned
    /// 1:1 with the analysis vectors the envelope was materialized from.
    pub fn into_lanes(self) -> MaterializedComponentMetaTypeLanes {
        self.lanes
    }
}

/// The open positional lane bundle a wire converter reads after the
/// DESTRUCTIVE transfer ([`MaterializedComponentMetaTypes::into_lanes`]).
/// Every vector is order-aligned 1:1 with its analysis counterpart;
/// nested lanes mirror the analysis' nested topology (per-slot bindings,
/// per-branch fallthrough rows).
#[derive(Debug, Default)]
pub struct MaterializedComponentMetaTypeLanes {
    /// `props[i].type` — aligned with `ComponentMetaAnalysis::props`.
    pub props: Vec<TypeExpr>,
    /// `events[i].payload` — aligned with `ComponentMetaAnalysis::events`
    /// (duplicate event names preserved positionally).
    pub event_payloads: Vec<TypeExpr>,
    /// `slots[i].bindings[j].type` — outer aligned with
    /// `ComponentMetaAnalysis::slots`, inner with `slots[i].bindings`.
    pub slot_bindings: Vec<Vec<TypeExpr>>,
    /// `models[i].type` — aligned with `ComponentMetaAnalysis::models`.
    pub models: Vec<TypeExpr>,
    /// `exposed[i].type` — aligned with `ComponentMetaAnalysis::exposed`.
    pub exposed: Vec<TypeExpr>,
    /// `publicInstance.members[i].type` — aligned with
    /// `ComponentMetaAnalysis::public_instance.members`; empty when the
    /// sidecar is absent.
    pub public_instance_members: Vec<TypeExpr>,
    /// `typeRegistry[i].type` — aligned with the (overlay-finalized)
    /// `ComponentMetaAnalysis::type_registry`.
    pub type_registry_entries: Vec<TypeExpr>,
    /// `acceptedProps[i].type` — aligned with
    /// `ComponentMetaAnalysis::accepted_props`.
    pub accepted_props: Vec<TypeExpr>,
    /// `acceptedEvents[i].payload` — aligned with
    /// `ComponentMetaAnalysis::accepted_events`.
    pub accepted_event_payloads: Vec<TypeExpr>,
    /// `fallthroughSurface.branches[i].props[j].type` — outer aligned with
    /// the branch vector, inner with each branch's `props`; empty when the
    /// surface is `None`.
    pub fallthrough_props: Vec<Vec<TypeExpr>>,
    /// `fallthroughSurface.branches[i].events[j].payload` — outer aligned
    /// with the branch vector, inner with each branch's `events`; empty when
    /// the surface is `None`.
    pub fallthrough_event_payloads: Vec<Vec<TypeExpr>>,
}

/// Narrowed output-only resolution sidecar: only what a wire converter needs
/// — NOT the whole `ResolvedComponentMetaState` (which is semantic state and
/// stays inside the session). Carried on the envelope by the sidecar-seeding
/// entries (payload + audited); the sidecar-less output-envelope entries
/// carry `None`.
#[derive(Debug, Clone)]
pub struct ComponentMetaResolutionOutput {
    /// The projection mode the resolution ran under.
    pub mode: crate::types::ProjectionMode,
    /// Resolved per-macro metadata (declaration identity, native-props
    /// visibility surface, JSDoc).
    pub resolved_macros: Vec<crate::meta_resolve::ResolvedMacroMeta>,
    /// Native declaration metadata for each resolved type-registry entry
    /// (the wire converter's per-name declaration sidecar).
    pub resolved_type_registry_meta: Vec<crate::meta_resolve::ResolvedTypeRegistryMeta>,
    /// Origin subgraph for semantic results (`Expanded` mode only).
    pub origin_graph: Option<verter_protocol::types::OriginGraphDto>,
}

/// Session-internal resolution SEED for the output builder: the narrowed
/// wire sidecar PLUS the resolved type-registry overlay entries the
/// session-owned name-overlay finalize consumes BEFORE materialization
/// (the overlay entries merge into `analysis.type_registry` and are then
/// dropped — they never reach the wire converter separately).
#[derive(Debug)]
pub(crate) struct ComponentMetaResolutionSeed {
    /// Resolved type-registry overlay entries (merged into the analysis'
    /// registry by name: replace-in-place or append).
    pub(crate) resolved_type_registry: Vec<ResolvedTypeAnalysis>,
    /// The narrowed output-only sidecar carried on the envelope.
    pub(crate) output: ComponentMetaResolutionOutput,
}

impl ComponentMetaResolutionSeed {
    /// Seed from a live cold-resolve state.
    pub(crate) fn from_resolved_state(
        resolved: &crate::meta_resolve::ResolvedComponentMetaState,
    ) -> Self {
        Self {
            resolved_type_registry: resolved.resolved_type_registry.clone(),
            output: ComponentMetaResolutionOutput {
                mode: resolved.mode,
                resolved_macros: resolved.resolved_macros.clone(),
                resolved_type_registry_meta: resolved.resolved_type_registry_meta.clone(),
                origin_graph: resolved.origin_graph.clone(),
            },
        }
    }

    /// Seed from a warm-cache resolution template (no rehydration — the
    /// template carries every sidecar field the output needs).
    pub(crate) fn from_template(
        template: &crate::component_meta_result_db::ResolutionTemplate,
    ) -> Self {
        Self {
            resolved_type_registry: template.resolved_type_registry.clone(),
            output: ComponentMetaResolutionOutput {
                mode: template.mode,
                resolved_macros: template.resolved_macros.clone(),
                resolved_type_registry_meta: template.resolved_type_registry_meta.clone(),
                origin_graph: template.origin_graph.clone(),
            },
        }
    }
}

/// Which output lane a materialization failure occurred in — one variant
/// per materialized wire type lane.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ComponentMetaOutputLane {
    /// The `props[].type` lane.
    Prop,
    /// The `events[].payload` lane.
    EventPayload,
    /// The `slots[].bindings[].type` lane.
    SlotBinding,
    /// The `models[].type` lane.
    Model,
    /// The `exposed[].type` lane.
    Exposed,
    /// The `publicInstance.members[].type` lane.
    PublicInstanceMember,
    /// The merged `typeRegistry[].type` lane.
    TypeRegistryEntry,
    /// The `acceptedProps[].type` lane.
    AcceptedProp,
    /// The `acceptedEvents[].payload` lane.
    AcceptedEventPayload,
    /// The `fallthroughSurface.branches[].props[].type` lane.
    FallthroughProp,
    /// The `fallthroughSurface.branches[].events[].payload` lane.
    FallthroughEventPayload,
}

impl ComponentMetaOutputLane {
    /// The wire lane path, for diagnostics.
    pub fn path(self) -> &'static str {
        match self {
            ComponentMetaOutputLane::Prop => "props[].type",
            ComponentMetaOutputLane::EventPayload => "events[].payload",
            ComponentMetaOutputLane::SlotBinding => "slots[].bindings[].type",
            ComponentMetaOutputLane::Model => "models[].type",
            ComponentMetaOutputLane::Exposed => "exposed[].type",
            ComponentMetaOutputLane::PublicInstanceMember => "publicInstance.members[].type",
            ComponentMetaOutputLane::TypeRegistryEntry => "typeRegistry[].type",
            ComponentMetaOutputLane::AcceptedProp => "acceptedProps[].type",
            ComponentMetaOutputLane::AcceptedEventPayload => "acceptedEvents[].payload",
            ComponentMetaOutputLane::FallthroughProp => {
                "fallthroughSurface.branches[].props[].type"
            }
            ComponentMetaOutputLane::FallthroughEventPayload => {
                "fallthroughSurface.branches[].events[].payload"
            }
        }
    }
}

/// One step of the interior position path within a composed source shell —
/// the typed breadcrumb a failed REQUIRED interior dereference carries so
/// the output error names the exact nested position that failed. Produced
/// by the strict raise entry
/// (`ProjectSemanticDispatch::raise_semantic_type_source_to_hot_strict`);
/// defined here (next to the public output error that transports it) so the
/// public error surface stays fully nameable.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum InteriorSourceStep {
    /// A named object / surface / synthesized / leaf-object member value.
    Member(std::sync::Arc<str>),
    /// A function parameter position (source order).
    Parameter { ordinal: u32 },
    /// The function return-type position.
    ReturnType,
    /// A type-parameter constraint position (source order).
    TypeParamConstraint { ordinal: u32 },
    /// A type-parameter default position (source order).
    TypeParamDefault { ordinal: u32 },
    /// A tuple element position (source order).
    TupleElement { ordinal: u32 },
    /// A closed leaf-union arm (source order).
    UnionArm { ordinal: u32 },
    /// An index-signature KEY position (declaration order).
    IndexSignatureKey { ordinal: u32 },
    /// An index-signature VALUE position (declaration order).
    IndexSignatureValue { ordinal: u32 },
    /// The object position of a path-precise indexed access.
    IndexedAccessObject,
    /// A call-signature position (declaration order).
    CallSignature { ordinal: u32 },
    /// A construct-signature position (declaration order).
    ConstructSignature { ordinal: u32 },
}

impl std::fmt::Display for InteriorSourceStep {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            InteriorSourceStep::Member(name) => write!(f, ".{name}"),
            InteriorSourceStep::Parameter { ordinal } => write!(f, ".param[{ordinal}]"),
            InteriorSourceStep::ReturnType => write!(f, ".return"),
            InteriorSourceStep::TypeParamConstraint { ordinal } => {
                write!(f, ".typeParam[{ordinal}].constraint")
            }
            InteriorSourceStep::TypeParamDefault { ordinal } => {
                write!(f, ".typeParam[{ordinal}].default")
            }
            InteriorSourceStep::TupleElement { ordinal } => write!(f, ".tuple[{ordinal}]"),
            InteriorSourceStep::UnionArm { ordinal } => write!(f, ".unionArm[{ordinal}]"),
            InteriorSourceStep::IndexSignatureKey { ordinal } => {
                write!(f, ".indexSignature[{ordinal}].key")
            }
            InteriorSourceStep::IndexSignatureValue { ordinal } => {
                write!(f, ".indexSignature[{ordinal}].value")
            }
            InteriorSourceStep::IndexedAccessObject => write!(f, ".indexedAccessObject"),
            InteriorSourceStep::CallSignature { ordinal } => write!(f, ".callSignature[{ordinal}]"),
            InteriorSourceStep::ConstructSignature { ordinal } => {
                write!(f, ".constructSignature[{ordinal}]")
            }
        }
    }
}

/// Why an output lane's source position failed to materialize.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ComponentMetaOutputFailure {
    /// The source has no live graph representation under the request view
    /// (the shared raise returned no handle).
    UnraisableSource,
    /// The lane position is a REQUIRED source position whose faithful source
    /// the producer could not construct (`SourcePosition::Failed`). The
    /// failure is typed at the producer and FAILS the output — it is never
    /// rendered as an `unknown` success. Genuinely schema-ABSENT positions
    /// (`SourcePosition::Absent`) are NOT failures: they keep rendering the
    /// centralized typed `Unknown`.
    RequiredSourceUnavailable {
        /// The producer's typed source-construction failure.
        failure: SemanticSourceFailure,
    },
    /// A PRESENT interior position of a composed source shell failed its
    /// required dereference (a member value slot, a typed parameter, a
    /// tuple element, an index-signature key/value, ... whose locator the
    /// live view could not raise). Genuinely ABSENT schema positions (an
    /// unannotated parameter, an inferred return) are NOT failures — they
    /// legitimately materialize as typed `Unknown`. Fail-closed: the
    /// interior miss must never silently render as `Unknown`.
    InteriorSourceMiss {
        /// The nested position path from the source root to the failed
        /// dereference.
        path: std::sync::Arc<[InteriorSourceStep]>,
    },
    /// The raised node could not be shell-materialized at the sealed output
    /// seam.
    ShellMaterializationMiss,
    /// The raised source's materialized shape carries an
    /// unknown-materializing resolver-failure carrier at its ROOT or
    /// INTERIOR — an interned `Opaque` control failure whose shell fold
    /// would render a completed `unknown`. Conservatively FAIL-CLOSED: the
    /// graph does not carry per-position absent-vs-failed provenance yet
    /// (see `docs/arch/stage10-b6-p4b-debt-rows.md` DEBT ROW #2 for the
    /// deferred fine-grained provenance), so a `Present` source that would
    /// fold an interior failure into a successful `unknown` fails typed
    /// instead. The legitimately-publishable carriers (a recursive
    /// reference, a declaration placeholder) are NOT failures and pass;
    /// proven schema absence never reaches this raise — it renders the
    /// centralized typed `Unknown` through the `Absent` arm.
    UnknownMaterializingSourceInterior {
        /// The composition path from the source root to the deref'd
        /// position whose raised body carried the failure (EMPTY for a
        /// direct source-root deref).
        path: std::sync::Arc<[InteriorSourceStep]>,
    },
}

/// Strict typed output-materialization error: a PRESENT source the terminal
/// sink could not raise / shell-materialize — INCLUDING a failed REQUIRED
/// interior dereference inside a successfully-composed root — FAILS the
/// output; it is never silently rendered as `Unknown`. Carries the failed
/// lane, the lane index (aligned with the analysis vector; nested lanes also
/// carry the inner index), and the failed source.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ComponentMetaOutputError {
    /// The lane that failed.
    pub lane: ComponentMetaOutputLane,
    /// Index within the lane, aligned with the analysis vector (for a
    /// nested lane: the OUTER index — the slot / branch position).
    pub index: usize,
    /// For a nested lane (slot bindings, fallthrough rows): the INNER
    /// index within the outer element. `None` for flat lanes.
    pub inner_index: Option<usize>,
    /// The source POSITION that failed to materialize (boxed: the error
    /// travels up `Result` returns, so the payload stays off the happy
    /// path's stack). A failed REQUIRED position carries its `Failed` arm —
    /// it has no `SemanticTypeSource` to report.
    pub position: Box<SourcePosition>,
    /// The failure class.
    pub failure: ComponentMetaOutputFailure,
}

impl std::fmt::Display for ComponentMetaOutputError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let failure = match &self.failure {
            ComponentMetaOutputFailure::UnraisableSource => {
                "the source has no live graph representation under the request view".to_string()
            }
            ComponentMetaOutputFailure::RequiredSourceUnavailable { failure } => match failure {
                SemanticSourceFailure::UnrepresentableRequiredPayload => {
                    "a REQUIRED payload position has no representable source \
                     (the producer could not construct a faithful typed source)"
                        .to_string()
                }
                SemanticSourceFailure::UnrepresentableRequiredMemberValue => {
                    "a REQUIRED member-value position has no representable source \
                     (no authored slot, no use-site slot, no reference identity, \
                     no closed fact)"
                        .to_string()
                }
            },
            ComponentMetaOutputFailure::InteriorSourceMiss { path } => {
                let mut rendered = String::from("<root>");
                for step in path.iter() {
                    rendered.push_str(&step.to_string());
                }
                format!(
                    "a required interior position failed its dereference at {rendered} \
                     (a present locator the live view could not raise)"
                )
            }
            ComponentMetaOutputFailure::ShellMaterializationMiss => {
                "the raised node could not be shell-materialized at the output seam".to_string()
            }
            ComponentMetaOutputFailure::UnknownMaterializingSourceInterior { path } => {
                let mut rendered = String::from("<root>");
                for step in path.iter() {
                    rendered.push_str(&step.to_string());
                }
                format!(
                    "the raised source's materialized shape carries an unknown-materializing \
                     resolver failure at {rendered} (it would render a completed `unknown`)"
                )
            }
        };
        match self.inner_index {
            Some(inner) => write!(
                f,
                "component-meta output materialization failed at {} index {}.{inner}: {failure}",
                self.lane.path(),
                self.index,
            ),
            None => write!(
                f,
                "component-meta output materialization failed at {} index {}: {failure}",
                self.lane.path(),
                self.index,
            ),
        }
    }
}

impl std::error::Error for ComponentMetaOutputError {}
