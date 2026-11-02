//! Diagnostics capability slot.
//!
//! Pull diagnostics removed — we use push diagnostics exclusively.
//! Push diagnostics stay visible during typing (VS Code adjusts their positions
//! as the document changes), eliminating the flickering caused by pull diagnostics
//! returning stale/incomplete results during typing cooldown.
//!
//! This slot therefore contributes no cell today; it is the single home for
//! the advertised diagnostics surface should that change.

use super::CapabilityContext;
use tower_lsp_server::ls_types::ServerCapabilities;

pub(crate) fn contribute(_caps: &mut ServerCapabilities, _ctx: &CapabilityContext) {}
