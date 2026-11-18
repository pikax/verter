//! Canonical ids, the request-id grammar and rendered virtual ids.

use super::*;

#[test]
fn parse_raw_id_bare_path_returns_main() {
    let result = parse_raw_id("Comp.vue").unwrap();
    assert_eq!(result.canonical_id, "Comp.vue");
    assert_eq!(result.node_kind, VirtualNodeKind::Main);
    assert!(!result.was_lsp_like);
}

#[test]
fn parse_raw_id_empty_query_returns_main() {
    let result = parse_raw_id("Comp.vue?").unwrap();
    assert_eq!(result.canonical_id, "Comp.vue");
    assert_eq!(result.node_kind, VirtualNodeKind::Main);
}

#[test]
fn parse_raw_id_unknown_type_without_index_returns_main() {
    let result = parse_raw_id("Comp.vue?type=foo").unwrap();
    assert_eq!(result.node_kind, VirtualNodeKind::Main);
}

#[test]
fn parse_raw_id_unknown_type_with_index_returns_custom() {
    let result = parse_raw_id("Comp.vue?vue&type=foo&index=3").unwrap();
    assert_eq!(result.node_kind, VirtualNodeKind::Custom { index: 3 });
}

#[test]
fn parse_raw_id_lsp_style_format() {
    let result = parse_raw_id("Comp.vue._VERTER_.bundle.ts").unwrap();
    assert_eq!(result.canonical_id, "Comp.vue");
    assert_eq!(result.node_kind, VirtualNodeKind::Main);
    assert!(result.was_lsp_like);
}

#[test]
fn parse_raw_id_lsp_custom_format() {
    let result = parse_raw_id("Comp.vue._VERTER_.custom.2.docs").unwrap();
    assert_eq!(result.canonical_id, "Comp.vue");
    assert_eq!(result.node_kind, VirtualNodeKind::Custom { index: 2 });
    assert!(result.was_lsp_like);
}

#[test]
fn parse_raw_id_lsp_unknown_suffix_is_rejected() {
    assert_eq!(
        parse_raw_id("Comp.vue._VERTER_.unknown.ts").unwrap_err(),
        RejectedRawId {
            reason: RawIdRejection::UnknownVirtualSuffix,
            raw_path: "Comp.vue".to_string(),
            marker: None,
        }
    );
}

#[test]
fn parse_raw_id_windows_backslashes_normalized() {
    let result = parse_raw_id("C:\\Users\\foo\\Comp.vue").unwrap();
    assert_eq!(result.canonical_id, "c:/Users/foo/Comp.vue");
}

#[test]
fn parse_raw_id_style_default_index_zero() {
    let result = parse_raw_id("Comp.vue?vue&type=style").unwrap();
    assert_eq!(result.node_kind, VirtualNodeKind::Style { index: 0 });
}

#[test]
fn parse_raw_id_case_insensitive_type_param() {
    let result = parse_raw_id("Comp.vue?VUE&TYPE=SCRIPT").unwrap();
    assert_eq!(result.node_kind, VirtualNodeKind::Script);
}

#[test]
fn resolve_external_absolute() {
    assert_eq!(
        resolve_external("/src/Comp.vue", "/lib/helper.ts"),
        "/lib/helper.ts"
    );
}

#[test]
fn resolve_external_relative_sibling() {
    assert_eq!(
        resolve_external("/src/Comp.vue", "./helper.ts"),
        "/src/helper.ts"
    );
}

#[test]
fn resolve_external_relative_parent() {
    assert_eq!(
        resolve_external("/src/components/Comp.vue", "../utils.ts"),
        "/src/utils.ts"
    );
}

#[test]
fn resolve_external_double_parent() {
    assert_eq!(
        resolve_external("/src/a/b/Comp.vue", "../../c.ts"),
        "/src/c.ts"
    );
}

#[test]
fn resolve_external_bare_specifier() {
    assert_eq!(resolve_external("/src/Comp.vue", "lodash"), "lodash");
}

#[test]
fn resolve_external_dot_slash() {
    assert_eq!(
        resolve_external("/src/Comp.vue", "./sub/file.ts"),
        "/src/sub/file.ts"
    );
}

// ═══════════════════════════════════════════════════════════
// Additional id.rs tests
// ═══════════════════════════════════════════════════════════

