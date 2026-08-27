//! Resolution-world identity vocabulary: `ResolutionWorldId`,
//! `WorkspaceAuthorityId`, `SessionFingerprint`, `ResolutionPopulation`.
//!
//! `ResolutionWorldBasis` (`attempt_outcome.rs`) compares this exact
//! structured tuple across attempts for basis-restart correctness — one
//! identity tuple, not two "equivalent" tuples requiring a conversion at
//! that fence. The host remains responsible for MINTING real values
//! (`WorkspaceRead::capture_resolution_world` and its authority/session
//! counters) and publishing captured worlds; this module owns the value
//! types plus narrow checked constructors that preserve the `0`-placeholder
//! invariant, since Rust has no friend-crate visibility to keep minting
//! `pub(crate)` across the boundary once the host constructs these values.

/// Identity of one immutable resolution world.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct ResolutionWorldId(u64);

impl ResolutionWorldId {
    /// Sentinel identity for a `ResolutionBasis` not yet bound to a real
    /// captured resolution world. `from_raw` asserts its raw value is
    /// non-zero, so this `0` sentinel can never equal, or be confused with,
    /// a genuinely minted world id — a placeholder basis built from it fails
    /// every real-world comparison instead of silently validating against
    /// unrelated live data.
    pub const UNBOUND_PLACEHOLDER: Self = Self(0);

    /// Mint a real (non-zero) world id. The host's per-engine world counter
    /// is the sole production caller.
    ///
    /// # Panics
    /// Panics if `raw` is `0` — `0` is reserved for
    /// [`Self::UNBOUND_PLACEHOLDER`] and must never be confused with a real
    /// minted id.
    pub fn from_raw(raw: u64) -> Self {
        assert_ne!(raw, 0, "resolution world ids must be non-zero");
        Self(raw)
    }

    /// Test-only constructor for a downstream crate's own unit tests —
    /// production code must never mint a `ResolutionWorldId` this way; the
    /// only production mint path is the host's world counter via
    /// [`Self::from_raw`]. Gated behind `test-support` per the repo-wide
    /// convention.
    #[cfg(any(test, feature = "test-support"))]
    #[must_use]
    pub fn test_only(raw: u64) -> Self {
        Self::from_raw(raw)
    }
}

/// Process-unique authority a resolution world belongs to.
///
/// `ResolutionWorldId`'s counter restarts at `1` for every host engine, so a
/// bare `ResolutionWorldId` is unique only WITHIN one engine, not globally.
/// A cross-engine consumer (`ResolutionBasis`, which compares bases across
/// attempts that may originate from different engines in test/multi-workspace
/// contexts) needs this discriminator alongside the world id.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct WorkspaceAuthorityId(u64);

impl WorkspaceAuthorityId {
    /// Sentinel authority for a placeholder `ResolutionBasis` — see
    /// [`ResolutionWorldId::UNBOUND_PLACEHOLDER`].
    pub const UNBOUND_PLACEHOLDER: Self = Self(0);

    /// Mint a real authority id. The host's process-unique authority
    /// counter is the sole production caller.
    pub fn from_raw(raw: u64) -> Self {
        Self(raw)
    }

    /// Test-only constructor — see [`ResolutionWorldId::test_only`].
    #[cfg(any(test, feature = "test-support"))]
    #[must_use]
    pub fn test_only(raw: u64) -> Self {
        Self::from_raw(raw)
    }
}

/// A per-session fingerprint distinguishing a session-scoped resolution
/// population from the base population and from other sessions.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct SessionFingerprint([u8; 16]);

impl SessionFingerprint {
    /// Mint a real (non-zero) session fingerprint. The host's session
    /// lifecycle is the sole production caller.
    ///
    /// # Panics
    /// Panics if `raw` is `0` — session fingerprints must be non-zero.
    pub fn from_raw(raw: u64) -> Self {
        assert_ne!(raw, 0, "session fingerprints must be non-zero");
        let mut bytes = [0_u8; 16];
        bytes[..8].copy_from_slice(&raw.to_le_bytes());
        bytes[8..].copy_from_slice(&(!raw).to_le_bytes());
        Self(bytes)
    }

    /// Test-only constructor — see [`ResolutionWorldId::test_only`].
    #[cfg(any(test, feature = "test-support"))]
    #[must_use]
    pub fn test_only(raw: u64) -> Self {
        Self::from_raw(raw)
    }
}

/// Which resolution population a fact/world belongs to: the shared base
/// population, or one session's overlay population.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum ResolutionPopulation {
    Base,
    Session(SessionFingerprint),
}

#[cfg(test)]
#[path = "resolution_world_identity_tests.rs"]
mod tests;
