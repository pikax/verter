//! Headless transport-seam coverage for [`ExtensionTypeProvider`].
//!
//! Wired back as `#[cfg(test)] #[path = "extension_provider_tests.rs"] mod tests;`
//! from `extension_provider.rs`.
//!
//! The extension provider talks to the VS Code extension host over a single
//! `$/verter/tsQuery` request choke point. In production that choke point is a
//! concrete `tower_lsp_server::Client` ([`super::LspTsQueryTransport`]), which a
//! headless Rust test cannot drive — leaving the provider's completion /
//! resolve / diagnostics request envelopes covered ONLY by the (CI-disabled)
//! VS Code E2E job. The [`super::TsQueryTransport`] seam closes that gap: these
//! tests inject a [`ScriptedTsQueryTransport`] that RECORDS every emitted
//! `command + arguments` envelope and replays scripted response bodies, so the
//! provider's request shaping and its typed-result mapping are exercised
//! end-to-end with no live `Client`.
//!
//! Discrimination: the mock asserts each command matches the scripted
//! expectation in emission order and records the exact `arguments` JSON. If the
//! provider stopped routing through the transport (e.g. the seam wiring broke,
//! or a method emitted the wrong command / arg shape), the recorded-call
//! assertions and the per-command `assert_eq!` inside the mock both fail. The
//! test does NOT re-implement any tsserver-family mapping — it feeds raw
//! tsserver-shaped bodies and asserts the SHARED
//! `verter_type_runtime::tsserver::ipc` helpers (still the single owner) produce
//! the typed `Completion` / `CompletionResolveResult` / `TypeDiagnostic`
//! results.

use std::collections::VecDeque;
use std::future::Future;
use std::sync::{Arc, Mutex};

use serde_json::{json, Value};

use super::{ExtensionTypeProvider, TsQueryTransport};
use crate::server::TsQueryParams;
use crate::type_provider::protocol::*;
use crate::type_provider::traits::TypeProvider;

/// One recorded `$/verter/tsQuery` envelope.
#[derive(Debug, Clone)]
struct TsQueryCall {
    command: String,
    arguments: Value,
}

/// A contents-cache mutation to apply when a given command is served, used to
/// simulate a concurrent `update_file` landing mid-request.
struct ScriptedCacheMutation {
    /// Apply when this command is requested.
    command: String,
    /// Canonical cache key to overwrite.
    path: String,
    /// Replacement content.
    content: Arc<str>,
    /// Shared handle to the provider's contents cache.
    handle: Arc<tokio::sync::Mutex<std::collections::HashMap<String, Arc<str>>>>,
}

#[derive(Default)]
struct ScriptState {
    /// Every emitted envelope, in emission order.
    calls: Vec<TsQueryCall>,
    /// Scripted `(expected_command, response_body)` pairs, popped FIFO.
    responses: VecDeque<(String, Value)>,
    /// Cache mutations to apply (FIFO) when their command is served.
    mutations: VecDeque<ScriptedCacheMutation>,
}

/// A scripted, in-memory [`TsQueryTransport`] for headless provider tests.
///
/// Records every emitted command + arguments and returns the next scripted
/// response body, asserting the command matches the scripted expectation (so a
/// mis-ordered or mis-named request fails loudly inside the transport).
#[derive(Clone, Default)]
struct ScriptedTsQueryTransport {
    state: Arc<Mutex<ScriptState>>,
}

impl ScriptedTsQueryTransport {
    fn new() -> Self {
        Self::default()
    }

    /// Queue a `(expected_command, response_body)` pair.
    fn push_response(&self, command: &str, body: Value) {
        self.state
            .lock()
            .unwrap()
            .responses
            .push_back((command.to_string(), body));
    }

    /// Queue a contents-cache mutation applied just before `command`'s response
    /// is returned — simulating a concurrent `update_file` during the await.
    fn push_cache_mutation(
        &self,
        command: &str,
        path: &str,
        content: &str,
        handle: Arc<tokio::sync::Mutex<std::collections::HashMap<String, Arc<str>>>>,
    ) {
        self.state
            .lock()
            .unwrap()
            .mutations
            .push_back(ScriptedCacheMutation {
                command: command.to_string(),
                path: path.to_string(),
                content: Arc::from(content),
                handle,
            });
    }

