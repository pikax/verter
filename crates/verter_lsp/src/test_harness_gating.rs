//! Require-mode (fail-closed) provider gating for the real-provider test
//! harness: the provider-kind identity and the absent-provider policy.
//!
//! Split from [`crate::test_harness`] so the pure decision surface
//! (`provider_absence_outcome`) stays independently unit-testable and harness
//! orchestration remains focused.

use crate::test_harness::RealProviderTestSession;

// ---------------------------------------------------------------------------
// Provider kind
// ---------------------------------------------------------------------------

/// Which real type provider to spawn.
#[derive(Debug, Clone, Copy)]
pub(crate) enum TestProviderKind {
    Tsserver,
    Tsgo,
}

impl TestProviderKind {
    /// The require-mode env var that turns this provider's absence into a HARD
    /// failure instead of a graceful skip. CI sets `VERTER_REQUIRE_TSGO=1` (see
    /// `.github/workflows/ci.yml`), so the tsgo real-provider parity tests
    /// genuinely gate there and can never skip-as-pass on a runner where the
    /// asset is expected. `VERTER_REQUIRE_TSSERVER` is the analogous knob for
    /// the tsserver variant.
    pub(crate) fn require_env(self) -> &'static str {
        match self {
            TestProviderKind::Tsserver => "VERTER_REQUIRE_TSSERVER",
            TestProviderKind::Tsgo => "VERTER_REQUIRE_TSGO",
        }
    }

    pub(crate) fn label(self) -> &'static str {
        match self {
            TestProviderKind::Tsserver => "tsserver",
            TestProviderKind::Tsgo => "tsgo",
        }
    }
}

// ---------------------------------------------------------------------------
// Require-mode (fail-closed) provider gating
// ---------------------------------------------------------------------------

/// What an absent provider means for a real-provider test: a HARD failure when
/// the run requires that provider (`VERTER_REQUIRE_{TSGO,TSSERVER}=1`, e.g.
/// strict CI), else a graceful skip. Pure so both branches are unit-tested
/// regardless of whether the provider happens to be installed on the running
/// machine.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ProviderAbsence {
    /// Required but missing — the test must FAIL (never skip-as-pass).
    HardFail,
    /// Not required — record a skip and degrade gracefully.
    SkipWithReason,
}

/// Pure decision: given whether the provider is required, how should its
/// absence be handled.
pub(crate) fn provider_absence_outcome(required: bool) -> ProviderAbsence {
    if required {
        ProviderAbsence::HardFail
    } else {
        ProviderAbsence::SkipWithReason
    }
}

// ---------------------------------------------------------------------------
// Body receipt status
// ---------------------------------------------------------------------------

/// The terminal status a real-provider test body EARNS.
///
/// A receipt is an attestation that the body's assertions ran against a live
/// provider. Returning from the body is NOT that proof — a body that hit a
/// documented degradation path (cold provider warmup, an empty controlled
/// result) also returns, and returning is exactly what it does. The status is
/// therefore derived from the session's recorded degradation ledger, never from
/// control flow reaching the end of the generated test function.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum BodyReceiptStatus {
    /// The body completed with NO recorded degradation: its assertions ran.
    BodyReturned,
    /// The body recorded a degradation and never reached its assertions.
    SkippedWarmup,
    /// No session was ever built: the provider was absent, unspawnable, or
    /// REJECTED by the toolchain support policy (e.g. a below-floor tsgo). The
    /// body never started, so it can attest nothing.
    SkippedNoProvider,
}

impl BodyReceiptStatus {
    /// The machine-greppable token stamped into the receipt line.
    pub(crate) fn token(self) -> &'static str {
        match self {
            BodyReceiptStatus::BodyReturned => "body-returned",
            BodyReceiptStatus::SkippedWarmup => "SKIPPED-WARMUP",
            BodyReceiptStatus::SkippedNoProvider => "SKIPPED-NO-PROVIDER",
        }
    }
}

/// Pure derivation: a recorded degradation ALWAYS wins over "the body returned".
/// Keeping it pure is what makes the non-vacuity property unit-testable without
/// a live provider.
pub(crate) fn body_receipt_status(skip_reason: Option<&str>) -> BodyReceiptStatus {
    match skip_reason {
        Some(_) => BodyReceiptStatus::SkippedWarmup,
        None => BodyReceiptStatus::BodyReturned,
    }
}

/// Render one receipt line in the single shared format.
///
/// The result is ALWAYS a single line: a receipt is a machine-greppable
/// attestation, and a reason that wraps (the tsgo resolver's multi-candidate
/// rejection report is several lines) would leave a scanner reading only the
/// head and losing the rejection detail. Interior line breaks collapse to
/// `" | "`.
fn receipt_line(
    test: &str,
    provider_label: &str,
    require_mode: bool,
    status: BodyReceiptStatus,
    reason: Option<&str>,
) -> String {
    let mut line = format!(
        "RECEIPT real-provider test={test} provider={provider_label} require_mode={} status={}",
        u8::from(require_mode),
        status.token(),
    );
    if let Some(reason) = reason {
        line.push_str(" reason=");
        line.push_str(&single_line(reason));
    }
    line
}

