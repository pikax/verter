//! Inline unit tests extracted from `test_harness.rs` (sibling test module).
//!
//! Wired back as `#[cfg(test)] #[path = "test_harness_tests.rs"] mod test_harness_tests;`
//! at the tail of `test_harness.rs`. Kept in a `_tests.rs` sibling so the harness
//! source stays small and readable; the tests run unchanged via the `super`
//! glob.

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
/// env is set — proven by forcing tsgo absent via the require env regardless of
/// whether tsgo is installed. Reverting the require check (always-skip) makes
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
    // Under require-mode there is no skip to attest — the absence PANICS, so a
    // skip receipt must not be minted (it would advertise a tolerated skip on
    // exactly the run that must fail).
    let receipt = absent_provider_skip_receipt(TestProviderKind::Tsgo, "forced-absent");

    // Restore env before asserting so a failure cannot leak the override.
    match prev {
        Some(v) => std::env::set_var(key, v),
        None => std::env::remove_var(key),
    }

    assert!(
        outcome.is_err(),
        "VERTER_REQUIRE_TSGO=1 with an absent provider must PANIC (fail-closed), not return a skip"
    );
    assert!(
        receipt.is_none(),
        "require-mode absence is a hard failure, never a receipted skip: {receipt:?}"
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
    // The SAME funnel decision `handle_absent_provider` prints, captured as a
    // value so the receipt is provable without intercepting stderr.
    let receipt = absent_provider_skip_receipt(TestProviderKind::Tsgo, "absent, not required");

    match prev {
        Some(v) => std::env::set_var(key, v),
        None => std::env::remove_var(key),
    }

    assert!(
        result.is_none(),
        "an absent, non-required provider must return None (skip), not a session"
    );
    // A skip is never an ORDINARY pass: the funnel emits a named, greppable
    // receipt so a run whose engine was absent or policy-rejected can be told
    // apart from one whose assertions executed. Dropping the receipt (the
    // pre-fix behaviour) makes this fail.
    let receipt = receipt.expect("a non-required absent provider must emit a skip receipt");
    assert!(
        receipt.contains("status=SKIPPED-NO-PROVIDER")
            && receipt.contains("reason=absent, not required")
            && receipt.contains("provider=tsgo"),
        "the skip receipt must name the status, the provider and the reason: {receipt}"
    );
}

