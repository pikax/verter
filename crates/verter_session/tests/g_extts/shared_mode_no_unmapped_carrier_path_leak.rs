//! Guard: `shared_mode_no_unmapped_carrier_path_leak`.
//!
//! The deny-by-default server→editor EGRESS policy for Verter carrier
//! overlays: on the shared/non-owning path — an editor-owned `tsgo --lsp`
//! engine Verter relays for — the relay must never forward a
//! carrier-attributed frame to the editor unmapped. Carrier authority is the
//! relay's monotonic `carrier_egress_taint` set (the URIs Verter itself EVER
//! injected via `didOpen` — tainted before the wire send, never removed on
//! `didClose`), never a generated-suffix heuristic; positions are NOT mapped
//! (carrier-referencing entries on position-mapped channels drop fail-closed
//! until a live `ProviderPositionMapper` can present source locations).
//!
//! `verter_session` does not link `verter_tsgo_api`, so — like
//! `shared_mode_requires_full_ts_lsp_proxy` — this guard asserts the SOURCE
//! STRUCTURE of `crates/verter_tsgo_api/src/relay.rs` + `egress.rs`; the
//! BEHAVIORAL proofs live in `crates/verter_tsgo_api/src/egress_tests.rs`
//! (the decision table: carrier publishDiagnostics suppressed, mixed
//! responses filtered per entry with the user entry surviving, unrecognized
//! carrier-referencing frames suppressed fail-closed, suppressed server
//! requests answered on the server's behalf, carrier-free frames forwarded)
//! and `crates/verter_tsgo_api/src/relay_tests.rs` (whole-pump wiring over
//! in-memory transports). Six structural facts:
//!
//!   1. `server_to_editor_pump` routes every non-demuxed frame through
//!      `classify_egress` — the raw byte-identical forward is NOT
//!      unconditional; every raw write sits INSIDE the
//!      `EgressDecision::Forward` arm's brace span, after the classifier
//!      ran (a raw write after the match is an unconditional leak).
//!   2. `classify_egress` is deny-by-default: the sole
//!      `EgressDecision::Forward` production site in `egress.rs` is the
//!      carrier-FREE fast path (gated by the deep `references_carrier`
//!      scan); the carrier-referencing branch can only answer, suppress,
//!      or filter.
//!   3. `EgressDecision` is the closed five-arm vocabulary (`Forward` /
//!      `Suppress` / `FilterCarrierEntries` / `AnswerServer` /
//!      `AnswerEditor`) and the pump acts on it: `Suppress` drops the frame
//!      (recorded on the `suppressed_egress` counter, nothing written), and
//!      the filtered frame is re-encoded via `encode_message` inside the
//!      filter arm — never inside the forward arm (carrier-free frames stay
//!      byte-identical).
//!   4. The pump's `AnswerServer` arm answers the SERVER: the synthesized
//!      response for a suppressed server→client request is encoded and sent
//!      through the serialized server writer (`server_tx`), never through
//!      the editor transport, and the not-forwarded frame is recorded on
//!      the `suppressed_egress` counter.
//!   5. The pump's `AnswerEditor` arm answers the EDITOR: the synthesized
//!      carrier-free neutral that completes a tracked editor request whose
//!      carrier-referencing response was kept back is re-encoded via
//!      `encode_message` and written to the editor transport
//!      (`editor_write`) — never the RAW carrier bytes (fact 1's arm-span
//!      invariant keeps `write_all(&raw` exclusive to the `Forward` arm),
//!      never through the server writer (`server_tx`) — and the
//!      kept-from-editor frame is recorded on the `suppressed_egress`
//!      counter.
//!   6. The pump answers a reserved-id (`verter:*`) server→client REQUEST
//!      to the SERVER before the egress classifier runs: the synthesized
//!      negative (`synthesize_server_response`) is sent through the
//!      serialized server writer (`server_tx`) and recorded on the
//!      `suppressed_egress` counter, and the interception never writes to
//!      the editor transport — a forwarded reserved-id request could only
//!      be answered under a reserved id the editor→server pump drops,
//!      leaving the server's request unresolved forever.
//!
//! The inline self-test proves the predicates DISCRIMINATE: a pump body that
//! writes raw without a `classify_egress` call fails; a pump that forwards
//! raw before classifying fails; a pump with an unconditional raw write
//! after the match (outside the Forward arm's braces) fails; a classifier
//! whose carrier-referencing branch produces `EgressDecision::Forward`
//! fails; a classifier that forwards before the carrier scan fails; a pump
//! whose filter arm never re-encodes fails; an answer arm that writes to
//! the editor transport fails; an answer-editor arm that never re-encodes,
//! routes to the server writer, or skips the counter fails; an
//! answer-editor arm forwarding the raw carrier bytes fails fact 1's
//! arm-span invariant; and a pump without the reserved-id request
//! interception, with the interception after the classifier, or with an
//! interception that editor-routes fails fact 6.

use std::fs;
use std::path::PathBuf;

/// Repo root (two parents up from `crates/verter_session`).
fn workspace_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("crates/")
        .parent()
        .expect("repo root")
        .to_path_buf()
}

fn read_source(rel: &str) -> String {
    let path = workspace_root().join(rel);
    fs::read_to_string(&path).unwrap_or_else(|e| panic!("read {}: {e}", path.display()))
}

