//! Require-mode (fail-closed) provider gating for the real-provider test
//! harness: the provider-kind identity and the absent-provider policy.
//!
//! Split from [`crate::test_harness`] so the pure decision surface
//! (`provider_absence_outcome`) stays independently unit-testable and the
//! harness file stays within the production size guard.

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
}

impl BodyReceiptStatus {
    /// The machine-greppable token stamped into the receipt line.
    pub(crate) fn token(self) -> &'static str {
        match self {
            BodyReceiptStatus::BodyReturned => "body-returned",
            BodyReceiptStatus::SkippedWarmup => "SKIPPED-WARMUP",
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
    let status = body_receipt_status(skip_reason);
    let mut line = format!(
        "RECEIPT real-provider test={test} provider={provider_label} require_mode={} status={}",
        u8::from(require_mode),
        status.token(),
    );
    if let Some(reason) = skip_reason {
        line.push_str(" reason=");
        line.push_str(reason);
    }
    line
}

/// Read the require-mode env var for a provider kind (`"1"` ⇒ required).
pub(crate) fn provider_required(kind: TestProviderKind) -> bool {
    std::env::var(kind.require_env())
        .map(|v| v == "1")
        .unwrap_or(false)
}

/// Resolve an absent-provider situation: under require-mode this PANICS (the
/// fail-closed gate); otherwise it prints a skip marker and returns `None` so
/// the caller returns early. A skip is never reported as a pass.
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
            eprintln!("skipping ({}): {reason}", kind.label());
            None
        }
    }
}
