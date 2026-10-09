use super::*;

/// Server capabilities must NOT include `diagnostic_provider` (pull diagnostics).
/// We use push diagnostics exclusively to avoid flickering during typing.
#[test]
fn capabilities_do_not_include_pull_diagnostics() {
    let caps = host_server_capabilities(true);
    assert!(
        caps.diagnostic_provider.is_none(),
        "diagnostic_provider must be removed — we use push diagnostics only"
    );
}

/// The advertised semantic-token legend IS the shared mapping owner's
/// published vocabulary, name-for-name and index-for-index. Every provider
/// lane remaps its token space into `verter_type_runtime::semantic_tokens`'
/// published indices, so a wire legend that drifts from those arrays re-opens
/// the exact index-space mismatch this pin exists to prevent.
#[test]
fn advertised_semantic_token_legend_is_the_shared_owners_published_vocabulary() {
    use tower_lsp_server::ls_types::SemanticTokensServerCapabilities;

    let caps = host_server_capabilities(true);
    let Some(SemanticTokensServerCapabilities::SemanticTokensOptions(options)) =
        caps.semantic_tokens_provider.as_ref()
    else {
        panic!("semantic tokens must be advertised as SemanticTokensOptions");
    };

    let advertised_types: Vec<&str> = options
        .legend
        .token_types
        .iter()
        .map(|t| t.as_str())
        .collect();
    assert_eq!(
        advertised_types,
        verter_type_runtime::semantic_tokens::VERTER_TOKEN_TYPES.to_vec(),
        "legend token types must equal the shared owner's published array, in order"
    );

    let advertised_modifiers: Vec<&str> = options
        .legend
        .token_modifiers
        .iter()
        .map(|m| m.as_str())
        .collect();
    assert_eq!(
        advertised_modifiers,
        verter_type_runtime::semantic_tokens::VERTER_TOKEN_MODIFIERS.to_vec(),
        "legend token modifiers must equal the shared owner's published array, in order \
         (including the TypeScript-family `local` bit — without it every \
         function-scoped binding's token fails closed and disappears)"
    );
}

#[test]
fn did_open_startup_policy_publishes_child_contracts_without_a_provider() {
    let none = did_open_startup_policy(crate::TypeProviderKind::None);
    assert!(
        none.sync_imported_carrier_apis,
        "provider-neutral child contracts must be published even without an external type provider"
    );
    assert!(
        !none.publish_diagnostics,
        "should not publish diagnostics inline"
    );
}

/// The editor-owned tsserver route has no local provider buffers: its only
/// content authority is the durable carrier store consumed by the editor
/// plugin. A live source edit must therefore take the same membership/publish
/// branch as managed tsserver, not the direct-open branch used by tsgo.
#[tokio::test(flavor = "multi_thread")]
async fn editor_tsserver_live_publish_refreshes_durable_carrier_content() {
    let workspace_root = unique_server_ws_root("editor_live");
    let tsconfig = format!("{workspace_root}/tsconfig.json");
    let canonical_id = format!("{workspace_root}/src/App.vue");

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
    install_test_resolver_for_root(server, &workspace_root, Some(&tsconfig));

    let initial = "<script setup lang=\"ts\">\nconst resolved = 1\n</script>\n<template><div>{{ resolved }}</div></template>\n";
    let uri = open_test_vue(server, &canonical_id, initial);
    assert!(server.publish_carrier_to_external_ts(&canonical_id).await);

    let initial_manifest = carrier_manifest_strict(&workspace_root)
        .expect("the editor-owned publish must have written a manifest");
    let initial_ide = initial_manifest
        .projects
        .get(&tsconfig)
        .and_then(|project| {
            project.ready_files.iter().find_map(|(provider, ready)| {
                (provider.ends_with(".vue.tsx")).then_some(ready.clone())
            })
        })
        .expect("editor-owned initial publish must materialize the IDE carrier");

    let updated = "<script setup lang=\"ts\">\nconst resolved = 1\nunresolvedAfterEdit\n</script>\n<template><div>{{ resolved }}</div></template>\n";
    let update = server.documents.did_change(&uri, 2, updated);
    assert!(
        update.changed,
        "the edit must invalidate the compiled carrier"
    );
    assert!(server.publish_carrier_to_external_ts(&canonical_id).await);

    let updated_manifest = carrier_manifest_strict(&workspace_root)
        .expect("the post-edit republish must have written a manifest");
    let updated_ide = updated_manifest
        .projects
        .get(&tsconfig)
        .and_then(|project| {
            project.ready_files.iter().find_map(|(provider, ready)| {
                (provider.ends_with(".vue.tsx")).then_some(ready.clone())
            })
        })
        .expect("editor-owned live publish must retain the IDE carrier");
    assert_ne!(
        updated_ide.content_hash, initial_ide.content_hash,
        "a live edit must replace the durable carrier bytes read by the editor plugin"
    );
}

/// Discriminates the server-side interactive publish path
/// (`publish_carrier_to_external_ts` → `reconcile_carrier_via_gateway`'s
/// tsserver `Published` branch) against the SAME compile-to-identity race the
/// sync-coordinator tests close: a `did_change` landing between this
/// function's own compile and its call into the carrier-sync gateway must
/// never let the gateway pair stale IDE bytes with the edited source.
///
/// Before this fix, `reconcile_carrier_via_gateway` self-captured its pin AT
/// GATEWAY ENTRY — AFTER this function's own compile already ran. An edit
/// landing in the pause window below would make that self-captured pin
/// observe revision B while `ide.code` was compiled from revision A: the pin
/// then MATCHES the still-B live identity at record time and the record
/// proceeds, pairing stale A bytes with B's source — a torn pair. This test's
/// pause point sits exactly where that self-capture used to happen
/// (immediately after the compile, before the gateway call), so it
/// reproduces the defect if the pin capture is moved back there instead of
/// being threaded in from the caller before the compile.
#[tokio::test(flavor = "multi_thread")]
async fn publish_carrier_pin_is_captured_before_the_compile_not_after() {
    let workspace_root = unique_server_ws_root("publish_pin_race");
    let tsconfig = format!("{workspace_root}/tsconfig.json");
    let canonical_id = format!("{workspace_root}/src/App.vue");

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
    install_test_resolver_for_root(server, &workspace_root, Some(&tsconfig));

    const SOURCE_A: &str = "<script setup lang=\"ts\">\nconst msg = 'revision-a'\n</script>\n\
                             <template><div>{{ msg }}</div></template>\n";
    const SOURCE_B: &str =
        "<script setup lang=\"ts\">\nconst msg = 'revision-b-edited'\n</script>\n\
                             <template><div>{{ msg }}</div></template>\n";
    let uri = open_test_vue(server, &canonical_id, SOURCE_A);

    // Pause right after `publish_carrier_to_external_ts`'s own compile, the
    // pre-fix self-capture spot.
    let (arrived, release) = server.pause_next_publish_carrier_after_compile(&canonical_id);

    let publish = server.publish_carrier_to_external_ts(&canonical_id);
    let edit = async {
        arrived.notified().await;
        let result = server.documents.did_change(&uri, 2, SOURCE_B);
        assert!(
            result.changed,
            "the interleaved edit must really commit revision B"
        );
        release.notify_one();
    };
    let (published, ()) = futures_util::future::join(publish, edit).await;
    assert!(published, "the publish pass must still complete");

    assert_eq!(
        server
            .documents
            .get(&uri)
            .expect("document stays open")
            .source
            .as_ref(),
        SOURCE_B,
        "precondition: the live document is revision B"
    );

    // The pin was captured before the compile — before this pause, before the
    // edit — so it stays anchored to revision A while the live identity moves
    // to B. The fenced record inside the gateway's `Published` branch must
    // refuse outright rather than pair mismatched content.
    let ide_path =
        verter_session_query::resolution::carrier_ide_provider_path(&canonical_id, false);
    assert!(
        server
            .documents
            .provider_surfaces()
            .current_snapshot(&ide_path)
            .is_none(),
        "a pin captured before the compile must make the publish gateway's \
         record refuse when an edit lands after that capture — a recorded \
         surface here means the pin was captured too late (or not honored), \
         reproducing the pre-fix torn-pairing defect"
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn edited_carrier_republishes_its_own_contract_after_an_isolated_edit() {
    // A document with no import roots keeps its delivered import receipt
    // across an edit that cannot change what it imports. The same background
    // pass ALSO owns this carrier's own public component contract, and a
    // content edit DOES invalidate that contract (its snapshot is bound to the
    // source revision). The pass's early-return freshness gate must therefore
    // cover the self-contract leg as well as the import legs: gating solely on
    // the promoted import receipt leaves the contract permanently cold for the
    // whole generation, because every later enqueue returns early and the only
    // producer never runs. A parent typing `<DraftCard ` then never sees the
    // child's props.
    const CHILD_V1: &str =
        "<script setup lang=\"ts\">\ninterface DraftProps { title: string }\ndefineProps<DraftProps>()\n</script>\n";
    const CHILD_V2: &str = "<script setup lang=\"ts\">\ninterface DraftProps { title: string; unusedOnly?: boolean }\ndefineProps<DraftProps>()\n</script>\n";

    let (_temp, service, drain_handle, _provider, workspace_id) =
        make_definition_test_server_with_config(
            &[("src/DraftCard.vue", "vue", CHILD_V1)],
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
    let child_uri = workspace_uri(&workspace_id, "src/DraftCard.vue");
    let child_id = format!("{workspace_id}/src/DraftCard.vue");

    server.publish_import_dependencies_settled(&child_uri).await;
    assert!(
        server.cached_child_public_contract(&child_id).is_some(),
        "the first background pass must commit the carrier's own contract"
    );
    assert!(
        server.dependency_readiness_capture(&child_uri).is_ready(),
        "a rootless carrier must hold a delivered import receipt"
    );

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
                text: CHILD_V2.to_string(),
            }],
        },
    )
    .await;

    assert!(
        server.cached_child_public_contract(&child_id).is_none(),
        "the edit must retire the contract published against the previous revision"
    );

    // The same production pass the background lane runs. It must not treat the
    // preserved import receipt as proof that the self-contract leg is
    // delivered.
    server.publish_import_dependencies_settled(&child_uri).await;
    assert!(
        server.cached_child_public_contract(&child_id).is_some(),
        "the publication pass must republish the edited carrier's own contract"
    );

    drain_handle.abort();
    drop(service);
}

