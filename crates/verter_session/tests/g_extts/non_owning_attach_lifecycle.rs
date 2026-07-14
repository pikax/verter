//! Guard: `non_owning_attach_lifecycle`.
//!
//! The attach substrate (`crates/verter_tsgo_api/src/attach.rs`) distinguishes
//! an engine Verter OWNS (spawned) from an editor-owned engine Verter is merely
//! ATTACHED to. Six non-owning invariants are load-bearing:
//!
//!   1. The NON-OWNING teardown (`TsgoAttach::detach`) retracts Verter's own
//!      overlays through the typed `did_close` op (which sends
//!      `textDocument/didClose`) and drops the `--api` pipe
//!      (`self.api.close`) — and NEVER sends `exit`, NEVER `start_kill`s, and
//!      NEVER `wait`s a child. Verter must not terminate an engine it did not
//!      spawn.
//!   2. The non-owning composer (`TsgoAttach::attach_to_initialized`) attaches
//!      to an ALREADY-initialized editor connection: it must not run the OWNED
//!      handshake (`lsp_handshake`) and must not originate an LSP `initialize`
//!      request — the editor already initialized that connection; a second
//!      `initialize` is a protocol violation.
//!   3. A Verter-originated LSP `initialize` REQUEST exists ONLY inside the
//!      OWNED handshake-half (`TsgoAttach::lsp_handshake`) — the sole place
//!      Verter initializes a connection it spawned.
//!   4. The `exit`-sending teardown (`shutdown`) is NOT public: the invariant
//!      is STRUCTURAL, not caller discipline. The sole public teardown entry
//!      is the ownership-dispatched `teardown()`, so no public API can send
//!      `exit` on a non-owning attach.
//!   5. The `"exit"` method literal appears ONLY inside the `shutdown` fn
//!      body — no other fn in `attach.rs` sends `exit`.
//!   6. `attach_to_initialized` REQUIRES a
//!      `ConnectionOwnership::AttachedNonOwning` connection at entry (before
//!      the gate + session open) — an `Owned` connection must not enter the
//!      non-owning composer.
//!
//! This STATIC guard reads `attach.rs`, brace-depth-extracts the relevant fn
//! bodies (mirroring `tsgo_capability_gate_on_version`), and asserts the six
//! invariants. The inline self-test proves the predicates DISCRIMINATE: a
//! sample `detach` body WITH `exit`/`start_kill` records failures and one
//! without passes; a sample `attach_to_initialized` that calls `lsp_handshake`
//! or sends `initialize` or lacks the ownership refusal records failures; a
//! `pub async fn shutdown` sample fails the visibility predicate; an `"exit"`
//! site outside `shutdown` fails the span predicate.

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

fn attach_rs() -> PathBuf {
    workspace_root().join("crates/verter_tsgo_api/src/attach.rs")
}

fn read_attach() -> String {
    let path = attach_rs();
    fs::read_to_string(&path).unwrap_or_else(|e| panic!("read {}: {e}", path.display()))
}

