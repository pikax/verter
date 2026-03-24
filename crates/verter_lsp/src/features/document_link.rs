// Document links: make import paths and src attributes clickable.

use tower_lsp_server::ls_types::*;
use verter_host::FileAnalysisSnapshot;

use crate::documents::line_index::LineIndex;
use crate::documents::sfc_scanner::SfcBlock;
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
    blocks: &[SfcBlock],
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
        if block.attrs_raw.contains("src=") {
            if let Some((val_start, val_end)) =
                find_attribute_value_span(source, block.open_tag_start, block.open_tag_end, "src")
            {
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

/// Find the byte range of an attribute value in an SFC block's opening tag.
///
/// Searches for `attr_name="value"` or `attr_name='value'` and returns the
/// range of the value (inside quotes).
fn find_attribute_value_span(
    text: &str,
    tag_start: u32,
    tag_end: u32,
    attr_name: &str,
) -> Option<(u32, u32)> {
    let start = tag_start as usize;
    let end = (tag_end as usize).min(text.len());
    if start >= end {
        return None;
    }

    let slice = &text[start..end];

    // Find attr_name= pattern
    let pattern = format!("{attr_name}=");
    let attr_pos = slice.find(&pattern)?;
    let after_eq = attr_pos + pattern.len();

    if after_eq >= slice.len() {
        return None;
    }

    let quote = slice.as_bytes()[after_eq];
    if quote != b'"' && quote != b'\'' {
        return None;
    }

    let val_start = after_eq + 1;
    let rest = &slice[val_start..];
    let val_len = rest.find(quote as char)?;

    let abs_start = start + val_start;
    let abs_end = abs_start + val_len;
    Some((abs_start as u32, abs_end as u32))
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
    use crate::documents::sfc_scanner::scan_sfc_blocks;
    use verter_analysis::types::ImportBindingKind;
    use verter_analysis::*;

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
        let blocks = scan_sfc_blocks(source);
        let line_index = LineIndex::new_utf16(source);

        let analysis = FileAnalysisSnapshot {
            imports: vec![AnalyzedImport {
                source: "./Foo.vue".to_string(),
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
        let blocks = scan_sfc_blocks(source);
        let line_index = LineIndex::new_utf16(source);

        let analysis = FileAnalysisSnapshot {
            imports: vec![AnalyzedImport {
                source: "vue".to_string(),
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
    fn test_attribute_value_span() {
        let text = r#"<script src="./script.ts" lang="ts">"#;
        let result = find_attribute_value_span(text, 0, text.len() as u32, "src");
        assert!(result.is_some());
        let (start, end) = result.unwrap();
        assert_eq!(&text[start as usize..end as usize], "./script.ts");
    }
}