/// Collapse a reason to one line: split on line breaks, trim each fragment,
/// drop the empties, and join with `" | "`.
fn single_line(reason: &str) -> String {
    reason
        .lines()
        .map(str::trim)
        .filter(|fragment| !fragment.is_empty())
        .collect::<Vec<_>>()
        .join(" | ")
}

/// Render the single end-of-body receipt line for a test.
///
/// One receipt per generated test, whatever the outcome — a skipped body emits
/// `status=SKIPPED-WARMUP` with its reason, never `status=body-returned`.
pub(crate) fn body_receipt_line(
    test: &str,
    provider_label: &str,
    require_mode: bool,
    skip_reason: Option<&str>,
) -> String {
    receipt_line(
        test,
        provider_label,
        require_mode,
        body_receipt_status(skip_reason),
        skip_reason,
    )
}

/// Render the receipt for a test that never obtained a session because the
/// provider was absent, unspawnable, or policy-REJECTED.
///
/// This is the receipt the rail was missing. Without it a real-provider test
/// whose engine failed discovery returned from its `let Some(session) = … else
/// { return; }` guard and libtest reported an ORDINARY PASS — indistinguishable
/// from a run whose assertions executed. A wrong-version tsgo (the support
/// policy refuses anything outside stable `>=7.0.2, <7.1.0`) is exactly that
/// case, so a green local run proved nothing while CI — which sets
/// `VERTER_REQUIRE_TSGO=1` and therefore takes the panic branch — failed.
///
/// The status is a DISTINCT token from `SKIPPED-WARMUP`: a warmup skip means a
/// live provider existed and the body still degraded, whereas this means no
/// engine was ever obtained. Conflating them would hide an environment fault
/// inside the ordinary-degradation bucket.
pub(crate) fn absent_provider_receipt_line(
    test: &str,
    provider_label: &str,
    require_mode: bool,
    reason: &str,
) -> String {
    receipt_line(
        test,
        provider_label,
        require_mode,
        BodyReceiptStatus::SkippedNoProvider,
        Some(reason),
    )
}

/// The running test's identity for a receipt line.
///
/// libtest names the thread it runs each test on after the test's full path, so
/// under both `cargo test` and `cargo nextest` (one test per process) this is
/// the test name. It is best-effort by nature — a receipt emitted off a
/// harness-spawned thread reports `<unknown>` rather than lying about which
/// test skipped.
pub(crate) fn current_test_identity() -> String {
    std::thread::current()
        .name()
        .unwrap_or("<unknown>")
        .to_string()
}

/// Read the require-mode env var for a provider kind (`"1"` ⇒ required).
pub(crate) fn provider_required(kind: TestProviderKind) -> bool {
    std::env::var(kind.require_env())
        .map(|v| v == "1")
        .unwrap_or(false)
}

/// Resolve an absent-provider situation: under require-mode this PANICS (the
/// fail-closed gate); otherwise it emits the `SKIPPED-NO-PROVIDER` receipt and
/// returns `None` so the caller returns early. A skip is never reported as a
/// pass.
///
/// This is the SINGLE funnel every `None` return out of
/// [`crate::test_harness::TestSessionBuilder::build`] passes through
/// (discovery miss, policy rejection, spawn failure, `--api` attach failure),
/// so emitting the receipt HERE covers every real-provider test — the
/// macro-generated ones and the hand-written ones alike — without any per-test
/// opt-in a new test could forget.
///
/// Split from the env read (`provider_required`) so the panic-vs-skip policy
/// (`provider_absence_outcome`) is independently unit testable.
pub(crate) fn handle_absent_provider(
    kind: TestProviderKind,
    reason: &str,
) -> Option<RealProviderTestSession> {
    match provider_absence_outcome(provider_required(kind)) {
        ProviderAbsence::HardFail => panic!(
            "{}=1 but the {} real-provider test cannot run: {reason}",
            kind.require_env(),
            kind.label(),
        ),
        ProviderAbsence::SkipWithReason => {
            if let Some(receipt) = absent_provider_skip_receipt(kind, reason) {
                eprintln!("{receipt}");
            }
            None
        }
    }
}

/// The receipt line an absent-provider SKIP must emit, or `None` when the
/// absence is a require-mode HARD FAILURE (which panics instead of skipping).
///
/// Pure, so the funnel's own decision — "a skip always emits a named
/// `SKIPPED-NO-PROVIDER` receipt" — is unit-testable without capturing stderr
/// and without a provider installed on the running machine.
pub(crate) fn absent_provider_skip_receipt(kind: TestProviderKind, reason: &str) -> Option<String> {
    match provider_absence_outcome(provider_required(kind)) {
        ProviderAbsence::HardFail => None,
        ProviderAbsence::SkipWithReason => Some(absent_provider_receipt_line(
            &current_test_identity(),
            kind.label(),
            false,
            reason,
        )),
    }
}
