//! Vue [`ProjectionBackend`] adapter.
//!
//! Projects the IDE companion over an already-admitted parse through the
//! parsed compiler core and an IDE-only [`CompileRequest`]. Catalog lookup
//! keys adapter × epoch × Projection.

use verter_language::{
    syntax_profile_id_for, FileLanguage, FrameworkAdapterId, LanguageId, ParseOptions,
    SyntaxProfileId,
};

use crate::compile::types::{
    CompileDiagnostic, VerterTsxBlock, VueExecutionInputs, VueMacroSemanticInput,
};
use crate::compile_request::{CompileRequest, ProductKind};
use crate::framework_common::capability::{Present, ProjectionBackend};
use crate::framework_common::carrier_compiler::{
    CompileUnsupported, IdeOutput, RuntimeBlockContentInputs, RuntimeOutputDescriptor,
    SourceMapFidelity,
};
use crate::framework_common::catalog::{ProjectionCap, TypedCapabilityRegistration};
use crate::framework_common::generated_chunk::{
    compose_generated_chunk, GeneratedFragment, GeneratedUnit,
};
use crate::framework_common::vue_bridge::VueCarrierCompiler;
use crate::framework_common::vue_carrier_frontend::VueSfcV3;
use crate::framework_common::{CarrierCompiler, FrameworkParseArtifact};
use crate::standalone::{DirectCompileError, StandaloneCompiler};

/// Vue IDE projection backend.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct VueProjectionBackend;

/// Compile diagnostic tagged with the source space it was emitted against.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VueProjectionDiagnostic {
    /// The compile diagnostic.
    pub diagnostic: CompileDiagnostic,
    /// Source-space token for this diagnostic's origin.
    pub source_space_token: String,
}

/// IDE companion plus compile diagnostics retained from the parsed core.
#[derive(Debug, Clone)]
pub struct VueIdeCompanion {
    /// Generated TSX/JSX companion.
    pub ide: IdeOutput,
    /// Non-fatal compile diagnostics from the parsed core, each tagged with
    /// the source space they were emitted against.
    pub diagnostics: Vec<VueProjectionDiagnostic>,
}

/// Execution inputs excluded from projection-request identity.
#[derive(Debug, Clone, Default)]
pub struct VueProjectionInputs {
    /// Host-selected block bytes for multi-unit IDE composition.
    pub block_content: RuntimeBlockContentInputs,
    /// Resolved Vue facts threaded beside the request.
    pub execution: VueExecutionInputs,
    /// Authoritative Vue macro semantics, when supplied.
    pub macros: VueMacroSemanticInput,
}

/// Typed Vue IDE projection refusal.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum VueProjectionError {
    /// Compile-unsupported refusal.
    Unsupported(CompileUnsupported),
    /// The request named a product other than the IDE companion.
    NotIdeOnly {
        /// Product the IDE projection will not emit.
        unexpected: ProductKind,
    },
    /// Parsed-core refusal that is not a [`CompileUnsupported`].
    Direct(DirectCompileError),
}

impl VueProjectionBackend {
    /// Adapter this backend answers to.
    #[must_use]
    pub fn adapter_id(&self) -> FrameworkAdapterId {
        VueCarrierCompiler.adapter_id()
    }

    /// Carrier language this backend projects.
    #[must_use]
    pub fn carrier_language_id(&self) -> LanguageId {
        VueCarrierCompiler.carrier_language_id()
    }
}

impl ProjectionBackend for VueProjectionBackend {
    type IdeCompanion = VueIdeCompanion;
    type PublicApi = ();
    type Declarations = ();
    type ParseArtifact = FrameworkParseArtifact;
    type Request = CompileRequest;
    type ExecutionInputs = VueProjectionInputs;
    type Error = VueProjectionError;

