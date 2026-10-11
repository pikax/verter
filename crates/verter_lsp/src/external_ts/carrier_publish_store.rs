//! The on-disk content-addressed carrier-snapshot store + atomic manifest — the
//! Rust publish authority the Node `@verter/typescript-plugin` reads SYNCHRONOUSLY
//! (the two processes share NO memory).
//!
//! ## Why this exists (§2.2 of the external TypeScript engine contract)
//!
//! The plugin's host APIs are SYNCHRONOUS and tsserver caches their results —
//! including NEGATIVE results. So a companion must never be advertised before its
//! content exists on disk, and a reader must never observe a torn manifest. This
//! store realises the architect's split-manifest two-phase-publish defense:
//!
//! 1. **Content-addressed blobs.** Each carrier's content is written to
//!    `blobs/blake3-<content_hash_hex>.<ext>` and its source map to
//!    `maps/blake3-<map_hash_hex>.json`. Content-addressing makes a blob write
//!    IDEMPOTENT and STABLE: the same content always lands at the same path, and a
//!    temp-then-rename write means a reader never sees a half-written blob.
//! 2. **Split manifest.** The published [`Manifest`] separates `owned_sources`
//!    (the full project-owned carrier set, known the moment ownership resolves)
//!    from `ready_files` (a `provider_uri` enters ONLY after its content blob write
//!    succeeds). The plugin's `getExternalFiles` returns only `ready_files`.
//! 3. **Two-phase publish.** The write step writes every blob + map (idempotent,
//!    skipped if already present). The commit step appends ONE journal record
//!    naming only the rows the publication changes, advancing the monotonic
//!    `epoch`. The record is the LAST thing written and is applied whole or not at
//!    all, so a reader sees either the old or the new state — never a torn one —
//!    and every `ready_files` entry it names has a blob on disk.
//! 4. **Incremental manifest.** The manifest is not one file rewritten per
//!    publication: `head.json` names a generation whose compacted base
//!    (`snapshot-<generation>.json`) plus append-only journal
//!    (`journal-<generation>.log`) fold to it. Writer and readers process only the
//!    records appended since they last looked, and bounded compaction keeps a cold
//!    load proportional to the live rows — see the `journal` submodule.
//!
//! ## Location — NEVER the user's working tree
//!
//! The store lives under `std::env::temp_dir()` (mirroring the
//! [`crate::svelte_assets`] `host_shim_dir()` pattern):
//! `<temp>/verter-carrier-store/<host-version>/<workspace-hash>/`. The
//! `workspace-hash` is `blake3` over the canonicalized, case-folded workspace root
//! path, rendered as the PORTABLE `blake3-<hex>` (NEVER `blake3:<hex>` — the colon
//! is NTFS-illegal). Every path is built with [`Path::join`], never string
//! concatenation.
//!
//! ## Last-good + GC
//!
//! Publishing is purely ADDITIVE to `blobs/`/`maps/`; a committed journal record is the
//! only mutation of the pointer set. A blob a previous manifest could reference is NEVER
//! clobbered (content-addressing guarantees a re-publish of the same content is a
//! no-op, and a new content lands at a new path). GC of unreferenced blobs is OUT
//! OF SCOPE for this sub-block — see the `gc` follow-up note on [`CarrierPublishStore`].

use std::collections::BTreeMap;
use std::io::Write;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use verter_session::external_ts::{PublishSnapshot, ScriptKind, SnapshotFile, SnapshotRole};
// The single workspace filesystem-case-identity policy (Windows + default macOS fold
// case, Linux is exact) — shared with the tsgo `--api` membership comparator.
use verter_span::path::fs_is_case_insensitive;

/// The directory name segment for the whole carrier store (under the system temp
/// dir). A single fixed segment so every host version's stores cluster under it.
const STORE_DIR_NAME: &str = "verter-carrier-store";

/// The default carrier-store host-version segment: the Verter LSP package version
/// (mirroring the [`crate::svelte_assets`] `host_shim_dir()` precedent). It is the
/// SINGLE source both the publish path and the tsserver spawn use to derive the
/// store dir, so they agree on one location without negotiating a TS version at
/// spawn time. An LSP upgrade clusters stores under a fresh segment, never reusing
/// stale blobs across LSP versions.
///
/// Under a test build a per-session override of this segment can be installed
/// (see [`test_store_dir_override`]) so each real-provider test session gets its
/// own store tree; both the publish backend ([`CarrierPublishStore::open`] via
/// [`TsserverEngineBackend::with_default_host_version`]) and the tsserver spawn
/// ([`default_carrier_store_dir_string`]) read THIS one function, so an installed
/// override moves both sides onto the same isolated dir. Production is unaffected:
/// the override branch is `#[cfg(test)]`-only and the live derivation returns the
/// package version verbatim.
#[must_use]
pub fn default_carrier_store_host_version() -> &'static str {
    #[cfg(test)]
    if let Some(segment) = test_store_dir_override::current() {
        return segment;
    }
    env!("CARGO_PKG_VERSION")
}

/// Test-only per-session override of the carrier-store host-version segment.
///
/// The production store dir is keyed `(host_version, workspace_root)`, so two test
/// sessions over the SAME fixture workspace root resolve to the SAME on-disk store
/// — an earlier session's blobs/manifest then leak into a later session's cold
/// read. The real-provider test harness installs a UNIQUE segment per session so
/// each session's dir is `…/verter-carrier-store/<unique-segment>/<workspace-hash>/`,
/// fully isolated, while the production `(host_version, workspace_root)` derivation
/// stays byte-identical (this override is the only thing that touches the segment,
/// and only in a test build).
///
/// The segment is read by [`default_carrier_store_host_version`], which is the
/// single function BOTH the LSP-side publish backend and the tsserver spawn-dir
/// string call — so installing one override moves both sides onto the same dir.
/// The override is process-global; the harness holds it only across the
/// synchronous server construction (no `.await`), so concurrent sessions never
/// observe each other's segment.
#[cfg(test)]
pub mod test_store_dir_override {
    use std::sync::Mutex;

    /// The currently-installed segment (a leaked `&'static str` so
    /// [`super::default_carrier_store_host_version`] can return it). `None` ⇒ no
    /// override (the live package-version segment). Leaking is acceptable here: it
    /// is a test-only path with a bounded number of sessions per process, each
    /// leaking a few dozen bytes once.
    static OVERRIDE: Mutex<Option<&'static str>> = Mutex::new(None);

    /// Serializes the install→read→clear window so two concurrent sessions cannot
    /// interleave their segments across the synchronous server construction that
    /// reads [`super::default_carrier_store_host_version`].
    static INSTALL_LOCK: Mutex<()> = Mutex::new(());

    /// The currently-installed override segment, if any.
    #[must_use]
    pub fn current() -> Option<&'static str> {
        *OVERRIDE.lock().expect("carrier store-dir override lock")
    }

    /// Acquire the install lock for the duration of a server construction that
    /// reads the override. Returned guard must outlive the `set`/`clear` pair so a
    /// concurrent session's construction does not observe a foreign segment.
    pub fn install_lock() -> std::sync::MutexGuard<'static, ()> {
        INSTALL_LOCK.lock().expect("carrier store-dir install lock")
    }

    /// Install `segment` as the active override (leaking it to `&'static`). Hold
    /// [`install_lock`] across the matching [`clear`].
    pub fn set(segment: &str) {
        let leaked: &'static str = Box::leak(segment.to_owned().into_boxed_str());
        *OVERRIDE.lock().expect("carrier store-dir override lock") = Some(leaked);
    }

    /// Clear the active override (restore the live package-version segment).
    pub fn clear() {
        *OVERRIDE.lock().expect("carrier store-dir override lock") = None;
    }
}