/// The byte span `(start, end)` of the body of the block (fn / enum) whose
/// header starts with `sig`, extracted by a brace-depth scan: find `sig`,
/// advance to its opening `{`, take to the matching `}`. `None` if absent or
/// unbalanced.
fn block_span(src: &str, sig: &str) -> Option<(usize, usize)> {
    let sig_at = src.find(sig)?;
    let after_sig = &src[sig_at..];
    let open_rel = after_sig.find('{')?;
    let bytes = after_sig.as_bytes();
    let mut depth = 0usize;
    let mut i = open_rel;
    while i < bytes.len() {
        match bytes[i] {
            b'{' => depth += 1,
            b'}' => {
                depth -= 1;
                if depth == 0 {
                    return Some((sig_at + open_rel + 1, sig_at + i));
                }
            }
            _ => {}
        }
        i += 1;
    }
    None
}

/// The body text of the block whose header starts with `sig` (see
/// [`block_span`]).
fn block_body(src: &str, sig: &str) -> Option<String> {
    block_span(src, sig).map(|(start, end)| src[start..end].to_string())
}

/// The byte span `(start, end)` of the braced body of the match arm whose
/// pattern starts with `arm_token`: find `arm_token`, advance past its `=>`,
/// require the next non-whitespace token to be `{` (a non-braced arm yields
/// `None` — the guard demands a braced arm body it can bound), and take to
/// the matching `}` by brace-depth scan.
fn arm_span(src: &str, arm_token: &str) -> Option<(usize, usize)> {
    let arm_at = src.find(arm_token)?;
    let after_arrow = arm_at + src[arm_at..].find("=>")? + 2;
    let rest = &src[after_arrow..];
    let open_rel = rest.find(|c: char| !c.is_whitespace())?;
    if rest.as_bytes()[open_rel] != b'{' {
        return None;
    }
    let open = after_arrow + open_rel;
    let bytes = src.as_bytes();
    let mut depth = 0usize;
    let mut i = open;
    while i < bytes.len() {
        match bytes[i] {
            b'{' => depth += 1,
            b'}' => {
                depth -= 1;
                if depth == 0 {
                    return Some((open + 1, i));
                }
            }
            _ => {}
        }
        i += 1;
    }
    None
}

/// Fact 1: the pump classifies every non-demuxed frame BEFORE any raw
/// forward, and every raw write sits INSIDE the `EgressDecision::Forward`
/// arm's brace span — a raw write merely textually after the arm token
/// (e.g. an unconditional write after the match) fails. Returns failures
/// (empty ⇒ pass).
fn pump_egress_routing_failures(pump_body: &str) -> Vec<String> {
    let mut failures = Vec::new();
    let Some(classify_at) = pump_body.find("classify_egress") else {
        failures.push(
            "`server_to_editor_pump` must route every non-demuxed frame \
             through `classify_egress` — an unconditional raw forward leaks \
             carrier frames to the editor"
                .to_string(),
        );
        return failures;
    };
    let Some((forward_start, forward_end)) = arm_span(pump_body, "EgressDecision::Forward") else {
        failures.push(
            "the pump must gate its raw byte-identical forward behind a \
             braced `EgressDecision::Forward` match arm"
                .to_string(),
        );
        return failures;
    };
    let needle = "write_all(&raw";
    let mut raw_writes = 0usize;
    let mut from = 0usize;
    while let Some(rel) = pump_body[from..].find(needle) {
        let at = from + rel;
        raw_writes += 1;
        if at < classify_at {
            failures.push(format!(
                "the pump forwards raw bytes at byte {at}, BEFORE the \
                 `classify_egress` call at byte {classify_at} — the egress \
                 policy must run before any raw forward"
            ));
        }
        if at < forward_start || at >= forward_end {
            failures.push(format!(
                "the pump forwards raw bytes at byte {at}, OUTSIDE the \
                 `EgressDecision::Forward` arm's body (bytes {forward_start}..\
                 {forward_end}) — every raw forward must sit inside the \
                 forward arm's braces; an unconditional raw write after the \
                 match forwards every classified frame anyway"
            ));
        }
        from = at + needle.len();
    }
    if raw_writes == 0 {
        failures.push(
            "the pump must still carry the RAW byte-identical forward for \
             carrier-free frames (`write_all(&raw`) — the scan must hit the \
             real path (non-vacuous)"
                .to_string(),
        );
    }
    failures
}

