//! Inline unit tests extracted from `test_harness.rs` (sibling test module).
//!
//! Wired back as `#[cfg(test)] #[path = "test_harness_tests.rs"] mod test_harness_tests;`
//! at the tail of `test_harness.rs`. Kept in a `_tests.rs` sibling so the harness
//! source stays under the `no_oversize_files` line budget; the tests run unchanged
//! via the `super` glob.

use super::*;

// ---------------------------------------------------------------------------
// Require-mode fail-closed tests
// ---------------------------------------------------------------------------
//
// These prove the harness is FAIL-CLOSED: an absent provider under require-mode
// is a HARD failure, never a skip-as-pass (the exact vacuity class a prior fix
// found and removed). They exercise the pure policy + the harness build path
// with a guaranteed-absent provider, so they discriminate regardless of whether
// a provider happens to be installed on the running machine.

/// The require decision is non-vacuous: requiring a provider turns its absence
/// into a HARD failure, so a provider-absent CI run can never report the gate
/// green by skipping. Both branches are covered (pure function — no provider
/// needed on the machine).
#[test]
fn provider_absence_is_hard_fail_when_required_else_skip() {
    assert_eq!(
        provider_absence_outcome(true),
        ProviderAbsence::HardFail,
        "a required-but-absent provider must FAIL the test, not skip"
    );
    assert_eq!(
        provider_absence_outcome(false),
        ProviderAbsence::SkipWithReason,
        "a non-required absent provider degrades to a graceful skip"
    );
}

/// `handle_absent_provider` PANICS (fail-closed) when the provider's require
/// env is set — proven by forcing tgo absent via the require env regardless of
/// whether tgo is installed. Reverting the require check (always-skip) makes
/// this test stop panicking, so it is discriminating.
///
/// Serialized via a process-global mutex because it mutates a process env var,
/// which other env-reading tests in this binary could observe.
#[test]
fn handle_absent_provider_fails_closed_under_require_env() {
    let _guard = require_env_test_lock().lock().unwrap();
    let key = TestProviderKind::Tsgo.require_env();
    let prev = std::env::var_os(key);
    std::env::set_var(key, "1");

    let outcome = std::panic::catch_unwind(|| {
        // Same-thread: the env var set above is visible. Forces the absent path.
        handle_absent_provider(
            TestProviderKind::Tsgo,
            "forced-absent for fail-closed proof",
        )
    });

    // Restore env before asserting so a failure cannot leak the override.
    match prev {
        Some(v) => std::env::set_var(key, v),
        None => std::env::remove_var(key),
    }

    assert!(
        outcome.is_err(),
        "VERTER_REQUIRE_TSGO=1 with an absent provider must PANIC (fail-closed), not return a skip"
    );
}

/// Without the require env set, an absent provider degrades to a graceful skip
/// (`None`) — the non-CI developer ergonomics path. Pairs with the test above
/// to pin both halves of the gate.
#[test]
fn handle_absent_provider_skips_without_require_env() {
    let _guard = require_env_test_lock().lock().unwrap();
    let key = TestProviderKind::Tsgo.require_env();
    let prev = std::env::var_os(key);
    std::env::remove_var(key);

    let result = handle_absent_provider(TestProviderKind::Tsgo, "absent, not required");

    match prev {
        Some(v) => std::env::set_var(key, v),
        None => std::env::remove_var(key),
    }

    assert!(
        result.is_none(),
        "an absent, non-required provider must return None (skip), not a session"
    );
}

/// `handle_absent_provider` PANICS (fail-closed) when `VERTER_REQUIRE_TSSERVER`
/// is set — the symmetric counterpart of the tgo gate, proven by forcing the
/// tsserver kind absent via its require env regardless of whether tsserver is
/// installed. Reverting the require check (always-skip) makes this test stop
/// panicking, so it is discriminating.
///
/// Serialized via the same process-global mutex as the tgo require tests because
/// it mutates a process env var that other env-reading tests could observe.
#[test]
fn tsserver_handle_absent_provider_fails_closed_under_require_env() {
    let _guard = require_env_test_lock().lock().unwrap();
    let key = TestProviderKind::Tsserver.require_env();
    assert_eq!(
        key, "VERTER_REQUIRE_TSSERVER",
        "the tsserver require knob must be VERTER_REQUIRE_TSSERVER (symmetric with tgo)"
    );
    let prev = std::env::var_os(key);
    std::env::set_var(key, "1");

    let outcome = std::panic::catch_unwind(|| {
        // Same-thread: the env var set above is visible. Forces the absent path.
        handle_absent_provider(
            TestProviderKind::Tsserver,
            "forced-absent for fail-closed proof",
        )
    });

    // Restore env before asserting so a failure cannot leak the override.
    match prev {
        Some(v) => std::env::set_var(key, v),
        None => std::env::remove_var(key),
    }

    assert!(
        outcome.is_err(),
        "VERTER_REQUIRE_TSSERVER=1 with an absent provider must PANIC (fail-closed), not return a skip"
    );
}

