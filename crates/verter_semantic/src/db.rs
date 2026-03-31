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
    /// Cached binding declarations with reactivity facts.
    bindings: Option<Vec<(BindingDeclaration, ReactivityFact)>>,
    /// Cached import graph for cross-file symbol resolution.
    import_graph: Option<FileImportGraph>,
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
            import_graph: None,
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
            import_graph: None,
        });
        entry.revision = revision;
        entry.bindings = Some(bindings);
    }

    /// Query the import graph for a file.
    pub fn import_graph(
        &self,
        file_ref: &FileRef,
        current_revision: RevisionMarker,
    ) -> QueryResult<Option<FileImportGraph>> {
        match self.files.get(&file_ref.file_id) {
            Some(entry) if entry.revision == current_revision => {
                QueryResult::complete(entry.import_graph.clone(), current_revision)
            }
            Some(entry) if current_revision.is_newer_than(&entry.revision) => {
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
        let entry = self.files.entry(file_id).or_insert_with(|| FileSemantic {
            revision,
            component_surface: None,
            bindings: None,
            import_graph: None,
        });
        entry.revision = revision;
        entry.import_graph = Some(graph);
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
        // Bindings were not set, but the file entry exists with None bindings
        assert!(b_result.is_complete()); // complete with None value
        assert!(b_result.value.is_none());
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
        use verter_analysis::types::{AnalyzedMacro, AnalyzedMacroKind, ScriptAnalysisSnapshot};

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
            prop_fields: vec![verter_analysis::types::AnalyzedPropField {
                name: "title".into(),
                is_optional: false,
                span: Span::new(20, 25),
                type_annotation: Some("string".into()),
                description: None,
                tags: Vec::new(),
                resolution_source: verter_analysis::types::TypeResolutionSource::Rust,
                resolution_error: None,
            }],
            emit_fields: Vec::new(),
            slot_fields: Vec::new(),
            default_keys: Vec::new(),
            default_values: Vec::new(),
            expose_fields: Vec::new(),
            resolved_local_types: Vec::new(),
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
        use verter_analysis::types::{
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
            prop_fields: vec![verter_analysis::types::AnalyzedPropField {
                name: "label".into(),
                is_optional: true,
                span: Span::new(10, 15),
                type_annotation: Some("string".into()),
                description: None,
                tags: Vec::new(),
                resolution_source: verter_analysis::types::TypeResolutionSource::Rust,
                resolution_error: None,
            }],
            emit_fields: Vec::new(),
            slot_fields: Vec::new(),
            default_keys: Vec::new(),
            default_values: Vec::new(),
            expose_fields: Vec::new(),
            resolved_local_types: Vec::new(),
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
}
