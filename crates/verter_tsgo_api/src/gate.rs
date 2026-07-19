//! Runtime fail-closed wire gate.
//!
//! Before a client is usable, the engine it spawned must be validated against
//! the maintained wire pin ([`crate::proto::schema_manifest::PINNED`]). The
//! gate compares two dimensions:
//!   1. the engine version string — validated against the SUPPORTED-VERSION
//!      POLICY ([`crate::toolchain::policy`]), not by equality with one build:
//!      [`classify_engine_version`] accepts stable `7.0.x` releases in the
//!      supported window (`>=7.0.2, <7.1.0`) and NOTHING else — RCs, betas,
//!      nightlies, and other minors/majors are refused in production. A
//!      DEV-ONLY env-gated override
//!      ([`crate::toolchain::policy::DEV_NIGHTLY_OVERRIDE_ENV`]) re-admits
//!      integer-handle nightlies for nightly gate testing. The pin's
//!      [`crate::proto::schema_manifest::SchemaManifest::engine_version`] is
//!      the reference build the codec was verified against, not the sole
//!      accepted version. The observed version comes from either an OWNED
//!      `--version` probe / discovered package
//!      ([`ObservedEngine::from_codec_wire`], witness
//!      [`EngineVersionWitness::VersionProbe`]) or a SHARED in-band
//!      `serverInfo` report ([`ObservedEngine::from_in_band_server_info`],
//!      witness [`EngineVersionWitness::InBandServerInfo`]); and
//!   2. the wire fingerprint — the codec's compiled-in fingerprint is always
//!      `PINNED.wire_fingerprint()`, so a fingerprint mismatch can only arise
//!      from an observed manifest assembled from a different engine.
//!
//! An UNKNOWN shape (a version outside the supported window, or a fingerprint
//! that does not match) makes the gate refuse to start: it returns a typed
//! [`crate::error::TsgoApiError::UnsupportedTsgoWire`], so a diverged tsgo never
//! silently ships.
//!
//! A version string can lie; the wire cannot. The gate therefore has a third,
//! version-independent rail: [`require_integer_snapshot_handle`] validates the
//! FIRST `updateSnapshot` response's snapshot handle as a bare JSON integer.
//! An engine whose version satisfies the policy but hands back a string
//! handle is refused at that first response, before any product result.
//!
//! Decoupling note: the gate does not itself spawn or probe the engine — it is
//! a pure validation function over an [`ObservedEngine`] the transport layer
//! supplies. This keeps it unit-testable without a live process and lets the
//! caller decide where the observed version comes from (a `--version` probe,
//! the discovered package.json, an in-band `serverInfo`, etc.).

use crate::error::{TsgoApiError, TsgoApiResult};
use crate::proto::schema_manifest::{SchemaManifest, PINNED};
use crate::toolchain::policy::{
    VersionPolicy, DEV_NIGHTLY_OVERRIDE_ENV, SUPPORTED_TSGO_RANGE_LABEL,
};

/// The engine-release channel an accepted version string belongs to.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EngineChannel {
    /// A bare stable `7.0.x` build in the supported window (`>=7.0.2, <7.1.0`)
    /// with NO prerelease or build-metadata suffix — the ONLY production
    /// channel.
    StableRelease,
    /// A `7.0.0-dev.<date>.<n>` nightly at or after the integer-handle wire
    /// flip, admitted ONLY under the DEV-ONLY override
    /// ([`crate::toolchain::policy::DEV_NIGHTLY_OVERRIDE_ENV`]) for nightly
    /// gate testing — never in production.
    NightlyPreview,
}

/// Classify an engine version string under the process's env-derived policy
/// ([`VersionPolicy::from_env`]). See [`classify_engine_version_with`].
pub fn classify_engine_version(v: &str) -> Option<EngineChannel> {
    classify_engine_version_with(v, &VersionPolicy::from_env())
}

