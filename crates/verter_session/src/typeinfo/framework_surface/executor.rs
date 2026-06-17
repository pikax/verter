#![deny(missing_docs)]
//! The framework-surface executor — the single audited entry-point for the
//! `GRAPH_OPERATION_FRAMEWORK_SURFACES` wire operation.
//!
//! [`VerterHost::resolve_framework_surface_with_audit`] is the one public entry.
//! It runs the existing [`validate_type_info_graph_request`] FIRST (the envelope
//! validator — operation/payload-arm match, schema echo, and the nested
//! framework-surface validator), so a malformed envelope is rejected with a
//! typed wire error BEFORE any registry lookup or semantic dispatch. Only the
//! validated inner request reaches the module-private executor body.
//!
//! Executor flow (post-validation):
//! 1. Intern `selector.framework_adapter_id`; registry lookup. An unknown id is
//!    a typed `MalformedPayload` (NO new error variant).
//! 2. Resolve the selector to a [`ResolvedComponentSelector`] (default export
//!    via the synthesized `default`; named export via the shallow inventory).
//! 3. The requested set is ALWAYS [`ALL_FRAMEWORK_SURFACE_KINDS`] (the wire
//!    request carries no requested-kind field). The response carries EXACTLY ONE
//!    entry per known kind.
//! 4. A [`SurfaceRegistration::Deferred`] adapter answers every kind
//!    structurally UNSUPPORTED (a structural response, NOT an error).
//! 5. A [`SurfaceRegistration::Adapter`] adapter PLANS its demands against the
//!    facts/carrier-only [`FrameworkAdapterCtx`], the executor RESOLVES each
//!    [`PlannedDemand`] through the module-private [`ExecutorResolveCtx`] (an
//!    EXHAUSTIVE match — no wildcard arm — over the closed 5-variant taxonomy),
//!    the adapter NORMALIZES the resolved data, and [`graph_export`] encodes the
//!    `FrameworkSurfacePayload`.
//!
//! The executor resolves EVERY [`PlannedDemand`] THROUGH the one shared
//! type-resolution engine (the relocated Vue delegates / `resolve_shallow_surface_for`
//! / `project_shallow_surface_from_base`). It is NOT a second resolver — it
//! plans, dispatches to the shared engine, and encodes.

use std::sync::atomic::Ordering;
use std::sync::Arc;

use verter_audit::payloads::typeinfo_graph::{
    FrameworkSurfaceKindSupportTag, GraphOperationTag, TypeInfoGraphPayload,
};
use verter_audit::{
    AuditedResult, ProjectionModeTag, RequestAuditRecord, RequestKind, RequestKindPayload,
    RequestMemoryAudit, RequestStoreAudit, RequestTimingAudit, WaitAudit,
};
use verter_language::FrameworkAdapterId;
use verter_protocol::typeinfo::graph::{
    self as wire, ComponentSelector, FrameworkSurfaceKind, FrameworkSurfaceKindSupport,
    FrameworkSurfacePayload, FrameworkTag, TypeInfoGraphRequest, TypeInfoGraphResponse,
    TypeInfoRequestError,
};
use verter_protocol::verter::v1::{type_info_graph_response, type_info_request_error};

use crate::framework::ctx::FrameworkAdapterCtx;
use crate::framework::descriptor::ALL_FRAMEWORK_SURFACE_KINDS;
use crate::framework::registry::SurfaceRegistration;
use crate::host_audit_runtime::AuditRequestRegistration;
use crate::instant::Instant;
use crate::request_context::{RequestContext, RequestContextGuard};
use crate::semantic_query::ProjectionMode;
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

/// The audit operation tag the framework-surface executor registers under.
///
/// Re-exported so binding-layer plumbing and tests reference the same tag the
/// executor records instead of re-spelling it.
pub const FRAMEWORK_SURFACE_AUDIT_OPERATION: GraphOperationTag =
    GraphOperationTag::FrameworkSurfaces;

