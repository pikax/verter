//! Headless mechanics e2e for the relay shim: `[fake editor] -> [real shim] ->
//! [real tsgo]`.
//!
//! A FAKE EDITOR spawns the REAL shim binary and speaks LSP over its stdio
//! (`initialize` → capture the relayed real-tsgo `serverInfo.version` →
//! `initialized`). It then plays the CONTROL CLIENT: reads the shim's
//! advertisement, `verter/hello` (nonce verified), `verter/waitInitialized`,
//! `verter/initializeApiSession` → connects the returned `--api` pipe DIRECTLY
//! with [`ApiAttachClient`], injects an INLINE off-disk carrier overlay via
//! `verter/carrierDidOpenSynced`, `updateSnapshot(tsconfig)`, and reads semantic
//! diagnostics. The REAL checker seeing the carrier (a deliberate TS2322, and
//! the negative: no spurious TS2307) proves the whole transport chain.
//!
//! The negatives prove the relay's leak/id-demux/version guarantees hold
//! end-to-end through the real shim + tsgo.
//!
//! Gating: NON-VACUOUS whenever tsgo is present. Under `VERTER_REQUIRE_TSGO` a
//! missing engine is a HARD failure (a skip would be a vacuous pass). The REAL
//! Verter-IDE-codegen `.vue`-macro proof is a later concern — an inline carrier
//! is the honest mechanics-level proof here.

use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex as StdMutex};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::process::{Child, ChildStdin, ChildStdout, Command};

use verter_tsgo_api::api_attach::ApiAttachClient;
use verter_tsgo_api::control::messages::PROTOCOL_VERSION;
use verter_tsgo_api::control::{Advertisement, ControlClient};
use verter_tsgo_api::jsonrpc::{encode_message, JsonRpcConnection, MessageFramer};
use verter_tsgo_api::proto::types::ProjectResponse;
use verter_tsgo_api::transport::pipe_attach::connect_attach_pipe;

fn workspace_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(|p| p.parent())
        .expect("workspace root above crates/")
        .to_path_buf()
}

/// Discover the engine, honoring `VERTER_REQUIRE_TSGO` (a skip under that env is
/// a vacuous-pass failure).
///
/// Coverage note: the `var_os` presence read below carries no regression test. Reaching it demands
/// a discovery FAILURE, and forcing one from inside this process means mutating the environment
/// this suite shares across its tests — so the only honest exercise of it is another self-re-execing
/// child harness. The fixture's own non-Unicode decoding is covered directly (see the probes near
/// [`non_utf8_path`]); this one line is held by review, not by a test.
async fn engine_or_skip() -> Option<PathBuf> {
    let request = verter_tsgo_api::toolchain::discovery::ResolutionRequest::for_environment(
        verter_tsgo_api::toolchain::validation::Capability::Lsp,
        Some(workspace_root()),
    );
    match verter_tsgo_api::toolchain::discovery::resolve(&request).await {
        Ok(resolution) => Some(resolution.path),
        Err(e) => {
            // PRESENCE semantics, so `var_os`: `std::env::var(..).is_ok()` reports FALSE for a
            // non-UTF-8 value, which would silently downgrade "the engine is REQUIRED" to a skip —
            // a vacuous pass in the one place this file refuses to allow one.
            if std::env::var_os("VERTER_REQUIRE_TSGO").is_some() {
                panic!("VERTER_REQUIRE_TSGO is set but tsgo was not found: {e}. A skip would be a vacuous pass.");
            }
            eprintln!("[skip] tsgo engine not found ({e}); set VERTER_REQUIRE_TSGO to require it");
            None
        }
    }
}

fn norm(p: &Path) -> String {
    p.to_string_lossy().replace('\\', "/")
}

/// Path comparison matching the tsgo engine's canonicalization (lowercased drive
/// letter, forward slashes, case-insensitive fold).
fn path_eq(a: &str, b: &str) -> bool {
    a.replace('\\', "/")
        .eq_ignore_ascii_case(&b.replace('\\', "/"))
}

/// The carrier's path AS THE ENGINE REPORTS IT in the project's root-file set.
fn engine_carrier_path<'a>(project: &'a ProjectResponse, carrier: &str) -> Option<&'a str> {
    project
        .root_files
        .iter()
        .find(|f| path_eq(f, carrier))
        .map(String::as_str)
}

fn tempdir(tag: &str) -> PathBuf {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let dir = std::env::temp_dir().join(format!(
        "verter_shim_live_{tag}_{}_{nanos}",
        std::process::id()
    ));
    std::fs::create_dir_all(&dir).expect("create temp dir");
    dir
}

/// A [`tempdir`] whose removal is owned by the scope, not by a statement at the bottom of a test.
///
/// A trailing `remove_dir_all` is skipped by exactly the paths that leave the most litter behind: an
/// early `return`, a `?`, and every panic — including the assertion failures a test exists to
/// produce. `Drop` runs on all of them (and while unwinding), so the directory goes away whether the
/// test passes, fails, or bails out before it ever reaches its assertions.
pub(super) struct ScopedTempDir(PathBuf);

impl ScopedTempDir {
    pub(super) fn new(tag: &str) -> Self {
        Self(tempdir(tag))
    }

    pub(super) fn path(&self) -> &Path {
        &self.0
    }
}

impl Drop for ScopedTempDir {
    fn drop(&mut self) {
        // Best-effort: a failure to clean up must not itself panic (a panic in `Drop` while already
        // unwinding aborts the process and would erase the real assertion message).
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

/// A configured project on disk: `src/util.ts` + a `tsconfig.json` whose
/// `include` covers `src/**/*` (so an off-disk `src/Carrier.ts` overlay is a
/// member). Returns the tsconfig path.
fn write_fixture(dir: &Path) -> PathBuf {
    let src = dir.join("src");
    std::fs::create_dir_all(&src).unwrap();
    std::fs::write(
        src.join("util.ts"),
        "export function double(n: number): number {\n  return n * 2;\n}\n",
    )
    .unwrap();
    let tsconfig = dir.join("tsconfig.json");
    std::fs::write(
        &tsconfig,
        r#"{
  "compilerOptions": {
    "strict": true,
    "target": "ES2020",
    "module": "ESNext",
    "moduleResolution": "Bundler",
    "noEmit": true,
    "skipLibCheck": true
  },
  "include": ["src/**/*"]
}
"#,
    )
    .unwrap();
    tsconfig
}

/// The off-disk carrier overlay: imports on-disk `./util` (must resolve — no
/// TS2307) and carries a uniquely-named deliberate TS2322 (`string → number`).
/// The unique symbol name + `Carrier.ts` basename + its URI are the leak probes.
fn carrier_fixture(dir: &Path) -> (String, String, String) {
    let carrier_path = dir.join("src").join("Carrier.ts");
    let carrier_norm = norm(&carrier_path);
    let carrier_uri = format!("file:///{}", carrier_norm.trim_start_matches('/'));
    let src = "import { double } from \"./util\";\n\
         export const ok: number = double(21);\n\
         export const verterCarrierLeakProbe: number = \"definitely not a number\";\n"
        .to_string();
    (carrier_norm, carrier_uri, src)
}

/// A fake editor over the shim's stdio: writes LSP frames to the shim's stdin
/// (via a serialized writer task), records EVERY frame the shim writes to its
/// stdout (so a leak test can inspect the whole editor-visible stream), AND
/// auto-answers each server→client request with `null` — exactly what a real
/// editor (and the crate's `JsonRpcConnection` default handler) does, so the
/// real tsgo never blocks on `workspace/configuration` / `client/registerCapability`.
struct FakeEditor {
    out_tx: tokio::sync::mpsc::UnboundedSender<Vec<u8>>,
    frames: Arc<StdMutex<Vec<serde_json::Value>>>,
}

impl FakeEditor {
    fn new(stdin: ChildStdin, stdout: ChildStdout) -> Self {
        // A serialized writer task owns stdin: both `send` and the reader's
        // auto-answers push onto it, so no two writes interleave.
        let (out_tx, mut out_rx) = tokio::sync::mpsc::unbounded_channel::<Vec<u8>>();
        tokio::spawn(async move {
            let mut stdin = stdin;
            while let Some(bytes) = out_rx.recv().await {
                if stdin.write_all(&bytes).await.is_err() {
                    break;
                }
                if stdin.flush().await.is_err() {
                    break;
                }
            }
        });

        let frames = Arc::new(StdMutex::new(Vec::new()));
        let sink = Arc::clone(&frames);
        let answer_tx = out_tx.clone();
        tokio::spawn(async move {
            let mut out = stdout;
            let mut framer = MessageFramer::new();
            let mut chunk = [0u8; 8192];
            loop {
                let n = match out.read(&mut chunk).await {
                    Ok(0) | Err(_) => break,
                    Ok(n) => n,
                };
                framer.push(&chunk[..n]);
                while let Ok(Some(msg)) = framer.next_message() {
                    // Auto-answer a server→client REQUEST (id + method) with a
                    // null result so tsgo does not block on configuration /
                    // capability round-trips.
                    let has_id = msg.get("id").map(|v| !v.is_null()).unwrap_or(false);
                    if has_id && msg.get("method").is_some() {
                        let id = msg.get("id").cloned().unwrap_or(serde_json::Value::Null);
                        let reply =
                            serde_json::json!({ "jsonrpc": "2.0", "id": id, "result": null });
                        let _ = answer_tx.send(encode_message(&reply));
                    }
                    sink.lock().unwrap().push(msg);
                }
            }
        });
        Self { out_tx, frames }
    }

    async fn send(&self, msg: &serde_json::Value) {
        let _ = self.out_tx.send(encode_message(msg));
    }

    fn all_frames(&self) -> Vec<serde_json::Value> {
        self.frames.lock().unwrap().clone()
    }

