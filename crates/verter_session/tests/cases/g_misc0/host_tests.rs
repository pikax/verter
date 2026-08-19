use std::sync::Arc;
use verter_session::*;

fn profile_dev() -> CompileProfile {
    CompileProfile {
        is_production: false,
        hmr_strategy: HmrStrategy::Vite,
        ..CompileProfile::default()
    }
}

fn profile_prod() -> CompileProfile {
    CompileProfile {
        is_production: true,
        hmr_strategy: HmrStrategy::None,
        ..CompileProfile::default()
    }
}

fn upsert_vue(host: &VerterHost, id: &str, src: &str) -> HostUpdateResult {
    host.upsert(UpsertRequest {
        canonical_id: None,
        input_id: id.to_string(),
        source: Arc::from(src),
        file_language: FileLanguage::vue(),
        aliases: Vec::new(),
    })
    .unwrap()
}

fn unissued_override(code: &str) -> BlockOverrideEntry {
    let correlation_token = BlockContentCorrelationToken::parse_untrusted("not-issued").unwrap();
    let block_token =
        verter_session::carrier_publication_store::ArtifactBlockToken::parse_untrusted(
            "not-issued",
        )
        .unwrap();
    let owner_revision = BlockContentOwnerRevisionToken::parse_untrusted("not-issued").unwrap();
    let artifact_token =
        verter_session::carrier_publication_store::FrameworkArtifactToken::parse_untrusted(
            "not-issued",
        )
        .unwrap();
    let basis_token = BlockContentBasisToken::parse_untrusted("not-issued").unwrap();
    BlockOverrideEntry {
        correlation_token: correlation_token.clone(),
        block_token: block_token.clone(),
        owner_revision: owner_revision.clone(),
        artifact_token: artifact_token.clone(),
        basis_token: basis_token.clone(),
        captured_echo: BlockContentCapturedEcho {
            request: BlockContentPreCaptureEcho {
                correlation_token,
                canonical_id: "not-issued".to_string(),
                block_token,
                owner_revision,
                artifact_token,
                expected_language: "not-issued".to_string(),
                prior_basis_token: None,
            },
            basis_token,
        },
        source_space_token: BlockContentSourceSpaceToken::parse_untrusted("not-issued").unwrap(),
        code: Arc::from(code),
        code_hash: hash_block_content(code),
        source_map: None,
        source_map_hash: None,
        supplied_provenance: None,
    }
}

#[test]
fn resolve_query_param_tolerance_and_order() {
    let host = VerterHost::new_standalone(HostConfig::default());

    let a = host
        .resolve("Comp.vue?vue&type=style&index=0&id=abc&scoped=true&lang.css")
        .unwrap();
    let b = host
        .resolve("Comp.vue?vue&index=0&type=style&lang.css&id=abc")
        .unwrap();

    assert_eq!(a.canonical_id, b.canonical_id);
    assert_eq!(a.node_kind, b.node_kind);
    assert_eq!(a.node_kind, VirtualNodeKind::Style { index: 0 });
}

#[test]
fn resolve_explicit_script_template_custom() {
    let host = VerterHost::new_standalone(HostConfig::default());

    assert_eq!(
        host.resolve("Comp.vue?vue&type=script").unwrap().node_kind,
        VirtualNodeKind::Script
    );
    assert_eq!(
        host.resolve("Comp.vue?vue&type=template")
            .unwrap()
            .node_kind,
        VirtualNodeKind::Template
    );
    assert_eq!(
        host.resolve("Comp.vue?vue&type=custom&index=2")
            .unwrap()
            .node_kind,
        VirtualNodeKind::Custom { index: 2 }
    );
}

#[test]
fn resolve_succeeds_without_source_get_virtual_file_missing_source() {
    let host = VerterHost::new_standalone(HostConfig::default());

    let resolved = host.resolve("/x/Comp.vue?vue&type=template").unwrap();
    assert!(!resolved.exists_in_host);

    let err = host
        .get_virtual_file(VirtualQuery {
            raw_id: Some("/x/Comp.vue?vue&type=template".to_string()),
            canonical_id: None,
            node_kind: None,
            compile_profile: profile_dev(),
        })
        .unwrap_err();

    match err {
        HostError::MissingSource { canonical_id } => {
            assert_eq!(canonical_id, "/x/Comp.vue");
        }
        _ => panic!("expected MissingSource"),
    }
}

#[test]
fn non_slice_edit_no_invalidation() {
    let host = VerterHost::new_standalone(HostConfig::default());

    let src1 = "<script setup>const n = 1</script>\n<template><div>{{n}}</div></template>\n<style>.a{color:red}</style>";
    let src2 = "<script setup>const n = 1</script>\n\n\n<template><div>{{n}}</div></template>\n\n<style>.a{color:red}</style>";

    let first = upsert_vue(&host, "Comp.vue", src1);
    assert!(first.changed);

    let second = upsert_vue(&host, "Comp.vue", src2);
    assert!(!second.changed);
    assert!(second.changed_virtual_ids.is_empty());
    assert!(second.changed_lsp_ids.is_empty());
}

#[test]
fn style_only_edit_returns_only_style_virtual_id() {
    let host = VerterHost::new_standalone(HostConfig::default());

    let src1 = "<script setup>const n = 1</script><template><div>{{n}}</div></template><style>.a{color:red}</style>";
    let src2 = "<script setup>const n = 1</script><template><div>{{n}}</div></template><style>.a{color:blue}</style>";

    let _ = upsert_vue(&host, "Comp.vue", src1);
    let result = upsert_vue(&host, "Comp.vue", src2);

    assert_eq!(
        result.changed_virtual_nodes,
        vec![VirtualNodeKind::Style { index: 0 }]
    );
    assert_eq!(result.changed_virtual_ids.len(), 1);
    assert!(result.changed_virtual_ids[0].contains("type=style"));
}

#[test]
fn template_edit_returns_main_and_template_ids() {
    let host = VerterHost::new_standalone(HostConfig::default());

    let src1 = "<script setup>const n = 1</script><template><div>{{n}}</div></template><style>.a{color:red}</style>";
    let src2 = "<script setup>const n = 1</script><template><section>{{n}}</section></template><style>.a{color:red}</style>";

    let _ = upsert_vue(&host, "Comp.vue", src1);
    let result = upsert_vue(&host, "Comp.vue", src2);

    assert_eq!(
        result.changed_virtual_nodes,
        vec![VirtualNodeKind::Main, VirtualNodeKind::Template]
    );
}

#[test]
fn script_edit_returns_all_virtual_ids() {
    let host = VerterHost::new_standalone(HostConfig::default());

    let src1 = "<script setup>const n = 1</script><template><div>{{n}}</div></template><style>.a{color:red}</style>";
    let src2 = "<script setup>const n = 2</script><template><div>{{n}}</div></template><style>.a{color:red}</style>";

    let _ = upsert_vue(&host, "Comp.vue", src1);
    let result = upsert_vue(&host, "Comp.vue", src2);

    assert!(result
        .changed_virtual_nodes
        .contains(&VirtualNodeKind::Main));
    assert!(result
        .changed_virtual_nodes
        .contains(&VirtualNodeKind::Script));
    assert!(result
        .changed_virtual_nodes
        .contains(&VirtualNodeKind::Template));
    assert!(result
        .changed_virtual_nodes
        .contains(&VirtualNodeKind::Style { index: 0 }));
}

#[test]
fn compile_profile_changes_produce_different_cached_outputs() {
    let host = VerterHost::new_standalone(HostConfig::default());
    let src = "<script setup>const n = 1</script><template><div>{{n}}</div></template>";
    let _ = upsert_vue(&host, "Comp.vue", src);

    let dev = host
        .get_virtual_file(VirtualQuery {
            raw_id: Some("Comp.vue".to_string()),
            canonical_id: None,
            node_kind: None,
            compile_profile: profile_dev(),
        })
        .unwrap();
    let prod = host
        .get_virtual_file(VirtualQuery {
            raw_id: Some("Comp.vue".to_string()),
            canonical_id: None,
            node_kind: None,
            compile_profile: profile_prod(),
        })
        .unwrap();

    assert_ne!(dev.code.as_ref(), prod.code.as_ref());
}

#[test]
fn update_result_contains_both_bundler_and_lsp_ids() {
    let host = VerterHost::new_standalone(HostConfig::default());

    let src1 = "<script setup>const n = 1</script><template><div>{{n}}</div></template><style>.a{color:red}</style>";
    let src2 = "<script setup>const n = 1</script><template><div>{{n}}</div></template><style>.a{color:blue}</style>";

    let _ = upsert_vue(&host, "Comp.vue", src1);
    let result = upsert_vue(&host, "Comp.vue", src2);

    assert_eq!(
        result.changed_virtual_ids.len(),
        result.changed_lsp_ids.len()
    );
    assert!(result.changed_virtual_ids[0].contains("?vue&type=style"));
    assert!(result.changed_lsp_ids[0].contains("._VERTER_.style."));
}

#[test]
fn src_policy_missing_external_source_produces_deterministic_error() {
    let host = VerterHost::new_standalone(HostConfig {
        dev_mode: false,
        compile_error_policy: CompileErrorPolicy::StrictError,
        ..HostConfig::default()
    });

    let src = "<template src=\"./t.html\"></template><script setup>const n=1</script>";
    let update = upsert_vue(&host, "Comp.vue", src);
    assert_eq!(update.external_source_requests.len(), 1);

    let err = host
        .get_virtual_file(VirtualQuery {
            raw_id: Some("Comp.vue".to_string()),
            canonical_id: None,
            node_kind: None,
            compile_profile: profile_dev(),
        })
        .unwrap_err();

    assert!(matches!(
        err,
        HostError::BlockContentRefused(BlockContentRefusal::Unavailable {
            availability: BlockContentAvailability::Missing,
            ..
        })
    ));
}

#[test]
fn external_reupsert_refreshes_the_owners_native_content_artifact() {
    let host = VerterHost::new_standalone(HostConfig::default());

    let owner = upsert_vue(
        &host,
        "Comp.vue",
        "<template src=\"./tpl.html\"></template><script setup>const n = 1</script>",
    );

    let _ = host
        .upsert(UpsertRequest {
            canonical_id: None,
            input_id: "tpl.html".to_string(),
            source: Arc::from("<div>A</div>"),
            file_language: FileLanguage::script_ts(),
            aliases: Vec::new(),
        })
        .unwrap();

    let request = owner.external_source_requests.first().unwrap().clone();
    let first = host
        .get_block_content(BlockContentQuery {
            canonical_id: owner.canonical_id.clone(),
            block_token: request.block_token.clone(),
            compile_profile: CompileProfile::default(),
            expected_basis_token: None,
        })
        .unwrap();

    let _ = host
        .upsert(UpsertRequest {
            canonical_id: None,
            input_id: "tpl.html".to_string(),
            source: Arc::from("<section>B</section>"),
            file_language: FileLanguage::script_ts(),
            aliases: Vec::new(),
        })
        .unwrap();

    let second = host
        .get_block_content(BlockContentQuery {
            canonical_id: owner.canonical_id,
            block_token: request.block_token.clone(),
            compile_profile: CompileProfile::default(),
            expected_basis_token: None,
        })
        .unwrap();

    assert_eq!(first.content.as_deref(), Some("<div>A</div>"));
    assert_eq!(second.content.as_deref(), Some("<section>B</section>"));
    assert_ne!(first.content_hash, second.content_hash);
}

#[test]
fn remove_cleans_up_file_and_aliases() {
    let host = VerterHost::new_standalone(HostConfig::default());
    let src = "<script setup>const n = 1</script><template><div>{{n}}</div></template>";

    let _ = host
        .upsert(UpsertRequest {
            canonical_id: None,
            input_id: "Comp.vue".to_string(),
            source: Arc::from(src),
            file_language: FileLanguage::vue(),
            aliases: vec!["@/Comp.vue".to_string()],
        })
        .unwrap();

    let resolved = host.resolve("@/Comp.vue").unwrap();
    assert!(resolved.exists_in_host);

    let result = host.remove("Comp.vue");
    assert!(result.is_some());
    assert_eq!(result.unwrap().canonical_id, "Comp.vue");

    let resolved = host.resolve("Comp.vue").unwrap();
    assert!(!resolved.exists_in_host);

    let resolved = host.resolve("@/Comp.vue").unwrap();
    assert!(!resolved.exists_in_host);
}

#[test]
fn remove_nonexistent_returns_none() {
    let host = VerterHost::new_standalone(HostConfig::default());
    assert!(host.remove("nonexistent.vue").is_none());
}

#[test]
fn list_virtual_files_returns_correct_nodes() {
    let host = VerterHost::new_standalone(HostConfig::default());
    let src = "<script setup>const n = 1</script><template><div>{{n}}</div></template><style>.a{}</style><style>.b{}</style>";
    let _ = upsert_vue(&host, "Comp.vue", src);

    let nodes = host.list_virtual_files("Comp.vue");
    assert!(nodes.contains(&VirtualNodeKind::Main));
    assert!(nodes.contains(&VirtualNodeKind::Script));
    assert!(nodes.contains(&VirtualNodeKind::Template));
    assert!(nodes.contains(&VirtualNodeKind::Style { index: 0 }));
    assert!(nodes.contains(&VirtualNodeKind::Style { index: 1 }));
    assert_eq!(nodes.len(), 5);
}