/// Fact 2: `classify_egress` is deny-by-default — every
/// `EgressDecision::Forward` production site in egress.rs sits inside the
/// `classify_egress` body, gated by the deep `references_carrier` scan; the
/// carrier-referencing branch never forwards. Returns failures (empty ⇒
/// pass).
fn classifier_forward_gate_failures(egress_src: &str) -> Vec<String> {
    let mut failures = Vec::new();
    let Some((classify_start, classify_end)) =
        block_span(egress_src, "pub(crate) fn classify_egress")
    else {
        failures.push(
            "egress.rs must carry `pub(crate) fn classify_egress` — the \
             single egress classifier"
                .to_string(),
        );
        return failures;
    };
    let needle = "EgressDecision::Forward";
    let mut inside: Vec<usize> = Vec::new();
    let mut from = 0usize;
    while let Some(rel) = egress_src[from..].find(needle) {
        let at = from + rel;
        if at >= classify_start && at < classify_end {
            inside.push(at);
        } else {
            failures.push(format!(
                "egress.rs produces `EgressDecision::Forward` at byte {at}, \
                 OUTSIDE the `classify_egress` body — the forward decision \
                 belongs exclusively to the carrier-free fast path; a \
                 carrier-referencing branch that forwards leaks unmapped \
                 carrier data to the editor"
            ));
        }
        from = at + needle.len();
    }
    let body = &egress_src[classify_start..classify_end];
    let Some(scan_at) = body.find("references_carrier") else {
        failures.push(
            "`classify_egress` must gate the forward decision on the deep \
             `references_carrier` scan (deny-by-default)"
                .to_string(),
        );
        return failures;
    };
    if inside.is_empty() {
        failures.push(
            "`classify_egress` must produce `EgressDecision::Forward` on the \
             carrier-free fast path — the scan must hit the real classifier \
             (non-vacuous)"
                .to_string(),
        );
    }
    for at in &inside {
        let rel = at - classify_start;
        if rel < scan_at {
            failures.push(format!(
                "`classify_egress` forwards (byte {rel} into the body) BEFORE \
                 the `references_carrier` carrier scan (byte {scan_at}) — \
                 deny-by-default requires the scan to gate the forward"
            ));
        }
    }
    if !body.contains("classify_carrier_referencing") {
        failures.push(
            "`classify_egress` must delegate carrier-referencing frames to \
             the deny branch (`classify_carrier_referencing`) — suppress or \
             filter, never forward"
                .to_string(),
        );
    }
    failures
}

/// Fact 3a: the closed five-arm decision vocabulary. Returns failures
/// (empty ⇒ pass).
fn decision_vocabulary_failures(enum_body: &str) -> Vec<String> {
    let mut failures = Vec::new();
    for variant in [
        "Forward",
        "Suppress",
        "FilterCarrierEntries",
        "AnswerServer",
        "AnswerEditor",
    ] {
        if !enum_body.contains(variant) {
            failures.push(format!(
                "the `EgressDecision` enum must carry the `{variant}` arm — \
                 the five-arm forward/suppress/filter/answer-server/\
                 answer-editor vocabulary is the closed egress decision set"
            ));
        }
    }
    failures
}

/// Fact 3b: the pump acts on the decision — `Suppress` drops and records,
/// the filtered frame re-encodes INSIDE the filter arm, and the forward arm
/// never re-encodes (carrier-free frames stay byte-identical). Returns
/// failures (empty ⇒ pass).
fn pump_decision_action_failures(pump_body: &str) -> Vec<String> {
    let mut failures = Vec::new();
    if !pump_body.contains("EgressDecision::Suppress") {
        failures.push(
            "the pump must handle the `EgressDecision::Suppress` arm (the \
             whole-frame drop)"
                .to_string(),
        );
    }
    if !pump_body.contains("suppressed_egress") {
        failures.push(
            "the pump's suppress arm must RECORD the drop on the \
             `suppressed_egress` counter (an unobservable drop is \
             indistinguishable from a dead wire)"
                .to_string(),
        );
    }
    match arm_span(pump_body, "EgressDecision::FilterCarrierEntries") {
        None => failures.push(
            "the pump must handle the `EgressDecision::FilterCarrierEntries` \
             arm (the per-entry filtered frame) in a braced match arm"
                .to_string(),
        ),
        Some((start, end)) => {
            if !pump_body[start..end].contains("encode_message") {
                failures.push(
                    "the pump must re-encode the filtered frame via \
                     `encode_message` INSIDE the filter arm — the filtered \
                     value, not the raw bytes, reaches the editor"
                        .to_string(),
                );
            }
        }
    }
    if let Some((start, end)) = arm_span(pump_body, "EgressDecision::Forward") {
        if pump_body[start..end].contains("encode_message") {
            failures.push(
                "the pump's `Forward` arm must never re-encode \
                 (`encode_message`) — carrier-free frames stay byte-identical"
                    .to_string(),
            );
        }
    }
    failures
}

/// Fact 4: the pump's `AnswerServer` arm answers the SERVER — the
/// synthesized response is encoded and sent through the serialized server
/// writer (`server_tx`), never through the editor transport, and the
/// not-forwarded frame is recorded on the `suppressed_egress` counter.
/// Returns failures (empty ⇒ pass).
fn pump_answer_server_failures(pump_body: &str) -> Vec<String> {
    let mut failures = Vec::new();
    let Some((start, end)) = arm_span(pump_body, "EgressDecision::AnswerServer") else {
        failures.push(
            "the pump must handle the `EgressDecision::AnswerServer` arm in \
             a braced match arm — a suppressed server→client request that is \
             not answered leaves the server waiting forever"
                .to_string(),
        );
        return failures;
    };
    let arm = &pump_body[start..end];
    if !arm.contains("server_tx") {
        failures.push(
            "the pump's `AnswerServer` arm must send the synthesized \
             response through the one serialized server writer (`server_tx`), \
             so server-bound writes never interleave mid-frame"
                .to_string(),
        );
    }
    if !arm.contains("encode_message") {
        failures.push(
            "the pump's `AnswerServer` arm must encode the synthesized \
             response (`encode_message`) before sending it to the server"
                .to_string(),
        );
    }
    if arm.contains("editor_write") {
        failures.push(
            "the pump's `AnswerServer` arm must NEVER write to the editor \
             transport (`editor_write`) — the suppressed request and its \
             synthesized answer are server-side only"
                .to_string(),
        );
    }
    if !arm.contains("suppressed_egress") {
        failures.push(
            "the pump's `AnswerServer` arm must record the \
             not-forwarded-to-editor frame on the `suppressed_egress` \
             counter"
                .to_string(),
        );
    }
    failures
}

