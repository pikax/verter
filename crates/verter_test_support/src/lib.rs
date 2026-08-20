//! Shared test-only primitives that prevent flake classes from being
//! reintroduced one crate at a time.
//!
//! `3bf93df59` fixed three known-flaky tests and 13 hardcoded temp-path call
//! sites, but the fix itself created five near-identical `unique_temp_dir`
//! helpers (one per file) — the Shared Optimized Codebase rule broken by the
//! very commit that landed it. This crate is the single, lowest reusable
//! owner: a dev-dependency-only "harness" crate (see
//! `crates/verter_identity/tests/cases/workspace_dependency_layers.rs`,
//! `LAYER_7_HARNESSES`) that every consumer's test code calls into instead of
//! rolling its own.
//!
//! For staged fixture *trees* with lease-based cross-process reclamation
//! (materialised `node_modules`, copied fixture sources, and similar
//! multi-file workspaces that outlive a single test), see
//! `verter_lsp::test_harness_fixture_dependencies` — a more elaborate,
//! already-correct implementation this crate does not duplicate or replace.
//! Reach for [`unique_temp_dir`] when a test just needs one scratch
//! directory or file path with no cross-process reclamation story.

use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering};

/// A per-process, per-call temp path under `name`.
///
/// Bare `std::env::temp_dir().join(name)` is a shared OS path with no
/// per-process component: two concurrent invocations of a test suite on the
/// same machine (e.g. two git worktrees, or a retry racing a still-cleaning-up
/// prior run) race each other's `remove_dir_all` + rewrite of the SAME path,
/// producing spurious missing/wrong-content failures. This was the root
/// cause behind 13 call sites fixed in `3bf93df59`.
///
/// The returned path is unique per **call**, not just per process: an
/// in-process ordinal is appended alongside the process id, so two tests in
/// the same binary that happen to pick the same literal `name` still get
/// distinct paths — the process-id-only convention those 13 sites used could
/// not make that guarantee.
///
/// This function only MINTS the path; it does not create or remove anything.
/// Callers own `create_dir_all` / `remove_dir_all` on the returned path
/// exactly as before.
pub fn unique_temp_dir(name: &str) -> PathBuf {
    static ORDINAL: AtomicU64 = AtomicU64::new(0);
    let ordinal = ORDINAL.fetch_add(1, Ordering::Relaxed);
    std::env::temp_dir().join(format!("{name}-{}-{ordinal}", std::process::id()))
}

/// Bind an OS-assigned ephemeral TCP port on `127.0.0.1` and return it.
///
/// A test that needs a real, reachable port for a spawned server must never
/// hardcode one: a fixed port collides across concurrent test processes (and
/// with anything else running on the machine) exactly like the temp-path
/// class this crate exists to prevent. Binding port `0` asks the OS to
/// assign a free one.
///
/// Caveat, stated rather than hidden: this has an unavoidable TOCTOU window
/// — the listener used to discover the port is dropped before the caller can
/// bind its own, so in principle another process could claim the port in
/// between. That is the same trade-off every "ask the OS for a free port,
/// then hand it to a subprocess" pattern makes; it is still categorically
/// safer than a hardcoded literal, which collides on every single concurrent
/// run rather than in a narrow race window.
pub fn ephemeral_tcp_port() -> u16 {
    std::net::TcpListener::bind("127.0.0.1:0")
        .expect("bind an OS-assigned ephemeral TCP port")
        .local_addr()
        .expect("read back the bound ephemeral port")
        .port()
}

/// A deterministic call counter for correctness assertions that must not
/// touch a clock.
///
/// `store_view_build_wall_cost_is_flat_across_host_sizes` (fixed in
/// `3bf93df59`, now `store_view_build_touches_no_owner_at_any_host_size` in
/// `crates/verter_session/src/store_view_o1_build_tests.rs`) asserted a wall
/// clock `Instant::now()` ratio to prove an O(1) build property, which
/// flaked under load. The fix replaced it with a THREAD-LOCAL counter
/// (`crate::store_view_roots::store_view_owner_visits`) that increments on
/// every read through a scope-gated production code path, and asserted the
/// counter's value directly — proving the same property without touching a
/// clock.
///
/// This type is the generic, reusable shape of that pattern for a test whose
/// code under test runs on a single thread: reset before the operation,
/// exercise it, read the count back. When the code under test can run on
/// multiple threads or re-entrant scopes, `thread_local!` is the right
/// primitive instead — copy the shape in
/// `crates/verter_session/src/store_view_roots.rs` (search
/// `store_view_owner_visits`) rather than adapting this type, since a single
/// process-wide atomic cannot express "count only while this thread is
/// inside a marked scope".
///
/// The maintainer's standing rule this exists to support: `verter_session`
/// correctness tests are deterministic, never wall-clock; wall-clock budgets
/// belong in `verter_bench`. That rule generalizes to every correctness test
/// in this workspace — a test proving "this stayed O(1)" or "this ran
/// exactly N times" should count, not time.
#[derive(Debug, Default)]
pub struct DeterministicCounter(AtomicUsize);

