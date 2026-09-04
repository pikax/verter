//! The browser projection of the session's typed compile-request envelope.
//!
//! One direction only: a [`host::CompileRequestResponse`] (or the typed
//! failure that replaces it) becomes the JavaScript value the caller
//! receives. Nothing here decides anything about the compile — the
//! response is already complete when it arrives, so this module carries it
//! across the boundary and invents no vocabulary of its own: the product
//! tags it publishes, and the product names its refusals use, are the
//! REQUEST's spellings, so a caller matches an answer against what it sent
//! without learning a second set of names.
//!
//! The payload shapes reuse the SHARED FFI output DTOs wherever one already
//! exists ([`FfiDiagnosticsSnapshot`], [`FfiIdeResponse`],
//! [`FfiVirtualNodeKind`], [`FfiVirtualMeta`]), so a diagnostic, a node kind
//! and an IDE projection read identically on this route and on the legacy
//! per-node reads beside it — including the UTF-16 offsets, which come from
//! the same `host_diagnostics_to_ffi` conversion the legacy routes use.

use serde::Serialize;

use verter_ffi::convert::{host_diagnostics_to_ffi, host_error_to_string, host_node_kind_to_ffi};
use verter_protocol::types::{
    FfiDestructuredBlockMeta, FfiDiagnosticsSnapshot, FfiIdeResponse, FfiVirtualMeta,
    FfiVirtualNodeKind,
};
use verter_session as host;

/// One separately addressed output of a compiled runtime product.
///
/// A runtime product is not one blob: the carrier's assembled main module,
/// its script, its compiled template, each style block and each custom
/// block are distinct modules a consumer loads and maps independently, so
/// each row keeps its own code and its own map.
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct WasmCompiledVirtualNode {
    node: FfiVirtualNodeKind,
    code: String,
    source_map: Option<String>,
    lang: Option<String>,
    meta: FfiVirtualMeta,
}

/// One row of a response's product list, one-to-one with a requested
/// product kind and in request order.
///
/// Tagged by `kind` with the SAME spellings the request's product arms
/// carry, so a caller matches a row against the product it asked for
/// without learning a second vocabulary.
///
/// An arm may flatten its payload beside `kind` ONLY when that payload is
/// a wire-owned DTO — one whose fields exist to be serialised and cannot
/// gain a `kind` without this taxonomy being part of the change. That holds
/// for [`FfiIdeResponse`], and the shared schema flattens its product
/// options the same way.
///
/// Every other arm NESTS under its own key. A flattened arm over a payload
/// this wire does not own is a latent collision: a field named `kind` added
/// upstream is serialised into the same object as the discriminant, the
/// later write wins, and every consumer's `kind === "…"` branch stops
/// matching on a row that still carries its data. That is why the analysis
/// arm nests its semantic-crate snapshot, as the runtime arms already nest
/// their nodes.
#[derive(Serialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub(crate) enum WasmCompiledProduct {
    RuntimeClient {
        nodes: Vec<WasmCompiledVirtualNode>,
    },
    RuntimeServer {
        nodes: Vec<WasmCompiledVirtualNode>,
    },
    IdeCompanion(FfiIdeResponse),
    Analysis {
        analysis: Box<verter_semantic::analysis::template::TemplateAnalysisSnapshot>,
    },
}

/// The result of executing one caller-supplied typed compile request.
///
/// Complete-only, exactly as the session envelope is: every requested
/// product is present, and a refusal is thrown instead of being reported as
/// a partial response, a `null`, or a boolean.
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct WasmCompileRequestResponse {
    canonical_id: String,
    diagnostics: FfiDiagnosticsSnapshot,
    products: Vec<WasmCompiledProduct>,
}

/// Project a host IDE response, resolving the destructured block's mappings
/// against the carrier source in UTF-16 offsets.
///
/// Shared by this route and the legacy cached IDE read, so both publish the
/// identical payload for the same compiled surface.
pub(crate) fn ide_response_to_ffi(
    response: &host::IdeResponse,
    sfc_source: Option<&str>,
) -> FfiIdeResponse {
    FfiIdeResponse {
        code: response.code.to_string(),
        source_map: response.source_map.as_ref().map(ToString::to_string),
        is_jsx: response.is_jsx,
        destructured_block: destructured_block_to_ffi(response, sfc_source),
    }
}

fn destructured_block_to_ffi(
    response: &host::IdeResponse,
    sfc_source: Option<&str>,
) -> Option<FfiDestructuredBlockMeta> {
    let meta = response.destructured_block.as_ref()?;
    let sfc = sfc_source.unwrap_or("");
    let bindings: Vec<verter_ffi::convert::DestructuredBindingInput<'_>> = meta
        .bindings
        .iter()
        .map(|binding| verter_ffi::convert::DestructuredBindingInput {
            name: &binding.name,
            source_start: binding.source_span.start,
            source_end: binding.source_span.end,
        })
        .collect();
    Some(verter_ffi::convert::convert_destructured_block_meta(
        &bindings,
        meta.block_start,
        meta.block_end,
        sfc,
        &response.code,
        verter_ffi::convert::OffsetEncoding::Utf16,
    ))
}

