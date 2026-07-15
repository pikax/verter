//! The headless deny-by-default server→editor egress policy for Verter
//! carrier overlays.
//!
//! [`classify_egress`] suppresses carrier-referencing notifications (and any
//! unfilterable response no tracked editor request correlates with),
//! filters carrier entries from RECOGNIZED editor-correlated response
//! shapes, answers EVERY carrier-referencing server→client REQUEST on the
//! server's behalf ([`EgressDecision::AnswerServer`]; never editor-routed —
//! a contaminated `workspace/applyEdit`, mixed or all-carrier, is answered
//! `{"applied": false}`), completes a carrier-referencing unfilterable response to
//! a TRACKED editor request with a method-valid neutral to the editor
//! ([`EgressDecision::AnswerEditor`] — original id, no carrier, no strand;
//! an UNtracked such response still suppresses whole, fail-closed), and
//! drops unmapped carrier-referencing entries for position-mapped channels
//! until a live `ProviderPositionMapper` can present source locations. It
//! does not claim live editor attachment, source-position presentation, or
//! pass-through transparency for carrier-contaminated frames; carrier-free
//! forwarded frames remain byte-identical.
//!
//! Carrier authority is EXACTLY the relay's monotonic `carrier_egress_taint`
//! set — the URIs Verter itself EVER injected via `didOpen`, tainted before
//! the wire send and never removed on `didClose` (the retraction state,
//! `open_overlays`, is a separate axis: same injection lifecycle, active
//! lifetime only) — matched CANONICALLY (percent-decode + case/slash fold,
//! [`canonicalize_carrier_uri`]) against every string in the frame, object
//! keys included, so a carrier the engine echoes back in a different URI
//! encoding than Verter injected (`file:///c%3A/…` vs `file:///C:/…`) still
//! matches; carrier-referencing pure server-log/trace notifications are
//! additionally suppressed when their free text EMBEDS a carrier path as a
//! substring. No generated-suffix heuristics, and no token namespace:
//! Verter-origin progress/work-token suppression belongs to the live layer,
//! and only when backed by real minted-token tracking.

use std::collections::HashSet;

use serde_json::Value;
use verter_span::path::fs_is_case_insensitive;
use verter_span::uri::{file_uri_to_path, percent_decode};

/// The canonicalized carrier taint set plus the host filesystem's case policy — the
/// authority every carrier match consults. Bundling the policy with the set keeps the
/// fold IDENTICAL for the set and every scanned candidate.
///
/// Canonicalization always percent-decodes, slash-folds, and folds the Windows
/// DRIVE-LETTER case (a drive letter is always case-insensitive and only appears in a
/// Windows path, so `c:` and `C:` — the injected-vs-echoed drive encoding — always
/// match). The REST of the path folds case ONLY when the host filesystem is
/// case-insensitive ([`fs_is_case_insensitive`]): on a case-sensitive FS (Linux) a
/// case-distinct user path is a DIFFERENT file and must NOT be folded into the carrier
/// (the over-suppression this closes), while the deny-by-default Windows/macOS leak
/// suppression (`file:///c%3A/…` vs `file:///C:/…`) is preserved.
struct CarrierMatcher {
    canonical: HashSet<String>,
    case_insensitive: bool,
}

impl CarrierMatcher {
    /// Canonicalize the raw taint set under `case_insensitive`.
    fn new(carrier_uris: &HashSet<String>, case_insensitive: bool) -> Self {
        let canonical = carrier_uris
            .iter()
            .map(|u| canonicalize_carrier_uri(u, case_insensitive))
            .filter(|u| !u.is_empty())
            .collect();
        Self {
            canonical,
            case_insensitive,
        }
    }

    /// Whether the taint set is empty (nothing injected, or nothing canonicalizes to a
    /// usable key) — nothing can match, so nothing can leak.
    fn is_empty(&self) -> bool {
        self.canonical.is_empty()
    }

