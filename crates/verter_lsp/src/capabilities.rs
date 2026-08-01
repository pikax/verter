use serde_json::json;
use tower_lsp_server::ls_types::*;

/// Build the server capabilities to advertise during initialization.
///
/// `encoding` is the negotiated position encoding to announce to the client.
/// Watcher glob covering every registered framework-carrier extension,
/// built from `LanguageRegistry::carrier_extensions()` (e.g.
/// `**/*.{svelte,vue}`). Carrier rows without a registered carrier
/// implementation widen the glob too — their watched events are inert
/// (no virtual-file wiring exists for them), so watching is harmless
/// and the glob stays registry-derived rather than hand-enumerated.
pub(crate) fn carrier_watch_glob() -> String {
    let extensions = verter_session::LanguageRegistry::global().carrier_extensions();
    glob_for_extensions(&extensions)
}

/// Watcher glob covering every registered ADAPTER-MODULE extension across all
/// adapters (e.g. `**/*.{svelte.js,svelte.ts}`), built from
/// `LanguageRegistry::adapter_module_extensions(...)`. An adapter module is a
/// standalone NON-component rune module (`.svelte.ts` / `.svelte.js`) — NOT a
/// carrier — so it is NOT covered by [`carrier_watch_glob`]; this dedicated
/// glob is the descriptor-derived authority for its coverage (the generic
/// `**/*.{ts,tsx,…}` glob no longer carries rune-module responsibility).
/// Returns `None` when no adapter registers any module extension.
pub(crate) fn adapter_module_watch_glob() -> Option<String> {
    let registry = verter_session::LanguageRegistry::global();
    let extensions = registry.all_adapter_module_extensions();
    if extensions.is_empty() {
        return None;
    }
    Some(glob_for_extensions(&extensions))
}

/// Build a `**/*.{a,b,…}` watcher glob for a set of extensions (single-element
/// sets render as `**/*.ext`).
fn glob_for_extensions(extensions: &[&str]) -> String {
    match extensions {
        [single] => format!("**/*.{single}"),
        many => format!("**/*.{{{}}}", many.join(",")),
    }
}

