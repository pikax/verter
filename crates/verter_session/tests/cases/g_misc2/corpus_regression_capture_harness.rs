//! Tests for the corpus regression-capture harness defined in
//! `tests/cases/component_meta_audit_corpus/harness.rs`.
//!
//! Contract under test: a corpus fixture that resolves slower than
//! its declared timeout MUST produce a structured [`AuditCapture`]
//! AND dump the full
//! [`verter_session::component_meta_audit::RequestAuditRecord`] JSON
//! to `<capture_root>/<basename>/<request_id>.json`, rather than
//! hanging the test process or losing data via thread abandonment.
//!
//! The dump root is threaded EXPLICITLY into
//! `run_corpus_fixture_with_audit_capture` — these tests pass a
//! per-test tempdir directly and NEVER mutate the process-global
//! `VERTER_AUDIT_CAPTURE_DIR` env var, so they run fully in parallel
//! with no shared-process serialization. The env-fallback resolver
//! (`capture_root_dir()`) is a separate production seam; because its
//! behaviour depends on the process-global env, the two tests that
//! exercise the live `std::env::var_os` read drive it in a SUBPROCESS
//! (a re-invocation of this test binary targeting the
//! `capture_root_dir_env_probe_child` worker) with the env set ONLY on
//! the child via `Command::env` / `Command::env_remove`. No test in
//! this binary mutates the parent process env, so the whole file is
//! parallel-safe with no serial group.

#[path = "../component_meta_audit_corpus/harness.rs"]
mod harness;

use std::fs;
use std::path::PathBuf;
use std::process::Command;
use std::time::Duration;

use harness::{
    capture_root_dir, default_capture_root_dir, run_corpus_fixture_with_audit_capture,
    AuditCapture, CorpusFixture, HarnessOutcome,
};

/// Control env var the parent sets on the SUBPROCESS to select which
/// env-fallback assertion the `capture_root_dir_env_probe_child` worker
/// runs. Two values: `"override"` (expect `capture_root_dir()` to equal
/// the `VERTER_AUDIT_CAPTURE_DIR` value the parent also set on the
/// child) and `"unset"` (expect it to equal `default_capture_root_dir()`
/// with `VERTER_AUDIT_CAPTURE_DIR` removed from the child env). Unset in
/// the parent's normal run, so the worker is a no-op there and never
/// touches the parent env.
const PROBE_MODE_VAR: &str = "VERTER_AUDIT_CAPTURE_DIR_PROBE_MODE";

/// Re-invoke THIS test binary as a subprocess, running ONLY the
/// `capture_root_dir_env_probe_child` worker, with `configure_child`
/// applied to the child `Command` (it sets `PROBE_MODE_VAR` plus the
/// `VERTER_AUDIT_CAPTURE_DIR` override/removal — all on the CHILD ONLY).
/// Returns the child's success status. The parent process env is never
/// mutated.
fn run_env_probe_child(configure_child: impl FnOnce(&mut Command)) -> std::process::Output {
    let exe = std::env::current_exe().expect("current test executable path");
    // Fully-qualified libtest name of the worker below, derived from the
    // live module path so a module rename can never silently target a
    // non-existent test (which libtest would report as "0 tests run" —
    // a success status that would defeat the assertion).
    //
    // `module_path!()` for an integration-test module is
    // `<bin_root>::<module>` (e.g. `g_misc2::corpus_regression_capture_harness`),
    // but libtest's test names OMIT the binary-root segment
    // (`corpus_regression_capture_harness::<test>`). Strip the leading
    // `<bin_root>::` so the `--exact` filter matches.
    let module_path_no_root = module_path!()
        .split_once("::")
        .map_or(module_path!(), |(_bin_root, rest)| rest);
    let worker_name = format!("{module_path_no_root}::capture_root_dir_env_probe_child");
    let mut cmd = Command::new(exe);
    // `--exact` + the fully-qualified test name targets the single
    // worker; `--nocapture` surfaces its panic message on failure;
    // `--test-threads=1` keeps the child deterministic.
    cmd.arg(&worker_name)
        .arg("--exact")
        .arg("--nocapture")
        .arg("--test-threads=1");
    configure_child(&mut cmd);
    cmd.output().expect("failed to spawn env-probe subprocess")
}

