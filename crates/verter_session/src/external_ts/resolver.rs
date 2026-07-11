//! The project-resolution layer of the project-bound external-TS contract.
//!
//! `ExternalTsProjectResolver` maps a source URI to one of the explicit
//! carrier-ownership resolution states. The name is deliberately NOT a bare
//! `ProjectResolver` — `verter_semantic::analysis::project_resolver::ProjectResolver`
//! is re-exported elsewhere and the two must not collide; consumers reach this
//! one as `external_ts::ExternalTsProjectResolver`.
//!
//! The implementation ([`WorkspaceProjectResolver`]) runs the §2.2 / §2.6-step-4
//! carrier-path conflict pass FIRST — UNCONDITIONALLY, before ownership is even
//! consulted — then resolves ownership through `verter_workspace`'s
//! configured-owner resolution (the TS-correct extension model). ANY source
//! (owned, unowned, or multiply-owned) is DOWNGRADED to `Ambiguous` (fail closed)
//! when serving a carrier at its companion path would shadow a real user file, or
//! when a same-stem rune module makes the bare import ambiguous. The conflict is a
//! property of the disk layout, not of tsconfig ownership: Verter NEVER
//! overlay-shadows a real user file, in EVERY owner-resolution state.

use std::sync::Arc;

use verter_workspace::resolver::{
    normalize_canonical_id, path_is_carrier, strip_carrier_extension,
};
use verter_workspace::traits::WorkspaceRead;
use verter_workspace::workspace_snapshot::{
    ConfiguredOwnerResolution, ProjectId, ProjectPayload, SnapshotGeneration, WorkspaceSnapshot,
};

use crate::framework::descriptor::{carrier_companion_identities_for_source, CarrierCompanion};

use super::engine::{EnsureProject, EnvDims};

/// Why a clean owner was downgraded to [`CarrierOwnershipResolution::Ambiguous`] by
/// the carrier-path conflict pass. Recorded so callers / tests can distinguish the
/// model conflicts from a genuine two-project overlap.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AmbiguityCause {
    /// Two (or more) configured projects claim the file with no deterministic
    /// leaf — the `verter_workspace` `ConfiguredOwnerResolution::Ambiguous` case.
    MultipleOwners,
    /// A real user file already occupies the exact carrier-companion path; Verter
    /// must not overlay-shadow it.
    CarrierPathOccupiedByRealFile,
    /// A same-stem rune module (`Foo.svelte.ts` beside `Foo.svelte`) the engine's
    /// extension probe reaches first makes the bare import ambiguous.
    SameStemRuneModule,
}

/// A resolved configured-project binding: the ONLY state in which external-TS
/// results are produced for a carrier source.
///
/// A `ProjectBinding` is the head of the `provider_op_requires_resolved_project`
/// type-state chain: it (and ONLY it) can mint an [`EnsureProject`], which is the
/// sole way to reach the engine's `ensure_project` and obtain the
/// [`BoundProject`](super::engine::BoundProject) witness every production op
/// requires. Its fields are private; it is constructed only inside this module.
///
/// The binding carries the resolved project's identity/generation provenance —
/// `project_id` (its index in the owning snapshot) and `ownership_generation`
/// (the snapshot generation the ownership decision was made at) — alongside the
/// canonical tsconfig / workspace-root / references / env dims.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProjectBinding {
    workspace_root: Arc<str>,
    tsconfig_uri: Arc<str>,
    ts_version: Arc<str>,
    env_dims: EnvDims,
    /// Resolved project references (the reference-graph data the binding carries;
    /// live cross-program publishing is a downstream concern).
    references: Vec<Arc<str>>,
    /// The resolved project's id in the owning snapshot.
    project_id: ProjectId,
    /// The snapshot generation the ownership decision was made at.
    ownership_generation: SnapshotGeneration,
}

impl ProjectBinding {
    /// Crate-internal constructor (resolver-produced only).
    #[allow(clippy::too_many_arguments)]
    pub(super) fn new(
        workspace_root: Arc<str>,
        tsconfig_uri: Arc<str>,
        ts_version: Arc<str>,
        env_dims: EnvDims,
        references: Vec<Arc<str>>,
        project_id: ProjectId,
        ownership_generation: SnapshotGeneration,
    ) -> Self {
        Self {
            workspace_root,
            tsconfig_uri,
            ts_version,
            env_dims,
            references,
            project_id,
            ownership_generation,
        }
    }

