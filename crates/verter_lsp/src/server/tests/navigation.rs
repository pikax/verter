use super::*;

#[test]
fn module_reference_request_kind_uses_require_semantics() {
    let require_reference = test_module_reference_with_semantics(
        "'pkg'",
        Some("pkg"),
        &[],
        verter_session_query::analysis::types::ModuleReferenceAnalyzability::Exact,
        0,
        5,
        verter_session_query::analysis::types::ModuleReferenceSemantics::Require,
        false,
    );
    assert_eq!(
        module_reference_request_kind(&require_reference),
        verter_session_query::resolution::ResolveRequestKind::RequireCall
    );

    let type_reference = test_module_reference_with_semantics(
        "'pkg'",
        Some("pkg"),
        &[],
        verter_session_query::analysis::types::ModuleReferenceAnalyzability::Exact,
        0,
        5,
        verter_session_query::analysis::types::ModuleReferenceSemantics::Import,
        true,
    );
    assert_eq!(
        module_reference_request_kind(&type_reference),
        verter_session_query::resolution::ResolveRequestKind::TypeImport
    );
}

// @ai-generated - Proves editor tsserver owns carrier hover, navigation, and rename.
#[tokio::test(flavor = "multi_thread")]
async fn editor_tsserver_yields_only_rename_and_keeps_serving_merged_features() {
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
    server
        .hover_native_semantics_enabled
        .store(true, std::sync::atomic::Ordering::Release);
    let source = "<script setup lang=\"ts\">\nconst count = 1\n</script>\n<template><div>{{ count }}</div></template>\n";
    let uri = open_test_vue(server, "/workspace/src/App.vue", source);
    let position = find_document_position(server, &uri, "{{ count", 3);

    // MERGED features keep serving. VS Code aggregates hover, definition and
    // references across every provider, so withholding Verter's native answer
    // cannot promote the editor plugin's — it can only leave the user with
    // nothing when the plugin has no answer for this carrier.
    let hover = super::super::nav_features::handle_hover(server, hover_params(&uri, position))
        .await
        .expect("hover request succeeds");
    assert!(
        hover.is_some(),
        "a merged feature must keep its native answer on the editor-owned route"
    );

    let definition = super::super::nav_features_navigation::handle_goto_definition(
        server,
        goto_definition_params(&uri, position),
    )
    .await
    .expect("definition request succeeds");
    assert!(
        definition.is_some(),
        "a merged feature must keep its native answer on the editor-owned route"
    );

    let references = super::super::nav_features_navigation::handle_references(
        server,
        ReferenceParams {
            text_document_position: TextDocumentPositionParams {
                text_document: TextDocumentIdentifier { uri: uri.clone() },
                position,
            },
            context: ReferenceContext {
                include_declaration: true,
            },
            work_done_progress_params: Default::default(),
            partial_result_params: Default::default(),
        },
    )
    .await
    .expect("references request succeeds");
    assert!(
        references.is_some_and(|found| !found.is_empty()),
        "a merged feature must keep its native answer on the editor-owned route"
    );

    let prepare = super::super::rename_prepare::handle_prepare_rename(
        server,
        TextDocumentPositionParams {
            text_document: TextDocumentIdentifier { uri: uri.clone() },
            position,
        },
    )
    .await
    .expect("prepare-rename request succeeds");
    assert!(
        prepare.is_none(),
        "the LSP must not claim the editor plugin's rename position, got {prepare:?}"
    );

    let rename = super::super::nav_features_navigation::handle_rename(
        server,
        RenameParams {
            text_document_position: TextDocumentPositionParams {
                text_document: TextDocumentIdentifier { uri },
                position,
            },
            new_name: "renamed".into(),
            work_done_progress_params: Default::default(),
        },
    )
    .await
    .expect("rename request succeeds");
    assert!(
        rename.is_none(),
        "the LSP must not compete with the attested editor rename provider, got {rename:?}"
    );
}

// A carrier owned by MULTIPLE configured projects must FAIL rename CLOSED (a
// clear error, no WorkspaceEdit) — never a silent partial cross-project rename.
// This is the safety boundary for the newly-narrowed multi-claimant resolution:
// per-file features serve from the single tsgo default owner, but a provider
// rename would only cover that one project and leave a symbol that escapes it
// (exported + imported by a sibling project) dangling.
//
// DISCRIMINATING: without the multi-claimant rename gate, `handle_rename` would
// run the provider rename against the resolved owner and return a WorkspaceEdit
// (`Ok(Some(_))`) — a partial edit — instead of the fail-closed error asserted
// here.
#[tokio::test(flavor = "multi_thread")]
async fn multi_claimant_carrier_fails_rename_closed_never_partial() {
    let ws = Arc::new(verter_workspace::FilesystemWorkspace::new(
        verter_workspace::FilesystemOptions::default(),
    ));
    let host = Arc::new(VerterHost::new(HostConfig::default(), ws.clone()));
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

    // A carrier exporting a symbol another project could import — the escape case.
    let source = "<script setup lang=\"ts\">\nexport const shared = 1\n</script>\n<template><div>{{ shared }}</div></template>\n";
    let uri = open_test_vue(server, "/workspace/src/App.vue", source);
    // Publish the MULTI-CLAIMANT ownership root LAST — after server construction and
    // `did_open` — so it is the authoritative root the rename gate observes.
    publish_multi_claimant_root(&ws, "/workspace");
    assert!(
        matches!(
            server.carrier_multi_claimancy(&uri),
            super::super::provider_state::CarrierMultiClaimancy::Ready
        ),
        "the carrier must be detected as multi-claimant for this test to be meaningful"
    );
    let position = find_document_position(server, &uri, "shared", 0);

    let prepare = super::super::rename_prepare::handle_prepare_rename(
        server,
        TextDocumentPositionParams {
            text_document: TextDocumentIdentifier { uri: uri.clone() },
            position,
        },
    )
    .await;
    assert!(
        prepare.is_err(),
        "prepare-rename on a multi-claimant carrier must FAIL CLOSED, got {prepare:?}"
    );

    let rename = super::super::nav_features_navigation::handle_rename(
        server,
        RenameParams {
            text_document_position: TextDocumentPositionParams {
                text_document: TextDocumentIdentifier { uri: uri.clone() },
                position,
            },
            new_name: "renamed".into(),
            work_done_progress_params: Default::default(),
        },
    )
    .await;
    match rename {
        Err(err) => assert!(
            err.message.contains("multiple TypeScript projects"),
            "the fail-closed rename error must explain the multi-claimant cause, got {err:?}"
        ),
        Ok(edit) => panic!(
            "a multi-claimant carrier must NEVER return a rename edit (silent partial \
             cross-project rename), got {edit:?}"
        ),
    }
}

// Genuine-bootstrap mirror of
// `multi_claimant_carrier_fails_rename_closed_never_partial`. The workspace is
// left on the exact eager root `Engine::new()` publishes: ownership is not
// authoritative and the project graph is empty. Rename must not infer unique
// ownership from that absence of claimants.
//
// DISCRIMINATING: without the authority-first check, the gate recognizes a cold
// snapshot only when its already-populated graph says `Ambiguous`. Against the
// genuine empty bootstrap root it returns `NotMultiClaimant`; both prepare and
// rename proceed, and the injected provider's single-file `WorkspaceEdit`
// escapes.
#[tokio::test(flavor = "multi_thread")]
async fn genuine_bootstrap_carrier_fails_rename_closed_never_partial() {
    let ws = Arc::new(verter_workspace::FilesystemWorkspace::new(
        verter_workspace::FilesystemOptions::default(),
    ));
    let (service, _socket, provider) = make_claimancy_rename_test_server(Arc::clone(&ws));
    let server = service.inner();

    let source = "<script setup lang=\"ts\">\nexport const shared = 1\n</script>\n<template><div>{{ shared }}</div></template>\n";
    let uri = open_test_vue(server, "/workspace/src/App.vue", source);
    let bootstrap = ws
        .load_published()
        .expect("Engine::new must eagerly publish the bootstrap root");
    assert!(
        !bootstrap.ownership_ready
            && bootstrap.snapshot.projects.is_empty()
            && matches!(
                server.carrier_multi_claimancy(&uri),
                super::super::provider_state::CarrierMultiClaimancy::NotReady
            ),
        "the genuine empty bootstrap graph must classify carrier rename as NotReady, got \
         ownership_ready={}, projects={}, claimancy={:?}",
        bootstrap.ownership_ready,
        bootstrap.snapshot.projects.len(),
        server.carrier_multi_claimancy(&uri),
    );
    let position = find_document_position(server, &uri, "{{ shared }}", 3);
    server.ensure_current_file_synced(&uri).await;
    server.publish_import_dependencies_settled(&uri).await;
    let after_sync = ws
        .load_published()
        .expect("provider sync must leave the eager bootstrap root published");
    assert!(
        Arc::ptr_eq(&bootstrap, &after_sync)
            && !after_sync.ownership_ready
            && after_sync.snapshot.projects.is_empty()
            && matches!(
                server.carrier_multi_claimancy(&uri),
                super::super::provider_state::CarrierMultiClaimancy::NotReady
            ),
        "provider sync helpers must not manufacture ownership authority; got \
         same_root={}, ownership_ready={}, projects={}, claimancy={:?}",
        Arc::ptr_eq(&bootstrap, &after_sync),
        after_sync.ownership_ready,
        after_sync.snapshot.projects.len(),
        server.carrier_multi_claimancy(&uri),
    );
    let ctx = synced_type_provider_context(server, &uri).await;
    let usage_offset = merge::carrier_position_to_tsx_offset_validated(
        &position,
        &ctx.carrier_line_index,
        &ctx.mapper,
        &ctx.tsx_line_index,
    )
    .expect("usage position should map into TSX");
    provider.set_rename_locations(
        &ctx.tsx_path,
        usage_offset,
        vec![crate::type_provider::protocol::RenameLocation {
            path: ctx.tsx_path.clone(),
            start: usage_offset,
            end: usage_offset + "shared".len() as u32,
        }],
    );

    let rename = super::super::nav_features_navigation::handle_rename(
        server,
        RenameParams {
            text_document_position: TextDocumentPositionParams {
                text_document: TextDocumentIdentifier { uri: uri.clone() },
                position,
            },
            new_name: "renamed".into(),
            work_done_progress_params: Default::default(),
        },
    )
    .await;
    match rename {
        Err(err) => assert!(
            err.message.contains("authoritative project ownership"),
            "the bootstrap refusal must explain the missing authority, got {err:?}"
        ),
        Ok(edit) => {
            panic!("a genuine-bootstrap carrier must NEVER return a rename edit, got {edit:?}")
        }
    }

    let prepare = super::super::rename_prepare::handle_prepare_rename(
        server,
        TextDocumentPositionParams {
            text_document: TextDocumentIdentifier { uri },
            position,
        },
    )
    .await;
    match prepare {
        Err(err) => assert!(
            err.message.contains("authoritative project ownership"),
            "prepare-rename must expose the missing-authority cause, got {err:?}"
        ),
        Ok(response) => {
            panic!("prepare-rename must expose the bootstrap refusal, got {response:?}")
        }
    }
}

/// Proves that rename admission refuses a pre-scripted provider response when
/// the ownership authority-assignment marker moves away from the published root
/// generation during the provider await. The rebuild pauses in `configure_paths`
/// after assigning the new authority but before production's per-file rebinding
/// step and before publishing the rebuilt root; releasing it afterward also
/// proves that the held rebuild publishes the expected ambiguous graph.
///
/// This mock's response is independent of ownership, and its open/query paths do
/// not consume the stored authority or perform production's `resync_open_files`
/// rebinding. This test therefore does NOT prove that an open provider file moved
/// to the rebuilt winner.
#[tokio::test(flavor = "multi_thread")]
async fn rename_refuses_when_ownership_assignment_marker_moves_during_provider_await() {
    let temp = tempfile::tempdir().expect("temp dir");
    let workspace = temp.path().join("workspace");
    std::fs::create_dir_all(workspace.join("src")).expect("workspace source dir");
    let source = "<script setup lang=\"ts\">\nexport const shared = 1\n</script>\n<template><div>{{ shared }}</div></template>\n";
    std::fs::write(workspace.join("src/App.vue"), source).expect("write carrier");
    let config = r#"{
  "compilerOptions": {
    "baseUrl": ".",
    "paths": { "@/*": ["src/*"] }
  },
  "include": ["src/**/*.vue"]
}"#;
    std::fs::write(workspace.join("tsconfig.zzz.json"), config)
        .expect("write initial unique config");

    let ws = Arc::new(verter_workspace::FilesystemWorkspace::new(
        verter_workspace::FilesystemOptions::default(),
    ));
    let (service, socket, provider) = make_claimancy_rename_test_server(Arc::clone(&ws));
    let drain_handle = tokio::spawn(async move {
        let mut socket = socket;
        while socket.next().await.is_some() {}
    });
    let server = service.inner();
    server.swap_vfs_workspace(Arc::clone(&ws));
    server.vite_config_options.lock().await.enabled = false;
    let workspace_id = crate::test_utils::canonical_test_path(&workspace);
    *server.workspace_roots.lock().await = vec![crate::uri::path_to_file_uri(&workspace_id)
        .expect("workspace URI")
        .as_str()
        .to_string()];

    server
        .spawn_background_init(None, "stale-authority regression initial build")
        .await;
    let initial_root = wait_published_root(&ws, |root| {
        root.ownership_ready
            && root.snapshot.generation
                == verter_workspace::workspace_snapshot::SnapshotGeneration(1)
    });

    let canonical = format!("{workspace_id}/src/App.vue");
    let initial_resolution = initial_root
        .snapshot
        .configured_owner_resolution_for_file(&canonical);
    assert!(
        matches!(
            initial_resolution,
            verter_workspace::workspace_snapshot::ConfiguredOwnerResolution::Unique(_)
        ),
        "the pre-rebuild root must genuinely prove unique ownership, got \
         {initial_resolution:?} from projects {:?}",
        initial_root
            .snapshot
            .projects
            .iter()
            .map(|project| (&project.root, &project.payload))
            .collect::<Vec<_>>()
    );
    assert!(
        matches!(
            provider.configured_owner(&canonical),
            Some(verter_type_runtime::traits::ProjectOwnership::Owned(owner))
                if owner.config_path.ends_with("/tsconfig.zzz.json")
        ),
        "the provider must initially consume the unique root's owner"
    );

    let uri = open_test_vue(server, &canonical, source);
    let position = find_document_position(server, &uri, "{{ shared }}", 3);
    server.ensure_current_file_synced(&uri).await;
    server.publish_import_dependencies_settled(&uri).await;
    let ctx = synced_type_provider_context(server, &uri).await;
    let usage_offset = merge::carrier_position_to_tsx_offset_validated(
        &position,
        &ctx.carrier_line_index,
        &ctx.mapper,
        &ctx.tsx_line_index,
    )
    .expect("usage position should map into TSX");
    provider.set_rename_locations(
        &ctx.tsx_path,
        usage_offset,
        vec![crate::type_provider::protocol::RenameLocation {
            path: ctx.tsx_path.clone(),
            start: usage_offset,
            end: usage_offset + "shared".len() as u32,
        }],
    );

    // Suspend the provider response only after admission has observed a matching
    // initial marker/root generation. The rebuild then advances the assignment
    // marker while this exact request is awaiting its answer.
    let (rename_arrived, rename_release) = provider.block_get_rename_locations(&ctx.tsx_path);
    let (configure_arrived, configure_release) = provider.block_configure_paths();
    let rename = super::super::nav_features_navigation::handle_rename(
        server,
        RenameParams {
            text_document_position: TextDocumentPositionParams {
                text_document: TextDocumentIdentifier { uri: uri.clone() },
                position,
            },
            new_name: "renamed".into(),
            work_done_progress_params: Default::default(),
        },
    );
    let rebuild_during_provider_await = async {
        tokio::time::timeout(
            std::time::Duration::from_secs(10),
            rename_arrived.notified(),
        )
        .await
        .expect("rename must reach the provider under the initial ownership witness");

        std::fs::write(workspace.join("tsconfig.json"), config)
            .expect("add the second claimant config");
        server
            .spawn_background_init(None, "stale-authority regression rebuild")
            .await;
        tokio::time::timeout(
            std::time::Duration::from_secs(10),
            configure_arrived.notified(),
        )
        .await
        .expect("rebuild must pause after installing provider ownership");

        let held_root = ws
            .load_published()
            .expect("the previous ready root must remain published during rebuild");
        assert!(
            Arc::ptr_eq(&initial_root, &held_root)
                && held_root.ownership_ready
                && matches!(
                    held_root
                        .snapshot
                        .configured_owner_resolution_for_file(&canonical),
                    verter_workspace::workspace_snapshot::ConfiguredOwnerResolution::Unique(_)
                ),
            "the held rebuild window must expose the old ready Unique root"
        );
        assert!(
            matches!(
                provider.configured_owner(&canonical),
                Some(verter_type_runtime::traits::ProjectOwnership::Owned(owner))
                    if owner.config_path.ends_with("/tsconfig.json")
            ),
            "the mock must expose the rebuilt authority's new default winner"
        );
        assert!(
            matches!(
                server.carrier_multi_claimancy(&uri),
                super::super::provider_state::CarrierMultiClaimancy::NotReady
            ),
            "a new request entering the root/provider mismatch must refuse as NotReady"
        );

        rename_release.notify_one();
    };
    let (rename, ()) = futures_util::future::join(rename, rebuild_during_provider_await).await;
    match rename {
        Err(err) => assert!(
            err.message.contains("ownership changed"),
            "the rebuild-window refusal must name ownership instability, got {err:?}"
        ),
        Ok(edit) => panic!(
            "a stale ready root must NEVER license a provider rename after ownership moved, \
             got {edit:?}"
        ),
    }

    configure_release.notify_one();
    let rebuilt_root = wait_published_root(&ws, |root| {
        root.snapshot.generation == verter_workspace::workspace_snapshot::SnapshotGeneration(2)
    });
    assert!(
        rebuilt_root.ownership_ready
            && matches!(
                rebuilt_root
                    .snapshot
                    .configured_owner_resolution_for_file(&canonical),
                verter_workspace::workspace_snapshot::ConfiguredOwnerResolution::Ambiguous(_)
            ),
        "the exact held rebuild must publish the ambiguous graph"
    );

    drain_handle.abort();
    drop(service);
}

