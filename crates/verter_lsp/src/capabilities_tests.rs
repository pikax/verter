//! Tests for the advertised server-capability surface.
//!
//! The serialized `ServerCapabilities` JSON is the public wire contract of
//! the initialize handshake, so its bytes are asserted directly against the
//! golden below — the exact surface the handshake has always published. The
//! per-slot fold must reproduce it whole: a slot writing another slot's cell,
//! dropping a cell or altering a value fails here for every supported
//! position encoding and both resolve-provider values, which is the
//! sole-owner proof for each slot's cells.

use serde_json::json;
use tower_lsp_server::ls_types::PositionEncodingKind;
use verter_session::{HostConfig, VerterHost};

/// The advertised capabilities of a serving host, built the way the
/// initialize handler builds them: from THAT host's own composed
/// classification authority, never a process-global registry.
fn advertised_capabilities(
    encoding: &PositionEncodingKind,
    resolve_provider: bool,
) -> serde_json::Value {
    let host = VerterHost::new_standalone(HostConfig::default());
    serde_json::to_value(crate::capabilities::server_capabilities(
        encoding,
        resolve_provider,
        host.language_classifier(),
    ))
    .expect("server capabilities must serialize")
}

/// The wire-contract golden. `encoding` and `resolve_provider` are the two
/// negotiated inputs; the semantic-token legend is pinned by name and index
/// to the shared mapping owner's published arrays
/// (`verter_type_runtime::semantic_tokens`), so the golden cannot drift from
/// the vocabulary every provider lane remaps into.
fn golden_capabilities(encoding: &str, resolve_provider: bool) -> serde_json::Value {
    json!({
        "positionEncoding": encoding,
        "textDocumentSync": 2,
        "selectionRangeProvider": true,
        "hoverProvider": true,
        "completionProvider": {
            "resolveProvider": resolve_provider,
            "triggerCharacters": [".", "@", "<", ":", "#", " ", "\"", "'"],
            "completionItem": { "labelDetailsSupport": true },
        },
        "signatureHelpProvider": {
            "triggerCharacters": ["(", ","],
            "retriggerCharacters": [","],
        },
        "definitionProvider": true,
        "typeDefinitionProvider": true,
        "referencesProvider": true,
        "documentHighlightProvider": true,
        "documentSymbolProvider": true,
        "workspaceSymbolProvider": true,
        "codeActionProvider": {
            "codeActionKinds": [
                "source.organizeImports",
                "quickfix",
                "refactor",
                "refactor.extract",
            ],
        },
        "codeLensProvider": { "resolveProvider": false },
        "documentFormattingProvider": true,
        "documentOnTypeFormattingProvider": { "firstTriggerCharacter": ">" },
        "renameProvider": { "prepareProvider": true },
        "documentLinkProvider": { "resolveProvider": false },
        "colorProvider": true,
        "foldingRangeProvider": true,
        "workspace": {
            "workspaceFolders": { "supported": true, "changeNotifications": true },
            "fileOperations": {
                "didCreate": {
                    "filters": [
                        { "scheme": "file", "pattern": { "glob": "**/*.{svelte,vue}" } },
                    ],
                },
                "didDelete": {
                    "filters": [
                        { "scheme": "file", "pattern": { "glob": "**/*.{svelte,vue}" } },
                    ],
                },
            },
        },
        "callHierarchyProvider": true,
        "semanticTokensProvider": {
            "legend": {
                "tokenTypes": verter_type_runtime::semantic_tokens::VERTER_TOKEN_TYPES,
                "tokenModifiers": verter_type_runtime::semantic_tokens::VERTER_TOKEN_MODIFIERS,
            },
            "full": true,
        },
        "linkedEditingRangeProvider": true,
        "inlayHintProvider": true,
        "experimental": {
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
                "queryMethods": ["$/verter/audit/getRecord", "$/verter/audit/getRecent"],
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
            },
        },
    })
}

#[test]
fn advertised_capabilities_match_the_wire_contract_exactly() {
    for (encoding, wire_name) in [
        (PositionEncodingKind::UTF8, "utf-8"),
        (PositionEncodingKind::UTF16, "utf-16"),
        (PositionEncodingKind::UTF32, "utf-32"),
    ] {
        for resolve_provider in [true, false] {
            assert_eq!(
                advertised_capabilities(&encoding, resolve_provider),
                golden_capabilities(wire_name, resolve_provider),
                "advertised capabilities drifted from the wire contract \
                 (position encoding {wire_name}, resolve_provider {resolve_provider})"
            );
        }
    }
}