    /// Whether `uri`, canonicalized under the same policy, is a tainted carrier
    /// (whole-URI canonical equality).
    fn matches(&self, uri: &str) -> bool {
        self.canonical
            .contains(&canonicalize_carrier_uri(uri, self.case_insensitive))
    }

    /// Whether `text`, canonicalized under the same policy, EMBEDS a canonical carrier
    /// key as a SUBSTRING (the pure-log/trace free-text leak).
    fn text_embeds_carrier(&self, text: &str) -> bool {
        let norm = canonical_text(text, self.case_insensitive);
        self.canonical.iter().any(|k| norm.contains(k.as_str()))
    }
}

/// The egress decision for one server→editor frame.
#[derive(Debug, PartialEq)]
pub(crate) enum EgressDecision {
    /// The frame is carrier-free: forward the RAW original bytes,
    /// byte-identical.
    Forward,
    /// Drop the whole frame: carrier `textDocument/publishDiagnostics`, any
    /// other carrier-referencing notification, or a carrier-referencing
    /// RESPONSE no recognized filter can strip clean AND no tracked editor
    /// request correlates with (fail-closed, deny-by-default). Produced
    /// only for frames that owe nobody a reply the relay can vouch for — a
    /// suppressed server→client REQUEST is answered to the server instead
    /// ([`EgressDecision::AnswerServer`]), and an unfilterable response to
    /// a TRACKED editor request is completed with a neutral to the editor
    /// ([`EgressDecision::AnswerEditor`]).
    Suppress,
    /// The re-encoded frame with its carrier entries removed — a RESPONSE to
    /// the editor's own request, stripped of the carrier entries in its
    /// recognized `result` shape; any non-carrier entries survive (an
    /// all-carrier recognized shape re-encodes to an empty carrier-free
    /// result). Responses only: a carrier-referencing server→client REQUEST
    /// is never editor-routed, filtered or otherwise (see
    /// [`EgressDecision::AnswerServer`]).
    FilterCarrierEntries(Value),
    /// Do not forward the frame to the editor; send this synthesized
    /// JSON-RPC response to the SERVER instead. Produced for EVERY
    /// carrier-referencing server→client REQUEST (`id` + `method`) — mixed
    /// or all-carrier alike: forwarding raw would leak carrier data,
    /// forwarding a filtered `workspace/applyEdit` would be a partial-apply
    /// lie, and dropping would leave the server waiting forever — so the
    /// relay answers on the editor's behalf with a protocol-valid negative
    /// carrying the ORIGINAL request id (see [`synthesize_server_response`]).
    AnswerServer(Value),
    /// Do not forward the frame to the editor; send this synthesized
    /// NEUTRAL response — carrying the ORIGINAL editor-request id and NO
    /// carrier data — to the EDITOR so its pending request resolves.
    /// Produced only for a carrier-referencing response to a TRACKED editor
    /// request that no recognized filter can strip clean: suppressing it
    /// whole would strand the editor's request forever, so the relay
    /// completes it fail-closed: a sanitized `-32803` error by default, and
    /// `result: null` ONLY for a method on the explicit null-valid allowlist
    /// (see [`synthesize_editor_response`]). An UNtracked such response still
    /// suppresses fail-closed ([`EgressDecision::Suppress`]) — the relay
    /// never fabricates a reply for an id it did not track.
    AnswerEditor(Value),
}

