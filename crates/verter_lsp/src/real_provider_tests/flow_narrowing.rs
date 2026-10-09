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

use tower_lsp_server::ls_types::{
    Diagnostic, DiagnosticSeverity, DidChangeWatchedFilesParams, FileChangeType, FileEvent,
    NumberOrString, Uri,
};
use tower_lsp_server::LanguageServer;
use verter_compiler::flow_check::fixtures::Expected;
use verter_compiler::flow_check::ide_fixtures::{self, SfcFixture};
use verter_compiler::flow_check::seam::Span;

use crate::test_harness::{real_provider_test, RealProviderTestSession};

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

/// Open `fixture` and return its mapped error diagnostics once the canary
/// proves the provider checked it.
async fn checked_errors(
    session: &RealProviderTestSession,
    fixture: &SfcFixture,
) -> (Uri, Vec<Expected>) {
    let relative = format!(
        "src/{}{}.vue",
        fixture.name,
        if session.is_tsgo() {
            "Tsgo"
        } else {
            "Tsserver"
        }
    );
    let _published = session.publish_workspace_file(&relative, &fixture.source);
    session
        .server()
        .did_change_watched_files(DidChangeWatchedFilesParams {
            changes: vec![FileEvent {
                uri: session.workspace_uri(&relative),
                typ: FileChangeType::CREATED,
            }],
        })
        .await;
    let uri = session.open_fixture_file(&relative).await;
    // A cold provider project answers diagnostics only once it has built the
    // program; a completion inside the canary proves it did.
    session
        .wait_until_ready(&uri, "flowCanary", 4, "flowCanary")
        .await;
    for _ in 0..30 {
        let diagnostics = session
            .merged_diagnostics_until(&uri, |diagnostics| {
                diagnostics.iter().any(|d| code_of(d) == 2322)
            })
            .await;
        let errors: Vec<Expected> = diagnostics
            .iter()
            .filter(|d| d.severity == Some(DiagnosticSeverity::ERROR))
            .map(|d| Expected {
                code: code_of(d),
                authored: authored_span(session, &uri, d),
            })
            .collect();
        if errors.contains(&Expected {
            code: 2322,
            authored: fixture.canary,
        }) {
            return (uri, errors);
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

async fn assert_fixture(session: &RealProviderTestSession, fixture: &SfcFixture) -> Uri {
    let (uri, reported) = checked_errors(session, fixture).await;
    if let Err(mismatch) = check_exact(fixture, &reported) {
        panic!("[{}] {mismatch}", session.provider_kind().label());
    }
    uri
}

real_provider_test!(
    flow_narrowing_semantics_are_exact,
    fixture = FIXTURE,
    async fn run(session) {
        for fixture in ide_fixtures::semantic_suite() {
            let uri = assert_fixture(session, &fixture).await;
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
        }
    }
);

real_provider_test!(
    flow_narrowing_linear_matrix_checks_every_callback,
    fixture = FIXTURE,
    async fn run(session) {
        for &n in &verter_compiler::flow_check::fixtures::MATRIX_SIZES {
            for fixture in [
                ide_fixtures::flat_matrix(n, false),
                ide_fixtures::flat_matrix(n, true),
                ide_fixtures::nested_matrix(n, false),
                ide_fixtures::nested_matrix(n, true),
                ide_fixtures::component_matrix(n, false),
                ide_fixtures::component_matrix(n, true),
            ] {
                assert_fixture(session, &fixture).await;
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
