use super::*;

/// W02 feature-set consistency (signature help): signature help is a
/// request-answering provider-backed feature, so it heals a projection-less
/// carrier through the SAME attempt-bounded interactive repair hover and
/// completion use — it must not stay dark until a background tick.
#[tokio::test(flavor = "multi_thread")]
async fn signature_help_heals_a_projectionless_carrier_on_the_first_request() {
    let broken_source = "<script setup lang=\"ts\">\nconst broken = (((\n";
    let fixed_source = "<script setup lang=\"ts\">\nconst healedTarget: string = \"ok\"\n</script>\n<template><div>{{ healedTarget.at(0) }}</div></template>\n";
    let kind = crate::TypeProviderKind::Tsgo;
    let (tsx_path, tsx_offset, position) = provider_target_via_twin_server(
        kind,
        "/workspace/src/App.vue",
        "vue",
        fixed_source,
        "at(0)",
        3,
    );

    let provider = Arc::new(MockTypeProvider::new());
    let type_provider: Arc<dyn TypeProvider> = provider.clone();
    let service = make_hover_test_service_with_kind(type_provider, kind);
    let server = service.inner();
    install_test_resolver(server);

    let uri = open_test_vue(server, "/workspace/src/App.vue", broken_source);
    assert!(
        server.documents.get_projection(&uri).is_none(),
        "precondition: the malformed open must leave the carrier projection-less"
    );
    let _ = server.documents.did_change(&uri, 2, fixed_source);
    assert!(
        server.documents.get_projection(&uri).is_none(),
        "the commit must not compile"
    );

    provider.set_signature_help(
        &tsx_path,
        tsx_offset,
        Some(crate::type_provider::protocol::SignatureHelp {
            signatures: vec![crate::type_provider::protocol::SignatureInfo {
                label: "at(index: number): string | undefined".to_string(),
                documentation: None,
                parameters: vec![],
                active_parameter: None,
            }],
            active_signature: Some(0),
            active_parameter: None,
        }),
    );

    let help = server
        .signature_help(SignatureHelpParams {
            text_document_position_params: TextDocumentPositionParams {
                text_document: TextDocumentIdentifier { uri: uri.clone() },
                position,
            },
            work_done_progress_params: WorkDoneProgressParams::default(),
            context: None,
        })
        .await
        .expect("signature help request must not error to the client");
    assert!(
        help.is_some(),
        "the FIRST signature-help request after the fix must heal the missing \
         projection and answer through the provider — None means signature \
         help stays dark on a carrier hover already heals"
    );
    assert!(
        server.documents.get_projection(&uri).is_some(),
        "the interactive repair must have installed the projection"
    );
    let signature_calls = provider
        .calls()
        .iter()
        .filter(|call| matches!(call, MockCall::GetSignatureHelp { .. }))
        .count();
    assert!(
        signature_calls >= 1,
        "the answer must have come through the provider rail"
    );
}

/// W02 feature-set consistency (code actions): quickfix code actions are a
/// request-answering provider-backed feature, so they heal a projection-less
/// carrier through the same attempt-bounded interactive repair.
#[tokio::test(flavor = "multi_thread")]
async fn code_action_heals_a_projectionless_carrier_on_the_first_request() {
    let broken_source = "<script setup lang=\"ts\">\nconst broken = (((\n";
    let fixed_source = "<script setup lang=\"ts\">\nconst healedTarget: string = \"ok\"\n</script>\n<template><div>{{ healedTarget.length }}</div></template>\n";
    let kind = crate::TypeProviderKind::Tsgo;

    let provider = Arc::new(MockTypeProvider::new());
    let type_provider: Arc<dyn TypeProvider> = provider.clone();
    let service = make_hover_test_service_with_kind(type_provider, kind);
    let server = service.inner();
    install_test_resolver(server);

    let uri = open_test_vue(server, "/workspace/src/App.vue", broken_source);
    assert!(
        server.documents.get_projection(&uri).is_none(),
        "precondition: the malformed open must leave the carrier projection-less"
    );
    let _ = server.documents.did_change(&uri, 2, fixed_source);
    assert!(
        server.documents.get_projection(&uri).is_none(),
        "the commit must not compile"
    );

    let position = find_document_position(server, &uri, "healedTarget.length", 1);
    let _ = server
        .code_action(CodeActionParams {
            text_document: TextDocumentIdentifier { uri: uri.clone() },
            range: Range {
                start: position,
                end: position,
            },
            context: CodeActionContext::default(),
            work_done_progress_params: WorkDoneProgressParams::default(),
            partial_result_params: PartialResultParams::default(),
        })
        .await
        .expect("code action request must not error to the client");
    assert!(
        server.documents.get_projection(&uri).is_some(),
        "the interactive repair must have installed the projection — a \
         projection-less carrier must not silently skip the provider action \
         surface while hover on the same carrier heals"
    );
    let action_calls = provider
        .calls()
        .iter()
        .filter(|call| matches!(call, MockCall::GetCodeActions { .. }))
        .count();
    assert!(
        action_calls >= 1,
        "the healed request must have queried the provider action surface"
    );
}

