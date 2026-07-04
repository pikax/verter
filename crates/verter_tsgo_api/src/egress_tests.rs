//! Behavioral decision-table tests for [`classify_egress`] — the
//! deny-by-default server→editor carrier egress classifier — over synthetic
//! JSON-RPC frames. Every deny/filter case carries a NEGATIVE assertion: the
//! carrier URI must be ABSENT from a filtered value while the user entry
//! SURVIVES (discriminating a per-entry filter from a whole-frame drop), and
//! control frames prove the carrier-free path still forwards (discriminating
//! "suppressed" from "classifier denies everything").

use super::*;

use serde_json::json;

/// The carrier overlay URI (a member of the monotonic `carrier_egress_taint`
/// set the relay classifies against).
const CARRIER: &str = "file:///ws/App.vue.tsx";
/// A plain user document URI (never carrier-attributed).
const USER: &str = "file:///ws/user.ts";

fn carriers() -> HashSet<String> {
    std::iter::once(CARRIER.to_string()).collect()
}

fn lsp_range() -> Value {
    json!({ "start": { "line": 0, "character": 0 }, "end": { "line": 0, "character": 4 } })
}

/// Unwrap a `FilterCarrierEntries` decision and run the shared NEGATIVE
/// assertion: the carrier URI is ABSENT from the re-encoded frame.
fn expect_filtered(decision: EgressDecision, context: &str) -> Value {
    match decision {
        EgressDecision::FilterCarrierEntries(v) => {
            let text = v.to_string();
            assert!(
                !text.contains(CARRIER),
                "{context}: the carrier URI must be ABSENT from the filtered \
                 frame: {text}"
            );
            v
        }
        other => panic!(
            "{context}: expected FilterCarrierEntries (per-entry filter, not \
             a whole-frame drop or a raw forward), got {other:?}"
        ),
    }
}

/// Unwrap an `AnswerServer` decision: the synthesized JSON-RPC response the
/// relay sends back to the SERVER on the editor's behalf. NOT Forward (the
/// editor never sees the request) and NOT Suppress (the server would wait
/// forever on a dropped request).
fn expect_answer_server(decision: EgressDecision, context: &str) -> Value {
    match decision {
        EgressDecision::AnswerServer(v) => v,
        other => panic!(
            "{context}: expected AnswerServer (a suppressed server→client \
             REQUEST must be ANSWERED on the server's behalf — never \
             forwarded, never dropped), got {other:?}"
        ),
    }
}

#[test]
fn carrier_publish_diagnostics_suppressed_user_file_forwarded() {
    let set = carriers();
    let carrier_diag = json!({
        "jsonrpc": "2.0",
        "method": "textDocument/publishDiagnostics",
        "params": {
            "uri": CARRIER,
            "diagnostics": [{ "range": lsp_range(), "message": "carrier-internal" }],
        },
    });
    assert_eq!(
        classify_egress(&carrier_diag, &set, None),
        EgressDecision::Suppress,
        "diagnostics for a carrier overlay must never reach the editor"
    );
    let user_diag = json!({
        "jsonrpc": "2.0",
        "method": "textDocument/publishDiagnostics",
        "params": { "uri": USER, "diagnostics": [] },
    });
    assert_eq!(
        classify_egress(&user_diag, &set, None),
        EgressDecision::Forward,
        "user-file diagnostics forward untouched (carrier-free fast path)"
    );
}