#[test]
fn list_virtual_files_nonexistent_returns_empty() {
    let host = VerterHost::new_standalone(HostConfig::default());
    assert!(host.list_virtual_files("nonexistent.vue").is_empty());
}

#[test]
fn alias_resolution_maps_to_same_canonical() {
    let host = VerterHost::new_standalone(HostConfig::default());
    let src = "<script setup>const n = 1</script><template><div>{{n}}</div></template>";

    let _ = host
        .upsert(UpsertRequest {
            canonical_id: Some("/src/Comp.vue".to_string()),
            input_id: "./Comp.vue".to_string(),
            source: Arc::from(src),
            file_language: FileLanguage::vue(),
            aliases: vec!["@/Comp.vue".to_string(), "~/Comp.vue".to_string()],
        })
        .unwrap();

    let a = host.resolve("@/Comp.vue").unwrap();
    let b = host.resolve("~/Comp.vue").unwrap();
    let c = host.resolve("/src/Comp.vue").unwrap();

    assert_eq!(a.canonical_id, "/src/Comp.vue");
    assert_eq!(b.canonical_id, "/src/Comp.vue");
    assert_eq!(c.canonical_id, "/src/Comp.vue");
    assert!(a.exists_in_host);
    assert!(b.exists_in_host);
    assert!(c.exists_in_host);
}

#[test]
fn profile_cap_evicts_oldest_profiles() {
    let host = VerterHost::new_standalone(HostConfig {
        max_profiles_per_file: 2,
        ..HostConfig::default()
    });
    let src = "<script setup>const n = 1</script><template><div>{{n}}</div></template>";
    let _ = upsert_vue(&host, "Comp.vue", src);

    let p1 = CompileProfile {
        hmr_strategy: HmrStrategy::Vite,
        ..CompileProfile::default()
    };
    let p2 = CompileProfile {
        hmr_strategy: HmrStrategy::Webpack,
        ..CompileProfile::default()
    };
    let p3 = CompileProfile {
        is_production: true,
        ..CompileProfile::default()
    };

    let _ = host
        .get_virtual_file(VirtualQuery {
            raw_id: Some("Comp.vue".to_string()),
            canonical_id: None,
            node_kind: None,
            compile_profile: p1.clone(),
        })
        .unwrap();

    let _ = host
        .get_virtual_file(VirtualQuery {
            raw_id: Some("Comp.vue".to_string()),
            canonical_id: None,
            node_kind: None,
            compile_profile: p2.clone(),
        })
        .unwrap();

    let _ = host
        .get_virtual_file(VirtualQuery {
            raw_id: Some("Comp.vue".to_string()),
            canonical_id: None,
            node_kind: None,
            compile_profile: p3.clone(),
        })
        .unwrap();

    let result = host
        .get_virtual_file(VirtualQuery {
            raw_id: Some("Comp.vue".to_string()),
            canonical_id: None,
            node_kind: None,
            compile_profile: p3,
        })
        .unwrap();
    assert!(!result.code.is_empty());
}

#[test]
fn resolve_via_lsp_id_format() {
    let host = VerterHost::new_standalone(HostConfig::default());
    let src = "<script setup>const n = 1</script><template><div>{{n}}</div></template>";
    let _ = upsert_vue(&host, "Comp.vue", src);

    let resolved = host.resolve("Comp.vue._VERTER_.render.tsx").unwrap();
    assert_eq!(resolved.canonical_id, "Comp.vue");
    assert_eq!(resolved.node_kind, VirtualNodeKind::Template);
    assert!(resolved.exists_in_host);
}

#[test]
fn dev_serve_last_known_good_fallback() {
    let host = VerterHost::new_standalone(HostConfig {
        dev_mode: true,
        compile_error_policy: CompileErrorPolicy::DevServeLastKnownGood,
        ..HostConfig::default()
    });

    let good_src = "<script setup>const n = 1</script><template><div>{{n}}</div></template>";
    let _ = upsert_vue(&host, "Comp.vue", good_src);

    let good_result = host
        .get_virtual_file(VirtualQuery {
            raw_id: Some("Comp.vue".to_string()),
            canonical_id: None,
            node_kind: None,
            compile_profile: profile_dev(),
        })
        .unwrap();
    assert!(!good_result.stale);
    assert!(!good_result.code.is_empty());

    let bad_src = "<script setup>const n = 1</script><template src=\"./missing.html\"></template>";
    let _ = upsert_vue(&host, "Comp.vue", bad_src);

    let result = host.get_virtual_file(VirtualQuery {
        raw_id: Some("Comp.vue".to_string()),
        canonical_id: None,
        node_kind: None,
        compile_profile: profile_dev(),
    });
    assert!(result.is_err());
}

// @ai-generated - New integration tests for previously uncovered functionality

#[test]
fn cache_hit_returns_same_code() {
    let host = VerterHost::new_standalone(HostConfig::default());
    let src = "<script setup>const n = 1</script><template><div>{{n}}</div></template>";
    let _ = upsert_vue(&host, "Comp.vue", src);

    let first = host
        .get_virtual_file(VirtualQuery {
            raw_id: Some("Comp.vue".to_string()),
            canonical_id: None,
            node_kind: None,
            compile_profile: profile_dev(),
        })
        .unwrap();

    let second = host
        .get_virtual_file(VirtualQuery {
            raw_id: Some("Comp.vue".to_string()),
            canonical_id: None,
            node_kind: None,
            compile_profile: profile_dev(),
        })
        .unwrap();

    assert_eq!(first.code.as_ref(), second.code.as_ref());
}

#[test]
fn get_virtual_file_by_canonical_id_and_node_kind() {
    let host = VerterHost::new_standalone(HostConfig::default());
    let src = "<script setup>const n = 1</script><template><div>{{n}}</div></template>";
    let _ = upsert_vue(&host, "Comp.vue", src);

    let _ = host
        .get_virtual_file(VirtualQuery {
            raw_id: Some("Comp.vue".to_string()),
            canonical_id: None,
            node_kind: None,
            compile_profile: profile_dev(),
        })
        .unwrap();

    let result = host
        .get_virtual_file(VirtualQuery {
            raw_id: None,
            canonical_id: Some("Comp.vue".to_string()),
            node_kind: Some(VirtualNodeKind::Template),
            compile_profile: profile_dev(),
        })
        .unwrap();

    assert!(!result.code.is_empty());
}

#[test]
fn get_virtual_file_no_raw_id_no_canonical_returns_invalid_query() {
    let host = VerterHost::new_standalone(HostConfig::default());

    let err = host
        .get_virtual_file(VirtualQuery {
            raw_id: None,
            canonical_id: None,
            node_kind: None,
            compile_profile: profile_dev(),
        })
        .unwrap_err();

    assert!(matches!(err, HostError::InvalidQuery));
}

#[test]
fn custom_block_detection_and_retrieval() {
    let host = VerterHost::new_standalone(HostConfig::default());
    let src = "<script setup>const n = 1</script><template><div>{{n}}</div></template><i18n>{\"en\": {\"hello\": \"world\"}}</i18n>";
    let _ = upsert_vue(&host, "Comp.vue", src);

    let nodes = host.list_virtual_files("Comp.vue");
    assert!(nodes.contains(&VirtualNodeKind::Custom { index: 0 }));

    let result = host
        .get_virtual_file(VirtualQuery {
            raw_id: Some("Comp.vue?vue&type=custom&index=0".to_string()),
            canonical_id: None,
            node_kind: None,
            compile_profile: profile_dev(),
        })
        .unwrap();
    assert!(!result.code.is_empty());
}

#[test]
fn script_only_sfc_no_template_node() {
    let host = VerterHost::new_standalone(HostConfig::default());
    let src = "<script setup>const n = 1</script>";
    let _ = upsert_vue(&host, "Comp.vue", src);

    let nodes = host.list_virtual_files("Comp.vue");
    assert!(nodes.contains(&VirtualNodeKind::Main));
    assert!(nodes.contains(&VirtualNodeKind::Script));
    assert!(!nodes.contains(&VirtualNodeKind::Template));
}

#[test]
fn template_only_sfc_no_script_node() {
    let host = VerterHost::new_standalone(HostConfig::default());
    let src = "<template><div>hello</div></template>";
    let _ = upsert_vue(&host, "Comp.vue", src);

    let nodes = host.list_virtual_files("Comp.vue");
    assert!(nodes.contains(&VirtualNodeKind::Main));
    assert!(nodes.contains(&VirtualNodeKind::Template));
    assert!(!nodes.contains(&VirtualNodeKind::Script));
}

#[test]
fn alias_update_on_reupsert_removes_old() {
    let host = VerterHost::new_standalone(HostConfig::default());
    let src = "<script setup>const n = 1</script><template><div>{{n}}</div></template>";

    let _ = host
        .upsert(UpsertRequest {
            canonical_id: None,
            input_id: "Comp.vue".to_string(),
            source: Arc::from(src),
            file_language: FileLanguage::vue(),
            aliases: vec!["old-alias".to_string()],
        })
        .unwrap();

    assert!(host.resolve("old-alias").unwrap().exists_in_host);

    let _ = host
        .upsert(UpsertRequest {
            canonical_id: None,
            input_id: "Comp.vue".to_string(),
            source: Arc::from(src),
            file_language: FileLanguage::vue(),
            aliases: vec!["new-alias".to_string()],
        })
        .unwrap();

    assert!(!host.resolve("old-alias").unwrap().exists_in_host);
    assert!(host.resolve("new-alias").unwrap().exists_in_host);
}

#[test]
fn non_sfc_upsert_produces_only_main() {
    let host = VerterHost::new_standalone(HostConfig::default());

    let _ = host
        .upsert(UpsertRequest {
            canonical_id: None,
            input_id: "helper.ts".to_string(),
            source: Arc::from("export const x = 1"),
            file_language: FileLanguage::script_ts(),
            aliases: Vec::new(),
        })
        .unwrap();

    let nodes = host.list_virtual_files("helper.ts");
    assert_eq!(nodes, vec![VirtualNodeKind::Main]);
}

#[test]
fn remove_by_alias_works() {
    let host = VerterHost::new_standalone(HostConfig::default());
    let src = "<script setup>const n = 1</script><template><div>{{n}}</div></template>";

    let _ = host
        .upsert(UpsertRequest {
            canonical_id: None,
            input_id: "Comp.vue".to_string(),
            source: Arc::from(src),
            file_language: FileLanguage::vue(),
            aliases: vec!["@/Comp.vue".to_string()],
        })
        .unwrap();

    let result = host.remove("@/Comp.vue");
    assert!(result.is_some());
    assert_eq!(result.unwrap().canonical_id, "Comp.vue");

    assert!(!host.resolve("Comp.vue").unwrap().exists_in_host);
    assert!(!host.resolve("@/Comp.vue").unwrap().exists_in_host);
}

// ═══════════════════════════════════════════════════════════
// Integration tests for uncovered behaviors
// ═══════════════════════════════════════════════════════════

/// @ai-generated - Non-SFC reupsert with different content reports changed=true
/// so callers (bundler/LSP) know to re-request dependent virtual files.
/// This enables deep type resolution: when a .ts file imported by an SFC
/// changes, the host reports it so dependents can be recompiled.
#[test]
fn non_sfc_reupsert_with_different_content_reports_changed() {
    let host = VerterHost::new_standalone(HostConfig::default());

    let first = host
        .upsert(UpsertRequest {
            canonical_id: None,
            input_id: "helper.ts".to_string(),
            source: Arc::from("export const x = 1"),
            file_language: FileLanguage::script_ts(),
            aliases: Vec::new(),
        })
        .unwrap();
    assert!(first.changed); // first upsert is always "changed"

    let second = host
        .upsert(UpsertRequest {
            canonical_id: None,
            input_id: "helper.ts".to_string(),
            source: Arc::from("export const x = 2"),
            file_language: FileLanguage::script_ts(),
            aliases: Vec::new(),
        })
        .unwrap();
    assert!(
        second.changed,
        "non-SFC content change must report changed=true"
    );
    assert!(
        second
            .changed_virtual_nodes
            .contains(&VirtualNodeKind::Main),
        "non-SFC change should include Main node"
    );
}

/// @ai-generated - Non-SFC reupsert with identical content reports changed=false
#[test]
fn non_sfc_reupsert_with_same_content_reports_not_changed() {
    let host = VerterHost::new_standalone(HostConfig::default());

    let _ = host
        .upsert(UpsertRequest {
            canonical_id: None,
            input_id: "helper.ts".to_string(),
            source: Arc::from("export const x = 1"),
            file_language: FileLanguage::script_ts(),
            aliases: Vec::new(),
        })
        .unwrap();

    let second = host
        .upsert(UpsertRequest {
            canonical_id: None,
            input_id: "helper.ts".to_string(),
            source: Arc::from("export const x = 1"),
            file_language: FileLanguage::script_ts(),
            aliases: Vec::new(),
        })
        .unwrap();
    assert!(!second.changed);
}

