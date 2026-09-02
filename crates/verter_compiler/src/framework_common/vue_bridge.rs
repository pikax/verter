//! Vue carrier bridge.
//!
//! [`VueParseCarrier`] wraps a parsed SFC. [`build_vue_parse_artifact`]
//! produces the unregistered artifact; the projector owns inventory
//! geometry. Typed carrier is reachable only through this module's
//! downcasts or [`open_vue_carrier`]
//! (`carrier_downcast_confined_to_owning_adapter`).

use std::any::Any;
use std::sync::Arc;

use verter_language::{
    parse_key_for, syntax_profile_id_for, CarrierParse, FileLanguage, FrameworkAdapterId,
    JsModuleKind, LanguageId, ScriptSourceType, UnregisteredFrameworkParseArtifact,
    VUE_SYNTAX_COMPATIBILITY_DOMAIN, VUE_SYNTAX_COMPATIBILITY_EPOCH,
};
use verter_span::Span;

use verter_parser::parser::types::ParsedSfc;
use verter_parser::types::NodeProp;

use crate::compile::types::{VueExecutionInputs, VueMacroSemanticInput};
use crate::compile::{compile_from_parsed, parse_sfc};
use crate::compile_request::{
    CompileProduct, CompileRequest, CompileRequestError, FrameworkCompileRequest,
    IdeProductRequest, VueBackendRequest, VueCompileRequest,
};
use crate::framework_common::carrier_compiler::{
    CarrierCompileOutcome, CarrierCompiler, CompileUnsupported, IdeCompileOptions, IdeOutput,
    RuntimeCompileOptions, RuntimeCompileOutput, RuntimeCustomBlock, RuntimeDiagnostic,
    RuntimeMainModule, RuntimeOutputDescriptor, RuntimeScriptBlock, RuntimeStyleBlock,
    RuntimeTemplateBlock, SourceMapFidelity,
};
use crate::framework_common::FrameworkParseArtifact;
use verter_language::ParseOptions;

/// The concrete Vue carrier: the full parsed SFC behind the erasure
/// seam.
#[derive(Debug)]
pub struct VueParseCarrier {
    parsed: Arc<ParsedSfc>,
    parse_options: ParseOptions,
}

impl VueParseCarrier {
    /// Wrap a parsed SFC together with the parse-affecting options that
    /// produced it.
    pub fn new(parsed: Arc<ParsedSfc>, parse_options: ParseOptions) -> Self {
        Self {
            parsed,
            parse_options,
        }
    }

    /// The wrapped parse result.
    pub fn parsed(&self) -> &ParsedSfc {
        &self.parsed
    }

    /// The wrapped parse result, as the shared handle.
    pub fn parsed_arc(&self) -> &Arc<ParsedSfc> {
        &self.parsed
    }

    /// Parse-affecting options stamped at artifact construction.
    pub fn parse_options(&self) -> &ParseOptions {
        &self.parse_options
    }
}

impl CarrierParse for VueParseCarrier {
    fn __verter_as_any(&self) -> &dyn Any {
        self
    }
    fn __verter_as_any_arc(self: Arc<Self>) -> Arc<dyn Any + Send + Sync> {
        self
    }
}

/// Attribute lookup mirroring the session's historical
/// `extract_attrs` + `find_attr` semantics byte-for-byte: names match
/// case-insensitively, a present-but-empty value reads as `"true"`.
/// Returns the value string plus the value span when one exists.
fn attr_value(props: &[NodeProp], source: &str, name: &str) -> Option<(String, Option<Span>)> {
    props.iter().find_map(|p| {
        let attr_name = &source[p.start as usize..p.name_end as usize];
        if !attr_name.eq_ignore_ascii_case(name) {
            return None;
        }
        match (p.value_start, p.value_end) {
            (Some(s), Some(e)) => {
                let value = &source[s as usize..e as usize];
                if value.is_empty() {
                    Some(("true".to_string(), Some(Span::new(s, e))))
                } else {
                    Some((value.to_string(), Some(Span::new(s, e))))
                }
            }
            _ => Some(("true".to_string(), None)),
        }
    })
}

/// Resolve the SFC's script source type from `<script lang>` data —
/// the first script block (plain `<script>` before `<script setup>`)
/// carrying an explicit `lang` attribute decides; `tsx`/`jsx`/`js`
/// (ASCII-case-insensitively) map to their dialects, anything else is
/// TypeScript.
pub fn vue_script_source_type(parsed: &ParsedSfc, source: &str) -> ScriptSourceType {
    let lang = [parsed.script(), parsed.script_setup()]
        .into_iter()
        .flatten()
        .find_map(|script| {
            attr_value(&script.attributes, source, "lang")
                .map(|(value, _)| value)
                .filter(|v| v != "true")
        });

    // The JS module kinds pin the historical Vue carrier dialects:
    // `lang="js"` resolves the classic-script grammar
    // (`JsModuleKind::Script`) and `lang="jsx"` the module grammar
    // (`JsModuleKind::Module`) — the exact OXC `SourceType`s
    // (`script()` / `jsx()`) the Vue parse pipeline has always
    // computed for these rows.
    match lang.as_deref().map(|value| value.to_ascii_lowercase()) {
        Some(lang) if lang == "tsx" => ScriptSourceType::Tsx,
        Some(lang) if lang == "jsx" => ScriptSourceType::Jsx(JsModuleKind::Module),
        Some(lang) if lang == "js" => ScriptSourceType::Js(JsModuleKind::Script),
        _ => ScriptSourceType::Ts,
    }
}

/// Map the Vue carrier's own parse diagnostics onto the framework-neutral
/// mapped channel. Every entry carries a real retained span (`Diagnostic`'s
/// own `span` field, never a fabricated one); the code crosses as the
/// `CompilerErrorCode`'s stable `code_str()` spelling, and arguments pass
/// through directly (`verter_parser::diagnostics::Diagnostic::arguments` is
/// already `Vec<verter_language::DiagnosticArg>`).
fn vue_parse_diagnostics(
    diagnostics: &[crate::diagnostics::Diagnostic],
    parse_key: &verter_language::ParseKey,
) -> Vec<verter_language::LanguageDiagnostic> {
    let mut mapped: Vec<verter_language::LanguageDiagnostic> = diagnostics
        .iter()
        .map(|diagnostic| verter_language::LanguageDiagnostic {
            span: diagnostic.span,
            severity: match diagnostic.severity {
                crate::diagnostics::DiagnosticSeverity::Error => {
                    verter_language::LanguageDiagnosticSeverity::Error
                }
                crate::diagnostics::DiagnosticSeverity::Warning => {
                    verter_language::LanguageDiagnosticSeverity::Warning
                }
                crate::diagnostics::DiagnosticSeverity::Info => {
                    verter_language::LanguageDiagnosticSeverity::Info
                }
            },
            code: diagnostic.code.code_str(),
            arguments: diagnostic.arguments.clone(),
            message: diagnostic.message.clone(),
            // Every Vue parse-time diagnostic (`MissingSfcEntryBlock`,
            // `TemplateFunctionalUnsupported`, …) describes a defect the
            // compiler cannot safely produce output around — unlike
            // Svelte's recoverable strict-parse facts, there is no
            // "faithful tree, still compilable" middle ground here.
            blocks_compile: true,
        })
        .collect();
    verter_language::sort_language_diagnostics(parse_key, &mut mapped);
    mapped
}

/// Wrap a parsed Vue SFC for the registered projector.
pub fn build_vue_parse_artifact(
    source: &str,
    parsed: Arc<ParsedSfc>,
    options: &ParseOptions,
) -> Arc<UnregisteredFrameworkParseArtifact> {
    let language = FileLanguage::vue();
    let syntax_profile =
        syntax_profile_id_for(&language, options).expect("the built-in Vue syntax profile exists");
    let parse_key = parse_key_for(
        source,
        &language,
        VUE_SYNTAX_COMPATIBILITY_DOMAIN,
        VUE_SYNTAX_COMPATIBILITY_EPOCH,
        &syntax_profile,
    )
    .expect("the built-in Vue parse identity exists");
    let diagnostics = vue_parse_diagnostics(&parsed.diagnostics, &parse_key);
    Arc::new(UnregisteredFrameworkParseArtifact::new(
        FrameworkAdapterId::vue(),
        LanguageId::new("vue"),
        Arc::new(parse_key),
        Arc::new(syntax_profile),
        diagnostics,
        Arc::new(VueParseCarrier::new(parsed, options.clone())),
    ))
}

/// The Vue carrier compiler — the reference [`CarrierCompiler`].
///
/// Delegates call-for-call to the existing Vue pipeline (`parse_sfc` +
/// `compile_from_parsed`): it edits NO Vue parser or codegen module and
/// reaches the parsed SFC back out of the type-erased artifact through
/// its own inherent downcast — no capability token, since only this
/// adapter's own inherent methods call the raw carrier downcast on its
/// own artifacts.
#[derive(Default)]
pub struct VueCarrierCompiler;

impl std::fmt::Debug for VueCarrierCompiler {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("VueCarrierCompiler").finish_non_exhaustive()
    }
}

/// The registered-projector opener installed on the Vue framework leg
/// (`CarrierLeg::open`). Returns the artifact's erased carrier ONLY for a
/// Vue-adapter artifact — the sole cross-crate entry point for reaching a
/// Vue registered artifact's typed carrier.
#[doc(hidden)]
pub fn open_vue_carrier(artifact: &FrameworkParseArtifact) -> Option<Arc<dyn CarrierParse>> {
    artifact.erased_carrier_for_adapter(&FrameworkAdapterId::vue())
}

impl VueCarrierCompiler {
    pub(super) fn unregistered_carrier_arc(
        &self,
        artifact: &UnregisteredFrameworkParseArtifact,
    ) -> Option<Arc<VueParseCarrier>> {
        verter_language::__carrier_downcast_arc::<VueParseCarrier>(artifact)
    }

    /// Reach the parsed SFC back out of a Vue artifact, or `None` when the
    /// erased payload is not a Vue carrier. The inherent downcast home for
    /// the Vue bridge.
    pub(super) fn parsed_sfc<'a>(
        &self,
        artifact: &'a FrameworkParseArtifact,
    ) -> Option<&'a ParsedSfc> {
        artifact
            .carrier_ref::<VueParseCarrier>()
            .map(VueParseCarrier::parsed)
    }

    pub(super) fn unregistered_parsed_sfc<'a>(
        &self,
        artifact: &'a UnregisteredFrameworkParseArtifact,
    ) -> Option<&'a ParsedSfc> {
        verter_language::__carrier_downcast_ref::<VueParseCarrier>(artifact)
            .map(VueParseCarrier::parsed)
    }
}

/// Compile a Vue artifact already elected by a registered publication store.
/// No raw-source parsing is reachable through this boundary.
#[doc(hidden)]
pub fn compile_registered_vue_artifact(
    source: &str,
    artifact: &FrameworkParseArtifact,
    request: &crate::compile_request::CompileRequest,
    execution_inputs: &VueExecutionInputs,
    macro_semantics: &VueMacroSemanticInput,
    allocator: &oxc_allocator::Allocator,
) -> Result<crate::compile::VerterCompileResult, CompileUnsupported> {
    let compiler = VueCarrierCompiler;
    let Some(parsed) = compiler.parsed_sfc(artifact) else {
        return Err(CompileUnsupported::NoIdeProjection {
            adapter_id: artifact.adapter_id().clone(),
        });
    };
    let exact_source = artifact
        .inventory()
        .source_spaces()
        .first()
        .is_some_and(|space| space.bytes().as_ref() == source);
    if !exact_source {
        return Err(CompileUnsupported::NoIdeProjection {
            adapter_id: artifact.adapter_id().clone(),
        });
    }
    let result = compile_from_parsed(
        source,
        parsed,
        request,
        execution_inputs,
        macro_semantics,
        allocator,
    )
    .map_err(CompileUnsupported::RequestExecutionRefused)?;
    // Determinism digest over the emitted block LENGTHS: two runs over the
    // same source must emit the same sizes. A cheap tripwire for
    // nondeterministic codegen — it does not hash the bytes.
    verter_audit::attribute_digest!(
        CompiledOutputDigest,
        (result.script.as_ref().map_or(0usize, |b| b.code.len()) as u64).wrapping_mul(0x1000_0001)
            ^ (result.template.as_ref().map_or(0usize, |b| b.code.len()) as u64)
                .wrapping_mul(0x1000_0003)
            ^ (result.tsx.as_ref().map_or(0usize, |b| b.code.len()) as u64)
                .wrapping_mul(0x1000_0007)
            ^ (result.styles.len() as u64).wrapping_mul(0x1000_000d)
    );
    Ok(result)
}

/// Resolves the Vue backend (Vdom/Vapor) a [`compile_registered_vue_artifact`]
/// call against this exact `(artifact, request)` pair will use — the SAME
/// resolution `compile_from_parsed` applies internally
/// (`request.resolve_vue_backend(parsed.is_vapor())`), exposed so audit /
/// reporting call sites can attribute the REAL resolved backend (accounting
/// for the source's own implicit `<template vapor>` marker) rather than
/// re-deriving a lossy approximation from the request's declared backend
/// intent alone. Returns `None` when the artifact carries no parsed Vue SFC
/// (foreign carrier) — the caller has nothing to resolve against.
pub fn resolve_vue_backend_for_audit(
    artifact: &FrameworkParseArtifact,
    request: &crate::compile_request::CompileRequest,
) -> Option<
    Result<crate::compile_request::ResolvedVueBackend, crate::compile_request::CompileRequestError>,
