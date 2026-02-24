use oxc_allocator::Allocator;
use oxc_ast::ast::*;
use oxc_parser::{ParseOptions, Parser};
use oxc_span::SourceType;

use crate::classify::classify_vue_api;
use crate::types::{AnalyzedImport, AnalyzedImportBinding, ImportSourceInfo};

/// Extract lightweight import source info from script content.
/// Returns source strings and binding names for each import declaration.
pub fn extract_import_sources(
    content: &str,
    source_type: SourceType,
    allocator: &Allocator,
) -> Vec<ImportSourceInfo> {
    let parser = Parser::new(allocator, content, source_type).with_options(ParseOptions {
        parse_regular_expression: false,
        ..ParseOptions::default()
    });
    let result = parser.parse();
    if result.panicked {
        return Vec::new();
    }

    let mut out = Vec::new();
    for stmt in &result.program.body {
        match stmt {
            Statement::ImportDeclaration(decl) => {
                let source = decl.source.value.to_string();
                let is_type_only = decl.import_kind.is_type();
                let mut bindings = Vec::new();

                if let Some(specifiers) = &decl.specifiers {
                    for spec in specifiers {
                        match spec {
                            ImportDeclarationSpecifier::ImportSpecifier(s) => {
                                bindings.push(s.local.name.to_string());
                            }
                            ImportDeclarationSpecifier::ImportDefaultSpecifier(s) => {
                                bindings.push(s.local.name.to_string());
                            }
                            ImportDeclarationSpecifier::ImportNamespaceSpecifier(s) => {
                                bindings.push(s.local.name.to_string());
                            }
                        }
                    }
                }

                out.push(ImportSourceInfo {
                    source,
                    is_type_only,
                    bindings,
                });
            }
            Statement::ExportNamedDeclaration(decl) => {
                // Re-exports: `export { Foo } from './other'`
                if let Some(ref source) = decl.source {
                    let mut bindings = Vec::new();
                    for spec in &decl.specifiers {
                        let name = spec.exported.name().to_string();
                        bindings.push(name);
                    }
                    out.push(ImportSourceInfo {
                        source: source.value.to_string(),
                        is_type_only: decl.export_kind.is_type(),
                        bindings,
                    });
                }
            }
            _ => {}
        }
    }

    out
}

/// Analyze a single import declaration into an `AnalyzedImport`.
/// Called per-statement from the single-pass AST walk in `build_script_analysis`.
pub(crate) fn analyze_import_declaration(decl: &ImportDeclaration<'_>) -> AnalyzedImport {
    let source = decl.source.value.to_string();
    let is_type_only = decl.import_kind.is_type();
    let is_vue = is_vue_source(&source);

    let mut bindings = Vec::new();
    if let Some(specifiers) = &decl.specifiers {
        for spec in specifiers {
            match spec {
                ImportDeclarationSpecifier::ImportSpecifier(s) => {
                    let local_name = s.local.name.to_string();
                    let spec_type_only = s.import_kind.is_type();
                    // Classify by the *imported* (original) name, not the local alias.
                    // e.g. `import { ref as myRef } from 'vue'` → classify "ref", not "myRef"
                    let vue_api = if is_vue {
                        Some(classify_vue_api(&s.imported.name()))
                    } else {
                        None
                    };
                    bindings.push(AnalyzedImportBinding {
                        name: local_name,
                        is_type_only: spec_type_only,
                        vue_api,
                        span_start: s.local.span.start,
                        span_end: s.local.span.end,
                    });
                }
                ImportDeclarationSpecifier::ImportDefaultSpecifier(s) => {
                    bindings.push(AnalyzedImportBinding {
                        name: s.local.name.to_string(),
                        is_type_only: false,
                        vue_api: None,
                        span_start: s.local.span.start,
                        span_end: s.local.span.end,
                    });
                }
                ImportDeclarationSpecifier::ImportNamespaceSpecifier(s) => {
                    bindings.push(AnalyzedImportBinding {
                        name: s.local.name.to_string(),
                        is_type_only: false,
                        vue_api: None,
                        span_start: s.local.span.start,
                        span_end: s.local.span.end,
                    });
                }
            }
        }
    }

    AnalyzedImport {
        source,
        is_type_only,
        bindings,
        span_start: decl.span.start,
        span_end: decl.span.end,
        resolved_canonical_id: None,
    }
}

