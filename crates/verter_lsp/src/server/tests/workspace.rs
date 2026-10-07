use super::*;

/// A synthetic store-backed workspace root must be unique across concurrent PROCESSES,
/// not merely within one process.
///
/// These tests run one per PROCESS, so a per-process counter disambiguates nothing
/// across them, and `SystemTime::now()` is only MICROSECOND-resolution on macOS. A root
/// built from the clock alone aliases two test processes that reach it inside the same
/// microsecond onto ONE on-disk carrier store and one `manifest.json`, where a
/// read-modify-write from either erases the other's.
#[test]
fn synthetic_server_ws_root_is_unique_across_processes_not_only_within_one() {
    // Same microsecond, same per-process counter, same tag — only the process differs.
    const SAME_MICROSECOND: u128 = 1_785_068_278_682_867_000;
    let a = server_ws_root_for("b1_prod", 4242, SAME_MICROSECOND, 0);
    let b = server_ws_root_for("b1_prod", 4243, SAME_MICROSECOND, 0);
    assert_ne!(
        a, b,
        "two test PROCESSES deriving a root in the same microsecond must not get the \
         SAME workspace root"
    );

    // The consequence that actually bites: the derived store dirs must differ.
    let host = crate::external_ts::default_carrier_store_host_version();
    assert_ne!(
        crate::external_ts::carrier_store_dir_for(host, &a),
        crate::external_ts::carrier_store_dir_for(host, &b),
        "distinct test processes must resolve DISTINCT carrier-store dirs; an aliased \
         dir means two processes share one manifest"
    );

    // Distinct tags must stay distinct, and the within-process counter must still work.
    assert_ne!(
        server_ws_root_for("b1_prod", 4242, SAME_MICROSECOND, 0),
        server_ws_root_for("b1_other", 4242, SAME_MICROSECOND, 0),
        "two tags must not collapse onto one root"
    );

    // Every tag this file actually mints must be process-varying. `membership_publish`
    // and `foreign_mapping` regressed by building their root from a function-local
    // `AtomicUsize` starting at 0 instead of this seam: under one-test-per-process every
    // process minted `_0`, so the root was a FIXED STRING shared by every process AND
    // every run. `foreign_mapping` was the worse of the two — a SHARED fixture helper
    // with two callers, so two concurrent processes aliased onto one manifest.
    for tag in [
        "editor_live",
        "b1_prod",
        "b1_other",
        "reqsurf_pub",
        "membership_publish",
        "foreign_mapping",
    ] {
        assert_ne!(
            server_ws_root_for(tag, 4242, SAME_MICROSECOND, 0),
            server_ws_root_for(tag, 4243, SAME_MICROSECOND, 0),
            "tag {tag} must vary with the process identity"
        );
    }
    assert_ne!(
        server_ws_root_for("b1_prod", 4242, SAME_MICROSECOND, 0),
        server_ws_root_for("b1_prod", 4242, SAME_MICROSECOND, 1),
        "two roots taken inside one microsecond by ONE process must still differ"
    );

    // And the live derivation must actually vary the process identity in.
    let live = unique_server_ws_root("b1_prod");
    assert!(
        live.starts_with(&format!("/verter_b1_prod_{}_", std::process::id())),
        "unique_server_ws_root must carry this process's identity; got {live}"
    );
}

/// The carrier-store oracle must surface an unreadable / unparseable manifest as a
/// FAILURE, never launder it into "nothing is published".
///
/// `CarrierPublishStore::current_manifest` deliberately reports a fresh EMPTY manifest
/// for a corrupt one — correct for a read-only diagnostics view, wrong beneath these
/// tests' assertions. Two shapes go wrong: a presence `.expect(...)` blames the publish
/// instead of the store, and an ABSENCE assertion (`owner loss must retract`) passes
/// VACUOUSLY because an empty manifest trivially satisfies "not owned".
#[test]
fn carrier_manifest_oracle_reports_a_corrupt_manifest_as_a_failure_not_as_absence() {
    use crate::external_ts::{default_carrier_store_host_version, CarrierPublishStore};

    let corrupt_root = unique_server_ws_root("oracle_corrupt");
    let store = CarrierPublishStore::open(default_carrier_store_host_version(), &corrupt_root);
    std::fs::create_dir_all(store.workspace_dir()).expect("create the store dir");
    std::fs::write(store.manifest_path(), b"{ this manifest is truncated")
        .expect("write a corrupt manifest");

    // Pin the fail-open behaviour of the diagnostics reader the oracle must NOT inherit.
    assert!(
        store.current_manifest().projects.is_empty(),
        "the diagnostics reader is fail-open by design; this pins what the oracle must \
         not inherit"
    );

    let detail = std::panic::catch_unwind(|| carrier_manifest_strict(&corrupt_root)).expect_err(
        "a present-but-corrupt manifest must be a hard FAILURE from the oracle, never an \
         empty manifest that reads as nothing being published",
    );
    let message = detail
        .downcast_ref::<String>()
        .cloned()
        .unwrap_or_else(|| "<non-string panic>".to_string());
    assert!(
        message.contains("unparseable"),
        "the failure must name the actual cause; got {message:?}"
    );

    // A genuinely absent manifest stays distinguishable from a corrupt one.
    let absent_root = unique_server_ws_root("oracle_absent");
    assert!(
        carrier_manifest_strict(&absent_root).is_none(),
        "a store that was never published must read as None, not as a failure"
    );
}

#[test]
fn fixture_workspace_root_returns_canonical_path() {
    let workspace_id = fixture_workspace_root("single-project");

    assert!(
        workspace_id.starts_with('/') || workspace_id.chars().nth(1) == Some(':'),
        "fixture workspace path should be absolute, got: {workspace_id}"
    );
    assert!(
        !workspace_id.contains("/../"),
        "fixture workspace path should not retain dot segments, got: {workspace_id}"
    );
}

#[test]
fn analyzed_refs_resolve_extensionless_vue_dependencies_to_exact_files() {
    let resolver =
        verter_resolution::ModuleResolverCore::new(vec![verter_workspace::ide_project_config(
            "/workspace".to_string(),
            "/workspace".to_string(),
            Some("/workspace/tsconfig.app.json".to_string()),
        )]);
    let reader = TestResolverReader::with_files(&[
        "/workspace/src/tempUtil.ts",
        "/workspace/src/ExternalChild.vue",
    ]);
    let source =
        "import { MAGIC } from './tempUtil';\nimport ExternalChild from './ExternalChild.vue';\n";
    let temp_util_expr = "'./tempUtil'";
    let child_expr = "'./ExternalChild.vue'";
    let temp_util_start = source.find(temp_util_expr).unwrap();
    let child_start = source.find(child_expr).unwrap();

    let resolved = collect_resolved_provider_dependencies_from_analyzed_refs(
        &resolver,
        None,
        &reader,
        "/workspace/src/TempImporter.vue",
        &[
            test_analyzed_module_reference(
                temp_util_expr,
                Some("./tempUtil"),
                &[],
                verter_session_query::analysis::types::ModuleReferenceAnalyzability::Exact,
                temp_util_start,
                temp_util_start + temp_util_expr.len(),
            ),
            test_analyzed_module_reference(
                child_expr,
                Some("./ExternalChild.vue"),
                &[],
                verter_session_query::analysis::types::ModuleReferenceAnalyzability::Exact,
                child_start,
                child_start + child_expr.len(),
            ),
        ],
    )
    .expect("memory-backed resolution is publishable");

    let resolved_sources = resolved
        .iter()
        .map(|entry| entry.source_id.as_str())
        .collect::<Vec<_>>();
    assert_eq!(
            resolved_sources,
            vec!["/workspace/src/tempUtil.ts", "/workspace/src/ExternalChild.vue"],
            "Vue dependency tracking should use exact canonical IDs for extensionless TS imports and exact Vue imports"
        );
}

#[test]
fn vue_tsx_collision_with_real_file() {
    // A real .vue.tsx file exists but there's no matching .vue source in any project.
    // source_id_from_provider_carrier_path should return None (collision guard).
    let resolver =
        verter_resolution::ModuleResolverCore::new(vec![verter_workspace::ide_project_config(
            "/workspace/src".to_string(),
            "/workspace".to_string(),
            Some("/workspace/tsconfig.app.json".to_string()),
        )]);
    let host = VerterHost::new_standalone(HostConfig::default());

    // "/workspace/src/weird.vue.tsx" has no backing "/workspace/src/weird.vue"
    // registered in any project, so the resolver should not strip the suffix
    assert_eq!(
        source_id_from_provider_carrier_path(&resolver, &host, "/other/weird.vue.tsx"),
        None,
        ".vue.tsx with no backing .vue in any project should return None"
    );
}