> {
    let compiler = VueCarrierCompiler;
    let parsed = compiler.parsed_sfc(artifact)?;
    Some(request.resolve_vue_backend(parsed.is_vapor()))
}

impl CarrierCompiler for VueCarrierCompiler {
    fn adapter_id(&self) -> FrameworkAdapterId {
        FrameworkAdapterId::vue()
    }

    fn carrier_language_id(&self) -> LanguageId {
        // The `.vue` SFC carrier language. A same-adapter non-carrier row
        // (e.g. an external Vue template) is NOT this language and is not
        // routed through the SFC parse path.
        LanguageId::new("vue")
    }

    fn parse(
        &self,
        source: &str,
        opts: &ParseOptions,
    ) -> Result<Arc<UnregisteredFrameworkParseArtifact>, verter_language::SyntaxReject> {
        // This crate never substitutes a default for an absent delimiter
        // pair — that decision belongs to whoever constructs `ParseOptions`
        // (see `ParseOptions::vue_standard`). It only rejects a request it
        // cannot honor: a literal empty-tuple delimiter pair is un-
        // tokenizable — an empty open AND an empty close both match every
        // position with zero-width consumption, which hangs the tokenizer
        // in a zero-advance loop (verified empirically: >1,000,000 events
        // emitted without terminating). Every other pair, including one
        // that happens to equal the tokenizer's own built-in default, is
        // passed through to `parse_sfc` literally.
        if opts.delimiters.0.is_empty() && opts.delimiters.1.is_empty() {
            let language = FileLanguage::vue();
            let syntax_profile = syntax_profile_id_for(&language, opts)
                .expect("the built-in Vue syntax profile exists");
            let parse_key = parse_key_for(
                source,
                &language,
                VUE_SYNTAX_COMPATIBILITY_DOMAIN,
                VUE_SYNTAX_COMPATIBILITY_EPOCH,
                &syntax_profile,
            )
            .expect("the built-in Vue parse identity exists");
            return Err(verter_language::SyntaxReject::UnsupportedProfile {
                parse_key: Arc::new(parse_key),
                syntax_profile: Arc::new(syntax_profile),
                reason: verter_language::UnsupportedSyntaxProfileReason::UnsupportedOption,
            });
        }
        let delimiters = Some((opts.delimiters.0.as_str(), opts.delimiters.1.as_str()));
        let parsed = Arc::new(parse_sfc(source, delimiters, Some(&opts.custom_elements)));
        Ok(build_vue_parse_artifact(source, parsed, opts))
    }

    fn compile_ide(
        &self,
        source: &str,
        artifact: &FrameworkParseArtifact,
        opts: &IdeCompileOptions,
    ) -> Result<IdeOutput, CompileUnsupported> {
        let request = vue_ide_only_request(
            opts.filename.clone(),
            None,
            false,
            false,
            IdeProductRequest {
                want_source_map: !opts.skip_source_map,
                embed_ambient_types: opts.embed_ambient_types,
                ide_chunk_boundaries: opts.block_content.template.is_some(),
                ..Default::default()
            },
            vue_request_from_admitted_artifact(artifact),
        )?;
        // This registry route carries no host-issued admission, so the
        // projection grant is minted at the route boundary (crate-private
        // mint); admission-issued flows carve theirs off the consumed
        // admission instead.
        let grant = super::capability::ProductExecutionGrant::mint(
            crate::compile_request::ProductKind::IdeCompanion,
        );
        super::registered_carrier_projection::project_ide_from_catalog(
            grant,
            artifact,
            source,
            &request,
            &super::registered_carrier_projection::ProjectionCatalogInputs {
                block_content: opts.block_content.clone(),
                vue_execution: VueExecutionInputs::default(),
                vue_macros: VueMacroSemanticInput::default(),
            },
        )
        .map(|companion| companion.ide)
    }

    fn compile_bundle(
        &self,
        source: &str,
        artifact: &FrameworkParseArtifact,
        opts: &RuntimeCompileOptions,
        alloc: &oxc_allocator::Allocator,
    ) -> Result<CarrierCompileOutcome, CompileUnsupported> {
        vue_carrier_bundle(
            source,
            artifact,
            opts,
            alloc,
            super::carrier_compiler::registry_route_execution_grants(opts),
        )
    }
}

/// The one Vue bundle orchestration over an admitted parse: ordered
/// runtime, IDE-projection, and template-fact capability calls with shared
/// prerequisites and deduplicated diagnostics. Shared by the compatibility
/// [`CarrierCompiler::compile_bundle`] route and the Vue host-integration
/// backend so both drive the identical single-population pass.
///
/// Every product-backend leg requires — and consumes — its own
/// [`super::capability::ProductExecutionGrant`]: the host route carves
/// them off the consumed admission, the registry bundle route mints them
/// crate-privately at its boundary, and a demanded-but-ungranted leg
/// fails closed typed.
pub(crate) fn vue_carrier_bundle(
    source: &str,
    artifact: &FrameworkParseArtifact,
    opts: &RuntimeCompileOptions,
    alloc: &oxc_allocator::Allocator,
    mut grants: super::capability::ProductExecutionGrants,
) -> Result<CarrierCompileOutcome, CompileUnsupported> {
    let Some(parsed) = VueCarrierCompiler.parsed_sfc(artifact) else {
        return Err(CompileUnsupported::NoIdeProjection {
            adapter_id: VueCarrierCompiler.adapter_id(),
        });
    };

    // The same two fail-closed rules `CompileRequest::new` /
    // `resolve_vue_backend` apply to every `CompileRequest`-constructed
    // route apply here too: this orchestration is a SEPARATE production
    // entry into the same shared codegen substrate — the host-integration
    // backend reaches it from an admitted `CompileRequest` (options
    // derived off the admitted request), while the retained registry
    // route reaches it from bare `RuntimeCompileOptions`. Without this
    // check an SSR x Vapor or inline x SSR request would silently reach
    // codegen and produce wrong output instead of a typed refusal — the
    // same bug class `CompileRequest` construction closes for its own
    // callers. `parsed.is_vapor()` covers the implicit `<template
    // vapor>` marker; `opts.force_vapor` covers the explicit request —
    // together they mirror `resolve_vue_backend`'s `Inferred` fallback.
    let effective_vapor = opts.force_vapor || parsed.is_vapor();
    if opts.ssr && effective_vapor {
        return Err(CompileUnsupported::RequestExecutionRefused(
            CompileRequestError::SsrVaporBackendUnsupported,
        ));
    }
    if opts.ssr && opts.inline == Some(true) {
        return Err(CompileUnsupported::RequestExecutionRefused(
            CompileRequestError::InlineSsrUnsupported,
        ));
    }

    // A selected template fragment is inserted at the compiler-registered
    // IDE hole. For a carrier with only a plain script that hole is the
    // script-content boundary, which is mid-module rather than a valid JSX
    // statement position. These are parser-owned carrier facts plus the
    // host-selected block input; do not rediscover the geometry by scanning
    // source text. Keep this exact partially-capable class fail-closed.
    if opts.want_ide
        && opts.block_content.template.is_some()
        && parsed.script().is_some()
        && parsed.script_setup().is_none()
    {
        return Err(CompileUnsupported::BlockContentIdeUnavailable {
            adapter_id: VueCarrierCompiler.adapter_id(),
        });
    }

    match (
        opts.block_content.script.as_ref(),
        opts.block_content.script_setup.as_ref(),
    ) {
        (Some(_), Some(_)) | (Some(_), None) | (None, Some(_)) if opts.want_ide => {
            return Err(CompileUnsupported::BlockContentIdeUnavailable {
                adapter_id: VueCarrierCompiler.adapter_id(),
            });
        }
        _ => {}
    }

    // Runtime products come from one bundler pass. IDE projection is
    // catalog-owned and never re-enters compile_inner as TSX on this
    // path. Template facts are catalog-owned. An IDE-only request
    // therefore skips the bundler pass unless a runtime product is
    // actually required.
    let mut bundle = RuntimeCompileOutput::default();
    if opts.want_runtime {
        // The runtime leg is selected from the immutable catalog at
        // execution, exactly like the projection and template-fact legs —
        // the catalog is execution's authority, not just admission's
        // boolean. A miss (no registered Vue runtime row for this
        // adapter × epoch) refuses typed: the runtime capability this
        // request touches is not available, never a fallback emitter.
        let Some(crate::standalone::InstalledRuntimeBackend::Vue(backend)) =
            crate::standalone::registered_runtime_for(artifact.adapter_id(), artifact.epoch())
        else {
            return Err(CompileUnsupported::RequestExecutionRefused(
                CompileRequestError::CapabilityUnsupported(if opts.ssr {
                    crate::compile_request::CapabilityCell::VueSsr
                } else if effective_vapor {
                    crate::compile_request::CapabilityCell::VueVaporClient
                } else {
                    crate::compile_request::CapabilityCell::VueVdomClient
                }),
            ));
        };
        // The runtime leg executes only under its consume-once grant for
        // the demanded runtime kind — never with no admission evidence.
        let expected_runtime = if opts.ssr {
            crate::compile_request::ProductKind::RuntimeServer
        } else {
            crate::compile_request::ProductKind::RuntimeClient
        };
        let Some(grant) = grants
            .runtime
            .take()
            .filter(|grant| grant.admits(expected_runtime))
        else {
            return Err(CompileUnsupported::ProductExecutionUngranted {
                product: expected_runtime,
            });
        };
        bundle = backend.compile_bundle_runtime(grant, source, parsed, opts, alloc)?;
    }

    if opts.want_ide {
        let mut vue = vue_request_from_admitted_artifact(artifact);
        vue.backend = if opts.force_vapor {
            VueBackendRequest::Vapor
        } else {
            VueBackendRequest::Inferred
        };
        vue.ssr = opts.ssr;
        vue.comments = opts.comments;
        vue.runtime_module_name = opts.runtime_module_name.clone();
        vue.script_custom_element = Some(opts.custom_element);
        let request = vue_ide_only_request(
            opts.filename.clone(),
            opts.component_id.clone(),
            opts.is_production,
            opts.force_js,
            IdeProductRequest {
                want_source_map: opts.ide_source_map.unwrap_or(opts.source_map),
                embed_ambient_types: opts.embed_ambient_types,
                conditional_root_narrowing: opts.conditional_root_narrowing,
                strict_slots: opts.strict_slots,
                types_module_name: opts.types_module_name.clone(),
                ide_chunk_boundaries: opts.block_content.template.is_some(),
                ..Default::default()
            },
            vue,
        )?;
        let execution = opts.vue_facts.clone().unwrap_or_default();
        let macros = execution
            .macro_runtime
            .clone()
            .map(VueMacroSemanticInput::Runtime)
            .unwrap_or_default();
        // The projection leg executes only under its consume-once grant.
        let Some(grant) = grants
            .projection
            .take()
            .filter(|grant| grant.admits(crate::compile_request::ProductKind::IdeCompanion))
        else {
            return Err(CompileUnsupported::ProductExecutionUngranted {
                product: crate::compile_request::ProductKind::IdeCompanion,
            });
        };
        let companion = super::registered_carrier_projection::project_ide_from_catalog(
            grant,
            artifact,
            source,
            &request,
            &super::registered_carrier_projection::ProjectionCatalogInputs {
                block_content: opts.block_content.clone(),
                vue_execution: execution,
                vue_macros: macros,
            },
        )?;
        bundle.tsx = Some(companion.ide);
        if opts.want_runtime {
            extend_unique_diagnostics(&mut bundle.diagnostics, companion.diagnostics);
        } else {
            bundle.diagnostics = companion.diagnostics;
        }
    }

    // Vue emits a runtime surface or a genuine compile error; it never
    // fail-closes on an unsupported runtime surface the way Svelte does, so
    // its outcome is always `Produced`.
    Ok(CarrierCompileOutcome::Produced(
        with_catalog_template_facts(
            bundle,
            artifact,
            source,
            opts.want_template_data,
            opts.block_content
                .template
                .as_ref()
                .map(|input| input.code.as_ref()),
        ),
    ))
}

fn vue_ide_only_request(
    filename: Option<String>,
    component_id: Option<String>,
    is_production: bool,
    force_js: bool,
    ide: IdeProductRequest,
    vue: VueCompileRequest,
) -> Result<CompileRequest, CompileUnsupported> {
    CompileRequest::new(
        vec![CompileProduct::IdeCompanion(ide)],
        FrameworkCompileRequest::Vue(vue),
        None,
        filename,
        component_id,
        is_production,
        force_js,
    )
    .map_err(CompileUnsupported::RequestExecutionRefused)
}

fn vue_request_from_admitted_artifact(artifact: &FrameworkParseArtifact) -> VueCompileRequest {
    let Some(options) = artifact
        .carrier_ref::<VueParseCarrier>()
        .map(VueParseCarrier::parse_options)
    else {
        return VueCompileRequest::default();
    };
    VueCompileRequest {
        delimiters: Some(options.delimiters.clone()),
        is_custom_element: options.custom_elements.clone(),
        ..Default::default()
    }
}

fn extend_unique_diagnostics(dest: &mut Vec<RuntimeDiagnostic>, extra: Vec<RuntimeDiagnostic>) {
    for diagnostic in extra {
        let already = dest.iter().any(|existing| {
            existing.code == diagnostic.code
                && existing.message == diagnostic.message
                && existing.span == diagnostic.span
        });
        if !already {
            dest.push(diagnostic);
        }
    }
}