/// Fact 5: the pump's `AnswerEditor` arm answers the EDITOR — the
/// synthesized carrier-free neutral is re-encoded (`encode_message`) and
/// written to the editor transport (`editor_write`), never sent through the
/// server writer (`server_tx`), and the kept-from-editor frame is recorded
/// on the `suppressed_egress` counter. (Fact 1's arm-span invariant already
/// keeps the RAW byte forward — `write_all(&raw` — exclusive to the
/// `Forward` arm, so this arm can never leak the original carrier bytes.)
/// Returns failures (empty ⇒ pass).
fn pump_answer_editor_failures(pump_body: &str) -> Vec<String> {
    let mut failures = Vec::new();
    let Some((start, end)) = arm_span(pump_body, "EgressDecision::AnswerEditor") else {
        failures.push(
            "the pump must handle the `EgressDecision::AnswerEditor` arm in \
             a braced match arm — a suppressed carrier-referencing response to a \
             tracked editor request that is not completed leaves the \
             editor's request stranded forever"
                .to_string(),
        );
        return failures;
    };
    let arm = &pump_body[start..end];
    if !arm.contains("encode_message") {
        failures.push(
            "the pump's `AnswerEditor` arm must encode the synthesized \
             neutral (`encode_message`) — the carrier-free replacement, \
             never the original frame, reaches the editor"
                .to_string(),
        );
    }
    if !arm.contains("editor_write") {
        failures.push(
            "the pump's `AnswerEditor` arm must write the synthesized \
             neutral to the editor transport (`editor_write`) — that write \
             is what resolves the editor's pending request"
                .to_string(),
        );
    }
    if arm.contains("server_tx") {
        failures.push(
            "the pump's `AnswerEditor` arm must NEVER send through the \
             serialized server writer (`server_tx`) — the neutral completes \
             an EDITOR request; the server is owed nothing"
                .to_string(),
        );
    }
    if !arm.contains("suppressed_egress") {
        failures.push(
            "the pump's `AnswerEditor` arm must record the \
             kept-from-editor carrier frame on the `suppressed_egress` \
             counter"
                .to_string(),
        );
    }
    failures
}

/// Fact 6: the pump answers a reserved-id (`verter:*`) server→client
/// REQUEST to the SERVER before the egress classifier runs — the
/// synthesized negative (`synthesize_server_response`) is sent through the
/// serialized server writer (`server_tx`) and recorded on the
/// `suppressed_egress` counter, and the interception block (bounded from
/// the synthesis call to its `continue`) never writes to the editor
/// transport (`editor_write`). A forwarded reserved-id request could only
/// be answered under a reserved id the editor→server pump drops as a
/// reservation violation, leaving the server's request unresolved forever.
/// Returns failures (empty ⇒ pass).
fn pump_reserved_id_request_answer_failures(pump_body: &str) -> Vec<String> {
    let mut failures = Vec::new();
    let Some(synth_at) = pump_body.find("synthesize_server_response") else {
        failures.push(
            "`server_to_editor_pump` must intercept a reserved-id \
             (`verter:*`) server→client REQUEST and answer the SERVER with \
             the synthesized negative (`synthesize_server_response`) — a \
             forwarded reserved-id request hangs the server (its answer \
             could only carry a reserved id the editor→server pump drops)"
                .to_string(),
        );
        return failures;
    };
    let Some(classify_at) = pump_body.find("classify_egress") else {
        failures.push(
            "`server_to_editor_pump` must still route frames through \
             `classify_egress` — fact 6 orders the reserved-id interception \
             against it"
                .to_string(),
        );
        return failures;
    };
    if synth_at > classify_at {
        failures.push(format!(
            "the pump synthesizes the reserved-id request answer at byte \
             {synth_at}, AFTER the `classify_egress` call at byte \
             {classify_at} — the anomaly must be intercepted BEFORE the \
             egress classifier (it is a namespace fact, not a carrier fact)"
        ));
        return failures;
    }
    let Some(continue_rel) = pump_body[synth_at..].find("continue") else {
        failures.push(
            "the reserved-id interception must end in a `continue` — the \
             answered request must never fall through to classification or \
             forwarding"
                .to_string(),
        );
        return failures;
    };
    let interception = &pump_body[synth_at..synth_at + continue_rel];
    if !interception.contains("server_tx") {
        failures.push(
            "the reserved-id interception must send the synthesized \
             negative through the serialized server writer (`server_tx`) — \
             the answer is server-bound"
                .to_string(),
        );
    }
    if !interception.contains("suppressed_egress") {
        failures.push(
            "the reserved-id interception must record the answered anomaly \
             on the `suppressed_egress` counter"
                .to_string(),
        );
    }
    if interception.contains("editor_write") {
        failures.push(
            "the reserved-id interception must NEVER write to the editor \
             transport (`editor_write`) — the anomaly answer is server-side \
             only"
                .to_string(),
        );
    }
    failures
}