/// Synthesize the JSON-RPC response that answers a suppressed server→client
/// REQUEST on the editor's behalf, carrying the ORIGINAL request id:
/// `workspace/applyEdit` → the negative `ApplyWorkspaceEditResult`
/// (`{"applied": false}`); every other method → the sanitized JSON-RPC
/// `RequestFailed` error (`{"code": -32803, "message": "request failed"}`,
/// mirroring the editor-side neutral — a `-32601` "method not found" would
/// let a conforming server treat the capability as unsupported for real
/// user files). Called by the classifier's
/// [`EgressDecision::AnswerServer`] production for carrier-referencing
/// requests, and by the relay's server→editor pump for the reserved-id
/// request anomaly (a server→client request carrying a `verter:*` id,
/// which could never be answered through the editor). Pure — the caller
/// routes the value to the server writer.
pub(crate) fn synthesize_server_response(msg: &Value) -> Value {
    let id = msg.get("id").cloned().unwrap_or(Value::Null);
    if msg.get("method").and_then(|m| m.as_str()) == Some("workspace/applyEdit") {
        serde_json::json!({
            "jsonrpc": "2.0",
            "id": id,
            "result": { "applied": false },
        })
    } else {
        serde_json::json!({
            "jsonrpc": "2.0",
            "id": id,
            "error": { "code": -32803, "message": "request failed" },
        })
    }
}

/// Classify one server→editor frame against the carrier egress-taint set.
/// Deny-by-default, ordered:
///
/// 1. Carrier-free fast path: no string anywhere in the frame (object keys
///    included — a `WorkspaceEdit.changes` map keys entries by URI)
///    CANONICALIZES ([`canonicalize_carrier_uri`]) to a member of the
///    canonicalized taint set, AND (for a pure server-log/trace notification)
///    no free-text body embeds a canonical carrier key as a substring → forward
///    raw. This is the overwhelmingly common editor traffic; it stays
///    byte-identical.
/// 2. The frame references at least one carrier URI (canonical whole-URI match)
///    or is a carrier-referencing pure-log/trace notification → answer,
///    suppress, or filter by channel (see [`classify_carrier_referencing`] /
///    [`is_carrier_referencing_log_trace`]); it is never forwarded raw.
///
/// `editor_pending_method` is the method of the tracked editor request this
/// frame responds to, if any (the relay's ingress `id → method` record) —
/// consulted ONLY on the response branch, where a carrier-referencing unfilterable
/// response to a tracked request completes as a method-valid neutral
/// ([`EgressDecision::AnswerEditor`]) instead of stranding the editor.
///
/// Carrier authority is EXACTLY `carrier_uris` (the relay's monotonic
/// `carrier_egress_taint` set) — no second authority, token namespaces
/// included.
pub(crate) fn classify_egress(
    msg: &Value,
    carrier_uris: &HashSet<String>,
    editor_pending_method: Option<&str>,
) -> EgressDecision {
    // Canonicalize the taint set ONCE under the host FS case policy
    // ([`CarrierMatcher`]: percent-decode, strip `file://`, forward-slash, always-fold
    // the drive letter, fold the rest of the path only on a case-insensitive FS) so a
    // carrier the engine echoes back in a DIFFERENT URI encoding than Verter injected —
    // a percent-encoded drive colon (`file:///c%3A/…`), a case-folded drive, a backslash
    // path — still matches its tainted carrier, WITHOUT folding a case-distinct user path
    // into a carrier on a case-sensitive FS. The SAME canonicalizer (via the matcher) is
    // applied to every scanned candidate below. An empty taint set — or one whose members
    // all canonicalize away to nothing — yields an empty matcher, which the carrier-free
    // fast path forwards without a match.
    let matcher = CarrierMatcher::new(carrier_uris, fs_is_case_insensitive());
    // A pure server-log / trace NOTIFICATION whose free-text body EMBEDS a
    // carrier path as a SUBSTRING (a verbose `window/logMessage` logs the
    // didOpen'd carrier's own path) leaks Verter's injected carrier into the
    // editor's log channel — the structured scan below only recognizes a WHOLE
    // carrier URI. These frames carry no editor-actionable semantic content, so
    // a carrier-referencing one is dropped wholesale (fail-closed — strands
    // nothing). With an empty matcher the substring scan matches nothing, so
    // this never fires ahead of the carrier-free forward.
    if is_carrier_referencing_log_trace(msg, &matcher) {
        return EgressDecision::Suppress;
    }
    // Carrier-free fast path (deny-by-default): the deep `references_carrier`
    // carrier scan is the SOLE gate on the forward decision. An empty matcher
    // (nothing injected, or nothing canonicalizes to a usable key) OR a frame no
    // canonical carrier whole-URI match touches → forward the RAW original bytes.
    // This is the one forward production site and the overwhelmingly common editor
    // traffic; it stays byte-identical.
    if matcher.is_empty() || !references_carrier(msg, &matcher) {
        return EgressDecision::Forward;
    }
    classify_carrier_referencing(msg, &matcher, editor_pending_method)
}