    /// The owning tsconfig URI.
    #[must_use]
    pub fn tsconfig_uri(&self) -> &str {
        &self.tsconfig_uri
    }

    /// The owning workspace root.
    #[must_use]
    pub fn workspace_root(&self) -> &str {
        &self.workspace_root
    }

    /// The orthogonal env dimensions for this project.
    #[must_use]
    pub fn env_dims(&self) -> &EnvDims {
        &self.env_dims
    }

    /// Resolved project-reference URIs (reference-graph awareness).
    #[must_use]
    pub fn references(&self) -> &[Arc<str>] {
        &self.references
    }

    /// The resolved project's id in the owning snapshot.
    #[must_use]
    pub fn project_id(&self) -> ProjectId {
        self.project_id
    }

    /// The snapshot generation the ownership decision was made at.
    #[must_use]
    pub fn ownership_generation(&self) -> SnapshotGeneration {
        self.ownership_generation
    }

    /// Mint the [`EnsureProject`] request for this binding — the ONLY way to
    /// obtain one. From here the engine's `ensure_project` yields the
    /// [`BoundProject`](super::engine::BoundProject) witness; without a
    /// `ProjectBinding` there is no `EnsureProject`, hence no production op.
    #[must_use]
    pub fn ensure_project_request(&self) -> EnsureProject {
        EnsureProject::new(
            Arc::clone(&self.workspace_root),
            Arc::clone(&self.tsconfig_uri),
            Arc::clone(&self.ts_version),
            self.env_dims.project_identity,
            self.env_dims,
        )
    }

    /// Construct a binding directly for downstream-crate TESTS that need a resolved
    /// binding without standing up a full `WorkspaceSnapshot` + `WorkspaceRead`
    /// resolver. Gated behind the `test-util` feature (enabled only by downstream
    /// crates' `[dev-dependencies]`), so it is UNAVAILABLE in a normal production
    /// build: production code obtains a binding ONLY from
    /// [`WorkspaceProjectResolver::resolve`] (the resolution gate), preserving the
    /// `provider_op_requires_resolved_project` witness discipline. This is a
    /// test-only seam, not a production path.
    #[cfg(feature = "test-util")]
    #[doc(hidden)]
    #[must_use]
    #[allow(clippy::too_many_arguments)]
    pub fn new_for_test(
        workspace_root: impl Into<Arc<str>>,
        tsconfig_uri: impl Into<Arc<str>>,
        ts_version: impl Into<Arc<str>>,
        env_dims: EnvDims,
        references: Vec<Arc<str>>,
        project_id: ProjectId,
        ownership_generation: SnapshotGeneration,
    ) -> Self {
        Self::new(
            workspace_root.into(),
            tsconfig_uri.into(),
            ts_version.into(),
            env_dims,
            references,
            project_id,
            ownership_generation,
        )
    }
}

/// The synthetic-scratch binding for untitled buffers / files outside any
/// tsconfig. Carries a SEPARATE, clearly-labelled scratch witness usable only
/// for non-cross-file features — never production project semantics, never warms
/// a project cache.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ScratchBinding {
    label: Arc<str>,
}

impl ScratchBinding {
    /// Crate-internal constructor.
    pub(super) fn new(label: Arc<str>) -> Self {
        Self { label }
    }

    /// A human-readable label for this scratch buffer.
    #[must_use]
    pub fn label(&self) -> &str {
        &self.label
    }
}

