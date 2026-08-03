//! Dependency-neutral fact-signature carriers.
//!
//! These types are the single validity rail shared by workspace resolution and
//! the session cache runtime. Domain owners provide [`FactVersionValidator`]
//! implementations; cache entries always retain a [`ReadSetSignature`].

use std::sync::Arc;

use crate::fact_read_set::FactReadSetFinalise;
use crate::fact_registry::{FactKey, FactLane};
use crate::resolution_currency::{ResolutionFactKey, ResolutionFactRef};

pub type FactHash16 = [u8; 16];

/// Per-slot candidate cap for every fact-validated cache slot.
///
/// One shared bound: the workspace resolution slot and the session
/// `ValidatedFactCache` slot retain at most this many concurrent
/// candidates per key and evict the oldest (FIFO) on the next
/// admission. Declared here — the dependency-neutral carrier module —
/// so both slots cannot drift apart.
pub const CANDIDATE_CAP: usize = 4;

/// Content-free parse-environment identity carried by source-env facts.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct ParseEnvHash(FactHash16);

impl ParseEnvHash {
    #[must_use]
    pub const fn from_env_hash(hash: FactHash16) -> Self {
        Self(hash)
    }
}

/// Derived per-canonical hashes the session's store view snapshots.
///
/// The `ImportRoute` kind is deliberately ABSENT: an owner's
/// import-route dependency is a RESOLVE-domain fact
/// ([`ResolveImportsFactRef::Resolution`]) carrying the sealed
/// resolution transaction's own observations, validated against a
/// captured immutable resolution world. Expressing it as a derived hash
/// forced the store-view build to re-resolve every published owner's
/// known-miss specifiers just to compose the digest.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum DerivedFactKind {
    Route,
    DirectSource,
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct ParseFactRef {
    pub canonical_id: String,
    pub key: FactKey,
    pub lane: FactLane,
    pub expected_hash: FactHash16,
}

/// The closed resolve-imports fact domain.
///
/// Semantic resolved-import facts and workspace resolution-currency facts are
/// alternatives under the same `FactVersionRef::ResolveImports` discriminant;
/// neither domain has a sibling witness or admission rail.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum ResolveImportsFactRef {
    Semantic {
        canonical_id: String,
        key: FactKey,
        lane: FactLane,
        expected_hash: FactHash16,
    },
    Resolution(ResolutionFactRef),
}

impl ResolveImportsFactRef {
    #[must_use]
    pub fn canonical_id(&self) -> Option<&str> {
        match self {
            Self::Semantic { canonical_id, .. } => Some(canonical_id),
            Self::Resolution(fact) => fact.key.canonical_id(),
        }
    }

