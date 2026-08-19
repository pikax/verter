//! Dependency-neutral fact-signature carriers.
//!
//! These types are the single validity rail shared by workspace resolution and
//! the session cache runtime. Domain owners provide [`FactVersionValidator`]
//! implementations; cache entries always retain a [`ReadSetSignature`].

use std::sync::Arc;

use crate::fact_read_set::FactReadSetFinalise;
use crate::fact_registry::{FactKey, FactLane};
use crate::resolution_currency::{
    ResolutionFactKey, ResolutionFactRef, ResolutionPopulation, ResolutionWorldId,
};

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
    /// production paths that move these (`publish_snapshot` via
    /// `configure_projects`, and `WorkspaceChange::ConfigChanged`) do not
    /// bump the content generation, so folding source-env into the
    /// content domain would let an env change survive a content-compacted
    /// signature.
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
    /// Deliberately not a ledger counter. `ContextSelection` is versioned
    /// in a SEPARATE `context_versions` map that the fact ledger's
    /// mutators never touch (`CapturedResolutionWorld::fact_version`
    /// routes that key away from `ResolutionFactRoot::version`), so a
    /// counter advanced by ledger mutation alone is blind to a
    /// published-context replacement and would let a context change
    /// stale-serve. Root identity advances on base and session
    /// publication — exactly the boundary `replace_published` crosses —
    /// so it covers the whole domain, `ContextSelection` included.
    ResolutionRoots {
        base: ResolutionWorldId,
        session: Option<ResolutionWorldId>,
    },
    /// [`CompactionDomain::SemanticImports`]'s composite.
    ///
    /// A bare membership counter is UNSOUND for this domain. Its store
    /// answers per KEY — `(canonical, content_hash, parse_env_hash,
    /// resolve_env_hash, resolver_version)` — so the same membership
    /// generation describes a different answer once any key dimension
    /// moves. A witness pinning only the membership counter would keep
    /// validating across a content edit or an env republication that
    /// re-keys every slot it stands in for, because no admission
    /// happened. The composite therefore pins the domains those
    /// dimensions belong to alongside its own counter. See
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
///   contributors. Deliberately NOT `artifact_generation`, which also
///   moves for first-time index materialisation, same-fingerprint
///   self-heal and cache-only repopulation — none of which is a semantic
///   validity flip, and all of which are frequent INSIDE an active fact
///   tracer. Copying it would make the domain refuse its own consumers'
///   cold work.
/// * `content` — an augmenter's content moved, which moves its
///   `parse_stable_hash` and so the set fingerprint.
/// * `source_env` — the parse-env identity the augmenter artifacts are
///   keyed by moved.
/// * `workspace_shape` — the project graph moved, which re-composes the
///   `AugmentationTargetKey` the index is keyed by.
///
/// No RESOLUTION component: the augmentation index is keyed off artifact
/// identity and project env, and is never composed against the
/// resolved-import world.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct RouteSurfaceStamp {
    pub route_surface: u64,
    pub content: u64,
    pub source_env: u64,
    pub workspace_shape: u64,
}

/// The composite [`CompactionDomain::SemanticImports`] stamp.
///
/// Each component answers for one way a recorded semantic-import fact
/// can stop being current:
///
/// * `semantic_imports` — the store's own membership moved (an
///   admission, an eviction inside that admission, or a clear).
/// * `content` — a `content_hash` key dimension moved, so the slot the
///   consumer would read is no longer the slot it read.
/// * `source_env` — a `parse_env_hash` key dimension moved, likewise.
/// * `resolution` — the resolved-import world the `resolve_env_hash`
///   dimension and the producer's route witness are composed against
///   was republished.
/// * `workspace_shape` — the project graph moved, which re-composes the
///   per-canonical env bundle the producer keys on.
///
/// Over-coverage is deliberate where the domains overlap: a component
/// that is redundant for a given change is never wrong, while a missing
/// one is a stale serve.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct SemanticImportsStamp {
    pub semantic_imports: u64,
    pub content: u64,
    pub source_env: u64,
    pub resolution: ResolutionRootsStamp,
    pub workspace_shape: u64,
}

/// The captured resolution world's root identity, as a component of a
/// composite stamp.
///
/// The same quantity [`AggregateStamp::ResolutionRoots`] carries, named
/// separately so a composite can hold it by value. Root identity rather
/// than a ledger counter, for the reason recorded on that variant:
/// `ContextSelection` is versioned outside the fact ledger, so a counter
/// advanced by ledger mutation alone is blind to a published-context
/// replacement.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct ResolutionRootsStamp {
    pub base: ResolutionWorldId,
    pub session: Option<ResolutionWorldId>,
}

/// Identity of the overlay SET a session view has installed.
///
/// The same quantity `augmentation_population_for_view` keys its session
/// population on: it captures *which* overlays are installed, so two
/// sessions holding identical overlay sets share an identity and a session
/// whose overlays change gets a fresh one. A bare session id would let a
/// session presenting a base-only overlay set be cached as session-correct.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct SessionOverlayFingerprint(u64);

impl SessionOverlayFingerprint {
    /// `None` for the ZERO fingerprint, which is not a session identity:
    /// "no overlays installed" IS the base view. Admitting zero here would
    /// partition base entries from themselves — every base scope minting a
    /// `SessionOverlay(0)` aggregate no base read can satisfy — so the
    /// constructor refuses it rather than leaving the check to each caller.
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

/// The population of the EFFECTIVE VALIDATING VIEW a scope's aggregates
/// are minted under.
///
/// Distinct from [`ResolutionPopulation`], which is the workspace
/// resolution world's own session-domain identity and has nothing to do
/// with overlay installation. Conflating them would be a type lie: they
/// are separate identity spaces owned by separate producers.
///
/// A REQUEST-COMPLETION overlay has a variant here, and the reason it
/// once did not is worth stating: the objection was that such an overlay
/// is append-only *within* a request and therefore has no identity stable
/// for the life of a scope. The premise is half right. The overlay's KEY
/// SET only grows, but a key's VALUE is replaced — a retried request
/// re-completes the same canonical with a different whole hash — so
/// "append-only" was never the property that would have made a
/// key-derived identity stable, and a scope-stable identity was never
/// obtainable by counting keys.
///
/// What IS obtainable is an identity of the overlay's exact SHADOWING
/// STATE: a process-unique overlay id plus a revision that advances only
/// when the effective shadowing changes, under the same
/// serialized-mutation protocol every other compaction clock uses. Two
/// scopes reading the same overlay at the same revision validate
/// identically by construction, and a scope whose overlay moves under it
/// is not "unstable identity" — it is MOVEMENT, which the basis re-check
/// already reports. See [`Self::refined_by_completion`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum ViewPopulation {
    /// No overlay: the base host view.
    Base,
    /// A session view, keyed by its installed overlay set.
    SessionOverlay(SessionOverlayFingerprint),
    /// A request-scoped completion overlay that is SHADOWING something,
    /// on top of a base or session parent.
    RequestCompletion(RequestCompletion),
}

