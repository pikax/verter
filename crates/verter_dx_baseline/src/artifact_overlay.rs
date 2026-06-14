//! The versioned mutable artifact overlay.
//!
//! `MaterializedWorkspace` is split into an immutable scaffold (source files,
//! tsconfig, vendored shims, tool roots) and this mutable, version-stamped
//! overlay of per-edit generated artifacts (`.vue.tsx`, source maps, `.vue.ts`
//! twins). The overlay is the single authority that prevents the differential
//! from ever comparing `verter@editN` with `baseline@edit0`: a probe at version
//! `V` on a generated artifact is refused with `baseline_artifact_stale` unless
//! the overlay holds THAT SPECIFIC generated path at `>= V`. Freshness is
//! tracked per `(authored URI, generated path)`: two artifacts of one authored
//! document (the `.vue.tsx` entry and the `.vue.ts` twin) advance independently,
//! so a sync that refreshes only the twin leaves a probe on the still-stale
//! entry refused — an authored-URI-coarse gate would wrongly clear it.
//!
//! This module is pure bookkeeping. It decides which `TypeProvider` operation
//! each synced file needs (`open`/`load`/`update`) but never touches a provider
//! — the bridge performs the I/O against the decisions returned here.

use std::collections::{HashMap, HashSet};

use verter_span::path::canonicalize_path;
use verter_type_runtime::file_uri_to_path;

use crate::protocol::{AppliedSync, BaselineFile, ChangedTwin, FileRole, SyncAction, Version};

/// Result of testing a probe's version against the overlay (for an authored URI
/// or a specific generated path, depending on the probe).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProbeStatus {
    /// The overlay holds the probed key at `>= requested`.
    Fresh,
    /// Nothing recorded for the probed key, or only a version `< requested`.
    Stale { have: Option<Version> },
}

/// Map a generated/provider-graph path back to its authored `.vue` URI.
///
/// `Foo.vue.tsx` / `Foo.vue.jsx` / `Foo.vue.ts` all map to `Foo.vue`. Any other
/// path (a vendored shim, a `.ts` lib, a plain `.ts`) maps to `None` — those are
/// import-resolution support files and gate no probe.
pub fn authored_uri_for(generated_path: &str) -> Option<String> {
    for ext in [".tsx", ".jsx", ".ts"] {
        if let Some(stem) = generated_path.strip_suffix(ext) {
            if stem.ends_with(".vue") {
                return Some(stem.to_string());
            }
        }
    }
    None
}

/// Canonical key for an authored-URI version slot.
///
/// The protocol addresses one document two ways that MUST reconcile to a single
/// key: the path-derived authored URI from a generated artifact path
/// (`/abs/Foo.vue`, produced by [`authored_uri_for`] during `open`/`sync`) and
/// the protocol `uri` of a probe (`file:///abs/Foo.vue`). Both are routed
/// through `file_uri_to_path` (strips the `file://` scheme; resolves Windows
/// drive / UNC / localhost authorities and percent-encoding) then
/// `canonicalize_path` (slash + drive-case normalization), so the open-stamp and
/// the query/diagnostics gate never address divergent keys for the same file.
fn version_key(raw: &str) -> String {
    canonicalize_path(&file_uri_to_path(raw))
}

/// Versioned overlay state for one bridge session.
#[derive(Debug, Default)]
pub struct ArtifactOverlay {
    /// Authored `.vue` URI → newest synced version. A document-level rollup: a
    /// URI is at version `V` when ANY of its generated artifacts reached `V`.
    /// Too coarse to gate a probe (see `path_versions`); retained as the
    /// document-level observable the write-side gating is characterized against.
    uri_versions: HashMap<String, Version>,
    /// Generated path → newest synced version. THE path-precise probe authority:
    /// a probe at version `V` on a generated artifact is fresh only when THIS
    /// exact path was synced at `>= V`. Two artifacts of one authored document
    /// (the `.vue.tsx` entry and the `.vue.ts` twin) advance independently, so a
    /// sync that refreshes only the twin must not clear a probe for the still-
    /// stale entry — the URI-level rollup cannot make that decision.
    path_versions: HashMap<String, Version>,
    /// Generated paths currently open in the provider (so we pick
    /// `update_file` over a re-`open`/`load`).
    open_paths: HashSet<String>,
    /// Authored `.vue` URI → newest `sourceMapIdentity`, when one was supplied.
    /// `None` records a map-absent sync (kept so map-absent can be surfaced,
    /// never crash).
    source_map_identity: HashMap<String, Option<String>>,
}

impl ArtifactOverlay {
    pub fn new() -> Self {
        Self::default()
    }

    // The accessors below are the overlay's observable-state query surface —
    // exercised by this module's unit tests and reserved for the runner. Those
    // not reached from `main` are `#[allow(dead_code)]` so the bin-only profile
    // does not flag them.