#[tokio::test(flavor = "multi_thread")]
async fn imported_child_contract_cache_is_provenance_fenced_and_republished() {
    const CHILD_V1: &str = "<script lang=\"ts\">\ninterface Props { title: string; unusedOnly?: boolean }\nlet { title }: Props = $props();\n</script>\n<p>{title}</p>\n";
    const CHILD_V2: &str = "<script lang=\"ts\">\ninterface Props { title: string; unusedOnly?: boolean; second?: string }\nlet { title }: Props = $props();\n</script>\n<p>{title}</p>\n";
    const CHILD_V3: &str = "<script lang=\"ts\">\ninterface Props { title: string; unusedOnly?: boolean; third?: boolean }\nlet { title }: Props = $props();\n</script>\n<p>{title}</p>\n";
    const PARENT: &str = "<script lang=\"ts\">\nimport DraftCard from './DraftCard.svelte';\n</script>\n<DraftCard />\n";

    let (_temp, service, drain_handle, provider, workspace_id) =
        make_definition_test_server_with_config(
            &[
                ("src/DraftCard.svelte", "svelte", CHILD_V1),
                ("src/App.svelte", "svelte", PARENT),
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
    let app_uri = workspace_uri(&workspace_id, "src/App.svelte");
    let child_uri = workspace_uri(&workspace_id, "src/DraftCard.svelte");
    let child_id = format!("{workspace_id}/src/DraftCard.svelte");

    server.publish_import_dependencies_settled(&app_uri).await;
    assert!(server.cached_child_public_contract(&child_id).is_some());

    // Provider delivery and contract publication are independent. A missing
    // contract under byte-current provider state runs only the projector.
    let provider_state = server
        .provider_sync_state_for_source(&child_id)
        .expect("initial provider delivery");
    let provider_call_count = provider.file_sync_calls().len();
    let projection_count = server.child_public_contract_projection_count_for_test();
    server.evict_child_public_contract_for_test(&child_id);
    assert!(server
        .sync_imported_carrier_api_lightweight(&child_id)
        .await
        .is_complete());
    assert!(server.cached_child_public_contract(&child_id).is_some());
    assert_eq!(provider.file_sync_calls().len(), provider_call_count);
    assert_eq!(
        server.provider_sync_state_for_source(&child_id),
        Some(provider_state.clone()),
        "contract-only repair must not advance provider state"
    );
    assert_eq!(
        server.child_public_contract_projection_count_for_test(),
        projection_count + 1
    );

    // An unrelated workspace-content mutation must not invalidate a child
    // contract. The child revision, publication witness, resolver snapshot,
    // and project generation are unchanged, so neither provider delivery nor
    // contract projection should repeat.
    let projection_count = server.child_public_contract_projection_count_for_test();
    server.documents.host().notify_upsert(
        &format!("{workspace_id}/src/unrelated.ts"),
        Arc::<str>::from("export const unrelated = 1;"),
    );
    assert!(server.cached_child_public_contract(&child_id).is_some());
    let provider_call_count = provider.file_sync_calls().len();
    assert!(server
        .sync_imported_carrier_api_lightweight(&child_id)
        .await
        .is_complete());
    assert!(server.cached_child_public_contract(&child_id).is_some());
    assert_eq!(provider.file_sync_calls().len(), provider_call_count);
    assert_eq!(
        server.child_public_contract_projection_count_for_test(),
        projection_count
    );

    // Replacing the published resolver/config world evicts the prior key. The
    // same background wrapper must establish a contract in the new world.
    install_test_resolver_for_root(
        server,
        &workspace_id,
        Some(&format!("{workspace_id}/tsconfig.json")),
    );
    assert!(server.cached_child_public_contract(&child_id).is_none());
    assert!(server
        .sync_imported_carrier_api_lightweight(&child_id)
        .await
        .is_complete());
    assert!(server.cached_child_public_contract(&child_id).is_some());

    // A source mutation inside composition must refuse the stale projection.
    // The next settled pass publishes the new revision, and removal makes the
    // pure cache capture fail closed again.
    assert!(server.documents.did_change(&child_uri, 2, CHILD_V2).changed);
    assert!(server.cached_child_public_contract(&child_id).is_none());
    server.ensure_current_file_synced(&child_uri).await;
    tokio::task::block_in_place(|| {
        server
            .documents
            .host()
            .get_public_api_projection(&child_id)
            .expect("revision-two background projection")
            .expect("revision-two component projection")
    });
    let _ = server
        .sync_imported_carrier_api_lightweight(&child_id)
        .await;
    assert!(
        server.cached_child_public_contract(&child_id).is_some(),
        "revision-two background sync must publish its contract"
    );
    assert!(
        server.imported_carrier_already_delivered(&child_id),
        "revision-two provider publication must be current before isolating the contract fence: \
         state={:?}, live={:?}",
        server.provider_sync_state_for_source(&child_id),
        server.documents.host().get_source(&child_id)
    );
    server.evict_child_public_contract_for_test(&child_id);
    let documents = Arc::clone(server.test_documents());
    let hook_uri = child_uri.clone();
    server.set_child_contract_after_projection_hook_for_test(Box::new(move || {
        assert!(documents.did_change(&hook_uri, 3, CHILD_V3).changed);
    }));
    assert!(
        !server
            .sync_imported_carrier_api_lightweight(&child_id)
            .await
            .is_complete(),
        "a revision change during composition must refuse publication"
    );
    assert!(server.cached_child_public_contract(&child_id).is_none());
    server.ensure_current_file_synced(&child_uri).await;
    tokio::task::block_in_place(|| {
        server
            .documents
            .host()
            .get_public_api_projection(&child_id)
            .expect("revision-three background projection")
            .expect("revision-three component projection")
    });
    assert!(server
        .sync_imported_carrier_api_lightweight(&child_id)
        .await
        .is_complete());
    let current = server
        .cached_child_public_contract(&child_id)
        .expect("settled post-edit publication");
    let verter_session::framework::ComponentContractAvailability::Supported(contract) = current
    else {
        panic!("edited fixture contract must remain supported")
    };
    assert!(contract
        .props
        .iter()
        .any(|prop| prop.name.as_ref() == "third"));

    server.documents.did_close(&child_uri);
    assert!(server.documents.host().remove(&child_id).is_some());
    assert!(
        server.cached_child_public_contract(&child_id).is_none(),
        "a removed child cannot retain a readable contract"
    );

    drain_handle.abort();
    drop(service);
}

#[tokio::test(flavor = "multi_thread")]
async fn background_init_drain_clears_stale_macro_type_diagnostic_for_package_exports_dep() {
    let temp = tempfile::tempdir().expect("temp dir");
    let workspace = temp.path().join("workspace");
    std::fs::create_dir_all(workspace.join("src")).expect("create src dir");
    std::fs::create_dir_all(workspace.join("node_modules/motion/dist"))
        .expect("create motion dist dir");
    std::fs::write(workspace.join("tsconfig.json"), "{}").expect("write tsconfig");
    std::fs::write(
        workspace.join("node_modules/motion/package.json"),
        r#"{
                "name": "motion",
                "exports": {
                    ".": {
                        "types": "./dist/index.d.ts"
                    }
                }
            }"#,
    )
    .expect("write motion package");
    std::fs::write(
        workspace.join("node_modules/motion/dist/index.d.ts"),
        "export interface MotionProps { duration: number }\n",
    )
    .expect("write motion types");

    let popup_source = "<script setup lang=\"ts\">\nimport type { MotionProps } from 'motion'\nconst props = defineProps<MotionProps>()\n</script>\n<template><div>{{ props.duration }}</div></template>";
    std::fs::write(workspace.join("src/Popup.vue"), popup_source).expect("write Popup.vue");

    let workspace_id = crate::test_utils::canonical_test_path(&workspace);
    let popup_id = format!("{workspace_id}/src/Popup.vue");
    let uri = crate::uri::path_to_file_uri(&popup_id).expect("file uri");

    let host = crate::test_utils::make_filesystem_test_host(&workspace);
    host.configure_projects(vec![verter_workspace::ide_project_config(
        workspace_id.clone(),
        workspace_id.clone(),
        Some(format!("{workspace_id}/tsconfig.json")),
    )]);

    let documents = DocumentRegistry::new(Arc::clone(&host));
    let _ = documents.did_open(&TextDocumentItem {
        uri: uri.clone(),
        language_id: "vue".to_string(),
        version: 1,
        text: popup_source.to_string(),
    });

    let cached_verter_diags = DashMap::new();

    // With TypeImport resolution and a filesystem-backed host, the macro type dep
    // resolves immediately via the "types" export condition in package.json.
    // No stale HOST_MISSING_MACRO_TYPE_DEP diagnostic should appear.
    let diags =
        crate::server::document_diagnostics_for_test(&documents, &uri, &cached_verter_diags, None);
    assert!(
        !diags.iter().any(|d| matches!(
            &d.code,
            Some(NumberOrString::String(code)) if code == "HOST_MISSING_MACRO_TYPE_DEP"
        )),
        "macro type dep 'motion' with types-only exports should resolve via TypeImport, got: {diags:?}"
    );
    let cache = cached_verter_diags
        .get(uri.as_str())
        .expect("diagnostics should be cached");
    assert_eq!(
        cache.0, 1,
        "cached doc version should match did_open version"
    );
}

#[test]
fn compute_verter_diagnostics_flags_fixture_fragment_component_data_attr() {
    let workspace_id = crate::test_harness::provider_fixture_workspace_root("single-project");
    let app_path = format!("{workspace_id}/src/App.vue");
    let app_source = std::fs::read_to_string(&app_path).expect("fixture App.vue should exist");
    let uri = crate::uri::path_to_file_uri(&app_path).expect("fixture uri should be valid");

    // Root the host at the SAME staged copy `workspace_id`, `app_path` and the
    // document URI come from. Rooting it at the authored tree instead would make
    // the host resolve a different fixture than the one under test.
    let host = crate::test_utils::make_filesystem_test_host(std::path::Path::new(&workspace_id));
    let documents = Arc::new(DocumentRegistry::new(Arc::clone(&host)));
    let _ = documents.did_open(&TextDocumentItem {
        uri: uri.clone(),
        language_id: "vue".to_string(),
        version: 1,
        text: app_source,
    });

    let cached_verter_diags = Arc::new(DashMap::new());

    let diags =
        crate::server::document_diagnostics_for_test(&documents, &uri, &cached_verter_diags, None);
    let fragment_path = format!("{workspace_id}/src/FragmentComp.vue");
    let fragment_analysis = resolve_component_for(host.as_ref(), &app_path, "./FragmentComp.vue")
        .map(|(_, analysis)| analysis);

    assert!(
            diags.iter().any(|diag| {
                matches!(
                    diag.code.as_ref(),
                    Some(NumberOrString::String(code)) if code == "verter/unknown-prop"
                ) && diag.message.contains("data-test")
            }),
            "fixture fragment component should flag data-test, got: {diags:?}, child_loaded={}, child_template_roots={:?}, child_macros={:?}, child_components={:?}",
            host.get_analysis(&fragment_path).is_some(),
            fragment_analysis.as_ref().and_then(|analysis| {
                analysis.template.as_ref().map(|template| {
                    template
                        .elements
                        .iter()
                        .filter(|element| element.parent_index.is_none())
                        .map(|element| element.tag.clone())
                        .collect::<Vec<_>>()
                })
            }),
            fragment_analysis
                .as_ref()
                .map(|analysis| analysis.macros.iter().map(|mac| mac.kind).collect::<Vec<_>>()),
            fragment_analysis.as_ref().map(|analysis| {
                analysis
                    .template
                    .as_ref()
                    .map(|template| template.components.iter().map(|comp| comp.name.clone()).collect::<Vec<_>>())
            })
        );
}

/// Fail-open boundary: whole-object escapes, destructured `defineProps`
/// (provider-owned TS6133 — the merge cannot dedup across sources), and
/// `useSlots()` each silence their WHOLE kind — zero unused-declaration
/// diagnostics for this component.
#[test]
fn unused_declaration_diagnostics_fail_open_on_escapes_destructure_and_use_slots() {
    let dir = tempfile::tempdir().unwrap();
    let source = "<script setup lang=\"ts\">\n\
                  import { useSlots } from 'vue';\n\
                  const { neverReadProp } = defineProps<{ neverReadProp: number }>();\n\
                  const emit = defineEmits<{ neverEmitted: [] }>();\n\
                  const forwarded = emit;\n\
                  defineSlots<{ neverOutlet(): unknown }>();\n\
                  const slots = useSlots();\n\
                  console.log(forwarded, slots);\n\
                  </script>\n\
                  \n\
                  <template>\n\
                  <div />\n\
                  </template>\n";
    let file = dir.path().join("FailOpen.vue");
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
            Some(NumberOrString::String(code))
                if code == "verter/no-unused-props"
                    || code == "verter/no-unused-emit-declarations"
                    || code == "verter/no-unused-slots"
        )),
        "escaped/destructured/useSlots component must produce ZERO unused-declaration \
         diagnostics (fail-open; destructured props are provider-owned TS6133), got: {diags:?}"
    );
}

