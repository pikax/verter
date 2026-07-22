//! Throwaway instrumentation: measure LSP-path future state-machine sizes.
//!
//! Run with:
//!   cargo test -p verter_lsp --lib future_size_measure -- --nocapture --ignored
//!   cargo test -p verter_lsp --lib future_size_measure --release -- --nocapture --ignored
//!
//! Not a gate. Numbers are printed and copied into docs/arch/future/* findings.

use std::mem::{size_of, size_of_val};
use std::sync::Arc;
use std::time::Duration;

use tower_lsp_server::ls_types::*;
use tower_lsp_server::LanguageServer;
use verter_session::{HostConfig, VerterHost};

use super::VerterLanguageServer;
use crate::type_provider::mock::MockTypeProvider;
use crate::type_provider::traits::TypeProvider;
use crate::{LspConfig, ProjectSyncMode, LSP_MAX_CONCURRENCY};

fn report(label: &str, bytes: usize) {
    eprintln!(
        "[future-size] {label}: {bytes} B ({:.1} KiB)",
        bytes as f64 / 1024.0
    );
}

/// Keeps the `LspService` alive so the inner server reference stays valid.
struct MeasureServer {
    service: tower_lsp_server::LspService<VerterLanguageServer>,
    _socket: tower_lsp_server::ClientSocket,
}

impl MeasureServer {
    fn new(audit_enabled: bool) -> Self {
        let mut config = HostConfig::default();
        config.audit_enabled = audit_enabled;
        let host = Arc::new(VerterHost::new_standalone(config));
        let provider: Arc<dyn TypeProvider> = Arc::new(MockTypeProvider::new());
        let host_for_server = Arc::clone(&host);
        let provider_for_server = Arc::clone(&provider);
        let (service, socket) = tower_lsp_server::LspService::new(move |client| {
            VerterLanguageServer::new(
                client,
                LspConfig {
                    host: Arc::clone(&host_for_server),
                    type_provider: Some(Arc::clone(&provider_for_server)),
                    project_sync_mode: ProjectSyncMode::FullProject,
                    type_provider_kind: crate::TypeProviderKind::Tsgo,
                    mcp_port: None,
                    type_provider_reason: Some("measure".into()),
                    suppress_imported_carrier_prewarm: true,
                },
            )
        });
        Self {
            service,
            _socket: socket,
        }
    }

    fn server(&self) -> &VerterLanguageServer {
        self.service.inner()
    }
}

fn open_synthetic(server: &VerterLanguageServer) -> (Uri, Position) {
    let path = "/tmp/verter-future-size/App.vue";
    let uri: Uri = format!("file://{path}").parse().expect("uri");
    let source = "<script setup lang=\"ts\">\nconst count = 1\n</script>\n<template><div>{{ count }}</div></template>\n";
    let _ = server.documents.did_open(&TextDocumentItem {
        uri: uri.clone(),
        language_id: "vue".to_string(),
        version: 1,
        text: source.to_string(),
    });
    // Position on `count` in the template — any in-range position is fine for sizing.
    let position = Position {
        line: 3,
        character: 18,
    };
    (uri, position)
}

fn goto_params(uri: &Uri, position: Position) -> GotoDefinitionParams {
    GotoDefinitionParams {
        text_document_position_params: TextDocumentPositionParams {
            text_document: TextDocumentIdentifier { uri: uri.clone() },
            position,
        },
        work_done_progress_params: Default::default(),
        partial_result_params: Default::default(),
    }
}

fn hover_params(uri: &Uri, position: Position) -> HoverParams {
    HoverParams {
        text_document_position_params: TextDocumentPositionParams {
            text_document: TextDocumentIdentifier { uri: uri.clone() },
            position,
        },
        work_done_progress_params: Default::default(),
    }
}

fn completion_params(uri: &Uri, position: Position) -> CompletionParams {
    CompletionParams {
        text_document_position: TextDocumentPositionParams {
            text_document: TextDocumentIdentifier { uri: uri.clone() },
            position,
        },
        work_done_progress_params: Default::default(),
        partial_result_params: Default::default(),
        context: Some(CompletionContext {
            trigger_kind: CompletionTriggerKind::INVOKED,
            trigger_character: None,
        }),
    }
}