#[test]
fn shared_mode_no_unmapped_carrier_path_leak() {
    let relay_src = read_source("crates/verter_tsgo_api/src/relay.rs");
    let egress_src = read_source("crates/verter_tsgo_api/src/egress.rs");
    let pump_body = block_body(&relay_src, "async fn server_to_editor_pump")
        .expect("relay.rs must carry the server→editor pump");
    let enum_body = block_body(&egress_src, "pub(crate) enum EgressDecision")
        .expect("egress.rs must carry the `EgressDecision` enum");

    let mut failures = pump_egress_routing_failures(&pump_body);
    failures.extend(classifier_forward_gate_failures(&egress_src));
    failures.extend(decision_vocabulary_failures(&enum_body));
    failures.extend(pump_decision_action_failures(&pump_body));
    failures.extend(pump_answer_server_failures(&pump_body));
    failures.extend(pump_answer_editor_failures(&pump_body));
    failures.extend(pump_reserved_id_request_answer_failures(&pump_body));

    assert!(
        failures.is_empty(),
        "the deny-by-default server→editor carrier egress policy \
         (crates/verter_tsgo_api/src/relay.rs + egress.rs) is violated:\n{}",
        failures.join("\n")
    );
}

/// DISCRIMINATING self-test: each predicate FIRES on a violating sample and
/// PASSES on a conforming one — the guard is non-vacuous.
#[test]
fn shared_mode_no_unmapped_carrier_path_leak_self_test_discriminates() {
    // A conforming pump body: demux, then the reserved-id request answer,
    // then classify, then act per arm.
    let good_pump = r#"
        if is_response && frame_carries_verter_id(&msg) { continue; }
        if is_request && frame_carries_verter_id(&msg) {
            let resp = synthesize_server_response(&msg);
            let _ = server_tx.send(encode_message(&resp)).await;
            suppressed_egress.fetch_add(1, Ordering::Relaxed);
            continue;
        }
        let carriers = snapshot();
        match classify_egress(&msg, &carriers, editor_pending_method.as_deref()) {
            EgressDecision::Forward => {
                if editor_write.write_all(&raw).await.is_err() { break; }
            }
            EgressDecision::Suppress => {
                suppressed_egress.fetch_add(1, Ordering::Relaxed);
            }
            EgressDecision::FilterCarrierEntries(filtered) => {
                let bytes = encode_message(&filtered);
                if editor_write.write_all(&bytes).await.is_err() { break; }
            }
            EgressDecision::AnswerServer(resp) => {
                let _ = server_tx.send(encode_message(&resp)).await;
                suppressed_egress.fetch_add(1, Ordering::Relaxed);
            }
            EgressDecision::AnswerEditor(resp) => {
                let bytes = encode_message(&resp);
                if editor_write.write_all(&bytes).await.is_err() { break; }
                suppressed_egress.fetch_add(1, Ordering::Relaxed);
            }
        }
    "#;
    assert!(
        pump_egress_routing_failures(good_pump).is_empty(),
        "a pump that classifies before its gated raw forward must pass"
    );
    assert!(
        pump_decision_action_failures(good_pump).is_empty(),
        "a pump that drops+records on suppress and re-encodes on filter must pass"
    );
    assert!(
        pump_answer_server_failures(good_pump).is_empty(),
        "an answer arm that encodes to the server writer and records must pass"
    );
    assert!(
        pump_answer_editor_failures(good_pump).is_empty(),
        "an answer-editor arm that encodes the neutral to the editor \
         transport and records must pass"
    );
    assert!(
        pump_reserved_id_request_answer_failures(good_pump).is_empty(),
        "a pump that answers the reserved-id request to the server writer \
         before classifying, records, and continues must pass"
    );

    // A pump WITHOUT the reserved-id request interception (the demux alone)
    // fails fact 6 — a forwarded reserved-id server request hangs the server.
    let no_reserved_id_interception = r#"
        if is_response && frame_carries_verter_id(&msg) { continue; }
        let carriers = snapshot();
        match classify_egress(&msg, &carriers, editor_pending_method.as_deref()) {
            EgressDecision::Forward => {
                if editor_write.write_all(&raw).await.is_err() { break; }
            }
            EgressDecision::Suppress => {
                suppressed_egress.fetch_add(1, Ordering::Relaxed);
            }
        }
    "#;
    assert!(
        pump_reserved_id_request_answer_failures(no_reserved_id_interception)
            .iter()
            .any(|f| f.contains("synthesize_server_response")),
        "a pump without the reserved-id request interception must fail"
    );

    // An interception placed AFTER the classifier fails on ordering — the
    // anomaly is a namespace fact and must never reach classification.
    let interception_after_classify = r#"
        if is_response && frame_carries_verter_id(&msg) { continue; }
        let carriers = snapshot();
        match classify_egress(&msg, &carriers, editor_pending_method.as_deref()) {
            EgressDecision::Forward => {
                if editor_write.write_all(&raw).await.is_err() { break; }
            }
        }
        if is_request && frame_carries_verter_id(&msg) {
            let resp = synthesize_server_response(&msg);
            let _ = server_tx.send(encode_message(&resp)).await;
            suppressed_egress.fetch_add(1, Ordering::Relaxed);
            continue;
        }
    "#;
    assert!(
        pump_reserved_id_request_answer_failures(interception_after_classify)
            .iter()
            .any(|f| f.contains("AFTER")),
        "an interception after the classifier must fail on ordering"
    );

    // An interception that editor-routes the anomaly answer fails — the
    // answer is server-bound only.
    let interception_to_editor = r#"
        if is_response && frame_carries_verter_id(&msg) { continue; }
        if is_request && frame_carries_verter_id(&msg) {
            let resp = synthesize_server_response(&msg);
            let bytes = encode_message(&resp);
            if editor_write.write_all(&bytes).await.is_err() { break; }
            let _ = server_tx.send(encode_message(&resp)).await;
            suppressed_egress.fetch_add(1, Ordering::Relaxed);
            continue;
        }
        let carriers = snapshot();
        match classify_egress(&msg, &carriers, editor_pending_method.as_deref()) {
            EgressDecision::Forward => {
                if editor_write.write_all(&raw).await.is_err() { break; }
            }
        }
    "#;
    assert!(
        pump_reserved_id_request_answer_failures(interception_to_editor)
            .iter()
            .any(|f| f.contains("editor_write")),
        "an interception writing the anomaly answer to the editor transport \
         must fail"
    );

    // An interception that records nothing fails.
    let interception_unrecorded = r#"
        if is_response && frame_carries_verter_id(&msg) { continue; }
        if is_request && frame_carries_verter_id(&msg) {
            let resp = synthesize_server_response(&msg);
            let _ = server_tx.send(encode_message(&resp)).await;
            continue;
        }
        let carriers = snapshot();
        match classify_egress(&msg, &carriers, editor_pending_method.as_deref()) {
            EgressDecision::Forward => {
                if editor_write.write_all(&raw).await.is_err() { break; }
            }
        }
    "#;
    assert!(
        pump_reserved_id_request_answer_failures(interception_unrecorded)
            .iter()
            .any(|f| f.contains("suppressed_egress")),
        "an interception that records nothing must fail"
    );

    // An UNCONDITIONAL raw write AFTER the match — textually after the
    // `EgressDecision::Forward` token but OUTSIDE the Forward arm's braces —
    // forwards every classified frame anyway and must fail.
    let raw_write_after_match = r#"
        match classify_egress(&msg, &carriers) {
            EgressDecision::Forward => {}
            EgressDecision::Suppress => {
                suppressed_egress.fetch_add(1, Ordering::Relaxed);
            }
            EgressDecision::FilterCarrierEntries(filtered) => {
                let bytes = encode_message(&filtered);
            }
            EgressDecision::AnswerServer(resp) => {
                let _ = server_tx.send(encode_message(&resp)).await;
                suppressed_egress.fetch_add(1, Ordering::Relaxed);
            }
        }
        if editor_write.write_all(&raw).await.is_err() { break; }
    "#;
    assert!(
        pump_egress_routing_failures(raw_write_after_match)
            .iter()
            .any(|f| f.contains("OUTSIDE")),
        "an unconditional raw write after the match (outside the Forward \
         arm's brace span) must fail — textual order alone is not proof"
    );

    // A pump that forwards raw UNCONDITIONALLY (no classifier) fails.
    let unconditional_pump = r#"
        if editor_write.write_all(&raw).await.is_err() { break; }
    "#;
    assert!(
        pump_egress_routing_failures(unconditional_pump)
            .iter()
            .any(|f| f.contains("classify_egress")),
        "a pump writing raw without `classify_egress` must fail"
    );

    // A pump that forwards raw BEFORE classifying fails on ordering.
    let forward_before_classify = r#"
        if editor_write.write_all(&raw).await.is_err() { break; }
        match classify_egress(&msg, &carriers) {
            EgressDecision::Forward => {}
            EgressDecision::Suppress => { suppressed_egress.fetch_add(1, Ordering::Relaxed); }
            EgressDecision::FilterCarrierEntries(filtered) => {
                let bytes = encode_message(&filtered);
            }
        }
    "#;
    assert!(
        pump_egress_routing_failures(forward_before_classify)
            .iter()
            .any(|f| f.contains("BEFORE")),
        "a pump forwarding raw before the classifier must fail"
    );

    // A pump whose filter arm never re-encodes fails.
    let no_reencode_pump = r#"
        match classify_egress(&msg, &carriers) {
            EgressDecision::Forward => {
                if editor_write.write_all(&raw).await.is_err() { break; }
            }
            EgressDecision::Suppress => {
                suppressed_egress.fetch_add(1, Ordering::Relaxed);
            }
            EgressDecision::FilterCarrierEntries(filtered) => {
                if editor_write.write_all(&raw).await.is_err() { break; }
            }
        }
    "#;
    assert!(
        pump_decision_action_failures(no_reencode_pump)
            .iter()
            .any(|f| f.contains("encode_message")),
        "a filter arm that writes raw instead of re-encoding must fail"
    );

    // A pump whose suppress arm records nothing fails.
    let unrecorded_suppress = r#"
        match classify_egress(&msg, &carriers) {
            EgressDecision::Forward => {
                if editor_write.write_all(&raw).await.is_err() { break; }
            }
            EgressDecision::Suppress => {}
            EgressDecision::FilterCarrierEntries(filtered) => {
                let bytes = encode_message(&filtered);
            }
        }
    "#;
    assert!(
        pump_decision_action_failures(unrecorded_suppress)
            .iter()
            .any(|f| f.contains("suppressed_egress")),
        "a suppress arm that records nothing must fail"
    );

    // A pump WITHOUT the AnswerServer arm fails — a suppressed server
    // request would drop with no reply and hang the server.
    assert!(
        pump_answer_server_failures(no_reencode_pump)
            .iter()
            .any(|f| f.contains("AnswerServer")),
        "a pump without the AnswerServer arm must fail"
    );

    // An answer arm that writes the synthesized response to the EDITOR
    // transport fails — the answer is server-side only.
    let answer_to_editor = r#"
        match classify_egress(&msg, &carriers) {
            EgressDecision::Forward => {
                if editor_write.write_all(&raw).await.is_err() { break; }
            }
            EgressDecision::Suppress => {
                suppressed_egress.fetch_add(1, Ordering::Relaxed);
            }
            EgressDecision::FilterCarrierEntries(filtered) => {
                let bytes = encode_message(&filtered);
                if editor_write.write_all(&bytes).await.is_err() { break; }
            }
            EgressDecision::AnswerServer(resp) => {
                let bytes = encode_message(&resp);
                if editor_write.write_all(&bytes).await.is_err() { break; }
                suppressed_egress.fetch_add(1, Ordering::Relaxed);
            }
        }
    "#;
    assert!(
        pump_answer_server_failures(answer_to_editor)
            .iter()
            .any(|f| f.contains("editor_write")),
        "an answer arm that writes to the editor transport must fail"
    );

    // An answer arm that silently swallows the response (no server write)
    // fails — the server would wait forever.
    let answer_dropped = r#"
        match classify_egress(&msg, &carriers) {
            EgressDecision::Forward => {
                if editor_write.write_all(&raw).await.is_err() { break; }
            }
            EgressDecision::Suppress => {
                suppressed_egress.fetch_add(1, Ordering::Relaxed);
            }
            EgressDecision::FilterCarrierEntries(filtered) => {
                let bytes = encode_message(&filtered);
                if editor_write.write_all(&bytes).await.is_err() { break; }
            }
            EgressDecision::AnswerServer(resp) => {
                suppressed_egress.fetch_add(1, Ordering::Relaxed);
            }
        }
    "#;
    assert!(
        pump_answer_server_failures(answer_dropped)
            .iter()
            .any(|f| f.contains("server_tx")),
        "an answer arm that never writes to the server must fail"
    );

    // A pump WITHOUT the AnswerEditor arm fails — a suppressed carrier-referencing
    // response to a tracked editor request would strand it forever.
    assert!(
        pump_answer_editor_failures(answer_dropped)
            .iter()
            .any(|f| f.contains("AnswerEditor")),
        "a pump without the AnswerEditor arm must fail"
    );

    // An answer-editor arm that routes the neutral through the SERVER
    // writer (and never the editor transport) fails on both counts — the
    // neutral completes an EDITOR request.
    let answer_editor_to_server = r#"
        match classify_egress(&msg, &carriers, editor_pending_method.as_deref()) {
            EgressDecision::Forward => {
                if editor_write.write_all(&raw).await.is_err() { break; }
            }
            EgressDecision::Suppress => {
                suppressed_egress.fetch_add(1, Ordering::Relaxed);
            }
            EgressDecision::FilterCarrierEntries(filtered) => {
                let bytes = encode_message(&filtered);
                if editor_write.write_all(&bytes).await.is_err() { break; }
            }
            EgressDecision::AnswerServer(resp) => {
                let _ = server_tx.send(encode_message(&resp)).await;
                suppressed_egress.fetch_add(1, Ordering::Relaxed);
            }
            EgressDecision::AnswerEditor(resp) => {
                let _ = server_tx.send(encode_message(&resp)).await;
                suppressed_egress.fetch_add(1, Ordering::Relaxed);
            }
        }
    "#;
    let misrouted = pump_answer_editor_failures(answer_editor_to_server);
    assert!(
        misrouted.iter().any(|f| f.contains("server_tx"))
            && misrouted.iter().any(|f| f.contains("editor_write")),
        "an answer-editor arm routing through the server writer instead of \
         the editor transport must fail on both predicates; got {misrouted:?}"
    );

    // An answer-editor arm that forwards the RAW carrier bytes instead of
    // the re-encoded neutral fails TWICE: the arm-level encode check AND
    // fact 1's arm-span invariant (a `write_all(&raw` outside the Forward
    // arm's braces) — the arm-span teeth cover the new arm.
    let answer_editor_raw = r#"
        match classify_egress(&msg, &carriers, editor_pending_method.as_deref()) {
            EgressDecision::Forward => {
                if editor_write.write_all(&raw).await.is_err() { break; }
            }
            EgressDecision::Suppress => {
                suppressed_egress.fetch_add(1, Ordering::Relaxed);
            }
            EgressDecision::FilterCarrierEntries(filtered) => {
                let bytes = encode_message(&filtered);
                if editor_write.write_all(&bytes).await.is_err() { break; }
            }
            EgressDecision::AnswerServer(resp) => {
                let _ = server_tx.send(encode_message(&resp)).await;
                suppressed_egress.fetch_add(1, Ordering::Relaxed);
            }
            EgressDecision::AnswerEditor(resp) => {
                if editor_write.write_all(&raw).await.is_err() { break; }
                suppressed_egress.fetch_add(1, Ordering::Relaxed);
            }
        }
    "#;
    assert!(
        pump_answer_editor_failures(answer_editor_raw)
            .iter()
            .any(|f| f.contains("encode_message")),
        "an answer-editor arm that never re-encodes must fail"
    );
    assert!(
        pump_egress_routing_failures(answer_editor_raw)
            .iter()
            .any(|f| f.contains("OUTSIDE")),
        "an answer-editor arm forwarding raw carrier bytes must fail the \
         Forward-arm-span invariant (raw writes are Forward-arm-exclusive)"
    );

    // An answer-editor arm that records nothing fails.
    let answer_editor_unrecorded = r#"
        match classify_egress(&msg, &carriers, editor_pending_method.as_deref()) {
            EgressDecision::Forward => {
                if editor_write.write_all(&raw).await.is_err() { break; }
            }
            EgressDecision::Suppress => {
                suppressed_egress.fetch_add(1, Ordering::Relaxed);
            }
            EgressDecision::FilterCarrierEntries(filtered) => {
                let bytes = encode_message(&filtered);
                if editor_write.write_all(&bytes).await.is_err() { break; }
            }
            EgressDecision::AnswerServer(resp) => {
                let _ = server_tx.send(encode_message(&resp)).await;
                suppressed_egress.fetch_add(1, Ordering::Relaxed);
            }
            EgressDecision::AnswerEditor(resp) => {
                let bytes = encode_message(&resp);
                if editor_write.write_all(&bytes).await.is_err() { break; }
            }
        }
    "#;
    assert!(
        pump_answer_editor_failures(answer_editor_unrecorded)
            .iter()
            .any(|f| f.contains("suppressed_egress")),
        "an answer-editor arm that records nothing must fail"
    );

    // A conforming classifier: the sole forward site is the carrier-free
    // fast path, gated by the deep scan; the carrier branch only denies.
    let good_classifier = r#"
pub(crate) fn classify_egress(msg: &Value, carrier_uris: &HashSet<String>) -> EgressDecision {
    if carrier_uris.is_empty() || !references_carrier(msg, carrier_uris) {
        return EgressDecision::Forward;
    }
    classify_carrier_referencing(msg, carrier_uris)
}
fn classify_carrier_referencing(msg: &Value, carrier_uris: &HashSet<String>) -> EgressDecision {
    EgressDecision::Suppress
}
"#;
    assert!(
        classifier_forward_gate_failures(good_classifier).is_empty(),
        "a classifier whose only forward site is the gated carrier-free path must pass"
    );

    // A classifier whose CARRIER branch forwards fails (a forward production
    // site outside the classify body).
    let leaking_classifier = r#"
pub(crate) fn classify_egress(msg: &Value, carrier_uris: &HashSet<String>) -> EgressDecision {
    if carrier_uris.is_empty() || !references_carrier(msg, carrier_uris) {
        return EgressDecision::Forward;
    }
    classify_carrier_referencing(msg, carrier_uris)
}
fn classify_carrier_referencing(msg: &Value, carrier_uris: &HashSet<String>) -> EgressDecision {
    EgressDecision::Forward
}
"#;
    assert!(
        classifier_forward_gate_failures(leaking_classifier)
            .iter()
            .any(|f| f.contains("OUTSIDE")),
        "a carrier branch that forwards must fail"
    );

    // A classifier that forwards BEFORE the carrier scan fails on ordering.
    let ungated_classifier = r#"
pub(crate) fn classify_egress(msg: &Value, carrier_uris: &HashSet<String>) -> EgressDecision {
    if msg.get("method").is_none() {
        return EgressDecision::Forward;
    }
    if references_carrier(msg, carrier_uris) {
        return classify_carrier_referencing(msg, carrier_uris);
    }
    classify_carrier_referencing(msg, carrier_uris)
}
"#;
    assert!(
        classifier_forward_gate_failures(ungated_classifier)
            .iter()
            .any(|f| f.contains("BEFORE")),
        "a classifier forwarding before the carrier scan must fail"
    );

    // A classifier without the deep scan at all fails.
    let scanless_classifier = r#"
pub(crate) fn classify_egress(msg: &Value, carrier_uris: &HashSet<String>) -> EgressDecision {
    EgressDecision::Forward
}
"#;
    assert!(
        !classifier_forward_gate_failures(scanless_classifier).is_empty(),
        "a classifier without the `references_carrier` scan must fail"
    );

    // A source without the classifier fails on PRESENCE.
    assert!(
        !classifier_forward_gate_failures("fn other() {}").is_empty(),
        "a source without `classify_egress` must fail (non-vacuous)"
    );

    // The closed vocabulary: all five arms pass; a missing arm fails.
    let good_enum = r#"
        Forward,
        Suppress,
        FilterCarrierEntries(Value),
        AnswerServer(Value),
        AnswerEditor(Value),
    "#;
    assert!(
        decision_vocabulary_failures(good_enum).is_empty(),
        "the five-arm vocabulary must pass"
    );
    let two_arm_enum = r#"
        Forward,
        Suppress,
    "#;
    assert!(
        decision_vocabulary_failures(two_arm_enum)
            .iter()
            .any(|f| f.contains("FilterCarrierEntries")),
        "a vocabulary without the filter arm must fail"
    );
    let three_arm_enum = r#"
        Forward,
        Suppress,
        FilterCarrierEntries(Value),
    "#;
    assert!(
        decision_vocabulary_failures(three_arm_enum)
            .iter()
            .any(|f| f.contains("AnswerServer")),
        "a vocabulary without the answer-server arm must fail — a \
         suppressed server→client request would drop and hang the server"
    );
    let four_arm_enum = r#"
        Forward,
        Suppress,
        FilterCarrierEntries(Value),
        AnswerServer(Value),
    "#;
    assert!(
        decision_vocabulary_failures(four_arm_enum)
            .iter()
            .any(|f| f.contains("AnswerEditor")),
        "a vocabulary without the answer-editor arm must fail — a \
         suppressed carrier-referencing response to a tracked editor request \
         would strand it"
    );
}