/// Byte span `(start, end)` of the body of the fn whose signature starts with
/// `sig`, extracted by a brace-depth scan: find `sig`, advance to its opening
/// `{`, take to the matching `}`. `None` if the signature is absent or the
/// braces are unbalanced.
fn fn_body_span(src: &str, sig: &str) -> Option<(usize, usize)> {
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

/// The body text of the fn whose signature starts with `sig`.
fn extract_fn_body(src: &str, sig: &str) -> Option<String> {
    let (start, end) = fn_body_span(src, sig)?;
    Some(src[start..end].to_string())
}

/// Engine-terminating markers a NON-OWNING teardown must never contain: the
/// `exit` notification method literal, a child `start_kill`, a child `wait`.
const OWNED_TEARDOWN_MARKERS: &[&str] = &["\"exit\"", "start_kill", ".wait("];

/// Predicate 1: the `detach` body retracts overlays + drops the pipe, and
/// contains NO engine-terminating marker. Returns failures (empty ⇒ pass).
fn detach_failures(body: &str) -> Vec<String> {
    let mut failures = Vec::new();
    // detach retracts Verter's overlays through the typed `did_close` lifecycle
    // op (which sends `textDocument/didClose` AND threads the overlay tracker,
    // so a non-owning teardown closes exactly the overlays Verter opened).
    if !body.contains("did_close") {
        failures.push(
            "detach must retract overlays via the typed `did_close` \
             (`textDocument/didClose`)"
                .to_string(),
        );
    }
    if !body.contains("self.api.close") {
        failures.push("detach must drop the `--api` pipe via `self.api.close`".to_string());
    }
    for marker in OWNED_TEARDOWN_MARKERS {
        if body.contains(marker) {
            failures.push(format!(
                "NON-OWNING detach must never contain `{marker}` — Verter must \
                 not terminate an engine it did not spawn"
            ));
        }
    }
    failures
}

/// The pattern of a Verter-originated LSP `initialize` REQUEST (the closing
/// quote excludes the `initialized` notification).
const INITIALIZE_REQUEST_PATTERN: &str = "request(\"initialize\"";

/// Predicate 2: the `attach_to_initialized` body neither runs the OWNED
/// handshake nor originates an `initialize` request, and it REQUIRES a
/// `ConnectionOwnership::AttachedNonOwning` connection (the ownership refusal
/// symmetric with `attach_over`'s Owned check).
fn attach_to_initialized_failures(body: &str) -> Vec<String> {
    let mut failures = Vec::new();
    if body.contains("lsp_handshake") {
        failures.push(
            "attach_to_initialized must NOT call `lsp_handshake` — the editor \
             already initialized this connection"
                .to_string(),
        );
    }
    if body.contains(INITIALIZE_REQUEST_PATTERN) {
        failures.push(
            "attach_to_initialized must NOT originate an LSP `initialize` \
             request on an editor-owned connection"
                .to_string(),
        );
    }
    if !body.contains("ConnectionOwnership::AttachedNonOwning") {
        failures.push(
            "attach_to_initialized must REQUIRE a \
             `ConnectionOwnership::AttachedNonOwning` connection at entry \
             (refuse an Owned connection before the gate + session open)"
                .to_string(),
        );
    }
    failures
}

/// Predicate 4: the `exit`-sending teardown (`shutdown`) exists and is NOT
/// public — the sole public teardown entry stays the ownership-dispatched
/// `teardown()`, making the non-owning invariant structural.
fn shutdown_visibility_failures(src: &str) -> Vec<String> {
    let mut failures = Vec::new();
    if !src.contains("async fn shutdown") {
        failures.push(
            "the owned teardown `async fn shutdown` must exist in attach.rs \
             (teardown()'s Owned arm dispatches to it)"
                .to_string(),
        );
    }
    // Reject EVERY visibility modifier on `shutdown` — a `pub(crate)` /
    // `pub(super)` shutdown is still reachable from a same-crate non-owning
    // caller, so crate-internal exposure is NOT private for this invariant.
    for vis in [
        "pub async fn shutdown",
        "pub(crate) async fn shutdown",
        "pub(super) async fn shutdown",
    ] {
        if src.contains(vis) {
            failures.push(format!(
                "`shutdown` (the `exit`-sending teardown) must NOT be exposed (`{vis}`) — any \
                 visibility lets a caller bypass the ownership-dispatched `teardown()` and send \
                 `exit` on a non-owning attach; keep it fully private"
            ));
        }
    }
    failures
}

/// The `exit` LSP method literal as it appears at a send site (the
/// double-quoted form — doc-comment backtick prose does not match).
const EXIT_METHOD_LITERAL: &str = "\"exit\"";

/// Predicate 5: every `"exit"` method-literal site in `src` lies inside `span`
/// (the `shutdown` body), and at least one exists there (shutdown genuinely
/// terminates the owned engine — non-vacuous).
fn exit_only_in_shutdown_failures(src: &str, span: (usize, usize)) -> Vec<String> {
    let mut failures = Vec::new();
    let sites = pattern_sites(src, EXIT_METHOD_LITERAL);
    let inside = sites
        .iter()
        .filter(|&&at| at >= span.0 && at < span.1)
        .count();
    let outside: Vec<usize> = sites
        .iter()
        .copied()
        .filter(|&at| at < span.0 || at >= span.1)
        .collect();
    if inside == 0 {
        failures.push(
            "`shutdown` must send the `exit` notification (the OWNED teardown \
             genuinely terminates the engine — non-vacuous)"
                .to_string(),
        );
    }
    if !outside.is_empty() {
        failures.push(format!(
            "an `\"exit\"` method literal exists OUTSIDE the `shutdown` body \
             (byte offsets {outside:?}) — `shutdown` is the ONLY fn allowed \
             to send `exit`"
        ));
    }
    failures
}

/// Every byte offset of `pattern` in `src`.
fn pattern_sites(src: &str, pattern: &str) -> Vec<usize> {
    let mut sites = Vec::new();
    let mut from = 0usize;
    while let Some(rel) = src[from..].find(pattern) {
        let at = from + rel;
        sites.push(at);
        from = at + pattern.len();
    }
    sites
}

/// Predicate 3: every `initialize` REQUEST site in `src` lies inside `span`
/// (the `lsp_handshake` body), and at least one exists there (the handshake
/// really originates it — non-vacuous).
fn initialize_only_in_handshake_failures(src: &str, span: (usize, usize)) -> Vec<String> {
    let mut failures = Vec::new();
    let sites = pattern_sites(src, INITIALIZE_REQUEST_PATTERN);
    let inside = sites
        .iter()
        .filter(|&&at| at >= span.0 && at < span.1)
        .count();
    let outside: Vec<usize> = sites
        .iter()
        .copied()
        .filter(|&at| at < span.0 || at >= span.1)
        .collect();
    if inside == 0 {
        failures.push(
            "lsp_handshake must originate the LSP `initialize` request (the \
             OWNED handshake-half is its sole home)"
                .to_string(),
        );
    }
    if !outside.is_empty() {
        failures.push(format!(
            "a Verter-originated `initialize` request exists OUTSIDE \
             lsp_handshake (byte offsets {outside:?}) — the OWNED \
             handshake-half is the ONLY place Verter initializes a connection"
        ));
    }
    failures
}

#[test]
fn non_owning_attach_lifecycle() {
    let src = read_attach();

    let detach_body = extract_fn_body(&src, "pub async fn detach").unwrap_or_else(|| {
        panic!(
            "could not extract the `pub async fn detach` body from {} — the \
             non-owning teardown must exist and be brace-balanced",
            attach_rs().display()
        )
    });
    let ati_body =
        extract_fn_body(&src, "pub async fn attach_to_initialized").unwrap_or_else(|| {
            panic!(
                "could not extract the `pub async fn attach_to_initialized` body \
                 from {} — the non-owning composer must exist",
                attach_rs().display()
            )
        });
    let handshake_span = fn_body_span(&src, "pub async fn lsp_handshake").unwrap_or_else(|| {
        panic!(
            "could not extract the `pub async fn lsp_handshake` body span from \
             {} — the OWNED handshake-half must exist",
            attach_rs().display()
        )
    });
    // `async fn shutdown` also matches inside a (violating) `pub async fn
    // shutdown`, so the span extraction works either way; the visibility
    // predicate below rejects the `pub` form.
    let shutdown_span = fn_body_span(&src, "async fn shutdown").unwrap_or_else(|| {
        panic!(
            "could not extract the `async fn shutdown` body span from {} — \
             the owned teardown must exist and be brace-balanced",
            attach_rs().display()
        )
    });

    let mut failures = detach_failures(&detach_body);
    failures.extend(attach_to_initialized_failures(&ati_body));
    failures.extend(initialize_only_in_handshake_failures(&src, handshake_span));
    failures.extend(shutdown_visibility_failures(&src));
    failures.extend(exit_only_in_shutdown_failures(&src, shutdown_span));

    assert!(
        failures.is_empty(),
        "the non-owning attach lifecycle invariants \
         (crates/verter_tsgo_api/src/attach.rs) are violated:\n{}",
        failures.join("\n")
    );
}

/// DISCRIMINATING self-test: each predicate FIRES on a violating sample and
/// PASSES on a conforming one — the guard is non-vacuous, not merely satisfied
/// by the live source.
#[test]
fn non_owning_attach_lifecycle_self_test_discriminates() {
    // A conforming detach body: didClose + api pipe drop, no termination.
    let good_detach = r#"
        let uris: Vec<String> = { self.open_overlays.lock().unwrap().iter().cloned().collect() };
        let channel = self.injection_channel();
        for uri in uris {
            let _ = channel.did_close(&uri).await;
        }
        let _ = self.api.close().await;
        Ok(())
    "#;
    assert!(
        detach_failures(good_detach).is_empty(),
        "a detach that only retracts overlays + drops the pipe must pass"
    );

    // A violating detach body: same retraction, but it ALSO terminates the
    // engine (exit + start_kill + wait) — every termination marker must fire.
    let bad_detach = r#"
        let channel = self.injection_channel();
        let _ = channel.did_close(&uri).await;
        let _ = self.api.close().await;
        let _ = self.lsp.conn.notify("exit", serde_json::Value::Null).await;
        if let Some(mut child) = self.lsp.child.take() {
            let _ = child.start_kill();
            let _ = child.wait().await;
        }
        Ok(())
    "#;
    let bad = detach_failures(bad_detach);
    assert!(
        bad.iter().any(|f| f.contains("\"exit\"")),
        "a detach that sends `exit` must fail the predicate; got {bad:?}"
    );
    assert!(
        bad.iter().any(|f| f.contains("start_kill")),
        "a detach that kills the child must fail the predicate; got {bad:?}"
    );
    assert!(
        bad.iter().any(|f| f.contains(".wait(")),
        "a detach that waits the child must fail the predicate; got {bad:?}"
    );

    // A detach missing the retraction/pipe-drop must fail on PRESENCE too.
    let empty_detach = "Ok(())";
    let missing = detach_failures(empty_detach);
    assert!(
        missing.iter().any(|f| f.contains("textDocument/didClose"))
            && missing.iter().any(|f| f.contains("self.api.close")),
        "a detach that retracts nothing must fail; got {missing:?}"
    );

    // A conforming attach_to_initialized: ownership refusal + gate + session
    // open, no handshake.
    let good_ati = r#"
        if lsp.ownership() != ConnectionOwnership::AttachedNonOwning {
            return Err(TsgoApiError::Transport("attach_to_initialized requires a non-owning connection".into()));
        }
        let clearance = gate::validate(&ObservedEngine::from_in_band_server_info(observed_version.into()))?;
        let (session, api) = Self::attach_api_session(&lsp.conn).await?;
        Ok(Self::from_parts(lsp, api, session, clearance))
    "#;
    assert!(
        attach_to_initialized_failures(good_ati).is_empty(),
        "a non-owning composer that refuses Owned, gates, and opens the \
         session must pass"
    );

    // A violating attach_to_initialized MISSING the ownership refusal: an
    // Owned connection would enter the non-owning composer unchecked.
    let bad_ati_no_ownership = r#"
        let clearance = gate::validate(&ObservedEngine::from_in_band_server_info(observed_version.into()))?;
        let (session, api) = Self::attach_api_session(&lsp.conn).await?;
        Ok(Self::from_parts(lsp, api, session, clearance))
    "#;
    assert!(
        attach_to_initialized_failures(bad_ati_no_ownership)
            .iter()
            .any(|f| f.contains("AttachedNonOwning")),
        "an attach_to_initialized missing the ownership refusal must fail"
    );

    // A violating attach_to_initialized that runs the OWNED handshake.
    let bad_ati_handshake = r#"
        let clearance = Self::lsp_handshake(&lsp.conn, root_uri).await?;
        let (session, api) = Self::attach_api_session(&lsp.conn).await?;
    "#;
    assert!(
        attach_to_initialized_failures(bad_ati_handshake)
            .iter()
            .any(|f| f.contains("lsp_handshake")),
        "an attach_to_initialized that calls lsp_handshake must fail"
    );

    // A violating attach_to_initialized that originates `initialize` directly.
    let bad_ati_initialize = r#"
        let init = lsp.conn.request("initialize", init_params).await?;
        let (session, api) = Self::attach_api_session(&lsp.conn).await?;
    "#;
    assert!(
        attach_to_initialized_failures(bad_ati_initialize)
            .iter()
            .any(|f| f.contains("initialize")),
        "an attach_to_initialized that sends `initialize` must fail"
    );

    // The `initialize`-only-in-handshake predicate: a source whose only
    // `initialize` request is inside the handshake span passes; one with a
    // second site outside fails; one whose handshake never initializes fails.
    let src_ok = r#"
        fn other() { conn.notify("initialized", x); }
        fn handshake() { conn.request("initialize", params) }
    "#;
    let span_ok = fn_body_span(src_ok, "fn handshake").expect("span");
    assert!(
        initialize_only_in_handshake_failures(src_ok, span_ok).is_empty(),
        "a source whose sole `initialize` request is in the handshake passes"
    );

    let src_leak = r#"
        fn rogue() { conn.request("initialize", params) }
        fn handshake() { conn.request("initialize", params) }
    "#;
    let span_leak = fn_body_span(src_leak, "fn handshake").expect("span");
    assert!(
        initialize_only_in_handshake_failures(src_leak, span_leak)
            .iter()
            .any(|f| f.contains("OUTSIDE")),
        "an `initialize` request outside the handshake must fail"
    );

    let src_vacuous = r#"
        fn handshake() { conn.notify("initialized", x) }
    "#;
    let span_vacuous = fn_body_span(src_vacuous, "fn handshake").expect("span");
    assert!(
        !initialize_only_in_handshake_failures(src_vacuous, span_vacuous).is_empty(),
        "a handshake that never originates `initialize` must fail (non-vacuous)"
    );

    // The shutdown-visibility predicate: a PUBLIC `exit`-sending shutdown is
    // the structural hole (a caller can bypass `teardown()`); a private one
    // passes; a source with no shutdown at all fails (non-vacuous).
    let src_pub_shutdown = r#"
        pub async fn shutdown(mut self) -> TsgoApiResult<()> { Ok(()) }
    "#;
    assert!(
        shutdown_visibility_failures(src_pub_shutdown)
            .iter()
            .any(|f| f.contains("NOT be exposed")),
        "a `pub async fn shutdown` must fail the visibility predicate"
    );
    // `pub(crate)` is still same-crate-reachable → must ALSO fail (a crate-local
    // non-owning caller could invoke it and send `exit`).
    let src_pub_crate_shutdown = r#"
        pub(crate) async fn shutdown(mut self) -> TsgoApiResult<()> { Ok(()) }
    "#;
    assert!(
        shutdown_visibility_failures(src_pub_crate_shutdown)
            .iter()
            .any(|f| f.contains("NOT be exposed")),
        "a `pub(crate) async fn shutdown` must ALSO fail the visibility predicate"
    );
    let src_private_shutdown = r#"
        async fn shutdown(mut self) -> TsgoApiResult<()> { Ok(()) }
    "#;
    assert!(
        shutdown_visibility_failures(src_private_shutdown).is_empty(),
        "a private `async fn shutdown` must pass the visibility predicate"
    );
    let src_no_shutdown = r#"
        pub async fn teardown(self) -> TsgoApiResult<()> { Ok(()) }
    "#;
    assert!(
        !shutdown_visibility_failures(src_no_shutdown).is_empty(),
        "a source with no `async fn shutdown` must fail (non-vacuous)"
    );

    // The `"exit"`-only-in-shutdown predicate: a source whose only quoted
    // `"exit"` send site is inside the shutdown span passes; a second site
    // outside fails; a shutdown that never sends `exit` fails (non-vacuous).
    let src_exit_ok = r#"
        pub async fn detach(self) { conn.notify("textDocument/didClose", x); }
        async fn shutdown(mut self) { conn.notify("exit", y) }
    "#;
    let span_exit_ok = fn_body_span(src_exit_ok, "async fn shutdown").expect("span");
    assert!(
        exit_only_in_shutdown_failures(src_exit_ok, span_exit_ok).is_empty(),
        "a source whose sole `\"exit\"` site is inside shutdown must pass"
    );
    let src_exit_leak = r#"
        pub async fn detach(self) { conn.notify("exit", x); }
        async fn shutdown(mut self) { conn.notify("exit", y) }
    "#;
    let span_exit_leak = fn_body_span(src_exit_leak, "async fn shutdown").expect("span");
    assert!(
        exit_only_in_shutdown_failures(src_exit_leak, span_exit_leak)
            .iter()
            .any(|f| f.contains("OUTSIDE")),
        "an `\"exit\"` send site outside shutdown must fail"
    );
    let src_exit_vacuous = r#"
        async fn shutdown(mut self) { conn.close().await }
    "#;
    let span_exit_vacuous = fn_body_span(src_exit_vacuous, "async fn shutdown").expect("span");
    assert!(
        !exit_only_in_shutdown_failures(src_exit_vacuous, span_exit_vacuous).is_empty(),
        "a shutdown that never sends `exit` must fail (non-vacuous)"
    );

    // The brace-depth extractor stops at the matching brace (no bleed into a
    // sibling fn).
    let sample = r#"
impl TsgoAttach {
    pub async fn detach(self) -> TsgoApiResult<()> {
        let _ = self.api.close().await;
        Ok(())
    }

    pub fn sibling(&self) {
        start_kill_should_not_be_seen()
    }
}
"#;
    let body = extract_fn_body(sample, "pub async fn detach").expect("extracts detach body");
    assert!(body.contains("self.api.close"));
    assert!(
        !body.contains("start_kill_should_not_be_seen"),
        "the extractor must STOP at detach's matching brace"
    );
}
