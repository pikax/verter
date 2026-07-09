//! Runtime fail-closed wire gate.
//!
//! Before a client is usable, the engine it spawned must be validated against
//! the maintained wire pin ([`crate::proto::schema_manifest::PINNED`]). The
//! gate compares two dimensions:
//!   1. the engine version string — validated by CHANNEL, not by equality with
//!      one build: [`classify_engine_version`] accepts the bare `7.0.<patch>`
//!      GA stable-release channel, the `7.0.<patch>-rc` release-candidate
//!      channel, and `7.0.0-dev.<date>.<n>` nightlies at or after the
//!      integer-handle wire (earlier nightlies issue STRING opaque handles, a
//!      different wire class). The pin's
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
//! An UNKNOWN shape (a version outside the accepted channels, or a fingerprint
//! that does not match) makes the gate refuse to start: it returns a typed
//! [`crate::error::TsgoApiError::UnsupportedTsgoWire`], so a diverged tsgo never
//! silently ships.
//!
//! A version string can lie; the wire cannot. The gate therefore has a third,
//! version-independent rail: [`require_integer_snapshot_handle`] validates the
//! FIRST `updateSnapshot` response's snapshot handle as a bare JSON integer.
//! An engine that classifies into an accepted channel but hands back a string
//! handle is refused at that first response, before any product result.
//!
//! Decoupling note: the gate does not itself spawn or probe the engine — it is
//! a pure validation function over an [`ObservedEngine`] the transport layer
//! supplies. This keeps it unit-testable without a live process and lets the
//! caller decide where the observed version comes from (a `--version` probe,
//! the discovered package.json, an in-band `serverInfo`, etc.).

use crate::error::{TsgoApiError, TsgoApiResult};
use crate::proto::schema_manifest::{SchemaManifest, PINNED};

/// The earliest nightly build date (`YYYYMMDD` in `7.0.0-dev.YYYYMMDD.N`) whose
/// `--api` wire issues opaque handles as bare JSON integers. Earlier nightlies
/// issue STRING handles — a different opaque-handle wire class the codec does
/// not speak — and are refused.
const NIGHTLY_INTEGER_HANDLE_FLIP_DATE: u32 = 20260604;

/// The engine-release channel an accepted version string belongs to. The codec
/// speaks a CHANNEL, not one build: every build within an accepted channel
/// shares the integer-handle `--api` wire the codec targets.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EngineChannel {
    /// A bare `7.0.<patch>` GA stable-release build (e.g. `7.0.2`) — the current
    /// reference channel, with NO prerelease or build-metadata suffix.
    StableRelease,
    /// A `7.0.<patch>-rc` release-candidate build (e.g. `7.0.1-rc`).
    RcRelease,
    /// A `7.0.0-dev.<date>.<n>` nightly at or after the integer-handle wire
    /// (see [`classify_engine_version`]).
    NightlyPreview,
}