    /// Whether a generated path is currently open in the provider.
    #[allow(dead_code)]
    pub fn is_open(&self, path: &str) -> bool {
        self.open_paths.contains(path)
    }

    /// Newest synced version for an authored `.vue` URI, if any.
    #[allow(dead_code)]
    pub fn latest_version(&self, uri: &str) -> Option<Version> {
        self.uri_versions.get(&version_key(uri)).copied()
    }

    /// Newest `sourceMapIdentity` recorded for `uri` (inner `None` = a sync
    /// that carried no source map — the map-absent case).
    #[allow(dead_code)]
    pub fn source_map_identity(&self, uri: &str) -> Option<&Option<String>> {
        self.source_map_identity.get(&version_key(uri))
    }

    /// Whether a PRESENT source map was recorded for `uri` (a sync that carried a
    /// `sourceMapIdentity`). A map-absent sync (inner `None`) and a never-synced
    /// URI both report `false`, so a `requiresSourceMap` probe is refused in
    /// either case.
    pub fn source_map_present(&self, uri: &str) -> bool {
        matches!(
            self.source_map_identity.get(&version_key(uri)),
            Some(Some(_))
        )
    }

    /// Compare a recorded version against a requested probe `version`.
    ///
    /// An unversioned/sentinel probe (LSP uses a negative version such as `-1`
    /// for "no known version") cannot be proven fresh against any synced
    /// artifact — refuse it explicitly rather than letting `have >= -1` pass.
    fn freshness(have: Option<Version>, version: Version) -> ProbeStatus {
        if version < 0 {
            return ProbeStatus::Stale { have };
        }
        match have {
            Some(h) if h >= version => ProbeStatus::Fresh,
            _ => ProbeStatus::Stale { have },
        }
    }

    /// Test a probe at `version` against the authored-URI rollup.
    ///
    /// COARSE: a URI is fresh once ANY of its generated artifacts reached
    /// `version`. This cannot gate a real probe — a sync that refreshes only the
    /// `.vue.ts` twin would wrongly mark the `.vue.tsx` entry fresh. The
    /// production staleness gate is the path-precise [`probe_path_status`]; this
    /// is the document-level observable the write-side gating is characterized
    /// against (and is reserved for the runner).
    ///
    /// [`probe_path_status`]: Self::probe_path_status
    #[allow(dead_code)]
    pub fn probe_status(&self, uri: &str, version: Version) -> ProbeStatus {
        Self::freshness(self.uri_versions.get(&version_key(uri)).copied(), version)
    }

    /// Test a probe at `version` against the SPECIFIC generated artifact `path`.
    ///
    /// The authoritative staleness gate: the differential may only run a probe on
    /// a generated artifact the overlay actually holds at `>= version`. Unlike
    /// the URI rollup [`probe_status`], it refuses a probe on an artifact that was
    /// not itself synced at `version` even when a SIBLING artifact of the same
    /// authored document was — so a sync refreshing only the `.vue.ts` twin
    /// leaves a probe on the still-stale `.vue.tsx` entry refused.
    ///
    /// [`probe_status`]: Self::probe_status
    pub fn probe_path_status(&self, path: &str, version: Version) -> ProbeStatus {
        Self::freshness(self.path_versions.get(&version_key(path)).copied(), version)
    }

    /// Decide the provider action for each file WITHOUT mutating overlay state.
    ///
    /// The bridge applies these decisions to the provider and only then commits
    /// the corresponding version/open-state through [`commit_open`] /
    /// [`commit_sync`]. Keeping planning side-effect-free is what makes a
    /// provider failure leave the overlay un-advanced: nothing is marked fresh
    /// before the provider has accepted it.
    ///
    /// [`commit_open`]: Self::commit_open
    /// [`commit_sync`]: Self::commit_sync
    pub fn plan(&self, files: &[BaselineFile]) -> Vec<AppliedSync> {
        let mut newly_open: HashSet<&str> = HashSet::new();
        let mut applied = Vec::with_capacity(files.len());
        for file in files {
            let already_open =
                self.open_paths.contains(&file.path) || newly_open.contains(file.path.as_str());
            let action = if already_open {
                SyncAction::Updated
            } else if file.role == FileRole::Entry {
                newly_open.insert(file.path.as_str());
                SyncAction::Opened
            } else {
                // api / support: import-resolution only — load, do not editor-open.
                SyncAction::Loaded
            };
            applied.push(AppliedSync {
                path: file.path.clone(),
                action,
            });
        }
        applied
    }

