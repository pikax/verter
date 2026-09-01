//! `VerterHost::compile_with_audit` — single-file audited compile entry-point.
//!
//! VUE-ONLY BY CONSTRUCTION: this entry names the Vue SFC producer itself
//! instead of consuming a request-scoped bound host identity that would elect
//! one, so the Vue carrier is its whole supported surface. It compiles the
//! Vue artifact elected by the host's registered publication store, and on any
//! other registered framework carrier (a `.svelte` file) it fails closed with
//! a typed `VerterE001` diagnostic rather than silently Vue-compiling it.
//!
//! The framework-neutral answer is the host-backed compile route, which
//! selects its producer from a request-scoped host binding's catalog arm — NOT
//! an outer carrier-registry bundle call, which reintroduces exactly the
//! registry-selected dispatch the bound route forbids.
//!
//! Wraps one registered-artifact compile call in the same
//! audit-registration / TLS-observer machinery the component-meta entry-point
//! uses. The producer crate (`verter_compiler`) emits `record_phase_timing`
//! at phase boundaries (parse / transform / codegen / css_analysis /
//! sourcemap) and `record_event(CompileCodeTransformOp)` at every
//! `CodeTransform` operation entry; the session-side `RequestContext`
//! aggregates these signals into per-request atomics. This entry-point
//! reads the atomics, assembles a [`verter_audit::CompilePayload`], and
//! finalises through the registration so consumers via
//! `take_audit_record(request_id)` work uniformly with the
//! component-meta path.
//!
//! Returns an [`verter_audit::AuditedResult<VerterCompileResult,
//! Infallible>`] carrier. Compile has no request-fault path —
//! diagnostics live in `VerterCompileResult.errors` — so the outcome is
//! always `Ok`. The carrier's `audit` field is mandatory: the
//! full-capture path returns an
//! [`verter_audit::AuditCaptureState::ActiveStored`] record, while the
//! filtered / disabled / file-not-found paths return the cheap
//! default-filled record marked
//! [`verter_audit::AuditCaptureState::FilteredNoop`] /
//! [`verter_audit::AuditCaptureState::AuditDisabled`].
//!
//! Bound-route attribution: both bound compile lanes — the host-backed
//! multi-product route (`compile_entry`) and the render-only lane
//! (`compile_entry_runtime_render`) — carry their structured attribution
//! on the request-scoped bound identity
//! ([`crate::host_resolve::native_host_binding::NativeHostRequestAttribution`]:
//! the registered catalog row plus the bound snapshot), checked against
//! the executed artifact at each lane's per-arm consumption points via
//! [`debug_assert_compile_bound_attribution`] so the attribution can
//! never disagree with the backend arm that actually executed.

use std::convert::Infallible;
use std::sync::atomic::Ordering;
use std::sync::Arc;

use crate::CompileTarget;
use oxc_allocator::Allocator;
use verter_audit::{
    payloads::tags::{CompileBackendTag, CompileProductSetTag},
    AuditedResult, CompilePayload, RequestAuditRecord, RequestKind, RequestKindPayload,
};
use verter_compiler::compile::types::VueExecutionInputs;
use verter_compiler::compile::{VerterCompileResult, VueMacroSemanticInput};
use verter_compiler::compile_request::{
    AnalysisProductRequest, CompileProduct, CompileRequest, DeclarationProductRequest,
    FrameworkCompileRequest, IdeProductRequest, ResolvedVueBackend, RuntimeProductRequest,
    VueBackendRequest, VueOptionAttempt,
};
use verter_compiler::framework_common::vue_bridge::{
    compile_registered_vue_artifact, resolve_vue_backend_for_audit,
};

/// The only options `compile_with_audit_options` callers have ever actually
/// overridden (verified by a workspace-wide read-site audit of the deleted
/// public `VerterCompileOptions`): a source-map on/off override and the two
/// Verter-internal composition knobs (`force_vapor`/`force_js`, absent from
/// the official Vue option inventory, same as their legacy fields).
/// Replaces `VerterCompileOptions` for this one call boundary — not a
/// second option authority: this crate's only entry into a canonical
/// `CompileRequest` for the audited compile route builds one from exactly
/// these fields plus the caller's `CompileTarget`, below.
#[derive(Debug, Clone, Copy, Default)]
pub struct CompileAuditOverrides {
    pub source_map: bool,
    pub force_vapor: bool,
    pub force_js: bool,
}

