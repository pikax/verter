//! CSS-native navigation legs shared by the `nav_features_navigation`
//! handlers: the workspace-wide global-class definition/reference extensions.

use tower_lsp_server::ls_types::*;

use crate::documents::carrier_structure::project_carrier_blocks_for_document;

use super::handler_guard::block_in_place_if_available;
use super::VerterLanguageServer;

/// The GLOBAL css class at the request position, when this file declares the
/// class globally (non-scoped block or `:global(...)`).
pub(super) fn global_class_target(
    server: &VerterLanguageServer,
    uri: &Uri,
    position: &Position,
) -> Option<String> {
    let doc = server.documents.get(uri)?;
    let analysis = server.documents.get_analysis(uri)?;
    let offset = doc.line_index.position_to_offset(position)? as usize;
    let blocks = project_carrier_blocks_for_document(&doc);
    crate::css::global_classes::global_class_target_at(offset, &doc.source, &blocks, &analysis)
}
/// Extend `base` definition locations with every OTHER file's global
/// declarations of `class_name`, deduplicated. A cross-file population the
/// workspace index could not certify answers the retryable partial error
/// rather than a list that reads as complete.
pub(super) fn merge_global_class_definitions(
    server: &VerterLanguageServer,
    uri: &Uri,
    class_name: &str,
    base: Option<GotoDefinitionResponse>,
) -> tower_lsp_server::jsonrpc::Result<Option<GotoDefinitionResponse>> {
    let origin_canonical = server.documents.get_canonical_id(uri);
    let encoding = server.position_encoding.read().clone();
    let mut locations: Vec<Location> = match base {
        Some(GotoDefinitionResponse::Scalar(loc)) => vec![loc],
        Some(GotoDefinitionResponse::Array(locs)) => locs,
        _ => Vec::new(),
    };
    let cross = block_in_place_if_available(|| {
        crate::css::global_classes::collect_cross_file_global_class_locations(
            server.documents.host(),
            origin_canonical.as_deref(),
            class_name,
            encoding,
            true,
        )
    });
    if let Some(shortfall) = &cross.shortfall {
        return Err(super::server_utils::index_partial_error(shortfall));
    }
    push_deduped(&mut locations, cross.value);
    Ok(match locations.len() {
        0 => None,
        1 => Some(GotoDefinitionResponse::Scalar(
            locations.into_iter().next().unwrap(),
        )),
        _ => Some(GotoDefinitionResponse::Array(locations)),
    })
}

/// Extend `base` reference locations with every OTHER file's global
/// declarations AND usages of `class_name`, deduplicated. A cross-file
/// population the workspace index could not certify answers the retryable
/// partial error rather than a list that reads as complete.
pub(super) fn merge_global_class_references(
    server: &VerterLanguageServer,
    uri: &Uri,
    class_name: &str,
    mut base: Vec<Location>,
) -> tower_lsp_server::jsonrpc::Result<Vec<Location>> {
    let origin_canonical = server.documents.get_canonical_id(uri);
    let encoding = server.position_encoding.read().clone();
    let cross = block_in_place_if_available(|| {
        crate::css::global_classes::collect_cross_file_global_class_locations(
            server.documents.host(),
            origin_canonical.as_deref(),
            class_name,
            encoding,
            false,
        )
    });
    if let Some(shortfall) = &cross.shortfall {
        return Err(super::server_utils::index_partial_error(shortfall));
    }
    push_deduped(&mut base, cross.value);
    Ok(base)
}

fn push_deduped(locations: &mut Vec<Location>, extra: Vec<Location>) {
    for loc in extra {
        if !locations
            .iter()
            .any(|l| l.uri == loc.uri && l.range == loc.range)
        {
            locations.push(loc);
        }
    }
}