/// A legacy Svelte `<slot>` has NO declaration site — the unused-declaration
/// diagnostics apply only to explicit type-level declarations and must never
/// invent one for Svelte markup.
#[test]
fn svelte_legacy_slot_produces_no_unused_declaration_diagnostic() {
    let dir = tempfile::tempdir().unwrap();
    let source = "<script lang=\"ts\">\n\
                  export let title: string;\n\
                  console.log(title);\n\
                  </script>\n\
                  \n\
                  <div><slot /></div>\n";
    let file = dir.path().join("LegacySlot.svelte");
    std::fs::write(&file, source).unwrap();

    let host = crate::test_utils::make_filesystem_test_host(dir.path());
    let documents = Arc::new(DocumentRegistry::new(Arc::clone(&host)));
    let uri =
        crate::uri::path_to_file_uri(&file.to_string_lossy().replace('\\', "/")).expect("uri");
    let _ = documents.did_open(&TextDocumentItem {
        uri: uri.clone(),
        language_id: "svelte".to_string(),
        version: 1,
        text: source.to_string(),
    });

    let cached_verter_diags = Arc::new(DashMap::new());
    let diags =
        crate::server::document_diagnostics_for_test(&documents, &uri, &cached_verter_diags, None);

    assert!(
        !diags.iter().any(|diag| matches!(
            diag.code.as_ref(),
            Some(NumberOrString::String(code)) if code.starts_with("verter/no-unused-")
        )),
        "legacy <slot> has no declaration site — nothing to flag, got: {diags:?}"
    );
}

