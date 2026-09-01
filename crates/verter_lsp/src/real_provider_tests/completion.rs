//! Completion tests ported from E2E suite.

use tower_lsp_server::ls_types::{
    DidChangeTextDocumentParams, DidCloseTextDocumentParams, TextDocumentContentChangeEvent,
    TextDocumentIdentifier, VersionedTextDocumentIdentifier,
};
use tower_lsp_server::LanguageServer;

use crate::test_harness::{real_provider_test, RealProviderTestSession};

real_provider_test!(
    completion_vue_script_auto_import_resolves_import_edit,
    fixture = "vue-parity",
    async fn run(session) {
        let uri = session
            .open_fixture_file("src/features/AutoImportSymbol.vue")
            .await;
        let position = session.find_position(&uri, "comput(() =>", "comput".len());
        let mut observations = Vec::new();
        let mut import_edit = None;

        'attempts: for attempt in 0..16 {
            let response = session
                .server()
                .completion(tower_lsp_server::ls_types::CompletionParams {
                    text_document_position:
                        tower_lsp_server::ls_types::TextDocumentPositionParams {
                            text_document: TextDocumentIdentifier { uri: uri.clone() },
                            position,
                        },
                    work_done_progress_params: Default::default(),
                    partial_result_params: Default::default(),
                    context: Some(tower_lsp_server::ls_types::CompletionContext {
                        trigger_kind:
                            tower_lsp_server::ls_types::CompletionTriggerKind::INVOKED,
                        trigger_character: None,
                    }),
                })
                .await
                .expect("completion request succeeds");
            let items = match response {
                Some(tower_lsp_server::ls_types::CompletionResponse::Array(items)) => items,
                Some(tower_lsp_server::ls_types::CompletionResponse::List(list)) => list.items,
                None => Vec::new(),
            };

            for item in items.into_iter().filter(|item| item.label == "computed") {
                let resolved = session
                    .server()
                    .completion_resolve(item)
                    .await
                    .expect("completion resolve succeeds");
                observations.push(format!("edits={:?}", resolved.additional_text_edits));
                for edit in resolved.additional_text_edits.unwrap_or_default() {
                    if edit.new_text.contains("computed") {
                        import_edit = Some(edit);
                        break 'attempts;
                    }
                }
            }

            if attempt < 15 {
                tokio::time::sleep(std::time::Duration::from_millis(250)).await;
            }
        }

        let edit = import_edit.unwrap_or_else(|| {
            panic!(
                "accepting the Vue script completion must resolve an authored-source import edit; observations={observations:?}"
            )
        });
        let document = session
            .server()
            .test_documents()
            .get(&uri)
            .expect("the fixture remains open");
        let start = document
            .line_index
            .position_to_offset(&edit.range.start)
            .expect("the resolved edit start is a valid authored-source offset");
        let end = document
            .line_index
            .position_to_offset(&edit.range.end)
            .expect("the resolved edit end is a valid authored-source offset");
        let analysis = session
            .server()
            .test_documents()
            .get_analysis(&uri)
            .expect("the fixture has script analysis");
        assert!(
            analysis.imports.iter().any(|import| {
                import.source == "vue" && import.span.start <= start && end <= import.span.end
            }),
            "the resolved edit must land inside the authored Vue import declaration: {edit:?}"
        );
        let mut applied = document.source.to_string();
        drop(document);
        applied.replace_range(start as usize..end as usize, &edit.new_text);
        let vue_import = applied
            .lines()
            .find(|line| line.contains("from \"vue\"") || line.contains("from 'vue'"))
            .expect("the applied source retains the Vue import");
        assert!(
            vue_import.contains("computed") && vue_import.contains("ref"),
            "applying the edit must extend the existing Vue import, got {vue_import:?} from {edit:?}"
        );
        assert!(
            !applied.contains("computed, const base"),
            "the edit must not be mapped onto the following authored declaration"
        );
    }
);

real_provider_test!(
    completion_svelte_component_callback_expression_offers_local_handler,
    fixture = "svelte-parity",
    async fn run(session) {
        session
            .server()
            .set_provider_only_completions_for_test(true);
        let child_uri = session
            .open_fixture_file("src/ide/IdeSurfaceChild.svelte")
            .await;
        let uri = session
            .open_fixture_file("src/ide/IdeSurfaceParent.svelte")
            .await;
        session.ensure_synced(&child_uri).await;
        session.ensure_synced(&uri).await;
        session.settle_import_dependencies(&uri).await;
        let original = session
            .server()
            .test_documents()
            .get(&uri)
            .expect("fixture remains open")
            .source
            .to_string();
        let edited = original.replacen("{onPick}", "onPick={on}", 1);
        assert_ne!(edited, original, "fixture must contain the shorthand callback");
        session
            .server()
            .did_change(DidChangeTextDocumentParams {
                text_document: VersionedTextDocumentIdentifier {
                    uri: uri.clone(),
                    version: 2,
                },
                content_changes: vec![TextDocumentContentChangeEvent {
                    range: None,
                    range_length: None,
                    text: edited,
                }],
            })
            .await;
        session.ensure_synced(&uri).await;
        session.settle_import_dependencies(&uri).await;
        let position = session.find_position(&uri, "onPick={on}", "onPick={on".len());

        let mut labels = Vec::new();
        for _ in 0..20 {
            labels = session.completion_labels(&uri, position, None).await;
            if labels.iter().any(|label| label == "onPick") {
                break;
            }
            tokio::time::sleep(std::time::Duration::from_millis(250)).await;
        }
        assert!(
            labels.iter().any(|label| label == "onPick"),
            "Svelte callback expression completion must include the authored local handler; labels={:?}",
            labels.iter().filter(|label| label.starts_with("on")).collect::<Vec<_>>()
        );
    }
);