/// The per-workspace carrier-store dir under the system temp dir
/// (`<temp>/verter-carrier-store/<host-version>/<workspace-hash>/`) — the SINGLE
/// path-derivation both the LSP publish path ([`CarrierPublishStore::open`]) and
/// the tsserver spawn (which delivers it to the plugin via
/// `VERTER_CARRIER_STORE_DIR`) compute, so the plugin reads exactly the store the
/// LSP writes. `host_version` is the per-host-version segment (use
/// [`default_carrier_store_host_version`] on the live path).
#[must_use]
#[allow(
    clippy::disallowed_methods,
    reason = "content-addressed cache root keyed by host version + workspace hash, not a per-test \
              scratch dir removed and rewritten across runs — see verter_test_support::unique_temp_dir \
              for the anti-pattern this lint actually guards against"
)]
pub fn carrier_store_dir_for(host_version: &str, workspace_root: &str) -> PathBuf {
    std::env::temp_dir()
        .join(STORE_DIR_NAME)
        .join(host_version)
        .join(workspace_hash_dir(workspace_root))
}

/// The LSP-default per-workspace carrier-store dir as a portable forward-slash
/// string — the form the tsserver spawn delivers to the plugin through the
/// `VERTER_CARRIER_STORE_DIR` environment variable. Built on
/// [`carrier_store_dir_for`] with [`default_carrier_store_host_version`], so a
/// spawn caller and the live publish backend ([`CarrierPublishStore`]) resolve the
/// same directory. The forward-slash normalization matches the path form the
/// plugin's `node:path` joins expect on every platform.
#[must_use]
pub fn default_carrier_store_dir_string(workspace_root: &str) -> String {
    carrier_store_dir_for(default_carrier_store_host_version(), workspace_root)
        .to_string_lossy()
        .replace('\\', "/")
}

/// Render a 16-byte hash as the PORTABLE on-disk identifier `blake3-<hex>`.
///
/// NEVER `blake3:<hex>` — the colon is one of the NTFS-illegal characters
/// (`< > : " | ? * \`), so a `:`-form basename is unopenable on Windows. The
/// `blake3-` prefix is the sanitized form mandated by the Cross-Platform
/// Portability rule for a generated on-disk name.
#[must_use]
fn blake3_name(hash: &[u8; 16]) -> String {
    let mut s = String::with_capacity(7 + 32);
    s.push_str("blake3-");
    for b in hash {
        // Two lowercase hex digits per byte — matches `^blake3-[0-9a-f]+$`.
        s.push(char::from_digit((b >> 4) as u32, 16).expect("nibble"));
        s.push(char::from_digit((b & 0x0f) as u32, 16).expect("nibble"));
    }
    s
}

/// The file extension (no leading dot) for a carrier blob, by its TypeScript
/// `ScriptKind`. The blob name is `blake3-<content_hash_hex>.<ext>` so a reader can
/// hand the right script kind to tsserver from the path alone if needed.
#[must_use]
fn blob_ext(script_kind: ScriptKind) -> &'static str {
    match script_kind {
        ScriptKind::Tsx => "tsx",
        ScriptKind::Ts => "ts",
        ScriptKind::Jsx => "jsx",
        ScriptKind::Js => "js",
    }
}

/// Compute the per-workspace store dir name from the workspace root path.
///
/// `blake3` over the CANONICALIZED path bytes, case-folded ONLY on a
/// case-insensitive filesystem (Windows / macOS-default) so the same workspace
/// opened with a different-case drive letter maps to ONE store there, while two
/// genuinely case-DISTINCT roots on a case-sensitive filesystem (Linux) get
/// DISTINCT stores. Canonicalization is best-effort: an un-canonicalizable path
/// (does not exist yet) falls back to the raw path string — the hash is a
/// directory-disambiguator, not a security boundary, so a stable-per-string
/// fallback is correct.
#[must_use]
fn workspace_hash_dir(workspace_root: &str) -> String {
    let canonical = std::fs::canonicalize(workspace_root)
        .ok()
        .and_then(|p| p.to_str().map(str::to_owned))
        .unwrap_or_else(|| workspace_root.to_owned());
    let folded = if fs_is_case_insensitive() {
        canonical.to_lowercase()
    } else {
        canonical
    };
    let digest = blake3::hash(folded.as_bytes());
    let mut h16 = [0u8; 16];
    h16.copy_from_slice(&digest.as_bytes()[..16]);
    blake3_name(&h16)
}

// ── manifest schema (serde) ──────────────────────────────────────────────

/// The TypeScript `ScriptKind` as serialized in the manifest. A standalone wire
/// enum (the manifest must not depend on the contract enum's serde representation),
/// mapped from [`ScriptKind`] at write time.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ManifestScriptKind {
    #[serde(rename = "TSX")]
    Tsx,
    #[serde(rename = "TS")]
    Ts,
    #[serde(rename = "JSX")]
    Jsx,
    #[serde(rename = "JS")]
    Js,
}

impl From<ScriptKind> for ManifestScriptKind {
    fn from(k: ScriptKind) -> Self {
        match k {
            ScriptKind::Tsx => ManifestScriptKind::Tsx,
            ScriptKind::Ts => ManifestScriptKind::Ts,
            ScriptKind::Jsx => ManifestScriptKind::Jsx,
            ScriptKind::Js => ManifestScriptKind::Js,
        }
    }
}

/// The carrier role as serialized in the manifest (standalone wire enum, mapped
/// from the contract [`SnapshotRole`]).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ManifestRole {
    CarrierIde,
    CarrierApi,
    Shadow,
    Real,
}

impl From<SnapshotRole> for ManifestRole {
    fn from(r: SnapshotRole) -> Self {
        match r {
            SnapshotRole::CarrierIde => ManifestRole::CarrierIde,
            SnapshotRole::CarrierApi => ManifestRole::CarrierApi,
            SnapshotRole::Shadow => ManifestRole::Shadow,
            SnapshotRole::Real => ManifestRole::Real,
        }
    }
}

/// One entry in a project's `owned_sources`: the full project-owned carrier set,
/// known the moment ownership resolves (BEFORE any content is published). The
/// plugin learns which sources the project owns from here even before their blobs
/// exist; a source is advertised through `getExternalFiles` ONLY once it appears in
/// `ready_files`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct OwnedSource {
    pub source_uri: String,
    pub provider_uri: String,
    pub role: ManifestRole,
    pub script_kind: ManifestScriptKind,
}

/// One entry in a project's `ready_files`: a `provider_uri` whose content blob
/// write has SUCCEEDED. Carries the content-addressed blob/map relative paths so
/// the plugin reads the exact bytes the offsets/maps were produced against.
///
/// INVARIANT (the two-phase guarantee): every `ReadyFile` named in a published
/// manifest has its `blob_rel` present on disk — the manifest swap is the commit
/// step, after every blob write in the write step succeeded.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ReadyStructureStamp {
    pub schema_version: u32,
    pub artifact_token: String,
    pub script_content_ranges: Vec<[u32; 2]>,
    /// Parser-identified markup opening-tag spans (additive; absent in stamps
    /// written before the field existed).
    #[serde(skip_serializing_if = "Vec::is_empty", default)]
    pub markup_opening_ranges: Vec<[u32; 2]>,
}

/// The carrier-store wire contract version this publisher writes: the manifest
/// `schema_version` every published `ReadyStructureStamp` carries.
///
/// A locally authored FORMAT pin, not a negotiated observation of the peer, and
/// not one shared constant across languages: the Rust producer names it here,
/// while each TypeScript consumer mirrors the same number as a literal
/// (`packages/language-shared/src/carrier/remap.ts`,
/// `packages/typescript-plugin/src/index.ts`) and fails closed to `null` on a
/// mismatch, so a bump de-synchronises the reader instead of mis-mapping it. As
/// a dimension of an observed engine profile
/// (see `TsserverEngineBackend::serving_identity`) it records WHICH wire
/// contract this publisher writes — a differently pinned publisher composes
/// different query identities — and asserts nothing about what a peer speaks.
pub const CARRIER_STORE_WIRE_PIN: u64 = 1;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ReadyFile {
    pub content_hash: String,
    pub version: u64,
    pub script_kind: ManifestScriptKind,
    pub role: ManifestRole,
    pub map_hash: String,
    /// `blobs/blake3-<content_hash_hex>.<ext>` — relative to the workspace store dir.
    pub blob_rel: String,
    /// `maps/blake3-<map_hash_hex>.json` — relative to the workspace store dir.
    /// `None` when the carrier carries no source map (a zero `map_hash`).
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub map_rel: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub structure: Option<ReadyStructureStamp>,
    /// The store epoch of the commit that published these bytes under this
    /// provider: a republication of identical bytes keeps it, any change takes
    /// the publishing commit's epoch. Epochs of one store instance are never
    /// reused, so together with the instance it is the row's non-reusable
    /// publication stamp — the same for every writer process. `0` for a row
    /// written before the field existed.
    #[serde(skip_serializing_if = "is_zero", default)]
    pub published_epoch: u64,
}

