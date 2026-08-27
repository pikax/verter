//! A34: a byte-changing preprocessor result adds exactly one parse.
//! Worst-case Vue path (non-CSS dialect, all three stages rewriting) totals five.

use std::sync::Arc;

use verter_css_syntax::parse_style_ir_thread_invocations;
use verter_session::{
    hash_block_content, BlockOverrideEntry, BlockOverrideRequest, CompileProfile, FileLanguage,
    HostConfig, UpsertRequest, VerterHost, VirtualNodeKind, VirtualQuery,
};

fn parse_count() -> u64 {
    parse_style_ir_thread_invocations()
}

fn supplied_entry(request: &verter_session::PreprocessorRequest, code: &str) -> BlockOverrideEntry {
    BlockOverrideEntry {
        correlation_token: request.correlation_token.clone(),
        block_token: request.block_token.clone(),
        owner_revision: request.owner_revision.clone(),
        artifact_token: request.artifact_token.clone(),
        basis_token: request.basis_token.clone(),
        captured_echo: request.captured_echo.clone(),
        source_space_token: request.source_space_token.clone(),
        code: Arc::from(code),
        code_hash: hash_block_content(code),
        source_map: None,
        source_map_hash: None,
        dependencies: Vec::new(),
        diagnostics: Vec::new(),
        processor_identity: "test-preprocessor".to_string(),
        processor_version: "0.0.0-test".to_string(),
        config_fingerprint: None,
    }
}

fn compile_style(host: &VerterHost, canonical_id: &str) -> verter_session::VirtualFileResponse {
    host.get_virtual_file(VirtualQuery {
        raw_id: None,
        canonical_id: Some(canonical_id.to_string()),
        node_kind: Some(VirtualNodeKind::Style { index: 0 }),
        compile_profile: CompileProfile::default(),
    })
    .unwrap_or_else(|error| panic!("style compile must succeed: {error:?}"))
}

fn admit_and_compile(
    host: &VerterHost,
    canonical_id: &str,
    request: &verter_session::PreprocessorRequest,
    code: &str,
) {
    let _ = host
        .apply_block_overrides(BlockOverrideRequest {
            canonical_id: canonical_id.to_string(),
            compile_profile: CompileProfile::default(),
            overrides: vec![supplied_entry(request, code)],
        })
        .expect("completed override is admitted");
    let _ = compile_style(host, canonical_id);
}