/// @ai-generated - resolve_external with excessive ../ clamps at root
#[test]
fn resolve_external_excessive_dotdot() {
    // /a/Comp.vue → owner parts = ["", "a"], after pop filename = [""]
    // ../../../z.ts → ".." can't pop past root "" → stays [""], pushes "z.ts" → ["", "z.ts"]
    let result = resolve_external("/a/Comp.vue", "../../../z.ts");
    assert_eq!(result, "/z.ts");
}

/// @ai-generated - render_ids + parse_raw_id roundtrip for all node kinds
#[test]
fn render_ids_parse_raw_id_roundtrip() {
    let meta = FileMeta {
        has_script: true,
        has_template: true,
        script_lang: Some("ts".to_string()),
        style_langs: vec![Some("scss".to_string()), None],
        custom_types: vec!["i18n".to_string()],
        ..FileMeta::default()
    };
    let cases: Vec<VirtualNodeKind> = vec![
        VirtualNodeKind::Main,
        VirtualNodeKind::Script,
        VirtualNodeKind::Template,
        VirtualNodeKind::Style { index: 0 },
        VirtualNodeKind::Style { index: 1 },
        VirtualNodeKind::Custom { index: 0 },
    ];

    for node in &cases {
        let (bundler_id, lsp_id) = render_ids("/src/Comp.vue", node, &meta);

        // Bundler ID should roundtrip
        let parsed_b = parse_raw_id(&bundler_id).unwrap();
        assert_eq!(
            parsed_b.canonical_id, "/src/Comp.vue",
            "bundler roundtrip failed for {:?}",
            node
        );
        assert_eq!(
            parsed_b.node_kind, *node,
            "bundler node kind failed for {:?}",
            node
        );

        // LSP ID should roundtrip
        let parsed_l = parse_raw_id(&lsp_id).unwrap();
        assert_eq!(
            parsed_l.canonical_id, "/src/Comp.vue",
            "lsp roundtrip failed for {:?}",
            node
        );
        assert_eq!(
            parsed_l.node_kind, *node,
            "lsp node kind failed for {:?}",
            node
        );
        assert!(
            parsed_l.was_lsp_like,
            "lsp format not detected for {:?}",
            node
        );
    }
}

// ═══════════════════════════════════════════════════════════
// Custom block URL format tests (matching @vitejs/plugin-vue)
// ═══════════════════════════════════════════════════════════

/// @ai-generated - render_ids for custom blocks must use type={blockType} (not type=custom&blockType=...)
#[test]
fn render_ids_custom_block_uses_block_type_as_type_param() {
    let meta = FileMeta {
        custom_types: vec!["route".to_string(), "i18n".to_string()],
        ..FileMeta::default()
    };

    let (bundler_0, _) = render_ids(
        "/src/Comp.vue",
        &VirtualNodeKind::Custom { index: 0 },
        &meta,
    );
    let (bundler_1, _) = render_ids(
        "/src/Comp.vue",
        &VirtualNodeKind::Custom { index: 1 },
        &meta,
    );

    // Must match @vitejs/plugin-vue format: type={blockType}&index={idx}
    assert_eq!(bundler_0, "/src/Comp.vue?vue&type=route&index=0");
    assert_eq!(bundler_1, "/src/Comp.vue?vue&type=i18n&index=1");

    // Must NOT contain type=custom or blockType=
    assert!(!bundler_0.contains("type=custom"));
    assert!(!bundler_0.contains("blockType="));
    assert!(!bundler_1.contains("type=custom"));
    assert!(!bundler_1.contains("blockType="));
}

/// @ai-generated - render_ids falls back to "custom" when custom_types is empty
#[test]
fn render_ids_custom_block_fallback_type() {
    let meta = FileMeta::default(); // no custom_types
    let (bundler, _) = render_ids(
        "/src/Comp.vue",
        &VirtualNodeKind::Custom { index: 0 },
        &meta,
    );
    assert_eq!(bundler, "/src/Comp.vue?vue&type=custom&index=0");
}

/// @ai-generated - parse_raw_id treats type=route with index as Custom (new format)
#[test]
fn parse_raw_id_block_type_as_type_param() {
    let result = parse_raw_id("Comp.vue?vue&type=route&index=0").unwrap();
    assert_eq!(result.canonical_id, "Comp.vue");
    assert_eq!(result.node_kind, VirtualNodeKind::Custom { index: 0 });

    let result2 = parse_raw_id("Comp.vue?vue&type=i18n&index=2").unwrap();
    assert_eq!(result2.node_kind, VirtualNodeKind::Custom { index: 2 });
}