/// The explicit carrier-ownership resolution states (§2.1). A config-less
/// operation for a production carrier source is NOT a silent fallthrough — it is
/// the `NoProject` / `Ambiguous` arm, which carries no [`ProjectBinding`] and thus
/// no witness. Scratch is NOT a state here: an untitled buffer / file outside any
/// tsconfig resolves through the SEPARATE non-production [`ScratchResolution`]
/// lane, so scratch can never masquerade as a carrier-ownership decision.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CarrierOwnershipResolution {
    /// The published ownership view is not yet authoritative (bootstrap): a
    /// TRANSIENT state, the sole retryable one. NOT a terminal absence — a carrier
    /// queried here re-resolves once the real project graph publishes.
    NotReady,
    /// No owning tsconfig — fail closed (Verter-native non-TS features may still
    /// answer). Terminal.
    NoProject,
    /// A resolved configured project — the only state that produces external-TS
    /// results for a carrier source.
    Bound(ProjectBinding),
    /// Two configs claim the file with no deterministic leaf, OR a carrier-path
    /// conflict — fail closed. Terminal. `candidates` are the candidate tsconfig
    /// URIs (for a `verter(project)` diagnostic); a disk-layout conflict carries
    /// no candidates.
    Ambiguous {
        /// Candidate configured-project tsconfig URIs that overlap on the source
        /// (empty for a disk-layout carrier-path conflict).
        candidates: Vec<Arc<str>>,
        /// Why the source is ambiguous.
        cause: AmbiguityCause,
    },
}

/// The synthetic-scratch resolution lane — SEPARATE from
/// [`CarrierOwnershipResolution`], so scratch is never a production
/// carrier-ownership state. An untitled buffer / a file the caller has decided to
/// serve outside any tsconfig resolves here, carrying only the non-cross-file
/// scratch witness.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ScratchResolution {
    /// An untitled buffer / file outside any tsconfig — served by an explicit
    /// scratch project for non-cross-file features only.
    SyntheticScratch(ScratchBinding),
}

impl ScratchResolution {
    /// Build the scratch resolution for an untitled buffer / a file the caller has
    /// decided to serve outside any tsconfig (e.g. a standalone carrier opened with
    /// no project). This is the ONLY way to mint the scratch state, keeping
    /// `ScratchBinding`'s constructor crate-private. It is NOT production project
    /// semantics — non-cross-file features only.
    #[must_use]
    pub fn synthetic_scratch(label: impl Into<Arc<str>>) -> Self {
        ScratchResolution::SyntheticScratch(ScratchBinding::new(label.into()))
    }
}

/// A generation-stamped editor-binding witness, an OPTIONAL input to
/// [`ExternalTsProjectResolver::resolve`]. It attests that the editor bound a
/// carrier to a project at a given snapshot generation. It is threaded as an
/// optional input and every call site passes `None` (fail-closed); it carries no
/// logic and the resolver does not yet consume it (a validated residual tie MAY
/// later resolve to `Bound`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GenerationStampedEditorWitness {
    editor_bound_tsconfig_uri: Arc<str>,
    generation: SnapshotGeneration,
}

impl GenerationStampedEditorWitness {
    /// Build a generation-stamped editor-binding witness.
    #[must_use]
    pub fn new(
        editor_bound_tsconfig_uri: impl Into<Arc<str>>,
        generation: SnapshotGeneration,
    ) -> Self {
        Self {
            editor_bound_tsconfig_uri: editor_bound_tsconfig_uri.into(),
            generation,
        }
    }

    /// The tsconfig URI the editor bound the carrier to.
    #[must_use]
    pub fn editor_bound_tsconfig_uri(&self) -> &str {
        &self.editor_bound_tsconfig_uri
    }

    /// The snapshot generation this witness was stamped at.
    #[must_use]
    pub fn generation(&self) -> SnapshotGeneration {
        self.generation
    }
}

/// The project-resolution seam.
pub trait ExternalTsProjectResolver {
    /// Resolve `source_uri` to one of the explicit carrier-ownership states.
    ///
    /// `editor_witness` is an OPTIONAL generation-stamped editor-binding input;
    /// every call site passes `None` (fail-closed) and it is not yet consumed.
    fn resolve(
        &self,
        source_uri: &str,
        editor_witness: Option<GenerationStampedEditorWitness>,
    ) -> CarrierOwnershipResolution;
}

