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
const CARRIER_PATH: &str = "src/Target.vue";
const CARRIER: &str = "<script setup lang=\"ts\">\ndefineProps<{ label: string }>()\nconst msg = 'child'\n</script>\n<template>{{ msg }}</template>\n";

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

async fn assert_closed_carrier_applicability(route: CrossFileRoute, api: bool) {
    for client in [Client::DocumentChanges, Client::ChangesOnly] {
        for barrier in [
            None,
            Some(RequestBarrier::ProviderDecode),
            Some(RequestBarrier::Settlement),
        ] {
            let app = if api {
                APP.replace("const msg", "import Target from './Target.vue'\nconst msg")
            } else {
                APP.to_string()
            };
            let fixture = Fixture::with_files(
                &[(APP_PATH, "vue", &app), (CARRIER_PATH, "vue", CARRIER)],
                verter_session::HostConfig::default(),
            )
            .await;
            let util_uri = workspace_uri(&fixture.workspace_id, CARRIER_PATH);
            let util_canonical = crate::documents::uri_to_canonical_id(&util_uri);
            fixture.server().client_applies_versioned_edits.store(
                client == Client::DocumentChanges,
                std::sync::atomic::Ordering::Release,
            );
            fixture.server().ensure_current_file_synced(&util_uri).await;
            let child_ctx = fixture
                .server()
                .type_provider_context(&util_uri)
                .expect("child IDE context");
            let path = if api {
                fixture
                    .server()
                    .provider_sync_states
                    .get(&util_canonical)
                    .and_then(|state| state.api_path.clone())
                    .expect("child API path")
            } else {
                child_ctx.tsx_path.clone()
            };
            let snapshot = fixture
                .server()
                .documents
                .provider_surfaces()
                .current_snapshot(&path)
                .expect("child projection");
            fixture
                .provider
                .accept_unrecorded_delivery(&path, &snapshot.provider_content);
            fixture
                .provider
                .hold_engine_target(&path, &snapshot.provider_content);
            let needle = if api { "label" } else { "msg" };
            let source_start = CARRIER
                .find(if api { "label: string" } else { "const msg" })
                .expect("source token")
                + if api { 0 } else { 6 };
            let source_index = crate::documents::line_index::LineIndex::new_utf16(CARRIER);
            let expected_range = Range {
                start: source_index
                    .offset_to_position(source_start as u32)
                    .expect("source start"),
                end: source_index
                    .offset_to_position((source_start + needle.len()) as u32)
                    .expect("source end"),
            };
            let start = if api {
                let captured = fixture
                    .server()
                    .documents
                    .provider_surfaces()
                    .capture_current_carrier_api_set();
                let crate::type_provider::merge::ApiSurfaceResolution::Vouched(ctx) =
                    crate::provider_surface_store::classify_captured_api_surface(
                        None,
                        &captured,
                        &path,
                        PositionEncodingKind::UTF16,
                    )
                else {
                    panic!("vouched API");
                };
                snapshot
                    .provider_content
                    .match_indices(needle)
                    .find_map(|(offset, _)| {
                        let range = crate::type_provider::merge::api_surface_range_to_carrier_range(
                            offset as u32,
                            (offset + needle.len()) as u32,
                            &ctx.tsx_line_index,
                            &ctx.mapper,
                            &ctx.carrier_line_index,
                            &ctx.carrier_line_index,
                        );
                        (range == Some(expected_range)).then_some(offset as u32)
                    })
                    .expect("mapped API property")
            } else {
                (snapshot
                    .provider_content
                    .find("const msg")
                    .expect("IDE declaration")
                    + 6) as u32
            };
            let end = start + needle.len() as u32;
            fixture.server().documents.did_close(&util_uri);
            let scenario = Scenario {
                fixture,
                util_uri,
                util_canonical,
            };
            let ctx = scenario.fixture.context().await;
            let position = scenario.fixture.position("{{ msg", 3);
            let offset = scenario.fixture.tsx_offset(&ctx, position);
            let at = match route {
                CrossFileRoute::References => {
                    scenario.fixture.provider.set_references(
                        &ctx.tsx_path,
                        offset,
                        vec![wire::TypeLocation {
                            path: path.clone(),
                            start,
                            end,
                        }],
                    );
                    Range {
                        start: position,
                        end: position,
                    }
                }
                CrossFileRoute::Rename => {
                    scenario.fixture.provider.set_rename_locations(
                        &ctx.tsx_path,
                        offset,
                        vec![wire::RenameLocation {
                            path: path.clone(),
                            start,
                            end,
                        }],
                    );
                    Range {
                        start: position,
                        end: position,
                    }
                }
                CrossFileRoute::CodeAction => {
                    let range = scenario.fixture.range("const msg");
                    scenario.fixture.provider.set_code_actions(
                        &ctx.tsx_path,
                        scenario.fixture.tsx_offset(&ctx, range.start),
                        scenario.fixture.tsx_offset(&ctx, range.end),
                        vec![wire::TypeCodeAction {
                            title: CODE_ACTION_TITLE.to_string(),
                            kind: Some("quickfix".to_string()),
                            edits: vec![wire::TypeCodeEdit {
                                path: path.clone(),
                                start,
                                end,
                                new_text: "renamedMsg".to_string(),
                            }],
                        }],
                    );
                    range
                }
                CrossFileRoute::Definition => unreachable!("closed carrier control routes"),
            };
            scenario.fixture.barriers.clear();
            if let Some(barrier) = barrier {
                let server = scenario.fixture.server().clone();
                let canonical = scenario.util_canonical.clone();
                scenario.fixture.barriers.arm(
                    barrier,
                    Arc::new(move |_| {
                        server.documents.host().notify_upsert(
                            &canonical,
                            Arc::from(format!("<!-- moved -->\n{CARRIER}")),
                        );
                        Box::pin(async {})
                    }),
                );
            }
            let response: tower_lsp_server::jsonrpc::Result<Vec<(Uri, Range)>> = match route {
                CrossFileRoute::References => {
                    scenario.locations(route, at).await.map(|locations| {
                        locations
                            .into_iter()
                            .map(|location| (location.uri, location.range))
                            .collect()
                    })
                }
                CrossFileRoute::Rename => scenario.rename(at).await.map(|edit| {
                    edit.into_iter()
                        .flat_map(|edit| delivered_target_ranges(&edit))
                        .collect()
                }),
                CrossFileRoute::CodeAction => scenario.code_action(at).await.map(|actions| {
                    actions
                        .into_iter()
                        .flat_map(|edit| delivered_target_ranges(&edit))
                        .collect()
                }),
                CrossFileRoute::Definition => unreachable!("closed carrier control routes"),
            };
            assert_eq!(
                &*scenario
                    .fixture
                    .server()
                    .documents
                    .provider_surfaces()
                    .current_snapshot(&path)
                    .expect("projection still current")
                    .provider_content,
                &*snapshot.provider_content,
                "only the real carrier source moved"
            );
            let current = scenario
                .fixture
                .server()
                .documents
                .provider_surfaces()
                .current_snapshot(&path)
                .expect("retained projection");
            assert_eq!(current.stamp.map_hash, snapshot.stamp.map_hash);
            assert_eq!(&*current.carrier_source, CARRIER);
            if barrier.is_some() {
                assert_eq!(
                    response.expect_err("changed closed carrier refuses").code,
                    tower_lsp_server::jsonrpc::ErrorCode::ContentModified,
                    "{client:?}/{route:?}/{barrier:?}/API={api}"
                );
            } else {
                let targets = response.expect("unchanged closed carrier succeeds");
                assert!(targets.iter().any(|(uri, range)| scenario.is_util(uri) && *range == expected_range), "{client:?}/{route:?}/API={api}: unchanged answer targets the closed carrier: {targets:?}");
            }
        }
    }
}

