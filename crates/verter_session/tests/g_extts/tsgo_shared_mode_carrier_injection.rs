//! Guard: `tsgo_shared_mode_carrier_injection`.
//!
//! The injectable-proxy substrate shape
//! (`crates/verter_tsgo_api/src/relay.rs`): Verter injects carrier frames
//! onto an editor-owned `tsgo --lsp` server stream through the relay, under
//! the reserved `verter:*` request-id namespace, with responses demuxing back
//! to Verter and editor traffic passing through untouched. Four structural
//! facts:
//!
//!   1. Injected-request ids are minted in the reserved namespace: the
//!      namespace constant is exactly `"verter:"` and the mint path formats
//!      ids from it off the `next_inject_id` counter.
//!   2. The server→editor pump demuxes reserved-namespace responses to the
//!      `verter_pending` waiter table (`.remove(` the waiter — never the
//!      editor) and forwards every other frame to the editor (`write_all` —
//!      pass-through transparency). The demux branch TERMINATES the frame's
//!      handling: a `continue` sits between the `.remove(` demux site and
//!      the editor `write_all`, so a demuxed `verter:*` response can never
//!      ALSO forward to the editor.
//!   3. The editor→server pump validates the reservation: an editor frame
//!      carrying a reserved id is dropped and recorded
//!      (`reservation_violations`), never forwarded (`server_tx.send` is the
//!      forward path it must guard).
//!   4. Injection routes through the deny-by-default gate: the relay's write
//!      surface is `injection_channel` (constructing the
//!      `CarrierInjectionChannel`), whose `gated_notify`/`gated_request` run
//!      `carrier_write_allowed` before forwarding.
//!
//! SCOPE (the narrower truth): this STATIC guard pins the substrate SHAPE.
//! The end-to-end BEHAVIORAL proofs (deny-by-default refusal, allowlist
//! passthrough, transparency, injection, `verter:*` demux, didOpen-before-
//! barrier ordering) live in `crates/verter_tsgo_api/src/relay_tests.rs`. It
//! does NOT assert live editor Program membership or type-flow through a real
//! editor engine.
//!
//! The inline self-test proves the predicates DISCRIMINATE: a mint without
//! the namespace, a pump without the demux or the pass-through, an editor
//! pump that never records the violation, and an ungated injection write each
//! fail; conforming samples pass.

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

fn read_relay() -> String {
    let path = workspace_root().join("crates/verter_tsgo_api/src/relay.rs");
    fs::read_to_string(&path).unwrap_or_else(|e| panic!("read {}: {e}", path.display()))
}

/// The body of the block whose header starts with `sig`, extracted by a
/// brace-depth scan: find `sig`, advance to its opening `{`, take to the
/// matching `}`. `None` if absent or unbalanced.
fn block_body(src: &str, sig: &str) -> Option<String> {
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
                    return Some(after_sig[open_rel + 1..i].to_string());
                }
            }
            _ => {}
        }
        i += 1;
    }
    None
}

/// Fact 1: reserved-namespace minting. Returns failures (empty ⇒ pass).
fn namespace_minting_failures(src: &str, mint_body: &str) -> Vec<String> {
    let mut failures = Vec::new();
    if !src.contains("VERTER_ID_NAMESPACE: &str = \"verter:\"") {
        failures.push(
            "relay.rs must define the reserved id namespace constant as \
             exactly `\"verter:\"`"
                .to_string(),
        );
    }
    if !mint_body.contains("VERTER_ID_NAMESPACE") {
        failures.push(
            "the injected-id mint must format ids from the reserved \
             `VERTER_ID_NAMESPACE` (no ad-hoc prefix)"
                .to_string(),
        );
    }
    if !mint_body.contains("next_inject_id") {
        failures.push(
            "the injected-id mint must draw from the `next_inject_id` counter \
             (unique ids per injected request)"
                .to_string(),
        );
    }
    failures
}

/// Fact 2: the server→editor pump demuxes reserved responses to the waiter
/// table and forwards everything else to the editor.
fn server_pump_failures(body: &str) -> Vec<String> {
    let mut failures = Vec::new();
    if !body.contains("frame_carries_verter_id") && !body.contains("VERTER_ID_NAMESPACE") {
        failures.push(
            "the server→editor pump must test frames against the reserved \
             `verter:*` namespace to demux Verter responses"
                .to_string(),
        );
    }
    if !body.contains(".remove(") {
        failures.push(
            "the server→editor pump must route a reserved response to its \
             pending waiter (`.remove(` from the waiter table), not the editor"
                .to_string(),
        );
    }
    if !body.contains("write_all") {
        failures.push(
            "the server→editor pump must forward non-reserved frames to the \
             editor (`write_all` — pass-through transparency)"
                .to_string(),
        );
    }
    // The demux branch must TERMINATE the frame's handling: after routing a
    // reserved response to its waiter (`.remove(`), the branch `continue`s
    // BEFORE the editor forward (`write_all`) — a demux-then-STILL-forward
    // body would leak the `verter:*` response to the editor.
    if let Some(remove_at) = body.find(".remove(") {
        let after_demux = &body[remove_at..];
        let continue_at = after_demux.find("continue");
        let write_at = after_demux.find("write_all");
        let demux_terminates = match (continue_at, write_at) {
            (Some(c), Some(w)) => c < w,
            (Some(_), None) => true,
            (None, _) => false,
        };
        if !demux_terminates {
            failures.push(
                "the server→editor pump's demux branch must `continue` before \
                 the editor `write_all` — a demuxed `verter:*` response must \
                 never ALSO forward to the editor"
                    .to_string(),
            );
        }
    }
    failures
}