// ── wants_code_action_kind tests ────────────────────────────────

#[test]
fn test_wants_code_action_kind_no_filter() {
    // No `only` → all kinds wanted
    assert!(wants_code_action_kind(None, "quickfix"));
    assert!(wants_code_action_kind(None, "source.organizeImports"));
    assert!(wants_code_action_kind(None, "refactor.extract"));
}

#[test]
fn test_wants_code_action_kind_exact_match() {
    let kinds = vec![CodeActionKind::new("quickfix")];
    assert!(wants_code_action_kind(Some(&kinds), "quickfix"));
    assert!(!wants_code_action_kind(Some(&kinds), "refactor"));
    assert!(!wants_code_action_kind(
        Some(&kinds),
        "source.organizeImports"
    ));
}

#[test]
fn test_wants_code_action_kind_prefix_hierarchy() {
    // `only: [refactor]` should match `refactor.extract`
    let kinds = vec![CodeActionKind::new("refactor")];
    assert!(wants_code_action_kind(Some(&kinds), "refactor.extract"));
    assert!(wants_code_action_kind(Some(&kinds), "refactor"));
    assert!(!wants_code_action_kind(Some(&kinds), "quickfix"));

    // `only: [refactor.extract]` should match `refactor` (parent)
    let kinds = vec![CodeActionKind::new("refactor.extract")];
    assert!(wants_code_action_kind(Some(&kinds), "refactor"));
    assert!(wants_code_action_kind(Some(&kinds), "refactor.extract"));
    assert!(!wants_code_action_kind(Some(&kinds), "quickfix"));
}

#[test]
fn test_wants_code_action_kind_no_false_prefix() {
    // "quickfixExtra" should NOT match "quickfix"
    let kinds = vec![CodeActionKind::new("quickfix")];
    assert!(!wants_code_action_kind(Some(&kinds), "quickfixExtra"));

    // "refactoring" should NOT match "refactor"
    let kinds = vec![CodeActionKind::new("refactor")];
    assert!(!wants_code_action_kind(Some(&kinds), "refactoring"));
}

#[test]
fn test_wants_code_action_kind_multiple_kinds() {
    let kinds = vec![
        CodeActionKind::new("quickfix"),
        CodeActionKind::new("source.organizeImports"),
    ];
    assert!(wants_code_action_kind(Some(&kinds), "quickfix"));
    assert!(wants_code_action_kind(
        Some(&kinds),
        "source.organizeImports"
    ));
    assert!(!wants_code_action_kind(Some(&kinds), "refactor"));
}