/// Svelte's `$props()` rune shares semantic macro/member facts with Vue's
/// `defineProps`, but that reuse must never enable Vue-only unused-declaration
/// diagnostics on the authored Svelte carrier. Provider-owned TS6133 remains
/// independent and is covered by the real-provider diagnostic contract.
#[test]
fn svelte_props_rune_produces_no_vue_unused_declaration_diagnostic() {
    let dir = tempfile::tempdir().unwrap();
    let source = "<script lang=\"ts\">\n\
                  import type { Snippet } from 'svelte';\n\
                  interface Props { header?: Snippet; body?: Snippet }\n\
                  let { header, body }: Props = $props();\n\
                  </script>\n\
                  \n\
                  {@render body?.()}\n";
    let file = dir.path().join("UnusedSnippetProp.svelte");
    std::fs::write(&file, source).unwrap();

    let host = crate::test_utils::make_filesystem_test_host(dir.path());
    let documents = Arc::new(DocumentRegistry::new(Arc::clone(&host)));
    let uri =
        crate::uri::path_to_file_uri(&file.to_string_lossy().replace('\\', "/")).expect("uri");
    let _ = documents.did_open(&TextDocumentItem {
        uri: uri.clone(),
        language_id: "svelte".to_string(),
        version: 1,
        text: source.to_string(),
    });

    let cached_verter_diags = Arc::new(DashMap::new());
    let diags =
        crate::server::document_diagnostics_for_test(&documents, &uri, &cached_verter_diags, None);

    assert!(
        !diags.iter().any(|diag| matches!(
            diag.code.as_ref(),
            Some(NumberOrString::String(code)) if code.starts_with("verter/no-unused-")
        )),
        "Svelte rune facts must not enable Vue-only unused diagnostics, got: {diags:?}"
    );
}

/// The `$emit` template form of the same pattern, end-to-end from real source
/// (the population layer's `$emit` suppression is otherwise only exercised
/// with hand-built occurrences).
#[test]
fn template_dollar_emit_call_suppresses_emit_diagnostics() {
    let dir = tempfile::tempdir().unwrap();
    let source = "<script setup lang=\"ts\">\n\
                  defineEmits<{ close: [] }>();\n\
                  </script>\n\
                  \n\
                  <template>\n\
                  <button @click=\"$emit('close')\">x</button>\n\
                  </template>\n";
    let file = dir.path().join("DollarEmit.vue");
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
            Some(NumberOrString::String(code)) if code == "verter/no-unused-emit-declarations"
        )),
        "a template `$emit('close')` must suppress unused-emit diagnostics, got: {diags:?}"
    );
}

/// ISSUE-8 (TS6133 quick-fix threading): the code-action handler must parse the
/// editor's `context.diagnostics` codes and forward them to the TypeProvider so a
/// "Remove unused declaration" fix can be requested. Pressing CTRL+. on an unused
/// `<script setup>` `const foo` sends the published TS6133 (`code:
/// String("6133")`); the handler must thread the integer `6133` to
/// `get_code_actions` AND map the provider's deletion edit back to the `.vue`
/// source.
///
/// Discriminating: before this change the trait had no diagnostics channel and
/// the handler forwarded none, so `MockCall::GetCodeActions.diagnostics` would be
/// empty (the code never reaches the provider) and the keyed response would not
/// fire.
#[tokio::test]
async fn code_action_threads_diagnostic_code_to_type_provider_and_maps_edit_back() {
    let provider = Arc::new(MockTypeProvider::new());
    let type_provider: Arc<dyn TypeProvider> = provider.clone();
    let service = make_hover_test_service(type_provider);
    let server = service.inner();
    install_test_resolver(server);

    // An unused top-level binding: `const foo = 1` is referenced nowhere, so
    // Block F surfaces TS6133 at this decl span and CTRL+. lands here.
    let source = "<script setup lang=\"ts\">\nconst foo = 1\n</script>\n<template></template>\n";
    let uri = open_test_vue(server, "/workspace/src/Unused.vue", source);
    // Establish the carrier IDE projection so the handler's
    // `type_provider_context` resolves (the direct-handler call skips the
    // implicit sync that `server.completion(...)` would perform).
    server.test_ensure_synced(&uri).await;

    // The carrier range of the unused binding identifier `foo` (a token that
    // round-trips through the carrier↔TSX mapper). This stands in for the TS6133
    // diagnostic span the editor sends on CTRL+. and the cursor selection.
    let decl_start = find_document_position(server, &uri, "foo = 1", 0);
    let decl_end = find_document_position(server, &uri, "foo = 1", "foo".len());

    // Map that carrier range to TSX offsets so we can arm the keyed mock response
    // exactly where the handler will query.
    let ctx = synced_type_provider_context(server, &uri).await;
    let tsx_start = merge::carrier_position_to_tsx_offset_validated(
        &decl_start,
        &ctx.carrier_line_index,
        &ctx.mapper,
        &ctx.tsx_line_index,
    )
    .expect("decl start maps to tsx");
    let tsx_end = merge::carrier_position_to_tsx_offset_validated(
        &decl_end,
        &ctx.carrier_line_index,
        &ctx.mapper,
        &ctx.tsx_line_index,
    )
    .expect("decl end maps to tsx");

    // The provider returns a "Remove unused declaration" action whose edit DELETES
    // the `const foo = 1` TSX span (new_text empty). The carrier-IDE path maps it
    // back to the `.vue` source range.
    provider.set_code_actions(
        &ctx.tsx_path,
        tsx_start,
        tsx_end,
        vec![TypeCodeAction {
            title: "Remove unused declaration".to_string(),
            kind: Some("quickfix".to_string()),
            edits: vec![crate::type_provider::protocol::TypeCodeEdit {
                path: ctx.tsx_path.clone(),
                start: tsx_start,
                end: tsx_end,
                new_text: String::new(),
            }],
        }],
    );

    // Drive the handler with the published TS6133 in context.diagnostics — exactly
    // what the editor sends on CTRL+. over the faded decl.
    let actions = super::super::aux_features::handle_code_action(
        server,
        CodeActionParams {
            text_document: TextDocumentIdentifier { uri: uri.clone() },
            range: Range {
                start: decl_start,
                end: decl_end,
            },
            context: CodeActionContext {
                diagnostics: vec![Diagnostic {
                    range: Range {
                        start: decl_start,
                        end: decl_end,
                    },
                    code: Some(NumberOrString::String("6133".to_string())),
                    source: Some("ts".to_string()),
                    message: "'foo' is declared but its value is never read.".to_string(),
                    ..Default::default()
                }],
                only: None,
                trigger_kind: None,
            },
            work_done_progress_params: Default::default(),
            partial_result_params: Default::default(),
        },
    )
    .await
    .expect("code_action returns Ok");

    // 1. The integer code 6133 reached the provider via the threaded context.
    let calls = provider.calls();
    let threaded = calls.iter().find_map(|c| match c {
        MockCall::GetCodeActions { diagnostics, .. } => Some(diagnostics.clone()),
        _ => None,
    });
    let threaded = threaded.expect("get_code_actions must have been called");
    assert!(
        threaded.iter().any(|d| d.code == 6133),
        "the TS6133 code must be threaded to the provider, got {threaded:?}"
    );

    // 2. The provider's deletion action mapped back to the `.vue` source: a
    //    workspace edit over the carrier with empty new_text covering `const foo`.
    let actions = actions.expect("an action must be returned");
    let remove = actions.iter().find_map(|a| match a {
        CodeActionOrCommand::CodeAction(ca) if ca.title == "Remove unused declaration" => Some(ca),
        _ => None,
    });
    let remove = remove.expect("the remove-unused action must survive map-back");
    let changes = remove
        .edit
        .as_ref()
        .and_then(|e| e.changes.as_ref())
        .expect("the remove action must carry workspace changes");
    let (edit_uri, edits) = changes.iter().next().expect("one changed file");
    assert_eq!(
        edit_uri, &uri,
        "the deletion must target the .vue carrier source, not the TSX"
    );
    assert_eq!(edits.len(), 1, "one deletion edit");
    assert!(
        edits[0].new_text.is_empty(),
        "a remove-unused fix deletes (empty new_text), got {:?}",
        edits[0].new_text
    );
    // The mapped-back range must cover the `const foo` decl in the .vue source
    // (line 1, the `<script setup>` body), never a line-0 mis-map.
    assert_eq!(
        edits[0].range.start, decl_start,
        "deletion start must map to the .vue decl, got {:?}",
        edits[0].range.start
    );
    assert_ne!(
        edits[0].range,
        Range::default(),
        "the deletion must never collapse to (0,0)"
    );
}

