#![deny(missing_docs)]
//! The framework-surface executor for the
//! `GRAPH_OPERATION_FRAMEWORK_SURFACES` wire operation.
//!
//! [`VerterHost::type_info_with_audit`] is the one public entry for every
//! served operation. It runs [`validate_type_info_graph_request`] FIRST (the
//! envelope validator — operation/payload-arm match, schema echo, and the
//! nested framework-surface validator), so a malformed envelope is rejected
//! with a typed wire error BEFORE any registry lookup or semantic dispatch.
//! Only the validated inner request reaches this executor body.
//!
//! Executor flow (post-validation):
//! 1. Resolve `selector.framework_profile` (schema 11, a 32-byte digest the
//!    validator proved well-shaped) against the registry's admitted profile
//!    index — one hash lookup to the sealed `FrameworkProfileId` and its
//!    registration, never through an adapter spelling. A reference the host
//!    did not mint is the typed `UnknownFrameworkProfile` error, before any
//!    indexing, export resolution, planning or surface-store access.
//! 2. Resolve the selector to a [`ResolvedComponentSelector`] carrying that
//!    identity (default export via the synthesized `default`; named export
//!    via the shallow inventory).
//! 3. The requested set is the request's facet demand
//!    (`FrameworkSurfaceRequest.requested_kinds`, validated non-empty and
//!    duplicate-free), taken in enum-tag order. Only requested kinds are
//!    planned, resolved and encoded: the response carries EXACTLY ONE entry
//!    per requested kind and none for an undemanded kind.
//! 4. A [`SurfaceRegistration::Deferred`] adapter answers every kind
//!    structurally UNSUPPORTED (a structural response, NOT an error).
//! 5. A [`SurfaceRegistration::Adapter`] adapter PLANS its demands against the
//!    facts/carrier-only [`FrameworkAdapterCtx`], the executor RESOLVES each
//!    [`PlannedDemand`] through the module-private [`ExecutorResolveCtx`] (an
//!    EXHAUSTIVE match — no wildcard arm — over the closed 4-variant taxonomy),
//!    the adapter NORMALIZES the resolved data, and [`graph_export`] encodes the
//!    `FrameworkSurfacePayload`, whose selector echo names the admitted
//!    profile.
//!
//! The executor resolves EVERY [`PlannedDemand`] THROUGH the one shared
//! type-resolution engine (the relocated Vue delegates / `resolve_shallow_surface_for`
//! / `project_shallow_surface_from_base`). It is NOT a second resolver — it
//! plans, dispatches to the shared engine, and encodes.

use std::sync::Arc;

use verter_audit::payloads::typeinfo_graph::{
    FrameworkSurfaceKindSupportTag, GraphOperationTag, TypeInfoGraphPayload,
};
use verter_audit::ProjectionModeTag;
use verter_protocol::typeinfo::graph::{
    self as wire, ComponentSelector, FrameworkSurfaceKind, FrameworkSurfaceKindSupport,
    FrameworkSurfacePayload, FrameworkTag, TypeInfoGraphRequest, TypeInfoGraphResponse,
    TypeInfoRequestError,
};
use verter_protocol::verter::v1::{type_info_graph_response, type_info_request_error};

use crate::framework::ctx::FrameworkAdapterCtx;
use crate::framework::descriptor::ALL_FRAMEWORK_SURFACE_KINDS;
use crate::framework::registry::{RegisteredFrameworkProfile, SurfaceRegistration};
use crate::typeinfo::framework_surface::graph_export::{
    encode_framework_surfaces, encode_framework_surfaces_with_unsupported_message,
};
use crate::typeinfo::framework_surface::plan::{
    ComponentExport, PlannedDemand, ResolvedComponentSelector, ResolvedDemand, ResolvedItem,
    ResolvedSurfaces,
};
use crate::typeinfo::framework_surface::results::{
    MacroSurfaceDtos, NormalizedSurfaces, ResolvedMacroPayload, ResolvedOutcome,
};
use crate::typeinfo::types::{TypeInfoQueryLevel, VueMacroSurfaceRequest};
use crate::VerterHost;
use verter_type_engine::semantic_query::ProjectionMode;

/// The audit operation tag the framework-surface executor registers under.
///
/// Re-exported so binding-layer plumbing and tests reference the same tag the
/// executor records instead of re-spelling it.
pub const FRAMEWORK_SURFACE_AUDIT_OPERATION: GraphOperationTag =
    GraphOperationTag::FrameworkSurfaces;

