//! Per-file dependency state and the workspace's reverse-dependency graph.
//!
//! # Dependency-class extensibility policy (§2.17)
//!
//! Any new reverse-discoverable dependency MUST be added as a class field on
//! [`DependencySnapshot`] with explicit lifecycle (which writers replace it;
//! whether `record_parsed_edges` clears it). New classes MUST update
//! [`DependencySnapshot::canonical_dep_union`] for inclusion in the canonical
//! reverse axis OR introduce a separate keying axis (e.g. the stem-axis
//! pattern). Tests MUST cover the new class symmetrically: write/replace,
//! query (canonical + stem if applicable), `remove_file`, and
//! `replace_exact_resolutions` interaction. Adding new dep types into
//! `lazy_resolved` or any other catch-all class is forbidden — the F1.5
//! latent defect (ambient deps silently dropped on parse re-record) was
//! caused by exactly this kind of routing.

use rustc_hash::FxHashMap;
use std::collections::BTreeSet;

use crate::path_matches_prefix;
use crate::types::{
    ExactResolution, ExactResolutionResult, ResolutionContext, ResolvePhase, ResolveRequestKind,
};

/// Coherent per-owner dependency state. Each class is owned by exactly one
/// kind of writer event; the canonical reverse axis is maintained via
/// union-diff so cross-class state is self-consistent.
///
/// # Class lifecycle (binding, R4/R5 — aligned with host's `cc.import_routes` lifecycle)
///
/// **Caller contract (binding for plugins/bundlers):** `record_parsed_edges`
/// clears `exact_resolved` and `exact_resolutions`. Bundlers MUST re-call
/// `set_import_dependencies` after every successful upsert (including
/// byte-identical fast-path returns) to repopulate exact resolutions. This
/// matches host's `cc.import_routes.clear()` lifecycle at
/// `host_upsert.rs:170`. A bundler that forgets to re-call after a re-upsert
/// silently loses its resolutions — same as today's host behaviour.
///
/// - `parsed_resolved`: replaced wholesale by every `record_parsed_edges`.
/// - `parsed_unresolved_relatives`: PERMANENT parser source-of-truth.
///   Replaced wholesale by every `record_parsed_edges`. **Never destroyed by
///   exact-resolution writers** (F18). The set of "active" unresolved stems
///   feeding `reverse_deps_by_stem` is computed as `parsed_unresolved_relatives
///   \ entries dampened by an exact_resolutions row with phase=CodegenBlocker
///   AND matching kind AND resolved_canonical_id.is_some()`.
/// - `exact_resolved`: replaced wholesale by `replace_exact_resolutions`.
///   **CLEARED by `record_parsed_edges`** (matches host_upsert.rs:170 which
///   clears `cc.import_routes` on every upsert — closes F11).
/// - `lazy_resolved`: cleared by `record_parsed_edges` and re-accumulated
///   on subsequent bare-import resolution.
/// - `ambient_resolved`: **NOT cleared by `record_parsed_edges`** (closes
///   F1.5 latent defect — ambient resolution is bare-name-driven, not parsed
///   from imports). Cleared only on `remove_file` and explicit
///   `replace_ambient_resolved`.
/// - `semantic_transitive`: **CLEARED by `record_parsed_edges`** (matches
///   host's `cc.dependencies` reset; the macro resolver re-fires post-upsert
///   and re-populates via `replace_semantic_transitive`).
/// - `bare_specifiers`: replaced by `record_parsed_edges`. Not part of the
///   reverse graph; kept for the lazy-resolution loop.
/// - `exact_resolutions`: keyed by `(specifier, phase, kind)`. CLEARED by
///   `record_parsed_edges`. Replaced wholesale by `replace_exact_resolutions`.
#[derive(Debug, Default)]
pub(crate) struct DependencySnapshot {
    /// Resolved canonical IDs for `Relative`/`ExternalSrc` parsed edges.
    pub parsed_resolved: BTreeSet<String>,
    /// Permanent parser source-of-truth for relative imports whose
    /// `resolve_import` returned `None`. Keyed by `(normalized specifier,
    /// kind)` so per-kind active-stem computation is precise. Value:
    /// path-joined stem.
    pub parsed_unresolved_relatives: FxHashMap<(String, ResolveRequestKind), String>,
    /// Resolved canonical IDs from `replace_exact_resolutions`.
    pub exact_resolved: BTreeSet<String>,
    /// Resolved canonical IDs from `add_lazy_resolved_dep` (bare-import cache).
    pub lazy_resolved: BTreeSet<String>,
    /// Resolved canonical IDs from `record_ambient_dependency` /
    /// `replace_ambient_resolved`.
    pub ambient_resolved: BTreeSet<String>,
    /// Resolved canonical IDs from `replace_semantic_transitive`.
    pub semantic_transitive: BTreeSet<String>,
    /// Stored bare specifiers (not yet resolved). Set by `record_parsed_edges`.
    pub bare_specifiers: Vec<(String, ResolveRequestKind)>,
    /// Bundler-injected exact resolutions, keyed by `(specifier, phase, kind)`.
    pub exact_resolutions: FxHashMap<(String, ResolvePhase, ResolveRequestKind), ExactResolution>,
}