// Positive control for the multi-claimant fail-closed rename gate: a UNIQUE
// carrier in an AUTHORITATIVE snapshot must STILL rename normally — a real
// `WorkspaceEdit`, never the fail-closed error. This guards against an over-broad
// regression where rename is refused after ownership publication.
//
// DISCRIMINATING: widen the gate to fire on a unique carrier and `prepare` becomes
// `Err` / `rename` becomes the fail-closed `Err`, failing the assertions below.
#[tokio::test(flavor = "multi_thread")]
async fn unique_carrier_still_renames_normally_not_fail_closed() {
    let app_source = "<script setup lang=\"ts\">\nconst vueTsTitle: string = \"x\"\n</script>\n<template><section>{{ vueTsTitle }}</section></template>\n";
    let (_temp, service, drain_handle, provider, workspace_id) =
        make_definition_test_server(&[("src/App.vue", "vue", app_source)]).await;
    let app_uri = workspace_uri(&workspace_id, "src/App.vue");
    let server = service.inner();

    let canonical = server
        .documents
        .get_canonical_id(&app_uri)
        .expect("the open carrier must have a canonical id");
    let published = server
        .documents
        .host()
        .workspace_read()
        .published_root()
        .expect("the definition fixture must publish its ownership graph");
    // The carrier has a SINGLE configured owner in an AUTHORITATIVE snapshot —
    // the admission gate must serve.
    assert!(
        published.ownership_ready
            && matches!(
                published
                    .snapshot
                    .configured_owner_resolution_for_file(&canonical),
                verter_workspace::workspace_snapshot::ConfiguredOwnerResolution::Unique(_)
            )
            && matches!(
                server.carrier_multi_claimancy(&app_uri),
                super::super::provider_state::CarrierMultiClaimancy::NotMultiClaimant(_)
            ),
        "the control must be authoritatively unique and admitted, got \
         ownership_ready={}, resolution={:?}, claimancy={:?}",
        published.ownership_ready,
        published
            .snapshot
            .configured_owner_resolution_for_file(&canonical),
        server.carrier_multi_claimancy(&app_uri),
    );

    let position = find_document_position(server, &app_uri, "{{ vueTsTitle }}", 3);
    // Lifecycle surface sync + settled background dependency publication: the
    // rename handler only CAPTURES readiness (it never starts the pass), so the
    // test provides both receipts the way production does.
    server.ensure_current_file_synced(&app_uri).await;
    server.publish_import_dependencies_settled(&app_uri).await;
    let ctx = synced_type_provider_context(server, &app_uri).await;
    let usage_offset = merge::carrier_position_to_tsx_offset_validated(
        &position,
        &ctx.carrier_line_index,
        &ctx.mapper,
        &ctx.tsx_line_index,
    )
    .expect("usage position should map into TSX");
    // The provider answers over the PROVIDER-side token in the IDE surface.
    provider.set_rename_locations(
        &ctx.tsx_path,
        usage_offset,
        vec![crate::type_provider::protocol::RenameLocation {
            path: ctx.tsx_path.clone(),
            start: usage_offset,
            end: usage_offset + "vueTsTitle".len() as u32,
        }],
    );

    // Prepare-rename must NOT fail closed (it returns Ok — a real prepare, never the
    // multi-claimant Err).
    let prepare = super::super::rename_prepare::handle_prepare_rename(
        server,
        TextDocumentPositionParams {
            text_document: TextDocumentIdentifier {
                uri: app_uri.clone(),
            },
            position,
        },
    )
    .await;
    assert!(
        prepare.is_ok(),
        "prepare-rename on a unique carrier must NOT fail closed, got {prepare:?}"
    );

    // Rename must return a REAL WorkspaceEdit (never the fail-closed Err).
    let edit = super::super::nav_features_navigation::handle_rename(
        server,
        RenameParams {
            text_document_position: TextDocumentPositionParams {
                text_document: TextDocumentIdentifier {
                    uri: app_uri.clone(),
                },
                position,
            },
            new_name: "renamedTitle".into(),
            work_done_progress_params: Default::default(),
        },
    )
    .await
    .expect("rename on a unique carrier must succeed, not fail closed")
    .expect("a unique carrier must return a real WorkspaceEdit");

    let triples = workspace_edit_triples(&edit);
    assert!(
        triples
            .iter()
            .any(|(_, _, new_text)| new_text == "renamedTitle"),
        "the unique-carrier rename must edit the symbol to the new name, got {triples:?}"
    );

    drain_handle.abort();
    drop(service);
}

#[tokio::test]
async fn goto_definition_component_event_name_reaches_child_define_emits() {
    let child_source = "<script setup lang=\"ts\">\nconst emit = defineEmits<{ custom: [payload: string] }>()\n</script>\n";
    let parent_source = "<script setup lang=\"ts\">\nimport MyComp from './MyComp.vue'\nfunction handleCustom(payload: string) {}\n</script>\n<template>\n  <MyComp @custom=\"handleCustom\" />\n</template>\n";
    let (_temp, service, drain_handle, _provider, workspace_id) = make_definition_test_server(&[
        ("src/MyComp.vue", "vue", child_source),
        ("src/App.vue", "vue", parent_source),
    ])
    .await;

    let app_uri = workspace_uri(&workspace_id, "src/App.vue");
    let child_uri = workspace_uri(&workspace_id, "src/MyComp.vue");
    let server = service.inner();
    let position = find_document_position(server, &app_uri, "@custom=\"handleCustom\"", 1);

    let response = server
        .goto_definition(goto_definition_params(&app_uri, position))
        .await
        .expect("goto definition should succeed")
        .expect("component event should resolve");
    let locations = definition_locations(response);
    let target = locations
        .iter()
        .find(|location| location.uri == child_uri)
        .expect("definition should point to MyComp.vue");

    assert_eq!(
        target.range.start.line,
        line_for_snippet(child_source, "custom: [payload: string]"),
        "definition should point to the child defineEmits declaration"
    );

    drain_handle.abort();
    drop(service);
}

/// A definition answered natively from an imported child — no provider leg —
/// settles through the same admission as every other definition: an edit of
/// the requested document or a workspace replacement after admission answers
/// `ContentModified`, never a location computed under the moved inputs.
#[tokio::test(flavor = "multi_thread")]
async fn native_cross_file_definition_settles_through_its_admission() {
    let child_source = "<script setup lang=\"ts\">\nconst emit = defineEmits<{ custom: [payload: string] }>()\n</script>\n";
    let parent_source = "<script setup lang=\"ts\">\nimport MyComp from './MyComp.vue'\nfunction handleCustom(payload: string) {}\n</script>\n<template>\n  <MyComp @custom=\"handleCustom\" />\n</template>\n";
    for movement in ["edit", "workspace"] {
        let (_temp, service, drain_handle, _provider, workspace_id) =
            make_definition_test_server(&[
                ("src/MyComp.vue", "vue", child_source),
                ("src/App.vue", "vue", parent_source),
            ])
            .await;
        let app_uri = workspace_uri(&workspace_id, "src/App.vue");
        let child_uri = workspace_uri(&workspace_id, "src/MyComp.vue");
        let server = service.inner();
        let position = find_document_position(server, &app_uri, "@custom=\"handleCustom\"", 1);
        let unmoved = definition_locations(
            server
                .goto_definition(goto_definition_params(&app_uri, position))
                .await
                .expect("an unmoved definition succeeds")
                .expect("the child event resolves natively"),
        );
        assert!(
            unmoved.iter().any(|location| location.uri == child_uri),
            "{movement}: the unmoved definition reaches the child: {unmoved:?}"
        );

        let moved = Arc::new(std::sync::atomic::AtomicBool::new(false));
        {
            let server = server.clone();
            let moved = Arc::clone(&moved);
            let app_uri = app_uri.clone();
            let workspace_id = workspace_id.clone();
            let edited = format!("{parent_source}<!-- edited -->\n");
            let next_version = server
                .documents
                .get(&app_uri)
                .expect("the carrier is open")
                .version
                + 1;
            server.request_barriers().clear();
            server.request_barriers().arm(
                super::super::test_support::RequestBarrier::Capture,
                Arc::new(move |arrival| {
                    if arrival == 0 {
                        if movement == "edit" {
                            let _ = server.documents.did_change(&app_uri, next_version, &edited);
                        } else {
                            let tsconfig = format!("{workspace_id}/tsconfig.json");
                            install_test_resolver_for_root(
                                &server,
                                &workspace_id,
                                Some(tsconfig.as_str()),
                            );
                        }
                        moved.store(true, std::sync::atomic::Ordering::SeqCst);
                    }
                    Box::pin(async {})
                }),
            );
        }
        let raced = server
            .goto_definition(goto_definition_params(&app_uri, position))
            .await;
        // The armed action holds a server clone; clear it before any assertion
        // so the server is released even when one fails.
        server.request_barriers().clear();
        assert!(
            moved.load(std::sync::atomic::Ordering::SeqCst),
            "{movement}: the native definition passes through its admission"
        );
        assert!(
            matches!(&raced, Err(error) if error.code == tower_lsp_server::jsonrpc::ErrorCode::ContentModified),
            "{movement}: a native definition whose inputs moved after admission answers \
             ContentModified: {raced:?}"
        );

        drain_handle.abort();
        drop(service);
    }
}

#[tokio::test]
async fn goto_definition_component_event_name_reaches_child_listener_prop() {
    let child_source = "<script setup lang=\"ts\">\ndefineProps<{\n  label: string\n  onAlert?: (payload: string) => void\n}>()\n</script>\n";
    let parent_source = "<script setup lang=\"ts\">\nimport OnEventPropComp from './OnEventPropComp.vue'\nfunction handleAlert(payload: string) {}\n</script>\n<template>\n  <OnEventPropComp label=\"ok\" @alert=\"handleAlert\" />\n</template>\n";
    let (_temp, service, drain_handle, _provider, workspace_id) = make_definition_test_server(&[
        ("src/OnEventPropComp.vue", "vue", child_source),
        ("src/App.vue", "vue", parent_source),
    ])
    .await;

    let app_uri = workspace_uri(&workspace_id, "src/App.vue");
    let child_uri = workspace_uri(&workspace_id, "src/OnEventPropComp.vue");
    let server = service.inner();
    let position = find_document_position(server, &app_uri, "@alert=\"handleAlert\"", 1);

    let response = server
        .goto_definition(goto_definition_params(&app_uri, position))
        .await
        .expect("goto definition should succeed")
        .expect("prop-backed event should resolve");
    let locations = definition_locations(response);
    let target = locations
        .iter()
        .find(|location| location.uri == child_uri)
        .expect("definition should point to OnEventPropComp.vue");

    assert_eq!(
        target.range.start.line,
        line_for_snippet(child_source, "onAlert?: (payload: string) => void"),
        "definition should point to the child listener prop"
    );

    drain_handle.abort();
    drop(service);
}

#[tokio::test]
async fn goto_definition_component_event_name_returns_emit_before_listener_prop() {
    let child_source = "<script setup lang=\"ts\">\ndefineProps<{\n  onAlert?: () => void\n}>()\nconst emit = defineEmits<{ alert: [] }>()\n</script>\n";
    let parent_source = "<script setup lang=\"ts\">\nimport BothEventComp from './BothEventComp.vue'\nfunction handleAlert() {}\n</script>\n<template>\n  <BothEventComp @alert=\"handleAlert\" />\n</template>\n";
    let (_temp, service, drain_handle, _provider, workspace_id) = make_definition_test_server(&[
        ("src/BothEventComp.vue", "vue", child_source),
        ("src/App.vue", "vue", parent_source),
    ])
    .await;

    let app_uri = workspace_uri(&workspace_id, "src/App.vue");
    let child_uri = workspace_uri(&workspace_id, "src/BothEventComp.vue");
    let server = service.inner();
    let position = find_document_position(server, &app_uri, "@alert=\"handleAlert\"", 1);

    let response = server
        .goto_definition(goto_definition_params(&app_uri, position))
        .await
        .expect("goto definition should succeed")
        .expect("event should resolve");
    let locations = definition_locations(response);

    assert_eq!(locations.len(), 2, "should return emit and listener prop");
    assert_eq!(locations[0].uri, child_uri, "emit should resolve in child");
    assert_eq!(
        locations[1].uri, child_uri,
        "listener prop should resolve in child"
    );
    assert_eq!(
        locations[0].range.start.line,
        line_for_snippet(child_source, "alert: []"),
        "defineEmits should come first"
    );
    assert_eq!(
        locations[1].range.start.line,
        line_for_snippet(child_source, "onAlert?: () => void"),
        "listener prop should come second"
    );

    drain_handle.abort();
    drop(service);
}

#[tokio::test]
async fn goto_definition_component_event_name_returns_none_when_child_has_no_match() {
    let child_source = "<script setup lang=\"ts\">\ndefineEmits<{ alert: [] }>()\ndefineProps<{ onAlert?: () => void }>()\n</script>\n";
    let parent_source = "<script setup lang=\"ts\">\nimport MyComp from './MyComp.vue'\nfunction handleMissing() {}\n</script>\n<template>\n  <MyComp @missing=\"handleMissing\" />\n</template>\n";
    let (_temp, service, drain_handle, _provider, workspace_id) = make_definition_test_server(&[
        ("src/MyComp.vue", "vue", child_source),
        ("src/App.vue", "vue", parent_source),
    ])
    .await;

    let app_uri = workspace_uri(&workspace_id, "src/App.vue");
    let server = service.inner();
    let position = find_document_position(server, &app_uri, "@missing=\"handleMissing\"", 1);

    let response = server
        .goto_definition(goto_definition_params(&app_uri, position))
        .await
        .expect("goto definition should succeed");

    assert!(
        response.is_none(),
        "unknown child component events should suppress same-file handler fallback"
    );

    drain_handle.abort();
    drop(service);
}

#[tokio::test]
async fn goto_definition_component_event_name_handles_barrel_reexports() {
    let child_source =
        "<script setup lang=\"ts\">\nconst emit = defineEmits<{ custom: [] }>()\n</script>\n";
    let barrel_source = "export { default as BarrelComp } from './BarrelComp.vue'\n";
    let parent_source = "<script setup lang=\"ts\">\nimport { BarrelComp } from './components'\nfunction handleCustom() {}\n</script>\n<template>\n  <BarrelComp @custom=\"handleCustom\" />\n</template>\n";
    let (_temp, service, drain_handle, _provider, workspace_id) = make_definition_test_server(&[
        ("src/components/BarrelComp.vue", "vue", child_source),
        ("src/components/index.ts", "typescript", barrel_source),
        ("src/App.vue", "vue", parent_source),
    ])
    .await;

    let app_uri = workspace_uri(&workspace_id, "src/App.vue");
    let child_uri = workspace_uri(&workspace_id, "src/components/BarrelComp.vue");
    let server = service.inner();
    let position = find_document_position(server, &app_uri, "@custom=\"handleCustom\"", 1);

    let response = server
        .goto_definition(goto_definition_params(&app_uri, position))
        .await
        .expect("goto definition should succeed")
        .expect("barrel event should resolve");
    let locations = definition_locations(response);
    let target = locations
        .iter()
        .find(|location| location.uri == child_uri)
        .expect("definition should follow the barrel to BarrelComp.vue");

    assert_eq!(
        target.range.start.line,
        line_for_snippet(child_source, "custom: []"),
        "definition should point to the re-exported child emit declaration"
    );

    drain_handle.abort();
    drop(service);
}

#[tokio::test(flavor = "multi_thread")]
async fn rename_after_did_change_repairs_latest_provider_surface_for_vue_and_svelte() {
    for (extension, language_id, first_source, edited_source, marker, cursor_offset) in [
        (
            "vue",
            "vue",
            "<script setup lang=\"ts\">const localName = 1</script><template>{{ localName }}</template>",
            "<script setup lang=\"ts\">\nconst localName = 1\n</script>\n<template>{{ localName }}</template>\n",
            "{{ localName",
            3,
        ),
        (
            "svelte",
            "svelte",
            "<script lang=\"ts\">const localName = 1</script>{localName}",
            "<script lang=\"ts\">\nconst localName = 1\n</script>\n{localName}\n",
            "{localName}",
            1,
        ),
    ] {
        let app_path = format!("src/App.{extension}");
        let (_temp, service, drain_handle, provider, workspace_id) =
            make_definition_test_server_with_kind(
                &[(&app_path, language_id, first_source)],
                crate::TypeProviderKind::Tsgo,
            )
            .await;
        let server = service.inner();
        let uri = workspace_uri(&workspace_id, &app_path);
        server.ensure_current_file_synced(&uri).await;
        server.publish_import_dependencies_settled(&uri).await;
        provider.clear_calls();

        super::super::lifecycle::handle_did_change(
            server,
            DidChangeTextDocumentParams {
                text_document: VersionedTextDocumentIdentifier {
                    uri: uri.clone(),
                    version: 2,
                },
                content_changes: vec![TextDocumentContentChangeEvent {
                    range: None,
                    range_length: None,
                    text: edited_source.to_string(),
                }],
            },
        )
        .await;
        let position = find_document_position(server, &uri, marker, cursor_offset);
        let _ = super::super::nav_features_navigation::handle_rename(
            server,
            RenameParams {
                text_document_position: TextDocumentPositionParams {
                    text_document: TextDocumentIdentifier { uri: uri.clone() },
                    position,
                },
                new_name: "renamedLocal".into(),
                work_done_progress_params: Default::default(),
            },
        )
        .await
        .expect("rename request should succeed");

        let calls = provider.calls();
        assert!(
            calls
                .iter()
                .any(|call| matches!(call, MockCall::UpdateFile { .. })),
            "{extension}: rename must repair the latest provider buffer: {calls:?}"
        );
        assert!(
            calls
                .iter()
                .any(|call| matches!(call, MockCall::GetRenameLocations { .. })),
            "{extension}: rename must query the provider after repairing the buffer: {calls:?}"
        );

        drain_handle.abort();
        drop(service);
    }
}

#[tokio::test]
async fn goto_definition_component_event_name_skips_type_provider_virtual_fallback() {
    let child_source = "<script setup lang=\"ts\">\nconst emit = defineEmits<{ custom: [payload: string] }>()\n</script>\n";
    let parent_source = "<script setup lang=\"ts\">\nimport MyComp from './MyComp.vue'\nfunction handleCustom(payload: string) {}\n</script>\n<template>\n  <MyComp @custom=\"handleCustom\" />\n</template>\n";
    let (_temp, service, drain_handle, provider, workspace_id) = make_definition_test_server(&[
        ("src/MyComp.vue", "vue", child_source),
        ("src/App.vue", "vue", parent_source),
    ])
    .await;

    let app_uri = workspace_uri(&workspace_id, "src/App.vue");
    let child_uri = workspace_uri(&workspace_id, "src/MyComp.vue");
    let server = service.inner();
    let position = find_document_position(server, &app_uri, "@custom=\"handleCustom\"", 1);
    let ctx = synced_type_provider_context(server, &app_uri).await;
    let tsx_offset = merge::carrier_position_to_tsx_offset_validated(
        &position,
        &ctx.carrier_line_index,
        &ctx.mapper,
        &ctx.tsx_line_index,
    )
    .expect("event position should map into TSX");
    provider.set_definitions(
        &ctx.tsx_path,
        tsx_offset,
        vec![TypeLocation {
            path: ctx.tsx_path.clone(),
            start: 0,
            end: 0,
        }],
    );

    let response = server
        .goto_definition(goto_definition_params(&app_uri, position))
        .await
        .expect("goto definition should succeed")
        .expect("native component event should resolve");
    let locations = definition_locations(response);
    let target = locations
        .iter()
        .find(|location| location.uri == child_uri)
        .expect("native child definition should win");

    assert_eq!(
        target.range.start.line,
        line_for_snippet(child_source, "custom: [payload: string]"),
        "native child definition should be returned instead of the virtual parent file"
    );
    assert!(
        !provider
            .calls()
            .iter()
            .any(|call| matches!(call, MockCall::GetDefinition { .. })),
        "native component event resolution should skip the type provider entirely"
    );

    drain_handle.abort();
    drop(service);
}

#[tokio::test]
async fn contract_unknown_event_name_produces_no_definition() {
    let child_source =
        "<script setup lang=\"ts\">\nconst emit = defineEmits<{ custom: [] }>()\n</script>\n";
    let parent_source = "<script setup lang=\"ts\">\nimport MyComp from './MyComp.vue'\nfunction h() {}\n</script>\n<template>\n  <MyComp @nope=\"h\" />\n</template>\n";
    let (_temp, service, drain_handle, _provider, workspace_id) = make_definition_test_server(&[
        ("src/MyComp.vue", "vue", child_source),
        ("src/App.vue", "vue", parent_source),
    ])
    .await;

    let app_uri = workspace_uri(&workspace_id, "src/App.vue");
    let server = service.inner();
    let position = find_document_position(server, &app_uri, "@nope=", 1);

    let response = server
        .goto_definition(goto_definition_params(&app_uri, position))
        .await
        .expect("goto definition should succeed");
    assert!(
        response.is_none(),
        "unknown event must fail closed with no link, got {response:?}"
    );

    drain_handle.abort();
    drop(service);
}

