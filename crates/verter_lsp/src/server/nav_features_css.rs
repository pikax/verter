//! CSS-native navigation legs shared by the `nav_features_navigation`
//! handlers: the editor-owned css-only serving (class-token definition /
//! references — results with NO TS correlate the editor's TS plugin can never
//! own) and the workspace-wide global-class extensions.

use tower_lsp_server::ls_types::*;

use crate::documents::sfc_scanner::scan_sfc_blocks;

use super::handler_guard::block_in_place_if_available;
use super::VerterLanguageServer;

/// Rewrite same-file sentinel URIs inside a definition response.
pub(super) fn fix_definition_sentinel_uris(def: &mut GotoDefinitionResponse, uri: &Uri) {
    match def {
        GotoDefinitionResponse::Scalar(loc) => {
            if loc.uri.as_str() == crate::features::definition::SAME_FILE_URI_STR {
                loc.uri = uri.clone();
            }
        }
        GotoDefinitionResponse::Array(locs) => {
            for loc in locs.iter_mut() {
                if loc.uri.as_str() == crate::features::definition::SAME_FILE_URI_STR {
                    loc.uri = uri.clone();
                }
            }
        }
        GotoDefinitionResponse::Link(_) => {}
    }
}

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
    let blocks = scan_sfc_blocks(&doc.source);
    crate::css::global_classes::global_class_target_at(offset, &doc.source, &blocks, &analysis)
}

/// Extend `base` definition locations with every OTHER file's global
/// declarations of `class_name`, deduplicated.
pub(super) fn merge_global_class_definitions(
    server: &VerterLanguageServer,
    uri: &Uri,
    class_name: &str,
    base: Option<GotoDefinitionResponse>,
) -> Option<GotoDefinitionResponse> {
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
    push_deduped(&mut locations, cross);
    match locations.len() {
        0 => None,
        1 => Some(GotoDefinitionResponse::Scalar(
            locations.into_iter().next().unwrap(),
        )),
        _ => Some(GotoDefinitionResponse::Array(locations)),
    }
}

/// Extend `base` reference locations with every OTHER file's global
/// declarations AND usages of `class_name`, deduplicated.
pub(super) fn merge_global_class_references(
    server: &VerterLanguageServer,
    uri: &Uri,
    class_name: &str,
    mut base: Vec<Location>,
) -> Vec<Location> {
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
    push_deduped(&mut base, cross);
    base
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

/// The editor-owned definition response: EXACTLY the css leg (class token →
/// rule, style class → usages) plus the global-class cross-file declarations.
pub(super) fn editor_owned_css_definition(
    server: &VerterLanguageServer,
    uri: &Uri,
    position: &Position,
) -> Option<GotoDefinitionResponse> {
    let css_result = (|| {
        let doc = server.documents.get(uri)?;
        let analysis = server.documents.get_analysis(uri);
        let blocks = scan_sfc_blocks(&doc.source);
        let mut def = crate::features::definition::css_only_definition_at_position(
            position,
            &doc.source,
            &blocks,
            analysis.as_ref(),
            &doc.line_index,
        )?;
        fix_definition_sentinel_uris(&mut def, uri);
        Some(def)
    })();
    if let Some(class_name) = global_class_target(server, uri, position) {
        return merge_global_class_definitions(server, uri, &class_name, css_result);
    }
    css_result
}

/// The editor-owned references response: EXACTLY the css leg (same-file
/// occurrences) plus the workspace-wide global-class extension.
pub(super) fn editor_owned_css_references(
    server: &VerterLanguageServer,
    uri: &Uri,
    position: &Position,
) -> Option<Vec<Location>> {
    let mut locations = {
        let doc = server.documents.get(uri)?;
        let analysis = server.documents.get_analysis(uri)?;
        let offset = doc.line_index.position_to_offset(position)? as usize;
        let blocks = scan_sfc_blocks(&doc.source);
        let mut locations = crate::features::references::css_only_references_at_position(
            offset,
            &doc.source,
            &blocks,
            &analysis,
            &doc.line_index,
        )?;
        for loc in &mut locations {
            if loc.uri.as_str() == crate::features::references::SAME_FILE_URI_STR {
                loc.uri = uri.clone();
            }
        }
        locations
    };
    if let Some(class_name) = global_class_target(server, uri, position) {
        locations = merge_global_class_references(server, uri, &class_name, locations);
    }
    (!locations.is_empty()).then_some(locations)
}
