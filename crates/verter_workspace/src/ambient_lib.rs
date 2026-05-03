//! Ambient lib registration types and engine storage.
//!
//! sub-plan: per-project ambient TypeScript libs (e.g. lib.es5.d.ts)
//! are registered against a [`crate::project_key::ProjectStableKey`] and stored
//! lock-free on the [`crate::engine::Engine`] via `ArcSwap`. They are visible
//! only via [`WorkspaceAccess::read_ambient_lib`] / `lookup_ambient_symbol`,
//! and shadow under user files via [`WorkspaceAccess::file_exists`] (A5).
//!
//! Identity rule (A3): keys include the workspace root so multi-root setups
//! with the same `tsconfig.json` paths produce distinct keys. See
//! [`crate::project_key::ProjectStableKey`].
//!
//! Path normalization (A7): `register_ambient_lib` and `read_ambient_lib`
//! normalize via [`normalize_canonical_id`] (`\` -> `/`, trim leading `/`)
//! so `\\lib.es5.d.ts`, `/lib.es5.d.ts`, and `lib.es5.d.ts` all hit the
//! same entry.

use std::sync::Arc;

use rustc_hash::FxHashMap;
use verter_scheduler::invalidation::Hash16;

use crate::project_key::ProjectStableKey;
use crate::workspace_snapshot::ProjectId;

/// Public spec passed to [`crate::traits::WorkspaceAccess::register_ambient_lib`].
///
/// `project_id` is `None` to default to the single configured project (errors
/// when there is more than one). `canonical_id` is the user-form id (e.g.
/// `lib.es5.d.ts`), normalized internally per A7. `source` is the lib source
/// text.
#[derive(Debug, Clone)]
pub struct AmbientLibSpec {
    pub project_id: Option<ProjectId>,
    pub canonical_id: Arc<str>,
    pub source: Arc<str>,
}

/// Errors returned by ambient lib registration.
#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum AmbientLibError {
    #[error("workspace not bootstrapped (no ambient lib backend)")]
    NotBootstrapped,
    #[error("workspace has no published snapshot yet")]
    NotPublished,
    #[error("project_id not found, and no single primary project to default to")]
    UnknownOrAmbiguousProject,
    #[error("canonical_id `{0}` already registered as a non-ambient file in project")]
    NonAmbientCollision(Arc<str>),
    #[error("lib parse failed: {0}")]
    ParseFailure(String),
}

/// Hit returned by [`crate::traits::WorkspaceAccess::lookup_ambient_symbol`].
///
/// `canonical_id` is the trimmed normalized canonical (e.g. `lib.es5.d.ts`).
/// `virtual_id` is the project-scoped `ambient:/<C|F><32hex>/<canonical>`
/// form used as the file id in scheduler / dep graph.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AmbientSymbolHit {
    pub project: ProjectStableKey,
    pub canonical_id: Arc<str>,
    pub virtual_id: Arc<str>,
    pub lib_order: u32,
}

/// Engine-side ambient lib registry, swapped lock-free via `ArcSwap`.
#[derive(Default, Clone)]
pub struct AmbientLibsByProject {
    pub by_project: FxHashMap<ProjectStableKey, ProjectAmbientLibs>,
}

/// Per-project ambient lib state.
#[derive(Default, Clone)]
pub struct ProjectAmbientLibs {
    /// canonical_id (trimmed, normalized per A7) -> entry.
    pub libs: FxHashMap<Arc<str>, AmbientLibEntry>,
    /// A2: symbol_name -> list of (canonical_id, lib_order). Pre-sorted
    /// ascending by lib_order so first wins on duplicate symbols.
    pub symbol_index: FxHashMap<Arc<str>, Vec<(Arc<str>, u32)>>,
}

/// Per-canonical ambient lib entry.
#[derive(Clone)]
pub struct AmbientLibEntry {
    pub source: Arc<str>,
    pub content_hash: Hash16,
    /// Registration order, used for TS-lib precedence on duplicate symbols.
    pub lib_order: u32,
    /// Top-level export names extracted at registration via cheap shallow parse (A6).
    pub top_level_exports: Arc<[Arc<str>]>,
}