fn with_catalog_template_facts(
    mut bundle: RuntimeCompileOutput,
    artifact: &FrameworkParseArtifact,
    source: &str,
    want: bool,
    selected_template: Option<&str>,
) -> RuntimeCompileOutput {
    bundle.template_data = if want {
        let basis = match selected_template {
            Some(bytes) => {
                super::registered_carrier_projection::TemplateFactsBasis::SelectedTemplate(bytes)
            }
            None => super::registered_carrier_projection::TemplateFactsBasis::AdmittedArtifact,
        };
        super::registered_carrier_projection::template_facts_from_catalog(artifact, source, basis)
            .inspect(|facts| {
                // The fact producer is the only pass that parses template
                // expressions on this route — its diagnostics (e.g. a
                // malformed `v-if` expression) are published with the
                // bundle, deduplicated against the carrier's own channel.
                // The product keeps its own copy attached so downstream
                // conversions of the facts carry the same set.
                extend_unique_diagnostics(&mut bundle.diagnostics, facts.diagnostics.clone());
            })
    } else {
        None
    };
    bundle
}

/// Re-express a Vue [`VerterCompileResult`] as the framework-neutral
/// [`RuntimeCompileOutput`]. Vue leaves `main.body_code` `None` — the host
/// assembles the `_sfc_main` module from the neutral block fields (its
/// virtual-file concern: style/custom virtual imports + HMR).
///
/// Public so conformance/test harnesses can drive the genuine
/// compile → bundle → assemble pipeline without re-implementing the
/// conversion (the Vue conformance seed in `verter_vue_conformance`).
pub fn vue_result_to_runtime_bundle(
    source: &str,
    parsed: &ParsedSfc,
    result: crate::compile::VerterCompileResult,
) -> RuntimeCompileOutput {
    let (carrier_space, carrier_artifact) = RuntimeOutputDescriptor::carrier_source(source);
    let declared = [(carrier_space.as_str(), carrier_artifact.as_str())];
    let script = result.script.map(|s| {
        let output_descriptor = RuntimeOutputDescriptor::generated(
            &s.code,
            (!s.source_map.is_empty()).then_some(s.source_map.as_str()),
            &declared,
            SourceMapFidelity::Approximate,
        );
        RuntimeScriptBlock {
            code: s.code,
            source_map: s.source_map,
            setup: s.setup,
            output_descriptor,
            generated_template_hole: s.generated_template_hole,
            runtime_imports: s
                .runtime_imports
                .iter()
                .map(|name| (*name).to_string())
                .collect(),
            sfc_export_placement: s.sfc_export_placement,
        }
    });
    let template = result.template.map(|t| {
        let output_descriptor = RuntimeOutputDescriptor::generated(
            &t.code,
            (!t.source_map.is_empty()).then_some(t.source_map.as_str()),
            &declared,
            SourceMapFidelity::Approximate,
        );
        RuntimeTemplateBlock {
            code: t.code,
            source_map: t.source_map,
            imports: t.imports.iter().map(|s| (*s).to_string()).collect(),
            ssr_imports: t.ssr_imports.iter().map(|s| (*s).to_string()).collect(),
            render_export: t.render_export,
            output_descriptor,
        }
    });
    let styles = result
        .styles
        .into_iter()
        .zip(parsed.style_nodes())
        .map(|(s, node)| {
            let exact_map = node.content.as_ref().and_then(|content| {
                let authored = &source[content.start as usize..content.end as usize];
                (authored == s.code)
                    .then(|| exact_slice_source_map(source, content.start, authored))
            });
            let output_descriptor = RuntimeOutputDescriptor::generated(
                &s.code,
                exact_map.as_deref(),
                &declared,
                if exact_map.is_some() {
                    SourceMapFidelity::Exact
                } else {
                    SourceMapFidelity::Approximate
                },
            );
            RuntimeStyleBlock {
                code: s.code,
                // Vue's style pipeline produces no per-block css map here (style
                // post-processing happens host/JS-side), and carries no
                // `:global` fact.
                source_map: None,
                lang: s.lang,
                // Vue scoping rides the `data-v-…` attribute on `scope_id`, not a
                // per-block class hash.
                scope_hash: None,
                has_global: false,
                output_descriptor,
            }
        })
        .collect();
    let custom_blocks = result
        .custom_blocks
        .into_iter()
        .map(|b| RuntimeCustomBlock {
            block_type: b.block_type,
            content: b.content,
        })
        .collect();
    let tsx = result.tsx.map(|tsx| {
        let output_descriptor = RuntimeOutputDescriptor::generated(
            &tsx.code,
            (!tsx.source_map.is_empty()).then_some(tsx.source_map.as_str()),
            &declared,
            SourceMapFidelity::Approximate,
        );
        IdeOutput {
            code: tsx.code,
            source_map: tsx.source_map,
            is_jsx: tsx.is_jsx,
            duration_ms: tsx.duration_ms,
            destructured_block: tsx.destructured_block,
            output_descriptor,
            generated_template_hole: tsx.generated_template_hole,
            generated_template_chunk: tsx.generated_template_chunk,
        }
    });
    // The template-data extraction pass's own diagnostic slice rides WITH
    // the facts. It is a subset of `errors` — the bundle's `diagnostics`
    // below already publishes it once for this route — so the attached copy
    // exists for downstream conversions of the data, not as a second
    // publication.
    let template_facts_diagnostics: Vec<RuntimeDiagnostic> = result
        .template_data_diagnostics
        .iter()
        .map(|d| RuntimeDiagnostic {
            severity: d.severity.into(),
            code: d.code.clone(),
            message: d.message.clone(),
            span: d
                .span
                .unwrap_or_else(|| verter_span::Span::new(0, source.len() as u32)),
        })
        .collect();
    let template_data = result.template_data.map(|data| {
        super::registered_carrier_projection::TemplateFactsProduct {
            data,
            diagnostics: template_facts_diagnostics,
        }
    });
    let diagnostics = result
        .errors
        .into_iter()
        .map(|d| RuntimeDiagnostic {
            severity: d.severity.into(),
            code: d.code,
            message: d.message,
            // Vue codegen's `CompileDiagnostic.span` is itself optional (a
            // Vue-codegen concern out of this boundary's scope); a diagnostic
            // with no mapped construct location is a whole-component result
            // at THIS boundary, so it carries the whole-source span rather
            // than an absent one.
            span: d
                .span
                .unwrap_or_else(|| verter_span::Span::new(0, source.len() as u32)),
        })
        .collect();

    RuntimeCompileOutput {
        // Vue: host-assembled main module — no directly-emitted body.
        main: RuntimeMainModule::default(),
        script,
        template,
        styles,
        custom_blocks,
        scope_id: result.scope_id,
        tsx,
        template_data,
        diagnostics,
        // The RESOLVED inline topology — the compiler already merged the
        // render into `setup()` when true, so host assembly takes the inline
        // branch (no render attach, no setup-return filter).
        inline: result.inline,
    }
}

