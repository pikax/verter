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
