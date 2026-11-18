//! Vue bundle orchestration over an admitted Vue parse.
//!
//! The runtime half of the Vue carrier: the registered compile and audit
//! entries, the one Vue bundle orchestration the host-integration backend
//! drives, custom-block staging, and the conversion of a compile result into
//! the neutral runtime bundle. The frontend half (parse, carrier payload,
//! artifact construction) lives in [`super::vue_bridge`].

use verter_identity::encoding::{CanonicalEncode, CanonicalEncoder};
use verter_language::LanguageId;
use verter_span::Span;

use verter_parser::parser::types::ParsedSfc;

use crate::assembly::{assemble_vue_runtime_main, VueRuntimeMainRequest};
use crate::compile::compile_from_parsed;
use crate::compile::types::VueExecutionInputs;
use crate::compile_request::{
    CompileProduct, CompileRequest, CompileRequestError, FrameworkCompileRequest,
    IdeProductRequest, VueBackendRequest, VueCompileRequest,
};
use crate::framework_common::carrier_compiler::{
    CarrierCompileOutcome, CompileUnsupported, IdeOutput, QualifiedRuntimeStyle,
    RuntimeCompileOptions, RuntimeCompileOutput, RuntimeDiagnostic, RuntimeOutputDescriptor,
    RuntimeScriptBlock, RuntimeTemplateBlock, SourceMapFidelity,
};
use crate::framework_common::vue_carrier_frontend::VueCarrierFrontend;
use crate::framework_common::FrameworkParseArtifact;
use crate::parser::types::{sfc_script_dialect, SfcScriptDialect};