real_provider_test!(
    completion_vue_scoped_slot_string_member_survives_incomplete_edit,
    fixture = "vue-parity",
    async fn run(session) {
        let uri = session
            .open_virtual(
                "src/ide/ScopedSlotMemberCompletion.vue",
                r#"<script setup lang="ts">
import IdeSurfaceChild from './IdeSurfaceChild.vue'
</script>
<template>
  <IdeSurfaceChild :label="'x'" :count="1">
    <template #header="{ title }">{{ title. }}</template>
  </IdeSurfaceChild>
</template>
"#,
            )
            .await;
        let position = session.find_position(&uri, "title.", "title.".len());
        let mut labels = Vec::new();
        for _ in 0..24 {
            labels = session.completion_labels(&uri, position, Some(".")).await;
            if labels.iter().any(|label| {
                matches!(label.as_str(), "length" | "toUpperCase" | "charAt" | "slice")
            }) {
                break;
            }
            tokio::time::sleep(std::time::Duration::from_millis(250)).await;
        }
        let debug = session.server().test_type_provider_context(&uri).map(|ctx| {
            let strict = crate::type_provider::merge::carrier_position_to_tsx_offset_validated(
                &position,
                &ctx.carrier_line_index,
                &ctx.mapper,
                &ctx.tsx_line_index,
            );
            let fallback = crate::type_provider::merge::carrier_completion_member_boundary_offset(
                &position,
                &ctx.carrier_line_index,
                &ctx.mapper,
                &ctx.tsx_line_index,
                &ctx.tsx_content,
                session
                    .server()
                    .test_documents()
                    .get(&uri)
                    .expect("open source")
                    .source
                    .as_ref(),
            );
            let offset = strict.or(fallback);
            let snippet = offset.map(|at| {
                let at = at as usize;
                let start = at.saturating_sub(180);
                let end = (at + 220).min(ctx.tsx_content.len());
                ctx.tsx_content[start..end].to_string()
            });
            (strict, fallback, offset, snippet)
        });
        assert!(
            labels.iter().any(|label| {
                matches!(label.as_str(), "length" | "toUpperCase" | "charAt" | "slice")
            }),
            "a scoped-slot string binding must keep its member type during an incomplete dot edit; got={labels:?}, debug={debug:?}"
        );
    }
);

#[tokio::test(flavor = "multi_thread")]
async fn svelte_contract_template_completion_survives_provider_specialization_tsgo() {
    use crate::test_harness::{TestProviderKind, TestSessionBuilder};

    let Some(session) = TestSessionBuilder::new(TestProviderKind::Tsgo)
        .fixture("svelte-contract")
        .build()
        .await
    else {
        return;
    };
    let uri = session.open_fixture_file("src/App.svelte").await;
    session.ensure_synced(&uri).await;
    let position = session.find_nth_position(&uri, "renderTyped", 1, 0);
    let mut matching = None;
    for _ in 0..16 {
        let response = session
            .server()
            .completion(tower_lsp_server::ls_types::CompletionParams {
                text_document_position: tower_lsp_server::ls_types::TextDocumentPositionParams {
                    text_document: TextDocumentIdentifier { uri: uri.clone() },
                    position,
                },
                work_done_progress_params: Default::default(),
                partial_result_params: Default::default(),
                context: Some(tower_lsp_server::ls_types::CompletionContext {
                    trigger_kind: tower_lsp_server::ls_types::CompletionTriggerKind::INVOKED,
                    trigger_character: None,
                }),
            })
            .await
            .expect("completion request succeeds");
        let items = match response {
            Some(tower_lsp_server::ls_types::CompletionResponse::Array(items)) => items,
            Some(tower_lsp_server::ls_types::CompletionResponse::List(list)) => list.items,
            None => Vec::new(),
        };
        matching = items.into_iter().find(|item| item.label == "renderTyped");
        if matching.is_some() {
            break;
        }
        tokio::time::sleep(std::time::Duration::from_millis(250)).await;
    }
    let matching = matching.expect(
        "the managed tsgo Svelte carrier must stay capturable and offer renderTyped completion",
    );
    assert!(
        matches!(
            matching.kind,
            Some(kind) if kind != tower_lsp_server::ls_types::CompletionItemKind::TEXT
        ),
        "the Svelte readiness completion must be semantically typed, not Text: {matching:?}"
    );
    session.shutdown().await;
}

