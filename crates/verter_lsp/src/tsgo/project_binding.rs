//! The ONE shared per-carrier project-binding helper for the tsgo admission layer.
//!
//! A framework carrier reaches the external-TS engine as a member of its REAL
//! configured project — never a config-less inferred / single-file Program. This
//! helper is the SOLE host-backed resolution path both the always-present OWNED
//! carrier-diagnostics gate ([`crate::tsgo::composite`]) and the optional SHARED
//! overlay drive: it resolves a carrier SOURCE to its owning configured project's
//! [`BoundProject`] witness over the host's LIVE published snapshot through the
//! shared [`WorkspaceProjectResolver`], minting the witness from the resolved
//! [`ProjectBinding`] through the tsgo [`EngineBackend`]. There is ONE binding path;
//! neither the OWNED gate nor SHARED resolves ownership on its own.
//!
//! Every non-bound state — a not-yet-ready published snapshot, `NoProject`,
//! `Ambiguous`, `SyntheticScratch`, or an `ensure_project` failure — is a DISTINCT
//! fail-closed outcome that yields NO `BoundProject`, so the caller serves no
//! external-TS result for the carrier (never an inferred/path-only fallback).

use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use verter_session::external_ts::{
    AmbiguityCause, BoundProject, CarrierOwnershipResolution, EngineBackend, EnvDims,
    ExternalTsProjectResolver, ProjectBinding, WorkspaceProjectResolver,
};
use verter_session::VerterHost;
use verter_workspace::published_state::PublishedRoot;
use verter_workspace::resolver::normalize_canonical_id;

use crate::external_ts::TsgoEngineBackend;

/// The bootstrap engine version the OWNED gate resolves + mints the witness with.
///
/// `ts_version` is carried onto the resolved binding's metadata and the minted
/// backend capabilities, but it is NOT load-bearing for the witness identity, the
/// binding's project identity / tsconfig / references, or the downstream `--api`
/// operation (OWNED user-facing diagnostics ride the `--lsp` pull; the SHARED `--api`
/// snapshot rail keys on the transport's own gate-observed version). So the coarse
/// bound-or-not gate decision — and the tsconfig the SHARED path reuses from the
/// witness — are version-independent, and this empty bootstrap is safe (it mirrors
/// the shared overlay's `Arc::from("")` shadow-safety probe).
const OWNED_GATE_BOOTSTRAP_VERSION: &str = "";

/// A carrier resolved to its owning configured project's [`BoundProject`] witness,
/// plus the resolved [`ProjectBinding`] and the published-snapshot generation it was
/// resolved at. The SHARED overlay reuses ALL THREE (the binding for its per-query
/// re-decision, the generation for the transport re-arm, and `bound.project()` — the
/// version-independent owning tsconfig — for the `--api` overlay), so a bound carrier
/// is resolved EXACTLY ONCE for both the OWNED gate and the SHARED union.
#[derive(Debug)]
pub struct BoundCarrier {
    bound: BoundProject,
    binding: ProjectBinding,
    generation: u64,
}

impl BoundCarrier {
    /// The minted project-bound witness (its `project()` is the owning tsconfig).
    #[must_use]
    pub fn bound(&self) -> &BoundProject {
        &self.bound
    }

    /// The resolved project binding (for the SHARED per-query re-decision + transport).
    #[must_use]
    pub fn binding(&self) -> &ProjectBinding {
        &self.binding
    }

    /// The published-snapshot / config generation the binding was resolved at.
    #[must_use]
    pub fn generation(&self) -> u64 {
        self.generation
    }
}

/// The outcome of resolving a carrier source to its owning configured project. Only
/// [`CarrierBinding::Bound`] admits an external-TS result; every other arm is a
/// DISTINCT fail-closed state (kept distinct for testing + diagnostics) that yields
/// NO `BoundProject` — the caller serves no external-TS diagnostics for the carrier,
/// never an inferred/path-only fallback.
#[derive(Debug)]
pub enum CarrierBinding {
    /// A resolved configured project — the ONLY state that admits an external-TS
    /// result. Carries the minted witness + binding + generation (boxed: the bound
    /// payload dwarfs the unit fail-closed variants).
    Bound(Box<BoundCarrier>),
    /// The host's published snapshot is not yet ready (`published_root() == None`) —
    /// fail closed to no result (matches the SHARED `published_root()?` semantics),
    /// NEVER recovered via path-only inferred discovery.
    PreSnapshot,
    /// The resolver found no owning tsconfig for the source.
    NoProject,
    /// Two configs claim the source with no deterministic leaf, or a carrier-path
    /// conflict (a real user file at a companion path / a same-stem rune module).
    Ambiguous(AmbiguityCause),
    /// An untitled buffer / file outside any tsconfig — the scratch lane, never a
    /// configured-project external-TS result.
    SyntheticScratch,
    /// The binding resolved but the engine backend refused to mint the witness.
    EnsureFailed,
}

