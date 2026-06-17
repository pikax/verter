#![deny(missing_docs)]
//! The framework-neutral public-API projection seam.
//!
//! Some frameworks project a component into a public-API virtual file the IDE
//! type-checks (a Vue SFC's `ComponentPublicInstance`-based `.ts` surface).
//! [`ComponentApiProjector`] is the per-adapter seam that renders that surface;
//! the host's public-API entry selects the projector by the canonical's
//! resolved [`FileLanguage`](verter_language::FileLanguage) adapter id.
//!
//! The Vue leg is the EXEMPT legacy extraction: it delegates to the deep
//! pipeline body (`render_vue_public_api_legacy`) that consumes cached TSC
//! state and external-type collection. A static guard bans semantic dispatch /
//! OXC / query-time resolution in NON-Vue projectors — the legacy Vue body is
//! the sole exemption.

use verter_language::FileLanguage;

use crate::types::{CompileProfile, PublicApiMode, TscResponse};
use crate::VerterHost;

/// One framework's public-API projection policy.
///
/// The host selects the impl by the canonical's resolved
/// [`FileLanguage`](verter_language::FileLanguage) adapter id and calls
/// [`Self::render_api`]; a `None` return is the no-projection answer (exactly
/// the host's pre-registry non-Vue behavior).
pub trait ComponentApiProjector: Send + Sync {
    /// Render the component's public-API surface for the requested mode, or
    /// `None` when this component projects no public-API virtual file.
    fn render_api(&self, cx: ComponentApiProjectorCtx<'_>) -> Option<TscResponse>;
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
}