#[tokio::test]
async fn contract_unknown_slot_name_produces_no_definition() {
    let child_source = "<script setup lang=\"ts\">\ndefineSlots<{ header(props: {}): any }>()\n</script>\n<template>\n  <slot name=\"header\" />\n</template>\n";
    let parent_source = "<script setup lang=\"ts\">\nimport MyComp from './MyComp.vue'\n</script>\n<template>\n  <MyComp>\n    <template #footer>\n      x\n    </template>\n  </MyComp>\n</template>\n";
    let (_temp, service, drain_handle, _provider, workspace_id) = make_definition_test_server(&[
        ("src/MyComp.vue", "vue", child_source),
        ("src/App.vue", "vue", parent_source),
    ])
    .await;

    let app_uri = workspace_uri(&workspace_id, "src/App.vue");
    let server = service.inner();
    let position = find_document_position(server, &app_uri, "#footer", 1);

    let response = server
        .goto_definition(goto_definition_params(&app_uri, position))
        .await
        .expect("goto definition should succeed");
    assert!(
        response.is_none(),
        "unknown slot must fail closed with no link, got {response:?}"
    );

    drain_handle.abort();
    drop(service);
}

#[tokio::test]
async fn contract_unknown_prop_name_produces_no_definition() {
    // Negative control guarding the deleted file-start fallback in
    // component_resolve.rs: a prop attribute with NO matching child declaration
    // must fail closed — no link, no 0:0 mis-mapped affordance. Reintroducing
    // the fallback fails this test.
    let child_source = "<script setup lang=\"ts\">\ndefineProps<{ title: string }>()\n</script>\n";
    let parent_source = "<script setup lang=\"ts\">\nimport MyComp from './MyComp.vue'\nconst v = 'x'\n</script>\n<template>\n  <MyComp :nope=\"v\" />\n</template>\n";
    let (_temp, service, drain_handle, _provider, workspace_id) = make_definition_test_server(&[
        ("src/MyComp.vue", "vue", child_source),
        ("src/App.vue", "vue", parent_source),
    ])
    .await;

    let app_uri = workspace_uri(&workspace_id, "src/App.vue");
    let server = service.inner();
    let position = find_document_position(server, &app_uri, ":nope=", 1);

    let response = server
        .goto_definition(goto_definition_params(&app_uri, position))
        .await
        .expect("goto definition should succeed");
    assert!(
        response.is_none(),
        "unknown prop must fail closed with no link, got {response:?}"
    );

    drain_handle.abort();
    drop(service);
}

#[tokio::test]
async fn contract_kebab_prop_rename_classifies_cross_file_usage() {
    // Spot check: kebab template usage is discoverable as a rename of the
    // camelCase child declaration (script + template span the same binding).
    let child_source = "<script setup lang=\"ts\">\ndefineProps<{ myProp: string }>()\n</script>\n";
    let parent_source = "<script setup lang=\"ts\">\nimport MyComp from './MyComp.vue'\nconst v = 'x'\n</script>\n<template>\n  <MyComp :my-prop=\"v\" />\n</template>\n";
    let (_temp, service, drain_handle, _provider, workspace_id) = make_definition_test_server(&[
        ("src/MyComp.vue", "vue", child_source),
        ("src/App.vue", "vue", parent_source),
    ])
    .await;

    let app_uri = workspace_uri(&workspace_id, "src/App.vue");
    let child_uri = workspace_uri(&workspace_id, "src/MyComp.vue");
    let server = service.inner();
    let position = find_document_position(server, &app_uri, ":my-prop=", 1);

    match server.classify_child_prop_rename(&app_uri, &position) {
        super::super::child_prop_rename::ChildPropRenameClass::Confirmed(target) => {
            assert_eq!(
                target.usage.parent_prop_name, "my-prop",
                "usage name is the authored kebab form"
            );
            match target.declaration {
                super::super::child_prop_rename::ChildPropDeclarationProof::Known {
                    uri,
                    range,
                    ..
                } => {
                    assert_eq!(uri, child_uri, "declaration must be the child SFC");
                    let range = range.expect("declaration range must resolve");
                    assert_eq!(
                        range.start.line,
                        line_for_snippet(child_source, "myProp: string"),
                        "rename declaration must be the camel myProp field"
                    );
                }
                super::super::child_prop_rename::ChildPropDeclarationProof::Unknown => {
                    panic!("expected Known declaration, got Unknown")
                }
            }
        }
        super::super::child_prop_rename::ChildPropRenameClass::NotChildProp => {
            panic!("kebab usage must classify as Confirmed rename, got NotChildProp")
        }
    }

    drain_handle.abort();
    drop(service);
}

#[tokio::test]
async fn contract_kebab_prop_rename_refuses_when_a_second_parent_is_unproven() {
    // EXECUTED rename (not classification): a SECOND parent also passes
    // `:my-prop`, while the provider answer below names only the initiating
    // parent. The old declaration+initiating-parent gate shipped that partial
    // WorkspaceEdit; the public-prop admission must now refuse before asking the
    // provider because no available authority proves every parent was found.
    let child_source = "<script setup lang=\"ts\">\ndefineProps<{ myProp: string }>()\n</script>\n";
    let parent_source = "<script setup lang=\"ts\">\nimport MyComp from './MyComp.vue'\nconst v = 'x'\n</script>\n<template>\n  <MyComp :my-prop=\"v\" />\n</template>\n";
    let second_parent_source = "<script setup lang=\"ts\">\nimport MyComp from './MyComp.vue'\nconst other = 'y'\n</script>\n<template>\n  <MyComp :my-prop=\"other\" />\n</template>\n";
    let (_temp, service, drain_handle, provider, workspace_id) = make_definition_test_server(&[
        ("src/MyComp.vue", "vue", child_source),
        ("src/App.vue", "vue", parent_source),
        ("src/SecondParent.vue", "vue", second_parent_source),
    ])
    .await;

    let app_uri = workspace_uri(&workspace_id, "src/App.vue");
    let server = service.inner();
    let position = find_document_position(server, &app_uri, ":my-prop=", 1);

    // Production-shaped setup: the child's `{carrier}.verter.ts` API surface
    // must be live for the synthesis snapshot capture — delivered by the
    // BACKGROUND dependency publication (settled here), which also mints the
    // DependencyReady receipt the rename handler captures. Then seed the
    // provider's OWN rename answer for the parent usage leg in the parent IDE
    // surface (what tsserver returns; tsgo omits even this and the gate fails
    // closed).
    server.ensure_current_file_synced(&app_uri).await;
    server.publish_import_dependencies_settled(&app_uri).await;
    let ctx = synced_type_provider_context(server, &app_uri).await;
    let usage_offset = merge::carrier_position_to_tsx_offset_validated(
        &position,
        &ctx.carrier_line_index,
        &ctx.mapper,
        &ctx.tsx_line_index,
    )
    .expect("usage position should map into TSX");
    // A real provider answers over the PROVIDER-side token — the IDE surface
    // spells the prop camelCase (`myProp`), not the authored kebab — so the
    // seeded range covers exactly that token's extent.
    provider.set_rename_locations(
        &ctx.tsx_path,
        usage_offset,
        vec![crate::type_provider::protocol::RenameLocation {
            path: ctx.tsx_path.clone(),
            start: usage_offset,
            end: usage_offset + "myProp".len() as u32,
        }],
    );

    let rename_queries_before = provider
        .calls()
        .iter()
        .filter(|call| {
            matches!(
                call,
                crate::type_provider::mock::MockCall::GetRenameLocations { .. }
            )
        })
        .count();
    let prepared = super::super::rename_prepare::handle_prepare_rename(
        server,
        TextDocumentPositionParams {
            text_document: TextDocumentIdentifier {
                uri: app_uri.clone(),
            },
            position,
        },
    )
    .await
    .expect("prepare request succeeds");
    assert!(
        prepared.is_none(),
        "prepare must not offer a public component-prop rename"
    );

    let result = super::super::nav_features_navigation::handle_rename(
        server,
        RenameParams {
            text_document_position: TextDocumentPositionParams {
                text_document: TextDocumentIdentifier {
                    uri: app_uri.clone(),
                },
                position,
            },
            new_name: "myPropRenamed".into(),
            work_done_progress_params: Default::default(),
        },
    )
    .await;
    let error = result.expect_err(
        "a public component-prop rename with an unproven sibling parent must return no edit",
    );
    assert_eq!(
        error.code,
        tower_lsp_server::jsonrpc::ErrorCode::ServerError(-32803)
    );
    assert!(
        error.message.contains("complete cross-file usage proof"),
        "the refusal must explain why no WorkspaceEdit is safe, got {:?}",
        error.message
    );
    let rename_queries_after = provider
        .calls()
        .iter()
        .filter(|call| {
            matches!(
                call,
                crate::type_provider::mock::MockCall::GetRenameLocations { .. }
            )
        })
        .count();
    assert_eq!(
        rename_queries_after, rename_queries_before,
        "public-prop admission must refuse before provider rename is called"
    );

    drain_handle.abort();
    drop(service);
}

/// A provider on a case-insensitive filesystem (tsgo/tsserver on Windows)
/// reports paths fully case-folded (`d:/…/app.vue`). The current-carrier
/// rename leg must still classify as the current carrier and re-anchor to the
/// AUTHORED request URI — never echo the provider's folded path as a second
/// URI (clients key edits case-sensitively and silently drop them).
// A fully case-folded path is the same file only on a case-insensitive host.
// On Linux it may name a distinct real file, so accepting it there would make
// rename fail open and edit the wrong source.
#[cfg(any(target_os = "windows", target_os = "macos"))]
#[tokio::test]
async fn contract_rename_provider_case_folded_carrier_path_reanchors_to_authored_uri() {
    let app_source = "<script setup lang=\"ts\">\nconst vueTsTitle: string = \"x\"\n</script>\n<template><section>{{ vueTsTitle }}</section></template>\n";
    let (_temp, service, drain_handle, provider, workspace_id) =
        make_definition_test_server(&[("src/App.vue", "vue", app_source)]).await;
    let app_uri = workspace_uri(&workspace_id, "src/App.vue");
    let server = service.inner();
    let position = find_document_position(server, &app_uri, "{{ vueTsTitle }}", 3);

    // Lifecycle surface sync + settled background dependency publication (the
    // rename handler only captures readiness — see the unique-carrier test).
    server.ensure_current_file_synced(&app_uri).await;
    server.publish_import_dependencies_settled(&app_uri).await;
    let ctx = synced_type_provider_context(server, &app_uri).await;
    let usage_offset = merge::carrier_position_to_tsx_offset_validated(
        &position,
        &ctx.carrier_line_index,
        &ctx.mapper,
        &ctx.tsx_line_index,
    )
    .expect("usage position should map into TSX");
    // Seed the provider's answer: the CURRENT CARRIER path in fully-folded
    // case (the Windows tsgo spelling), offsets in carrier coordinates.
    let carrier_path = crate::documents::uri_to_canonical_id(&app_uri);
    let folded_carrier_path = carrier_path.to_lowercase();
    let carrier_token_start = app_source.find("{{ vueTsTitle }}").unwrap() as u32 + 3;
    provider.set_rename_locations(
        &ctx.tsx_path,
        usage_offset,
        vec![crate::type_provider::protocol::RenameLocation {
            path: folded_carrier_path.clone(),
            start: carrier_token_start,
            end: carrier_token_start + "vueTsTitle".len() as u32,
        }],
    );

    let edit = super::super::nav_features_navigation::handle_rename(
        server,
        RenameParams {
            text_document_position: TextDocumentPositionParams {
                text_document: TextDocumentIdentifier {
                    uri: app_uri.clone(),
                },
                position,
            },
            new_name: "renamedTitle".into(),
            work_done_progress_params: Default::default(),
        },
    )
    .await
    .expect("rename request should succeed")
    .expect("rename must return the merged edit");

    let changes = edit.changes.expect("workspace edit must carry changes");
    for key in changes.keys() {
        assert_eq!(
            key.as_str(),
            app_uri.as_str(),
            "every edit must be keyed under the AUTHORED uri, never the provider's folded path: {changes:?}"
        );
    }
    let edits = &changes[&app_uri];
    assert_eq!(
        edits.len(),
        2,
        "script declaration + template use must both be edited: {changes:?}"
    );

    drain_handle.abort();
    drop(service);
}

#[tokio::test]
async fn contract_kebab_prop_references_include_child_declaration() {
    // Find-references at the kebab usage must include the child's camel
    // `defineProps` declaration even when the provider enumerates nothing —
    // Verter injects the resolved declaration (the same shared resolution the
    // goto-definition props branch and the rename classification consume).
    let child_source = "<script setup lang=\"ts\">\ndefineProps<{ myProp: string }>()\n</script>\n";
    let parent_source = "<script setup lang=\"ts\">\nimport MyComp from './MyComp.vue'\nconst v = 'x'\n</script>\n<template>\n  <MyComp :my-prop=\"v\" />\n</template>\n";
    let (_temp, service, drain_handle, _provider, workspace_id) = make_definition_test_server(&[
        ("src/MyComp.vue", "vue", child_source),
        ("src/App.vue", "vue", parent_source),
    ])
    .await;

    let app_uri = workspace_uri(&workspace_id, "src/App.vue");
    let child_uri = workspace_uri(&workspace_id, "src/MyComp.vue");
    let server = service.inner();
    let position = find_document_position(server, &app_uri, ":my-prop=", 1);

    let locations = super::super::nav_features_navigation::handle_references(
        server,
        ReferenceParams {
            text_document_position: TextDocumentPositionParams {
                text_document: TextDocumentIdentifier {
                    uri: app_uri.clone(),
                },
                position,
            },
            context: ReferenceContext {
                include_declaration: true,
            },
            work_done_progress_params: Default::default(),
            partial_result_params: Default::default(),
        },
    )
    .await
    .expect("references request should succeed")
    .expect("references must include the injected child declaration");

    let expected_decl = range_for_authored_snippet(server, &child_uri, "myProp");
    assert!(
        locations
            .iter()
            .any(|loc| loc.uri == child_uri && loc.range == expected_decl),
        "references must include the child declaration range-exact: {locations:?}"
    );

    drain_handle.abort();
    drop(service);
}

#[tokio::test]
async fn svelte_render_cross_clause_and_shadow_bindings_preserve_provider_definition() {
    for (_label, source) in [
        (
            "an else branch cannot capture a snippet declared in the main branch",
            "<script>const providerTarget = 1;</script>{#if true}{#snippet row()}{/snippet}{:else}{@render row()}{/if}",
        ),
        (
            "an each binding shadows an outer snippet",
            "<script>const providerTarget = 1;</script>{#snippet row()}{/snippet}{#each [] as row}{@render row()}{/each}",
        ),
    ] {
        let (_temp, service, drain_handle, provider, workspace_id) =
            make_svelte_definition_server(&[("src/App.svelte", source)]).await;
        let app_uri = workspace_uri(&workspace_id, "src/App.svelte");
        assert_svelte_same_file_definition(
            service.inner(),
            &provider,
            &app_uri,
            ("@render row", 8),
            "providerTarget",
        )
        .await;
        drain_handle.abort();
        drop(service);
    }
}

#[tokio::test]
async fn svelte_ambiguous_local_render_snippets_preserve_provider_definition() {
    let source = "<script>const providerTarget = 1;</script>{#snippet row()}{/snippet}{#snippet row()}{/snippet}{@render row()}";
    let (_temp, service, drain_handle, provider, workspace_id) =
        make_svelte_definition_server(&[("src/App.svelte", source)]).await;
    let server = service.inner();
    let app_uri = workspace_uri(&workspace_id, "src/App.svelte");
    let document = server
        .documents
        .get(&app_uri)
        .expect("open Svelte document");
    let offset = document
        .line_index
        .position_to_offset(&find_document_position(server, &app_uri, "@render row", 8))
        .expect("render callee offset");
    let visibility = document.feature_snapshot.as_ref().and_then(|snapshot| {
        crate::documents::carrier_structure::svelte_render_lexical_visibility_at(
            snapshot.structure(),
            offset,
        )
    });
    assert_eq!(
        visibility,
        Some(crate::documents::carrier_structure::SvelteRenderLexicalVisibility::Ambiguous),
        "the planted-provider control must exercise a genuinely ambiguous authored scope"
    );
    drop(document);

    assert_svelte_same_file_definition(
        server,
        &provider,
        &app_uri,
        ("@render row", 8),
        "providerTarget",
    )
    .await;

    drain_handle.abort();
    drop(service);
}

#[tokio::test]
async fn svelte_non_direct_prop_render_tokens_preserve_provider_definition() {
    for (_label, source, query) in [
        (
            "member callee",
            "<script>\nconst providerTarget = 1;\n/** @type {{ children?: () => void }} */\nlet { children } = $props();\nconst obj = { children };\n</script>\n{@render obj.children()}",
            ("obj.children", 5),
        ),
        (
            "render argument",
            "<script>\nconst providerTarget = 1;\n/** @type {{ children?: () => void }} */\nlet { children } = $props();\nconst other = (value) => value;\n</script>\n{@render other(children)}",
            ("other(children)", 7),
        ),
    ] {
        let (_temp, service, drain_handle, provider, workspace_id) =
            make_svelte_definition_server(&[("src/App.svelte", source)]).await;
        let app_uri = workspace_uri(&workspace_id, "src/App.svelte");
        assert_svelte_same_file_definition(
            service.inner(),
            &provider,
            &app_uri,
            query,
            "providerTarget",
        )
        .await;
        drain_handle.abort();
        drop(service);
    }
}

#[tokio::test]
async fn svelte_shadowed_script_prop_render_preserves_provider_definition() {
    for (_label, source) in [
        (
            "each item",
            "<script lang=\"ts\">\nimport type { Snippet } from 'svelte';\nconst providerTarget = 1;\ninterface Props { row?: Snippet }\nlet { row }: Props = $props();\n</script>\n{#each [] as row}{@render row()}{/each}",
        ),
        (
            "await binding",
            "<script>\nconst providerTarget = 1;\n/** @type {{ row?: () => void }} */\nlet { row } = $props();\nconst promise = Promise.resolve(() => {});\n</script>\n{#await promise then row}{@render row()}{/await}",
        ),
        (
            "snippet parameter",
            "<script>\nconst providerTarget = 1;\n/** @type {{ row?: () => void }} */\nlet { row } = $props();\n</script>\n{#snippet wrapper(row)}{@render row()}{/snippet}",
        ),
    ] {
        let (_temp, service, drain_handle, provider, workspace_id) =
            make_svelte_definition_server(&[("src/App.svelte", source)]).await;
        let app_uri = workspace_uri(&workspace_id, "src/App.svelte");
        assert_svelte_same_file_definition(
            service.inner(),
            &provider,
            &app_uri,
            ("@render row", 8),
            "providerTarget",
        )
        .await;
        drain_handle.abort();
        drop(service);
    }
}