    async fn wait_for(
        &self,
        pred: impl Fn(&serde_json::Value) -> bool,
        timeout: Duration,
    ) -> Option<serde_json::Value> {
        let deadline = tokio::time::Instant::now() + timeout;
        loop {
            if let Some(found) = self
                .frames
                .lock()
                .unwrap()
                .iter()
                .find(|m| pred(m))
                .cloned()
            {
                return Some(found);
            }
            if tokio::time::Instant::now() >= deadline {
                return None;
            }
            tokio::time::sleep(Duration::from_millis(25)).await;
        }
    }
}

/// Resolve a sibling `[[bin]]` target's path the way a RELOCATED test binary must.
///
/// `env!("CARGO_BIN_EXE_<name>")` is a COMPILE-TIME constant: it bakes in the absolute path under
/// the build machine's `target/` directory. That is correct for a plain `cargo test`, and wrong the
/// moment the test binary is executed anywhere else — a `cargo nextest archive` extracted into a
/// different directory (or onto a different machine) has its binaries under the extraction root, and
/// the baked path either does not exist there or, on the same machine, still points at the ORIGINAL
/// `target/` copy so the run silently exercises a binary that is not the one under test.
///
/// nextest solves this by re-pointing every binary at RUNTIME: it sets `NEXTEST_BIN_EXE_<name>` for
/// each of the crate's `[[bin]]` targets to the path of the copy it is actually running from
/// (<https://nexte.st/docs/ci-features/archiving/>). Measured under nextest 0.9.130 here:
/// `NEXTEST_BIN_EXE_verter-relay-shim` and `NEXTEST_BIN_EXE_fake_tsgo_heartbeat` are both present in
/// the test process's environment, and an archive run points them at the EXTRACTION directory.
///
/// So: prefer the runtime variable, and fall back to the compile-time `env!` ONLY outside nextest —
/// a plain `cargo test` sets no such variable and needs no remapping.
///
/// The fallback is FAIL-CLOSED under nextest, which is the whole point. An unconditional fallback
/// looks harmless (the variable "is always set") but is a fail-open hole: nextest, or a future
/// version of it, failing to export one `NEXTEST_BIN_EXE_*` would not surface as an error — the run
/// would quietly launch the ORIGINAL build-tree binary. On a relocated archive whose extracted
/// fixture is stale while `target/` stays pristine, `fake_tsgo_fixture_binary_matches_its_source`
/// would then hash the pristine original and PASS, reproducing exactly the false-green it exists to
/// prevent. nextest publishes `NEXTEST=1` so a test can detect it
/// (<https://nexte.st/docs/configuration/env-vars/>); with that marker present, a missing runtime
/// path is a setup failure, not a fallback. See [`resolve_bin_exe`] for the decision and the four
/// tests that pin it.
macro_rules! bin_exe {
    ($name:literal) => {{
        match resolve_bin_exe(
            $name,
            std::env::var_os(concat!("NEXTEST_BIN_EXE_", $name)),
            std::env::var_os("NEXTEST").is_some(),
            env!(concat!("CARGO_BIN_EXE_", $name)),
        ) {
            Ok(path) => path,
            Err(message) => panic!("{message}"),
        }
    }};
}

/// The resolution decision behind [`bin_exe!`], factored out so it is testable without mutating the
/// process environment (a process-global mutation this suite must not perform).
///
/// `runtime_path` is the value of `NEXTEST_BIN_EXE_<name>`, `under_nextest` is the presence of
/// nextest's own `NEXTEST` marker variable, and `compile_time_path` is `CARGO_BIN_EXE_<name>` as
/// baked in at compile time.
///
/// An EMPTY runtime value is treated exactly as an absent one: it is not a path, and using it would
/// turn the defect into an obscure "cannot spawn `''`" instead of a named setup failure.
pub(super) fn resolve_bin_exe(
    name: &str,
    runtime_path: Option<std::ffi::OsString>,
    under_nextest: bool,
    compile_time_path: &str,
) -> Result<PathBuf, String> {
    verter_test_support::resolve_test_binary_path(
        name,
        runtime_path,
        under_nextest,
        compile_time_path,
    )
}

/// A missing runtime variable UNDER NEXTEST must be a loud failure, never the build-tree binary.
#[test]
fn under_nextest_a_missing_runtime_bin_var_fails_instead_of_using_the_build_tree_binary() {
    let build_tree = "/build/machine/target/debug/fake_tsgo_heartbeat";
    let resolved = resolve_bin_exe("fake_tsgo_heartbeat", None, true, build_tree);
    let message = resolved.expect_err(
        "under nextest a missing NEXTEST_BIN_EXE_* must FAIL: silently launching the compile-time \
         build-tree binary lets a relocated archive validate a pristine original while running a \
         stale extracted copy — the exact false-green the fixture-freshness guard exists to prevent",
    );
    assert!(
        message.contains("NEXTEST_BIN_EXE_fake_tsgo_heartbeat"),
        "the failure must name the missing variable so the setup defect is actionable; got \
         {message:?}"
    );
    assert!(
        !message.contains(build_tree),
        "the failure must not present the build-tree path as a usable resolution; got {message:?}"
    );
}

/// An EMPTY runtime value is not a usable path — under nextest it is the same setup defect.
#[test]
fn under_nextest_an_empty_runtime_bin_var_is_treated_as_missing_not_as_an_empty_path() {
    let resolved = resolve_bin_exe(
        "verter-relay-shim",
        Some(std::ffi::OsString::from("")),
        true,
        "/build/machine/target/debug/verter-relay-shim",
    );
    let message = resolved
        .expect_err("an empty NEXTEST_BIN_EXE_* is not a path; it must fail loudly under nextest");
    assert!(
        message.contains("NEXTEST_BIN_EXE_verter-relay-shim"),
        "the failure must name the offending variable; got {message:?}"
    );
}

/// OUTSIDE nextest the compile-time fallback is the correct answer — a plain `cargo test` sets no
/// runtime variable and needs no remapping. This is the arm the fail-closed rule must NOT break.
///
/// Scope: this pins the DECISION only, by calling the helper directly. It says nothing about the
/// macro's wiring into that arm, because under the canonical gate every real `bin_exe!` invocation
/// runs under nextest with a runtime path present — so the macro's `env!(concat!("CARGO_BIN_EXE_",
/// …))` argument is never observed. [`the_bin_exe_macro_itself_uses_the_compile_time_constant_without_nextest`]
/// owns that half.
#[test]
fn outside_nextest_the_compile_time_fallback_is_still_used() {
    let compile_time = "/build/machine/target/debug/fake_tsgo_heartbeat";
    let resolved = resolve_bin_exe("fake_tsgo_heartbeat", None, false, compile_time)
        .expect("outside nextest the compile-time path must remain the fallback");
    assert_eq!(
        resolved,
        PathBuf::from(compile_time),
        "a plain `cargo test` run must keep resolving through the compile-time `CARGO_BIN_EXE_*`"
    );
}

/// A present runtime value always wins over the compile-time constant, in either environment.
#[test]
fn a_present_runtime_bin_var_wins_over_the_compile_time_constant() {
    let compile_time = "/build/machine/target/debug/fake_tsgo_heartbeat";
    let runtime = "/extracted/archive/fake_tsgo_heartbeat";
    for under_nextest in [true, false] {
        let resolved = resolve_bin_exe(
            "fake_tsgo_heartbeat",
            Some(std::ffi::OsString::from(runtime)),
            under_nextest,
            compile_time,
        )
        .expect("a present runtime path resolves");
        assert_eq!(
            resolved,
            PathBuf::from(runtime),
            "the RUNTIME path must win (under_nextest={under_nextest})"
        );
        assert_ne!(
            resolved,
            PathBuf::from(compile_time),
            "the compile-time constant must not be preferred when a runtime path exists \
             (under_nextest={under_nextest})"
        );
    }
}

/// The environment half of the contract, for EVERY binary this suite launches.
///
/// The pure-function tests above pin the decision; this one pins the premise the decision rests on
/// — that a real nextest run actually supplies a usable `NEXTEST_BIN_EXE_*` for both bins. Under a
/// plain `cargo test` there is nothing to check (no nextest, no remapping), so the body is
/// deliberately nextest-only: it is a live assertion on the nextest gate surface, where a nextest
/// upgrade that stopped exporting one of these variables would now surface HERE as a setup failure
/// rather than as a silent fall-back to the build tree.
#[test]
fn under_nextest_every_launched_bin_has_a_usable_runtime_path() {
    if std::env::var_os("NEXTEST").is_none() {
        eprintln!("[skip] not running under nextest; the runtime-variable premise does not apply");
        return;
    }
    for name in ["verter-relay-shim", "fake_tsgo_heartbeat"] {
        let var = format!("NEXTEST_BIN_EXE_{name}");
        let value = std::env::var_os(&var)
            .unwrap_or_else(|| panic!("nextest must export {var} for the bin this suite launches"));
        assert!(!value.is_empty(), "{var} must be a usable path, not empty",);
        let path = PathBuf::from(&value);
        assert!(
            path.is_file(),
            "{var} must point at an existing binary; got {path:?}"
        );
    }
    // And the macro itself must agree with that environment EXACTLY, for both bins — not merely
    // resolve to SOME existing file. `bin_exe!` hands the runtime `OsString` through verbatim
    // (`PathBuf::from(path)`), so strict equality is the right comparison: nothing on either side
    // normalises separators, appends `.exe`, or rewrites the value, so the two cannot legitimately
    // differ. Canonicalising would be WRONG here — it resolves symlinks, so a build tree symlinked
    // to (or from) an extraction root would compare EQUAL to it, dissolving the very distinction
    // under test.
    //
    // Honest scope: on a same-machine, non-archive nextest run the runtime value and the
    // compile-time constant happen to be the SAME string, so this equality alone cannot tell a
    // still-wired macro from one rewired straight to `env!("CARGO_BIN_EXE_…")`. It is what makes a
    // REAL relocated-archive run red on that defect; the same-machine discrimination is owned by
    // [`the_bin_exe_macro_itself_honours_a_relocated_runtime_path`], which drives the macro with a
    // runtime path that provably differs from the build tree.
    assert_eq!(
        bin_exe!("verter-relay-shim"),
        runtime_bin_path("verter-relay-shim"),
        "the macro must resolve to exactly the path nextest supplied, not merely to some file \
         that happens to exist"
    );
    assert_eq!(
        bin_exe!("fake_tsgo_heartbeat"),
        runtime_bin_path("fake_tsgo_heartbeat"),
        "the macro must resolve to exactly the path nextest supplied, not merely to some file \
         that happens to exist"
    );
}

/// The `NEXTEST_BIN_EXE_<name>` value as a path, for a bin nextest is known to have exported.
fn runtime_bin_path(name: &str) -> PathBuf {
    PathBuf::from(
        std::env::var_os(format!("NEXTEST_BIN_EXE_{name}"))
            .unwrap_or_else(|| panic!("nextest must export NEXTEST_BIN_EXE_{name}")),
    )
}

/// The compile-time `CARGO_BIN_EXE_<name>` constant for a bin this suite launches.
///
/// `env!` takes a literal, so the crate's two `[[bin]]` targets are spelled out; an unknown name is
/// a hard error rather than a silent miss.
///
/// PRIVATE ON PURPOSE, and it must stay that way. Rust visibility is prefix-closed downward, so any
/// `pub` — even `pub(super)` — would hand this function to
/// [`super::relocation_control`](super::relocation_control), the module that drives the relocation
/// control. That module must be structurally unable to name the build tree: it exists to prove the
/// control sources its copies from the RUNTIME path, and it can only prove that if the historical
/// direct `compile_time_bin_path(name)` call is a compile error there rather than a silent green.
/// Being private to `shim_live` is what makes that error `E0603` from every path spelling.
fn compile_time_bin_path(name: &str) -> &'static str {
    match name {
        "verter-relay-shim" => env!("CARGO_BIN_EXE_verter-relay-shim"),
        "fake_tsgo_heartbeat" => env!("CARGO_BIN_EXE_fake_tsgo_heartbeat"),
        other => panic!("no compile-time CARGO_BIN_EXE_* constant is known for the bin {other:?}"),
    }
}

/// The bins `bin_exe!` is invoked with, and which the relocation control therefore covers.
pub(super) const LAUNCHED_BINS: [&str; 2] = ["verter-relay-shim", "fake_tsgo_heartbeat"];

/// Set by the parent half of the relocation control to the directory holding the relocated copies.
/// Its presence is what ARMS the child half; nothing else sets it.
pub(super) const RELOCATED_BIN_DIR_VAR: &str = "VERTER_SHIM_TEST_RELOCATED_BIN_DIR";

/// Printed by the child half ONLY after every macro result matched its relocated path. The parent
/// requires it, so a child that skipped, was filtered out, or never reached the assertions cannot
/// report a pass.
pub(super) const RELOCATION_SENTINEL: &str =
    "relocated-bin-exe-control: every macro result matched";

/// The libtest path of the child half, as the parent's `--exact` filter. A rename that desyncs this
/// makes the filter match nothing, which the parent's sentinel + `1 passed` checks turn into a loud
/// failure rather than a silent green.
pub(super) const RELOCATION_CHILD_TEST: &str =
    "cases::shim_live::relocated_runtime_paths_are_what_bin_exe_resolves_to";

/// Printed by the plain-Cargo control's child half ONLY after every macro result matched the
/// compile-time constant. The parent requires it, so a child that skipped, was filtered out, or
/// never reached its assertions cannot report a pass.
const PLAIN_CARGO_SENTINEL: &str = "plain-cargo-bin-exe-control: every macro result matched";

/// The libtest path of the plain-Cargo control's child half, as the parent's `--exact` filter. A
/// rename that desyncs this makes the filter match nothing, which the parent's sentinel +
/// `1 passed` checks turn into a loud failure rather than a silent green.
const PLAIN_CARGO_CHILD_TEST: &str =
    "cases::shim_live::without_nextest_bin_exe_resolves_to_the_compile_time_constant";

/// The MACRO's own wiring, driven through a runtime path that provably differs from the build tree.
///
/// [`under_nextest_every_launched_bin_has_a_usable_runtime_path`] asserts the macro equals the
/// runtime value, but on a same-machine nextest run that value IS the compile-time constant
/// (measured: both are `<worktree>/target/debug/<bin>`), so it cannot distinguish a macro that still
/// consults [`resolve_bin_exe`] from one rewired to `PathBuf::from(env!("CARGO_BIN_EXE_…"))` — the
/// exact fail-open regression the runtime resolution exists to prevent, and one the four pure
/// [`resolve_bin_exe`] tests cannot see either, because they call the helper directly.
///
/// So this reproduces the ONE condition that separates the two: a relocated archive, where the
/// runtime path is NOT the build-tree path. Real copies of both bins — taken from the path this
/// process is itself running them from, never from the build tree, see
/// [`super::relocation_control::drive_relocation_control`] — are placed in a temp directory, and
/// the test binary re-runs ITSELF as a child with `NEXTEST=1`
/// and both `NEXTEST_BIN_EXE_*` pointed at those copies — a child's environment, never this
/// process's (a process-global mutation this suite must not perform). The child then asserts each
/// `bin_exe!` result IS the relocated copy and is NOT the build-tree binary; rewiring the macro past
/// the helper makes it resolve to the build tree and fails both assertions.
///
/// The control is proven to have APPLIED, not merely to have exited 0: libtest exits 0 when a filter
/// matches nothing, so the parent requires the child's sentinel line AND its `1 passed` summary.
#[test]
fn the_bin_exe_macro_itself_honours_a_relocated_runtime_path() {
    super::relocation_control::drive_relocation_control(&|name| {
        compile_time_bin_path(name).to_owned()
    });
}