impl VerterHost {
    /// The framework-surface executor body, after the one typeinfo entry
    /// (`type_info_with_audit`) validated the envelope and matched its served
    /// operation. Returns the wire response plus the audit payload.
    pub(crate) fn execute_framework_surface(
        &self,
        request: TypeInfoGraphRequest,
    ) -> (TypeInfoGraphResponse, TypeInfoGraphPayload) {
        let framework_request = match request.payload {
            Some(
                verter_protocol::verter::v1::type_info_graph_request::Payload::FrameworkSurface(r),
            ) => r,
            // The validator already proved the payload arm matches the
            // operation; this is unreachable, but the executor refuses to assume
            // it — a defensive malformed rejection rather than a panic.
            _ => {
                let error = malformed("framework-surface payload arm missing after validation");
                let payload = TypeInfoGraphPayload::from_validation_error(
                    GraphOperationTag::FrameworkSurfaces,
                );
                return (error_response(error), payload);
            }
        };

        // The validator proved the selector present, its canonical non-empty
        // and its profile reference a 32-byte digest.
        let request_selector = framework_request
            .selector
            .as_ref()
            .expect("validator proves selector present");
        let profile_ref = request_selector
            .framework_profile
            .as_ref()
            .expect("validator proves framework_profile present");
        // The facet demand the validator proved well-formed: every planned,
        // resolved and encoded kind below comes from this slice.
        let requested = requested_surface_kinds(&framework_request);
        let requested: &[FrameworkSurfaceKind] = &requested;

        // ADMISSION — one lookup from the wire digest to the admitted profile
        // and its registration. An unknown, disabled or stale (other-epoch)
        // reference fails here, AFTER shape validation and BEFORE any
        // indexing, export resolution, planning or surface-store access.
        let Some(admitted) = self
            .framework_registry()
            .resolve_profile(&profile_ref.digest)
        else {
            let error = TypeInfoRequestError {
                kind: Some(type_info_request_error::Kind::UnknownFrameworkProfile(
                    wire::wire_error_unknown_framework_profile(profile_ref.clone()),
                )),
            };
            let payload =
                TypeInfoGraphPayload::from_validation_error(GraphOperationTag::FrameworkSurfaces);
            return (error_response(error), payload);
        };
        let registration = admitted.registration.as_ref();
        // The selector echo is built from the ADMITTED identity, not copied
        // from the request bytes.
        let selector = admitted_selector_echo(request_selector, admitted);

        // SUBJECT — the owner file is decoded once, before any planning: a
        // source the host does not know is a typed refusal.
        let resolved_selector = match self.resolve_component_selector(&selector, admitted) {
            Ok(resolved) => resolved,
            Err(error) => {
                let payload = TypeInfoGraphPayload::from_validation_error(
                    GraphOperationTag::FrameworkSurfaces,
                );
                return (error_response(error), payload);
            }
        };
        // The fact every answer to this request names: the same query
        // identity its issued member references carry.
        let fact_query = crate::typeinfo::node_refs::component_facets_query_digest(
            &resolved_selector,
            requested,
        );

        // A NAMED-export selector must name a real EXPORT of the owner. The
        // export table is authoritative (a private local that is not exported is
        // NOT a valid named-export target). Two malformed cases are rejected here
        // BEFORE planning, NOT silently fall-through to the default component
        // surface (which would resolve the WRONG component):
        //   1. `has_export_name` with an EMPTY name — an incoherent selector;
        //   2. a named export the owner does not export.
        // Additionally, an adapter that does NOT resolve per-export component
        // surfaces (REGISTRY DATA — `descriptor.supports_named_export_surfaces`,
        // NOT a framework identity test) rejects a named-export request rather
        // than silently serving it as the default. The Vue keystone adapter
        // resolves the SFC's DEFAULT-export component surface only.
        if selector.has_export_name {
            if selector.export_name.is_empty() {
                let error =
                    malformed("selector sets has_export_name but carries an empty export_name");
                return (error_response(error), framework_payload(&[]));
            }
            if let ComponentExport::Named(name) = &resolved_selector.export {
                let is_export = self
                    .ensure_indexed_ready_serve(resolved_selector.canonical.as_ref())
                    .is_some_and(|serve| serve.indexed.shallow_state.export_target(name).is_some());
                if !is_export {
                    let error = malformed(&format!(
                        "named export `{name}` is not exported by `{}`",
                        resolved_selector.canonical
                    ));
                    return (error_response(error), framework_payload(&[]));
                }
                // The export exists, but this adapter resolves the default-export
                // component surface only — a named-export framework surface is not
                // a distinct resolution for it (registry capability).
                if !registration.descriptor.supports_named_export_surfaces {
                    let error = malformed(&format!(
                        "named-export framework surfaces are not supported for adapter `{}` \
                         (requested `{name}`); request the default-export component surface",
                        registration.descriptor.id
                    ));
                    return (error_response(error), framework_payload(&[]));
                }
            }
        }

        let framework_tag = registration.descriptor.tag;

        // NOT-A-COMPONENT-CARRIER gate (default-export selector): the component
        // surface of a default-export request only exists when the canonical is
        // the requested adapter's COMPONENT CARRIER (a `.vue` / `.svelte`). A
        // canonical that classifies as anything else under the SAME adapter — a
        // Svelte rune module (`.svelte.ts`/`.svelte.js`, a NON-COMPONENT module of
        // reactive values), a plain script, or a different framework's carrier —
        // has NO component to resolve. Such a request resolves STRUCTURALLY (a
        // registered adapter answered) with every component surface kind
        // structurally UNSUPPORTED — distinct from a component carrier's
        // supported-empty kind. This runs BEFORE planning so the adapter never
        // resolves a rune module's value exports into a phantom Expose surface.
        //
        // A NAMED-export selector is already validated above (the named export
        // exists AND the adapter supports per-export surfaces); the default-export
        // component gate does not apply to it.
        if matches!(resolved_selector.export, ComponentExport::Default) {
            let classified = self
                .language_classifier()
                .classify(resolved_selector.canonical.as_ref());
            let is_component_carrier = classified.is_framework_carrier()
                && classified.adapter_id() == Some(&registration.descriptor.id)
                && classified.carrier_language_id()
                    == registration.descriptor.carrier_language.as_ref();
            if !is_component_carrier {
                let mut encoded = encode_framework_surfaces_with_unsupported_message(
                    &NormalizedSurfaces::default(),
                    requested,
                    &[] as &[FrameworkSurfaceKind],
                    "canonical is not a component carrier for this adapter \
                     (no component surface to resolve)",
                );
                encoded.graph.fact_identity =
                    Some(crate::typeinfo::node_refs::fact_identity(fact_query, None));
                let payload = framework_payload(&encoded.surfaces);
                let response = framework_surface_response(
                    framework_request.schema_version,
                    selector,
                    framework_tag,
                    encoded.graph,
                    encoded.surfaces,
                );
                return (response, payload);
            }
        }

        let normalized = match &registration.surface {
            // A Deferred adapter answers every kind structurally UNSUPPORTED — a
            // structural response, NOT an error.
            SurfaceRegistration::Deferred => NormalizedSurfaces::default(),
            SurfaceRegistration::Adapter(adapter) => {
                let ctx = FrameworkAdapterCtx::new(registration, self);
                let plan = adapter.plan_surfaces(&ctx, &resolved_selector, requested);
                // Capture ONE proven-current request view AFTER validation and
                // thread it through EVERY demand resolver — so all of a single
                // response's props / emits / slots / model resolve against ONE
                // coherent owner version, never a mix from different versions
                // under churn. When no current view settles the request has no
                // basis: it is refused whole as `UnstableState`, never answered
                // as supported-empty kinds.
                //
                // Resolution and normalization run under ONE native fact
                // trace; once the response is encoded, every exported member
                // whose source replays exactly is issued a reference under the
                // basis that trace committed. Issuance never changes the
                // answer.
                //
                // The owner is materialised BEFORE the view is captured: the
                // trace observes the owner's source, and only a view captured
                // after the owner is materialised can vouch for that
                // observation. Materialising inside the request instead would
                // leave a cold request's basis uncommittable.
                let _ = self.ensure_indexed_ready_serve(resolved_selector.canonical.as_ref());
                match crate::typeinfo::current_store_view_for_query(self) {
                    Ok(current_view) => {
                        let overlay =
                            Arc::new(crate::resolver_core::CanonicalCompletionOverlay::new());
                        verter_audit::specialise!(crate::capture::installed_mode() => M, {
                            let host_ctx = crate::resolver_core::HostResolverContext::<'_, M>::from_current_in(
                                self,
                                &current_view,
                                overlay,
                            );
                            let dispatch =
                                verter_type_engine::project_semantic_dispatch::ProjectSemanticDispatch::new(
                                    &host_ctx,
                                );
                            let resolve_ctx = ExecutorResolveCtx {
                                ctx: &host_ctx,
                                dispatch: &dispatch,
                                members: std::cell::RefCell::new(Vec::new()),
                            };
                            let (normalized, trace) = crate::typeinfo::native_basis::trace_native_fact(
                                &host_ctx,
                                &[resolved_selector.canonical.as_ref()],
                                || {
                                    let resolved = resolve_ctx.resolve_plan(&resolved_selector, plan);
                                    adapter.normalize(&ctx, resolved)
                                },
                            );
                            let mut encoded = encode_framework_surfaces(
                                &normalized,
                                requested,
                                registration.descriptor.supported_surfaces,
                            );
                            let members = exported_framework_members(
                                &dispatch,
                                &resolved_selector,
                                resolve_ctx.members.into_inner(),
                                &encoded.exported_members,
                            );
                            // Each issued key is published on its exported member
                            // node (schema 12), independent of provenance.
                            let issued = crate::typeinfo::node_refs::issue_component_facets(
                                self,
                                current_view.view(),
                                &verter_type_engine::resolver_core::fact_validation_port::FactValidationView::new(&host_ctx),
                                trace,
                                &resolved_selector,
                                requested,
                                members,
                            );
                            // Every issued member shares the one committed basis;
                            // an answer that addressed no member committed none.
                            let basis = match issued {
                                Ok(issued) => {
                                    let basis = issued.first().map(|(_, key)| key.basis);
                                    publish_member_node_refs(&mut encoded.graph, issued);
                                    basis
                                }
                                Err(_) => None,
                            };
                            encoded.graph.fact_identity =
                                Some(crate::typeinfo::node_refs::fact_identity(fact_query, basis));
                            let payload = framework_payload(&encoded.surfaces);
                            let response = framework_surface_response(
                                framework_request.schema_version,
                                selector,
                                framework_tag,
                                encoded.graph,
                                encoded.surfaces,
                            );
                            return (response, payload);
                        })
                    }
                    Err(unsettled) => {
                        return (
                            error_response(unsettled.into()),
                            TypeInfoGraphPayload::from_unsettled_input(
                                GraphOperationTag::FrameworkSurfaces,
                            ),
                        );
                    }
                }
            }
        };

