//! VFS-driven project-bound sync seam for the external-TypeScript-engine path
//! (§2.5 / §2.7 / C12 of `docs/arch/external-ts-engine-architecture.md`).
//!
//! This seam migrates path-centric provider sync to **project-bound snapshot
//! publishing**. It is a PURE planning/gating layer (no live engine — unit-testable
//! without a provider): it builds the publish/gating/dedupe/index data from the
//! project-bound contract DTOs (`verter_session::external_ts`) and the single
//! [`ProviderSurfaceStore`], leaving the per-engine native mechanics to the backend
//! that consumes them.
//!
//! Five pieces, all framework-agnostic (Vue AND Svelte), all driven from typed
//! data (never source slicing / string heuristics):
//!
//! 1. **[`ProjectSyncBatch`] / [`plan_publish_for_resolution`]** — the §2.5 sync
//!    planner: ownership resolution → per-project **atomic delta batch** keyed by
//!    the owning tsconfig URI. A `NoProject`/`Ambiguous`/`SyntheticScratch`
//!    resolution yields no batch (no-owner ⇒ no external-TS publish; fail closed).
//! 2. **[`SpanClass`] / [`classify_provider_span`]** — span classification:
//!    every returned span is `SourceMappable` (mapped back to the carrier source),
//!    `GeneratedOnly` (a synthetic helper region — **SUPPRESSED**, never escapes),
//!    or `External` (a real on-disk `.ts` — returned as-is). Built on the
//!    fail-closed [`ProviderPositionMapper::tsx_to_carrier`] (None ⇒ no source
//!    correlation).
//! 3. **[`RequestEpoch`]** — multi-file-aware version gating: the engine/project
//!    epoch (`(generation, content_hash, map_hash)` per touched provider file) is
//!    captured **before AND after** each request; the result is dropped if ANY
//!    touched provider file or map changed mid-flight (not just the queried file —
//!    rename/find-references/definition return multi-file spans).
//! 4. **[`QueryDedupeRegistry`]** — query de-dupe + shared cancellation keyed by
//!    `(project, provider_uri, carrier_offset, feature, content_hash, map_hash,
//!    required_version, feature_param)`, wired to a shared [`CancellationToken`] so
//!    a supersession cancels the in-flight engine work (not just response-dropping).
//!    It elides the duplicate engine query for an identical in-flight key; result
//!    delivery to joiners is the backend's concern (added when a backend consumes it).
//! 5. **[`EagerApiIndexPlan`]** — the eager `CarrierApi` index: the public-API
//!    surfaces for ALL project-owned carrier sources are generated + import-indexed
//!    up front so a CLOSED component appears in auto-import / find-refs; full IDE
//!    TSX (`CarrierIde`) stays lazy for open/queried carriers.

use std::sync::Arc;

use dashmap::DashMap;
use verter_span::TsPosition;

use verter_scheduler::cancellation::CancellationToken;
use verter_session::external_ts::{
    ProjectBinding, ProjectResolution, PublishSnapshot, QueryFeature, ScriptKind, SnapshotFile,
    SnapshotRole,
};

use verter_semantic::analysis::types::Hash16;

use crate::documents::provider_projection::ProviderPositionMapper;
use crate::provider_surface_store::ProviderSurfaceStore;

// ── span classification ─────────────────────────────────────────────────

/// How a returned provider span/edit is classified for the user-facing result
/// (§2.7 C12). The classification decides whether the span maps back to the
/// carrier source, is suppressed, or is returned as-is.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SpanClass {
    /// The provider span maps back to a user-source span in the carrier
    /// (`.vue`/`.svelte`). Returned, mapped through the carrier's source map.
    SourceMappable,
    /// The provider span is inside a synthetic Verter-generated helper region
    /// of a carrier companion with NO user-source correlate (or it straddles the
    /// generated/source boundary). **SUPPRESSED** — it never escapes to the user.
    GeneratedOnly,
    /// The provider span is on a genuine on-disk `.ts`/`.tsx` (not a Verter
    /// carrier companion). Returned AS-IS — it is a real file, not a synthetic
    /// region.
    External,
}

impl SpanClass {
    /// Whether a span of this class is SUPPRESSED (dropped before reaching the
    /// user). Only [`GeneratedOnly`](Self::GeneratedOnly) is suppressed — a
    /// synthetic helper region has no faithful user-source location, so surfacing
    /// it would land the user on generated scaffolding.
    #[must_use]
    pub fn is_suppressed(self) -> bool {
        matches!(self, SpanClass::GeneratedOnly)
    }
}

