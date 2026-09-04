//! Native projection of the session's typed compile-request envelope.
//!
//! One direction only: a [`host::CompileRequestResponse`] (or the typed
//! failure that replaces it) becomes the JavaScript value the caller
//! receives. Nothing here decides anything about the compile — the
//! response is already complete when it arrives.

use napi::bindgen_prelude::*;
use napi::{Env, Error, Result, Status};
use verter_compiler::compile_request::{
    CapabilityCell, CompileRequestError, ProductKind, RuntimeStyleProcessing, VueOnlyAxis,
};
use verter_ffi::convert::{host_diagnostics_to_ffi, host_error_to_string};
use verter_protocol::types::{FfiDiagnostic, FfiDiagnosticArg};
use verter_session as host;

use crate::{
    ffi_err, host_node_kind_to_napi, NapiCompileRequestFailure, NapiCompileRequestProduct,
    NapiCompileRequestResponse, NapiCompileRequestVirtualNode, NapiCompileRequestsEntry,
    NapiDestructuredBinding, NapiDestructuredBlockMeta, NapiDiagnostic, NapiDiagnosticArg,
    NapiDiagnosticsSnapshot, NapiIdeResponse, NapiVirtualMeta,
};

pub(crate) fn compile_product_kind_to_str(kind: ProductKind) -> &'static str {
    match kind {
        ProductKind::RuntimeClient => "runtimeClient",
        ProductKind::RuntimeServer => "runtimeServer",
        ProductKind::IdeCompanion => "ideCompanion",
        ProductKind::PublicApi => "publicApi",
        ProductKind::Declarations => "declarations",
        ProductKind::Analysis => "analysis",
    }
}

pub(crate) fn compile_request_construction_refused(error: &CompileRequestError) -> String {
    let detail = match error {
        CompileRequestError::UnsupportedOption { option, .. } => {
            format!("unsupported option '{option}'")
        }
        CompileRequestError::MalformedOptionValue { option, value } => {
            format!("malformed value '{value}' for option '{option}'")
        }
        CompileRequestError::SsrVaporBackendUnsupported => {
            "SSR is unsupported with a Vapor backend".to_string()
        }
        CompileRequestError::VueOnlyAxisOnSvelteRequest(axis) => {
            format!(
                "Vue-only option '{}' is not admitted on a Svelte request",
                vue_only_axis_name(*axis)
            )
        }
        CompileRequestError::InlineSsrUnsupported => {
            "inline assembly is unsupported with SSR".to_string()
        }
        CompileRequestError::VaporInlineNotYetImplemented => {
            "inline assembly is not implemented for Vapor".to_string()
        }
        CompileRequestError::CapabilityUnsupported(cell) => {
            format!("unsupported capability '{}'", capability_cell_name(*cell))
        }
        CompileRequestError::EmptyProductSet => "product set is empty".to_string(),
        CompileRequestError::DuplicateProduct(kind) => {
            format!("duplicate product '{}'", compile_product_kind_to_str(*kind))
        }
        CompileRequestError::ConflictingRuntimeStyleProcessing { first, conflicting } => {
            format!(
                "conflicting runtime styleProcessing values '{}' and '{}'",
                runtime_style_processing_name(*first),
                runtime_style_processing_name(*conflicting)
            )
        }
        CompileRequestError::RuntimeStyleProcessingUnsupported {
            framework,
            requested,
        } => format!(
            "runtime styleProcessing '{}' is unsupported for {framework}",
            runtime_style_processing_name(*requested)
        ),
        CompileRequestError::FrameworkMismatch { expected, actual } => {
            format!("compile request framework '{actual}' does not match '{expected}'")
        }
    };
    format!("compile request construction refused: {detail}")
}

fn vue_only_axis_name(axis: VueOnlyAxis) -> &'static str {
    match axis {
        VueOnlyAxis::TypesModuleName => "typesModuleName",
        VueOnlyAxis::ConditionalRootNarrowing => "conditionalRootNarrowing",
        VueOnlyAxis::StrictSlots => "strictSlots",
    }
}