#[test]
fn vue_tsx_virtual_file_resolves() {
    // A virtual .vue.tsx with a backing .vue source registered in a project.
    let resolver =
        verter_resolution::ModuleResolverCore::new(vec![verter_workspace::ide_project_config(
            "/workspace".to_string(),
            "/workspace".to_string(),
            Some("/workspace/tsconfig.app.json".to_string()),
        )]);
    // Host must have the backing .vue source for the collision guard to pass
    let host = VerterHost::new_standalone(HostConfig::default());
    host.upsert(verter_session::UpsertRequest {
        canonical_id: Some("/workspace/src/App.vue".to_string()),
        input_id: "/workspace/src/App.vue".to_string(),
        source: "<template><div/></template>".into(),
        file_language: verter_session::FileLanguage::vue(),
        aliases: Vec::new(),
    })
    .unwrap();

    assert_eq!(
        source_id_from_provider_carrier_path(&resolver, &host, "/workspace/src/App.vue.tsx")
            .as_deref(),
        Some("/workspace/src/App.vue"),
        "virtual .vue.tsx with backing .vue source should resolve to .vue"
    );
}

#[test]
fn build_workspace_components_enumerates_svelte_and_strips_extension() {
    // Component auto-import must enumerate `.svelte` carriers and
    // derive the PascalCase component name via the registry-backed strip
    // (`MyButton.svelte` → `MyButton`). A plain `.ts` is NOT a carrier and is
    // excluded. Discrimination: pre-change (`!kind.is_vue()`) the Svelte
    // component is skipped and never appears.
    let host = VerterHost::new_standalone(HostConfig::default());
    host.upsert(verter_session::UpsertRequest {
        canonical_id: Some("/workspace/src/MyButton.svelte".to_string()),
        input_id: "/workspace/src/MyButton.svelte".to_string(),
        source: "<script>let x = 1;</script>".into(),
        file_language: verter_session::FileLanguage::svelte(),
        aliases: Vec::new(),
    })
    .unwrap();
    host.upsert(verter_session::UpsertRequest {
        canonical_id: Some("/workspace/src/util.ts".to_string()),
        input_id: "/workspace/src/util.ts".to_string(),
        source: "export const x = 1;".into(),
        file_language: verter_session::FileLanguage::script_ts(),
        aliases: Vec::new(),
    })
    .unwrap();

    // A dotted filename stem (`Model.Named.vue`) must sanitize to a VALID JS
    // identifier (`ModelNamed`) — the `.` is a word separator feeding PascalCase,
    // NOT a literal character carried into the component name. Pre-fix this
    // yielded `Model.Named`, an invalid identifier that produced the syntax error
    // `import Model.Named from './Model.Named.vue'`.
    host.upsert(verter_session::UpsertRequest {
        canonical_id: Some("/workspace/src/Model.Named.vue".to_string()),
        input_id: "/workspace/src/Model.Named.vue".to_string(),
        source: "<script setup lang=\"ts\"></script>".into(),
        file_language: verter_session::FileLanguage::vue(),
        aliases: Vec::new(),
    })
    .unwrap();
    // Parity: the same sanitization must hold for the Svelte carrier.
    host.upsert(verter_session::UpsertRequest {
        canonical_id: Some("/workspace/src/Dotted.Svelte.Name.svelte".to_string()),
        input_id: "/workspace/src/Dotted.Svelte.Name.svelte".to_string(),
        source: "<script>let x = 1;</script>".into(),
        file_language: verter_session::FileLanguage::svelte(),
        aliases: Vec::new(),
    })
    .unwrap();

    let components = build_workspace_components(&host, "/workspace/src/App.svelte");
    let names: Vec<&str> = components.iter().map(|c| c.name.as_str()).collect();
    assert!(
        names.contains(&"MyButton"),
        "Svelte component must be enumerated with the stripped PascalCase name, got: {names:?}"
    );
    assert!(
        !names.iter().any(|n| n.contains("util")),
        "a plain .ts file is NOT a carrier and must be excluded, got: {names:?}"
    );
    assert!(
        !names.iter().any(|n| n.contains(".svelte")),
        "the carrier extension must be stripped from the component name, got: {names:?}"
    );
    // Dotted Vue stem → valid identifier (discriminating: pre-fix `Model.Named`).
    assert!(
        names.contains(&"ModelNamed"),
        "a dotted Vue carrier stem `Model.Named.vue` must sanitize to the valid \
         identifier `ModelNamed`, got: {names:?}"
    );
    // Dotted Svelte stem → valid identifier (parity).
    assert!(
        names.contains(&"DottedSvelteName"),
        "a dotted Svelte carrier stem must sanitize to the valid identifier \
         `DottedSvelteName`, got: {names:?}"
    );
    // Negative: NO derived component name may contain a `.` — every name must be
    // a valid JS identifier usable verbatim as both the tag and import binding.
    assert!(
        !names.iter().any(|n| n.contains('.')),
        "no derived component name may contain a `.` (invalid identifier), got: {names:?}"
    );
    // Non-dotted names must NOT regress.
    assert!(
        names.contains(&"MyButton"),
        "non-dotted PascalCase stem must remain unchanged, got: {names:?}"
    );
}

#[test]
fn build_workspace_components_sanitizes_dotted_and_special_chars() {
    // Identifier-formatting contract for `build_workspace_components`:
    // - `.` and any char outside `[A-Za-z0-9_$]` act as word separators feeding
    //   PascalCase (`Model.Named` → `ModelNamed`, `my-comp` → `MyComp`).
    // - `_` is a valid identifier char but ALSO behaves as a separator (existing
    //   kebab/snake behavior is preserved: `my_comp` → `MyComp`).
    // - `$` is a valid identifier char and is NOT a separator: it is kept verbatim
    //   (`My$Comp` → `My$Comp`).
    // - A name that would otherwise begin with a digit is prefixed with `_`.
    let host = VerterHost::new_standalone(HostConfig::default());
    for stem in [
        "Model.Named",
        "my-comp",
        "2cool",
        "weird@name!here",
        "MyComponent",
    ] {
        host.upsert(verter_session::UpsertRequest {
            canonical_id: Some(format!("/ws/src/{stem}.vue")),
            input_id: format!("/ws/src/{stem}.vue"),
            source: "<script setup lang=\"ts\"></script>".into(),
            file_language: verter_session::FileLanguage::vue(),
            aliases: Vec::new(),
        })
        .unwrap();
    }

    let components = build_workspace_components(&host, "/ws/src/App.vue");
    let names: Vec<&str> = components.iter().map(|c| c.name.as_str()).collect();

    assert!(names.contains(&"ModelNamed"), "got: {names:?}");
    assert!(names.contains(&"MyComp"), "got: {names:?}");
    // Leading-digit guard: `2cool` → `_2cool` (PascalCase capitalizes nothing
    // before the digit, so the `_` guard is the only mutation).
    assert!(names.contains(&"_2cool"), "got: {names:?}");
    // Non-identifier chars are separators: `weird@name!here` → `WeirdNameHere`.
    assert!(names.contains(&"WeirdNameHere"), "got: {names:?}");
    assert!(names.contains(&"MyComponent"), "got: {names:?}");
    // Every derived name is a valid JS identifier start + body.
    for n in &names {
        let mut chars = n.chars();
        let first = chars.next().expect("non-empty name");
        assert!(
            first.is_ascii_alphabetic() || first == '_' || first == '$',
            "name {n:?} must start with a valid identifier char, got: {names:?}"
        );
        assert!(
            n.chars()
                .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '$'),
            "name {n:?} must contain only valid identifier chars, got: {names:?}"
        );
    }
}

#[test]
fn build_workspace_components_preserves_dollar_and_skips_non_ascii_only() {
    // End-to-end through `build_workspace_components`: a `$`-bearing carrier
    // keeps its `$`; a non-ASCII-only stem is SKIPPED (empty sanitized name).
    let host = VerterHost::new_standalone(HostConfig::default());
    for stem in ["My$Comp", "$Leading", "日本語", "MyButton"] {
        host.upsert(verter_session::UpsertRequest {
            canonical_id: Some(format!("/ws/src/{stem}.vue")),
            input_id: format!("/ws/src/{stem}.vue"),
            source: "<script setup lang=\"ts\"></script>".into(),
            file_language: verter_session::FileLanguage::vue(),
            aliases: Vec::new(),
        })
        .unwrap();
    }

    let components = build_workspace_components(&host, "/ws/src/App.vue");
    let names: Vec<&str> = components.iter().map(|c| c.name.as_str()).collect();

    assert!(
        names.contains(&"My$Comp"),
        "`$` must be preserved, got: {names:?}"
    );
    assert!(
        names.contains(&"$Leading"),
        "a leading `$` carrier must enumerate, got: {names:?}"
    );
    assert!(names.contains(&"MyButton"), "got: {names:?}");
    // The non-ASCII-only carrier sanitizes to `""` and is SKIPPED — no empty or
    // invalid name leaks into the component list.
    assert!(
        !names.iter().any(|n| n.is_empty()),
        "no empty component name may be emitted, got: {names:?}"
    );
    for n in &names {
        assert!(
            n.chars()
                .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '$'),
            "every emitted name must be a valid identifier, got: {names:?}"
        );
    }
}

