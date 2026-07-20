#![deny(missing_docs)]
//! The framework-neutral public-API projection seam.
//!
//! Some frameworks project a component into a public-API virtual file the IDE
//! type-checks (a Vue SFC's `ComponentPublicInstance`-based `.ts` surface).
//! [`ComponentApiProjector`] is the per-adapter seam that renders that surface;
//! the host's public-API entry selects the projector by the canonical's
//! resolved [`FileLanguage`](verter_language::FileLanguage) adapter id.
//!
//! The Vue leg is the legacy extraction: it delegates to the deep pipeline
//! body (`render_vue_public_api_legacy`) that consumes cached TSC state and
//! external-type collection. Adapter projectors may consume their cached AST
//! facts and ask the shared framework-surface executor to dereference authored
//! locators; they must not reparse source or introduce a private resolver.

use verter_language::FileLanguage;

use crate::types::{CompileProfile, PublicApiMode, PublicApiProjectionError, TscResponse};
use crate::VerterHost;

/// One resolved public prop exposed to editor/host consumers alongside a
/// framework component's declaration carrier.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ComponentPublicProp {
    /// Authored public prop name.
    pub name: String,
    /// Best safe resolved display type. `None` is an honest unresolved row.
    pub type_annotation: Option<String>,
    /// Whether callers may omit the prop.
    pub optional: bool,
    /// Whether the framework captured an authored runtime default.
    pub has_default: bool,
}

/// Framework-neutral public component contract produced from semantic facts,
/// never reconstructed by parsing the generated declaration text.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ComponentPublicContract {
    /// Public prop rows in the framework surface's stable source order.
    pub props: Vec<ComponentPublicProp>,
}

/// One projector result: the declaration response plus its structured public
/// contract when the adapter provides one.
#[derive(Debug, Clone)]
pub struct ComponentApiProjection {
    /// Generated public declaration surface.
    pub response: TscResponse,
    /// Semantic public contract. `None` preserves adapters whose established
    /// public surface has not opted into this sidecar.
    pub contract: Option<ComponentPublicContract>,
}

/// One framework's public-API projection policy.
///
/// The host selects the impl by the canonical's resolved
/// [`FileLanguage`](verter_language::FileLanguage) adapter id and calls
/// [`Self::render_api`]; `Ok(None)` is the no-projection answer, while a
/// selected carrier's projection failure remains a typed error.
pub trait ComponentApiProjector: Send + Sync {
    /// Render the component's public-API surface for the requested mode.
    ///
    /// `Ok(None)` means the adapter intentionally exposes no public-API
    /// virtual file for this language/mode. Projection refusals return their
    /// exact typed failure.
    fn render_api(
        &self,
        cx: ComponentApiProjectorCtx<'_>,
    ) -> Result<Option<ComponentApiProjection>, PublicApiProjectionError>;
}

/// The public-API projection context.
///
/// Carries the canonical, the canonical's RUNTIME-loaded [`FileLanguage`]
/// (the explicit `UpsertRequest.file_language` the source was loaded with —
/// the same authority the pre-registry Vue gate consulted, NOT a static path
/// re-classification), the requested [`PublicApiMode`], the optional compile
/// [`CompileProfile`], and the host handle the Vue legacy flow needs.
pub struct ComponentApiProjectorCtx<'a> {
    /// The host handle the projector renders against.
    pub host: &'a VerterHost,
    /// The ALREADY-alias-resolved canonical id the host classified. The
    /// projector renders against THIS exact target (it does NOT re-resolve the
    /// alias) so classification and rendering operate on one coherent
    /// canonical — a concurrent alias relabel cannot classify one target and
    /// render another.
    pub resolved_canonical: &'a str,
    /// The canonical's runtime-loaded language row, captured for the SAME
    /// `resolved_canonical` — the per-adapter leg matches it against its
    /// descriptor's `carrier_language` so a same-adapter non-carrier language
    /// (e.g. a template row) does not enter the carrier-only public-API flow.
    pub file_language: &'a FileLanguage,
    /// The requested public-API surface mode.
    pub mode: PublicApiMode,
    /// The compile profile, when script/content overrides apply.
    pub profile: Option<&'a CompileProfile>,
    /// The batch-shared cold seed + active session view (crate-private; least
    /// authority). `Some` on every host render path (scalar `N=1` and batch).
    /// Vue consumes it for cross-file macro-type resolution. Svelte uses the
    /// same request seed to dereference AST-captured `$props()` and dispatcher
    /// locators through the shared framework-surface executor.
    pub(crate) render_seed: Option<PublicApiRenderSeed<'a>>,
}

/// The batch-shared cold-seed store view + active session view threaded into a
/// public-API render so a render takes ZERO per-call store-view reads.
///
/// Captured ONCE — per scalar call (`N=1`) or per batch — as a
/// [`crate::resolver_store::BatchFixedView`] and shared across every item: the
/// O(N²) store-view-cliff collapse. Least authority: the raw `BatchFixedView`
/// is intentionally NOT exposed; the projector only needs the cold seed to
/// build its request-bound resolver context.
pub(crate) struct PublicApiRenderSeed<'a> {
    /// The batch-shared OVERLAID cold-seed for the external-type collection /
    /// extraction resolver context. Reused across every item; the cold compute
    /// seeds from it WITHOUT a fresh per-item `resolver_store_view_read()`.
    pub(crate) cold_seed: &'a crate::resolver_store::ColdSeedHostStoreView,
    /// The exact session/profile view the cold seed was rooted through.
    /// Profile-owned block overrides ride this view as one immutable source
    /// overlay, so syntax extraction, semantic macro projection, and revision
    /// fencing all observe the same bytes.
    pub(crate) view: &'a dyn crate::session_view::SessionView,
}
