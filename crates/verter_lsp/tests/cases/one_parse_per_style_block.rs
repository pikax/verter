//! LSP CSS analysis request charges one top-level parse.

use std::sync::Arc;

use verter_css_syntax::parse_style_ir_thread_invocations;
use verter_lsp::documents::carrier_structure::test_carrier_blocks;
use verter_lsp::documents::line_index::LineIndex;
use verter_lsp::features::color_info::document_colors;
use verter_session::{FileLanguage, HostConfig, UpsertRequest, VerterHost};

#[test]
fn lsp_css_analysis_request_charges_one_top_level_parse_entry() {
    let source = "<template><div class=\"card\">x</div></template>\n\
                  <style>.card { color: #ff0000; }</style>";
    let host = VerterHost::new_standalone(HostConfig::default());
    let before = parse_style_ir_thread_invocations();
    let update = host
        .upsert(UpsertRequest {
            canonical_id: None,
            input_id: "/workspace/LspColors.vue".to_string(),
            source: Arc::from(source),
            file_language: FileLanguage::vue(),
            aliases: Vec::new(),
        })
        .expect("upsert");
    let analysis = host
        .get_analysis(&update.canonical_id)
        .expect("style analysis");
    assert!(
        analysis.styles[0].css.is_some(),
        "native CSS facts must be projected"
    );
    let charged = parse_style_ir_thread_invocations() - before;
    assert!(
        charged <= 1,
        "LSP CSS analysis request must not charge a second parse_style_ir, got {charged}"
    );
    let blocks = test_carrier_blocks(source);
    let line_index = LineIndex::new_utf16(source);
    let after_analysis = parse_style_ir_thread_invocations();
    let colors = document_colors(source, &blocks, Some(&analysis), &line_index);
    assert!(
        !colors.is_empty(),
        "document_colors must emit chips from the analysis"
    );
    assert_eq!(
        parse_style_ir_thread_invocations() - after_analysis,
        0,
        "the LSP color reader must not parse again"
    );
}