impl CarrierBinding {
    /// Whether a real configured-project witness resolved (the admission gate).
    #[must_use]
    pub fn is_bound(&self) -> bool {
        matches!(self, CarrierBinding::Bound(_))
    }

    /// The [`BoundCarrier`] IFF a configured project was bound, else `None` — every
    /// non-bound state collapses to the ONE fail-closed `None` the caller gates on.
    #[must_use]
    pub fn into_bound(self) -> Option<BoundCarrier> {
        match self {
            CarrierBinding::Bound(bound) => Some(*bound),
            _ => None,
        }
    }
}

/// Resolve the carrier `source`'s owning project over the host's LIVE published
/// snapshot through the shared [`WorkspaceProjectResolver`], returning the FULL
/// [`CarrierOwnershipResolution`] and the snapshot/config generation it was resolved at
/// (`None` when the published snapshot is not yet ready). The single host-backed
/// resolution entry the OWNED gate, the SHARED binding path, and the shadow-safety
/// gate all share — the env-dims closure reads the host's per-project R21 env-hash
/// reader (`host_view_env_hashes_for` / `host_view_project_identity_for`), never a
/// fabricated/default env identity.
///
/// `ts_version` is carried onto a resolved binding's metadata; it is NOT load-bearing
/// for the witness identity or the `--api` op, so a bootstrap value (the OWNED gate)
/// or an empty value (the shadow-safety probe) is safe.
///
/// `readiness_mode` selects how a PRESENT-but-cold published snapshot is treated —
/// see [`OwnershipReadinessMode`].
#[must_use]
pub fn resolve_carrier(
    host: &VerterHost,
    source: &str,
    ts_version: Arc<str>,
    readiness_mode: OwnershipReadinessMode,
) -> Option<(CarrierOwnershipResolution, u64)> {
    let ws_read = host.workspace_read();
    let published = ws_read.published_root()?;
    let generation = published.snapshot.generation.0;
    // The env-dims reader is keyed on a MEMBER canonical of the resolved project
    // (the resolved carrier source), NOT the tsconfig path: a tsconfig file is
    // normally outside the project's membership set, so keying the per-canonical
    // host readers on it resolves to no owner and falls back to workspace-default
    // dims. An owned member yields the project's real per-project env identity.
    let env_dims_source = |member_canonical: &str| {
        let env = host.host_view_env_hashes_for(member_canonical);
        EnvDims {
            parse_env_hash: env.parse_env_hash,
            resolve_env_hash: env.resolve_env_hash,
            lib_env_hash: env.lib_env_hash,
            project_identity: host.host_view_project_identity_for(member_canonical),
        }
    };
    // Under `PresentSnapshotAuthoritative` a PRESENT published snapshot is the
    // authority: the bootstrap-absent case is the earlier `published_root()?` (⇒
    // `PreSnapshot`), and a present-but-empty snapshot must still resolve (⇒
    // `NoProject`), never defer — the OWNED admission gate + the shadow-safety probe
    // rely on this (a present snapshot published with `ownership_ready == false` must
    // still bind). Under `ObservePublishedReadiness` the resolver instead threads the
    // real `PublishedRoot::ownership_ready`, so a cold-bootstrap snapshot resolves
    // `NotReady` (the `verter(project)` diagnostics consumer defers rather than
    // emitting a premature terminal decision, exactly as the carrier-sync gateway).
    let ownership_ready = match readiness_mode {
        OwnershipReadinessMode::PresentSnapshotAuthoritative => true,
        OwnershipReadinessMode::ObservePublishedReadiness => published.ownership_ready,
    };
    let resolver = WorkspaceProjectResolver::new(
        published.snapshot.as_ref(),
        ws_read.as_ref(),
        ts_version,
        &env_dims_source,
        ownership_ready,
    );
    Some((resolver.resolve(source, None), generation))
}