#[test]
fn preprocessed_result_adds_exactly_one_parse() {
    // Vue: supplied identity +1. Mutation that reddens: parse the preprocessed
    // result twice (admission + transform fallback).
    {
        let host = VerterHost::new_standalone(HostConfig::default());
        let update = host
            .upsert(UpsertRequest {
                canonical_id: None,
                input_id: "/workspace/RoundTrip.vue".to_string(),
                source: Arc::from(
                    "<template><div class=\"tone\">x</div></template>\
                     <style lang=\"postcss\">.tone { color: red; }</style>",
                ),
                file_language: FileLanguage::vue(),
                aliases: Vec::new(),
            })
            .expect("vue upsert");
        let request = update
            .preprocessor_requests
            .first()
            .expect("postcss needs external preprocessing")
            .clone();
        let before = parse_count();
        admit_and_compile(
            &host,
            &update.canonical_id,
            &request,
            ".tone { color: red; }",
        );
        assert_eq!(
            parse_count() - before,
            1,
            "first byte-changing supplied result must add exactly one parse"
        );

        let before_reuse = parse_count();
        let _ = compile_style(&host, &update.canonical_id);
        assert_eq!(
            parse_count() - before_reuse,
            0,
            "reusing the same sealed supplied result must add zero parses"
        );

        let different = ".tone { color: blue; }";
        let recapture = host
            .upsert(UpsertRequest {
                canonical_id: Some(update.canonical_id.clone()),
                input_id: "/workspace/RoundTrip.vue".to_string(),
                source: Arc::from(
                    "<template><div class=\"tone\">x</div></template>\
                     <style lang=\"postcss\">.tone { color: red; }</style>",
                ),
                file_language: FileLanguage::vue(),
                aliases: Vec::new(),
            })
            .expect("recapture");
        let next_request = recapture
            .preprocessor_requests
            .first()
            .expect("recapture still needs preprocessing")
            .clone();
        let before_next = parse_count();
        admit_and_compile(&host, &recapture.canonical_id, &next_request, different);
        assert_eq!(
            parse_count() - before_next,
            1,
            "a one-byte-different supplied result must add exactly one parse"
        );
    }

    // Svelte shares the same +1 bound at host admission of the supplied CSS.
    {
        let host = VerterHost::new_standalone(HostConfig::default());
        let update = host
            .upsert(UpsertRequest {
                canonical_id: None,
                input_id: "/workspace/RoundTrip.svelte".to_string(),
                source: Arc::from(
                    "<div class=\"tone\">x</div>\n<style lang=\"postcss\">.tone { color: red; }</style>",
                ),
                file_language: FileLanguage::svelte(),
                aliases: Vec::new(),
            })
            .expect("svelte upsert");
        let request = update
            .preprocessor_requests
            .first()
            .expect("postcss needs external preprocessing")
            .clone();
        let before = parse_count();
        let _ = host
            .apply_block_overrides(BlockOverrideRequest {
                canonical_id: update.canonical_id.clone(),
                compile_profile: CompileProfile::default(),
                overrides: vec![supplied_entry(&request, ".tone { color: red; }")],
            })
            .expect("svelte supplied result is admitted");
        assert_eq!(
            parse_count() - before,
            1,
            "Svelte supplied result must add exactly one parse"
        );
    }

    // Worst-case Vue: non-CSS dialect, all three stages rewriting on the
    // supplied CSS identity. Mutation that reddens: add a fourth parse.
    {
        let host = VerterHost::new_standalone(HostConfig::default());
        let before = parse_count();
        let update = host
            .upsert(UpsertRequest {
                canonical_id: None,
                input_id: "/workspace/WorstCase.vue".to_string(),
                source: Arc::from(
                    "<template><div class=\"card\">x</div></template>\
                     <style lang=\"postcss\" scoped module>\
                     .card { color: v-bind(theme); nested { display: block; } }\
                     </style>",
                ),
                file_language: FileLanguage::vue(),
                aliases: Vec::new(),
            })
            .expect("worst-case upsert");
        let request = update
            .preprocessor_requests
            .first()
            .expect("postcss needs external preprocessing")
            .clone();
        // Supplied result is CSS (preprocessor output). Nested syntax is
        // flattened so the modules rewrite sees a class selector.
        admit_and_compile(
            &host,
            &update.canonical_id,
            &request,
            ".card { color: v-bind(theme); }\n.card nested { display: block; }",
        );
        let style = compile_style(&host, &update.canonical_id);
        assert!(
            style.code.contains("var(--"),
            "v-bind stage must rewrite: {}",
            style.code
        );
        assert!(
            style.code.contains("card_"),
            "css-modules stage must rewrite: {}",
            style.code
        );
        assert!(
            style.code.contains("[data-v-"),
            "scoped stage must rewrite: {}",
            style.code
        );
        assert!(
            !style.code.contains("v-bind("),
            "v-bind source must not survive: {}",
            style.code
        );
        assert!(
            !style.code.contains(".card {") && !style.code.contains(".card nested"),
            "unhashed class selectors must not survive modules: {}",
            style.code
        );
        let worst = parse_count() - before;
        assert!(
            worst <= 5,
            "worst-case non-CSS + three Vue stages must not exceed five parses, got {worst}"
        );
        assert_eq!(
            worst, 5,
            "worst-case non-CSS + three Vue stages totals five, got {worst}"
        );
    }
}