/// Classify an engine version string into an accepted [`EngineChannel`] under
/// an explicit [`VersionPolicy`].
///
/// - `Some(EngineChannel::StableRelease)` for a stable version satisfying the
///   supported window (`>=7.0.2, <7.1.0`, no prerelease, no build metadata).
/// - `Some(EngineChannel::NightlyPreview)` for an integer-handle nightly when
///   (and only when) the policy carries the DEV-ONLY nightly override.
/// - `None` for anything else — the caller fails closed: a different
///   major/minor (`7.1.x`, `8.x`), a below-floor patch (`7.0.0`, `7.0.1`), any
///   prerelease (`-rc`, `-beta`, …), any build-metadata suffix, and every
///   nightly under the production policy.
pub fn classify_engine_version_with(v: &str, policy: &VersionPolicy) -> Option<EngineChannel> {
    let version = policy.check_str(v).ok()?;
    if version.is_stable() {
        Some(EngineChannel::StableRelease)
    } else {
        // Only an integer-handle nightly can pass a policy check with a
        // prerelease (the dev override admits nothing else).
        Some(EngineChannel::NightlyPreview)
    }
}

/// Where an [`ObservedEngine`]'s version string came from. Carried through to
/// [`GateClearance::witness`] so downstream policy can distinguish a
/// transport-level probe from an engine self-report.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EngineVersionWitness {
    /// An OWNED-path `--version` probe (or the discovered distribution's
    /// `package.json`) — observed before the engine serves any request.
    VersionProbe,
    /// A SHARED-path in-band `serverInfo` report — the engine names its own
    /// version over an already-open connection.
    InBandServerInfo,
}

/// What the transport observed about a freshly spawned engine, to be validated
/// against the maintained pin.
#[derive(Debug, Clone)]
pub struct ObservedEngine {
    /// The engine version string the transport observed (from a `--version`
    /// probe, the discovered distribution's `package.json`, or an in-band
    /// `serverInfo` report).
    pub engine_version: String,
    /// The wire fingerprint the active codec targets. In production this is
    /// always [`SchemaManifest::wire_fingerprint`] of the codec's own pinned
    /// manifest; the parameter exists so tests can inject a diverged value to
    /// exercise the fail-closed path.
    pub wire_fingerprint: u64,
    /// How the version string was observed.
    pub witness: EngineVersionWitness,
}

impl ObservedEngine {
    /// Construct an observation for an engine reporting `engine_version` via an
    /// OWNED `--version` probe (or the discovered package), using the codec's
    /// own compiled-in wire fingerprint. This is the normal production path:
    /// the codec only ever speaks one wire, so the observed fingerprint is the
    /// pinned one and the version is the only free variable.
    pub fn from_codec_wire(engine_version: impl Into<String>) -> Self {
        Self {
            engine_version: engine_version.into(),
            wire_fingerprint: PINNED.wire_fingerprint(),
            witness: EngineVersionWitness::VersionProbe,
        }
    }

    /// Construct an observation for an engine that reported `engine_version`
    /// IN-BAND via `serverInfo` (the SHARED path), using the codec's own
    /// compiled-in wire fingerprint.
    pub fn from_in_band_server_info(engine_version: impl Into<String>) -> Self {
        Self {
            engine_version: engine_version.into(),
            wire_fingerprint: PINNED.wire_fingerprint(),
            witness: EngineVersionWitness::InBandServerInfo,
        }
    }
}

/// A capability the gate confirmed is supported by the validated wire. Returned
/// from a successful [`validate`] so the caller can gate features without
/// re-deriving the op set.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WireCapability {
    /// The engine speaks the MessagePack tuple `--api` wire this codec targets.
    SyncTupleApi,
}

/// The outcome of a successful gate check: the validated manifest plus the
/// confirmed capability table.
#[derive(Debug, Clone)]
pub struct GateClearance {
    /// The manifest the engine was validated against.
    pub manifest: SchemaManifest,
    /// The capabilities confirmed for this wire.
    pub capabilities: Vec<WireCapability>,
    /// The engine version string the gate channel-classified and accepted.
    pub observed_version: String,
    /// How the accepted version string was observed.
    pub witness: EngineVersionWitness,
}

