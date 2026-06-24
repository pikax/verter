//! The project-resolution layer of the project-bound external-TS contract.
//!
//! `ExternalTsProjectResolver` maps a source URI to one of the four explicit
//! project-resolution states. The name is deliberately NOT a bare
//! `ProjectResolver` — `verter_semantic::analysis::project_resolver::ProjectResolver`
//! is re-exported elsewhere and the two must not collide; consumers reach this
//! one as `external_ts::ExternalTsProjectResolver`.
//!
//! The implementation ([`WorkspaceProjectResolver`]) resolves ownership
//! through `verter_workspace`'s configured-owner resolution (the TS-correct
//! extension model) and then runs the §2.2 / §2.6-step-4 carrier-path conflict
//! pass: a clean owner is DOWNGRADED to `Ambiguous` (fail closed) when serving a
//! carrier at its companion path would shadow a real user file, or when a
//! same-stem rune module makes the bare import ambiguous. Verter NEVER
//! overlay-shadows a real user file.

use std::sync::Arc;

use verter_workspace::resolver::{
    normalize_canonical_id, path_is_carrier, strip_carrier_extension,
};
use verter_workspace::traits::WorkspaceRead;
use verter_workspace::workspace_snapshot::{
    ConfiguredOwnerResolution, ProjectPayload, WorkspaceSnapshot,
};

use crate::framework::descriptor::built_in_descriptors;

use super::engine::{EnsureProject, EnvDims};

/// Why a clean owner was downgraded to [`ProjectResolution::Ambiguous`] by the
/// carrier-path conflict pass. Recorded so callers / tests can distinguish the
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
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProjectBinding {
    workspace_root: Arc<str>,
    tsconfig_uri: Arc<str>,
    ts_version: Arc<str>,
    env_dims: EnvDims,
    /// Resolved project references (the reference-graph data the binding carries;
    /// live cross-program publishing is a downstream concern).
    references: Vec<Arc<str>>,
}

impl ProjectBinding {
    /// Crate-internal constructor (resolver-produced only).
    pub(super) fn new(
        workspace_root: Arc<str>,
        tsconfig_uri: Arc<str>,
        ts_version: Arc<str>,
        env_dims: EnvDims,
        references: Vec<Arc<str>>,
    ) -> Self {
        Self {
            workspace_root,
            tsconfig_uri,
            ts_version,
            env_dims,
            references,
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

/// The four explicit project-resolution states (§2.1). A config-less operation
/// for a production carrier source is NOT a silent fallthrough — it is the
/// `NoProject` / `Ambiguous` arm, which carries no [`ProjectBinding`] and thus no
/// witness.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ProjectResolution {
    /// A resolved configured project — the only state that produces external-TS
    /// results for a carrier source.
    ProjectBinding(ProjectBinding),
    /// No owning tsconfig — fail closed (Verter-native non-TS features may still
    /// answer).
    NoProject,
    /// Two configs claim the file with no deterministic leaf, OR a carrier-path
    /// conflict — fail closed.
    Ambiguous(AmbiguityCause),
    /// An untitled buffer / file outside any tsconfig — served by an explicit
    /// scratch project for non-cross-file features only.
    SyntheticScratch(ScratchBinding),
}

impl ProjectResolution {
    /// Build the `SyntheticScratch` state for an untitled buffer / a file the
    /// caller has decided to serve outside any tsconfig (e.g. a standalone
    /// carrier opened with no project). This is the ONLY way to mint the
    /// scratch state, keeping `ScratchBinding`'s constructor crate-private. It
    /// is NOT production project semantics — non-cross-file features only.
    #[must_use]
    pub fn synthetic_scratch(label: impl Into<Arc<str>>) -> Self {
        ProjectResolution::SyntheticScratch(ScratchBinding::new(label.into()))
    }
}

/// The project-resolution seam.
pub trait ExternalTsProjectResolver {
    /// Resolve `source_uri` to one of the four explicit states.
    fn resolve(&self, source_uri: &str) -> ProjectResolution;
}

/// Supplies the R21 [`EnvDims`] for a resolved project. The host implements this
/// over its env-hash reader (`host_view_env_hashes_for` /
/// `host_view_project_identity_for`); tests supply explicit dims. The resolver
/// NEVER fabricates env identity itself (no default/zero env hashes in
/// production source — that is the per-project-aliasing hazard the
/// `no_default_env_hashes_in_production` guard bans), so env dims are an injected
/// host responsibility carried through the binding.
pub trait ProjectEnvDimsSource {
    /// The orthogonal env dimensions for the configured project at `tsconfig_uri`.
    fn env_dims_for(&self, tsconfig_uri: &str) -> EnvDims;
}

impl<F> ProjectEnvDimsSource for F
where
    F: Fn(&str) -> EnvDims,
{
    fn env_dims_for(&self, tsconfig_uri: &str) -> EnvDims {
        self(tsconfig_uri)
    }
}

/// The resolver: configured-owner resolution + carrier-path conflict
/// pass, over a [`WorkspaceSnapshot`] and a [`WorkspaceRead`] (for disk probes).
pub struct WorkspaceProjectResolver<'a> {
    snapshot: &'a WorkspaceSnapshot,
    workspace: &'a dyn WorkspaceRead,
    ts_version: Arc<str>,
    env_dims: &'a dyn ProjectEnvDimsSource,
}

impl<'a> WorkspaceProjectResolver<'a> {
    /// Build a resolver over a snapshot + workspace read view + an env-dims
    /// source. The env-dims source is the host's per-project R21 env-hash reader
    /// (or, in tests, an explicit-value closure); the resolver does not compute
    /// or default env identity.
    pub fn new(
        snapshot: &'a WorkspaceSnapshot,
        workspace: &'a dyn WorkspaceRead,
        ts_version: impl Into<Arc<str>>,
        env_dims: &'a dyn ProjectEnvDimsSource,
    ) -> Self {
        Self {
            snapshot,
            workspace,
            ts_version: ts_version.into(),
            env_dims,
        }
    }

