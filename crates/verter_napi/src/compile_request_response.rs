//! Native projection of the session's typed compile-request envelope.
//!
//! One direction only: a [`host::CompileRequestResponse`] (or the typed
//! failure that replaces it) becomes the JavaScript value the caller
//! receives. Nothing here decides anything about the compile — the
//! response is already complete when it arrives.
//!
//! The JS-facing unions of that value — the product row, the typed failure
//! and the batch entry — are declared here through
//! [`js_object_union`](crate::host_compile_request_ts::js_object_union), so
//! the declaration list that builds the JavaScript object is the list the
//! published `host-compile-response.generated.ts` is rendered from. A new
//! product arm is one more row in [`NapiCompiledProduct`]'s list: that row
//! is its serialisation and its declaration at once.
//!
//! Serialisation stays native rather than going through serde: every arm
//! field is a napi value in its own right (a `#[napi(object)]` struct, a
//! string, `null`) and is written onto a fresh object tag-first and in
//! declaration order — the calls, the keys and the key order a
//! `#[napi(object)]` struct with those fields produces, so every value a
//! caller already reads is unchanged, absent optional fields stay absent,
//! and no nested object pays for a serde round trip. A serde projection
//! (`Env::to_js_value`) would have needed a `Serialize` mirror and skip
//! rules on every leaf object and a property copy onto the thrown `Error`.
//! The one field that does cross through serde is the missing-input list:
//! a `verter_protocol` wire DTO that the browser binding serialises the
//! same way.

use napi::bindgen_prelude::*;
use napi::{Env, Error, Result, Status};
use ts_rs::{Config, TypeVisitor, TS};
use verter_compiler::compile_request::{CompileRequestError, ProductKind};
use verter_ffi::convert::{compile_dependency_gaps_to_ffi, host_error_to_string};
use verter_protocol::compile_response::FfiMissingInput;
use verter_protocol::types::{FfiDiagnostic, SpanEncodingTag, Utf8SpanPayload};
use verter_session as host;

use crate::host_compile_request_ts::js_object_union;
use crate::{
    ffi_err, host_node_kind_to_napi, NapiCompileRequestResponse, NapiCompileRequestVirtualNode,
    NapiDestructuredBinding, NapiDestructuredBlockMeta, NapiDiagnostic, NapiDiagnosticsSnapshot,
    NapiIdeResponse, NapiVirtualMeta, NapiVirtualNodeKind,
};

// The shared host types every native projection reaches. Their published
// declarations are written in `packages/native/host-types.ts`, which the
// generated response imports them from.
verter_protocol::ts_declared_elsewhere! {
    NapiDiagnosticsSnapshot => "HostDiagnosticsSnapshot",
    NapiIdeResponse => "HostIdeResponse",
    NapiVirtualMeta => "HostVirtualMeta",
    NapiVirtualNodeKind => "HostVirtualNodeKind",
}

/// A slot that is always `null`: the half of an untagged union's arm that
/// says which arm it is.
pub struct JsNull;

impl ToNapiValue for JsNull {
    unsafe fn to_napi_value(env: sys::napi_env, _: Self) -> Result<sys::napi_value> {
        Null::to_napi_value(env, Null)
    }
}

impl TS for JsNull {
    type WithoutGenerics = Self;
    type OptionInnerType = Self;

    fn name(_: &Config) -> String {
        String::from("null")
    }

    fn inline(cfg: &Config) -> String {
        Self::name(cfg)
    }
}

/// A wire DTO that crosses the native boundary through its `serde`
/// serialisation, published under the DTO's own TypeScript name.
pub struct Serialized<T>(pub T);

impl<T: serde::Serialize> ToNapiValue for Serialized<T> {
    unsafe fn to_napi_value(env: sys::napi_env, val: Self) -> Result<sys::napi_value> {
        Ok(Env::from_raw(env).to_js_value(&val.0)?.raw())
    }
}

impl<T: TS + 'static> TS for Serialized<T> {
    type WithoutGenerics = Self;
    type OptionInnerType = Self;

    fn name(cfg: &Config) -> String {
        T::name(cfg)
    }

    fn inline(cfg: &Config) -> String {
        T::inline(cfg)
    }

    fn visit_generics(v: &mut impl TypeVisitor)
    where
        Self: 'static,
    {
        v.visit::<T>();
        T::visit_generics(v);
    }
}

