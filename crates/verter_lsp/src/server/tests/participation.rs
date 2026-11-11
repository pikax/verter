//! COX0: the LSP admits every semantic request, custom method and
//! diagnostics producer through one participation gate
//! (`docs/arch/kernel/cox0d-participation.md`).

use super::*;
use verter_language::participation::CapabilityOperation as Op;
use verter_protocol::participation::ParticipationInput;

const APP: &str = "<template>\n  <div>{{ msg }}</div>\n</template>\n<script setup lang=\"ts\">\nconst msg = 'hi'\n</script>\n";

fn service() -> tower_lsp_server::LspService<VerterLanguageServer> {
    make_hover_test_service(Arc::new(MockTypeProvider::new()))
}

fn input(json: serde_json::Value) -> ParticipationInput {
    serde_json::from_value(json).expect("well-formed participation input")
}

fn everywhere(preset: &str) -> ParticipationInput {
    input(serde_json::json!({ "version": 1, "presets": [
        { "target": { "kind": "all" }, "selector": { "kind": "all" }, "preset": preset }
    ]}))
}

fn folding(uri: &Uri) -> FoldingRangeParams {
    FoldingRangeParams {
        text_document: TextDocumentIdentifier { uri: uri.clone() },
        work_done_progress_params: Default::default(),
        partial_result_params: Default::default(),
    }
}

fn refusal(error: &tower_lsp_server::jsonrpc::Error) -> Option<&str> {
    assert_eq!(
        error.code,
        tower_lsp_server::jsonrpc::ErrorCode::ServerError(-32803),
        "a refusal is RequestFailed, never an empty answer: {error:?}"
    );
    error
        .data
        .as_ref()?
        .get("verterParticipation")?
        .get("primary")?
        .as_str()
}

fn profile_ref(server: &VerterLanguageServer, uri: &Uri) -> serde_json::Value {
    let profile = verter_session::participation::file_profile(
        server.documents.host(),
        &uri_to_canonical_id(uri),
    );
    let kind = match profile {
        verter_language::participation::ProfileRef::Framework(_) => "framework",
        verter_language::participation::ProfileRef::Carrier(_) => "carrier",
        verter_language::participation::ProfileRef::Project(_) => "project",
    };
    serde_json::json!({ "kind": kind, "id": profile.digest().to_hex() })
}

/// PC04/PC12: a disabled scope refuses a request with a typed reason even
/// though the method stays statically advertised, and the same request is
/// served again once the scope is re-enabled.
#[tokio::test]
async fn disabled_scope_refuses_statically_advertised_requests() {
    let service = service();
    let server = service.inner();
    let uri = open_test_vue(server, "/cox0/App.vue", APP);
    assert!(host_server_capabilities(true)
        .folding_range_provider
        .is_some());

    let receipt = server
        .update_participation(everywhere("disabled"))
        .await
        .expect("valid input");
    assert!(receipt.changed);
    let refused = server.folding_range(folding(&uri)).await.unwrap_err();
    assert_eq!(refusal(&refused), Some("disabled"));
    let refused = server
        .get_analysis(GetAnalysisParams {
            uri: uri.as_str().to_owned(),
        })
        .await
        .unwrap_err();
    assert_eq!(
        refusal(&refused),
        Some("disabled"),
        "custom methods pass the same gate"
    );

    // `workspace` admits no interactive presentation either, for a distinct reason.
    server
        .update_participation(everywhere("workspace"))
        .await
        .expect("valid input");
    let refused = server.folding_range(folding(&uri)).await.unwrap_err();
    assert_eq!(refusal(&refused), Some("workspaceOnly"));

    server
        .update_participation(everywhere("full"))
        .await
        .expect("valid input");
    assert!(server.folding_range(folding(&uri)).await.is_ok());
}

