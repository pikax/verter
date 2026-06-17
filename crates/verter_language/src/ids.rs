//! Interned identifier newtypes for the language routing authority.
//!
//! All three ids wrap an interned `Arc<str>`: clones are pointer-cheap,
//! equality is content equality, and the id sets stay OPEN — a future
//! framework vertical registers a new id without a central enum edit.
//!
//! The intern table is deliberately crate-local: this crate is a
//! zero-dependency leaf and no lower-level crate exposes a reusable
//! interning facility (`verter_span` owns spans, not strings;
//! `verter_session`'s semantic-query interner sits far above this
//! crate). The table only grows with DISTINCT id strings, and ids
//! originate from the fixed built-in registry rows, registered
//! framework verticals, and the closed FFI accepted-string set —
//! never from arbitrary user input — so growth is bounded by the set
//! of registered languages.

use std::collections::HashSet;
use std::fmt;
use std::sync::{Arc, Mutex, OnceLock};

/// Global intern table. Every id constructed through [`intern`] shares
/// one `Arc<str>` per distinct string, so per-file classification
/// results clone interned pointers instead of allocating.
fn intern(value: &str) -> Arc<str> {
    static TABLE: OnceLock<Mutex<HashSet<Arc<str>>>> = OnceLock::new();
    let table = TABLE.get_or_init(|| Mutex::new(HashSet::new()));
    let mut guard = table.lock().expect("intern table poisoned");
    if let Some(existing) = guard.get(value) {
        return Arc::clone(existing);
    }
    let arc: Arc<str> = Arc::from(value);
    guard.insert(Arc::clone(&arc));
    arc
}

/// Identity of a framework adapter (open set).
///
/// `"vue"` and `"svelte"` are the built-in carrier rows; future
/// verticals add their own ids through their registry rows.
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct FrameworkAdapterId(Arc<str>);

impl FrameworkAdapterId {
    /// Intern an adapter id.
    pub fn new(id: &str) -> Self {
        Self(intern(id))
    }

    /// The built-in Vue adapter id.
    ///
    /// Memoized: helper constructors run on hot per-file paths, so the
    /// built-in ids are interned once and cloned (a refcount bump) —
    /// never re-locking the intern table.
    pub fn vue() -> Self {
        static VUE: OnceLock<FrameworkAdapterId> = OnceLock::new();
        VUE.get_or_init(|| Self::new("vue")).clone()
    }

    /// The built-in Svelte adapter id. Memoized like [`Self::vue`].
    pub fn svelte() -> Self {
        static SVELTE: OnceLock<FrameworkAdapterId> = OnceLock::new();
        SVELTE.get_or_init(|| Self::new("svelte")).clone()
    }

    /// `true` when this id is the built-in Vue adapter.
    pub fn is_vue(&self) -> bool {
        self.0.as_ref() == "vue"
    }

    /// `true` when this id is the built-in Svelte adapter.
    pub fn is_svelte(&self) -> bool {
        self.0.as_ref() == "svelte"
    }

    /// The interned string form.
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for FrameworkAdapterId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

/// Identity of a concrete language within an adapter (open set).
///
/// Distinguishes the languages one adapter can own (e.g. an adapter's
/// carrier language vs. its external template language).
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct LanguageId(Arc<str>);

impl LanguageId {
    /// Intern a language id.
    pub fn new(id: &str) -> Self {
        Self(intern(id))
    }

    /// The interned string form.
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for LanguageId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

/// Identity of a project capability bit gating a candidate row (open set).
///
/// A [`crate::GatedCandidate`] names the capability that must be derived
/// from project state before its candidate classification applies. This
/// crate only names capabilities; deriving and snapshotting them is
/// host-level work.
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct CapabilityId(Arc<str>);

impl CapabilityId {
    /// Intern a capability id.
    pub fn new(id: &str) -> Self {
        Self(intern(id))
    }

    /// The interned string form.
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for CapabilityId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn interned_ids_share_storage_and_compare_by_content() {
        let a = FrameworkAdapterId::new("vue");
        let b = FrameworkAdapterId::vue();
        assert_eq!(a, b);
        assert!(Arc::ptr_eq(&a.0, &b.0), "same id must share one Arc");
        assert!(a.is_vue());
        assert!(!FrameworkAdapterId::svelte().is_vue());
        assert_eq!(FrameworkAdapterId::svelte().as_str(), "svelte");
    }

    #[test]
    fn distinct_ids_are_distinct() {
        assert_ne!(FrameworkAdapterId::vue(), FrameworkAdapterId::svelte());
        assert_ne!(LanguageId::new("vue"), LanguageId::new("svelte"));
        assert_ne!(CapabilityId::new("a"), CapabilityId::new("b"));
    }
}
