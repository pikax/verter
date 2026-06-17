#![deny(missing_docs)]
//! The Vue public-API projector leg.
//!
//! Delegates to `render_vue_public_api_legacy` (the deep pipeline body in
//! `host_resolve/virtual_file_pipeline.rs`): the Vue surface is the EXEMPT
//! legacy extraction that consumes cached TSC state and external-type
//! collection. The host's public-API entry and this leg converge on that one
//! body.

use crate::framework::api_projector::{ComponentApiProjector, ComponentApiProjectorCtx};
use crate::types::TscResponse;

/// The Vue component-API projector.
#[derive(Debug, Default)]
pub struct VueComponentApiProjector;

impl ComponentApiProjector for VueComponentApiProjector {
    fn render_api(&self, cx: ComponentApiProjectorCtx<'_>) -> Option<TscResponse> {
        let ComponentApiProjectorCtx {
            host,
            resolved_canonical,
            file_language,
            mode,
            profile,
        } = cx;
        // Carrier-narrowness: the public-API surface is produced only for the
        // Vue SFC CARRIER row, never a same-adapter non-carrier row (e.g. a
        // Vue template). Confirm the runtime language is this adapter's
        // carrier language (the descriptor's `carrier_language`) — the
        // registry-driven decomposition of the pre-registry `is_vue()` gate
        // (adapter routing happens at dispatch; carrier match happens here).
        let descriptor = crate::framework::descriptor::vue_descriptor();
        if file_language.carrier_language_id() != descriptor.carrier_language.as_ref() {
            return None;
        }
        // Render against the ALREADY-resolved canonical the host classified —
        // not the raw alias id — so the language gate and the render share one
        // alias resolution (no classify-one / render-another split).
        host.render_vue_public_api_legacy(resolved_canonical, mode, profile)
    }
}