/// The typed ownership of the provider file a returned span belongs to — the
/// STRUCTURAL discriminant span classification keys on (NEVER a path-substring
/// heuristic). It is the contract's [`SnapshotRole`] as recorded for the file in
/// the single [`ProviderSurfaceStore`]: a `Carrier*` role is a Verter-generated
/// companion (whose synthetic regions must be suppressed), while `Real` is a
/// genuine on-disk `.ts`/`.tsx` (returned as-is). `Shadow` is a self-file
/// rune-module surface — a Verter-served companion buffer, so it is treated as a
/// companion (its prelude region must be suppressed, its source region mapped).
///
/// The caller obtains this from the store (`ProviderSurfaceSnapshot::kind` for the
/// returned provider path), so the companion-vs-real decision is a construction-
/// time FACT, not a string sniff at classification time.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SpanSubjectKind {
    /// A Verter-generated carrier companion (`CarrierIde`/`CarrierApi`/`CarrierBatch`)
    /// or a self-file rune-module surface (`Shadow`). Synthetic regions suppress;
    /// mapped regions map back.
    Companion,
    /// A genuine on-disk `.ts`/`.tsx` (`Real`). Returned as-is, never suppressed.
    External,
}

impl SpanSubjectKind {
    /// Derive the typed subject kind from the contract [`SnapshotRole`] the store
    /// recorded for the returned provider file. This is the SOLE companion-vs-real
    /// classification authority — structural, no path inspection.
    #[must_use]
    pub fn from_role(role: SnapshotRole) -> Self {
        match role {
            SnapshotRole::CarrierIde
            | SnapshotRole::CarrierApi
            | SnapshotRole::CarrierBatch
            | SnapshotRole::Shadow => SpanSubjectKind::Companion,
            SnapshotRole::Real => SpanSubjectKind::External,
        }
    }
}

/// A minimal projection of the source↔provider mapper used by span classification.
///
/// Implemented for the real [`ProviderPositionMapper`] (whose `tsx_to_carrier` /
/// `tsx_range_to_carrier` return `None` for synthetic / prelude / rewritten regions
/// — the fail-closed primitive C12 rides on) and by a test stub. The classifier
/// never reads source text or matches on path strings — companion-vs-real is the
/// typed [`SpanSubjectKind`], and mapped-vs-synthetic is this mapper's verdict.
pub trait SpanMapperView {
    /// Whether the provider-buffer RANGE `[(start_line,start_char), (end_line,
    /// end_char))` maps back to a user-source range AS A WHOLE. `false` ⇒ at least
    /// one endpoint (or the span between them) has no faithful source correlation
    /// (a synthetic region, or a span straddling the generated/source boundary).
    /// Range-based — a TypeScript result is a span/edit, not a point, so a span
    /// whose start maps but whose end crosses into generated territory must NOT
    /// classify as mappable.
    fn provider_range_maps_to_source(
        &self,
        start_line: u32,
        start_char: u32,
        end_line: u32,
        end_char: u32,
    ) -> bool;
}

impl SpanMapperView for ProviderPositionMapper {
    fn provider_range_maps_to_source(
        &self,
        start_line: u32,
        start_char: u32,
        end_line: u32,
        end_char: u32,
    ) -> bool {
        // `tsx_range_to_carrier` is the fail-closed RANGE primitive: it maps ONLY
        // when BOTH endpoints have a user-source correlation in the same projection
        // component, so a span straddling a synthetic helper region / prelude /
        // rewritten specifier returns `None` (⇒ GeneratedOnly, suppressed).
        //
        // TODO(follow-up): the underlying `tsx_range_to_carrier` validates the two
        // ENDPOINTS (and same-component membership), not every interior column. A
        // range that starts and ends in mapped source but spans a `SelfFile`
        // rune-module rewritten-specifier INTERIOR could still map. Tightening that
        // is a `ProviderPositionMapper`/`SelfFileProviderMapper` concern (the mapper
        // owns interior-region range validation), not this seam's; tracked so the
        // classifier upgrades for free once the mapper enforces it.
        self.tsx_range_to_carrier(
            TsPosition::new(start_line, start_char),
            TsPosition::new(end_line, end_char),
        )
        .is_some()
    }
}

/// Classify a returned provider RANGE `[(start_line,start_char),(end_line,end_char))`
/// whose owning provider file has the typed [`SpanSubjectKind`] `subject`.
///
/// Range-based (a TypeScript result is a span/edit, not a point), structural
/// (companion-vs-real is the typed `subject`, NEVER a path-substring sniff):
/// - a [`Companion`](SpanSubjectKind::Companion) range that maps AS A WHOLE →
///   [`SpanClass::SourceMappable`];
/// - a [`Companion`](SpanSubjectKind::Companion) range that does NOT fully map
///   (synthetic region, or straddling the generated/source boundary) →
///   [`SpanClass::GeneratedOnly`] (suppressed — generated content never escapes);
/// - an [`External`](SpanSubjectKind::External) range → [`SpanClass::External`]
///   (returned as-is, regardless of mapping — a real `.ts` is not Verter's to
///   suppress).
#[must_use]
pub fn classify_provider_range(
    mapper: &dyn SpanMapperView,
    subject: SpanSubjectKind,
    start_line: u32,
    start_char: u32,
    end_line: u32,
    end_char: u32,
) -> SpanClass {
    match subject {
        // A real on-disk `.ts`/`.tsx` — returned as-is, never suppressed.
        SpanSubjectKind::External => SpanClass::External,
        SpanSubjectKind::Companion => {
            if mapper.provider_range_maps_to_source(start_line, start_char, end_line, end_char) {
                SpanClass::SourceMappable
            } else {
                // A synthetic helper region inside a carrier companion, or a span
                // straddling the generated/source boundary: suppress.
                SpanClass::GeneratedOnly
            }
        }
    }
}

