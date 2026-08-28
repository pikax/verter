//! NAPI analysis/match boundary charges one top-level parse.

use std::sync::Arc;

use verter_css_syntax::parse_style_ir_thread_invocations;
use verter_session::{FileLanguage, HostConfig, UpsertRequest, VerterHost};

#[test]
fn napi_boundary_charges_one_top_level_parse_entry() {
    let host = VerterHost::new_standalone(HostConfig::default());
    let before = parse_style_ir_thread_invocations();
    let update = host
        .upsert(UpsertRequest {
            canonical_id: None,
            input_id: "/workspace/NapiCard.vue".to_string(),
            source: Arc::from(
                "<template><div class=\"card\">x</div></template>\
                 <style>.card { color: red; }</style>",
            ),
            file_language: FileLanguage::vue(),
            aliases: Vec::new(),
        })
        .expect("upsert");
    let analysis = host.get_analysis(&update.canonical_id).expect("analysis");
    assert!(analysis.styles[0].css.is_some());
    let charged = parse_style_ir_thread_invocations() - before;
    assert!(
        charged <= 1,
        "NAPI analysis must not charge a second parse_style_ir, got {charged}"
    );
}