#[allow(
    clippy::trivially_copy_pass_by_ref,
    reason = "serde skip predicate signature"
)]
fn is_zero(value: &u64) -> bool {
    *value == 0
}

/// One project's manifest entry: its full owned carrier set plus the subset that is
/// ready (content on disk). `ready_files` is keyed by `provider_uri`.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct ProjectEntry {
    pub owned_sources: Vec<OwnedSource>,
    pub ready_files: BTreeMap<String, ReadyFile>,
}

/// The atomic manifest. `epoch` is monotonic across every publish to this
/// workspace store; the plugin re-reads when it advances. Keyed by `project_uri`
/// (the owning tsconfig URI).
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct Manifest {
    pub epoch: u64,
    pub host_version: String,
    pub projects: BTreeMap<String, ProjectEntry>,
}

// ── the publish batch input ──────────────────────────────────────────────

/// How a [`PublishBatch`]'s `owned_sources` reconciles with the project's existing
/// owned set — the publish contract that decides whether sibling carriers are
/// pruned.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OwnedSetScope {
    /// `owned_sources` is the FULL authoritative project owned set. The store
    /// REWRITES the project's `owned_sources` to it and PRUNES `ready_files` to the
    /// entries it admits — a `provider_uri` whose source is no longer in the owned
    /// set is removed (the deleted / no-longer-owned carrier is no longer
    /// advertised). Use when the publisher knows every carrier the project owns.
    ProjectAuthoritative,
    /// `owned_sources` is a PER-SOURCE delta (the touched carrier's rows only — the
    /// live per-edit publish case). The store UNIONS it by `source_uri` (refreshing
    /// this source's rows, leaving sibling carriers' rows intact) and does NOT prune
    /// — a single carrier's publish must never retract its siblings, which it does
    /// not know about. Sibling retraction goes through
    /// [`CarrierPublishStore::retract_sources`].
    SourceDelta,
}

/// A per-project atomic publish: the owning project, the owned-source rows (their
/// reconciliation governed by [`OwnedSetScope`]), and the ready files whose content
/// is to be written + advertised.
///
/// Built from the [`PublishSnapshot`] the project-bound sync seam produces (its
/// `SnapshotFile`s carry the content-addressed `content_hash` / `map_hash` this
/// store keys blobs on).
#[derive(Debug, Clone)]
pub struct PublishBatch {
    /// The owning workspace root (selects the per-workspace store dir).
    pub workspace_root: String,
    /// The owning project (tsconfig URI) — the manifest `projects` key.
    pub project_uri: String,
    /// The owned-source rows for this publish; reconciled per [`Self::owned_scope`].
    /// May be empty to publish content for a project whose owned set was set by a
    /// prior batch.
    pub owned_sources: Vec<OwnedSource>,
    /// Whether `owned_sources` is the project's authoritative full set (prune) or a
    /// per-source delta (union, no prune).
    pub owned_scope: OwnedSetScope,
    /// The files to write blobs/maps for and advertise in `ready_files`. Empty when
    /// only the owned set is being registered (the owned-then-content split).
    pub ready: PublishSnapshot,
}

impl PublishBatch {
    /// Build a [`PublishBatch`] from a [`PublishSnapshot`] and the owned-source set.
    /// The owned-source rows are derived from the snapshot's own files when
    /// `owned_sources` is `None` (the common case where the published delta IS the
    /// owned set); pass `Some(..)` to register a different owned set than the delta.
    /// `owned_scope` selects the reconciliation contract (authoritative-prune vs
    /// per-source-delta union).
    #[must_use]
    pub fn from_snapshot(
        workspace_root: impl Into<String>,
        snapshot: PublishSnapshot,
        owned_sources: Option<Vec<OwnedSource>>,
        owned_scope: OwnedSetScope,
    ) -> Self {
        let project_uri = snapshot.project.to_string();
        let owned_sources = owned_sources.unwrap_or_else(|| {
            snapshot
                .files
                .iter()
                .map(owned_source_of_file)
                .collect::<Vec<_>>()
        });
        Self {
            workspace_root: workspace_root.into(),
            project_uri,
            owned_sources,
            owned_scope,
            ready: snapshot,
        }
    }
}

/// Derive an [`OwnedSource`] row from a snapshot file (its source/provider/role/
/// script-kind). Used when the owned set equals the published delta.
#[must_use]
fn owned_source_of_file(file: &SnapshotFile) -> OwnedSource {
    OwnedSource {
        source_uri: file.source_uri.to_string(),
        provider_uri: file.provider_uri.to_string(),
        role: file.role.into(),
        script_kind: file.script_kind.into(),
    }
}

// ── the store ─────────────────────────────────────────────────────────────
// ── the store ─────────────────────────────────────────────────────────────

#[path = "carrier_publish_journal.rs"]
mod journal;

#[cfg(any(test, feature = "semantic-observe"))]
pub use journal::StoreWork;
use journal::{
    frame_record, journal_file, load_published, observe_work, parse_generation_file, read_head,
    snapshot_file, JournalOp, JournalRecord, ObservedWork, StoreCursor, StoreHead, StoreState,
    TailEnd, COMPACTION_FLOOR, HEAD_FILE, STORE_FORMAT, WRITER_LOCK_FILE,
};

/// A commit-boundary fault a test arms on the writer: the commit stops at the
/// named boundary as if the process died there (the step's error is returned and
/// the writer's in-memory fold is dropped). Test-only.
#[cfg(test)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum CommitFault {
    /// Stop before any record byte is written.
    BeforeAppend,
    /// Write only the first `bytes` of the record line, then stop.
    TornAppend { bytes: usize },
    /// Write the whole record line (committed), then stop before returning.
    AfterAppend,
    /// Stop after the next generation's base is written.
    CompactionAfterSnapshot,
    /// Stop after the next generation's empty journal is written.
    CompactionAfterJournal,
    /// Stop after the head swap, before the writer adopts the new generation.
    CompactionAfterHead,
}

/// The writer's folded state and open journal handle, guarded by
/// [`CarrierPublishStore::writer`].
#[derive(Debug, Default)]
struct WriterState {
    /// The folded authoritative generation, or `None` before the first commit and
    /// after any failed one (the next commit reloads from disk).
    cursor: Option<StoreCursor>,
    /// The append handle of `cursor.generation`'s journal.
    journal: Option<(u64, std::fs::File)>,
    /// Highest epoch this process committed: seeds a store re-initialised after its
    /// directory vanished, so the epoch never regresses.
    last_epoch: u64,
    work: ObservedWork,
    #[cfg(test)]
    fault: Option<CommitFault>,
}

impl WriterState {
    fn forget(&mut self) {
        self.cursor = None;
        self.journal = None;
    }

    /// Consume the armed fault when it is `at`.
    #[cfg(test)]
    fn trip(&mut self, at: CommitFault) -> std::io::Result<()> {
        if self.fault == Some(at) {
            self.fault = None;
            return Err(std::io::Error::other(format!(
                "injected commit fault at {at:?}"
            )));
        }
        Ok(())
    }
}

/// An advisory exclusive lock on the store's `writer.lock`, released on drop (and
/// by the OS when the holding process dies, so a crash never wedges the store).
struct WriterLockGuard(std::fs::File);

impl WriterLockGuard {
    fn acquire(path: &Path) -> std::io::Result<Self> {
        let file = std::fs::OpenOptions::new()
            .create(true)
            .read(true)
            .write(true)
            .truncate(false)
            .open(path)?;
        file.lock()?;
        Ok(Self(file))
    }
}

impl Drop for WriterLockGuard {
    fn drop(&mut self) {
        let _ = self.0.unlock();
    }
}

