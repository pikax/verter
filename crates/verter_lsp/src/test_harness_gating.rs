//! Fail-closed provider gating for the real-provider test harness: the
//! provider-kind identity, and the policy for a test that obtained no session.
//!
//! Two orthogonal inputs decide that policy, and both matter:
//!
//! - the CAUSE ([`ProviderUnavailable`]) — was the engine merely absent, or was
//!   it found and then crashed;
//! - the run's require-mode (`VERTER_REQUIRE_{TSGO,TSSERVER}=1`, set by CI).
//!
//! A crash fails on every run; an absence fails only when the run requires the
//! engine. Folding the cause away is what let an entire provider lane report
//! passes on a platform where not one provider process ever started.
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

/// WHY a real-provider test obtained no session. The two causes are NOT
/// interchangeable, and collapsing them is what made an entire provider lane
/// vacuous on one platform while reporting green.
///
/// A machine that does not have the engine is a legitimate platform absence: the
/// test has nothing to run against and skipping is the honest answer (unless the
/// run declares the engine required). An engine that was FOUND, started, and
/// then died is a different event entirely — the platform HAS the provider, so
/// something about the environment or the harness is broken, and reporting that
/// as "provider not available" launders a fault into an expected condition.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ProviderUnavailable {
    /// Nothing was ever started: discovery found no engine, or the toolchain
    /// support policy REFUSED every candidate it found. A platform absence.
    NotFound,
    /// An engine was discovered and a process WAS started, and it failed to
    /// become usable — a spawn or `--api` attach crash. An environment or
    /// harness fault, never a platform absence.
    SpawnFailed,
}

/// What "no session" means for a real-provider test: a HARD failure, or a
/// graceful skip. Pure so both branches are unit-tested regardless of whether
/// the provider happens to be installed on the running machine.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ProviderAbsence {
    /// The test must FAIL (never skip-as-pass): either the run requires the
    /// provider, or the provider was found and CRASHED.
    HardFail,
    /// A genuine platform absence on a run that does not require the provider —
    /// record a skip and degrade gracefully.
    SkipWithReason,
}