/// Fact 3: the editor→server pump drops + records reserved-id editor frames.
fn editor_pump_failures(body: &str) -> Vec<String> {
    let mut failures = Vec::new();
    if !body.contains("frame_carries_verter_id") && !body.contains("VERTER_ID_NAMESPACE") {
        failures.push(
            "the editor→server pump must validate the reserved `verter:*` \
             namespace on editor frames"
                .to_string(),
        );
    }
    if !body.contains("reservation_violations") || !body.contains("fetch_add") {
        failures.push(
            "the editor→server pump must RECORD a dropped reserved-id editor \
             frame (`reservation_violations` counter)"
                .to_string(),
        );
    }
    if !body.contains("server_tx.send") {
        failures.push(
            "the editor→server pump must forward legitimate frames through \
             the serialized server writer (`server_tx.send`)"
                .to_string(),
        );
    }
    failures
}

/// Fact 4: injection routes through the deny-by-default gate — the write
/// body runs `carrier_write_allowed` BEFORE the sink forward.
fn gated_injection_failures(body: &str, fn_name: &str, forward_marker: &str) -> Vec<String> {
    let mut failures = Vec::new();
    let Some(gate_at) = body.find("carrier_write_allowed") else {
        failures.push(format!(
            "the injection write `{fn_name}` must run the \
             `carrier_write_allowed` deny-by-default gate"
        ));
        return failures;
    };
    match body.find(forward_marker) {
        None => failures.push(format!(
            "the injection write `{fn_name}` must forward admitted writes via \
             `{forward_marker}`"
        )),
        Some(forward_at) if forward_at < gate_at => failures.push(format!(
            "the injection write `{fn_name}` forwards (`{forward_marker}` at \
             byte {forward_at}) BEFORE the gate (`carrier_write_allowed` at \
             byte {gate_at})"
        )),
        Some(_) => {}
    }
    failures
}

#[test]
fn tsgo_shared_mode_carrier_injection() {
    let src = read_relay();

    let mint_body = block_body(&src, "fn mint_injected_id")
        .expect("relay.rs must carry the injected-id mint (`fn mint_injected_id`)");
    let server_pump_body = block_body(&src, "async fn server_to_editor_pump")
        .expect("relay.rs must carry the server→editor pump");
    let editor_pump_body = block_body(&src, "async fn editor_to_server_pump")
        .expect("relay.rs must carry the editor→server pump");
    let notify_body = block_body(&src, "async fn gated_notify")
        .expect("relay.rs must carry the channel's gated `gated_notify`");
    let request_body = block_body(&src, "async fn gated_request")
        .expect("relay.rs must carry the channel's gated `gated_request`");
    let injection_channel_body = block_body(&src, "pub fn injection_channel")
        .expect("relay.rs must expose the relay's `injection_channel`");

    let mut failures = namespace_minting_failures(&src, &mint_body);
    failures.extend(server_pump_failures(&server_pump_body));
    failures.extend(editor_pump_failures(&editor_pump_body));
    failures.extend(gated_injection_failures(
        &notify_body,
        "gated_notify",
        "send_notify",
    ));
    failures.extend(gated_injection_failures(
        &request_body,
        "gated_request",
        "send_request",
    ));
    if !injection_channel_body.contains("CarrierInjectionChannel::new") {
        failures.push(
            "the relay's `injection_channel` must construct the gated \
             `CarrierInjectionChannel` (the single write gate) over its \
             private inject port"
                .to_string(),
        );
    }

    assert!(
        failures.is_empty(),
        "the injectable-proxy substrate shape \
         (crates/verter_tsgo_api/src/relay.rs) is violated:\n{}",
        failures.join("\n")
    );
}

