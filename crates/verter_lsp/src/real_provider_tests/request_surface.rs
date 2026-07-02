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
//!   item (fail closed; the pre-fix behaviour merged provider items computed
//!   against the stale surface and anchored them through the fresh mapper);
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
        if !session.wait_until_ready(&uri, "action.disabled", 7, "disabled").await {
            eprintln!("skipping: provider not warmed up");
            return;
        }

        // Baseline (stable captured surface): the identifier completion inside
        // `{{ count }}` serves provider-derived items.
        let pos = session.find_position(&uri, "{{ count }}", 3);
        let baseline = completion_items(session, &uri, pos).await;
        if !baseline.iter().any(is_provider_item) {
            // The provider surfaced nothing at this position in this
            // environment — the torn-vs-dropped distinction below would be
            // vacuous, so skip rather than over-fire.
            eprintln!("skipping: provider surfaced no items at the probe position");
            return;
        }

        // Pin the torn window open: the edit lands in the DOCUMENT REGISTRY
        // ONLY (live source, artifacts, and mapper all advance) while the
        // provider still holds the previously-synced surface — no re-sync is
        // driven. Inserting ABOVE the probe position shifts every mapping
        // below it, so a provider response computed against the stale surface
        // could only be anchored WRONGLY through the fresh state.
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
        let provider_labels: Vec<&str> = raced
            .iter()
            .filter(|i| is_provider_item(i))
            .map(|i| i.label.as_str())
            .collect();
        assert!(
            provider_labels.is_empty(),
            "a completion issued while the edit is un-synced must NOT serve \
             provider-derived items (the provider still holds the previous surface; \
             mapping its response through the fresh state would be torn), got \
             provider items: {provider_labels:?}"
        );

        // Recovery (the gate is not over-eager): drive the sync, then the SAME
        // position serves provider-derived items again.
        session.ensure_synced(&uri).await;
        let mut recovered = false;
        for attempt in 0..5 {
            let items = completion_items(session, &uri, pos_after_edit).await;
            if items.iter().any(is_provider_item) {
                recovered = true;
                break;
            }
            if attempt < 4 {
                session.ensure_synced(&uri).await;
                tokio::time::sleep(std::time::Duration::from_millis(300)).await;
            }
        }
        assert!(
            recovered,
            "after the sync lands, provider-derived completions must be served again \
             (the fail-closed gate must not drop healthy results)"
        );
    }
);
