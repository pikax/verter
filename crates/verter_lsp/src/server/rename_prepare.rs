//! Prepare-rename handling and the shared fail-closed rename errors.

use tower_lsp_server::jsonrpc::Result;
use tower_lsp_server::ls_types::{PrepareRenameResponse, TextDocumentPositionParams};

use crate::features::rename::UnenumeratedRegion;
use crate::type_provider::merge;

use super::handler_guard::HandlerGuard;
use super::rename_plan::{
    rename_request_admission, RenameAdmission, RenamePlan, RenameTargetResolution,
};
use super::VerterLanguageServer;

/// LSP `RequestFailed` (-32803): the request failed for a known, user-facing
/// reason (not a protocol/internal fault). tower-lsp has no named variant.
const REQUEST_FAILED: tower_lsp_server::jsonrpc::ErrorCode =
    tower_lsp_server::jsonrpc::ErrorCode::ServerError(-32803);

/// The fail-closed error a rename / prepare-rename returns for a carrier owned by
/// multiple configured projects. It is non-silent and carries no edit, so a
/// partial cross-project rename can never ship.
pub(super) fn multi_claimant_rename_unavailable_error() -> tower_lsp_server::jsonrpc::Error {
    tower_lsp_server::jsonrpc::Error {
        code: REQUEST_FAILED,
        message: "verter: rename is unavailable for a carrier owned by multiple TypeScript \
                  projects — a cross-project rename could leave the symbol dangling in sibling \
                  projects. Give the carrier a single owning tsconfig (disambiguate its \
                  include/references) to enable rename."
            .into(),
        data: None,
    }
}

/// The user-visible reason a rename was refused because Verter's own same-file
/// occurrence inventory can never cover this file — `None` when the shortfall
/// has no user-actionable cause to state.
///
/// REPORTING ONLY, and deliberately partial. It converts a refusal that already
/// happened into one the user can act on; it never decides that a refusal
/// happens. A region with no error here leaves its refusal exactly as silent as
/// it was, which is why the match is EXHAUSTIVE rather than a catch-all: a new
/// [`UnenumeratedRegion`] must be classified deliberately, not defaulted into
/// (or out of) a user-facing message.
///
/// Only [`UnenumeratedRegion::StyleVBindExpression`] qualifies today. It is the
/// one TERMINAL shortfall that is Verter's own limitation AND is fixable by the
/// author: the identifier is named by a `<style>` `v-bind()` expression that
/// Verter's rename span set structurally never carries, and no carrier capability
/// can make that claim whole. The other two are not the user's problem to solve —
/// [`UnenumeratedRegion::MarkupOccurrences`] is granted by the plan owner for any
/// carrier that models markup occurrences, and
/// [`UnenumeratedRegion::NoOccurrenceInventory`] means the TypeScript provider,
/// not Verter, is the authority for the position.
pub(super) fn rename_incompleteness_error(
    region: Option<UnenumeratedRegion>,
) -> Option<tower_lsp_server::jsonrpc::Error> {
    match region? {
        UnenumeratedRegion::StyleVBindExpression => Some(tower_lsp_server::jsonrpc::Error {
            code: REQUEST_FAILED,
            message: "verter: rename is unavailable for a binding referenced by a <style> \
                      v-bind() expression — Verter's rename does not rewrite style expressions, \
                      so it cannot prove the rename would be complete and refuses rather than \
                      leaving v-bind() bound to a name that no longer exists. Rename the binding \
                      and its v-bind() expression by hand, or move the value out of <style> to \
                      enable rename."
                .into(),
            data: None,
        }),
        UnenumeratedRegion::MarkupOccurrences | UnenumeratedRegion::NoOccurrenceInventory => None,
    }
}

