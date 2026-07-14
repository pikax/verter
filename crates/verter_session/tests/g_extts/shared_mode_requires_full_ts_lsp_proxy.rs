//! Guard: `shared_mode_requires_full_ts_lsp_proxy`.
//!
//! The substrate no-bypass for the gated write proxy: on the shared /
//! non-owning path — an editor-owned `tsgo --lsp` engine Verter attaches to
//! or relays for — every public Verter write goes through the deny-by-default
//! `CarrierInjectionChannel` gate; no public non-owning surface hands out (or
//! leaks a clone of) the raw `JsonRpcConnection`. Five structural facts:
//!
//!   1. `impl TsgoAttach<NonOwning>` (the non-owning attach surface in
//!      `crates/verter_tsgo_api/src/attach.rs`) exposes NO raw-wire accessor:
//!      no `fn lsp(` and no mention of `&JsonRpcConnection` (so no method can
//!      return or leak it).
//!   2. EVERY `-> &JsonRpcConnection` accessor in `attach.rs` lies INSIDE the
//!      `impl TsgoAttach<Owned>` block — the sole legitimate raw-wire
//!      accessor is `TsgoAttach<Owned>::lsp`. An accessor on any other type
//!      (e.g. a `TsgoLspConnection::connection()`) is reachable on a
//!      `new_attached` (non-owning) connection and defeats deny-by-default.
//!   3. `CarrierInjectionChannel` (`crates/verter_tsgo_api/src/relay.rs`)
//!      never exposes its sink: no `pub` field in the struct body, no
//!      `pub fn connection`, and nothing in relay.rs returns
//!      `&JsonRpcConnection`.
//!   4. The channel's write path is deny-by-default: BOTH `gated_notify` and
//!      `gated_request` (the private senders) run the `carrier_write_allowed`
//!      allowlist gate BEFORE forwarding to the sink, refusing with the typed
//!      `WriteGateDenied`.
//!   5. The channel exposes NO public raw method-string write
//!      (`pub async fn notify`/`request`) — the only public writes are the
//!      typed carrier ops.
//!
//! SCOPE (the narrower truth): this guard enforces the STRUCTURAL no-bypass
//! of the gated proxy substrate — shared-path/non-owning writes cannot reach
//! the wire around the gate. It does NOT assert engine-mode selection (which
//! engine a live attach binds to is another layer's decision), and it does
//! NOT prove live editor behavior — the behavioral refusal/transparency
//! proofs live in `crates/verter_tsgo_api/src/relay_tests.rs`.
//!
//! The inline self-test proves the predicates DISCRIMINATE: a sample
//! non-owning impl WITH an `lsp()` accessor fails and one without passes; a
//! sample attach source with a `TsgoLspConnection::connection()` accessor
//! outside the owned block fails and one whose sole accessor is
//! `TsgoAttach<Owned>::lsp` passes; a channel struct with a `pub` sink field
//! fails; a `gated_notify` body that skips the gate (or gates only after
//! forwarding) fails; a public raw `notify` sender fails the no-raw-surface
//! fact.

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

/// The byte span `(start, end)` of the body of the block (fn / impl / struct)
/// whose header starts with `sig`, extracted by a brace-depth scan: find
/// `sig`, advance to its opening `{`, take to the matching `}`. `None` if
/// absent or unbalanced.
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

/// Fact 1: the non-owning attach impl block exposes no raw-wire accessor.
/// Returns failures (empty ⇒ pass).
fn non_owning_impl_failures(block: &str) -> Vec<String> {
    let mut failures = Vec::new();
    if !block.contains("pub async fn detach") {
        failures.push(
            "the `impl TsgoAttach<NonOwning>` block must carry the non-owning \
             surface (`pub async fn detach`) — the scan must hit the real block"
                .to_string(),
        );
    }
    if block.contains("fn lsp(") {
        failures.push(
            "`impl TsgoAttach<NonOwning>` must NOT expose a raw-wire accessor \
             (`fn lsp(`) — non-owning writes go exclusively through the gated \
             CarrierInjectionChannel"
                .to_string(),
        );
    }
    if block.contains("&JsonRpcConnection") {
        failures.push(
            "`impl TsgoAttach<NonOwning>` must NOT return or leak \
             `&JsonRpcConnection` — the raw connection is Clone, so any \
             borrow hands out an ungated write path"
                .to_string(),
        );
    }
    failures
}