#[test]
fn workspace_symbol_mixed_response_strips_carrier_symbol_keeps_user() {
    let set = carriers();
    let response = json!({
        "jsonrpc": "2.0",
        "id": 7,
        "result": [
            { "name": "CarrierOnlySymbol", "kind": 12,
              "location": { "uri": CARRIER, "range": lsp_range() } },
            { "name": "UserSymbol", "kind": 12,
              "location": { "uri": USER, "range": lsp_range() } },
        ],
    });
    let filtered = expect_filtered(
        classify_egress(&response, &set, None),
        "mixed workspace/symbol response",
    );
    let items = filtered["result"]
        .as_array()
        .expect("the filtered result stays an array");
    assert_eq!(
        items.len(),
        1,
        "exactly the carrier symbol is dropped: {items:?}"
    );
    assert_eq!(
        items[0]["name"],
        json!("UserSymbol"),
        "the USER symbol must SURVIVE the filter (per-entry filter, not a \
         whole-frame drop): {items:?}"
    );
    assert!(
        !filtered.to_string().contains("CarrierOnlySymbol"),
        "the carrier symbol entry must be ABSENT: {filtered}"
    );
    assert_eq!(
        filtered["id"],
        json!(7),
        "the response identity (id) is preserved for the editor's own request"
    );
}

#[test]
fn workspace_diagnostic_report_strips_carrier_document_keeps_user() {
    let set = carriers();
    let response = json!({
        "jsonrpc": "2.0",
        "id": 3,
        "result": { "items": [
            { "kind": "full", "uri": CARRIER,
              "items": [{ "range": lsp_range(), "message": "internal" }] },
            { "kind": "full", "uri": USER, "items": [] },
        ] },
    });
    let filtered = expect_filtered(
        classify_egress(&response, &set, None),
        "workspace/diagnostic report",
    );
    let items = filtered["result"]["items"]
        .as_array()
        .expect("the report items stay an array");
    assert_eq!(items.len(), 1, "the carrier document report is dropped");
    assert_eq!(
        items[0]["uri"],
        json!(USER),
        "the USER document report must SURVIVE: {items:?}"
    );
}

#[test]
fn location_array_response_drops_carrier_location_keeps_user() {
    let set = carriers();
    // Location[] — references / implementation / typeDefinition /
    // documentHighlight-style results.
    let locations = json!({
        "jsonrpc": "2.0",
        "id": 4,
        "result": [
            { "uri": CARRIER, "range": lsp_range() },
            { "uri": USER, "range": lsp_range() },
        ],
    });
    let filtered = expect_filtered(
        classify_egress(&locations, &set, None),
        "Location[] response",
    );
    let items = filtered["result"].as_array().unwrap();
    assert_eq!(items.len(), 1, "the carrier Location is dropped");
    assert_eq!(
        items[0]["uri"],
        json!(USER),
        "the USER Location must SURVIVE: {items:?}"
    );

    // LocationLink[] — definition-style results (`targetUri`).
    let links = json!({
        "jsonrpc": "2.0",
        "id": 5,
        "result": [
            { "targetUri": CARRIER, "targetRange": lsp_range(),
              "targetSelectionRange": lsp_range() },
            { "targetUri": USER, "targetRange": lsp_range(),
              "targetSelectionRange": lsp_range() },
        ],
    });
    let filtered = expect_filtered(
        classify_egress(&links, &set, None),
        "LocationLink[] response",
    );
    let items = filtered["result"].as_array().unwrap();
    assert_eq!(items.len(), 1, "the carrier LocationLink is dropped");
    assert_eq!(
        items[0]["targetUri"],
        json!(USER),
        "the USER LocationLink must SURVIVE: {items:?}"
    );
}