/// PC01/PC02/PC09: a proven overlap on one document's TypeScript diagnostic
/// publication withdraws exactly that facet there. The document's other
/// facets and requests, and every other document, are untouched; an explicit
/// `full` for the facet wins over the overlap.
#[tokio::test]
async fn overlap_withdraws_only_the_overlapping_diagnostic_facet() {
    use crate::participation::DiagnosticFacets;
    let service = service();
    let server = service.inner();
    let app = open_test_vue(server, "/cox0/App.vue", APP);
    let other = open_test_vue(server, "/cox0/Other.vue", APP);
    let overlap = serde_json::json!({
        "profile": profile_ref(server, &app),
        "operation": "diagnostics.typescript",
        "selector": { "kind": "document", "uri": app.as_str() },
        "evidence": "c".repeat(64),
    });
    server
        .update_participation(input(serde_json::json!({
            "version": 1, "ownership": [overlap.clone()]
        })))
        .await
        .expect("valid input");

    assert_eq!(
        DiagnosticFacets::admitted(&server.documents, &uri_to_canonical_id(&app)),
        DiagnosticFacets {
            typescript: false,
            ..DiagnosticFacets::ALL
        }
    );
    assert_eq!(
        DiagnosticFacets::admitted(&server.documents, &uri_to_canonical_id(&other)),
        DiagnosticFacets::ALL
    );
    assert!(server.folding_range(folding(&app)).await.is_ok());

    server
        .update_participation(input(serde_json::json!({
            "version": 1,
            "ownership": [overlap],
            "presets": [{ "target": { "kind": "operation", "operation": "diagnostics.typescript" },
                          "selector": { "kind": "all" }, "preset": "full" }]
        })))
        .await
        .expect("valid input");
    assert_eq!(
        DiagnosticFacets::admitted(&server.documents, &uri_to_canonical_id(&app)),
        DiagnosticFacets::ALL
    );
}

/// DEM0 `DEM-D01`: a document whose Verter diagnostic facets are all
/// withdrawn runs no Verter producer and warms no cache entry.
#[tokio::test]
async fn withdrawn_verter_facets_run_no_producer() {
    let service = service();
    let server = service.inner();
    let uri = open_test_vue(server, "/cox0/App.vue", APP);
    server
        .update_participation(everywhere("workspace"))
        .await
        .expect("valid input");
    let cache = DashMap::new();
    let diagnostics = crate::server::verter_owned_diagnostics(
        &server.documents,
        &uri,
        &uri_to_canonical_id(&uri),
        &cache,
        None,
        None,
    );
    assert!(diagnostics.is_empty());
    assert!(cache.is_empty(), "a withdrawn publication warms nothing");
}

/// PC07: an answer computed under a superseded demand epoch is never served,
/// while an equivalent re-observation mints no epoch and leaves it current.
#[tokio::test]
async fn superseded_answers_are_never_served() {
    let service = service();
    let server = service.inner();
    let uri = open_test_vue(server, "/cox0/App.vue", APP);

    let unchanged = server
        .interactive(Op::FoldingRange, Some(&uri), async {
            let receipt = server
                .update_participation(everywhere("auto"))
                .await
                .expect("valid input");
            assert!(!receipt.changed, "restating the default is no change");
            Ok(())
        })
        .await;
    assert!(unchanged.is_ok());

    let superseded = server
        .interactive(Op::FoldingRange, Some(&uri), async {
            server
                .update_participation(everywhere("full"))
                .await
                .expect("valid input");
            Ok(())
        })
        .await
        .unwrap_err();
    assert_eq!(
        superseded.code,
        tower_lsp_server::jsonrpc::ErrorCode::ContentModified
    );
}

/// An invalid preset refuses the update and leaves the epoch unchanged; an
/// observation naming an unknown profile is reported and withdraws nothing.
#[tokio::test]
async fn invalid_inputs_change_nothing() {
    let service = service();
    let server = service.inner();
    let uri = open_test_vue(server, "/cox0/App.vue", APP);
    let before = server.documents.participation().epoch();
    let error = server
        .update_participation(everywhere("off"))
        .await
        .unwrap_err();
    assert_eq!(
        error.code,
        tower_lsp_server::jsonrpc::ErrorCode::InvalidParams
    );
    assert_eq!(server.documents.participation().epoch(), before);

    let receipt = server
        .update_participation(input(serde_json::json!({ "version": 1, "ownership": [{
            "profile": { "kind": "framework", "id": "0".repeat(64) },
            "operation": "hover",
            "selector": { "kind": "all" },
            "evidence": "d".repeat(64),
        }]})))
        .await
        .expect("observations never refuse the input");
    assert!(!receipt.changed);
    assert_eq!(
        receipt.invalid_evidence,
        vec!["ownership[0]: unknown profile"]
    );
    assert!(server.folding_range(folding(&uri)).await.is_ok());
}

