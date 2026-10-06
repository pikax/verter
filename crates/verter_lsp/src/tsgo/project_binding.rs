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

use std::sync::Arc;

use verter_session::external_ts::{
    AmbiguityCause, BoundProject, CarrierOwnershipResolution, EngineBackend, EnvDims,
    ExternalTsProjectResolver, ProjectBinding, WorkspaceProjectResolver,
};
use verter_session::VerterHost;
use verter_session_query::resolution::normalize_canonical_id;
use verter_workspace::published_state::PublishedRoot;
use verter_workspace::{
    decide_generated_unit_admission_with_basis, CanonicalPath, GeneratedUnitAdmission,
};

use crate::external_ts::TsgoEngineBackend;
use verter_type_runtime::provider_hub::{ProjectBasis, ProjectBindingInput};

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
///
/// It also RETAINS the exact workspace snapshot the binding was resolved over, so the
/// generated-unit admission a SHARED write needs ([`Self::admit_generated_units`]) is
/// decided against the SAME membership that decided ownership — never a later
/// publication. A cached `BoundCarrier` lives exactly one admission epoch, so a changed
/// `include`/`files`/`exclude` (a new publication) can never be answered from it.
pub struct BoundCarrier {
    bound: BoundProject,
    binding: ProjectBinding,
    generation: u64,
    basis: ResolvedPublication,
}

impl std::fmt::Debug for BoundCarrier {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        // The retained snapshot is the whole project graph — identify it, never dump it.
        f.debug_struct("BoundCarrier")
            .field("bound", &self.bound)
            .field("binding", &self.binding)
            .field("generation", &self.generation)
            .finish_non_exhaustive()
    }
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

    /// Decide whether EVERY generated unit of `units` is admitted to this carrier's
    /// owning configured project, over the snapshot the binding was resolved at.
    ///
    /// A unit is a member of the project that admits its membership BASIS — the
    /// carrier source it projects from (an extension-specific `src/**/*.vue`
    /// include owns `Foo.vue` and thereby admits its `Foo.vue.tsx`), or the unit
    /// itself when it derives from no carrier — so this remains a separate proof
    /// a write into an engine Verter does not own must hold, never a fact the
    /// binding alone carries. The membership authority is the workspace's;
    /// nothing here re-derives it.
    #[must_use]
    pub fn admit_generated_units(&self, units: &[CanonicalPath]) -> GeneratedUnitAdmission {
        decide_generated_unit_admission_with_basis(
            self.basis.published.snapshot.as_ref(),
            &CanonicalPath::new(self.binding.tsconfig_uri()),
            units,
            crate::external_ts::carrier_membership_basis,
        )
    }

    /// Retain the publication that supplied both ownership and membership.
    #[must_use]
    pub fn published(&self) -> &Arc<PublishedRoot> {
        &self.basis.published
    }
}

/// The exact publication and generations observed with one resolver answer.
/// A later caller must not re-read the generations and attach them to an old
/// project binding.
#[derive(Clone)]
pub struct ResolvedPublication {
    pub published: Arc<PublishedRoot>,
    pub content_generation: u64,
    pub project_generation: u64,
}

impl ResolvedPublication {
    #[must_use]
    pub fn current(host: &VerterHost) -> Option<Self> {
        let ws_read = host.workspace_read();
        Some(Self {
            published: ws_read.published_root()?,
            content_generation: ws_read.content_generation(),
            project_generation: host.project_type_store().current_project_generation(),
        })
    }
}

impl PartialEq for ResolvedPublication {
    fn eq(&self, other: &Self) -> bool {
        Arc::ptr_eq(&self.published, &other.published)
            && self.content_generation == other.content_generation
            && self.project_generation == other.project_generation
    }
}

impl Eq for ResolvedPublication {}