/// `handle_absent_provider` PANICS (fail-closed) when `VERTER_REQUIRE_TSSERVER`
/// is set — the symmetric counterpart of the tsgo gate, proven by forcing the
/// tsserver kind absent via its require env regardless of whether tsserver is
/// installed. Reverting the require check (always-skip) makes this test stop
/// panicking, so it is discriminating.
///
/// Serialized via the same process-global mutex as the tsgo require tests because
/// it mutates a process env var that other env-reading tests could observe.
#[test]
fn tsserver_handle_absent_provider_fails_closed_under_require_env() {
    let _guard = require_env_test_lock().lock().unwrap();
    let key = TestProviderKind::Tsserver.require_env();
    assert_eq!(
        key, "VERTER_REQUIRE_TSSERVER",
        "the tsserver require knob must be VERTER_REQUIRE_TSSERVER (symmetric with tsgo)"
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
    let receipt = absent_provider_skip_receipt(TestProviderKind::Tsserver, "absent, not required");

    match prev {
        Some(v) => std::env::set_var(key, v),
        None => std::env::remove_var(key),
    }

    assert!(
        result.is_none(),
        "an absent, non-required tsserver must return None (skip), not a session"
    );
    let receipt = receipt.expect("a non-required absent tsserver must emit a skip receipt");
    assert!(
        receipt.contains("status=SKIPPED-NO-PROVIDER") && receipt.contains("provider=tsserver"),
        "the tsserver skip receipt must name the status and the provider: {receipt}"
    );
}

// ---------------------------------------------------------------------------
// Receipt non-vacuity
// ---------------------------------------------------------------------------
//
// The receipt is an attestation that a body's assertions ran against a live
// provider. Returning from the body is NOT that proof: a body that `return`ed at
// a warmup guard also returns. These pin the derivation that keeps the two
// apart — the status comes from the recorded degradation, never from control
// flow reaching the end of the generated test function.

/// A recorded degradation can NEVER mint a `body-returned` receipt, and it
/// carries its reason. Deriving the status from "the body returned" instead
/// (the vacuity the receipts previously hid) makes both assertions fail.
#[test]
fn a_recorded_degradation_receipt_is_skipped_never_body_returned() {
    let line = body_receipt_line(
        "some_test_tsgo",
        "tsgo",
        true,
        Some("provider-not-warmed-up"),
    );

    assert!(
        line.contains("status=SKIPPED-WARMUP"),
        "a body that degraded must report SKIPPED-WARMUP: {line}"
    );
    assert!(
        !line.contains("body-returned"),
        "a degraded body must NEVER attest body-returned — that is the vacuity: {line}"
    );
    assert!(
        line.contains("reason=provider-not-warmed-up"),
        "the receipt must carry WHY the body stopped short: {line}"
    );
}

/// Only a body with NOTHING recorded earns `body-returned`, and the require-mode
/// flag is stamped so a receipt scan can tell a genuinely gated run apart from a
/// permissive local one.
#[test]
fn an_undegraded_body_earns_body_returned_with_the_require_flag() {
    let required = body_receipt_line("some_test_tsgo", "tsgo", true, None);
    assert!(
        required.contains("status=body-returned") && required.contains("require_mode=1"),
        "an undegraded require-mode body must attest body-returned: {required}"
    );
    assert!(
        !required.contains("reason="),
        "an undegraded body has no skip reason to report: {required}"
    );

    let permissive = body_receipt_line("some_test_tsgo", "tsgo", false, None);
    assert!(
        permissive.contains("require_mode=0"),
        "the require-mode flag must reflect the run: {permissive}"
    );
}

/// The status derivation itself is the non-vacuity rule: ANY recorded reason is
/// a skip, and only the absence of one is a completed body. Pure, so it
/// discriminates on every machine with no provider installed.
#[test]
fn body_receipt_status_is_decided_by_the_degradation_ledger() {
    assert_eq!(
        body_receipt_status(Some("anything at all")),
        BodyReceiptStatus::SkippedWarmup,
        "a recorded degradation must classify as a skip"
    );
    assert_eq!(
        body_receipt_status(None),
        BodyReceiptStatus::BodyReturned,
        "an empty ledger is the only thing that earns body-returned"
    );
    assert_ne!(
        BodyReceiptStatus::SkippedWarmup.token(),
        BodyReceiptStatus::BodyReturned.token(),
        "the two statuses must be distinguishable in a receipt scan"
    );
}

/// A test that never obtained a session (absent, unspawnable, or POLICY-REJECTED
/// engine — e.g. a below-floor tsgo) must attest a DISTINCT
/// `SKIPPED-NO-PROVIDER` status carrying the reason, never `body-returned` and
/// never the warmup bucket.
///
/// This is the hole that made a wrong-version tsgo read as an ordinary green
/// pass: the builder returned `None`, the test's `let Some(session) = … else {
/// return; }` guard returned, and NO receipt was emitted at all. Reverting the
/// receipt (or folding it into `SKIPPED-WARMUP`) makes these assertions fail.
#[test]
fn an_absent_or_rejected_provider_earns_a_distinct_no_provider_receipt() {
    let line = absent_provider_receipt_line(
        "carrier_dx_enhanced_both_engines_both_frameworks_tsgo",
        "tsgo",
        false,
        "tsgo binary not found: no usable tsgo engine found for the `--lsp` surface",
    );

    assert!(
        line.contains("status=SKIPPED-NO-PROVIDER"),
        "a test with no engine must report SKIPPED-NO-PROVIDER: {line}"
    );
    assert!(
        !line.contains("body-returned"),
        "a test that never got a session must NEVER attest body-returned: {line}"
    );
    assert!(
        !line.contains("SKIPPED-WARMUP"),
        "an absent engine is NOT a warmup degradation — the two must stay distinguishable: {line}"
    );
    assert!(
        line.contains("reason=tsgo binary not found"),
        "the receipt must carry WHY no engine was obtained: {line}"
    );
    assert!(
        line.contains("test=carrier_dx_enhanced_both_engines_both_frameworks_tsgo"),
        "the skip must be NAMED, not anonymous: {line}"
    );
}

/// A receipt is one greppable LINE even when the reason is the tsgo resolver's
/// multi-line multi-candidate rejection report — otherwise a scanner reads the
/// head and loses exactly the detail (which candidate, which policy) that
/// explains the skip.
#[test]
fn a_multi_line_reason_is_collapsed_into_one_greppable_receipt_line() {
    let reason = "no usable tsgo engine found for the `--lsp` surface.\n  - [VERTER_TSGO_BIN \
                  override] /x/tsgo: tsgo 7.0.0-dev.20260526.1 is a nightly prerelease\n\n  \
                  install a supported engine";
    let line = absent_provider_receipt_line("some_test_tsgo", "tsgo", false, reason);

    assert_eq!(
        line.lines().count(),
        1,
        "a receipt must be a single greppable line: {line:?}"
    );
    assert!(
        line.contains("nightly prerelease") && line.contains("install a supported engine"),
        "collapsing must PRESERVE every fragment of the reason, not truncate it: {line}"
    );
    assert!(
        line.contains(" | "),
        "the collapsed fragments must stay separated: {line}"
    );
}

/// All three receipt statuses render distinct machine-greppable tokens, so a
/// receipt scan can separate "assertions ran", "live provider but the body
/// degraded", and "no engine at all". Collapsing any two makes this fail.
#[test]
fn the_three_receipt_statuses_are_mutually_distinguishable() {
    let tokens = [
        BodyReceiptStatus::BodyReturned.token(),
        BodyReceiptStatus::SkippedWarmup.token(),
        BodyReceiptStatus::SkippedNoProvider.token(),
    ];
    for (i, a) in tokens.iter().enumerate() {
        for b in tokens.iter().skip(i + 1) {
            assert_ne!(a, b, "receipt statuses must be distinguishable in a scan");
        }
    }
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