impl ViewPopulation {
    /// Project the population of a view whose parent is refined by a
    /// request-completion overlay.
    ///
    /// `None` means "no population can be named", which disarms
    /// compaction for the reading scope — the same fail-safe direction as
    /// an absent producer.
    ///
    /// **An EMPTY completion overlay projects to its PARENT.** An overlay
    /// that shadows nothing changes no validation answer: every fact the
    /// view validates, it validates exactly as the parent does. Giving it
    /// a distinct population would partition every empty request view
    /// from the durable base/session candidates AND from each other, so
    /// no aggregate could ever be reused across requests — the common
    /// case, and the one that has to reuse.
    ///
    /// A SHADOWING overlay gets `(parent, overlay_id, revision)`, so its
    /// aggregates are reusable within that exact overlay state and by
    /// nothing else. The parent travels inside the identity rather than
    /// being erased: two overlays with identical contents over DIFFERENT
    /// sessions are different populations, which is the same rule the
    /// session fingerprint already enforces one level up.
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
            // honest. Naming no population is the only sound answer —
            // projecting to the parent here would claim "shadows nothing"
            // while a shadow is being installed.
            CompletionOverlayState::InFlight => None,
        }
    }
}

/// The population a request-completion overlay REFINES.
///
/// Deliberately not `ViewPopulation` itself: a completion overlay's parent
/// is always a durable population, never another completion overlay, and
/// making that unrepresentable keeps the identity one level deep instead
/// of an arbitrarily nested chain nothing constructs.
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

/// The exact shadowing state of one request-completion overlay.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct RequestCompletion {
    /// The durable population this overlay refines.
    pub parent: ViewPopulationParent,
    /// Which overlay. Process-unique, so two concurrent requests that
    /// happen to reach the same revision number are still distinct.
    pub overlay_id: OverlayId,
    /// Which state of that overlay. Advances only when the EFFECTIVE
    /// shadowing changes, so a write that replaces a value with an equal
    /// one does not fork the population.
    pub revision: u64,
}

/// A completion overlay's shadowing state as its holder can currently
/// report it.
///
/// Three answers, not two: "shadows nothing", "shadows this exact state",
/// and "a writer holds the state open". The third is not a variation of
/// the first — see [`ViewPopulation::refined_by_completion`].
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

/// Process-unique identity of one request-completion overlay.
///
/// Minted at overlay construction and never reused within the process, so
/// an aggregate minted under one request's overlay can never be satisfied
/// by another request's overlay that reached the same revision. Not
/// content-derived: two overlays holding identical entries are still
/// different populations, because they are different requests' append
/// windows and either may grow next.
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

/// The population identity a [`DomainGenerationFact`] speaks for.
///
/// Two disjoint spaces, deliberately not collapsed into one:
///
/// * [`Self::Resolution`] — [`CompactionDomain::Resolution`] only. Its
///   precise facts carry a population in their own keys, so its buckets
///   partition themselves and its authority is the captured resolution
///   world.
/// * [`Self::View`] — every other domain. Their facts carry no population,
///   so it can only come from the view that validated them.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum AggregatePopulation {
    Resolution(ResolutionPopulation),
    View(ViewPopulation),
}

/// Live stamp of each compaction domain, as read by whoever installs
/// a fact tracer.
///
/// Every field is `Option`: a domain with NO live generation source does
/// not compact and stays precise. That is the fail-safe direction — an
/// aggregate is never minted without a producer that can advance it, so a
/// compacted bucket can never become a permanently-valid stale witness.
///
/// Three domains are populated by the workspace engine, which owns their
/// counters. The remaining three are owned by the session's
/// `ProjectTypeStore` and are supplied by the session-side tracer
/// installer; a workspace-only tracer leaves them `None` and their buckets
/// stay precise.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct AggregateGenerations {
    pub content: Option<AggregateStamp>,
    pub source_env: Option<AggregateStamp>,
    pub semantic_imports: Option<AggregateStamp>,
    pub resolution: Option<AggregateStamp>,
    pub route_surface: Option<AggregateStamp>,
    pub workspace_shape: Option<AggregateStamp>,
    /// The population of the EFFECTIVE VALIDATING VIEW this scope
    /// compacts against.
    ///
    /// Required — not merely informative — for the four view-derived
    /// domains (`Content`, `SourceEnv`, `SemanticImports`,
    /// `RouteSurface`). Their facts carry no population of their own, so
    /// an aggregate minted without one would claim "the whole domain
    /// held" without saying *for whom*, and a session-overlay-derived
    /// signature would then satisfy a base read. A domain with a live
    /// stamp but no view population therefore does NOT compact; its
    /// bucket stays precise. That is the same fail-safe direction as an
    /// absent stamp, for the same reason.
    ///
    /// `Resolution` never reads this (its population is in its keys) and
    /// `WorkspaceShape` never reads it either (a project generation is a
    /// whole-host scalar no overlay shadows).
    pub view_population: Option<ViewPopulation>,
}

impl AggregateGenerations {
    /// The stamp that stands in for a whole domain's precise bucket, or
    /// `None` when the domain has no live producer in this scope.
    ///
    /// Exhaustive by domain — a new [`CompactionDomain`] cannot compile
    /// without declaring where its stamp comes from. It is the ONE
    /// mapping, used by producer and validator alike, so the two cannot
    /// disagree about what a domain's aggregate means.
    #[must_use]
    pub fn stamp_for(&self, domain: CompactionDomain) -> Option<AggregateStamp> {
        match domain {
            CompactionDomain::Content => self.content,
            CompactionDomain::SourceEnv => self.source_env,
            CompactionDomain::SemanticImports => self.semantic_imports,
            CompactionDomain::Resolution => self.resolution,
            CompactionDomain::RouteSurface => self.route_surface,
            CompactionDomain::WorkspaceShape => self.workspace_shape,
        }
    }

    /// Whether a scope holding this basis could MINT `domain`'s terminal
    /// aggregate.
    ///
    /// The SAME two conditions [`compact_domains`](crate::fact_read_set)
    /// applies — a live stamp AND a population for the bucket — read
    /// without a fact in hand. Sharing one predicate is the point: a
    /// domain this basis cannot mint for is one whose precise facts stay
    /// precise, so nothing this scope produces can ever claim that domain
    /// held, and its generation moving cannot make any witness wrong.
    ///
    /// Where the population comes from is domain-specific and not
    /// interchangeable:
    ///
    /// * [`CompactionDomain::Resolution`] carries a population in its
    ///   precise facts' own keys, so a stamp alone is enough;
    /// * [`CompactionDomain::WorkspaceShape`] is a whole-host scalar no
    ///   overlay shadows, so its aggregate is base-scoped unconditionally;
    /// * the remaining four have no population of their own and mint
    ///   nothing until the validating view supplies one.
    #[must_use]
    pub fn can_mint(&self, domain: CompactionDomain) -> bool {
        if self.stamp_for(domain).is_none() {
            return false;
        }
        match domain {
            CompactionDomain::Resolution | CompactionDomain::WorkspaceShape => true,
            CompactionDomain::Content
            | CompactionDomain::SourceEnv
            | CompactionDomain::SemanticImports
            | CompactionDomain::RouteSurface => self.view_population.is_some(),
        }
    }

