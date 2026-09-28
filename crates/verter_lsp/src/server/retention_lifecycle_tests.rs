//! Request-lifetime retention, proved through the REAL LSP request handlers.
//!
//! The store's own unit tests prove that `ProviderSurfaceStore` releases what it
//! owns. That is a necessary property and not a sufficient one: the question a
//! long editing session asks is whether the SERVER — the handler, the context it
//! builds, the capture a request pins — still holds a document version after the
//! request that needed it is over. Only the real handlers can answer it, so
//! these drive `<VerterLanguageServer as LanguageServer>::semantic_tokens_full`
//! and `::goto_definition` and observe what survives.
//!
//! The instrument is a WEAK handle per retained generation. A weak handle
//! observes an allocation without owning it, so "is this document version still
//! alive anywhere in the process?" gets a yes/no answer rather than a byte total
//! that another participant's activity could move.

use std::sync::Arc;

use tower_lsp_server::ls_types::{
    GotoDefinitionParams, PartialResultParams, Position, SemanticTokensParams,
    TextDocumentIdentifier, TextDocumentPositionParams, Uri, WorkDoneProgressParams,
};
use tower_lsp_server::LanguageServer;

use crate::provider_surface_store::ProviderSurfaceSnapshot;
use crate::server::nav_features_navigation::nav_features_rename_completeness_tests::fixture_with_provider;
use crate::server::VerterLanguageServer;
use crate::type_provider::mock::{MockCall, MockTypeProvider};

/// A carrier with enough template and script to make one retained version
/// measurable, and a `msg` binding a definition request can land on.
fn source(revision: usize) -> String {
    let mut lines = vec!["<script setup lang=\"ts\">".to_string()];
    lines.push(format!("const msg = 'hello {revision:04}'"));
    for index in 0..24 {
        lines.push(format!("const field{index} = `{{msg}}:{index}`"));
    }
    lines.push("</script>".to_string());
    lines.push("<template><div>{{ msg }}</div></template>".to_string());
    lines.push(String::new());
    lines.join("\n")
}

fn semantic_tokens_params(uri: &Uri) -> SemanticTokensParams {
    SemanticTokensParams {
        work_done_progress_params: WorkDoneProgressParams::default(),
        partial_result_params: PartialResultParams::default(),
        text_document: TextDocumentIdentifier { uri: uri.clone() },
    }
}

/// Weak handles to every generation the store owns right now, so a later probe
/// can ask whether any of them outlived the request that captured it.
fn weak_generations(
    server: &VerterLanguageServer,
) -> Vec<(Arc<str>, std::sync::Weak<ProviderSurfaceSnapshot>)> {
    server
        .documents
        .provider_surfaces()
        .weak_handles_to_retained_surfaces()
}

fn still_alive(handles: &[(Arc<str>, std::sync::Weak<ProviderSurfaceSnapshot>)]) -> Vec<&str> {
    handles
        .iter()
        .filter(|(_, weak)| weak.upgrade().is_some())
        .map(|(path, _)| &**path)
        .collect()
}

/// WSP6-AC3: repeated token requests across document revisions do not retain
/// every old document version.
///
/// Semantic tokens are a render-cadence decoration: the editor re-asks on every
/// scroll, cursor move and edit. Each request builds a token BASE — the provider
/// surface, both UTF-16 line indexes and the position mapper it merges through —
/// from the current surface. Two hundred of those across two hundred revisions
/// must leave the session holding ONE surface per live path, not two hundred.
///
/// This drives the real `semantic_tokens_full` entry point, against a provider
/// that returns real tokens, so the retention under test is the one in the
/// handler and context lifecycle rather than a store call a test makes itself.
///
/// Discriminating: the per-revision generations are captured as weak handles
/// BEFORE the loop continues past them; a handler or store that kept any of them
/// leaves a live handle behind and the final assertion names it.
#[tokio::test(flavor = "multi_thread")]
async fn repeated_token_requests_retain_one_document_version_not_every_version() {
    const REVISIONS: usize = 200;

    let provider = Arc::new(MockTypeProvider::new());
    let fixture = fixture_with_provider(&source(0), Some(provider.clone())).await;
    let server = fixture.server();
    server.ensure_current_file_synced(&fixture.uri).await;

    // The token response is real: the merge runs, so the base it merges through
    // is really built on every request.
    let tsx_path = server
        .type_provider_context(&fixture.uri)
        .expect("the synced carrier must have a provider surface")
        .tsx_path;
    provider.set_semantic_tokens(
        &tsx_path,
        vec![crate::type_provider::protocol::SemanticToken {
            start: 0,
            length: 3,
            token_type: 0,
            token_modifiers: 0,
        }],
    );

    let _ = server
        .semantic_tokens_full(semantic_tokens_params(&fixture.uri))
        .await;
    let after_first = server
        .documents
        .provider_surfaces()
        .retained_surface_count();
    assert!(
        after_first > 0,
        "the first token request must run against a recorded surface"
    );

    // Every generation the session passes through, observed but not owned.
    let mut superseded: Vec<(Arc<str>, std::sync::Weak<ProviderSurfaceSnapshot>)> = Vec::new();

    for revision in 1..REVISIONS {
        superseded.extend(weak_generations(server));
        let changed = source(revision);
        assert!(
            server
                .documents
                .did_change(&fixture.uri, revision as i32 + 1, &changed)
                .changed,
            "each revision must land as a real document change"
        );
        server.ensure_current_file_synced(&fixture.uri).await;
        let _ = server
            .semantic_tokens_full(semantic_tokens_params(&fixture.uri))
            .await;
    }

    assert_eq!(
        server
            .documents
            .provider_surfaces()
            .retained_surface_count(),
        after_first,
        "{REVISIONS} token requests across {REVISIONS} revisions must retain what one retains"
    );

    // Every generation the loop moved past is gone — not merely unreferenced by
    // the store's map, but released by everything, handlers and contexts alike.
    let alive = still_alive(&superseded);
    let current: Vec<String> = weak_generations(server)
        .iter()
        .map(|(path, _)| path.to_string())
        .collect();
    let leaked: Vec<&str> = alive
        .iter()
        .copied()
        .filter(|path| !current.iter().any(|live| live == path))
        .collect();
    assert!(
        leaked.is_empty(),
        "superseded document versions are still retained after their token requests \
         completed: {leaked:?}"
    );

    assert!(
        provider
            .calls()
            .iter()
            .any(|call| matches!(call, MockCall::GetSemanticTokens { .. })),
        "the lane must actually have exercised the provider-backed token path"
    );

    fixture.shutdown().await;
}

