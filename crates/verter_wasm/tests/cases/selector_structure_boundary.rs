//! Unstructurable selectors are skipped at the WASM match boundary.
//! A selector recorded without structure is not re-parsed.

use std::sync::Arc;

use verter_semantic::analysis::parse_selector_thread_invocations;
use verter_session::{FileLanguage, HostConfig, UpsertRequest, VerterHost};

#[test]
fn wasm_boundary_skips_unstructurable_selector() {
    let host = VerterHost::new_standalone(HostConfig::default());
    let source = "<template><div class=\"card\">x</div></template>\n\
                  <style lang=\"scss\">\n\
                  .card { color: red; }\n\
                  .foo#{$x} { color: blue; }\n\
                  </style>";
    let update = host
        .upsert(UpsertRequest {
            canonical_id: None,
            input_id: "/workspace/Skip.vue".to_string(),
            source: Arc::from(source),
            file_language: FileLanguage::vue(),
            aliases: Vec::new(),
        })
        .expect("upsert");
    let analysis = host.get_analysis(&update.canonical_id).expect("analysis");
    let css = analysis.styles[0].css.as_ref().expect("css");
    assert!(
        css.selectors
            .iter()
            .any(|selector| selector.text == ".card" && selector.structure.is_some()),
        "structurable sibling is retained: {:?}",
        css.selectors.iter().map(|s| &s.text).collect::<Vec<_>>()
    );
    assert!(
        css.selectors
            .iter()
            .any(|selector| selector.structure.is_none()),
        "an unstructurable interpolation must be recorded without structure"
    );

    let before = parse_selector_thread_invocations();
    let results = verter_wasm::build_selector_match_results(
        &analysis,
        &host.get_source(&update.canonical_id).expect("source"),
    );
    assert_eq!(
        parse_selector_thread_invocations() - before,
        0,
        "the WASM boundary must skip, not re-parse, structure-less selectors"
    );
    let texts: Vec<&str> = results
        .iter()
        .map(|result| result.selector_text.as_str())
        .collect();
    assert!(
        texts.contains(&".card"),
        "structurable selector still matches: {texts:?}"
    );
    assert!(
        texts.iter().all(|text| *text != ".foo#{$x}"),
        "unstructurable selector is skipped: {texts:?}"
    );
}
