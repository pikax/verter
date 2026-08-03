use super::*;

pub(super) fn transport_child_hover_result(
    canonical_id: &str,
    result: std::result::Result<
        crate::server::component_resolve::ChildHoverOutcome,
        verter_session::PublicApiProjectionError,
    >,
) -> Result<crate::server::component_resolve::ChildHoverOutcome> {
    result.map_err(|error| crate::public_api_projection_jsonrpc_error("hover", canonical_id, error))
}
/// Whether the completion position sits inside a style `v-bind(|)` context.
pub(super) fn is_style_v_bind_context(
    server: &VerterLanguageServer,
    uri: &Uri,
    position: &Position,
) -> bool {
    (|| {
        let doc = server.documents.get(uri)?;
        let analysis = server.documents.get_analysis(uri);
        let blocks = project_carrier_blocks_for_document(&doc);
        let offset = doc.line_index.position_to_offset(position)?;
        Some(matches!(
            classify_cursor_context_for_language(
                offset,
                &doc.source,
                &blocks,
                analysis.as_ref(),
                CarrierTemplateLanguage::from_uri(uri.as_str()),
            ),
            CursorContext::Style(crate::features::cursor_context::StyleCursorContext::VBind)
        ))
    })()
    .unwrap_or(false)
}