/// A `source.removeUnused`-only request carrying a NON-unused numeric TS diagnostic
/// (here TS2304 "Cannot find name") must likewise not forward to the quickfix
/// provider path. The source-only kind is out of scope for this carrier path
/// regardless of the diagnostic code, so the gate must never fire for it — proving
/// the fix does not merely special-case the TS6133 code.
///
/// Discriminating: before the fix the gate's `source.removeUnused` arm forwarded
/// unconditionally (it never inspected the diagnostic code), so a `GetCodeActions`
/// call WAS recorded and the assertion FAILS; after the fix the gate does not fire
/// and the assertion PASSES.
#[tokio::test]
async fn code_action_source_only_request_does_not_forward_non_unused_diagnostic() {
    let provider = Arc::new(MockTypeProvider::new());
    let type_provider: Arc<dyn TypeProvider> = provider.clone();
    let service = make_hover_test_service(type_provider);
    let server = service.inner();
    install_test_resolver(server);

    // Reuse the proven-mappable `const foo = 1` decl span (the `foo` identifier
    // round-trips carrier↔TSX), so the ONLY reason the provider is not queried is
    // the gate refusing a source-only request — not an unmappable range. The
    // diagnostic carried here is a non-unused TS code (2304), not TS6133.
    let source = "<script setup lang=\"ts\">\nconst foo = 1\n</script>\n<template></template>\n";
    let uri = open_test_vue(server, "/workspace/src/SourceOnlyNonUnused.vue", source);
    server.test_ensure_synced(&uri).await;

    let diag_start = find_document_position(server, &uri, "foo = 1", 0);
    let diag_end = find_document_position(server, &uri, "foo = 1", "foo".len());
    let range = Range {
        start: diag_start,
        end: diag_end,
    };

    super::super::aux_features::handle_code_action(
        server,
        CodeActionParams {
            text_document: TextDocumentIdentifier { uri: uri.clone() },
            range,
            context: CodeActionContext {
                diagnostics: vec![Diagnostic {
                    range,
                    // A non-unused numeric TS code (TS2304), not TS6133. The handler
                    // keys only on `code`/`range`, so the message text is immaterial.
                    code: Some(NumberOrString::String("2304".to_string())),
                    source: Some("ts".to_string()),
                    message: "Cannot find name 'Foo'.".to_string(),
                    ..Default::default()
                }],
                only: Some(vec![CodeActionKind::new("source.removeUnused")]),
                trigger_kind: None,
            },
            work_done_progress_params: Default::default(),
            partial_result_params: Default::default(),
        },
    )
    .await
    .expect("code_action returns Ok");

    assert!(
        !provider
            .calls()
            .iter()
            .any(|c| matches!(c, MockCall::GetCodeActions { .. })),
        "an only=[source.removeUnused] request must NOT forward to the provider \
         quickfix path even for a non-unused diagnostic, got calls={:?}",
        provider.calls()
    );
}

#[test]
fn compute_verter_diagnostics_ignores_plain_typescript_files() {
    let host = Arc::new(VerterHost::new_standalone(
        verter_session::HostConfig::default(),
    ));
    let documents = Arc::new(DocumentRegistry::new(Arc::clone(&host)));

    let uri: Uri = "file:///workspace/src/__verter_mayberef_repro__.ts"
        .parse()
        .unwrap();
    let source = "type MaybeRef<T> = T\n\nexport function useLockScroll(target: MaybeRef<HTMLElement | null> = null) {\n  return target\n}\n";
    let _ = documents.did_open(&TextDocumentItem {
        uri: uri.clone(),
        language_id: "typescript".to_string(),
        version: 1,
        text: source.to_string(),
    });

    let cached_verter_diags = Arc::new(DashMap::new());

    let diags =
        crate::server::document_diagnostics_for_test(&documents, &uri, &cached_verter_diags, None);

    assert!(
        documents.get(&uri).is_some(),
        "the typescript document should be tracked"
    );
    assert!(
        documents.get_ide(&uri).is_none(),
        "plain typescript files should not have Vue IDE output"
    );
    assert!(
        !diags.iter().any(|d| {
            matches!(
                &d.code,
                Some(NumberOrString::String(code)) if code == "XMissingEndTag"
            )
        }),
        "plain typescript files must not surface Verter template parse diagnostics, got: {diags:?}"
    );
    assert!(
        diags.is_empty(),
        "plain typescript files should not publish Verter diagnostics, got: {diags:?}"
    );
}

#[test]
fn compute_verter_diagnostics_surfaces_one_projection_budget_failure_at_macro_span() {
    let host = Arc::new(VerterHost::new_standalone(verter_session::HostConfig {
        analysis_level: verter_session::AnalysisLevel::Full,
        projection_op_budget: 1,
        ..verter_session::HostConfig::default()
    }));
    let documents = Arc::new(DocumentRegistry::new(Arc::clone(&host)));
    let uri: Uri = "file:///workspace/src/Limited.vue".parse().unwrap();
    let source = r#"<script setup lang="ts">
type Box<T> = { value: T }
type Props<T> = { first: Box<T>; second: Box<T> }
const props = defineProps<Props<string>>()
</script>
<template>{{ props.first.value }}</template>
"#;
    let _ = documents.did_open(&TextDocumentItem {
        uri: uri.clone(),
        language_id: "vue".to_string(),
        version: 1,
        text: source.to_string(),
    });

    let diagnostics =
        crate::server::document_diagnostics_for_test(&documents, &uri, &DashMap::new(), None);
    let limited: Vec<_> = diagnostics
        .iter()
        .filter(|diagnostic| {
            diagnostic.code
                == Some(NumberOrString::String(
                    crate::features::diagnostics::TYPE_EXPANSION_BUDGET_CODE.to_string(),
                ))
        })
        .collect();

    assert_eq!(
        limited.len(),
        1,
        "public diagnostics must deduplicate the root limit"
    );
    assert_eq!(
        limited[0].message,
        "Type expansion exceeded Verter's safe evaluation budget."
    );
    assert_eq!(limited[0].severity, Some(DiagnosticSeverity::WARNING));
    assert_eq!(
        limited[0].range.start.line, 3,
        "the failure must point at the defineProps root demand"
    );
}