/// Deep recursive scan: does any string anywhere in `value` — object keys
/// included (a `WorkspaceEdit.changes` map keys its entries by URI) —
/// CANONICALIZE to a member of `canonical_carriers` (the canonicalized taint
/// set)? Canonical comparison ([`canonicalize_carrier_uri`]: percent-decoding,
/// case / slash folding) so a carrier URI the engine echoes in a different
/// encoding than Verter injected (`file:///c%3A/…` vs `file:///C:/…`) still
/// matches — while a plain user document URI never does.
fn references_carrier(value: &Value, carriers: &CarrierMatcher) -> bool {
    match value {
        Value::String(s) => carriers.matches(s),
        Value::Array(items) => items.iter().any(|v| references_carrier(v, carriers)),
        Value::Object(map) => map
            .iter()
            .any(|(key, v)| carriers.matches(key) || references_carrier(v, carriers)),
        _ => false,
    }
}

/// Pure server-log / trace NOTIFICATION methods whose free-text message body may
/// embed a carrier path as a SUBSTRING (a verbose `window/logMessage` trace
/// logs the didOpen'd carrier's own path). They carry no editor-actionable
/// semantic content, so a carrier-referencing one is suppressed wholesale.
const LOG_TRACE_NOTIFICATION_METHODS: &[&str] = &[
    "window/logMessage",
    "window/showMessage",
    "window/logTrace",
    "$/logTrace",
];

/// Whether `msg` is one of the pure server-log/trace notifications
/// ([`LOG_TRACE_NOTIFICATION_METHODS`]) whose free text EMBEDS a tainted carrier
/// path — the leak [`references_carrier`]'s whole-URI match cannot catch (the
/// carrier appears as a SUBSTRING of a larger trace line).
///
/// A verbose server log emits the carrier path in many forms across its trace
/// lines (the `file://` URI, a lowercased drive path, a backslash path), so the
/// scan canonicalizes each log string ([`canonical_text`]) and substring-matches
/// the canonical carrier keys (`canonical_carriers`). Scoped to pure-log/trace
/// frames ONLY — the structured carrier-reference classification uses whole-URI
/// canonical equality, so this substring rule never over-broadly suppresses a
/// legitimate semantic frame.
fn is_carrier_referencing_log_trace(msg: &Value, carriers: &CarrierMatcher) -> bool {
    let Some(method) = msg.get("method").and_then(|m| m.as_str()) else {
        return false;
    };
    if !LOG_TRACE_NOTIFICATION_METHODS.contains(&method) {
        return false;
    }
    frame_text_embeds_carrier_key(msg, carriers)
}

/// Deep recursive scan: does any string anywhere in `value`, once canonicalized
/// ([`canonical_text`]), CONTAIN a canonical carrier key as a SUBSTRING?
/// Distinct from [`references_carrier`]'s whole-URI canonical equality — used
/// ONLY for the pure-log/trace free-text leak.
fn frame_text_embeds_carrier_key(value: &Value, carriers: &CarrierMatcher) -> bool {
    match value {
        Value::String(s) => carriers.text_embeds_carrier(s),
        Value::Array(items) => items
            .iter()
            .any(|v| frame_text_embeds_carrier_key(v, carriers)),
        Value::Object(map) => map
            .values()
            .any(|v| frame_text_embeds_carrier_key(v, carriers)),
        _ => false,
    }
}

