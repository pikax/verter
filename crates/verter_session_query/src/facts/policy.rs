//! The layout and precision widths one host builds its fact sets with.
//!
//! A [`FactPolicy`] is chosen once, at a host's composition root, before
//! the host builds a store or admits work. Every [`FactSet`] and every
//! [`FactReadSet`] carries the policy it was built under, and every set
//! built from another inherits it, so no construction path can fall back
//! to a width the host did not choose.
//!
//! The policy names four decisions. They hold the same production value
//! in pairs, but none stands in for another:
//!
//! | Decision | Production |
//! |---|---:|
//! | [`FactPolicy::fresh_slice_max`]: widest freshly sealed set held as one slice | 1024 |
//! | [`FactPolicy::strict_root_threshold`]: completed-carrier width above which its completion takes the strict-root rewrite | 1024 |
//! | [`FactPolicy::precise_domain_max`]: most distinct facts a compaction bucket keeps precise | 1024 |
//! | [`FactPolicy::extension_slice_max`]: widest set held as one slice when built by extending another | 32 |
//!
//! In a shipped build the policy is zero-sized and every accessor is the
//! production constant, so carrying it costs no storage, allocation or
//! branch. Under `test-support` it holds the host's widths, and a host can
//! be composed with smaller ones ([`FactPolicy::with_widths`]) so a gate
//! test reaches the same branches a production-width fixture reaches —
//! the same algorithms, the same charging, only a smaller crossover.
//!
//! Equality, ordering, hashing and digests of a set stay content-defined:
//! two sets with the same facts are equal whatever policy built them. The
//! policy guards construction and reuse instead: combining sets built
//! under different policies is a defect, checked before any shortcut that
//! would reuse one set's storage for another ([`FactPolicy::assert_compatible`]).
//!
//! [`FactSet`]: super::fact_set::FactSet
//! [`FactReadSet`]: super::fact_read_set::FactReadSet

use std::sync::Arc;

use super::fact_set::FactSet;
use super::version::FactVersionRef;

/// Production [`FactPolicy::fresh_slice_max`].
const FRESH_SLICE_MAX: usize = 1_024;
/// Production [`FactPolicy::strict_root_threshold`].
const STRICT_ROOT_THRESHOLD: usize = 1_024;
/// Production [`FactPolicy::precise_domain_max`].
const PRECISE_DOMAIN_MAX: usize = 1_024;
/// Production [`FactPolicy::extension_slice_max`].
const EXTENSION_SLICE_MAX: usize = 32;

/// The widths one host builds its fact sets with. See the module docs.
#[derive(Clone, Copy, PartialEq, Eq)]
pub struct FactPolicy {
    #[cfg(any(test, feature = "test-support"))]
    widths: FactWidths,
}

// A shipped build carries the policy for free: it occupies no byte of any
// set, tracer or snapshot that holds it, and every accessor is a constant.
#[cfg(not(any(test, feature = "test-support")))]
const _: () = {
    assert!(std::mem::size_of::<FactPolicy>() == 0);
    assert!(FactPolicy::production().fresh_slice_max() == FRESH_SLICE_MAX);
    assert!(FactPolicy::production().strict_root_threshold() == STRICT_ROOT_THRESHOLD);
    assert!(FactPolicy::production().precise_domain_max() == PRECISE_DOMAIN_MAX);
    assert!(FactPolicy::production().extension_slice_max() == EXTENSION_SLICE_MAX);
};

/// The four widths of a test-support policy ([`FactPolicy::with_widths`]).
#[cfg(any(test, feature = "test-support"))]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FactWidths {
    /// See [`FactPolicy::fresh_slice_max`].
    pub fresh_slice_max: u16,
    /// See [`FactPolicy::strict_root_threshold`].
    pub strict_root_threshold: u16,
    /// See [`FactPolicy::precise_domain_max`].
    pub precise_domain_max: u16,
    /// See [`FactPolicy::extension_slice_max`].
    pub extension_slice_max: u16,
}

#[cfg(any(test, feature = "test-support"))]
impl FactWidths {
    /// The production widths.
    pub const PRODUCTION: Self = Self {
        fresh_slice_max: FRESH_SLICE_MAX as u16,
        strict_root_threshold: STRICT_ROOT_THRESHOLD as u16,
        precise_domain_max: PRECISE_DOMAIN_MAX as u16,
        extension_slice_max: EXTENSION_SLICE_MAX as u16,
    };
}

/// Why a set of widths cannot be a policy.
#[cfg(any(test, feature = "test-support"))]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FactPolicyError {
    /// A width of zero: every decision needs at least one fact on the
    /// narrow side.
    ZeroWidth,
    /// The extension slice limit exceeds the fresh slice limit: a set built
    /// by extension would stay a slice wider than a freshly sealed one may.
    ExtensionWiderThanFresh,
}

impl std::fmt::Debug for FactPolicy {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("FactPolicy")
            .field("fresh_slice_max", &self.fresh_slice_max())
            .field("strict_root_threshold", &self.strict_root_threshold())
            .field("precise_domain_max", &self.precise_domain_max())
            .field("extension_slice_max", &self.extension_slice_max())
            .finish()
    }
}

impl FactPolicy {
    /// The production policy: the only one a shipped build has.
    #[inline]
    #[must_use]
    pub const fn production() -> Self {
        Self {
            #[cfg(any(test, feature = "test-support"))]
            widths: FactWidths::PRODUCTION,
        }
    }