/// Rebuilds a canonical `CompileRequest` from the `CompileTarget` preset the
/// audited route still accepts. `CompileTarget` stays a public bitflags
/// type at this one boundary (a Rust-caller preset-selection convenience —
/// `parse_compile_target`/`parse_compile_target_wasm` gate the ONLY
/// externally-reachable path onto it to the closed
/// `"BUNDLER"`/`"IDE"`/`"ANALYSIS"`/`"META"`/`"TSX"`/`"TSC"` string set,
/// the NAPI/WASM public audit-target inventory both bindings actually
/// advertise), so this function is the enforcement point that keeps it
/// from being a silent admission bypass: `BUNDLER`/`IDE`/`TSC`/`ANALYSIS`/
/// `META` are the only preset VALUES any production or test caller
/// passes, but `TSX` is bit-identical to `IDE` and `META` is
/// bit-identical to `ANALYSIS` (both are `SCRIPT | TEMPLATE_DATA`), so
/// the match below names four distinct bit patterns — `META` is
/// accepted through the shared `ANALYSIS` pattern, not omitted (naming
/// it too would be an `unreachable_patterns` warning under
/// `-D warnings`) — and every other value, including an arbitrary bit
/// combination a Rust caller could otherwise construct directly, refuses
/// here rather than falling through to a best-effort product-set guess.
fn request_from_target(
    target: CompileTarget,
    overrides: CompileAuditOverrides,
) -> Result<CompileRequest, String> {
    // `ANALYSIS` and `META` are bit-identical (`SCRIPT | TEMPLATE_DATA`) —
    // matching both would be an `unreachable_patterns` warning under
    // `-D warnings`. `ANALYSIS` covers the shared bit pattern; `META` is
    // accepted by construction, not omitted.
    if !matches!(
        target,
        CompileTarget::BUNDLER | CompileTarget::IDE | CompileTarget::TSC | CompileTarget::ANALYSIS
    ) {
        return Err(format!(
            "compile_with_audit only accepts the BUNDLER/IDE/TSC/ANALYSIS/META target presets; \
             got an unsanctioned CompileTarget bit combination: {target:?}"
        ));
    }
    let mut products = Vec::new();
    if target.contains(CompileTarget::TEMPLATE) {
        products.push(CompileProduct::RuntimeClient(RuntimeProductRequest {
            runtime_source_map: overrides.source_map,
            ..Default::default()
        }));
    }
    if target.contains(CompileTarget::TSX) {
        products.push(CompileProduct::IdeCompanion(IdeProductRequest {
            want_source_map: overrides.source_map,
            ..Default::default()
        }));
    }
    if target.contains(CompileTarget::TSC) {
        products.push(CompileProduct::Declarations(
            DeclarationProductRequest::default(),
        ));
    }
    let want_script_bindings_only =
        target.contains(CompileTarget::SCRIPT) && !target.contains(CompileTarget::TEMPLATE);
    let want_template_data = target.contains(CompileTarget::TEMPLATE_DATA);
    if want_script_bindings_only || want_template_data {
        products.push(CompileProduct::Analysis(AnalysisProductRequest {
            want_script_bindings: want_script_bindings_only,
            want_template_data,
        }));
    }
    if products.is_empty() {
        products.push(CompileProduct::RuntimeClient(RuntimeProductRequest {
            runtime_source_map: overrides.source_map,
            ..Default::default()
        }));
    }

    let attempt = VueOptionAttempt {
        backend: if overrides.force_vapor {
            VueBackendRequest::Vapor
        } else {
            VueBackendRequest::Inferred
        },
        ..Default::default()
    };
    // Routes through the SAME option-admission surface the session's
    // bound host-backed demand reaches inside the framework host
    // backend's issuance (`VueOptionAttempt::into_request`), rather than
    // constructing `VueCompileRequest` directly:
    // `CompileAuditOverrides` has no field mapping onto any of the 12
    // unsupported-fail-closed slots today, so this can never actually
    // refuse here — but it is the structural admission gate, not a
    // reachability argument, and it is what keeps a future
    // `CompileAuditOverrides` field addition honest by construction.
    let vue = attempt
        .into_request()
        .map_err(|error| format!("{error:?}"))?;

    CompileRequest::new(
        products,
        FrameworkCompileRequest::Vue(vue),
        None,
        None,
        None,
        false,
        overrides.force_js,
    )
    .map_err(|error| format!("{error:?}"))
}

use crate::component_meta_audit::{RequestMemoryAudit, RequestStoreAudit, RequestTimingAudit};
use crate::instant::Instant;
use crate::request_context::{RequestContext, RequestContextGuard};
use crate::typeinfo::vue_macro_codegen::VueMacroCodegenDemand;
use crate::VerterHost;

/// The full requested product set, read directly off the `CompileRequest`
/// — every product the request carries, not a collapsed "primary path".
fn request_products_tag(request: &CompileRequest) -> CompileProductSetTag {
    let mut tag = CompileProductSetTag::default();
    for product in request.products() {
        match product {
            CompileProduct::RuntimeClient(_) => tag.runtime_client = true,
            CompileProduct::RuntimeServer(_) => tag.runtime_server = true,
            CompileProduct::IdeCompanion(_) => tag.ide_companion = true,
            CompileProduct::PublicApi(_) => tag.public_api = true,
            CompileProduct::Declarations(_) => tag.declarations = true,
            CompileProduct::Analysis(_) => tag.analysis = true,
        }
    }
    tag
}

/// The request's DECLARED backend intent, before any source has been
/// parsed — `Vdom`/`Vapor` when the caller pinned one explicitly,
/// `Inferred` when it is left to the source's own `<template vapor>`
/// marker. Used for the early-return audit branches (file-not-found,
/// non-Vue carrier, artifact-unavailable) that never reach a parsed
/// artifact; [`resolved_backend_tag`] supersedes this once one is
/// available.
fn declared_backend_tag(request: &CompileRequest) -> CompileBackendTag {
    match request.vue().map(|vue| vue.backend) {
        Some(VueBackendRequest::Vdom) => CompileBackendTag::Vdom,
        Some(VueBackendRequest::Vapor) => CompileBackendTag::Vapor,
        Some(VueBackendRequest::Inferred) | None => CompileBackendTag::Inferred,
    }
}