/// Canonicalize a carrier URI to a comparison key. Applied to BOTH the taint set
/// and every scanned candidate, so a carrier URI the engine echoes in a different
/// encoding than Verter injected still canonicalizes to the SAME key.
///
/// URI parsing routes through the ONE shared `verter_span::uri::file_uri_to_path`
/// owner (NOT a private reimplementation): it percent-decodes, forward-slashes, and
/// resolves the URI authority per RFC 8089 — an empty/`localhost` authority is the
/// LOCAL file (`file:///C:/…`, `file://localhost/C:/…` collapse to `C:/…`), and any
/// other authority is the canonical `//host/share` UNC identity (both the 2-slash
/// `file://server/share` and the 4-slash `file:////server/share` forms collapse to
/// `//server/share`). Then the FS-appropriate fold applies: ALWAYS fold the Windows
/// drive-letter case (`C:` == `c:` — the same file), and fold the REST of the path
/// ONLY on a case-insensitive FS, so a case-distinct user path on a case-sensitive
/// FS (a DIFFERENT file) is never folded into the carrier.
fn canonicalize_carrier_uri(uri: &str, case_insensitive: bool) -> String {
    let path = file_uri_to_path(uri);
    let drive_folded = fold_drive_letter(&path);
    if case_insensitive {
        drive_folded.to_ascii_lowercase()
    } else {
        drive_folded
    }
}

/// Lowercase a leading Windows drive letter (`C:/…` → `c:/…`), leaving the rest of the
/// path untouched. A drive letter is always case-insensitive and only appears in a
/// Windows path, so folding it is safe (and required for `c%3A` vs `C:` matching) on
/// every platform.
fn fold_drive_letter(path: &str) -> String {
    let bytes = path.as_bytes();
    if bytes.len() >= 2 && bytes[1] == b':' && bytes[0].is_ascii_alphabetic() {
        let mut folded = String::with_capacity(path.len());
        folded.push(bytes[0].to_ascii_lowercase() as char);
        folded.push_str(&path[1..]);
        folded
    } else {
        path.to_string()
    }
}

/// Canonicalize a free-text string for substring matching against a canonical
/// carrier key: percent-decode (through the shared `verter_span::uri::percent_decode`
/// owner), forward-slash, fold every embedded Windows drive letter
/// ([`fold_drive_letters_in_text`] — ALIGNED with the structured
/// [`canonicalize_carrier_uri`]'s always-fold-the-drive rule so a `C:/…` drive path
/// in a trace line matches the `c:/…` carrier key on a case-sensitive FS too), and
/// fold case ONLY on a case-insensitive FS (the `file://` scheme is left intact — the
/// carrier key is substring-matched WITHIN the text). A case-sensitive FS preserves
/// the rest of the path's case, so a case-distinct path in a log line does not embed a
/// carrier key.
fn canonical_text(s: &str, case_insensitive: bool) -> String {
    let slashed = percent_decode(s).replace('\\', "/");
    let drive_folded = fold_drive_letters_in_text(&slashed);
    if case_insensitive {
        drive_folded.to_ascii_lowercase()
    } else {
        drive_folded
    }
}

/// Lowercase every Windows drive-letter occurrence (`X:/`) in `s` — the free-text
/// analogue of [`fold_drive_letter`] (which folds only a LEADING drive), so a drive
/// path embedded ANYWHERE in a trace line folds its (case-insensitive) drive
/// identically to the structured carrier key. A drive letter is folded only at a
/// token boundary (start-of-string or after a non-alphanumeric char) so an `X:/`
/// inside an identifier is never touched. Safe on every platform — a drive letter is
/// case-insensitive on the only OS that has drives.
fn fold_drive_letters_in_text(s: &str) -> String {
    let chars: Vec<char> = s.chars().collect();
    let mut out = String::with_capacity(s.len());
    let mut i = 0;
    while i < chars.len() {
        let at_boundary = i == 0 || !chars[i - 1].is_ascii_alphanumeric();
        if at_boundary
            && i + 2 < chars.len()
            && chars[i].is_ascii_alphabetic()
            && chars[i + 1] == ':'
            && chars[i + 2] == '/'
        {
            out.push(chars[i].to_ascii_lowercase());
            out.push(':');
            out.push('/');
            i += 3;
        } else {
            out.push(chars[i]);
            i += 1;
        }
    }
    out
}