fn references_params(uri: &Uri, position: Position) -> ReferenceParams {
    ReferenceParams {
        text_document_position: TextDocumentPositionParams {
            text_document: TextDocumentIdentifier { uri: uri.clone() },
            position,
        },
        work_done_progress_params: Default::default(),
        partial_result_params: Default::default(),
        context: ReferenceContext {
            include_declaration: true,
        },
    }
}

fn rename_params(uri: &Uri, position: Position) -> RenameParams {
    RenameParams {
        text_document_position: TextDocumentPositionParams {
            text_document: TextDocumentIdentifier { uri: uri.clone() },
            position,
        },
        work_done_progress_params: Default::default(),
        new_name: "renamed".into(),
    }
}

fn document_symbol_params(uri: &Uri) -> DocumentSymbolParams {
    DocumentSymbolParams {
        text_document: TextDocumentIdentifier { uri: uri.clone() },
        work_done_progress_params: Default::default(),
        partial_result_params: Default::default(),
    }
}

fn code_action_params(uri: &Uri) -> CodeActionParams {
    CodeActionParams {
        text_document: TextDocumentIdentifier { uri: uri.clone() },
        range: Range {
            start: Position {
                line: 0,
                character: 0,
            },
            end: Position {
                line: 0,
                character: 1,
            },
        },
        context: CodeActionContext {
            diagnostics: Vec::new(),
            only: None,
            trigger_kind: None,
        },
        work_done_progress_params: Default::default(),
        partial_result_params: Default::default(),
    }
}

fn inlay_hint_params(uri: &Uri) -> InlayHintParams {
    InlayHintParams {
        text_document: TextDocumentIdentifier { uri: uri.clone() },
        range: Range {
            start: Position {
                line: 0,
                character: 0,
            },
            end: Position {
                line: 4,
                character: 0,
            },
        },
        work_done_progress_params: Default::default(),
    }
}

fn semantic_tokens_params(uri: &Uri) -> SemanticTokensParams {
    SemanticTokensParams {
        text_document: TextDocumentIdentifier { uri: uri.clone() },
        work_done_progress_params: Default::default(),
        partial_result_params: Default::default(),
    }
}

fn signature_help_params(uri: &Uri, position: Position) -> SignatureHelpParams {
    SignatureHelpParams {
        text_document_position_params: TextDocumentPositionParams {
            text_document: TextDocumentIdentifier { uri: uri.clone() },
            position,
        },
        work_done_progress_params: Default::default(),
        context: None,
    }
}

fn document_highlight_params(uri: &Uri, position: Position) -> DocumentHighlightParams {
    DocumentHighlightParams {
        text_document_position_params: TextDocumentPositionParams {
            text_document: TextDocumentIdentifier { uri: uri.clone() },
            position,
        },
        work_done_progress_params: Default::default(),
        partial_result_params: Default::default(),
    }
}