/// Group owned rows per source in first-appearance order — the unit a
/// [`JournalOp::OwnedPut`] replaces.
fn group_by_source(rows: &[OwnedSource]) -> Vec<(String, Vec<OwnedSource>)> {
    let mut grouped: Vec<(String, Vec<OwnedSource>)> = Vec::new();
    let mut index: std::collections::HashMap<&str, usize> = std::collections::HashMap::new();
    for row in rows {
        match index.get(row.source_uri.as_str()) {
            Some(&at) => grouped[at].1.push(row.clone()),
            None => {
                index.insert(row.source_uri.as_str(), grouped.len());
                grouped.push((row.source_uri.clone(), vec![row.clone()]));
            }
        }
    }
    grouped
}

/// The ops retracting `source_uri` from one project: its owned rows, and the ready
/// entry of every provider those rows named.
fn retract_source_ops(
    project_uri: &str,
    project: &journal::ProjectState,
    source_uri: &str,
    ops: &mut Vec<JournalOp>,
) {
    let Some(rows) = project.owned_rows_of(source_uri) else {
        return;
    };
    ops.push(JournalOp::OwnedDel {
        project: project_uri.to_owned(),
        source_uri: source_uri.to_owned(),
    });
    let mut dropped = std::collections::HashSet::new();
    for row in rows {
        if project.ready.contains_key(&row.provider_uri) && dropped.insert(&row.provider_uri) {
            ops.push(JournalOp::ReadyDel {
                project: project_uri.to_owned(),
                provider_uri: row.provider_uri.clone(),
            });
        }
    }
}

/// The on-disk content-addressed carrier-snapshot store + incremental manifest.
///
/// One store per `(host-version, workspace)`; cheap to construct (it only computes
/// paths). Every mutation is one commit: blobs/maps first (idempotent), then ONE
/// appended journal record naming only the rows it changes, advancing the
/// monotonic epoch — see the [`journal`] module for the format, the commit
/// boundary, compaction and recovery.
///
/// GC FOLLOW-UP: unreferenced blobs/maps accumulate (publishing is additive). A
/// future sub-block adds a sweep that retains every blob/map referenced by the
/// CURRENT manifest (and a short last-good window) and deletes the rest. This
/// sub-block never clobbers, so correctness does not depend on GC.
#[derive(Debug)]
pub struct CarrierPublishStore {
    /// The per-workspace store dir:
    /// `<temp>/verter-carrier-store/<host-version>/<workspace-hash>/`.
    workspace_dir: PathBuf,
    host_version: String,
    /// Serializes commits within this process and owns the folded state, so a
    /// commit never re-reads the published rows: it absorbs only the records other
    /// writers appended since its last commit, then appends its own. Commits across
    /// processes serialize on the `writer.lock` advisory lock. The write step
    /// (content-addressed blob writes) stays lock-free — each content hashes to a
    /// distinct path and the write is idempotent.
    writer: parking_lot::Mutex<WriterState>,
}

impl CarrierPublishStore {
    /// Open (compute the paths for) the store for `workspace_root` at this
    /// `host_version`. The store root is under the system temp dir — NEVER the user
    /// workspace. Directories are created lazily on the first publish, and the
    /// published state is folded lazily on the first commit.
    #[must_use]
    pub fn open(host_version: impl Into<String>, workspace_root: &str) -> Self {
        let host_version = host_version.into();
        let workspace_dir = carrier_store_dir_for(&host_version, workspace_root);
        Self {
            workspace_dir,
            host_version,
            writer: parking_lot::Mutex::new(WriterState::default()),
        }
    }

    /// The per-workspace store dir (under the system temp dir).
    #[must_use]
    pub fn workspace_dir(&self) -> &Path {
        &self.workspace_dir
    }

    /// The `blobs/` directory.
    #[must_use]
    pub fn blobs_dir(&self) -> PathBuf {
        self.workspace_dir.join("blobs")
    }

    /// The `maps/` directory.
    #[must_use]
    pub fn maps_dir(&self) -> PathBuf {
        self.workspace_dir.join("maps")
    }

    /// The `head.json` commit-pointer path (names the authoritative generation).
    #[must_use]
    pub fn head_path(&self) -> PathBuf {
        self.workspace_dir.join(HEAD_FILE)
    }

    /// The relative blob path for a content hash + script kind
    /// (`blobs/blake3-<hex>.<ext>`). PORTABLE — built with `Path` components and the
    /// `blake3-` prefix (no `:`).
    #[must_use]
    fn blob_rel(content_hash: &[u8; 16], script_kind: ScriptKind) -> String {
        // Forward slash is the manifest's portable relative-path separator (the
        // plugin joins it onto the store dir); the on-disk write uses `Path::join`.
        format!(
            "blobs/{}.{}",
            blake3_name(content_hash),
            blob_ext(script_kind)
        )
    }

    /// The relative map path for a map hash (`maps/blake3-<hex>.json`), or `None`
    /// for a zero hash (no source map).
    #[must_use]
    fn map_rel(map_hash: &[u8; 16]) -> Option<String> {
        if map_hash == &[0u8; 16] {
            return None;
        }
        Some(format!("maps/{}.json", blake3_name(map_hash)))
    }

    /// A fresh empty manifest at epoch 0 — the diagnostics view of a store that
    /// has never published or cannot be read.
    fn fresh_manifest(&self) -> Manifest {
        Manifest {
            epoch: 0,
            host_version: self.host_version.clone(),
            projects: BTreeMap::new(),
        }
    }

    /// Arm a commit-boundary fault for the next commit that reaches it. Test-only.
    #[cfg(test)]
    pub(crate) fn arm_commit_fault(&self, fault: CommitFault) {
        self.writer.lock().fault = Some(fault);
    }

    /// The deterministic work this writer performed so far. OPTIONAL measurement
    /// state (test and `semantic-observe` builds only).
    #[cfg(any(test, feature = "semantic-observe"))]
    #[must_use]
    pub fn work(&self) -> StoreWork {
        self.writer.lock().work
    }

    /// One commit: fold any records other writers appended, resolve `build`'s ops
    /// against the folded state, append them as ONE record (the commit boundary),
    /// apply them, and compact when the journal has outgrown the live rows.
    /// Returns the record's epoch.
    ///
    /// FAIL-CLOSED, NEVER CLOBBER: an unreadable head, base or journal (other than
    /// a torn tail) fails the commit before anything is appended; the on-disk store
    /// stays intact and the next commit reloads it. A torn tail is truncated under
    /// the writer lock before the append.
    fn commit(&self, build: impl FnOnce(&StoreState) -> Vec<JournalOp>) -> std::io::Result<u64> {
        std::fs::create_dir_all(&self.workspace_dir)?;
        let mut guard = self.writer.lock();
        let w = &mut *guard;
        let _lock = WriterLockGuard::acquire(&self.workspace_dir.join(WRITER_LOCK_FILE))?;

        if let Err(e) = self.sync(w) {
            w.forget();
            return Err(e);
        }
        let cursor = w.cursor.as_ref().expect("a synced writer holds a cursor");
        let record = JournalRecord {
            epoch: cursor.state.epoch + 1,
            ops: build(&cursor.state),
        };
        let line = frame_record(&record)?;
        if let Err(e) = self.append(w, &line) {
            w.forget();
            return Err(e);
        }

        let cursor = w.cursor.as_mut().expect("a synced writer holds a cursor");
        cursor.state.apply_record(record);
        cursor.journal_offset += line.len() as u64;
        cursor.journal_records += 1;
        let epoch = cursor.state.epoch;
        let due = cursor.journal_records >= COMPACTION_FLOOR.max(cursor.state.live_rows());
        w.last_epoch = epoch;
        observe_work!(w.work, |work| work.records_appended += 1);

        #[cfg(test)]
        if let Err(e) = w.trip(CommitFault::AfterAppend) {
            w.forget();
            return Err(e);
        }

        // The record is committed; a failed compaction only leaves the current
        // generation authoritative (its files are complete), so it never fails the
        // publication.
        if due {
            if let Err(e) = self.compact(w) {
                tracing::warn!(
                    store = %self.workspace_dir.display(),
                    error = %e,
                    "carrier store compaction failed; the current generation stays authoritative"
                );
                w.forget();
            }
        }
        Ok(epoch)
    }