/// Classify a single returned provider POINT at `(line, character)` — the
/// degenerate empty-range case (a zero-width position, e.g. a definition target).
/// Delegates to [`classify_provider_range`] with `start == end`.
#[must_use]
pub fn classify_provider_point(
    mapper: &dyn SpanMapperView,
    subject: SpanSubjectKind,
    line: u32,
    character: u32,
) -> SpanClass {
    classify_provider_range(mapper, subject, line, character, line, character)
}

// ── multi-file epoch capture + version gating ───────────────────────────

/// The per-file engine/project epoch identity captured for one touched provider
/// file: its current `(generation, content_hash, map_hash)`, or `Absent` when the
/// file has no current snapshot (closed / never synced).
///
/// `generation` advances on EVERY record/close, so a re-sync (even byte-identical)
/// advances it; `content_hash` + `map_hash` are the content/source-map identity a
/// mapped result was produced against. Any difference between the before/after
/// capture is a mid-flight change.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum FileEpoch {
    /// The file is `Current` with this identity.
    Present {
        generation: u64,
        content_hash: Hash16,
        map_hash: Hash16,
    },
    /// The file has no current snapshot (closed / never synced).
    Absent,
}

/// A multi-file engine/project epoch captured across a set of provider files,
/// keyed by provider path (order-INDEPENDENT — a result that returns the same set
/// in a different order is not spuriously stale).
///
/// Captured BEFORE and AFTER a request; the result is dropped if the two captures
/// disagree on any file. The capture is **project-bound** (every CURRENT surface
/// owned by the project — [`Self::capture_project`]), so a multi-file result
/// (rename / find-references / definition) is validated against EVERY project
/// surface a returned span could land in, not only the queried file — closing the
/// caller-dependent fail-open hole where a non-queried returned file changing
/// mid-flight would slip through.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RequestEpoch {
    /// `provider_path` → its epoch at capture time. A path NOT in the map was not
    /// a project surface at capture (treated as `Absent` when looked up).
    files: std::collections::HashMap<Arc<str>, FileEpoch>,
}

impl RequestEpoch {
    /// Capture the epoch of every CURRENT provider surface OWNED BY `project` (the
    /// project-bound capture). This is the authoritative before/after snapshot a
    /// multi-file result is validated against: any project surface that changes,
    /// closes, or appears mid-flight is detected. The owned-surface set is read
    /// from the single [`ProviderSurfaceStore`] (no second ownership map).
    #[must_use]
    pub fn capture_project(store: &ProviderSurfaceStore, project: &str) -> Self {
        let paths = store.current_project_surface_paths(project);
        Self::capture_paths(store, &paths)
    }

    /// Capture the epoch of an EXPLICIT provider-path set against the store's
    /// CURRENT state. A path with no current snapshot captures as
    /// [`FileEpoch::Absent`] (so a close mid-flight is a detected change). Used by
    /// [`Self::capture_project`] and directly when the touched set is already known.
    #[must_use]
    pub fn capture_paths(store: &ProviderSurfaceStore, paths: &[Arc<str>]) -> Self {
        let files = paths
            .iter()
            .map(|path| (Arc::clone(path), epoch_of(store, path)))
            .collect();
        Self { files }
    }

    /// Whether this (before) epoch agrees with `other` (after) on EVERY file in
    /// EITHER capture — order-independent, and a file present in one capture but
    /// absent from the other is a disagreement (a surface that appeared or
    /// disappeared mid-flight). This is the multi-file freshness predicate.
    #[must_use]
    pub fn unchanged_since(&self, other: &RequestEpoch) -> bool {
        if self.files.len() != other.files.len() {
            return false;
        }
        // Equal length + every self entry matches other ⇒ identical key sets and
        // identical epochs (a key in self missing from other fails the lookup).
        self.files
            .iter()
            .all(|(path, epoch)| other.files.get(path).is_some_and(|o| o == epoch))
    }

    /// Whether a result is FRESH (keep it) given the `before` and `after` epochs.
    /// Fresh iff nothing changed mid-flight; otherwise DROPPED (fail closed).
    /// Multi-file aware: a change on ANY captured file drops the result.
    #[must_use]
    pub fn result_is_fresh(before: &RequestEpoch, after: &RequestEpoch) -> bool {
        before.unchanged_since(after)
    }

