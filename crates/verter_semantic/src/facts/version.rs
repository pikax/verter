//! `FactVersionRef`: the crate-wide, dependency-neutral fact-signature
//! identity vocabulary. Every domain (module resolution, parse, route
//! surface, program analysis, whole-file/project scalars, and the
//! cache-validation self-root witness) shares this one closed identity
//! type — a domain owner provides a `FactVersionValidator` implementation
//! (host-owned), but the identity IR itself is single-owner.
//!
//! This is immutable identity IR only — cache authority (the fact ledger,
//! admission, validation, mutation propagation, version counters,
//! compaction, replay ledgers, publication, invalidation,
//! candidate-retention policy) stays workspace/session-owned.

use std::sync::Arc;

use crate::facts::registry::{FactKey, FactLane, SymbolSpace};
use crate::facts::resolution::ResolutionFactRef;
use crate::resolver_core::resolution_world_identity::{ResolutionPopulation, ResolutionWorldId};

pub type FactHash16 = [u8; 16];

/// The compaction domain a precise fact belongs to.
///
/// Compaction is DOMAIN-WISE, not whole-signature: a domain whose precise
/// bucket outgrows its threshold is replaced by that domain's single
/// terminal aggregate, and every other domain stays precise. The
/// classification is total over [`FactVersionRef`] by construction — see
/// [`compaction_domain`], whose exhaustive match is the compiler rail that
/// makes a new fact variant impossible to add without giving it a domain.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum CompactionDomain {
    /// `FileWholeHash`, `DerivedFactHash`, `Parse` — everything whose
    /// validity moves with file CONTENT.
    Content,
    /// `FileSourceEnv` — `parse_env_hash` / `parse_key` /
    /// `file_language_id`. Deliberately NOT [`Self::Content`]: the two
    /// production paths that move these do not bump the content
    /// generation, so folding source-env into the content domain would
    /// let an env change survive a content-compacted signature.
    SourceEnv,
    /// `ResolveImports::Semantic` — the session's resolved-import facts.
    SemanticImports,
    /// `ResolveImports::Resolution` — workspace resolution currency.
    Resolution,
    /// `RouteSurface` — module-augmentation shape and effective export
    /// sets.
    RouteSurface,
    /// `ProjectGeneration` — workspace SHAPE only. Already terminal, so
    /// this domain's bucket needs no lifting.
    WorkspaceShape,
}

/// Total classification of a fact into its compaction domain.
///
/// The match is exhaustive and wildcard-free on purpose: adding a
/// [`FactVersionRef`] variant without assigning it a domain is a compile
/// error, not a silently-uncompacted bucket.
#[must_use]
pub fn compaction_domain(fact: &FactVersionRef) -> CompactionDomain {
    match fact {
        FactVersionRef::FileWholeHash { .. }
        | FactVersionRef::DerivedFactHash { .. }
        | FactVersionRef::Parse(_) => CompactionDomain::Content,
        FactVersionRef::FileSourceEnv { .. } => CompactionDomain::SourceEnv,
        FactVersionRef::ResolveImports(ResolveImportsFactRef::Semantic { .. }) => {
            CompactionDomain::SemanticImports
        }
        FactVersionRef::ResolveImports(ResolveImportsFactRef::Resolution(_)) => {
            CompactionDomain::Resolution
        }
        FactVersionRef::RouteSurface(_) => CompactionDomain::RouteSurface,
        FactVersionRef::ProjectGeneration { .. } => CompactionDomain::WorkspaceShape,
        // An aggregate is already its own domain's terminal form.
        FactVersionRef::DomainGeneration(fact) => fact.domain,
        // A strict self-root world is a terminal witness for the structural
        // self-root axis, not a bucket eligible for domain compaction.
        FactVersionRef::StrictSelfRootWorld(_) => CompactionDomain::Content,
        // Whole-function body facts derive from file content; their validity
        // moves with the content generation.
        FactVersionRef::ProgramAnalysis(_) => CompactionDomain::Content,
    }
}

/// The exact, typed value a domain's terminal aggregate pins.
///
/// Exact stamps, never a digest: the tuple is a handful of scalars and a
/// signature carries at most one aggregate per domain, so there is nothing
/// to compress and no reason to take on a collision assumption.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum AggregateStamp {
    /// A single monotonic counter. Sound only for a domain with exactly
    /// ONE producer — `Content`, `SourceEnv`, `WorkspaceShape`.
    Generation(u64),
    /// The captured resolution world's ROOT IDENTITY.
    ///
    /// Deliberately not a ledger counter — root identity advances on base
    /// and session publication, covering the whole domain including
    /// `ContextSelection` (versioned outside the fact ledger).
    ResolutionRoots {
        base: ResolutionWorldId,
        session: Option<ResolutionWorldId>,
    },
    /// [`CompactionDomain::SemanticImports`]'s composite. See
    /// [`SemanticImportsStamp`].
    SemanticImports(SemanticImportsStamp),
    /// [`CompactionDomain::RouteSurface`]'s composite. See
    /// [`RouteSurfaceStamp`].
    RouteSurface(RouteSurfaceStamp),
}

