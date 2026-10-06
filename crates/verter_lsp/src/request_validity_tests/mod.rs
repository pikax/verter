//! Request validity under concurrent background work.
//!
//! Every foreground route is driven through the same barrier-controlled
//! harness: a request reaches four named [`RequestBarrier`]s (basis capture,
//! provider dispatch, provider-response decode, settlement), and a schedule
//! moves server state at a chosen barrier through the real producer that owns
//! that state. Nothing sleeps and nothing polls: a barrier action runs inside
//! the request, in program order.
//!
//! Three row families share one route table:
//! - the steady row proves the fixture gives every route a NONEMPTY provider
//!   answer with exactly one provider dispatch when nothing moves;
//! - the controls (`controls`) pin the fail-closed guarantees: a request whose
//!   inputs genuinely changed never answers with a range mapped through another
//!   revision;
//! - the liveness rows (`liveness`) require a coherent answer while background
//!   work moves only state that cannot change it.
//!
//! The route universe is the production [`ForegroundRoute`] enum the request
//! disposition settles by. `foreground_routes!` generates every route's rows
//! together with an exhaustive `match` over that enum, and each per-route
//! property is an exhaustive `match` without a wildcard, so a production route
//! added without its rows does not compile.
//!
//! [`ForegroundRoute`]: crate::documents::ForegroundRoute

use std::sync::Arc;

use tower_lsp_server::ls_types::*;
use tower_lsp_server::LanguageServer;

use super::server_tests::{
    authored_token_ranges, find_document_position, make_definition_test_server_with_config,
    rename_edit_ranges, synced_type_provider_context, tsgo_resolve_envelope_item, workspace_uri,
};
use super::test_support::{RequestBarrier, RequestBarriers};
use super::{TypeProviderContext, VerterLanguageServer};
pub(super) use crate::documents::ForegroundRoute as Route;
use crate::type_provider::merge;
use crate::type_provider::mock::{MockCall, MockTypeProvider};
use crate::type_provider::protocol as wire;

mod controls;
mod liveness;
mod movement;
mod pins;

const APP_PATH: &str = "src/App.vue";

/// One carrier every route answers from. `'hello'` is a position no native
/// Verter feature ever reports, so a location or highlight on it can only have
/// come from the provider. `label` is an instance member no script binding
/// declares, so only the provider can navigate or rename it.
const APP: &str = "<script setup lang=\"ts\">\nconst msg = 'hello'\nfunction greet(name: string) {\n  return name\n}\ngreet(msg)\n</script>\n<template><div>{{ msg }} {{ msg.length }} {{ label }}</div></template>\n";

const HOVER_TEXT: &str = "const msg: \"hello\"";
const SIGNATURE_LABEL: &str = "greet(name: string): string";
const COMPLETION_LABEL: &str = "messageFromProvider";
const RESOLVED_DETAIL: &str = "resolved by the provider";
const RESOLVED_IMPORT: &str = "import { messageFromProvider } from './provider'\n";
const CODE_ACTION_TITLE: &str = "Replace the greeting";
const INLAY_LABEL: &str = ": string";

