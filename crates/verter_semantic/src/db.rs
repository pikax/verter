//! Semantic database — revision-gated query engine.
//!
//! The semantic DB holds cached semantic facts keyed by (canonical_file_id,
//! query_key, revision). Queries are pure over immutable snapshots — they
//! do not perform I/O or block on cross-file wakeups.
//!
//! When a query cannot be fully resolved (missing parser snapshot, missing
//! provider data, etc.), it returns a [`Completeness::Partial`] or
//! [`Completeness::Unavailable`] result with explicit missing dependencies.
//! The session decides whether to materialize, defer, or surface partial results.

use rustc_hash::FxHashMap;

use crate::facts::binding::BindingDeclaration;
use crate::facts::component::ComponentSurface;
use crate::facts::reactivity::ReactivityFact;
use crate::query::{Completeness, QueryResult};
use crate::refs::FileRef;
use crate::revision::RevisionMarker;

/// Per-file semantic cache entry.
#[derive(Debug)]
struct FileSemantic {
    /// The revision at which this entry was computed.
    revision: RevisionMarker,
    /// Cached component surface (if this file is a Vue SFC).
    component_surface: Option<ComponentSurface>,
    /// Cached binding declarations with reactivity facts.
    bindings: Option<Vec<(BindingDeclaration, ReactivityFact)>>,
}

/// The semantic database.
///
/// Holds revision-gated caches for semantic facts. Queries check whether
/// cached entries are still valid by comparing revision markers.
#[derive(Debug, Default)]
pub struct SemanticDb {
    /// Per-file semantic caches, keyed by canonical file ID.
    files: FxHashMap<String, FileSemantic>,
}

impl SemanticDb {
    pub fn new() -> Self {
        Self::default()
    }

    /// Query the component surface for a file.
    ///
    /// Returns the cached surface if it was computed at the given revision,
    /// or `Unavailable` if no surface has been computed yet.
    pub fn component_surface(
        &self,
        file_ref: &FileRef,
        current_revision: RevisionMarker,
    ) -> QueryResult<Option<ComponentSurface>> {
        match self.files.get(&file_ref.file_id) {
            Some(entry) if entry.revision == current_revision => {
                QueryResult::complete(entry.component_surface.clone(), current_revision)
            }
            Some(entry) if current_revision.is_newer_than(&entry.revision) => {
                // Stale cache — return what we have as partial
                QueryResult::partial(entry.component_surface.clone(), current_revision, vec![])
            }
            _ => QueryResult::unavailable(None, current_revision, vec![]),
        }
    }

    /// Store a computed component surface for a file.
    pub fn set_component_surface(
        &mut self,
        file_id: String,
        revision: RevisionMarker,
        surface: ComponentSurface,
    ) {
        let entry = self.files.entry(file_id).or_insert_with(|| FileSemantic {
            revision,
            component_surface: None,
            bindings: None,
        });
        entry.revision = revision;
        entry.component_surface = Some(surface);
    }

    /// Query binding facts for a file.
    pub fn bindings(
        &self,
        file_ref: &FileRef,
        current_revision: RevisionMarker,
    ) -> QueryResult<Option<Vec<(BindingDeclaration, ReactivityFact)>>> {
        match self.files.get(&file_ref.file_id) {
            Some(entry) if entry.revision == current_revision => {
                QueryResult::complete(entry.bindings.clone(), current_revision)
            }
            Some(entry) if current_revision.is_newer_than(&entry.revision) => {
                QueryResult::partial(entry.bindings.clone(), current_revision, vec![])
            }
            _ => QueryResult::unavailable(None, current_revision, vec![]),
        }
    }

    /// Store computed binding facts for a file.
    pub fn set_bindings(
        &mut self,
        file_id: String,
        revision: RevisionMarker,
        bindings: Vec<(BindingDeclaration, ReactivityFact)>,
    ) {
        let entry = self.files.entry(file_id).or_insert_with(|| FileSemantic {
            revision,
            component_surface: None,
            bindings: None,
        });
        entry.revision = revision;
        entry.bindings = Some(bindings);
    }

    /// Invalidate all cached facts for a file.
    pub fn invalidate(&mut self, file_id: &str) {
        self.files.remove(file_id);
    }

    /// Number of files with cached semantic data.
    pub fn cached_file_count(&self) -> usize {
        self.files.len()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::facts::component::{DeclaredSurface, PropFact};
    use verter_span::Span;

    fn make_revision(ws: u64) -> RevisionMarker {
        RevisionMarker {
            workspace_revision: ws,
            ..RevisionMarker::initial()
        }
    }

    #[test]
    fn empty_db_returns_unavailable() {
        let db = SemanticDb::new();
        let file = FileRef::new("app.vue");
        let result = db.component_surface(&file, make_revision(1));

        assert_eq!(result.completeness, Completeness::Unavailable);
        assert!(result.value.is_none());
    }

    #[test]
    fn set_and_query_component_surface() {
        let mut db = SemanticDb::new();
        let rev = make_revision(1);

        let mut surface = ComponentSurface::default();
        surface.declared.props.push(PropFact {
            name: "color".into(),
            is_optional: true,
            type_text: Some("string".into()),
            default_value: None,
            description: None,
            span: Span::new(10, 20),
        });

        db.set_component_surface("app.vue".into(), rev, surface);

        let file = FileRef::new("app.vue");
        let result = db.component_surface(&file, rev);

        // Positive: result is complete and has the prop
        assert!(result.is_complete());
        let surface = result.value.unwrap();
        assert_eq!(surface.declared.props.len(), 1);
        assert_eq!(surface.declared.props[0].name, "color");
    }

    #[test]
    fn stale_revision_returns_partial() {
        let mut db = SemanticDb::new();
        let rev1 = make_revision(1);
        let rev2 = make_revision(2);

        db.set_component_surface("app.vue".into(), rev1, ComponentSurface::default());

        let file = FileRef::new("app.vue");
        let result = db.component_surface(&file, rev2);

        // Positive: returns partial (stale data available)
        assert_eq!(result.completeness, Completeness::Partial);
        // Negative: not complete
        assert!(!result.is_complete());
    }

    #[test]
    fn invalidate_removes_cached_data() {
        let mut db = SemanticDb::new();
        let rev = make_revision(1);
        db.set_component_surface("app.vue".into(), rev, ComponentSurface::default());

        assert_eq!(db.cached_file_count(), 1);
        db.invalidate("app.vue");
        assert_eq!(db.cached_file_count(), 0);

        let file = FileRef::new("app.vue");
        let result = db.component_surface(&file, rev);
        assert_eq!(result.completeness, Completeness::Unavailable);
    }

    #[test]
    fn different_files_are_independent() {
        let mut db = SemanticDb::new();
        let rev = make_revision(1);

        db.set_component_surface("a.vue".into(), rev, ComponentSurface::default());

        let result_a = db.component_surface(&FileRef::new("a.vue"), rev);
        let result_b = db.component_surface(&FileRef::new("b.vue"), rev);

        assert!(result_a.is_complete());
        assert_eq!(result_b.completeness, Completeness::Unavailable);
    }
}