    /// Commit an initial `open` snapshot at `version`, AFTER the provider has
    /// accepted every file.
    ///
    /// The initial `open` is a single-version snapshot: every document's
    /// artifacts are present at the baseline version, so each authored URI AND
    /// each of its generated artifact paths advances to it.
    pub fn commit_open(
        &mut self,
        files: &[BaselineFile],
        applied: &[AppliedSync],
        version: Version,
    ) {
        for (file, a) in files.iter().zip(applied) {
            if a.action == SyncAction::Opened {
                self.open_paths.insert(file.path.clone());
            }
            if let Some(uri) = authored_uri_for(&file.path) {
                self.bump_version(&uri, version);
                // Record this exact generated artifact at the baseline version —
                // the path-precise probe authority. At open every artifact (entry
                // AND twin) is genuinely present at `version`, so each is stamped.
                self.bump_path_version(&file.path, version);
                // Record the entry artifact's source-map presence at open time so
                // an edit-0 `requiresSourceMap` probe is not falsely refused for
                // an artifact that DOES have a map. The entry (`.vue.tsx`) carries
                // the IDE compiled-code map that gates the refusal; the api twin
                // (`.vue.ts`) shares the same authored URI but its map must not
                // override the entry's, so only the entry role records here.
                if file.role == FileRole::Entry {
                    self.source_map_identity
                        .insert(version_key(&uri), file.source_map_identity.clone());
                }
            }
        }
    }

    /// Commit a per-edit `syncArtifacts` for one authored `uri`, AFTER the
    /// provider has accepted every file.
    ///
    /// Versioning is per `(authored URI, generated path)`: the committed `uri`
    /// AND each applied artifact path that maps to it advance to `version` ONLY
    /// when an artifact for `uri` itself was in `files` (an empty or sibling-only
    /// payload never marks `uri` fresh), and each explicitly-named twin advances
    /// ITS OWN authored URI and ITS OWN generated path to ITS OWN version. A
    /// sibling artifact present in `files` only for import-resolution refresh does
    /// NOT inherit the edited document's version — broadcasting one document's
    /// version across siblings is exactly the false-fresh bug (a parent edit at
    /// v5 must never mark a child fresh-through-v5). Path-precise recording also
    /// keeps a sync that refreshes only the `.vue.ts` twin from clearing a probe
    /// on the same document's still-stale `.vue.tsx` entry.
    pub fn commit_sync(
        &mut self,
        uri: &str,
        version: Version,
        files: &[BaselineFile],
        applied: &[AppliedSync],
        changed_twins: &[ChangedTwin],
        source_map_identity: Option<String>,
    ) {
        for (file, a) in files.iter().zip(applied) {
            if a.action == SyncAction::Opened {
                self.open_paths.insert(file.path.clone());
            }
            // A file's derived authored URI is deliberately NOT advanced to
            // `version` here (see the per-URI rule above).
        }
        // The edited document advances to its own LSP version ONLY when an
        // artifact for THIS authored URI was actually part of the synced files.
        // `commit_sync` runs only after `apply_files` succeeded, so presence in
        // `files` means the artifact was applied to the provider. An empty or
        // sibling-only payload leaves the queried URI un-advanced: the provider
        // still holds the pre-edit content for that specific artifact, so a later
        // vN probe for it must still be refused stale. Marking it fresh here is
        // the false-fresh bug — a vN probe would skip the stale gate while the
        // provider answers from edit-0 content.
        let uri_key = version_key(uri);
        // Record the SPECIFIC generated paths that were applied for this URI at
        // `version` — the path-precise probe authority. Only the artifacts the
        // sync actually carried advance; a sibling artifact (a DIFFERENT authored
        // document) present only for an import-resolution refresh is skipped here,
        // and a same-document artifact absent from this payload (e.g. the entry
        // when only the twin synced) keeps its prior, older version. That is what
        // makes a v2 probe on a still-edit-0 entry refused even though the twin
        // advanced the shared URI to v2.
        let mut authored_artifact_applied = false;
        for file in files {
            if authored_uri_for(&file.path).is_some_and(|u| version_key(&u) == uri_key) {
                authored_artifact_applied = true;
                self.bump_path_version(&file.path, version);
            }
        }
        if authored_artifact_applied {
            self.bump_version(uri, version);
            // The source map belongs to the URI's own artifact; record it only
            // when that artifact was synced (never clobber an un-touched URI).
            self.source_map_identity
                .insert(uri_key, source_map_identity);
        }
        // A named twin advances its OWN authored URI to its OWN version, but ONLY
        // when the twin's exact generated file was present in `files` — i.e. was
        // actually applied to the provider (`commit_sync` runs after `apply_files`
        // succeeded, so presence in `files` means the artifact reached the
        // provider). A twin named in `changed_public_api_twins` but absent from
        // `files` (including any `files: []` sync) was never pushed, so it stays
        // stale: marking it fresh would skip the stale gate while the provider
        // still answers from pre-edit content — the same false-fresh class as a
        // sibling-only sync of the queried URI.
        for twin in changed_twins {
            let twin_key = version_key(&twin.path);
            let twin_file_applied = files.iter().any(|f| version_key(&f.path) == twin_key);
            if !twin_file_applied {
                continue;
            }
            // The twin's exact generated path is fresh at its OWN version — its
            // file was applied this sync. Its authored-URI rollup advances too,
            // but never to the edited document's `version`.
            self.bump_path_version(&twin.path, twin.version);
            if let Some(twin_uri) = authored_uri_for(&twin.path) {
                self.bump_version(&twin_uri, twin.version);
            }
        }
    }

