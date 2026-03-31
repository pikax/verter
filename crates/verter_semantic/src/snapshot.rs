//! Aggregated semantic snapshot for a file.
//!
//! A [`FileSemanticSnapshot`] combines all cached semantic facts for a single
//! file into one structure, suitable for consumers that need the full picture
//! (e.g., diagnostics, MCP, component-meta).

use serde::{Deserialize, Serialize};

use crate::facts::binding::BindingDeclaration;
use crate::facts::boundary::ComponentInstanceEdge;
use crate::facts::component::ComponentSurface;
use crate::facts::reactivity::ReactivityFact;
use crate::facts::symbol::FileImportGraph;
use crate::revision::RevisionMarker;

/// Aggregated semantic facts for a single file.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FileSemanticSnapshot {
    /// Canonical file ID.
    pub file_id: String,
    /// Revision at which this snapshot was computed.
    pub revision: RevisionMarker,
    /// Component surface (if this is a Vue SFC).
    pub component_surface: Option<ComponentSurface>,
    /// Binding declarations with reactivity classifications.
    pub bindings: Vec<(BindingDeclaration, ReactivityFact)>,
    /// Import graph for cross-file resolution.
    pub import_graph: FileImportGraph,
    /// Component-instance edges from this file's template.
    pub boundary_edges: Vec<ComponentInstanceEdge>,
}

impl FileSemanticSnapshot {
    /// Create an empty snapshot for a file with no cached data.
    pub fn empty(file_id: String, revision: RevisionMarker) -> Self {
        Self {
            file_id,
            revision,
            component_surface: None,
            bindings: Vec::new(),
            import_graph: FileImportGraph::default(),
            boundary_edges: Vec::new(),
        }
    }

    /// Find a binding by name.
    pub fn find_binding(&self, name: &str) -> Option<&(BindingDeclaration, ReactivityFact)> {
        self.bindings.iter().find(|(decl, _)| decl.name == name)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_snapshot() {
        let snap = FileSemanticSnapshot::empty("app.vue".into(), RevisionMarker::initial());
        assert_eq!(snap.file_id, "app.vue");
        assert!(snap.component_surface.is_none());
        assert!(snap.bindings.is_empty());
        assert!(snap.import_graph.imports.is_empty());
    }

    #[test]
    fn find_binding_by_name() {
        use crate::facts::binding::{BindingDeclaration, BindingKind};
        use crate::facts::reactivity::ReactivityFact;

        let snap = FileSemanticSnapshot {
            file_id: "app.vue".into(),
            revision: RevisionMarker::initial(),
            component_surface: None,
            bindings: vec![(
                BindingDeclaration {
                    name: "count".into(),
                    kind: BindingKind::Const,
                    span: verter_span::Span::new(10, 15),
                    usages: vec![],
                },
                ReactivityFact::non_reactive(),
            )],
            import_graph: FileImportGraph::default(),
            boundary_edges: Vec::new(),
        };

        // Positive: found
        assert!(snap.find_binding("count").is_some());
        // Negative: not found
        assert!(snap.find_binding("missing").is_none());
    }
}