/// The control must be runnable WHERE A RELOCATED ARCHIVE ACTUALLY RUNS.
///
/// A control that proves the macro honours a relocated path is worth nothing if the control itself
/// assumes a non-relocated layout. Sourcing the copies from the compile-time `CARGO_BIN_EXE_*`
/// constant would do exactly that: on the host an archive was moved to, the builder's absolute
/// `target/` tree is absent, so the copy fails with "No such file or directory" — the control dies
/// in setup, INCIDENTALLY, in precisely the scenario it exists to simulate, and never reaches the
/// macro assertion it was written for.
///
/// So [`super::relocation_control::drive_relocation_control`] takes the compile-time constant as an
/// INJECTED parameter and this test drives the whole control — copy, child spawn, sentinel,
/// `1 passed` — with that parameter pointed at a directory proven not to exist. Green here means the
/// control's copy source is NOT the injected build-tree path: under nextest it is the runtime
/// `NEXTEST_BIN_EXE_*` value, which is present by definition inside the archive the test binary is
/// running from.
///
/// An injected absent path only proves that if the injection is PROVEN CONSUMED. Two independent
/// rails do that, because an absent-path assertion alone does not: a driver that ignored the
/// parameter and read the build tree directly would leave the absent directory absent — and pass.
///
/// 1. STRUCTURAL (the primary): the driver lives in [`super::relocation_control`], a SIBLING module
///    of this one, and [`compile_time_bin_path`] is private to `shim_live`. Rust visibility is
///    prefix-closed downward, so a sibling is outside it: the historical
///    `PathBuf::from(compile_time_bin_path(name))` inside the driver is `E0603`/`E0425` — it does
///    not compile, under any path spelling. The injected closure is the only build-tree path the
///    driver can obtain.
/// 2. INVOCATION COUNT (the complement): the closure below counts its calls, and this test asserts
///    one call per launched bin. The structural rail cannot stop the driver from re-deriving the
///    constant through `env!` (a macro no visibility rule can seal), but such a driver ignores the
///    parameter — and the count catches exactly that.
///
/// Nextest-only by construction: outside nextest there is no runtime path, the compile-time fallback
/// is the only (and correct) source, and a plain `cargo test` build tree exists by definition.
#[test]
fn the_relocation_control_does_not_read_the_compile_time_build_tree() {
    if std::env::var_os("NEXTEST").is_none() {
        eprintln!(
            "[skip] not running under nextest; there is no runtime path to source from and the \
             compile-time fallback is the correct source for a plain `cargo test`"
        );
        return;
    }

    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let absent_build_tree = std::env::temp_dir().join(format!(
        "verter_shim_absent_build_tree_{}_{nanos}",
        std::process::id()
    ));
    // Prove the negative control APPLIED: a "nonexistent" path that happens to exist would make this
    // test pass for the wrong reason.
    assert!(
        !absent_build_tree.exists(),
        "the stand-in build tree must NOT exist, or this test proves nothing; {absent_build_tree:?}"
    );

    let injections = AtomicUsize::new(0);
    super::relocation_control::drive_relocation_control(&|name| {
        injections.fetch_add(1, Ordering::Relaxed);
        // Only the DIRECTORY is bogus: the platform's own file name (`.exe` on Windows) is kept, so
        // a failure can only come from the absent directory, never from a name mismatch.
        let real = PathBuf::from(compile_time_bin_path(name));
        let file_name = real.file_name().expect("a bin path ends in a file name");
        absent_build_tree
            .join(file_name)
            .to_string_lossy()
            .into_owned()
    });

    // The injection is the control. A driver that never asked for it proved nothing by leaving the
    // absent directory absent, so require one ask per launched bin — the count a driver that
    // re-derived the constant for itself (through `env!`, the one spelling no visibility rule can
    // seal) would fail.
    assert_eq!(
        injections.load(Ordering::Relaxed),
        LAUNCHED_BINS.len(),
        "the control must obtain its build-tree path for EVERY launched bin from the injected \
         provider; a driver that sources one itself makes this test vacuous"
    );
    assert!(
        !absent_build_tree.exists(),
        "the control must not have created the stand-in build tree; {absent_build_tree:?}"
    );
}

/// The child half of [`the_bin_exe_macro_itself_honours_a_relocated_runtime_path`]. It is inert
/// unless that parent arms it through [`RELOCATED_BIN_DIR_VAR`], because it needs an environment no
/// test may impose on its own process.
#[test]
fn relocated_runtime_paths_are_what_bin_exe_resolves_to() {
    let Some(relocated) = std::env::var_os(RELOCATED_BIN_DIR_VAR) else {
        eprintln!(
            "[skip] child half of the relocation control; \
             `the_bin_exe_macro_itself_honours_a_relocated_runtime_path` drives it"
        );
        return;
    };
    let relocated = PathBuf::from(relocated);

    // Every `bin_exe!` invocation in this suite names one of these two bins, so covering both
    // covers the macro's whole live surface.
    for (name, resolved) in [
        ("verter-relay-shim", bin_exe!("verter-relay-shim")),
        ("fake_tsgo_heartbeat", bin_exe!("fake_tsgo_heartbeat")),
    ] {
        let build_tree = PathBuf::from(compile_time_bin_path(name));
        let expected = relocated.join(build_tree.file_name().expect("a bin file name"));
        // Exact equality, not canonicalisation: both sides descend from the one `OsString` the
        // parent set, and canonicalising would resolve symlinks and could collapse the relocated
        // copy onto the build tree — erasing the distinction this control exists to draw.
        assert_eq!(
            resolved, expected,
            "`bin_exe!({name:?})` must resolve to the RUNTIME path nextest supplied"
        );
        assert_ne!(
            resolved, build_tree,
            "`bin_exe!({name:?})` resolved to the compile-time build-tree binary; on a relocated \
             archive that silently runs a binary that is not the one under test"
        );
        assert!(
            resolved.is_file(),
            "the relocated copy must exist; got {resolved:?}"
        );
    }
    println!("{RELOCATION_SENTINEL}");
}

/// The MACRO's plain-Cargo arm, observed through the macro itself rather than through
/// [`resolve_bin_exe`].
///
/// The gap this closes: every real `bin_exe!` invocation in the canonical gate runs under nextest
/// with a runtime path present, and the gate's second, non-nextest surface executes only
/// `verter_session`. So the macro's `env!(concat!("CARGO_BIN_EXE_", $name))` ARGUMENT — the value the
/// fallback arm exists to deliver — was never evaluated by anything the gate runs.
/// [`outside_nextest_the_compile_time_fallback_is_still_used`] cannot see it either: it calls the
/// helper directly and passes the path in by hand. A macro rewired to hand the fallback an empty
/// string, or to the wrong constant, stayed green everywhere.
///
/// So this reproduces the one environment that exercises that argument: a child process with
/// `NEXTEST` and every `NEXTEST_BIN_EXE_*` REMOVED — a child's environment, never this process's (a
/// process-global mutation this suite must not perform). The child invokes the real macro for both
/// launched bins and asserts each equals the compile-time constant.
///
/// The control is proven to have APPLIED, not merely to have exited 0: libtest exits 0 when a filter
/// matches nothing, so this requires the child's sentinel line AND its `1 passed` summary.
#[test]
fn the_bin_exe_macro_itself_uses_the_compile_time_constant_without_nextest() {
    let mut child = std::process::Command::new(
        std::env::current_exe().expect("the running test binary's own path"),
    );
    child
        .arg("--exact")
        .arg(PLAIN_CARGO_CHILD_TEST)
        .arg("--nocapture")
        // Strip the whole nextest environment the macro consults, so the child is a plain-Cargo run
        // by construction: no marker, and no runtime path for either bin.
        .env_remove("NEXTEST");
    for name in LAUNCHED_BINS {
        child.env_remove(format!("NEXTEST_BIN_EXE_{name}"));
    }

    let output = child.output().expect("run the child half of the control");
    let stdout = String::from_utf8_lossy(&output.stdout).into_owned();
    let stderr = String::from_utf8_lossy(&output.stderr).into_owned();

    assert!(
        output.status.success(),
        "`bin_exe!` did not resolve to the compile-time constant outside nextest: with no runtime \
         path available the macro must hand `resolve_bin_exe` the `CARGO_BIN_EXE_*` value for the \
         bin it names.\n--- child stdout ---\n{stdout}\n--- child stderr ---\n{stderr}"
    );
    assert!(
        stdout.contains(PLAIN_CARGO_SENTINEL),
        "the child half must have RUN its assertions — no {PLAIN_CARGO_SENTINEL:?} in its output, \
         so the control did not apply (a filter that matches nothing still exits 0).\n\
         --- child stdout ---\n{stdout}\n--- child stderr ---\n{stderr}"
    );
    assert!(
        stdout.contains("1 passed"),
        "exactly the one filtered child test must have run; the `--exact {PLAIN_CARGO_CHILD_TEST}` \
         filter is out of date.\n--- child stdout ---\n{stdout}\n--- child stderr ---\n{stderr}"
    );
}

/// The child half of [`the_bin_exe_macro_itself_uses_the_compile_time_constant_without_nextest`].
///
/// Inert under nextest, where there is no plain-Cargo arm to observe. Under a genuine
/// `cargo test -p verter_relay_shim --tests` it also runs on its own — the same assertions, on the
/// same macro, in the environment the parent has to synthesise under nextest.
///
/// The assertion is a STRING comparison against the constant, and deliberately never a filesystem
/// check. `CARGO_BIN_EXE_*` names a path in the BUILD machine's `target/` tree; on a relocated
/// archive that tree is absent, so demanding the resolved path exist would fail a run whose macro
/// resolution is entirely correct. What the macro must get right here is WHICH constant it hands
/// the fallback — that is a value comparison, not an on-disk one.
#[test]
fn without_nextest_bin_exe_resolves_to_the_compile_time_constant() {
    if std::env::var_os("NEXTEST").is_some() {
        eprintln!(
            "[skip] running under nextest; \
             `the_bin_exe_macro_itself_uses_the_compile_time_constant_without_nextest` drives the \
             plain-Cargo environment this half asserts in"
        );
        return;
    }
    // The premise, asserted rather than assumed: with any of these present the macro would take the
    // runtime arm and this test would pin nothing.
    for name in LAUNCHED_BINS {
        let var = format!("NEXTEST_BIN_EXE_{name}");
        assert!(
            std::env::var_os(&var).is_none(),
            "{var} is set, so the macro would resolve through the RUNTIME arm and this test would \
             not observe the compile-time fallback at all"
        );
    }

    // Every `bin_exe!` invocation in this suite names one of these two bins, so covering both
    // covers the macro's whole live surface.
    for (name, resolved) in [
        ("verter-relay-shim", bin_exe!("verter-relay-shim")),
        ("fake_tsgo_heartbeat", bin_exe!("fake_tsgo_heartbeat")),
    ] {
        assert_eq!(
            resolved,
            PathBuf::from(compile_time_bin_path(name)),
            "with no runtime path available, `bin_exe!({name:?})` must resolve to that bin's \
             compile-time `CARGO_BIN_EXE_*` constant"
        );
    }
    println!("{PLAIN_CARGO_SENTINEL}");
}

/// Path of the `verter-relay-shim` binary this test process is actually
/// running against (`NEXTEST_BIN_EXE_*` under nextest, `CARGO_BIN_EXE_*`
/// outside it). Sibling live tests reuse this so they never spawn cargo.
pub(super) fn relay_shim_bin() -> PathBuf {
    bin_exe!("verter-relay-shim")
}

/// Spawn the REAL shim binary as the editor's `tsgo`, forwarding `--lsp
/// --stdio` to the real engine.
fn spawn_shim(tsgo: &Path, control_dir: &Path, session_key: &str) -> Child {
    Command::new(relay_shim_bin())
        .arg("--real-tsgo")
        .arg(tsgo)
        .arg("--control-dir")
        .arg(control_dir)
        .arg("--session-key")
        .arg(session_key)
        .arg("--")
        .arg("--lsp")
        .arg("--stdio")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::inherit())
        .spawn()
        .expect("spawn the relay shim binary")
}