/// A requested product kind, as a failure names it: the request's own
/// product tag, published as the request union's tag type rather than as a
/// second spelling of the same list.
pub struct RequestedProductKind(pub ProductKind);

impl ToNapiValue for RequestedProductKind {
    unsafe fn to_napi_value(env: sys::napi_env, val: Self) -> Result<sys::napi_value> {
        <&str as ToNapiValue>::to_napi_value(env, val.0.wire_tag())
    }
}

impl TS for RequestedProductKind {
    type WithoutGenerics = Self;
    type OptionInnerType = Self;

    fn name(_: &Config) -> String {
        String::from("HostRequestedProduct[\"kind\"]")
    }

    fn inline(cfg: &Config) -> String {
        Self::name(cfg)
    }
}

js_object_union! {
    /// One row of a response's product list, one-to-one with a requested
    /// product kind and in request order, tagged with the same spelling the
    /// request used.
    pub enum NapiCompiledProduct tagged "kind" as "HostCompiledProduct" {
        RuntimeClient {
            nodes: Vec<NapiCompileRequestVirtualNode>,
        } => "runtimeClient" as "HostRuntimeCompiledProduct",
        RuntimeServer {
            nodes: Vec<NapiCompileRequestVirtualNode>,
        } => "runtimeServer" as "HostRuntimeCompiledProduct",
        /// The IDE projection row, nested under its own key.
        IdeCompanion {
            ide: NapiIdeResponse,
        } => "ideCompanion" as "HostIdeCompiledProduct",
        /// `analysis` is a JSON-stringified `TemplateAnalysisSnapshot`
        /// (template components, binding occurrences, unresolved bindings,
        /// defined slots, template refs). It is a different, narrower payload
        /// than `getAnalysis()`, which returns a JSON-stringified
        /// `FileAnalysisSnapshot` (script-level imports, bindings, macros, and
        /// macro type dependencies) — the two share their JSON-string
        /// transport, not their shape.
        Analysis {
            analysis: String,
        } => "analysis" as "HostAnalysisCompiledProduct",
    }
}

js_object_union! {
    /// Typed terminal failure for one request: what `compileRequest` throws
    /// (as an `Error` carrying these fields) and what a `compileRequests`
    /// entry carries in `failure`.
    pub enum NapiCompileRequestFailure tagged "kind" as "HostCompileFailure" {
        /// A failure that carries nothing beyond its message and
        /// diagnostics: a host error, a refusal by the compiler's own rules,
        /// or a refusal at this binding.
        Host {
            canonicalId: String,
            message: String,
            diagnostics: NapiDiagnosticsSnapshot,
        } => "host" as "HostCompilePlainFailure",
        /// The request named a framework other than the registered source's.
        FrameworkMismatch {
            canonicalId: String,
            message: String,
            diagnostics: NapiDiagnosticsSnapshot,
            requestedFramework: String,
            registeredFramework: String,
        } => "frameworkMismatch" as "HostCompileFrameworkMismatchFailure",
        /// A requested product the compile does not support, or did not
        /// produce.
        UnsupportedProduct {
            canonicalId: String,
            message: String,
            diagnostics: NapiDiagnosticsSnapshot,
            productKind: RequestedProductKind,
        } => "unsupportedProduct" as "HostCompileProductFailure",
        ProductNotProduced {
            canonicalId: String,
            message: String,
            diagnostics: NapiDiagnosticsSnapshot,
            productKind: RequestedProductKind,
        } => "productNotProduced" as "HostCompileProductFailure",
        /// The runtime surface refused the compile; `diagnosticCode` names
        /// the refusing rule.
        RuntimeSurfaceRefused {
            canonicalId: String,
            message: String,
            diagnostics: NapiDiagnosticsSnapshot,
            diagnosticCode: String,
        } => "runtimeSurfaceRefused" as "HostCompileRuntimeSurfaceFailure",
        Refused {
            canonicalId: String,
            message: String,
            diagnostics: NapiDiagnosticsSnapshot,
        } => "refused" as "HostCompilePlainFailure",
        /// The compile could not prove its macro type dependencies present
        /// or absent from the inputs this host holds. Nothing was published.
        /// `missingInputs` names each input it could not establish, in the
        /// host's order, and is never empty.
        DependenciesIncomplete {
            canonicalId: String,
            message: String,
            diagnostics: NapiDiagnosticsSnapshot,
            missingInputs: Serialized<Vec<FfiMissingInput>>,
        } => "dependenciesIncomplete" as "HostCompileDependenciesIncompleteFailure",
        Binding {
            canonicalId: String,
            message: String,
            diagnostics: NapiDiagnosticsSnapshot,
        } => "binding" as "HostCompilePlainFailure",
    }
}

