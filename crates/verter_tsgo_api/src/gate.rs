//! Runtime fail-closed wire gate.
//!
//! Before a client is usable, the engine it spawned must be validated against
//! the maintained wire pin ([`crate::proto::schema_manifest::PINNED`]). The
//! gate compares two dimensions:
//!   1. the engine version string — validated against the SUPPORTED-VERSION
//!      POLICY ([`crate::toolchain::policy`]), not by equality with one build:
//!      [`classify_engine_version`] accepts the pinned TypeScript build and
//!      the rest of its release line
//!      ([`crate::toolchain::policy::SUPPORTED_TSGO_RANGE_LABEL`]: later stable
//!      patches, and `beta`/`rc` builds of the pinned release) and NOTHING
//!      else — nightlies, alphas, earlier releases and other minors/majors are
//!      refused. The pin's
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
//! FIRST snapshot response's snapshot handle as a bare JSON integer. An engine
//! whose version satisfies the policy but hands back any other handle shape is
//! refused at that first response, before any product result.
//!
//! Decoupling note: the gate does not itself spawn or probe the engine — it is
//! a pure validation function over an [`ObservedEngine`] the transport layer
//! supplies. This keeps it unit-testable without a live process and lets the
//! caller decide where the observed version comes from (a `--version` probe,
//! the discovered package.json, an in-band `serverInfo`, etc.).

use crate::error::{TsgoApiError, TsgoApiResult};
use crate::proto::schema_manifest::{SchemaManifest, PINNED};
use crate::toolchain::policy::{VersionPolicy, SUPPORTED_TSGO_RANGE_LABEL};

/// The engine-release channel an accepted version string belongs to.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EngineChannel {
    /// A stable build on the supported line, with NO prerelease or
    /// build-metadata suffix.
    StableRelease,
    /// A `beta`/`rc` build of the pinned release
    /// ([`crate::toolchain::policy::TYPESCRIPT_PIN`]) at or after the pin.
    PinnedPrerelease,
}

/// Classify an engine version string under the production policy. See
/// [`classify_engine_version_with`].
pub fn classify_engine_version(v: &str) -> Option<EngineChannel> {
    classify_engine_version_with(v, &VersionPolicy::production())
}