/// Measure wrapper-layer sizes with a tiny body so layer cost is isolated.
#[tokio::test]
#[ignore = "throwaway instrumentation — run manually"]
async fn measure_wrapper_layer_sizes() {
    eprintln!("=== wrapper layers (tiny body) profile={} ===", profile());

    let tiny = async { Ok::<u8, tower_lsp_server::jsonrpc::Error>(7u8) };
    report("tiny async body", size_of_val(&tiny));

    let timeout_body = async { Ok::<u8, tower_lsp_server::jsonrpc::Error>(7u8) };
    let timed = tokio::time::timeout(Duration::from_secs(1), timeout_body);
    report("tokio::time::timeout(tiny)", size_of_val(&timed));

    let deadline_body = async { Ok::<u8, tower_lsp_server::jsonrpc::Error>(7u8) };
    let deadlined = verter_type_runtime::deadline::with_deadline(
        Duration::from_secs(1),
        tokio::time::timeout(Duration::from_secs(1), deadline_body),
    );
    report("with_deadline(timeout(tiny))", size_of_val(&deadlined));

    let run_deadline =
        crate::audit_harness::run_with_deadline(Duration::from_secs(1), async { Ok(7u8) });
    report("run_with_deadline(tiny)", size_of_val(&run_deadline));

    // Dummy host for run_with_audit.
    let mut config_on = HostConfig::default();
    config_on.audit_enabled = true;
    let host = Arc::new(VerterHost::new_standalone(config_on));
    let audit_on = crate::audit_harness::run_with_audit(
        &host,
        verter_audit::payloads::tags::LspMethodTag::GotoDefinition,
        "file:///tmp/x.vue".into(),
        Some(Position {
            line: 0,
            character: 0,
        }),
        async { Ok(7u8) },
        |_p, _v| {},
    );
    report("run_with_audit(tiny, audit_on)", size_of_val(&audit_on));
    drop(audit_on);

    let mut config_off = HostConfig::default();
    config_off.audit_enabled = false;
    let host_off = Arc::new(VerterHost::new_standalone(config_off));
    let audit_off = crate::audit_harness::run_with_audit(
        &host_off,
        verter_audit::payloads::tags::LspMethodTag::GotoDefinition,
        "file:///tmp/x.vue".into(),
        Some(Position {
            line: 0,
            character: 0,
        }),
        async { Ok(7u8) },
        |_p, _v| {},
    );
    report("run_with_audit(tiny, audit_off)", size_of_val(&audit_off));
    drop(audit_off);

    // Capacity × size for the serve-loop collection (outer slots are BoxFuture).
    type BoxFut =
        futures_util::future::BoxFuture<'static, Option<tower_lsp_server::jsonrpc::Response>>;
    report(
        "BoxFuture slot (buffer_unordered element)",
        size_of::<BoxFut>(),
    );
    report("LSP_MAX_CONCURRENCY", LSP_MAX_CONCURRENCY);
    report(
        "capacity×BoxFuture_slot (inline collection footprint)",
        LSP_MAX_CONCURRENCY * size_of::<BoxFut>(),
    );

    // If the handler future were held unboxed (counterfactual for Q3).
    // We re-measure a real handler below; here just document the formula using a
    // stand-in size that will be replaced by the measured audited handler size.
}

fn profile() -> &'static str {
    if cfg!(debug_assertions) {
        "debug"
    } else {
        "release"
    }
}

