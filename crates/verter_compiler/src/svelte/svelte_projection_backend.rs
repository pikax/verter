//! Svelte [`ProjectionBackend`] adapter.
//!
//! Projects the IDE companion over an already-admitted parse through the
//! parsed compiler core and an IDE-only [`CompileRequest`]. Catalog lookup
//! keys adapter × epoch × Projection.

use verter_language::{
    syntax_profile_id_for, FileLanguage, FrameworkAdapterId, LanguageId, ParseOptions,
    SyntaxProfileId,
};

use crate::compile_request::{CompileRequest, ProductKind};
use crate::framework_common::capability::{Present, ProjectionBackend};
use crate::framework_common::carrier_compiler::{
    CompileUnsupported, IdeOutput, RuntimeOutputDescriptor,
};
use crate::framework_common::catalog::{ProjectionCap, TypedCapabilityRegistration};
use crate::framework_common::{CarrierCompiler, FrameworkParseArtifact};
use crate::standalone::{DirectCompileError, StandaloneCompiler};
use crate::svelte::ide::SvelteIdeUnsupportedDiagnostic;

use super::carrier::SvelteCarrierCompiler;
use super::carrier_frontend::SvelteSfc5;

/// Svelte IDE projection backend.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct SvelteProjectionBackend;

/// Compile diagnostic tagged with the source space it was emitted against.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SvelteProjectionDiagnostic {
    /// The compile diagnostic.
    pub diagnostic: SvelteIdeUnsupportedDiagnostic,
    /// Source-space token for this diagnostic's origin.
    pub source_space_token: String,
}

/// IDE companion plus compile diagnostics retained from the parsed core.
#[derive(Debug, Clone)]
pub struct SvelteIdeCompanion {
    /// Generated TSX/JSX companion.
    pub ide: IdeOutput,
    /// Non-fatal compile diagnostics from the parsed core, each tagged with
    /// the source space they were emitted against.
    pub diagnostics: Vec<SvelteProjectionDiagnostic>,
}

/// Execution inputs excluded from projection-request identity.
#[derive(Debug, Clone, Default)]
pub struct SvelteProjectionInputs;

/// Typed Svelte IDE projection refusal.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SvelteProjectionError {
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

impl SvelteProjectionBackend {
    /// Adapter this backend answers to.
    #[must_use]
    pub fn adapter_id(&self) -> FrameworkAdapterId {
        SvelteCarrierCompiler.adapter_id()
    }

    /// Carrier language this backend projects.
    #[must_use]
    pub fn carrier_language_id(&self) -> LanguageId {
        SvelteCarrierCompiler.carrier_language_id()
    }
}

impl ProjectionBackend for SvelteProjectionBackend {
    type IdeCompanion = SvelteIdeCompanion;
    type PublicApi = ();
    type Declarations = ();
    type ParseArtifact = FrameworkParseArtifact;
    type Request = CompileRequest;
    type ExecutionInputs = SvelteProjectionInputs;
    type Error = SvelteProjectionError;

    fn project_ide(
        &self,
        source: &str,
        artifact: &FrameworkParseArtifact,
        request: &CompileRequest,
        _inputs: &SvelteProjectionInputs,
    ) -> Result<SvelteIdeCompanion, SvelteProjectionError> {
        require_ide_only(request)?;
        let Some(parsed) = SvelteCarrierCompiler.parsed_svelte(artifact) else {
            return Err(no_ide());
        };
        let requested_profile = requested_svelte_syntax_profile(request)?;
        let exact_source = artifact
            .inventory()
            .source_spaces()
            .first()
            .is_some_and(|space| space.bytes().as_ref() == source);
        if !exact_source || artifact.syntax_profile() != &requested_profile {
            return Err(no_ide());
        }

        let lowering = StandaloneCompiler
            .lower_svelte_from_parsed(source, parsed, request)
            .map_err(map_direct)?;
        let (space, _) = RuntimeOutputDescriptor::carrier_source(source);
        Ok(SvelteIdeCompanion {
            ide: lowering.ide,
            diagnostics: wrap_projection_diagnostics(lowering.diagnostics, &space),
        })
    }
}

fn svelte_standard_parse_options() -> ParseOptions {
    ParseOptions {
        svelte_loose: false,
        ..ParseOptions::vue_standard()
    }
}

fn requested_svelte_syntax_profile(
    request: &CompileRequest,
) -> Result<SyntaxProfileId, SvelteProjectionError> {
    if request.svelte().is_none() {
        return Err(no_ide());
    }
    syntax_profile_id_for(&FileLanguage::svelte(), &svelte_standard_parse_options())
        .map_err(|_| no_ide())
}

fn no_ide() -> SvelteProjectionError {
    SvelteProjectionError::Unsupported(CompileUnsupported::NoIdeProjection {
        adapter_id: FrameworkAdapterId::svelte(),
    })
}

fn require_ide_only(request: &CompileRequest) -> Result<(), SvelteProjectionError> {
    if request.svelte().is_none() {
        return Err(no_ide());
    }
    let mut saw_ide = false;
    for product in request.products() {
        match product.kind() {
            ProductKind::IdeCompanion => saw_ide = true,
            unexpected => return Err(SvelteProjectionError::NotIdeOnly { unexpected }),
        }
    }
    if !saw_ide {
        return Err(SvelteProjectionError::Unsupported(
            CompileUnsupported::TargetMissingIde,
        ));
    }
    Ok(())
}

fn map_direct(err: DirectCompileError) -> SvelteProjectionError {
    match err {
        DirectCompileError::UnsupportedProduct(ProductKind::IdeCompanion) => {
            SvelteProjectionError::Unsupported(CompileUnsupported::TargetMissingIde)
        }
        DirectCompileError::UnsupportedProduct(kind) => {
            SvelteProjectionError::NotIdeOnly { unexpected: kind }
        }
        other => SvelteProjectionError::Direct(other),
    }
}

fn wrap_projection_diagnostics(
    diagnostics: Vec<SvelteIdeUnsupportedDiagnostic>,
    carrier_token: &str,
) -> Vec<SvelteProjectionDiagnostic> {
    diagnostics
        .into_iter()
        .map(|diagnostic| SvelteProjectionDiagnostic {
            diagnostic,
            source_space_token: carrier_token.to_string(),
        })
        .collect()
}

/// Typed Svelte projection catalog row.
#[must_use]
pub fn svelte_projection_backend_registration(
) -> TypedCapabilityRegistration<ProjectionCap<SvelteProjectionBackend>> {
    TypedCapabilityRegistration::register_projection::<SvelteSfc5, _>(
        SvelteProjectionBackend.adapter_id(),
        SvelteProjectionBackend.carrier_language_id(),
        Present(SvelteProjectionBackend),
    )
}