impl VerterHost {
    /// Resolve a component's framework surfaces, returning the wire
    /// [`TypeInfoGraphResponse`] (the `framework_surface` arm on success, the
    /// `error` arm on a typed rejection) plus the request's audit record.
    ///
    /// The audited entry takes the FULL wire envelope and runs
    /// [`validate_type_info_graph_request`](crate::typeinfo::request_validation::validate_type_info_graph_request)
    /// FIRST — a bare-inner-request entry is forbidden, since it would reopen the
    /// op/payload-mismatch and schema-echo invalid states the envelope validator
    /// closes. A validation failure returns the typed wire error in the response
    /// `error` arm; no registry lookup or semantic dispatch runs.
    #[must_use]
    pub fn resolve_framework_surface_with_audit(
        &self,
        envelope: TypeInfoGraphRequest,
    ) -> AuditedResult<TypeInfoGraphResponse, TypeInfoRequestError> {
        let request_id = self.next_request_id();
        crate::request_context::increment_requests_created();

        let footprint_capture = self.config.footprint_capture && self.config.audit_enabled;
        let timing_capture = self.config.audit_timing_capture && self.config.audit_enabled;
        // Best-effort canonical id for the audit record (the selector may be
        // absent on a malformed envelope).
        let canonical_for_audit = framework_selector_canonical(&envelope);
        let ctx = RequestContext::with_kind_timing_and_projection_budget(
            request_id,
            Arc::<str>::from(canonical_for_audit.as_str()),
            RequestKind::TypeInfoGraph,
            footprint_capture,
            timing_capture,
            None,
            self.config.projection_op_budget,
        );
        let registration = Arc::new(AuditRequestRegistration::new(self, Arc::clone(&ctx)));
        let _ = ctx.install_audit_registration(Arc::clone(&registration));

        let request_start = Instant::now();
        let (response, payload) = match registration.as_ref() {
            AuditRequestRegistration::Active(_) => {
                let _ctx_guard = RequestContextGuard::install(Arc::clone(&ctx));
                self.execute_framework_surface(envelope)
            }
            AuditRequestRegistration::Noop => {
                let _noop_guard = verter_audit::install_noop_observer();
                self.execute_framework_surface(envelope)
            }
        };
        let total_ms = request_start.elapsed().as_secs_f64() * 1000.0;

        // VALIDATION-FIRST CONTRACT: the response is the `error` arm exactly when
        // validation (or selector resolution) rejected the request. Both arms
        // are audited identically.
        let outcome = framework_response_outcome(&response);

        if matches!(registration.as_ref(), AuditRequestRegistration::Noop) {
            let state = if self.config.audit_enabled {
                verter_audit::AuditCaptureState::FilteredNoop
            } else {
                verter_audit::AuditCaptureState::AuditDisabled
            };
            let record = noop_framework_record(
                request_id,
                &canonical_for_audit,
                ctx.parent_request_id,
                ctx.trace_id.clone(),
                state,
                payload,
            );
            return audited_from_outcome(response, outcome, record);
        }

        let timings = RequestTimingAudit {
            total_ms,
            ..RequestTimingAudit::default()
        };
        let store = RequestStoreAudit {
            cache_layers: crate::component_meta_audit::snapshot_cache_layers_from_tls(),
            bypass_diagnostics: crate::component_meta_audit::snapshot_bypass_diagnostics_from_tls(),
            ..RequestStoreAudit::default()
        };
        let memory = RequestMemoryAudit {
            process_rss_peak_bytes: ctx.process_rss_peak_bytes.load(Ordering::Relaxed),
            ..RequestMemoryAudit::default()
        };
        let waits = if ctx.timing_capture {
            Some(WaitAudit {
                lock_wait_ns: ctx.lock_wait_ns.load(Ordering::Relaxed),
                queue_wait_ns: ctx.queue_wait_ns.load(Ordering::Relaxed),
                lock_acquisitions: ctx.lock_acquisitions.load(Ordering::Relaxed),
            })
        } else {
            None
        };

        let record = RequestAuditRecord {
            request_id,
            canonical_id: canonical_for_audit,
            kind: RequestKind::TypeInfoGraph,
            parent_request_id: ctx.parent_request_id.map(|id| id.to_string()),
            from_cache: false,
            timings,
            memory,
            store,
            footprint: None,
            scheduler: ctx.scheduler_audit.lock().clone(),
            files: Vec::new(),
            waits,
            kind_payload: RequestKindPayload::TypeInfoGraph(payload),
            capture_state: verter_audit::AuditCaptureState::ActiveStored,
            trace_id: ctx.trace_id.clone(),
        };

        let cloned = record.clone();
        registration.finalize(record);
        audited_from_outcome(response, outcome, cloned)
    }

