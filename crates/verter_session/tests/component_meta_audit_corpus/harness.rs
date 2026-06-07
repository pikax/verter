//! Corpus regression-capture harness.
//!
//! Wraps a single hermetic [`AuditedRequest`] resolution behind a
//! cooperative-cancellation worker so that any corpus fixture that
//! resolves slower than its declared timeout is converted into a
//! structured [`AuditCapture`] record AND a JSON dump of the full
//! [`RequestAuditRecord`].
//!
//! ## Cooperative cancellation
//!
//! - The resolution runs on a dedicated worker thread.
//! - `timeout` declares the slow-vs-fast classification boundary:
//!   a fixture that finishes in <= `timeout` is `Completed`; one
//!   that finishes after `timeout` (but before the hang deadline)
//!   is `Slow` and produces an [`AuditCapture`].
//! - Before the hang deadline expires we set a cancellation flag.
//!   The audit pipeline does not yet poll this flag at every phase
//!   boundary — when it does, the worker can exit early. Until then,
//!   the flag is a forward-compatible signal that future audited
//!   producers can observe.
//! - The hang deadline is `max(timeout * 2, MIN_HARD_DEADLINE)`. The
//!   floor exists so tests of the harness itself can pass tiny
//!   timeouts without racing into a spurious abort, while real
//!   corpus runs with multi-second timeouts get the plan's
//!   `timeout * 2` semantics directly.
//! - If the worker still has not finished after the hang deadline,
//!   the harness aborts the test process via [`std::process::abort`]
//!   rather than abandoning the worker thread. A detached worker
//!   would keep host locks live and corrupt later tests in the same
//!   target, so terminating the entire process is the only safe
//!   option once cooperative cancellation has been ignored.
//!
//! ## Capture artifact
//!
//! On a slow-but-successful resolution, the harness dumps the full
//! `RequestAuditRecord` JSON to:
//!
//! ```text
//! ${VERTER_AUDIT_CAPTURE_DIR:-target/audit-captures}/<fixture_basename>/<request_id>.json
//! ```
//!
//! The `target/` default lives under cargo's build directory, which
//! is gitignored, so dumps do not pollute the source tree. CI can
//! override the destination via the env var to upload artifacts.

#![allow(dead_code)]

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::thread;
use std::time::{Duration, Instant};

use verter_audit::RequestKind;
use verter_session::audited_request::{AuditedRequest, AuditedRequestError};
use verter_session::component_meta_audit::RequestAuditRecord;

/// Inputs describing one corpus fixture.
#[derive(Debug, Clone)]
pub struct CorpusFixture {
    /// Filesystem-friendly basename used as the per-fixture dump
    /// subdirectory (`<base>/<request_id>.json`).
    pub basename: String,
    /// Canonical id the hermetic host should resolve.
    pub canonical_id: String,
    /// Vue SFC source for the canonical_id.
    pub source: String,
}

/// The structured slow-fixture summary returned to the caller when
/// a fixture exceeds its declared timeout. The full record is in
/// [`Self::capture_path`]; this struct surfaces the high-signal
/// fields so a corpus test can `panic!()` with a useful message.
#[derive(Debug, Clone)]
pub struct AuditCapture {
    /// Audited request id; matches `RequestAuditRecord::request_id`.
    pub request_id: u64,
    /// Producer kind (always [`RequestKind::ComponentMeta`] for the
    /// current corpus, but typed for future fixtures).
    pub kind: RequestKind,
    /// Top-N files sorted by `read_ms + parse_ms + lower_ms`. Empty
    /// when the record carried no per-file attribution.
    pub slowest_files: Vec<SlowestFile>,
    /// Aggregate hit rate across every per-cache layer in the
    /// record's [`verter_session::component_meta_audit::RequestStoreAudit`].
    /// `None` when neither hits nor misses were observed (the cold
    /// path may legitimately bypass every counted layer).
    pub cache_hit_rate: Option<f64>,
    /// Name of the deepest non-zero phase from
    /// [`verter_session::component_meta_audit::RequestAuditRecord::timings`].
    /// `None` when every per-phase ms was 0.0 (e.g. a request that
    /// errored before producing useful timing).
    pub last_completed_phase: Option<&'static str>,
    /// Absolute path to the dumped JSON file.
    pub capture_path: PathBuf,
}

