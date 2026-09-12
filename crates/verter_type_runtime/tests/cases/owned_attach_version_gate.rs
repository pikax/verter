//! The OWNED tsgo attach runs its fail-closed wire gate BEFORE it opens the
//! `--api` session, proven on real process + protocol traffic.
//!
//! `TsgoOwnedProvider::attach` probes the engine's `--version`, validates it
//! against the supported-window policy, and only then asks the already-running
//! `tsgo --lsp` process to mint an `--api` session
//! (`custom/initializeAPISession`). The ordering is the whole point: a refused
//! engine must never have a session opened against it, because an opened
//! session is a live attach pipe on a process whose wire Verter does not trust.
//!
//! Both legs drive a real child process (the deterministic fake engine, whose
//! `--version` answer is selected by the binary's file name) and observe the
//! real `--lsp` transport, so the assertion is "what reached the wire", not
//! "what the source text looks like":
//!
//! - REFUSED (`v710` reports `7.1.0`, outside the `>=7.0.2, <7.1.0` window):
//!   `attach` fails and NOT ONE BYTE reaches the `--lsp` transport.
//! - ACCEPTED (`ok` reports `7.0.2`): `attach` proceeds and
//!   `custom/initializeAPISession` DOES reach the `--lsp` transport.
//!
//! The pair is what discriminates. The refused leg alone would also pass for a
//! provider that had stopped talking to the `--lsp` surface entirely; the
//! accepted leg alone would also pass for a provider that opens the session
//! first and gates afterwards. Together they pin the gate strictly before the
//! session open.

use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use tokio::io::AsyncReadExt;

use verter_type_runtime::tsgo::{TsgoOwnedProvider, TsgoTypeProvider};

/// Copy the fake engine to a scenario-named path — the scenario rides the
/// binary's FILE NAME, so parallel tests never share mutable environment. The
/// copy lands via an atomic rename so a concurrent reader never executes a
/// partially-written file.
fn fake_engine(scenario: &str) -> PathBuf {
    static DIR: std::sync::OnceLock<PathBuf> = std::sync::OnceLock::new();
    let dir = DIR.get_or_init(|| verter_test_support::unique_temp_dir("verter-tr-fake-engines"));
    std::fs::create_dir_all(dir).expect("create fake engine dir");
    let name = if cfg!(windows) {
        format!("verter-tsgo-fake-{scenario}.exe")
    } else {
        format!("verter-tsgo-fake-{scenario}")
    };
    let target = dir.join(name);
    if !target.exists() {
        static COPY_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());
        let _guard = COPY_LOCK.lock().unwrap();
        if !target.exists() {
            let tmp = dir.join(format!(".copying-{scenario}"));
            std::fs::copy(
                verter_test_support::cargo_test_binary_path!("verter_type_runtime_fake_engine"),
                &tmp,
            )
            .expect("copy the fake engine");
            // A leftover from a killed earlier run must not block the rename.
            let _ = std::fs::remove_file(&target);
            std::fs::rename(&tmp, &target).expect("rename the fake engine into place");
        }
    }
    target
}

/// An `--lsp` feature provider over an in-memory transport whose peer end is
/// returned, so the test can observe exactly what `attach` puts on the wire.
/// Nothing answers the peer: every assertion below is about the REQUEST side.
fn lsp_over_duplex() -> (Arc<TsgoTypeProvider>, tokio::io::DuplexStream) {
    let (side, peer) = tokio::io::duplex(64 * 1024);
    let (read, write) = tokio::io::split(side);
    (
        Arc::new(TsgoTypeProvider::from_initialized_transport(read, write)),
        peer,
    )
}

/// Liveness bound for a cold child-process start plus its `--version` write.
/// Not a correctness threshold — nothing below asserts the bound itself.
const PROBE_BOUND: Duration = Duration::from_secs(30);

/// How long to wait for silence on the `--lsp` transport before concluding the
/// refused attach wrote nothing. Long enough that a provider which DID send
/// `custom/initializeAPISession` would be caught.
const SILENCE_WINDOW: Duration = Duration::from_secs(2);

#[tokio::test]
async fn refused_engine_version_opens_no_api_session_on_the_lsp_transport() {
    let (lsp, mut lsp_peer) = lsp_over_duplex();

    let attached = tokio::time::timeout(
        PROBE_BOUND,
        TsgoOwnedProvider::attach(lsp, fake_engine("v710")),
    )
    .await
    .expect("the version probe is bounded — attach must not hang on a refused engine");

    let err =
        attached.expect_err("an engine outside the supported version window must fail the attach");
    let message = format!("{err:?}");
    assert!(
        message.contains("7.1.0"),
        "the refusal must name the observed version it rejected; got: {message}"
    );

    // The whole invariant: the refused engine never had an `--api` session
    // opened against it, so the `--lsp` transport saw nothing at all.
    let mut buf = [0u8; 4096];
    let observed = tokio::time::timeout(SILENCE_WINDOW, lsp_peer.read(&mut buf)).await;
    if let Ok(Ok(n)) = observed {
        if n > 0 {
            panic!(
                "a version-refused attach must open NO `--api` session; it wrote {n} bytes to the \
                 --lsp transport: {}",
                String::from_utf8_lossy(&buf[..n])
            );
        }
    }
}

#[tokio::test]
async fn accepted_engine_version_opens_the_api_session_on_the_lsp_transport() {
    let (lsp, mut lsp_peer) = lsp_over_duplex();

    // Nothing answers `custom/initializeAPISession`, so `attach` would sit on
    // its own session timeout. The request itself is the observable under test.
    let attaching = tokio::spawn(async move {
        let _ = TsgoOwnedProvider::attach(lsp, fake_engine("ok")).await;
    });

    let mut seen = Vec::new();
    let mut buf = [0u8; 4096];
    let request = tokio::time::timeout(PROBE_BOUND, async {
        loop {
            let n = lsp_peer
                .read(&mut buf)
                .await
                .expect("read the --lsp transport");
            if n == 0 {
                return None;
            }
            seen.extend_from_slice(&buf[..n]);
            if let Ok(text) = std::str::from_utf8(&seen) {
                if text.contains("custom/initializeAPISession") {
                    return Some(text.to_string());
                }
            }
        }
    })
    .await
    .expect("an accepted engine must reach the --api session open within the probe bound");

    attaching.abort();

    let request = request.expect("the --lsp transport closed before the session open was sent");
    assert!(
        request.contains("custom/initializeAPISession"),
        "an accepted engine must open the `--api` session through the `--lsp` surface; saw: \
         {request}"
    );
}