/// Poll `control_dir` until the shim publishes its advertisement, returning its PATH alongside it.
///
/// The path matters beyond discovery: `run_relay` removes the advertisement at exactly ONE place —
/// immediately after its steady-state teardown select resolves — so the file's DISAPPEARANCE is an
/// observable witness that the shim has left that select. `relay_stop_with_crashed_child_…` below
/// uses it to order a crash strictly after the teardown arm has been chosen.
async fn wait_for_advertisement(control_dir: &Path, session_key: &str) -> (PathBuf, Advertisement) {
    let deadline = tokio::time::Instant::now() + Duration::from_secs(20);
    loop {
        if let Ok((path, adv)) = Advertisement::find_for_session_key(control_dir, session_key) {
            return (path, adv);
        }
        assert!(
            tokio::time::Instant::now() < deadline,
            "the shim never published its advertisement"
        );
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
}

/// Await a control/api future under a bounded deadline so a mechanics hang fails
/// fast with the step name (never an unbounded wall-clock hang).
async fn with_timeout<F: std::future::Future>(step: &str, fut: F) -> F::Output {
    match tokio::time::timeout(Duration::from_secs(45), fut).await {
        Ok(v) => v,
        Err(_) => panic!("mechanics step {step:?} timed out (bounded deadline)"),
    }
}

fn init_params(root_uri: &str) -> serde_json::Value {
    serde_json::json!({
        "processId": std::process::id(),
        "rootUri": root_uri,
        "capabilities": {},
        "workspaceFolders": [{ "uri": root_uri, "name": "verter" }],
    })
}

/// The wired session: shim + fake editor + control client + attached `--api`
/// checker, initialized up to (but excluding) carrier injection.
struct Harness {
    shim: Child,
    editor: FakeEditor,
    ctl: ControlClient,
    api: ApiAttachClient,
    server_version: String,
    dir: PathBuf,
    tsconfig_norm: String,
    /// The discovered shim advertisement — its endpoint + nonce let a test open a
    /// FRESH control connection after a detach (to prove the shim stayed alive).
    adv: Advertisement,
}

/// Drive the full chain up to a ready attached `--api` checker: spawn the shim,
/// run the editor LSP handshake over its stdio, read + verify the advertisement,
/// hello, waitInitialized, initializeApiSession, connect the `--api` pipe.
async fn setup(tsgo: &Path, tag: &str) -> Harness {
    let dir = tempdir(tag);
    let tsconfig = write_fixture(&dir);
    let tsconfig_norm = norm(&tsconfig);
    let root_uri = format!("file:///{}", norm(&dir).trim_start_matches('/'));
    let control_dir = dir.join("ctl");
    let session_key = tag.to_string();

    let mut shim = spawn_shim(tsgo, &control_dir, &session_key);
    let editor_stdin = shim.stdin.take().expect("shim stdin piped");
    let editor_stdout = shim.stdout.take().expect("shim stdout piped");
    let editor = FakeEditor::new(editor_stdin, editor_stdout);

    // LSP initialize over the shim stdio → the relayed REAL tsgo serverInfo.version.
    editor
        .send(&serde_json::json!({
            "jsonrpc": "2.0", "id": 1, "method": "initialize", "params": init_params(&root_uri),
        }))
        .await;
    let init_resp = editor
        .wait_for(|m| m["id"] == 1, Duration::from_secs(40))
        .await
        .expect("the relayed initialize response");
    let relayed_version = init_resp["result"]["serverInfo"]["version"]
        .as_str()
        .expect("the relayed initialize carries serverInfo.version")
        .to_string();
    eprintln!("[mechanics] relayed real-tsgo serverInfo.version = {relayed_version:?}");
    assert_eq!(
        relayed_version, "7.0.2",
        "the fake editor observes the REAL relayed tsgo version"
    );
    editor
        .send(&serde_json::json!({ "jsonrpc": "2.0", "method": "initialized", "params": {} }))
        .await;

    // The control client: discover the advertisement, verify the nonce on hello.
    let (_adv_path, adv) = wait_for_advertisement(&control_dir, &session_key).await;
    assert_eq!(adv.protocol, PROTOCOL_VERSION);
    let mut ctl = ControlClient::connect(&adv.endpoint)
        .await
        .expect("connect the control endpoint");
    let hello = with_timeout("hello", ctl.hello(&adv.nonce, "verter_lsp"))
        .await
        .expect("hello (nonce + protocol verified)");
    assert_eq!(
        hello.editor_session_generation,
        adv.editor_session_generation
    );
    assert_eq!(hello.wire_pin, adv.wire_pin);

    // waitInitialized: the in-band witness the relay captured.
    let witness = with_timeout("waitInitialized", ctl.wait_initialized())
        .await
        .expect("waitInitialized");
    let server_version = witness
        .server_info_version
        .clone()
        .expect("the in-band serverInfo.version witness");
    assert_eq!(server_version, "7.0.2");
    assert_eq!(witness.root_uri.as_deref(), Some(root_uri.as_str()));

    // initializeApiSession → connect the minted `--api` pipe DIRECTLY.
    let api_session = with_timeout("initializeApiSession", ctl.initialize_api_session())
        .await
        .expect("initializeApiSession");
    assert_eq!(api_session.handle_kind, "integer");
    let endpoint = api_session.endpoint().expect("a minted --api endpoint");
    let (read, write) = connect_attach_pipe(endpoint)
        .await
        .expect("connect the minted --api pipe");
    let api = ApiAttachClient::new(JsonRpcConnection::connect(read, write));
    with_timeout("--api initialize", api.initialize())
        .await
        .expect("--api initialize");

    Harness {
        shim,
        editor,
        ctl,
        api,
        server_version,
        dir,
        tsconfig_norm,
        adv,
    }
}

/// Retract Verter's overlays (a NON-DESTRUCTIVE `verter/detach`), then kill the
/// shim process (the test owns the shim's lifecycle here) + clean the temp dir.
async fn teardown(mut h: Harness) {
    let _ = h.ctl.detach(true).await;
    let _ = h.ctl.close().await;
    let _ = h.shim.start_kill();
    let _ = tokio::time::timeout(Duration::from_secs(10), h.shim.wait()).await;
    let _ = std::fs::remove_dir_all(&h.dir);
}

/// THE mechanics proof: `[fake editor] -> [real shim] -> [real tsgo]`. An inline
/// carrier injected over the control protocol is seen by the attached `--api`
/// checker (deliberate TS2322 present; no spurious TS2307).
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn fake_editor_through_real_shim_and_tsgo_sees_injected_carrier() {
    let Some(tsgo) = engine_or_skip().await else {
        return;
    };
    let h = setup(&tsgo, "mechanics").await;
    let (carrier_norm, carrier_uri, carrier_src) = carrier_fixture(&h.dir);

    // Inject the off-disk carrier over the CONTROL protocol (relay injection +
    // sync barrier), then open the configured project on the --api side.
    with_timeout(
        "carrierDidOpenSynced",
        h.ctl
            .carrier_did_open_synced(&carrier_uri, "typescript", 1, &carrier_src),
    )
    .await
    .expect("carrier didOpenSynced through the control protocol");
    let snap = tokio::time::timeout(
        Duration::from_secs(30),
        h.api
            .update_snapshot_open_project(&h.tsconfig_norm, &h.server_version),
    )
    .await
    .expect("updateSnapshot timed out")
    .expect("updateSnapshot");

    let project = snap
        .project_for_config(|c| path_eq(c, &h.tsconfig_norm))
        .expect("the configured project is in the snapshot");
    let engine_carrier = engine_carrier_path(project, &carrier_norm).unwrap_or_else(|| {
        panic!(
            "the injected carrier must be a Program root of the configured project; roots: {:?}",
            project.root_files
        )
    });

    let diags = tokio::time::timeout(
        Duration::from_secs(30),
        h.api
            .get_semantic_diagnostics(&snap.snapshot, &project.id, engine_carrier),
    )
    .await
    .expect("getSemanticDiagnostics timed out")
    .expect("getSemanticDiagnostics");

    eprintln!(
        "[mechanics] --api carrier diagnostics codes = {:?}",
        diags.iter().map(|d| d.code).collect::<Vec<_>>()
    );
    // THE PROOF: the deliberate TS2322 surfaces through the attached --api
    // checker — so the carrier injected over the CONTROL protocol reached the
    // REAL tsgo's shared project.Session.
    assert!(
        diags.iter().any(|d| d.code == 2322),
        "the --api checker must see the control-injected carrier's TS2322; got: {diags:?}"
    );
    // NEGATIVE: `./util` resolved — no spurious TS2307 (the carrier is a genuine
    // member of the configured project).
    assert!(
        !diags.iter().any(|d| d.code == 2307),
        "the carrier's ./util import must resolve (no false TS2307); got: {diags:?}"
    );

    teardown(h).await;
}

/// CARRIER-LEAK-LIVE + ID-DEMUX: with a carrier injected + processed by the real
/// tsgo, the FAKE EDITOR never receives any frame carrying the carrier URI/text,
/// never receives a `verter:*`-id frame (the injected barrier/session responses
/// demux to the control side), and an editor-origin `verter:*` request is
/// dropped (never answered) while a normal server response still reaches it.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn relay_suppresses_carrier_leak_and_demuxes_verter_ids_end_to_end() {
    let Some(tsgo) = engine_or_skip().await else {
        return;
    };
    let h = setup(&tsgo, "leakdemux").await;
    let (carrier_norm, carrier_uri, carrier_src) = carrier_fixture(&h.dir);

    // Inject + process the carrier so the real tsgo genuinely holds it (any
    // server→editor frame that would reference it must be suppressed).
    with_timeout(
        "carrierDidOpenSynced",
        h.ctl
            .carrier_did_open_synced(&carrier_uri, "typescript", 1, &carrier_src),
    )
    .await
    .expect("carrier didOpenSynced");
    let snap = with_timeout(
        "updateSnapshot",
        h.api
            .update_snapshot_open_project(&h.tsconfig_norm, &h.server_version),
    )
    .await
    .expect("updateSnapshot");
    if let Some(project) = snap.project_for_config(|c| path_eq(c, &h.tsconfig_norm)) {
        if let Some(engine_carrier) = engine_carrier_path(project, &carrier_norm) {
            let _ = h
                .api
                .get_semantic_diagnostics(&snap.snapshot, &project.id, engine_carrier)
                .await;
        }
    }

    // Drive an editor request whose server response could reference the carrier
    // (a workspace symbol for the carrier's unique export), plus an editor-origin
    // reserved `verter:*` request that MUST be dropped.
    h.editor
        .send(&serde_json::json!({
            "jsonrpc": "2.0", "id": 100, "method": "workspace/symbol",
            "params": { "query": "verterCarrierLeakProbe" },
        }))
        .await;
    h.editor
        .send(&serde_json::json!({
            "jsonrpc": "2.0", "id": "verter:probe", "method": "workspace/symbol",
            "params": { "query": "anything" },
        }))
        .await;
    // Let the server answer + emit any pushed diagnostics.
    let _ = h
        .editor
        .wait_for(|m| m["id"] == 100, Duration::from_secs(15))
        .await;
    h.editor
        .send(&serde_json::json!({
            "jsonrpc": "2.0", "id": 101, "method": "workspace/symbol",
            "params": { "query": "fence" },
        }))
        .await;
    let _ = h
        .editor
        .wait_for(|m| m["id"] == 101, Duration::from_secs(15))
        .await;

    let frames = h.editor.all_frames();
    // (1) No carrier URI / text / basename ever reaches the editor.
    for frame in &frames {
        let text = frame.to_string();
        assert!(
            !text.contains(&carrier_uri)
                && !text.contains("verterCarrierLeakProbe")
                && !text.contains("Carrier.ts"),
            "the carrier leaked to the fake editor: {frame}"
        );
    }
    // (2) No reserved `verter:*` id ever reaches the editor (the injected
    //     barrier / api-session responses demux to the control side).
    for frame in &frames {
        if let Some(id) = frame.get("id").and_then(|v| v.as_str()) {
            assert!(
                !id.starts_with("verter:"),
                "a reserved verter:* id leaked to the editor: {frame}"
            );
        }
    }
    // (3) The editor-origin `verter:*` request is DROPPED — no response ever
    //     comes back for it (a reservation violation, never forwarded).
    let reserved_answer = h
        .editor
        .wait_for(
            |m| m.get("id").and_then(|v| v.as_str()) == Some("verter:probe"),
            Duration::from_millis(400),
        )
        .await;
    assert!(
        reserved_answer.is_none(),
        "an editor-origin verter:* request must be dropped, not answered"
    );
    // (4) Forwarding still works: the editor DID receive the relayed initialize
    //     response (a non-carrier server→editor frame).
    assert!(
        frames.iter().any(|m| m["id"] == 1),
        "the relay must forward non-carrier server frames (the initialize response) to the editor"
    );

    teardown(h).await;
}