/// Measure real LSP handler futures on a synthetic SFC with a mock provider.
#[tokio::test]
#[ignore = "throwaway instrumentation — run manually"]
async fn measure_handler_future_sizes() {
    eprintln!("=== handler futures profile={} ===", profile());

    for audit in [false, true] {
        eprintln!("--- audit_enabled={audit} ---");
        let holder = MeasureServer::new(audit);
        let server = holder.server();
        let (uri, position) = open_synthetic(server);

        // Trait-method futures (what the router boxes into buffer_unordered).
        {
            let fut = server.goto_definition(goto_params(&uri, position));
            report("LanguageServer::goto_definition", size_of_val(&fut));
            drop(fut);
        }
        {
            let fut = server.hover(hover_params(&uri, position));
            report("LanguageServer::hover", size_of_val(&fut));
            drop(fut);
        }
        {
            let fut = server.completion(completion_params(&uri, position));
            report("LanguageServer::completion", size_of_val(&fut));
            drop(fut);
        }
        {
            let fut = server.references(references_params(&uri, position));
            report("LanguageServer::references", size_of_val(&fut));
            drop(fut);
        }
        {
            let fut = server.rename(rename_params(&uri, position));
            report("LanguageServer::rename", size_of_val(&fut));
            drop(fut);
        }
        {
            let fut = server.document_symbol(document_symbol_params(&uri));
            report("LanguageServer::document_symbol", size_of_val(&fut));
            drop(fut);
        }
        {
            let fut = server.code_action(code_action_params(&uri));
            report("LanguageServer::code_action", size_of_val(&fut));
            drop(fut);
        }
        {
            let fut = server.inlay_hint(inlay_hint_params(&uri));
            report("LanguageServer::inlay_hint", size_of_val(&fut));
            drop(fut);
        }
        {
            let fut = server.semantic_tokens_full(semantic_tokens_params(&uri));
            report("LanguageServer::semantic_tokens_full", size_of_val(&fut));
            drop(fut);
        }
        {
            let fut = server.signature_help(signature_help_params(&uri, position));
            report("LanguageServer::signature_help", size_of_val(&fut));
            drop(fut);
        }
        {
            let fut = server.document_highlight(document_highlight_params(&uri, position));
            report("LanguageServer::document_highlight", size_of_val(&fut));
            drop(fut);
        }
        {
            let fut = server.goto_type_definition(goto_params(&uri, position));
            report("LanguageServer::goto_type_definition", size_of_val(&fut));
            drop(fut);
        }
        {
            let fut = server.completion_resolve(CompletionItem {
                label: "x".into(),
                ..Default::default()
            });
            report("LanguageServer::completion_resolve", size_of_val(&fut));
            drop(fut);
        }

        // Direct body futures (inner layers).
        {
            let fut = super::nav_features_navigation::handle_goto_definition(
                &server,
                goto_params(&uri, position),
            );
            report("handle_goto_definition body", size_of_val(&fut));
            drop(fut);
        }
        {
            let fut = super::nav_features_audit::handle_goto_definition_with_audit(
                &server,
                goto_params(&uri, position),
            );
            report("handle_goto_definition_with_audit", size_of_val(&fut));
            // capacity × size using the measured audited handler
            report(
                "capacity×audited_goto_def (heap if each slot boxed, counterfactual if unboxed)",
                LSP_MAX_CONCURRENCY * size_of_val(&fut),
            );
            drop(fut);
        }
        {
            let fut = super::nav_features::handle_hover(&server, hover_params(&uri, position));
            report("handle_hover body", size_of_val(&fut));
            drop(fut);
        }
        {
            let fut = super::nav_features_audit::handle_hover_with_audit(
                &server,
                hover_params(&uri, position),
            );
            report("handle_hover_with_audit", size_of_val(&fut));
            drop(fut);
        }
        {
            let fut =
                super::nav_features::handle_completion(&server, completion_params(&uri, position));
            report("handle_completion body", size_of_val(&fut));
            drop(fut);
        }
        {
            let fut = super::nav_features_audit::handle_completion_with_audit(
                &server,
                completion_params(&uri, position),
            );
            report("handle_completion_with_audit", size_of_val(&fut));
            drop(fut);
        }
        {
            let fut = super::nav_features_navigation::handle_references(
                &server,
                references_params(&uri, position),
            );
            report("handle_references body", size_of_val(&fut));
            drop(fut);
        }
        {
            let fut = super::nav_features_audit::handle_references_with_audit(
                &server,
                references_params(&uri, position),
            );
            report("handle_references_with_audit", size_of_val(&fut));
            drop(fut);
        }
        {
            let fut = super::nav_features_navigation::handle_rename(
                &server,
                rename_params(&uri, position),
            );
            report("handle_rename body", size_of_val(&fut));
            drop(fut);
        }
        {
            let fut = super::nav_features_audit::handle_rename_with_audit(
                &server,
                rename_params(&uri, position),
            );
            report("handle_rename_with_audit", size_of_val(&fut));
            drop(fut);
        }
        {
            let fut = super::nav_features_navigation::handle_goto_type_definition(
                &server,
                goto_params(&uri, position),
            );
            report("handle_goto_type_definition body", size_of_val(&fut));
            drop(fut);
        }
        {
            let fut =
                super::aux_features::handle_document_symbol(&server, document_symbol_params(&uri));
            report("handle_document_symbol body", size_of_val(&fut));
            drop(fut);
        }
        {
            let fut = super::aux_features::handle_code_action(&server, code_action_params(&uri));
            report("handle_code_action body", size_of_val(&fut));
            drop(fut);
        }
        {
            let fut = super::aux_features::handle_inlay_hint(&server, inlay_hint_params(&uri));
            report("handle_inlay_hint body", size_of_val(&fut));
            drop(fut);
        }
        {
            let fut = super::aux_features::handle_semantic_tokens_full(
                &server,
                semantic_tokens_params(&uri),
            );
            report("handle_semantic_tokens_full body", size_of_val(&fut));
            drop(fut);
        }
        {
            let fut = super::aux_features::handle_signature_help(
                &server,
                signature_help_params(&uri, position),
            );
            report("handle_signature_help body", size_of_val(&fut));
            drop(fut);
        }
        {
            let fut = super::aux_features::handle_document_highlight(
                &server,
                document_highlight_params(&uri, position),
            );
            report("handle_document_highlight body", size_of_val(&fut));
            drop(fut);
        }

        // Sync chain futures polled from handlers, plus the handler-side
        // readiness join (the import-set pass itself is a detached spawned
        // task and never a handler-polled future).
        {
            let fut = server.ensure_current_file_synced(&uri);
            report("ensure_current_file_synced", size_of_val(&fut));
            drop(fut);
        }
        {
            let fut = server.dependency_readiness_join(&uri);
            report("dependency_readiness_join", size_of_val(&fut));
            drop(fut);
        }

        // Lifecycle notifications (also land on the serve loop).
        {
            let fut = server.did_open(DidOpenTextDocumentParams {
                text_document: TextDocumentItem {
                    uri: uri.clone(),
                    language_id: "vue".into(),
                    version: 2,
                    text: "x".into(),
                },
            });
            report("LanguageServer::did_open", size_of_val(&fut));
            drop(fut);
        }
        {
            let fut = server.did_change(DidChangeTextDocumentParams {
                text_document: VersionedTextDocumentIdentifier {
                    uri: uri.clone(),
                    version: 3,
                },
                content_changes: vec![TextDocumentContentChangeEvent {
                    range: None,
                    range_length: None,
                    text: "y".into(),
                }],
            });
            report("LanguageServer::did_change", size_of_val(&fut));
            drop(fut);
        }
    }

    // Provider hop (already boxed at the trait boundary) — independent of server.
    {
        let tp: Arc<dyn TypeProvider> = Arc::new(MockTypeProvider::new());
        let fut = tp.get_definition("synthetic.tsx", 0);
        report(
            "TypeProvider::get_definition (ProviderFuture)",
            size_of_val(&fut),
        );
        drop(fut);
        let fut = tp.get_hover("synthetic.tsx", 0);
        report(
            "TypeProvider::get_hover (ProviderFuture)",
            size_of_val(&fut),
        );
        drop(fut);
        let fut = tp.get_completions("synthetic.tsx", 0, None);
        report(
            "TypeProvider::get_completions (ProviderFuture)",
            size_of_val(&fut),
        );
        drop(fut);
    }

    // Key struct sizes for Q4 (locals held across await).
    eprintln!("=== struct sizes (locals / context carriers) ===");
    report(
        "size_of TypeProviderContext",
        size_of::<super::TypeProviderContext>(),
    );
    report(
        "size_of ProviderProjectionContext",
        size_of::<super::ProviderProjectionContext>(),
    );
    report(
        "size_of LspRequestPayload",
        size_of::<verter_audit::LspRequestPayload>(),
    );
    report(
        "size_of ProviderSurfaceSnapshot",
        size_of::<crate::provider_surface_store::ProviderSurfaceSnapshot>(),
    );
    report(
        "size_of LineIndex",
        size_of::<crate::documents::line_index::LineIndex>(),
    );
    report(
        "size_of ProviderPositionMapper",
        size_of::<crate::documents::provider_projection::ProviderPositionMapper>(),
    );
    report("size_of Uri", size_of::<Uri>());
    report(
        "size_of GotoDefinitionParams",
        size_of::<GotoDefinitionParams>(),
    );
    report("size_of HoverParams", size_of::<HoverParams>());
    report("size_of CompletionParams", size_of::<CompletionParams>());
    report("size_of Arc<str>", size_of::<Arc<str>>());
    report("size_of String", size_of::<String>());
}