/// DISCRIMINATING self-test: each predicate FIRES on a violating sample and
/// PASSES on a conforming one — the guard is non-vacuous.
#[test]
fn tsgo_shared_mode_carrier_injection_self_test_discriminates() {
    // Namespace minting: conforming source + mint pass.
    let good_src = r#"pub(crate) const VERTER_ID_NAMESPACE: &str = "verter:";"#;
    let good_mint = r#"
        let n = self.next_inject_id.fetch_add(1, Ordering::Relaxed);
        format!("{VERTER_ID_NAMESPACE}{n}")
    "#;
    assert!(
        namespace_minting_failures(good_src, good_mint).is_empty(),
        "a namespaced counter-backed mint must pass"
    );
    // A mint with an ad-hoc prefix (no namespace constant) fails.
    let bad_mint = r#"format!("req-{}", self.counter)"#;
    let bad = namespace_minting_failures(good_src, bad_mint);
    assert!(
        bad.iter().any(|f| f.contains("VERTER_ID_NAMESPACE"))
            && bad.iter().any(|f| f.contains("next_inject_id")),
        "a mint bypassing the namespace + counter must fail; got {bad:?}"
    );
    // A source without the exact reserved constant fails.
    assert!(
        namespace_minting_failures("const OTHER: &str = \"x:\";", good_mint)
            .iter()
            .any(|f| f.contains("\"verter:\"")),
        "a relay without the reserved namespace constant must fail"
    );

    // Server pump: conforming demux + pass-through passes.
    let good_server_pump = r#"
        if is_response && frame_carries_verter_id(&msg) {
            if let Some(tx) = verter_pending.lock().remove(&id) { let _ = tx.send(msg); }
            continue;
        }
        if editor_write.write_all(&encode_message(&msg)).await.is_err() { break; }
    "#;
    assert!(
        server_pump_failures(good_server_pump).is_empty(),
        "a demuxing, forwarding server pump must pass"
    );
    // A pump missing the demux fails; one missing the pass-through fails.
    let no_demux = r#"
        if editor_write.write_all(&encode_message(&msg)).await.is_err() { break; }
    "#;
    assert!(
        server_pump_failures(no_demux)
            .iter()
            .any(|f| f.contains(".remove(")),
        "a server pump without the waiter demux must fail"
    );
    let no_forward = r#"
        if frame_carries_verter_id(&msg) {
            if let Some(tx) = verter_pending.lock().remove(&id) { let _ = tx.send(msg); }
        }
    "#;
    assert!(
        server_pump_failures(no_forward)
            .iter()
            .any(|f| f.contains("pass-through transparency")),
        "a server pump without the editor pass-through must fail"
    );
    // A pump that demuxes but STILL forwards the frame (no `continue` between
    // the `.remove(` demux site and the editor `write_all`) leaks the
    // `verter:*` response to the editor — it must fail.
    let demux_then_forward = r#"
        if is_response && frame_carries_verter_id(&msg) {
            if let Some(tx) = verter_pending.lock().remove(&id) { let _ = tx.send(msg.clone()); }
        }
        if editor_write.write_all(&raw).await.is_err() { break; }
    "#;
    assert!(
        server_pump_failures(demux_then_forward)
            .iter()
            .any(|f| f.contains("`continue` before")),
        "a server pump that demuxes but STILL forwards the frame must fail"
    );

    // Editor pump: conforming drop+record+forward passes.
    let good_editor_pump = r#"
        if frame_carries_verter_id(&msg) {
            reservation_violations.fetch_add(1, Ordering::Relaxed);
            continue;
        }
        if server_tx.send(encode_message(&msg)).await.is_err() { return; }
    "#;
    assert!(
        editor_pump_failures(good_editor_pump).is_empty(),
        "a validating, recording, forwarding editor pump must pass"
    );
    // A pump that forwards everything without validating/recording fails.
    let blind_pump = r#"
        if server_tx.send(encode_message(&msg)).await.is_err() { return; }
    "#;
    let blind = editor_pump_failures(blind_pump);
    assert!(
        blind.iter().any(|f| f.contains("verter:*"))
            && blind.iter().any(|f| f.contains("reservation_violations")),
        "an editor pump that never validates the reservation must fail; got {blind:?}"
    );

    // Gated injection: an ungated write fails; gate-after-forward fails;
    // gate-before-forward passes.
    let good_write = r#"
        if !carrier_write_allowed(method) {
            return Err(TsgoApiError::WriteGateDenied { method: method.to_string() });
        }
        self.sink.send_notify(method, params).await
    "#;
    assert!(
        gated_injection_failures(good_write, "notify", "send_notify").is_empty(),
        "a gate-before-forward write must pass"
    );
    assert!(
        gated_injection_failures("self.sink.send_notify(m, p).await", "notify", "send_notify")
            .iter()
            .any(|f| f.contains("carrier_write_allowed")),
        "an ungated injection write must fail"
    );
    let gate_after = r#"
        self.sink.send_notify(method, params).await?;
        if !carrier_write_allowed(method) { return Err(e); }
        Ok(())
    "#;
    assert!(
        gated_injection_failures(gate_after, "notify", "send_notify")
            .iter()
            .any(|f| f.contains("BEFORE the gate")),
        "a forward-before-gate injection write must fail"
    );
}