/// F3 (review finding): the provider code-action gate must NOT fire for an
/// `only=["refactor"]` or `only=["source.removeUnused"]` request. The provider's
/// `get_code_actions` issues `getCodeFixes` only — it returns QUICKFIX-kind actions
/// (the TS6133 remove-unused-declaration fix and its delete-all companion), never
/// refactors and never source actions — so forwarding it on a refactor-only OR a
/// `source.removeUnused`-only request returns actions the client explicitly did not
/// ask for (an LSP `context.only` violation). The gate must fire ONLY for
/// `["quickfix"]` and the implicit all-kinds (`None`) case. The `source.removeUnused`
/// SOURCE action is a separate surface DEFERRED to the `source.*` backlog and is NOT
/// wired into this gate.
///
/// Discriminating: before this fix the gate included a `source.removeUnused`
/// arm, so the `only=["source.removeUnused"]` invocation recorded a
/// `MockCall::GetCodeActions`; this test asserts NO such call is recorded for
/// source.removeUnused-only (nor for the already-excluded refactor-only) while
/// one IS recorded for quickfix-only, so it FAILS pre-fix and PASSES post-fix.
#[tokio::test]
async fn code_action_provider_gate_excludes_refactor_only_request() {
    let provider = Arc::new(MockTypeProvider::new());
    let type_provider: Arc<dyn TypeProvider> = provider.clone();
    let service = make_hover_test_service(type_provider);
    let server = service.inner();
    install_test_resolver(server);

    let source = "<script setup lang=\"ts\">\nconst foo = 1\n</script>\n<template></template>\n";
    let uri = open_test_vue(server, "/workspace/src/RefactorGate.vue", source);
    server.test_ensure_synced(&uri).await;

    let decl_start = find_document_position(server, &uri, "foo = 1", 0);
    let decl_end = find_document_position(server, &uri, "foo = 1", "foo".len());
    let range = Range {
        start: decl_start,
        end: decl_end,
    };

    let make_params = |only: Option<Vec<CodeActionKind>>| CodeActionParams {
        text_document: TextDocumentIdentifier { uri: uri.clone() },
        range,
        context: CodeActionContext {
            diagnostics: vec![Diagnostic {
                range,
                code: Some(NumberOrString::String("6133".to_string())),
                source: Some("ts".to_string()),
                message: "'foo' is declared but its value is never read.".to_string(),
                ..Default::default()
            }],
            only,
            trigger_kind: None,
        },
        work_done_progress_params: Default::default(),
        partial_result_params: Default::default(),
    };

    let recorded_get_code_actions = |provider: &MockTypeProvider| {
        provider
            .calls()
            .iter()
            .any(|c| matches!(c, MockCall::GetCodeActions { .. }))
    };

    // refactor-only: the provider gate must NOT fire (no quickfix forwarding).
    super::super::aux_features::handle_code_action(
        server,
        make_params(Some(vec![CodeActionKind::REFACTOR])),
    )
    .await
    .expect("code_action returns Ok");
    assert!(
        !recorded_get_code_actions(&provider),
        "an only=[refactor] request must NOT forward to the provider quickfix path, \
         got calls={:?}",
        provider.calls()
    );

    // quickfix-only: the provider gate MUST fire.
    provider.clear_calls();
    super::super::aux_features::handle_code_action(
        server,
        make_params(Some(vec![CodeActionKind::QUICKFIX])),
    )
    .await
    .expect("code_action returns Ok");
    assert!(
        recorded_get_code_actions(&provider),
        "an only=[quickfix] request MUST forward to the provider, got calls={:?}",
        provider.calls()
    );

    // source.removeUnused-only: the provider gate must NOT fire. The provider
    // returns `quickfix`-kind actions, so forwarding a `source.removeUnused`-only
    // request would leak quickfixes the client did not ask for (a `context.only`
    // violation). The `source.removeUnused` SOURCE action is deferred to the
    // `source.*` backlog and is not wired into this gate.
    provider.clear_calls();
    let source_only_result = super::super::aux_features::handle_code_action(
        server,
        make_params(Some(vec![CodeActionKind::new("source.removeUnused")])),
    )
    .await
    .expect("code_action returns Ok");
    assert!(
        !recorded_get_code_actions(&provider),
        "an only=[source.removeUnused] request must NOT forward to the provider \
         quickfix path, got calls={:?}",
        provider.calls()
    );
    // And no `quickfix`-kinded provider action leaks back to the client.
    let leaked_quickfix =
        source_only_result
            .unwrap_or_default()
            .into_iter()
            .any(|item| match item {
                CodeActionOrCommand::CodeAction(action) => action.kind.as_ref().is_some_and(|k| {
                    k.as_str() == "quickfix" || k.as_str().starts_with("quickfix.")
                }),
                CodeActionOrCommand::Command(_) => false,
            });
    assert!(
        !leaked_quickfix,
        "an only=[source.removeUnused] request must not leak quickfix-kinded \
         provider actions"
    );

    // No `only` filter (implicit all-kinds): the provider gate MUST fire.
    provider.clear_calls();
    super::super::aux_features::handle_code_action(server, make_params(None))
        .await
        .expect("code_action returns Ok");
    assert!(
        recorded_get_code_actions(&provider),
        "an unfiltered (only=None) request MUST forward to the provider, got calls={:?}",
        provider.calls()
    );
}

