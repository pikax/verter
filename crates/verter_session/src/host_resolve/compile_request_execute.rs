//! The caller-supplied canonical compile request: the session entry that
//! executes one, the lane behind it, and the virtual-node publication both
//! compile lanes share.
//!
//! The entry ([`VerterHost::compile_request`]) takes a canonical id plus a
//! canonical [`CompileRequest`] and returns the typed response in ONE
//! call. The supplied request is the demand document end to end: no
//! function on this route holds a `CompileProfile`, derives one, or reads
//! one — the product set, the framework options, the published node set
//! and the assembly axes all come from the request itself, the registered
//! immutable source snapshot supplies the bytes, and supplied (externally
//! preprocessed) block artifacts are read from the profile-less bucket
//! only — the one `apply_block_overrides` writes to when the caller names
//! no compile profile.
//!
//! [`publish_runtime_nodes`] is the ONE projection of an admitted runtime
//! bundle into virtual nodes. Both lanes drive it: the profile-derived one
//! states its [`RuntimeNodePublication`] from a profile's target bits,
//! this one states it from the request's product set, and neither has its
//! own copy of how a bundle becomes nodes.

use std::sync::Arc;

use rustc_hash::FxHashMap;

use oxc_allocator::Allocator;

use super::compile_request_build::{
    execute_supplied_host_request, no_carrier_artifact_diagnostics, HostProductsFailure,
    SuppliedRequestFailure,
};
use super::native_host_binding::BoundNativeHostRequest;
use super::virtual_file_pipeline::{publish_runtime_nodes, RuntimeNodePublication};
use crate::types::*;
use crate::VerterHost;
use verter_compiler::framework_common::RuntimeDiagnosticSeverity;