/// @ai-generated - render_ids → parse_raw_id roundtrip for new custom block format
#[test]
fn render_parse_roundtrip_new_custom_format() {
    let meta = FileMeta {
        custom_types: vec!["route".to_string(), "i18n".to_string(), "docs".to_string()],
        ..FileMeta::default()
    };

    for idx in 0..3 {
        let node = VirtualNodeKind::Custom { index: idx };
        let (bundler_id, lsp_id) = render_ids("/src/Comp.vue", &node, &meta);

        let parsed_b = parse_raw_id(&bundler_id).unwrap();
        assert_eq!(
            parsed_b.canonical_id, "/src/Comp.vue",
            "bundler roundtrip canonical_id for index {}",
            idx
        );
        assert_eq!(
            parsed_b.node_kind, node,
            "bundler roundtrip node_kind for index {}",
            idx
        );

        let parsed_l = parse_raw_id(&lsp_id).unwrap();
        assert_eq!(
            parsed_l.canonical_id, "/src/Comp.vue",
            "lsp roundtrip canonical_id for index {}",
            idx
        );
        assert_eq!(
            parsed_l.node_kind, node,
            "lsp roundtrip node_kind for index {}",
            idx
        );
    }
}

/// @ai-generated - canonicalize_id: both query and ._VERTER_. — query takes precedence
#[test]
fn canonicalize_id_both_query_and_verter() {
    // If input has both ? and ._VERTER_., the ? split happens first
    let result = canonicalize_id("Comp.vue?vue&type=script._VERTER_.foo");
    assert_eq!(result, "Comp.vue");
}

/// @ai-generated - resolve_external with owner having no directory part
#[test]
fn resolve_external_no_directory() {
    let result = resolve_external("Comp.vue", "./helper.ts");
    assert_eq!(result, "helper.ts");
}

/// @ai-generated - Windows drive letter normalization: C:\ and c:\ should match
#[test]
fn canonicalize_id_normalizes_drive_letter_case() {
    let upper = canonicalize_id("C:/src/App.vue");
    let lower = canonicalize_id("c:/src/App.vue");
    assert_eq!(upper, lower, "drive letter case should be normalized");
}

/// @ai-generated - Windows backslash + uppercase drive letter
#[test]
fn canonicalize_id_backslash_and_drive_letter() {
    let result = canonicalize_id("D:\\src\\App.vue");
    assert_eq!(result, "d:/src/App.vue");
}

#[test]
fn canonicalize_id_strips_windows_extended_length_prefix() {
    let result = canonicalize_id("//?/C:/Users/david/workspace/src/App.vue");
    assert_eq!(result, "c:/Users/david/workspace/src/App.vue");
}

#[test]
fn canonicalize_id_strips_windows_extended_length_backslash_prefix() {
    let result = canonicalize_id("\\\\?\\C:\\Users\\david\\workspace\\src\\App.vue");
    assert_eq!(result, "c:/Users/david/workspace/src/App.vue");
}

#[test]
fn canonicalize_id_extended_length_does_not_corrupt_query_param() {
    // Ensure //?/ prefix doesn't cause the ? to be treated as query separator
    let result = canonicalize_id("//?/D:/project/Comp.vue?vue&type=script");
    assert_eq!(result, "d:/project/Comp.vue");
}

#[test]
fn canonicalize_id_extended_length_unc_keeps_server_and_share() {
    for raw in [
        "//?/UNC/srv/share/a.vue",
        "\\\\?\\UNC\\srv\\share\\a.vue",
        "//?/UNC/srv/share/a.vue?vue&type=script",
    ] {
        assert_eq!(canonicalize_id(raw), "//srv/share/a.vue", "{raw}");
    }
}

#[test]
fn canonicalize_id_extended_length_preserves_path() {
    let result = canonicalize_id("//?/C:/Users/david/node_modules/motion/dist/index.d.ts");
    assert_eq!(result, "c:/Users/david/node_modules/motion/dist/index.d.ts");
    assert!(
        !result.as_ref().contains("//?/"),
        "extended-length prefix must be stripped"
    );
}