/// The composite [`CompactionDomain::RouteSurface`] stamp.
///
/// * `route_surface` — the augmentation WORLD moved: a published
///   augmenter set changed, or an artifact retirement removed
///   contributors.
/// * `content` — an augmenter's content moved, which moves its
///   `parse_stable_hash` and so the set fingerprint.
/// * `source_env` — the parse-env identity the augmenter artifacts are
///   keyed by moved.
/// * `workspace_shape` — the project graph moved, which re-composes the
///   `AugmentationTargetKey` the index is keyed by.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct RouteSurfaceStamp {
    pub route_surface: u64,
    pub content: u64,
    pub source_env: u64,
    pub workspace_shape: u64,
}

/// The captured resolution world's root identity, as a component of a
/// composite stamp.
///
/// The same quantity [`AggregateStamp::ResolutionRoots`] carries, named
/// separately so a composite can hold it by value.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct ResolutionRootsStamp {
    pub base: ResolutionWorldId,
    pub session: Option<ResolutionWorldId>,
}

/// The composite [`CompactionDomain::SemanticImports`] stamp.
///
/// * `semantic_imports` — the store's own membership moved.
/// * `content` — a `content_hash` key dimension moved.
/// * `source_env` — a `parse_env_hash` key dimension moved.
/// * `resolution` — the resolved-import world the `resolve_env_hash`
///   dimension and the producer's route witness are composed against was
///   republished.
/// * `workspace_shape` — the project graph moved.
///
/// Over-coverage is deliberate where the domains overlap.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct SemanticImportsStamp {
    pub semantic_imports: u64,
    pub content: u64,
    pub source_env: u64,
    pub resolution: ResolutionRootsStamp,
    pub workspace_shape: u64,
}

/// Identity of the overlay SET a session view has installed.
///
/// The same quantity a session population is keyed on: it captures *which*
/// overlays are installed, so two sessions holding identical overlay sets
/// share an identity and a session whose overlays change gets a fresh one.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct SessionOverlayFingerprint(u64);

impl SessionOverlayFingerprint {
    /// `None` for the ZERO fingerprint, which is not a session identity:
    /// "no overlays installed" IS the base view.
    #[must_use]
    pub const fn new(fingerprint: u64) -> Option<Self> {
        if fingerprint == 0 {
            None
        } else {
            Some(Self(fingerprint))
        }
    }

    #[must_use]
    pub const fn get(self) -> u64 {
        self.0
    }
}

/// Process-unique identity of one request-completion overlay.
///
/// Minted at overlay construction and never reused within the process, so
/// an aggregate minted under one request's overlay can never be satisfied
/// by another request's overlay that reached the same revision.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct OverlayId(u64);

impl OverlayId {
    /// Mint the next id. One relaxed increment; monotonic within the
    /// process, and never `0` so a zero-initialised field cannot pass for
    /// a minted one.
    #[must_use]
    pub fn fresh() -> Self {
        static NEXT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(1);
        Self(NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed))
    }

    #[must_use]
    pub const fn get(self) -> u64 {
        self.0
    }
}

/// The population a request-completion overlay REFINES.
///
/// Deliberately not `ViewPopulation` itself: a completion overlay's parent
/// is always a durable population, never another completion overlay.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum ViewPopulationParent {
    Base,
    SessionOverlay(SessionOverlayFingerprint),
}

impl From<ViewPopulationParent> for ViewPopulation {
    #[inline]
    fn from(parent: ViewPopulationParent) -> Self {
        match parent {
            ViewPopulationParent::Base => Self::Base,
            ViewPopulationParent::SessionOverlay(fingerprint) => Self::SessionOverlay(fingerprint),
        }
    }
}

/// A completion overlay's shadowing state as its holder can currently
/// report it.
///
/// Three answers, not two: "shadows nothing", "shadows this exact state",
/// and "a writer holds the state open".
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum CompletionOverlayState {
    /// Shadows no fact any validation reads.
    Empty,
    /// Shadows at least one fact, at this exact revision.
    Shadowing {
        overlay_id: OverlayId,
        revision: u64,
    },
    /// A writer is mid-update; the state cannot be named.
    InFlight,
}

/// The exact shadowing state of one request-completion overlay.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct RequestCompletion {
    /// The durable population this overlay refines.
    pub parent: ViewPopulationParent,
    /// Which overlay. Process-unique.
    pub overlay_id: OverlayId,
    /// Which state of that overlay. Advances only when the EFFECTIVE
    /// shadowing changes.
    pub revision: u64,
}