    /// Snapshot every recorded envelope in emission order.
    fn calls(&self) -> Vec<TsQueryCall> {
        self.state.lock().unwrap().calls.clone()
    }

    /// All recorded commands, in emission order.
    fn commands(&self) -> Vec<String> {
        self.calls().into_iter().map(|c| c.command).collect()
    }

    /// The recorded arguments for the FIRST envelope with `command`.
    fn first_args(&self, command: &str) -> Value {
        self.calls()
            .into_iter()
            .find(|c| c.command == command)
            .unwrap_or_else(|| panic!("no `{command}` envelope was emitted"))
            .arguments
    }
}

impl TsQueryTransport for ScriptedTsQueryTransport {
    fn ts_query(
        &self,
        params: TsQueryParams,
    ) -> impl Future<Output = Result<Value, TypeProviderError>> + Send + '_ {
        let result = {
            let mut state = self.state.lock().unwrap();
            state.calls.push(TsQueryCall {
                command: params.command.clone(),
                arguments: params.arguments.clone(),
            });
            // Apply a queued cache mutation for this command BEFORE returning the
            // response, so the provider's per-response snapshot (taken after this
            // await) sees the updated content — modelling a concurrent
            // `update_file` landing while the request was in flight. The provider
            // holds no cache lock at the await point, so `try_lock` succeeds.
            if let Some(mutation) = state
                .mutations
                .pop_front_if(|m| m.command == params.command)
            {
                let mut cache = mutation
                    .handle
                    .try_lock()
                    .expect("provider holds no cache lock at the request await point");
                cache.insert(mutation.path, mutation.content);
            }
            match state.responses.pop_front() {
                Some((expected, body)) => {
                    assert_eq!(
                        params.command, expected,
                        "extension provider emitted `{}` but the script expected `{expected}`",
                        params.command
                    );
                    Ok(body)
                }
                None => Err(TypeProviderError::new(format!(
                    "no scripted response for `{}`",
                    params.command
                ))),
            }
        };
        std::future::ready(result)
    }
}

