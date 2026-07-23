//! Prepare-rename handling and the shared multi-project fail-closed error.

use tower_lsp_server::jsonrpc::Result;
use tower_lsp_server::ls_types::{PrepareRenameResponse, TextDocumentPositionParams};

use crate::documents::sfc_scanner::scan_sfc_blocks;
use crate::features::rename::prepare_rename;

use super::handler_guard::HandlerGuard;
use super::VerterLanguageServer;

/// The fail-closed error a rename / prepare-rename returns for a carrier owned by
/// multiple configured projects. It is non-silent and carries no edit, so a
/// partial cross-project rename can never ship.
pub(super) fn multi_claimant_rename_unavailable_error() -> tower_lsp_server::jsonrpc::Error {
    tower_lsp_server::jsonrpc::Error {
        // LSP `RequestFailed` (-32803): the request failed for a known, user-facing
        // reason (not a protocol/internal fault). tower-lsp has no named variant.
        code: tower_lsp_server::jsonrpc::ErrorCode::ServerError(-32803),
        message: "verter: rename is unavailable for a carrier owned by multiple TypeScript \
                  projects — a cross-project rename could leave the symbol dangling in sibling \
                  projects. Give the carrier a single owning tsconfig (disambiguate its \
                  include/references) to enable rename."
            .into(),
        data: None,
    }
}

pub(super) async fn handle_prepare_rename(
    server: &VerterLanguageServer,
    params: TextDocumentPositionParams,
) -> Result<Option<PrepareRenameResponse>> {
    let _hg = HandlerGuard::new("prepare_rename");
    let uri = &params.text_document.uri;
    let position = &params.position;

    if server.editor_owns_carrier_rename() {
        return Ok(None);
    }
    if server.documents.get_virtual_source_uri(uri).is_some() {
        return Ok(None);
    }
    if server.carrier_is_multi_claimant(uri) {
        return Err(multi_claimant_rename_unavailable_error());
    }

    let result = (|| {
        let doc = server.documents.get(uri)?;
        let analysis = server.documents.get_analysis(uri);
        let blocks = scan_sfc_blocks(&doc.source);
        let range = prepare_rename(
            position,
            &doc.source,
            &blocks,
            analysis.as_ref(),
            &doc.line_index,
        )?;
        Some(PrepareRenameResponse::Range(range))
    })();

    Ok(result)
}