/// Pure decision: given WHY no session was obtained and whether the run requires
/// the provider, how should that be handled.
///
/// A [`ProviderUnavailable::SpawnFailed`] cause is a HARD failure on every run,
/// require-mode or not. This is deliberately NOT a require-mode question: the
/// require env answers "must this machine have the engine", and a crashed spawn
/// has already answered that YES — the engine was there. Routing it through the
/// require gate is what let a Windows-only exec-boundary defect report 93 green
/// passes for tests that never started a single provider process, while CI
/// (which sets the require env) stayed green and honest and so never contradicted
/// them.
pub(crate) fn provider_absence_outcome(
    cause: ProviderUnavailable,
    required: bool,
) -> ProviderAbsence {
    match cause {
        ProviderUnavailable::SpawnFailed => ProviderAbsence::HardFail,
        ProviderUnavailable::NotFound => {
            if required {
                ProviderAbsence::HardFail
            } else {
                ProviderAbsence::SkipWithReason
            }
        }
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
    /// No session was ever built because no engine EXISTS here: the provider was
    /// absent, or REJECTED by the toolchain support policy (e.g. a below-floor
    /// tsgo). The body never started, so it can attest nothing.
    SkippedNoProvider,
    /// No session was built because an engine that DOES exist here was started
    /// and crashed. Distinct from [`Self::SkippedNoProvider`] because it is not a
    /// skip at all: the test FAILS. The receipt is still minted, before the
    /// failure, so a log scan can separate the fault from a platform absence
    /// instead of finding the two under one token.
    SpawnCrashed,
}

impl BodyReceiptStatus {
    /// The machine-greppable token stamped into the receipt line.
    pub(crate) fn token(self) -> &'static str {
        match self {
            BodyReceiptStatus::BodyReturned => "body-returned",
            BodyReceiptStatus::SkippedWarmup => "SKIPPED-WARMUP",
            BodyReceiptStatus::SkippedNoProvider => "SKIPPED-NO-PROVIDER",
            BodyReceiptStatus::SpawnCrashed => "SPAWN-CRASHED",
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

/// Render the receipt for a test whose engine was FOUND and then crashed at
/// spawn or attach.
///
/// A separate token from `SKIPPED-NO-PROVIDER` because the two demand opposite
/// responses: one means "this machine cannot run this test", the other means
/// "this machine can, and something is broken". Under one token a whole lane's
/// spawn failures are indistinguishable from a machine without the engine
/// installed, which is exactly how a Windows-only exec-boundary defect stayed
/// invisible while every affected test reported a pass. The receipt is minted
/// even though the test then FAILS: the failure names one test, the receipt makes
/// the class greppable across the whole run.
pub(crate) fn spawn_crashed_receipt_line(
    test: &str,
    provider_label: &str,
    require_mode: bool,
    reason: &str,
) -> String {
    receipt_line(
        test,
        provider_label,
        require_mode,
        BodyReceiptStatus::SpawnCrashed,
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

/// Resolve a "no session" situation. A genuine platform absence on a permissive
/// run emits the `SKIPPED-NO-PROVIDER` receipt and returns `None` so the caller
/// returns early; every other case PANICS — a required-but-missing provider, and
/// ANY spawn/attach crash regardless of require-mode. A skip is never reported as
/// a pass, and a crash is never reported as a skip.
///
/// This is the SINGLE funnel every `None` return out of
/// [`crate::test_harness::TestSessionBuilder::build`] passes through
/// (discovery miss, policy rejection, spawn failure, `--api` attach failure),
/// so the disposition HERE covers every real-provider test — the
/// macro-generated ones and the hand-written ones alike — without any per-test
/// opt-in a new test could forget. `cause` is what the funnel cannot infer and
/// the call site knows for free: whether a process was ever started.
///
/// Split from the env read (`provider_required`) so the panic-vs-skip policy
/// (`provider_absence_outcome`) is independently unit testable.
pub(crate) fn handle_absent_provider(
    kind: TestProviderKind,
    cause: ProviderUnavailable,
    reason: &str,
) -> Option<RealProviderTestSession> {
    // Mint the receipt BEFORE dispositioning, so a crash is greppable in the log
    // even on the path that panics immediately after.
    if let Some(receipt) = provider_unavailable_receipt(kind, cause, reason) {
        eprintln!("{receipt}");
    }
    match provider_absence_outcome(cause, provider_required(kind)) {
        ProviderAbsence::HardFail => panic!("{}", hard_fail_message(kind, cause, reason)),
        ProviderAbsence::SkipWithReason => None,
    }
}

/// Why the run is failing, in the terms of the situation that caused it.
///
/// The two messages must not be interchangeable: a require-mode absence tells the
/// reader to install the engine, whereas a crash tells them the engine is already
/// here and something else is wrong. Handing a developer the "install it" message
/// for a value-mangling defect in the spawn path is how the class survives.
fn hard_fail_message(kind: TestProviderKind, cause: ProviderUnavailable, reason: &str) -> String {
    match cause {
        ProviderUnavailable::NotFound => format!(
            "{}=1 but the {} real-provider test cannot run: {reason}",
            kind.require_env(),
            kind.label(),
        ),
        ProviderUnavailable::SpawnFailed => format!(
            "the {} real-provider engine was DISCOVERED on this machine and then failed to \
             start: {reason}. A spawn/attach crash is an environment or harness fault, not a \
             platform absence, so it FAILS the test on every run — {} does not gate it and \
             cannot silence it. Fix the spawn (or the value handed to it); do not skip.",
            kind.label(),
            kind.require_env(),
        ),
    }
}

/// The receipt line a "no session" situation must emit, or `None` when there is
/// nothing to attest — a require-mode ABSENCE, which panics instead of skipping
/// and must not advertise a tolerated skip on exactly the run that must fail.
///
/// A spawn crash always emits, on every run: it is the only record that the
/// engine was present and the fault was ours.
///
/// Pure, so the funnel's own decision is unit-testable without capturing stderr
/// and without a provider installed on the running machine.
pub(crate) fn provider_unavailable_receipt(
    kind: TestProviderKind,
    cause: ProviderUnavailable,
    reason: &str,
) -> Option<String> {
    let required = provider_required(kind);
    match cause {
        ProviderUnavailable::SpawnFailed => Some(spawn_crashed_receipt_line(
            &current_test_identity(),
            kind.label(),
            required,
            reason,
        )),
        ProviderUnavailable::NotFound => match provider_absence_outcome(cause, required) {
            ProviderAbsence::HardFail => None,
            ProviderAbsence::SkipWithReason => Some(absent_provider_receipt_line(
                &current_test_identity(),
                kind.label(),
                false,
                reason,
            )),
        },
    }
}
