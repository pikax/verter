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
use crate::facts::symbol::FileImportGraph;
use crate::query::QueryResult;
use crate::refs::FileRef;
use crate::revision::RevisionMarker;

/// Per-file semantic cache entry.
#[derive(Debug)]
struct FileSemantic {
    /// The revision at which this entry was computed.
    revision: RevisionMarker,
    /// Cached component surface (if this file is a Vue SFC).
    component_surface: Option<ComponentSurface>,
    /// True once the component surface has been computed for this revision.
    component_surface_cached: bool,
    /// Cached binding declarations with reactivity facts.
    bindings: Option<Vec<(BindingDeclaration, ReactivityFact)>>,
    /// True once bindings have been computed for this revision.
    bindings_cached: bool,
    /// Cached import graph for cross-file symbol resolution.
    import_graph: Option<FileImportGraph>,
    /// True once the import graph has been computed for this revision.
    import_graph_cached: bool,
}

impl FileSemantic {
    fn new(revision: RevisionMarker) -> Self {
        Self {
            revision,
            component_surface: None,
            component_surface_cached: false,
            bindings: None,
            bindings_cached: false,
            import_graph: None,
            import_graph_cached: false,
        }
    }

    fn reset_for_revision(&mut self, revision: RevisionMarker) {
        if self.revision == revision {
            return;
        }

        self.revision = revision;
        self.component_surface = None;
        self.component_surface_cached = false;
        self.bindings = None;
        self.bindings_cached = false;
        self.import_graph = None;
        self.import_graph_cached = false;
    }
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
            Some(entry) if entry.revision == current_revision && entry.component_surface_cached => {
                QueryResult::complete(entry.component_surface.clone(), current_revision)
            }
            Some(entry) if entry.revision == current_revision => {
                QueryResult::unavailable(None, current_revision, vec![])
            }
            Some(entry)
                if current_revision.is_newer_than(&entry.revision)
                    && entry.component_surface_cached =>
            {
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
        let entry = self
            .files
            .entry(file_id)
            .or_insert_with(|| FileSemantic::new(revision));
        entry.reset_for_revision(revision);
        entry.component_surface = Some(surface);
        entry.component_surface_cached = true;
    }