    /// A policy with `widths`, for a host composed to reach wide-set
    /// branches with small fixtures.
    ///
    /// # Errors
    ///
    /// When a width is zero, or the extension slice limit exceeds the fresh
    /// slice limit.
    #[cfg(any(test, feature = "test-support"))]
    pub fn with_widths(widths: FactWidths) -> Result<Self, FactPolicyError> {
        if widths.fresh_slice_max == 0
            || widths.strict_root_threshold == 0
            || widths.precise_domain_max == 0
            || widths.extension_slice_max == 0
        {
            return Err(FactPolicyError::ZeroWidth);
        }
        if widths.extension_slice_max > widths.fresh_slice_max {
            return Err(FactPolicyError::ExtensionWiderThanFresh);
        }
        Ok(Self { widths })
    }

    /// The widths of this policy.
    #[cfg(any(test, feature = "test-support"))]
    #[must_use]
    pub const fn widths(self) -> FactWidths {
        self.widths
    }

    /// The widest freshly sealed set held as one shared slice; a wider one
    /// is sealed into tree storage that charges itself.
    #[inline]
    #[must_use]
    pub const fn fresh_slice_max(self) -> usize {
        #[cfg(any(test, feature = "test-support"))]
        {
            self.widths.fresh_slice_max as usize
        }
        #[cfg(not(any(test, feature = "test-support")))]
        {
            FRESH_SLICE_MAX
        }
    }

    /// The completed-carrier width above which a structural completion
    /// carries its self-roots as one strict-world witness, when the
    /// completion is what made the carrier that wide.
    #[inline]
    #[must_use]
    pub const fn strict_root_threshold(self) -> usize {
        #[cfg(any(test, feature = "test-support"))]
        {
            self.widths.strict_root_threshold as usize
        }
        #[cfg(not(any(test, feature = "test-support")))]
        {
            STRICT_ROOT_THRESHOLD
        }
    }

    /// The most distinct facts a compaction bucket keeps precise; the first
    /// beyond lifts the bucket to its terminal aggregate.
    #[inline]
    #[must_use]
    pub const fn precise_domain_max(self) -> usize {
        #[cfg(any(test, feature = "test-support"))]
        {
            self.widths.precise_domain_max as usize
        }
        #[cfg(not(any(test, feature = "test-support")))]
        {
            PRECISE_DOMAIN_MAX
        }
    }

    /// The widest set held as one shared slice when it is built by
    /// extending another set; a wider extension is a tree.
    #[inline]
    #[must_use]
    pub const fn extension_slice_max(self) -> usize {
        #[cfg(any(test, feature = "test-support"))]
        {
            self.widths.extension_slice_max as usize
        }
        #[cfg(not(any(test, feature = "test-support")))]
        {
            EXTENSION_SLICE_MAX
        }
    }

    /// Check that a set built under `other` may be combined with, or reuse
    /// the storage of, a set built under this policy. Called before any
    /// unchanged-set or shared-storage shortcut. Always true in a shipped
    /// build, where it compiles away; a plain assertion under test support,
    /// so release-mode tests check it too.
    #[inline]
    #[track_caller]
    pub fn assert_compatible(self, other: Self) {
        #[cfg(any(test, feature = "test-support"))]
        assert_eq!(
            self, other,
            "fact sets built under different fact policies were combined"
        );
        #[cfg(not(any(test, feature = "test-support")))]
        let _ = other;
    }

    /// The empty set.
    #[must_use]
    pub fn empty_set(self) -> FactSet {
        FactSet::empty_under(self)
    }

    /// The set of `facts`, in any order and with any repetition.
    #[must_use]
    pub fn set_of(self, facts: impl Into<Vec<FactVersionRef>>) -> FactSet {
        FactSet::from_facts_under(self, facts.into())
    }

    /// The set of a shared run of facts: the run itself when it is already
    /// canonical and no wider than [`Self::fresh_slice_max`], otherwise
    /// sealed afresh.
    #[must_use]
    pub fn set_of_shared(self, facts: Arc<[FactVersionRef]>) -> FactSet {
        FactSet::from_shared_under(self, facts)
    }

    /// Seal an arbitrary fact collection as a complete signature (see
    /// [`super::fact_read_set::seal_canonical_signature`]).
    #[must_use]
    pub fn seal(self, facts: Vec<FactVersionRef>) -> FactSet {
        super::fact_read_set::seal_canonical_signature(self, facts)
    }

    /// A fresh tracer building under this policy.
    #[must_use]
    pub fn read_set(self) -> super::fact_read_set::FactReadSet {
        super::fact_read_set::FactReadSet::new(self)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn widths_are_validated_and_production_is_the_constants() {
        let production = FactPolicy::production();
        assert_eq!(production.fresh_slice_max(), 1_024);
        assert_eq!(production.strict_root_threshold(), 1_024);
        assert_eq!(production.precise_domain_max(), 1_024);
        assert_eq!(production.extension_slice_max(), 32);
        assert_eq!(
            FactPolicy::with_widths(FactWidths {
                extension_slice_max: 0,
                ..FactWidths::PRODUCTION
            }),
            Err(FactPolicyError::ZeroWidth)
        );
        assert_eq!(
            FactPolicy::with_widths(FactWidths {
                fresh_slice_max: 16,
                extension_slice_max: 17,
                ..FactWidths::PRODUCTION
            }),
            Err(FactPolicyError::ExtensionWiderThanFresh)
        );
        assert_eq!(
            FactPolicy::with_widths(FactWidths::PRODUCTION),
            Ok(production)
        );
    }

    #[test]
    #[should_panic(expected = "different fact policies")]
    fn combining_policies_is_refused() {
        let small = FactPolicy::with_widths(FactWidths {
            fresh_slice_max: 8,
            strict_root_threshold: 8,
            precise_domain_max: 8,
            extension_slice_max: 4,
        })
        .expect("valid widths");
        FactPolicy::production().assert_compatible(small);
    }
}