#[test]
fn every_role_marker_and_language_parses() {
    let roles = [
        ("script", VirtualNodeKind::Script),
        ("template", VirtualNodeKind::Template),
        ("style&index=2", VirtualNodeKind::Style { index: 2 }),
        ("i18n&index=1", VirtualNodeKind::Custom { index: 1 }),
    ];
    for (marker_key, marker) in [
        ("vue", RequestMarker::Vue),
        ("verter", RequestMarker::Verter),
    ] {
        for (role, node_kind) in &roles {
            for lang in [None, Some("ts"), Some("module.css")] {
                let lang_token = lang.map_or(String::new(), |ext| format!("&lang.{ext}"));
                let raw = format!("/src/Comp.vue?{marker_key}&type={role}{lang_token}");
                let parsed = parse_raw_id(&raw).unwrap();
                assert_eq!(parsed.canonical_id, "/src/Comp.vue", "{raw}");
                assert_eq!(parsed.raw_path, "/src/Comp.vue", "{raw}");
                assert_eq!(parsed.marker, Some(marker), "{raw}");
                assert_eq!(parsed.node_kind, *node_kind, "{raw}");
                assert_eq!(parsed.lang.as_deref(), lang, "{raw}");
                assert!(!parsed.scoped, "{raw}");
            }
        }
    }
}

#[test]
fn malformed_requests_are_rejected_with_their_reason() {
    use RawIdRejection::{BothMarkers, InvalidIndex, MarkerWithoutType, RepeatedOwnedKey};
    for (raw, rejection) in [
        ("Comp.vue?vue&verter&type=script", BothMarkers),
        ("Comp.vue?vue", MarkerWithoutType),
        ("Comp.vue?vue&type=", MarkerWithoutType),
        ("Comp.vue?verter&lang.ts", MarkerWithoutType),
        ("Comp.vue?vue&vue&type=script", RepeatedOwnedKey),
        ("Comp.vue?verter&verter&type=script", RepeatedOwnedKey),
        ("Comp.vue?vue&type=script&type=style", RepeatedOwnedKey),
        ("Comp.vue?vue&type=style&index=0&index=1", RepeatedOwnedKey),
        ("Comp.vue?vue&type=style&index=-1", InvalidIndex),
        ("Comp.vue?vue&type=style&index=+1", InvalidIndex),
        ("Comp.vue?vue&type=style&index=a", InvalidIndex),
        ("Comp.vue?vue&type=style&index=", InvalidIndex),
        ("Comp.vue?vue&type=style&index", InvalidIndex),
    ] {
        assert_eq!(parse_raw_id(raw).unwrap_err().reason, rejection, "{raw}");
    }
}

#[test]
fn a_custom_block_request_keeps_its_block_type() {
    let parsed = parse_raw_id("Comp.vue?vue&type=i18n&index=0").unwrap();
    assert_eq!(parsed.node_kind, VirtualNodeKind::Custom { index: 0 });
    assert_eq!(parsed.block_type.as_deref(), Some("i18n"));
    assert_eq!(
        render_resolved_ids("/src/Comp.vue", &parsed, None, RequestMarker::Vue).0,
        "/src/Comp.vue?vue&type=i18n&index=0"
    );

    let lsp = parse_raw_id("Comp.vue._VERTER_.custom.2.docs").unwrap();
    assert_eq!(lsp.block_type.as_deref(), Some("docs"));
    for not_custom in [
        "Comp.vue?vue&type=style&index=0",
        "Comp.vue?type=i18n&index=0",
    ] {
        assert_eq!(
            parse_raw_id(not_custom).unwrap().block_type,
            None,
            "{not_custom}"
        );
    }
}

#[test]
fn a_rejected_request_keeps_its_path_and_marker() {
    let rejected = parse_raw_id("/src/App.vue?vue").unwrap_err();
    assert_eq!(rejected.reason, RawIdRejection::MarkerWithoutType);
    assert_eq!(rejected.raw_path, "/src/App.vue");
    assert_eq!(rejected.marker, Some(RequestMarker::Vue));
    assert_eq!(
        parse_raw_id("/src/App.svelte?vue&verter&type=style")
            .unwrap_err()
            .marker,
        None
    );
}