/// The population of the EFFECTIVE VALIDATING VIEW a scope's aggregates
/// are minted under.
///
/// Distinct from [`ResolutionPopulation`], which is the workspace
/// resolution world's own session-domain identity and has nothing to do
/// with overlay installation.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum ViewPopulation {
    /// No overlay: the base host view.
    Base,
    /// A session view, keyed by its installed overlay set.
    SessionOverlay(SessionOverlayFingerprint),
    /// A request-scoped completion overlay that is SHADOWING something, on
    /// top of a base or session parent.
    RequestCompletion(RequestCompletion),
}

impl ViewPopulation {
    /// Project the population of a view whose parent is refined by a
    /// request-completion overlay.
    ///
    /// `None` means "no population can be named", which disarms
    /// compaction for the reading scope.
    ///
    /// **An EMPTY completion overlay projects to its PARENT.** A SHADOWING
    /// overlay gets `(parent, overlay_id, revision)`.
    #[must_use]
    pub fn refined_by_completion(
        parent: ViewPopulationParent,
        state: CompletionOverlayState,
    ) -> Option<Self> {
        match state {
            CompletionOverlayState::Empty => Some(parent.into()),
            CompletionOverlayState::Shadowing {
                overlay_id,
                revision,
            } => Some(Self::RequestCompletion(RequestCompletion {
                parent,
                overlay_id,
                revision,
            })),
            // A writer is mid-update: the shadowing state has no readable
            // value, so neither the parent projection nor a revision is
            // honest. Naming no population is the only sound answer.
            CompletionOverlayState::InFlight => None,
        }
    }
}

/// The population identity a [`DomainGenerationFact`] speaks for.
///
/// Two disjoint spaces, deliberately not collapsed into one:
/// * [`Self::Resolution`] — [`CompactionDomain::Resolution`] only. Its
///   precise facts carry a population in their own keys.
/// * [`Self::View`] — every other domain. Their facts carry no population,
///   so it can only come from the view that validated them.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum AggregatePopulation {
    Resolution(ResolutionPopulation),
    View(ViewPopulation),
}

/// A whole domain's precise bucket, lifted to its terminal aggregate
/// because the bucket outgrew its threshold.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct DomainGenerationFact {
    pub domain: CompactionDomain,
    pub population: AggregatePopulation,
    pub stamp: AggregateStamp,
}

/// Content-free parse-environment identity carried by source-env facts.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct ParseEnvHash(FactHash16);

impl ParseEnvHash {
    #[must_use]
    pub const fn new(hash: FactHash16) -> Self {
        Self(hash)
    }

    #[must_use]
    pub const fn from_env_hash(hash: FactHash16) -> Self {
        Self::new(hash)
    }

    #[must_use]
    pub const fn get(self) -> FactHash16 {
        self.0
    }
}

/// Whether a derived fact's route participates in the module-augmentation
/// route surface, or is a direct-source (non-augmentation) derivation.
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
/// Semantic resolved-import facts and workspace resolution-currency facts
/// are alternatives under the same `FactVersionRef::ResolveImports`
/// discriminant; neither domain has a sibling witness or admission rail.
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

/// The exact function identity of a program-analysis `FlowBody` fact:
/// canonical + owner + merged name + space + function part + overload
/// ordinal. Content-free and env-free like every fact reference — the
/// slot env tail is a query-key dimension, never a validation input.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct ProgramAnalysisFunctionRef {
    pub canonical_id: Arc<str>,
    pub owner: verter_type_expr::TopLevelOwnerId,
    pub merged_symbol_name: Arc<str>,
    pub symbol_space: SymbolSpace,
    pub function_part: verter_type_expr::facts::FunctionPartIdentity,
    pub overload_ordinal: u32,
}

/// Program-analysis-domain fact reference. The `FlowBody` rail roots a
/// whole-function body demand on its exact function identity plus the
/// `flow_body_stable_hash` the producing read observed.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum ProgramAnalysisFactRef {
    /// One served function position's whole-body stable hash, observed
    /// from the per-file `FunctionProgramIndex`.
    FlowBody {
        /// The exact function identity.
        function: ProgramAnalysisFunctionRef,
        /// The observed whole-body stable hash.
        flow_body_stable_hash: FactHash16,
    },
}