/// Build the advertised server capabilities.
///
/// `resolve_provider` is the HONEST completion-resolve capability: it must
/// reflect whether the active type provider actually implements
/// `completionItem/resolve` (auto-import / lazy detail), not be hard-coded. A
/// session with no provider, or a provider without resolve support, advertises
/// `resolve_provider: false` so the client never sends resolve requests the
/// server would silently no-op.
pub fn server_capabilities(
    encoding: &PositionEncodingKind,
    resolve_provider: bool,
) -> ServerCapabilities {
    ServerCapabilities {
        position_encoding: Some(encoding.clone()),
        text_document_sync: Some(TextDocumentSyncCapability::Kind(
            TextDocumentSyncKind::INCREMENTAL,
        )),
        completion_provider: Some(CompletionOptions {
            resolve_provider: Some(resolve_provider),
            trigger_characters: Some(vec![
                ".".into(),
                "@".into(),
                "<".into(),
                ":".into(),
                "#".into(),
                " ".into(),
                "\"".into(),
                "'".into(),
            ]),
            completion_item: Some(CompletionOptionsCompletionItem {
                label_details_support: Some(true),
            }),
            ..Default::default()
        }),
        hover_provider: Some(HoverProviderCapability::Simple(true)),
        definition_provider: Some(OneOf::Left(true)),
        type_definition_provider: Some(TypeDefinitionProviderCapability::Simple(true)),
        references_provider: Some(OneOf::Left(true)),
        rename_provider: Some(OneOf::Right(RenameOptions {
            prepare_provider: Some(true),
            work_done_progress_options: Default::default(),
        })),
        // Pull diagnostics removed — we use push diagnostics exclusively.
        // Push diagnostics stay visible during typing (VS Code adjusts their positions
        // as the document changes), eliminating the flickering caused by pull diagnostics
        // returning stale/incomplete results during typing cooldown.
        document_symbol_provider: Some(OneOf::Left(true)),
        folding_range_provider: Some(FoldingRangeProviderCapability::Simple(true)),
        selection_range_provider: Some(SelectionRangeProviderCapability::Simple(true)),
        linked_editing_range_provider: Some(LinkedEditingRangeServerCapabilities::Simple(true)),
        document_link_provider: Some(DocumentLinkOptions {
            resolve_provider: Some(false),
            work_done_progress_options: Default::default(),
        }),
        document_highlight_provider: Some(OneOf::Left(true)),
        signature_help_provider: Some(SignatureHelpOptions {
            trigger_characters: Some(vec!["(".into(), ",".into()]),
            retrigger_characters: Some(vec![",".into()]),
            work_done_progress_options: Default::default(),
        }),
        color_provider: Some(ColorProviderCapability::Simple(true)),
        document_formatting_provider: Some(OneOf::Left(true)),
        // Proactive tag auto-close fires on the `>` that closes an open tag.
        // The handler (`auto_close_tag`) requires a `>` immediately before the
        // cursor, so `>` is the only trigger it can act on — no `/` more-trigger.
        document_on_type_formatting_provider: Some(DocumentOnTypeFormattingOptions {
            first_trigger_character: ">".to_string(),
            more_trigger_character: None,
        }),
        call_hierarchy_provider: Some(CallHierarchyServerCapability::Simple(true)),
        workspace_symbol_provider: Some(OneOf::Left(true)),
        code_action_provider: Some(CodeActionProviderCapability::Options(CodeActionOptions {
            code_action_kinds: Some(vec![
                CodeActionKind::SOURCE_ORGANIZE_IMPORTS,
                CodeActionKind::QUICKFIX,
                CodeActionKind::REFACTOR,
                CodeActionKind::REFACTOR_EXTRACT,
            ]),
            ..Default::default()
        })),
        code_lens_provider: Some(CodeLensOptions {
            resolve_provider: Some(false),
        }),
        inlay_hint_provider: Some(OneOf::Left(true)),
        // The advertised legend IS the shared mapping owner's published
        // vocabulary (`verter_type_runtime::semantic_tokens`): every provider
        // lane remaps its own token space into these exact indices/bits, so the
        // wire legend must be BUILT from the same arrays — a hand-maintained
        // copy here would silently re-introduce the index-space mismatch the
        // shared owner exists to prevent.
        semantic_tokens_provider: Some(SemanticTokensServerCapabilities::SemanticTokensOptions(
            SemanticTokensOptions {
                full: Some(SemanticTokensFullOptions::Bool(true)),
                legend: SemanticTokensLegend {
                    token_types: verter_type_runtime::semantic_tokens::VERTER_TOKEN_TYPES
                        .iter()
                        .map(|name| SemanticTokenType::new(name))
                        .collect(),
                    token_modifiers: verter_type_runtime::semantic_tokens::VERTER_TOKEN_MODIFIERS
                        .iter()
                        .map(|name| SemanticTokenModifier::new(name))
                        .collect(),
                },
                ..Default::default()
            },
        )),
        // Verter audit producer capability — surfaces the per-method
        // budgets and trace-output env-var so clients can opt in to
        // observability tooling. The shape is intentionally narrow;
        // version bumps come with a documented migration path.
        //
        // `queryMethods` advertises the read-only audit-query custom
        // LSP methods clients can call to inspect the host's records
        // store. They never mutate audit state.
        experimental: Some(json!({
            "verterAudit": {
                "version": 1,
                "kind": "Lsp",
                "methods": [
                    "hover",
                    "gotoDefinition",
                    "completion",
                    "references",
                    "diagnostics",
                    "documentSymbols",
                    "semanticTokens",
                    "inlayHints",
                    "codeAction",
                    "rename",
                ],
                "cancellationContract": "finalize-with-marker",
                "traceOutEnv": "VERTER_LSP_AUDIT_TRACE_OUT",
                "queryMethods": [
                    "$/verter/audit/getRecord",
                    "$/verter/audit/getRecent",
                ],
            }
        })),
        workspace: Some(WorkspaceServerCapabilities {
            workspace_folders: Some(WorkspaceFoldersServerCapabilities {
                supported: Some(true),
                change_notifications: Some(OneOf::Left(true)),
            }),
            file_operations: Some(WorkspaceFileOperationsServerCapabilities {
                did_create: Some(FileOperationRegistrationOptions {
                    filters: vec![FileOperationFilter {
                        scheme: Some("file".to_string()),
                        pattern: FileOperationPattern {
                            glob: carrier_watch_glob(),
                            matches: None,
                            options: None,
                        },
                    }],
                }),
                did_delete: Some(FileOperationRegistrationOptions {
                    filters: vec![FileOperationFilter {
                        scheme: Some("file".to_string()),
                        pattern: FileOperationPattern {
                            glob: carrier_watch_glob(),
                            matches: None,
                            options: None,
                        },
                    }],
                }),
                ..Default::default()
            }),
        }),
        ..Default::default()
    }
}