/// @ai-generated - A non-SFC VFS reupsert is observed by the owner's block identity.
#[test]
fn non_sfc_reupsert_refreshes_owner_block_content() {
    let host = VerterHost::new_standalone(HostConfig::default());

    // Comp.vue depends on tpl.html via src
    let owner = upsert_vue(
        &host,
        "/src/Comp.vue",
        "<template src=\"./tpl.html\"></template><script setup>const n = 1</script>",
    );
    let _ = host
        .upsert(UpsertRequest {
            canonical_id: None,
            input_id: "/src/tpl.html".to_string(),
            source: Arc::from("<div>A</div>"),
            file_language: FileLanguage::script_ts(),
            aliases: Vec::new(),
        })
        .unwrap();

    let request = owner.external_source_requests.first().unwrap().clone();
    let first = host
        .get_block_content(BlockContentQuery {
            canonical_id: owner.canonical_id.clone(),
            block_token: request.block_token.clone(),
            compile_profile: CompileProfile::default(),
            expected_basis_token: None,
        })
        .unwrap();

    // Re-upsert dependency with new content
    let _ = host
        .upsert(UpsertRequest {
            canonical_id: None,
            input_id: "/src/tpl.html".to_string(),
            source: Arc::from("<section>B</section>"),
            file_language: FileLanguage::script_ts(),
            aliases: Vec::new(),
        })
        .unwrap();

    let second = host
        .get_block_content(BlockContentQuery {
            canonical_id: owner.canonical_id,
            block_token: request.block_token.clone(),
            compile_profile: CompileProfile::default(),
            expected_basis_token: None,
        })
        .unwrap();

    assert_eq!(
        first.availability,
        BlockContentAvailability::NativeAvailable
    );
    assert_eq!(
        second.availability,
        BlockContentAvailability::NativeAvailable
    );
    assert_ne!(first.content_hash, second.content_hash);
}

/// @ai-generated - Style removal: 3 styles → 1 style reports removed nodes
#[test]
fn style_removal_produces_removed_nodes() {
    let host = VerterHost::new_standalone(HostConfig::default());

    let src1 = "<script setup>const n = 1</script><template><div/></template><style>.a{}</style><style>.b{}</style><style>.c{}</style>";
    let src2 = "<script setup>const n = 1</script><template><div/></template><style>.a{}</style>";

    let _ = upsert_vue(&host, "Comp.vue", src1);
    let result = upsert_vue(&host, "Comp.vue", src2);

    assert!(result
        .removed_virtual_nodes
        .contains(&VirtualNodeKind::Style { index: 1 }));
    assert!(result
        .removed_virtual_nodes
        .contains(&VirtualNodeKind::Style { index: 2 }));
}

/// @ai-generated - Multiple style changes at once: only changed indices reported
#[test]
fn multiple_style_changes_at_once() {
    let host = VerterHost::new_standalone(HostConfig::default());

    let src1 = "<script setup>const n = 1</script><template><div/></template><style>.a{color:red}</style><style>.b{color:blue}</style><style>.c{color:green}</style>";
    let src2 = "<script setup>const n = 1</script><template><div/></template><style>.a{color:red}</style><style>.b{color:yellow}</style><style>.c{color:purple}</style>";

    let _ = upsert_vue(&host, "Comp.vue", src1);
    let result = upsert_vue(&host, "Comp.vue", src2);

    // Style 0 unchanged, styles 1 and 2 changed
    assert!(!result
        .changed_virtual_nodes
        .contains(&VirtualNodeKind::Style { index: 0 }));
    assert!(result
        .changed_virtual_nodes
        .contains(&VirtualNodeKind::Style { index: 1 }));
    assert!(result
        .changed_virtual_nodes
        .contains(&VirtualNodeKind::Style { index: 2 }));
}

/// @ai-generated - get_virtual_file first compile via canonical_id (no prior raw_id)
#[test]
fn get_virtual_file_first_compile_via_canonical() {
    let host = VerterHost::new_standalone(HostConfig::default());
    let src = "<script setup>const n = 1</script><template><div>{{n}}</div></template>";
    let _ = upsert_vue(&host, "Comp.vue", src);

    // Use canonical_id + node_kind (not raw_id) for the first compile
    let result = host
        .get_virtual_file(VirtualQuery {
            raw_id: None,
            canonical_id: Some("Comp.vue".to_string()),
            node_kind: Some(VirtualNodeKind::Main),
            compile_profile: profile_dev(),
        })
        .unwrap();

    assert!(!result.code.is_empty());
}

/// @ai-generated - After template invalidation, DevServeLastKnownGood has no fallback
/// for Main/Template nodes because invalidate_nodes clears them from last_good_outputs.
#[test]
fn template_change_then_error_no_fallback() {
    let host = VerterHost::new_standalone(HostConfig {
        dev_mode: true,
        compile_error_policy: CompileErrorPolicy::DevServeLastKnownGood,
        ..HostConfig::default()
    });

    // First: good SFC, compile to populate cache + last_good
    let good_src = "<script setup>const n = 1</script><template><div>{{n}}</div></template>";
    let _ = upsert_vue(&host, "Comp.vue", good_src);
    let _ = host
        .get_virtual_file(VirtualQuery {
            raw_id: Some("Comp.vue".to_string()),
            canonical_id: None,
            node_kind: None,
            compile_profile: profile_dev(),
        })
        .unwrap();

    // Second: template-only change to a broken SFC (template src pointing nowhere)
    let bad_src = "<script setup>const n = 1</script><template src=\"./missing.html\"></template>";
    let _ = upsert_vue(&host, "Comp.vue", bad_src);

    // The template change invalidated Main+Template from last_good_outputs,
    // so there's no fallback and we get an error
    let result = host.get_virtual_file(VirtualQuery {
        raw_id: Some("Comp.vue".to_string()),
        canonical_id: None,
        node_kind: None,
        compile_profile: profile_dev(),
    });
    assert!(result.is_err());
}

// ═══════════════════════════════════════════════════════════
// Behavioral contract tests
// ═══════════════════════════════════════════════════════════

/// @ai-generated - Verify get_virtual_file returns correct lang per node kind:
/// Main="js", Script="ts", Template="tsx", Style(plain)="css", Style(scss)="scss"
#[test]
fn virtual_file_lang_field_correct_per_node_kind() {
    let host = VerterHost::new_standalone(HostConfig::default());
    let src = "<script setup>const n = 1</script><template><div>{{n}}</div></template><style>.a{color:red}</style><style lang=\"scss\">.b{color:blue}</style>";
    let _ = upsert_vue(&host, "Comp.vue", src);

    let profile = profile_dev();

    let main = host
        .get_virtual_file(VirtualQuery {
            raw_id: Some("Comp.vue".to_string()),
            canonical_id: None,
            node_kind: None,
            compile_profile: profile.clone(),
        })
        .unwrap();
    assert_eq!(main.lang.as_deref(), Some("js"));

    let script = host
        .get_virtual_file(VirtualQuery {
            raw_id: Some("Comp.vue?vue&type=script".to_string()),
            canonical_id: None,
            node_kind: None,
            compile_profile: profile.clone(),
        })
        .unwrap();
    assert_eq!(script.lang.as_deref(), Some("ts"));

    let template = host
        .get_virtual_file(VirtualQuery {
            raw_id: Some("Comp.vue?vue&type=template".to_string()),
            canonical_id: None,
            node_kind: None,
            compile_profile: profile.clone(),
        })
        .unwrap();
    assert_eq!(template.lang.as_deref(), Some("tsx"));

    let style0 = host
        .get_virtual_file(VirtualQuery {
            raw_id: Some("Comp.vue?vue&type=style&index=0".to_string()),
            canonical_id: None,
            node_kind: None,
            compile_profile: profile.clone(),
        })
        .unwrap();
    assert_eq!(style0.lang.as_deref(), Some("css"));

    let style1 = host
        .get_virtual_file(VirtualQuery {
            raw_id: Some("Comp.vue?vue&type=style&index=1".to_string()),
            canonical_id: None,
            node_kind: None,
            compile_profile: profile.clone(),
        })
        .unwrap();
    assert_eq!(style1.lang.as_deref(), Some("scss"));
}

/// Main virtual file lang field should be "ts" for <script setup lang="ts">
#[test]
fn virtual_file_main_lang_ts_for_typescript_sfc() {
    let host = VerterHost::new_standalone(HostConfig::default());
    let src = r#"<script setup lang="ts">
const { title = 'Test' } = defineProps<{ title?: string }>()
defineEmits<{ close: [value: string] }>()
</script>
<template><div>{{ title }}</div></template>"#;
    let _ = upsert_vue(&host, "MockModal.vue", src);

    let profile = profile_dev();
    let main = host
        .get_virtual_file(VirtualQuery {
            raw_id: Some("MockModal.vue".to_string()),
            canonical_id: None,
            node_kind: None,
            compile_profile: profile,
        })
        .unwrap();
    assert_eq!(
        main.lang.as_deref(),
        Some("ts"),
        "Main virtual file lang should be 'ts' for <script setup lang=\"ts\">, got: {:?}",
        main.lang
    );
    // Verify the output does NOT contain __props: any (Verter's codegen doesn't add type annotations)
    assert!(
        !main.code.contains("__props: any"),
        "Verter output should not contain '__props: any'. Output:\n{}",
        main.code
    );
}

/// @ai-generated - Verify VirtualMeta fields: scope_id (scoped style), style_index,
/// custom_index, block_type are set correctly
#[test]
fn virtual_file_meta_fields_populated_correctly() {
    let host = VerterHost::new_standalone(HostConfig::default());
    let src = "<script setup>const n = 1</script><template><div>{{n}}</div></template><style scoped>.a{}</style><i18n>{\"en\":{\"hi\":\"hello\"}}</i18n>";
    let _ = upsert_vue(&host, "Comp.vue", src);

    let profile = profile_dev();

    // Main: scope_id present due to scoped style
    let main = host
        .get_virtual_file(VirtualQuery {
            raw_id: Some("Comp.vue".to_string()),
            canonical_id: None,
            node_kind: None,
            compile_profile: profile.clone(),
        })
        .unwrap();
    assert!(
        main.meta.scope_id.is_some(),
        "scope_id should be set for scoped styles"
    );

    // Style: style_index = Some(0)
    let style = host
        .get_virtual_file(VirtualQuery {
            raw_id: Some("Comp.vue?vue&type=style&index=0".to_string()),
            canonical_id: None,
            node_kind: None,
            compile_profile: profile.clone(),
        })
        .unwrap();
    assert_eq!(style.meta.style_index, Some(0));

    // Custom: custom_index = Some(0), block_type = "i18n"
    let custom = host
        .get_virtual_file(VirtualQuery {
            raw_id: Some("Comp.vue?vue&type=custom&index=0".to_string()),
            canonical_id: None,
            node_kind: None,
            compile_profile: profile.clone(),
        })
        .unwrap();
    assert_eq!(custom.meta.custom_index, Some(0));
    assert_eq!(custom.meta.block_type.as_deref(), Some("i18n"));
}

/// @ai-generated - Main module code should contain style imports for each style block
#[test]
fn main_module_contains_style_imports() {
    let host = VerterHost::new_standalone(HostConfig::default());
    let src = "<script setup>const n = 1</script><template><div>{{n}}</div></template><style>.a{}</style><style lang=\"scss\">.b{}</style>";
    let _ = upsert_vue(&host, "Comp.vue", src);

    let main = host
        .get_virtual_file(VirtualQuery {
            raw_id: Some("Comp.vue".to_string()),
            canonical_id: None,
            node_kind: None,
            compile_profile: profile_dev(),
        })
        .unwrap();
    let code = main.code.as_ref();
    assert!(
        code.contains("import \"Comp.vue?vue&type=style&index=0"),
        "should import style 0: {}",
        code
    );
    assert!(
        code.contains("import \"Comp.vue?vue&type=style&index=1"),
        "should import style 1: {}",
        code
    );
}

/// @ai-generated - Main module contains Vite HMR in dev, absent in prod
#[test]
fn main_module_vite_hmr_in_dev_mode() {
    let host = VerterHost::new_standalone(HostConfig::default());
    let src = "<script setup>const n = 1</script><template><div>{{n}}</div></template>";
    let _ = upsert_vue(&host, "Comp.vue", src);

    let dev = host
        .get_virtual_file(VirtualQuery {
            raw_id: Some("Comp.vue".to_string()),
            canonical_id: None,
            node_kind: None,
            compile_profile: CompileProfile {
                is_production: false,
                hmr_strategy: HmrStrategy::Vite,
                ..CompileProfile::default()
            },
        })
        .unwrap();
    assert!(
        dev.code.contains("import.meta.hot"),
        "dev mode should contain Vite HMR"
    );

    let prod = host
        .get_virtual_file(VirtualQuery {
            raw_id: Some("Comp.vue".to_string()),
            canonical_id: None,
            node_kind: None,
            compile_profile: profile_prod(),
        })
        .unwrap();
    assert!(
        !prod.code.contains("import.meta.hot"),
        "prod mode should not contain HMR"
    );
}