impl std::fmt::Debug for AmbientLibEntry {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("AmbientLibEntry")
            .field("content_hash", &self.content_hash)
            .field("lib_order", &self.lib_order)
            .field("top_level_exports", &self.top_level_exports)
            .field("source_len", &self.source.len())
            .finish()
    }
}

/// A7: normalize an ambient canonical_id at the public API boundary.
///
/// Replaces `\` with `/`, then trims leading `/` characters. So
/// `\\lib.es5.d.ts`, `/lib.es5.d.ts`, and `lib.es5.d.ts` all collapse to
/// `lib.es5.d.ts`.
pub fn normalize_canonical_id(raw: &str) -> Arc<str> {
    let normalized = raw.replace('\\', "/");
    Arc::from(normalized.trim_start_matches('/'))
}

/// Compute the project-scoped ambient virtual canonical id used as the
/// `file_id` in scheduler / dep graph: `ambient:/<tag>/<canonical>`.
pub fn ambient_virtual_canonical_id(stable_key: ProjectStableKey, canonical_id: &str) -> Arc<str> {
    let canonical = normalize_canonical_id(canonical_id);
    Arc::from(format!(
        "ambient:/{}/{}",
        stable_key.to_hex_tag(),
        canonical
    ))
}

/// Compute a Hash16 over a source bytes block. Uses xxh3_128, matching the
/// project-key hash function and the audit-corpus footprint hash function.
pub fn compute_ambient_hash16(bytes: &[u8]) -> Hash16 {
    xxhash_rust::xxh3::xxh3_128(bytes).to_le_bytes()
}

/// CAS-loop swap of an entry into the ambient-libs registry.
///
/// Returns `true` if the swap occurred (i.e. content_hash differed from any
/// existing entry — caller must bump content_generation in that case).
/// Returns `false` if the existing entry already had matching content_hash
/// (idempotent re-registration is a no-op).
///
/// Lock-free CAS retry — if a concurrent writer races us, we re-read and
/// retry. Worst case the registry grows monotonically; at steady state the
/// number of retries equals the number of contending writers.
pub(crate) fn cas_register(
    storage: &arc_swap::ArcSwap<AmbientLibsByProject>,
    stable_key: ProjectStableKey,
    canonical: Arc<str>,
    source: Arc<str>,
    content_hash: Hash16,
    top_level_exports: Arc<[Arc<str>]>,
) -> bool {
    loop {
        let current = storage.load_full();
        // Fast path: idempotent re-registration with matching content_hash.
        if let Some(p) = current.by_project.get(&stable_key) {
            if let Some(existing) = p.libs.get(canonical.as_ref()) {
                if existing.content_hash == content_hash {
                    return false;
                }
            }
        }

        let mut new_state: AmbientLibsByProject = (*current).clone();
        let p = new_state.by_project.entry(stable_key).or_default();

        // lib_order: max existing + 1 (registration order). If the canonical
        // is being replaced, it KEEPS its existing lib_order so symbol
        // precedence stays stable across re-registration.
        let lib_order = match p.libs.get(canonical.as_ref()) {
            Some(existing) => existing.lib_order,
            None => p
                .libs
                .values()
                .map(|e| e.lib_order)
                .max()
                .map_or(0, |m| m + 1),
        };

        // Update symbol_index: remove old entry's symbols, add new ones.
        if let Some(old) = p.libs.get(canonical.as_ref()) {
            for sym in old.top_level_exports.iter() {
                if let Some(list) = p.symbol_index.get_mut(sym) {
                    list.retain(|(cid, _)| cid != &canonical);
                    if list.is_empty() {
                        // Drop empty symbol_index keys to avoid leaks across
                        // many re-registrations.
                        p.symbol_index.remove(sym);
                    }
                }
            }
        }
        for sym in top_level_exports.iter() {
            p.symbol_index
                .entry(Arc::clone(sym))
                .or_default()
                .push((Arc::clone(&canonical), lib_order));
        }
        // Keep symbol lists sorted ascending by lib_order so first wins on
        // duplicate symbols.
        for list in p.symbol_index.values_mut() {
            list.sort_by_key(|(_, ord)| *ord);
        }

        p.libs.insert(
            Arc::clone(&canonical),
            AmbientLibEntry {
                source: Arc::clone(&source),
                content_hash,
                lib_order,
                top_level_exports: Arc::clone(&top_level_exports),
            },
        );

        let new_arc = std::sync::Arc::new(new_state);
        let prev = storage.compare_and_swap(&current, new_arc);
        if std::sync::Arc::ptr_eq(&prev, &current) {
            return true;
        }
        // Lost the race — retry.
    }
}