    /// The validation-first executor body. Returns the wire response plus the
    /// audit payload. NEVER reaches registry/semantic work before
    /// `validate_type_info_graph_request` returns `Ok`.
    fn execute_framework_surface(
        &self,
        envelope: TypeInfoGraphRequest,
    ) -> (TypeInfoGraphResponse, TypeInfoGraphPayload) {
        // VALIDATION FIRST — a malformed envelope (op/payload mismatch, schema
        // echo divergence, missing selector) is rejected here before any
        // registry lookup or semantic dispatch.
        let validated = match crate::typeinfo::request_validation::validate_type_info_graph_request(
            &envelope,
        ) {
            Ok(v) => v,
            Err(error) => {
                let payload = TypeInfoGraphPayload::from_validation_error(
                    GraphOperationTag::FrameworkSurfaces,
                );
                return (error_response(error), payload);
            }
        };
        let request = validated.into_inner();
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

        // The validator proved the selector is present and both ids non-empty.
        let selector = framework_request
            .selector
            .clone()
            .expect("validator proves selector present");
        let adapter_id = FrameworkAdapterId::new(&selector.framework_adapter_id);

        let Some(registration) = self.framework_registry().get(&adapter_id) else {
            // An unknown adapter id is a malformed payload (NO new error
            // variant), surfaced AFTER validation but before any semantic work.
            let error = malformed(&format!(
                "no framework adapter registered for id `{adapter_id}`"
            ));
            let payload =
                TypeInfoGraphPayload::from_validation_error(GraphOperationTag::FrameworkSurfaces);
            return (error_response(error), payload);
        };

        let resolved_selector = self.resolve_component_selector(&selector);

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
                let encoded = encode_framework_surfaces_with_unsupported_message(
                    &NormalizedSurfaces::default(),
                    ALL_FRAMEWORK_SURFACE_KINDS,
                    &[] as &[FrameworkSurfaceKind],
                    "canonical is not a component carrier for this adapter \
                     (no component surface to resolve)",
                );
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
                let plan =
                    adapter.plan_surfaces(&ctx, &resolved_selector, ALL_FRAMEWORK_SURFACE_KINDS);
                // Capture ONE proven-current request view AFTER validation and
                // thread it through EVERY demand resolver — so all of a single
                // response's props / emits / slots / model resolve against ONE
                // coherent owner version, never a mix from different versions
                // under churn. On sustained churn the view capture fails and the
                // adapter normalizes an EMPTY resolved set (the established
                // query-returner miss → every supported kind decodes
                // supported-empty).
                match crate::typeinfo::current_store_view_for_query(self) {
                    Some(current_view) => {
                        let overlay =
                            Arc::new(crate::resolver_core::CanonicalCompletionOverlay::new());
                        let host_ctx = crate::resolver_core::HostResolverContext::from_current(
                            self,
                            &current_view,
                            overlay,
                        );
                        let resolve_ctx = ExecutorResolveCtx {
                            host: self,
                            ctx: &host_ctx,
                        };
                        let resolved = resolve_ctx.resolve_plan(&resolved_selector, plan);
                        adapter.normalize(&ctx, resolved)
                    }
                    None => NormalizedSurfaces::default(),
                }
            }
        };

        // The descriptor's supported set decides supported-empty vs structural
        // UNSUPPORTED per kind. A Deferred registration's normalized set is
        // empty, so every kind fills supported-empty — but the design's D-s
        // requirement is a Deferred adapter answers UNSUPPORTED; pass an EMPTY
        // supported set for Deferred so every kind fills structurally
        // UNSUPPORTED.
        let supported = match &registration.surface {
            SurfaceRegistration::Deferred => &[] as &[FrameworkSurfaceKind],
            SurfaceRegistration::Adapter(_) => registration.descriptor.supported_surfaces,
        };

        // A Deferred adapter's per-kind UNSUPPORTED diagnostic names the
        // intermediate state explicitly (D-ag) — the surfaces are not yet
        // registered, distinct from a supported adapter's per-kind unsupport.
        let encoded = match &registration.surface {
            SurfaceRegistration::Deferred => encode_framework_surfaces_with_unsupported_message(
                &normalized,
                ALL_FRAMEWORK_SURFACE_KINDS,
                supported,
                "framework surfaces are not yet registered for this adapter",
            ),
            SurfaceRegistration::Adapter(_) => {
                encode_framework_surfaces(&normalized, ALL_FRAMEWORK_SURFACE_KINDS, supported)
            }
        };

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
    /// carrier components).
    fn resolve_component_selector(
        &self,
        selector: &ComponentSelector,
    ) -> ResolvedComponentSelector {
        let canonical: Arc<str> = Arc::from(selector.canonical_id.as_str());
        let export = if selector.has_export_name && !selector.export_name.is_empty() {
            ComponentExport::Named(Arc::from(selector.export_name.as_str()))
        } else {
            ComponentExport::Default
        };
        ResolvedComponentSelector { canonical, export }
    }
}