/// How [`resolve_carrier`] treats a PRESENT-but-cold published snapshot.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OwnershipReadinessMode {
    /// A PRESENT published snapshot is the authority: resolve `Bound` / `NoProject`
    /// / `Ambiguous`, NEVER `NotReady`. The always-present OWNED carrier-diagnostics
    /// gate and the SHARED overlay shadow-safety probe use this — the
    /// bootstrap-absent case is the earlier `published_root()?` (⇒ `PreSnapshot`),
    /// and a present-but-empty snapshot must still resolve authoritatively. Sourcing
    /// readiness from the bootstrap bool here would regress the OWNED gate: a present
    /// snapshot published with `ownership_ready == false` (the base-VFS publish) must
    /// still bind its owner rather than spuriously defer.
    PresentSnapshotAuthoritative,
    /// OBSERVE the published root's `ownership_ready`: a non-authoritative
    /// (cold-bootstrap) snapshot resolves `NotReady` instead of a premature terminal
    /// `NoProject` / `Ambiguous`. The user-visible `verter(project)` diagnostics use
    /// this so a bootstrap snapshot defers (no false diagnostic) exactly as the
    /// carrier-sync gateway does, rather than surfacing a spurious no-owner warning.
    ObservePublishedReadiness,
}

/// Resolve the carrier `source` to its owning configured project's [`BoundProject`]
/// witness — the ONE admission entry the always-present OWNED carrier-diagnostics
/// gate obtains a `BoundProject` from before delegating to `TsgoOwnedProvider`.
///
/// Published-snapshot → [`WorkspaceProjectResolver`] → `resolve(source)` → on
/// [`CarrierOwnershipResolution::Bound`] mint the witness through
/// `TsgoEngineBackend::ensure_project(binding.ensure_project_request())`. Every other
/// state ([`CarrierOwnershipResolution::NoProject`] / [`CarrierOwnershipResolution::Ambiguous`] /
/// [`CarrierOwnershipResolution::NotReady`], a pre-published snapshot, or an
/// `ensure_project` failure) is a DISTINCT fail-closed [`CarrierBinding`] variant
/// that yields NO witness — NEVER a path-only inferred fallback.
#[must_use]
pub fn resolve_carrier_bound(host: &Arc<VerterHost>, source: &str) -> CarrierBinding {
    let ts_version: Arc<str> = Arc::from(OWNED_GATE_BOOTSTRAP_VERSION);
    let Some((resolution, generation)) = resolve_carrier(
        host.as_ref(),
        source,
        Arc::clone(&ts_version),
        OwnershipReadinessMode::PresentSnapshotAuthoritative,
    ) else {
        return CarrierBinding::PreSnapshot;
    };
    match resolution {
        CarrierOwnershipResolution::Bound(binding) => {
            // Mint the BoundProject witness through the tsgo engine backend — the
            // project-bound contract's per-query witness discipline (no path-only
            // bypass). `ensure_project` is an infallible pure witness mint for a
            // resolved binding, but a refusal is a DISTINCT fail-closed state.
            let backend = TsgoEngineBackend::new(ts_version);
            match backend.ensure_project(binding.ensure_project_request()) {
                Ok(bound) => CarrierBinding::Bound(Box::new(BoundCarrier {
                    bound,
                    binding,
                    generation,
                })),
                Err(_) => CarrierBinding::EnsureFailed,
            }
        }
        CarrierOwnershipResolution::NoProject => CarrierBinding::NoProject,
        CarrierOwnershipResolution::Ambiguous { cause, .. } => CarrierBinding::Ambiguous(cause),
        // Ownership not yet authoritative (bootstrap) ⇒ fail closed to the same
        // no-result state as a missing published snapshot; the OWNED gate re-resolves
        // once ownership is authoritative.
        CarrierOwnershipResolution::NotReady => CarrierBinding::PreSnapshot,
    }
}

/// The upper bound on live admission entries within a single generation — a
/// defense-in-depth cap beyond the per-generation prune so a pathological churn of
/// distinct carrier sources at ONE generation cannot grow the map without bound. On
/// overflow the generation's entries are dropped wholesale (the next queries
/// re-resolve); admission stays correct, only warmth is lost.
const ADMISSION_CACHE_MAX_ENTRIES: usize = 4096;