/// One row of [`AuditCapture::slowest_files`].
#[derive(Debug, Clone)]
pub struct SlowestFile {
    /// Canonical id reported by `FileAudit`.
    pub canonical_id: String,
    /// `read_ms + parse_ms + lower_ms` (None entries treated as 0).
    pub total_ms: f64,
}

/// Outcome of [`run_corpus_fixture_with_audit_capture`].
///
/// `RequestAuditRecord` is heap-boxed because it is large
/// (~1KB) and `clippy::large_enum_variant` would otherwise force
/// every `HarnessOutcome` value to carry that bulk on the stack.
#[derive(Debug)]
pub enum HarnessOutcome {
    /// Resolution completed within the declared timeout. Test code
    /// can inspect the record but typically just asserts on the
    /// presence of expected fields.
    Completed {
        /// The full audit record produced by the resolution.
        record: Box<RequestAuditRecord>,
    },
    /// Resolution completed but exceeded the timeout. Treated as a
    /// regression by the caller. `capture` carries the high-signal
    /// summary; the full JSON is at `capture.capture_path`.
    Slow {
        /// Structured slow-fixture summary; full record on disk.
        capture: AuditCapture,
    },
    /// Resolution failed for a non-timeout reason (e.g. hermetic
    /// fixture lacked transitive deps). The error variant is the
    /// underlying [`AuditedRequestError`].
    ResolveError {
        /// Underlying audited-request error.
        error: AuditedRequestError,
    },
}

/// Drive a hermetic [`AuditedRequest`] for `fixture`, classifying
/// the result as `Completed` (under threshold), `Slow` (over
/// threshold but completed — capture written), or `ResolveError`
/// (resolution failed for a non-timeout reason).
///
/// Aborts the test process if the worker fails to return within
/// the hang deadline (`max(timeout * 2, MIN_HARD_DEADLINE)`).
/// **No thread is ever abandoned.**
pub fn run_corpus_fixture_with_audit_capture(
    fixture: CorpusFixture,
    timeout: Duration,
) -> HarnessOutcome {
    let cancel = Arc::new(AtomicBool::new(false));
    let cancel_for_worker = Arc::clone(&cancel);
    let basename_for_thread = fixture.basename.clone();

    // The worker takes ownership of the fixture so the move is clean
    // across thread boundaries.
    let started = Instant::now();
    let worker = thread::Builder::new()
        .name(format!("audit-corpus-worker:{basename_for_thread}"))
        .spawn(move || run_audited(fixture, cancel_for_worker))
        .expect("spawning the corpus regression worker thread must succeed");

    let result = wait_for_worker(worker, timeout, &cancel);
    let elapsed = started.elapsed();

    match result {
        WorkerResult::Done(boxed) => match *boxed {
            Ok(record) => {
                if elapsed > timeout {
                    // Successful resolution that exceeded the
                    // timeout — treat as a regression, dump the
                    // record, return the structured capture.
                    let capture = write_capture(&record)
                        .unwrap_or_else(|e| panic!("harness failed to persist audit capture: {e}"));
                    HarnessOutcome::Slow { capture }
                } else {
                    HarnessOutcome::Completed {
                        record: Box::new(record),
                    }
                }
            }
            Err(error) => HarnessOutcome::ResolveError { error },
        },
        WorkerResult::ProcessAbortedDueToHang => {
            // `wait_for_worker` calls `std::process::abort()` itself
            // before returning; reaching here is unreachable in
            // practice, but the variant lets the function be total.
            unreachable!("hang abort path returns by aborting the process");
        }
    }
}

