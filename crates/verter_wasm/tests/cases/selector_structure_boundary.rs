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
    let results = verter_ffi::convert::build_selector_match_results(
        &analysis,
        &host.get_source(&update.canonical_id).expect("source"),
    )
    .expect("selector spans in extent");
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

/// Document symbols are converted from the analysis' UTF-8 bytes to UTF-16
/// against the source, and a container symbol spans the range its members
/// really occupy: no container carries a placeholder `0..0` range.
#[test]
fn document_symbols_cover_real_utf16_ranges_after_a_non_ascii_prefix() {
    let host = VerterHost::new_standalone(HostConfig::default());
    // The astral comment makes UTF-8 offsets differ from UTF-16 ones.
    let source = "<!-- \u{1F600} -->\n\
                  <script setup lang=\"ts\">\n\
                  import { ref } from 'vue'\n\
                  const count = ref(0)\n\
                  </script>\n\
                  <template><div class=\"card\">{{ count }}</div></template>\n\
                  <style>\n\
                  .card { color: red; }\n\
                  </style>";
    let update = host
        .upsert(UpsertRequest {
            canonical_id: None,
            input_id: "/workspace/Symbols.vue".to_string(),
            source: Arc::from(source),
            file_language: FileLanguage::vue(),
            aliases: Vec::new(),
        })
        .expect("upsert");
    let analysis = host.get_analysis(&update.canonical_id).expect("analysis");
    let symbols = verter_ffi::convert::build_document_symbols_from_analysis(&analysis, source)
        .expect("symbol spans in extent");
    let units: Vec<u16> = source.encode_utf16().collect();
    assert!(
        !symbols.is_empty(),
        "the file has script, template and style"
    );
    for container in &symbols {
        assert!(
            container.span_start < container.span_end,
            "container {} must cover a real range, not a placeholder",
            container.name
        );
        assert!((container.span_end as usize) <= units.len());
        for child in &container.children {
            assert!(
                container.span_start <= child.span_start && child.span_end <= container.span_end,
                "{} must lie inside its container {}",
                child.name,
                container.name
            );
        }
    }
    let script = symbols
        .iter()
        .find(|symbol| symbol.name == "script")
        .expect("script container");
    let count = script
        .children
        .iter()
        .find(|symbol| symbol.name == "count")
        .expect("count binding");
    assert_eq!(
        String::from_utf16(&units[count.span_start as usize..count.span_end as usize]).unwrap(),
        "count"
    );
}