#[test]
fn rename_workspace_edit_response_drops_carrier_edits_keeps_user() {
    let set = carriers();
    // `changes` map keyed by URI — the carrier appears ONLY as an object KEY:
    // the deep scan must inspect keys, or a rename touching a carrier leaks.
    let mut changes = serde_json::Map::new();
    changes.insert(
        CARRIER.to_string(),
        json!([{ "range": lsp_range(), "newText": "renamed" }]),
    );
    changes.insert(
        USER.to_string(),
        json!([{ "range": lsp_range(), "newText": "renamed" }]),
    );
    let by_changes = json!({
        "jsonrpc": "2.0",
        "id": 6,
        "result": { "changes": changes },
    });
    let filtered = expect_filtered(
        classify_egress(&by_changes, &set, None),
        "rename WorkspaceEdit (changes)",
    );
    let kept = filtered["result"]["changes"]
        .as_object()
        .expect("the filtered changes stay a map");
    assert!(
        kept.contains_key(USER),
        "the USER edit must SURVIVE: {kept:?}"
    );
    assert!(
        !kept.contains_key(CARRIER),
        "the carrier-URI changes key must be ABSENT: {kept:?}"
    );
    assert_eq!(kept.len(), 1, "exactly the carrier entry is dropped");

    // `documentChanges` variant (TextDocumentEdit entries).
    let by_doc_changes = json!({
        "jsonrpc": "2.0",
        "id": 8,
        "result": { "documentChanges": [
            { "textDocument": { "uri": CARRIER, "version": 1 },
              "edits": [{ "range": lsp_range(), "newText": "a" }] },
            { "textDocument": { "uri": USER, "version": 1 },
              "edits": [{ "range": lsp_range(), "newText": "b" }] },
        ] },
    });
    let filtered = expect_filtered(
        classify_egress(&by_doc_changes, &set, None),
        "rename WorkspaceEdit (documentChanges)",
    );
    let kept = filtered["result"]["documentChanges"].as_array().unwrap();
    assert_eq!(kept.len(), 1, "the carrier TextDocumentEdit is dropped");
    assert_eq!(
        kept[0]["textDocument"]["uri"],
        json!(USER),
        "the USER TextDocumentEdit must SURVIVE: {kept:?}"
    );
}

#[test]
fn apply_edit_request_carrier_referencing_answers_server_fail_closed() {
    let set = carriers();
    // A MIXED (carrier+user) server-originated `workspace/applyEdit` is a
    // server→client REQUEST: it is NEVER routed to the editor — neither raw
    // nor filtered (a filtered forward would be a partial-apply lie: the
    // editor answers `applied:true` while the carrier part was silently
    // dropped). The whole request is answered to the SERVER with
    // `{applied:false}` under the ORIGINAL id.
    let mut mixed_changes = serde_json::Map::new();
    mixed_changes.insert(
        CARRIER.to_string(),
        json!([{ "range": lsp_range(), "newText": "x" }]),
    );
    mixed_changes.insert(
        USER.to_string(),
        json!([{ "range": lsp_range(), "newText": "y" }]),
    );
    let mixed = json!({
        "jsonrpc": "2.0",
        "id": 11,
        "method": "workspace/applyEdit",
        "params": { "label": "refactor", "edit": { "changes": mixed_changes } },
    });
    let resp = expect_answer_server(
        classify_egress(&mixed, &set, None),
        "mixed workspace/applyEdit (fail-closed, never editor-routed)",
    );
    assert_eq!(
        resp["id"],
        json!(11),
        "the synthesized response carries the ORIGINAL request id: {resp}"
    );
    assert_eq!(
        resp["result"],
        json!({ "applied": false }),
        "a carrier-referencing applyEdit is answered with the negative \
         ApplyWorkspaceEditResult — the user remainder is NOT forwarded: {resp}"
    );
    assert!(
        resp.get("method").is_none() && resp.get("error").is_none(),
        "the synthesized applyEdit answer is a plain RESULT response: {resp}"
    );

    // ALL-carrier `changes`: same fail-closed answer — never dropped (hang)
    // and never forwarded (leak).
    let mut only_carrier = serde_json::Map::new();
    only_carrier.insert(
        CARRIER.to_string(),
        json!([{ "range": lsp_range(), "newText": "x" }]),
    );
    let all_carrier = json!({
        "jsonrpc": "2.0",
        "id": 12,
        "method": "workspace/applyEdit",
        "params": { "edit": { "changes": only_carrier } },
    });
    let resp = expect_answer_server(
        classify_egress(&all_carrier, &set, None),
        "all-carrier workspace/applyEdit (changes)",
    );
    assert_eq!(resp["id"], json!(12));
    assert_eq!(resp["result"], json!({ "applied": false }));

    // ALL-carrier `documentChanges`: same synthesized negative answer.
    let all_carrier_docs = json!({
        "jsonrpc": "2.0",
        "id": 13,
        "method": "workspace/applyEdit",
        "params": { "edit": { "documentChanges": [
            { "textDocument": { "uri": CARRIER, "version": 1 },
              "edits": [{ "range": lsp_range(), "newText": "x" }] },
        ] } },
    });
    let resp = expect_answer_server(
        classify_egress(&all_carrier_docs, &set, None),
        "all-carrier workspace/applyEdit (documentChanges)",
    );
    assert_eq!(resp["id"], json!(13));
    assert_eq!(resp["result"], json!({ "applied": false }));

    // A carrier URI OUTSIDE the edit (the label) — equally fail-closed.
    let mut user_only = serde_json::Map::new();
    user_only.insert(
        USER.to_string(),
        json!([{ "range": lsp_range(), "newText": "y" }]),
    );
    let carrier_outside_edit = json!({
        "jsonrpc": "2.0",
        "id": 16,
        "method": "workspace/applyEdit",
        "params": { "label": CARRIER, "edit": { "changes": user_only } },
    });
    let resp = expect_answer_server(
        classify_egress(&carrier_outside_edit, &set, None),
        "applyEdit with a carrier URI outside the edit",
    );
    assert_eq!(resp["id"], json!(16));
    assert_eq!(resp["result"], json!({ "applied": false }));
}