/// Answer/suppress/filter for a frame that references at least one carrier
/// URI. Deny-by-default: only editor-correlated RESPONSES with a recognized
/// filterable shape are re-encoded with their carrier entries removed;
/// EVERY server→client REQUEST is ANSWERED on the server's behalf (never
/// editor-routed — protocol liveness without a leak or a partial-apply
/// lie); a carrier-referencing unfilterable RESPONSE to a TRACKED editor request
/// completes as a method-valid neutral to the editor
/// ([`EgressDecision::AnswerEditor`] — no strand, no carrier); everything
/// else — carrier `textDocument/publishDiagnostics`, unrecognized
/// notifications, unfilterable responses with no tracked editor request —
/// drops whole (fail-closed; those frames owe nobody a reply the relay can
/// vouch for).
fn classify_carrier_referencing(
    msg: &Value,
    carriers: &CarrierMatcher,
    editor_pending_method: Option<&str>,
) -> EgressDecision {
    let has_id = msg.get("id").map(|v| !v.is_null()).unwrap_or(false);
    let has_method = msg.get("method").is_some();
    // A RESPONSE to the editor's own request (`id` present, no `method`;
    // responses to Verter-injected `verter:*` requests were already demuxed
    // upstream): filter the `result` by shape. A response owes the SERVER
    // no reply; its deny outcome is the method-aware neutral when the
    // editor request is tracked, a whole-frame drop otherwise.
    if has_id && !has_method {
        return filter_response(msg, carriers, editor_pending_method);
    }
    // A server→client REQUEST (`id` + `method`): NEVER editor-routed —
    // forwarding (raw or filtered) would either leak carrier data or be a
    // partial-apply lie (a `workspace/applyEdit` whose carrier entries were
    // silently dropped while the editor answers `applied:true`), and
    // dropping would leave the server waiting forever. Every
    // carrier-referencing server request is answered on the editor's behalf
    // (`workspace/applyEdit` → `{applied:false}`, anything else → the
    // method-agnostic JSON-RPC error).
    if has_id && has_method {
        return EgressDecision::AnswerServer(synthesize_server_response(msg));
    }
    // A carrier-referencing NOTIFICATION (`method`, no `id`) — carrier
    // `textDocument/publishDiagnostics`, any unrecognized notification —
    // drops whole (no reply owed).
    EgressDecision::Suppress
}

/// Filter a carrier-referencing RESPONSE: strip carrier entries from the
/// recognized `result` shapes and re-encode. Every deny path — no `result`
/// (e.g. an error response naming a carrier), an unrecognized shape, or a
/// carrier URI surviving the recognized filters — resolves through
/// [`deny_response`]: the method-valid neutral to the editor when the
/// responded-to editor request is tracked, the whole-frame drop otherwise
/// (fail-closed).
fn filter_response(
    msg: &Value,
    carriers: &CarrierMatcher,
    editor_pending_method: Option<&str>,
) -> EgressDecision {
    let Some(result) = msg.get("result") else {
        // Nothing recognized to strip — deny.
        return deny_response(msg, editor_pending_method);
    };
    let Some(filtered) = filter_result_shape(result, carriers) else {
        return deny_response(msg, editor_pending_method);
    };
    let mut rebuilt = msg.clone();
    let Some(obj) = rebuilt.as_object_mut() else {
        return deny_response(msg, editor_pending_method);
    };
    obj.insert("result".to_string(), filtered);
    if references_carrier(&rebuilt, carriers) {
        return deny_response(msg, editor_pending_method);
    }
    EgressDecision::FilterCarrierEntries(rebuilt)
}