/// Supplies the R21 [`EnvDims`] for a resolved project. The host implements this
/// over its env-hash reader (`host_view_env_hashes_for` /
/// `host_view_project_identity_for`); tests supply explicit dims. The resolver
/// NEVER fabricates env identity itself (no default/zero env hashes in
/// production source — that is the per-project-aliasing hazard the
/// `no_default_env_hashes_in_production` guard bans), so env dims are an injected
/// host responsibility carried through the binding.
///
/// The reader is keyed on a MEMBER canonical of the resolved project (the resolved
/// carrier source), NOT the tsconfig path — env hashes are uniform across a
/// project's members, and a tsconfig path is normally outside the membership set,
/// so keying on it reads back the workspace-default fallback.
pub trait ProjectEnvDimsSource {
    /// The orthogonal env dimensions for the configured project that owns
    /// `project_member_canonical` (a resolved source member of the project).
    fn env_dims_for(&self, project_member_canonical: &str) -> EnvDims;
}

impl<F> ProjectEnvDimsSource for F
where
    F: Fn(&str) -> EnvDims,
{
    fn env_dims_for(&self, project_member_canonical: &str) -> EnvDims {
        self(project_member_canonical)
    }
}

/// The resolver: configured-owner resolution + carrier-path conflict
/// pass, over a [`WorkspaceSnapshot`] and a [`WorkspaceRead`] (for disk probes).
pub struct WorkspaceProjectResolver<'a> {
    snapshot: &'a WorkspaceSnapshot,
    workspace: &'a dyn WorkspaceRead,
    ts_version: Arc<str>,
    env_dims: &'a dyn ProjectEnvDimsSource,
    /// Whether the published ownership view this resolver reads is authoritative.
    /// `false` during bootstrap (empty/pre-graph publication) ⇒ [`resolve`] yields
    /// [`CarrierOwnershipResolution::NotReady`] rather than a premature `NoProject`.
    /// Sourced from the SAME `PublishedRoot::ownership_ready` the snapshot came
    /// from; a resolver built directly over a standalone snapshot (tests) passes
    /// `true`.
    ///
    /// [`resolve`]: ExternalTsProjectResolver::resolve
    ownership_ready: bool,
}

impl<'a> WorkspaceProjectResolver<'a> {
    /// Build a resolver over a snapshot + workspace read view + an env-dims
    /// source. The env-dims source is the host's per-project R21 env-hash reader
    /// (or, in tests, an explicit-value closure); the resolver does not compute
    /// or default env identity. `ownership_ready` is the snapshot's published
    /// authoritative-vs-bootstrap signal (`PublishedRoot::ownership_ready`).
    pub fn new(
        snapshot: &'a WorkspaceSnapshot,
        workspace: &'a dyn WorkspaceRead,
        ts_version: impl Into<Arc<str>>,
        env_dims: &'a dyn ProjectEnvDimsSource,
        ownership_ready: bool,
    ) -> Self {
        Self {
            snapshot,
            workspace,
            ts_version: ts_version.into(),
            env_dims,
            ownership_ready,
        }
    }

    /// Build the `ProjectBinding` for a resolved configured project id, reading
    /// its tsconfig / workspace-root / references off the snapshot and its R21
    /// env dims from the injected source. Returns `None` if the id is somehow a
    /// fallback (it never is for a configured owner, but the match is exhaustive
    /// and fail-closed).
    ///
    /// `source_uri` is the RESOLVED carrier source — a real member of this
    /// project — and is the canonical the env-dims source reads the project's R21
    /// dims from. It is NOT the tsconfig PATH: the tsconfig file is normally
    /// OUTSIDE the project's `ConfiguredMembership`, so a host env-hash reader
    /// keyed on it resolves to no owner and falls back to workspace-default dims
    /// (which would alias every project's cache dimensions to one bundle). An
    /// owned member canonical resolves to the project and yields its real
    /// per-project env identity.
    fn binding_for(
        &self,
        id: verter_workspace::workspace_snapshot::ProjectId,
        source_uri: &str,
    ) -> Option<ProjectBinding> {
        let project = self.snapshot.project(id);
        match &project.payload {
            ProjectPayload::Configured {
                tsconfig_path,
                references,
                ..
            } => {
                let references: Vec<Arc<str>> = references
                    .iter()
                    .map(|r| Arc::<str>::from(r.as_str()))
                    .collect();
                let tsconfig_uri = Arc::<str>::from(tsconfig_path.as_str());
                // Source the R21 env dims from the resolved project via one of its
                // MEMBER canonicals (this resolved source), NOT the tsconfig PATH —
                // see the method doc: a tsconfig path is not a membership member and
                // reads back workspace-default dims.
                let env_dims = self.env_dims.env_dims_for(source_uri);
                Some(ProjectBinding::new(
                    Arc::<str>::from(project.workspace_root.as_str()),
                    tsconfig_uri,
                    Arc::clone(&self.ts_version),
                    env_dims,
                    references,
                    id,
                    self.snapshot.generation,
                ))
            }
            ProjectPayload::Fallback { .. } => None,
        }
    }