fn exact_slice_source_map(source: &str, source_start: u32, output: &str) -> String {
    let resolver = crate::cursor::position::PositionResolver::new(source);
    let mut builder = oxc_sourcemap::SourceMapBuilder::default();
    let source_id = builder.set_source_and_content("carrier", source);
    let mut source_offset = source_start as usize;
    for (generated_line, line) in output.split_inclusive('\n').enumerate() {
        let (source_line, source_column) = resolver.offset_to_line_and_col(source_offset);
        builder.add_token(
            generated_line as u32,
            0,
            (source_line - 1) as u32,
            (source_column - 1) as u32,
            Some(source_id),
            None,
        );
        source_offset += line.len();
    }
    builder.into_sourcemap().to_json_string()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::framework_common::carrier_compiler::CompileBundleProducedExt;
    use crate::framework_common::{
        carrier_compiler::OutputSourceSpaceKind, FrameworkSemanticAuthority,
        RuntimeBlockContentInput, RuntimeBlockContentInputs,
    };
    use std::fs;
    use std::path::{Path, PathBuf};
    use std::process::Command;
    use verter_css_syntax::CssDialect;
    use verter_language::{ExternalLinkKind, ScriptRegionKind};

    /// Stand in for the host's identity for the tool that produced the bytes
    /// these tests supply.
    fn supplied_producer() -> verter_css_syntax::StyleProducer {
        verter_css_syntax::StyleProducer::External(
            verter_css_syntax::ExternalStyleProducer::new("sass", Some("1.77.0"), None)
                .expect("named producer"),
        )
    }

    /// Stand in for the host's admission of already-preprocessed bytes, which
    /// is the shape these tests exercise.
    fn supplied_style(css: &str) -> crate::style_planner::PreparedStyleIr {
        crate::style_planner::prepare_supplied_style(
            verter_css_syntax::PreprocessedStyle::admitted(css, supplied_producer()),
        )
        .expect("supplied css parses")
    }

    fn workspace_root() -> PathBuf {
        PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .and_then(Path::parent)
            .expect("crate is <workspace>/crates/verter_compiler")
            .to_path_buf()
    }

    fn typescript_launcher() -> PathBuf {
        let launcher = workspace_root().join("node_modules/typescript/lib/tsc.js");
        assert!(
            launcher.is_file(),
            "output-validity tests require the pinned TypeScript launcher at {}; run pnpm install",
            launcher.display()
        );
        launcher
    }

    fn typescript_syntax_check(
        node_program: &Path,
        launcher: &Path,
        name: &str,
        code: &str,
        is_jsx: bool,
        extra_args: &[&str],
    ) -> Result<(), String> {
        let project = tempfile::tempdir().expect("create TSX validity directory");
        let extension = if is_jsx { "jsx" } else { "tsx" };
        let path = project.path().join(format!("{name}.{extension}"));
        let environment = project.path().join("jsx-runtime.d.ts");
        fs::write(&path, code).expect("write emitted TSX");
        fs::write(
            &environment,
            concat!(
                "declare module 'vue/jsx-runtime' {\n",
                "  export const Fragment: any;\n",
                "  export function jsx(...args: any[]): any;\n",
                "  export function jsxs(...args: any[]): any;\n",
                "}\n",
            ),
        )
        .expect("write JSX runtime validity stub");
        let output = Command::new(node_program)
            .arg(launcher)
            .args([
                "--noEmit",
                "--skipLibCheck",
                "--ignoreConfig",
                "--allowJs",
                "--checkJs",
                "false",
                "--jsx",
                "preserve",
                "--strictNullChecks",
                "false",
                "--pretty",
                "false",
                "--listFiles",
            ])
            .args(extra_args)
            .arg(&environment)
            .arg(&path)
            .current_dir(project.path())
            .output()
            .map_err(|error| format!("failed to run tsc syntax gate: {error}"))?;
        let diagnostics = format!(
            "{}{}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
        println!("TSC_VALIDITY {name} EXIT={:?}", output.status.code());
        let Some(exit_code) = output.status.code() else {
            return Err(format!(
                "tsc terminated without an exit code for {name}:\n{diagnostics}"
            ));
        };
        let target_name = format!("{name}.{extension}");
        let analyzed_target = diagnostics.lines().any(|line| {
            Path::new(line.trim())
                .file_name()
                .is_some_and(|file_name| file_name == target_name.as_str())
        });

        let mut parsed_diagnostics = Vec::new();
        for line in diagnostics.lines() {
            let Some(code_start) = line.find("error TS") else {
                continue;
            };
            let digits = line[code_start + "error TS".len()..]
                .chars()
                .take_while(char::is_ascii_digit)
                .collect::<String>();
            let diagnostic_code = digits.parse::<u32>().map_err(|_| {
                format!("unparseable tsc diagnostic for {name}: {line}\n{diagnostics}")
            })?;
            parsed_diagnostics.push((diagnostic_code, line));
        }

        let rejected = parsed_diagnostics
            .iter()
            .filter(|(diagnostic_code, _)| !matches!(diagnostic_code, 2307 | 7026))
            .map(|(_, line)| *line)
            .collect::<Vec<_>>();
        if !analyzed_target
            || !rejected.is_empty()
            || (exit_code != 0 && parsed_diagnostics.is_empty())
        {
            let failure_output = if rejected.is_empty() {
                diagnostics.clone()
            } else {
                rejected.join("\n")
            };
            return Err(format!(
                "tsc did not complete a clean syntax analysis for {name} (exit {exit_code}, analyzed_target={analyzed_target}):\n{}\n--- emitted ---\n{code}",
                failure_output
            ));
        }
        Ok(())
    }

    fn assert_typescript_syntax_valid(name: &str, code: &str, is_jsx: bool) {
        typescript_syntax_check(
            Path::new("node"),
            &typescript_launcher(),
            name,
            code,
            is_jsx,
            &[],
        )
        .unwrap_or_else(|error| panic!("{error}"));
    }

    fn javascript_module_check(node_program: &Path, name: &str, code: &str) -> Result<(), String> {
        let project = tempfile::tempdir().expect("create JS validity directory");
        let path = project.path().join(format!("{name}.mjs"));
        fs::write(&path, code).expect("write emitted module");
        let output = Command::new(node_program)
            .arg("--check")
            .arg(&path)
            .current_dir(project.path())
            .output()
            .map_err(|error| format!("failed to run node syntax gate: {error}"))?;
        println!("NODE_CHECK {name} EXIT={:?}", output.status.code());
        let Some(exit_code) = output.status.code() else {
            return Err(format!(
                "node --check terminated without an exit code for {name}"
            ));
        };
        if exit_code != 0 {
            return Err(format!(
                "node --check rejected {name}:\n{}{}\n--- emitted ---\n{code}",
                String::from_utf8_lossy(&output.stdout),
                String::from_utf8_lossy(&output.stderr)
            ));
        }
        Ok(())
    }

    fn assert_javascript_module_valid(name: &str, code: &str) {
        javascript_module_check(Path::new("node"), name, code)
            .unwrap_or_else(|error| panic!("{error}"));
    }

    /// @ai-generated - Negative control for the real TypeScript gate: JSX
    /// closing-tag syntax must be rejected rather than treated as environment noise.
    #[test]
    fn typescript_syntax_gate_rejects_jsx_mismatched_closing_tag() {
        let rejected = typescript_syntax_check(
            Path::new("node"),
            &typescript_launcher(),
            "mismatched_jsx",
            "const view = <Foo><Bar></Foo>;\n",
            false,
            &[],
        );
        assert!(rejected.is_err(), "the gate accepted malformed JSX");
    }

    /// @ai-generated - Execution and malformed-output controls for the tsc gate.
    #[test]
    fn typescript_syntax_gate_proves_execution_and_rejects_invalid_output() {
        let launcher = typescript_launcher();
        let invalid = typescript_syntax_check(
            Path::new("node"),
            &launcher,
            "invalid_emitted_fixture",
            "export const value = ;\n",
            false,
            &[],
        );
        assert!(invalid.is_err(), "the gate accepted invalid emitted TSX");

        let non_invocation = typescript_syntax_check(
            Path::new("node"),
            &launcher,
            "unknown_option",
            "export const value = 1;\n",
            false,
            &["--ignoreConfigZZ"],
        )
        .expect_err("an unknown option must not pass without analysing the fixture");
        assert!(
            non_invocation.contains("TS5023") || non_invocation.contains("analyzed_target=false"),
            "unexpected non-invocation evidence: {non_invocation}"
        );

        let missing_binary = workspace_root().join("__missing_node_for_tsc_validity_gate__");
        assert!(
            typescript_syntax_check(
                &missing_binary,
                &launcher,
                "missing_binary",
                "export const value = 1;\n",
                false,
                &[],
            )
            .is_err(),
            "a missing node binary passed the tsc gate"
        );
    }

    /// @ai-generated - Execution and malformed-output controls for node --check.
    #[test]
    fn javascript_syntax_gate_proves_execution_and_rejects_invalid_output() {
        assert!(
            javascript_module_check(
                Path::new("node"),
                "invalid_emitted_fixture",
                "export const value = ;\n",
            )
            .is_err(),
            "node --check accepted an invalid emitted module"
        );

        let missing_binary = workspace_root().join("__missing_node_for_js_validity_gate__");
        assert!(
            javascript_module_check(&missing_binary, "missing_binary", "export {};\n").is_err(),
            "a missing node binary passed the node --check gate"
        );
    }

    fn artifact_for(source: &str) -> Arc<FrameworkParseArtifact> {
        use verter_language::carrier_grammar::{
            CarrierGrammarAuthority, CarrierGrammarConfig, CarrierParserGrammarVersion,
            FrameworkAdapterSemanticVersion,
        };
        use verter_language::registered_source_authority::{
            CanonicalFileId, FileIncarnation, RegisteredSourceAuthority, SourceGeneration,
        };
        let source_authority = RegisteredSourceAuthority::new().unwrap();
        let grammar_authority = CarrierGrammarAuthority::new().unwrap();
        let config = CarrierGrammarConfig::vue("{{", "}}", std::iter::empty::<&str>()).unwrap();
        grammar_authority
            .register_carrier_grammar(
                verter_language::FileLanguage::vue(),
                FrameworkAdapterSemanticVersion::new(1).unwrap(),
                CarrierParserGrammarVersion::new(1).unwrap(),
                config.clone(),
            )
            .unwrap();
        let snapshot = source_authority
            .register_source(
                CanonicalFileId::new("file:///fixture.vue"),
                FileIncarnation::new(1),
                SourceGeneration::new(1),
                verter_language::FileLanguage::vue(),
                Arc::from(source),
            )
            .unwrap();
        let accepted = grammar_authority
            .accept_registered_source(&source_authority, &snapshot, &config)
            .unwrap();
        Arc::new(
            crate::framework_common::CarrierCompilerRegistry::built_in()
                .project_registered(&accepted)
                .expect("fixture source parses")
                .into_framework_parse_artifact(),
        )
    }

    fn expected_parse_key(source: &str) -> verter_language::ParseKey {
        let language = FileLanguage::vue();
        // `artifact_for` registers the fixture with the real standard `{{`
        // / `}}` grammar (see `CarrierGrammarConfig::vue` above) — match
        // that here rather than `ParseOptions::default()`, which no longer
        // means "Vue's standard delimiters" (it means "the caller supplied
        // nothing", an empty value distinct from any real request).
        let profile = syntax_profile_id_for(&language, &ParseOptions::vue_standard()).unwrap();
        parse_key_for(
            source,
            &language,
            VUE_SYNTAX_COMPATIBILITY_DOMAIN,
            VUE_SYNTAX_COMPATIBILITY_EPOCH,
            &profile,
        )
        .unwrap()
    }

    fn projected_script(code: &str, lang: &str) -> RuntimeBlockContentInput {
        RuntimeBlockContentInput {
            code: Arc::from(code),
            source_map: None,
            lang: lang.to_string(),
            content_artifact_token: format!("content:{lang}"),
            source_space_token: format!("space:{lang}"),
            parsed: None,
            producer: None,
        }
    }

    #[test]
    fn two_projected_script_spaces_remain_typed_unavailable() {
        let source = concat!(
            "<script src=\"./logic.js\"></script>",
            "<script setup src=\"./setup.js\"></script>",
            "<template><div /></template>"
        );
        let compiler = VueCarrierCompiler;
        let artifact = artifact_for(source);
        let alloc = oxc_allocator::Allocator::new();
        let result = compiler.compile_bundle_expect_produced(
            source,
            &artifact,
            &RuntimeCompileOptions {
                block_content: RuntimeBlockContentInputs {
                    script: Some(projected_script("export default {}", "js")),
                    script_setup: Some(projected_script("const answer = 42", "js")),
                    ..Default::default()
                },
                ..Default::default()
            },
            &alloc,
        );

        assert!(matches!(
            result,
            Err(CompileUnsupported::BlockContentRuntimeUnavailable { .. })
        ));
    }

    #[test]
    fn compile_bundle_runtime_branch_delegates_to_the_typed_runtime_backend() {
        // The bundle route reaches its runtime product only through the typed
        // Vue runtime backend; the per-thread delegation counter is the
        // witness that no route-private runtime emitter remains.
        let compiler = VueCarrierCompiler;
        let source = "<script setup>const n = 1</script><template><div>{{ n }}</div></template>";
        let artifact = artifact_for(source);
        let alloc = oxc_allocator::Allocator::new();

        let before = crate::standalone::runtime_backend_delegation_count();
        compiler
            .compile_bundle_expect_produced(
                source,
                &artifact,
                &RuntimeCompileOptions::default(),
                &alloc,
            )
            .expect("vue runtime bundle");
        assert_eq!(
            crate::standalone::runtime_backend_delegation_count(),
            before + 1,
            "the bundle runtime branch must delegate exactly once to the typed Vue runtime backend"
        );

        let before_ide_only = crate::standalone::runtime_backend_delegation_count();
        compiler
            .compile_bundle_expect_produced(
                source,
                &artifact,
                &RuntimeCompileOptions {
                    want_runtime: false,
                    want_ide: true,
                    ..Default::default()
                },
                &alloc,
            )
            .expect("vue ide-only bundle");
        assert_eq!(
            crate::standalone::runtime_backend_delegation_count(),
            before_ide_only,
            "a bundle request planning no runtime product must not touch the runtime backend"
        );
    }

    /// @ai-generated - Proves external template lowering receives the inline
    /// setup binding metadata instead of resolving an imported component by name.
    #[test]
    fn projected_template_and_inline_setup_share_runtime_bindings() {
        let source = concat!(
            "<template src=\"./view.html\"></template>",
            "<script setup>import Foo from './Foo.vue'</script>"
        );
        let compiler = VueCarrierCompiler;
        let artifact = artifact_for(source);
        let alloc = oxc_allocator::Allocator::new();
        let output = compiler
            .compile_bundle_expect_produced(
                source,
                &artifact,
                &RuntimeCompileOptions {
                    filename: Some("Projected.vue".to_string()),
                    block_content: RuntimeBlockContentInputs {
                        template: Some(projected_script("<Foo />", "html")),
                        ..Default::default()
                    },
                    ..Default::default()
                },
                &alloc,
            )
            .expect("external template has a truthful single-source runtime path");
        let template = output.template.expect("standalone render output");

        assert!(
            !template
                .imports
                .iter()
                .any(|name| name.contains("resolveComponent")),
            "a setup import must not degrade to runtime name resolution: {:?}\n{}",
            template.imports,
            template.code
        );
        assert!(
            !template.code.contains("_component_Foo")
                && (template.code.contains("$setup.Foo")
                    || template.code.contains("$setup[\"Foo\"]")
                    || template.code.contains("$setup['Foo']")),
            "the render must address the transferred setup binding:\n{}",
            template.code
        );
        assert_eq!(
            template.output_descriptor.source_map.declared_space_tokens,
            vec!["space:html".to_string()],
            "the standalone render map belongs to the selected template source space"
        );
    }

    #[test]
    fn two_projected_script_spaces_keep_ide_typed_unavailable() {
        let source = concat!(
            "<script src=\"./logic.js\"></script>",
            "<script setup src=\"./setup.js\"></script>"
        );
        let compiler = VueCarrierCompiler;
        let artifact = artifact_for(source);
        let alloc = oxc_allocator::Allocator::new();
        let result = compiler.compile_bundle_expect_produced(
            source,
            &artifact,
            &RuntimeCompileOptions {
                filename: Some("Projected.vue".to_string()),
                want_ide: true,
                block_content: RuntimeBlockContentInputs {
                    script: Some(projected_script("export default {}", "js")),
                    script_setup: Some(projected_script("const answer = 42", "js")),
                    ..Default::default()
                },
                ..Default::default()
            },
            &alloc,
        );

        assert!(matches!(
            result,
            Err(CompileUnsupported::BlockContentIdeUnavailable { .. })
        ));
    }

    #[test]
    fn runtime_leg_is_catalog_selected_a_catalog_miss_refuses_typed() {
        // The runtime leg must be selected from the immutable catalog at
        // execution, exactly like the projection and template-fact legs: an
        // artifact whose epoch has no registered runtime row refuses typed
        // instead of compiling through a concretely-constructed backend.
        let source = "<script setup>const n = 1</script><template><div>{{ n }}</div></template>";
        let artifact = artifact_for(source).remint_epoch_for_tests("unknown-epoch");
        let alloc = oxc_allocator::Allocator::new();
        let before = crate::standalone::runtime_backend_delegation_count();
        let result = VueCarrierCompiler.compile_bundle(
            source,
            &artifact,
            &RuntimeCompileOptions {
                want_runtime: true,
                ..Default::default()
            },
            &alloc,
        );
        assert!(
            matches!(
                result,
                Err(CompileUnsupported::RequestExecutionRefused(
                    CompileRequestError::CapabilityUnsupported(_)
                ))
            ),
            "a runtime-catalog miss must refuse typed, got {result:?}"
        );
        assert_eq!(
            crate::standalone::runtime_backend_delegation_count(),
            before,
            "no runtime backend may execute on a catalog miss"
        );
    }

    #[test]
    fn compile_bundle_refuses_explicit_ssr_and_force_vapor() {
        // SSR requested together with an explicit Vapor backend — RC.3
        // defines no Cartesian Vapor server compiler backend. This trait
        // method is a SEPARATE production entry into the shared codegen
        // substrate from `CompileRequest::new` (the session's per-file
        // compile path routes here without constructing a `CompileRequest`
        // first); without this guard the combination would reach codegen
        // and produce wrong output instead of a typed refusal.
        let source = "<template><div>{{ a }}</div></template>";
        let compiler = VueCarrierCompiler;
        let artifact = artifact_for(source);
        let alloc = oxc_allocator::Allocator::new();
        let result = compiler.compile_bundle(
            source,
            &artifact,
            &RuntimeCompileOptions {
                filename: Some("SsrVapor.vue".to_string()),
                want_runtime: true,
                ssr: true,
                force_vapor: true,
                ..Default::default()
            },
            &alloc,
        );
        assert!(
            matches!(
                result,
                Err(CompileUnsupported::RequestExecutionRefused(
                    CompileRequestError::SsrVaporBackendUnsupported
                ))
            ),
            "expected a typed SsrVaporBackendUnsupported refusal, got {result:?}"
        );
    }

    #[test]
    fn compile_bundle_refuses_implicit_vapor_marker_with_ssr() {
        // The implicit half of the same rule: the source's own `<template
        // vapor>` marker (not an explicit `force_vapor` request) resolves
        // to a Vapor backend just as surely, and must refuse identically —
        // mirrors `CompileRequest::resolve_vue_backend`'s post-parse check.
        let source = "<template vapor><div>{{ a }}</div></template>";
        let compiler = VueCarrierCompiler;
        let artifact = artifact_for(source);
        let alloc = oxc_allocator::Allocator::new();
        let result = compiler.compile_bundle(
            source,
            &artifact,
            &RuntimeCompileOptions {
                filename: Some("ImplicitVaporSsr.vue".to_string()),
                want_runtime: true,
                ssr: true,
                ..Default::default()
            },
            &alloc,
        );
        assert!(
            matches!(
                result,
                Err(CompileUnsupported::RequestExecutionRefused(
                    CompileRequestError::SsrVaporBackendUnsupported
                ))
            ),
            "expected a typed SsrVaporBackendUnsupported refusal for the implicit marker, got {result:?}"
        );
    }

    #[test]
    fn compile_bundle_refuses_inline_with_ssr() {
        // Inline assembly requested together with SSR — the SSR capability
        // has no inline axis; must never be silently demoted to non-inline.
        let source = concat!(
            "<script setup>const a = 1</script>",
            "<template><div>{{ a }}</div></template>"
        );
        let compiler = VueCarrierCompiler;
        let artifact = artifact_for(source);
        let alloc = oxc_allocator::Allocator::new();
        let result = compiler.compile_bundle(
            source,
            &artifact,
            &RuntimeCompileOptions {
                filename: Some("InlineSsr.vue".to_string()),
                want_runtime: true,
                ssr: true,
                inline: Some(true),
                ..Default::default()
            },
            &alloc,
        );
        assert!(
            matches!(
                result,
                Err(CompileUnsupported::RequestExecutionRefused(
                    CompileRequestError::InlineSsrUnsupported
                ))
            ),
            "expected a typed InlineSsrUnsupported refusal, got {result:?}"
        );
    }

    #[test]
    fn compile_bundle_permits_ssr_without_vapor_or_inline() {
        // Negative control for the two guards above: SSR alone (no Vapor,
        // no explicit inline) must still produce a runtime bundle — the
        // guards are precise to the Cartesian combination, not a blanket
        // SSR refusal.
        let source = "<template><div>{{ a }}</div></template>";
        let compiler = VueCarrierCompiler;
        let artifact = artifact_for(source);
        let alloc = oxc_allocator::Allocator::new();
        let bundle = compiler
            .compile_bundle_expect_produced(
                source,
                &artifact,
                &RuntimeCompileOptions {
                    filename: Some("SsrOnly.vue".to_string()),
                    want_runtime: true,
                    ssr: true,
                    ..Default::default()
                },
                &alloc,
            )
            .expect("SSR alone must still compile");
        assert!(bundle.script.is_some() || bundle.template.is_some());
    }

    /// The map-coupling regression, proven through the real production
    /// route (`compile_bundle` itself — not the `CompileRequest`
    /// predicate-level unit test in `compile_request/mod.rs`, which only
    /// proves `wants_runtime_source_map()`'s own logic in isolation and
    /// never actually runs a compile). Requesting BOTH the runtime AND the
    /// IDE product with `source_map: false` must produce a runtime script
    /// with NO source map — the pre-fix bug computed
    /// `source_map: opts.source_map || opts.want_ide`, which would
    /// silently turn the runtime map ON here purely because `want_ide` was
    /// set, even though the caller asked for no map at all.
    #[test]
    fn compile_bundle_does_not_force_the_runtime_map_on_merely_because_ide_is_requested() {
        let source = concat!(
            "<script setup>const count = 1</script>",
            "<template><div>{{ count }}</div></template>"
        );
        let compiler = VueCarrierCompiler;
        let artifact = artifact_for(source);
        let alloc = oxc_allocator::Allocator::new();
        let bundle = compiler
            .compile_bundle_expect_produced(
                source,
                &artifact,
                &RuntimeCompileOptions {
                    filename: Some("NoMap.vue".to_string()),
                    want_runtime: true,
                    want_ide: true,
                    source_map: false,
                    ..Default::default()
                },
                &alloc,
            )
            .expect("runtime + IDE compile without a requested map still produces both outputs");
        let script = bundle.script.expect("runtime script block");
        assert!(
            script.source_map.is_empty(),
            "requesting the IDE product must NOT force the runtime script's own \
             source map on when source_map=false was requested; got: {:?}",
            script.source_map
        );
        let tsx = bundle.tsx.expect("IDE tsx block");
        assert!(
            tsx.source_map.is_empty(),
            "the IDE map must independently honor source_map=false too; got: {:?}",
            tsx.source_map
        );
    }

    /// The companion positive control: `source_map: true` with BOTH
    /// products requested must populate BOTH maps independently — proving
    /// the decoupling is precise (neither field is a dead flag), not just
    /// that both happen to stay off together.
    #[test]
    fn compile_bundle_populates_both_maps_independently_when_both_are_requested() {
        let source = concat!(
            "<script setup>const count = 1</script>",
            "<template><div>{{ count }}</div></template>"
        );
        let compiler = VueCarrierCompiler;
        let artifact = artifact_for(source);
        let alloc = oxc_allocator::Allocator::new();
        let bundle = compiler
            .compile_bundle_expect_produced(
                source,
                &artifact,
                &RuntimeCompileOptions {
                    filename: Some("BothMaps.vue".to_string()),
                    want_runtime: true,
                    want_ide: true,
                    source_map: true,
                    ..Default::default()
                },
                &alloc,
            )
            .expect("runtime + IDE compile with a requested map produces both outputs");
        let script = bundle.script.expect("runtime script block");
        assert!(
            !script.source_map.is_empty(),
            "source_map=true must populate the runtime script's own map"
        );
        let tsx = bundle.tsx.expect("IDE tsx block");
        assert!(
            !tsx.source_map.is_empty(),
            "source_map=true must independently populate the IDE map too"
        );
    }

    #[test]
    fn projected_plain_script_runtime_is_typed_unavailable_until_its_module_is_valid() {
        let source = concat!(
            "<script src=\"./logic.js\"></script>",
            "<template><div>{{ a }}</div></template>"
        );
        let compiler = VueCarrierCompiler;
        let alloc = oxc_allocator::Allocator::new();
        let result = compiler.compile_bundle_expect_produced(
            source,
            &artifact_for(source),
            &RuntimeCompileOptions {
                filename: Some("ProjectedPlain.vue".to_string()),
                block_content: RuntimeBlockContentInputs {
                    script: Some(projected_script(
                        "export default { data: () => ({ a: 1 }) }",
                        "js",
                    )),
                    ..Default::default()
                },
                ..Default::default()
            },
            &alloc,
        );

        assert!(matches!(
            result,
            Err(CompileUnsupported::BlockContentRuntimeUnavailable { .. })
        ));
    }

    /// @ai-generated - A single IDE surface may merge carrier script bindings
    /// with a projected template only when both source spaces remain declared.
    #[test]
    fn projected_template_ide_composes_two_source_spaces() {
        let source = concat!(
            "<template src=\"./view.html\"></template>",
            "<script setup>const count = 1</script>"
        );
        let compiler = VueCarrierCompiler;
        let artifact = artifact_for(source);
        let alloc = oxc_allocator::Allocator::new();
        let output = compiler
            .compile_bundle_expect_produced(
                source,
                &artifact,
                &RuntimeCompileOptions {
                    filename: Some("Projected.vue".to_string()),
                    source_map: true,
                    want_ide: true,
                    block_content: RuntimeBlockContentInputs {
                        template: Some(projected_script("<div>{{ count }}</div>", "html")),
                        ..Default::default()
                    },
                    ..Default::default()
                },
                &alloc,
            )
            .expect("two-source IDE lowering has a composed-map path");
        let ide = output.tsx.expect("IDE output");

        assert!(
            ide.code.contains("count = 1"),
            "script chunk missing:\n{}",
            ide.code
        );
        assert!(
            ide.code.contains("{ count }"),
            "template chunk missing:\n{}",
            ide.code
        );
        assert_eq!(
            ide.output_descriptor.source_space.kind,
            OutputSourceSpaceKind::GeneratedComposite
        );
        assert_eq!(
            ide.output_descriptor.source_map.fidelity,
            SourceMapFidelity::Approximate
        );
        assert_eq!(
            ide.output_descriptor.source_map.declared_space_tokens.len(),
            2
        );
        let raw_map = ide
            .output_descriptor
            .source_map
            .raw_map
            .as_deref()
            .expect("composed map");
        assert!(
            raw_map.contains("space:html"),
            "external source absent from map: {raw_map}"
        );
    }

    /// @ai-generated - The Options API script geometry places an external
    /// template fragment mid-module, so both compiler IDE entry points must fail closed.
    #[test]
    fn external_template_with_plain_script_is_ide_typed_unavailable() {
        let source = concat!(
            "<template src=\"./view.html\"></template>",
            "<script>export default { data: () => ({ count: 1 }) }</script>"
        );
        let compiler = VueCarrierCompiler;
        let artifact = artifact_for(source);
        let block_content = RuntimeBlockContentInputs {
            template: Some(projected_script("<div>{{ count }}</div>", "html")),
            ..Default::default()
        };
        let alloc = oxc_allocator::Allocator::new();

        let bundle = compiler.compile_bundle_expect_produced(
            source,
            &artifact,
            &RuntimeCompileOptions {
                filename: Some("ExternalPlain.vue".to_string()),
                source_map: true,
                want_ide: true,
                block_content: block_content.clone(),
                ..Default::default()
            },
            &alloc,
        );
        assert!(matches!(
            bundle,
            Err(CompileUnsupported::BlockContentIdeUnavailable { .. })
        ));

        let direct = compiler.compile_ide(
            source,
            &artifact,
            &IdeCompileOptions {
                filename: Some("ExternalPlain.vue".to_string()),
                block_content,
                ..Default::default()
            },
        );
        assert!(matches!(
            direct,
            Err(CompileUnsupported::BlockContentIdeUnavailable { .. })
        ));
    }

    #[test]
    fn direct_compile_ide_accepts_the_block_content_channel() {
        let source = concat!(
            "<template src=\"./view.html\"></template>",
            "<script setup>const count = 1</script>"
        );
        let compiler = VueCarrierCompiler;
        let output = compiler
            .compile_ide(
                source,
                &artifact_for(source),
                &IdeCompileOptions {
                    filename: Some("Direct.vue".to_string()),
                    block_content: RuntimeBlockContentInputs {
                        template: Some(projected_script("<p>{{ count }}</p>", "html")),
                        ..Default::default()
                    },
                    ..Default::default()
                },
            )
            .expect("direct IDE block-content lowering");
        assert!(output.code.contains("{ count }"));
        assert_eq!(
            output
                .output_descriptor
                .source_map
                .declared_space_tokens
                .len(),
            2
        );
    }

    /// @ai-generated - Supplied plain CSS is stage-two input and keeps its external source space.
    #[test]
    fn supplied_external_style_is_scoped_in_its_own_source_space() {
        let source =
            "<template><div class=\"x\"/></template><style scoped src=\"./theme.css\"></style>";
        let compiler = VueCarrierCompiler;
        let alloc = oxc_allocator::Allocator::new();
        let output = compiler
            .compile_bundle_expect_produced(
                source,
                &artifact_for(source),
                &RuntimeCompileOptions {
                    filename: Some("ExternalStyle.vue".to_string()),
                    component_id: Some("scope123".to_string()),
                    source_map: true,
                    block_content: RuntimeBlockContentInputs {
                        styles: vec![Some(RuntimeBlockContentInput {
                            code: Arc::from(".x { color: red; }"),
                            source_map: None,
                            lang: "css".to_string(),
                            content_artifact_token: "artifact:theme-css".to_string(),
                            source_space_token: "space:theme-css".to_string(),
                            parsed: None,
                            producer: None,
                        })],
                        ..Default::default()
                    },
                    ..Default::default()
                },
                &alloc,
            )
            .expect("supplied external style compiles");
        let style = &output.styles[0];

        assert!(style.code.contains(".x[data-v-scope123]"), "{}", style.code);
        assert_eq!(style.lang.as_deref(), Some("css"));
        assert_eq!(
            style.output_descriptor.source_map.declared_space_tokens,
            vec!["space:theme-css"]
        );
        assert_ne!(
            style.output_descriptor.source_space.token,
            "space:theme-css"
        );
    }

    /// Who made the selected bytes decides which stage the cascade records
    /// them at — not a comparison between the authored and selected dialects.
    ///
    /// The block here is authored plain CSS AND the tool published plain CSS,
    /// so the two dialects are equal and a dialect comparison answers "nobody
    /// preprocessed this". The bytes are then recorded as the carrier's own
    /// authored content and the tool that actually made them disappears from
    /// the record — the provenance loss this routing exists to prevent. Only
    /// the host knows, and it says so through the producer.
    ///
    /// The supplied route is fail-closed: it consumes the artifact the host
    /// admitted rather than re-parsing bytes it did not make. Withholding that
    /// artifact is therefore what makes the branch observable from outside —
    /// under a dialect comparison this same input compiles as authored CSS.
    #[test]
    fn an_external_tools_output_over_a_plain_css_block_takes_the_supplied_route() {
        let source = concat!(
            "<template><div class=\"a\"/></template>",
            "<style scoped>.a { color: red; }</style>"
        );
        let css = ".a { color: red; }";
        let compile = |parsed: Option<crate::style_planner::PreparedStyleIr>| {
            let compiler = VueCarrierCompiler;
            let alloc = oxc_allocator::Allocator::new();
            compiler.compile_bundle(
                source,
                &artifact_for(source),
                &RuntimeCompileOptions {
                    filename: Some("PlainSupplied.vue".to_string()),
                    component_id: Some("scope123".to_string()),
                    block_content: RuntimeBlockContentInputs {
                        styles: vec![Some(RuntimeBlockContentInput {
                            code: Arc::from(css),
                            source_map: None,
                            lang: "css".to_string(),
                            content_artifact_token: "artifact:supplied-css".to_string(),
                            source_space_token: "space:supplied-css".to_string(),
                            parsed,
                            producer: Some(supplied_producer()),
                        })],
                        ..Default::default()
                    },
                    ..Default::default()
                },
                &alloc,
            )
        };

        let CarrierCompileOutcome::Produced(output) =
            compile(Some(supplied_style(css))).expect("an admitted external artifact compiles")
        else {
            panic!("this fixture produces a runtime surface");
        };
        assert!(
            output.styles[0].code.contains(".a[data-v-scope123]"),
            "the supplied route still runs every plain-CSS stage: {}",
            output.styles[0].code
        );

        match compile(None) {
            Err(CompileUnsupported::BlockContentRuntimeUnavailable { .. }) => {}
            other => panic!(
                "an external producer's bytes with no admitted artifact must refuse rather than \
                 be re-read as the carrier's own authored content, got {other:?}"
            ),
        }
    }

    /// @ai-generated - Unknown selected style lang must refuse, not rewrite as CSS.
    #[test]
    fn unknown_selected_style_lang_refuses_css_cascade_rewrite() {
        let source = concat!(
            "<template><div class=\"a\"/></template>",
            "<style lang=\"postcss\" scoped>.a { color: red; }</style>"
        );
        let prepared = supplied_style(".a { color: red; }");
        let compiler = VueCarrierCompiler;
        let alloc = oxc_allocator::Allocator::new();
        let result = compiler.compile_bundle(
            source,
            &artifact_for(source),
            &RuntimeCompileOptions {
                filename: Some("UnknownDialect.vue".to_string()),
                component_id: Some("scope123".to_string()),
                block_content: RuntimeBlockContentInputs {
                    styles: vec![Some(RuntimeBlockContentInput {
                        code: Arc::from(".a { color: red; }"),
                        source_map: None,
                        lang: "postcss".to_string(),
                        content_artifact_token: "artifact:postcss".to_string(),
                        source_space_token: "space:postcss".to_string(),
                        parsed: Some(prepared),
                        producer: Some(supplied_producer()),
                    })],
                    ..Default::default()
                },
                ..Default::default()
            },
            &alloc,
        );
        match result {
            Err(CompileUnsupported::BlockContentRuntimeUnavailable { .. }) => {}
            Ok(CarrierCompileOutcome::Produced(output)) => panic!(
                "unknown selected lang must not produce a CSS cascade rewrite: {}",
                output.styles[0].code
            ),
            other => panic!("expected BlockContentRuntimeUnavailable, got {other:?}"),
        }
    }

    /// @ai-generated - Sequential style stages must not claim an uncomposed exact map.
    #[test]
    fn sequential_external_style_rewrites_report_approximate_map_fidelity() {
        let source =
            "<template><div class=\"x\"/></template><style scoped src=\"./theme.css\"></style>";
        let compiler = VueCarrierCompiler;
        let alloc = oxc_allocator::Allocator::new();
        let output = compiler
            .compile_bundle_expect_produced(
                source,
                &artifact_for(source),
                &RuntimeCompileOptions {
                    filename: Some("ExternalStyle.vue".to_string()),
                    component_id: Some("scope123".to_string()),
                    source_map: true,
                    block_content: RuntimeBlockContentInputs {
                        styles: vec![Some(RuntimeBlockContentInput {
                            code: Arc::from(".x { color: v-bind(tone); }"),
                            source_map: None,
                            lang: "css".to_string(),
                            content_artifact_token: "artifact:theme-css".to_string(),
                            source_space_token: "space:theme-css".to_string(),
                            parsed: None,
                            producer: None,
                        })],
                        ..Default::default()
                    },
                    ..Default::default()
                },
                &alloc,
            )
            .expect("supplied external style compiles");
        let style = &output.styles[0];

        assert!(
            style.code.contains("var(--scope123-tone)"),
            "{}",
            style.code
        );
        assert!(style.code.contains(".x[data-v-scope123]"), "{}", style.code);
        assert_eq!(
            style.output_descriptor.source_map.fidelity,
            SourceMapFidelity::Approximate
        );
    }

    /// @ai-generated - A10i must hold for the "supplied-preprocessor-output"
    /// branch too (authored SCSS whose supplied bytes are already-compiled
    /// plain CSS, so the authored-v-bind stage does not apply): the modules
    /// stage produces no edits here (no class selector to hash), so its
    /// retained IR must hand straight into the scoped-selector stage
    /// instead of forcing a second parse. Proven against the OLD
    /// two-independent-calls pattern run directly on the same fixture, not
    /// just the new entry point's absolute count.
    #[test]
    fn supplied_preprocessor_output_reuses_parsed_ir_across_modules_and_scoped() {
        use crate::style_planner::{
            parse_ir_invocation_count, reset_parse_ir_invocation_count, transform_vue_css_modules,
            transform_vue_scoped_css, PlainCssInput, StyleRewriteOutcome,
        };

        let plain_input = || {
            PlainCssInput::try_new(
                "body { color: red; }",
                CssDialect::Css,
                "theme.css",
                "space:theme-scss",
                "artifact:theme-scss",
            )
            .expect("plain css")
        };

        // The old per-stage-independent pattern this branch used to run:
        // each call parses its own input, even though the modules stage
        // produces no edits.
        reset_parse_ir_invocation_count();
        let modules_outcome =
            transform_vue_css_modules(plain_input(), "scope123").expect("modules stage");
        let after_modules = match modules_outcome {
            StyleRewriteOutcome::Unchanged { .. } => "body { color: red; }".to_string(),
            StyleRewriteOutcome::Rewritten { code, .. } => code,
        };
        let scoped_plain = PlainCssInput::try_new(
            &after_modules,
            CssDialect::Css,
            "theme.css",
            "space:theme-scss",
            "artifact:theme-scss",
        )
        .expect("plain css");
        let _ = transform_vue_scoped_css(scoped_plain, "scope123").expect("scoped stage");
        assert_eq!(
            parse_ir_invocation_count(),
            2,
            "running modules and scoped independently re-parses the \
             unchanged hand-off, costing 2 even though the modules stage \
             produced no edits"
        );

        // The real production branch (already-preprocessed CSS, module +
        // scoped) must cost exactly 1.
        let source = concat!(
            "<template><div/></template>",
            "<style module scoped lang=\"scss\" src=\"./theme.scss\"></style>"
        );
        let compiler = VueCarrierCompiler;
        let alloc = oxc_allocator::Allocator::new();
        reset_parse_ir_invocation_count();
        let prepared = supplied_style("body { color: red; }");
        let output = compiler
            .compile_bundle_expect_produced(
                source,
                &artifact_for(source),
                &RuntimeCompileOptions {
                    filename: Some("Theme.vue".to_string()),
                    component_id: Some("scope123".to_string()),
                    source_map: true,
                    block_content: RuntimeBlockContentInputs {
                        styles: vec![Some(RuntimeBlockContentInput {
                            code: Arc::from("body { color: red; }"),
                            source_map: None,
                            lang: "css".to_string(),
                            content_artifact_token: "artifact:theme-scss".to_string(),
                            source_space_token: "space:theme-scss".to_string(),
                            parsed: Some(prepared),
                            producer: Some(supplied_producer()),
                        })],
                        ..Default::default()
                    },
                    ..Default::default()
                },
                &alloc,
            )
            .expect("supplied preprocessor output compiles");
        assert_eq!(
            parse_ir_invocation_count(),
            1,
            "an unchanged modules stage must hand its retained IR into \
             scoping, not force a second parse"
        );
        let style = &output.styles[0];
        assert!(
            style.code.contains("body[data-v-scope123]"),
            "{}",
            style.code
        );
    }

    #[test]
    fn prepared_ir_does_not_join_by_source_text() {
        let source = concat!(
            "<template><div/></template>",
            "<style module scoped lang=\"scss\" src=\"./a.scss\"></style>",
            "<style module scoped lang=\"scss\" src=\"./b.scss\"></style>",
        );
        let css = ".card { color: red; }";
        let prepared = supplied_style(css);
        let compiler = VueCarrierCompiler;
        let alloc = oxc_allocator::Allocator::new();
        let err = compiler
            .compile_bundle(
                source,
                &artifact_for(source),
                &RuntimeCompileOptions {
                    filename: Some("Two.vue".to_string()),
                    component_id: Some("scope123".to_string()),
                    block_content: RuntimeBlockContentInputs {
                        styles: vec![
                            Some(RuntimeBlockContentInput {
                                code: Arc::from(css),
                                source_map: None,
                                lang: "css".to_string(),
                                content_artifact_token: "artifact:a".to_string(),
                                source_space_token: "space:a".to_string(),
                                parsed: None,
                                producer: Some(supplied_producer()),
                            }),
                            Some(RuntimeBlockContentInput {
                                code: Arc::from(css),
                                source_map: None,
                                lang: "css".to_string(),
                                content_artifact_token: "artifact:b".to_string(),
                                source_space_token: "space:b".to_string(),
                                parsed: None,
                                producer: Some(supplied_producer()),
                            }),
                        ],
                        ..Default::default()
                    },
                    prepared_styles: vec![Some(prepared), None],
                    ..Default::default()
                },
                &alloc,
            )
            .expect_err("second block must fail closed, not steal the first IR by text");
        assert!(
            matches!(
                err,
                CompileUnsupported::BlockContentRuntimeUnavailable { .. }
            ),
            "{err:?}"
        );
    }

    /// @ai-generated - Supplied output for an authored inline template keeps
    /// Vue's production setup-return topology and declares both source spaces.
    #[test]
    fn supplied_inline_template_composes_into_production_script() {
        let source = concat!(
            "<template lang=\"pug\">div {{ count }}</template>",
            "<script setup>const count = 1</script>"
        );
        let compiler = VueCarrierCompiler;
        let alloc = oxc_allocator::Allocator::new();
        let output = compiler
            .compile_bundle_expect_produced(
                source,
                &artifact_for(source),
                &RuntimeCompileOptions {
                    filename: Some("Supplied.vue".to_string()),
                    is_production: true,
                    source_map: true,
                    block_content: RuntimeBlockContentInputs {
                        template: Some(projected_script("<div>{{ count }}</div>", "html")),
                        ..Default::default()
                    },
                    ..Default::default()
                },
                &alloc,
            )
            .expect("supplied inline template has a generated-chunk path");

        assert!(
            output.inline,
            "production supplied-inline topology was lost"
        );
        assert!(
            output.template.is_none(),
            "inline topology emitted a detached render"
        );
        let script = output.script.expect("composed runtime script");
        assert!(script.code.contains("const count = 1"));
        assert!(
            script.code.contains("return (_ctx"),
            "inline render absent:\n{}",
            script.code
        );
        assert_eq!(
            script.output_descriptor.source_space.kind,
            OutputSourceSpaceKind::GeneratedComposite
        );
        assert_eq!(
            script
                .output_descriptor
                .source_map
                .declared_space_tokens
                .len(),
            2
        );
    }

    #[test]
    fn generated_template_hole_uses_registered_geometry_not_user_marker_text() {
        let source = concat!(
            "<template src=\"./view.html\"></template>",
            "<script setup>",
            "const marker = \"/* verter-generated-template-hole */\";",
            "const count = 1",
            "</script>"
        );
        let compiler = VueCarrierCompiler;
        let alloc = oxc_allocator::Allocator::new();
        let output = compiler
            .compile_bundle_expect_produced(
                source,
                &artifact_for(source),
                &RuntimeCompileOptions {
                    filename: Some("MarkerCollision.vue".to_string()),
                    source_map: true,
                    want_ide: true,
                    block_content: RuntimeBlockContentInputs {
                        template: Some(projected_script("<div>{{ count }}</div>", "html")),
                        ..Default::default()
                    },
                    ..Default::default()
                },
                &alloc,
            )
            .expect("external template IDE composition");
        let ide = output.tsx.expect("IDE surface");
        assert!(
            ide.code
                .contains("const marker = \"/* verter-generated-template-hole */\";"),
            "the user's marker literal was used as geometry:\n{}",
            ide.code
        );
        assert!(ide.code.contains("{ count }"));
    }

    #[test]
    fn runtime_template_hole_uses_registered_geometry_not_user_marker_text() {
        let source = concat!(
            "<template lang=\"pug\">div {{ count }}</template>",
            "<script setup>",
            "const marker = \"/* verter-runtime-template-hole */\";",
            "const count = 1",
            "</script>"
        );
        let compiler = VueCarrierCompiler;
        let alloc = oxc_allocator::Allocator::new();
        let output = compiler
            .compile_bundle_expect_produced(
                source,
                &artifact_for(source),
                &RuntimeCompileOptions {
                    filename: Some("RuntimeMarkerCollision.vue".to_string()),
                    is_production: true,
                    source_map: true,
                    block_content: RuntimeBlockContentInputs {
                        template: Some(projected_script("<div>{{ count }}</div>", "html")),
                        ..Default::default()
                    },
                    ..Default::default()
                },
                &alloc,
            )
            .expect("supplied inline template composition");
        let script = output.script.expect("runtime script");
        assert!(
            script
                .code
                .contains("const marker = \"/* verter-runtime-template-hole */\";"),
            "the user's marker literal was used as geometry:\n{}",
            script.code
        );
        assert!(script.code.contains("return (_ctx"));
    }

    #[test]
    fn supplied_inline_template_with_carrier_parse_error_is_typed_unavailable() {
        let source = "<template lang=\"pug\">div</template>\n<script setup>\nconst a = 1";
        let compiler = VueCarrierCompiler;
        let alloc = oxc_allocator::Allocator::new();
        let result = compiler.compile_bundle_expect_produced(
            source,
            &artifact_for(source),
            &RuntimeCompileOptions {
                filename: Some("BrokenInline.vue".to_string()),
                is_production: true,
                block_content: RuntimeBlockContentInputs {
                    template: Some(projected_script("<div></div>", "html")),
                    ..Default::default()
                },
                ..Default::default()
            },
            &alloc,
        );
        assert!(matches!(
            result,
            Err(CompileUnsupported::BlockContentRuntimeUnavailable { .. })
        ));
    }

    /// @ai-generated - Projected scripts stay IDE-gated until their composed
    /// carrier is valid TypeScript and contains no raw SFC markup.
    #[test]
    fn projected_scripts_are_ide_typed_unavailable() {
        let compiler = VueCarrierCompiler;
        let alloc = oxc_allocator::Allocator::new();
        for (source, block_content) in [
            (
                concat!(
                    "<script setup src=\"./logic.ts\"></script>",
                    "<template><div>{{ count }}</div></template>"
                ),
                RuntimeBlockContentInputs {
                    script_setup: Some(projected_script("const count = 1", "ts")),
                    ..Default::default()
                },
            ),
            (
                concat!(
                    "<script src=\"./logic.ts\"></script>",
                    "<template><div>{{ count }}</div></template>"
                ),
                RuntimeBlockContentInputs {
                    script: Some(projected_script(
                        "export default { data: () => ({ count: 1 }) }",
                        "ts",
                    )),
                    ..Default::default()
                },
            ),
        ] {
            let result = compiler.compile_bundle_expect_produced(
                source,
                &artifact_for(source),
                &RuntimeCompileOptions {
                    filename: Some("ProjectedScript.vue".to_string()),
                    source_map: true,
                    want_ide: true,
                    block_content,
                    ..Default::default()
                },
                &alloc,
            );
            assert!(matches!(
                result,
                Err(CompileUnsupported::BlockContentIdeUnavailable { .. })
            ));
        }
    }

    /// @ai-generated - Pins per-unit output source-space and artifact identity.
    #[test]
    fn runtime_units_publish_qualified_output_descriptors() {
        let source = concat!(
            "<script setup>const count = 1</script>",
            "<template><div>{{ count }}</div></template>",
            "<style>.root { color: red }</style>"
        );
        let compiler = VueCarrierCompiler;
        let artifact = artifact_for(source);
        let alloc = oxc_allocator::Allocator::new();
        let output = compiler
            .compile_bundle_expect_produced(
                source,
                &artifact,
                &RuntimeCompileOptions {
                    filename: Some("Qualified.vue".to_string()),
                    source_map: true,
                    want_ide: true,
                    ..Default::default()
                },
                &alloc,
            )
            .expect("native carrier compile");

        let script = output.script.expect("script output");
        let template = output.template.expect("template output");
        let style = output.styles.first().expect("style output");
        let ide = output.tsx.expect("IDE output");

        for descriptor in [
            &script.output_descriptor,
            &template.output_descriptor,
            &style.output_descriptor,
            &ide.output_descriptor,
        ] {
            assert!(!descriptor.source_space.token.is_empty());
            assert!(!descriptor.content_artifact.token.is_empty());
            assert_eq!(
                descriptor.content_artifact.source_space_token,
                descriptor.source_space.token
            );
            assert_eq!(
                descriptor.source_map.destination_space_token,
                descriptor.source_space.token
            );
            assert_eq!(
                descriptor.source_space.utf8_byte_len,
                descriptor.content_artifact.utf8_byte_len
            );
        }

        assert_eq!(
            script.output_descriptor.source_map.fidelity,
            SourceMapFidelity::Approximate,
            "script rewrites are represented honestly as approximate mappings"
        );
        assert_eq!(
            style.output_descriptor.source_map.fidelity,
            SourceMapFidelity::Exact,
            "an unchanged native style is an exact one-space identity"
        );
    }

    /// @ai-generated - Exercises every currently lifted IDE carrier/template
    /// geometry through the real, execution-proving TypeScript syntax gate.
    #[test]
    fn lifted_ide_classes_pass_tsc_syntax_validation() {
        let compiler = VueCarrierCompiler;
        let alloc = oxc_allocator::Allocator::new();

        let native_source = concat!(
            "<script setup>const count = 1</script>",
            "<template><div>{{ count }}</div></template>"
        );
        let native = compiler
            .compile_bundle_expect_produced(
                native_source,
                &artifact_for(native_source),
                &RuntimeCompileOptions {
                    filename: Some("Native.vue".to_string()),
                    source_map: true,
                    want_ide: true,
                    ..Default::default()
                },
                &alloc,
            )
            .expect("native IDE compile")
            .tsx
            .expect("native IDE output");
        assert_typescript_syntax_valid("native", &native.code, native.is_jsx);

        let native_plain_source = concat!(
            "<script lang=\"ts\">export default { data: () => ({ count: 1 }) }</script>",
            "<template><div>{{ count }}</div></template>"
        );
        let native_plain = compiler
            .compile_bundle_expect_produced(
                native_plain_source,
                &artifact_for(native_plain_source),
                &RuntimeCompileOptions {
                    filename: Some("NativePlain.vue".to_string()),
                    source_map: true,
                    want_ide: true,
                    ..Default::default()
                },
                &alloc,
            )
            .expect("native plain-script IDE compile")
            .tsx
            .expect("native plain-script IDE output");
        assert_typescript_syntax_valid(
            "native_plain_script",
            &native_plain.code,
            native_plain.is_jsx,
        );

        let external_source = concat!(
            "<template src=\"./view.html\"></template>",
            "<script setup>const count = 1</script>"
        );
        let external = compiler
            .compile_bundle_expect_produced(
                external_source,
                &artifact_for(external_source),
                &RuntimeCompileOptions {
                    filename: Some("ExternalTemplate.vue".to_string()),
                    source_map: true,
                    want_ide: true,
                    block_content: RuntimeBlockContentInputs {
                        template: Some(projected_script("<div>{{ count }}</div>", "html")),
                        ..Default::default()
                    },
                    ..Default::default()
                },
                &alloc,
            )
            .expect("external template IDE compile")
            .tsx
            .expect("external template IDE output");
        assert_typescript_syntax_valid("external_template", &external.code, external.is_jsx);

        for (name, source, selected_template) in [
            (
                "external_no_script",
                "<template src=\"./view.html\"></template>",
                "<div>external-only</div>",
            ),
            (
                "external_both_scripts",
                concat!(
                    "<template src=\"./view.html\"></template>",
                    "<script lang=\"ts\">export default {}</script>",
                    "<script setup lang=\"ts\">const count = 1</script>"
                ),
                "<div>{{ count }}</div>",
            ),
            (
                "external_empty_template",
                concat!(
                    "<template src=\"./view.html\"></template>",
                    "<script setup lang=\"ts\">const count = 1</script>"
                ),
                "",
            ),
            (
                "external_jsx_hostile_attribute",
                concat!(
                    "<template src=\"./view.html\"></template>",
                    "<script setup lang=\"ts\">const count = 1</script>"
                ),
                "<div title=\"a`b\">{{ count }}</div>",
            ),
            (
                "validated_supplied_template",
                concat!(
                    "<template lang=\"pug\">div authored-only</template>",
                    "<script setup lang=\"ts\">const count = 1</script>"
                ),
                "<div>{{ count }}</div>",
            ),
        ] {
            let ide = compiler
                .compile_bundle_expect_produced(
                    source,
                    &artifact_for(source),
                    &RuntimeCompileOptions {
                        filename: Some(format!("{name}.vue")),
                        source_map: true,
                        want_ide: true,
                        block_content: RuntimeBlockContentInputs {
                            template: Some(projected_script(selected_template, "html")),
                            ..Default::default()
                        },
                        ..Default::default()
                    },
                    &alloc,
                )
                .unwrap_or_else(|error| panic!("{name} IDE compile failed: {error:?}"))
                .tsx
                .unwrap_or_else(|| panic!("{name} IDE output missing"));
            assert_typescript_syntax_valid(name, &ide.code, ide.is_jsx);
        }

        let direct = compiler
            .compile_ide(
                external_source,
                &artifact_for(external_source),
                &IdeCompileOptions {
                    filename: Some("DirectExternalTemplate.vue".to_string()),
                    block_content: RuntimeBlockContentInputs {
                        template: Some(projected_script("<div>{{ count }}</div>", "html")),
                        ..Default::default()
                    },
                    ..Default::default()
                },
            )
            .expect("direct external-template IDE compile");
        assert_typescript_syntax_valid("direct_external_template", &direct.code, direct.is_jsx);
    }

    /// @ai-generated - Exercises every lifted runtime topology, including the
    /// carrier plain-script arm, through real node --check.
    #[test]
    fn lifted_runtime_classes_pass_node_check() {
        let compiler = VueCarrierCompiler;
        let alloc = oxc_allocator::Allocator::new();

        let projected_setup_source = concat!(
            "<script setup src=\"./logic.js\"></script>",
            "<template><div>{{ count }}</div></template>"
        );
        let projected_setup = compiler
            .compile_bundle_expect_produced(
                projected_setup_source,
                &artifact_for(projected_setup_source),
                &RuntimeCompileOptions {
                    filename: Some("ProjectedSetup.vue".to_string()),
                    block_content: RuntimeBlockContentInputs {
                        script_setup: Some(projected_script("const count = 1", "js")),
                        ..Default::default()
                    },
                    ..Default::default()
                },
                &alloc,
            )
            .expect("projected setup runtime compile");
        assert_javascript_module_valid(
            "projected_setup_script",
            &projected_setup.script.expect("projected setup script").code,
        );
        assert_javascript_module_valid(
            "projected_setup_template",
            &projected_setup
                .template
                .expect("carrier template output")
                .code,
        );

        let external_template_source = concat!(
            "<template src=\"./view.html\"></template>",
            "<script setup>const count = 1</script>"
        );
        let external_template = compiler
            .compile_bundle_expect_produced(
                external_template_source,
                &artifact_for(external_template_source),
                &RuntimeCompileOptions {
                    filename: Some("ExternalTemplate.vue".to_string()),
                    block_content: RuntimeBlockContentInputs {
                        template: Some(projected_script("<div>{{ count }}</div>", "html")),
                        ..Default::default()
                    },
                    ..Default::default()
                },
                &alloc,
            )
            .expect("external template runtime compile");
        assert_javascript_module_valid(
            "external_template_script",
            &external_template
                .script
                .expect("carrier script output")
                .code,
        );
        assert_javascript_module_valid(
            "external_template_render",
            &external_template
                .template
                .expect("external render output")
                .code,
        );

        let external_plain_source = concat!(
            "<template src=\"./view.html\"></template>",
            "<script>export default { data: () => ({ count: 1 }) }</script>"
        );
        let external_plain = compiler
            .compile_bundle_expect_produced(
                external_plain_source,
                &artifact_for(external_plain_source),
                &RuntimeCompileOptions {
                    filename: Some("ExternalPlain.vue".to_string()),
                    block_content: RuntimeBlockContentInputs {
                        template: Some(projected_script("<div>{{ count }}</div>", "html")),
                        ..Default::default()
                    },
                    ..Default::default()
                },
                &alloc,
            )
            .expect("external template with carrier plain script runtime compile");
        assert_javascript_module_valid(
            "external_plain_script",
            &external_plain
                .script
                .expect("carrier plain script output")
                .code,
        );
        assert_javascript_module_valid(
            "external_plain_render",
            &external_plain
                .template
                .expect("external plain-script render output")
                .code,
        );

        let external_both_source = concat!(
            "<template src=\"./view.html\"></template>",
            "<script>export default {}</script>",
            "<script setup>const count = 1</script>"
        );
        let external_both = compiler
            .compile_bundle_expect_produced(
                external_both_source,
                &artifact_for(external_both_source),
                &RuntimeCompileOptions {
                    filename: Some("ExternalBoth.vue".to_string()),
                    block_content: RuntimeBlockContentInputs {
                        template: Some(projected_script("<div>{{ count }}</div>", "html")),
                        ..Default::default()
                    },
                    ..Default::default()
                },
                &alloc,
            )
            .expect("external template with both carrier scripts runtime compile");
        assert_javascript_module_valid(
            "external_both_script",
            &external_both.script.expect("both-scripts output").code,
        );
        assert_javascript_module_valid(
            "external_both_render",
            &external_both
                .template
                .expect("both-scripts render output")
                .code,
        );

        let external_no_script_source = "<template src=\"./view.html\"></template>";
        let external_no_script = compiler
            .compile_bundle_expect_produced(
                external_no_script_source,
                &artifact_for(external_no_script_source),
                &RuntimeCompileOptions {
                    filename: Some("ExternalNoScript.vue".to_string()),
                    block_content: RuntimeBlockContentInputs {
                        template: Some(projected_script("<div>external-only</div>", "html")),
                        ..Default::default()
                    },
                    ..Default::default()
                },
                &alloc,
            )
            .expect("external template without carrier scripts runtime compile");
        if let Some(script) = external_no_script.script {
            assert_javascript_module_valid("external_no_script_shell", &script.code);
        }
        assert_javascript_module_valid(
            "external_no_script_render",
            &external_no_script
                .template
                .expect("no-script render output")
                .code,
        );

        let supplied_inline_source = concat!(
            "<template lang=\"pug\">div {{ count }}</template>",
            "<script setup>const count = 1</script>"
        );
        let supplied_inline = compiler
            .compile_bundle_expect_produced(
                supplied_inline_source,
                &artifact_for(supplied_inline_source),
                &RuntimeCompileOptions {
                    filename: Some("SuppliedInline.vue".to_string()),
                    is_production: true,
                    source_map: true,
                    block_content: RuntimeBlockContentInputs {
                        template: Some(projected_script("<div>{{ count }}</div>", "html")),
                        ..Default::default()
                    },
                    ..Default::default()
                },
                &alloc,
            )
            .expect("supplied inline runtime compile");
        assert_javascript_module_valid(
            "supplied_inline_script",
            &supplied_inline.script.expect("inline runtime script").code,
        );
    }

    #[test]
    fn script_regions_carry_kind_and_resolved_source_type() {
        let source =
            "<script>export default {}</script>\n<script setup lang=\"tsx\">const a = 1</script>";
        let artifact = artifact_for(source);
        let regions = artifact.script_regions();
        assert_eq!(regions.len(), 2);
        assert_eq!(regions[0].kind, ScriptRegionKind::Module);
        assert_eq!(regions[1].kind, ScriptRegionKind::Instance);
        // The first block WITH a lang attribute decides for the SFC —
        // both regions are stamped with the resolved dialect.
        assert_eq!(regions[0].source_type, ScriptSourceType::Tsx);
        assert_eq!(regions[1].source_type, ScriptSourceType::Tsx);
        // Content spans slice to the script text.
        assert_eq!(regions[0].span.slice(source).trim(), "export default {}");
        assert_eq!(regions[1].span.slice(source).trim(), "const a = 1");
    }

    #[test]
    fn lang_resolution_matches_the_historical_attr_scan() {
        // (source, expected) — the historical `sfc_script_source_type`
        // semantics: lowercase compare, unknown/absent → Ts.
        let cases = [
            ("<script lang=\"ts\">a</script>", ScriptSourceType::Ts),
            ("<script lang=\"tsx\">a</script>", ScriptSourceType::Tsx),
            ("<script lang=\"TSX\">a</script>", ScriptSourceType::Tsx),
            (
                "<script lang=\"jsx\">a</script>",
                ScriptSourceType::Jsx(JsModuleKind::Module),
            ),
            (
                "<script lang=\"js\">a</script>",
                ScriptSourceType::Js(JsModuleKind::Script),
            ),
            ("<script lang=\"coffee\">a</script>", ScriptSourceType::Ts),
            ("<script lang>a</script>", ScriptSourceType::Ts),
            ("<script>a</script>", ScriptSourceType::Ts),
            ("<template><div /></template>", ScriptSourceType::Ts),
        ];
        for (source, expected) in cases {
            let parsed = Arc::new(parse_sfc(source, None, None));
            assert_eq!(
                vue_script_source_type(&parsed, source),
                expected,
                "lang resolution drifted for {source:?}"
            );
        }
    }

    #[test]
    fn template_style_regions_and_external_links_are_populated() {
        let source = "<template src=\"./tpl.html\"></template>\n<style src=\"./a.css\"></style>\n<style>.x{}</style>\n<script src=\"./impl.ts\"></script>";
        let artifact = artifact_for(source);
        assert_eq!(artifact.common().template_regions().len(), 1);
        assert_eq!(artifact.common().style_regions().len(), 2);
        let external_links = artifact.common().external_links();
        let links: Vec<(&ExternalLinkKind, &str)> = external_links
            .iter()
            .map(|l| (&l.kind, l.specifier.as_str()))
            .collect();
        assert!(links.contains(&(&ExternalLinkKind::Script, "./impl.ts")));
        assert!(links.contains(&(&ExternalLinkKind::Template, "./tpl.html")));
        assert!(links.contains(&(&ExternalLinkKind::Style, "./a.css")));
        // The second style block has no src.
        assert_eq!(
            links
                .iter()
                .filter(|(k, _)| matches!(k, ExternalLinkKind::Style))
                .count(),
            1
        );
    }

    #[test]
    fn script_regions_are_source_ordered_when_setup_precedes_plain_script() {
        // The parser exposes plain-script-then-setup accessor order; the
        // artifact must re-order by source position.
        let source = "<script setup lang=\"ts\">const a = 1</script>\n<script lang=\"ts\">export default {}</script>";
        let artifact = artifact_for(source);
        let regions = artifact.script_regions();
        assert_eq!(regions.len(), 2);
        assert_eq!(
            regions[0].kind,
            ScriptRegionKind::Instance,
            "the <script setup> block appears first in source"
        );
        assert_eq!(regions[1].kind, ScriptRegionKind::Module);
        assert!(
            regions[0].span.start < regions[1].span.start,
            "regions must be ordered by source position"
        );
    }

    #[test]
    fn artifact_identity_names_the_vue_adapter() {
        let source = "<script>a</script>";
        let artifact = artifact_for(source);
        assert!(artifact.adapter_id().is_vue());
        assert_eq!(artifact.language_id().as_str(), "vue");
        assert_eq!(artifact.parse_key(), &expected_parse_key(source));
        assert!(artifact.diagnostics().is_empty());
    }

    // ── Vue CarrierCompiler impl ───────────────────────────────────

    #[test]
    fn vue_compiler_parse_stamps_the_parse_key_and_vue_identity() {
        let compiler = VueCarrierCompiler;
        assert!(compiler.adapter_id().is_vue());
        let source = "<script setup lang=\"ts\">const a = 1</script>\n<template><div /></template>";
        let artifact = artifact_for(source);
        assert!(artifact.adapter_id().is_vue());
        assert_eq!(artifact.parse_key(), &expected_parse_key(source));
        assert_eq!(artifact.script_regions().len(), 1);
        assert_eq!(artifact.common().template_regions().len(), 1);
    }

    #[test]
    fn vue_compiler_compile_ide_produces_tsx_for_a_typescript_sfc() {
        let compiler = VueCarrierCompiler;
        let source = "<script setup lang=\"ts\">const a: number = 1</script>\n<template><div>{{ a }}</div></template>";
        let artifact = artifact_for(source);
        let opts = IdeCompileOptions {
            filename: Some("App.vue".to_string()),
            ..Default::default()
        };
        let out = compiler
            .compile_ide(source, &artifact, &opts)
            .expect("a TS SFC compiles to a TSX IDE artifact");
        assert!(!out.is_jsx, "a lang=ts SFC yields TSX, not JSX");
        assert!(!out.code.is_empty(), "IDE code must be produced");
    }

    #[test]
    fn vue_compiler_compile_ide_rejects_a_foreign_artifact_with_typed_unsupported() {
        let compiler = VueCarrierCompiler;
        // An artifact stamped for another adapter cannot be opened by the
        // Vue ctx — the bridge returns the typed unsupported answer.
        let foreign = crate::framework_common::registered_carrier_projection::parse_registered_source_for_tests(
            verter_language::FileLanguage::svelte(),
            verter_language::carrier_grammar::CarrierGrammarConfig::Svelte,
            "<p>a</p>",
        );
        let err = compiler
            .compile_ide(
                "<script>a</script>",
                &foreign,
                &IdeCompileOptions::default(),
            )
            .expect_err("a foreign artifact has no Vue carrier to open");
        assert!(matches!(err, CompileUnsupported::NoIdeProjection { .. }));
    }

    #[test]
    fn vue_compiler_template_data_extracts_component_usages() {
        let source = "<script setup lang=\"ts\">import Child from './Child.vue'</script>\n<template><Child :foo=\"1\" /></template>";
        let artifact = artifact_for(source);
        let facts = crate::framework_common::VueSemanticAuthority
            .template_facts(source, &artifact)
            .expect("a Vue artifact must produce template facts")
            .data;
        assert!(
            facts.components.iter().any(|c| c.tag_name == "Child"),
            "template facts must surface the <Child> component usage"
        );
    }

    #[test]
    fn compile_bundle_template_facts_come_from_the_catalog() {
        let source = concat!(
            "<script setup lang=\"ts\">import Child from './Child.vue'</script>\n",
            "<template><Child :foo=\"1\" /></template>",
        );
        let artifact = artifact_for(source);
        let alloc = oxc_allocator::Allocator::new();
        let opts = RuntimeCompileOptions {
            want_runtime: false,
            want_ide: false,
            want_template_data: true,
            ..Default::default()
        };
        let bundle = VueCarrierCompiler
            .compile_bundle_expect_produced(source, &artifact, &opts, &alloc)
            .expect("Vue compile_bundle produces a bundle");
        let catalog =
            crate::framework_common::registered_carrier_projection::template_facts_from_catalog(
                &artifact,
                source,
                crate::framework_common::registered_carrier_projection::TemplateFactsBasis::AdmittedArtifact,
            )
            .expect("catalog must produce Vue template facts")
            .data;
        let bundled = bundle
            .template_data
            .as_ref()
            .expect("want_template_data must fill catalog facts");
        let bundled = &bundled.data;
        assert_eq!(bundled.components.len(), catalog.components.len());
        assert!(
            bundled
                .components
                .iter()
                .any(|component| component.tag_name == "Child"),
            "bundle template facts must retain the <Child> usage"
        );

        let reminted = artifact.remint_epoch_for_tests("unknown-epoch");
        let refused = VueCarrierCompiler
            .compile_bundle_expect_produced(source, &reminted, &opts, &alloc)
            .expect("runtime/IDE-free compile still produces a bundle");
        assert!(
            refused.template_data.is_none(),
            "a catalog miss must leave template_data None, not independently extract"
        );
    }

    fn template_facts_opts(template: Option<&str>) -> RuntimeCompileOptions {
        RuntimeCompileOptions {
            want_runtime: false,
            want_ide: false,
            want_template_data: true,
            block_content: RuntimeBlockContentInputs {
                template: template.map(|code| projected_script(code, "html")),
                ..Default::default()
            },
            ..Default::default()
        }
    }

    #[test]
    fn compile_bundle_refuses_template_facts_for_external_src_selected_content() {
        let source = concat!(
            "<script setup lang=\"ts\">import Child from './Child.vue'</script>\n",
            "<template src=\"./view.html\"></template>",
        );
        let artifact = artifact_for(source);
        let alloc = oxc_allocator::Allocator::new();
        let _ = crate::framework_common::registered_carrier_projection::take_template_facts_producer_invocations();
        let bundle = VueCarrierCompiler
            .compile_bundle_expect_produced(
                source,
                &artifact,
                &template_facts_opts(Some("<Child :foo=\"1\" />")),
                &alloc,
            )
            .expect("Vue compile_bundle produces a bundle");
        assert!(
            bundle.template_data.is_none(),
            "non-empty selected content for an external template src must refuse facts, not Some(empty)"
        );
        assert_eq!(
            crate::framework_common::registered_carrier_projection::take_template_facts_producer_invocations(),
            0,
            "a selected-content mismatch must not invoke the semantic producer"
        );
    }

    #[test]
    fn compile_bundle_refuses_template_facts_when_selected_bytes_differ() {
        let source = concat!(
            "<script setup lang=\"ts\">import Original from './Original.vue'</script>\n",
            "<template><Original /></template>",
        );
        let artifact = artifact_for(source);
        let alloc = oxc_allocator::Allocator::new();
        let _ = crate::framework_common::registered_carrier_projection::take_template_facts_producer_invocations();
        let bundle = VueCarrierCompiler
            .compile_bundle_expect_produced(
                source,
                &artifact,
                &template_facts_opts(Some("<Replacement />")),
                &alloc,
            )
            .expect("Vue compile_bundle produces a bundle");
        assert!(
            bundle.template_data.is_none(),
            "selected bytes that replace the admitted template must refuse facts, never publish the superseded <Original />"
        );
        assert_eq!(
            crate::framework_common::registered_carrier_projection::take_template_facts_producer_invocations(),
            0,
            "a selected-content mismatch must not invoke the semantic producer"
        );
    }

    #[test]
    fn compile_bundle_keeps_admitted_template_facts_when_selected_bytes_match() {
        let source = concat!(
            "<script setup lang=\"ts\">import Original from './Original.vue'</script>\n",
            "<template><Original /></template>",
        );
        let artifact = artifact_for(source);
        let alloc = oxc_allocator::Allocator::new();
        let _ = crate::framework_common::registered_carrier_projection::take_template_facts_producer_invocations();
        let bundle = VueCarrierCompiler
            .compile_bundle_expect_produced(
                source,
                &artifact,
                &template_facts_opts(Some("<Original />")),
                &alloc,
            )
            .expect("Vue compile_bundle produces a bundle");
        let facts = bundle
            .template_data
            .as_ref()
            .expect("byte-identical selected content must keep admitted carrier facts");
        assert!(
            facts
                .data
                .components
                .iter()
                .any(|component| component.tag_name == "Original"),
            "byte-identical selection must retain the admitted <Original /> usage"
        );
        assert!(
            facts
                .data
                .components
                .iter()
                .all(|component| component.tag_name != "Replacement"),
            "byte-identical selection must not invent a replacement component"
        );
        assert_eq!(
            crate::framework_common::registered_carrier_projection::take_template_facts_producer_invocations(),
            1,
            "an admitted selected match must invoke the semantic producer exactly once"
        );
    }
}