/// Check if a source specifier refers to Vue.
fn is_vue_source(source: &str) -> bool {
    source == "vue"
        || source == "vue/dist/vue.esm-bundler.js"
        || source.starts_with("vue/")
        || source == "@vue/runtime-core"
        || source == "@vue/runtime-dom"
        || source == "@vue/reactivity"
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::VueApiClassification;

    fn parse_imports(code: &str) -> Vec<ImportSourceInfo> {
        let alloc = Allocator::new();
        let source_type = SourceType::ts();
        extract_import_sources(code, source_type, &alloc)
    }

    /// @ai-generated - Basic relative import with named bindings
    #[test]
    fn relative_import_with_bindings() {
        let result = parse_imports("import { MyType, OtherType } from './types';");
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].source, "./types");
        assert!(!result[0].is_type_only);
        assert_eq!(result[0].bindings, vec!["MyType", "OtherType"]);
    }

    /// @ai-generated - Type-only import
    #[test]
    fn type_only_import() {
        let result = parse_imports("import type { MyType } from './types';");
        assert_eq!(result.len(), 1);
        assert!(result[0].is_type_only);
        assert_eq!(result[0].bindings, vec!["MyType"]);
    }

    /// @ai-generated - Bare specifier (e.g., from node_modules)
    #[test]
    fn bare_specifier() {
        let result = parse_imports("import { ref, computed } from 'vue';");
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].source, "vue");
        assert_eq!(result[0].bindings, vec!["ref", "computed"]);
    }

    /// @ai-generated - Default import
    #[test]
    fn default_import() {
        let result = parse_imports("import MyComponent from './MyComponent.vue';");
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].bindings, vec!["MyComponent"]);
    }

    /// @ai-generated - Namespace import
    #[test]
    fn namespace_import() {
        let result = parse_imports("import * as Utils from './utils';");
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].bindings, vec!["Utils"]);
    }

    /// @ai-generated - Re-export with source
    #[test]
    fn reexport_with_source() {
        let result = parse_imports("export { Foo, Bar } from './other';");
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].source, "./other");
        assert_eq!(result[0].bindings, vec!["Foo", "Bar"]);
    }

    /// @ai-generated - Multiple imports
    #[test]
    fn multiple_imports() {
        let result =
            parse_imports("import { ref } from 'vue';\nimport type { MyType } from './types';");
        assert_eq!(result.len(), 2);
        assert_eq!(result[0].source, "vue");
        assert_eq!(result[1].source, "./types");
        assert!(result[1].is_type_only);
    }

    /// @ai-generated - Parse error doesn't panic
    #[test]
    fn parse_error_graceful() {
        // Should not panic, returns either empty or partial results
        let result = parse_imports("import { from");
        // OXC is lenient, but we verify at minimum that the function doesn't panic
        // and returns a finite-length result
        assert!(
            result.len() <= 1,
            "parse error should return empty or minimal partial results"
        );
    }

    /// @ai-generated - Side-effect import (no bindings)
    #[test]
    fn side_effect_import() {
        let result = parse_imports("import './styles.css';");
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].source, "./styles.css");
        assert!(result[0].bindings.is_empty());
    }

    fn analyze_imports(code: &str) -> Vec<AnalyzedImport> {
        let alloc = Allocator::new();
        let parser =
            Parser::new(&alloc, code, SourceType::ts()).with_options(ParseOptions::default());
        let result = parser.parse();
        result
            .program
            .body
            .iter()
            .filter_map(|stmt| {
                if let Statement::ImportDeclaration(decl) = stmt {
                    Some(analyze_import_declaration(decl))
                } else {
                    None
                }
            })
            .collect()
    }

    /// @ai-generated - Vue API classification in analyzed imports
    #[test]
    fn analyze_imports_classifies_vue_apis() {
        let imports = analyze_imports("import { ref, MyType } from 'vue';");
        assert_eq!(imports.len(), 1);
        assert_eq!(
            imports[0].bindings[0].vue_api,
            Some(VueApiClassification::Ref)
        );
        assert_eq!(
            imports[0].bindings[1].vue_api,
            Some(VueApiClassification::Other)
        );
    }

    /// @ai-generated - Aliased Vue API import is classified by imported name, not local alias
    #[test]
    fn aliased_vue_import_classified_by_imported_name() {
        let imports = analyze_imports("import { ref as myRef, computed as calc } from 'vue';");
        assert_eq!(imports.len(), 1);
        // Local name is the alias
        assert_eq!(imports[0].bindings[0].name, "myRef");
        assert_eq!(imports[0].bindings[1].name, "calc");
        // But Vue API classification must be based on the imported (original) name
        assert_eq!(
            imports[0].bindings[0].vue_api,
            Some(VueApiClassification::Ref),
            "ref aliased as myRef should still be classified as Ref"
        );
        assert_eq!(
            imports[0].bindings[1].vue_api,
            Some(VueApiClassification::Computed),
            "computed aliased as calc should still be classified as Computed"
        );
    }
}