/// Proves that `compute_verter_diagnostics_for` bypasses its cache when the
/// host's `diagnostics_generation` changes (even if the document version hasn't).
#[test]
fn compute_verter_diagnostics_bypasses_cache_after_host_recompile() {
    use verter_session::{CompileErrorPolicy, FileLanguage, UpsertRequest};

    let host = Arc::new(VerterHost::new_standalone(verter_session::HostConfig {
        dev_mode: false,
        compile_error_policy: CompileErrorPolicy::StrictError,
        ..verter_session::HostConfig::default()
    }));
    let documents = Arc::new(DocumentRegistry::new(Arc::clone(&host)));

    // SFC with a macro type dep on ./types
    let source = "<script setup lang=\"ts\">\nimport type { Props } from './types'\nconst props = defineProps<Props>()\n</script>\n<template><div>{{ props.msg }}</div></template>";
    let uri: Uri = "file:///workspace/src/Comp.vue".parse().unwrap();
    let _ = documents.did_open(&TextDocumentItem {
        uri: uri.clone(),
        language_id: "vue".to_string(),
        version: 1,
        text: source.to_string(),
    });

    let cached_verter_diags = Arc::new(DashMap::new());

    // First call — should contain HOST_MISSING_MACRO_TYPE_DEP
    let diags1 =
        crate::server::document_diagnostics_for_test(&documents, &uri, &cached_verter_diags, None);
    assert!(
        diags1.iter().any(|d| matches!(
            &d.code,
            Some(NumberOrString::String(c)) if c.contains("HOST_MISSING_MACRO_TYPE_DEP")
        )),
        "first call should contain HOST_MISSING_MACRO_TYPE_DEP, got: {diags1:?}"
    );

    // Load the dependency
    let _ = host.upsert(UpsertRequest {
        canonical_id: None,
        input_id: "/workspace/src/types.ts".to_string(),
        source: Arc::from("export interface Props { msg: string }"),
        file_language: FileLanguage::script_ts(),
        aliases: vec![],
    });

    // Force recompile with the tsx_profile (same as documents.get_diagnostics uses)
    let _ = host.ensure_compiled("/workspace/src/Comp.vue", &documents.tsx_profile.read());

    // Second call — same doc version, but diagnostics_generation changed
    let diags2 =
        crate::server::document_diagnostics_for_test(&documents, &uri, &cached_verter_diags, None);
    assert!(
            !diags2.iter().any(|d| matches!(
                &d.code,
                Some(NumberOrString::String(c)) if c.contains("HOST_MISSING_MACRO_TYPE_DEP")
            )),
            "second call should NOT contain HOST_MISSING_MACRO_TYPE_DEP after dep loaded, got: {diags2:?}"
        );
}

/// Gap (d) defect 2 — an OWNER-RESOLVED tsserver carrier reaches the store/ledger
/// through the reconciler, NOT the no-op ProjectSync content verbs. Driven through
/// `sync_compiled_carrier_to_provider` (the post-compile sync decision
/// `resync_background_carrier_file` calls). DISCRIMINATING: pre-change the
/// owner-resolved branch ran `sync_tsx`/`sync_dts` (tsserver NO-OPS), so the ledger
/// stayed empty; the publish gap meant background/file-watch updates never reached
/// the store.
#[tokio::test(flavor = "multi_thread")]
async fn sync_compiled_owner_resolved_publishes_ledger_via_reconciler() {
    use crate::external_ts::CanonicalSource;

    let provider = Arc::new(MockTypeProvider::new());
    let type_provider: Arc<dyn TypeProvider> = provider.clone();
    let service = make_hover_test_service(type_provider);
    let server = service.inner();
    let workspace_root = unique_server_ws_root("membership_publish");
    let tsconfig = format!("{workspace_root}/tsconfig.json");
    let canonical_id = format!("{workspace_root}/src/App.vue");
    install_test_resolver_for_root(server, &workspace_root, Some(&tsconfig));
    open_test_vue(server, &canonical_id, MEMBERSHIP_TEST_VUE);

    assert!(
        !server
            .membership_ledger()
            .expect("ledger")
            .is_advertised(&CanonicalSource::from(canonical_id.as_str())),
        "precondition: not advertised before the resync publish"
    );

    // The post-compile sync decision for an owner-resolved tsserver carrier. The
    // tsserver owned branch publishes through `publish_carrier_to_external_ts`
    // (which compiles internally), so the ledger advertises regardless of the
    // passed IDE output.
    server
        .sync_compiled_carrier_to_provider(&canonical_id, None, None)
        .await;

    assert!(
        server
            .membership_ledger()
            .expect("ledger")
            .is_advertised(&CanonicalSource::from(canonical_id.as_str())),
        "an owner-resolved tsserver carrier MUST be advertised in the ledger via the \
         reconciler (not the no-op ProjectSync verbs)"
    );
    // The ledger-backed advertised set for the project includes the carrier's
    // companion provider paths.
    let advertised = server.external_ts_advertised_for_project(&tsconfig);
    assert!(
        advertised.iter().any(|p| p.starts_with(&canonical_id)),
        "the project's ledger-backed getExternalFiles set must include the carrier's \
         companions, got {advertised:?}"
    );
}

/// With the production workspace scan parked on an unrelated item, a replayed
/// open document whose dependency content was already delivered is still not
/// certified until its DependencyReady receipt is minted: the coordinator's
/// tick decides the publication during the parked scan and fail-closes on the
/// missing receipt — the hold the counter observes — even though the engine
/// already holds the dependency. Once the receipt is minted the target is
/// certified with a current merged-diagnostics receipt while level 2 of the
/// readiness ladder is still unannounced, and level 2 follows only once the
/// parked item lets the scan finish.
#[tokio::test(flavor = "multi_thread")]
async fn a_scan_parked_on_unrelated_work_does_not_delay_an_open_documents_diagnostics() {
    let fixture = scan_publication_fixture(true).await;
    let server = fixture.server();
    let (scan_parked, release_scan) = crate::sync_coordinator::test_hooks::block_after_ide_compile(
        &fixture.id("far/Unrelated.vue"),
    );
    // Hold the target's import-publication pass — the one route that mints
    // its DependencyReady receipt. The open's eager warmup still delivers the
    // child into the engine (its content is current); only the receipt lags,
    // so a publication decided while this guard is held can only be held back
    // by the receipt gate, never excused by the delivery.
    let publication = server.import_sync.lock_for(&fixture.id("src/Target.vue"));
    let publication_parked = publication.lock().await;
    let target = fixture.open("src/Target.vue").await;
    let holds_before = server.sync_coordinator.dependency_holds();

    server.spawn_background_init(None, "parked scan").await;
    tokio::time::timeout(std::time::Duration::from_secs(20), scan_parked.notified())
        .await
        .expect("the workspace scan must reach the unrelated item");
    fixture.await_ready_announced().await;
    fixture
        .await_while_polling(
            &target,
            || server.sync_coordinator.dependency_holds() > holds_before,
            "the parked scan must decide the target's publication and hold it on the missing receipt",
        )
        .await;
    assert!(
        !server.documents.diagnostics_ready(&target),
        "a publication held for want of its receipt must not be certified"
    );
    assert!(server.sync_coordinator.workspace_scan_in_progress());

    drop(publication_parked);
    fixture
        .await_while_polling(
            &target,
            || {
                server.documents.diagnostics_ready(&target)
                    && server.sync_coordinator.diag_tasks_live() == 0
            },
            "the target must be certified while the scan is parked elsewhere",
        )
        .await;
    assert!(server.sync_coordinator.workspace_scan_in_progress());
    assert!(
        fixture.sync_complete.lock().is_empty(),
        "level 2 must stay unannounced while a scan item is parked"
    );

    release_scan.notify_one();
    fixture.await_sync_complete().await;
    assert!(!server.sync_coordinator.workspace_scan_in_progress());
}