macro_rules! foreground_routes {
    ($(
        $module:ident => $route:ident {
            $($(#[$attr:meta])* $row:ident: $movement:ident),+ $(,)?
        }
    ),+ $(,)?) => {
        /// Every production route has rows: a route without one is a
        /// non-exhaustive match.
        fn every_route_has_rows(route: Route) {
            match route {
                $(Route::$route => {}),+
            }
        }

        $(
            mod $module {
                use super::Route;

                #[tokio::test(flavor = "multi_thread")]
                async fn answers_once_when_nothing_moves() {
                    super::assert_steady(Route::$route).await;
                }

                super::controls::control_rows!(Route::$route);
                super::liveness::liveness_rows!(
                    Route::$route;
                    $($(#[$attr])* $row: $movement),+
                );
            }
        )+
    };
}

foreground_routes! {
    hover => Hover {
        answers_through_diagnostics_republication: DiagnosticsRepublication,
        answers_through_identical_surface_records: IdenticalSurfaceRecord,
        answers_through_equivalent_root_publication: EquivalentRootPublication,
    },
    signature_help => SignatureHelp {
        answers_through_diagnostics_republication: DiagnosticsRepublication,
        answers_through_identical_surface_records: IdenticalSurfaceRecord,
        answers_through_equivalent_root_publication: EquivalentRootPublication,
    },
    definition => Definition {
        answers_through_diagnostics_republication: DiagnosticsRepublication,
        answers_through_identical_surface_records: IdenticalSurfaceRecord,
        answers_through_equivalent_root_publication: EquivalentRootPublication,
    },
    type_definition => TypeDefinition {
        answers_through_diagnostics_republication: DiagnosticsRepublication,
        answers_through_identical_surface_records: IdenticalSurfaceRecord,
        answers_through_equivalent_root_publication: EquivalentRootPublication,
    },
    references => References {
        answers_through_diagnostics_republication: DiagnosticsRepublication,
        answers_through_identical_surface_records: IdenticalSurfaceRecord,
        answers_through_equivalent_root_publication: EquivalentRootPublication,
    },
    document_highlight => DocumentHighlight {
        answers_through_diagnostics_republication: DiagnosticsRepublication,
        answers_through_identical_surface_records: IdenticalSurfaceRecord,
        answers_through_equivalent_root_publication: EquivalentRootPublication,
    },
    prepare_rename => PrepareRename {
        answers_through_diagnostics_republication: DiagnosticsRepublication,
        answers_through_identical_surface_records: IdenticalSurfaceRecord,
        answers_through_equivalent_root_publication: EquivalentRootPublication,
    },
    rename => Rename {
        answers_through_diagnostics_republication: DiagnosticsRepublication,
        answers_through_identical_surface_records: IdenticalSurfaceRecord,
        answers_through_equivalent_root_publication: EquivalentRootPublication,
    },
    completion => Completion {
        answers_through_diagnostics_republication: DiagnosticsRepublication,
        answers_through_identical_surface_records: IdenticalSurfaceRecord,
        answers_through_equivalent_root_publication: EquivalentRootPublication,
    },
    completion_resolve => CompletionResolve {
        answers_through_diagnostics_republication: DiagnosticsRepublication,
        answers_through_identical_surface_records: IdenticalSurfaceRecord,
        answers_through_equivalent_root_publication: EquivalentRootPublication,
    },
    code_action => CodeAction {
        answers_through_diagnostics_republication: DiagnosticsRepublication,
        answers_through_identical_surface_records: IdenticalSurfaceRecord,
        answers_through_equivalent_root_publication: EquivalentRootPublication,
    },
    inlay_hint => InlayHint {
        answers_through_diagnostics_republication: DiagnosticsRepublication,
        answers_through_identical_surface_records: IdenticalSurfaceRecord,
        answers_through_equivalent_root_publication: EquivalentRootPublication,
    },
    semantic_tokens => SemanticTokens {
        answers_through_diagnostics_republication: DiagnosticsRepublication,
        answers_through_identical_surface_records: IdenticalSurfaceRecord,
        answers_through_equivalent_root_publication: EquivalentRootPublication,
    },
}

/// How one request ended, reduced to what a schedule may assert on.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) enum Outcome {
    /// The reply carries the provider's contribution, normalized to the
    /// mapped ranges and payload a client would see.
    Answered(String),
    /// The reply carries no provider contribution (`Ok(None)` or a native-only
    /// answer).
    Empty,
    /// `ContentModified`.
    ContentModified,
    /// Any other typed error.
    Refused(String),
}

/// A server with one open carrier, synced to a mock provider whose queries
/// reach the server's request barriers.
pub(super) struct Fixture {
    _temp: tempfile::TempDir,
    service: tower_lsp_server::LspService<VerterLanguageServer>,
    drain: tokio::task::JoinHandle<()>,
    pub(super) provider: Arc<MockTypeProvider>,
    pub(super) uri: Uri,
    pub(super) canonical: String,
    pub(super) workspace_id: String,
    pub(super) barriers: Arc<RequestBarriers>,
}

impl Drop for Fixture {
    fn drop(&mut self) {
        self.drain.abort();
    }
}

impl Fixture {
    pub(super) async fn new() -> Self {
        let (temp, service, drain, provider, workspace_id) =
            make_definition_test_server_with_config(
                &[(APP_PATH, "vue", APP)],
                crate::TypeProviderKind::Tsgo,
                verter_session::HostConfig::default(),
                false,
            )
            .await;
        let server = service.inner();
        let uri = workspace_uri(&workspace_id, APP_PATH);
        server.ensure_current_file_synced(&uri).await;
        server.publish_import_dependencies_settled(&uri).await;
        let barriers = server.request_barriers();
        provider.set_request_barriers(Arc::clone(&barriers));
        let canonical = crate::documents::uri_to_canonical_id(&uri);
        Self {
            _temp: temp,
            service,
            drain,
            provider,
            uri,
            canonical,
            workspace_id,
            barriers,
        }
    }

    pub(super) fn server(&self) -> &VerterLanguageServer {
        self.service.inner()
    }

    pub(super) async fn context(&self) -> TypeProviderContext {
        synced_type_provider_context(self.server(), &self.uri).await
    }

    fn position(&self, needle: &str, delta: usize) -> Position {
        find_document_position(self.server(), &self.uri, needle, delta)
    }

    fn range(&self, needle: &str) -> Range {
        Range {
            start: self.position(needle, 0),
            end: self.position(needle, needle.len()),
        }
    }

    fn tsx_offset(&self, ctx: &TypeProviderContext, position: Position) -> u32 {
        merge::carrier_position_to_tsx_offset_validated(
            &position,
            &ctx.carrier_line_index,
            &ctx.mapper,
            &ctx.tsx_line_index,
        )
        .unwrap_or_else(|| panic!("{position:?} maps into the IDE surface"))
    }

    /// The generated span of the authored `needle`.
    fn tsx_span(&self, ctx: &TypeProviderContext, needle: &str) -> (u32, u32) {
        let start = self.tsx_offset(ctx, self.position(needle, 0));
        (start, start + needle.len() as u32)
    }

    /// Provider queries of `route`'s kind recorded since the last clear.
    pub(super) fn dispatches(&self, route: Route) -> usize {
        self.provider
            .calls()
            .iter()
            .filter(|call| route.dispatched_by(call))
            .count()
    }
}

/// The request inputs and the provider answer one route is armed with.
pub(super) struct Armed {
    position: Position,
    range: Range,
    resolve_item: Option<CompletionItem>,
    literal: Range,
}

impl Route {
    /// Whether the route re-asks the provider once after a lost delivery,
    /// through the shared bounded recovery.
    pub(super) fn recovers_a_lost_delivery(self) -> bool {
        match self {
            Route::Hover | Route::Definition | Route::TypeDefinition | Route::Completion => true,
            Route::SignatureHelp
            | Route::References
            | Route::DocumentHighlight
            | Route::PrepareRename
            | Route::Rename
            | Route::CompletionResolve
            | Route::CodeAction
            | Route::InlayHint
            | Route::SemanticTokens => false,
        }
    }

    /// Whether `call` is this route's provider query.
    fn dispatched_by(self, call: &MockCall) -> bool {
        match self {
            Route::Hover => matches!(call, MockCall::GetHover { .. }),
            Route::SignatureHelp => matches!(call, MockCall::GetSignatureHelp { .. }),
            Route::Definition => matches!(call, MockCall::GetDefinition { .. }),
            Route::TypeDefinition => matches!(call, MockCall::GetTypeDefinition { .. }),
            Route::References => matches!(call, MockCall::GetReferences { .. }),
            Route::DocumentHighlight => matches!(call, MockCall::GetDocumentHighlights { .. }),
            Route::PrepareRename | Route::Rename => {
                matches!(call, MockCall::GetRenameLocations { .. })
            }
            Route::Completion => matches!(call, MockCall::GetCompletions { .. }),
            Route::CompletionResolve => matches!(call, MockCall::ResolveCompletion { .. }),
            Route::CodeAction => matches!(call, MockCall::GetCodeActions { .. }),
            Route::InlayHint => matches!(call, MockCall::GetInlayHints { .. }),
            Route::SemanticTokens => matches!(call, MockCall::GetSemanticTokens { .. }),
        }
    }

    /// Configure the provider answer this route queries for.
    pub(super) async fn arm(self, fixture: &Fixture) -> Armed {
        let ctx = fixture.context().await;
        let provider = &fixture.provider;
        let path = ctx.tsx_path.clone();
        let template_msg = fixture.position("{{ msg", 3);
        let literal = fixture.range("'hello'");
        let (literal_start, literal_end) = fixture.tsx_span(&ctx, "'hello'");
        let literal_location = || {
            vec![wire::TypeLocation {
                path: path.clone(),
                start: literal_start,
                end: literal_end,
            }]
        };
        let mut armed = Armed {
            position: template_msg,
            range: Range {
                start: template_msg,
                end: template_msg,
            },
            resolve_item: None,
            literal,
        };
        match self {
            Route::Hover => {
                let offset = fixture.tsx_offset(&ctx, template_msg);
                provider.set_hover(
                    &path,
                    offset,
                    Some(wire::HoverInfo {
                        contents: HOVER_TEXT.to_string(),
                        display_signature: Some(
                            crate::type_provider::mock::test_display_signature(HOVER_TEXT),
                        ),
                        ..Default::default()
                    }),
                );
            }
            Route::SignatureHelp => {
                let position = fixture.position("greet(msg)", 6);
                provider.set_signature_help(
                    &path,
                    fixture.tsx_offset(&ctx, position),
                    Some(wire::SignatureHelp {
                        signatures: vec![wire::SignatureInfo {
                            label: SIGNATURE_LABEL.to_string(),
                            documentation: None,
                            parameters: Vec::new(),
                            active_parameter: None,
                        }],
                        active_signature: Some(0),
                        active_parameter: Some(0),
                    }),
                );
                armed.position = position;
            }
            Route::Definition => {
                let position = fixture.position("{{ label", 3);
                provider.set_definitions(
                    &path,
                    fixture.tsx_offset(&ctx, position),
                    literal_location(),
                );
                armed.position = position;
            }
            Route::TypeDefinition => provider.set_type_definitions(
                &path,
                fixture.tsx_offset(&ctx, template_msg),
                literal_location(),
            ),
            Route::References => provider.set_references(
                &path,
                fixture.tsx_offset(&ctx, template_msg),
                literal_location(),
            ),
            Route::DocumentHighlight => provider.set_highlights(
                &path,
                fixture.tsx_offset(&ctx, template_msg),
                vec![wire::TypeDocumentHighlight {
                    start: literal_start,
                    end: literal_end,
                    kind: wire::TypeDocumentHighlightKind::Write,
                }],
            ),
            Route::PrepareRename => {
                let position = fixture.position("{{ label", 3);
                let (start, end) = fixture.tsx_span(&ctx, "label }}");
                provider.set_rename_locations(
                    &path,
                    fixture.tsx_offset(&ctx, position),
                    vec![wire::RenameLocation {
                        path: path.clone(),
                        start,
                        end: end - " }}".len() as u32,
                    }],
                );
                armed.position = position;
            }
            Route::Rename => {
                let locations = authored_token_ranges(APP, "msg")
                    .into_iter()
                    .map(|(line, character, _, _)| {
                        let start = fixture.tsx_offset(&ctx, Position::new(line, character));
                        wire::RenameLocation {
                            path: path.clone(),
                            start,
                            end: start + "msg".len() as u32,
                        }
                    })
                    .collect::<Vec<_>>();
                provider.set_rename_locations(
                    &path,
                    fixture.tsx_offset(&ctx, template_msg),
                    locations,
                );
            }
            Route::Completion => {
                let position = fixture.position("msg.length", 4);
                let completion = wire::Completion {
                    label: COMPLETION_LABEL.to_string(),
                    kind: Some(wire::CompletionKind::Property),
                    detail: None,
                    documentation: None,
                    edit_range_start: None,
                    edit_range_end: None,
                    text_edit_new_text: None,
                    insert_text: None,
                    sort_text: None,
                    insert_text_format: None,
                    commit_characters: None,
                    filter_text: None,
                    preselect: None,
                    label_details: None,
                    data: None,
                };
                provider.set_completions(
                    &path,
                    fixture.tsx_offset(&ctx, position),
                    vec![completion],
                );
                armed.position = position;
            }
            Route::CompletionResolve => {
                let key = wire::CompletionResolveData::Lsp {
                    label: COMPLETION_LABEL.to_string(),
                    data: serde_json::json!({ "exportName": COMPLETION_LABEL }),
                };
                // The provider's import edit is addressed in the surface the
                // resolve queried, so the edit can only be placed through that
                // surface's map.
                let (import_at, _) = fixture.tsx_span(&ctx, "const msg");
                provider.set_resolve_completion(
                    &path,
                    key,
                    Some(wire::CompletionResolveResult {
                        detail: Some(RESOLVED_DETAIL.to_string()),
                        additional_text_edits: vec![wire::ResolvedTextEdit {
                            start: import_at,
                            end: import_at,
                            new_text: RESOLVED_IMPORT.to_string(),
                        }],
                        ..Default::default()
                    }),
                );
                armed.resolve_item = Some(tsgo_resolve_envelope_item(
                    crate::type_provider::traits::TypeProvider::provider_id(
                        fixture.provider.as_ref(),
                    ),
                    &path,
                    COMPLETION_LABEL,
                ));
            }
            Route::CodeAction => {
                let range = fixture.range("const msg");
                provider.set_code_actions(
                    &path,
                    fixture.tsx_offset(&ctx, range.start),
                    fixture.tsx_offset(&ctx, range.end),
                    vec![wire::TypeCodeAction {
                        title: CODE_ACTION_TITLE.to_string(),
                        kind: Some("quickfix".to_string()),
                        edits: vec![wire::TypeCodeEdit {
                            path: path.clone(),
                            start: literal_start,
                            end: literal_end,
                            new_text: "'goodbye'".to_string(),
                        }],
                    }],
                );
                armed.range = range;
            }
            Route::InlayHint => {
                let range = Range {
                    start: fixture.position("const msg", 0),
                    end: fixture.position("greet(msg)", 0),
                };
                let (msg_start, _) = fixture.tsx_span(&ctx, "msg = 'hello'");
                provider.set_inlay_hints(
                    &path,
                    fixture.tsx_offset(&ctx, range.start),
                    fixture.tsx_offset(&ctx, range.end),
                    vec![wire::InlayHint {
                        position: msg_start + "msg".len() as u32,
                        label: INLAY_LABEL.to_string(),
                        kind: Some(wire::InlayHintKind::Type),
                        padding_left: None,
                        padding_right: None,
                    }],
                );
                armed.range = range;
            }
            Route::SemanticTokens => {
                let (start, _) = fixture.tsx_span(&ctx, "msg = 'hello'");
                provider.set_semantic_tokens(
                    &path,
                    vec![wire::SemanticToken {
                        start,
                        length: "msg".len() as u32,
                        token_type: 7,
                        token_modifiers: 1,
                    }],
                );
            }
        }
        armed
    }

    /// Send this route's request and reduce the reply to an [`Outcome`].
    pub(super) async fn ask(self, fixture: &Fixture, armed: &Armed) -> Outcome {
        let server = fixture.server();
        let uri = fixture.uri.clone();
        let position = armed.position;
        let text_position = TextDocumentPositionParams {
            text_document: TextDocumentIdentifier { uri: uri.clone() },
            position,
        };
        let reply: tower_lsp_server::jsonrpc::Result<Option<String>> = match self {
            Route::Hover => server
                .hover(HoverParams {
                    text_document_position_params: text_position,
                    work_done_progress_params: Default::default(),
                })
                .await
                .map(|hover| {
                    hover.and_then(|hover| {
                        let text = match hover.contents {
                            HoverContents::Markup(markup) => markup.value,
                            other => format!("{other:?}"),
                        };
                        text.contains(HOVER_TEXT)
                            .then(|| format!("{:?} {text}", hover.range))
                    })
                }),
            Route::SignatureHelp => server
                .signature_help(SignatureHelpParams {
                    text_document_position_params: text_position,
                    work_done_progress_params: Default::default(),
                    context: None,
                })
                .await
                .map(|help| {
                    help.filter(|help| {
                        help.signatures
                            .iter()
                            .any(|signature| signature.label == SIGNATURE_LABEL)
                    })
                    .map(|help| format!("{help:?}"))
                }),
            Route::Definition => server
                .goto_definition(GotoDefinitionParams {
                    text_document_position_params: text_position,
                    work_done_progress_params: Default::default(),
                    partial_result_params: Default::default(),
                })
                .await
                .map(|response| literal_locations(response, &uri, armed.literal)),
            Route::TypeDefinition => server
                .goto_type_definition(GotoDefinitionParams {
                    text_document_position_params: text_position,
                    work_done_progress_params: Default::default(),
                    partial_result_params: Default::default(),
                })
                .await
                .map(|response| literal_locations(response, &uri, armed.literal)),
            Route::References => server
                .references(ReferenceParams {
                    text_document_position: text_position,
                    context: ReferenceContext {
                        include_declaration: true,
                    },
                    work_done_progress_params: Default::default(),
                    partial_result_params: Default::default(),
                })
                .await
                .map(|locations| {
                    literal_locations(
                        locations.map(GotoDefinitionResponse::Array),
                        &uri,
                        armed.literal,
                    )
                }),
            Route::DocumentHighlight => server
                .document_highlight(DocumentHighlightParams {
                    text_document_position_params: text_position,
                    work_done_progress_params: Default::default(),
                    partial_result_params: Default::default(),
                })
                .await
                .map(|highlights| {
                    let highlights = highlights.unwrap_or_default();
                    highlights
                        .iter()
                        .any(|highlight| highlight.range == armed.literal)
                        .then(|| format!("{highlights:?}"))
                }),
            Route::PrepareRename => server
                .prepare_rename(text_position)
                .await
                .map(|response| response.map(|response| format!("{response:?}"))),
            Route::Rename => server
                .rename(RenameParams {
                    text_document_position: text_position,
                    new_name: "message".to_string(),
                    work_done_progress_params: Default::default(),
                })
                .await
                .map(|edit| {
                    edit.map(|edit| rename_edit_ranges(&edit, &uri))
                        .filter(|ranges| !ranges.is_empty())
                        .map(|ranges| format!("{ranges:?}"))
                }),
            Route::Completion => server
                .completion(CompletionParams {
                    text_document_position: text_position,
                    work_done_progress_params: Default::default(),
                    partial_result_params: Default::default(),
                    context: Some(CompletionContext {
                        trigger_kind: CompletionTriggerKind::INVOKED,
                        trigger_character: None,
                    }),
                })
                .await
                .map(|response| {
                    let items = match response {
                        Some(CompletionResponse::Array(items)) => items,
                        Some(CompletionResponse::List(list)) => list.items,
                        None => Vec::new(),
                    };
                    items
                        .into_iter()
                        .find(|item| item.label == COMPLETION_LABEL)
                        .map(|item| format!("{:?} {:?}", item.label, item.text_edit))
                }),
            Route::CompletionResolve => {
                let item = armed
                    .resolve_item
                    .clone()
                    .expect("completion resolve is armed with an item");
                server.completion_resolve(item).await.map(|item| {
                    (item.detail.as_deref() == Some(RESOLVED_DETAIL))
                        .then(|| format!("{:?} {:?}", item.detail, item.additional_text_edits))
                })
            }
            Route::CodeAction => server
                .code_action(CodeActionParams {
                    text_document: TextDocumentIdentifier { uri: uri.clone() },
                    range: armed.range,
                    context: CodeActionContext {
                        diagnostics: Vec::new(),
                        only: Some(vec![CodeActionKind::QUICKFIX]),
                        trigger_kind: None,
                    },
                    work_done_progress_params: Default::default(),
                    partial_result_params: Default::default(),
                })
                .await
                .map(|actions| {
                    actions
                        .unwrap_or_default()
                        .into_iter()
                        .find_map(|action| match action {
                            CodeActionOrCommand::CodeAction(action)
                                if action.title == CODE_ACTION_TITLE =>
                            {
                                Some(format!("{:?}", action.edit))
                            }
                            _ => None,
                        })
                }),
            Route::InlayHint => server
                .inlay_hint(InlayHintParams {
                    text_document: TextDocumentIdentifier { uri: uri.clone() },
                    range: armed.range,
                    work_done_progress_params: Default::default(),
                })
                .await
                .map(|hints| {
                    hints
                        .unwrap_or_default()
                        .into_iter()
                        .find_map(|hint| match &hint.label {
                            InlayHintLabel::String(label) if label == INLAY_LABEL => {
                                Some(format!("{:?} {label}", hint.position))
                            }
                            _ => None,
                        })
                }),
            Route::SemanticTokens => server
                .semantic_tokens_full(SemanticTokensParams {
                    text_document: TextDocumentIdentifier { uri: uri.clone() },
                    work_done_progress_params: Default::default(),
                    partial_result_params: Default::default(),
                })
                .await
                .map(|tokens| match tokens {
                    Some(SemanticTokensResult::Tokens(tokens)) if !tokens.data.is_empty() => {
                        Some(format!("{:?}", tokens.data))
                    }
                    _ => None,
                }),
        };
        match reply {
            // Each fixture lives in its own temporary directory; answers from
            // different fixtures compare on everything but that root.
            Ok(Some(answer)) => {
                Outcome::Answered(answer.replace(&fixture.workspace_id, "<workspace>"))
            }
            Ok(None) => Outcome::Empty,
            Err(error) if error.code == tower_lsp_server::jsonrpc::ErrorCode::ContentModified => {
                Outcome::ContentModified
            }
            Err(error) => Outcome::Refused(format!("{error:?}")),
        }
    }
}

/// The provider's literal-position locations, or `None` when the reply holds
/// none.
fn literal_locations(
    response: Option<GotoDefinitionResponse>,
    uri: &Uri,
    literal: Range,
) -> Option<String> {
    let locations = match response? {
        GotoDefinitionResponse::Scalar(location) => vec![location],
        GotoDefinitionResponse::Array(locations) => locations,
        GotoDefinitionResponse::Link(links) => links
            .into_iter()
            .map(|link| Location {
                uri: link.target_uri,
                range: link.target_range,
            })
            .collect(),
    };
    let literal = locations
        .iter()
        .filter(|location| location.uri == *uri && location.range == literal)
        .collect::<Vec<_>>();
    (!literal.is_empty()).then(|| format!("{literal:?}"))
}

/// The answer `route` gives on an unmoved fixture: a fresh server, armed, asked
/// once with no schedule. Every other row compares against it.
pub(super) async fn reference_answer(route: Route) -> String {
    let fixture = Fixture::new().await;
    let armed = route.arm(&fixture).await;
    fixture.provider.clear_calls();
    match route.ask(&fixture, &armed).await {
        Outcome::Answered(answer) => answer,
        other => {
            panic!("{route:?}: an unmoved fixture must answer from the provider, got {other:?}")
        }
    }
}

/// Every route answers its armed provider contribution, through exactly one
/// provider dispatch, when nothing moves during the request — and reaches every
/// barrier the schedules move state at.
async fn assert_steady(route: Route) {
    every_route_has_rows(route);
    let fixture = Fixture::new().await;
    let armed = route.arm(&fixture).await;
    fixture.provider.clear_calls();
    fixture.barriers.clear();
    let outcome = route.ask(&fixture, &armed).await;
    let Outcome::Answered(answer) = &outcome else {
        panic!("{route:?}: an unmoved request must answer from the provider, got {outcome:?}");
    };
    assert!(!answer.is_empty(), "{route:?}: the answer is nonempty");
    assert_eq!(
        fixture.dispatches(route),
        1,
        "{route:?}: one provider dispatch per unmoved request"
    );
    for barrier in BARRIERS {
        assert!(
            fixture.barriers.arrivals(barrier) >= 1,
            "{route:?}: the request must reach {barrier:?}"
        );
    }
    let canonical_answer = match route {
        Route::Definition | Route::TypeDefinition | Route::References => {
            format!("{:?}", armed.literal)
        }
        _ => String::new(),
    };
    assert!(
        answer.contains(&canonical_answer),
        "{route:?}: the provider location maps onto the authored literal: {answer}"
    );
    if route == Route::CompletionResolve {
        assert!(
            answer.contains("messageFromProvider } from"),
            "the resolve places the provider's import edit: {answer}"
        );
    }
    if route == Route::Rename {
        assert_eq!(
            answer,
            &format!("{:?}", authored_token_ranges(APP, "msg")),
            "rename edits exactly the authored occurrences"
        );
    }
}

/// The four barriers every route reaches, in request order.
pub(super) const BARRIERS: [RequestBarrier; 4] = [
    RequestBarrier::Capture,
    RequestBarrier::ProviderDispatch,
    RequestBarrier::ProviderDecode,
    RequestBarrier::Settlement,
];