/// Lower bound on the hang-detection deadline. Real corpus runs use
/// multi-second timeouts so `timeout * 2` already dominates; the floor
/// exists so harness tests can pass tiny `timeout` values without
/// racing into a spurious process abort.
const MIN_HARD_DEADLINE: Duration = Duration::from_secs(30);

enum WorkerResult {
    Done(Box<Result<RequestAuditRecord, AuditedRequestError>>),
    /// Sentinel only used to make `wait_for_worker`'s type total.
    /// `wait_for_worker` aborts the process before returning this.
    ProcessAbortedDueToHang,
}

fn wait_for_worker(
    worker: thread::JoinHandle<Result<RequestAuditRecord, AuditedRequestError>>,
    timeout: Duration,
    cancel: &AtomicBool,
) -> WorkerResult {
    // Hang deadline: the `timeout * 2` semantics for real corpus
    // timeouts (which are multi-second),
    // but enforce a floor so tiny test-only timeouts cannot trigger
    // a spurious process abort.
    let hard_deadline_at =
        Instant::now() + std::cmp::max(timeout.saturating_mul(2), MIN_HARD_DEADLINE);
    // Cancel signal fires once we cross the slow-classification
    // boundary, so future audited producers that poll the flag can
    // exit early. Real corpus tests will see this fire as soon as
    // they cross `timeout`.
    let cancel_at = Instant::now() + timeout;

    let poll_slice = Duration::from_millis(50);
    let mut cancel_signalled = false;

    loop {
        if worker.is_finished() {
            return WorkerResult::Done(Box::new(join_or_panic(worker)));
        }
        let now = Instant::now();
        if !cancel_signalled && now >= cancel_at {
            cancel.store(true, Ordering::SeqCst);
            cancel_signalled = true;
        }
        if now >= hard_deadline_at {
            // Worker still running past the hang deadline. Aborting
            // the process is the only safe option — joining would
            // block forever and detaching would leak host locks into
            // later tests in this binary.
            eprintln!(
                "verter audit corpus harness: worker did not return within the hang deadline; aborting test process to avoid thread abandonment"
            );
            std::process::abort();
        }
        thread::sleep(poll_slice);
    }
}

fn join_or_panic(
    worker: thread::JoinHandle<Result<RequestAuditRecord, AuditedRequestError>>,
) -> Result<RequestAuditRecord, AuditedRequestError> {
    match worker.join() {
        Ok(res) => res,
        Err(payload) => {
            // Surface the worker panic as the calling test's failure.
            std::panic::resume_unwind(payload);
        }
    }
}

fn run_audited(
    fixture: CorpusFixture,
    _cancel: Arc<AtomicBool>,
) -> Result<RequestAuditRecord, AuditedRequestError> {
    // The cancellation flag is observable by audited producers that
    // wire phase-boundary checks. Today, hermetic component-meta
    // resolution is fast and synchronous; the flag is preserved as
    // an Arc capture so the worker thread can be cooperatively
    // signalled once downstream consumers start polling it.
    let CorpusFixture {
        canonical_id,
        source,
        ..
    } = fixture;
    let (_analysis, _resolution, record) = AuditedRequest::builder()
        .files([(canonical_id.clone(), source)])
        .resolve_component_meta(&canonical_id)?;
    Ok(record)
}

/// Resolve the capture root directory. Honours
/// `VERTER_AUDIT_CAPTURE_DIR` when set, otherwise falls back to the
/// workspace-local `target/audit-captures/` directory.
pub fn capture_root_dir() -> PathBuf {
    if let Some(custom) = std::env::var_os("VERTER_AUDIT_CAPTURE_DIR") {
        return PathBuf::from(custom);
    }
    default_capture_root_dir()
}

/// Workspace-local default capture root: `<workspace>/target/audit-captures/`.
/// Exposed so tests can verify the default-path computation without
/// mutating the env var (which races with concurrent tests).
pub fn default_capture_root_dir() -> PathBuf {
    // CARGO_MANIFEST_DIR points at this crate (verter_session). The
    // workspace `target/` lives two levels up.
    let manifest_dir = env!("CARGO_MANIFEST_DIR");
    PathBuf::from(manifest_dir)
        .join("..")
        .join("..")
        .join("target")
        .join("audit-captures")
}

