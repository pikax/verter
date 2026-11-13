//! Wire DTOs of the typed host compile RESPONSE that both bindings reach.
//!
//! - [`FfiMissingInput`] is what a `dependenciesIncomplete` refusal names:
//!   one row per input the compile could not establish. The session's
//!   typed gap converts into it in `verter_ffi`, exhaustively, so a gap
//!   kind added there fails to compile until this wire names it.
//! - The `Browser*` declarations are the browser binding's response
//!   envelope. They live here rather than in `verter_wasm` so the
//!   generator that renders their published TypeScript can reach them: the
//!   declaration a browser caller type-checks against is rendered from the
//!   very types that binding serialises.
//!
//! The native binding's envelope differs (it nests the IDE payload, sends
//! `analysis` as a JSON string, and throws a structured `Error`), so it
//! keeps its own declarations beside its own conversion in `verter_napi`.

use serde::Serialize;
use verter_session_query::analysis::template::TemplateAnalysisSnapshot;

use crate::js_union::__ts_rs::{Config, TS};
use crate::types::{
    FfiDiagnosticsSnapshot, FfiIdeResponse, FfiVirtualMeta, FfiVirtualNodeKind, Utf8SpanPayload,
};

crate::ts_declared_elsewhere! {
    FfiDiagnosticsSnapshot => "HostDiagnosticsSnapshot",
    FfiIdeResponse => "HostIdeResponse",
    FfiVirtualMeta => "HostVirtualMeta",
    FfiVirtualNodeKind => "HostVirtualNodeKind",
}

crate::serialized_js_union! {
    /// One input a compile could not establish while proving its macro type
    /// dependencies present or absent, in the order the host reports them.
    ///
    /// The list is deterministic for a given host state. Supplying an input
    /// can let the compile proceed far enough to name a deeper one, so a
    /// caller retries only while it is supplying something new; an input it
    /// already supplied that is named again, or a list naming only `lookup`
    /// rows or refused `route` rows, is terminal.
    pub enum FfiMissingInput tagged "kind" as "HostMissingInput" {
        /// `specifier`, imported by `importer`, has no route on this host
        /// (`refused: false`, a witnessed miss) or its route could not be
        /// admitted (`refused: true`).
        Route {
            importer: String,
            specifier: String,
            refused: bool,
        } => "route" as "HostMissingRoute",
        /// The route resolves to `canonicalId`, which this host cannot load.
        File {
            canonicalId: String,
        } => "file" as "HostMissingFile",
        /// The lookup of `typeName` from `importSource` ended early (budget,
        /// recursion, or an unfinished traversal).
        Lookup {
            importSource: String,
            typeName: String,
        } => "lookup" as "HostMissingLookup",
    }
}

/// One separately addressed output of a compiled runtime product.
///
/// A runtime product is not one blob: the assembled main module, the
/// script, the compiled template, each style block and each custom block
/// are distinct modules a consumer loads and maps independently, so each
/// row carries its own code and its own map.
///
/// The optional fields carry no `| null`: the route serialises through
/// `serde_wasm_bindgen`'s default serializer, where an absent value is
/// `undefined` and never `null`.
#[derive(Serialize, ts_rs::TS)]
#[serde(rename_all = "camelCase")]
#[ts(rename = "HostCompiledVirtualNode", optional_fields)]
pub struct BrowserCompiledVirtualNode {
    /// Which node of the carrier this row is.
    pub node: FfiVirtualNodeKind,
    pub code: String,
    /// This node's own JSON source map, when the request asked for maps.
    pub source_map: Option<String>,
    /// Output language (e.g. `"js"`, `"ts"`, `"css"`).
    pub lang: Option<String>,
    /// Block-specific metadata carried beside the node's bytes.
    pub meta: FfiVirtualMeta,
}

/// The template analysis payload, as it crosses the browser boundary.
pub type BrowserTemplateAnalysis = Utf8SpanPayload<Box<TemplateAnalysisSnapshot>>;