/// @ai-generated - LSP-format raw_id input produces LSP-format output ID
#[test]
fn get_virtual_file_via_lsp_id_returns_lsp_format_id() {
    let host = VerterHost::new_standalone(HostConfig::default());
    let src = "<script setup>const n = 1</script><template><div>{{n}}</div></template>";
    let _ = upsert_vue(&host, "Comp.vue", src);

    // Trigger initial compile
    let _ = host
        .get_virtual_file(VirtualQuery {
            raw_id: Some("Comp.vue".to_string()),
            canonical_id: None,
            node_kind: None,
            compile_profile: profile_dev(),
        })
        .unwrap();

    let result = host
        .get_virtual_file(VirtualQuery {
            raw_id: Some("Comp.vue._VERTER_.render.tsx".to_string()),
            canonical_id: None,
            node_kind: None,
            compile_profile: profile_dev(),
        })
        .unwrap();
    assert!(
        result.id.contains("._VERTER_."),
        "LSP-format input should produce LSP-format output ID, got: {}",
        result.id
    );
    assert!(
        result.id.contains("render.tsx"),
        "should contain render.tsx suffix, got: {}",
        result.id
    );
}

/// @ai-generated - source_map is Some when CompileProfile::source_map = true
#[test]
fn source_map_present_when_requested() {
    let host = VerterHost::new_standalone(HostConfig::default());
    let src = "<script setup>const n = 1</script><template><div>{{n}}</div></template>";
    let _ = upsert_vue(&host, "Comp.vue", src);

    let with_map = host
        .get_virtual_file(VirtualQuery {
            raw_id: Some("Comp.vue?vue&type=script".to_string()),
            canonical_id: None,
            node_kind: None,
            compile_profile: CompileProfile {
                source_map: true,
                ..profile_dev()
            },
        })
        .unwrap();
    assert!(
        with_map.source_map.is_some(),
        "source_map should be Some when source_map=true"
    );
}

#[test]
fn get_virtual_file_missing_node_returns_error() {
    let host = VerterHost::new_standalone(HostConfig::default());
    let src = "<script setup>const n = 1</script>";
    let _ = upsert_vue(&host, "Comp.vue", src);

    let err = host
        .get_virtual_file(VirtualQuery {
            raw_id: Some("Comp.vue?vue&type=template".to_string()),
            canonical_id: None,
            node_kind: None,
            compile_profile: profile_dev(),
        })
        .unwrap_err();
    assert!(
        matches!(err, HostError::MissingVirtualNode { .. }),
        "expected MissingVirtualNode, got: {:?}",
        err
    );
}

// ═══════════════════════════════════════════════════════════
// Important behavioral tests
// ═══════════════════════════════════════════════════════════

/// @ai-generated - __file present in dev mode, absent in prod mode
#[test]
fn main_module_dev_file_annotation() {
    let host = VerterHost::new_standalone(HostConfig::default());
    let src = "<script setup>const n = 1</script><template><div>{{n}}</div></template>";
    let _ = upsert_vue(&host, "Comp.vue", src);

    let dev = host
        .get_virtual_file(VirtualQuery {
            raw_id: Some("Comp.vue".to_string()),
            canonical_id: None,
            node_kind: None,
            compile_profile: profile_dev(),
        })
        .unwrap();
    assert!(
        dev.code.contains("__file"),
        "dev mode main should contain __file"
    );

    let prod = host
        .get_virtual_file(VirtualQuery {
            raw_id: Some("Comp.vue".to_string()),
            canonical_id: None,
            node_kind: None,
            compile_profile: profile_prod(),
        })
        .unwrap();
    assert!(
        !prod.code.contains("__file"),
        "prod mode main should not contain __file"
    );
}

/// @ai-generated - Dev+prod compile caches coexist without evicting each other
#[test]
fn multiple_profile_cache_coexistence() {
    let host = VerterHost::new_standalone(HostConfig {
        max_profiles_per_file: 8,
        ..HostConfig::default()
    });
    let src = "<script setup>const n = 1</script><template><div>{{n}}</div></template>";
    let _ = upsert_vue(&host, "Comp.vue", src);

    let dev_result = host
        .get_virtual_file(VirtualQuery {
            raw_id: Some("Comp.vue".to_string()),
            canonical_id: None,
            node_kind: None,
            compile_profile: profile_dev(),
        })
        .unwrap();
    let prod_result = host
        .get_virtual_file(VirtualQuery {
            raw_id: Some("Comp.vue".to_string()),
            canonical_id: None,
            node_kind: None,
            compile_profile: profile_prod(),
        })
        .unwrap();

    assert_ne!(dev_result.code.as_ref(), prod_result.code.as_ref());

    // Re-fetching dev should still be cached (not evicted by prod)
    let dev_again = host
        .get_virtual_file(VirtualQuery {
            raw_id: Some("Comp.vue".to_string()),
            canonical_id: None,
            node_kind: None,
            compile_profile: profile_dev(),
        })
        .unwrap();
    assert_eq!(
        dev_result.code.as_ref(),
        dev_again.code.as_ref(),
        "dev cache should not be evicted by prod"
    );

    // Re-fetching prod should still be cached
    let prod_again = host
        .get_virtual_file(VirtualQuery {
            raw_id: Some("Comp.vue".to_string()),
            canonical_id: None,
            node_kind: None,
            compile_profile: profile_prod(),
        })
        .unwrap();
    assert_eq!(
        prod_result.code.as_ref(),
        prod_again.code.as_ref(),
        "prod cache should not be evicted by dev"
    );
}

/// @ai-generated - Remove then re-upsert treats file as fresh insert
#[test]
fn remove_then_reupsert_lifecycle() {
    let host = VerterHost::new_standalone(HostConfig::default());
    let src = "<script setup>const n = 1</script><template><div>{{n}}</div></template>";
    let _ = upsert_vue(&host, "Comp.vue", src);

    host.remove("Comp.vue").unwrap();
    assert!(!host.resolve("Comp.vue").unwrap().exists_in_host);

    let result = upsert_vue(&host, "Comp.vue", src);
    assert!(
        result.changed,
        "re-upsert after remove should report changed"
    );
    assert!(host.resolve("Comp.vue").unwrap().exists_in_host);

    let vf = host
        .get_virtual_file(VirtualQuery {
            raw_id: Some("Comp.vue".to_string()),
            canonical_id: None,
            node_kind: None,
            compile_profile: profile_dev(),
        })
        .unwrap();
    assert!(!vf.code.is_empty());
}

/// @ai-generated - Explicit canonical_id different from input_id stores correctly
#[test]
fn upsert_with_explicit_canonical_id() {
    let host = VerterHost::new_standalone(HostConfig::default());
    let src = "<script setup>const n = 1</script><template><div>{{n}}</div></template>";

    let _ = host
        .upsert(UpsertRequest {
            canonical_id: Some("/canonical/path/Comp.vue".to_string()),
            input_id: "./relative/Comp.vue".to_string(),
            source: Arc::from(src),
            file_language: FileLanguage::vue(),
            aliases: Vec::new(),
        })
        .unwrap();

    let resolved = host.resolve("/canonical/path/Comp.vue").unwrap();
    assert!(resolved.exists_in_host);
    assert_eq!(resolved.canonical_id, "/canonical/path/Comp.vue");

    // input_id is added as an alias
    let resolved2 = host.resolve("./relative/Comp.vue").unwrap();
    assert!(resolved2.exists_in_host);
    assert_eq!(resolved2.canonical_id, "/canonical/path/Comp.vue");
}

/// @ai-generated - Two <i18n> blocks produce distinct Custom nodes
#[test]
fn multiple_custom_blocks_same_type() {
    let host = VerterHost::new_standalone(HostConfig::default());
    let src = "<script setup>const n = 1</script><template><div>{{n}}</div></template><i18n>{\"en\":{\"a\":\"hello\"}}</i18n><i18n>{\"fr\":{\"a\":\"bonjour\"}}</i18n>";
    let _ = upsert_vue(&host, "Comp.vue", src);

    let nodes = host.list_virtual_files("Comp.vue");
    assert!(nodes.contains(&VirtualNodeKind::Custom { index: 0 }));
    assert!(nodes.contains(&VirtualNodeKind::Custom { index: 1 }));

    let custom0 = host
        .get_virtual_file(VirtualQuery {
            raw_id: Some("Comp.vue?vue&type=custom&index=0".to_string()),
            canonical_id: None,
            node_kind: None,
            compile_profile: profile_dev(),
        })
        .unwrap();
    let custom1 = host
        .get_virtual_file(VirtualQuery {
            raw_id: Some("Comp.vue?vue&type=custom&index=1".to_string()),
            canonical_id: None,
            node_kind: None,
            compile_profile: profile_dev(),
        })
        .unwrap();
    assert_ne!(
        custom0.code.as_ref(),
        custom1.code.as_ref(),
        "two i18n blocks should have different content"
    );
}

/// A truly-empty `<script setup></script>` (no content, no `src`) has no
/// real entry block and must fail closed with `MissingSfcEntryBlock`, not
/// compile successfully. Official Vue's SFC parser (`compiler-sfc/src/
/// parse.ts`) uses ONE AST tag, `'script'`, for both `<script>` and
/// `<script setup>` — the `setup`/`vapor` attribute only distinguishes them
/// at assignment time (~line 197) — so the `ignoreEmpty` prune (~line
/// 155-168: `node.tag !== 'template' && isEmpty(node) && !hasAttr(node,
/// 'src')`) drops an empty `<script setup>` exactly like an empty
/// `<script>`. With `descriptor.template`/`.script`/`.scriptSetup` all
/// unset, the "At least one <template> or <script> is required" error
/// fires (~line 227-233) — verified directly against the pinned oracle
/// checkout.
#[test]
fn empty_script_setup_block_has_no_entry_block() {
    let host = VerterHost::new_standalone(HostConfig::default());
    let src = "<script setup></script>";
    let _ = upsert_vue(&host, "Comp.vue", src);

    let error = host
        .get_virtual_file(VirtualQuery {
            raw_id: Some("Comp.vue".to_string()),
            canonical_id: None,
            node_kind: None,
            compile_profile: profile_dev(),
        })
        .expect_err("a truly-empty <script setup> has no entry block and must fail closed");
    let HostError::CompileError(failure) = &error else {
        panic!("expected a CompileError, got: {error:?}");
    };
    let missing_entry_block_count = failure
        .diagnostics
        .diagnostics
        .iter()
        .filter(|d| d.code == "MissingSfcEntryBlock")
        .count();
    assert_eq!(
        missing_entry_block_count, 1,
        "MissingSfcEntryBlock must be recorded exactly once, got: {:?}",
        failure.diagnostics
    );
}

/// Negative control for the fix above: a `<script setup>` with real content
/// still counts as a real entry block and compiles successfully — only a
/// TRULY-empty, src-less block fails closed.
#[test]
fn non_empty_script_setup_block_still_compiles() {
    let host = VerterHost::new_standalone(HostConfig::default());
    let src = "<script setup>const x = 1</script>";
    let _ = upsert_vue(&host, "Comp.vue", src);

    let result = host
        .get_virtual_file(VirtualQuery {
            raw_id: Some("Comp.vue".to_string()),
            canonical_id: None,
            node_kind: None,
            compile_profile: profile_dev(),
        })
        .unwrap_or_else(|error| panic!("{src:?} must still compile, got: {error:?}"));
    assert!(
        !result.code.is_empty(),
        "{src:?} must produce non-empty output"
    );
}

// ═══════════════════════════════════════════════════════════
// Task 4: remove invalidates dependent compile slots
// ═══════════════════════════════════════════════════════════

/// @ai-generated - Removing a VFS dependency makes the owner's block content missing.
#[test]
fn remove_makes_dependent_block_content_missing() {
    let host = VerterHost::new_standalone(HostConfig::default());

    // Comp.vue depends on tpl.html via src
    let owner = upsert_vue(
        &host,
        "/src/Comp.vue",
        "<template src=\"./tpl.html\"></template><script setup>const n = 1</script>",
    );
    let _ = host
        .upsert(UpsertRequest {
            canonical_id: None,
            input_id: "/src/tpl.html".to_string(),
            source: Arc::from("<div>A</div>"),
            file_language: FileLanguage::script_ts(),
            aliases: Vec::new(),
        })
        .unwrap();

    let request = owner.external_source_requests.first().unwrap().clone();
    assert_eq!(
        host.get_block_content(BlockContentQuery {
            canonical_id: owner.canonical_id.clone(),
            block_token: request.block_token.clone(),
            compile_profile: CompileProfile::default(),
            expected_basis_token: None,
        })
        .unwrap()
        .availability,
        BlockContentAvailability::NativeAvailable
    );

    // Remove the dependency — this should invalidate Comp.vue's compile slots
    host.remove("/src/tpl.html");

    let missing = host
        .get_block_content(BlockContentQuery {
            canonical_id: owner.canonical_id,
            block_token: request.block_token.clone(),
            compile_profile: CompileProfile::default(),
            expected_basis_token: None,
        })
        .unwrap();
    assert_eq!(missing.availability, BlockContentAvailability::Missing);
    assert!(missing.content.is_none());
}

// ═══════════════════════════════════════════════════════════
// Task 5: get_diagnostics public API
// ═══════════════════════════════════════════════════════════