/// Drives `ExtensionTypeProvider` through the mock transport across the full
/// completion → completion-details → resolve → diagnostics flow, asserting both
/// the emitted `$/verter/tsQuery` request envelopes and the typed results.
///
/// This is the discriminating headless proof of the transport seam: every
/// assertion below depends on the provider routing each method through
/// `TsQueryTransport::ts_query`. Reverting the seam (binding `query()` back to a
/// concrete `Client`) makes the provider impossible to construct with the mock,
/// and any drift in the emitted command/args shape trips either the recorded-call
/// assertions here or the per-command `assert_eq!` inside the mock.
#[tokio::test]
async fn extension_provider_transport_mock_drives_completion_resolve_and_diagnostics() {
    // `/`-rooted absolute paths are canonicalize_path no-ops, so the recorded
    // `file` arg equals the input and same-file resolve-edit matching holds.
    let file = "/workspace/src/entry.ts";
    let content = "myHelper\n"; // 8-byte symbol; cursor after it is byte offset 8.

    let transport = ScriptedTsQueryTransport::new();

    // open_file → "open"
    transport.push_response("open", json!({}));
    // get_completions → "completionInfo"
    transport.push_response(
        "completionInfo",
        json!({
            "isMemberCompletion": false,
            "entries": [
                {
                    "name": "myHelper",
                    "kind": "const",
                    "sortText": "0",
                    "source": "./helper",
                    "data": { "exportName": "myHelper", "moduleSpecifier": "./helper" }
                }
            ]
        }),
    );
    // get_completion_details → "completionEntryDetails"
    transport.push_response(
        "completionEntryDetails",
        json!([
            {
                "name": "myHelper",
                "kind": "const",
                "displayParts": [{ "text": "const myHelper: () => void" }],
                "documentation": [{ "text": "A helper." }]
            }
        ]),
    );
    // resolve_completion → "completionEntryDetails" (with auto-import codeActions)
    transport.push_response(
        "completionEntryDetails",
        json!([
            {
                "name": "myHelper",
                "kind": "const",
                "displayParts": [{ "text": "const myHelper: () => void" }],
                "documentation": [{ "text": "A helper." }],
                "codeActions": [
                    {
                        "description": "Add import from \"./helper\"",
                        "changes": [
                            {
                                "fileName": file,
                                "textChanges": [
                                    {
                                        "start": { "line": 1, "offset": 1 },
                                        "end": { "line": 1, "offset": 1 },
                                        "newText": "import { myHelper } from \"./helper\";\n"
                                    }
                                ]
                            }
                        ]
                    }
                ]
            }
        ]),
    );
    // get_diagnostics → three passes, with a duplicate to prove dedup.
    transport.push_response(
        "semanticDiagnosticsSync",
        json!([
            {
                "text": "Type 'string' is not assignable to type 'number'.",
                "category": "error",
                "code": 2322,
                "start": { "line": 1, "offset": 1 },
                "end": { "line": 1, "offset": 9 }
            }
        ]),
    );
    transport.push_response(
        "syntacticDiagnosticsSync",
        json!([
            {
                "text": "';' expected.",
                "category": "error",
                "code": 1005,
                "start": { "line": 1, "offset": 9 },
                "end": { "line": 1, "offset": 9 }
            }
        ]),
    );
    transport.push_response(
        "suggestionDiagnosticsSync",
        json!([
            {
                "text": "'myHelper' is declared but its value is never read.",
                "category": "suggestion",
                "code": 6133,
                "start": { "line": 1, "offset": 1 },
                "end": { "line": 1, "offset": 9 }
            },
            // Duplicate of the semantic diagnostic (same span/code/message) —
            // merge_diagnostic_sets must collapse it.
            {
                "text": "Type 'string' is not assignable to type 'number'.",
                "category": "error",
                "code": 2322,
                "start": { "line": 1, "offset": 1 },
                "end": { "line": 1, "offset": 9 }
            }
        ]),
    );

    let provider = ExtensionTypeProvider::with_transport(transport.clone(), "/workspace");

    // ── open ────────────────────────────────────────────────────────────
    provider
        .open_file(file, content)
        .await
        .expect("open_file routes through the mock transport");

    // ── completions ─────────────────────────────────────────────────────
    let completions = provider
        .get_completions(file, 8, None)
        .await
        .expect("get_completions routes through the mock transport");

    // The emitted `completionInfo` envelope carries the converted position and
    // the auto-import-enabling flags.
    let ci_args = transport.first_args("completionInfo");
    assert_eq!(ci_args["file"], json!(file));
    assert_eq!(ci_args["line"], json!(1), "byte offset 8 → line 1");
    assert_eq!(ci_args["offset"], json!(9), "byte offset 8 → 1-based col 9");
    assert_eq!(ci_args["includeExternalModuleExports"], json!(true));
    assert_eq!(ci_args["includeInsertTextCompletions"], json!(true));

    assert_eq!(completions.items.len(), 1);
    let item = &completions.items[0];
    assert_eq!(item.label, "myHelper");
    // The parsed entry carries the tsserver resolve handle, stamped with the
    // completion-site byte offset (8), with the auto-import `source`.
    match item
        .data
        .as_ref()
        .expect("completion carries a resolve handle")
    {
        CompletionResolveData::TsserverEntry {
            name,
            source,
            offset,
            data,
        } => {
            assert_eq!(name, "myHelper");
            assert_eq!(source.as_deref(), Some("./helper"));
            assert_eq!(
                *offset, 8,
                "the request byte offset is stamped on the handle"
            );
            assert!(data.is_some(), "the entry's resolve `data` is preserved");
        }
        other => panic!("expected a TsserverEntry resolve handle, got {other:?}"),
    }

    // ── completion details ──────────────────────────────────────────────
    let enriched = provider
        .get_completion_details(file, 8, &completions.items)
        .await
        .expect("get_completion_details routes through the mock transport");

    // The raw `completionEntryDetails` request forwards the entry's `source`
    // and `data` (so an auto-import entry resolves against the right module).
    let ced_args = transport.first_args("completionEntryDetails");
    let entry0 = &ced_args["entryNames"][0];
    assert_eq!(entry0["name"], json!("myHelper"));
    assert_eq!(
        entry0["source"],
        json!("./helper"),
        "entryNames[0].source must be forwarded from the resolve handle"
    );
    assert_eq!(
        entry0["data"],
        json!({ "exportName": "myHelper", "moduleSpecifier": "./helper" }),
        "entryNames[0].data must be forwarded from the resolve handle"
    );
    assert_eq!(enriched.len(), 1);
    assert_eq!(
        enriched[0].detail.as_deref(),
        Some("const myHelper: () => void"),
        "displayParts enrich the item detail"
    );
    assert_eq!(enriched[0].documentation.as_deref(), Some("A helper."));

    // ── resolve (auto-import on accept) ─────────────────────────────────
    let resolve_data = enriched[0]
        .data
        .clone()
        .expect("the enriched item keeps its resolve handle");
    let resolved = provider
        .resolve_completion(file, resolve_data)
        .await
        .expect("resolve_completion routes through the mock transport")
        .expect("the scripted codeActions produce a resolve result");

    assert_eq!(
        resolved.additional_text_edits.len(),
        1,
        "the same-file auto-import edit is returned"
    );
    let edit = &resolved.additional_text_edits[0];
    assert_eq!(edit.start, 0, "line 1/offset 1 → byte 0");
    assert_eq!(edit.end, 0);
    assert_eq!(edit.new_text, "import { myHelper } from \"./helper\";\n");
    assert_eq!(
        resolved.detail.as_deref(),
        Some("const myHelper: () => void")
    );
    assert_eq!(resolved.documentation.as_deref(), Some("A helper."));

    // ── diagnostics (semantic ∪ syntactic ∪ suggestion, deduped) ────────
    let diagnostics = provider
        .get_diagnostics(file)
        .await
        .expect("get_diagnostics routes through the mock transport");

    // All three diagnostic passes were emitted, in order, after the resolve.
    let commands = transport.commands();
    assert_eq!(
        commands,
        vec![
            "open".to_string(),
            "completionInfo".to_string(),
            "completionEntryDetails".to_string(),
            "completionEntryDetails".to_string(),
            "semanticDiagnosticsSync".to_string(),
            "syntacticDiagnosticsSync".to_string(),
            "suggestionDiagnosticsSync".to_string(),
        ],
        "the provider routes every method through the transport, in order"
    );

    // The merged set has all three categories with the duplicate collapsed:
    // semantic(2322) + syntactic(1005) + suggestion(6133) = 3 (the duplicate
    // 2322 in the suggestion pass is dropped).
    assert_eq!(
        diagnostics.len(),
        3,
        "duplicate (same span/code/message) collapsed by merge_diagnostic_sets"
    );
    let codes: Vec<Option<&str>> = diagnostics.iter().map(|d| d.code.as_deref()).collect();
    assert_eq!(codes, vec![Some("2322"), Some("1005"), Some("6133")]);
    // Categories map to the right severities (suggestion → Hint).
    assert!(matches!(
        diagnostics[0].severity,
        TypeDiagnosticSeverity::Error
    ));
    assert!(matches!(
        diagnostics[2].severity,
        TypeDiagnosticSeverity::Hint
    ));
}