#[test]
fn a_request_without_a_marker_is_main() {
    for raw in [
        "/src/Foo.vue?macro=true",
        "/src/Foo.vue?type=style&index=0",
        "/src/Foo.vue?type=script&type=style&index=x",
        "/src/Foo.vue?used&inline&t=123",
    ] {
        let parsed = parse_raw_id(raw).unwrap();
        assert_eq!(parsed.node_kind, VirtualNodeKind::Main, "{raw}");
        assert_eq!(parsed.marker, None, "{raw}");
        assert_eq!(parsed.canonical_id, "/src/Foo.vue", "{raw}");
    }
}

#[test]
fn the_path_ends_at_the_first_query_separator() {
    let parsed = parse_raw_id("/src/Foo.vue?vue&type=style&x=a?b&lang.css").unwrap();
    assert_eq!(parsed.raw_path, "/src/Foo.vue");
    assert_eq!(parsed.canonical_id, "/src/Foo.vue");
    assert_eq!(parsed.node_kind, VirtualNodeKind::Style { index: 0 });
    assert_eq!(parsed.lang.as_deref(), Some("css"));

    let extended = parse_raw_id("//?/C:/proj/Foo.vue?vue&type=script").unwrap();
    assert_eq!(extended.raw_path, "//?/C:/proj/Foo.vue");
    assert_eq!(extended.canonical_id, "c:/proj/Foo.vue");
    assert_eq!(extended.node_kind, VirtualNodeKind::Script);
}

#[test]
fn both_language_spellings_and_scoped_are_read() {
    let dotted = parse_raw_id("Comp.vue?vue&type=style&index=0&lang.less").unwrap();
    let keyed = parse_raw_id("Comp.vue?vue&type=style&index=0&lang=less").unwrap();
    assert_eq!(dotted.lang.as_deref(), Some("less"));
    assert_eq!(keyed.lang, dotted.lang);
    assert!(!dotted.scoped);

    let scoped = parse_raw_id("Comp.vue?vue&type=style&index=0&scoped=7ba5bd90&lang.css").unwrap();
    assert!(scoped.scoped);
}

#[test]
fn render_ids_write_the_carrier_marker_and_the_language_last() {
    let meta = FileMeta {
        has_script: true,
        script_lang: Some("ts".to_string()),
        style_langs: vec![Some("scss".to_string())],
        ..FileMeta::default()
    };
    let (script, _) = render_ids("/src/Comp.vue", &VirtualNodeKind::Script, &meta);
    assert_eq!(script, "/src/Comp.vue?vue&type=script&lang.ts");

    let svelte = FileMeta {
        request_marker: RequestMarker::Verter,
        ..meta.clone()
    };
    let style = VirtualNodeKind::Style { index: 0 };
    let (svelte_style, svelte_style_lsp) = render_ids("/src/App.svelte", &style, &svelte);
    assert_eq!(
        svelte_style,
        "/src/App.svelte?verter&type=style&index=0&lang.scss"
    );
    assert_eq!(svelte_style_lsp, "/src/App.svelte._VERTER_.style.0.scss");

    let (svelte_template, _) = render_ids("/src/App.svelte", &VirtualNodeKind::Template, &svelte);
    assert_eq!(svelte_template, "/src/App.svelte?verter&type=template");

    let no_lang = FileMeta::default();
    let (plain_script, _) = render_ids("/src/Plain.vue", &VirtualNodeKind::Script, &no_lang);
    assert_eq!(plain_script, "/src/Plain.vue?vue&type=script&lang.js");
}

#[test]
fn an_unheld_carrier_renders_in_the_language_the_request_names() {
    let parsed = parse_raw_id("/src/Comp.vue?vue&type=style&index=1&lang.module.css").unwrap();
    assert_eq!(parsed.lang.as_deref(), Some("module.css"));
    let (bundler, lsp) = render_resolved_ids("/src/Comp.vue", &parsed, None, RequestMarker::Vue);
    assert_eq!(
        bundler,
        "/src/Comp.vue?vue&type=style&index=1&lang.module.css"
    );
    assert_eq!(lsp, "/src/Comp.vue._VERTER_.style.1.module.css");

    let held = FileMeta {
        style_langs: vec![None, Some("scss".to_string())],
        ..FileMeta::default()
    };
    let (held_bundler, _) =
        render_resolved_ids("/src/Comp.vue", &parsed, Some(&held), RequestMarker::Vue);
    assert_eq!(
        held_bundler,
        "/src/Comp.vue?vue&type=style&index=1&lang.scss"
    );
}