/// The REAL resolved backend once a parsed artifact is available —
/// accounts for the source's own implicit `<template vapor>` marker
/// rather than only the request's declared intent. Falls back to
/// [`declared_backend_tag`] when resolution is unavailable (a foreign
/// non-Vue artifact) or errors (unreachable for this Vue-only,
/// never-SSR audited path, but handled rather than unwrapped).
fn resolved_backend_tag(
    artifact: &verter_compiler::framework_common::FrameworkParseArtifact,
    request: &CompileRequest,
) -> CompileBackendTag {
    match resolve_vue_backend_for_audit(artifact, request) {
        Some(Ok(ResolvedVueBackend::Vdom)) => CompileBackendTag::Vdom,
        Some(Ok(ResolvedVueBackend::Vapor)) => CompileBackendTag::Vapor,
        Some(Err(_)) | None => declared_backend_tag(request),
    }
}

impl VerterHost {
    /// Compile a single canonical SFC with full audit capture.
    ///
    /// Looks up the source through the workspace and compiles the exact
    /// scheduler-owned registered artifact with `target` driving the codegen
    /// flags. Returns the typed result plus an
    /// [`RequestAuditRecord`] when capture is enabled. The record
    /// carries a `RequestKind::Compile { target: <tag> }` discriminant
    /// and a [`CompilePayload`] populated with per-phase timings and
    /// codegen counts.
    ///
    /// The compile path runs whether or not audit is enabled —
    /// `audit_enabled = false` returns the cheap default-filled record
    /// marked [`verter_audit::AuditCaptureState::AuditDisabled`] without
    /// any payload-building cost.
    pub fn compile_with_audit(
        self: &Arc<Self>,
        canonical_id: &str,
        target: CompileTarget,
    ) -> AuditedResult<VerterCompileResult, Infallible> {
        self.compile_with_audit_options(
            canonical_id,
            target,
            CompileAuditOverrides {
                source_map: true,
                ..CompileAuditOverrides::default()
            },
        )
    }

