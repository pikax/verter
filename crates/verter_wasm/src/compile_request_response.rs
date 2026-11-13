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
//! The envelope it fills is declared in `verter_protocol::compile_response`
//! ([`BrowserCompileRequestResponse`] and its product rows), where the
//! generator that renders this package's published TypeScript can reach
//! it: the declaration a browser caller type-checks against is rendered
//! from the very types serialised here.
//!
//! The payload shapes reuse the SHARED FFI output DTOs wherever one already
//! exists ([`FfiDiagnosticsSnapshot`], [`FfiIdeResponse`],
//! `FfiVirtualNodeKind`, [`FfiVirtualMeta`]), so a diagnostic, a node kind
//! and an IDE projection read identically on this route and on the legacy
//! per-node reads beside it — including the UTF-16 offsets, which come from
//! the same `host_diagnostics_to_ffi` conversion the legacy routes use.
//!
//! [`FfiDiagnosticsSnapshot`]: verter_protocol::types::FfiDiagnosticsSnapshot

use verter_ffi::convert::{host_diagnostics_to_ffi, host_error_to_string, host_node_kind_to_ffi};
use verter_protocol::compile_response::{
    BrowserCompileRequestResponse, BrowserCompiledProduct, BrowserCompiledVirtualNode,
};
use verter_protocol::types::{
    FfiDestructuredBlockMeta, FfiIdeResponse, FfiVirtualMeta, Utf8SpanPayload,
};
use verter_session as host;

/// Project a host IDE response, resolving the destructured block's mappings
/// against the carrier source in UTF-16 offsets.
///
/// Shared by this route and the legacy cached IDE read, so both publish the
/// identical payload for the same compiled surface.
pub(crate) fn ide_response_to_ffi(
    response: &host::IdeResponse,
    sfc_source: Option<&str>,
) -> Result<FfiIdeResponse, String> {
    Ok(FfiIdeResponse {
        code: response.code.to_string(),
        source_map: response.source_map.as_ref().map(ToString::to_string),
        is_jsx: response.is_jsx,
        destructured_block: destructured_block_to_ffi(response, sfc_source)?,
    })
}

fn destructured_block_to_ffi(
    response: &host::IdeResponse,
    sfc_source: Option<&str>,
) -> Result<Option<FfiDestructuredBlockMeta>, String> {
    let Some(meta) = response.destructured_block.as_ref() else {
        return Ok(None);
    };
    let refused = |error: verter_span::CoordError| {
        format!("destructured block span cannot be converted exactly: {error}")
    };
    let bindings = meta
        .bindings
        .iter()
        .map(|binding| {
            Ok(verter_ffi::convert::DestructuredBindingInput {
                name: &binding.name,
                source: verter_ffi::convert::source_range(binding.source_span)?,
            })
        })
        .collect::<Result<Vec<_>, verter_span::CoordError>>()
        .map_err(refused)?;
    let block = verter_ffi::convert::generated_range(verter_span::Span::new(
        meta.block_start,
        meta.block_end,
    ))
    .map_err(refused)?;
    verter_ffi::convert::convert_destructured_block_meta(
        &bindings,
        block,
        sfc_source,
        &response.code,
        verter_protocol::types::SpanEncodingTag::Utf16,
    )
    .map(Some)
    .map_err(|error| format!("destructured block span cannot be converted exactly: {error}"))
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
) -> Result<BrowserCompileRequestResponse, String> {
    let host::CompileRequestResponse {
        canonical_id,
        diagnostics,
        products,
    } = response;
    let products = products
        .into_iter()
        .map(|product| compiled_product_to_wasm(&canonical_id, product, sfc_source))
        .collect::<Result<Vec<_>, _>>()?;
    Ok(BrowserCompileRequestResponse {
        canonical_id,
        diagnostics: host_diagnostics_to_ffi(&diagnostics, sfc_source),
        products,
    })
}

fn compiled_product_to_wasm(
    canonical_id: &str,
    product: host::CompiledProduct,
    sfc_source: Option<&str>,
) -> Result<BrowserCompiledProduct, String> {
    use verter_compiler::compile_request::ProductKind;

    Ok(match product {
        host::CompiledProduct::Runtime { kind, nodes } => {
            let nodes = nodes.into_iter().map(compiled_node_to_wasm).collect();
            match kind {
                ProductKind::RuntimeClient => BrowserCompiledProduct::RuntimeClient { nodes },
                ProductKind::RuntimeServer => BrowserCompiledProduct::RuntimeServer { nodes },
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
            BrowserCompiledProduct::IdeCompanion(ide_response_to_ffi(&response, sfc_source)?)
        }
        host::CompiledProduct::Analysis(snapshot) => BrowserCompiledProduct::Analysis {
            analysis: Utf8SpanPayload::new(snapshot),
        },
    })
}

fn compiled_node_to_wasm(node: host::CompiledVirtualNode) -> BrowserCompiledVirtualNode {
    BrowserCompiledVirtualNode {
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
        Failure::DependenciesIncomplete {
            canonical_id,
            gaps: _,
            diagnostics,
        } => format!(
            "incomplete dependencies for host compile request {canonical_id}{}",
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