/// Negative discrimination: a `Lsp`-shaped resolve key (the upstream-LSP /
/// TSGO handle) cannot have come from the extension provider, so
/// `resolve_completion` fails closed WITHOUT emitting a transport request.
#[tokio::test]
async fn extension_provider_resolve_rejects_non_tsserver_handle_without_transport_call() {
    let transport = ScriptedTsQueryTransport::new();
    let provider = ExtensionTypeProvider::with_transport(transport.clone(), "/workspace");

    let resolved = provider
        .resolve_completion(
            "/workspace/src/entry.ts",
            CompletionResolveData::Lsp {
                label: "myHelper".to_string(),
                data: json!({ "anything": true }),
            },
        )
        .await
        .expect("a non-tsserver handle fails closed, not errors");

    assert!(
        resolved.is_none(),
        "a non-tsserver resolve handle yields no result"
    );
    assert!(
        transport.calls().is_empty(),
        "fail-closed resolve must not emit a $/verter/tsQuery request"
    );
}

/// F2 (review finding): the extension provider's `get_code_actions` must surface
/// BOTH the single "Remove unused declaration" fix AND its combined "Delete all
/// unused declarations" companion. The companion requires the provider to (a) read
/// the `fixId` + `fixAllDescription` from the `getCodeFixes` response and (b) follow
/// up with a `getCombinedCodeFix` request carrying the SHARED
/// `combined_code_fix_args` scope shape.
///
/// This drives the actual Rust combined-fix loop end-to-end through the mock
/// transport. Discriminating three ways:
///   * if the provider stopped reading `fixId` from the bridge response, the
///     combined branch would never run and only ONE action would come back;
///   * the test asserts the emitted `getCombinedCodeFix` args EXACTLY match
///     `combined_code_fix_args(file, fix_id)` — `{ scope: { type, args: { file } },
///     fixId }` — so a drifted arg shape fails loudly;
///   * the combined action's title comes from `fixAllDescription` (never a
///     title-string match), proving the typed fix-all path.
#[tokio::test]
async fn extension_provider_get_code_actions_surfaces_single_and_combined_unused_fix() {
    use verter_type_runtime::tsserver::ipc::combined_code_fix_args;

    let file = "/workspace/src/entry.ts";
    // `const unused = 1` — TS6133 fires at the decl. Byte offsets 6..11 cover the
    // identifier `unused` (the diagnostic span the handler forwards).
    let content = "const unused = 1;\n";

    let transport = ScriptedTsQueryTransport::new();

    // open_file → "open" (populates the provider's content cache so byte offsets
    // convert to 1-based tsserver positions).
    transport.push_response("open", json!({}));

    // getCodeFixes → the single "Remove unused declaration" fix, carrying the
    // typed `fixId` + `fixAllDescription` the bridge now forwards.
    transport.push_response(
        "getCodeFixes",
        json!([
            {
                "description": "Remove unused declaration for: 'unused'",
                "fixId": "unusedIdentifier_delete",
                "fixAllDescription": "Delete all unused declarations",
                "changes": [
                    {
                        "fileName": file,
                        "textChanges": [
                            {
                                "start": { "line": 1, "offset": 1 },
                                "end": { "line": 1, "offset": 18 },
                                "newText": ""
                            }
                        ]
                    }
                ]
            }
        ]),
    );

    // getCombinedCodeFix → the "fix all" companion edits for that fixId.
    transport.push_response(
        "getCombinedCodeFix",
        json!({
            "changes": [
                {
                    "fileName": file,
                    "textChanges": [
                        {
                            "start": { "line": 1, "offset": 1 },
                            "end": { "line": 1, "offset": 18 },
                            "newText": ""
                        }
                    ]
                }
            ]
        }),
    );

    let provider = ExtensionTypeProvider::with_transport(transport.clone(), "/workspace");

    provider
        .open_file(file, content)
        .await
        .expect("open_file routes through the mock transport");

    // The diagnostic context: TS6133 over the `unused` identifier (byte 6..11).
    let diag = ProviderDiagnosticContext {
        code: 6133,
        start: 6,
        end: 11,
    };
    let actions = provider
        .get_code_actions(file, 6, 11, &[diag])
        .await
        .expect("get_code_actions routes through the mock transport");

    // The provider followed getCodeFixes with a getCombinedCodeFix, in order.
    let commands = transport.commands();
    assert_eq!(
        commands,
        vec![
            "open".to_string(),
            "getCodeFixes".to_string(),
            "getCombinedCodeFix".to_string(),
        ],
        "the provider must follow getCodeFixes with a getCombinedCodeFix for the combinable fixId"
    );

    // The getCodeFixes request carried the deduped numeric error code.
    let cf_args = transport.first_args("getCodeFixes");
    assert_eq!(cf_args["errorCodes"], json!([6133]));

    // The getCombinedCodeFix request shape EXACTLY matches the shared
    // `combined_code_fix_args(file, fix_id)` — proving the provider does not
    // hand-roll the scope shape.
    let combined_args = transport.first_args("getCombinedCodeFix");
    assert_eq!(
        combined_args,
        combined_code_fix_args(file, "unusedIdentifier_delete"),
        "the combined-fix request must use the shared combined_code_fix_args scope shape"
    );
    assert_eq!(combined_args["scope"]["type"], json!("file"));
    assert_eq!(combined_args["scope"]["args"]["file"], json!(file));
    assert_eq!(combined_args["fixId"], json!("unusedIdentifier_delete"));

    // BOTH actions surface: the single deletion AND the combined "Delete all
    // unused declarations" (titled from `fixAllDescription`).
    let titles: Vec<&str> = actions.iter().map(|a| a.title.as_str()).collect();
    assert!(
        titles
            .iter()
            .any(|t| t.contains("Remove unused declaration")),
        "the single remove-unused fix must be surfaced, got {titles:?}"
    );
    assert!(
        titles.contains(&"Delete all unused declarations"),
        "the combined fix-all companion (titled from fixAllDescription) must be surfaced, \
         got {titles:?}"
    );
    assert_eq!(
        actions.len(),
        2,
        "exactly the single fix and its combined companion, got {titles:?}"
    );
    // The combined action carries the deletion edit (empty new_text).
    let combined = actions
        .iter()
        .find(|a| a.title == "Delete all unused declarations")
        .expect("combined action present");
    assert_eq!(combined.edits.len(), 1, "the combined fix carries its edit");
    assert!(
        combined.edits[0].new_text.is_empty(),
        "the combined deletion edit has empty new_text"
    );
}