impl DependencySnapshot {
    /// Union of every class that contributes a canonical id to the reverse
    /// graph. Stems are NOT included (different keying, separate axis).
    pub(crate) fn canonical_dep_union(&self) -> BTreeSet<String> {
        let mut out = self.parsed_resolved.clone();
        out.extend(self.exact_resolved.iter().cloned());
        out.extend(self.lazy_resolved.iter().cloned());
        out.extend(self.ambient_resolved.iter().cloned());
        out.extend(self.semantic_transitive.iter().cloned());
        out
    }

    /// Active unresolved-stem set: `parsed_unresolved_relatives` minus
    /// specifiers dampened by a `CodegenBlocker` exact resolution. Drives
    /// `reverse_deps_by_stem`. R5 restricts dampening to
    /// `phase = ResolvePhase::CodegenBlocker` because parsed-unresolved
    /// relatives are emitted from `record_parsed_edges` with
    /// `phase = CodegenBlocker`. ProviderGraph-only exacts must NOT dampen
    /// a CodegenBlocker stem.
    pub(crate) fn active_unresolved_stems(&self) -> BTreeSet<String> {
        // Collect (spec, kind) keys dampened by CodegenBlocker exacts only.
        let dampened: rustc_hash::FxHashSet<(String, ResolveRequestKind)> = self
            .exact_resolutions
            .iter()
            .filter(|((_, phase, _), r)| {
                *phase == ResolvePhase::CodegenBlocker && r.resolved_canonical_id.is_some()
            })
            .map(|((spec, _phase, kind), _)| {
                (
                    crate::relative_path::normalize_relative_specifier(spec),
                    *kind,
                )
            })
            .collect();

        self.parsed_unresolved_relatives
            .iter()
            .filter(|((spec, kind), _)| !dampened.contains(&(spec.clone(), *kind)))
            .map(|(_, stem)| stem.clone())
            .collect()
    }
}

#[derive(Debug, Default)]
pub(crate) struct FileEdgeState {
    pub deps: DependencySnapshot,
}

/// Public view of an owner's dependency snapshot. Cloned-copy of the internal
/// state; consumers may not borrow into the [`EdgeStore`].
#[derive(Debug, Clone, Default)]
pub struct DependencySnapshotView {
    pub parsed_resolved: BTreeSet<String>,
    pub parsed_unresolved_relatives: FxHashMap<(String, ResolveRequestKind), String>,
    pub exact_resolved: BTreeSet<String>,
    pub lazy_resolved: BTreeSet<String>,
    pub ambient_resolved: BTreeSet<String>,
    pub semantic_transitive: BTreeSet<String>,
    pub bare_specifiers: Vec<(String, ResolveRequestKind)>,
}