    /// Variant of [`Self::compile_with_audit`] that lets the caller
    /// override [`CompileAuditOverrides`] (e.g. enable `force_vapor` or
    /// `force_js`). The default `compile_with_audit` enables
    /// `source_map: true` so the sourcemap-phase timing has work to
    /// observe; this entry-point trades convenience for explicit
    /// control.
    pub fn compile_with_audit_options(
        self: &Arc<Self>,
        canonical_id: &str,
        target: CompileTarget,
        overrides: CompileAuditOverrides,
    ) -> AuditedResult<VerterCompileResult, Infallible> {
        let request = match request_from_target(target, overrides) {
            Ok(request) => request,
            Err(error) => {
                // The `CompileTarget` preset itself is unsanctioned (or the
                // derived request otherwise fails admission) — the same
                // "always returns a record" contract every other refusal
                // branch in this function honors, never a panic.
                let mut empty = VerterCompileResult {
                    script: None,
                    template: None,
                    styles: Vec::new(),
                    custom_blocks: Vec::new(),
                    scope_id: String::new(),
                    errors: Vec::new(),
                    parse_duration_ms: 0.0,
                    total_duration_ms: 0.0,
                    tsx: None,
                    tsc: None,
                    template_data: None,
                    template_data_diagnostics: Vec::new(),
                    template_binding_metadata: Default::default(),
                    inline: false,
                    requested_mode: verter_audit::payloads::tags::CompileCacheModeTag::Session,
                    actual_mode: verter_audit::payloads::tags::CompileCacheModeTag::Session,
                    downgrade_reason: None,
                };
                empty
                    .errors
                    .push(verter_compiler::compile::CompileDiagnostic {
                        severity: verter_compiler::compile::CompileDiagnosticSeverity::Error,
                        code: "VerterE004".to_string(),
                        message: format!(
                            "compile request construction refused for '{canonical_id}': {error}"
                        ),
                        span: None,
                    });
                let request_id = self.next_request_id();
                let state = if self.config.audit_enabled {
                    verter_audit::AuditCaptureState::FilteredNoop
                } else {
                    verter_audit::AuditCaptureState::AuditDisabled
                };
                let parent_request_id = verter_scheduler::request_context::current_request_id()
                    .map(|id| id.to_string());
                let record = noop_compile_record(
                    request_id,
                    canonical_id,
                    parent_request_id,
                    CompileProductSetTag::default(),
                    CompileBackendTag::default(),
                    state,
                );
                return AuditedResult::ok(empty, record);
            }
        };
        let request_products = request_products_tag(&request);
        // The declared-intent backend — the only one available before a
        // parsed artifact exists. Upgraded to the REAL resolved backend
        // (accounting for the source's own `<template vapor>` marker) once
        // `framework_parse` is in scope, below.
        let request_backend = declared_backend_tag(&request);
        // 1. Read source through workspace. On miss, return an empty
        //    result whose only diagnostic is the not-found error. The
        //    carrier still carries a cheap default-filled record so the
        //    always-a-record contract holds.
        let source_arc = match self.workspace().read_file(canonical_id) {
            Some(s) => s,
            None => {
                let mut empty = VerterCompileResult {
                    script: None,
                    template: None,
                    styles: Vec::new(),
                    custom_blocks: Vec::new(),
                    scope_id: String::new(),
                    errors: Vec::new(),
                    parse_duration_ms: 0.0,
                    total_duration_ms: 0.0,
                    tsx: None,
                    tsc: None,
                    template_data: None,
                    template_data_diagnostics: Vec::new(),
                    template_binding_metadata: Default::default(),
                    inline: false,
                    requested_mode: verter_audit::payloads::tags::CompileCacheModeTag::Session,
                    actual_mode: verter_audit::payloads::tags::CompileCacheModeTag::Session,
                    downgrade_reason: None,
                };
                empty
                    .errors
                    .push(verter_compiler::compile::CompileDiagnostic {
                        severity: verter_compiler::compile::CompileDiagnosticSeverity::Error,
                        code: "VerterE000".to_string(),
                        message: format!("file not found in workspace: {canonical_id}"),
                        span: None,
                    });
                let request_id = self.next_request_id();
                let state = if self.config.audit_enabled {
                    verter_audit::AuditCaptureState::FilteredNoop
                } else {
                    verter_audit::AuditCaptureState::AuditDisabled
                };
                let parent_request_id = verter_scheduler::request_context::current_request_id()
                    .map(|id| id.to_string());
                let record = noop_compile_record(
                    request_id,
                    canonical_id,
                    parent_request_id,
                    request_products,
                    request_backend,
                    state,
                );
                return AuditedResult::ok(empty, record);
            }
        };
        let source: &str = source_arc.as_ref();

        // Vue-only guard. Every arm below drives
        // `compile_registered_vue_artifact` — the Vue SFC producer, named by
        // THIS function rather than selected from the request's registered
        // framework identity. Driving a NON-Vue framework carrier (a
        // `.svelte` file) through it would silently produce WRONG output
        // (Vue-compiling a Svelte component), so it FAILS CLOSED on a non-Vue
        // carrier with a typed diagnostic instead of emitting wrong bytes.
        //
        // The framework-neutral answer is the host-backed compile route,
        // which selects its producer from a request-scoped host binding's
        // catalog arm; this audited spelling consumes no binding, so its
        // supported surface is the Vue carrier alone.
        let language = self.language_classifier().classify(canonical_id);
        if language.is_framework_carrier() && !language.is_vue() {
            let mut unsupported = VerterCompileResult {
                script: None,
                template: None,
                styles: Vec::new(),
                custom_blocks: Vec::new(),
                scope_id: String::new(),
                errors: Vec::new(),
                parse_duration_ms: 0.0,
                total_duration_ms: 0.0,
                tsx: None,
                tsc: None,
                template_data: None,
                template_data_diagnostics: Vec::new(),
                template_binding_metadata: Default::default(),
                inline: false,
                requested_mode: verter_audit::payloads::tags::CompileCacheModeTag::Session,
                actual_mode: verter_audit::payloads::tags::CompileCacheModeTag::Session,
                downgrade_reason: None,
            };
            unsupported
                .errors
                .push(verter_compiler::compile::CompileDiagnostic {
                    severity: verter_compiler::compile::CompileDiagnosticSeverity::Error,
                    code: "VerterE001".to_string(),
                    message: format!(
                        "compile_with_audit is the Vue-only audited compile path; the non-Vue \
                         framework carrier '{canonical_id}' is not supported here (compile it \
                         through the host-backed compile routes)"
                    ),
                    span: None,
                });
            let request_id = self.next_request_id();
            let state = if self.config.audit_enabled {
                verter_audit::AuditCaptureState::FilteredNoop
            } else {
                verter_audit::AuditCaptureState::AuditDisabled
            };
            let parent_request_id =
                verter_scheduler::request_context::current_request_id().map(|id| id.to_string());
            let record = noop_compile_record(
                request_id,
                canonical_id,
                parent_request_id,
                request_products,
                request_backend,
                state,
            );
            return AuditedResult::ok(unsupported, record);
        }

        let framework_parse = self.ensure_loaded(canonical_id).then(|| {
            self.scheduler
                .try_get_source(canonical_id)
                .filter(|snapshot| snapshot.source.as_ref() == source)
                .and_then(|snapshot| {
                    snapshot
                        .downcast_data::<crate::host_executor::HostSourceData>()
                        .and_then(|data| data.framework_parse.as_ref().map(Arc::clone))
                })
        });
        let Some(framework_parse) = framework_parse.flatten() else {
            let rejected = registered_compile_rejected(
                "VerterE002",
                format!(
                    "registered source artifact unavailable for audited compile: {canonical_id}"
                ),
            );
            let request_id = self.next_request_id();
            let state = if self.config.audit_enabled {
                verter_audit::AuditCaptureState::FilteredNoop
            } else {
                verter_audit::AuditCaptureState::AuditDisabled
            };
            let parent_request_id =
                verter_scheduler::request_context::current_request_id().map(|id| id.to_string());
            return AuditedResult::ok(
                rejected,
                noop_compile_record(
                    request_id,
                    canonical_id,
                    parent_request_id,
                    request_products,
                    request_backend,
                    state,
                ),
            );
        };

        // The REAL resolved backend, now that a parsed artifact is
        // available — accounts for the source's own implicit `<template
        // vapor>` marker, superseding `request_backend`'s pre-parse
        // declared-intent value for every branch below.
        let resolved_backend = resolved_backend_tag(&framework_parse, &request);

        let allocator = Allocator::new();
        let execution_inputs = VueExecutionInputs::default();

        // 2. Audit-disabled fast path: drive the producer with NO
        //    `RequestContextGuard` installed. Producer-side
        //    `current_observer()` returns `None`, the instrumentation
        //    short-circuits at the TLS check, and nothing is published.
        //    The carrier still carries a cheap default-filled record
        //    marked `AuditDisabled`.
        if !self.config.audit_enabled {
            let macro_semantics = self.vue_macro_compile_input(canonical_id, target);
            let result = compile_registered_vue_artifact(
                source,
                &framework_parse,
                &request,
                &execution_inputs,
                &macro_semantics,
                &allocator,
            )
            .unwrap_or_else(|_| {
                registered_compile_rejected(
                    "VerterE003",
                    "registered Vue artifact rejected by its adapter".to_string(),
                )
            });
            let request_id = self.next_request_id();
            let parent_request_id =
                verter_scheduler::request_context::current_request_id().map(|id| id.to_string());
            let record = noop_compile_record(
                request_id,
                canonical_id,
                parent_request_id,
                request_products,
                resolved_backend,
                verter_audit::AuditCaptureState::AuditDisabled,
            );
            return AuditedResult::ok(result, record);
        }

        // 3. Stamp request id and increment created-counter so the
        //    `AuditedRequest` harness's multi-request guard surfaces
        //    correctly when a closure issues both a component-meta
        //    and compile call.
        let request_id = self.next_request_id();
        crate::request_context::increment_requests_created();

        // 4. Construct the request context with the Compile kind.
        let footprint_capture = self.config.footprint_capture;
        let timing_capture = self.config.audit_timing_capture;
        let ctx = RequestContext::with_kind_and_timing(
            request_id,
            Arc::<str>::from(canonical_id),
            RequestKind::Compile {
                products: request_products,
                backend: resolved_backend,
            },
            footprint_capture,
            timing_capture,
            None,
        );

        // 5. Build the audit registration. `Active` adds the request
        //    to the registry; `Noop` short-circuits when the consumer
        //    filter rejects the kind.
        let registration = Arc::new(crate::host_audit_runtime::AuditRequestRegistration::new(
            self,
            Arc::clone(&ctx),
        ));
        let _ = ctx.install_audit_registration(Arc::clone(&registration));

        // 6. Capture parent correlation off the RequestContext (sniffed
        //    from the scheduler TLS at construction). A compile issued
        //    inside another audited request's window inherits that
        //    request's id as its `parent_request_id`.
        let parent_request_id = ctx.parent_request_id.map(|id| id.to_string());

        // 7. Branch Active / Noop BEFORE assembling any audit payload.
        //    The compile RESULT computes on both arms (consumers asked
        //    for it), but the Noop arm installs the cheap no-op observer
        //    and skips all heavy payload-assembly work — matching the
        //    analyze / resolve_type / typeinfo entry-points. This avoids
        //    installing the real `RequestContextGuard` and assembling a
        //    full `CompilePayload` only to discard it on a
        //    consumer-filtered request.
        if matches!(
            registration.as_ref(),
            crate::host_audit_runtime::AuditRequestRegistration::Noop
        ) {
            let _noop_guard = verter_audit::install_noop_observer();
            let macro_semantics = self.vue_macro_compile_input(canonical_id, target);
            let result = compile_registered_vue_artifact(
                source,
                &framework_parse,
                &request,
                &execution_inputs,
                &macro_semantics,
                &allocator,
            )
            .unwrap_or_else(|_| {
                registered_compile_rejected(
                    "VerterE003",
                    "registered Vue artifact rejected by its adapter".to_string(),
                )
            });
            let record = noop_compile_record(
                request_id,
                canonical_id,
                parent_request_id,
                request_products,
                resolved_backend,
                verter_audit::AuditCaptureState::FilteredNoop,
            );
            return AuditedResult::ok(result, record);
        }

        // 8. Active arm: install the TLS guard so producers in
        //    `verter_compiler` see `current_observer() = Some(ctx)`.
        let _ctx_guard = RequestContextGuard::install(Arc::clone(&ctx));

        // 9. Drive the compile. Producers emit `record_phase_timing`
        //    + `record_event(CompileCodeTransformOp)` while this
        //    block runs.
        let total_start = Instant::now();
        let macro_semantics = self.vue_macro_compile_input(canonical_id, target);
        let result = compile_registered_vue_artifact(
            source,
            &framework_parse,
            &request,
            &execution_inputs,
            &macro_semantics,
            &allocator,
        )
        .unwrap_or_else(|_| {
            registered_compile_rejected(
                "VerterE003",
                "registered Vue artifact rejected by its adapter".to_string(),
            )
        });
        let total_ms = total_start.elapsed().as_secs_f64() * 1000.0;

        // 10. Read accumulators off the active request context.
        let payload = self.assemble_compile_payload(request_products, resolved_backend, &result);
        let store = RequestStoreAudit::default();
        let memory = RequestMemoryAudit::default();
        let timings = RequestTimingAudit {
            total_ms,
            ..RequestTimingAudit::default()
        };

        let record = RequestAuditRecord {
            request_id,
            canonical_id: canonical_id.to_string(),
            target_identity: Some(verter_audit::RequestTargetIdentity::registered(
                canonical_id,
            )),
            kind: RequestKind::Compile {
                products: request_products,
                backend: resolved_backend,
            },
            parent_request_id,
            from_cache: false,
            timings,
            memory,
            store,
            footprint: None,
            scheduler: None,
            files: Vec::new(),
            waits: None,
            kind_payload: RequestKindPayload::Compile(payload),
            capture_state: verter_audit::AuditCaptureState::ActiveStored,
            trace_id: String::new(),
        };

        // 11. Finalise the record. Drop the TLS guard AFTER finalize so
        //     the per-request counters stay coherent with the record we
        //     publish.
        registration.finalize(record.clone());
        drop(_ctx_guard);
        AuditedResult::ok(result, record)
    }