    #[must_use]
    pub fn resolution_fact(&self) -> Option<&ResolutionFactRef> {
        match self {
            Self::Resolution(fact) => Some(fact),
            Self::Semantic { .. } => None,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct RouteSurfaceFactRef {
    pub canonical_id: String,
    pub key: FactKey,
    pub lane: FactLane,
    pub expected_hash: FactHash16,
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum FactVersionRef {
    FileWholeHash {
        canonical_id: String,
        hash: FactHash16,
    },
    DerivedFactHash {
        canonical_id: String,
        kind: DerivedFactKind,
        hash: FactHash16,
    },
    Parse(ParseFactRef),
    ResolveImports(ResolveImportsFactRef),
    RouteSurface(RouteSurfaceFactRef),
    FileSourceEnv {
        canonical_id: String,
        parse_env_hash: ParseEnvHash,
        parser_version: u32,
        file_language_id: verter_language::FileLanguage,
    },
    ProjectGeneration {
        generation: u64,
    },
}

impl FactVersionRef {
    #[must_use]
    pub fn canonical_id(&self) -> Option<&str> {
        match self {
            Self::FileWholeHash { canonical_id, .. }
            | Self::DerivedFactHash { canonical_id, .. }
            | Self::FileSourceEnv { canonical_id, .. } => Some(canonical_id),
            Self::Parse(fact) => Some(&fact.canonical_id),
            Self::ResolveImports(fact) => fact.canonical_id(),
            Self::RouteSurface(fact) => Some(&fact.canonical_id),
            Self::ProjectGeneration { .. } => None,
        }
    }
}

/// Single validation interface for a fact-version signature.
pub trait FactVersionValidator {
    fn validates_fact_version(&self, fact: &FactVersionRef) -> bool;

    #[inline]
    fn validates_fact_signature(&self, facts: &[FactVersionRef]) -> bool {
        facts.iter().all(|fact| self.validates_fact_version(fact))
    }
}

#[derive(Clone, Debug)]
pub struct ReadSetSignature {
    pub facts: Arc<[FactVersionRef]>,
    pub overflowed: bool,
}

impl ReadSetSignature {
    #[must_use]
    pub fn new(facts: Arc<[FactVersionRef]>) -> Self {
        Self {
            facts,
            overflowed: false,
        }
    }

    #[must_use]
    pub fn overflow() -> Self {
        Self {
            facts: Arc::from([]),
            overflowed: true,
        }
    }

    #[must_use]
    pub fn empty() -> Self {
        Self {
            facts: Arc::from([]),
            overflowed: false,
        }
    }

    #[must_use]
    pub fn validates(&self, validator: &dyn FactVersionValidator) -> bool {
        !self.overflowed && validator.validates_fact_signature(&self.facts)
    }

    #[must_use]
    pub fn canonical_ids(&self) -> Vec<Arc<str>> {
        let mut seen = rustc_hash::FxHashSet::<Arc<str>>::default();
        let mut out = Vec::new();
        for fact in self.facts.iter() {
            let Some(canonical_id) = fact.canonical_id() else {
                continue;
            };
            let canonical_id: Arc<str> = Arc::from(canonical_id);
            if seen.insert(Arc::clone(&canonical_id)) {
                out.push(canonical_id);
            }
        }
        out
    }

    /// The canonicals this signature observed as a PATH — a typed probe, a
    /// realpath, or a manifest fingerprint — in first-observation order.
    ///
    /// Strictly narrower than [`Self::canonical_ids`], and deliberately so —
    /// see [`ResolutionFactKey::reobservable_path_canonical_id`] for which
    /// families qualify and why the rest must not.
    #[must_use]
    pub fn resolution_path_canonical_ids(&self) -> Vec<Arc<str>> {
        let mut seen = rustc_hash::FxHashSet::<Arc<str>>::default();
        let mut out = Vec::new();
        for fact in self.facts.iter() {
            let FactVersionRef::ResolveImports(fact) = fact else {
                continue;
            };
            let Some(fact) = fact.resolution_fact() else {
                continue;
            };
            let Some(canonical_id) = fact.key.reobservable_path_canonical_id() else {
                continue;
            };
            let canonical_id: Arc<str> = Arc::from(canonical_id);
            if seen.insert(Arc::clone(&canonical_id)) {
                out.push(canonical_id);
            }
        }
        out
    }

    #[must_use]
    pub const fn is_overflow(&self) -> bool {
        self.overflowed
    }

    #[must_use]
    pub const fn is_cacheable(&self) -> bool {
        !self.overflowed
    }

    #[must_use]
    pub(crate) fn resolution_fact_version(
        &self,
        key: &ResolutionFactKey,
    ) -> Option<crate::resolution_currency::ResolutionFactVersion> {
        self.facts.iter().find_map(|fact| {
            let FactVersionRef::ResolveImports(fact) = fact else {
                return None;
            };
            let fact = fact.resolution_fact()?;
            (&fact.key == key).then_some(fact.version)
        })
    }
}

#[derive(Debug, Clone)]
pub enum SignatureAdmission {
    Cacheable(ReadSetSignature),
    NonCacheable(verter_audit::NonAdmissionReason),
}

impl SignatureAdmission {
    #[must_use]
    pub fn from_finalise(finalise: FactReadSetFinalise) -> Self {
        match finalise {
            FactReadSetFinalise::Ok(facts) => Self::Cacheable(ReadSetSignature::new(facts)),
            FactReadSetFinalise::NonCacheable(_) => {
                Self::NonCacheable(verter_audit::NonAdmissionReason::UnresolvedProvenance)
            }
            FactReadSetFinalise::Overflow => {
                Self::NonCacheable(verter_audit::NonAdmissionReason::SignatureOverflow)
            }
        }
    }

    #[must_use]
    pub fn cacheable(&self) -> Option<&ReadSetSignature> {
        match self {
            Self::Cacheable(signature) => Some(signature),
            Self::NonCacheable(_) => None,
        }
    }

    #[must_use]
    pub fn into_cacheable(self) -> Option<ReadSetSignature> {
        match self {
            Self::Cacheable(signature) => Some(signature),
            Self::NonCacheable(_) => None,
        }
    }
}
