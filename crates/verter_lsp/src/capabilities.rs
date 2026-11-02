use serde_json::json;
use tower_lsp_server::ls_types::*;
use verter_session::framework::HostLanguageClassifier;

mod code_actions;
mod code_lens;
mod color;
mod diagnostics;
mod document_links;
mod file_operations;
mod formatting;
mod navigation;
mod on_type_formatting;
mod semantic_tokens;
mod workspace;

/// Build the server capabilities to advertise during initialization.
///
/// `encoding` is the negotiated position encoding to announce to the client.
/// `framework` is the serving host's own composed classification authority:
/// the watcher globs below are derived from the registry THIS host built, so
/// the advertised framework surface cannot describe a framework set the host
/// does not serve. Reading a process-global registry instead would let a
/// watcher's surface and the host's surface disagree.
pub(crate) fn carrier_watch_glob(framework: &HostLanguageClassifier) -> String {
    glob_for_extensions(&framework.carrier_extensions())
}

/// Watcher glob covering every registered ADAPTER-MODULE extension across all
/// adapters (e.g. `**/*.{svelte.js,svelte.ts}`), built from the serving
/// host's own composed classification authority. An adapter module is a
/// standalone NON-component rune module (`.svelte.ts` / `.svelte.js`) — NOT a
/// carrier — so it is NOT covered by [`carrier_watch_glob`]; this dedicated
/// glob is the descriptor-derived authority for its coverage (the generic
/// `**/*.{ts,tsx,…}` glob no longer carries rune-module responsibility).
/// Returns `None` when the serving host classifies no adapter-module extension.
pub(crate) fn adapter_module_watch_glob(framework: &HostLanguageClassifier) -> Option<String> {
    let extensions = framework.adapter_module_extensions();
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

/// The inputs of the advertised capability surface: everything
/// [`server_capabilities`] itself and the per-slot contributors fold from.
/// New fields are added only when a capability cell actually needs them.
pub(crate) struct CapabilityContext<'a> {
    /// The negotiated position encoding to announce to the client.
    pub(crate) encoding: &'a PositionEncodingKind,
    /// The HONEST completion-resolve capability: it must reflect whether the
    /// active type provider actually implements `completionItem/resolve`
    /// (auto-import / lazy detail), not be hard-coded.
    pub(crate) resolve_provider: bool,
    /// The serving host's composed classification authority, read for the
    /// watcher globs (e.g. `**/*.{svelte,vue}`). Carrier rows without a
    /// registered carrier implementation widen the glob too — their watched
    /// events are inert (no virtual-file wiring exists for them), so watching
    /// is harmless and the glob stays registry-derived rather than
    /// hand-enumerated.
    pub(crate) framework: &'a HostLanguageClassifier,
}

/// Build the advertised server capabilities.
///
/// Cells without an owning slot module are set in the literal below; every
/// slot-owned cell is written only by its slot's `contribute`, folded in the
/// fixed order. The serialized result is the server's wire contract, so a
/// move must never change a cell's value —
/// `capabilities_tests.rs` pins it.
pub fn server_capabilities(
    encoding: &PositionEncodingKind,
    resolve_provider: bool,
    framework: &HostLanguageClassifier,
) -> ServerCapabilities {
    let ctx = CapabilityContext {
        encoding,
        resolve_provider,
        framework,
    };
    let mut caps = ServerCapabilities {
        position_encoding: Some(ctx.encoding.clone()),
        text_document_sync: Some(TextDocumentSyncCapability::Kind(
            TextDocumentSyncKind::INCREMENTAL,
        )),
        completion_provider: Some(CompletionOptions {
            resolve_provider: Some(ctx.resolve_provider),
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
        document_symbol_provider: Some(OneOf::Left(true)),
        folding_range_provider: Some(FoldingRangeProviderCapability::Simple(true)),
        document_highlight_provider: Some(OneOf::Left(true)),
        signature_help_provider: Some(SignatureHelpOptions {
            trigger_characters: Some(vec!["(".into(), ",".into()]),
            retrigger_characters: Some(vec![",".into()]),
            work_done_progress_options: Default::default(),
        }),
        call_hierarchy_provider: Some(CallHierarchyServerCapability::Simple(true)),
        workspace_symbol_provider: Some(OneOf::Left(true)),
        inlay_hint_provider: Some(OneOf::Left(true)),
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
            },
            "verterInteractionTrace": {
                "version": 1,
                "stages": [
                    "request_received",
                    "admitted",
                    "provider_work",
                    "serialize",
                    "outbound_enqueued",
                    "outbound_written",
                    "complete",
                ],
                "queryMethod": "$/verter/getStatistics",
                "initOption": "interactionTrace.enabled",
                "sourceLogging": "off-by-default",
                "clientStagesOwner": "WSP1L",
            }
        })),
        ..Default::default()
    };
    diagnostics::contribute(&mut caps, &ctx);
    workspace::contribute(&mut caps, &ctx);
    semantic_tokens::contribute(&mut caps, &ctx);
    navigation::contribute(&mut caps, &ctx);
    document_links::contribute(&mut caps, &ctx);
    color::contribute(&mut caps, &ctx);
    code_lens::contribute(&mut caps, &ctx);
    file_operations::contribute(&mut caps, &ctx);
    on_type_formatting::contribute(&mut caps, &ctx);
    formatting::contribute(&mut caps, &ctx);
    code_actions::contribute(&mut caps, &ctx);
    caps
}

#[cfg(test)]
#[path = "capabilities_tests.rs"]
mod tests;