#[test]
fn completion_list_drops_carrier_referencing_item_keeps_plain_item() {
    let set = carriers();
    // A completion item whose edits/data reference a carrier is suppressible
    // whatever its provider-specific shape — the plain item survives.
    let list = json!({
        "jsonrpc": "2.0",
        "id": 14,
        "result": { "isIncomplete": false, "items": [
            { "label": "carrierImport",
              "additionalTextEdits": [{ "range": lsp_range(), "newText": "import x" }],
              "data": { "uri": CARRIER } },
            { "label": "plainKeyword", "kind": 14 },
        ] },
    });
    let filtered = expect_filtered(classify_egress(&list, &set, None), "completion list");
    let items = filtered["result"]["items"].as_array().unwrap();
    assert_eq!(items.len(), 1, "the carrier-referencing item is dropped");
    assert_eq!(
        items[0]["label"],
        json!("plainKeyword"),
        "the plain user item must SURVIVE: {items:?}"
    );
    assert!(
        !filtered.to_string().contains("carrierImport"),
        "the dropped item must be ABSENT whole: {filtered}"
    );

    // Bare-array completion results filter identically.
    let bare = json!({
        "jsonrpc": "2.0",
        "id": 15,
        "result": [
            { "label": "carrierImport", "data": { "uri": CARRIER } },
            { "label": "plainKeyword" },
        ],
    });
    let filtered = expect_filtered(classify_egress(&bare, &set, None), "bare completion array");
    let items = filtered["result"].as_array().unwrap();
    assert_eq!(items.len(), 1);
    assert_eq!(items[0]["label"], json!("plainKeyword"));
}

#[test]
fn carrier_free_unrecognized_method_forwards_transparently() {
    let set = carriers();
    let notification = json!({
        "jsonrpc": "2.0",
        "method": "custom/experimental",
        "params": { "uri": USER, "payload": [1, 2, 3] },
    });
    assert_eq!(
        classify_egress(&notification, &set, None),
        EgressDecision::Forward,
        "transparency: a carrier-free frame of ANY method forwards untouched"
    );
    let response = json!({
        "jsonrpc": "2.0",
        "id": 20,
        "result": { "anything": { "nested": USER } },
    });
    assert_eq!(
        classify_egress(&response, &set, None),
        EgressDecision::Forward,
        "a carrier-free response of an unrecognized shape still forwards"
    );
}

