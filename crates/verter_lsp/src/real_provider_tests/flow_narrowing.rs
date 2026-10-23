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
    Diagnostic, DiagnosticSeverity, DidChangeTextDocumentParams, DidChangeWatchedFilesParams,
    DidCloseTextDocumentParams, FileChangeType, FileEvent, NumberOrString,
    TextDocumentContentChangeEvent, TextDocumentIdentifier, Uri, VersionedTextDocumentIdentifier,
};
use tower_lsp_server::LanguageServer;
use verter_compiler::flow_check::fixtures::Expected;
use verter_compiler::flow_check::ide_fixtures::{self, SfcFixture};
use verter_compiler::flow_check::seam::Span;

use crate::flow_fixture_tests::semantic_partitions;
use crate::test_harness::{real_provider_test, PublishedWorkspaceFile, RealProviderTestSession};

const FIXTURE: &str = "flow-narrowing";

fn code_of(diagnostic: &Diagnostic) -> u32 {
    match &diagnostic.code {
        Some(NumberOrString::Number(code)) => *code as u32,
        Some(NumberOrString::String(code)) => code.parse().unwrap_or(0),
        None => 0,
    }
}

/// `None` when a stale diagnostic still refers to a longer previous source.
fn try_authored_span(
    session: &RealProviderTestSession,
    uri: &Uri,
    diagnostic: &Diagnostic,
) -> Option<Span> {
    let doc = session.server().test_documents().get(uri)?;
    let start = doc.line_index.position_to_offset(&diagnostic.range.start)?;
    let end = doc.line_index.position_to_offset(&diagnostic.range.end)?;
    Some(Span::new(start, end))
}

/// One project member for a corpus. Each case still opens its complete source
/// through the real watched-file and editor lifecycle. The file remains on
/// disk until all cases close, so old missing roots cannot accumulate.
struct OpenDocument {
    uri: Uri,
    version: i32,
}

struct FixtureSlot {
    relative: String,
    file: PublishedWorkspaceFile,
    change: FileChangeType,
    open: Option<OpenDocument>,
}

impl FixtureSlot {
    fn new(session: &RealProviderTestSession) -> Self {
        let relative = session.scratch_relative("src/Corpus", "vue");
        let file = session.publish_workspace_file(&relative, "<template />");
        Self {
            relative,
            file,
            change: FileChangeType::CREATED,
            open: None,
        }
    }

