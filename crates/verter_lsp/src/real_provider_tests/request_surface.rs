//! Request-surface consistency with a REAL provider: a completion racing an
//! edit must have a DETERMINISTIC, never-torn outcome.
//!
//! The mid-request interleaving itself is exercised deterministically at the
//! mock level (the `on_query` seam in `server_tests`); with a live engine no
//! seam can be injected between the handler's capture and the provider await,
//! so this test PINS the state that interleaving produces — the live document
//! advanced while the provider still holds the previously-synced surface — and
//! asserts the outcome contract that must hold under EVERY interleaving:
//!
//! - while the edit is un-synced, the response carries NO provider-derived
//!   item (fail closed — merging provider items computed against the stale
//!   surface and anchoring them through the fresh mapper would be torn);
//! - after the sync lands, provider-derived items are served again (the
//!   fail-closed gate is not over-eager).

use tower_lsp_server::ls_types::{
    CompletionContext, CompletionItem, CompletionParams, CompletionResponse, CompletionTriggerKind,
    PartialResultParams, Position, TextDocumentIdentifier, TextDocumentPositionParams, Uri,
    WorkDoneProgressParams,
};
use tower_lsp_server::LanguageServer;

use crate::test_harness::{real_provider_test, RealProviderTestSession};

/// Raw completion items at a position (labels alone cannot distinguish a
/// provider-derived item from a Verter-native one).
async fn completion_items(
    session: &RealProviderTestSession,
    uri: &Uri,
    position: Position,
) -> Vec<CompletionItem> {
    let params = CompletionParams {
        text_document_position: TextDocumentPositionParams {
            text_document: TextDocumentIdentifier { uri: uri.clone() },
            position,
        },
        work_done_progress_params: WorkDoneProgressParams::default(),
        partial_result_params: PartialResultParams::default(),
        context: Some(CompletionContext {
            trigger_kind: CompletionTriggerKind::INVOKED,
            trigger_character: None,
        }),
    };
    match session.server().completion(params).await {
        Ok(Some(CompletionResponse::Array(items))) => items,
        Ok(Some(CompletionResponse::List(list))) => list.items,
        Ok(None) => Vec::new(),
        Err(e) => {
            eprintln!("completion error: {e}");
            Vec::new()
        }
    }
}

/// Whether an item was minted from the type provider's response (it carries
/// the provider-neutral `verter_resolve` envelope the merge attaches to every
/// provider completion). Verter-native items never carry it.
fn is_provider_item(item: &CompletionItem) -> bool {
    item.data
        .as_ref()
        .and_then(|d| d.get("verter_resolve"))
        .is_some()
}

real_provider_test!(
    completion_racing_an_edit_never_serves_a_torn_provider_result,
    fixture = "single-project",
    async fn run(session) {
        let uri = session.open_fixture_file("src/App.vue").await;

        // Wait for provider warmup.
        if !session.require_or_skip_ready(&uri, "action.disabled", 7, "disabled").await {
            return;
        }

        // Baseline (stable captured surface): the identifier completion inside
        // `{{ count }}` serves provider-derived items.
        let pos = session.find_position(&uri, "{{ count }}", 3);
        let baseline = completion_items(session, &uri, pos).await;
        if !baseline.iter().any(is_provider_item)
            && session.allow_empty_result_skip(
                "the provider surfaced no items at the probe position, so the \
                 torn-vs-dropped distinction below would be vacuous",
            )
        {
            // Off require-mode only: the degradation is RECORDED on the session,
            // so the end-of-body receipt reports SKIPPED-WARMUP rather than
            // attesting assertions that never ran. Under require-mode the helper
            // panics — a controlled fixture that surfaces nothing is a genuine
            // provider/materialization regression.
            return;
        }

        // Create the stale-provider window directly: the edit lands in the
        // document registry only, so source, artifacts, and mapper advance
        // while the provider still holds the previous surface. Inserting above
        // the probe shifts every mapping below it.
        let server = session.server();
        let edited = {
            let doc = server
                .test_documents()
                .get(&uri)
                .expect("document is open");
            doc.source
                .replace("<template>", "<template>\n  <p>inserted-line</p>")
        };
        assert!(
            edited.contains("inserted-line"),
            "fixture must contain a <template> tag for the edit"
        );
        let _ = server.test_documents().did_change(&uri, 9001, &edited);

        // The probe position in the EDITED document (one line down).
        let pos_after_edit = session.find_position(&uri, "{{ count }}", 3);
        let raced = completion_items(session, &uri, pos_after_edit).await;

        let provider_labels: Vec<String> = raced
            .iter()
            .filter(|i| is_provider_item(i))
            .map(|i| i.label.clone())
            .collect();

        // The contract is a DISJUNCTION, and asserting only one of its arms is
        // what made this test flip under load: the repair needs a provider
        // round-trip, and a round-trip that does not complete within its bound
        // legitimately leaves the surface un-repaired. Both outcomes are
        // correct; the forbidden one is provider items served against a surface
        // that does not carry the edit — items computed on the stale text and
        // then anchored through the fresh mapper. That is the tear, and it is
        // exactly what the un-repaired arm below rules out.
        let repaired = session
            .server()
            .test_type_provider_context(&uri)
            .is_some_and(|ctx| ctx.tsx_content.contains("inserted-line"));

        if repaired {
            // The surface carries the edit, so the engine can answer from it.
            // Whether it has finished TYPING it is the engine's business and no
            // applied-generation receipt exists to wait on, so an empty first
            // answer is absorbed by an independent readiness probe and ONE
            // further call — never a retry loop around the assertion.
            let mut labels = provider_labels;
            if labels.is_empty() {
                let ctx = session
                    .server()
                    .test_type_provider_context(&uri)
                    .expect("the repaired provider context is present");
                let ready = tokio::time::timeout(std::time::Duration::from_secs(30), async {
                    loop {
                        let typed = session
                            .provider()
                            .get_completions(&ctx.tsx_path, 0, None)
                            .await
                            .is_ok_and(|result| !result.items.is_empty());
                        if typed {
                            return;
                        }
                        tokio::time::sleep(std::time::Duration::from_millis(100)).await;
                    }
                })
                .await;
                assert!(
                    ready.is_ok(),
                    "the external engine never typed the repaired carrier surface \
                     within the 30s watchdog"
                );
                labels = completion_items(session, &uri, pos_after_edit)
                    .await
                    .iter()
                    .filter(|i| is_provider_item(i))
                    .map(|i| i.label.clone())
                    .collect();
            }
            assert!(
                !labels.is_empty(),
                "the repaired surface carries the edit, so completion must serve \
                 provider-derived items from that generation"
            );
        } else {
            assert!(
                provider_labels.is_empty(),
                "the provider still holds the pre-edit surface, so every \
                 provider-derived item here was computed against stale text and \
                 anchored through the fresh mapper — the exact tear this test \
                 exists to forbid; got {provider_labels:?}"
            );
        }
    }
);
