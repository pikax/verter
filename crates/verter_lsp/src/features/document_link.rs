// Document links: make import paths and src attributes clickable.

use tower_lsp_server::ls_types::*;
use verter_session::FileAnalysisSnapshot;

use crate::documents::carrier_structure::CarrierBlockView;
use crate::documents::line_index::LineIndex;
use crate::features::sentinel_uris::UNKNOWN_FILE_URI;

/// Build document links from import source paths.
///
/// Produces clickable links for:
/// - `import ... from './Foo.vue'` → link on the source string
/// - `<script src="...">` and `<style src="...">` → link on the src attribute value
///
/// Only produces links for imports with a `resolved_canonical_id` (relative imports that
/// the host resolved to an absolute path).
pub fn build_document_links(
    source: &str,
    blocks: &[CarrierBlockView],
    analysis: Option<&FileAnalysisSnapshot>,
    line_index: &LineIndex,
) -> Vec<DocumentLink> {
    let mut links = Vec::new();

    // Links from import statements
    if let Some(analysis) = analysis {
        for import in &analysis.imports {
            if let Some(ref canonical_id) = import.resolved_canonical_id {
                // Find the source string literal within the import statement span
                if let Some((src_start, src_end)) = find_import_source_span(
                    source,
                    import.span.start,
                    import.span.end,
                    &import.source,
                ) {
                    if let (Some(start), Some(end)) = (
                        line_index.offset_to_position(src_start),
                        line_index.offset_to_position(src_end),
                    ) {
                        let target = canonical_id_to_uri(canonical_id);
                        links.push(DocumentLink {
                            range: Range { start, end },
                            target: Some(target),
                            tooltip: Some(canonical_id.clone()),
                            data: None,
                        });
                    }
                }
            }
        }
    }

    // Links from src attributes on SFC blocks
    for block in blocks {
        if let Some(attribute) = block
            .attributes
            .iter()
            .find(|attribute| attribute.name == "src")
        {
            if let (Some(val_start), Some(val_end)) = (attribute.value_start, attribute.value_end) {
                if let (Some(start), Some(end)) = (
                    line_index.offset_to_position(val_start),
                    line_index.offset_to_position(val_end),
                ) {
                    let tooltip = source
                        .get(val_start as usize..val_end as usize)
                        .map(|s| s.to_string());
                    links.push(DocumentLink {
                        range: Range { start, end },
                        target: None, // Client resolves relative path
                        tooltip,
                        data: None,
                    });
                }
            }
        }
    }

    links
}

/// Find the byte range of the import source string literal within an import statement.
///
/// Searches for the quoted string matching `source` between `span_start` and `span_end`.
/// Returns the range of the string content (inside quotes).
fn find_import_source_span(
    text: &str,
    span_start: u32,
    span_end: u32,
    source: &str,
) -> Option<(u32, u32)> {
    let start = span_start as usize;
    let end = (span_end as usize).min(text.len());
    if start >= end {
        return None;
    }

    let slice = &text[start..end];

    // Search for the source string in quotes (single or double)
    for quote in ['"', '\''] {
        let needle = format!("{quote}{source}{quote}");
        if let Some(pos) = slice.find(&needle) {
            let abs_start = start + pos + 1; // skip opening quote
            let abs_end = abs_start + source.len();
            return Some((abs_start as u32, abs_end as u32));
        }
    }

    None
}

/// Convert a canonical ID (filesystem path) to a file URI.
fn canonical_id_to_uri(canonical_id: &str) -> Uri {
    // canonical_id is typically a forward-slash path like "/d/dev/project/file.vue"
    // or "d:/dev/project/file.vue" (Windows)
    let path = if canonical_id.starts_with('/') {
        format!("file://{canonical_id}")
    } else {
        // Windows path like "d:/..."
        format!("file:///{canonical_id}")
    };
    path.parse().unwrap_or_else(|_| {
        format!("file:///{canonical_id}")
            .parse()
            .unwrap_or_else(|_| UNKNOWN_FILE_URI.clone())
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::documents::carrier_structure::test_carrier_blocks;
    use verter_semantic::analysis::types::ImportBindingKind;
    use verter_semantic::analysis::*;

    #[test]
    fn test_import_source_span() {
        let source = "import { ref } from 'vue'";
        let result = find_import_source_span(source, 0, source.len() as u32, "vue");
        assert!(result.is_some());
        let (start, end) = result.unwrap();
        assert_eq!(&source[start as usize..end as usize], "vue");
    }

    #[test]
    fn test_import_source_span_double_quotes() {
        let source = r#"import Foo from "./Foo.vue""#;
        let result = find_import_source_span(source, 0, source.len() as u32, "./Foo.vue");
        assert!(result.is_some());
        let (start, end) = result.unwrap();
        assert_eq!(&source[start as usize..end as usize], "./Foo.vue");
    }

    #[test]
    fn test_document_links_from_analysis() {
        let source = "<script setup>\nimport Foo from './Foo.vue'\n</script>\n<template>\n<Foo />\n</template>";
        let blocks = test_carrier_blocks(source);
        let line_index = LineIndex::new_utf16(source);

        let analysis = FileAnalysisSnapshot {
            imports: vec![AnalyzedImport {
                source: "./Foo.vue".to_string(),
                owner: verter_type_expr::TopLevelOwnerId::instance(0),
                is_type_only: false,
                bindings: vec![AnalyzedImportBinding {
                    name: "Foo".to_string(),
                    kind: ImportBindingKind::Named,
                    imported_name: None,
                    is_type_only: false,
                    vue_api: None,
                    span: verter_span::Span::new(0, 0),
                }],
                span: verter_span::Span::new(15, 42),
                resolved_canonical_id: Some("/project/Foo.vue".to_string()),
            }],
            ..Default::default()
        };

        let links = build_document_links(source, &blocks, Some(&analysis), &line_index);
        assert_eq!(links.len(), 1);
        assert!(links[0].target.is_some());
        assert_eq!(links[0].tooltip.as_deref(), Some("/project/Foo.vue"));
    }

    #[test]
    fn test_no_link_without_resolved_id() {
        let source = "<script setup>\nimport { ref } from 'vue'\n</script>";
        let blocks = test_carrier_blocks(source);
        let line_index = LineIndex::new_utf16(source);

        let analysis = FileAnalysisSnapshot {
            imports: vec![AnalyzedImport {
                source: "vue".to_string(),
                owner: verter_type_expr::TopLevelOwnerId::instance(0),
                is_type_only: false,
                bindings: vec![],
                span: verter_span::Span::new(15, 40),
                resolved_canonical_id: None, // No resolved path
            }],
            ..Default::default()
        };

        let links = build_document_links(source, &blocks, Some(&analysis), &line_index);
        assert!(links.is_empty());
    }

    #[test]
    fn src_link_uses_parsed_attribute_identity_and_exact_value_span() {
        let source = r#"<script data-src="wrong" src = './script.ts' lang="ts"></script>"#;
        let blocks = test_carrier_blocks(source);
        let links = build_document_links(source, &blocks, None, &LineIndex::new_utf16(source));
        assert_eq!(links.len(), 1);
        assert_eq!(links[0].tooltip.as_deref(), Some("./script.ts"));
    }
}