    async fn close(&mut self, session: &RealProviderTestSession) {
        if let Some(open) = self.open.take() {
            session
                .server()
                .did_close(DidCloseTextDocumentParams {
                    text_document: TextDocumentIdentifier { uri: open.uri },
                })
                .await;
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
        session
            .server()
            .did_change_watched_files(DidChangeWatchedFilesParams {
                changes: vec![FileEvent {
                    uri: session.workspace_uri(&self.relative),
                    typ: change,
                }],
            })
            .await;
        let uri = if let Some(open) = &mut self.open {
            open.version += 1;
            let uri = open.uri.clone();
            let version = open.version;
            session
                .server()
                .did_change(DidChangeTextDocumentParams {
                    text_document: VersionedTextDocumentIdentifier {
                        uri: uri.clone(),
                        version,
                    },
                    content_changes: vec![TextDocumentContentChangeEvent {
                        range: None,
                        range_length: None,
                        text: fixture.source.clone(),
                    }],
                })
                .await;
            uri
        } else {
            let uri = session.open_fixture_file(&self.relative).await;
            self.open = Some(OpenDocument {
                uri: uri.clone(),
                version: 1,
            });
            uri
        };
        let outcome = AssertUnwindSafe(body(uri)).catch_unwind().await;
        if outcome.is_err() {
            // A failed case must not leave an open document whose file is
            // about to be deleted with the slot.
            self.close(session).await;
        }
        match outcome {
            Ok(value) => value,
            Err(payload) => std::panic::resume_unwind(payload),
        }
    }
}

/// The mapped error diagnostics of the open `fixture` once its own canary
/// proves the provider checked this source. A previous case's TS2322 does
/// not count: the canary span is part of this fixture.
async fn checked_errors(
    session: &RealProviderTestSession,
    fixture: &SfcFixture,
    uri: &Uri,
) -> Vec<Expected> {
    session.ensure_synced(uri).await;
    for attempt in 0..100 {
        let diagnostics = session.server().test_merged_diagnostics(uri).await;
        let errors: Vec<Expected> = diagnostics
            .iter()
            .filter(|d| d.severity == Some(DiagnosticSeverity::ERROR))
            .filter_map(|d| {
                Some(Expected {
                    code: code_of(d),
                    authored: try_authored_span(session, uri, d)?,
                })
            })
            .collect();
        if errors.contains(&Expected {
            code: 2322,
            authored: fixture.canary,
        }) {
            return errors;
        }
        if attempt < 99 {
            tokio::time::sleep(std::time::Duration::from_millis(50)).await;
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

async fn assert_semantic_fixture(
    session: &RealProviderTestSession,
    slot: &mut FixtureSlot,
    fixture: &SfcFixture,
) {
    assert_fixture(session, slot, fixture, |uri| async move {
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

async fn run_semantic_partition(session: &RealProviderTestSession, index: usize) {
    let partitions = semantic_partitions();
    let fixtures = partitions
        .get(index)
        .unwrap_or_else(|| panic!("semantic partition {index} is missing"));
    let mut slot = FixtureSlot::new(session);
    for fixture in fixtures {
        assert_semantic_fixture(session, &mut slot, fixture).await;
    }
    slot.close(session).await;
}

macro_rules! narrowing_partition {
    ($name:ident, $index:literal) => {
        real_provider_test!(
            $name,
            fixture = FIXTURE,
            async fn run(session) {
                run_semantic_partition(session, $index).await;
            }
        );
    };
}

narrowing_partition!(flow_narrowing_semantics_are_exact, 0);
narrowing_partition!(flow_narrowing_semantics_partition_1, 1);
narrowing_partition!(flow_narrowing_semantics_partition_2, 2);
narrowing_partition!(flow_narrowing_semantics_partition_3, 3);
narrowing_partition!(flow_narrowing_semantics_partition_4, 4);
narrowing_partition!(flow_narrowing_semantics_partition_5, 5);
narrowing_partition!(flow_narrowing_semantics_partition_6, 6);
narrowing_partition!(flow_narrowing_semantics_partition_7, 7);
narrowing_partition!(flow_narrowing_semantics_partition_8, 8);
narrowing_partition!(flow_narrowing_semantics_partition_9, 9);
narrowing_partition!(flow_narrowing_semantics_partition_10, 10);
narrowing_partition!(flow_narrowing_semantics_partition_11, 11);
narrowing_partition!(flow_narrowing_semantics_partition_12, 12);
narrowing_partition!(flow_narrowing_semantics_partition_13, 13);
narrowing_partition!(flow_narrowing_semantics_partition_14, 14);
narrowing_partition!(flow_narrowing_semantics_partition_15, 15);
narrowing_partition!(flow_narrowing_semantics_partition_16, 16);
narrowing_partition!(flow_narrowing_semantics_partition_17, 17);
narrowing_partition!(flow_narrowing_semantics_partition_18, 18);
narrowing_partition!(flow_narrowing_semantics_partition_19, 19);
narrowing_partition!(flow_narrowing_semantics_partition_20, 20);
narrowing_partition!(flow_narrowing_semantics_partition_21, 21);
narrowing_partition!(flow_narrowing_semantics_partition_22, 22);
narrowing_partition!(flow_narrowing_semantics_partition_23, 23);
narrowing_partition!(flow_narrowing_semantics_partition_24, 24);
narrowing_partition!(flow_narrowing_semantics_partition_25, 25);
narrowing_partition!(flow_narrowing_semantics_partition_26, 26);

async fn run_narrowing_matrix(session: &RealProviderTestSession, fixtures: Vec<SfcFixture>) {
    let mut slot = FixtureSlot::new(session);
    for fixture in &fixtures {
        assert_fixture(session, &mut slot, fixture, |_| async {}).await;
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
    slot.close(session).await;
}

/// One provider session per matrix size. 128, 256, 512 and 1024 each publish
/// and require exact diagnostics. A single session walking every size exceeded
/// the debug budget. `assert_production_matrix_growth` counts linearity; it
/// does not replace this sweep.
macro_rules! narrowing_matrix_case {
    ($name:ident, $build:ident, $broken:literal) => {
        paste::paste! {
            real_provider_test!(
                [<$name _128>],
                fixture = FIXTURE,
                async fn run(session) {
                    run_narrowing_matrix(session, vec![ide_fixtures::$build(128, $broken)]).await;
                }
            );
            real_provider_test!(
                [<$name _256>],
                fixture = FIXTURE,
                async fn run(session) {
                    run_narrowing_matrix(session, vec![ide_fixtures::$build(256, $broken)]).await;
                }
            );
            real_provider_test!(
                [<$name _512>],
                fixture = FIXTURE,
                async fn run(session) {
                    run_narrowing_matrix(session, vec![ide_fixtures::$build(512, $broken)]).await;
                }
            );
            real_provider_test!(
                [<$name _1024>],
                fixture = FIXTURE,
                async fn run(session) {
                    run_narrowing_matrix(session, vec![ide_fixtures::$build(1024, $broken)]).await;
                }
            );
        }
    };
}

narrowing_matrix_case!(
    flow_narrowing_linear_matrix_checks_every_callback,
    flat_matrix,
    false
);
narrowing_matrix_case!(
    flow_narrowing_linear_matrix_checks_every_callback_broken,
    flat_matrix,
    true
);
narrowing_matrix_case!(
    flow_narrowing_linear_matrix_checks_nested_callbacks,
    nested_matrix,
    false
);
narrowing_matrix_case!(
    flow_narrowing_linear_matrix_checks_nested_callbacks_broken,
    nested_matrix,
    true
);
narrowing_matrix_case!(
    flow_narrowing_linear_matrix_checks_component_callbacks,
    component_matrix,
    false
);
narrowing_matrix_case!(
    flow_narrowing_linear_matrix_checks_component_callbacks_broken,
    component_matrix,
    true
);
narrowing_matrix_case!(
    flow_narrowing_linear_matrix_checks_nested_component_callbacks,
    nested_component_matrix,
    false
);
narrowing_matrix_case!(
    flow_narrowing_linear_matrix_checks_nested_component_callbacks_broken,
    nested_component_matrix,
    true
);