/// Answer `textDocument/prepareRename` through the shared rename-plan owner.
///
/// The server advertises `prepareProvider: true`, so this answer decides whether
/// the client ever sends `textDocument/rename` at all — a `null` here means the
/// editor aborts and the position's authority is never consulted. So the plan
/// comes from the ONE shared classification
/// ([`RenameTargetResolution::prepare_plan`]) and, where the TypeScript provider
/// is the sole authority, this handler ASKS it.
pub(super) async fn handle_prepare_rename(
    server: &VerterLanguageServer,
    params: TextDocumentPositionParams,
) -> Result<Option<PrepareRenameResponse>> {
    let _hg = HandlerGuard::new("prepare_rename");
    let uri = &params.text_document.uri;
    let position = &params.position;

    match rename_request_admission(server, uri) {
        RenameAdmission::Decline => return Ok(None),
        RenameAdmission::Refuse(error) => return Err(error),
        RenameAdmission::Serve => {}
    }

    let resolution = RenameTargetResolution::resolve(server, uri, position).await;
    Ok(match resolution.prepare_plan() {
        RenamePlan::Offer(range) => Some(PrepareRenameResponse::Range(range)),
        RenamePlan::ProbeProvider { anchor } => {
            provider_proves_rename_target(server, uri, position, anchor)
                .await
                .then_some(PrepareRenameResponse::Range(anchor))
        }
        RenamePlan::Decline => None,
    })
}

/// Whether the TypeScript provider — the SOLE semantic authority at a
/// provider-only position — proves a rename target that maps safely back onto
/// the authored token at `anchor`.
///
/// Every row of the fail-closed matrix answers `false`, and `false` means prepare
/// offers nothing:
///
/// * provider ABSENT (none configured) — nothing to ask;
/// * a SELF-FILE rune-module projection — its workspace-edit positions are not
///   mapped through the self-file mapper, so `handle_rename` defers there and
///   prepare must not advertise what rename will not serve;
/// * no captured provider surface, or the cursor does not map into the generated
///   TSX (unsafe mapping);
/// * the configured-project carrier frontier is incomplete — the same
///   completeness precondition `handle_rename` requires;
/// * the provider ERRORS (transport/engine failure, and where a timeout lands);
/// * the provider answers an EMPTY location set — which is also what a carrier
///   denied by feature admission serves, so it proves nothing;
/// * the captured surface was superseded before the response was read
///   (post-await validation);
/// * no returned location maps back onto the authored token under the cursor —
///   incomplete transaction geometry: an offer would promise the editor a rename
///   of a range the transaction cannot deliver.
///
/// A `true` answer is NOT authority for the follow-up rename: `handle_rename`
/// re-resolves, re-queries, re-validates, and applies its own completeness gates.
/// This probe reads only the CURRENT request's carrier surface — it captures no
/// API/foreign surface set and holds no rename fence, because it emits no edit.
async fn provider_proves_rename_target(
    server: &VerterLanguageServer,
    uri: &tower_lsp_server::ls_types::Uri,
    position: &tower_lsp_server::ls_types::Position,
    anchor: tower_lsp_server::ls_types::Range,
) -> bool {
    let Some(type_provider) = &server.type_provider else {
        return false;
    };
    if server.is_self_file_projection(uri) {
        return false;
    }
    let Some(ctx) = server.repaired_type_provider_context(uri).await else {
        return false;
    };
    let Some(tsx_offset) = merge::carrier_position_to_tsx_offset_validated(
        position,
        &ctx.carrier_line_index,
        &ctx.mapper,
        &ctx.tsx_line_index,
    ) else {
        return false;
    };
    if !server.prepare_workspace_symbol_frontier(uri).await {
        tracing::debug!("prepare_rename: configured-project carrier frontier is not complete");
        return false;
    }
    let Ok(locations) = type_provider
        .get_rename_locations(&ctx.tsx_path, tsx_offset)
        .await
    else {
        tracing::debug!("prepare_rename: provider rename query failed — offering nothing");
        return false;
    };
    if locations.is_empty() {
        return false;
    }
    // Post-await validation (fail closed): a response produced against a
    // superseded surface must not be mapped, so it proves nothing either.
    if !server.provider_context_still_valid(uri, &ctx) {
        tracing::debug!("prepare_rename: dropping provider answer — captured surface is stale");
        return false;
    }
    // The provider must own THIS authored token: one of its locations, mapped
    // back through the same captured surface, has to land exactly on `anchor`.
    locations.iter().any(|location| {
        verter_span::path::fs_paths_equal(&location.path, &ctx.tsx_path)
            && merge::tsx_range_to_carrier_range(
                location.start,
                location.end,
                &ctx.tsx_line_index,
                &ctx.mapper,
                &ctx.carrier_line_index,
            ) == Some(anchor)
    })
}