    /// Whether EVERY returned provider path is fresh against this (before) capture
    /// — the returned-path validation. For each returned path:
    /// - if it was a project surface in the before-capture, its CURRENT epoch must
    ///   still match the captured one (unchanged); AND
    /// - if it was ABSENT from the before-capture, the STORE must NOT currently know
    ///   it as a virtual surface — a path the store knows (Current OR Closing) is a
    ///   Verter companion that appeared (or is closing) mid-flight and FAILS CLOSED;
    ///   only a path FULLY UNKNOWN to the store (a genuine on-disk `.ts`/`.tsx` the
    ///   store never synced) passes as external.
    ///
    /// Companion-vs-external is decided by the SINGLE [`ProviderSurfaceStore`]
    /// authority ([`ProviderSurfaceStore::is_known_virtual_surface`]) — NOT a
    /// caller-supplied path heuristic — so a `CarrierApi` companion like
    /// `Foo.vue.verter.ts` (which a `.vue.tsx`/`.svelte.tsx` suffix check would
    /// misclassify as external) is correctly treated as a companion and fails
    /// closed.
    ///
    /// This is the fail-closed gate for a result whose touched set is DISCOVERED
    /// from the engine response: a returned project surface that was not captured
    /// before the request, or whose epoch advanced, drops the result.
    #[must_use]
    pub fn returned_paths_all_fresh(
        &self,
        store: &ProviderSurfaceStore,
        returned_paths: &[Arc<str>],
    ) -> bool {
        returned_paths.iter().all(|path| {
            match self.files.get(path) {
                // Captured before ⇒ must be unchanged now.
                Some(captured) => &epoch_of(store, path) == captured,
                // NOT captured before: a path the store knows as a virtual surface
                // (Current or Closing) is a Verter companion that appeared/closed
                // mid-flight ⇒ fail closed. A fully-unknown path is a genuine
                // external file ⇒ pass.
                None => !store.is_known_virtual_surface(path),
            }
        })
    }

    /// The number of captured files in this epoch (diagnostics / tests).
    #[must_use]
    pub fn captured_len(&self) -> usize {
        self.files.len()
    }
}

/// The current epoch identity of one provider path against the store (`Absent`
/// when it has no current snapshot).
fn epoch_of(store: &ProviderSurfaceStore, path: &str) -> FileEpoch {
    match store.current_snapshot(path) {
        Some(snap) => FileEpoch::Present {
            generation: snap.stamp.generation,
            content_hash: snap.stamp.content_hash.to_hash16(),
            map_hash: snap.stamp.map_hash,
        },
        None => FileEpoch::Absent,
    }
}

// ── project-bound sync planner (§2.5) ─────────────────────────────────────────

/// One file planned into a per-project atomic delta batch. The version-gate
/// identity (`content_hash`, `map_hash`, `version`) travels with it so the engine
/// snapshot and a later [`crate::external_ts_sync::QueryDedupeKey`] agree.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PlannedFile {
    pub source_uri: Arc<str>,
    pub provider_uri: Arc<str>,
    pub role: SnapshotRole,
    pub script_kind: ScriptKind,
    pub content: Arc<str>,
    pub content_hash: Hash16,
    pub map_hash: Hash16,
    pub version: u64,
}

impl PlannedFile {
    /// Build a planned file FROM a current store snapshot — the content-from-store
    /// path that keeps the single [`ProviderSurfaceStore`] the sole content
    /// authority (the seam never invents bytes the store does not own).
    ///
    /// FAIL CLOSED: returns `None` unless the snapshot belongs to the project the
    /// `binding` owns — its recorded `project_owner` must equal
    /// `binding.tsconfig_uri()`. So a batch built via [`ProjectSyncBatch::for_binding`]
    /// from these planned files can only carry surfaces the store recorded under
    /// THAT resolved binding (never a foreign project's surface, never a
    /// project-owner-less legacy record).
    ///
    /// `script_kind` is derived structurally from the recorded surface role
    /// (`Tsx` for the `.tsx` IDE/batch carrier, `Ts` for the `.verter.ts` API
    /// carrier / shadow / real) — not from a path-extension sniff.
    #[must_use]
    pub fn from_snapshot(
        binding: &ProjectBinding,
        snapshot: &crate::provider_surface_store::ProviderSurfaceSnapshot,
    ) -> Option<Self> {
        // Project-ownership gate: the surface must belong to this binding.
        let owner = snapshot.project_owner.as_deref()?;
        if owner != binding.tsconfig_uri() {
            return None;
        }
        let role = snapshot_role_of(snapshot.kind);
        Some(Self {
            source_uri: Arc::clone(&snapshot.source_canonical),
            provider_uri: Arc::clone(&snapshot.stamp.provider_path),
            role,
            // ScriptKind is the file's syntactic language, determined by the
            // provider path's EXTENSION (the same fact TypeScript itself keys on) —
            // NOT the role. A `Real` surface can be `.tsx` and a `Shadow` rune
            // module can be `.svelte.js`, so deriving from the role alone would
            // misclassify; the extension is authoritative.
            script_kind: script_kind_for_path(&snapshot.stamp.provider_path),
            content: Arc::clone(&snapshot.provider_content),
            content_hash: snapshot.stamp.content_hash.to_hash16(),
            map_hash: snapshot.stamp.map_hash,
            version: snapshot.stamp.generation,
        })
    }

