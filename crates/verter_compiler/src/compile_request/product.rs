//! The requested-product vocabulary — the neutral replacement for
//! `CompileTarget` bitflags. A [`CompileRequest`](super::CompileRequest)
//! carries a non-empty set of [`CompileProduct`]s with no duplicate kind;
//! each variant owns exactly the profile axes that affect it (architecture
//! §7.1's "no single global `OutputProfileId`... presumed valid for every
//! product").

use verter_identity::profile::{OutputProfileId, PresentationProfileId, SerializationProfileId};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ProductKind {
    RuntimeClient,
    RuntimeServer,
    IdeCompanion,
    PublicApi,
    Declarations,
    /// Script-macro bindings and/or raw template facts, no codegen
    /// emission. The former `ANALYSIS`/`META` bitflag presets (bit-identical
    /// today; both `SCRIPT | TEMPLATE_DATA`) collapse onto this one product
    /// kind, since two callers requesting the identical shape is not a
    /// second product — see [`AnalysisProductRequest`] for why its two axes
    /// stay independently settable rather than being merged into one flag.
    Analysis,
}

/// Runtime (client or server) product options. `inline` is meaningful only
/// on a `RuntimeClient` product — its presence on a `RuntimeServer` product
/// is refused at [`super::CompileRequest::new`] time (`inline x SSR` fails
/// closed, no silent non-inline demotion).
#[derive(Debug, Clone, Default)]
pub struct RuntimeProductRequest {
    /// `None` resolves to `CompileRequest::is_production` (official
    /// default: inline in production builds) — the same default
    /// `CodegenOptions::resolve_inline` implements today, carried forward
    /// as the request's own derived default rather than reimplemented
    /// downstream.
    pub inline: Option<bool>,
    /// Independent of every other product's own map demand — fixes the
    /// `source_map: opts.source_map || opts.want_ide` coupling: requesting
    /// an [`IdeCompanion`](super::CompileProduct::IdeCompanion) product
    /// alongside this one no longer silently turns this flag on.
    pub runtime_source_map: bool,
    pub output_profile: Option<OutputProfileId>,
    pub serialization: Option<SerializationProfileId>,
}

impl RuntimeProductRequest {
    pub fn resolve_inline(&self, is_production: bool) -> bool {
        self.inline.unwrap_or(is_production)
    }
}

#[derive(Debug, Clone, Default)]
pub struct IdeProductRequest {
    /// Independent of the runtime product's own
    /// [`RuntimeProductRequest::runtime_source_map`] — see that field's doc
    /// for the coupling this independence fixes.
    pub want_source_map: bool,
    /// Embed `declare module "@verter/types"` in generated TSX (so
    /// `import ... from "@verter/types"` resolves without the real
    /// package installed).
    pub embed_ambient_types: bool,
    /// Experimental: conditional root generic narrowing.
    pub conditional_root_narrowing: bool,
    /// Experimental: strict slot children type checking.
    pub strict_slots: bool,
    /// Types module name for TSX helper imports. `None` resolves to
    /// `"@verter/types"`.
    pub types_module_name: Option<String>,
    /// Emit typed IDE chunk boundaries for multi-source composition
    /// (carrier adapters set this only while compiling isolated units).
    pub ide_chunk_boundaries: bool,
    pub output_profile: Option<OutputProfileId>,
    /// The required `SourceProjectionMap` companion is implicit — a
    /// property of requesting this product at all, never a disable-able
    /// field (architecture §7.1).
    pub diagnostics: Option<PresentationProfileId>,
    pub serialization: Option<SerializationProfileId>,
}

#[derive(Debug, Clone, Default)]
pub struct PublicApiProductRequest {
    pub output_profile: Option<OutputProfileId>,
    pub serialization: Option<SerializationProfileId>,
}

#[derive(Debug, Clone, Default)]
pub struct DeclarationProductRequest {
    pub output_profile: Option<OutputProfileId>,
    pub serialization: Option<SerializationProfileId>,
}

/// Script-macro bindings and raw template-data extraction, independently
/// settable — mirrors the legacy `CompileTarget` bit math exactly: template-
/// data extraction alone (no script-bindings axis set) still requires
/// script-level analysis as a prerequisite (`needs_script()` intersected
/// `TEMPLATE_DATA` directly), which
/// [`super::CompileRequest::wants_script_output`] preserves; the two former
/// presets (`META`, `ANALYSIS`) always set both together, but the LSP's
/// real `IDE | TEMPLATE_DATA` profile sets template-data alone — so
/// collapsing these into one boolean would lose a distinction production
/// code actually exercises.
#[derive(Debug, Clone, Copy, Default)]
pub struct AnalysisProductRequest {
    pub want_script_bindings: bool,
    pub want_template_data: bool,
}