    /// Map ambiguous configured-project ids to their candidate tsconfig URIs, so a
    /// later `verter(project)` diagnostic can list the configs that overlap on the
    /// source. A fallback id (never a configured owner) contributes nothing.
    fn candidate_tsconfig_uris(&self, ids: &[ProjectId]) -> Vec<Arc<str>> {
        ids.iter()
            .filter_map(|id| match &self.snapshot.project(*id).payload {
                ProjectPayload::Configured { tsconfig_path, .. } => {
                    Some(Arc::<str>::from(tsconfig_path.as_str()))
                }
                ProjectPayload::Fallback { .. } => None,
            })
            .collect()
    }

    /// The §2.2 / §2.6-step-4 carrier-path conflict pass. Given ANY carrier
    /// `source_uri` (regardless of its owner-resolution state), return
    /// `Some(cause)` if the source must be downgraded to `Ambiguous` (fail closed)
    /// because a real user file occupies a companion path or a same-stem rune module
    /// sits beside it, else `None`. Consulted unconditionally by [`Self::resolve`]
    /// so the real-file-shadow gate can never be bypassed by an unowned / ambiguous
    /// owner state.
    ///
    /// The caller-supplied `source_uri` is NORMALIZED at the entry (mirroring the
    /// rest of `verter_workspace::resolver`) so a non-canonical URI (uppercase
    /// drive / backslashes) cannot bypass the disk-occupancy / same-stem probes
    /// on a case-insensitive FS — the fail-closed gate must never be evadable by
    /// a non-canonical caller URI.
    fn carrier_path_conflict(&self, source_uri: &str) -> Option<AmbiguityCause> {
        let source_uri = normalize_canonical_id(source_uri);
        let source_uri = source_uri.as_str();

        // Only a carrier SOURCE can have a companion-path / rune conflict.
        if !path_is_carrier(source_uri) {
            return None;
        }

        // (a) A real user file at ANY descriptor-owned carrier-companion path ⇒ never
        // overlay-shadow it. The companion paths are DERIVED from the adapter's
        // `VirtualFileNaming` authority across EVERY family — the IDE carrier (Vue's
        // `JsxConditional` yields BOTH `{name}.vue.tsx` AND `{name}.vue.jsx`, Svelte's
        // `Suffix(".tsx")` yields `{name}.svelte.tsx`), the extension-middle declaration
        // carrier (`{name}.d.vue.ts`), the `.verter.ts` import-surface API, the
        // testing-API, and any sidecar. The script kind / JSX-ness of the source is not
        // known at ownership time, so a real file at ANY occupiable companion path is a
        // shadow conflict. Never a hardcoded suffix list in the resolver.
        for companion in self.descriptor_carrier_companion_paths(source_uri) {
            if self.workspace.file_exists(&companion.path) {
                return Some(AmbiguityCause::CarrierPathOccupiedByRealFile);
            }
        }

        // (b) A same-stem rune module beside the component (`Foo.svelte.ts`
        // beside `Foo.svelte`) the engine probes first ⇒ ambiguous. Rune
        // extensions are sourced from the registry (`all_adapter_module_extensions`)
        // and are matched to the SAME carrier family by prefix, so a `.vue`
        // source (no `vue.*` rune family) never trips this.
        let stem = strip_carrier_extension(source_uri);
        let carrier_ext = source_uri
            .strip_prefix(stem)
            .and_then(|rest| rest.strip_prefix('.'));
        if let Some(carrier_ext) = carrier_ext {
            let family_prefix = format!("{carrier_ext}.");
            for rune_ext in
                verter_language::LanguageRegistry::global().all_adapter_module_extensions()
            {
                if !rune_ext.starts_with(&family_prefix) {
                    continue;
                }
                let rune_candidate = format!("{stem}.{rune_ext}");
                if self.workspace.file_exists(&rune_candidate) {
                    return Some(AmbiguityCause::SameStemRuneModule);
                }
            }
        }

        None
    }