    fn vue_macro_compile_input(
        &self,
        canonical_id: &str,
        target: CompileTarget,
    ) -> VueMacroSemanticInput {
        VueMacroCodegenDemand::for_compile_target(target)
            .map(|demand| {
                self.produce_vue_macro_codegen(canonical_id, demand)
                    .compiler_input()
            })
            .unwrap_or(VueMacroSemanticInput::Unavailable)
    }

    fn assemble_compile_payload(
        &self,
        products: CompileProductSetTag,
        backend: CompileBackendTag,
        result: &VerterCompileResult,
    ) -> CompilePayload {
        // Read the per-request accumulators off the TLS context that
        // is still installed at the call site of
        // `compile_with_audit_options`. The `RequestContextGuard`
        // outlives this call.
        let (parse_us, transform_us, codegen_us, css_us, sourcemap_us, ct_ops) =
            match crate::request_context::current_request_context() {
                Some(ctx) => (
                    ctx.compile_parse_us.load(Ordering::Relaxed),
                    ctx.compile_transform_us.load(Ordering::Relaxed),
                    ctx.compile_codegen_us.load(Ordering::Relaxed),
                    ctx.compile_css_analysis_us.load(Ordering::Relaxed),
                    ctx.compile_sourcemap_us.load(Ordering::Relaxed),
                    ctx.compile_code_transform_ops.load(Ordering::Relaxed),
                ),
                None => (0, 0, 0, 0, 0, 0),
            };
        let to_ms = |us: u64| -> Option<f64> {
            if us == 0 {
                None
            } else {
                Some(us as f64 / 1_000.0)
            }
        };

        // Output bytes: sum every present block's code length. The
        // consumer filter doesn't get to see these bytes; they're
        // strictly observability for "how big was the codegen output".
        let mut output_bytes: u64 = 0;
        let mut sourcemap_bytes: u64 = 0;
        let mut num_script_blocks: u32 = 0;
        if let Some(s) = result.script.as_ref() {
            output_bytes = output_bytes.saturating_add(s.code.len() as u64);
            sourcemap_bytes = sourcemap_bytes.saturating_add(s.source_map.len() as u64);
            num_script_blocks = num_script_blocks.saturating_add(1);
        }
        if let Some(t) = result.template.as_ref() {
            output_bytes = output_bytes.saturating_add(t.code.len() as u64);
            sourcemap_bytes = sourcemap_bytes.saturating_add(t.source_map.len() as u64);
        }
        for style in result.styles.iter() {
            output_bytes = output_bytes.saturating_add(style.code.len() as u64);
        }
        if let Some(tsx) = result.tsx.as_ref() {
            output_bytes = output_bytes.saturating_add(tsx.code.len() as u64);
            sourcemap_bytes = sourcemap_bytes.saturating_add(tsx.source_map.len() as u64);
        }
        if let Some(tsc) = result.tsc.as_ref() {
            output_bytes = output_bytes.saturating_add(tsc.code.len() as u64);
            sourcemap_bytes = sourcemap_bytes.saturating_add(tsc.source_map.len() as u64);
        }

        // num_directives / num_components: extracted from
        // template_data when available. The non-data path leaves them
        // at 0 — this keeps the producer cost minimal for the
        // bundler-default code path that does not need analysis.
        // `template_data.directives` is not a single field on the raw
        // form — directive observations split across `comment_directives`,
        // `v_for_directives`, `v_model_directives`, plus event handlers
        // (which are conceptually directive-shaped). The audit count is
        // the sum of the structural directive arrays.
        let (num_directives, num_components) = result
            .template_data
            .as_ref()
            .map(|td| {
                let directives = (td.comment_directives.len()
                    + td.v_for_directives.len()
                    + td.v_model_directives.len()) as u32;
                let components = td.components.len() as u32;
                (directives, components)
            })
            .unwrap_or((0, 0));

        CompilePayload {
            products,
            backend,
            parse_ms: to_ms(parse_us),
            transform_ms: to_ms(transform_us),
            codegen_ms: to_ms(codegen_us),
            css_analysis_ms: to_ms(css_us),
            sourcemap_ms: to_ms(sourcemap_us),
            output_bytes,
            sourcemap_bytes,
            num_directives,
            num_components,
            num_style_blocks: result.styles.len() as u32,
            num_script_blocks,
            code_transform_ops: ct_ops as u32,
        }
    }
}

