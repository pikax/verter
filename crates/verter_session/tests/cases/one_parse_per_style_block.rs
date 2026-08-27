//! One top-level parse entry per style block within a request.

use std::sync::Arc;

use verter_css_syntax::{
    parse_inline_style_declarations_thread_invocations, parse_style_ir_thread_invocations,
};
use verter_semantic::analysis::DomQueryKind;
use verter_session::{
    CompileProfile, CompileTarget, FileLanguage, HostConfig, UpsertRequest, VerterHost,
    VirtualNodeKind, VirtualQuery,
};

fn parse_count() -> u64 {
    parse_style_ir_thread_invocations()
}

fn inline_count() -> u64 {
    parse_inline_style_declarations_thread_invocations()
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

fn compile_main(
    host: &VerterHost,
    canonical_id: &str,
    profile: CompileProfile,
) -> verter_session::VirtualFileResponse {
    host.get_virtual_file(VirtualQuery {
        raw_id: None,
        canonical_id: Some(canonical_id.to_string()),
        node_kind: Some(VirtualNodeKind::Main),
        compile_profile: profile,
    })
    .unwrap_or_else(|error| panic!("main compile must succeed: {error:?}"))
}

fn upsert(host: &VerterHost, id: &str, source: &str, language: FileLanguage) -> String {
    host.upsert(UpsertRequest {
        canonical_id: None,
        input_id: id.to_string(),
        source: Arc::from(source),
        file_language: language,
        aliases: Vec::new(),
    })
    .expect("upsert")
    .canonical_id
}

#[test]
fn each_session_route_charges_one_top_level_parse_entry() {
    // Vue SFC compile — runtime, IDE, runtime+IDE.
    for (label, target) in [
        ("runtime", CompileTarget::BUNDLER),
        ("ide", CompileTarget::IDE),
        ("runtime+ide", CompileTarget::BUNDLER | CompileTarget::IDE),
    ] {
        let host = VerterHost::new_standalone(HostConfig::default());
        let canonical = upsert(
            &host,
            &format!("/workspace/Card-{label}.vue"),
            "<template><div class=\"card\">x</div></template>\
             <style scoped>.card { color: red; }</style>",
            FileLanguage::vue(),
        );
        let profile = CompileProfile {
            target,
            ..CompileProfile::default()
        };
        let before = parse_count();
        if target.needs_style() {
            let compiled = compile_style(&host, &canonical, profile);
            assert!(!compiled.code.is_empty(), "{label} style output");
        } else {
            let analysis = host.get_analysis(&canonical).expect("analysis");
            assert!(analysis.styles[0].css.is_some());
        }
        let charged = parse_count() - before;
        assert!(
            charged <= 1,
            "Vue {label} must not charge a second parse_style_ir, got {charged}"
        );
    }

    // Svelte SFC compile.
    {
        let host = VerterHost::new_standalone(HostConfig::default());
        let canonical = upsert(
            &host,
            "/workspace/Card.svelte",
            "<div class=\"card\">x</div>\n<style>.card { color: red; }</style>",
            FileLanguage::svelte(),
        );
        let before = parse_count();
        let compiled = compile_style(&host, &canonical, CompileProfile::default());
        assert!(!compiled.code.is_empty());
        assert!(
            parse_count() - before <= 1,
            "Svelte compile must not charge a second parse_style_ir"
        );
    }

    // Three distinct inline `style=""` consumers: VDOM, SSR, semantic extract.
    {
        let host = VerterHost::new_standalone(HostConfig::default());
        let source = "<template><div style=\"color: red; font-size: 14px\">x</div></template>";
        let canonical = upsert(
            &host,
            "/workspace/InlineVdom.vue",
            source,
            FileLanguage::vue(),
        );
        let before_inline = inline_count();
        let before_ir = parse_count();
        let _ = compile_main(&host, &canonical, CompileProfile::default());
        let inline = inline_count() - before_inline;
        let ir = parse_count() - before_ir;
        assert!(
            inline >= 1 && inline <= 2,
            "VDOM compile charges the nested inline parse, got inline={inline}"
        );
        assert!(
            ir >= 1 && ir <= 2,
            "VDOM compile charges the nested style_ir parse, got ir={ir}"
        );
    }
    {
        let host = VerterHost::new_standalone(HostConfig::default());
        let source = "<template><div style=\"color: red; font-size: 14px\">x</div></template>";
        let canonical = upsert(
            &host,
            "/workspace/InlineSsr.vue",
            source,
            FileLanguage::vue(),
        );
        let before_inline = inline_count();
        let before_ir = parse_count();
        let profile = CompileProfile {
            ssr: true,
            ..CompileProfile::default()
        };
        let _ = compile_main(&host, &canonical, profile);
        let inline = inline_count() - before_inline;
        let ir = parse_count() - before_ir;
        assert!(
            inline >= 1 && inline <= 2,
            "SSR compile charges the nested inline parse, got inline={inline}"
        );
        assert!(
            ir >= 1 && ir <= 2,
            "SSR compile charges the nested style_ir parse, got ir={ir}"
        );
    }
    {
        let host = VerterHost::new_standalone(HostConfig::default());
        let source = "<template><div style=\"--label: red\">x</div></template>";
        let canonical = upsert(
            &host,
            "/workspace/InlineExtract.vue",
            source,
            FileLanguage::vue(),
        );
        let before_inline = inline_count();
        let before_ir = parse_count();
        let analysis = host.get_analysis(&canonical).expect("inline analysis");
        assert!(analysis.template.is_some());
        assert!(
            inline_count() - before_inline <= 1,
            "extract inline must not re-parse"
        );
        assert!(
            parse_count() - before_ir <= 1,
            "extract nested style_ir must not re-parse"
        );
    }

    // Four DomQueryKind routes — each independently selectable.
    let variants = [
        (
            DomQueryKind::QuerySelector,
            "document.querySelector('.card')",
        ),
        (
            DomQueryKind::QuerySelectorAll,
            "document.querySelectorAll('.card')",
        ),
        (
            DomQueryKind::GetElementById,
            "document.getElementById('app')",
        ),
        (
            DomQueryKind::GetElementsByClassName,
            "document.getElementsByClassName('card')",
        ),
    ];
    assert_eq!(variants.len(), DomQueryKind::ALL.len());
    for (kind, call) in variants {
        let host = VerterHost::new_standalone(HostConfig::default());
        let source = format!(
            "<script>{call};</script>\
             <template><div class=\"card\" id=\"app\">x</div></template>"
        );
        let before = parse_count();
        let canonical = upsert(
            &host,
            &format!("/workspace/Dom-{kind:?}.vue"),
            &source,
            FileLanguage::vue(),
        );
        let analysis = host.get_analysis(&canonical).expect("dom analysis");
        let calls = &analysis.dom_query_calls;
        assert!(
            calls.iter().any(|site| site.kind == kind),
            "{kind:?} must be recorded"
        );
        assert!(
            parse_count() - before <= 1,
            "{kind:?} must not charge a second parse_style_ir"
        );
    }
}

#[test]
fn forward_handoff_of_parsed_ir_charges_no_further_parse_entry() {
    let host = VerterHost::new_standalone(HostConfig::default());
    let canonical = upsert(
        &host,
        "/workspace/Handoff.vue",
        "<template><div class=\"card\">x</div></template>\
         <style scoped>.card { color: red; }</style>",
        FileLanguage::vue(),
    );
    let analysis = host.get_analysis(&canonical).expect("analysis first");
    assert!(analysis.styles[0].css.is_some());
    let before = parse_count();
    let compiled = compile_style(&host, &canonical, CompileProfile::default());
    assert!(!compiled.code.is_empty());
    assert_eq!(
        parse_count() - before,
        0,
        "compile holding a parsed IR must charge no further parse_style_ir"
    );
}