    /// Every descriptor-valid carrier-companion IDENTITY for a carrier `source_uri`,
    /// across ALL companion families (IDE, extension-middle declaration,
    /// import-surface API, testing-API, sidecar), derived from the owning adapter's
    /// `VirtualFileNaming` authority through the registry-level
    /// [`crate::framework::descriptor::carrier_companion_identities_for_source`] (NOT a
    /// hardcoded suffix list). A real user file at ANY of these occupiable paths is a
    /// shadow conflict. Framework-agnostic: a new adapter participates the moment its
    /// descriptor is registered, with no per-adapter branch here.
    fn descriptor_carrier_companion_paths(&self, source_uri: &str) -> Vec<CarrierCompanion> {
        carrier_companion_identities_for_source(source_uri)
    }
}

impl ExternalTsProjectResolver for WorkspaceProjectResolver<'_> {
    fn resolve(
        &self,
        source_uri: &str,
        editor_witness: Option<GenerationStampedEditorWitness>,
    ) -> CarrierOwnershipResolution {
        // The generation-stamped editor witness is an OPTIONAL input that is not yet
        // consumed: a validated residual tie MAY later resolve to `Bound`.
        // Fail-closed — a present witness never widens the result here.
        let _ = editor_witness;

        // A non-authoritative (bootstrap) published ownership view is TRANSIENT, not
        // a terminal absence. Return `NotReady` — the sole retryable state — instead
        // of a premature `NoProject`, so a carrier queried before the real project
        // graph publishes re-resolves once ownership is authoritative.
        if !self.ownership_ready {
            return CarrierOwnershipResolution::NotReady;
        }

        // The carrier-path conflict pass is a property of the DISK LAYOUT (a real
        // user file at a companion path, or a same-stem rune module beside the
        // source) — INDEPENDENT of tsconfig ownership. "Verter NEVER overlay-shadows
        // a real user file" holds for EVERY owner-resolution state, so the conflict
        // pass runs FIRST, UNCONDITIONALLY: a real file at the companion path
        // downgrades the source to `Ambiguous` whether it is owned (`Unique`),
        // unowned (`None`), or multiply-owned (`Ambiguous(MultipleOwners)`) — never
        // only the clean-owner arm. `CarrierPathOccupiedByRealFile` correctly takes
        // precedence over a `MultipleOwners` overlap: the safety-critical shadow
        // conflict is checked before ownership is even consulted. (A non-carrier
        // source short-circuits to `None` inside `carrier_path_conflict`, so this is
        // a no-op for plain `.ts`/`.tsx` sources.)
        if let Some(cause) = self.carrier_path_conflict(source_uri) {
            // A disk-layout conflict carries no candidate configs — it is a property
            // of the file layout, not of tsconfig overlap.
            return CarrierOwnershipResolution::Ambiguous {
                candidates: Vec::new(),
                cause,
            };
        }
        match self
            .snapshot
            .configured_owner_resolution_for_file(source_uri)
        {
            ConfiguredOwnerResolution::None => CarrierOwnershipResolution::NoProject,
            ConfiguredOwnerResolution::Ambiguous(ids) => CarrierOwnershipResolution::Ambiguous {
                candidates: self.candidate_tsconfig_uris(&ids),
                cause: AmbiguityCause::MultipleOwners,
            },
            ConfiguredOwnerResolution::Unique(id) => match self.binding_for(id, source_uri) {
                Some(binding) => CarrierOwnershipResolution::Bound(binding),
                // A configured owner that is somehow not Configured: fail closed
                // rather than fabricate a binding.
                None => CarrierOwnershipResolution::NoProject,
            },
        }
    }
}
