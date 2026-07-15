//! `VerterHost::compile_with_audit` — single-file audited compile entry-point.
//!
//! VUE-ONLY: this entry drives the hardcoded Vue SFC runtime compiler
//! ([`verter_compiler::compile::compile`]) directly — it is NOT the
//! framework-neutral carrier path. It fails closed on a non-Vue framework
//! carrier (a `.svelte` file) with a typed `VerterE001` diagnostic rather than
//! silently Vue-compiling it. Routing this audited path through the carrier
//! registry so it compiles every registered carrier is a tracked follow-up
//! (docs/arch/svelte-native-compiler-plan.md §11).
//!
//! Wraps one [`verter_compiler::compile::compile`] call in the same
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

use std::convert::Infallible;
use std::sync::atomic::Ordering;
use std::sync::Arc;

use oxc_allocator::Allocator;
use verter_audit::{
    payloads::tags::CompileTargetTag, AuditedResult, CompilePayload, RequestAuditRecord,
    RequestKind, RequestKindPayload,
};
use verter_compiler::compile::{
    compile as compile_sfc, CodegenOptions, CompileTarget, VerterCompileOptions,
    VerterCompileResult,
};

use crate::component_meta_audit::{RequestMemoryAudit, RequestStoreAudit, RequestTimingAudit};
use crate::instant::Instant;
use crate::request_context::{RequestContext, RequestContextGuard};
use crate::VerterHost;

/// Map a `CompileTarget` bitset to the audit's stringly tag. The tag
/// reflects the *primary* codegen path the request is exercising:
/// - `TSX` set → `Ide` (LSP / tsgo path).
/// - else if `TEMPLATE` set → `Vdom` (the bundler / runtime path).
/// - otherwise → `Vdom` (no template-codegen target — the tag is still
///   the closest descriptor of what producers will emit).
///
/// Vapor-mode detection requires parsed SFC state; the call-site does
/// not have that. Callers that need explicit Vapor attribution should
/// pass [`compile_with_audit_options`] with `force_vapor = true` and
/// the entry-point will tag the kind as `Vapor` regardless of the bit
/// presence.
fn target_to_tag(target: CompileTarget, force_vapor: bool) -> CompileTargetTag {
    if force_vapor {
        return CompileTargetTag::Vapor;
    }
    if target.contains(CompileTarget::TSX) {
        CompileTargetTag::Ide
    } else {
        CompileTargetTag::Vdom
    }
}