/// The module-private resolve context the executor drives each
/// [`PlannedDemand`] through. NEVER exported, NEVER passed to adapter code.
///
/// It resolves each demand THROUGH the one shared type-resolution engine (the
/// relocated Vue delegates and the shared shallow-surface / path-projection
/// dispatch). It is not a second resolver: every arm dispatches to existing
/// shared resolution.
struct ExecutorResolveCtx<'a> {
    host: &'a VerterHost,
    /// The ONE proven-current request view every demand resolves against, so a
    /// single response never mixes owner versions under churn.
    ctx: &'a dyn crate::resolver_core::ResolverContext,
}

impl ExecutorResolveCtx<'_> {
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
        match demand {
            PlannedDemand::MacroPayload { owner, selector } => ResolvedDemand::MacroPayload(
                self.resolve_macro_payload(&owner, requested_kind, &selector),
            ),
            PlannedDemand::PathProjection { base, path, mode } => {
                ResolvedDemand::PathProjection(self.resolve_path_projection(&base, &path, mode))
            }
            PlannedDemand::ShallowSurface { node } => {
                ResolvedDemand::ShallowSurface(self.resolve_shallow_surface(&node))
            }
            PlannedDemand::SvelteSurface { owner, source } => ResolvedDemand::SvelteSurface(
                crate::typeinfo::framework_surface::svelte_exec::resolve_svelte_surface(
                    self.host, self.ctx, &owner, source,
                ),
            ),
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
        use verter_semantic::analysis::types::AnalyzedMacroKind;

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
            let dtos = crate::typeinfo::framework_surface::vue_exec::vue_macro_dtos_with_ctx(
                self.ctx, &request,
            );
            fold_requested_slot(&mut aggregate, requested_kind, &dtos);
        }
        if !any {
            // No macro of the requested kind (nor a model contributing to props)
            // — the selector has no such surface.
            return ResolvedOutcome::Missing;
        }
        ResolvedOutcome::Resolved(Arc::new(aggregate))
    }

    /// Resolve a path projection off a base node handle through the shared
    /// path-projection dispatch.
    ///
    /// Not exercised by the Vue adapter (Vue plans only public-type +
    /// macro-payload demands), but a real, non-stub resolution for later
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
        path: &[crate::semantic_query::PathSegment],
        mode: ProjectionMode,
    ) -> ResolvedOutcome<crate::typeinfo::surface::TypeInfoSurface> {
        use crate::semantic_query::{
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
        // response. `ctx.dispatch()` keeps the surface inside the single engine.
        let dispatch = self.ctx.dispatch();
        // Resolve the base declaration CARRIER through the shared engine — the
        // SAME `ResolveDecl` a named-declaration shallow surface resolves
        // through — then project the path-precise selector. The path walker runs
        // intermediate hops in `Navigate` and the terminal hop under the
        // one-level `Shallow` surface synthesiser. NO second resolver.
        let decl = match dispatch.execute_type_node(SemanticQueryKey::ResolveDecl(ResolveDeclKey {
            scope: ScopeId {
                canonical_id: Arc::from(base.owner_canonical.as_ref()),
                local_scope: None,
            },
            name: Arc::from(base.symbol_name.as_ref()),
        })) {
            QueryResult::Value(SemanticQueryOutput { value: node, .. }) => node,
            QueryResult::Recursive(node) => node,
            QueryResult::Error(_) => return ResolvedOutcome::Missing,
        };
        match self.host.project_shallow_surface_from_base(
            self.ctx,
            &dispatch,
            decl,
            Arc::from(path.to_vec().into_boxed_slice()),
            ProjectionReductionContext::published(ProjectionMode::Shallow),
        ) {
            Some(surface) => ResolvedOutcome::Resolved(surface),
            None => ResolvedOutcome::Missing,
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

/// The canonical id the audit record uses, best-effort from the envelope's
/// framework selector. Returns an empty string when the envelope carries no
/// framework selector (a malformed envelope).
fn framework_selector_canonical(envelope: &TypeInfoGraphRequest) -> String {
    match &envelope.payload {
        Some(verter_protocol::verter::v1::type_info_graph_request::Payload::FrameworkSurface(
            r,
        )) => r
            .selector
            .as_ref()
            .map(|s| s.canonical_id.clone())
            .unwrap_or_default(),
        _ => String::new(),
    }
}

/// Build the audit payload from the encoded per-kind entries.
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

/// Classify the executor's response into the audited `Ok`/`Err` outcome.
///
/// The `error` arm is the typed wire error (validation / unknown adapter); the
/// `framework_surface` arm is success.
fn framework_response_outcome(
    response: &TypeInfoGraphResponse,
) -> Result<(), TypeInfoRequestError> {
    match &response.kind {
        Some(type_info_graph_response::Kind::Error(error)) => Err(error.clone()),
        _ => Ok(()),
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

/// Assemble the audited carrier from the response + outcome + record.
fn audited_from_outcome(
    response: TypeInfoGraphResponse,
    outcome: Result<(), TypeInfoRequestError>,
    record: RequestAuditRecord,
) -> AuditedResult<TypeInfoGraphResponse, TypeInfoRequestError> {
    match outcome {
        Ok(()) => AuditedResult::ok(response, record),
        Err(error) => AuditedResult::err(error, record),
    }
}

/// Build the noop audit record for a filtered / disabled request.
fn noop_framework_record(
    request_id: u64,
    canonical_id: &str,
    parent_request_id: Option<u64>,
    trace_id: String,
    state: verter_audit::AuditCaptureState,
    payload: TypeInfoGraphPayload,
) -> RequestAuditRecord {
    RequestAuditRecord {
        request_id,
        canonical_id: canonical_id.to_string(),
        kind: RequestKind::TypeInfoGraph,
        parent_request_id: parent_request_id.map(|id| id.to_string()),
        from_cache: false,
        timings: RequestTimingAudit::default(),
        memory: RequestMemoryAudit::default(),
        store: RequestStoreAudit::default(),
        footprint: None,
        scheduler: None,
        files: Vec::new(),
        waits: None,
        kind_payload: RequestKindPayload::TypeInfoGraph(payload),
        capture_state: state,
        trace_id,
    }
}