fn runtime_style_processing_name(value: RuntimeStyleProcessing) -> &'static str {
    match value {
        RuntimeStyleProcessing::Complete => "complete",
        RuntimeStyleProcessing::AuthoredOnly => "authored-only",
    }
}

fn capability_cell_name(cell: CapabilityCell) -> &'static str {
    match cell {
        CapabilityCell::VueParseLocal => "VUE-PARSE-LOCAL",
        CapabilityCell::VueVdomClient => "VUE-VDOM-CLIENT",
        CapabilityCell::VueVaporClient => "VUE-VAPOR-CLIENT",
        CapabilityCell::VueSsr => "VUE-SSR",
        CapabilityCell::VueSsrVaporBackend => "VUE-SSR-VAPOR-BACKEND",
        CapabilityCell::VueMacroLocal => "VUE-MACRO-LOCAL",
        CapabilityCell::VueMacroImported => "VUE-MACRO-IMPORTED",
        CapabilityCell::VueScopedSlotted => "VUE-SCOPED-SLOTTED",
        CapabilityCell::VueCustomElement => "VUE-CUSTOM-ELEMENT",
        CapabilityCell::VueTemplateOptions => "VUE-TEMPLATE-OPTIONS",
        CapabilityCell::VueAsyncSetup => "VUE-ASYNC-SETUP",
        CapabilityCell::VuePublicApi => "VUE-PUBLIC-API",
        CapabilityCell::VueTsc => "VUE-TSC",
        CapabilityCell::VueDeclaration => "VUE-DECLARATION",
        CapabilityCell::VueCompatV2 => "VUE-COMPAT-V2",
        CapabilityCell::VueOtherVersion => "VUE-OTHER-VERSION",
        CapabilityCell::SveltePraseLocal => "SVELTE-PARSE-LOCAL",
        CapabilityCell::SvelteClientRunes => "SVELTE-CLIENT-RUNES",
        CapabilityCell::SvelteClientLegacy => "SVELTE-CLIENT-LEGACY",
        CapabilityCell::SvelteServerRunes => "SVELTE-SERVER-RUNES",
        CapabilityCell::SvelteServerLegacy => "SVELTE-SERVER-LEGACY",
        CapabilityCell::SvelteComponent => "SVELTE-COMPONENT",
        CapabilityCell::SvelteModule => "SVELTE-MODULE",
        CapabilityCell::SvelteSemanticCore => "SVELTE-SEMANTIC-CORE",
        CapabilityCell::SvelteCustomElement => "SVELTE-CUSTOM-ELEMENT",
        CapabilityCell::SvelteAsyncExperimental => "SVELTE-ASYNC-EXPERIMENTAL",
        CapabilityCell::SvelteHydration => "SVELTE-HYDRATION",
        CapabilityCell::SveltePublicApi => "SVELTE-PUBLIC-API",
        CapabilityCell::SvelteTsc => "SVELTE-TSC",
        CapabilityCell::SvelteDeclaration => "SVELTE-DECLARATION",
        CapabilityCell::SvelteHmr => "SVELTE-HMR",
        CapabilityCell::SvelteCompatApi4 => "SVELTE-COMPAT-API4",
        CapabilityCell::SvelteOfficialAst => "SVELTE-OFFICIAL-AST",
        CapabilityCell::SvelteOtherVersion => "SVELTE-OTHER-VERSION",
    }
}

// A diagnostic and its arguments project through the shared FFI converter
// (`verter_ffi::convert::host_diagnostics_to_ffi`) so NAPI and WASM cannot
// diverge on severity spelling, UTF-16 span mapping, or argument shape —
// this crate only re-shapes the FFI struct into its `#[napi(object)]`
// mirror, never re-derives the conversion.

impl From<FfiDiagnosticArg> for NapiDiagnosticArg {
    fn from(argument: FfiDiagnosticArg) -> Self {
        NapiDiagnosticArg {
            kind: argument.kind,
            boolean: argument.boolean,
            unsigned: argument.unsigned,
            signed: argument.signed,
            text: argument.text,
            spanStart: argument.span_start,
            spanEnd: argument.span_end,
        }
    }
}