    /// Every domain a scope holding this basis could compact.
    const DOMAINS: [CompactionDomain; 6] = [
        CompactionDomain::Content,
        CompactionDomain::SourceEnv,
        CompactionDomain::SemanticImports,
        CompactionDomain::Resolution,
        CompactionDomain::RouteSurface,
        CompactionDomain::WorkspaceShape,
    ];

    /// `true` when this basis could compact at least one domain.
    ///
    /// The short-circuit for movement detection: a scope that can mint
    /// nothing produces no aggregate, so no generation movement can
    /// corrupt it and there is nothing to re-read.
    ///
    /// Deliberately "could MINT", not "has a stamp". A basis routinely
    /// carries a stamp for a domain it cannot mint — every scope seeded
    /// from a view before a population reaches it carries four. Treating
    /// those as compactable would destabilise a scope for a claim it
    /// never made, which costs the admission of essentially every cold
    /// compute that resolves an import while buying no soundness at all.
    #[must_use]
    pub fn names_any_domain(&self) -> bool {
        Self::DOMAINS.iter().any(|domain| self.can_mint(*domain))
    }

    /// `true` when any domain this basis could COMPACT has a different
    /// stamp in `live`.
    ///
    /// The movement predicate, and the mirror of [`Self::can_mint`]: only
    /// a domain this scope could mint an aggregate for is examined,
    /// because only such a domain can be claimed to have "held" across a
    /// generation the observations do not come from. A domain present
    /// here but absent from `live` counts as MOVED — a stamp that can no
    /// longer be read is not a stamp that can be vouched for.
    ///
    /// `view_population` is not compared: it identifies the validating
    /// VIEW, which is fixed for the life of a scope and is not something
    /// a producer advances. It participates only through `can_mint`,
    /// which is where it belongs.
    #[must_use]
    pub fn any_named_domain_moved(&self, live: &Self) -> bool {
        Self::DOMAINS.iter().any(|domain| {
            self.can_mint(*domain) && self.stamp_for(*domain) != live.stamp_for(*domain)
        })
    }

    /// Compose a basis from an ALREADY-BOUND view's captured seed plus the
    /// host's live O(1) counters.
    ///
    /// The whole reason a basis is split into these two halves: composing
    /// it needs data only a store view holds (the two composite stamps'
    /// key dimensions, and the resolution-root identity inside the
    /// semantic-imports composite), but READING a store view is an
    /// `O(store-view read)` operation that a per-tracer-scope caller must
    /// not perform — not at installation and, far hotter, not at every
    /// admission boundary's movement re-check. Capturing the view's
    /// contribution ONCE as an [`AggregateBasisSeed`] and re-composing it
    /// against live counters makes both ends `O(1)`.
    ///
    /// Which component comes from which half is not interchangeable:
    ///
    /// * participating SCALAR domains (`Content`, `SourceEnv`) and the
    ///   global `WorkspaceShape` domain take their stamp LIVE — they are
    ///   single-producer counters, and movement detection exists precisely
    ///   to notice them advancing mid-scope;
    /// * the two COMPOSITE domains keep every key dimension the seeding
    ///   view captured and take only the domain's OWN clock live. Their
    ///   clocks are deliberately not store-view token dimensions, so a
    ///   captured clock would report the generation the view was built at
    ///   and an admission landing mid-scope would go unobserved — which is
    ///   the gap movement detection exists to close. The other components
    ///   must stay CAPTURED, because an aggregate is validated against the
    ///   view that reads it and a component composed from a different
    ///   source than the validator's would disagree on a value neither
    ///   side moved.
    ///
    /// A domain whose live clock is in flight (`None`) disarms — the same
    /// fail-safe direction as an absent producer.
    #[must_use]
    pub fn from_seed(seed: &AggregateBasisSeed, live: &LiveAggregateCounters) -> Self {
        let AggregateBasisSeed::Vouched {
            view_population,
            view_domains,
            semantic_imports,
            route_surface,
        } = seed
        else {
            // No bound, current view vouches for this scope: it names no
            // domain, compacts nothing, and re-reading live counters
            // could tell it nothing.
            return Self::default();
        };
        Self {
            content: view_domains
                .contains(CompactionDomain::Content)
                .then_some(AggregateStamp::Generation(live.content)),
            source_env: view_domains
                .contains(CompactionDomain::SourceEnv)
                .then_some(())
                .and_then(|()| live.source_env.map(AggregateStamp::Generation)),
            semantic_imports: view_domains
                .contains(CompactionDomain::SemanticImports)
                .then_some(())
                .and_then(|()| {
                    semantic_imports_with_live_clock(*semantic_imports, live.semantic_imports)
                }),
            // Supplied by the resolution transaction that owns the
            // captured world, not by a view seed.
            resolution: None,
            route_surface: view_domains
                .contains(CompactionDomain::RouteSurface)
                .then_some(())
                .and_then(|()| route_surface_with_live_clock(*route_surface, live.route_surface)),
            workspace_shape: Some(AggregateStamp::Generation(live.workspace_shape)),
            view_population: *view_population,
        }
    }
}

/// [`AggregateStamp::SemanticImports`] with the domain's own membership
/// counter taken live and every other component left as the seeding view
/// captured it.
fn semantic_imports_with_live_clock(
    captured: Option<AggregateStamp>,
    live: Option<u64>,
) -> Option<AggregateStamp> {
    let AggregateStamp::SemanticImports(captured) = captured? else {
        // Unreachable by construction — the seed carries this slot only
        // from `semantic_imports_stamp`, which answers with its own
        // variant or not at all. A refusal rather than a panic so a
        // future reshaping degrades to "this domain does not compact".
        return None;
    };
    Some(AggregateStamp::SemanticImports(SemanticImportsStamp {
        semantic_imports: live?,
        ..captured
    }))
}

/// [`AggregateStamp::RouteSurface`] with the augmentation-world clock
/// taken live and every other component left as the seeding view captured
/// it.
fn route_surface_with_live_clock(
    captured: Option<AggregateStamp>,
    live: Option<u64>,
) -> Option<AggregateStamp> {
    let AggregateStamp::RouteSurface(captured) = captured? else {
        return None;
    };
    Some(AggregateStamp::RouteSurface(RouteSurfaceStamp {
        route_surface: live?,
        ..captured
    }))
}