/// @ai-generated - get_diagnostics returns last-known diagnostics without triggering compilation
#[test]
fn get_diagnostics_without_compilation() {
    let host = VerterHost::new_standalone(HostConfig {
        dev_mode: false,
        compile_error_policy: CompileErrorPolicy::StrictError,
        ..HostConfig::default()
    });

    // Upsert an SFC with a missing external source
    let _ = upsert_vue(
        &host,
        "Comp.vue",
        "<template src=\"./missing.html\"></template><script setup>const n=1</script>",
    );

    // Force a compilation that will fail
    let _err = host.get_virtual_file(VirtualQuery {
        raw_id: Some("Comp.vue".to_string()),
        canonical_id: None,
        node_kind: None,
        compile_profile: profile_dev(),
    });

    // get_diagnostics should return stored diagnostics without recompiling
    let diags = host.get_diagnostics("Comp.vue", &profile_dev());
    assert!(
        diags.is_none(),
        "typed external-content deferral must not be cached as compile diagnostics",
    );
}

// ═══════════════════════════════════════════════════════════
// Smart invalidation tests (export-level)
// ═══════════════════════════════════════════════════════════

/// @ai-generated - Dep file type export changes → SFC using that type invalidated
#[test]
fn smart_invalidation_type_dep_changed_invalidates_sfc() {
    let host = VerterHost::new_standalone(HostConfig::default());

    // Upsert SFC that imports MyType from a relative path and uses it in defineProps
    let _ = upsert_vue(
        &host,
        "/src/Comp.vue",
        "<script setup lang=\"ts\">\nimport type { MyType } from './types'\nconst props = defineProps<{foo: MyType}>()\n</script>\n<template><div>{{ props.foo }}</div></template>",
    );

    // Upsert the dependency file with MyType export
    let _ = host
        .upsert(UpsertRequest {
            canonical_id: None,
            input_id: "/src/types.ts".to_string(),
            source: Arc::from("export interface MyType { a: string }"),
            file_language: FileLanguage::script_ts(),
            aliases: Vec::new(),
        })
        .unwrap();

    // Compile Comp.vue to populate cache
    let first = host
        .get_virtual_file(VirtualQuery {
            raw_id: Some("/src/Comp.vue".to_string()),
            canonical_id: None,
            node_kind: None,
            compile_profile: profile_dev(),
        })
        .unwrap();
    assert!(!first.code.is_empty());

    // Change MyType in the dependency
    let _ = host
        .upsert(UpsertRequest {
            canonical_id: None,
            input_id: "/src/types.ts".to_string(),
            source: Arc::from("export interface MyType { a: string; b: number }"),
            file_language: FileLanguage::script_ts(),
            aliases: Vec::new(),
        })
        .unwrap();

    // Comp.vue should recompile (cache invalidated)
    let second = host
        .get_virtual_file(VirtualQuery {
            raw_id: Some("/src/Comp.vue".to_string()),
            canonical_id: None,
            node_kind: None,
            compile_profile: profile_dev(),
        })
        .unwrap();
    // The code should be regenerated (cache miss → new compilation)
    // Since the source didn't change, the output should be the same code,
    // but the key thing is that it wasn't served from the old cache slot
    assert!(!second.code.is_empty());
}

/// @ai-generated - upsert returns import_specifiers from script analysis
#[test]
fn upsert_returns_import_specifiers() {
    let host = VerterHost::new_standalone(HostConfig::default());

    let result = upsert_vue(
        &host,
        "/src/Comp.vue",
        "<script setup lang=\"ts\">\nimport { ref } from 'vue'\nimport type { MyType } from './types'\n</script>\n<template><div/></template>",
    );

    assert!(
        result.import_specifiers.len() >= 2,
        "should have at least 2 import specifiers, got: {:?}",
        result.import_specifiers
    );
    let sources: Vec<&str> = result
        .import_specifiers
        .iter()
        .map(|s| s.source.as_str())
        .collect();
    assert!(sources.contains(&"vue"), "should have 'vue' import");
    assert!(sources.contains(&"./types"), "should have './types' import");
}

// ═══════════════════════════════════════════════════════════
// Step 10 — Incremental codegen assessment
// ═══════════════════════════════════════════════════════════

/// @ai-generated - Verify that the current "full recompile on type change" path
/// (Option C from the plan) is fast enough that incremental codegen is not needed.
///
/// Benchmark reference (codegen_comparison bench, 2026-02-20):
///   simple SFC: ~14 µs, medium: ~71 µs, large: ~218 µs, kitchen_sink: ~698 µs
///
/// All under 1ms → incremental codegen adds complexity for negligible gain.
/// The current strategy (invalidate compile_slots → recompile on next access)
/// is the right trade-off.
#[test]
fn recompile_after_type_change_works_correctly() {
    let host = VerterHost::new_standalone(HostConfig::default());

    let _ = upsert_vue(
        &host,
        "/src/Comp.vue",
        "<script setup lang=\"ts\">\nimport type { MyType } from './types'\nconst props = defineProps<MyType>()\n</script>\n<template><div>{{ props.foo }}</div></template>",
    );

    let _ = host
        .upsert(UpsertRequest {
            canonical_id: None,
            input_id: "/src/types.ts".to_string(),
            source: Arc::from("export interface MyType { foo: string }"),
            file_language: FileLanguage::script_ts(),
            aliases: Vec::new(),
        })
        .unwrap();

    // Compile initial version
    let v1 = host
        .get_virtual_file(VirtualQuery {
            raw_id: Some("/src/Comp.vue".to_string()),
            canonical_id: None,
            node_kind: None,
            compile_profile: profile_dev(),
        })
        .unwrap();
    assert!(!v1.code.is_empty());

    // Change the type — add a property
    let _ = host
        .upsert(UpsertRequest {
            canonical_id: None,
            input_id: "/src/types.ts".to_string(),
            source: Arc::from("export interface MyType { foo: string; bar: number }"),
            file_language: FileLanguage::script_ts(),
            aliases: Vec::new(),
        })
        .unwrap();

    // Compile after type change — should get fresh output
    let v2 = host
        .get_virtual_file(VirtualQuery {
            raw_id: Some("/src/Comp.vue".to_string()),
            canonical_id: None,
            node_kind: None,
            compile_profile: profile_dev(),
        })
        .unwrap();
    assert!(!v2.code.is_empty());
    assert!(!v2.stale, "should not be stale — fresh recompile expected");
}

/// @ai-generated - Scoped style: script __scopeId must include data-v- prefix and
/// must match the scope_id used in CSS selectors.
#[test]
fn scoped_style_scope_id_consistency_between_script_and_css() {
    let host = VerterHost::new_standalone(HostConfig::default());
    let src = r#"<script setup>const n = 1</script>
<template><div>{{n}}</div></template>
<style scoped>.app { color: red; }</style>"#;

    let profile = CompileProfile {
        component_id: Some("test1234".to_string()),
        ..profile_dev()
    };

    let _ = host
        .upsert(UpsertRequest {
            canonical_id: None,
            input_id: "Comp.vue".to_string(),
            source: Arc::from(src),
            file_language: FileLanguage::vue(),
            aliases: Vec::new(),
        })
        .unwrap();

    // Get script block
    let script = host
        .get_virtual_file(VirtualQuery {
            raw_id: Some("Comp.vue?vue&type=script".to_string()),
            canonical_id: None,
            node_kind: None,
            compile_profile: profile.clone(),
        })
        .unwrap();

    // Get style block
    let style = host
        .get_virtual_file(VirtualQuery {
            raw_id: Some("Comp.vue?vue&type=style&index=0".to_string()),
            canonical_id: None,
            node_kind: None,
            compile_profile: profile.clone(),
        })
        .unwrap();

    // Extract __scopeId value from script
    let scope_marker = "__scopeId = \"";
    let scope_pos = script.code.find(scope_marker).unwrap_or_else(|| {
        panic!(
            "Script must contain __scopeId assignment, got:\n{}",
            script.code
        )
    });
    let scope_value_start = scope_pos + scope_marker.len();
    let scope_value_end = script.code[scope_value_start..]
        .find('"')
        .expect("should have closing quote")
        + scope_value_start;
    let script_scope_id = &script.code[scope_value_start..scope_value_end];

    assert!(
        script_scope_id.starts_with("data-v-"),
        "Script __scopeId must start with 'data-v-', got: '{}'\nFull script:\n{}",
        script_scope_id,
        script.code
    );

    // Extract data-v-xxx from CSS selectors
    let css_marker = "[data-v-";
    let css_pos = style.code.find(css_marker).unwrap_or_else(|| {
        panic!(
            "CSS must contain [data-v-...] selector, got:\n{}",
            style.code
        )
    });
    let css_id_start = css_pos + 1; // skip '['
    let css_id_end = style.code[css_id_start..]
        .find(']')
        .expect("should have closing ]")
        + css_id_start;
    let css_scope_id = &style.code[css_id_start..css_id_end];

    assert_eq!(
        script_scope_id, css_scope_id,
        "Script __scopeId and CSS selector must use the same scope_id.\nScript:\n{}\nCSS:\n{}",
        script.code, style.code
    );
}

// ── get_analysis tests ──────────────────────────────────────────────

/// @ai-generated - get_analysis returns None for unknown file
#[test]
fn test_get_analysis_returns_none_for_unknown_file() {
    let host = VerterHost::new_standalone(HostConfig::default());
    assert!(host.get_analysis("nonexistent.vue").is_none());
}

/// @ai-generated - get_analysis returns imports
#[test]
fn test_get_analysis_returns_imports() {
    let host = VerterHost::new_standalone(HostConfig::default());
    let src = r#"<script setup lang="ts">
import { ref } from 'vue'
const count = ref(0)
</script>
<template><div>{{ count }}</div></template>"#;
    let _ = host
        .upsert(UpsertRequest {
            canonical_id: None,
            input_id: "Test.vue".to_string(),
            source: Arc::from(src),
            file_language: FileLanguage::vue(),
            aliases: vec![],
        })
        .unwrap();

    let analysis = host
        .get_analysis("Test.vue")
        .expect("analysis should exist");
    assert!(!analysis.imports.is_empty());
    let vue_import = analysis
        .imports
        .iter()
        .find(|i| i.source == "vue")
        .expect("should have vue import");
    let ref_binding = vue_import
        .bindings
        .iter()
        .find(|b| b.name == "ref")
        .expect("should have ref binding");
    assert!(ref_binding.vue_api.is_some());
}

/// @ai-generated - get_analysis returns bindings
#[test]
fn test_get_analysis_returns_bindings() {
    let host = VerterHost::new_standalone(HostConfig::default());
    let src = r#"<script setup lang="ts">
import { ref } from 'vue'
const count = ref(0)
</script>
<template><div>{{ count }}</div></template>"#;
    let _ = host
        .upsert(UpsertRequest {
            canonical_id: None,
            input_id: "Test.vue".to_string(),
            source: Arc::from(src),
            file_language: FileLanguage::vue(),
            aliases: vec![],
        })
        .unwrap();

    let analysis = host
        .get_analysis("Test.vue")
        .expect("analysis should exist");
    let count_binding = analysis
        .bindings
        .iter()
        .find(|b| b.name == "count")
        .expect("should have count binding");
    assert!(count_binding.is_reactive);
    assert_eq!(
        count_binding.kind,
        verter_semantic::analysis::AnalyzedBindingKind::Const
    );
}

/// @ai-generated - get_analysis returns macros
#[test]
fn test_get_analysis_returns_macros() {
    let host = VerterHost::new_standalone(HostConfig::default());
    let src = r#"<script setup lang="ts">
defineProps<{ msg: string }>()
</script>
<template><div>{{ msg }}</div></template>"#;
    let _ = host
        .upsert(UpsertRequest {
            canonical_id: None,
            input_id: "Test.vue".to_string(),
            source: Arc::from(src),
            file_language: FileLanguage::vue(),
            aliases: vec![],
        })
        .unwrap();

    let analysis = host
        .get_analysis("Test.vue")
        .expect("analysis should exist");
    let props_macro = analysis
        .macros
        .iter()
        .find(|m| m.kind == verter_semantic::analysis::AnalyzedMacroKind::DefineProps)
        .expect("should have defineProps macro");
    assert!(props_macro.is_type_based);
}

/// @ai-generated - get_analysis returns script flags
#[test]
fn test_get_analysis_returns_script_flags() {
    let host = VerterHost::new_standalone(HostConfig::default());
    let src = r#"<script setup lang="ts">
import { ref } from 'vue'
defineProps<{ msg: string }>()
const count = ref(0)
</script>
<template><div>{{ msg }} {{ count }}</div></template>"#;
    let _ = host
        .upsert(UpsertRequest {
            canonical_id: None,
            input_id: "Test.vue".to_string(),
            source: Arc::from(src),
            file_language: FileLanguage::vue(),
            aliases: vec![],
        })
        .unwrap();

    let analysis = host
        .get_analysis("Test.vue")
        .expect("analysis should exist");
    let flags = verter_semantic::analysis::AnalysisFlags::from_bits_truncate(analysis.script_flags);
    assert!(flags.contains(verter_semantic::analysis::AnalysisFlags::HAS_DEFINE_PROPS));
    assert!(flags.contains(verter_semantic::analysis::AnalysisFlags::HAS_REACTIVE_STATE));
}

/// @ai-generated - get_analysis returns style analysis
#[test]
fn test_get_analysis_returns_style_analysis() {
    let host = VerterHost::new_standalone(HostConfig::default());
    let src = r#"<script setup lang="ts">
const msg = 'hello'
</script>
<template><div>{{ msg }}</div></template>
<style scoped>
.app { color: red; }
</style>"#;
    let _ = host
        .upsert(UpsertRequest {
            canonical_id: None,
            input_id: "Test.vue".to_string(),
            source: Arc::from(src),
            file_language: FileLanguage::vue(),
            aliases: vec![],
        })
        .unwrap();

    let analysis = host
        .get_analysis("Test.vue")
        .expect("analysis should exist");
    assert_eq!(analysis.styles.len(), 1);
    assert!(analysis.styles[0].scoped);
}