/// The combined "fix all" branch converts each `getCombinedCodeFix` response's
/// edit offsets against content current as of THAT response, not a snapshot
/// taken once before the loop. A concurrent `update_file` landing while the
/// combined request is in flight must be reflected when the response is parsed.
///
/// Discriminating: the combined edit targets a position (line 3) that exists
/// only in the UPDATED content. A snapshot taken before the loop holds the
/// original single-line content, whose codec clamps the past-EOF line to a
/// different (EOF) byte offset; the fresh per-response snapshot resolves the
/// line-3 position to its real byte offset. The assertion pins that real offset.
#[tokio::test]
async fn extension_provider_combined_fix_uses_content_current_as_of_each_response() {
    let file = "/workspace/src/entry.ts";
    let original = "const unused = 1;\n";
    // Three lines; the combined edit targets line 3. Byte 12 is the start of the
    // third line (`line0\n` = 6 bytes, `line1\n` = 6 bytes).
    let updated = "line0\nline1\nDELETE_ME = 1;\n";
    let line3_start: u32 = 12;
    assert_eq!(
        updated.as_bytes()[line3_start as usize], b'D',
        "byte 12 is the start of the third line in the updated content"
    );

    let transport = ScriptedTsQueryTransport::new();
    transport.push_response("open", json!({}));
    // Single fix on line 1 — valid against both the original and updated content.
    transport.push_response(
        "getCodeFixes",
        json!([
            {
                "description": "Remove unused declaration for: 'unused'",
                "fixId": "unusedIdentifier_delete",
                "fixAllDescription": "Delete all unused declarations",
                "changes": [
                    {
                        "fileName": file,
                        "textChanges": [
                            {
                                "start": { "line": 1, "offset": 1 },
                                "end": { "line": 1, "offset": 6 },
                                "newText": ""
                            }
                        ]
                    }
                ]
            }
        ]),
    );
    // Combined response edits LINE 3 — only resolvable against the updated content.
    transport.push_response(
        "getCombinedCodeFix",
        json!({
            "changes": [
                {
                    "fileName": file,
                    "textChanges": [
                        {
                            "start": { "line": 3, "offset": 1 },
                            "end": { "line": 3, "offset": 10 },
                            "newText": ""
                        }
                    ]
                }
            ]
        }),
    );

    let provider = ExtensionTypeProvider::with_transport(transport.clone(), "/workspace");
    provider
        .open_file(file, original)
        .await
        .expect("open_file routes through the mock transport");

    // The concurrent edit lands while the combined request is in flight.
    transport.push_cache_mutation(
        "getCombinedCodeFix",
        &verter_span::path::canonicalize_path(file),
        updated,
        provider.contents_handle_for_test(),
    );

    let diag = ProviderDiagnosticContext {
        code: 6133,
        start: 6,
        end: 11,
    };
    let actions = provider
        .get_code_actions(file, 6, 11, &[diag])
        .await
        .expect("get_code_actions routes through the mock transport");

    let combined = actions
        .iter()
        .find(|a| a.title == "Delete all unused declarations")
        .expect("the combined fix-all action surfaces");
    assert_eq!(combined.edits.len(), 1, "the combined fix carries its edit");
    assert_eq!(
        combined.edits[0].start, line3_start,
        "the combined edit's offset must be computed against content current as of the response \
         (the line-3 start at byte {line3_start}), not a stale pre-loop snapshot"
    );
}