#[tokio::test(flavor = "multi_thread")]
async fn watched_ts_delete_republishes_only_open_importers() {
    use verter_workspace::WorkspaceRead;
    let fixture = watched_dependency_fixture(true).await;
    let server = fixture.service.inner();
    let helper = format!("{}/src/helper.ts", fixture.root);
    let consumer = format!("{}/src/Consumer.vue", fixture.root);
    let uri = workspace_uri(&fixture.root, "src/Consumer.vue");
    let workspace = server.vfs_workspace.read().clone().unwrap();
    assert!(workspace.file_exists(&helper));
    // A closed importer remains in the graph but is not owed editor diagnostics.
    let closed = format!("{}/src/Closed.vue", fixture.root);
    server.documents.host().upsert(UpsertRequest {
        canonical_id: Some(closed.clone()), input_id: closed.clone(),
        source: Arc::from("<script setup lang=\"ts\">import { value } from './helper';</script><template>{{ value }}</template>"),
        file_language: FileLanguage::vue(), aliases: Vec::new(),
    }).unwrap();
    server.refresh_carrier_dependency_tracking(&closed);
    let affected = workspace.affected_canonicals(&helper);
    assert!(affected.contains(&consumer) && affected.contains(&closed));
    let surface = server.capture_provider_request_surface(&uri).unwrap();
    let provider_path = surface.stamp.provider_path.to_string();
    let start = surface.provider_content.find("./helper").unwrap() as u32;
    fixture.provider.set_diagnostics(
        &provider_path,
        vec![TypeDiagnostic {
            message: "Cannot find module './helper'".into(),
            severity: crate::type_provider::protocol::TypeDiagnosticSeverity::Error,
            start,
            end: start + 8,
            code: Some("2307".into()),
            tags: Vec::new(),
            related_information: Vec::new(),
        }],
    );
    fixture.provider.clear_calls();
    std::fs::remove_file(fixture._temp.path().join("src/helper.ts")).unwrap();
    crate::server::lifecycle::handle_did_change_watched_files(
        server,
        DidChangeWatchedFilesParams {
            changes: vec![FileEvent {
                uri: workspace_uri(&fixture.root, "src/helper.ts"),
                typ: FileChangeType::DELETED,
            }],
        },
    )
    .await;
    assert!(
        !server.documents.diagnostics_ready(&uri),
        "dependency deletion must retire the clean receipt before returning"
    );
    server
        .sync_coordinator
        .await_until(
            || {
                server.documents.diagnostics_ready(&uri)
                    && server.sync_coordinator.diag_tasks_live() == 0
            },
            || panic!("watched dependency deletion never republished the consumer"),
        )
        .await;
    assert!(!workspace.file_exists(&helper));
    let calls = fixture.provider.calls();
    // An engine that is an LSP server learns about the disk only from its
    // client, so the event itself is forwarded — before anything is re-checked.
    let forwarded = calls
        .iter()
        .position(|call| {
            matches!(
                call,
                MockCall::WatchedFilesChanged { changes }
                    if changes.as_slice() == [verter_type_runtime::WatchedFileChange {
                        path: helper.clone(),
                        kind: verter_type_runtime::WatchedFileChangeKind::Deleted,
                    }]
            )
        })
        .expect("the provider must be told the dependency was deleted");
    assert!(
        calls.iter().enumerate().all(|(index, call)| {
            !matches!(call, MockCall::GetDiagnostics { .. }) || index > forwarded
        }),
        "the importer is re-checked only after the provider knows about the disk: {calls:?}"
    );
    let close = calls
        .iter()
        .position(|call| matches!(call, MockCall::CloseFile { path } if path == &helper))
        .unwrap();
    let pulls: Vec<_> = calls
        .iter()
        .enumerate()
        .filter_map(|(index, call)| match call {
            MockCall::GetDiagnostics { path } => Some((index, path)),
            _ => None,
        })
        .collect();
    assert!(!pulls.is_empty());
    assert!(
        pulls
            .iter()
            .all(|(index, path)| *index > close && *path == &provider_path),
        "only the open consumer must be checked, after helper close: {calls:?}"
    );
    tokio::time::timeout(std::time::Duration::from_secs(5), async {
        loop {
            let changed = fixture.published_changed.notified();
            if fixture.published.lock().iter().any(|batch| {
                batch.uri == uri
                    && batch.version == Some(1)
                    && batch.diagnostics.iter().any(|diagnostic| {
                        diagnostic.code == Some(NumberOrString::String("2307".into()))
                    })
            }) {
                break;
            }
            changed.await;
        }
    })
    .await
    .expect("the unchanged consumer must receive the new missing-module diagnostic");
}

/// A STYLE-ONLY edit must not silently erase the file's diagnostics.
///
/// This is the one edit shape that clears without arming anything. The host
/// upsert clears `latest_diagnostics` on any semantic change, and a style slice
/// is one — verified here rather than assumed, by asserting the errors are
/// present before the edit and that the edit really did classify as style-only
/// (the template text is byte-identical across it). But `handle_did_change`
/// skipped the whole coordinator block for a style-only edit, because none of
/// what it does — provider sync, hover-cache invalidation, dependency-frontier
/// refresh, import republication — is owed for a CSS tweak.
///
/// The result was the branch's own regression in a narrower window, reached with
/// no race at all: change a colour, and every template error in the file stops
/// being reported. Worse than merely going quiet, the republish that follows
/// pushes the emptiness to the editor.
///
/// The invariant this pins is the general one — anything that clears
/// `latest_diagnostics` must arm the recompute that refills it — checked at the
/// shape that violated it. Driven through the real serve-loop ingress so it
/// covers `handle_did_change`'s wiring, not a hand-staged signal.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_style_only_edit_does_not_erase_the_files_diagnostics() {
    // A template whose STRUCTURE parses (so the upsert can diff slices and
    // classify the edit as style-only at all) but whose directive expression does
    // not compile, held BYTE-IDENTICAL across the edit, plus a style block that
    // is the only thing that moves. A template broken badly enough to fail
    // parsing is NOT usable here: slice diffing cannot classify it, the upsert
    // reports no change, and nothing is cleared — the test would pass vacuously.
    let revision = |color: &str| {
        format!(
            "<script setup lang=\"ts\">\nconst count = 1\n</script>\n\
             <template>\n  <div v-if=\"count ===\">{{{{ count }}}}</div>\n</template>\n\
             <style scoped>\n.a {{ color: {color}; }}\n</style>\n"
        )
    };
    let host = Arc::new(VerterHost::new_standalone(HostConfig::default()));
    let service = ingress_measurement_server(&host);
    let uri = open_test_vue(
        service.inner(),
        "/workspace/src/Styled.vue",
        &revision("red"),
    );
    let canonical_id = crate::documents::uri_to_canonical_id(&uri);
    let profile = service.inner().documents.tsx_profile.read().clone();

    let opened = host
        .get_diagnostics(&canonical_id, &profile)
        .map(|snapshot| snapshot.diagnostics.len())
        .unwrap_or(0);
    assert!(
        opened > 0,
        "precondition: the malformed template must report Verter's own parse \
         errors at open, or this test has nothing to lose"
    );
    let cached_verter_diags = Arc::clone(&service.inner().cached_verter_diags);
    let coordinator = service.inner().sync_coordinator.clone();

    let (mut client_to_server, serve) = serve_over_duplex_initialized(service).await;
    let styled = revision("blue");
    {
        use tokio::io::AsyncWriteExt;
        let body = serde_json::to_string(&serde_json::json!({
            "jsonrpc": "2.0",
            "method": "textDocument/didChange",
            "params": {
                "textDocument": { "uri": uri.as_str(), "version": 2 },
                "contentChanges": [ { "text": styled } ],
            },
        }))
        .expect("didChange frame serializes");
        client_to_server
            .write_all(format!("Content-Length: {}\r\n\r\n{}", body.len(), body).as_bytes())
            .await
            .expect("the edit must reach the server");
        client_to_server.flush().await.expect("flush the edit");
    }

    // Wait for the debounced republish stamped with the edited version.
    coordinator
        .await_until(
            || {
                cached_verter_diags
                    .get(uri.as_str())
                    .is_some_and(|entry| entry.0 == 2)
            },
            || {
                panic!(
                    "the style-only edit never produced a republish for v2; cached version: \
                     {:?}",
                    cached_verter_diags.get(uri.as_str()).map(|entry| entry.0)
                )
            },
        )
        .await;
    let published = cached_verter_diags
        .get(uri.as_str())
        .map(|entry| entry.2.clone())
        .expect("v2 republish was awaited");

    let parse_errors = published
        .iter()
        .filter(|diagnostic| {
            matches!(
                diagnostic.code.as_ref(),
                Some(NumberOrString::String(code)) if code.starts_with('X')
            )
        })
        .count();
    assert!(
        parse_errors > 0,
        "a style-only edit erased the file's template errors: the host cleared \
         `latest_diagnostics` for it and nothing was armed to recompute them, so \
         the republish pushed an empty set for a file whose template still does \
         not parse. Published: {:?}",
        published
            .iter()
            .map(|diagnostic| diagnostic.code.clone())
            .collect::<Vec<_>>()
    );

    // The edit really was style-only: a template change would make this test
    // pass through the ordinary edit path and prove nothing about the skip.
    assert!(
        host.get_source(&canonical_id)
            .as_deref()
            .is_some_and(|source| source.contains("color: blue")),
        "precondition: the styled revision must be the committed one"
    );
    serve.abort();
}