fn write_capture(record: &RequestAuditRecord) -> std::io::Result<AuditCapture> {
    let root = capture_root_dir();
    let basename = derive_basename(&record.canonical_id);
    let dir = root.join(&basename);
    std::fs::create_dir_all(&dir)?;
    let file_path = dir.join(format!("{}.json", record.request_id));
    let json = serde_json::to_string_pretty(record).map_err(std::io::Error::other)?;
    std::fs::write(&file_path, json)?;
    Ok(AuditCapture {
        request_id: record.request_id,
        kind: record.kind.clone(),
        slowest_files: top_slowest_files(record, 5),
        cache_hit_rate: aggregate_cache_hit_rate(record),
        last_completed_phase: deepest_phase(record),
        capture_path: file_path,
    })
}

/// Strip directory and extension from a canonical id so the dump
/// directory is filesystem-friendly. `/Foo.vue` → `Foo`.
fn derive_basename(canonical_id: &str) -> String {
    let trimmed = canonical_id.trim_start_matches('/');
    let stem = match Path::new(trimmed).file_stem() {
        Some(s) => s.to_string_lossy().to_string(),
        None => trimmed.replace(['/', '\\'], "_"),
    };
    if stem.is_empty() {
        "unknown_fixture".to_string()
    } else {
        stem
    }
}

fn top_slowest_files(record: &RequestAuditRecord, n: usize) -> Vec<SlowestFile> {
    let mut rows: Vec<SlowestFile> = record
        .files
        .iter()
        .map(|f| {
            let total =
                f.read_ms.unwrap_or(0.0) + f.parse_ms.unwrap_or(0.0) + f.lower_ms.unwrap_or(0.0);
            SlowestFile {
                canonical_id: f.canonical_id.clone(),
                total_ms: total,
            }
        })
        .collect();
    rows.sort_by(|a, b| {
        b.total_ms
            .partial_cmp(&a.total_ms)
            .unwrap_or(std::cmp::Ordering::Equal)
    });
    rows.truncate(n);
    rows
}

fn aggregate_cache_hit_rate(record: &RequestAuditRecord) -> Option<f64> {
    let layers = &record.store.cache_layers;
    let groups = [
        &layers.indexed,
        &layers.analysis,
        &layers.owner_import,
        &layers.route_owned_shallow,
        &layers.component_meta,
        &layers.route_db,
        &layers.ref_cycle,
        &layers.intrinsic_registry,
        &layers.semantic_graph,
        &layers.materialize_structure,
        &layers.materialize_memo,
        &layers.member_shape_cache,
        &layers.prepared_surface,
        &layers.prepared_member,
    ];
    let mut hits: u64 = 0;
    let mut misses: u64 = 0;
    for layer in groups {
        hits = hits.saturating_add(layer.hits);
        misses = misses.saturating_add(layer.misses);
    }
    let total = hits.saturating_add(misses);
    if total == 0 {
        return None;
    }
    Some(hits as f64 / total as f64)
}

fn deepest_phase(record: &RequestAuditRecord) -> Option<&'static str> {
    // Canonical phase order, latest-first. The deepest non-zero phase
    // is the "last phase the request reached before completing or
    // hanging".
    let timings = &record.timings;
    let ordered: [(&'static str, f64); 8] = [
        ("serialize", timings.serialize_ms),
        ("materialize", timings.materialize_ms),
        ("solver", timings.solver_ms),
        ("imported_root_proof", timings.imported_root_proof_ms),
        ("direct_import_proof", timings.direct_import_proof_ms),
        ("store_merge", timings.store_merge_ms),
        ("store_read", timings.store_read_ms),
        ("capture_inputs", timings.capture_inputs_ms),
    ];
    ordered.iter().find(|(_, ms)| *ms > 0.0).map(|(n, _)| *n)
}