/// Project resolver facts translated into the hub's provider-neutral basis.
/// The live reader fences publication replacement and content/project drift at
/// bind, request admission, and the actor's provider-visible write point.
pub fn hub_binding_input(
    host: &Arc<VerterHost>,
    source: &str,
    binding: &ProjectBinding,
    resolved: ResolvedPublication,
) -> ProjectBindingInput {
    let basis = ProjectBasis::new(
        resolved.published,
        resolved.content_generation,
        resolved.project_generation,
    );
    let host = Arc::clone(host);
    let current = Arc::new(move || {
        let current = ResolvedPublication::current(&host)?;
        Some(ProjectBasis::new(
            current.published,
            current.content_generation,
            current.project_generation,
        ))
    });
    ProjectBindingInput::new(
        normalize_canonical_id(source),
        binding.tsconfig_uri().to_string(),
        binding.references().iter().map(|r| r.to_string()).collect(),
        basis,
        current,
    )
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
    resolve_carrier_over_snapshot(host, source, ts_version, readiness_mode)
        .map(|(resolution, generation, _)| (resolution, generation))
}

/// The same resolver result with its exact publication retained for hub
/// admission. A later publication with a repeated scalar generation is a
/// distinct basis and cannot reuse this binding.
#[must_use]
pub fn resolve_carrier_with_publication(
    host: &VerterHost,
    source: &str,
    ts_version: Arc<str>,
    readiness_mode: OwnershipReadinessMode,
) -> Option<(CarrierOwnershipResolution, u64, ResolvedPublication)> {
    resolve_carrier_over_snapshot(host, source, ts_version, readiness_mode)
}

/// [`resolve_carrier`] plus the exact workspace snapshot the resolution was decided over,
/// for a caller that must decide a FURTHER membership fact against the same publication.
fn resolve_carrier_over_snapshot(
    host: &VerterHost,
    source: &str,
    ts_version: Arc<str>,
    readiness_mode: OwnershipReadinessMode,
) -> Option<(CarrierOwnershipResolution, u64, ResolvedPublication)> {
    let ws_read = host.workspace_read();
    let published = ws_read.published_root()?;
    let generation = published.snapshot.generation.0;
    let basis = ResolvedPublication {
        published: Arc::clone(&published),
        content_generation: ws_read.content_generation(),
        project_generation: host.project_type_store().current_project_generation(),
    };
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
    Some((resolver.resolve(source, None), generation, basis))
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
    let Some((resolution, generation, basis)) = resolve_carrier_over_snapshot(
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
                    basis,
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

/// An admission EPOCH: the UNREPEATABLE publication identity a carrier's
/// generated-unit admission decisions are scoped to. Admission is an AUTHZ
/// surface — a decision recorded at one epoch MUST NOT authorize a write at a
/// later epoch — so the epoch combines all of:
///
/// * `published` — the EXACT published-root publication (`None` before the
///   first publish). Identity is the RETAINED `Arc<PublishedRoot>` POINTER
///   (`Arc::ptr_eq`), NEVER the `snapshot.generation.0` scalar:
///   `ProjectGraph::from_configs` hard-codes every rebuilt graph to generation
///   1, so a reconfigure republishes generation 1 and the scalar REPEATS. Two
///   distinct publications carrying the same scalar are DISTINCT epochs
///   because their Arc pointers differ, and a retained Arc cannot be
///   freed-and-reused underneath a live holder (the ABA guard).
/// * `content_generation` — the workspace file-existence / content generation.
/// * `project_generation` — the host's MONOTONIC `current_project_generation()`,
///   which advances on a host-mediated reconfigure (never on a content edit).
///
/// Epoch equality requires ALL THREE to match — the publication by POINTER
/// identity, the two generations by value. This is the sweep-generation key of
/// the shared overlay's per-unit decision caches (a changed publication, file
/// set, or owner re-decides every unit); the WRITE AUTHORITY itself is the
/// hub-issued admission bound to this basis.
pub struct AdmissionEpoch {
    pub(super) published: Option<Arc<PublishedRoot>>,
    content_generation: u64,
    project_generation: u64,
}

impl AdmissionEpoch {
    /// The host's CURRENT admission epoch, read cheaply (NO resolve): the live
    /// published root RETAINED as the `Arc<PublishedRoot>` identity (`None`
    /// before the first publish) plus the content + monotonic project
    /// generations.
    pub fn current(host: &Arc<VerterHost>) -> Self {
        let ws_read = host.workspace_read();
        let published = ws_read.published_root();
        let content_generation = ws_read.content_generation();
        let project_generation = host.project_type_store().current_project_generation();
        Self {
            published,
            content_generation,
            project_generation,
        }
    }
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

impl Eq for AdmissionEpoch {}