/// Validate an [`ObservedEngine`] against the maintained pin under the
/// process's env-derived policy ([`VersionPolicy::from_env`]).
///
/// On success returns a [`GateClearance`]; on any mismatch returns a typed
/// [`TsgoApiError::UnsupportedTsgoWire`] (the client must then refuse to start).
pub fn validate(observed: &ObservedEngine) -> TsgoApiResult<GateClearance> {
    validate_with_policy(observed, &PINNED, &VersionPolicy::from_env())
}

/// Validate against an explicit pin under the PRODUCTION policy. [`validate`]
/// delegates to [`validate_with_policy`]; tests use this to pin a known
/// manifest deterministically.
pub fn validate_against(
    observed: &ObservedEngine,
    pin: &SchemaManifest,
) -> TsgoApiResult<GateClearance> {
    validate_with_policy(observed, pin, &VersionPolicy::production())
}

/// Validate against the maintained pin under an explicit [`VersionPolicy`].
/// Production callers use [`validate`] (env-derived policy); the toolchain
/// validator injects its own policy so the wire check and the provisioning
/// policy can never disagree.
pub fn validate_with(
    observed: &ObservedEngine,
    policy: &VersionPolicy,
) -> TsgoApiResult<GateClearance> {
    validate_with_policy(observed, &PINNED, policy)
}

/// Validate an [`ObservedEngine`] against an explicit pin under an explicit
/// [`VersionPolicy`]. The policy injects the DEV-ONLY nightly override
/// deterministically (nightly gate testing); production goes through
/// [`validate`].
pub fn validate_with_policy(
    observed: &ObservedEngine,
    pin: &SchemaManifest,
    policy: &VersionPolicy,
) -> TsgoApiResult<GateClearance> {
    // Dimension 1: the supported-version policy. An engine outside the
    // supported window (stable `>=7.0.2, <7.1.0`; integer-handle nightlies
    // only under the dev override) is an unknown wire and is refused; the
    // pin's version is the reference build, not an equality bar.
    if classify_engine_version_with(&observed.engine_version, policy).is_none() {
        return Err(TsgoApiError::UnsupportedTsgoWire(format!(
            "engine version `{}` is not supported: Verter supports tsgo \
             (TypeScript 7 native) stable `{SUPPORTED_TSGO_RANGE_LABEL}` only \
             (reference build `{}`); install a supported stable release, or \
             re-verify the hand-written codec and bump the schema manifest to \
             support a new TypeScript version (nightly gate testing may set \
             {DEV_NIGHTLY_OVERRIDE_ENV}=1)",
            observed.engine_version, pin.engine_version
        )));
    }

    // Dimension 2: wire fingerprint. A mismatch means the framing/op/callback
    // inventory the engine speaks diverges from what the codec hand-writes.
    let expected = pin.wire_fingerprint();
    if observed.wire_fingerprint != expected {
        return Err(TsgoApiError::UnsupportedTsgoWire(format!(
            "wire fingerprint {:#018x} does not match the pinned {:#018x}; \
             the tsgo `--api` wire diverged from the hand-written codec",
            observed.wire_fingerprint, expected
        )));
    }

    Ok(GateClearance {
        manifest: *pin,
        capabilities: vec![WireCapability::SyncTupleApi],
        observed_version: observed.engine_version.clone(),
        witness: observed.witness,
    })
}

/// The version-independent wire rail: the FIRST `updateSnapshot` response's
/// snapshot handle must be a bare JSON integer within the i64
/// [`crate::proto::types::OpaqueHandle`] domain.
///
/// The version dimension trusts what the engine (or its package) REPORTS; this
/// rail validates what the engine actually SPEAKS. An engine whose version
/// satisfies the support policy but whose first snapshot handle is a string, a
/// non-integer, or an integer OUTSIDE the i64 handle domain
/// (e.g. a `u64` past `i64::MAX`, which the codec's `OpaqueHandle(i64)` cannot
/// represent) is on a different opaque-handle wire class and is refused with a
/// typed [`TsgoApiError::UnsupportedTsgoWire`] naming `observed_version`.
pub fn require_integer_snapshot_handle(
    raw_snapshot: &serde_json::Value,
    observed_version: &str,
) -> TsgoApiResult<()> {
    if raw_snapshot.as_i64().is_some() {
        return Ok(());
    }
    Err(TsgoApiError::UnsupportedTsgoWire(format!(
        "engine `{observed_version}` returned a first `updateSnapshot` snapshot \
         handle that is not a bare i64 integer (got `{raw_snapshot}`); the codec \
         only speaks the integer-handle `--api` wire, refusing to proceed"
    )))
}