/// The bounded number of cold-path fence retries. On each miss the admission decision is
/// resolved with the cache lock RELEASED; if a publication supersedes the captured epoch
/// mid-resolve the decision is discarded and re-resolved at the new epoch (never warmed).
/// After this bound a freshly-resolved decision is returned WITHOUT promoting it warm — so
/// sustained churn fails closed rather than caching a torn result. Mirrors the completion
/// fence's "retry at most 3 times on mid-flight changes" rule.
const ADMISSION_FENCE_MAX_RETRIES: usize = 3;

/// An admission EPOCH: the UNREPEATABLE publication identity a carrier FEATURE admission is
/// scoped to. Feature admission is an AUTHZ surface — a decision recorded at one epoch MUST
/// NOT authorize a feature at a later epoch — so the epoch combines all of:
///
/// * `published` — the EXACT published-root publication (`None` before the first publish).
///   Identity is the RETAINED `Arc<PublishedRoot>` POINTER (`Arc::ptr_eq`), NEVER the
///   `snapshot.generation.0` scalar: `ProjectGraph::from_configs` hard-codes every rebuilt
///   graph to generation 1, so a reconfigure republishes generation 1 and the scalar
///   REPEATS. Two distinct publications carrying the same scalar are DISTINCT epochs
///   because their Arc pointers differ, and the cache RETAINS the Arc so its pointer cannot
///   be freed-and-reused underneath a live entry (the ABA guard).
/// * `content_generation` — the workspace file-existence / content generation: a companion
///   appearing / disappearing, or a content edit, re-decides admission, and it advances
///   INDEPENDENTLY of a republish.
/// * `project_generation` — the host's MONOTONIC `current_project_generation()`, which
///   advances on a host-mediated reconfigure (never on a content edit) and NEVER resets.
///
/// Epoch equality requires ALL THREE to match — the publication by POINTER identity, the
/// two generations by value. `PartialEq` is hand-written (not derived): `PublishedRoot` is
/// compared by `Arc` pointer, never by content.
struct AdmissionEpoch {
    published: Option<Arc<PublishedRoot>>,
    content_generation: u64,
    project_generation: u64,
}

impl PartialEq for AdmissionEpoch {
    fn eq(&self, other: &Self) -> bool {
        self.content_generation == other.content_generation
            && self.project_generation == other.project_generation
            && match (&self.published, &other.published) {
                (Some(a), Some(b)) => Arc::ptr_eq(a, b),
                (None, None) => true,
                _ => false,
            }
    }
}

/// The admission map key: the NORMALIZED carrier SOURCE path. Keying on the normalized
/// source (not the companion path) means every companion of one carrier shares ONE
/// admission decision, and a backslash / non-canonical path cannot evade a warm hit. The
/// epoch is NOT part of the key — it is anchored ONCE on [`AdmissionCacheState`] and every
/// live entry belongs to that single epoch (a store at a new epoch clears the map first).
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
struct CarrierAdmissionKey {
    source: String,
}

/// The exact fail-closed reason a carrier feature admission was DENIED — the non-bound
/// [`CarrierBinding`] states, preserved distinctly (an ADMITTED carrier is never one of
/// these). `Copy`: the denied arm of a warm hit is a trivial clone.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AdmissionDenial {
    /// The host's published snapshot is not yet ready.
    PreSnapshot,
    /// The resolver found no owning tsconfig for the source.
    NoProject,
    /// Two configs claim the source, or a carrier-path conflict.
    Ambiguous,
    /// An untitled buffer / file outside any tsconfig — the scratch lane.
    SyntheticScratch,
    /// The binding resolved but the engine backend refused to mint the witness.
    EnsureFailed,
}

/// The cached outcome of a carrier FEATURE admission: an ADMITTED carrier carries the
/// resolved [`BoundCarrier`] witness behind an `Arc` (a warm hit is an arc-clone), and a
/// DENIED carrier carries the exact fail-closed [`AdmissionDenial`]. Only
/// [`Admission::Admitted`] authorizes an OWNED feature delegation; every other state —
/// and, by construction, any never-produced state — FAILS CLOSED.
#[derive(Clone)]
pub enum Admission {
    /// The carrier resolved to its owning configured project's `BoundProject` witness.
    Admitted(Arc<BoundCarrier>),
    /// The carrier failed closed — no external-TS feature is served (the exact reason).
    Denied(AdmissionDenial),
}

impl Admission {
    /// Whether the carrier was ADMITTED — the SOLE state that authorizes an OWNED feature
    /// delegation. A `Denied` (and any never-constructed) admission fails closed.
    #[must_use]
    pub fn is_admitted(&self) -> bool {
        matches!(self, Admission::Admitted(_))
    }