/// Fact 2: EVERY `-> &JsonRpcConnection` accessor in attach.rs lies INSIDE
/// the `impl TsgoAttach<Owned>` block — the raw wire is owned-only. An
/// accessor on any other type (e.g. a `TsgoLspConnection::connection()`)
/// hands the raw connection to a `new_attached` (non-owning) caller around
/// the gate. Returns failures (empty ⇒ pass).
fn attach_raw_accessor_failures(attach_src: &str) -> Vec<String> {
    let mut failures = Vec::new();
    let Some((owned_start, owned_end)) = block_span(attach_src, "impl TsgoAttach<Owned>") else {
        failures.push(
            "attach.rs must carry the `impl TsgoAttach<Owned>` block (the \
             sole home of the raw-wire accessor)"
                .to_string(),
        );
        return failures;
    };
    let needle = "-> &JsonRpcConnection";
    let mut inside = 0usize;
    let mut from = 0usize;
    while let Some(rel) = attach_src[from..].find(needle) {
        let at = from + rel;
        if at >= owned_start && at < owned_end {
            inside += 1;
        } else {
            failures.push(format!(
                "attach.rs returns `&JsonRpcConnection` at byte {at}, OUTSIDE \
                 the `impl TsgoAttach<Owned>` block — the raw-wire accessor \
                 is owned-only; any other accessor (e.g. a \
                 `TsgoLspConnection::connection()`) is reachable on a \
                 non-owning connection and defeats deny-by-default"
            ));
        }
        from = at + needle.len();
    }
    if inside == 0 {
        failures.push(
            "the `impl TsgoAttach<Owned>` block must carry the sole raw-wire \
             accessor (`fn lsp(&self) -> &JsonRpcConnection`) — the scan must \
             hit the real block (non-vacuous)"
                .to_string(),
        );
    }
    failures
}

/// Fact 3a: the channel struct holds its sink privately (no `pub` field).
fn channel_struct_failures(struct_body: &str) -> Vec<String> {
    let mut failures = Vec::new();
    if !struct_body.contains("sink") {
        failures.push(
            "the `CarrierInjectionChannel` struct must hold a private sink \
             field — the scan must hit the real struct"
                .to_string(),
        );
    }
    if struct_body.contains("pub ") {
        failures.push(
            "the `CarrierInjectionChannel` struct must have NO `pub` field — \
             a public sink field hands the raw wire out around the gate"
                .to_string(),
        );
    }
    failures
}

/// Fact 3b: relay.rs never hands the raw connection out.
fn relay_no_raw_exposure_failures(relay_src: &str) -> Vec<String> {
    let mut failures = Vec::new();
    if relay_src.contains("pub fn connection") {
        failures.push(
            "relay.rs must NOT expose a `pub fn connection` accessor — the \
             sink stays private behind the gate"
                .to_string(),
        );
    }
    if relay_src.contains("-> &JsonRpcConnection") {
        failures.push(
            "relay.rs must NOT contain `-> &JsonRpcConnection` — no fn \
             returns the raw connection; it never leaves the gate"
                .to_string(),
        );
    }
    failures
}

/// Fact 4: a gated write body runs `carrier_write_allowed` BEFORE forwarding
/// to the sink (`forward_marker` is the sink call, e.g. `send_notify`).
fn gate_before_forward_failures(body: &str, fn_name: &str, forward_marker: &str) -> Vec<String> {
    let mut failures = Vec::new();
    let Some(gate_at) = body.find("carrier_write_allowed") else {
        failures.push(format!(
            "the channel's `{fn_name}` must run the `carrier_write_allowed` \
             deny-by-default gate"
        ));
        return failures;
    };
    if !body.contains("WriteGateDenied") {
        failures.push(format!(
            "the channel's `{fn_name}` must refuse with the typed \
             `WriteGateDenied` error"
        ));
    }
    match body.find(forward_marker) {
        None => failures.push(format!(
            "the channel's `{fn_name}` must forward admitted writes to the \
             sink via `{forward_marker}`"
        )),
        Some(forward_at) if forward_at < gate_at => failures.push(format!(
            "the channel's `{fn_name}` forwards to the sink \
             (`{forward_marker}` at byte {forward_at}) BEFORE the gate \
             (`carrier_write_allowed` at byte {gate_at}) — the gate must \
             refuse before the wire"
        )),
        Some(_) => {}
    }
    failures
}

