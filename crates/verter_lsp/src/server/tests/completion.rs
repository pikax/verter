use super::*;

#[test]
fn child_contract_completion_adapter_preserves_event_and_slot_order() {
    use verter_session::framework::{
        ComponentContractAvailability, ComponentPublicContract, ContractExactness,
        ContractProvenance, FrameworkAdapterId, PublicDerivedHandlerShape, PublicEvent, PublicSlot,
        PublicSlotBinding, PublicSlotInput, PublicTypeReference,
    };

    let slot_binding_type = PublicTypeReference {
        publication: verter_session::meta_resolve::MaterializedTypePublication::for_test(
            verter_type_expr::PublicationResult::Published {
                selected_source: Arc::new(verter_type_expr::facts::SemanticTypeSource::Closed(
                    verter_type_expr::facts::ClosedTypeFact::Leaf(
                        verter_type_expr::facts::LeafTypeFact::Primitive(
                            verter_type_expr::PrimitiveName::String,
                        ),
                    ),
                )),
                semantic_authority: verter_type_expr::SemanticAuthority::Resolved,
                exactness: verter_type_expr::ResolutionExactness::ExactConcrete,
                reason: Box::new(verter_type_expr::PublicationReason::ResolvedExactConcrete),
                provenance: verter_type_expr::PublicationProvenance::Resolved {
                    provenance: verter_type_expr::ResolutionProvenance::FrameworkSurface,
                },
            },
            Some(verter_type_expr::TypeExpr::Primitive(
                verter_type_expr::PrimitiveName::String,
            )),
            None,
        ),
    };

    let contract = ComponentContractAvailability::Supported(Arc::new(ComponentPublicContract {
        adapter_id: FrameworkAdapterId::vue(),
        exactness: ContractExactness::Exact,
        degradation: Arc::from([]),
        provenance: ContractProvenance::ComponentMetaOutput,
        props: Arc::from([]),
        events: Arc::from([
            PublicEvent {
                name: Arc::from("pick"),
                overloads: Arc::from([]),
                derived_handler: PublicDerivedHandlerShape {
                    overloads: Arc::from([]),
                },
                exactness: ContractExactness::Exact,
                degradation: Arc::from([]),
                provenance: ContractProvenance::ComponentMetaOutput,
            },
            PublicEvent {
                name: Arc::from("close"),
                overloads: Arc::from([]),
                derived_handler: PublicDerivedHandlerShape {
                    overloads: Arc::from([]),
                },
                exactness: ContractExactness::Exact,
                degradation: Arc::from([]),
                provenance: ContractProvenance::ComponentMetaOutput,
            },
        ]),
        slots: Arc::from([
            PublicSlot {
                name: Arc::from("header"),
                optional: true,
                input: PublicSlotInput {
                    bindings: Arc::from([PublicSlotBinding {
                        name: Arc::from("item"),
                        ty: slot_binding_type,
                    }]),
                },
                return_type: None,
                exactness: ContractExactness::Exact,
                degradation: Arc::from([]),
                provenance: ContractProvenance::ComponentMetaOutput,
            },
            PublicSlot {
                name: Arc::from("default"),
                optional: false,
                input: PublicSlotInput {
                    bindings: Arc::from([]),
                },
                return_type: None,
                exactness: ContractExactness::Exact,
                degradation: Arc::from([]),
                provenance: ContractProvenance::ComponentMetaOutput,
            },
        ]),
    }));

    let analysis = super::super::nav_features::child_contract_completion_analysis(contract)
        .expect("supported child contract must adapt");
    let template = analysis.template.expect("template projection");
    assert_eq!(
        template
            .emit_definitions
            .iter()
            .map(|event| event.event_name.as_str())
            .collect::<Vec<_>>(),
        ["pick", "close"]
    );
    assert!(template
        .emit_definitions
        .iter()
        .all(|event| event.is_declared
            && event.emit_locations.is_empty()
            && event.span.is_empty()));
    assert_eq!(
        template
            .defined_slots
            .iter()
            .map(|slot| slot.name.as_str())
            .collect::<Vec<_>>(),
        ["header", "default"]
    );
    assert!(template.defined_slots[0].has_bindings);
    assert_eq!(template.defined_slots[0].binding_names, ["item"]);
    assert_eq!(template.defined_slots[0].binding_expressions, [""]);
    assert!(template.defined_slots[0]
        .binding_value_spans
        .iter()
        .all(verter_span::Span::is_empty));
    assert!(!template.defined_slots[1].has_bindings);
    assert!(template
        .defined_slots
        .iter()
        .all(|slot| { !slot.has_fallback_content && slot.span.is_empty() }));
}

/// In the editor-owned topology the TypeScript plugin is the typed member-list
/// owner. Verter still owns bare template-scope completions, but returning that
/// same outer scope for `obj.|` makes VS Code merge unrelated bindings into the
/// plugin's precise property list.
/// Broken-script recovery is TypeScript-provider-owned: compiler partial
/// recovery keeps the carrier queryable, while Verter's native analysis never
/// claims semantic authority over a partial script AST. Dedicated provider
/// recovery tests cover both identifiers and functions.
#[tokio::test(flavor = "multi_thread")]
async fn editor_tsserver_completion_yields_member_access_but_keeps_bare_scope() {
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
    let source = "<script setup lang=\"ts\">\nconst obj = { field: 1 }\nconst outer = 2\n</script>\n<template><div>{{ obj }} {{ obj.field }}</div></template>\n";
    let uri = open_test_vue(server, "/workspace/src/App.vue", source);
    let line_index = server
        .documents
        .get(&uri)
        .expect("open document")
        .line_index
        .clone();

    let bare_offset = source.find("{{ obj }}").expect("bare interpolation") + "{{ obj".len();
    let bare_position = line_index
        .offset_to_position(bare_offset as u32)
        .expect("bare source position");
    let bare = completion_labels(
        super::super::nav_features::handle_completion(
            server,
            completion_params(&uri, bare_position, None),
        )
        .await
        .expect("bare completion succeeds"),
    );
    assert!(
        bare.iter().any(|label| label == "obj") && bare.iter().any(|label| label == "outer"),
        "Verter must retain the bare template scope, got {bare:?}"
    );

    let member_offset = source.find("obj.field").expect("member access") + "obj.".len();
    let member_position = line_index
        .offset_to_position(member_offset as u32)
        .expect("member source position");
    let member = super::super::nav_features::handle_completion(
        server,
        completion_params(&uri, member_position, Some(".")),
    )
    .await
    .expect("member completion succeeds");
    assert!(
        member.is_none(),
        "editor-owned member access must yield to the TypeScript plugin, got {:?}",
        completion_labels(member)
    );

    let script_offset = source.find("const outer").expect("script declaration") + "const out".len();
    let script_position = line_index
        .offset_to_position(script_offset as u32)
        .expect("script source position");
    let script = super::super::nav_features::handle_completion(
        server,
        completion_params(&uri, script_position, None),
    )
    .await
    .expect("script completion succeeds");
    assert!(
        script.is_none(),
        "editor-owned script completion must yield to the TypeScript plugin, got {:?}",
        completion_labels(script)
    );
}