/// DEM0 `DEM-D06`: opening a document in a disabled scope retains its bytes
/// and schedules no eager provider or diagnostics work.
#[tokio::test]
async fn disabled_open_schedules_no_eager_work() {
    let service = service();
    let server = service.inner();
    server
        .update_participation(everywhere("disabled"))
        .await
        .expect("valid input");
    let uri: Uri = "file:///cox0/Closed.vue".parse().expect("uri");
    server
        .did_open(DidOpenTextDocumentParams {
            text_document: TextDocumentItem {
                uri: uri.clone(),
                language_id: "vue".to_owned(),
                version: 1,
                text: APP.to_owned(),
            },
        })
        .await;
    assert!(server.documents.get(&uri).is_some(), "bytes are retained");
    assert!(!server
        .needs_deferred_sync
        .contains(&uri_to_canonical_id(&uri)));
}

/// Every code-action kind is its own cell: a request no kind admits is
/// refused, and produced actions are attributed by their LSP kind.
#[tokio::test]
async fn code_action_kinds_are_separate_cells() {
    let refactor = CodeActionOrCommand::CodeAction(CodeAction {
        title: "extract".to_owned(),
        kind: Some(CodeActionKind::REFACTOR_EXTRACT),
        ..Default::default()
    });
    let organize = CodeActionOrCommand::CodeAction(CodeAction {
        title: "organize".to_owned(),
        kind: Some(CodeActionKind::SOURCE_ORGANIZE_IMPORTS),
        ..Default::default()
    });
    let fix = CodeActionOrCommand::CodeAction(CodeAction {
        title: "fix".to_owned(),
        kind: Some(CodeActionKind::QUICKFIX),
        ..Default::default()
    });
    assert_eq!(
        crate::participation::code_action_operation(&refactor),
        Op::Refactor
    );
    assert_eq!(
        crate::participation::code_action_operation(&organize),
        Op::OrganizeImports
    );
    assert_eq!(
        crate::participation::code_action_operation(&fix),
        Op::QuickFix
    );

    let service = service();
    let server = service.inner();
    let uri = open_test_vue(server, "/cox0/App.vue", APP);
    server
        .update_participation(input(serde_json::json!({ "version": 1, "presets": [
            { "target": { "kind": "operation", "operation": "codeAction.refactor" },
              "selector": { "kind": "all" }, "preset": "disabled" }
        ]})))
        .await
        .expect("valid input");
    let params = |only: Option<Vec<CodeActionKind>>| CodeActionParams {
        text_document: TextDocumentIdentifier { uri: uri.clone() },
        range: Range::default(),
        context: CodeActionContext {
            only,
            ..Default::default()
        },
        work_done_progress_params: Default::default(),
        partial_result_params: Default::default(),
    };
    let produced = vec![refactor.clone(), organize.clone(), fix.clone()];
    let served = server
        .participating_code_actions(params(None), |narrowed| async move {
            // With no `only`, the producer pass is still told to run only
            // the admitted kinds: the withdrawn refactor producer never runs.
            assert_eq!(
                narrowed.context.only,
                Some(vec![
                    CodeActionKind::QUICKFIX,
                    CodeActionKind::SOURCE_ORGANIZE_IMPORTS
                ])
            );
            Ok(Some(produced.clone()))
        })
        .await
        .expect("two kinds stay admitted");
    assert_eq!(served, Some(vec![organize.clone(), fix.clone()]));
    // An explicit `only` reaches the producer narrowed to the admitted kinds.
    let served = server
        .participating_code_actions(
            params(Some(vec![
                CodeActionKind::REFACTOR,
                CodeActionKind::SOURCE_ORGANIZE_IMPORTS,
            ])),
            |narrowed| async move {
                assert_eq!(
                    narrowed.context.only,
                    Some(vec![CodeActionKind::SOURCE_ORGANIZE_IMPORTS])
                );
                Ok(Some(vec![organize.clone()]))
            },
        )
        .await
        .expect("organize imports stays admitted");
    assert_eq!(served.map(|actions| actions.len()), Some(1));
    let refused = server
        .participating_code_actions(params(Some(vec![CodeActionKind::REFACTOR])), |_| async {
            // A request for only withdrawn kinds must run no producer: this
            // answer would read as ContentModified, not as the refusal.
            Err::<Option<CodeActionResponse>, _>(crate::participation::superseded_error())
        })
        .await
        .unwrap_err();
    assert_eq!(refusal(&refused), Some("disabled"));

    server
        .update_participation(everywhere("disabled"))
        .await
        .expect("valid input");
    let refused = server
        .participating_code_actions(params(None), |_| async { Ok(Some(vec![refactor])) })
        .await
        .unwrap_err();
    assert_eq!(refusal(&refused), Some("disabled"));
}