    /// Bring the writer's fold up to the authoritative on-disk state: initialise a
    /// store with no head, reload on a generation change (or a first commit), and
    /// otherwise apply only the records appended since the last commit.
    fn sync(&self, w: &mut WriterState) -> std::io::Result<()> {
        let dir = &self.workspace_dir;
        let Some(head) = read_head(dir)? else {
            return self.initialize(w);
        };
        let current = w.cursor.as_ref().is_some_and(|c| c.follows(&head));
        let end = if current {
            let cursor = w.cursor.as_mut().expect("checked above");
            match cursor.tail(dir, &mut w.work) {
                Ok(end) => end,
                // The journal shrank under us: refold from the base.
                Err(e) if e.kind() == std::io::ErrorKind::UnexpectedEof => self.reload(w, &head)?,
                Err(e) => return Err(e),
            }
        } else {
            self.reload(w, &head)?
        };
        if end == TailEnd::Torn {
            let cursor = w.cursor.as_ref().expect("synced");
            let journal = std::fs::OpenOptions::new()
                .write(true)
                .open(cursor.journal_path(dir))?;
            journal.set_len(cursor.journal_offset)?;
            journal.sync_all()?;
            w.journal = None;
        }
        Ok(())
    }

    fn reload(&self, w: &mut WriterState, head: &StoreHead) -> std::io::Result<TailEnd> {
        w.forget();
        let (cursor, end) =
            StoreCursor::load(&self.workspace_dir, head, &mut w.work).map_err(|e| {
                // Under the writer lock nothing retires a generation, so a missing
                // base or journal is corruption, not a race.
                if e.kind() == std::io::ErrorKind::NotFound {
                    std::io::Error::new(
                        std::io::ErrorKind::InvalidData,
                        format!(
                            "carrier store head names generation {} whose files are missing: {e}",
                            head.generation
                        ),
                    )
                } else {
                    e
                }
            })?;
        w.cursor = Some(cursor);
        Ok(end)
    }