    /// The resolved [`BoundCarrier`] witness IFF the carrier was ADMITTED, else `None`.
    /// The witness is RETAINED so a consumer that needs the owning project (the binding
    /// or the minted `BoundProject`) reuses this ONE resolution rather than re-resolving.
    #[must_use]
    pub fn bound_carrier(&self) -> Option<&BoundCarrier> {
        match self {
            Admission::Admitted(carrier) => Some(carrier),
            Admission::Denied(_) => None,
        }
    }

    /// The exact fail-closed [`AdmissionDenial`] IFF the carrier was DENIED, else `None`.
    #[must_use]
    pub fn denial(&self) -> Option<AdmissionDenial> {
        match self {
            Admission::Denied(reason) => Some(*reason),
            Admission::Admitted(_) => None,
        }
    }
}

/// The epoch-scoped OWNED carrier FEATURE admission cache owned by
/// [`TsgoCompositeProvider`](crate::tsgo::composite::TsgoCompositeProvider). Every carrier
/// FEATURE provider call resolves its owning project ONCE per `(source, admission epoch)`
/// through the shared [`resolve_carrier_bound`] resolver; the decision is memoized and
/// reused on every warm-state editor query at the SAME epoch. There is ONE resolver — this
/// cache MEMOIZES it; it is NOT a second binding engine, and it does NOT touch the
/// carrier-diagnostics gate (which resolves its own binding per query).
///
/// The epoch is the UNREPEATABLE publication identity ([`AdmissionEpoch`]): the RETAINED
/// `Arc<PublishedRoot>` pointer plus the content + project generations. Keying the memo on
/// the publication Arc identity — NOT the repeatable `snapshot.generation.0` scalar —
/// closes the reconfigure reset hole: `configure_projects` publishes the new (possibly
/// non-owning) graph BEFORE it bumps the monotonic project generation, and
/// `ProjectGraph::from_configs` resets that scalar to 1, so a scalar key let an `admit`
/// racing the window between the publish and the bump reconstruct a prior owning epoch's
/// tuple and be served the stale `Admitted` warm (a fail-OPEN cross-epoch privilege
/// bleed). The publication-Arc epoch makes the republished root a natural lookup MISS, and
/// the cold-path fence discards any decision whose epoch was superseded mid-resolve — the
/// monotonic project generation ALONE does NOT cover that window.
///
/// Warm hit: normalize the source + capture the cheap epoch + map lookup under the SAME
/// epoch + clone the [`Admission`] (an arc-clone for `Admitted`). NO provider sync, NO FS
/// probe, NO `--lsp` request, and the cache lock is NEVER held across the cold resolve.
#[derive(Default)]
pub struct CarrierAdmissionCache {
    state: Mutex<AdmissionCacheState>,
}

/// The live admission entries plus the ONE epoch they belong to.
#[derive(Default)]
struct AdmissionCacheState {
    /// The [`AdmissionEpoch`] every live `entries` decision belongs to. A store at a
    /// DIFFERENT epoch — a different publication `Arc`, or a content / project-generation
    /// change — CLEARS the map and re-anchors here, RETAINING the new publication Arc. So
    /// no stale `Admitted` from an old epoch survives into a new one (no cross-epoch
    /// privilege bleed), the map stays bounded to one epoch's carriers, and the retained
    /// Arc pointer cannot be freed-and-reused underneath a live entry (the ABA guard).
    epoch: Option<AdmissionEpoch>,
    entries: HashMap<CarrierAdmissionKey, Admission>,
}