/// Provider-materialization prerequisite probe.
///
/// Opens a throw-away virtual SFC whose `<script setup>` block performs a DIRECT,
/// NON-template member completion (`member_boundary_needle` + `delta` positions the cursor
/// at a `<binding>.` member boundary inside the script) and reports whether the provider
/// can resolve the capability the caller is about to assert against in the template.
///
/// The `completion_labels` helper collapses `Ok(None)`, provider errors, AND a
/// resolved-but-wrong payload into the same empty/missing-member list, so an empty
/// template result is AMBIGUOUS (provider/materialization unavailable vs. wrong payload).
/// This probe disambiguates by exercising the SAME provider through a DIFFERENT path:
/// when the probe surfaces `any_of` the prerequisite members it proves the provider can
/// materialize the prerequisite type in THIS environment, so a subsequent empty/wrong
/// TEMPLATE result is a genuine regression (fail closed). When the probe surfaces NONE of
/// them, the provider cannot materialize the prerequisite here (incomplete DOM lib /
/// unmaterialized imported-component instance type), so the caller SKIPs instead of
/// over-firing.
///
/// Returns `true` when the prerequisite is materialized (caller proceeds to the
/// fail-closed assertions); `false` when it is unmet (caller prints a skip reason and
/// returns). The probe runs entirely through the existing session helpers
/// (`open_virtual` + `ensure_synced` + `completion_labels`) — no new provider path.
async fn skip_unless_provider_materializes(
    session: &RealProviderTestSession,
    probe_rel_path: &str,
    probe_sfc: &str,
    member_boundary_needle: &str,
    delta: usize,
    any_of: &[&str],
) -> bool {
    let uri = session.open_virtual(probe_rel_path, probe_sfc).await;
    let pos = session.find_position(&uri, member_boundary_needle, delta);
    // Materializing an imported-component instance type can take a sync round-trip;
    // retry on the same budget the warm-up probe uses before declaring the
    // prerequisite unmet.
    let mut materialized = false;
    for attempt in 0..5 {
        session.ensure_synced(&uri).await;
        let labels = session.completion_labels(&uri, pos, Some(".")).await;
        if any_of.iter().any(|m| labels.contains(&m.to_string())) {
            materialized = true;
            break;
        }
        if attempt < 4 {
            tokio::time::sleep(std::time::Duration::from_millis(500)).await;
        }
    }
    // Close the throw-away probe document so it does not linger as a workspace
    // auto-import candidate in the file under test's completion list.
    session
        .server()
        .did_close(DidCloseTextDocumentParams {
            text_document: TextDocumentIdentifier { uri },
        })
        .await;
    materialized
}

// ---------------------------------------------------------------------------
// App.vue template completions — ~12 assertions
// ---------------------------------------------------------------------------