/// tsserver serves carriers from the publish store, so the gateway's
/// publication is its IDE-leg application. Once a revision is published,
/// committed and advertised, an open owes nothing and a repair re-armed for the
/// same revision publishes nothing; an edit owes the leg again.
#[tokio::test(flavor = "multi_thread")]
async fn a_tsserver_revision_already_published_is_not_published_again() {
    let provider = Arc::new(MockTypeProvider::new());
    let type_provider: Arc<dyn TypeProvider> = provider.clone();
    let service = make_hover_test_service(type_provider);
    let server = service.inner();
    let canonical_id = "/workspace/src/PublishedOnce.vue";
    install_test_resolver_for_root(server, "/workspace", Some("/workspace/tsconfig.json"));
    let uri = open_test_vue(server, canonical_id, MEMBERSHIP_TEST_VUE);
    server.ensure_current_file_synced(&uri).await;
    let ide_path = server
        .provider_sync_state_for_source(canonical_id)
        .and_then(|state| state.ide_path)
        .expect("the repair committed the published carrier");
    let published = server
        .documents
        .provider_surfaces()
        .current_snapshot(&ide_path)
        .expect("the publication recorded the IDE companion")
        .stamp
        .generation;

    assert!(
        !server.ide_leg_owed_for_open_document(&uri),
        "a published, committed and advertised revision owes no IDE leg"
    );
    server.needs_ide_sync.insert(canonical_id.to_string());
    server.ensure_current_file_synced(&uri).await;
    assert_eq!(
        server
            .documents
            .provider_surfaces()
            .current_snapshot(&ide_path)
            .expect("the IDE companion stays recorded")
            .stamp
            .generation,
        published,
        "a repair of an already-published revision must not publish it a second time"
    );

    let _ = server
        .documents
        .did_change(&uri, 2, &MEMBERSHIP_TEST_VUE.replace("'hi'", "'edited'"));
    assert!(
        server.ide_leg_owed_for_open_document(&uri),
        "an edit owes the leg again"
    );
}

/// The completion recovery republishes an OPEN document's companions, which
/// records and versions its surfaces. It must take the document's lane, so it
/// cannot supersede a lane holder's recorded surface mid-transaction.
///
/// **The regression boundary is the WIRING, so the proof drives the wiring.**
/// The defect this fences was the recovery closure in `server/nav_features.rs`
/// choosing the lane-less `publish_carrier_to_external_ts` over the lane-taking
/// `publish_open_carrier_to_external_ts`, so the test drives the PRODUCTION
/// completion path: a completion request whose provider query fails, which is
/// what makes the shared bounded recovery run its resync arm, with the
/// document's lane held by this test.
///
/// **The oracle is an observed ordering event, not a deadline.** Two fence
/// points are armed, and the recovery arm can only ever park at ONE of them:
///
/// * `at_fence` — inside `publish_open_carrier_to_external_ts`, after the open
///   generation resolves and before the lease is taken. Only the lane-taking
///   wrapper has this point, so reaching it is positive proof the call site
///   chose the fenced entry.
/// * `at_compile` — inside the shared `publish_carrier_to_external_ts`, after
///   its own compile. A lane-bypassing arm never enters the wrapper and so
///   lands here directly, without ever taking the lane.
///
/// The fenced arm therefore parks at `at_fence` and blocks on the held lease
/// before any compile; the unfenced arm parks at `at_compile`. Waiting for
/// whichever arrives first and asserting which one it was is scheduling
/// independent: a lane-bypassing arm that is merely SLOW still fails, and a
/// correct arm that is slow still passes. The remaining timeouts only bound
/// genuine hangs on positively-observed events.
#[tokio::test(flavor = "multi_thread")]
async fn the_open_carrier_republish_waits_for_the_document_lane() {
    const SETTLE: std::time::Duration = std::time::Duration::from_secs(30);

    let provider = Arc::new(MockTypeProvider::new());
    let type_provider: Arc<dyn TypeProvider> = provider.clone();
    let service = make_hover_test_service(type_provider);
    let server = service.inner();
    let canonical_id = "/workspace/src/RepublishOnLane.vue";
    install_test_resolver_for_root(server, "/workspace", Some("/workspace/tsconfig.json"));
    // A member-access position, so completion consults the type provider
    // instead of answering from the template's own render-proxy scope.
    let source = "<script setup lang=\"ts\">\nconst state = { count: 0 }\n</script>\n\
                  <template><div>{{ state.count }}</div></template>\n";
    let uri = open_test_vue(server, canonical_id, source);
    // Settle the document's own IDE repair first: the completion request's
    // inline repair shares this document's lane, so a still-owed repair would
    // queue behind the test's holder instead of reaching the recovery arm.
    server.ensure_current_file_synced(&uri).await;
    let line_index = server
        .documents
        .get(&uri)
        .expect("open document")
        .line_index
        .clone();
    let position = line_index
        .offset_to_position(source.find("state.count").expect("member access") as u32 + 6)
        .expect("member-access source position");

    // Both recovery attempts fail, so the resync arm is the only thing this
    // request can do after its first `Err`.
    provider.fail_next_completions(4);
    let (at_fence, pass_fence) = server.pause_next_open_carrier_publish_before_lease(canonical_id);
    let (at_compile, pass_compile) = server.pause_next_publish_carrier_after_compile(canonical_id);
    let published_before = server
        .membership_ledger()
        .expect("tsserver has a ledger")
        .record_snapshot(&crate::external_ts::CanonicalSource::from(canonical_id));
    let held = match server.documents.try_delivery_lane(canonical_id) {
        crate::document_sync_lane::DeliveryLane::Acquired(guard) => guard,
        other => panic!("the open document's lane is free, got {other:?}"),
    };

    let request = super::super::nav_features::handle_completion(
        server,
        completion_params(&uri, position, None),
    );
    tokio::pin!(request);
    // Every wait below is a `select!` that ALSO polls the request: a parked
    // arm only progresses while the request future is being driven.
    let parked = tokio::select! {
        biased;
        _ = at_fence.notified() => "the fenced wrapper's pre-lease fence point",
        _ = at_compile.notified() => "the shared publish's post-compile seam",
        finished = &mut request => panic!(
            "the completion request returned before the recovery arm reached either fence \
             point (recovery ran the lane-less publish and finished: {finished:?})"
        ),
    };
    assert_eq!(
        parked, "the fenced wrapper's pre-lease fence point",
        "the completion recovery reached {parked} while the document's lane was held: the \
         recovery arm wrote an open document's surface without taking the lane"
    );
    assert_eq!(
        server
            .membership_ledger()
            .expect("tsserver has a ledger")
            .record_snapshot(&crate::external_ts::CanonicalSource::from(canonical_id)),
        published_before,
        "an arm parked at the fence has published nothing, so it cannot have superseded the \
         lane holder's recorded surface"
    );

    // Handing the fence over and releasing the holder lets the queued arm take
    // the lane, reach the compile seam it could not reach before, and publish
    // on its turn.
    drop(held);
    pass_fence.notify_one();
    tokio::time::timeout(SETTLE, async {
        let mut released_seam = false;
        loop {
            tokio::select! {
                biased;
                _ = at_compile.notified(), if !released_seam => {
                    released_seam = true;
                    pass_compile.notify_one();
                }
                result = &mut request => return result,
            }
        }
    })
    .await
    .expect("the released arm reaches the publish's compile seam and the request finishes")
    .expect("the completion request succeeds");
    assert!(
        server
            .membership_ledger()
            .expect("tsserver has a ledger")
            .is_advertised(&crate::external_ts::CanonicalSource::from(canonical_id)),
        "the republish ran on its turn"
    );
}