    /// Create a store: a fresh instance at generation 1 whose empty base sits at the
    /// last committed epoch, an empty journal, then the head.
    fn initialize(&self, w: &mut WriterState) -> std::io::Result<()> {
        let dir = &self.workspace_dir;
        let generation = 1;
        let instance = mint_store_instance(dir);
        let mut state = match std::fs::read(dir.join("manifest.json")) {
            // A store written before the journal format holds its membership in
            // `manifest.json` alone; fold it into the first base so no published
            // source/provider row is lost. Unparseable fails the publish rather
            // than silently erasing it.
            Ok(bytes) => StoreState::from_manifest(
                serde_json::from_slice::<Manifest>(&bytes).map_err(|e| {
                    std::io::Error::new(
                        std::io::ErrorKind::InvalidData,
                        format!("legacy carrier manifest is present but unparseable: {e}"),
                    )
                })?,
            ),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                StoreState::empty(w.last_epoch, self.host_version.clone())
            }
            Err(e) => return Err(e),
        };
        state.epoch = state.epoch.max(w.last_epoch);
        self.write_generation(generation, &instance, &state)?;
        w.forget();
        w.cursor = Some(StoreCursor {
            generation,
            instance,
            journal_offset: 0,
            journal_records: 0,
            state,
        });
        fsync_dir(dir);
        Ok(())
    }

    /// Write `generation`'s base and empty journal, then swap the head to it.
    fn write_generation(
        &self,
        generation: u64,
        instance: &str,
        state: &StoreState,
    ) -> std::io::Result<()> {
        let dir = &self.workspace_dir;
        let base = serde_json::to_vec(&state.to_manifest()).map_err(std::io::Error::other)?;
        write_atomic(dir, &dir.join(snapshot_file(generation)), &base)?;
        self.after_snapshot_written()?;
        write_atomic(dir, &dir.join(journal_file(generation)), b"")?;
        self.after_journal_written()?;
        let head = StoreHead {
            format: STORE_FORMAT,
            generation,
            instance: instance.to_owned(),
            host_version: self.host_version.clone(),
        };
        let head = serde_json::to_vec(&head).map_err(std::io::Error::other)?;
        write_atomic(dir, &self.head_path(), &head)?;
        fsync_dir(dir);
        Ok(())
    }

    // The compaction fault seams read the armed fault without re-entering the
    // writer guard: `compact` moves it into `compaction_fault` for the duration.
    #[cfg(test)]
    fn after_snapshot_written(&self) -> std::io::Result<()> {
        compaction_fault::trip(CommitFault::CompactionAfterSnapshot)
    }
    #[cfg(not(test))]
    #[allow(clippy::unused_self, clippy::unnecessary_wraps)]
    fn after_snapshot_written(&self) -> std::io::Result<()> {
        Ok(())
    }
    #[cfg(test)]
    fn after_journal_written(&self) -> std::io::Result<()> {
        compaction_fault::trip(CommitFault::CompactionAfterJournal)
    }
    #[cfg(not(test))]
    #[allow(clippy::unused_self, clippy::unnecessary_wraps)]
    fn after_journal_written(&self) -> std::io::Result<()> {
        Ok(())
    }

    /// Append one framed record to the current journal and make it durable.
    fn append(&self, w: &mut WriterState, line: &[u8]) -> std::io::Result<()> {
        let generation = w.cursor.as_ref().expect("synced").generation;
        if !matches!(&w.journal, Some((g, _)) if *g == generation) {
            let path = self.workspace_dir.join(journal_file(generation));
            let file = std::fs::OpenOptions::new()
                .append(true)
                .open(&path)
                .map_err(|e| {
                    std::io::Error::new(
                        std::io::ErrorKind::InvalidData,
                        format!(
                            "carrier store journal {} cannot be opened: {e}",
                            path.display()
                        ),
                    )
                })?;
            w.journal = Some((generation, file));
        }
        #[cfg(test)]
        {
            w.trip(CommitFault::BeforeAppend)?;
            if let Some(CommitFault::TornAppend { bytes }) = w.fault {
                w.fault = None;
                let (_, file) = w.journal.as_mut().expect("opened above");
                file.write_all(&line[..bytes.min(line.len())])?;
                file.sync_data()?;
                return Err(std::io::Error::other("injected commit fault at TornAppend"));
            }
        }
        let (_, file) = w.journal.as_mut().expect("opened above");
        file.write_all(line)?;
        file.sync_data()
    }

    /// Fold the current journal into the next generation's base, swap the head to
    /// it, and retire every generation older than the one just superseded (which a
    /// reader that read the previous head may still be loading).
    fn compact(&self, w: &mut WriterState) -> std::io::Result<()> {
        let cursor = w.cursor.as_ref().expect("synced");
        let previous = cursor.generation;
        let next = previous + 1;
        #[cfg(test)]
        let _armed = compaction_fault::arm(&mut w.fault);
        self.write_generation(next, &cursor.instance, &cursor.state)?;
        #[cfg(test)]
        compaction_fault::trip(CommitFault::CompactionAfterHead)?;

        let cursor = w.cursor.as_mut().expect("synced");
        observe_work!(w.work, |work| {
            work.compactions += 1;
            work.compaction_rows_written += cursor.state.live_rows();
        });
        cursor.generation = next;
        cursor.journal_offset = 0;
        cursor.journal_records = 0;
        w.journal = None;

        if let Ok(entries) = std::fs::read_dir(&self.workspace_dir) {
            for entry in entries.flatten() {
                let name = entry.file_name();
                let retired = name
                    .to_str()
                    .and_then(parse_generation_file)
                    .is_some_and(|generation| generation < previous);
                if retired {
                    let _ = std::fs::remove_file(entry.path());
                }
            }
        }
        Ok(())
    }

    /// Two-phase publish for ONE project. Returns the NEW epoch.
    ///
    /// Write step — write every blob + map (temp-then-rename, idempotent: a
    /// content-addressed blob that already exists is skipped). NOTHING is advertised
    /// yet.
    ///
    /// Commit step — append ONE journal record that reconciles this project's
    /// `owned_sources` per [`OwnedSetScope`] and puts a `ready_files` entry for every
    /// file whose blob write succeeded in the write step. The record is the commit
    /// boundary: a reader applies it whole or not at all, and every `ready_files`
    /// entry it names has its blob on disk.
    pub fn publish_batch(&self, batch: &PublishBatch) -> std::io::Result<u64> {
        let blobs_dir = self.blobs_dir();
        let maps_dir = self.maps_dir();
        std::fs::create_dir_all(&self.workspace_dir)?;
        std::fs::create_dir_all(&blobs_dir)?;
        std::fs::create_dir_all(&maps_dir)?;

        // ── Write step: write every blob + map. Collect the ready_files entries ──
        // ONLY for files whose blob write succeeded — a provider_uri enters
        // ready_files ONLY AFTER its content exists.
        let mut ready_entries: Vec<(String, ReadyFile)> =
            Vec::with_capacity(batch.ready.files.len());
        for file in &batch.ready.files {
            let blob_rel = Self::blob_rel(&file.content_hash, file.script_kind);
            let blob_abs = self.workspace_dir.join(&blob_rel);
            // Content-addressed ⇒ idempotent. Skip the write if the blob already
            // exists (its bytes are by definition this content).
            if !blob_abs.exists() {
                write_atomic(&blobs_dir, &blob_abs, file.content.as_bytes())?;
            }

            // The source map (if any) — content-addressed by `map_hash`. The map
            // blob is written from the snapshot's `map_json` (the serialized
            // `ProviderPositionMapper`). FAIL-CLOSED TWO-PHASE FOR MAPS: `map_rel`
            // is advertised ONLY when the map blob exists on disk — either it was
            // already present (content-addressed idempotency) or this publish wrote
            // it from `map_json`. A file carrying a `map_hash` but no `map_json`
            // (the in-memory rename-mapping path that has only the parsed mapper)
            // advertises NO map blob (no broken pointer); its `map_hash` identity is
            // still recorded.
            let map_rel = match (Self::map_rel(&file.map_hash), &file.map_json) {
                (Some(rel), Some(json)) => {
                    let map_abs = self.workspace_dir.join(&rel);
                    if !map_abs.exists() {
                        write_atomic(&maps_dir, &map_abs, json.as_bytes())?;
                    }
                    Some(rel)
                }
                // A map_hash present but no JSON: the blob may already exist from a
                // prior publish that DID carry the JSON (content-addressed) — only
                // then advertise it; otherwise no on-disk map blob.
                (Some(rel), None) => {
                    let map_abs = self.workspace_dir.join(&rel);
                    map_abs.exists().then_some(rel)
                }
                // No source map at all.
                (None, _) => None,
            };

            ready_entries.push((
                file.provider_uri.to_string(),
                ReadyFile {
                    // The bare lowercase hex (no `blake3-` prefix) — the prefix is
                    // an on-disk-name sanitization concern, not the identity value.
                    content_hash: hex16(&file.content_hash),
                    version: file.version,
                    script_kind: file.script_kind.into(),
                    role: file.role.into(),
                    map_hash: hex16(&file.map_hash),
                    blob_rel,
                    map_rel,
                    structure: file
                        .structure
                        .as_ref()
                        .map(|structure| ReadyStructureStamp {
                            schema_version: structure.schema_version,
                            artifact_token: structure.artifact_token.to_string(),
                            script_content_ranges: structure.script_content_ranges.clone(),
                            markup_opening_ranges: structure.markup_opening_ranges.clone(),
                        }),
                    // Stamped at commit, against the folded state the record
                    // is resolved on.
                    published_epoch: 0,
                },
            ));
        }

        // ── Commit step: resolve the reconciliation into row ops, append them ──
        self.commit(|state| reconcile_publish_ops(state, batch, ready_entries))
    }

    /// Retract one or more SOURCE carriers from a project — the
    /// delete / no-owner / now-ambiguous transition.
    ///
    /// Removes every `owned_sources` row of the named sources and the `ready_files`
    /// entry of every provider those rows named, in one committed record (advancing
    /// the epoch). After this, `getExternalFiles` no longer advertises the
    /// retracted carrier's companions. A source not present is a no-op for that
    /// source. Returns the new epoch (always advanced, so the plugin re-reads even
    /// for a pure retraction). Blobs are NOT deleted (content-addressed; GC is a
    /// separate sweep) — only the pointer set shrinks.
    ///
    /// This is the explicit counterpart to a [`OwnedSetScope::SourceDelta`] publish:
    /// a per-source publish adds/refreshes its own rows and never prunes siblings,
    /// so a sibling that leaves the project is retracted HERE rather than implied.
    pub fn retract_sources(&self, project_uri: &str, source_uris: &[&str]) -> std::io::Result<u64> {
        self.commit(|state| {
            let mut ops = Vec::new();
            if let Some(project) = state.projects.get(project_uri) {
                let mut seen = std::collections::HashSet::new();
                for source_uri in source_uris {
                    if seen.insert(*source_uri) {
                        retract_source_ops(project_uri, project, source_uri, &mut ops);
                    }
                }
            }
            ops
        })
    }

    /// Retract a SOURCE carrier from EVERY project that owns it — the
    /// delete / owner-no-longer-resolvable transition where the prior owning project
    /// is not known (a deleted carrier's owner can no longer be resolved). Removes
    /// the source's owned rows + advertised companions from every project entry in
    /// one committed record. A no-op (still epoch-advancing) when no project owns
    /// the source.
    pub fn retract_source_from_all_projects(&self, source_uri: &str) -> std::io::Result<u64> {
        self.commit(|state| {
            let mut ops = Vec::new();
            for (project_uri, project) in &state.projects {
                retract_source_ops(project_uri, project, source_uri, &mut ops);
            }
            ops
        })
    }

    /// Retract a SOURCE carrier from every project that owns it EXCEPT
    /// `keep_project_uri` — the owner-CHANGE (A→B) prune. The live per-source
    /// publish into the NEW owning project uses [`OwnedSetScope::SourceDelta`]
    /// (union, never prune), so it leaves the source's stale rows in its OLD
    /// project. This removes the source's owned rows + advertised companions from
    /// every OTHER project (so the old project's `getExternalFiles` stops serving
    /// it) while leaving the new owning project's freshly-published rows intact,
    /// in one committed record. A no-op (still epoch-advancing) when no other
    /// project owns the source.
    pub fn retract_source_from_all_projects_except(
        &self,
        source_uri: &str,
        keep_project_uri: &str,
    ) -> std::io::Result<u64> {
        self.commit(|state| {
            let mut ops = Vec::new();
            for (project_uri, project) in &state.projects {
                // Leave the new owning project's just-published rows intact.
                if project_uri != keep_project_uri {
                    retract_source_ops(project_uri, project, source_uri, &mut ops);
                }
            }
            ops
        })
    }

    /// Read the published state from disk STRICTLY: `Ok(None)` only when the store
    /// has never committed (no head), and an error for an unreadable or corrupt
    /// head, base or journal. A torn journal tail (an append in flight or
    /// interrupted) is not applied. Loads the authoritative generation from
    /// scratch; an incremental follower is [`PublishedStoreReader`].
    pub fn read_published(&self) -> std::io::Result<Option<Manifest>> {
        let mut work = ObservedWork::default();
        Ok(load_published(&self.workspace_dir, &mut work)?.map(|c| c.state.to_manifest()))
    }

    /// Read the current manifest from disk for DIAGNOSTICS / the plugin-equivalent
    /// reader (a fresh default when none exists OR is unreadable).
    ///
    /// Unlike the commit path — which must fail closed so a corrupt store never
    /// clobbers other projects on the next commit — this read-only view tolerates a
    /// corrupt store by reporting a fresh empty manifest (it never WRITES, so there
    /// is nothing to clobber; surfacing "empty" is the correct diagnostics
    /// behaviour for an unreadable store). Use [`Self::read_published`] where an
    /// unreadable store must not read as "nothing published".
    #[must_use]
    pub fn current_manifest(&self) -> Manifest {
        self.read_published()
            .ok()
            .flatten()
            .unwrap_or_else(|| self.fresh_manifest())
    }
}