        // The descriptor's supported set decides supported-empty vs structural
        // UNSUPPORTED per kind. A Deferred registration's normalized set is
        // empty, so every kind fills supported-empty — but the design's
        // requirement is a Deferred adapter answers UNSUPPORTED; pass an EMPTY
        // supported set for Deferred so every kind fills structurally
        // UNSUPPORTED.
        let supported = match &registration.surface {
            SurfaceRegistration::Deferred => &[] as &[FrameworkSurfaceKind],
            SurfaceRegistration::Adapter(_) => registration.descriptor.supported_surfaces,
        };

        // A Deferred adapter's per-kind UNSUPPORTED diagnostic names the
        // intermediate state explicitly — the surfaces are not yet
        // registered, distinct from a supported adapter's per-kind unsupport.
        let mut encoded = match &registration.surface {
            SurfaceRegistration::Deferred => encode_framework_surfaces_with_unsupported_message(
                &normalized,
                requested,
                supported,
                "framework surfaces are not yet registered for this adapter",
            ),
            SurfaceRegistration::Adapter(_) => {
                encode_framework_surfaces(&normalized, requested, supported)
            }
        };
        encoded.graph.fact_identity =
            Some(crate::typeinfo::node_refs::fact_identity(fact_query, None));

        let payload = framework_payload(&encoded.surfaces);
        let response = framework_surface_response(
            framework_request.schema_version,
            selector,
            framework_tag,
            encoded.graph,
            encoded.surfaces,
        );
        (response, payload)
    }

    /// Resolve the wire [`ComponentSelector`] to a typed
    /// [`ResolvedComponentSelector`].
    ///
    /// A selector with `has_export_name` resolves to a named export; otherwise
    /// it is the component's default export (the synthesized `default` for
    /// carrier components). The owner file decodes through
    /// [`VerterHost::decode_source`], so an unknown owner is `UnknownSource`.
    fn resolve_component_selector(
        &self,
        selector: &ComponentSelector,
        admitted: &RegisteredFrameworkProfile,
    ) -> Result<ResolvedComponentSelector, TypeInfoRequestError> {
        let canonical = self.decode_source(&selector.canonical_id)?.canonical_arc();
        let export = if selector.has_export_name && !selector.export_name.is_empty() {
            ComponentExport::Named(Arc::from(selector.export_name.as_str()))
        } else {
            ComponentExport::Default
        };
        Ok(ResolvedComponentSelector {
            canonical,
            export,
            framework_profile: admitted.profile.clone(),
        })
    }
}