/// PROTOCOL-VERSION-MISMATCH: a `verter/hello` with a wrong protocol version
/// fails closed (no attach); a correct-protocol hello on the same endpoint
/// succeeds — the discriminating pair.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn control_hello_wrong_protocol_fails_closed_live() {
    let Some(tsgo) = engine_or_skip().await else {
        return;
    };
    let dir = tempdir("protomismatch");
    let control_dir = dir.join("ctl");
    let session_key = "protomismatch";
    let mut shim = spawn_shim(&tsgo, &control_dir, session_key);
    // The shim advertises independently of any editor init.
    let (_adv_path, adv) = wait_for_advertisement(&control_dir, session_key).await;

    // Wrong protocol → fail closed (an error response, no attach).
    {
        let (read, write) = connect_attach_pipe(&adv.endpoint)
            .await
            .expect("connect control endpoint");
        let conn = JsonRpcConnection::connect(read, write);
        let result = conn
            .request(
                "verter/hello",
                serde_json::json!({
                    "protocol": PROTOCOL_VERSION + 1, "nonce": adv.nonce, "client": "verter_lsp",
                }),
            )
            .await;
        assert!(
            matches!(
                result,
                Err(verter_tsgo_api::error::TsgoApiError::Transport(_))
            ),
            "a wrong protocol version must fail closed (error response), got {result:?}"
        );
        let _ = conn.close().await;
    }

    // Correct protocol on a fresh connection → succeeds (discriminates the gate).
    {
        let mut ctl = ControlClient::connect(&adv.endpoint)
            .await
            .expect("connect control endpoint");
        let hello = ctl
            .hello(&adv.nonce, "verter_lsp")
            .await
            .expect("a correct-protocol hello must succeed");
        assert_eq!(hello.protocol, PROTOCOL_VERSION);
        let _ = ctl.close().await;
    }

    let _ = shim.start_kill();
    let _ = tokio::time::timeout(Duration::from_secs(10), shim.wait()).await;
    let _ = std::fs::remove_dir_all(&dir);
}

/// NON-DESTRUCTIVE DETACH (T3): `verter/detach` retracts Verter's overlays and drops
/// the Verter control pipe ONLY — it must NEVER tear the shim down or kill the shim's
/// OWNED tsgo child (doing so would destroy the editor's own type-checking). Proven
/// LIVE through the real shim + real tsgo: after a detach the shim process is still
/// running AND a FRESH control connection still hellos on the SAME advertised endpoint
/// (a torn-down shim would have removed the advertisement + dropped its listener).
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn verter_detach_is_non_destructive_shim_and_child_survive() {
    let Some(tsgo) = engine_or_skip().await else {
        return;
    };
    let mut h = setup(&tsgo, "detachlive").await;

    // Verter detaches (retract overlays + drop the Verter control pipe).
    with_timeout("detach", h.ctl.detach(true))
        .await
        .expect("detach");
    let _ = h.ctl.close().await;

    // The PROTOCOL FENCE first, and it is also the definitive discriminator:
    // a FRESH control connection hellos successfully on the SAME advertised
    // endpoint. A torn-down shim would have aborted its accept loop and
    // removed the advertisement, so this connect/hello would fail. Checking
    // `try_wait()` before this proves nothing — an erroneous teardown races
    // it in the SAME direction, so a not-yet-exited process reads as alive.
    let mut ctl2 = ControlClient::connect(&h.adv.endpoint)
        .await
        .expect("a fresh control connection after detach — the shim endpoint is still alive");
    let hello = with_timeout("re-hello", ctl2.hello(&h.adv.nonce, "verter_lsp"))
        .await
        .expect("a fresh hello after detach must succeed — the shim was NOT torn down by detach");
    assert_eq!(hello.protocol, PROTOCOL_VERSION);
    let _ = ctl2.close().await;

    // Only now is process liveness meaningful: the shim has demonstrably
    // accepted a connection and completed a protocol round-trip AFTER the
    // detach, so this reads a process that was serving, not one that merely
    // had not exited yet.
    assert!(
        matches!(h.shim.try_wait(), Ok(None)),
        "the shim (and its OWNED tsgo child) must stay ALIVE after a non-destructive verter/detach"
    );

    // Explicit cleanup (the test owns the shim's lifecycle).
    let _ = h.shim.start_kill();
    let _ = tokio::time::timeout(Duration::from_secs(10), h.shim.wait()).await;
    let _ = std::fs::remove_dir_all(&h.dir);
}

/// I1 — RAII child ownership: if the shim's `--lsp` setup fails AFTER the real tsgo
/// child is spawned but BEFORE steady state, the child must be killed + reaped, never
/// orphaned. PORTABLE (runs on every platform with NO real engine): a FAKE tsgo
/// heartbeat child stands in for tsgo, and the setup failure is induced by a
/// `--control-dir` whose parent is a regular file, so the control bind / advertisement
/// write cannot create the directory.
///
/// RED before the guard: the spawned fake tsgo is dropped un-killed on the early `Err`
/// return (`kill_on_drop` is off), so it keeps heart-beating AFTER the shim process
/// exits — an orphan. GREEN: the `ChildSetupGuard` reaps it on the setup failure, so the
/// heartbeat file stops growing once the shim exits, and the shim still exits non-zero.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn setup_failure_after_spawn_kills_fake_tsgo() {
    let dir = tempdir("setupfail");
    // The control_dir's PARENT is a regular FILE, so creating control_dir (the UDS parent
    // dir on Unix / the advertisement dir on Windows) fails — a deterministic setup
    // failure AFTER the child spawn on both platforms.
    let regular_file = dir.join("not_a_dir");
    std::fs::write(&regular_file, b"x").unwrap();
    let bad_control_dir = regular_file.join("nope");
    let heartbeat = dir.join("heartbeat.log");

    let mut shim = Command::new(bin_exe!("verter-relay-shim"))
        .arg("--real-tsgo")
        .arg(bin_exe!("fake_tsgo_heartbeat"))
        .arg("--control-dir")
        .arg(&bad_control_dir)
        .arg("--session-key")
        .arg("setupfail")
        .arg("--")
        .arg("--lsp")
        .arg("--stdio")
        .env("FAKE_TSGO_HEARTBEAT_FILE", &heartbeat)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::inherit())
        .kill_on_drop(true)
        .spawn()
        .expect("spawn the relay shim binary");

    // The shim fails fast on the setup error (after the child spawn) → non-zero exit.
    let status = tokio::time::timeout(Duration::from_secs(20), shim.wait())
        .await
        .expect("the shim must exit promptly on a setup failure (bounded)")
        .expect("await the shim exit status");
    assert!(
        !status.success(),
        "a setup failure after spawn must exit the shim NON-ZERO; got {status:?}"
    );

    // After the shim has exited, the fake tsgo must be DEAD (reaped by the guard), so the
    // heartbeat file stops growing. Sample across the fake's ~30ms beat interval: a
    // still-alive orphan would append several more bytes.
    let sample = |p: &Path| std::fs::metadata(p).map(|m| m.len()).unwrap_or(0);
    let before = sample(&heartbeat);
    tokio::time::sleep(Duration::from_millis(500)).await;
    let after = sample(&heartbeat);
    assert_eq!(
        before, after,
        "the fake tsgo must be reaped on setup failure (no orphan): the heartbeat grew \
         {before}->{after} bytes AFTER the shim exited, so the child was left running"
    );

    let _ = std::fs::remove_dir_all(&dir);
}

/// Sample a heartbeat file's byte length (0 if it does not exist yet).
///
/// Platform-neutral on purpose: the shutdown, orphan, and crash tests that read it are spread across
/// Unix-only, Linux/Windows-only, and fully portable gates, so a cfg-scoped sampler would have to be
/// duplicated per gate.
fn heartbeat_len(path: &Path) -> u64 {
    std::fs::metadata(path).map(|m| m.len()).unwrap_or(0)
}

/// HARD-kill the shim so NEITHER its RAII `Drop` NOR its Unix signal handlers can run — only the
/// OS containment primitive can then reap the OWNED child.
#[cfg(windows)]
fn hard_kill_shim(shim: &mut Child) {
    // `start_kill` maps to `TerminateProcess`: an uncatchable OS kill with no Drop / atexit /
    // handler — exactly the residual orphan window a kill-on-close Job Object closes.
    let _ = shim.start_kill();
}
#[cfg(target_os = "linux")]
fn hard_kill_shim(shim: &mut Child) {
    if let Some(pid) = shim.id() {
        // SAFETY: SIGKILL to a live pid — uncatchable, no handler, no Drop, no cleanup.
        unsafe {
            libc::kill(pid as libc::pid_t, libc::SIGKILL);
        }
    }
}

/// F1(OS) — OS-BACKED CONTAINMENT is the PRIMARY orphan guarantee. A shim that is HARD-killed
/// (`TerminateProcess` on Windows / an uncatchable `SIGKILL` on Linux), so NEITHER its RAII `Drop`
/// NOR its Unix signal handlers can run, must STILL take its OWNED real-tsgo child down with it —
/// via a Windows kill-on-close Job Object / a Linux `PR_SET_PDEATHSIG`. This is the residual orphan
/// window the cooperative RAII + signal paths cannot close.
///
/// Scoped to the two platforms that HAVE that hard kernel primitive. macOS/BSD have no parent-death
/// signal, so a hard-killed shim there can orphan the child until the RAII/handler path runs;
/// asserting the reap on macOS would be a FALSE test, so it is compiled out there — the guarantee on
/// macOS is process-group + RAII (best-effort), documented on `OwnedChild`.
///
/// RED before OS containment (a bare `Child`): the hard-killed shim leaves the fake tsgo running
/// (its heartbeat keeps growing after the shim is gone). GREEN with containment: the OS reaps it.
#[cfg(any(target_os = "linux", windows))]
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn hard_killed_shim_does_not_orphan_real_tsgo() {
    let dir = tempdir("hardkill");
    let control_dir = dir.join("ctl");
    let heartbeat = dir.join("heartbeat.log");

    let mut shim = Command::new(bin_exe!("verter-relay-shim"))
        .arg("--real-tsgo")
        .arg(bin_exe!("fake_tsgo_heartbeat"))
        .arg("--control-dir")
        .arg(&control_dir)
        .arg("--session-key")
        .arg("hardkill")
        .arg("--")
        .arg("--lsp")
        .arg("--stdio")
        .env("FAKE_TSGO_HEARTBEAT_FILE", &heartbeat)
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .stderr(Stdio::inherit())
        .kill_on_drop(true)
        .spawn()
        .expect("spawn the relay shim binary");
    // Hold the shim's stdin OPEN so the relay does not stop on an editor EOF before the kill.
    let _shim_stdin = shim.stdin.take().expect("shim stdin piped");

    // Steady state: the advertisement is published only AFTER the child is spawned + contained, so
    // observing it proves the child is live under containment (and makes this test non-vacuous — a
    // missing fake bin could never advertise and this gate would time out rather than pass hollow).
    let (_adv_path, _adv) = wait_for_advertisement(&control_dir, "hardkill").await;

    // Non-vacuity gate: the fake tsgo must be genuinely alive + beating before the hard kill.
    let warm = heartbeat_len(&heartbeat);
    tokio::time::sleep(Duration::from_millis(200)).await;
    assert!(
        heartbeat_len(&heartbeat) > warm,
        "the fake tsgo must be beating before the hard kill (else the reap assertion is vacuous)"
    );

    // HARD-kill the shim: no Drop, no signal handler — only the OS primitive can reap the child.
    hard_kill_shim(&mut shim);
    // Reap the shim so the OS releases its handles (Windows closes the last job handle → the job's
    // KILL_ON_JOB_CLOSE reaps the child; Linux delivers the child's PDEATHSIG on the shim's death).
    let _ = tokio::time::timeout(Duration::from_secs(10), shim.wait()).await;
    // Let the OS finish reaping the contained child before sampling.
    tokio::time::sleep(Duration::from_millis(300)).await;

    // THE guarantee: the OS reaped the OWNED child, so the heartbeat stops growing once the shim is
    // gone. Sample across several ~30ms beat intervals.
    let before = heartbeat_len(&heartbeat);
    tokio::time::sleep(Duration::from_millis(700)).await;
    let after = heartbeat_len(&heartbeat);
    assert_eq!(
        before, after,
        "a HARD-killed shim must not orphan the real tsgo: the heartbeat grew {before}->{after} \
         bytes after the shim was hard-killed, so the OS did not reap the OWNED child"
    );

    let _ = std::fs::remove_dir_all(&dir);
}