/// The template analysis payload stays UNSTRUCTURED in the projection: its
/// schema belongs to `@verter/language-shared`, which a browser binding's
/// published types do not pull in.
impl TS for BrowserTemplateAnalysis {
    type WithoutGenerics = Self;
    type OptionInnerType = Self;

    fn docs() -> Option<String> {
        Some(ts_rs::format_docs(&[
            " The TEMPLATE analysis snapshot — binding occurrences, expression",
            " diagnostics, and the rest of the template's facts.",
            "",
            " This is NOT what `getAnalysis()` returns. `getAnalysis()` publishes the",
            " whole-file snapshot (imports, bindings, macros, styles, and a nested",
            " `template`); this payload is that snapshot's `template` value and nothing",
            " else. `@verter/language-shared` declares the same object as",
            " `TemplateAnalysisSnapshot`; it stays unstructured here rather than",
            " pulling an editor package into a browser binding's published types, and",
            " the equivalence is proven at run time by the browser boundary tests",
            " rather than asserted only in prose.",
            "",
            " Its spans are UTF-8 BYTE offsets into the SFC source. Diagnostics and",
            " destructured binding source spans are UTF-16 offsets into that source;",
            " destructured block bounds are UTF-16 offsets into the generated IDE code.",
        ]))
    }

    fn name(_: &Config) -> String {
        String::from("HostTemplateAnalysisSnapshot")
    }

    fn inline(_: &Config) -> String {
        String::from("Record<string, unknown>")
    }

    fn decl(cfg: &Config) -> String {
        format!("type {} = {};", Self::name(cfg), Self::inline(cfg))
    }

    fn decl_concrete(cfg: &Config) -> String {
        Self::decl(cfg)
    }

    fn output_path() -> Option<std::path::PathBuf> {
        Some(std::path::PathBuf::from("HostTemplateAnalysisSnapshot.ts"))
    }
}

// An arm flattens its payload beside `kind` ONLY when that payload is a
// wire-owned DTO — one whose fields exist to be serialised and cannot gain a
// `kind` without this taxonomy being part of the change. Every other arm
// NESTS under its own key: a flattened payload this wire does not own could
// gain a `kind` field upstream, overwrite the discriminant, and stop every
// consumer's `kind === "…"` branch from matching a row that still carries
// its data.
crate::serialized_js_union! {
    /// One row of a response's product list, one-to-one with a requested
    /// product kind and in request order, tagged with the same spelling the
    /// request used.
    pub enum BrowserCompiledProduct tagged "kind" as "HostCompiledProduct" {
        RuntimeClient {
            nodes: Vec<BrowserCompiledVirtualNode>,
        } => "runtimeClient" as "HostCompiledRuntimeProduct",
        RuntimeServer {
            nodes: Vec<BrowserCompiledVirtualNode>,
        } => "runtimeServer" as "HostCompiledRuntimeProduct",
        /// The IDE projection row: the shared IDE response, flattened beside
        /// its tag.
        IdeCompanion(FfiIdeResponse) => "ideCompanion" as "HostCompiledIdeProduct",
        /// The template analysis, nested under its own key rather than
        /// flattened into the row, so no field of the payload can collide
        /// with the `kind` discriminant.
        Analysis {
            analysis: BrowserTemplateAnalysis,
        } => "analysis" as "HostCompiledAnalysisProduct",
    }
}

/// The result of executing one typed compile request.
///
/// Complete-only: every requested product is present. A refusal at any stage
/// THROWS — there is no partial response, no `null`, and no boolean.
#[derive(Serialize, ts_rs::TS)]
#[serde(rename_all = "camelCase")]
#[ts(rename = "HostCompileRequestResponse")]
pub struct BrowserCompileRequestResponse {
    /// The canonical id the request executed against, after alias resolution.
    pub canonical_id: String,
    /// The compile's diagnostics, deduplicated once across every product.
    /// Spans are UTF-16 offsets into the registered source.
    pub diagnostics: FfiDiagnosticsSnapshot,
    /// One row per requested product kind, in request order.
    pub products: Vec<BrowserCompiledProduct>,
}