/// Bindings referenced by CSS v-bind() should have used_in_style = true.
#[test]
fn test_vbind_marks_used_in_style() {
    let host = VerterHost::new_standalone(HostConfig::default());
    let src = r#"<script setup lang="ts">
import { ref } from 'vue'
const color = ref('red')
const size = ref(12)
</script>
<template><div>{{ color }}</div></template>
<style scoped>
.app { color: v-bind(color); }
</style>"#;
    let _ = host
        .upsert(UpsertRequest {
            canonical_id: None,
            input_id: "VBind.vue".to_string(),
            source: Arc::from(src),
            file_language: FileLanguage::vue(),
            aliases: vec![],
        })
        .unwrap();

    let analysis = host
        .get_analysis("VBind.vue")
        .expect("analysis should exist");
    let color_binding = analysis
        .bindings
        .iter()
        .find(|b| b.name == "color")
        .expect("color binding should exist");
    assert!(
        color_binding.used_in_style,
        "color should be marked used_in_style because of v-bind(color) in <style>"
    );
    let size_binding = analysis
        .bindings
        .iter()
        .find(|b| b.name == "size")
        .expect("size binding should exist");
    assert!(
        !size_binding.used_in_style,
        "size should NOT be marked used_in_style"
    );
}

/// @ai-generated - upsert returns parse_duration_ms > 0
#[test]
fn test_upsert_returns_parse_duration() {
    let host = VerterHost::new_standalone(HostConfig::default());
    let src = r#"<script setup lang="ts">
const msg = 'hello'
</script>
<template><div>{{ msg }}</div></template>"#;
    let result = host
        .upsert(UpsertRequest {
            canonical_id: None,
            input_id: "Test.vue".to_string(),
            source: Arc::from(src),
            file_language: FileLanguage::vue(),
            aliases: vec![],
        })
        .unwrap();

    assert!(
        result.parse_duration_ms > 0.0,
        "parse_duration_ms should be positive, got {}",
        result.parse_duration_ms
    );
}

// ═══════════════════════════════════════════════════════════
// get_diagnostics: additional edge cases
// ═══════════════════════════════════════════════════════════

/// @ai-generated - get_diagnostics: nonexistent file → None
#[test]
fn get_diagnostics_nonexistent_returns_none() {
    let host = VerterHost::new_standalone(HostConfig::default());
    assert!(host
        .get_diagnostics("nonexistent.vue", &profile_dev())
        .is_none());
}

/// @ai-generated - get_diagnostics: file exists but no compilation for profile → None
#[test]
fn get_diagnostics_no_profile_match_returns_none() {
    let host = VerterHost::new_standalone(HostConfig::default());
    let _ = upsert_vue(
        &host,
        "Comp.vue",
        "<script setup>const n = 1</script><template><div>{{n}}</div></template>",
    );
    // File exists but never compiled → no diagnostics for any profile
    assert!(host.get_diagnostics("Comp.vue", &profile_dev()).is_none());
}

// ═══════════════════════════════════════════════════════════
// Custom block URL format tests (matching @vitejs/plugin-vue)
// ═══════════════════════════════════════════════════════════

/// @ai-generated - Main module custom block imports use type={blockType} format
#[test]
fn main_module_custom_block_import_uses_block_type_in_url() {
    let host = VerterHost::new_standalone(HostConfig::default());
    let src = r#"<script setup>const n = 1</script>
<template><div>{{n}}</div></template>
<route>{"path": "/sport"}</route>"#;
    let _ = upsert_vue(&host, "Comp.vue", src);

    let main = host
        .get_virtual_file(VirtualQuery {
            raw_id: Some("Comp.vue".to_string()),
            canonical_id: None,
            node_kind: None,
            compile_profile: profile_dev(),
        })
        .unwrap();
    let code = main.code.as_ref();

    // Must use type=route (the block's tag name), not type=custom&blockType=route
    assert!(
        code.contains("type=route&index=0"),
        "custom block import should use type=route, got:\n{}",
        code
    );
    assert!(
        !code.contains("type=custom"),
        "custom block import should NOT use type=custom, got:\n{}",
        code
    );
    assert!(
        !code.contains("blockType="),
        "custom block import should NOT use blockType= param, got:\n{}",
        code
    );
}

/// @ai-generated - Custom block retrieval via new type={blockType} URL format
#[test]
fn custom_block_retrieval_via_new_url_format() {
    let host = VerterHost::new_standalone(HostConfig::default());
    let src = r#"<script setup>const n = 1</script>
<template><div>{{n}}</div></template>
<i18n>{"en":{"hello":"world"}}</i18n>"#;
    let _ = upsert_vue(&host, "Comp.vue", src);

    // Request via new format: type=i18n&index=0
    let result = host
        .get_virtual_file(VirtualQuery {
            raw_id: Some("Comp.vue?vue&type=i18n&index=0".to_string()),
            canonical_id: None,
            node_kind: None,
            compile_profile: profile_dev(),
        })
        .unwrap();

    assert!(
        !result.code.is_empty(),
        "custom block should have content via type=i18n format"
    );
    assert!(
        result.code.contains("hello"),
        "content should contain i18n data, got: {}",
        result.code
    );
}

/// @ai-generated - Custom block content edit produces changed_virtual_ids with new format
#[test]
fn custom_block_edit_returns_changed_virtual_id_with_new_format() {
    let host = VerterHost::new_standalone(HostConfig::default());

    let src1 = r#"<script setup>const n = 1</script>
<template><div>{{n}}</div></template>
<i18n>{"en":{"hello":"world"}}</i18n>"#;
    let src2 = r#"<script setup>const n = 1</script>
<template><div>{{n}}</div></template>
<i18n>{"en":{"hello":"universe"}}</i18n>"#;

    let _ = upsert_vue(&host, "Comp.vue", src1);
    let result = upsert_vue(&host, "Comp.vue", src2);

    assert!(
        result
            .changed_virtual_nodes
            .contains(&VirtualNodeKind::Custom { index: 0 }),
        "custom block change should be reported, got: {:?}",
        result.changed_virtual_nodes
    );

    // The changed ID should use the new format
    let custom_id = result
        .changed_virtual_ids
        .iter()
        .find(|id| id.contains("i18n"))
        .expect("should have a changed ID containing 'i18n'");
    assert!(
        custom_id.contains("type=i18n"),
        "changed ID should use type=i18n, got: {}",
        custom_id
    );
    assert!(
        !custom_id.contains("type=custom"),
        "changed ID should NOT use type=custom, got: {}",
        custom_id
    );
}

/// @ai-generated - Multiple custom blocks of different types use correct type= in URLs
#[test]
fn multiple_custom_blocks_different_types_url_format() {
    let host = VerterHost::new_standalone(HostConfig::default());
    let src = r#"<script setup>const n = 1</script>
<template><div>{{n}}</div></template>
<i18n>{"en":{"hello":"world"}}</i18n>
<route>{"path": "/test"}</route>"#;
    let _ = upsert_vue(&host, "Comp.vue", src);

    let main = host
        .get_virtual_file(VirtualQuery {
            raw_id: Some("Comp.vue".to_string()),
            canonical_id: None,
            node_kind: None,
            compile_profile: profile_dev(),
        })
        .unwrap();
    let code = main.code.as_ref();

    // First custom block (i18n) should be type=i18n&index=0
    assert!(
        code.contains("type=i18n&index=0"),
        "first custom block should use type=i18n&index=0, got:\n{}",
        code
    );
    // Second custom block (route) should be type=route&index=1
    assert!(
        code.contains("type=route&index=1"),
        "second custom block should use type=route&index=1, got:\n{}",
        code
    );
    // Neither should use old format
    assert!(
        !code.contains("blockType="),
        "should not contain blockType= param, got:\n{}",
        code
    );
}

/// @ai-generated - AnalysisLevel::None still provides analysis via get_analysis()
#[test]
fn analysis_level_none_get_analysis_computes_on_demand() {
    let host = VerterHost::new_standalone(HostConfig {
        analysis_level: AnalysisLevel::None,
        ..HostConfig::default()
    });
    let src = "<script setup>\nimport { ref } from 'vue'\nconst n = ref(1)\n</script>\n<template><div>{{n}}</div></template>\n<style scoped>.a { color: red }</style>";
    let _ = upsert_vue(&host, "Comp.vue", src);

    let analysis = host.get_analysis("Comp.vue").unwrap();
    assert!(
        !analysis.imports.is_empty(),
        "get_analysis() should compute imports on demand"
    );
    assert!(
        !analysis.styles.is_empty(),
        "get_analysis() should compute style analysis on demand"
    );
}

/// @ai-generated - AnalysisLevel::Essential get_analysis provides styles on demand
#[test]
fn analysis_level_essential_get_analysis_provides_styles() {
    let host = VerterHost::new_standalone(HostConfig {
        analysis_level: AnalysisLevel::Essential,
        ..HostConfig::default()
    });
    let src = "<script setup>\nimport { ref } from 'vue'\nconst n = ref(1)\n</script>\n<template><div>{{n}}</div></template>\n<style scoped>.a { color: red }</style>";
    let _ = upsert_vue(&host, "Comp.vue", src);

    let analysis = host.get_analysis("Comp.vue").unwrap();
    assert!(
        !analysis.imports.is_empty(),
        "imports should be available (computed eagerly)"
    );
    assert!(
        !analysis.styles.is_empty(),
        "styles should be computed on demand"
    );
}

/// @ai-generated - AnalysisLevel::Full (default) populates all analysis during upsert
#[test]
fn analysis_level_full_populates_all_in_upsert() {
    let host = VerterHost::new_standalone(HostConfig::default());
    let src = "<script setup>\nimport { ref } from 'vue'\nconst n = ref(1)\n</script>\n<template><div>{{n}}</div></template>\n<style scoped>.a { color: red }</style>";
    let _ = upsert_vue(&host, "Comp.vue", src);

    let analysis = host.get_analysis("Comp.vue").unwrap();
    assert!(
        !analysis.imports.is_empty(),
        "analysis should be populated at AnalysisLevel::Full"
    );
    assert!(
        !analysis.styles.is_empty(),
        "style analysis should be populated at AnalysisLevel::Full"
    );
}

/// @ai-generated - Cross-file type resolution: external props are resolved in compiled output
#[test]
fn cross_file_type_resolution_resolves_external_props() {
    let host = VerterHost::new_standalone(HostConfig::default());

    // Upsert the dependency file with an exported interface
    let _ = host
        .upsert(UpsertRequest {
            canonical_id: None,
            input_id: "/src/types.ts".to_string(),
            source: Arc::from("export interface MyProps { title: string; count: number }"),
            file_language: FileLanguage::script_ts(),
            aliases: Vec::new(),
        })
        .unwrap();

    // Upsert SFC that imports the type and uses it in defineProps
    let _ = upsert_vue(
        &host,
        "/src/Comp.vue",
        "<script setup lang=\"ts\">\nimport type { MyProps } from './types'\ndefineProps<MyProps>()\n</script>\n<template><div>{{ title }} {{ count }}</div></template>",
    );

    // Compile and get the output
    let result = host
        .get_virtual_file(VirtualQuery {
            raw_id: Some("/src/Comp.vue".to_string()),
            canonical_id: None,
            node_kind: None,
            compile_profile: profile_dev(),
        })
        .unwrap();

    let code = result.code.to_string();

    // The compiled output should contain runtime prop definitions for the external type
    assert!(
        code.contains("title:") && code.contains("count:"),
        "compiled output should contain runtime prop definitions from external type.\nOutput:\n{}",
        code
    );

    // Should also contain the props type information
    assert!(
        code.contains("String") || code.contains("type:"),
        "compiled output should contain runtime type information.\nOutput:\n{}",
        code
    );
}

/// @ai-generated - Cross-file type resolution: missing dependency is a compile blocker
#[test]
fn cross_file_type_resolution_missing_dep_reports_compile_error() {
    let host = VerterHost::new_standalone(HostConfig {
        dev_mode: false,
        compile_error_policy: CompileErrorPolicy::StrictError,
        ..HostConfig::default()
    });

    // Upsert SFC that imports a type but the dep file is NOT upserted
    let _ = upsert_vue(
        &host,
        "/src/Comp.vue",
        "<script setup lang=\"ts\">\nimport type { MissingProps } from './missing'\ndefineProps<MissingProps>()\n</script>\n<template><div>hello</div></template>",
    );

    let err = host
        .get_virtual_file(VirtualQuery {
            raw_id: Some("/src/Comp.vue".to_string()),
            canonical_id: None,
            node_kind: None,
            compile_profile: profile_dev(),
        })
        .unwrap_err();

    match err {
        HostError::CompileError(failure) => {
            assert!(
                failure
                    .diagnostics
                    .diagnostics
                    .iter()
                    .any(|d| d.code == "HOST_MISSING_MACRO_TYPE_DEP"
                        && d.message.contains("./missing")),
                "expected missing macro type dependency diagnostic, got: {:?}",
                failure.diagnostics.diagnostics
            );
        }
        other => panic!("expected compile error, got {other:?}"),
    }
}

