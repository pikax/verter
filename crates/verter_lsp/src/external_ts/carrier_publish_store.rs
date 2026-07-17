//! The on-disk content-addressed carrier-snapshot store + atomic manifest — the
//! Rust publish authority the Node `@verter/typescript-plugin` reads SYNCHRONOUSLY
//! (the two processes share NO memory).
//!
//! ## Why this exists (§2.2 of `docs/arch/external-ts-engine-architecture.md`)
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
//! 2. **Split manifest.** `manifest.json` separates `owned_sources` (the full
//!    project-owned carrier set, known the moment ownership resolves) from
//!    `ready_files` (a `provider_uri` enters ONLY after its content blob write
//!    succeeds). The plugin's `getExternalFiles` returns only `ready_files`.
//! 3. **Two-phase publish.** The write step writes every blob + map (idempotent,
//!    skipped if already present). The commit step atomically swaps `manifest.json`
//!    advancing the monotonic `epoch`. The manifest is the LAST thing written and
//!    its swap is atomic, so a reader sees either the old or the new manifest —
//!    never a torn one — and every `ready_files` entry it names has a blob on disk.
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
//! Publishing is purely ADDITIVE to `blobs/`/`maps/`; the manifest swap is the only
//! mutation of the pointer set. A blob a previous manifest could reference is NEVER
//! clobbered (content-addressing guarantees a re-publish of the same content is a
//! no-op, and a new content lands at a new path). GC of unreferenced blobs is OUT
//! OF SCOPE for this sub-block — see the `gc` follow-up note on [`CarrierPublishStore`].

use std::collections::BTreeMap;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

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

/// The on-disk content-addressed carrier-snapshot store + atomic manifest.
///
/// One store per `(host-version, workspace)`; cheap to construct (it only computes
/// paths). `publish_batch` is the sole mutation entry point and is the two-phase
/// publish: blobs/maps first (idempotent), then an atomic manifest swap advancing
/// the monotonic epoch.
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
    /// A process-local epoch advisory used ONLY to seed a fresh manifest's epoch
    /// monotonically when the on-disk manifest is unreadable. The authoritative
    /// epoch is read from the on-disk manifest under the publish lock and advanced;
    /// this guards against an epoch regression if the manifest file is transiently
    /// unreadable.
    last_epoch: AtomicU64,
    /// Serializes the COMMIT STEP (the manifest read-modify-write + atomic swap)
    /// across threads sharing this store. It guarantees two things at once:
    /// (1) the epoch read-increment-write is atomic, so concurrent publishes can
    /// never read the same epoch and both write `epoch + 1` (losing one); and
    /// (2) only one thread is mid-`persist` over `manifest.json` at a time, so the
    /// Windows `ReplaceFile`/`MoveFileEx` atomic-replace never contends on a target
    /// another thread is concurrently replacing (which returns `PermissionDenied`).
    /// The write step (content-addressed blob writes) stays lock-free — each content
    /// hashes to a distinct path and the write is idempotent.
    manifest_lock: parking_lot::Mutex<()>,
}