/// Classify an engine version string into an accepted [`EngineChannel`].
///
/// - `Some(EngineChannel::StableRelease)` for a bare `7.0.<patch>` GA build
///   where `<patch>` is one or more ASCII digits and there is NO prerelease or
///   build-metadata suffix (`7.0.0`, `7.0.2`, `7.0.13`).
/// - `Some(EngineChannel::RcRelease)` for `7.0.<patch>-rc` where `<patch>` is
///   one or more ASCII digits (`7.0.0-rc`, `7.0.1-rc`, `7.0.12-rc`).
/// - `Some(EngineChannel::NightlyPreview)` for `7.0.0-dev.<date>.<n>` where
///   `<date>` is exactly eight ASCII digits numerically at or after
///   [`NIGHTLY_INTEGER_HANDLE_FLIP_DATE`] and `<n>` is one or more ASCII
///   digits.
/// - `None` for anything else — the caller fails closed on an unclassified
///   version. A different major/minor (`7.1.x`, `8.x`), a prerelease other than
///   `-rc` (`7.0.2-beta`), and any build-metadata suffix are refused until their
///   wire is separately verified.
pub fn classify_engine_version(v: &str) -> Option<EngineChannel> {
    let rest = v.strip_prefix("7.0.")?;

    // Stable-release channel: a bare `7.0.<patch>` GA build — the patch is pure
    // ASCII digits with no `-rc`/`-beta`/`+build` suffix. This is the reference
    // channel the codec is now verified against.
    if !rest.is_empty() && rest.bytes().all(|b| b.is_ascii_digit()) {
        return Some(EngineChannel::StableRelease);
    }

    // Release-candidate channel: `7.0.<patch>-rc`.
    if let Some(patch) = rest.strip_suffix("-rc") {
        if !patch.is_empty() && patch.bytes().all(|b| b.is_ascii_digit()) {
            return Some(EngineChannel::RcRelease);
        }
    }

    // Nightly channel: `7.0.0-dev.<date>.<n>`, integer-handle builds only.
    if let Some(dev) = rest.strip_prefix("0-dev.") {
        let (date, seq) = dev.split_once('.')?;
        if date.len() == 8
            && date.bytes().all(|b| b.is_ascii_digit())
            && !seq.is_empty()
            && seq.bytes().all(|b| b.is_ascii_digit())
        {
            let date_num: u32 = date.parse().ok()?;
            if date_num >= NIGHTLY_INTEGER_HANDLE_FLIP_DATE {
                return Some(EngineChannel::NightlyPreview);
            }
        }
    }

    None
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

/// Validate an [`ObservedEngine`] against the maintained pin.
///
/// On success returns a [`GateClearance`]; on any mismatch returns a typed
/// [`TsgoApiError::UnsupportedTsgoWire`] (the client must then refuse to start).
pub fn validate(observed: &ObservedEngine) -> TsgoApiResult<GateClearance> {
    validate_against(observed, &PINNED)
}

/// Validate against an explicit pin. [`validate`] delegates here with
/// [`PINNED`]; tests use this to pin a known manifest deterministically.
pub fn validate_against(
    observed: &ObservedEngine,
    pin: &SchemaManifest,
) -> TsgoApiResult<GateClearance> {
    // Dimension 1: version channel. An engine outside the accepted channels
    // (GA stable releases, rc releases, integer-handle nightlies) is an unknown
    // wire and is refused; the pin's version is the reference build, not an
    // equality bar.
    if classify_engine_version(&observed.engine_version).is_none() {
        return Err(TsgoApiError::UnsupportedTsgoWire(format!(
            "engine version `{}` is not in a supported channel (accepted: bare \
             `7.0.<patch>` GA stable releases, `7.0.<patch>-rc` release \
             candidates, or `7.0.0-dev.<date>.<n>` nightlies at/after the \
             integer-handle wire; reference build `{}`); re-verify the \
             hand-written codec and bump the schema manifest",
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
/// classifies into an accepted channel but whose first snapshot handle is a
/// string, a non-integer, or an integer OUTSIDE the i64 handle domain
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

    // ── DISCRIMINATING: the version-channel classifier's accept/reject table ─
    #[test]
    fn classifier_accepts_stable_release_channel() {
        // Bare `7.0.<patch>` GA builds classify into the stable-release channel.
        assert_eq!(
            classify_engine_version("7.0.2"),
            Some(EngineChannel::StableRelease),
            "the GA `7.0.2` reference build is the stable-release channel"
        );
        assert_eq!(
            classify_engine_version("7.0.0"),
            Some(EngineChannel::StableRelease),
            "the earliest `7.0.x` GA patch is accepted"
        );
        assert_eq!(
            classify_engine_version("7.0.13"),
            Some(EngineChannel::StableRelease),
            "a multi-digit GA patch is accepted"
        );
    }

    // ── DISCRIMINATING: the stable arm is a DISTINCT channel from rc — a bare
    //    `7.0.2` and its `7.0.2-rc` sibling classify into different arms. ───────
    #[test]
    fn stable_and_rc_are_distinct_channels() {
        assert_eq!(
            classify_engine_version("7.0.2"),
            Some(EngineChannel::StableRelease)
        );
        assert_eq!(
            classify_engine_version("7.0.2-rc"),
            Some(EngineChannel::RcRelease)
        );
        assert_ne!(
            classify_engine_version("7.0.2"),
            classify_engine_version("7.0.2-rc"),
            "GA and its rc sibling are not the same channel"
        );
    }

    // ── DISCRIMINATING: bare out-of-range and suffixed stable strings are
    //    refused — the stable arm is `7.0.<patch>` ONLY, no prerelease/build,
    //    no other major/minor. ────────────────────────────────────────────────
    #[test]
    fn classifier_rejects_out_of_channel_and_suffixed_stable_versions() {
        // A different minor / major as a BARE stable string is out of channel.
        assert_eq!(classify_engine_version("7.1.0"), None, "wrong minor");
        assert_eq!(classify_engine_version("7.1.2"), None, "wrong minor");
        assert_eq!(classify_engine_version("8.0.0"), None, "wrong major");
        assert_eq!(classify_engine_version("6.0.0"), None, "major < 7");
        // A prerelease/build suffix other than `-rc` is refused.
        assert_eq!(
            classify_engine_version("7.0.2-beta"),
            None,
            "beta prerelease"
        );
        assert_eq!(
            classify_engine_version("7.0.2-alpha"),
            None,
            "alpha prerelease"
        );
        assert_eq!(
            classify_engine_version("7.0.2-rc.1"),
            None,
            "a dotted rc suffix is not the bare `-rc` grammar"
        );
        assert_eq!(
            classify_engine_version("7.0.2+build"),
            None,
            "build metadata is refused"
        );
        // Malformed bare-stable shapes.
        assert_eq!(classify_engine_version("7.0."), None, "empty patch");
        assert_eq!(classify_engine_version("7.0.x"), None, "non-digit patch");
        assert_eq!(classify_engine_version("7.0.2.3"), None, "extra segment");
    }

    #[test]
    fn classifier_accepts_rc_release_channel() {
        assert_eq!(
            classify_engine_version("7.0.1-rc"),
            Some(EngineChannel::RcRelease)
        );
        assert_eq!(
            classify_engine_version("7.0.2-rc"),
            Some(EngineChannel::RcRelease)
        );
        assert_eq!(
            classify_engine_version("7.0.0-rc"),
            Some(EngineChannel::RcRelease)
        );
        assert_eq!(
            classify_engine_version("7.0.12-rc"),
            Some(EngineChannel::RcRelease)
        );
    }

    #[test]
    fn classifier_accepts_integer_handle_nightlies() {
        assert_eq!(
            classify_engine_version("7.0.0-dev.20260604.1"),
            Some(EngineChannel::NightlyPreview),
            "the first integer-handle nightly date is accepted"
        );
        assert_eq!(
            classify_engine_version("7.0.0-dev.20260703.1"),
            Some(EngineChannel::NightlyPreview),
            "later nightlies are accepted"
        );
    }

    #[test]
    fn classifier_rejects_string_handle_nightlies() {
        assert_eq!(
            classify_engine_version("7.0.0-dev.20260603.1"),
            None,
            "the day before the integer-handle wire is a string-handle build"
        );
        assert_eq!(classify_engine_version("7.0.0-dev.20260526.1"), None);
    }

    #[test]
    fn classifier_rejects_malformed_and_out_of_channel_versions() {
        // A nightly missing its `.N` sequence suffix is malformed.
        assert_eq!(classify_engine_version("7.0.0-dev.20260604"), None);
        // Out-of-channel version shapes.
        assert_eq!(classify_engine_version("6.9.9"), None);
        assert_eq!(classify_engine_version("7.1.0-rc"), None);
        assert_eq!(classify_engine_version("8.0.0-rc"), None);
        assert_eq!(classify_engine_version("7.0.1-beta"), None);
        assert_eq!(classify_engine_version("garbage"), None);
        assert_eq!(classify_engine_version(""), None);
        // Non-digit patch / date / sequence components.
        assert_eq!(classify_engine_version("7.0.x-rc"), None);
        assert_eq!(classify_engine_version("7.0.-rc"), None);
        assert_eq!(classify_engine_version("7.0.0-dev.2026060x.1"), None);
        assert_eq!(classify_engine_version("7.0.0-dev.20260604.x"), None);
        assert_eq!(classify_engine_version("7.0.0-dev.20260604.1.2"), None);
    }

    // ── DISCRIMINATING: a version outside the channels is refused (fail-closed)
    #[test]
    fn out_of_channel_version_refuses_to_start() {
        // A string-handle nightly is outside the accepted channels: refused.
        let observed = ObservedEngine::from_codec_wire("7.0.0-dev.20260526.1");
        let err = validate(&observed).expect_err("a string-handle nightly must be refused");
        assert!(
            matches!(err, TsgoApiError::UnsupportedTsgoWire(ref m) if m.contains("7.0.0-dev.20260526.1")),
            "got {err:?}"
        );

        // A different major/minor is outside the channels: refused.
        let observed = ObservedEngine::from_codec_wire("6.9.9");
        let err = validate(&observed).expect_err("an out-of-channel version must be refused");
        assert!(
            matches!(err, TsgoApiError::UnsupportedTsgoWire(ref m) if m.contains("6.9.9")),
            "the refusal must name the observed version; got {err:?}"
        );
    }

    // ── DISCRIMINATING: the pinned rc version PASSES the gate (the inverse of
    //    the above — the genuine rc engine clears the version dimension). ──────
    #[test]
    fn pinned_rc_version_passes_the_gate() {
        let observed = ObservedEngine::from_codec_wire("7.0.1-rc");
        let clearance = validate(&observed).expect("the pinned rc version must pass");
        assert_eq!(clearance.manifest.engine_version, "7.0.1-rc");
    }

    // ── DISCRIMINATING: the accepted set is a CHANNEL, not one build — a later
    //    rc patch and an integer-handle nightly both clear the version dimension.
    #[test]
    fn later_rc_patch_passes_the_gate() {
        let observed = ObservedEngine::from_codec_wire("7.0.2-rc");
        validate(&observed).expect("any 7.0.x-rc build is in the accepted channel");
    }

    // ── DISCRIMINATING: the GA stable-release channel clears the gate. This
    //    asserts ACCEPTANCE (not the variant name) so it compiles against the
    //    pre-arm tree and FAILS RED there (bare `7.0.2` classified `None` → the
    //    gate refused it); it goes GREEN once the `StableRelease` arm lands. ────
    #[test]
    fn stable_release_ga_version_passes_the_gate() {
        assert!(
            classify_engine_version("7.0.2").is_some(),
            "bare GA `7.0.2` must classify into an accepted channel"
        );
        let observed = ObservedEngine::from_codec_wire("7.0.2");
        validate(&observed).expect("the GA stable release must clear the wire gate");
    }

    #[test]
    fn nightly_at_or_after_integer_handle_flip_passes_the_gate() {
        let observed = ObservedEngine::from_codec_wire("7.0.0-dev.20260604.1");
        let clearance =
            validate(&observed).expect("an integer-handle nightly must pass the channel gate");
        assert!(clearance
            .capabilities
            .contains(&WireCapability::SyncTupleApi));
    }

    // ── DISCRIMINATING: version acceptance is channel membership, not equality
    //    with the pin's reference build — a pin naming a different in-channel
    //    reference still accepts an in-channel engine. ─────────────────────────
    #[test]
    fn version_acceptance_is_channel_membership_not_pin_equality() {
        let mut pin = PINNED;
        pin.engine_version = "7.0.9-rc"; // a different reference build, same channel
        let real = ObservedEngine::from_codec_wire("7.0.1-rc");
        assert!(
            validate_against(&real, &pin).is_ok(),
            "acceptance is channel membership; the pin's version is the \
             reference build, not an equality bar"
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
        // Same version, but the pin's fingerprint differs from the codec's.
        let err = validate_against(&real, &diverged_pin).expect_err("must reject");
        assert!(matches!(err, TsgoApiError::UnsupportedTsgoWire(_)));
    }

    // ── DISCRIMINATING: witness provenance round-trips onto the clearance ────
    #[test]
    fn witness_provenance_round_trips_onto_clearance() {
        let probed = ObservedEngine::from_codec_wire("7.0.1-rc");
        assert_eq!(probed.witness, EngineVersionWitness::VersionProbe);
        let clearance = validate(&probed).expect("probed engine passes");
        assert_eq!(clearance.witness, EngineVersionWitness::VersionProbe);
        assert_eq!(clearance.observed_version, "7.0.1-rc");

        let in_band = ObservedEngine::from_in_band_server_info("7.0.0-dev.20260604.1");
        assert_eq!(in_band.witness, EngineVersionWitness::InBandServerInfo);
        let clearance = validate(&in_band).expect("in-band engine passes");
        assert_eq!(clearance.witness, EngineVersionWitness::InBandServerInfo);
        assert_eq!(clearance.observed_version, "7.0.0-dev.20260604.1");
    }

    // ── DISCRIMINATING: the first-snapshot integer-handle rail ──────────────
    #[test]
    fn integer_snapshot_handles_pass_the_rail() {
        assert!(require_integer_snapshot_handle(&serde_json::json!(3), "7.0.1-rc").is_ok());
        assert!(require_integer_snapshot_handle(&serde_json::json!(0), "7.0.1-rc").is_ok());
        // The full i64 handle domain the codec's OpaqueHandle(i64) can represent.
        assert!(require_integer_snapshot_handle(
            &serde_json::Value::Number(serde_json::Number::from(i64::MAX)),
            "7.0.1-rc"
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
        let err = require_integer_snapshot_handle(&over, "7.0.1-rc").expect_err(
            "a handle past i64::MAX is outside the OpaqueHandle domain and must be refused",
        );
        assert!(
            matches!(err, TsgoApiError::UnsupportedTsgoWire(ref m)
                if m.contains("7.0.1-rc") && m.contains("not a bare i64 integer")),
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
            let err = require_integer_snapshot_handle(&handle, "7.0.0-dev.20260604.1")
                .expect_err("a non-integer handle must be refused");
            assert!(
                matches!(err, TsgoApiError::UnsupportedTsgoWire(ref m)
                    if m.contains("7.0.0-dev.20260604.1") && m.contains("not a bare i64 integer")),
                "the refusal must name the observed version and the handle \
                 shape; handle {handle}, got {err:?}"
            );
        }
    }
}