/// signal (D3) — faithful Unix signal-exit + no orphan on a signal delivered to the SHIM:
/// a SIGTERM to a running shim must kill + reap its OWNED tsgo child (no orphan) and then
/// re-raise the signal so the shim itself exits VIA SIGTERM. UNIX-ONLY (POSIX signals);
/// cfg-compiled-out on Windows.
///
/// READINESS GATE (F8): the test waits for the shim's ADVERTISEMENT before signalling. That
/// gate is SOUND because the shim installs its shutdown handlers BEFORE it spawns the child —
/// and long before the advertisement is published — so an observed advertisement
/// deterministically implies the handlers are live. A signal delivered from this point is caught
/// (buffered by tokio) and drives the guarded teardown; it can never slip through an unhandled
/// setup-signal gap, and there is no spawn→install window in which the child could be orphaned.
///
/// RED before the signal handlers: the shim had NO SIGTERM handler, so SIGTERM's default
/// action killed the shim WITHOUT cleanup — orphaning the fake tsgo (its heartbeat keeps
/// growing after the shim exits). GREEN: the handler reaps the child (heartbeat stops) and
/// re-raises SIGTERM (the shim exits via the signal, never a masked clean exit).
#[cfg(unix)]
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn shim_sigterm_reaps_owned_child_and_reraises_the_signal() {
    use std::os::unix::process::ExitStatusExt;

    let dir = tempdir("sigterm");
    let control_dir = dir.join("ctl");
    let heartbeat = dir.join("heartbeat.log");

    let mut shim = Command::new(bin_exe!("verter-relay-shim"))
        .arg("--real-tsgo")
        .arg(bin_exe!("fake_tsgo_heartbeat"))
        .arg("--control-dir")
        .arg(&control_dir)
        .arg("--session-key")
        .arg("sigterm")
        .arg("--")
        .arg("--lsp")
        .arg("--stdio")
        .env("FAKE_TSGO_HEARTBEAT_FILE", &heartbeat)
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .stderr(Stdio::inherit())
        .kill_on_drop(true)
        .spawn()
        .expect("spawn the relay shim binary");
    // Hold the shim's stdin OPEN so the relay does not stop on an editor EOF before we
    // signal it (a null stdin would tear the relay down immediately).
    let _shim_stdin = shim.stdin.take().expect("shim stdin piped");

    // Readiness gate: the advertisement is published AFTER the shutdown handlers are
    // installed (F1), so observing it proves the handlers are live — SIGTERM from here is
    // caught + reaped, never dropped through an unhandled setup-signal gap.
    let (_adv_path, _adv) = wait_for_advertisement(&control_dir, "sigterm").await;
    let pid = shim.id().expect("the shim has a pid") as libc::pid_t;

    // Deliver SIGTERM to the shim.
    // SAFETY: kill(2) with a live pid + a valid signal number.
    let rc = unsafe { libc::kill(pid, libc::SIGTERM) };
    assert_eq!(rc, 0, "kill(shim, SIGTERM) must succeed");

    // The shim must exit, faithfully reporting the signal (never masked as a clean exit).
    let status = tokio::time::timeout(Duration::from_secs(15), shim.wait())
        .await
        .expect("the shim must exit after SIGTERM (bounded)")
        .expect("await the shim exit status");
    assert_eq!(
        status.signal(),
        Some(libc::SIGTERM),
        "the shim must exit VIA SIGTERM (faithful signal-exit); got {status:?}"
    );
    assert!(
        !status.success(),
        "a signal-terminated shim is never a success exit; got {status:?}"
    );

    // THE fix: the OWNED child was reaped, not orphaned — the heartbeat stops growing once
    // the shim has exited. Pre-fix the shim was killed by SIGTERM's default action without
    // cleanup, orphaning the child (heartbeat keeps growing).
    let before = heartbeat_len(&heartbeat);
    tokio::time::sleep(Duration::from_millis(500)).await;
    let after = heartbeat_len(&heartbeat);
    assert_eq!(
        before, after,
        "SIGTERM must reap the OWNED child (no orphan): the heartbeat grew {before}->{after} \
         bytes after the shim exited, so the child was left running"
    );

    let _ = std::fs::remove_dir_all(&dir);
}

/// The fixture source, baked in at the moment THIS TEST BINARY was compiled. Hashing it and
/// comparing against the fixture binary's own baked-in copy is what detects a MISMATCHED fixture
/// binary (see `fake_tsgo_fixture_binary_matches_its_source`). The `include_str!` earns its keep by
/// keeping the EXPECTED hash fresh — it makes the fixture source a build input of THIS test target,
/// so the test side can never be the stale one. It does NOT (and was measured not to) create any
/// test→bin rebuild edge; see the guard below.
const FIXTURE_SOURCE: &str = include_str!("../support/fake_tsgo_heartbeat.rs");

// The freshness hash — ONE definition, shared with the fixture bin by `include!` so the two sides
// cannot drift apart.
include!("../support/fnv1a64.rs");

/// KNOWN-ANSWER TEST for the shared freshness hash: it must be the algorithm it says it is.
///
/// A shared definition guarantees the two sides AGREE; it does not guarantee they agree on FNV-1a.
/// A wrong constant is invisible to `fake_tsgo_fixture_binary_matches_its_source` (both sides
/// `include!` the same code, so they drift together), and it stays invisible until someone
/// cross-checks the "FNV-1a, 64-bit" claim against a reference implementation and finds it false.
/// This pins it to the PUBLISHED vectors instead: the empty input yields the offset basis, and
/// `"hello"` yields `a430d84680aabd0b`. Mutating either constant, or swapping the XOR/multiply
/// order, turns this RED.
#[test]
fn fnv1a64_matches_the_published_fnv1a_64_vectors() {
    assert_eq!(
        fnv1a64(b""),
        0xcbf2_9ce4_8422_2325,
        "the empty input must yield the FNV-1a 64 OFFSET BASIS unchanged"
    );
    assert_eq!(
        fnv1a64(b"hello"),
        0xa430_d846_80aa_bd0b,
        "the shared freshness hash must be real FNV-1a 64 (published vector for \"hello\"), not a \
         look-alike with an off-by-one-nibble prime"
    );
}

/// FIXTURE-MISMATCH GUARD — the PRIMARY protection that the `fake_tsgo_heartbeat` binary the live
/// tests below spawn was built from the fixture source in THIS tree.
///
/// The hazard is a verification-integrity one, not an inconvenience: a fixture binary that does not
/// match the tree can satisfy assertions for reasons unrelated to the code under test or — worse —
/// fail to crash or beat at all and make the orphan/crash assertions pass VACUOUSLY. That vacuity
/// was empirically observed while diagnosing this suite.
///
/// WHAT IT CHECKS, exactly: it runs `--fixture-source-hash` on the binary resolved by `bin_exe!`,
/// and compares against this test target's own `include_str!` copy of the fixture source. Because
/// EVERY live test in this file launches the fixture through that same `bin_exe!` resolution, the
/// binary this guard interrogates is — structurally, not by coincidence — the binary those tests
/// spawn. That is the whole claim, and it is what an earlier version of this comment got wrong: it
/// asserted coverage of "a `nextest archive` extracted on another machine" while every launch here
/// still used the COMPILE-TIME `env!("CARGO_BIN_EXE_*")`, which bakes in the build machine's
/// `target/` path and therefore validated a binary the relocated run would not have executed. The
/// runtime resolution documented on `bin_exe!` is what closed that gap.
///
/// MEASURED — `cargo nextest archive`, extracted and run with the ORIGINAL `target/debug/`
/// fixture binary renamed away: the run resolves `NEXTEST_BIN_EXE_fake_tsgo_heartbeat` to the
/// EXTRACTED copy and this guard passes against it; with the compile-time `env!` the same run fails
/// to even spawn (the baked path no longer exists). So on a relocated archive the guard now
/// validates the binary that actually runs.
///
/// Also measured: cargo DOES rebuild this sibling `[[bin]]` on a plain
/// `cargo test -p verter_relay_shim --test main` when its source changes — binary targets are built
/// automatically for a selected integration test (that is why `CARGO_BIN_EXE_*` exists), and the bin
/// rebuilds because `tests/support/fake_tsgo_heartbeat.rs` is its OWN declared `path`. Editing the
/// fixture and rebuilding changes the on-disk binary with the `include_str!` above REMOVED, so that
/// rebuild is cargo's automatic bin build and owes nothing to any include edge.
///
/// So this test is not a backstop to a build-ordering guarantee; it IS the guarantee for the cases
/// cargo's rebuild does not cover: a test binary run WITHOUT cargo (a `target/debug/deps/main-*`
/// invoked directly), a binary replaced out-of-band or staged from elsewhere, or one uplifted from a
/// different profile/target dir. In all of those there is no cargo fingerprint in the loop at all.
/// Both sides hash the SAME file — the fixture at ITS compile time, this test at ITS OWN — so a
/// mismatched binary carries the wrong hash and fails LOUDLY here instead of corrupting the suite's
/// meaning.
///
/// What it deliberately does NOT claim: it does not verify the SHIM binary (only the fixture); it
/// does not detect a fixture whose source matches but whose dependencies/toolchain differ; and the
/// archive result above was measured on this machine with a relocated extraction, not on a second
/// machine or a second OS — the mechanism (a runtime-supplied path) is the same, but only the
/// same-machine relocation was executed.
///
/// Discriminates: overwriting the built fixture with one compiled from a different revision of its
/// source makes THIS test fail with both hashes, while every assertion-bearing live test still
/// "passes" against the wrong fixture. Proving that requires bypassing cargo (which would re-uplift
/// the correct binary from its fingerprinted `deps/` cache and silently undo the plant): overwrite
/// `target/debug/fake_tsgo_heartbeat` and run the already-built test binary DIRECTLY.
#[test]
fn fake_tsgo_fixture_binary_matches_its_source() {
    let output = std::process::Command::new(bin_exe!("fake_tsgo_heartbeat"))
        .arg("--fixture-source-hash")
        .output()
        .expect("run the fake tsgo fixture with --fixture-source-hash");
    assert!(
        output.status.success(),
        "--fixture-source-hash must exit 0; got {:?} (stderr: {:?})",
        output.status,
        String::from_utf8_lossy(&output.stderr)
    );
    let printed = String::from_utf8_lossy(&output.stdout).trim().to_string();
    let expected = format!(
        "FAKE_TSGO_FIXTURE_SOURCE_HASH:{:016x}",
        fnv1a64(FIXTURE_SOURCE.as_bytes())
    );
    assert_eq!(
        printed, expected,
        "the fake_tsgo_heartbeat BINARY does not match this tree: it was built from a different \
         revision of tests/support/fake_tsgo_heartbeat.rs than this test compiled against, so every \
         live test in this file is running a fixture that does not match the code under test. \
         Rebuild it with `cargo build -p verter_relay_shim --bins`."
    );
}

/// A path under `dir` whose last component is provably NOT valid UTF-8.
///
/// Unix paths are byte strings, so `0xff` is a legal path byte and an OS-legal environment value —
/// but it is not valid UTF-8, which is exactly the input `std::env::var` refuses (`NotUnicode`) and
/// `.ok()` then silently degrades into "unset". The returned path is only ever used as an
/// environment VALUE the fixture must decode; nothing requires the filesystem to accept the name
/// (macOS's APFS rejects non-UTF-8 names outright, so a test that needed one on disk could not run
/// here at all).
///
/// The `to_str().is_none()` assertion is the plant-applied proof: a "non-UTF-8" path that quietly
/// came out valid would make every test below pass for the wrong reason.
///
/// Coverage note: these probes are Unix-only. Windows can express the same hazard — an unpaired
/// surrogate built with `OsStringExt::from_wide` is an OS-legal value that `std::env::var` refuses
/// — and no test here constructs one, so the Windows half of the decoding contract is held by the
/// shared `var_os`/`args_os` implementation rather than by its own assertions.
#[cfg(unix)]
fn non_utf8_path(dir: &Path, stem: &str) -> PathBuf {
    use std::os::unix::ffi::OsStringExt;
    let mut bytes = dir.join(stem).into_os_string().into_vec();
    bytes.extend_from_slice(b".\xff");
    let path = PathBuf::from(std::ffi::OsString::from_vec(bytes));
    assert!(
        path.to_str().is_none(),
        "the probe path must be genuinely non-UTF-8, or the test proves nothing; got {path:?}"
    );
    path
}

/// A path-valued environment variable that is NOT valid UTF-8 must not KILL the fixture.
///
/// `std::env::var(..).expect(..)` panics on a `NotUnicode` value, so a non-UTF-8 `TMPDIR` — an
/// OS-legal configuration on Linux — would take the fixture down in its first statement, before it
/// ever beats. Every live test in this file then fails through a vacuous "the child never started"
/// symptom that names no cause. The fixture must decode the value as an `OsString`, so the same
/// value simply becomes the path it was given.
///
/// Assertion scope: liveness only. Whether the WRITE at that path succeeds is filesystem policy
/// (APFS refuses non-UTF-8 names), and the fixture already tolerates a failed open. What must not
/// happen is the process dying on the decode.
#[cfg(unix)]
#[test]
fn a_non_utf8_heartbeat_path_does_not_kill_the_fixture() {
    let dir = ScopedTempDir::new("nonutf8_heartbeat");
    let heartbeat = non_utf8_path(dir.path(), "heartbeat");

    let mut child = std::process::Command::new(bin_exe!("fake_tsgo_heartbeat"))
        .env("FAKE_TSGO_HEARTBEAT_FILE", &heartbeat)
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .spawn()
        .expect("spawn the fake tsgo fixture");

    std::thread::sleep(Duration::from_millis(500));
    let died = child.try_wait().expect("poll the fixture");
    let _ = child.kill();
    let output = child.wait_with_output().expect("reap the fixture");
    let stderr = String::from_utf8_lossy(&output.stderr).into_owned();

    assert!(
        died.is_none(),
        "the fixture exited ({died:?}) instead of running with a non-UTF-8 heartbeat path: it \
         forced an OS path through a Unicode string. --- fixture stderr ---\n{stderr}"
    );
}

