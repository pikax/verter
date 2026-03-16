use rustc_hash::FxHashMap;
use std::collections::BTreeSet;

use crate::types::{ExactResolution, ExactResolutionResult};

/// Per-file edge and resolution state.
///
/// Forward dependencies are tracked in three disjoint sets:
/// - `eagerly_resolved_deps`: from `record_parsed_edges()` (relative + src edges)
/// - `exact_resolved_deps`: from `set_exact_resolutions()` (bundler/LSP injected)
/// - `lazily_resolved_deps`: from `add_lazily_resolved_dep()` (bare-import cache)
///
/// The union of all three is the full forward-dep set used for reverse-dep tracking.
#[derive(Debug, Default)]
pub(crate) struct FileEdgeState {
    /// Exact resolutions injected by bundler/LSP (specifier → resolution).
    pub exact_resolutions: FxHashMap<String, ExactResolution>,
    /// Deps from eagerly resolved edges (relative imports, external src blocks).
    /// Set by `record_parsed_edges()`, cleared and replaced on each call.
    pub eagerly_resolved_deps: BTreeSet<String>,
    /// Deps from exact resolutions (bundler/LSP injected).
    /// Set by `set_exact_resolutions()`, cleared and replaced on each call.
    pub exact_resolved_deps: BTreeSet<String>,
    /// Deps from lazy resolution of bare imports during `resolve_import()`.
    /// Accumulated over time, cleared by `record_parsed_edges()`.
    pub lazily_resolved_deps: BTreeSet<String>,
    /// Stored bare specifiers (not yet resolved). Key: specifier, Value: kind info.
    pub bare_specifiers: Vec<(String, crate::types::ResolveRequestKind)>,
}

impl FileEdgeState {
    /// The full forward-dep set (union of all three dep sets).
    fn all_deps(&self) -> BTreeSet<String> {
        let mut all = self.eagerly_resolved_deps.clone();
        all.extend(self.exact_resolved_deps.iter().cloned());
        all.extend(self.lazily_resolved_deps.iter().cloned());
        all
    }
}

/// Storage for per-file edge state and the reverse dependency graph.
///
/// The edge store tracks:
/// - Per-file exact resolutions (authoritative specifier→canonical_id mappings)
/// - Per-file resolved forward dependencies (eagerly + exact + lazily resolved)
/// - Global reverse dependency graph (canonical_id → set of dependents)
#[derive(Debug, Default)]
pub struct EdgeStore {
    /// Per-file edge state.
    files: FxHashMap<String, FileEdgeState>,
    /// Reverse dependency graph: dependency → set of files that depend on it.
    reverse_deps: FxHashMap<String, BTreeSet<String>>,
}

impl EdgeStore {
    pub fn new() -> Self {
        Self::default()
    }

    /// Get exact resolutions for a file.
    pub fn get_exact_resolution(
        &self,
        canonical_id: &str,
        specifier: &str,
    ) -> Option<&ExactResolution> {
        self.files
            .get(canonical_id)
            .and_then(|state| state.exact_resolutions.get(specifier))
    }

    /// Set exact resolutions for a file. Replaces any previous exact resolutions
    /// AND their derived deps. Updates the reverse dep graph correctly.
    pub fn set_exact_resolutions(
        &mut self,
        canonical_id: &str,
        resolutions: Vec<ExactResolution>,
    ) -> ExactResolutionResult {
        let state = self.files.entry(canonical_id.to_string()).or_default();

        let old_deps = state.all_deps();

        // Replace exact resolutions and their dep set
        state.exact_resolutions.clear();
        state.exact_resolved_deps.clear();

        let mut newly_resolved = Vec::new();
        for resolution in resolutions {
            if let Some(ref id) = resolution.resolved_canonical_id {
                if state.exact_resolved_deps.insert(id.clone()) {
                    // Only "newly resolved" if not already in eager or lazy sets
                    if !state.eagerly_resolved_deps.contains(id)
                        && !state.lazily_resolved_deps.contains(id)
                    {
                        newly_resolved.push(id.clone());
                    }
                }
            }
            state
                .exact_resolutions
                .insert(resolution.specifier.clone(), resolution);
        }

        let new_deps = state.all_deps();

        // Update reverse deps with the full old/new diff
        self.update_reverse_deps(canonical_id, &old_deps, &new_deps);

        ExactResolutionResult { newly_resolved }
    }