/// The deny outcome for a carrier-referencing RESPONSE no recognized filter
/// can strip clean: with a TRACKED editor request (`editor_pending_method`
/// present) the relay owes the editor a resolution — answer with the
/// method-valid neutral ([`synthesize_editor_response`]); with NO tracked
/// request, drop whole (fail-closed — never fabricate a reply for an id the
/// relay did not track).
fn deny_response(msg: &Value, editor_pending_method: Option<&str>) -> EgressDecision {
    match editor_pending_method {
        Some(method) => EgressDecision::AnswerEditor(synthesize_editor_response(msg, method)),
        None => EgressDecision::Suppress,
    }
}

/// The LSP request methods whose result type explicitly admits `null` — the
/// ONLY methods whose suppressed carrier-referencing response completes with a
/// synthesized `result: null` (protocol dispatch on the JSON-RPC method
/// string — routing, not a semantic heuristic). A method ABSENT from this
/// allowlist completes as the sanitized `-32803` error instead (fail-closed;
/// see [`synthesize_editor_response`]). The list stays CONSERVATIVE: a missing
/// null-valid method only OVER-errors (a harmless lost completion), but a
/// wrongly-INCLUDED non-null method would emit a protocol-invalid
/// `result: null` — so completeness is a nice-to-have, fail-closed is the
/// invariant.
const NULL_VALID_METHODS: &[&str] = &[
    // Navigation / lookup — `X | null`.
    "textDocument/hover",
    "textDocument/declaration",
    "textDocument/definition",
    "textDocument/typeDefinition",
    "textDocument/implementation",
    "textDocument/references",
    "textDocument/documentHighlight",
    "textDocument/documentSymbol",
    "textDocument/moniker",
    "textDocument/linkedEditingRange",
    // Completion / signature — `X | null`.
    "textDocument/completion",
    "textDocument/signatureHelp",
    // Code actions / lenses / links — `X[] | null`.
    "textDocument/codeAction",
    "textDocument/codeLens",
    "textDocument/documentLink",
    // Formatting — `TextEdit[] | null`.
    "textDocument/formatting",
    "textDocument/rangeFormatting",
    "textDocument/onTypeFormatting",
    "textDocument/willSaveWaitUntil",
    // Rename — `WorkspaceEdit | null` / `Range … | null`.
    "textDocument/rename",
    "textDocument/prepareRename",
    // Ranges / hierarchies — `X[] | null`.
    "textDocument/foldingRange",
    "textDocument/selectionRange",
    "textDocument/prepareCallHierarchy",
    "callHierarchy/incomingCalls",
    "callHierarchy/outgoingCalls",
    "textDocument/prepareTypeHierarchy",
    "typeHierarchy/supertypes",
    "typeHierarchy/subtypes",
    // Semantic tokens — `X | null`.
    "textDocument/semanticTokens/full",
    "textDocument/semanticTokens/full/delta",
    "textDocument/semanticTokens/range",
    // Inlay hints / inline values — `X[] | null`.
    "textDocument/inlayHint",
    "textDocument/inlineValue",
    // Workspace — `X | null`.
    "workspace/symbol",
    "workspace/executeCommand",
    "workspace/willCreateFiles",
    "workspace/willRenameFiles",
    "workspace/willDeleteFiles",
];