/// A non-UTF-8 TRIGGER path must ARM the fixture, never silently disarm it.
///
/// This is the failure mode that hurts most: `std::env::var(..).ok()` maps `NotUnicode` onto the
/// same `None` as "unset", so a trigger the parent test provably set reads back as absent. Each
/// trigger family below is deliberately FAIL-CLOSED on a half-configuration, so on the broken decode
/// the fixture panics ("… is set without …") and never beats; on the correct `var_os` decode it arms
/// and beats. The heartbeat is therefore the observable: growth proves the fixture got PAST trigger
/// resolution with the non-UTF-8 value accepted.
///
/// The trigger file itself is never created, so neither trigger can fire — the fixture beats until
/// this test kills it, and the test measures arming, not firing. Firing is not observable at all
/// here: macOS refuses to create a file whose name is not valid UTF-8 (`EILSEQ`), so a trigger path
/// this test can prove is non-Unicode is a path the filesystem will not let it touch.
///
/// The observable is therefore SUSTAINED beating plus a live process — not a first byte. That
/// distinction is what makes the assertion discriminating rather than decorative:
///
/// - A fixture that reads the trigger path through a Unicode string sees `NotUnicode` as "unset",
///   trips the fail-closed half-configuration check, and dies before its first beat.
/// - A fixture that honours the signal variable but IGNORES the trigger file — resolving the crash
///   from its own clock instead of an event — raises the signal on itself a few beats in and is
///   gone long before the threshold. A first-beat assertion cannot see that; this one does.
///
/// That second case reads a DEATH as its evidence, so it would be worthless if the raise could
/// silently fail to kill. It cannot: the fixture restores the signal's default disposition and
/// unblocks it immediately before raising, so an inherited `SIG_IGN` or blocked mask — both of which
/// survive `exec`, and either of which a supervisor or a `trap '' TERM` shell can hand this process
/// — cannot keep the fixture alive past the threshold.
#[cfg(unix)]
#[test]
fn non_utf8_trigger_paths_arm_the_fixture_instead_of_disarming_it() {
    for (label, code_var, code_value, file_var) in [
        (
            "crash (raise) trigger",
            "FAKE_TSGO_RAISE_SIGNAL",
            libc::SIGTERM.to_string(),
            "FAKE_TSGO_RAISE_WHEN_FILE",
        ),
        (
            "portable exit trigger",
            "FAKE_TSGO_EXIT_CODE",
            "7".to_string(),
            "FAKE_TSGO_EXIT_WHEN_FILE",
        ),
    ] {
        let dir = ScopedTempDir::new("nonutf8_trigger");
        // The heartbeat path is ordinary: only the TRIGGER path is non-UTF-8, so a failure can only
        // come from the trigger's decode.
        let heartbeat = dir.path().join("heartbeat.log");
        let trigger = non_utf8_path(dir.path(), "trigger");

        let mut child = std::process::Command::new(bin_exe!("fake_tsgo_heartbeat"))
            .env("FAKE_TSGO_HEARTBEAT_FILE", &heartbeat)
            .env(code_var, &code_value)
            .env(file_var, &trigger)
            .stdout(Stdio::null())
            .stderr(Stdio::piped())
            .spawn()
            .expect("spawn the fake tsgo fixture");

        // One byte per beat, measured at ~40ms each, so this threshold is ten beats — about 0.4s
        // against the 10s deadline below. It sits well past the point at which a fixture resolving
        // its trigger from its own clock would already have fired, and well short of the deadline a
        // healthy one needs. A fixture that DIES ends the wait immediately rather than sitting out
        // that deadline, so a genuine regression fails fast and names the exit status.
        const SUSTAINED_BEATS: u64 = 10;
        let deadline = std::time::Instant::now() + Duration::from_secs(10);
        let mut beat = 0;
        let mut died = None;
        while std::time::Instant::now() < deadline {
            beat = heartbeat_len(&heartbeat);
            died = child.try_wait().expect("poll the fixture");
            if died.is_some() || beat >= SUSTAINED_BEATS {
                break;
            }
            std::thread::sleep(Duration::from_millis(10));
        }
        let _ = child.kill();
        let output = child.wait_with_output().expect("reap the fixture");
        let stderr = String::from_utf8_lossy(&output.stderr).into_owned();

        assert!(
            beat >= SUSTAINED_BEATS,
            "the {label} was configured with a non-UTF-8 path and the fixture beat {beat} times, \
             short of {SUSTAINED_BEATS} (exit: {died:?}): either the value was decoded through a \
             Unicode string — reading back as UNSET, so the fail-closed half-configuration check \
             killed the fixture — or the trigger fired from the fixture's own clock rather than \
             waiting on the file it was given. --- fixture stderr ---\n{stderr}"
        );
        assert!(
            died.is_none(),
            "the {label} was armed on a trigger file that was never created, so the fixture must \
             still be running; it exited ({died:?}) instead, which means the trigger did not wait \
             on the path it was handed. --- fixture stderr ---\n{stderr}"
        );
    }
}

/// A non-Unicode ARGV item must not kill the fixture — argv is the OTHER OS-supplied byte channel.
///
/// `std::env::args` PANICS during iteration on an argument that is not valid Unicode. This fixture
/// stands in as the relay shim's engine, so its argv is whatever the editor forwarded, including
/// paths the editor chose; a single non-Unicode byte anywhere in it would abort the fixture in its
/// first statement, and every live test in this file would then fail through a "the child never
/// started" symptom that names no cause. `args_os` hands the bytes back unchanged.
///
/// Observed through the `--fixture-source-hash` probe because that is the one fixture path which
/// scans argv AND produces output: the probe must still find its own flag past the non-Unicode
/// item, and print the hash.
#[cfg(unix)]
#[test]
fn a_non_utf8_argv_item_does_not_kill_the_fixture() {
    use std::os::unix::ffi::OsStringExt;

    // The plant-applied proof, as with the path probes: an argument that quietly came out valid
    // would make this pass for the wrong reason.
    let non_unicode = std::ffi::OsString::from_vec(b"--\xff".to_vec());
    assert!(
        non_unicode.to_str().is_none(),
        "the probe argument must be genuinely non-Unicode, or the test proves nothing"
    );

    // Ahead of the probe flag, so a fixture that walks argv through `String` dies before reaching
    // it rather than short-circuiting past it.
    let output = std::process::Command::new(bin_exe!("fake_tsgo_heartbeat"))
        .arg(&non_unicode)
        .arg("--fixture-source-hash")
        .output()
        .expect("run the fake tsgo fixture with a non-Unicode argv item");
    let stderr = String::from_utf8_lossy(&output.stderr).into_owned();

    assert!(
        output.status.success(),
        "the fixture failed ({:?}) on a non-Unicode argv item: it walked argv through a Unicode \
         string, which panics on exactly the bytes an editor is free to forward. \
         --- fixture stderr ---\n{stderr}",
        output.status
    );
    assert_eq!(
        String::from_utf8_lossy(&output.stdout).trim(),
        format!(
            "FAKE_TSGO_FIXTURE_SOURCE_HASH:{:016x}",
            fnv1a64(FIXTURE_SOURCE.as_bytes())
        ),
        "the probe must still find its flag past a non-Unicode argv item and print the hash. \
         --- fixture stderr ---\n{stderr}"
    );
}

/// Gate on the fake tsgo being ALIVE and beating: the heartbeat file exists and is non-empty.
///
/// This is the readiness anchor the signal-crash tests use in place of a wall-clock warm-up. It
/// absorbs the child's fork+exec+dyld latency (~112ms measured here, and worse under parallel
/// load) so the crash the test subsequently triggers is timed from a point BOTH sides can
/// observe, rather than from the child's own `main()` which the shim's grace window cannot see.
async fn wait_for_child_alive(heartbeat: &Path) {
    let deadline = std::time::Instant::now() + Duration::from_secs(20);
    while std::time::Instant::now() < deadline {
        if heartbeat_len(heartbeat) > 0 {
            return;
        }
        tokio::time::sleep(Duration::from_millis(5)).await;
    }
    panic!(
        "the fake tsgo never wrote a heartbeat at {}: it is not alive, so any crash assertion \
         downstream would be vacuous",
        heartbeat.display()
    );
}

/// Block until the shim's steady-state teardown select has RESOLVED, witnessed by the removal of
/// its advertisement.
///
/// `run_relay` removes the advertisement at exactly ONE place in the whole binary: immediately
/// after the teardown select yields, before it dispatches on the chosen arm. So the file's
/// disappearance is a happens-after witness for "an arm has been chosen" — which is what lets a
/// test create an effect that is ORDERED strictly after the choice, instead of racing it.
#[cfg(unix)]
async fn wait_for_teardown_selected(advertisement: &Path) {
    let deadline = std::time::Instant::now() + Duration::from_secs(20);
    while std::time::Instant::now() < deadline {
        if !advertisement.exists() {
            return;
        }
        // Poll finely: the witness is consumed to order work inside the shim's 200ms grace window,
        // so observation latency is budget spent out of that window.
        tokio::time::sleep(Duration::from_millis(1)).await;
    }
    panic!(
        "the shim never removed its advertisement at {}: its teardown select never resolved, so \
         any assertion about which teardown arm ran would be vacuous",
        advertisement.display()
    );
}

/// THE PORTABLE regression test for the runtime shutdown fix (`drop(runtime)` →
/// `shutdown_background()`): a shim whose engine exits while the EDITOR'S STDIN IS STILL OPEN must
/// still terminate, and with the engine's exit code.
///
/// WHY IT EXISTS SEPARATELY. The two tests that already discriminate that fix —
/// `shim_sigterm_reaps_owned_child_and_reraises_the_signal` and
/// `child_signal_exit_is_faithfully_reraised_not_masked_as_success` — are `#[cfg(unix)]` and cannot
/// be otherwise: one delivers a POSIX signal with `kill(2)`, the other asserts `status.signal()`.
/// Both are compiled out on Windows, so before this test a Windows-only regression restoring
/// `drop(runtime)` would have evaded every Windows gate. The production comment claims the hazard is
/// cross-platform ("the editor-stdin read is a blocking-pool read on Windows too"); that claim now
/// has a test on the platform it names.
///
/// The mechanism is what makes it portable: the engine's death is a NORMAL EXIT with a chosen code
/// (`FAKE_TSGO_EXIT_WHEN_FILE`, see the fixture), not a signal. Nothing about the assertion — the
/// shim exits, bounded, with the child's code — is Unix-shaped.
///
/// WHAT IT DISCRIMINATES, exactly. The editor stdin is held OPEN for the whole test, so
/// `tokio::io::stdin()`'s `spawn_blocking` `read(2)` is still parked on the blocking pool when the
/// child exits. A blocking runtime teardown waits for that thread, which an idle-but-open editor
/// stdin never releases, so the shim wedges with its exit already decided. MEASURED here by planting
/// `drop(runtime);` in place of `runtime.shutdown_background();` in `main`: this test blows its 15s
/// bound (`Elapsed`), and passes in 0.34s unplanted.
///
/// The exit CODE, not merely the exit, is asserted: a shim that terminated by some other route (or
/// masked the engine's status) would still "exit", and this test would then be pinning liveness
/// alone.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn child_exit_with_editor_stdin_open_still_exits_the_shim() {
    let dir = ScopedTempDir::new("childexit");
    let control_dir = dir.path().join("ctl");
    let heartbeat = dir.path().join("heartbeat.log");
    let exit_trigger = dir.path().join("exit.trigger");
    // A distinctive, non-zero, non-default code: neither the shim's own success (0), its usage error
    // (2), nor its internal-failure code (1), so a masked or self-attributed exit cannot masquerade
    // as the engine's.
    const ENGINE_EXIT_CODE: i32 = 7;

    let mut shim = Command::new(bin_exe!("verter-relay-shim"))
        .arg("--real-tsgo")
        .arg(bin_exe!("fake_tsgo_heartbeat"))
        .arg("--control-dir")
        .arg(&control_dir)
        .arg("--session-key")
        .arg("childexit")
        .arg("--")
        .arg("--lsp")
        .arg("--stdio")
        .env("FAKE_TSGO_HEARTBEAT_FILE", &heartbeat)
        // The engine exits NORMALLY the moment the trigger file appears — the portable stand-in for
        // an engine that terminates on its own, anchored on an event this test causes rather than a
        // wall-clock delay racing the child's exec latency.
        .env("FAKE_TSGO_EXIT_CODE", ENGINE_EXIT_CODE.to_string())
        .env("FAKE_TSGO_EXIT_WHEN_FILE", &exit_trigger)
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .stderr(Stdio::inherit())
        .kill_on_drop(true)
        .spawn()
        .expect("spawn the relay shim binary");
    // THE condition under test: the editor's stdin stays OPEN for the rest of the test, so the
    // shim's blocking-pool stdin read is still parked when the child exits. Dropping this handle
    // would close the wedge and make the assertion below pass even with the blocking teardown.
    let _shim_stdin = shim.stdin.take().expect("shim stdin piped");

    // Readiness: the engine is genuinely up and beating, so an exit assertion cannot pass for the
    // trivial reason that the child never ran.
    wait_for_child_alive(&heartbeat).await;
    std::fs::write(&exit_trigger, b"exit").expect("write the exit trigger");

    let status = tokio::time::timeout(Duration::from_secs(15), shim.wait())
        .await
        .expect(
            "the shim must exit after its engine exited, even with the editor's stdin still OPEN: \
             a runtime teardown that WAITS on the blocking pool parks forever on that idle read, so \
             the shim hangs with its exit already decided",
        )
        .expect("await the shim exit status");
    assert_eq!(
        status.code(),
        Some(ENGINE_EXIT_CODE),
        "the shim must exit with its engine's exit code; got {status:?}"
    );
}