/// Classify an engine version string into an accepted [`EngineChannel`] under
/// an explicit [`VersionPolicy`].
///
/// - `Some(EngineChannel::StableRelease)` for a stable version on the
///   supported line at or after the pin.
/// - `Some(EngineChannel::PinnedPrerelease)` for a `beta`/`rc` build of the
///   pinned release at or after the pin.
/// - `None` for anything else — the caller fails closed: another minor/major,
///   a release before the pin, a nightly or other prerelease channel, any
///   build-metadata suffix.
pub fn classify_engine_version_with(v: &str, policy: &VersionPolicy) -> Option<EngineChannel> {
    let version = policy.check_str(v).ok()?;
    if version.is_stable() {
        Some(EngineChannel::StableRelease)
    } else {
        Some(EngineChannel::PinnedPrerelease)
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
/// production policy ([`VersionPolicy::production`]).
///
/// On success returns a [`GateClearance`]; on any mismatch returns a typed
/// [`TsgoApiError::UnsupportedTsgoWire`] (the client must then refuse to start).
pub fn validate(observed: &ObservedEngine) -> TsgoApiResult<GateClearance> {
    validate_with_policy(observed, &PINNED, &VersionPolicy::production())
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
/// Production callers use [`validate`]; the toolchain
/// validator injects its own policy so the wire check and the provisioning
/// policy can never disagree.
pub fn validate_with(
    observed: &ObservedEngine,
    policy: &VersionPolicy,
) -> TsgoApiResult<GateClearance> {
    validate_with_policy(observed, &PINNED, policy)
}

/// Validate an [`ObservedEngine`] against an explicit pin under an explicit
/// [`VersionPolicy`]; production goes through [`validate`].
pub fn validate_with_policy(
    observed: &ObservedEngine,
    pin: &SchemaManifest,
    policy: &VersionPolicy,
) -> TsgoApiResult<GateClearance> {
    // Dimension 1: the supported-version policy. An engine outside the
    // supported window (the pin's release line) is an unknown wire and is
    // refused; the pin's version is the reference build, not an equality bar.
    if classify_engine_version_with(&observed.engine_version, policy).is_none() {
        return Err(TsgoApiError::UnsupportedTsgoWire(format!(
            "engine version `{}` is not supported: Verter supports tsgo \
             (TypeScript 7 native) `{SUPPORTED_TSGO_RANGE_LABEL}` only \
             (reference build `{}`); install the reference build, or \
             re-verify the hand-written codec and move the TypeScript pin to \
             support a new TypeScript version",
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

/// The version-independent wire rail: the FIRST snapshot response's
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
        "engine `{observed_version}` returned a first snapshot \
         handle that is not a bare i64 integer (got `{raw_snapshot}`); the codec \
         only speaks the integer-handle `--api` wire, refusing to proceed"
    )))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::toolchain::policy::{pinned_version, TYPESCRIPT_PIN};

    /// A stable version on the pin's line, `patch` levels past the pin's.
    fn later_stable(offset: u64) -> String {
        let pin = pinned_version();
        format!("{}.{}.{}", pin.major, pin.minor, pin.patch + offset)
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

    // ── DISCRIMINATING: the wire pin's reference build IS the TypeScript
    //    pin — the codec is verified against the build Verter targets. ──────
    #[test]
    fn wire_reference_build_is_the_typescript_pin() {
        assert_eq!(PINNED.engine_version, TYPESCRIPT_PIN);
    }

    // ── DISCRIMINATING: the pin classifies by its own channel, and stable
    //    releases on the line classify as stable. ─────────────────────────────
    #[test]
    fn line_versions_classify_by_channel() {
        let expected = if pinned_version().is_stable() {
            EngineChannel::StableRelease
        } else {
            EngineChannel::PinnedPrerelease
        };
        assert_eq!(classify_engine_version(TYPESCRIPT_PIN), Some(expected));
        for offset in [0, 1, 9] {
            let v = later_stable(offset);
            assert_eq!(
                classify_engine_version(&v),
                Some(EngineChannel::StableRelease),
                "{v}"
            );
        }
    }

    // ── DISCRIMINATING: everything off the line is refused — earlier
    //    releases, later minors, nightlies, alphas, build metadata. ──────────
    #[test]
    fn off_line_and_unsupported_channels_are_refused() {
        let pin = pinned_version();
        let core = format!("{}.{}.{}", pin.major, pin.minor, pin.patch);
        for v in [
            format!("{}.{}.9", pin.major, pin.minor - 1),
            format!("{}.{}.0", pin.major, pin.minor + 1),
            format!("{}.0.0", pin.major + 1),
            "6.9.9".to_string(),
            format!("{core}-dev.20261022.1"),
            format!("{core}-alpha"),
            format!("{}+build", later_stable(0)),
        ] {
            assert_eq!(classify_engine_version(&v), None, "{v}");
        }
    }

    #[test]
    fn malformed_versions_are_refused() {
        for v in ["", "garbage", "7.1", "7.1.", "7.1.x", "7.1.2.3", "7.01.2"] {
            assert_eq!(classify_engine_version(v), None, "`{v}`");
        }
    }

    // ── DISCRIMINATING: `validate` refuses unsupported versions with an
    //    ACTIONABLE message (names the version and the supported window). ──
    #[test]
    fn validate_refuses_unsupported_versions_actionably() {
        let pin = pinned_version();
        for v in [
            format!("{}.{}.{}-dev.20261022.1", pin.major, pin.minor, pin.patch),
            format!("{}.{}.0", pin.major, pin.minor + 1),
            format!("{}.{}.9", pin.major, pin.minor - 1),
        ] {
            let err =
                validate(&ObservedEngine::from_codec_wire(&v)).expect_err("{v} must be refused");
            match &err {
                TsgoApiError::UnsupportedTsgoWire(msg) => {
                    assert!(msg.contains(&v), "names the version: {msg}");
                    assert!(
                        msg.contains(SUPPORTED_TSGO_RANGE_LABEL),
                        "names the supported window: {msg}"
                    );
                }
                other => panic!("expected UnsupportedTsgoWire, got {other:?}"),
            }
        }
    }

    #[test]
    fn validate_accepts_a_later_stable_patch_on_the_line() {
        let v = later_stable(9);
        let clearance =
            validate(&ObservedEngine::from_codec_wire(&v)).expect("a later stable patch passes");
        assert_eq!(clearance.observed_version, v);
        assert!(clearance
            .capabilities
            .contains(&WireCapability::SyncTupleApi));
    }

    // ── DISCRIMINATING: version acceptance is WINDOW membership, not equality
    //    with the pin's reference build. ────────────────────────────────────────
    #[test]
    fn version_acceptance_is_window_membership_not_pin_equality() {
        let reference = later_stable(9);
        let mut pin = PINNED;
        pin.engine_version = Box::leak(reference.into_boxed_str());
        let real = ObservedEngine::from_codec_wire(TYPESCRIPT_PIN);
        assert!(
            validate_against(&real, &pin).is_ok(),
            "acceptance is window membership; the pin's version is the reference \
             build, not an equality bar"
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
        let first = later_stable(1);
        let probed = ObservedEngine::from_codec_wire(&first);
        assert_eq!(probed.witness, EngineVersionWitness::VersionProbe);
        let clearance = validate(&probed).expect("probed engine passes");
        assert_eq!(clearance.witness, EngineVersionWitness::VersionProbe);
        assert_eq!(clearance.observed_version, first);

        let second = later_stable(2);
        let in_band = ObservedEngine::from_in_band_server_info(&second);
        assert_eq!(in_band.witness, EngineVersionWitness::InBandServerInfo);
        let clearance = validate(&in_band).expect("in-band engine passes");
        assert_eq!(clearance.witness, EngineVersionWitness::InBandServerInfo);
        assert_eq!(clearance.observed_version, second);
    }

    // ── DISCRIMINATING: the first-snapshot integer-handle rail ──────────────
    #[test]
    fn integer_snapshot_handles_pass_the_rail() {
        assert!(require_integer_snapshot_handle(&serde_json::json!(3), TYPESCRIPT_PIN).is_ok());
        assert!(require_integer_snapshot_handle(&serde_json::json!(0), TYPESCRIPT_PIN).is_ok());
        // The full i64 handle domain the codec's OpaqueHandle(i64) can represent.
        assert!(require_integer_snapshot_handle(
            &serde_json::Value::Number(serde_json::Number::from(i64::MAX)),
            TYPESCRIPT_PIN
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
        let err = require_integer_snapshot_handle(&over, TYPESCRIPT_PIN).expect_err(
            "a handle past i64::MAX is outside the OpaqueHandle domain and must be refused",
        );
        assert!(
            matches!(err, TsgoApiError::UnsupportedTsgoWire(ref m)
                if m.contains(TYPESCRIPT_PIN) && m.contains("not a bare i64 integer")),
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
            let err = require_integer_snapshot_handle(&handle, TYPESCRIPT_PIN)
                .expect_err("a non-integer handle must be refused");
            assert!(
                matches!(err, TsgoApiError::UnsupportedTsgoWire(ref m)
                    if m.contains(TYPESCRIPT_PIN) && m.contains("not a bare i64 integer")),
                "the refusal must name the observed version and the handle \
                 shape; handle {handle}, got {err:?}"
            );
        }
    }
}