impl CarrierAdmissionCache {
    /// A fresh, empty admission cache.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Admit (or fail-closed deny) the carrier `source` at the host's CURRENT admission
    /// epoch. A warm hit at the SAME epoch (the same publication `Arc` identity + content +
    /// project generation) returns the memoized decision cheaply; a cold miss resolves ONCE
    /// through [`resolve_carrier_bound`] with the cache lock RELEASED, fences the result
    /// against a mid-resolve publication, then memoizes it epoch-scoped.
    #[must_use]
    pub fn admit(&self, host: &Arc<VerterHost>, source: &str) -> Admission {
        let key = CarrierAdmissionKey {
            source: normalize_canonical_id(source),
        };
        // Bounded fence retries: if a publication supersedes the captured epoch mid-resolve
        // the decision belongs to a stale view — discard it and re-resolve at the new epoch
        // (never warm a torn result). After the bound, fail closed by returning a
        // freshly-resolved decision that is NOT stored.
        for _ in 0..ADMISSION_FENCE_MAX_RETRIES {
            let epoch = Self::current_epoch(host);
            if let Some(cached) = self.lookup(&key, &epoch) {
                return cached;
            }
            // Cold: resolve ONCE with NO cache lock held (the resolve walks the workspace
            // project resolver + mints a witness — never under the map mutex).
            let admission = Self::resolve(host, source);
            // Cold-path fence: the resolve read LIVE state with the lock released. If the
            // publication identity moved since `epoch` was captured, the decision belongs to
            // a superseded epoch — discard WITHOUT storing and retry at the new epoch.
            if Self::current_epoch(host) == epoch {
                self.store(&key, epoch, admission.clone());
                return admission;
            }
        }
        // Sustained mid-flight churn exhausted the fence retries: return the current
        // decision WITHOUT promoting it warm (a torn/racing result is never cached).
        Self::resolve(host, source)
    }

    /// The host's CURRENT admission epoch, read cheaply (NO resolve): the live published
    /// root RETAINED as the `Arc<PublishedRoot>` identity (`None` before the first publish)
    /// plus the content + monotonic project generations.
    fn current_epoch(host: &Arc<VerterHost>) -> AdmissionEpoch {
        let ws_read = host.workspace_read();
        let published = ws_read.published_root();
        let content_generation = ws_read.content_generation();
        let project_generation = host.project_type_store().current_project_generation();
        AdmissionEpoch {
            published,
            content_generation,
            project_generation,
        }
    }

    /// Resolve the admission decision for `source` through the ONE shared
    /// [`resolve_carrier_bound`] resolver, mapping every non-bound state to its exact
    /// fail-closed [`AdmissionDenial`]. The bound witness is wrapped `Arc` in place (the
    /// resolver's `Box<BoundCarrier>` converts without a re-box).
    fn resolve(host: &Arc<VerterHost>, source: &str) -> Admission {
        match resolve_carrier_bound(host, source) {
            CarrierBinding::Bound(bound) => Admission::Admitted(Arc::from(bound)),
            CarrierBinding::PreSnapshot => Admission::Denied(AdmissionDenial::PreSnapshot),
            CarrierBinding::NoProject => Admission::Denied(AdmissionDenial::NoProject),
            CarrierBinding::Ambiguous(_) => Admission::Denied(AdmissionDenial::Ambiguous),
            CarrierBinding::SyntheticScratch => {
                Admission::Denied(AdmissionDenial::SyntheticScratch)
            }
            CarrierBinding::EnsureFailed => Admission::Denied(AdmissionDenial::EnsureFailed),
        }
    }

    /// A warm-hit lookup: the memoized decision for `key` IFF the cache is anchored at the
    /// SAME `epoch` (the same publication `Arc` identity + content + project generation),
    /// else `None`. A different epoch — including a re-published root at the SAME
    /// `snapshot.generation.0` scalar but a DIFFERENT `Arc` pointer — is a natural miss, so
    /// a stale `Admitted` is never served across a publication.
    fn lookup(&self, key: &CarrierAdmissionKey, epoch: &AdmissionEpoch) -> Option<Admission> {
        let state = self.state.lock().expect("admission cache mutex poisoned");
        if state.epoch.as_ref() != Some(epoch) {
            return None;
        }
        state.entries.get(key).cloned()
    }

    /// Memoize `admission` under `key`, epoch-scoped. A store at a DIFFERENT epoch than the
    /// map's current anchor CLEARS every prior-epoch entry and re-anchors + RETAINS the new
    /// publication `Arc` (no stale `Admitted` survives a publication / generation change —
    /// no cross-epoch privilege bleed); a per-epoch overflow past
    /// [`ADMISSION_CACHE_MAX_ENTRIES`] likewise drops the epoch's entries. Bounded either
    /// way.
    fn store(&self, key: &CarrierAdmissionKey, epoch: AdmissionEpoch, admission: Admission) {
        let mut state = self.state.lock().expect("admission cache mutex poisoned");
        if state.epoch.as_ref() != Some(&epoch) {
            state.entries.clear();
            state.epoch = Some(epoch);
        } else if state.entries.len() >= ADMISSION_CACHE_MAX_ENTRIES {
            state.entries.clear();
        }
        state.entries.insert(key.clone(), admission);
    }
}