/// signal (D3) — faithful propagation of the CHILD's signal-exit: if the real tsgo dies
/// from a signal (an engine crash), the shim must re-raise that signal rather than mask it
/// as a clean success. UNIX-ONLY; cfg-compiled-out on Windows.
///
/// The editor stdin is held OPEN for the whole test, so the relay never stops and the shim
/// observes the crash through the CHILD-EXIT arm of the teardown select.
///
/// RED before the single-status-owner teardown: the child-exit arm did `let _ = status`
/// and the teardown returned `Ok(())`, so the shim exited with code 0 — a MASKED success
/// that hid the engine crash. GREEN: `ShimExit::from_status` maps the child's signal-exit
/// to `ShimExit::Signal`, which `main` re-raises, so the shim exits via that signal.
#[cfg(unix)]
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn child_signal_exit_is_faithfully_reraised_not_masked_as_success() {
    use std::os::unix::process::ExitStatusExt;

    let dir = tempdir("childsig");
    let control_dir = dir.join("ctl");
    let heartbeat = dir.join("heartbeat.log");
    let crash_trigger = dir.join("crash.trigger");

    let mut shim = Command::new(bin_exe!("verter-relay-shim"))
        .arg("--real-tsgo")
        .arg(bin_exe!("fake_tsgo_heartbeat"))
        .arg("--control-dir")
        .arg(&control_dir)
        .arg("--session-key")
        .arg("childsig")
        .arg("--")
        .arg("--lsp")
        .arg("--stdio")
        .env("FAKE_TSGO_HEARTBEAT_FILE", &heartbeat)
        // The fake tsgo raises SIGTERM on ITSELF the moment the trigger file appears — an
        // engine crash on an event this test causes, not a wall-clock delay racing exec latency.
        .env("FAKE_TSGO_RAISE_SIGNAL", libc::SIGTERM.to_string())
        .env("FAKE_TSGO_RAISE_WHEN_FILE", &crash_trigger)
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .stderr(Stdio::inherit())
        .kill_on_drop(true)
        .spawn()
        .expect("spawn the relay shim binary");
    // Hold stdin open so the shim observes the CHILD exit (not an editor-EOF relay stop).
    let _shim_stdin = shim.stdin.take().expect("shim stdin piped");

    // Readiness: the engine is genuinely up and beating before we crash it, so the assertion
    // below cannot pass for the trivial reason that the child never ran.
    wait_for_child_alive(&heartbeat).await;
    std::fs::write(&crash_trigger, b"crash").expect("write the crash trigger");

    // The child dies from SIGTERM → the shim must faithfully re-raise it, not report code 0.
    let status = tokio::time::timeout(Duration::from_secs(15), shim.wait())
        .await
        .expect("the shim must exit after the child's signal-death (bounded)")
        .expect("await the shim exit status");
    assert_eq!(
        status.signal(),
        Some(libc::SIGTERM),
        "a child that dies from SIGTERM must be faithfully re-raised by the shim, never \
         masked as a clean exit; got {status:?}"
    );
    assert!(
        !status.success(),
        "the shim must NOT report success when its engine was signal-killed; got {status:?}"
    );

    let _ = std::fs::remove_dir_all(&dir);
}

/// F7(b) — an editor disconnect (relay stop) that COINCIDES with an engine crash must NOT be
/// masked as a clean `Code(0)` shim exit. The teardown grace-check must reap the crashed child
/// and propagate ITS signal, never assume the relay stop was a clean disconnect and return
/// `Code(0)`. UNIX-ONLY; cfg-compiled-out on Windows.
///
/// SCOPE — what this test is and is NOT evidence for. It is a CONTRACT test for the relay-stop
/// grace arm: it pins that arm's mapping and nothing else. It is NOT evidence for the runtime
/// shutdown change in `main` (`drop(runtime)` → `shutdown_background()`), even though the two
/// travelled together: this test drops the editor stdin BEFORE it triggers the crash, so the old
/// blocking runtime drop had no wedged editor-stdin `read(2)` to wait on and this exit path was
/// already reachable. The tests that DO discriminate that shutdown change are
/// `shim_sigterm_reaps_owned_child_and_reraises_the_signal` and
/// `child_signal_exit_is_faithfully_reraised_not_masked_as_success` — both hold the editor stdin
/// OPEN across the shim's exit, so before the fix they wedge and blow their 15s bound (measured:
/// 15.39s and 15.45s, both `Elapsed`), and after it they finish in 0.72s and 0.33s. Under that same
/// plant THIS test stays GREEN — which is the whole point of recording the split here.
///
/// This test pins ONE production expression — the `Some(status) => ShimExit::from_status(status)`
/// arm of `Teardown::RelayStopped => match grace_check_child_exit(…)`. Reaching it is not enough:
/// three different teardown paths can end in the same signal-exit, so the test has to make the
/// other two UNREACHABLE rather than merely unlikely. It does that twice over.
///
/// (1) ORDERING excludes the `ChildExited` arm — structurally, with no timing assumption. The
/// steady-state select is BIASED (`Signal`, then `ChildExited`, then `RelayStopped`), so a child
/// that is already dead when the disconnect arrives is observed as a child exit, and that arm maps
/// the status through its OWN `from_status` call which the plant does not touch. The fixture exits
/// only when the crash trigger appears, and this test creates that trigger strictly AFTER
/// `wait_for_teardown_selected` observes the advertisement gone — i.e. after the select has already
/// yielded. At the moment of the choice the child therefore cannot have exited, `child.wait()`
/// cannot have been ready, and no signal was sent to the shim: `RelayStopped` is the only arm the
/// select can have taken.
///
/// (2) The CRASH SIGNAL excludes the post-grace arm — again observably, not by timing luck. The
/// fixture self-raises `SIGKILL`, and `SIGKILL` is the one status the two relay-stop arms map
/// DIFFERENTLY: inside the grace window the shim has not killed anything, so `from_status`
/// propagates `Signal(SIGKILL)` and the shim dies of `SIGKILL`; past the window the shim owns the
/// kill and `shim_exit_after_relay_stop_kill` deliberately attributes a bare `SIGKILL` to ITSELF,
/// yielding `Code(0)`. So `status.signal() == SIGKILL` below is reachable through the grace-`Some`
/// arm and NOTHING else — and if the crash ever did land late, this test FAILS LOUDLY (it would see
/// the `Code(0)` disconnect exit) instead of passing through a path it did not mean to test. A
/// `SIGTERM` fixture cannot make that distinction: all three paths propagate it identically.
///
/// The mutation that must turn this RED, and does: replace that arm's body with `ShimExit::Code(0)`
/// — the historical masking bug. The sibling `child_signal_exit_is_faithfully_reraised_…` stays
/// GREEN under the same plant, which is what makes this test's coverage its OWN and not a duplicate.
///
/// TIMING BUDGET — the crash is anchored on an event, never a delay. An earlier wall-clock fixture
/// countdown ran from the CHILD's `main()`, silently including fork+exec+dyld latency (~112ms here,
/// worse under parallel load), and pushed the crash past the window; do not reintroduce a
/// sleep-based anchor. The trigger is now written after the advertisement is observed gone, which
/// `run_relay` does BEFORE entering `grace_check_child_exit`, so the write happens essentially at
/// the window's opening edge and the fixture's ≤30ms trigger poll consumes a small fraction of the
/// 200ms.
#[cfg(unix)]
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn relay_stop_with_crashed_child_propagates_child_signal_not_code_zero() {
    use std::os::unix::process::ExitStatusExt;

    let dir = tempdir("relaycrash");
    let control_dir = dir.join("ctl");
    let heartbeat = dir.join("heartbeat.log");
    let crash_trigger = dir.join("crash.trigger");

    let mut shim = Command::new(bin_exe!("verter-relay-shim"))
        .arg("--real-tsgo")
        .arg(bin_exe!("fake_tsgo_heartbeat"))
        .arg("--control-dir")
        .arg(&control_dir)
        .arg("--session-key")
        .arg("relaycrash")
        .arg("--")
        .arg("--lsp")
        .arg("--stdio")
        .env("FAKE_TSGO_HEARTBEAT_FILE", &heartbeat)
        // The fake tsgo crashes with SIGKILL the moment the trigger file appears — created BELOW,
        // strictly after the shim's teardown select has resolved. SIGKILL is the ARM DISCRIMINATOR
        // (see the doc comment): the grace arm propagates it, the post-grace arm reports Code(0).
        .env("FAKE_TSGO_RAISE_SIGNAL", libc::SIGKILL.to_string())
        .env("FAKE_TSGO_RAISE_WHEN_FILE", &crash_trigger)
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .stderr(Stdio::inherit())
        .kill_on_drop(true)
        .spawn()
        .expect("spawn the relay shim binary");
    let shim_stdin = shim.stdin.take().expect("shim stdin piped");

    // 1. The engine is genuinely up and beating (absorbs fork+exec latency), and the shim has
    //    reached steady state — the advertisement is published only after the child is spawned and
    //    contained, so its presence is the steady-state gate AND the handle for step 3.
    wait_for_child_alive(&heartbeat).await;
    let (advertisement, _adv) = wait_for_advertisement(&control_dir, "relaycrash").await;

    // 2. The editor disconnects. No trigger file exists yet, so the fixture CANNOT have exited:
    //    the biased select's `ChildExited` arm is not ready, and nothing signalled the shim.
    drop(shim_stdin);

    // 3. Wait for the shim to leave the select. By (2) the arm it chose can only be
    //    `Teardown::RelayStopped`, so the shim is now at the relay-stop grace check.
    wait_for_teardown_selected(&advertisement).await;

    // 4. NOW crash the engine — inside the grace window the disconnect opened, and provably after
    //    the arm was chosen rather than racing the choice.
    std::fs::write(&crash_trigger, b"crash").expect("write the crash trigger");

    // The child's crash must reach the shim's exit status faithfully — never a masked Code(0).
    let status = tokio::time::timeout(Duration::from_secs(15), shim.wait())
        .await
        .expect("the shim must exit after the disconnect + child crash (bounded)")
        .expect("await the shim exit status");
    assert_eq!(
        status.signal(),
        Some(libc::SIGKILL),
        "an engine crash inside the relay-stop grace window must propagate the CHILD's signal. \
         SIGKILL is reachable ONLY through the grace-check's `Some(status) => from_status` arm: \
         the post-grace arm attributes a bare SIGKILL to the shim's own kill and reports Code(0), \
         and the child-exit arm was excluded by ordering (the trigger is written only after the \
         teardown select resolved). Got {status:?} — a clean exit here means the crash was masked, \
         or landed outside the window and took a path this test does not cover."
    );
    assert!(
        !status.success(),
        "the shim must NOT report success when its engine crashed during teardown; got {status:?}"
    );

    let _ = std::fs::remove_dir_all(&dir);
}

/// IDENTITY MARKER: the shim embeds a stable ASCII identity marker so a packaging step can prove a
/// candidate file's BYTES are the Verter relay shim (not a renamed `tsgo` or an unrelated binary) by
/// scanning for the pinned `VERTER_RELAY_SHIM_IDENTITY:v1:` prefix. The hidden
/// `--verter-shim-identity` flag prints it; here we run the REAL binary with that flag and confirm
/// the pinned prefix is emitted — proving the literal is genuinely embedded AND reachable in the
/// shipped bytes. Portable: it needs no engine, no control dir, and no editor stdio.
#[test]
fn verter_shim_identity_flag_prints_marker() {
    let output = std::process::Command::new(bin_exe!("verter-relay-shim"))
        .arg("--verter-shim-identity")
        .output()
        .expect("run the relay shim binary with --verter-shim-identity");
    assert!(
        output.status.success(),
        "--verter-shim-identity must exit 0; got {:?} (stderr: {:?})",
        output.status,
        String::from_utf8_lossy(&output.stderr)
    );
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains("VERTER_RELAY_SHIM_IDENTITY:v1:"),
        "the shim must print the pinned identity marker on --verter-shim-identity; got stdout \
         {stdout:?}"
    );
}