/// The bound compile lane a [`debug_assert_compile_bound_attribution`]
/// check attributes — names the route in the assertion diagnostics so a
/// tripped invariant identifies which lane paired a foreign binding with
/// its compile input.
#[derive(Debug, Clone, Copy)]
pub(crate) enum BoundCompileRoute {
    /// The host-backed multi-product route: the profile-derived
    /// `compile_entry` and the caller-supplied-request seam, which share
    /// this bound execution.
    HostBacked,
    /// The render-only route (`compile_entry_runtime_render`).
    RuntimeRender,
}

impl BoundCompileRoute {
    fn as_str(self) -> &'static str {
        match self {
            Self::HostBacked => "host-backed",
            Self::RuntimeRender => "runtime-render",
        }
    }
}

/// Structured bound-topology attribution check: the registered catalog
/// row the request was bound to (adapter, carrier language, framework
/// epoch, host epoch) plus the bound snapshot identity must describe
/// EXACTLY the artifact the compile admission was issued and executed
/// over. Called at each bound lane's per-arm binding consumption points,
/// immediately before issuance, so the attribution a consumer would
/// report can never disagree with the executed bound topology.
/// Debug-build invariant: a mismatch is a session wiring defect (the
/// binding and the compile input were read from different request
/// contexts), not a user-input outcome.
pub(crate) fn debug_assert_compile_bound_attribution(
    route: BoundCompileRoute,
    attribution: &crate::host_resolve::native_host_binding::NativeHostRequestAttribution,
    artifact: &verter_compiler::framework_common::FrameworkParseArtifact,
    canonical_id: &str,
) {
    let route = route.as_str();
    let identity = attribution.catalog_identity();
    verter_debug_assert_eq!(
        identity.adapter_id(),
        artifact.adapter_id(),
        "{route} bound attribution must name the executed artifact's adapter"
    );
    verter_debug_assert_eq!(
        identity.carrier_language_id(),
        artifact.language_id(),
        "{route} bound attribution must name the executed artifact's carrier language"
    );
    verter_debug_assert_eq!(
        identity.epoch(),
        artifact.epoch(),
        "{route} bound attribution must name the executed artifact's framework epoch"
    );
    verter_debug_assert!(
        identity.host_epoch().is_some(),
        "a host-integration catalog row always carries a host epoch"
    );
    verter_debug_assert_eq!(
        attribution.snapshot().canonical_id(),
        canonical_id,
        "{route} bound attribution must name the executed request's canonical id"
    );
}