/// The module-private resolve context the executor drives each
/// [`PlannedDemand`] through. NEVER exported, NEVER passed to adapter code.
///
/// It resolves each demand THROUGH the one shared type-resolution engine (the
/// relocated Vue delegates and the shared shallow-surface / path-projection
/// dispatch). It is not a second resolver: every arm dispatches to existing
/// shared resolution.
struct ExecutorResolveCtx<
    'a,
    M: verter_audit::observe::ObserveExec = verter_audit::observe::Uncaptured,
> {
    /// The ONE proven-current request view every demand resolves against, so a
    /// single response never mixes owner versions under churn.
    ctx: &'a dyn crate::resolver_core::HostRequestContext<M>,
    dispatch: &'a verter_type_engine::project_semantic_dispatch::ProjectSemanticDispatch<
        'a,
        crate::resolver_core::HostCapabilities<M>,
    >,
    /// Every replayable member source a resolved macro contributed, with its
    /// producing macro: the candidates the response's exported members are
    /// addressed by.
    members: std::cell::RefCell<Vec<MemberSource>>,
}

/// A replayable member source one producer contributed.
struct MemberSource {
    surface: FrameworkSurfaceKind,
    role: crate::typeinfo::node_refs::MemberRole,
    name: String,
    occurrence: Option<verter_type_expr::facts::ResolvedEmitOccurrenceId>,
    producer: crate::typeinfo::node_refs::MemberProducer,
    scope_owner: verter_type_expr::TopLevelOwnerId,
    source: verter_type_expr::facts::SemanticTypeSource,
}

/// Publish each issued member key on its exported node: one
/// `node_id_map` entry per addressed member, in node order.
fn publish_member_node_refs(
    graph: &mut verter_protocol::verter::v1::SemanticTypeGraph,
    mut issued: Vec<(u32, crate::typeinfo::node_refs::IssuedRefKey)>,
) {
    issued.sort_by_key(|(node, _)| *node);
    graph
        .node_id_map
        .extend(issued.into_iter().map(|(node_id, key)| {
            verter_protocol::typeinfo::graph::NodeIdMapEntry {
                node_id,
                identity: None,
                node_ref: Some(key.to_wire()),
            }
        }));
}

/// The exported members a response can address: each exported props,
/// expose, options, slots, model or emits member whose name (for an emit,
/// its occurrence) exactly one producer contributed a replayable source
/// for. A member contributed twice, or exported twice, is ambiguous and
/// addressed by nothing.
fn exported_framework_members<M: verter_audit::observe::ObserveExec>(
    dispatch: &verter_type_engine::project_semantic_dispatch::ProjectSemanticDispatch<
        '_,
        crate::resolver_core::HostCapabilities<M>,
    >,
    selector: &ResolvedComponentSelector,
    sources: Vec<MemberSource>,
    exported: &[crate::typeinfo::framework_surface::graph_export::ExportedMemberNode],
) -> Vec<crate::typeinfo::node_refs::ExportedFrameworkMember> {
    // A member is its surface, name and (for an emit payload) occurrence.
    let exports = |node: &crate::typeinfo::framework_surface::graph_export::ExportedMemberNode,
                   source: &MemberSource| {
        node.kind == source.surface
            && node.name == source.name
            && node.occurrence == source.occurrence
    };
    let exported_count =
        |source: &MemberSource| exported.iter().filter(|node| exports(node, source)).count();
    let contributed_count = |source: &MemberSource| {
        sources
            .iter()
            .filter(|other| {
                other.surface == source.surface
                    && other.name == source.name
                    && other.occurrence == source.occurrence
            })
            .count()
    };
    let export_name: Arc<str> = match &selector.export {
        ComponentExport::Default => Arc::from("default"),
        ComponentExport::Named(name) => Arc::clone(name),
    };
    sources
        .iter()
        .filter(|source| exported_count(source) == 1 && contributed_count(source) == 1)
        .filter_map(|source| {
            let type_node_id = exported
                .iter()
                .find(|node| exports(node, source))?
                .type_node_id;
            Some((type_node_id, source))
        })
        .map(
            |(type_node_id, source)| crate::typeinfo::node_refs::ExportedFrameworkMember {
                type_node_id,
                anchor: dispatch.type_slot_for(
                    Arc::clone(&selector.canonical),
                    source.scope_owner,
                    Arc::clone(&export_name),
                ),
                locator: crate::typeinfo::node_refs::FrameworkMemberLocator {
                    component_canonical: Arc::clone(&selector.canonical),
                    component_export: selector.export.clone(),
                    framework_profile: selector.framework_profile.clone(),
                    scope_owner: source.scope_owner,
                    surface: source.surface,
                    producer: source.producer,
                    role: source.role,
                    member_name: Arc::from(source.name.as_str()),
                    occurrence: source.occurrence.clone(),
                    source: source.source.clone(),
                },
            },
        )
        .collect()
}