fn delivered_target_ranges(edit: &WorkspaceEdit) -> Vec<(Uri, Range)> {
    let mut targets: Vec<_> = document_edits(edit)
        .into_iter()
        .flat_map(|(uri, _, edits)| edits.into_iter().map(move |edit| (uri.clone(), edit.range)))
        .collect();
    if let Some(changes) = &edit.changes {
        targets.extend(
            changes
                .iter()
                .flat_map(|(uri, edits)| edits.iter().map(move |edit| (uri.clone(), edit.range))),
        );
    }
    targets
}

#[tokio::test(flavor = "multi_thread")]
async fn closed_carrier_ide_references_validate_retained_source() {
    assert_closed_carrier_applicability(CrossFileRoute::References, false).await;
}

#[tokio::test(flavor = "multi_thread")]
async fn closed_carrier_api_rename_validates_retained_source() {
    assert_closed_carrier_applicability(CrossFileRoute::Rename, true).await;
}

#[tokio::test(flavor = "multi_thread")]
async fn closed_carrier_rename_validates_retained_source() {
    assert_closed_carrier_applicability(CrossFileRoute::Rename, false).await;
}

#[tokio::test(flavor = "multi_thread")]
async fn closed_carrier_code_action_validates_retained_source() {
    assert_closed_carrier_applicability(CrossFileRoute::CodeAction, false).await;
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

#[tokio::test(flavor = "multi_thread")]
async fn a_closed_target_changed_before_decode_or_settlement_refuses_the_answer() {
    for client in [Client::DocumentChanges, Client::ChangesOnly] {
        for barrier in [RequestBarrier::ProviderDecode, RequestBarrier::Settlement] {
            for route in [
                CrossFileRoute::Rename,
                CrossFileRoute::CodeAction,
                CrossFileRoute::Definition,
                CrossFileRoute::References,
            ] {
                let scenario = Scenario::new(client).await;
                scenario
                    .fixture
                    .server()
                    .documents
                    .did_close(&scenario.util_uri);
                scenario
                    .fixture
                    .provider
                    .hold_engine_target(&scenario.util_canonical, UTIL);
                let at = scenario.arm(route).await;
                let server = scenario.fixture.server().clone();
                let canonical = scenario.util_canonical.clone();
                scenario.fixture.barriers.clear();
                scenario.fixture.barriers.arm(
                    barrier,
                    Arc::new(move |_| {
                        server
                            .documents
                            .host()
                            .notify_upsert(&canonical, Arc::from(format!("// moved\n{UTIL}")));
                        Box::pin(async {})
                    }),
                );
                let error = match route {
                    CrossFileRoute::Rename => scenario.rename(at).await.err(),
                    CrossFileRoute::CodeAction => scenario.code_action(at).await.err(),
                    CrossFileRoute::Definition | CrossFileRoute::References => {
                        scenario.locations(route, at).await.err()
                    }
                };
                assert_eq!(
                    error.map(|error| error.code),
                    Some(tower_lsp_server::jsonrpc::ErrorCode::ContentModified),
                    "{client:?}/{route:?}/{barrier:?}"
                );
            }
        }
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn native_navigation_keeps_closed_reexport_intermediates_until_settlement() {
    let app = "<script setup lang=\"ts\">\nimport { helper } from './util'\nconsole.log(helper)\n</script>\n";
    let fixture = Fixture::with_files(
        &[
            (APP_PATH, "vue", app),
            (
                "src/util.ts",
                "typescript",
                "export { helper } from './middle'\n",
            ),
            (
                "src/middle.ts",
                "typescript",
                "export { helper } from './leaf'\n",
            ),
            ("src/leaf.ts", "typescript", "export const helper = 1\n"),
        ],
        verter_session::HostConfig::default(),
    )
    .await;
    for path in ["src/util.ts", "src/middle.ts", "src/leaf.ts"] {
        fixture
            .server()
            .documents
            .did_close(&workspace_uri(&fixture.workspace_id, path));
    }
    let position = super::super::server_tests::find_document_position(
        fixture.server(),
        &fixture.uri,
        "helper }",
        1,
    );
    let params = || GotoDefinitionParams {
        text_document_position_params: TextDocumentPositionParams {
            text_document: TextDocumentIdentifier {
                uri: fixture.uri.clone(),
            },
            position,
        },
        work_done_progress_params: Default::default(),
        partial_result_params: Default::default(),
    };
    let answer = fixture
        .server()
        .goto_definition(params())
        .await
        .expect("unmoved native answer");
    assert!(format!("{answer:?}").contains("/leaf.ts"));
    let server = fixture.server().clone();
    let middle = crate::documents::uri_to_canonical_id(&workspace_uri(
        &fixture.workspace_id,
        "src/middle.ts",
    ));
    fixture.barriers.clear();
    fixture.barriers.arm(
        RequestBarrier::Settlement,
        Arc::new(move |_| {
            let _ = server
                .documents
                .host()
                .upsert(verter_session::UpsertRequest {
                    canonical_id: None,
                    input_id: middle.clone(),
                    source: Arc::from("export const helper = 2\n"),
                    file_language: verter_session::FileLanguage::script_ts(),
                    aliases: Vec::new(),
                })
                .expect("replace intermediate");
            Box::pin(async {})
        }),
    );
    let error = fixture
        .server()
        .goto_definition(params())
        .await
        .expect_err("changed intermediate refuses the answer");
    assert_eq!(
        error.code,
        tower_lsp_server::jsonrpc::ErrorCode::ContentModified
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn native_assembly_retries_analysis_a_source_b_after_source_returns_to_a() {
    let scenario = Scenario::new(Client::ChangesOnly).await;
    let server = scenario.fixture.server();
    server.documents.did_close(&scenario.util_uri);
    let host = server.documents.host();
    let mut attempts = 0;
    let (_, source) = server
        .read_child_at_one_revision(&scenario.util_canonical, || {
            let capture = match host.capture_export_span(&scenario.util_canonical, "msg") {
                verter_session::NativeExportRead::Captured(capture) => capture,
                other => panic!("fixture export: {other:?}"),
            };
            attempts += 1;
            let commit = |source: &str| {
                let _ = host
                    .upsert(verter_session::UpsertRequest {
                        canonical_id: None,
                        input_id: scenario.util_canonical.clone(),
                        source: Arc::from(source),
                        file_language: verter_session::FileLanguage::script_ts(),
                        aliases: Vec::new(),
                    })
                    .expect("source transition");
            };
            if attempts == 1 {
                commit(&format!("// moved\n{UTIL}"));
            }
            let source = host
                .get_source(&scenario.util_canonical)
                .expect("source for geometry");
            if attempts == 1 {
                commit(UTIL);
            }
            Some((capture.start, source))
        })
        .expect("coherent retry");
    assert_eq!(
        attempts, 2,
        "the mixed first read is rejected despite equal endpoint hashes"
    );
    assert_eq!(&*source, UTIL);
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