fn registered_compile_rejected(code: &str, message: String) -> VerterCompileResult {
    let mut result = VerterCompileResult {
        script: None,
        template: None,
        styles: Vec::new(),
        custom_blocks: Vec::new(),
        scope_id: String::new(),
        errors: Vec::new(),
        parse_duration_ms: 0.0,
        total_duration_ms: 0.0,
        tsx: None,
        tsc: None,
        template_data: None,
        template_data_diagnostics: Vec::new(),
        template_binding_metadata: Default::default(),
        inline: false,
        requested_mode: verter_audit::payloads::tags::CompileCacheModeTag::Session,
        actual_mode: verter_audit::payloads::tags::CompileCacheModeTag::Session,
        downgrade_reason: None,
    };
    result
        .errors
        .push(verter_compiler::compile::CompileDiagnostic {
            severity: verter_compiler::compile::CompileDiagnosticSeverity::Error,
            code: code.to_string(),
            message,
            span: None,
        });
    result
}

/// Build the cheap default-filled [`RequestAuditRecord`] returned on
/// the filtered / disabled / file-not-found compile path. No
/// per-request counters are collected — the payload is the zero-valued
/// default carrying only the resolved product-set/backend tags, and
/// `capture_state` records why the full path was skipped.
fn noop_compile_record(
    request_id: u64,
    canonical_id: &str,
    parent_request_id: Option<String>,
    products: CompileProductSetTag,
    backend: CompileBackendTag,
    capture_state: verter_audit::AuditCaptureState,
) -> RequestAuditRecord {
    RequestAuditRecord {
        request_id,
        canonical_id: canonical_id.to_string(),
        target_identity: Some(verter_audit::RequestTargetIdentity::registered(
            canonical_id,
        )),
        kind: RequestKind::Compile { products, backend },
        parent_request_id,
        from_cache: false,
        timings: RequestTimingAudit::default(),
        memory: RequestMemoryAudit::default(),
        store: RequestStoreAudit::default(),
        footprint: None,
        scheduler: None,
        files: Vec::new(),
        waits: None,
        kind_payload: RequestKindPayload::Compile(CompilePayload {
            products,
            backend,
            ..CompilePayload::default()
        }),
        capture_state,
        trace_id: String::new(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use verter_compiler::compile_request::VueCompileRequest;

    /// `request_products_tag`'s multi-product accuracy: a request carrying
    /// TWO distinct products (`RuntimeClient` + `IdeCompanion`) must set
    /// BOTH corresponding tag fields, not collapse to whichever one a
    /// lossy "primary path" mirror would have picked (the exact bug class
    /// `CompileTargetTag` was replaced for). Built directly against a
    /// hand-constructed multi-product `CompileRequest` rather than through
    /// `compile_with_audit`: every preset `request_from_target` accepts
    /// (`BUNDLER`/`IDE`/`TSC`/`ANALYSIS`/`META`) produces exactly one
    /// product, so a multi-product request is not reachable there BY
    /// DESIGN — this tests the tag-building function's own correctness
    /// directly, independent of which caller (if any, in the future)
    /// constructs a multi-product request.
    #[test]
    fn request_products_tag_carries_every_product_a_multi_product_request_carries() {
        let request = CompileRequest::new(
            vec![
                CompileProduct::RuntimeClient(RuntimeProductRequest::default()),
                CompileProduct::IdeCompanion(IdeProductRequest::default()),
                CompileProduct::Analysis(AnalysisProductRequest {
                    want_script_bindings: false,
                    want_template_data: true,
                }),
            ],
            FrameworkCompileRequest::Vue(VueCompileRequest::default()),
            None,
            None,
            None,
            false,
            false,
        )
        .expect("RuntimeClient + IdeCompanion + Analysis constructs together");

        let tag = request_products_tag(&request);
        assert!(tag.runtime_client, "RuntimeClient must survive in the tag");
        assert!(tag.ide_companion, "IdeCompanion must survive in the tag");
        assert!(tag.analysis, "Analysis must survive in the tag");
        assert!(
            !tag.runtime_server && !tag.public_api && !tag.declarations,
            "products NOT on the request must stay false, got: {tag:?}"
        );
    }

    /// Negative control: a lone `IdeCompanion` request must set ONLY that
    /// field — discriminates against a regression that always sets every
    /// field regardless of the actual product set.
    #[test]
    fn request_products_tag_is_precise_for_a_single_product_request() {
        let request = CompileRequest::new(
            vec![CompileProduct::IdeCompanion(IdeProductRequest::default())],
            FrameworkCompileRequest::Vue(VueCompileRequest::default()),
            None,
            None,
            None,
            false,
            false,
        )
        .expect("a lone IdeCompanion product constructs");

        let tag = request_products_tag(&request);
        assert_eq!(
            tag,
            CompileProductSetTag {
                ide_companion: true,
                ..Default::default()
            }
        );
    }

    /// `ANALYSIS` (`SCRIPT | TEMPLATE_DATA`, no `TEMPLATE`) must construct
    /// a single `Analysis` product requesting BOTH script bindings and
    /// template data — the exact preset NAPI's `compileWithAudit` and
    /// WASM's equivalent still publicly advertise and parse
    /// (`parse_compile_target`/`parse_compile_target_wasm`). Regression
    /// test: `request_from_target` accepting only BUNDLER/IDE/TSC would
    /// silently break this previously-working preset.
    #[test]
    fn analysis_target_constructs_a_script_and_template_data_analysis_product() {
        let request =
            request_from_target(CompileTarget::ANALYSIS, CompileAuditOverrides::default())
                .expect("ANALYSIS must be an accepted compile_with_audit target preset");

        assert_eq!(request.products().len(), 1, "{:?}", request.products());
        match &request.products()[0] {
            CompileProduct::Analysis(analysis) => {
                assert!(analysis.want_script_bindings, "{analysis:?}");
                assert!(analysis.want_template_data, "{analysis:?}");
            }
            other => panic!("expected a lone Analysis product, got: {other:?}"),
        }
    }

    /// `META` is bit-identical to `ANALYSIS` (`SCRIPT | TEMPLATE_DATA`)
    /// and must construct the identical product — proves `META` is
    /// accepted through the shared `ANALYSIS` match arm, not silently
    /// dropped by the `unreachable_patterns`-avoiding match shape.
    #[test]
    fn meta_target_constructs_the_same_analysis_product_as_analysis() {
        let analysis_request =
            request_from_target(CompileTarget::ANALYSIS, CompileAuditOverrides::default())
                .expect("ANALYSIS must construct");
        let meta_request =
            request_from_target(CompileTarget::META, CompileAuditOverrides::default())
                .expect("META must be an accepted compile_with_audit target preset");

        assert_eq!(
            request_products_tag(&analysis_request),
            request_products_tag(&meta_request),
        );
        match (&analysis_request.products()[0], &meta_request.products()[0]) {
            (CompileProduct::Analysis(a), CompileProduct::Analysis(m)) => {
                assert_eq!(a.want_script_bindings, m.want_script_bindings);
                assert_eq!(a.want_template_data, m.want_template_data);
            }
            other => panic!("expected both to construct a lone Analysis product: {other:?}"),
        }
    }
}