/// The half of a compaction basis that only an ALREADY-BOUND store view
/// can supply, captured ONCE at tracer installation.
///
/// Fail-safe by construction: the [`Default`] and the constructor for a
/// scope with no bound current view are the same [`Self::Unvouched`] case,
/// which names no domain. A `StoreView` implementation that does not
/// override its projection therefore disables compaction and movement
/// detection for the scopes it seeds, rather than vouching for stamps it
/// cannot answer for.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum AggregateBasisSeed {
    /// No bound, CURRENT view stands behind this scope. It names no
    /// domain: nothing compacts and no movement can corrupt it.
    #[default]
    Unvouched,
    /// A current, bound view's captured contribution. Each slot is
    /// `None` when that composite has a component the view cannot answer
    /// for — a witness pinning fewer dimensions than its store keys on is
    /// a stale serve, so a missing component disarms the domain rather
    /// than substituting a constant.
    Vouched {
        /// Effective validation population captured from the bound view.
        /// `None` disarms the view-derived domains while leaving global
        /// domains available.
        view_population: Option<ViewPopulation>,
        /// Which view-derived domain stamps this scope is permitted to
        /// compose. This keeps domain participation explicit instead of
        /// letting the presence of a population arm unrelated clocks.
        view_domains: ViewAggregateDomains,
        /// [`AggregateStamp::SemanticImports`] as the view captured it,
        /// including the resolution-root identity component.
        semantic_imports: Option<AggregateStamp>,
        /// [`AggregateStamp::RouteSurface`] as the view captured it.
        route_surface: Option<AggregateStamp>,
    },
}

/// Explicit participation set for the four view-derived compaction
/// domains. A population identifies a view; it does not by itself decide
/// which domain clocks are safe to observe at a given producer boundary.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct ViewAggregateDomains(u8);

impl ViewAggregateDomains {
    const CONTENT_BIT: u8 = 1 << 0;
    const SOURCE_ENV_BIT: u8 = 1 << 1;
    const SEMANTIC_IMPORTS_BIT: u8 = 1 << 2;
    const ROUTE_SURFACE_BIT: u8 = 1 << 3;

    pub const CONTENT: Self = Self(Self::CONTENT_BIT);
    pub const CONTENT_SOURCE_ENV: Self = Self(Self::CONTENT_BIT | Self::SOURCE_ENV_BIT);
    pub const ALL: Self = Self(
        Self::CONTENT_BIT
            | Self::SOURCE_ENV_BIT
            | Self::SEMANTIC_IMPORTS_BIT
            | Self::ROUTE_SURFACE_BIT,
    );

    #[must_use]
    fn contains(self, domain: CompactionDomain) -> bool {
        let bit = match domain {
            CompactionDomain::Content => Self::CONTENT_BIT,
            CompactionDomain::SourceEnv => Self::SOURCE_ENV_BIT,
            CompactionDomain::SemanticImports => Self::SEMANTIC_IMPORTS_BIT,
            CompactionDomain::RouteSurface => Self::ROUTE_SURFACE_BIT,
            CompactionDomain::Resolution | CompactionDomain::WorkspaceShape => return false,
        };
        self.0 & bit != 0
    }
}

/// The live, `O(1)` half of a compaction basis: one atomic load per
/// domain clock, no store-view read.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LiveAggregateCounters {
    pub content: u64,
    pub source_env: Option<u64>,
    pub workspace_shape: u64,
    /// The semantic-import store's stable membership generation, `None`
    /// while an admission is in flight.
    pub semantic_imports: Option<u64>,
    /// The augmentation world's stable clock, `None` while a mutation is
    /// in flight.
    pub route_surface: Option<u64>,
}

/// One domain's terminal aggregate: "every precise fact this scope
/// observed in `domain`, under `population`, held as of `generation`".
///
/// `population` is what stops one population's aggregate from validating
/// against another's on a numeric coincidence, and it is DOMAIN-SPECIFIC:
///
/// * [`CompactionDomain::Resolution`] partitions from the precise bucket —
///   its keys carry a population and its base and session roots are
///   independent.
/// * [`CompactionDomain::WorkspaceShape`] is global to the host.
/// * The remaining four derive from the EFFECTIVE validating view,
///   including session-overlay and request-completion-overlay identity. A
///   session overlay re-roots whole hashes and parse facts while leaving
///   the workspace content generation untouched, so assuming `Base` for
///   them would let an overlay-derived signature validate as base. They
///   therefore mint no aggregate until a view-derived population is
///   supplied alongside their stamp — see
///   [`AggregateGenerations::view_population`] — and until then their
///   buckets simply stay precise.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct DomainGenerationFact {
    pub domain: CompactionDomain,
    pub population: AggregatePopulation,
    pub stamp: AggregateStamp,
}

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

/// The exact function identity of a program-analysis `FlowBody` fact:
/// canonical + owner + merged name + space + function part + overload
/// ordinal. Content-free and env-free like every fact reference — the
/// slot env tail is a query-key dimension, never a validation input.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct ProgramAnalysisFunctionRef {
    pub canonical_id: std::sync::Arc<str>,
    pub owner: verter_type_expr::TopLevelOwnerId,
    pub merged_symbol_name: std::sync::Arc<str>,
    pub symbol_space: crate::fact_registry::SymbolSpace,
    pub function_part: verter_type_expr::facts::FunctionPartIdentity,
    pub overload_ordinal: u32,
}

/// Program-analysis-domain fact reference. The `FlowBody` rail roots a
/// whole-function body demand on its exact function identity plus the
/// `flow_body_stable_hash` the producing read observed — content-free
/// view-agnostic like every `FactVersionRef` (the overlay/base
/// distinction lives on the validation side).
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
/// The authority id prevents two workspace instances from aliasing; its
/// dedicated generation covers live trackedness inputs that do not live in an
/// immutable store root (`file_exists` and `derived_raw_cache` presence). The
/// two root epochs distinguish immutable scheduler/artifact populations, and
/// `population` distinguishes base, session, and request-completion views.
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
    /// Terminal witness for a set of structural self-roots that was strictly
    /// validated in one exact effective view before publication.
    StrictSelfRootWorld(StrictSelfRootWorld),
}

