//! Per-response-class applicability across documents.
//!
//! Every answer here reaches into a second open document, `UTIL`, beside the
//! requested carrier. An edit-bearing answer names the version of every open
//! document it was computed against, in the shape the client negotiated; a
//! navigation or edit answer decodes each location through the revision it
//! captured for that document, and a captured document edited before
//! settlement fails the request with `ContentModified` — never an edit or a
//! location that addresses bytes the client no longer holds.

use std::sync::Arc;

use tower_lsp_server::ls_types::*;
use tower_lsp_server::LanguageServer;

use super::super::server_tests::{authored_token_ranges, workspace_uri};
use super::super::test_support::RequestBarrier;
use super::movement::{edited_app, Handles};
use super::{Fixture, APP, APP_PATH, RESOLVED_IMPORT};
use crate::type_provider::protocol as wire;

const UTIL_PATH: &str = "src/util.ts";
const UTIL: &str = "export const label = 'util'\nexport const msg = label\n";
/// The version the client holds `UTIL` at when the request is sent, distinct
/// from every version the fixture opens a document at.
const UTIL_VERSION: i32 = 7;
const CODE_ACTION_TITLE: &str = "Rename across files";

/// How the client applies edits, as `initialize` negotiates it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Client {
    DocumentChanges,
    ChangesOnly,
}

/// The answering routes whose answer reaches into `UTIL`.
#[derive(Clone, Copy, Debug)]
enum CrossFileRoute {
    Rename,
    CodeAction,
    Definition,
    References,
}

struct Scenario {
    fixture: Fixture,
    util_uri: Uri,
    util_canonical: String,
}

impl Scenario {
    async fn new(client: Client) -> Self {
        let fixture = Fixture::with_files(
            &[(APP_PATH, "vue", APP), (UTIL_PATH, "typescript", UTIL)],
            verter_session::HostConfig::default(),
        )
        .await;
        let util_uri = workspace_uri(&fixture.workspace_id, UTIL_PATH);
        let util_canonical = crate::documents::uri_to_canonical_id(&util_uri);
        let _ = fixture
            .server()
            .documents
            .did_change(&util_uri, UTIL_VERSION, UTIL);
        fixture.server().client_applies_versioned_edits.store(
            client == Client::DocumentChanges,
            std::sync::atomic::Ordering::Release,
        );
        Self {
            fixture,
            util_uri,
            util_canonical,
        }
    }

    /// `needle`'s byte span in `UTIL`.
    fn util_span(needle: &str) -> (u32, u32) {
        let start = UTIL.find(needle).expect("needle in UTIL") as u32;
        (start, start + needle.len() as u32)
    }

    /// `needle`'s range in `UTIL`.
    fn util_range(needle: &str) -> Range {
        let index = crate::documents::line_index::LineIndex::new_utf16(UTIL);
        let (start, end) = Self::util_span(needle);
        Range {
            start: index.offset_to_position(start).expect("start"),
            end: index.offset_to_position(end).expect("end"),
        }
    }

    fn util_location(&self) -> wire::TypeLocation {
        let (start, end) = Self::util_span("msg");
        wire::TypeLocation {
            path: self.util_canonical.clone(),
            start,
            end,
        }
    }

    /// Edit `UTIL` at `barrier` of the next request.
    fn edit_util_at(&self, barrier: RequestBarrier) {
        let server = self.fixture.server().clone();
        let util_uri = self.util_uri.clone();
        self.fixture.barriers.arm(
            barrier,
            Arc::new(move |arrival| {
                if arrival == 0 {
                    let _ = server.documents.did_change(
                        &util_uri,
                        UTIL_VERSION + 1,
                        &format!("// moved\n{UTIL}"),
                    );
                }
                Box::pin(async {})
            }),
        );
    }