    /// Plan + commit an `open` in one step. No provider is in the loop here, so
    /// this is the overlay's own test convenience; the bridge instead calls
    /// [`plan`] → apply to the provider → [`commit_open`].
    ///
    /// [`plan`]: Self::plan
    /// [`commit_open`]: Self::commit_open
    #[cfg(test)]
    pub fn open(&mut self, files: &[BaselineFile], version: Version) -> Vec<AppliedSync> {
        let applied = self.plan(files);
        self.commit_open(files, &applied, version);
        applied
    }

    /// Plan + commit a `syncArtifacts` in one step (overlay test convenience).
    #[cfg(test)]
    pub fn sync(
        &mut self,
        uri: &str,
        version: Version,
        files: &[BaselineFile],
        changed_twins: &[ChangedTwin],
        source_map_identity: Option<String>,
    ) -> Vec<AppliedSync> {
        let applied = self.plan(files);
        self.commit_sync(
            uri,
            version,
            files,
            &applied,
            changed_twins,
            source_map_identity,
        );
        applied
    }

    /// Advance an authored URI's rollup version monotonically — a later sync
    /// never downgrades an already-recorded version.
    fn bump_version(&mut self, uri: &str, version: Version) {
        Self::bump(&mut self.uri_versions, uri, version);
    }

    /// Advance a SPECIFIC generated path's version monotonically — a later sync
    /// never downgrades an already-recorded version.
    fn bump_path_version(&mut self, path: &str, version: Version) {
        Self::bump(&mut self.path_versions, path, version);
    }