    fn project_ide(
        &self,
        source: &str,
        artifact: &FrameworkParseArtifact,
        request: &CompileRequest,
        inputs: &VueProjectionInputs,
    ) -> Result<VueIdeCompanion, VueProjectionError> {
        require_ide_only(request)?;
        let Some(parsed) = VueCarrierCompiler.parsed_sfc(artifact) else {
            return Err(no_ide());
        };
        let requested_profile = requested_vue_syntax_profile(request)?;
        let exact_source = artifact
            .inventory()
            .source_spaces()
            .first()
            .is_some_and(|space| space.bytes().as_ref() == source);
        if !exact_source || artifact.syntax_profile() != &requested_profile {
            return Err(no_ide());
        }

        if inputs.block_content.script.is_some() || inputs.block_content.script_setup.is_some() {
            return Err(block_content_unavailable());
        }
        if inputs.block_content.template.is_some()
            && parsed.script().is_some()
            && parsed.script_setup().is_none()
        {
            return Err(block_content_unavailable());
        }

        let mut lowering = StandaloneCompiler
            .lower_vue_from_parsed(
                source,
                parsed,
                request,
                &inputs.execution,
                &inputs.macros,
                &inputs.block_content,
            )
            .map_err(map_direct)?;
        let tsx = lowering
            .result
            .tsx
            .take()
            .ok_or(VueProjectionError::Unsupported(
                CompileUnsupported::TargetMissingIde,
            ))?;
        match inputs.block_content.template.as_ref() {
            None => {
                let (space, artifact_token) = RuntimeOutputDescriptor::carrier_source(source);
                Ok(companion_from_tsx(
                    tsx,
                    wrap_projection_diagnostics(
                        lowering.result.errors,
                        lowering.selected_diagnostics,
                        &space,
                        None,
                    ),
                    &[(space.as_str(), artifact_token.as_str())],
                ))
            }
            Some(selected) => assemble_selected_template(
                source,
                tsx,
                lowering.result.errors,
                lowering.selected_diagnostics,
                selected,
            ),
        }
    }
}

fn requested_vue_syntax_profile(
    request: &CompileRequest,
) -> Result<SyntaxProfileId, VueProjectionError> {
    let vue = request.vue().ok_or_else(no_ide)?;
    let mut options = ParseOptions::vue_standard();

    if let Some(delimiters) = &vue.delimiters {
        options.delimiters = delimiters.clone();
    }
    options.custom_elements.clone_from(&vue.is_custom_element);

    syntax_profile_id_for(&FileLanguage::vue(), &options).map_err(|_| no_ide())
}

fn no_ide() -> VueProjectionError {
    VueProjectionError::Unsupported(CompileUnsupported::NoIdeProjection {
        adapter_id: FrameworkAdapterId::vue(),
    })
}

fn block_content_unavailable() -> VueProjectionError {
    VueProjectionError::Unsupported(CompileUnsupported::BlockContentIdeUnavailable {
        adapter_id: FrameworkAdapterId::vue(),
    })
}

fn require_ide_only(request: &CompileRequest) -> Result<(), VueProjectionError> {
    if request.vue().is_none() {
        return Err(no_ide());
    }
    let mut saw_ide = false;
    for product in request.products() {
        match product.kind() {
            ProductKind::IdeCompanion => saw_ide = true,
            unexpected => return Err(VueProjectionError::NotIdeOnly { unexpected }),
        }
    }
    if !saw_ide {
        return Err(VueProjectionError::Unsupported(
            CompileUnsupported::TargetMissingIde,
        ));
    }
    Ok(())
}

fn map_direct(err: DirectCompileError) -> VueProjectionError {
    match err {
        DirectCompileError::Vue(error) => {
            VueProjectionError::Unsupported(CompileUnsupported::RequestExecutionRefused(error))
        }
        DirectCompileError::UnsupportedProduct(ProductKind::IdeCompanion) => {
            VueProjectionError::Unsupported(CompileUnsupported::TargetMissingIde)
        }
        DirectCompileError::UnsupportedProduct(kind) => {
            VueProjectionError::NotIdeOnly { unexpected: kind }
        }
        other => VueProjectionError::Direct(other),
    }
}