impl NapiCompileRequestFailure {
    /// The failure's human-readable message.
    pub fn message(&self) -> &str {
        match self {
            Self::Host { message, .. }
            | Self::FrameworkMismatch { message, .. }
            | Self::UnsupportedProduct { message, .. }
            | Self::ProductNotProduced { message, .. }
            | Self::RuntimeSurfaceRefused { message, .. }
            | Self::Refused { message, .. }
            | Self::DependenciesIncomplete { message, .. }
            | Self::Binding { message, .. } => message,
        }
    }

    /// The failure's diagnostics.
    pub fn diagnostics(&self) -> &NapiDiagnosticsSnapshot {
        match self {
            Self::Host { diagnostics, .. }
            | Self::FrameworkMismatch { diagnostics, .. }
            | Self::UnsupportedProduct { diagnostics, .. }
            | Self::ProductNotProduced { diagnostics, .. }
            | Self::RuntimeSurfaceRefused { diagnostics, .. }
            | Self::Refused { diagnostics, .. }
            | Self::DependenciesIncomplete { diagnostics, .. }
            | Self::Binding { diagnostics, .. } => diagnostics,
        }
    }
}

js_object_union! {
    /// One batch result at its original input position: exactly one of
    /// `response` and `failure` is `null`.
    pub enum NapiCompileRequestsEntry untagged as "HostCompileRequestsEntry" {
        Response {
            canonicalId: String,
            response: NapiCompileRequestResponse,
            failure: JsNull,
        } as "HostCompileRequestsResponseEntry",
        Failure {
            canonicalId: String,
            response: JsNull,
            failure: NapiCompileRequestFailure,
        } as "HostCompileRequestsFailureEntry",
    }
}

/// The native framing of a canonical request-construction refusal.
///
/// Only the framing: every word naming the offending option, capability,
/// product or axis comes from `CompileRequestError`'s own `Display` in
/// `verter_compiler`, which both bindings render. A refusal vocabulary
/// kept here would fork from the browser binding's for the same refused
/// request.
pub(crate) fn compile_request_construction_refused(error: &CompileRequestError) -> String {
    format!("compile request construction refused: {error}")
}

// A diagnostic projects through the shared FFI converter
// (`verter_ffi::convert::host_diagnostic_to_ffi`) so NAPI and WASM cannot
// diverge on severity spelling or UTF-16 span mapping — this crate only
// re-shapes the FFI struct into its `#[napi(object)]` mirror, never
// re-derives the conversion.

impl From<FfiDiagnostic> for NapiDiagnostic {
    fn from(diagnostic: FfiDiagnostic) -> Self {
        NapiDiagnostic {
            severity: diagnostic.severity,
            code: diagnostic.code,
            message: diagnostic.message,
            spanStart: diagnostic.span_start,
            spanEnd: diagnostic.span_end,
            spanEncoding: diagnostic.span_encoding.as_wire().to_string(),
            spanError: diagnostic.span_error,
        }
    }
}

pub(crate) fn host_diagnostics_to_napi(
    input: &host::DiagnosticsSnapshot,
    source: Option<&str>,
) -> NapiDiagnosticsSnapshot {
    host_diagnostics_to_napi_with(input, &verter_ffi::convert::SpanConverter::new(source))
}