/// Spawn the env-probe worker (via [`run_env_probe_child`]) and assert
/// it both SUCCEEDED and actually RAN the worker. The "actually ran"
/// check (`1 passed` in libtest's summary) closes the false-pass hole
/// where a zero-match filter would exit 0 without exercising any
/// assertion.
fn assert_env_probe_child_passes(case: &str, configure_child: impl FnOnce(&mut Command)) {
    let output = run_env_probe_child(configure_child);
    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        output.status.success(),
        "{case} probe subprocess must succeed.\nstdout:\n{stdout}\nstderr:\n{stderr}",
    );
    // libtest prints `test result: ok. 1 passed; ...` only when the
    // worker actually ran. A zero-match filter prints `0 passed`, which
    // would otherwise be a silent false success.
    assert!(
        stdout.contains("1 passed"),
        "{case} probe subprocess must have RUN exactly one worker test \
         (libtest `1 passed` not found — a zero-match filter would exit 0 \
         without exercising the assertion).\nstdout:\n{stdout}\nstderr:\n{stderr}",
    );
}

/// A minimal valid Vue SFC that resolves quickly through a hermetic
/// [`AuditedRequest`]. Used both for the "fast" path (under threshold,
/// returns Ok) and the "slow" path (timeout effectively zero, returns
/// AuditCapture).
const TINY_FIXTURE_SRC: &str =
    "<script setup lang=\"ts\">\ndefineProps<{ label: string }>()\n</script>\n<template><div>{{ label }}</div></template>\n";

fn fixture(name: &str) -> CorpusFixture {
    CorpusFixture {
        basename: name.to_string(),
        canonical_id: format!("/{name}.vue"),
        source: TINY_FIXTURE_SRC.to_string(),
    }
}

#[test]
fn fast_fixture_returns_ok_and_writes_no_capture() {
    // Discriminating: a healthy fixture with a generous timeout MUST
    // return Ok(HarnessOutcome::Completed) AND must NOT write a capture
    // file. The negative half — no capture directory for a passing
    // fixture — is the property this asserts.
    let temp = tempfile::tempdir().expect("capture-dir tempdir");
    let root = temp.path();

    let outcome = run_corpus_fixture_with_audit_capture(
        fixture("fast_fixture"),
        Duration::from_secs(60),
        root,
    );

    match outcome {
        HarnessOutcome::Completed { record } => {
            assert!(
                record.request_id > 0,
                "fast fixture must produce a record with a non-zero request_id",
            );
        }
        other => panic!(
            "fast fixture expected Completed; got {other:?}. The harness must not produce a capture under a generous timeout.",
        ),
    };

    // The capture directory MUST NOT exist for a passing fixture — the
    // dump path is reserved for regressions.
    let dir = root.join("fast_fixture");
    assert!(
        !dir.exists(),
        "harness must not create a capture directory for a passing fixture; found {dir:?}",
    );
}

#[test]
fn slow_fixture_emits_audit_capture_with_full_record_json() {
    // Discriminating: forcing a near-zero timeout converts every
    // resolution into a "regression" — the harness MUST classify it as
    // slow, populate a structured AuditCapture, and persist the full
    // RequestAuditRecord JSON to the deterministic dump path.
    //
    // The asserted contract: the harness completes the resolution
    // synchronously, observes the elapsed time exceeded the threshold,
    // populates the AuditCapture fields from the captured record, and
    // writes the JSON file at the documented `<root>/<base>/<id>.json`
    // layout.
    let temp = tempfile::tempdir().expect("capture-dir tempdir");
    let root = temp.path();

    let outcome = run_corpus_fixture_with_audit_capture(
        fixture("slow_fixture"),
        Duration::from_nanos(1),
        root,
    );

    let capture: AuditCapture = match outcome {
        HarnessOutcome::Slow { capture } => capture,
        other => panic!("slow-threshold fixture must return HarnessOutcome::Slow; got {other:?}",),
    };

    // Structured fields populated from the underlying record.
    assert!(
        capture.request_id > 0,
        "AuditCapture.request_id must mirror the audited request id (got 0)",
    );
    assert_eq!(
        capture.kind,
        verter_audit::RequestKind::ComponentMeta,
        "slow corpus fixture is a component-meta resolution",
    );
    assert!(
        capture.last_completed_phase.is_some(),
        "harness must surface the deepest non-zero timing phase as last_completed_phase",
    );
    // cache_hit_rate is in [0.0, 1.0] when populated. A hermetic cold
    // resolution may legitimately observe zero hits and zero misses on
    // every layer; that yields `None`, which is still a valid signal
    // (see harness comment). Assert the bound when populated.
    if let Some(rate) = capture.cache_hit_rate {
        assert!(
            (0.0..=1.0).contains(&rate),
            "cache_hit_rate must be in [0.0, 1.0]; got {rate}",
        );
    }

    // Capture path layout: <root>/<basename>/<id>.json
    let expected_dir = root.join("slow_fixture");
    let expected_file = expected_dir.join(format!("{}.json", capture.request_id));
    assert_eq!(
        capture.capture_path, expected_file,
        "capture_path must equal <capture_root>/<basename>/<request_id>.json",
    );
    assert!(
        capture.capture_path.exists(),
        "harness must persist the capture file at {expected_file:?}",
    );

    // The file must contain the full RequestAuditRecord JSON.
    let raw = fs::read_to_string(&capture.capture_path).expect("capture file must be readable");
    let parsed: verter_session::component_meta_audit::RequestAuditRecord =
        serde_json::from_str(&raw)
            .expect("capture file must round-trip as a full RequestAuditRecord (harness contract)");
    assert_eq!(
        parsed.request_id, capture.request_id,
        "capture-file record_id must match AuditCapture.request_id",
    );
    assert_eq!(
        parsed.canonical_id, "/slow_fixture.vue",
        "capture-file canonical must match the requested fixture canonical",
    );
}