#[test]
fn unrecognized_carrier_referencing_frames_suppress_fail_closed() {
    let set = carriers();
    // An unrecognized NOTIFICATION naming the carrier.
    let notification = json!({
        "jsonrpc": "2.0",
        "method": "custom/experimental",
        "params": { "uri": CARRIER },
    });
    assert_eq!(
        classify_egress(&notification, &set, None),
        EgressDecision::Suppress,
        "an unrecognized carrier-referencing notification drops whole"
    );
    // A response whose result is an UNRECOGNIZED shape (a bare Location
    // object): no recognized filter can strip it → fail closed.
    let bare_location = json!({
        "jsonrpc": "2.0",
        "id": 9,
        "result": { "uri": CARRIER, "range": lsp_range() },
    });
    assert_eq!(
        classify_egress(&bare_location, &set, None),
        EgressDecision::Suppress,
        "an unfilterable carrier-referencing response shape drops whole"
    );
    // A response with no `result` at all (an error naming the carrier).
    let error_response = json!({
        "jsonrpc": "2.0",
        "id": 10,
        "error": { "code": -32603, "message": CARRIER },
    });
    assert_eq!(
        classify_egress(&error_response, &set, None),
        EgressDecision::Suppress,
        "a carrier-referencing error response has nothing filterable — drop"
    );
}

#[test]
fn carrier_referencing_server_request_answered_with_error_original_id() {
    let set = carriers();
    // A non-applyEdit server→client REQUEST steering the editor at a
    // carrier: never forwarded (leak), never dropped (the server would wait
    // forever) — answered on the server's behalf with a protocol-valid
    // JSON-RPC error under the ORIGINAL id.
    let request = json!({
        "jsonrpc": "2.0",
        "id": "srv-1",
        "method": "window/showDocument",
        "params": { "uri": CARRIER },
    });
    let resp = expect_answer_server(
        classify_egress(&request, &set, None),
        "carrier-referencing window/showDocument request",
    );
    assert_eq!(
        resp["id"],
        json!("srv-1"),
        "the synthesized response carries the ORIGINAL request id: {resp}"
    );
    assert_eq!(
        resp["error"]["code"],
        json!(-32803),
        "the method-agnostic negative is the sanitized RequestFailed error \
         (a -32601 would let a conforming server treat the capability as \
         unsupported for real user files): {resp}"
    );
    assert_eq!(
        resp["error"]["message"],
        json!("request failed"),
        "the error carries the fixed benign message: {resp}"
    );
    assert!(
        resp.get("result").is_none() && resp.get("method").is_none(),
        "the synthesized answer is a plain ERROR response: {resp}"
    );
}

#[test]
fn carrier_only_definition_response_answers_editor_neutral_when_method_known() {
    let set = carriers();
    // A carrier-ONLY singleton Location result — an unfilterable shape. With
    // the responded-to editor request TRACKED (`textDocument/definition`,
    // a null-admitting result type), the classifier completes the request
    // with the method-valid NEUTRAL under the ORIGINAL id — no carrier, no
    // strand.
    let response = json!({
        "jsonrpc": "2.0",
        "id": 21,
        "result": { "uri": CARRIER, "range": lsp_range() },
    });
    let decision = classify_egress(&response, &set, Some("textDocument/definition"));
    let EgressDecision::AnswerEditor(neutral) = decision else {
        panic!(
            "a carrier-only unfilterable response to a TRACKED editor \
             request must complete via AnswerEditor (Suppress would strand \
             the editor's pending request), got {decision:?}"
        );
    };
    assert_eq!(
        neutral,
        json!({ "jsonrpc": "2.0", "id": 21, "result": null }),
        "the neutral carries the ORIGINAL id and a method-valid null result \
         — nothing else: {neutral}"
    );
    assert!(
        !neutral.to_string().contains(CARRIER),
        "the synthesized neutral must carry NO carrier data: {neutral}"
    );
}

