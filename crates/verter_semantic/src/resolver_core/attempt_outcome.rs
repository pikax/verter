//! Batched input-loading contract types: `AttemptOutcome<T>` / `LoadSet`.
//!
//! Under the input-loading contract, the
//! compiler, resolver, `TypeInfo`, flow, and reusable query kernels consume
//! one immutable observation view per attempt and perform no hidden
//! filesystem/network/process/package-manager I/O. A kernel operation that
//! cannot complete against its current snapshot returns
//! `AttemptOutcome::NeedInputs(LoadSet)` describing exactly what is missing,
//! rather than blocking the calling thread.
//!
//! These types are the resolver kernel's closed retry vocabulary.
//! They are a *different* envelope from
//! [`crate::query::QueryResult`]'s `Completeness`/`missing_inputs` shape,
//! which reports public query-result completeness, not kernel retry
//! signaling — the two must not be merged.

use std::collections::BTreeSet;
use std::sync::Arc;

/// Canonical file/path identity as observed by a kernel attempt.
///
/// Dependency-neutral interned string, matching the shape of
/// `verter_session::capture_token::CanonicalId` (`Arc<str>`) without
/// depending on that crate — `verter_semantic` sits below `verter_session`
/// in the dependency graph and must not name it.
pub type CanonicalId = Arc<str>;

/// Which lowering space a [`InputKey::DeclBody`] demand targets.
///
/// A bare `(canonical, owner, name)` `DeclBody` key cannot tell a retry
/// driver whether [`crate::resolver_core::ResolverObservation::type_decl`]
/// or [`crate::resolver_core::ResolverObservation::value_decl`] produced
/// it — the two spaces can independently miss for the same declaration
/// name (e.g. a `type Foo` and a `const Foo` in the same file), and a
/// driver that could not tell them apart would either load the wrong
/// space or load both speculatively every time.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum DeclarationSpace {
    Type,
    Value,
}

/// One independently-loadable input a kernel attempt discovered it needs.
///
/// Closed taxonomy: every kernel-observable I/O shape a non-flow resolver /
/// `TypeInfo` kernel can require (contract §2: "the kernel discovers all
/// independently reachable missing observations it can identify without
/// fabricating semantic answers"). Adding a variant is a deliberate
/// widening of the observation surface, not a per-caller extension point.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum InputKey {
    /// The full parsed/shallow-indexed content of a canonical file.
    FileContent { canonical: CanonicalId },
    /// Whether a path exists on disk (file or directory), independent of
    /// content — the `probe_path`/`probe_path_for_context` shape.
    PathProbe { path: CanonicalId },
    /// A `package.json` manifest at a directory.
    PackageManifest { directory: CanonicalId },
    /// Symlink-resolved real path for a candidate.
    RealPath { path: CanonicalId },
    /// The lazily lowered body of one declaration in `canonical`, keyed by
    /// its top-level owner + symbol name (the `DeclBindingKey` shape) plus
    /// the [`DeclarationSpace`] it was demanded in (see that type's docs)
    /// — `verter_session`'s `DeclBodyMemo`/`DeclLoweringService` demand.
    /// Distinct from `FileContent`: the file's shallow index can
    /// be fully loaded while an individual declaration's body is still
    /// un-lowered.
    DeclBody {
        canonical: CanonicalId,
        owner: verter_type_expr::TopLevelOwnerId,
        name: Arc<str>,
        space: DeclarationSpace,
    },
    /// The module-augmentation contributor index for one
    /// `AugmentationTargetKey` — `verter_session`'s
    /// `FileArtifactStore::ensure_augmentation_index_populated` demand.
    /// Distinct from `FileContent`: an augmenter's OWN shallow index can be fully loaded
    /// while the cross-project inverse index that finds it as a
    /// contributor is still unscanned.
    ModuleAugmentationIndex {
        target: crate::resolver_core::AugmentationTargetKey,
    },
    /// The memoized `FunctionBodySkeleton` of one content-pinned function —
    /// `verter_session`'s `FunctionFlowGraphStore`/`RetainedSnapshotSkeletonSource`
    /// demand. Distinct from
    /// `DeclBody`: a skeleton is a FLOW-specific, per-function-position
    /// artifact (arena-free, span-relative-to-function-start), not a
    /// declaration's lowered type/value body.
    FlowFunctionSkeleton {
        key: crate::resolver_core::FlowFunctionObservationKey,
    },
}