#[cfg(test)]
mod tests {
    use super::*;

    // ── DISCRIMINATING: the DEV-ONLY override re-admits integer-handle
    //    nightlies at the GATE level — classifier and validate — and nothing
    //    else (never rc/beta, never pre-flip string-handle nightlies). ────────
    #[test]
    fn dev_override_classifies_integer_handle_nightlies_only() {
        let policy = VersionPolicy::with_dev_nightly_override();
        assert_eq!(
            classify_engine_version_with("7.0.0-dev.20260604.1", &policy),
            Some(EngineChannel::NightlyPreview)
        );
        assert_eq!(
            classify_engine_version_with("7.0.0-dev.20260703.1", &policy),
            Some(EngineChannel::NightlyPreview)
        );
        // A pre-flip (string-handle) nightly stays refused.
        assert_eq!(
            classify_engine_version_with("7.0.0-dev.20260603.1", &policy),
            None
        );
        // The override is NOT a general prerelease/range bypass.
        assert_eq!(classify_engine_version_with("7.0.2-rc", &policy), None);
        assert_eq!(classify_engine_version_with("7.0.2-beta", &policy), None);
        assert_eq!(classify_engine_version_with("7.1.0", &policy), None);
        // Stable acceptance is unaffected by the override flag.
        assert_eq!(
            classify_engine_version_with("7.0.9", &policy),
            Some(EngineChannel::StableRelease)
        );
    }

    #[test]
    fn validate_with_policy_admits_a_nightly_only_under_the_dev_override() {
        let observed = ObservedEngine::from_codec_wire("7.0.0-dev.20260604.1");
        let clearance = validate_with_policy(
            &observed,
            &PINNED,
            &VersionPolicy::with_dev_nightly_override(),
        )
        .expect("an integer-handle nightly passes under the dev override");
        assert_eq!(clearance.observed_version, "7.0.0-dev.20260604.1");
        // The SAME engine is refused by the production policy.
        let err = validate_with_policy(&observed, &PINNED, &VersionPolicy::production())
            .expect_err("production refuses the nightly");
        assert!(matches!(err, TsgoApiError::UnsupportedTsgoWire(_)));
    }

    #[test]
    fn matching_engine_passes_the_gate() {
        let observed = ObservedEngine::from_codec_wire(PINNED.engine_version);
        let clearance = validate(&observed).expect("matching engine must pass");
        assert_eq!(clearance.manifest.engine_version, PINNED.engine_version);
        assert_eq!(clearance.observed_version, PINNED.engine_version);
        assert!(clearance
            .capabilities
            .contains(&WireCapability::SyncTupleApi));
    }

    // ── DISCRIMINATING: the ONLY production channel is stable `7.0.x` in the
    //    supported window — stable patches classify, nothing else does. ──────
    #[test]
    fn stable_in_window_classifies_as_stable_release() {
        for v in ["7.0.2", "7.0.3", "7.0.9", "7.0.13"] {
            assert_eq!(
                classify_engine_version(v),
                Some(EngineChannel::StableRelease),
                "{v}"
            );
        }
    }

    // ── DISCRIMINATING: below-floor stables are REFUSED — `7.0.0`/`7.0.1`
    //    were accepted by the old channel classifier; the floor is 7.0.2. ────
    #[test]
    fn below_floor_stable_versions_are_refused() {
        assert_eq!(classify_engine_version("7.0.0"), None, "below the floor");
        assert_eq!(classify_engine_version("7.0.1"), None, "below the floor");
    }