impl VerterHost {
    /// Execute ONE caller-supplied canonical [`CompileRequest`] against an
    /// already-registered source and return its typed result.
    ///
    /// This is the whole transaction in one call: the caller registers the
    /// source once through the ordinary source-only upsert, then hands
    /// this entry a canonical id and a request. There is no ensure-then-read
    /// pair to order correctly, and no boolean whose meaning a caller has
    /// to infer.
    ///
    /// The supplied request is the demand document end to end. NOTHING on
    /// this route builds a [`CompileProfile`] from it, reads a profile to
    /// decide what to compile, or consults a profile-keyed cache slot: the
    /// product set, the framework options, the source identity, and the
    /// published node set all come from the request itself, and the
    /// registered immutable source snapshot supplies the bytes. Supplied
    /// (externally preprocessed) block artifacts are visible ONLY from the
    /// profile-less admission bucket — the one `apply_block_overrides`
    /// writes to when the caller names no compile profile; artifacts
    /// admitted under a named profile stay invisible, and a block whose
    /// authored dialect needs preprocessing with no profile-less override
    /// still refuses as unavailable.
    ///
    /// The source is NOT part of the request. This entry reads the already
    /// stored snapshot by canonical id, resolving an alias once, and a
    /// request whose framework arm contradicts the registered carrier is
    /// refused rather than compiled under the wrong carrier.
    ///
    /// Complete-only: a construction, admission, or execution refusal fails
    /// the WHOLE request and publishes no sibling product. There is no
    /// partial response, no null, and no silent fallback to profile-derived
    /// demand.
    ///
    /// # Errors
    ///
    /// [`CompileRequestFailure`] — see its arms.
    pub fn compile_request(
        &self,
        canonical_id: &str,
        mut request: verter_compiler::compile_request::CompileRequest,
    ) -> Result<CompileRequestResponse, CompileRequestFailure> {
        use crate::block_content::SuppliedBlockScope;

        let canonical = self.resolve_alias_or_canonical(canonical_id);
        // The carrier file name is SOURCE identity, and the caller stated
        // it as this call's canonical id. Binding the unset slot here is
        // what keeps the component name, the scoped style hash, and every
        // emitted map's `sources` entry the same as they are for the same
        // registered source on any other route. A caller-stated name is
        // never overwritten.
        request.bind_default_filename(&canonical);

        // External block bytes are VFS-owned compiler inputs; resolve and
        // load those blockers before taking the coherent owner/content
        // capture below, exactly as the profile-derived lanes do.
        self.hydrate_compile_blockers(&canonical);
        let block_content_capture_fence = self.block_content.admission_fence.lock();

        // ── ONE coherent source snapshot ──
        // Every content-determined input derives from this single read, so
        // the bytes and the analysis beside them cohere.
        let source_snap = self.scheduler.try_get_source(&canonical).ok_or_else(|| {
            CompileRequestFailure::Host(HostError::MissingSource {
                canonical_id: canonical.clone(),
            })
        })?;
        let efs = self
            .effective_file_state_from_snapshot(&source_snap, &canonical, None)
            .ok_or_else(|| {
                CompileRequestFailure::Host(HostError::MissingSource {
                    canonical_id: canonical.clone(),
                })
            })?;

        let compile_input = {
            use crate::host_executor::HostSourceData;
            let hd = source_snap
                .downcast_data::<HostSourceData>()
                .ok_or_else(|| {
                    CompileRequestFailure::Host(HostError::MissingSource {
                        canonical_id: canonical.clone(),
                    })
                })?;
            let parse = &hd.parse;
            // This route reads supplied (externally preprocessed) block
            // artifacts from the PROFILE-LESS bucket — the bucket
            // `apply_block_overrides` writes to when the caller names no
            // compile profile — and no other: artifacts admitted under a
            // named profile stay invisible, so the route never inherits
            // another route's preprocessed bytes. A block whose authored
            // dialect needs external preprocessing and has no profile-less
            // override still refuses as unavailable.
            let style_content = self.capture_compiler_style_content(
                &canonical,
                &parse.style_analyses,
                SuppliedBlockScope::Unprofiled,
            );
            let block_content = self
                .capture_compiler_block_content(&canonical, SuppliedBlockScope::Unprofiled)
                .map_err(CompileRequestFailure::Host)?;
            CompileInput {
                canonical_id: canonical.clone(),
                source: efs.source,
                whole_hash: efs.whole_hash,
                meta: parse.meta.clone(),
                parse_diagnostics: parse.parse_diagnostics.clone(),
                src_blocks: parse.src_blocks.clone(),
                external_requests: parse.external_requests.clone(),
                has_supplied_block_content: block_content.has_supplied,
                block_content_inputs: block_content.inputs,
                macro_type_deps: efs.script_analysis.macro_type_deps.clone(),
                script_imports: efs.script_analysis.imports.clone(),
                script_macros: efs.script_analysis.macros.clone(),
                script_bindings: efs.script_analysis.bindings.clone(),
                script_macro_usage: efs.script_analysis.macro_usage.clone(),
                script_vue_api_calls: efs.script_analysis.vue_api_calls.clone(),
                framework_parse: efs.framework_parse,
                style_v_bind_vars: style_content.v_bind_vars,
                style_v_bind_usage_complete: style_content.usage_complete,
                prepared_styles: parse.prepared_styles.clone(),
            }
        };
        drop(block_content_capture_fence);

        // The host OWNS the cached-parse validity decision. The bound
        // backend executes over the artifact's own registered parse, so a
        // request carrying parse-affecting template grammar (custom
        // delimiters, custom element tag matchers) cannot be honored here
        // and refuses instead of compiling under the wrong grammar. Read
        // off the REQUEST — this route has no other grammar statement.
        //
        // Svelte has no counterpart gate, deliberately. Its `runes` and
        // `namespace` axes are parse-relevant too, but the profile-derived
        // Svelte lane routes them into the bundle execution over the same
        // registered artifact without gating either, so refusing them only
        // here would make the typed seam reject demands the legacy route
        // serves. The asymmetry tracks the two carriers' existing
        // behaviour; closing it belongs to the Svelte carrier's own
        // parse-key identity, not to this route.
        if let Some(vue) = request.vue() {
            let default_delimiters = vue
                .delimiters
                .as_ref()
                .is_none_or(|(open, close)| open == "{{" && close == "}}");
            if !default_delimiters || !vue.is_custom_element.is_empty() {
                return Err(CompileRequestFailure::Host(HostError::GrammarMismatch(
                    crate::carrier_publication_store::GrammarMismatch,
                )));
            }
        }

        // The registered carrier identity decides which framework backend
        // may execute, and the bound backend is the sole authority for
        // that decision: a request naming another framework is refused at
        // its admission and surfaces as the typed framework mismatch. This
        // route derives no framework identity of its own.
        let artifact = compile_input.framework_parse.as_deref().ok_or_else(|| {
            CompileRequestFailure::Refused {
                canonical_id: canonical.clone(),
                diagnostics: no_carrier_artifact_diagnostics(
                    &canonical,
                    compile_input.source.len() as u32,
                ),
            }
        })?;
        // The request-scoped binding for this compile ATTEMPT — the same
        // common binding point as the profile-derived routes. This route
        // consults and populates no compile cache slot, so it binds as a
        // stateless attempt.
        let binding = self
            .bind_native_host_compile_attempt(
                Some(artifact),
                &canonical,
                compile_input.source.len() as u32,
                &source_snap,
                CompileCacheMode::Stateless,
            )
            .map_err(CompileRequestFailure::Host)?;

        self.compile_request_entry(&compile_input, request, binding)
    }

