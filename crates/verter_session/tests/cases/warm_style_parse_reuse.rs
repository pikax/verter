//! A33: a style content identity already parsed charges no additional
//! `parse_style_ir` on a later compile. Gateway count, not cache inspection.

use std::sync::Arc;

use verter_css_syntax::parse_style_ir_thread_invocations;
use verter_session::{
    CompileProfile, FileLanguage, HostConfig, UpsertRequest, VerterHost, VirtualNodeKind,
    VirtualQuery,
};

fn parse_count() -> u64 {
    parse_style_ir_thread_invocations()
}

fn compile_style(
    host: &VerterHost,
    canonical_id: &str,
    profile: CompileProfile,
) -> verter_session::VirtualFileResponse {
    host.get_virtual_file(VirtualQuery {
        raw_id: None,
        canonical_id: Some(canonical_id.to_string()),
        node_kind: Some(VirtualNodeKind::Style { index: 0 }),
        compile_profile: profile,
    })
    .unwrap_or_else(|error| panic!("style compile must succeed: {error:?}"))
}

fn warm_route(id: &str, source: &str, language: FileLanguage) {
    let host = VerterHost::new_standalone(HostConfig::default());
    let update = host
        .upsert(UpsertRequest {
            canonical_id: None,
            input_id: id.to_string(),
            source: Arc::from(source),
            file_language: language,
            aliases: Vec::new(),
        })
        .expect("carrier upsert");
    let analysis = host
        .get_analysis(&update.canonical_id)
        .expect("style analysis must run before compile");
    assert!(
        !analysis.styles.is_empty(),
        "native style analysis must retain a style block"
    );
    assert!(
        analysis.styles[0].css.is_some(),
        "native CSS facts must be projected from the retained parse"
    );

    let first = compile_style(&host, &update.canonical_id, CompileProfile::default());
    assert!(
        !first.code.is_empty(),
        "first compile must emit style output"
    );

    // Distinct compile invocation that misses the host compile cache (the
    // cache is the host's; A33 is parse reuse, not compile-slot reuse).
    // Mutation that reddens this gate: drop the prepared IR while assembling
    // this second request so the transform falls back to a raw parse.
    let second_profile = CompileProfile {
        is_production: true,
        ..CompileProfile::default()
    };
    let before = parse_count();
    let second = compile_style(&host, &update.canonical_id, second_profile);
    assert_eq!(
        parse_count() - before,
        0,
        "warm compile of unchanged style content must charge 0 additional parse_style_ir"
    );
    assert!(
        !second.code.is_empty(),
        "second compile must still emit style output"
    );
}

#[test]
fn warm_style_content_charges_no_additional_parse() {
    warm_route(
        "/workspace/WarmCard.vue",
        "<template><div class=\"card\">x</div></template>\
         <style scoped>.card { color: red; }</style>",
        FileLanguage::vue(),
    );
    warm_route(
        "/workspace/WarmCard.svelte",
        "<div class=\"card\">x</div>\n<style>.card { color: red; }</style>",
        FileLanguage::svelte(),
    );

    // Same style text at a different block origin must not alias the first IR.
    let host = VerterHost::new_standalone(HostConfig::default());
    let first = host
        .upsert(UpsertRequest {
            canonical_id: None,
            input_id: "/workspace/OriginA.vue".to_string(),
            source: Arc::from(
                "<template><div class=\"card\">x</div></template>\
                 <style scoped>.card { color: red; }</style>",
            ),
            file_language: FileLanguage::vue(),
            aliases: Vec::new(),
        })
        .expect("origin A");
    let second = host
        .upsert(UpsertRequest {
            canonical_id: None,
            input_id: "/workspace/OriginB.vue".to_string(),
            source: Arc::from(
                "<script>export default {}</script>\
                 <template><div class=\"card\">x</div></template>\
                 <style scoped>.card { color: red; }</style>",
            ),
            file_language: FileLanguage::vue(),
            aliases: Vec::new(),
        })
        .expect("origin B");
    let first_offset = host
        .get_analysis(&first.canonical_id)
        .expect("origin A analysis")
        .styles[0]
        .content_offset;
    let second_offset = host
        .get_analysis(&second.canonical_id)
        .expect("origin B analysis")
        .styles[0]
        .content_offset;
    assert_ne!(
        first_offset, second_offset,
        "the two fixtures must occupy different style origins"
    );
    let _ = compile_style(&host, &first.canonical_id, CompileProfile::default());
    let before = parse_count();
    let profile = CompileProfile {
        is_production: true,
        ..CompileProfile::default()
    };
    let compiled = compile_style(&host, &second.canonical_id, profile);
    assert_eq!(
        parse_count() - before,
        0,
        "a later compile of a distinct origin still reuses that origin's prepared IR"
    );
    assert!(
        compiled.code.contains("[data-v-"),
        "origin-B compile must still scope: {}",
        compiled.code
    );
}
