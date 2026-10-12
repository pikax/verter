//! Template condition narrowing through the production IDE route, against
//! BOTH real providers.
//!
//! Every fixture is a complete SFC from `verter_compiler::flow_check::
//! ide_fixtures`. It is opened through the language server exactly as an
//! editor opens it: the ordinary `CompileTarget::IDE` emitter generates the
//! carrier, the provider checks it, and the merged diagnostics are mapped back
//! to the SFC. The mapped error set must equal the fixture's exact
//! `(code, authored span)` set — valid callbacks report nothing, invalid twins
//! report exactly their planted errors, and dropping an essential condition
//! reports exactly the reads it guarded. The script canary proves the provider
//! checked the file before a template result is accepted.

use std::future::Future;
use std::panic::AssertUnwindSafe;

use futures_util::FutureExt;
use tower_lsp_server::ls_types::{
    Diagnostic, DiagnosticSeverity, DidChangeWatchedFilesParams, DidCloseTextDocumentParams,
    FileChangeType, FileEvent, NumberOrString, TextDocumentIdentifier, Uri,
};
use tower_lsp_server::LanguageServer;
use verter_compiler::flow_check::fixtures::Expected;
use verter_compiler::flow_check::ide_fixtures::{self, SfcFixture};
use verter_compiler::flow_check::seam::Span;

use crate::test_harness::{real_provider_test, PublishedWorkspaceFile, RealProviderTestSession};

const FIXTURE: &str = "flow-narrowing";

fn code_of(diagnostic: &Diagnostic) -> u32 {
    match &diagnostic.code {
        Some(NumberOrString::Number(code)) => *code as u32,
        Some(NumberOrString::String(code)) => code.parse().unwrap_or(0),
        None => 0,
    }
}

/// The authored byte span of a merged diagnostic.
fn authored_span(session: &RealProviderTestSession, uri: &Uri, diagnostic: &Diagnostic) -> Span {
    let doc = session
        .server()
        .test_documents()
        .get(uri)
        .expect("the fixture is open");
    let start = doc
        .line_index
        .position_to_offset(&diagnostic.range.start)
        .expect("a mapped diagnostic starts inside the source");
    let end = doc
        .line_index
        .position_to_offset(&diagnostic.range.end)
        .expect("a mapped diagnostic ends inside the source");
    Span::new(start, end)
}

/// One project member for a corpus. Each case still opens its complete source
/// through the real watched-file and editor lifecycle. The file remains on
/// disk until all cases close, so old missing roots cannot accumulate.
struct FixtureSlot {
    relative: String,
    file: PublishedWorkspaceFile,
    change: FileChangeType,
}

impl FixtureSlot {
    fn new(session: &RealProviderTestSession) -> Self {
        let relative = format!(
            "src/Corpus{}.vue",
            if session.is_tsgo() {
                "Tsgo"
            } else {
                "Tsserver"
            }
        );
        let file = session.publish_workspace_file(&relative, "<template />");
        Self {
            relative,
            file,
            change: FileChangeType::CREATED,
        }
    }

    async fn with_fixture<T, Fut>(
        &mut self,
        session: &RealProviderTestSession,
        fixture: &SfcFixture,
        body: impl FnOnce(Uri) -> Fut,
    ) -> T
    where
        Fut: Future<Output = T>,
    {
        self.file.replace_contents(&fixture.source);
        let change = self.change;
        self.change = FileChangeType::CHANGED;
        with_fixture_path(session, &self.relative, change, body).await
    }
}

/// Close the document before the corpus owner can delete its file, including
/// on assertion failure. A failed case must not leave an open missing source.
async fn with_fixture_path<T, Fut>(
    session: &RealProviderTestSession,
    relative: &str,
    change: FileChangeType,
    body: impl FnOnce(Uri) -> Fut,
) -> T
where
    Fut: Future<Output = T>,
{
    session
        .server()
        .did_change_watched_files(DidChangeWatchedFilesParams {
            changes: vec![FileEvent {
                uri: session.workspace_uri(relative),
                typ: change,
            }],
        })
        .await;
    let uri = session.open_fixture_file(relative).await;
    let outcome = AssertUnwindSafe(body(uri.clone())).catch_unwind().await;
    session
        .server()
        .did_close(DidCloseTextDocumentParams {
            text_document: TextDocumentIdentifier { uri },
        })
        .await;
    match outcome {
        Ok(value) => value,
        Err(payload) => std::panic::resume_unwind(payload),
    }
}