/// WSP6-AC2: a cancelled inspection cannot pin the project graph.
///
/// A provider-backed navigation request PINS the surfaces it may have to map a
/// returned offset through, captured before the query so a racing background
/// sync cannot retroactively change them. That pin is correct while the request
/// lives — and must not outlive it. LSP cancellation is the request future being
/// dropped, so that is exactly what this does: drive a real `goto_definition` to
/// an observed in-flight point inside the provider call, edit the workspace
/// underneath it so the store moves on from the pinned generations, then drop
/// the request.
///
/// Discriminating: the pinned generations are held as weak handles. Between the
/// re-syncs and the drop they are alive with the cancelled request as their only
/// owner (asserted, so a fixture that pinned nothing cannot vacuously pass);
/// after the drop every one of them must be gone. A request-owned clone
/// surviving its request — a detached task, a cache keyed by the captured set —
/// leaves a live handle and fails the final assertion.
#[tokio::test(flavor = "multi_thread")]
async fn a_cancelled_inspection_releases_every_graph_handle_it_pinned() {
    let provider = Arc::new(MockTypeProvider::new());
    let fixture = fixture_with_provider(&source(0), Some(provider.clone())).await;
    let server = fixture.server();
    server.ensure_current_file_synced(&fixture.uri).await;

    let pinned = weak_generations(server);
    assert!(
        !pinned.is_empty(),
        "the fixture must have recorded at least one surface for the request to pin"
    );

    // The provider never answers: the request stays inside the query, exactly
    // where its capture is live.
    provider.hang_definition();
    let position = Position::new(0, 0);
    let mut inspection = Box::pin(server.goto_definition(GotoDefinitionParams {
        text_document_position_params: TextDocumentPositionParams {
            text_document: TextDocumentIdentifier {
                uri: fixture.uri.clone(),
            },
            position,
        },
        work_done_progress_params: WorkDoneProgressParams::default(),
        partial_result_params: PartialResultParams::default(),
    }));

    let reached_provider = tokio::select! {
        _ = &mut inspection => false,
        _ = provider.wait_until_calls(|calls| {
            calls.iter().any(|call| matches!(call, MockCall::GetDefinition { .. }))
        }) => true,
    };
    assert!(
        reached_provider,
        "the inspection must reach the provider, so its capture is genuinely in flight"
    );

    // The workspace moves on underneath the in-flight request: the store drops
    // the generations it captured, leaving the request as their only owner.
    for revision in 1..6 {
        let changed = source(revision);
        let _ = server
            .documents
            .did_change(&fixture.uri, revision as i32 + 1, &changed);
        // An edit also invalidates the document's diagnostics, which is what the
        // real `did_change` lifecycle does. It matters here because a committed
        // diagnostics RECEIPT retains the surface its publication was proved
        // against, so without it this lane would be asking whether the cancelled
        // request released its pin while a publication still held one.
        server
            .documents
            .invalidate_diagnostics(fixture.uri.as_str());
        server.ensure_current_file_synced(&fixture.uri).await;
    }

    assert!(
        !still_alive(&pinned).is_empty(),
        "the in-flight inspection must still be pinning the generations it captured — \
         otherwise the release below proves nothing"
    );

    // Cancellation: the client withdrew the request, so its future is dropped.
    drop(inspection);
    // Let anything the drop woke settle before asking what survived.
    for _ in 0..16 {
        tokio::task::yield_now().await;
    }

    let alive = still_alive(&pinned);
    assert!(
        alive.is_empty(),
        "a cancelled inspection must release every graph handle it pinned, but these \
         generations are still alive: {alive:?}"
    );

    fixture.shutdown().await;
}