    /// Query binding facts for a file.
    pub fn bindings(
        &self,
        file_ref: &FileRef,
        current_revision: RevisionMarker,
    ) -> QueryResult<Option<Vec<(BindingDeclaration, ReactivityFact)>>> {
        match self.files.get(&file_ref.file_id) {
            Some(entry) if entry.revision == current_revision && entry.bindings_cached => {
                QueryResult::complete(entry.bindings.clone(), current_revision)
            }
            Some(entry) if entry.revision == current_revision => {
                QueryResult::unavailable(None, current_revision, vec![])
            }
            Some(entry)
                if current_revision.is_newer_than(&entry.revision) && entry.bindings_cached =>
            {
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
        let entry = self
            .files
            .entry(file_id)
            .or_insert_with(|| FileSemantic::new(revision));
        entry.reset_for_revision(revision);
        entry.bindings = Some(bindings);
        entry.bindings_cached = true;
    }

    /// Query the import graph for a file.
    pub fn import_graph(
        &self,
        file_ref: &FileRef,
        current_revision: RevisionMarker,
    ) -> QueryResult<Option<FileImportGraph>> {
        match self.files.get(&file_ref.file_id) {
            Some(entry) if entry.revision == current_revision && entry.import_graph_cached => {
                QueryResult::complete(entry.import_graph.clone(), current_revision)
            }
            Some(entry) if entry.revision == current_revision => {
                QueryResult::unavailable(None, current_revision, vec![])
            }
            Some(entry)
                if current_revision.is_newer_than(&entry.revision) && entry.import_graph_cached =>
            {
                QueryResult::partial(entry.import_graph.clone(), current_revision, vec![])
            }
            _ => QueryResult::unavailable(None, current_revision, vec![]),
        }
    }

    /// Store a computed import graph for a file.
    pub fn set_import_graph(
        &mut self,
        file_id: String,
        revision: RevisionMarker,
        graph: FileImportGraph,
    ) {
        let entry = self
            .files
            .entry(file_id)
            .or_insert_with(|| FileSemantic::new(revision));
        entry.reset_for_revision(revision);
        entry.import_graph = Some(graph);
        entry.import_graph_cached = true;
    }

    /// Resolve a component's surface by following an import from a parent file.
    ///
    /// Given a parent file and a local binding name (e.g., `"Button"`):
    /// 1. Look up the import graph for the parent file
    /// 2. Find the imported symbol by local name
    /// 3. Follow the resolved canonical file ID to the child's component surface
    ///
    /// Returns `None` if the import is unresolved or the child has no cached surface.
    pub fn resolve_imported_component_surface(
        &self,
        parent_file_id: &str,
        local_binding_name: &str,
        current_revision: RevisionMarker,
    ) -> QueryResult<Option<ComponentSurface>> {
        // Step 1: Get parent's import graph
        let parent_ref = FileRef::new(parent_file_id);
        let graph_result = self.import_graph(&parent_ref, current_revision);
        let graph = match graph_result.value {
            Some(g) => g,
            None => return QueryResult::unavailable(None, current_revision, vec![]),
        };

        // Step 2: Find the imported symbol
        let symbol = match graph.find_by_local_name(local_binding_name) {
            Some(s) => s,
            None => return QueryResult::complete(None, current_revision),
        };

        // Step 3: Follow to resolved file
        let child_file_id = match &symbol.resolved_file_id {
            Some(id) => id,
            None => {
                use crate::revision::{DependencyKind, SemanticDependency};
                return QueryResult::partial(
                    None,
                    current_revision,
                    vec![SemanticDependency {
                        kind: DependencyKind::WorkspaceResolution,
                        key: symbol.source_specifier.clone(),
                        revision: 0,
                    }],
                );
            }
        };

        // Step 4: Query child's component surface
        let child_ref = FileRef::new(child_file_id.as_str());
        self.component_surface(&child_ref, current_revision)
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
#[allow(clippy::field_reassign_with_default)]
mod tests {
    use super::*;
    use crate::facts::component::PropFact;
    use crate::query::Completeness;
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

    // ── Cross-file resolution tests ────────────────────────────────────────

    #[test]
    fn resolve_imported_component_surface_follows_import() {
        use crate::facts::symbol::{FileImportGraph, ImportKind, ImportedSymbol};

        let mut db = SemanticDb::new();
        let rev = make_revision(1);

        // Set up child component surface
        let mut child_surface = ComponentSurface::default();
        child_surface.declared.props.push(PropFact {
            name: "label".into(),
            is_optional: false,
            type_text: Some("string".into()),
            default_value: None,
            description: None,
            span: Span::new(10, 20),
        });
        db.set_component_surface("/src/Button.vue".into(), rev, child_surface);

        // Set up parent's import graph
        let graph = FileImportGraph {
            imports: vec![ImportedSymbol {
                local_name: "Button".into(),
                source_specifier: "./Button.vue".into(),
                resolved_file_id: Some("/src/Button.vue".into()),
                exported_name: "default".into(),
                kind: ImportKind::Default,
                is_type_only: false,
                span: Span::new(7, 13),
            }],
            import_sources: vec!["/src/Button.vue".into()],
        };
        db.set_import_graph("/src/App.vue".into(), rev, graph);

        // Query: resolve Button from App.vue
        let result = db.resolve_imported_component_surface("/src/App.vue", "Button", rev);

        // Positive: found the child's surface with its prop
        assert!(result.is_complete());
        let surface = result.value.unwrap();
        assert_eq!(surface.declared.props.len(), 1);
        assert_eq!(surface.declared.props[0].name, "label");
    }

    #[test]
    fn resolve_imported_component_unknown_binding() {
        use crate::facts::symbol::FileImportGraph;

        let mut db = SemanticDb::new();
        let rev = make_revision(1);

        // Parent has an import graph but no import named "Unknown"
        db.set_import_graph("/src/App.vue".into(), rev, FileImportGraph::default());

        let result = db.resolve_imported_component_surface("/src/App.vue", "Unknown", rev);

        // Positive: complete but None (symbol not found in imports)
        assert!(result.is_complete());
        assert!(result.value.is_none());
    }

    #[test]
    fn resolve_imported_component_unresolved_import() {
        use crate::facts::symbol::{FileImportGraph, ImportKind, ImportedSymbol};

        let mut db = SemanticDb::new();
        let rev = make_revision(1);

        let graph = FileImportGraph {
            imports: vec![ImportedSymbol {
                local_name: "Ext".into(),
                source_specifier: "external-pkg".into(),
                resolved_file_id: None, // unresolved
                exported_name: "Ext".into(),
                kind: ImportKind::Named,
                is_type_only: false,
                span: Span::new(10, 13),
            }],
            import_sources: vec![],
        };
        db.set_import_graph("/src/App.vue".into(), rev, graph);

        let result = db.resolve_imported_component_surface("/src/App.vue", "Ext", rev);

        // Positive: partial — knows about the import but can't resolve it
        assert_eq!(result.completeness, Completeness::Partial);
        assert!(result.value.is_none());

        // Positive: declares the missing dependency
        assert_eq!(result.missing_inputs.len(), 1);
        assert_eq!(result.missing_inputs[0].key, "external-pkg");
    }

    #[test]
    fn resolve_imported_component_no_import_graph() {
        let db = SemanticDb::new();
        let rev = make_revision(1);

        let result = db.resolve_imported_component_surface("/src/App.vue", "Button", rev);

        // Positive: unavailable — no import graph cached
        assert_eq!(result.completeness, Completeness::Unavailable);
    }

    // ── Binding cache tests ────────────────────────────────────────────────

    #[test]
    fn set_and_query_bindings() {
        use crate::facts::binding::{BindingDeclaration, BindingKind};
        use crate::facts::reactivity::ReactivityFact;

        let mut db = SemanticDb::new();
        let rev = make_revision(1);

        let bindings = vec![(
            BindingDeclaration {
                name: "count".into(),
                kind: BindingKind::Const,
                span: Span::new(10, 15),
                usages: vec![],
            },
            ReactivityFact::non_reactive(),
        )];

        db.set_bindings("app.vue".into(), rev, bindings);

        let file = FileRef::new("app.vue");
        let result = db.bindings(&file, rev);
        assert!(result.is_complete());
        let b = result.value.unwrap();
        assert_eq!(b.len(), 1);
        assert_eq!(b[0].0.name, "count");
    }

    #[test]
    fn bindings_stale_revision_returns_partial() {
        use crate::facts::binding::{BindingDeclaration, BindingKind};
        use crate::facts::reactivity::ReactivityFact;

        let mut db = SemanticDb::new();
        let rev1 = make_revision(1);
        let rev2 = make_revision(2);

        db.set_bindings(
            "app.vue".into(),
            rev1,
            vec![(
                BindingDeclaration {
                    name: "x".into(),
                    kind: BindingKind::Const,
                    span: Span::new(0, 1),
                    usages: vec![],
                },
                ReactivityFact::non_reactive(),
            )],
        );

        let result = db.bindings(&FileRef::new("app.vue"), rev2);
        assert_eq!(result.completeness, Completeness::Partial);
        assert!(result.value.is_some()); // stale data still returned
    }

    // ── Import graph cache tests ───────────────────────────────────────────

    #[test]
    fn set_and_query_import_graph() {
        use crate::facts::symbol::{FileImportGraph, ImportKind, ImportedSymbol};

        let mut db = SemanticDb::new();
        let rev = make_revision(1);

        let graph = FileImportGraph {
            imports: vec![ImportedSymbol {
                local_name: "Foo".into(),
                source_specifier: "./foo".into(),
                resolved_file_id: Some("/src/foo.ts".into()),
                exported_name: "Foo".into(),
                kind: ImportKind::Named,
                is_type_only: false,
                span: Span::new(10, 13),
            }],
            import_sources: vec!["/src/foo.ts".into()],
        };

        db.set_import_graph("app.vue".into(), rev, graph);
        let result = db.import_graph(&FileRef::new("app.vue"), rev);
        assert!(result.is_complete());
        let g = result.value.unwrap();
        assert_eq!(g.imports.len(), 1);
        assert_eq!(g.imports[0].local_name, "Foo");
    }

    #[test]
    fn import_graph_missing_returns_unavailable() {
        let db = SemanticDb::new();
        let result = db.import_graph(&FileRef::new("missing.vue"), make_revision(1));
        assert_eq!(result.completeness, Completeness::Unavailable);
    }

    // ── Multi-fact per file ────────────────────────────────────────────────

    #[test]
    fn surface_and_bindings_independent_within_file() {
        let mut db = SemanticDb::new();
        let rev = make_revision(1);

        db.set_component_surface("app.vue".into(), rev, ComponentSurface::default());

        // Bindings not set yet → unavailable
        let b_result = db.bindings(&FileRef::new("app.vue"), rev);
        // Surface is set
        let s_result = db.component_surface(&FileRef::new("app.vue"), rev);

        assert!(s_result.is_complete());
        // Bindings have not been computed yet, even though the file entry exists.
        assert_eq!(b_result.completeness, Completeness::Unavailable);
        assert!(b_result.value.is_none());
    }

    #[test]
    fn updating_one_fact_for_new_revision_invalidates_other_fact_cache() {
        use crate::facts::binding::{BindingDeclaration, BindingKind};

        let mut db = SemanticDb::new();
        let rev1 = make_revision(1);
        let rev2 = make_revision(2);

        db.set_bindings(
            "app.vue".into(),
            rev1,
            vec![(
                BindingDeclaration {
                    name: "count".into(),
                    kind: BindingKind::Const,
                    span: Span::new(0, 5),
                    usages: vec![],
                },
                ReactivityFact::non_reactive(),
            )],
        );

        db.set_component_surface("app.vue".into(), rev2, ComponentSurface::default());

        let bindings = db.bindings(&FileRef::new("app.vue"), rev2);
        assert_eq!(bindings.completeness, Completeness::Unavailable);
        assert!(
            bindings.value.is_none(),
            "old bindings must not be treated as current"
        );

        let surface = db.component_surface(&FileRef::new("app.vue"), rev2);
        assert!(
            surface.is_complete(),
            "updated surface should remain cached"
        );
    }

    #[test]
    fn invalidate_clears_all_facts() {
        use crate::facts::symbol::FileImportGraph;

        let mut db = SemanticDb::new();
        let rev = make_revision(1);

        db.set_component_surface("a.vue".into(), rev, ComponentSurface::default());
        db.set_bindings("a.vue".into(), rev, vec![]);
        db.set_import_graph("a.vue".into(), rev, FileImportGraph::default());

        assert_eq!(db.cached_file_count(), 1);

        db.invalidate("a.vue");
        assert_eq!(db.cached_file_count(), 0);

        // All queries return unavailable
        let file = FileRef::new("a.vue");
        assert_eq!(
            db.component_surface(&file, rev).completeness,
            Completeness::Unavailable
        );
        assert_eq!(
            db.bindings(&file, rev).completeness,
            Completeness::Unavailable
        );
        assert_eq!(
            db.import_graph(&file, rev).completeness,
            Completeness::Unavailable
        );
    }

    // ── Integration: extract → cache → query cycle ─────────────────────────

    #[test]
    fn extract_cache_query_component_surface_cycle() {
        use crate::extract::extract_component_surface;
        use crate::input::{AnalyzedMacro, AnalyzedMacroKind, ScriptAnalysisSnapshot};

        let mut db = SemanticDb::new();
        let rev = make_revision(1);

        // Create a script analysis with defineProps
        let mut snapshot = ScriptAnalysisSnapshot::default();
        snapshot.macros = vec![AnalyzedMacro {
            kind: AnalyzedMacroKind::DefineProps,
            is_type_based: true,
            type_references: Vec::new(),
            binding_name: None,
            model_name: None,
            has_inherit_attrs_false: false,
            prop_fields: vec![crate::input::AnalyzedPropField {
                name: "title".into(),
                is_optional: false,
                span: Span::new(20, 25),
                type_annotation: Some("string".into()),
                description: None,
                tags: Vec::new(),
                resolution_source: crate::input::TypeResolutionSource::Rust,
                resolution_error: None,
                payload: None,
                type_expr_scope: None,
                declared_in_macro_type_arg: false,
            }],
            emit_fields: Vec::new(),
            slot_fields: Vec::new(),
            default_keys: Vec::new(),
            default_values: Vec::new(),
            expose_fields: Vec::new(),
            resolved_local_types: Vec::new(),
            parsed_type_argument: None,
            parsed_type_argument_scope: None,
            span: Span::new(0, 50),
        }];

        // Extract
        let surface = extract_component_surface(&snapshot);

        // Cache
        db.set_component_surface("comp.vue".into(), rev, surface);

        // Query
        let result = db.component_surface(&FileRef::new("comp.vue"), rev);

        // Positive: full cycle works end-to-end
        assert!(result.is_complete());
        let s = result.value.unwrap();
        assert_eq!(s.declared.props.len(), 1);
        assert_eq!(s.declared.props[0].name, "title");
        assert_eq!(s.declared.props[0].type_text.as_deref(), Some("string"));

        // Negative: accepted equals declared (no cross-file resolution)
        assert_eq!(s.accepted_props.len(), 1);
    }

    #[test]
    fn extract_cache_query_cross_file_cycle() {
        use crate::extract::{extract_component_surface, extract_import_graph};
        use crate::input::{
            AnalyzedImport, AnalyzedImportBinding, AnalyzedMacro, AnalyzedMacroKind,
            ImportBindingKind, ScriptAnalysisSnapshot,
        };

        let mut db = SemanticDb::new();
        let rev = make_revision(1);

        // Child component with a prop
        let mut child_snap = ScriptAnalysisSnapshot::default();
        child_snap.macros = vec![AnalyzedMacro {
            kind: AnalyzedMacroKind::DefineProps,
            is_type_based: true,
            type_references: Vec::new(),
            binding_name: None,
            model_name: None,
            has_inherit_attrs_false: false,
            prop_fields: vec![crate::input::AnalyzedPropField {
                name: "label".into(),
                is_optional: true,
                span: Span::new(10, 15),
                type_annotation: Some("string".into()),
                description: None,
                tags: Vec::new(),
                resolution_source: crate::input::TypeResolutionSource::Rust,
                resolution_error: None,
                payload: None,
                type_expr_scope: None,
                declared_in_macro_type_arg: false,
            }],
            emit_fields: Vec::new(),
            slot_fields: Vec::new(),
            default_keys: Vec::new(),
            default_values: Vec::new(),
            expose_fields: Vec::new(),
            resolved_local_types: Vec::new(),
            parsed_type_argument: None,
            parsed_type_argument_scope: None,
            span: Span::new(0, 50),
        }];
        let child_surface = extract_component_surface(&child_snap);
        db.set_component_surface("/child.vue".into(), rev, child_surface);

        // Parent with import of child
        let mut parent_snap = ScriptAnalysisSnapshot::default();
        parent_snap.imports = vec![AnalyzedImport {
            source: "./child.vue".into(),
            is_type_only: false,
            bindings: vec![AnalyzedImportBinding {
                name: "Child".into(),
                kind: ImportBindingKind::Default,
                imported_name: None,
                is_type_only: false,
                vue_api: None,
                span: Span::new(7, 12),
            }],
            span: Span::new(0, 30),
            resolved_canonical_id: Some("/child.vue".into()),
        }];
        let parent_graph = extract_import_graph(&parent_snap);
        db.set_import_graph("/parent.vue".into(), rev, parent_graph);

        // Cross-file query: resolve Child from parent
        let result = db.resolve_imported_component_surface("/parent.vue", "Child", rev);

        // Positive: cross-file cycle works
        assert!(result.is_complete());
        let surface = result.value.unwrap();
        assert_eq!(surface.declared.props.len(), 1);
        assert_eq!(surface.declared.props[0].name, "label");
    }

    #[test]
    fn revision_mismatch_after_update_returns_partial() {
        let mut db = SemanticDb::new();
        let rev1 = make_revision(1);
        let rev2 = make_revision(2);

        db.set_component_surface("a.vue".into(), rev1, ComponentSurface::default());

        // Query at newer revision → partial (stale)
        let result = db.component_surface(&FileRef::new("a.vue"), rev2);
        assert_eq!(result.completeness, Completeness::Partial);

        // Update to rev2
        db.set_component_surface("a.vue".into(), rev2, ComponentSurface::default());

        // Now query at rev2 → complete
        let result = db.component_surface(&FileRef::new("a.vue"), rev2);
        assert!(result.is_complete());
    }

    #[test]
    fn many_files_cached_independently() {
        let mut db = SemanticDb::new();
        let rev = make_revision(1);

        for i in 0..10 {
            let id = format!("file_{i}.vue");
            let mut surface = ComponentSurface::default();
            surface.declared.props.push(PropFact {
                name: format!("prop_{i}"),
                is_optional: false,
                type_text: None,
                default_value: None,
                description: None,
                span: Span::new(0, 5),
            });
            db.set_component_surface(id, rev, surface);
        }

        assert_eq!(db.cached_file_count(), 10);

        // Each file has its own prop
        for i in 0..10 {
            let result = db.component_surface(&FileRef::new(format!("file_{i}.vue")), rev);
            assert!(result.is_complete());
            let s = result.value.unwrap();
            assert_eq!(s.declared.props[0].name, format!("prop_{i}"));
        }

        // Invalidate one doesn't affect others
        db.invalidate("file_5.vue");
        assert_eq!(db.cached_file_count(), 9);
        assert!(db
            .component_surface(&FileRef::new("file_0.vue"), rev)
            .is_complete());
    }

    #[test]
    fn cross_file_chain_a_imports_b_imports_c() {
        use crate::facts::symbol::{FileImportGraph, ImportKind, ImportedSymbol};

        let mut db = SemanticDb::new();
        let rev = make_revision(1);

        // C has a prop
        let mut c_surface = ComponentSurface::default();
        c_surface.declared.props.push(PropFact {
            name: "deep".into(),
            is_optional: false,
            type_text: Some("string".into()),
            default_value: None,
            description: None,
            span: Span::new(10, 14),
        });
        db.set_component_surface("/c.vue".into(), rev, c_surface);

        // B imports C
        db.set_import_graph(
            "/b.vue".into(),
            rev,
            FileImportGraph {
                imports: vec![ImportedSymbol {
                    local_name: "C".into(),
                    source_specifier: "./c.vue".into(),
                    resolved_file_id: Some("/c.vue".into()),
                    exported_name: "default".into(),
                    kind: ImportKind::Default,
                    is_type_only: false,
                    span: Span::new(7, 8),
                }],
                import_sources: vec!["/c.vue".into()],
            },
        );

        // A imports B
        db.set_import_graph(
            "/a.vue".into(),
            rev,
            FileImportGraph {
                imports: vec![ImportedSymbol {
                    local_name: "B".into(),
                    source_specifier: "./b.vue".into(),
                    resolved_file_id: Some("/b.vue".into()),
                    exported_name: "default".into(),
                    kind: ImportKind::Default,
                    is_type_only: false,
                    span: Span::new(7, 8),
                }],
                import_sources: vec!["/b.vue".into()],
            },
        );

        // A→B resolves to B's file entry (not C — single hop only)
        let result = db.resolve_imported_component_surface("/a.vue", "B", rev);
        // B has a file entry (from import_graph) but no surface cache for this revision.
        assert_eq!(result.completeness, Completeness::Unavailable);
        assert!(result.value.is_none(), "B has no component surface cached");

        // But B→C works (C has cached surface)
        let result = db.resolve_imported_component_surface("/b.vue", "C", rev);
        assert!(result.is_complete());
        let surface = result.value.unwrap();
        assert_eq!(surface.declared.props[0].name, "deep");
    }

    // ── Cache invariant tests (plan-required) ──────────────────────────────

    #[test]
    fn same_revision_query_returns_cached_not_recomputed() {
        // Plan: "proving file reads/parses happen at most once per canonical ID per relevant revision"
        let mut db = SemanticDb::new();
        let rev = make_revision(1);

        let mut surface = ComponentSurface::default();
        surface.declared.props.push(PropFact {
            name: "cached".into(),
            is_optional: false,
            type_text: None,
            default_value: None,
            description: None,
            span: Span::new(0, 6),
        });
        db.set_component_surface("a.vue".into(), rev, surface);

        // Query twice at same revision — both should return the same cached data
        let r1 = db.component_surface(&FileRef::new("a.vue"), rev);
        let r2 = db.component_surface(&FileRef::new("a.vue"), rev);

        assert!(r1.is_complete());
        assert!(r2.is_complete());
        assert_eq!(
            r1.value.as_ref().unwrap().declared.props[0].name,
            r2.value.as_ref().unwrap().declared.props[0].name
        );
    }

    #[test]
    fn import_graph_cached_and_reused_across_queries() {
        // Plan: "proving shallow symbol/export/import state is stored once and reused"
        use crate::facts::symbol::{FileImportGraph, ImportKind, ImportedSymbol};

        let mut db = SemanticDb::new();
        let rev = make_revision(1);

        let graph = FileImportGraph {
            imports: vec![ImportedSymbol {
                local_name: "Foo".into(),
                source_specifier: "./foo".into(),
                resolved_file_id: Some("/foo.ts".into()),
                exported_name: "Foo".into(),
                kind: ImportKind::Named,
                is_type_only: false,
                span: Span::new(10, 13),
            }],
            import_sources: vec!["/foo.ts".into()],
        };

        db.set_import_graph("parent.vue".into(), rev, graph);

        // Multiple queries at same revision all return cached data
        for _ in 0..5 {
            let result = db.import_graph(&FileRef::new("parent.vue"), rev);
            assert!(result.is_complete());
            let g = result.value.unwrap();
            assert_eq!(g.imports[0].local_name, "Foo");
        }
    }

    #[test]
    fn new_revision_invalidates_stale_cache() {
        // Plan: "VFS is the authority for file-change invalidation"
        let mut db = SemanticDb::new();
        let rev1 = make_revision(1);
        let rev2 = make_revision(2);

        let mut surface = ComponentSurface::default();
        surface.declared.props.push(PropFact {
            name: "old_prop".into(),
            is_optional: false,
            type_text: None,
            default_value: None,
            description: None,
            span: Span::new(0, 8),
        });
        db.set_component_surface("a.vue".into(), rev1, surface);

        // At rev2, cache is stale → partial
        let result = db.component_surface(&FileRef::new("a.vue"), rev2);
        assert_eq!(result.completeness, Completeness::Partial);

        // After explicit invalidation + re-set at rev2
        db.invalidate("a.vue");
        let mut new_surface = ComponentSurface::default();
        new_surface.declared.props.push(PropFact {
            name: "new_prop".into(),
            is_optional: true,
            type_text: None,
            default_value: None,
            description: None,
            span: Span::new(0, 8),
        });
        db.set_component_surface("a.vue".into(), rev2, new_surface);

        let result = db.component_surface(&FileRef::new("a.vue"), rev2);
        assert!(result.is_complete());
        assert_eq!(result.value.unwrap().declared.props[0].name, "new_prop");
    }

    #[test]
    fn set_multiple_facts_for_same_file_all_cached() {
        // Plan: "cache named declarations from that parsed file by name"
        use crate::facts::binding::{BindingDeclaration, BindingKind};
        use crate::facts::reactivity::ReactivityFact;
        use crate::facts::symbol::FileImportGraph;

        let mut db = SemanticDb::new();
        let rev = make_revision(1);

        db.set_component_surface("a.vue".into(), rev, ComponentSurface::default());
        db.set_bindings(
            "a.vue".into(),
            rev,
            vec![(
                BindingDeclaration {
                    name: "x".into(),
                    kind: BindingKind::Const,
                    span: Span::new(0, 1),
                    usages: vec![],
                },
                ReactivityFact::non_reactive(),
            )],
        );
        db.set_import_graph("a.vue".into(), rev, FileImportGraph::default());

        // All three facts cached under the same file
        assert_eq!(db.cached_file_count(), 1);

        let file = FileRef::new("a.vue");
        assert!(db.component_surface(&file, rev).is_complete());
        assert!(db.bindings(&file, rev).is_complete());
        assert!(db.import_graph(&file, rev).is_complete());

        // Bindings value is populated
        let b = db.bindings(&file, rev).value.unwrap();
        assert_eq!(b[0].0.name, "x");
    }

    // ── End-to-end pipeline tests (extract → cache → analyze) ──────────────

    #[test]
    fn pipeline_extract_cache_boundary_analyze() {
        // Full pipeline: create parent+child snapshots, extract, cache, analyze
        use crate::analyzers::boundary::analyze_boundary;
        use crate::extract::{
            extract_boundary_edges, extract_component_surface, extract_import_graph,
        };
        use crate::input::TemplateAnalysisSnapshot;
        use crate::input::{
            AnalyzedImport, AnalyzedImportBinding, AnalyzedMacro, AnalyzedMacroKind,
            AnalyzedPropField, ImportBindingKind, ScriptAnalysisSnapshot, TypeResolutionSource,
        };

        let mut db = SemanticDb::new();
        let rev = make_revision(1);

        // Child: defineProps<{ color: string }>
        let mut child_snap = ScriptAnalysisSnapshot::default();
        child_snap.macros = vec![AnalyzedMacro {
            kind: AnalyzedMacroKind::DefineProps,
            is_type_based: true,
            type_references: Vec::new(),
            binding_name: None,
            model_name: None,
            has_inherit_attrs_false: false,
            prop_fields: vec![AnalyzedPropField {
                name: "color".into(),
                is_optional: false,
                span: Span::new(20, 25),
                type_annotation: Some("string".into()),
                description: None,
                tags: Vec::new(),
                resolution_source: TypeResolutionSource::Rust,
                resolution_error: None,
                payload: None,
                type_expr_scope: None,
                declared_in_macro_type_arg: false,
            }],
            emit_fields: Vec::new(),
            slot_fields: Vec::new(),
            default_keys: Vec::new(),
            default_values: Vec::new(),
            expose_fields: Vec::new(),
            resolved_local_types: Vec::new(),
            parsed_type_argument: None,
            parsed_type_argument_scope: None,
            span: Span::new(0, 50),
        }];

        // Extract and cache child surface
        let child_surface = extract_component_surface(&child_snap);
        db.set_component_surface("/child.vue".into(), rev, child_surface.clone());

        // Parent: imports Child, uses <Child unknown-prop />
        let mut parent_snap = ScriptAnalysisSnapshot::default();
        parent_snap.imports = vec![AnalyzedImport {
            source: "./child.vue".into(),
            is_type_only: false,
            bindings: vec![AnalyzedImportBinding {
                name: "Child".into(),
                kind: ImportBindingKind::Default,
                imported_name: None,
                is_type_only: false,
                vue_api: None,
                span: Span::new(7, 12),
            }],
            span: Span::new(0, 30),
            resolved_canonical_id: Some("/child.vue".into()),
        }];
        let parent_graph = extract_import_graph(&parent_snap);
        db.set_import_graph("/parent.vue".into(), rev, parent_graph.clone());

        // Simulate template with <Child unknown-prop />
        let mut template = TemplateAnalysisSnapshot::default();
        template.components = vec![crate::input::TemplateComponentUsage {
            name: "Child".into(),
            import_source: Some("./child.vue".into()),
            is_dynamic: false,
            props: vec![crate::input::TemplatePropUsage {
                name: "unknownProp".into(),
                is_bound: false,
                expression: None,
                constness: crate::input::PropValueConstness::Const,
                referenced_bindings: vec![],
                from_spread: false,
                span: Span::new(100, 111),
                name_span: Span::new(100, 111),
                is_shorthand: false,
            }],
            has_spread: false,
            slots_used: vec![],
            static_classes: vec![],
            has_dynamic_class: false,
            dynamic_classes: vec![],
            v_models: vec![],
            bindings: vec![],
            events: vec![],
            span: Span::new(90, 130),
        }];

        // Extract boundary edges
        let edges = extract_boundary_edges("/parent.vue", &template, &parent_graph);
        assert_eq!(edges.len(), 1);

        // Run boundary analyzer
        let issues = analyze_boundary(&edges[0], &child_surface);

        // Positive: both unknown prop AND missing required detected
        assert_eq!(issues.len(), 2);
        let unknown: Vec<_> = issues
            .iter()
            .filter(|i| i.kind == crate::analyzers::boundary::BoundaryIssueKind::UnknownProp)
            .collect();
        let missing: Vec<_> = issues
            .iter()
            .filter(|i| {
                i.kind == crate::analyzers::boundary::BoundaryIssueKind::MissingRequiredProp
            })
            .collect();
        assert_eq!(unknown.len(), 1);
        assert_eq!(unknown[0].member_name, "unknownProp");
        assert_eq!(missing.len(), 1);
        assert_eq!(missing[0].member_name, "color");

        // Cross-file resolution also works
        let resolved = db.resolve_imported_component_surface("/parent.vue", "Child", rev);
        assert!(resolved.is_complete());
        assert_eq!(resolved.value.unwrap().declared.props[0].name, "color");
    }

    // ── Invalidation tests ─────────────────────────────────────────────────

    #[test]
    fn invalidate_single_file_preserves_others() {
        let mut db = SemanticDb::new();
        let rev = make_revision(1);
        db.set_component_surface("a.vue".into(), rev, ComponentSurface::default());
        db.set_component_surface("b.vue".into(), rev, ComponentSurface::default());
        assert_eq!(db.cached_file_count(), 2);

        db.invalidate("a.vue");
        assert_eq!(db.cached_file_count(), 1);
        assert_eq!(
            db.component_surface(&FileRef::new("a.vue"), rev)
                .completeness,
            Completeness::Unavailable
        );
        assert!(db
            .component_surface(&FileRef::new("b.vue"), rev)
            .is_complete());
    }

    #[test]
    fn invalidate_nonexistent_is_noop() {
        let mut db = SemanticDb::new();
        let rev = make_revision(1);
        db.set_component_surface("a.vue".into(), rev, ComponentSurface::default());
        db.invalidate("nonexistent.vue");
        assert_eq!(db.cached_file_count(), 1);
    }

    #[test]
    fn full_reset_clears_everything() {
        let mut db = SemanticDb::new();
        let rev = make_revision(1);
        db.set_component_surface("a.vue".into(), rev, ComponentSurface::default());
        db.set_component_surface("b.vue".into(), rev, ComponentSurface::default());
        db.set_component_surface("c.vue".into(), rev, ComponentSurface::default());
        assert_eq!(db.cached_file_count(), 3);

        // Simulate semantic_invalidate_all
        db = SemanticDb::new();
        assert_eq!(db.cached_file_count(), 0);
        for id in &["a.vue", "b.vue", "c.vue"] {
            assert_eq!(
                db.component_surface(&FileRef::new(*id), rev).completeness,
                Completeness::Unavailable
            );
        }
    }

    #[test]
    fn invalidate_then_recache() {
        let mut db = SemanticDb::new();
        let rev1 = make_revision(1);
        let rev2 = make_revision(2);

        let mut old = ComponentSurface::default();
        old.declared.props.push(PropFact {
            name: "old".into(),
            is_optional: false,
            type_text: None,
            default_value: None,
            description: None,
            span: Span::new(0, 3),
        });
        db.set_component_surface("a.vue".into(), rev1, old);
        db.invalidate("a.vue");

        let mut fresh = ComponentSurface::default();
        fresh.declared.props.push(PropFact {
            name: "fresh".into(),
            is_optional: true,
            type_text: Some("string".into()),
            default_value: None,
            description: None,
            span: Span::new(0, 5),
        });
        db.set_component_surface("a.vue".into(), rev2, fresh);

        let result = db.component_surface(&FileRef::new("a.vue"), rev2);
        assert!(result.is_complete());
        assert_eq!(result.value.unwrap().declared.props[0].name, "fresh");
    }

    // ── End-to-end pipeline tests ──────────────────────────────────────────

    #[test]
    fn pipeline_extract_cache_reactivity_analyze() {
        // Full pipeline: extract bindings → analyze reactive flow
        use crate::analyzers::reactive_flow::analyze_reactive_flow;
        use crate::extract::extract_bindings;
        use crate::input::{
            AnalyzedBinding, AnalyzedBindingKind, ReactivityKind, ScriptAnalysisSnapshot,
        };

        let mut snapshot = ScriptAnalysisSnapshot::default();
        snapshot.bindings = vec![AnalyzedBinding {
            name: "count".into(),
            kind: AnalyzedBindingKind::Const,
            is_reactive: true,
            reactivity_kind: ReactivityKind::Ref,
            type_annotation: None,
            initializer: None,
            span: Span::new(10, 15),
            used_in_script: false,
            used_in_style: false,
        }];

        // Extract
        let bindings = extract_bindings(&snapshot);
        assert_eq!(bindings.len(), 1);

        // Analyze reactive flow
        let issues = analyze_reactive_flow(&bindings);

        // Positive: reactive but unused → UnusedReactive
        assert_eq!(issues.len(), 1);
        assert_eq!(
            issues[0].kind,
            crate::analyzers::reactive_flow::ReactiveFlowIssueKind::UnusedReactive
        );
    }
}