#[test]
fn carrier_only_responses_to_null_valid_methods_answer_editor_null() {
    let set = carriers();
    // The CONTROL for the fail-closed inversion: a method on the explicit
    // null-valid allowlist (a genuinely `X | null` result type) still
    // completes with `result: null` — the inversion must NOT over-rotate into
    // erroring every method. Unknown/custom methods (NOT null-valid) error
    // instead (see `unknown_method_answers_editor_sanitized_error_fail_closed`).
    for method in [
        "textDocument/hover",
        "textDocument/semanticTokens/full",
        "textDocument/inlayHint",
        "textDocument/linkedEditingRange",
        "workspace/executeCommand",
    ] {
        let response = json!({
            "jsonrpc": "2.0",
            "id": 30,
            "result": { "uri": CARRIER, "range": lsp_range() },
        });
        let decision = classify_egress(&response, &set, Some(method));
        let EgressDecision::AnswerEditor(neutral) = decision else {
            panic!(
                "a carrier-only unfilterable response to a TRACKED `{method}` \
                 request must complete via AnswerEditor, got {decision:?}"
            );
        };
        assert_eq!(
            neutral,
            json!({ "jsonrpc": "2.0", "id": 30, "result": null }),
            "`{method}` (a null-valid result type on the allowlist) completes \
             with the neutral `result: null`: {neutral}"
        );
        assert!(
            !neutral.to_string().contains(CARRIER),
            "the synthesized neutral must carry NO carrier data: {neutral}"
        );
    }
}

#[test]
fn null_invalid_methods_answer_editor_sanitized_error() {
    let set = carriers();
    // Methods absent from the null-valid allowlist whose result cannot validly
    // be a synthesized `null` — the `*/resolve` family (object-only results)
    // and `initialize` (an InitializeResult) — complete fail-closed as the
    // sanitized RequestFailed error, never `result: null`.
    for method in [
        "completionItem/resolve",
        "codeAction/resolve",
        "codeLens/resolve",
        "documentLink/resolve",
        "inlayHint/resolve",
        "workspaceSymbol/resolve",
        "initialize",
    ] {
        let response = json!({
            "jsonrpc": "2.0",
            "id": 31,
            "result": { "uri": CARRIER, "range": lsp_range() },
        });
        let decision = classify_egress(&response, &set, Some(method));
        let EgressDecision::AnswerEditor(sanitized) = decision else {
            panic!(
                "a carrier-only response to a TRACKED `{method}` request \
                 must complete via AnswerEditor, got {decision:?}"
            );
        };
        assert_eq!(
            sanitized["error"]["code"],
            json!(-32803),
            "`{method}` (a null-invalid result type) completes as the \
             sanitized RequestFailed error: {sanitized}"
        );
        assert!(
            sanitized.get("result").is_none(),
            "`{method}` must NOT complete as `result: null`: {sanitized}"
        );
        assert!(
            !sanitized.to_string().contains(CARRIER),
            "the sanitized error must carry NO carrier data: {sanitized}"
        );
    }
}

#[test]
fn carrier_only_response_without_tracked_method_suppresses_fail_closed() {
    let set = carriers();
    // The SAME carrier-only unfilterable response with NO tracked editor
    // request (`editor_pending_method = None`): fail-closed whole-frame
    // drop — the relay never fabricates a reply for an id it did not track.
    let response = json!({
        "jsonrpc": "2.0",
        "id": 22,
        "result": { "uri": CARRIER, "range": lsp_range() },
    });
    assert_eq!(
        classify_egress(&response, &set, None),
        EgressDecision::Suppress,
        "an UNtracked carrier-only response must still suppress fail-closed \
         — no fabricated reply"
    );
}