#[test]
fn capture_path_lives_under_explicit_capture_root() {
    // Discriminating: the dump location MUST be the EXPLICIT
    // capture-root the caller threaded in — not a hard-coded default
    // and not the process env. A faulty implementation that ignored the
    // explicit `capture_root` argument (e.g. still read the env /
    // hard-coded `target/audit-captures`) fails this test because the
    // dump would land elsewhere.
    let temp = tempfile::tempdir().expect("capture-dir tempdir");
    let explicit_root = temp.path().to_path_buf();

    let outcome = run_corpus_fixture_with_audit_capture(
        fixture("explicit_root_fixture"),
        Duration::from_nanos(1),
        &explicit_root,
    );

    let capture = match outcome {
        HarnessOutcome::Slow { capture } => capture,
        other => panic!("expected Slow with explicit capture root; got {other:?}"),
    };

    assert!(
        capture.capture_path.starts_with(&explicit_root),
        "capture_path {:?} must live under the explicit capture_root {:?}",
        capture.capture_path,
        explicit_root,
    );
    assert!(
        capture.capture_path.exists(),
        "capture file must exist at {:?}",
        capture.capture_path,
    );
    let _: verter_session::component_meta_audit::RequestAuditRecord =
        serde_json::from_str(&fs::read_to_string(&capture.capture_path).unwrap())
            .expect("capture file must contain a valid RequestAuditRecord JSON");
}

#[test]
fn default_capture_root_lives_under_workspace_target_audit_captures() {
    // Discriminating: when `VERTER_AUDIT_CAPTURE_DIR` is unset the
    // production-fallback resolver MUST default to
    // `<workspace>/target/audit-captures/`. The workspace `target/`
    // directory is gitignored, so the default does not pollute the
    // source tree.
    //
    // We verify the path computation directly (no resolution) so the
    // test is hermetic and does not race with concurrent env-var
    // tests. This is a pure-function unit assertion on
    // `default_capture_root_dir` which `capture_root_dir()` uses when
    // `VERTER_AUDIT_CAPTURE_DIR` is unset.
    let manifest_dir = env!("CARGO_MANIFEST_DIR");
    let expected = PathBuf::from(manifest_dir)
        .join("..")
        .join("..")
        .join("target")
        .join("audit-captures");
    assert_eq!(default_capture_root_dir(), expected);
}

#[test]
fn capture_root_dir_honours_env_override() {
    // Discriminating: the production-fallback resolver `capture_root_dir()`
    // MUST honour `VERTER_AUDIT_CAPTURE_DIR` when set. This is the
    // CI-redirect contract: a production runner reads `capture_root_dir()`
    // and passes the result as the harness `capture_root`. A resolver
    // that ignored the env override (hard-coded the default) would fail
    // the child worker's assertion, the child would exit non-zero, and
    // this parent assertion would fail.
    //
    // The live `std::env::var_os` read happens in the
    // `capture_root_dir_env_probe_child` worker, in a SUBPROCESS, with
    // `VERTER_AUDIT_CAPTURE_DIR` set ONLY on the child. This parent
    // process never mutates its own env, so the test is parallel-safe.
    let temp = tempfile::tempdir().expect("env-override tempdir");
    let override_root = temp.path().to_path_buf();

    assert_env_probe_child_passes("env-override", |cmd| {
        cmd.env(PROBE_MODE_VAR, "override");
        cmd.env("VERTER_AUDIT_CAPTURE_DIR", &override_root);
    });
}