    /// Arm `route`'s provider answer and return its request position (and
    /// range, for a code action).
    async fn arm(&self, route: CrossFileRoute) -> Range {
        let fixture = &self.fixture;
        let ctx = fixture.context().await;
        let path = ctx.tsx_path.clone();
        let template_msg = fixture.position("{{ msg", 3);
        let at = |position: Position| fixture.tsx_offset(&ctx, position);
        let cursor = Range {
            start: template_msg,
            end: template_msg,
        };
        match route {
            CrossFileRoute::Rename => {
                let mut locations = authored_token_ranges(APP, "msg")
                    .into_iter()
                    .map(|(line, character, _, _)| {
                        let start = at(Position::new(line, character));
                        wire::RenameLocation {
                            path: path.clone(),
                            start,
                            end: start + "msg".len() as u32,
                        }
                    })
                    .collect::<Vec<_>>();
                let util = self.util_location();
                locations.push(wire::RenameLocation {
                    path: util.path,
                    start: util.start,
                    end: util.end,
                });
                fixture
                    .provider
                    .set_rename_locations(&path, at(template_msg), locations);
                cursor
            }
            CrossFileRoute::CodeAction => {
                let range = fixture.range("const msg");
                let (literal_start, literal_end) = fixture.tsx_span(&ctx, "'hello'");
                let (util_start, util_end) = Self::util_span("'util'");
                fixture.provider.set_code_actions(
                    &path,
                    at(range.start),
                    at(range.end),
                    vec![wire::TypeCodeAction {
                        title: CODE_ACTION_TITLE.to_string(),
                        kind: Some("quickfix".to_string()),
                        edits: vec![
                            wire::TypeCodeEdit {
                                path: path.clone(),
                                start: literal_start,
                                end: literal_end,
                                new_text: "'goodbye'".to_string(),
                            },
                            wire::TypeCodeEdit {
                                path: self.util_canonical.clone(),
                                start: util_start,
                                end: util_end,
                                new_text: "'moved'".to_string(),
                            },
                        ],
                    }],
                );
                range
            }
            CrossFileRoute::Definition => {
                // `label` is an instance member no script binding declares, so
                // only the provider resolves it.
                let label = fixture.position("{{ label", 3);
                fixture
                    .provider
                    .set_definitions(&path, at(label), vec![self.util_location()]);
                Range {
                    start: label,
                    end: label,
                }
            }
            CrossFileRoute::References => {
                fixture.provider.set_references(
                    &path,
                    at(template_msg),
                    vec![self.util_location()],
                );
                cursor
            }
        }
    }

    async fn rename(&self, at: Range) -> tower_lsp_server::jsonrpc::Result<Option<WorkspaceEdit>> {
        self.fixture
            .server()
            .rename(RenameParams {
                text_document_position: TextDocumentPositionParams {
                    text_document: TextDocumentIdentifier {
                        uri: self.fixture.uri.clone(),
                    },
                    position: at.start,
                },
                new_name: "message".to_string(),
                work_done_progress_params: Default::default(),
            })
            .await
    }

    async fn code_action(
        &self,
        range: Range,
    ) -> tower_lsp_server::jsonrpc::Result<Option<WorkspaceEdit>> {
        let actions = self
            .fixture
            .server()
            .code_action(CodeActionParams {
                text_document: TextDocumentIdentifier {
                    uri: self.fixture.uri.clone(),
                },
                range,
                context: CodeActionContext {
                    diagnostics: Vec::new(),
                    only: Some(vec![CodeActionKind::QUICKFIX]),
                    trigger_kind: None,
                },
                work_done_progress_params: Default::default(),
                partial_result_params: Default::default(),
            })
            .await?;
        Ok(actions
            .unwrap_or_default()
            .into_iter()
            .find_map(|action| match action {
                CodeActionOrCommand::CodeAction(action) if action.title == CODE_ACTION_TITLE => {
                    action.edit
                }
                _ => None,
            }))
    }

    async fn locations(
        &self,
        route: CrossFileRoute,
        at: Range,
    ) -> tower_lsp_server::jsonrpc::Result<Vec<Location>> {
        let server = self.fixture.server();
        let position = TextDocumentPositionParams {
            text_document: TextDocumentIdentifier {
                uri: self.fixture.uri.clone(),
            },
            position: at.start,
        };
        let locations = match route {
            CrossFileRoute::Definition => {
                match server
                    .goto_definition(GotoDefinitionParams {
                        text_document_position_params: position,
                        work_done_progress_params: Default::default(),
                        partial_result_params: Default::default(),
                    })
                    .await?
                {
                    Some(GotoDefinitionResponse::Scalar(location)) => vec![location],
                    Some(GotoDefinitionResponse::Array(locations)) => locations,
                    Some(GotoDefinitionResponse::Link(links)) => links
                        .into_iter()
                        .map(|link| Location {
                            uri: link.target_uri,
                            range: link.target_selection_range,
                        })
                        .collect(),
                    None => Vec::new(),
                }
            }
            CrossFileRoute::References => server
                .references(ReferenceParams {
                    text_document_position: position,
                    context: ReferenceContext {
                        include_declaration: true,
                    },
                    work_done_progress_params: Default::default(),
                    partial_result_params: Default::default(),
                })
                .await?
                .unwrap_or_default(),
            CrossFileRoute::Rename | CrossFileRoute::CodeAction => {
                unreachable!("{route:?} answers an edit")
            }
        };
        Ok(locations)
    }

    fn is_util(&self, uri: &Uri) -> bool {
        verter_span::path::fs_paths_equal(
            &crate::documents::uri_to_canonical_id(uri),
            &self.util_canonical,
        )
    }

    fn is_app(&self, uri: &Uri) -> bool {
        verter_span::path::fs_paths_equal(
            &crate::documents::uri_to_canonical_id(uri),
            &self.fixture.canonical,
        )
    }
}