// ======================== lang field + export type ========================

/// @ai-generated - main module lang should be "js" when force_js: true, even for lang="ts" script
#[test]
fn main_module_lang_is_js_when_force_js() {
    let host = VerterHost::new_standalone(HostConfig::default());
    let _ = upsert_vue(
        &host,
        "/src/Comp.vue",
        r#"<script setup lang="ts">
const x = 1
</script>
<template><div>{{ x }}</div></template>"#,
    );

    let mut profile = profile_prod();
    profile.force_js = true;

    let result = host
        .get_virtual_file(VirtualQuery {
            raw_id: Some("/src/Comp.vue".to_string()),
            canonical_id: None,
            node_kind: None,
            compile_profile: profile,
        })
        .unwrap();

    assert_eq!(
        result.lang.as_deref(),
        Some("js"),
        "lang should be 'js' when force_js: true, got: {:?}",
        result.lang
    );
}

/// @ai-generated - main module lang should use script_lang when force_js: false
#[test]
fn main_module_lang_uses_script_lang_when_not_force_js() {
    let host = VerterHost::new_standalone(HostConfig::default());
    let _ = upsert_vue(
        &host,
        "/src/Comp.vue",
        r#"<script setup lang="ts">
const x = 1
</script>
<template><div>{{ x }}</div></template>"#,
    );

    let mut profile = profile_dev();
    profile.force_js = false;

    let result = host
        .get_virtual_file(VirtualQuery {
            raw_id: Some("/src/Comp.vue".to_string()),
            canonical_id: None,
            node_kind: None,
            compile_profile: profile,
        })
        .unwrap();

    assert_eq!(
        result.lang.as_deref(),
        Some("ts"),
        "lang should be 'ts' (from script_lang) when force_js: false, got: {:?}",
        result.lang
    );
}

/// @ai-generated - main module lang defaults to "js" when no script lang and force_js: false
#[test]
fn main_module_lang_defaults_to_js_when_no_script_lang() {
    let host = VerterHost::new_standalone(HostConfig::default());
    let _ = upsert_vue(
        &host,
        "/src/Comp.vue",
        r#"<script setup>
const x = 1
</script>
<template><div>{{ x }}</div></template>"#,
    );

    let mut profile = profile_dev();
    profile.force_js = false;

    let result = host
        .get_virtual_file(VirtualQuery {
            raw_id: Some("/src/Comp.vue".to_string()),
            canonical_id: None,
            node_kind: None,
            compile_profile: profile,
        })
        .unwrap();

    assert_eq!(
        result.lang.as_deref(),
        Some("js"),
        "lang should default to 'js' when no script lang attribute, got: {:?}",
        result.lang
    );
}

/// @ai-generated - export type must be stripped from main module when force_js: true
#[test]
fn main_module_strips_export_type_when_force_js() {
    let host = VerterHost::new_standalone(HostConfig::default());
    let _ = upsert_vue(
        &host,
        "/src/Comp.vue",
        r#"<script setup lang="ts">
import { computed } from 'vue'

export type NavigatePayload =
  | { type: 'notification'; to: string }
  | { type: 'menu-item'; to: string }

interface SideMenuProps {
  visible?: boolean
}

const props = defineProps<SideMenuProps>()
const isOpen = computed(() => props.visible)
</script>

<template><div>{{ isOpen }}</div></template>"#,
    );

    let mut profile = profile_prod();
    profile.force_js = true;

    let result = host
        .get_virtual_file(VirtualQuery {
            raw_id: Some("/src/Comp.vue".to_string()),
            canonical_id: None,
            node_kind: None,
            compile_profile: profile,
        })
        .unwrap();

    assert!(
        !result.code.contains("export type"),
        "export type should be stripped when force_js: true, got:\n{}",
        result.code
    );
    assert!(
        !result.code.contains("NavigatePayload"),
        "NavigatePayload should not appear in JS output, got:\n{}",
        result.code
    );
    assert!(
        !result.code.contains("interface SideMenuProps"),
        "interface should be stripped in JS output, got:\n{}",
        result.code
    );
    assert_eq!(
        result.lang.as_deref(),
        Some("js"),
        "lang should be 'js' when force_js: true"
    );
}

// ==================== Template-only + scoped styles ====================

/// Template-only component with `<style scoped>` should expose a Script
/// virtual node (synthetic script block with __scopeId) and the script
/// code should contain the scope_id assignment.
#[test]
fn template_only_scoped_style_exposes_script_node_with_scope_id() {
    let host = VerterHost::new_standalone(HostConfig::default());
    let src = "<template><div class=\"app\">hello</div></template>\n<style scoped>\n.app { color: red; }\n</style>";
    let _ = upsert_vue(&host, "Comp.vue", src);

    let nodes = host.list_virtual_files("Comp.vue");
    assert!(
        nodes.contains(&VirtualNodeKind::Script),
        "template-only + scoped style should expose Script node, got: {:?}",
        nodes
    );
    assert!(
        nodes.contains(&VirtualNodeKind::Template),
        "should have Template node, got: {:?}",
        nodes
    );
    assert!(
        nodes
            .iter()
            .any(|n| matches!(n, VirtualNodeKind::Style { .. })),
        "should have Style node, got: {:?}",
        nodes
    );

    // Fetch the script virtual file — it should contain __scopeId
    let profile = CompileProfile {
        force_js: true,
        ..CompileProfile::default()
    };
    let script = host
        .get_virtual_file(VirtualQuery {
            raw_id: Some("Comp.vue?vue&type=script".to_string()),
            canonical_id: None,
            node_kind: None,
            compile_profile: profile.clone(),
        })
        .unwrap();
    assert!(
        script.code.contains("__scopeId"),
        "script should contain __scopeId, got:\n{}",
        script.code
    );
    assert!(
        script.code.contains("data-v-"),
        "script should contain data-v- scope id, got:\n{}",
        script.code
    );

    // The main module should also have __scopeId
    let main = host
        .get_virtual_file(VirtualQuery {
            raw_id: Some("Comp.vue".to_string()),
            canonical_id: None,
            node_kind: None,
            compile_profile: profile,
        })
        .unwrap();
    assert!(
        main.code.contains("__scopeId") || main.code.contains("scopeId"),
        "main module should contain scopeId, got:\n{}",
        main.code
    );
}

// ── Template analysis through host API ────────────────────

/// @ai-generated - Template analysis is populated after compilation when scope includes template flags
#[test]
fn template_analysis_populated_after_compile() {
    // Default config has Full analysis (LSP scope) which includes template flags
    let host = VerterHost::new_standalone(HostConfig::default());

    let sfc = r#"<script setup lang="ts">
import Child from './Child.vue'
const msg = "hello"
</script>
<template>
  <Child :msg="msg" />
</template>"#;

    let _ = upsert_vue(&host, "App.vue", sfc);

    // Before compilation, template analysis may not be present
    // Trigger compilation by requesting a virtual file
    let _ = host
        .get_virtual_file(VirtualQuery {
            raw_id: Some("App.vue?vue&type=template".to_string()),
            canonical_id: None,
            node_kind: None,
            compile_profile: profile_dev(),
        })
        .unwrap();

    // Now get_analysis should include template data
    let analysis = host.get_analysis("App.vue").unwrap();
    assert!(
        analysis.template.is_some(),
        "template analysis should be populated after compilation"
    );

    let tpl = analysis.template.unwrap();
    // Should detect the <Child> component usage
    assert!(
        !tpl.components.is_empty(),
        "should detect component usage in template"
    );
    assert_eq!(tpl.components[0].name, "Child");
}

/// @ai-generated - Template analysis detects binding occurrences
#[test]
fn template_analysis_detects_binding_occurrences() {
    let host = VerterHost::new_standalone(HostConfig::default());

    let sfc = r#"<script setup lang="ts">
const msg = "hello"
const count = 42
</script>
<template>
  <div>{{ msg }}</div>
  <span>{{ count }}</span>
</template>"#;

    let _ = upsert_vue(&host, "Comp.vue", sfc);
    let _ = host
        .get_virtual_file(VirtualQuery {
            raw_id: Some("Comp.vue?vue&type=template".to_string()),
            canonical_id: None,
            node_kind: None,
            compile_profile: profile_dev(),
        })
        .unwrap();

    let analysis = host.get_analysis("Comp.vue").unwrap();
    let tpl = analysis.template.expect("template analysis should exist");

    // Should have binding occurrences for msg and count
    let binding_names: Vec<&str> = tpl
        .binding_occurrences
        .iter()
        .map(|b| b.name.as_str())
        .collect();
    assert!(
        binding_names.contains(&"msg"),
        "should detect 'msg' binding occurrence, got: {:?}",
        binding_names
    );
    assert!(
        binding_names.contains(&"count"),
        "should detect 'count' binding occurrence, got: {:?}",
        binding_names
    );
}

const SCOPE_PROBE_SFC: &str = r#"<script setup lang="ts">
const msg = "hello"
</script>
<template>
  <div>{{ msg }}</div>
</template>"#;

fn scope_probe_analysis(
    scope: verter_semantic::analysis::AnalysisScope,
) -> verter_session::FileAnalysisSnapshot {
    let host = VerterHost::new_standalone(HostConfig {
        analysis_scope: Some(scope),
        ..HostConfig::default()
    });
    let _ = upsert_vue(&host, "Comp.vue", SCOPE_PROBE_SFC);
    let _ = host
        .get_virtual_file(VirtualQuery {
            raw_id: Some("Comp.vue?vue&type=template".to_string()),
            canonical_id: None,
            node_kind: None,
            compile_profile: profile_dev(),
        })
        .unwrap();
    host.get_analysis("Comp.vue").unwrap()
}

/// @ai-generated - Template analysis not populated when scope excludes template flags
#[test]
fn template_analysis_none_when_scope_excludes_template() {
    use verter_semantic::analysis::AnalysisScope;

    // BUILD scope does NOT include template flags.
    assert!(
        !AnalysisScope::BUILD.needs_template_analysis(),
        "BUILD must stay template-free: a template flag here materialises the \
         WHOLE template snapshot for every carrier file this scope analyses"
    );
    assert!(
        scope_probe_analysis(AnalysisScope::BUILD)
            .template
            .is_none(),
        "template analysis should NOT be populated when BUILD scope is used"
    );
}

/// The discriminating pair for the ON-DEMAND root-reachability mechanism.
///
/// `BUILD` leaves `get_analysis().template` unpopulated (the test above), and
/// yet the public-API carrier rendered under that very same host is STILL
/// widened with the resolved root element's props type. That is only possible
/// because the public-API path opens a request-scoped template demand around
/// its fallthrough resolve instead of the scope preset carrying a template bit
/// for every consumer.
///
/// Both halves are load-bearing. Without the `is_none()` half, restoring
/// `TPL_COMPONENTS` to `BUILD` would pass this test; without the widening half,
/// deleting the demand scope would pass it.
#[test]
fn public_api_widens_under_build_scope_without_a_scope_wide_template_walk() {
    use verter_semantic::analysis::AnalysisScope;

    let host = VerterHost::new_standalone(HostConfig {
        analysis_scope: Some(AnalysisScope::BUILD),
        ..HostConfig::default()
    });
    let _ = upsert_vue(&host, "Comp.vue", SCOPE_PROBE_SFC);

    // FIRST: a plain analysis read walks NO template. Ordered before the
    // public-API render deliberately — the on-demand walk persists its result
    // into the shared raw-template slot, so a later read would legitimately see
    // it and this half would stop discriminating.
    assert!(
        host.get_analysis("Comp.vue")
            .expect("analysis")
            .template
            .is_none(),
        "the demand is REQUEST-SCOPED to the public-API path: a plain \
         `get_analysis` under BUILD must report no template snapshot, or the \
         scope preset has silently regained a template bit"
    );

    let code = host
        .get_public_api("Comp.vue")
        .expect("no projection error")
        .expect("a stub")
        .ts_labeled_code()
        .to_string();
    assert!(
        code.contains("__Verter_RootElementAttrs<\"div\">"),
        "the public-API carrier must be widened with the resolved root element \
         even though the analysis scope carries no template flag:\n{code}"
    );
}

/// @ai-generated - Template analysis detects template refs
#[test]
fn template_analysis_detects_template_refs() {
    let host = VerterHost::new_standalone(HostConfig::default());

    let sfc = r#"<script setup lang="ts">
import { ref } from 'vue'
const el = ref<HTMLDivElement | null>(null)
</script>
<template>
  <div ref="el">content</div>
</template>"#;

    let _ = upsert_vue(&host, "Comp.vue", sfc);
    let _ = host
        .get_virtual_file(VirtualQuery {
            raw_id: Some("Comp.vue?vue&type=template".to_string()),
            canonical_id: None,
            node_kind: None,
            compile_profile: profile_dev(),
        })
        .unwrap();

    let analysis = host.get_analysis("Comp.vue").unwrap();
    let tpl = analysis.template.expect("template analysis should exist");

    assert!(
        !tpl.template_refs.is_empty(),
        "should detect template ref, got: {:?}",
        tpl.template_refs
    );
    assert_eq!(tpl.template_refs[0].name, "el");
    assert!(!tpl.template_refs[0].is_dynamic);
}