    /// Lower this planned file into the contract's [`SnapshotFile`] DTO. The
    /// `open_state` is supplied by the caller (the sync planner knows editor-open
    /// state); the conversion here is the closed mapping, no second source of
    /// truth.
    #[must_use]
    fn into_snapshot_file(
        self,
        open_state: verter_session::external_ts::OpenState,
    ) -> SnapshotFile {
        SnapshotFile {
            source_uri: self.source_uri,
            provider_uri: self.provider_uri,
            role: self.role,
            script_kind: self.script_kind,
            content: self.content,
            content_hash: self.content_hash,
            map_hash: self.map_hash,
            // The planner carries the map IDENTITY (`map_hash`) but not the
            // serialized map JSON — the in-memory `ProviderSurfaceSnapshot` holds a
            // PARSED `ProviderPositionMapper`, not its source JSON. Threading the
            // map JSON onto the publish path (so the on-disk store writes the map
            // blob) is part of the live-publish wiring; until then this is `None`
            // and the store advertises no on-disk map blob for the file (it still
            // records the `map_hash` identity) — the fail-closed two-phase rule for
            // maps (never advertise a map blob that does not exist).
            map_json: None,
            version: self.version,
            open_state,
        }
    }
}

/// A per-project atomic delta batch (§2.5 step 5: "the adapter applies ONE atomic
/// batch per project").
///
/// CONSTRUCTION IS BINDING-GATED: the ONLY constructor ([`Self::for_binding`])
/// takes a resolved [`ProjectBinding`], reading the owning project URI FROM it.
/// There is no raw-string constructor, so a batch cannot be fabricated for a
/// `NoProject` / `Ambiguous` / `SyntheticScratch` source — this extends the
/// contract's `provider_op_requires_resolved_project` witness discipline to the
/// sync seam (the no-owner-⇒-no-publish gate is structural, not advisory). The
/// `project` field therefore always equals a resolved binding's `tsconfig_uri`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProjectSyncBatch {
    project: Arc<str>,
    files: Vec<PlannedFile>,
    resolution_map_version: u64,
    fs_generation: u64,
}

impl ProjectSyncBatch {
    /// Build the atomic batch for the project the resolved `binding` owns. The
    /// owning tsconfig URI is taken from the binding (not a caller-supplied string),
    /// so a batch is constructible ONLY from a resolved [`ProjectBinding`] — a
    /// `NoProject` / `Ambiguous` / `SyntheticScratch` source carries no binding and
    /// thus cannot produce a publish (fail closed).
    ///
    /// Callers obtain the `binding` from [`plan_publish_for_resolution`] (the
    /// resolution gate). `files` are the planned changed/added carriers (sourced
    /// from the single [`ProviderSurfaceStore`] — the seam never fabricates content
    /// the store does not own; see [`PlannedFile::from_snapshot`]).
    #[must_use]
    pub fn for_binding(
        binding: &ProjectBinding,
        files: Vec<PlannedFile>,
        resolution_map_version: u64,
        fs_generation: u64,
    ) -> Self {
        Self {
            project: Arc::from(binding.tsconfig_uri()),
            files,
            resolution_map_version,
            fs_generation,
        }
    }

    /// The owning project (tsconfig URI) this batch publishes to — always a
    /// resolved binding's `tsconfig_uri`.
    #[must_use]
    pub fn project(&self) -> &str {
        &self.project
    }

    /// The planned files in this batch.
    #[must_use]
    pub fn files(&self) -> &[PlannedFile] {
        &self.files
    }

    /// Lower the batch into the contract's [`PublishSnapshot`] DTO, marking every
    /// file `Closed` by default (the eager-index / background path). A caller with
    /// per-file open state uses [`Self::into_publish_snapshot_with`].
    #[must_use]
    pub fn into_publish_snapshot(self) -> PublishSnapshot {
        self.into_publish_snapshot_with(|_| verter_session::external_ts::OpenState::Closed)
    }