impl<M: verter_audit::observe::ObserveExec> ExecutorResolveCtx<'_, M> {
    /// Resolve every planned demand into a [`ResolvedSurfaces`] set.
    fn resolve_plan(
        &self,
        selector: &ResolvedComponentSelector,
        plan: crate::typeinfo::framework_surface::plan::FrameworkSurfacePlan,
    ) -> ResolvedSurfaces {
        let items = plan
            .items
            .into_iter()
            .map(|planned| ResolvedItem {
                kind: planned.kind,
                result: self.resolve_demand(selector, planned.kind, planned.demand),
            })
            .collect();
        ResolvedSurfaces { items }
    }

    /// Resolve ONE planned demand through the shared engine.
    ///
    /// EXHAUSTIVE match over the closed [`PlannedDemand`] taxonomy — NO wildcard
    /// arm. Adding a variant breaks this match (the closed-vocabulary
    /// discipline).
    fn resolve_demand(
        &self,
        _selector: &ResolvedComponentSelector,
        requested_kind: FrameworkSurfaceKind,
        demand: PlannedDemand,
    ) -> ResolvedDemand {
        // PER-DEMAND completeness. The shared engine's degradation reaches
        // this layer through the request/compute completeness rails, not
        // through `ResolvedOutcome` — so without this read a resolution that
        // FAILED to determine the surface encoded as `SUPPORTED` +
        // `GRAPH_EXACTNESS_EXACT_RESOLVED`, byte-identical to a clean one.
        // Scoped PER DEMAND so one kind's degradation never downgrades a
        // sibling kind, and BUBBLED (not discarded) so the enclosing request
        // still records it.
        let scope = verter_type_engine::request_context::ColdComputeCompletenessScope::enter();
        let resolved = match demand {
            PlannedDemand::MacroPayload { owner, selector } => ResolvedDemand::MacroPayload(
                self.resolve_macro_payload(&owner, requested_kind, &selector),
            ),
            PlannedDemand::PathProjection { base, path, mode } => {
                ResolvedDemand::PathProjection(self.resolve_path_projection(&base, &path, mode))
            }
            PlannedDemand::ShallowSurface { node } => {
                ResolvedDemand::ShallowSurface(self.resolve_shallow_surface(&node))
            }
            PlannedDemand::SvelteSurface { owner, source } => {
                let outcome =
                    crate::typeinfo::framework_surface::svelte_exec::resolve_svelte_surface(
                        self.ctx,
                        self.dispatch,
                        &owner,
                        source,
                    );
                if let ResolvedOutcome::Resolved(dtos) = &outcome {
                    self.collect_member_sources(
                        requested_kind,
                        crate::typeinfo::node_refs::MemberProducer::Svelte(source),
                        dtos,
                    );
                }
                ResolvedDemand::SvelteSurface(outcome)
            }
        };
        let observed = verter_type_engine::request_context::current_cold_compute_completeness();
        drop(scope);
        match observed {
            verter_type_engine::semantic_query::ResultCompleteness::Complete => resolved,
            verter_type_engine::semantic_query::ResultCompleteness::Partial(reasons) => resolved
                .degraded_by(&format!(
                    "framework surface resolution was degraded: {reasons:?}"
                )),
        }
    }

    /// Resolve a macro payload through the relocated Vue macro-DTO delegate.
    ///
    /// AGGREGATES across EVERY macro of the requested kind in the owner's
    /// AUTHORITATIVE shallow snapshot — a component with two `defineModel` calls
    /// surfaces both bindings, not just the first a `macro_index` hint would
    /// pick. PROPS additionally folds in EVERY `defineModel`'s synthesized prop
    /// (a `defineModel` contributes a prop to the component's `$props`), so a
    /// `defineModel`-only component still surfaces its `modelValue` prop.
    ///
    /// The [`MacroPayloadSelector`]'s `macro_index` is INTENTIONALLY ignored in
    /// this keystone adapter: a surface kind always aggregates EVERY contributing
    /// macro of that kind (the §9 surface contract), never a single
    /// index-selected macro. A future request-narrowing vertical that needs a
    /// single macro reads `macro_index` then; until then aggregation is the only
    /// correct behavior and the hint is a no-op.
    ///
    /// [`MacroPayloadSelector`]: crate::typeinfo::framework_surface::plan::MacroPayloadSelector
    fn resolve_macro_payload(
        &self,
        owner: &str,
        requested_kind: FrameworkSurfaceKind,
        _selector: &crate::typeinfo::framework_surface::plan::MacroPayloadSelector,
    ) -> ResolvedMacroPayload {
        use verter_session_query::analysis::types::AnalyzedMacroKind;

        // The owner snapshot AND every macro DTO read flow through the ONE
        // request-bound `ctx`, so the whole response resolves against a single
        // coherent owner version.
        let Some(indexed) = self
            .ctx
            .ensure_indexed_ready_serve(owner)
            .map(|serve| serve.indexed)
        else {
            return ResolvedOutcome::Missing;
        };

        // Which macro kinds contribute to the requested surface kind. PROPS is
        // contributed by BOTH `defineProps` AND every `defineModel` (a model
        // binding is also a prop); MODEL is contributed only by `defineModel`;
        // EMITS / SLOTS / OPTIONS / EXPOSE by their own macro kind. The object
        // surfaces (`defineOptions<T>` / `defineExpose<T>`) resolve through the
        // SAME shared object-surface projection as props/emits/slots — they are
        // SUPPORTED-with-members, never unsupported-because-present. The
        // requested WIRE kind is the authority (not the selector's macro kind),
        // because `defineModel` contributes to two surface kinds.
        let contributes = |mac_kind: AnalyzedMacroKind| -> bool {
            match requested_kind {
                FrameworkSurfaceKind::Props => matches!(
                    mac_kind,
                    AnalyzedMacroKind::DefineProps | AnalyzedMacroKind::DefineModel
                ),
                FrameworkSurfaceKind::Emits => mac_kind == AnalyzedMacroKind::DefineEmits,
                FrameworkSurfaceKind::Slots => mac_kind == AnalyzedMacroKind::DefineSlots,
                FrameworkSurfaceKind::Model => mac_kind == AnalyzedMacroKind::DefineModel,
                FrameworkSurfaceKind::Options => mac_kind == AnalyzedMacroKind::DefineOptions,
                FrameworkSurfaceKind::Expose => mac_kind == AnalyzedMacroKind::DefineExpose,
            }
        };

        // Resolve EVERY contributing macro and fold the requested slot across
        // all of them into one aggregate bundle.
        let mut aggregate = MacroSurfaceDtos::default();
        let mut any = false;
        let mut any_partial = false;
        for (macro_index, mac) in indexed.snapshot.macros.iter().enumerate() {
            if !contributes(mac.kind) {
                continue;
            }
            any = true;
            let request = VueMacroSurfaceRequest {
                owner_canonical: Arc::from(owner),
                macro_index,
                macro_kind: mac.kind,
                root_identity: indexed.whole_hash,
                level: TypeInfoQueryLevel::FullMetadata,
            };
            let dtos_read = crate::typeinfo::framework_surface::vue_exec::vue_macro_dtos_with_ctx(
                self.ctx,
                self.dispatch,
                &request,
            )
            .unwrap_or_else(
                crate::typeinfo::framework_surface::MacroDtosRefusal::into_partial_read,
            );
            any_partial |= dtos_read.is_partial();
            if let Ok(index) = u32::try_from(macro_index) {
                self.collect_member_sources(
                    requested_kind,
                    crate::typeinfo::node_refs::MemberProducer::VueMacro {
                        kind: mac.kind,
                        index,
                    },
                    &dtos_read.dtos,
                );
            }
            fold_requested_slot(&mut aggregate, requested_kind, &dtos_read.dtos);
        }
        if !any {
            // No macro of the requested kind (nor a model contributing to props)
            // — the selector has no such surface.
            return ResolvedOutcome::Missing;
        }
        if any_partial {
            // A contributing macro surface tripped a fuse mid-resolution: the
            // aggregate carries a usable subset but is structurally incomplete.
            // Surface PARTIAL (maps to the wire `PARTIAL` support status) rather
            // than masquerading as a fully-resolved surface.
            return ResolvedOutcome::Partial {
                value: Arc::new(aggregate),
                diagnostics: vec!["macro surface resolution hit a budget / fatal diagnostic; \
                     surface is partial"
                    .to_string()],
            };
        }
        ResolvedOutcome::Resolved(Arc::new(aggregate))
    }

    /// Record every replayable member source `dtos` (one producer's bundle)
    /// contributes to `requested_kind`: a props row's value authority, an
    /// exposed row's value source, an options member's, a slot's or a model
    /// binding's value source, or an emit occurrence's payload source. Closed, synthesized, failed and absent positions are not
    /// addresses and are skipped. Each source replays under the owner its
    /// authored anchor is written in.
    fn collect_member_sources(
        &self,
        requested_kind: FrameworkSurfaceKind,
        producer: crate::typeinfo::node_refs::MemberProducer,
        dtos: &MacroSurfaceDtos,
    ) {
        use crate::typeinfo::node_refs::{is_replayable_source, source_scope_owner, MemberRole};
        type Row<'r> = (
            MemberRole,
            &'r str,
            Option<&'r verter_type_expr::facts::SemanticTypeSource>,
        );
        // An emit payload is keyed by its occurrence: overloads of one event
        // name are distinct members.
        let occurrences: Vec<Option<&verter_type_expr::facts::ResolvedEmitOccurrenceId>> =
            match requested_kind {
                FrameworkSurfaceKind::Emits => {
                    dtos.emit_fields().iter().map(|row| Some(&row.id)).collect()
                }
                _ => Vec::new(),
            };
        let rows: Vec<Row<'_>> = match requested_kind {
            FrameworkSurfaceKind::Props => dtos
                .prop_fields()
                .iter()
                .map(|row| {
                    (
                        MemberRole::Prop,
                        row.analysis.name.as_str(),
                        row.authority.source(),
                    )
                })
                .collect(),
            FrameworkSurfaceKind::Expose => dtos
                .exposed_fields
                .iter()
                .map(|row| {
                    (
                        MemberRole::Exposed,
                        row.analysis.name.as_str(),
                        row.type_source.present(),
                    )
                })
                .collect(),
            FrameworkSurfaceKind::Options => dtos
                .option_sources
                .iter()
                .map(|row| {
                    (
                        MemberRole::OptionsMember,
                        row.name.as_str(),
                        row.source.present(),
                    )
                })
                .collect(),
            FrameworkSurfaceKind::Slots => dtos
                .slot_sources
                .iter()
                .map(|row| (MemberRole::Slot, row.name.as_str(), row.source.present()))
                .collect(),
            FrameworkSurfaceKind::Model => dtos
                .model
                .as_ref()
                .map(|surface| {
                    surface
                        .bindings
                        .iter()
                        .map(|binding| {
                            (
                                MemberRole::Model,
                                binding.name.as_str(),
                                binding.source.as_ref(),
                            )
                        })
                        .collect()
                })
                .unwrap_or_default(),
            FrameworkSurfaceKind::Emits => dtos
                .emit_fields()
                .iter()
                .map(|row| {
                    (
                        MemberRole::EmitPayload,
                        row.name.as_str(),
                        row.payload_source.present(),
                    )
                })
                .collect(),
        };
        let mut members = self.members.borrow_mut();
        for (index, (role, name, source)) in rows.into_iter().enumerate() {
            let Some(source) = source.filter(|source| is_replayable_source(source)) else {
                continue;
            };
            let Some(scope_owner) = source_scope_owner(source) else {
                continue;
            };
            members.push(MemberSource {
                surface: requested_kind,
                role,
                name: name.to_string(),
                occurrence: occurrences.get(index).copied().flatten().cloned(),
                producer,
                scope_owner,
                source: source.clone(),
            });
        }
    }

    /// Resolve a path projection off a base node handle through the shared
    /// path-projection dispatch.
    ///
    /// Not exercised by the Vue adapter (Vue plans only macro-payload
    /// demands), but a real, non-stub resolution for later
    /// framework verticals: it resolves the base declaration's carrier then
    /// projects `path` through the shared shallow-surface dispatch.
    ///
    /// A framework SURFACE is inherently the one-level `Shallow` projection of
    /// the terminal hop, so the only legal terminal `mode` is `Shallow`. A
    /// non-`Shallow` terminal mode is honored explicitly — it routes to a typed
    /// `Unsupported` outcome rather than being silently coerced to `Shallow`
    /// (which would resolve a surface the caller did not request).
    fn resolve_path_projection(
        &self,
        base: &crate::typeinfo::framework_surface::plan::TypeNodeHandle,
        path: &[verter_type_engine::semantic_query::PathSegment],
        mode: ProjectionMode,
    ) -> ResolvedOutcome<crate::typeinfo::surface::TypeInfoSurface> {
        use verter_type_engine::semantic_query::{
            ProjectionReductionContext, QueryResult, ResolveDeclKey, ScopeId, SemanticQueryApi,
            SemanticQueryKey, SemanticQueryOutput,
        };
        if mode != ProjectionMode::Shallow {
            // The framework-surface member surface is one-level shallow; a
            // non-Shallow terminal projection is not representable here.
            return ResolvedOutcome::Unsupported {
                diagnostics: vec![
                    "framework-surface path projection supports only the Shallow terminal mode"
                        .to_string(),
                ],
            };
        }
        // Resolve against the ONE request-bound `ctx` (NOT a fresh view) so this
        // demand shares the same coherent owner version as the rest of the
        // response. the borrowed request facade keeps the surface inside the single engine.
        let dispatch = self.dispatch;
        // Resolve the base declaration CARRIER through the shared engine — the
        // SAME `ResolveDecl` a named-declaration shallow surface resolves
        // through — then project the path-precise selector. The path walker runs
        // intermediate hops in `Navigate` and the terminal hop under the
        // one-level `Shallow` surface synthesiser. NO second resolver.
        let decl = match dispatch.execute_type_node(SemanticQueryKey::ResolveDecl(ResolveDeclKey {
            scope: ScopeId {
                canonical_id: verter_type_expr::OwnedStr::from(base.owner_canonical.as_ref()),
                owner: verter_type_expr::TopLevelOwnerId::ordinary_file(),
                local_scope: None,
                binder_scope_id: verter_type_engine::semantic_query::BinderScopeId::file_scope(
                    verter_type_expr::TopLevelOwnerId::ordinary_file(),
                ),
            },
            name: verter_type_expr::OwnedStr::from(base.symbol_name.as_ref()),
        })) {
            QueryResult::Value(SemanticQueryOutput { value: node, .. }) => node,
            QueryResult::Recursive(node) => node,
            QueryResult::Error(_) => return ResolvedOutcome::Missing,
        };
        match crate::typeinfo::shallow_surface::project_shallow_surface_from_base(
            self.ctx,
            dispatch,
            decl,
            verter_type_expr::OwnedSlice::from(path.to_vec().into_boxed_slice()),
            ProjectionReductionContext::published(ProjectionMode::Shallow),
            None,
        ) {
            verter_type_engine::semantic_query::surface_resolution::SurfaceResolution::Resolved(surface)
            | verter_type_engine::semantic_query::surface_resolution::SurfaceResolution::OpenPresence(surface) => {
                ResolvedOutcome::Resolved(surface.into_inner())
            }
            verter_type_engine::semantic_query::surface_resolution::SurfaceResolution::NoSurface(_) => {
                ResolvedOutcome::Missing
            }
            // A failed projection is PARTIAL on the wire — its typed reason is
            // recorded and the usable subset (if any) rides the partial arm;
            // it never encodes as an absent surface.
            verter_type_engine::semantic_query::surface_resolution::SurfaceResolution::Incomplete(
                incomplete,
            ) => {
                let diagnostics = vec![format!(
                    "shallow-surface-unresolved::{:?}",
                    incomplete.reasons()
                )];
                let value = incomplete
                    .into_recorded_partial()
                    .unwrap_or_else(crate::typeinfo::surface::TypeInfoSurface::empty);
                ResolvedOutcome::Partial { value, diagnostics }
            }
        }
    }

    /// Resolve a node handle's one-level shallow surface through the shared
    /// shallow-surface dispatch.
    ///
    /// Not exercised by the Vue adapter, but a real resolution: the handle's
    /// owner + symbol name route through the SAME shared decl-resolve +
    /// one-level `Shallow` synthesis a named TS declaration resolves through —
    /// an EMPTY-path projection of the base declaration, against the ONE
    /// request-bound `ctx` (so it shares the response's coherent owner version).
    fn resolve_shallow_surface(
        &self,
        node: &crate::typeinfo::framework_surface::plan::TypeNodeHandle,
    ) -> ResolvedOutcome<crate::typeinfo::surface::TypeInfoSurface> {
        self.resolve_path_projection(node, &[], ProjectionMode::Shallow)
    }
}