#[test]
fn carrier_resolve_response_answers_editor_sanitized_error_when_method_known() {
    let set = carriers();
    // A `completionItem/resolve` response must return an OBJECT — `null` is
    // not method-valid — so the tracked completion is a SANITIZED JSON-RPC
    // error under the original id, carrying no carrier data.
    let response = json!({
        "jsonrpc": "2.0",
        "id": 23,
        "result": {
            "label": "carrierImport",
            "additionalTextEdits": [{ "range": lsp_range(), "newText": "import x" }],
            "data": { "uri": CARRIER },
        },
    });
    let decision = classify_egress(&response, &set, Some("completionItem/resolve"));
    let EgressDecision::AnswerEditor(sanitized) = decision else {
        panic!(
            "a carrier-only resolve response to a TRACKED editor request \
             must complete via AnswerEditor, got {decision:?}"
        );
    };
    assert_eq!(
        sanitized["id"],
        json!(23),
        "the sanitized error carries the ORIGINAL id: {sanitized}"
    );
    assert_eq!(
        sanitized["error"]["code"],
        json!(-32803),
        "a resolve method (object-only result) completes as the sanitized \
         RequestFailed error, not `result: null`: {sanitized}"
    );
    assert!(
        sanitized.get("result").is_none(),
        "the sanitized completion is a plain ERROR response: {sanitized}"
    );
    let text = sanitized.to_string();
    assert!(
        !text.contains(CARRIER) && !text.contains("carrierImport"),
        "the sanitized error must carry NO carrier data: {text}"
    );
}

#[test]
fn carrier_diagnostic_related_documents_answers_editor_error_not_null() {
    let set = carriers();
    // A pull-diagnostic report (`textDocument/diagnostic`) whose
    // `relatedDocuments` is keyed by a carrier URI: the `items` filter strips
    // the top-level diagnostics, but the carrier-keyed `relatedDocuments` entry
    // survives -> the post-filter carrier re-scan denies. A
    // `DocumentDiagnosticReport` has NO null variant, so a `result: null`
    // completion would be protocol-INVALID. The tracked request completes
    // fail-closed with the sanitized `-32803` error, NEVER `result: null`.
    let response = json!({
        "jsonrpc": "2.0",
        "id": 40,
        "result": {
            "kind": "full",
            "items": [],
            "relatedDocuments": {
                CARRIER: { "kind": "full",
                           "items": [{ "range": lsp_range(), "message": "carrier-internal" }] },
            },
        },
    });
    let decision = classify_egress(&response, &set, Some("textDocument/diagnostic"));
    let EgressDecision::AnswerEditor(neutral) = decision else {
        panic!(
            "a carrier-referencing diagnostic report to a TRACKED request must \
             complete via AnswerEditor, got {decision:?}"
        );
    };
    assert_eq!(
        neutral["error"]["code"],
        json!(-32803),
        "`textDocument/diagnostic` (a NON-null result type) completes as the \
         sanitized RequestFailed error, NEVER `result: null`: {neutral}"
    );
    assert!(
        neutral.get("result").is_none(),
        "a non-null-valid method must NOT complete as `result: null`: {neutral}"
    );
    assert!(
        !neutral.to_string().contains(CARRIER),
        "the sanitized error carries NO carrier data: {neutral}"
    );
}

#[test]
fn non_null_result_methods_answer_editor_sanitized_error() {
    let set = carriers();
    // The confirmer-named methods whose LSP result type CANNOT be null:
    // pull-diagnostics and document color. An unfilterable carrier-referencing
    // response (a bare unrecognized object) tracked to any of them must
    // complete fail-closed with the sanitized `-32803` error, never null.
    for method in [
        "textDocument/diagnostic",
        "workspace/diagnostic",
        "textDocument/documentColor",
        "textDocument/colorPresentation",
    ] {
        let response = json!({
            "jsonrpc": "2.0",
            "id": 41,
            "result": { "uri": CARRIER, "range": lsp_range() },
        });
        let decision = classify_egress(&response, &set, Some(method));
        let EgressDecision::AnswerEditor(sanitized) = decision else {
            panic!(
                "`{method}` carrier-only response must complete via AnswerEditor, got {decision:?}"
            );
        };
        assert_eq!(
            sanitized["error"]["code"],
            json!(-32803),
            "`{method}` (a non-null result type) must complete as the sanitized \
             error, NEVER `result: null`: {sanitized}"
        );
        assert!(
            sanitized.get("result").is_none(),
            "`{method}` must NOT complete as `result: null`: {sanitized}"
        );
    }
}