impl DeterministicCounter {
    pub const fn new() -> Self {
        Self(AtomicUsize::new(0))
    }

    /// Zero the counter. Call before the operation under test.
    pub fn reset(&self) {
        self.0.store(0, Ordering::SeqCst);
    }

    /// Record one occurrence. Call from the instrumented production path.
    pub fn increment(&self) {
        self.0.fetch_add(1, Ordering::SeqCst);
    }

    /// Read the current count. Call after the operation under test.
    pub fn get(&self) -> usize {
        self.0.load(Ordering::SeqCst)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unique_temp_dir_paths_never_repeat_across_calls_with_the_same_name() {
        let a = unique_temp_dir("probe");
        let b = unique_temp_dir("probe");
        assert_ne!(
            a, b,
            "two calls with the identical name must mint distinct paths, not \
             just distinct-per-process ones"
        );
        assert!(a.starts_with(std::env::temp_dir()));
        assert!(b.starts_with(std::env::temp_dir()));
    }

    #[test]
    fn unique_temp_dir_paths_are_writable_scratch_directories() {
        let dir = unique_temp_dir("verter_test_support_selftest");
        assert!(!dir.exists(), "a freshly minted path must not pre-exist");
        std::fs::create_dir_all(&dir).expect("create the minted scratch directory");
        std::fs::write(dir.join("marker.txt"), b"ok").expect("write into the scratch directory");
        assert!(dir.join("marker.txt").is_file());
        std::fs::remove_dir_all(&dir).expect("clean up the scratch directory");
    }

    #[test]
    fn ephemeral_tcp_port_binds_a_reachable_nonzero_port() {
        let port = ephemeral_tcp_port();
        assert_ne!(
            port, 0,
            "the OS must assign a concrete port, not echo back 0"
        );
        // The port must be immediately re-bindable: nothing else holds it,
        // because the listener that discovered it was dropped first.
        let relisten = std::net::TcpListener::bind(("127.0.0.1", port));
        assert!(
            relisten.is_ok(),
            "an ephemeral port must be free immediately after discovery"
        );
    }

    #[test]
    fn ephemeral_tcp_port_calls_do_not_collide_with_each_other() {
        let mut ports = std::collections::HashSet::new();
        for _ in 0..16 {
            // Keep each listener alive so the next bind cannot reuse its port —
            // this is what actually proves 16 concurrent binds get 16 distinct
            // ports, rather than 16 sequential reuses of one freed port.
            let port = ephemeral_tcp_port();
            let held = std::net::TcpListener::bind(("127.0.0.1", port))
                .expect("re-bind the just-discovered ephemeral port");
            assert!(
                ports.insert(port),
                "ephemeral port {port} was assigned twice"
            );
            std::mem::forget(held); // released at process exit; this is a short-lived test process
        }
    }

    #[test]
    fn deterministic_counter_starts_at_zero_and_resets() {
        let counter = DeterministicCounter::new();
        assert_eq!(counter.get(), 0);
        counter.increment();
        counter.increment();
        assert_eq!(counter.get(), 2);
        counter.reset();
        assert_eq!(counter.get(), 0);
    }

    /// The discriminating case: a counter-based assertion catches a
    /// regression a wall-clock ratio assertion could miss (or flake on) —
    /// it never observes "cheap but nonzero", only exact call count.
    #[test]
    fn deterministic_counter_discriminates_any_nonzero_call_count() {
        let counter = DeterministicCounter::new();
        counter.reset();
        // Simulate a correct O(1) path: zero calls.
        assert_eq!(
            counter.get(),
            0,
            "a correct O(1) path must move the counter by zero"
        );
        // Simulate a regressed per-item path: N calls for N items.
        for _ in 0..5 {
            counter.increment();
        }
        assert_eq!(
            counter.get(),
            5,
            "a regressed per-item path must be caught by the exact count, at any \
             host size — a wall-clock ratio can be lost in noise; a count cannot"
        );
    }
}