/// Fold the requested surface kind's slot from one resolved macro bundle into
/// the aggregate. PROPS / EMITS append fields + index signatures; SLOTS append
/// slot fields; MODEL appends model bindings; OPTIONS / EXPOSE append the
/// object-surface named members. A resolved-but-empty surface still establishes
/// the slot present (supported-empty), distinct from a component that has no
/// such macro at all (the `Missing` outcome upstream).
fn fold_requested_slot(
    aggregate: &mut MacroSurfaceDtos,
    requested: FrameworkSurfaceKind,
    dtos: &MacroSurfaceDtos,
) {
    use crate::typeinfo::framework_surface::results::{
        EmitsSurface, ExposeSurface, OptionsSurface, PropsSurface,
    };
    match requested {
        FrameworkSurfaceKind::Props => {
            let target = aggregate.props.get_or_insert_with(PropsSurface::default);
            target.fields.extend(dtos.prop_fields().iter().cloned());
            target
                .index_signatures
                .extend(dtos.prop_index_signatures().iter().cloned());
        }
        FrameworkSurfaceKind::Emits => {
            let target = aggregate.emits.get_or_insert_with(EmitsSurface::default);
            target.fields.extend(dtos.emit_fields().iter().cloned());
            target
                .index_signatures
                .extend(dtos.emit_index_signatures().iter().cloned());
        }
        FrameworkSurfaceKind::Slots => {
            if !dtos.slot_fields().is_empty() {
                aggregate
                    .slots
                    .get_or_insert_with(Vec::new)
                    .extend(dtos.slot_fields().iter().cloned());
                aggregate
                    .slot_return_publications
                    .extend(dtos.slot_return_publications().iter().cloned());
                aggregate
                    .slot_member_types
                    .extend(dtos.slot_member_types.iter().cloned());
            } else {
                // A slots macro that resolved an empty surface still establishes
                // the slot bundle as present (supported-empty), distinct from a
                // component with no defineSlots at all.
                aggregate.slots.get_or_insert_with(Vec::new);
            }
        }
        FrameworkSurfaceKind::Model => {
            if let Some(model) = &dtos.model {
                aggregate
                    .model
                    .get_or_insert_with(Default::default)
                    .bindings
                    .extend(model.bindings.iter().cloned());
            }
        }
        FrameworkSurfaceKind::Options => {
            // `defineOptions<T>()` is an object-member surface; fold its members,
            // establishing the slot present even when the surface is empty.
            let target = aggregate
                .options
                .get_or_insert_with(OptionsSurface::default);
            if let Some(options) = &dtos.options {
                target.members.extend(options.members.iter().cloned());
            }
        }
        FrameworkSurfaceKind::Expose => {
            let target = aggregate.expose.get_or_insert_with(ExposeSurface::default);
            if let Some(expose) = &dtos.expose {
                target.members.extend(expose.members.iter().cloned());
            }
        }
    }
}