    /// Lower the batch into a [`PublishSnapshot`], resolving each file's open state
    /// through `open_state_of` (the sync planner's editor-open oracle).
    #[must_use]
    pub fn into_publish_snapshot_with(
        self,
        open_state_of: impl Fn(&str) -> verter_session::external_ts::OpenState,
    ) -> PublishSnapshot {
        let files = self
            .files
            .into_iter()
            .map(|f| {
                let open_state = open_state_of(&f.source_uri);
                f.into_snapshot_file(open_state)
            })
            .collect();
        PublishSnapshot {
            project: self.project,
            files,
            resolution_map_version: self.resolution_map_version,
            fs_generation: self.fs_generation,
        }
    }
}

/// Plan a project-bound publish for a resolved [`ProjectResolution`] (§2.5
/// step 2). Returns the resolved [`ProjectBinding`] to publish under ONLY for the
/// `ProjectBinding` state; `NoProject` / `Ambiguous` / `SyntheticScratch` yield
/// `None` (no external-TS publish — fail closed, the no-owner-⇒-no-result rule).
///
/// This is the gate that keeps a config-less / scratch source from EVER warming a
/// project cache: a caller cannot build a [`ProjectSyncBatch`] without a binding
/// to read the owner URI / env dims from.
#[must_use]
pub fn plan_publish_for_resolution(resolution: &ProjectResolution) -> Option<&ProjectBinding> {
    match resolution {
        ProjectResolution::ProjectBinding(binding) => Some(binding),
        // No owner / ambiguous / scratch ⇒ no project-bound publish. Verter-native
        // (non-external-TS) features may still answer, but external-TS is fail-closed.
        ProjectResolution::NoProject
        | ProjectResolution::Ambiguous(_)
        | ProjectResolution::SyntheticScratch(_) => None,
    }
}

/// Map a stored [`ProviderSurfaceKind`](crate::provider_surface_store::ProviderSurfaceKind)
/// to the contract [`SnapshotRole`]. The two enums are kept distinct (the contract
/// DTO does not depend on the store enum); this is the single mapping point on the
/// publish path.
#[must_use]
fn snapshot_role_of(kind: crate::provider_surface_store::ProviderSurfaceKind) -> SnapshotRole {
    use crate::provider_surface_store::ProviderSurfaceKind as K;
    match kind {
        K::CarrierIde => SnapshotRole::CarrierIde,
        K::CarrierApi => SnapshotRole::CarrierApi,
        K::CarrierBatch => SnapshotRole::CarrierBatch,
        K::Shadow => SnapshotRole::Shadow,
        K::Real => SnapshotRole::Real,
    }
}

/// The TypeScript `ScriptKind` of a provider file, determined by its path
/// EXTENSION — the same syntactic fact TypeScript itself uses to select a
/// language. This is NOT a semantic decision (it is the file's language by
/// extension, exactly as `tsc` resolves it), so it is the correct authority for
/// `ScriptKind`: a `.tsx`/`.jsx` is JSX-bearing, a `.ts`/`.js` is not — regardless
/// of the surface ROLE (a `Real` surface can be `.tsx`, a `Shadow` rune module can
/// be `.svelte.js`). The carrier companion extensions (`.vue.tsx`, `.svelte.tsx`)
/// end in `.tsx`; the `.verter.ts` API carrier ends in `.ts`. Defaults to `Ts` for
/// an unrecognised extension (the conservative non-JSX default).
#[must_use]
fn script_kind_for_path(provider_path: &str) -> ScriptKind {
    if provider_path.ends_with(".tsx") {
        ScriptKind::Tsx
    } else if provider_path.ends_with(".jsx") {
        ScriptKind::Jsx
    } else if provider_path.ends_with(".js")
        || provider_path.ends_with(".mjs")
        || provider_path.ends_with(".cjs")
    {
        ScriptKind::Js
    } else {
        // `.ts`/`.mts`/`.cts`/`.d.ts` and anything else → TS (non-JSX default).
        ScriptKind::Ts
    }
}

// ── eager CarrierApi index + lazy CarrierIde ─────────────────────────────

/// One eagerly-indexed public-API companion: the source URI it derives from and
/// the `CarrierApi` provider companion path the engine import-indexes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ApiCompanion {
    /// The owned carrier source (`/proj/src/Button.vue`).
    pub source_uri: Arc<str>,
    /// The redirect-reached `CarrierApi` companion path
    /// (`/proj/src/Button.vue.verter.ts`).
    pub provider_uri: Arc<str>,
    /// Always [`SnapshotRole::CarrierApi`] — the eager index force-materializes the
    /// lightweight public-API surface ONLY; `CarrierIde` stays lazy.
    pub role: SnapshotRole,
}