/// Fact 5: the channel exposes NO public raw method-string write surface — the
/// only public writes are the typed carrier ops. A `pub async fn notify` /
/// `pub async fn request` on the channel is the escape hatch that admits
/// kind-mismatched ops and untracked overlays; it must not be public.
fn no_public_raw_write_surface_failures(relay_src: &str) -> Vec<String> {
    let mut failures = Vec::new();
    for sig in ["pub async fn notify", "pub async fn request"] {
        if relay_src.contains(sig) {
            failures.push(format!(
                "relay.rs must NOT expose `{sig}` on the carrier channel — the \
                 raw method-string write is private (`gated_notify`/`gated_request`); \
                 a public raw sender admits kind-mismatched ops and untracked overlays"
            ));
        }
    }
    failures
}

#[test]
fn shared_mode_requires_full_ts_lsp_proxy() {
    let attach_src = read_source("crates/verter_tsgo_api/src/attach.rs");
    let relay_src = read_source("crates/verter_tsgo_api/src/relay.rs");

    let non_owning_block = block_body(&attach_src, "impl TsgoAttach<NonOwning>")
        .expect("attach.rs must carry the `impl TsgoAttach<NonOwning>` block");
    let channel_struct = block_body(&relay_src, "pub struct CarrierInjectionChannel")
        .expect("relay.rs must carry the `CarrierInjectionChannel` struct");
    let notify_body = block_body(&relay_src, "async fn gated_notify")
        .expect("relay.rs must carry the channel's private `gated_notify`");
    let request_body = block_body(&relay_src, "async fn gated_request")
        .expect("relay.rs must carry the channel's private `gated_request`");

    let mut failures = non_owning_impl_failures(&non_owning_block);
    failures.extend(attach_raw_accessor_failures(&attach_src));
    failures.extend(channel_struct_failures(&channel_struct));
    failures.extend(relay_no_raw_exposure_failures(&relay_src));
    failures.extend(gate_before_forward_failures(
        &notify_body,
        "gated_notify",
        "send_notify",
    ));
    failures.extend(gate_before_forward_failures(
        &request_body,
        "gated_request",
        "send_request",
    ));
    failures.extend(no_public_raw_write_surface_failures(&relay_src));

    assert!(
        failures.is_empty(),
        "the gated write-proxy no-bypass (crates/verter_tsgo_api/src/attach.rs \
         + relay.rs) is violated:\n{}",
        failures.join("\n")
    );
}