/// Compile a Vue artifact already elected by a registered publication store.
/// No raw-source parsing is reachable through this boundary. The caller's
/// [`crate::compile_transaction::CompileAttempt`] carries the staged Vue
/// runtime macro projection — the sole semantic handoff this route reads.
/// Reached only through [`crate::standalone::registered_audit_compile`], the
/// runtime-catalog selection for the artifact's carrier.
pub(crate) fn compile_registered_vue_artifact(
    source: &str,
    artifact: &FrameworkParseArtifact,
    request: &crate::compile_request::CompileRequest,
    execution_inputs: &VueExecutionInputs,
    attempt: &crate::compile_transaction::CompileAttempt<'_>,
    allocator: &oxc_allocator::Allocator,
) -> Result<crate::compile::VerterCompileResult, CompileUnsupported> {
    let Some(parsed) = VueCarrierFrontend.parsed_sfc(artifact) else {
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
        attempt.vue_macro_semantics(),
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
pub(crate) fn resolve_vue_backend_for_audit(
    artifact: &FrameworkParseArtifact,
    request: &crate::compile_request::CompileRequest,
) -> Option<
    Result<crate::compile_request::ResolvedVueBackend, crate::compile_request::CompileRequestError>,
> {
    let parsed = VueCarrierFrontend.parsed_sfc(artifact)?;
    Some(request.resolve_vue_backend(parsed.is_vapor()))
}

/// The one Vue bundle orchestration over an admitted parse: ordered
/// runtime, IDE-projection, and template-fact capability calls with shared
/// prerequisites and deduplicated diagnostics. Driven by the Vue
/// host-integration backend, so every admitted demand runs the identical
/// single-population pass.
///
/// Every product-backend leg requires — and consumes — its own
/// [`super::capability::ProductExecutionGrant`]: the host route carves
/// them off the consumed admission, and a demanded-but-ungranted leg
/// fails closed typed.
pub(crate) fn vue_carrier_bundle(
    source: &str,
    artifact: &FrameworkParseArtifact,
    opts: &RuntimeCompileOptions,
    attempt: &crate::compile_transaction::CompileAttempt<'_>,
    alloc: &oxc_allocator::Allocator,
    mut grants: super::capability::ProductExecutionGrants,
) -> Result<CarrierCompileOutcome, CompileUnsupported> {
    let Some(parsed) = VueCarrierFrontend.parsed_sfc(artifact) else {
        return Err(CompileUnsupported::NoIdeProjection {
            adapter_id: VueCarrierFrontend.adapter_id(),
        });
    };

    // The same two fail-closed rules `CompileRequest::new` /
    // `resolve_vue_backend` apply to every `CompileRequest`-constructed
    // route apply here too: this orchestration reads neutral
    // `RuntimeCompileOptions` derived off the admitted request, and the
    // source's own `<template vapor>` marker is known only after parse.
    // Without this check an SSR x Vapor or inline x SSR combination would
    // silently reach codegen and produce wrong output instead of a typed
    // refusal — the same bug class `CompileRequest` construction closes
    // for its own callers. `parsed.is_vapor()` covers the implicit
    // `<template vapor>` marker; `opts.force_vapor` covers the explicit
    // request — together they mirror `resolve_vue_backend`'s `Inferred`
    // fallback.
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
            adapter_id: VueCarrierFrontend.adapter_id(),
        });
    }

    match (
        opts.block_content.script.as_ref(),
        opts.block_content.script_setup.as_ref(),
    ) {
        (Some(_), Some(_)) | (Some(_), None) | (None, Some(_)) if opts.want_ide => {
            return Err(CompileUnsupported::BlockContentIdeUnavailable {
                adapter_id: VueCarrierFrontend.adapter_id(),
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
        let Some(backend) = super::carrier_registry::CarrierCompilerBackendRegistry::built_in()
            .runtime::<super::vue_runtime_backend::VueRuntimeBackend>(
            artifact.carrier_profile(),
            artifact.framework_profile(),
        ) else {
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
        bundle = backend.compile_bundle_runtime(grant, source, parsed, opts, attempt, alloc)?;
        // Descriptors are minted whether or not Main is demanded.
        stage_custom_block_artifacts(&mut bundle, artifact);
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
        // The projection leg reads the SAME staged handoff the runtime leg
        // reads — the transaction's carrier, never a second bundle field.
        let macros = attempt.vue_macro_semantics().clone();
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
    // its outcome is always `Produced`. Assembly runs after diagnostic
    // aggregation so an error-bearing compile is zero-work for Main and
    // still publishes its diagnostics.
    let mut bundle = with_catalog_template_facts(
        bundle,
        artifact,
        source,
        opts.want_template_data,
        opts.block_content
            .template
            .as_ref()
            .map(|input| input.code.as_ref()),
    );
    #[cfg(any(test, feature = "test-support"))]
    if opts.drop_required_script_map {
        if let Some(script) = bundle.script.as_mut() {
            script.source_map.clear();
        }
    }
    if opts.want_main && !bundle.has_errors() {
        emit_assembled_vue_main(&mut bundle, parsed, opts)
            .map_err(|failure| CompileUnsupported::VueMainAssemblyFailed(failure.to_string()))?;
    }
    Ok(CarrierCompileOutcome::Produced(bundle))
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
    let Some(options) = VueCarrierFrontend.parse_options(artifact) else {
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

/// Assemble the Vue `_sfc_main` runtime module behind the Vue runtime
/// compiler: host identifiers ride in on `opts.vue_main`, topology is
/// compiler-owned, and the staged compile-artifact handoff becomes
/// `bundle.main`.
pub(crate) fn emit_assembled_vue_main(
    bundle: &mut RuntimeCompileOutput,
    parsed: &ParsedSfc,
    opts: &RuntimeCompileOptions,
) -> Result<(), crate::assembly::VueMainAssemblyFailure> {
    let runtime = opts.runtime_module_name.as_deref().unwrap_or("vue");
    let canonical_id = opts
        .vue_canonical_id
        .as_deref()
        .filter(|id| !id.is_empty())
        .or(opts.filename.as_deref())
        .unwrap_or("");
    let derived_lang = vue_script_lang_from_parsed(parsed);
    let script_lang = opts.vue_script_lang.as_deref().or(derived_lang.as_deref());
    let assembled = assemble_vue_runtime_main(VueRuntimeMainRequest {
        canonical_id,
        compiled: bundle,
        script_lang,
        has_script: opts.vue_has_script,
        has_template: opts.vue_has_template,
        force_js: opts.force_js,
        source_map: opts.source_map,
        runtime,
        ssr: opts.ssr,
        decoration: opts.vue_main.clone(),
    })?;
    bundle.main = Some(assembled);
    Ok(())
}

fn vue_script_lang_from_parsed(parsed: &ParsedSfc) -> Option<String> {
    if parsed.script().is_none() && parsed.script_setup().is_none() {
        return None;
    }
    match sfc_script_dialect(parsed.script_setup(), parsed.script()) {
        SfcScriptDialect::JavaScript => Some("js".to_string()),
        SfcScriptDialect::Jsx => Some("jsx".to_string()),
        SfcScriptDialect::TypeScript => Some("ts".to_string()),
        SfcScriptDialect::Tsx => Some("tsx".to_string()),
    }
}

/// Retained custom-block parse facts in transit from the runtime leg to the
/// Vue bridge. Only this module reads or fills them, so they never become a
/// custom-block surface beside the descriptors they mint.
#[derive(Debug, Default)]
pub struct StagedCustomBlockFacts(Vec<crate::compile::VerterCustomBlock>);

/// Diagnostic code for custom-block facts that cannot become descriptors.
const CUSTOM_BLOCK_REFUSED: &str = "vue-runtime-custom-block-refused";

/// Source lineage of a custom-block cell: the registered canonical file and
/// its incarnation. The session-scoped authority namespace, the generation
/// and the bytes stay out; the carrier-bytes revision carries the bytes.
struct VueCustomBlockSource<'a>(
    &'a verter_language::registered_source_authority::RegisteredSourceSnapshotId,
);
impl CanonicalEncode for VueCustomBlockSource<'_> {
    const DOMAIN_TAG: &'static str = "verter.compiler.vue.custom_blocks.source.v1";
    fn encode_fields(&self, e: &mut CanonicalEncoder) {
        e.field_bytes(1, self.0.canonical_digest().as_bytes());
        e.field_u64(2, self.0.file_incarnation().get());
    }
}

/// Input basis of a custom-block cell: the admitted parse it was read from.
struct VueCustomBlockBasis<'a>(&'a verter_language::ParseKey);
impl CanonicalEncode for VueCustomBlockBasis<'_> {
    const DOMAIN_TAG: &'static str = "verter.compiler.vue.custom_blocks.basis.v1";
    fn encode_fields(&self, e: &mut CanonicalEncoder) {
        e.field_bytes(1, self.0.canonical_bytes());
    }
}

/// Producer contract of every custom-block descriptor and its `"sfc"` artifact.
struct VueCustomBlockProducer;
impl CanonicalEncode for VueCustomBlockProducer {
    const DOMAIN_TAG: &'static str = "verter.compiler.vue.custom_blocks.producer.v1";
    fn encode_fields(&self, e: &mut CanonicalEncoder) {
        e.field_str(1, "vue-runtime-custom-blocks");
    }
}

pub(crate) fn custom_block_refusal(error: impl std::fmt::Display, span: Span) -> RuntimeDiagnostic {
    RuntimeDiagnostic {
        severity: super::carrier_compiler::RuntimeDiagnosticSeverity::Error,
        code: CUSTOM_BLOCK_REFUSED.to_string(),
        message: format!("custom block cannot be published: {error}"),
        span,
    }
}

/// Take the runtime leg's custom-block facts and mint their descriptors
/// against the admitted artifact. A refusal publishes no descriptor and
/// fails the compile with an error diagnostic rather than an empty set. A
/// carrier without custom blocks does no work.
fn stage_custom_block_artifacts(
    bundle: &mut RuntimeCompileOutput,
    artifact: &FrameworkParseArtifact,
) {
    let StagedCustomBlockFacts(facts) = std::mem::take(&mut bundle.custom_block_facts);
    if facts.is_empty() {
        return;
    }
    match vue_custom_block_artifacts(artifact, facts) {
        Ok(set) => bundle.custom_block_artifacts = Some(set),
        Err(refusal) => bundle.diagnostics.push(refusal),
    }
}

/// The custom-block producer cell: one `"sfc"` source unit over the
/// registered carrier bytes, one `"sfc"` analysis artifact, and one
/// validated descriptor per block attached to it, moving each block's facts
/// in. Facts read from bytes other than the registered carrier refuse as
/// `SourceMismatch`.
fn vue_custom_block_artifacts(
    artifact: &FrameworkParseArtifact,
    facts: Vec<crate::compile::VerterCustomBlock>,
) -> Result<crate::assembly::CompileArtifactSet, RuntimeDiagnostic> {
    use crate::assembly::source_unit::carrier_revision_of;
    use crate::assembly::{
        ArtifactContent, ArtifactProvenance, ArtifactSourceUnit, ArtifactUnavailableReason,
        CompileArtifact, CompileArtifactSet, ContentId, CustomBlockDescriptorError, SourceId,
        SourceUnit,
    };
    use verter_identity::identity::{InputBasisId, ResultContractId};

    let Some((registered, source)) =
        artifact
            .inventory()
            .source_spaces()
            .iter()
            .find_map(|space| match &space.identity {
                verter_language::SourceSpaceIdentity::RegisteredSnapshot { snapshot } => {
                    Some((snapshot, space.bytes()))
                }
                verter_language::SourceSpaceIdentity::DerivedTransformOutput { .. } => None,
            })
    else {
        return Err(custom_block_refusal(
            CustomBlockDescriptorError::UnknownSourceUnit,
            Span::new(0, 0),
        ));
    };
    let whole = Span::new(0, source.len() as u32);
    let content = ContentId::from_content_bytes(source.as_bytes());
    if facts.iter().any(|block| block.source_content != content) {
        return Err(custom_block_refusal(
            CustomBlockDescriptorError::SourceMismatch,
            whole,
        ));
    }

    let source_id = SourceId::from_canonical(&VueCustomBlockSource(registered));
    let unit = SourceUnit::mint(
        source_id.clone(),
        carrier_revision_of(&content),
        "sfc",
        content.clone(),
    );
    let provenance = ArtifactProvenance {
        input_basis: InputBasisId::from_canonical(&VueCustomBlockBasis(artifact.parse_key())),
        producer: ResultContractId::from_canonical(&VueCustomBlockProducer),
        inputs: std::collections::BTreeSet::from([unit.id().clone()]),
    };
    let sfc = CompileArtifact::new(
        unit.id().clone(),
        crate::compile_request::ProductKind::Analysis,
        LanguageId::new("vue"),
        "sfc",
        provenance.clone(),
        ArtifactContent::Unavailable(ArtifactUnavailableReason::NotProduced),
    );
    let descriptors = facts
        .into_iter()
        .map(|block| {
            let region = block.region;
            custom_block_descriptor(block, &unit, &provenance, sfc.id())
                .map_err(|error| custom_block_refusal(error, region))
        })
        .collect::<Result<Vec<_>, _>>()?;
    CompileArtifactSet::new(
        vec![ArtifactSourceUnit {
            unit,
            source_span: whole,
        }],
        vec![sfc],
    )
    .map_err(|error| custom_block_refusal(error, whole))?
    .attach_custom_blocks(descriptors)
    .map_err(|error| custom_block_refusal(error, whole))
}

/// One custom block's descriptor over `unit`, the source unit the block's
/// facts were read from, attached to `attached_to`. The block's facts move
/// in; its body is local text, `src`-backed, or empty.
pub(crate) fn custom_block_descriptor(
    block: crate::compile::VerterCustomBlock,
    unit: &crate::assembly::SourceUnit,
    provenance: &crate::assembly::ArtifactProvenance,
    attached_to: &crate::assembly::ArtifactId,
) -> Result<crate::assembly::CustomBlockDescriptor, crate::assembly::CustomBlockDescriptorError> {
    use crate::assembly::{
        ContentId, CustomBlockContent, CustomBlockDescriptor, CustomBlockDescriptorRequest,
        CustomBlockLifecycle,
    };
    let content = if block.src.is_some() {
        CustomBlockContent::SrcBacked
    } else if block.content.is_empty() {
        CustomBlockContent::Empty
    } else {
        CustomBlockContent::Local {
            content: ContentId::from_content_bytes(block.content.as_bytes()),
            text: block.content,
        }
    };
    CustomBlockDescriptor::try_new(CustomBlockDescriptorRequest {
        source_unit: unit.id().clone(),
        source_id: unit.source_id().clone(),
        revision: unit.revision().clone(),
        source_content: unit.content().clone(),
        role: block.block_type,
        lang: block.lang,
        src: block.src,
        attributes: block.attrs,
        source_order: block.source_order,
        region: block.region,
        content,
        provenance: provenance.clone(),
        attached_to: attached_to.clone(),
        lifecycle: CustomBlockLifecycle::Complete,
    })
}

/// Re-express a Vue [`VerterCompileResult`] as the framework-neutral
/// [`RuntimeCompileOutput`]. Block fields are filled here; the assembled
/// `_sfc_main` body is emitted by [`emit_assembled_vue_main`] on the
/// runtime-bundle path. Direct conversion (conformance/tests) leaves
/// `main` unstaged so the compiler-owned assemble adapter can run
/// once with host identifiers.
///
/// Public so conformance/test harnesses can drive the genuine
/// compile → bundle → assemble pipeline without re-implementing the
/// conversion (the Vue conformance seed in `verter_vue_conformance`).
///
/// With no host selection to fill a style slot the compile left empty, the
/// style list is withheld; the runtime routes publish through
/// [`vue_result_to_runtime_parts`] and decide what an empty slot means.
pub fn vue_result_to_runtime_bundle(
    source: &str,
    parsed: &ParsedSfc,
    result: crate::compile::VerterCompileResult,
) -> RuntimeCompileOutput {
    let (mut bundle, styles) = vue_result_to_runtime_parts(source, parsed, result);
    bundle.qualified_styles = styles
        .into_iter()
        .collect::<Option<Vec<_>>>()
        .unwrap_or_default();
    bundle
}

/// [`vue_result_to_runtime_bundle`] with the style list left per block: one
/// slot per authored `<style>` in source order, empty where the compile had
/// no qualified value. The returned bundle's `qualified_styles` is empty — a
/// route fills the slots its host selected and then publishes the list once,
/// with no hole in it.
pub(crate) fn vue_result_to_runtime_parts(
    source: &str,
    parsed: &ParsedSfc,
    result: crate::compile::VerterCompileResult,
) -> (RuntimeCompileOutput, Vec<Option<QualifiedRuntimeStyle>>) {
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
    // One slot per authored block. A block whose `lang` names no admitted
    // dialect has no qualified value, and none is manufactured to hold its
    // place: the slot stays empty for the host's selection to fill.
    let styles: Vec<Option<QualifiedRuntimeStyle>> = result
        .styles
        .into_iter()
        .zip(parsed.style_nodes())
        .map(|(s, node)| {
            let result = s.result?;
            let exact_map = node.content.as_ref().and_then(|content| {
                let authored = &source[content.start as usize..content.end as usize];
                (authored == result.code())
                    .then(|| exact_slice_source_map(source, content.start, authored))
            });
            let output_descriptor = RuntimeOutputDescriptor::generated(
                result.code(),
                exact_map.as_deref(),
                &declared,
                if exact_map.is_some() {
                    SourceMapFidelity::Exact
                } else {
                    SourceMapFidelity::Approximate
                },
            );
            Some(QualifiedRuntimeStyle {
                // The cascade that minted these bytes named their stage,
                // dialect and producer; this conversion carries that value
                // rather than re-deriving any of it from the bytes.
                result,
                // The carrier's own authored block is the space the rewrite
                // read — this route admits no external continuation, so
                // there is no produced space it could have consumed.
                consumed_stage: verter_css_syntax::StyleStage::Authored,
                // Vue's style pipeline produces no per-block css map here (style
                // post-processing happens host/JS-side), and carries no
                // `:global` fact.
                source_map: None,
                // Vue scoping rides the `data-v-…` attribute on `scope_id`, not a
                // per-block class hash.
                scope_hash: None,
                has_global: false,
                output_descriptor,
            })
        })
        .collect();
    // The parse facts feed the descriptor mint: the Vue bridge takes them
    // to mint `custom_block_artifacts` against the admitted artifact
    // (`stage_custom_block_artifacts`).
    let custom_block_facts = result.custom_blocks;
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

    let bundle = RuntimeCompileOutput {
        // Vue: compiler-owned main module is assembled on the runtime-bundle
        // path. Direct conversion stages no main handoff.
        main: None,
        script,
        template,
        qualified_styles: Vec::new(),
        // Minted by the Vue bridge, which holds the registered artifact; a
        // direct conversion has none and publishes no descriptors.
        custom_block_artifacts: None,
        custom_block_facts: StagedCustomBlockFacts(custom_block_facts),
        scope_id: result.scope_id,
        tsx,
        template_data,
        diagnostics,
        // The RESOLVED inline topology — the compiler already merged the
        // render into `setup()` when true, so host assembly takes the inline
        // branch (no render attach, no setup-return filter).
        inline: result.inline,
    };
    (bundle, styles)
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
    use crate::framework_common::vue_bridge::{parse_sfc, vue_script_source_type};
    use crate::framework_common::{
        carrier_compiler::OutputSourceSpaceKind, FrameworkSemanticAuthority, ProjectionBackend,
        RuntimeBlockContentInput, RuntimeBlockContentInputs, VueProjectionBackend,
        VueProjectionError, VueProjectionInputs,
    };
    use std::fs;
    use std::path::{Path, PathBuf};
    use std::process::Command;
    use std::sync::Arc;
    use verter_css_syntax::CssDialect;
    use verter_language::{
        parse_key_for, syntax_profile_id_for, ExternalLinkKind, FileLanguage, JsModuleKind,
        ParseOptions, ScriptRegionKind, ScriptSourceType, VUE_SYNTAX_COMPATIBILITY_DOMAIN,
        VUE_SYNTAX_COMPATIBILITY_EPOCH,
    };

    /// Test-only direct route onto the shared bundle orchestration, minting
    /// the leg grants a host admission would carve. Its semantic-only
    /// transaction carries no macro semantics, so a typed macro fails
    /// closed exactly as `Unavailable` demands.
    fn compile_bundle_direct(
        source: &str,
        artifact: &FrameworkParseArtifact,
        opts: &RuntimeCompileOptions,
        alloc: &oxc_allocator::Allocator,
    ) -> Result<CarrierCompileOutcome, CompileUnsupported> {
        let attempt = crate::compile_transaction::CompileAttempt::enter_semantic("");
        vue_carrier_bundle(
            source,
            artifact,
            opts,
            &attempt,
            alloc,
            crate::framework_common::carrier_compiler::direct_bundle_execution_grants(opts),
        )
    }

    /// Test-only direct route onto the typed Vue IDE projection backend,
    /// minting the grant a host admission would carve. The request carries
    /// the admitted artifact's own syntax profile.
    fn project_ide_direct(
        source: &str,
        artifact: &FrameworkParseArtifact,
        filename: &str,
        block_content: RuntimeBlockContentInputs,
    ) -> Result<IdeOutput, VueProjectionError> {
        let request = vue_ide_only_request(
            Some(filename.to_string()),
            None,
            false,
            false,
            IdeProductRequest {
                want_source_map: true,
                ide_chunk_boundaries: block_content.template.is_some(),
                ..Default::default()
            },
            vue_request_from_admitted_artifact(artifact),
        )
        .expect("ide-only request constructs");
        let grant = crate::framework_common::capability::ProductExecutionGrant::mint(
            crate::compile_request::ProductKind::IdeCompanion,
        );
        let inputs = VueProjectionInputs {
            block_content,
            ..Default::default()
        };
        VueProjectionBackend
            .project_ide(grant, source, artifact, &request, &inputs)
            .map(|companion| companion.ide)
    }

    /// Test-only convenience over [`compile_bundle_direct`] for fixtures
    /// whose carrier is known to PRODUCE.
    ///
    /// Deliberately test-only: production code matches the outcome
    /// exhaustively, so a refusal can never be unwrapped into "some bundle"
    /// there. A test that is ABOUT the refusal calls
    /// [`compile_bundle_direct`] and matches the arm.
    struct DirectBundleDriver;

    impl DirectBundleDriver {
        fn compile_bundle_expect_produced(
            &self,
            source: &str,
            artifact: &FrameworkParseArtifact,
            opts: &RuntimeCompileOptions,
            alloc: &oxc_allocator::Allocator,
        ) -> Result<RuntimeCompileOutput, CompileUnsupported> {
            compile_bundle_direct(source, artifact, opts, alloc).map(|outcome| {
                outcome
                    .into_produced()
                    .expect("this fixture's carrier produces a runtime surface")
            })
        }
    }

    /// Stand in for the host's identity for the tool that produced the bytes
    /// these tests supply.
    fn supplied_producer() -> verter_css_syntax::PreprocessorIdentity {
        verter_css_syntax::PreprocessorIdentity::Named(
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
        artifact_for_file(source, "file:///fixture.vue")
    }

    fn artifact_for_file(source: &str, canonical_file: &str) -> Arc<FrameworkParseArtifact> {
        use verter_language::carrier_grammar::{
            CarrierGrammarAuthority, CarrierParserGrammarVersion, FrameworkAdapterSemanticVersion,
        };
        use verter_language::registered_source_authority::{
            CanonicalFileId, FileIncarnation, RegisteredSourceAuthority, SourceGeneration,
        };
        let source_authority = RegisteredSourceAuthority::new().unwrap();
        let grammar_authority = CarrierGrammarAuthority::new().unwrap();
        let config =
            crate::framework_common::vue_grammar_config("{{", "}}", std::iter::empty::<&str>())
                .unwrap();
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
                CanonicalFileId::new(canonical_file),
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
            crate::framework_common::registered_carrier_projection::project_registered_accepted(
                &accepted,
            )
            .expect("fixture source parses")
            .into_framework_parse_artifact(),
        )
    }

    fn expected_parse_key(source: &str) -> verter_language::ParseKey {
        let language = FileLanguage::vue();
        // `artifact_for` registers the fixture with the real standard `{{`
        // / `}}` grammar (see `vue_grammar_config` above) — match
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
            authored_basis: None,
            diagnostics: Vec::new(),
        }
    }

    fn staged_runtime(
        source: &str,
        blocks: &RuntimeBlockContentInputs,
        inline: bool,
    ) -> Result<
        crate::assembly::StagedCompileArtifacts,
        crate::framework::vue::assembly::VueCompileRefusal,
    > {
        use crate::compile_request::{
            CompileProduct, CompileRequest, FrameworkCompileRequest, RuntimeProductRequest,
        };
        use crate::framework::vue::assembly::{
            compile_vue_runtime, VueCompileInputs, VueCompileRefusal,
        };
        use crate::work_ledger::CompilerWorkLedger;
        use verter_audit::observe::Uncaptured;
        let request = CompileRequest::new(
            vec![CompileProduct::RuntimeClient(RuntimeProductRequest {
                inline: Some(inline),
                runtime_source_map: true,
                ..Default::default()
            })],
            FrameworkCompileRequest::Vue(Default::default()),
            None,
            Some("Supplied.vue".into()),
            None,
            inline,
            true,
        )
        .unwrap();
        let parsed = parse_sfc(source, None, None);
        compile_vue_runtime(
            &VueCompileInputs {
                source,
                parsed: &parsed,
                request: &request,
                execution: &Default::default(),
                macros: &Default::default(),
                block_content: blocks,
                decoration: &Default::default(),
                canonical_id: "Supplied.vue",
            },
            &oxc_allocator::Allocator::default(),
            &mut CompilerWorkLedger::<Uncaptured>::new(),
        )?
        .stage(crate::compile_request::ProductKind::RuntimeClient)
        .map_err(VueCompileRefusal::Artifact)
    }

    #[test]
    fn two_projected_script_spaces_remain_typed_unavailable() {
        let source = concat!(
            "<script src=\"./logic.js\"></script>",
            "<script setup src=\"./setup.js\"></script>",
            "<template><div /></template>"
        );
        let compiler = DirectBundleDriver;
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
    fn carrier_bundle_runtime_branch_delegates_to_the_typed_runtime_backend() {
        // The bundle route reaches its runtime product only through the typed
        // Vue runtime backend; the per-thread delegation counter is the
        // witness that no route-private runtime emitter remains.
        let compiler = DirectBundleDriver;
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

    /// Compile `source` through the bundle orchestration over its own
    /// freshly registered artifact.
    fn compile_registered(source: &str, opts: &RuntimeCompileOptions) -> RuntimeCompileOutput {
        let artifact = artifact_for(source);
        let alloc = oxc_allocator::Allocator::new();
        DirectBundleDriver
            .compile_bundle_expect_produced(source, &artifact, opts, &alloc)
            .expect("vue runtime bundle produces")
    }

    fn custom_block_set(output: &RuntimeCompileOutput) -> &crate::assembly::CompileArtifactSet {
        output
            .custom_block_artifacts
            .as_ref()
            .expect("custom blocks stage their own artifact set")
    }

    fn sfc_revision(output: &RuntimeCompileOutput) -> crate::assembly::SourceRevision {
        custom_block_set(output)
            .source_units()
            .next()
            .expect("the cell stages one sfc unit")
            .unit
            .revision()
            .clone()
    }

    /// AC1/AC2: every custom block gets exactly one complete, source-backed
    /// descriptor, minted by the Vue bridge without a Main demand, bound to
    /// one `"sfc"` unit over the registered carrier bytes and attached to
    /// that unit's `"sfc"` artifact. The descriptors are the bundle's only
    /// custom-block surface.
    #[test]
    fn custom_blocks_produce_source_backed_descriptors() {
        use crate::assembly::{
            ArtifactContent, ContentId, CustomBlockContent, CustomBlockLifecycle,
        };
        let source = concat!(
            "<script setup>const n = 1</script>",
            "<i18n lang=\"json\">{\"a\":1}</i18n>",
            "<docs src=\"./docs.md\"></docs>",
            "<note></note>",
        );
        let output = compile_registered(source, &RuntimeCompileOptions::default());
        assert!(output.main.is_none(), "Main is not demanded here");

        assert_eq!(output.custom_block_descriptors().len(), 3);

        let set = custom_block_set(&output);
        let units: Vec<_> = set.source_units().collect();
        assert_eq!(units.len(), 1, "one sfc unit for the whole carrier");
        let unit = &units[0];
        assert_eq!(unit.unit.logical_role(), "sfc");
        assert_eq!(unit.source_span, Span::new(0, source.len() as u32));
        assert_eq!(
            *unit.unit.content(),
            ContentId::from_content_bytes(source.as_bytes())
        );
        assert_eq!(
            *unit.unit.revision(),
            crate::assembly::source_unit::carrier_revision(source),
            "the sfc revision is the carrier-bytes revision, not a generated-output basis"
        );
        assert_eq!(set.artifacts().len(), 1);
        let sfc = &set.artifacts()[0];
        assert_eq!(sfc.name(), "sfc");
        assert_eq!(sfc.source_unit(), unit.unit.id());
        assert!(matches!(sfc.content, ArtifactContent::Unavailable(_)));

        let descriptors = set.custom_blocks();
        assert_eq!(descriptors.len(), 3, "one descriptor per custom block");
        for (order, descriptor) in descriptors.iter().enumerate() {
            assert_eq!(descriptor.source_order(), order as u32);
            assert_eq!(descriptor.source_unit(), unit.unit.id());
            assert_eq!(descriptor.source_id(), unit.unit.source_id());
            assert_eq!(descriptor.revision(), unit.unit.revision());
            assert_eq!(descriptor.source_content(), unit.unit.content());
            assert_eq!(descriptor.attached_to(), sfc.id());
            assert_eq!(*descriptor.provenance(), sfc.provenance);
            assert_eq!(
                descriptor.provenance().inputs,
                std::collections::BTreeSet::from([unit.unit.id().clone()])
            );
            assert_eq!(descriptor.lifecycle(), CustomBlockLifecycle::Complete);
        }
        for pair in descriptors.windows(2) {
            assert!(
                pair[0].region().end <= pair[1].region().start,
                "sibling regions must not overlap and must stay ordered"
            );
        }

        let i18n = &descriptors[0];
        assert_eq!(i18n.role(), "i18n");
        assert_eq!(i18n.lang(), Some("json"));
        assert_eq!(i18n.src(), None);
        let region = i18n.region();
        match i18n.content() {
            CustomBlockContent::Local { text, content } => {
                assert_eq!(text, &source[region.start as usize..region.end as usize]);
                assert_eq!(*content, ContentId::from_content_bytes(text.as_bytes()));
            }
            other => panic!("expected local content, got {other:?}"),
        }

        let docs = &descriptors[1];
        assert_eq!(docs.role(), "docs");
        assert_eq!(docs.src(), Some("./docs.md"));
        assert!(matches!(docs.content(), CustomBlockContent::SrcBacked));

        let note = &descriptors[2];
        assert_eq!(note.role(), "note");
        assert!(matches!(note.content(), CustomBlockContent::Empty));
    }

    /// AC1/AC3: descriptor identity and the sfc revision depend on the
    /// registered carrier alone. Demanding Main, a host filename, production
    /// mode, or an HMR flavour changes none of them, and Main's own artifact
    /// set stays free of custom-block material.
    #[test]
    fn custom_block_identity_ignores_main_demand_and_main_options() {
        let source = "<script setup>const n = 1</script><i18n lang=\"json\">{\"a\":1}</i18n>";
        let plain = compile_registered(source, &RuntimeCompileOptions::default());
        let with_main = compile_registered(
            source,
            &RuntimeCompileOptions {
                want_main: true,
                ..Default::default()
            },
        );
        let production = compile_registered(
            source,
            &RuntimeCompileOptions {
                filename: Some("src/Comp.vue".to_string()),
                is_production: true,
                want_main: true,
                vue_main: crate::assembly::VueMainDecoration {
                    hmr: crate::compile_request::RuntimeHmrStrategy::Vite,
                    is_production: true,
                    ..Default::default()
                },
                ..Default::default()
            },
        );
        let ids = |output: &RuntimeCompileOutput| {
            custom_block_set(output)
                .custom_blocks()
                .iter()
                .map(|descriptor| descriptor.id().clone())
                .collect::<Vec<_>>()
        };
        assert_eq!(ids(&plain), ids(&with_main));
        assert_eq!(ids(&plain), ids(&production));
        assert_eq!(sfc_revision(&plain), sfc_revision(&with_main));
        assert_eq!(sfc_revision(&plain), sfc_revision(&production));

        let main = with_main.main.as_ref().expect("Main is demanded").set();
        assert!(main.custom_blocks().is_empty());
        assert!(main
            .source_units()
            .all(|unit| unit.unit.logical_role() != "sfc"));
    }

    /// AC2/AC3 edit-revert: a block-only edit changes no generated
    /// script/template byte, yet must move the sfc revision and remint the
    /// descriptor; reverting the edit restores both.
    #[test]
    fn custom_block_edit_remints_identity_and_revert_restores_it() {
        let opts = RuntimeCompileOptions::default();
        let source = "<script setup>const n = 1</script><i18n lang=\"json\">{\"a\":1}</i18n>";
        let edited = "<script setup>const n = 1</script><i18n lang=\"json\">{\"a\":2}</i18n>";
        let first = compile_registered(source, &opts);
        let id = |output: &RuntimeCompileOutput| {
            custom_block_set(output).custom_blocks()[0].id().clone()
        };

        let again = compile_registered(source, &opts);
        assert_eq!(
            id(&first),
            id(&again),
            "same bytes mint the same descriptor"
        );
        assert_eq!(sfc_revision(&first), sfc_revision(&again));

        let after_edit = compile_registered(edited, &opts);
        assert_ne!(id(&first), id(&after_edit));
        assert_ne!(
            sfc_revision(&first),
            sfc_revision(&after_edit),
            "a block-only edit must move the sfc revision"
        );

        let reverted = compile_registered(source, &opts);
        assert_eq!(
            id(&first),
            id(&reverted),
            "reverting restores the descriptor"
        );
        assert_eq!(sfc_revision(&first), sfc_revision(&reverted));
    }

    /// The registered file names the descriptors' source lineage, whatever
    /// host filename a request carries: two files registered with
    /// byte-identical content never share an identity, while registering one
    /// file again mints the same identity.
    #[test]
    fn custom_block_descriptors_bind_to_the_registered_file() {
        let source = "<script setup>const n = 1</script><i18n lang=\"json\">{\"a\":1}</i18n>";
        let alloc = oxc_allocator::Allocator::new();
        let compile = |canonical_file: &str, filename: Option<&str>| {
            let artifact = artifact_for_file(source, canonical_file);
            DirectBundleDriver
                .compile_bundle_expect_produced(
                    source,
                    &artifact,
                    &RuntimeCompileOptions {
                        filename: filename.map(str::to_string),
                        ..Default::default()
                    },
                    &alloc,
                )
                .expect("vue bundle with a custom block produces")
        };
        let descriptor =
            |output: &RuntimeCompileOutput| custom_block_set(output).custom_blocks()[0].clone();

        let a = descriptor(&compile("file:///A.vue", None));
        let b = descriptor(&compile("file:///B.vue", None));
        assert_ne!(
            a.source_id(),
            b.source_id(),
            "separately registered byte-identical files must not share a source lineage"
        );
        assert_ne!(a.id(), b.id());

        let a_again = descriptor(&compile("file:///A.vue", Some("Same.vue")));
        assert_eq!(
            a.id(),
            a_again.id(),
            "one registered file mints one identity, independent of the host filename"
        );
        let b_named = descriptor(&compile("file:///B.vue", Some("Same.vue")));
        assert_ne!(
            a_again.id(),
            b_named.id(),
            "a shared host filename does not merge two registered files"
        );
    }

    /// AC2 'ordered attrs' is identity-bearing (`CustomBlockDescriptorId::mint`
    /// hashes attributes in order): a producer that sorted, deduped, or
    /// otherwise reordered attrs would still pass a find-first lang/src
    /// check, so this pins the full ordered vector exactly and proves
    /// reordering remints the descriptor id.
    #[test]
    fn custom_block_descriptor_attributes_preserve_exact_authored_order() {
        let opts = RuntimeCompileOptions::default();
        let forward = compile_registered(
            "<script setup>const n = 1</script><i18n foo=\"1\" bar=\"2\">{}</i18n>",
            &opts,
        );
        let forward = &custom_block_set(&forward).custom_blocks()[0];
        assert_eq!(
            forward.attributes(),
            &[
                ("foo".to_string(), "1".to_string()),
                ("bar".to_string(), "2".to_string())
            ],
            "attributes must reach the descriptor in exact authored order, not sorted or deduped"
        );

        let reversed = compile_registered(
            "<script setup>const n = 1</script><i18n bar=\"2\" foo=\"1\">{}</i18n>",
            &opts,
        );
        let reversed = &custom_block_set(&reversed).custom_blocks()[0];
        assert_eq!(
            reversed.attributes(),
            &[
                ("bar".to_string(), "2".to_string()),
                ("foo".to_string(), "1".to_string())
            ],
        );
        assert_ne!(
            forward.id(),
            reversed.id(),
            "swapping attribute order must remint the descriptor id (order is identity-bearing)"
        );
    }

    /// `lang`/`src` match their attribute names exactly, as Vue's own SFC
    /// parser does: an upper-case `LANG` stays an ordinary authored
    /// attribute rather than failing descriptor construction as an alias.
    #[test]
    fn custom_block_lang_and_src_match_attribute_names_exactly() {
        let output = compile_registered(
            "<script setup>const n = 1</script><i18n LANG=\"json\">{}</i18n>",
            &RuntimeCompileOptions::default(),
        );
        let descriptor = &custom_block_set(&output).custom_blocks()[0];
        assert_eq!(descriptor.lang(), None);
        assert_eq!(
            descriptor.attributes(),
            &[("LANG".to_string(), "json".to_string())]
        );
    }

    /// AC2 span exactness: a descriptor's region is the exact byte-identical
    /// slice backing its content, not merely equal-length — a producer bug
    /// that anchors the region at a shifted equal-length window must fail
    /// this.
    #[test]
    fn custom_block_descriptor_region_is_byte_exact() {
        let source = "<script setup>const n = 1</script><i18n lang=\"json\">{\"a\":1}</i18n>";
        let output = compile_registered(source, &RuntimeCompileOptions::default());
        let region = custom_block_set(&output).custom_blocks()[0].region();
        assert_eq!(
            &source[region.start as usize..region.end as usize],
            "{\"a\":1}",
            "region must be the exact backing byte slice, not an equal-length shifted window"
        );
    }

    /// A self-closing custom block has no content span; its region anchors
    /// at the tag's content-start position (a zero-length span there),
    /// never a rescanned or approximated position.
    #[test]
    fn self_closing_custom_block_region_anchors_at_tag_open_end() {
        let source = "<script setup>const n = 1</script><i18n lang=\"json\"/>";
        let output = compile_registered(source, &RuntimeCompileOptions::default());
        let descriptor = &custom_block_set(&output).custom_blocks()[0];
        let region = descriptor.region();
        assert_eq!(
            region.start, region.end,
            "self-closing block anchors a zero-length region"
        );
        // The `<i18n .../>` tag is the last thing in the source, so the
        // exclusive position just past its own `>` is `source.len()`. A
        // producer anchoring at `tag_open.start` or `tag_open.name_end`
        // (or any other in-tag offset) would land short of this exact byte.
        assert_eq!(
            region.start as usize,
            source.len(),
            "the anchor must land exactly at the tag_open end (just past the tag's own `>`), not any other in-tag position"
        );
        assert!(matches!(
            descriptor.content(),
            crate::assembly::CustomBlockContent::Empty
        ));
    }

    /// AC3: custom-block facts must come from the admitted artifact's
    /// registered bytes. A caller handing the orchestration an
    /// equal-length payload whose block differs (`{}` vs `[]`) passes every
    /// region and length check, so only the facts' source binding refuses
    /// it: no descriptor publishes and the compile fails with an error.
    #[test]
    fn custom_block_facts_from_bytes_other_than_the_registered_source_refuse() {
        let registered = "<script setup>const n = 1</script><i18n>{}</i18n>";
        let supplied = "<script setup>const n = 1</script><i18n>[]</i18n>";
        let artifact = artifact_for(registered);
        let alloc = oxc_allocator::Allocator::new();
        let output = DirectBundleDriver
            .compile_bundle_expect_produced(
                supplied,
                &artifact,
                &RuntimeCompileOptions {
                    want_main: true,
                    ..Default::default()
                },
                &alloc,
            )
            .expect("the refusal travels as a diagnostic");
        assert!(output.custom_block_artifacts.is_none());
        assert!(
            output.main.is_none(),
            "an erroring compile assembles no Main"
        );
        let refusal = output
            .diagnostics
            .iter()
            .find(|diagnostic| diagnostic.code == CUSTOM_BLOCK_REFUSED)
            .expect("the refusal is reported");
        assert_eq!(
            refusal.severity,
            super::super::carrier_compiler::RuntimeDiagnosticSeverity::Error
        );
        assert!(
            refusal.message.contains("SourceMismatch"),
            "the refusal names the source mismatch: {}",
            refusal.message
        );
    }

    /// A malformed custom block (e.g. an empty `lang` value) fails the
    /// compile with an error located on the block — never a silent empty
    /// descriptor set indistinguishable from "no custom blocks in this file".
    #[test]
    fn malformed_custom_block_surfaces_as_compile_error_not_silent_drop() {
        let source = "<script setup>const n = 1</script><i18n lang=\"\">{}</i18n>";
        let output = compile_registered(
            source,
            &RuntimeCompileOptions {
                want_main: true,
                ..Default::default()
            },
        );
        assert!(output.custom_block_artifacts.is_none());
        assert!(output.main.is_none());
        assert!(output.has_errors());
        let refusal = output
            .diagnostics
            .iter()
            .find(|diagnostic| diagnostic.code == CUSTOM_BLOCK_REFUSED)
            .expect("the malformed block is reported");
        assert!(refusal.message.contains("Malformed"), "{}", refusal.message);
        assert_eq!(
            &source[refusal.span.start as usize..refusal.span.end as usize],
            "{}",
            "the error is located on the refused block"
        );
    }

    /// AC4: a carrier with no custom blocks performs zero descriptor work and
    /// stages no custom-block set.
    #[test]
    fn no_custom_blocks_produces_no_descriptors() {
        let output = compile_registered(
            "<script setup>const n = 1</script><template><div>{{ n }}</div></template>",
            &RuntimeCompileOptions::default(),
        );
        assert!(output.custom_block_descriptors().is_empty());
        assert!(output.custom_block_artifacts.is_none());
    }

    /// @ai-generated - Proves external template lowering receives the inline
    /// setup binding metadata instead of resolving an imported component by name.
    #[test]
    fn projected_template_and_inline_setup_share_runtime_bindings() {
        let source = concat!(
            "<template src=\"./view.html\"></template>",
            "<script setup>import Foo from './Foo.vue'</script>"
        );
        let compiler = DirectBundleDriver;
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
        let compiler = DirectBundleDriver;
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
        let artifact = artifact_for(source).with_unregistered_profile_for_tests();
        let alloc = oxc_allocator::Allocator::new();
        let before = crate::standalone::runtime_backend_delegation_count();
        let result = compile_bundle_direct(
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
    fn carrier_bundle_refuses_explicit_ssr_and_force_vapor() {
        // SSR requested together with an explicit Vapor backend — RC.3
        // defines no Cartesian Vapor server compiler backend. The bundle
        // orchestration reads neutral options rather than a
        // `CompileRequest`, so it re-checks the combination itself; without
        // this guard the combination would reach codegen and produce wrong
        // output instead of a typed refusal.
        let source = "<template><div>{{ a }}</div></template>";
        let artifact = artifact_for(source);
        let alloc = oxc_allocator::Allocator::new();
        let result = compile_bundle_direct(
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
    fn carrier_bundle_refuses_implicit_vapor_marker_with_ssr() {
        // The implicit half of the same rule: the source's own `<template
        // vapor>` marker (not an explicit `force_vapor` request) resolves
        // to a Vapor backend just as surely, and must refuse identically —
        // mirrors `CompileRequest::resolve_vue_backend`'s post-parse check.
        let source = "<template vapor><div>{{ a }}</div></template>";
        let artifact = artifact_for(source);
        let alloc = oxc_allocator::Allocator::new();
        let result = compile_bundle_direct(
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
    fn carrier_bundle_refuses_inline_with_ssr() {
        // Inline assembly requested together with SSR — the SSR capability
        // has no inline axis; must never be silently demoted to non-inline.
        let source = concat!(
            "<script setup>const a = 1</script>",
            "<template><div>{{ a }}</div></template>"
        );
        let artifact = artifact_for(source);
        let alloc = oxc_allocator::Allocator::new();
        let result = compile_bundle_direct(
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
    fn carrier_bundle_permits_ssr_without_vapor_or_inline() {
        // Negative control for the two guards above: SSR alone (no Vapor,
        // no explicit inline) must still produce a runtime bundle — the
        // guards are precise to the Cartesian combination, not a blanket
        // SSR refusal.
        let source = "<template><div>{{ a }}</div></template>";
        let compiler = DirectBundleDriver;
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
    /// route (the bundle orchestration itself — not the `CompileRequest`
    /// predicate-level unit test in `compile_request/mod.rs`, which only
    /// proves `wants_runtime_source_map()`'s own logic in isolation and
    /// never actually runs a compile). Requesting BOTH the runtime AND the
    /// IDE product with `source_map: false` must produce a runtime script
    /// with NO source map — the pre-fix bug computed
    /// `source_map: opts.source_map || opts.want_ide`, which would
    /// silently turn the runtime map ON here purely because `want_ide` was
    /// set, even though the caller asked for no map at all.
    #[test]
    fn carrier_bundle_does_not_force_the_runtime_map_on_merely_because_ide_is_requested() {
        let source = concat!(
            "<script setup>const count = 1</script>",
            "<template><div>{{ count }}</div></template>"
        );
        let compiler = DirectBundleDriver;
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
    fn carrier_bundle_populates_both_maps_independently_when_both_are_requested() {
        let source = concat!(
            "<script setup>const count = 1</script>",
            "<template><div>{{ count }}</div></template>"
        );
        let compiler = DirectBundleDriver;
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
        let compiler = DirectBundleDriver;
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
        let compiler = DirectBundleDriver;
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
        let compiler = DirectBundleDriver;
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

        let direct = project_ide_direct(source, &artifact, "ExternalPlain.vue", block_content)
            .expect_err("the projection backend refuses the mid-module fragment");
        assert!(matches!(
            direct,
            VueProjectionError::Unsupported(CompileUnsupported::BlockContentIdeUnavailable { .. })
        ));
    }

    /// @ai-generated - Supplied plain CSS is stage-two input and keeps its external source space.
    #[test]
    fn supplied_external_style_is_scoped_in_its_own_source_space() {
        let source =
            "<template><div class=\"x\"/></template><style scoped src=\"./theme.css\"></style>";
        let compiler = DirectBundleDriver;
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
                            authored_basis: None,
                            diagnostics: Vec::new(),
                        })],
                        ..Default::default()
                    },
                    ..Default::default()
                },
                &alloc,
            )
            .expect("supplied external style compiles");
        let style = &output.qualified_styles[0];

        assert!(
            style.result.code().contains(".x[data-v-scope123]"),
            "{}",
            style.result.code()
        );
        assert_eq!(style.lang(), "css");
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
            let alloc = oxc_allocator::Allocator::new();
            compile_bundle_direct(
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
                            authored_basis: None,
                            diagnostics: Vec::new(),
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
            output.qualified_styles[0]
                .result
                .code()
                .contains(".a[data-v-scope123]"),
            "the supplied route still runs every plain-CSS stage: {}",
            output.qualified_styles[0].result.code()
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
        let alloc = oxc_allocator::Allocator::new();
        let result = compile_bundle_direct(
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
                        authored_basis: None,
                        diagnostics: Vec::new(),
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
                output.qualified_styles[0].result.code()
            ),
            other => panic!("expected BlockContentRuntimeUnavailable, got {other:?}"),
        }
    }

    /// @ai-generated - Sequential style stages must not claim an uncomposed exact map.
    #[test]
    fn sequential_external_style_rewrites_report_approximate_map_fidelity() {
        let source =
            "<template><div class=\"x\"/></template><style scoped src=\"./theme.css\"></style>";
        let compiler = DirectBundleDriver;
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
                            authored_basis: None,
                            diagnostics: Vec::new(),
                        })],
                        ..Default::default()
                    },
                    ..Default::default()
                },
                &alloc,
            )
            .expect("supplied external style compiles");
        let style = &output.qualified_styles[0];

        assert!(
            style.result.code().contains("var(--scope123-tone)"),
            "{}",
            style.result.code()
        );
        assert!(
            style.result.code().contains(".x[data-v-scope123]"),
            "{}",
            style.result.code()
        );
        assert_eq!(
            style.output_descriptor.source_map.fidelity,
            SourceMapFidelity::Approximate
        );
    }

    /// @ai-generated - one parse per content identity must hold for the
    /// "supplied-preprocessor-output" branch too (authored SCSS whose supplied bytes are already-compiled
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
        let compiler = DirectBundleDriver;
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
                            authored_basis: None,
                            diagnostics: Vec::new(),
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
        let style = &output.qualified_styles[0];
        assert!(
            style.result.code().contains("body[data-v-scope123]"),
            "{}",
            style.result.code()
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
        let alloc = oxc_allocator::Allocator::new();
        let err = compile_bundle_direct(
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
                            authored_basis: None,
                            diagnostics: Vec::new(),
                        }),
                        Some(RuntimeBlockContentInput {
                            code: Arc::from(css),
                            source_map: None,
                            lang: "css".to_string(),
                            content_artifact_token: "artifact:b".to_string(),
                            source_space_token: "space:b".to_string(),
                            parsed: None,
                            producer: Some(supplied_producer()),
                            authored_basis: None,
                            diagnostics: Vec::new(),
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
        let output = staged_runtime(
            source,
            &RuntimeBlockContentInputs {
                template: Some(projected_script("<div>{{ count }}</div>", "html")),
                ..Default::default()
            },
            true,
        )
        .expect("supplied inline module");
        let code = output.code();
        assert!(!code.contains("_sfc_main.render ="), "{code}");
        assert!(code.contains("const count = 1"));
        assert!(
            code.contains("return (_ctx"),
            "inline render absent:\n{}",
            code
        );
        let map = oxc_sourcemap::SourceMap::from_json_string(output.source_map().unwrap()).unwrap();
        assert_eq!(
            map.get_sources().collect::<Vec<_>>(),
            ["Supplied.vue", "space:html"]
        );
        assert_javascript_module_valid("supplied_inline", code);
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
        let compiler = DirectBundleDriver;
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
        let output = staged_runtime(
            source,
            &RuntimeBlockContentInputs {
                template: Some(projected_script("<div>{{ count }}</div>", "html")),
                ..Default::default()
            },
            true,
        )
        .expect("supplied inline template composition");
        let code = output.code();
        assert!(
            code.contains("const marker = \"/* verter-runtime-template-hole */\";"),
            "{code}"
        );
        assert!(code.contains("return (_ctx"));
        assert_javascript_module_valid("marker_collision", code);
    }

    #[test]
    fn supplied_inline_template_with_carrier_parse_error_is_typed_unavailable() {
        let source = "<template lang=\"pug\">div</template>\n<script setup>\nconst a = 1";
        let result = staged_runtime(
            source,
            &RuntimeBlockContentInputs {
                template: Some(projected_script("<div></div>", "html")),
                ..Default::default()
            },
            true,
        );
        assert!(matches!(
            result,
            Err(crate::framework::vue::assembly::VueCompileRefusal::InvalidSource { .. })
        ));
    }

    /// @ai-generated - Projected scripts stay IDE-gated until their composed
    /// carrier is valid TypeScript and contains no raw SFC markup.
    #[test]
    fn projected_scripts_are_ide_typed_unavailable() {
        let compiler = DirectBundleDriver;
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
        let compiler = DirectBundleDriver;
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
        let style = output.qualified_styles.first().expect("style output");
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
        let compiler = DirectBundleDriver;
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

        let direct = project_ide_direct(
            external_source,
            &artifact_for(external_source),
            "DirectExternalTemplate.vue",
            RuntimeBlockContentInputs {
                template: Some(projected_script("<div>{{ count }}</div>", "html")),
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
        for (name, source, blocks, inline) in [
            ("projected_setup", "<script setup src=\"./logic.ts\"></script><template><div>{{ count }}</div></template>", RuntimeBlockContentInputs { script_setup: Some(projected_script("const count: number = 1", "ts")), ..Default::default() }, false),
            ("external_template", "<template src=\"./view.html\"></template><script setup>const count = 1</script>", RuntimeBlockContentInputs { template: Some(projected_script("<div>{{ count }}</div>", "html")), ..Default::default() }, false),
            ("external_plain", "<template src=\"./view.html\"></template><script>export default { data: () => ({ count: 1 }) }</script>", RuntimeBlockContentInputs { template: Some(projected_script("<div>{{ count }}</div>", "html")), ..Default::default() }, false),
            ("external_both", "<template src=\"./view.html\"></template><script>export default {}</script><script setup>const count = 1</script>", RuntimeBlockContentInputs { template: Some(projected_script("<div>{{ count }}</div>", "html")), ..Default::default() }, false),
            ("external_no_script", "<template src=\"./view.html\"></template>", RuntimeBlockContentInputs { template: Some(projected_script("<div>external-only</div>", "html")), ..Default::default() }, false),
            ("supplied_inline", "<template lang=\"pug\">div {{ count }}</template><script setup>const count = 1</script>", RuntimeBlockContentInputs { template: Some(projected_script("<div>{{ count }}</div>", "html")), ..Default::default() }, true),
        ] {
            let staged = staged_runtime(source, &blocks, inline).unwrap_or_else(|error| panic!("{name}: {error:?}"));
            assert!(staged.code().contains("export default _sfc_main"), "{}", staged.code());
            assert_javascript_module_valid(name, staged.code());
        }
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
    fn external_links_name_the_script_role_of_a_script_link_only() {
        let source = "<script setup lang=\"ts\" src=\"./setup.ts\"></script>
<script src=\"./impl.ts\"></script>
<template src=\"./tpl.html\"></template>";
        let artifact = artifact_for(source);
        let links = artifact.common().external_links();
        let roles: Vec<(&str, Option<verter_language::ScriptRole>)> = links
            .iter()
            .map(|link| (link.specifier.as_str(), link.script_role))
            .collect();
        assert_eq!(
            roles,
            [
                ("./setup.ts", Some(verter_language::ScriptRole::Setup)),
                ("./impl.ts", Some(verter_language::ScriptRole::Module)),
                ("./tpl.html", None),
            ]
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

    // ── Vue carrier frontend identity ───────────────────────────────

    #[test]
    fn vue_frontend_parse_stamps_the_parse_key_and_vue_identity() {
        assert!(VueCarrierFrontend.adapter_id().is_vue());
        let source = "<script setup lang=\"ts\">const a = 1</script>\n<template><div /></template>";
        let artifact = artifact_for(source);
        assert!(artifact.adapter_id().is_vue());
        assert_eq!(artifact.parse_key(), &expected_parse_key(source));
        assert_eq!(artifact.script_regions().len(), 1);
        assert_eq!(artifact.common().template_regions().len(), 1);
    }

    #[test]
    fn vue_ide_projection_produces_tsx_for_a_typescript_sfc() {
        let source = "<script setup lang=\"ts\">const a: number = 1</script>\n<template><div>{{ a }}</div></template>";
        let artifact = artifact_for(source);
        let out = project_ide_direct(source, &artifact, "App.vue", Default::default())
            .expect("a TS SFC compiles to a TSX IDE artifact");
        assert!(!out.is_jsx, "a lang=ts SFC yields TSX, not JSX");
        assert!(!out.code.is_empty(), "IDE code must be produced");
    }

    #[test]
    fn vue_compiler_js_check_directive_is_a_leading_pragma() {
        let source = include_str!("../../../../tests/sfc-projection/STP12/fixtures/checkjs-on.vue");
        let artifact = artifact_for(source);
        let out = project_ide_direct(source, &artifact, "App.vue", Default::default())
            .expect("a JavaScript SFC compiles to a JSX IDE artifact");

        assert!(out.is_jsx, "a JavaScript SFC must yield JSX");
        assert!(
            out.code
                .starts_with("// @ts-check\n/** @jsxImportSource vue */\n"),
            "the authored check directive must remain a leading pragma:\n{}",
            out.code
        );
        assert!(
            out.code.contains("const label = 1"),
            "the checked JavaScript diagnostic source must remain authored:\n{}",
            out.code
        );
    }

    #[test]
    fn vue_compiler_js_unchecked_script_keeps_template_projection() {
        let source =
            include_str!("../../../../tests/sfc-projection/STP12/fixtures/checkjs-off.vue");
        let artifact = artifact_for(source);
        let out = project_ide_direct(source, &artifact, "App.vue", Default::default())
            .expect("an unchecked JavaScript SFC still projects its template");

        assert!(out.is_jsx, "a JavaScript SFC must yield JSX");
        assert!(
            out.code
                .starts_with("// @ts-nocheck\n/** @jsxImportSource vue */\n"),
            "the authored unchecked policy must remain a leading pragma:\n{}",
            out.code
        );
        assert!(
            out.code.contains("{ label }"),
            "the template must remain independently projected:\n{}",
            out.code
        );
    }

    #[test]
    fn vue_compiler_jsdoc_generic_uses_public_instance_contract() {
        let source =
            include_str!("../../../../tests/sfc-projection/STP12/fixtures/jsdoc-generic.vue");
        let artifact = artifact_for(source);
        let out = project_ide_direct(source, &artifact, "App.vue", Default::default())
            .expect("a JavaScript SFC compiles to a JSX IDE artifact");

        assert!(out.is_jsx, "a JavaScript SFC must yield JSX");
        assert!(
            out.code
                .contains("@template T @param {T} value @returns {T}"),
            "the authored JSDoc generic must remain in the JavaScript projection:\n{}",
            out.code
        );
        assert!(
            out.code
                .contains("InstanceType<typeof import(\"./App.vue.verter.js\")['default']>"),
            "the JavaScript self-instance must use the public constructor bridge:\n{}",
            out.code
        );
    }

    #[test]
    fn vue_compiler_jsx_keeps_authored_jsx_expression() {
        let source = include_str!("../../../../tests/sfc-projection/STP12/fixtures/jsx.vue");
        let artifact = artifact_for(source);
        let out = project_ide_direct(source, &artifact, "App.vue", Default::default())
            .expect("a JSX SFC compiles to a JSX IDE artifact");

        assert!(out.is_jsx, "a JSX SFC must yield JSX");
        assert!(
            out.code.contains("<span>authored</span>"),
            "the authored JSX expression must not be rewritten as TypeScript:\n{}",
            out.code
        );
    }

    #[test]
    fn vue_compiler_js_projection_never_injects_nocheck() {
        let source =
            include_str!("../../../../tests/sfc-projection/STP12/fixtures/suppression.vue");
        let artifact = artifact_for(source);
        let out = project_ide_direct(source, &artifact, "App.vue", Default::default())
            .expect("a JavaScript SFC compiles to a JSX IDE artifact");

        assert!(
            !out.code.contains("@ts-nocheck"),
            "the projection must not suppress customer diagnostics:\n{}",
            out.code
        );
        assert!(
            out.code.contains("const label = 1"),
            "the customer diagnostic source must remain visible:\n{}",
            out.code
        );
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
    fn carrier_bundle_template_facts_come_from_the_catalog() {
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
        let bundle = DirectBundleDriver
            .compile_bundle_expect_produced(source, &artifact, &opts, &alloc)
            .expect("the Vue carrier bundle produces");
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

        let reminted = artifact.with_unregistered_profile_for_tests();
        let refused = DirectBundleDriver
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
    fn carrier_bundle_refuses_template_facts_for_external_src_selected_content() {
        let source = concat!(
            "<script setup lang=\"ts\">import Child from './Child.vue'</script>\n",
            "<template src=\"./view.html\"></template>",
        );
        let artifact = artifact_for(source);
        let alloc = oxc_allocator::Allocator::new();
        let _ = crate::framework_common::registered_carrier_projection::take_template_facts_producer_invocations();
        let bundle = DirectBundleDriver
            .compile_bundle_expect_produced(
                source,
                &artifact,
                &template_facts_opts(Some("<Child :foo=\"1\" />")),
                &alloc,
            )
            .expect("the Vue carrier bundle produces");
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
    fn carrier_bundle_refuses_template_facts_when_selected_bytes_differ() {
        let source = concat!(
            "<script setup lang=\"ts\">import Original from './Original.vue'</script>\n",
            "<template><Original /></template>",
        );
        let artifact = artifact_for(source);
        let alloc = oxc_allocator::Allocator::new();
        let _ = crate::framework_common::registered_carrier_projection::take_template_facts_producer_invocations();
        let bundle = DirectBundleDriver
            .compile_bundle_expect_produced(
                source,
                &artifact,
                &template_facts_opts(Some("<Replacement />")),
                &alloc,
            )
            .expect("the Vue carrier bundle produces");
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
    fn carrier_bundle_keeps_admitted_template_facts_when_selected_bytes_match() {
        let source = concat!(
            "<script setup lang=\"ts\">import Original from './Original.vue'</script>\n",
            "<template><Original /></template>",
        );
        let artifact = artifact_for(source);
        let alloc = oxc_allocator::Allocator::new();
        let _ = crate::framework_common::registered_carrier_projection::take_template_facts_producer_invocations();
        let bundle = DirectBundleDriver
            .compile_bundle_expect_produced(
                source,
                &artifact,
                &template_facts_opts(Some("<Original />")),
                &alloc,
            )
            .expect("the Vue carrier bundle produces");
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

    /// A host that selects content for every `<style>` block still gets its
    /// style imports: the main module's prelude counts the published styles,
    /// and a selected block is published like any other. Counting a separate
    /// list that a full selection emptied dropped every `import` silently —
    /// the component rendered with no CSS and nothing refused.
    #[test]
    fn fully_selected_styles_still_drive_the_main_module_style_imports() {
        let source = "<template><div class=\"x\"/></template><style>.x { color: red; }</style>";
        let artifact = artifact_for(source);
        let alloc = oxc_allocator::Allocator::new();
        let bundle = DirectBundleDriver
            .compile_bundle_expect_produced(
                source,
                &artifact,
                &RuntimeCompileOptions {
                    filename: Some("Selected.vue".to_string()),
                    component_id: Some("scope123".to_string()),
                    want_runtime: true,
                    want_main: true,
                    vue_has_template: true,
                    block_content: RuntimeBlockContentInputs {
                        styles: vec![Some(RuntimeBlockContentInput {
                            code: Arc::from(".x { color: red; }"),
                            source_map: None,
                            lang: "css".to_string(),
                            content_artifact_token: "artifact:theme-css".to_string(),
                            source_space_token: "space:theme-css".to_string(),
                            parsed: None,
                            producer: None,
                            authored_basis: None,
                            diagnostics: Vec::new(),
                        })],
                        ..Default::default()
                    },
                    vue_main: crate::assembly::VueMainDecoration {
                        style_specifiers: vec![
                            "Selected.vue?vue&type=style&index=0&lang.css".to_string()
                        ],
                        ..crate::assembly::VueMainDecoration::default()
                    },
                    ..Default::default()
                },
                &alloc,
            )
            .expect("a fully selected style compiles");
        assert_eq!(
            bundle.qualified_styles.len(),
            1,
            "the selected block publishes exactly one style"
        );
        let body = bundle.main.as_ref().expect("staged main handoff").code();
        assert!(
            body.contains("import \"Selected.vue?vue&type=style&index=0&lang.css\""),
            "the selected style's host specifier must still be imported, got:\n{body}"
        );
    }
    /// A self-closing `src` block in a `lang` the rewrite cannot name has no
    /// slot value of its own; the host's supplied CSS is what fills it.
    #[test]
    fn self_closing_unnameable_block_publishes_the_supplied_result() {
        let source =
            "<template><div class=\"x\"/></template><style lang=\"postcss\" src=\"./theme.css\" />";
        let css = ".x { color: red; }";
        let alloc = oxc_allocator::Allocator::new();
        let output = DirectBundleDriver
            .compile_bundle_expect_produced(
                source,
                &artifact_for(source),
                &RuntimeCompileOptions {
                    filename: Some("SrcPostCss.vue".to_string()),
                    component_id: Some("scope123".to_string()),
                    want_runtime: true,
                    block_content: RuntimeBlockContentInputs {
                        styles: vec![Some(RuntimeBlockContentInput {
                            code: Arc::from(css),
                            source_map: None,
                            lang: "css".to_string(),
                            content_artifact_token: "artifact:theme-output".to_string(),
                            source_space_token: "space:theme-output".to_string(),
                            parsed: Some(supplied_style(css)),
                            producer: Some(supplied_producer()),
                            authored_basis: None,
                            diagnostics: Vec::new(),
                        })],
                        ..Default::default()
                    },
                    ..Default::default()
                },
                &alloc,
            )
            .expect("a supplied result fills the empty slot");
        assert!(!output.has_errors(), "{:?}", output.diagnostics);
        assert_eq!(output.qualified_styles.len(), 1);
        assert_eq!(
            output.qualified_styles[0].result.stage(),
            verter_css_syntax::StyleStage::Preprocessed
        );
        assert_eq!(output.qualified_styles[0].result.code(), css);
    }

    /// With nothing supplied, that block has no content the compiler can
    /// qualify, so the compile fails closed instead of publishing a hole or a
    /// borrowed dialect.
    #[test]
    fn self_closing_unnameable_block_without_supplied_content_fails_closed() {
        let source = "<template><div class=\"x\"/></template><style>.ok { color: blue }</style><style lang=\"postcss\" src=\"./theme.css\" />";
        let alloc = oxc_allocator::Allocator::new();
        let result = compile_bundle_direct(
            source,
            &artifact_for(source),
            &RuntimeCompileOptions {
                filename: Some("SrcPostCss.vue".to_string()),
                want_runtime: true,
                ..Default::default()
            },
            &alloc,
        );
        match result {
            Err(CompileUnsupported::BlockContentRuntimeUnavailable { .. }) => {}
            Ok(CarrierCompileOutcome::Produced(output)) => panic!(
                "an unfillable slot must not publish, got {:?}",
                output
                    .qualified_styles
                    .iter()
                    .map(|style| (style.result.stage(), style.lang(), style.result.code()))
                    .collect::<Vec<_>>()
            ),
            other => panic!("expected BlockContentRuntimeUnavailable, got {other:?}"),
        }
    }

    /// The bundler route for a `lang` the rewrite cannot name: the host runs
    /// the tool and supplies its CSS, and the published style states exactly
    /// that — preprocessed bytes from the named tool, with no refusal.
    #[test]
    fn unnameable_authored_dialect_with_supplied_css_publishes_the_preprocessed_result() {
        let source =
            "<template><div class=\"x\"/></template><style lang=\"postcss\">.x { color: red; }</style>";
        let css = ".x { color: red; }";
        let alloc = oxc_allocator::Allocator::new();
        let output = DirectBundleDriver
            .compile_bundle_expect_produced(
                source,
                &artifact_for(source),
                &RuntimeCompileOptions {
                    filename: Some("PostCss.vue".to_string()),
                    component_id: Some("scope123".to_string()),
                    want_runtime: true,
                    block_content: RuntimeBlockContentInputs {
                        styles: vec![Some(RuntimeBlockContentInput {
                            code: Arc::from(css),
                            source_map: None,
                            lang: "css".to_string(),
                            content_artifact_token: "artifact:postcss-output".to_string(),
                            source_space_token: "space:postcss-output".to_string(),
                            parsed: Some(supplied_style(css)),
                            producer: Some(supplied_producer()),
                            authored_basis: None,
                            diagnostics: Vec::new(),
                        })],
                        ..Default::default()
                    },
                    ..Default::default()
                },
                &alloc,
            )
            .expect("a supplied result for an unnameable dialect compiles");
        assert!(!output.has_errors(), "{:?}", output.diagnostics);
        assert_eq!(output.qualified_styles.len(), 1);
        let style = &output.qualified_styles[0];
        assert_eq!(
            style.result.stage(),
            verter_css_syntax::StyleStage::Preprocessed
        );
        assert_eq!(
            style.consumed_stage,
            verter_css_syntax::StyleStage::Preprocessed
        );
        assert!(style.result.producer().is_external());
        assert!(!style.result.is_refused());
        assert_eq!(style.result.code(), css);
    }

    #[test]
    fn style_only_demand_does_not_assemble_main() {
        let source = "<style>.x{color:red}</style><template><div/></template>";
        let artifact = artifact_for(source);
        let alloc = oxc_allocator::Allocator::new();
        crate::assembly::reset_vue_main_assembly_count();
        let bundle = DirectBundleDriver
            .compile_bundle_expect_produced(
                source,
                &artifact,
                &RuntimeCompileOptions {
                    want_runtime: true,
                    want_main: false,
                    filename: Some("StyleOnly.vue".to_string()),
                    ..Default::default()
                },
                &alloc,
            )
            .expect("STYLE-only runtime compile produces a bundle");
        assert_eq!(
            crate::assembly::vue_main_assembly_count(),
            0,
            "unrequested Main must be zero-work"
        );
        assert!(bundle.main.is_none());
        crate::assembly::reset_vue_main_assembly_count();
        let assembled = DirectBundleDriver
            .compile_bundle_expect_produced(
                source,
                &artifact,
                &RuntimeCompileOptions {
                    want_runtime: true,
                    want_main: true,
                    filename: Some("StyleOnly.vue".to_string()),
                    ..Default::default()
                },
                &alloc,
            )
            .expect("Main demand assembles");
        assert_eq!(crate::assembly::vue_main_assembly_count(), 1);
        assert!(assembled.main.is_some());
    }

    #[test]
    fn empty_script_map_with_main_demand_is_vue_main_assembly_failed() {
        let source = "<script setup>const n = 1</script><template><div>{{ n }}</div></template>";
        let artifact = artifact_for(source);
        let alloc = oxc_allocator::Allocator::new();
        let opts = RuntimeCompileOptions {
            filename: Some("Maps.vue".to_string()),
            source_map: true,
            want_runtime: true,
            want_main: false,
            vue_has_script: true,
            vue_has_template: true,
            ..Default::default()
        };
        let mut bundle = DirectBundleDriver
            .compile_bundle_expect_produced(source, &artifact, &opts, &alloc)
            .expect("maps-on compile without Main demand produces a bundle");
        let script = bundle.script.as_mut().expect("script block");
        script.source_map.clear();
        let parsed = VueCarrierFrontend
            .parsed_sfc(artifact.as_ref())
            .expect("admitted Vue parse");
        let mut refuse_opts = opts;
        refuse_opts.want_main = true;
        let failure = emit_assembled_vue_main(&mut bundle, parsed, &refuse_opts)
            .expect_err("empty script map must refuse compiler Main assembly");
        assert!(bundle.main.is_none());
        assert!(matches!(
            CompileUnsupported::VueMainAssemblyFailed(failure.to_string()),
            CompileUnsupported::VueMainAssemblyFailed(_)
        ));
    }

    #[test]
    fn template_only_main_lang_is_javascript() {
        let source = "<template><div/></template>";
        let artifact = artifact_for(source);
        let alloc = oxc_allocator::Allocator::new();
        let bundle = DirectBundleDriver
            .compile_bundle_expect_produced(
                source,
                &artifact,
                &RuntimeCompileOptions {
                    filename: Some("T.vue".to_string()),
                    want_main: true,
                    vue_has_script: false,
                    vue_has_template: true,
                    vue_script_lang: None,
                    ..Default::default()
                },
                &alloc,
            )
            .expect("template-only compiles");
        assert_eq!(bundle.main.as_ref().map(|staged| staged.lang()), Some("js"));
    }

    #[test]
    fn demanded_main_with_maps_qualifies_authored_geometry() {
        let source = "<script setup>const n = 1</script><template><div>{{ n }}</div></template>";
        let artifact = artifact_for(source);
        let alloc = oxc_allocator::Allocator::new();
        let bundle = DirectBundleDriver
            .compile_bundle_expect_produced(
                source,
                &artifact,
                &RuntimeCompileOptions {
                    filename: Some("Comp.vue".to_string()),
                    vue_canonical_id: Some("/src/Canonical.vue".to_string()),
                    source_map: true,
                    want_main: true,
                    vue_has_script: true,
                    vue_has_template: true,
                    vue_main: crate::assembly::VueMainDecoration {
                        hmr: crate::compile_request::RuntimeHmrStrategy::Vite,
                        ..crate::assembly::VueMainDecoration::default()
                    },
                    ..Default::default()
                },
                &alloc,
            )
            .expect("Main demand with maps produces a bundle");
        let staged = bundle.main.as_ref().expect("staged main handoff");
        let body = staged.code();
        assert!(
            body.contains("_sfc_main.__file = \"/src/Canonical.vue\""),
            "host canonical id must drive __file, got:\n{body}"
        );
        let set = staged.set();
        let main = set
            .artifacts()
            .iter()
            .find(|artifact| artifact.name() == "main")
            .expect("main artifact");
        let map = main
            .maps
            .iter()
            .find(|map| map.family == crate::assembly::ArtifactMapFamily::RuntimeSourceMap)
            .expect("runtime map");
        let main_unit = set
            .source_units()
            .find(|unit| unit.unit.logical_role() == "main")
            .expect("main unit")
            .unit
            .id()
            .clone();
        assert!(
            map.segments
                .iter()
                .all(|segment| segment.source_unit != main_unit),
            "qualified segments must not identity-map onto generated Main"
        );
        assert!(
            !map.segments.is_empty()
                || map.sources.iter().any(|id| {
                    set.source_units()
                        .any(|unit| unit.unit.id() == id && unit.unit.logical_role() != "main")
                }),
            "map must name authored inputs"
        );
    }
}