/// A completion item's resolve never outlives a participation transition,
/// even one that returns to the original mask with unchanged source.
#[tokio::test]
async fn completion_items_never_resolve_across_a_transition() {
    let service = service();
    let server = service.inner();
    let issued = match crate::participation::stamp_completion(
        CompletionResponse::Array(vec![CompletionItem {
            label: "computed".to_owned(),
            data: Some(serde_json::json!({ "exportName": "computed" })),
            ..Default::default()
        }]),
        server.documents.participation().fence(),
    ) {
        CompletionResponse::Array(mut items) => items.remove(0),
        CompletionResponse::List(_) => unreachable!("an array stays an array"),
    };
    assert!(!crate::participation::completion_item_superseded(
        &server.documents,
        &issued
    ));
    server
        .update_participation(everywhere("disabled"))
        .await
        .expect("valid input");
    server
        .update_participation(everywhere("auto"))
        .await
        .expect("valid input");
    assert!(crate::participation::completion_item_superseded(
        &server.documents,
        &issued
    ));
    let refused = server.completion_resolve(issued).await.unwrap_err();
    assert_eq!(
        refused.code,
        tower_lsp_server::jsonrpc::ErrorCode::ContentModified
    );
}

/// A workspace-wide demand spans every served profile, so withdrawing one
/// profile refuses it rather than serving that profile through another's
/// admission.
#[tokio::test]
async fn workspace_wide_demands_need_every_served_profile() {
    let service = service();
    let server = service.inner();
    let app = open_test_vue(server, "/cox0/App.vue", APP);
    let symbols = || {
        crate::participation::admit(
            &server.documents,
            Op::WorkspaceSymbol,
            None,
            verter_language::participation::DemandPurpose::Interactive,
            verter_language::participation::ProviderRequirement::NotRequired,
        )
    };
    assert!(symbols().is_ok());
    server
        .update_participation(input(serde_json::json!({ "version": 1, "presets": [
            { "target": { "kind": "profile", "profile": profile_ref(server, &app) },
              "selector": { "kind": "all" }, "preset": "disabled" }
        ]})))
        .await
        .expect("valid input");
    assert_eq!(
        symbols().map(|_| ()).unwrap_err().primary,
        verter_language::participation::NonExecutionReason::Disabled
    );
}

/// A `workspace` scope admits no interactive or publication demand, so it is
/// owed no eager open work either.
#[tokio::test]
async fn workspace_scope_open_schedules_no_eager_work() {
    let service = service();
    let server = service.inner();
    server
        .update_participation(everywhere("workspace"))
        .await
        .expect("valid input");
    let uri: Uri = "file:///cox0/Workspace.vue".parse().expect("uri");
    server
        .did_open(DidOpenTextDocumentParams {
            text_document: TextDocumentItem {
                uri: uri.clone(),
                language_id: "vue".to_owned(),
                version: 1,
                text: APP.to_owned(),
            },
        })
        .await;
    assert!(!server
        .needs_deferred_sync
        .contains(&uri_to_canonical_id(&uri)));
}

/// PC09 at the host boundary: a stylesheet-region overlap withdraws only
/// the style-region cell, and the receipt reports exactly that, so a host
/// gates its client-local producers on the same cells.
#[tokio::test]
async fn receipt_reports_region_cells_for_client_local_gating() {
    let service = service();
    let server = service.inner();
    let app = open_test_vue(server, "/cox0/App.vue", APP);
    let profile = profile_ref(server, &app);
    let id = profile["id"].as_str().expect("profile id").to_owned();
    let receipt = server
        .update_participation(input(serde_json::json!({ "version": 1, "ownership": [{
            "profile": profile,
            "operation": "hover",
            "selector": { "kind": "regionRole", "role": "style" },
            "evidence": "e".repeat(64),
        }]})))
        .await
        .expect("valid input");
    assert!(receipt.changed);
    let cell = |operation: &str, role: Option<&str>| {
        receipt
            .cells
            .iter()
            .find(|cell| {
                cell.profile.id == id
                    && cell.operation == operation
                    && cell.region_role.as_deref() == role
            })
            .cloned()
            .unwrap_or_else(|| panic!("receipt cell {operation} {role:?}"))
    };
    let style_hover = cell("hover", Some("style"));
    assert!(!style_hover.admitted);
    assert_eq!(style_hover.mode, "workspaceOnly");
    assert!(cell("hover", None).admitted, "template hover stays");
    assert!(cell("completion", Some("style")).admitted);
    assert!(server.folding_range(folding(&app)).await.is_ok());
}