    /// The caller-supplied-request compile lane: execute the ONE admitted
    /// population through the request's BOUND framework host-integration
    /// backend, then project the admitted products into the typed
    /// response.
    ///
    /// Publication is driven by the REQUEST's product set, not by profile
    /// target bits: a runtime product publishes its separately addressed
    /// virtual nodes, the IDE product publishes the IDE projection, and
    /// the analysis product publishes the host analysis payload — each row
    /// one-to-one with a requested kind, in request order.
    fn compile_request_entry(
        &self,
        snapshot: &CompileInput,
        request: verter_compiler::compile_request::CompileRequest,
        binding: Option<BoundNativeHostRequest>,
    ) -> Result<CompileRequestResponse, CompileRequestFailure> {
        use verter_compiler::compile_request::{CompileProduct, ProductKind};

        let canonical = snapshot.canonical_id.clone();
        let refused = |diagnostics: DiagnosticsSnapshot| CompileRequestFailure::Refused {
            canonical_id: canonical.clone(),
            diagnostics,
        };
        let diagnostics = snapshot.parse_diagnostics.clone();

        let Some(binding) = binding else {
            return Err(refused(diagnostics.merge(no_carrier_artifact_diagnostics(
                &canonical,
                snapshot.source.len() as u32,
            ))));
        };
        let artifact = snapshot
            .framework_parse
            .as_deref()
            .expect("the binding was issued over this input's own carrier artifact");

        // The published product ROWS are decided before execution, off the
        // request itself, so the response can never carry a row the caller
        // did not ask for or omit one it did.
        let requested: Vec<ProductKind> = request
            .products()
            .iter()
            .map(CompileProduct::kind)
            .collect();
        let assembly = crate::compile::VueMainAssemblyAxes {
            force_js: request.force_js(),
            source_map: request.products().iter().any(|product| match product {
                CompileProduct::RuntimeClient(runtime) | CompileProduct::RuntimeServer(runtime) => {
                    runtime.runtime_source_map
                }
                _ => false,
            }),
            runtime_module_name: request
                .vue()
                .and_then(|vue| vue.runtime_module_name.clone()),
            ssr: requested.contains(&ProductKind::RuntimeServer),
            is_production: request.is_production(),
            // The dev-server tooling flavour and the SSR-manifest key form
            // are host build knobs the canonical request carries as its own
            // axes (`with_host_assembly_axes`) — the same pair the legacy
            // `CompileProfile` states.
            hmr_strategy: match request.hmr_strategy() {
                verter_compiler::compile_request::RuntimeHmrStrategy::None => HmrStrategy::None,
                verter_compiler::compile_request::RuntimeHmrStrategy::Vite => HmrStrategy::Vite,
                verter_compiler::compile_request::RuntimeHmrStrategy::Webpack => {
                    HmrStrategy::Webpack
                }
            },
            // The official plugin emits the SSR-manifest registration
            // unconditionally on an ssr build (dev AND production), so an
            // ssr compile that omitted it would leave the bundler unable
            // to collect this module's render-tree dependencies.
            emit_ssr_module_registration: true,
            ssr_module_id: request.ssr_module_id().map(str::to_owned),
        };
        let policy = RuntimeNodePublication {
            // A runtime product is the whole runtime surface: its
            // separately addressed nodes are its published form, so every
            // node the admitted bundle produced belongs to that one row.
            publish_runtime_module: true,
            publish_script: true,
            publish_template: true,
            publish_style: true,
            runtime_module_name: assembly.runtime_module_name.clone(),
            assembly,
        };

        let alloc = Allocator::new();
        let products =
            match execute_supplied_host_request(self, binding, artifact, request, snapshot, &alloc)
            {
                Ok(products) => products,
                Err(SuppliedRequestFailure::FrameworkMismatch {
                    requested,
                    registered,
                }) => {
                    return Err(CompileRequestFailure::FrameworkMismatch {
                        canonical_id: canonical,
                        requested,
                        registered,
                    });
                }
                Err(SuppliedRequestFailure::UnsupportedProduct { kind, diagnostics }) => {
                    return Err(CompileRequestFailure::UnsupportedProduct {
                        canonical_id: canonical,
                        kind,
                        diagnostics,
                    });
                }
                Err(SuppliedRequestFailure::Products(HostProductsFailure::Fatal(payload))) => {
                    return Err(refused(diagnostics.merge(payload)));
                }
                // The backend fail-closed on the runtime surface this request
                // asked for. All-or-none: the transaction ends here, no output
                // is assembled and no sibling product is lifted. The non-fatal
                // diagnostics collected before the refusal still travel with
                // it, so the reason stays visible.
                Err(SuppliedRequestFailure::Products(HostProductsFailure::Surface {
                    diagnostic_code,
                    message,
                    span,
                    diagnostics: refusal_diagnostics,
                })) => {
                    let mut lifted = lift_runtime_diagnostics(&refusal_diagnostics);
                    lifted.push(HostDiagnostic {
                        severity: HostSeverity::Warning,
                        code: diagnostic_code.clone(),
                        message: message.clone(),
                        arguments: Vec::new(),
                        span,
                    });
                    return Err(CompileRequestFailure::RuntimeSurfaceRefused {
                        canonical_id: canonical,
                        diagnostic_code,
                        message,
                        diagnostics: diagnostics.merge(DiagnosticsSnapshot::from_vec(lifted)),
                    });
                }
            };

        // ONE deduplicated diagnostic set for the whole admitted compile,
        // not a per-product set each consumer would have to merge. The
        // dedup is against the carrier's own parse-time channel: the Vue
        // backend's bundle execution reuses the already-parsed artifact and
        // clones that same parse's diagnostics wholesale, so a parse-time
        // diagnostic is otherwise double-counted.
        let mut compile_diags = diagnostics.clone();
        if !products.diagnostics().is_empty() {
            compile_diags = compile_diags.merge_deduplicated(DiagnosticsSnapshot::from_vec(
                lift_runtime_diagnostics(products.diagnostics()),
            ));
        }
        if compile_diags.has_errors {
            return Err(refused(compile_diags));
        }

        let mut outputs = FxHashMap::default();
        publish_runtime_nodes(snapshot, &products, &policy, &mut outputs).map_err(refused)?;

        let mut rows = Vec::with_capacity(requested.len());
        for kind in requested {
            let row = match kind {
                kind @ (ProductKind::RuntimeClient | ProductKind::RuntimeServer) => {
                    // Stable node order, so a consumer reading rows in
                    // sequence sees the same addressing on every compile.
                    let mut nodes: Vec<(VirtualNodeKind, CachedVirtualFile)> =
                        std::mem::take(&mut outputs).into_iter().collect();
                    nodes.sort_by(|(left, _), (right, _)| left.cmp(right));
                    CompiledProduct::Runtime {
                        kind,
                        nodes: nodes
                            .into_iter()
                            .map(|(node, file)| CompiledVirtualNode {
                                node,
                                code: file.code,
                                source_map: file.source_map,
                                lang: file.lang,
                                meta: file.meta,
                            })
                            .collect(),
                    }
                }
                ProductKind::IdeCompanion => {
                    // Admission proves the capability is registered and the
                    // demand routable; it does NOT prove the execution
                    // published bytes. Complete-only: an absent payload
                    // fails the whole request naming the kind, rather than
                    // aborting the caller's thread on a premise the
                    // accessor does not carry.
                    //
                    // No production input reaches this arm today — every
                    // admitted IDE product publishes TSX, including for an
                    // empty carrier — so unlike the Analysis arm below it
                    // has no discriminating public-boundary test. It stays
                    // because the accessor is an `Option` and the
                    // alternative to handling `None` is a panic.
                    let Some(ide) = products.ide_companion() else {
                        return Err(CompileRequestFailure::ProductNotProduced {
                            canonical_id: canonical,
                            kind,
                            diagnostics: compile_diags,
                        });
                    };
                    CompiledProduct::Ide(IdeResponse {
                        code: Arc::from(ide.code.as_str()),
                        source_map: (!ide.source_map.is_empty())
                            .then(|| Arc::from(ide.source_map.as_str())),
                        is_jsx: ide.is_jsx,
                        destructured_block: ide.destructured_block.clone(),
                    })
                }
                ProductKind::Analysis => {
                    // The fact producer fails CLOSED to no payload — a
                    // selected `<template src="...">` whose bytes are not
                    // the admitted host block's, or a source that no longer
                    // binds to the artifact's parse key. That is deliberate
                    // behaviour, so the admitted-but-unpublished case is
                    // genuinely reachable and is reported typed.
                    let Some(facts) = products.template_facts() else {
                        return Err(CompileRequestFailure::ProductNotProduced {
                            canonical_id: canonical,
                            kind,
                            diagnostics: compile_diags,
                        });
                    };
                    let (analysis, _admission) = self.template_analysis_from_facts(snapshot, facts);
                    CompiledProduct::Analysis(Box::new(analysis))
                }
                // Refused at admission, so execution is unreachable for
                // these kinds — the seam reports them as the typed
                // unsupported failure above.
                ProductKind::PublicApi | ProductKind::Declarations => {
                    return Err(CompileRequestFailure::UnsupportedProduct {
                        canonical_id: canonical,
                        kind,
                        diagnostics: compile_diags,
                    })
                }
            };
            rows.push(row);
        }

        Ok(CompileRequestResponse {
            canonical_id: canonical,
            diagnostics: compile_diags,
            products: rows,
        })
    }
}

/// Lift this lane's compiled-surface diagnostics into the host diagnostic
/// shape. The profile-derived lanes each carry their own copy of this
/// mapping.
fn lift_runtime_diagnostics(
    diagnostics: &[verter_compiler::framework_common::RuntimeDiagnostic],
) -> Vec<HostDiagnostic> {
    diagnostics
        .iter()
        .map(|diagnostic| HostDiagnostic {
            severity: match diagnostic.severity {
                RuntimeDiagnosticSeverity::Error => HostSeverity::Error,
                RuntimeDiagnosticSeverity::Warning => HostSeverity::Warning,
                RuntimeDiagnosticSeverity::Info => HostSeverity::Info,
            },
            code: diagnostic.code.clone(),
            message: diagnostic.message.clone(),
            arguments: Vec::new(),
            span: diagnostic.span,
        })
        .collect()
}