/// The captured resolution-world identity a [`ResolutionBasis`] is bound
/// to — the workspace-side half of the basis.
///
/// Exact structured tuple, deliberately NOT folded into a scalar
/// fingerprint — mirrors the existing `AggregateStamp` precedent (exact
/// tuples over digests, `verter_workspace::fact_cache::AggregateStamp`).
/// `workspace_authority` is required because `ResolutionWorldId`'s counter
/// restarts at `1` per `Engine` — root ids are unique WITHIN one engine,
/// not globally. `base` is the exact base-world root id; `session` is
/// `Some` only for a session-population attempt, carrying the exact
/// session-world root id alongside the base it composes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct ResolutionWorldBasis {
    workspace_authority: crate::resolver_core::WorkspaceAuthorityId,
    population: crate::resolver_core::ResolutionPopulation,
    base: crate::resolver_core::ResolutionWorldId,
    session: Option<crate::resolver_core::ResolutionWorldId>,
}

impl ResolutionWorldBasis {
    #[must_use]
    pub const fn new(
        workspace_authority: crate::resolver_core::WorkspaceAuthorityId,
        population: crate::resolver_core::ResolutionPopulation,
        base: crate::resolver_core::ResolutionWorldId,
        session: Option<crate::resolver_core::ResolutionWorldId>,
    ) -> Self {
        Self {
            workspace_authority,
            population,
            base,
            session,
        }
    }

    #[must_use]
    pub const fn workspace_authority(&self) -> crate::resolver_core::WorkspaceAuthorityId {
        self.workspace_authority
    }

    #[must_use]
    pub const fn population(&self) -> crate::resolver_core::ResolutionPopulation {
        self.population
    }

    #[must_use]
    pub const fn base(&self) -> crate::resolver_core::ResolutionWorldId {
        self.base
    }

    #[must_use]
    pub const fn session(&self) -> Option<crate::resolver_core::ResolutionWorldId> {
        self.session
    }
}

/// The project/config resolution basis a [`LoadSet`] was computed against
/// (contract §4 step 9, §6: a basis change invalidates an in-flight load
/// and forces a restart rather than splicing data into the old attempt).
///
/// An exact structured pair: the captured resolution-world
/// identity, plus (for a full session attempt) the exact
/// [`crate::resolver_core::StoreViewValidationToken`] binding the
/// non-resolution session/store-view dimensions. `session_view` is `None`
/// for a workspace-only attempt (no `verter_session` in the picture). The
/// kernel does not interpret either field's internals, only compares the
/// whole basis for equality across attempts — never a hash/fold of it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct ResolutionBasis {
    resolution_world: ResolutionWorldBasis,
    session_view: Option<crate::resolver_core::StoreViewValidationToken>,
}

impl ResolutionBasis {
    #[must_use]
    pub const fn new(
        resolution_world: ResolutionWorldBasis,
        session_view: Option<crate::resolver_core::StoreViewValidationToken>,
    ) -> Self {
        Self {
            resolution_world,
            session_view,
        }
    }

    #[must_use]
    pub const fn resolution_world(&self) -> ResolutionWorldBasis {
        self.resolution_world
    }

    #[must_use]
    pub const fn session_view(&self) -> Option<crate::resolver_core::StoreViewValidationToken> {
        self.session_view
    }

    /// Placeholder basis for an explicitly unbound attempt view.
    /// Every field is each type's own `UNBOUND_PLACEHOLDER` sentinel,
    /// which `verter_workspace`'s `fresh()` non-zero invariant guarantees
    /// a real captured-world mint can never produce — a placeholder basis
    /// can never be mistaken for, or spuriously equal, a real one. NOT a
    /// scalar fold; do not spread new uses beyond the
    /// existing pre-driver placeholder call sites, and replace each with
    /// a real basis once its `ResolverAttemptView` wiring lands.
    #[must_use]
    pub const fn unbound_placeholder() -> Self {
        Self::new(
            ResolutionWorldBasis::new(
                crate::resolver_core::WorkspaceAuthorityId::UNBOUND_PLACEHOLDER,
                crate::resolver_core::ResolutionPopulation::Base,
                crate::resolver_core::ResolutionWorldId::UNBOUND_PLACEHOLDER,
                None,
            ),
            None,
        )
    }
}

/// Normalized, sorted, deduplicated set of missing inputs plus the
/// resolution basis they were computed against (contract §2, §4).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LoadSet {
    keys: Vec<InputKey>,
    basis: ResolutionBasis,
}

