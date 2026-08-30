//! Svelte [`RuntimeCompilerBackend`] adapter.
//!
//! Emits runtime products over an already-admitted parse through the
//! parsed compiler core and a runtime-only [`CompileRequest`]. Catalog
//! lookup keys adapter × epoch × Runtime.

use verter_debug_assert::verter_debug_assert;
use verter_language::{
    syntax_profile_id_for, FileLanguage, FrameworkAdapterId, LanguageId, ParseOptions,
    SyntaxProfileId,
};

use crate::assembly::AssembledArtifact;
use crate::compile_request::{CompileRequest, ProductKind};
use crate::framework_common::capability::{Present, RuntimeCompilerBackend};
use crate::framework_common::catalog::{RuntimeCap, TypedCapabilityRegistration};
use crate::framework_common::{CarrierCompiler, FrameworkParseArtifact};
use crate::standalone::{
    DirectCompileError, DirectCompileOutput, StandaloneCompiler, SvelteExecutionInputs,
};

use super::carrier::SvelteCarrierCompiler;
use super::carrier_frontend::SvelteSfc5;

/// Svelte runtime compiler backend.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct SvelteRuntimeBackend;

/// Execution inputs excluded from runtime-request identity.
#[derive(Debug, Clone, Default)]
pub struct SvelteRuntimeInputs {
    /// Resolved Svelte facts threaded beside the request.
    pub execution: SvelteExecutionInputs,
}

/// Typed Svelte runtime refusal.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SvelteRuntimeError {
    /// The request named a product other than a runtime target.
    NotRuntimeOnly {
        /// Product the runtime backend will not emit.
        unexpected: ProductKind,
    },
    /// The admitted artifact is not a usable Svelte parse
    /// (`parsed_svelte(artifact)` is `None`). Not an empty product set —
    /// [`CompileRequest::new`] refuses that before this backend runs.
    UnusableParse,
    /// Requested source bytes do not match the admitted artifact.
    SourceMismatch,
    /// Requested syntax profile does not match the admitted artifact.
    ProfileMismatch,
    /// The request is not a Svelte compile request.
    FrameworkMismatch,
    /// Parsed-core refusal that is not a runtime topology or request refusal.
    Direct(DirectCompileError),
}

impl SvelteRuntimeBackend {
    /// Adapter this backend answers to.
    #[must_use]
    pub fn adapter_id(&self) -> FrameworkAdapterId {
        SvelteCarrierCompiler.adapter_id()
    }

    /// Carrier language this backend compiles.
    #[must_use]
    pub fn carrier_language_id(&self) -> LanguageId {
        SvelteCarrierCompiler.carrier_language_id()
    }
}

impl RuntimeCompilerBackend<SvelteSfc5> for SvelteRuntimeBackend {
    type RuntimeClient = AssembledArtifact;
    type RuntimeServer = AssembledArtifact;
    type ParseArtifact = FrameworkParseArtifact;
    type Request = CompileRequest;
    type ExecutionInputs = SvelteRuntimeInputs;
    type Error = SvelteRuntimeError;
    type Output = DirectCompileOutput;

    fn compile_runtime(
        &self,
        source: &str,
        artifact: &FrameworkParseArtifact,
        request: &CompileRequest,
        inputs: &SvelteRuntimeInputs,
    ) -> Result<DirectCompileOutput, SvelteRuntimeError> {
        require_runtime_only(request)?;
        let Some(parsed) = SvelteCarrierCompiler.parsed_svelte(artifact) else {
            return Err(SvelteRuntimeError::UnusableParse);
        };
        let requested_profile = requested_svelte_syntax_profile(request)?;
        let exact_source = artifact
            .inventory()
            .source_spaces()
            .first()
            .is_some_and(|space| space.bytes().as_ref() == source);
        if !exact_source {
            return Err(SvelteRuntimeError::SourceMismatch);
        }
        if artifact.syntax_profile() != &requested_profile {
            return Err(SvelteRuntimeError::ProfileMismatch);
        }

        StandaloneCompiler
            .compile_svelte_from_parsed(source, parsed, request, &inputs.execution)
            .map_err(map_parsed_runtime)
    }
}

fn svelte_standard_parse_options() -> ParseOptions {
    ParseOptions {
        svelte_loose: false,
        ..ParseOptions::default()
    }
}

fn requested_svelte_syntax_profile(
    request: &CompileRequest,
) -> Result<SyntaxProfileId, SvelteRuntimeError> {
    if request.svelte().is_none() {
        return Err(SvelteRuntimeError::FrameworkMismatch);
    }
    syntax_profile_id_for(&FileLanguage::svelte(), &svelte_standard_parse_options())
        .map_err(|_| SvelteRuntimeError::ProfileMismatch)
}

fn require_runtime_only(request: &CompileRequest) -> Result<(), SvelteRuntimeError> {
    if request.svelte().is_none() {
        return Err(SvelteRuntimeError::FrameworkMismatch);
    }
    for product in request.products() {
        match product.kind() {
            ProductKind::RuntimeClient | ProductKind::RuntimeServer => {}
            unexpected => return Err(SvelteRuntimeError::NotRuntimeOnly { unexpected }),
        }
    }
    let products_non_empty = !request.products().is_empty();
    verter_debug_assert!(
        products_non_empty,
        "CompileRequest construction refuses an empty product set"
    );
    Ok(())
}