/// Every `(target, version, edits)` the edit delivers as a `TextDocumentEdit`.
fn document_edits(edit: &WorkspaceEdit) -> Vec<(Uri, Option<i32>, Vec<TextEdit>)> {
    let document_edits: Vec<&TextDocumentEdit> = match &edit.document_changes {
        Some(DocumentChanges::Edits(edits)) => edits.iter().collect(),
        Some(DocumentChanges::Operations(operations)) => operations
            .iter()
            .filter_map(|operation| match operation {
                DocumentChangeOperation::Edit(edit) => Some(edit),
                DocumentChangeOperation::Op(_) => None,
            })
            .collect(),
        None => Vec::new(),
    };
    document_edits
        .into_iter()
        .map(|edit| {
            (
                edit.text_document.uri.clone(),
                edit.text_document.version,
                edit.edits
                    .iter()
                    .map(|edit| match edit {
                        OneOf::Left(edit) => edit.clone(),
                        OneOf::Right(annotated) => annotated.text_edit.clone(),
                    })
                    .collect(),
            )
        })
        .collect()
}

/// Assert `edit` is delivered in `client`'s shape, and — for a versioned
/// client — that the requested carrier and `UTIL` each carry the version the
/// request captured, with `UTIL`'s edit at `util_range`.
fn assert_bound(scenario: &Scenario, client: Client, edit: &WorkspaceEdit, util_range: Range) {
    match client {
        Client::DocumentChanges => {
            assert!(
                edit.changes.is_none(),
                "a documentChanges client never receives unversioned `changes`: {edit:?}"
            );
            let edits = document_edits(edit);
            let util = edits
                .iter()
                .find(|(uri, _, _)| scenario.is_util(uri))
                .unwrap_or_else(|| panic!("the edit reaches the other open document: {edit:?}"));
            assert_eq!(
                util.1,
                Some(UTIL_VERSION),
                "the other open document's edit names the version it was computed against"
            );
            assert!(
                util.2.iter().any(|edit| edit.range == util_range),
                "the other document's edit is decoded through its captured bytes: {util:?}"
            );
            let app = edits
                .iter()
                .find(|(uri, _, _)| scenario.is_app(uri))
                .unwrap_or_else(|| panic!("the edit reaches the requested document: {edit:?}"));
            assert_eq!(
                app.1,
                Some(1),
                "the requested document's edit names its admitted version"
            );
            assert!(
                edits.iter().all(|(_, version, _)| version.is_some()),
                "every target of this edit is open, so every target is versioned: {edits:?}"
            );
        }
        Client::ChangesOnly => {
            assert!(
                edit.document_changes.is_none(),
                "a client without documentChanges is handed only `changes`: {edit:?}"
            );
            let changes = edit
                .changes
                .as_ref()
                .unwrap_or_else(|| panic!("the edit is delivered as `changes`: {edit:?}"));
            let util = changes
                .iter()
                .find(|(uri, _)| scenario.is_util(uri))
                .unwrap_or_else(|| panic!("the edit reaches the other open document: {edit:?}"));
            assert!(
                util.1.iter().any(|edit| edit.range == util_range),
                "the other document's edit is decoded through its captured bytes: {util:?}"
            );
            assert!(
                changes.keys().any(|uri| scenario.is_app(uri)),
                "the edit reaches the requested document: {edit:?}"
            );
        }
    }
}

/// A rename that reaches a second open document delivers one versioned
/// `TextDocumentEdit` per target to a documentChanges client, and keeps the
/// `changes` shape for a client without it.
#[tokio::test(flavor = "multi_thread")]
async fn rename_binds_every_open_target_to_its_captured_version() {
    for client in [Client::DocumentChanges, Client::ChangesOnly] {
        let scenario = Scenario::new(client).await;
        let at = scenario.arm(CrossFileRoute::Rename).await;
        let edit = scenario
            .rename(at)
            .await
            .unwrap_or_else(|error| panic!("{client:?}: the unmoved rename answers: {error:?}"))
            .unwrap_or_else(|| panic!("{client:?}: the unmoved rename answers an edit"));
        assert_bound(&scenario, client, &edit, Scenario::util_range("msg"));
        if client == Client::ChangesOnly {
            let changes = edit
                .changes
                .as_ref()
                .unwrap_or_else(|| panic!("the rename keeps its `changes` shape: {edit:?}"));
            assert!(
                changes.iter().any(|(uri, edits)| scenario.is_util(uri)
                    && edits.iter().any(|e| e.range == Scenario::util_range("msg"))),
                "the rename reaches the other open document: {edit:?}"
            );
        }
    }
}