/// CAS-loop unregistration. Returns `true` if an entry was removed.
pub(crate) fn cas_unregister(
    storage: &arc_swap::ArcSwap<AmbientLibsByProject>,
    stable_key: ProjectStableKey,
    canonical: Arc<str>,
) -> bool {
    loop {
        let current = storage.load_full();
        let exists = current
            .by_project
            .get(&stable_key)
            .and_then(|p| p.libs.get(canonical.as_ref()))
            .is_some();
        if !exists {
            return false;
        }
        let mut new_state: AmbientLibsByProject = (*current).clone();
        if let Some(p) = new_state.by_project.get_mut(&stable_key) {
            if let Some(old) = p.libs.remove(canonical.as_ref()) {
                for sym in old.top_level_exports.iter() {
                    if let Some(list) = p.symbol_index.get_mut(sym) {
                        list.retain(|(cid, _)| cid != &canonical);
                        if list.is_empty() {
                            p.symbol_index.remove(sym);
                        }
                    }
                }
            }
            // GC empty project entries.
            if p.libs.is_empty() {
                new_state.by_project.remove(&stable_key);
            }
        }
        let new_arc = std::sync::Arc::new(new_state);
        let prev = storage.compare_and_swap(&current, new_arc);
        if std::sync::Arc::ptr_eq(&prev, &current) {
            return true;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalize_canonical_id_collapses_paths() {
        assert_eq!(&*normalize_canonical_id("lib.es5.d.ts"), "lib.es5.d.ts");
        assert_eq!(&*normalize_canonical_id("/lib.es5.d.ts"), "lib.es5.d.ts");
        assert_eq!(&*normalize_canonical_id("\\lib.es5.d.ts"), "lib.es5.d.ts");
        assert_eq!(&*normalize_canonical_id("\\\\lib.es5.d.ts"), "lib.es5.d.ts");
        assert_eq!(&*normalize_canonical_id("//lib.es5.d.ts"), "lib.es5.d.ts");
        // Non-leading slashes preserved.
        assert_eq!(
            &*normalize_canonical_id("ts/lib.es5.d.ts"),
            "ts/lib.es5.d.ts"
        );
        assert_eq!(
            &*normalize_canonical_id("\\ts\\lib.es5.d.ts"),
            "ts/lib.es5.d.ts"
        );
    }

    #[test]
    fn ambient_virtual_canonical_id_includes_project_tag_and_canonical() {
        let key = ProjectStableKey::Configured([0xAB; 16]);
        let virt = ambient_virtual_canonical_id(key, "lib.es5.d.ts");
        let s: &str = &virt;
        assert!(s.starts_with("ambient:/C"), "got {s}");
        assert!(s.ends_with("/lib.es5.d.ts"), "got {s}");
        // No double-slash or backslashes — A7 normalization applies.
        assert!(!s.contains("\\"));
        assert!(!s.contains("//"));
    }

    #[test]
    fn compute_ambient_hash16_is_stable() {
        let a = compute_ambient_hash16(b"export const x: number;");
        let b = compute_ambient_hash16(b"export const x: number;");
        let c = compute_ambient_hash16(b"export const y: number;");
        assert_eq!(a, b);
        assert_ne!(a, c);
    }
}