fn map_parsed_runtime(err: DirectCompileError) -> SvelteRuntimeError {
    match err {
        DirectCompileError::FrameworkMismatch { .. } => SvelteRuntimeError::FrameworkMismatch,
        DirectCompileError::UnsupportedProduct(
            kind @ (ProductKind::RuntimeClient | ProductKind::RuntimeServer),
        ) => SvelteRuntimeError::Direct(DirectCompileError::UnsupportedProduct(kind)),
        DirectCompileError::UnsupportedProduct(kind) => {
            SvelteRuntimeError::NotRuntimeOnly { unexpected: kind }
        }
        DirectCompileError::Svelte(_)
        | DirectCompileError::SvelteFragment(_)
        | DirectCompileError::SvelteOption(_)
        | DirectCompileError::UnsupportedSvelteNamespace
        | DirectCompileError::Publish(_)
        // Vue construction/execution refusals are not Svelte-reachable
        // (`CompileRequest::new` refuses them). Leave them Direct — never
        // rewrite as a Svelte request-execution refusal.
        | DirectCompileError::Vue(_)
        | DirectCompileError::VueComposition(_)
        | DirectCompileError::StalePreparedInput { .. } => SvelteRuntimeError::Direct(err),
    }
}

/// Typed Svelte runtime catalog row.
#[must_use]
pub fn svelte_runtime_backend_registration(
) -> TypedCapabilityRegistration<RuntimeCap<SvelteRuntimeBackend>> {
    TypedCapabilityRegistration::register_runtime::<SvelteSfc5, _>(
        SvelteRuntimeBackend.adapter_id(),
        SvelteRuntimeBackend.carrier_language_id(),
        Present(SvelteRuntimeBackend),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::compile_request::CompileRequestError;

    #[test]
    fn vue_ssr_vapor_is_not_rewritten_as_a_svelte_request_execution_refusal() {
        let mapped = map_parsed_runtime(DirectCompileError::Vue(
            CompileRequestError::SsrVaporBackendUnsupported,
        ));
        match mapped {
            SvelteRuntimeError::Direct(DirectCompileError::Vue(
                CompileRequestError::SsrVaporBackendUnsupported,
            )) => {}
            other => panic!(
                "Vue SSR×vapor must stay Direct(Vue(...)), not a Svelte request-execution refusal, got {other:?}"
            ),
        }
    }

    #[test]
    fn vue_vapor_inline_is_not_rewritten_as_a_svelte_request_execution_refusal() {
        let mapped = map_parsed_runtime(DirectCompileError::Vue(
            CompileRequestError::VaporInlineNotYetImplemented,
        ));
        match mapped {
            SvelteRuntimeError::Direct(DirectCompileError::Vue(
                CompileRequestError::VaporInlineNotYetImplemented,
            )) => {}
            other => panic!(
                "Vue vapor×inline must stay Direct(Vue(...)), not a Svelte request-execution refusal, got {other:?}"
            ),
        }
    }

    fn classify(err: SvelteRuntimeError) -> &'static str {
        match err {
            SvelteRuntimeError::NotRuntimeOnly { .. } => "product",
            SvelteRuntimeError::UnusableParse => "parse",
            SvelteRuntimeError::SourceMismatch => "source",
            SvelteRuntimeError::ProfileMismatch => "profile",
            SvelteRuntimeError::FrameworkMismatch => "framework",
            SvelteRuntimeError::Direct(_) => "direct",
        }
    }

    #[test]
    fn vue_compile_request_errors_stay_direct_on_the_svelte_runtime_path() {
        for error in [
            CompileRequestError::SsrVaporBackendUnsupported,
            CompileRequestError::VaporInlineNotYetImplemented,
            CompileRequestError::InlineSsrUnsupported,
        ] {
            let mapped = map_parsed_runtime(DirectCompileError::Vue(error.clone()));
            assert_eq!(
                classify(mapped.clone()),
                "direct",
                "Vue {error:?} must not be representable as a Svelte request-execution refusal"
            );
            assert_eq!(
                mapped,
                SvelteRuntimeError::Direct(DirectCompileError::Vue(error))
            );
        }
    }

    #[test]
    fn svelte_possible_direct_errors_stay_direct() {
        assert_eq!(
            map_parsed_runtime(DirectCompileError::UnsupportedSvelteNamespace),
            SvelteRuntimeError::Direct(DirectCompileError::UnsupportedSvelteNamespace)
        );
        assert_eq!(
            map_parsed_runtime(DirectCompileError::UnsupportedProduct(
                ProductKind::RuntimeClient
            )),
            SvelteRuntimeError::Direct(DirectCompileError::UnsupportedProduct(
                ProductKind::RuntimeClient
            ))
        );
        assert_eq!(
            map_parsed_runtime(DirectCompileError::UnsupportedProduct(
                ProductKind::IdeCompanion
            )),
            SvelteRuntimeError::NotRuntimeOnly {
                unexpected: ProductKind::IdeCompanion
            }
        );
    }

    #[test]
    fn requested_svelte_profile_uses_admitted_parse_options_constructor() {
        let requested = svelte_standard_parse_options();
        assert_eq!(
            requested,
            ParseOptions::default(),
            "requested Svelte profile must mint from ParseOptions::default(), the admitted-artifact constructor"
        );
        assert_ne!(
            requested,
            ParseOptions {
                svelte_loose: false,
                ..ParseOptions::vue_standard()
            },
            "Vue delimiter defaults must not leak onto the Svelte runtime identity path"
        );
    }
}