/// Collision-free identity of the exact world in which a structural cache
/// entry's self-roots were strictly validated before their precise
/// `FileWholeHash` facts were collapsed.
///
/// The authority id prevents two host instances from aliasing; its
/// dedicated generation covers live trackedness inputs that do not live in
/// an immutable store root (`file_exists`/`derived_raw_cache` presence).
/// The two root epochs distinguish immutable scheduler/artifact
/// populations, and `population` distinguishes base, session, and
/// request-completion views.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct StrictSelfRootWorld {
    pub authority_id: u64,
    pub authority_generation: u64,
    pub source_epoch: u64,
    pub artifact_epoch: u64,
    pub population: ViewPopulation,
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
    ProgramAnalysis(ProgramAnalysisFactRef),
    FileSourceEnv {
        canonical_id: String,
        parse_env_hash: ParseEnvHash,
        parse_key: verter_language::ParseKey,
        file_language_id: verter_language::FileLanguage,
    },
    ProjectGeneration {
        generation: u64,
    },
    /// A whole domain's precise bucket, lifted to its terminal aggregate
    /// because the bucket outgrew its threshold. See
    /// [`DomainGenerationFact`].
    DomainGeneration(DomainGenerationFact),
    /// Terminal witness for a set of structural self-roots that was
    /// strictly validated in one exact effective view before publication.
    StrictSelfRootWorld(StrictSelfRootWorld),
}

/// How one fact attributes to canonical files.
///
/// [`FactVersionRef::canonical_id`] answers "which canonical, if any" and
/// collapses the two reasons a fact names none into a single `None`. They
/// are not the same reason and they do not license the same handling:
///
/// * [`Self::ProjectScalar`] names no canonical because it DESCRIBES
///   none — a whole-project counter is not a statement about any file.
/// * [`Self::DomainAggregate`] names no canonical because it STANDS IN FOR
///   the domain's precise facts across an unbounded set of them. Skipping
///   it does not make a per-canonical projection smaller, it makes it an
///   UNDER-APPROXIMATION.
/// * [`Self::StrictSelfRootWorld`] names no canonical because it certifies
///   a set of strictly validated structural roots in one exact effective
///   view. It is neither a project scalar nor a generic domain aggregate.
///
/// Any consumer that groups, filters, or projects a signature by canonical
/// branches on this rather than on the `Option`, so the aggregate case is
/// a named decision instead of whatever the `Option` happened to produce.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FactAttribution<'a> {
    /// Attributable to exactly this canonical file.
    Canonical(&'a str),
    /// A whole-project scalar ([`FactVersionRef::ProjectGeneration`]).
    ProjectScalar,
    /// A whole-domain terminal aggregate
    /// ([`FactVersionRef::DomainGeneration`]), standing in for every
    /// precise fact the observing scope read in that domain.
    DomainAggregate(CompactionDomain),
    /// A terminal structural self-root witness. It names no canonical and
    /// is deliberately distinct from a whole-domain aggregate.
    StrictSelfRootWorld,
}

impl FactVersionRef {
    #[must_use]
    pub fn attribution(&self) -> FactAttribution<'_> {
        match self {
            Self::FileWholeHash { canonical_id, .. }
            | Self::DerivedFactHash { canonical_id, .. }
            | Self::FileSourceEnv { canonical_id, .. } => FactAttribution::Canonical(canonical_id),
            Self::Parse(fact) => FactAttribution::Canonical(&fact.canonical_id),
            // The resolution sub-domain's project-scoped entries name no
            // canonical either, and for the same reason a `ProjectGeneration`
            // does: they describe a project, not a file.
            Self::ResolveImports(fact) => match fact.canonical_id() {
                Some(canonical_id) => FactAttribution::Canonical(canonical_id),
                None => FactAttribution::ProjectScalar,
            },
            Self::RouteSurface(fact) => FactAttribution::Canonical(&fact.canonical_id),
            Self::ProgramAnalysis(fact) => match fact {
                ProgramAnalysisFactRef::FlowBody { function, .. } => {
                    FactAttribution::Canonical(function.canonical_id.as_ref())
                }
            },
            Self::ProjectGeneration { .. } => FactAttribution::ProjectScalar,
            Self::DomainGeneration(aggregate) => FactAttribution::DomainAggregate(aggregate.domain),
            Self::StrictSelfRootWorld(_) => FactAttribution::StrictSelfRootWorld,
        }
    }

    /// The canonical this fact names, or `None` when it names none.
    ///
    /// Derived from [`Self::attribution`] so the two cannot drift. Use it
    /// only where "names no canonical" and "stands in for many" are
    /// genuinely interchangeable; where they are not, match on the
    /// attribution directly.
    #[must_use]
    pub fn canonical_id(&self) -> Option<&str> {
        match self.attribution() {
            FactAttribution::Canonical(canonical_id) => Some(canonical_id),
            // Reporting a canonical for either would make a compacted
            // signature claim a self-root it does not have.
            FactAttribution::ProjectScalar
            | FactAttribution::DomainAggregate(_)
            | FactAttribution::StrictSelfRootWorld => None,
        }
    }
}

#[cfg(test)]
#[path = "version_tests.rs"]
mod tests;