/// A code action whose edit reaches a second open document names both
/// documents' captured versions.
#[tokio::test(flavor = "multi_thread")]
async fn code_action_binds_every_open_target_to_its_captured_version() {
    for client in [Client::DocumentChanges, Client::ChangesOnly] {
        let scenario = Scenario::new(client).await;
        let range = scenario.arm(CrossFileRoute::CodeAction).await;
        let edit = scenario
            .code_action(range)
            .await
            .unwrap_or_else(|error| {
                panic!("{client:?}: the unmoved code action answers: {error:?}")
            })
            .unwrap_or_else(|| panic!("{client:?}: the provider's cross-file action is served"));
        assert_bound(&scenario, client, &edit, Scenario::util_range("'util'"));
    }
}

/// A navigation answer into a second open document decodes the location
/// through that document's open bytes.
#[tokio::test(flavor = "multi_thread")]
async fn navigation_decodes_a_foreign_location_through_its_open_revision() {
    for route in [CrossFileRoute::Definition, CrossFileRoute::References] {
        let scenario = Scenario::new(Client::DocumentChanges).await;
        let at = scenario.arm(route).await;
        let locations = scenario
            .locations(route, at)
            .await
            .unwrap_or_else(|error| panic!("{route:?}: the unmoved request answers: {error:?}"));
        assert!(
            locations
                .iter()
                .any(|location| scenario.is_util(&location.uri)
                    && location.range == Scenario::util_range("msg")),
            "{route:?}: the location lands on the other document's authored token: {locations:?}"
        );
    }
}

/// The other open document is edited after the answer was decoded through it
/// and before settlement: every cross-file route answers `ContentModified` for
/// every client — never an edit naming no version, an edit naming a version
/// the client has replaced, or a location into bytes the client no longer
/// holds.
#[tokio::test(flavor = "multi_thread")]
async fn a_target_edited_before_settlement_fails_the_request() {
    for client in [Client::DocumentChanges, Client::ChangesOnly] {
        for route in [
            CrossFileRoute::Rename,
            CrossFileRoute::CodeAction,
            CrossFileRoute::Definition,
            CrossFileRoute::References,
        ] {
            let scenario = Scenario::new(client).await;
            let at = scenario.arm(route).await;
            scenario.fixture.barriers.clear();
            scenario.edit_util_at(RequestBarrier::Settlement);
            let error = match route {
                CrossFileRoute::Rename => scenario.rename(at).await.err(),
                CrossFileRoute::CodeAction => scenario.code_action(at).await.err(),
                CrossFileRoute::Definition | CrossFileRoute::References => {
                    scenario.locations(route, at).await.err()
                }
            };
            assert!(
                scenario
                    .fixture
                    .barriers
                    .arrivals(RequestBarrier::Settlement)
                    >= 1,
                "{client:?}/{route:?}: the request reached settlement"
            );
            assert_eq!(
                error.map(|error| error.code),
                Some(tower_lsp_server::jsonrpc::ErrorCode::ContentModified),
                "{client:?}/{route:?}: a target edited before settlement fails the request"
            );
        }
    }
}

/// A completion-resolve `additionalTextEdits` import is placed through the
/// carrier revision the captured provider surface was built from. It lands on
/// the unmoved document, and a document that moved after the capture yields no
/// edit (and marks the request's captured target incoherent, so it settles
/// `ContentModified`) rather than placing the import through the later
/// revision's line index.
#[tokio::test(flavor = "multi_thread")]
async fn completion_resolve_places_edits_only_through_the_captured_revision() {
    use crate::server::nav_features_completion_resolve::{
        capture_resolve_surface, resolve_provider_auto_import_edits,
    };
    use crate::type_provider::auto_import::ProviderImportEdit;

    let fixture = Fixture::new().await;
    let ctx = fixture.context().await;
    let path = ctx.tsx_path.clone();
    let (import_at, _) = fixture.tsx_span(&ctx, "const msg");
    let provider_edits = vec![ProviderImportEdit {
        start: import_at,
        end: import_at,
        new_text: RESOLVED_IMPORT.to_string(),
    }];
    let server = fixture.server();
    let (_, surface) =
        capture_resolve_surface(server, &path).expect("the carrier serves a provider surface");

    let placed = resolve_provider_auto_import_edits(server, &path, Some(&surface), &provider_edits)
        .expect("the unmoved carrier resolves")
        .expect("the import is placed in the carrier");
    assert!(
        placed
            .iter()
            .any(|edit| edit.new_text.contains("messageFromProvider")),
        "the unmoved carrier receives the provider's import: {placed:?}"
    );

    Handles::of(&fixture).edit(2, &edited_app());
    assert_eq!(
        resolve_provider_auto_import_edits(server, &path, Some(&surface), &provider_edits),
        Ok(None),
        "a carrier that moved after the surface capture receives no import"
    );
}