#[test]
fn unknown_method_answers_editor_sanitized_error_fail_closed() {
    let set = carriers();
    // An unknown/custom tracked method is NOT provably null-valid: fail-closed
    // default is the sanitized `-32803` error (not `result: null`).
    let response = json!({
        "jsonrpc": "2.0",
        "id": 42,
        "result": { "uri": CARRIER, "range": lsp_range() },
    });
    let decision = classify_egress(&response, &set, Some("custom/experimental"));
    let EgressDecision::AnswerEditor(sanitized) = decision else {
        panic!("an unknown-method carrier-only response must complete via AnswerEditor, got {decision:?}");
    };
    assert_eq!(
        sanitized["error"]["code"],
        json!(-32803),
        "an unknown method completes fail-closed with the sanitized error: {sanitized}"
    );
    assert!(
        sanitized.get("result").is_none(),
        "an unknown method must NOT complete as `result: null`: {sanitized}"
    );
}

#[test]
fn carrier_free_progress_frames_forward_regardless_of_token() {
    // Transparency: progress/work-done frames are not suppressed headlessly.
    // Carrier authority is EXACTLY the open-overlay set — a carrier-free
    // frame forwards whatever its `params.token` value (string namespaces
    // are NOT a second authority).
    let set = carriers();
    for method in [
        "$/progress",
        "window/workDoneProgress/create",
        "window/workDoneProgress/cancel",
    ] {
        for token in ["editor-work-9", "verter:3"] {
            let frame = json!({
                "jsonrpc": "2.0",
                "method": method,
                "params": { "token": token, "value": { "kind": "report" } },
            });
            assert_eq!(
                classify_egress(&frame, &set, None),
                EgressDecision::Forward,
                "a carrier-FREE `{method}` frame (token `{token}`) forwards \
                 untouched — the classifier consults exactly the open-overlay \
                 set, never a token namespace"
            );
        }
    }
    // A non-string (numeric) token is equally carrier-free — forward.
    let numeric = json!({
        "jsonrpc": "2.0",
        "method": "$/progress",
        "params": { "token": 12, "value": { "kind": "end" } },
    });
    assert_eq!(
        classify_egress(&numeric, &set, None),
        EgressDecision::Forward
    );
}

#[test]
fn empty_carrier_set_forwards_every_carrier_shaped_frame() {
    let empty: HashSet<String> = HashSet::new();
    let frames = [
        json!({
            "jsonrpc": "2.0",
            "method": "textDocument/publishDiagnostics",
            "params": { "uri": CARRIER, "diagnostics": [] },
        }),
        json!({
            "jsonrpc": "2.0",
            "id": 1,
            "result": [
                { "name": "Sym", "location": { "uri": CARRIER, "range": lsp_range() } },
            ],
        }),
        json!({
            "jsonrpc": "2.0",
            "id": 2,
            "method": "workspace/applyEdit",
            "params": { "edit": { "documentChanges": [
                { "textDocument": { "uri": CARRIER, "version": 1 }, "edits": [] },
            ] } },
        }),
        json!({
            "jsonrpc": "2.0",
            "method": "custom/experimental",
            "params": { "uri": CARRIER },
        }),
        // Progress frames included: with no overlays open NOTHING is
        // suppressed — token strings are not a carrier authority.
        json!({
            "jsonrpc": "2.0",
            "method": "$/progress",
            "params": { "token": "verter:7", "value": { "kind": "begin", "title": "x" } },
        }),
    ];
    for frame in &frames {
        assert_eq!(
            classify_egress(frame, &empty, None),
            EgressDecision::Forward,
            "with no open overlays there is no carrier attribution — every \
             frame forwards: {frame}"
        );
    }
}