/// The eager `CarrierApi` index plan for a project. The macro-derived
/// public-API surfaces for ALL project-owned carrier sources are enumerated up
/// front so TypeScript auto-import / find-all-references draw from the project's
/// indexed export surfaces — a CLOSED `.vue`/`.svelte` component whose API surface
/// is missing would otherwise vanish from completions and references.
///
/// Full IDE TSX (`CarrierIde`) is NOT part of this plan: it is generated lazily for
/// open/diagnosed/queried carriers (and on-demand under a redirect-ON reference,
/// handled by the sync/redirection path — not here). The plan therefore force-
/// materializes ONLY `CarrierApi`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EagerApiIndexPlan {
    project: Arc<str>,
    api_companions: Vec<ApiCompanion>,
}

impl EagerApiIndexPlan {
    /// Build the eager index plan for `project` over its owned carrier sources.
    ///
    /// `api_companion_of` maps a source URI to its `CarrierApi` companion path
    /// (the descriptor-owned `{name}.vue.verter.ts` identity the caller supplies —
    /// the registry stays framework-agnostic by not hardcoding a suffix). A source
    /// with no derivable API companion is SKIPPED (fail closed — never an entry
    /// with no resolvable surface), so an unknown / non-carrier file never enters
    /// the index.
    #[must_use]
    pub fn for_owned_sources(
        project: impl Into<Arc<str>>,
        owned_sources: impl IntoIterator<Item = Arc<str>>,
        api_companion_of: impl Fn(&str) -> Option<Arc<str>>,
    ) -> Self {
        let api_companions = owned_sources
            .into_iter()
            .filter_map(|source| {
                let provider_uri = api_companion_of(&source)?;
                Some(ApiCompanion {
                    source_uri: source,
                    provider_uri,
                    role: SnapshotRole::CarrierApi,
                })
            })
            .collect();
        Self {
            project: project.into(),
            api_companions,
        }
    }

    /// The owning project (tsconfig URI).
    #[must_use]
    pub fn project(&self) -> &str {
        &self.project
    }

    /// Every eagerly-indexed `CarrierApi` companion.
    #[must_use]
    pub fn api_companions(&self) -> &[ApiCompanion] {
        &self.api_companions
    }

    /// Whether the index contains the `CarrierApi` surface for `source_uri` — the
    /// closed-component-in-auto-import/find-refs property.
    #[must_use]
    pub fn contains_api_for(&self, source_uri: &str) -> bool {
        self.api_companions
            .iter()
            .any(|c| &*c.source_uri == source_uri)
    }
}

// ── query de-dupe / cancellation (§2.7) ───────────────────────────────────────

/// The query de-dupe key (§2.7): `(project, provider_uri, carrier_offset, feature,
/// content_hash, map_hash, required_version, feature_param)`. Two queries with an
/// equal key are the SAME work and join one in-flight slot.
///
/// Every dimension of the contract [`Query`](verter_session::external_ts::Query)
/// fail-closed identity is in the key:
/// - `content_hash` + `map_hash` make a query against stale carrier content / a
///   stale source-map a DISTINCT slot (a fresh edit never joins a pre-edit slot);
/// - **`required_version`** is the snapshot-version gate the contract carries
///   (`Query.required_version`). It is load-bearing: a byte-identical re-sync or a
///   close-reopen ADVANCES the store generation while PRESERVING `content_hash` +
///   `map_hash`, so without the version a fresh query would wrongly join a STALE
///   in-flight slot and receive a result computed under the wrong snapshot —
///   violating the version-gated fail-closed contract;
/// - **`feature_param`** is the typed feature-parameter identity for a
///   parameterized feature (e.g. a rename's new-name hash): two renames at the same
///   offset with DIFFERENT replacement text are DIFFERENT work and must not join.
///   `[0u8; 16]` for parameter-less features (hover/definition/…).
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct QueryDedupeKey {
    pub project: Arc<str>,
    pub provider_uri: Arc<str>,
    pub carrier_offset: u32,
    pub feature: QueryFeature,
    pub content_hash: Hash16,
    pub map_hash: Hash16,
    /// The snapshot version the query is gated against (`Query.required_version`).
    pub required_version: u64,
    /// Typed feature-parameter identity (e.g. rename new-name hash); `[0u8; 16]`
    /// for parameter-less features.
    pub feature_param: Hash16,
}

/// The admission ticket a query holds for its in-flight slot. The LEADER created
/// the slot first (it is the one that runs the engine query); a JOINER observed an
/// existing slot for an identical [`QueryDedupeKey`] and must NOT issue a duplicate
/// engine query for it. Both share the slot's [`CancellationToken`], so a
/// supersession cancels the in-flight engine work ONCE for the whole join group.
///
/// SCOPE: this is the de-dupe + shared-cancellation half of the §2.7 interactive
/// lane — it ELIDES the duplicate engine query and shares cancellation. It does NOT
/// itself broadcast the leader's RESULT to joiners: result delivery is the engine
/// backend's concern (the result channel is added when a backend consumes this
/// registry), so a joiner today learns it must not re-issue (via [`Self::is_leader`]
/// = `false`) rather than receiving a payload from this type.
///
/// On `Drop`, the LEADER retires its slot (the de-dupe window closes), so a later
/// identical query leads a fresh slot. A joiner's drop does NOT retire the slot.
pub struct QueryAdmission {
    registry: QueryDedupeRegistry,
    key: QueryDedupeKey,
    token: CancellationToken,
    /// The unique id of the slot this admission belongs to. The leader retires its
    /// slot ON DROP only if the live slot still carries THIS id — a slot retired by
    /// a `cancel` and re-created by a later `admit` has a different id, so a stale
    /// leader-drop cannot evict the fresh slot.
    slot_id: u64,
    is_leader: bool,
}