impl LoadSet {
    /// Builds a normalized `LoadSet`: sorts and dedups `keys` (contract
    /// §2: "`LoadSet` is normalized, sorted, deduplicated").
    #[must_use]
    pub fn new(mut keys: Vec<InputKey>, basis: ResolutionBasis) -> Self {
        keys.sort();
        keys.dedup();
        Self { keys, basis }
    }

    /// An empty `LoadSet` against `basis` — never itself a valid
    /// `NeedInputs` payload (contract §4 step 5: an empty delta with no
    /// basis change is `InputResolutionNoProgress`, not a repeat
    /// `NeedInputs`), but a useful zero value for accumulation.
    #[must_use]
    pub const fn empty(basis: ResolutionBasis) -> Self {
        Self {
            keys: Vec::new(),
            basis,
        }
    }

    #[must_use]
    pub fn keys(&self) -> &[InputKey] {
        &self.keys
    }

    #[must_use]
    pub const fn basis(&self) -> ResolutionBasis {
        self.basis
    }

    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.keys.is_empty()
    }

    /// Contract §4 step 4: `delta = L.keys - accumulated_requested`.
    #[must_use]
    pub fn delta(&self, accumulated_requested: &BTreeSet<InputKey>) -> Vec<InputKey> {
        self.keys
            .iter()
            .filter(|key| !accumulated_requested.contains(*key))
            .cloned()
            .collect()
    }
}

/// Closed taxonomy naming every [`crate::resolver_core::ResolverObservation`]
/// method — one variant per method, in trait declaration order.
///
/// The keyed input-load failure variants name a loadable
/// [`InputKey`], but five observations (`env_hashes`, `project_identity`,
/// `lookup_ambient_symbol`, `project_generation`,
/// `workspace_is_package_backed`'s no-load derivation) are IMMEDIATE
/// values with no `InputKey` to name — a driver missing the capability to
/// answer one of those (e.g. a workspace-only `ResolverAttemptView` asked
/// for `project_generation`, which only a full session driver can answer)
/// needs a typed way to say so without fabricating an `InputKey` that
/// does not exist for that observation.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ResolverObservationKind {
    EnvHashes,
    ProjectIdentity,
    WholeHash,
    WorkspaceIsPackageBacked,
    LookupAmbientSymbol,
    ProjectGeneration,
    TypeDecl,
    ValueDecl,
    ModuleAugmentationIndex,
    FunctionBodySkeleton,
    PathProbe,
    RealPath,
    PackageManifest,
}

/// Why a bounded input-load result cannot satisfy its exact reservation.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum InputLoadIntegrityReason {
    KeySetMismatch,
    BasisMismatch,
    ActualOverReservation,
    IncompleteBoundedCapture,
}

/// Typed resource/progress failures (contract §8) — distinct from a bare
/// bool/`Option` "gave up" signal so callers can report unresolved keys and
/// consumed budget without exposing sensitive ambient paths beyond the
/// product's diagnostic policy.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AttemptFailure {
    /// Contract §4 step 5: `delta` was empty and neither the resolution
    /// basis nor any previously observed fact changed.
    InputResolutionNoProgress { unresolved: Vec<InputKey> },
    InputResolutionAttemptLimit {
        unresolved: Vec<InputKey>,
        attempts: u32,
    },
    InputResolutionUniqueKeyLimit {
        unresolved: Vec<InputKey>,
        unique_keys: u32,
    },
    InputResolutionByteLimit {
        unresolved: Vec<InputKey>,
        bytes: u64,
    },
    InputResolutionDepthLimit {
        unresolved: Vec<InputKey>,
        depth: u32,
    },
    InputResolutionChurnLimit {
        unresolved: Vec<InputKey>,
        churn: u32,
    },
    InputResolutionAliasGeometryRetentionLimit {
        retained: u32,
        prospective: u32,
        maximum: u32,
    },
    InputResolutionCompletedWitnessRetentionLimit {
        retained: u32,
        prospective: u32,
        maximum: u32,
    },
    /// The requested input is permanently unavailable from this loader.
    /// This is terminal and never retried or cached as stable missing.
    InputLoadUnavailable { key: InputKey },
    /// A same-key I/O flight reported a transient (non-stable-missing)
    /// failure. The workspace driver may retry only after proving that the
    /// next attempt and the same reservation both fit the operation ledger.
    TransientInputLoadFailure { key: InputKey },
    /// The bounded loader did not faithfully satisfy its exact reservation.
    /// This is terminal and never cacheable as either a hit or stable miss.
    InputLoadIntegrity {
        unresolved: Vec<InputKey>,
        reason: InputLoadIntegrityReason,
    },
    /// Conditional commit (contract §4 step 8-9) lost the race past the
    /// configured churn/retry budget.
    InputCommitConflictExceeded { retries: u32 },
    /// A driver was asked for an observation outside its own populated
    /// capability subset (see
    /// [`ResolverObservationKind`]'s docs). Never itself a `NeedInputs`
    /// signal: the driver genuinely cannot answer, keyed or not, so the
    /// caller must fall back to a different attempt view/driver rather
    /// than retry.
    ObservationUnavailable {
        observation: ResolverObservationKind,
    },
}