impl From<FfiDiagnostic> for NapiDiagnostic {
    fn from(diagnostic: FfiDiagnostic) -> Self {
        NapiDiagnostic {
            severity: diagnostic.severity,
            code: diagnostic.code,
            message: diagnostic.message,
            spanStart: diagnostic.span_start,
            spanEnd: diagnostic.span_end,
            arguments: diagnostic.arguments.into_iter().map(Into::into).collect(),
        }
    }
}

pub(crate) fn host_diagnostic_to_napi(
    diagnostic: &host::HostDiagnostic,
    source: Option<&str>,
) -> NapiDiagnostic {
    verter_ffi::convert::host_diagnostic_to_ffi(diagnostic, source).into()
}

pub(crate) fn host_diagnostics_to_napi(
    input: &host::DiagnosticsSnapshot,
    source: Option<&str>,
) -> NapiDiagnosticsSnapshot {
    let snapshot = host_diagnostics_to_ffi(input, source);
    NapiDiagnosticsSnapshot {
        diagnostics: snapshot.diagnostics.into_iter().map(Into::into).collect(),
        hasErrors: snapshot.has_errors,
    }
}

fn empty_diagnostics_snapshot() -> NapiDiagnosticsSnapshot {
    NapiDiagnosticsSnapshot {
        diagnostics: Vec::new(),
        hasErrors: false,
    }
}

pub(crate) fn host_ide_to_napi(input: host::IdeResponse, source: &str) -> NapiIdeResponse {
    let destructured_block = input.destructured_block.as_ref().map(|meta| {
        let bindings: Vec<verter_ffi::convert::DestructuredBindingInput<'_>> = meta
            .bindings
            .iter()
            .map(|binding| verter_ffi::convert::DestructuredBindingInput {
                name: &binding.name,
                source_start: binding.source_span.start,
                source_end: binding.source_span.end,
            })
            .collect();
        let ffi = verter_ffi::convert::convert_destructured_block_meta(
            &bindings,
            meta.block_start,
            meta.block_end,
            source,
            &input.code,
            verter_ffi::convert::OffsetEncoding::Utf16,
        );
        NapiDestructuredBlockMeta {
            bindings: ffi
                .bindings
                .into_iter()
                .map(|binding| NapiDestructuredBinding {
                    name: binding.name,
                    sourceStart: binding.source_start,
                    sourceEnd: binding.source_end,
                })
                .collect(),
            blockStart: ffi.block_start,
            blockEnd: ffi.block_end,
        }
    });
    NapiIdeResponse {
        code: input.code.to_string(),
        sourceMap: input.source_map.map(|map| map.to_string()),
        isJsx: input.is_jsx,
        destructuredBlock: destructured_block,
    }
}

pub(crate) fn compile_request_response_to_napi(
    input: host::CompileRequestResponse,
    source: &str,
) -> Result<NapiCompileRequestResponse> {
    let host::CompileRequestResponse {
        canonical_id,
        diagnostics,
        products,
    } = input;
    let products = products
        .into_iter()
        .map(|product| match product {
            host::CompiledProduct::Runtime { kind, nodes } => {
                let kind = match kind {
                    ProductKind::RuntimeClient => "runtimeClient",
                    ProductKind::RuntimeServer => "runtimeServer",
                    kind @ (ProductKind::IdeCompanion
                    | ProductKind::PublicApi
                    | ProductKind::Declarations
                    | ProductKind::Analysis) => {
                        return Err(ffi_err(format!(
                            "refused host compile request for {canonical_id}: the {} product published a runtime output row, which has no runtime wire tag",
                            compile_product_kind_to_str(kind)
                        )));
                    }
                };
                Ok(NapiCompileRequestProduct {
                    kind: kind.to_string(),
                    nodes: Some(
                        nodes
                            .into_iter()
                            .map(|node| NapiCompileRequestVirtualNode {
                                node: host_node_kind_to_napi(&node.node),
                                code: node.code.to_string(),
                                sourceMap: node.source_map.map(|map| map.to_string()),
                                lang: node.lang,
                                meta: NapiVirtualMeta {
                                    scopeId: node.meta.scope_id,
                                    blockType: node.meta.block_type,
                                },
                            })
                            .collect(),
                    ),
                    ide: None,
                    analysis: None,
                })
            }
            host::CompiledProduct::Ide(ide) => Ok(NapiCompileRequestProduct {
                kind: "ideCompanion".to_string(),
                nodes: None,
                ide: Some(host_ide_to_napi(ide, source)),
                analysis: None,
            }),
            host::CompiledProduct::Analysis(analysis) => {
                let analysis = serde_json::to_string(analysis.as_ref()).map_err(|error| {
                    Error::new(
                        Status::GenericFailure,
                        format!("analysis serialization error: {error}"),
                    )
                })?;
                Ok(NapiCompileRequestProduct {
                    kind: "analysis".to_string(),
                    nodes: None,
                    ide: None,
                    analysis: Some(analysis),
                })
            }
        })
        .collect::<Result<Vec<_>>>()?;
    Ok(NapiCompileRequestResponse {
        canonicalId: canonical_id,
        diagnostics: host_diagnostics_to_napi(&diagnostics, Some(source)),
        products,
    })
}