/// Provider-parity extension E2E must attribute every completion item to the
/// selected TypeScript engine. Verter still owns carrier generation, sync, and
/// source mapping, but its native completion producer is disabled for this rail.
/// This test is the behavioral discriminator: the ordinary template producer
/// would contribute `localValue`, while the mock provider contributes that label
/// as a Function plus an out-of-scope global. A bare identifier position proves
/// the provider is queried; the Function kind proves the surviving item came from
/// it, and the missing global proves the subtractive template visibility boundary.
#[tokio::test(flavor = "multi_thread")]
async fn provider_only_completion_mode_emits_no_verter_native_items() {
    let provider = Arc::new(MockTypeProvider::new());
    let type_provider: Arc<dyn TypeProvider> = provider.clone();
    let service = make_hover_test_service_tsgo(type_provider);
    let server = service.inner();
    install_test_resolver(server);
    server.set_provider_only_completions_for_test(true);

    let source = "<script setup lang=\"ts\">\nconst localValue = 1\n</script>\n<template><div>{{ local }}</div></template>\n";
    let uri = open_test_vue(server, "/workspace/src/ProviderOnly.vue", source);
    let position = find_document_position(server, &uri, "{{ local }}", "{{ local".len());
    set_type_completions_at_vue_position(
        server,
        &provider,
        &uri,
        position,
        vec![
            mock_completion(
                "localValue",
                crate::type_provider::protocol::CompletionKind::Function,
            ),
            mock_completion(
                "AbortController",
                crate::type_provider::protocol::CompletionKind::Class,
            ),
            mock_completion(
                "localValueGenerated",
                crate::type_provider::protocol::CompletionKind::Variable,
            ),
        ],
    );

    let response = server
        .completion(completion_params(&uri, position, None))
        .await
        .expect("provider-only completion request should succeed");
    let items = match response {
        Some(CompletionResponse::Array(items)) => items,
        Some(CompletionResponse::List(list)) => list.items,
        None => Vec::new(),
    };

    assert_eq!(
        items
            .iter()
            .map(|item| item.label.as_str())
            .collect::<Vec<_>>(),
        vec!["localValue"],
        "the provider-only rail must retain only provider items visible in template scope"
    );
    assert_eq!(
        items[0].kind,
        Some(CompletionItemKind::FUNCTION),
        "the surviving completion kind must come from the provider, not Verter's native local"
    );
    assert!(
        provider
            .calls()
            .iter()
            .any(|call| matches!(call, MockCall::GetCompletions { .. })),
        "the provider-only rail must issue a real provider completion query"
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn provider_only_completion_keeps_typed_slot_lexical_locals() {
    let provider = Arc::new(MockTypeProvider::new());
    let type_provider: Arc<dyn TypeProvider> = provider.clone();
    let service = make_hover_test_service_tsgo(type_provider);
    let server = service.inner();
    install_test_resolver(server);
    server.set_provider_only_completions_for_test(true);

    let source = "<script setup lang=\"ts\">\nconst outer = 1\n</script>\n<template><TypedSlot v-slot=\"{ slotItem, slotIndex: index, ...rest }\"><p>{{ sl }}</p></TypedSlot></template>\n";
    let uri = open_test_vue(server, "/workspace/src/ProviderSlot.vue", source);
    let position = find_document_position(server, &uri, "{{ sl }}", "{{ sl".len());
    set_type_completions_at_vue_position(
        server,
        &provider,
        &uri,
        position,
        vec![
            mock_completion(
                "slotItem",
                crate::type_provider::protocol::CompletionKind::Variable,
            ),
            mock_completion(
                "window",
                crate::type_provider::protocol::CompletionKind::Variable,
            ),
        ],
    );

    let labels = completion_labels(
        server
            .completion(completion_params(&uri, position, None))
            .await
            .expect("provider-only slot completion should succeed"),
    );
    assert_eq!(
        labels,
        vec!["slotItem"],
        "slot locals come from the provider while ambient globals stay outside template scope"
    );
}

#[tokio::test]
async fn initialized_returns_before_background_configure_paths_completes() {
    let temp_root = verter_test_support::unique_temp_dir("verter-lsp-init");
    std::fs::create_dir_all(temp_root.join("src")).expect("temp project should be created");
    std::fs::write(
        temp_root.join("tsconfig.json"),
        r#"{
  "compilerOptions": {
    "baseUrl": ".",
    "paths": {
      "@/*": ["src/*"]
    }
  }
}"#,
    )
    .expect("tsconfig should be written");

    let provider = Arc::new(SlowConfigurePathsProvider::default());
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
    let drain_handle = tokio::spawn(async move {
        let mut socket = socket;
        while socket.next().await.is_some() {}
    });

    let server = service.inner();
    server.vite_config_options.lock().await.enabled = false;
    *server.workspace_roots.lock().await = vec![format!(
        "file:///{}",
        temp_root.to_string_lossy().replace('\\', "/")
    )];

    // `configure_paths` is gated on a `Notify` this test never signals, so it
    // hangs forever. A `initialized()` that incorrectly awaited background
    // path configuration would therefore hang here too — proven by a
    // generous hang-detector timeout, not by a tight wall-clock ceiling that
    // a loaded machine can blow through on legitimately slow (but correct)
    // synchronous work.
    tokio::time::timeout(
        std::time::Duration::from_secs(10),
        server.initialized(InitializedParams {}),
    )
    .await
    .expect(
        "initialized() should not wait for configure_paths/background discovery — it hung, \
         meaning it awaited the never-releasing background task",
    );

    {
        let notified = provider.configure_paths_started_notify.notified();
        tokio::pin!(notified);
        notified.as_mut().enable();
        if provider.configure_paths_started.load(Ordering::SeqCst) == 0 {
            tokio::time::timeout(std::time::Duration::from_secs(5), notified)
                .await
                .expect("background init should still configure paths after initialized() returns");
        }
    }

    // Release the never-completing background task so it doesn't outlive
    // this test. `notify_one`, not `notify_waiters`: the counter above only
    // proves `configure_paths` was CALLED, not that its returned future has
    // been polled yet (and so registered as a waiter) — `notify_waiters`
    // wakes only already-registered waiters and would silently lose this
    // notification if the future hasn't been polled yet, leaking the task.
    // `notify_one` buffers a permit for a not-yet-registered waiter too, so
    // release is guaranteed regardless of polling order.
    provider.configure_paths_release.notify_one();

    drain_handle.abort();
    drop(service);
}

#[tokio::test]
async fn svelte_dotted_component_completion_is_sanitized_but_edit_placement_defers() {
    // GAP (Svelte parity, honest): the SHARED completion path sanitizes a dotted
    // `.svelte` carrier exactly like a `.vue` one — a valid tag + CLASS kind. The
    // auto-import EDIT placement, however, is a PRE-EXISTING gap: it only targets
    // a Vue `<script setup>`, so for a Svelte `<script>` block it returns `None`.
    // This test pins BOTH facts so the Svelte completion stays covered and the
    // placement deferral is explicit (not silently broken).
    let child_source = "<script lang=\"ts\"></script>\n<div />\n";
    // A Svelte component uses a plain `<script>` (NOT `<script setup>`).
    let parent_source = "<script lang=\"ts\">\n  let x = 1;\n</script>\n<div />\n";
    let (_temp, service, drain_handle, _provider, workspace_id) = make_definition_test_server(&[
        ("src/Dotted.Svelte.Name.svelte", "svelte", child_source),
        ("src/App.svelte", "svelte", parent_source),
    ])
    .await;

    let app_uri = workspace_uri(&workspace_id, "src/App.svelte");
    let app_canonical = format!("{workspace_id}/src/App.svelte");
    let server = service.inner();

    // (1) SHARED completion path: the dotted Svelte carrier sanitizes to a valid
    //     identifier with the real `.svelte` path preserved.
    let ws_components = build_workspace_components(&server.documents.host(), &app_canonical);
    let svelte = ws_components
        .iter()
        .find(|c| c.import_path.ends_with("Dotted.Svelte.Name.svelte"))
        .unwrap_or_else(|| {
            panic!(
                "the dotted Svelte carrier must enumerate, got: {:?}",
                ws_components
                    .iter()
                    .map(|c| (&c.name, &c.import_path))
                    .collect::<Vec<_>>()
            )
        });
    assert_eq!(
        svelte.name, "DottedSvelteName",
        "Svelte parity: a dotted `.svelte` stem must sanitize identically to Vue"
    );

    // The shared COMPLETION ITEM synthesis (tag insert + CLASS kind) is the
    // user-visible shared surface — assert it produces a valid tag + CLASS.
    let analysis = verter_session_query::analysis::file_analysis::FileAnalysisSnapshot::default();
    let items = crate::features::completion::tag_name_completions(
        &analysis,
        Some(&ws_components),
        Some(app_uri.as_str()),
    );
    let item = items
        .iter()
        .find(|i| i.label == "DottedSvelteName")
        .expect("Svelte dotted component must appear as a sanitized tag completion");
    assert_eq!(
        item.kind,
        Some(tower_lsp_server::ls_types::CompletionItemKind::CLASS),
        "Svelte component tag item must use CLASS (shared with Vue)"
    );

    // (2) HONEST deferral: the auto-import EDIT placement only targets a Vue
    //     `<script setup>`. For a Svelte plain `<script>` it returns `None` —
    //     a SEPARATE, already-tracked placement gap, NOT a regression of this
    //     fix. (The sanitized name + CLASS kind above already apply to Svelte.)
    let edit = server.build_auto_import_edit(app_uri.as_str(), &svelte.name, &svelte.import_path);
    assert!(
        edit.is_none(),
        "Svelte auto-import EDIT placement is a pre-existing deferral (no `<script setup>` anchor); \
         got an unexpected edit: {edit:?}"
    );

    drain_handle.abort();
    drop(service);
}

#[tokio::test]
async fn completion_resolves_barrel_reexport_props_via_index_file() {
    let child_source =
            "<script setup lang=\"ts\">\ndefineProps<{ label: string; zIndex?: number }>()\n</script>\n";
    let barrel_source = "export { default as BarrelComp } from './BarrelComp.vue'\n";
    let parent_source = "<script setup lang=\"ts\">\nimport { BarrelComp } from './components'\n</script>\n<template>\n  <BarrelComp  />\n</template>\n";
    let (_temp, service, drain_handle, provider, workspace_id) = make_definition_test_server(&[
        ("src/components/BarrelComp.vue", "vue", child_source),
        ("src/components/index.ts", "typescript", barrel_source),
        ("src/App.vue", "vue", parent_source),
    ])
    .await;

    let app_uri = workspace_uri(&workspace_id, "src/App.vue");
    let server = service.inner();
    server.publish_import_dependencies_settled(&app_uri).await;

    // Cursor at `<BarrelComp |/>` — in attribute position
    let cursor_pos = parent_source.find("<BarrelComp ").unwrap() + "<BarrelComp ".len();
    let line_index = LineIndex::new_utf16(parent_source);
    let position = line_index.offset_to_position(cursor_pos as u32).unwrap();

    let labels = completion_labels(
        server
            .completion(completion_params(&app_uri, position, None))
            .await
            .expect("completion request should succeed"),
    );

    // Positive: child props should appear via barrel re-export
    assert!(
        labels.contains(&"label".to_string()),
        "barrel-imported component should offer 'label' prop, got: {labels:?}"
    );
    assert!(
        labels.contains(&"z-index".to_string()),
        "barrel-imported component should offer 'z-index' prop (kebab-case), got: {labels:?}"
    );

    // Negative: internal symbols must not leak
    assert!(
        !labels.iter().any(|l| l.contains("___VERTER___")),
        "internal symbols must not leak: {labels:?}"
    );

    // The attribution-only E2E rail must still reach the already-synchronized
    // provider when the optional native barrel-contract cache is cold. Normal
    // product requests remain cache-only and fail closed on this miss.
    server.evict_barrel_component_route_for_test(
        &format!("{workspace_id}/src/App.vue"),
        "BarrelComp",
    );
    server.set_provider_only_completions_for_test(true);
    let provider_queries_before = provider
        .calls()
        .iter()
        .filter(|call| matches!(call, MockCall::GetCompletions { .. }))
        .count();
    let _ = server
        .completion(completion_params(&app_uri, position, None))
        .await
        .expect("provider-only completion should remain queryable on a native cache miss");
    let provider_queries_after = provider
        .calls()
        .iter()
        .filter(|call| matches!(call, MockCall::GetCompletions { .. }))
        .count();
    assert_eq!(
        provider_queries_after,
        provider_queries_before + 1,
        "a native barrel-contract miss must not short-circuit the provider-attribution rail"
    );

    drain_handle.abort();
    drop(service);
}

#[tokio::test]
async fn completion_does_not_cold_load_children_for_native_enrichment() {
    for case in D1_CARRIER_CASES {
        let child_path = format!("src/DirectComp.{}", case.extension);
        let parent_path = format!("src/App.{}", case.extension);
        let (temp, service, drain_handle, _provider, workspace_id) =
            make_definition_test_server(&[]).await;
        let workspace = temp.path().join("workspace");
        std::fs::create_dir_all(workspace.join("src")).expect("create source directory");
        std::fs::write(
            workspace.join(format!("src/DirectComp.{}", case.extension)),
            case.child_source,
        )
        .expect("write cold child");
        std::fs::write(
            workspace.join(format!("src/App.{}", case.extension)),
            case.incomplete_parent_source,
        )
        .expect("write parent");

        let app_uri = workspace_uri(&workspace_id, &parent_path);
        let server = service.inner();
        let app_canonical = format!("{workspace_id}/{parent_path}");
        let child_canonical = format!("{workspace_id}/{child_path}");
        let _ = server.documents.did_open(&TextDocumentItem {
            uri: app_uri.clone(),
            language_id: case.language_id.to_string(),
            version: 1,
            text: case.incomplete_parent_source.to_string(),
        });
        // Parent compilation may warm imports proactively. Hold the existing
        // publication lane while evicting so the approved detached self-heal
        // cannot win the post-request assertion; this test measures foreground
        // request behavior, while dedicated publication tests cover recovery.
        let publication_lane = server.import_sync.lock_for(&app_canonical);
        let publication_guard = publication_lane.lock().await;
        server.documents.host().evict(&child_canonical);
        assert!(
            server
                .documents
                .host()
                .get_analysis(&child_canonical)
                .is_none(),
            "{} precondition: child must be cold when completion begins",
            case.name
        );
        let cursor_pos = case
            .incomplete_parent_source
            .find("<DirectComp ")
            .expect("component tag")
            + "<DirectComp ".len();
        let position = LineIndex::new_utf16(case.incomplete_parent_source)
            .offset_to_position(cursor_pos as u32)
            .expect("completion position");

        let labels = completion_labels(
            server
                .completion(completion_params(&app_uri, position, None))
                .await
                .expect("completion request should succeed"),
        );
        assert!(
            server
                .documents
                .host()
                .get_analysis(&child_canonical)
                .is_none(),
            "{} completion must leave a cold child to background/TypeScript ownership",
            case.name
        );
        assert!(
            !labels.contains(&"label".to_string()) && !labels.contains(&case.index_label.to_string()),
            "{} native completion must not fabricate typed props without a committed cache: {labels:?}",
            case.name
        );

        drop(publication_guard);
        drain_handle.abort();
        drop(service);
    }
}

#[tokio::test]
async fn svelte_completion_uses_declared_public_prop_keys_not_local_bindings() {
    let cases = [
        (
            "ts-alias-rest",
            "<script lang=\"ts\">\nlet { publicName: localName, 'string-key': stringLocal, ...rest }: { publicName: string; 'string-key': number; restOnly?: boolean } = $props();\nconst internalOnly = true;\n</script>\n",
            &["publicName", "string-key", "restOnly"][..],
            &["localName", "stringLocal", "rest", "internalOnly"][..],
        ),
        (
            "ts-whole-object",
            "<script lang=\"ts\">\nlet props: { wholeProp: string; optionalProp?: number } = $props();\n</script>\n",
            &["wholeProp", "optionalProp"][..],
            &["props"][..],
        ),
        (
            "ts-named-interface",
            "<script lang=\"ts\">\ninterface CorpusProps { tone0: string; caption1?: number; }\nlet { tone0, caption1 }: CorpusProps = $props();\n</script>\n",
            &["tone0", "caption1"][..],
            &[][..],
        ),
        (
            "js-alias-rest",
            "<script>\n/** @type {{ publicName: string, 'string-key': number, restOnly?: boolean }} */\nlet { publicName: localName, 'string-key': stringLocal, ...rest } = $props();\nconst internalOnly = true;\n</script>\n",
            &["publicName", "string-key", "restOnly"][..],
            &["localName", "stringLocal", "rest", "internalOnly"][..],
        ),
        (
            "js-whole-object",
            "<script>\n/** @type {{ wholeProp: string, optionalProp?: number }} */\nlet props = $props();\n</script>\n",
            &["wholeProp", "optionalProp"][..],
            &["props"][..],
        ),
    ];
    let parent_source =
        "<script>\nimport DirectComp from './DirectComp.svelte';\n</script>\n<DirectComp un />";

    for (name, child_source, expected, forbidden) in cases {
        let (_temp, service, drain_handle, _provider, workspace_id) =
            make_definition_test_server(&[
                ("src/DirectComp.svelte", "svelte", child_source),
                ("src/App.svelte", "svelte", parent_source),
            ])
            .await;
        let server = service.inner();
        let app_uri = workspace_uri(&workspace_id, "src/App.svelte");
        settle_child_contracts(server, &app_uri, &workspace_id, &["src/DirectComp.svelte"]).await;
        let child_id = format!("{workspace_id}/src/DirectComp.svelte");
        let contract = server
            .cached_child_public_contract(&child_id)
            .expect("settled child contract");
        assert!(
            matches!(
                contract,
                verter_session::framework::ComponentContractAvailability::Supported(_)
            ),
            "{name}: settled child contract must be supported, got {contract:?}"
        );
        let cursor = parent_source
            .find("<DirectComp un")
            .expect("component attribute")
            + "<DirectComp un".len();
        let position = LineIndex::new_utf16(parent_source)
            .offset_to_position(cursor as u32)
            .expect("completion position");
        let labels = completion_labels(
            server
                .completion(completion_params(&app_uri, position, None))
                .await
                .expect("completion request should succeed"),
        );

        for expected_key in expected {
            assert!(
                labels.iter().any(|label| label == expected_key),
                "{name}: declared public key `{expected_key}` missing from {labels:?}"
            );
        }
        for forbidden_name in forbidden {
            assert!(
                labels.iter().all(|label| label != forbidden_name),
                "{name}: local/non-prop binding `{forbidden_name}` leaked into {labels:?}"
            );
        }

        drain_handle.abort();
        drop(service);
    }
}

/// A completion read through a published child contract settles only while
/// that contract still validates against everything it was derived from. An
/// imported props type that changes before settlement — neither the parent nor
/// the child source moving — supersedes the answer instead of delivering the
/// contract's superseded props beside a provider answer on the new basis.
#[tokio::test(flavor = "multi_thread")]
async fn child_contract_completion_rejects_an_answer_whose_imported_props_type_moved() {
    let types_source = "export interface TypedChildProps { beforeProp: string }\n";
    let child_source = "<script setup lang=\"ts\">\nimport type { TypedChildProps } from './typed-child-props'\ndefineProps<TypedChildProps>()\n</script>\n";
    let parent_source = "<script setup lang=\"ts\">\nimport TypedChild from './TypedChild.vue'\n</script>\n<template>\n  <TypedChild  />\n</template>\n";
    let (_temp, service, drain_handle, _provider, workspace_id) = make_definition_test_server(&[
        ("src/typed-child-props.ts", "typescript", types_source),
        ("src/TypedChild.vue", "vue", child_source),
        ("src/App.vue", "vue", parent_source),
    ])
    .await;
    let server = service.inner();
    let uri = workspace_uri(&workspace_id, "src/App.vue");
    let types_uri = workspace_uri(&workspace_id, "src/typed-child-props.ts");
    settle_child_contracts(server, &uri, &workspace_id, &["src/TypedChild.vue"]).await;
    let cursor = parent_source.find("<TypedChild ").unwrap() + "<TypedChild ".len();
    let position = LineIndex::new_utf16(parent_source)
        .offset_to_position(cursor as u32)
        .expect("completion position");
    let unmoved = completion_labels(
        server
            .completion(completion_params(&uri, position, None))
            .await
            .expect("an unmoved completion succeeds"),
    );
    assert!(
        unmoved.contains(&"before-prop".to_string()),
        "the published child contract answers the completion: {unmoved:?}"
    );

    let moved = Arc::new(std::sync::atomic::AtomicBool::new(false));
    {
        let server = server.clone();
        let types_uri = types_uri.clone();
        let moved = Arc::clone(&moved);
        server.request_barriers().clear();
        server.request_barriers().arm(
            super::super::test_support::RequestBarrier::Settlement,
            Arc::new(move |arrival| {
                if arrival == 0 {
                    assert!(
                        server
                            .documents
                            .did_change(
                                &types_uri,
                                2,
                                "export interface TypedChildProps { afterProp: string }\n",
                            )
                            .changed
                    );
                    moved.store(true, std::sync::atomic::Ordering::SeqCst);
                }
                Box::pin(async {})
            }),
        );
    }
    let raced = server
        .completion(completion_params(&uri, position, None))
        .await;
    server.request_barriers().clear();
    assert!(moved.load(std::sync::atomic::Ordering::SeqCst));
    assert!(
        matches!(&raced, Err(error) if error.code == tower_lsp_server::jsonrpc::ErrorCode::ContentModified),
        "a completion read through a contract whose imported props type moved before \
         settlement answers ContentModified: {raced:?}"
    );

    drain_handle.abort();
    drop(service);
}

#[tokio::test(flavor = "multi_thread")]
async fn concurrent_stable_completions_do_not_cancel_each_other() {
    let child_source =
        "<script setup lang=\"ts\">\ndefineProps<{ tone0: string; caption1?: string }>()\n</script>\n";
    let parent_source = "<script setup lang=\"ts\">\nimport CorpusChild from './CorpusChild.vue'\n</script>\n<template>\n  <CorpusChild  />\n</template>\n";
    let (_temp, service, drain_handle, _provider, workspace_id) = make_definition_test_server(&[
        ("src/CorpusChild.vue", "vue", child_source),
        ("src/Corpus1.vue", "vue", parent_source),
    ])
    .await;
    let server = service.inner();
    let uri = workspace_uri(&workspace_id, "src/Corpus1.vue");
    settle_child_contracts(server, &uri, &workspace_id, &["src/CorpusChild.vue"]).await;
    let cursor = parent_source.find("<CorpusChild ").unwrap() + "<CorpusChild ".len();
    let position = LineIndex::new_utf16(parent_source)
        .offset_to_position(cursor as u32)
        .expect("completion position");

    // Queue every request behind the edit fence so they are all live at once.
    // A process-global completion generation cancels the first 31 here even
    // though document/version state is stable and every request is valid.
    let fence = server.did_change_mutex.lock().await;
    let completions = futures_util::future::join_all(
        (0..32).map(|_| server.completion(completion_params(&uri, position, None))),
    );
    let release = async move {
        tokio::task::yield_now().await;
        drop(fence);
    };
    let (responses, ()) = futures_util::future::join(completions, release).await;

    for (index, response) in responses.into_iter().enumerate() {
        let labels = completion_labels(response.expect("completion request should succeed"));
        assert!(
            labels.contains(&"tone0".to_string()) && labels.contains(&"caption1".to_string()),
            "stable concurrent completion {index} must remain typed, got {labels:?}"
        );
    }

    drain_handle.abort();
    drop(service);
}

/// A completion sent after an edit whose commit is still waiting on the
/// edit-commit fence answers against that edit's revision. Admitting it before
/// the queued edit commits would pin the replaced revision, and the edit
/// committing ahead of the completion's source read would then answer
/// `ContentModified` for an edit the client sent first.
#[tokio::test(flavor = "multi_thread")]
async fn completion_sent_after_a_queued_edit_answers_the_edited_revision() {
    let child_source =
        "<script setup lang=\"ts\">\ndefineProps<{ tone0: string; caption1?: string }>()\n</script>\n";
    let parent_source = "<script setup lang=\"ts\">\nimport CorpusChild from './CorpusChild.vue'\n</script>\n<template>\n  <CorpusChild  />\n</template>\n";
    let (_temp, service, drain_handle, _provider, workspace_id) = make_definition_test_server(&[
        ("src/CorpusChild.vue", "vue", child_source),
        ("src/Corpus1.vue", "vue", parent_source),
    ])
    .await;
    let server = service.inner();
    let uri = workspace_uri(&workspace_id, "src/Corpus1.vue");
    settle_child_contracts(server, &uri, &workspace_id, &["src/CorpusChild.vue"]).await;
    let cursor = parent_source.find("<CorpusChild ").unwrap() + "<CorpusChild ".len();
    let position = LineIndex::new_utf16(parent_source)
        .offset_to_position(cursor as u32)
        .expect("completion position");
    let version = server.documents.get(&uri).expect("open").version;
    let edited = format!("{parent_source}<!-- edited -->\n");

    // Hold the fence so the edit queues on it, then send the completion.
    let fence = server.did_change_mutex.lock().await;
    let edit = super::super::lifecycle::handle_did_change(
        server,
        DidChangeTextDocumentParams {
            text_document: VersionedTextDocumentIdentifier {
                uri: uri.clone(),
                version: version + 1,
            },
            content_changes: vec![TextDocumentContentChangeEvent {
                range: None,
                range_length: None,
                text: edited.clone(),
            }],
        },
    );
    let completion = server.completion(completion_params(&uri, position, None));
    let release = async move {
        tokio::task::yield_now().await;
        drop(fence);
    };
    let ((), response, ()) = futures_util::future::join3(edit, completion, release).await;

    assert_eq!(
        server.documents.get(&uri).expect("open").version,
        version + 1,
        "the queued edit committed"
    );
    let labels = completion_labels(
        response.expect("a completion sent after the edit answers the edited revision"),
    );
    assert!(
        labels.contains(&"tone0".to_string()),
        "the completion answers the child's typed props: {labels:?}"
    );

    drain_handle.abort();
    drop(service);
}

/// D1 (open+edit+completion race): a completion that arrives BEFORE the did_open
/// notification has registered the document must HOLD for the in-flight open and
/// then answer typed — never return `Ok(None)`, which the editor renders as a
/// document-text (word) fallback. tower-lsp runs did_open and completion
/// concurrently, so this race is reachable under load.
///
/// RED pre-fix: with no hold, a completion on a not-yet-registered document
/// returned `Ok(None)` immediately (word fallback). GREEN post-fix: the handler
/// waits (bounded) for the open to land and answers the child's typed props.
#[tokio::test]
async fn completion_holds_for_in_flight_open_vue_ts_legacy_lane() {
    let child_source =
        "<script setup lang=\"ts\">\ndefineProps<{ label: string; zIndex?: number }>()\n</script>\n";
    let parent_source = "<script setup lang=\"ts\">\nimport DirectComp from './DirectComp.vue'\n</script>\n<template>\n  <DirectComp  />\n</template>\n";

    let temp = tempfile::tempdir().expect("temp dir");
    let workspace = temp.path().join("workspace");
    std::fs::create_dir_all(&workspace).expect("workspace dir");
    std::fs::write(workspace.join("tsconfig.json"), "{}").expect("write tsconfig");
    std::fs::create_dir_all(workspace.join("src")).expect("src dir");
    std::fs::write(workspace.join("src/DirectComp.vue"), child_source).expect("write child");
    std::fs::write(workspace.join("src/App.vue"), parent_source).expect("write parent");

    let provider = Arc::new(MockTypeProvider::new());
    let type_provider: Arc<dyn TypeProvider> = provider.clone();
    let vfs_workspace: Arc<dyn verter_workspace::WorkspaceAccess> = Arc::new(
        verter_workspace::FilesystemWorkspace::new(verter_workspace::FilesystemOptions::default()),
    );
    let host = Arc::new(VerterHost::new(HostConfig::default(), vfs_workspace));
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
    let drain_handle = tokio::spawn(async move {
        let mut socket = socket;
        while socket.next().await.is_some() {}
    });
    let workspace_id = crate::test_utils::canonical_test_path(&workspace);
    let server = service.inner();
    server.documents.set_semantic_analysis_enabled(true);
    host.configure_projects(vec![verter_workspace::ide_project_config(
        workspace_id.clone(),
        workspace_id.clone(),
        Some(format!("{workspace_id}/tsconfig.json")),
    )]);
    install_test_resolver_for_root(
        server,
        &workspace_id,
        Some(&format!("{workspace_id}/tsconfig.json")),
    );

    let app_id = format!("{workspace_id}/src/App.vue");
    host.upsert(UpsertRequest {
        canonical_id: Some(app_id),
        input_id: format!("{workspace_id}/src/App.vue"),
        source: parent_source.into(),
        file_language: FileLanguage::vue(),
        aliases: Vec::new(),
    })
    .expect("seed parent host source without registering the document");
    let warm_uri = workspace_uri(&workspace_id, "src/Warm.vue");
    let _ = server.documents.did_open(&TextDocumentItem {
        uri: warm_uri.clone(),
        language_id: "vue".to_string(),
        version: 1,
        text: parent_source.to_string(),
    });
    settle_child_contracts(server, &warm_uri, &workspace_id, &["src/DirectComp.vue"]).await;
    let app_uri = workspace_uri(&workspace_id, "src/App.vue");
    assert!(
        server.documents.get(&app_uri).is_none(),
        "the registration-race fixture must start before parent registration"
    );
    assert!(
        server
            .cached_child_public_contract(&format!("{workspace_id}/src/DirectComp.vue"))
            .is_some(),
        "the registration-race fixture must settle its child contract before the latch"
    );

    // The document is NOT open yet — the completion races the did_open.
    let cursor_pos = parent_source.find("<DirectComp ").unwrap() + "<DirectComp ".len();
    let line_index = LineIndex::new_utf16(parent_source);
    let position = line_index.offset_to_position(cursor_pos as u32).unwrap();

    // Deterministic ordering, not a guessed sleep: prove the completion is
    // genuinely parked (`Pending`) waiting on the document's registration
    // BEFORE `did_open` lands, the way the sibling
    // `completion_holds_for_in_flight_open_without_cold_loading_children`
    // already does. A fixed sleep only guesses that completion reached its
    // wait point first — under load `did_open` can land before completion is
    // even polled, so the hold this test targets is never exercised (a
    // vacuous pass), or completion can still be mid-registration when the
    // assertion runs.
    let mut completion = Box::pin(server.completion(completion_params(&app_uri, position, None)));
    assert!(
        matches!(
            futures_util::poll!(completion.as_mut()),
            std::task::Poll::Pending
        ),
        "completion must wait when the parent has not registered"
    );
    let _ = server.documents.did_open(&TextDocumentItem {
        uri: app_uri.clone(),
        language_id: "vue".to_string(),
        version: 1,
        text: parent_source.to_string(),
    });
    let completion_result = completion.await;

    let labels = completion_labels(completion_result.expect("completion request should succeed"));
    assert!(
        labels.contains(&"label".to_string()),
        "the held completion must answer the child's typed 'label' prop, got: {labels:?}"
    );
    assert!(
        labels.contains(&"z-index".to_string()),
        "the held completion must answer the child's typed 'z-index' prop, got: {labels:?}"
    );

    drain_handle.abort();
    drop(service);
}

#[tokio::test]
async fn completion_holds_for_in_flight_open_without_cold_loading_children() {
    for case in D1_CARRIER_CASES {
        let child_path = format!("src/DirectComp.{}", case.extension);
        let parent_path = format!("src/App.{}", case.extension);
        let (temp, service, drain_handle, _provider, workspace_id) =
            make_definition_test_server(&[]).await;
        let workspace = temp.path().join("workspace");
        std::fs::create_dir_all(workspace.join("src")).expect("source directory");
        std::fs::write(workspace.join(&child_path), case.child_source)
            .expect("write disk-only child");
        std::fs::write(workspace.join(&parent_path), case.edited_parent_source)
            .expect("write disk-only parent");

        let server = service.inner();
        let app_uri = workspace_uri(&workspace_id, &parent_path);
        let child_uri = workspace_uri(&workspace_id, &child_path);
        let child_canonical = crate::documents::uri_to_canonical_id(&child_uri);
        // The helper's background scanner may discover newly-written disk files;
        // force the discriminating request-start state to be genuinely cold.
        server.documents.host().evict(&child_canonical);
        assert!(
            server.documents.get(&app_uri).is_none()
                && server.documents.get(&child_uri).is_none()
                && server
                    .documents
                    .host()
                    .get_analysis(&child_canonical)
                    .is_none(),
            "{} precondition: parent is pre-registration and child is disk-only/cold",
            case.name
        );
        let cursor = case
            .edited_parent_source
            .find("<DirectComp ")
            .expect("component tag")
            + "<DirectComp ".len();
        let position = LineIndex::new_utf16(case.edited_parent_source)
            .offset_to_position(cursor as u32)
            .expect("completion position");

        let mut completion =
            Box::pin(server.completion(completion_params(&app_uri, position, None)));
        assert!(
            matches!(
                futures_util::poll!(completion.as_mut()),
                std::task::Poll::Pending
            ),
            "{} completion must wait when the parent has not registered",
            case.name
        );
        let _ = server.documents.did_open(&TextDocumentItem {
            uri: app_uri.clone(),
            language_id: case.language_id.to_string(),
            version: 1,
            text: case.edited_parent_source.to_string(),
        });
        completion
            .await
            .expect("completion request should succeed after did_open");
        assert!(
            server.documents.get(&app_uri).is_some(),
            "{} parent must be registered before completion returns",
            case.name
        );
        assert!(
            server.documents.get(&child_uri).is_none(),
            "{} child must remain unopened in the document registry",
            case.name
        );
        drain_handle.abort();
        drop(service);
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn unrelated_document_completion_does_not_wait_for_blocked_provider_update() {
    let child_source =
        "<script setup lang=\"ts\">\ndefineProps<{ independentProp: string }>()\n</script>\n";
    let editing_source = "<script setup lang=\"ts\">\nconst value = 1\n</script>\n<template><div>{{ value }}</div></template>\n";
    let edited_source = "<script setup lang=\"ts\">\nconst value = 2\n</script>\n<template><div>{{ value }}</div></template>\n";
    let independent_source = "<script setup lang=\"ts\">\nimport IndependentChild from './IndependentChild.vue'\n</script>\n<template><IndependentChild  /></template>\n";
    let (_temp, service, drain_handle, provider, workspace_id) =
        make_definition_test_server_with_kind(
            &[
                ("src/IndependentChild.vue", "vue", child_source),
                ("src/Editing.vue", "vue", editing_source),
                ("src/Independent.vue", "vue", independent_source),
            ],
            crate::TypeProviderKind::Tsgo,
        )
        .await;
    let server = service.inner();
    let editing_uri = workspace_uri(&workspace_id, "src/Editing.vue");
    let independent_uri = workspace_uri(&workspace_id, "src/Independent.vue");
    settle_child_contracts(
        server,
        &independent_uri,
        &workspace_id,
        &["src/IndependentChild.vue"],
    )
    .await;
    server.ensure_current_file_synced(&editing_uri).await;
    let editing_ide_path = server
        .active_ide_path_for_uri(&editing_uri)
        .expect("editing carrier provider path");
    let (update_arrived, update_release) = provider.block_update_file(&editing_ide_path);

    let edit = super::super::lifecycle::handle_did_change(
        server,
        DidChangeTextDocumentParams {
            text_document: VersionedTextDocumentIdentifier {
                uri: editing_uri,
                version: 2,
            },
            content_changes: vec![TextDocumentContentChangeEvent {
                range: None,
                range_length: None,
                text: edited_source.to_string(),
            }],
        },
    );
    let probe = async {
        update_arrived.notified().await;
        let cursor =
            independent_source.find("<IndependentChild ").unwrap() + "<IndependentChild ".len();
        let position = LineIndex::new_utf16(independent_source)
            .offset_to_position(cursor as u32)
            .expect("completion position");
        let result = tokio::time::timeout(
            BLOCKED_PROVIDER_PROBE_LIVENESS,
            server.completion(completion_params(&independent_uri, position, None)),
        )
        .await;
        // Released only now: everything above ran while the provider update was
        // still blocked, which is what makes this a non-blocking proof.
        update_release.notify_one();
        result
    };
    let ((), completion_result) = futures_util::future::join(edit, probe).await;
    let response = completion_result
        .expect("an unrelated completion must not wait for provider publication")
        .expect("completion request should succeed");
    let labels = completion_labels(response);
    assert!(
        labels.contains(&"independent-prop".to_string()),
        "unrelated completion must answer typed native props: {labels:?}"
    );

    drain_handle.abort();
    drop(service);
}

/// Runs on the serve thread, as every handler does in the shipped binary: this
/// body polls a `did_change`, a second `did_change` and a `completion` handler
/// future INLINE and nested, which is exactly the shape
/// [`crate::SERVE_THREAD_STACK_BYTES`] is sized for. On libtest's default
/// thread an unoptimized build overflows the stack before any assertion runs.
#[test]
fn unrelated_edit_commit_and_completion_do_not_wait_for_blocked_provider_update() {
    crate::run_on_serve_thread(|| {
        tokio::runtime::Builder::new_multi_thread()
            .enable_all()
            .build()
            .expect("test runtime must build")
            .block_on(async {
            let blocker_v1 = "<script setup lang=\"ts\">const blocker = 1</script>\n";
            let blocker_v2 = "<script setup lang=\"ts\">const blocker = 2</script>\n";
            let target_v1 = "<script setup lang=\"ts\">\nimport OldChild from './OldChild.vue'\n</script>\n<template><OldChild  /></template>\n";
            let target_v2 = "<script setup lang=\"ts\">\nimport NewChild from './NewChild.vue'\n</script>\n<template><NewChild  /></template>\n";
            let warm_source = "<script setup lang=\"ts\">\nimport NewChild from './NewChild.vue'\n</script>\n<template><NewChild /></template>\n";
            let (_temp, service, drain_handle, provider, workspace_id) =
                make_definition_test_server_with_kind(
                    &[
                        ("src/Blocker.vue", "vue", blocker_v1),
                        (
                            "src/OldChild.vue",
                            "vue",
                            "<script setup lang=\"ts\">defineProps<{ staleProp: string }>()</script>",
                        ),
                        (
                            "src/NewChild.vue",
                            "vue",
                            "<script setup lang=\"ts\">defineProps<{ currentProp: string }>()</script>",
                        ),
                        ("src/Target.vue", "vue", target_v1),
                        ("src/Warm.vue", "vue", warm_source),
                    ],
                    crate::TypeProviderKind::Tsgo,
                )
                .await;
            let server = service.inner();
            let blocker_uri = workspace_uri(&workspace_id, "src/Blocker.vue");
            let target_uri = workspace_uri(&workspace_id, "src/Target.vue");
            let warm_uri = workspace_uri(&workspace_id, "src/Warm.vue");
            settle_child_contracts(server, &warm_uri, &workspace_id, &["src/NewChild.vue"]).await;
            server.ensure_current_file_synced(&blocker_uri).await;
            let blocker_ide_path = server
                .active_ide_path_for_uri(&blocker_uri)
                .expect("blocker provider path");
            let (update_arrived, update_release) = provider.block_update_file(&blocker_ide_path);

            let blocked_update = super::super::lifecycle::handle_did_change(
                server,
                DidChangeTextDocumentParams {
                    text_document: VersionedTextDocumentIdentifier {
                        uri: blocker_uri,
                        version: 2,
                    },
                    content_changes: vec![TextDocumentContentChangeEvent {
                        range: None,
                        range_length: None,
                        text: blocker_v2.to_string(),
                    }],
                },
            );
            let probe = async {
                update_arrived.notified().await;
                let target_edit = super::super::lifecycle::handle_did_change(
                    server,
                    DidChangeTextDocumentParams {
                        text_document: VersionedTextDocumentIdentifier {
                            uri: target_uri.clone(),
                            version: 2,
                        },
                        content_changes: vec![TextDocumentContentChangeEvent {
                            range: None,
                            range_length: None,
                            text: target_v2.to_string(),
                        }],
                    },
                );
                let observe = async {
                    tokio::time::timeout(BLOCKED_PROVIDER_PROBE_LIVENESS, async {
                        loop {
                            if server.documents.get(&target_uri).map(|doc| doc.version) == Some(2) {
                                break;
                            }
                            tokio::task::yield_now().await;
                        }
                    })
                    .await
                    .expect("unrelated target commit must not wait for blocker provider publication");

                    settle_child_contracts(server, &target_uri, &workspace_id, &["src/NewChild.vue"]).await;

                    let cursor = target_v2.find("<NewChild ").unwrap() + "<NewChild ".len();
                    let position = LineIndex::new_utf16(target_v2)
                        .offset_to_position(cursor as u32)
                        .expect("current completion position");
                    let response = tokio::time::timeout(
                        BLOCKED_PROVIDER_PROBE_LIVENESS,
                        server.completion(completion_params(&target_uri, position, None)),
                    )
                    .await
                    .expect("completion must use the independently committed target edit")
                    .expect("completion succeeds");
                    let labels = completion_labels(response);
                    assert!(
                        labels.contains(&"current-prop".to_string()),
                        "completion must answer current target props: {labels:?}"
                    );
                    assert!(
                        !labels.contains(&"stale-prop".to_string()),
                        "completion must not answer pre-edit target props: {labels:?}"
                    );

                    update_release.notify_one();
                };
                futures_util::future::join(target_edit, observe).await;
            };
            futures_util::future::join(blocked_update, probe).await;

            drain_handle.abort();
            drop(service);
            });
    });
}

#[tokio::test(flavor = "multi_thread")]
async fn completion_answers_content_modified_when_the_document_advances_during_the_provider_query()
{
    let old_child =
        "<script setup lang=\"ts\">\ndefineProps<{ staleV1Prop: string }>()\n</script>\n";
    let new_child =
        "<script setup lang=\"ts\">\ndefineProps<{ currentV2Prop: string }>()\n</script>\n";
    let v1_source = "<script setup lang=\"ts\">\nimport OldChild from './OldChild.vue'\n</script>\n<template><OldChild  /></template>\n";
    let v2_source = "<script setup lang=\"ts\">\nimport NewChild from './NewChild.vue'\n</script>\n<template><NewChild  /></template>\n";
    let warm_source = "<script setup lang=\"ts\">\nimport OldChild from './OldChild.vue'\nimport NewChild from './NewChild.vue'\n</script>\n<template><OldChild /><NewChild /></template>\n";
    let (_temp, service, drain_handle, provider, workspace_id) =
        make_definition_test_server_with_kind(
            &[
                ("src/OldChild.vue", "vue", old_child),
                ("src/NewChild.vue", "vue", new_child),
                ("src/App.vue", "vue", v1_source),
                ("src/Warm.vue", "vue", warm_source),
            ],
            crate::TypeProviderKind::Tsgo,
        )
        .await;
    let server = service.inner();
    let app_uri = workspace_uri(&workspace_id, "src/App.vue");
    let warm_uri = workspace_uri(&workspace_id, "src/Warm.vue");
    settle_child_contracts(
        server,
        &warm_uri,
        &workspace_id,
        &["src/OldChild.vue", "src/NewChild.vue"],
    )
    .await;

    server.ensure_current_file_synced(&app_uri).await;
    let ide_path = server
        .active_ide_path_for_uri(&app_uri)
        .expect("synced provider path");
    let (query_arrived, query_release) = provider.block_get_completions(&ide_path);
    let cursor = v1_source.find("<OldChild ").unwrap() + "<OldChild ".len();
    let position = LineIndex::new_utf16(v1_source)
        .offset_to_position(cursor as u32)
        .expect("completion position");

    let completion = server.completion(completion_params(&app_uri, position, None));
    let edit = async {
        query_arrived.notified().await;
        let result = server.documents.did_change(&app_uri, 2, v2_source);
        assert!(
            result.changed,
            "v2 must commit while the v1 query is suspended"
        );
        settle_child_contracts(server, &app_uri, &workspace_id, &["src/NewChild.vue"]).await;
        query_release.notify_one();
    };
    let (completion_result, ()) = futures_util::future::join(completion, edit).await;
    assert!(
        matches!(&completion_result, Err(error) if error.code == tower_lsp_server::jsonrpc::ErrorCode::ContentModified),
        "an edit committed during the provider query answers ContentModified, never v1 props \n         or v2 props for the v1 cursor: {completion_result:?}"
    );
    let v2_cursor = v2_source.find("<NewChild ").unwrap() + "<NewChild ".len();
    let v2_position = LineIndex::new_utf16(v2_source)
        .offset_to_position(v2_cursor as u32)
        .expect("completion position");
    let labels = completion_labels(
        server
            .completion(completion_params(&app_uri, v2_position, None))
            .await
            .expect("a completion admitted against v2 succeeds"),
    );
    assert!(
        labels.contains(&"current-v2-prop".to_string())
            && !labels.contains(&"stale-v1-prop".to_string()),
        "a new request answers v2 native analysis: {labels:?}"
    );

    drain_handle.abort();
    drop(service);
}

/// A completion's cursor names a position in the revision it was admitted
/// against. An edit that commits while the request computes answers
/// `ContentModified` — the cursor is never reinterpreted against the later
/// revision — and a new request against the edited revision answers it.
#[tokio::test(flavor = "multi_thread")]
async fn completion_answers_content_modified_after_an_edit_and_a_new_request_answers_the_edit() {
    let child = |prop: &str| {
        format!("<script setup lang=\"ts\">defineProps<{{ {prop}: string }}>()</script>")
    };
    let source = |child_name: &str| {
        format!(
            "<script setup lang=\"ts\">\nimport Child from './{child_name}.vue'\n</script>\n<template><Child  /></template>\n"
        )
    };
    let v1 = source("V1Child");
    let v2 = source("V2Child");
    let warm_source = "<script setup lang=\"ts\">\nimport V1 from './V1Child.vue'\nimport V2 from './V2Child.vue'\n</script>\n<template><V1 /><V2 /></template>\n";
    let v1_child = child("vOneProp");
    let v2_child = child("vTwoProp");
    let (_temp, service, drain_handle, _provider, workspace_id) =
        make_definition_test_server_with_kind(
            &[
                ("src/V1Child.vue", "vue", &v1_child),
                ("src/V2Child.vue", "vue", &v2_child),
                ("src/App.vue", "vue", &v1),
                ("src/Warm.vue", "vue", warm_source),
            ],
            crate::TypeProviderKind::Tsgo,
        )
        .await;
    let server = service.inner();
    let app_uri = workspace_uri(&workspace_id, "src/App.vue");
    let warm_uri = workspace_uri(&workspace_id, "src/Warm.vue");
    settle_child_contracts(
        server,
        &warm_uri,
        &workspace_id,
        &["src/V1Child.vue", "src/V2Child.vue"],
    )
    .await;
    let (arrived, release) = server.pause_next_completion_after_snapshot();
    let cursor = v1.find("<Child ").unwrap() + "<Child ".len();
    let position = LineIndex::new_utf16(&v1)
        .offset_to_position(cursor as u32)
        .expect("completion position");

    let completion = server.completion(completion_params(&app_uri, position, None));
    let edit = async {
        arrived.notified().await;
        assert!(server.documents.did_change(&app_uri, 2, &v2).changed);
        settle_child_contracts(server, &app_uri, &workspace_id, &["src/V2Child.vue"]).await;
        release.notify_one();
    };
    let (raced, ()) = futures_util::future::join(completion, edit).await;
    assert!(
        matches!(&raced, Err(error) if error.code == tower_lsp_server::jsonrpc::ErrorCode::ContentModified),
        "a completion whose document was edited after admission answers ContentModified: {raced:?}"
    );

    let current = completion_labels(
        server
            .completion(completion_params(&app_uri, position, None))
            .await
            .expect("a completion admitted against the edited revision succeeds"),
    );
    assert!(
        current.contains(&"v-two-prop".to_string()) && !current.contains(&"v-one-prop".to_string()),
        "a new request answers the edited revision: {current:?}"
    );

    drain_handle.abort();
    drop(service);
}

/// A close and reopen that reuses the client version is a new document
/// incarnation: a completion admitted before the close answers
/// `ContentModified` rather than an answer for the reopened document, and a
/// new request after the reopen answers the reopened document.
#[tokio::test(flavor = "multi_thread")]
async fn completion_answers_content_modified_across_a_reused_version_reopen() {
    let child = |prop: &str| {
        format!("<script setup lang=\"ts\">defineProps<{{ {prop}: string }}>()</script>")
    };
    let source = |child_name: &str| {
        format!(
            "<script setup lang=\"ts\">\nimport Child from './{child_name}.vue'\n</script>\n<template><Child  /></template>\n"
        )
    };
    let before = source("BeforeChild");
    let reopened = source("ReChild");
    let warm_source = "<script setup lang=\"ts\">\nimport Before from './BeforeChild.vue'\nimport Re from './ReChild.vue'\n</script>\n<template><Before /><Re /></template>\n";
    let before_child = child("beforeCloseProp");
    let reopened_child = child("reopenedProp");
    let (_temp, service, drain_handle, _provider, workspace_id) =
        make_definition_test_server_with_kind(
            &[
                ("src/BeforeChild.vue", "vue", &before_child),
                ("src/ReChild.vue", "vue", &reopened_child),
                ("src/App.vue", "vue", &before),
                ("src/Warm.vue", "vue", warm_source),
            ],
            crate::TypeProviderKind::Tsgo,
        )
        .await;
    let server = service.inner();
    let app_uri = workspace_uri(&workspace_id, "src/App.vue");
    let warm_uri = workspace_uri(&workspace_id, "src/Warm.vue");
    settle_child_contracts(
        server,
        &warm_uri,
        &workspace_id,
        &["src/BeforeChild.vue", "src/ReChild.vue"],
    )
    .await;
    let version = server
        .documents
        .get(&app_uri)
        .expect("the carrier is open")
        .version;
    let (arrived, release) = server.pause_next_completion_after_snapshot();
    let cursor = before.find("<Child ").unwrap() + "<Child ".len();
    let position = LineIndex::new_utf16(&before)
        .offset_to_position(cursor as u32)
        .expect("completion position");

    let completion = server.completion(completion_params(&app_uri, position, None));
    let reopen = async {
        arrived.notified().await;
        super::super::lifecycle::handle_did_close(
            server,
            DidCloseTextDocumentParams {
                text_document: TextDocumentIdentifier {
                    uri: app_uri.clone(),
                },
            },
        )
        .await;
        super::super::lifecycle::handle_did_open(
            server,
            DidOpenTextDocumentParams {
                text_document: TextDocumentItem {
                    uri: app_uri.clone(),
                    language_id: "vue".to_string(),
                    version,
                    text: reopened.clone(),
                },
            },
        )
        .await;
        settle_child_contracts(server, &app_uri, &workspace_id, &["src/ReChild.vue"]).await;
        release.notify_one();
    };
    let (raced, ()) = futures_util::future::join(completion, reopen).await;
    assert!(
        matches!(&raced, Err(error) if error.code == tower_lsp_server::jsonrpc::ErrorCode::ContentModified),
        "a completion admitted before a reused-version reopen answers ContentModified: {raced:?}"
    );

    let current = completion_labels(
        server
            .completion(completion_params(&app_uri, position, None))
            .await
            .expect("a completion admitted against the reopened document succeeds"),
    );
    assert!(
        current.contains(&"reopened-prop".to_string())
            && !current.contains(&"before-close-prop".to_string()),
        "a new request after the reopen answers the reopened document: {current:?}"
    );

    drain_handle.abort();
    drop(service);
}

#[tokio::test(flavor = "multi_thread")]
async fn completion_answers_content_modified_when_a_reused_version_reopen_lands_during_the_provider_query(
) {
    let old_child = "<script setup lang=\"ts\">defineProps<{ oldOpenProp: string }>()</script>";
    let new_child = "<script setup lang=\"ts\">defineProps<{ newOpenProp: string }>()</script>";
    let old_source = "<script setup lang=\"ts\">\nimport Child from './OldChild.vue'\n</script>\n<template><Child  /></template>\n";
    let new_source = "<script setup lang=\"ts\">\nimport Child from './NewChild.vue'\n</script>\n<template><Child  /></template>\n";
    let warm_source = "<script setup lang=\"ts\">\nimport OldChild from './OldChild.vue'\nimport NewChild from './NewChild.vue'\n</script>\n<template><OldChild /><NewChild /></template>\n";
    let (_temp, service, drain_handle, provider, workspace_id) =
        make_definition_test_server_with_kind(
            &[
                ("src/OldChild.vue", "vue", old_child),
                ("src/NewChild.vue", "vue", new_child),
                ("src/App.vue", "vue", old_source),
                ("src/Warm.vue", "vue", warm_source),
            ],
            crate::TypeProviderKind::Tsgo,
        )
        .await;
    let server = service.inner();
    let app_uri = workspace_uri(&workspace_id, "src/App.vue");
    let warm_uri = workspace_uri(&workspace_id, "src/Warm.vue");
    settle_child_contracts(
        server,
        &warm_uri,
        &workspace_id,
        &["src/OldChild.vue", "src/NewChild.vue"],
    )
    .await;
    server.ensure_current_file_synced(&app_uri).await;
    let ide_path = server
        .active_ide_path_for_uri(&app_uri)
        .expect("provider path");
    let (query_arrived, query_release) = provider.block_get_completions(&ide_path);
    let cursor = old_source.find("<Child ").unwrap() + "<Child ".len();
    let position = LineIndex::new_utf16(old_source)
        .offset_to_position(cursor as u32)
        .expect("completion position");

    let completion = server.completion(completion_params(&app_uri, position, None));
    let reopen = async {
        query_arrived.notified().await;
        server.documents.did_close(&app_uri);
        let _ = server.documents.did_open(&TextDocumentItem {
            uri: app_uri.clone(),
            language_id: "vue".to_string(),
            version: 1,
            text: new_source.to_string(),
        });
        settle_child_contracts(server, &app_uri, &workspace_id, &["src/NewChild.vue"]).await;
        query_release.notify_one();
    };
    let (completion_result, ()) = futures_util::future::join(completion, reopen).await;
    assert!(
        matches!(&completion_result, Err(error) if error.code == tower_lsp_server::jsonrpc::ErrorCode::ContentModified),
        "a reused-version reopen during the provider query answers ContentModified, never the \n         reopened document's props for the closed document's cursor: {completion_result:?}"
    );
    let labels = completion_labels(
        server
            .completion(completion_params(&app_uri, position, None))
            .await
            .expect("a completion admitted against the reopened document succeeds"),
    );
    assert!(
        labels.contains(&"new-open-prop".to_string())
            && !labels.contains(&"old-open-prop".to_string()),
        "a new request answers the reopened document identity: {labels:?}"
    );

    drain_handle.abort();
    drop(service);
}

#[tokio::test]
async fn paired_svelte_component_attribute_completion_returns_child_props() {
    let child_source =
        "<script lang=\"ts\">let { pairedProp }: { pairedProp: string } = $props();</script>\n";
    let parent_source =
        "<script lang=\"ts\">\nimport Child from './Child.svelte';\n</script>\n<Child ></Child>\n";
    let (_temp, service, drain_handle, _provider, workspace_id) = make_definition_test_server(&[
        ("src/Child.svelte", "svelte", child_source),
        ("src/App.svelte", "svelte", parent_source),
    ])
    .await;
    let server = service.inner();
    let app_uri = workspace_uri(&workspace_id, "src/App.svelte");
    settle_child_contracts(server, &app_uri, &workspace_id, &["src/Child.svelte"]).await;
    let cursor = parent_source.find("<Child ").unwrap() + "<Child ".len();
    let position = LineIndex::new_utf16(parent_source)
        .offset_to_position(cursor as u32)
        .expect("completion position");
    let labels = completion_labels(
        server
            .completion(completion_params(&app_uri, position, None))
            .await
            .expect("completion succeeds"),
    );
    assert!(
        labels.contains(&"pairedProp".to_string()),
        "a paired Svelte component is template markup, not a custom SFC block: {labels:?}"
    );
    assert!(
        !labels.contains(&"lang".to_string()),
        "paired component attributes must not receive SFC-block attributes: {labels:?}"
    );

    drain_handle.abort();
    drop(service);
}

// The `<template #|` PUBLIC-BOUNDARY contract lives in
// `real_provider_tests::template_surface::completion_vue_slot_name_offers_child_declared_slots`.
// A `MockTypeProvider` server exercises Verter's native half in isolation and
// enables the analysis-sidebar opt-in, so a mock-backed shorthand lane passed
// while a real user's merged path returned zero items — the lanes below cover
// native shapes the boundary test does not (longhand syntax, used-slot
// filtering) and never stand in for it.

#[tokio::test]
async fn contract_slot_name_completion_longhand_offers_child_declared_slots() {
    let parent_source = "<script setup lang=\"ts\">\nimport MyComp from './MyComp.vue'\n</script>\n<template>\n  <MyComp>\n    <template v-slot:\n  </MyComp>\n</template>\n";
    let labels = d5_complete_labels(
        &[
            ("src/MyComp.vue", "vue", D5_CHILD_SOURCE),
            ("src/App.vue", "vue", parent_source),
        ],
        "src/App.vue",
        "<template v-slot:",
    )
    .await;
    for expected in ["header", "default", "mySlot"] {
        assert!(
            labels.contains(&expected.to_string()),
            "`<template v-slot:|` must offer declared slot {expected}, got: {labels:?}"
        );
    }
}

#[tokio::test]
async fn contract_slot_name_completion_filters_already_used_slots() {
    // Stable parse: the slot being completed is a CLOSED empty attribute
    // (`<template #="">`), so the typed element tree retains the usage and the
    // sibling `<template #header>` directive — the filter is a typed fact.
    let parent_source = "<script setup lang=\"ts\">\nimport MyComp from './MyComp.vue'\n</script>\n<template>\n  <MyComp>\n    <template #header=\"{ title }\">\n      <span>{{ title }}</span>\n    </template>\n    <template #=\"\"></template>\n  </MyComp>\n</template>\n";
    let (_temp, service, drain_handle, _provider, workspace_id) = make_definition_test_server(&[
        ("src/MyComp.vue", "vue", D5_CHILD_SOURCE),
        ("src/App.vue", "vue", parent_source),
    ])
    .await;
    let server = service.inner();
    let uri = workspace_uri(&workspace_id, "src/App.vue");
    settle_child_contracts(server, &uri, &workspace_id, &["src/MyComp.vue"]).await;
    let cursor = parent_source.rfind("<template #").unwrap() + "<template #".len();
    let position = LineIndex::new_utf16(parent_source)
        .offset_to_position(cursor as u32)
        .expect("completion position");
    let labels = completion_labels(
        server
            .completion(completion_params(&uri, position, None))
            .await
            .expect("completion succeeds"),
    );
    assert!(
        !labels.contains(&"header".to_string()),
        "already-used slot must be filtered, got: {labels:?}"
    );
    for expected in ["default", "mySlot"] {
        assert!(
            labels.contains(&expected.to_string()),
            "unused slot {expected} must remain, got: {labels:?}"
        );
    }
    drain_handle.abort();
    drop(service);
}

#[tokio::test]
async fn contract_svelte_snippet_slot_completion_offers_child_snippet_props() {
    let child_source = "<script setup lang=\"ts\">\ndefineProps<{ label: string }>()\ndefineSlots<{ header(props: { title: string }): any; default(): any }>()\n</script>\n<template><slot name=\"header\" title=\"t\" /><slot /></template>\n";
    let parent_source = "<script lang=\"ts\">\n  import IdeSurfaceChild from './IdeSurfaceChild.svelte';\n</script>\n<IdeSurfaceChild>\n  {#snippet \n</IdeSurfaceChild>\n";
    let parent_source = parent_source.replace("./IdeSurfaceChild.svelte", "./IdeSurfaceChild.vue");
    let (_temp, service, drain_handle, provider, workspace_id) =
        make_definition_test_server_with_config(
            &[
                ("src/IdeSurfaceChild.vue", "vue", child_source),
                ("src/App.svelte", "svelte", parent_source.as_str()),
            ],
            crate::TypeProviderKind::None,
            HostConfig {
                analysis_scope: Some(verter_semantic::analysis::AnalysisScope::BUILD),
                metrics_enabled: true,
                ..HostConfig::default()
            },
            false,
        )
        .await;
    let server = service.inner();
    let app_uri = workspace_uri(&workspace_id, "src/App.svelte");
    let child_uri = workspace_uri(&workspace_id, "src/IdeSurfaceChild.vue");
    let child_id = format!("{workspace_id}/src/IdeSurfaceChild.vue");
    server.documents.did_close(&child_uri);
    server.publish_import_dependencies_settled(&app_uri).await;
    let contract = server
        .cached_child_public_contract(&child_id)
        .expect("background child contract");
    let verter_session::framework::ComponentContractAvailability::Supported(contract) = &contract
    else {
        panic!("Svelte fixture must publish a supported contract: {contract:?}")
    };
    assert!(
        contract
            .slots
            .iter()
            .any(|slot| slot.name.as_ref() == "header"),
        "the public contract must own the header slot: {contract:?}"
    );
    assert!(server
        .documents
        .cached_semantic_analysis(&child_id)
        .is_none());
    let parent_analysis = server
        .documents
        .source_feature_analysis(&app_uri)
        .expect("parent source-feature analysis");
    assert!(
        parent_analysis.imports.iter().any(|import| {
            import.source == "./IdeSurfaceChild.vue"
                && import
                    .bindings
                    .iter()
                    .any(|binding| binding.name == "IdeSurfaceChild")
        }),
        "parent imports: {:?}",
        parent_analysis.imports
    );
    assert!(
        parent_analysis.template.is_none(),
        "the regression requires parser-owned ancestry without a parent template snapshot: {:?}",
        parent_analysis.template
    );

    let cursor = parent_source.find("{#snippet ").unwrap() + "{#snippet ".len();
    let position = LineIndex::new_utf16(&parent_source)
        .offset_to_position(cursor as u32)
        .expect("completion position");
    let metrics_before = server.documents.host().metrics_snapshot();
    let provenance_before = server.documents.host().provenance_snapshot();
    let projections_before = server.child_public_contract_projection_count_for_test();
    let provider_calls_before = provider.file_sync_calls().len();
    let workspace = server
        .vfs_workspace
        .read()
        .clone()
        .expect("published workspace");
    let reads_before = workspace.vfs_provenance_snapshot();
    let labels = completion_labels(
        server
            .completion(completion_params(&app_uri, position, None))
            .await
            .expect("completion succeeds"),
    );
    for expected in ["header", "default"] {
        assert!(
            labels.contains(&expected.to_string()),
            "`{{#snippet |` must offer the child's snippet prop {expected}, got: {labels:?}"
        );
    }
    assert!(
        !labels.contains(&"label".to_string()),
        "non-snippet props must not be offered, got: {labels:?}"
    );
    let metrics_after = server.documents.host().metrics_snapshot();
    let provenance_after = server.documents.host().provenance_snapshot();
    let reads_after = workspace.vfs_provenance_snapshot();
    assert_eq!(
        provenance_after.get_analysis_calls,
        provenance_before.get_analysis_calls
    );
    assert_eq!(
        metrics_after.compile_requests,
        metrics_before.compile_requests
    );
    assert_eq!(
        server.child_public_contract_projection_count_for_test(),
        projections_before
    );
    assert_eq!(provider.file_sync_calls().len(), provider_calls_before);
    assert_eq!(
        (
            reads_after.native_fs_read_file_miss_count,
            reads_after.native_fs_read_dir_count,
            reads_after.resolution_evidence_live_read_count,
        ),
        (
            reads_before.native_fs_read_file_miss_count,
            reads_before.native_fs_read_dir_count,
            reads_before.resolution_evidence_live_read_count,
        )
    );
    drain_handle.abort();
    drop(service);
}

#[tokio::test]
async fn contract_svelte_render_callee_completion_offers_in_scope_snippets() {
    let doc_source = "<script lang=\"ts\">\n  let items = $state([1, 2]);\n</script>\n\n{#snippet row(item: number)}\n  <li>{item}</li>\n{/snippet}\n\n{#snippet cell()}\n  <td>x</td>\n{/snippet}\n\n<ul>\n  {@render \n</ul>\n";
    let labels = d5_complete_labels(
        &[("src/List.svelte", "svelte", doc_source)],
        "src/List.svelte",
        "{@render ",
    )
    .await;
    for expected in ["row", "cell"] {
        assert!(
            labels.contains(&expected.to_string()),
            "`{{@render |` must offer in-scope snippet {expected}, got: {labels:?}"
        );
    }
}

#[tokio::test]
async fn svelte_progressive_script_bindings_survive_incomplete_member_edits() {
    for (label, language, declaration) in [
        ("ts", " lang=\"ts\"", "  let draftLabel: string = 'x';"),
        ("js", "", "  let draftLabel = 'x';"),
    ] {
        let initial =
            format!("<script{language}>\n{declaration}\n</script>\n<p>{{draftLabel}}</p>\n");
        let changed = initial.replace(
            "\n</script>",
            "\n  const childMemberCheckpoint = draftLabel.\n</script>",
        );
        let (_temp, service, drain_handle, _provider, workspace_id) =
            make_default_profile_definition_test_server(&[(
                "src/App.svelte",
                "svelte",
                initial.as_str(),
            )])
            .await;
        let server = service.inner();
        let app_uri = workspace_uri(&workspace_id, "src/App.svelte");

        assert!(server.documents.did_change(&app_uri, 2, &changed).changed);
        let position = find_document_position(
            server,
            &app_uri,
            "childMemberCheckpoint = draftLabel.",
            "childMemberCheckpoint = ".len() + 1,
        );
        let response = server
            .goto_definition(goto_definition_params(&app_uri, position))
            .await
            .expect("goto definition request")
            .unwrap_or_else(|| panic!("{label}: unchanged local binding must remain addressable"));
        let locations = definition_locations(response);
        assert_eq!(locations.len(), 1, "{label}: expected one authored target");
        assert_eq!(
            locations[0].uri, app_uri,
            "{label}: target must stay authored"
        );
        assert_eq!(
            locations[0].range.start.line,
            line_for_snippet(&changed, "let draftLabel"),
            "{label}: target must be the unchanged declaration"
        );

        drain_handle.abort();
        drop(service);
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn component_completion_cold_parent_import_converges_without_test_prewarm() {
    // This is a convergence deadline, not a latency assertion. The canonical
    // nextest surface runs thousands of tests concurrently, so a two-second
    // wall-clock deadline can expire before the background publication task is
    // scheduled even though the exact witness arrives immediately afterwards.
    const CONVERGENCE_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(10);
    const CHILD: &str = "<script setup lang=\"ts\">\ninterface DraftProps { title: string; unusedOnly?: boolean }\ndefineProps<DraftProps>()\n</script>\n";
    const PARENT: &str = "<script setup lang=\"ts\">\nimport DraftCard from './DraftCard.vue'\n</script>\n<template><DraftCard :title=\"heading\" /></template>\n";
    const PARENT_PARTIAL: &str = "<script setup lang=\"ts\">\nimport DraftCard from './DraftCard.vue'\n</script>\n<template><DraftCard :></template>\n";

    let (_temp, service, drain_handle, _provider, workspace_id) =
        make_definition_test_server_with_config(
            &[
                ("src/DraftCard.vue", "vue", CHILD),
                ("src/App.vue", "vue", PARENT),
            ],
            crate::TypeProviderKind::Tsserver,
            HostConfig {
                analysis_scope: Some(verter_semantic::analysis::AnalysisScope::BUILD),
                metrics_enabled: true,
                ..HostConfig::default()
            },
            false,
        )
        .await;
    let server = service.inner();
    let app_uri = workspace_uri(&workspace_id, "src/App.vue");
    let child_uri = workspace_uri(&workspace_id, "src/DraftCard.vue");
    let child_id = format!("{workspace_id}/src/DraftCard.vue");

    // Exercise the same lifecycle path as an editor restoring and typing into
    // empty buffers. Open both buffers before authoring the child, so its
    // background publication is fenced by the final workspace/resolver
    // snapshot and the later parent content edits are the only invalidation
    // pressure under test.
    server.documents.did_close(&app_uri);
    super::super::lifecycle::handle_did_open(
        server,
        DidOpenTextDocumentParams {
            text_document: TextDocumentItem {
                uri: app_uri.clone(),
                language_id: "vue".to_string(),
                version: 1,
                text: String::new(),
            },
        },
    )
    .await;

    // Do not call the test-only settled publication helper: the normal
    // post-edit background lane must publish the child's own contract before
    // its future parent imports it.
    server.documents.did_close(&child_uri);
    super::super::lifecycle::handle_did_open(
        server,
        DidOpenTextDocumentParams {
            text_document: TextDocumentItem {
                uri: child_uri.clone(),
                language_id: "vue".to_string(),
                version: 1,
                text: String::new(),
            },
        },
    )
    .await;
    super::super::lifecycle::handle_did_change(
        server,
        DidChangeTextDocumentParams {
            text_document: VersionedTextDocumentIdentifier {
                uri: child_uri.clone(),
                version: 2,
            },
            content_changes: vec![TextDocumentContentChangeEvent {
                range: None,
                range_length: None,
                text: CHILD.to_string(),
            }],
        },
    )
    .await;
    // Re-arm the background lane on every poll instead of resting on the one
    // debounced pass the edit scheduled. Publication is enqueue-driven: a pass
    // whose projection is not available yet returns a retryable outcome and
    // ends the lane, and only a later trigger restarts it. In an editor that
    // trigger is the next feature request that misses the contract; with no
    // request in flight this loop is the only thing left to supply it. The
    // enqueue is idle-guarded, so an already-active pass merely records a
    // trailing one — the contract still has to come from the normal background
    // publication, never a test-only prewarm.
    tokio::time::timeout(CONVERGENCE_TIMEOUT, async {
        while server.cached_child_public_contract(&child_id).is_none() {
            server.enqueue_import_dependency_publication_if_idle(&child_uri);
            tokio::time::sleep(std::time::Duration::from_millis(10)).await;
        }
    })
    .await
    .expect("normal child edit publication must commit its future import contract");

    assert!(
        server.cached_child_public_contract(&child_id).is_some(),
        "the committed child contract must be warm before progressive parent typing"
    );

    let mut version = 1;
    let component_head_end =
        PARENT.find("<DraftCard ").expect("component head") + "<DraftCard ".len();
    let mut edit_ends = (3..PARENT.len()).step_by(3).collect::<Vec<_>>();
    edit_ends.extend([component_head_end, PARENT.len()]);
    edit_ends.sort_unstable();
    edit_ends.dedup();
    for end in edit_ends {
        version += 1;
        super::super::lifecycle::handle_did_change(
            server,
            DidChangeTextDocumentParams {
                text_document: VersionedTextDocumentIdentifier {
                    uri: app_uri.clone(),
                    version,
                },
                content_changes: vec![TextDocumentContentChangeEvent {
                    range: None,
                    range_length: None,
                    text: PARENT[..end].to_string(),
                }],
            },
        )
        .await;
        if end == component_head_end {
            let position = LineIndex::new_utf16(&PARENT[..end])
                .offset_to_position(end as u32)
                .expect("component-head completion position");
            let labels = completion_labels(
                server
                    .completion(completion_params(&app_uri, position, None))
                    .await
                    .expect("component-head completion request"),
            );
            assert!(
                labels.contains(&"unused-only".to_string()),
                "the first `<DraftCard ` checkpoint must consume the already committed child contract: {labels:?}"
            );
        }
        tokio::time::sleep(std::time::Duration::from_millis(5)).await;
    }
    version += 1;
    super::super::lifecycle::handle_did_change(
        server,
        DidChangeTextDocumentParams {
            text_document: VersionedTextDocumentIdentifier {
                uri: app_uri.clone(),
                version,
            },
            content_changes: vec![TextDocumentContentChangeEvent {
                range: None,
                range_length: None,
                text: PARENT_PARTIAL.to_string(),
            }],
        },
    )
    .await;

    let cursor = PARENT_PARTIAL.find("<DraftCard :").expect("component") + "<DraftCard :".len();
    let position = LineIndex::new_utf16(PARENT_PARTIAL)
        .offset_to_position(cursor as u32)
        .expect("completion position");
    tokio::time::timeout(CONVERGENCE_TIMEOUT, async {
        loop {
            let labels = completion_labels(
                server
                    .completion(completion_params(&app_uri, position, None))
                    .await
                    .expect("completion request"),
            );
            if labels.contains(&"unused-only".to_string()) {
                break;
            }
            tokio::time::sleep(std::time::Duration::from_millis(10)).await;
        }
    })
    .await
    .expect("zero-length `:` directive head must retain child prop completion authority");

    drain_handle.abort();
    drop(service);
}

#[tokio::test(flavor = "multi_thread")]
async fn svelte_progressive_component_completion_uses_committed_child_contract() {
    for (provider_label, provider_kind) in [
        ("none", crate::TypeProviderKind::None),
        ("tsgo", crate::TypeProviderKind::Tsgo),
        ("tsserver", crate::TypeProviderKind::Tsserver),
        ("editor-tsserver", crate::TypeProviderKind::EditorTsserver),
    ] {
        for (label, child_source, script_language) in [
        (
            "ts",
            "<script lang=\"ts\">\ninterface DraftProps { title: string; unusedOnly?: boolean }\nlet { title }: DraftProps = $props();\n</script>\n<p>{title}</p>\n",
            " lang=\"ts\"",
        ),
        (
            "js",
            "<script>\n/** @type {{ title: string, unusedOnly?: boolean }} */\nlet { title } = $props();\n</script>\n<p>{title}</p>\n",
            "",
        ),
    ] {
        for (route, import_statement, barrel_source) in [
            ("direct", "import DraftCard from './DraftCard.svelte';", None),
            (
                "barrel",
                "import { DraftCard } from './components';",
                Some("export { default as DraftCard } from './DraftCard.svelte';\n"),
            ),
        ] {
            for (child_state, close_child) in [("loaded", false), ("cold", true)] {
                let parent_initial = format!(
                    "<script{script_language}>\n{import_statement}\n</script>\n<p>ready</p>\n"
                );
                let mut files = vec![
                    ("src/DraftCard.svelte", "svelte", child_source),
                    ("src/App.svelte", "svelte", parent_initial.as_str()),
                ];
                if let Some(barrel_source) = barrel_source {
                    files.push(("src/components.ts", "typescript", barrel_source));
                }
            let (_temp, service, drain_handle, provider, workspace_id) =
                make_definition_test_server_with_config(
                    &files,
                    provider_kind,
                    HostConfig {
                        analysis_scope: Some(verter_semantic::analysis::AnalysisScope::BUILD),
                        metrics_enabled: true,
                        ..HostConfig::default()
                    },
                    false,
                )
                .await;
            let server = service.inner();
            let app_uri = workspace_uri(&workspace_id, "src/App.svelte");
            let child_uri = workspace_uri(&workspace_id, "src/DraftCard.svelte");
            if close_child {
                server.documents.did_close(&child_uri);
            }
            server.publish_import_dependencies_settled(&app_uri).await;
            let child_id = format!("{workspace_id}/src/DraftCard.svelte");
            assert!(
                server.cached_child_public_contract(&child_id).is_some(),
                "{provider_label}/{label}/{route}/{child_state}: initial background publication must commit the child contract"
            );
            let provider_calls_before_parent_edit = provider.file_sync_calls().len();
            if matches!(provider_kind, crate::TypeProviderKind::None) {
                assert_eq!(
                    provider_calls_before_parent_edit,
                    0,
                    "{provider_label}/{label}/{route}/{child_state}: provider-neutral child publication must not open or update provider buffers"
                );
            }
            let changed = parent_initial.replace("<p>ready</p>", "<DraftCard un />");
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
                        text: changed.clone(),
                    }],
                },
            )
            .await;
            let cached = server
                .cached_child_public_contract(&child_id)
                .unwrap_or_else(|| panic!("{provider_label}/{label}/{route}/{child_state}: an unrelated parent edit must preserve the already-published child contract immediately"));
            let verter_session::framework::ComponentContractAvailability::Supported(contract) =
                cached
            else {
                panic!("{provider_label}/{label}/{route}/{child_state}: fixture contract must be supported")
            };
            assert!(
                contract.props.iter().any(|prop| prop.name.as_ref() == "unusedOnly"),
                "{provider_label}/{label}/{route}/{child_state}: published contract must retain an unused declared prop"
            );

            // `did_change` schedules import publication independently. The
            // cached contract above must remain immediately readable, but the
            // background lane can still be finishing a no-op/revalidation pass
            // under a loaded runner. Settle that lane before attributing any
            // later projection-count change to the foreground completion.
            server.publish_import_dependencies_settled(&app_uri).await;

            // Settle the independently-owned CURRENT-file provider surface
            // before measuring the child-contract completion seam. The
            // assertion below then discriminates child cache reads from either
            // parent repair work or child projection work.
            server.ensure_current_file_synced(&app_uri).await;

            // Foreground repair settles only the IDE buffer; did_change also
            // owes a debounced API sync and diagnostics pass. Await this
            // fixture's first dispatch and completed publish before sampling
            // counters, so those independent writes cannot be mistaken for
            // work performed by completion. A cleared dirty bit or live-task
            // gauge alone can be observed before the coordinator finishes.
            server.sync_coordinator.await_until(
                || {
                    let receipts = &server.sync_coordinator.receipts;
                    receipts.dispatch_ticks.load(std::sync::atomic::Ordering::SeqCst) > 0
                        && receipts.diags_published_count.load(std::sync::atomic::Ordering::SeqCst) > 0
                },
                || panic!("{provider_label}/{label}/{route}/{child_state}: the parent edit's deferred sync and diagnostics must finish"),
            ).await;

            let provider_calls_for_completion = provider.file_sync_calls().len();
            let projection_count = server.child_public_contract_projection_count_for_test();
            let compile_requests = server.documents.host().metrics_snapshot().compile_requests;
            let workspace = server
                .vfs_workspace
                .read()
                .clone()
                .expect("published test workspace");
            let workspace_reads = workspace.vfs_provenance_snapshot();
            let cursor = changed.find("<DraftCard un").expect("component")
                + "<DraftCard un".len();
            let position = LineIndex::new_utf16(&changed)
                .offset_to_position(cursor as u32)
                .expect("completion position");
            let labels = completion_labels(
                server
                    .completion(completion_params(&app_uri, position, None))
                    .await
                    .expect("completion request"),
            );
            assert!(
                labels.contains(&"unusedOnly".to_string()),
                "{provider_label}/{label}/{route}/{child_state}: committed declared child prop must survive the incomplete \
                 parent edit: {labels:?}"
            );
            assert_eq!(
                server.child_public_contract_projection_count_for_test(),
                projection_count,
                "{provider_label}/{label}/{route}/{child_state}: completion must not invoke the contract projector"
            );
            assert_eq!(
                server.documents.host().metrics_snapshot().compile_requests,
                compile_requests,
                "{provider_label}/{label}/{route}/{child_state}: completion must not start a host compile"
            );
            let workspace_reads_after = workspace.vfs_provenance_snapshot();
            assert_eq!(
                (
                    workspace_reads_after.native_fs_read_file_miss_count,
                    workspace_reads_after.native_fs_read_dir_count,
                    workspace_reads_after.resolution_evidence_live_read_count,
                ),
                (
                    workspace_reads.native_fs_read_file_miss_count,
                    workspace_reads.native_fs_read_dir_count,
                    workspace_reads.resolution_evidence_live_read_count,
                ),
                "{provider_label}/{label}/{route}/{child_state}: completion must not perform filesystem/evidence reads"
            );
            assert_eq!(
                provider.file_sync_calls().len(),
                provider_calls_for_completion,
                "{provider_label}/{label}/{route}/{child_state}: native completion must not publish provider buffers"
            );

            drain_handle.abort();
            drop(service);
        }
        }
    }
    }
}

#[tokio::test]
async fn completion_queries_type_provider_for_partial_scoped_slot_locals() {
    let provider = Arc::new(MockTypeProvider::new());
    let type_provider: Arc<dyn TypeProvider> = provider.clone();
    let service = make_hover_test_service(type_provider);
    let server = service.inner();
    install_test_resolver(server);

    let child_source = r#"<script setup lang="ts">
interface SlotItem {
  id: number
  name: string
}

defineSlots<{
  default(props: { slotItem: SlotItem; slotIndex: number; slotTotal: number }): any
}>()
</script>
<template>
  <slot :slotItem="{ id: 1, name: 'first' }" :slotIndex="0" :slotTotal="1" />
</template>
"#;
    let slot_source = r#"<script setup lang="ts">
import TypedSlotComp from './TypedSlotComp.vue'

const outerLabel = 'outer'
</script>

<template>
  <TypedSlotComp v-slot="{ slotItem, slotIndex, slotTotal }">
    <p>{{ sl }}</p>
    <p>{{ slotItem.name }}</p>
    <p>{{ slotIndex }}</p>
    <p>{{ slotTotal }}</p>
    <p>{{ outerLabel }}</p>
  </TypedSlotComp>
</template>
"#;

    let _child_uri = open_test_vue(server, "/workspace/src/TypedSlotComp.vue", child_source);
    let slot_uri = open_test_vue(server, "/workspace/src/TemplateSlotCases.vue", slot_source);
    let position = find_document_position(server, &slot_uri, "{{ sl }}", 5);
    let slot_ctx = synced_type_provider_context(server, &slot_uri).await;
    let slot_tsx_offset = merge::carrier_position_to_tsx_offset_validated(
        &position,
        &slot_ctx.carrier_line_index,
        &slot_ctx.mapper,
        &slot_ctx.tsx_line_index,
    )
    .expect("slot completion position should map to tsx");
    let slot_expr_context = classify_expression_context_with_trigger(
        &slot_ctx.tsx_content,
        slot_tsx_offset as usize,
        None,
    );
    let slot_snippet = debug_snippet(&slot_ctx.tsx_content, slot_tsx_offset as usize)
        .unwrap_or_else(|| ("<none>".to_string(), "<none>".to_string()));

    set_type_completions_at_vue_position(
        server,
        &provider,
        &slot_uri,
        position,
        vec![
            crate::type_provider::protocol::Completion {
                label: "slotItem".to_string(),
                kind: Some(crate::type_provider::protocol::CompletionKind::Variable),
                detail: Some("const slotItem: SlotItem".to_string()),
                documentation: None,
                edit_range_start: None,
                edit_range_end: None,
                text_edit_new_text: None,
                insert_text: None,
                sort_text: None,
                insert_text_format: None,
                commit_characters: None,
                filter_text: None,
                preselect: None,
                label_details: None,
                data: None,
            },
            crate::type_provider::protocol::Completion {
                label: "slotIndex".to_string(),
                kind: Some(crate::type_provider::protocol::CompletionKind::Variable),
                detail: Some("const slotIndex: number".to_string()),
                documentation: None,
                edit_range_start: None,
                edit_range_end: None,
                text_edit_new_text: None,
                insert_text: None,
                sort_text: None,
                insert_text_format: None,
                commit_characters: None,
                filter_text: None,
                preselect: None,
                label_details: None,
                data: None,
            },
            crate::type_provider::protocol::Completion {
                label: "slotTotal".to_string(),
                kind: Some(crate::type_provider::protocol::CompletionKind::Variable),
                detail: Some("const slotTotal: number".to_string()),
                documentation: None,
                edit_range_start: None,
                edit_range_end: None,
                text_edit_new_text: None,
                insert_text: None,
                sort_text: None,
                insert_text_format: None,
                commit_characters: None,
                filter_text: None,
                preselect: None,
                label_details: None,
                data: None,
            },
            crate::type_provider::protocol::Completion {
                label: "Set".to_string(),
                kind: Some(crate::type_provider::protocol::CompletionKind::Class),
                detail: Some("global".to_string()),
                documentation: None,
                edit_range_start: None,
                edit_range_end: None,
                text_edit_new_text: None,
                insert_text: None,
                sort_text: None,
                insert_text_format: None,
                commit_characters: None,
                filter_text: None,
                preselect: None,
                label_details: None,
                data: None,
            },
        ],
    );

    let labels = completion_labels(
        server
            .completion(completion_params(&slot_uri, position, None))
            .await
            .expect("completion request should succeed"),
    );
    let calls = provider.calls();

    assert!(
            labels.contains(&"slotItem".to_string()),
            "slotItem should be present, got: {labels:?}, expr_context={slot_expr_context:?}, tsx_before={:?}, tsx_after={:?}, calls={calls:?}",
            slot_snippet.0,
            slot_snippet.1,
        );
    assert!(
        labels.contains(&"slotIndex".to_string()),
        "slotIndex should be present, got: {labels:?}"
    );
    assert!(
        labels.contains(&"slotTotal".to_string()),
        "slotTotal should be present, got: {labels:?}"
    );
    assert!(
        !labels.contains(&"Set".to_string()),
        "global completions should stay filtered for partial slot locals, got: {labels:?}"
    );
}

#[tokio::test]
async fn completion_queries_type_provider_for_scoped_slot_member_access() {
    let provider = Arc::new(MockTypeProvider::new());
    let type_provider: Arc<dyn TypeProvider> = provider.clone();
    let service = make_hover_test_service(type_provider);
    let server = service.inner();
    install_test_resolver(server);

    let child_source = r#"<script setup lang="ts">
interface SlotItem {
  id: number
  name: string
}

defineSlots<{
  default(props: { slotItem: SlotItem; slotIndex: number; slotTotal: number }): any
}>()
</script>
<template>
  <slot :slotItem="{ id: 1, name: 'first' }" :slotIndex="0" :slotTotal="1" />
</template>
"#;
    let slot_source = r#"<script setup lang="ts">
import TypedSlotComp from './TypedSlotComp.vue'

const outerLabel = 'outer'
</script>

<template>
  <TypedSlotComp v-slot="{ slotItem, slotIndex, slotTotal }">
    <p>{{ sl }}</p>
    <p>{{ slotItem.name }}</p>
    <p>{{ slotIndex }}</p>
    <p>{{ slotTotal }}</p>
    <p>{{ outerLabel }}</p>
  </TypedSlotComp>
</template>
"#;

    let _child_uri = open_test_vue(server, "/workspace/src/TypedSlotComp.vue", child_source);
    let slot_uri = open_test_vue(server, "/workspace/src/TemplateSlotCases.vue", slot_source);
    let position = find_document_position(server, &slot_uri, "slotItem.name", 9);
    let slot_ctx = synced_type_provider_context(server, &slot_uri).await;
    let slot_tsx_offset = merge::carrier_position_to_tsx_offset_validated(
        &position,
        &slot_ctx.carrier_line_index,
        &slot_ctx.mapper,
        &slot_ctx.tsx_line_index,
    )
    .expect("slot member position should map to tsx");
    let slot_expr_context = classify_expression_context_with_trigger(
        &slot_ctx.tsx_content,
        slot_tsx_offset as usize,
        None,
    );
    let slot_snippet = debug_snippet(&slot_ctx.tsx_content, slot_tsx_offset as usize)
        .unwrap_or_else(|| ("<none>".to_string(), "<none>".to_string()));

    set_type_completions_at_vue_position(
        server,
        &provider,
        &slot_uri,
        position,
        vec![
            crate::type_provider::protocol::Completion {
                label: "name".to_string(),
                kind: Some(crate::type_provider::protocol::CompletionKind::Property),
                detail: Some("(property) name: string".to_string()),
                documentation: None,
                edit_range_start: None,
                edit_range_end: None,
                text_edit_new_text: None,
                insert_text: None,
                sort_text: None,
                insert_text_format: None,
                commit_characters: None,
                filter_text: None,
                preselect: None,
                label_details: None,
                data: None,
            },
            crate::type_provider::protocol::Completion {
                label: "id".to_string(),
                kind: Some(crate::type_provider::protocol::CompletionKind::Property),
                detail: Some("(property) id: number".to_string()),
                documentation: None,
                edit_range_start: None,
                edit_range_end: None,
                text_edit_new_text: None,
                insert_text: None,
                sort_text: None,
                insert_text_format: None,
                commit_characters: None,
                filter_text: None,
                preselect: None,
                label_details: None,
                data: None,
            },
            crate::type_provider::protocol::Completion {
                label: "outerLabel".to_string(),
                kind: Some(crate::type_provider::protocol::CompletionKind::Variable),
                detail: Some("const outerLabel: string".to_string()),
                documentation: None,
                edit_range_start: None,
                edit_range_end: None,
                text_edit_new_text: None,
                insert_text: None,
                sort_text: None,
                insert_text_format: None,
                commit_characters: None,
                filter_text: None,
                preselect: None,
                label_details: None,
                data: None,
            },
        ],
    );

    let labels = completion_labels(
        server
            .completion(completion_params(&slot_uri, position, None))
            .await
            .expect("completion request should succeed"),
    );
    let calls = provider.calls();

    assert!(
            labels.contains(&"name".to_string()),
            "name should be present for scoped-slot member access, got: {labels:?}, expr_context={slot_expr_context:?}, tsx_before={:?}, tsx_after={:?}, calls={calls:?}",
            slot_snippet.0,
            slot_snippet.1,
        );
    assert!(
        labels.contains(&"id".to_string()),
        "id should be present for scoped-slot member access, got: {labels:?}"
    );
    assert!(
        !labels.contains(&"outerLabel".to_string()),
        "member access should suppress outer scope identifiers, got: {labels:?}"
    );
}

/// Incomplete member access in `<script setup>` (`a.`).
///
/// The recovery codegen keeps the virtual file valid TSX, and the completion handler
/// classifies the SCRIPT dot boundary as `MemberAccess` so the dot-trigger + member-filtering
/// machinery runs (it previously ran for template expressions only). The provider
/// returns number members; the LSP surfaces them with NO `___VERTER___` recovery
/// token leaking into a label.
#[tokio::test]
async fn script_member_access_completion_returns_number_members() {
    let provider = Arc::new(MockTypeProvider::new());
    let type_provider: Arc<dyn TypeProvider> = provider.clone();
    let service = make_hover_test_service(type_provider);
    let server = service.inner();
    install_test_resolver(server);

    let source = "<script setup>\nlet a = 1;\na.\n</script>\n";
    let uri = open_test_vue(server, "/workspace/src/Member.vue", source);

    // The IDE virtual file must be VALID TSX — the recovery fix is what stops the
    // whole-file language service from degrading to "No Suggestions".
    let ide = server
        .documents
        .get_ide(&uri)
        .expect("IDE TSX should exist");
    {
        let alloc = oxc_allocator::Allocator::new();
        let parsed =
            verter_parser::oxc_parse::Parser::new(&alloc, &ide.code, oxc_span::SourceType::tsx())
                .parse();
        assert!(
            parsed.diagnostics.is_empty(),
            "the LSP must ship VALID TSX for `a.`, got {:?}\n--- TSX ---\n{}",
            parsed
                .diagnostics
                .iter()
                .map(|e| e.to_string())
                .collect::<Vec<_>>(),
            ide.code
        );
    }

    // Cursor right after the `a.` dot boundary.
    let position = find_document_position(server, &uri, "a.", 2);

    // Compute the TSX offset exactly as the completion handler does: the strict
    // mapper is None at the zero-width member boundary by design, so fall back to
    // the completion-boundary helper.
    let ctx = synced_type_provider_context(server, &uri).await;
    let vue_source = server.documents.get(&uri).unwrap().source.clone();
    let tsx_offset = merge::carrier_position_to_tsx_offset_validated(
        &position,
        &ctx.carrier_line_index,
        &ctx.mapper,
        &ctx.tsx_line_index,
    )
    .or_else(|| {
        merge::carrier_completion_member_boundary_offset(
            &position,
            &ctx.carrier_line_index,
            &ctx.mapper,
            &ctx.tsx_line_index,
            &ctx.tsx_content,
            &vue_source,
        )
    })
    .expect("the `a.` dot boundary must map to a TSX offset (strict or fallback)");

    // The script position must classify as member access.
    let expr_ctx =
        classify_expression_context_with_trigger(&ctx.tsx_content, tsx_offset as usize, Some("."));
    assert!(
        matches!(expr_ctx, ExpressionContext::MemberAccess),
        "script `a.` must classify as MemberAccess, got {expr_ctx:?}"
    );

    use crate::type_provider::protocol::CompletionKind;
    provider.set_completions(
        &ctx.tsx_path,
        tsx_offset,
        vec![
            mock_completion("toFixed", CompletionKind::Method),
            mock_completion("toString", CompletionKind::Method),
            mock_completion("valueOf", CompletionKind::Method),
            // A non-member global the provider would also offer — member filtering
            // must drop it.
            mock_completion("AbortController", CompletionKind::Variable),
        ],
    );

    // Both WITH the explicit `.` trigger and WITHOUT it (Ctrl+Space) must work.
    for trigger in [Some("."), None] {
        let labels = completion_labels(
            server
                .completion(completion_params(&uri, position, trigger))
                .await
                .expect("completion request should succeed"),
        );
        for member in ["toFixed", "toString", "valueOf"] {
            assert!(
                labels.contains(&member.to_string()),
                "script member access (trigger={trigger:?}) must return `{member}`, got {labels:?}"
            );
        }
        assert!(
            labels.iter().all(|l| !l.contains("___VERTER___")),
            "no completion label may leak a ___VERTER___ recovery token, got {labels:?}"
        );
        assert!(
            !labels.contains(&"AbortController".to_string()),
            "member-access filtering must drop non-member globals, got {labels:?}"
        );
    }
}

/// NEGATIVE: an ordinary `<script setup>` IDENTIFIER position must NOT
/// inherit the template-only `IdentifierExpected` TypeProvider suppression — TS
/// globals and imports are valid in script, so the provider's results flow
/// through. (In a template the same context skips the provider; in script it must
/// not.)
#[tokio::test]
async fn script_identifier_completion_is_not_suppressed() {
    let provider = Arc::new(MockTypeProvider::new());
    let type_provider: Arc<dyn TypeProvider> = provider.clone();
    let service = make_hover_test_service(type_provider);
    let server = service.inner();
    install_test_resolver(server);

    let source = "<script setup>\nconst myValue = 1;\nmyVal\n</script>\n";
    let uri = open_test_vue(server, "/workspace/src/Ident.vue", source);
    // Cursor INSIDE the partial reference `myVal` (after `myV`), a clearly-mapped
    // identifier position with prefix `myV`.
    let position = find_document_position(server, &uri, "myVal\n", 3);

    let ctx = synced_type_provider_context(server, &uri).await;
    let tsx_offset = merge::carrier_position_to_tsx_offset_validated(
        &position,
        &ctx.carrier_line_index,
        &ctx.mapper,
        &ctx.tsx_line_index,
    )
    .expect("script identifier position should map to tsx");

    use crate::type_provider::protocol::CompletionKind;
    provider.set_completions(
        &ctx.tsx_path,
        tsx_offset,
        vec![mock_completion("myValue", CompletionKind::Variable)],
    );

    let labels = completion_labels(
        server
            .completion(completion_params(&uri, position, None))
            .await
            .expect("completion request should succeed"),
    );
    assert!(
        labels.contains(&"myValue".to_string()),
        "ordinary script identifier completions must consult the TypeProvider \
         (must NOT inherit template IdentifierExpected suppression), got {labels:?}"
    );
}

#[tokio::test]
async fn completion_queries_type_provider_for_partial_scoped_slot_member_access() {
    let provider = Arc::new(MockTypeProvider::new());
    let type_provider: Arc<dyn TypeProvider> = provider.clone();
    let service = make_hover_test_service(type_provider);
    let server = service.inner();
    install_test_resolver(server);

    let child_source = r#"<script setup lang="ts">
interface SlotItem {
  id: number
  name: string
}

defineSlots<{
  default(props: { slotItem: SlotItem; slotIndex: number; slotTotal: number }): any
}>()
</script>
<template>
  <slot :slotItem="{ id: 1, name: 'first' }" :slotIndex="0" :slotTotal="1" />
</template>
"#;
    let slot_source = r#"<script setup lang="ts">
import TypedSlotComp from './TypedSlotComp.vue'

const outerLabel = 'outer'
</script>

<template>
  <TypedSlotComp v-slot="{ slotItem, slotIndex, slotTotal }">
    <p>{{ sl }}</p>
    <p>{{ slotItem.na }}</p>
    <p>{{ slotItem.name }}</p>
    <p>{{ slotIndex }}</p>
    <p>{{ slotTotal }}</p>
    <p>{{ outerLabel }}</p>
  </TypedSlotComp>
</template>
"#;

    let _child_uri = open_test_vue(server, "/workspace/src/TypedSlotComp.vue", child_source);
    let slot_uri = open_test_vue(server, "/workspace/src/TemplateSlotCases.vue", slot_source);
    let position = find_document_position(server, &slot_uri, "slotItem.na", 11);
    let slot_ctx = synced_type_provider_context(server, &slot_uri).await;
    let slot_tsx_offset = merge::carrier_position_to_tsx_offset_validated(
        &position,
        &slot_ctx.carrier_line_index,
        &slot_ctx.mapper,
        &slot_ctx.tsx_line_index,
    )
    .expect("partial slot member position should map to tsx");
    let slot_expr_context = classify_expression_context_with_trigger(
        &slot_ctx.tsx_content,
        slot_tsx_offset as usize,
        None,
    );
    let slot_snippet = debug_snippet(&slot_ctx.tsx_content, slot_tsx_offset as usize)
        .unwrap_or_else(|| ("<none>".to_string(), "<none>".to_string()));

    set_type_completions_at_vue_position(
        server,
        &provider,
        &slot_uri,
        position,
        vec![
            crate::type_provider::protocol::Completion {
                label: "name".to_string(),
                kind: Some(crate::type_provider::protocol::CompletionKind::Property),
                detail: Some("(property) name: string".to_string()),
                documentation: None,
                edit_range_start: None,
                edit_range_end: None,
                text_edit_new_text: None,
                insert_text: None,
                sort_text: None,
                insert_text_format: None,
                commit_characters: None,
                filter_text: None,
                preselect: None,
                label_details: None,
                data: None,
            },
            crate::type_provider::protocol::Completion {
                label: "id".to_string(),
                kind: Some(crate::type_provider::protocol::CompletionKind::Property),
                detail: Some("(property) id: number".to_string()),
                documentation: None,
                edit_range_start: None,
                edit_range_end: None,
                text_edit_new_text: None,
                insert_text: None,
                sort_text: None,
                insert_text_format: None,
                commit_characters: None,
                filter_text: None,
                preselect: None,
                label_details: None,
                data: None,
            },
            crate::type_provider::protocol::Completion {
                label: "outerLabel".to_string(),
                kind: Some(crate::type_provider::protocol::CompletionKind::Variable),
                detail: Some("const outerLabel: string".to_string()),
                documentation: None,
                edit_range_start: None,
                edit_range_end: None,
                text_edit_new_text: None,
                insert_text: None,
                sort_text: None,
                insert_text_format: None,
                commit_characters: None,
                filter_text: None,
                preselect: None,
                label_details: None,
                data: None,
            },
        ],
    );

    let labels = completion_labels(
        server
            .completion(completion_params(&slot_uri, position, None))
            .await
            .expect("completion request should succeed"),
    );
    let calls = provider.calls();

    assert!(
            labels.contains(&"name".to_string()),
            "name should be present for partial scoped-slot member access, got: {labels:?}, expr_context={slot_expr_context:?}, tsx_before={:?}, tsx_after={:?}, calls={calls:?}",
            slot_snippet.0,
            slot_snippet.1,
        );
    assert!(
        labels.contains(&"id".to_string()),
        "id should be present for partial scoped-slot member access, got: {labels:?}"
    );
    assert!(
        !labels.contains(&"outerLabel".to_string()),
        "partial member access should suppress outer scope identifiers, got: {labels:?}"
    );
}

#[tokio::test]
async fn completion_queries_type_provider_for_partial_vfor_member_access() {
    let provider = Arc::new(MockTypeProvider::new());
    let type_provider: Arc<dyn TypeProvider> = provider.clone();
    let service = make_hover_test_service(type_provider);
    let server = service.inner();
    install_test_resolver(server);

    let source = r#"<script setup lang="ts">
interface Action {
  label: string
  disabled: boolean
  handler: () => void
}

const actions: Action[] = [{ label: 'ok', disabled: false, handler: () => {} }]
</script>

<template>
  <div>
    <button v-for="action in actions" :key="action.label" :disabled="action.di">
      {{ action.label }}
    </button>
  </div>
</template>
"#;

    let uri = open_test_vue(server, "/workspace/src/App.vue", source);
    let position = find_document_position(server, &uri, "action.di", 7);
    let ctx = synced_type_provider_context(server, &uri).await;
    let tsx_offset = merge::carrier_position_to_tsx_offset_validated(
        &position,
        &ctx.carrier_line_index,
        &ctx.mapper,
        &ctx.tsx_line_index,
    )
    .expect("v-for member access position should map to tsx");
    let expr_context =
        classify_expression_context_with_trigger(&ctx.tsx_content, tsx_offset as usize, None);
    let snippet = debug_snippet(&ctx.tsx_content, tsx_offset as usize)
        .unwrap_or_else(|| ("<none>".to_string(), "<none>".to_string()));

    set_type_completions_at_vue_position(
        server,
        &provider,
        &uri,
        position,
        vec![
            crate::type_provider::protocol::Completion {
                label: "disabled".to_string(),
                kind: Some(crate::type_provider::protocol::CompletionKind::Property),
                detail: Some("(property) disabled: boolean".to_string()),
                documentation: None,
                edit_range_start: None,
                edit_range_end: None,
                text_edit_new_text: None,
                insert_text: None,
                sort_text: None,
                insert_text_format: None,
                commit_characters: None,
                filter_text: None,
                preselect: None,
                label_details: None,
                data: None,
            },
            crate::type_provider::protocol::Completion {
                label: "label".to_string(),
                kind: Some(crate::type_provider::protocol::CompletionKind::Property),
                detail: Some("(property) label: string".to_string()),
                documentation: None,
                edit_range_start: None,
                edit_range_end: None,
                text_edit_new_text: None,
                insert_text: None,
                sort_text: None,
                insert_text_format: None,
                commit_characters: None,
                filter_text: None,
                preselect: None,
                label_details: None,
                data: None,
            },
            crate::type_provider::protocol::Completion {
                label: "handler".to_string(),
                kind: Some(crate::type_provider::protocol::CompletionKind::Method),
                detail: Some("(method) handler(): void".to_string()),
                documentation: None,
                edit_range_start: None,
                edit_range_end: None,
                text_edit_new_text: None,
                insert_text: None,
                sort_text: None,
                insert_text_format: None,
                commit_characters: None,
                filter_text: None,
                preselect: None,
                label_details: None,
                data: None,
            },
            crate::type_provider::protocol::Completion {
                label: "actions".to_string(),
                kind: Some(crate::type_provider::protocol::CompletionKind::Variable),
                detail: Some("const actions: Action[]".to_string()),
                documentation: None,
                edit_range_start: None,
                edit_range_end: None,
                text_edit_new_text: None,
                insert_text: None,
                sort_text: None,
                insert_text_format: None,
                commit_characters: None,
                filter_text: None,
                preselect: None,
                label_details: None,
                data: None,
            },
        ],
    );

    let labels = completion_labels(
        server
            .completion(completion_params(&uri, position, None))
            .await
            .expect("completion request should succeed"),
    );
    let calls = provider.calls();

    assert!(
            labels.contains(&"disabled".to_string()),
            "disabled should be present for v-for member access, got: {labels:?}, expr_context={expr_context:?}, tsx_before={:?}, tsx_after={:?}, calls={calls:?}",
            snippet.0,
            snippet.1,
        );
    assert!(
        labels.contains(&"label".to_string()),
        "label should be present for v-for member access, got: {labels:?}"
    );
    assert!(
        labels.contains(&"handler".to_string()),
        "handler should be present for v-for member access, got: {labels:?}"
    );
    assert!(
        !labels.contains(&"actions".to_string()),
        "member access should suppress outer identifiers, got: {labels:?}"
    );
}

#[tokio::test]
async fn completion_queries_type_provider_for_fixture_vfor_member_access_after_broken_interpolation(
) {
    let provider = Arc::new(MockTypeProvider::new());
    let type_provider: Arc<dyn TypeProvider> = provider.clone();
    let service = make_hover_test_service(type_provider);
    let server = service.inner();
    install_test_resolver(server);

    let source =
        include_str!("../../../../../packages/vue-vscode/e2e/fixtures/single-project/src/App.vue");
    let uri = open_test_vue(server, "/workspace/src/App.vue", source);
    let position = find_document_position(server, &uri, "action.disabled", 7);
    let ctx = synced_type_provider_context(server, &uri).await;
    let tsx_offset = merge::carrier_position_to_tsx_offset_validated(
        &position,
        &ctx.carrier_line_index,
        &ctx.mapper,
        &ctx.tsx_line_index,
    )
    .expect("fixture member access position should map to tsx");
    let expr_context =
        classify_expression_context_with_trigger(&ctx.tsx_content, tsx_offset as usize, None);
    let snippet = debug_snippet(&ctx.tsx_content, tsx_offset as usize)
        .unwrap_or_else(|| ("<none>".to_string(), "<none>".to_string()));

    set_type_completions_at_vue_position(
        server,
        &provider,
        &uri,
        position,
        vec![
            crate::type_provider::protocol::Completion {
                label: "disabled".to_string(),
                kind: Some(crate::type_provider::protocol::CompletionKind::Property),
                detail: Some("(property) disabled: boolean".to_string()),
                documentation: None,
                edit_range_start: None,
                edit_range_end: None,
                text_edit_new_text: None,
                insert_text: None,
                sort_text: None,
                insert_text_format: None,
                commit_characters: None,
                filter_text: None,
                preselect: None,
                label_details: None,
                data: None,
            },
            crate::type_provider::protocol::Completion {
                label: "label".to_string(),
                kind: Some(crate::type_provider::protocol::CompletionKind::Property),
                detail: Some("(property) label: string".to_string()),
                documentation: None,
                edit_range_start: None,
                edit_range_end: None,
                text_edit_new_text: None,
                insert_text: None,
                sort_text: None,
                insert_text_format: None,
                commit_characters: None,
                filter_text: None,
                preselect: None,
                label_details: None,
                data: None,
            },
            crate::type_provider::protocol::Completion {
                label: "handler".to_string(),
                kind: Some(crate::type_provider::protocol::CompletionKind::Method),
                detail: Some("(method) handler(): void".to_string()),
                documentation: None,
                edit_range_start: None,
                edit_range_end: None,
                text_edit_new_text: None,
                insert_text: None,
                sort_text: None,
                insert_text_format: None,
                commit_characters: None,
                filter_text: None,
                preselect: None,
                label_details: None,
                data: None,
            },
        ],
    );

    let labels = completion_labels(
        server
            .completion(completion_params(&uri, position, None))
            .await
            .expect("completion request should succeed"),
    );
    let calls = provider.calls();

    assert!(
            labels.contains(&"disabled".to_string()),
            "disabled should be present for fixture v-for member access, got: {labels:?}, expr_context={expr_context:?}, tsx_before={:?}, tsx_after={:?}, calls={calls:?}",
            snippet.0,
            snippet.1,
        );
    assert!(
        labels.contains(&"label".to_string()),
        "label should be present for fixture v-for member access, got: {labels:?}"
    );
    assert!(
        labels.contains(&"handler".to_string()),
        "handler should be present for fixture v-for member access, got: {labels:?}"
    );
    assert!(
        !labels.contains(&"actions".to_string()),
        "fixture member access should suppress outer identifiers, got: {labels:?}"
    );
}

#[tokio::test]
async fn completion_queries_type_provider_for_scoped_slot_member_access_after_prior_partial_member_access(
) {
    let provider = Arc::new(MockTypeProvider::new());
    let type_provider: Arc<dyn TypeProvider> = provider.clone();
    let service = make_hover_test_service(type_provider);
    let server = service.inner();
    install_test_resolver(server);

    let child_source = r#"<script setup lang="ts">
interface SlotItem {
  id: number
  name: string
}

defineSlots<{
  default(props: { slotItem: SlotItem; slotIndex: number; slotTotal: number }): any
}>()
</script>
<template>
  <slot :slotItem="{ id: 1, name: 'first' }" :slotIndex="0" :slotTotal="1" />
</template>
"#;
    let slot_source = r#"<script setup lang="ts">
import TypedSlotComp from './TypedSlotComp.vue'

const outerLabel = 'outer'
</script>

<template>
  <TypedSlotComp v-slot="{ slotItem, slotIndex, slotTotal }">
    <p>{{ sl }}</p>
    <p>{{ slotItem.na }}</p>
    <p>{{ slotItem.name }}</p>
    <p>{{ slotIndex }}</p>
    <p>{{ slotTotal }}</p>
    <p>{{ outerLabel }}</p>
  </TypedSlotComp>
</template>
"#;

    let _child_uri = open_test_vue(server, "/workspace/src/TypedSlotComp.vue", child_source);
    let slot_uri = open_test_vue(server, "/workspace/src/TemplateSlotCases.vue", slot_source);
    let position = find_document_position(server, &slot_uri, "slotItem.name", 9);

    set_type_completions_at_vue_position(
        server,
        &provider,
        &slot_uri,
        position,
        vec![
            crate::type_provider::protocol::Completion {
                label: "name".to_string(),
                kind: Some(crate::type_provider::protocol::CompletionKind::Property),
                detail: Some("(property) name: string".to_string()),
                documentation: None,
                edit_range_start: None,
                edit_range_end: None,
                text_edit_new_text: None,
                insert_text: None,
                sort_text: None,
                insert_text_format: None,
                commit_characters: None,
                filter_text: None,
                preselect: None,
                label_details: None,
                data: None,
            },
            crate::type_provider::protocol::Completion {
                label: "id".to_string(),
                kind: Some(crate::type_provider::protocol::CompletionKind::Property),
                detail: Some("(property) id: number".to_string()),
                documentation: None,
                edit_range_start: None,
                edit_range_end: None,
                text_edit_new_text: None,
                insert_text: None,
                sort_text: None,
                insert_text_format: None,
                commit_characters: None,
                filter_text: None,
                preselect: None,
                label_details: None,
                data: None,
            },
            crate::type_provider::protocol::Completion {
                label: "outerLabel".to_string(),
                kind: Some(crate::type_provider::protocol::CompletionKind::Variable),
                detail: Some("const outerLabel: string".to_string()),
                documentation: None,
                edit_range_start: None,
                edit_range_end: None,
                text_edit_new_text: None,
                insert_text: None,
                sort_text: None,
                insert_text_format: None,
                commit_characters: None,
                filter_text: None,
                preselect: None,
                label_details: None,
                data: None,
            },
        ],
    );

    let labels = completion_labels(
        server
            .completion(completion_params(&slot_uri, position, Some(".")))
            .await
            .expect("completion request should succeed"),
    );

    assert!(
            labels.contains(&"name".to_string()),
            "name should be present for scoped-slot member access after prior partial member access, got: {labels:?}"
        );
    assert!(
            labels.contains(&"id".to_string()),
            "id should be present for scoped-slot member access after prior partial member access, got: {labels:?}"
        );
    assert!(
            !labels.contains(&"outerLabel".to_string()),
            "member access after prior partial member access should suppress outer scope identifiers, got: {labels:?}"
        );
}

#[tokio::test]
async fn completion_retries_member_access_without_dot_trigger_when_backend_returns_empty() {
    let provider = Arc::new(TriggerSensitiveCompletionProvider);
    let type_provider: Arc<dyn TypeProvider> = provider.clone();
    let service = make_hover_test_service(type_provider);
    let server = service.inner();
    install_test_resolver(server);

    let child_source = r#"<script setup lang="ts">
interface SlotItem {
  id: number
  name: string
}

defineSlots<{
  default(props: { slotItem: SlotItem; slotIndex: number; slotTotal: number }): any
}>()
</script>
<template>
  <slot :slotItem="{ id: 1, name: 'first' }" :slotIndex="0" :slotTotal="1" />
</template>
"#;
    let slot_source = r#"<script setup lang="ts">
import TypedSlotComp from './TypedSlotComp.vue'

const outerLabel = 'outer'
</script>

<template>
  <TypedSlotComp v-slot="{ slotItem, slotIndex, slotTotal }">
    <p>{{ slotItem.name }}</p>
    <p>{{ slotIndex }}</p>
    <p>{{ slotTotal }}</p>
    <p>{{ outerLabel }}</p>
  </TypedSlotComp>
</template>
"#;

    let _child_uri = open_test_vue(server, "/workspace/src/TypedSlotComp.vue", child_source);
    let slot_uri = open_test_vue(server, "/workspace/src/TemplateSlotCases.vue", slot_source);
    let position = find_document_position(server, &slot_uri, "slotItem.name", 9);

    let labels = completion_labels(
        server
            .completion(completion_params(&slot_uri, position, Some(".")))
            .await
            .expect("completion request should succeed"),
    );

    assert!(
        labels.contains(&"name".to_string()),
        "member access retry should recover property completions, got: {labels:?}"
    );
    assert!(
        labels.contains(&"id".to_string()),
        "member access retry should recover property completions, got: {labels:?}"
    );
    assert!(
        !labels.contains(&"outerLabel".to_string()),
        "member access retry should not fall back to outer identifiers, got: {labels:?}"
    );
}

#[tokio::test]
async fn completion_synthesizes_dot_trigger_for_member_access_without_trigger_character() {
    let provider = Arc::new(DotTriggerRequiredCompletionProvider::default());
    let type_provider: Arc<dyn TypeProvider> = provider.clone();
    // The provider identifies as tsgo, so exercise the direct-open surface. A
    // tsserver-kind service would route this synthetic provider through the
    // process-wide on-disk publish store; parallel unit tests using the same
    // `/workspace/src/App.vue` identity could then replace its manifest between
    // publish and capture. That is neither the provider contract under test nor
    // a hermetic request surface.
    let service = make_hover_test_service_tsgo(type_provider);
    let server = service.inner();
    install_test_resolver(server);

    let source = r#"<script setup lang="ts">
interface Action {
  label: string
  disabled: boolean
  handler: () => void
}

const actions: Action[] = [{ label: 'ok', disabled: false, handler: () => {} }]
</script>

<template>
  <div>
    <button v-for="action in actions" :key="action.label" :disabled="action.disabled">
      {{ action.label }}
    </button>
  </div>
</template>
"#;

    let uri = open_test_vue(server, "/workspace/src/App.vue", source);
    let position = find_document_position(server, &uri, "action.disabled", 7);
    let _ctx = synced_type_provider_context(server, &uri).await;

    let labels = completion_labels(
        server
            .completion(completion_params(&uri, position, None))
            .await
            .expect("completion request should succeed"),
    );

    assert!(
        labels.contains(&"disabled".to_string()),
        "member access completion should synthesize a dot trigger, got: {labels:?}"
    );
    assert!(
        labels.contains(&"label".to_string()),
        "member access completion should synthesize a dot trigger, got: {labels:?}"
    );
    assert!(
        labels.contains(&"handler".to_string()),
        "member access completion should synthesize a dot trigger, got: {labels:?}"
    );
    assert!(
            !labels.contains(&"actions".to_string()),
            "member access completion should stay scoped when synthesizing a dot trigger, got: {labels:?}"
        );
}

#[tokio::test]
async fn completion_queries_type_provider_for_partial_identifier_recovery() {
    let provider = Arc::new(MockTypeProvider::new());
    let type_provider: Arc<dyn TypeProvider> = provider.clone();
    let service = make_hover_test_service(type_provider);
    let server = service.inner();
    install_test_resolver(server);

    let recovery_source = r#"<script setup lang="ts">
import { ref } from 'vue'
import MyComp from './MyComp.vue'

const count = ref(1)

function safeAction() {
  count.value++
}

const broken =
</script>

<template>
  <div>
    <p>{{ cou }}</p>
    <p>{{ count }}</p>
    <button @click="safeAction">go</button>
    <MyComp foo="ok" :bar="count" />
  </div>
</template>
"#;

    let recovery_uri = open_test_vue(
        server,
        "/workspace/src/TemplateRecovery.vue",
        recovery_source,
    );
    let position = find_document_position(server, &recovery_uri, "{{ cou }}", 6);
    let recovery_ctx = synced_type_provider_context(server, &recovery_uri).await;
    let recovery_tsx_offset = merge::carrier_position_to_tsx_offset_validated(
        &position,
        &recovery_ctx.carrier_line_index,
        &recovery_ctx.mapper,
        &recovery_ctx.tsx_line_index,
    )
    .expect("recovery completion position should map to tsx");
    let recovery_expr_context = classify_expression_context_with_trigger(
        &recovery_ctx.tsx_content,
        recovery_tsx_offset as usize,
        None,
    );
    let recovery_snippet = debug_snippet(&recovery_ctx.tsx_content, recovery_tsx_offset as usize)
        .unwrap_or_else(|| ("<none>".to_string(), "<none>".to_string()));

    set_type_completions_at_vue_position(
        server,
        &provider,
        &recovery_uri,
        position,
        vec![
            crate::type_provider::protocol::Completion {
                label: "count".to_string(),
                kind: Some(crate::type_provider::protocol::CompletionKind::Variable),
                detail: Some("const count: Ref<number>".to_string()),
                documentation: None,
                edit_range_start: None,
                edit_range_end: None,
                text_edit_new_text: None,
                insert_text: None,
                sort_text: None,
                insert_text_format: None,
                commit_characters: None,
                filter_text: None,
                preselect: None,
                label_details: None,
                data: None,
            },
            crate::type_provider::protocol::Completion {
                label: "safeAction".to_string(),
                kind: Some(crate::type_provider::protocol::CompletionKind::Function),
                detail: Some("function safeAction(): void".to_string()),
                documentation: None,
                edit_range_start: None,
                edit_range_end: None,
                text_edit_new_text: None,
                insert_text: None,
                sort_text: None,
                insert_text_format: None,
                commit_characters: None,
                filter_text: None,
                preselect: None,
                label_details: None,
                data: None,
            },
            crate::type_provider::protocol::Completion {
                label: "console".to_string(),
                kind: Some(crate::type_provider::protocol::CompletionKind::Module),
                detail: Some("global".to_string()),
                documentation: None,
                edit_range_start: None,
                edit_range_end: None,
                text_edit_new_text: None,
                insert_text: None,
                sort_text: None,
                insert_text_format: None,
                commit_characters: None,
                filter_text: None,
                preselect: None,
                label_details: None,
                data: None,
            },
        ],
    );

    let labels = completion_labels(
        server
            .completion(completion_params(&recovery_uri, position, None))
            .await
            .expect("completion request should succeed"),
    );
    let calls = provider.calls();

    assert!(
            labels.contains(&"count".to_string()),
            "count should be present, got: {labels:?}, expr_context={recovery_expr_context:?}, tsx_before={:?}, tsx_after={:?}, calls={calls:?}",
            recovery_snippet.0,
            recovery_snippet.1,
        );
    assert!(
        !labels.contains(&"console".to_string()),
        "global completions should stay filtered for broken-script recovery, got: {labels:?}"
    );
}

#[tokio::test]
async fn completion_queries_type_provider_for_partial_function_recovery() {
    let provider = Arc::new(MockTypeProvider::new());
    let type_provider: Arc<dyn TypeProvider> = provider.clone();
    let service = make_hover_test_service(type_provider);
    let server = service.inner();
    install_test_resolver(server);

    let recovery_source = r#"<script setup lang="ts">
import { ref } from 'vue'
import MyComp from './MyComp.vue'

const count = ref(1)

function safeAction() {
  count.value++
}

const broken =
</script>

<template>
  <div>
    <p>{{ cou }}</p>
    <p>{{ safeA }}</p>
    <p>{{ count }}</p>
    <button @click="safeAction">go</button>
    <MyComp foo="ok" :bar="count" />
  </div>
</template>
"#;

    let recovery_uri = open_test_vue(
        server,
        "/workspace/src/TemplateRecovery.vue",
        recovery_source,
    );
    let position = find_document_position(server, &recovery_uri, "{{ safeA }}", 8);
    let recovery_ctx = synced_type_provider_context(server, &recovery_uri).await;
    let recovery_tsx_offset = merge::carrier_position_to_tsx_offset_validated(
        &position,
        &recovery_ctx.carrier_line_index,
        &recovery_ctx.mapper,
        &recovery_ctx.tsx_line_index,
    )
    .expect("partial function recovery position should map to tsx");
    let recovery_expr_context = classify_expression_context_with_trigger(
        &recovery_ctx.tsx_content,
        recovery_tsx_offset as usize,
        None,
    );
    let recovery_snippet = debug_snippet(&recovery_ctx.tsx_content, recovery_tsx_offset as usize)
        .unwrap_or_else(|| ("<none>".to_string(), "<none>".to_string()));

    set_type_completions_at_vue_position(
        server,
        &provider,
        &recovery_uri,
        position,
        vec![
            crate::type_provider::protocol::Completion {
                label: "safeAction".to_string(),
                kind: Some(crate::type_provider::protocol::CompletionKind::Function),
                detail: Some("function safeAction(): void".to_string()),
                documentation: None,
                edit_range_start: None,
                edit_range_end: None,
                text_edit_new_text: None,
                insert_text: None,
                sort_text: None,
                insert_text_format: None,
                commit_characters: None,
                filter_text: None,
                preselect: None,
                label_details: None,
                data: None,
            },
            crate::type_provider::protocol::Completion {
                label: "count".to_string(),
                kind: Some(crate::type_provider::protocol::CompletionKind::Variable),
                detail: Some("const count: Ref<number>".to_string()),
                documentation: None,
                edit_range_start: None,
                edit_range_end: None,
                text_edit_new_text: None,
                insert_text: None,
                sort_text: None,
                insert_text_format: None,
                commit_characters: None,
                filter_text: None,
                preselect: None,
                label_details: None,
                data: None,
            },
            crate::type_provider::protocol::Completion {
                label: "console".to_string(),
                kind: Some(crate::type_provider::protocol::CompletionKind::Module),
                detail: Some("global".to_string()),
                documentation: None,
                edit_range_start: None,
                edit_range_end: None,
                text_edit_new_text: None,
                insert_text: None,
                sort_text: None,
                insert_text_format: None,
                commit_characters: None,
                filter_text: None,
                preselect: None,
                label_details: None,
                data: None,
            },
        ],
    );

    let labels = completion_labels(
        server
            .completion(completion_params(&recovery_uri, position, None))
            .await
            .expect("completion request should succeed"),
    );
    let calls = provider.calls();

    assert!(
            labels.contains(&"safeAction".to_string()),
            "safeAction should be present after broken-script recovery, got: {labels:?}, expr_context={recovery_expr_context:?}, tsx_before={:?}, tsx_after={:?}, calls={calls:?}",
            recovery_snippet.0,
            recovery_snippet.1,
        );
    assert!(
        !labels.contains(&"console".to_string()),
        "global completions should stay filtered for broken-script recovery, got: {labels:?}"
    );
}

#[tokio::test]
async fn completion_queries_type_provider_for_nested_partial_member_access() {
    let provider = Arc::new(MockTypeProvider::new());
    let type_provider: Arc<dyn TypeProvider> = provider.clone();
    let service = make_hover_test_service(type_provider);
    let server = service.inner();
    install_test_resolver(server);

    let source = r#"<script setup lang="ts">
import { ref, computed, reactive } from 'vue'

const mixed = ref<string | number>(0)

interface DeepNested {
  deep: { value: string; count: number }
}
const nested = reactive<DeepNested>({ deep: { value: 'hello', count: 1 } })

const Status = { Active: 'active', Inactive: 'inactive' } as const
type StatusType = typeof Status[keyof typeof Status]
const currentStatus = ref<StatusType>('active')

interface HasName { name: string }
interface HasAge { age: number }
type Person = HasName & HasAge
const person = ref<Person>({ name: 'Alice', age: 30 })

const summary = computed(() => `${person.value.name}: ${person.value.age}`)
</script>
<template>
  <div>
    <p>{{ mixed }}</p>
    <p>{{ nested.deep.va }}</p>
    <p>{{ nested.deep }}</p>
    <p>{{ currentStatus }}</p>
    <p>{{ person }}</p>
    <p>{{ summary }}</p>
  </div>
</template>
"#;

    let uri = open_test_vue(server, "/workspace/src/TypeResolutionCases.vue", source);
    let position = find_document_position(server, &uri, "nested.deep.va", "nested.deep.va".len());

    set_type_completions_at_vue_position(
        server,
        &provider,
        &uri,
        position,
        vec![
            crate::type_provider::protocol::Completion {
                label: "value".to_string(),
                kind: Some(crate::type_provider::protocol::CompletionKind::Property),
                detail: Some("(property) value: string".to_string()),
                documentation: None,
                edit_range_start: None,
                edit_range_end: None,
                text_edit_new_text: None,
                insert_text: None,
                sort_text: None,
                insert_text_format: None,
                commit_characters: None,
                filter_text: None,
                preselect: None,
                label_details: None,
                data: None,
            },
            crate::type_provider::protocol::Completion {
                label: "count".to_string(),
                kind: Some(crate::type_provider::protocol::CompletionKind::Property),
                detail: Some("(property) count: number".to_string()),
                documentation: None,
                edit_range_start: None,
                edit_range_end: None,
                text_edit_new_text: None,
                insert_text: None,
                sort_text: None,
                insert_text_format: None,
                commit_characters: None,
                filter_text: None,
                preselect: None,
                label_details: None,
                data: None,
            },
        ],
    );

    let labels = completion_labels(
        server
            .completion(completion_params(&uri, position, None))
            .await
            .expect("completion request should succeed"),
    );

    assert!(
        labels.contains(&"value".to_string()),
        "value should be present for nested member access, got: {labels:?}"
    );
    assert!(
        labels.contains(&"count".to_string()),
        "count should be present for nested member access, got: {labels:?}"
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn tsgo_barrel_eager_sync_follows_the_complete_reexport_closure() {
    // Usage.vue -> `./components` (export *) -> `./Foo` (export {default as Foo} from './Foo.vue').
    // TSGO needs explicit rewritten buffers. A recursive BFS that classifies by
    // RESOLVED-target carrier-ness must sync `Foo.vue`'s surface through every
    // re-export hop; cycles terminate through the visited set rather than an
    // arbitrary depth/node cap.
    let foo = "<script setup lang=\"ts\">\ndefineProps<{ foo: boolean; bar?: string }>()\n</script>\n<template><div>{{ foo }}</div></template>\n";
    let mid_barrel = "export { default as Foo } from './Foo.vue'\n";
    let top_barrel = "export * from './Foo'\n";
    let usage = "<script setup lang=\"ts\">\nimport { Foo } from './components'\n</script>\n<template><Foo></Foo></template>\n";
    // tsgo: the barrel BFS reach is engine-agnostic; tsgo makes the terminal-carrier
    // sync an observable `open_file` (tsserver is publish-only for carriers).
    let (_temp, service, drain_handle, provider, workspace_id) =
        make_definition_test_server_with_kind(
            &[
                ("src/components/Foo/Foo.vue", "vue", foo),
                ("src/components/Foo/index.ts", "typescript", mid_barrel),
                ("src/components/index.ts", "typescript", top_barrel),
                ("src/Usage.vue", "vue", usage),
            ],
            crate::TypeProviderKind::Tsgo,
        )
        .await;

    let server = service.inner();
    let usage_uri = workspace_uri(&workspace_id, "src/Usage.vue");

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
        opened.iter().any(|p| p.contains("Foo.vue")),
        "recursive tsgo barrel sync must sync the nested re-export carrier Foo.vue surface; opened={opened:?}"
    );

    drain_handle.abort();
    drop(service);
}

#[tokio::test(start_paused = true)]
async fn completion_reopens_current_file_when_open_buffer_provider_loses_virtual_file_content() {
    let provider = Arc::new(LostContentCompletionProvider::default());
    let type_provider: Arc<dyn TypeProvider> = provider.clone();
    // The lost-content REOPEN recovery is the inferred-open-buffer mechanism
    // (close+reopen the carrier TSX to refresh content). It is tsgo-specific: under
    // tsserver the carrier content lives in the publish store (not an open buffer),
    // so the carrier-companion open verbs are no-ops and this reopen path does not
    // apply. Characterize it on the engine that uses it.
    let service = make_hover_test_service_tsgo(type_provider);
    let server = service.inner();
    install_test_resolver(server);

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

    let ctx = synced_type_provider_context(server, &uri).await;
    provider.drop_open_path(&ctx.tsx_path);

    let position = find_document_position(server, &uri, "action.disabled", 7);
    let recovery_started = tokio::time::Instant::now();
    let labels = completion_labels(
        server
            .completion(completion_params(&uri, position, None))
            .await
            .expect("completion request should succeed"),
    );
    let calls = provider.calls();
    assert_eq!(
        tokio::time::Instant::now(),
        recovery_started,
        "successful content repair must settle completion without a readiness timer"
    );
    let open_count = calls
        .iter()
        .filter(|call| {
            matches!(
                call,
                MockCall::OpenFile { path, .. } if path == &ctx.tsx_path
            )
        })
        .count();

    assert!(
            labels.contains(&"disabled".to_string()),
            "completion should recover after the provider loses the current-file TSX content, got: {labels:?}, calls={calls:?}"
        );
    assert!(
            labels.contains(&"label".to_string()),
            "completion should recover after the provider loses the current-file TSX content, got: {labels:?}"
        );
    assert!(
            open_count >= 2,
            "recovery should force a reopen of the current-file TSX path after provider content loss, calls={calls:?}"
        );
}

#[tokio::test(flavor = "multi_thread")]
async fn completion_syncs_current_file_api_when_open_buffer_provider_needs_self_public_api() {
    let provider = Arc::new(LostContentCompletionProvider::requiring_current_api());
    let type_provider: Arc<dyn TypeProvider> = provider.clone();
    // The lost-content current-API re-SYNC recovery is the inferred-open-buffer
    // mechanism (re-sync the carrier API content into an open buffer). It is
    // tsgo-specific: under tsserver the carrier API lives in the publish store, so
    // the carrier-companion sync verbs are no-ops. Characterize it on tsgo.
    let service = make_hover_test_service_tsgo(type_provider);
    let server = service.inner();
    install_test_resolver(server);

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

    let position = find_document_position(server, &uri, "action.disabled", 7);
    let labels = completion_labels(
        server
            .completion(completion_params(&uri, position, None))
            .await
            .expect("completion request should succeed"),
    );
    let calls = provider.calls();

    assert!(
            labels.contains(&"disabled".to_string()),
            "completion should recover by syncing the current file API when tsserver requires the self public API, got: {labels:?}, calls={calls:?}"
        );
    assert!(
            calls.iter().any(|call| matches!(
                call,
                MockCall::OpenFile { path, .. } if path == "/workspace/src/App.vue.verter.ts"
            )),
            "recovery should open the current file .vue.ts path when the provider requires it, calls={calls:?}"
        );
}

#[tokio::test(flavor = "multi_thread")]
async fn completion_with_real_tsserver_returns_fixture_vfor_member_access_properties() {
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

    let app_path = format!("{workspace_id}/src/App.vue");
    let app_source = std::fs::read_to_string(&app_path).expect("fixture App.vue should exist");
    let uri: Uri = format!("file://{app_path}")
        .parse()
        .expect("fixture uri should be valid");
    server
        .did_open(DidOpenTextDocumentParams {
            text_document: TextDocumentItem {
                uri: uri.clone(),
                language_id: "vue".to_string(),
                version: 1,
                text: app_source,
            },
        })
        .await;

    let position = find_document_position(server, &uri, "action.disabled", 7);
    let ctx = synced_type_provider_context(server, &uri).await;
    let tsx_offset = merge::carrier_position_to_tsx_offset_validated(
        &position,
        &ctx.carrier_line_index,
        &ctx.mapper,
        &ctx.tsx_line_index,
    )
    .expect("fixture member access position should map to tsx");
    let _expr_context =
        classify_expression_context_with_trigger(&ctx.tsx_content, tsx_offset as usize, None);

    // The staged fixture reaching a typed state in the real tsserver+plugin is
    // an eventual-consistency fact, not a fixed-duration one — a single sleep
    // guesses how long that takes and flips under load. Poll the DIRECT
    // provider probe (not the `server.completion` call under test) as a pure
    // readiness gate, with a generous overall budget; still FAIL (never
    // skip) once the budget is exhausted. This is deliberately NOT a retry
    // around the actual assertion: `server.completion` — the thing this
    // test exists to check — is called exactly ONCE, after readiness is
    // established, so a regression that broke completion on a genuinely
    // already-typed fixture cannot be masked by retrying the completion
    // call itself until it happens to pass.
    // External tsserver indexing is unowned: poll the DIRECT probe until
    // the typed surface appears. The 10s timeout is the sole watchdog;
    // iteration count is not a bound. `server.completion` is still called
    // exactly once after this returns.
    let direct_labels = tokio::time::timeout(std::time::Duration::from_secs(10), async {
        loop {
            // A transient error here (tsserver still indexing) is part of
            // the readiness gap this loop exists to absorb, not a hard
            // failure — only exhausting the watchdog below is.
            if let Ok(direct_result) = provider
                .get_completions(&ctx.tsx_path, tsx_offset, Some("."))
                .await
            {
                let labels: Vec<String> = direct_result
                    .items
                    .into_iter()
                    .map(|item| item.label)
                    .collect();
                if labels.contains(&"disabled".to_string()) {
                    return labels;
                }
            }
            tokio::time::sleep(std::time::Duration::from_millis(250)).await;
        }
    })
    .await;
    if direct_labels.is_err() {
        provider.shutdown().await;
        panic!(
            "fixture never reached a typed state in tsserver within the 10s watchdog; \
             direct probe never saw `disabled`"
        );
    }

    let labels = completion_labels(
        server
            .completion(completion_params(&uri, position, None))
            .await
            .expect("completion request should succeed"),
    );
    if !labels.contains(&"disabled".to_string()) {
        // FAIL, never skip. The fixture is confirmed typed (the direct probe
        // above saw `disabled`), so a missing `disabled` here is a real
        // `server.completion` defect, not a readiness gap.
        provider.shutdown().await;
        panic!(
            "member-access completion must resolve the fixture's typed surface; \
             `disabled` is missing even though the fixture is confirmed typed, \
             got: {labels:?}"
        );
    }
    assert!(
            labels.contains(&"label".to_string()),
            "real tsserver fixture member access should include label, got: {labels:?}, direct_labels={direct_labels:?}"
        );
    assert!(
            labels.contains(&"handler".to_string()),
            "real tsserver fixture member access should include handler, got: {labels:?}, direct_labels={direct_labels:?}"
        );
}

#[tokio::test(flavor = "multi_thread")]
async fn completion_with_real_tsserver_recovers_fixture_vfor_member_access_immediately_after_open()
{
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

    let app_path = format!("{workspace_id}/src/App.vue");
    let app_source = std::fs::read_to_string(&app_path).expect("fixture App.vue should exist");
    let uri: Uri = crate::uri::path_to_file_uri(&app_path).expect("fixture uri should be valid");
    server
        .did_open(DidOpenTextDocumentParams {
            text_document: TextDocumentItem {
                uri: uri.clone(),
                language_id: "vue".to_string(),
                version: 1,
                text: app_source,
            },
        })
        .await;

    let position = find_document_position(server, &uri, "action.disabled", 7);
    let labels = completion_labels(
        server
            .completion(completion_params(&uri, position, None))
            .await
            .expect("completion request should succeed"),
    );

    if !labels.contains(&"disabled".to_string()) {
        // FAIL, never skip. `disabled` is absent exactly when the fixture's
        // dependency surface did not resolve, and returning green here reports a
        // pass for a test that ran none of its assertions.
        provider.shutdown().await;
        panic!(
            "member-access completion must resolve the fixture's typed surface; \
             `disabled` is missing, which means the staged fixture's dependencies \
             did not materialize, got: {labels:?}"
        );
    }
    assert!(
        labels.contains(&"label".to_string()),
        "immediate real tsserver fixture member access should include label, got: {labels:?}"
    );
    assert!(
        labels.contains(&"handler".to_string()),
        "immediate real tsserver fixture member access should include handler, got: {labels:?}"
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn completion_with_real_tsserver_recovers_fixture_vfor_member_access_on_dot_trigger_immediately_after_open(
) {
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

    let app_path = format!("{workspace_id}/src/App.vue");
    let app_source = std::fs::read_to_string(&app_path).expect("fixture App.vue should exist");
    let uri: Uri = crate::uri::path_to_file_uri(&app_path).expect("fixture uri should be valid");
    server
        .did_open(DidOpenTextDocumentParams {
            text_document: TextDocumentItem {
                uri: uri.clone(),
                language_id: "vue".to_string(),
                version: 1,
                text: app_source,
            },
        })
        .await;

    let position = find_document_position(server, &uri, "action.disabled", 7);
    let labels = completion_labels(
        server
            .completion(completion_params(&uri, position, Some(".")))
            .await
            .expect("completion request should succeed"),
    );

    if !labels.contains(&"disabled".to_string()) {
        // FAIL, never skip. `disabled` is absent exactly when the fixture's
        // dependency surface did not resolve, and returning green here reports a
        // pass for a test that ran none of its assertions.
        provider.shutdown().await;
        panic!(
            "member-access completion must resolve the fixture's typed surface; \
             `disabled` is missing, which means the staged fixture's dependencies \
             did not materialize, got: {labels:?}"
        );
    }
    assert!(
            labels.contains(&"label".to_string()),
            "immediate dot-trigger real tsserver fixture member access should include label, got: {labels:?}"
        );
    assert!(
            labels.contains(&"handler".to_string()),
            "immediate dot-trigger real tsserver fixture member access should include handler, got: {labels:?}"
        );
}

#[tokio::test(flavor = "multi_thread")]
async fn completion_with_real_tsserver_recovers_when_current_file_sync_was_missed() {
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
    // Production request deadlines are ZERO: a correctness test validates the
    // RESULT, and a test-only 15s completion backstop cancels under load.
    // Hang detection is the independent nextest process watchdog, not a
    // load-sensitive request cancellation.
    let host = Arc::new(VerterHost::new_standalone(HostConfig::default()));
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

    let app_path = format!("{workspace_id}/src/App.vue");
    let app_source = std::fs::read_to_string(&app_path).expect("fixture App.vue should exist");
    let uri = open_test_vue(server, &app_path, &app_source);

    let position = find_document_position(server, &uri, "action.disabled", 7);

    // This fixture's whole premise is that the current-file sync to tsserver
    // was MISSED, so nothing pushes the file's content to tsserver except
    // `server.completion` itself repairing it — a decoupled direct-probe
    // readiness gate genuinely cannot converge on its own, unlike the
    // sibling immediately-after-open tests.
    //
    // That does NOT mean retrying the tested call itself: this call —
    // exactly once — is what actually triggers the repair (the real
    // production path: `completion` -> `repaired_type_provider_context` ->
    // `ensure_current_file_synced`), so it IS the mechanism under test. Its
    // result is intentionally not asserted on: the repair's own
    // request/response round-trip to the real spawned tsserver process can
    // return before tsserver has finished indexing, independent of whether
    // `completion` behaved correctly.
    // The first completion IS the repair: it must itself call
    // `ensure_current_file_synced`. Asserting only a later call would let a
    // regression that needs two requests pass. Labels may still be empty
    // until tsserver finishes indexing, so the discriminator here is the
    // sync side-effect, not the completion surface.
    let first = server
        .completion(completion_params(&uri, position, None))
        .await
        .expect("the repair-triggering completion request should succeed");
    let _first_labels = completion_labels(first);
    let app_canonical = format!("{workspace_id}/src/App.vue");
    let synced = server
        .provider_sync_state_for_source(&app_canonical)
        .expect("completion must repair the missed current-file sync");
    assert!(
        synced.ide_background_loaded || synced.commit_stamp.is_some(),
        "the first completion must have synced the current file, got {synced:?}"
    );

    // Now bound the wait on an INDEPENDENT direct probe (not `server.completion`)
    // for the real external tsserver process to finish indexing — genuine
    // multi-process eventual-consistency latency, unrelated to whether
    // `completion`'s own serving logic is correct. This is the SAME
    // readiness-gate shape as the sibling immediately-after-open tests.
    let ctx = synced_type_provider_context_surface_only(server, &uri);
    let tsx_offset = merge::carrier_position_to_tsx_offset_validated(
        &position,
        &ctx.carrier_line_index,
        &ctx.mapper,
        &ctx.tsx_line_index,
    )
    .expect("fixture member access position should map to tsx");
    let direct_labels = tokio::time::timeout(std::time::Duration::from_secs(10), async {
        loop {
            if let Ok(direct_result) = provider
                .get_completions(&ctx.tsx_path, tsx_offset, Some("."))
                .await
            {
                let labels: Vec<String> = direct_result
                    .items
                    .into_iter()
                    .map(|item| item.label)
                    .collect();
                if labels.contains(&"disabled".to_string()) {
                    return labels;
                }
            }
            tokio::time::sleep(std::time::Duration::from_millis(250)).await;
        }
    })
    .await;
    if direct_labels.is_err() {
        provider.shutdown().await;
        panic!(
            "completion's repair never reached a typed state in tsserver within the \
             10s watchdog; direct probe never saw `disabled`"
        );
    }

    // The single, untouched, asserted call: given the fixture is now
    // confirmed typed, `server.completion` must serve the correct surface.
    let labels = completion_labels(
        server
            .completion(completion_params(&uri, position, None))
            .await
            .expect("completion request should succeed"),
    );
    assert!(
        labels.contains(&"disabled".to_string()),
        "completion should repair a missed current-file tsserver sync, got: {labels:?}, \
         direct_labels={direct_labels:?}"
    );
    assert!(
        labels.contains(&"label".to_string()),
        "completion should repair a missed current-file tsserver sync, got: {labels:?}"
    );
    assert!(
        labels.contains(&"handler".to_string()),
        "completion should repair a missed current-file tsserver sync, got: {labels:?}"
    );
}

/// A closed carrier cannot enrich a completion with unqualified provider data.
#[tokio::test]
async fn completion_resolve_requires_an_open_document_for_provider_envelopes() {
    let provider = Arc::new(MockTypeProvider::new());
    provider.set_provider_id("tsserver");
    let service = make_hover_test_service(provider.clone());
    let server = service.inner();
    install_test_resolver(server);
    let uri = open_test_vue(
        server,
        "/workspace/App.vue",
        "<script setup lang=\"ts\">const count = 1</script><template>{{ count }}</template>",
    );
    let ctx = synced_type_provider_context(server, &uri).await;
    let item = tsserver_resolve_envelope_item("tsserver", &ctx.tsx_path, "computed");
    server.completion_resolve(item.clone()).await.unwrap();
    assert!(provider
        .calls()
        .iter()
        .any(|call| matches!(call, MockCall::ResolveCompletion { .. })));
    provider.clear_calls();
    server.documents.did_close(&uri);

    for explicit_uri in [false, true] {
        let mut closed_item = item.clone();
        if explicit_uri {
            closed_item.data.as_mut().unwrap()["uri"] = serde_json::json!(uri.as_str());
        }
        let result = server.completion_resolve(closed_item).await;
        assert!(
            matches!(result, Err(error) if error.code == tower_lsp_server::jsonrpc::ErrorCode::ContentModified)
        );
    }
    assert!(!provider
        .calls()
        .iter()
        .any(|call| matches!(call, MockCall::ResolveCompletion { .. })));
    let native = CompletionItem {
        label: "native".into(),
        ..Default::default()
    };
    assert_eq!(
        server.completion_resolve(native.clone()).await.unwrap(),
        native
    );
}

/// Dispatch reaches `resolve_completion` for the provider-NEUTRAL
/// `verter_resolve` envelope — NOT the old provider-baked `data.tsgo` gate.
///
/// Discriminating: a tsserver-kind mock provider (NOT tsgo) carrying the neutral
/// envelope MUST have `resolve_completion` invoked. The pre-fix code gated on
/// `data.get("tsgo") == Some(true)`, so a tsserver item would never reach
/// resolve — this asserts the call IS made, which fails on the old gate.
#[tokio::test]
async fn completion_resolve_dispatches_neutral_envelope_to_provider() {
    let provider = Arc::new(MockTypeProvider::new());
    provider.set_provider_id("tsserver");
    // Configure a resolve response keyed on the exact resolve key the envelope carries.
    let resolve_key = crate::type_provider::protocol::CompletionResolveData::TsserverEntry {
        name: "computed".to_string(),
        source: Some("vue".to_string()),
        data: Some(serde_json::json!({ "exportName": "computed" })),
        offset: 0,
    };
    let type_provider: Arc<dyn TypeProvider> = provider.clone();
    let service = make_hover_test_service(type_provider);
    let server = service.inner();
    install_test_resolver(server);
    let uri = open_test_vue(
        server,
        "/workspace/App.vue",
        "<script setup lang=\"ts\">const count = 1</script><template>{{ count }}</template>",
    );
    let ctx = synced_type_provider_context(server, &uri).await;
    provider.set_resolve_completion(
        &ctx.tsx_path,
        resolve_key,
        Some(CompletionResolveResult {
            additional_text_edits: vec![],
            ..Default::default()
        }),
    );

    let item = tsserver_resolve_envelope_item("tsserver", &ctx.tsx_path, "computed");
    let _ = super::super::nav_features::handle_completion_resolve(server, item).await;

    assert!(
        provider.calls().iter().any(|c| matches!(
            c,
            MockCall::ResolveCompletion { path, .. } if *path == ctx.tsx_path
        )),
        "the neutral verter_resolve envelope must dispatch to the provider's resolve_completion \
         (the old code gated on data.tsgo and never reached tsserver)"
    );
}

/// Provider-id mismatch FAILS CLOSED: an item minted by one provider must never
/// be resolved against a different active provider (mid-session swap safety).
///
/// Discriminating: the envelope says `provider_id: "tsgo"` but the active
/// provider reports `"tsserver"`. `resolve_completion` must NOT be called. A
/// dispatch that ignored provider identity would (wrongly) call it.
#[tokio::test]
async fn completion_resolve_fails_closed_on_provider_id_mismatch() {
    let provider = Arc::new(MockTypeProvider::new());
    provider.set_provider_id("tsserver");
    let type_provider: Arc<dyn TypeProvider> = provider.clone();
    let service = make_hover_test_service(type_provider);
    let server = service.inner();
    install_test_resolver(server);

    // Envelope stamped by a DIFFERENT provider ("tsgo") than the active one.
    let item = tsserver_resolve_envelope_item("tsgo", "/workspace/App.vue.tsx", "computed");
    let result = super::super::nav_features::handle_completion_resolve(server, item.clone()).await;

    assert!(
        result.is_ok(),
        "a provider mismatch is a benign no-op (item returned unchanged), not an error"
    );
    assert!(
        !provider
            .calls()
            .iter()
            .any(|c| matches!(c, MockCall::ResolveCompletion { .. })),
        "a provider-id mismatch must FAIL CLOSED — resolve_completion must not be called against \
         a foreign provider's item"
    );
}

/// BOTH a tsgo-kind and a tsserver-kind provider resolve through the SAME neutral
/// envelope dispatch — provider-agnostic (one path, validated by id), not a
/// tsgo-only branch — each carrying its OWN real resolve-key shape.
///
/// This models the REAL per-provider data shapes (review finding T3): TSGO mints
/// `CompletionResolveData::Lsp { label, data }` (the only key its
/// `resolve_completion` accepts), while the tsserver family mints
/// `TsserverEntry`. Feeding `TsserverEntry` to a tsgo provider (as the old test
/// did) proved only generic mock dispatch, not that the dispatch routes the
/// provider's real key shape. The dispatch must reach resolve AND carry the exact
/// key the matching provider would accept.
#[tokio::test]
async fn completion_resolve_envelope_resolves_for_both_provider_kinds() {
    // (provider kind, the resolve key that kind's real provider accepts, the
    // matching envelope-item builder).
    let tsgo_key = crate::type_provider::protocol::CompletionResolveData::Lsp {
        label: "computed".to_string(),
        data: serde_json::json!({ "exportName": "computed" }),
    };
    let tsserver_key = crate::type_provider::protocol::CompletionResolveData::TsserverEntry {
        name: "computed".to_string(),
        source: Some("vue".to_string()),
        data: Some(serde_json::json!({ "exportName": "computed" })),
        offset: 0,
    };

    for (kind, resolve_key, build_item) in [
        (
            "tsgo",
            tsgo_key,
            tsgo_resolve_envelope_item as fn(&str, &str, &str) -> CompletionItem,
        ),
        (
            "tsserver",
            tsserver_key,
            tsserver_resolve_envelope_item as fn(&str, &str, &str) -> CompletionItem,
        ),
    ] {
        let provider = Arc::new(MockTypeProvider::new());
        provider.set_provider_id(kind);
        let type_provider: Arc<dyn TypeProvider> = provider.clone();
        let service = make_hover_test_service(type_provider);
        let server = service.inner();
        install_test_resolver(server);
        let uri = open_test_vue(
            server,
            "/workspace/App.vue",
            "<script setup lang=\"ts\">const count = 1</script><template>{{ count }}</template>",
        );
        let ctx = synced_type_provider_context(server, &uri).await;
        provider.set_resolve_completion(
            &ctx.tsx_path,
            resolve_key.clone(),
            Some(CompletionResolveResult::default()),
        );

        let item = build_item(kind, &ctx.tsx_path, "computed");
        let _ = super::super::nav_features::handle_completion_resolve(server, item).await;

        // Resolve was reached AND the dispatched key is exactly the one this
        // provider's real `resolve_completion` accepts (the `Lsp`/`TsserverEntry`
        // shape — not a generic stand-in).
        let dispatched_key = provider.calls().into_iter().find_map(|c| match c {
            MockCall::ResolveCompletion { data, .. } => Some(data),
            _ => None,
        });
        assert_eq!(
            dispatched_key.as_ref(),
            Some(&resolve_key),
            "provider kind '{kind}' must receive its own real resolve-key shape, \
             routed unchanged through the neutral envelope"
        );
    }
}

/// F4: lazy `completionItem/resolve` enrichment is APPLIED — the resolved
/// `detail` and `documentation` are folded onto the returned `CompletionItem`.
///
/// Discriminating: `CompletionResolveResult` carries `detail`/`documentation`
/// and the tsserver mapper populates them, but the pre-fix
/// `handle_completion_resolve` consumed only `additional_text_edits`, dropping
/// the enrichment (a dishonest contract). This resolves an item whose result has
/// ONLY detail/docs (no edits) and asserts both land on the item — which fails on
/// the old edits-only dispatch.
#[tokio::test]
async fn completion_resolve_applies_detail_and_documentation() {
    let provider = Arc::new(MockTypeProvider::new());
    provider.set_provider_id("tsgo");
    let resolve_key = crate::type_provider::protocol::CompletionResolveData::Lsp {
        label: "computed".to_string(),
        data: serde_json::json!({ "exportName": "computed" }),
    };
    let type_provider: Arc<dyn TypeProvider> = provider.clone();
    let service = make_hover_test_service(type_provider);
    let server = service.inner();
    install_test_resolver(server);
    let uri = open_test_vue(
        server,
        "/workspace/App.vue",
        "<script setup lang=\"ts\">const count = 1</script><template>{{ count }}</template>",
    );
    let ctx = synced_type_provider_context(server, &uri).await;
    provider.set_resolve_completion(
        &ctx.tsx_path,
        resolve_key,
        Some(CompletionResolveResult {
            additional_text_edits: vec![],
            detail: Some("(alias) const computed: …".to_string()),
            documentation: Some("Takes a getter function…".to_string()),
            ..Default::default()
        }),
    );

    let item = tsgo_resolve_envelope_item("tsgo", &ctx.tsx_path, "computed");
    let resolved = super::super::nav_features::handle_completion_resolve(server, item)
        .await
        .expect("resolve returns the enriched item");

    assert_eq!(
        resolved.detail.as_deref(),
        Some("(alias) const computed: …"),
        "the resolved detail/signature must be applied onto the item"
    );
    match resolved.documentation {
        Some(Documentation::MarkupContent(MarkupContent { value, .. })) => {
            assert_eq!(value, "Takes a getter function…");
        }
        other => panic!("expected markdown documentation, got {other:?}"),
    }
}

/// A provider resolve answer is accepted only through a surface captured for
/// the open carrier before the query. An open carrier whose surface was never
/// synced, or whose source moved past the recorded surface, has nothing to
/// bracket the answer with: the resolve is refused without asking the provider,
/// so detail-only enrichment computed against an unknown surface never lands on
/// the item.
#[tokio::test]
async fn completion_resolve_refuses_provider_enrichment_without_a_current_surface() {
    let provider = Arc::new(MockTypeProvider::new());
    provider.set_provider_id("tsgo");
    let type_provider: Arc<dyn TypeProvider> = provider.clone();
    let service = make_hover_test_service(type_provider);
    let server = service.inner();
    install_test_resolver(server);
    let source =
        "<script setup lang=\"ts\">const count = 1</script><template>{{ count }}</template>";
    let uri = open_test_vue(server, "/workspace/App.vue", source);
    let arm = |path: &str| {
        provider.set_resolve_completion(
            path,
            crate::type_provider::protocol::CompletionResolveData::Lsp {
                label: "computed".to_string(),
                data: serde_json::json!({ "exportName": "computed" }),
            },
            Some(CompletionResolveResult {
                detail: Some("(alias) const computed: …".to_string()),
                ..Default::default()
            }),
        );
    };
    let assert_refused = |result: Result<CompletionItem>, case: &str| {
        assert!(
            matches!(&result, Err(error) if error.code == tower_lsp_server::jsonrpc::ErrorCode::ContentModified),
            "{case}: the resolve is refused, got {result:?}"
        );
        assert!(
            !provider
                .calls()
                .iter()
                .any(|call| matches!(call, MockCall::ResolveCompletion { .. })),
            "{case}: the provider is never asked without a surface to bracket its answer"
        );
    };

    // Open, but no surface was ever recorded for its provider path.
    let tsx_path = server
        .target_ide_path_for_uri(&uri)
        .expect("the open carrier has a provider path");
    arm(&tsx_path);
    provider.clear_calls();
    assert_refused(
        server
            .completion_resolve(tsgo_resolve_envelope_item("tsgo", &tsx_path, "computed"))
            .await,
        "unsynced surface",
    );

    // Synced, then edited past the recorded surface without a resync.
    let ctx = synced_type_provider_context(server, &uri).await;
    arm(&ctx.tsx_path);
    let _ = server
        .documents
        .did_change(&uri, 2, &source.replace("count = 1", "count = 22"));
    provider.clear_calls();
    assert_refused(
        server
            .completion_resolve(tsgo_resolve_envelope_item(
                "tsgo",
                &ctx.tsx_path,
                "computed",
            ))
            .await,
        "stale surface",
    );
}

/// Level 2 of the readiness ladder is announced by the post-scan completion
/// alone. Publishing a carrier writes the on-disk store and tells the editor
/// so, but a client (or a test gate) waiting for the provider to be synced must
/// never be released by it.
#[tokio::test(flavor = "multi_thread")]
async fn a_carrier_publication_never_announces_provider_sync_completion() {
    // The fixture opens two carriers and waits for both to be published and
    // certified; no scan and no post-scan completion ever runs.
    let fixture = watched_dependency_fixture(true).await;
    assert_eq!(
        *fixture.sync_complete.lock(),
        Vec::<u64>::new(),
        "no provider-sync completion was reached, so none may be announced"
    );
    assert!(
        fixture
            .store_changed
            .load(std::sync::atomic::Ordering::SeqCst)
            > 0,
        "the carrier publications still tell the editor the store changed"
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn post_scan_completion_refreshes_healthy_documents_while_another_carrier_is_pending() {
    let fixture = watched_dependency_fixture(true).await;
    let server = fixture.service.inner();
    let healthy = format!("{}/src/Consumer.vue", fixture.root);
    let healthy_uri = workspace_uri(&fixture.root, "src/Consumer.vue");
    let healthy_provider_path = server
        .capture_provider_request_surface(&healthy_uri)
        .unwrap()
        .stamp
        .provider_path
        .to_string();

    // An unrelated closed carrier whose provider delivery keeps failing.
    let broken = format!("{}/src/Broken.vue", fixture.root);
    std::fs::write(
        fixture._temp.path().join("src/Broken.vue"),
        "<template><p>broken</p></template>",
    )
    .unwrap();
    fixture.provider.set_fail_carrier_metadata_source(&broken);
    server
        .vfs_workspace
        .read()
        .as_ref()
        .unwrap()
        .apply_changes(vec![verter_workspace::WorkspaceChange::FileChanged {
            canonical_id: broken.clone(),
            source: None,
        }]);

    assert!(server.documents.diagnostics_ready(&healthy_uri));
    server.queue_snapshot_provider_sync(healthy.clone());
    server.queue_snapshot_provider_sync(broken.clone());
    drain_pending_provider_sync_for(server).await;
    assert!(
        server.pending_snapshot_provider_sync.contains(&broken),
        "precondition: the failing carrier stays queued"
    );
    assert!(
        !server.pending_snapshot_provider_sync.contains(&healthy),
        "precondition: the healthy carrier settles"
    );
    assert!(
        !server.documents.diagnostics_ready(&healthy_uri),
        "precondition: the drain advanced the healthy document past its completed receipt"
    );

    fixture.provider.clear_calls();
    let announced_before = fixture.sync_complete.lock().len();
    let generation = server
        .init_generation
        .load(std::sync::atomic::Ordering::Acquire);
    assert!(
        !server.complete_post_scan(generation).await,
        "level 2 must stay unannounced while provider work is pending"
    );
    server
        .sync_coordinator
        .await_until(
            || {
                server.documents.diagnostics_ready(&healthy_uri)
                    && server.sync_coordinator.diag_tasks_live() == 0
            },
            || {
                panic!(
                    "a healthy open document is owed a current receipt even while an unrelated carrier is pending"
                )
            },
        )
        .await;
    assert!(
        fixture.provider.calls().iter().any(
            |call| matches!(call, MockCall::GetDiagnostics { path } if path == &healthy_provider_path)
        ),
        "the receipt must come from a fresh provider pull: {:?}",
        fixture.provider.calls()
    );
    assert!(server.pending_snapshot_provider_sync.contains(&broken));
    assert_eq!(fixture.sync_complete.lock().len(), announced_before);
}

#[tokio::test(flavor = "multi_thread")]
async fn post_scan_completion_withholds_the_receipt_of_a_pending_open_document() {
    let fixture = watched_dependency_fixture(true).await;
    let server = fixture.service.inner();
    let pending = format!("{}/src/Consumer.vue", fixture.root);
    let pending_uri = workspace_uri(&fixture.root, "src/Consumer.vue");
    let settled_uri = workspace_uri(&fixture.root, "src/Unrelated.vue");

    server
        .documents
        .host()
        .bump_diagnostics_generation(&pending);
    server.queue_snapshot_provider_sync(pending.clone());
    let announced_before = fixture.sync_complete.lock().len();
    let pending_provider_path = server
        .capture_provider_request_surface(&pending_uri)
        .unwrap()
        .stamp
        .provider_path
        .to_string();
    fixture.provider.clear_calls();
    let generation = server
        .init_generation
        .load(std::sync::atomic::Ordering::Acquire);
    assert!(!server.complete_post_scan(generation).await);
    assert!(
        !server.documents.diagnostics_ready(&settled_uri),
        "the settled document's prior receipt is retired before the call returns"
    );
    server
        .sync_coordinator
        .await_until(
            || {
                server.documents.diagnostics_ready(&settled_uri)
                    && server.sync_coordinator.diag_tasks_live() == 0
            },
            || panic!("the settled open document never received its fresh receipt"),
        )
        .await;
    assert!(
        !server.documents.diagnostics_ready(&pending_uri),
        "a document whose provider sync is still pending must not be certified"
    );
    assert!(
        !fixture.provider.calls().iter().any(
            |call| matches!(call, MockCall::GetDiagnostics { path } if path == &pending_provider_path)
        ),
        "a pending document is not pulled at all: {:?}",
        fixture.provider.calls()
    );
    assert_eq!(fixture.sync_complete.lock().len(), announced_before);
}

#[tokio::test(flavor = "multi_thread")]
async fn post_scan_completion_resumes_after_a_late_provider_drain() {
    let fixture = watched_dependency_fixture(true).await;
    let server = fixture.service.inner();
    let path = format!("{}/src/helper.ts", fixture.root);
    server.queue_snapshot_provider_sync(path.clone());
    let generation = server
        .init_generation
        .load(std::sync::atomic::Ordering::Acquire);
    let completion = background_init::finish_post_scan(Arc::downgrade(&server.core), generation);
    tokio::pin!(completion);
    assert!(
        futures_util::poll!(&mut completion).is_pending(),
        "an unsettled scan must retain its completion until a later drain"
    );
    drain_pending_provider_sync_for(server).await;
    assert!(!server.pending_snapshot_provider_sync.contains(&path));
    assert!(
        tokio::time::timeout(std::time::Duration::from_secs(5), completion)
            .await
            .unwrap(),
        "a recovery drain must announce the existing scan without a new initialization"
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn post_scan_completion_announces_only_the_current_settled_generation() {
    let fixture = watched_dependency_fixture(true).await;
    let server = fixture.service.inner();
    let uri = workspace_uri(&fixture.root, "src/Consumer.vue");
    let generation = server
        .init_generation
        .load(std::sync::atomic::Ordering::Acquire);

    assert!(
        !server.complete_post_scan(generation + 1).await,
        "a generation that is not the live one must not announce or publish"
    );

    assert!(server.complete_post_scan(generation).await);
    server
        .sync_coordinator
        .await_until(
            || {
                server.documents.diagnostics_ready(&uri)
                    && server.sync_coordinator.diag_tasks_live() == 0
            },
            || panic!("the announced generation never republished its open documents"),
        )
        .await;
    tokio::time::timeout(std::time::Duration::from_secs(5), async {
        while fixture.sync_complete.lock().last() != Some(&generation) {
            tokio::task::yield_now().await;
        }
    })
    .await
    .expect("an empty pending set announces level 2");
    assert!(
        !fixture.sync_complete.lock().contains(&(generation + 1)),
        "the superseded generation must never be announced"
    );
}

/// F1 (virtual-file completion routing) discriminating regression test.
///
/// `handle_completion` has TWO provider-completion paths:
///   * the CARRIER path (`merge_completions`) for a real `.vue` URI, and
///   * the `verter-virtual://` path (`nav_features.rs` virtual-file branch),
///     which routes through `merge::provider_completion_to_lsp_item`.
///
/// The F1 fix was on the SECOND path: it previously stripped `Completion.data`,
/// so a provider auto-import returned on the virtual-file branch could never
/// carry its actionable `verter_resolve` envelope and could never resolve into
/// an import edit. The shipped VS Code E2E "auto-import" test opens a real
/// `.vue` URI — it exercises the CARRIER path, NOT this branch — so it does NOT
/// discriminate the F1 fix (reverting the virtual-file branch to the
/// `data`-stripping form leaves the whole corpus green).
///
/// This test drives `handle_completion` over a `verter-virtual://` URI with a
/// mock provider returning an ACTIONABLE `TsserverEntry` (a `source`-bearing
/// auto-import handle) and asserts the emitted LSP item carries the
/// provider-neutral `verter_resolve` envelope (kind + provider id + carrier
/// path + serialized provider key). Reverting the virtual-file branch to strip
/// `data` (e.g. mapping each item to a bare `CompletionItem { label, ... }`
/// with `data: None`) makes this assertion RED while the rest of the suite
/// stays green — the discriminator the §1a verification found missing.
#[tokio::test]
async fn virtual_file_completion_routes_actionable_handle_through_envelope() {
    // tsserver-kind mock so the envelope's `provider_id` is "tsserver" — proving
    // the path is provider-neutral, not a tsgo-only branch.
    let provider = Arc::new(MockTypeProvider::new());
    provider.set_provider_id("tsserver");

    // The virtual file routes completions to the source `.vue`'s generated TSX.
    let tsx_path = "/workspace/src/App.vue.tsx";
    // Virtual document content (already in generated-TSX coordinates). The
    // completion request lands at the end of `computed` on line 0.
    let virtual_content = "computed\n";
    let request_offset = "computed".len() as u32; // byte offset 8

    // An ACTIONABLE auto-import handle: a `TsserverEntry` carrying a module
    // `source` (the auto-import key). This is exactly the shape a real tsserver/
    // extension completion for an unimported `computed` from `vue` produces.
    let actionable = crate::type_provider::protocol::Completion {
        label: "computed".to_string(),
        kind: Some(crate::type_provider::protocol::CompletionKind::Function),
        detail: None,
        documentation: None,
        edit_range_start: None,
        edit_range_end: None,
        text_edit_new_text: None,
        insert_text: None,
        sort_text: None,
        insert_text_format: None,
        commit_characters: None,
        filter_text: None,
        preselect: None,
        label_details: None,
        data: Some(
            crate::type_provider::protocol::CompletionResolveData::TsserverEntry {
                name: "computed".to_string(),
                source: Some("vue".to_string()),
                data: None,
                offset: 0,
            },
        ),
    };
    // A NON-actionable local handle (no source/data) — must NOT earn an envelope.
    let local = crate::type_provider::protocol::Completion {
        label: "localVar".to_string(),
        kind: Some(crate::type_provider::protocol::CompletionKind::Variable),
        detail: None,
        documentation: None,
        edit_range_start: None,
        edit_range_end: None,
        text_edit_new_text: None,
        insert_text: None,
        sort_text: None,
        insert_text_format: None,
        commit_characters: None,
        filter_text: None,
        preselect: None,
        label_details: None,
        data: Some(
            crate::type_provider::protocol::CompletionResolveData::TsserverEntry {
                name: "localVar".to_string(),
                source: None,
                data: None,
                offset: 0,
            },
        ),
    };
    // The provider answers the virtual file's TSX path at the request offset.
    provider.set_completions(tsx_path, request_offset, vec![actionable, local]);

    let type_provider: Arc<dyn TypeProvider> = provider.clone();
    let service = make_hover_test_service(type_provider);
    let server = service.inner();
    install_test_resolver(server);

    // Open the backing `.vue` source so `get_canonical_id`/`active_ide_path_for_uri`
    // can resolve the source URI to its generated-TSX path.
    let source_uri: Uri = "file:///workspace/src/App.vue".parse().unwrap();
    let _ = server.documents.did_open(&TextDocumentItem {
        uri: source_uri.clone(),
        language_id: "vue".to_string(),
        version: 1,
        text: "<template><div/></template>".to_string(),
    });

    // Mark the source's provider sync state as IDE-loaded at the TSX path, so
    // `active_ide_path_for_uri(source)` returns the carrier path the virtual-file
    // branch routes completion + resolve against.
    server.provider_sync_states.insert(
        "/workspace/src/App.vue".to_string(),
        ProviderSyncState {
            owner_binding: crate::provider_sync::ProviderOwnerBinding::Unresolved,
            ide_path: Some(tsx_path.to_string()),
            api_path: None,
            decl_path: None,
            shadow_path: None,
            ide_background_loaded: true,
            api_background_loaded: false,
            decl_background_loaded: false,
            shadow_background_loaded: false,
            committed_ide_surface: None,
            committed_api_surface: None,
            commit_stamp: None,
            api_delivered_hash: None,
            api_observed_hash: None,
            shadow_delivered_source_hash: None,
        },
    );
    // Record the surface a successful IDE sync would have recorded — the
    // virtual-file routing context resolves the TSX path through the CAPTURED
    // surface, not an independent committed-path read. Fenced with the open
    // document's own current identity (no race here — this is test seeding).
    let seed_revision = server.documents.snapshot_identity(&source_uri);
    if let Some(sync) = server.project_sync.clone() {
        sync.open_tsx(tsx_path, virtual_content)
            .await
            .expect("virtual completion fixture publishes through the production open path");
    }
    server.record_carrier_ide_snapshot_with_pin(
        seed_revision
            .as_ref()
            .map(|revision| (&source_uri, revision)),
        "/workspace/src/App.vue",
        tsx_path,
        virtual_content,
        None,
    );

    // Open the virtual document (`verter-virtual://...?sourceUri=<vue-uri>`).
    let virtual_uri_str = format!(
        "verter-virtual://generated/App.vue.tsx?sourceUri={}",
        source_uri.as_str()
    );
    let virtual_uri: Uri = virtual_uri_str.parse().expect("virtual uri parses");
    let _ = server.documents.did_open(&TextDocumentItem {
        uri: virtual_uri.clone(),
        language_id: "typescriptreact".to_string(),
        version: 1,
        text: virtual_content.to_string(),
    });

    // Sanity: the virtual-file context resolves to the carrier TSX path (so we
    // know the branch under test is actually entered, not silently fallen
    // through to the normal completion path).
    let vf_ctx = server
        .virtual_file_context(&virtual_uri)
        .expect("virtual-file context must resolve to the source TSX path");
    assert_eq!(
        vf_ctx.tsx_path, tsx_path,
        "virtual-file context must route to the backing .vue's generated TSX"
    );

    // Completion at byte offset 8 (end of `computed`) on line 0.
    let position = Position {
        line: 0,
        character: request_offset,
    };
    let response = super::super::nav_features::handle_completion(
        server,
        completion_params(&virtual_uri, position, None),
    )
    .await
    .expect("handle_completion succeeds");

    let items = match response {
        Some(CompletionResponse::List(list)) => list.items,
        Some(CompletionResponse::Array(items)) => items,
        None => panic!("virtual-file completion must return items"),
    };

    // The provider went through the virtual-file branch (keyed on the TSX path).
    assert!(
        provider.calls().iter().any(|c| matches!(
            c,
            MockCall::GetCompletions { path, offset }
                if path == tsx_path && *offset == request_offset
        )),
        "completion must query the provider on the carrier TSX path via the virtual-file branch, \
         calls={:?}",
        provider.calls()
    );

    // The actionable auto-import item carries the neutral `verter_resolve` envelope.
    let computed = items
        .iter()
        .find(|i| i.label == "computed")
        .expect("the auto-import `computed` item must survive the virtual-file branch");
    let envelope = computed
        .data
        .as_ref()
        .and_then(|d| d.get("verter_resolve"))
        .unwrap_or_else(|| {
            panic!(
                "F1: the virtual-file branch must preserve the actionable resolve handle as a \
                 `verter_resolve` envelope (reverting the fix strips `data` and this is absent); \
                 got data={:?}",
                computed.data
            )
        });
    assert_eq!(
        envelope.get("kind").and_then(|v| v.as_str()),
        Some("type_provider"),
        "envelope kind must be type_provider"
    );
    assert_eq!(
        envelope.get("provider_id").and_then(|v| v.as_str()),
        Some("tsserver"),
        "envelope must carry the active provider id"
    );
    assert_eq!(
        envelope.get("provider_path").and_then(|v| v.as_str()),
        Some(tsx_path),
        "envelope must route resolve back to the carrier TSX path"
    );
    let provider_data = envelope
        .get("provider_data")
        .expect("envelope carries the serialized provider resolve key");
    assert_eq!(
        provider_data.get("source").and_then(|v| v.as_str()),
        Some("vue"),
        "the serialized handle must preserve the auto-import `source` key"
    );

    // Negative: the NON-actionable local item must NOT carry an envelope (no
    // per-keystroke payload bloat for a no-op resolve — review finding F3).
    let local = items
        .iter()
        .find(|i| i.label == "localVar")
        .expect("the local item must also pass through the virtual-file branch");
    assert!(
        local
            .data
            .as_ref()
            .and_then(|d| d.get("verter_resolve"))
            .is_none(),
        "a non-actionable local handle must NOT be stamped with a resolve envelope, got data={:?}",
        local.data
    );
}

/// S2 co-migration (mock): the v-bind completion `detail` renders through the
/// SAME shared boundary formatter — `(kind) display_signature` where the
/// structured kind exists. A bare-signature read would silently drop the
/// `(kind) ` prefix the pre-migration first-non-fence-line scrape carried.
#[tokio::test]
async fn v_bind_completion_detail_carries_kind_prefixed_signature() {
    let source = "<script setup lang=\"ts\">\nimport { ref } from 'vue'\nconst width = ref(10)\n</script>\n<template><div>x</div></template>\n<style scoped>\n.x { width: v-bind(); }\n</style>\n";
    let (_temp, service, drain_handle, provider, workspace_id) =
        make_definition_test_server(&[("src/App.vue", "vue", source)]).await;

    let uri = workspace_uri(&workspace_id, "src/App.vue");
    let server = service.inner();

    let decl_pos = find_document_position(server, &uri, "const width", 6);
    set_structured_hover_at_vue_position(
        server,
        &provider,
        &uri,
        decl_pos,
        crate::type_provider::protocol::HoverInfo {
            contents: "```typescript\n(const) const width: Ref<number>\n```".to_string(),
            display_signature: Some(crate::type_provider::mock::test_display_signature(
                "const width: Ref<number>",
            )),
            kind: Some(crate::type_provider::protocol::QuickInfoKind::Const),
            ..Default::default()
        },
    );

    let pos = find_document_position(server, &uri, "v-bind()", 7);
    let response = server
        .completion(completion_params(&uri, pos, None))
        .await
        .expect("completion request should succeed")
        .expect("v-bind completion must offer setup bindings");

    let items = match response {
        CompletionResponse::List(list) => list.items,
        CompletionResponse::Array(items) => items,
    };
    let width = items
        .iter()
        .find(|i| i.label == "width")
        .expect("setup binding offered by bare name");
    assert_eq!(
        width.detail.as_deref(),
        Some("(const) const width: Ref<number>"),
        "detail must render (kind) + display_signature through the shared boundary formatter"
    );

    drain_handle.abort();
    drop(service);
}

/// Completion inside `v-bind(|)` offers the setup bindings by bare name with
/// the provider type as detail — and never property-name/snippet junk.
#[tokio::test]
async fn v_bind_completion_offers_typed_setup_bindings() {
    let source = "<script setup lang=\"ts\">\nimport { ref } from 'vue'\nconst width = ref(10)\n</script>\n<template><div>x</div></template>\n<style scoped>\n.x { width: v-bind(); }\n</style>\n";
    let (_temp, service, drain_handle, provider, workspace_id) =
        make_definition_test_server(&[("src/App.vue", "vue", source)]).await;

    let uri = workspace_uri(&workspace_id, "src/App.vue");
    let server = service.inner();

    let decl_pos = find_document_position(server, &uri, "const width", 6);
    set_type_hover_at_vue_position(
        server,
        &provider,
        &uri,
        decl_pos,
        "const width: Ref<number>",
    );

    // Cursor between the parens of v-bind().
    let pos = find_document_position(server, &uri, "v-bind()", 7);
    let response = server
        .completion(completion_params(&uri, pos, None))
        .await
        .expect("completion request should succeed")
        .expect("v-bind completion must offer setup bindings");

    let items = match response {
        CompletionResponse::List(list) => list.items,
        CompletionResponse::Array(items) => items,
    };
    let width = items
        .iter()
        .find(|i| i.label == "width")
        .expect("setup binding offered by bare name");
    assert_eq!(
        width.detail.as_deref(),
        Some("const width: Ref<number>"),
        "provider type attached as detail"
    );
    assert!(
        !items.iter().any(|i| i.label.starts_with("v-bind(")),
        "no nested v-bind(...) snippet junk inside v-bind(: {:?}",
        items.iter().map(|i| &i.label).collect::<Vec<_>>()
    );
    assert!(
        !items.iter().any(|i| i.label == "display"),
        "no css property-name junk inside v-bind("
    );

    drain_handle.abort();
    drop(service);
}

#[tokio::test(start_paused = true)]
async fn generic_rename_fails_closed_while_project_carrier_frontier_is_incomplete() {
    let (_temp, service, drain_handle, provider, workspace_id) =
        make_definition_test_server_with_kind(
            &[
                ("src/MyComp.vue", "vue", READINESS_CHILD_SOURCE),
                ("src/App.vue", "vue", READINESS_PARENT_SOURCE),
            ],
            crate::TypeProviderKind::Tsgo,
        )
        .await;
    let server = service.inner();
    let app_uri = workspace_uri(&workspace_id, "src/App.vue");

    // This regression needs the production configured-project membership
    // frontier, not the broad spec-only fixture default.
    let workspace = server
        .vfs_workspace
        .read()
        .clone()
        .expect("test workspace is installed");
    let app_id = format!("{workspace_id}/src/App.vue");
    let child_id = format!("{workspace_id}/src/MyComp.vue");
    let root = verter_workspace::CanonicalPath::new(&workspace_id);
    let tsconfig = format!("{workspace_id}/tsconfig.json");
    let spec = verter_session_query::resolution::StaticMembershipSpec {
        files: Vec::new(),
        include: vec![verter_session_query::resolution::CompiledGlob::new(
            verter_session_query::resolution::NormalizedGlob::from_root_and_pattern(&root, "**/*"),
        )],
        exclude: Arc::from([]),
    };
    let materialized_files = [
        verter_workspace::CanonicalPath::new(&app_id),
        verter_workspace::CanonicalPath::new(&child_id),
    ]
    .into_iter()
    .collect();
    let projects = vec![verter_workspace::workspace_snapshot::OwnershipProject {
        id: verter_session_query::resolution::ProjectId(0),
        root: root.clone(),
        workspace_root: root.clone(),
        payload: verter_workspace::workspace_snapshot::ProjectPayload::Configured {
            tsconfig_path: verter_workspace::CanonicalPath::new(&tsconfig),
            membership: verter_session_query::resolution::ConfiguredMembership {
                spec,
                materialized_files,
            },
            compiler_options: verter_session_query::resolution::IdeProjectCompilerOptions::default(
            ),
            references: Vec::new(),
            workspace_aliases: Vec::new(),
        },
    }];
    let resolver =
        verter_resolution::ModuleResolverCore::new(vec![verter_workspace::ide_project_config(
            workspace_id.clone(),
            workspace_id.clone(),
            Some(tsconfig),
        )]);
    let snapshot = Arc::new(verter_workspace::WorkspaceSnapshot {
        owners_memo: Default::default(),
        projects,
        resolver,
        generation: verter_workspace::workspace_snapshot::SnapshotGeneration(2),
    });
    let views = crate::workspace_state::build_lsp_views(&*workspace, &snapshot, vec![]);
    workspace.publish_snapshot(verter_workspace::PublishedRoot::with_ext(
        snapshot,
        Box::new(views),
    ));

    server.ensure_current_file_synced(&app_uri).await;
    let position = find_document_position(server, &app_uri, "handleCustom(payload", 1);
    let ctx = server
        .type_provider_context(&app_uri)
        .expect("the initiating carrier surface is ready");
    let offset = merge::carrier_position_to_tsx_offset_validated(
        &position,
        &ctx.carrier_line_index,
        &ctx.mapper,
        &ctx.tsx_line_index,
    )
    .expect("rename position maps into the provider projection");
    provider.set_rename_locations(
        &ctx.tsx_path,
        offset,
        vec![crate::type_provider::protocol::RenameLocation {
            path: ctx.tsx_path.clone(),
            start: offset,
            end: offset + "handleCustom".len() as u32,
        }],
    );

    let profile = server.documents.tsx_profile.read().clone();
    let _ = server
        .documents
        .host()
        .ensure_ide_compiled(&child_id, &profile);
    let child_ide = server
        .documents
        .host()
        .get_ide(&child_id, &profile)
        .expect("the closed child has an IDE projection for publication");
    let parked_child_admission = match server
        .reconcile_carrier_via_gateway(&child_id, child_ide.is_jsx, Some(&child_ide), None)
        .await
    {
        crate::external_ts::CarrierSyncDecision::DirectOpen { pending, .. } => pending,
        _ => panic!("managed tsgo publication must return a pending direct-open"),
    };
    // Model the exact two-phase window: durable editor-store publication has
    // committed, but local tsgo has not confirmed the returned direct-open.
    server.provider_sync_states.remove(&child_id);

    let coordinator = server
        .carrier_publish_coordinator
        .as_ref()
        .expect("managed tsgo also publishes the editor-facing carrier store");
    let advertised = coordinator
        .activate_published_sources(&[app_id.clone(), child_id.clone()])
        .await
        .expect("store advertisement inspection succeeds");
    assert_eq!(
        advertised, 2,
        "precondition: the editor tsserver store is complete while local tsgo admission is parked"
    );
    assert!(
        server
            .provider_sync_state_for_source(&child_id)
            .is_none_or(|state| !state.ide_background_loaded || state.commit_stamp.is_none()),
        "precondition: store completeness must not imply local tsgo completeness"
    );

    // `edit.is_none()` below discriminates fail-closed correctness itself —
    // `provider_sync_states` was left incomplete for `child_id` above, so a
    // correct rename must decline without escaping a same-file subset. But a
    // wall-clock ceiling around that (the original `elapsed < 500ms`) either
    // flips under machine load or, loosened enough to survive load, stops
    // discriminating "instant" from "an arbitrarily slow failure" at all —
    // the exact tension this test previously resolved by dropping the bound
    // entirely (and losing the promptness coverage with it).
    //
    // The test runs on tokio's PAUSED virtual clock instead: the clock only
    // ever advances via an explicit timer firing. `parked_child_admission`
    // is plain data with no lock or `Notify` of its own (holding it does not
    // block anything), so a correct fail-closed rename never needs a timer
    // at all — it is pure computation over already-resident state. Any
    // virtual-clock movement here can only mean the implementation fell back
    // to some real wait (joining the parked publication via a poll/backoff
    // loop, a `Notify`/`timeout` race, etc.), which this assertion catches
    // exactly and deterministically, with zero wall-clock dependence.
    let start = tokio::time::Instant::now();
    let edit = tokio::time::timeout(
        std::time::Duration::from_secs(10),
        super::super::nav_features_navigation::handle_rename(
            server,
            RenameParams {
                text_document_position: TextDocumentPositionParams {
                    text_document: TextDocumentIdentifier {
                        uri: app_uri.clone(),
                    },
                    position,
                },
                new_name: "renamedHandler".to_string(),
                work_done_progress_params: Default::default(),
            },
        ),
    )
    .await
    .expect("rename must not join the parked background publication — it hung")
    .expect("incomplete rename must fail closed without a protocol error");
    assert_eq!(
        tokio::time::Instant::now(),
        start,
        "a fail-closed rename decision must not consume any virtual clock \
         time — any movement here means it fell back to waiting on the \
         parked background publication instead of deciding from resident \
         state"
    );
    assert!(
        edit.is_none(),
        "a same-file provider subset must never escape"
    );

    drop(parked_child_admission);
    drain_handle.abort();
    drop(service);
}

/// Completion never awaits imported-carrier sync: with the child companion sync
/// blocked, completion answers immediately (capture-only readiness) and the
/// import set is delivered by BACKGROUND publication after release.
#[tokio::test]
async fn completion_returns_fast_without_awaiting_import_carrier_sync() {
    let (_temp, service, drain_handle, provider, workspace_id) =
        make_definition_test_server_with_kind(
            &[
                ("src/MyComp.vue", "vue", READINESS_CHILD_SOURCE),
                ("src/App.vue", "vue", READINESS_PARENT_SOURCE),
            ],
            crate::TypeProviderKind::Tsgo,
        )
        .await;

    let server = service.inner();
    let app_uri = workspace_uri(&workspace_id, "src/App.vue");
    let child_id = format!("{workspace_id}/src/MyComp.vue");
    let child_ide_path =
        verter_session_query::resolution::carrier_ide_provider_path(&child_id, false);

    // SurfaceReady for the current file (lifecycle path), so completion's
    // provider context capture succeeds and only the dependency leg differs.
    server.ensure_current_file_synced(&app_uri).await;

    let (_arrived, release) = provider.block_open_file(&child_ide_path);

    let position = find_document_position(server, &app_uri, "handleCustom(payload", 1);
    // A capture-only completion never touches the blocked import-carrier
    // sync at all, so it must resolve on its VERY FIRST poll — a fully
    // structural proof with zero timing dependence, not a race between two
    // independently-scheduled event timestamps (which still leaves a real,
    // if narrow, preemption window between an event firing and the instant
    // that records it). `now_or_never` polls the completion future exactly
    // once with a no-op waker: if the poll chain never reaches a genuine
    // suspension point, it returns `Some` immediately; if completion
    // incorrectly awaits the blocked `release` (never fired at this point),
    // that first poll returns `Poll::Pending`, which `now_or_never` reports
    // as `None`. This cannot pass by favorable scheduling — it is a fact
    // about the future's poll chain, checked exactly once, synchronously.
    let completion = futures_util::FutureExt::now_or_never(
        server.completion(completion_params(&app_uri, position, None)),
    )
    .expect(
        "completion did not resolve on its first poll — it is awaiting the \
         blocked import-carrier sync instead of answering capture-only",
    );
    assert!(
        completion.is_ok(),
        "completion must answer with the child companion sync blocked; got {completion:?}"
    );

    // Now that completion has been proven not to depend on it, release the
    // blocked child open so background publication can proceed.
    release.notify_one();

    // The dependency delivery still happens — in the background: the child
    // companion must eventually be pushed without any further request.
    tokio::time::timeout(std::time::Duration::from_secs(5), async {
        provider
            .wait_until_calls(|calls| {
                calls.iter().any(
                    |c| matches!(c, MockCall::OpenFile { path, .. } if path == &child_ide_path),
                )
            })
            .await;
    })
    .await
    .expect("the imported child companion must be delivered by BACKGROUND publication");

    drain_handle.abort();
    drop(service);
}

/// A publication that lands INCOMPLETE (no committed provider surface at pull
/// time, a refused commit) leaves the open document owed a current receipt.
/// The drain that later settles that carrier re-arms the publication. Before,
/// an incomplete publication left no receipt at all, the drain's generation
/// bump found nothing to outdate, and the document stayed uncertified until
/// the next editor signal — which an editor waiting on the receipt never sends.
#[tokio::test(flavor = "multi_thread")]
async fn an_incomplete_publication_is_re_armed_when_the_drain_settles_the_carrier() {
    let fixture = watched_dependency_fixture(true).await;
    let server = fixture.service.inner();
    let healthy = format!("{}/src/Consumer.vue", fixture.root);
    let healthy_uri = workspace_uri(&fixture.root, "src/Consumer.vue");
    server
        .sync_coordinator
        .await_until(
            || server.documents.diagnostics_ready(&healthy_uri),
            || panic!("control: the open document settles into a complete receipt"),
        )
        .await;

    // The provider batch could not be merged: the publication is incomplete.
    let publication = server
        .documents
        .begin_diagnostics_publication(&healthy_uri)
        .expect("an open document admits a publication");
    server
        .documents
        .publish_diagnostics(&healthy_uri, &publication, Vec::new(), false, None)
        .await;
    assert!(
        !server.documents.diagnostics_ready(&healthy_uri),
        "precondition: an incomplete publication is not a current receipt"
    );

    let mut refreshes = server.documents.subscribe_diagnostics_refresh();
    server.queue_snapshot_provider_sync(healthy.clone());
    drain_pending_provider_sync_for(server).await;
    let refresh = refreshes
        .try_recv()
        .expect("the drain re-arms the owed publication when it settles the carrier");
    assert_eq!(refresh.uri, healthy_uri);
    server
        .sync_coordinator
        .await_until(
            || {
                server.documents.diagnostics_ready(&healthy_uri)
                    && server.sync_coordinator.diag_tasks_live() == 0
            },
            || panic!("the re-armed publication completes into a current receipt"),
        )
        .await;
}