impl CarrierPublishStore {
    /// Open (compute the paths for) the store for `workspace_root` at this
    /// `host_version`. The store root is under the system temp dir — NEVER the user
    /// workspace. Directories are created lazily on the first publish.
    #[must_use]
    pub fn open(host_version: impl Into<String>, workspace_root: &str) -> Self {
        let host_version = host_version.into();
        let workspace_dir = carrier_store_dir_for(&host_version, workspace_root);
        Self {
            workspace_dir,
            host_version,
            last_epoch: AtomicU64::new(0),
            manifest_lock: parking_lot::Mutex::new(()),
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

    /// The `manifest.json` path.
    #[must_use]
    pub fn manifest_path(&self) -> PathBuf {
        self.workspace_dir.join("manifest.json")
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

    /// Read the current on-disk manifest, returning a fresh default ONLY when the
    /// manifest does not exist (`NotFound`).
    ///
    /// FAIL-CLOSED, NEVER CLOBBER: any OTHER error — a transient read failure, or a
    /// parse error on a present-but-corrupt manifest — is PROPAGATED, not swallowed
    /// into a fresh empty manifest. Swallowing would let the commit step (which
    /// read-modify-writes this manifest) reset to empty and then atomically swap a
    /// manifest carrying ONLY the current project, ERASING every OTHER project's
    /// entries. A propagated error fails the publish instead (the on-disk manifest
    /// stays intact; the next publish retries). A fresh `NotFound` manifest seeds
    /// its epoch from `last_epoch` so a first publish cannot regress the epoch.
    fn read_manifest(&self) -> std::io::Result<Manifest> {
        match std::fs::read(self.manifest_path()) {
            Ok(bytes) => serde_json::from_slice::<Manifest>(&bytes).map_err(|e| {
                std::io::Error::new(
                    std::io::ErrorKind::InvalidData,
                    format!("carrier manifest is present but unparseable: {e}"),
                )
            }),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(self.fresh_manifest()),
            Err(e) => Err(e),
        }
    }

    /// A fresh empty manifest seeded with the process-local epoch advisory and this
    /// store's host version — the fallback when none exists / it is unreadable.
    fn fresh_manifest(&self) -> Manifest {
        Manifest {
            epoch: self.last_epoch.load(Ordering::Acquire),
            host_version: self.host_version.clone(),
            projects: BTreeMap::new(),
        }
    }

    /// Two-phase publish for ONE project. Returns the NEW epoch.
    ///
    /// Write step — write every blob + map (temp-then-rename, idempotent: a
    /// content-addressed blob that already exists is skipped). NOTHING is advertised
    /// yet.
    ///
    /// Commit step — atomically swap `manifest.json` (write `manifest.json.tmp` in
    /// the SAME dir, then atomic-rename over `manifest.json`) advancing the monotonic
    /// `epoch`, after rewriting this project's `owned_sources` and inserting a
    /// `ready_files` entry for every file whose blob write succeeded in the write step.
    ///
    /// The manifest swap is the LAST write and is atomic, so a concurrent reader
    /// sees either the old or new manifest — never a torn one — and every
    /// `ready_files` entry the new manifest names has its blob on disk.
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
                },
            ));
        }

        // ── Commit step: rewrite the project entry + atomic manifest swap ──
        // Serialized across threads (epoch atomicity + non-contending manifest
        // persist on Windows). The write step above ran lock-free.
        let _swap = self.manifest_lock.lock();
        // FAIL-CLOSED: a present-but-corrupt / transiently-unreadable manifest
        // propagates here rather than resetting to empty — so a commit can never
        // clobber other projects' entries by reading a fresh empty manifest.
        let mut manifest = self.read_manifest()?;
        manifest.host_version = self.host_version.clone();
        manifest.epoch += 1;
        let new_epoch = manifest.epoch;
        self.last_epoch.store(new_epoch, Ordering::Release);

        let project = manifest
            .projects
            .entry(batch.project_uri.clone())
            .or_default();
        // Reconcile the owned set per the publish contract.
        match batch.owned_scope {
            // Authoritative: REWRITE the owned set (when the batch carries one — an
            // empty owned set means "publish content only, keep the existing owned
            // set"). The ready-files prune below drops any entry the new owned set
            // no longer admits, so a deleted / no-longer-owned carrier stops being
            // advertised.
            OwnedSetScope::ProjectAuthoritative => {
                if !batch.owned_sources.is_empty() {
                    project.owned_sources = batch.owned_sources.clone();
                }
            }
            // Per-source delta: UNION by `source_uri` — drop the prior rows for
            // every source this batch carries, then append the batch's rows. Sibling
            // carriers' rows stay intact (a single carrier's publish never retracts
            // a sibling it does not know about).
            OwnedSetScope::SourceDelta => {
                if !batch.owned_sources.is_empty() {
                    let touched: std::collections::HashSet<&str> = batch
                        .owned_sources
                        .iter()
                        .map(|o| o.source_uri.as_str())
                        .collect();
                    // The provider URIs the touched sources advertised BEFORE this
                    // delta. A companion identity change (the `.tsx` → `.jsx`
                    // extension flip on a script-kind correction) must retract the
                    // superseded ready entry: a stale entry stays resolvable
                    // through `ready_files`, joins the tsserver Program, and
                    // tsserver's output-file membership check then excludes the
                    // current same-stem companion from the configured project.
                    let prior_provider_uris: std::collections::HashSet<String> = project
                        .owned_sources
                        .iter()
                        .filter(|o| touched.contains(o.source_uri.as_str()))
                        .map(|o| o.provider_uri.clone())
                        .collect();
                    project
                        .owned_sources
                        .retain(|existing| !touched.contains(existing.source_uri.as_str()));
                    project.owned_sources.extend(batch.owned_sources.clone());
                    let current_provider_uris: std::collections::HashSet<&str> = project
                        .owned_sources
                        .iter()
                        .map(|o| o.provider_uri.as_str())
                        .collect();
                    project.ready_files.retain(|provider_uri, _| {
                        !prior_provider_uris.contains(provider_uri.as_str())
                            || current_provider_uris.contains(provider_uri.as_str())
                    });
                }
            }
        }
        // Merge the ready files (a provider_uri re-published advances to its new
        // content/version; a provider_uri only in a prior publish is preserved).
        for (provider_uri, entry) in ready_entries {
            project.ready_files.insert(provider_uri, entry);
        }
        // PRUNE (authoritative publish only): a `ready_files` entry whose
        // `provider_uri` is no longer in the authoritative owned set is no longer
        // owned — remove it so `getExternalFiles` stops advertising the carrier of a
        // deleted / no-owner / now-ambiguous source. A per-source delta does NOT
        // prune (it does not carry the full owned set); sibling retraction is
        // explicit via `retract_sources`.
        if batch.owned_scope == OwnedSetScope::ProjectAuthoritative {
            let owned_provider_uris: std::collections::HashSet<&str> = project
                .owned_sources
                .iter()
                .map(|o| o.provider_uri.as_str())
                .collect();
            project
                .ready_files
                .retain(|provider_uri, _| owned_provider_uris.contains(provider_uri.as_str()));
        }

        let manifest_json = serde_json::to_vec_pretty(&manifest).map_err(std::io::Error::other)?;
        write_atomic(&self.workspace_dir, &self.manifest_path(), &manifest_json)?;
        // Best-effort fsync of the workspace dir so the manifest rename is durable
        // (no-op on platforms without dir fsync).
        fsync_dir(&self.workspace_dir);

        Ok(new_epoch)
    }

    /// Retract one or more SOURCE carriers from a project — the
    /// delete / no-owner / now-ambiguous transition.
    ///
    /// Removes every `owned_sources` row AND every `ready_files` entry whose
    /// `source_uri` is in `source_uris`, then atomically swaps the manifest
    /// (advancing the epoch). After this, `getExternalFiles` no longer advertises
    /// the retracted carrier's companions. A source not present is a no-op for that
    /// source. Returns the new epoch (always advanced, so the plugin re-reads even
    /// for a pure retraction). Blobs are NOT deleted (content-addressed; GC is a
    /// separate sweep) — only the pointer set shrinks.
    ///
    /// This is the explicit counterpart to a [`OwnedSetScope::SourceDelta`] publish:
    /// a per-source publish adds/refreshes its own rows and never prunes siblings,
    /// so a sibling that leaves the project is retracted HERE rather than implied.
    pub fn retract_sources(&self, project_uri: &str, source_uris: &[&str]) -> std::io::Result<u64> {
        std::fs::create_dir_all(&self.workspace_dir)?;
        let _swap = self.manifest_lock.lock();
        let mut manifest = self.read_manifest()?;
        manifest.host_version = self.host_version.clone();
        manifest.epoch += 1;
        let new_epoch = manifest.epoch;
        self.last_epoch.store(new_epoch, Ordering::Release);

        if let Some(project) = manifest.projects.get_mut(project_uri) {
            let retract: std::collections::HashSet<&str> = source_uris.iter().copied().collect();
            // The provider_uris belonging to the retracted sources (drawn from the
            // owned rows about to be removed) — these are the `ready_files` keys to
            // drop.
            let retract_provider_uris: std::collections::HashSet<String> = project
                .owned_sources
                .iter()
                .filter(|o| retract.contains(o.source_uri.as_str()))
                .map(|o| o.provider_uri.clone())
                .collect();
            project
                .owned_sources
                .retain(|o| !retract.contains(o.source_uri.as_str()));
            project
                .ready_files
                .retain(|provider_uri, _| !retract_provider_uris.contains(provider_uri));
        }

        let manifest_json = serde_json::to_vec_pretty(&manifest).map_err(std::io::Error::other)?;
        write_atomic(&self.workspace_dir, &self.manifest_path(), &manifest_json)?;
        fsync_dir(&self.workspace_dir);
        Ok(new_epoch)
    }

    /// Retract a SOURCE carrier from EVERY project that owns it — the
    /// delete / owner-no-longer-resolvable transition where the prior owning project
    /// is not known (a deleted carrier's owner can no longer be resolved). Removes
    /// the source's owned rows + advertised companions from every project entry, then
    /// atomically swaps the manifest. A no-op (still epoch-advancing) when no project
    /// owns the source.
    pub fn retract_source_from_all_projects(&self, source_uri: &str) -> std::io::Result<u64> {
        std::fs::create_dir_all(&self.workspace_dir)?;
        let _swap = self.manifest_lock.lock();
        let mut manifest = self.read_manifest()?;
        manifest.host_version = self.host_version.clone();
        manifest.epoch += 1;
        let new_epoch = manifest.epoch;
        self.last_epoch.store(new_epoch, Ordering::Release);

        for project in manifest.projects.values_mut() {
            let retract_provider_uris: std::collections::HashSet<String> = project
                .owned_sources
                .iter()
                .filter(|o| o.source_uri == source_uri)
                .map(|o| o.provider_uri.clone())
                .collect();
            project.owned_sources.retain(|o| o.source_uri != source_uri);
            project
                .ready_files
                .retain(|provider_uri, _| !retract_provider_uris.contains(provider_uri));
        }

        let manifest_json = serde_json::to_vec_pretty(&manifest).map_err(std::io::Error::other)?;
        write_atomic(&self.workspace_dir, &self.manifest_path(), &manifest_json)?;
        fsync_dir(&self.workspace_dir);
        Ok(new_epoch)
    }

    /// Retract a SOURCE carrier from every project that owns it EXCEPT
    /// `keep_project_uri` — the owner-CHANGE (A→B) prune. The live per-source
    /// publish into the NEW owning project uses [`OwnedSetScope::SourceDelta`]
    /// (union, never prune), so it leaves the source's stale rows in its OLD
    /// project. This removes the source's owned rows + advertised companions from
    /// every OTHER project (so the old project's `getExternalFiles` stops serving
    /// it) while leaving the new owning project's freshly-published rows intact.
    /// Atomically swaps the manifest (advancing the epoch). A no-op (still
    /// epoch-advancing) when no other project owns the source.
    pub fn retract_source_from_all_projects_except(
        &self,
        source_uri: &str,
        keep_project_uri: &str,
    ) -> std::io::Result<u64> {
        std::fs::create_dir_all(&self.workspace_dir)?;
        let _swap = self.manifest_lock.lock();
        let mut manifest = self.read_manifest()?;
        manifest.host_version = self.host_version.clone();
        manifest.epoch += 1;
        let new_epoch = manifest.epoch;
        self.last_epoch.store(new_epoch, Ordering::Release);

        for (project_uri, project) in manifest.projects.iter_mut() {
            // Leave the new owning project's just-published rows intact.
            if project_uri == keep_project_uri {
                continue;
            }
            let retract_provider_uris: std::collections::HashSet<String> = project
                .owned_sources
                .iter()
                .filter(|o| o.source_uri == source_uri)
                .map(|o| o.provider_uri.clone())
                .collect();
            project.owned_sources.retain(|o| o.source_uri != source_uri);
            project
                .ready_files
                .retain(|provider_uri, _| !retract_provider_uris.contains(provider_uri));
        }

        let manifest_json = serde_json::to_vec_pretty(&manifest).map_err(std::io::Error::other)?;
        write_atomic(&self.workspace_dir, &self.manifest_path(), &manifest_json)?;
        fsync_dir(&self.workspace_dir);
        Ok(new_epoch)
    }

    /// Read the current manifest from disk for DIAGNOSTICS / the plugin-equivalent
    /// reader (a fresh default when none exists OR is unreadable).
    ///
    /// Unlike the publish path's [`Self::read_manifest`] — which must fail closed so
    /// a corrupt manifest never clobbers other projects on the next commit — this
    /// read-only view tolerates a corrupt manifest by reporting a fresh empty one
    /// (it never WRITES, so there is nothing to clobber; surfacing "empty" is the
    /// correct diagnostics behaviour for an unreadable manifest).
    #[must_use]
    pub fn current_manifest(&self) -> Manifest {
        self.read_manifest()
            .unwrap_or_else(|_| self.fresh_manifest())
    }
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
    // process is momentarily holding the target — the in-process publish path
    // serializes this under `manifest_lock`, but a second store instance / process
    // could still contend. A short bounded retry (NEVER a busy-spin, NEVER
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
