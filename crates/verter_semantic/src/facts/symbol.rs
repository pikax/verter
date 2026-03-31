//! Cross-file symbol identity facts.
//!
//! Tracks imported symbols, their origins, and re-export chains.
//! The semantic DB uses these to resolve cross-file references
//! without re-walking the import graph on every query.

use serde::{Deserialize, Serialize};
use verter_span::Span;

/// An imported symbol's identity in the semantic model.
///
/// Tracks the local binding name, the canonical source file,
/// and the original exported name in that file.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct ImportedSymbol {
    /// Local name as used in this file.
    pub local_name: String,
    /// Import source specifier as written (e.g., `"./types"`, `"vue"`).
    pub source_specifier: String,
    /// Canonical file ID of the resolved source (None if unresolved).
    pub resolved_file_id: Option<String>,
    /// Original exported name in the source file (e.g., `"default"`, `"Foo"`).
    pub exported_name: String,
    /// Import syntax form.
    pub kind: ImportKind,
    /// Whether this is a type-only import.
    pub is_type_only: bool,
    /// SFC-absolute span of the import specifier.
    pub span: Span,
}

/// How a symbol was imported.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum ImportKind {
    /// `import { Foo } from "..."`
    Named,
    /// `import Foo from "..."`
    Default,
    /// `import * as Foo from "..."`
    Namespace,
}

/// A file's complete import graph as seen by the semantic model.
///
/// Extracted from `ScriptAnalysisSnapshot.imports` with resolved canonical IDs.
/// This is the semantic entry point for cross-file symbol resolution.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct FileImportGraph {
    /// All imported symbols in this file.
    pub imports: Vec<ImportedSymbol>,
    /// Distinct canonical file IDs that this file imports from.
    pub import_sources: Vec<String>,
}

impl FileImportGraph {
    /// Find an imported symbol by its local name.
    pub fn find_by_local_name(&self, name: &str) -> Option<&ImportedSymbol> {
        self.imports.iter().find(|s| s.local_name == name)
    }

    /// Find all imports from a specific canonical file.
    pub fn imports_from(&self, canonical_id: &str) -> Vec<&ImportedSymbol> {
        self.imports
            .iter()
            .filter(|s| s.resolved_file_id.as_deref() == Some(canonical_id))
            .collect()
    }

    /// Returns true if this file has any unresolved imports.
    pub fn has_unresolved(&self) -> bool {
        self.imports.iter().any(|s| s.resolved_file_id.is_none())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_import(local: &str, source: &str, resolved: Option<&str>) -> ImportedSymbol {
        ImportedSymbol {
            local_name: local.to_string(),
            source_specifier: source.to_string(),
            resolved_file_id: resolved.map(|s| s.to_string()),
            exported_name: local.to_string(),
            kind: ImportKind::Named,
            is_type_only: false,
            span: Span::new(0, 10),
        }
    }

    #[test]
    fn empty_import_graph() {
        let graph = FileImportGraph::default();
        assert!(graph.imports.is_empty());
        assert!(graph.import_sources.is_empty());
        assert!(!graph.has_unresolved());
    }

    #[test]
    fn find_by_local_name() {
        let graph = FileImportGraph {
            imports: vec![
                make_import("Foo", "./foo", Some("/src/foo.ts")),
                make_import("Bar", "./bar", Some("/src/bar.ts")),
            ],
            import_sources: vec!["/src/foo.ts".into(), "/src/bar.ts".into()],
        };

        // Positive: finds by name
        let found = graph.find_by_local_name("Foo");
        assert!(found.is_some());
        assert_eq!(found.unwrap().source_specifier, "./foo");

        // Negative: not found
        assert!(graph.find_by_local_name("Baz").is_none());
    }

    #[test]
    fn imports_from_filters_by_source() {
        let graph = FileImportGraph {
            imports: vec![
                make_import("Foo", "./shared", Some("/src/shared.ts")),
                make_import("Bar", "./shared", Some("/src/shared.ts")),
                make_import("Qux", "./other", Some("/src/other.ts")),
            ],
            import_sources: vec!["/src/shared.ts".into(), "/src/other.ts".into()],
        };

        let from_shared = graph.imports_from("/src/shared.ts");
        assert_eq!(from_shared.len(), 2);

        let from_other = graph.imports_from("/src/other.ts");
        assert_eq!(from_other.len(), 1);
        assert_eq!(from_other[0].local_name, "Qux");
    }

    #[test]
    fn has_unresolved_detects_missing_ids() {
        let graph = FileImportGraph {
            imports: vec![
                make_import("Foo", "./foo", Some("/src/foo.ts")),
                make_import("Bar", "external-pkg", None),
            ],
            import_sources: vec!["/src/foo.ts".into()],
        };

        assert!(graph.has_unresolved());
    }

    #[test]
    fn all_resolved_returns_false() {
        let graph = FileImportGraph {
            imports: vec![make_import("Foo", "./foo", Some("/src/foo.ts"))],
            import_sources: vec!["/src/foo.ts".into()],
        };

        assert!(!graph.has_unresolved());
    }

    #[test]
    fn default_import_kind() {
        let sym = ImportedSymbol {
            local_name: "App".into(),
            source_specifier: "./App.vue".into(),
            resolved_file_id: Some("/src/App.vue".into()),
            exported_name: "default".into(),
            kind: ImportKind::Default,
            is_type_only: false,
            span: Span::new(7, 10),
        };

        assert_eq!(sym.kind, ImportKind::Default);
        assert_eq!(sym.exported_name, "default");
        // Negative: not named or namespace
        assert_ne!(sym.kind, ImportKind::Named);
        assert_ne!(sym.kind, ImportKind::Namespace);
    }
}