    // ── DISCRIMINATING: the production RC channel is REMOVED — every `-rc`
    //    build classifies as None now (previously `Some(RcRelease)`). ─────────
    #[test]
    fn rc_versions_are_refused_in_production() {
        for v in [
            "7.0.0-rc",
            "7.0.1-rc",
            "7.0.2-rc",
            "7.0.12-rc",
            "7.1.0-rc",
            "8.0.0-rc",
        ] {
            assert_eq!(classify_engine_version(v), None, "{v}");
        }
    }

    // ── DISCRIMINATING: nightlies are REFUSED in production — the dev-only
    //    override (tested below via the policy-injecting entry points) is the
    //    sole escape hatch. ────────────────────────────────────────────────────
    #[test]
    fn nightly_versions_are_refused_in_production() {
        assert_eq!(classify_engine_version("7.0.0-dev.20260604.1"), None);
        assert_eq!(classify_engine_version("7.0.0-dev.20260703.1"), None);
        assert_eq!(classify_engine_version("7.0.0-dev.20260603.1"), None);
    }

    #[test]
    fn out_of_window_and_suffixed_versions_are_refused() {
        for v in [
            "7.1.0",
            "7.1.2",
            "8.0.0",
            "6.9.9",
            "7.0.2-beta",
            "7.0.2-alpha",
            "7.0.2-rc.1",
            "7.0.2+build",
        ] {
            assert_eq!(classify_engine_version(v), None, "{v}");
        }
    }

    #[test]
    fn malformed_versions_are_refused() {
        for v in [
            "",
            "garbage",
            "7.0",
            "7.0.",
            "7.0.x",
            "7.0.2.3",
            "7.00.2",
            "7.0.0-dev.20260604",
            "7.0.0-dev.2026060x.1",
            "7.0.0-dev.20260604.x",
            "7.0.0-dev.20260604.1.2",
        ] {
            assert_eq!(classify_engine_version(v), None, "`{v}`");
        }
    }

    // ── DISCRIMINATING: `validate` refuses rc/nightly/out-of-window versions
    //    with an ACTIONABLE message (names the version, the stable window). ──
    #[test]
    fn validate_refuses_rc_nightly_and_out_of_window_actionably() {
        for v in ["7.0.1-rc", "7.0.0-dev.20260604.1", "7.1.0", "7.0.0"] {
            let err =
                validate(&ObservedEngine::from_codec_wire(v)).expect_err("{v} must be refused");
            match &err {
                TsgoApiError::UnsupportedTsgoWire(msg) => {
                    assert!(msg.contains(v), "names the version: {msg}");
                    assert!(
                        msg.contains(">=7.0.2, <7.1.0"),
                        "names the supported window: {msg}"
                    );
                    assert!(msg.contains("stable"), "states the stable-only rule: {msg}");
                }
                other => panic!("expected UnsupportedTsgoWire, got {other:?}"),
            }
        }
    }

    #[test]
    fn validate_accepts_a_stable_patch_in_the_window() {
        let clearance = validate(&ObservedEngine::from_codec_wire("7.0.9"))
            .expect("a stable patch in the window passes");
        assert_eq!(clearance.observed_version, "7.0.9");
        assert!(clearance
            .capabilities
            .contains(&WireCapability::SyncTupleApi));
    }

    // ── DISCRIMINATING: version acceptance is WINDOW membership, not equality
    //    with the pin's reference build. ────────────────────────────────────────
    #[test]
    fn version_acceptance_is_window_membership_not_pin_equality() {
        let mut pin = PINNED;
        pin.engine_version = "7.0.9"; // a different reference build, same window
        let real = ObservedEngine::from_codec_wire("7.0.2");
        assert!(
            validate_against(&real, &pin).is_ok(),
            "acceptance is window membership; the pin's version is the reference              build, not an equality bar"
        );
    }

    // ── DISCRIMINATING: a mismatched fingerprint is refused ─────────────────
    #[test]
    fn mismatched_fingerprint_refuses_to_start() {
        let observed = ObservedEngine {
            engine_version: PINNED.engine_version.to_string(),
            wire_fingerprint: PINNED.wire_fingerprint() ^ 0xdead_beef,
            witness: EngineVersionWitness::VersionProbe,
        };
        let err = validate(&observed).expect_err("a diverged wire must be refused");
        assert!(
            matches!(err, TsgoApiError::UnsupportedTsgoWire(_)),
            "got {err:?}"
        );
    }