#[doc(hidden)]
pub fn compile_request_failure_to_napi(
    failure: host::CompileRequestFailure,
    fallback_canonical_id: String,
    source: Option<&str>,
) -> NapiCompileRequestFailure {
    let diagnostic_message = |diagnostics: &host::DiagnosticsSnapshot, fallback: &str| {
        diagnostics
            .diagnostics
            .iter()
            .find(|diagnostic| diagnostic.severity == host::HostSeverity::Error)
            .or_else(|| diagnostics.diagnostics.first())
            .map(|diagnostic| diagnostic.message.clone())
            .unwrap_or_else(|| fallback.to_string())
    };

    match failure {
        host::CompileRequestFailure::Host(error) => {
            let message = host_error_to_string(&error);
            let diagnostics = match &error {
                host::HostError::CompileError(failure) => {
                    host_diagnostics_to_napi(&failure.diagnostics, source)
                }
                _ => empty_diagnostics_snapshot(),
            };
            NapiCompileRequestFailure {
                kind: "host".to_string(),
                canonicalId: fallback_canonical_id,
                message,
                diagnostics,
                requestedFramework: None,
                registeredFramework: None,
                productKind: None,
                diagnosticCode: None,
            }
        }
        host::CompileRequestFailure::FrameworkMismatch {
            canonical_id,
            requested,
            registered,
        } => NapiCompileRequestFailure {
            kind: "frameworkMismatch".to_string(),
            canonicalId: canonical_id.clone(),
            message: format!(
                "compile request for '{canonical_id}' names framework '{requested}', but the registered source uses '{registered}'"
            ),
            diagnostics: empty_diagnostics_snapshot(),
            requestedFramework: Some(requested.to_string()),
            registeredFramework: Some(registered),
            productKind: None,
            diagnosticCode: None,
        },
        host::CompileRequestFailure::UnsupportedProduct {
            canonical_id,
            kind,
            diagnostics,
        } => {
            let message = diagnostic_message(&diagnostics, "compile product is unsupported");
            NapiCompileRequestFailure {
                kind: "unsupportedProduct".to_string(),
                canonicalId: canonical_id,
                message,
                diagnostics: host_diagnostics_to_napi(&diagnostics, source),
                requestedFramework: None,
                registeredFramework: None,
                productKind: Some(compile_product_kind_to_str(kind).to_string()),
                diagnosticCode: None,
            }
        }
        host::CompileRequestFailure::ProductNotProduced {
            canonical_id,
            kind,
            diagnostics,
        } => {
            let message = diagnostic_message(&diagnostics, "compile product was not produced");
            NapiCompileRequestFailure {
                kind: "productNotProduced".to_string(),
                canonicalId: canonical_id,
                message,
                diagnostics: host_diagnostics_to_napi(&diagnostics, source),
                requestedFramework: None,
                registeredFramework: None,
                productKind: Some(compile_product_kind_to_str(kind).to_string()),
                diagnosticCode: None,
            }
        }
        host::CompileRequestFailure::RuntimeSurfaceRefused {
            canonical_id,
            diagnostic_code,
            message,
            diagnostics,
        } => NapiCompileRequestFailure {
            kind: "runtimeSurfaceRefused".to_string(),
            canonicalId: canonical_id,
            message,
            diagnostics: host_diagnostics_to_napi(&diagnostics, source),
            requestedFramework: None,
            registeredFramework: None,
            productKind: None,
            diagnosticCode: Some(diagnostic_code),
        },
        host::CompileRequestFailure::Refused {
            canonical_id,
            diagnostics,
        } => {
            let message = diagnostic_message(&diagnostics, "compile request was refused");
            NapiCompileRequestFailure {
                kind: "refused".to_string(),
                canonicalId: canonical_id,
                message,
                diagnostics: host_diagnostics_to_napi(&diagnostics, source),
                requestedFramework: None,
                registeredFramework: None,
                productKind: None,
                diagnosticCode: None,
            }
        }
    }
}