    /// Record parsed edges for a file. Replaces the previous edge state.
    ///
    /// - Eagerly resolved edges (Relative, ExternalSrc) should have their
    ///   resolved canonical IDs passed in `eagerly_resolved`.
    /// - Bare specifiers are stored for later resolution.
    /// - Clears exact_resolutions and all dep sets for the file.
    pub fn record_parsed_edges(
        &mut self,
        canonical_id: &str,
        eagerly_resolved: Vec<String>,
        bare_specifiers: Vec<(String, crate::types::ResolveRequestKind)>,
    ) {
        let state = self.files.entry(canonical_id.to_string()).or_default();

        let old_deps = state.all_deps();

        // Clear all state (match host_upsert.rs:216 behavior)
        state.exact_resolutions.clear();
        state.exact_resolved_deps.clear();
        state.lazily_resolved_deps.clear();
        state.eagerly_resolved_deps.clear();

        // Set eagerly resolved deps
        for id in eagerly_resolved {
            state.eagerly_resolved_deps.insert(id);
        }

        // Store bare specifiers for lazy resolution
        state.bare_specifiers = bare_specifiers;

        let new_deps = state.all_deps();

        // Update reverse deps with the full old/new diff
        self.update_reverse_deps(canonical_id, &old_deps, &new_deps);
    }

    /// Add a lazily resolved dependency (from bare-import resolution).
    /// Also updates the reverse dep graph.
    pub fn add_lazily_resolved_dep(&mut self, canonical_id: &str, dep_id: &str) -> bool {
        let state = self.files.entry(canonical_id.to_string()).or_default();

        // Already tracked in some dep set?
        if state.eagerly_resolved_deps.contains(dep_id)
            || state.exact_resolved_deps.contains(dep_id)
            || !state.lazily_resolved_deps.insert(dep_id.to_string())
        {
            return false;
        }

        self.reverse_deps
            .entry(dep_id.to_string())
            .or_default()
            .insert(canonical_id.to_string());
        true
    }

    /// Add a resolved dependency to a file's forward dep set (legacy API).
    /// Routes to `add_lazily_resolved_dep`.
    pub fn add_resolved_dep(&mut self, canonical_id: &str, dep_id: &str) -> bool {
        self.add_lazily_resolved_dep(canonical_id, dep_id)
    }

    /// Get forward dependencies for a file (union of all dep sets).
    pub fn forward_deps(&self, canonical_id: &str) -> Vec<String> {
        self.files
            .get(canonical_id)
            .map(|state| state.all_deps().into_iter().collect())
            .unwrap_or_default()
    }

    /// Get reverse dependencies (files that depend on this file).
    pub fn reverse_deps(&self, canonical_id: &str) -> Vec<String> {
        self.reverse_deps
            .get(canonical_id)
            .map(|deps| deps.iter().cloned().collect())
            .unwrap_or_default()
    }

    /// Check if a file has any exact resolutions set.
    pub fn has_exact_resolutions(&self, canonical_id: &str) -> bool {
        self.files
            .get(canonical_id)
            .map(|state| !state.exact_resolutions.is_empty())
            .unwrap_or(false)
    }

    /// Remove all state for a file. Updates the reverse dep graph.
    pub fn remove_file(&mut self, canonical_id: &str) {
        if let Some(state) = self.files.remove(canonical_id) {
            // Remove this file from all its dependencies' reverse dep sets
            for dep in state.all_deps() {
                if let Some(rev_set) = self.reverse_deps.get_mut(&dep) {
                    rev_set.remove(canonical_id);
                    if rev_set.is_empty() {
                        self.reverse_deps.remove(&dep);
                    }
                }
            }
        }

        // Remove reverse dep entry for this file
        self.reverse_deps.remove(canonical_id);
    }

    /// Get stored bare specifiers for a file (for lazy resolution).
    pub fn bare_specifiers(
        &self,
        canonical_id: &str,
    ) -> &[(String, crate::types::ResolveRequestKind)] {
        self.files
            .get(canonical_id)
            .map(|state| state.bare_specifiers.as_slice())
            .unwrap_or(&[])
    }

    fn update_reverse_deps(
        &mut self,
        canonical_id: &str,
        old_deps: &BTreeSet<String>,
        new_deps: &BTreeSet<String>,
    ) {
        // Remove from deps that are no longer referenced
        for old_dep in old_deps {
            if !new_deps.contains(old_dep) {
                if let Some(rev_set) = self.reverse_deps.get_mut(old_dep) {
                    rev_set.remove(canonical_id);
                    if rev_set.is_empty() {
                        self.reverse_deps.remove(old_dep);
                    }
                }
            }
        }

        // Add to new deps
        for new_dep in new_deps {
            if !old_deps.contains(new_dep) {
                self.reverse_deps
                    .entry(new_dep.clone())
                    .or_default()
                    .insert(canonical_id.to_string());
            }
        }
    }
}

#[cfg(test)]
#[path = "exact_resolution_tests.rs"]
mod tests;