/// The audit target, best-effort from the envelope's framework selector.
///
/// A missing selector or empty selector canonical is a malformed request with
/// no document target, so it uses `NotApplicable` explicitly.
pub(crate) fn framework_selector_target(
    envelope: &TypeInfoGraphRequest,
) -> (String, verter_audit::RequestTargetIdentity) {
    match &envelope.payload {
        Some(verter_protocol::verter::v1::type_info_graph_request::Payload::FrameworkSurface(
            r,
        )) => match &r.selector {
            Some(selector) if selector.canonical_id.is_empty() => (
                String::new(),
                verter_audit::RequestTargetIdentity::NotApplicable,
            ),
            Some(selector) => (
                selector.canonical_id.clone(),
                verter_audit::RequestTargetIdentity::registered(selector.canonical_id.clone()),
            ),
            None => (
                String::new(),
                verter_audit::RequestTargetIdentity::NotApplicable,
            ),
        },
        _ => (
            String::new(),
            verter_audit::RequestTargetIdentity::NotApplicable,
        ),
    }
}

/// The typed selector echo: the request's file and export, qualified by the
/// reference of the profile the host ADMITTED (built from its sealed
/// identity, never copied from the request bytes).
fn admitted_selector_echo(
    selector: &ComponentSelector,
    admitted: &RegisteredFrameworkProfile,
) -> ComponentSelector {
    ComponentSelector {
        canonical_id: selector.canonical_id.clone(),
        export_name: selector.export_name.clone(),
        has_export_name: selector.has_export_name,
        framework_profile: Some(admitted.wire_ref()),
    }
}