    /// Build the `ProjectBinding` for a resolved configured project id, reading
    /// its tsconfig / workspace-root / references off the snapshot and its R21
    /// env dims from the injected source. Returns `None` if the id is somehow a
    /// fallback (it never is for a configured owner, but the match is exhaustive
    /// and fail-closed).
    fn binding_for(
        &self,
        id: verter_workspace::workspace_snapshot::ProjectId,
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
                let env_dims = self.env_dims.env_dims_for(&tsconfig_uri);
                Some(ProjectBinding::new(
                    Arc::<str>::from(project.workspace_root.as_str()),
                    tsconfig_uri,
                    Arc::clone(&self.ts_version),
                    env_dims,
                    references,
                ))
            }
            ProjectPayload::Fallback { .. } => None,
        }
    }

    /// The §2.2 / §2.6-step-4 carrier-path conflict pass. Given a `source_uri`
    /// that resolved to a clean owner, return `Some(cause)` if the source must be
    /// downgraded to `Ambiguous` (fail closed), else `None`.
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

        // (a) A real user file at ANY descriptor-valid IDE carrier-companion path
        // ⇒ never overlay-shadow it. The companion path(s) are DERIVED from the
        // adapter's `VirtualFileNaming` authority — Vue's `JsxConditional` yields
        // BOTH `{name}.vue.tsx` AND `{name}.vue.jsx`, Svelte's `Suffix(".tsx")`
        // yields `{name}.svelte.tsx`. The script kind / JSX-ness of the source is
        // not known at ownership time, so a real file at ANY of them is a shadow
        // conflict. Never a hardcoded suffix list in the resolver.
        for carrier_path in self.descriptor_ide_carrier_paths(source_uri) {
            if self.workspace.file_exists(&carrier_path) {
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

    /// Every descriptor-valid IDE carrier-companion PATH for a carrier
    /// `source_uri`, derived from the owning adapter's `VirtualFileNaming`
    /// authority (NOT a hardcoded `.tsx`). The source is classified to its
    /// carrier language via the shared `LanguageRegistry`, matched to its
    /// `FrameworkAdapterDescriptor`, and each `ide_carrier_suffixes()` entry is
    /// appended to the full carrier canonical (`Foo.vue` + `.jsx` →
    /// `Foo.vue.jsx`). Framework-agnostic: a new adapter participates the moment
    /// its descriptor is registered, with no per-adapter branch here.
    fn descriptor_ide_carrier_paths(&self, source_uri: &str) -> Vec<String> {
        use verter_language::StaticClassification;

        let registry = verter_language::LanguageRegistry::global();
        // A carrier extension (`.vue`/`.svelte`) is a STATIC carrier row, so it
        // classifies to `Resolved(Framework { .. })`; the carrier language id is
        // the descriptor-match key.
        let StaticClassification::Resolved(language) = registry.classify_static(source_uri) else {
            return Vec::new();
        };
        let Some(carrier_language_id) = language.carrier_language_id().cloned() else {
            return Vec::new();
        };

        built_in_descriptors()
            .iter()
            .filter(|descriptor| descriptor.carrier_language.as_ref() == Some(&carrier_language_id))
            .filter_map(|descriptor| descriptor.virtual_file_naming.as_ref())
            .flat_map(|naming| naming.ide_carrier_suffixes())
            .map(|suffix| format!("{source_uri}{suffix}"))
            .collect()
    }
}

impl ExternalTsProjectResolver for WorkspaceProjectResolver<'_> {
    fn resolve(&self, source_uri: &str) -> ProjectResolution {
        match self
            .snapshot
            .configured_owner_resolution_for_file(source_uri)
        {
            ConfiguredOwnerResolution::None => ProjectResolution::NoProject,
            ConfiguredOwnerResolution::Ambiguous(_) => {
                ProjectResolution::Ambiguous(AmbiguityCause::MultipleOwners)
            }
            ConfiguredOwnerResolution::Unique(id) => {
                // Even a clean owner fails closed on a carrier-path conflict.
                if let Some(cause) = self.carrier_path_conflict(source_uri) {
                    return ProjectResolution::Ambiguous(cause);
                }
                match self.binding_for(id) {
                    Some(binding) => ProjectResolution::ProjectBinding(binding),
                    // A configured owner that is somehow not Configured: fail
                    // closed rather than fabricate a binding.
                    None => ProjectResolution::NoProject,
                }
            }
        }
    }
}
