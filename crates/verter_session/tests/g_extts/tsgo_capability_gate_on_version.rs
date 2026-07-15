//! Guard: `tsgo_capability_gate_on_version`.
//!
//! The OWNED tsgo provider is version-gated: `TsgoOwnedProvider::attach` must run
//! the fail-closed wire gate (`probe_engine_version` → `gate::validate` over
//! `ObservedEngine::from_codec_wire`) BEFORE it opens the `--api` session — before
//! `initialize_api_session`, before connecting the attach pipe, before
//! constructing the `ApiAttachClient`, before `client.initialize`. A probe
//! failure / version mismatch / fingerprint mismatch returns
//! `Err(TypeProviderError)` and the owned provider is NEVER exposed.
//!
//! This STATIC guard targets ONLY `crates/verter_type_runtime/src/tsgo/owned.rs`
//! and ONLY the body of `impl TsgoOwnedProvider { pub async fn attach(...) }`
//! (extracted by a brace-depth scan from the `pub async fn attach` signature to
//! its matching closing brace). Asserting against the whole crate would be
//! satisfied by `TsgoClient::connect` or the gate's own unit tests — this targets
//! the OWNED attach body specifically.
//!
//! It asserts the attach body:
//!   - contains `probe_engine_version`, `gate::validate`, `ObservedEngine::from_codec_wire`; and
//!   - the gate (`gate::validate`) occurs BEFORE the first occurrence of each
//!     attach-initialization marker: `initialize_api_session`,
//!     `connect_attach_pipe`, `ApiAttachClient::new`, `client.initialize`.
//!
//! DISCRIMINATING: the inline self-test proves the ordering predicate FIRES on a
//! sample body with the gate before `initialize_api_session` and FAILS when the
//! gate is absent or appears AFTER attach initialization. So this guard is RED on
//! the pre-D2 tree (whose attach starts with `lsp.initialize_api_session().await?`
//! and has no gate) and GREEN after.

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

fn owned_rs() -> PathBuf {
    workspace_root().join("crates/verter_type_runtime/src/tsgo/owned.rs")
}

/// The required pre-attach gate calls.
const GATE_CALLS: &[&str] = &[
    "probe_engine_version",
    "gate::validate",
    "ObservedEngine::from_codec_wire",
];

/// The attach-initialization markers the gate must precede. These are the real
/// calls in `owned.rs`'s `attach` body (verified against source):
///   - `initialize_api_session` — opens the `--api` session,
///   - `connect_attach_pipe` — connects the attach pipe,
///   - `ApiAttachClient::new` — constructs the attach client,
///   - `client.initialize` — initializes the attach client.
const ATTACH_INIT_MARKERS: &[&str] = &[
    "initialize_api_session",
    "connect_attach_pipe",
    "ApiAttachClient::new",
    "client.initialize",
];

/// Extract the body of `pub async fn attach` from `src` via a brace-depth scan:
/// find the signature, advance to its opening `{`, then take to the matching `}`.
/// Returns `None` if the signature is absent or the braces are unbalanced.
fn extract_attach_body(src: &str) -> Option<String> {
    let sig = src.find("pub async fn attach")?;
    let after_sig = &src[sig..];
    let open_rel = after_sig.find('{')?;
    let body_bytes = after_sig.as_bytes();
    let mut depth = 0usize;
    let mut i = open_rel;
    while i < body_bytes.len() {
        match body_bytes[i] {
            b'{' => depth += 1,
            b'}' => {
                depth -= 1;
                if depth == 0 {
                    // body is between the first `{` (exclusive) and this `}`.
                    return Some(after_sig[open_rel + 1..i].to_string());
                }
            }
            _ => {}
        }
        i += 1;
    }
    None
}

/// Does `gate::validate` occur in `body` BEFORE the first occurrence of every
/// marker in `markers`, with all `required` gate calls present? Returns the list
/// of failures (empty ⇒ the gate is correctly placed pre-attach).
///
/// A marker that is ABSENT from the body is not an ordering failure (the gate
/// trivially precedes a call that does not exist); the gate's own PRESENCE is
/// asserted separately via `required`.
fn gate_precedes_attach_init(body: &str, required: &[&str], markers: &[&str]) -> Vec<String> {
    let mut failures = Vec::new();

    // Presence of every required gate call.
    for call in required {
        if !body.contains(call) {
            failures.push(format!("missing required pre-attach gate call `{call}`"));
        }
    }

    // The gate's ordering anchor is `gate::validate` (the fail-closed decision).
    let Some(gate_at) = body.find("gate::validate") else {
        // Absence already recorded above; nothing more to order.
        return failures;
    };

    for marker in markers {
        if let Some(marker_at) = body.find(marker) {
            if gate_at > marker_at {
                failures.push(format!(
                    "`gate::validate` (byte {gate_at}) occurs AFTER attach-init marker \
                     `{marker}` (byte {marker_at}) — the gate must run BEFORE any `--api` \
                     session is opened"
                ));
            }
        }
    }

    failures
}