#[tokio::test]
async fn svelte_js_render_definition_survives_change_before_template_analysis_rebuild() {
    let initial = "<script>\n  let item = 'x';\n  /** @type {{ rowSnippet?: () => void }} */\n  let { rowSnippet } = $props();\n</script>\n{#snippet rowSnippet(thing)}\n  <li>{thing}</li>\n{/snippet}\n<ul>{@render rowSnippet(item)}</ul>\n";
    let changed = "<script>\n  let item = 'y';\n  /** @type {{ rowSnippet?: () => void }} */\n  let { rowSnippet } = $props();\n</script>\n{#snippet rowSnippet(thing)}\n  <li>{thing}</li>\n{/snippet}\n<ul>{@render rowSnippet(item)}</ul>\n";
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

    assert!(server.documents.did_change(&app_uri, 2, changed).changed);
    assert!(
        server
            .documents
            .get_analysis(&app_uri)
            .and_then(|analysis| analysis.template)
            .is_none(),
        "the regression boundary is the current parser structure before template analysis rebuild"
    );

    let position = find_document_position(server, &app_uri, "@render rowSnippet", 8);
    // Model the provider's already-open current surface without invoking the
    // foreground IDE compile: the unrelated equal-width edit leaves this
    // projection/mapping byte geometry intact, while BUILD/template analysis
    // remains absent exactly as it does in the production startup window.
    let mut state = server
        .provider_sync_state_for_source(&canonical_id)
        .unwrap_or_else(|| ProviderSyncState {
            owner_binding: crate::provider_sync::ProviderOwnerBinding::Unresolved,
            ..Default::default()
        });
    let ide_path = server
        .target_ide_path_for_uri(&app_uri)
        .expect("Svelte JS IDE path");
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
        .expect("current provider surface without a foreground compile");
    let query_offset = merge::carrier_position_to_tsx_offset_validated(
        &position,
        &ctx.carrier_line_index,
        &ctx.mapper,
        &ctx.tsx_line_index,
    )
    .expect("render callee must map into the repaired JSX surface");
    provider.set_definitions(
        &ctx.tsx_path,
        query_offset,
        vec![TypeLocation {
            path: format!("{workspace_id}/node_modules/svelte/index.d.ts"),
            start: 0,
            end: 1,
        }],
    );

    let response = server
        .goto_definition(goto_definition_params(&app_uri, position))
        .await
        .expect("goto definition request")
        .expect("local snippet definition");
    let locations = definition_locations(response);
    let snippet_start = changed.find("{#snippet rowSnippet").unwrap() + "{#snippet ".len();
    let changed_index = LineIndex::new_utf16(changed);
    let expected = Range::new(
        changed_index
            .offset_to_position(snippet_start as u32)
            .expect("local snippet start"),
        changed_index
            .offset_to_position((snippet_start + "rowSnippet".len()) as u32)
            .expect("local snippet end"),
    );
    assert_eq!(
        locations,
        vec![Location {
            uri: app_uri,
            range: expected,
        }],
        "parser-owned local snippet authority must exclude the provider's package declaration"
    );

    drain_handle.abort();
    drop(service);
}