    /// Monotonically raise `key`'s version in `map` (shared by the URI rollup and
    /// the path-precise store so both can never silently roll a version back).
    fn bump(map: &mut HashMap<String, Version>, key: &str, version: Version) {
        let entry = map.entry(version_key(key)).or_insert(version);
        if version > *entry {
            *entry = version;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::protocol::FileRole;

    fn file(path: &str, role: FileRole) -> BaselineFile {
        BaselineFile {
            path: path.to_string(),
            content: "x".to_string(),
            role,
            source_map_identity: None,
        }
    }

    fn file_with_map(path: &str, role: FileRole, map: Option<&str>) -> BaselineFile {
        BaselineFile {
            path: path.to_string(),
            content: "x".to_string(),
            role,
            source_map_identity: map.map(String::from),
        }
    }

    #[test]
    fn authored_uri_maps_vue_artifacts_only() {
        assert_eq!(
            authored_uri_for("/ws/Foo.vue.tsx").as_deref(),
            Some("/ws/Foo.vue")
        );
        assert_eq!(
            authored_uri_for("/ws/Foo.vue.jsx").as_deref(),
            Some("/ws/Foo.vue")
        );
        assert_eq!(
            authored_uri_for("/ws/Foo.vue.ts").as_deref(),
            Some("/ws/Foo.vue")
        );
        // Negative: plain .ts / shims do not map to an authored URI.
        assert_eq!(authored_uri_for("/ws/util.ts"), None);
        assert_eq!(authored_uri_for("/ws/node_modules/vue/index.d.ts"), None);
    }

    #[test]
    fn open_sets_baseline_version_and_open_state() {
        let mut o = ArtifactOverlay::new();
        let applied = o.open(
            &[
                file("/ws/Foo.vue.tsx", FileRole::Entry),
                file("/ws/Foo.vue.ts", FileRole::Api),
                file("/ws/node_modules/vue/index.d.ts", FileRole::Support),
            ],
            1,
        );
        assert_eq!(applied[0].action, SyncAction::Opened);
        assert_eq!(applied[1].action, SyncAction::Loaded);
        assert_eq!(applied[2].action, SyncAction::Loaded);
        assert!(o.is_open("/ws/Foo.vue.tsx"));
        assert!(!o.is_open("/ws/Foo.vue.ts"));
        assert_eq!(o.latest_version("/ws/Foo.vue"), Some(1));
    }

    #[test]
    fn probe_is_fresh_at_or_below_synced_version_and_stale_above() {
        let mut o = ArtifactOverlay::new();
        o.open(&[file("/ws/Foo.vue.tsx", FileRole::Entry)], 2);
        assert_eq!(o.probe_status("/ws/Foo.vue", 1), ProbeStatus::Fresh);
        assert_eq!(o.probe_status("/ws/Foo.vue", 2), ProbeStatus::Fresh);
        assert_eq!(
            o.probe_status("/ws/Foo.vue", 3),
            ProbeStatus::Stale { have: Some(2) }
        );
    }

    #[test]
    fn probe_for_unknown_uri_is_stale_with_no_version() {
        let o = ArtifactOverlay::new();
        assert_eq!(
            o.probe_status("file:///never.vue", 1),
            ProbeStatus::Stale { have: None }
        );
    }

    #[test]
    fn sync_updates_already_open_entry_and_advances_version() {
        let mut o = ArtifactOverlay::new();
        o.open(&[file("/ws/Foo.vue.tsx", FileRole::Entry)], 1);
        // Edit -> version 2 -> sync.
        let applied = o.sync(
            "/ws/Foo.vue",
            2,
            &[file("/ws/Foo.vue.tsx", FileRole::Entry)],
            &[],
            Some("map-hash-abc".to_string()),
        );
        assert_eq!(applied.len(), 1);
        // Already open => Updated, not re-Opened.
        assert_eq!(applied[0].action, SyncAction::Updated);
        assert_eq!(o.probe_status("/ws/Foo.vue", 2), ProbeStatus::Fresh);
        assert_eq!(
            o.probe_status("/ws/Foo.vue", 3),
            ProbeStatus::Stale { have: Some(2) }
        );
        assert_eq!(
            o.source_map_identity("/ws/Foo.vue"),
            Some(&Some("map-hash-abc".to_string()))
        );
    }

    #[test]
    fn sync_loads_new_support_file_and_opens_new_entry() {
        let mut o = ArtifactOverlay::new();
        o.open(&[file("/ws/Foo.vue.tsx", FileRole::Entry)], 1);
        let applied = o.sync(
            "/ws/Foo.vue",
            2,
            &[
                file("/ws/Bar.vue.ts", FileRole::Api), // newly discovered twin
                file("/ws/Bar.vue.tsx", FileRole::Entry), // newly opened entry
            ],
            &[],
            None,
        );
        assert_eq!(applied[0].action, SyncAction::Loaded);
        assert_eq!(applied[1].action, SyncAction::Opened);
        assert!(o.is_open("/ws/Bar.vue.tsx"));
        // Bar's artifacts were pushed to the provider, but Foo's sync version
        // must NOT broadcast onto the sibling Bar URI (per-URI versioning): Bar
        // gates stale until its OWN sync names it.
        assert_eq!(
            o.probe_status("/ws/Bar.vue", 2),
            ProbeStatus::Stale { have: None }
        );
    }

    #[test]
    fn map_absent_sync_is_recorded_not_dropped() {
        let mut o = ArtifactOverlay::new();
        o.sync(
            "/ws/Foo.vue",
            1,
            &[file("/ws/Foo.vue.tsx", FileRole::Entry)],
            &[],
            None,
        );
        // Recorded as present-key with inner None — distinguishable from "never synced".
        assert_eq!(o.source_map_identity("/ws/Foo.vue"), Some(&None));
        assert_eq!(o.source_map_identity("/ws/Other.vue"), None);
        // A map-absent sync reports no PRESENT map; a never-synced URI likewise.
        assert!(!o.source_map_present("/ws/Foo.vue"));
        assert!(!o.source_map_present("/ws/Other.vue"));
    }

    #[test]
    fn present_source_map_reports_present() {
        let mut o = ArtifactOverlay::new();
        o.sync(
            "/ws/Foo.vue",
            1,
            &[file("/ws/Foo.vue.tsx", FileRole::Entry)],
            &[],
            Some("map-hash".to_string()),
        );
        assert!(o.source_map_present("/ws/Foo.vue"));
        // Negative: a different URI still reports absent.
        assert!(!o.source_map_present("/ws/Other.vue"));
    }

    // ── the initial open records each entry's source-map presence ────────────

    #[test]
    fn open_records_entry_source_map_presence() {
        let mut o = ArtifactOverlay::new();
        o.open(
            &[
                file_with_map("/ws/Foo.vue.tsx", FileRole::Entry, Some("map-0")),
                file_with_map("/ws/Bar.vue.tsx", FileRole::Entry, None),
            ],
            1,
        );
        // The entry that materialized WITH a map reports its map present at
        // edit-0 — a `requiresSourceMap` probe at v1 must not be falsely refused.
        assert!(o.source_map_present("/ws/Foo.vue"));
        // The entry that materialized WITHOUT a map reports absent.
        assert!(!o.source_map_present("/ws/Bar.vue"));
    }

    #[test]
    fn open_does_not_let_twin_clobber_entry_source_map_presence() {
        let mut o = ArtifactOverlay::new();
        // Both the entry (.vue.tsx, has a map) and the api twin (.vue.ts, no map)
        // derive the SAME authored URI. The entry's map is authoritative for the
        // `requiresSourceMap` gate — the twin must not erase it.
        o.open(
            &[
                file_with_map("/ws/Foo.vue.tsx", FileRole::Entry, Some("map-0")),
                file_with_map("/ws/Foo.vue.ts", FileRole::Api, None),
            ],
            1,
        );
        assert!(
            o.source_map_present("/ws/Foo.vue"),
            "the api twin (no map) must not clobber the entry's recorded map"
        );
    }

    #[test]
    fn version_never_downgrades() {
        let mut o = ArtifactOverlay::new();
        // Each sync carries Foo's own artifact, so its version actually advances.
        o.sync(
            "/ws/Foo.vue",
            5,
            &[file("/ws/Foo.vue.tsx", FileRole::Entry)],
            &[],
            None,
        );
        // A late, lower-versioned sync must not roll the URI back.
        o.sync(
            "/ws/Foo.vue",
            3,
            &[file("/ws/Foo.vue.tsx", FileRole::Entry)],
            &[],
            None,
        );
        assert_eq!(o.latest_version("/ws/Foo.vue"), Some(5));
        assert_eq!(
            o.probe_status("/ws/Foo.vue", 4),
            ProbeStatus::Fresh,
            "a probe at v4 stays fresh — overlay holds v5"
        );
    }

    // ── per-authored-URI versioning ──────────────────────────────────────────

    #[test]
    fn parent_sync_does_not_broadcast_its_version_to_a_sibling_child_uri() {
        let mut o = ArtifactOverlay::new();
        // A parent edit at v5 pushes the refreshed child twin for import
        // resolution, but does NOT name it as an independently-versioned twin.
        o.sync(
            "/ws/Parent.vue",
            5,
            &[
                file("/ws/Parent.vue.tsx", FileRole::Entry),
                file("/ws/Child.vue.ts", FileRole::Api),
            ],
            &[],
            None,
        );
        // The parent's own URI is fresh at v5.
        assert_eq!(o.probe_status("/ws/Parent.vue", 5), ProbeStatus::Fresh);
        // The child URI must NOT inherit the parent's v5. A child probe at v2
        // (after a later child edit, before the child's own sync) is refused.
        assert_eq!(
            o.probe_status("/ws/Child.vue", 2),
            ProbeStatus::Stale { have: None }
        );
        // The child then syncs at its OWN version → fresh.
        o.sync(
            "/ws/Child.vue",
            2,
            &[file("/ws/Child.vue.tsx", FileRole::Entry)],
            &[],
            None,
        );
        assert_eq!(o.probe_status("/ws/Child.vue", 2), ProbeStatus::Fresh);
        // Negative: the parent's v5 never made the child fresh-through-v5.
        assert_eq!(
            o.probe_status("/ws/Child.vue", 5),
            ProbeStatus::Stale { have: Some(2) }
        );
    }

    #[test]
    fn named_changed_twin_advances_only_its_own_uri_at_its_own_version() {
        let mut o = ArtifactOverlay::new();
        // A parent edit at v5 pushes the refreshed child twin (its file IS in the
        // synced/applied payload) and names it with the child's OWN version 1.
        o.sync(
            "/ws/Parent.vue",
            5,
            &[
                file("/ws/Parent.vue.tsx", FileRole::Entry),
                file("/ws/Child.vue.ts", FileRole::Api),
            ],
            &[ChangedTwin {
                path: "/ws/Child.vue.ts".to_string(),
                version: 1,
            }],
            None,
        );
        // The child is fresh only up to its own named version (1), never v5.
        assert_eq!(o.probe_status("/ws/Child.vue", 1), ProbeStatus::Fresh);
        assert_eq!(
            o.probe_status("/ws/Child.vue", 2),
            ProbeStatus::Stale { have: Some(1) }
        );
        // Negative: the parent's v5 did not leak onto the child.
        assert_eq!(
            o.probe_status("/ws/Child.vue", 5),
            ProbeStatus::Stale { have: Some(1) }
        );
    }

    #[test]
    fn named_twin_advances_only_when_its_own_file_was_applied() {
        let mut o = ArtifactOverlay::new();
        // (a) A parent edit names a child twin in changedPublicApiTwins but the
        // sync carries NO files — the twin's own generated file was never pushed
        // to the provider. Marking it fresh would be the false-fresh bug: a probe
        // would skip the stale gate while the provider holds pre-edit content.
        o.sync(
            "/ws/Parent.vue",
            5,
            &[],
            &[ChangedTwin {
                path: "/ws/Foo.vue.ts".to_string(),
                version: 4,
            }],
            None,
        );
        // The twin's file was absent → its URI must stay stale at its named version.
        assert_eq!(
            o.probe_status("/ws/Foo.vue", 4),
            ProbeStatus::Stale { have: None },
            "a named twin absent from files must not advance"
        );

        // (b) A later sync DOES carry the twin's exact generated file → it is
        // applied, so the named twin advances to ITS OWN version.
        o.sync(
            "/ws/Parent.vue",
            6,
            &[file("/ws/Foo.vue.ts", FileRole::Api)],
            &[ChangedTwin {
                path: "/ws/Foo.vue.ts".to_string(),
                version: 4,
            }],
            None,
        );
        assert_eq!(
            o.probe_status("/ws/Foo.vue", 4),
            ProbeStatus::Fresh,
            "an applied named twin advances to its own version"
        );
        // Negative: it never inherited the parent's version (6).
        assert_eq!(
            o.probe_status("/ws/Foo.vue", 6),
            ProbeStatus::Stale { have: Some(4) }
        );
    }

    // ── a sync advances the authored URI ONLY when that URI's own
    //    artifact was part of the synced (and applied) files ────────────────

    #[test]
    fn empty_file_sync_does_not_advance_authored_uri() {
        let mut o = ArtifactOverlay::new();
        o.open(&[file("/ws/Foo.vue.tsx", FileRole::Entry)], 1);
        // An edit bumps the LSP version to 5, but the sync payload carries NO
        // artifact for Foo — nothing for Foo was actually pushed to the provider.
        o.sync("/ws/Foo.vue", 5, &[], &[], None);
        // Foo must NOT be fresh at v5: its own artifact never reached the
        // provider, so a v5 probe still hits the edit-0 content and is refused.
        assert_eq!(
            o.probe_status("/ws/Foo.vue", 5),
            ProbeStatus::Stale { have: Some(1) }
        );
        // It stays fresh only up to the version actually applied (v1 from open).
        assert_eq!(o.probe_status("/ws/Foo.vue", 1), ProbeStatus::Fresh);
    }

    #[test]
    fn sibling_only_sync_leaves_unapplied_authored_uri_stale_but_sibling_fresh() {
        let mut o = ArtifactOverlay::new();
        o.open(&[file("/ws/Authored.vue.tsx", FileRole::Entry)], 1);
        // The sync names uri=Authored at v3, but the payload carries ONLY a
        // sibling artifact (also named as a changed twin at its own version) —
        // Authored's own artifact is absent.
        o.sync(
            "/ws/Authored.vue",
            3,
            &[file("/ws/Sibling.vue.tsx", FileRole::Entry)],
            &[ChangedTwin {
                path: "/ws/Sibling.vue.tsx".to_string(),
                version: 3,
            }],
            None,
        );
        // Authored's own artifact was absent → not advanced → a v3 probe for it
        // is still refused stale (it still holds edit-0 content).
        assert_eq!(
            o.probe_status("/ws/Authored.vue", 3),
            ProbeStatus::Stale { have: Some(1) }
        );
        // Guard: the sibling that WAS applied (and named) becomes fresh at v3.
        assert_eq!(o.probe_status("/ws/Sibling.vue", 3), ProbeStatus::Fresh);
    }

    #[test]
    fn empty_sync_does_not_record_a_source_map_for_the_unapplied_uri() {
        let mut o = ArtifactOverlay::new();
        // A present-map sync establishes Foo's map.
        o.sync(
            "/ws/Foo.vue",
            1,
            &[file("/ws/Foo.vue.tsx", FileRole::Entry)],
            &[],
            Some("map-1".to_string()),
        );
        assert!(o.source_map_present("/ws/Foo.vue"));
        // A later empty sync (no Foo artifact) must NOT overwrite Foo's recorded
        // map state — it touched no artifact for Foo.
        o.sync("/ws/Foo.vue", 2, &[], &[], None);
        assert!(
            o.source_map_present("/ws/Foo.vue"),
            "an empty sync must not clobber the previously recorded map presence"
        );
    }

    // ── file:// URI and path form reconcile to one version key ───────────────

    #[test]
    fn file_uri_and_path_forms_resolve_to_one_version_key() {
        let mut o = ArtifactOverlay::new();
        // Stamped by generated PATH on open (no file:// scheme).
        o.open(&[file("/abs/Foo.vue.tsx", FileRole::Entry)], 1);
        // Probed/queried by the file:// authored URI must hit the SAME key.
        assert_eq!(o.probe_status("file:///abs/Foo.vue", 1), ProbeStatus::Fresh);
        assert_eq!(o.latest_version("file:///abs/Foo.vue"), Some(1));
        // Negative: a genuinely-unknown document is still stale with no version.
        assert_eq!(
            o.probe_status("file:///abs/Other.vue", 1),
            ProbeStatus::Stale { have: None }
        );
    }

    // ── negative/sentinel version is refused, not silently fresh ─────────────

    #[test]
    fn negative_sentinel_version_is_refused_not_silently_fresh() {
        let mut o = ArtifactOverlay::new();
        o.sync(
            "/ws/Foo.vue",
            3,
            &[file("/ws/Foo.vue.tsx", FileRole::Entry)],
            &[],
            None,
        );
        // A -1 ("no version") probe must NOT pass just because have(3) >= -1.
        assert_eq!(
            o.probe_status("/ws/Foo.vue", -1),
            ProbeStatus::Stale { have: Some(3) }
        );
        // Negative: a real v3 probe on the same URI is still fresh.
        assert_eq!(o.probe_status("/ws/Foo.vue", 3), ProbeStatus::Fresh);
    }

    // ── the probe is path-precise: sibling artifacts of one document advance
    //    independently ───────────────────────────────────────────────────────

    #[test]
    fn path_probe_refuses_stale_entry_when_only_twin_advanced() {
        let mut o = ArtifactOverlay::new();
        // Open BOTH the entry and its api twin at v1.
        o.open(
            &[
                file("/ws/A.vue.tsx", FileRole::Entry),
                file("/ws/A.vue.ts", FileRole::Api),
            ],
            1,
        );
        // Sync uri=A at v2 carrying ONLY the twin (the entry is not in payload).
        o.sync(
            "/ws/A.vue",
            2,
            &[file("/ws/A.vue.ts", FileRole::Api)],
            &[],
            None,
        );
        // The twin path advanced to v2; the entry path is still the v1 open.
        assert_eq!(o.probe_path_status("/ws/A.vue.ts", 2), ProbeStatus::Fresh);
        assert_eq!(
            o.probe_path_status("/ws/A.vue.tsx", 2),
            ProbeStatus::Stale { have: Some(1) },
            "the un-synced entry path must stay stale at v2"
        );
        // The URI rollup is too coarse to gate a probe — it reports the document
        // "fresh at v2" purely because the twin advanced the shared URI. This is
        // exactly why the production gate must be probe_path_status, not this.
        assert_eq!(
            o.probe_status("/ws/A.vue", 2),
            ProbeStatus::Fresh,
            "the URI rollup is coarse by design; only probe_path_status gates"
        );
    }

    #[test]
    fn path_probe_allows_entry_synced_at_its_version_and_refuses_unsynced_twin() {
        let mut o = ArtifactOverlay::new();
        o.open(
            &[
                file("/ws/A.vue.tsx", FileRole::Entry),
                file("/ws/A.vue.ts", FileRole::Api),
            ],
            1,
        );
        // Sync uri=A at v2 carrying ONLY the entry.
        o.sync(
            "/ws/A.vue",
            2,
            &[file("/ws/A.vue.tsx", FileRole::Entry)],
            &[],
            None,
        );
        assert_eq!(o.probe_path_status("/ws/A.vue.tsx", 2), ProbeStatus::Fresh);
        // The twin was NOT synced at v2 (still the v1 open) → refused at v2.
        assert_eq!(
            o.probe_path_status("/ws/A.vue.ts", 2),
            ProbeStatus::Stale { have: Some(1) }
        );
    }

    #[test]
    fn path_probe_for_unknown_path_is_stale_and_negative_version_is_refused() {
        let mut o = ArtifactOverlay::new();
        o.sync(
            "/ws/Foo.vue",
            3,
            &[file("/ws/Foo.vue.tsx", FileRole::Entry)],
            &[],
            None,
        );
        // A genuinely-unknown generated path is stale with no recorded version.
        assert_eq!(
            o.probe_path_status("/ws/Never.vue.tsx", 3),
            ProbeStatus::Stale { have: None }
        );
        // A -1 ("no version") path probe must NOT pass just because have(3) >= -1.
        assert_eq!(
            o.probe_path_status("/ws/Foo.vue.tsx", -1),
            ProbeStatus::Stale { have: Some(3) }
        );
        // Negative control: a real v3 path probe on the synced artifact is fresh.
        assert_eq!(
            o.probe_path_status("/ws/Foo.vue.tsx", 3),
            ProbeStatus::Fresh
        );
    }

    #[test]
    fn named_twin_path_advances_only_when_its_file_was_applied() {
        let mut o = ArtifactOverlay::new();
        // (a) A parent edit names a child twin but carries NO files — the twin's
        // path was never applied, so it stays stale.
        o.sync(
            "/ws/Parent.vue",
            5,
            &[],
            &[ChangedTwin {
                path: "/ws/Child.vue.ts".to_string(),
                version: 4,
            }],
            None,
        );
        assert_eq!(
            o.probe_path_status("/ws/Child.vue.ts", 4),
            ProbeStatus::Stale { have: None },
            "a named twin path absent from files must not advance"
        );
        // (b) A later sync carries the twin's exact file → its path advances to
        // its OWN version, never the parent's v6.
        o.sync(
            "/ws/Parent.vue",
            6,
            &[file("/ws/Child.vue.ts", FileRole::Api)],
            &[ChangedTwin {
                path: "/ws/Child.vue.ts".to_string(),
                version: 4,
            }],
            None,
        );
        assert_eq!(
            o.probe_path_status("/ws/Child.vue.ts", 4),
            ProbeStatus::Fresh
        );
        assert_eq!(
            o.probe_path_status("/ws/Child.vue.ts", 6),
            ProbeStatus::Stale { have: Some(4) },
            "the named twin path never inherits the parent's version"
        );
    }
}