#[derive(Debug, Clone)]
pub enum CompileProduct {
    RuntimeClient(RuntimeProductRequest),
    RuntimeServer(RuntimeProductRequest),
    IdeCompanion(IdeProductRequest),
    PublicApi(PublicApiProductRequest),
    Declarations(DeclarationProductRequest),
    Analysis(AnalysisProductRequest),
}

impl CompileProduct {
    pub const fn kind(&self) -> ProductKind {
        match self {
            CompileProduct::RuntimeClient(_) => ProductKind::RuntimeClient,
            CompileProduct::RuntimeServer(_) => ProductKind::RuntimeServer,
            CompileProduct::IdeCompanion(_) => ProductKind::IdeCompanion,
            CompileProduct::PublicApi(_) => ProductKind::PublicApi,
            CompileProduct::Declarations(_) => ProductKind::Declarations,
            CompileProduct::Analysis(_) => ProductKind::Analysis,
        }
    }
}

/// A caller-settable request axis the host bundle execution has no
/// routing channel for — it would either be dropped (no consumer reads
/// it) or substituted (a consumer overwrites it with a value derived from
/// the execution inputs).
///
/// Framework-neutral: the axes live on the product requests and on the
/// request itself, not on either framework's option block, so both host
/// integrations refuse the same rows through the same reader
/// ([`unroutable_host_request_axis`]).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UnroutableHostRequestAxis {
    /// An [`AnalysisProductRequest`] with neither axis set: the product
    /// would be admitted and then publish nothing, so its row could only
    /// be reported as an unproduced payload after a successful compile.
    AnalysisProducesNothing,
    /// [`RuntimeProductRequest::output_profile`]: no execution consumer.
    RuntimeOutputProfile,
    /// [`RuntimeProductRequest::serialization`]: no execution consumer.
    RuntimeSerialization,
    /// [`IdeProductRequest::ide_chunk_boundaries`]: the carrier bridge
    /// SUBSTITUTES this axis with one derived from the selected template
    /// block, so a caller-stated value never reaches codegen.
    IdeChunkBoundaries,
    /// [`IdeProductRequest::output_profile`]: no execution consumer.
    IdeOutputProfile,
    /// [`IdeProductRequest::diagnostics`]: no execution consumer.
    IdeDiagnosticsPresentation,
    /// [`IdeProductRequest::serialization`]: no execution consumer.
    IdeSerialization,
    /// [`super::CompileRequest::semantic_profile`]: read only by the
    /// standalone route, never by the host bundle execution.
    SemanticProfile,
}

/// The one reader for [`UnroutableHostRequestAxis`]: the product-set rows
/// in request order, then the request-level rows. Both host integrations
/// call it, so neither can drift on which axes it refuses.
#[must_use]
pub fn unroutable_host_request_axis(
    request: &super::CompileRequest,
) -> Option<UnroutableHostRequestAxis> {
    for product in request.products() {
        let axis = match product {
            CompileProduct::RuntimeClient(runtime) | CompileProduct::RuntimeServer(runtime) => [
                (
                    runtime.output_profile.is_some(),
                    UnroutableHostRequestAxis::RuntimeOutputProfile,
                ),
                (
                    runtime.serialization.is_some(),
                    UnroutableHostRequestAxis::RuntimeSerialization,
                ),
            ]
            .into_iter()
            .find_map(|(present, axis)| present.then_some(axis)),
            CompileProduct::IdeCompanion(ide) => [
                (
                    ide.ide_chunk_boundaries,
                    UnroutableHostRequestAxis::IdeChunkBoundaries,
                ),
                (
                    ide.output_profile.is_some(),
                    UnroutableHostRequestAxis::IdeOutputProfile,
                ),
                (
                    ide.diagnostics.is_some(),
                    UnroutableHostRequestAxis::IdeDiagnosticsPresentation,
                ),
                (
                    ide.serialization.is_some(),
                    UnroutableHostRequestAxis::IdeSerialization,
                ),
            ]
            .into_iter()
            .find_map(|(present, axis)| present.then_some(axis)),
            CompileProduct::Analysis(analysis) => (!analysis.want_script_bindings
                && !analysis.want_template_data)
                .then_some(UnroutableHostRequestAxis::AnalysisProducesNothing),
            // Both kinds are refused as unsupported products before any
            // axis of theirs could be consulted.
            CompileProduct::PublicApi(_) | CompileProduct::Declarations(_) => None,
        };
        if axis.is_some() {
            return axis;
        }
    }
    request
        .semantic_profile()
        .is_some()
        .then_some(UnroutableHostRequestAxis::SemanticProfile)
}