pub(crate) fn binding_failure_to_napi(
    canonical_id: String,
    message: String,
) -> NapiCompileRequestFailure {
    NapiCompileRequestFailure {
        kind: "binding".to_string(),
        canonicalId: canonical_id,
        message,
        diagnostics: empty_diagnostics_snapshot(),
        requestedFramework: None,
        registeredFramework: None,
        productKind: None,
        diagnosticCode: None,
    }
}

pub(crate) fn compile_request_failure_status(failure: &host::CompileRequestFailure) -> Status {
    match failure {
        host::CompileRequestFailure::Host(error) => crate::host_error_status(error),
        host::CompileRequestFailure::FrameworkMismatch { .. }
        | host::CompileRequestFailure::UnsupportedProduct { .. }
        | host::CompileRequestFailure::Refused { .. } => Status::InvalidArg,
        host::CompileRequestFailure::ProductNotProduced { .. }
        | host::CompileRequestFailure::RuntimeSurfaceRefused { .. } => Status::GenericFailure,
    }
}

pub(crate) fn compile_request_error(
    env: &Env,
    status: Status,
    failure: NapiCompileRequestFailure,
) -> Result<Error> {
    let NapiCompileRequestFailure {
        kind,
        canonicalId,
        message,
        diagnostics,
        requestedFramework,
        registeredFramework,
        productKind,
        diagnosticCode,
    } = failure;
    let mut error = env.create_error(Error::new(status, message))?;
    error.set_named_property("kind", kind)?;
    error.set_named_property("canonicalId", canonicalId)?;
    error.set_named_property("diagnostics", diagnostics)?;
    if let Some(value) = requestedFramework {
        error.set_named_property("requestedFramework", value)?;
    }
    if let Some(value) = registeredFramework {
        error.set_named_property("registeredFramework", value)?;
    }
    if let Some(value) = productKind {
        error.set_named_property("productKind", value)?;
    }
    if let Some(value) = diagnosticCode {
        error.set_named_property("diagnosticCode", value)?;
    }
    Ok(Error::from((&error).into_unknown(env)?))
}

pub(crate) fn binding_failure_entry(
    canonical_id: String,
    message: String,
) -> NapiCompileRequestsEntry {
    NapiCompileRequestsEntry {
        canonicalId: canonical_id.clone(),
        response: None,
        failure: Some(binding_failure_to_napi(canonical_id, message)),
    }
}

pub(crate) fn failure_canonical_id<'a>(
    failure: &'a host::CompileRequestFailure,
    fallback: &'a str,
) -> &'a str {
    match failure {
        host::CompileRequestFailure::Host(_) => fallback,
        host::CompileRequestFailure::FrameworkMismatch { canonical_id, .. }
        | host::CompileRequestFailure::UnsupportedProduct { canonical_id, .. }
        | host::CompileRequestFailure::ProductNotProduced { canonical_id, .. }
        | host::CompileRequestFailure::RuntimeSurfaceRefused { canonical_id, .. }
        | host::CompileRequestFailure::Refused { canonical_id, .. } => canonical_id,
    }
}