/// The request's facet demand in enum-tag (canonical) order.
///
/// The validator has proved `requested_kinds` non-empty, known and
/// duplicate-free (every accepted framework-surface schema carries the
/// demand), so this is a pure filter of [`ALL_FRAMEWORK_SURFACE_KINDS`] that
/// keeps the canonical order whatever order the client wrote.
fn requested_surface_kinds(request: &wire::FrameworkSurfaceRequest) -> Vec<FrameworkSurfaceKind> {
    ALL_FRAMEWORK_SURFACE_KINDS
        .iter()
        .copied()
        .filter(|kind| request.requested_kinds.contains(&(*kind as i32)))
        .collect()
}

fn framework_payload(surfaces: &[wire::FrameworkSurfaceKindEntry]) -> TypeInfoGraphPayload {
    let mut support_counts = std::collections::BTreeMap::new();
    for entry in surfaces {
        let support = entry
            .status
            .as_ref()
            .map(|s| s.support)
            .unwrap_or(FrameworkSurfaceKindSupport::Unspecified as i32);
        let tag = match support {
            x if x == FrameworkSurfaceKindSupport::Supported as i32 => {
                FrameworkSurfaceKindSupportTag::Supported
            }
            x if x == FrameworkSurfaceKindSupport::Unsupported as i32 => {
                FrameworkSurfaceKindSupportTag::Unsupported
            }
            x if x == FrameworkSurfaceKindSupport::Partial as i32 => {
                FrameworkSurfaceKindSupportTag::Partial
            }
            _ => FrameworkSurfaceKindSupportTag::Unspecified,
        };
        *support_counts.entry(tag).or_insert(0) += 1;
    }
    TypeInfoGraphPayload {
        operation: GraphOperationTag::FrameworkSurfaces,
        mode: ProjectionModeTag::default(),
        schema_version: wire::TYPEINFO_GRAPH_SCHEMA_VERSION,
        framework_surface_entry_count: u32::try_from(surfaces.len()).unwrap_or(u32::MAX),
        framework_surface_support_counts: support_counts,
        ..TypeInfoGraphPayload::empty()
    }
}

/// Build the `error` response arm.
fn error_response(error: TypeInfoRequestError) -> TypeInfoGraphResponse {
    TypeInfoGraphResponse {
        kind: Some(type_info_graph_response::Kind::Error(error)),
    }
}

/// Build the `framework_surface` response arm.
fn framework_surface_response(
    schema_version: u32,
    selector: ComponentSelector,
    framework: FrameworkTag,
    graph: wire::SemanticTypeGraph,
    surfaces: Vec<wire::FrameworkSurfaceKindEntry>,
) -> TypeInfoGraphResponse {
    TypeInfoGraphResponse {
        kind: Some(type_info_graph_response::Kind::FrameworkSurface(
            FrameworkSurfacePayload {
                schema_version,
                selector: Some(selector),
                framework: framework as i32,
                graph: Some(graph),
                surfaces,
            },
        )),
    }
}

/// Build a typed `MalformedPayload` wire error with detail.
fn malformed(detail: &str) -> TypeInfoRequestError {
    TypeInfoRequestError {
        kind: Some(type_info_request_error::Kind::MalformedPayload(
            verter_protocol::typeinfo::graph::wire_error_malformed_payload(detail),
        )),
    }
}