impl QueryAdmission {
    /// Whether this admission LEADS the slot (it runs the engine query). A joiner
    /// returns `false`.
    #[must_use]
    pub fn is_leader(&self) -> bool {
        self.is_leader
    }

    /// The slot's shared cancellation token. Cancelling it cancels the in-flight
    /// engine work for the whole join group (wired to the engine, not just
    /// response-dropping).
    #[must_use]
    pub fn cancellation_token(&self) -> CancellationToken {
        self.token.clone()
    }

    /// Whether this admission shares the same in-flight slot as `other` — i.e. they
    /// joined the same query (same key AND same slot generation), so cancelling one
    /// cancels the other's engine work.
    #[must_use]
    pub fn shares_cancellation_with(&self, other: &QueryAdmission) -> bool {
        self.key == other.key && self.slot_id == other.slot_id
    }
}

impl Drop for QueryAdmission {
    fn drop(&mut self) {
        if self.is_leader {
            // The leader retires the slot on completion (de-dupe window closed), but
            // ONLY if the slot still carries THIS leader's id — a re-admission after
            // a `cancel` + retire installs a NEW slot id, so a stale leader-drop must
            // not evict it. `remove_if` makes the check-and-remove atomic.
            let slot_id = self.slot_id;
            self.registry
                .inner
                .slots
                .remove_if(&self.key, |_, slot| slot.id == slot_id);
        }
    }
}

/// One in-flight de-dupe slot: a unique id (for the leader-retire guard) and the
/// shared cancellation token for a join group of identical queries.
struct InflightSlot {
    id: u64,
    token: CancellationToken,
}

/// The query de-dupe registry for in-flight engine queries (§2.7). Concurrent
/// identical queries (equal [`QueryDedupeKey`]) JOIN one slot (the duplicate engine
/// query is elided); a supersession cancels the slot's shared token.
#[derive(Clone, Default)]
pub struct QueryDedupeRegistry {
    inner: Arc<DedupeInner>,
}

#[derive(Default)]
struct DedupeInner {
    slots: DashMap<QueryDedupeKey, InflightSlot>,
    /// Monotonic slot-id source; each new slot gets a fresh id so a leader-drop can
    /// distinguish its own slot from a same-key slot created after a retire.
    next_slot_id: std::sync::atomic::AtomicU64,
}

impl QueryDedupeRegistry {
    /// A fresh, empty registry.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Admit a query: if no slot exists for `key`, create one and LEAD it; else
    /// JOIN the existing slot (sharing its cancellation token). Returns the
    /// admission ticket.
    #[must_use]
    pub fn admit(&self, key: QueryDedupeKey) -> QueryAdmission {
        use dashmap::mapref::entry::Entry;
        match self.inner.slots.entry(key.clone()) {
            Entry::Occupied(slot) => {
                // JOIN: share the in-flight leader's token + slot id, do not lead.
                let token = slot.get().token.clone();
                let slot_id = slot.get().id;
                QueryAdmission {
                    registry: self.clone(),
                    key,
                    token,
                    slot_id,
                    is_leader: false,
                }
            }
            Entry::Vacant(vacancy) => {
                // LEAD: mint a fresh slot id + token, install the slot.
                let slot_id = self
                    .inner
                    .next_slot_id
                    .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                let token = CancellationToken::new();
                vacancy.insert(InflightSlot {
                    id: slot_id,
                    token: token.clone(),
                });
                QueryAdmission {
                    registry: self.clone(),
                    key,
                    token,
                    slot_id,
                    is_leader: true,
                }
            }
        }
    }

    /// Cancel the in-flight slot for `key` (a supersession). Trips the slot's
    /// shared token — every joiner observes the cancel — and retires the slot so a
    /// later identical query leads a fresh one. A no-op if no slot exists.
    pub fn cancel(&self, key: &QueryDedupeKey) {
        if let Some((_, slot)) = self.inner.slots.remove(key) {
            slot.token.cancel();
        }
    }

    /// The number of in-flight slots (diagnostics / tests).
    #[must_use]
    pub fn inflight_len(&self) -> usize {
        self.inner.slots.len()
    }
}

#[cfg(test)]
#[path = "external_ts_sync_tests.rs"]
mod tests;