/// The `source.removeUnused` source-action kind must NOT leak provider quickfixes
/// into the response. This proves the scope-creep fix end-to-end at the RESULT
/// layer (distinct from the call-recording layer the gate test above asserts on):
/// the provider is armed to return a real `quickfix`-kind "Remove unused
/// declaration" action over the unused-decl span, and an `only=["source.removeUnused"]`
/// request must come back with NO quickfix-kind action — `source.removeUnused` is a
/// SOURCE action deferred to the `source.*` backlog and is out of scope for this
/// QUICKFIX-only carrier path.
///
/// Discriminating: before the fix the gate forwarded `source.removeUnused`-only
/// requests, so the armed `quickfix` action mapped back into the response and the
/// assertion FAILS; after the fix the gate does not fire, no provider call is made,
/// the response carries no quickfix, and the assertion PASSES.
#[tokio::test]
async fn code_action_source_only_request_does_not_leak_provider_quickfix() {
    let provider = Arc::new(MockTypeProvider::new());
    let type_provider: Arc<dyn TypeProvider> = provider.clone();
    let service = make_hover_test_service(type_provider);
    let server = service.inner();
    install_test_resolver(server);

    let source = "<script setup lang=\"ts\">\nconst foo = 1\n</script>\n<template></template>\n";
    let uri = open_test_vue(server, "/workspace/src/SourceOnlyLeak.vue", source);
    server.test_ensure_synced(&uri).await;

    let decl_start = find_document_position(server, &uri, "foo = 1", 0);
    let decl_end = find_document_position(server, &uri, "foo = 1", "foo".len());

    // Arm the provider with a real `quickfix`-kind action exactly where the handler
    // would query (the carrier range mapped to TSX offsets). If the gate forwards a
    // source-only request, this action maps back and leaks into the response.
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

    let range = Range {
        start: decl_start,
        end: decl_end,
    };
    let response = super::super::aux_features::handle_code_action(
        server,
        CodeActionParams {
            text_document: TextDocumentIdentifier { uri: uri.clone() },
            range,
            context: CodeActionContext {
                diagnostics: vec![Diagnostic {
                    range,
                    code: Some(NumberOrString::String("6133".to_string())),
                    source: Some("ts".to_string()),
                    message: "'foo' is declared but its value is never read.".to_string(),
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

    // The provider quickfix path must not have run for a source-only request.
    assert!(
        !provider
            .calls()
            .iter()
            .any(|c| matches!(c, MockCall::GetCodeActions { .. })),
        "an only=[source.removeUnused] request must NOT forward to the provider \
         quickfix path, got calls={:?}",
        provider.calls()
    );
    // And no quickfix-kinded action may surface in the response — the armed action
    // would have leaked here pre-fix.
    let leaked_quickfix = response
        .unwrap_or_default()
        .into_iter()
        .any(|item| match item {
            CodeActionOrCommand::CodeAction(action) => action
                .kind
                .as_ref()
                .is_some_and(|k| k.as_str() == "quickfix" || k.as_str().starts_with("quickfix.")),
            CodeActionOrCommand::Command(_) => false,
        });
    assert!(
        !leaked_quickfix,
        "an only=[source.removeUnused] request must not leak a quickfix-kind action \
         into the response"
    );
}

/// P1-01 (mock): `getBindingTypes` derives its per-binding value SOLELY from
/// the provider's structured `display_signature` — never from the rendered
/// `contents` blob. The seeded `contents` is deliberately garbage that names
/// the binding with a WRONG type; a scrape would surface it, the structured
/// read cannot.
#[tokio::test(flavor = "multi_thread")]
async fn binding_types_read_provider_display_signature_directly() {
    let (service, provider, uri) = make_request_surface_carrier().await;
    let server = service.inner();

    let decl_pos = find_document_position(server, &uri, "const msg", 6);
    set_structured_hover_at_vue_position(
        server,
        &provider,
        &uri,
        decl_pos,
        crate::type_provider::protocol::HoverInfo {
            // A scrape of this blob would yield `WRONG_SCRAPED_TYPE` for `msg`.
            contents: "```typescript\nmsg: WRONG_SCRAPED_TYPE\n```".to_string(),
            display_signature: Some(crate::type_provider::mock::test_display_signature(
                "const msg: \"hello\"",
            )),
            ..Default::default()
        },
    );

    let value = server
        .get_binding_types(crate::server::protocol_types::GetAnalysisParams {
            uri: uri.as_str().to_string(),
        })
        .await
        .expect("getBindingTypes request should succeed");

    assert_eq!(
        value.get("msg"),
        Some(&serde_json::json!({ "displaySignature": "const msg: \"hello\"" })),
        "the wire value must be the provider's display signature verbatim, got: {value}"
    );
    // Forbidden result: nothing derived from the rendered blob may surface.
    assert!(
        !value.to_string().contains("WRONG_SCRAPED_TYPE"),
        "no consumer may recover the value from `contents`, got: {value}"
    );
}

/// SIGNATURE-HELP runtime drop coverage (representative of the aux class): a
/// re-sync landing a fresh surface generation while the request awaits the
/// provider must drop the provider signature (fail closed).
#[tokio::test(flavor = "multi_thread")]
async fn signature_help_drops_provider_result_when_surface_regenerates_mid_request() {
    let (service, provider, uri) = make_request_surface_carrier().await;
    let server = service.inner();

    let position = find_document_position(server, &uri, "{{ msg", 3);
    let ctx = synced_type_provider_context(server, &uri).await;
    let tsx_offset = merge::carrier_position_to_tsx_offset_validated(
        &position,
        &ctx.carrier_line_index,
        &ctx.mapper,
        &ctx.tsx_line_index,
    )
    .expect("position maps to tsx");
    provider.set_signature_help(
        &ctx.tsx_path,
        tsx_offset,
        Some(crate::type_provider::protocol::SignatureHelp {
            signatures: vec![crate::type_provider::protocol::SignatureInfo {
                label: "SIGNATURE_SENTINEL(msg: string): void".to_string(),
                documentation: None,
                parameters: Vec::new(),
                active_parameter: None,
            }],
            active_signature: Some(0),
            active_parameter: None,
        }),
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
        .signature_help(SignatureHelpParams {
            text_document_position_params: TextDocumentPositionParams {
                text_document: TextDocumentIdentifier { uri: uri.clone() },
                position,
            },
            work_done_progress_params: Default::default(),
            context: None,
        })
        .await
        .expect("signature help request should succeed");
    assert!(
        !response
            .iter()
            .flat_map(|s| s.signatures.iter())
            .any(|sig| sig.label.contains("SIGNATURE_SENTINEL")),
        "a provider signature produced against a superseded surface must be DROPPED, \
         got {response:?}"
    );
}

/// Signature help follows the same client-owned cancellation policy as hover.
#[tokio::test]
async fn shipped_signature_help_has_no_feature_latency_deadline() {
    let (service, provider, uri) = make_request_surface_carrier().await;
    let server = service.inner();
    provider.hang_signature_help();

    let position = find_document_position(server, &uri, "{{ msg", 3);
    let budget = server
        .documents
        .host()
        .config()
        .lsp_method_timeouts
        .request_deadlines
        .code_action;

    let outcome = tokio::time::timeout(
        std::time::Duration::from_millis(100),
        <VerterLanguageServer as tower_lsp_server::LanguageServer>::signature_help(
            server,
            SignatureHelpParams {
                context: None,
                text_document_position_params: TextDocumentPositionParams {
                    text_document: TextDocumentIdentifier { uri: uri.clone() },
                    position,
                },
                work_done_progress_params: Default::default(),
            },
        ),
    )
    .await;
    assert!(
        budget.is_zero(),
        "production signature help must be unbounded"
    );
    assert!(
        outcome.is_err(),
        "signature help returned before client cancellation, so a hidden latency deadline remains"
    );
    assert!(
        provider
            .calls()
            .iter()
            .any(|c| matches!(c, MockCall::GetSignatureHelp { .. })),
        "the handler must have reached the wedged get_signature_help, \
         else the deadline is untested"
    );
}
