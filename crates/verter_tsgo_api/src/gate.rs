//! Runtime fail-closed wire gate.
//!
//! Before a client is usable, the engine it spawned must be validated against
//! the maintained wire pin ([`crate::proto::schema_manifest::PINNED`]). The
//! gate compares two dimensions:
//!   1. the engine version string the tsgo process reports (via the
//!      `initialize` handshake, [`crate::proto::types::InitializeResponse`] is
//!      version-less, so the version is taken from the discovered package /
//!      `--version`), and
//!   2. the wire fingerprint — the codec's compiled-in fingerprint is always
//!      `PINNED.wire_fingerprint()`, so a fingerprint mismatch can only arise
//!      from an observed manifest assembled from a different engine.
//!
//! An UNKNOWN shape (a version the pin does not name, or a fingerprint that does
//! not match) makes the gate refuse to start: it returns a typed
//! [`crate::error::TsgoApiError::UnsupportedTsgoWire`], so a diverged tsgo never
//! silently ships.
//!
//! Decoupling note: the gate does not itself spawn or probe the engine — it is
//! a pure validation function over an [`ObservedEngine`] the transport layer
//! supplies. This keeps it unit-testable without a live process and lets the
//! caller decide where the observed version comes from (a `--version` probe,
//! the discovered package.json, etc.).

use crate::error::{TsgoApiError, TsgoApiResult};
use crate::proto::schema_manifest::{SchemaManifest, PINNED};

/// What the transport observed about a freshly spawned engine, to be validated
/// against the maintained pin.
#[derive(Debug, Clone)]
pub struct ObservedEngine {
    /// The engine version string the transport observed (from a `--version`
    /// probe or the discovered distribution's `package.json`).
    pub engine_version: String,
    /// The wire fingerprint the active codec targets. In production this is
    /// always [`SchemaManifest::wire_fingerprint`] of the codec's own pinned
    /// manifest; the parameter exists so tests can inject a diverged value to
    /// exercise the fail-closed path.
    pub wire_fingerprint: u64,
}

impl ObservedEngine {
    /// Construct an observation for an engine reporting `engine_version`, using
    /// the codec's own compiled-in wire fingerprint. This is the normal
    /// production path: the codec only ever speaks one wire, so the observed
    /// fingerprint is the pinned one and the version is the only free variable.
    pub fn from_codec_wire(engine_version: impl Into<String>) -> Self {
        Self {
            engine_version: engine_version.into(),
            wire_fingerprint: PINNED.wire_fingerprint(),
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
    // Dimension 1: version. The pin names exactly one supported version; an
    // engine reporting anything else is an unknown wire and is refused.
    if observed.engine_version != pin.engine_version {
        return Err(TsgoApiError::UnsupportedTsgoWire(format!(
            "engine version `{}` is not the pinned version `{}`; \
             re-verify the hand-written codec and bump the schema manifest",
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
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn matching_engine_passes_the_gate() {
        let observed = ObservedEngine::from_codec_wire(PINNED.engine_version);
        let clearance = validate(&observed).expect("matching engine must pass");
        assert_eq!(clearance.manifest.engine_version, PINNED.engine_version);
        assert!(clearance
            .capabilities
            .contains(&WireCapability::SyncTupleApi));
    }

    // ── DISCRIMINATING: a mismatched version is refused (fail-closed) ───────
    #[test]
    fn mismatched_version_refuses_to_start() {
        // The retired dev-preview version is NOT the pinned `7.0.1-rc`, so an
        // engine reporting it is an unknown wire and must be refused.
        let observed = ObservedEngine::from_codec_wire("7.0.0-dev.20260526.1");
        let err = validate(&observed).expect_err("a different version must be refused");
        assert!(
            matches!(err, TsgoApiError::UnsupportedTsgoWire(ref m) if m.contains("7.0.0-dev.20260526.1")),
            "got {err:?}"
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

    // ── DISCRIMINATING: a mismatched fingerprint is refused ─────────────────
    #[test]
    fn mismatched_fingerprint_refuses_to_start() {
        let observed = ObservedEngine {
            engine_version: PINNED.engine_version.to_string(),
            wire_fingerprint: PINNED.wire_fingerprint() ^ 0xdead_beef,
        };
        let err = validate(&observed).expect_err("a diverged wire must be refused");
        assert!(
            matches!(err, TsgoApiError::UnsupportedTsgoWire(_)),
            "got {err:?}"
        );
    }

    // ── A mutated PIN must reject the genuine engine (RED-style sensitivity) ─
    #[test]
    fn mutated_pin_rejects_the_real_engine() {
        // Simulate a maintainer recording the wrong pinned version: the real
        // engine (reporting the true version + the true codec fingerprint) must
        // then FAIL the gate against the mutated pin.
        let mut bad_pin = PINNED;
        bad_pin.engine_version = "6.9.9-wrong";
        let real = ObservedEngine::from_codec_wire(PINNED.engine_version);
        assert!(
            validate_against(&real, &bad_pin).is_err(),
            "the genuine engine must not pass a mismatched pin"
        );

        // And the correctly-pinned manifest accepts it.
        assert!(validate_against(&real, &PINNED).is_ok());
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
}