impl DependencySnapshotView {
    fn from_snapshot(snap: &DependencySnapshot) -> Self {
        Self {
            parsed_resolved: snap.parsed_resolved.clone(),
            parsed_unresolved_relatives: snap.parsed_unresolved_relatives.clone(),
            exact_resolved: snap.exact_resolved.clone(),
            lazy_resolved: snap.lazy_resolved.clone(),
            ambient_resolved: snap.ambient_resolved.clone(),
            semantic_transitive: snap.semantic_transitive.clone(),
            bare_specifiers: snap.bare_specifiers.clone(),
        }
    }
}

/// Storage for per-file edge state and the reverse dependency graph.
///
/// The edge store tracks:
/// - Per-file dependency snapshots (six dep classes plus stored
///   `bare_specifiers` and `exact_resolutions`).
/// - Two global reverse dependency graphs:
///   - `reverse_deps_canonical`: canonical id → set of dependents.
///   - `reverse_deps_by_stem`: stem (extensionless joined-path) → set of
///     dependents whose unresolved relative imports point at the stem.
#[derive(Debug, Default)]
pub struct EdgeStore {
    /// Per-file edge state.
    files: FxHashMap<String, FileEdgeState>,

    /// Canonical-axis: dep canonical_id -> set of owners whose
    /// `canonical_dep_union()` contains the dep. Aggregates parsed_resolved
    /// + exact_resolved + lazy_resolved + ambient_resolved + semantic_transitive.
    reverse_deps_canonical: FxHashMap<String, BTreeSet<String>>,

    /// Stem-axis: stem -> set of owners with an unresolved relative whose
    /// joined stem equals this. Stems are extensionless. Querying is
    /// driven by `Engine::reverse_deps_for` stripping the queried target's
    /// extension and looking up the bucket.
    reverse_deps_by_stem: FxHashMap<String, BTreeSet<String>>,
}

impl EdgeStore {
    pub fn new() -> Self {
        Self::default()
    }

    /// Get exact resolutions for a file, keyed by (specifier, phase, kind).
    pub fn get_exact_resolution(
        &self,
        canonical_id: &str,
        specifier: &str,
        ctx: ResolutionContext,
    ) -> Option<&ExactResolution> {
        self.files.get(canonical_id).and_then(|state| {
            state
                .deps
                .exact_resolutions
                .get(&(specifier.to_string(), ctx.phase, ctx.kind))
        })
    }

    /// Replace bundler-injected exact resolutions for a file. Active-stem
    /// set is recomputed AFTER the exact mutation; `reverse_deps_by_stem`
    /// is updated against the active-stem diff. Parsed-unresolved entries
    /// are NOT destroyed — when bundler later removes/Nones a resolution,
    /// the stem becomes active again automatically (F18 active-stem model).
    pub fn replace_exact_resolutions(
        &mut self,
        canonical_id: &str,
        resolutions: Vec<ExactResolution>,
    ) -> ExactResolutionResult {
        let mut newly_resolved = Vec::new();
        let pre_existing_other_class = {
            // Snapshot pre-existing parsed/lazy/ambient/semantic so we can
            // determine which incoming exact-resolved deps are "newly"
            // resolved (i.e., not already present in another class).
            let state = self.files.entry(canonical_id.to_string()).or_default();
            let mut other = state.deps.parsed_resolved.clone();
            other.extend(state.deps.lazy_resolved.iter().cloned());
            other.extend(state.deps.ambient_resolved.iter().cloned());
            other.extend(state.deps.semantic_transitive.iter().cloned());
            other
        };

        self.write_pattern(canonical_id, |snap| {
            snap.exact_resolutions.clear();
            snap.exact_resolved.clear();

            for resolution in resolutions {
                if let Some(ref id) = resolution.resolved_canonical_id {
                    if snap.exact_resolved.insert(id.clone())
                        && !pre_existing_other_class.contains(id)
                    {
                        newly_resolved.push(id.clone());
                    }
                }
                let key = (
                    resolution.specifier.clone(),
                    resolution.phase,
                    resolution.kind,
                );
                snap.exact_resolutions.insert(key, resolution);
            }
        });

        ExactResolutionResult { newly_resolved }
    }