/// Synthesize the NEUTRAL response that completes a tracked editor request
/// whose real (carrier-referencing) response was kept from the editor,
/// carrying the ORIGINAL request id and NO carrier data. FAIL-CLOSED: the
/// default is the sanitized JSON-RPC `RequestFailed` error (`-32803`) with a
/// fixed benign message; `{"result": null}` is produced ONLY for a method on
/// the explicit [`NULL_VALID_METHODS`] allowlist (an LSP result type that
/// admits `null`). A method ABSENT from the allowlist — an unknown/custom
/// method, or a non-null-result method (`textDocument/diagnostic`,
/// `workspace/diagnostic`, `textDocument/documentColor`,
/// `textDocument/colorPresentation`, the `*/resolve` family, `initialize`) —
/// errors, never emitting a protocol-invalid `result: null`. The safety
/// asymmetry is deliberate: an omitted null-valid method only over-errors
/// (harmless), a wrongly-listed non-null method would leak an invalid null.
/// Pure — the caller routes the value to the editor transport.
fn synthesize_editor_response(msg: &Value, method: &str) -> Value {
    let id = msg.get("id").cloned().unwrap_or(Value::Null);
    if NULL_VALID_METHODS.contains(&method) {
        serde_json::json!({
            "jsonrpc": "2.0",
            "id": id,
            "result": Value::Null,
        })
    } else {
        serde_json::json!({
            "jsonrpc": "2.0",
            "id": id,
            "error": { "code": -32803, "message": "request failed" },
        })
    }
}

/// Strip carrier entries from a recognized response `result` shape. `None`
/// means the shape is unrecognized (the caller denies the response through
/// [`deny_response`] — it never forwards):
///
/// - an ARRAY of entries — workspace/document symbols
///   (`{location:{uri}}`), references / implementation / typeDefinition /
///   documentHighlight Locations (`{uri}`), definition LocationLinks
///   (`{targetUri}`), bare completion arrays — drops every entry that
///   references a carrier anywhere;
/// - a `WorkspaceEdit` (`changes` keyed by URI and/or `documentChanges`) —
///   see [`filter_workspace_edit`];
/// - an `items`-bearing object — completion lists, workspace/diagnostic
///   reports — drops the carrier-referencing items (provider-specific item
///   shapes bearing carrier edits/imports are suppressible per item).
fn filter_result_shape(result: &Value, carriers: &CarrierMatcher) -> Option<Value> {
    match result {
        Value::Array(items) => Some(Value::Array(retain_carrier_free(items, carriers))),
        Value::Object(map) => {
            if map.contains_key("changes") || map.contains_key("documentChanges") {
                return Some(filter_workspace_edit(result, carriers));
            }
            if let Some(Value::Array(items)) = map.get("items") {
                let mut filtered = map.clone();
                filtered.insert(
                    "items".to_string(),
                    Value::Array(retain_carrier_free(items, carriers)),
                );
                return Some(Value::Object(filtered));
            }
            None
        }
        _ => None,
    }
}

/// Strip carrier entries from a `WorkspaceEdit`: `changes` entries KEYED by
/// a carrier URI and `documentChanges` elements referencing a carrier
/// anywhere (`textDocument.uri`, create/rename/delete file-operation URIs)
/// are dropped; user entries survive. A non-object edit passes through
/// unchanged (the caller's post-filter carrier re-scan then denies the
/// frame — it never forwards with a surviving carrier reference).
fn filter_workspace_edit(edit: &Value, carriers: &CarrierMatcher) -> Value {
    let Some(map) = edit.as_object() else {
        return edit.clone();
    };
    let mut filtered = map.clone();
    if let Some(Value::Object(changes)) = map.get("changes") {
        let kept: serde_json::Map<String, Value> = changes
            .iter()
            .filter(|(uri, _)| !carriers.matches(uri))
            .map(|(k, v)| (k.clone(), v.clone()))
            .collect();
        filtered.insert("changes".to_string(), Value::Object(kept));
    }
    if let Some(Value::Array(doc_changes)) = map.get("documentChanges") {
        filtered.insert(
            "documentChanges".to_string(),
            Value::Array(retain_carrier_free(doc_changes, carriers)),
        );
    }
    Value::Object(filtered)
}

/// The elements of `items` that reference NO carrier URI anywhere (per-entry
/// drop over the recognized array channels).
fn retain_carrier_free(items: &[Value], carriers: &CarrierMatcher) -> Vec<Value> {
    items
        .iter()
        .filter(|item| !references_carrier(item, carriers))
        .cloned()
        .collect()
}

#[cfg(test)]
#[path = "egress_tests.rs"]
mod tests;