pub(crate) fn host_diagnostics_to_napi_with(
    input: &host::DiagnosticsSnapshot,
    spans: &verter_ffi::convert::SpanConverter<'_>,
) -> NapiDiagnosticsSnapshot {
    let snapshot = verter_ffi::convert::host_diagnostics_to_ffi_with(input, spans);
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

pub(crate) fn host_ide_to_napi(input: host::IdeResponse, source: &str) -> Result<NapiIdeResponse> {
    let destructured_block = input
        .destructured_block
        .as_ref()
        .map(|meta| {
            let bindings: Vec<verter_ffi::convert::DestructuredBindingInput<'_>> = meta
                .bindings
                .iter()
                .map(|binding| {
                    Ok(verter_ffi::convert::DestructuredBindingInput {
                        name: &binding.name,
                        source: verter_ffi::convert::source_range(binding.source_span)?,
                    })
                })
                .collect::<std::result::Result<_, verter_span::CoordError>>()
                .map_err(|error| {
                    Error::new(
                        Status::GenericFailure,
                        format!("destructured binding span cannot be converted exactly: {error}"),
                    )
                })?;
            let ffi = verter_ffi::convert::convert_destructured_block_meta(
                &bindings,
                verter_ffi::convert::generated_range(verter_span::Span::new(
                    meta.block_start,
                    meta.block_end,
                ))
                .map_err(|error| {
                    Error::new(
                        Status::GenericFailure,
                        format!("destructured block span cannot be converted exactly: {error}"),
                    )
                })?,
                Some(source),
                &input.code,
                SpanEncodingTag::Utf16,
            )
            .map_err(|error| {
                Error::new(
                    Status::GenericFailure,
                    format!("destructured block span cannot be converted exactly: {error}"),
                )
            })?;
            Ok::<_, Error>(NapiDestructuredBlockMeta {
                bindings: ffi
                    .bindings
                    .into_iter()
                    .map(|binding| NapiDestructuredBinding {
                        name: binding.name,
                        sourceStart: binding.source_start,
                        sourceEnd: binding.source_end,
                        sourceEncoding: binding.source_encoding.as_wire().to_string(),
                    })
                    .collect(),
                blockStart: ffi.block_start,
                blockEnd: ffi.block_end,
                blockEncoding: ffi.block_encoding.as_wire().to_string(),
            })
        })
        .transpose()?;
    Ok(NapiIdeResponse {
        code: input.code.to_string(),
        sourceMap: input.source_map.map(|map| map.to_string()),
        isJsx: input.is_jsx,
        destructuredBlock: destructured_block,
    })
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
                let nodes = nodes
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
                    .collect();
                match kind {
                    ProductKind::RuntimeClient => Ok(NapiCompiledProduct::RuntimeClient { nodes }),
                    ProductKind::RuntimeServer => Ok(NapiCompiledProduct::RuntimeServer { nodes }),
                    ProductKind::IdeCompanion
                    | ProductKind::PublicApi
                    | ProductKind::Declarations
                    | ProductKind::Analysis => Err(ffi_err(format!(
                        "refused host compile request for {canonical_id}: the {} product published a runtime output row, which has no runtime wire tag",
                        kind.wire_tag()
                    ))),
                }
            }
            host::CompiledProduct::Ide(ide) => Ok(NapiCompiledProduct::IdeCompanion {
                ide: host_ide_to_napi(ide, source)?,
            }),
            host::CompiledProduct::Analysis(analysis) => {
                let analysis = serde_json::to_string(&Utf8SpanPayload::new(analysis.as_ref())).map_err(|error| {
                    Error::new(
                        Status::GenericFailure,
                        format!("analysis serialization error: {error}"),
                    )
                })?;
                Ok(NapiCompiledProduct::Analysis { analysis })
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
            NapiCompileRequestFailure::Host {
                canonicalId: fallback_canonical_id,
                message,
                diagnostics,
            }
        }
        host::CompileRequestFailure::FrameworkMismatch {
            canonical_id,
            requested,
            registered,
        } => NapiCompileRequestFailure::FrameworkMismatch {
            canonicalId: canonical_id.clone(),
            message: format!(
                "compile request for '{canonical_id}' names framework '{requested}', but the registered source uses '{registered}'"
            ),
            diagnostics: empty_diagnostics_snapshot(),
            requestedFramework: requested.to_string(),
            registeredFramework: registered,
        },
        host::CompileRequestFailure::UnsupportedProduct {
            canonical_id,
            kind,
            diagnostics,
        } => NapiCompileRequestFailure::UnsupportedProduct {
            canonicalId: canonical_id,
            message: diagnostic_message(&diagnostics, "compile product is unsupported"),
            diagnostics: host_diagnostics_to_napi(&diagnostics, source),
            productKind: RequestedProductKind(kind),
        },
        host::CompileRequestFailure::ProductNotProduced {
            canonical_id,
            kind,
            diagnostics,
        } => NapiCompileRequestFailure::ProductNotProduced {
            canonicalId: canonical_id,
            message: diagnostic_message(&diagnostics, "compile product was not produced"),
            diagnostics: host_diagnostics_to_napi(&diagnostics, source),
            productKind: RequestedProductKind(kind),
        },
        host::CompileRequestFailure::RuntimeSurfaceRefused {
            canonical_id,
            diagnostic_code,
            message,
            diagnostics,
        } => NapiCompileRequestFailure::RuntimeSurfaceRefused {
            canonicalId: canonical_id,
            message,
            diagnostics: host_diagnostics_to_napi(&diagnostics, source),
            diagnosticCode: diagnostic_code,
        },
        host::CompileRequestFailure::Refused {
            canonical_id,
            diagnostics,
        } => NapiCompileRequestFailure::Refused {
            canonicalId: canonical_id,
            message: diagnostic_message(&diagnostics, "compile request was refused"),
            diagnostics: host_diagnostics_to_napi(&diagnostics, source),
        },
        host::CompileRequestFailure::DependenciesIncomplete {
            canonical_id,
            gaps,
            diagnostics,
        } => NapiCompileRequestFailure::DependenciesIncomplete {
            canonicalId: canonical_id,
            message: diagnostic_message(
                &diagnostics,
                "compile request dependencies are incomplete",
            ),
            diagnostics: host_diagnostics_to_napi(&diagnostics, source),
            missingInputs: Serialized(compile_dependency_gaps_to_ffi(gaps)),
        },
    }
}