    /// Replace `parsed_resolved` + `parsed_unresolved_relatives` +
    /// `bare_specifiers` + clear `lazy_resolved` + clear `exact_resolved` +
    /// clear `exact_resolutions` + clear `semantic_transitive`.
    /// **NOT clear `ambient_resolved` (F1.5).**
    pub fn replace_parsed_edges(
        &mut self,
        canonical_id: &str,
        parsed_resolved: BTreeSet<String>,
        unresolved_pairs: Vec<((String, ResolveRequestKind), String)>,
        bare_specifiers: Vec<(String, ResolveRequestKind)>,
    ) {
        self.write_pattern(canonical_id, |snap| {
            // Per F11 lifecycle: clear classes that are bundler/parser-driven.
            snap.parsed_resolved = parsed_resolved;
            snap.parsed_unresolved_relatives.clear();
            for (key, stem) in unresolved_pairs {
                snap.parsed_unresolved_relatives.insert(key, stem);
            }
            snap.bare_specifiers = bare_specifiers;
            snap.lazy_resolved.clear();
            snap.exact_resolved.clear();
            snap.exact_resolutions.clear();
            snap.semantic_transitive.clear();
            // ambient_resolved survives parse re-record (F1.5).
        });
    }

    /// Add a single lazy-resolved bare-import dep. Returns `true` if newly
    /// inserted into `lazy_resolved`. R5 (closes Codex P1): idempotency is
    /// checked ONLY against `lazy_resolved`, not other classes. The
    /// reverse-axis canonical bucket is updated via `canonical_dep_union`
    /// diff — if the dep is also present in another class, the union is
    /// unchanged and the reverse-bucket update is a no-op (correct: owner
    /// stays in the bucket).
    pub fn add_lazy_resolved_dep(&mut self, canonical_id: &str, dep_id: &str) -> bool {
        let mut inserted = false;
        self.write_pattern(canonical_id, |snap| {
            inserted = snap.lazy_resolved.insert(dep_id.to_string());
        });
        inserted
    }

    /// Replace `ambient_resolved` set wholesale.
    #[allow(dead_code)]
    pub fn replace_ambient_resolved(&mut self, canonical_id: &str, deps: BTreeSet<String>) {
        self.write_pattern(canonical_id, |snap| {
            snap.ambient_resolved = deps;
        });
    }

    /// Add a single ambient-resolved dep (incremental — first caller is
    /// `record_ambient_dependency` from session-side
    /// `resolve_ambient_global` at `ambient_resolve.rs:44`).
    /// Returns `true` if newly inserted.
    pub fn add_ambient_resolved_dep(&mut self, canonical_id: &str, virtual_id: &str) -> bool {
        let mut inserted = false;
        self.write_pattern(canonical_id, |snap| {
            inserted = snap.ambient_resolved.insert(virtual_id.to_string());
        });
        inserted
    }

    /// Replace `semantic_transitive` set wholesale. Always fires regardless
    /// of `canonical_dep_union` equality (closes F15).
    pub fn replace_semantic_transitive(&mut self, canonical_id: &str, deps: BTreeSet<String>) {
        self.write_pattern(canonical_id, |snap| {
            snap.semantic_transitive = deps;
        });
    }

    /// Public inspection — clone of the owner's snapshot.
    pub fn snapshot(&self, canonical_id: &str) -> Option<DependencySnapshotView> {
        self.files
            .get(canonical_id)
            .map(|state| DependencySnapshotView::from_snapshot(&state.deps))
    }

