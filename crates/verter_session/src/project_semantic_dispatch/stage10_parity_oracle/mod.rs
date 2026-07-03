//! Dual-leg body-source parity oracle (test-only).
//!
//! Proves that the locator-backed production body source
//! (`lower_locator` + the `LowerLocator` query) publishes BYTE-IDENTICAL
//! surfaces to the retained prepared-body implementation, by running the
//! same fixture graph through two fresh hermetic hosts:
//!
//! - **`BodyLeg::NewLocator`** — the production path, exactly as shipped.
//! - **`BodyLeg::LegacyPreparedBody`** — a thread-local RAII toggle routes
//!   `lower_decl_body_with_provenance` through the retained
//!   prepared-body implementation ([`legacy_leg_tests`]) for the duration
//!   of the leg.
//!
//! Comparison is over canonical-JSON **published-surface envelopes**
//! ([`envelope_tests::OracleEnvelope`]) — full published DTOs with
//! sorted-map canonicalisation — never graph-node ids and never `Debug`
//! output. Committed goldens are NOT the oracle truth; the two live legs
//! are.
//!
//! The module is wired ONLY behind `#[cfg(test)]` (see
//! `project_semantic_dispatch/mod.rs`) — it never links into production
//! or plain debug builds. The single production touch-point is the
//! guarded delegation at the top of `lower_decl_body_with_provenance`,
//! itself `#[cfg(test)]`.

use std::cell::Cell;

pub(crate) mod cases_tests;
pub(crate) mod envelope_tests;
pub(crate) mod legacy_leg_tests;
mod runner_tests;

thread_local! {
    /// Whether the legacy prepared-body leg is active on this thread.
    static LEGACY_LEG_ACTIVE: Cell<bool> = const { Cell::new(false) };
    /// Number of prepared-body reads the legacy leg served on this thread
    /// since the guard was activated. The runner asserts this is non-zero
    /// on the legacy leg (anti-vacuity: the leg genuinely exercised the
    /// retained body source).
    static LEGACY_PREPARED_BODY_READS: Cell<u64> = const { Cell::new(0) };
}

/// RAII toggle for the legacy prepared-body leg. While alive (on the
/// constructing thread), `lower_decl_body_with_provenance` delegates to
/// the retained prepared-body implementation and bumps the read counter.
pub(crate) struct LegacyPreparedBodyLegGuard {
    _priv: (),
}

impl LegacyPreparedBodyLegGuard {
    /// Activate the legacy leg on the current thread, resetting the read
    /// counter so the caller observes only this leg's reads.
    pub(crate) fn activate() -> Self {
        LEGACY_LEG_ACTIVE.with(|f| f.set(true));
        LEGACY_PREPARED_BODY_READS.with(|c| c.set(0));
        Self { _priv: () }
    }
}

impl Drop for LegacyPreparedBodyLegGuard {
    fn drop(&mut self) {
        LEGACY_LEG_ACTIVE.with(|f| f.set(false));
    }
}

/// Whether the legacy prepared-body leg is active on this thread — the
/// single predicate the guarded delegation in
/// `lower_decl_body_with_provenance` consults.
pub(super) fn legacy_prepared_body_leg_active() -> bool {
    LEGACY_LEG_ACTIVE.with(|f| f.get())
}

/// Bumped by the retained prepared-body implementation on every body it
/// serves while the legacy leg is active.
pub(super) fn note_legacy_prepared_body_read() {
    LEGACY_PREPARED_BODY_READS.with(|c| c.set(c.get() + 1));
}

/// Prepared-body reads served since the current legacy-leg guard was
/// activated (thread-local).
pub(crate) fn legacy_prepared_body_reads() -> u64 {
    LEGACY_PREPARED_BODY_READS.with(|c| c.get())
}

/// The published-surface classes the parity oracle must cover. Each class
/// has at least one [`PublishedSurfaceCase`] adapter; the manifest test in
/// [`runner_tests`] proves exhaustive coverage.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(crate) enum Stage10SurfaceClass {
    /// Full native component-meta payload for a `.vue` component.
    ComponentMetaPayload,
    /// Fallthrough / root-inheritance metadata surface.
    FallthroughRootInheritance,
    /// Macro own-body provenance (`declared_in_macro_type_arg`) on the
    /// published prop surface.
    MacroOwnBodyProvenance,
    /// Open-key-domain carrier-stop (L1): an open object-filter utility
    /// publishes as a shallow carrier, never a materialised surface.
    OpenKeyDomainCarrierStopL1,
    /// Cross-file module-augmentation merged declaration surface.
    ModuleAugmentationSurface,
    /// Generic instantiation with substituted type arguments.
    GenericSubstitution,
}

impl Stage10SurfaceClass {
    /// Every class, for the exhaustive-coverage manifest test.
    pub(crate) const ALL: &'static [Stage10SurfaceClass] = &[
        Stage10SurfaceClass::ComponentMetaPayload,
        Stage10SurfaceClass::FallthroughRootInheritance,
        Stage10SurfaceClass::MacroOwnBodyProvenance,
        Stage10SurfaceClass::OpenKeyDomainCarrierStopL1,
        Stage10SurfaceClass::ModuleAugmentationSurface,
        Stage10SurfaceClass::GenericSubstitution,
    ];
}

/// Which body source a leg runs on.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum BodyLeg {
    /// The production locator-backed body source.
    NewLocator,
    /// The retained prepared-body implementation behind the RAII guard.
    LegacyPreparedBody,
}