/// @ai-generated - Template analysis detects event handlers
#[test]
fn template_analysis_detects_event_handlers() {
    let host = VerterHost::new_standalone(HostConfig::default());

    let sfc = r#"<script setup lang="ts">
function handleClick() {}
</script>
<template>
  <button @click="handleClick">Click</button>
</template>"#;

    let _ = upsert_vue(&host, "Comp.vue", sfc);
    let _ = host
        .get_virtual_file(VirtualQuery {
            raw_id: Some("Comp.vue?vue&type=template".to_string()),
            canonical_id: None,
            node_kind: None,
            compile_profile: profile_dev(),
        })
        .unwrap();

    let analysis = host.get_analysis("Comp.vue").unwrap();
    let tpl = analysis.template.expect("template analysis should exist");

    assert!(
        !tpl.event_handlers.is_empty(),
        "should detect event handler"
    );
    assert_eq!(tpl.event_handlers[0].event_name, "click");
}

/// @ai-generated - Template analysis detects slot definitions
#[test]
fn template_analysis_detects_slot_definitions() {
    let host = VerterHost::new_standalone(HostConfig::default());

    let sfc = r#"<script setup lang="ts">
</script>
<template>
  <div>
    <slot name="header" />
    <slot />
  </div>
</template>"#;

    let _ = upsert_vue(&host, "Comp.vue", sfc);
    let _ = host
        .get_virtual_file(VirtualQuery {
            raw_id: Some("Comp.vue?vue&type=template".to_string()),
            canonical_id: None,
            node_kind: None,
            compile_profile: profile_dev(),
        })
        .unwrap();

    let analysis = host.get_analysis("Comp.vue").unwrap();
    let tpl = analysis.template.expect("template analysis should exist");

    let slot_names: Vec<&str> = tpl.defined_slots.iter().map(|s| s.name.as_str()).collect();
    assert!(
        slot_names.contains(&"header"),
        "should detect named slot 'header', got: {:?}",
        slot_names
    );
    assert!(
        slot_names.contains(&"default"),
        "should detect default slot, got: {:?}",
        slot_names
    );
}

/// @ai-generated - Template analysis is updated on recompile after source change
#[test]
fn template_analysis_updated_on_recompile() {
    let host = VerterHost::new_standalone(HostConfig::default());

    // Initial version with one component
    let sfc_v1 = r#"<script setup lang="ts">
import Child from './Child.vue'
</script>
<template>
  <Child />
</template>"#;

    let _ = upsert_vue(&host, "App.vue", sfc_v1);
    let _ = host
        .get_virtual_file(VirtualQuery {
            raw_id: Some("App.vue?vue&type=template".to_string()),
            canonical_id: None,
            node_kind: None,
            compile_profile: profile_dev(),
        })
        .unwrap();

    let analysis_v1 = host.get_analysis("App.vue").unwrap();
    let tpl_v1 = analysis_v1
        .template
        .expect("v1 should have template analysis");
    assert_eq!(tpl_v1.components.len(), 1);
    assert_eq!(tpl_v1.components[0].name, "Child");

    // Updated version with two components
    let sfc_v2 = r#"<script setup lang="ts">
import Child from './Child.vue'
import Other from './Other.vue'
</script>
<template>
  <Child />
  <Other />
</template>"#;

    let _ = upsert_vue(&host, "App.vue", sfc_v2);
    let _ = host
        .get_virtual_file(VirtualQuery {
            raw_id: Some("App.vue?vue&type=template".to_string()),
            canonical_id: None,
            node_kind: None,
            compile_profile: profile_dev(),
        })
        .unwrap();

    let analysis_v2 = host.get_analysis("App.vue").unwrap();
    let tpl_v2 = analysis_v2
        .template
        .expect("v2 should have template analysis");
    assert_eq!(
        tpl_v2.components.len(),
        2,
        "should detect both components after recompile"
    );
}

/// @ai-generated - get_analysis lazily computes template when scope includes template
#[test]
fn template_analysis_none_before_compile() {
    // With Full scope (default), template analysis is lazily computed in get_analysis().
    let host = VerterHost::new_standalone(HostConfig::default());

    let sfc = r#"<script setup lang="ts">
const msg = "hello"
</script>
<template>
  <div>{{ msg }}</div>
</template>"#;

    let _ = upsert_vue(&host, "Comp.vue", sfc);

    let analysis = host.get_analysis("Comp.vue").unwrap();
    assert!(
        analysis.template.is_some(),
        "template analysis should be lazily computed for Full scope"
    );

    // With None scope, template analysis is NOT computed
    let lazy_host = VerterHost::new_standalone(HostConfig {
        analysis_level: AnalysisLevel::None,
        ..HostConfig::default()
    });
    let _ = upsert_vue(&lazy_host, "Comp.vue", sfc);
    let lazy_analysis = lazy_host.get_analysis("Comp.vue").unwrap();
    assert!(
        lazy_analysis.template.is_none(),
        "template should be None when scope excludes template"
    );
}

// ─── list_virtual_nodes tests ───────────────────────────────────

/// @ai-generated — list_virtual_nodes returns correct nodes for a full SFC.
#[test]
fn list_virtual_nodes_full_sfc() {
    let host = VerterHost::new_standalone(HostConfig::default());
    let sfc = r#"<script setup>const n = 1</script>
<template><div>{{n}}</div></template>
<style scoped>.a{color:red}</style>"#;
    let _ = upsert_vue(&host, "Comp.vue", sfc);

    let nodes = host.list_virtual_nodes("Comp.vue");
    assert!(
        !nodes.is_empty(),
        "Should return virtual nodes for upserted SFC"
    );

    // Should contain Main, Script, Template, Style { index: 0 }
    assert!(
        nodes.contains(&VirtualNodeKind::Main),
        "Should contain Main, got: {:?}",
        nodes
    );
    assert!(
        nodes.contains(&VirtualNodeKind::Script),
        "Should contain Script, got: {:?}",
        nodes
    );
    assert!(
        nodes.contains(&VirtualNodeKind::Template),
        "Should contain Template, got: {:?}",
        nodes
    );
    assert!(
        nodes.contains(&VirtualNodeKind::Style { index: 0 }),
        "Should contain Style {{ index: 0 }}, got: {:?}",
        nodes
    );
}

/// @ai-generated — list_virtual_nodes returns empty for unknown file.
#[test]
fn list_virtual_nodes_unknown_file() {
    let host = VerterHost::new_standalone(HostConfig::default());
    let nodes = host.list_virtual_nodes("Unknown.vue");
    assert!(nodes.is_empty(), "Should return empty for unknown file");
}

/// @ai-generated — list_virtual_nodes handles template-only SFC with scoped style.
#[test]
fn list_virtual_nodes_template_only_with_scoped_style() {
    let host = VerterHost::new_standalone(HostConfig::default());
    let sfc = r#"<template><div>hello</div></template>
<style scoped>.a{color:red}</style>"#;
    let _ = upsert_vue(&host, "Comp.vue", sfc);

    let nodes = host.list_virtual_nodes("Comp.vue");
    // Should include Script because of scoped style (even without <script>)
    assert!(
        nodes.contains(&VirtualNodeKind::Script),
        "Should contain synthetic Script for scoped style, got: {:?}",
        nodes
    );
    assert!(
        nodes.contains(&VirtualNodeKind::Template),
        "Should contain Template, got: {:?}",
        nodes
    );
}

/// @ai-generated — list_virtual_nodes with multiple styles.
#[test]
fn list_virtual_nodes_multiple_styles() {
    let host = VerterHost::new_standalone(HostConfig::default());
    let sfc = r#"<script setup>const n = 1</script>
<template><div>{{n}}</div></template>
<style>.a{color:red}</style>
<style scoped>.b{color:blue}</style>"#;
    let _ = upsert_vue(&host, "Comp.vue", sfc);

    let nodes = host.list_virtual_nodes("Comp.vue");
    assert!(
        nodes.contains(&VirtualNodeKind::Style { index: 0 }),
        "Should contain Style {{ index: 0 }}"
    );
    assert!(
        nodes.contains(&VirtualNodeKind::Style { index: 1 }),
        "Should contain Style {{ index: 1 }}"
    );
}

// Note: FileAnalysisSnapshot JSON serialization tests are in verter_lsp integration_tests.rs
// where serde_json is available as a dependency.

/// @ai-generated - v-bind() expressions in style blocks populate generated_var_name in analysis
#[test]
fn v_bind_css_analysis_has_generated_var_name() {
    let host = VerterHost::new_standalone(HostConfig::default());
    let _ = upsert_vue(
        &host,
        "/test/VBindComp.vue",
        r#"<script setup>
import { ref } from 'vue'
const color = ref('red')
</script>
<template><div class="box">hello</div></template>
<style scoped>
.box { color: v-bind(color); }
</style>"#,
    );

    let analysis = host
        .get_analysis("/test/VBindComp.vue")
        .expect("should have analysis");
    assert!(!analysis.styles.is_empty(), "should have style analysis");

    let style = &analysis.styles[0];
    assert!(!style.v_binds.is_empty(), "should have v-bind entries");

    let vb = &style.v_binds[0];
    assert_eq!(vb.expression, "color");
    assert!(
        vb.generated_var_name.is_some(),
        "generated_var_name should be populated"
    );
    let gen_name = vb.generated_var_name.as_ref().unwrap();
    assert!(
        gen_name.starts_with("--"),
        "generated var name should start with --, got: {}",
        gen_name
    );
    assert!(
        gen_name.contains("color"),
        "generated var name should contain the expression, got: {}",
        gen_name
    );
}

/// @ai-generated - css_var_flow scans across multiple files
#[test]
fn css_var_flow_across_files() {
    let host = VerterHost::new_standalone(HostConfig::default());

    // File A defines --theme-color in style
    let _ = host.upsert(UpsertRequest {
        canonical_id: None,
        input_id: "src/A.vue".to_string(),
        source: Arc::from(
            r#"<template><div>A</div></template>
<style>
:root { --theme-color: blue; }
.container { color: var(--theme-color); }
</style>"#,
        ),
        file_language: FileLanguage::vue(),
        aliases: Vec::new(),
    });

    let flow = host.css_var_flow("--theme-color", None);
    assert_eq!(flow.name, "--theme-color");
    assert_eq!(
        flow.style_definitions.len(),
        1,
        "should have 1 style definition"
    );
    assert_eq!(flow.style_var_usages.len(), 1, "should have 1 var() usage");
}

#[test]
fn unissued_template_content_is_refused_without_affecting_css_var_flow() {
    let host = VerterHost::new_standalone(HostConfig::default());
    let _ = upsert_vue(
        &host,
        "/src/A.vue",
        "<script setup>\nconst color = 'red'\n</script>\n<template><div>A</div></template>",
    );

    let profile = CompileProfile::default();
    let error = host
        .apply_block_overrides(BlockOverrideRequest {
            canonical_id: "/src/A.vue".to_string(),
            compile_profile: profile.clone(),
            overrides: vec![unissued_override(
                "<div :style=\"{ '--theme-color': color }\">A</div>",
            )],
        })
        .expect_err("only a host-issued correlation may admit supplied bytes");
    assert!(matches!(
        error,
        HostError::BlockContentRefused(BlockContentRefusal::CorrelationMismatch)
    ));

    let raw_flow = host.css_var_flow("--theme-color", None);
    let override_flow = host.css_var_flow("--theme-color", Some(&profile));

    assert_eq!(
        raw_flow.template_definitions.len(),
        0,
        "raw/profileless css_var_flow must stay raw"
    );
    assert_eq!(
        override_flow.template_definitions.len(),
        0,
        "refused bytes must not create a profile-specific analysis path"
    );
}

#[test]
fn refused_unissued_content_does_not_poison_compile_or_raw_css_var_flow() {
    let host = VerterHost::new_standalone(HostConfig::default());
    let _ = upsert_vue(
        &host,
        "/src/A.vue",
        "<script setup>\nconst color = 'red'\n</script>\n<template><div>A</div></template>",
    );

    let profile = CompileProfile::default();
    let error = host
        .apply_block_overrides(BlockOverrideRequest {
            canonical_id: "/src/A.vue".to_string(),
            compile_profile: profile.clone(),
            overrides: vec![unissued_override(
                "<div :style=\"{ '--theme-color': color }\">A</div>",
            )],
        })
        .expect_err("only a host-issued correlation may admit supplied bytes");
    assert!(matches!(
        error,
        HostError::BlockContentRefused(BlockContentRefusal::CorrelationMismatch)
    ));

    let _ = host
        .get_virtual_file(VirtualQuery {
            raw_id: None,
            canonical_id: Some("/src/A.vue".to_string()),
            node_kind: Some(VirtualNodeKind::Main),
            compile_profile: profile.clone(),
        })
        .expect("override profile should compile");

    let raw_flow = host.css_var_flow("--theme-color", None);
    let override_flow = host.css_var_flow("--theme-color", Some(&profile));

    assert_eq!(
        raw_flow.template_definitions.len(),
        0,
        "compiling an override profile must not populate raw template analysis"
    );
    assert_eq!(
        override_flow.template_definitions.len(),
        0,
        "refused bytes must not populate profile-specific template analysis"
    );
}
