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
            // (Front-check + pop instead of `VecDeque::pop_front_if`: the
            // latter is unstable on the pinned stable toolchain.)
            let front_matches = state
                .mutations
                .front()
                .is_some_and(|m| m.command == params.command);
            if let Some(mutation) = front_matches.then(|| state.mutations.pop_front()).flatten() {
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
        .get_completions(
            &crate::type_provider::traits::ProviderQuery::at_engine_surface(file),
            8,
            None,
        )
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
        .get_completion_details(
            &crate::type_provider::traits::ProviderQuery::at_engine_surface(file),
            8,
            &completions.items,
        )
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
        .resolve_completion(
            &crate::type_provider::traits::ProviderQuery::at_engine_surface(file),
            resolve_data,
        )
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
            &crate::type_provider::traits::ProviderQuery::at_engine_surface(
                "/workspace/src/entry.ts",
            ),
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
        .get_code_actions(
            &crate::type_provider::traits::ProviderQuery::at_engine_surface(file),
            6,
            11,
            &[diag],
        )
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

/// The combined "fix all" branch decodes each `getCombinedCodeFix` response's
/// edit offsets against the bytes the file held when THAT request was sent,
/// never against the cache re-read after the answer: a concurrent
/// `update_file` landing while the combined request is in flight must not
/// move the decode under it.
///
/// Discriminating: the combined edit names line 1, columns 7..13 — `unused`
/// in the bytes the request was sent with. Re-reading the cache after the
/// answer finds the replacement, whose first line (`line0`) has no column 7:
/// the strict converter drops the edit and the combined action never
/// surfaces.
#[tokio::test]
async fn extension_provider_combined_fix_decodes_against_the_bytes_it_was_sent_with() {
    let file = "/workspace/src/entry.ts";
    let original = "const unused = 1;\n";
    let updated = "line0\nline1\nDELETE_ME = 1;\n";
    let unused_start = original.find("unused").expect("fixture names unused") as u32;

    let transport = ScriptedTsQueryTransport::new();
    transport.push_response("open", json!({}));
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
    transport.push_response(
        "getCombinedCodeFix",
        json!({
            "changes": [
                {
                    "fileName": file,
                    "textChanges": [
                        {
                            "start": { "line": 1, "offset": 7 },
                            "end": { "line": 1, "offset": 13 },
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
        .get_code_actions(
            &crate::type_provider::traits::ProviderQuery::at_engine_surface(file),
            6,
            11,
            &[diag],
        )
        .await
        .expect("get_code_actions routes through the mock transport");

    let combined = actions
        .iter()
        .find(|a| a.title == "Delete all unused declarations")
        .expect("the combined fix-all action surfaces");
    assert_eq!(combined.edits.len(), 1, "the combined fix carries its edit");
    assert_eq!(
        (combined.edits[0].start, combined.edits[0].end),
        (unused_start, unused_start + 6),
        "the combined edit decodes against the bytes its request was sent with"
    );
}

// ── `projectRootPath`: the producer half of project-bound resolution ──
//
// The extension host resolves each file's TypeScript from the root the provider
// stamps on `open` / `updateOpen`. These tests drive the PRODUCTION producer
// (`ExtensionTypeProvider::open_file` / `update_file`) over the real workspace
// snapshot and assert the emitted envelope, so "the registry binds the declared
// root" is backed by proof that the declared root is the OWNING PROJECT's.
//
// Discrimination: the fixture is a single-folder pnpm monorepo — one workspace
// folder (`/ws`), a nested configured package (`/ws/packages/app`). Deriving the
// root from workspace folders yields `/ws` for every file in it, so a provider
// that stamps a folder-derived root fails every assertion below.

/// The provider as production wires it for a single-folder monorepo: one
/// workspace folder, and the snapshot-backed configured-owner authority.
///
/// `nested_config` is the config FILE that defines the nested package's
/// configured project. It is a parameter because the project's identity is that
/// exact file, not the literal name `tsconfig.json` — a package configured by
/// `jsconfig.json` or `tsconfig.app.json` is just as configured, and must be
/// declared just as precisely.
async fn monorepo_provider_with_config(
    transport: ScriptedTsQueryTransport,
    nested_config: &str,
) -> ExtensionTypeProvider<ScriptedTsQueryTransport> {
    let provider = ExtensionTypeProvider::with_transport(transport, "/ws");

    // Exactly what `background_init` sends: the editor's workspace FOLDERS.
    provider
        .update_workspace_folders(vec![json!({ "uri": "file:///ws", "name": "ws" })], vec![])
        .await
        .expect("workspace folders sync");

    let resolver = verter_resolution::ModuleResolverCore::new(vec![
        verter_workspace::ide_project_config(
            "/ws".to_string(),
            "/ws".to_string(),
            Some("/ws/tsconfig.json".to_string()),
        ),
        verter_workspace::ide_project_config(
            "/ws/packages/app".to_string(),
            "/ws".to_string(),
            Some(nested_config.to_string()),
        ),
    ]);
    let snapshot = crate::test_utils::make_test_snapshot(
        resolver,
        &[
            ("/ws", "/ws", Some("/ws/tsconfig.json")),
            ("/ws/packages/app", "/ws", Some(nested_config)),
        ],
    );
    provider.set_project_ownership(Arc::new(
        crate::configured_owner::SnapshotOwnerAuthority::new(snapshot),
    ));

    provider
}

async fn monorepo_provider(
    transport: ScriptedTsQueryTransport,
) -> ExtensionTypeProvider<ScriptedTsQueryTransport> {
    monorepo_provider_with_config(transport, "/ws/packages/app/tsconfig.json").await
}

#[tokio::test]
async fn open_stamps_the_owning_package_root_not_the_workspace_folder() {
    let transport = ScriptedTsQueryTransport::new();
    transport.push_response("open", json!({}));
    let provider = monorepo_provider(transport.clone()).await;

    provider
        .open_file("/ws/packages/app/src/App.vue.tsx", "export const a = 1;\n")
        .await
        .expect("open_file routes through the mock transport");

    let args = transport.first_args("open");
    assert_eq!(
        args.get("projectRootPath").and_then(|v| v.as_str()),
        Some("/ws/packages/app"),
        "the extension host resolves TypeScript from this root: a nested package must be \
         served from its OWN install, so the producer must send the owning project root — \
         sending the workspace folder `/ws` is what reports \
         `/ws/packages/app/node_modules/typescript` absent"
    );
}

#[tokio::test]
async fn update_open_stamps_the_owning_package_root_too() {
    let transport = ScriptedTsQueryTransport::new();
    transport.push_response("open", json!({}));
    transport.push_response("updateOpen", json!(true));
    let provider = monorepo_provider(transport.clone()).await;

    let file = "/ws/packages/app/src/App.vue.tsx";
    provider
        .open_file(file, "export const a = 1;\n")
        .await
        .expect("open_file routes through the mock transport");
    provider
        .update_file(file, "export const a = 2;\n")
        .await
        .expect("update_file routes through the mock transport");

    // `updateOpen` carries the root on each open entry; the recorded envelope
    // must not fall back to the folder for the follow-up sync either.
    let open_root = transport
        .first_args("open")
        .get("projectRootPath")
        .and_then(|v| v.as_str())
        .map(str::to_string);
    assert_eq!(open_root.as_deref(), Some("/ws/packages/app"));
    let update = transport.first_args("updateOpen");
    assert_eq!(
        update
            .pointer("/openFiles/0/projectRootPath")
            .and_then(|v| v.as_str()),
        Some("/ws/packages/app"),
        "the follow-up sync re-declares the owning package root: {update}"
    );
}

#[tokio::test]
async fn a_file_outside_every_nested_package_still_stamps_the_root_project() {
    let transport = ScriptedTsQueryTransport::new();
    transport.push_response("open", json!({}));
    let provider = monorepo_provider(transport.clone()).await;

    provider
        .open_file("/ws/src/Root.vue.tsx", "export const a = 1;\n")
        .await
        .expect("open_file routes through the mock transport");

    assert_eq!(
        transport
            .first_args("open")
            .get("projectRootPath")
            .and_then(|v| v.as_str()),
        Some("/ws"),
        "a file the root project owns keeps the root project"
    );
}

#[tokio::test]
async fn without_an_ownership_authority_the_workspace_folder_is_the_last_resort() {
    // Before init publishes a snapshot only folders are known. That is a real
    // (transient) state and must not panic or emit an empty root.
    let transport = ScriptedTsQueryTransport::new();
    transport.push_response("open", json!({}));
    let provider = ExtensionTypeProvider::with_transport(transport.clone(), "/ws");
    provider
        .update_workspace_folders(vec![json!({ "uri": "file:///ws", "name": "ws" })], vec![])
        .await
        .expect("workspace folders sync");

    provider
        .open_file("/ws/packages/app/src/App.vue.tsx", "export const a = 1;\n")
        .await
        .expect("open_file routes through the mock transport");

    assert_eq!(
        transport
            .first_args("open")
            .get("projectRootPath")
            .and_then(|v| v.as_str()),
        Some("/ws"),
    );
}

// ── Fail-closed: a refused project must not read as an empty result ──
//
// The extension host THROWS when it cannot serve a file's project (no workspace
// TypeScript, or a library-less install). The provider's promise is that the
// refusal propagates as a `TypeProviderError`. A feature that maps the refusal
// to `Ok(None)` / `Ok(vec![])` reports "nothing to say here" for a provider that
// is actually disabled — a silently wrong answer, and precisely the class the
// fail-closed contract exists to prevent.
//
// Discrimination: the scripted transport has NO queued response, so every
// primary query errors. Each assertion below fails if its feature swallows it.

#[tokio::test]
async fn a_refused_project_propagates_instead_of_reading_as_an_empty_result() {
    let transport = ScriptedTsQueryTransport::new();
    transport.push_response("open", json!({}));
    let provider = ExtensionTypeProvider::with_transport(transport, "/ws");
    let file = "/ws/src/App.vue.tsx";
    // Open first so every assertion below reaches its QUERY: a feature that
    // short-circuits on missing cached content would otherwise return empty for
    // a reason unrelated to the refusal.
    provider
        .open_file(file, "export const a = 1;\n")
        .await
        .expect("open_file routes through the mock transport");

    assert!(
        provider
            .get_hover(
                &crate::type_provider::traits::ProviderQuery::at_engine_surface(file),
                0
            )
            .await
            .is_err(),
        "hover must propagate the refusal, not answer `no hover here`"
    );
    assert!(
        provider.get_diagnostics(file).await.is_err(),
        "a refused semantic pass must not report a clean file"
    );
    assert!(
        provider
            .get_signature_help(
                &crate::type_provider::traits::ProviderQuery::at_engine_surface(file),
                0
            )
            .await
            .is_err(),
        "signature help must propagate the refusal"
    );
    assert!(
        provider
            .get_semantic_tokens(
                &crate::type_provider::traits::ProviderQuery::at_engine_surface(file)
            )
            .await
            .is_err(),
        "semantic tokens must propagate the refusal"
    );
    assert!(
        provider
            .get_document_highlights(
                &crate::type_provider::traits::ProviderQuery::at_engine_surface(file),
                0
            )
            .await
            .is_err(),
        "document highlights must propagate the refusal"
    );
    assert!(
        provider
            .get_inlay_hints(
                &crate::type_provider::traits::ProviderQuery::at_engine_surface(file),
                0,
                1
            )
            .await
            .is_err(),
        "inlay hints must propagate the refusal"
    );
    // A real diagnostic context: an EMPTY one legitimately short-circuits before
    // any query (no error codes ⇒ nothing to fix), so it would not reach the
    // refusal at all.
    let diag = ProviderDiagnosticContext {
        code: 6133,
        start: 0,
        end: 1,
    };
    assert!(
        provider
            .get_code_actions(
                &crate::type_provider::traits::ProviderQuery::at_engine_surface(file),
                0,
                1,
                std::slice::from_ref(&diag)
            )
            .await
            .is_err(),
        "the primary `getCodeFixes` query is what produces the quick fixes: answering \
         `no fixes available` for a project the host refused hides the refusal behind an \
         empty lightbulb"
    );
}

// ── `projectConfigPath`: the project's IDENTITY, not merely its directory ──
//
// A configured project IS its config file. One directory can hold several
// (`tsconfig.app.json` + `tsconfig.node.json` is the stock Vite layout), each
// with its own compiler options; and a project configured by `jsconfig.json` has
// no `tsconfig.json` at all. A consumer given only the directory therefore
// collapses sibling projects into one service and has to GUESS which config to
// read — so the producer declares the exact owning config alongside the root.
//
// Discrimination: the nested package's config is named in the snapshot and
// asserted on the envelope. A producer that sends only the root, or that assumes
// the name `tsconfig.json`, fails these.

#[tokio::test]
async fn open_declares_the_owning_projects_config_file_alongside_its_root() {
    let transport = ScriptedTsQueryTransport::new();
    transport.push_response("open", json!({}));
    let provider = monorepo_provider(transport.clone()).await;

    provider
        .open_file("/ws/packages/app/src/App.vue.tsx", "export const a = 1;\n")
        .await
        .expect("open_file routes through the mock transport");

    let args = transport.first_args("open");
    assert_eq!(
        args.get("projectConfigPath").and_then(|v| v.as_str()),
        Some("/ws/packages/app/tsconfig.json"),
        "the owning project's config decides its compiler options; the root directory \
         alone cannot — `/ws/packages/app` is also the directory of every sibling config \
         that package may declare"
    );
}

#[tokio::test]
async fn open_declares_a_jsconfig_owned_project_by_its_own_config_name() {
    // `jsconfig.json` is a configured project exactly like `tsconfig.json`; a
    // consumer that searches for the literal name `tsconfig.json` finds nothing
    // here and silently falls back to invented default options.
    let transport = ScriptedTsQueryTransport::new();
    transport.push_response("open", json!({}));
    let provider =
        monorepo_provider_with_config(transport.clone(), "/ws/packages/app/jsconfig.json").await;

    provider
        .open_file("/ws/packages/app/src/main.js", "export const a = 1;\n")
        .await
        .expect("open_file routes through the mock transport");

    let args = transport.first_args("open");
    assert_eq!(
        args.get("projectRootPath").and_then(|v| v.as_str()),
        Some("/ws/packages/app"),
    );
    assert_eq!(
        args.get("projectConfigPath").and_then(|v| v.as_str()),
        Some("/ws/packages/app/jsconfig.json"),
    );
}

#[tokio::test]
async fn update_open_reopen_declares_the_config_too() {
    // An update of an open file builds its own `openFiles` envelope. A config
    // declared only on the first `open` would leave the updated file bound to a
    // service built from guessed options.
    let transport = ScriptedTsQueryTransport::new();
    transport.push_response("open", json!({}));
    transport.push_response("updateOpen", json!(true));
    let provider = monorepo_provider(transport.clone()).await;

    let file = "/ws/packages/app/src/App.vue.tsx";
    provider
        .open_file(file, "export const a = 1;\n")
        .await
        .expect("open_file routes through the mock transport");
    provider
        .update_file(file, "export const a = 2;\n")
        .await
        .expect("update_file routes through the mock transport");

    let args = transport.first_args("updateOpen");
    let entry = args
        .get("openFiles")
        .and_then(|v| v.as_array())
        .and_then(|entries| entries.first())
        .expect("an update of an open file carries an openFiles entry");
    assert_eq!(
        entry.get("projectRootPath").and_then(|v| v.as_str()),
        Some("/ws/packages/app"),
    );
    assert_eq!(
        entry.get("projectConfigPath").and_then(|v| v.as_str()),
        Some("/ws/packages/app/tsconfig.json"),
    );
}

#[tokio::test]
async fn without_an_ownership_authority_no_config_is_invented() {
    // Before init publishes a snapshot the provider knows folders only. It must
    // declare no config at all rather than guess `<folder>/tsconfig.json`: the
    // consumer then discovers one for itself, and a wrong declared identity would
    // be worse than none.
    let transport = ScriptedTsQueryTransport::new();
    transport.push_response("open", json!({}));
    let provider = ExtensionTypeProvider::with_transport(transport.clone(), "/ws");
    provider
        .update_workspace_folders(vec![json!({ "uri": "file:///ws", "name": "ws" })], vec![])
        .await
        .expect("workspace folders sync");

    provider
        .open_file("/ws/packages/app/src/App.vue.tsx", "export const a = 1;\n")
        .await
        .expect("open_file routes through the mock transport");

    let args = transport.first_args("open");
    assert_eq!(
        args.get("projectRootPath").and_then(|v| v.as_str()),
        Some("/ws"),
    );
    assert!(
        args.get("projectConfigPath")
            .is_none_or(serde_json::Value::is_null),
        "no configured owner is known, so no config identity may be asserted: {args}"
    );
}

// ── Rebinding: the authority lands AFTER files are already open ──
//
// Init opens files as soon as the editor does, but the exact workspace snapshot
// — and with it the configured-owner authority — is published later. Everything
// opened in between carries the bootstrap folder identity, which for a nested
// package is the WRONG project: the extension host then resolves that package's
// TypeScript from the workspace folder and reports its own install absent.
//
// `background_init` calls `resync_open_files` immediately after installing the
// authority for exactly this reason. The provider must therefore RE-DECLARE
// every live file with its authoritative binding; an inherited no-op leaves
// every bootstrap-opened file bound to the folder until it happens to be edited
// again (only an `update_file` re-declares its file's binding).
//
// Discrimination: the fixture opens BEFORE the authority exists and asserts on
// the envelopes emitted AFTER it lands. A provider that inherits the trait's
// no-op emits nothing and fails on the recorded-command assertion.

/// The snapshot-backed authority `background_init` installs for the
/// single-folder monorepo fixture (one folder `/ws`, nested configured package
/// `/ws/packages/app`).
fn monorepo_authority() -> Arc<dyn crate::type_provider::traits::ConfiguredOwnerAuthority> {
    let resolver = verter_resolution::ModuleResolverCore::new(vec![
        verter_workspace::ide_project_config(
            "/ws".to_string(),
            "/ws".to_string(),
            Some("/ws/tsconfig.json".to_string()),
        ),
        verter_workspace::ide_project_config(
            "/ws/packages/app".to_string(),
            "/ws".to_string(),
            Some("/ws/packages/app/tsconfig.json".to_string()),
        ),
    ]);
    let snapshot = crate::test_utils::make_test_snapshot(
        resolver,
        &[
            ("/ws", "/ws", Some("/ws/tsconfig.json")),
            (
                "/ws/packages/app",
                "/ws",
                Some("/ws/packages/app/tsconfig.json"),
            ),
        ],
    );
    Arc::new(crate::configured_owner::SnapshotOwnerAuthority::new(
        snapshot,
    ))
}

/// A provider with the editor's workspace folders and NO ownership authority —
/// the bootstrap state every file opened before snapshot publication sees.
async fn bootstrap_provider(
    transport: ScriptedTsQueryTransport,
) -> ExtensionTypeProvider<ScriptedTsQueryTransport> {
    let provider = ExtensionTypeProvider::with_transport(transport, "/ws");
    provider
        .update_workspace_folders(vec![json!({ "uri": "file:///ws", "name": "ws" })], vec![])
        .await
        .expect("workspace folders sync");
    provider
}

#[tokio::test]
async fn resync_rebinds_a_file_opened_before_the_ownership_authority_landed() {
    let transport = ScriptedTsQueryTransport::new();
    transport.push_response("open", json!({}));
    transport.push_response("close", json!({}));
    transport.push_response("open", json!({}));
    let provider = bootstrap_provider(transport.clone()).await;

    let file = "/ws/packages/app/src/App.vue.tsx";
    let content = "export const a = 1;\n";
    provider
        .open_file(file, content)
        .await
        .expect("open_file routes through the mock transport");
    assert_eq!(
        transport
            .first_args("open")
            .get("projectRootPath")
            .and_then(|v| v.as_str()),
        Some("/ws"),
        "the bootstrap open can only know the folder — this is the state the resync fixes"
    );

    // Init publishes the exact snapshot, installs the authority, and resyncs.
    provider.set_project_ownership(monorepo_authority());
    provider
        .resync_open_files()
        .await
        .expect("the resync sweep routes through the mock transport");

    assert_eq!(
        transport.commands(),
        vec!["open".to_string(), "close".to_string(), "open".to_string()],
        "the file must be closed on the project it was mis-bound to and re-declared \
         on its real owner; an inherited no-op emits nothing here"
    );
    let reopen = transport
        .calls()
        .into_iter()
        .filter(|call| call.command == "open")
        .nth(1)
        .expect("the resync re-declares the file")
        .arguments;
    assert_eq!(
        reopen.get("projectRootPath").and_then(|v| v.as_str()),
        Some("/ws/packages/app"),
        "the re-declaration must carry the OWNING package root — that is the whole \
         point of resyncing after the authority lands: {reopen}"
    );
    assert_eq!(
        reopen.get("projectConfigPath").and_then(|v| v.as_str()),
        Some("/ws/packages/app/tsconfig.json"),
        "…and the owning config, or the consumer keys the rebound file by a guess"
    );
    assert_eq!(
        reopen.get("fileContent").and_then(|v| v.as_str()),
        Some(content),
        "the re-open carries the live buffer, not a stale disk read"
    );
}

#[tokio::test]
async fn resync_closes_a_file_no_configured_project_owns_instead_of_rebinding_it() {
    // Opened during bootstrap under the folder last-resort, then found to be
    // owned by no configured project at all. Terminal `NoProject`: the file must
    // be closed, not re-declared against an invented owner.
    let transport = ScriptedTsQueryTransport::new();
    transport.push_response("open", json!({}));
    transport.push_response("close", json!({}));
    let provider = bootstrap_provider(transport.clone()).await;

    let file = "/elsewhere/Detached.vue.tsx";
    provider
        .open_file(file, "export const a = 1;\n")
        .await
        .expect("open_file routes through the mock transport");

    provider.set_project_ownership(monorepo_authority());
    provider
        .resync_open_files()
        .await
        .expect("the resync sweep routes through the mock transport");

    assert_eq!(
        transport.commands(),
        vec!["open".to_string(), "close".to_string()],
        "an unowned file is closed and left closed: re-opening it would re-assert a \
         project the authority says does not exist"
    );
}

#[tokio::test]
async fn an_authoritatively_unowned_file_fails_closed_rather_than_binding_an_invented_project() {
    // `NoProject` is TERMINAL under the Project-Bound External-TS Contract. A
    // file excluded from every configured program (here by `node_modules/**`)
    // must not be bound to the nearest configured ancestor, and must not fall
    // through to the workspace folder either — both invent a project the
    // authority did not name.
    let transport = ScriptedTsQueryTransport::new();
    // A response IS queued: the refusal under test must be the OWNERSHIP
    // decision, never a transport that had nothing to answer with. A provider
    // that binds an invented project succeeds here.
    transport.push_response("open", json!({}));
    let provider = bootstrap_provider(transport.clone()).await;
    provider.set_project_ownership(monorepo_authority());

    let file = "/ws/node_modules/dep/index.d.ts";
    let result = provider.open_file(file, "export const a = 1;\n").await;

    assert!(
        result.is_err(),
        "no configured project claims this file, so there is no project to open it in"
    );
    assert_eq!(
        transport.commands(),
        Vec::<String>::new(),
        "…and nothing may be declared to the extension host on the way to failing: {:?}",
        transport.commands()
    );
}

#[tokio::test]
async fn completion_details_propagate_a_refusal_instead_of_returning_the_previous_items() {
    // The enrichment round-trip is where a project REBIND becomes visible: the
    // list was produced by the project that owned the file when `completionInfo`
    // ran, and the details request can land after the file has been re-declared
    // to a different project (an ownership authority arriving mid-session, a
    // config change). If that project refuses, returning the original items
    // serves the OLD project's answer under the new binding — a cross-project
    // stale result, which is exactly what the project-bound contract forbids.
    let transport = ScriptedTsQueryTransport::new();
    transport.push_response("open", json!({}));
    transport.push_response(
        "completionInfo",
        json!({ "entries": [{ "name": "answer", "kind": "const", "sortText": "11" }] }),
    );
    // No scripted response for `completionEntryDetails` ⇒ the host refuses it,
    // exactly as a project whose TypeScript cannot serve does.
    let provider = bootstrap_provider(transport.clone()).await;

    let file = "/ws/src/App.vue.tsx";
    provider
        .open_file(file, "export const answer = 1;\n")
        .await
        .expect("open_file routes through the mock transport");
    let completions = provider
        .get_completions(
            &crate::type_provider::traits::ProviderQuery::at_engine_surface(file),
            0,
            None,
        )
        .await
        .expect("the completion list itself succeeded");
    assert_eq!(completions.items.len(), 1);

    assert!(
        provider
            .get_completion_details(
                &crate::type_provider::traits::ProviderQuery::at_engine_surface(file),
                0,
                &completions.items
            )
            .await
            .is_err(),
        "a refused enrichment must propagate: answering with the items the previous \
         binding produced hides the refusal and serves another project's result"
    );
}

// ── Semantic tokens: TS "2020" classification decode + Verter legend remap ──
//
// `encodedSemanticClassifications-full` with `"format": "2020"` packs each
// span's classification as `((tokenTypeIdx + 1) << 8) | modifierSet` in
// TypeScript's classifier-2020 legend (types: class=0, enum=1, interface=2,
// namespace=3, typeParameter=4, type=5, parameter=6, variable=7, enumMember=8,
// property=9, function=10, method=11; modifier bits: declaration=0, static=1,
// async=2, readonly=3, defaultLibrary=4, local=5). Tokens cross the
// `TypeProvider` boundary in VERTER's published legend space, so the provider
// must decode the 2020 packing (fields are NOT `type | mods << 8` — they are
// the other way around, plus the `+1` offset) and remap BOTH halves by name.
//
// Discrimination: the classification constants below are asymmetric — a decoder
// that swaps the fields, drops the `+1`, or forwards TS-legend indices produces
// different numbers for every assertion.
#[tokio::test]
async fn semantic_tokens_decode_2020_and_remap_into_verter_legend_space() {
    let file = "/workspace/src/entry.ts";
    let content = "interface Shape { area: number }\nconst localCount = 42;\n";

    let transport = ScriptedTsQueryTransport::new();
    transport.push_response("open", json!({}));
    transport.push_response(
        "encodedSemanticClassifications-full",
        json!({
            "spans": [
                // "Shape" @ 10, len 5 — TS interface(2) + declaration(bit 0):
                // ((2 + 1) << 8) | 0b000001 = 769
                10, 5, 769,
                // "localCount" @ 39, len 10 — TS variable(7) + declaration(bit 0)
                // + readonly(bit 3) + local(bit 5):
                // ((7 + 1) << 8) | 0b101001 = 2089
                39, 10, 2089,
            ]
        }),
    );

    let provider = ExtensionTypeProvider::with_transport(transport, "/workspace");
    provider.open_file(file, content).await.expect("open");
    let tokens = provider
        .get_semantic_tokens(&crate::type_provider::traits::ProviderQuery::at_engine_surface(file))
        .await
        .expect("semantic tokens");

    assert_eq!(tokens.len(), 2, "both spans decode: {tokens:?}");

    // Verter legend: interface = type index 4; declaration = modifier bit 0.
    assert_eq!(tokens[0].start, 10);
    assert_eq!(tokens[0].length, 5);
    assert_eq!(
        tokens[0].token_type, 4,
        "TS-2020 `interface` (2) must remap to Verter `interface` (4); the \
         pre-fix inverted decode yields 1 here"
    );
    assert_eq!(
        tokens[0].token_modifiers, 1,
        "TS-2020 `declaration` (bit 0) must remap to Verter `declaration` (bit 0); \
         the pre-fix inverted decode reads the type field as modifiers and yields 3"
    );

    // Verter legend: variable = 8; declaration|readonly|local = bits 0,2,10.
    assert_eq!(tokens[1].start, 39);
    assert_eq!(tokens[1].length, 10);
    assert_eq!(
        tokens[1].token_type, 8,
        "TS-2020 `variable` (7) must remap to Verter `variable` (8)"
    );
    assert_eq!(
        tokens[1].token_modifiers,
        (1 << 0) | (1 << 2) | (1 << 10),
        "modifier BITS remap individually by name: TS declaration(0)/readonly(3)/\
         local(5) become Verter declaration(0)/readonly(2)/local(10) — forwarding \
         the raw bitset (0b101001) is the colors-look-plausible-but-wrong failure"
    );
}

/// Fail-closed half: a classification whose decoded type index is outside the
/// TS-2020 legend must DROP the token, and a zero type field (impossible under
/// the `+1` packing — only produced by mis-decoding) must not panic or emit.
#[tokio::test]
async fn semantic_tokens_drop_unmappable_classifications_instead_of_guessing() {
    let file = "/workspace/src/entry.ts";
    let content = "const ok = 1;\n";

    let transport = ScriptedTsQueryTransport::new();
    transport.push_response("open", json!({}));
    transport.push_response(
        "encodedSemanticClassifications-full",
        json!({
            "spans": [
                // Type index 12 is outside the 12-entry TS-2020 legend:
                // ((12 + 1) << 8) | 0 = 3328 → dropped.
                0, 2, 3328,
                // A raw zero "type" field (no +1 offset possible) → dropped.
                3, 2, 0,
                // "ok" @ 6, len 2 — variable(7) + declaration: survives.
                6, 2, ((7 + 1) << 8) | 1,
            ]
        }),
    );

    let provider = ExtensionTypeProvider::with_transport(transport, "/workspace");
    provider.open_file(file, content).await.expect("open");
    let tokens = provider
        .get_semantic_tokens(&crate::type_provider::traits::ProviderQuery::at_engine_surface(file))
        .await
        .expect("semantic tokens");

    assert_eq!(
        tokens.len(),
        1,
        "unmappable classifications are dropped, never emitted with a guessed \
         kind: {tokens:?}"
    );
    assert_eq!(tokens[0].start, 6);
    assert_eq!(tokens[0].token_type, 8, "Verter `variable`");
    assert_eq!(tokens[0].token_modifiers, 1, "Verter `declaration`");
}

#[tokio::test]
async fn inlay_hints_use_absolute_utf16_request_offsets_and_return_byte_positions() {
    let file = "/workspace/src/entry.ts";
    let content = "é\nconst answer = makeValue(42);\n";

    let transport = ScriptedTsQueryTransport::new();
    transport.push_response("open", json!({}));
    transport.push_response(
        "provideInlayHints",
        json!([{
            "text": "value:",
            "position": { "line": 2, "offset": 26 },
            "kind": "Parameter",
            "whitespaceAfter": true,
        }]),
    );

    let provider = ExtensionTypeProvider::with_transport(transport.clone(), "/workspace");
    provider.open_file(file, content).await.expect("open");
    let hints = provider
        .get_inlay_hints(
            &crate::type_provider::traits::ProviderQuery::at_engine_surface(file),
            3,
            content.len() as u32,
        )
        .await
        .expect("inlay hints");

    let args = transport.first_args("provideInlayHints");
    assert_eq!(
        args["start"],
        json!(2),
        "the byte offset after `é\\n` is absolute UTF-16 offset 2, not line 2"
    );
    assert_eq!(
        args["length"],
        json!(content.encode_utf16().count() - 2),
        "length is an absolute UTF-16 span, not an approximate line count"
    );

    assert_eq!(hints.len(), 1, "{hints:?}");
    assert_eq!(
        hints[0].position, 28,
        "tsserver line/offset must convert through the cached text into bytes"
    );
    assert!(matches!(hints[0].kind, Some(InlayHintKind::Parameter)));
    assert_eq!(hints[0].label, "value:");
}

/// The extension's application receipt is what its language service
/// acknowledged, never the contents cache that runs ahead of it: a `load_file`
/// sends nothing, a refused delivery leaves the service's bytes unknown, and a
/// close withdraws them.
#[tokio::test]
async fn applied_content_certifies_only_acknowledged_deliveries() {
    use verter_type_runtime::traits::AppliedContent;
    let transport = ScriptedTsQueryTransport::new();
    transport.push_response("open", json!({}));
    transport.push_response("updateOpen", json!(true));
    let provider = ExtensionTypeProvider::with_transport(transport.clone(), "/ws");
    let file = "/ws/src/App.vue.tsx";

    provider
        .load_file(file, "export const loaded = 1;\n")
        .await
        .expect("a load only caches");
    assert_eq!(
        provider.applied_content(file),
        AppliedContent::NotApplied,
        "a load sends nothing to the extension"
    );

    provider
        .open_file(file, "export const a = 1;\n")
        .await
        .expect("the extension acknowledges the open");
    assert_eq!(
        provider.applied_content(file),
        AppliedContent::Applied(Arc::from("export const a = 1;\n"))
    );

    provider
        .update_file(file, "export const b = 2;\n")
        .await
        .expect("the extension acknowledges the update");
    assert_eq!(
        provider.applied_content(file),
        AppliedContent::Applied(Arc::from("export const b = 2;\n"))
    );

    // Nothing scripted: the extension refuses this delivery.
    assert!(provider
        .update_file(file, "export const c = 3;\n")
        .await
        .is_err());
    assert_eq!(
        provider.applied_content(file),
        AppliedContent::NotApplied,
        "after a refused delivery the extension's bytes are unknown"
    );

    transport.push_response("updateOpen", json!(true));
    provider
        .update_file(file, "export const d = 4;\n")
        .await
        .expect("the extension acknowledges the re-delivery");
    assert_eq!(
        provider.applied_content(file),
        AppliedContent::Applied(Arc::from("export const d = 4;\n"))
    );
    transport.push_response("close", json!({}));
    provider
        .close_file(file)
        .await
        .expect("the close is acknowledged");
    assert_eq!(
        provider.applied_content(file),
        AppliedContent::NotApplied,
        "a closed file holds nothing"
    );
}

/// A `$/verter/tsQuery` transport whose every request is held until the test
/// answers it: each arrival is observable, and its acknowledgement or failure
/// is released on the test's schedule.
#[derive(Clone, Default)]
struct HeldTsQueryTransport {
    arrivals: Arc<Mutex<VecDeque<HeldQuery>>>,
    arrived: Arc<tokio::sync::Notify>,
}

/// One request held by [`HeldTsQueryTransport`].
struct HeldQuery {
    command: String,
    content: Option<String>,
    reply: tokio::sync::oneshot::Sender<Result<Value, TypeProviderError>>,
}

impl HeldQuery {
    fn acknowledge(self) {
        let _ = self.reply.send(Ok(json!(true)));
    }

    fn refuse(self) {
        let _ = self
            .reply
            .send(Err(TypeProviderError::new("refused".to_string())));
    }
}

impl HeldTsQueryTransport {
    /// The next request to arrive, once it has reached the transport.
    async fn next_arrival(&self) -> HeldQuery {
        loop {
            let notified = self.arrived.notified();
            tokio::pin!(notified);
            notified.as_mut().enable();
            if let Some(query) = self.arrivals.lock().unwrap().pop_front() {
                return query;
            }
            tokio::time::timeout(std::time::Duration::from_secs(10), notified)
                .await
                .expect("the provider never sent the expected request");
        }
    }
}

impl TsQueryTransport for HeldTsQueryTransport {
    fn ts_query(
        &self,
        params: TsQueryParams,
    ) -> impl Future<Output = Result<Value, TypeProviderError>> + Send + '_ {
        let (reply, answer) = tokio::sync::oneshot::channel();
        let content = params
            .arguments
            .get("fileContent")
            .or_else(|| {
                params
                    .arguments
                    .pointer("/changedFiles/0/textChanges/0/newText")
            })
            .or_else(|| params.arguments.pointer("/openFiles/0/fileContent"))
            .and_then(Value::as_str)
            .map(str::to_string);
        self.arrivals.lock().unwrap().push_back(HeldQuery {
            command: params.command,
            content,
            reply,
        });
        self.arrived.notify_waiters();
        async move {
            answer
                .await
                .unwrap_or_else(|_| Err(TypeProviderError::new("dropped".to_string())))
        }
    }
}

/// Open `file` with `content` through a held transport and acknowledge it.
async fn open_acknowledged(
    provider: &Arc<ExtensionTypeProvider<HeldTsQueryTransport>>,
    transport: &HeldTsQueryTransport,
    file: &str,
    content: &str,
) {
    let opening = {
        let provider = Arc::clone(provider);
        let (file, content) = (file.to_string(), content.to_string());
        tokio::spawn(async move { provider.open_file(&file, &content).await })
    };
    transport.next_arrival().await.acknowledge();
    opening
        .await
        .expect("the open task completes")
        .expect("the extension acknowledged the open");
}

/// Start an update of `file` to `content` and return its task once the
/// request has reached the extension.
async fn update_in_flight(
    provider: &Arc<ExtensionTypeProvider<HeldTsQueryTransport>>,
    transport: &HeldTsQueryTransport,
    file: &str,
    content: &str,
) -> (
    tokio::task::JoinHandle<Result<(), TypeProviderError>>,
    HeldQuery,
) {
    let updating = {
        let provider = Arc::clone(provider);
        let (file, content) = (file.to_string(), content.to_string());
        tokio::spawn(async move { provider.update_file(&file, &content).await })
    };
    let held = transport.next_arrival().await;
    assert_eq!(held.command, "updateOpen");
    assert_eq!(held.content.as_deref(), Some(content));
    (updating, held)
}

/// A delivery that reached the extension and was then dropped before its
/// answer settled leaves the service's bytes unknown: the receipt for the
/// bytes acknowledged before it is withdrawn, not kept.
#[tokio::test]
async fn a_cancelled_delivery_withdraws_the_earlier_receipt() {
    use verter_type_runtime::traits::AppliedContent;
    let transport = HeldTsQueryTransport::default();
    let provider = Arc::new(ExtensionTypeProvider::with_transport(
        transport.clone(),
        "/ws",
    ));
    let file = "/ws/src/App.vue.tsx";
    open_acknowledged(&provider, &transport, file, "export const a = 1;\n").await;
    assert_eq!(
        provider.applied_content(file),
        AppliedContent::Applied(Arc::from("export const a = 1;\n"))
    );

    // The extension applies B and holds its answer; the write is cancelled.
    let (updating, held) =
        update_in_flight(&provider, &transport, file, "export const b = 2;\n").await;
    updating.abort();
    assert!(updating
        .await
        .expect_err("the write was cancelled")
        .is_cancelled());
    drop(held);
    assert_eq!(
        provider.applied_content(file),
        AppliedContent::NotApplied,
        "the service may hold B: the receipt for A must not survive the cancelled write"
    );
}

/// Re-issuing the bytes the receipt already certifies leaves it standing: the
/// service holds them whether or not the repeat lands.
#[tokio::test]
async fn an_identical_redelivery_keeps_the_receipt_while_in_flight() {
    use verter_type_runtime::traits::AppliedContent;
    let transport = HeldTsQueryTransport::default();
    let provider = Arc::new(ExtensionTypeProvider::with_transport(
        transport.clone(),
        "/ws",
    ));
    let file = "/ws/src/App.vue.tsx";
    open_acknowledged(&provider, &transport, file, "export const a = 1;\n").await;
    let (updating, held) =
        update_in_flight(&provider, &transport, file, "export const a = 1;\n").await;
    assert_eq!(
        provider.applied_content(file),
        AppliedContent::Applied(Arc::from("export const a = 1;\n"))
    );
    held.acknowledge();
    updating.await.unwrap().unwrap();
    assert_eq!(
        provider.applied_content(file),
        AppliedContent::Applied(Arc::from("export const a = 1;\n"))
    );
}

/// An acknowledgement certifies its bytes only while its delivery is the
/// file's newest: an older delivery acknowledged after a newer one settled
/// never overwrites the newer receipt.
#[tokio::test]
async fn an_overtaken_acknowledgement_never_overwrites_a_newer_receipt() {
    use verter_type_runtime::traits::AppliedContent;
    let transport = HeldTsQueryTransport::default();
    let provider = Arc::new(ExtensionTypeProvider::with_transport(
        transport.clone(),
        "/ws",
    ));
    let file = "/ws/src/App.vue.tsx";
    open_acknowledged(&provider, &transport, file, "export const a = 0;\n").await;
    let (first, a) = update_in_flight(&provider, &transport, file, "export const a = 1;\n").await;
    let (second, b) = update_in_flight(&provider, &transport, file, "export const b = 2;\n").await;
    b.acknowledge();
    second.await.unwrap().unwrap();
    assert_eq!(
        provider.applied_content(file),
        AppliedContent::Applied(Arc::from("export const b = 2;\n"))
    );
    a.acknowledge();
    first.await.unwrap().unwrap();
    assert_eq!(
        provider.applied_content(file),
        AppliedContent::Applied(Arc::from("export const b = 2;\n")),
        "the overtaken acknowledgement of A must not replace the receipt for B"
    );
}

/// An acknowledgement is tied to its own delivery, not to equal bytes: with
/// A, B and A again issued, the FIRST A's late acknowledgement certifies
/// nothing — the service may hold B until the second A lands.
#[tokio::test]
async fn an_equal_bytes_acknowledgement_of_an_older_delivery_certifies_nothing() {
    use verter_type_runtime::traits::AppliedContent;
    let transport = HeldTsQueryTransport::default();
    let provider = Arc::new(ExtensionTypeProvider::with_transport(
        transport.clone(),
        "/ws",
    ));
    let file = "/ws/src/App.vue.tsx";
    open_acknowledged(&provider, &transport, file, "export const z = 0;\n").await;
    let (first_a, a1) =
        update_in_flight(&provider, &transport, file, "export const a = 1;\n").await;
    let (then_b, b) = update_in_flight(&provider, &transport, file, "export const b = 2;\n").await;
    let (second_a, a2) =
        update_in_flight(&provider, &transport, file, "export const a = 1;\n").await;

    b.acknowledge();
    then_b.await.unwrap().unwrap();
    a1.acknowledge();
    first_a.await.unwrap().unwrap();
    assert_eq!(
        provider.applied_content(file),
        AppliedContent::NotApplied,
        "neither the overtaken B nor the first A proves what the service holds"
    );

    a2.acknowledge();
    second_a.await.unwrap().unwrap();
    assert_eq!(
        provider.applied_content(file),
        AppliedContent::Applied(Arc::from("export const a = 1;\n"))
    );
}

/// A refused newest delivery withdraws the receipt, and an older delivery's
/// late acknowledgement cannot restore it.
#[tokio::test]
async fn a_refused_newest_delivery_is_not_masked_by_an_older_acknowledgement() {
    use verter_type_runtime::traits::AppliedContent;
    let transport = HeldTsQueryTransport::default();
    let provider = Arc::new(ExtensionTypeProvider::with_transport(
        transport.clone(),
        "/ws",
    ));
    let file = "/ws/src/App.vue.tsx";
    open_acknowledged(&provider, &transport, file, "export const z = 0;\n").await;
    let (first, a) = update_in_flight(&provider, &transport, file, "export const a = 1;\n").await;
    let (second, b) = update_in_flight(&provider, &transport, file, "export const b = 2;\n").await;
    b.refuse();
    assert!(second.await.unwrap().is_err());
    a.acknowledge();
    first.await.unwrap().unwrap();
    assert_eq!(provider.applied_content(file), AppliedContent::NotApplied);
}

/// A stale failure is inert: a refused older delivery settling after a newer
/// one was acknowledged leaves the newer receipt standing — every delivery
/// replaces the whole buffer, so the service holds the newer bytes whatever
/// the older one did.
#[tokio::test]
async fn a_stale_failure_never_withdraws_a_newer_receipt() {
    use verter_type_runtime::traits::AppliedContent;
    let transport = HeldTsQueryTransport::default();
    let provider = Arc::new(ExtensionTypeProvider::with_transport(
        transport.clone(),
        "/ws",
    ));
    let file = "/ws/src/App.vue.tsx";
    open_acknowledged(&provider, &transport, file, "export const z = 0;\n").await;
    let (first, a) = update_in_flight(&provider, &transport, file, "export const a = 1;\n").await;
    let (second, b) = update_in_flight(&provider, &transport, file, "export const b = 2;\n").await;
    b.acknowledge();
    second.await.unwrap().unwrap();
    a.refuse();
    assert!(first.await.unwrap().is_err());
    assert_eq!(
        provider.applied_content(file),
        AppliedContent::Applied(Arc::from("export const b = 2;\n")),
        "the overtaken refusal of A must not withdraw the receipt for B"
    );
}

/// A delivery issued before a close settles nothing after it: its late
/// failure leaves the re-opened file's receipt standing.
#[tokio::test]
async fn a_failure_issued_before_a_close_never_withdraws_the_reopened_receipt() {
    use verter_type_runtime::traits::AppliedContent;
    let transport = HeldTsQueryTransport::default();
    let provider = Arc::new(ExtensionTypeProvider::with_transport(
        transport.clone(),
        "/ws",
    ));
    let file = "/ws/src/App.vue.tsx";
    open_acknowledged(&provider, &transport, file, "export const z = 0;\n").await;
    let (updating, a) =
        update_in_flight(&provider, &transport, file, "export const a = 1;\n").await;

    let closing = {
        let provider = Arc::clone(&provider);
        let file = file.to_string();
        tokio::spawn(async move { provider.close_file(&file).await })
    };
    let close = transport.next_arrival().await;
    assert_eq!(close.command, "close");
    close.acknowledge();
    closing.await.unwrap().unwrap();
    open_acknowledged(&provider, &transport, file, "export const c = 3;\n").await;

    a.refuse();
    assert!(updating.await.unwrap().is_err());
    assert_eq!(
        provider.applied_content(file),
        AppliedContent::Applied(Arc::from("export const c = 3;\n")),
        "a failure issued before the close must not withdraw the re-opened receipt"
    );
}

/// The extension language service's buffer state, mutated by each envelope
/// exactly as `ExtensionTsService.handleQuery` applies it: `open` and
/// `updateOpen`'s `openFiles` set the whole buffer, `changedFiles` splices
/// its ranged text changes into the buffer the service holds (positions
/// resolved by the service's own line walk), and a refused request applies
/// nothing.
#[derive(Clone, Default)]
struct ServiceModelTransport {
    state: Arc<Mutex<ServiceModel>>,
}

#[derive(Default)]
struct ServiceModel {
    buffers: std::collections::HashMap<String, String>,
    /// Requests left to refuse, unapplied, before the service accepts again.
    refusals: usize,
}

impl ServiceModelTransport {
    fn refuse_next(&self) {
        self.state.lock().unwrap().refusals += 1;
    }

    fn held(&self, file: &str) -> Option<String> {
        self.state.lock().unwrap().buffers.get(file).cloned()
    }

    /// The service's 1-based line/offset → offset walk (ASCII fixtures, so
    /// UTF-16 units and bytes agree).
    fn position_to_offset(text: &str, line: u64, offset: u64) -> usize {
        let mut current = 1;
        let mut i = 0;
        let bytes = text.as_bytes();
        while current < line && i < bytes.len() {
            if bytes[i] == b'\n' {
                current += 1;
            }
            i += 1;
        }
        (i + offset as usize - 1).min(text.len())
    }
}

impl TsQueryTransport for ServiceModelTransport {
    fn ts_query(
        &self,
        params: TsQueryParams,
    ) -> impl Future<Output = Result<Value, TypeProviderError>> + Send + '_ {
        let mut state = self.state.lock().unwrap();
        let result = if state.refusals > 0 {
            state.refusals -= 1;
            Err(TypeProviderError::new("refused".to_string()))
        } else {
            let args = &params.arguments;
            match params.command.as_str() {
                "open" => {
                    let file = args["file"].as_str().unwrap().to_string();
                    let content = args["fileContent"].as_str().unwrap().to_string();
                    state.buffers.insert(file, content);
                    Ok(json!({}))
                }
                "updateOpen" => {
                    for entry in args["openFiles"].as_array().into_iter().flatten() {
                        if let Some(content) = entry["fileContent"].as_str() {
                            let file = entry["file"].as_str().unwrap().to_string();
                            state.buffers.insert(file, content.to_string());
                        }
                    }
                    for entry in args["changedFiles"].as_array().into_iter().flatten() {
                        let file = entry["fileName"].as_str().unwrap();
                        let Some(text) = state.buffers.get_mut(file) else {
                            continue;
                        };
                        let mut changes: Vec<&Value> = entry["textChanges"]
                            .as_array()
                            .into_iter()
                            .flatten()
                            .collect();
                        let key = |c: &Value| {
                            (
                                c["start"]["line"].as_u64().unwrap(),
                                c["start"]["offset"].as_u64().unwrap(),
                            )
                        };
                        changes.sort_by_key(|c| std::cmp::Reverse(key(c)));
                        for change in changes {
                            let at = |pos: &Value| {
                                Self::position_to_offset(
                                    text,
                                    pos["line"].as_u64().unwrap(),
                                    pos["offset"].as_u64().unwrap(),
                                )
                            };
                            let (start, end) = (at(&change["start"]), at(&change["end"]));
                            let new_text = change["newText"].as_str().unwrap();
                            *text = format!("{}{new_text}{}", &text[..start], &text[end..]);
                        }
                    }
                    Ok(json!(true))
                }
                "close" => Ok(json!({})),
                other => Err(TypeProviderError::new(format!("unmodelled `{other}`"))),
            }
        };
        std::future::ready(result)
    }
}

/// Certified bytes are the bytes the service holds, after the service has
/// physically applied every envelope the provider emitted.
fn assert_certifies_held(
    provider: &ExtensionTypeProvider<ServiceModelTransport>,
    service: &ServiceModelTransport,
    file: &str,
    context: &str,
) {
    use verter_type_runtime::traits::AppliedContent;
    let held = service.held(file).expect("the service holds the file");
    assert_eq!(
        provider.applied_content(file),
        AppliedContent::Applied(Arc::from(held.as_str())),
        "{context}: the receipt must certify exactly the bytes the service holds"
    );
}

/// An update certifies the whole buffer the service ends up holding, even when
/// the contents cache was moved by a `load_file` that delivered nothing.
#[tokio::test]
async fn an_update_after_a_cache_only_load_replaces_the_whole_service_buffer() {
    let service = ServiceModelTransport::default();
    let provider = ExtensionTypeProvider::with_transport(service.clone(), "/ws");
    let file = "/ws/src/App.vue.tsx";
    provider
        .open_file(file, "const a=1;\nconst b=2;\nconst c=3;\n")
        .await
        .unwrap();
    provider.load_file(file, "const a=1;\n").await.unwrap();
    provider
        .update_file(file, "const replacement=4;\n")
        .await
        .unwrap();
    assert_eq!(
        service.held(file).as_deref(),
        Some("const replacement=4;\n")
    );
    assert_certifies_held(&provider, &service, file, "after a cache-only load");
}

/// A refused shorter update leaves the contents cache ahead of the service;
/// the successful retry must still replace the whole service buffer.
#[tokio::test]
async fn a_retry_after_a_refused_update_replaces_the_whole_service_buffer() {
    let service = ServiceModelTransport::default();
    let provider = ExtensionTypeProvider::with_transport(service.clone(), "/ws");
    let file = "/ws/src/App.vue.tsx";
    provider
        .open_file(file, "const a=1;\nconst b=2;\nconst c=3;\n")
        .await
        .unwrap();
    service.refuse_next();
    assert!(provider.update_file(file, "const a=1;\n").await.is_err());
    provider
        .update_file(file, "const replacement=4;\n")
        .await
        .unwrap();
    assert_eq!(
        service.held(file).as_deref(),
        Some("const replacement=4;\n")
    );
    assert_certifies_held(&provider, &service, file, "after a refused update");
}

/// The resync re-open delivers the contents cache, which a `load_file` may
/// have moved past the receipt: the receipt must follow the re-opened bytes,
/// never keep certifying the bytes the re-open replaced.
#[tokio::test]
async fn a_resync_reopen_settles_the_bytes_it_delivers() {
    let service = ServiceModelTransport::default();
    let provider = ExtensionTypeProvider::with_transport(service.clone(), "/ws");
    let file = "/ws/src/App.vue.tsx";
    provider
        .open_file(file, "export const a = 1;\n")
        .await
        .unwrap();
    provider
        .load_file(file, "export const b = 2;\n")
        .await
        .unwrap();
    provider.resync_open_files().await.unwrap();
    assert_eq!(service.held(file).as_deref(), Some("export const b = 2;\n"));
    assert_certifies_held(&provider, &service, file, "after a resync re-open");
}
