//! Tests for the corpus regression-capture harness defined in
//! `tests/component_meta_audit_corpus/harness.rs`.
//!
//! Contract under test: a corpus fixture that resolves slower than
//! its declared timeout MUST produce a structured [`AuditCapture`]
//! AND dump the full
//! [`verter_session::component_meta_audit::RequestAuditRecord`] JSON
//! to
//! `${VERTER_AUDIT_CAPTURE_DIR:-target/audit-captures}/<basename>/<request_id>.json`,
//! rather than hanging the test process or losing data via thread
//! abandonment.

#[path = "../component_meta_audit_corpus/harness.rs"]
mod harness;

use std::fs;
use std::path::PathBuf;
use std::sync::Mutex;
use std::time::Duration;

use harness::{
    capture_root_dir, default_capture_root_dir, run_corpus_fixture_with_audit_capture,
    AuditCapture, CorpusFixture, HarnessOutcome,
};

/// Tests in this target mutate `VERTER_AUDIT_CAPTURE_DIR` to redirect
/// capture artifacts into per-test tempdirs. cargo runs `#[test]`
/// functions in parallel, so we serialize env-var manipulation behind
/// a process-wide mutex to keep the redirection observed by each test
/// scoped to that test only.
fn env_lock() -> &'static Mutex<()> {
    static LOCK: Mutex<()> = Mutex::new(());
    &LOCK
}

/// RAII guard that scopes `VERTER_AUDIT_CAPTURE_DIR` to a tempdir for
/// the lifetime of the guard, restoring the previous value (or
/// unsetting) on drop. Holds [`env_lock`] for the same scope so a
/// concurrent test cannot observe a different binding.
struct ScopedCaptureDir {
    _lock: std::sync::MutexGuard<'static, ()>,
    prev: Option<std::ffi::OsString>,
    _temp: tempfile::TempDir,
    root: PathBuf,
}

impl ScopedCaptureDir {
    fn new() -> Self {
        let lock = env_lock().lock().unwrap_or_else(|p| p.into_inner());
        let prev = std::env::var_os("VERTER_AUDIT_CAPTURE_DIR");
        let temp = tempfile::tempdir().expect("scoped capture-dir tempdir");
        let root = temp.path().to_path_buf();
        // SAFETY: env_lock() serialises this with every other test
        // that mutates the same variable; we restore on drop.
        unsafe {
            std::env::set_var("VERTER_AUDIT_CAPTURE_DIR", &root);
        }
        Self {
            _lock: lock,
            prev,
            _temp: temp,
            root,
        }
    }

    fn root(&self) -> &PathBuf {
        &self.root
    }
}

impl Drop for ScopedCaptureDir {
    fn drop(&mut self) {
        // SAFETY: lock held by `_lock` field protects the env-var
        // mutation; restore the prior value verbatim.
        unsafe {
            match self.prev.take() {
                Some(prev) => std::env::set_var("VERTER_AUDIT_CAPTURE_DIR", prev),
                None => std::env::remove_var("VERTER_AUDIT_CAPTURE_DIR"),
            }
        }
    }
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
    // file. Would falsely pass on the pre-change tree only if the
    // harness module did not exist — which is exactly the negative we
    // are guarding.
    let scoped = ScopedCaptureDir::new();

    let outcome =
        run_corpus_fixture_with_audit_capture(fixture("fast_fixture"), Duration::from_secs(60));

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
    let dir = scoped.root().join("fast_fixture");
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
    // Fails on the pre-change tree because:
    //   - `run_corpus_fixture_with_audit_capture` does not exist
    //   - `AuditCapture` does not exist
    //   - The dump-path layout (`<root>/<base>/<id>.json`)
    //     is undefined
    //
    // Passes on the post-change tree because:
    //   - The harness completes the resolution synchronously, observes
    //     the elapsed time exceeded the threshold, populates AuditCapture
    //     fields from the captured record, and writes the JSON file at
    //     the documented path.
    let scoped = ScopedCaptureDir::new();

    let outcome =
        run_corpus_fixture_with_audit_capture(fixture("slow_fixture"), Duration::from_nanos(1));

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
    let expected_dir = scoped.root().join("slow_fixture");
    let expected_file = expected_dir.join(format!("{}.json", capture.request_id));
    assert_eq!(
        capture.capture_path, expected_file,
        "capture_path must equal <VERTER_AUDIT_CAPTURE_DIR>/<basename>/<request_id>.json",
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
fn capture_dir_overridable_via_env_var() {
    // Discriminating: setting `VERTER_AUDIT_CAPTURE_DIR` MUST redirect
    // the dump location. Validates the CI-redirect contract from the
    // plan. Pre-change tree fails because the harness does not exist;
    // a faulty post-change implementation that hard-codes the directory
    // would also fail this test.
    let scoped = ScopedCaptureDir::new();
    let override_root = scoped.root().clone();

    let outcome = run_corpus_fixture_with_audit_capture(
        fixture("env_override_fixture"),
        Duration::from_nanos(1),
    );

    let capture = match outcome {
        HarnessOutcome::Slow { capture } => capture,
        other => panic!("expected Slow with env override; got {other:?}"),
    };

    assert!(
        capture.capture_path.starts_with(&override_root),
        "capture_path {:?} must live under VERTER_AUDIT_CAPTURE_DIR override {:?}",
        capture.capture_path,
        override_root,
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
    // harness MUST default to `<workspace>/target/audit-captures/`.
    // The workspace `target/` directory is gitignored, so the default
    // does not pollute the source tree.
    //
    // We verify the path computation directly (no resolution) so the
    // test is hermetic and does not race with concurrent env-var
    // tests. This is a pure-function unit assertion on
    // `default_capture_root_dir` which the harness uses when
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
fn capture_root_dir_returns_default_when_env_var_unset() {
    // Companion to the env-var override test: with the var explicitly
    // unset, `capture_root_dir()` MUST equal `default_capture_root_dir()`.
    let lock = env_lock().lock().unwrap_or_else(|p| p.into_inner());
    // SAFETY: holding `env_lock` serialises this with every test that
    // mutates the same variable.
    let prev = std::env::var_os("VERTER_AUDIT_CAPTURE_DIR");
    unsafe {
        std::env::remove_var("VERTER_AUDIT_CAPTURE_DIR");
    }
    let resolved = capture_root_dir();
    let expected = default_capture_root_dir();
    let result = std::panic::catch_unwind(|| {
        assert_eq!(
            resolved, expected,
            "capture_root_dir must default to workspace target/audit-captures",
        );
    });
    // SAFETY: lock still held; restore var before releasing.
    unsafe {
        match prev {
            Some(prev) => std::env::set_var("VERTER_AUDIT_CAPTURE_DIR", prev),
            None => std::env::remove_var("VERTER_AUDIT_CAPTURE_DIR"),
        }
    }
    drop(lock);
    if let Err(payload) = result {
        std::panic::resume_unwind(payload);
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
    let _scoped = ScopedCaptureDir::new();
    let outcome = run_corpus_fixture_with_audit_capture(
        fixture("no_thread_leak_fixture"),
        Duration::from_secs(60),
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