#[test]
fn tsgo_capability_gate_on_version() {
    let src = read_owned();
    let body = extract_attach_body(&src).unwrap_or_else(|| {
        panic!(
            "could not extract the `pub async fn attach` body from {} — the owned attach path \
             must exist and be brace-balanced",
            owned_rs().display()
        )
    });

    let failures = gate_precedes_attach_init(&body, GATE_CALLS, ATTACH_INIT_MARKERS);

    assert!(
        failures.is_empty(),
        "the OWNED tsgo attach path (`TsgoOwnedProvider::attach` in \
         crates/verter_type_runtime/src/tsgo/owned.rs) must run the fail-closed wire gate \
         (`probe_engine_version` → `gate::validate(&ObservedEngine::from_codec_wire(..))`) \
         BEFORE opening the `--api` session (before `initialize_api_session` / \
         `connect_attach_pipe` / `ApiAttachClient::new` / `client.initialize`).\n{}",
        failures.join("\n")
    );
}

fn read_owned() -> String {
    let path = owned_rs();
    fs::read_to_string(&path).unwrap_or_else(|e| panic!("read {}: {e}", path.display()))
}

/// DISCRIMINATING self-test: the ordering predicate FIRES on a sample attach body
/// with the gate before `initialize_api_session`, and FAILS (records a failure)
/// when the gate is absent or appears AFTER attach initialization. This proves the
/// predicate itself is non-vacuous — it is not merely satisfied by the live source.
#[test]
fn tsgo_capability_gate_self_test_discriminates() {
    // A correctly-gated body: the gate runs first, then the `--api` session opens.
    let good = r#"
        let version = probe_engine_version(tsgo_bin.as_ref())
            .map_err(|e| TypeProviderError::new(format!("probe: {e}")))?;
        let _clearance = gate::validate(&ObservedEngine::from_codec_wire(version))
            .map_err(|e| TypeProviderError::new(format!("wire: {e}")))?;
        let session = lsp.initialize_api_session().await?;
        let (read, write) = connect_attach_pipe(&session.pipe).await?;
        let client = ApiAttachClient::new(JsonRpcConnection::connect(read, write));
        client.initialize().await?;
    "#;
    assert!(
        gate_precedes_attach_init(good, GATE_CALLS, ATTACH_INIT_MARKERS).is_empty(),
        "a body that gates BEFORE opening the --api session must pass"
    );

    // The pre-D2 body: attach starts with `initialize_api_session`, NO gate. The
    // predicate must record failures (missing gate calls).
    let pre_d2 = r#"
        let session = lsp.initialize_api_session().await?;
        let (read, write) = connect_attach_pipe(&session.pipe).await?;
        let client = ApiAttachClient::new(JsonRpcConnection::connect(read, write));
        client.initialize().await?;
    "#;
    let pre_failures = gate_precedes_attach_init(pre_d2, GATE_CALLS, ATTACH_INIT_MARKERS);
    assert!(
        !pre_failures.is_empty(),
        "the pre-D2 attach body (no gate at all) must FAIL the predicate"
    );
    assert!(
        pre_failures.iter().any(|f| f.contains("gate::validate")),
        "the failure must name the missing `gate::validate` gate call"
    );

    // A body that places the gate AFTER opening the session must record an
    // ORDERING failure (even though all gate calls are present).
    let after = r#"
        let session = lsp.initialize_api_session().await?;
        let version = probe_engine_version(tsgo_bin.as_ref())?;
        let _clearance = gate::validate(&ObservedEngine::from_codec_wire(version))?;
        let (read, write) = connect_attach_pipe(&session.pipe).await?;
        let client = ApiAttachClient::new(JsonRpcConnection::connect(read, write));
        client.initialize().await?;
    "#;
    let after_failures = gate_precedes_attach_init(after, GATE_CALLS, ATTACH_INIT_MARKERS);
    assert!(
        after_failures
            .iter()
            .any(|f| f.contains("AFTER attach-init marker `initialize_api_session`")),
        "a gate placed AFTER initialize_api_session must record an ordering failure; got {after_failures:?}"
    );

    // The brace-depth extractor isolates the attach body and stops at the matching
    // brace (it does not run past `attach` into a sibling fn).
    let sample = r#"
impl TsgoOwnedProvider {
    pub async fn attach(lsp: Arc<TsgoTypeProvider>) -> Result<Self, E> {
        let v = probe_engine_version(b)?;
        if v.is_empty() { return Err(E); }
        Ok(Self { lsp })
    }

    pub fn lsp_provider(&self) -> &Arc<TsgoTypeProvider> {
        connect_attach_pipe_should_not_be_seen()
    }
}
"#;
    let body = extract_attach_body(sample).expect("extracts attach body");
    assert!(
        body.contains("probe_engine_version"),
        "the extracted body must include the attach body's gate call"
    );
    assert!(
        !body.contains("connect_attach_pipe_should_not_be_seen"),
        "the extractor must STOP at attach's matching brace and not bleed into the sibling fn"
    );
}