    /// Get forward dependencies for a file (union of all canonical-axis
    /// dep classes — `parsed_resolved` + `exact_resolved` + `lazy_resolved` +
    /// `ambient_resolved` + `semantic_transitive`). Stems are NOT included.
    pub fn forward_deps(&self, canonical_id: &str) -> Vec<String> {
        self.files
            .get(canonical_id)
            .map(|state| state.deps.canonical_dep_union().into_iter().collect())
            .unwrap_or_default()
    }

    /// Single union query (R4 hot-path optimised — closes Gemini/Codex
    /// CRITICAL PERFORMANCE / F19): when only one bucket hits, return its
    /// contents directly without allocating a `BTreeSet` for dedup.
    pub fn reverse_deps_for_target(
        &self,
        target: &str,
        stripped_target: Option<&str>,
    ) -> Vec<String> {
        // Collect non-empty bucket references first.
        let mut sources: smallvec::SmallVec<[&BTreeSet<String>; 4]> = smallvec::SmallVec::new();
        if let Some(s) = self.reverse_deps_canonical.get(target) {
            if !s.is_empty() {
                sources.push(s);
            }
        }
        if let Some(s) = self.reverse_deps_by_stem.get(target) {
            if !s.is_empty() {
                sources.push(s);
            }
        }
        if let Some(stripped) = stripped_target {
            if let Some(s) = self.reverse_deps_canonical.get(stripped) {
                if !s.is_empty() {
                    sources.push(s);
                }
            }
            if let Some(s) = self.reverse_deps_by_stem.get(stripped) {
                if !s.is_empty() {
                    sources.push(s);
                }
            }
        }

        match sources.len() {
            0 => Vec::new(),
            // Hot path: 99% of resolved-canonical queries hit only the
            // canonical bucket. Direct clone, no dedup needed.
            1 => sources[0].iter().cloned().collect(),
            _ => {
                let mut out: BTreeSet<String> = BTreeSet::new();
                for s in sources {
                    out.extend(s.iter().cloned());
                }
                out.into_iter().collect()
            }
        }
    }

    /// Backward-compatible shim — same as
    /// `reverse_deps_for_target(canonical_id, None)`.
    pub fn reverse_deps(&self, canonical_id: &str) -> Vec<String> {
        self.reverse_deps_for_target(canonical_id, None)
    }

    /// Number of files with edge-state entries.
    pub fn file_count(&self) -> usize {
        self.files.len()
    }

    /// Number of reverse-dependency buckets currently tracked across both
    /// axes (canonical + stem). Reported via the workspace resource snapshot.
    pub fn reverse_dep_bucket_count(&self) -> usize {
        self.reverse_deps_canonical.len() + self.reverse_deps_by_stem.len()
    }

    /// Check if a file has any exact resolutions set.
    pub fn has_exact_resolutions(&self, canonical_id: &str) -> bool {
        self.files
            .get(canonical_id)
            .map(|state| !state.deps.exact_resolutions.is_empty())
            .unwrap_or(false)
    }

    /// Surgically remove all state for a file. O(canonical_union +
    /// active_stems) per removal — no global iteration. Closes
    /// Gemini's CRITICAL PERFORMANCE finding and CC's M1.
    pub fn remove_file(&mut self, canonical_id: &str) {
        if let Some(state) = self.files.remove(canonical_id) {
            // Canonical-axis: clean using the pre-removal union (every dep
            // the owner participated in across all classes incl. ambient).
            for dep in state.deps.canonical_dep_union() {
                if let Some(bucket) = self.reverse_deps_canonical.get_mut(&dep) {
                    bucket.remove(canonical_id);
                    if bucket.is_empty() {
                        self.reverse_deps_canonical.remove(&dep);
                    }
                }
            }
            // Stem-axis: clean using ACTIVE stems at time of removal (the
            // only ones that were actually inserted into reverse_deps_by_stem).
            for stem in state.deps.active_unresolved_stems() {
                if let Some(bucket) = self.reverse_deps_by_stem.get_mut(&stem) {
                    bucket.remove(canonical_id);
                    if bucket.is_empty() {
                        self.reverse_deps_by_stem.remove(&stem);
                    }
                }
            }
        }
        // The owner being removed is also a possible dep target.
        self.reverse_deps_canonical.remove(canonical_id);
        self.reverse_deps_by_stem.remove(canonical_id);
    }