/// Project the session's typed envelope for JavaScript.
///
/// `sfc_source` is the registered carrier source the compile ran against;
/// it is what turns the host's UTF-8 diagnostic spans into the UTF-16
/// offsets a JavaScript caller indexes with, so it is supplied rather than
/// rediscovered here.
///
/// Fallible, and fails CLOSED: a product row this taxonomy cannot tag
/// exactly refuses the whole response rather than publishing the row under
/// a substituted tag, which a JavaScript caller cannot tell from a correct
/// one.
pub(crate) fn compile_request_response_to_wasm(
    response: host::CompileRequestResponse,
    sfc_source: Option<&str>,
) -> Result<WasmCompileRequestResponse, String> {
    let host::CompileRequestResponse {
        canonical_id,
        diagnostics,
        products,
    } = response;
    let products = products
        .into_iter()
        .map(|product| compiled_product_to_wasm(&canonical_id, product, sfc_source))
        .collect::<Result<Vec<_>, _>>()?;
    Ok(WasmCompileRequestResponse {
        canonical_id,
        diagnostics: host_diagnostics_to_ffi(&diagnostics, sfc_source),
        products,
    })
}

fn compiled_product_to_wasm(
    canonical_id: &str,
    product: host::CompiledProduct,
    sfc_source: Option<&str>,
) -> Result<WasmCompiledProduct, String> {
    use verter_compiler::compile_request::ProductKind;

    Ok(match product {
        host::CompiledProduct::Runtime { kind, nodes } => {
            let nodes = nodes.into_iter().map(compiled_node_to_wasm).collect();
            match kind {
                ProductKind::RuntimeClient => WasmCompiledProduct::RuntimeClient { nodes },
                ProductKind::RuntimeServer => WasmCompiledProduct::RuntimeServer { nodes },
                // Both runtime tags are matched by name, and the remaining
                // kinds are listed rather than swept up by a wildcard: a
                // product kind added upstream fails THIS build instead of
                // reaching a browser mislabelled. No input produces a
                // runtime row under these kinds today, so the arm is a
                // fail-closed refusal rather than a fabricated tag.
                kind @ (ProductKind::IdeCompanion
                | ProductKind::PublicApi
                | ProductKind::Declarations
                | ProductKind::Analysis) => {
                    return Err(format!(
                        "refused host compile request for {canonical_id}: the {} product \
                         published a runtime output row, which has no runtime wire tag",
                        kind.wire_tag()
                    ));
                }
            }
        }
        host::CompiledProduct::Ide(response) => {
            WasmCompiledProduct::IdeCompanion(ide_response_to_ffi(&response, sfc_source))
        }
        host::CompiledProduct::Analysis(snapshot) => {
            WasmCompiledProduct::Analysis { analysis: snapshot }
        }
    })
}

fn compiled_node_to_wasm(node: host::CompiledVirtualNode) -> WasmCompiledVirtualNode {
    WasmCompiledVirtualNode {
        node: host_node_kind_to_ffi(&node.node),
        code: node.code.to_string(),
        source_map: node.source_map.as_ref().map(ToString::to_string),
        lang: node.lang,
        meta: FfiVirtualMeta {
            scope_id: node.meta.scope_id,
            block_type: node.meta.block_type,
        },
    }
}

/// Render a typed compile-request refusal as the thrown JavaScript error.
///
/// Each arm states which authority refused and what it refused, drawn from
/// the failure's own typed fields. The refusal's diagnostics carry the exact
/// rule that refused it, so they ride along verbatim rather than re-worded.
pub(crate) fn compile_request_failure_to_string(failure: &host::CompileRequestFailure) -> String {
    use host::CompileRequestFailure as Failure;

    match failure {
        Failure::Host(error) => host_error_to_string(error),
        Failure::FrameworkMismatch {
            canonical_id,
            requested,
            registered,
        } => format!(
            "refused host compile request for {canonical_id}: the request names {requested}, but \
             the registered carrier is {registered}"
        ),
        Failure::UnsupportedProduct {
            canonical_id,
            kind,
            diagnostics,
        } => format!(
            "refused host compile request for {canonical_id}: no host production route for the \
             {} product{}",
            kind.wire_tag(),
            rendered_diagnostics(diagnostics)
        ),
        Failure::ProductNotProduced {
            canonical_id,
            kind,
            diagnostics,
        } => format!(
            "refused host compile request for {canonical_id}: the {} product was admitted and \
             published no payload{}",
            kind.wire_tag(),
            rendered_diagnostics(diagnostics)
        ),
        Failure::RuntimeSurfaceRefused {
            canonical_id,
            diagnostic_code,
            message,
            diagnostics,
        } => format!(
            "refused host compile request for {canonical_id}: {diagnostic_code}: {message}{}",
            rendered_diagnostics(diagnostics)
        ),
        Failure::Refused {
            canonical_id,
            diagnostics,
        } => format!(
            "refused host compile request for {canonical_id}{}",
            rendered_diagnostics(diagnostics)
        ),
    }
}

/// The refusal's own diagnostics, appended verbatim. An empty set appends
/// nothing rather than an empty bracket pair.
fn rendered_diagnostics(diagnostics: &host::DiagnosticsSnapshot) -> String {
    if diagnostics.diagnostics.is_empty() {
        return String::new();
    }
    let rendered = diagnostics
        .diagnostics
        .iter()
        .map(|diagnostic| format!("{}: {}", diagnostic.code, diagnostic.message))
        .collect::<Vec<_>>()
        .join("; ");
    format!(" [{rendered}]")
}