/// How one fact attributes to canonical files.
///
/// [`FactVersionRef::canonical_id`] answers "which canonical, if any"
/// and collapses the two reasons a fact names none into a single
/// `None`. They are not the same reason and they do not license the
/// same handling:
///
/// * [`Self::ProjectScalar`] names no canonical because it DESCRIBES
///   none — a whole-project counter is not a statement about any file.
/// * [`Self::DomainAggregate`] names no canonical because it STANDS IN
///   FOR the domain's precise facts across an unbounded set of them.
///   Skipping it does not make a per-canonical projection smaller, it
///   makes it an UNDER-APPROXIMATION.
/// * [`Self::StrictSelfRootWorld`] names no canonical because it certifies a
///   set of strictly validated structural roots in one exact effective view.
///   It is neither a project scalar nor a generic domain aggregate.
///
/// Any consumer that groups, filters, or projects a signature by
/// canonical branches on this rather than on the `Option`, so the
/// aggregate case is a named decision instead of whatever the `Option`
/// happened to produce.
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
    /// A terminal structural self-root witness. It names no canonical and is
    /// deliberately distinct from a whole-domain aggregate.
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
            // The resolution sub-domain's project-scoped entries
            // (`ResolutionEntry::ExplicitProject`) name no canonical
            // either, and for the same reason a `ProjectGeneration`
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
    /// Derived from [`Self::attribution`] so the two cannot drift. Use
    /// it only where "names no canonical" and "stands in for many" are
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

    /// The compaction domains whose precise per-canonical facts this
    /// signature carries as a terminal aggregate, in first-appearance
    /// order.
    ///
    /// Non-empty means [`Self::canonical_ids`] — and every projection
    /// derived from it — is a STRICT UNDER-APPROXIMATION of this
    /// signature's dependency set, not merely a smaller one: the
    /// aggregate stands in for facts on an unbounded set of canonicals
    /// and names none of them.
    ///
    /// Two consumer classes, and the distinction is load-bearing:
    ///
    /// * A consumer that GROUPS or EVICTS by canonical (reverse-index
    ///   registration, per-canonical drain) may use the narrower
    ///   projection. It registers under fewer canonicals, so eager
    ///   eviction covers less — but the aggregate rejects on ANY
    ///   movement in its domain and read-side validation stays
    ///   authoritative, so the outcome is a lost eviction, never a
    ///   stale serve.
    /// * A consumer whose CORRECTNESS needs coverage — anything that
    ///   treats the projection as "the complete set this signature
    ///   depends on" — must consult this first and fail closed. For
    ///   that class an empty projection is indistinguishable from a
    ///   complete one, which is exactly the silent-wrong-answer shape.
    #[must_use]
    pub fn aggregated_domains(&self) -> Vec<CompactionDomain> {
        let mut out: Vec<CompactionDomain> = Vec::new();
        for fact in self.facts.iter() {
            if let FactAttribution::DomainAggregate(domain) = fact.attribution() {
                if !out.contains(&domain) {
                    out.push(domain);
                }
            }
        }
        out
    }

    /// Whether this signature carries `domain` as a terminal aggregate
    /// rather than as precise per-canonical facts.
    ///
    /// The single-domain form of [`Self::aggregated_domains`], for a
    /// consumer that only projects one domain's canonicals.
    #[must_use]
    pub fn aggregates_domain(&self, domain: CompactionDomain) -> bool {
        self.facts.iter().any(|fact| {
            matches!(
                fact.attribution(),
                FactAttribution::DomainAggregate(aggregated) if aggregated == domain
            )
        })
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

    /// Whether this witness carries resolution evidence but cannot enumerate
    /// any path observation a live evidence source can re-read.
    ///
    /// A terminal Resolution aggregate is necessarily un-enumerable. The
    /// converse is deliberately not assumed: a derived resolution witness can
    /// also name no re-observable path fact. Consumers that need a complete
    /// re-observation plan must ask this property rather than using aggregation
    /// as a proxy for it.
    #[must_use]
    pub fn resolution_evidence_is_unenumerable(&self) -> bool {
        let mut carries_resolution_evidence = false;
        let mut carries_reobservable_path = false;
        for fact in self.facts.iter() {
            if matches!(
                fact.attribution(),
                FactAttribution::DomainAggregate(CompactionDomain::Resolution)
            ) {
                return true;
            }
            let FactVersionRef::ResolveImports(fact) = fact else {
                continue;
            };
            let Some(fact) = fact.resolution_fact() else {
                continue;
            };
            carries_resolution_evidence = true;
            if fact.key.reobservable_path_canonical_id().is_some() {
                carries_reobservable_path = true;
            }
        }
        carries_resolution_evidence && !carries_reobservable_path
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
            FactReadSetFinalise::MutationUnstable => {
                Self::NonCacheable(verter_audit::NonAdmissionReason::MutationUnstable)
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

#[cfg(test)]
mod compaction_domain_tests {
    use super::*;
    use crate::fact_registry::{FactKey, SymbolSpace};
    use crate::resolution_currency::{
        CanonicalResolutionId, ResolutionFactKey, ResolutionFactVersion, ResolutionWorldId,
    };

    fn ts_language() -> verter_language::FileLanguage {
        verter_language::LanguageRegistry::global()
            .classify_static("/p/a.ts")
            .static_resolution()
    }

    fn test_parse_key(marker: u8) -> verter_language::ParseKey {
        verter_language::default_parse_identity_for(
            &format!("/* source-env test {marker} */"),
            &ts_language(),
        )
        .unwrap()
        .1
    }

    fn parse_fact() -> FactVersionRef {
        FactVersionRef::Parse(ParseFactRef {
            canonical_id: "/p/a.ts".to_string(),
            key: FactKey::Export {
                name: "Foo".into(),
                space: SymbolSpace::Type,
            },
            lane: FactLane::Semantic,
            expected_hash: [1; 16],
        })
    }

    fn semantic_import_fact() -> FactVersionRef {
        FactVersionRef::ResolveImports(ResolveImportsFactRef::Semantic {
            canonical_id: "/p/a.ts".to_string(),
            key: FactKey::Export {
                name: "Foo".into(),
                space: SymbolSpace::Type,
            },
            lane: FactLane::Semantic,
            expected_hash: [2; 16],
        })
    }

    fn resolution_fact() -> FactVersionRef {
        FactVersionRef::ResolveImports(ResolveImportsFactRef::Resolution(
            crate::resolution_currency::ResolutionFactRef {
                key: ResolutionFactKey::PathProbe {
                    canonical: CanonicalResolutionId::new("/p/a.ts"),
                    population: ResolutionPopulation::Base,
                },
                version: ResolutionFactVersion::INITIAL,
            },
        ))
    }

    /// Plan Block 1A TDD step 4: every current leaf variant maps to its
    /// domain, and the mapping is asserted DIRECTLY rather than inferred
    /// from whether some signature happened to compact.
    ///
    /// The `FileSourceEnv -> SourceEnv` row is the load-bearing one and
    /// is asserted separately from `Content` on purpose. Plan B2's own
    /// terminal-domain table folds `FileSourceEnv` into the content
    /// domain; the errata's MANDATORY correction splits it, because the
    /// two production paths that move `parse_env_hash` /
    /// `parse_key` / `file_language_id` (`publish_snapshot` /
    /// `rebuild_and_publish`, and `WorkspaceChange::ConfigChanged`)
    /// deliberately leave `content_generation` alone. Without this
    /// assertion, reverting to the plan's table is invisible: no
    /// non-resolution domain has a basis yet, so nothing compacts and
    /// every other test stays green — while the misclassification
    /// becomes a live stale-serve the moment those domains are armed.
    ///
    /// Mutation recipe: fold the `FileSourceEnv` arm of
    /// `compaction_domain` back into the `Content` arm (plan B2's
    /// table). The `FileSourceEnv` assertion fails immediately.
    #[test]
    fn every_leaf_variant_maps_to_its_compaction_domain() {
        let rows: Vec<(FactVersionRef, CompactionDomain)> = vec![
            (
                FactVersionRef::FileWholeHash {
                    canonical_id: "/p/a.ts".to_string(),
                    hash: [0; 16],
                },
                CompactionDomain::Content,
            ),
            (
                FactVersionRef::DerivedFactHash {
                    canonical_id: "/p/a.ts".to_string(),
                    kind: DerivedFactKind::Route,
                    hash: [0; 16],
                },
                CompactionDomain::Content,
            ),
            (parse_fact(), CompactionDomain::Content),
            (semantic_import_fact(), CompactionDomain::SemanticImports),
            (resolution_fact(), CompactionDomain::Resolution),
            (
                FactVersionRef::RouteSurface(RouteSurfaceFactRef {
                    canonical_id: "/p/a.ts".to_string(),
                    key: FactKey::Export {
                        name: "Foo".into(),
                        space: SymbolSpace::Type,
                    },
                    lane: FactLane::Semantic,
                    expected_hash: [3; 16],
                }),
                CompactionDomain::RouteSurface,
            ),
            (
                FactVersionRef::FileSourceEnv {
                    canonical_id: "/p/a.ts".to_string(),
                    parse_env_hash: ParseEnvHash::from_env_hash([4; 16]),
                    parse_key: test_parse_key(1),
                    file_language_id: ts_language(),
                },
                CompactionDomain::SourceEnv,
            ),
            (
                FactVersionRef::ProjectGeneration { generation: 7 },
                CompactionDomain::WorkspaceShape,
            ),
        ];

        for (fact, expected) in &rows {
            assert_eq!(
                compaction_domain(fact),
                *expected,
                "{fact:?} must classify as {expected:?}"
            );
        }

        assert_ne!(
            compaction_domain(&FactVersionRef::FileSourceEnv {
                canonical_id: "/p/a.ts".to_string(),
                parse_env_hash: ParseEnvHash::from_env_hash([4; 16]),
                parse_key: test_parse_key(1),
                file_language_id: ts_language(),
            }),
            CompactionDomain::Content,
            "SOURCE-ENV IS NOT CONTENT. `publish_snapshot` / `rebuild_and_publish` and \
             `WorkspaceChange::ConfigChanged` move parse_env_hash / parse_key / \
             file_language_id WITHOUT bumping content_generation, so a source-env fact \
             compacted into the content domain would survive an env change"
        );

        // Every domain the taxonomy declares is reachable from a real
        // leaf, so no arm of `compaction_domain` is dead.
        let covered: std::collections::BTreeSet<CompactionDomain> =
            rows.iter().map(|(_, domain)| *domain).collect();
        assert_eq!(
            covered.len(),
            6,
            "every CompactionDomain must be produced by at least one leaf variant; \
             covered {covered:?}"
        );
    }

    /// An aggregate is already its own domain's terminal form, so it
    /// classifies back into the domain it stands for. This is what makes
    /// the no-regrow rule work: a reused signature's aggregate lands in
    /// the same bucket as the precise facts it must absorb.
    #[test]
    fn an_aggregate_classifies_into_the_domain_it_stands_for() {
        for domain in [
            CompactionDomain::Content,
            CompactionDomain::SourceEnv,
            CompactionDomain::SemanticImports,
            CompactionDomain::Resolution,
            CompactionDomain::RouteSurface,
            CompactionDomain::WorkspaceShape,
        ] {
            let fact = FactVersionRef::DomainGeneration(DomainGenerationFact {
                domain,
                population: AggregatePopulation::Resolution(ResolutionPopulation::Base),
                stamp: AggregateStamp::ResolutionRoots {
                    base: ResolutionWorldId::fresh(1),
                    session: None,
                },
            });
            assert_eq!(compaction_domain(&fact), domain);
        }
    }
}

#[cfg(test)]
mod aggregate_basis_seed_tests {
    use super::*;
    use crate::resolution_currency::ResolutionWorldId;

    /// Captured components deliberately differ from every live counter,
    /// so a composition that sourced a component from the wrong half is
    /// visible as a value mismatch rather than a coincidence.
    const CAPTURED_CONTENT: u64 = 700;
    const CAPTURED_SOURCE_ENV: u64 = 701;
    const CAPTURED_WORKSPACE_SHAPE: u64 = 702;
    const CAPTURED_SEMANTIC_IMPORTS: u64 = 703;
    const CAPTURED_ROUTE_SURFACE: u64 = 704;

    fn captured_semantic_imports() -> AggregateStamp {
        AggregateStamp::SemanticImports(SemanticImportsStamp {
            semantic_imports: CAPTURED_SEMANTIC_IMPORTS,
            content: CAPTURED_CONTENT,
            source_env: CAPTURED_SOURCE_ENV,
            resolution: ResolutionRootsStamp {
                base: ResolutionWorldId::fresh(9),
                session: None,
            },
            workspace_shape: CAPTURED_WORKSPACE_SHAPE,
        })
    }

    fn captured_route_surface() -> AggregateStamp {
        AggregateStamp::RouteSurface(RouteSurfaceStamp {
            route_surface: CAPTURED_ROUTE_SURFACE,
            content: CAPTURED_CONTENT,
            source_env: CAPTURED_SOURCE_ENV,
            workspace_shape: CAPTURED_WORKSPACE_SHAPE,
        })
    }

    fn live() -> LiveAggregateCounters {
        LiveAggregateCounters {
            content: 11,
            source_env: Some(12),
            workspace_shape: 13,
            semantic_imports: Some(14),
            route_surface: Some(15),
        }
    }

    fn vouched() -> AggregateBasisSeed {
        AggregateBasisSeed::Vouched {
            view_population: Some(ViewPopulation::Base),
            view_domains: ViewAggregateDomains::ALL,
            semantic_imports: Some(captured_semantic_imports()),
            route_surface: Some(captured_route_surface()),
        }
    }

    /// A scope with no bound current view compacts nothing, so it must
    /// name nothing — even though every live counter is readable. A
    /// composition that ignored the seed would arm the three scalar
    /// domains here.
    #[test]
    fn an_unvouched_seed_names_no_domain_even_with_readable_live_counters() {
        let basis = AggregateGenerations::from_seed(&AggregateBasisSeed::Unvouched, &live());
        assert!(
            !basis.names_any_domain(),
            "an unvouched seed must name no domain; got {basis:?}"
        );
        assert_eq!(basis, AggregateGenerations::default());
    }

    /// The three single-producer domains take their stamp LIVE. Pinning
    /// the exact live values (which differ from every captured component)
    /// is what makes this discriminate a captured-scalar composition.
    #[test]
    fn scalar_domains_take_the_live_counter_not_a_captured_component() {
        let basis = AggregateGenerations::from_seed(&vouched(), &live());
        assert_eq!(basis.content, Some(AggregateStamp::Generation(11)));
        assert_eq!(basis.source_env, Some(AggregateStamp::Generation(12)));
        assert_eq!(basis.workspace_shape, Some(AggregateStamp::Generation(13)));
    }

    /// The semantic-imports composite substitutes ONLY its own membership
    /// counter. Every other component stays exactly as the seeding view
    /// captured it, including the resolution-root identity — a component
    /// recomposed from a live source would disagree with the validator's
    /// view on a value neither side moved.
    #[test]
    fn the_semantic_imports_composite_substitutes_only_its_own_clock() {
        let basis = AggregateGenerations::from_seed(&vouched(), &live());
        let Some(AggregateStamp::SemanticImports(stamp)) = basis.semantic_imports else {
            panic!(
                "expected a semantic-imports composite; got {:?}",
                basis.semantic_imports
            );
        };
        assert_eq!(stamp.semantic_imports, 14, "own clock must be LIVE");
        assert_eq!(
            stamp.content, CAPTURED_CONTENT,
            "content must stay CAPTURED"
        );
        assert_eq!(
            stamp.source_env, CAPTURED_SOURCE_ENV,
            "source_env must stay CAPTURED"
        );
        assert_eq!(
            stamp.workspace_shape, CAPTURED_WORKSPACE_SHAPE,
            "workspace_shape must stay CAPTURED"
        );
        assert_eq!(
            stamp.resolution,
            ResolutionRootsStamp {
                base: ResolutionWorldId::fresh(9),
                session: None,
            },
            "the resolution-root identity must stay CAPTURED"
        );
    }

    /// The same rule for the route-surface composite.
    #[test]
    fn the_route_surface_composite_substitutes_only_its_own_clock() {
        let basis = AggregateGenerations::from_seed(&vouched(), &live());
        let Some(AggregateStamp::RouteSurface(stamp)) = basis.route_surface else {
            panic!(
                "expected a route-surface composite; got {:?}",
                basis.route_surface
            );
        };
        assert_eq!(stamp.route_surface, 15, "own clock must be LIVE");
        assert_eq!(
            stamp.content, CAPTURED_CONTENT,
            "content must stay CAPTURED"
        );
        assert_eq!(
            stamp.source_env, CAPTURED_SOURCE_ENV,
            "source_env must stay CAPTURED"
        );
        assert_eq!(
            stamp.workspace_shape, CAPTURED_WORKSPACE_SHAPE,
            "workspace_shape must stay CAPTURED"
        );
    }

    /// A composite the seeding view could not answer for stays absent —
    /// a live clock alone is not a stamp, because the clock says nothing
    /// about the key dimensions the store answers by.
    #[test]
    fn a_composite_the_view_could_not_answer_stays_absent() {
        let seed = AggregateBasisSeed::Vouched {
            view_population: Some(ViewPopulation::Base),
            view_domains: ViewAggregateDomains::ALL,
            semantic_imports: None,
            route_surface: None,
        };
        let basis = AggregateGenerations::from_seed(&seed, &live());
        assert_eq!(basis.semantic_imports, None);
        assert_eq!(basis.route_surface, None);
        // The scalar domains are unaffected: one composite disarming must
        // not cost the others their precision.
        assert_eq!(basis.content, Some(AggregateStamp::Generation(11)));
    }

    /// A clock in flight disarms its own composite and leaves the rest
    /// armed — the same fail-safe direction as an absent producer.
    #[test]
    fn an_in_flight_clock_disarms_only_its_own_composite() {
        let live = LiveAggregateCounters {
            semantic_imports: None,
            ..live()
        };
        let basis = AggregateGenerations::from_seed(&vouched(), &live);
        assert_eq!(basis.semantic_imports, None);
        assert!(
            matches!(basis.route_surface, Some(AggregateStamp::RouteSurface(_))),
            "the sibling composite must stay armed; got {:?}",
            basis.route_surface
        );
    }

    /// Movement detection is armed by a vouched seed and is INERT for an
    /// unvouched one: the pair is what makes the seed's fail-safe
    /// observable at the predicate every admission boundary consults.
    ///
    /// The moving domain is `WorkspaceShape`, the one a seeded basis can
    /// mint without a view population — see the pair below for why that
    /// choice is load-bearing rather than arbitrary.
    #[test]
    fn a_vouched_basis_detects_mintable_movement_and_an_unvouched_one_cannot() {
        let installed = AggregateGenerations::from_seed(&vouched(), &live());
        let moved_live = LiveAggregateCounters {
            workspace_shape: 99,
            ..live()
        };
        let moved = AggregateGenerations::from_seed(&vouched(), &moved_live);
        assert!(
            installed.any_named_domain_moved(&moved),
            "a live workspace-shape bump must register as movement under a vouched seed"
        );

        let unvouched = AggregateGenerations::from_seed(&AggregateBasisSeed::Unvouched, &live());
        let unvouched_moved =
            AggregateGenerations::from_seed(&AggregateBasisSeed::Unvouched, &moved_live);
        assert!(
            !unvouched.any_named_domain_moved(&unvouched_moved),
            "an unvouched scope can mint nothing, so nothing can move under it"
        );
    }

    /// **Movement is examined for the domains a scope can MINT, not for
    /// every domain it holds a stamp for.**
    ///
    /// A basis with no view population carries four view-derived stamps
    /// it can never mint an aggregate from. Their generations move
    /// constantly inside cold computes — a resolved-import admission
    /// moves `SemanticImports` on essentially every one — and refusing a
    /// scope for a claim it never made would cost that compute's
    /// admission while buying no soundness.
    ///
    /// Asserted as a PAIR against the same movement, because "ignore
    /// content" alone would also be satisfied by never examining it: with
    /// a population supplied the very same bump DOES register.
    #[test]
    fn movement_is_examined_only_for_domains_the_basis_can_mint() {
        let moved_live = LiveAggregateCounters {
            content: 12,
            semantic_imports: Some(77),
            ..live()
        };

        let AggregateBasisSeed::Vouched {
            semantic_imports,
            route_surface,
            ..
        } = vouched()
        else {
            unreachable!()
        };
        let unpopulated_seed = AggregateBasisSeed::Vouched {
            view_population: None,
            view_domains: ViewAggregateDomains::ALL,
            semantic_imports,
            route_surface,
        };
        let unpopulated = AggregateGenerations::from_seed(&unpopulated_seed, &live());
        let unpopulated_moved = AggregateGenerations::from_seed(&unpopulated_seed, &moved_live);
        assert!(
            !unpopulated.can_mint(CompactionDomain::Content),
            "fixture invariant: a seeded basis must not be able to mint Content"
        );
        assert!(
            !unpopulated.any_named_domain_moved(&unpopulated_moved),
            "a domain this basis cannot mint must not destabilise the scope when it moves"
        );

        // The same two bumps, with the population that makes those
        // domains mintable.
        let populated = AggregateGenerations::from_seed(&vouched(), &live());
        let populated_moved = AggregateGenerations::from_seed(&vouched(), &moved_live);
        assert!(
            populated.can_mint(CompactionDomain::Content),
            "a view population is exactly what makes a view-derived domain mintable"
        );
        assert!(
            populated.any_named_domain_moved(&populated_moved),
            "and once mintable, the SAME movement must destabilise the scope — otherwise the \
             assertion above is satisfied by never examining the domain at all"
        );
    }

    /// A vouched seed carries the population its bound view captured.
    /// The population and explicit domain participation are independent:
    /// both are required to mint a view-derived aggregate.
    #[test]
    fn a_seeded_basis_supplies_its_view_population() {
        let basis = AggregateGenerations::from_seed(&vouched(), &live());
        assert_eq!(basis.view_population, Some(ViewPopulation::Base));
        assert!(
            basis.can_mint(CompactionDomain::Content),
            "the population and Content participation arm compaction together"
        );
    }
}

#[cfg(test)]
mod request_completion_population_tests {
    use super::*;

    fn session() -> ViewPopulationParent {
        ViewPopulationParent::SessionOverlay(
            SessionOverlayFingerprint::new(0xABCD).expect("non-zero"),
        )
    }

    /// **An empty completion overlay projects to its PARENT.**
    ///
    /// This is what makes an aggregate minted under one empty request
    /// view reusable by the next one, and by the durable base reader.
    /// A distinct population here would partition every empty request
    /// view from every other, and no view-derived aggregate would ever
    /// warm-hit across requests.
    #[test]
    fn an_empty_overlay_projects_to_its_parent_population() {
        assert_eq!(
            ViewPopulation::refined_by_completion(
                ViewPopulationParent::Base,
                CompletionOverlayState::Empty
            ),
            Some(ViewPopulation::Base)
        );
        assert_eq!(
            ViewPopulation::refined_by_completion(session(), CompletionOverlayState::Empty),
            Some(ViewPopulation::SessionOverlay(
                SessionOverlayFingerprint::new(0xABCD).expect("non-zero")
            ))
        );
    }

    /// Two DIFFERENT empty overlays project to the SAME population, which
    /// is the property the reuse depends on — asserted separately,
    /// because "projects to the parent" would also be satisfied by an
    /// implementation that folded the overlay id in somewhere.
    #[test]
    fn two_distinct_empty_overlays_share_one_population() {
        let first = ViewPopulation::refined_by_completion(
            ViewPopulationParent::Base,
            CompletionOverlayState::Empty,
        );
        let second = ViewPopulation::refined_by_completion(
            ViewPopulationParent::Base,
            CompletionOverlayState::Empty,
        );
        assert_eq!(first, second);
        assert_eq!(first, Some(ViewPopulation::Base));
    }

    /// A SHADOWING overlay is its own population, and does NOT satisfy a
    /// base read. The pair with the empty case above is the whole rule.
    #[test]
    fn a_shadowing_overlay_is_a_distinct_population_from_its_parent() {
        let id = OverlayId::fresh();
        let shadowing = ViewPopulation::refined_by_completion(
            ViewPopulationParent::Base,
            CompletionOverlayState::Shadowing {
                overlay_id: id,
                revision: 2,
            },
        );
        assert_ne!(
            shadowing,
            Some(ViewPopulation::Base),
            "a shadowing overlay re-roots the per-canonical facts an aggregate collapses, so its \
             aggregate must never satisfy a base read"
        );
        assert_eq!(
            shadowing,
            Some(ViewPopulation::RequestCompletion(RequestCompletion {
                parent: ViewPopulationParent::Base,
                overlay_id: id,
                revision: 2,
            }))
        );
    }

    /// The identity discriminates on ALL THREE components. Asserted
    /// component-by-component against one baseline, so an implementation
    /// that dropped any single one is red.
    #[test]
    fn the_identity_discriminates_on_parent_overlay_and_revision() {
        let id = OverlayId::fresh();
        let other_id = OverlayId::fresh();
        assert_ne!(id, other_id, "ids must be process-unique");
        let shadow = |parent, overlay_id, revision| {
            ViewPopulation::refined_by_completion(
                parent,
                CompletionOverlayState::Shadowing {
                    overlay_id,
                    revision,
                },
            )
        };
        let base = shadow(ViewPopulationParent::Base, id, 1);

        assert_ne!(base, shadow(session(), id, 1), "parent must discriminate");
        assert_ne!(
            base,
            shadow(ViewPopulationParent::Base, other_id, 1),
            "overlay id must discriminate: two requests at the same revision are not one \
             population"
        );
        assert_ne!(
            base,
            shadow(ViewPopulationParent::Base, id, 2),
            "revision must discriminate: the same overlay in a different shadowing state is not \
             the same population"
        );
        assert_eq!(
            base,
            shadow(ViewPopulationParent::Base, id, 1),
            "and the same three components are the same population — otherwise nothing reuses"
        );
    }

    /// **A writer mid-update names NO population.**
    ///
    /// Not the parent: projecting to the parent would claim "shadows
    /// nothing" while a shadow is being installed, which is exactly the
    /// stale serve the population exists to prevent. Not a revision
    /// either — there is no readable one.
    #[test]
    fn an_in_flight_overlay_names_no_population_rather_than_its_parent() {
        assert_eq!(
            ViewPopulation::refined_by_completion(
                ViewPopulationParent::Base,
                CompletionOverlayState::InFlight
            ),
            None
        );
        assert_eq!(
            ViewPopulation::refined_by_completion(session(), CompletionOverlayState::InFlight),
            None
        );
    }

    /// Ids are process-unique and never zero, so a zero-initialised field
    /// cannot pass for a minted one.
    #[test]
    fn overlay_ids_are_unique_and_never_zero() {
        let ids: Vec<OverlayId> = (0..64).map(|_| OverlayId::fresh()).collect();
        assert!(ids.iter().all(|id| id.get() != 0));
        let distinct: std::collections::BTreeSet<u64> = ids.iter().map(|id| id.get()).collect();
        assert_eq!(distinct.len(), ids.len(), "ids must never repeat");
    }

    /// A completion population is a `ViewPopulation`, so it flows through
    /// the aggregate machinery unchanged — including `can_mint`, which is
    /// what will arm the four view-derived domains under it.
    #[test]
    fn a_completion_population_arms_the_view_derived_domains() {
        let basis = AggregateGenerations {
            content: Some(AggregateStamp::Generation(1)),
            view_population: ViewPopulation::refined_by_completion(
                ViewPopulationParent::Base,
                CompletionOverlayState::Shadowing {
                    overlay_id: OverlayId::fresh(),
                    revision: 1,
                },
            ),
            ..AggregateGenerations::default()
        };
        assert!(
            basis.can_mint(CompactionDomain::Content),
            "a completion population is a population: it must arm the view-derived domains \
             exactly as a session one does"
        );

        let in_flight = AggregateGenerations {
            view_population: ViewPopulation::refined_by_completion(
                ViewPopulationParent::Base,
                CompletionOverlayState::InFlight,
            ),
            ..basis
        };
        assert!(
            !in_flight.can_mint(CompactionDomain::Content),
            "and an unnameable one must disarm them"
        );
    }
}