pub(crate) fn binding_failure_to_napi(
    canonical_id: String,
    message: String,
) -> NapiCompileRequestFailure {
    NapiCompileRequestFailure::Binding {
        canonicalId: canonical_id,
        message,
        diagnostics: empty_diagnostics_snapshot(),
    }
}

pub(crate) fn compile_request_failure_status(failure: &host::CompileRequestFailure) -> Status {
    match failure {
        host::CompileRequestFailure::Host(error) => crate::host_error_status(error),
        host::CompileRequestFailure::FrameworkMismatch { .. }
        | host::CompileRequestFailure::UnsupportedProduct { .. }
        | host::CompileRequestFailure::Refused { .. }
        | host::CompileRequestFailure::DependenciesIncomplete { .. } => Status::InvalidArg,
        host::CompileRequestFailure::ProductNotProduced { .. }
        | host::CompileRequestFailure::RuntimeSurfaceRefused { .. } => Status::GenericFailure,
    }
}

/// The `Error` `compileRequest` throws for `failure`: its message is the
/// failure's message, and every field of the failure's arm — the tag
/// included — is an own property of it.
///
/// The arm writes its properties through the same conversion a batch
/// entry's failure object uses, so the two cannot carry different fields
/// for the same failure. That conversion writes `message` too; on an
/// `Error` that assignment finds the own, non-enumerable `message` the
/// constructor created and stores the same string into it, so the property
/// keeps its attributes and its position.
pub(crate) fn compile_request_error(
    env: &Env,
    status: Status,
    failure: NapiCompileRequestFailure,
) -> Result<Error> {
    let mut error = env.create_error(Error::new(status, failure.message().to_owned()))?;
    failure.write_properties(&mut error)?;
    Ok(Error::from((&error).into_unknown(env)?))
}

pub(crate) fn binding_failure_entry(
    canonical_id: String,
    message: String,
) -> NapiCompileRequestsEntry {
    NapiCompileRequestsEntry::Failure {
        canonicalId: canonical_id.clone(),
        response: JsNull,
        failure: binding_failure_to_napi(canonical_id, message),
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
        | host::CompileRequestFailure::Refused { canonical_id, .. }
        | host::CompileRequestFailure::DependenciesIncomplete { canonical_id, .. } => canonical_id,
    }
}