fn ide_output_from_tsx(tsx: VerterTsxBlock, declared: &[(&str, &str)]) -> IdeOutput {
    let output_descriptor = RuntimeOutputDescriptor::generated(
        &tsx.code,
        (!tsx.source_map.is_empty()).then_some(tsx.source_map.as_str()),
        declared,
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
}

fn wrap_projection_diagnostics(
    carrier_diagnostics: Vec<CompileDiagnostic>,
    selected_diagnostics: Vec<CompileDiagnostic>,
    carrier_token: &str,
    selected_token: Option<&str>,
) -> Vec<VueProjectionDiagnostic> {
    let mut diagnostics =
        Vec::with_capacity(carrier_diagnostics.len() + selected_diagnostics.len());
    diagnostics.extend(
        carrier_diagnostics
            .into_iter()
            .map(|diagnostic| VueProjectionDiagnostic {
                diagnostic,
                source_space_token: carrier_token.to_string(),
            }),
    );
    if let Some(selected_token) = selected_token {
        diagnostics.extend(selected_diagnostics.into_iter().map(|diagnostic| {
            VueProjectionDiagnostic {
                diagnostic,
                source_space_token: selected_token.to_string(),
            }
        }));
    }
    diagnostics
}

fn companion_from_tsx(
    tsx: VerterTsxBlock,
    diagnostics: Vec<VueProjectionDiagnostic>,
    declared: &[(&str, &str)],
) -> VueIdeCompanion {
    VueIdeCompanion {
        ide: ide_output_from_tsx(tsx, declared),
        diagnostics,
    }
}

fn assemble_selected_template(
    source: &str,
    mut shell: VerterTsxBlock,
    carrier_diagnostics: Vec<CompileDiagnostic>,
    selected_diagnostics: Vec<CompileDiagnostic>,
    selected: &crate::framework_common::RuntimeBlockContentInput,
) -> Result<VueIdeCompanion, VueProjectionError> {
    let hole = shell
        .generated_template_hole
        .clone()
        .ok_or_else(block_content_unavailable)?;
    let template_chunk = shell
        .generated_template_chunk
        .as_ref()
        .ok_or_else(block_content_unavailable)?;
    let (carrier_space, carrier_artifact) = RuntimeOutputDescriptor::carrier_source(source);
    let composed = compose_generated_chunk(
        "",
        GeneratedUnit {
            code: &shell.code,
            source_map: &shell.source_map,
            source_space: &carrier_space,
            source,
        },
        hole,
        GeneratedFragment {
            unit: GeneratedUnit {
                code: &template_chunk.code,
                source_map: &template_chunk.source_map,
                source_space: &selected.source_space_token,
                source: &selected.code,
            },
            range: 0..template_chunk.code.len() as u32,
        },
    )
    .ok_or_else(block_content_unavailable)?;
    shell.code = composed.code;
    shell.source_map = composed.source_map;
    shell.generated_template_hole = None;
    shell.generated_template_chunk = None;
    let declared = [
        (carrier_space.as_str(), carrier_artifact.as_str()),
        (
            selected.source_space_token.as_str(),
            selected.content_artifact_token.as_str(),
        ),
    ];
    Ok(companion_from_tsx(
        shell,
        wrap_projection_diagnostics(
            carrier_diagnostics,
            selected_diagnostics,
            &carrier_space,
            Some(&selected.source_space_token),
        ),
        &declared,
    ))
}

/// Typed Vue projection catalog row.
#[must_use]
pub fn vue_projection_backend_registration(
) -> TypedCapabilityRegistration<ProjectionCap<VueProjectionBackend>> {
    TypedCapabilityRegistration::register_projection::<VueSfcV3, _>(
        VueProjectionBackend.adapter_id(),
        VueProjectionBackend.carrier_language_id(),
        Present(VueProjectionBackend),
    )
}