/// DISCRIMINATING self-test: each predicate FIRES on a violating sample and
/// PASSES on a conforming one — the guard is non-vacuous.
#[test]
fn shared_mode_requires_full_ts_lsp_proxy_self_test_discriminates() {
    // A conforming non-owning impl: detach + teardown, no raw accessor.
    let good_non_owning = r#"
        pub async fn detach(self) -> TsgoApiResult<()> {
            let channel = self.injection_channel();
            let _ = channel.notify("textDocument/didClose", params).await;
            let _ = self.api.close().await;
            Ok(())
        }
        pub async fn teardown(self) -> TsgoApiResult<()> { self.detach().await }
    "#;
    assert!(
        non_owning_impl_failures(good_non_owning).is_empty(),
        "a non-owning impl writing only through the channel must pass"
    );

    // A violating non-owning impl WITH the raw accessor.
    let bad_non_owning_lsp = r#"
        pub async fn detach(self) -> TsgoApiResult<()> { Ok(()) }
        pub fn lsp(&self) -> &JsonRpcConnection { &self.lsp.conn }
    "#;
    let bad = non_owning_impl_failures(bad_non_owning_lsp);
    assert!(
        bad.iter().any(|f| f.contains("fn lsp(")),
        "a non-owning impl exposing `lsp()` must fail; got {bad:?}"
    );
    assert!(
        bad.iter().any(|f| f.contains("&JsonRpcConnection")),
        "a non-owning impl returning `&JsonRpcConnection` must fail; got {bad:?}"
    );

    // A block that is not the real non-owning surface fails on PRESENCE.
    assert!(
        !non_owning_impl_failures("pub fn other(&self) {}").is_empty(),
        "a block without the non-owning surface must fail (non-vacuous)"
    );

    // Raw accessors are owned-block-only: an attach source whose SOLE
    // `-> &JsonRpcConnection` accessor is `TsgoAttach<Owned>::lsp` passes.
    let good_attach = r#"
impl TsgoLspConnection {
    pub fn ownership(&self) -> ConnectionOwnership { self.ownership }
}
impl TsgoAttach<Owned> {
    pub fn lsp(&self) -> &JsonRpcConnection { &self.lsp.conn }
}
"#;
    assert!(
        attach_raw_accessor_failures(good_attach).is_empty(),
        "an attach source whose only raw accessor is `TsgoAttach<Owned>::lsp` \
         must pass"
    );
    // A `TsgoLspConnection::connection()` accessor OUTSIDE the owned block —
    // reachable on a `new_attached` connection — must fail.
    let bad_attach = r#"
impl TsgoLspConnection {
    pub fn connection(&self) -> &JsonRpcConnection { &self.conn }
}
impl TsgoAttach<Owned> {
    pub fn lsp(&self) -> &JsonRpcConnection { &self.lsp.conn }
}
"#;
    assert!(
        attach_raw_accessor_failures(bad_attach)
            .iter()
            .any(|f| f.contains("OUTSIDE")),
        "a `TsgoLspConnection::connection()` raw accessor outside the owned \
         block must fail"
    );
    // An owned block WITHOUT the raw accessor fails on presence (non-vacuous).
    let no_accessor_attach = r#"
impl TsgoAttach<Owned> {
    pub async fn teardown(self) -> TsgoApiResult<()> { self.shutdown().await }
}
"#;
    assert!(
        !attach_raw_accessor_failures(no_accessor_attach).is_empty(),
        "an owned block without the raw accessor must fail (non-vacuous)"
    );
    // A source with no owned block at all fails on PRESENCE.
    assert!(
        !attach_raw_accessor_failures("impl Other { }").is_empty(),
        "a source without the `impl TsgoAttach<Owned>` block must fail"
    );

    // A conforming channel struct: private fields only.
    let good_struct = r#"
        sink: &'a dyn GatedWireSink,
        open_overlays: &'a StdMutex<HashSet<String>>,
    "#;
    assert!(
        channel_struct_failures(good_struct).is_empty(),
        "a channel struct with private fields must pass"
    );
    // A violating channel struct: the sink field is public.
    let bad_struct = r#"
        pub sink: &'a dyn GatedWireSink,
    "#;
    assert!(
        channel_struct_failures(bad_struct)
            .iter()
            .any(|f| f.contains("NO `pub` field")),
        "a channel struct with a `pub` sink field must fail"
    );

    // Raw-exposure predicates fire on an exposing relay body.
    let bad_relay = r#"
        pub fn connection(&self) -> &JsonRpcConnection { &self.conn }
    "#;
    let bad_exposure = relay_no_raw_exposure_failures(bad_relay);
    assert!(
        bad_exposure.iter().any(|f| f.contains("pub fn connection"))
            && bad_exposure
                .iter()
                .any(|f| f.contains("-> &JsonRpcConnection")),
        "a relay exposing the raw connection must fail both predicates"
    );
    assert!(
        relay_no_raw_exposure_failures("fn private_pump() {}").is_empty(),
        "a relay without raw exposure must pass"
    );

    // The gate-before-forward predicate: conforming body passes.
    let good_notify = r#"
        if !carrier_write_allowed(method) {
            return Err(TsgoApiError::WriteGateDenied { method: method.to_string() });
        }
        self.sink.send_notify(method, params).await
    "#;
    assert!(
        gate_before_forward_failures(good_notify, "notify", "send_notify").is_empty(),
        "a notify that gates before forwarding must pass"
    );
    // An UNGATED notify fails.
    let ungated_notify = "self.sink.send_notify(method, params).await";
    assert!(
        gate_before_forward_failures(ungated_notify, "notify", "send_notify")
            .iter()
            .any(|f| f.contains("carrier_write_allowed")),
        "a notify without the allowlist gate must fail"
    );
    // A notify that forwards FIRST and gates after fails on ordering.
    let gate_after_forward = r#"
        self.sink.send_notify(method, params).await?;
        if !carrier_write_allowed(method) {
            return Err(TsgoApiError::WriteGateDenied { method: method.to_string() });
        }
        Ok(())
    "#;
    assert!(
        gate_before_forward_failures(gate_after_forward, "notify", "send_notify")
            .iter()
            .any(|f| f.contains("BEFORE the gate")),
        "a notify that forwards before gating must fail"
    );

    // The no-public-raw-write fact: a public raw sender fails; a private one passes.
    assert!(
        no_public_raw_write_surface_failures("pub async fn notify(&self, m: &str) {}")
            .iter()
            .any(|f| f.contains("pub async fn notify")),
        "a public raw `notify` sender must fail the no-raw-surface fact"
    );
    assert!(
        no_public_raw_write_surface_failures("pub async fn request(&self, m: &str) {}")
            .iter()
            .any(|f| f.contains("pub async fn request")),
        "a public raw `request` sender must fail the no-raw-surface fact"
    );
    assert!(
        no_public_raw_write_surface_failures("async fn gated_notify(&self) {}").is_empty(),
        "a private `gated_notify` sender must pass"
    );
}