/// Resolve one publish batch into row ops against the folded state — the single
/// owner of the owned-set reconciliation contract ([`OwnedSetScope`]).
fn reconcile_publish_ops(
    state: &StoreState,
    batch: &PublishBatch,
    mut ready_entries: Vec<(String, ReadyFile)>,
) -> Vec<JournalOp> {
    let project_uri = batch.project_uri.as_str();
    let mut ops = Vec::new();
    let empty = journal::ProjectState::default();
    let project = match state.projects.get(project_uri) {
        Some(project) => project,
        None => {
            ops.push(JournalOp::ProjectPut {
                project: project_uri.to_owned(),
            });
            &empty
        }
    };
    // This record commits at the next epoch. A row republished with the bytes
    // and map it already carries keeps its publication stamp; any other row
    // is published by this commit.
    let publishing_epoch = state.epoch + 1;
    for (provider_uri, file) in &mut ready_entries {
        file.published_epoch = match project.ready.get(provider_uri.as_str()) {
            Some(held)
                if held.content_hash == file.content_hash && held.map_hash == file.map_hash =>
            {
                held.published_epoch
            }
            _ => publishing_epoch,
        };
    }
    let groups = group_by_source(&batch.owned_sources);
    match batch.owned_scope {
        // Authoritative: REWRITE the owned set (when the batch carries one — an
        // empty owned set means "publish content only, keep the existing owned
        // set"), then PRUNE every ready entry the resulting owned set no longer
        // admits, so a deleted / no-longer-owned carrier stops being advertised.
        OwnedSetScope::ProjectAuthoritative => {
            let owned_after: std::collections::HashSet<&str> = if groups.is_empty() {
                project.owned_providers().collect()
            } else {
                ops.push(JournalOp::OwnedClear {
                    project: project_uri.to_owned(),
                });
                batch
                    .owned_sources
                    .iter()
                    .map(|o| o.provider_uri.as_str())
                    .collect()
            };
            let republished: std::collections::HashSet<&str> =
                ready_entries.iter().map(|(p, _)| p.as_str()).collect();
            let prune: Vec<String> = project
                .ready
                .keys()
                .filter(|p| !owned_after.contains(p.as_str()) && !republished.contains(p.as_str()))
                .cloned()
                .collect();
            for (source_uri, rows) in groups {
                ops.push(JournalOp::OwnedPut {
                    project: project_uri.to_owned(),
                    source_uri,
                    rows,
                });
            }
            for provider_uri in prune {
                ops.push(JournalOp::ReadyDel {
                    project: project_uri.to_owned(),
                    provider_uri,
                });
            }
            for (provider_uri, file) in ready_entries {
                if owned_after.contains(provider_uri.as_str()) {
                    ops.push(JournalOp::ReadyPut {
                        project: project_uri.to_owned(),
                        provider_uri,
                        file,
                    });
                } else if project.ready.contains_key(&provider_uri) {
                    ops.push(JournalOp::ReadyDel {
                        project: project_uri.to_owned(),
                        provider_uri,
                    });
                }
            }
        }
        // Per-source delta: replace the rows of every source this batch carries;
        // sibling carriers' rows stay intact (a single carrier's publish never
        // retracts a sibling it does not know about). A companion identity change
        // (the `.tsx` → `.jsx` extension flip on a script-kind correction) must
        // retract the superseded ready entry: a stale entry stays resolvable
        // through `ready_files`, joins the tsserver Program, and tsserver's
        // output-file membership check then excludes the current same-stem
        // companion from the configured project.
        OwnedSetScope::SourceDelta => {
            let mut superseded: Vec<String> = Vec::new();
            if !groups.is_empty() {
                let batch_providers: std::collections::HashSet<&str> = batch
                    .owned_sources
                    .iter()
                    .map(|o| o.provider_uri.as_str())
                    .collect();
                // Owned-row references each prior provider loses with the touched
                // sources' old rows.
                let mut lost: std::collections::HashMap<&str, usize> =
                    std::collections::HashMap::new();
                for (source_uri, _) in &groups {
                    for row in project.owned_rows_of(source_uri).unwrap_or_default() {
                        *lost.entry(row.provider_uri.as_str()).or_default() += 1;
                    }
                }
                for (provider_uri, lost) in lost {
                    let still_owned = batch_providers.contains(provider_uri)
                        || project.owned_ref_count(provider_uri) > lost;
                    if !still_owned && project.ready.contains_key(provider_uri) {
                        superseded.push(provider_uri.to_owned());
                    }
                }
                superseded.sort();
            }
            for (source_uri, rows) in groups {
                ops.push(JournalOp::OwnedPut {
                    project: project_uri.to_owned(),
                    source_uri,
                    rows,
                });
            }
            for provider_uri in superseded {
                ops.push(JournalOp::ReadyDel {
                    project: project_uri.to_owned(),
                    provider_uri,
                });
            }
            for (provider_uri, file) in ready_entries {
                ops.push(JournalOp::ReadyPut {
                    project: project_uri.to_owned(),
                    provider_uri,
                    file,
                });
            }
        }
    }
    ops
}

/// The test-only compaction fault slot: `compact` moves the writer's armed fault
/// here for the duration of the generation write, so the write's step seams can
/// consume it without re-entering the writer guard.
#[cfg(test)]
mod compaction_fault {
    use super::CommitFault;
    use std::cell::Cell;

    thread_local! {
        static ARMED: Cell<Option<CommitFault>> = const { Cell::new(None) };
    }

    /// Restores an unconsumed fault to the writer slot on drop.
    pub(super) struct Armed<'a>(&'a mut Option<CommitFault>);

    impl Drop for Armed<'_> {
        fn drop(&mut self) {
            *self.0 = ARMED.with(Cell::take);
        }
    }

    pub(super) fn arm(slot: &mut Option<CommitFault>) -> Armed<'_> {
        ARMED.with(|armed| armed.set(slot.take()));
        Armed(slot)
    }

    pub(super) fn trip(at: CommitFault) -> std::io::Result<()> {
        ARMED.with(|armed| {
            if armed.get() == Some(at) {
                armed.set(None);
                return Err(std::io::Error::other(format!(
                    "injected commit fault at {at:?}"
                )));
            }
            Ok(())
        })
    }
}

/// An incremental follower of a store's published state — the Rust mirror of the
/// Node plugin's disk reader. [`Self::refresh`] reads the small head, then only the
/// journal bytes appended since the last refresh; it reloads a base only when a
/// compaction changed the generation. Never writes.
#[derive(Debug)]
pub struct PublishedStoreReader {
    dir: PathBuf,
    cursor: Option<StoreCursor>,
    work: ObservedWork,
}

impl PublishedStoreReader {
    /// Follow the store at `dir` (a [`CarrierPublishStore::workspace_dir`]).
    #[must_use]
    pub fn open(dir: impl Into<PathBuf>) -> Self {
        Self {
            dir: dir.into(),
            cursor: None,
            work: ObservedWork::default(),
        }
    }

    /// Apply everything committed since the last refresh. On an error the
    /// previously folded state is kept (fail closed: last good, never torn).
    pub fn refresh(&mut self) -> std::io::Result<()> {
        let Some(head) = read_head(&self.dir)? else {
            self.cursor = None;
            return Ok(());
        };
        if let Some(cursor) = self.cursor.as_mut() {
            if cursor.follows(&head) {
                // Records already applied before a failure are committed records;
                // the cursor stays at the last one that verified.
                match cursor.tail(&self.dir, &mut self.work) {
                    Ok(_) => return Ok(()),
                    Err(e)
                        if !matches!(
                            e.kind(),
                            std::io::ErrorKind::UnexpectedEof | std::io::ErrorKind::NotFound
                        ) =>
                    {
                        return Err(e)
                    }
                    Err(_) => {}
                }
            }
        }
        if let Some(cursor) = load_published(&self.dir, &mut self.work)? {
            self.cursor = Some(cursor);
        }
        Ok(())
    }

    /// The folded published state, or `None` before anything was published.
    #[must_use]
    pub fn manifest(&self) -> Option<Manifest> {
        self.cursor.as_ref().map(|c| c.state.to_manifest())
    }