    /// Remove all state for files under a directory prefix.
    pub fn remove_under(&mut self, prefix: &str) {
        let to_remove: Vec<String> = self
            .files
            .keys()
            .filter(|path| path_matches_prefix(path, prefix))
            .cloned()
            .collect();
        for canonical_id in to_remove {
            self.remove_file(&canonical_id);
        }
    }

    /// Get stored bare specifiers for a file (for lazy resolution).
    pub fn bare_specifiers(&self, canonical_id: &str) -> &[(String, ResolveRequestKind)] {
        self.files
            .get(canonical_id)
            .map(|state| state.deps.bare_specifiers.as_slice())
            .unwrap_or(&[])
    }

    /// Common write pattern (R5 borrow-checker-explicit): take a single
    /// `&mut FileEdgeState`, compute BEFORE union/active-stems, run the
    /// caller's mutate closure, compute AFTER union/active-stems, then
    /// outside the inner scope diff-update both reverse axes.
    fn write_pattern(&mut self, canonical_id: &str, mutate: impl FnOnce(&mut DependencySnapshot)) {
        // Inner scope: holds &mut state. Only DependencySnapshot is read/written.
        let (old_union, old_active_stems, new_union, new_active_stems) = {
            let state = self.files.entry(canonical_id.to_string()).or_default();
            let old_union = state.deps.canonical_dep_union();
            let old_active_stems = state.deps.active_unresolved_stems();
            mutate(&mut state.deps);
            let new_union = state.deps.canonical_dep_union();
            let new_active_stems = state.deps.active_unresolved_stems();
            (old_union, old_active_stems, new_union, new_active_stems)
        };
        // Outer scope: &mut state has been dropped. Now safe to take
        // &mut self.reverse_deps_*.
        self.apply_canonical_union_diff(canonical_id, &old_union, &new_union);
        self.apply_stem_diff(canonical_id, &old_active_stems, &new_active_stems);
    }

    fn apply_canonical_union_diff(
        &mut self,
        canonical_id: &str,
        old_union: &BTreeSet<String>,
        new_union: &BTreeSet<String>,
    ) {
        for old in old_union.difference(new_union) {
            if let Some(bucket) = self.reverse_deps_canonical.get_mut(old) {
                bucket.remove(canonical_id);
                if bucket.is_empty() {
                    self.reverse_deps_canonical.remove(old);
                }
            }
        }
        for new in new_union.difference(old_union) {
            self.reverse_deps_canonical
                .entry(new.clone())
                .or_default()
                .insert(canonical_id.to_string());
        }
    }

    fn apply_stem_diff(
        &mut self,
        canonical_id: &str,
        old_stems: &BTreeSet<String>,
        new_stems: &BTreeSet<String>,
    ) {
        for old in old_stems.difference(new_stems) {
            if let Some(bucket) = self.reverse_deps_by_stem.get_mut(old) {
                bucket.remove(canonical_id);
                if bucket.is_empty() {
                    self.reverse_deps_by_stem.remove(old);
                }
            }
        }
        for new in new_stems.difference(old_stems) {
            self.reverse_deps_by_stem
                .entry(new.clone())
                .or_default()
                .insert(canonical_id.to_string());
        }
    }
}

#[cfg(test)]
#[path = "exact_resolution_tests.rs"]
mod tests;