    #[test]
    fn pin_with_diverged_op_set_changes_fingerprint_and_rejects() {
        // A pin whose op inventory differs has a different fingerprint, so the
        // real engine's (codec) fingerprint no longer matches it.
        const DIVERGED: &[&str] = &["echo", "initialize"];
        let mut diverged_pin = PINNED;
        diverged_pin.ops = DIVERGED;
        let real = ObservedEngine::from_codec_wire(PINNED.engine_version);
        let err = validate_against(&real, &diverged_pin).expect_err("must reject");
        assert!(matches!(err, TsgoApiError::UnsupportedTsgoWire(_)));
    }

    // ── DISCRIMINATING: witness provenance round-trips onto the clearance ────
    #[test]
    fn witness_provenance_round_trips_onto_clearance() {
        let probed = ObservedEngine::from_codec_wire("7.0.3");
        assert_eq!(probed.witness, EngineVersionWitness::VersionProbe);
        let clearance = validate(&probed).expect("probed engine passes");
        assert_eq!(clearance.witness, EngineVersionWitness::VersionProbe);
        assert_eq!(clearance.observed_version, "7.0.3");

        let in_band = ObservedEngine::from_in_band_server_info("7.0.4");
        assert_eq!(in_band.witness, EngineVersionWitness::InBandServerInfo);
        let clearance = validate(&in_band).expect("in-band engine passes");
        assert_eq!(clearance.witness, EngineVersionWitness::InBandServerInfo);
        assert_eq!(clearance.observed_version, "7.0.4");
    }

    // ── DISCRIMINATING: the first-snapshot integer-handle rail ──────────────
    #[test]
    fn integer_snapshot_handles_pass_the_rail() {
        assert!(require_integer_snapshot_handle(&serde_json::json!(3), "7.0.3").is_ok());
        assert!(require_integer_snapshot_handle(&serde_json::json!(0), "7.0.3").is_ok());
        // The full i64 handle domain the codec's OpaqueHandle(i64) can represent.
        assert!(require_integer_snapshot_handle(
            &serde_json::Value::Number(serde_json::Number::from(i64::MAX)),
            "7.0.3"
        )
        .is_ok());
    }

    // ── DISCRIMINATING: an integer BEYOND the i64 handle domain (a u64 past
    //    i64::MAX, which OpaqueHandle(i64) cannot represent) fails closed. ──────
    #[test]
    fn snapshot_handle_beyond_i64_fails_the_rail_closed() {
        let over = serde_json::Value::Number(serde_json::Number::from(u64::MAX));
        assert!(
            over.is_u64() && over.as_i64().is_none(),
            "fixture must be a u64 past i64::MAX"
        );
        let err = require_integer_snapshot_handle(&over, "7.0.3").expect_err(
            "a handle past i64::MAX is outside the OpaqueHandle domain and must be refused",
        );
        assert!(
            matches!(err, TsgoApiError::UnsupportedTsgoWire(ref m)
                if m.contains("7.0.3") && m.contains("not a bare i64 integer")),
            "got {err:?}"
        );
    }

    #[test]
    fn non_integer_snapshot_handles_fail_the_rail_closed() {
        for handle in [
            serde_json::json!("n0000000000000003"),
            serde_json::json!("3"),
            serde_json::json!(null),
            serde_json::json!(3.5),
            serde_json::json!({}),
            serde_json::json!([]),
        ] {
            let err = require_integer_snapshot_handle(&handle, "7.0.3")
                .expect_err("a non-integer handle must be refused");
            assert!(
                matches!(err, TsgoApiError::UnsupportedTsgoWire(ref m)
                    if m.contains("7.0.3") && m.contains("not a bare i64 integer")),
                "the refusal must name the observed version and the handle \
                 shape; handle {handle}, got {err:?}"
            );
        }
    }
}