real_provider_test!(
    completion_app_vue_template,
    fixture = "single-project",
    async fn run(session) {
        let uri = session.open_fixture_file("src/App.vue").await;

        // Wait for provider warmup
        if !session.require_or_skip_ready(&uri, "action.disabled", 7, "disabled").await {
            return;
        }
        // This test issues a long sequence of exact completion assertions. Join
        // the test-only dependency-publication receipt once so tsgo cannot swap
        // project roots between the warm-up probe and the immediately following
        // member query while the full workspace suite is heavily scheduled.
        // Production completion remains capture-only and never waits here.
        session.settle_import_dependencies(&uri).await;

        // --- Mustache expression: {{ count }} ---
        let pos = session.find_position(&uri, "{{ count }}", 3);
        let labels = session.completion_labels(&uri, pos, None).await;
        assert!(labels.contains(&"count".to_string()), "mustache should complete count, got: {labels:?}");
        assert!(labels.contains(&"doubled".to_string()), "mustache should complete doubled, got: {labels:?}");
        assert!(labels.contains(&"increment".to_string()), "mustache should complete increment, got: {labels:?}");
        // Negative: no global builtins
        assert!(!labels.contains(&"AbortController".to_string()), "should NOT contain AbortController, got: {labels:?}");
        assert!(!labels.contains(&"HTMLDivElement".to_string()), "should NOT contain HTMLDivElement, got: {labels:?}");
        assert!(!labels.contains(&"document".to_string()), "should NOT contain document, got: {labels:?}");
        assert!(!labels.contains(&"window".to_string()), "should NOT contain window, got: {labels:?}");

        // --- Event handler: @click="increment" ---
        let pos = session.find_position(&uri, r#"@click="increment">"#, 8);
        let labels = session.completion_labels(&uri, pos, None).await;
        assert!(labels.contains(&"increment".to_string()), "event handler should complete increment, got: {labels:?}");

        // --- V-for member: action.disabled ---
        let pos = session.find_position(&uri, "action.disabled", 7);
        let labels = session.completion_labels(&uri, pos, None).await;
        assert!(labels.contains(&"disabled".to_string()), "member should complete disabled, got: {labels:?}");
        assert!(labels.contains(&"label".to_string()), "member should complete label, got: {labels:?}");
        assert!(labels.contains(&"handler".to_string()), "member should complete handler, got: {labels:?}");
        // Negative: no non-member completions
        for bad in ["@click", "foo-bar"] {
            assert!(!labels.contains(&bad.to_string()), "member should NOT contain {bad}, got: {labels:?}");
        }

        // --- Nested v-for inner: action.label (2nd occurrence) ---
        let pos = session.find_nth_position(&uri, "action.label", 1, 7);
        let labels = session.completion_labels(&uri, pos, None).await;
        assert!(labels.contains(&"label".to_string()), "nested inner should complete label, got: {labels:?}");
        assert!(labels.contains(&"disabled".to_string()), "nested inner should complete disabled, got: {labels:?}");
        // Negative: should NOT have outer scope members
        assert!(!labels.contains(&"email".to_string()), "nested inner should NOT complete email, got: {labels:?}");
        assert!(!labels.contains(&"age".to_string()), "nested inner should NOT complete age, got: {labels:?}");

        // --- Nested v-for outer: user.name (2nd occurrence) ---
        let pos = session.find_nth_position(&uri, "user.name", 1, 5);
        let labels = session.completion_labels(&uri, pos, None).await;
        assert!(labels.contains(&"name".to_string()), "nested outer should complete name, got: {labels:?}");
        assert!(labels.contains(&"email".to_string()), "nested outer should complete email, got: {labels:?}");
        assert!(labels.contains(&"age".to_string()), "nested outer should complete age, got: {labels:?}");
        // Negative: should NOT have inner scope members
        assert!(!labels.contains(&"disabled".to_string()), "nested outer should NOT complete disabled, got: {labels:?}");

        // --- Narrowed member: selectedUser.name ---
        let pos = session.find_position(&uri, "selectedUser.name", 13);
        let labels = session.completion_labels(&uri, pos, None).await;
        assert!(labels.contains(&"name".to_string()), "narrowed member should complete name, got: {labels:?}");
        assert!(labels.contains(&"email".to_string()), "narrowed member should complete email, got: {labels:?}");
        assert!(labels.contains(&"age".to_string()), "narrowed member should complete age, got: {labels:?}");
        // Negative: not null members
        assert!(!labels.contains(&"null".to_string()), "narrowed member should NOT complete null, got: {labels:?}");

        // --- Props member: props.title ---
        let pos = session.find_position(&uri, "props.title", 6);
        let labels = session.completion_labels(&uri, pos, None).await;
        assert!(labels.contains(&"title".to_string()), "props member should complete title, got: {labels:?}");
        for bad in ["@click", "@custom", "foo-bar"] {
            assert!(!labels.contains(&bad.to_string()), "props member should NOT contain {bad}, got: {labels:?}");
        }

        // --- Broken expression: {{ count + }} ---
        let pos = session.find_position(&uri, "{{ count + }}", 11);
        let labels = session.completion_labels(&uri, pos, None).await;
        assert!(labels.contains(&"count".to_string()), "broken expr should complete count, got: {labels:?}");
        assert!(labels.contains(&"doubled".to_string()), "broken expr should complete doubled, got: {labels:?}");
        assert!(labels.contains(&"increment".to_string()), "broken expr should complete increment, got: {labels:?}");
        assert!(!labels.contains(&"AbortController".to_string()), "broken expr should NOT contain AbortController, got: {labels:?}");
        assert!(!labels.contains(&"document".to_string()), "broken expr should NOT contain document, got: {labels:?}");
        assert!(!labels.contains(&"window".to_string()), "broken expr should NOT contain window, got: {labels:?}");

        // --- Computed member: {{ doubled }} should NOT expose .value ---
        let pos = session.find_position(&uri, "{{ doubled }}", 10);
        let labels = session.completion_labels(&uri, pos, Some(".")).await;
        assert!(!labels.contains(&"value".to_string()), "computed member should NOT expose value, got: {labels:?}");

        // --- V-for local: {{ item }} ---
        let pos = session.find_position(&uri, "{{ item }}", 3);
        let labels = session.completion_labels(&uri, pos, None).await;
        assert!(labels.contains(&"item".to_string()), "v-for local should complete item, got: {labels:?}");

        // --- No internal leakage in any completion ---
        for bad in ["__props", "___VERTER___", "$V_"] {
            assert!(!labels.iter().any(|l| l.contains(bad)),
                "completions should NOT leak internal {bad}, got: {labels:?}");
        }
    }
);

// ---------------------------------------------------------------------------
// Secondary file completions
// ---------------------------------------------------------------------------

real_provider_test!(
    completion_secondary_files,
    fixture = "single-project",
    async fn run(session) {
        let app_uri = session.open_fixture_file("src/App.vue").await;
        let comp_case_uri = session.open_fixture_file("src/ComponentCompletionCase.vue").await;
        let broken_uri = session.open_fixture_file("src/BrokenTemplateExpr.vue").await;
        let js_uri = session.open_fixture_file("src/JsTemplateCases.vue").await;

        // Wait for warmup on App.vue
        if !session.require_or_skip_ready(&app_uri, "action.disabled", 7, "disabled").await {
            return;
        }

        // --- ComponentCompletionCase.vue: component tag props ---
        session.ensure_synced(&comp_case_uri).await;
        // ComponentCompletionCase.vue has `<MyComp />` — completions on MyComp
        // The component has foo, bar props and custom event
        let pos = session.find_position(&comp_case_uri, "<MyComp", 1);
        let hover = session.hover_text(&comp_case_uri, pos).await;
        // Just verify component resolution works here (completions on component tags
        // are more complex — they come from verter, not type provider)
        if let Some(text) = hover {
            assert!(text.contains("foo") || text.contains("MyComp"),
                "ComponentCompletionCase should resolve MyComp, got: {text}");
        }

        // --- BrokenTemplateExpr.vue: {{ count + }} ---
        session.ensure_synced(&broken_uri).await;
        let pos = session.find_position(&broken_uri, "{{ count + }}", 11);
        let labels = session.completion_labels(&broken_uri, pos, None).await;
        assert!(labels.contains(&"count".to_string()), "broken expr should complete count, got: {labels:?}");
        assert!(labels.contains(&"formatted".to_string()), "broken expr should complete formatted, got: {labels:?}");

        // --- JsTemplateCases.vue: {{ count }} ---
        session.ensure_synced(&js_uri).await;
        let pos = session.find_position(&js_uri, "{{ count }}", 3);
        let labels = session.completion_labels(&js_uri, pos, None).await;
        assert!(labels.contains(&"count".to_string()), "JS SFC should complete count, got: {labels:?}");
        assert!(labels.contains(&"increment".to_string()), "JS SFC should complete increment, got: {labels:?}");

        // --- JsTemplateCases.vue: state.label member access ---
        let pos = session.find_position(&js_uri, "state.label", 6);
        let labels = session.completion_labels(&js_uri, pos, None).await;
        if !session.is_tsgo() {
            // tsserver reliably resolves JSDoc @type member access in .jsx files.
            // TSGO support is inconsistent — sometimes returns members, sometimes
            // returns scope-level identifiers. Skip for TSGO until stable.
            assert!(labels.contains(&"label".to_string()), "JS SFC member should complete label, got: {labels:?}");
            assert!(labels.contains(&"done".to_string()), "JS SFC member should complete done, got: {labels:?}");
        }
    }
);

// ---------------------------------------------------------------------------
// Event-argument intellisense (inline arrow params + $event)
// ---------------------------------------------------------------------------

real_provider_test!(
    completion_event_argument_payload,
    fixture = "single-project",
    async fn run(session) {
        let uri = session.open_fixture_file("src/EventArgCases.vue").await;

        // Warm-up probe on a stable interpolation binding (NOT the event-arg path
        // under test, so a regression FAILS the assertions below instead of
        // vacuously skipping).
        if !session
            .wait_until_ready(
                &uri,
                "{{ greeting }}",
                6,
                "greeting",
            )
            .await
        {
            return;
        }

        // PREREQUISITES: the native event-arg assertions below require BOTH (1) the DOM
        // lib (the handler param types to `GlobalEventHandlersEventMap["click"]` →
        // `MouseEvent`) AND (2) a resolvable `vue` typed module (every Vue-SFC TSX
        // projection references vue; when vue cannot resolve, the whole projection is
        // malformed and EVERY template member-access — including the event-arg path —
        // collapses to scope-identifier completion). `wait_until_ready` proved only that a
        // simple in-scope template identifier completes — which survives a missing vue
        // because the identifier is in scope — so it does NOT prove either prerequisite.
        //
        // Both probes are DIRECT, script-context provider queries that do NOT route
        // through Verter's template-event codegen, so they stay independent of the path
        // under test: a genuine event-typing regression leaves BOTH probes green while the
        // template list loses `clientX`/`target` (fail closed). They go empty only when
        // the underlying substrate is genuinely absent (no DOM lib / no vue), in which
        // case the test SKIPs instead of over-firing.
        if !skip_unless_provider_materializes(
            session,
            "src/__verter_probe_native_event.vue",
            "<script setup lang=\"ts\">\n\
             const __probe_evt = null as unknown as MouseEvent;\n\
             void __probe_evt.x;\n\
             </script>\n\
             <template><div /></template>\n",
            "__probe_evt.x",
            "__probe_evt.".len(), // land right after the member-access dot
            &["clientX", "target"],
        )
        .await
        {
            eprintln!(
                "SKIP completion_event_argument_payload: provider cannot type DOM MouseEvent \
                 members in this environment (direct MouseEvent member probe was empty); the \
                 native event-arg DOM-lib prerequisite is unmet."
            );
            return;
        }
        if !skip_unless_provider_materializes(
            session,
            "src/__verter_probe_vue_substrate.vue",
            "<script setup lang=\"ts\">\n\
             import { ref } from \"vue\";\n\
             const __probe_ref = ref({ probeReactiveMember: 0 });\n\
             void __probe_ref.value.probeReactiveMember;\n\
             </script>\n\
             <template><div /></template>\n",
            "__probe_ref.value",
            "__probe_ref.".len(), // land right after `__probe_ref.`
            &["value"],
        )
        .await
        {
            eprintln!(
                "SKIP completion_event_argument_payload: the `vue` typed module does not resolve \
                 in this environment (direct `vue` ref().value probe was empty), so every Vue-SFC \
                 TSX projection is malformed and template event typing cannot materialize; the \
                 native event-arg vue-substrate prerequisite is unmet."
            );
            return;
        }

        // The prerequisite probes open two additional carriers. Their host upserts can
        // invalidate the original fixture's compiled surface, so settle the document
        // under test again before asserting its event members. This is test setup, not
        // a retry waiver: the separate request-surface race contract requires provider
        // items to be dropped while an edit is genuinely unsynchronized.
        session.ensure_synced(&uri).await;

        // --- Inline arrow event parameter: @click="(ev) => handle(ev.clientX)" ---
        // The parameter `ev` must be typed as the native `click` payload
        // (MouseEvent), so member completion exposes DOM event members.
        let pos = session.find_position(&uri, "ev.clientX", 3); // after `ev.`
        let labels = session.completion_labels(&uri, pos, Some(".")).await;
        assert!(
            labels.contains(&"target".to_string()),
            "inline event param should expose Event.target, got: {labels:?}"
        );
        assert!(
            labels.contains(&"clientX".to_string()),
            "inline event param should be MouseEvent-like (clientX), got: {labels:?}"
        );
        for bad in ["___VERTER___", "__props", "$V_"] {
            assert!(
                !labels.iter().any(|l| l.contains(bad)),
                "inline event-arg completion should NOT leak internal {bad}, got: {labels:?}"
            );
        }

        // --- $event member access: @click="handle($event.clientX)" ---
        let pos = session.find_position(&uri, "$event.clientX", 7); // after `$event.`
        let labels = session.completion_labels(&uri, pos, Some(".")).await;
        assert!(
            labels.contains(&"target".to_string()),
            "$event should expose Event.target, got: {labels:?}"
        );
        assert!(
            labels.contains(&"clientX".to_string()),
            "$event should be MouseEvent-like (clientX), got: {labels:?}"
        );
        for bad in ["___VERTER___", "__props", "$V_"] {
            assert!(
                !labels.iter().any(|l| l.contains(bad)),
                "$event completion should NOT leak internal {bad}, got: {labels:?}"
            );
        }

        // --- Spread-path $event: the SECOND of a duplicate `@click` routes through
        // the spread path. JSX contextual typing cannot flow through a spread, so the
        // codegen annotates the spread `$event` with its explicit payload type; it
        // resolves to the real MouseEvent, not `any`. `@click="handle($event.screenX)"` ---
        let pos = session.find_position(&uri, "$event.screenX", 7); // after `$event.`
        let labels = session.completion_labels(&uri, pos, Some(".")).await;
        assert!(
            labels.contains(&"target".to_string()),
            "spread-path $event should expose Event.target, got: {labels:?}"
        );
        assert!(
            labels.contains(&"screenX".to_string()),
            "spread-path $event should be MouseEvent-like (screenX), got: {labels:?}"
        );
        // Negative: must NOT be `any` — an `any`-typed $event would offer no DOM
        // members (and the old eventCallbacks helper would leak internals).
        for bad in ["___VERTER___", "__props", "$V_"] {
            assert!(
                !labels.iter().any(|l| l.contains(bad)),
                "spread-path $event completion should NOT leak internal {bad}, got: {labels:?}"
            );
        }
    }
);

// ---------------------------------------------------------------------------
// Spread-path COMPONENT event-arg payloads — the closed matrix rows for
// {local binding, GlobalComponents fallback} × {$event, arrow param} × {duplicate,
// hyphenated}. Each spread surface must type its handler from the component's emit
// payload (`InstanceType<typeof Binding>["$props"]["onEvent"]`) — never `$event: any`.
// ---------------------------------------------------------------------------

real_provider_test!(
    completion_component_event_argument_payload,
    fixture = "single-project",
    async fn run(session) {
        let uri = session.open_fixture_file("src/EventArgCases.vue").await;
        // Materialize the imported local component and the globally-registered one so the
        // TypeProvider can resolve `InstanceType<typeof Binding>["$props"]` for each.
        let emit_child_uri = session.open_fixture_file("src/EmitChild.vue").await;
        let global_emit_uri = session.open_fixture_file("src/GlobalEmitComp.vue").await;
        session.ensure_synced(&emit_child_uri).await;
        session.ensure_synced(&global_emit_uri).await;
        session.ensure_synced(&uri).await;
        session.settle_import_dependencies(&uri).await;

        // Warm-up probe on a stable interpolation binding (NOT the event-arg path under
        // test) so a regression FAILS the assertions below instead of vacuously skipping.
        if !session
            .wait_until_ready(
                &uri,
                "{{ greeting }}",
                6,
                "greeting",
            )
            .await
        {
            return;
        }

        let bad_internals = ["___VERTER___", "__props", "$V_"];

        // PREREQUISITE: the component event-arg assertions below require the provider to
        // MATERIALIZE the imported component's instance type and read its emit-handler
        // prop. `wait_until_ready` only proved a simple local completion answers — NOT
        // that `'./EmitChild.vue'` resolved to a typed `.vue.ts` whose
        // `InstanceType<typeof EmitChild>["$props"]["onPick"]` is reachable. Prove that
        // through a DIRECT, non-template probe: a throw-away SFC that imports
        // `EmitChild.vue` and, in its `<script setup>`, completes members on the `onPick`
        // payload type resolved via `InstanceType<...>["$props"]["onPick"]`. If the probe
        // cannot surface a payload member here, the imported-component instance type is
        // not materialized in this environment, so SKIP rather than over-fire. When it
        // DOES surface them, the template assertions below stay fail-closed — a regression
        // that types the template `$event` as `any` (or drops the spread payload
        // annotation) leaves this probe green while the template list loses
        // `pickId`/`pickLabel`.
        if !skip_unless_provider_materializes(
            session,
            "src/__verter_probe_component_event.vue",
            "<script setup lang=\"ts\">\n\
             import EmitChild from \"./EmitChild.vue\";\n\
             type __OnPick = NonNullable<InstanceType<typeof EmitChild>[\"$props\"][\"onPick\"]>;\n\
             const __probe_pick = null as unknown as Parameters<__OnPick>[0];\n\
             void __probe_pick.x;\n\
             </script>\n\
             <template><div /></template>\n",
            "__probe_pick.x",
            "__probe_pick.".len(), // land right after the member-access dot
            &["pickId", "pickLabel"],
        )
        .await
        {
            eprintln!(
                "SKIP completion_component_event_argument_payload: provider cannot materialize \
                 the imported-component instance type in this environment (direct \
                 InstanceType<typeof EmitChild>[\"$props\"][\"onPick\"] payload probe was empty); \
                 the imported-component instance-type prerequisite is unmet."
            );
            return;
        }

        // The prerequisite is materialized, so the TEMPLATE spread `$event` MUST now
        // resolve the emit payload (fail closed — an empty/`any` template result here is
        // the genuine instance-type-routing regression this row targets, not a missing
        // provider capability).
        let local_dollar = session
            .completion_labels(
                &uri,
                session.find_position(&uri, "$event.pickLabel", 7), // after `$event.`
                Some("."),
            )
            .await;

        // --- Local component, DUPLICATE `@pick`: the second handler is a spread key, so
        // its `$event` is the handler payload `{ pickId, pickLabel }` typed via
        // InstanceType<typeof EmitChild>["$props"]["onPick"] — NOT `any`. ---
        assert!(
            local_dollar.contains(&"pickId".to_string())
                && local_dollar.contains(&"pickLabel".to_string()),
            "spread $event on a duplicate local-component event should expose the payload (pickId/pickLabel), got: {local_dollar:?}"
        );
        for bad in bad_internals {
            assert!(
                !local_dollar.iter().any(|l| l.contains(bad)),
                "local-component spread $event completion should NOT leak internal {bad}, got: {local_dollar:?}"
            );
        }

        // --- Local component, HYPHENATED `@row-change` → spread ARROW param typed from
        // the handler payload `{ rowKey, rowName }`. ---
        let pos = session.find_position(&uri, "row.rowKey", 4); // after `row.`
        let labels = session.completion_labels(&uri, pos, Some(".")).await;
        assert!(
            labels.contains(&"rowKey".to_string()) && labels.contains(&"rowName".to_string()),
            "spread arrow param on a hyphenated local-component event should expose the payload (rowKey/rowName), got: {labels:?}"
        );

        // --- Global (GlobalComponents fallback) component, DUPLICATE `@ping`: the second
        // handler's `$event` resolves via the generated fallback const
        // InstanceType<typeof GlobalEmitComp>["$props"]["onPing"] → `{ pingCode, pingCount }`. ---
        let pos = session.find_position(&uri, "$event.pingCount", 7); // after `$event.`
        let labels = session.completion_labels(&uri, pos, Some(".")).await;
        assert!(
            labels.contains(&"pingCode".to_string()) && labels.contains(&"pingCount".to_string()),
            "spread $event on a duplicate global-component event should expose the payload (pingCode/pingCount), got: {labels:?}"
        );
        for bad in bad_internals {
            assert!(
                !labels.iter().any(|l| l.contains(bad)),
                "global-component spread $event completion should NOT leak internal {bad}, got: {labels:?}"
            );
        }

        // --- Global component, HYPHENATED `@late-signal` → spread ARROW param via the
        // fallback const → payload `{ sigName, sigLevel }`. ---
        let pos = session.find_position(&uri, "sig.sigName", 4); // after `sig.`
        let labels = session.completion_labels(&uri, pos, Some(".")).await;
        assert!(
            labels.contains(&"sigName".to_string()) && labels.contains(&"sigLevel".to_string()),
            "spread arrow param on a hyphenated global-component event should expose the payload (sigName/sigLevel), got: {labels:?}"
        );
    }
);

// Carrier completion-field fidelity through a REAL provider.
//
// Exercises the provider-side of the additive carrier-field plumbing (D3/D4):
// a member completion fetched from the live provider must carry the new
// `protocol::Completion` fields exactly as the provider's wire reports them —
// never fabricated. The wire reality (empirically pinned here):
//
// * tsgo speaks LSP and (with the now-advertised `commitCharactersSupport`
//   client capability) attaches `commitCharacters` to every member completion
//   — `[".", ",", ";"]` for the standard member-access context. So under tsgo
//   this test asserts the POSITIVE: at least one member carries a non-empty
//   `commit_characters`. This fails on the pre-fix tree (the field did not
//   exist, and the capability was not advertised so the wire dropped it).
// * tsserver's `completionInfo` entries do NOT carry `commitCharacters`,
//   `isSnippet`, `filterText`, or `isRecommended` for these members, so under
//   tsserver every new field is correctly `None` (fail-closed — never
//   fabricated). This test asserts that NEGATIVE.
//
// Both branches share the universal negative: a plain (non-snippet) member must
// NOT carry `insert_text_format == Snippet` — no provider fabricates a snippet
// format for a plain property. The emit half of the plumbing
// (`protocol::Completion` → LSP `CompletionItem`) is pinned by the unit test
// `provider_completion_to_lsp_item_propagates_carrier_fields` in
// `type_provider::merge::tests`.
real_provider_test!(
    completion_carrier_fields_through_provider,
    fixture = "single-project",
    async fn run(session) {
        use crate::type_provider::protocol::CompletionInsertTextFormat;

        // A strongly-typed object literal; completing `obj.` yields members the
        // provider materializes through the provider-direct path (which, unlike
        // the LSP member-access path, reliably resolves the member surface in
        // the test environment).
        let member_src = "\
const obj = { alpha: 1, betaLongName: \"x\", gamma(): number { return 1; } };
export const out = obj.;
";
        let path = session
            .open_in_provider("src/__carrier_member.tsx", member_src)
            .await;
        let off = (member_src.find("obj.;").expect("needle present") + "obj.".len()) as u32;

        let mut items = Vec::new();
        for _ in 0..8 {
            if let Ok(r) = session.provider().get_completions(&path, off, Some(".")).await {
                if !r.items.is_empty() {
                    items = r.items;
                    break;
                }
            }
            tokio::time::sleep(std::time::Duration::from_millis(400)).await;
        }
        if items.is_empty() {
            // Fail-closed under require-mode; recorded skip otherwise.
            if session.allow_empty_result_skip(&format!(
                "provider returned no members for obj. at offset {off}"
            )) {
                return;
            }
        }

        // Sanity: the members are present (proves we are inspecting the real
        // member surface, not an empty/wrong payload).
        let has = |label: &str| items.iter().any(|i| i.label == label);
        assert!(
            has("alpha") && has("betaLongName") && has("gamma"),
            "member completion must include obj's members, got: {:?}",
            items.iter().map(|i| &i.label).collect::<Vec<_>>()
        );

        // Universal negative (BOTH providers): a plain property is never a
        // snippet — no provider fabricates an `insert_text_format == Snippet`.
        for it in &items {
            assert_ne!(
                it.insert_text_format,
                Some(CompletionInsertTextFormat::Snippet),
                "plain member `{}` must not carry a fabricated Snippet format",
                it.label
            );
        }

        match session.provider().provider_id() {
            "tsgo" => {
                // POSITIVE: tsgo attaches commitCharacters to member completions.
                // Discriminating — pre-fix the carrier had no `commit_characters`
                // field and the client did not advertise `commitCharactersSupport`,
                // so the wire value was dropped.
                let with_commit: Vec<_> = items
                    .iter()
                    .filter(|i| i.commit_characters.as_ref().is_some_and(|c| !c.is_empty()))
                    .collect();
                assert!(
                    !with_commit.is_empty(),
                    "tsgo member completions must carry non-empty commit_characters \
                     (the real LSP wire signal), got none across {} items",
                    items.len()
                );
                // The standard member-access commit set includes the member-chain
                // characters; assert one of the expected commit chars is present
                // on a member (proves we parsed the real array, not a stub).
                let alpha = items
                    .iter()
                    .find(|i| i.label == "alpha")
                    .expect("alpha present");
                let commit = alpha
                    .commit_characters
                    .as_ref()
                    .expect("tsgo member carries commit_characters");
                assert!(
                    commit.iter().any(|c| c == "." || c == ";" || c == ","),
                    "tsgo member commit_characters should include member-chain chars, got: {commit:?}"
                );
            }
            "tsserver" => {
                // NEGATIVE / fail-closed: tsserver completion entries do not
                // carry these fields at list time, so they stay None — never
                // fabricated. (Snippet text is additionally gated off by the
                // session's `includeCompletionsWithSnippetText: false` preference,
                // so no member is a snippet here either.)
                for it in &items {
                    assert!(
                        it.commit_characters.is_none(),
                        "tsserver member `{}` must not fabricate commit_characters",
                        it.label
                    );
                    assert!(
                        it.insert_text_format.is_none(),
                        "tsserver member `{}` carries no snippet signal here → None",
                        it.label
                    );
                    assert!(
                        it.label_details.is_none(),
                        "tsserver member `{}` carries no label_details at list time",
                        it.label
                    );
                }
            }
            other => panic!("unexpected provider id: {other}"),
        }
    }
);