/// The mapped error diagnostics of the open `fixture` once the canary proves
/// the provider checked it.
async fn checked_errors(
    session: &RealProviderTestSession,
    fixture: &SfcFixture,
    uri: &Uri,
) -> Vec<Expected> {
    // The exact diagnostic canary below proves this source was checked. A
    // separate completion warmup would build an auto-import answer that this
    // diagnostic contract never observes.
    for _ in 0..30 {
        let diagnostics = session
            .merged_diagnostics_until(uri, |diagnostics| {
                diagnostics.iter().any(|d| code_of(d) == 2322)
            })
            .await;
        let errors: Vec<Expected> = diagnostics
            .iter()
            .filter(|d| d.severity == Some(DiagnosticSeverity::ERROR))
            .map(|d| Expected {
                code: code_of(d),
                authored: authored_span(session, uri, d),
            })
            .collect();
        if errors.contains(&Expected {
            code: 2322,
            authored: fixture.canary,
        }) {
            return errors;
        }
    }
    panic!(
        "[{}] {}: the provider never reported the canary, so the template was not checked",
        session.provider_kind().label(),
        fixture.name
    );
}

/// The reported errors equal the fixture's, with multiplicity.
fn check_exact(fixture: &SfcFixture, reported: &[Expected]) -> Result<(), String> {
    let mut reported = reported.to_vec();
    reported.sort();
    if reported == fixture.expected {
        return Ok(());
    }
    let describe = |items: &[Expected]| -> String {
        items
            .iter()
            .map(|e| {
                format!(
                    "TS{}@{}..{} `{}`",
                    e.code,
                    e.authored.start,
                    e.authored.end,
                    fixture
                        .source
                        .get(e.authored.start as usize..e.authored.end as usize)
                        .unwrap_or("<outside>")
                )
            })
            .collect::<Vec<_>>()
            .join(", ")
    };
    Err(format!(
        "{}: diagnostics differ\n  expected: [{}]\n  reported: [{}]",
        fixture.name,
        describe(&fixture.expected),
        describe(&reported)
    ))
}

/// Open `fixture`, require its exact error set, run `after` against the still
/// open document, and close it.
async fn assert_fixture<Fut>(
    session: &RealProviderTestSession,
    slot: &mut FixtureSlot,
    fixture: &SfcFixture,
    after: impl FnOnce(Uri) -> Fut,
) where
    Fut: Future<Output = ()>,
{
    slot.with_fixture(session, fixture, |uri| async move {
        let reported = checked_errors(session, fixture, &uri).await;
        if let Err(mismatch) = check_exact(fixture, &reported) {
            panic!("[{}] {mismatch}", session.provider_kind().label());
        }
        after(uri).await;
    })
    .await;
}

real_provider_test!(
    flow_narrowing_semantics_are_exact,
    fixture = FIXTURE,
    async fn run(session) {
        let mut slot = FixtureSlot::new(session);
        for fixture in ide_fixtures::semantic_suite() {
            let fixture = &fixture;
            assert_fixture(session, &mut slot, fixture, |uri| async move {
                for hover in &fixture.hovers {
                    let doc_position = {
                        let doc = session
                            .server()
                            .test_documents()
                            .get(&uri)
                            .expect("the fixture is open");
                        doc.line_index
                            .offset_to_position(hover.authored.start)
                            .expect("a hovered identifier lies inside the source")
                    };
                    let mut text = String::new();
                    for _ in 0..20 {
                        if let Some(found) = session.hover_text(&uri, doc_position).await {
                            text = found;
                            break;
                        }
                        tokio::time::sleep(std::time::Duration::from_millis(250)).await;
                    }
                    assert!(
                        text.contains(hover.contains),
                        "[{}] {}: the contextually typed parameter `{}` must hover as `{}`; got {text:?}",
                        session.provider_kind().label(),
                        fixture.name,
                        &fixture.source[hover.authored.start as usize..hover.authored.end as usize],
                        hover.contains,
                    );
                }
            })
            .await;
        }
    }
);

real_provider_test!(
    flow_narrowing_linear_matrix_checks_every_callback,
    fixture = FIXTURE,
    async fn run(session) {
        // Every case retains its complete source and exact diagnostic proof.
        // Reuse one on-disk project member so each content change invalidates
        // that member rather than creating another root and deleting the last.
        let mut slot = FixtureSlot::new(session);
        for &n in &verter_compiler::flow_check::fixtures::LIVE_MATRIX_SIZES {
            for fixture in [
                ide_fixtures::flat_matrix(n, false),
                ide_fixtures::flat_matrix(n, true),
                ide_fixtures::nested_matrix(n, false),
                ide_fixtures::nested_matrix(n, true),
                ide_fixtures::nested_component_matrix(n, false),
                ide_fixtures::nested_component_matrix(n, true),
                ide_fixtures::component_matrix(n, false),
                ide_fixtures::component_matrix(n, true),
            ] {
                assert_fixture(session, &mut slot, &fixture, |_| async {}).await;
                if fixture.name.ends_with("Broken") {
                    // One planted error per callback (plus the canary): every
                    // callback body was type-checked.
                    assert_eq!(
                        fixture.expected.len(),
                        fixture.guarded_callbacks + 1,
                        "{}: one planted error per callback",
                        fixture.name
                    );
                }
            }
        }
    }
);