impl AttemptFailure {
    /// Whether this terminal means the input-resolution work envelope was
    /// exhausted before the attempt could prove its answer.
    #[must_use]
    pub fn is_input_resolution_limit(&self) -> bool {
        matches!(
            self,
            Self::InputResolutionAttemptLimit { .. }
                | Self::InputResolutionUniqueKeyLimit { .. }
                | Self::InputResolutionByteLimit { .. }
                | Self::InputResolutionDepthLimit { .. }
                | Self::InputResolutionChurnLimit { .. }
                | Self::InputResolutionAliasGeometryRetentionLimit { .. }
                | Self::InputResolutionCompletedWitnessRetentionLimit { .. }
        )
    }
}

/// Per-attempt outcome of a non-flow kernel operation (contract §2).
///
/// `Complete`/`NeedInputs`/`Terminal` are exhaustive and closed: a kernel
/// method returning this type can never block the calling thread — the
/// only way to signal "not yet answerable" is `NeedInputs(LoadSet)`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AttemptOutcome<T> {
    Complete(T),
    NeedInputs(LoadSet),
    Terminal(AttemptFailure),
}

impl<T> AttemptOutcome<T> {
    /// Maps the `Complete` payload, leaving `NeedInputs`/`Terminal` as-is.
    pub fn map<U>(self, f: impl FnOnce(T) -> U) -> AttemptOutcome<U> {
        match self {
            Self::Complete(value) => AttemptOutcome::Complete(f(value)),
            Self::NeedInputs(load_set) => AttemptOutcome::NeedInputs(load_set),
            Self::Terminal(failure) => AttemptOutcome::Terminal(failure),
        }
    }

    #[must_use]
    pub fn is_complete(&self) -> bool {
        matches!(self, Self::Complete(_))
    }

    #[must_use]
    pub fn is_need_inputs(&self) -> bool {
        matches!(self, Self::NeedInputs(_))
    }

    #[must_use]
    pub fn is_terminal(&self) -> bool {
        matches!(self, Self::Terminal(_))
    }

    /// Extracts the `Complete` payload, discarding `NeedInputs`/`Terminal`.
    #[must_use]
    pub fn complete(self) -> Option<T> {
        match self {
            Self::Complete(value) => Some(value),
            Self::NeedInputs(_) | Self::Terminal(_) => None,
        }
    }
}

/// A successfully completed kernel attempt's answer, paired with the
/// [`crate::resolver_core::AttemptOutput`] it accumulated along the way.
///
/// The only envelope that publishes an [`AttemptOutput`] with a completed
/// kernel answer.
/// `AttemptOutcome::Complete(T)` itself stays UNCHANGED — this wrapper
/// exists at the TOP-LEVEL kernel entry point ([`KernelAttempt`]), not on
/// [`ResolverObservation`]'s 13 inbound query methods, which have no
/// outbound effects of their own to accumulate.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CompletedAttempt<T> {
    pub value: T,
    pub output: crate::resolver_core::AttemptOutput,
}

impl<T> CompletedAttempt<T> {
    #[must_use]
    pub const fn new(value: T, output: crate::resolver_core::AttemptOutput) -> Self {
        Self { value, output }
    }
}

/// The top-level kernel attempt envelope: [`AttemptOutcome`] specialized
/// so a successful attempt carries its accumulated
/// [`crate::resolver_core::AttemptOutput`] alongside the answer.
/// `NeedInputs`/`Terminal` carry no output — an attempt that does not
/// reach `Complete` discards everything it accumulated (contract §4: no
/// torn/partial output is ever promoted).
pub type KernelAttempt<T> = AttemptOutcome<CompletedAttempt<T>>;

#[cfg(test)]
#[path = "attempt_outcome_tests.rs"]
mod attempt_outcome_tests;