#[test]
fn build_workspace_components_dotted_collision_is_deterministic() {
    // Collision case: `Model.Named.vue` and `ModelNamed.vue` BOTH sanitize to
    // `ModelNamed`. `host.list_files()` is DashMap-backed (non-deterministic
    // iteration order), so without a tie-break the candidate ORDER — and the
    // collision winner the downstream label-dedup keeps — would flap run-to-run.
    // `build_workspace_components` sorts by canonical file id, so the order and
    // the collision winner are STABLE. This test asserts DETERMINISM: repeated
    // calls yield byte-identical `(name, import_path)` sequences, the order is
    // canonical-path-sorted, and the lexicographically-first canonical path
    // (`Model.Named.vue` < `ModelNamed.vue`) is the first `ModelNamed` candidate.
    let host = VerterHost::new_standalone(HostConfig::default());
    host.upsert(verter_session::UpsertRequest {
        canonical_id: Some("/ws/src/Model.Named.vue".to_string()),
        input_id: "/ws/src/Model.Named.vue".to_string(),
        source: "<script setup lang=\"ts\"></script>".into(),
        file_language: verter_session::FileLanguage::vue(),
        aliases: Vec::new(),
    })
    .unwrap();
    host.upsert(verter_session::UpsertRequest {
        canonical_id: Some("/ws/src/ModelNamed.vue".to_string()),
        input_id: "/ws/src/ModelNamed.vue".to_string(),
        source: "<script setup lang=\"ts\"></script>".into(),
        file_language: verter_session::FileLanguage::vue(),
        aliases: Vec::new(),
    })
    .unwrap();

    // Run the enumeration several times; every run must produce byte-identical
    // output (name + path, in the same order).
    let baseline: Vec<(String, String)> = build_workspace_components(&host, "/ws/src/App.vue")
        .into_iter()
        .map(|c| (c.name, c.import_path))
        .collect();
    for _ in 0..8 {
        let again: Vec<(String, String)> = build_workspace_components(&host, "/ws/src/App.vue")
            .into_iter()
            .map(|c| (c.name, c.import_path))
            .collect();
        assert_eq!(
            again, baseline,
            "enumeration must be deterministic run-to-run, got {again:?} vs {baseline:?}"
        );
    }

    let model_named: Vec<&str> = baseline
        .iter()
        .filter(|(name, _)| name == "ModelNamed")
        .map(|(_, path)| path.as_str())
        .collect();
    // Both carriers surface under the same sanitized name, each preserving its
    // OWN real import path (the path is never sanitized).
    assert_eq!(
        model_named.len(),
        2,
        "both colliding carriers must be enumerated, got paths: {model_named:?}"
    );
    // DETERMINISTIC WINNER: candidates are ordered by canonical path, so the
    // lexicographically-first path `…/Model.Named.vue` is the FIRST `ModelNamed`
    // candidate (the one the downstream first-wins label dedup keeps).
    assert!(
        model_named[0].ends_with("Model.Named.vue"),
        "the canonical-path-first carrier must be the first collision candidate, got: {model_named:?}"
    );
    assert!(
        model_named[1].ends_with("ModelNamed.vue"),
        "the second collision candidate must be the later canonical path, got: {model_named:?}"
    );
}

// @ai-generated
#[tokio::test(flavor = "multi_thread")]
async fn permanent_child_projection_failure_settles_until_its_provenance_changes() {
    const MALFORMED_CHILD: &str = r#"<script setup lang="ts" attrs="Attrs.">
import type { Attrs } from './types'
</script><template/>"#;
    const REPAIRED_CHILD: &str = r#"<script setup lang="ts">
defineProps<{ title?: string }>()
</script><template/>"#;
    const PARENT: &str = r#"<script setup lang="ts">
import Child from './Child.vue'
</script><template><Child /></template>"#;

    let (_temp, service, drain_handle, _provider, workspace_id) =
        make_definition_test_server_with_config(
            &[
                ("src/Child.vue", "vue", MALFORMED_CHILD),
                ("src/App.vue", "vue", PARENT),
            ],
            crate::TypeProviderKind::None,
            HostConfig {
                analysis_scope: Some(verter_semantic::analysis::AnalysisScope::BUILD),
                ..HostConfig::default()
            },
            false,
        )
        .await;
    let server = service.inner();
    let app_uri = workspace_uri(&workspace_id, "src/App.vue");
    let child_uri = workspace_uri(&workspace_id, "src/Child.vue");
    let child_id = format!("{workspace_id}/src/Child.vue");

    server.publish_import_dependencies_settled(&app_uri).await;
    assert!(server.child_public_contract_is_settled(&child_id));
    assert!(server.cached_child_public_contract(&child_id).is_none());
    assert!(server.dependency_readiness_capture(&app_uri).is_ready());
    let projection_count = server.child_public_contract_projection_count_for_test();

    server.publish_import_dependencies_settled(&app_uri).await;
    assert_eq!(
        server.child_public_contract_projection_count_for_test(),
        projection_count,
        "a provenance-current permanent failure must not be projected again"
    );

    assert!(
        server
            .documents
            .did_change(&child_uri, 2, REPAIRED_CHILD)
            .changed
    );
    server.publish_import_dependencies_settled(&app_uri).await;
    assert!(
        server.cached_child_public_contract(&child_id).is_some(),
        "editing the malformed child must invalidate the failure and allow recovery"
    );
    assert!(
        server.child_public_contract_projection_count_for_test() > projection_count,
        "the changed provenance must run a fresh projection"
    );

    drain_handle.abort();
    drop(service);
}

/// W02 discriminator (Vue, both provider kinds): a carrier opened MALFORMED
/// (no projection), then FIXED by a commit that compiles nothing, must get a
/// real provider-backed hover on the FIRST interactive request — with the
/// background coordinator structurally excluded (see the section comment).
///
/// `.length` is a member only the provider can answer (the native hover lane
/// has no member types), so the assertion is observable strictly through the
/// provider rail: pre-change the projection-less document fails the surface
/// capture, the provider is never queried, and hover is None.
#[tokio::test(flavor = "multi_thread")]
async fn projectionless_carrier_heals_on_the_first_interactive_request() {
    let broken_source = "<script setup lang=\"ts\">\nconst broken = (((\n";
    let fixed_source = "<script setup lang=\"ts\">\nconst healedTarget: string = \"ok\"\n</script>\n<template><div>{{ healedTarget.length }}</div></template>\n";
    for kind in [
        crate::TypeProviderKind::Tsserver,
        crate::TypeProviderKind::Tsgo,
    ] {
        let (tsx_path, tsx_offset, position) = provider_target_via_twin_server(
            kind,
            "/workspace/src/App.vue",
            "vue",
            fixed_source,
            ".length",
            1,
        );

        let provider = Arc::new(MockTypeProvider::new());
        let type_provider: Arc<dyn TypeProvider> = provider.clone();
        let service = make_hover_test_service_with_kind(type_provider, kind);
        let server = service.inner();
        install_test_resolver(server);

        // Opened malformed: the open-time compile fails, so no projection.
        let uri = open_test_vue(server, "/workspace/src/App.vue", broken_source);
        assert!(
            server.documents.get_projection(&uri).is_none(),
            "{kind}: precondition — the malformed open must leave the carrier \
             projection-less, or this test exercises nothing"
        );

        // The user fixes the file. The REGISTRY commit stores text and compiles
        // nothing — and sends NO coordinator signal, so no background path can
        // install the projection before the interactive request below.
        let _ = server.documents.did_change(&uri, 2, fixed_source);
        assert!(
            server.documents.get_projection(&uri).is_none(),
            "{kind}: the commit must not compile — if it did, the per-keystroke \
             compile is back and this asserts nothing about interactive repair"
        );

        provider.set_hover(
            &tsx_path,
            tsx_offset,
            Some(HoverInfo {
                contents: "(property) String.length: number".to_string(),
                display_signature: Some(crate::type_provider::mock::test_display_signature(
                    "(property) String.length: number",
                )),
                ..Default::default()
            }),
        );

        let hover = server
            .hover(hover_params(&uri, position))
            .await
            .expect("hover request must not error to the client");
        assert!(
            hover.is_some(),
            "{kind}: the FIRST interactive request after the fix must heal the \
             missing projection and answer through the provider — None means the \
             document stayed dark until a background tick"
        );
        let text = hover_text(hover);
        assert!(
            text.contains("String.length"),
            "{kind}: the healed hover must be the provider answer, got: {text}"
        );
        assert!(
            server.documents.get_projection(&uri).is_some(),
            "{kind}: the interactive repair must have installed the projection"
        );
        let hover_calls = provider
            .calls()
            .iter()
            .filter(|call| matches!(call, MockCall::GetHover { .. }))
            .count();
        assert!(
            hover_calls >= 1,
            "{kind}: the answer must have come through the provider rail"
        );
    }
}