/// Without `VERTER_REQUIRE_TSSERVER` set, an absent tsserver degrades to a
/// graceful skip (`None`) — the non-CI developer ergonomics path. Pairs with the
/// test above to pin both halves of the tsserver gate.
#[test]
fn tsserver_handle_absent_provider_skips_without_require_env() {
    let _guard = require_env_test_lock().lock().unwrap();
    let key = TestProviderKind::Tsserver.require_env();
    let prev = std::env::var_os(key);
    std::env::remove_var(key);

    let result = handle_absent_provider(TestProviderKind::Tsserver, "absent, not required");

    match prev {
        Some(v) => std::env::set_var(key, v),
        None => std::env::remove_var(key),
    }

    assert!(
        result.is_none(),
        "an absent, non-required tsserver must return None (skip), not a session"
    );
}

/// Process-global lock so the env-mutating require-mode tests do not race each
/// other (or any other env-reading test) within this test binary.
fn require_env_test_lock() -> &'static std::sync::Mutex<()> {
    static LOCK: std::sync::OnceLock<std::sync::Mutex<()>> = std::sync::OnceLock::new();
    LOCK.get_or_init(|| std::sync::Mutex::new(()))
}

// ---------------------------------------------------------------------------
// Per-session carrier-store isolation
// ---------------------------------------------------------------------------
//
// These pin the isolation mechanism that makes the real-provider tsserver tests
// deterministic in a shared run: each session gets a UNIQUE carrier-store dir, so
// no manifest/blob state leaks from an earlier session into a later session's cold
// read. They run without a provider (pure path-derivation + the test-only segment
// override), so they discriminate on every machine.

/// Two sessions over the SAME fixture workspace root must resolve to DIFFERENT
/// carrier-store dirs. Before the isolation fix the dir was keyed purely on
/// `(host_version, workspace_root)`, so two same-fixture sessions collided on one
/// dir (the leak). [`unique_store_segment`] gives each session its own segment, so
/// the dirs differ even for an identical workspace root — reverting to a constant
/// segment makes this assertion fail.
#[test]
fn distinct_sessions_get_distinct_carrier_store_dirs_for_same_workspace() {
    let workspace_root = "/some/fixture/workspace-root";

    let seg_a = unique_store_segment();
    let seg_b = unique_store_segment();
    assert_ne!(
        seg_a, seg_b,
        "each session must mint a UNIQUE store-dir segment, else same-fixture sessions collide"
    );

    let dir_a = crate::external_ts::carrier_store_dir_for(&seg_a, workspace_root);
    let dir_b = crate::external_ts::carrier_store_dir_for(&seg_b, workspace_root);
    assert_ne!(
        dir_a, dir_b,
        "two sessions over the same workspace root must get isolated (different) store dirs"
    );

    // The unique segment keeps the package-version prefix so the per-session trees
    // still cluster under the version (production layout), and is a PORTABLE path
    // segment (no NTFS-illegal chars) so the dir is creatable on every platform.
    assert!(
        seg_a.starts_with(env!("CARGO_PKG_VERSION")),
        "the session segment must keep the package-version prefix: {seg_a}"
    );
    assert!(
        seg_a
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '-')),
        "the session segment must be a portable path component: {seg_a}"
    );
}

/// Installing a session segment override moves BOTH the LSP-side publish backend's
/// store dir (`carrier_store_dir_for(default_carrier_store_host_version(), …)`,
/// which is what [`crate::external_ts::TsserverEngineBackend::with_default_host_version`]
/// uses) AND the tsserver spawn-dir string (`default_carrier_store_dir_string`)
/// onto the SAME isolated dir — that shared read is what keeps the publish side and
/// the plugin side in agreement on the per-session dir. Clearing restores the live
/// package-version segment, so production is unaffected.
#[test]
fn store_dir_override_aligns_spawn_and_backend_then_restores() {
    // Serialize with any concurrent session construction (and the other override
    // test) so the process-global override read here is not observed mid-flight.
    let _guard = crate::external_ts::test_store_dir_override::install_lock();

    let workspace_root = "/some/fixture/workspace-root";
    let segment = unique_store_segment();

    // No override installed ⇒ the live package-version segment.
    crate::external_ts::test_store_dir_override::clear();
    assert_eq!(
        crate::external_ts::default_carrier_store_host_version(),
        env!("CARGO_PKG_VERSION"),
        "with no override the segment must be the live package version (production unaffected)"
    );

    crate::external_ts::test_store_dir_override::set(&segment);
    assert_eq!(
        crate::external_ts::default_carrier_store_host_version(),
        segment,
        "an installed override must be the segment both sides read"
    );

    // Both sides resolve through `default_carrier_store_host_version`, so they land
    // on the SAME isolated dir for this session.
    let spawn_dir = crate::external_ts::default_carrier_store_dir_string(workspace_root);
    let backend_dir = crate::external_ts::carrier_store_dir_for(
        crate::external_ts::default_carrier_store_host_version(),
        workspace_root,
    );
    assert_eq!(
        spawn_dir,
        backend_dir.to_string_lossy().replace('\\', "/"),
        "spawn env dir and LSP backend dir must agree under the installed override"
    );
    assert!(
        spawn_dir.contains(&segment),
        "the resolved per-session dir must contain the unique segment: {spawn_dir}"
    );

    crate::external_ts::test_store_dir_override::clear();
    assert_eq!(
        crate::external_ts::default_carrier_store_host_version(),
        env!("CARGO_PKG_VERSION"),
        "clearing the override must restore the live package-version segment"
    );
}