    /// The folded epoch, or `None` before anything was published.
    #[must_use]
    pub fn epoch(&self) -> Option<u64> {
        self.cursor.as_ref().map(|c| c.state.epoch)
    }

    /// The deterministic work this reader performed so far. OPTIONAL measurement
    /// state (test and `semantic-observe` builds only).
    #[cfg(any(test, feature = "semantic-observe"))]
    #[must_use]
    pub fn work(&self) -> StoreWork {
        self.work
    }
}

/// The carrier store as the tsserver plugin reads it, as the publication
/// authority of every carrier byte that engine reads out of band.
///
/// Reads follow the published state — every writer's commits, not only this
/// process's — through an incremental [`PublishedStoreReader`], so a query
/// settles against what the plugin can serve, never against a local record of
/// what this process registered.
pub struct CarrierStorePublications {
    reader: parking_lot::Mutex<PublishedStoreReader>,
}

impl CarrierStorePublications {
    /// The authority over the store at `dir` (the directory the plugin is
    /// pointed at).
    #[must_use]
    pub fn open(dir: impl Into<PathBuf>) -> Self {
        Self {
            reader: parking_lot::Mutex::new(PublishedStoreReader::open(dir)),
        }
    }
}

impl verter_type_runtime::provider_query::SurfacePublications for CarrierStorePublications {
    fn position(&self) -> Option<verter_type_runtime::provider_query::PublicationPosition> {
        let mut reader = self.reader.lock();
        reader.refresh().ok()?;
        let cursor = reader.cursor.as_ref()?;
        Some(verter_type_runtime::provider_query::PublicationPosition {
            instance: std::sync::Arc::from(cursor.instance.as_str()),
            epoch: cursor.state.epoch,
        })
    }

    fn attest(&self, path: &str, bytes: &str) -> verter_type_runtime::provider_query::Attestation {
        use verter_type_runtime::provider_query::{Attestation, PublicationPosition};
        let mut reader = self.reader.lock();
        if reader.refresh().is_err() {
            return Attestation::Unreadable;
        }
        let Some(cursor) = reader.cursor.as_ref() else {
            return Attestation::Unpublished;
        };
        let wanted = verter_span::path::canonicalize_path(path);
        let names = |uri: &str| verter_span::path::canonicalize_path(uri) == wanted;
        // The plugin serves a companion under its provider path, and an IDE
        // companion also under its authored source path.
        let mut rows: Vec<&ReadyFile> = Vec::new();
        for project in cursor.state.projects.values() {
            rows.extend(
                project
                    .ready
                    .iter()
                    .filter(|(provider, _)| names(provider))
                    .map(|(_, file)| file),
            );
            for (source, owned) in project.owned_groups() {
                if !names(source) {
                    continue;
                }
                rows.extend(
                    owned
                        .iter()
                        .filter(|row| row.role == ManifestRole::CarrierIde)
                        .filter_map(|row| project.ready.get(&row.provider_uri)),
                );
            }
        }
        if rows.is_empty() {
            return Attestation::Unpublished;
        }
        let digest = blake3::hash(bytes.as_bytes());
        let mut h16 = [0u8; 16];
        h16.copy_from_slice(&digest.as_bytes()[..16]);
        let content_hash = hex16(&h16);
        if rows.iter().any(|row| row.content_hash != content_hash) {
            return Attestation::Contradicted;
        }
        Attestation::Attested(PublicationPosition {
            instance: std::sync::Arc::from(cursor.instance.as_str()),
            epoch: rows
                .iter()
                .map(|row| row.published_epoch)
                .max()
                .unwrap_or(0),
        })
    }
}

/// A store-instance identity: unique per creation (process, clock and a
/// process-local counter), hashed with the store dir.
fn mint_store_instance(dir: &Path) -> String {
    static CREATED: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_nanos());
    let seed = format!(
        "{}|{}|{nanos}|{}",
        dir.display(),
        std::process::id(),
        CREATED.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
    );
    let digest = blake3::hash(seed.as_bytes());
    let mut h16 = [0u8; 16];
    h16.copy_from_slice(&digest.as_bytes()[..16]);
    hex16(&h16)
}

/// Seed `dir` as a published store whose generation 1 base is `manifest` (with an
/// empty journal) — for tests that drive a real reader against a fixture store.
#[cfg(test)]
pub(crate) fn seed_published_store(dir: &Path, manifest: &Manifest) -> std::io::Result<()> {
    std::fs::create_dir_all(dir)?;
    let base = serde_json::to_vec(manifest).map_err(std::io::Error::other)?;
    write_atomic(dir, &dir.join(snapshot_file(1)), &base)?;
    write_atomic(dir, &dir.join(journal_file(1)), b"")?;
    let head = StoreHead {
        format: STORE_FORMAT,
        generation: 1,
        instance: "seeded".to_owned(),
        host_version: manifest.host_version.clone(),
    };
    let head = serde_json::to_vec(&head).map_err(std::io::Error::other)?;
    write_atomic(dir, &dir.join(HEAD_FILE), &head)
}

/// Render a 16-byte hash as lowercase hex (no prefix) — the manifest
/// `content_hash` / `map_hash` value form.
#[must_use]
fn hex16(hash: &[u8; 16]) -> String {
    let mut s = String::with_capacity(32);
    for b in hash {
        s.push(char::from_digit((b >> 4) as u32, 16).expect("nibble"));
        s.push(char::from_digit((b & 0x0f) as u32, 16).expect("nibble"));
    }
    s
}

/// Atomically write `bytes` to `final_path` via a temp file in `dir` (the SAME
/// directory as the final — a cross-device rename fails otherwise), fsync, then an
/// atomic replace-over-existing rename.
///
/// CROSS-PLATFORM ATOMIC REPLACE: `NamedTempFile::persist` atomically replaces an
/// existing target on every platform — `ReplaceFile`/`MoveFileEx` on Windows (where
/// a plain `std::fs::rename` FAILS when the target exists), `rename(2)` on Unix. The
/// file is `sync_all`'d BEFORE persist so its bytes are durable before the rename
/// makes it visible (the `tempfile` doc notes persist does not itself fsync).
fn write_atomic(dir: &Path, final_path: &Path, bytes: &[u8]) -> std::io::Result<()> {
    let mut tmp = tempfile::NamedTempFile::new_in(dir)?;
    tmp.write_all(bytes)?;
    tmp.flush()?;
    // fsync the file contents before the rename makes it visible (persist does not).
    tmp.as_file().sync_all()?;

    // Atomic replace-over-existing. On Windows `ReplaceFile`/`MoveFileEx` can
    // transiently fail with `PermissionDenied` (or a sharing violation) if another
    // process is momentarily holding the target — the in-process commit path
    // serializes under the writer guard and `writer.lock`, but a reader process
    // holding the target open can still contend. A short bounded retry (NEVER a busy-spin, NEVER
    // unbounded) absorbs that transient; an `AlreadyExists`/genuine error after the
    // retries surfaces. The temp file is preserved across retries (persist returns
    // it on failure).
    let mut current = tmp;
    let mut attempt = 0u32;
    loop {
        match current.persist(final_path) {
            Ok(_) => return Ok(()),
            Err(e) => {
                attempt += 1;
                let transient = matches!(
                    e.error.kind(),
                    std::io::ErrorKind::PermissionDenied | std::io::ErrorKind::AlreadyExists
                );
                if attempt >= 5 || !transient {
                    return Err(e.error);
                }
                current = e.file;
                std::thread::sleep(std::time::Duration::from_millis(2 * u64::from(attempt)));
            }
        }
    }
}

/// Best-effort fsync of a directory so a rename into it is durable. A no-op where
/// the platform does not support opening / fsyncing a directory (e.g. Windows,
/// where `File::open` on a dir fails) — durability there rides the file's own
/// `sync_all` plus the OS journal, which is the documented best-effort contract.
fn fsync_dir(dir: &Path) {
    if let Ok(handle) = std::fs::File::open(dir) {
        let _ = handle.sync_all();
    }
}

#[cfg(test)]
#[path = "carrier_publish_store_tests.rs"]
mod tests;

#[cfg(test)]
#[path = "carrier_publish_journal_tests.rs"]
mod journal_tests;