#[tokio::test]
async fn svelte_direct_render_prop_definition_does_not_require_template_analysis() {
    for (label, language, declaration) in [
        (
            "ts",
            " lang=\"ts\"",
            "import type { Snippet } from 'svelte';\ninterface Props { children?: Snippet<[boolean]> }\nlet { children }: Props = $props();",
        ),
        (
            "js",
            "",
            "/** @typedef {import('svelte').Snippet<[boolean]>} Children */\n/** @type {{ children?: Children }} */\nlet { children } = $props();",
        ),
        (
            "js-ordinary-function-prop",
            "",
            "/** @type {{ children?: () => void }} */\nlet { children } = $props();",
        ),
    ] {
        let source = format!(
            "<script{language}>\n{declaration}\n</script>\n{{@render children?.(true)}}\n"
        );
        let (_temp, service, drain_handle, provider, workspace_id) =
            make_default_profile_definition_test_server(&[(
                "src/App.svelte",
                "svelte",
                source.as_str(),
            )])
            .await;
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
        let changed = source.replace("children?.(true)", "children?.(false)");
        assert!(server.documents.did_change(&app_uri, 2, &changed).changed);
        let evidence = server
            .documents
            .host()
            .resolve_svelte_script_facts(&canonical_id);
        let capture = server
            .documents
            .capture_source_feature_document(&app_uri)
            .unwrap_or_else(|| panic!("{label}: current source-feature capture"));
        assert!(
            server
                .documents
                .source_feature_host_revision_is_current(&capture),
            "{label}: captured host revision must be current"
        );
        assert!(
            server
                .documents
                .source_feature_capture_is_current(&app_uri, &capture),
            "{label}: captured document/host identity must be current"
        );
        let binding_span = match &evidence {
            verter_session::framework::script_facts::ScriptFactEvidence::Exact(exact) => {
                exact
                    .facts()
                    .syntax()
                    .props_calls()
                    .iter()
                    .flat_map(|call| call.local_bindings.iter())
                    .find(|binding| binding.name == "children")
                    .unwrap_or_else(|| {
                        panic!(
                            "{label}: test fixture must expose exact authored $props binding geometry"
                        )
                    })
                    .span
            }
            other => panic!(
                "{label}: test fixture must produce exact Svelte facts, got {}",
                match other {
                    verter_session::framework::script_facts::ScriptFactEvidence::Partial(_) =>
                        "partial",
                    verter_session::framework::script_facts::ScriptFactEvidence::Unavailable(_) =>
                        "unavailable",
                    verter_session::framework::script_facts::ScriptFactEvidence::NotApplicable(_) =>
                        "not-applicable",
                    verter_session::framework::script_facts::ScriptFactEvidence::Exact(_) =>
                        unreachable!(),
                }
            ),
        };
        assert!(binding_span.end > binding_span.start);
        assert!(
            server
                .documents
                .get_analysis(&app_uri)
                .and_then(|analysis| analysis.template)
                .is_none(),
            "{label}: regression requires parser facts without template analysis"
        );
        let position = find_document_position(server, &app_uri, "@render children", 8);
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
        .unwrap_or_else(|| panic!("{label}: render callee must map into the provider surface"));
        provider.set_definitions(
            &ctx.tsx_path,
            query_offset,
            vec![TypeLocation {
                path: format!("{workspace_id}/node_modules/svelte/index.d.ts"),
                start: 0,
                end: 1,
            }],
        );

        let response = server
            .goto_definition(goto_definition_params(&app_uri, position))
            .await
            .expect("goto definition request")
            .unwrap_or_else(|| panic!("{label}: incoming snippet prop must resolve to source"));
        let locations = definition_locations(response);
        let changed_index = LineIndex::new_utf16(&changed);
        let expected_range = Range::new(
            changed_index
                .offset_to_position(binding_span.start)
                .unwrap_or_else(|| panic!("{label}: authored binding start")),
            changed_index
                .offset_to_position(binding_span.end)
                .unwrap_or_else(|| panic!("{label}: authored binding end")),
        );
        assert_eq!(
            locations,
            vec![Location {
                uri: app_uri,
                range: expected_range,
            }],
            "{label}: source authority must exclude the planted package definition"
        );

        drain_handle.abort();
        drop(service);
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn svelte_render_definition_discards_a_torn_document_and_host_revision() {
    const SOURCE_A: &str = "<script lang=\"ts\">\nimport type { Snippet } from 'svelte';\ninterface Props { children?: Snippet }\nlet { children }: Props = $props();\n</script>\n{@render children?.()}\n";
    const SOURCE_B: &str = "<script lang=\"ts\">\n// revision B shifts every following host fact\nimport type { Snippet } from 'svelte';\ninterface Props { children?: Snippet }\nlet { children }: Props = $props();\n</script>\n{@render children?.()}\n";

    let (_temp, service, drain_handle, provider, workspace_id) =
        make_default_profile_definition_test_server(&[("src/App.svelte", "svelte", SOURCE_A)])
            .await;
    let server = service.inner();
    let app_uri = workspace_uri(&workspace_id, "src/App.svelte");
    let position = find_document_position(server, &app_uri, "@render children", 8);
    let ctx = synced_type_provider_context(server, &app_uri).await;
    let query_offset = merge::carrier_position_to_tsx_offset_validated(
        &position,
        &ctx.carrier_line_index,
        &ctx.mapper,
        &ctx.tsx_line_index,
    )
    .expect("initial render callee maps to provider surface");
    let provider_path = format!("{workspace_id}/node_modules/svelte/index.d.ts");
    provider.set_definitions(
        &ctx.tsx_path,
        query_offset,
        vec![TypeLocation {
            path: provider_path.clone(),
            start: 0,
            end: 1,
        }],
    );

    let (host_advanced_tx, host_advanced_rx) = std::sync::mpsc::channel();
    let (release_edit_tx, release_edit_rx) = std::sync::mpsc::channel();
    server
        .documents
        .set_before_change_document_reacquire_hook_for_test(Box::new(move |_, _| {
            host_advanced_tx
                .send(())
                .expect("test observes host upsert before document commit");
            release_edit_rx
                .recv_timeout(std::time::Duration::from_secs(10))
                .expect("definition request releases the paused edit");
        }));
    let edit_documents = Arc::clone(server.test_documents());
    let edit_uri = app_uri.clone();
    let edit = std::thread::spawn(move || edit_documents.did_change(&edit_uri, 2, SOURCE_B));
    host_advanced_rx
        .recv_timeout(std::time::Duration::from_secs(10))
        .expect("edit reaches the host-upsert/document-commit boundary");

    let outcome = tokio::time::timeout(
        std::time::Duration::from_secs(5),
        server.goto_definition(goto_definition_params(&app_uri, position)),
    )
    .await;
    release_edit_tx
        .send(())
        .expect("release the document commit after the request finishes");
    assert!(edit.join().expect("edit thread joins").changed);

    let response = outcome
        .expect("definition must not wait for document commit")
        .expect("definition request succeeds");
    assert!(
        response.is_none(),
        "a request must not combine revision-A source geometry with revision-B host facts; \
         the independent provider-surface identity fence also rejects the stale projection"
    );

    drain_handle.abort();
    drop(service);
}

#[tokio::test]
async fn svelte_bind_this_token_produces_no_definition() {
    // The `this` keyword has no declaration — fail closed, never a mis-mapped
    // link (the bound local keeps its own mapped surface).
    let (_temp, service, drain_handle, _provider, workspace_id) =
        make_svelte_definition_server(&[("src/App.svelte", SVELTE_TS_BIND_THIS_SOURCE)]).await;
    let app_uri = workspace_uri(&workspace_id, "src/App.svelte");
    assert_svelte_token_fails_closed(service.inner(), &app_uri, ("bind:this={boxEl}", 5)).await;
    drain_handle.abort();
    drop(service);
}

#[tokio::test]
async fn svelte_valued_class_name_produces_no_definition() {
    // A valued `class:name={cond}` name is a CSS class name, not a binding —
    // no declaration exists for the token, so it must fail closed (the
    // `data-class-*` rewrite keeps only the condition mapped).
    let source = "<script lang=\"ts\">\n  let isActive = true;\n</script>\n<div class:isActive={isActive}>x</div>\n";
    let (_temp, service, drain_handle, _provider, workspace_id) =
        make_svelte_definition_server(&[("src/App.svelte", source)]).await;
    let app_uri = workspace_uri(&workspace_id, "src/App.svelte");
    assert_svelte_token_fails_closed(service.inner(), &app_uri, ("class:isActive={", 6)).await;
    drain_handle.abort();
    drop(service);
}

#[tokio::test]
async fn svelte_valued_style_name_produces_no_definition() {
    // A valued `style:name={value}` name is a CSS property, not a binding — no
    // declaration exists for the token, so it must fail closed (the value
    // keeps its own mapped void-check).
    let source = "<script lang=\"ts\">\n  let accentColor = 'red';\n</script>\n<div style:accentColor={accentColor}>x</div>\n";
    let (_temp, service, drain_handle, _provider, workspace_id) =
        make_svelte_definition_server(&[("src/App.svelte", source)]).await;
    let app_uri = workspace_uri(&workspace_id, "src/App.svelte");
    assert_svelte_token_fails_closed(service.inner(), &app_uri, ("style:accentColor={", 6)).await;
    drain_handle.abort();
    drop(service);
}

#[tokio::test]
async fn svelte_component_on_event_name_produces_no_definition() {
    // A component `on:` event name is projected as a synthetic string literal
    // inside `__verter_event(Child, "pick", h)` — no resolvable symbol and no
    // authored declaration to map back to: native resolution must not
    // fabricate a target and the provider has none either. Fail closed.
    let (_temp, service, drain_handle, _provider, workspace_id) = make_svelte_definition_server(&[
        ("src/Child.svelte", SVELTE_TS_CHILD_SOURCE),
        ("src/App.svelte", SVELTE_TS_PARENT_ON_EVENT_SOURCE),
    ])
    .await;
    let app_uri = workspace_uri(&workspace_id, "src/App.svelte");
    assert_svelte_token_fails_closed(service.inner(), &app_uri, ("on:pick={", 3)).await;
    drain_handle.abort();
    drop(service);
}

#[tokio::test]
async fn svelte_js_component_on_event_name_produces_no_definition() {
    let (_temp, service, drain_handle, _provider, workspace_id) = make_svelte_definition_server(&[
        ("src/Child.svelte", SVELTE_JS_CHILD_SOURCE),
        ("src/App.svelte", SVELTE_JS_PARENT_ON_EVENT_SOURCE),
    ])
    .await;
    let app_uri = workspace_uri(&workspace_id, "src/App.svelte");
    assert_svelte_token_fails_closed(service.inner(), &app_uri, ("on:pick={", 3)).await;
    drain_handle.abort();
    drop(service);
}

#[tokio::test]
async fn goto_type_definition_returns_none_without_provider() {
    let host = Arc::new(VerterHost::new_standalone(HostConfig::default()));
    let (service, _socket) = tower_lsp_server::LspService::new(move |_client| {
        VerterLanguageServer::new(
            crate::outbound::Outbound::default(),
            LspConfig {
                host: Arc::clone(&host),
                type_provider: None,
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
    let drain_handle = tokio::spawn(async move {
        let mut socket = socket;
        while socket.next().await.is_some() {}
    });

    let server = service.inner();
    let source = "<script setup lang=\"ts\">\nconst count: number = 0\n</script>\n";
    let uri: Uri = "file:///test/App.vue".parse().unwrap();
    let _ = server.documents.did_open(&TextDocumentItem {
        uri: uri.clone(),
        language_id: "vue".to_string(),
        version: 1,
        text: source.to_string(),
    });

    let params = GotoDefinitionParams {
        text_document_position_params: TextDocumentPositionParams {
            text_document: TextDocumentIdentifier { uri: uri.clone() },
            position: Position {
                line: 1,
                character: 6,
            },
        },
        work_done_progress_params: Default::default(),
        partial_result_params: Default::default(),
    };
    let result = server
        .goto_type_definition(params)
        .await
        .expect("handler should not error");

    assert!(
        result.is_none(),
        "type definition should return None without a type provider"
    );

    drain_handle.abort();
    drop(service);
}

#[tokio::test]
async fn goto_type_definition_delegates_to_provider() {
    let source = "<script setup lang=\"ts\">\nconst count: number = 0\n</script>\n";
    let (_temp, service, drain_handle, provider, workspace_id) =
        make_definition_test_server(&[("src/App.vue", "vue", source)]).await;

    let app_uri = workspace_uri(&workspace_id, "src/App.vue");
    let server = service.inner();
    let position = find_document_position(server, &app_uri, "count", 0);

    // Set up mock to return a type definition when queried
    {
        let ctx = synced_type_provider_context(server, &app_uri).await;
        if let Some(tsx_offset) = merge::carrier_position_to_tsx_offset_validated(
            &position,
            &ctx.carrier_line_index,
            &ctx.mapper,
            &ctx.tsx_line_index,
        ) {
            // Point the type-definition at the real `count` identifier in the generated TSX so
            // its offsets map back to the `.vue` source. (Offsets in the synthetic preamble,
            // e.g. 0..5, do not map and are correctly dropped fail-closed — they would have
            // collapsed to a line-0 range under the old `.unwrap_or_default()` behavior.)
            provider.set_type_definitions(
                &ctx.tsx_path,
                tsx_offset,
                vec![TypeLocation {
                    path: ctx.tsx_path.clone(),
                    start: tsx_offset,
                    end: tsx_offset + "count".len() as u32,
                }],
            );
        }
    }

    let params = GotoDefinitionParams {
        text_document_position_params: TextDocumentPositionParams {
            text_document: TextDocumentIdentifier {
                uri: app_uri.clone(),
            },
            position,
        },
        work_done_progress_params: Default::default(),
        partial_result_params: Default::default(),
    };
    let result = server
        .goto_type_definition(params)
        .await
        .expect("handler should not error");

    // Verify the provider was called with get_type_definition (not get_definition)
    assert!(
        provider
            .calls()
            .iter()
            .any(|call| matches!(call, MockCall::GetTypeDefinition { .. })),
        "handler should delegate to get_type_definition on the provider"
    );
    assert!(
        !provider
            .calls()
            .iter()
            .any(|call| matches!(call, MockCall::GetDefinition { .. })),
        "handler should NOT call get_definition"
    );

    // The merge logic should produce a response when the provider returns locations
    assert!(
        result.is_some(),
        "type definition should return locations when provider has results"
    );

    drain_handle.abort();
    drop(service);
}

/// W02 liveness (rename ingress): a compile blocked on unavailable input is
/// never memoized as a verdict on its bytes, so a genuinely later request
/// retries it.
///
/// Driven through `handle_rename` itself, so the whole request path is under
/// test — admission, the repair, plan resolution — rather than the internal
/// helper the handler calls:
///
/// Both the open-time attempt and each rename observe a missing external
/// `src=` file. `XUnavailableMacroSemanticResult` and
/// `XMissingMacroSemanticBundle` share the same non-memoized classification,
/// covering scheduler cancellation without requiring a cancellation race in
/// this request-path test.
///
/// Measured on the host's `compile_cold_runs`, not only on calls into the
/// registry accounting ingress: the claim is about compiles the rename
/// ingress causes.
#[tokio::test(flavor = "multi_thread")]
async fn a_later_rename_retries_a_failed_projectionless_revision() {
    let provider: Arc<dyn TypeProvider> = Arc::new(MockTypeProvider::new());
    let service = make_hover_test_service_with_kind(provider, crate::TypeProviderKind::Tsgo);
    let server = service.inner();
    install_test_resolver(server);

    let canonical_id = "/workspace/src/App.vue";
    let uri = open_test_vue(
        server,
        canonical_id,
        "<script setup lang=\"ts\" src=\"./missing.ts\"></script>\n<template><div/></template>\n",
    );
    assert!(
        server.documents.get_projection(&uri).is_none(),
        "precondition: the unavailable external input must leave the carrier projection-less"
    );
    assert!(
        server.current_file_needs_inline_type_provider_sync(&uri),
        "an unavailable-input failure must not bind the open bytes"
    );

    let rename_at = |position: Position| RenameParams {
        text_document_position: TextDocumentPositionParams {
            text_document: TextDocumentIdentifier { uri: uri.clone() },
            position,
        },
        new_name: "renamed".into(),
        work_done_progress_params: Default::default(),
    };
    let position = Position {
        line: 0,
        character: 1,
    };
    let rename_once = || async {
        let edit =
            super::super::nav_features_navigation::handle_rename(server, rename_at(position))
                .await
                .expect("a rename on a broken carrier must not error to the client");
        assert!(
            edit.is_none(),
            "a projection-less broken carrier has no surface to rename against — \
             it fails closed, never a partial or mis-mapped WorkspaceEdit, got {edit:?}"
        );
    };

    let cold_runs = || {
        server
            .documents
            .host()
            .provenance_snapshot()
            .compile_cold_runs
    };
    let cold_before_first = cold_runs();
    rename_once().await;
    let cold_after_first = cold_runs();
    assert_eq!(
        cold_after_first - cold_before_first,
        0,
        "B2 external-content deferral must reject before compiler admission"
    );
    assert!(server.current_file_needs_inline_type_provider_sync(&uri));
    rename_once().await;
    assert_eq!(
        cold_runs() - cold_after_first,
        0,
        "a retryable B2 deferral must remain outside compiler admission"
    );
    assert!(server.current_file_needs_inline_type_provider_sync(&uri));
}

/// No-silent-empty for CTRL+CLICK: a transient provider failure on a MEMBER
/// definition must resync + retry once — the navigation recovers instead of
/// silently returning nothing. This is the definition half of the hover
/// contract above (`hover_recovers_with_resync_and_retry_after_transient_
/// provider_error`): without it, the same perturbed provider state that hover
/// self-heals through leaves every member CTRL+CLICK dead (members have no
/// native fallback), which is exactly the reported "`bar` NEVER works while
/// hover shows the correct type" asymmetry.
#[tokio::test]
async fn definition_recovers_with_resync_and_retry_after_transient_provider_error() {
    for kind in [
        crate::TypeProviderKind::Tsserver,
        crate::TypeProviderKind::Tsgo,
    ] {
        let provider = Arc::new(MockTypeProvider::new());
        let type_provider: Arc<dyn TypeProvider> = provider.clone();
        let service = make_hover_test_service_with_kind(type_provider, kind);
        let server = service.inner();
        install_test_resolver(server);

        let (app_uri, member_position, expected_decl_range) =
            seed_member_definition_fixture(server, &provider, "/workspace/src/App.vue");

        // ONE transient failure, then the provider answers normally.
        provider.fail_next_definitions(1);

        let locs = definition_locations_of(
            server
                .goto_definition(definition_params(&app_uri, member_position))
                .await
                .expect("definition request should succeed"),
        );
        assert!(
            !locs.is_empty(),
            "{kind}: a transient provider error must recover to the member definition, got empty"
        );
        assert!(
            locs.iter()
                .any(|l| l.uri == app_uri && l.range == expected_decl_range),
            "{kind}: the recovered member definition must land on the authored `bar` declaration \
             {expected_decl_range:?}, got: {locs:?}"
        );
        let definition_calls = provider
            .calls()
            .iter()
            .filter(|call| matches!(call, MockCall::GetDefinition { .. }))
            .count();
        assert_eq!(
            definition_calls, 2,
            "{kind}: exactly one retry after the transient failure, got {definition_calls} \
             definition calls"
        );
    }
}

/// Fail-closed bound for the definition retry: a PERSISTENT provider failure
/// retries exactly once after a resync and then returns the native result
/// (None for a member — never a fabricated location, never a spin).
#[tokio::test]
async fn definition_fails_closed_after_bounded_retry_when_provider_keeps_failing() {
    for kind in [
        crate::TypeProviderKind::Tsserver,
        crate::TypeProviderKind::Tsgo,
    ] {
        let provider = Arc::new(MockTypeProvider::new());
        let type_provider: Arc<dyn TypeProvider> = provider.clone();
        let service = make_hover_test_service_with_kind(type_provider, kind);
        let server = service.inner();
        install_test_resolver(server);

        let (app_uri, member_position, _expected_decl_range) =
            seed_member_definition_fixture(server, &provider, "/workspace/src/App.vue");

        provider.fail_next_definitions(16);

        let resp = server
            .goto_definition(definition_params(&app_uri, member_position))
            .await
            .expect("definition request must not error to the client");
        assert!(
            definition_locations_of(resp).is_empty(),
            "{kind}: a persistent provider error fails closed after the bounded retry"
        );
        let definition_calls = provider
            .calls()
            .iter()
            .filter(|call| matches!(call, MockCall::GetDefinition { .. }))
            .count();
        assert_eq!(
            definition_calls, 2,
            "{kind}: retries are bounded to exactly one resync+retry, got {definition_calls} \
             definition calls"
        );
    }
}

/// The type-definition twin of the transient-recovery contract: the governing
/// surface principle names definition AND type-definition, and the same router
/// `NotReady`/restart/IPC perturbation must heal identically for Go to Type
/// Definition on a member position.
#[tokio::test]
async fn type_definition_recovers_with_resync_and_retry_after_transient_provider_error() {
    for kind in [
        crate::TypeProviderKind::Tsserver,
        crate::TypeProviderKind::Tsgo,
    ] {
        let provider = Arc::new(MockTypeProvider::new());
        let type_provider: Arc<dyn TypeProvider> = provider.clone();
        let service = make_hover_test_service_with_kind(type_provider, kind);
        let server = service.inner();
        install_test_resolver(server);

        let (app_uri, member_position, expected_decl_range) =
            seed_member_definition_fixture(server, &provider, "/workspace/src/App.vue");

        // ONE transient failure, then the provider answers normally.
        provider.fail_next_type_definitions(1);

        let locs = definition_locations_of(
            server
                .goto_type_definition(definition_params(&app_uri, member_position))
                .await
                .expect("type-definition request should succeed"),
        );
        assert!(
            !locs.is_empty(),
            "{kind}: a transient provider error must recover to the member type definition, \
             got empty"
        );
        assert!(
            locs.iter()
                .any(|l| l.uri == app_uri && l.range == expected_decl_range),
            "{kind}: the recovered member type definition must land on the authored `bar` \
             declaration {expected_decl_range:?}, got: {locs:?}"
        );
        let type_definition_calls = provider
            .calls()
            .iter()
            .filter(|call| matches!(call, MockCall::GetTypeDefinition { .. }))
            .count();
        assert_eq!(
            type_definition_calls, 2,
            "{kind}: exactly one retry after the transient failure, got {type_definition_calls} \
             type-definition calls"
        );
    }
}

/// Fail-closed bound for the type-definition retry: a PERSISTENT provider
/// failure retries exactly once after a resync and then returns None — never a
/// fabricated location, never a spin.
#[tokio::test]
async fn type_definition_fails_closed_after_bounded_retry_when_provider_keeps_failing() {
    for kind in [
        crate::TypeProviderKind::Tsserver,
        crate::TypeProviderKind::Tsgo,
    ] {
        let provider = Arc::new(MockTypeProvider::new());
        let type_provider: Arc<dyn TypeProvider> = provider.clone();
        let service = make_hover_test_service_with_kind(type_provider, kind);
        let server = service.inner();
        install_test_resolver(server);

        let (app_uri, member_position, _expected_decl_range) =
            seed_member_definition_fixture(server, &provider, "/workspace/src/App.vue");

        provider.fail_next_type_definitions(16);

        let resp = server
            .goto_type_definition(definition_params(&app_uri, member_position))
            .await
            .expect("type-definition request must not error to the client");
        assert!(
            definition_locations_of(resp).is_empty(),
            "{kind}: a persistent provider error fails closed after the bounded retry"
        );
        let type_definition_calls = provider
            .calls()
            .iter()
            .filter(|call| matches!(call, MockCall::GetTypeDefinition { .. }))
            .count();
        assert_eq!(
            type_definition_calls, 2,
            "{kind}: retries are bounded to exactly one resync+retry, got {type_definition_calls} \
             type-definition calls"
        );
    }
}

/// Guard `self_file_rename_and_code_actions_gated_off`: rename and code actions
/// are DEFERRED for a SELF-FILE rune-module own buffer — their workspace-EDIT
/// positions are not yet mapped through the self-file mapper, so an applied edit
/// could land off by the prelude offset (or inside the prelude) and CORRUPT the
/// module. The handlers must be a CLEAN no-op for a rune module (no rename, no
/// actions), NEVER a wrong/unmapped edit, and must NOT query the TypeProvider
/// for rename locations / code actions. (Carrier rename/code-actions unchanged —
/// pinned elsewhere.)
#[tokio::test]
async fn self_file_rename_and_code_actions_gated_off() {
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
        text: "export const count = $state(0);\n".to_string(),
    });
    // Sync the self-file Shadow state so the projection + provider path exist.
    assert!(
        server.sync_self_file_shadow_unresolved(&rune_uri).await,
        "the open rune module syncs its self-file Shadow provider state"
    );
    assert!(
        server.is_self_file_projection(&rune_uri),
        "the rune module must carry a SelfFile projection"
    );

    // Arm the provider with a rename location AND a code action at the rune's
    // OWN canonical path — if the handlers were NOT gated, these would surface
    // as an (unmapped, position-corrupting) workspace edit. Cover the whole
    // buffer offset range so any forwarded request would match.
    provider.set_rename_locations(
        canonical_id,
        0,
        vec![RenameLocation {
            path: canonical_id.to_string(),
            start: 13,
            end: 18,
        }],
    );
    for off in 0..40u32 {
        provider.set_rename_locations(
            canonical_id,
            off,
            vec![RenameLocation {
                path: canonical_id.to_string(),
                start: 13,
                end: 18,
            }],
        );
    }
    provider.set_code_actions(
        canonical_id,
        0,
        u32::MAX,
        vec![TypeCodeAction {
            title: "Convert to named import".to_string(),
            kind: Some("quickfix".to_string()),
            edits: vec![crate::type_provider::protocol::TypeCodeEdit {
                path: canonical_id.to_string(),
                start: 0,
                end: 5,
                new_text: "let".to_string(),
            }],
        }],
    );

    // Rename: must be a CLEAN no-op (no edit), NOT a wrong/unmapped edit.
    let rename = super::super::nav_features_navigation::handle_rename(
        server,
        RenameParams {
            text_document_position: TextDocumentPositionParams {
                text_document: TextDocumentIdentifier {
                    uri: rune_uri.clone(),
                },
                position: Position::new(0, 13),
            },
            new_name: "renamed".to_string(),
            work_done_progress_params: Default::default(),
        },
    )
    .await
    .expect("rename returns Ok");
    assert!(
        rename.is_none(),
        "rename on a rune-module own buffer must be a clean no-op, got {rename:?}"
    );

    // Code actions: must be a CLEAN no-op (no actions).
    let actions = super::super::aux_features::handle_code_action(
        server,
        CodeActionParams {
            text_document: TextDocumentIdentifier {
                uri: rune_uri.clone(),
            },
            range: Range {
                start: Position::new(0, 13),
                end: Position::new(0, 18),
            },
            context: CodeActionContext {
                diagnostics: Vec::new(),
                only: None,
                trigger_kind: None,
            },
            work_done_progress_params: Default::default(),
            partial_result_params: Default::default(),
        },
    )
    .await
    .expect("code_action returns Ok");
    assert!(
        actions.as_ref().map(|a| a.is_empty()).unwrap_or(true),
        "code actions on a rune-module own buffer must be a clean no-op, got {actions:?}"
    );

    // Discriminator: the TypeProvider must NOT have been queried for rename
    // locations or code actions for the rune module — the gate short-circuits
    // BEFORE any forwarded edit-producing request.
    let calls = provider.calls();
    assert!(
        !calls
            .iter()
            .any(|c| matches!(c, MockCall::GetRenameLocations { .. })),
        "rename must NOT forward to the TypeProvider for a rune module, calls={calls:?}"
    );
    assert!(
        !calls
            .iter()
            .any(|c| matches!(c, MockCall::GetCodeActions { .. })),
        "code actions must NOT forward to the TypeProvider for a rune module, calls={calls:?}"
    );

    drain.abort();
}

#[test]
fn declaration_overlay_refcount_release_closes_only_unreferenced_overlays() {
    // `DeclOverlayOwner` is glob-imported from `background_drain_decl_closure` via
    // `use super::*`. This pins the slot-surgery contract of `release_root`:
    // WHICH overlays drain to empty (and are returned for close) when a root closes.

    // Two open roots reach a SHARED declaration overlay; root A also reaches a
    // PRIVATE overlay only it uses. Seed each slot directly (mirroring what a real
    // open records) at a non-zero generation.
    let owner = DeclOverlayOwner::default();
    owner.test_seed_slot("/ws/Shared.d.vue.ts", &["/ws/A.vue", "/ws/B.vue"], 1);
    owner.test_seed_slot("/ws/OnlyA.d.vue.ts", &["/ws/A.vue"], 1);

    // The release returns `DeclCloseTarget` records; the subject under test is WHICH
    // overlays drain — project the returned targets to their paths.
    let close_paths = |targets: Vec<DeclCloseTarget>| -> Vec<String> {
        targets.into_iter().map(|t| t.decl_path).collect()
    };

    // Closing root A: its PRIVATE overlay is now unreferenced (must close); the
    // SHARED overlay is still reached by B (must be retained, NOT returned).
    let to_close = close_paths(owner.release_root("/ws/A.vue"));
    assert_eq!(
        to_close,
        vec!["/ws/OnlyA.d.vue.ts".to_string()],
        "only A's private overlay is unreferenced after A closes, got: {to_close:?}"
    );
    // The shared overlay slot survives, now referenced only by B.
    let shared_roots: Vec<String> = owner
        .test_slot_roots("/ws/Shared.d.vue.ts")
        .expect("shared overlay still tracked")
        .into_iter()
        .collect();
    assert_eq!(
        shared_roots,
        vec!["/ws/B.vue".to_string()],
        "shared overlay now reached only by B"
    );
    // The drained private overlay is kept as a generation TOMBSTONE (empty roots) —
    // the slot is GC'd only when the guarded close confirms the provider close, so a
    // re-open racing the pending close is never lost. Its roots are empty.
    assert_eq!(
        owner.test_slot_roots("/ws/OnlyA.d.vue.ts"),
        Some(HashSet::new()),
        "the drained overlay is kept as an empty tombstone until the close confirms"
    );

    // Closing root B drains the shared overlay → it is now unreferenced.
    let to_close = close_paths(owner.release_root("/ws/B.vue"));
    assert_eq!(
        to_close,
        vec!["/ws/Shared.d.vue.ts".to_string()],
        "the shared overlay closes once its last reaching root (B) closes, got: {to_close:?}"
    );
    // Every slot has drained to an empty tombstone (no live root reaches anything).
    assert!(
        owner
            .test_slots_snapshot()
            .iter()
            .all(|(_, roots)| roots.is_empty()),
        "every overlay slot has drained to empty once all roots close, slots={:?}",
        owner.test_slots_snapshot()
    );

    // Releasing a root that never reached anything closes nothing (no panic). The
    // empty tombstones are not re-returned (their roots were already empty before
    // this release, so this release removed nothing from them).
    let to_close = close_paths(owner.release_root("/ws/Never.vue"));
    assert!(
        to_close.is_empty(),
        "releasing an unknown root closes nothing, got: {to_close:?}"
    );
}

/// A virtual tab is a document the host never ingests, so the readiness basis
/// a virtual-file definition settles against carries no diagnostics
/// generation. Background settlement of the carrier the tab mirrors (or of the
/// tab's own id) during the provider await is therefore not a basis move: the
/// definition answers with the provider's location instead of
/// `ContentModified`. The served surface stays guarded by the provider-surface
/// check, which is what a carrier resync with new content trips.
#[tokio::test(flavor = "multi_thread")]
async fn virtual_file_definition_survives_a_generation_move_during_the_provider_await() {
    let content = "computed\n";
    let (service, provider, virtual_uri, tsx_path) =
        make_virtual_file_fixture(content, content).await;
    let server = service.inner();
    let offset = "comp".len() as u32;
    provider.set_definitions(
        &tsx_path,
        offset,
        vec![crate::type_provider::protocol::TypeLocation {
            path: tsx_path.clone(),
            start: 0,
            end: "computed".len() as u32,
        }],
    );
    let virtual_canonical = crate::documents::uri_to_canonical_id(&virtual_uri);
    let documents = Arc::clone(&server.documents);
    let moved_canonical = virtual_canonical.clone();
    provider.set_on_query(
        &tsx_path,
        Box::new(move || {
            documents
                .host()
                .bump_diagnostics_generation("/workspace/src/App.vue");
            documents
                .host()
                .bump_diagnostics_generation(&moved_canonical);
        }),
    );

    let response = server
        .goto_definition(goto_definition_params(
            &virtual_uri,
            Position {
                line: 0,
                character: offset,
            },
        ))
        .await
        .unwrap_or_else(|error| {
            panic!("a generation move during the provider await must not fail it: {error:?}")
        });

    assert_eq!(
        server
            .documents
            .host()
            .get_diagnostics_generation(&virtual_canonical),
        None,
        "the host never tracks a virtual tab, so its basis has no generation to move"
    );
    let Some(GotoDefinitionResponse::Array(locations)) = response else {
        panic!("the virtual-file definition answers with the provider location, got {response:?}");
    };
    assert_eq!(locations.len(), 1, "{locations:?}");
    assert_eq!(locations[0].range.start, Position::new(0, 0));
    assert_eq!(
        locations[0].range.end,
        Position::new(0, "computed".len() as u32)
    );
}

/// STABLE foreign surface: a provider definition landing in a FOREIGN carrier's
/// IDE surface maps back onto the foreign `.vue` through the surface pinned at
/// request start — guards against the pinned-set resolver over-dropping.
#[tokio::test(flavor = "multi_thread")]
async fn definition_maps_foreign_carrier_location_through_pinned_surface() {
    let (service, _provider, parent_uri, position, _child_ide_path, _child_canonical) =
        make_foreign_mapping_fixture().await;
    let server = service.inner();

    let response = server
        .goto_definition(GotoDefinitionParams {
            text_document_position_params: TextDocumentPositionParams {
                text_document: TextDocumentIdentifier {
                    uri: parent_uri.clone(),
                },
                position,
            },
            work_done_progress_params: Default::default(),
            partial_result_params: Default::default(),
        })
        .await
        .expect("definition request should succeed");
    let locations = match response {
        Some(GotoDefinitionResponse::Array(locs)) => locs,
        Some(GotoDefinitionResponse::Scalar(loc)) => vec![loc],
        other => panic!("expected definition locations, got {other:?}"),
    };
    assert!(
        locations
            .iter()
            .any(|l| l.uri.as_str().ends_with("/Child.vue")),
        "a stable foreign surface must map the child location onto Child.vue, got {locations:?}"
    );
}

/// A FOREIGN carrier re-sync landing a fresh generation with drifted content
/// while the request is awaiting the provider must cause the foreign location
/// to be DROPPED: the provider answered against the surface pinned at request
/// start, and mapping its offsets through the merge-time current surface would
/// land on wrong `.vue` positions (torn, not stale).
#[tokio::test(flavor = "multi_thread")]
async fn definition_drops_foreign_carrier_location_when_foreign_surface_advances_mid_request() {
    let (service, provider, parent_uri, position, child_ide_path, child_canonical) =
        make_foreign_mapping_fixture().await;
    let server = service.inner();

    // Mid-request seam: a concurrent CHILD re-sync lands a fresh generation
    // with DIFFERENT provider content between the request-start pin and the
    // merge. The racing surface's carrier source byte-matches the live child
    // document and carries a usable mapper — exactly the shape a merge-time
    // live-current resolver would ACCEPT and mis-map the provider's
    // request-start offsets through.
    let parent_ctx = synced_type_provider_context(server, &parent_uri).await;
    let store = server.documents.provider_surfaces().clone();
    let raced_child_path = child_ide_path.clone();
    let pinned_child = store
        .current_snapshot(&child_ide_path)
        .expect("child surface current");
    let raced_mapper = pinned_child.source_map.as_ref().map(|m| (**m).clone());
    // Same prefix + trailing drift: the request-start offsets still MAP through
    // the racing surface (a merge-time live-current resolver would serve them),
    // but the content hash differs — the pinned-set resolver must drop.
    let raced_content: String = format!("{}\n// trailing drift", pinned_child.provider_content);
    provider.set_on_query(
        &parent_ctx.tsx_path,
        Box::new(move || {
            store.record(
                crate::provider_surface_store::RecordSurface::carrier_legacy(
                    crate::provider_surface_store::ProviderSurfaceKind::CarrierIde,
                    raced_child_path,
                    child_canonical,
                    Arc::from(raced_content.as_str()),
                    raced_mapper,
                    Arc::from(REQUEST_SURFACE_APP),
                ),
            );
        }),
    );

    let response = server
        .goto_definition(GotoDefinitionParams {
            text_document_position_params: TextDocumentPositionParams {
                text_document: TextDocumentIdentifier {
                    uri: parent_uri.clone(),
                },
                position,
            },
            work_done_progress_params: Default::default(),
            partial_result_params: Default::default(),
        })
        .await
        .expect("definition request should succeed");
    let locations = match response {
        Some(GotoDefinitionResponse::Array(locs)) => locs,
        Some(GotoDefinitionResponse::Scalar(loc)) => vec![loc],
        None => Vec::new(),
        other => panic!("unexpected definition response shape: {other:?}"),
    };
    assert!(
        !locations
            .iter()
            .any(|l| l.uri.as_str().ends_with("/Child.vue")),
        "a foreign location must be DROPPED when the foreign surface advanced \
         mid-request — mapping through the merge-time surface would be wrong, \
         got {locations:?}"
    );
}

/// A FOREIGN carrier surface that moves to different bytes and back while the
/// provider holds the query ends byte- and map-identical to the pinned one, but
/// the provider may have answered against the intermediate surface: the foreign
/// location is dropped, never mapped through the pinned map.
#[tokio::test(flavor = "multi_thread")]
async fn definition_drops_foreign_carrier_location_when_foreign_surface_moves_and_returns() {
    let (service, provider, parent_uri, position, child_ide_path, child_canonical) =
        make_foreign_mapping_fixture().await;
    let server = service.inner();
    let parent_ctx = synced_type_provider_context(server, &parent_uri).await;
    let store = server.documents.provider_surfaces().clone();
    let synced = store
        .current_snapshot(&child_ide_path)
        .expect("child surface current");
    // The surface the request pins is the one the round trip below returns to,
    // byte- and map-identical.
    record_foreign_child_surface(&store, &synced, &child_canonical, &synced.provider_content);
    let pinned = store
        .current_snapshot(&child_ide_path)
        .expect("child surface current");
    let unmoved = server
        .goto_definition(foreign_definition_params(&parent_uri, position))
        .await
        .expect("an unmoved definition succeeds");
    assert!(
        format!("{unmoved:?}").contains("/Child.vue"),
        "the pinned foreign surface maps the location when nothing moves: {unmoved:?}"
    );
    provider.set_on_query(
        &parent_ctx.tsx_path,
        Box::new(move || {
            let drifted = format!("{}\n// drift", pinned.provider_content);
            record_foreign_child_surface(&store, &pinned, &child_canonical, &drifted);
            record_foreign_child_surface(
                &store,
                &pinned,
                &child_canonical,
                &pinned.provider_content,
            );
        }),
    );

    let response = server
        .goto_definition(foreign_definition_params(&parent_uri, position))
        .await
        .expect("definition request should succeed");
    let locations = match response {
        Some(GotoDefinitionResponse::Array(locs)) => locs,
        Some(GotoDefinitionResponse::Scalar(loc)) => vec![loc],
        None => Vec::new(),
        other => panic!("unexpected definition response shape: {other:?}"),
    };
    assert!(
        !locations
            .iter()
            .any(|l| l.uri.as_str().ends_with("/Child.vue")),
        "a foreign surface that changed and changed back during the query must not \
         vouch the provider's location, got {locations:?}"
    );
}

/// A FOREIGN carrier surface a definition location was decoded through stays
/// bracketed until settlement: a change to it after the decode supersedes the
/// answer rather than deliver a location mapped through a replaced surface.
#[tokio::test(flavor = "multi_thread")]
async fn definition_decoded_through_a_foreign_surface_settles_only_while_it_holds() {
    let (service, _provider, parent_uri, position, child_ide_path, child_canonical) =
        make_foreign_mapping_fixture().await;
    let server = service.inner();
    let unmoved = server
        .goto_definition(foreign_definition_params(&parent_uri, position))
        .await
        .expect("an unmoved definition succeeds");
    assert!(
        format!("{unmoved:?}").contains("/Child.vue"),
        "the unmoved definition maps the foreign location: {unmoved:?}"
    );

    let store = server.documents.provider_surfaces().clone();
    let pinned = store
        .current_snapshot(&child_ide_path)
        .expect("child surface current");
    let moved = Arc::new(std::sync::atomic::AtomicBool::new(false));
    {
        let moved = Arc::clone(&moved);
        server.request_barriers().clear();
        server.request_barriers().arm(
            super::super::test_support::RequestBarrier::Settlement,
            Arc::new(move |arrival| {
                if arrival == 0 {
                    let drifted = format!("{}\n// drift", pinned.provider_content);
                    record_foreign_child_surface(&store, &pinned, &child_canonical, &drifted);
                    moved.store(true, std::sync::atomic::Ordering::SeqCst);
                }
                Box::pin(async {})
            }),
        );
    }
    let raced = server
        .goto_definition(foreign_definition_params(&parent_uri, position))
        .await;
    server.request_barriers().clear();
    assert!(moved.load(std::sync::atomic::Ordering::SeqCst));
    assert!(
        matches!(&raced, Err(error) if error.code == tower_lsp_server::jsonrpc::ErrorCode::ContentModified),
        "a definition decoded through a foreign surface that moved before settlement \
         answers ContentModified: {raced:?}"
    );
}

/// REFERENCES runtime drop coverage (representative of the navigation class):
/// a re-sync landing a fresh surface generation while the references request
/// awaits the provider must drop the provider locations (fail closed).
#[tokio::test(flavor = "multi_thread")]
async fn references_drop_provider_locations_when_surface_regenerates_mid_request() {
    let (service, provider, uri) = make_request_surface_carrier().await;
    let server = service.inner();

    // A provider references response at the mapped `msg` position, pointing at
    // the carrier's own IDE surface (would map back into the source).
    let position = find_document_position(server, &uri, "{{ msg", 3);
    let ctx = synced_type_provider_context(server, &uri).await;
    let tsx_offset = merge::carrier_position_to_tsx_offset_validated(
        &position,
        &ctx.carrier_line_index,
        &ctx.mapper,
        &ctx.tsx_line_index,
    )
    .expect("position maps to tsx");
    // The provider's reference points at the STRING LITERAL — a location
    // Verter's own reference analysis never emits, so its presence/absence in
    // the response discriminates the PROVIDER contribution specifically.
    let target = ctx.tsx_content.find("'hello'").expect("token in tsx") as u32;
    provider.set_references(
        &ctx.tsx_path,
        tsx_offset,
        vec![crate::type_provider::protocol::TypeLocation {
            path: ctx.tsx_path.clone(),
            start: target,
            end: target + 7,
        }],
    );

    let ide_path = server.active_ide_path_for_uri(&uri).expect("live IDE path");
    let store = server.documents.provider_surfaces().clone();
    let raced_path = ide_path.clone();
    provider.set_on_query(
        &ide_path,
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

    let response = server
        .references(ReferenceParams {
            text_document_position: TextDocumentPositionParams {
                text_document: TextDocumentIdentifier { uri: uri.clone() },
                position,
            },
            context: ReferenceContext {
                include_declaration: true,
            },
            work_done_progress_params: Default::default(),
            partial_result_params: Default::default(),
        })
        .await
        .expect("references request should succeed");
    // The provider's literal-position location must not survive; Verter-native
    // `msg` references (from its own analysis) may still be present — the
    // invariant is the PROVIDER contribution dropped.
    let literal_pos = find_document_position(server, &uri, "'hello'", 0);
    let has_provider_ref = response.iter().flatten().any(|l| {
        l.uri == uri
            && l.range.start.line == literal_pos.line
            && l.range.start.character == literal_pos.character
    });
    assert!(
        !has_provider_ref,
        "provider references produced against a superseded surface must be DROPPED, \
         got {response:?}"
    );
}

#[tokio::test]
async fn contract_builtin_directive_definition_is_fail_closed_empty() {
    // There is nothing authored to jump to for a built-in directive: the
    // definition stays empty (never a fabricated target).
    for needle in ["v-if", "v-for", "v-show", "v-pre"] {
        let (_temp, service, drain_handle, _provider, workspace_id) =
            make_definition_test_server(&[("src/App.vue", "vue", D6_PARENT_SOURCE)]).await;
        let server = service.inner();
        let uri = workspace_uri(&workspace_id, "src/App.vue");
        let position = find_document_position(server, &uri, needle, 1);
        let response = server
            .goto_definition(goto_definition_params(&uri, position))
            .await
            .expect("goto definition should succeed");
        assert!(
            response.is_none(),
            "{needle} name must have NO definition target (fail-closed), got: {response:?}"
        );
        drain_handle.abort();
        drop(service);
    }
}

#[tokio::test]
async fn contract_svelte_transition_family_names_definition_navigates_to_authored_function() {
    // The authored function name keeps its source map inside the synthetic
    // wrapper, so go-to-definition resolves range-exact to the authored
    // declaration — never the shim, never fail-open.
    let source = "<script lang=\"ts\">\n  function slideAway(node: HTMLElement, params: { duration: number }) {\n    void node;\n    void params;\n    return { duration: 100 };\n  }\n  function enterOnly(node: HTMLElement) {\n    void node;\n    return { duration: 50 };\n  }\n  function exitOnly(node: HTMLElement) {\n    void node;\n    return { duration: 50 };\n  }\n  function shuffleFlip(node: HTMLElement) {\n    void node;\n    return { duration: 200 };\n  }\n</script>\n\n<span transition:slideAway>y</span>\n<b in:enterOnly out:exitOnly>z</b>\n<li animate:shuffleFlip>w</li>\n";
    for (query, target) in [
        (("transition:slideAway", 16usize), "slideAway"),
        (("in:enterOnly", 4usize), "enterOnly"),
        (("out:exitOnly", 5usize), "exitOnly"),
        (("animate:shuffleFlip", 9usize), "shuffleFlip"),
    ] {
        let (_temp, service, drain_handle, provider, workspace_id) =
            make_definition_test_server(&[("src/App.svelte", "svelte", source)]).await;
        let server = service.inner();
        let uri = workspace_uri(&workspace_id, "src/App.svelte");
        assert_svelte_same_file_definition(server, &provider, &uri, query, target).await;
        drain_handle.abort();
        drop(service);
    }
}

#[tokio::test]
async fn contract_svelte_transition_family_keyword_definition_fails_closed() {
    // The keyword token (`transition`, `animate`) is overwritten by synthetic
    // text in the projection: there is no authored target, so definition must
    // fail closed empty — never a fabricated link.
    for query in [("transition:fade", 2usize), ("animate:flip", 2usize)] {
        let (_temp, service, drain_handle, _provider, workspace_id) =
            make_definition_test_server(&[("src/App.svelte", "svelte", D6_SVELTE_SOURCE)]).await;
        let server = service.inner();
        let uri = workspace_uri(&workspace_id, "src/App.svelte");
        assert_svelte_token_fails_closed(server, &uri, query).await;
        drain_handle.abort();
        drop(service);
    }
}

// =====================================================================
// B4: workspace-wide GLOBAL css class references / definition
// =====================================================================

/// A class declared in a NON-scoped block is global: find-all-references from
/// its style declaration spans the workspace (declarations + usages).
#[tokio::test]
async fn global_css_class_references_span_workspace() {
    let a_source = "<template>\n  <div class=\"btn\"></div>\n</template>\n<style>\n.btn { color: red; }\n</style>\n";
    let b_source = "<template>\n  <button class=\"btn\"></button>\n</template>\n";
    let (_temp, service, drain_handle, _provider, workspace_id) = make_definition_test_server(&[
        ("src/A.vue", "vue", a_source),
        ("src/B.vue", "vue", b_source),
    ])
    .await;

    let a_uri = workspace_uri(&workspace_id, "src/A.vue");
    let b_uri = workspace_uri(&workspace_id, "src/B.vue");
    let server = service.inner();
    // Cursor on "btn" in the style selector `.btn`.
    let position = find_document_position(server, &a_uri, ".btn { color", 1);

    let response = server
        .references(ReferenceParams {
            text_document_position: TextDocumentPositionParams {
                text_document: TextDocumentIdentifier { uri: a_uri.clone() },
                position,
            },
            context: ReferenceContext {
                include_declaration: true,
            },
            work_done_progress_params: Default::default(),
            partial_result_params: Default::default(),
        })
        .await
        .expect("references request should succeed")
        .expect("global class must have references");

    assert!(
        response.iter().any(|l| l.uri == a_uri),
        "same-file references present: {response:?}"
    );
    let b_ref = response
        .iter()
        .find(|l| l.uri == b_uri)
        .expect("global class references must reach B.vue's template usage");
    assert_eq!(
        b_ref.range.start.line,
        line_for_snippet(b_source, "class=\"btn\""),
        "B.vue reference must be the exact class token line"
    );

    drain_handle.abort();
    drop(service);
}

/// NEGATIVE: a class declared only in a SCOPED block never produces
/// cross-file references — B.vue's same-named usage is untouched.
#[tokio::test]
async fn scoped_css_class_references_stay_same_file() {
    let a_source = "<template>\n  <div class=\"btn\"></div>\n</template>\n<style scoped>\n.btn { color: red; }\n</style>\n";
    let b_source = "<template>\n  <button class=\"btn\"></button>\n</template>\n";
    let (_temp, service, drain_handle, _provider, workspace_id) = make_definition_test_server(&[
        ("src/A.vue", "vue", a_source),
        ("src/B.vue", "vue", b_source),
    ])
    .await;

    let a_uri = workspace_uri(&workspace_id, "src/A.vue");
    let server = service.inner();
    let position = find_document_position(server, &a_uri, ".btn { color", 1);

    let response = server
        .references(ReferenceParams {
            text_document_position: TextDocumentPositionParams {
                text_document: TextDocumentIdentifier { uri: a_uri.clone() },
                position,
            },
            context: ReferenceContext {
                include_declaration: true,
            },
            work_done_progress_params: Default::default(),
            partial_result_params: Default::default(),
        })
        .await
        .expect("references request should succeed");

    let locations = response.unwrap_or_default();
    assert!(
        locations.iter().all(|l| l.uri == a_uri),
        "a SCOPED class must never cross the file boundary: {locations:?}"
    );

    drain_handle.abort();
    drop(service);
}

/// NEGATIVE: a `<style module>` class is NEVER a cross-file reference
/// origin. Module classes compile to hashed local names (`$style.*` owned by
/// the TS surface) — references from the module declaration must not reach
/// B.vue's same-named global class or usage.
#[tokio::test]
async fn module_css_class_references_never_cross_files() {
    let a_source = "<template>\n  <div class=\"btn\"></div>\n</template>\n<style module>\n.btn { color: red; }\n</style>\n";
    let b_source = "<template>\n  <button class=\"btn\"></button>\n</template>\n<style>\n.btn { margin: 0; }\n</style>\n";
    let (_temp, service, drain_handle, _provider, workspace_id) = make_definition_test_server(&[
        ("src/A.vue", "vue", a_source),
        ("src/B.vue", "vue", b_source),
    ])
    .await;

    let a_uri = workspace_uri(&workspace_id, "src/A.vue");
    let server = service.inner();
    let position = find_document_position(server, &a_uri, ".btn { color", 1);

    let response = server
        .references(ReferenceParams {
            text_document_position: TextDocumentPositionParams {
                text_document: TextDocumentIdentifier { uri: a_uri.clone() },
                position,
            },
            context: ReferenceContext {
                include_declaration: true,
            },
            work_done_progress_params: Default::default(),
            partial_result_params: Default::default(),
        })
        .await
        .expect("references request should succeed");

    let locations = response.unwrap_or_default();
    assert!(
        locations.iter().all(|l| l.uri == a_uri),
        "a `<style module>` class must never cross the file boundary: {locations:?}"
    );

    drain_handle.abort();
    drop(service);
}

/// NEGATIVE: a `<style module>` declaration in ANOTHER file is never served
/// as a cross-file definition target for a global class of the same name.
#[tokio::test]
async fn module_css_class_declaration_is_never_a_cross_file_definition_target() {
    // A.vue declares `.shared` in a MODULE block; B.vue declares and uses a
    // GLOBAL `.shared`. Definition from B's usage walks the workspace but
    // must not surface A's module declaration.
    let a_source =
        "<template>\n  <p></p>\n</template>\n<style module>\n.shared { color: red; }\n</style>\n";
    let b_source = "<template>\n  <div class=\"shared\"></div>\n</template>\n<style>\n.shared { margin: 0; }\n</style>\n";
    let (_temp, service, drain_handle, _provider, workspace_id) = make_definition_test_server(&[
        ("src/A.vue", "vue", a_source),
        ("src/B.vue", "vue", b_source),
    ])
    .await;

    let a_uri = workspace_uri(&workspace_id, "src/A.vue");
    let b_uri = workspace_uri(&workspace_id, "src/B.vue");
    let server = service.inner();
    let position = find_document_position(server, &b_uri, "class=\"shared\"", 8);

    let response = server
        .goto_definition(goto_definition_params(&b_uri, position))
        .await
        .expect("goto definition should succeed")
        .expect("B's own global declaration must resolve");
    let locations = definition_locations(response);
    assert!(
        locations.iter().any(|l| l.uri == b_uri),
        "own declaration present: {locations:?}"
    );
    assert!(
        locations.iter().all(|l| l.uri != a_uri),
        "a `<style module>` declaration must never be a cross-file target: {locations:?}"
    );

    drain_handle.abort();
    drop(service);
}

/// Definition on a global class usage includes global declarations from
/// OTHER files.
#[tokio::test]
async fn global_css_class_definition_reaches_other_files_declarations() {
    let a_source = "<template>\n  <div class=\"shared\"></div>\n</template>\n<style>\n.shared { color: red; }\n</style>\n";
    let b_source =
        "<template>\n  <p></p>\n</template>\n<style>\n.shared { margin: 0; }\n</style>\n";
    let (_temp, service, drain_handle, _provider, workspace_id) = make_definition_test_server(&[
        ("src/A.vue", "vue", a_source),
        ("src/B.vue", "vue", b_source),
    ])
    .await;

    let a_uri = workspace_uri(&workspace_id, "src/A.vue");
    let b_uri = workspace_uri(&workspace_id, "src/B.vue");
    let server = service.inner();
    let position = find_document_position(server, &a_uri, "class=\"shared\"", 8);

    let response = server
        .goto_definition(goto_definition_params(&a_uri, position))
        .await
        .expect("goto definition should succeed")
        .expect("global class must resolve");
    let locations = definition_locations(response);
    assert!(
        locations.iter().any(|l| l.uri == a_uri),
        "own declaration present: {locations:?}"
    );
    let b_decl = locations
        .iter()
        .find(|l| l.uri == b_uri)
        .expect("global class definition must include B.vue's declaration");
    assert_eq!(
        b_decl.range.start.line,
        line_for_snippet(b_source, ".shared { margin"),
        "B.vue target must be the declaration token line"
    );

    drain_handle.abort();
    drop(service);
}

/// NEGATIVE interference guard: a TS identifier position is untouched by the
/// css leg — references on a script binding still resolve normally.
#[tokio::test]
async fn global_css_leg_does_not_shadow_script_references() {
    let a_source = "<script setup lang=\"ts\">\nconst count = 1\nconsole.log(count)\n</script>\n<style>\n.btn { color: red; }\n</style>\n";
    let (_temp, service, drain_handle, _provider, workspace_id) =
        make_definition_test_server(&[("src/A.vue", "vue", a_source)]).await;

    let a_uri = workspace_uri(&workspace_id, "src/A.vue");
    let server = service.inner();
    let position = find_document_position(server, &a_uri, "const count", 6);

    let response = server
        .references(ReferenceParams {
            text_document_position: TextDocumentPositionParams {
                text_document: TextDocumentIdentifier { uri: a_uri.clone() },
                position,
            },
            context: ReferenceContext {
                include_declaration: true,
            },
            work_done_progress_params: Default::default(),
            partial_result_params: Default::default(),
        })
        .await
        .expect("references request should succeed")
        .expect("script binding references resolve");
    assert!(
        response.len() >= 2,
        "declaration + usage of `count` expected: {response:?}"
    );

    drain_handle.abort();
    drop(service);
}

/// Real-pipeline probe: a svelte carrier's class token navigates to its
/// component style rule through the REAL host analysis (markup tokens +
/// scanned styles must survive the served snapshot).
#[tokio::test]
async fn svelte_real_pipeline_class_definition_reaches_style_rule() {
    let source = "<script lang=\"ts\">\n  let on = true;\n</script>\n\n<div class=\"chip-live\" class:on>chip</div>\n\n<style>\n  .chip-live {\n    color: red;\n  }\n  .on {\n    font-weight: bold;\n  }\n</style>\n";
    let (_temp, service, drain_handle, _provider, workspace_id) =
        make_definition_test_server(&[("src/CssIntel.svelte", "svelte", source)]).await;

    let uri = workspace_uri(&workspace_id, "src/CssIntel.svelte");
    let server = service.inner();

    let analysis = server
        .documents
        .get_analysis(&uri)
        .expect("svelte analysis must serve");
    assert!(
        !analysis.styles.is_empty(),
        "served svelte snapshot must carry style analyses"
    );
    assert!(
        !analysis.markup_class_tokens.is_empty(),
        "served svelte snapshot must carry markup class tokens"
    );

    let position = find_document_position(server, &uri, "class=\"chip-live\"", 8);
    let response = server
        .goto_definition(goto_definition_params(&uri, position))
        .await
        .expect("goto definition should succeed")
        .expect("svelte class token must navigate to its rule");
    let locations = definition_locations(response);
    assert!(
        locations.iter().any(|l| l.uri == uri),
        "definition targets the component style rule: {locations:?}"
    );
    let target = &locations[0];
    assert_eq!(
        target.range.start.line,
        line_for_snippet(source, ".chip-live {"),
        "definition lands on the .chip-live rule line"
    );

    drain_handle.abort();
    drop(service);
}

/// Default-profile Vue CSS definition uses template data emitted by the normal
/// IDE projection compile even though optional semantic analysis is disabled.
#[tokio::test]
async fn vue_default_profile_class_definition_reaches_style_rule() {
    let source = "<template>\n  <div class=\"chip-live\">chip</div>\n</template>\n\n<style>\n.chip-live {\n  color: red;\n}\n</style>\n";
    let (_temp, service, drain_handle, _provider, workspace_id) =
        make_default_profile_definition_test_server(&[("src/CssIntel.vue", "vue", source)]).await;

    let uri = workspace_uri(&workspace_id, "src/CssIntel.vue");
    let server = service.inner();
    assert!(
        !server.documents.semantic_analysis_enabled(),
        "the production default must leave optional semantic analysis off"
    );
    assert_eq!(
        server.documents.host().config().effective_scope(),
        verter_semantic::analysis::AnalysisScope::BUILD,
        "the projection host must use the production BUILD scope"
    );

    let analysis = server
        .documents
        .get_analysis(&uri)
        .expect("BUILD projection analysis must serve");
    let template = analysis
        .template
        .as_deref()
        .expect("the IDE projection's TEMPLATE_DATA target must publish Vue template facts");
    assert!(
        template
            .elements
            .iter()
            .any(|element| element.static_classes().any(|class| class == "chip-live")),
        "the compiled template data must carry the authored class"
    );
    assert!(
        analysis.markup_class_tokens.is_empty(),
        "Vue currently has no parse-domain markup class-token producer"
    );
    assert!(
        analysis.styles.iter().any(|style| {
            style
                .css
                .as_ref()
                .is_some_and(|css| css.classes.iter().any(|class| class.name == "chip-live"))
        }),
        "the declaring style rule must still be present"
    );

    let position = find_document_position(server, &uri, "class=\"chip-live\"", 8);
    let response = server
        .goto_definition(goto_definition_params(&uri, position))
        .await
        .expect("goto definition should succeed")
        .expect("Vue class token should navigate to its style rule");
    let locations = definition_locations(response);
    assert_eq!(
        locations[0].range.start.line,
        line_for_snippet(source, ".chip-live {"),
        "definition lands on the .chip-live rule line"
    );

    drain_handle.abort();
    drop(service);
}

/// Default-profile Svelte CSS definition uses parse-domain markup class tokens
/// and therefore remains available without optional semantic analysis.
#[tokio::test]
async fn svelte_default_profile_class_definition_reaches_style_rule() {
    let source = "<script lang=\"ts\">\n  let on = true;\n</script>\n\n<div class=\"chip-live\" class:on>chip</div>\n\n<style>\n  .chip-live {\n    color: red;\n  }\n</style>\n";
    let (_temp, service, drain_handle, _provider, workspace_id) =
        make_default_profile_definition_test_server(&[("src/CssIntel.svelte", "svelte", source)])
            .await;

    let uri = workspace_uri(&workspace_id, "src/CssIntel.svelte");
    let server = service.inner();
    assert!(
        !server.documents.semantic_analysis_enabled(),
        "the production default must leave optional semantic analysis off"
    );
    assert_eq!(
        server.documents.host().config().effective_scope(),
        verter_semantic::analysis::AnalysisScope::BUILD,
        "the projection host must use the production BUILD scope"
    );

    let analysis = server
        .documents
        .get_analysis(&uri)
        .expect("BUILD projection analysis must serve");
    assert!(
        analysis
            .template
            .as_ref()
            .is_none_or(|template| template.elements.is_empty()),
        "Svelte class navigation must not rely on a template element inventory"
    );
    assert!(
        analysis
            .markup_class_tokens
            .iter()
            .any(|token| token.name == "chip-live"),
        "Svelte parse-domain markup tokens must survive the served snapshot"
    );
    assert!(
        analysis.styles.iter().any(|style| {
            style
                .css
                .as_ref()
                .is_some_and(|css| css.classes.iter().any(|class| class.name == "chip-live"))
        }),
        "the declaring style rule must be present"
    );

    let position = find_document_position(server, &uri, "class=\"chip-live\"", 8);
    let response = server
        .goto_definition(goto_definition_params(&uri, position))
        .await
        .expect("goto definition should succeed")
        .expect("Svelte class token must navigate to its style rule");
    let locations = definition_locations(response);
    assert_eq!(
        locations[0].range.start.line,
        line_for_snippet(source, ".chip-live {"),
        "definition lands on the .chip-live rule line"
    );

    drain_handle.abort();
    drop(service);
}

// ===========================================================================
// Always-on production request deadline.
//
// The per-method timeout must apply on the production path, not only inside the
// audit harness: with audit disabled (the production default) a handler body that
// ran unbounded let a wedged type provider park the handler forever. These prove
// the handler fails closed on the production deadline, and that an unrelated
// request is still dispatched while wedged handlers are outstanding.
// ===========================================================================

/// A wedged provider must not park the production definition handler. Audit is OFF
/// (production default); the mock's `get_definition` hangs forever; the handler
/// must return `request_cancelled` within the production deadline.
#[tokio::test]
async fn production_definition_handler_fails_closed_when_the_provider_wedges() {
    let mut config = HostConfig::default();
    assert!(
        !config.audit_enabled,
        "T2 must exercise the production audit-off path"
    );
    config.lsp_method_timeouts.request_deadlines.goto_definition =
        std::time::Duration::from_millis(300);
    let host = Arc::new(VerterHost::new_standalone(config));

    let provider = Arc::new(MockTypeProvider::new());
    provider.hang_definition();
    let type_provider: Arc<dyn TypeProvider> = provider.clone();
    let host_for_server = Arc::clone(&host);
    let provider_for_server = Arc::clone(&type_provider);
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

    let source = "<script setup lang=\"ts\">\nconst count = 1\n</script>\n<template><div>{{ count }}</div></template>\n";
    let uri = open_test_vue(server, "/workspace/src/App.vue", source);
    let position = find_document_position(server, &uri, "{{ count", 3);

    // Production-shaped readiness: surface committed (the open path's sync) and
    // the DependencyReady receipt settled, so the definition actually reaches
    // the wedged provider hop this test characterizes.
    server.ensure_current_file_synced(&uri).await;
    server.publish_import_dependencies_settled(&uri).await;

    let outcome = tokio::time::timeout(
        std::time::Duration::from_secs(5),
        super::super::nav_features_audit::handle_goto_definition_with_audit(
            server,
            goto_definition_params(&uri, position),
        ),
    )
    .await;

    let result = outcome
        .expect("the definition handler must return within its production deadline, never wedge");
    let err = result.expect_err("a wedged provider must fail the request closed");
    assert_eq!(
        err.code,
        tower_lsp_server::jsonrpc::ErrorCode::RequestCancelled,
        "a deadline expiry must surface as request_cancelled, got {err:?}"
    );
    assert!(
        provider
            .calls()
            .iter()
            .any(|c| matches!(c, MockCall::GetDefinition { .. })),
        "the handler must have actually reached the wedged get_definition (else the deadline is untested)"
    );
}

/// Control requests (`$/verter/getStatistics`) must stay responsive while a burst
/// of wedged semantic handlers is in flight. Drives the REAL tower-lsp serve loop
/// over duplex pipes: with a low serve concurrency and no per-request deadline, a
/// handful of wedged definition handlers exhausted every slot, the server stopped
/// reading client stdin, and getStatistics never dispatched at all.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn get_statistics_stays_live_under_a_burst_of_wedged_definitions() {
    use tokio::io::AsyncReadExt;

    // Production audit-off path, but a 2s deadline so the wedged definitions hold
    // their serve-loop slots long enough that a starved getStatistics would miss
    // the 1s liveness bar.
    let mut config = HostConfig::default();
    config.lsp_method_timeouts.request_deadlines.goto_definition =
        std::time::Duration::from_secs(2);
    let host = Arc::new(VerterHost::new_standalone(config));

    let provider = Arc::new(MockTypeProvider::new());
    provider.hang_definition();
    let type_provider: Arc<dyn TypeProvider> = provider.clone();
    let host_for_server = Arc::clone(&host);
    let provider_for_server = Arc::clone(&type_provider);
    let (service, _socket) = tower_lsp_server::LspService::build(move |_client| {
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
    })
    .custom_method(
        "$/verter/getStatistics",
        VerterLanguageServer::get_statistics,
    )
    .finish();

    // Pre-open the document directly so definition dispatch never races didOpen.
    let source = "<script setup lang=\"ts\">\nconst count = 1\n</script>\n<template><div>{{ count }}</div></template>\n";
    let uri = open_test_vue(service.inner(), "/workspace/src/App.vue", source);
    let position = find_document_position(service.inner(), &uri, "{{ count", 3);

    // Wire the serve loop over duplex pipes: client writes → server stdin,
    // server stdout → client reads.
    let (server_stdin_read, client_to_server) = tokio::io::duplex(1 << 16);
    let (server_to_client, client_stdout_read) = tokio::io::duplex(1 << 16);
    tokio::spawn(async move {
        let outbound = service.inner().outbound().clone();
        crate::outbound::serve(server_stdin_read, server_to_client, service, outbound).await;
    });

    let writer = Arc::new(tokio::sync::Mutex::new(client_to_server));

    // Reader: parse frames, record response arrival instants by id, and
    // auto-respond to any server→client request so nothing stalls.
    let (resp_tx, mut resp_rx) =
        tokio::sync::mpsc::unbounded_channel::<(i64, std::time::Instant)>();
    let reader_writer = Arc::clone(&writer);
    tokio::spawn(async move {
        let mut reader = tokio::io::BufReader::new(client_stdout_read);
        loop {
            // Read headers.
            let mut content_length = 0usize;
            let mut line = String::new();
            loop {
                line.clear();
                use tokio::io::AsyncBufReadExt;
                let n = match reader.read_line(&mut line).await {
                    Ok(0) | Err(_) => return,
                    Ok(n) => n,
                };
                let _ = n;
                let trimmed = line.trim();
                if trimmed.is_empty() {
                    break;
                }
                if let Some(len) = trimmed.strip_prefix("Content-Length:") {
                    content_length = len.trim().parse().unwrap_or(0);
                }
            }
            let mut body = vec![0u8; content_length];
            if reader.read_exact(&mut body).await.is_err() {
                return;
            }
            let Ok(msg) = serde_json::from_slice::<serde_json::Value>(&body) else {
                continue;
            };
            let has_method = msg.get("method").is_some();
            let id = msg.get("id").and_then(|v| v.as_i64());
            match (has_method, id) {
                (true, Some(id)) => {
                    // Server→client request: auto-respond null.
                    let reply = serde_json::json!({"jsonrpc":"2.0","id":id,"result":null});
                    write_lsp_frame(&reader_writer, &reply).await;
                }
                (false, Some(id)) => {
                    let _ = resp_tx.send((id, std::time::Instant::now()));
                }
                _ => {}
            }
        }
    });

    // Initialize handshake.
    write_lsp_frame(
        &writer,
        &serde_json::json!({
            "jsonrpc":"2.0","id":0,"method":"initialize",
            "params": {"processId": null, "rootUri": null, "capabilities": {}}
        }),
    )
    .await;
    // Wait for the initialize response (id=0).
    let init_ok = tokio::time::timeout(std::time::Duration::from_secs(5), async {
        while let Some((id, _)) = resp_rx.recv().await {
            if id == 0 {
                return true;
            }
        }
        false
    })
    .await;
    assert!(matches!(init_ok, Ok(true)), "server must answer initialize");
    write_lsp_frame(
        &writer,
        &serde_json::json!({"jsonrpc":"2.0","method":"initialized","params":{}}),
    )
    .await;

    // Fire a burst of definition requests (they wedge on the mock, holding slots
    // for the full 2s production deadline), then immediately getStatistics.
    for id in 1..=8i64 {
        write_lsp_frame(
            &writer,
            &serde_json::json!({
                "jsonrpc":"2.0","id":id,"method":"textDocument/definition",
                "params": {
                    "textDocument": {"uri": uri.as_str()},
                    "position": {"line": position.line, "character": position.character}
                }
            }),
        )
        .await;
    }
    let stats_sent = std::time::Instant::now();
    write_lsp_frame(
        &writer,
        &serde_json::json!({"jsonrpc":"2.0","id":1000,"method":"$/verter/getStatistics","params":{}}),
    )
    .await;

    // getStatistics must answer well before the 2s definition deadlines.
    let stats = tokio::time::timeout(std::time::Duration::from_millis(1000), async {
        while let Some((id, at)) = resp_rx.recv().await {
            if id == 1000 {
                return Some(at);
            }
        }
        None
    })
    .await;
    let arrived = stats
        .expect("getStatistics must not be starved by wedged definition handlers")
        .expect("response stream closed before getStatistics answered");
    assert!(
        arrived.duration_since(stats_sent) < std::time::Duration::from_secs(1),
        "getStatistics answered too slowly ({:?}) — control requests are being starved",
        arrived.duration_since(stats_sent)
    );
}

// ===========================================================================
// Definition-latency freshness gate (DependencyReady receipt).
//
// Every go-to-definition re-ran the imported-carrier + barrel preamble and
// re-pushed byte-identical carrier companions (`.vue.tsx` + `.vue.ts`) as
// full-text didChange, bumping the LSP version and invalidating the engine's
// whole program → a project-scale re-check on the next query. Delivery is now
// BACKGROUND-owned: a request's readiness miss enqueues one publication, the
// publication mints the DependencyReady receipt, and every later request on an
// unchanged document CAPTURES the receipt — so the steady-state definition
// path emits ZERO redundant sync. `ProjectSync::synced_tsx_contents` is a
// record-only evidence ledger of the bytes the engine last received — it does no
// compare-and-skip of its own, so the receipt is what makes the steady state quiet.
// ===========================================================================

/// After the background publication settles (DependencyReady minted), every
/// identical go-to-definition performs ZERO carrier-companion `update_file` /
/// `open_file` / `load_file` — the receipt capture skips the delivery pass that
/// would re-push them. Uses Tsgo so the DirectOpen arm actually delivers
/// companion content (tsserver suppresses it entirely).
#[tokio::test]
async fn definition_second_identical_request_performs_zero_carrier_resync() {
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

    let app_uri = workspace_uri(&workspace_id, "src/App.vue");
    let server = service.inner();
    let position = find_document_position(server, &app_uri, "@custom=\"handleCustom\"", 1);

    // Cold request: answers natively and ENQUEUES the background publication
    // that opens + syncs the carrier companions into the provider.
    let first = server
        .goto_definition(goto_definition_params(&app_uri, position))
        .await
        .expect("first goto definition succeeds")
        .expect("component event resolves on the first request");
    assert!(
        !definition_locations(first).is_empty(),
        "the cold request must resolve to the child defineEmits"
    );
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
        "the readiness miss must enqueue a background publication that mints \
         DependencyReady",
    );

    // Count carrier-sync verbs performed AFTER the receipt is committed.
    let is_sync_verb = |c: &MockCall| {
        matches!(
            c,
            MockCall::OpenFile { .. } | MockCall::UpdateFile { .. } | MockCall::LoadFile { .. }
        )
    };
    let before = provider.calls().iter().filter(|c| is_sync_verb(c)).count();
    // Positive control: the settled publication DID push companions. Without
    // this, a fixture that never syncs companions at all would satisfy the
    // zero-delta assertion below and the receipt would be untested.
    assert!(
        before > 0,
        "the background publication must actually have pushed carrier companions, \
         else the zero-resync assertion below is vacuous"
    );

    let second = server
        .goto_definition(goto_definition_params(&app_uri, position))
        .await
        .expect("second goto definition succeeds")
        .expect("component event still resolves on the repeat request");
    assert!(
        !definition_locations(second).is_empty(),
        "the freshness gate must not change the answer"
    );

    let after = provider.calls().iter().filter(|c| is_sync_verb(c)).count();
    let new_syncs: Vec<_> = provider
        .calls()
        .into_iter()
        .filter(|c| is_sync_verb(c))
        .skip(before)
        .collect();
    assert_eq!(
        after - before,
        0,
        "a definition after DependencyReady must re-push ZERO carrier companions \
         (byte-identical didChange invalidates the engine program); got: {new_syncs:?}"
    );

    drain_handle.abort();
    drop(service);
}

/// A definition CANCELLED by its request deadline must not prevent
/// DependencyReady from being minted once the import-set work completes.
///
/// This is the cancel-loop root: the request-started preamble ran INSIDE the
/// deadline-cancelled handler body, so the deadline dropped the pass mid-flight
/// and the freshness memo (whose publication requires a COMPLETE pass) was
/// never recorded — the next identical request repeated the identical storm.
/// Background-owned publication survives the request: the receipt must appear
/// after the blocked child sync is released, with NO further request.
#[tokio::test]
async fn cancelled_definition_does_not_prevent_dependency_ready_publication() {
    let mut deadlines = verter_session::LspMethodBudgets::interactive_defaults();
    deadlines.goto_definition = std::time::Duration::from_millis(200);
    let (_temp, service, drain_handle, provider, workspace_id) =
        make_definition_test_server_with_kind_and_deadlines(
            &[
                ("src/MyComp.vue", "vue", READINESS_CHILD_SOURCE),
                ("src/App.vue", "vue", READINESS_PARENT_SOURCE),
            ],
            crate::TypeProviderKind::Tsgo,
            deadlines,
        )
        .await;

    let server = service.inner();
    let app_uri = workspace_uri(&workspace_id, "src/App.vue");
    let child_id = format!("{workspace_id}/src/MyComp.vue");
    let child_ide_path =
        verter_session_query::resolution::carrier_ide_provider_path(&child_id, false);

    // Park the import-set pass INSIDE the child companion open, past the
    // definition deadline.
    let (arrived, release) = provider.block_open_file(&child_ide_path);

    let position = find_document_position(server, &app_uri, "@custom=\"handleCustom\"", 1);
    let _ = server
        .goto_definition(goto_definition_params(&app_uri, position))
        .await;

    // The import-set pass must actually have REACHED the blocked child open
    // (otherwise the release below proves nothing).
    tokio::time::timeout(std::time::Duration::from_secs(5), arrived.notified())
        .await
        .expect("the import-set pass must reach the blocked child companion open");
    assert_eq!(
        server.import_sync.recorded_len(),
        0,
        "no DependencyReady receipt may exist while the child companion sync is blocked"
    );

    // Release the blocked child open. The definition request is long gone
    // (cancelled or answered); ONLY background-owned publication can finish the
    // pass and mint the receipt now.
    release.notify_one();

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
        "DependencyReady must be minted by background completion after the blocked \
         child sync is released — a cancelled definition must not kill the pass \
         (the cancel-loop root: no receipt, so the next request repeats the storm)",
    );

    drain_handle.abort();
    drop(service);
}

/// With the dependency receipt MISSING and the publication lane busy, the
/// definition handler must NOT start (or inline-join) the import-set pass: it
/// answers within its deadline from what it has, pushes ZERO import companions
/// from its own turn, and the miss heals through a BACKGROUND enqueue.
#[tokio::test]
async fn definition_returns_fast_without_starting_import_set_when_missing() {
    let mut deadlines = verter_session::LspMethodBudgets::interactive_defaults();
    deadlines.goto_definition = std::time::Duration::from_millis(500);
    let (_temp, service, drain_handle, provider, workspace_id) =
        make_definition_test_server_with_kind_and_deadlines(
            &[
                ("src/MyComp.vue", "vue", READINESS_CHILD_SOURCE),
                ("src/App.vue", "vue", READINESS_PARENT_SOURCE),
            ],
            crate::TypeProviderKind::Tsgo,
            deadlines,
        )
        .await;

    let server = service.inner();
    let app_uri = workspace_uri(&workspace_id, "src/App.vue");
    let app_id = format!("{workspace_id}/src/App.vue");

    // SurfaceReady: commit the current file's IDE surface through the lifecycle
    // path (did_open's sync), so only the DEPENDENCY receipt is missing.
    server.ensure_current_file_synced(&app_uri).await;

    // Hold the per-document publication lane so any (background) publication
    // attempt parks instead of completing — the handler must not wait for it.
    let lane = server.import_sync.lock_for(&app_id);
    let lane_guard = lane.lock().await;

    let verbs_before = import_sync_verb_count(&provider);
    let position = find_document_position(server, &app_uri, "@custom=\"handleCustom\"", 1);
    let result = server
        .goto_definition(goto_definition_params(&app_uri, position))
        .await;

    assert!(
        result.is_ok(),
        "a MISSING dependency receipt must yield an answer (native or empty), never \
         a deadline cancellation from waiting on import-set work; got {result:?}"
    );
    // The zero-verbs check below is the actual structural proof that the
    // handler never touched the import-set lane (and so never contended on
    // `lane_guard`) — a wall-clock ceiling here would add no discriminating
    // power over it while flipping under machine load, since the handler's
    // own ambient deadline is only 500ms.
    assert_eq!(
        import_sync_verb_count(&provider) - verbs_before,
        0,
        "the handler turn must push ZERO import companions when the receipt is missing"
    );

    // The miss must have ENQUEUED background publication: releasing the lane
    // lets it run to completion and mint the receipt with no further request.
    drop(lane_guard);
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
        "a readiness miss must enqueue BACKGROUND publication that mints \
         DependencyReady once the lane frees",
    );

    drain_handle.abort();
    drop(service);
}

/// A definition with the import closure missing must not inline the walk, and a
/// definition issued while publication is IN FLIGHT must not join it either.
/// Both requests answer immediately while exactly ONE background publication
/// owns the dependency walk.
#[tokio::test]
async fn definition_does_not_join_in_flight_dependency_publication() {
    // Parent -> barrel (`./components` re-export hop) -> child carrier: the walk
    // being joined includes the barrel BFS leg, not only direct imports.
    let child = "<script setup lang=\"ts\">\ndefineProps<{ foo: boolean }>()\n</script>\n<template><div>{{ foo }}</div></template>\n";
    let barrel = "export { default as MyComp } from './MyComp.vue'\n";
    let usage = "<script setup lang=\"ts\">\nimport { MyComp } from './components'\n</script>\n<template><MyComp></MyComp></template>\n";
    let (_temp, service, drain_handle, provider, workspace_id) =
        make_definition_test_server_with_kind(
            &[
                ("src/components/MyComp.vue", "vue", child),
                ("src/components/index.ts", "typescript", barrel),
                ("src/Usage.vue", "vue", usage),
            ],
            crate::TypeProviderKind::Tsgo,
        )
        .await;

    let server = service.inner();
    let usage_uri = workspace_uri(&workspace_id, "src/Usage.vue");
    let child_id = format!("{workspace_id}/src/components/MyComp.vue");
    let child_ide_path =
        verter_session_query::resolution::carrier_ide_provider_path(&child_id, false);
    let (arrived, release) = provider.block_open_file(&child_ide_path);

    let position = find_document_position(server, &usage_uri, "<MyComp>", 1);

    // First definition: receipt missing, no publication in flight. It must NOT
    // inline the import walk — the blocked child open holds until released
    // below, so an inlined walk would hang the handler on it, not merely run
    // slow. A generous hang-detecting timeout discriminates exactly as well
    // as a tight elapsed ceiling, without a loaded machine's legitimate
    // handler latency tripping a false failure.
    let first = tokio::time::timeout(
        std::time::Duration::from_secs(10),
        server.goto_definition(goto_definition_params(&usage_uri, position)),
    )
    .await
    .expect("definition must not inline the import walk — it hung on the blocked child sync");
    assert!(
        first.is_ok(),
        "definition with a blocked import closure must still answer; got {first:?}"
    );

    // The BACKGROUND publication must reach the blocked child open.
    tokio::time::timeout(std::time::Duration::from_secs(5), arrived.notified())
        .await
        .expect("background publication must reach the blocked child companion open");

    // Second definition while publication is IN FLIGHT: navigation is
    // capture-only and must answer BEFORE the parked publication is released
    // (`release` is not notified until below) — an await on the in-flight
    // publication would hang, not merely run slow.
    let second = tokio::time::timeout(
        std::time::Duration::from_secs(10),
        server.goto_definition(goto_definition_params(&usage_uri, position)),
    )
    .await
    .expect("definition must never await in-flight dependency publication — it hung");
    assert!(
        second.is_ok(),
        "a definition observing in-flight publication must still answer; got {second:?}"
    );

    release.notify_one();

    tokio::time::timeout(std::time::Duration::from_secs(5), async {
        let canonical = server
            .documents
            .get(&usage_uri)
            .expect("parent stays open")
            .canonical_id
            .clone();
        let key = server
            .import_sync_freshness_key()
            .expect("host generations");
        server.import_sync.wait_fresh_at(&canonical, key).await;
    })
    .await
    .expect("publication must mint DependencyReady after release");

    // Exactly ONE walk delivered the child companion: capture-only requests
    // never start a second BFS (`OpenFile` is the first-open verb; a second walk
    // would re-open or re-push the same companion).
    let child_opens = provider
        .calls()
        .iter()
        .filter(|c| matches!(c, MockCall::OpenFile { path, .. } if path == &child_ide_path))
        .count();
    assert_eq!(
        child_opens, 1,
        "exactly one import-set walk may deliver the child companion; a capture-only \
         observer must never start a second walk"
    );

    drain_handle.abort();
    drop(service);
}

// ===========================================================================
// Wall-clock effect of the shortened deadlines (hermetic).
//
// The corpus definition bench needs the external 731-SFC corpus and the
// dx-harness orchestrator, neither available in a hermetic worktree. These
// stand in for the acceptance signal directly on the real handler + deadline
// path with a mock provider: shortening the deadline must cut the dead-tail
// wait WITHOUT dropping the answered count or adding latency to a healthy
// request. The mock's two cohorts are the two modes the corpus distribution is
// bimodal between — a fast healthy body and a never-returning tail.
// ===========================================================================

/// A healthy definition (mock answers immediately) is answered without its
/// deadline participating, under the shortened per-kind budget and under the
/// old flat 15s alike. The deadline is a bound, not a barrier.
///
/// Proven on the PAUSED clock as an exact zero: the virtual clock does not
/// move across either call, so neither request waited on any timer at all.
/// The previous shape measured elapsed time and then discarded it, asserting
/// nothing the name claimed; a wall-clock ceiling would not have been a
/// discriminator either, since it flips with machine load.
#[tokio::test(start_paused = true)]
async fn a_healthy_definition_never_waits_on_its_deadline() {
    async fn answers_without_consuming_time(deadline: std::time::Duration) {
        let provider = Arc::new(MockTypeProvider::new());
        let (service, _host) = wedged_provider_server(Arc::clone(&provider), |config| {
            config.lsp_method_timeouts.request_deadlines.goto_definition = deadline;
        });
        let server = service.inner();
        let uri = open_test_vue(server, "/workspace/src/App.vue", DEADLINE_TEST_SOURCE);
        let position = find_document_position(server, &uri, "{{ count", 3);
        // Mock returns an (empty) definition immediately — the healthy body.
        let started = tokio::time::Instant::now();
        let result = super::super::nav_features_audit::handle_goto_definition_with_audit(
            server,
            goto_definition_params(&uri, position),
        )
        .await;
        result.expect("a healthy definition is answered, not cancelled");
        assert_eq!(
            tokio::time::Instant::now(),
            started,
            "a request whose body already succeeded must not consume any of \
             its {deadline:?} budget"
        );
    }

    answers_without_consuming_time(std::time::Duration::from_millis(2500)).await;
    answers_without_consuming_time(std::time::Duration::from_secs(15)).await;
}

/// A rename that cannot prove its edit set is complete must REFUSE, never ship
/// a partial one.
///
/// An EMPTY provider location set is not evidence of completeness. It is what a
/// carrier DENIED by provider-feature admission returns
/// (`TsgoCompositeProvider::get_rename_locations` serves `Ok(vec![])` on
/// denial), and it is indistinguishable from a provider that resolved nothing.
/// Verter's own rename half is SAME-FILE ONLY, and for a Svelte carrier it does
/// not even see the markup occurrences (`TemplateAnalysisSnapshot`
/// `binding_occurrences` is empty for `.svelte` — the markup collector only
/// gathers component usages, snippets and directives). Shipping that half as
/// authoritative is a SUCCESSFUL rename that leaves the source referencing a
/// name that no longer exists.
///
/// DISCRIMINATES: before the completeness gate, the Svelte row returned a
/// 2-of-3 `WorkspaceEdit` — script declaration + script usage, with
/// `{jsValue.label}` silently untouched.
#[tokio::test(flavor = "multi_thread")]
async fn rename_refuses_a_partial_edit_set_when_the_provider_supplies_no_locations() {
    // Every row is exercised before any verdict, so one carrier's failure never
    // hides the other's.
    let mut violations: Vec<String> = Vec::new();
    for (extension, language_id, source) in [
        ("vue", "vue", RENAME_COMPLETENESS_VUE),
        ("svelte", "svelte", RENAME_COMPLETENESS_SVELTE),
    ] {
        let app_path = format!("src/JavaScriptCase.{extension}");
        let (_temp, service, drain_handle, provider, workspace_id) =
            make_definition_test_server_with_kind(
                &[(&app_path, language_id, source)],
                crate::TypeProviderKind::Tsgo,
            )
            .await;
        let server = service.inner();
        let uri = workspace_uri(&workspace_id, &app_path);
        server.ensure_current_file_synced(&uri).await;
        server.publish_import_dependencies_settled(&uri).await;

        let position = find_document_position(server, &uri, "jsValue", 0);
        let edit = super::super::nav_features_navigation::handle_rename(
            server,
            RenameParams {
                text_document_position: TextDocumentPositionParams {
                    text_document: TextDocumentIdentifier { uri: uri.clone() },
                    position,
                },
                new_name: "jsDatum".into(),
                work_done_progress_params: Default::default(),
            },
        )
        .await
        .expect("an incomplete rename must fail closed, not raise a protocol error");

        // The provider WAS consulted — this is a completeness refusal, not a
        // short-circuit that never reached the provider.
        assert!(
            provider
                .calls()
                .iter()
                .any(|call| matches!(call, MockCall::GetRenameLocations { .. })),
            "{extension}: the handler must consult the provider before deciding completeness"
        );

        // The durable invariant, stated over the SET: a returned transaction
        // covers EVERY authored occurrence, or there is no transaction at all.
        if let Some(ws) = &edit {
            let covered = rename_edit_ranges(ws, &uri);
            let authored = authored_token_ranges(source, "jsValue");
            if covered != authored {
                violations.push(format!(
                    "{extension}: SILENT PARTIAL — covered {covered:?}, authored {authored:?}"
                ));
            }
            violations.push(format!(
                "{extension}: a rename whose provider yielded no locations must refuse, got \
                 {covered:?}"
            ));
        }

        drain_handle.abort();
        drop(service);
    }
    assert!(
        violations.is_empty(),
        "rename must refuse rather than ship an unproven edit set:\n{}",
        violations.join("\n")
    );
}

/// The completeness gate must not have widened into a blanket refusal: a
/// carrier whose provider DOES answer still renames, and still covers the exact
/// authored occurrence set.
#[tokio::test(flavor = "multi_thread")]
async fn rename_still_covers_the_full_authored_set_when_the_provider_answers() {
    let app_path = "src/JavaScriptCase.vue";
    let (_temp, service, drain_handle, provider, workspace_id) =
        make_definition_test_server_with_kind(
            &[(app_path, "vue", RENAME_COMPLETENESS_VUE)],
            crate::TypeProviderKind::Tsgo,
        )
        .await;
    let server = service.inner();
    let uri = workspace_uri(&workspace_id, app_path);
    server.ensure_current_file_synced(&uri).await;
    server.publish_import_dependencies_settled(&uri).await;

    let position = find_document_position(server, &uri, "jsValue", 0);
    let ctx = synced_type_provider_context(server, &uri).await;
    let decl_offset = merge::carrier_position_to_tsx_offset_validated(
        &position,
        &ctx.carrier_line_index,
        &ctx.mapper,
        &ctx.tsx_line_index,
    )
    .expect("the declaration position maps into the IDE surface");
    provider.set_rename_locations(
        &ctx.tsx_path,
        decl_offset,
        vec![crate::type_provider::protocol::RenameLocation {
            path: ctx.tsx_path.clone(),
            start: decl_offset,
            end: decl_offset + "jsValue".len() as u32,
        }],
    );

    let edit = super::super::nav_features_navigation::handle_rename(
        server,
        RenameParams {
            text_document_position: TextDocumentPositionParams {
                text_document: TextDocumentIdentifier { uri: uri.clone() },
                position,
            },
            new_name: "jsDatum".into(),
            work_done_progress_params: Default::default(),
        },
    )
    .await
    .expect("rename request should succeed")
    .expect("an answering provider must still produce a rename");

    assert_eq!(
        rename_edit_ranges(&edit, &uri),
        authored_token_ranges(RENAME_COMPLETENESS_VUE, "jsValue"),
        "the rename must cover the exact authored occurrence set, got {edit:?}"
    );

    drain_handle.abort();
    drop(service);
}

/// Svelte's generated TypeScript surface can repeat a typed local in synthetic
/// component scaffolding. The provider correctly includes that generated-only
/// occurrence in its rename answer, but no source-map range exists for it. A
/// conservative authored-token inventory must prove the real source transaction
/// complete so this synthetic same-companion drop does not veto a valid rename.
#[tokio::test(flavor = "multi_thread")]
async fn svelte_typed_local_rename_ignores_only_generated_occurrence_after_full_source_coverage() {
    const SOURCE: &str = "<script lang=\"ts\">\ninterface ContractValue { label: string; count: number }\nlet typedValue: ContractValue = { label: \"typed\", count: 1 };\nfunction renderTyped(): string { return `${typedValue.label}:${typedValue.count}`; }\n</script>\n<button onclick={renderTyped}>{typedValue.label}</button>\n";
    let app_path = "src/App.svelte";
    let (_temp, service, drain_handle, provider, workspace_id) =
        make_definition_test_server_with_kind(
            &[(app_path, "svelte", SOURCE)],
            crate::TypeProviderKind::Tsgo,
        )
        .await;
    let server = service.inner();
    let uri = workspace_uri(&workspace_id, app_path);
    server.ensure_current_file_synced(&uri).await;
    server.publish_import_dependencies_settled(&uri).await;

    let position = find_document_position(server, &uri, "typedValue", 0);
    let ctx = synced_type_provider_context(server, &uri).await;
    let query_offset = merge::carrier_position_to_tsx_offset_validated(
        &position,
        &ctx.carrier_line_index,
        &ctx.mapper,
        &ctx.tsx_line_index,
    )
    .expect("the declaration maps into the Svelte IDE surface");
    let mut provider_locations = authored_token_ranges(SOURCE, "typedValue")
        .into_iter()
        .map(|(start_line, start_character, _, _)| {
            let start = merge::carrier_position_to_tsx_offset_validated(
                &Position::new(start_line, start_character),
                &ctx.carrier_line_index,
                &ctx.mapper,
                &ctx.tsx_line_index,
            )
            .expect("every authored typedValue occurrence maps into the Svelte IDE surface");
            crate::type_provider::protocol::RenameLocation {
                path: ctx.tsx_path.clone(),
                start,
                end: start + "typedValue".len() as u32,
            }
        })
        .collect::<Vec<_>>();
    provider_locations.push(crate::type_provider::protocol::RenameLocation {
        path: ctx.tsx_path.clone(),
        start: ctx.tsx_content.len() as u32 + 100,
        end: ctx.tsx_content.len() as u32 + 100 + "typedValue".len() as u32,
    });
    provider.set_rename_locations(&ctx.tsx_path, query_offset, provider_locations);

    let edit = super::super::nav_features_navigation::handle_rename(
        server,
        RenameParams {
            text_document_position: TextDocumentPositionParams {
                text_document: TextDocumentIdentifier { uri: uri.clone() },
                position,
            },
            new_name: "renamedValue".into(),
            work_done_progress_params: Default::default(),
        },
    )
    .await
    .expect("rename request should succeed")
    .expect("full authored coverage must admit the rename despite generated-only scaffolding");

    assert_eq!(
        rename_edit_ranges(&edit, &uri),
        authored_token_ranges(SOURCE, "typedValue"),
        "the admitted rename must still cover exactly every authored occurrence"
    );

    drain_handle.abort();
    drop(service);
}

/// CSS class rename is a Verter-native surface with NO TypeScript correlate:
/// the provider legitimately yields nothing for it, and the completeness gate
/// must not refuse it. Proves the gate keys on the provider-backed binding
/// route, not on "the provider returned an empty vector".
#[tokio::test(flavor = "multi_thread")]
async fn css_class_rename_still_serves_without_any_provider_locations() {
    let source = "<template>\n  <div class=\"panel\"></div>\n</template>\n\n<style scoped>\n.panel {\n  color: red;\n}\n</style>\n";
    let app_path = "src/Styled.vue";
    let (_temp, service, drain_handle, _provider, workspace_id) =
        make_definition_test_server_with_kind(
            &[(app_path, "vue", source)],
            crate::TypeProviderKind::Tsgo,
        )
        .await;
    let server = service.inner();
    let uri = workspace_uri(&workspace_id, app_path);
    server.ensure_current_file_synced(&uri).await;
    server.publish_import_dependencies_settled(&uri).await;

    let position = find_document_position(server, &uri, "class=\"panel\"", 7);
    let edit = super::super::nav_features_navigation::handle_rename(
        server,
        RenameParams {
            text_document_position: TextDocumentPositionParams {
                text_document: TextDocumentIdentifier { uri: uri.clone() },
                position,
            },
            new_name: "card".into(),
            work_done_progress_params: Default::default(),
        },
    )
    .await
    .expect("rename request should succeed")
    .expect("a native CSS rename must still serve without provider locations");

    assert_eq!(
        rename_edit_ranges(&edit, &uri),
        authored_token_ranges(source, "panel"),
        "the CSS rename must cover the template and style occurrences, got {edit:?}"
    );

    drain_handle.abort();
    drop(service);
}