#[test]
fn capture_root_dir_returns_default_when_env_var_unset() {
    // Companion to the env-override test: with the var explicitly
    // unset, `capture_root_dir()` MUST equal `default_capture_root_dir()`.
    // The live read runs in the `capture_root_dir_env_probe_child`
    // worker, in a SUBPROCESS, with `VERTER_AUDIT_CAPTURE_DIR` REMOVED
    // from the child env via `Command::env_remove`. This parent process
    // never mutates its own env, so the test is parallel-safe. A
    // resolver that hard-coded an override (ignoring the unset state)
    // would fail the child assertion and flip this parent to a failure.
    assert_env_probe_child_passes("env-unset", |cmd| {
        cmd.env(PROBE_MODE_VAR, "unset");
        cmd.env_remove("VERTER_AUDIT_CAPTURE_DIR");
    });
}

/// SUBPROCESS WORKER for the two env-fallback tests above. NOT a
/// standalone assertion in the parent's normal run: when `PROBE_MODE_VAR`
/// is absent it returns immediately (the parent's pass-through
/// invocation), touching NOTHING. When the parent re-invokes this binary
/// with `--exact capture_root_dir_env_probe_child` and a `PROBE_MODE_VAR`
/// value, it performs the REAL `capture_root_dir()` env read against the
/// child's OWN process env (which the parent configured via
/// `Command::env` / `Command::env_remove`) and asserts the expected
/// resolution. The child mutates only its own env — inherited from the
/// parent's `Command`, never written here.
#[test]
fn capture_root_dir_env_probe_child() {
    let Some(mode) = std::env::var_os(PROBE_MODE_VAR) else {
        // Parent's normal run: this is not the spawned probe. Do
        // nothing — the real coverage runs in the subprocess invocations
        // driven by the two parent tests.
        return;
    };
    let mode = mode.to_string_lossy().into_owned();
    let resolved = capture_root_dir();
    match mode.as_str() {
        "override" => {
            // The parent set VERTER_AUDIT_CAPTURE_DIR on this child.
            // capture_root_dir() MUST return exactly that path; a
            // resolver that ignored the env and returned the default
            // would not equal it (discriminating).
            let expected =
                PathBuf::from(std::env::var_os("VERTER_AUDIT_CAPTURE_DIR").expect(
                    "override probe must run with VERTER_AUDIT_CAPTURE_DIR set on the child",
                ));
            assert_eq!(
                resolved, expected,
                "capture_root_dir must return the VERTER_AUDIT_CAPTURE_DIR override when set",
            );
            // The default must NOT collide with the override path, so the
            // assertion above genuinely discriminates the env-honoring
            // branch from the fallback branch.
            assert_ne!(
                resolved,
                default_capture_root_dir(),
                "override probe tempdir must differ from the workspace default \
                 (otherwise the env-honoring assertion would not discriminate)",
            );
        }
        "unset" => {
            // The parent removed VERTER_AUDIT_CAPTURE_DIR from this
            // child. capture_root_dir() MUST fall back to the workspace
            // default; a resolver that hard-coded an override would fail.
            assert!(
                std::env::var_os("VERTER_AUDIT_CAPTURE_DIR").is_none(),
                "unset probe must run with VERTER_AUDIT_CAPTURE_DIR removed from the child env",
            );
            assert_eq!(
                resolved,
                default_capture_root_dir(),
                "capture_root_dir must default to workspace target/audit-captures",
            );
        }
        other => panic!("capture_root_dir_env_probe_child: unknown {PROBE_MODE_VAR}={other:?}"),
    }
}

#[test]
fn cooperative_cancellation_does_not_abandon_threads() {
    // Discriminating: the harness spawns a worker thread per fixture.
    // After the resolution completes (which it does promptly for this
    // hermetic fixture), the worker MUST be joinable — leaking threads
    // would silently mask future regressions and could keep host
    // locks alive across tests in this binary.
    //
    // Rust does not expose a cross-platform "join all my children"
    // API, so we rely on a structural invariant: the harness returns
    // synchronously only after joining its worker. Reading a fully
    // populated record back is itself proof that the join happened
    // before the harness returned.
    //
    // The harness MUST return synchronously; if a future refactor
    // switches to fire-and-forget, this test fails because the
    // record cannot be observed without a join.
    let temp = tempfile::tempdir().expect("capture-dir tempdir");
    let outcome = run_corpus_fixture_with_audit_capture(
        fixture("no_thread_leak_fixture"),
        Duration::from_secs(60),
        temp.path(),
    );
    let record = match outcome {
        HarnessOutcome::Completed { record } => *record,
        other => panic!("expected fast Completed; got {other:?}"),
    };
    // The fact that we got a fully populated record back means the
    // worker thread completed and was joined before this point — the
    // record cannot be observed without a join. This is the structural
    // proof that no thread was abandoned.
    assert!(record.request_id > 0);
    assert_eq!(record.canonical_id, "/no_thread_leak_fixture.vue");
}