impl VerterHost {
    /// Compile a single canonical SFC with full audit capture.
    ///
    /// Looks up the source through the workspace, calls
    /// [`verter_compiler::compile::compile`] with `target` driving the
    /// codegen flags, and returns the typed result plus an
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
            VerterCompileOptions {
                source_map: true,
                ..VerterCompileOptions::default()
            },
        )
    }

    /// Variant of [`Self::compile_with_audit`] that lets the caller
    /// override `VerterCompileOptions` (e.g. enable `force_vapor` or
    /// `force_js`). The default `compile_with_audit` enables
    /// `source_map: true` so the sourcemap-phase timing has work to
    /// observe; this entry-point trades convenience for explicit
    /// control.
    pub fn compile_with_audit_options(
        self: &Arc<Self>,
        canonical_id: &str,
        target: CompileTarget,
        verter_options: VerterCompileOptions,
    ) -> AuditedResult<VerterCompileResult, Infallible> {
        let force_vapor = verter_options.force_vapor;
        let request_tag = target_to_tag(target, force_vapor);
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
                    request_tag,
                    state,
                );
                return AuditedResult::ok(empty, record);
            }
        };
        let source: &str = source_arc.as_ref();

        // Vue-only guard. `compile_sfc` (`verter_compiler::compile::compile`) is
        // the hardcoded Vue SFC runtime compiler — it is NOT the framework-
        // neutral carrier path (`compile_entry` → `CarrierCompilerRegistry::
        // compile_bundle`). Driving a NON-Vue framework carrier (a `.svelte`
        // file) through it would silently produce WRONG output (Vue-compiling a
        // Svelte component). This audited path stays Vue-only and FAILS CLOSED
        // on a non-Vue carrier with a clear typed diagnostic rather than
        // emitting wrong bytes.
        //
        // TODO(follow-up): route `compile_with_audit` through the carrier
        // registry (`compile_bundle`) so the audit path compiles every
        // registered carrier — see docs/arch/svelte-native-compiler-plan.md §11
        // (the audit/helper compile-caller carrier migration follow-up).
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
                         framework carrier '{canonical_id}' is not supported here (route it \
                         through the carrier registry)"
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
                request_tag,
                state,
            );
            return AuditedResult::ok(unsupported, record);
        }

        let codegen_options = CodegenOptions {
            target,
            ..CodegenOptions::default()
        };
        let allocator = Allocator::new();

        // 2. Audit-disabled fast path: drive the producer with NO
        //    `RequestContextGuard` installed. Producer-side
        //    `current_observer()` returns `None`, the instrumentation
        //    short-circuits at the TLS check, and nothing is published.
        //    The carrier still carries a cheap default-filled record
        //    marked `AuditDisabled`.
        if !self.config.audit_enabled {
            let result = compile_sfc(source, &codegen_options, &verter_options, &allocator);
            let request_id = self.next_request_id();
            let parent_request_id =
                verter_scheduler::request_context::current_request_id().map(|id| id.to_string());
            let record = noop_compile_record(
                request_id,
                canonical_id,
                parent_request_id,
                request_tag,
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

        let tag = request_tag;

        // 4. Construct the request context with the Compile kind.
        let footprint_capture = self.config.footprint_capture;
        let timing_capture = self.config.audit_timing_capture;
        let ctx = RequestContext::with_kind_and_timing(
            request_id,
            Arc::<str>::from(canonical_id),
            RequestKind::Compile { target: tag },
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
            let result = compile_sfc(source, &codegen_options, &verter_options, &allocator);
            let record = noop_compile_record(
                request_id,
                canonical_id,
                parent_request_id,
                tag,
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
        let result = compile_sfc(source, &codegen_options, &verter_options, &allocator);
        let total_ms = total_start.elapsed().as_secs_f64() * 1000.0;

        // 10. Read accumulators off the active request context.
        let payload = self.assemble_compile_payload(tag, target, force_vapor, &result);
        let store = RequestStoreAudit::default();
        let memory = RequestMemoryAudit::default();
        let timings = RequestTimingAudit {
            total_ms,
            ..RequestTimingAudit::default()
        };

        let record = RequestAuditRecord {
            request_id,
            canonical_id: canonical_id.to_string(),
            kind: RequestKind::Compile { target: tag },
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

    fn assemble_compile_payload(
        &self,
        tag: CompileTargetTag,
        target: CompileTarget,
        force_vapor: bool,
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

        let _ = (target, force_vapor); // Silence unused warnings — reserved for future tag-derivation refinements.
        CompilePayload {
            target: tag,
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

/// Build the cheap default-filled [`RequestAuditRecord`] returned on
/// the filtered / disabled / file-not-found compile path. No
/// per-request counters are collected — the payload is the zero-valued
/// default carrying only the resolved `target` tag, and
/// `capture_state` records why the full path was skipped.
fn noop_compile_record(
    request_id: u64,
    canonical_id: &str,
    parent_request_id: Option<String>,
    target: CompileTargetTag,
    capture_state: verter_audit::AuditCaptureState,
) -> RequestAuditRecord {
    RequestAuditRecord {
        request_id,
        canonical_id: canonical_id.to_string(),
        kind: RequestKind::Compile { target },
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
            target,
            ..CompilePayload::default()
        }),
        capture_state,
        trace_id: String::new(),
    }
}
