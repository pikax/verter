use super::*;

#[test]
fn needs_provider_sync_insert_and_remove() {
    let set = DashSet::new();
    let id = "C:/project/src/App.vue".to_string();
    set.insert(id.clone());
    assert!(set.contains(&id), "should contain the inserted id");
    let removed = set.remove(&id);
    assert!(removed.is_some(), "remove should return Some");
    assert!(!set.contains(&id), "should no longer contain the id");
}

#[test]
fn provider_sync_without_snapshot_is_deferred_not_fallback_rewritten() {
    let source =
            "import Foo from './Foo.vue';\nimport util from './util';\nconst keep = import(`./${name}.vue`);\n";
    let foo_expr = "'./Foo.vue'";
    let util_expr = "'./util'";
    let dynamic_expr = "`./${name}.vue`";
    let foo_start = source.find(foo_expr).unwrap();
    let util_start = source.find(util_expr).unwrap();
    let dynamic_start = source.find(dynamic_expr).unwrap();

    let reader =
        TestResolverReader::with_files(&["/workspace/src/Foo.vue", "/workspace/src/util.ts"]);

    let prepared = prepare_non_carrier_provider_sync(
        &verter_session::framework::HostLanguageClassifier::default(),
        None,
        &reader,
        "/workspace/src/App.ts",
        source,
        &[
            test_module_reference(
                foo_expr,
                Some("./Foo.vue"),
                &[],
                verter_session_query::analysis::types::ModuleReferenceAnalyzability::Exact,
                foo_start,
                foo_start + foo_expr.len(),
            ),
            test_module_reference(
                util_expr,
                Some("./util"),
                &[],
                verter_session_query::analysis::types::ModuleReferenceAnalyzability::Exact,
                util_start,
                util_start + util_expr.len(),
            ),
            test_module_reference(
                dynamic_expr,
                None,
                &["./Foo.vue"],
                verter_session_query::analysis::types::ModuleReferenceAnalyzability::FiniteSet,
                dynamic_start,
                dynamic_start + dynamic_expr.len(),
            ),
        ],
    );
    assert!(
        prepared.is_none(),
        "provider sync should be deferred until a resolver snapshot exists"
    );
}

/// A result-only adapter has no immutable-world witness. Its useful transient
/// target must therefore never become a provider buffer or an exact host route.
#[test]
fn provider_sync_refuses_return_only_resolution_products() {
    let resolver =
        verter_resolution::ModuleResolverCore::new(vec![verter_workspace::ide_project_config(
            "/workspace".to_string(),
            "/workspace".to_string(),
            Some("/workspace/tsconfig.json".to_string()),
        )]);
    let source = "import { value } from './dep';\n";
    let expr = "'./dep'";
    let start = source.find(expr).expect("fixture import");

    let prepared = prepare_non_carrier_provider_sync(
        &verter_session::framework::HostLanguageClassifier::default(),
        Some(&PublishedResolverSnapshot {
            resolver,
            resolution_view: None,
            ownership_ready: true,
        }),
        &ReturnOnlyResolverReader,
        "/workspace/src/App.ts",
        source,
        &[test_module_reference(
            expr,
            Some("./dep"),
            &[],
            verter_session_query::analysis::types::ModuleReferenceAnalyzability::Exact,
            start,
            start + expr.len(),
        )],
    );

    assert!(
        prepared.is_none(),
        "ResolutionUntrackedBackend must stop the whole resolution-derived provider product"
    );

    // Mutation recipe: project the adapter outcome through the transient result
    // path in either rewrite or dependency collection. Preparation becomes Some
    // and the downstream sync/route sinks can publish the unwitnessed target.
}

#[test]
fn provider_sync_with_snapshot_uses_resolved_dependencies_only() {
    let resolver =
        verter_resolution::ModuleResolverCore::new(vec![verter_workspace::ide_project_config(
            "/workspace".to_string(),
            "/workspace".to_string(),
            Some("/workspace/tsconfig.app.json".to_string()),
        )]);
    let reader =
        TestResolverReader::with_files(&["/workspace/src/Foo.vue", "/workspace/src/util.ts"]);
    let source =
            "import Foo from './Foo.vue';\nimport util from './util';\nconst keep = import(`./${name}.vue`);\n";
    let foo_expr = "'./Foo.vue'";
    let util_expr = "'./util'";
    let dynamic_expr = "`./${name}.vue`";
    let foo_start = source.find(foo_expr).unwrap();
    let util_start = source.find(util_expr).unwrap();
    let dynamic_start = source.find(dynamic_expr).unwrap();

    let prepared = prepare_non_carrier_provider_sync(
        &verter_session::framework::HostLanguageClassifier::default(),
        Some(&PublishedResolverSnapshot {
            resolver,
            resolution_view: None,
            ownership_ready: true,
        }),
        &reader,
        "/workspace/src/App.ts",
        source,
        &[
            test_module_reference(
                foo_expr,
                Some("./Foo.vue"),
                &[],
                verter_session_query::analysis::types::ModuleReferenceAnalyzability::Exact,
                foo_start,
                foo_start + foo_expr.len(),
            ),
            test_module_reference(
                util_expr,
                Some("./util"),
                &[],
                verter_session_query::analysis::types::ModuleReferenceAnalyzability::Exact,
                util_start,
                util_start + util_expr.len(),
            ),
            test_module_reference(
                dynamic_expr,
                None,
                &["./Foo.vue", "./util"],
                verter_session_query::analysis::types::ModuleReferenceAnalyzability::FiniteSet,
                dynamic_start,
                dynamic_start + dynamic_expr.len(),
            ),
        ],
    )
    .expect("resolver snapshot should prepare provider sync");

    let resolved_sources = prepared
        .resolved_dependencies
        .iter()
        .map(|entry| entry.source_id.as_str())
        .collect::<Vec<_>>();
    assert_eq!(
        resolved_sources,
        vec!["/workspace/src/Foo.vue", "/workspace/src/util.ts"],
        "exact and finite-set dependencies should resolve through the native resolver"
    );
    assert!(
        prepared
            .resolved_dependencies
            .iter()
            .any(|entry| entry.provider_specifier == "./Foo.vue.verter.ts"),
        "Vue dependencies should target their provider API paths"
    );
    assert!(
        prepared
            .resolved_dependencies
            .iter()
            .any(|entry| entry.provider_specifier == "./util"),
        "non-Vue workspace dependencies should preserve the source import specifier"
    );
    assert!(
        prepared.rewritten.contains("'./Foo.vue.verter.ts'"),
        "exact Vue imports should rewrite through the resolved provider specifier"
    );
    assert!(
        prepared.rewritten.contains("'./util'"),
        "non-Vue workspace imports should stay source-compatible in the provider file"
    );
    assert!(
        prepared.rewritten.contains("import(`./${name}.vue`)"),
        "finite-set dynamics must keep the original expression text"
    );
}

#[test]
fn provider_vue_path_helpers_use_original_paths() {
    let resolver =
        verter_resolution::ModuleResolverCore::new(vec![verter_workspace::ide_project_config(
            "/workspace".to_string(),
            "/workspace".to_string(),
            Some("/workspace/tsconfig.app.json".to_string()),
        )]);

    let ide_path =
        provider_ide_path_for_source(&resolver, "/workspace/src/App.vue", false).unwrap();
    let api_path = provider_api_path_for_source(&resolver, "/workspace/src/App.vue").unwrap();

    assert_eq!(
        ide_path, "/workspace/src/App.vue.tsx",
        "Vue IDE path should be canonical_id.tsx"
    );
    assert_eq!(
        api_path, "/workspace/src/App.vue.verter.ts",
        "Vue API path should be canonical_id.ts"
    );
}

#[test]
fn provider_path_helpers_round_trip_through_resolver() {
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

    let ide_path = provider_ide_path_for_source(&resolver, "/workspace/src/App.vue", true).unwrap();
    let api_path = provider_api_path_for_source(&resolver, "/workspace/src/App.vue").unwrap();

    assert_eq!(
        source_id_from_provider_carrier_path(&resolver, &host, &ide_path).as_deref(),
        Some("/workspace/src/App.vue")
    );
    assert_eq!(
        source_id_from_provider_carrier_path(&resolver, &host, &api_path).as_deref(),
        Some("/workspace/src/App.vue")
    );
}

/// The advertised `resolve_provider` capability is HONEST: it mirrors the
/// `resolve_provider` argument (which the initialize handler derives from the
/// active provider's `supports_completion_resolve`), never a hard-coded `true`.
#[test]
fn resolve_provider_capability_is_honest() {
    let with_resolve = host_server_capabilities(true);
    assert_eq!(
        with_resolve
            .completion_provider
            .as_ref()
            .and_then(|c| c.resolve_provider),
        Some(true),
        "a resolve-capable provider must advertise resolve_provider: true"
    );

    let without_resolve = host_server_capabilities(false);
    assert_eq!(
        without_resolve
            .completion_provider
            .as_ref()
            .and_then(|c| c.resolve_provider),
        Some(false),
        "a session without resolve support must advertise resolve_provider: false, not a \
         dishonest true"
    );
}

#[test]
fn did_open_startup_policy_enables_sync_for_tsgo_and_tsserver() {
    let tsgo = did_open_startup_policy(crate::TypeProviderKind::Tsgo);
    assert!(
        tsgo.sync_imported_carrier_apis,
        "TSGO should eagerly sync imported .vue files"
    );
    assert!(
        !tsgo.publish_diagnostics,
        "should not publish diagnostics inline"
    );

    let tsserver = did_open_startup_policy(crate::TypeProviderKind::Tsserver);
    assert!(
        tsserver.sync_imported_carrier_apis,
        "tsserver should eagerly sync imported .vue files"
    );
    assert!(
        !tsserver.publish_diagnostics,
        "should not publish diagnostics inline"
    );

    let editor_tsserver = did_open_startup_policy(crate::TypeProviderKind::EditorTsserver);
    assert!(
        editor_tsserver.sync_imported_carrier_apis,
        "the editor tsserver plugin still requires imported carrier-store publication"
    );
}

#[test]
fn did_open_provider_sync_policy_skips_api_sync_for_tsserver_but_not_tsgo() {
    let tsserver = did_open_provider_sync_policy(crate::TypeProviderKind::Tsserver);
    assert!(
        tsserver.await_ide_sync,
        "tsserver cold open should still await current-file TSX sync"
    );
    assert!(
        !tsserver.await_api_sync,
        "tsserver cold open should not await current-file .vue.ts sync"
    );

    let tsgo = did_open_provider_sync_policy(crate::TypeProviderKind::Tsgo);
    assert!(
        tsgo.await_api_sync,
        "TSGO cold open should continue awaiting API sync"
    );

    let no_provider = did_open_provider_sync_policy(crate::TypeProviderKind::None);
    assert!(
        no_provider.await_ide_sync,
        "the cold-open policy should keep TSX sync enabled regardless of provider kind"
    );
    assert!(
        !no_provider.await_api_sync,
        "verter-only mode should not await API sync"
    );

    let editor_tsserver = did_open_provider_sync_policy(crate::TypeProviderKind::EditorTsserver);
    assert!(
        !editor_tsserver.await_ide_sync && !editor_tsserver.await_api_sync,
        "an editor-owned server must not be driven through local provider sync"
    );
    assert!(
        editor_tsserver.background_api_sync,
        "an editor-owned server still queues durable carrier-store publication"
    );
}

#[tokio::test]
async fn editor_tsserver_constructs_store_publication_without_a_local_provider() {
    let host = Arc::new(VerterHost::new_standalone(HostConfig::default()));
    let host_for_server = Arc::clone(&host);
    let (service, _socket) = tower_lsp_server::LspService::new(move |_client| {
        VerterLanguageServer::new(
            crate::outbound::Outbound::default(),
            LspConfig {
                host: Arc::clone(&host_for_server),
                type_provider: None,
                project_sync_mode: ProjectSyncMode::FullProject,
                type_provider_kind: crate::TypeProviderKind::EditorTsserver,
                type_provider_topology: crate::TypeProviderTopology::EditorTsserver,
                mcp_port: None,
                type_provider_reason: Some("attested editor project".into()),
                type_provider_advisory: None,
                suppress_imported_carrier_prewarm: false,
            },
        )
    });
    let server = service.inner();

    assert!(server.type_provider.is_none());
    assert!(server.project_sync.is_none());
    // The debounced coordinator is ALWAYS constructed: its publish half
    // carries Verter-owned diagnostics (lint / unused-declaration hints /
    // template errors) on every route — the editor-owned tsserver plugin
    // route has NO in-process provider, yet files opened after init must
    // still receive Verter-owned pushes (the provider-sync half no-ops).
    let _always_present: &crate::sync_coordinator::SyncCoordinatorHandle = &server.sync_coordinator;
    assert!(
        server.carrier_publish_coordinator.is_some(),
        "the editor plugin route requires the durable store publisher"
    );
}

#[tokio::test]
async fn managed_tsgo_constructs_both_direct_provider_sync_and_editor_store_publication() {
    let host = Arc::new(VerterHost::new_standalone(HostConfig::default()));
    let provider: Arc<dyn TypeProvider> = Arc::new(MockTypeProvider::new());
    let host_for_server = Arc::clone(&host);
    let provider_for_server = Arc::clone(&provider);
    let (service, _socket) = tower_lsp_server::LspService::new(move |_client| {
        VerterLanguageServer::new(
            crate::outbound::Outbound::default(),
            LspConfig {
                host: Arc::clone(&host_for_server),
                type_provider: Some(Arc::clone(&provider_for_server)),
                project_sync_mode: ProjectSyncMode::FullProject,
                type_provider_kind: crate::TypeProviderKind::Tsgo,
                type_provider_topology: crate::TypeProviderTopology::ManagedTsgo,
                mcp_port: None,
                type_provider_reason: Some("managed tsgo".into()),
                type_provider_advisory: None,
                suppress_imported_carrier_prewarm: false,
            },
        )
    });
    let server = service.inner();

    assert!(
        server.project_sync.is_some(),
        "managed tsgo must retain its direct companion-buffer delivery leg"
    );
    assert!(
        server.carrier_publish_coordinator.is_some(),
        "the editor plugin also requires durable carrier membership on managed tsgo"
    );
}

#[test]
fn did_open_resolves_carrier_working_set_from_upsert_import_facts() {
    use verter_workspace::{WorkspaceAccess, WorkspaceRead};

    let workspace =
        verter_workspace::MemoryWorkspace::new(verter_workspace::MemoryOptions::default());
    workspace.set_project_graph(verter_workspace::ProjectGraph::from_configs(vec![
        verter_workspace::VfsProjectConfig {
            root: "/workspace".to_string(),
            rank: verter_workspace::ProjectRank::Inferred,
            tsconfig_path: None,
            root_files: Vec::new(),
            extensions: vec![".ts".to_string(), ".vue".to_string(), ".svelte".to_string()],
            workspace_root: "/workspace".to_string(),
            workspace_aliases: Vec::new(),
            compiler_options: Default::default(),
            references: Vec::new(),
            membership: verter_workspace::configured_membership_match_all_under_root(
                &verter_workspace::CanonicalPath::new("/workspace"),
            ),
        },
    ]));
    WorkspaceAccess::set_exact_resolutions(
        &workspace,
        "/workspace/src/App.vue",
        vec![
            ("./Child.vue", "/workspace/src/Child.vue"),
            ("./Panel.svelte", "/workspace/src/Panel.svelte"),
            ("./plain", "/workspace/src/plain.ts"),
        ]
        .into_iter()
        .map(
            |(specifier, resolved_canonical_id)| verter_workspace::ExactResolution {
                specifier: specifier.to_string(),
                phase: verter_session_query::resolution::ResolvePhase::CodegenBlocker,
                kind: verter_session_query::resolution::ResolveRequestKind::EsmImport,
                resolved_canonical_id: Some(resolved_canonical_id.to_string()),
                possible_canonical_ids: vec![resolved_canonical_id.to_string()],
            },
        )
        .collect(),
    );
    let imports = vec![
        verter_session::ScriptImportInfo {
            source: "./Child.vue".to_string(),
            is_type_only: false,
            bindings: vec!["Child".to_string()],
        },
        verter_session::ScriptImportInfo {
            source: "./Panel.svelte".to_string(),
            is_type_only: false,
            bindings: vec!["Panel".to_string()],
        },
        verter_session::ScriptImportInfo {
            source: "./plain".to_string(),
            is_type_only: false,
            bindings: vec!["plain".to_string()],
        },
    ];

    let ids = collect_imported_carrier_priority_ids_from_specifiers_for_publication(
        &verter_session::framework::HostLanguageClassifier::default(),
        &imports,
        Some("/workspace/src/App.vue"),
        |parent, specifier| {
            WorkspaceRead::resolve_import_outcome(
                &workspace,
                parent,
                specifier,
                verter_session_query::resolution::ResolutionContext {
                    phase: verter_session_query::resolution::ResolvePhase::CodegenBlocker,
                    kind: verter_session_query::resolution::ResolveRequestKind::EsmImport,
                },
            )
            .into_publication()
            .map_result(|resolution| resolution.source_id)
        },
    );
    let ids = ids.expect("fixture resolutions should be admitted");

    assert_eq!(
        ids,
        vec![
            "/workspace/src/Child.vue".to_string(),
            "/workspace/src/Panel.svelte".to_string(),
        ]
    );
}

#[test]
fn did_open_prioritizes_exact_and_finite_dynamic_targets() {
    let resolver =
        verter_resolution::ModuleResolverCore::new(vec![verter_workspace::ide_project_config(
            "/workspace".to_string(),
            "/workspace".to_string(),
            Some("/workspace/tsconfig.app.json".to_string()),
        )]);
    let reader = TestResolverReader::with_files(&[
        "/workspace/src/Foo.vue",
        "/workspace/src/Bar.vue",
        "/workspace/src/util.ts",
    ]);
    let targets = collect_priority_carrier_public_api_targets_from_module_references(
        Some(&PublishedResolverSnapshot {
            resolver,
            resolution_view: None,
            ownership_ready: true,
        }),
        &reader,
        "/workspace/src/App.vue",
        &[
            test_analyzed_module_reference(
                "'./Foo.vue'",
                Some("./Foo.vue"),
                &[],
                verter_session_query::analysis::types::ModuleReferenceAnalyzability::Exact,
                0,
                10,
            ),
            test_analyzed_module_reference(
                "`./${name}.vue`",
                None,
                &["./Bar.vue", "./util"],
                verter_session_query::analysis::types::ModuleReferenceAnalyzability::FiniteSet,
                11,
                27,
            ),
        ],
    )
    .expect("memory-backed resolution is publishable");

    assert_eq!(
        targets,
        vec![
            "/workspace/src/Foo.vue".to_string(),
            "/workspace/src/Bar.vue".to_string()
        ]
    );
}

#[test]
fn unknown_dynamic_imports_sync_no_provider_dependencies() {
    let resolver =
        verter_resolution::ModuleResolverCore::new(vec![verter_workspace::ide_project_config(
            "/workspace".to_string(),
            "/workspace".to_string(),
            Some("/workspace/tsconfig.app.json".to_string()),
        )]);
    let reader = TestResolverReader::with_files(&["/workspace/src/Foo.vue"]);
    let targets = collect_priority_carrier_public_api_targets_from_module_references(
        Some(&PublishedResolverSnapshot {
            resolver,
            resolution_view: None,
            ownership_ready: true,
        }),
        &reader,
        "/workspace/src/App.vue",
        &[test_analyzed_module_reference(
            "`./${name}.vue`",
            None,
            &[],
            verter_session_query::analysis::types::ModuleReferenceAnalyzability::UnknownDynamic,
            0,
            15,
        )],
    )
    .expect("unknown dynamic references perform no resolution");

    assert!(
        targets.is_empty(),
        "unknown dynamic imports must not speculate provider dependencies"
    );
}

#[tokio::test]
async fn resolve_component_document_for_usage_follows_barrel_reexports() {
    let child_source =
        "<script setup lang=\"ts\">\nconst emit = defineEmits<{ custom: [] }>()\n</script>\n";
    let barrel_source = "export { default as BarrelComp } from './BarrelComp.vue'\n";
    let parent_source = "<script setup lang=\"ts\">\nimport { BarrelComp } from './components'\n</script>\n<template>\n  <BarrelComp @custom=\"handleCustom\" />\n</template>\n";
    let (_temp, service, drain_handle, _provider, workspace_id) = make_definition_test_server(&[
        ("src/components/BarrelComp.vue", "vue", child_source),
        ("src/components/index.ts", "typescript", barrel_source),
        ("src/App.vue", "vue", parent_source),
    ])
    .await;

    let app_uri = workspace_uri(&workspace_id, "src/App.vue");
    let child_uri = workspace_uri(&workspace_id, "src/components/BarrelComp.vue");
    let server = service.inner();
    let analysis = server
        .documents
        .get_analysis(&app_uri)
        .expect("parent analysis should exist");
    let template = analysis
        .template
        .as_ref()
        .expect("template analysis should exist");
    let component = template
        .components
        .iter()
        .find(|component| component.name == "BarrelComp")
        .expect("template should include BarrelComp usage");

    assert_eq!(
        component.import_source.as_deref(),
        Some("./components"),
        "template component should retain the raw barrel import source"
    );
    assert_eq!(
        server
            .component_import_binding_name(&analysis, component)
            .as_deref(),
        Some("BarrelComp"),
        "named barrel imports should preserve the local component binding name"
    );

    let parent_canonical_id = uri_to_canonical_id(&app_uri);
    let barrel_canonical_id = server
        .resolve_import_specifier_transient(&parent_canonical_id, "./components")
        .expect("barrel import should resolve to a concrete module");

    assert!(
        barrel_canonical_id.ends_with("/src/components/index.ts"),
        "extensionless barrel imports should resolve to index.ts, got {barrel_canonical_id}"
    );
    assert!(
        server
            .documents
            .host()
            .get_export_span_follow_reexports(&barrel_canonical_id, "BarrelComp")
            .is_some(),
        "barrel export should resolve to the re-exported child"
    );

    let child = server
        .resolve_component_document_for_usage(&app_uri, &analysis, component)
        .expect("component usage should resolve through the barrel");

    assert_eq!(
        child.uri, child_uri,
        "barrel should resolve to the child SFC"
    );
    assert!(
        child.analysis.macros.iter().any(|mac| {
            mac.kind == verter_session_query::analysis::types::AnalyzedMacroKind::DefineEmits
                && mac.emit_fields.iter().any(|field| field.name == "custom")
        }),
        "resolved child analysis should expose the child's emit declaration"
    );

    drain_handle.abort();
    drop(service);
}

/// `didChange` commits Vue/Svelte x TS/JS editor bytes without retaining the
/// notification on provider I/O. The debounced coordinator owns the eventual
/// provider refresh.
#[tokio::test(flavor = "multi_thread")]
async fn did_change_acknowledges_before_provider_refresh_for_all_carrier_modes() {
    for case in D1_CARRIER_CASES {
        let child_path = format!("src/DirectComp.{}", case.extension);
        let stale_child_path = format!("src/BeforeComp.{}", case.extension);
        let parent_path = format!("src/App.{}", case.extension);
        let (_temp, service, drain_handle, provider, workspace_id) =
            make_definition_test_server_with_kind(
                &[
                    (child_path.as_str(), case.language_id, case.child_source),
                    (
                        stale_child_path.as_str(),
                        case.language_id,
                        case.stale_child_source,
                    ),
                    (
                        parent_path.as_str(),
                        case.language_id,
                        case.opened_parent_source,
                    ),
                ],
                crate::TypeProviderKind::Tsgo,
            )
            .await;

        let app_uri = workspace_uri(&workspace_id, &parent_path);
        let server = service.inner();
        server.ensure_current_file_synced(&app_uri).await;
        let ide_path = server
            .active_ide_path_for_uri(&app_uri)
            .expect("initial provider surface must be synced");
        provider.clear_calls();
        let (update_arrived, update_release) = provider.block_update_file(&ide_path);

        // `update_file` is blocked indefinitely (never released here) — a
        // handler that incorrectly awaited the provider refresh would hang
        // this forever, so a generous hang-detector timeout discriminates
        // exactly as well as a tight one, without a loaded machine's
        // legitimate (non-provider) latency on the compile path tripping a
        // false failure.
        tokio::time::timeout(std::time::Duration::from_secs(10), async {
            super::super::lifecycle::handle_did_change(
                server,
                DidChangeTextDocumentParams {
                    text_document: VersionedTextDocumentIdentifier {
                        uri: app_uri.clone(),
                        version: 2,
                    },
                    content_changes: vec![TextDocumentContentChangeEvent {
                        range: None,
                        range_length: None,
                        text: case.edited_parent_source.to_string(),
                    }],
                },
            )
            .await;
        })
        .await
        .unwrap_or_else(|_| {
            panic!(
                "{} did_change must acknowledge before provider refresh",
                case.name
            )
        });

        let document = server
            .documents
            .get(&app_uri)
            .expect("edited document remains open");
        assert_eq!(
            document.version, 2,
            "{} must execute the EDIT leg",
            case.name
        );
        assert_eq!(
            document.source.as_ref(),
            case.edited_parent_source,
            "{} must retain the delayed did_change content",
            case.name
        );
        drop(document);

        tokio::time::timeout(std::time::Duration::from_secs(2), update_arrived.notified())
            .await
            .unwrap_or_else(|_| {
                panic!(
                    "{} coordinator must eventually refresh the provider",
                    case.name
                )
            });
        update_release.notify_one();

        drain_handle.abort();
        drop(service);
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn did_change_coalesces_provider_refresh_to_latest_committed_edit() {
    let v1 = "<script setup lang=\"ts\">\nimport FirstChild from './FirstChild.vue'\n</script>\n<template><FirstChild  /></template>\n";
    let v2 = "<script setup lang=\"ts\">\nimport SecondChild from './SecondChild.vue'\n</script>\n<template><SecondChild  /></template>\n";
    let v3 = "<script setup lang=\"ts\">\nimport LatestChild from './LatestChild.vue'\n</script>\n<template><LatestChild  /></template>\n";
    let (_temp, service, drain_handle, provider, workspace_id) =
        make_definition_test_server_with_kind(
            &[
                (
                    "src/FirstChild.vue",
                    "vue",
                    "<script setup lang=\"ts\">defineProps<{ firstProp: string }>()</script>",
                ),
                (
                    "src/SecondChild.vue",
                    "vue",
                    "<script setup lang=\"ts\">defineProps<{ secondProp: string }>()</script>",
                ),
                (
                    "src/LatestChild.vue",
                    "vue",
                    "<script setup lang=\"ts\">defineProps<{ latestProp: string }>()</script>",
                ),
                ("src/App.vue", "vue", v1),
            ],
            crate::TypeProviderKind::Tsgo,
        )
        .await;
    let server = service.inner();
    let uri = workspace_uri(&workspace_id, "src/App.vue");
    server.ensure_current_file_synced(&uri).await;
    let ide_path = server
        .active_ide_path_for_uri(&uri)
        .expect("synced provider path");
    provider.clear_calls();
    let (update_arrived, update_release) = provider.block_update_file(&ide_path);
    let change = |version, text: &'static str| {
        super::super::lifecycle::handle_did_change(
            server,
            DidChangeTextDocumentParams {
                text_document: VersionedTextDocumentIdentifier {
                    uri: uri.clone(),
                    version,
                },
                content_changes: vec![TextDocumentContentChangeEvent {
                    range: None,
                    range_length: None,
                    text: text.to_string(),
                }],
            },
        )
    };

    tokio::time::timeout(std::time::Duration::from_millis(250), change(2, v2))
        .await
        .expect("v2 did_change must not wait for provider I/O");
    tokio::time::timeout(std::time::Duration::from_millis(250), change(3, v3))
        .await
        .expect("v3 did_change must not wait for provider I/O");
    assert_eq!(
        server.documents.get(&uri).map(|doc| doc.version),
        Some(3),
        "v3 is committed before the background provider refresh"
    );

    tokio::time::timeout(std::time::Duration::from_secs(2), update_arrived.notified())
        .await
        .expect("the coalesced provider refresh must eventually run");
    let published = provider
        .calls()
        .into_iter()
        .filter_map(|call| match call {
            MockCall::UpdateFile { path, content } if path == ide_path => Some(content),
            _ => None,
        })
        .collect::<Vec<_>>();
    assert_eq!(
        published.len(),
        1,
        "rapid edits must coalesce to one provider refresh"
    );
    assert!(
        published[0].contains("LatestChild") && !published[0].contains("SecondChild"),
        "the provider refresh must contain only the latest committed edit: {published:?}"
    );
    update_release.notify_one();

    drain_handle.abort();
    drop(service);
}

#[tokio::test(flavor = "multi_thread")]
async fn unrelated_edit_preserves_rootless_document_dependency_receipt() {
    let app_v1 = "<script lang=\"ts\">\nimport Child from './Child.svelte'\nconst local = 1\n</script>\n<Child />{local}\n";
    let app_v2 = "<script lang=\"ts\">\nimport Child from './Child.svelte'\nconst local = 2\n</script>\n<Child />{local}\n";
    let rootless = "<script>\nlet jsValue = 1\n</script>\n{jsValue}\n";
    let (_temp, service, drain_handle, _provider, workspace_id) = make_definition_test_server(&[
        ("src/Child.svelte", "svelte", "<div />"),
        ("src/App.svelte", "svelte", app_v1),
        ("src/JavaScriptCase.svelte", "svelte", rootless),
    ])
    .await;
    let server = service.inner();
    let app_uri = workspace_uri(&workspace_id, "src/App.svelte");
    let rootless_uri = workspace_uri(&workspace_id, "src/JavaScriptCase.svelte");
    server
        .publish_import_dependencies_settled(&rootless_uri)
        .await;
    assert!(server
        .dependency_readiness_capture(&rootless_uri)
        .is_ready());

    super::super::lifecycle::handle_did_change(
        server,
        DidChangeTextDocumentParams {
            text_document: VersionedTextDocumentIdentifier {
                uri: app_uri,
                version: 2,
            },
            content_changes: vec![TextDocumentContentChangeEvent {
                range: None,
                range_length: None,
                text: app_v2.to_string(),
            }],
        },
    )
    .await;

    assert!(
        server
            .dependency_readiness_capture(&rootless_uri)
            .is_ready(),
        "an unrelated edit cannot invalidate a document with an empty dependency closure"
    );

    drain_handle.abort();
    drop(service);
}

#[tokio::test]
async fn svelte_recovered_script_prop_facts_fail_closed_to_provider() {
    let initial = "<script lang=\"ts\">\nconst providerTarget = 1;\nlet { row } = $props();\nvoid 0;\n</script>\n{@render row()}";
    let changed = initial.replace("void 0;", "return;");
    let (_temp, service, drain_handle, provider, workspace_id) =
        make_default_profile_definition_test_server(&[("src/App.svelte", "svelte", initial)]).await;
    let server = service.inner();
    let app_uri = workspace_uri(&workspace_id, "src/App.svelte");
    let canonical_id = server
        .documents
        .get_canonical_id(&app_uri)
        .expect("open canonical id");
    let profile = server.documents.tsx_profile.read().clone();
    let initial_ide = server
        .documents
        .host()
        .get_ide(&canonical_id, &profile)
        .expect("initial IDE surface");
    assert!(server.documents.did_change(&app_uri, 2, &changed).changed);
    let evidence = server
        .documents
        .host()
        .resolve_svelte_script_facts(&canonical_id);
    let verter_session::framework::script_facts::ScriptFactEvidence::Partial(partial) = evidence
    else {
        panic!("recoverable script must expose partial facts");
    };
    assert!(
        partial.exact_syntax().is_none(),
        "recovered syntax must not mint absence-sensitive `$props` authority"
    );

    let position = find_document_position(server, &app_uri, "@render row", 8);
    let mut state = server
        .provider_sync_state_for_source(&canonical_id)
        .unwrap_or_else(|| ProviderSyncState {
            owner_binding: crate::provider_sync::ProviderOwnerBinding::Unresolved,
            ..Default::default()
        });
    let ide_path = server
        .target_ide_path_for_uri(&app_uri)
        .expect("Svelte IDE path");
    state.ide_path = Some(ide_path.clone());
    state.ide_background_loaded = true;
    server.commit_provider_sync_state(&canonical_id, state);
    let revision = server
        .documents
        .snapshot_identity(&app_uri)
        .expect("current edited identity");
    server.record_carrier_ide_snapshot_with_pin(
        Some((&app_uri, &revision)),
        &canonical_id,
        &ide_path,
        &initial_ide.code,
        initial_ide.source_map.as_deref(),
    );
    let ctx = server
        .type_provider_context(&app_uri)
        .expect("current provider surface without foreground compile");
    let query_offset = merge::carrier_position_to_tsx_offset_validated(
        &position,
        &ctx.carrier_line_index,
        &ctx.mapper,
        &ctx.tsx_line_index,
    )
    .expect("render callee must map into the retained provider surface");
    let target_range = range_for_authored_snippet(server, &app_uri, "providerTarget");
    let target_offset = merge::carrier_position_to_tsx_offset_validated(
        &target_range.start,
        &ctx.carrier_line_index,
        &ctx.mapper,
        &ctx.tsx_line_index,
    )
    .expect("provider target must map into the retained provider surface");
    provider.set_definitions(
        &ctx.tsx_path,
        query_offset,
        vec![TypeLocation {
            path: ctx.tsx_path.clone(),
            start: target_offset,
            end: target_offset + "providerTarget".len() as u32,
        }],
    );

    let response = server
        .goto_definition(goto_definition_params(&app_uri, position))
        .await
        .expect("goto definition request")
        .expect("provider definition");
    let locations = definition_locations(response);
    assert!(
        locations
            .iter()
            .any(|location| location.uri == app_uri && location.range == target_range),
        "recovered facts must preserve the planted provider target: {locations:?}"
    );

    drain_handle.abort();
    drop(service);
}

#[tokio::test]
async fn barrel_import_binding_in_vue_script_skips_type_provider_barrel_result() {
    let overlay_source = "<script setup lang=\"ts\">\nconst visible = ref(false)\n</script>\n<template>\n  <div>Overlay</div>\n</template>\n";
    let barrel_source = "export { default as Overlay } from './Overlay.vue'\nexport { default as Button } from './Button.vue'\n";
    let button_source = "<script setup lang=\"ts\">\ndefineProps<{ label: string }>()\n</script>\n";
    let app_source = "<script setup lang=\"ts\">\nimport { ref, computed } from 'vue'\nimport { Overlay, Button } from './components'\n\nconst count = ref(0)\nconst doubled = computed(() => count.value * 2)\nconst showOverlay = ref(false)\n\nfunction increment() { count.value++ }\n</script>\n<template>\n  <div>\n    <p>{{ count }} x 2 = {{ doubled }}</p>\n    <button @click=\"increment\">+</button>\n    <Button label=\"Open\" @click=\"showOverlay = true\" />\n    <Overlay :show=\"showOverlay\" :zIndex=\"100\" :lockScroll=\"true\">\n      <p>Overlay content</p>\n    </Overlay>\n  </div>\n</template>\n";
    let (_temp, service, drain_handle, provider, workspace_id) = make_definition_test_server(&[
        ("src/components/Overlay.vue", "vue", overlay_source),
        ("src/components/Button.vue", "vue", button_source),
        ("src/components/index.ts", "typescript", barrel_source),
        ("src/App.vue", "vue", app_source),
    ])
    .await;

    let app_uri = workspace_uri(&workspace_id, "src/App.vue");
    let overlay_uri = workspace_uri(&workspace_id, "src/components/Overlay.vue");
    let barrel_path = format!("{workspace_id}/src/components/index.ts");
    let server = service.inner();
    let position = find_document_position(server, &app_uri, "{ Overlay, Button }", 2);
    let ctx = synced_type_provider_context(server, &app_uri).await;
    let tsx_offset = merge::carrier_position_to_tsx_offset_validated(
        &position,
        &ctx.carrier_line_index,
        &ctx.mapper,
        &ctx.tsx_line_index,
    )
    .expect("import position should map into TSX");
    provider.set_definitions(
        &ctx.tsx_path,
        tsx_offset,
        vec![TypeLocation {
            path: barrel_path,
            start: 20,
            end: 27,
        }],
    );

    let response = server
        .goto_definition(goto_definition_params(&app_uri, position))
        .await
        .expect("goto definition should succeed")
        .expect("import binding should resolve");
    let locations = definition_locations(response);
    let target = locations
        .iter()
        .find(|loc| loc.uri == overlay_uri)
        .expect("definition should point to Overlay.vue");

    assert_eq!(
            target.range,
            Range::default(),
            "Vue script import binding should resolve through barrel to the terminal component file start even when the type provider returns the barrel"
        );
    assert!(
        !locations
            .iter()
            .any(|loc| loc.uri.as_str().ends_with("/src/components/index.ts")),
        "Vue script import binding should not stop at the barrel file"
    );
    assert!(
        !provider
            .calls()
            .iter()
            .any(|call| matches!(call, MockCall::GetDefinition { .. })),
        "native import binding resolution should skip the type provider entirely"
    );

    drain_handle.abort();
    drop(service);
}

/// W02 deterministic bound: a syntax verdict binds only its exact source
/// bytes, fails closed, and prevents later requests from recompiling it.
#[tokio::test(flavor = "multi_thread")]
async fn broken_projectionless_carrier_fails_closed_and_bounds_content_verdicts() {
    let provider = Arc::new(MockTypeProvider::new());
    let type_provider: Arc<dyn TypeProvider> = provider.clone();
    let service = make_hover_test_service_with_kind(type_provider, crate::TypeProviderKind::Tsgo);
    let server = service.inner();
    install_test_resolver(server);
    // Provider-rail only: with the native lane off, a hover answer could only
    // come from a (wrong) provider query against a surface that must not exist.
    server
        .hover_native_semantics_enabled
        .store(false, std::sync::atomic::Ordering::Release);

    let uri = open_test_vue(
        server,
        "/workspace/src/App.vue",
        "<script setup lang=\"ts\">\nconst broken = (((\n",
    );
    assert!(
        server.documents.get_projection(&uri).is_none(),
        "precondition: the malformed open must leave the carrier projection-less"
    );
    assert!(
        !server.current_file_needs_inline_type_provider_sync(&uri),
        "the open-time syntax verdict must bind these exact bytes"
    );
    // A later revision, STILL malformed (the ordinary state of a file being
    // typed). The commit stores text only.
    let _ =
        server
            .documents
            .did_change(&uri, 2, "<script setup lang=\"ts\">\nconst broken = ((((\n");
    assert!(
        server.documents.get_projection(&uri).is_none(),
        "the commit must not compile"
    );

    assert!(
        server.current_file_needs_inline_type_provider_sync(&uri),
        "new bytes have no deterministic verdict yet"
    );

    let position = Position {
        line: 1,
        character: 8,
    };
    let first = server
        .hover(hover_params(&uri, position))
        .await
        .expect("hover request must not error to the client");
    assert!(
        first.is_none(),
        "a broken carrier fails closed on the interactive path — a clean \
         no-result, never a wrong or stale one"
    );

    assert!(
        !server.current_file_needs_inline_type_provider_sync(&uri),
        "the repair's syntax verdict must bind these exact bytes"
    );

    let cold_before_second = server
        .documents
        .host()
        .provenance_snapshot()
        .compile_cold_runs;
    let second = server
        .hover(hover_params(&uri, position))
        .await
        .expect("hover request must not error to the client");
    assert!(second.is_none(), "still fails closed on repeat");
    assert_eq!(
        server
            .documents
            .host()
            .provenance_snapshot()
            .compile_cold_runs,
        cold_before_second,
        "a later request must not recompile bytes with a deterministic verdict"
    );

    let hover_calls = provider
        .calls()
        .iter()
        .filter(|call| matches!(call, MockCall::GetHover { .. }))
        .count();
    assert_eq!(
        hover_calls, 0,
        "a projection-less broken carrier must never reach the provider — there \
         is no committed surface to query"
    );

    // A new revision is likewise repairable.
    let _ = server.documents.did_change(
        &uri,
        3,
        "<script setup lang=\"ts\">\nconst broken = (((((\n",
    );
    assert!(
        server.current_file_needs_inline_type_provider_sync(&uri),
        "a content change remains eligible for interactive repair"
    );
}

/// The retained-source comparison must guard the provider WRITE itself. This
/// pauses after the old pre-sync fence, then commits revision B before the
/// provider call can consume revision A's retained bytes.
#[tokio::test(flavor = "multi_thread")]
async fn a_source_change_after_the_check_prevents_the_provider_write() {
    let (service, provider, uri) = make_request_surface_carrier().await;
    let server = service.inner();
    let canonical_id = "/workspace/src/App.vue";
    const SOURCE_B: &str = r#"<script setup lang="ts">
const msg = 'provider-write-window'
const extra = 42
</script>
<template><div>{{ msg }}{{ extra }}</div></template>
"#;

    provider.clear_calls();
    server.needs_ide_sync.insert(canonical_id.to_string());
    let (arrived, release) = server.pause_next_ide_sync_before_provider_write(canonical_id);

    let repair = server.ensure_current_file_synced(&uri);
    let edit = async {
        arrived.notified().await;
        let _ = server.documents.did_change(&uri, 2, SOURCE_B);
        release.notify_one();
    };
    futures_util::future::join(repair, edit).await;

    assert!(
        provider.file_sync_calls().is_empty(),
        "revision A's retained bytes must not be written to the provider after \
         revision B commits in the check-to-sync window"
    );
}

/// The RETRY IDENTITY FENCE: a concurrent edit landing between the two
/// attempts can put a DIFFERENT token at the same coordinates (`foo.bar` →
/// `fyy.baz`), and a fence-less retry would return a coherent-but-WRONG
/// definition for the original request (the recomputed offset validates only
/// that the coordinate maps within the NEW surface). The shared recovery owner
/// must refuse the retry when the recaptured surface's carrier source differs
/// from the initial capture — fail closed, and never issue the second query.
#[tokio::test]
async fn provider_retry_is_fenced_on_carrier_source_identity() {
    let provider = Arc::new(MockTypeProvider::new());
    let type_provider: Arc<dyn TypeProvider> = provider.clone();
    let service = make_hover_test_service(type_provider);
    let server = service.inner();
    install_test_resolver(server);

    // v2 is byte-shape-identical to v1 (same lengths, same coordinates), so the
    // member position still MAPS in v2 — the fence, not offset validation, must
    // be what blocks the retry.
    let v2_source = r#"<script setup lang="ts">
const fyy = { baz: 1 }
</script>
<template><div>{{ fyy.baz }}</div></template>
"#;

    let (app_uri, member_position, _expected_decl_range) =
        seed_member_definition_fixture(server, &provider, "/workspace/src/App.vue");
    let initial_ctx = synced_type_provider_context_surface_only(server, &app_uri);
    let initial_offset = merge::carrier_position_to_tsx_offset_validated(
        &member_position,
        &initial_ctx.carrier_line_index,
        &initial_ctx.mapper,
        &initial_ctx.tsx_line_index,
    )
    .expect("member usage maps into the generated TSX");

    // A poison answer the retry WOULD return if the fence let it through:
    // `baz`'s declaration in the v2 surface, coherent with the fresh context.
    let poison = crate::type_provider::protocol::TypeLocation {
        path: initial_ctx.tsx_path.clone(),
        start: 0,
        end: 3,
    };
    let calls = Arc::new(AtomicUsize::new(0));
    let calls_in = calls.clone();

    let outcome = super::super::provider_recovery::provider_query_with_bounded_recovery(
        "definition",
        &member_position,
        initial_ctx,
        initial_offset,
        |_tsx_path: String, _offset: u32| {
            let n = calls_in.fetch_add(1, Ordering::SeqCst);
            let poison = poison.clone();
            async move {
                if n == 0 {
                    Err(crate::type_provider::protocol::TypeProviderError::new(
                        "scripted transient failure".to_string(),
                    ))
                } else {
                    Ok(vec![poison])
                }
            }
        },
        || async {
            // The concurrent edit lands while the handler is resyncing.
            let _ = server.documents.did_change(&app_uri, 2, v2_source);
        },
        // The post-resync surface describes the EDITED document (v2).
        || Some(synced_type_provider_context_surface_only(server, &app_uri)),
    )
    .await;

    assert!(
        outcome.value.is_none(),
        "a retry against an edited carrier source must FAIL CLOSED — it would answer a \
         different request than the one asked"
    );
    assert_eq!(
        calls.load(Ordering::SeqCst),
        1,
        "the identity fence must refuse the SECOND query outright, not launder a \
         cross-revision answer"
    );
}

/// The fence keys on SOURCE identity, not surface generation: a resync that
/// re-records the SAME carrier source under a fresh generation/stamp must
/// still retry (an over-strict generation fence would turn every recovery
/// into a fail-closed miss and reintroduce the dead-CTRL+CLICK defect).
#[tokio::test]
async fn provider_retry_proceeds_when_carrier_source_is_unchanged_across_regeneration() {
    let provider = Arc::new(MockTypeProvider::new());
    let type_provider: Arc<dyn TypeProvider> = provider.clone();
    let service = make_hover_test_service(type_provider);
    let server = service.inner();
    install_test_resolver(server);

    let (app_uri, member_position, _expected_decl_range) =
        seed_member_definition_fixture(server, &provider, "/workspace/src/App.vue");
    let initial_ctx = synced_type_provider_context_surface_only(server, &app_uri);
    let initial_generation = initial_ctx.snapshot.stamp.generation;
    let initial_source_hash = initial_ctx.snapshot.source_hash;
    let initial_offset = merge::carrier_position_to_tsx_offset_validated(
        &member_position,
        &initial_ctx.carrier_line_index,
        &initial_ctx.mapper,
        &initial_ctx.tsx_line_index,
    )
    .expect("member usage maps into the generated TSX");

    let answer = crate::type_provider::protocol::TypeLocation {
        path: initial_ctx.tsx_path.clone(),
        start: 7,
        end: 10,
    };
    let calls = Arc::new(AtomicUsize::new(0));
    let calls_in = calls.clone();
    // The stamp/source identity of the surface the recapture actually served,
    // recorded so the discrimination preconditions below assert on the REAL
    // retry context rather than on what this test hoped it built.
    let recaptured = Arc::new(std::sync::Mutex::new(
        None::<(u64, crate::provider_surface_store::ContentHash)>,
    ));
    let recaptured_in = recaptured.clone();

    let outcome = super::super::provider_recovery::provider_query_with_bounded_recovery(
        "definition",
        &member_position,
        initial_ctx,
        initial_offset,
        |_tsx_path: String, _offset: u32| {
            let n = calls_in.fetch_add(1, Ordering::SeqCst);
            let answer = answer.clone();
            async move {
                if n == 0 {
                    Err(crate::type_provider::protocol::TypeProviderError::new(
                        "scripted transient failure".to_string(),
                    ))
                } else {
                    Ok(vec![answer])
                }
            }
        },
        || async {},
        || {
            // Mint a genuinely FRESH surface generation over byte-identical
            // source: re-record the same provider content (every record
            // advances the store generation), then recapture. Without this,
            // the recapture would serve the INITIAL snapshot back and a
            // generation-equality fence would pass this test vacuously —
            // exactly what it exists to rule out.
            let canonical_id = server
                .documents
                .get_canonical_id(&app_uri)
                .expect("canonical id");
            let ide = server.documents.get_ide(&app_uri).expect("IDE output");
            let tsx_path = server
                .active_ide_path_for_uri(&app_uri)
                .expect("live IDE path");
            let seed_revision = server.documents.snapshot_identity(&app_uri);
            server.record_carrier_ide_snapshot_with_pin(
                seed_revision.as_ref().map(|revision| (&app_uri, revision)),
                &canonical_id,
                &tsx_path,
                &ide.code,
                None,
            );
            let ctx = server.type_provider_context(&app_uri)?;
            *recaptured_in.lock().unwrap() =
                Some((ctx.snapshot.stamp.generation, ctx.snapshot.source_hash));
            Some(ctx)
        },
    )
    .await;

    // Discrimination preconditions: the retry surface really was a DIFFERENT
    // generation over the SAME source — so a generation-equality fence fails
    // this test while the source-identity fence passes it.
    let (retry_generation, retry_source_hash) = recaptured
        .lock()
        .unwrap()
        .expect("the recovery must have recaptured a retry surface");
    assert!(
        retry_generation > initial_generation,
        "precondition: the recapture must serve a genuinely ADVANCED surface generation \
         (initial {initial_generation}, retry {retry_generation})"
    );
    assert_eq!(
        retry_source_hash, initial_source_hash,
        "precondition: the regenerated surface must describe byte-identical carrier source"
    );

    assert!(
        outcome.value.is_some_and(|locs| locs.len() == 1),
        "an identical-source retry must proceed and recover the provider answer"
    );
    assert_eq!(
        calls.load(Ordering::SeqCst),
        2,
        "the retry must actually re-query when the carrier source is unchanged"
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn background_init_drains_pending_snapshot_provider_sync_for_open_vue_file() {
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

    assert!(
        !pending_snapshot_provider_sync.contains("/workspace/src/App.vue"),
        "drained open Vue files should be removed from the pending snapshot queue"
    );

    let state = provider_sync_states
        .get("/workspace/src/App.vue")
        .map(|entry| entry.clone())
        .expect("drained sync should commit owner-aware provider state");
    assert!(
        !state.is_unresolved(),
        "drain must set an owner-aware binding on provider state"
    );

    let calls = provider.file_sync_calls();
    assert!(
        calls.iter().any(|call| matches!(
            call,
            MockCall::OpenFile { path, .. } | MockCall::UpdateFile { path, .. }
                if path.ends_with(".vue.verter.ts")
        )),
        "drain should sync the Vue public API through .vue.verter.ts"
    );
    assert!(
        calls.iter().any(|call| matches!(
            call,
            MockCall::OpenFile { path, .. } | MockCall::UpdateFile { path, .. }
                if path.ends_with(".tsx")
        )),
        "drain should sync the open Vue IDE file through the synthetic TSX path"
    );
}

/// An open plain script queued before the resolver snapshot was published is
/// delivered to tsserver with its AUTHORED import specifiers, exactly as the
/// coordinator tick and the editor ingress deliver it. Rewriting
/// `./Comp.vue` to its generated API companion is a tsgo-only projection; on
/// tsserver the editor-side position mapper stays the identity, so rewritten
/// bytes would shift every hover and navigation position in the file onto a
/// neighbouring token.
#[tokio::test(flavor = "multi_thread")]
async fn pending_snapshot_drain_delivers_an_open_script_with_authored_specifiers_on_tsserver() {
    let temp = tempfile::tempdir().expect("temp dir");
    let workspace = temp.path().join("workspace");
    std::fs::create_dir_all(workspace.join("src")).expect("workspace dir");
    std::fs::write(workspace.join("tsconfig.json"), "{}").expect("write tsconfig");
    let component_source = "<script setup lang=\"ts\">
const label = 1;
</script>
<template><div>{{ label }}</div></template>";
    let authored = "import Comp from \"./Comp.vue\";

export const direct = Comp;
";
    std::fs::write(workspace.join("src").join("Comp.vue"), component_source)
        .expect("write carrier");
    std::fs::write(workspace.join("src").join("consumer.ts"), authored).expect("write consumer");
    let workspace_id = crate::test_utils::canonical_test_path(&workspace);
    let consumer_id = format!("{workspace_id}/src/consumer.ts");
    let project = verter_workspace::ide_project_config(
        workspace_id.clone(),
        workspace_id.clone(),
        Some(format!("{workspace_id}/tsconfig.json")),
    );

    let host = Arc::new(VerterHost::new(
        HostConfig::default(),
        Arc::new(verter_workspace::FilesystemWorkspace::new(
            verter_workspace::FilesystemOptions::default(),
        )),
    ));
    host.configure_projects(vec![project.clone()]);
    let documents = Arc::new(DocumentRegistry::new(Arc::clone(&host)));
    for (relative, language_id, text) in [
        ("src/Comp.vue", "vue", component_source),
        ("src/consumer.ts", "typescript", authored),
    ] {
        let _ = documents.did_open(&TextDocumentItem {
            uri: crate::uri::path_to_file_uri(&format!("{workspace_id}/{relative}"))
                .expect("file uri"),
            language_id: language_id.to_string(),
            version: 1,
            text: text.to_string(),
        });
    }

    let provider = Arc::new(MockTypeProvider::new());
    let sync = ProjectSync::new(provider.clone(), ProjectSyncMode::FullProject);
    let snapshot = PublishedResolverSnapshot {
        resolver: verter_resolution::ModuleResolverCore::new(vec![project]),
        resolution_view: None,
        ownership_ready: true,
    };
    let generation_before = host.get_diagnostics_generation(&consumer_id).unwrap_or(0);
    let outcome = super::super::background_drain::sync_pending_snapshot_provider_file(
        Some(&sync),
        &documents,
        &snapshot,
        &DashMap::new(),
        &consumer_id,
        None,
        &crate::external_ts::CarrierTransactionCoordinator::new(),
        false,
        &dashmap::DashSet::new(),
    )
    .await;

    let delivered: Vec<String> = provider
        .calls()
        .into_iter()
        .filter_map(|call| match call {
            MockCall::OpenFile { path, content }
            | MockCall::OpenFileBackground { path, content }
            | MockCall::LoadFile { path, content }
            | MockCall::UpdateFile { path, content }
                if path == consumer_id =>
            {
                Some(content)
            }
            _ => None,
        })
        .collect();
    assert_eq!(
        delivered.last().map(String::as_str),
        Some(authored),
        "the drained script must reach tsserver verbatim: {delivered:?}"
    );
    assert!(
        matches!(
            outcome,
            super::super::background_drain::SyncOutcome::FullyReconciled
        ),
        "a delivered script is reconciled"
    );
    assert!(
        host.get_diagnostics_generation(&consumer_id).unwrap_or(0) > generation_before,
        "the re-synced buffer must outdate any diagnostics receipt the document still owes"
    );
}

/// DISCRIMINATING: a pending carrier whose sync failed while the provider was
/// unavailable (a replacement gap between epochs) must be re-driven by the
/// drain's own bounded successor chain once the provider serves again. After
/// startup's last scanner pass no external drain exists to retry it, so a
/// queue-only contract strands the source with no diagnostics. RED-before: the
/// retained entry is never re-synced (the editor-neutral contract observed
/// `publishDiagnostics` never settling for exactly these sources). GREEN-after:
/// the successor pass re-syncs through the recovered provider and dequeues.
#[tokio::test(flavor = "multi_thread")]
async fn pending_snapshot_provider_sync_redrives_transiently_failed_source_after_recovery() {
    let host = Arc::new(VerterHost::new_standalone(HostConfig::default()));
    let documents = Arc::new(DocumentRegistry::new(Arc::clone(&host)));
    let uri: Uri = "file:///workspace/src/App.vue".parse().unwrap();
    let _ = documents.did_open(&TextDocumentItem {
        uri: uri.clone(),
        language_id: "vue".to_string(),
        version: 1,
        text: "<template><div /></template>".to_string(),
    });

    let provider = Arc::new(MockTypeProvider::new());
    provider.set_fail_file_ops(true);
    let sync = ProjectSync::new(provider.clone(), ProjectSyncMode::FullProject);
    let vfs_workspace = Arc::new(crate::test_utils::make_test_vfs_workspace_with_resolver(
        "/workspace",
        Some("/workspace/tsconfig.app.json"),
    ));
    let provider_sync_states: DashMap<String, crate::provider_sync::ProviderSyncState> =
        DashMap::new();
    let provider_sync_states = Arc::new(provider_sync_states);
    let pending_snapshot_provider_sync: DashSet<String> = DashSet::new();
    let pending_snapshot_provider_sync = Arc::new(pending_snapshot_provider_sync);
    pending_snapshot_provider_sync.insert("/workspace/src/App.vue".to_string());

    let drain = Arc::new(PendingSyncDrain {
        project_sync: Some(sync),
        documents: Arc::clone(&documents),
        vfs_workspace: Arc::clone(&vfs_workspace),
        provider_sync_states: Arc::clone(&provider_sync_states),
        pending_snapshot_provider_sync: Arc::clone(&pending_snapshot_provider_sync),
        is_tsgo: false,
        mru_canonical_ids: None,
        carrier_publish_coordinator: None,
        carrier_transaction_coordinator: Arc::new(
            crate::external_ts::CarrierTransactionCoordinator::new(),
        ),
    });
    // Fast schedule: 5ms successors with no backoff, enough attempts to cross
    // the recovery point the test controls explicitly.
    let redrive = PendingSyncRedrive {
        initial_delay_ms: 5,
        backoff_factor: 1,
        max_attempts: 50,
    };
    drain_pending_snapshot_provider_sync_owned(Arc::clone(&drain), redrive, 1).await;

    assert!(
        pending_snapshot_provider_sync.contains("/workspace/src/App.vue"),
        "the pass against the failed provider must retain the entry (fail closed)"
    );

    // The provider recovers (its replacement now serves).
    provider.set_fail_file_ops(false);

    let drained = tokio::time::timeout(std::time::Duration::from_secs(5), async {
        while pending_snapshot_provider_sync.contains("/workspace/src/App.vue") {
            tokio::time::sleep(std::time::Duration::from_millis(5)).await;
        }
    })
    .await
    .is_ok();
    assert!(
        drained,
        "the bounded successor chain must re-drive the transiently failed source after recovery"
    );
    assert!(
        provider_sync_states.get("/workspace/src/App.vue").is_some(),
        "the re-driven sync should commit owner-aware provider state"
    );
}

/// DISCRIMINATING: the pending-sync re-drive budget is spent per ENTRY, not
/// per chain. A chain that exhausts its attempt cap with the entry still
/// queued must NOT be restarted by a later plain arm (the coordinator's
/// loop-wake arm) — not even after the provider recovers — while the RETRY
/// SIGNAL (`signal_pending_sync_redrive`: an external drain pass, or an
/// engine (re)start) clears the budget and re-drives the entry to
/// convergence. RED-before: every coordinator wake armed a fresh bounded
/// chain, so a refused entry was retried for the rest of the session; and
/// after the chain stopped, nothing re-drove the parked entry even once the
/// provider served again (the editor-neutral CI stall).
#[tokio::test(flavor = "multi_thread")]
async fn pending_sync_redrive_budget_survives_wakes_and_re_arms_on_the_retry_signal() {
    let host = Arc::new(VerterHost::new_standalone(HostConfig::default()));
    let documents = Arc::new(DocumentRegistry::new(Arc::clone(&host)));
    let uri: Uri = "file:///workspace/src/App.vue".parse().unwrap();
    let _ = documents.did_open(&TextDocumentItem {
        uri: uri.clone(),
        language_id: "vue".to_string(),
        version: 1,
        text: "<template><div /></template>".to_string(),
    });

    let provider = Arc::new(MockTypeProvider::new());
    provider.set_fail_file_ops(true);
    let sync = ProjectSync::new(provider.clone(), ProjectSyncMode::FullProject);
    let vfs_workspace = Arc::new(crate::test_utils::make_test_vfs_workspace_with_resolver(
        "/workspace",
        Some("/workspace/tsconfig.app.json"),
    ));
    let provider_sync_states: DashMap<String, crate::provider_sync::ProviderSyncState> =
        DashMap::new();
    let provider_sync_states = Arc::new(provider_sync_states);
    let pending_snapshot_provider_sync: DashSet<String> = DashSet::new();
    let pending_snapshot_provider_sync = Arc::new(pending_snapshot_provider_sync);
    pending_snapshot_provider_sync.insert("/workspace/src/App.vue".to_string());

    let drain = Arc::new(PendingSyncDrain {
        project_sync: Some(sync),
        documents: Arc::clone(&documents),
        vfs_workspace: Arc::clone(&vfs_workspace),
        provider_sync_states: Arc::clone(&provider_sync_states),
        pending_snapshot_provider_sync: Arc::clone(&pending_snapshot_provider_sync),
        is_tsgo: false,
        mru_canonical_ids: None,
        carrier_publish_coordinator: None,
        carrier_transaction_coordinator: Arc::new(
            crate::external_ts::CarrierTransactionCoordinator::new(),
        ),
    });
    // Fast schedule: cap the chain at ONE successor pass so the exhaustion is
    // reached deterministically and quickly.
    let redrive = PendingSyncRedrive {
        initial_delay_ms: 5,
        backoff_factor: 1,
        max_attempts: 2,
    };
    drain_pending_snapshot_provider_sync_owned(Arc::clone(&drain), redrive, 1).await;

    // Wait for the armed successor pass to run and the chain to end at its
    // attempt cap: the provider call count stabilizes with the entry queued.
    let stabilized = tokio::time::timeout(std::time::Duration::from_secs(5), async {
        let mut last = provider.file_sync_calls().len();
        loop {
            tokio::time::sleep(std::time::Duration::from_millis(50)).await;
            let now = provider.file_sync_calls().len();
            if now == last {
                break;
            }
            last = now;
        }
    })
    .await
    .is_ok();
    assert!(stabilized, "the bounded chain must end at its attempt cap");
    assert!(
        pending_snapshot_provider_sync.contains("/workspace/src/App.vue"),
        "the entry the chain could not settle stays queued (fail closed)"
    );
    let budget_calls = provider.file_sync_calls().len();
    assert!(
        budget_calls > 0,
        "the immediate pass and its one successor must have attempted the sync"
    );

    // A coordinator-style plain arm — even after the provider recovers —
    // must NOT restart a chain for the exhausted entry.
    provider.set_fail_file_ops(false);
    arm_pending_sync_redrive_once(&drain, redrive);
    tokio::time::sleep(std::time::Duration::from_millis(150)).await;
    assert_eq!(
        provider.file_sync_calls().len(),
        budget_calls,
        "a plain wake must not retry an entry that spent its chain budget"
    );
    assert!(
        pending_snapshot_provider_sync.contains("/workspace/src/App.vue"),
        "the exhausted entry is not re-driven by a mere coordinator wake"
    );

    // The RETRY SIGNAL clears the budget and re-drives to convergence — the
    // engine-(re)start path's guarantee that a replacement gap longer than
    // one bounded chain cannot strand the parked sync.
    signal_pending_sync_redrive(&drain, redrive);
    let drained = tokio::time::timeout(std::time::Duration::from_secs(5), async {
        while pending_snapshot_provider_sync.contains("/workspace/src/App.vue") {
            tokio::time::sleep(std::time::Duration::from_millis(5)).await;
        }
    })
    .await
    .is_ok();
    assert!(
        drained,
        "the retry signal must re-arm the chain and settle the recovered entry"
    );
    assert!(
        provider_sync_states.get("/workspace/src/App.vue").is_some(),
        "the re-driven sync should commit owner-aware provider state"
    );

    // New work stays live while an old cohort holds its budget: a freshly
    // queued entry (here: the same source re-queued after a superseded
    // commit) is NOT in the exhausted cohort and arms on a plain wake. The
    // armed pass either attempts a leg still owed or, finding every leg
    // already current, settles the entry without a provider round trip; an
    // unarmed entry does neither.
    let settled_calls = provider.file_sync_calls().len();
    provider.set_fail_file_ops(true);
    pending_snapshot_provider_sync.insert("/workspace/src/App.vue".to_string());
    arm_pending_sync_redrive_once(&drain, redrive);
    let attempted = tokio::time::timeout(std::time::Duration::from_secs(5), async {
        loop {
            if provider.file_sync_calls().len() > settled_calls
                || !pending_snapshot_provider_sync.contains("/workspace/src/App.vue")
            {
                break;
            }
            tokio::time::sleep(std::time::Duration::from_millis(5)).await;
        }
    })
    .await
    .is_ok();
    assert!(
        attempted,
        "work queued after the last exhaustion stays eligible for a plain arm"
    );
}

/// DISCRIMINATING: a retry signal (an engine start repairing the inputs the
/// pass is failing on) that lands WHILE a chain's final pass runs must survive
/// the chain's end — the chain yields to it instead of recording its
/// exhaustion over the budget the signal just cleared. RED-before: the chain
/// refreshed `pass_generation` AFTER the pass, so the mid-pass signal's
/// generation bump was folded into the baseline, `signalled_since_pass` read
/// `false`, the exhaustion record re-parked the entry, and the signal's own
/// arm had stood down on the chain's guard — the stranded-sync stall behind
/// the editor-neutral/VS Code E2E diagnostics timeouts.
#[tokio::test(flavor = "multi_thread")]
async fn pending_sync_redrive_signal_landing_during_a_pass_is_not_swallowed() {
    let host = Arc::new(VerterHost::new_standalone(HostConfig::default()));
    let documents = Arc::new(DocumentRegistry::new(Arc::clone(&host)));
    let uri: Uri = "file:///workspace/src/App.vue".parse().unwrap();
    let _ = documents.did_open(&TextDocumentItem {
        uri: uri.clone(),
        language_id: "vue".to_string(),
        version: 1,
        text: "<template><div /></template>".to_string(),
    });

    let provider = Arc::new(MockTypeProvider::new());
    provider.set_fail_file_ops(true);
    let sync = ProjectSync::new(provider.clone(), ProjectSyncMode::FullProject);
    let vfs_workspace = Arc::new(crate::test_utils::make_test_vfs_workspace_with_resolver(
        "/workspace",
        Some("/workspace/tsconfig.app.json"),
    ));
    let provider_sync_states: DashMap<String, crate::provider_sync::ProviderSyncState> =
        DashMap::new();
    let provider_sync_states = Arc::new(provider_sync_states);
    let pending_snapshot_provider_sync: DashSet<String> = DashSet::new();
    let pending_snapshot_provider_sync = Arc::new(pending_snapshot_provider_sync);
    pending_snapshot_provider_sync.insert("/workspace/src/App.vue".to_string());

    let carrier_transaction_coordinator =
        Arc::new(crate::external_ts::CarrierTransactionCoordinator::new());
    let drain = Arc::new(PendingSyncDrain {
        project_sync: Some(sync),
        documents: Arc::clone(&documents),
        vfs_workspace: Arc::clone(&vfs_workspace),
        provider_sync_states: Arc::clone(&provider_sync_states),
        pending_snapshot_provider_sync: Arc::clone(&pending_snapshot_provider_sync),
        is_tsgo: false,
        mru_canonical_ids: None,
        carrier_publish_coordinator: None,
        carrier_transaction_coordinator: Arc::clone(&carrier_transaction_coordinator),
    });
    // Cap the chain at ONE successor pass so the exhaustion decision is the
    // very pass the signal lands in.
    let redrive = PendingSyncRedrive {
        initial_delay_ms: 5,
        backoff_factor: 1,
        max_attempts: 2,
    };
    drain_pending_snapshot_provider_sync_owned(Arc::clone(&drain), redrive, 1).await;
    assert!(
        pending_snapshot_provider_sync.contains("/workspace/src/App.vue"),
        "the failed pass must retain the entry (fail closed)"
    );

    // Hook the engine start onto the successor pass's first provider open:
    // the mock captures the open's failure BEFORE firing the one-shot
    // callback, so the signal + recovery land mid-pass while the pass itself
    // still fails — exactly the replacement-gap interleaving.
    let open_path = provider
        .file_sync_calls()
        .iter()
        .find_map(|call| match call {
            MockCall::OpenFile { path, .. } => Some(path.clone()),
            _ => None,
        })
        .expect("the failed pass must have attempted a provider open");
    {
        let provider_for_cb = Arc::clone(&provider);
        let coordinator_for_cb = Arc::clone(&carrier_transaction_coordinator);
        provider.set_on_open_file(
            &open_path,
            Box::new(move || {
                // The fresh epoch serves: later provider calls succeed.
                provider_for_cb.set_fail_file_ops(false);
                // The engine-start retry signal: its own arm stands down on
                // the running chain's guard, so only the generation bump (the
                // cleared budget) must carry its intent through the chain's
                // end.
                coordinator_for_cb.pending_redrive_clear_exhausted();
            }),
        );
    }

    // No further signal is issued by the test: the chain itself must hand the
    // mid-pass signal its fresh chain, which settles the recovered entry.
    let drained = tokio::time::timeout(std::time::Duration::from_secs(5), async {
        while pending_snapshot_provider_sync.contains("/workspace/src/App.vue") {
            tokio::time::sleep(std::time::Duration::from_millis(5)).await;
        }
    })
    .await
    .is_ok();
    assert!(
        drained,
        "a retry signal landing mid-pass must not be swallowed by the chain's exhaustion record"
    );
    assert!(
        provider_sync_states.get("/workspace/src/App.vue").is_some(),
        "the re-driven sync should commit owner-aware provider state"
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn open_vue_provider_state_survives_owner_none_snapshot_drain() {
    // Editor-liveness invariant: an OPEN Vue document's provider state must
    // NOT be removed (nor its TSX closed) merely because the ready ownership
    // snapshot resolves no owner for it. The drain may keep it queued for a
    // future owner, but it must preserve the open file's unresolved state and
    // keep its IDE TSX live so hover/completion keep working.
    let host = Arc::new(VerterHost::new_standalone(HostConfig::default()));
    let documents = DocumentRegistry::new(Arc::clone(&host));
    let uri: Uri = "file:///workspace/src/App.vue".parse().unwrap();
    let _ = documents.did_open(&TextDocumentItem {
        uri: uri.clone(),
        language_id: "vue".to_string(),
        version: 1,
        text: "<template><div>{{ msg }}</div></template>".to_string(),
    });

    let provider = Arc::new(MockTypeProvider::new());
    let sync = ProjectSync::new(provider.clone(), ProjectSyncMode::FullProject);
    // Ready snapshot whose only project lives at `/other` — it does NOT own
    // the open `/workspace/src/App.vue`, so owner resolution returns None.
    let vfs_workspace = crate::test_utils::make_test_vfs_workspace_with_resolver(
        "/other",
        Some("/other/tsconfig.json"),
    );
    let provider_sync_states = DashMap::new();
    provider_sync_states.insert(
        "/workspace/src/App.vue".to_string(),
        ProviderSyncState {
            owner_binding: crate::provider_sync::ProviderOwnerBinding::Unresolved,
            ide_path: Some("/workspace/src/App.vue.tsx".to_string()),
            api_path: Some("/workspace/src/App.vue.verter.ts".to_string()),
            decl_path: None,
            ide_background_loaded: true,
            api_background_loaded: true,
            decl_background_loaded: false,
            shadow_path: None,
            shadow_background_loaded: false,
            committed_ide_surface: None,
            commit_stamp: None,
            api_delivered_hash: None,
            api_observed_hash: None,
            shadow_delivered_source_hash: None,
        },
    );
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

    // Positive: the open file's provider state SURVIVES, still unresolved.
    let state = provider_sync_states
        .get("/workspace/src/App.vue")
        .map(|entry| entry.clone())
        .expect("open Vue file must keep its provider sync state across an owner-None drain");
    assert!(
        state.is_unresolved(),
        "ownership-None must not upgrade the binding; it stays unresolved, got {:?}",
        state.owner_binding
    );
    assert_eq!(
        state.ide_path.as_deref(),
        Some("/workspace/src/App.vue.tsx"),
        "the open file's IDE TSX path must be preserved"
    );

    // INV-3: the ready `/other` snapshot resolves `/workspace/src/App.vue` to a
    // TERMINAL `NoProject`, so the drain DEQUEUES it — a terminal ownership decision
    // is never retried into a provider on every drain. Editor-liveness is preserved
    // above (the open file's state survives + its TSX stays live), and re-owning on a
    // later config change is re-driven by the foreground sync's owner-mismatch
    // reconcile, not a stale drain retry.
    assert!(
        !pending_snapshot_provider_sync.contains("/workspace/src/App.vue"),
        "a terminal NoProject carrier must be dequeued (never retried into a provider)"
    );

    let calls = provider.file_sync_calls();
    // Negative: the drain must NOT close the open file's live IDE/API paths.
    assert!(
        !calls.iter().any(|call| matches!(
            call,
            MockCall::CloseFile { path }
                if path == "/workspace/src/App.vue.tsx" || path == "/workspace/src/App.vue.verter.ts"
        )),
        "owner-None drain must NOT close an open Vue file's live provider paths, calls={calls:?}"
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn drain_owned_to_unowned_open_vue_converts_state_to_unresolved() {
    // FIX-1 + R2-8: an OPEN Vue file that was previously `Owned` becomes unowned
    // when a ready snapshot resolves no owner for it. The drain MUST convert the
    // committed state to `Unresolved` (so `needs_owner_reconcile` can later
    // re-bind it) — it must NOT reuse the stale `Owned` binding (which would
    // panic the debug_assert and strand the file on a dead owner) and must NOT
    // carry the stale owner-derived `.vue.ts` API path. The live IDE TSX is
    // preserved and never closed; the dropped owner-derived `.vue.ts` IS closed
    // (R2-8 — it is an orphaned provider artifact once unowned).
    let host = Arc::new(VerterHost::new_standalone(HostConfig::default()));
    let documents = DocumentRegistry::new(Arc::clone(&host));
    let uri: Uri = "file:///workspace/src/App.vue".parse().unwrap();
    let _ = documents.did_open(&TextDocumentItem {
        uri: uri.clone(),
        language_id: "vue".to_string(),
        version: 1,
        text: "<template><div>{{ msg }}</div></template>".to_string(),
    });

    let provider = Arc::new(MockTypeProvider::new());
    let sync = ProjectSync::new(provider.clone(), ProjectSyncMode::FullProject);
    // Ready snapshot at `/other` — it does NOT own the open `/workspace` file.
    let vfs_workspace = crate::test_utils::make_test_vfs_workspace_with_resolver(
        "/other",
        Some("/other/tsconfig.json"),
    );

    let provider_sync_states = DashMap::new();
    // Prior committed state is OWNED (the FIX-1 trigger) with a stale API path.
    provider_sync_states.insert(
        "/workspace/src/App.vue".to_string(),
        ProviderSyncState {
            owner_binding: crate::provider_sync::ProviderOwnerBinding::Owned(
                "/old/tsconfig.json".to_string(),
            ),
            ide_path: Some("/workspace/src/App.vue.tsx".to_string()),
            api_path: Some("/workspace/src/App.vue.verter.ts".to_string()),
            decl_path: None,
            ide_background_loaded: true,
            api_background_loaded: true,
            decl_background_loaded: false,
            shadow_path: None,
            shadow_background_loaded: false,
            committed_ide_surface: None,
            commit_stamp: None,
            api_delivered_hash: None,
            api_observed_hash: None,
            shadow_delivered_source_hash: None,
        },
    );
    let pending_snapshot_provider_sync = DashSet::new();
    pending_snapshot_provider_sync.insert("/workspace/src/App.vue".to_string());

    // No panic (pre-fix the verter_debug_assert!(is_unresolved) fires on the Owned reuse).
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

    let state = provider_sync_states
        .get("/workspace/src/App.vue")
        .map(|entry| entry.clone())
        .expect("owned→unowned open Vue file must keep provider sync state");
    // Discriminator: pre-fix this would still be Owned("/old/tsconfig.json").
    assert!(
        state.is_unresolved(),
        "owned→unowned open Vue file must be converted to Unresolved, got {:?}",
        state.owner_binding
    );
    assert_eq!(
        state.ide_path.as_deref(),
        Some("/workspace/src/App.vue.tsx"),
        "the live IDE TSX path must be preserved across the conversion"
    );
    // Discriminator: the stale owner-derived API path must be dropped.
    assert!(
        state.api_path.is_none(),
        "the stale owner-derived `.vue.ts` API path must be dropped, got {:?}",
        state.api_path
    );

    let calls = provider.file_sync_calls();
    // Negative: the owner-INDEPENDENT live IDE TSX must NOT be closed
    // (editor-liveness keeps the open document's hover/completion working).
    assert!(
        !calls.iter().any(|call| matches!(
            call,
            MockCall::CloseFile { path } if path == "/workspace/src/App.vue.tsx"
        )),
        "owned→unowned conversion must NOT close the open file's live IDE TSX, calls={calls:?}"
    );
    // Positive (R2-8): the dropped owner-derived `.vue.ts` IS closed — once
    // unowned no project provides it, so leaving it open leaks an orphan.
    assert!(
        calls.iter().any(|call| matches!(
            call,
            MockCall::CloseFile { path } if path == "/workspace/src/App.vue.verter.ts"
        )),
        "owned→unowned conversion must CLOSE the dropped owner-derived `.vue.ts`, calls={calls:?}"
    );

    // INV-3: under the ready `/other` snapshot the owner-lost carrier resolves to a
    // TERMINAL `NoProject`, so the drain DEQUEUES it (a terminal ownership decision
    // is never retried). The owner→unowned conversion + editor-liveness are asserted
    // above; a later config change re-binds it through the foreground owner-mismatch
    // reconcile, not a stale drain retry.
    assert!(
        !pending_snapshot_provider_sync.contains("/workspace/src/App.vue"),
        "a terminal owner-lost carrier must be dequeued (never retried into a provider)"
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn open_unresolved_carrier_no_ide_output_commits_forced_unresolved_binding() {
    // R2-1: the no-IDE branch of `sync_open_unresolved_carrier_provider_file` must
    // still COMMIT the forced-`Unresolved` state when a prior committed state
    // exists — never abandon the conversion and leave a stale `Owned` binding.
    // An owned→unowned OPEN Vue file with a transient IDE compile miss (ide=None)
    // would otherwise stay stuck on a dead owner: its committed binding stays
    // `Owned` → `needs_owner_reconcile` (is_unresolved && ownership_ready) is
    // false → the file can never re-bind. The fix converts the binding to
    // `Unresolved` and drops the stale owner-derived `.vue.ts` API path while
    // preserving the live IDE TSX path (and NEVER closing it — there is no new
    // IDE code to re-sync this pass).
    let host = Arc::new(VerterHost::new_standalone(HostConfig::default()));
    let documents = DocumentRegistry::new(Arc::clone(&host));
    let uri: Uri = "file:///workspace/src/App.vue".parse().unwrap();
    let _ = documents.did_open(&TextDocumentItem {
        uri: uri.clone(),
        language_id: "vue".to_string(),
        version: 1,
        text: "<template><div>{{ msg }}</div></template>".to_string(),
    });

    let provider = Arc::new(MockTypeProvider::new());
    let sync = ProjectSync::new(provider.clone(), ProjectSyncMode::FullProject);

    let provider_sync_states = DashMap::new();
    // Prior committed state is OWNED with a live `.tsx` IDE path AND a stale
    // owner-derived `.vue.ts` API path (the owned→unowned trigger).
    provider_sync_states.insert(
        "/workspace/src/App.vue".to_string(),
        ProviderSyncState {
            owner_binding: crate::provider_sync::ProviderOwnerBinding::Owned(
                "/old/tsconfig.json".to_string(),
            ),
            ide_path: Some("/workspace/src/App.vue.tsx".to_string()),
            api_path: Some("/workspace/src/App.vue.verter.ts".to_string()),
            decl_path: None,
            ide_background_loaded: true,
            api_background_loaded: true,
            decl_background_loaded: false,
            shadow_path: None,
            shadow_background_loaded: false,
            committed_ide_surface: None,
            commit_stamp: None,
            api_delivered_hash: None,
            api_observed_hash: None,
            shadow_delivered_source_hash: None,
        },
    );

    // Drive the no-IDE branch directly: no compiled IDE output this pass.
    let synced = sync_open_unresolved_carrier_provider_file(
        &sync,
        &documents,
        &provider_sync_states,
        "/workspace/src/App.vue",
        false,
        None,
        None,
        &crate::external_ts::CarrierTransactionCoordinator::new(),
    )
    .await;
    assert!(
        !synced,
        "no-IDE preserve pass must return false so the file stays queued"
    );

    let state = provider_sync_states
        .get("/workspace/src/App.vue")
        .map(|entry| entry.clone())
        .expect("the open file's provider state must be preserved, not removed");
    // Discriminator: pre-fix this branch returned WITHOUT committing, so the
    // stale `Owned("/old/tsconfig.json")` binding survived.
    assert!(
        state.is_unresolved(),
        "no-IDE owned→unowned pass must commit a forced-Unresolved binding, got {:?}",
        state.owner_binding
    );
    // The live IDE TSX path is preserved (owner-independent artifact).
    assert_eq!(
        state.ide_path.as_deref(),
        Some("/workspace/src/App.vue.tsx"),
        "the live IDE TSX path must be preserved across the no-IDE conversion"
    );
    // The stale owner-derived API path must be dropped (no project provides it).
    assert!(
        state.api_path.is_none(),
        "the stale owner-derived `.vue.ts` API path must be dropped, got {:?}",
        state.api_path
    );

    // Negative: with no new IDE code, the live IDE TSX is NEITHER re-opened/
    // updated NOR closed — the editor-liveness invariant keeps it alive and
    // there is no fresh code to re-sync this pass.
    let calls = provider.file_sync_calls();
    assert!(
        !calls.iter().any(|call| matches!(
            call,
            MockCall::OpenFile { path, .. }
                | MockCall::UpdateFile { path, .. }
                | MockCall::LoadFile { path, .. }
                | MockCall::CloseFile { path }
            if path == "/workspace/src/App.vue.tsx"
        )),
        "no-IDE preserve pass must not touch the live IDE TSX, calls={calls:?}"
    );
    // R2-8: but the stale owner-derived `.vue.ts` dropped by the conversion MUST
    // be closed even on the no-IDE branch — the provider still holds it open and
    // it is invalid once unowned.
    assert!(
        calls.iter().any(|call| matches!(
            call,
            MockCall::CloseFile { path } if path == "/workspace/src/App.vue.verter.ts"
        )),
        "no-IDE owned→unowned conversion must close the dropped `.vue.ts`, calls={calls:?}"
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn open_unresolved_carrier_closes_dropped_owner_api_path_keeps_ide_tsx() {
    // R2-8: converting an OPEN Vue file owned→unowned drops the owner-derived
    // `.vue.ts` API path from the committed state, but the provider still holds
    // that `.vue.ts` open. The conversion MUST close the stale `.vue.ts`
    // (orphaned artifact — no project provides it once unowned) while NEVER
    // closing the owner-independent IDE `.vue.tsx` (the editor-liveness
    // invariant keeps the open document's TSX live). Drives the IDE-present
    // branch (fresh `Some(ide)` code) so the TSX is re-synced and the conversion
    // commits.
    let host = Arc::new(VerterHost::new_standalone(HostConfig::default()));
    let documents = DocumentRegistry::new(Arc::clone(&host));
    let uri: Uri = "file:///workspace/src/App.vue".parse().unwrap();
    let _ = documents.did_open(&TextDocumentItem {
        uri: uri.clone(),
        language_id: "vue".to_string(),
        version: 1,
        text: "<template><div>{{ msg }}</div></template>".to_string(),
    });

    let provider = Arc::new(MockTypeProvider::new());
    let sync = ProjectSync::new(provider.clone(), ProjectSyncMode::FullProject);

    let provider_sync_states = DashMap::new();
    // Prior committed state is OWNED with a live `.tsx` IDE path AND a stale
    // owner-derived `.vue.ts` API path (both background-loaded → the provider
    // genuinely holds both open).
    provider_sync_states.insert(
        "/workspace/src/App.vue".to_string(),
        ProviderSyncState {
            owner_binding: crate::provider_sync::ProviderOwnerBinding::Owned(
                "/old/tsconfig.json".to_string(),
            ),
            ide_path: Some("/workspace/src/App.vue.tsx".to_string()),
            api_path: Some("/workspace/src/App.vue.verter.ts".to_string()),
            decl_path: None,
            ide_background_loaded: true,
            api_background_loaded: true,
            decl_background_loaded: false,
            shadow_path: None,
            shadow_background_loaded: false,
            committed_ide_surface: None,
            commit_stamp: None,
            api_delivered_hash: None,
            api_observed_hash: None,
            shadow_delivered_source_hash: None,
        },
    );

    // Fresh IDE output this pass → the TSX is re-synced and the conversion
    // commits the forced-Unresolved state.
    let ide = verter_session::IdeResponse {
        code: std::sync::Arc::from("export default {}"),
        source_map: None,
        is_jsx: false,
        destructured_block: None,
    };
    let open_revision = documents
        .snapshot_identity(&uri)
        .expect("the open document has a live identity to pin");
    let synced = sync_open_unresolved_carrier_provider_file(
        &sync,
        &documents,
        &provider_sync_states,
        "/workspace/src/App.vue",
        false,
        Some(&ide),
        Some((&uri, &open_revision)),
        &crate::external_ts::CarrierTransactionCoordinator::new(),
    )
    .await;
    assert!(
        !synced,
        "open unresolved preserve pass returns false so the file stays queued"
    );

    // The committed state must be the converted Unresolved state: IDE TSX kept,
    // owner-derived API dropped.
    let state = provider_sync_states
        .get("/workspace/src/App.vue")
        .map(|entry| entry.clone())
        .expect("the open file's provider state must be preserved");
    assert!(state.is_unresolved(), "binding must be forced Unresolved");
    assert_eq!(
        state.ide_path.as_deref(),
        Some("/workspace/src/App.vue.tsx"),
        "the live IDE TSX path must be preserved"
    );
    assert!(
        state.api_path.is_none(),
        "the owner-derived API path is dropped"
    );

    let calls = provider.file_sync_calls();
    // Discriminator (RED pre-fix): the dropped owner-derived `.vue.ts` must be
    // CLOSED in the provider — otherwise it leaks as an untracked artifact.
    assert!(
        calls.iter().any(|call| matches!(
            call,
            MockCall::CloseFile { path } if path == "/workspace/src/App.vue.verter.ts"
        )),
        "owned→unowned conversion must CLOSE the stale `.vue.ts`, calls={calls:?}"
    );
    // Discriminator: the owner-independent IDE `.vue.tsx` must NEVER be closed
    // (closing it would kill the open document's hover/completion).
    assert!(
        !calls.iter().any(|call| matches!(
            call,
            MockCall::CloseFile { path } if path == "/workspace/src/App.vue.tsx"
        )),
        "owned→unowned conversion must NOT close the live IDE TSX, calls={calls:?}"
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn preserve_open_unresolved_carrier_failed_first_open_commits_no_dead_ide_path() {
    // R3-1 (a) [P0]: an OPEN unowned `.vue` with NO prior live IDE state whose
    // first `open_tsx` FAILS must NOT commit a `ide_path` the provider never
    // opened. A committed `ide_path = Some(p)` is a promise that hover/completion
    // can route to `p` — `active_ide_path_for_uri` hands it to the type provider.
    // Pre-fix the preserve helper committed `ide_path = Some(.tsx)` even though
    // the open failed, so `active_ide_path_for_uri` returned an UNOPENED `.tsx`
    // and queries routed to a dead TSX (the `no ide_context` failure class).
    //
    // Driven through the real foreground entry `ensure_current_file_synced`
    // under a ready snapshot that does NOT own the file, so the owner-None
    // preserve branch (which also queues the file) is exercised end to end.
    let provider = Arc::new(MockTypeProvider::new());
    let type_provider: Arc<dyn TypeProvider> = provider.clone();
    let service = make_hover_test_service_tsgo(type_provider);
    let server = service.inner();
    // Ready snapshot at `/other` — it does NOT own the open `/workspace` file.
    install_test_resolver_for_root(server, "/other", Some("/other/tsconfig.json"));

    let uri = open_test_vue(
        server,
        "/workspace/src/App.vue",
        r#"<script setup lang="ts">
const msg = 'hello'
</script>
<template><div>{{ msg }}</div></template>
"#,
    );
    let canonical_id = "/workspace/src/App.vue";

    // No prior committed provider state for this file (the open→unowned first
    // pass). Force the IDE `.tsx` open to FAIL (records the call, returns Err).
    provider.set_fail_sync_path("/workspace/src/App.vue.tsx");

    server.ensure_current_file_synced(&uri).await;

    // Reach (R3-2 discipline): the pass MUST have ATTEMPTED to open the new
    // `.tsx` (the failing mock records the open before erroring). A no-op impl
    // that returned before syncing would vacuously pass the dead-path assertion.
    let calls = provider.file_sync_calls();
    assert!(
        calls.iter().any(|call| matches!(
            call,
            MockCall::OpenFile { path, .. } | MockCall::UpdateFile { path, .. }
            if path == "/workspace/src/App.vue.tsx"
        )),
        "preserve must ATTEMPT to open the unresolved `.tsx` before failing, calls={calls:?}"
    );

    // The committed binding is forced Unresolved (it is an open unowned file)…
    let state = server
        .provider_sync_state_for_source(canonical_id)
        .expect("the open file's provider state must be committed");
    assert!(
        state.is_unresolved(),
        "an open unowned file must commit an Unresolved binding, got {:?}",
        state.owner_binding
    );
    // …and it must NOT advertise a dead IDE path: the failed open never went
    // live, so the committed `ide_path` must be None (no dead TSX to route to).
    assert!(
        state.ide_path.is_none(),
        "a failed first-open must NOT commit a dead `ide_path`, got {:?}",
        state.ide_path
    );

    // Discriminator (RED pre-fix): `active_ide_path_for_uri` must return None —
    // pre-fix it returned the unopened `/workspace/src/App.vue.tsx`.
    assert_eq!(
        server.active_ide_path_for_uri(&uri),
        None,
        "active IDE path must be None when the provider never opened the TSX (no dead path)"
    );

    // The file stays queued for a future retry (a later snapshot owner upgrade
    // or a successful re-open).
    assert!(
        server.pending_snapshot_provider_sync.contains(canonical_id),
        "a failed preserve open must keep the file queued for retry"
    );
}

/// D2: a storm of concurrent interactive repairs on ONE document (rapid
/// hover/completion sweeping) must coalesce into a SINGLE foreground repair, not
/// N concurrent recompile + carrier-gateway + provider-sync passes stampeding the
/// provider. The per-document singleflight serializes the repairs; the freshness
/// token (re-checked under the lock) lets the waiters whose trigger was already
/// resolved by the in-flight repair return without re-syncing.
///
/// RED pre-fix: with no singleflight, every concurrent caller observed
/// `has_committed_state == false` (none had committed yet) and ran the full
/// repair, so the IDE `.tsx` was opened N times for N concurrent requests.
#[tokio::test(flavor = "multi_thread")]
async fn ensure_current_file_synced_singleflights_concurrent_repairs() {
    let provider = Arc::new(MockTypeProvider::new());
    let type_provider: Arc<dyn TypeProvider> = provider.clone();
    let service = make_hover_test_service_tsgo(type_provider);
    let server = service.inner();

    let uri = open_test_vue(
        server,
        "/workspace/src/App.vue",
        r#"<script setup lang="ts">
const msg = 'hello'
</script>
<template><div>{{ msg }}</div></template>
"#,
    );
    let canonical_id = "/workspace/src/App.vue";

    let ide_syncs = |provider: &MockTypeProvider| {
        provider
            .file_sync_calls()
            .iter()
            .filter(|call| {
                matches!(
                    call,
                    MockCall::OpenFile { path, .. } | MockCall::UpdateFile { path, .. }
                        if path == "/workspace/src/App.vue.tsx"
                )
            })
            .count()
    };

    // Fire 16 concurrent repairs on the same document (the hover storm).
    let repairs: Vec<_> = (0..16)
        .map(|_| server.ensure_current_file_synced(&uri))
        .collect();
    futures_util::future::join_all(repairs).await;

    assert_eq!(
        ide_syncs(&provider),
        1,
        "16 concurrent repairs must coalesce into ONE IDE sync, calls={:?}",
        provider.file_sync_calls()
    );
    // Correctness: the single repair still committed a live IDE state.
    let state = server
        .provider_sync_state_for_source(canonical_id)
        .expect("the coalesced repair must commit a provider sync state");
    assert!(
        state.ide_background_loaded,
        "the coalesced repair must leave the IDE companion loaded"
    );

    // A subsequent sequential repair is a no-op (the freshness token holds).
    server.ensure_current_file_synced(&uri).await;
    assert_eq!(
        ide_syncs(&provider),
        1,
        "a fresh document must not re-sync on a later interactive pass, calls={:?}",
        provider.file_sync_calls()
    );
}

/// D2's interactive-repair singleflight is carrier-neutral: both framework
/// carriers and both script modes coalesce a 16-request storm to one IDE sync.
#[tokio::test(flavor = "multi_thread")]
async fn ensure_current_file_synced_singleflights_vue_svelte_ts_js_repairs() {
    let cases = [
        (
            "vue-ts",
            "/workspace/src/App.vue",
            "vue",
            "<script setup lang=\"ts\">\nconst msg = 'hello'\n</script>\n<template><div>{{ msg }}</div></template>\n",
            "/workspace/src/App.vue.tsx",
            "/workspace/src/App.vue.jsx",
        ),
        (
            "vue-js",
            "/workspace/src/App.vue",
            "vue",
            "<script setup>\nconst msg = 'hello'\n</script>\n<template><div>{{ msg }}</div></template>\n",
            "/workspace/src/App.vue.jsx",
            "/workspace/src/App.vue.tsx",
        ),
        (
            "svelte-ts",
            "/workspace/src/App.svelte",
            "svelte",
            "<script lang=\"ts\">\nlet msg = $state('hello');\n</script>\n<div>{msg}</div>\n",
            "/workspace/src/App.svelte.tsx",
            "/workspace/src/App.svelte.jsx",
        ),
        (
            "svelte-js",
            "/workspace/src/App.svelte",
            "svelte",
            "<script>\nlet msg = $state('hello');\n</script>\n<div>{msg}</div>\n",
            "/workspace/src/App.svelte.jsx",
            "/workspace/src/App.svelte.tsx",
        ),
    ];

    for (name, canonical_id, language_id, source, expected_path, forbidden_path) in cases {
        let provider = Arc::new(MockTypeProvider::new());
        let type_provider: Arc<dyn TypeProvider> = provider.clone();
        let service = make_hover_test_service_tsgo(type_provider);
        let server = service.inner();
        let uri: Uri = format!("file://{canonical_id}")
            .parse()
            .expect("valid carrier uri");
        let _ = server.documents.did_open(&TextDocumentItem {
            uri: uri.clone(),
            language_id: language_id.to_string(),
            version: 1,
            text: source.to_string(),
        });

        let (open_arrived, open_release) = provider.block_open_file(expected_path);
        let repairs = futures_util::future::join_all(
            (0..16).map(|_| server.ensure_current_file_synced(&uri)),
        );
        let release_winner = async {
            open_arrived.notified().await;
            // `join_all` polls every child before yielding; this extra yield makes
            // the queued-waiter schedule explicit and mutation-discriminating.
            tokio::task::yield_now().await;
            open_release.notify_one();
        };
        let (repairs, ()) = futures_util::future::join(repairs, release_winner).await;
        assert_eq!(repairs.len(), 16);

        let ide_syncs = || {
            provider
                .file_sync_calls()
                .iter()
                .filter(|call| {
                    matches!(
                        call,
                        MockCall::OpenFile { path, .. } | MockCall::UpdateFile { path, .. }
                            if path == expected_path
                    )
                })
                .count()
        };
        assert_eq!(
            ide_syncs(),
            1,
            "{name}: 16 concurrent repairs must coalesce into one IDE sync, calls={:?}",
            provider.file_sync_calls()
        );
        assert!(
            provider.file_sync_calls().iter().all(|call| !matches!(
                call,
                MockCall::OpenFile { path, .. } | MockCall::UpdateFile { path, .. }
                    if path == forbidden_path
            )),
            "{name}: the opposite script-mode companion must never be synced, calls={:?}",
            provider.file_sync_calls()
        );
        assert!(
            server
                .provider_sync_state_for_source(canonical_id)
                .is_some_and(|state| state.ide_background_loaded),
            "{name}: the winning repair must commit a live IDE state"
        );

        server.ensure_current_file_synced(&uri).await;
        assert_eq!(
            ide_syncs(),
            1,
            "{name}: a fresh sequential repair must remain a no-op, calls={:?}",
            provider.file_sync_calls()
        );
    }
}

#[tokio::test]
async fn did_close_sweeps_only_the_closed_documents_ide_sync_repair_lock() {
    let provider: Arc<dyn TypeProvider> = Arc::new(MockTypeProvider::new());
    let service = make_hover_test_service_tsgo(provider);
    let server = service.inner();
    let closed_uri = open_test_vue(
        server,
        "/workspace/src/Closed.vue",
        "<script setup lang=\"ts\">const closed = true</script><template><div /></template>",
    );
    let retained_uri = open_test_vue(
        server,
        "/workspace/src/Retained.vue",
        "<script setup lang=\"ts\">const retained = true</script><template><div /></template>",
    );

    server.ensure_current_file_synced(&closed_uri).await;
    server.ensure_current_file_synced(&retained_uri).await;
    assert!(
        server
            .ide_sync_repair_locks
            .has_lane("/workspace/src/Closed.vue")
            && server
                .ide_sync_repair_locks
                .has_lane("/workspace/src/Retained.vue"),
        "precondition: each touched document owns a repair lock"
    );

    super::super::lifecycle::handle_did_close(
        server,
        DidCloseTextDocumentParams {
            text_document: TextDocumentIdentifier {
                uri: closed_uri.clone(),
            },
        },
    )
    .await;

    assert!(
        !server
            .ide_sync_repair_locks
            .has_lane("/workspace/src/Closed.vue"),
        "did_close must sweep the closed document's repair lock"
    );
    assert!(
        server
            .ide_sync_repair_locks
            .has_lane("/workspace/src/Retained.vue"),
        "did_close must not sweep another open document's repair lock"
    );
}

#[tokio::test]
async fn did_close_does_not_accumulate_repair_locks_across_distinct_documents() {
    let provider: Arc<dyn TypeProvider> = Arc::new(MockTypeProvider::new());
    let service = make_hover_test_service_tsgo(provider);
    let server = service.inner();

    for index in 0..32 {
        let canonical_id = format!("/workspace/src/Transient{index}.vue");
        let uri = open_test_vue(
            server,
            &canonical_id,
            "<script setup lang=\"ts\">const value = true</script><template><div /></template>",
        );
        server.ensure_current_file_synced(&uri).await;
        assert!(
            server.ide_sync_repair_locks.has_lane(&canonical_id),
            "precondition: transient document {index} owns a repair lock"
        );

        super::super::lifecycle::handle_did_close(
            server,
            DidCloseTextDocumentParams {
                text_document: TextDocumentIdentifier { uri },
            },
        )
        .await;
        assert!(
            !server.ide_sync_repair_locks.has_lane(&canonical_id),
            "closed transient document {index} must not retain a repair lock"
        );
    }

    assert!(
        server.ide_sync_repair_locks.is_empty(),
        "distinct open/close cycles must leave no session-long repair-lock growth"
    );
}

#[tokio::test]
async fn did_close_retires_the_repair_lane_on_final_lease_drop_without_polling() {
    let provider: Arc<dyn TypeProvider> = Arc::new(MockTypeProvider::new());
    let service = make_hover_test_service_tsgo(provider);
    let server = service.inner();
    let uri = open_test_vue(
        server,
        "/workspace/src/Closing.vue",
        "<script setup lang=\"ts\">const value = true</script><template><div /></template>",
    );
    server.ensure_current_file_synced(&uri).await;
    let generation = server
        .ide_sync_repair_locks
        .open_generation("/workspace/src/Closing.vue")
        .expect("open generation");
    let retained_lease = server.ide_sync_repair_lease("/workspace/src/Closing.vue", generation);

    super::super::lifecycle::handle_did_close(
        server,
        DidCloseTextDocumentParams {
            text_document: TextDocumentIdentifier { uri: uri.clone() },
        },
    )
    .await;

    let mapped_lane = server
        .ide_sync_repair_locks
        .lane("/workspace/src/Closing.vue")
        .expect("a retained waiter prevents unsafe lane removal");
    assert!(
        Arc::ptr_eq(&mapped_lane, retained_lease.lane()),
        "did_close must not split a retained repair lane into a second mutex"
    );
    assert!(
        retained_lease.is_retired(),
        "close must retire the exact retained lane object"
    );
    drop(mapped_lane);
    drop(retained_lease);

    assert!(
        !server
            .ide_sync_repair_locks
            .has_lane("/workspace/src/Closing.vue"),
        "the final lease drop must synchronously retire the lane"
    );
}

#[tokio::test]
async fn encoded_virtual_uri_open_close_leaves_no_repair_generation_or_lane() {
    let provider: Arc<dyn TypeProvider> = Arc::new(MockTypeProvider::new());
    let service = make_hover_test_service_tsgo(provider);
    let server = service.inner();
    let virtual_uri: Uri = "verter-virtual:///tsx.tsx?sourceUri=file%3A%2F%2F%2FC%3A%2FUsers%20dev%2FEncoded%20App.vue"
        .parse()
        .expect("encoded virtual URI");

    super::super::lifecycle::handle_did_open(
        server,
        DidOpenTextDocumentParams {
            text_document: TextDocumentItem {
                uri: virtual_uri.clone(),
                language_id: "typescriptreact".to_string(),
                version: 1,
                text: "export const virtualValue = 1;".to_string(),
            },
        },
    )
    .await;
    super::super::lifecycle::handle_did_close(
        server,
        DidCloseTextDocumentParams {
            text_document: TextDocumentIdentifier { uri: virtual_uri },
        },
    )
    .await;

    assert!(
        server.ide_sync_repair_locks.is_empty(),
        "virtual documents never own carrier repair lanes: {:?}",
        server.ide_sync_repair_locks.mapped_canonical_ids()
    );
    assert!(
        server.ide_sync_repair_locks.has_no_open_generations(),
        "virtual documents never own carrier open generations"
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn stale_repair_paused_before_lease_cannot_recreate_lane_after_close() {
    let provider: Arc<dyn TypeProvider> = Arc::new(MockTypeProvider::new());
    let service = make_hover_test_service_tsgo(provider);
    let server = service.inner();
    let canonical_id = "/workspace/src/StaleRepair.vue";
    let uri = open_test_vue(
        server,
        canonical_id,
        "<script setup lang=\"ts\">const value = true</script><template><div /></template>",
    );
    server.ensure_current_file_synced(&uri).await;
    server.needs_ide_sync.insert(canonical_id.to_string());

    let (arrived, release) = server.pause_next_ide_sync_before_lease(canonical_id);
    let repair = server.ensure_current_file_synced(&uri);
    let close = async {
        arrived.notified().await;
        super::super::lifecycle::handle_did_close(
            server,
            DidCloseTextDocumentParams {
                text_document: TextDocumentIdentifier { uri: uri.clone() },
            },
        )
        .await;
        assert!(
            !server.ide_sync_repair_locks.has_lane(canonical_id),
            "close must retire its lane before the stale repair resumes"
        );
        release.notify_one();
    };
    futures_util::future::join(repair, close).await;

    assert!(
        server.documents.get(&uri).is_none(),
        "document must stay closed"
    );
    assert!(
        !server
            .ide_sync_repair_locks
            .has_open_generation(canonical_id),
        "close must retire the exact open generation"
    );
    assert!(
        !server.ide_sync_repair_locks.has_lane(canonical_id),
        "a stale post-close lease acquisition must not recreate a mapped lane"
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn stale_repair_cannot_retire_reopened_generation_in_close_open_aba() {
    let provider: Arc<dyn TypeProvider> = Arc::new(MockTypeProvider::new());
    let service = make_hover_test_service_tsgo(provider);
    let server = service.inner();
    let canonical_id = "/workspace/src/Reopened.vue";
    let uri = open_test_vue(
        server,
        canonical_id,
        "<script setup lang=\"ts\">const before = true</script><template><div /></template>",
    );
    server.ensure_current_file_synced(&uri).await;
    let old_generation = server
        .ide_sync_repair_locks
        .open_generation(canonical_id)
        .expect("old open generation");
    server.needs_ide_sync.insert(canonical_id.to_string());

    let (arrived, release) = server.pause_next_ide_sync_before_lease(canonical_id);
    let stale_repair = server.ensure_current_file_synced(&uri);
    let close_reopen = async {
        arrived.notified().await;
        super::super::lifecycle::handle_did_close(
            server,
            DidCloseTextDocumentParams {
                text_document: TextDocumentIdentifier { uri: uri.clone() },
            },
        )
        .await;
        super::super::lifecycle::handle_did_open(
            server,
            DidOpenTextDocumentParams {
                text_document: TextDocumentItem {
                    uri: uri.clone(),
                    language_id: "vue".to_string(),
                    version: 2,
                    text: "<script setup lang=\"ts\">const after = true</script><template><span /></template>"
                        .to_string(),
                },
            },
        )
        .await;
        let new_generation = server
            .ide_sync_repair_locks
            .open_generation(canonical_id)
            .expect("reopened generation");
        assert_ne!(
            new_generation, old_generation,
            "reopen must mint a distinct document generation"
        );
        release.notify_one();
        new_generation
    };
    let ((), new_generation) = futures_util::future::join(stale_repair, close_reopen).await;

    let lane = server
        .ide_sync_repair_locks
        .lane(canonical_id)
        .expect("reopened document must retain its live lane");
    assert_eq!(
        lane.generation(),
        new_generation,
        "mapped lane must belong to the reopened generation"
    );
    assert!(
        !lane.is_retired(),
        "the stale prior-generation repair must not retire the reopened lane"
    );
    drop(lane);
    assert!(
        server.documents.get(&uri).is_some(),
        "reopened document stays open"
    );
    server.ensure_current_file_synced(&uri).await;
    assert!(
        server
            .provider_sync_state_for_source(canonical_id)
            .is_some_and(|state| state.ide_background_loaded),
        "the reopened generation must remain repairable"
    );
}

/// A STRONGER interleave than `stale_repair_cannot_retire_reopened_generation_in_close_open_aba`:
/// the stale repair acquires its lane LEASE while the old generation is still open (so it
/// holds an `Arc` on the very lane object the reopen revives in place), the reopen captures
/// that lane while the close is parked inside its critical section (so the lane is revived
/// rather than replaced), and the stale repair wins the lane mutex only AFTER the revival.
/// Its stale-generation exit must not retire the revived lane — RED before the generation
/// gate on the stale path: the unguarded `retire()` retired the LIVE reopened lane (and the
/// repair-lease drop then removed its map entry), stripping the reopened document's
/// singleflight/close serialization.
#[tokio::test(flavor = "multi_thread")]
async fn stale_repair_holding_pre_close_lease_cannot_retire_revived_lane() {
    let provider: Arc<dyn TypeProvider> = Arc::new(MockTypeProvider::new());
    let service = make_hover_test_service_tsgo(provider);
    let server = service.inner();
    let canonical_id = "/workspace/src/RevivedLane.vue";
    let uri = open_test_vue(
        server,
        canonical_id,
        "<script setup lang=\"ts\">const before = true</script><template><div /></template>",
    );
    server.ensure_current_file_synced(&uri).await;
    let old_generation = server
        .ide_sync_repair_locks
        .open_generation(canonical_id)
        .expect("old open generation");
    server.needs_ide_sync.insert(canonical_id.to_string());

    let (repair_arrived, repair_release) = server.pause_next_ide_sync_after_lease(canonical_id);
    let (close_arrived, close_release) = server.pause_next_ide_sync_close_after_lock(canonical_id);

    let stale_repair = server.ensure_current_file_synced(&uri);
    let close_reopen = async {
        // The stale repair parks holding its pre-close lease (an Arc on the
        // current lane) before contending for the lane mutex.
        repair_arrived.notified().await;

        // Drive the close until it parks inside its critical section (lane
        // mutex held, generation not yet closed, lane not yet retired).
        let close = super::super::lifecycle::handle_did_close(
            server,
            DidCloseTextDocumentParams {
                text_document: TextDocumentIdentifier { uri: uri.clone() },
            },
        );
        futures_util::pin_mut!(close);
        futures_util::future::poll_fn(|cx| {
            let _ = std::future::Future::poll(close.as_mut(), cx);
            std::task::Poll::Ready(())
        })
        .await;
        close_arrived.notified().await;

        // Queue the reopen on the lane mutex while the close is parked: it
        // captures the still-live lane object, so its generation begin REVIVES
        // that lane in place instead of replacing an already-retired map entry.
        let reopen = super::super::lifecycle::handle_did_open(
            server,
            DidOpenTextDocumentParams {
                text_document: TextDocumentItem {
                    uri: uri.clone(),
                    language_id: "vue".to_string(),
                    version: 2,
                    text: "<script setup lang=\"ts\">const after = true</script><template><span /></template>"
                        .to_string(),
                },
            },
        );
        futures_util::pin_mut!(reopen);
        futures_util::future::poll_fn(|cx| {
            let _ = std::future::Future::poll(reopen.as_mut(), cx);
            std::task::Poll::Ready(())
        })
        .await;

        close_release.notify_one();
        close.as_mut().await;
        reopen.as_mut().await;
        let new_generation = server
            .ide_sync_repair_locks
            .open_generation(canonical_id)
            .expect("reopened generation");
        assert_ne!(
            new_generation, old_generation,
            "reopen must mint a distinct document generation"
        );

        // The stale repair contends for the revived lane's mutex only now —
        // after the revival — and must observe its own generation as stale.
        repair_release.notify_one();
        new_generation
    };
    let ((), new_generation) = futures_util::future::join(stale_repair, close_reopen).await;

    let lane = server
        .ide_sync_repair_locks
        .lane(canonical_id)
        .expect("reopened document must retain its live lane");
    assert_eq!(
        lane.generation(),
        new_generation,
        "mapped lane must belong to the reopened generation"
    );
    assert!(
        !lane.is_retired(),
        "a stale repair holding a pre-close lease must not retire the revived lane"
    );
    drop(lane);
    assert!(
        server.documents.get(&uri).is_some(),
        "reopened document stays open"
    );
    server.ensure_current_file_synced(&uri).await;
    assert!(
        server
            .provider_sync_state_for_source(canonical_id)
            .is_some_and(|state| state.ide_background_loaded),
        "the reopened generation must remain repairable"
    );
}

/// Owner loss through the production `ensure_current_file_synced` transition
/// retracts a previously owned carrier from the on-disk publish store.
///
/// RED before the fix: the interactive `publish_carrier_to_external_ts` early-
/// returned on the no-owner transition WITHOUT retracting — the retract inside
/// `publish_carrier` was dead in production because the no-owner branch never
/// reached it — so the stale carrier persisted in the store across owner loss. This
/// test drives the production entry (the same coverage class the original
/// BLOCKER-1's hermetic test missed by calling `publish_carrier` directly).
#[tokio::test(flavor = "multi_thread")]
async fn owner_loss_retracts_carrier_through_production_ensure_synced() {
    // Unique workspace root so the per-(host_version, ws_root) carrier store dir is
    // isolated from other tests. A unix-style absolute root round-trips cleanly
    // through `open_test_vue`'s `file://{path}` URI (canonical_id == path).
    let ws_root = unique_server_ws_root("b1_prod");
    let tsconfig = format!("{ws_root}/tsconfig.json");
    let source = format!("{ws_root}/src/App.vue");

    // A tsserver-kind server ⇒ a real `CarrierPublishCoordinator` (real backend +
    // on-disk store, mock provider). The production interactive entry
    // `ensure_current_file_synced` publishes the carrier into THIS store.
    let provider = std::sync::Arc::new(MockTypeProvider::new());
    let type_provider: Arc<dyn TypeProvider> = provider.clone();
    let service =
        make_hover_test_service_with_kind(type_provider, crate::TypeProviderKind::Tsserver);
    let server = service.inner();

    // 1. Owner-ready snapshot owning the workspace (Configured project) → the
    //    carrier resolves to a ProjectBinding and publishes.
    install_test_resolver_for_root(server, &ws_root, Some(&tsconfig));
    let uri = open_test_vue(
        server,
        &source,
        "<script setup lang=\"ts\">\nconst msg: string = 'hi'\n</script>\n\
         <template><div>{{ msg }}</div></template>\n",
    );

    server.ensure_current_file_synced(&uri).await;

    // The carrier must now be a member of its configured project in the store.
    // Capture the published provider paths so the post-retract check is path-exact
    // without hardcoding the companion suffix.
    let published_providers: Vec<String> = {
        let manifest =
            carrier_manifest_strict(&ws_root).expect("the publish must have written a manifest");
        let project = manifest
            .projects
            .get(&tsconfig)
            .expect("the owning project must have a manifest entry after a publish");
        let providers: Vec<String> = project
            .owned_sources
            .iter()
            .filter(|o| o.source_uri == source)
            .map(|o| o.provider_uri.clone())
            .collect();
        assert!(
            !providers.is_empty(),
            "after a publish under a configured owner the carrier source MUST be in \
             the store's owned set"
        );
        assert!(
            providers
                .iter()
                .any(|p| project.ready_files.contains_key(p)),
            "the published companion MUST be in ready_files (the getExternalFiles set)"
        );
        providers
    };

    // The ledger-backed `getExternalFiles` authority must agree with the on-disk
    // store: the reconciler is the single writer of BOTH, so a published source is
    // advertised in the ledger.
    assert!(
        server
            .membership_ledger()
            .expect("a tsserver server has a membership ledger")
            .is_advertised(&crate::external_ts::CanonicalSource::from(source.as_str())),
        "after a publish the source must be advertised in the ledger-backed getExternalFiles"
    );

    // 2. The owner DISAPPEARS: install a ready snapshot rooted ELSEWHERE that does
    //    NOT own this file (owner loss; ownership_ready = true).
    let other_root = unique_server_ws_root("b1_other");
    install_test_resolver_for_root(
        server,
        &other_root,
        Some(&format!("{other_root}/tsconfig.json")),
    );

    // 3. Drive the SAME production entry again. Owner loss MUST retract the carrier.
    server.ensure_current_file_synced(&uri).await;

    let manifest = carrier_manifest_strict(&ws_root);
    let still_owned = manifest
        .as_ref()
        .and_then(|manifest| manifest.projects.get(&tsconfig))
        .map(|p| p.owned_sources.iter().any(|o| o.source_uri == source))
        .unwrap_or(false);
    assert!(
        !still_owned,
        "owner loss through the production `ensure_current_file_synced` entry MUST \
         retract the carrier source from the store's owned set (it was DEAD CODE in \
         production before the fix)"
    );
    let still_ready = manifest
        .as_ref()
        .and_then(|manifest| manifest.projects.get(&tsconfig))
        .map(|p| {
            published_providers
                .iter()
                .any(|pv| p.ready_files.contains_key(pv))
        })
        .unwrap_or(false);
    assert!(
        !still_ready,
        "owner loss MUST remove the carrier's companions from ready_files so \
         getExternalFiles stops serving them — the carrier must DISAPPEAR from the \
         store manifest"
    );
    // …and the ledger-backed authority must agree: owner loss leaves the source
    // NOT advertised (the reconciler tombstoned it as the single writer of both).
    assert!(
        !server
            .membership_ledger()
            .expect("a tsserver server has a membership ledger")
            .is_advertised(&crate::external_ts::CanonicalSource::from(source.as_str())),
        "owner loss through the production entry MUST leave the source NOT advertised \
         in the ledger-backed getExternalFiles"
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn preserve_open_unresolved_carrier_failed_update_keeps_prior_live_ide_path() {
    // R3-1 (b): an OPEN unowned `.vue` with a prior LIVE IDE path whose UPDATE
    // fails must KEEP the prior live path — it is still open in the provider
    // (only the in-place update failed; the document stays usable with its
    // last-good content). The live path's loaded flag stays true, so the
    // committed state and `active_ide_path_for_uri` both retain it.
    let provider = Arc::new(MockTypeProvider::new());
    let type_provider: Arc<dyn TypeProvider> = provider.clone();
    let service = make_hover_test_service_tsgo(type_provider);
    let server = service.inner();
    install_test_resolver(server);

    let uri = open_test_vue(
        server,
        "/workspace/src/App.vue",
        r#"<script setup lang="ts">
const msg = 'hello'
</script>
<template><div>{{ msg }}</div></template>
"#,
    );
    let canonical_id = "/workspace/src/App.vue";

    // Seed a prior LIVE unresolved state: the `.tsx` is already background-loaded
    // (the provider genuinely holds it open from a prior successful pass).
    server.commit_provider_sync_state(
        canonical_id,
        ProviderSyncState {
            owner_binding: crate::provider_sync::ProviderOwnerBinding::Unresolved,
            ide_path: Some("/workspace/src/App.vue.tsx".to_string()),
            api_path: None,
            decl_path: None,
            ide_background_loaded: true,
            api_background_loaded: false,
            decl_background_loaded: false,
            shadow_path: None,
            shadow_background_loaded: false,
            committed_ide_surface: None,
            commit_stamp: None,
            api_delivered_hash: None,
            api_observed_hash: None,
            shadow_delivered_source_hash: None,
        },
    );

    // Fail the in-place update of the live `.tsx`.
    provider.set_fail_sync_path("/workspace/src/App.vue.tsx");

    let revision = server
        .documents
        .snapshot_identity(&uri)
        .expect("the open document has a live identity to pin");
    server
        .preserve_open_unresolved_carrier(
            canonical_id,
            false,
            Some("export default { updated: true }"),
            Some((&uri, &revision)),
        )
        .await;

    // Reach: the helper attempted the in-place UPDATE of the live `.tsx`.
    let calls = provider.file_sync_calls();
    assert!(
        calls.iter().any(|call| matches!(
            call,
            MockCall::UpdateFile { path, .. } if path == "/workspace/src/App.vue.tsx"
        )),
        "preserve must ATTEMPT to update the live `.tsx`, calls={calls:?}"
    );

    // The prior LIVE path is preserved — it is still open in the provider.
    let state = server
        .provider_sync_state_for_source(canonical_id)
        .expect("the open file's provider state must survive");
    assert!(state.is_unresolved(), "binding stays Unresolved");
    assert_eq!(
        state.ide_path.as_deref(),
        Some("/workspace/src/App.vue.tsx"),
        "a prior live IDE path must be preserved across a failed update, got {:?}",
        state.ide_path
    );
    assert!(
        state.ide_background_loaded,
        "the preserved live IDE path keeps its background-loaded flag"
    );
    assert_eq!(
        server.active_ide_path_for_uri(&uri),
        Some("/workspace/src/App.vue.tsx".to_string()),
        "a prior live IDE path stays the active IDE path after a failed update"
    );

    // The live `.tsx` must NEVER be closed (the document stays usable).
    assert!(
        !calls.iter().any(|call| matches!(
            call,
            MockCall::CloseFile { path } if path == "/workspace/src/App.vue.tsx"
        )),
        "a failed update must not close the live IDE TSX, calls={calls:?}"
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn preserve_open_unresolved_carrier_jsx_flip_syncs_new_tsx_and_closes_old_jsx_after_success()
{
    // R3-4 [P1]: an OPEN unowned `.vue` with a prior LIVE `.jsx` that flips to TS
    // (`is_jsx == false`) must sync the NEW code into the desired `.tsx`, NEVER
    // into the stale `.jsx`, and close the old `.jsx` ONLY AFTER the new `.tsx`
    // syncs (close-after-success). Pre-fix the preserve helper reused the prior
    // `.jsx` path (ignoring `is_jsx`), so the new TS code was synced into the
    // wrong (JSX) provider artifact and the `.tsx` was never opened.
    let provider = Arc::new(MockTypeProvider::new());
    let type_provider: Arc<dyn TypeProvider> = provider.clone();
    let service = make_hover_test_service_tsgo(type_provider);
    let server = service.inner();
    install_test_resolver(server);

    let uri = open_test_vue(
        server,
        "/workspace/src/App.vue",
        r#"<script setup lang="ts">
const msg = 'hello'
</script>
<template><div>{{ msg }}</div></template>
"#,
    );
    let canonical_id = "/workspace/src/App.vue";

    // Seed a prior LIVE `.jsx` unresolved state (the document was previously JS).
    server.commit_provider_sync_state(
        canonical_id,
        ProviderSyncState {
            owner_binding: crate::provider_sync::ProviderOwnerBinding::Unresolved,
            ide_path: Some("/workspace/src/App.vue.jsx".to_string()),
            api_path: None,
            decl_path: None,
            ide_background_loaded: true,
            api_background_loaded: false,
            decl_background_loaded: false,
            shadow_path: None,
            shadow_background_loaded: false,
            committed_ide_surface: None,
            commit_stamp: None,
            api_delivered_hash: None,
            api_observed_hash: None,
            shadow_delivered_source_hash: None,
        },
    );

    // Flip to TS: is_jsx = false → desired path is `.tsx`. Fresh IDE code; the
    // new `.tsx` open SUCCEEDS (no failure injection). A real pin, matching
    // how every production caller invokes this for an open document (see
    // `DocumentRegistry::open_compile_pin`) — this test's document IS open,
    // and the structural backstop in `record_carrier_ide_snapshot_inner`
    // correctly refuses an unpinned record for an open document.
    let revision = server
        .documents
        .snapshot_identity(&uri)
        .expect("the open document has a live identity to pin");
    server
        .preserve_open_unresolved_carrier(
            canonical_id,
            false,
            Some("export default { ts: true }"),
            Some((&uri, &revision)),
        )
        .await;

    let calls = provider.file_sync_calls();

    // Discriminator: the NEW `.tsx` must have been opened/synced…
    let new_tsx_idx = calls.iter().position(|call| {
        matches!(
            call,
            MockCall::OpenFile { path, .. } | MockCall::UpdateFile { path, .. }
                if path == "/workspace/src/App.vue.tsx"
        )
    });
    let new_tsx_idx = new_tsx_idx
        .unwrap_or_else(|| panic!("the is_jsx flip must sync the new `.tsx`, calls={calls:?}"));

    // Discriminator (RED pre-fix): the new TS code must NEVER be synced into the
    // stale `.jsx` artifact (pre-fix it was, because the prior path was reused).
    assert!(
        !calls.iter().any(|call| matches!(
            call,
            MockCall::OpenFile { path, .. } | MockCall::UpdateFile { path, .. }
                if path == "/workspace/src/App.vue.jsx"
        )),
        "the flip must NOT sync new code into the stale `.jsx`, calls={calls:?}"
    );

    // The old `.jsx` is closed AFTER the new `.tsx` syncs (close-after-success).
    let old_jsx_close_idx = calls.iter().position(|call| {
        matches!(
            call,
            MockCall::CloseFile { path } if path == "/workspace/src/App.vue.jsx"
        )
    });
    let old_jsx_close_idx = old_jsx_close_idx.unwrap_or_else(|| {
        panic!("the flipped-away `.jsx` must be closed after the new `.tsx` syncs, calls={calls:?}")
    });
    assert!(
        old_jsx_close_idx > new_tsx_idx,
        "old `.jsx` must close AFTER the new `.tsx` syncs (close-after-success), \
         tsx_idx={new_tsx_idx}, jsx_close_idx={old_jsx_close_idx}, calls={calls:?}"
    );

    // The committed state now points at the live `.tsx`.
    let state = server
        .provider_sync_state_for_source(canonical_id)
        .expect("flip must commit the new `.tsx` state");
    assert_eq!(
        state.ide_path.as_deref(),
        Some("/workspace/src/App.vue.tsx"),
        "committed IDE path must be the new `.tsx`, got {:?}",
        state.ide_path
    );
    assert!(
        state.ide_background_loaded,
        "the new `.tsx` is live after a successful flip sync"
    );
    assert_eq!(
        server.active_ide_path_for_uri(&uri),
        Some("/workspace/src/App.vue.tsx".to_string()),
        "the active IDE path follows the flip to `.tsx`"
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn preserve_open_unresolved_carrier_jsx_flip_no_ide_output_retains_prior_live_jsx() {
    // R5-1 ROW 7 (REGRESSION, P0): an OPEN unowned `.vue` with a prior LIVE
    // `.jsx` that flips to TS (`is_jsx == false`) but has NO compiled IDE output
    // this pass (a transient compile miss) must RETAIN the prior live `.jsx` —
    // it is still physically open in the provider and is the ONLY usable TSX.
    // The desired `.tsx` is queued for a later pass.
    //
    // Pre-unification the no-IDE branch ran `drop_unloaded_ide_path()` on the
    // freshly-rebuilt `.tsx` (loaded=false, since the prior `.jsx` ≠ `.tsx`),
    // committing `ide_path = None` while the `.jsx` stayed physically open →
    // `active_ide_path_for_uri` returned None → hover died, even though a live
    // `.jsx` was still serving the document.
    let provider = Arc::new(MockTypeProvider::new());
    let type_provider: Arc<dyn TypeProvider> = provider.clone();
    let service = make_hover_test_service(type_provider);
    let server = service.inner();
    install_test_resolver(server);

    let uri = open_test_vue(
        server,
        "/workspace/src/App.vue",
        r#"<script setup lang="ts">
const msg = 'hello'
</script>
<template><div>{{ msg }}</div></template>
"#,
    );
    let canonical_id = "/workspace/src/App.vue";

    // Seed a prior LIVE `.jsx` unresolved state (the document was previously JS).
    server.commit_provider_sync_state(
        canonical_id,
        ProviderSyncState {
            owner_binding: crate::provider_sync::ProviderOwnerBinding::Unresolved,
            ide_path: Some("/workspace/src/App.vue.jsx".to_string()),
            api_path: None,
            decl_path: None,
            ide_background_loaded: true,
            api_background_loaded: false,
            decl_background_loaded: false,
            shadow_path: None,
            shadow_background_loaded: false,
            committed_ide_surface: None,
            commit_stamp: None,
            api_delivered_hash: None,
            api_observed_hash: None,
            shadow_delivered_source_hash: None,
        },
    );

    // Flip to TS (is_jsx = false → desired `.tsx`) but with NO IDE code this pass.
    server
        .preserve_open_unresolved_carrier(canonical_id, false, None, None)
        .await;

    let calls = provider.file_sync_calls();

    // The committed state RETAINS the prior live `.jsx`, still loaded.
    let state = server
        .provider_sync_state_for_source(canonical_id)
        .expect("the open file's provider state must survive a no-IDE flip pass");
    assert!(state.is_unresolved(), "binding stays Unresolved");
    assert_eq!(
        state.ide_path.as_deref(),
        Some("/workspace/src/App.vue.jsx"),
        "a no-IDE flip must RETAIN the prior live `.jsx` (not drop to None), got {:?}",
        state.ide_path
    );
    assert!(
        state.ide_background_loaded,
        "the retained prior live `.jsx` keeps its background-loaded flag"
    );

    // Discriminator (RED pre-fix): `active_ide_path_for_uri` must return the
    // prior live `.jsx` — pre-fix it returned None (the committed `ide_path` was
    // dropped while the `.jsx` stayed physically open → hover dead).
    assert_eq!(
        server.active_ide_path_for_uri(&uri),
        Some("/workspace/src/App.vue.jsx".to_string()),
        "the prior live `.jsx` must stay the active IDE path through a no-IDE flip pass"
    );

    // The prior live `.jsx` must NEVER be closed (no successful new path synced).
    assert!(
        !calls.iter().any(|call| matches!(
            call,
            MockCall::CloseFile { path } if path == "/workspace/src/App.vue.jsx"
        )),
        "a no-IDE flip must not close the prior live `.jsx`, calls={calls:?}"
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn preserve_open_unresolved_carrier_jsx_flip_failed_tsx_sync_retains_prior_live_jsx() {
    // R5-1 ROW 9 (REGRESSION, P0): an OPEN unowned `.vue` with a prior LIVE
    // `.jsx` that flips to TS (`is_jsx == false`) whose new `.tsx` sync FAILS
    // must RETAIN the prior live `.jsx` — it is still physically open in the
    // provider and is the only usable TSX. The `.jsx` must NEVER be closed (no
    // successful replacement), and the desired `.tsx` is queued.
    //
    // Pre-unification the failed-sync branch ran `drop_unloaded_ide_path()` on
    // the failed `.tsx` (loaded=false), committing `ide_path = None` while the
    // `.jsx` stayed physically open → `active_ide_path_for_uri` returned None →
    // hover died. This is the exact regression this test pins.
    let provider = Arc::new(MockTypeProvider::new());
    let type_provider: Arc<dyn TypeProvider> = provider.clone();
    let service = make_hover_test_service_tsgo(type_provider);
    let server = service.inner();
    install_test_resolver(server);

    let uri = open_test_vue(
        server,
        "/workspace/src/App.vue",
        r#"<script setup lang="ts">
const msg = 'hello'
</script>
<template><div>{{ msg }}</div></template>
"#,
    );
    let canonical_id = "/workspace/src/App.vue";

    // Seed a prior LIVE `.jsx` unresolved state (the document was previously JS).
    server.commit_provider_sync_state(
        canonical_id,
        ProviderSyncState {
            owner_binding: crate::provider_sync::ProviderOwnerBinding::Unresolved,
            ide_path: Some("/workspace/src/App.vue.jsx".to_string()),
            api_path: None,
            decl_path: None,
            ide_background_loaded: true,
            api_background_loaded: false,
            decl_background_loaded: false,
            shadow_path: None,
            shadow_background_loaded: false,
            committed_ide_surface: None,
            commit_stamp: None,
            api_delivered_hash: None,
            api_observed_hash: None,
            shadow_delivered_source_hash: None,
        },
    );

    // Flip to TS with fresh IDE code, but FAIL the new `.tsx` first-open.
    provider.set_fail_sync_path("/workspace/src/App.vue.tsx");
    let revision = server
        .documents
        .snapshot_identity(&uri)
        .expect("the open document has a live identity to pin");
    server
        .preserve_open_unresolved_carrier(
            canonical_id,
            false,
            Some("export default { ts: true }"),
            Some((&uri, &revision)),
        )
        .await;

    let calls = provider.file_sync_calls();

    // Reach: the pass MUST have ATTEMPTED to open the new `.tsx` (the failing
    // mock records the open before erroring) — a no-op return would vacuously
    // pass the retention assertion below.
    assert!(
        calls.iter().any(|call| matches!(
            call,
            MockCall::OpenFile { path, .. } | MockCall::UpdateFile { path, .. }
            if path == "/workspace/src/App.vue.tsx"
        )),
        "the flip must ATTEMPT to open the new `.tsx` before failing, calls={calls:?}"
    );

    // The committed state RETAINS the prior live `.jsx`, still loaded.
    let state = server
        .provider_sync_state_for_source(canonical_id)
        .expect("the open file's provider state must survive a failed flip");
    assert!(state.is_unresolved(), "binding stays Unresolved");
    assert_eq!(
        state.ide_path.as_deref(),
        Some("/workspace/src/App.vue.jsx"),
        "a failed `.tsx` flip must RETAIN the prior live `.jsx` (not drop to None), got {:?}",
        state.ide_path
    );
    assert!(
        state.ide_background_loaded,
        "the retained prior live `.jsx` keeps its background-loaded flag"
    );

    // Discriminator (RED pre-fix): `active_ide_path_for_uri` must return the
    // prior live `.jsx` — pre-fix it returned None (committed `ide_path` dropped
    // while the `.jsx` stayed physically open → hover dead).
    assert_eq!(
        server.active_ide_path_for_uri(&uri),
        Some("/workspace/src/App.vue.jsx".to_string()),
        "the prior live `.jsx` must stay the active IDE path through a failed flip"
    );

    // The prior live `.jsx` must NEVER be closed when the replacement failed.
    assert!(
        !calls.iter().any(|call| matches!(
            call,
            MockCall::CloseFile { path } if path == "/workspace/src/App.vue.jsx"
        )),
        "a failed flip must not close the prior live `.jsx`, calls={calls:?}"
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn preserve_open_unresolved_carrier_prior_owned_jsx_flip_failed_drops_api_retains_jsx() {
    // R5-1 prior-Owned row (combined with row 9): when the prior binding was
    // `Owned` and the new `.tsx` flip sync FAILS, the owner `.vue.ts` is dropped
    // from state AND closed (R2-8, independent of the IDE outcome), while the
    // owner-INDEPENDENT prior live `.jsx` IDE path is RETAINED (still the active
    // path) and is NEVER closed.
    let provider = Arc::new(MockTypeProvider::new());
    let type_provider: Arc<dyn TypeProvider> = provider.clone();
    let service = make_hover_test_service_tsgo(type_provider);
    let server = service.inner();
    install_test_resolver(server);

    let uri = open_test_vue(
        server,
        "/workspace/src/App.vue",
        r#"<script setup lang="ts">
const msg = 'hello'
</script>
<template><div>{{ msg }}</div></template>
"#,
    );
    let canonical_id = "/workspace/src/App.vue";

    // Seed a prior OWNED state with a LIVE `.jsx` IDE path + a LIVE owner-derived
    // `.vue.ts` API path (the file was previously owned and JS).
    server.commit_provider_sync_state(
        canonical_id,
        ProviderSyncState {
            owner_binding: crate::provider_sync::ProviderOwnerBinding::Owned(
                "/old/tsconfig.json".to_string(),
            ),
            ide_path: Some("/workspace/src/App.vue.jsx".to_string()),
            api_path: Some("/workspace/src/App.vue.verter.ts".to_string()),
            decl_path: None,
            ide_background_loaded: true,
            api_background_loaded: true,
            decl_background_loaded: false,
            shadow_path: None,
            shadow_background_loaded: false,
            committed_ide_surface: None,
            commit_stamp: None,
            api_delivered_hash: None,
            api_observed_hash: None,
            shadow_delivered_source_hash: None,
        },
    );

    // Flip to TS with fresh IDE code, but FAIL the new `.tsx` first-open.
    provider.set_fail_sync_path("/workspace/src/App.vue.tsx");
    let revision = server
        .documents
        .snapshot_identity(&uri)
        .expect("the open document has a live identity to pin");
    server
        .preserve_open_unresolved_carrier(
            canonical_id,
            false,
            Some("export default { ts: true }"),
            Some((&uri, &revision)),
        )
        .await;

    let calls = provider.file_sync_calls();

    // Binding forced Unresolved; owner-derived API dropped from committed state.
    let state = server
        .provider_sync_state_for_source(canonical_id)
        .expect("the open file's provider state must survive");
    assert!(
        state.is_unresolved(),
        "an owned→unowned open file must commit an Unresolved binding, got {:?}",
        state.owner_binding
    );
    assert!(
        state.api_path.is_none(),
        "the owner-derived API path must be dropped from the committed state, got {:?}",
        state.api_path
    );

    // The owner-derived `.vue.ts` is CLOSED (R2-8, independent of the IDE failure).
    assert!(
        calls.iter().any(|call| matches!(
            call,
            MockCall::CloseFile { path } if path == "/workspace/src/App.vue.verter.ts"
        )),
        "the dropped owner-derived `.vue.ts` must be CLOSED even on a failed flip, calls={calls:?}"
    );

    // The owner-INDEPENDENT prior live `.jsx` is RETAINED and is the active path.
    assert_eq!(
        state.ide_path.as_deref(),
        Some("/workspace/src/App.vue.jsx"),
        "the owner-independent prior live `.jsx` must be retained on a failed flip, got {:?}",
        state.ide_path
    );
    assert_eq!(
        server.active_ide_path_for_uri(&uri),
        Some("/workspace/src/App.vue.jsx".to_string()),
        "the prior live `.jsx` stays the active IDE path"
    );

    // The IDE `.jsx`/`.tsx` are NEVER closed (the failed `.tsx` never went live;
    // the `.jsx` is the only usable TSX).
    assert!(
        !calls.iter().any(|call| matches!(
            call,
            MockCall::CloseFile { path }
                if path == "/workspace/src/App.vue.jsx" || path == "/workspace/src/App.vue.tsx"
        )),
        "the IDE TSX/JSX must NEVER be closed on an owned→unowned failed flip, calls={calls:?}"
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn drain_open_unresolved_carrier_no_ide_no_prior_commits_empty_unresolved() {
    // R6-3 (row 1, drain caller): with NO prior committed state AND no IDE
    // output this pass, the drain commits an EMPTY `Unresolved` state
    // (ide_path=None, binding=Unresolved) — recording the open file's
    // unresolved status (queued for retry), UNIFIED with the two
    // `preserve_open_unresolved_carrier` callers (which already commit this).
    //
    // Discriminator (RED pre-fix): the drain guarded the commit behind
    // `if previous.is_some()`, so row 1 committed NOTHING (state map empty) —
    // an open file's unresolved status was untracked on this path while the
    // preserve callers tracked it. The committed empty `Unresolved` advertises
    // NO live path (so `active_ide_path_for_uri` stays None — nothing is open)
    // and is `is_unresolved()` so `needs_owner_reconcile` picks it up.
    let host = Arc::new(VerterHost::new_standalone(HostConfig::default()));
    let documents = DocumentRegistry::new(Arc::clone(&host));
    let uri: Uri = "file:///workspace/src/App.vue".parse().unwrap();
    let _ = documents.did_open(&TextDocumentItem {
        uri: uri.clone(),
        language_id: "vue".to_string(),
        version: 1,
        text: "<template><div>{{ msg }}</div></template>".to_string(),
    });

    let provider = Arc::new(MockTypeProvider::new());
    let sync = ProjectSync::new(provider.clone(), ProjectSyncMode::FullProject);
    let provider_sync_states = DashMap::new();

    let synced = sync_open_unresolved_carrier_provider_file(
        &sync,
        &documents,
        &provider_sync_states,
        "/workspace/src/App.vue",
        false,
        None,
        None,
        &crate::external_ts::CarrierTransactionCoordinator::new(),
    )
    .await;
    assert!(!synced, "no prior state + no IDE output must return false");

    // UNIFIED: an empty `Unresolved` state is committed for the open file.
    let state = provider_sync_states
        .get("/workspace/src/App.vue")
        .map(|entry| entry.clone())
        .expect("row 1 must commit an empty Unresolved state (unified with preserve callers)");
    assert!(
        state.is_unresolved(),
        "row 1 commits a forced-Unresolved binding, got {:?}",
        state.owner_binding
    );
    assert!(
        state.ide_path.is_none(),
        "row 1 has no live IDE path to advertise, got {:?}",
        state.ide_path
    );
    assert!(state.api_path.is_none(), "row 1 has no API path");
    assert!(
        !state.ide_background_loaded,
        "row 1 advertises nothing as live in the provider"
    );

    // No prior + no IDE → nothing to open, update, or close.
    assert!(
        provider.file_sync_calls().is_empty(),
        "no prior state + no IDE output must not touch any provider file path, calls={:?}",
        provider.file_sync_calls()
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn preserve_open_unresolved_carrier_no_ide_no_prior_commits_empty_unresolved() {
    // R6-3 (row 1, server preserve caller): with NO prior committed state AND no
    // IDE code (`ide_code = None`), `Server::preserve_open_unresolved_carrier`
    // commits an EMPTY `Unresolved` state (ide_path=None, binding=Unresolved) —
    // recording the open file's unresolved status. This pins the SAME row-1
    // behavior as the drain + sync_coordinator callers (all three unified).
    let provider = Arc::new(MockTypeProvider::new());
    let type_provider: Arc<dyn TypeProvider> = provider.clone();
    let service = make_hover_test_service(type_provider);
    let server = service.inner();
    install_test_resolver(server);

    let uri = open_test_vue(
        server,
        "/workspace/src/App.vue",
        r#"<script setup lang="ts">
const msg = 'hello'
</script>
<template><div>{{ msg }}</div></template>
"#,
    );
    let canonical_id = "/workspace/src/App.vue";

    // No prior committed state seeded; no IDE code this pass.
    server
        .preserve_open_unresolved_carrier(canonical_id, false, None, None)
        .await;

    let state = server
        .provider_sync_state_for_source(canonical_id)
        .expect("row 1 must commit an empty Unresolved state");
    assert!(
        state.is_unresolved(),
        "row 1 commits a forced-Unresolved binding, got {:?}",
        state.owner_binding
    );
    assert!(
        state.ide_path.is_none(),
        "row 1 has no live IDE path to advertise, got {:?}",
        state.ide_path
    );
    assert!(state.api_path.is_none(), "row 1 has no API path");
    assert!(!state.ide_background_loaded);
    // Read-side gate agrees there is nothing live to serve.
    assert_eq!(
        server.active_ide_path_for_uri(&uri),
        None,
        "row 1 advertises no live IDE path"
    );

    // No prior + no IDE → nothing to open, update, or close in the provider.
    assert!(
        provider.file_sync_calls().is_empty(),
        "row 1 must not touch any provider file path, calls={:?}",
        provider.file_sync_calls()
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn drain_owner_transition_retains_prior_state_when_new_owner_sync_fails() {
    // FINAL DESIGN 7: owner reconciliation may move provider paths, but a
    // FAILED reconciliation must leave the previous open path alive. The drain
    // must sync the NEW owner's paths first and only close the stale paths
    // AFTER a successful sync — never close-then-sync. Here every provider
    // replacement carrier operation fails, so only the newly-created dependency
    // overlay may be rollback-closed and the prior state is retained unchanged.
    let host = Arc::new(VerterHost::new_standalone(HostConfig::default()));
    let documents = DocumentRegistry::new(Arc::clone(&host));
    let uri: Uri = "file:///workspace/src/App.vue".parse().unwrap();
    let _ = documents.did_open(&TextDocumentItem {
        uri: uri.clone(),
        language_id: "vue".to_string(),
        version: 1,
        text: "<template><div>{{ msg }}</div></template>".to_string(),
    });

    let provider = Arc::new(MockTypeProvider::new());
    let sync = ProjectSync::new(provider.clone(), ProjectSyncMode::FullProject);
    // Resolver owns the file at `/workspace` → new owner-aware state with a
    // `.tsx` IDE path (DIFFERENT from the seeded `.jsx`).
    let vfs_workspace = crate::test_utils::make_test_vfs_workspace_with_resolver(
        "/workspace",
        Some("/workspace/tsconfig.app.json"),
    );

    let provider_sync_states = DashMap::new();
    // Prior committed state from a stale owner: a DIFFERENT IDE path (.jsx)
    // plus the same API path (.ts), both already background-loaded.
    let prior_state = ProviderSyncState {
        owner_binding: crate::provider_sync::ProviderOwnerBinding::Owned(
            "/old/tsconfig.json".to_string(),
        ),
        ide_path: Some("/workspace/src/App.vue.jsx".to_string()),
        api_path: Some("/workspace/src/App.vue.verter.ts".to_string()),
        decl_path: None,
        ide_background_loaded: true,
        api_background_loaded: true,
        decl_background_loaded: false,
        shadow_path: None,
        shadow_background_loaded: false,
        committed_ide_surface: None,
        commit_stamp: None,
        api_delivered_hash: None,
        api_observed_hash: None,
        shadow_delivered_source_hash: None,
    };
    provider_sync_states.insert("/workspace/src/App.vue".to_string(), prior_state.clone());

    let pending_snapshot_provider_sync = DashSet::new();
    pending_snapshot_provider_sync.insert("/workspace/src/App.vue".to_string());

    // The dependency overlay must publish successfully so the injected failure
    // reaches the replacement IDE carrier itself.
    provider.set_fail_sync_path("/workspace/src/App.vue.tsx");
    provider.set_fail_sync_path("/workspace/src/App.vue.verter.ts");

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

    let calls = provider.file_sync_calls();
    // Reach (R3-2): the drain must have ATTEMPTED to sync the NEW owner's `.tsx`
    // (the failing mock records the open/update BEFORE returning Err) before any
    // no-close assertion. A no-op impl that returned before syncing would pass
    // the absence-of-close + state-unchanged asserts vacuously.
    assert!(
        calls.iter().any(|call| matches!(
            call,
            MockCall::OpenFile { path, .. }
                | MockCall::UpdateFile { path, .. }
                | MockCall::LoadFile { path, .. }
            if path == "/workspace/src/App.vue.tsx"
        )),
        "failed owner transition must REACH the sync and attempt the new `.tsx`, calls={calls:?}"
    );
    assert!(
        calls.iter().any(|call| matches!(
            call,
            MockCall::OpenFile { path, .. }
                | MockCall::UpdateFile { path, .. }
                | MockCall::LoadFile { path, .. }
            if path == "/workspace/src/App.vue.verter.ts"
        )),
        "failed owner transition must REACH the sync and attempt the new `.verter.ts`, calls={calls:?}"
    );
    assert!(
        calls.iter().any(|call| matches!(
            call,
            MockCall::OpenFile { path, .. }
                if path == "/workspace/src/App.vue.tsx.__verter_types.d.ts"
        )),
        "the dependency overlay must publish before the exact carrier failure, calls={calls:?}"
    );
    // Negative: the stale `.jsx` IDE path must NOT be closed, because the
    // replacement `.tsx` sync FAILED. (Pre-fix the drain closed stale paths
    // BEFORE syncing, so a CloseFile for the stale path was recorded.)
    assert!(
        !calls.iter().any(|call| matches!(
            call,
            MockCall::CloseFile { path } if path == "/workspace/src/App.vue.jsx"
        )),
        "failed owner transition must NOT close the prior IDE path, calls={calls:?}"
    );
    assert!(
        !calls.iter().any(|call| matches!(
            call,
            MockCall::CloseFile { path } if path == "/workspace/src/App.vue.verter.ts"
        )),
        "failed owner transition must NOT close the prior API path, calls={calls:?}"
    );
    assert!(
        calls.iter().all(|call| !matches!(
            call,
            MockCall::CloseFile { path }
                if path != "/workspace/src/App.vue.tsx.__verter_types.d.ts"
        )),
        "only the newly-created dependency overlay may be rollback-closed, calls={calls:?}"
    );
    // Positive: total replacement failure retains the complete prior state.
    let state = provider_sync_states
        .get("/workspace/src/App.vue")
        .map(|entry| entry.clone())
        .expect("a failed owner transition must retain the prior provider state");
    assert_eq!(
        state, prior_state,
        "failed owner transition must leave the prior state byte-for-byte unchanged"
    );

    // Positive: stays queued for a future (successful) reconciliation.
    assert!(
        pending_snapshot_provider_sync.contains("/workspace/src/App.vue"),
        "a failed owner transition should stay queued for a future reconciliation"
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn drain_owner_transition_closes_stale_path_only_after_successful_sync() {
    // FINAL DESIGN 7: on a SUCCESSFUL owner transition the drain syncs the new
    // paths first, commits, THEN closes genuinely-stale paths — and it skips
    // any stale path the new committed state still uses (a same-path rebind of
    // an owner-independent Vue artifact must never be closed).
    let host = Arc::new(VerterHost::new_standalone(HostConfig::default()));
    let documents = DocumentRegistry::new(Arc::clone(&host));
    let uri: Uri = "file:///workspace/src/App.vue".parse().unwrap();
    let _ = documents.did_open(&TextDocumentItem {
        uri: uri.clone(),
        language_id: "vue".to_string(),
        version: 1,
        text: "<template><div>{{ msg }}</div></template>".to_string(),
    });

    let provider = Arc::new(MockTypeProvider::new());
    let sync = ProjectSync::new(provider.clone(), ProjectSyncMode::FullProject);
    // Resolver owns the file at `/workspace` → new IDE path `.tsx`, API `.ts`.
    let vfs_workspace = crate::test_utils::make_test_vfs_workspace_with_resolver(
        "/workspace",
        Some("/workspace/tsconfig.app.json"),
    );

    let provider_sync_states = DashMap::new();
    // Prior committed state from a stale owner: IDE `.jsx` (DIFFERENT → genuinely
    // stale), API `.ts` (SAME → owner-independent rebind, must NOT be closed).
    provider_sync_states.insert(
        "/workspace/src/App.vue".to_string(),
        ProviderSyncState {
            owner_binding: crate::provider_sync::ProviderOwnerBinding::Owned(
                "/old/tsconfig.json".to_string(),
            ),
            ide_path: Some("/workspace/src/App.vue.jsx".to_string()),
            api_path: Some("/workspace/src/App.vue.verter.ts".to_string()),
            decl_path: None,
            ide_background_loaded: true,
            api_background_loaded: true,
            decl_background_loaded: false,
            shadow_path: None,
            shadow_background_loaded: false,
            committed_ide_surface: None,
            commit_stamp: None,
            api_delivered_hash: None,
            api_observed_hash: None,
            shadow_delivered_source_hash: None,
        },
    );

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

    let calls = provider.calls();

    // Negative: the API path `.ts` is owner-independent (same in old + new
    // state) — a same-path rebind must NOT close it. (Pre-fix the stale-set
    // included `.ts` on owner change and it was closed before sync.)
    assert!(
        !calls.iter().any(|call| matches!(
            call,
            MockCall::CloseFile { path } if path == "/workspace/src/App.vue.verter.ts"
        )),
        "a same-path API rebind must NOT close the live .ts path, calls={calls:?}"
    );

    // Find the ordering anchors: the new IDE `.tsx` sync (open or update) and
    // the stale `.jsx` close.
    let new_tsx_sync_idx = calls.iter().position(|call| {
        matches!(
            call,
            MockCall::OpenFile { path, .. } | MockCall::UpdateFile { path, .. }
                if path == "/workspace/src/App.vue.tsx"
        )
    });
    let stale_jsx_close_idx = calls.iter().position(|call| {
        matches!(
            call,
            MockCall::CloseFile { path } if path == "/workspace/src/App.vue.jsx"
        )
    });

    let new_tsx_sync_idx = new_tsx_sync_idx.unwrap_or_else(|| {
        panic!("drain must sync the new IDE .tsx path on a successful transition, calls={calls:?}")
    });
    let stale_jsx_close_idx = stale_jsx_close_idx.unwrap_or_else(|| {
        panic!("drain must close the genuinely-stale .jsx IDE path, calls={calls:?}")
    });

    // Positive: the stale `.jsx` close happens AFTER the new `.tsx` sync.
    // (Pre-fix close_stale ran BEFORE the sync, so this ordering was inverted.)
    assert!(
        stale_jsx_close_idx > new_tsx_sync_idx,
        "stale .jsx must close AFTER the new .tsx sync (close-after-sync), \
         tsx_sync_idx={new_tsx_sync_idx}, jsx_close_idx={stale_jsx_close_idx}, calls={calls:?}"
    );

    // Positive: the new owner-aware state is committed.
    let state = provider_sync_states
        .get("/workspace/src/App.vue")
        .map(|entry| entry.clone())
        .expect("successful transition should commit the new owner-aware state");
    assert!(
        !state.is_unresolved(),
        "successful transition should commit an owner-aware binding, got {:?}",
        state.owner_binding
    );
    assert_eq!(
        state.ide_path.as_deref(),
        Some("/workspace/src/App.vue.tsx"),
        "committed IDE path should be the new .tsx path"
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn sync_imported_carrier_api_lightweight_uses_unresolved_api_path_before_snapshot() {
    let temp = tempfile::tempdir().expect("temp dir");
    let workspace = temp.path().join("workspace");
    std::fs::create_dir_all(workspace.join("src")).expect("create src dir");
    std::fs::write(
        workspace.join("src/Child.vue"),
        r#"<script setup lang="ts">
defineProps<{ msg: string }>()
</script>
<template><div>{{ msg }}</div></template>
"#,
    )
    .expect("write child");

    let provider = Arc::new(MockTypeProvider::new());
    let type_provider: Arc<dyn TypeProvider> = provider.clone();
    let host = crate::test_utils::make_filesystem_test_host(&workspace);
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

    let server = service.inner();
    let child_id = crate::test_utils::canonical_test_path(&workspace.join("src/Child.vue"));

    server
        .sync_imported_carrier_api_lightweight(&child_id)
        .await;

    let calls = provider.file_sync_calls();
    let expected_api_path = format!("{child_id}.verter.ts");
    // Publish-only contract: in the pre-snapshot bootstrap window the carrier has
    // NO resolved configured project to be a member of yet, so under tsserver NO
    // carrier-companion content open reaches the provider — the carrier becomes a
    // member via the drain's re-publish once the snapshot resolves an owner.
    // Opening the unresolved `.vue.verter.ts` into tsserver here would make a
    // second content authority, which the publish-only contract forbids.
    assert!(
        !calls.iter().any(|call| matches!(
            call,
            MockCall::OpenFile { path, .. }
                | MockCall::UpdateFile { path, .. }
                | MockCall::LoadFile { path, .. }
            if path == &expected_api_path
        )),
        "tsserver pre-snapshot imported Vue API sync must NOT open the carrier companion \
         (publish-only; membership comes from the drain re-publish); calls={calls:?}"
    );

    let state = server
        .provider_sync_states
        .get(&child_id)
        .map(|entry| entry.clone())
        .expect("unresolved API sync should commit provider state");
    assert!(
        state.is_unresolved(),
        "pre-snapshot imported sync should mark the owner as unresolved"
    );
    assert_eq!(
        state.api_path.as_deref(),
        Some(expected_api_path.as_str()),
        "imported Vue API should use the canonical unresolved .vue.ts path"
    );
    assert!(
        state.api_background_loaded,
        "unresolved imported API sync should mark the API path as loaded"
    );
    assert!(
        server.pending_snapshot_provider_sync.contains(&child_id),
        "owner-aware sync should still be queued for reconciliation after snapshot discovery"
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn sync_imported_carrier_api_lightweight_opens_snapshot_api_path_for_tsserver() {
    let provider = Arc::new(MockTypeProvider::new());
    let type_provider: Arc<dyn TypeProvider> = provider.clone();
    let service = make_hover_test_service(type_provider);
    let server = service.inner();
    install_test_resolver(server);

    let child_id = "/workspace/src/Child.vue";
    let _child_uri = open_test_vue(
        server,
        child_id,
        r#"<script setup lang="ts">
defineProps<{ msg: string }>()
</script>
<template><div>{{ msg }}</div></template>
"#,
    );

    server.sync_imported_carrier_api_lightweight(child_id).await;

    let state = server
        .provider_sync_states
        .get(child_id)
        .map(|entry| entry.clone())
        .expect("snapshot imported API sync should commit provider state");
    let api_path = state
        .api_path
        .clone()
        .expect("snapshot imported API sync should record the API path");
    let calls = provider.file_sync_calls();

    // Publish-only contract: under tsserver the imported child carrier becomes a
    // configured-project MEMBER via the publish store + plugin — the LSP must NOT
    // open/update/load the synthetic companion as a second content authority. So
    // NO carrier-companion content verb reaches the provider for the API path.
    // Routing tsserver through the inferred open path would make the OpenFile
    // reappear and this assertion fail — the discriminating signal.
    assert!(
        !calls.iter().any(|call| matches!(
            call,
            MockCall::OpenFile { path, .. }
                | MockCall::UpdateFile { path, .. }
                | MockCall::LoadFile { path, .. }
            if path == &api_path
        )),
        "tsserver imported Vue API sync must NOT open/update/load the carrier companion \
         (publish-only); calls={calls:?}, api_path={api_path}"
    );
    // The committed state still marks the API store-resident (the plugin serves
    // it as a member), so the imported-carrier prewarm is observably complete.
    assert!(
        state.api_background_loaded,
        "snapshot imported API sync should mark the API path as store-resident (loaded)"
    );
}

/// Managed tsgo publishes carrier companions for the editor TypeScript consumer
/// before opening its own direct buffers. The store publication and the direct
/// tsgo admission are intentionally independent: a failed `open_dts` may leave a
/// valid editor-consumer store surface, but it must never mark that API path live
/// in tsgo provider state and must remain queued for retry.
///
/// DISCRIMINATING: the positive half proves a successful direct open is admitted;
/// the negative half proves a failed direct open is attempted but not admitted,
/// while preserving the separately-published editor membership.
#[tokio::test(flavor = "multi_thread")]
async fn open_dts_success_records_store_surface_and_failure_does_not() {
    let child_source = r#"<script setup lang="ts">
defineProps<{ msg: string }>()
</script>
<template><div>{{ msg }}</div></template>
"#;
    let child_id = "/workspace/src/Child.vue";

    // ── Positive: a successful open_dts records the carrier-API surface. ──
    {
        let provider = Arc::new(MockTypeProvider::new());
        let type_provider: Arc<dyn TypeProvider> = provider.clone();
        let service = make_hover_test_service_tsgo(type_provider);
        let server = service.inner();
        install_test_resolver(server);
        let _uri = open_test_vue(server, child_id, child_source);

        server.sync_imported_carrier_api_lightweight(child_id).await;

        let api_path = server
            .provider_sync_states
            .get(child_id)
            .and_then(|entry| entry.api_path.clone())
            .expect("the tsgo DirectOpen sync should commit the API path");

        // The provider verb fired (tsgo opens the companion directly).
        let calls = provider.file_sync_calls();
        assert!(
            calls.iter().any(|call| matches!(
                call,
                MockCall::OpenFile { path, .. } | MockCall::UpdateFile { path, .. }
                if path == &api_path
            )),
            "tsgo must open/update the carrier-API companion at {api_path}; calls={calls:?}"
        );
        let state = server
            .provider_sync_state_for_source(child_id)
            .expect("a successful direct sync must commit provider state");
        assert_eq!(
            state.api_path.as_deref(),
            Some(api_path.as_str()),
            "the successfully-opened API companion must be the committed live path"
        );
        assert!(
            state.api_background_loaded,
            "the successfully-opened API companion must be marked live"
        );

        // The shared store also records the independently-published editor surface.
        let snapshot = server
            .test_documents()
            .provider_surfaces()
            .current_snapshot(&api_path);
        let snapshot = snapshot.expect(
            "a successful open_dts MUST record the carrier-API surface in the store \
             (the verb and the record are coupled — the store is the authority)",
        );
        assert_eq!(
            snapshot.kind,
            crate::provider_surface_store::ProviderSurfaceKind::CarrierApi,
            "the recorded surface is the carrier-API kind"
        );
    }

    // ── Negative: a FAILED direct open is not admitted into tsgo state. ──
    {
        let provider = Arc::new(MockTypeProvider::new());
        // Pre-fail the carrier-API companion open: open_dts -> provider.open_file
        // returns Err for this path, so direct tsgo admission is skipped.
        let api_path = format!("{child_id}.verter.ts");
        provider.set_fail_sync_path(&api_path);
        let type_provider: Arc<dyn TypeProvider> = provider.clone();
        let service = make_hover_test_service_tsgo(type_provider);
        let server = service.inner();
        install_test_resolver(server);
        let _uri = open_test_vue(server, child_id, child_source);

        server.sync_imported_carrier_api_lightweight(child_id).await;

        // The provider verb was ATTEMPTED for the API path...
        let calls = provider.file_sync_calls();
        assert!(
            calls.iter().any(|call| matches!(
                call,
                MockCall::OpenFile { path, .. } | MockCall::UpdateFile { path, .. }
                if path == &api_path
            )),
            "the failed open was still attempted at {api_path}; calls={calls:?}"
        );
        // The editor-owned TypeScript service has its own durable store
        // publication, so that surface legitimately remains available even
        // though managed tsgo rejected its independent direct open.
        let editor_surface = server
            .test_documents()
            .provider_surfaces()
            .current_snapshot(&api_path)
            .expect("editor membership publication is independent of the direct tsgo open");
        assert_eq!(
            editor_surface.kind,
            crate::provider_surface_store::ProviderSurfaceKind::CarrierApi
        );

        let state = server
            .provider_sync_state_for_source(child_id)
            .expect("the sibling IDE success may commit a partial provider state");
        assert!(
            state.api_path.is_none() && !state.api_background_loaded,
            "a FAILED direct open must not admit the API companion into tsgo state"
        );
        assert!(
            server.pending_snapshot_provider_sync.contains(child_id),
            "a FAILED direct API open must stay queued for retry"
        );
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn sync_carrier_ide_unresolved_forces_unresolved_over_prior_owned() {
    // R2-3: `sync_carrier_ide_unresolved` is a bootstrap "unresolved" sync — it is
    // unresolved BY DEFINITION. It reuses a prior committed state (to keep the
    // background-loaded bookkeeping) but must FORCE the binding to `Unresolved`.
    // Pre-fix it only defaulted to `Unresolved` when NO state existed; a prior
    // `Owned` state was reused with its `Owned` binding and committed, which is
    // wrong for an unresolved bootstrap sync.
    let provider = Arc::new(MockTypeProvider::new());
    let type_provider: Arc<dyn TypeProvider> = provider.clone();
    let service = make_hover_test_service(type_provider);
    let server = service.inner();

    let canonical_id = "/workspace/src/App.vue";
    // Seed a prior committed OWNED state (the R2-3 trigger).
    server.commit_provider_sync_state(
        canonical_id,
        ProviderSyncState {
            owner_binding: crate::provider_sync::ProviderOwnerBinding::Owned(
                "/old/tsconfig.json".to_string(),
            ),
            ide_path: Some(format!("{canonical_id}.tsx")),
            api_path: None,
            decl_path: None,
            ide_background_loaded: true,
            api_background_loaded: false,
            decl_background_loaded: false,
            shadow_path: None,
            shadow_background_loaded: false,
            committed_ide_surface: None,
            commit_stamp: None,
            api_delivered_hash: None,
            api_observed_hash: None,
            shadow_delivered_source_hash: None,
        },
    );

    server
        .sync_carrier_ide_unresolved(canonical_id, "export const x = 1;", false, None)
        .await;

    let state = server
        .provider_sync_states
        .get(canonical_id)
        .map(|entry| entry.clone())
        .expect("bootstrap IDE sync should commit provider state");
    // Discriminator: pre-fix the binding stays `Owned("/old/tsconfig.json")`.
    assert!(
        state.is_unresolved(),
        "bootstrap sync_carrier_ide_unresolved must force an Unresolved binding, got {:?}",
        state.owner_binding
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn sync_carrier_api_unresolved_forces_unresolved_over_prior_owned() {
    // R2-3: `sync_carrier_api_unresolved` mirror — a bootstrap unresolved API sync
    // must force the binding to `Unresolved` even when reusing a prior `Owned`
    // committed state.
    let provider = Arc::new(MockTypeProvider::new());
    let type_provider: Arc<dyn TypeProvider> = provider.clone();
    let service = make_hover_test_service(type_provider);
    let server = service.inner();

    let canonical_id = "/workspace/src/App.vue";
    server.commit_provider_sync_state(
        canonical_id,
        ProviderSyncState {
            owner_binding: crate::provider_sync::ProviderOwnerBinding::Owned(
                "/old/tsconfig.json".to_string(),
            ),
            ide_path: Some(format!("{canonical_id}.tsx")),
            api_path: Some(format!("{canonical_id}.verter.ts")),
            decl_path: None,
            ide_background_loaded: true,
            api_background_loaded: true,
            decl_background_loaded: false,
            shadow_path: None,
            shadow_background_loaded: false,
            committed_ide_surface: None,
            commit_stamp: None,
            api_delivered_hash: None,
            api_observed_hash: None,
            shadow_delivered_source_hash: None,
        },
    );

    server
        .sync_carrier_api_unresolved(canonical_id, "export {};")
        .await;

    let state = server
        .provider_sync_states
        .get(canonical_id)
        .map(|entry| entry.clone())
        .expect("bootstrap API sync should commit provider state");
    // Discriminator: pre-fix the binding stays `Owned("/old/tsconfig.json")`.
    assert!(
        state.is_unresolved(),
        "bootstrap sync_carrier_api_unresolved must force an Unresolved binding, got {:?}",
        state.owner_binding
    );
    assert!(
        server
            .documents
            .provider_surfaces()
            .current_snapshot(state.api_path.as_deref().expect("bootstrap API path"))
            .is_none(),
        "without captured source bytes, binding conversion must not invent a surface"
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn background_api_sync_never_closes_the_live_ide_tsx_on_owner_change() {
    // R2-2: the `_in_background` API twin manages ONLY the API kind. An owner-key
    // change marks the (same-path) IDE `.tsx` stale via force-rebind. The pre-fix
    // path looped EVERY stale path and closed it — including the live IDE `.tsx`
    // — BEFORE syncing the API, killing hover even though it never re-syncs IDE.
    // The fix routes through the per-kind close-after-successful-sync discipline
    // with synced_kinds=[Api]: the IDE kind is reverted to its prior live path
    // and is NEVER closed here; only a genuinely-stale API path is closed (and a
    // same-path API rebind is not stale).
    let tmp = tempfile::tempdir().expect("temp dir");
    let workspace = tmp.path().join("workspace");
    std::fs::create_dir_all(workspace.join("src")).expect("create src dir");
    std::fs::write(
        workspace.join("src/App.vue"),
        r#"<script setup lang="ts">
defineProps<{ msg: string }>()
</script>
<template><div>{{ msg }}</div></template>
"#,
    )
    .expect("write App.vue");

    let host = crate::test_utils::make_filesystem_test_host(&workspace);
    let documents = Arc::new(DocumentRegistry::new(Arc::clone(&host)));
    let canonical_id = crate::test_utils::canonical_test_path(&workspace.join("src/App.vue"));
    assert!(host.ensure_loaded(&canonical_id), "App.vue should load");
    let _ = host.ensure_compiled(&canonical_id, &documents.tsx_profile.read());
    assert!(
        host.get_public_api(&canonical_id)
            .expect("public API projection")
            .is_some(),
        "compiled .vue must expose a public API for the background API sync"
    );

    let ide_path = format!("{canonical_id}.tsx");
    let api_path = format!("{canonical_id}.verter.ts");

    let provider = Arc::new(MockTypeProvider::new());
    let sync = ProjectSync::new(provider.clone(), ProjectSyncMode::FullProject);
    // A REAL resolver that owns the workspace: the background task now derives its
    // transition through the carrier-sync gateway (which resolves the owner), so the
    // owner-resolved `.vue.tsx`/`.vue.verter.ts` paths are produced internally.
    let workspace_root = crate::test_utils::canonical_test_path(&workspace);
    // A CONFIGURED owner (tsconfig): the carrier-sync gateway resolves ownership through
    // the shared `WorkspaceProjectResolver` over this published vfs, so the owner key is
    // the resolved tsconfig URI (never an inferred no-tsconfig config).
    let tsconfig = format!("{workspace_root}/tsconfig.json");
    let owner_vfs = configured_owner_vfs(&workspace_root, &tsconfig);
    let snapshot = PublishedResolverSnapshot {
        resolver: verter_resolution::ModuleResolverCore::new(vec![
            verter_workspace::ide_project_config(
                workspace_root.clone(),
                workspace_root.clone(),
                Some(tsconfig.clone()),
            ),
        ]),
        resolution_view: None,
        ownership_ready: true,
    };

    let provider_sync_states = Arc::new(DashMap::new());
    // Prior committed OWNED state under a DIFFERENT owner key, with the SAME IDE/API
    // paths the gateway will re-derive → owner_changed force-rebind marks BOTH the
    // IDE `.tsx` and API `.ts` stale.
    let prior_state = ProviderSyncState {
        owner_binding: crate::provider_sync::ProviderOwnerBinding::Owned(
            "/old/tsconfig.json".to_string(),
        ),
        ide_path: Some(ide_path.clone()),
        api_path: Some(api_path.clone()),
        decl_path: None,
        ide_background_loaded: true,
        api_background_loaded: true,
        decl_background_loaded: false,
        shadow_path: None,
        shadow_background_loaded: false,
        committed_ide_surface: None,
        commit_stamp: None,
        api_delivered_hash: None,
        api_observed_hash: None,
        shadow_delivered_source_hash: None,
    };
    provider_sync_states.insert(canonical_id.clone(), prior_state.clone());

    // Sanity: the gateway-equivalent transition marks the same-path IDE `.tsx` stale
    // (the close trigger). The task re-derives this internally; this local
    // computation only asserts the scenario is set up correctly. The re-derived owner
    // key is the resolved tsconfig URI.
    let sanity_next = crate::provider_sync::ProviderSyncState {
        owner_binding: crate::provider_sync::ProviderOwnerBinding::Owned(tsconfig.clone()),
        ide_path: Some(ide_path.clone()),
        api_path: Some(api_path.clone()),
        ..Default::default()
    };
    let sanity_transition = crate::provider_sync::prepare_sync_transition(
        &provider_sync_states,
        &canonical_id,
        sanity_next,
    );
    assert!(
        sanity_transition
            .stale_paths
            .iter()
            .any(|(kind, path)| *kind == ProviderPathKind::Ide && path == &ide_path),
        "owner-change force-rebind must mark the same-path IDE `.tsx` stale, stale={:?}",
        sanity_transition.stale_paths
    );
    sync_api_to_provider_background_task(
        sync,
        snapshot,
        Some(Arc::clone(&owner_vfs)),
        Arc::clone(&provider_sync_states),
        canonical_id.clone(),
        false,
        std::sync::Arc::new(crate::external_ts::CarrierTransactionCoordinator::new()),
        std::sync::Arc::new(dashmap::DashSet::new()),
        // The task's lane probe is answered by a standalone registry that holds no
        // open generation for this canonical, so it takes the same unserialized
        // path the task took before the shared per-document lane existed.
        Arc::clone(&documents),
    )
    .await;

    let calls = provider.file_sync_calls();
    // Discriminator: pre-fix the IDE `.tsx` was closed in the stale-paths loop.
    assert!(
        !calls.iter().any(|call| matches!(
            call,
            MockCall::CloseFile { path } if path == &ide_path
        )),
        "background API sync must NEVER close the live IDE `.tsx`, calls={calls:?}"
    );
    // The committed state retains the live IDE `.tsx` path (reverted to prior).
    let state = provider_sync_states
        .get(&canonical_id)
        .map(|entry| entry.clone())
        .expect("successful API sync must commit state");
    assert_eq!(
        state.ide_path.as_deref(),
        Some(ide_path.as_str()),
        "background API sync must retain the prior IDE `.tsx` path"
    );
    assert!(
        state.ide_background_loaded,
        "background API sync must retain the prior IDE loaded flag"
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn background_api_sync_superseded_admit_requeues_and_never_closes() {
    // Dropped-outcome + close-race: when the background API task's owned commit
    // is REFUSED by the admission gate (a newer transaction already committed a strictly-newer
    // stamp), the task must REQUEUE the source and close NOTHING — the computed stale paths may
    // be the newer transaction's live buffers.
    //
    // DISCRIMINATING: the pre-fix `let _ = admit_owned(..)` dropped the `Superseded` outcome —
    // it did NOT requeue and it ran the stale-path close UNCONDITIONALLY. This forces a stale
    // admit via a strictly-newer committed stamp and asserts the requeue happened and the stale
    // API path was NOT closed.
    let tmp = tempfile::tempdir().expect("temp dir");
    let workspace = tmp.path().join("workspace");
    std::fs::create_dir_all(workspace.join("src")).expect("create src dir");
    std::fs::write(
        workspace.join("src/App.vue"),
        r#"<script setup lang="ts">
defineProps<{ msg: string }>()
</script>
<template><div>{{ msg }}</div></template>
"#,
    )
    .expect("write App.vue");

    let host = crate::test_utils::make_filesystem_test_host(&workspace);
    let documents = Arc::new(DocumentRegistry::new(Arc::clone(&host)));
    let canonical_id = crate::test_utils::canonical_test_path(&workspace.join("src/App.vue"));
    assert!(host.ensure_loaded(&canonical_id), "App.vue should load");
    let _ = host.ensure_compiled(&canonical_id, &documents.tsx_profile.read());
    assert!(host
        .get_public_api(&canonical_id)
        .expect("public API projection")
        .is_some());

    let ide_path = format!("{canonical_id}.tsx");
    // A DIFFERENT (stale) prior API path so the task's transition yields a genuinely-stale API
    // path that the pre-fix unconditional close would have closed.
    let stale_api_path = format!("{canonical_id}.OLD.verter.ts");

    let provider = Arc::new(MockTypeProvider::new());
    let sync = ProjectSync::new(provider.clone(), ProjectSyncMode::FullProject);
    let workspace_root = crate::test_utils::canonical_test_path(&workspace);
    let tsconfig = format!("{workspace_root}/tsconfig.json");
    let owner_vfs = configured_owner_vfs(&workspace_root, &tsconfig);
    let snapshot = PublishedResolverSnapshot {
        resolver: verter_resolution::ModuleResolverCore::new(vec![
            verter_workspace::ide_project_config(
                workspace_root.clone(),
                workspace_root.clone(),
                Some(tsconfig.clone()),
            ),
        ]),
        resolution_view: None,
        ownership_ready: true,
    };

    let provider_sync_states = Arc::new(DashMap::new());
    // Prior committed state carrying a STRICTLY-NEWER stamp (a newer transaction already
    // committed) so the task's owned commit is refused (Superseded) at the admission gate.
    let prior_state = ProviderSyncState {
        owner_binding: crate::provider_sync::ProviderOwnerBinding::Owned(tsconfig.clone()),
        ide_path: Some(ide_path.clone()),
        api_path: Some(stale_api_path.clone()),
        decl_path: None,
        ide_background_loaded: true,
        api_background_loaded: true,
        decl_background_loaded: false,
        shadow_path: None,
        shadow_background_loaded: false,
        committed_ide_surface: None,
        commit_stamp: Some(crate::provider_sync::CarrierCommitStamp {
            ownership_generation: verter_workspace::workspace_snapshot::SnapshotGeneration(
                u64::MAX,
            ),
            source_revision: u64::MAX,
        }),
        api_delivered_hash: None,
        api_observed_hash: None,
        shadow_delivered_source_hash: None,
    };
    provider_sync_states.insert(canonical_id.clone(), prior_state);

    let requeue = std::sync::Arc::new(dashmap::DashSet::new());
    sync_api_to_provider_background_task(
        sync,
        snapshot,
        Some(Arc::clone(&owner_vfs)),
        Arc::clone(&provider_sync_states),
        canonical_id.clone(),
        false,
        std::sync::Arc::new(crate::external_ts::CarrierTransactionCoordinator::new()),
        Arc::clone(&requeue),
        // No open generation for this canonical in a registry that never opened
        // it, so the lane probe reports Closed and the task runs unserialized —
        // the same path it took before the shared lane existed.
        Arc::clone(&documents),
    )
    .await;

    // The refused (Superseded) commit re-queues the source for a fresh transaction (never a
    // requeue-less drop) and does NOT overwrite the newer committed state.
    assert!(
        requeue.contains(&canonical_id),
        "a Superseded background API commit must REQUEUE the source"
    );
    let surviving_rev = provider_sync_states
        .get(&canonical_id)
        .and_then(|s| s.commit_stamp.map(|c| c.source_revision));
    assert_eq!(
        surviving_rev,
        Some(u64::MAX),
        "a Superseded commit must NOT overwrite the strictly-newer committed state"
    );
    // No stale path was closed — the close is gated on Admitted.
    let calls = provider.file_sync_calls();
    assert!(
        !calls.iter().any(|call| matches!(
            call,
            MockCall::CloseFile { path } if path == &stale_api_path || path == &ide_path
        )),
        "a Superseded commit must close NOTHING (the stale paths may be a newer transaction's \
         live buffers), calls={calls:?}"
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn background_api_sync_failure_retains_prior_api_path_and_state() {
    // R2-2 (b): close-before-sync also meant an API sync failure left the old API
    // path closed. The fix syncs first; on failure nothing is committed and
    // nothing is closed — the prior state and prior API path are retained intact.
    let tmp = tempfile::tempdir().expect("temp dir");
    let workspace = tmp.path().join("workspace");
    std::fs::create_dir_all(workspace.join("src")).expect("create src dir");
    std::fs::write(
        workspace.join("src/App.vue"),
        r#"<script setup lang="ts">
defineProps<{ msg: string }>()
</script>
<template><div>{{ msg }}</div></template>
"#,
    )
    .expect("write App.vue");

    let host = crate::test_utils::make_filesystem_test_host(&workspace);
    let documents = Arc::new(DocumentRegistry::new(Arc::clone(&host)));
    let canonical_id = crate::test_utils::canonical_test_path(&workspace.join("src/App.vue"));
    assert!(host.ensure_loaded(&canonical_id), "App.vue should load");
    let _ = host.ensure_compiled(&canonical_id, &documents.tsx_profile.read());

    let ide_path = format!("{canonical_id}.tsx");
    // Prior API path is DIFFERENT from the new one so a close-before-sync of the
    // old path would be observable.
    let prior_api_path = format!("{canonical_id}.old.ts");
    let new_api_path = format!("{canonical_id}.verter.ts");

    let provider = Arc::new(MockTypeProvider::new());
    // Fail the NEW API path sync only.
    provider.set_fail_sync_path(&new_api_path);
    let sync = ProjectSync::new(provider.clone(), ProjectSyncMode::FullProject);
    // A REAL resolver owning the workspace: the background task derives the owner-
    // resolved `.vue.verter.ts` API path (== `new_api_path`) through the gateway.
    let workspace_root = crate::test_utils::canonical_test_path(&workspace);
    // A CONFIGURED owner (tsconfig): the gateway resolves ownership over this published
    // vfs through the shared `WorkspaceProjectResolver`.
    let tsconfig = format!("{workspace_root}/tsconfig.json");
    let owner_vfs = configured_owner_vfs(&workspace_root, &tsconfig);
    let snapshot = PublishedResolverSnapshot {
        resolver: verter_resolution::ModuleResolverCore::new(vec![
            verter_workspace::ide_project_config(
                workspace_root.clone(),
                workspace_root.clone(),
                Some(tsconfig.clone()),
            ),
        ]),
        resolution_view: None,
        ownership_ready: true,
    };

    let provider_sync_states = Arc::new(DashMap::new());
    // Prior committed state under a DIFFERENT owner key with a DIFFERENT API path
    // (`.old.ts`) → the gateway-derived `.verter.ts` API rebind marks `.old.ts` stale.
    let prior_state = ProviderSyncState {
        owner_binding: crate::provider_sync::ProviderOwnerBinding::Owned(
            "/old/tsconfig.json".to_string(),
        ),
        ide_path: Some(ide_path.clone()),
        api_path: Some(prior_api_path.clone()),
        decl_path: None,
        ide_background_loaded: true,
        api_background_loaded: true,
        decl_background_loaded: false,
        shadow_path: None,
        shadow_background_loaded: false,
        committed_ide_surface: None,
        commit_stamp: None,
        api_delivered_hash: None,
        api_observed_hash: None,
        shadow_delivered_source_hash: None,
    };
    provider_sync_states.insert(canonical_id.clone(), prior_state.clone());

    sync_api_to_provider_background_task(
        sync,
        snapshot,
        Some(Arc::clone(&owner_vfs)),
        Arc::clone(&provider_sync_states),
        canonical_id.clone(),
        false,
        std::sync::Arc::new(crate::external_ts::CarrierTransactionCoordinator::new()),
        std::sync::Arc::new(dashmap::DashSet::new()),
        // The task's lane probe is answered by a standalone registry that holds no
        // open generation for this canonical, so it takes the same unserialized
        // path the task took before the shared per-document lane existed.
        Arc::clone(&documents),
    )
    .await;

    let calls = provider.file_sync_calls();
    // Reach (R3-2): the background task must have ATTEMPTED to sync the NEW API
    // path (the failing mock records the open/update before erroring) before the
    // no-close assertion. A no-op impl that returned before syncing would pass
    // the absence-of-close + state-unchanged asserts vacuously.
    assert!(
        calls.iter().any(|call| matches!(
            call,
            MockCall::OpenFile { path, .. }
                | MockCall::UpdateFile { path, .. }
                | MockCall::LoadFile { path, .. }
            if path == &new_api_path
        )),
        "failed background API sync must REACH the sync and attempt the new API path, calls={calls:?}"
    );
    // Discriminator: the prior API path must NOT be closed (the new API sync
    // failed). Pre-fix the stale-paths loop closed it BEFORE syncing.
    assert!(
        !calls.iter().any(|call| matches!(
            call,
            MockCall::CloseFile { path } if path == &prior_api_path
        )),
        "a failed background API sync must NOT close the prior API path, calls={calls:?}"
    );
    // And the IDE `.tsx` must never be closed either.
    assert!(
        !calls.iter().any(|call| matches!(
            call,
            MockCall::CloseFile { path } if path == &ide_path
        )),
        "a failed background API sync must NOT close the live IDE `.tsx`, calls={calls:?}"
    );
    // The prior state is retained UNCHANGED (not committed/removed).
    let state = provider_sync_states
        .get(&canonical_id)
        .map(|entry| entry.clone())
        .expect("a failed background API sync must retain the prior state");
    assert_eq!(
        state, prior_state,
        "failed background API sync must leave the prior state byte-for-byte unchanged, got {state:?}"
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn sync_imported_carrier_api_lightweight_opens_snapshot_ide_path_for_tsgo() {
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
                type_provider_kind: crate::TypeProviderKind::Tsgo,
                type_provider_topology: crate::TypeProviderTopology::ManagedTsgo,
                mcp_port: None,
                type_provider_reason: None,
                type_provider_advisory: None,
                suppress_imported_carrier_prewarm: false,
            },
        )
    });

    let server = service.inner();
    install_test_resolver(server);

    let child_id = "/workspace/src/Child.vue";
    let _child_uri = open_test_vue(
        server,
        child_id,
        r#"<script setup lang="ts">
defineProps<{ msg: string }>()
</script>
<template><div>{{ msg }}</div></template>
"#,
    );

    server.sync_imported_carrier_api_lightweight(child_id).await;

    let state = server
        .provider_sync_states
        .get(child_id)
        .map(|entry| entry.clone())
        .expect("snapshot imported Vue sync should commit provider state");
    let ide_path = state
        .ide_path
        .clone()
        .expect("TSGO imported Vue sync should record the IDE path");
    let calls = provider.file_sync_calls();

    assert!(
            calls.iter().any(|call| matches!(
                call,
                MockCall::OpenFile { path, .. } if path == &ide_path
            )),
            "snapshot imported Vue sync should open the provider-facing IDE path for TSGO, calls={calls:?}, ide_path={ide_path}"
        );
}

// @ai-generated - Guards compiled package barrels from publishing unrelated import closure files.
#[tokio::test(flavor = "multi_thread")]
async fn barrel_eager_sync_skips_compiled_output_outside_the_reexport_carrier_graph() {
    // Usage.vue reaches a compiled package-style barrel whose public export is
    // ordinary JavaScript. The exported leaf privately imports a carrier and a
    // deeper JS dependency, but neither reference is a re-export. The provider
    // can resolve the unchanged `.mjs` graph from disk; publishing any of these
    // files would turn an import-dependency receipt into a serial file-op flood.
    let hidden = "<script setup lang=\"ts\">\ndefineProps<{ hidden: boolean }>()\n</script>\n<template><div /></template>\n";
    let entry = "export { Comp } from './Comp.mjs'\n";
    let component = "import Hidden from './Hidden.vue'\nimport './runtime.mjs'\nexport const Comp = () => Hidden\n";
    let runtime = "import './runtime-helper.mjs'\nexport const runtime = true\n";
    let runtime_helper = "export const helper = true\n";
    let usage = "<script setup lang=\"ts\">\nimport { Comp } from './dist/index.mjs'\n</script>\n<template><Comp /></template>\n";
    let (_temp, service, drain_handle, provider, workspace_id) =
        make_definition_test_server_with_kind(
            &[
                ("src/dist/Hidden.vue", "vue", hidden),
                ("src/dist/index.mjs", "javascript", entry),
                ("src/dist/Comp.mjs", "javascript", component),
                ("src/dist/runtime.mjs", "javascript", runtime),
                ("src/dist/runtime-helper.mjs", "javascript", runtime_helper),
                ("src/Usage.vue", "vue", usage),
            ],
            crate::TypeProviderKind::Tsgo,
        )
        .await;

    let server = service.inner();
    let usage_uri = workspace_uri(&workspace_id, "src/Usage.vue");
    provider.clear_calls();

    server
        .ensure_barrel_imports_synced_for_test(&usage_uri)
        .await;

    let calls = provider.file_sync_calls();
    let synced_paths: Vec<String> = calls
        .iter()
        .filter_map(|call| match call {
            MockCall::OpenFile { path, .. }
            | MockCall::OpenFileBackground { path, .. }
            | MockCall::LoadFile { path, .. }
            | MockCall::UpdateFile { path, .. }
            | MockCall::CloseFile { path } => Some(path.replace('\\', "/")),
            _ => None,
        })
        .collect();

    assert!(
        synced_paths.iter().all(|path| !path.contains("Hidden.vue")),
        "a private carrier import is not part of the barrel re-export graph; synced={synced_paths:?}"
    );
    assert!(
        synced_paths.iter().all(|path| !path.ends_with(".mjs")),
        "unchanged compiled output must stay disk-resolved instead of being pushed one file at a time; synced={synced_paths:?}"
    );

    drain_handle.abort();
    drop(service);
}

#[tokio::test(flavor = "multi_thread")]
async fn barrel_eager_sync_terminates_on_reexport_cycle_and_still_syncs_terminal() {
    // A re-export CYCLE: `a.ts` (`export * from './b'`) <-> `b.ts`
    // (`export * from './a'`), with `a.ts` ALSO re-exporting a terminal carrier
    // (`export { default as Cyc } from './Cyc.vue'`). The bounded level-BFS must
    // TERMINATE on the cycle (an unbounded walk would spin / overflow) AND still
    // reach the terminal `Cyc.vue` carrier. Discriminating on the cycle bound:
    // the test would hang (or never sync Cyc.vue) without cycle/visited tracking.
    let cyc = "<script setup lang=\"ts\">\ndefineProps<{ cyc: number }>()\n</script>\n<template><div>{{ cyc }}</div></template>\n";
    let a_barrel = "export * from './b'\nexport { default as Cyc } from './Cyc.vue'\n";
    let b_barrel = "export * from './a'\n";
    let usage = "<script setup lang=\"ts\">\nimport { Cyc } from './a'\n</script>\n<template><Cyc></Cyc></template>\n";
    // tsgo: the cycle-bounded BFS reach is engine-agnostic; tsgo makes the terminal-
    // carrier sync an observable `open_file` (tsserver is publish-only for carriers).
    let (_temp, service, drain_handle, provider, workspace_id) =
        make_definition_test_server_with_kind(
            &[
                ("src/Cyc.vue", "vue", cyc),
                ("src/a.ts", "typescript", a_barrel),
                ("src/b.ts", "typescript", b_barrel),
                ("src/Usage.vue", "vue", usage),
            ],
            crate::TypeProviderKind::Tsgo,
        )
        .await;

    let server = service.inner();
    let usage_uri = workspace_uri(&workspace_id, "src/Usage.vue");

    // If the BFS did not bound the cycle this call would not return.
    server
        .ensure_barrel_imports_synced_for_test(&usage_uri)
        .await;

    let calls = provider.file_sync_calls();
    let opened: Vec<String> = calls
        .iter()
        .filter_map(|call| match call {
            MockCall::OpenFile { path, .. } => Some(path.replace('\\', "/")),
            _ => None,
        })
        .collect();

    assert!(
        opened.iter().any(|p| p.contains("Cyc.vue")),
        "a re-export cycle must still terminate and sync the terminal carrier Cyc.vue; opened={opened:?}"
    );

    drain_handle.abort();
    drop(service);
}

#[tokio::test(flavor = "multi_thread")]
#[ignore = "install_test_resolver_for_root builds a ProjectPayload::Fallback that does not parse \
            tsconfig `paths`, so an @/-alias seed cannot resolve in this harness. The production \
            Configured-project resolver is alias-aware and the BFS routes @/ through it exactly \
            like the proven relative case above; @/ is verified end-to-end via the real-provider \
            suite / the live repro. Un-ignore once the unit harness supports a Configured project."]
async fn barrel_eager_sync_follows_aliased_reexport_hops() {
    // Same 2-hop barrel, imported through a tsconfig path alias (`@/components`). Resolution must
    // go through the shared alias-aware workspace resolver; classify-by-resolved-target then
    // follows the hops to the terminal carrier just as in the relative case.
    let tsconfig =
        "{ \"compilerOptions\": { \"baseUrl\": \".\", \"paths\": { \"@/*\": [\"src/*\"] } } }\n";
    let foo = "<script setup lang=\"ts\">\ndefineProps<{ foo: boolean; bar?: string }>()\n</script>\n<template><div>{{ foo }}</div></template>\n";
    let mid_barrel = "export { default as Foo } from './Foo.vue'\n";
    let top_barrel = "export * from './Foo'\n";
    let usage = "<script setup lang=\"ts\">\nimport { Foo } from '@/components'\n</script>\n<template><Foo></Foo></template>\n";
    let (_temp, service, drain_handle, provider, workspace_id) = make_definition_test_server(&[
        ("tsconfig.json", "json", tsconfig),
        ("src/components/Foo/Foo.vue", "vue", foo),
        ("src/components/Foo/index.ts", "typescript", mid_barrel),
        ("src/components/index.ts", "typescript", top_barrel),
        ("src/Usage.vue", "vue", usage),
    ])
    .await;

    let server = service.inner();
    let usage_uri = workspace_uri(&workspace_id, "src/Usage.vue");

    server
        .ensure_barrel_imports_synced_for_test(&usage_uri)
        .await;

    let opened: Vec<String> = provider
        .file_sync_calls()
        .iter()
        .filter_map(|call| match call {
            MockCall::OpenFile { path, .. } => Some(path.replace('\\', "/")),
            _ => None,
        })
        .collect();

    assert!(
        opened.iter().any(|p| p.contains("Foo.vue")),
        "aliased (@/) barrel import must resolve through tsconfig paths and sync the terminal carrier; opened={opened:?}"
    );

    drain_handle.abort();
    drop(service);
}

#[tokio::test(flavor = "multi_thread")]
async fn sync_imported_carrier_api_lightweight_preserves_open_unowned_state() {
    // FIX-5: the ready-no-owner arm of sync_imported_carrier_api_lightweight must
    // NOT clear+close+return for an OPEN `.vue` that is imported-by-an-open-file
    // and unowned. Pre-fix it called clear_provider_sync_state (removing state +
    // closing the live TSX) → re-triggered the no-ide_context bug on a sibling
    // path. Post-fix: open → preserve Unresolved + keep the TSX live.
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

    let server = service.inner();
    // Ready snapshot at `/other` — it does NOT own the open `/workspace` child.
    install_test_resolver_for_root(server, "/other", Some("/other/tsconfig.json"));

    let child_id = "/workspace/src/Child.vue";
    let _child_uri = open_test_vue(
        server,
        child_id,
        r#"<script setup lang="ts">
defineProps<{ msg: string }>()
</script>
<template><div>{{ msg }}</div></template>
"#,
    );

    // Seed a prior live state (as a bootstrap pass would have): unresolved with
    // a live TSX + API. The buggy arm would close BOTH and remove the entry.
    let child_tsx = format!("{child_id}.tsx");
    let child_api = format!("{child_id}.verter.ts");
    server.commit_provider_sync_state(
        child_id,
        ProviderSyncState {
            owner_binding: crate::provider_sync::ProviderOwnerBinding::Unresolved,
            ide_path: Some(child_tsx.clone()),
            api_path: Some(child_api.clone()),
            decl_path: None,
            ide_background_loaded: true,
            api_background_loaded: true,
            decl_background_loaded: false,
            shadow_path: None,
            shadow_background_loaded: false,
            committed_ide_surface: None,
            commit_stamp: None,
            api_delivered_hash: None,
            api_observed_hash: None,
            shadow_delivered_source_hash: None,
        },
    );

    server.sync_imported_carrier_api_lightweight(child_id).await;

    // Discriminator: the open child's state must SURVIVE (pre-fix it was
    // removed by clear_provider_sync_state).
    let state = server
        .provider_sync_states
        .get(child_id)
        .map(|entry| entry.clone())
        .expect("open unowned imported Vue file must keep its provider sync state");
    assert!(
        state.is_unresolved(),
        "open unowned imported Vue file must stay Unresolved, got {:?}",
        state.owner_binding
    );
    assert_eq!(
        state.ide_path.as_deref(),
        Some(child_tsx.as_str()),
        "the open child's live IDE TSX path must be preserved"
    );

    // Discriminator: the live TSX must NOT be closed.
    let calls = provider.file_sync_calls();
    assert!(
        !calls.iter().any(|call| matches!(
            call,
            MockCall::CloseFile { path } if path == &child_tsx
        )),
        "open unowned imported Vue file must NOT close its live TSX, calls={calls:?}"
    );

    // Positive: stays queued for a future owner reconciliation.
    assert!(
        server.pending_snapshot_provider_sync.contains(child_id),
        "open unowned imported Vue file must stay queued for future owner reconciliation"
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn sync_api_to_provider_retains_prior_path_when_replacement_sync_fails() {
    // Whole-class coverage: sync_api_to_provider (a live owner-resolved Vue sync
    // path reachable for OPEN files via sync_carrier_public_api_by_canonical_id) must
    // use close-AFTER-successful-sync. On an owner-key change that force-rebinds
    // the owner-independent `{src}.vue.ts`, a FAILED replacement sync must not
    // close the prior live path and must retain the prior state.
    let provider = Arc::new(MockTypeProvider::new());
    let type_provider: Arc<dyn TypeProvider> = provider.clone();
    let service = make_hover_test_service_tsgo(type_provider);
    let server = service.inner();
    install_test_resolver(server); // owns /workspace via tsconfig.json

    let uri = open_test_vue(
        server,
        "/workspace/src/App.vue",
        r#"<script setup lang="ts">
defineProps<{ msg: string }>()
</script>
<template><div>{{ msg }}</div></template>
"#,
    );

    let api_path = "/workspace/src/App.vue.verter.ts";
    // Fail the API `.vue.ts` sync.
    provider.set_fail_sync_path(api_path);
    // Seed prior state from a STALE owner key: same owner-independent `.vue.ts`
    // (force-rebind marks it stale on the owner change), already live.
    let prior_state = ProviderSyncState {
        owner_binding: crate::provider_sync::ProviderOwnerBinding::Owned(
            "/stale/tsconfig.json".to_string(),
        ),
        ide_path: None,
        api_path: Some(api_path.to_string()),
        decl_path: None,
        api_background_loaded: true,
        decl_background_loaded: false,
        ide_background_loaded: false,
        shadow_path: None,
        shadow_background_loaded: false,
        committed_ide_surface: None,
        commit_stamp: None,
        api_delivered_hash: None,
        api_observed_hash: None,
        shadow_delivered_source_hash: None,
    };
    server.commit_provider_sync_state("/workspace/src/App.vue", prior_state.clone());

    server.sync_api_to_provider(&uri).await;

    let calls = provider.file_sync_calls();
    // Reach (R3-2): `sync_api_to_provider` must have ATTEMPTED to sync the
    // `.vue.ts` (the failing mock records the open/update before erroring) before
    // the no-close assertion. A no-op impl that returned before syncing would
    // pass the absence-of-close + state-unchanged asserts vacuously.
    assert!(
        calls.iter().any(|call| matches!(
            call,
            MockCall::OpenFile { path, .. }
                | MockCall::UpdateFile { path, .. }
                | MockCall::LoadFile { path, .. }
            if path == api_path
        )),
        "failed sync_api_to_provider must REACH the sync and attempt the `.vue.ts`, calls={calls:?}"
    );
    // Discriminator: the prior live `.vue.ts` must NOT be closed (its sync failed).
    assert!(
        !calls.iter().any(|call| matches!(
            call,
            MockCall::CloseFile { path } if path == api_path
        )),
        "failed sync_api_to_provider must NOT close the prior live `.vue.ts`, calls={calls:?}"
    );
    // Positive: the prior state is retained unchanged on a fully-failed sync.
    let state = server
        .provider_sync_state_for_source("/workspace/src/App.vue")
        .expect("failed sync_api_to_provider must retain the prior state");
    assert_eq!(
        state, prior_state,
        "failed sync_api_to_provider must leave the prior state unchanged, got {state:?}"
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn ensure_current_file_synced_queues_unresolved_ide_path_for_snapshot_reconciliation() {
    // Engine-agnostic pre-snapshot unresolved-IDE state machine: route through the
    // tsgo inferred-open engine so the unresolved `.tsx` open is observable. Under
    // tsserver the carrier open is suppressed (publish-only; membership comes from
    // the drain re-publish once the snapshot resolves an owner).
    let provider = Arc::new(MockTypeProvider::new());
    let type_provider: Arc<dyn TypeProvider> = provider.clone();
    let service = make_hover_test_service_tsgo(type_provider);

    let server = service.inner();
    let uri = open_test_vue(
        server,
        "/workspace/src/App.vue",
        r#"<script setup lang="ts">
interface Action {
  label: string
  disabled: boolean
}

const actions: Action[] = [{ label: 'ok', disabled: false }]
</script>

<template>
  <button v-for="action in actions" :key="action.label" :disabled="action.disabled">
    {{ action.label }}
  </button>
</template>
"#,
    );

    server.ensure_current_file_synced(&uri).await;

    let calls = provider.file_sync_calls();
    assert!(
        calls.iter().any(|call| matches!(
            call,
            MockCall::OpenFile { path, .. } if path == "/workspace/src/App.vue.tsx"
        )),
        "pre-snapshot current-file sync should open the unresolved IDE path, calls={calls:?}"
    );

    let state = server
        .provider_sync_states
        .get("/workspace/src/App.vue")
        .map(|entry| entry.clone())
        .expect("unresolved IDE sync should commit provider state");
    assert!(
        state.is_unresolved(),
        "pre-snapshot current-file sync should mark the IDE owner as unresolved"
    );
    assert_eq!(
        state.ide_path.as_deref(),
        Some("/workspace/src/App.vue.tsx"),
        "pre-snapshot current-file sync should use the unresolved IDE path"
    );
    assert!(
        server
            .pending_snapshot_provider_sync
            .contains("/workspace/src/App.vue"),
        "pre-snapshot current-file sync should queue owner-aware reconciliation"
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn ensure_current_file_synced_preserves_open_unresolved_carrier_state_when_ready_owner_is_none(
) {
    // Editor-liveness invariant on the FOREGROUND sync path: when the ready
    // ownership snapshot resolves no owner for an OPEN Vue file, the sync must
    // keep the file's TSX live in the provider (unresolved open-document
    // state) and keep it queued — NOT clear the state and close the TSX.
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

    let server = service.inner();
    let uri = open_test_vue(
        server,
        "/workspace/src/App.vue",
        r#"<script setup lang="ts">
const msg = 'hello'
</script>

<template>
  <div>{{ msg }}</div>
</template>
"#,
    );

    server.ensure_current_file_synced(&uri).await;
    assert!(
        server
            .provider_sync_state_for_source("/workspace/src/App.vue")
            .expect("bootstrap sync should commit unresolved state")
            .is_unresolved(),
        "bootstrap sync should start from unresolved state"
    );

    provider.clear_calls();
    // Ready snapshot at `/other` — it does NOT own the open `/workspace` file.
    install_test_resolver_for_root(server, "/other", Some("/other/tsconfig.json"));

    server.ensure_current_file_synced(&uri).await;

    // Positive: the open file's provider state SURVIVES and stays unresolved.
    let state = server
        .provider_sync_state_for_source("/workspace/src/App.vue")
        .expect("open Vue file must keep its provider sync state when ready owner is None");
    assert!(
        state.is_unresolved(),
        "ownership-None must keep the open file's binding unresolved, got {:?}",
        state.owner_binding
    );
    assert_eq!(
        state.ide_path.as_deref(),
        Some("/workspace/src/App.vue.tsx"),
        "the open file's IDE TSX path must be preserved"
    );

    // Positive: stays queued for a future owner reconciliation.
    assert!(
        server
            .pending_snapshot_provider_sync
            .contains("/workspace/src/App.vue"),
        "open unresolved current-file sync should stay queued for future owner reconciliation"
    );

    // Positive: interactive type-provider lookups still resolve from committed
    // state (hover keeps working).
    assert!(
        server.type_provider_context(&uri).is_some(),
        "open unresolved Vue file must keep a live type-provider context for hover"
    );

    let calls = provider.file_sync_calls();
    // Negative: the foreground sync must NOT close the open file's live TSX.
    assert!(
        !calls.iter().any(|call| matches!(
            call,
            MockCall::CloseFile { path } if path == "/workspace/src/App.vue.tsx"
        )),
        "ready-but-unowned current-file sync must NOT close the open file's live IDE path, calls={calls:?}"
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn ensure_current_file_synced_reconciles_owned_open_vue_on_owner_loss() {
    // R2-5: the foreground freshness check computed
    // `needs_owner_reconcile = is_unresolved() && ownership_ready`, which NEVER
    // fires for a previously-`Owned` OPEN Vue that becomes owner-None (its
    // binding is still `Owned`, not unresolved). With its IDE already synced and
    // no dirty flag, it EARLY-RETURNED at the freshness check, keeping its stale
    // `Owned` binding + owner-derived `.vue.ts`. The fix also forces reconcile on
    // owner-loss/mismatch (ownership_ready AND committed `Owned` AND the live
    // snapshot resolves no owner), preserving the open file as Unresolved.
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

    let server = service.inner();
    let uri = open_test_vue(
        server,
        "/workspace/src/App.vue",
        r#"<script setup lang="ts">
const msg = 'hello'
</script>

<template>
  <div>{{ msg }}</div>
</template>
"#,
    );

    // Ready snapshot at `/other` — does NOT own the open `/workspace` file, so
    // its current owner resolves to None (the owner-loss arm).
    install_test_resolver_for_root(server, "/other", Some("/other/tsconfig.json"));
    provider.clear_calls();

    // Seed a STALE `Owned` committed state with the IDE TSX already background-
    // loaded (so `ide_already_synced` is true) AND an owner-derived `.vue.ts`.
    // No `needs_ide_sync` flag is set, so `needs_sync` is false — pre-fix the
    // freshness check early-returns here.
    server.needs_ide_sync.remove("/workspace/src/App.vue");
    server.commit_provider_sync_state(
        "/workspace/src/App.vue",
        ProviderSyncState {
            owner_binding: crate::provider_sync::ProviderOwnerBinding::Owned(
                "/stale/tsconfig.json".to_string(),
            ),
            ide_path: Some("/workspace/src/App.vue.tsx".to_string()),
            api_path: Some("/workspace/src/App.vue.verter.ts".to_string()),
            decl_path: None,
            ide_background_loaded: true,
            api_background_loaded: true,
            decl_background_loaded: false,
            shadow_path: None,
            shadow_background_loaded: false,
            committed_ide_surface: None,
            commit_stamp: None,
            api_delivered_hash: None,
            api_observed_hash: None,
            shadow_delivered_source_hash: None,
        },
    );

    server.ensure_current_file_synced(&uri).await;

    // Discriminator (RED pre-fix): the stale `Owned` binding survived the
    // early-return. Post-fix the owner-loss forces reconciliation to Unresolved.
    let state = server
        .provider_sync_state_for_source("/workspace/src/App.vue")
        .expect("open Vue file must keep its provider sync state on owner loss");
    assert!(
        state.is_unresolved(),
        "owner loss on an already-synced open `.vue` must reconcile to Unresolved \
         (not early-return on a stale Owned binding), got {:?}",
        state.owner_binding
    );
    assert_eq!(
        state.ide_path.as_deref(),
        Some("/workspace/src/App.vue.tsx"),
        "the open file's live IDE TSX path must be preserved"
    );
    assert!(
        state.api_path.is_none(),
        "the stale owner-derived `.vue.ts` must be dropped on owner loss, got {:?}",
        state.api_path
    );

    let calls = provider.file_sync_calls();
    // Negative: the live IDE TSX must NOT be closed (editor-liveness).
    assert!(
        !calls.iter().any(|call| matches!(
            call,
            MockCall::CloseFile { path } if path == "/workspace/src/App.vue.tsx"
        )),
        "owner-loss reconcile must NOT close the open file's live TSX, calls={calls:?}"
    );
    // Positive: stays queued for a future owner reconciliation.
    assert!(
        server
            .pending_snapshot_provider_sync
            .contains("/workspace/src/App.vue"),
        "owner-loss reconcile should keep the file queued for future owner reconciliation"
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn ensure_current_file_synced_does_not_rechurn_steady_state_unowned_open_vue() {
    // R3-3 [P1]: a steady-state OPEN unowned `.vue` (committed `Unresolved`, the
    // current snapshot also resolves NO owner) is FRESH — its committed binding
    // already matches the live resolution. A second foreground pass (hover /
    // completion) with no change must NOT recompile + re-sync the TSX. Pre-fix
    // `needs_owner_reconcile = state.is_unresolved() && ownership_ready` was true
    // for EVERY unowned-but-synced file, so every interactive query re-opened/
    // re-synced the provider artifact — a per-keystroke perf regression.
    let provider = Arc::new(MockTypeProvider::new());
    let type_provider: Arc<dyn TypeProvider> = provider.clone();
    let service = make_hover_test_service_tsgo(type_provider);
    let server = service.inner();
    // Ready snapshot at `/other` — does NOT own the open `/workspace` file.
    install_test_resolver_for_root(server, "/other", Some("/other/tsconfig.json"));

    let uri = open_test_vue(
        server,
        "/workspace/src/App.vue",
        r#"<script setup lang="ts">
const msg = 'hello'
</script>
<template><div>{{ msg }}</div></template>
"#,
    );

    // First pass: syncs the unresolved TSX and commits the `Unresolved` state.
    server.ensure_current_file_synced(&uri).await;
    let first = server
        .provider_sync_state_for_source("/workspace/src/App.vue")
        .expect("first pass must commit unresolved state");
    assert!(
        first.is_unresolved() && first.ide_background_loaded,
        "first pass should leave a live Unresolved IDE state, got {first:?}"
    );
    let first_calls = provider.file_sync_calls();
    assert!(
        first_calls.iter().any(|call| matches!(
            call,
            MockCall::OpenFile { path, .. } | MockCall::UpdateFile { path, .. }
            if path == "/workspace/src/App.vue.tsx"
        )),
        "first pass should sync the unresolved `.tsx`, calls={first_calls:?}"
    );

    // Second pass: NOTHING changed, owner still None. The committed `Unresolved`
    // binding matches the current (still-None) resolution → FRESH → no re-sync.
    provider.clear_calls();
    server.ensure_current_file_synced(&uri).await;

    // Discriminator (RED pre-fix): a second pass re-opened/re-synced the `.tsx`.
    let second_calls = provider.file_sync_calls();
    assert!(
        !second_calls.iter().any(|call| matches!(
            call,
            MockCall::OpenFile { path, .. } | MockCall::UpdateFile { path, .. }
            if path == "/workspace/src/App.vue.tsx"
        )),
        "a steady-state unowned open `.vue` must NOT re-sync its TSX on a second \
         no-change pass (no churn), calls={second_calls:?}"
    );
    // And nothing else churns either.
    assert!(
        second_calls.is_empty(),
        "a steady-state no-change pass must issue NO provider file ops, calls={second_calls:?}"
    );

    // The committed state is unchanged (still live Unresolved `.tsx`).
    let second = server
        .provider_sync_state_for_source("/workspace/src/App.vue")
        .expect("state survives the second pass");
    assert!(
        second.is_unresolved(),
        "steady-state binding stays Unresolved, got {:?}",
        second.owner_binding
    );
    assert_eq!(
        second.ide_path.as_deref(),
        Some("/workspace/src/App.vue.tsx"),
        "the live IDE path is unchanged across the no-churn pass"
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn did_change_does_not_eager_sync_ready_unowned_file_through_resolver_path() {
    let provider = Arc::new(MockTypeProvider::new());
    let type_provider: Arc<dyn TypeProvider> = provider.clone();
    let service = make_hover_test_service(type_provider);
    let server = service.inner();
    install_test_resolver_for_root(server, "/other", Some("/other/tsconfig.json"));

    let uri = open_test_vue(
        server,
        "/workspace/src/App.vue",
        r#"<script setup lang="ts">
const msg = 'hello'
</script>

<template>
  <div>{{ msg }}</div>
</template>
"#,
    );

    tower_lsp_server::LanguageServer::did_change(
        server,
        DidChangeTextDocumentParams {
            text_document: VersionedTextDocumentIdentifier {
                uri: uri.clone(),
                version: 2,
            },
            content_changes: vec![TextDocumentContentChangeEvent {
                range: None,
                range_length: None,
                text: r#"<script setup lang="ts">
const msg = 'updated'
</script>

<template>
  <div>{{ msg }}</div>
</template>
"#
                .to_string(),
            }],
        },
    )
    .await;

    let calls = provider.file_sync_calls();
    assert!(
        calls.is_empty(),
        "did_change must not eagerly sync a ready-but-unowned file through a raw resolver path, calls={calls:?}"
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn current_file_sync_reopens_when_live_ide_path_changes() {
    let provider = Arc::new(MockTypeProvider::new());
    let type_provider: Arc<dyn TypeProvider> = provider.clone();
    let service = make_hover_test_service_tsgo(type_provider);
    let server = service.inner();
    install_test_resolver(server);

    let uri = open_test_vue(
        server,
        "/workspace/src/App.vue",
        r#"<script setup lang="ts">
const msg = 'hello'
</script>

<template>
  <div>{{ msg }}</div>
</template>
"#,
    );

    server.ensure_current_file_synced(&uri).await;
    assert_eq!(
        server
            .provider_sync_state_for_source("/workspace/src/App.vue")
            .and_then(|state| state.ide_path),
        Some("/workspace/src/App.vue.tsx".to_string()),
        "initial sync should materialize the TSX path"
    );

    provider.clear_calls();
    tower_lsp_server::LanguageServer::did_change(
        server,
        DidChangeTextDocumentParams {
            text_document: VersionedTextDocumentIdentifier {
                uri: uri.clone(),
                version: 2,
            },
            content_changes: vec![TextDocumentContentChangeEvent {
                range: None,
                range_length: None,
                text: r#"<script setup lang="js">
const msg = 'updated'
</script>

<template>
  <div>{{ msg }}</div>
</template>
"#
                .to_string(),
            }],
        },
    )
    .await;

    let eager_calls = provider.file_sync_calls();
    assert!(
        !eager_calls.iter().any(|call| matches!(
            call,
            MockCall::OpenFile { path, .. } | MockCall::UpdateFile { path, .. }
                if path == "/workspace/src/App.vue.tsx"
        )),
        "did_change must not eagerly sync the stale TSX path after the live IDE path changes, calls={eager_calls:?}"
    );

    server.ensure_current_file_synced(&uri).await;

    let state = server
        .provider_sync_state_for_source("/workspace/src/App.vue")
        .expect("inline sync should commit the updated IDE path");
    assert_eq!(
        state.ide_path.as_deref(),
        Some("/workspace/src/App.vue.jsx"),
        "inline sync should switch the committed IDE path to JSX"
    );
    assert!(
        state.ide_background_loaded,
        "the new JSX path should be marked as loaded"
    );

    let calls = provider.file_sync_calls();
    assert!(
        calls.iter().any(|call| matches!(
            call,
            MockCall::CloseFile { path } if path == "/workspace/src/App.vue.tsx"
        )),
        "path change should close the stale TSX path, calls={calls:?}"
    );
    assert!(
        calls.iter().any(|call| matches!(
            call,
            MockCall::OpenFile { path, .. } if path == "/workspace/src/App.vue.jsx"
        )),
        "path change should open the new JSX path, calls={calls:?}"
    );
    assert!(
        !calls.iter().any(|call| matches!(
            call,
            MockCall::UpdateFile { path, .. } if path == "/workspace/src/App.vue.jsx"
        )),
        "path change should not treat the new JSX path as an already-open file, calls={calls:?}"
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn current_file_sync_retains_old_ide_path_when_new_sync_fails() {
    // FIX-7: the FOREGROUND ensure_current_file_synced must open/sync the NEW
    // IDE path FIRST, commit on success, THEN close the old path. On a jsx→tsx
    // transition whose new `.tsx` sync FAILS, the old `.jsx` must NOT be closed
    // and committed state must not be left pointing at the unsynced `.tsx`.
    // Pre-fix it closed the old path BEFORE the (failing) open.
    let provider = Arc::new(MockTypeProvider::new());
    let type_provider: Arc<dyn TypeProvider> = provider.clone();
    let service = make_hover_test_service_tsgo(type_provider);
    let server = service.inner();
    install_test_resolver(server);

    // Open as JS → committed live `.jsx` IDE path.
    let uri = open_test_vue(
        server,
        "/workspace/src/App.vue",
        r#"<script setup lang="js">
const msg = 'hello'
</script>
<template><div>{{ msg }}</div></template>
"#,
    );
    server.ensure_current_file_synced(&uri).await;
    assert_eq!(
        server
            .provider_sync_state_for_source("/workspace/src/App.vue")
            .and_then(|state| state.ide_path),
        Some("/workspace/src/App.vue.jsx".to_string()),
        "initial JS sync should materialize the .jsx IDE path"
    );

    // Change to TS → desired IDE path becomes `.tsx`. Fail the `.tsx` sync.
    provider.set_fail_sync_path("/workspace/src/App.vue.tsx");
    provider.clear_calls();
    tower_lsp_server::LanguageServer::did_change(
        server,
        DidChangeTextDocumentParams {
            text_document: VersionedTextDocumentIdentifier {
                uri: uri.clone(),
                version: 2,
            },
            content_changes: vec![TextDocumentContentChangeEvent {
                range: None,
                range_length: None,
                text: r#"<script setup lang="ts">
const msg = 'updated'
</script>
<template><div>{{ msg }}</div></template>
"#
                .to_string(),
            }],
        },
    )
    .await;

    server.ensure_current_file_synced(&uri).await;

    let calls = provider.file_sync_calls();
    // Reach (R3-2): the foreground pass must have ATTEMPTED to open the new
    // `.tsx` (the failing mock records the open before erroring) before the
    // no-close assertion. A no-op impl that returned before the open attempt
    // would pass the absence-of-close + state-retention asserts vacuously.
    assert!(
        calls.iter().any(|call| matches!(
            call,
            MockCall::OpenFile { path, .. } | MockCall::UpdateFile { path, .. }
            if path == "/workspace/src/App.vue.tsx"
        )),
        "failed foreground IDE transition must REACH the open and attempt the new `.tsx`, calls={calls:?}"
    );
    // Discriminator: the old live `.jsx` must NOT be closed because the new
    // `.tsx` sync failed. (Pre-fix the close ran BEFORE the open attempt.)
    assert!(
        !calls.iter().any(|call| matches!(
            call,
            MockCall::CloseFile { path } if path == "/workspace/src/App.vue.jsx"
        )),
        "failed foreground IDE transition must NOT close the old .jsx path, calls={calls:?}"
    );
    // Discriminator: committed state must NOT be left on the unsynced `.tsx`.
    let state = server
        .provider_sync_state_for_source("/workspace/src/App.vue")
        .expect("foreground sync should retain a committed state");
    assert_ne!(
        state.ide_path.as_deref(),
        Some("/workspace/src/App.vue.tsx"),
        "committed IDE path must not be left on the unsynced .tsx, got {:?}",
        state.ide_path
    );
    assert_eq!(
        state.ide_path.as_deref(),
        Some("/workspace/src/App.vue.jsx"),
        "the old live .jsx path must be retained as committed on failure"
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn current_file_needs_inline_type_provider_sync_when_matching_ide_path_is_not_loaded() {
    let provider = Arc::new(MockTypeProvider::new());
    let type_provider: Arc<dyn TypeProvider> = provider.clone();
    let service = make_hover_test_service(type_provider);
    let server = service.inner();
    install_test_resolver(server);

    let uri = open_test_vue(
        server,
        "/workspace/src/App.vue",
        r#"<script setup lang="ts">
const msg = 'hello'
</script>

<template>
  <div>{{ msg }}</div>
</template>
"#,
    );

    server.provider_sync_states.insert(
        "/workspace/src/App.vue".to_string(),
        ProviderSyncState {
            owner_binding: crate::provider_sync::ProviderOwnerBinding::Owned(
                "/workspace/tsconfig.json".to_string(),
            ),
            ide_path: Some("/workspace/src/App.vue.tsx".to_string()),
            api_path: Some("/workspace/src/App.vue.verter.ts".to_string()),
            decl_path: None,
            ide_background_loaded: false,
            api_background_loaded: true,
            decl_background_loaded: false,
            shadow_path: None,
            shadow_background_loaded: false,
            committed_ide_surface: None,
            commit_stamp: None,
            api_delivered_hash: None,
            api_observed_hash: None,
            shadow_delivered_source_hash: None,
        },
    );

    assert!(
            server.current_file_needs_inline_type_provider_sync(&uri),
            "matching IDE paths must still trigger inline sync until the TSX file has been opened in the provider"
        );
}

#[tokio::test(flavor = "multi_thread")]
async fn ensure_current_file_synced_marks_matching_ide_path_loaded() {
    let provider = Arc::new(MockTypeProvider::new());
    let type_provider: Arc<dyn TypeProvider> = provider.clone();
    let service = make_hover_test_service(type_provider);
    let server = service.inner();
    install_test_resolver(server);

    let uri = open_test_vue(
        server,
        "/workspace/src/App.vue",
        r#"<script setup lang="ts">
const msg = 'hello'
</script>

<template>
  <div>{{ msg }}</div>
</template>
"#,
    );

    server.ensure_current_file_synced(&uri).await;

    let state = server
        .provider_sync_state_for_source("/workspace/src/App.vue")
        .expect("current-file sync should commit provider state");
    assert!(
        state.ide_background_loaded,
        "successful current-file sync should mark the IDE path as loaded"
    );
    assert!(
        !server.current_file_needs_inline_type_provider_sync(&uri),
        "matching loaded IDE paths should not keep triggering inline sync"
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn real_tsserver_slot_member_access_stays_typed_after_opening_child_and_parent() {
    let workspace_id = crate::test_harness::provider_fixture_workspace_root("single-project");
    let tsdk = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../packages/vue-vscode/node_modules/typescript/lib")
        .to_string_lossy()
        .replace('\\', "/");
    let Some(node_path) = crate::tsserver::find_node() else {
        panic!("node must be on PATH; returning success after printing 'skipping' is a false pass");
    };
    let Some(tsserver_path) = crate::test_harness::harness_tsserver_path(&tsdk) else {
        panic!(
            "tsserver.js must exist under {tsdk}; returning success after printing 'skipping' is a false pass"
        );
    };
    let plugin_path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../packages/vue-vscode/node_modules")
        .to_string_lossy()
        .replace('\\', "/");
    // Per-session carrier-store isolation: a UNIQUE host-version segment so this
    // test's on-disk store does not leak into (or read from) another test sharing
    // the same fixture workspace root. The matching `store_segment` override is
    // installed around the server construction below so the LSP-side publish
    // backend resolves the SAME dir the spawned plugin reads.
    let store_segment = crate::test_harness::unique_store_segment();
    let carrier_store_dir =
        crate::external_ts::carrier_store_dir_for(&store_segment, &workspace_id)
            .to_string_lossy()
            .replace('\\', "/");
    let provider = match crate::tsserver::ipc::TsserverTypeProvider::spawn(
        &node_path,
        &tsserver_path.to_string_lossy().replace('\\', "/"),
        &workspace_id,
        Some(&plugin_path),
        Some(&carrier_store_dir),
        // verter_lsp-internal backend: the Rust merge layer maps responses.
        false,
        None,
    )
    .await
    {
        Ok(p) => Arc::new(p),
        Err(e) => {
            panic!(
                "tsserver spawn must succeed; returning success after printing 'skipping' is a false pass: {e}"
            );
        }
    };
    let type_provider: Arc<dyn TypeProvider> = provider.clone();
    let host = Arc::new(VerterHost::new_standalone(
        real_provider_correctness_config(),
    ));
    let host_for_server = Arc::clone(&host);
    let type_provider_for_server = Arc::clone(&type_provider);
    // Construct the server under this test's per-session store-dir override so the
    // LSP-side publish backend resolves the SAME isolated dir the spawned plugin
    // reads (`store_segment` derived above). The install lock is held across the
    // synchronous construction only, so concurrent tests never share a segment.
    let (service, _socket) =
        crate::test_harness::with_isolated_store_segment(&store_segment, || {
            tower_lsp_server::LspService::new(move |_client| {
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
            })
        });

    let server = service.inner();
    install_test_resolver_for_root(
        server,
        &workspace_id,
        Some(&format!("{workspace_id}/tsconfig.json")),
    );

    let child_path = format!("{workspace_id}/src/TypedSlotComp.vue");
    let child_source =
        std::fs::read_to_string(&child_path).expect("fixture TypedSlotComp.vue should exist");
    let child_uri = crate::uri::path_to_file_uri(&child_path).expect("child uri");
    server
        .did_open(DidOpenTextDocumentParams {
            text_document: TextDocumentItem {
                uri: child_uri,
                language_id: "vue".to_string(),
                version: 1,
                text: child_source,
            },
        })
        .await;

    let parent_path = format!("{workspace_id}/src/TemplateSlotCases.vue");
    let parent_source =
        std::fs::read_to_string(&parent_path).expect("fixture TemplateSlotCases.vue should exist");
    let parent_uri = crate::uri::path_to_file_uri(&parent_path).expect("parent uri");
    server
        .did_open(DidOpenTextDocumentParams {
            text_document: TextDocumentItem {
                uri: parent_uri.clone(),
                language_id: "vue".to_string(),
                version: 1,
                text: parent_source,
            },
        })
        .await;

    let member_position = find_document_position(server, &parent_uri, "slotItem.name", 9);
    let hover_position = find_document_position(server, &parent_uri, "slotItem.name", 2);
    // The staged fixture reaching a typed state in the real tsserver+plugin is
    // an eventual-consistency fact, not a fixed-duration one — a single sleep
    // guesses how long that takes and flips under load. Poll the DIRECT
    // provider probe (not `server.completion`/`server.hover`, the calls under
    // test) as a pure readiness gate, with a generous overall budget; still
    // FAIL (never skip) once the budget is exhausted. Completion and hover
    // are each called exactly ONCE, after readiness is established, so a
    // regression in either cannot be masked by retrying until it happens to
    // pass.
    {
        let ready = tokio::time::timeout(std::time::Duration::from_secs(10), async {
            loop {
                let ctx = synced_type_provider_context(server, &parent_uri).await;
                let tsx_offset = merge::carrier_position_to_tsx_offset_validated(
                    &member_position,
                    &ctx.carrier_line_index,
                    &ctx.mapper,
                    &ctx.tsx_line_index,
                );
                let probe_saw_name = match tsx_offset {
                    Some(tsx_offset) => match provider
                        .get_completions(&ctx.tsx_path, tsx_offset, Some("."))
                        .await
                    {
                        Ok(direct_result) => {
                            direct_result.items.iter().any(|item| item.label == "name")
                        }
                        Err(_) => false,
                    },
                    None => false,
                };
                if probe_saw_name {
                    return;
                }
                tokio::time::sleep(std::time::Duration::from_millis(250)).await;
            }
        })
        .await;
        if ready.is_err() {
            provider.shutdown().await;
            panic!(
                "fixture never reached a typed state in tsserver within the 10s watchdog; \
                 direct probe never saw `name` on the scoped-slot type"
            );
        }
    }
    let labels = completion_labels(
        server
            .completion(completion_params(&parent_uri, member_position, Some(".")))
            .await
            .expect("slot completion request should succeed"),
    );
    let hover = hover_text(
        server
            .hover(hover_params(&parent_uri, hover_position))
            .await
            .expect("slot hover request should succeed"),
    );
    let literal_debug =
        Some(synced_type_provider_context(server, &parent_uri).await).and_then(|ctx| {
            ctx.tsx_content.find("slotItem.name").map(|start| {
                (
                    ctx.tsx_path.clone(),
                    start as u32 + "slotItem.".len() as u32,
                )
            })
        });
    let direct_provider = provider.clone();
    let direct_debug = Some(synced_type_provider_context(server, &parent_uri).await)
        .and_then(|ctx| {
            let tsx_path = ctx.tsx_path.clone();
            merge::carrier_position_to_tsx_offset_validated(
                &member_position,
                &ctx.carrier_line_index,
                &ctx.mapper,
                &ctx.tsx_line_index,
            )
            .map(|tsx_offset| (ctx, tsx_offset, tsx_path))
        })
        .map(|(ctx, tsx_offset, tsx_path)| async move {
            direct_provider
                .get_completions(&ctx.tsx_path, tsx_offset, Some("."))
                .await
                .map(|result| {
                    (
                        result
                            .items
                            .into_iter()
                            .map(|item| item.label)
                            .collect::<Vec<_>>(),
                        tsx_path.clone(),
                    )
                })
                .map_err(|error| (error.to_string(), tsx_path))
        });
    let _parent_state = server
        .provider_sync_states
        .get(&parent_path)
        .map(|state| state.clone());
    let _child_state = server
        .provider_sync_states
        .get(&child_path)
        .map(|state| state.clone());
    let (_direct_labels, _tsx_path, _direct_error) = if let Some(fut) = direct_debug {
        match fut.await {
            Ok((labels, tsx_path)) => (Some(labels), Some(tsx_path), None),
            Err((error, tsx_path)) => (None, Some(tsx_path), Some(error)),
        }
    } else {
        (
            None,
            None,
            Some("missing type provider context".to_string()),
        )
    };
    let (_literal_labels, _literal_error) = if let Some((tsx_path, tsx_offset)) = literal_debug {
        match provider
            .get_completions(&tsx_path, tsx_offset, Some("."))
            .await
        {
            Ok(result) => (
                Some(
                    result
                        .items
                        .into_iter()
                        .map(|item| item.label)
                        .collect::<Vec<_>>(),
                ),
                None,
            ),
            Err(error) => (None, Some(error.to_string())),
        }
    } else {
        (
            None,
            Some("slotItem.name missing from generated TSX".to_string()),
        )
    };

    if !labels.contains(&"name".to_string()) {
        // FAIL, never skip — see the member-access discriminators above.
        provider.shutdown().await;
        panic!(
            "slot member completion must resolve the scoped-slot type; `name` is \
             missing, which means the staged fixture's dependencies did not \
             materialize, got: {labels:?}"
        );
    }
    assert!(
        labels.contains(&"id".to_string()),
        "slot member completions should include id, got: {labels:?}"
    );
    assert!(
        hover.contains("SlotItem") || (hover.contains("name") && hover.contains("id")),
        "slot hover should retain the slot item type, got: {hover}"
    );
    assert!(
        !hover.contains(": any"),
        "slot hover should not degrade to any, got: {hover}"
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn sync_pending_carrier_provider_file_composes_external_template_into_ide_output() {
    let temp = tempfile::tempdir().expect("temp dir");
    let workspace = temp.path().join("workspace");
    std::fs::create_dir_all(workspace.join("src/partials")).expect("create partials dir");
    std::fs::write(workspace.join("tsconfig.app.json"), "{}").expect("write tsconfig");
    std::fs::write(
        workspace.join("src/partials/panel.html"),
        "<div>{{ props.msg }}</div>",
    )
    .expect("write external template");
    std::fs::write(
        workspace.join("src/types.ts"),
        "import type { Nested } from '@/nested'\nexport interface Props { msg: Nested }",
    )
    .expect("write types dependency");
    std::fs::write(
        workspace.join("src/nested.ts"),
        "export type Nested = string",
    )
    .expect("write nested dependency");

    let workspace_id = crate::test_utils::canonical_test_path(&workspace);
    let app_id = format!("{workspace_id}/src/App.vue");
    let uri = crate::uri::path_to_file_uri(&app_id).expect("file uri");

    let host = crate::test_utils::make_filesystem_test_host(&workspace);
    let mut project = verter_workspace::ide_project_config(
        workspace_id.clone(),
        workspace_id.clone(),
        Some(format!("{workspace_id}/tsconfig.app.json")),
    );
    project.compiler_options = verter_session_query::resolution::IdeProjectCompilerOptions {
        base_url: Some(workspace_id.clone()),
        paths: vec![("@/*".to_string(), vec!["src/*".to_string()])],
        ..Default::default()
    };
    host.configure_projects(vec![project.clone()]);

    let documents = DocumentRegistry::new(Arc::clone(&host));
    let _ = documents.did_open(&TextDocumentItem {
            uri: uri.clone(),
            language_id: "vue".to_string(),
            version: 1,
            text: "<template src=\"@/partials/panel.html\"></template>\n<script setup lang=\"ts\">\nimport type { Props } from '@/types'\nconst props = defineProps<Props>()\n</script>".to_string(),
        });
    // The registered VFS read admits external bytes for classification,
    // analysis, and the composed IDE surface below.
    let external_source = host
        .get_source(&format!("{workspace_id}/src/partials/panel.html"))
        .expect("registered external source should be admitted from the VFS");
    assert!(
        external_source.contains("props.msg"),
        "the admitted source must be the registered external template"
    );
    // Type deps (types.ts) are resolved via VFS workspace read fallback during
    // compilation but may not be explicitly loaded into the scheduler.
    assert!(
        host.resolve_import_transient(&app_id, "@/types").is_some(),
        "macro type dep @/types should resolve via VFS"
    );

    // Verify the resolver can resolve these specifiers
    let snapshot = PublishedResolverSnapshot {
        resolver: verter_resolution::ModuleResolverCore::new(vec![project]),
        resolution_view: None,
        ownership_ready: true,
    };
    // The drain carries a `CarrierPublishCtx` with the published vfs (the single
    // ownership-resolution source; `coordinator: None` ⇒ direct-open).
    let owner_vfs =
        configured_owner_vfs(&workspace_id, &format!("{workspace_id}/tsconfig.app.json"));
    let carrier_publish = crate::server::background_drain::CarrierPublishCtx {
        coordinator: None,
        provider_delivery: crate::external_ts::CarrierProviderDelivery::DirectOpen,
        vfs: Arc::clone(&owner_vfs),
        ownership_ready: true,
    };
    let ws = documents.host().workspace_read();
    let external_resolved = ws.resolve_import(
        &app_id,
        "@/partials/panel.html",
        verter_session_query::resolution::ResolutionContext {
            kind: verter_session_query::resolution::ResolveRequestKind::SfcSrcAttr,
            phase: verter_session_query::resolution::ResolvePhase::CodegenBlocker,
        },
    );
    assert!(
        external_resolved.is_some(),
        "external src specifier should resolve through the native resolver"
    );
    assert!(
        external_resolved
            .unwrap()
            .source_id
            .ends_with("/src/partials/panel.html"),
        "external src should resolve to the real template file"
    );

    // Sync to provider and verify IDE output is available
    let provider = Arc::new(MockTypeProvider::new());
    let sync = ProjectSync::new(provider.clone(), ProjectSyncMode::FullProject);
    let provider_sync_states = DashMap::new();

    let synced = sync_pending_carrier_provider_file(
        Some(&sync),
        &documents,
        &snapshot,
        &provider_sync_states,
        &app_id,
        Some(&carrier_publish),
        &crate::external_ts::CarrierTransactionCoordinator::new(),
        &dashmap::DashSet::new(),
    )
    .await;
    assert_eq!(synced, SyncOutcome::FullyReconciled);

    let profile = documents.tsx_profile.read().clone();
    let ide = host
        .get_ide(&app_id, &profile)
        .expect("admitted external template must publish a composed IDE surface");
    assert!(ide.code.contains("props.msg"));
    let source_map: serde_json::Value = serde_json::from_str(
        ide.source_map
            .as_deref()
            .expect("composed IDE surface must publish its map"),
    )
    .expect("composed IDE map must be valid JSON");
    let sources = source_map["sources"]
        .as_array()
        .expect("composed IDE map must declare source spaces");
    assert_eq!(
        sources.len(),
        2,
        "carrier script and external template must remain distinct map sources"
    );

    let calls = provider.file_sync_calls();
    assert!(
        calls.iter().any(|call| matches!(
            call,
            MockCall::OpenFile { path, content } | MockCall::UpdateFile { path, content }
                if path.ends_with(".tsx") && content.contains("props.msg")
        )),
        "composed external content must be pushed through the TSX provider surface"
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn sync_pending_carrier_provider_file_syncs_ide_artifact_for_tsgo() {
    let temp = tempfile::tempdir().expect("temp dir");
    let workspace = temp.path().join("workspace");
    std::fs::create_dir_all(workspace.join("src")).expect("create src dir");
    std::fs::write(workspace.join("tsconfig.app.json"), "{}").expect("write tsconfig");

    let workspace_id = crate::test_utils::canonical_test_path(&workspace);
    let app_id = format!("{workspace_id}/src/App.vue");
    let uri = crate::uri::path_to_file_uri(&app_id).expect("file uri");

    let host = crate::test_utils::make_filesystem_test_host(&workspace);
    let documents = DocumentRegistry::new(Arc::clone(&host));
    let _ = documents.did_open(&TextDocumentItem {
        uri: uri.clone(),
        language_id: "vue".to_string(),
        version: 1,
        text: r#"<script setup lang="ts">
import Child from './Child.vue'
</script>
<template><Child msg="hi" /></template>"#
            .to_string(),
    });
    let _ = host.upsert(UpsertRequest {
        canonical_id: Some(format!("{workspace_id}/src/Child.vue")),
        input_id: format!("{workspace_id}/src/Child.vue"),
        source: Arc::<str>::from(
            r#"<script setup lang="ts">
defineProps<{ msg: string }>()
</script>
<template><div>{{ msg }}</div></template>"#,
        ),
        file_language: FileLanguage::vue(),
        aliases: Vec::new(),
    });

    let tsconfig = format!("{workspace_id}/tsconfig.app.json");
    let snapshot = PublishedResolverSnapshot {
        resolver: verter_resolution::ModuleResolverCore::new(vec![
            verter_workspace::ide_project_config(
                workspace_id.clone(),
                workspace_id.clone(),
                Some(tsconfig.clone()),
            ),
        ]),
        resolution_view: None,
        ownership_ready: true,
    };
    // The tsgo drain always carries a `CarrierPublishCtx` with the published vfs (the
    // single ownership-resolution source; its `coordinator` is `None` for tsgo).
    let owner_vfs = configured_owner_vfs(&workspace_id, &tsconfig);
    let carrier_publish = crate::server::background_drain::CarrierPublishCtx {
        coordinator: None,
        provider_delivery: crate::external_ts::CarrierProviderDelivery::DirectOpen,
        vfs: Arc::clone(&owner_vfs),
        ownership_ready: true,
    };
    let provider = Arc::new(MockTypeProvider::new());
    let sync = ProjectSync::new(provider.clone(), ProjectSyncMode::FullProject);
    let provider_sync_states = DashMap::new();

    let synced = sync_pending_carrier_provider_file(
        Some(&sync),
        &documents,
        &snapshot,
        &provider_sync_states,
        &app_id,
        Some(&carrier_publish),
        &crate::external_ts::CarrierTransactionCoordinator::new(),
        &dashmap::DashSet::new(),
    )
    .await;

    assert_eq!(
        synced,
        SyncOutcome::FullyReconciled,
        "pending Vue sync should fully reconcile for TSGO (both kinds synced)"
    );

    let calls = provider.file_sync_calls();
    assert!(
            calls.iter().any(|call| matches!(
                call,
                MockCall::OpenFile { path, .. } | MockCall::UpdateFile { path, .. } if path.ends_with(".vue.verter.ts")
            )),
            "TSGO pending sync should keep syncing the API artifact, calls={calls:?}"
        );
    assert!(
            calls.iter().any(|call| matches!(
                call,
                MockCall::OpenFile { path, .. } | MockCall::UpdateFile { path, .. } if path.ends_with(".vue.tsx")
            )),
            "TSGO pending sync should also sync the IDE artifact, calls={calls:?}"
        );
}

/// Guard `provider_projection_context_serves_both_carrier_and_self_file`: the
/// ONE generalized `provider_projection_context` serves BOTH a `.vue` carrier
/// (carrier-IDE projection) AND a `.svelte.ts` rune module (self-file
/// projection) — there is no parallel rune-only query path. The discriminating
/// assertion is the self-file prelude offset: a user-source line maps to
/// provider line `+ prelude_line_count`, and a provider position in the prelude
/// region drops to no source line (off-by-prelude if the offset were unwired).
#[tokio::test]
async fn provider_projection_context_serves_both_carrier_and_self_file() {
    use verter_span::TsPosition;

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
    install_test_resolver(server);

    // (1) CARRIER: a `.vue` file projects through the carrier-IDE branch of the
    // ONE generalized context.
    let vue_uri: Uri = "file:///workspace/App.vue".parse().unwrap();
    let _ = server.documents.did_open(&TextDocumentItem {
        uri: vue_uri.clone(),
        language_id: "vue".to_string(),
        version: 1,
        text: "<script setup lang=\"ts\">\nconst x = 1;\n</script>\n<template>{{ x }}</template>"
            .to_string(),
    });
    server.ensure_current_file_synced(&vue_uri).await;
    assert!(
        matches!(
            server.documents.get_projection(&vue_uri),
            Some(
                crate::documents::provider_projection::DocumentProviderProjection::CarrierIde { .. }
            )
        ),
        "a `.vue` carrier builds the carrier-IDE projection"
    );
    let carrier_ctx = server
        .provider_projection_context(&vue_uri)
        .expect("the carrier projects through the generalized context");
    assert!(
        carrier_ctx.provider_path.ends_with(".tsx") || carrier_ctx.provider_path.ends_with(".jsx"),
        "a carrier's provider path is an IDE TSX/JSX path, got {}",
        carrier_ctx.provider_path
    );

    // (2) SELF-FILE: a `.svelte.ts` rune module projects through the self-file
    // branch served by the SAME context — provider path IS the canonical id.
    let rune_uri: Uri = "file:///workspace/store.svelte.ts".parse().unwrap();
    let _ = server.documents.did_open(&TextDocumentItem {
        uri: rune_uri.clone(),
        language_id: "typescript".to_string(),
        version: 1,
        text: "export const s = $state(0);\n".to_string(),
    });
    // The production `did_open` handler drives the eager shadow sync; this test
    // opens through the registry directly, so drive it here (the query context
    // serves the RECORDED synced surface, not an on-demand rebuild).
    assert!(
        server.sync_self_file_shadow_unresolved(&rune_uri).await,
        "the self-file shadow sync should succeed against the mock provider"
    );
    let rune_ctx = server
        .provider_projection_context(&rune_uri)
        .expect("the rune module projects through the SAME generalized context");
    assert_eq!(
        rune_ctx.provider_path, "/workspace/store.svelte.ts",
        "a self-file rune module serves its provider buffer from its OWN canonical path"
    );
    // The provider content prepends the synthetic rune prelude.
    assert!(
        rune_ctx.provider_content.contains("$state"),
        "the rune-module provider buffer carries the synthetic rune prelude declarations"
    );

    // Discriminating: the self-file mapper offsets the user-source line DOWN by
    // the prelude line count. A provider position INSIDE the prelude region
    // drops to no source line (never a fake source-line-0).
    let drop_in_prelude = rune_ctx.mapper.tsx_to_carrier(TsPosition::new(0, 0));
    assert!(
        drop_in_prelude.is_none(),
        "a provider position in the synthetic prelude region must drop, not surface a source line"
    );
    // A user-source position maps to a provider line strictly BELOW the prelude.
    let mapped = rune_ctx
        .mapper
        .carrier_to_tsx(verter_span::LspPosition::new(0, 13))
        .expect("a user-source position maps into the provider buffer");
    assert!(
        mapped.pos.line > 0,
        "the user-source line must shift DOWN by the prelude line count (off-by-prelude if unwired)"
    );

    drain.abort();
}

/// Fail-closed auto-import completion-resolve for a self-file rune module.
///
/// The provider auto-import re-anchor (`resolve_provider_auto_import_edits`) maps a
/// provider edit back through a Vue `<script setup>` carrier. A self-file rune
/// module (Svelte `.svelte.ts` / `.svelte.js`) has NO `<script setup>` carrier
/// to re-anchor into — its provider path reverse-maps to itself, not to a
/// carrier source. Accepting such a completion must NOT error and must NOT
/// synthesize a bogus Vue block: it returns `Ok(None)`, leaving the completion
/// item unchanged (self-file auto-import placement is a separate capability).
///
/// Discriminating: this asserts `Ok(None)` for a NON-EMPTY provider-edit set on
/// a `.svelte.ts` self-file projection. The pre-fix carrier-only resolver had no
/// self-file gate — it either rejected the resolve with a structured error (no
/// resolvable carrier URI) or drove the Vue-specific `resolve_script_import_anchor`
/// over the rune-module source; neither yields the fail-closed `Ok(None)`. The
/// companion assertion pins that a real `.vue` carrier is NOT classified as a
/// self-file projection, so the Vue carrier re-anchor path stays reachable.
#[tokio::test]
async fn self_file_auto_import_resolve_fails_closed_with_no_edits() {
    use crate::type_provider::auto_import::ProviderImportEdit;

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
    install_test_resolver(server);

    // A self-file rune module: its provider buffer is served from its OWN
    // canonical path, so the completion item's stored `tsx_path` IS the
    // `.svelte.ts` path — not a Vue carrier IDE path.
    let rune_uri: Uri = "file:///workspace/store.svelte.ts".parse().unwrap();
    let _ = server.documents.did_open(&TextDocumentItem {
        uri: rune_uri.clone(),
        language_id: "typescript".to_string(),
        version: 1,
        text: "export const s = $state(0);\n".to_string(),
    });
    assert!(
        server.is_self_file_projection(&rune_uri),
        "precondition: a `.svelte.ts` rune module is a self-file projection"
    );

    // A NON-EMPTY auto-import edit set (the only case the carrier re-anchor runs
    // for). The bytes are a faithful new-import insertion as TSGO would emit.
    let provider_edits = vec![ProviderImportEdit {
        start: 0,
        end: 0,
        new_text: "import { foo } from './foo';\n".to_string(),
    }];
    let resolved =
        super::super::nav_features_completion_resolve::resolve_provider_auto_import_edits(
            server,
            "/workspace/store.svelte.ts",
            super::super::nav_features_completion_resolve::capture_resolve_surface(
                server,
                "/workspace/store.svelte.ts",
            )
            .as_ref()
            .map(|(_, snapshot)| &**snapshot),
            &provider_edits,
        );
    assert_eq!(
        resolved,
        Ok(None),
        "a self-file (`.svelte.ts`) auto-import resolve fails closed: no edits, no internal error"
    );

    // Companion: a real `.vue` carrier is a carrier-IDE projection, NOT a
    // self-file one — so the fail-closed self-file gate never fires for it and
    // the Vue carrier re-anchor path stays reachable.
    let vue_uri: Uri = "file:///workspace/App.vue".parse().unwrap();
    let _ = server.documents.did_open(&TextDocumentItem {
        uri: vue_uri.clone(),
        language_id: "vue".to_string(),
        version: 1,
        text: "<script setup lang=\"ts\">\nconst x = 1;\n</script>\n<template>{{ x }}</template>"
            .to_string(),
    });
    server.ensure_current_file_synced(&vue_uri).await;
    assert!(
        !server.is_self_file_projection(&vue_uri),
        "a `.vue` carrier is a carrier-IDE projection, never gated as self-file"
    );

    drain.abort();
}

/// A real `.svelte` carrier (NOT a `.svelte.ts` rune module) is a carrier-IDE
/// projection that reverse-maps to its `.svelte` source and is NOT a self-file
/// projection — so it slips past BOTH the no-carrier and the self-file gates on
/// completion-resolve. The carrier `<script setup>` import re-anchor is a
/// Vue-SFC-specific construct: a `.svelte` source has no Vue `<script setup>`,
/// so driving `resolve_script_import_anchor` over it would synthesize a bogus
/// Vue `<script setup>` block INTO the `.svelte` file. The resolve must instead
/// fail closed through the typed carrier-kind authority (a non-Vue carrier is
/// not re-anchorable), leaving the completion item unchanged — symmetric with
/// the code-action `resolve_carrier_preamble_import_anchor` Vue gate.
///
/// Discriminating: the `.svelte` carrier reverse-map IS present, the projection
/// is NOT self-file, and the provider-edit set is NON-EMPTY (the only case the
/// carrier re-anchor runs for). The pre-gate code never returned `Ok(None)` for
/// this real non-self-file carrier — it either errored on a missing IDE context
/// or synthesized a `CreateScriptSetup` Vue block over the `.svelte` source.
/// This asserts `Ok(None)` AND that no produced edit text contains a
/// `<script setup>`, so it FAILS before the gate and PASSES once a non-Vue
/// carrier fails closed.
#[tokio::test]
async fn non_vue_carrier_auto_import_resolve_fails_closed_no_script_setup_synthesis() {
    use crate::type_provider::auto_import::ProviderImportEdit;

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
    install_test_resolver(server);

    // A real `.svelte` carrier with NO `<script setup>` (Svelte uses a plain
    // `<script>`). It is owned and open, so `Comp.svelte.tsx` reverse-maps to
    // the `.svelte` source, and it projects through a carrier IDE TSX — not a
    // self-file rune-module buffer.
    let svelte_uri = open_test_svelte(
        server,
        "/workspace/Comp.svelte",
        "<script lang=\"ts\">\nlet x = 1;\n</script>\n<p>{x}</p>",
    );
    server.ensure_current_file_synced(&svelte_uri).await;
    let tsx_path = "/workspace/Comp.svelte.tsx";

    // Precondition 1: the `.svelte` carrier reverse-map IS present.
    assert_eq!(
        server.carrier_uri_from_ide_path(tsx_path).as_ref(),
        Some(&svelte_uri),
        "precondition: the `.svelte.tsx` IDE path reverse-maps to the open `.svelte` carrier URI"
    );
    // Precondition 2: a `.svelte` carrier is NOT a self-file projection — so the
    // existing self-file `Ok(None)` gate is NOT what would fire here.
    assert!(
        !server.is_self_file_projection(&svelte_uri),
        "precondition: a `.svelte` carrier is a carrier-IDE projection, not self-file"
    );

    // A NON-EMPTY auto-import edit set — the only case the carrier re-anchor runs
    // for. The bytes are a faithful new-import insertion as a provider would emit.
    let provider_edits = vec![ProviderImportEdit {
        start: 0,
        end: 0,
        new_text: "import { foo } from './foo';\n".to_string(),
    }];
    let resolved =
        super::super::nav_features_completion_resolve::resolve_provider_auto_import_edits(
            server,
            tsx_path,
            super::super::nav_features_completion_resolve::capture_resolve_surface(
                server, tsx_path,
            )
            .as_ref()
            .map(|(_, snapshot)| &**snapshot),
            &provider_edits,
        );
    // Negative assertion FIRST, so it stays load-bearing: whatever the resolve
    // produced, no edit may carry a synthesized Vue `<script setup>` block into
    // the `.svelte` source. Pre-gate, the resolve returned exactly such an edit
    // (`Ok(Some([<script setup ...>]))`), so this fires before the gate lands.
    if let Ok(Some(edits)) = &resolved {
        assert!(
            !edits.iter().any(|e| e.new_text.contains("<script setup")),
            "a `.svelte` carrier resolve must never synthesize a Vue `<script setup>` block; \
             got {edits:?}"
        );
    }
    // Stronger property: a non-Vue carrier fails closed entirely (no edits, no
    // error), leaving the completion item unchanged — symmetric with the
    // code-action `resolve_carrier_preamble_import_anchor` Vue gate.
    assert_eq!(
        resolved,
        Ok(None),
        "a non-Vue (`.svelte`) carrier auto-import resolve fails closed via the typed \
         carrier-kind authority: no edits, no error, no synthesized Vue block; got {resolved:?}"
    );

    drain.abort();
}

/// An editor edit to an OPEN `.svelte.ts` rune module re-syncs its self-file
/// own-buffer to the provider (the carrier eager-TSX path never fires for a
/// non-carrier, and the coordinator routes diagnostics through carrier IDE
/// state). After `handle_did_change`, `provider_projection_context` reflects
/// the EDITED source — stale own-buffer content would leave the old text.
#[tokio::test]
async fn rune_module_own_buffer_resyncs_on_did_change() {
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
    install_test_resolver(server);

    let rune_uri: Uri = "file:///workspace/store.svelte.ts".parse().unwrap();
    let _ = server.documents.did_open(&TextDocumentItem {
        uri: rune_uri.clone(),
        language_id: "typescript".to_string(),
        version: 1,
        text: "export const s = $state(0);\n".to_string(),
    });
    // The production `did_open` handler drives the eager shadow sync; this test
    // opens through the registry directly, so drive it here.
    assert!(
        server.sync_self_file_shadow_unresolved(&rune_uri).await,
        "the self-file shadow sync should succeed against the mock provider"
    );
    let ctx0 = server.provider_projection_context(&rune_uri).unwrap();
    assert!(ctx0.provider_content.contains("$state(0)"));

    // Edit the document, then drive the did_change handler. The notification
    // must acknowledge before provider I/O; the coordinator owns the refresh.
    let (update_arrived, update_release) = provider.block_update_file("/workspace/store.svelte.ts");
    super::super::lifecycle::handle_did_change(
        server,
        DidChangeTextDocumentParams {
            text_document: VersionedTextDocumentIdentifier {
                uri: rune_uri.clone(),
                version: 2,
            },
            content_changes: vec![TextDocumentContentChangeEvent {
                range: None,
                range_length: None,
                text: "export const count = $state(42);\n".to_string(),
            }],
        },
    )
    .await;
    tokio::time::timeout(std::time::Duration::from_secs(2), update_arrived.notified())
        .await
        .expect("did_change must schedule the rune shadow refresh");
    update_release.notify_one();
    server.ensure_current_file_synced(&rune_uri).await;

    let ctx1 = server
        .provider_projection_context(&rune_uri)
        .expect("the rune module is still queryable after did_change");
    assert!(
        ctx1.provider_content.contains("$state(42)") && ctx1.provider_content.contains("count"),
        "the own-buffer provider content must reflect the EDIT, got: {}",
        ctx1.provider_content
    );
    assert!(
        !ctx1.provider_content.contains("const s = $state(0)"),
        "the stale pre-edit own-buffer content must NOT linger"
    );

    drain.abort();
}

/// Guard `rune_module_self_file_state_closed_on_did_close`: an OPEN rune
/// module's self-file Shadow provider state is closed + removed on did_close.
/// The existing did_close branch is carrier-oriented (gated on `get_ide(...)`),
/// which never fires for a non-carrier rune module — this pins the explicit
/// self-file branch.
#[tokio::test]
async fn rune_module_self_file_state_closed_on_did_close() {
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

    let rune_uri: Uri = "file:///workspace/store.svelte.ts".parse().unwrap();
    let canonical_id = "/workspace/store.svelte.ts";
    let _ = server.documents.did_open(&TextDocumentItem {
        uri: rune_uri.clone(),
        language_id: "typescript".to_string(),
        version: 1,
        text: "export const s = $state(0);\n".to_string(),
    });
    // Sync the open-document self-file Shadow state.
    assert!(
        server.sync_self_file_shadow_unresolved(&rune_uri).await,
        "the open rune module syncs its self-file Shadow provider state"
    );
    let state = server
        .provider_sync_state_for_source(canonical_id)
        .expect("the rune module has provider sync state after the shadow sync");
    assert_eq!(
        state.shadow_path.as_deref(),
        Some(canonical_id),
        "the self-file Shadow path is the module's OWN canonical id"
    );
    assert!(state.shadow_background_loaded);

    // did_close must close + remove the self-file provider state.
    super::super::lifecycle::handle_did_close(
        server,
        DidCloseTextDocumentParams {
            text_document: TextDocumentIdentifier {
                uri: rune_uri.clone(),
            },
        },
    )
    .await;
    assert!(
        server
            .provider_sync_state_for_source(canonical_id)
            .is_none(),
        "did_close must remove the rune module's self-file provider state"
    );

    drain.abort();
}

/// Close-while-depended-upon: a plain `.ts` that an OPEN carrier imports is
/// closed in the editor. did_close retires the script's own self-file provider
/// state (the new every-plain-script close branch), but the dependent
/// carrier's provider surface must be UNTOUCHED — the provider still resolves
/// the closed script from disk, so the carrier's features keep answering.
/// DISCRIMINATING: a close that tears the carrier's sync state / provider
/// buffer (or closes nothing for the script) fails one of the assertions
/// below.
#[tokio::test(flavor = "multi_thread")]
async fn plain_script_close_while_depended_upon_keeps_carrier_features_answering() {
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

    // An OPEN carrier that imports the plain script.
    let app_uri = open_test_vue(
        server,
        "/workspace/src/App.vue",
        "<script setup lang=\"ts\">\nimport { utilValue } from './util'\nconst doubled = utilValue * 2\n</script>\n<template><div>{{ doubled }}</div></template>\n",
    );
    // The OPEN plain script the carrier depends on, with its self-file
    // own-buffer provider state committed (the production did_open drive).
    let util_uri: Uri = "file:///workspace/src/util.ts".parse().unwrap();
    let util_canonical = "/workspace/src/util.ts";
    let _ = server.documents.did_open(&TextDocumentItem {
        uri: util_uri.clone(),
        language_id: "typescript".to_string(),
        version: 1,
        text: "export const utilValue: number = 1;\n".to_string(),
    });
    assert!(
        server.sync_self_file_shadow_unresolved(&util_uri).await,
        "the open plain script syncs its self-file Shadow provider state"
    );
    assert_eq!(
        server
            .provider_sync_state_for_source(util_canonical)
            .and_then(|state| state.shadow_path)
            .as_deref(),
        Some(util_canonical),
        "precondition: the plain script has own-path Shadow state"
    );

    // Seed the carrier's query surface + a provider hover answer for it.
    let position = find_document_position(server, &app_uri, "{{ doubled", 3);
    set_type_hover_at_vue_position(server, &provider, &app_uri, position, "doubled: number");
    let carrier_canonical = "/workspace/src/App.vue";
    let carrier_ide_path = server
        .provider_sync_state_for_source(carrier_canonical)
        .and_then(|state| state.ide_path)
        .expect("precondition: the carrier has a committed IDE path");

    // Close the depended-upon plain script.
    super::super::lifecycle::handle_did_close(
        server,
        DidCloseTextDocumentParams {
            text_document: TextDocumentIdentifier {
                uri: util_uri.clone(),
            },
        },
    )
    .await;

    // The closed script's own provider state is retired (closed + removed) …
    assert!(
        server
            .provider_sync_state_for_source(util_canonical)
            .is_none(),
        "did_close must retire the closed plain script's self-file provider state"
    );
    let calls = provider.calls();
    assert!(
        calls.iter().any(|call| matches!(
            call,
            MockCall::CloseFile { path } if path == util_canonical
        )),
        "did_close must close the plain script's own-path provider buffer, calls={calls:?}"
    );
    // … while the dependent carrier's provider surface is UNTOUCHED …
    let carrier_state = server
        .provider_sync_state_for_source(carrier_canonical)
        .expect("closing a dependency must NOT remove the open carrier's provider state");
    assert_eq!(
        carrier_state.ide_path.as_deref(),
        Some(carrier_ide_path.as_str()),
        "the carrier's IDE path must survive its dependency's close"
    );
    assert!(
        !calls.iter().any(|call| matches!(
            call,
            MockCall::CloseFile { path } if path == &carrier_ide_path
        )),
        "closing a dependency must NOT close the carrier's provider buffer, calls={calls:?}"
    );
    // … and the carrier's features keep answering.
    let hover = super::super::nav_features::handle_hover(server, hover_params(&app_uri, position))
        .await
        .expect("hover request succeeds");
    assert!(
        hover_text(hover).contains("doubled: number"),
        "the dependent carrier's hover must keep answering after its plain-script \
         dependency closed"
    );

    drain.abort();
}

/// Symptom-level provider-route coverage for a plain `.ts`: hover, definition,
/// and diagnostics must ANSWER on every provider route.
///
/// - TSGO / managed tsserver (local provider): the features are served through
///   the self-file projection — the provider is queried at the script's OWN
///   canonical path (verbatim bytes, identity mapping), never a derived
///   companion path.
/// - Editor-owned tsserver: the editor's own TypeScript server natively owns a
///   plain `.ts` (it IS a TypeScript file), so the LSP must DEFER cleanly —
///   `None` answers, no error, no competing partial response — and publish no
///   diagnostics of its own.
#[tokio::test(flavor = "multi_thread")]
async fn plain_script_features_answer_on_every_provider_route() {
    use crate::type_provider::protocol::TypeDiagnosticSeverity;

    for kind in [
        crate::TypeProviderKind::Tsgo,
        crate::TypeProviderKind::Tsserver,
    ] {
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
                    type_provider_kind: kind,
                    type_provider_topology: crate::TypeProviderTopology::implied_by(kind),
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

        let canonical_id = "/workspace/src/util.ts";
        let source = "export const utilValue: number = 1;\nexport const doubled = utilValue * 2;\n";
        let uri: Uri = "file:///workspace/src/util.ts".parse().unwrap();
        let _ = server.documents.did_open(&TextDocumentItem {
            uri: uri.clone(),
            language_id: "typescript".to_string(),
            version: 1,
            text: source.to_string(),
        });
        // The production did_open drives the self-file shadow sync AND the
        // background dependency publication (the DependencyReady receipt the
        // navigation handlers capture); this test opens through the registry
        // directly, so drive both here.
        assert!(
            server.sync_self_file_shadow_unresolved(&uri).await,
            "{kind}: the plain script's self-file shadow sync succeeds"
        );
        server.publish_import_dependencies_settled(&uri).await;
        let ctx = server
            .type_provider_context(&uri)
            .expect("{kind}: the plain script is queryable through the self-file projection");
        assert_eq!(
            ctx.tsx_path, canonical_id,
            "{kind}: the provider path is the script's OWN canonical id"
        );
        assert_eq!(
            ctx.tsx_content.as_ref(),
            source,
            "{kind}: the plain script's provider buffer is the source verbatim"
        );

        // ── hover ──
        let position = find_document_position(server, &uri, "utilValue * 2", 2);
        let provider_offset = merge::carrier_position_to_tsx_offset_validated(
            &position,
            &ctx.carrier_line_index,
            &ctx.mapper,
            &ctx.tsx_line_index,
        )
        .expect("{kind}: the source position maps into the provider buffer");
        provider.set_hover(
            &ctx.tsx_path,
            provider_offset,
            Some(HoverInfo {
                contents: "const utilValue: number".to_string(),
                display_signature: Some(crate::type_provider::mock::test_display_signature(
                    "const utilValue: number",
                )),
                ..Default::default()
            }),
        );
        let hover = super::super::nav_features::handle_hover(server, hover_params(&uri, position))
            .await
            .expect("{kind}: hover request succeeds");
        assert!(
            hover_text(hover).contains("const utilValue: number"),
            "{kind}: hover must answer for a plain script"
        );

        // ── definition ──
        let decl_start = source.find("utilValue").expect("declaration token") as u32;
        provider.set_definitions(
            &ctx.tsx_path,
            provider_offset,
            vec![TypeLocation {
                path: ctx.tsx_path.clone(),
                start: decl_start,
                end: decl_start + "utilValue".len() as u32,
            }],
        );
        let definition = super::super::nav_features_navigation::handle_goto_definition(
            server,
            goto_definition_params(&uri, position),
        )
        .await
        .expect("{kind}: definition request succeeds")
        .expect("{kind}: definition must answer for a plain script");
        let locations = definition_locations(definition);
        let target = locations
            .iter()
            .find(|location| location.uri == uri)
            .expect("{kind}: the definition must target the plain script itself");
        assert_eq!(
            target.range.start.line, 0,
            "{kind}: the definition must land on the declaration line"
        );

        // ── diagnostics ──
        let diag_start = source.find("doubled").expect("diagnostic token") as u32;
        provider.set_diagnostics(
            &ctx.tsx_path,
            vec![TypeDiagnostic {
                message: "Type 'string' is not assignable to type 'number'.".to_string(),
                severity: TypeDiagnosticSeverity::Error,
                start: diag_start,
                end: diag_start + "doubled".len() as u32,
                code: Some("2322".to_string()),
                tags: Vec::new(),
                related_information: Vec::new(),
            }],
        );
        let diagnostics = server.compute_full_diagnostics(&uri).await;
        let type_diag = diagnostics
            .iter()
            .find(|diag| diag.message.contains("not assignable"))
            .expect("{kind}: diagnostics must answer for a plain script");
        assert_eq!(
            type_diag.range.start.line, 1,
            "{kind}: the type diagnostic must land on the declaration line (identity mapping)"
        );

        // Every feature queried the provider at the script's OWN canonical
        // path — never a derived companion.
        let calls = provider.calls();
        for expected in ["hover", "definition", "diagnostics"] {
            let matched = match expected {
                "hover" => calls.iter().any(|call| {
                    matches!(
                        call,
                        MockCall::GetHover { path, .. } if path == canonical_id
                    )
                }),
                "definition" => calls.iter().any(|call| {
                    matches!(
                        call,
                        MockCall::GetDefinition { path, .. } if path == canonical_id
                    )
                }),
                _ => calls.iter().any(|call| {
                    matches!(
                        call,
                        MockCall::GetDiagnostics { path } if path == canonical_id
                    )
                }),
            };
            assert!(
                matched,
                "{kind}: {expected} must query the provider at the script's OWN path, calls={calls:?}"
            );
        }

        drain.abort();
    }

    // ── Editor-owned tsserver: the editor natively owns plain-TS features, so
    // the LSP defers cleanly on every feature (never an error, never a
    // competing partial answer, no diagnostics of its own). ──
    let host = Arc::new(VerterHost::new_standalone(HostConfig::default()));
    let host_for_server = Arc::clone(&host);
    let (service, _socket) = tower_lsp_server::LspService::new(move |_client| {
        VerterLanguageServer::new(
            crate::outbound::Outbound::default(),
            LspConfig {
                host: Arc::clone(&host_for_server),
                type_provider: None,
                project_sync_mode: crate::ProjectSyncMode::FullProject,
                type_provider_kind: crate::TypeProviderKind::EditorTsserver,
                type_provider_topology: crate::TypeProviderTopology::EditorTsserver,
                mcp_port: None,
                type_provider_reason: Some("attested editor project".into()),
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

    let uri: Uri = "file:///workspace/src/util.ts".parse().unwrap();
    let _ = server.documents.did_open(&TextDocumentItem {
        uri: uri.clone(),
        language_id: "typescript".to_string(),
        version: 1,
        text: "export const utilValue: number = 1;\nexport const doubled = utilValue * 2;\n"
            .to_string(),
    });
    let position = find_document_position(server, &uri, "utilValue * 2", 2);

    let hover = super::super::nav_features::handle_hover(server, hover_params(&uri, position))
        .await
        .expect("editor route: hover request succeeds");
    assert!(
        hover.is_none(),
        "editor route: the LSP defers plain-script hover to the editor's own tsserver, got {hover:?}"
    );
    let definition = super::super::nav_features_navigation::handle_goto_definition(
        server,
        goto_definition_params(&uri, position),
    )
    .await
    .expect("editor route: definition request succeeds");
    assert!(
        definition.is_none(),
        "editor route: the LSP defers plain-script definition to the editor's own tsserver, got {definition:?}"
    );
    let diagnostics = server.compute_full_diagnostics(&uri).await;
    assert!(
        diagnostics.is_empty(),
        "editor route: the LSP publishes no diagnostics of its own for a plain script \
         (the editor's tsserver owns them), got {diagnostics:?}"
    );

    drain.abort();
}

/// R1b close-lifecycle: deleting a carrier source CLOSES its companions in the
/// provider (retracting the open diagnostics buffer + carrier→project route). The
/// companion close is the buffer-side half of carrier retraction (the store-side
/// half is `retract_carrier_from_external_ts`). DISCRIMINATING: with the
/// `close_provider_state` call removed from the delete handler, no `CloseFile`
/// reaches the provider and the assertions fail.
#[tokio::test]
async fn deleting_carrier_source_closes_its_companions_in_provider() {
    use crate::provider_sync::ProviderOwnerBinding;

    let provider = Arc::new(MockTypeProvider::new());
    provider.set_provider_id("tsserver");
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

    let vue_uri: Uri = "file:///workspace/src/Comp.vue".parse().unwrap();
    let canonical_id = "/workspace/src/Comp.vue";
    let ide_path = "/workspace/src/Comp.vue.tsx";
    let api_path = "/workspace/src/Comp.vue.verter.ts";

    // Open the carrier source and commit a provider sync state advertising both
    // companions (the state the carrier-membership sync establishes).
    let _ = server.documents.did_open(&TextDocumentItem {
        uri: vue_uri.clone(),
        language_id: "vue".to_string(),
        version: 1,
        text: "<script setup lang=\"ts\">const a = 1;</script>\n".to_string(),
    });
    server.commit_provider_sync_state(
        canonical_id,
        ProviderSyncState {
            owner_binding: ProviderOwnerBinding::Owned("/workspace/tsconfig.json".to_string()),
            ide_path: Some(ide_path.to_string()),
            api_path: Some(api_path.to_string()),
            decl_path: None,
            ide_background_loaded: false,
            api_background_loaded: false,
            decl_background_loaded: false,
            shadow_path: None,
            shadow_background_loaded: false,
            committed_ide_surface: None,
            commit_stamp: None,
            api_delivered_hash: None,
            api_observed_hash: None,
            shadow_delivered_source_hash: None,
        },
    );

    // Delete the carrier source → the handler closes its companions.
    super::super::lifecycle::handle_did_delete_files(
        server,
        DeleteFilesParams {
            files: vec![FileDelete {
                uri: vue_uri.as_str().to_string(),
            }],
        },
    )
    .await;

    let closed: Vec<String> = provider
        .calls()
        .into_iter()
        .filter_map(|c| match c {
            MockCall::CloseFile { path } => Some(path),
            _ => None,
        })
        .collect();
    assert!(
        closed.iter().any(|p| p == ide_path),
        "deleting the carrier must CLOSE its IDE companion ({ide_path}); closed: {closed:?}"
    );
    assert!(
        closed.iter().any(|p| p == api_path),
        "deleting the carrier must CLOSE its API companion ({api_path}); closed: {closed:?}"
    );
    // The provider sync state is gone (retracted).
    assert!(
        server
            .provider_sync_state_for_source(canonical_id)
            .is_none(),
        "deleting the carrier removes its provider sync state"
    );

    drain.abort();
}

#[tokio::test(flavor = "multi_thread")]
async fn resync_aliased_imports_resolves_and_syncs_after_registry_built() {
    // Setup: temp dir with workspace/src/App.vue importing @/components/Child.vue
    // Use a non-dot-prefixed directory so tsconfig discovery doesn't skip it
    // (tsconfig discovery skips dot-directories).
    let temp_base_guard =
        tempfile::TempDir::with_prefix("verter_test_resync_aliased").expect("temp dir");
    let temp_base = temp_base_guard.path().to_path_buf();
    let workspace = temp_base.join("workspace");
    std::fs::create_dir_all(workspace.join("src/components")).expect("create dirs");

    // Write a tsconfig.json with @/* -> src/* alias
    std::fs::write(
        workspace.join("tsconfig.json"),
        r#"{
  "compilerOptions": {
    "baseUrl": ".",
    "paths": {
      "@/*": ["src/*"]
    }
  }
}"#,
    )
    .expect("write tsconfig");

    // Write the child component on disk
    let child_source = r#"<script setup lang="ts">
defineProps<{ msg: string }>()
</script>
<template><div>{{ msg }}</div></template>"#;
    std::fs::write(workspace.join("src/components/Child.vue"), child_source)
        .expect("write Child.vue");

    // Canonicalize workspace path for consistent IDs
    let workspace_id_raw = std::fs::canonicalize(&workspace)
        .expect("canonical workspace")
        .to_string_lossy()
        .replace('\\', "/");
    // Strip Windows extended-length prefix that canonicalize() produces
    let workspace_id = workspace_id_raw
        .strip_prefix("//?/")
        .unwrap_or(&workspace_id_raw)
        .to_string();
    let app_id = format!("{workspace_id}/src/App.vue");
    let child_id = format!("{workspace_id}/src/components/Child.vue");

    // App.vue imports Child via alias
    let app_source = r#"<script setup lang="ts">
import Child from '@/components/Child.vue'
</script>
<template><Child msg="hello" /></template>"#
        .to_string();

    let vfs_workspace: Arc<dyn verter_workspace::WorkspaceAccess> = Arc::new(
        verter_workspace::FilesystemWorkspace::new(verter_workspace::FilesystemOptions::default()),
    );
    let host = Arc::new(VerterHost::new(HostConfig::default(), vfs_workspace));
    let documents = DocumentRegistry::new(Arc::clone(&host));
    let uri = crate::uri::path_to_file_uri(&app_id).expect("file uri");
    let _ = documents.did_open(&TextDocumentItem {
        uri: uri.clone(),
        language_id: "vue".to_string(),
        version: 1,
        text: app_source,
    });

    // Phase 1: before VFS snapshot is built — aliased import should NOT resolve
    let analysis = host.get_analysis(&app_id).expect("analysis for App.vue");
    let ids_before = collect_imported_carrier_priority_ids_from_imports_for_publication(
        &verter_session::framework::HostLanguageClassifier::default(),
        &analysis.imports,
        Some(&app_id),
        |parent, specifier| resolve_import_specifier_standalone(&host, parent, specifier),
    );
    // The bootstrap root carries no configured project, which is a COMPLETE
    // context observation (the stable `unowned` context), not a provenance
    // gap — so the request is admitted. What it admits is a witnessed MISS:
    // the `@/*` alias has no mapping until the registry publishes one, and an
    // admitted miss publishes no carrier id.
    let ids_before = ids_before
        .expect("a complete bootstrap root admits the unowned alias request instead of refusing");
    assert!(
        ids_before.is_empty(),
        "the `@/*` alias must not resolve before the project registry is published, got: \
         {ids_before:?}"
    );

    // Phase 2: Build and populate project registry with tsconfig alias
    let workspace_uri = crate::uri::path_to_file_uri_string(&workspace_id);
    let vite_opts = verter_workspace::ViteConfigOptions::default();
    let registry_ws =
        verter_workspace::FilesystemWorkspace::new(verter_workspace::FilesystemOptions::default());
    let build_result = crate::config::ProjectRegistry::from_workspace_roots(
        &registry_ws,
        &[workspace_uri],
        &vite_opts,
    );
    let registry = build_result.registry;

    host.configure_projects(
        registry
            .projects()
            .iter()
            .map(|p| p.to_ide_project_config())
            .collect(),
    );
    let vfs_workspace = make_test_vfs_workspace_from_registry(&registry);

    // Now aliased import should resolve
    let ids_after = collect_imported_carrier_priority_ids_from_imports_for_publication(
        &verter_session::framework::HostLanguageClassifier::default(),
        &analysis.imports,
        Some(&app_id),
        |parent, specifier| resolve_import_specifier_standalone(&host, parent, specifier),
    );
    let ids_after = ids_after.expect("published project registry should admit alias resolution");
    assert!(
        !ids_after.is_empty(),
        "aliased imports should resolve after project_registry is populated"
    );
    assert!(
        ids_after.iter().any(|id| id.ends_with("Child.vue")),
        "resolved imports should include Child.vue, got: {ids_after:?}"
    );

    // Phase 3: resync_aliased_imports_for_open_files should sync .vue.ts
    let provider = Arc::new(MockTypeProvider::new());
    let sync = ProjectSync::new(provider.clone(), ProjectSyncMode::FullProject);
    let provider_sync_states = DashMap::new();

    resync_aliased_imports_for_open_files(
        &documents,
        Some(&sync),
        &vfs_workspace,
        &provider_sync_states,
        false,
        None,
        &DeclOverlayOwner::default(),
        1,
        &crate::external_ts::CarrierTransactionCoordinator::new(),
        &dashmap::DashSet::new(),
    )
    .await;

    // Positive: Child.vue should have its .vue.ts synced
    let calls = provider.file_sync_calls();
    assert!(
        calls.iter().any(|call| matches!(
            call,
            MockCall::OpenFile { path, .. } if path.contains("Child.vue.verter.ts")
        )),
        "resync should open the Child API carrier (Child.vue.verter.ts) in the type provider, calls={calls:?}"
    );

    // Positive: provider_sync_states should have the child entry
    assert!(
        provider_sync_states.get(&child_id).is_some()
            || provider_sync_states
                .iter()
                .any(|entry| entry.key().ends_with("Child.vue")),
        "provider_sync_states should contain Child.vue entry"
    );

    // Negative: .ts imports should NOT be synced via this path
    assert!(
        !calls.iter().any(|call| matches!(
            call,
            MockCall::OpenFile { path, .. } if path.ends_with(".ts") && !path.ends_with(".vue.verter.ts")
        )),
        "resync should NOT sync non-.vue files, calls={calls:?}"
    );

    // Cleanup
    let _ = std::fs::remove_dir_all(&temp_base);
}

#[tokio::test(flavor = "multi_thread")]
async fn declaration_closure_proactively_opens_transitive_decl_overlays() {
    // The proactive declaration-closure pass (tsgo) must open the `.d.<ext>.ts`
    // declaration overlay for EVERY carrier in the transitive closure reachable
    // from an open root, so a bare `import B from "./B.vue"` resolves with no
    // TS2307. Cover the TRANSITIVE case: A imports B imports C — opening A opens
    // B.d.vue.ts AND C.d.vue.ts. Also pin that a cycle (C imports A) terminates.
    let temp_base_guard =
        tempfile::TempDir::with_prefix("verter_test_decl_closure_transitive").expect("temp dir");
    let temp_base = temp_base_guard.path().to_path_buf();
    let workspace = temp_base.join("workspace");
    std::fs::create_dir_all(workspace.join("src")).expect("create dirs");
    std::fs::write(
        workspace.join("tsconfig.json"),
        r#"{ "compilerOptions": { "baseUrl": "." } }"#,
    )
    .expect("write tsconfig");

    // A -> B -> C -> A (cycle back to A exercises the visited-set termination).
    std::fs::write(
        workspace.join("src/C.vue"),
        "<script setup lang=\"ts\">\nimport A from './A.vue'\ndefineProps<{ c: string }>()\n</script>\n<template><A/></template>",
    )
    .expect("write C.vue");
    std::fs::write(
        workspace.join("src/B.vue"),
        "<script setup lang=\"ts\">\nimport C from './C.vue'\ndefineProps<{ b: string }>()\n</script>\n<template><C/></template>",
    )
    .expect("write B.vue");
    std::fs::write(
        workspace.join("src/A.vue"),
        "<script setup lang=\"ts\">\nimport B from './B.vue'\ndefineProps<{ a: string }>()\n</script>\n<template><B/></template>",
    )
    .expect("write A.vue");

    let workspace_id_raw = std::fs::canonicalize(&workspace)
        .expect("canonical workspace")
        .to_string_lossy()
        .replace('\\', "/");
    let workspace_id = workspace_id_raw
        .strip_prefix("//?/")
        .unwrap_or(&workspace_id_raw)
        .to_string();
    let a_id = format!("{workspace_id}/src/A.vue");

    let vfs_workspace: Arc<dyn verter_workspace::WorkspaceAccess> = Arc::new(
        verter_workspace::FilesystemWorkspace::new(verter_workspace::FilesystemOptions::default()),
    );
    let host = Arc::new(VerterHost::new(HostConfig::default(), vfs_workspace));
    let documents = DocumentRegistry::new(Arc::clone(&host));
    let uri = crate::uri::path_to_file_uri(&a_id).expect("file uri");
    let _ = documents.did_open(&TextDocumentItem {
        uri: uri.clone(),
        language_id: "vue".to_string(),
        version: 1,
        text: std::fs::read_to_string(workspace.join("src/A.vue")).unwrap(),
    });

    // Build + populate the project registry so imports resolve.
    let workspace_uri = crate::uri::path_to_file_uri_string(&workspace_id);
    let vite_opts = verter_workspace::ViteConfigOptions::default();
    let registry_ws =
        verter_workspace::FilesystemWorkspace::new(verter_workspace::FilesystemOptions::default());
    let build_result = crate::config::ProjectRegistry::from_workspace_roots(
        &registry_ws,
        &[workspace_uri],
        &vite_opts,
    );
    host.configure_projects(
        build_result
            .registry
            .projects()
            .iter()
            .map(|p| p.to_ide_project_config())
            .collect(),
    );
    let vfs_workspace = make_test_vfs_workspace_from_registry(&build_result.registry);

    // Run the drain as the tsgo engine so the closure pass fires.
    let provider = Arc::new(MockTypeProvider::new());
    let sync = ProjectSync::new(provider.clone(), ProjectSyncMode::FullProject);
    let provider_sync_states = DashMap::new();
    let decl_overlay_owner = DeclOverlayOwner::default();

    resync_aliased_imports_for_open_files(
        &documents,
        Some(&sync),
        &vfs_workspace,
        &provider_sync_states,
        true, // is_tsgo — the closure pass is tsgo-scoped
        None,
        &decl_overlay_owner,
        1,
        &crate::external_ts::CarrierTransactionCoordinator::new(),
        &dashmap::DashSet::new(),
    )
    .await;

    let calls = provider.file_sync_calls();
    let opened_decl = |needle: &str| {
        calls
            .iter()
            .any(|call| matches!(call, MockCall::OpenFile { path, .. } if path.contains(needle)))
    };

    // TRANSITIVE: both the direct dependency B AND the deep dependency C have
    // their declaration overlays opened by the closure walk from A.
    assert!(
        opened_decl("B.d.vue.ts"),
        "closure opens the direct dependency's declaration overlay (B.d.vue.ts), calls={calls:?}"
    );
    assert!(
        opened_decl("C.d.vue.ts"),
        "closure opens the TRANSITIVE dependency's declaration overlay (C.d.vue.ts), calls={calls:?}"
    );

    // Reachability recorded: both overlays are tracked as reached from root A.
    // The recorded root is A's canonical (matched by suffix — the host normalizes
    // the drive-letter case, so an exact-string compare against `a_id` would be a
    // path-normalization false negative, not a logic failure). `a_id` is used to
    // anchor the expectation; the recorded reaching-root set must be non-empty and
    // name A.
    let _ = &a_id;
    let reaches_a = |key_needle: &str| {
        decl_overlay_owner
            .test_slots_snapshot()
            .iter()
            .any(|(key, roots)| {
                key.contains(key_needle)
                    && !roots.is_empty()
                    && roots.iter().all(|root| root.ends_with("/A.vue"))
            })
    };
    assert!(
        reaches_a("B.d.vue.ts") && reaches_a("C.d.vue.ts"),
        "both B and C declaration overlays are recorded as reached from root A, slots={:?}",
        decl_overlay_owner.test_slots_snapshot()
    );

    // The cycle (C -> A) did not infinite-loop: we reached this assertion. A's own
    // declaration overlay IS opened (the per-root emission — every open carrier root
    // emits its own `.d.<ext>.ts`), but A is NOT re-walked as a DEPENDENCY through
    // the cycle (the visited set seeds with A so the C -> A edge is a no-op).
    assert!(
        opened_decl("A.d.vue.ts"),
        "the open root A emits its OWN declaration overlay (A.d.vue.ts), calls={calls:?}"
    );

    let _ = std::fs::remove_dir_all(&temp_base);
}

#[tokio::test(flavor = "multi_thread")]
async fn lone_leaf_carrier_opens_its_own_declaration_overlay() {
    // P1 #1 (root-emission): a SINGLE open leaf carrier — one that NOTHING imports
    // and that imports nothing — must still get its OWN `.d.<ext>.ts` declaration
    // overlay opened. tsgo resolves a bare `import Leaf from "./Leaf.vue"` (from any
    // future importer, or a same-file self-reference) to the virtual declaration via
    // its native probe, so the declaration must be live whenever the carrier is open
    // — independent of any OTHER open file reaching it through the closure.
    //
    // RED-before: the closure pass only opened DEPENDENCIES' declarations (it seeded
    // `visited` with the root and skipped it), and the main per-document sync only
    // opened the IDE `.vue.tsx` + API `.vue.verter.ts`. So a lone leaf got neither —
    // its `Leaf.d.vue.ts` was never opened.
    let temp_base_guard =
        tempfile::TempDir::with_prefix("verter_test_decl_closure_lone_leaf").expect("temp dir");
    let temp_base = temp_base_guard.path().to_path_buf();
    let workspace = temp_base.join("workspace");
    std::fs::create_dir_all(workspace.join("src")).expect("create dirs");
    std::fs::write(
        workspace.join("tsconfig.json"),
        r#"{ "compilerOptions": { "baseUrl": "." } }"#,
    )
    .expect("write tsconfig");

    // A lone leaf — imports nothing, imported by nothing.
    std::fs::write(
        workspace.join("src/Leaf.vue"),
        "<script setup lang=\"ts\">\ndefineProps<{ leaf: string }>()\n</script>\n<template><div/></template>",
    )
    .expect("write Leaf.vue");

    let workspace_id_raw = std::fs::canonicalize(&workspace)
        .expect("canonical workspace")
        .to_string_lossy()
        .replace('\\', "/");
    let workspace_id = workspace_id_raw
        .strip_prefix("//?/")
        .unwrap_or(&workspace_id_raw)
        .to_string();
    let leaf_id = format!("{workspace_id}/src/Leaf.vue");

    let vfs_workspace: Arc<dyn verter_workspace::WorkspaceAccess> = Arc::new(
        verter_workspace::FilesystemWorkspace::new(verter_workspace::FilesystemOptions::default()),
    );
    let host = Arc::new(VerterHost::new(HostConfig::default(), vfs_workspace));
    let documents = DocumentRegistry::new(Arc::clone(&host));
    let uri = crate::uri::path_to_file_uri(&leaf_id).expect("file uri");
    let _ = documents.did_open(&TextDocumentItem {
        uri: uri.clone(),
        language_id: "vue".to_string(),
        version: 1,
        text: std::fs::read_to_string(workspace.join("src/Leaf.vue")).unwrap(),
    });

    let workspace_uri = crate::uri::path_to_file_uri_string(&workspace_id);
    let vite_opts = verter_workspace::ViteConfigOptions::default();
    let registry_ws =
        verter_workspace::FilesystemWorkspace::new(verter_workspace::FilesystemOptions::default());
    let build_result = crate::config::ProjectRegistry::from_workspace_roots(
        &registry_ws,
        &[workspace_uri],
        &vite_opts,
    );
    host.configure_projects(
        build_result
            .registry
            .projects()
            .iter()
            .map(|p| p.to_ide_project_config())
            .collect(),
    );
    let vfs_workspace = make_test_vfs_workspace_from_registry(&build_result.registry);

    let provider = Arc::new(MockTypeProvider::new());
    let sync = ProjectSync::new(provider.clone(), ProjectSyncMode::FullProject);
    let provider_sync_states = DashMap::new();
    let decl_overlay_owner = DeclOverlayOwner::default();

    resync_aliased_imports_for_open_files(
        &documents,
        Some(&sync),
        &vfs_workspace,
        &provider_sync_states,
        true, // is_tsgo — the closure pass is tsgo-scoped
        None,
        &decl_overlay_owner,
        1,
        &crate::external_ts::CarrierTransactionCoordinator::new(),
        &dashmap::DashSet::new(),
    )
    .await;

    let calls = provider.file_sync_calls();
    let opened_decl = |needle: &str| {
        calls
            .iter()
            .any(|call| matches!(call, MockCall::OpenFile { path, .. } if path.contains(needle)))
    };

    assert!(
        opened_decl("Leaf.d.vue.ts"),
        "a lone leaf carrier opens its OWN declaration overlay (Leaf.d.vue.ts), calls={calls:?}"
    );

    // The lone leaf's own declaration overlay is recorded as reached from itself
    // (a self-edge), so the `did_close` lifecycle retires it when the leaf closes.
    let _ = &leaf_id;
    let self_reached = decl_overlay_owner
        .test_slots_snapshot()
        .iter()
        .any(|(key, roots)| {
            key.contains("Leaf.d.vue.ts")
                && !roots.is_empty()
                && roots.iter().all(|root| root.ends_with("/Leaf.vue"))
        });
    assert!(
        self_reached,
        "the lone leaf's declaration overlay records a self-edge (reached from Leaf.vue), slots={:?}",
        decl_overlay_owner.test_slots_snapshot()
    );

    let _ = std::fs::remove_dir_all(&temp_base);
}

#[tokio::test(flavor = "multi_thread")]
async fn stale_pass_does_not_reopen_a_declaration_overlay_a_newer_pass_closed() {
    // The SYMMETRIC ADD gate at the PRODUCTION OPEN site (`open_overlay`): a STALE
    // closure pass must NOT re-open a declaration overlay once a NEWER pass has
    // authoritatively reconciled the root. Two overlapping `background_init` passes
    // (an older, in-flight pass + a newer pass) race on one shared owner; the older
    // pass's `open_overlay` could otherwise resurrect a `.d.<ext>.ts` overlay the
    // newer pass already determined unreachable and CLOSED, leaving it provider-
    // visible until the next reconcile of that root (potentially never).
    //
    // Driven deterministically through the REAL pass entry point
    // (`resync_aliased_imports_for_open_files` → `open_declaration_closure_for_open_files`):
    // the root's authoritative high-water mark is pre-advanced to model "a newer pass
    // already reconciled this root", then the pass is run at an OLDER generation. The
    // older pass's `open_overlay` for the root's own overlay must be GATED.
    //
    // RED-before (no ADD gate): the older pass re-opens the overlay (the open is
    // ungated and monotonic-max re-records the edge). Post-fix: the per-root
    // high-water gate rejects it.
    let temp_base_guard =
        tempfile::TempDir::with_prefix("verter_test_decl_closure_stale_reopen").expect("temp dir");
    let temp_base = temp_base_guard.path().to_path_buf();
    let workspace = temp_base.join("workspace");
    std::fs::create_dir_all(workspace.join("src")).expect("create dirs");
    std::fs::write(
        workspace.join("tsconfig.json"),
        r#"{ "compilerOptions": { "baseUrl": "." } }"#,
    )
    .expect("write tsconfig");
    std::fs::write(
        workspace.join("src/Root.vue"),
        "<script setup lang=\"ts\">\ndefineProps<{ root: string }>()\n</script>\n<template><div/></template>",
    )
    .expect("write Root.vue");

    let workspace_id_raw = std::fs::canonicalize(&workspace)
        .expect("canonical workspace")
        .to_string_lossy()
        .replace('\\', "/");
    let workspace_id = workspace_id_raw
        .strip_prefix("//?/")
        .unwrap_or(&workspace_id_raw)
        .to_string();
    let root_id = format!("{workspace_id}/src/Root.vue");

    let vfs_for_host: Arc<dyn verter_workspace::WorkspaceAccess> = Arc::new(
        verter_workspace::FilesystemWorkspace::new(verter_workspace::FilesystemOptions::default()),
    );
    let host = Arc::new(VerterHost::new(HostConfig::default(), vfs_for_host));
    let documents = DocumentRegistry::new(Arc::clone(&host));
    let uri = crate::uri::path_to_file_uri(&root_id).expect("file uri");
    let _ = documents.did_open(&TextDocumentItem {
        uri: uri.clone(),
        language_id: "vue".to_string(),
        version: 1,
        text: std::fs::read_to_string(workspace.join("src/Root.vue")).unwrap(),
    });

    let workspace_uri = crate::uri::path_to_file_uri_string(&workspace_id);
    let vite_opts = verter_workspace::ViteConfigOptions::default();
    let registry_ws =
        verter_workspace::FilesystemWorkspace::new(verter_workspace::FilesystemOptions::default());
    let build_result = crate::config::ProjectRegistry::from_workspace_roots(
        &registry_ws,
        &[workspace_uri],
        &vite_opts,
    );
    host.configure_projects(
        build_result
            .registry
            .projects()
            .iter()
            .map(|p| p.to_ide_project_config())
            .collect(),
    );
    let vfs_workspace = make_test_vfs_workspace_from_registry(&build_result.registry);

    let provider = Arc::new(MockTypeProvider::new());
    let sync = ProjectSync::new(provider.clone(), ProjectSyncMode::FullProject);
    let provider_sync_states = DashMap::new();
    let decl_overlay_owner = DeclOverlayOwner::default();

    let opened_root_decl = |calls: &[MockCall]| {
        calls
            .iter()
            .any(|call| matches!(call, MockCall::OpenFile { path, .. } if path.contains("Root.d.vue.ts")))
    };

    // Model "a NEWER pass already authoritatively reconciled Root": advance its
    // per-root high-water mark to 100. A pass OLDER than 100 is now stale for Root.
    // Seed under the EXACT canonical id the closure pass walks (the
    // `DocumentRegistry`'s id), not a hand-built path — the gate keys on it.
    let root_canonical = documents
        .get_canonical_id(&uri)
        .expect("canonical id for the open Root.vue");
    decl_overlay_owner.test_advance_root_authoritative_epoch(&root_canonical, 100);

    // The STALE pass (gen 5) must NOT (re-)open Root's declaration overlay — the
    // `open_overlay` ADD gate rejects it.
    resync_aliased_imports_for_open_files(
        &documents,
        Some(&sync),
        &vfs_workspace,
        &provider_sync_states,
        true, // is_tsgo — the closure pass is tsgo-scoped
        None,
        &decl_overlay_owner,
        5,
        &crate::external_ts::CarrierTransactionCoordinator::new(),
        &dashmap::DashSet::new(),
    )
    .await;
    let calls_after_stale = provider.file_sync_calls();
    assert!(
        !opened_root_decl(&calls_after_stale),
        "a STALE pass (gen 5, below Root's high-water 100) must NOT open Root.d.vue.ts — \
         the open_overlay ADD gate; calls={calls_after_stale:?}"
    );
    assert!(
        !decl_overlay_owner
            .test_slots_snapshot()
            .iter()
            .any(|(key, roots)| key.contains("Root.d.vue.ts") && !roots.is_empty()),
        "a STALE pass must not record a reaching edge for the gated overlay either; slots={:?}",
        decl_overlay_owner.test_slots_snapshot()
    );

    // INVERSE: a CURRENT pass (gen 101, at/over the high-water) DOES open the overlay
    // — the gate is not a blanket skip.
    resync_aliased_imports_for_open_files(
        &documents,
        Some(&sync),
        &vfs_workspace,
        &provider_sync_states,
        true,
        None,
        &decl_overlay_owner,
        101,
        &crate::external_ts::CarrierTransactionCoordinator::new(),
        &dashmap::DashSet::new(),
    )
    .await;
    let calls_after_current = provider.file_sync_calls();
    assert!(
        opened_root_decl(&calls_after_current),
        "a CURRENT pass (gen 101, at/over the high-water) DOES open Root.d.vue.ts — the \
         ADD gate admits authoritative opens; calls={calls_after_current:?}"
    );

    let _ = std::fs::remove_dir_all(&temp_base);
}

#[tokio::test(flavor = "multi_thread")]
async fn stale_open_gated_when_high_water_advances_between_its_gate_and_record() {
    // The ATOMICITY of the `open_overlay` ADD gate: the high-water READ and the slot
    // RECORD must be one critical section, so a newer reconcile advancing the per-root
    // high-water cannot land BETWEEN an older open's gate and its record. If it could,
    // the older open would slip the gate (read the pre-advance value) and then
    // re-record/re-open a `.d.<ext>.ts` overlay the newer pass already reconciled away
    // and closed — a lingering re-opened overlay with no future closer.
    //
    // Driven through the REAL pass entry (`resync_aliased_imports_for_open_files` →
    // `open_declaration_closure_for_open_files` → `open_overlay`), with the interleave
    // FORCED deterministically: the older pass (gen 5) blocks at the add-gate seam, the
    // test then advances Root's authoritative high-water to 100 (modelling the newer
    // pass), and only then releases the older open. The atomic gate-and-record observes
    // the advanced high-water under its shared entry guard and GATES the open.
    //
    // RED-before (gate read + record NOT under one guard, i.e. the gate dropped from
    // `gate_and_record_root_edge`): the older open records and `open_dts` re-opens
    // Root.d.vue.ts after the advance. GREEN-after: the shared guard gates it.
    let temp_base_guard =
        tempfile::TempDir::with_prefix("verter_test_decl_closure_stale_open_interleave")
            .expect("temp dir");
    let temp_base = temp_base_guard.path().to_path_buf();
    let workspace = temp_base.join("workspace");
    std::fs::create_dir_all(workspace.join("src")).expect("create dirs");
    std::fs::write(
        workspace.join("tsconfig.json"),
        r#"{ "compilerOptions": { "baseUrl": "." } }"#,
    )
    .expect("write tsconfig");
    std::fs::write(
        workspace.join("src/Root.vue"),
        "<script setup lang=\"ts\">\ndefineProps<{ root: string }>()\n</script>\n<template><div/></template>",
    )
    .expect("write Root.vue");

    let workspace_id_raw = std::fs::canonicalize(&workspace)
        .expect("canonical workspace")
        .to_string_lossy()
        .replace('\\', "/");
    let workspace_id = workspace_id_raw
        .strip_prefix("//?/")
        .unwrap_or(&workspace_id_raw)
        .to_string();
    let root_id = format!("{workspace_id}/src/Root.vue");

    let vfs_for_host: Arc<dyn verter_workspace::WorkspaceAccess> = Arc::new(
        verter_workspace::FilesystemWorkspace::new(verter_workspace::FilesystemOptions::default()),
    );
    let host = Arc::new(VerterHost::new(HostConfig::default(), vfs_for_host));
    let documents = DocumentRegistry::new(Arc::clone(&host));
    let uri = crate::uri::path_to_file_uri(&root_id).expect("file uri");
    let _ = documents.did_open(&TextDocumentItem {
        uri: uri.clone(),
        language_id: "vue".to_string(),
        version: 1,
        text: std::fs::read_to_string(workspace.join("src/Root.vue")).unwrap(),
    });

    let workspace_uri = crate::uri::path_to_file_uri_string(&workspace_id);
    let vite_opts = verter_workspace::ViteConfigOptions::default();
    let registry_ws =
        verter_workspace::FilesystemWorkspace::new(verter_workspace::FilesystemOptions::default());
    let build_result = crate::config::ProjectRegistry::from_workspace_roots(
        &registry_ws,
        &[workspace_uri],
        &vite_opts,
    );
    host.configure_projects(
        build_result
            .registry
            .projects()
            .iter()
            .map(|p| p.to_ide_project_config())
            .collect(),
    );
    let vfs_workspace = make_test_vfs_workspace_from_registry(&build_result.registry);

    let provider = Arc::new(MockTypeProvider::new());
    let sync = ProjectSync::new(provider.clone(), ProjectSyncMode::FullProject);
    let provider_sync_states = DashMap::new();
    let decl_overlay_owner = DeclOverlayOwner::default();

    let opened_root_decl = |calls: &[MockCall]| {
        calls
            .iter()
            .any(|call| matches!(call, MockCall::OpenFile { path, .. } if path.contains("Root.d.vue.ts")))
    };

    let root_canonical = documents
        .get_canonical_id(&uri)
        .expect("canonical id for the open Root.vue");

    // Arm the add-gate interleave seam, then run the OLDER pass (gen 5) CONCURRENTLY
    // with an interleave task that advances Root's high-water to 100 in the exact
    // window between the open's gate and its record. The high-water starts at 0, so the
    // older open passes any pre-seam state and is only gated by the atomic re-read.
    let interleave = decl_overlay_owner.arm_add_gate_interleave_for_test(&root_canonical);
    let carrier_coordinator = crate::external_ts::CarrierTransactionCoordinator::new();
    let pending = dashmap::DashSet::new();
    let pass = resync_aliased_imports_for_open_files(
        &documents,
        Some(&sync),
        &vfs_workspace,
        &provider_sync_states,
        true,
        None,
        &decl_overlay_owner,
        5,
        &carrier_coordinator,
        &pending,
    );
    let advance_between_gate_and_record = async {
        // Wait until the older open reaches the add-gate seam (after its path-lock +
        // root-open revalidation, before the atomic gate-record)...
        interleave.reached.notified().await;
        // ...model a NEWER pass authoritatively reconciling Root, advancing the
        // per-root high-water to 100 — landing strictly between the older open's gate
        // and its record...
        decl_overlay_owner.test_advance_root_authoritative_epoch(&root_canonical, 100);
        // ...then release the older open into its atomic gate-and-record.
        interleave.proceed.notify_one();
    };
    tokio::join!(pass, advance_between_gate_and_record);

    let calls_after_stale = provider.file_sync_calls();
    assert!(
        !opened_root_decl(&calls_after_stale),
        "a newer pass that advanced Root's high-water (100) BETWEEN the older open's \
         (gen 5) gate and its record must leave the older open GATED — Root.d.vue.ts \
         must NOT be (re-)opened; calls={calls_after_stale:?}"
    );
    assert!(
        !decl_overlay_owner
            .test_slots_snapshot()
            .iter()
            .any(|(key, roots)| key.contains("Root.d.vue.ts") && !roots.is_empty()),
        "the gated open must not record a reaching edge either; slots={:?}",
        decl_overlay_owner.test_slots_snapshot()
    );

    // INVERSE: a CURRENT pass (gen 101, at/over the advanced high-water 100) DOES open
    // the overlay — the atomic gate admits authoritative opens, it is not a blanket
    // skip. The seam is one-shot (consumed by the gen-5 pass), so this pass is unblocked.
    resync_aliased_imports_for_open_files(
        &documents,
        Some(&sync),
        &vfs_workspace,
        &provider_sync_states,
        true,
        None,
        &decl_overlay_owner,
        101,
        &crate::external_ts::CarrierTransactionCoordinator::new(),
        &dashmap::DashSet::new(),
    )
    .await;
    let calls_after_current = provider.file_sync_calls();
    assert!(
        opened_root_decl(&calls_after_current),
        "a CURRENT pass (gen 101, at/over the high-water) DOES open Root.d.vue.ts — the \
         atomic gate admits authoritative opens; calls={calls_after_current:?}"
    );

    let _ = std::fs::remove_dir_all(&temp_base);
}

#[tokio::test(flavor = "multi_thread")]
async fn closure_final_reconcile_drops_root_that_closed_mid_pass() {
    // P1 #2 (did_close-vs-pass race) at the PRODUCTION CALL SITE — NOT just the
    // pure primitive. The closure pass snapshots the open carrier roots at its
    // START, then `.await`s the overlay opens, then reconciles the refcount against
    // a re-read of the open set. If a root closes DURING the pass (after the
    // snapshot, while its overlay is being opened), its `did_close` releases its
    // edges, the per-root reconcile RE-records them, and the final reconcile must
    // drop it — otherwise the re-added edges + overlay leak permanently (no future
    // close event exists for a root that is no longer open).
    //
    // This drives the real async pass (`open_declaration_closure_for_open_files` via
    // `resync_aliased_imports_for_open_files`) and interleaves the close
    // DETERMINISTICALLY: the mock fires a one-shot callback the moment A's own
    // declaration overlay (`A.d.vue.ts`) is opened, and that callback closes A in
    // the `DocumentRegistry`. So by the time the pass reaches its final reconcile,
    // A is no longer open.
    //
    // The invariant: the final reconcile must NOT use the START-of-pass snapshot
    // (which still contains A), or A's re-recorded self-edge is KEPT and the overlay
    // is never closed — a permanent leak. Instead the call site
    // RE-READS the open set after the async work, so the now-closed A is dropped
    // and its solely-A overlay is returned for close.
    let temp_base_guard = tempfile::TempDir::with_prefix("verter_test_decl_closure_close_mid_pass")
        .expect("temp dir");
    let temp_base = temp_base_guard.path().to_path_buf();
    let workspace = temp_base.join("workspace");
    std::fs::create_dir_all(workspace.join("src")).expect("create dirs");
    std::fs::write(
        workspace.join("tsconfig.json"),
        r#"{ "compilerOptions": { "baseUrl": "." } }"#,
    )
    .expect("write tsconfig");

    // A lone carrier — its own declaration is the only overlay the pass opens, so
    // the interleave point (the open of `A.d.vue.ts`) is unambiguous and the
    // refcount holds exactly one slot (A's self-edge).
    std::fs::write(
        workspace.join("src/A.vue"),
        "<script setup lang=\"ts\">\ndefineProps<{ a: string }>()\n</script>\n<template><div/></template>",
    )
    .expect("write A.vue");

    let workspace_id_raw = std::fs::canonicalize(&workspace)
        .expect("canonical workspace")
        .to_string_lossy()
        .replace('\\', "/");
    let workspace_id = workspace_id_raw
        .strip_prefix("//?/")
        .unwrap_or(&workspace_id_raw)
        .to_string();
    let a_id = format!("{workspace_id}/src/A.vue");

    let vfs_workspace_access: Arc<dyn verter_workspace::WorkspaceAccess> = Arc::new(
        verter_workspace::FilesystemWorkspace::new(verter_workspace::FilesystemOptions::default()),
    );
    let host = Arc::new(VerterHost::new(HostConfig::default(), vfs_workspace_access));
    // `documents` is shared into the mock's open-file callback so the callback can
    // close A mid-pass through the SAME registry the pass re-reads.
    let documents = Arc::new(DocumentRegistry::new(Arc::clone(&host)));
    let uri = crate::uri::path_to_file_uri(&a_id).expect("file uri");
    let _ = documents.did_open(&TextDocumentItem {
        uri: uri.clone(),
        language_id: "vue".to_string(),
        version: 1,
        text: std::fs::read_to_string(workspace.join("src/A.vue")).unwrap(),
    });

    let workspace_uri = crate::uri::path_to_file_uri_string(&workspace_id);
    let vite_opts = verter_workspace::ViteConfigOptions::default();
    let registry_ws =
        verter_workspace::FilesystemWorkspace::new(verter_workspace::FilesystemOptions::default());
    let build_result = crate::config::ProjectRegistry::from_workspace_roots(
        &registry_ws,
        &[workspace_uri],
        &vite_opts,
    );
    host.configure_projects(
        build_result
            .registry
            .projects()
            .iter()
            .map(|p| p.to_ide_project_config())
            .collect(),
    );
    let vfs_workspace = make_test_vfs_workspace_from_registry(&build_result.registry);

    let provider = Arc::new(MockTypeProvider::new());
    let sync = ProjectSync::new(provider.clone(), ProjectSyncMode::FullProject);
    let provider_sync_states = DashMap::new();
    let decl_overlay_owner = DeclOverlayOwner::default();

    // The exact declaration-overlay path the pass opens for A (computed the same
    // way production does), so the interleave callback is armed for that precise
    // open. The pass SEEDS the root itself, so this is the FIRST overlay open.
    let a_decl_path = host
        .declaration_carrier_path(&a_id)
        .expect("A.vue projects a declaration carrier path");
    assert!(
        a_decl_path.contains("A.d.vue.ts"),
        "sanity: A's declaration overlay path is A.d.vue.ts, got {a_decl_path}"
    );

    // Deterministic mid-pass close: when `A.d.vue.ts` is opened (after the pass has
    // already snapshotted A as a live root), close A in the registry. The pass then
    // re-records A's self-edge in its per-root reconcile; only a final reconcile
    // against a FRESH (now-A-absent) open set removes it again.
    {
        let documents_for_cb = Arc::clone(&documents);
        let uri_for_cb = uri.clone();
        provider.set_on_open_file(
            &a_decl_path,
            Box::new(move || {
                documents_for_cb.did_close(&uri_for_cb);
            }),
        );
    }

    resync_aliased_imports_for_open_files(
        &documents,
        Some(&sync),
        &vfs_workspace,
        &provider_sync_states,
        true, // is_tsgo — the closure pass is tsgo-scoped
        None,
        &decl_overlay_owner,
        1,
        &crate::external_ts::CarrierTransactionCoordinator::new(),
        &dashmap::DashSet::new(),
    )
    .await;

    // The interleave actually happened: A's overlay open fired (so the callback ran
    // and A was closed mid-pass), and A is no longer open afterwards.
    let calls = provider.file_sync_calls();
    let opened_a_decl = calls
        .iter()
        .any(|call| matches!(call, MockCall::OpenFile { path, .. } if path == &a_decl_path));
    assert!(
        opened_a_decl,
        "the pass opened A's declaration overlay (arming the mid-pass close), calls={calls:?}"
    );
    assert!(
        !documents.open_uris().iter().any(|u| u == uri.as_str()),
        "A was closed mid-pass via the open-file callback"
    );

    // INVARIANT (the fix): after the pass, NO refcount edge exists for the
    // now-closed root A. RED-before this would still hold `A.d.vue.ts -> {A}`.
    let a_edge_remains = decl_overlay_owner
        .test_slots_snapshot()
        .iter()
        .any(|(_, roots)| roots.iter().any(|root| root.ends_with("/A.vue")));
    assert!(
        !a_edge_remains,
        "a root closed mid-pass leaves NO reaching-root edge — the stale start-of-pass \
         snapshot must NOT keep A; slots={:?}",
        decl_overlay_owner.test_slots_snapshot()
    );

    // INVARIANT (the fix): the overlay attributable SOLELY to the closed root A is
    // CLOSED by the final reconcile. RED-before this CloseFile never happened.
    let closed_a_decl = calls
        .iter()
        .any(|call| matches!(call, MockCall::CloseFile { path } if path == &a_decl_path));
    assert!(
        closed_a_decl,
        "A's solely-reached declaration overlay is CLOSED once A closes mid-pass, calls={calls:?}"
    );

    let _ = std::fs::remove_dir_all(&temp_base);
}

/// D1 (overlay LEAK — `handle_did_close` ordering) at the REAL handler. The
/// `did_close` handler captures the canonical id, then runs TWO halves whose
/// relative order is the bug under test: `[RELEASE]`
/// (`release_declaration_overlays_for_closed_root` — drops the closing root's
/// reachability edges and closes any solely-its overlay) and `[DIDCLOSE]`
/// (`documents.did_close` — removes the root from the open set the closure pass
/// reads). If `[RELEASE]` runs BEFORE `[DIDCLOSE]`, a closure pass whose
/// per-root + final reconcile lands in the `[RELEASE]..[DIDCLOSE]` window
/// RE-RECORDS the just-released edge while the root is STILL in `open_uris()`,
/// so the final reconcile KEEPS it — a permanent overlay leak (no future close
/// event exists for a root that is no longer open). The fix orders `[DIDCLOSE]`
/// before `[RELEASE]`: the pass then observes the root as closed and drops the
/// re-recorded edge.
///
/// This drives the REAL `handle_did_close` (not just `documents.did_close`)
/// racing the REAL `open_declaration_closure_for_open_files` on the server's ONE
/// shared `decl_overlay_refcount`. The interleave is DETERMINISTIC, not a timing
/// race: the mock pauses the closing task INSIDE `[RELEASE]`'s overlay close
/// (`block_close_file`), and the closure pass's re-record is run precisely in
/// that paused window via `tokio::join!`-driven gate coordination. All three
/// futures share `&server` (no spawn / no `'static` server handle needed); the
/// join polls the closure-pass future while the handler is suspended on the
/// close gate.
///
/// RED-before (pre-reorder, `[RELEASE]` then `[DIDCLOSE]`): the re-record lands
/// while A is still open → `A.d.vue.ts -> {A}` is KEPT → the leak assertion
/// FAILS. GREEN-after (`[DIDCLOSE]` then `[RELEASE]`): A leaves the open set
/// before the pass re-reads it → the edge is dropped → no leak.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn did_close_orders_didclose_before_release_so_no_overlay_leak_at_real_handler() {
    let temp_base_guard =
        tempfile::TempDir::with_prefix("verter_test_did_close_release_order_leak")
            .expect("temp dir");
    let temp_base = temp_base_guard.path().to_path_buf();
    let workspace = temp_base.join("workspace");
    std::fs::create_dir_all(workspace.join("src")).expect("create dirs");
    std::fs::write(
        workspace.join("tsconfig.json"),
        r#"{ "compilerOptions": { "baseUrl": "." } }"#,
    )
    .expect("write tsconfig");

    // A lone carrier — its own declaration is the only overlay the pass opens, so
    // the refcount holds exactly one slot (A's self-edge) and the close gate fires
    // on exactly that one overlay.
    std::fs::write(
        workspace.join("src/A.vue"),
        "<script setup lang=\"ts\">\ndefineProps<{ a: string }>()\n</script>\n<template><div/></template>",
    )
    .expect("write A.vue");

    let workspace_id_raw = std::fs::canonicalize(&workspace)
        .expect("canonical workspace")
        .to_string_lossy()
        .replace('\\', "/");
    let workspace_id = workspace_id_raw
        .strip_prefix("//?/")
        .unwrap_or(&workspace_id_raw)
        .to_string();
    let a_id = format!("{workspace_id}/src/A.vue");

    let vfs_workspace_access: Arc<dyn verter_workspace::WorkspaceAccess> = Arc::new(
        verter_workspace::FilesystemWorkspace::new(verter_workspace::FilesystemOptions::default()),
    );
    let host = Arc::new(VerterHost::new(HostConfig::default(), vfs_workspace_access));

    let workspace_uri = crate::uri::path_to_file_uri_string(&workspace_id);
    let vite_opts = verter_workspace::ViteConfigOptions::default();
    let registry_ws =
        verter_workspace::FilesystemWorkspace::new(verter_workspace::FilesystemOptions::default());
    let build_result = crate::config::ProjectRegistry::from_workspace_roots(
        &registry_ws,
        &[workspace_uri],
        &vite_opts,
    );
    host.configure_projects(
        build_result
            .registry
            .projects()
            .iter()
            .map(|p| p.to_ide_project_config())
            .collect(),
    );
    let vfs_workspace = make_test_vfs_workspace_from_registry(&build_result.registry)
        .into_inner()
        .expect("the test VFS workspace publishes a snapshot");

    let provider = Arc::new(MockTypeProvider::new());
    let type_provider: Arc<dyn TypeProvider> = provider.clone();
    let host_for_server = Arc::clone(&host);
    let type_provider_for_server = Arc::clone(&type_provider);
    let (service, _socket) = tower_lsp_server::LspService::new(move |_client| {
        VerterLanguageServer::new(
            crate::outbound::Outbound::default(),
            LspConfig {
                host: Arc::clone(&host_for_server),
                type_provider: Some(Arc::clone(&type_provider_for_server)),
                project_sync_mode: crate::ProjectSyncMode::FullProject,
                // The proactive declaration-overlay graph is a tsgo-only concern.
                type_provider_kind: crate::TypeProviderKind::Tsgo,
                type_provider_topology: crate::TypeProviderTopology::ManagedTsgo,
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
    server.install_vfs_workspace(vfs_workspace);

    let uri = crate::uri::path_to_file_uri(&a_id).expect("file uri");
    let _ = server.test_documents().did_open(&TextDocumentItem {
        uri: uri.clone(),
        language_id: "vue".to_string(),
        version: 1,
        text: std::fs::read_to_string(workspace.join("src/A.vue")).unwrap(),
    });

    // Re-bind `a_id` to the EXACT canonical id the registry assigns this URI — the
    // same string the closure pass records into the refcount — so every later
    // comparison is exact (the host normalises drive-letter case on Windows, so a
    // `canonicalize`-derived id can differ in case from the registry's).
    let a_id = server
        .test_documents()
        .get_canonical_id(&uri)
        .expect("A.vue has a canonical id after did_open");

    let a_decl_path = host
        .declaration_carrier_path(&a_id)
        .expect("A.vue projects a declaration carrier path");
    assert!(
        a_decl_path.contains("A.d.vue.ts"),
        "sanity: A's declaration overlay path is A.d.vue.ts, got {a_decl_path}"
    );

    // PRIME the refcount: one real closure pass with A open records A's self-edge
    // (`A.d.vue.ts -> {A}`) so the racing `[RELEASE]` actually has an overlay to
    // close — that close is the deterministic interleave point.
    server.test_run_declaration_closure_pass(1).await;
    let primed = server
        .test_decl_overlay_owner()
        .test_slot_roots(&a_decl_path)
        .is_some_and(|roots| roots.iter().any(|r| r == &a_id));
    assert!(
        primed,
        "priming pass records A's self-edge A.d.vue.ts -> {{A}}, slots={:?}",
        server.test_decl_overlay_owner().test_slots_snapshot()
    );

    // Arm the deterministic interleave: `[RELEASE]`'s close of A.d.vue.ts pauses
    // INSIDE the provider (signals `arrived`, awaits `release`).
    let (arrived, release) = provider.block_close_file(&a_decl_path);
    let pass_done = Arc::new(tokio::sync::Notify::new());

    // H — the REAL did_close handler. Pre-fix it runs [RELEASE] then [DIDCLOSE];
    // post-fix [DIDCLOSE] then [RELEASE]. Borrows `&server`.
    let handler = super::super::lifecycle::handle_did_close(
        server,
        DidCloseTextDocumentParams {
            text_document: TextDocumentIdentifier { uri: uri.clone() },
        },
    );

    // P — the REAL closure pass, run PRECISELY in the [RELEASE]'s-close window:
    // it waits until the handler is paused in the overlay close, then re-records
    // against whatever open set exists at that instant. Borrows `&server`.
    let pass_done_for_p = Arc::clone(&pass_done);
    let pass = async {
        arrived.notified().await;
        server.test_run_declaration_closure_pass(1).await;
        pass_done_for_p.notify_one();
    };

    // R — release the handler's paused close only AFTER the pass re-record has
    // landed, so the re-record is guaranteed to fall inside the window. Touches
    // only the notifies (no server borrow).
    let releaser = async {
        pass_done.notified().await;
        release.notify_one();
    };

    tokio::join!(handler, pass, releaser);

    // The handler ran to completion: A is closed in the registry.
    assert!(
        !server
            .test_documents()
            .open_uris()
            .iter()
            .any(|u| u == uri.as_str()),
        "the real did_close handler closed A"
    );

    // INVARIANT (the fix): after the real handler + the interleaved closure pass,
    // NO refcount edge for the now-closed root A remains — its solely-A overlay is
    // not leaked. RED-before (pre-reorder) this FAILS: the re-record landed while
    // A was still in open_uris(), so A.d.vue.ts -> {A} was KEPT.
    let leaked = server
        .test_decl_overlay_owner()
        .test_slots_snapshot()
        .iter()
        .any(|(_, roots)| roots.iter().any(|root| root == &a_id));
    assert!(
        !leaked,
        "a root closed by the REAL did_close handler must leave NO reaching-root edge — \
         [RELEASE] before [DIDCLOSE] re-records + KEEPS the edge (leak); \
         slots={:?}",
        server.test_decl_overlay_owner().test_slots_snapshot()
    );

    drain.abort();
    let _ = std::fs::remove_dir_all(&temp_base);
}

/// DECLARATION-OVERLAY CLOSE-VS-REOPEN serialization: a provider `close_dts` of a
/// shared overlay, decided when one root drains it, must NOT strand a DIFFERENT
/// still-open root that reaches the same overlay. The owner serializes the close
/// and a concurrent reopen of the same overlay path behind that path's lock, so the
/// reopen waits for the close to finish and then re-establishes the overlay — the
/// provider and the reachability graph end in agreement, never with the overlay
/// closed while a live root reaches it (TS2307 stranding).
///
/// EXACT interleave (engineered so the stale close's open-set REMOVAL is sequenced
/// strictly AFTER S's reopen of the same overlay):
///   * R is the SOLE root reaching `Shared.d.vue.ts` at release time.
///   * R closes via the REAL `handle_did_close`: `release_…_for_closed_root` drops
///     R, computes `now_unreferenced = [Shared.d.vue.ts, R.d.vue.ts]`, and the
///     owner's guarded close acquires `Shared.d.vue.ts`'s path lock and issues the
///     provider close — which PAUSES inside the close, HOLDING the lock (the
///     open-set removal has NOT applied yet).
///   * Concurrently the closure pass for the still-open S re-opens `Shared.d.vue.ts`
///     (S's bare `import Shared from "./Shared.vue"` needs the overlay).
///   * R's paused close is released ONLY after S's reopen of `Shared.d.vue.ts` has
///     landed (signalled by the provider open EFFECT) — or, when S is lock-blocked
///     and cannot reopen until R frees the lock, after a bounded fallback. R then
///     APPLIES its open-set removal and frees the lock.
///
/// ASSERT (no stranding): the provider STILL has `Shared.d.vue.ts` open AND the
/// owner records `{Shared.d.vue.ts -> S}` — the graph and the provider AGREE.
///
/// The provider is the FAITHFUL [`GatedDeclOverlayProvider`] (open/close EFFECT at
/// await-COMPLETION, mirroring the real `ExtensionTypeProvider`), NOT
/// `MockTypeProvider` (records at call-ENTRY, masking the ordering).
///
/// FORCED DISCRIMINATOR (no timeout): R's gated close is released by the FIRST of
/// exactly TWO mutually-exclusive owner observations — a `select!` with no sleep
/// arm. On a tree WITHOUT the owner's per-path serialization, S's reopen is not
/// lock-gated, so it lands WHILE R's close is paused and fires `shared_reopened`
/// FIRST (reason `ReopenedBeforeCloseReleased`); R's removal then lands AFTER S
/// re-opened `Shared.d.vue.ts`, leaving it closed in the provider though the graph
/// says `{Shared -> S}` → both the `reason` assertion AND the stranding assertion
/// FAIL. On the serialized owner S's `open_overlay` for Shared finds R's path lock
/// HELD and fires `shared_open_contended` FIRST (reason `BlockedOnPathLock`); S then
/// re-establishes the overlay only after the close completes and drops the lock, so
/// the provider keeps `Shared.d.vue.ts` and the asserted reason is
/// `BlockedOnPathLock` → both assertions PASS. The release rides the forced lock
/// ordering, never a clock — there is no timing race and no fallback that could go
/// green despite the bug. The contention probe is `#[cfg(test)]`-gated on the owner,
/// so production carries no probe.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn guarded_decl_close_does_not_strand_concurrently_reopened_overlay() {
    let temp_base_guard =
        tempfile::TempDir::with_prefix("verter_test_decl_close_supersession").expect("temp dir");
    let temp_base = temp_base_guard.path().to_path_buf();
    let workspace = temp_base.join("workspace");
    std::fs::create_dir_all(workspace.join("src")).expect("create dirs");
    std::fs::write(
        workspace.join("tsconfig.json"),
        r#"{ "compilerOptions": { "baseUrl": "." } }"#,
    )
    .expect("write tsconfig");

    // Shared is a leaf carrier imported by BOTH R and S — the shared declaration
    // overlay `Shared.d.vue.ts` is reached from each of them. R and S also seed
    // their own self-overlays, but the race is on the SHARED one.
    std::fs::write(
        workspace.join("src/Shared.vue"),
        "<script setup lang=\"ts\">\ndefineProps<{ s: string }>()\n</script>\n<template><div/></template>",
    )
    .expect("write Shared.vue");
    let r_src = "<script setup lang=\"ts\">\nimport Shared from './Shared.vue'\ndefineProps<{ r: string }>()\n</script>\n<template><Shared/></template>";
    std::fs::write(workspace.join("src/R.vue"), r_src).expect("write R.vue");
    let s_src = "<script setup lang=\"ts\">\nimport Shared from './Shared.vue'\ndefineProps<{ q: string }>()\n</script>\n<template><Shared/></template>";
    std::fs::write(workspace.join("src/S.vue"), s_src).expect("write S.vue");

    let workspace_id_raw = std::fs::canonicalize(&workspace)
        .expect("canonical workspace")
        .to_string_lossy()
        .replace('\\', "/");
    let workspace_id = workspace_id_raw
        .strip_prefix("//?/")
        .unwrap_or(&workspace_id_raw)
        .to_string();

    let vfs_workspace_access: Arc<dyn verter_workspace::WorkspaceAccess> = Arc::new(
        verter_workspace::FilesystemWorkspace::new(verter_workspace::FilesystemOptions::default()),
    );
    let host = Arc::new(VerterHost::new(HostConfig::default(), vfs_workspace_access));

    let workspace_uri = crate::uri::path_to_file_uri_string(&workspace_id);
    let vite_opts = verter_workspace::ViteConfigOptions::default();
    let registry_ws =
        verter_workspace::FilesystemWorkspace::new(verter_workspace::FilesystemOptions::default());
    let build_result = crate::config::ProjectRegistry::from_workspace_roots(
        &registry_ws,
        &[workspace_uri],
        &vite_opts,
    );
    host.configure_projects(
        build_result
            .registry
            .projects()
            .iter()
            .map(|p| p.to_ide_project_config())
            .collect(),
    );
    let vfs_workspace = make_test_vfs_workspace_from_registry(&build_result.registry)
        .into_inner()
        .expect("the test VFS workspace publishes a snapshot");

    let provider = Arc::new(GatedDeclOverlayProvider::default());
    let type_provider: Arc<dyn TypeProvider> = provider.clone();
    let host_for_server = Arc::clone(&host);
    let type_provider_for_server = Arc::clone(&type_provider);
    let (service, _socket) = tower_lsp_server::LspService::new(move |_client| {
        VerterLanguageServer::new(
            crate::outbound::Outbound::default(),
            LspConfig {
                host: Arc::clone(&host_for_server),
                type_provider: Some(Arc::clone(&type_provider_for_server)),
                project_sync_mode: crate::ProjectSyncMode::FullProject,
                // The proactive declaration-overlay graph is a tsgo-only concern.
                type_provider_kind: crate::TypeProviderKind::Tsgo,
                type_provider_topology: crate::TypeProviderTopology::ManagedTsgo,
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
    server.install_vfs_workspace(vfs_workspace);

    // Open R first (S is opened later, AFTER R's priming pass, so that at R's
    // release time R is the SOLE referencer of Shared.d.vue.ts).
    let r_uri = crate::uri::path_to_file_uri(&format!("{workspace_id}/src/R.vue")).expect("r uri");
    let _ = server.test_documents().did_open(&TextDocumentItem {
        uri: r_uri.clone(),
        language_id: "vue".to_string(),
        version: 1,
        text: r_src.to_string(),
    });
    let r_id = server
        .test_documents()
        .get_canonical_id(&r_uri)
        .expect("R.vue has a canonical id");
    let shared_id = format!("{workspace_id}/src/Shared.vue");
    let shared_decl_path = host
        .declaration_carrier_path(&shared_id)
        .expect("Shared.vue projects a declaration carrier path");
    assert!(
        shared_decl_path.contains("Shared.d.vue.ts"),
        "sanity: Shared's declaration overlay path is Shared.d.vue.ts, got {shared_decl_path}"
    );

    // PRIME with only R open: the closure pass records `{Shared.d.vue.ts -> R}`
    // (R imports Shared) and opens the overlay in the provider.
    server.test_run_declaration_closure_pass(1).await;
    let primed_shared_by_r = server
        .test_decl_overlay_owner()
        .test_slot_roots(&shared_decl_path)
        .is_some_and(|roots| roots.iter().any(|r| r == &r_id));
    assert!(
        primed_shared_by_r,
        "priming pass (R open) records Shared.d.vue.ts -> {{R}}, slots={:?}",
        server.test_decl_overlay_owner().test_slots_snapshot()
    );
    assert!(
        provider.open_paths_snapshot().contains(&shared_decl_path),
        "priming pass opened Shared.d.vue.ts in the provider, open_paths={:?}",
        provider.open_paths_snapshot()
    );

    // Now open S (imports Shared too) — but DO NOT run a pass for S yet. At R's
    // release time the refcount still says `{Shared -> R}` only, so R's release
    // computes Shared as now-unreferenced (the precondition for the stale close).
    let s_uri = crate::uri::path_to_file_uri(&format!("{workspace_id}/src/S.vue")).expect("s uri");
    let _ = server.test_documents().did_open(&TextDocumentItem {
        uri: s_uri.clone(),
        language_id: "vue".to_string(),
        version: 1,
        text: s_src.to_string(),
    });
    let s_id = server
        .test_documents()
        .get_canonical_id(&s_uri)
        .expect("S.vue has a canonical id");

    // Arm the deterministic interleave. R's release close of Shared.d.vue.ts pauses
    // INSIDE the provider close (signals `arrived`, awaits `release`) BEFORE the
    // open-set removal applies — and, in the lifecycle owner, R's guarded close
    // holds Shared's per-path serialization lock across that paused provider close.
    //
    // Two mutually-exclusive owner observations drive a deterministic release with NO
    // timeout — the discrimination rides a FORCED ordering, never a clock:
    //   * `shared_open_contended` fires the instant S's `open_overlay` for
    //     Shared.d.vue.ts finds the path's serialization lock already HELD (by R's
    //     in-flight close) and is about to await it. This is the POST-FIX signal: it
    //     proves S's open genuinely BLOCKED behind R's close instead of racing it.
    //   * `shared_reopened` fires the instant S's RE-OPEN of Shared.d.vue.ts lands its
    //     provider open EFFECT (the overlay is resolvable again). This is the PRE-FIX
    //     signal: WITHOUT serialization S's open is not lock-gated, so it re-opens
    //     Shared while R's close is still paused and signals here FIRST.
    let (arrived, release) = provider.block_close_path(&shared_decl_path);
    let shared_reopened = provider.signal_open_path(&shared_decl_path);
    let shared_open_contended = server
        .test_decl_overlay_owner()
        .signal_open_lock_contended_for_test(&shared_decl_path);

    // H — the REAL did_close handler for R: [DIDCLOSE](R) then [RELEASE](R). The
    // release drops R from Shared's set → Shared now-unreferenced → the owner's
    // guarded close acquires Shared's path lock and issues the gated provider close
    // of Shared.d.vue.ts (which pauses, holding the lock). Borrows `&server`.
    let handler = super::super::lifecycle::handle_did_close(
        server,
        DidCloseTextDocumentParams {
            text_document: TextDocumentIdentifier { uri: r_uri.clone() },
        },
    );

    /// Which of the two FORCED owner observations released R's gated close — the
    /// FORCED discriminator of the per-path serialization. There is no third
    /// (timeout) outcome: exactly one of these fires in each world.
    #[derive(Debug)]
    enum ReleaseReason {
        /// POST-FIX: S's `open_overlay` hit R's HELD path lock (it blocked on the
        /// serialization instead of racing R's close).
        BlockedOnPathLock,
        /// PRE-FIX: S re-opened Shared BEFORE R's close was released (no
        /// serialization — S's open raced R's in-flight close).
        ReopenedBeforeCloseReleased,
    }

    // P — once R's close is gated (a SINGLE `arrived` awaiter, so the one-shot notify
    // is never split between two futures), drive S's reopen and R's release
    // CONCURRENTLY. The releaser waits on the FIRST of the two owner observations and
    // frees R only then — a `select!` over exactly two outcomes, NO sleep:
    //   * post-fix (serialized owner): S's `open_overlay` for Shared finds R's path
    //     lock held → `shared_open_contended` fires → reason = BlockedOnPathLock →
    //     release R → R's close completes + drops the lock → S acquires it,
    //     revalidates S is open, records S, and re-opens Shared → no strand.
    //   * pre-fix (no serialization): S's open is not lock-gated, so it re-opens
    //     Shared while R's close is still paused → `shared_reopened` fires FIRST →
    //     reason = ReopenedBeforeCloseReleased → release R → R's stale close applies
    //     its open-set REMOVAL strictly AFTER S re-opened Shared → Shared stranded.
    // Both the `reason` assertion and the strand assertion below then discriminate
    // deterministically, with no timing dependence.
    let pass_and_release = async {
        arrived.notified().await;
        let pass = server.test_run_declaration_closure_pass(1);
        let releaser = async {
            let reason = tokio::select! {
                _ = shared_open_contended.notified() => ReleaseReason::BlockedOnPathLock,
                _ = shared_reopened.notified() => ReleaseReason::ReopenedBeforeCloseReleased,
            };
            release.notify_one();
            reason
        };
        let (synced, reason) = tokio::join!(pass, releaser);
        (synced, reason)
    };

    let (_, (_synced, reason)) = tokio::join!(handler, pass_and_release);

    // The handler ran to completion: R is closed in the registry.
    assert!(
        !server
            .test_documents()
            .open_uris()
            .iter()
            .any(|u| u == r_uri.as_str()),
        "the real did_close handler closed R"
    );

    // FORCED DISCRIMINATOR (no timeout): post-fix, S's open must have BLOCKED behind
    // R's held path lock — so the release reason is `BlockedOnPathLock`. Pre-fix (no
    // serialization) S re-opens Shared while R's close is paused, so `shared_reopened`
    // fires first and the reason is `ReopenedBeforeCloseReleased` → this FAILS
    // deterministically (the discrimination rides the forced lock ordering, never a
    // clock).
    assert!(
        matches!(reason, ReleaseReason::BlockedOnPathLock),
        "the serialized owner must make S's reopen BLOCK on R's held path lock (release \
         reason = BlockedOnPathLock); a non-serialized tree lets S re-open Shared before \
         R is released (reason = ReopenedBeforeCloseReleased). Got {reason:?}, calls={:?}",
        provider.calls()
    );

    // INVARIANT 1 (the fix): the owner records S still reaches Shared.d.vue.ts.
    let shared_reached_by_s = server
        .test_decl_overlay_owner()
        .test_slot_roots(&shared_decl_path)
        .is_some_and(|roots| roots.iter().any(|r| r == &s_id));
    assert!(
        shared_reached_by_s,
        "after the race the owner records S as reaching Shared.d.vue.ts, slots={:?}",
        server.test_decl_overlay_owner().test_slots_snapshot()
    );

    // INVARIANT 2 (the fix — the STRANDING assertion): the provider STILL has
    // Shared.d.vue.ts open, so S's bare `import Shared from "./Shared.vue"`
    // resolves (no TS2307). RED-before: R's stale close removed it from the
    // provider open-set AFTER S re-opened it → open_paths is missing Shared (it
    // typically contains only S's self-overlay S.d.vue.ts) → this FAILS, exactly
    // the stranding the refcount cannot see.
    let open_paths = provider.open_paths_snapshot();
    assert!(
        open_paths.contains(&shared_decl_path),
        "the provider must STILL have Shared.d.vue.ts open after the race (the \
         refcount says S reaches it) — a stale close that strands S on TS2307 is \
         the bug; provider open_paths={open_paths:?}, calls={:?}",
        provider.calls()
    );

    drain.abort();
    let _ = std::fs::remove_dir_all(&temp_base);
}

/// CLOSE-SIDE RECOVERABILITY: a close-side orphan — a drained overlay whose
/// provider `close_dts` did NOT complete (the close failed, or the close future
/// was interrupted before its await landed) — leaves the provider overlay STILL
/// OPEN with the slot reduced to an empty tombstone and NO live root reaching it.
/// A later sweep MUST re-examine that tombstone and re-issue the close, so the
/// orphaned provider overlay is eventually closed. The owner GCs a slot ONLY on a
/// CONFIRMED close, so a SURVIVING tombstone is exactly the "close not confirmed"
/// signal a later `reconcile_open_roots` must act on.
///
/// DISCRIMINATING: `reconcile_open_roots` (the post-pass sweep) must RE-RETURN a
/// surviving tombstone whose provider close never confirmed. RED-before: the sweep
/// only returned a slot it drained from non-empty to empty, so an already-empty
/// tombstone was permanently dropped — the orphan had no future closer (the
/// provider overlay leaked open forever). GREEN-after: the sweep re-returns the
/// unconfirmed tombstone, and the re-issued close finally closes the provider
/// overlay and GCs the slot.
#[tokio::test(flavor = "multi_thread")]
async fn unconfirmed_close_tombstone_is_recovered_by_a_later_reconcile() {
    let decl_path = "/ws/Dep.d.vue.ts";

    let provider = Arc::new(MockTypeProvider::new());
    let sync = ProjectSync::new(provider.clone(), ProjectSyncMode::FullProject);
    let states: DashMap<String, crate::provider_sync::ProviderSyncState> = DashMap::new();
    // The carrier owner state that carries the live Decl overlay (so guarded_close
    // has a committed provider state to strip + a path to close).
    states.insert(
        "/ws/Dep.vue".to_string(),
        crate::provider_sync::ProviderSyncState {
            owner_binding: crate::provider_sync::ProviderOwnerBinding::Owned(
                "/ws/tsconfig.json".into(),
            ),
            decl_path: Some(decl_path.to_string()),
            decl_background_loaded: true,
            ..Default::default()
        },
    );
    let owner = DeclOverlayOwner::default();

    // The overlay is reached by a single root A at generation 1.
    owner.test_seed_slot(decl_path, &["/ws/A.vue"], 1);

    // A closes → the overlay drains to an empty tombstone; the close is decided at
    // generation 1.
    let decision = owner.release_root("/ws/A.vue");
    assert_eq!(decision.len(), 1, "releasing A drains exactly one overlay");
    assert_eq!(
        decision[0].decl_path, decl_path,
        "releasing A drains the overlay it solely reached"
    );
    assert_eq!(
        decision[0].close_generation, 1,
        "the close is decided at the slot's generation 1"
    );

    // The provider close FAILS this time (a crashed/slow provider, or an interrupted
    // close). guarded_close keeps the tombstone (fail closed) and does NOT GC it.
    provider.set_fail_file_ops(true);
    owner.guarded_close(&sync, &states, &decision).await;
    provider.set_fail_file_ops(false);

    let close_attempts_after_fail = provider
        .file_sync_calls()
        .iter()
        .filter(|c| matches!(c, MockCall::CloseFile { path } if path == decl_path))
        .count();
    assert_eq!(
        close_attempts_after_fail,
        1,
        "the first close was attempted (and failed), calls={:?}",
        provider.file_sync_calls()
    );
    // The tombstone SURVIVES (the failed close did not GC it) — this is the
    // "close not confirmed" signal the sweep must recover from.
    assert_eq!(
        owner.test_slot_roots(decl_path),
        Some(HashSet::new()),
        "a failed close keeps the empty tombstone (fail closed, no GC)"
    );

    // A later sweep against the live open-root set (A is closed ⇒ empty) MUST
    // re-return the unconfirmed tombstone so its orphaned provider overlay is closed.
    // RED-before: the sweep skipped already-empty tombstones, so this is empty and
    // the orphan leaks forever.
    let resweep = owner.reconcile_open_roots(&HashSet::new(), 1);
    assert!(
        resweep.iter().any(|t| t.decl_path == decl_path),
        "a later reconcile must RE-RETURN the unconfirmed-close tombstone (provider \
         overlay still open, no live root reaches it) so it gets a future closer; \
         got: {resweep:?}"
    );

    // The re-issued close now succeeds → the provider overlay is closed and the slot
    // is GC'd (the orphan is recovered).
    owner.guarded_close(&sync, &states, &resweep).await;
    let close_attempts_total = provider
        .file_sync_calls()
        .iter()
        .filter(|c| matches!(c, MockCall::CloseFile { path } if path == decl_path))
        .count();
    assert_eq!(
        close_attempts_total,
        2,
        "the recovered close was re-issued (second attempt), calls={:?}",
        provider.file_sync_calls()
    );
    assert_eq!(
        owner.test_slot_roots(decl_path),
        None,
        "the confirmed re-close GCs the recovered tombstone"
    );

    // NEGATIVE CONTROL: with the orphan recovered (slot GC'd), a further sweep must
    // NOT manufacture a phantom close target — the recovery is bounded to genuine
    // unconfirmed tombstones, not an unconditional re-close of every path.
    let after_recovery = owner.reconcile_open_roots(&HashSet::new(), 1);
    assert!(
        after_recovery.is_empty(),
        "once the orphan is recovered (slot gone) the sweep returns nothing, got: {after_recovery:?}"
    );
}

/// CLOSE-SIDE RECOVERY IS IN-FLIGHT-AWARE: a sweep must NOT re-issue a close for a
/// tombstone whose close is CURRENTLY in-flight. The guarded close marks the slot
/// `close_pending` and holds the overlay's path lock across its provider await; a
/// redundant close issued by a racing `reconcile_open_roots` would only block on
/// that held lock (and, under a paused/gated close, DEADLOCK the very task that
/// would release it). So the recovery is gated on "close NOT in-flight": only a
/// tombstone whose close already finished UNCONFIRMED is re-returned.
///
/// DISCRIMINATING: with the provider close GATED (paused inside `close_dts`, the
/// path lock held and `close_pending` set), `reconcile_open_roots` against an empty
/// live set must return NOTHING for the in-flight overlay. RED-before (a sweep that
/// re-returns every empty tombstone regardless of the in-flight mark): it re-returns
/// the in-flight tombstone; releasing the gate then lets the in-flight close GC the
/// slot and the sweep's redundant target double-closes / contends. GREEN-after: the
/// in-flight tombstone is skipped, and once the gated close completes it GCs the
/// slot, leaving nothing for a later sweep.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn reconcile_does_not_reissue_close_for_an_in_flight_tombstone() {
    let decl_path = "/ws/Dep.d.vue.ts";

    let provider = Arc::new(GatedDeclOverlayProvider::default());
    // Make the overlay resolvable so there is an open path for the close to act on.
    provider
        .open_paths
        .lock()
        .unwrap()
        .insert(decl_path.to_string());
    let sync = ProjectSync::new(provider.clone(), ProjectSyncMode::FullProject);
    let states: DashMap<String, crate::provider_sync::ProviderSyncState> = DashMap::new();
    states.insert(
        "/ws/Dep.vue".to_string(),
        crate::provider_sync::ProviderSyncState {
            owner_binding: crate::provider_sync::ProviderOwnerBinding::Owned(
                "/ws/tsconfig.json".into(),
            ),
            decl_path: Some(decl_path.to_string()),
            decl_background_loaded: true,
            ..Default::default()
        },
    );
    let owner = Arc::new(DeclOverlayOwner::default());

    // The overlay drains to an empty tombstone; the close is decided at generation 1.
    owner.test_seed_slot(decl_path, &["/ws/A.vue"], 1);
    let decision = owner.release_root("/ws/A.vue");
    assert_eq!(decision.len(), 1);
    assert_eq!(decision[0].decl_path, decl_path);
    assert_eq!(decision[0].close_generation, 1);

    // Gate the provider close so it pauses INSIDE `close_dts` — at which point the
    // guarded close holds the path lock AND has marked the slot `close_pending`.
    let (arrived, release) = provider.block_close_path(decl_path);

    let owner_for_close = Arc::clone(&owner);
    let states_ref = &states;
    let close = async {
        owner_for_close
            .guarded_close(&sync, states_ref, &decision)
            .await;
    };

    let probe = async {
        // Wait until the close is paused inside the provider (path lock held,
        // close_pending set), then sweep with an empty live set.
        arrived.notified().await;
        let swept = owner.reconcile_open_roots(&HashSet::new(), 1);
        // INVARIANT: the in-flight tombstone is NOT re-returned.
        assert!(
            swept.iter().all(|t| t.decl_path != decl_path),
            "a sweep must NOT re-issue a close for a tombstone whose close is in-flight \
             (close_pending) — re-issuing contends on the held path lock; got: {swept:?}"
        );
        // Release the gated close so it completes and GCs the slot.
        release.notify_one();
    };

    tokio::join!(close, probe);

    // The in-flight close completed and GC'd the slot; a redundant close was never
    // issued (exactly one provider close for the overlay).
    let closes = provider
        .calls()
        .iter()
        .filter(|c| matches!(c, MockCall::CloseFile { path } if path == decl_path))
        .count();
    assert_eq!(
        closes,
        1,
        "exactly one provider close was issued for the in-flight overlay (no redundant \
         re-close), calls={:?}",
        provider.calls()
    );
    assert_eq!(
        owner.test_slot_roots(decl_path),
        None,
        "the completed in-flight close GC'd the slot"
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn resync_aliased_imports_retains_prior_path_when_replacement_sync_fails() {
    // FIX-3: the aliased-import resync pass must use close-AFTER-successful-sync
    // (skip-active, per-kind), NOT close-before-sync. An open imported `.vue`
    // undergoing an owner-key change has its owner-INDEPENDENT `{src}.vue.ts`
    // marked stale by the force-rebind clause; pre-fix the pass closed it BEFORE
    // syncing, so a failed sync left the artifact gone. Post-fix: a failed sync
    // closes nothing and retains the prior state.
    let temp_base_guard =
        tempfile::TempDir::with_prefix("verter_test_resync_aliased_retain").expect("temp dir");
    let temp_base = temp_base_guard.path().to_path_buf();
    let workspace = temp_base.join("workspace");
    std::fs::create_dir_all(workspace.join("src/components")).expect("create dirs");
    std::fs::write(
        workspace.join("tsconfig.json"),
        r#"{ "compilerOptions": { "baseUrl": ".", "paths": { "@/*": ["src/*"] } } }"#,
    )
    .expect("write tsconfig");
    let child_source = r#"<script setup lang="ts">
defineProps<{ msg: string }>()
</script>
<template><div>{{ msg }}</div></template>"#;
    std::fs::write(workspace.join("src/components/Child.vue"), child_source)
        .expect("write Child.vue");

    let workspace_id_raw = std::fs::canonicalize(&workspace)
        .expect("canonical workspace")
        .to_string_lossy()
        .replace('\\', "/");
    let workspace_id_stripped = workspace_id_raw
        .strip_prefix("//?/")
        .unwrap_or(&workspace_id_raw)
        .to_string();
    // Normalize through the production `CanonicalPath` (lowercases the Windows
    // drive letter) so the seeded state key matches the import-resolver-derived
    // `import_id` the aliased pass uses — otherwise the transition sees no prior
    // state and the close-before-sync regression is not exercised.
    let workspace_id = verter_workspace::CanonicalPath::new(&workspace_id_stripped)
        .as_str()
        .to_string();
    let app_id = format!("{workspace_id}/src/App.vue");
    let child_id = format!("{workspace_id}/src/components/Child.vue");
    let child_api_path = format!("{child_id}.verter.ts");

    let app_source = r#"<script setup lang="ts">
import Child from '@/components/Child.vue'
</script>
<template><Child msg="hello" /></template>"#
        .to_string();

    let vfs_access: Arc<dyn verter_workspace::WorkspaceAccess> = Arc::new(
        verter_workspace::FilesystemWorkspace::new(verter_workspace::FilesystemOptions::default()),
    );
    let host = Arc::new(VerterHost::new(HostConfig::default(), vfs_access));
    let documents = DocumentRegistry::new(Arc::clone(&host));
    let uri = crate::uri::path_to_file_uri(&app_id).expect("file uri");
    let _ = documents.did_open(&TextDocumentItem {
        uri: uri.clone(),
        language_id: "vue".to_string(),
        version: 1,
        text: app_source,
    });

    let workspace_uri = crate::uri::path_to_file_uri_string(&workspace_id);
    let vite_opts = verter_workspace::ViteConfigOptions::default();
    let registry_ws =
        verter_workspace::FilesystemWorkspace::new(verter_workspace::FilesystemOptions::default());
    let build_result = crate::config::ProjectRegistry::from_workspace_roots(
        &registry_ws,
        &[workspace_uri],
        &vite_opts,
    );
    let registry = build_result.registry;
    host.configure_projects(
        registry
            .projects()
            .iter()
            .map(|p| p.to_ide_project_config())
            .collect(),
    );
    let vfs_workspace = make_test_vfs_workspace_from_registry(&registry);

    let provider = Arc::new(MockTypeProvider::new());
    // Fail ONLY the child's API sync; everything else succeeds.
    provider.set_fail_sync_path(&child_api_path);
    let sync = ProjectSync::new(provider.clone(), ProjectSyncMode::FullProject);

    let provider_sync_states = DashMap::new();
    // Prior committed state from a STALE owner key: same owner-independent
    // `{child}.vue.ts` API path. The owner change marks that same path stale
    // (the force-rebind clause). Pre-fix the pass closed it BEFORE the sync;
    // the sync then FAILS, leaving the artifact gone. `api_background_loaded`
    // is false so the aliased pass actually processes the file (it skips files
    // already fully background-loaded).
    let prior_state = ProviderSyncState {
        owner_binding: crate::provider_sync::ProviderOwnerBinding::Owned(
            "/stale/tsconfig.json".to_string(),
        ),
        ide_path: None,
        api_path: Some(child_api_path.clone()),
        decl_path: None,
        api_background_loaded: false,
        decl_background_loaded: false,
        ide_background_loaded: false,
        shadow_path: None,
        shadow_background_loaded: false,
        committed_ide_surface: None,
        commit_stamp: None,
        api_delivered_hash: None,
        api_observed_hash: None,
        shadow_delivered_source_hash: None,
    };
    provider_sync_states.insert(child_id.clone(), prior_state.clone());

    resync_aliased_imports_for_open_files(
        &documents,
        Some(&sync),
        &vfs_workspace,
        &provider_sync_states,
        false,
        None,
        &DeclOverlayOwner::default(),
        1,
        &crate::external_ts::CarrierTransactionCoordinator::new(),
        &dashmap::DashSet::new(),
    )
    .await;

    let calls = provider.file_sync_calls();
    // R2-7 reach assertion (DISCRIMINATES a no-op discovery regression): the
    // aliased pass must actually REACH the child and ATTEMPT to sync its
    // `{child}.vue.ts` despite the injected failure (`set_fail_sync_path` records
    // the open/update call BEFORE returning Err). If the alias-collection
    // pipeline regressed to a no-op, no such attempt is recorded and the
    // absence-of-close + state-survival asserts below would PASS vacuously.
    assert!(
        calls.iter().any(|call| matches!(
            call,
            MockCall::OpenFile { path, .. }
                | MockCall::UpdateFile { path, .. }
                | MockCall::LoadFile { path, .. }
            if path == &child_api_path
        )),
        "aliased resync must REACH the child and attempt its `.vue.ts` sync, calls={calls:?}"
    );
    // Discriminator: the prior live `{child}.vue.ts` must NOT be closed, because
    // its replacement sync FAILED. (Pre-fix: closed before the sync attempt.)
    assert!(
        !calls.iter().any(|call| matches!(
            call,
            MockCall::CloseFile { path } if path == &child_api_path
        )),
        "failed aliased resync must NOT close the prior live API path, calls={calls:?}"
    );
    // Positive: the prior state is retained unchanged on a fully-failed sync.
    let state = provider_sync_states
        .get(&child_id)
        .map(|entry| entry.clone())
        .expect("failed aliased resync must retain the prior state");
    assert_eq!(
        state, prior_state,
        "failed aliased resync must leave the prior state unchanged, got {state:?}"
    );

    let _ = std::fs::remove_dir_all(&temp_base);
}

#[tokio::test(flavor = "multi_thread")]
async fn resync_aliased_imports_syncs_vue_ide_artifact_for_tsgo() {
    let temp_base_guard =
        tempfile::TempDir::with_prefix("verter_test_resync_aliased_tsgo").expect("temp dir");
    let temp_base = temp_base_guard.path().to_path_buf();
    let workspace = temp_base.join("workspace");
    std::fs::create_dir_all(workspace.join("src/components")).expect("create dirs");

    std::fs::write(
        workspace.join("tsconfig.json"),
        r#"{
  "compilerOptions": {
    "baseUrl": ".",
    "paths": {
      "@/*": ["src/*"]
    }
  }
}"#,
    )
    .expect("write tsconfig");

    std::fs::write(
        workspace.join("src/components/Child.vue"),
        r#"<script setup lang="ts">
defineProps<{ msg: string }>()
</script>
<template><div>{{ msg }}</div></template>"#,
    )
    .expect("write child");

    let workspace_id = crate::test_utils::canonical_test_path(&workspace);
    let app_id = format!("{workspace_id}/src/App.vue");

    let host = crate::test_utils::make_filesystem_test_host(&workspace);
    let documents = DocumentRegistry::new(Arc::clone(&host));
    let uri = crate::uri::path_to_file_uri(&app_id).expect("file uri");
    let _ = documents.did_open(&TextDocumentItem {
        uri,
        language_id: "vue".to_string(),
        version: 1,
        text: r#"<script setup lang="ts">
import Child from '@/components/Child.vue'
</script>
<template><Child msg="hello" /></template>"#
            .to_string(),
    });

    let vite_opts = verter_workspace::ViteConfigOptions::default();
    let workspace_uri = crate::uri::path_to_file_uri_string(&workspace_id);
    let registry_ws =
        verter_workspace::FilesystemWorkspace::new(verter_workspace::FilesystemOptions::default());
    let build_result = crate::config::ProjectRegistry::from_workspace_roots(
        &registry_ws,
        &[workspace_uri],
        &vite_opts,
    );
    let registry = build_result.registry;
    let vfs_workspace = make_test_vfs_workspace_from_registry(&registry);
    host.configure_projects(
        registry
            .projects()
            .iter()
            .map(|p| p.to_ide_project_config())
            .collect(),
    );

    let provider = Arc::new(MockTypeProvider::new());
    let sync = ProjectSync::new(provider.clone(), ProjectSyncMode::FullProject);
    let provider_sync_states = DashMap::new();

    resync_aliased_imports_for_open_files(
        &documents,
        Some(&sync),
        &vfs_workspace,
        &provider_sync_states,
        true,
        None,
        &DeclOverlayOwner::default(),
        1,
        &crate::external_ts::CarrierTransactionCoordinator::new(),
        &dashmap::DashSet::new(),
    )
    .await;

    let calls = provider.file_sync_calls();
    assert!(
        calls.iter().any(|call| matches!(
            call,
            MockCall::OpenFile { path, .. } if path.ends_with("Child.vue.tsx")
        )),
        "TSGO alias resync should open the Vue IDE artifact, calls={calls:?}"
    );

    let _ = std::fs::remove_dir_all(&temp_base);
}

#[tokio::test(flavor = "multi_thread")]
async fn resync_aliased_imports_syncs_barrel_and_vue_deps_for_tsgo() {
    // Setup: App.vue imports `{ Overlay }` from a barrel (./components/index.ts)
    // which re-exports `./Overlay.vue`. Both the barrel and its Vue dependency
    // must be synced eagerly so TSGO resolves the component types.
    let temp_base_guard =
        tempfile::TempDir::with_prefix("verter_test_resync_barrel_tsgo").expect("temp dir");
    let temp_base = temp_base_guard.path().to_path_buf();
    let workspace = temp_base.join("workspace");
    std::fs::create_dir_all(workspace.join("src/components")).expect("create dirs");

    std::fs::write(
        workspace.join("tsconfig.json"),
        r#"{
  "compilerOptions": {
    "baseUrl": ".",
    "paths": {
      "@/*": ["src/*"]
    }
  }
}"#,
    )
    .expect("write tsconfig");

    // Barrel file re-exports Overlay from its Vue component
    std::fs::write(
        workspace.join("src/components/index.ts"),
        r#"export { default as Overlay } from './Overlay.vue'"#,
    )
    .expect("write barrel");

    // Vue component behind the barrel
    std::fs::write(
        workspace.join("src/components/Overlay.vue"),
        r#"<script setup lang="ts">
defineProps<{ show: boolean }>()
</script>
<template><div v-if="show">overlay</div></template>"#,
    )
    .expect("write Overlay.vue");

    let workspace_id_raw = std::fs::canonicalize(&workspace)
        .expect("canonical workspace")
        .to_string_lossy()
        .replace('\\', "/");
    let workspace_id = workspace_id_raw
        .strip_prefix("//?/")
        .unwrap_or(&workspace_id_raw)
        .to_string();
    let app_id = format!("{workspace_id}/src/App.vue");

    let vfs_workspace: Arc<dyn verter_workspace::WorkspaceAccess> = Arc::new(
        verter_workspace::FilesystemWorkspace::new(verter_workspace::FilesystemOptions::default()),
    );
    let host = Arc::new(VerterHost::new(HostConfig::default(), vfs_workspace));
    let documents = DocumentRegistry::new(Arc::clone(&host));
    let uri = crate::uri::path_to_file_uri(&app_id).expect("file uri");
    let _ = documents.did_open(&TextDocumentItem {
        uri,
        language_id: "vue".to_string(),
        version: 1,
        text: r#"<script setup lang="ts">
import { Overlay } from './components'
</script>
<template><Overlay :show="true" /></template>"#
            .to_string(),
    });

    let vite_opts = verter_workspace::ViteConfigOptions::default();
    let workspace_uri = crate::uri::path_to_file_uri_string(&workspace_id);
    let registry_ws =
        verter_workspace::FilesystemWorkspace::new(verter_workspace::FilesystemOptions::default());
    let build_result = crate::config::ProjectRegistry::from_workspace_roots(
        &registry_ws,
        &[workspace_uri],
        &vite_opts,
    );
    let registry = build_result.registry;
    let vfs_workspace = make_test_vfs_workspace_from_registry(&registry);
    host.configure_projects(
        registry
            .projects()
            .iter()
            .map(|p| p.to_ide_project_config())
            .collect(),
    );

    let provider = Arc::new(MockTypeProvider::new());
    let sync = ProjectSync::new(provider.clone(), ProjectSyncMode::FullProject);
    let provider_sync_states = DashMap::new();

    resync_aliased_imports_for_open_files(
        &documents,
        Some(&sync),
        &vfs_workspace,
        &provider_sync_states,
        true,
        None,
        &DeclOverlayOwner::default(),
        1,
        &crate::external_ts::CarrierTransactionCoordinator::new(),
        &dashmap::DashSet::new(),
    )
    .await;

    let calls = provider.file_sync_calls();

    // Positive: Vue dependency Overlay.vue should be synced (IDE + API artifacts)
    assert!(
        calls.iter().any(|call| matches!(
            call,
            MockCall::OpenFile { path, .. } if path.contains("Overlay.vue")
        )),
        "Vue dependency Overlay.vue should be synced, calls={calls:?}"
    );

    // Positive: Barrel file should be synced to provider (via sync_file → update_file)
    // Note: carrier import specifiers are already resolvable before reaching the
    // provider — the compiler emits `.vue.tsx` for in-project carrier imports and
    // the resolver emits `.verter.ts` for non-carrier importer specifiers — so the
    // provider sends content unmodified (the mock records that raw content).
    assert!(
        calls.iter().any(|call| matches!(
            call,
            MockCall::OpenFile { path, .. }
                | MockCall::LoadFile { path, .. }
                | MockCall::UpdateFile { path, .. }
                if path.contains("components/index")
        )),
        "Barrel file index.ts should be synced to provider, calls={calls:?}"
    );

    // Negative: non-barrel utility imports should NOT trigger barrel sync
    assert!(
        !calls.iter().any(|call| matches!(
            call,
            MockCall::OpenFile { path, .. }
                | MockCall::LoadFile { path, .. }
                | MockCall::UpdateFile { path, .. }
                if path.contains("utils")
        )),
        "Utility files should not be synced through barrel path, calls={calls:?}"
    );

    let _ = std::fs::remove_dir_all(&temp_base);
}

#[tokio::test(flavor = "multi_thread")]
async fn resync_aliased_already_loaded_open_vue_reconciled_when_owner_lost() {
    // R2-4: the aliased-import resync skips a fully-background-loaded import
    // BEFORE resolving its current owner. A stale-`Owned` OPEN `.vue` whose owner
    // disappeared (snapshot now resolves None) must still be RECONCILED — pre-fix
    // the `already_loaded` skip short-circuited it, leaving it stranded on the
    // dead owner (the `no ide_context` class). Post-fix: the skip is gated on the
    // committed binding still matching the live resolution, so an owner-lost open
    // file falls through to `reconcile_unowned_carrier_provider_file` → converts to
    // Unresolved + closes the dropped owner-derived `.vue.ts`.
    let temp_base_guard =
        tempfile::TempDir::with_prefix("verter_test_r24_aliased_owner_lost").expect("temp dir");
    let temp_base = temp_base_guard.path().to_path_buf();
    let workspace = temp_base.join("workspace");
    std::fs::create_dir_all(workspace.join("src/components")).expect("create dirs");
    std::fs::write(
        workspace.join("tsconfig.json"),
        r#"{ "compilerOptions": { "baseUrl": ".", "paths": { "@/*": ["src/*"] } } }"#,
    )
    .expect("write tsconfig");
    std::fs::write(
        workspace.join("src/components/Child.vue"),
        r#"<script setup lang="ts">
defineProps<{ msg: string }>()
</script>
<template><div>{{ msg }}</div></template>"#,
    )
    .expect("write Child.vue");

    let workspace_id_raw = std::fs::canonicalize(&workspace)
        .expect("canonical workspace")
        .to_string_lossy()
        .replace('\\', "/");
    let workspace_id_stripped = workspace_id_raw
        .strip_prefix("//?/")
        .unwrap_or(&workspace_id_raw)
        .to_string();
    let workspace_id = verter_workspace::CanonicalPath::new(&workspace_id_stripped)
        .as_str()
        .to_string();
    let app_id = format!("{workspace_id}/src/App.vue");
    let child_id = format!("{workspace_id}/src/components/Child.vue");
    let child_tsx = format!("{child_id}.tsx");
    let child_api_path = format!("{child_id}.verter.ts");

    let vfs_access: Arc<dyn verter_workspace::WorkspaceAccess> = Arc::new(
        verter_workspace::FilesystemWorkspace::new(verter_workspace::FilesystemOptions::default()),
    );
    let host = Arc::new(VerterHost::new(HostConfig::default(), vfs_access));
    let documents = DocumentRegistry::new(Arc::clone(&host));
    let app_uri = crate::uri::path_to_file_uri(&app_id).expect("file uri");
    let _ = documents.did_open(&TextDocumentItem {
        uri: app_uri,
        language_id: "vue".to_string(),
        version: 1,
        text: r#"<script setup lang="ts">
import Child from '@/components/Child.vue'
</script>
<template><Child msg="hello" /></template>"#
            .to_string(),
    });
    // Open Child.vue: it is the already-loaded OPEN import whose owner was lost.
    let child_uri = crate::uri::path_to_file_uri(&child_id).expect("child uri");
    let _ = documents.did_open(&TextDocumentItem {
        uri: child_uri,
        language_id: "vue".to_string(),
        version: 1,
        text: r#"<script setup lang="ts">
defineProps<{ msg: string }>()
</script>
<template><div>{{ msg }}</div></template>"#
            .to_string(),
    });

    // Host registry owns the workspace (so the aliased import resolves Child).
    let workspace_uri = crate::uri::path_to_file_uri_string(&workspace_id);
    let vite_opts = verter_workspace::ViteConfigOptions::default();
    let registry_ws =
        verter_workspace::FilesystemWorkspace::new(verter_workspace::FilesystemOptions::default());
    let build_result = crate::config::ProjectRegistry::from_workspace_roots(
        &registry_ws,
        &[workspace_uri],
        &vite_opts,
    );
    host.configure_projects(
        build_result
            .registry
            .projects()
            .iter()
            .map(|p| p.to_ide_project_config())
            .collect(),
    );
    // Ready snapshot rooted at `/other` — does NOT own the workspace's Child.vue,
    // so its current owner resolves to None (the owner-loss arm).
    let vfs_workspace = crate::test_utils::make_test_vfs_workspace_with_resolver(
        "/other",
        Some("/other/tsconfig.json"),
    );

    let provider = Arc::new(MockTypeProvider::new());
    let sync = ProjectSync::new(provider.clone(), ProjectSyncMode::FullProject);

    let provider_sync_states = DashMap::new();
    // Prior state: STALE `Owned` binding, BOTH kinds background-loaded so the
    // `already_loaded` skip fires pre-fix. Carries the owner-derived `.vue.ts`.
    let prior_state = ProviderSyncState {
        owner_binding: crate::provider_sync::ProviderOwnerBinding::Owned(
            "/stale/tsconfig.json".to_string(),
        ),
        ide_path: Some(child_tsx.clone()),
        api_path: Some(child_api_path.clone()),
        decl_path: None,
        ide_background_loaded: true,
        api_background_loaded: true,
        decl_background_loaded: false,
        shadow_path: None,
        shadow_background_loaded: false,
        committed_ide_surface: None,
        commit_stamp: None,
        api_delivered_hash: None,
        api_observed_hash: None,
        shadow_delivered_source_hash: None,
    };
    provider_sync_states.insert(child_id.clone(), prior_state);

    // Non-tsgo aliased pass: the skip checks `api_background_loaded` (true here).
    resync_aliased_imports_for_open_files(
        &documents,
        Some(&sync),
        &vfs_workspace,
        &provider_sync_states,
        false,
        None,
        &DeclOverlayOwner::default(),
        1,
        &crate::external_ts::CarrierTransactionCoordinator::new(),
        &dashmap::DashSet::new(),
    )
    .await;

    // Discriminator (RED pre-fix): the already-loaded open Child was skipped, so
    // its stale `Owned` binding survived. Post-fix it is reconciled to Unresolved.
    let state = provider_sync_states
        .get(&child_id)
        .map(|entry| entry.clone())
        .expect("open Child.vue must keep a provider sync state");
    assert!(
        state.is_unresolved(),
        "an already-loaded open `.vue` whose owner was lost must be reconciled to \
         Unresolved (not skipped on a stale Owned binding), got {:?}",
        state.owner_binding
    );
    // The owner-derived `.vue.ts` dropped by the conversion must be closed.
    let calls = provider.file_sync_calls();
    assert!(
        calls.iter().any(|call| matches!(
            call,
            MockCall::CloseFile { path } if path == &child_api_path
        )),
        "owner-loss reconciliation must close the dropped `.vue.ts`, calls={calls:?}"
    );
    // The live IDE TSX must NOT be closed (editor-liveness for the open file).
    assert!(
        !calls.iter().any(|call| matches!(
            call,
            MockCall::CloseFile { path } if path == &child_tsx
        )),
        "owner-loss reconciliation must NOT close the open file's live TSX, calls={calls:?}"
    );

    let _ = std::fs::remove_dir_all(&temp_base);
}

#[tokio::test(flavor = "multi_thread")]
async fn resync_barrel_vue_dep_reconciles_open_owned_overlay_on_owner_loss() {
    // FIX-4 + R2-7: an OPEN `.vue` reached as a barrel dependency whose owner
    // resolves to None must NOT have its provider state removed nor its IDE TSX
    // closed (the barrel pass previously called
    // remove_provider_sync_state_and_close_paths unconditionally for an
    // owner-None `.vue`). It must be reconciled in place: a prior `Owned` binding
    // is converted to `Unresolved` (R2-4 barrel-pass gate + the open-document
    // editor-liveness path) and the dropped owner-derived `.vue.ts` is closed
    // (R2-8), while the live IDE TSX is preserved.
    //
    // The binding flip Owned→Unresolved + the `.vue.ts` close are the R2-7
    // DISCRIMINATING reach signals: they only happen if the barrel→Vue-re-export
    // discovery actually REACHED Overlay. If discovery regressed to a no-op, the
    // seeded `Owned` binding would survive unchanged and nothing would close, so
    // a pure state-survival + absence-of-close assertion would pass vacuously.
    let temp_base_guard =
        tempfile::TempDir::with_prefix("verter_test_barrel_open_unowned").expect("temp dir");
    let temp_base = temp_base_guard.path().to_path_buf();
    let workspace = temp_base.join("workspace");
    std::fs::create_dir_all(workspace.join("src/components")).expect("create dirs");
    std::fs::write(
        workspace.join("tsconfig.json"),
        r#"{ "compilerOptions": { "baseUrl": ".", "paths": { "@/*": ["src/*"] } } }"#,
    )
    .expect("write tsconfig");
    std::fs::write(
        workspace.join("src/components/index.ts"),
        r#"export { default as Overlay } from './Overlay.vue'"#,
    )
    .expect("write barrel");
    std::fs::write(
        workspace.join("src/components/Overlay.vue"),
        r#"<script setup lang="ts">
defineProps<{ show: boolean }>()
</script>
<template><div v-if="show">overlay</div></template>"#,
    )
    .expect("write Overlay.vue");

    let workspace_id_raw = std::fs::canonicalize(&workspace)
        .expect("canonical workspace")
        .to_string_lossy()
        .replace('\\', "/");
    let workspace_id_stripped = workspace_id_raw
        .strip_prefix("//?/")
        .unwrap_or(&workspace_id_raw)
        .to_string();
    // Verter's canonical-path normalization lowercases the Windows drive letter
    // (the import resolver emits `c:/...`), whereas `std::fs::canonicalize`
    // uppercases it (`C:/...`). Run it through the production `CanonicalPath`
    // normalizer so the open-document key, the seeded state key, and the
    // drain-discovered barrel-dep id all agree — otherwise the owner-None arm
    // runs against a different key.
    let workspace_id = verter_workspace::CanonicalPath::new(&workspace_id_stripped)
        .as_str()
        .to_string();
    let app_id = format!("{workspace_id}/src/App.vue");
    let overlay_id = format!("{workspace_id}/src/components/Overlay.vue");
    let overlay_tsx = format!("{overlay_id}.tsx");
    let overlay_api = format!("{overlay_id}.verter.ts");

    let vfs_access: Arc<dyn verter_workspace::WorkspaceAccess> = Arc::new(
        verter_workspace::FilesystemWorkspace::new(verter_workspace::FilesystemOptions::default()),
    );
    let host = Arc::new(VerterHost::new(HostConfig::default(), vfs_access));
    let documents = DocumentRegistry::new(Arc::clone(&host));
    let app_uri = crate::uri::path_to_file_uri(&app_id).expect("file uri");
    let _ = documents.did_open(&TextDocumentItem {
        uri: app_uri,
        language_id: "vue".to_string(),
        version: 1,
        text: r#"<script setup lang="ts">
import { Overlay } from './components'
</script>
<template><Overlay :show="true" /></template>"#
            .to_string(),
    });
    // Open Overlay.vue too: it is the OPEN barrel-dep whose state must survive.
    let overlay_uri = crate::uri::path_to_file_uri(&overlay_id).expect("overlay uri");
    let _ = documents.did_open(&TextDocumentItem {
        uri: overlay_uri.clone(),
        language_id: "vue".to_string(),
        version: 1,
        text: r#"<script setup lang="ts">
defineProps<{ show: boolean }>()
</script>
<template><div v-if="show">overlay</div></template>"#
            .to_string(),
    });

    // Host registry owns everything (so the barrel resolves Overlay). The
    // SNAPSHOT resolver below is rooted at `/other`, so ownership of Overlay
    // resolves to None — that is the owner-None barrel arm under test.
    let workspace_uri = crate::uri::path_to_file_uri_string(&workspace_id);
    let vite_opts = verter_workspace::ViteConfigOptions::default();
    let registry_ws =
        verter_workspace::FilesystemWorkspace::new(verter_workspace::FilesystemOptions::default());
    let build_result = crate::config::ProjectRegistry::from_workspace_roots(
        &registry_ws,
        &[workspace_uri],
        &vite_opts,
    );
    host.configure_projects(
        build_result
            .registry
            .projects()
            .iter()
            .map(|p| p.to_ide_project_config())
            .collect(),
    );
    // Ready snapshot at `/other` — does NOT own the workspace's Overlay.vue.
    let vfs_workspace = crate::test_utils::make_test_vfs_workspace_with_resolver(
        "/other",
        Some("/other/tsconfig.json"),
    );

    let provider = Arc::new(MockTypeProvider::new());
    let sync = ProjectSync::new(provider.clone(), ProjectSyncMode::FullProject);

    let provider_sync_states = DashMap::new();
    // Prior committed state: STALE `Owned` binding with the live IDE TSX AND a
    // stale owner-derived `.vue.ts` API path (as if a prior owner had synced it).
    // The barrel pass must reconcile this owner-loss in place.
    provider_sync_states.insert(
        overlay_id.clone(),
        ProviderSyncState {
            owner_binding: crate::provider_sync::ProviderOwnerBinding::Owned(
                "/stale/tsconfig.json".to_string(),
            ),
            ide_path: Some(overlay_tsx.clone()),
            api_path: Some(overlay_api.clone()),
            decl_path: None,
            ide_background_loaded: true,
            api_background_loaded: true,
            decl_background_loaded: false,
            shadow_path: None,
            shadow_background_loaded: false,
            committed_ide_surface: None,
            commit_stamp: None,
            api_delivered_hash: None,
            api_observed_hash: None,
            shadow_delivered_source_hash: None,
        },
    );

    // TSGO barrel pass (is_tsgo = true) reaches the owner-None arm for Overlay.
    resync_aliased_imports_for_open_files(
        &documents,
        Some(&sync),
        &vfs_workspace,
        &provider_sync_states,
        true,
        None,
        &DeclOverlayOwner::default(),
        1,
        &crate::external_ts::CarrierTransactionCoordinator::new(),
        &dashmap::DashSet::new(),
    )
    .await;

    // Discriminator: the open Overlay's state must SURVIVE (pre-fix it was
    // removed by remove_provider_sync_state_and_close_paths) AND its stale
    // `Owned` binding must be RECONCILED to Unresolved. The binding flip only
    // happens if the barrel pass actually reached Overlay (R2-7 reach signal #1).
    let state = provider_sync_states
        .get(&overlay_id)
        .map(|entry| entry.clone())
        .expect("open barrel-dep Overlay.vue must keep its provider sync state when unowned");
    assert!(
        state.is_unresolved(),
        "open barrel-dep must be RECONCILED to Unresolved on owner loss (proves the \
         barrel pass reached it), got {:?}",
        state.owner_binding
    );
    assert_eq!(
        state.ide_path.as_deref(),
        Some(overlay_tsx.as_str()),
        "open barrel-dep must preserve its live IDE TSX path"
    );
    assert!(
        state.api_path.is_none(),
        "the stale owner-derived `.vue.ts` must be dropped on owner loss, got {:?}",
        state.api_path
    );

    let calls = provider.file_sync_calls();
    // R2-7 reach signal #2 (DISCRIMINATES a no-op barrel-discovery regression):
    // the dropped owner-derived `.vue.ts` must be CLOSED (R2-8). This only occurs
    // if the barrel → Vue-re-export discovery actually REACHED Overlay; a no-op
    // regression would leave the seeded `Owned`+`.vue.ts` state untouched and
    // close nothing, so the assertions would NOT pass vacuously.
    assert!(
        calls.iter().any(|call| matches!(
            call,
            MockCall::CloseFile { path } if path == &overlay_api
        )),
        "barrel pass must REACH Overlay and close the dropped `.vue.ts`, calls={calls:?}"
    );
    // Discriminator: the open Overlay's live IDE TSX must NOT be closed.
    assert!(
        !calls.iter().any(|call| matches!(
            call,
            MockCall::CloseFile { path } if path == &overlay_tsx
        )),
        "open barrel-dep must NOT close its live IDE TSX, calls={calls:?}"
    );

    let _ = std::fs::remove_dir_all(&temp_base);
}

#[tokio::test(flavor = "multi_thread")]
async fn resync_background_vue_reconciles_owner_loss_when_ide_output_is_absent() {
    // R3-5 [P1]: the owner-None preserve/convert on the background resync path
    // must NOT be gated behind fresh compile (`get_ide`) output. Pre-fix the sync
    // body did `let Some(ide) = host.get_ide(..) else { return; }` BEFORE the
    // owner check, so a transient IDE compile miss (`ide == None`) left a
    // previously-`Owned` OPEN `.vue` stranded on its stale owner (the `no
    // ide_context` class). The fix detects owner-None and reconciles the BINDING
    // (force `Unresolved`, drop+close the owner-derived `.vue.ts`) before
    // requiring IDE output.
    //
    // Driven through `sync_compiled_carrier_to_provider` (the post-compile sync
    // decision, separated from the destructive disk reload) with `ide = None` —
    // the exact absent-IDE-output condition — under an owner-None snapshot for an
    // OPEN document.
    let provider = Arc::new(MockTypeProvider::new());
    let type_provider: Arc<dyn TypeProvider> = provider.clone();
    let service = make_hover_test_service(type_provider);
    let server = service.inner();
    // Ready snapshot at `/other` — does NOT own the open `/workspace` file.
    install_test_resolver_for_root(server, "/other", Some("/other/tsconfig.json"));

    let _uri = open_test_vue(
        server,
        "/workspace/src/App.vue",
        r#"<script setup lang="ts">
defineProps<{ msg: string }>()
</script>
<template><div>{{ msg }}</div></template>
"#,
    );
    let canonical_id = "/workspace/src/App.vue";
    let app_tsx = "/workspace/src/App.vue.tsx";
    let app_api = "/workspace/src/App.vue.verter.ts";

    // Seed a STALE `Owned` committed state with the IDE TSX live AND an owner-
    // derived `.vue.ts`. The owner is now None and the IDE output is absent —
    // pre-fix the binding stays stale (early return on the absent-IDE gate).
    server.commit_provider_sync_state(
        canonical_id,
        ProviderSyncState {
            owner_binding: crate::provider_sync::ProviderOwnerBinding::Owned(
                "/stale/tsconfig.json".to_string(),
            ),
            ide_path: Some(app_tsx.to_string()),
            api_path: Some(app_api.to_string()),
            decl_path: None,
            ide_background_loaded: true,
            api_background_loaded: true,
            decl_background_loaded: false,
            shadow_path: None,
            shadow_background_loaded: false,
            committed_ide_surface: None,
            commit_stamp: None,
            api_delivered_hash: None,
            api_observed_hash: None,
            shadow_delivered_source_hash: None,
        },
    );

    // Absent IDE output: pass `ide = None` (the transient compile-miss condition).
    server
        .sync_compiled_carrier_to_provider(canonical_id, None, None)
        .await;

    // Discriminator (RED pre-fix): the stale `Owned` binding survived because the
    // body returned on the absent-IDE gate before the owner check.
    let state = server
        .provider_sync_state_for_source(canonical_id)
        .expect("open Vue file must keep its provider sync state on owner loss");
    assert!(
        state.is_unresolved(),
        "owner loss with absent IDE output must reconcile the binding to Unresolved \
         (not return early on the IDE gate), got {:?}",
        state.owner_binding
    );
    // The live IDE TSX path is preserved (owner-independent artifact).
    assert_eq!(
        state.ide_path.as_deref(),
        Some(app_tsx),
        "the open file's live IDE TSX path must be preserved across the conversion"
    );
    // The stale owner-derived `.vue.ts` is dropped from the committed state…
    assert!(
        state.api_path.is_none(),
        "the stale owner-derived `.vue.ts` must be dropped on owner loss, got {:?}",
        state.api_path
    );

    let calls = provider.file_sync_calls();
    // …and CLOSED in the provider (R2-8), proving the conversion actually ran.
    assert!(
        calls.iter().any(|call| matches!(
            call,
            MockCall::CloseFile { path } if path == app_api
        )),
        "owner-loss reconcile must close the dropped `.vue.ts` even with absent IDE output, calls={calls:?}"
    );
    // The live IDE TSX must NEVER be closed (editor-liveness).
    assert!(
        !calls.iter().any(|call| matches!(
            call,
            MockCall::CloseFile { path } if path == app_tsx
        )),
        "owner-loss reconcile must NOT close the live IDE TSX, calls={calls:?}"
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn resync_background_vue_reconciles_owner_loss_before_compile_gate() {
    // R5-2 [P1]: the owner-None reconcile must run BEFORE the destructive disk
    // reload + compile gate in `resync_background_carrier_file`. R3-5 moved the
    // reconcile before the IDE-output requirement INSIDE
    // `sync_compiled_carrier_to_provider`, but the OUTER compile gate in
    // `resync_background_carrier_file` (`host.remove` then `ensure_loaded` then
    // `ensure_compiled`, with an early `return` on failure) still short-circuits
    // BEFORE `sync_compiled_carrier_to_provider` is ever called. A COMPILE FAILURE
    // (here: `host.remove` drops the in-memory source and the test harness has no
    // disk file to reload, so the load/compile gate fails) therefore left a
    // previously-`Owned` OPEN `.vue` stranded on its stale owner.
    //
    // The fix detects owner-None via the published resolver (a pure resolver
    // query, no compile) and reconciles the binding before the destructive
    // reload. Driven through `resync_background_carrier_file` (the entry that owns
    // the compile gate) under an owner-None snapshot for an OPEN document.
    let provider = Arc::new(MockTypeProvider::new());
    let type_provider: Arc<dyn TypeProvider> = provider.clone();
    let service = make_hover_test_service(type_provider);
    let server = service.inner();
    // Ready snapshot at `/other` — does NOT own the open `/workspace` file.
    install_test_resolver_for_root(server, "/other", Some("/other/tsconfig.json"));

    let _uri = open_test_vue(
        server,
        "/workspace/src/App.vue",
        r#"<script setup lang="ts">
defineProps<{ msg: string }>()
</script>
<template><div>{{ msg }}</div></template>
"#,
    );
    let canonical_id = "/workspace/src/App.vue";
    let app_tsx = "/workspace/src/App.vue.tsx";
    let app_api = "/workspace/src/App.vue.verter.ts";

    // Seed a STALE `Owned` committed state with the IDE TSX live AND an owner-
    // derived `.vue.ts`. The owner is now None; the destructive reload below
    // will fail to recompile (no disk file) — pre-fix the binding stays stale
    // because the compile gate returns before the owner-None reconcile.
    server.commit_provider_sync_state(
        canonical_id,
        ProviderSyncState {
            owner_binding: crate::provider_sync::ProviderOwnerBinding::Owned(
                "/stale/tsconfig.json".to_string(),
            ),
            ide_path: Some(app_tsx.to_string()),
            api_path: Some(app_api.to_string()),
            decl_path: None,
            ide_background_loaded: true,
            api_background_loaded: true,
            decl_background_loaded: false,
            shadow_path: None,
            shadow_background_loaded: false,
            committed_ide_surface: None,
            commit_stamp: None,
            api_delivered_hash: None,
            api_observed_hash: None,
            shadow_delivered_source_hash: None,
        },
    );

    server.resync_background_carrier_file(canonical_id).await;

    // Discriminator (RED pre-fix): the stale `Owned` binding survived because the
    // compile gate returned before the owner-None reconcile ran.
    let state = server
        .provider_sync_state_for_source(canonical_id)
        .expect("open Vue file must keep its provider sync state on owner loss");
    assert!(
        state.is_unresolved(),
        "owner loss with a failing compile gate must reconcile the binding to \
         Unresolved (not return early on the compile gate), got {:?}",
        state.owner_binding
    );
    // The live IDE TSX path is preserved (owner-independent artifact).
    assert_eq!(
        state.ide_path.as_deref(),
        Some(app_tsx),
        "the open file's live IDE TSX path must be preserved across the conversion"
    );
    // The stale owner-derived `.vue.ts` is dropped from the committed state…
    assert!(
        state.api_path.is_none(),
        "the stale owner-derived `.vue.ts` must be dropped on owner loss, got {:?}",
        state.api_path
    );

    let calls = provider.file_sync_calls();
    // …and CLOSED in the provider (R2-8), proving the conversion actually ran
    // even though the compile gate would have failed.
    assert!(
        calls.iter().any(|call| matches!(
            call,
            MockCall::CloseFile { path } if path == app_api
        )),
        "owner-loss reconcile must close the dropped `.vue.ts` even when the compile \
         gate fails, calls={calls:?}"
    );
    // The live IDE TSX must NEVER be closed (editor-liveness).
    assert!(
        !calls.iter().any(|call| matches!(
            call,
            MockCall::CloseFile { path } if path == app_tsx
        )),
        "owner-loss reconcile must NOT close the live IDE TSX, calls={calls:?}"
    );
}

/// Gap (d) — `resync_background_carrier_file` owner loss must RETRACT the
/// ledger-backed `getExternalFiles` membership, not only reconcile the provider
/// buffer binding. DISCRIMINATING: pre-change the resync owner-loss path corrected
/// the binding (preserve/clear) but left the STORE/ledger membership advertised.
#[tokio::test(flavor = "multi_thread")]
async fn resync_background_owner_loss_retracts_ledger_membership() {
    use crate::external_ts::CanonicalSource;
    let provider = Arc::new(MockTypeProvider::new());
    let type_provider: Arc<dyn TypeProvider> = provider.clone();
    let service = make_hover_test_service(type_provider);
    let server = service.inner();
    let tsconfig = "/workspace/tsconfig.json";
    let canonical_id = "/workspace/src/App.vue";
    install_test_resolver_for_root(server, "/workspace", Some(tsconfig));
    let uri = open_test_vue(server, canonical_id, MEMBERSHIP_TEST_VUE);

    // Publish under the resolved owner (the carrier is advertised).
    server.ensure_current_file_synced(&uri).await;
    assert!(
        server
            .membership_ledger()
            .expect("ledger")
            .is_advertised(&CanonicalSource::from(canonical_id)),
        "precondition: the carrier is advertised after the owned publish"
    );

    // Owner loss: a ready snapshot rooted ELSEWHERE that does not own the file.
    install_test_resolver_for_root(server, "/other", Some("/other/tsconfig.json"));
    server.resync_background_carrier_file(canonical_id).await;

    assert!(
        !server
            .membership_ledger()
            .expect("ledger")
            .is_advertised(&CanonicalSource::from(canonical_id)),
        "resync owner-loss MUST retract the ledger-backed getExternalFiles membership"
    );
}

/// A DIFFERENT bug class from the pin-staleness races the sibling tests in
/// this file discriminate: here the pin's identity NEVER MOVES (the document
/// is never edited) while the CONTENT the compile actually reads is silently
/// swapped out from under it.
///
/// `resync_background_carrier_file`'s destructive reload
/// (`host.remove` — which clears the workspace overlay via
/// `FilesystemWorkspace::notify_delete` — followed by `ensure_loaded`) is
/// designed for a genuinely closed dependency. When the SAME canonical id is
/// ALSO open in the editor with unsaved edits, the reload's `ensure_loaded`
/// used to fail outright for the previously-tracked/open document: verified
/// by hand (temporarily reverting the fix) that the raw workspace read after
/// `host.remove` genuinely returns DISK content (`disk-a`), but the host's
/// SCHEDULER-level `ensure_loaded` — which `resync_background_carrier_file`
/// actually calls — then fails to reload it at all, so nothing compiles and
/// nothing gets recorded. Either manifestation (a torn A/B pair, or a total
/// reload failure for a file the host had previously tracked) is the SAME
/// root defect: the destructive reload never re-establishes state for an
/// open document from its OWN buffer, so downstream behavior for that file is
/// undefined relative to what the open document actually holds. Post-fix,
/// the destructive reload re-establishes the host overlay from the open
/// document's OWN buffer before compiling (the scheduler's fast path then
/// finds it immediately), so the recorded surface coherently reflects B.
#[tokio::test(flavor = "multi_thread")]
async fn destructive_background_reload_never_substitutes_disk_for_an_open_documents_buffer() {
    let temp = tempfile::tempdir().expect("temp dir");
    let workspace_dir = temp.path().join("workspace");
    std::fs::create_dir_all(workspace_dir.join("src")).expect("workspace source dir");
    const SOURCE_A: &str = "<script setup lang=\"ts\">\nconst msg = 'disk-a'\n</script>\n\
                             <template><div>{{ msg }}</div></template>\n";
    const SOURCE_B: &str = "<script setup lang=\"ts\">\nconst msg = 'buffer-b'\n</script>\n\
                             <template><div>{{ msg }}</div></template>\n";
    std::fs::write(workspace_dir.join("src/App.vue"), SOURCE_A).expect("write disk source A");
    std::fs::write(
        workspace_dir.join("tsconfig.json"),
        r#"{ "include": ["src/**/*.vue"] }"#,
    )
    .expect("write tsconfig");

    let workspace_root = crate::test_utils::canonical_test_path(&workspace_dir);
    let tsconfig = format!("{workspace_root}/tsconfig.json");
    let canonical_id = format!("{workspace_root}/src/App.vue");

    let provider = Arc::new(MockTypeProvider::new());
    let type_provider: Arc<dyn TypeProvider> = provider.clone();
    let service = make_hover_test_service_tsgo(type_provider);
    let server = service.inner();
    // Installs a REAL disk-backed `FilesystemWorkspace` as both the
    // ownership-resolution source and (via `set_workspace`) the host's own
    // workspace — the same one `host.remove` / `ensure_loaded` reload
    // through, so this genuinely exercises disk fallthrough.
    install_test_resolver_for_root(server, &workspace_root, Some(&tsconfig));

    // Open the SAME file with UNSAVED content B — deliberately diverging
    // from disk's content A. No edit happens after this: the pin's identity
    // is stable for the rest of the test, so any torn pair proves content
    // substitution, not a stale-pin race.
    let uri = open_test_vue(server, &canonical_id, SOURCE_B);

    server.resync_background_carrier_file(&canonical_id).await;

    assert_eq!(
        server
            .documents
            .get(&uri)
            .expect("document stays open")
            .source
            .as_ref(),
        SOURCE_B,
        "precondition: the open buffer still holds the unsaved edit"
    );

    let ide_path =
        verter_session_query::resolution::carrier_ide_provider_path(&canonical_id, false);
    let recorded = server
        .documents
        .provider_surfaces()
        .current_snapshot(&ide_path)
        .expect(
            "the destructive reload must still deliver a coherent surface for the \
             open document — reflecting its own live buffer",
        );
    assert!(
        recorded.provider_content.contains("buffer-b"),
        "a destructive background reload must compile the OPEN document's own \
         live buffer, never silently fall through to disk — got: {}",
        recorded.provider_content
    );
    assert!(
        !recorded.provider_content.contains("disk-a"),
        "a destructive background reload must never pair disk content with the \
         open document's identity/source — got: {}",
        recorded.provider_content
    );
}

/// Gap (b) — for tsserver, the background API sync routes the carrier to the drain
/// (which reconciles through the membership reconciler) instead of running the
/// `ProjectSync` content task whose `.vue.ts` verbs are tsserver NO-OPS.
/// DISCRIMINATING: pre-change, with a ready snapshot + owner, it spawned the
/// background task (which never queued and never reached the store), so the carrier
/// was NOT in the pending-snapshot drain set.
#[tokio::test(flavor = "multi_thread")]
async fn sync_api_background_queues_carrier_for_drain_on_tsserver() {
    let provider = Arc::new(MockTypeProvider::new());
    let type_provider: Arc<dyn TypeProvider> = provider.clone();
    let service = make_hover_test_service(type_provider);
    let server = service.inner();
    let tsconfig = "/workspace/tsconfig.json";
    let canonical_id = "/workspace/src/App.vue";
    install_test_resolver_for_root(server, "/workspace", Some(tsconfig));
    let uri = open_test_vue(server, canonical_id, MEMBERSHIP_TEST_VUE);

    // Clear any queue entry seeded by `did_open` so the assertion is discriminating.
    server.pending_snapshot_provider_sync.remove(canonical_id);
    server.sync_api_to_provider_in_background(uri);

    assert!(
        server.pending_snapshot_provider_sync.contains(canonical_id),
        "the tsserver background API sync MUST queue the carrier for the drain (the \
         single membership reconciler), not run the no-op ProjectSync content task"
    );
}

/// An open script that NO configured project owns (a TypeScript lib file the
/// user navigated into, a scratch file outside the workspace) can never be
/// delivered to the provider. Once ownership is authoritative that is a
/// terminal answer, not a retry: left queued, it would hold the pending set
/// non-empty for the rest of the session and level 2 would never be announced.
#[tokio::test(flavor = "multi_thread")]
async fn an_open_script_with_no_owning_project_is_not_retried_forever() {
    let fixture = watched_dependency_fixture(true).await;
    let server = fixture.service.inner();
    let outside = tempfile::tempdir().unwrap();
    let ownerless = format!(
        "{}/lib.ownerless.d.ts",
        crate::test_utils::canonical_test_path(outside.path())
    );
    std::fs::write(
        outside.path().join("lib.ownerless.d.ts"),
        "declare const x: 1;",
    )
    .unwrap();
    server.documents.did_open(&TextDocumentItem {
        uri: crate::uri::path_to_file_uri(&ownerless).unwrap(),
        language_id: "typescript".into(),
        version: 1,
        text: "declare const x: 1;".into(),
    });
    let owned = format!("{}/src/helper.ts", fixture.root);

    server.queue_snapshot_provider_sync(ownerless.clone());
    server.queue_snapshot_provider_sync(owned.clone());
    drain_pending_provider_sync_for(server).await;

    assert!(
        !server.pending_snapshot_provider_sync.contains(&owned),
        "control: an owned script settles and is dequeued"
    );
    assert!(
        !server.pending_snapshot_provider_sync.contains(&ownerless),
        "a script no project owns is a terminal answer, not a pending retry"
    );
    let generation = server
        .init_generation
        .load(std::sync::atomic::Ordering::Acquire);
    assert!(
        server.complete_post_scan(generation).await,
        "with nothing genuinely pending, level 2 is announced"
    );
}

/// The import-dependency publication that mints a document's DependencyReady
/// receipt is what releases the publication a running workspace scan held back
/// for want of it: the document is certified while the scan flag is still up.
#[tokio::test(flavor = "multi_thread")]
async fn minting_a_dependency_receipt_releases_the_publication_a_scan_held() {
    let fixture = watched_dependency_fixture(true).await;
    let server = fixture.service.inner();
    let id = format!("{}/src/Held.vue", fixture.root);
    let uri = workspace_uri(&fixture.root, "src/Held.vue");
    let source = "<script setup lang=\"ts\">\nimport { value } from './helper';\n</script>\n<template>{{ value }}</template>";
    std::fs::write(fixture._temp.path().join("src/Held.vue"), source).unwrap();
    server.documents.did_open(&TextDocumentItem {
        uri: uri.clone(),
        language_id: "vue".into(),
        version: 1,
        text: source.into(),
    });
    server.refresh_carrier_dependency_tracking(&id);
    server.ensure_current_file_synced(&uri).await;

    server.sync_coordinator.set_workspace_scan_in_progress(true);
    let ticks_before = server.sync_coordinator.dispatch_ticks();
    server.sync_coordinator.signal_diagnostics_only(
        id.clone(),
        uri.to_string(),
        tokio::time::Instant::now() - std::time::Duration::from_secs(60),
    );
    server
        .sync_coordinator
        .await_until(
            || {
                !server.sync_coordinator.inbox_contains(&id)
                    && server.sync_coordinator.dispatch_ticks() > ticks_before
            },
            || panic!("the coordinator never dispatched the overdue signal"),
        )
        .await;
    assert!(
        !server.documents.diagnostics_ready(&uri),
        "with no current receipt the scan holds the publication back"
    );

    server.publish_import_dependencies_settled(&uri).await;
    server
        .sync_coordinator
        .await_until(
            || {
                server.documents.diagnostics_ready(&uri)
                    && server.sync_coordinator.diag_tasks_live() == 0
            },
            || panic!("the minted receipt must release the held publication mid-scan"),
        )
        .await;
    assert!(server.sync_coordinator.workspace_scan_in_progress());
    server
        .sync_coordinator
        .set_workspace_scan_in_progress(false);
}

/// An isolated edit elsewhere re-currents every rootless receipt without any
/// import pass minting one; the publication a running workspace scan held for
/// want of a current receipt is released by that promotion, not at scan end.
#[tokio::test(flavor = "multi_thread")]
async fn promoting_a_rootless_receipt_releases_the_publication_a_scan_held() {
    let fixture = watched_dependency_fixture(true).await;
    let server = fixture.service.inner();
    let id = format!("{}/src/Held.vue", fixture.root);
    let uri = workspace_uri(&fixture.root, "src/Held.vue");
    let source = "<template><p>held</p></template>";
    std::fs::write(fixture._temp.path().join("src/Held.vue"), source).unwrap();
    server.documents.did_open(&TextDocumentItem {
        uri: uri.clone(),
        language_id: "vue".into(),
        version: 1,
        text: source.into(),
    });
    server.refresh_carrier_dependency_tracking(&id);
    server.ensure_current_file_synced(&uri).await;

    server.sync_coordinator.set_workspace_scan_in_progress(true);
    let ticks_before = server.sync_coordinator.dispatch_ticks();
    server.sync_coordinator.signal_diagnostics_only(
        id.clone(),
        uri.to_string(),
        tokio::time::Instant::now() - std::time::Duration::from_secs(60),
    );
    server
        .sync_coordinator
        .await_until(
            || {
                !server.sync_coordinator.inbox_contains(&id)
                    && server.sync_coordinator.dispatch_ticks() > ticks_before
            },
            || panic!("the coordinator never dispatched the overdue signal"),
        )
        .await;
    assert!(
        !server.documents.diagnostics_ready(&uri),
        "with no current receipt the scan holds the publication back"
    );

    let key = server
        .import_sync_freshness_key()
        .expect("the fixture publishes a resolver snapshot");
    server
        .import_sync
        .record_delivered_with_rootless(id.clone(), key, true);

    let unrelated_uri = workspace_uri(&fixture.root, "src/Unrelated.vue");
    super::super::lifecycle::handle_did_change(
        server,
        DidChangeTextDocumentParams {
            text_document: VersionedTextDocumentIdentifier {
                uri: unrelated_uri,
                version: 2,
            },
            content_changes: vec![TextDocumentContentChangeEvent {
                range: None,
                range_length: None,
                text: "<template><p>edited</p></template>".into(),
            }],
        },
    )
    .await;
    server
        .sync_coordinator
        .await_until(
            || {
                server.documents.diagnostics_ready(&uri)
                    && server.sync_coordinator.diag_tasks_live() == 0
            },
            || panic!("the promoted receipt must release the held publication mid-scan"),
        )
        .await;
    assert!(server.sync_coordinator.workspace_scan_in_progress());
    server
        .sync_coordinator
        .set_workspace_scan_in_progress(false);
}

/// A restart replays every document the client still holds as an open, and
/// nothing in a replayed open singles out the one a caller is about to assert
/// on. Its status poll names it through the existing per-document analysis
/// request, and that request alone must move it ahead of every newer replayed
/// open: no fresh open and no other editor request against it is involved.
#[tokio::test(flavor = "multi_thread")]
async fn a_status_request_naming_a_document_serves_it_ahead_of_replayed_opens() {
    let fixture = watched_dependency_fixture(true).await;
    let server = fixture.service.inner();
    let inflight = crate::sync_coordinator::max_inflight_diagnostics(&server.type_provider_kind);
    let names: Vec<String> = (0..inflight + 3)
        .map(|index| format!("Replayed{index}"))
        .collect();
    let source = "<template><p>replayed</p></template>";
    let docs: Vec<(String, Uri)> = names
        .iter()
        .map(|name| {
            std::fs::write(
                fixture._temp.path().join("src").join(format!("{name}.vue")),
                source,
            )
            .unwrap();
            (
                format!("{}/src/{name}.vue", fixture.root),
                workspace_uri(&fixture.root, &format!("src/{name}.vue")),
            )
        })
        .collect();
    let pulled = |calls: &[MockCall]| -> Vec<String> {
        calls
            .iter()
            .filter_map(|call| match call {
                MockCall::GetDiagnostics { path } => path
                    .split("/src/")
                    .nth(1)
                    .and_then(|rest| rest.split('.').next())
                    .filter(|name| name.starts_with("Replayed"))
                    .map(str::to_string),
                _ => None,
            })
            .collect()
    };

    // Every pull waits at the provider, so the replayed opens fill the window
    // and stay there. Each open's debounced work is held by a change ticket
    // until every open's quiet window has elapsed, so the coordinator ranks the
    // whole replay at once: newest open first, and `Replayed0` — the document
    // the caller asserts on, and the first the client replays — last.
    let gate = fixture.provider.gate_diagnostics();
    fixture.provider.clear_calls();
    let held: Vec<_> = docs
        .iter()
        .map(|(id, _)| server.sync_coordinator.change_received(id.clone()))
        .collect();
    for (_, uri) in &docs {
        super::super::lifecycle::handle_did_open(
            server,
            DidOpenTextDocumentParams {
                text_document: TextDocumentItem {
                    uri: uri.clone(),
                    language_id: "vue".into(),
                    version: 1,
                    text: source.into(),
                },
            },
        )
        .await;
    }
    tokio::time::sleep(crate::edit_quiet_window::EDIT_QUIET_WINDOW * 2).await;
    for ticket in held.into_iter().rev() {
        drop(ticket);
    }
    tokio::time::timeout(
        std::time::Duration::from_secs(20),
        fixture
            .provider
            .wait_until_calls(|calls| pulled(calls).len() >= inflight),
    )
    .await
    .expect("the replayed opens never filled the pull window");
    assert!(
        !pulled(&fixture.provider.calls()).contains(&names[0]),
        "the asserted document is the oldest replayed open, so it must still be unpulled: {:?}",
        pulled(&fixture.provider.calls())
    );

    // The caller polls the asserted document's status the way the E2E-only
    // command does: an existing per-document request names it — the demand —
    // and only after it answers is the unchanged statistics snapshot read.
    server
        .get_analysis(crate::server::protocol_types::GetAnalysisParams {
            uri: docs[0].1.to_string(),
        })
        .await
        .expect("the per-document request must answer");
    let status = server
        .get_statistics(Some(
            crate::server::protocol_types::StatisticsRequestParams {
                include_events: false,
                scope: None,
            },
        ))
        .await
        .expect("the status request must answer");
    assert_eq!(
        status.diagnostics[docs[0].1.as_str()]["ready"],
        serde_json::json!(false)
    );

    // One pull finishes: the slot it frees goes to the document the status
    // request named, not to the newer replayed opens still owed a pull.
    gate.add_permits(1);
    tokio::time::timeout(
        std::time::Duration::from_secs(20),
        fixture
            .provider
            .wait_until_calls(|calls| pulled(calls).len() > inflight),
    )
    .await
    .expect("a freed slot must be refilled");
    let order = pulled(&fixture.provider.calls());
    assert_eq!(
        order[inflight], names[0],
        "the status request must put its document ahead of the replay: {order:?}"
    );

    gate.add_permits(names.len() * 4);
    server
        .sync_coordinator
        .await_until(
            || {
                docs.iter()
                    .all(|(_, uri)| server.documents.diagnostics_ready(uri))
                    && server.sync_coordinator.diag_tasks_live() == 0
            },
            || panic!("the replay must still drain completely"),
        )
        .await;
}

/// With an import the target actually depends on parked mid-delivery, the
/// production scan holds the target's publication fail-closed: its tick
/// decides, and holds, but the target is never certified. Once that
/// dependency's delivery settles the target is certified with the scan still
/// running.
#[tokio::test(flavor = "multi_thread")]
async fn a_scan_keeps_a_document_unready_while_its_imported_dependency_is_parked() {
    let fixture = scan_publication_fixture(false).await;
    let server = fixture.server();
    let (scan_parked, release_scan) = crate::sync_coordinator::test_hooks::block_after_ide_compile(
        &fixture.id("far/Unrelated.vue"),
    );
    // The child's delivery serializes on its own lifecycle lane; holding it
    // parks every import publication that would deliver it, and so the
    // target's receipt.
    let child_lane = server.ide_sync_lifecycle_lease(&fixture.id("src/Child.vue"));
    let child_parked = child_lane.lock().await;
    let target = fixture.open("src/Target.vue").await;
    let holds_before = server.sync_coordinator.dependency_holds();

    server
        .spawn_background_init(None, "parked dependency")
        .await;
    tokio::time::timeout(std::time::Duration::from_secs(20), scan_parked.notified())
        .await
        .expect("the workspace scan must reach the unrelated item");
    fixture.await_ready_announced().await;
    fixture
        .await_while_polling(
            &target,
            || server.sync_coordinator.dependency_holds() > holds_before,
            "the target's publication must be decided, and held, during the scan",
        )
        .await;
    assert!(
        !server.documents.diagnostics_ready(&target),
        "a document whose imported dependency is parked must stay not-ready"
    );
    assert!(server.sync_coordinator.workspace_scan_in_progress());

    drop(child_parked);
    fixture
        .await_while_polling(
            &target,
            || {
                server.documents.diagnostics_ready(&target)
                    && server.sync_coordinator.diag_tasks_live() == 0
            },
            "the target must be certified once its dependency is delivered",
        )
        .await;
    assert!(server.sync_coordinator.workspace_scan_in_progress());
    assert!(fixture.sync_complete.lock().is_empty());

    release_scan.notify_one();
    fixture.await_sync_complete().await;
}

/// An edit is no exemption from the scan's dependency gate. With the target's
/// real imported child parked mid-delivery and the production scan parked on
/// unrelated work, the target is edited through the production `didChange`
/// handler: the edited revision is synced to the provider and its publication
/// decided, yet it stays not-ready while the child is parked. Once the child
/// settles, the edit's own import publication mints the receipt and the edited
/// revision is certified while the unrelated scan item is still parked and
/// level 2 is still unannounced.
#[tokio::test(flavor = "multi_thread")]
async fn a_scan_keeps_an_edited_document_unready_while_its_imported_dependency_is_parked() {
    const EDIT_MARKER: &str = "edited_revision_marker";
    let fixture = scan_publication_fixture(false).await;
    let server = fixture.server();
    let (scan_parked, release_scan) = crate::sync_coordinator::test_hooks::block_after_ide_compile(
        &fixture.id("far/Unrelated.vue"),
    );
    let child_lane = server.ide_sync_lifecycle_lease(&fixture.id("src/Child.vue"));
    let child_parked = child_lane.lock().await;
    let target = fixture.open("src/Target.vue").await;
    let holds_before = server.sync_coordinator.dependency_holds();

    server.spawn_background_init(None, "parked edit").await;
    tokio::time::timeout(std::time::Duration::from_secs(20), scan_parked.notified())
        .await
        .expect("the workspace scan must reach the unrelated item");
    fixture.await_ready_announced().await;
    fixture
        .await_while_polling(
            &target,
            || server.sync_coordinator.dependency_holds() > holds_before,
            "the opened target's publication must be decided, and held, during the scan",
        )
        .await;

    // The user edits the target while the scan is parked; it still imports the
    // parked child.
    let holds_at_edit = server.sync_coordinator.dependency_holds();
    super::super::lifecycle::handle_did_change(
        server,
        DidChangeTextDocumentParams {
            text_document: VersionedTextDocumentIdentifier {
                uri: target.clone(),
                version: 2,
            },
            content_changes: vec![TextDocumentContentChangeEvent {
                range: None,
                range_length: None,
                text: format!(
                    "<script setup lang=\"ts\">\nimport Child from './Child.vue';\nconst {EDIT_MARKER} = 'edited';\n</script>\n<template><Child :label=\"{EDIT_MARKER}\" /></template>"
                ),
            }],
        },
    )
    .await;
    let edit_synced = |calls: &[MockCall]| {
        calls.iter().position(|call| match call {
            MockCall::RegisterCarrierMetadata {
                source_path,
                content,
                ..
            } => source_path.ends_with("Target.vue") && content.contains(EDIT_MARKER),
            _ => false,
        })
    };

    // The edited revision is synced, and the tick that synced it has decided
    // its publication: either held it, or certified it.
    fixture
        .await_while_polling(
            &target,
            || {
                edit_synced(&fixture.provider.calls()).is_some()
                    && (server.sync_coordinator.dependency_holds() > holds_at_edit
                        || server.documents.diagnostics_ready(&target))
            },
            "the edited revision must be synced and its publication decided during the scan",
        )
        .await;
    assert_eq!(
        server
            .documents
            .get(&target)
            .map(|document| document.version),
        Some(2)
    );
    assert!(
        !server.documents.diagnostics_ready(&target),
        "an edited document whose imported dependency is parked must stay not-ready"
    );
    assert!(server.sync_coordinator.workspace_scan_in_progress());

    drop(child_parked);
    fixture
        .await_while_polling(
            &target,
            || {
                server.documents.diagnostics_ready(&target)
                    && server.sync_coordinator.diag_tasks_live() == 0
            },
            "the edited target must be certified once its dependency is delivered",
        )
        .await;
    let calls = fixture.provider.calls();
    let synced_at = edit_synced(&calls).expect("the edited revision was synced");
    assert!(
        calls[synced_at..].iter().any(|call| matches!(
            call,
            MockCall::GetDiagnostics { path } if path.contains("Target.vue")
        )),
        "the certified diagnostics must be pulled for the edited revision: {calls:?}"
    );
    assert!(server.sync_coordinator.workspace_scan_in_progress());
    assert!(
        fixture.sync_complete.lock().is_empty(),
        "level 2 must stay unannounced while a scan item is parked"
    );

    release_scan.notify_one();
    fixture.await_sync_complete().await;
}

#[tokio::test(flavor = "multi_thread")]
async fn post_scan_recovery_never_announces_a_superseded_generation() {
    let fixture = watched_dependency_fixture(true).await;
    let server = fixture.service.inner();
    let path = format!("{}/src/helper.ts", fixture.root);
    server.queue_snapshot_provider_sync(path.clone());
    let generation = server
        .init_generation
        .load(std::sync::atomic::Ordering::Acquire);
    let announced = fixture.sync_complete.lock().len();
    let completion = background_init::finish_post_scan(Arc::downgrade(&server.core), generation);
    tokio::pin!(completion);
    assert!(futures_util::poll!(&mut completion).is_pending());
    server
        .init_generation
        .fetch_add(1, std::sync::atomic::Ordering::AcqRel);
    drain_pending_provider_sync_for(server).await;
    assert!(
        !tokio::time::timeout(std::time::Duration::from_secs(5), completion)
            .await
            .unwrap()
    );
    assert_eq!(fixture.sync_complete.lock().len(), announced);
}

#[tokio::test(flavor = "multi_thread")]
async fn watched_ts_create_refreshes_missing_lookup_before_open_importers() {
    use verter_workspace::WorkspaceRead;
    let fixture = watched_dependency_fixture(false).await;
    let server = fixture.service.inner();
    let helper = format!("{}/src/helper.ts", fixture.root);
    let consumer = format!("{}/src/Consumer.vue", fixture.root);
    let uri = workspace_uri(&fixture.root, "src/Consumer.vue");
    let workspace = server.vfs_workspace.read().clone().unwrap();
    assert!(
        !workspace.file_exists(&helper),
        "seed the cached missing-file lookup"
    );
    assert!(
        workspace.affected_canonicals(&helper).contains(&consumer),
        "an unresolved relative import must identify its consumer"
    );
    let provider_path = server
        .capture_provider_request_surface(&uri)
        .unwrap()
        .stamp
        .provider_path
        .to_string();
    fixture.provider.clear_calls();
    std::fs::write(
        fixture._temp.path().join("src/helper.ts"),
        "export const value = 2;",
    )
    .unwrap();
    crate::server::lifecycle::handle_did_change_watched_files(
        server,
        DidChangeWatchedFilesParams {
            changes: vec![FileEvent {
                uri: workspace_uri(&fixture.root, "src/helper.ts"),
                typ: FileChangeType::CREATED,
            }],
        },
    )
    .await;
    assert!(
        workspace.file_exists(&helper),
        "standard watcher events must invalidate cached absence even for a never-loaded helper"
    );
    server.sync_coordinator.await_until(
        || fixture.provider.calls().iter().any(|call| matches!(call, MockCall::GetDiagnostics { path } if path == &provider_path))
            && server.documents.diagnostics_ready(&uri) && server.sync_coordinator.diag_tasks_live() == 0,
        || panic!("created dependency never reached the open consumer"),
    ).await;
    let calls = fixture.provider.calls();
    let load = calls
        .iter()
        .position(|call| matches!(call, MockCall::LoadFile { path, .. } if path == &helper))
        .unwrap();
    assert!(calls.iter().enumerate().all(|(index, call)| !matches!(call, MockCall::GetDiagnostics { path } if index <= load || path != &provider_path)), "consumer queries must follow dependency publication: {calls:?}");

    let helper_uri = workspace_uri(&fixture.root, "src/helper.ts");
    let unsaved = "export const value = 'unsaved';";
    server.documents.did_open(&TextDocumentItem {
        uri: helper_uri,
        language_id: "typescript".into(),
        version: 1,
        text: unsaved.into(),
    });
    fixture.provider.clear_calls();
    std::fs::write(
        fixture._temp.path().join("src/helper.ts"),
        "export const value = 'disk';",
    )
    .unwrap();
    crate::server::lifecycle::handle_did_change_watched_files(
        server,
        DidChangeWatchedFilesParams {
            changes: vec![FileEvent {
                uri: workspace_uri(&fixture.root, "src/helper.ts"),
                typ: FileChangeType::CHANGED,
            }],
        },
    )
    .await;
    assert_eq!(
        server.documents.host().get_source(&helper).as_deref(),
        Some(unsaved)
    );
    assert!(
        fixture.provider.calls().is_empty(),
        "a disk event must not replace an open editor buffer"
    );
}

/// A watched `.svelte` change routes through the SAME carrier resync path as
/// `.vue` (the watcher glob includes every registered carrier extension, and
/// the lifecycle batch no longer Vue-gates the inner branch). The watched
/// `.svelte` here is never opened/compiled, so its resync finds no IDE
/// virtual-file output and creates no provider sync state and issues no
/// provider sync calls — the carrier resync is a no-op for provider state on
/// an uncompiled carrier.
#[tokio::test]
async fn watched_svelte_change_produces_no_provider_sync_state() {
    let mock = Arc::new(MockTypeProvider::new());
    let service = make_hover_test_service(mock.clone());
    let server = service.inner();
    install_test_resolver(server);

    let canonical = "/workspace/src/Box.svelte";
    crate::server::lifecycle::handle_did_change_watched_files(
        server,
        DidChangeWatchedFilesParams {
            changes: vec![FileEvent {
                uri: format!("file://{canonical}").parse().expect("valid uri"),
                typ: FileChangeType::CHANGED,
            }],
        },
    )
    .await;

    assert!(
        server.provider_sync_state_for_source(canonical).is_none(),
        "a watched .svelte change must create no provider sync state"
    );
    assert!(
        mock.file_sync_calls().is_empty(),
        "a watched .svelte change must sync nothing to the type provider"
    );
    let profile = server.documents.tsx_profile.read().clone();
    assert!(
        server
            .documents
            .host()
            .get_ide(canonical, &profile)
            .is_none(),
        "a watched .svelte change must leave no IDE virtual-file state"
    );
}

/// A closed-file `.svelte.ts` rune-module edit is classified EXPLICITLY as an
/// adapter module (the descriptor-derived `adapter_module_language_for`
/// predicate the `did_change_watched_files` branch uses) and routes through the
/// non-carrier resync — NOT the carrier path and NOT silently dropped. The
/// rune module is covered by its descriptor-derived watch glob (the S2a P1 gap,
/// closed server-side). The watched file is never opened, so the standalone
/// resync produces no provider state — the discriminating fact is the explicit
/// adapter-module classification (a carrier predicate would reject it).
#[tokio::test]
async fn did_change_watched_files_resyncs_rune_module_via_adapter_module_glob() {
    let mock = Arc::new(MockTypeProvider::new());
    let service = make_hover_test_service(mock.clone());
    let server = service.inner();
    install_test_resolver(server);

    let canonical = "/workspace/src/store.svelte.ts";
    // The rune module is classified as an ADAPTER MODULE, not a carrier — the
    // exact predicate the watched-files branch uses to route it.
    assert!(
        super::super::server_utils::adapter_module_language_for(
            &verter_session::framework::HostLanguageClassifier::default(),
            canonical
        )
        .is_some(),
        "a `.svelte.ts` is an adapter module (the descriptor-derived watch branch predicate)"
    );
    assert!(
        super::super::server_utils::carrier_language_for(
            &verter_session::framework::HostLanguageClassifier::default(),
            canonical
        )
        .is_none(),
        "a rune module is NOT a carrier — it must not route through the carrier resync"
    );

    crate::server::lifecycle::handle_did_change_watched_files(
        server,
        DidChangeWatchedFilesParams {
            changes: vec![FileEvent {
                uri: format!("file://{canonical}").parse().expect("valid uri"),
                typ: FileChangeType::CHANGED,
            }],
        },
    )
    .await;

    // No carrier IDE state is produced for a rune module (it has no IDE TSX).
    let profile = server.documents.tsx_profile.read().clone();
    assert!(
        server
            .documents
            .host()
            .get_ide(canonical, &profile)
            .is_none(),
        "a rune module produces no carrier IDE virtual-file state"
    );
}

/// The `did_change_watched_files` batch routes EVERY carrier
/// (`.vue`, `.svelte`, …) through the shared resync/delete queues — the
/// inner `language.is_vue()` gate is gone. A watched `.svelte` change whose
/// canonical falls outside every project root (owner-unresolved) enters the
/// carrier resync's unresolved-owner reconciliation EXACTLY like `.vue`,
/// queuing a snapshot provider sync for later drain. DISCRIMINATING: under
/// the pre-change Vue-only inner gate, the `.svelte` event was dropped at
/// the routing layer and nothing was queued; now it flows through.
#[tokio::test]
async fn did_change_watched_files_resyncs_svelte() {
    let mock = Arc::new(MockTypeProvider::new());
    let service = make_hover_test_service(mock.clone());
    let server = service.inner();
    // Resolver rooted elsewhere: the watched file is owner-unresolved.
    install_test_resolver_for_root(server, "/elsewhere", Some("/elsewhere/tsconfig.json"));

    let canonical = "/workspace/src/Box.svelte";
    crate::server::lifecycle::handle_did_change_watched_files(
        server,
        DidChangeWatchedFilesParams {
            changes: vec![FileEvent {
                uri: format!("file://{canonical}").parse().expect("valid uri"),
                typ: FileChangeType::CHANGED,
            }],
        },
    )
    .await;

    // The de-gated batch admits the `.svelte` carrier into the resync queue,
    // which (owner-unresolved) queues a snapshot provider sync — parity with
    // the `.vue` carrier (see
    // `background_init_drains_pending_snapshot_provider_sync_for_open_vue_file`).
    assert!(
        server.pending_snapshot_provider_sync.contains(canonical),
        "a watched .svelte carrier must flow through the carrier resync queue \
         (parity with .vue), queuing a snapshot provider sync"
    );
    // The synchronous handler itself opens no provider sync state and issues no
    // provider calls for the uncompiled carrier (the queued sync drains later
    // and is a no-op while no IDE virtual file exists).
    assert!(
        server.provider_sync_state_for_source(canonical).is_none(),
        "the synchronous watcher handler creates no provider sync state for an \
         uncompiled .svelte carrier"
    );
    assert!(
        mock.calls().is_empty(),
        "the synchronous watcher handler issues no provider calls, got {:?}",
        mock.calls()
    );

    // Discrimination control: a plain `.ts` source (NOT a carrier) takes the
    // TS/JS branch, never the carrier resync queue.
    let ts_canonical = "/workspace/src/util.ts";
    crate::server::lifecycle::handle_did_change_watched_files(
        server,
        DidChangeWatchedFilesParams {
            changes: vec![FileEvent {
                uri: format!("file://{ts_canonical}").parse().expect("valid uri"),
                typ: FileChangeType::CHANGED,
            }],
        },
    )
    .await;
    assert!(
        !server.pending_snapshot_provider_sync.contains(ts_canonical),
        "a non-carrier .ts file must not enter the carrier resync queue"
    );
}

/// `$/onFileChanged` for a `.svelte` carrier routes through the
/// carrier cleanup (delete) path. DISCRIMINATING: the pre-change
/// `params.uri.ends_with(".vue")` gate dropped the `.svelte` delete event
/// entirely, so a closed-file `.svelte` delete would NOT clean up host state;
/// the de-gated handler enters the carrier branch and removes the carrier
/// from the host. A plain `.ts` delete never enters the carrier branch (its
/// host state is unaffected by this handler).
#[tokio::test]
async fn on_file_changed_resyncs_and_cleans_svelte() {
    let mock = Arc::new(MockTypeProvider::new());
    let service = make_hover_test_service(mock.clone());
    let server = service.inner();
    install_test_resolver(server);

    // Open the carrier so the host holds its source (the watched delete must
    // then clean it up through the carrier branch).
    let canonical = "/workspace/src/Box.svelte";
    open_test_svelte(server, canonical, "<script>let x = 1;</script>");
    assert!(
        server.documents.host().get_source(canonical).is_some(),
        "precondition: the opened .svelte carrier is in the host"
    );

    // A control non-carrier `.ts` file is also present.
    let ts_canonical = "/workspace/src/util.ts";
    let _ = server.documents.did_open(&TextDocumentItem {
        uri: format!("file://{ts_canonical}").parse().unwrap(),
        language_id: "typescript".to_string(),
        version: 1,
        text: "export const x = 1;".to_string(),
    });

    // delete: the de-gated carrier branch removes the .svelte from the host.
    server
        .on_file_changed(OnFileChangedParams {
            uri: format!("file://{canonical}"),
            change_type: "delete".to_string(),
        })
        .await;
    assert!(
        server.documents.host().get_source(canonical).is_none(),
        "a watched .svelte delete must enter the carrier branch and remove the \
         carrier from the host (pre-change the .vue gate dropped this event)"
    );

    // The .ts delete does NOT take the carrier branch (the carrier branch is
    // the only place this handler removes host source); its own ingress is
    // unaffected here.
    server
        .on_file_changed(OnFileChangedParams {
            uri: format!("file://{ts_canonical}"),
            change_type: "delete".to_string(),
        })
        .await;
    assert!(
        server.documents.host().get_source(ts_canonical).is_some(),
        "a non-carrier .ts file must not be removed by the carrier branch"
    );
}

/// F1 (the generic stale closer never issues a `close_dts` for a declaration
/// overlay). Drive the production `close_stale_provider_paths` with a state that
/// carries a live declaration overlay and assert the provider received NO close for
/// the decl path — only the non-decl artifacts close.
///
/// `close_stale_provider_paths` takes the `NonDeclProviderPathKind` slice, so it is
/// fed exactly what a removal path feeds it: `state.active_non_decl_paths()`. The
/// declaration overlay is structurally excluded, so the faithful provider records a
/// `CloseFile` for the API surface but never for `App.d.vue.ts`. RED-before (the
/// pre-fix closer took `active_paths()` — which includes `Decl` — and matched
/// `Api | Decl => close_dts`, closing the overlay; here it cannot). GREEN-after.
#[tokio::test(flavor = "multi_thread")]
async fn generic_stale_closer_never_closes_declaration_overlay() {
    use crate::provider_sync::ProviderSyncState;

    let provider = Arc::new(MockTypeProvider::new());
    let sync = ProjectSync::new(provider.clone(), ProjectSyncMode::FullProject);
    let provider_surfaces = crate::provider_surface_store::ProviderSurfaceStore::new();

    let state = ProviderSyncState {
        owner_binding: crate::provider_sync::ProviderOwnerBinding::Owned(
            "/ws/tsconfig.json".into(),
        ),
        ide_path: Some("/ws/App.vue.tsx".to_string()),
        api_path: Some("/ws/App.vue.verter.ts".to_string()),
        decl_path: Some("/ws/App.d.vue.ts".to_string()),
        shadow_path: None,
        ide_background_loaded: true,
        api_background_loaded: true,
        decl_background_loaded: true,
        shadow_background_loaded: false,
        committed_ide_surface: None,
        commit_stamp: None,
        api_delivered_hash: None,
        api_observed_hash: None,
        shadow_delivered_source_hash: None,
    };

    // Exactly the call shape the removal paths use: the generic closer over the
    // NON-DECL active set.
    crate::provider_sync::close_stale_provider_paths(
        &sync,
        &provider_surfaces,
        &state.active_non_decl_paths(),
        "decl_close_guard_test",
    )
    .await;

    let calls = provider.file_sync_calls();
    // The declaration overlay was NEVER closed by the generic closer.
    assert!(
        !calls
            .iter()
            .any(|call| matches!(call, MockCall::CloseFile { path } if path == "/ws/App.d.vue.ts")),
        "the generic stale closer must NEVER issue a close for the declaration overlay \
         (App.d.vue.ts) — its lifecycle is owned by DeclOverlayOwner; calls={calls:?}"
    );
    // The non-decl artifacts WERE closed (positive control — the closer ran and is
    // not a no-op): the API `.verter.ts` and IDE `.tsx` both closed.
    assert!(
        calls
            .iter()
            .any(|call| matches!(call, MockCall::CloseFile { path } if path == "/ws/App.vue.verter.ts")),
        "the generic stale closer DOES close the non-decl API surface (positive control), calls={calls:?}"
    );
    assert!(
        calls
            .iter()
            .any(|call| matches!(call, MockCall::CloseFile { path } if path == "/ws/App.vue.tsx")),
        "the generic stale closer DOES close the non-decl IDE surface (positive control), calls={calls:?}"
    );
}

/// F3 (ABA supersession — generation FOLDED with the reaching-root set). A guarded
/// close decided when a slot drained at generation `G` must be SKIPPED once any open
/// has bumped the slot's generation past `G`, EVEN IF the reaching-root set looks
/// empty again at the close-time gate. The folded `DeclOverlaySlot { generation,
/// roots }` makes `(generation, roots)` one critical section, so the gate can never
/// observe a half-applied pair and a transient drain-then-refill (or a drain whose
/// generation advanced) is recognised as a superseded close.
///
/// Two cases, both deterministic on the owner:
///   1. A reference REAPPEARED (set non-empty): skip.
///   2. The set is EMPTY again but the generation ADVANCED past the decision
///      baseline: skip — the ABA hole the bare set cannot catch.
///
/// In both, the provider receives NO `close_dts` (a destructive close would strand
/// the reappearing/just-opened root on TS2307). The faithful provider records every
/// close, so the assertion is exact. RED-before (the owner — folded slot,
/// `guarded_close`, the generation gate — does not exist on the pre-fix tree; the
/// pre-fix sibling generation map is read/written outside the slot's critical
/// section, the false-invariant ABA hole this folds shut). GREEN-after.
#[tokio::test(flavor = "multi_thread")]
async fn guarded_close_is_superseded_when_generation_advanced_even_if_set_looks_empty() {
    let decl_path = "/ws/Shared.d.vue.ts";

    // CASE 1: a reaching root reappeared after the close was decided.
    {
        let provider = Arc::new(MockTypeProvider::new());
        let sync = ProjectSync::new(provider.clone(), ProjectSyncMode::FullProject);
        let states: DashMap<String, crate::provider_sync::ProviderSyncState> = DashMap::new();
        let owner = DeclOverlayOwner::default();

        // The slot drained at generation 5 → close decided with baseline gen 5.
        owner.test_seed_slot(decl_path, &["/ws/R.vue"], 5);
        let decision = owner.release_root("/ws/R.vue");
        assert_eq!(decision.len(), 1);
        assert_eq!(
            decision[0].decl_path, decl_path,
            "the close decision targets the drained overlay"
        );
        assert_eq!(
            decision[0].close_generation, 5,
            "the close decision captures the slot's generation (5) at the drain"
        );

        // A racing open re-referenced the overlay (set now {S}) and bumped the
        // generation (6). (test_replace_slot mirrors what open_overlay records.)
        owner.test_replace_slot(decl_path, &["/ws/S.vue"], 6);

        owner.guarded_close(&sync, &states, &decision).await;

        let closed = provider
            .file_sync_calls()
            .iter()
            .any(|call| matches!(call, MockCall::CloseFile { path } if path == decl_path));
        assert!(
            !closed,
            "a close decided at gen 5 must be SKIPPED once a reference reappeared (set \
             {{S}}, gen 6) — closing would strand S on TS2307; calls={:?}",
            provider.file_sync_calls()
        );
        // The slot survives (the live reference is intact).
        assert_eq!(
            owner.test_slot_roots(decl_path),
            Some(["/ws/S.vue".to_string()].into_iter().collect()),
            "the superseded close leaves the re-referenced slot intact"
        );
    }

    // CASE 2 (the ABA the bare set cannot catch): the set is EMPTY again at the gate,
    // but the generation advanced past the decision baseline.
    {
        let provider = Arc::new(MockTypeProvider::new());
        let sync = ProjectSync::new(provider.clone(), ProjectSyncMode::FullProject);
        let states: DashMap<String, crate::provider_sync::ProviderSyncState> = DashMap::new();
        let owner = DeclOverlayOwner::default();

        owner.test_seed_slot(decl_path, &["/ws/R.vue"], 5);
        let decision = owner.release_root("/ws/R.vue");
        assert_eq!(decision.len(), 1);
        assert_eq!(decision[0].decl_path, decl_path);
        assert_eq!(decision[0].close_generation, 5);

        // ABA: a root opened (bump → 6) and closed again (set drains to empty), so the
        // set LOOKS like the decision saw it ({}), but the generation has advanced.
        owner.test_replace_slot(decl_path, &[], 6);
        assert_eq!(
            owner.test_slot_generation(decl_path),
            6,
            "the slot generation advanced to 6 despite the empty set"
        );

        owner.guarded_close(&sync, &states, &decision).await;

        let closed = provider
            .file_sync_calls()
            .iter()
            .any(|call| matches!(call, MockCall::CloseFile { path } if path == decl_path));
        assert!(
            !closed,
            "a close decided at gen 5 must be SKIPPED once the generation advanced to 6, \
             EVEN with an empty set (the ABA case the bare reaching-root set cannot \
             catch); calls={:?}",
            provider.file_sync_calls()
        );
    }

    // POSITIVE CONTROL: an un-superseded close (set empty, generation UNCHANGED from
    // the decision) DOES issue the provider close — the gate is not a blanket skip.
    {
        let provider = Arc::new(MockTypeProvider::new());
        let sync = ProjectSync::new(provider.clone(), ProjectSyncMode::FullProject);
        let states: DashMap<String, crate::provider_sync::ProviderSyncState> = DashMap::new();
        let owner = DeclOverlayOwner::default();

        owner.test_seed_slot(decl_path, &["/ws/R.vue"], 5);
        let decision = owner.release_root("/ws/R.vue");
        // No racing open: the tombstone stays empty at gen 5 (unchanged).
        owner.guarded_close(&sync, &states, &decision).await;

        let closed = provider
            .file_sync_calls()
            .iter()
            .any(|call| matches!(call, MockCall::CloseFile { path } if path == decl_path));
        assert!(
            closed,
            "an un-superseded close (empty set, generation unchanged) DOES close the \
             overlay (positive control); calls={:?}",
            provider.file_sync_calls()
        );
        // The confirmed close GC'd the slot.
        assert_eq!(
            owner.test_slot_roots(decl_path),
            None,
            "a confirmed close GCs the empty tombstone slot"
        );
    }
}

/// SUPERSESSION GATE (the replacement invariant for the deleted post-await repair):
/// a close decided when an overlay drained at generation `G` must be SKIPPED once a
/// reaching root REOPENS the overlay in the decision→close window (re-referencing it
/// and advancing the generation past `G`). The owner has NO compensate-after-close
/// re-open; instead the guarded close's supersession gate refuses the now-stale
/// close, so a live root's overlay is never clobbered (which would strand that root
/// on TS2307) and a drained overlay is never resurrected for a root that is no
/// longer there.
///
/// This drives the EXACT decision→reopen→gate window through the REAL owner methods
/// against the FAITHFUL gated provider — distinct from the unit-level
/// `guarded_close_is_superseded_*` test (which sets slot state via `test_replace_slot`):
/// here the reopen runs through the REAL closure pass / `open_overlay`, so a
/// regression in the open path's record-and-bump is also caught, and the assertion
/// is on the PROVIDER end-state (the overlay stays open), not merely "no close call".
///   * R primes `Shared.d.vue.ts` (`{Shared -> R}` at generation `G`, Shared open).
///   * R's close is DECIDED: `release_root(R)` drains Shared to an empty tombstone
///     and captures the close baseline generation `G`.
///   * S (a still-open root that also imports Shared) RE-OPENS `Shared.d.vue.ts`
///     through the REAL closure pass — re-recording `{Shared -> S}` and advancing the
///     generation to `G+1`, with Shared re-synced in the provider.
///   * R's STALE close (baseline `G`) is issued LAST: the gate sees a non-empty set
///     AND `G+1 != G`, so it is SUPERSEDED and SKIPPED.
///
/// ASSERT: R's superseded close issues NO provider `close_dts` for Shared, Shared
/// STAYS OPEN in the provider, and the owner records `{Shared -> S}` (the live root
/// keeps it). DISCRIMINATING: locally weakening the supersession gate (so the close
/// is never recognised as superseded) makes R's stale close fire — closing Shared in
/// the provider and dropping S's edge — so the "stays open" assertion FAILS. With
/// the gate intact it PASSES. The sibling
/// `guarded_decl_close_does_not_strand_concurrently_reopened_overlay` discriminates
/// the orthogonal defense (the per-path SERIALIZATION) under true close-vs-reopen
/// concurrency; this one discriminates the GENERATION/REACHABILITY gate.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn stale_close_is_superseded_when_a_reaching_root_reopens_the_overlay() {
    let temp_base_guard =
        tempfile::TempDir::with_prefix("verter_test_decl_close_no_resurrect").expect("temp dir");
    let temp_base = temp_base_guard.path().to_path_buf();
    let workspace = temp_base.join("workspace");
    std::fs::create_dir_all(workspace.join("src")).expect("create dirs");
    std::fs::write(
        workspace.join("tsconfig.json"),
        r#"{ "compilerOptions": { "baseUrl": "." } }"#,
    )
    .expect("write tsconfig");

    // Shared is a leaf carrier imported by BOTH R and S.
    std::fs::write(
        workspace.join("src/Shared.vue"),
        "<script setup lang=\"ts\">\ndefineProps<{ s: string }>()\n</script>\n<template><div/></template>",
    )
    .expect("write Shared.vue");
    let r_src = "<script setup lang=\"ts\">\nimport Shared from './Shared.vue'\ndefineProps<{ r: string }>()\n</script>\n<template><Shared/></template>";
    std::fs::write(workspace.join("src/R.vue"), r_src).expect("write R.vue");
    let s_src = "<script setup lang=\"ts\">\nimport Shared from './Shared.vue'\ndefineProps<{ q: string }>()\n</script>\n<template><Shared/></template>";
    std::fs::write(workspace.join("src/S.vue"), s_src).expect("write S.vue");

    let workspace_id_raw = std::fs::canonicalize(&workspace)
        .expect("canonical workspace")
        .to_string_lossy()
        .replace('\\', "/");
    let workspace_id = workspace_id_raw
        .strip_prefix("//?/")
        .unwrap_or(&workspace_id_raw)
        .to_string();

    let vfs_workspace_access: Arc<dyn verter_workspace::WorkspaceAccess> = Arc::new(
        verter_workspace::FilesystemWorkspace::new(verter_workspace::FilesystemOptions::default()),
    );
    let host = Arc::new(VerterHost::new(HostConfig::default(), vfs_workspace_access));

    let workspace_uri = crate::uri::path_to_file_uri_string(&workspace_id);
    let vite_opts = verter_workspace::ViteConfigOptions::default();
    let registry_ws =
        verter_workspace::FilesystemWorkspace::new(verter_workspace::FilesystemOptions::default());
    let build_result = crate::config::ProjectRegistry::from_workspace_roots(
        &registry_ws,
        &[workspace_uri],
        &vite_opts,
    );
    host.configure_projects(
        build_result
            .registry
            .projects()
            .iter()
            .map(|p| p.to_ide_project_config())
            .collect(),
    );
    let vfs_workspace = make_test_vfs_workspace_from_registry(&build_result.registry)
        .into_inner()
        .expect("the test VFS workspace publishes a snapshot");

    let provider = Arc::new(GatedDeclOverlayProvider::default());
    let type_provider: Arc<dyn TypeProvider> = provider.clone();
    let host_for_server = Arc::clone(&host);
    let type_provider_for_server = Arc::clone(&type_provider);
    let (service, _socket) = tower_lsp_server::LspService::new(move |_client| {
        VerterLanguageServer::new(
            crate::outbound::Outbound::default(),
            LspConfig {
                host: Arc::clone(&host_for_server),
                type_provider: Some(Arc::clone(&type_provider_for_server)),
                project_sync_mode: crate::ProjectSyncMode::FullProject,
                type_provider_kind: crate::TypeProviderKind::Tsgo,
                type_provider_topology: crate::TypeProviderTopology::ManagedTsgo,
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
    server.install_vfs_workspace(vfs_workspace);

    // Open R, then prime so Shared is recorded `{Shared -> R}` and open in provider.
    let r_uri = crate::uri::path_to_file_uri(&format!("{workspace_id}/src/R.vue")).expect("r uri");
    let _ = server.test_documents().did_open(&TextDocumentItem {
        uri: r_uri.clone(),
        language_id: "vue".to_string(),
        version: 1,
        text: r_src.to_string(),
    });
    let r_id = server
        .test_documents()
        .get_canonical_id(&r_uri)
        .expect("R.vue has a canonical id");
    let shared_id = format!("{workspace_id}/src/Shared.vue");
    let shared_decl_path = host
        .declaration_carrier_path(&shared_id)
        .expect("Shared.vue projects a declaration carrier path");
    assert!(
        shared_decl_path.contains("Shared.d.vue.ts"),
        "sanity: Shared's declaration overlay path is Shared.d.vue.ts, got {shared_decl_path}"
    );

    // PRIME with only R open: records `{Shared.d.vue.ts -> R}` at some generation G
    // and opens Shared in the provider.
    server.test_run_declaration_closure_pass(1).await;
    assert!(
        provider.open_paths_snapshot().contains(&shared_decl_path),
        "priming pass opened Shared.d.vue.ts in the provider, open_paths={:?}",
        provider.open_paths_snapshot()
    );
    let r_reaches_shared = server
        .test_decl_overlay_owner()
        .test_slot_roots(&shared_decl_path)
        .is_some_and(|roots| roots.iter().any(|r| r == &r_id));
    assert!(
        r_reaches_shared,
        "priming pass records Shared.d.vue.ts -> {{R}}, slots={:?}",
        server.test_decl_overlay_owner().test_slots_snapshot()
    );

    // Open S (imports Shared too) — a still-open root. Its edge is recorded by the
    // REAL reopen pass below, not yet.
    let s_uri = crate::uri::path_to_file_uri(&format!("{workspace_id}/src/S.vue")).expect("s uri");
    let _ = server.test_documents().did_open(&TextDocumentItem {
        uri: s_uri.clone(),
        language_id: "vue".to_string(),
        version: 1,
        text: s_src.to_string(),
    });
    let s_id = server
        .test_documents()
        .get_canonical_id(&s_uri)
        .expect("S.vue has a canonical id");

    let owner = server.test_decl_overlay_owner();

    // STEP 1 — R's close DECISION: drain R from Shared. With S not yet recorded the
    // slot drains to an empty tombstone, and the returned target carries the close
    // baseline generation G (captured under the slot lock at the drain).
    let decision = owner.release_root(&r_id);
    let shared_decision: Vec<&DeclCloseTarget> = decision
        .iter()
        .filter(|t| t.decl_path == shared_decl_path)
        .collect();
    assert_eq!(
        shared_decision.len(),
        1,
        "releasing R decides a close for Shared.d.vue.ts, decision={decision:?}"
    );
    let baseline_gen = shared_decision[0].close_generation;

    // STEP 2 — S RE-OPENS Shared through the REAL closure pass BEFORE R's stale close
    // is issued: `open_overlay` re-records `{Shared -> S}`, advances the generation
    // past the baseline, and re-syncs Shared in the provider (the decision→close
    // window the gate must defend).
    server.test_run_declaration_closure_pass(1).await;
    assert!(
        owner
            .test_slot_roots(&shared_decl_path)
            .is_some_and(|roots| roots.iter().any(|r| r == &s_id)),
        "S's reopen pass re-records Shared.d.vue.ts -> {{S}}, slots={:?}",
        owner.test_slots_snapshot()
    );
    assert!(
        owner.test_slot_generation(&shared_decl_path) > baseline_gen,
        "S's reopen advances the generation past the close baseline ({baseline_gen}), \
         got {}",
        owner.test_slot_generation(&shared_decl_path)
    );

    // STEP 3 — R's STALE close is issued LAST with the baseline-G decision. The
    // guarded close's gate sees a non-empty set ({S}) AND the advanced generation, so
    // it is SUPERSEDED and must NOT touch the provider.
    let Some(sync) = server.project_sync.as_ref() else {
        panic!("the tsgo server has a project sync");
    };
    owner
        .guarded_close(sync, &server.provider_sync_states, &decision)
        .await;

    // INVARIANT 1 (gate skip): R's superseded close issued NO provider close for
    // Shared. RED if the gate is weakened (the stale close fires and closes Shared).
    let shared_closed = provider
        .calls()
        .iter()
        .any(|c| matches!(c, MockCall::CloseFile { path } if path == &shared_decl_path));
    assert!(
        !shared_closed,
        "R's stale close (baseline gen {baseline_gen}) must be SUPERSEDED by S's reopen \
         and issue NO provider close for Shared.d.vue.ts; calls={:?}",
        provider.calls()
    );

    // INVARIANT 2 (no clobber): Shared.d.vue.ts STAYS OPEN in the provider, so S's
    // bare `import Shared from "./Shared.vue"` still resolves (no TS2307), and the
    // owner still records the live root S reaching it.
    assert!(
        provider.open_paths_snapshot().contains(&shared_decl_path),
        "Shared.d.vue.ts must STAY OPEN after R's superseded close (S reaches it); \
         open_paths={:?}, calls={:?}",
        provider.open_paths_snapshot(),
        provider.calls()
    );
    assert!(
        owner
            .test_slot_roots(&shared_decl_path)
            .is_some_and(|roots| roots.iter().any(|r| r == &s_id)),
        "the superseded close leaves S's edge to Shared.d.vue.ts intact, slots={:?}",
        owner.test_slots_snapshot()
    );

    drain.abort();
    let _ = std::fs::remove_dir_all(&temp_base);
}

/// A successful direct (tsgo) IDE sync must record a `CarrierIde` surface at
/// the IDE provider path: the exact bytes delivered to the provider, plus the
/// carrier source they were compiled from. Without this producer, interactive
/// queries have no consistent surface to capture.
#[tokio::test(flavor = "multi_thread")]
async fn successful_direct_ide_sync_records_carrier_ide_surface() {
    let (service, _provider, uri) = make_request_surface_carrier().await;
    let server = service.inner();

    let ide_path = server
        .active_ide_path_for_uri(&uri)
        .expect("the direct IDE sync must commit a live IDE path");
    let snapshot = server
        .documents
        .provider_surfaces()
        .current_snapshot(&ide_path)
        .expect("a successful direct IDE sync must record a CarrierIde surface");
    assert_eq!(
        snapshot.kind,
        crate::provider_surface_store::ProviderSurfaceKind::CarrierIde,
        "the recorded surface must carry the CarrierIde role"
    );
    let delivered = server
        .project_sync
        .as_ref()
        .and_then(|sync| sync.synced_tsx_content(&ide_path))
        .expect("the direct sync records the projected provider bytes");
    assert_eq!(
        snapshot.provider_content.as_ref(),
        delivered.as_ref(),
        "the recorded surface must pin the EXACT bytes synced to the provider"
    );
    assert!(delivered.contains("from \"./App.vue.tsx.__verter_types\""));
    assert!(!delivered.contains("from \"@verter/types\""));
    let doc = server.documents.get(&uri).expect("document is open");
    assert_eq!(
        snapshot.carrier_source.as_ref(),
        doc.source.as_ref(),
        "the recorded surface must pin the carrier source the bytes were compiled from"
    );
    assert!(
        snapshot.source_map.is_some(),
        "the recorded CarrierIde surface must carry its source map"
    );
}

/// A FAILED direct IDE sync must not admit a queryable managed-tsgo surface.
/// The editor TypeScript consumer may already have a valid independently-
/// published store generation, but managed tsgo cannot commit or query through
/// it until its own direct buffer open succeeds.
#[tokio::test(flavor = "multi_thread")]
async fn failed_direct_ide_sync_records_no_carrier_ide_surface() {
    let provider = Arc::new(MockTypeProvider::new());
    let type_provider: Arc<dyn TypeProvider> = provider.clone();
    let service = make_hover_test_service_tsgo(type_provider);
    let server = service.inner();
    install_test_resolver(server);
    let uri = open_test_vue(server, "/workspace/src/App.vue", REQUEST_SURFACE_APP);

    let target_ide_path = server
        .target_ide_path_for_uri(&uri)
        .expect("owner-resolved carrier has a target IDE path");
    provider.set_fail_sync_path(&target_ide_path);

    server.sync_ide_to_provider(&uri).await;

    let editor_surface = server
        .documents
        .provider_surfaces()
        .current_snapshot(&target_ide_path)
        .expect("the independent editor-membership publish remains valid");
    assert_eq!(
        editor_surface.kind,
        crate::provider_surface_store::ProviderSurfaceKind::CarrierIde
    );
    assert!(
        provider.file_sync_calls().iter().any(|call| matches!(
            call,
            MockCall::UpdateFile { path, .. } if path == &target_ide_path
        )),
        "the managed-tsgo direct IDE sync must have been attempted"
    );
    assert!(
        server
            .provider_sync_state_for_source("/workspace/src/App.vue")
            .is_none(),
        "a failed direct IDE sync must not commit provider state"
    );
    assert!(
        server.active_ide_path_for_uri(&uri).is_none(),
        "a failed direct IDE sync must not expose a live IDE path"
    );
    assert!(
        server.capture_provider_request_surface(&uri).is_none(),
        "a failed direct IDE sync must not expose the editor-only surface to managed-tsgo queries"
    );
}

/// A successful self-file (rune module) shadow sync records a `Shadow` surface
/// at the module's OWN canonical path — the provider buffer bytes plus the
/// rewrite-aware mapper; a failed shadow sync records nothing.
#[tokio::test]
async fn self_file_shadow_sync_records_shadow_surface_on_success_only() {
    let provider = Arc::new(MockTypeProvider::new());
    let type_provider: Arc<dyn TypeProvider> = provider.clone();
    let service = make_hover_test_service(type_provider);
    let server = service.inner();

    let rune_uri: Uri = "file:///workspace/store.svelte.ts".parse().unwrap();
    let _ = server.documents.did_open(&TextDocumentItem {
        uri: rune_uri.clone(),
        language_id: "typescript".to_string(),
        version: 1,
        text: "export const s = $state(0);\n".to_string(),
    });
    assert!(
        server.sync_self_file_shadow_unresolved(&rune_uri).await,
        "the shadow sync should succeed against the mock provider"
    );
    let snapshot = server
        .documents
        .provider_surfaces()
        .current_snapshot("/workspace/store.svelte.ts")
        .expect("a successful shadow sync must record a Shadow surface");
    assert_eq!(
        snapshot.kind,
        crate::provider_surface_store::ProviderSurfaceKind::Shadow,
        "the recorded surface must carry the Shadow role"
    );
    assert!(
        snapshot.provider_content.contains("$state"),
        "the recorded surface must pin the provider buffer bytes"
    );
    assert!(
        snapshot.source_map.is_some(),
        "the recorded Shadow surface must carry the rewrite-aware mapper"
    );
    // The Shadow surface must stamp a REAL map identity derived from its
    // rewrite-aware mapper — a zeroed identity would let a mapper-changing
    // re-sync pass the byte-match honored gate and map through a stale mapper.
    assert_ne!(
        snapshot.stamp.map_hash, [0u8; 16],
        "the recorded Shadow surface must stamp a real (mapper-derived) map identity"
    );
    // Identity is deterministic: an identical re-sync stamps the SAME identity
    // (so a byte-identical re-record stays honored, no over-drop).
    assert!(
        server.sync_self_file_shadow_unresolved(&rune_uri).await,
        "the identical shadow re-sync should succeed"
    );
    let resynced = server
        .documents
        .provider_surfaces()
        .current_snapshot("/workspace/store.svelte.ts")
        .expect("the re-sync must record a fresh Shadow generation");
    assert_ne!(
        resynced.stamp.generation, snapshot.stamp.generation,
        "the re-sync must mint a fresh generation"
    );
    assert_eq!(
        resynced.stamp.map_hash, snapshot.stamp.map_hash,
        "an identical shadow re-sync must stamp the SAME map identity"
    );

    // A FAILED shadow sync records nothing.
    let other_uri: Uri = "file:///workspace/other.svelte.ts".parse().unwrap();
    let _ = server.documents.did_open(&TextDocumentItem {
        uri: other_uri.clone(),
        language_id: "typescript".to_string(),
        version: 1,
        text: "export const t = $state(1);\n".to_string(),
    });
    provider.set_fail_sync_path("/workspace/other.svelte.ts");
    assert!(
        !server.sync_self_file_shadow_unresolved(&other_uri).await,
        "the shadow sync should fail for the failure-injected path"
    );
    assert!(
        server
            .documents
            .provider_surfaces()
            .current_snapshot("/workspace/other.svelte.ts")
            .is_none(),
        "a failed shadow sync must not record a provider surface"
    );
}

/// The interactive query context must serve the RECORDED provider surface —
/// never a torn pair of the committed provider path with fresher live-compiled
/// content the provider has not received. After an edit that has NOT been
/// re-synced, the context either fails closed (`None`) or still serves the
/// recorded surface byte-exactly.
#[tokio::test]
async fn repaired_provider_context_flushes_an_edited_self_file_immediately() {
    let provider = Arc::new(MockTypeProvider::new());
    let type_provider: Arc<dyn TypeProvider> = provider.clone();
    let service = make_hover_test_service(type_provider);
    let server = service.inner();

    let uri: Uri = "file:///workspace/store.svelte.ts".parse().unwrap();
    let _ = server.documents.did_open(&TextDocumentItem {
        uri: uri.clone(),
        language_id: "typescript".to_string(),
        version: 1,
        text: "export const count = $state(0);\n".to_string(),
    });
    assert!(server.sync_self_file_shadow_unresolved(&uri).await);

    let _ = server.documents.did_change(
        &uri,
        2,
        "export const count = $state(1);\nexport const fresh = true;\n",
    );
    assert!(
        server.type_provider_context(&uri).is_none(),
        "the edited document must invalidate the old provider surface"
    );

    let repaired = server
        .repaired_type_provider_context(&uri)
        .await
        .expect("an interactive request must flush the current self-file before querying");
    assert_eq!(repaired.tsx_path, "/workspace/store.svelte.ts");
    assert!(
        repaired.tsx_content.contains("export const fresh = true"),
        "the repaired request surface must contain the latest authored bytes"
    );
    assert!(provider.file_sync_calls().iter().any(|call| matches!(
        call,
        MockCall::UpdateFile { path, content }
            if path == "/workspace/store.svelte.ts" && content.contains("fresh")
    )));
}

#[tokio::test(flavor = "multi_thread")]
async fn provider_query_context_serves_recorded_surface_never_torn_live_pair() {
    let (service, _provider, uri) = make_request_surface_carrier().await;
    let server = service.inner();
    let store = server.documents.provider_surfaces().clone();

    // Stable state: the context serves the recorded surface (anti-over-drop).
    let ctx = server
        .type_provider_context(&uri)
        .expect("a synced, unedited carrier must yield a provider query context");
    let recorded = store
        .current_snapshot(&ctx.tsx_path)
        .expect("the context's provider path must resolve to a recorded surface");
    assert_eq!(
        ctx.tsx_content.as_ref(),
        recorded.provider_content.as_ref(),
        "the stable-state context must serve the recorded surface content"
    );

    // Edit WITHOUT a re-sync: the live artifacts advance while the provider
    // still holds the previously-synced surface.
    let _ = server.documents.did_change(
        &uri,
        2,
        r#"<script setup lang="ts">
const msg = 'hello'
const extra = 'drift'
</script>
<template><div>{{ msg }}{{ extra }}</div></template>
"#,
    );

    match server.type_provider_context(&uri) {
        // Fail closed: no consistent captured surface for the edited document.
        None => {}
        Some(ctx) => {
            let snapshot = store
                .current_snapshot(&ctx.tsx_path)
                .expect("a served query context must be backed by a recorded provider surface");
            assert_eq!(
                ctx.tsx_content.as_ref(),
                snapshot.provider_content.as_ref(),
                "the query context must serve the recorded provider surface content, \
                 never live-compiled bytes the provider has not received"
            );
            assert!(
                !ctx.tsx_content.contains("extra"),
                "the un-synced edit must not leak into the provider query context"
            );
        }
    }
}

/// The OPEN-UNRESOLVED drain path (owner not yet resolved, editor-liveness IDE
/// open) must ALSO record the `CarrierIde` surface it delivers: the unresolved
/// carrier is queryable (hover/completion work through the unresolved IDE
/// path), so the request-surface capture needs a recorded surface here too.
#[tokio::test(flavor = "multi_thread")]
async fn open_unresolved_drain_ide_sync_records_carrier_ide_surface() {
    let host = Arc::new(VerterHost::new_standalone(HostConfig::default()));
    let documents = DocumentRegistry::new(Arc::clone(&host));
    let uri: Uri = "file:///workspace/src/App.vue".parse().unwrap();
    let source = "<script setup lang=\"ts\">\nconst msg = 'hello'\n</script>\n\
                  <template><div>{{ msg }}</div></template>\n";
    let _ = documents.did_open(&TextDocumentItem {
        uri: uri.clone(),
        language_id: "vue".to_string(),
        version: 1,
        text: source.to_string(),
    });
    let profile = documents.tsx_profile.read().clone();
    let ide = host
        .get_ide("/workspace/src/App.vue", &profile)
        .expect("IDE output should exist for the open carrier");

    let provider = Arc::new(MockTypeProvider::new());
    let sync = ProjectSync::new(provider.clone(), ProjectSyncMode::FullProject);
    let provider_sync_states = DashMap::new();

    let app_revision = documents
        .snapshot_identity(&uri)
        .expect("the open document has a live identity to pin");
    let synced = sync_open_unresolved_carrier_provider_file(
        &sync,
        &documents,
        &provider_sync_states,
        "/workspace/src/App.vue",
        false,
        Some(&ide),
        Some((&uri, &app_revision)),
        &crate::external_ts::CarrierTransactionCoordinator::new(),
    )
    .await;
    assert!(
        !synced,
        "the unresolved preserve pass keeps the file queued (returns false)"
    );

    let state = provider_sync_states
        .get("/workspace/src/App.vue")
        .map(|entry| entry.clone())
        .expect("the open unresolved carrier must commit provider state");
    let ide_path = state
        .ide_path
        .clone()
        .expect("the unresolved carrier must commit a live IDE path");
    let snapshot = documents
        .provider_surfaces()
        .current_snapshot(&ide_path)
        .expect("a successful open-unresolved IDE sync must record a CarrierIde surface");
    assert_eq!(
        snapshot.kind,
        crate::provider_surface_store::ProviderSurfaceKind::CarrierIde,
        "the recorded surface must carry the CarrierIde role"
    );
    assert_eq!(
        snapshot.provider_content.as_ref(),
        sync.synced_tsx_content(&ide_path)
            .expect("the unresolved sync records the projected provider bytes")
            .as_ref(),
        "the recorded surface must pin the EXACT bytes delivered to the provider"
    );
    let delivered = sync
        .synced_tsx_content(&ide_path)
        .expect("the unresolved sync records the projected provider bytes");
    assert!(delivered.contains("from \"./App.vue.tsx.__verter_types\""));
    assert!(!delivered.contains("from \"@verter/types\""));

    // A FAILED open-unresolved IDE sync records NOTHING (fail closed).
    let other_uri: Uri = "file:///workspace/src/Other.vue".parse().unwrap();
    let _ = documents.did_open(&TextDocumentItem {
        uri: other_uri.clone(),
        language_id: "vue".to_string(),
        version: 1,
        text: source.to_string(),
    });
    let other_ide = host
        .get_ide("/workspace/src/Other.vue", &profile)
        .expect("IDE output should exist for the second carrier");
    provider.set_fail_sync_path("/workspace/src/Other.vue.tsx");
    let other_revision = documents
        .snapshot_identity(&other_uri)
        .expect("the open document has a live identity to pin");
    let _ = sync_open_unresolved_carrier_provider_file(
        &sync,
        &documents,
        &provider_sync_states,
        "/workspace/src/Other.vue",
        false,
        Some(&other_ide),
        Some((&other_uri, &other_revision)),
        &crate::external_ts::CarrierTransactionCoordinator::new(),
    )
    .await;
    assert!(
        documents
            .provider_surfaces()
            .current_snapshot("/workspace/src/Other.vue.tsx")
            .is_none(),
        "a failed open-unresolved IDE sync must not record a provider surface"
    );
}

/// The PRODUCTION `did_close` path must retire the carrier's `CarrierIde`
/// surface from the store, not only close the provider buffer. Without the
/// retire, reopening the same text BEFORE any successful re-sync lets the
/// interactive capture resolve the stale surface (source bytes match) and
/// serve a query against a provider buffer that is CLOSED.
#[tokio::test(flavor = "multi_thread")]
async fn did_close_retires_carrier_ide_surface_so_reopen_fails_closed_until_resync() {
    let (service, provider, uri) = make_request_surface_carrier().await;
    let server = service.inner();

    let ide_path = server.active_ide_path_for_uri(&uri).expect("live IDE path");
    assert!(
        server
            .documents
            .provider_surfaces()
            .current_snapshot(&ide_path)
            .is_some(),
        "precondition: the synced carrier has a recorded CarrierIde surface"
    );

    // PRODUCTION close: the did_close handler closes the tsgo IDE buffer.
    server
        .did_close(DidCloseTextDocumentParams {
            text_document: TextDocumentIdentifier { uri: uri.clone() },
        })
        .await;
    assert!(
        provider
            .file_sync_calls()
            .iter()
            .any(|call| matches!(call, MockCall::CloseFile { path } if path == &ide_path)),
        "the did_close handler must close the tsgo IDE buffer"
    );

    // Reopen the SAME text; no successful re-sync has happened yet.
    let _ = server.documents.did_open(&TextDocumentItem {
        uri: uri.clone(),
        language_id: "vue".to_string(),
        version: 2,
        text: REQUEST_SURFACE_APP.to_string(),
    });

    // The closed surface must NOT be capturable: the provider buffer is closed,
    // so serving a query against the stale snapshot would be torn.
    assert!(
        server
            .documents
            .provider_surfaces()
            .current_snapshot(&ide_path)
            .is_none(),
        "did_close must retire the CarrierIde surface's active generation"
    );
    assert!(
        server.capture_provider_request_surface(&uri).is_none(),
        "a reopened, not-yet-resynced carrier must fail closed at capture — the \
         provider no longer holds the closed IDE buffer"
    );

    // A fresh successful sync restores capture (no permanent over-drop).
    server.sync_ide_to_provider(&uri).await;
    assert!(
        server.capture_provider_request_surface(&uri).is_some(),
        "a successful re-sync must restore the capturable surface"
    );
}

/// The PRODUCTION self-file `did_close` path must retire the rune module's
/// `Shadow` surface: the SelfFile capture arm resolves by canonical path with
/// no committed-state gate, so a lingering `Current` Shadow surface would be
/// captured for a reopened module whose provider buffer is CLOSED.
#[tokio::test(flavor = "multi_thread")]
async fn did_close_retires_shadow_surface_so_reopen_fails_closed_until_resync() {
    let provider = Arc::new(MockTypeProvider::new());
    let type_provider: Arc<dyn TypeProvider> = provider.clone();
    let service = make_hover_test_service(type_provider);
    let server = service.inner();

    let rune_uri: Uri = "file:///workspace/store.svelte.ts".parse().unwrap();
    let rune_text = "export const s = $state(0);\n";
    let _ = server.documents.did_open(&TextDocumentItem {
        uri: rune_uri.clone(),
        language_id: "typescript".to_string(),
        version: 1,
        text: rune_text.to_string(),
    });
    assert!(
        server.sync_self_file_shadow_unresolved(&rune_uri).await,
        "the shadow sync should succeed"
    );
    assert!(
        server
            .documents
            .provider_surfaces()
            .current_snapshot("/workspace/store.svelte.ts")
            .is_some(),
        "precondition: the synced rune module has a recorded Shadow surface"
    );

    // PRODUCTION close: the did_close handler clears the self-file state and
    // closes the own-path provider buffer.
    server
        .did_close(DidCloseTextDocumentParams {
            text_document: TextDocumentIdentifier {
                uri: rune_uri.clone(),
            },
        })
        .await;

    // Reopen the SAME text; no successful re-sync has happened yet.
    let _ = server.documents.did_open(&TextDocumentItem {
        uri: rune_uri.clone(),
        language_id: "typescript".to_string(),
        version: 2,
        text: rune_text.to_string(),
    });

    assert!(
        server
            .documents
            .provider_surfaces()
            .current_snapshot("/workspace/store.svelte.ts")
            .is_none(),
        "did_close must retire the Shadow surface's active generation"
    );
    assert!(
        server.capture_provider_request_surface(&rune_uri).is_none(),
        "a reopened, not-yet-resynced rune module must fail closed at capture — \
         the provider no longer holds the closed shadow buffer"
    );
}

/// A STALE virtual tab (its bytes hold a generation the provider no longer
/// serves) must FAIL CLOSED at context capture: offsets computed against the
/// drifted virtual buffer would index content the provider does not hold, so
/// no provider query may be issued and no provider result served.
#[tokio::test(flavor = "multi_thread")]
async fn virtual_file_query_fails_closed_when_virtual_buffer_is_stale() {
    // The recorded (provider-held) surface DIFFERS from the virtual tab bytes.
    let recorded = "const fresh = 1;\ncomputed\n";
    let stale_virtual = "computed\n";
    let (service, provider, virtual_uri, tsx_path) =
        make_virtual_file_fixture(recorded, stale_virtual).await;
    let server = service.inner();

    // A hover the PRE-GATE path would have served: keyed at the offset the
    // stale virtual buffer computes.
    let stale_offset = "computed".len() as u32;
    provider.set_hover(
        &tsx_path,
        stale_offset,
        Some(crate::type_provider::protocol::HoverInfo {
            contents: "VIRTUAL_HOVER_SENTINEL".to_string(),
            display_signature: Some(crate::type_provider::mock::test_display_signature(
                "VIRTUAL_HOVER_SENTINEL",
            )),
            ..Default::default()
        }),
    );

    assert!(
        server.virtual_file_context(&virtual_uri).is_none(),
        "a virtual tab whose bytes do not match the captured surface must fail \
         closed at context capture"
    );

    let hover = server
        .hover(hover_params(
            &virtual_uri,
            Position {
                line: 0,
                character: stale_offset,
            },
        ))
        .await
        .expect("hover request should succeed");
    let text = hover
        .map(|h| match h.contents {
            HoverContents::Markup(m) => m.value,
            _ => String::new(),
        })
        .unwrap_or_default();
    assert!(
        !text.contains("VIRTUAL_HOVER_SENTINEL"),
        "a stale virtual tab must not serve a provider hover computed against \
         drifted bytes, got: {text}"
    );
}

/// A provider re-sync advancing the surface generation while a virtual-file
/// query is awaiting the provider must cause the provider result to be
/// DROPPED (fail closed): the response was produced against a surface the
/// virtual tab no longer matches.
#[tokio::test(flavor = "multi_thread")]
async fn virtual_file_query_drops_provider_result_when_surface_advances_mid_request() {
    let content = "computed\n";
    let (service, provider, virtual_uri, tsx_path) =
        make_virtual_file_fixture(content, content).await;
    let server = service.inner();

    let offset = "computed".len() as u32;
    provider.set_hover(
        &tsx_path,
        offset,
        Some(crate::type_provider::protocol::HoverInfo {
            contents: "VIRTUAL_HOVER_SENTINEL".to_string(),
            display_signature: Some(crate::type_provider::mock::test_display_signature(
                "VIRTUAL_HOVER_SENTINEL",
            )),
            ..Default::default()
        }),
    );

    // Mid-request seam: a concurrent re-sync lands a NEW generation with
    // drifted content between the capture and the response handling.
    let store = server.documents.provider_surfaces().clone();
    let raced_path = tsx_path.clone();
    provider.set_on_query(
        &tsx_path,
        Box::new(move || {
            store.record(
                crate::provider_surface_store::RecordSurface::carrier_legacy(
                    crate::provider_surface_store::ProviderSurfaceKind::CarrierIde,
                    raced_path,
                    "/workspace/src/App.vue".to_string(),
                    Arc::from("// drifted ide content"),
                    None,
                    Arc::from("// drifted carrier source"),
                ),
            );
        }),
    );

    let hover = server
        .hover(hover_params(
            &virtual_uri,
            Position {
                line: 0,
                character: offset,
            },
        ))
        .await
        .expect("hover request should succeed");
    let text = hover
        .map(|h| match h.contents {
            HoverContents::Markup(m) => m.value,
            _ => String::new(),
        })
        .unwrap_or_default();
    assert!(
        !text.contains("VIRTUAL_HOVER_SENTINEL"),
        "a virtual-file hover produced against a superseded surface generation \
         must be DROPPED, got: {text}"
    );
}

/// With a byte-matched virtual tab and a stable surface, the virtual-file
/// branch DOES serve the provider result — guards against an over-eager
/// fail-closed gate breaking healthy virtual-file queries.
#[tokio::test(flavor = "multi_thread")]
async fn virtual_file_query_serves_provider_result_from_stable_matched_surface() {
    let content = "computed\n";
    let (service, provider, virtual_uri, tsx_path) =
        make_virtual_file_fixture(content, content).await;
    let server = service.inner();

    let offset = "computed".len() as u32;
    provider.set_hover(
        &tsx_path,
        offset,
        Some(crate::type_provider::protocol::HoverInfo {
            contents: "VIRTUAL_HOVER_SENTINEL".to_string(),
            display_signature: Some(crate::type_provider::mock::test_display_signature(
                "VIRTUAL_HOVER_SENTINEL",
            )),
            ..Default::default()
        }),
    );

    let text = hover_text(
        server
            .hover(hover_params(
                &virtual_uri,
                Position {
                    line: 0,
                    character: offset,
                },
            ))
            .await
            .expect("hover request should succeed"),
    );
    assert!(
        text.contains("VIRTUAL_HOVER_SENTINEL"),
        "a byte-matched virtual tab over a stable surface must serve the provider \
         hover, got: {text}"
    );
}

/// A carrier sync that FAILED inside the background publication must leave
/// DependencyReady COLD (not Ready), so the next request's readiness miss
/// enqueues a fresh publication that RETRIES that carrier.
///
/// Minting the receipt over a failed leg would warm-capture past the failure
/// until an unrelated edit bumps the generation: the failure arms only queue
/// `pending_snapshot_provider_sync`, whose sole drain is background init, so a
/// transient provider error would strand the carrier for the rest of the
/// session.
#[tokio::test]
async fn a_failed_carrier_sync_leaves_dependency_readiness_cold_and_retries() {
    let child_source = "<script setup lang=\"ts\">\nconst emit = defineEmits<{ custom: [payload: string] }>()\n</script>\n";
    let parent_source = "<script setup lang=\"ts\">\nimport MyComp from './MyComp.vue'\nfunction handleCustom(payload: string) {}\n</script>\n<template>\n  <MyComp @custom=\"handleCustom\" />\n</template>\n";
    let (_temp, service, drain_handle, provider, workspace_id) =
        make_definition_test_server_with_kind(
            &[
                ("src/MyComp.vue", "vue", child_source),
                ("src/App.vue", "vue", parent_source),
            ],
            crate::TypeProviderKind::Tsgo,
        )
        .await;

    // Fail ONLY the imported child's IDE companion; every other sync succeeds, so
    // the publication reaches its mint point with exactly one failed leg.
    let child_id = format!("{workspace_id}/src/MyComp.vue");
    let child_ide_path =
        verter_session_query::resolution::carrier_ide_provider_path(&child_id, false);
    provider.set_fail_sync_path(&child_ide_path);

    let app_uri = workspace_uri(&workspace_id, "src/App.vue");
    let server = service.inner();
    let position = find_document_position(server, &app_uri, "@custom=\"handleCustom\"", 1);

    let child_sync_attempts = |calls: Vec<MockCall>| {
        calls
            .into_iter()
            .filter(|call| match call {
                MockCall::OpenFile { path, .. }
                | MockCall::UpdateFile { path, .. }
                | MockCall::LoadFile { path, .. } => path == &child_ide_path,
                _ => false,
            })
            .count()
    };

    let _ = server
        .goto_definition(goto_definition_params(&app_uri, position))
        .await;

    // Reach assertion: the enqueued publication must ACTUALLY attempt the
    // failing sync, otherwise the retry assertion below would be vacuous.
    tokio::time::timeout(std::time::Duration::from_secs(5), async {
        provider
            .wait_until_calls(|calls| child_sync_attempts(calls.to_vec()) > 0)
            .await;
    })
    .await
    .expect(
        "the first request's enqueued publication must actually attempt the child's \
         IDE companion sync, else this test proves nothing",
    );
    let before = child_sync_attempts(provider.calls());
    assert_eq!(
        server.import_sync.recorded_len(),
        0,
        "a publication with a FAILED leg must not mint DependencyReady"
    );

    // The next request misses (no receipt), enqueues a fresh publication, and
    // that publication RETRIES the failed carrier.
    let _ = server
        .goto_definition(goto_definition_params(&app_uri, position))
        .await;

    tokio::time::timeout(std::time::Duration::from_secs(5), async {
        provider
            .wait_until_calls(|calls| child_sync_attempts(calls.to_vec()) > before)
            .await;
    })
    .await
    .expect(
        "a FAILED carrier sync must leave DependencyReady cold so the next request's \
         publication retries it",
    );
    assert_eq!(
        server.import_sync.recorded_len(),
        0,
        "the retrying publication (still failing) must still not mint DependencyReady"
    );

    drain_handle.abort();
    drop(service);
}

/// Two different documents must not queue behind one another. The per-document
/// singleflight that backs the import-set memo is keyed per canonical id; if it
/// ever collapsed to one shared lock it would serialize the whole editor —
/// exactly the kind of stability machinery that costs more than it saves.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn the_import_set_singleflight_does_not_serialize_across_documents() {
    let memo = crate::server::ImportSyncMemo::default();

    let first = memo.lock_for("/workspace/src/A.vue");
    let second = memo.lock_for("/workspace/src/B.vue");
    let same_as_first = memo.lock_for("/workspace/src/A.vue");

    assert!(
        !Arc::ptr_eq(&first, &second),
        "different documents must get different singleflight locks, or every \
         document serializes behind every other"
    );
    assert!(
        Arc::ptr_eq(&first, &same_as_first),
        "the same document must reuse one lock, or the singleflight coalesces nothing"
    );

    // Hold A's lock for the whole test; B must still be able to run.
    let _held = first.lock().await;

    let progressed = tokio::time::timeout(std::time::Duration::from_secs(2), second.lock()).await;
    assert!(
        progressed.is_ok(),
        "a document whose sibling holds its own singleflight lock must proceed \
         immediately, not wait behind it"
    );

    // And the same document genuinely does coalesce onto the held lock.
    let contended =
        tokio::time::timeout(std::time::Duration::from_millis(200), same_as_first.lock()).await;
    assert!(
        contended.is_err(),
        "the same document must coalesce onto the held lock, else the \
         singleflight is not a singleflight"
    );
}

/// A request racing an in-flight `did_open` must resume the instant the
/// document registers, not at the end of a polling step. The poll schedule this
/// replaced started at 20ms and coarsened to 80ms, so a document that landed
/// 1ms after a check still held the request for the rest of that interval —
/// latency the request had already earned the right not to pay.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_request_racing_an_open_resumes_the_moment_the_document_registers() {
    let provider = Arc::new(MockTypeProvider::new());
    let type_provider: Arc<dyn TypeProvider> = provider.clone();
    let service = make_hover_test_service_tsgo(type_provider);
    let server = service.inner();

    let uri: Uri = "file:///workspace/src/Late.vue".parse().unwrap();
    let source = "<script setup lang=\"ts\">\nconst count = 1\n</script>\n<template><div>{{ count }}</div></template>\n";

    // The fail-safe budget. Event-driven, the wait returns as soon as the
    // registration lands; it must never approach this ceiling. The negative
    // control — suppressing the registration signal — makes the wait fall
    // through to exactly this budget, so a bound comfortably below it
    // discriminates the event-driven wake from a fall-through while staying
    // robust to scheduler noise under a saturated test runner.
    const BUDGET: std::time::Duration = std::time::Duration::from_millis(300);

    let waiter = tokio::spawn({
        let server = server.clone();
        let uri = uri.clone();
        async move {
            server
                .documents
                .registration
                .wait_until(BUDGET, || server.documents.get(&uri).is_some())
                .await
        }
    });

    // Wait until the waiter has genuinely entered the notify/timeout race
    // (armed interest) before opening the document. The signal publishes an
    // arming receipt, so this is an exact wait, not a yield loop and not a
    // timing guess. Without it, a fixed delay before opening only makes the
    // race LIKELY, not certain: under scheduler contention the waiter could
    // still be scheduled after the open lands and silently take the
    // "already present" fast path instead of the race this test is named
    // for and exists to prove.
    server
        .documents
        .registration
        .wait_until_timeout_armed()
        .await;
    assert_eq!(
        server.documents.registration.timeout_arm_count(),
        1,
        "the waiter must have genuinely armed the notify/timeout race before \
         the document opens, or this test silently exercises the \
         already-present fast path instead of the race it is named for"
    );

    open_test_vue(server, "/workspace/src/Late.vue", source);

    let registered = waiter.await.unwrap();

    assert!(
        registered,
        "the wait must observe the registration it was waiting for"
    );
    // Structural proof, not a wall-clock one: the request must resume via the
    // registration signal, never by falling through to the {BUDGET:?}
    // fail-safe budget. A margin comparison against elapsed time narrows to
    // nothing under machine load; this counter is exact regardless of
    // scheduling.
    assert_eq!(
        server.documents.registration.budget_exhausted_count(),
        0,
        "the request fell through to the {BUDGET:?} fail-safe budget rather than waking \
         on the registration"
    );
}

/// A document that is already registered must not wait at all. The wait is for
/// a race that usually did not happen; the common case has to be free.
#[tokio::test]
async fn waiting_for_an_already_registered_document_returns_immediately() {
    let provider = Arc::new(MockTypeProvider::new());
    let type_provider: Arc<dyn TypeProvider> = provider.clone();
    let service = make_hover_test_service_tsgo(type_provider);
    let server = service.inner();

    let source = "<script setup lang=\"ts\">\nconst count = 1\n</script>\n<template><div>{{ count }}</div></template>\n";
    let uri = open_test_vue(server, "/workspace/src/App.vue", source);

    let registered = server
        .documents
        .registration
        .wait_until(std::time::Duration::from_millis(300), || {
            server.documents.get(&uri).is_some()
        })
        .await;

    assert!(registered, "an open document is registered");
    // Structural proof, not a wall-clock one: an already-registered document
    // must resolve on the synchronous fast path without ever arming the
    // notify/timeout race. A `elapsed < N` ceiling flips under machine load;
    // this counter cannot — it is exactly zero whenever the fast path was
    // taken, whatever the scheduler does.
    assert_eq!(
        server.documents.registration.timeout_arm_count(),
        0,
        "an already-registered document must cost nothing to wait for — it must not \
         enter the wait machinery at all"
    );
}

/// A document that never arrives must give up on its budget rather than hang.
#[tokio::test(start_paused = true)]
async fn waiting_for_a_document_that_never_arrives_gives_up_on_its_budget() {
    let provider = Arc::new(MockTypeProvider::new());
    let type_provider: Arc<dyn TypeProvider> = provider.clone();
    let service = make_hover_test_service_tsgo(type_provider);
    let server = service.inner();

    let uri: Uri = "file:///workspace/src/NeverOpened.vue".parse().unwrap();
    let budget = std::time::Duration::from_millis(80);
    let start = tokio::time::Instant::now();

    let registered = server
        .documents
        .registration
        .wait_until(budget, || server.documents.get(&uri).is_some())
        .await;

    assert!(!registered, "the document was never registered");
    assert_eq!(
        tokio::time::Instant::now(),
        start + budget,
        "an unmet condition must consume exactly its budget on the virtual clock"
    );
    assert_eq!(
        server.documents.registration.budget_exhausted_count(),
        1,
        "an unmet condition must resolve through the budget's own timeout arm exactly once"
    );
}

/// The pending-snapshot drain is the OTHER path that compiles a carrier's IDE
/// surface for an open document, and it must recover a projection-less document
/// too.
///
/// A carrier whose open-time compile failed has no provider projection; the
/// document commit never compiles one, and the interactive repair heals it only
/// when a provider-backed request arrives (attempt-bounded). If the path that
/// DOES compile here does not install the projection, a document nobody hovers
/// stays stranded with no IDE features.
#[tokio::test(flavor = "multi_thread")]
async fn the_pending_snapshot_drain_recovers_a_projectionless_carrier() {
    let temp = tempfile::tempdir().expect("temp dir");
    let workspace = temp.path().join("workspace");
    std::fs::create_dir_all(workspace.join("src")).expect("create src dir");
    std::fs::write(workspace.join("tsconfig.app.json"), "{}").expect("write tsconfig");

    let workspace_id = crate::test_utils::canonical_test_path(&workspace);
    let app_id = format!("{workspace_id}/src/App.vue");
    let uri = crate::uri::path_to_file_uri(&app_id).expect("file uri");

    let host = crate::test_utils::make_filesystem_test_host(&workspace);
    let documents = DocumentRegistry::new(Arc::clone(&host));

    // Opened malformed: no IDE surface, so no projection.
    let _ = documents.did_open(&TextDocumentItem {
        uri: uri.clone(),
        language_id: "vue".to_string(),
        version: 1,
        text: "<script setup lang=\"ts\">\nconst broken = (((\n".to_string(),
    });
    assert!(
        documents.get_projection(&uri).is_none(),
        "precondition: the malformed open must leave the carrier projection-less"
    );

    // Fixed by a later edit. The commit stores text and compiles nothing.
    let _ = documents.did_change(
        &uri,
        2,
        "<script setup lang=\"ts\">\nconst fixed = 1\n</script>\n\
         <template><div>{{ fixed }}</div></template>\n",
    );
    assert!(
        documents.get_projection(&uri).is_none(),
        "the commit must not compile — if it did, this asserts nothing about the drain"
    );

    let tsconfig = format!("{workspace_id}/tsconfig.app.json");
    let snapshot = PublishedResolverSnapshot {
        resolver: verter_resolution::ModuleResolverCore::new(vec![
            verter_workspace::ide_project_config(
                workspace_id.clone(),
                workspace_id.clone(),
                Some(tsconfig.clone()),
            ),
        ]),
        resolution_view: None,
        ownership_ready: true,
    };
    let owner_vfs = configured_owner_vfs(&workspace_id, &tsconfig);
    let carrier_publish = crate::server::background_drain::CarrierPublishCtx {
        coordinator: None,
        provider_delivery: crate::external_ts::CarrierProviderDelivery::DirectOpen,
        vfs: Arc::clone(&owner_vfs),
        ownership_ready: true,
    };
    let provider = Arc::new(MockTypeProvider::new());
    let sync = ProjectSync::new(provider.clone(), ProjectSyncMode::FullProject);
    let provider_sync_states = DashMap::new();

    let _ = sync_pending_carrier_provider_file(
        Some(&sync),
        &documents,
        &snapshot,
        &provider_sync_states,
        &app_id,
        Some(&carrier_publish),
        &crate::external_ts::CarrierTransactionCoordinator::new(),
        &dashmap::DashSet::new(),
    )
    .await;

    assert!(
        documents.get_projection(&uri).is_some(),
        "the drain compiles the carrier's IDE surface, so it must also install the \
         projection the failed open never built — otherwise this path leaves the \
         document stranded with no IDE features"
    );
}

/// A request repair re-armed for a revision whose IDE leg is already delivered,
/// recorded and committed — the debounced tick got there first — applies
/// nothing: one IDE-companion application per revision. A restarted engine no
/// longer holds those bytes, so the same re-armed repair then delivers again.
#[tokio::test(flavor = "multi_thread")]
async fn a_rearmed_repair_of_a_current_ide_leg_applies_nothing_until_the_engine_restarts() {
    let (service, provider, uri) = make_request_surface_carrier().await;
    let server = service.inner();
    let canonical_id = "/workspace/src/App.vue";
    let ide_path = server
        .active_ide_path_for_uri(&uri)
        .expect("the fixture's direct IDE sync commits a live IDE path");
    let ide_writes = || {
        provider
            .file_sync_calls()
            .into_iter()
            .filter(|call| {
                matches!(
                    call,
                    MockCall::OpenFile { path, .. } | MockCall::UpdateFile { path, .. }
                        if path == &ide_path
                )
            })
            .count()
    };
    let delivered = ide_writes();
    assert!(delivered >= 1, "the fixture delivered the IDE companion");

    server.needs_ide_sync.insert(canonical_id.to_string());
    server.ensure_current_file_synced(&uri).await;
    assert_eq!(
        ide_writes(),
        delivered,
        "a re-armed repair of an already-current revision must not apply it again"
    );
    assert!(
        server.capture_provider_request_surface(&uri).is_some(),
        "skipping a current leg leaves the committed surface serving requests"
    );

    provider.forget_applied_content();
    server.needs_ide_sync.insert(canonical_id.to_string());
    server.ensure_current_file_synced(&uri).await;
    assert_eq!(
        ide_writes(),
        delivered + 1,
        "a restarted engine no longer holds the bytes, so the repair delivers again"
    );
}

/// An open whose eager sync already delivered, recorded and committed the IDE
/// leg re-arms only what is still owed: the interactive repair is not re-armed,
/// so the first request after the open does not apply the revision a second
/// time. Once the engine no longer holds the bytes, the leg reads as owed.
#[tokio::test(flavor = "multi_thread")]
async fn did_open_rearms_the_interactive_repair_only_when_the_ide_leg_is_owed() {
    let provider = Arc::new(MockTypeProvider::new());
    let type_provider: Arc<dyn TypeProvider> = provider.clone();
    let service = make_hover_test_service_tsgo(type_provider);
    let server = service.inner();
    install_test_resolver(server);
    let canonical_id = "/workspace/src/App.vue";
    let uri: Uri = "file:///workspace/src/App.vue".parse().expect("test uri");
    server
        .did_open(DidOpenTextDocumentParams {
            text_document: TextDocumentItem {
                uri: uri.clone(),
                language_id: "vue".to_string(),
                version: 1,
                text: REQUEST_SURFACE_APP.to_string(),
            },
        })
        .await;

    assert!(
        server.capture_provider_request_surface(&uri).is_some(),
        "the open's eager sync committed the IDE surface"
    );
    assert!(
        !server.needs_ide_sync.contains(canonical_id),
        "an open whose IDE leg is already current must not re-arm the repair"
    );
    assert!(!server.ide_leg_owed_for_open_document(&uri));
    provider.forget_applied_content();
    assert!(
        server.ide_leg_owed_for_open_document(&uri),
        "once the engine no longer holds the bytes the leg is owed again"
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn an_api_provider_round_trip_does_not_block_interactive_ide_repair() {
    let (service, provider, uri) = make_request_surface_carrier().await;
    let server = service.inner();
    server.sync_api_to_provider(&uri).await;
    let canonical_id = crate::documents::uri_to_canonical_id(&uri);
    let generation = server
        .current_or_init_ide_sync_open_generation(&uri, &canonical_id)
        .await
        .expect("open generation");
    let _lease = server.ide_sync_repair_lease(&canonical_id, generation);
    let api_path = server
        .provider_sync_state_for_source(&canonical_id)
        .expect("owned fixture")
        .api_path
        .expect("API companion");
    provider.forget_applied_content();
    let (arrived, release) = provider.block_open_file(&api_path);
    let (updated, release_update) = provider.block_update_file(&api_path);
    let repair = async {
        tokio::select! { _ = arrived.notified() => {}, _ = updated.notified() => {} }
        server.needs_ide_sync.insert(canonical_id.clone());
        let result = tokio::time::timeout(
            std::time::Duration::from_secs(5),
            server.ensure_current_file_synced(&uri),
        )
        .await;
        release.notify_one();
        release_update.notify_one();
        result
    };
    let (_, result) = tokio::join!(server.sync_api_to_provider(&uri), repair);
    assert!(
        result.is_ok(),
        "IDE repair must finish while the API write is parked"
    );
    assert!(server.capture_provider_request_surface(&uri).is_some());
}

#[tokio::test(flavor = "multi_thread")]
async fn close_orders_after_the_holder_and_an_old_waiter_cannot_commit_into_reopen() {
    let (service, provider, uri) = lane_interleaving_fixture("CloseOrdersRepair").await;
    let server = service.inner();
    let id = crate::documents::uri_to_canonical_id(&uri);
    edit_interleaving_document(server, &uri, 2, "world");
    let (arrived, release) = server.pause_next_ide_sync_before_provider_write(&id);
    let closing = async {
        arrived.notified().await;
        let close = server.did_close(DidCloseTextDocumentParams {
            text_document: TextDocumentIdentifier { uri: uri.clone() },
        });
        tokio::pin!(close);
        let close_waited = futures_util::poll!(close.as_mut()).is_pending();
        let old_waiter = server.ensure_current_file_synced(&uri);
        tokio::pin!(old_waiter);
        let waiter_waited = futures_util::poll!(old_waiter.as_mut()).is_pending();
        release.notify_one();
        close.await;
        server
            .did_open(DidOpenTextDocumentParams {
                text_document: TextDocumentItem {
                    uri: uri.clone(),
                    language_id: "vue".to_string(),
                    version: 3,
                    text: REQUEST_SURFACE_APP.replace("'hello'", "'again'"),
                },
            })
            .await;
        let before = ide_application_count(&provider, &id);
        old_waiter.await;
        (close_waited, waiter_waited, before)
    };
    let (_, (close_waited, waiter_waited, before)) =
        tokio::join!(server.ensure_current_file_synced(&uri), closing);
    assert!(close_waited && waiter_waited);
    assert_eq!(
        before, 2,
        "the prior holder and the reopened generation each deliver once"
    );
    assert_eq!(
        ide_application_count(&provider, &id),
        before,
        "the old waiter delivered and committed nothing"
    );
    assert!(server.capture_provider_request_surface(&uri).is_some());
}

#[tokio::test(flavor = "multi_thread")]
async fn a_scanner_started_closed_yields_when_the_document_opens_after_compile() {
    let provider = Arc::new(MockTypeProvider::new());
    let service = make_hover_test_service_tsgo(provider.clone());
    let server = service.inner();
    install_test_resolver(server);
    let id = "/workspace/src/ScannerMeetsOpen.vue";
    let uri: Uri = "file:///workspace/src/ScannerMeetsOpen.vue"
        .parse()
        .unwrap();
    server
        .documents
        .host()
        .upsert(UpsertRequest {
            input_id: id.to_string(),
            canonical_id: Some(id.to_string()),
            source: Arc::from(REQUEST_SURFACE_APP),
            file_language: FileLanguage::vue(),
            aliases: vec![],
        })
        .unwrap();
    let profile = server.documents.tsx_profile.read().clone();
    let host = server.documents.host();
    let (arrived, release) = crate::sync_coordinator::test_hooks::block_after_ide_compile(id);
    let opening = async {
        arrived.notified().await;
        server
            .did_open(DidOpenTextDocumentParams {
                text_document: TextDocumentItem {
                    uri: uri.clone(),
                    language_id: "vue".to_string(),
                    version: 1,
                    text: REQUEST_SURFACE_APP.to_string(),
                },
            })
            .await;
        let before = ide_application_count(&provider, id);
        release.notify_one();
        before
    };
    let (_, before) = tokio::join!(
        crate::workspace_scanner::sync_file_to_provider(
            id,
            &host,
            Some(&server.documents),
            &profile,
            server.project_sync.as_ref(),
            server.documents.provider_surfaces(),
            &server.vfs_workspace,
            true,
            &server.provider_sync_states,
            server.carrier_publish_coordinator.as_ref(),
            &server.carrier_transaction_coordinator,
            Some(&server.pending_snapshot_provider_sync),
            None
        ),
        opening
    );
    assert_eq!(before, 1);
    assert_eq!(ide_application_count(&provider, id), before);
    assert!(server.pending_snapshot_provider_sync.contains(id));
    assert!(server.capture_provider_request_surface(&uri).is_some());
}

/// The document is still closed when the scanner probes its lane, and opens
/// with an edited buffer — the open's own eager repair delivering it — before
/// the scanner delivers. The scanner's delivery fence must see the open and
/// refuse its disk-compiled bytes, so the open buffer's surface is the only one
/// applied and is never clobbered.
#[tokio::test(flavor = "multi_thread")]
async fn a_scanner_started_closed_is_refused_when_the_document_opens_after_its_lane_probe() {
    let provider = Arc::new(MockTypeProvider::new());
    let service = make_hover_test_service_tsgo(provider.clone());
    let server = service.inner();
    install_test_resolver(server);
    let id = "/workspace/src/ScannerMeetsOpenAtDelivery.vue";
    let uri: Uri = "file:///workspace/src/ScannerMeetsOpenAtDelivery.vue"
        .parse()
        .unwrap();
    server
        .documents
        .host()
        .upsert(UpsertRequest {
            input_id: id.to_string(),
            canonical_id: Some(id.to_string()),
            source: Arc::from(REQUEST_SURFACE_APP),
            file_language: FileLanguage::vue(),
            aliases: vec![],
        })
        .unwrap();
    let profile = server.documents.tsx_profile.read().clone();
    let host = server.documents.host();
    let (arrived, release) = crate::sync_coordinator::test_hooks::block_before_delivery(id);
    let opening = async {
        arrived.notified().await;
        server
            .did_open(DidOpenTextDocumentParams {
                text_document: TextDocumentItem {
                    uri: uri.clone(),
                    language_id: "vue".to_string(),
                    version: 1,
                    text: REQUEST_SURFACE_APP.replace("'hello'", "'opened'"),
                },
            })
            .await;
        let before = ide_application_count(&provider, id);
        release.notify_one();
        before
    };
    let (_, before) = tokio::join!(
        crate::workspace_scanner::sync_file_to_provider(
            id,
            &host,
            Some(&server.documents),
            &profile,
            server.project_sync.as_ref(),
            server.documents.provider_surfaces(),
            &server.vfs_workspace,
            true,
            &server.provider_sync_states,
            server.carrier_publish_coordinator.as_ref(),
            &server.carrier_transaction_coordinator,
            Some(&server.pending_snapshot_provider_sync),
            None
        ),
        opening
    );
    assert_eq!(before, 1, "the open's own repair delivered the document");
    assert_eq!(
        ide_application_count(&provider, id),
        before,
        "the closed-start scanner delivered nothing beneath the open document"
    );
    assert!(server.pending_snapshot_provider_sync.contains(id));
    assert!(server.capture_provider_request_surface(&uri).is_some());
}

/// The document is closed when the coordinator probes its lane and opens before
/// the coordinator pins its revision. The pinned open revision belongs to the
/// open document's own lane, so the lane-less coordinator yields instead of
/// delivering that revision unserialized beside the open's own repair.
#[tokio::test(flavor = "multi_thread")]
async fn a_coordinator_started_closed_yields_when_the_document_opens_before_its_pin() {
    let provider = Arc::new(MockTypeProvider::new());
    let service = make_hover_test_service_tsgo(provider.clone());
    let server = service.inner();
    install_test_resolver(server);
    let id = "/workspace/src/CoordinatorMeetsOpenAtPin.vue";
    let uri: Uri = "file:///workspace/src/CoordinatorMeetsOpenAtPin.vue"
        .parse()
        .unwrap();
    server
        .documents
        .host()
        .upsert(UpsertRequest {
            input_id: id.to_string(),
            canonical_id: Some(id.to_string()),
            source: Arc::from(REQUEST_SURFACE_APP),
            file_language: FileLanguage::vue(),
            aliases: vec![],
        })
        .unwrap();
    let deps = lane_interleaving_deps(server);
    let (arrived, release) = crate::sync_coordinator::test_hooks::block_after_lane_probe(id);
    let opening = async {
        arrived.notified().await;
        server
            .did_open(DidOpenTextDocumentParams {
                text_document: TextDocumentItem {
                    uri: uri.clone(),
                    language_id: "vue".to_string(),
                    version: 1,
                    text: REQUEST_SURFACE_APP.replace("'hello'", "'opened'"),
                },
            })
            .await;
        let before = ide_application_count(&provider, id);
        release.notify_one();
        before
    };
    let (outcome, before) = tokio::join!(
        crate::sync_coordinator::synchronize_document_outcome_for_test(&deps, id, uri.as_str()),
        opening
    );
    assert_eq!(
        outcome,
        crate::sync_coordinator::SyncFileOutcome::LaneBusy,
        "a coordinator holding no lane must yield the open document's revision"
    );
    assert_eq!(
        ide_application_count(&provider, id),
        before,
        "the lane-less coordinator delivered nothing beside the open document"
    );
    assert!(server.pending_snapshot_provider_sync.contains(id));
}