/// W02 Svelte coverage: the interactive projection heal is carrier-generic.
///
/// Svelte's IDE lowering is ERROR-RECOVERING — every malformed fixture tried
/// (unterminated script/tag/expression/block, legacy+runes conflicts,
/// double `$props()`, …) still compiles an IDE surface — so the Vue tests'
/// "broken open" construction cannot reach the projection-less state for a
/// `.svelte` carrier. The state Svelte DOES occupy in production is the
/// startup race documented on `DocumentRegistry::get_ide`: `did_open` runs
/// before the host can serve the compile, so the document carries NO
/// projection while the host artifact may exist. `clear_projection_for_test`
/// reconstructs exactly that state on an otherwise pristine server (no
/// committed sync state, no recorded provider surface), so the heal must run
/// the FULL repair — recompile → gateway → sync → surface record → commit —
/// not merely stuff a mapper onto the document. One provider kind suffices
/// (the repair path is kind-agnostic above the sync verbs; the Vue
/// discriminator covers both kinds).
#[tokio::test(flavor = "multi_thread")]
async fn projectionless_svelte_carrier_heals_on_the_first_interactive_request() {
    let fixed_source = "<script lang=\"ts\">\nconst healedTarget: string = \"ok\";\n</script>\n<p>{healedTarget.length}</p>\n";
    let kind = crate::TypeProviderKind::Tsgo;
    let (tsx_path, tsx_offset, position) = provider_target_via_twin_server(
        kind,
        "/workspace/src/App.svelte",
        "svelte",
        fixed_source,
        ".length",
        1,
    );

    let provider = Arc::new(MockTypeProvider::new());
    let type_provider: Arc<dyn TypeProvider> = provider.clone();
    let service = make_hover_test_service_with_kind(type_provider, kind);
    let server = service.inner();
    install_test_resolver(server);

    let uri: Uri = "file:///workspace/src/App.svelte".parse().expect("uri");
    let _ = server.documents.did_open(&TextDocumentItem {
        uri: uri.clone(),
        language_id: "svelte".to_string(),
        version: 1,
        text: fixed_source.to_string(),
    });
    // Reconstruct the startup-race state: the open document has NO projection
    // (as if did_open ran before the host could compile). Nothing else is
    // seeded — no committed sync state, no recorded surface — so a cache-only
    // mapper install could NOT make the capture below succeed.
    //
    // The repair lane admits this projection-less carrier and coalesces
    // concurrent callers without retaining failure across later requests.
    server.documents.clear_projection_for_test(&uri);
    assert!(
        server.documents.get_projection(&uri).is_none(),
        "precondition: the Svelte carrier must be projection-less at the moment \
         of the interactive request"
    );
    assert!(
        server.current_file_needs_inline_type_provider_sync(&uri),
        "precondition: the reconstructed startup-race state must admit the \
         interactive repair"
    );

    provider.set_hover(
        &tsx_path,
        tsx_offset,
        Some(HoverInfo {
            contents: "(property) String.length: number".to_string(),
            display_signature: Some(crate::type_provider::mock::test_display_signature(
                "(property) String.length: number",
            )),
            ..Default::default()
        }),
    );

    let hover = server
        .hover(hover_params(&uri, position))
        .await
        .expect("hover request must not error to the client");
    assert!(
        hover.is_some(),
        "the FIRST interactive request after the fix must heal the missing \
         Svelte projection and answer through the provider"
    );
    let text = hover_text(hover);
    assert!(
        text.contains("String.length"),
        "the healed Svelte hover must be the provider answer, got: {text}"
    );
    assert!(
        server.documents.get_projection(&uri).is_some(),
        "the interactive repair must have installed the Svelte projection"
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn drain_keeps_partially_failed_vue_file_queued_for_retry() {
    // R2-6: a per-file sync that PARTIALLY succeeds (one kind syncs, another
    // fails and reverts) must NOT be dequeued — the failed kind would otherwise
    // never be retried (permanent suppression). Here the API `.vue.ts` sync is
    // injected to fail while the IDE `.tsx` succeeds; the file must STAY queued.
    let host = Arc::new(VerterHost::new_standalone(HostConfig::default()));
    let documents = DocumentRegistry::new(Arc::clone(&host));
    let uri: Uri = "file:///workspace/src/App.vue".parse().unwrap();
    let _ = documents.did_open(&TextDocumentItem {
        uri: uri.clone(),
        language_id: "vue".to_string(),
        version: 1,
        text: "<template><div /></template>".to_string(),
    });

    let provider = Arc::new(MockTypeProvider::new());
    // Fail ONLY the API `.vue.ts` sync; the IDE `.tsx` succeeds → PARTIAL.
    provider.set_fail_sync_path("/workspace/src/App.vue.verter.ts");
    let sync = ProjectSync::new(provider.clone(), ProjectSyncMode::FullProject);
    let vfs_workspace = crate::test_utils::make_test_vfs_workspace_with_resolver(
        "/workspace",
        Some("/workspace/tsconfig.app.json"),
    );
    let provider_sync_states = DashMap::new();
    let pending_snapshot_provider_sync = DashSet::new();
    pending_snapshot_provider_sync.insert("/workspace/src/App.vue".to_string());

    drain_pending_snapshot_provider_sync(
        Some(&sync),
        &documents,
        &vfs_workspace,
        &provider_sync_states,
        &pending_snapshot_provider_sync,
        false,
        None,
        None,
        &crate::external_ts::CarrierTransactionCoordinator::new(),
    )
    .await;

    // Discriminator (RED pre-fix): the partial success returned `true`, so the
    // drain removed the file and the failed API kind was never retried.
    assert!(
        pending_snapshot_provider_sync.contains("/workspace/src/App.vue"),
        "a partially-failed Vue sync must STAY queued so the failed kind is retried"
    );
    // Positive: the IDE `.tsx` kind DID sync this pass (the partial success).
    let calls = provider.file_sync_calls();
    assert!(
        calls.iter().any(|call| matches!(
            call,
            MockCall::OpenFile { path, .. } | MockCall::UpdateFile { path, .. }
            if path == "/workspace/src/App.vue.tsx"
        )),
        "the IDE TSX kind should have synced (the partial success), calls={calls:?}"
    );
    // Positive: the API `.vue.ts` was attempted (recorded before the injected Err).
    assert!(
        calls.iter().any(|call| matches!(
            call,
            MockCall::OpenFile { path, .. }
                | MockCall::UpdateFile { path, .. }
                | MockCall::LoadFile { path, .. }
            if path == "/workspace/src/App.vue.verter.ts"
        )),
        "the API `.vue.ts` kind should have been attempted (then failed), calls={calls:?}"
    );
}

// @ai-generated - Exact configured ownership, including genuine overlap, owns
// the allowImportingTsExtensions barrel policy.
#[test]
fn tsserver_authored_specifier_policy_is_project_exact_and_all_owner() {
    use verter_session_query::resolution::ProjectId;
    use verter_workspace::workspace_snapshot::{
        OwnershipProject, ProjectPayload, SnapshotGeneration,
    };

    fn configured_project(
        id: u32,
        root: &str,
        tsconfig: &str,
        allow_importing_ts_extensions: bool,
    ) -> OwnershipProject {
        let root = verter_workspace::CanonicalPath::new(root);
        OwnershipProject {
            id: ProjectId(id),
            root: root.clone(),
            workspace_root: verter_workspace::CanonicalPath::new("/workspace"),
            payload: ProjectPayload::Configured {
                tsconfig_path: verter_workspace::CanonicalPath::new(tsconfig),
                membership: verter_session_query::resolution::ConfiguredMembership {
                    spec: verter_session_query::resolution::StaticMembershipSpec {
                        files: Vec::new(),
                        include: vec![verter_session_query::resolution::CompiledGlob::new(
                            verter_session_query::resolution::NormalizedGlob::from_root_and_pattern(
                                &root, "**/*",
                            ),
                        )],
                        exclude: Arc::from([]),
                    },
                    materialized_files: Default::default(),
                },
                compiler_options: verter_session_query::resolution::IdeProjectCompilerOptions {
                    allow_importing_ts_extensions,
                    ..Default::default()
                },
                references: Vec::new(),
                workspace_aliases: Vec::new(),
            },
        }
    }

    let adjacent = verter_workspace::WorkspaceSnapshot {
        owners_memo: Default::default(),
        projects: vec![
            configured_project(0, "/workspace/a", "/workspace/a/tsconfig.json", true),
            configured_project(1, "/workspace/b", "/workspace/b/tsconfig.json", false),
        ],
        resolver: verter_resolution::ModuleResolverCore::new(Vec::new()),
        generation: SnapshotGeneration(1),
    };
    assert!(
        crate::carrier_provider_projection::configured_owners_allow_authored_carrier_specifiers(
            &adjacent,
            "/workspace/a/App.vue"
        ),
        "the true adjacent project must keep authored carrier specifiers"
    );
    assert!(
        !crate::carrier_provider_projection::configured_owners_allow_authored_carrier_specifiers(
            &adjacent,
            "/workspace/b/App.svelte"
        ),
        "the false adjacent project must retain compatibility rewrites"
    );

    let overlapping = verter_workspace::WorkspaceSnapshot {
        owners_memo: Default::default(),
        projects: vec![
            configured_project(0, "/workspace", "/workspace/tsconfig.a.json", true),
            configured_project(1, "/workspace", "/workspace/tsconfig.b.json", false),
        ],
        resolver: verter_resolution::ModuleResolverCore::new(Vec::new()),
        generation: SnapshotGeneration(1),
    };
    assert!(
        !crate::carrier_provider_projection::configured_owners_allow_authored_carrier_specifiers(
            &overlapping,
            "/workspace/App.vue"
        ),
        "one non-true co-owner must keep the single shared buffer compatible"
    );
}

/// Style root access under bound `withDefaults`: `<style>` `v-bind(props.color)`
/// references the bound props ROOT — member-level liveness cannot be bounded,
/// so the whole unused-prop kind suppresses (fail-open). The anti-silencing
/// control is the paired file without the `<style>` trigger: the SAME
/// component must still flag its genuinely-unused props.
#[test]
fn with_defaults_bound_props_style_root_vbind_suppresses() {
    let dir = tempfile::tempdir().unwrap();
    let source = "<script setup lang=\"ts\">\n\
                  const props = withDefaults(defineProps<{ color: string; unusedProp: number }>(), { color: 'red', unusedProp: 0 });\n\
                  </script>\n\
                  \n\
                  <template>\n\
                  <div class=\"x\">t</div>\n\
                  </template>\n\
                  \n\
                  <style>\n\
                  .x { color: v-bind(props.color); }\n\
                  </style>\n";
    let file = dir.path().join("WithDefaultsStyleVBind.vue");
    std::fs::write(&file, source).unwrap();

    let host = crate::test_utils::make_filesystem_test_host(dir.path());
    let documents = Arc::new(DocumentRegistry::new(Arc::clone(&host)));
    let uri =
        crate::uri::path_to_file_uri(&file.to_string_lossy().replace('\\', "/")).expect("uri");
    let _ = documents.did_open(&TextDocumentItem {
        uri: uri.clone(),
        language_id: "vue".to_string(),
        version: 1,
        text: source.to_string(),
    });

    let cached_verter_diags = Arc::new(DashMap::new());
    let diags =
        crate::server::document_diagnostics_for_test(&documents, &uri, &cached_verter_diags, None);

    assert!(
        !diags.iter().any(|diag| matches!(
            diag.code.as_ref(),
            Some(NumberOrString::String(code)) if code == "verter/no-unused-props"
        )),
        "style `v-bind(props.color)` on the bound withDefaults root must suppress every \
         unused-prop diagnostic (root liveness cannot be bounded), got: {diags:?}"
    );

    // Anti-silencing control: the SAME component without the `<style>`
    // trigger — neither prop is read anywhere, so BOTH must surface.
    let control_source = "<script setup lang=\"ts\">\n\
                  const props = withDefaults(defineProps<{ color: string; unusedProp: number }>(), { color: 'red', unusedProp: 0 });\n\
                  </script>\n\
                  \n\
                  <template>\n\
                  <div class=\"x\">t</div>\n\
                  </template>\n";
    let control_file = dir.path().join("WithDefaultsStyleVBindControl.vue");
    std::fs::write(&control_file, control_source).unwrap();
    let control_uri =
        crate::uri::path_to_file_uri(&control_file.to_string_lossy().replace('\\', "/"))
            .expect("uri");
    let _ = documents.did_open(&TextDocumentItem {
        uri: control_uri.clone(),
        language_id: "vue".to_string(),
        version: 1,
        text: control_source.to_string(),
    });
    let control_diags = crate::server::document_diagnostics_for_test(
        &documents,
        &control_uri,
        &cached_verter_diags,
        None,
    );
    let control_unused: Vec<_> = control_diags
        .iter()
        .filter(|diag| {
            matches!(
                diag.code.as_ref(),
                Some(NumberOrString::String(code)) if code == "verter/no-unused-props"
            )
        })
        .collect();
    assert_eq!(
        control_unused.len(),
        2,
        "anti-silencing control: without the style trigger both genuinely-unused props \
         must STILL be flagged, got: {control_diags:?}"
    );
    assert!(
        control_unused.iter().any(|d| d.message.contains("color"))
            && control_unused
                .iter()
                .any(|d| d.message.contains("unusedProp")),
        "control must flag exactly `color` and `unusedProp`, got: {control_diags:?}"
    );
}

// ── File watcher helper tests ──────────────────────────────────

#[test]
fn test_is_config_file_positive() {
    assert!(is_config_file("file:///project/tsconfig.json"));
    assert!(is_config_file("file:///project/tsconfig.app.json"));
    assert!(is_config_file("file:///project/tsconfig.node.json"));
    assert!(is_config_file("file:///project/.verterrc.json"));
    assert!(is_config_file("file:///project/vite.config.ts"));
    assert!(is_config_file("file:///project/vite.config.js"));
    assert!(is_config_file("file:///project/vite.config.mjs"));
    assert!(is_config_file("file:///project/vite.config.cjs"));
    assert!(is_config_file("file:///project/vite.config.mts"));
    assert!(is_config_file("file:///project/vite.config.cts"));
    assert!(is_config_file("file:///project/package.json"));
}

#[test]
fn test_is_config_file_negative() {
    assert!(!is_config_file("file:///project/src/App.vue"));
    assert!(!is_config_file("file:///project/src/utils.ts"));
    assert!(!is_config_file("file:///project/src/config.ts"));
    assert!(!is_config_file("file:///project/tsconfig-paths.ts"));
    assert!(!is_config_file("file:///project/my.config.ts"));
    assert!(!is_config_file("file:///project/verterrc.json"));
}

#[test]
fn test_is_config_file_node_modules_excluded() {
    // package.json inside node_modules must NOT trigger registry rebuilds
    assert!(!is_config_file(
        "/projects/myapp/node_modules/@verter/types/package.json"
    ));
    assert!(!is_config_file(
        "/projects/myapp/node_modules/vue/package.json"
    ));
    assert!(!is_config_file(
        "/projects/myapp/node_modules/.pnpm/vue@3.5.0/node_modules/vue/package.json"
    ));
    // tsconfig inside node_modules should also be excluded
    assert!(!is_config_file(
        "/projects/myapp/node_modules/some-lib/tsconfig.json"
    ));
    // But root-level config files still match
    assert!(is_config_file("/projects/myapp/package.json"));
    assert!(is_config_file("/projects/myapp/tsconfig.json"));
}

#[test]
fn test_is_config_file_windows_paths() {
    // Canonical IDs on Windows use forward slashes
    assert!(is_config_file("C:/project/tsconfig.json"));
    assert!(is_config_file("C:/project/package.json"));
    assert!(is_config_file("C:/project/.verterrc.json"));
    assert!(!is_config_file("C:/project/src/App.vue"));
    // Windows node_modules paths
    assert!(!is_config_file(
        "C:/project/node_modules/@verter/types/package.json"
    ));
}

/// The watcher glob is host-derived: it covers every carrier row of the
/// serving host's own composed classification authority, including
/// `.svelte` (a registered carrier). The IDE TSX projection for `.svelte` is
/// a separate vertical, so `resync` still produces no provider sync state for
/// it — the watcher coverage is the load-bearing assertion here.
#[test]
fn test_carrier_watch_glob_covers_registry_carrier_rows() {
    let host = VerterHost::new_standalone(HostConfig::default());
    let glob = crate::capabilities::carrier_watch_glob(host.language_classifier());
    assert_eq!(glob, "**/*.{svelte,vue}");
}

/// Guard `lifecycle_watch_globs_are_descriptor_derived`: the watcher globs are
/// DESCRIPTOR-DERIVED, never hand-listed. The carrier glob comes from the
/// host's composed registry carrier rows; the adapter-module glob comes from
/// the same authority's `adapter_module_extensions()`
/// (`**/*.{svelte.ts,svelte.js}`) — a rune module is NOT a carrier, so its
/// coverage is the dedicated adapter-module glob, NOT the generic
/// `**/*.{ts,tsx,…}` glob (which the assertion proves excludes the rune
/// extensions). Without the dedicated glob the rune module would only be
/// covered incidentally by the generic TS glob (the S2a P1 gap).
#[test]
fn lifecycle_watch_globs_are_descriptor_derived() {
    let host = VerterHost::new_standalone(HostConfig::default());
    let framework = host.language_classifier();
    // The adapter-module glob is built from the host's own composed
    // classification authority, covering the registered rune-module
    // extensions in longest-suffix-first row order.
    let adapter_glob = crate::capabilities::adapter_module_watch_glob(framework)
        .expect("the svelte adapter registers rune-module extensions");
    // The adapter-module glob is built from the SAME host-composed authority
    // — not a hand-listed literal.
    let from_registry = framework.adapter_module_extensions();
    assert_eq!(
        adapter_glob,
        format!("**/*.{{{}}}", from_registry.join(",")),
        "the adapter-module glob is descriptor-derived from the host's composed registry"
    );
    let mut sorted = from_registry.clone();
    sorted.sort_unstable();
    assert_eq!(
        sorted,
        vec!["svelte.js", "svelte.ts"],
        "the host's composed registry is the authority for the adapter-module extensions (svelte.ts + svelte.js)"
    );

    // The generic TS/JS glob does NOT carry rune-module coverage: `.svelte.ts`
    // / `.svelte.js` are NOT among the bare TS/JS extensions. (The glob
    // `**/*.{ts,...}` would match `foo.svelte.ts` by suffix, but the
    // classification + the dedicated adapter-module glob are what make the
    // rune-module coverage descriptor-driven and explicit.)
    let generic = "ts,tsx,js,jsx,mts,mjs,cts,cjs";
    assert!(
        !generic
            .split(',')
            .any(|e| e == "svelte.ts" || e == "svelte.js"),
        "rune-module extensions must not be hand-listed in the generic TS/JS glob"
    );
}

/// Open-before-ownership: `did_open` on a `.svelte.ts` rune module makes
/// `provider_projection_context` available at the module's OWN canonical path
/// BEFORE any resolver ownership is published (no `install_test_resolver`). The
/// self-file shadow path does NOT depend on `non_carrier_sync_state_for_source`
/// (which requires ownership), so the own buffer is queryable immediately.
#[tokio::test]
async fn rune_module_queryable_before_resolver_ownership() {
    let provider = Arc::new(MockTypeProvider::new());
    let type_provider: Arc<dyn TypeProvider> = provider.clone();
    let host = Arc::new(VerterHost::new_standalone(HostConfig::default()));
    let host_for_server = Arc::clone(&host);
    let type_provider_for_server = Arc::clone(&type_provider);
    let (service, _socket) = tower_lsp_server::LspService::new(move |_client| {
        VerterLanguageServer::new(
            crate::outbound::Outbound::default(),
            LspConfig {
                host: Arc::clone(&host_for_server),
                type_provider: Some(Arc::clone(&type_provider_for_server)),
                project_sync_mode: crate::ProjectSyncMode::FullProject,
                type_provider_kind: crate::TypeProviderKind::Tsserver,
                type_provider_topology: crate::TypeProviderTopology::ProjectTsserver,
                mcp_port: None,
                type_provider_reason: None,
                type_provider_advisory: None,
                suppress_imported_carrier_prewarm: false,
            },
        )
    });
    let socket = service.inner().outbound().wire();
    let drain = tokio::spawn(async move {
        let mut socket = socket;
        while socket.next().await.is_some() {}
    });
    let server = service.inner();
    // NO install_test_resolver — there is no published ownership snapshot.
    assert!(
        server.published_resolver().is_none(),
        "precondition: no resolver ownership is published"
    );

    let rune_uri: Uri = "file:///workspace/store.svelte.ts".parse().unwrap();
    let _ = server.documents.did_open(&TextDocumentItem {
        uri: rune_uri.clone(),
        language_id: "typescript".to_string(),
        version: 1,
        text: "export const s = $state(0);\n".to_string(),
    });
    // The production `did_open` handler drives the eager shadow sync (which
    // does NOT depend on resolver ownership); this test opens through the
    // registry directly, so drive it here. The query context serves the
    // RECORDED synced surface — a buffer the provider never received is not
    // queryable (fail closed).
    assert!(
        server.sync_self_file_shadow_unresolved(&rune_uri).await,
        "the ownership-independent shadow sync should succeed against the mock provider"
    );

    // The own buffer is queryable at its OWN canonical path before ownership.
    let ctx = server
        .provider_projection_context(&rune_uri)
        .expect("the rune module own buffer is queryable before resolver ownership");
    assert_eq!(ctx.provider_path, "/workspace/store.svelte.ts");
    assert!(ctx.provider_content.contains("$state"));

    drain.abort();
}

#[test]
fn declaration_refcount_record_for_released_root_is_reconciled_away() {
    // P1 #2 (race-safe record): if a root closes between the closure snapshot and
    // the per-overlay record, `DeclOverlayOwner::release_root` has ALREADY run for
    // that root — a naive late `insert(root)` would re-add the now-closed
    // root, stranding the overlay with no future close event (a permanent leak).
    //
    // The fix's invariant, exercised here directly on the pure map logic: after a
    // root is released, reconciling the live open-root set down to a set that does
    // NOT contain the released root removes it again, leaving the overlay
    // unreferenced (slot drained → returned for close). I.e. a closed root leaves
    // NOTHING behind even if its record raced the release.
    let owner = DeclOverlayOwner::default();

    // Root A had recorded reachability to an overlay, then closed (released).
    owner.test_seed_slot("/ws/Dep.d.vue.ts", &["/ws/A.vue"], 1);
    let released = owner.release_root("/ws/A.vue");
    // The release returns `DeclCloseTarget` records; assert on the path.
    assert_eq!(
        released
            .iter()
            .map(|t| t.decl_path.clone())
            .collect::<Vec<_>>(),
        vec!["/ws/Dep.d.vue.ts".to_string()],
        "releasing A drains the overlay it solely reached"
    );
    // The drained slot is kept as an empty tombstone (GC'd on a confirmed close),
    // so the reaching-root set is empty even though the slot key persists.
    assert_eq!(
        owner.test_slot_roots("/ws/Dep.d.vue.ts"),
        Some(HashSet::new()),
        "the drained slot is an empty tombstone until the close confirms"
    );

    // RACE: the in-flight closure pass — which snapshotted A as a root BEFORE the
    // close — now performs its late record, re-adding A.
    owner.reconcile_root_reachability("/ws/A.vue", &["/ws/Dep.d.vue.ts".to_string()], 1);
    // Without the race fix this would leave `Dep.d.vue.ts -> {A}` permanently. The
    // reconcile against the LIVE open-root set (A is no longer open ⇒ empty live
    // set) drops A again and returns the overlay for close.
    let now_unreferenced = owner.reconcile_open_roots(&HashSet::new(), 1);
    assert!(
        now_unreferenced
            .iter()
            .any(|t| t.decl_path == "/ws/Dep.d.vue.ts"),
        "a closed root that raced the record leaves NOTHING behind — the overlay is \
         returned for close, got: {now_unreferenced:?}"
    );
    assert!(
        owner
            .test_slots_snapshot()
            .iter()
            .all(|(_, roots)| roots.is_empty()),
        "no stale reaching-root edge remains for the closed root A, slots={:?}",
        owner.test_slots_snapshot()
    );
}

// ── VFS workspace integration ──

#[test]
fn vfs_workspace_rwlock_initially_none() {
    let vfs: Arc<parking_lot::RwLock<Option<Arc<verter_workspace::FilesystemWorkspace>>>> =
        Arc::new(parking_lot::RwLock::new(None));

    // Before background_init, the workspace is None
    assert!(
        vfs.read().is_none(),
        "VFS workspace should be None before initialization"
    );
}

#[test]
fn vfs_workspace_rwlock_install_and_access() {
    let vfs: Arc<parking_lot::RwLock<Option<Arc<verter_workspace::FilesystemWorkspace>>>> =
        Arc::new(parking_lot::RwLock::new(None));

    // Simulate what background_init does: build workspace and store it
    let workspace = Arc::new(verter_workspace::FilesystemWorkspace::new(
        verter_workspace::FilesystemOptions {
            roots: vec!["/test-project".to_string()],
            eager_preload: false,
        },
    ));
    *vfs.write() = Some(Arc::clone(&workspace));

    // Verify it's accessible
    let ws = vfs.read().clone();
    assert!(ws.is_some(), "VFS workspace should be Some after install");

    // Verify workspace options match
    assert_eq!(
        ws.unwrap().options().roots,
        vec!["/test-project".to_string()],
        "workspace roots should match what was installed"
    );
}

#[test]
fn vfs_workspace_with_project_graph() {
    let workspace = Arc::new(verter_workspace::FilesystemWorkspace::new(
        verter_workspace::FilesystemOptions {
            roots: vec!["/my-project".to_string()],
            eager_preload: false,
        },
    ));

    // Before setting a project graph, no fallback owner resolves.
    use verter_workspace::WorkspaceRead;
    assert!(
        workspace
            .published_root()
            .and_then(|root| root
                .snapshot
                .single_fallback_owner_for_file("/my-project/src/App.vue"))
            .is_none(),
        "empty project graph should have no owner"
    );

    // Set a simple project graph
    let graph =
        verter_workspace::ProjectGraph::from_configs(vec![verter_workspace::VfsProjectConfig {
            root: "/my-project".to_string(),
            rank: verter_workspace::ProjectRank::Inferred,
            tsconfig_path: None,
            root_files: vec![],
            extensions: vec![".vue".to_string()],
            workspace_root: "/my-project".to_string(),
            workspace_aliases: vec![],
            compiler_options: Default::default(),
            references: vec![],
            membership: verter_workspace::configured_membership_match_all_under_root(
                &verter_workspace::CanonicalPath::new("/my-project"),
            ),
        }]);
    workspace.set_project_graph(graph);

    // Now the fallback resolution should return the project.
    let root = workspace.published_root().expect("published snapshot");
    let owner = root
        .snapshot
        .single_fallback_owner_for_file("/my-project/src/App.vue");
    assert!(
        owner.is_some(),
        "file under project root should have an owner after graph set"
    );
    assert_eq!(
        root.snapshot.project(owner.unwrap()).root.as_str(),
        "/my-project",
        "owner should be the correct project root"
    );

    // Negative: file outside project root should have no owner
    assert!(
        root.snapshot
            .single_fallback_owner_for_file("/other-project/src/App.vue")
            .is_none(),
        "file outside project root should have no owner"
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn on_file_changed_invalidates_vfs_negative_cache_for_created_file() {
    use verter_workspace::WorkspaceRead;

    let temp = tempfile::tempdir().expect("temp dir");
    let workspace = temp.path().join("workspace");
    let src_dir = workspace.join("src");
    std::fs::create_dir_all(&src_dir).expect("create src dir");

    let root_id = crate::test_utils::canonical_test_path(&workspace);
    let file_id = format!("{root_id}/src/NewFile.vue");
    let file_uri = crate::uri::path_to_file_uri_string(&file_id);

    let provider: Arc<dyn TypeProvider> = Arc::new(MockTypeProvider::new());
    let service = make_hover_test_service(provider);
    let server = service.inner();
    let vfs_workspace = Arc::new(verter_workspace::FilesystemWorkspace::new(
        verter_workspace::FilesystemOptions {
            roots: vec![root_id.clone()],
            eager_preload: false,
        },
    ));
    server.install_vfs_workspace(Arc::clone(&vfs_workspace));

    assert!(
        !vfs_workspace.file_exists(&file_id),
        "missing file should seed a negative dir-index entry"
    );

    std::fs::write(
        workspace.join("src/NewFile.vue"),
        "<template><div/></template>",
    )
    .expect("write new file");

    server
        .on_file_changed(OnFileChangedParams {
            uri: file_uri,
            change_type: "create".to_string(),
        })
        .await;

    assert!(
        vfs_workspace.file_exists(&file_id),
        "create watcher events should invalidate the cached missing sibling result"
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn on_watcher_state_changed_invalidates_vfs_negative_cache_under_workspace_root() {
    use verter_workspace::WorkspaceRead;

    let temp = tempfile::tempdir().expect("temp dir");
    let workspace = temp.path().join("workspace");
    let src_dir = workspace.join("src");
    std::fs::create_dir_all(&src_dir).expect("create src dir");

    let root_id = crate::test_utils::canonical_test_path(&workspace);
    let file_id = format!("{root_id}/src/Recovered.vue");
    let root_uri = crate::uri::path_to_file_uri_string(&root_id);

    let provider: Arc<dyn TypeProvider> = Arc::new(MockTypeProvider::new());
    let service = make_hover_test_service(provider);
    let server = service.inner();
    let vfs_workspace = Arc::new(verter_workspace::FilesystemWorkspace::new(
        verter_workspace::FilesystemOptions {
            roots: vec![root_id.clone()],
            eager_preload: false,
        },
    ));
    server.install_vfs_workspace(Arc::clone(&vfs_workspace));

    assert!(
        !vfs_workspace.file_exists(&file_id),
        "missing file should seed a negative dir-index entry"
    );

    std::fs::write(
        workspace.join("src/Recovered.vue"),
        "<template><span/></template>",
    )
    .expect("write recovered file");

    server
        .on_watcher_state_changed(WatcherStateChangedParams {
            workspace_root: root_uri,
            reason: "overflow".to_string(),
        })
        .await;

    assert!(
        vfs_workspace.file_exists(&file_id),
        "watcher overflow should invalidate cached directory membership under the workspace root"
    );
}

#[test]
fn standalone_host_cannot_resolve_disk_files() {
    let tmp = tempfile::tempdir().expect("temp dir");
    let ws = tmp.path().join("workspace");
    std::fs::create_dir_all(&ws).unwrap();
    std::fs::write(ws.join("App.vue"), "<template><div/></template>").unwrap();

    let host = VerterHost::new_standalone(HostConfig::default());
    let file_id = verter_session_query::resolution::normalize_canonical_id(
        &ws.join("App.vue").to_string_lossy().replace('\\', "/"),
    );
    // Positive: standalone host cannot load disk files (documents the limitation)
    assert!(
        !host.ensure_loaded(&file_id),
        "standalone host should NOT be able to load disk files (no VFS)"
    );
    // Negative: also no analysis available
    assert!(
        host.get_analysis(&file_id).is_none(),
        "standalone host should have no analysis for disk-only files"
    );
}

/// `$/verter/getProjectOverview` counts `.svelte` carriers in the
/// component graph and the carrier-neutral `totalComponentFiles` stat, with
/// the per-file kind discriminant `"component"`. DISCRIMINATING: under the
/// pre-change `is_vue()` gates the Svelte file was neither counted nor kinded
/// as a component.
#[tokio::test]
async fn project_overview_counts_svelte_in_component_graph() {
    let mock = Arc::new(MockTypeProvider::new());
    let service = make_hover_test_service(mock.clone());
    let server = service.inner();
    install_test_resolver(server);

    let child = "<script>let x = 1;</script>";
    let parent = r#"<script>import Child from './Child.svelte';</script>
<Child />
"#;
    open_test_svelte(server, "/workspace/src/Child.svelte", child);
    let parent_uri = open_test_svelte(server, "/workspace/src/App.svelte", parent);
    // A plain .ts is NOT a component carrier.
    let _ = server.documents.did_open(&TextDocumentItem {
        uri: "file:///workspace/src/util.ts".parse().unwrap(),
        language_id: "typescript".to_string(),
        version: 1,
        text: "export const x = 1;".to_string(),
    });

    let overview = server
        .get_project_overview(serde_json::Value::Null)
        .await
        .expect("overview");

    assert!(
        overview.stats.total_component_files >= 2,
        "both .svelte carriers must be counted, got {}",
        overview.stats.total_component_files
    );
    // Every .svelte file kinds as a component; the .ts file does not.
    let svelte_kinds: Vec<&str> = overview
        .files
        .iter()
        .filter(|f| f.path.ends_with(".svelte"))
        .map(|f| f.kind)
        .collect();
    assert!(
        !svelte_kinds.is_empty() && svelte_kinds.iter().all(|k| *k == "component"),
        "every .svelte file must kind as `component`, got {svelte_kinds:?}"
    );
    assert!(
        overview
            .files
            .iter()
            .any(|f| f.path.ends_with("util.ts") && f.kind == "ts"),
        "the plain .ts file must kind as `ts`, not `component`"
    );
    // The parent's template-component edge for the Svelte child appears in the
    // component graph.
    let _ = parent_uri;
    assert!(
        overview
            .component_graph
            .iter()
            .any(|e| e.file.ends_with("App.svelte")),
        "the Svelte parent's component-usage edge must appear in the graph"
    );
}

/// Replacing the VFS workspace must evict the import-set memo.
///
/// The memo key embeds the workspace `content_generation`, which is PER-WORKSPACE:
/// a fresh workspace restarts it low, so an entry minted against the previous
/// workspace can collide with a low generation of the new one and serve a warm
/// skip for a pass that never ran against it. Eviction also bounds the map's
/// growth across a long session, since nothing else ever removes an entry.
#[tokio::test]
async fn swapping_the_vfs_workspace_evicts_the_import_set_memo() {
    let child_source = "<script setup lang=\"ts\">\nconst emit = defineEmits<{ custom: [payload: string] }>()\n</script>\n";
    let parent_source = "<script setup lang=\"ts\">\nimport MyComp from './MyComp.vue'\nfunction handleCustom(payload: string) {}\n</script>\n<template>\n  <MyComp @custom=\"handleCustom\" />\n</template>\n";
    let (_temp, service, drain_handle, _provider, workspace_id) =
        make_definition_test_server_with_kind(
            &[
                ("src/MyComp.vue", "vue", child_source),
                ("src/App.vue", "vue", parent_source),
            ],
            crate::TypeProviderKind::Tsgo,
        )
        .await;

    let app_uri = workspace_uri(&workspace_id, "src/App.vue");
    let server = service.inner();
    let position = find_document_position(server, &app_uri, "@custom=\"handleCustom\"", 1);

    // Mint a receipt through a real request's enqueued background publication.
    let _ = server
        .goto_definition(goto_definition_params(&app_uri, position))
        .await;
    tokio::time::timeout(std::time::Duration::from_secs(5), async {
        let canonical = server
            .documents
            .get(&app_uri)
            .expect("parent stays open")
            .canonical_id
            .clone();
        let key = server
            .import_sync_freshness_key()
            .expect("host generations");
        server.import_sync.wait_fresh_at(&canonical, key).await;
    })
    .await
    .expect(
        "the enqueued publication must actually have recorded a receipt, else this \
         test proves nothing",
    );

    // Replace the workspace exactly as `initialize` does.
    let replacement = Arc::new(verter_workspace::FilesystemWorkspace::new(
        verter_workspace::FilesystemOptions {
            roots: vec![workspace_id.clone()],
            eager_preload: false,
        },
    ));
    replacement.set_project_graph(verter_workspace::ProjectGraph::new());
    server.swap_vfs_workspace(replacement);

    assert_eq!(
        server.import_sync.recorded_len(),
        0,
        "a workspace swap must evict every memo entry: their keys belong to the workspace \
         that was just replaced"
    );

    drain_handle.abort();
    drop(service);
}

/// The case https://github.com/pikax/verter/issues/96 is actually about, at the
/// real ingress: a carrier with NO projection, typed into with revisions that do
/// not compile.
///
/// A commit that compiles "only until the first projection exists" sounds
/// bounded by document. It is not: a failed compile installs no projection, so
/// the next notification compiles again. Malformed intermediate revisions are
/// the normal state of typing, so that reinstates the serialized per-keystroke
/// compile queue in full — reachable without any unusual input.
///
/// Measured on the COLD-RUN rail. The post-success compile tick cannot see this
/// at all: a failing compile returns before that tick, so this burst reads zero
/// there whether it compiled once per notification or never. The companion test
/// `a_burst_of_did_change_notifications_does_not_compile_once_per_notification`
/// covers the established-projection burst.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn an_invalid_burst_on_a_projectionless_carrier_does_not_compile_per_notification() {
    const BURST: usize = 24;
    let host = Arc::new(VerterHost::new_standalone(HostConfig::default()));
    let service = ingress_measurement_server(&host);

    // Open in a state whose IDE projection cannot be built, so every commit
    // below takes the projection-less path.
    let uri = open_test_vue(
        service.inner(),
        "/workspace/src/Invalid.vue",
        "<script setup lang=\"ts\">\nconst broken = (((\n",
    );
    let canonical_id = crate::documents::uri_to_canonical_id(&uri);
    assert!(
        service.inner().documents.get_projection(&uri).is_none(),
        "precondition: the fixture must fail to project, or this burst runs the \
         established-projection path the sibling test already covers"
    );
    let coordinator = service.inner().sync_coordinator.clone();

    let (mut client_to_server, serve) = serve_over_duplex_initialized(service).await;
    // Same deterministic pin as the sibling test: the coordinator cannot
    // dispatch for a canonical id with a change in flight, so the in-flight
    // count below is attributable to the notification path alone.
    let ticket = coordinator.change_received(canonical_id.clone());
    let before = cold_compile_runs(&host);
    let (frames, final_source) = invalid_did_change_burst_frames(&uri, 2, BURST);
    {
        use tokio::io::AsyncWriteExt;
        client_to_server
            .write_all(&frames)
            .await
            .expect("the burst must reach the server");
        client_to_server.flush().await.expect("flush the burst");
    }

    // Fence on the LAST revision being committed. Waiting longer can only let
    // more commit-path compiles land, so it cannot turn a real per-notification
    // compile into a pass.
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(60);
    while host.get_source(&canonical_id).as_deref() != Some(final_source.as_str()) {
        assert!(
            std::time::Instant::now() < deadline,
            "precondition: every queued notification must be committed; the document \
             is still at {:?}",
            host.get_source(&canonical_id)
        );
        tokio::time::sleep(std::time::Duration::from_millis(20)).await;
    }
    let in_flight = cold_compile_runs(&host) - before;

    assert_eq!(
        in_flight, 0,
        "{BURST} malformed didChange notifications on a projection-less carrier \
         started {in_flight} cold compile run(s). Compiling while no projection exists \
         never terminates — the compile fails, no projection is installed, and the \
         next keystroke repeats it. That is the serialized queue of issue #96, \
         reachable by ordinary typing"
    );
    assert!(
        service_projection_still_absent(&host, &canonical_id),
        "the malformed carrier must still have no IDE surface — if one appeared, a \
         compile ran somewhere the rail did not attribute to this burst"
    );

    // Release the ticket. Exactly one debounced compile must follow — the
    // failing revision still owes its DIAGNOSTICS, which the failure arm stores
    // before returning `Err`, and that is the only reason the editor shows the
    // parse errors at all. A `Never` here is the empty-diagnostics regression;
    // a per-notification count is #96.
    drop(ticket);
    let settled = await_settled_cold_compiles(&host, before).await;
    assert_eq!(
        settled, 1,
        "after the burst went quiet the debounced coordinator must compile the \
         settled (still malformed) revision EXACTLY once ({settled} observed)"
    );
    assert!(
        service_projection_still_absent(&host, &canonical_id),
        "the settled revision still does not compile, so the debounced refresh must \
         install NO projection — a projection here means the fixture stopped being \
         malformed and the burst asserted nothing"
    );

    // Positive control: the rail moves for this document once a valid revision is
    // compiled on demand, so the counts above are not a dead counter.
    let before_control = cold_compile_runs(&host);
    let _ = host.upsert(verter_session::UpsertRequest {
        canonical_id: Some(canonical_id.clone()),
        input_id: canonical_id.clone(),
        source: Arc::from(
            "<script setup lang=\"ts\">\nconst fixed = 1\n</script>\n\
             <template><div>{{ fixed }}</div></template>\n",
        ),
        file_language: verter_session::FileLanguage::vue(),
        aliases: vec![],
    });
    let profile = verter_session::CompileProfile::default();
    let _ = host.ensure_ide_compiled(&canonical_id, &profile);
    assert!(
        cold_compile_runs(&host) > before_control,
        "compiling this document on demand must start a cold run — otherwise the zero \
         above is vacuous"
    );
    serve.abort();
}
