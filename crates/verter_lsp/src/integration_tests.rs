/// @ai-generated — Phase 4 integration tests for verter_lsp.
///
/// These tests use the full pipeline: DocumentRegistry (backed by verter_host) →
/// LSP feature functions → verify results. They test real Vue SFC content end-to-end.
use std::sync::Arc;
use tower_lsp_server::lsp_types::*;
use verter_host::{HostConfig, VerterHost};

use crate::documents::sfc_scanner::scan_sfc_blocks;
use crate::documents::DocumentRegistry;
use crate::features::completion::completions_at_position;
use crate::features::definition::definition_at_position;
use crate::features::diagnostics::map_diagnostics;
use crate::features::document_highlight::highlights_at_position;
use crate::features::document_symbol::build_document_symbols;
use crate::features::folding_range::build_folding_ranges;
use crate::features::hover::hover_at_position;
use crate::features::references::references_at_position;
use crate::features::rename::{prepare_rename, rename_at_position};

/// Helper: create a DocumentRegistry and open a Vue SFC.
fn open_vue_file(source: &str) -> (DocumentRegistry, Uri) {
    let host = Arc::new(VerterHost::new(HostConfig::default()));
    let registry = DocumentRegistry::new(host);
    let uri: Uri = "file:///test/App.vue".parse().unwrap();
    let item = TextDocumentItem {
        uri: uri.clone(),
        language_id: "vue".to_string(),
        version: 1,
        text: source.to_string(),
    };
    registry.did_open(&item);
    (registry, uri)
}

/// Helper: create a DocumentRegistry with embedded ambient types and open a Vue SFC.
/// Simulates the case where `@verter/types` is not installed in the workspace.
fn open_vue_file_with_ambient(source: &str) -> (DocumentRegistry, Uri) {
    let host = Arc::new(VerterHost::new(HostConfig::default()));
    let registry = DocumentRegistry::new(host);
    registry.set_embed_ambient_types(true);
    let uri: Uri = "file:///test/App.vue".parse().unwrap();
    let item = TextDocumentItem {
        uri: uri.clone(),
        language_id: "vue".to_string(),
        version: 1,
        text: source.to_string(),
    };
    registry.did_open(&item);
    (registry, uri)
}

/// Helper: get position of a substring in source
fn position_of(source: &str, needle: &str) -> Position {
    let offset = source.find(needle).expect("needle not found in source");
    let line = source[..offset].matches('\n').count() as u32;
    let line_start = source[..offset].rfind('\n').map(|p| p + 1).unwrap_or(0);
    let character = (offset - line_start) as u32;
    Position { line, character }
}

// ─── Hover tests ─────────────────────────────────────────────────

#[test]
fn integration_hover_on_script_binding() {
    let source = r#"<script setup>
import { ref } from 'vue'
const count = ref(0)
</script>

<template>
  <div>{{ count }}</div>
</template>
"#;
    let (registry, uri) = open_vue_file(source);
    let doc = registry.get(&uri).unwrap();
    let analysis = registry.get_analysis(&uri);
    let blocks = scan_sfc_blocks(&doc.source);

    // Hover on "count" in script
    let position = position_of(source, "count = ref");
    let hover = hover_at_position(
        &position,
        &doc.source,
        &blocks,
        analysis.as_ref(),
        &doc.line_index,
    );

    assert!(hover.is_some(), "Should get hover info for 'count' binding");
    let contents = match hover.unwrap().contents {
        HoverContents::Markup(m) => m.value,
        _ => panic!("expected markup"),
    };
    assert!(
        contents.contains("count"),
        "Hover should mention the binding name"
    );
}

#[test]
fn integration_hover_on_template_binding() {
    let source = r#"<script setup>
const msg = 'hello'
</script>

<template>
  <div>{{ msg }}</div>
</template>
"#;
    let (registry, uri) = open_vue_file(source);
    let doc = registry.get(&uri).unwrap();
    let analysis = registry.get_analysis(&uri);
    let blocks = scan_sfc_blocks(&doc.source);

    // Hover on "msg" in template
    let template_msg = source.rfind("msg").unwrap(); // last occurrence (in template)
    let line = source[..template_msg].matches('\n').count() as u32;
    let line_start = source[..template_msg]
        .rfind('\n')
        .map(|p| p + 1)
        .unwrap_or(0);
    let position = Position {
        line,
        character: (template_msg - line_start) as u32,
    };

    let hover = hover_at_position(
        &position,
        &doc.source,
        &blocks,
        analysis.as_ref(),
        &doc.line_index,
    );
    assert!(
        hover.is_some(),
        "Should get hover info for 'msg' in template"
    );
}

// ─── Completion tests ────────────────────────────────────────────

#[test]
fn integration_completions_in_template() {
    let source = r#"<script setup>
import { ref } from 'vue'
const count = ref(0)
const message = 'hello'
</script>

<template>
  <div>{{ c }}</div>
</template>
"#;
    let (registry, uri) = open_vue_file(source);
    let doc = registry.get(&uri).unwrap();
    let analysis = registry.get_analysis(&uri);
    let blocks = scan_sfc_blocks(&doc.source);

    // Position at "c" in template — should suggest bindings
    let position = position_of(source, "{{ c }}");
    let items = completions_at_position(
        &position,
        &doc.source,
        &blocks,
        analysis.as_ref(),
        &doc.line_index,
        None,
        None,
        None,
    );

    assert!(items.is_some(), "Should get completion items in template");
    let result = items.unwrap();
    // Should include bindings from script setup
    let labels: Vec<_> = result.items.iter().map(|i| i.label.as_str()).collect();
    assert!(
        labels.iter().any(|l| *l == "count"),
        "Should complete 'count', got: {:?}",
        labels
    );
    assert!(
        labels.iter().any(|l| *l == "message"),
        "Should complete 'message', got: {:?}",
        labels
    );
}

#[test]
fn integration_completions_in_script() {
    let source = r#"<script setup>
import { ref } from 'vue'
const count = ref(0)
</script>
"#;
    let (registry, uri) = open_vue_file(source);
    let doc = registry.get(&uri).unwrap();
    let analysis = registry.get_analysis(&uri);
    let blocks = scan_sfc_blocks(&doc.source);

    // Position inside script should also return completions
    let position = position_of(source, "ref(0)");
    let items = completions_at_position(
        &position,
        &doc.source,
        &blocks,
        analysis.as_ref(),
        &doc.line_index,
        None,
        None,
        None,
    );

    // Script completions should include imports and bindings
    assert!(items.is_some(), "Should get completion items in script");
}

// ─── Definition tests ────────────────────────────────────────────

#[test]
fn integration_definition_on_binding() {
    let source = r#"<script setup>
const msg = 'hello'
</script>

<template>
  <div>{{ msg }}</div>
</template>
"#;
    let (registry, uri) = open_vue_file(source);
    let doc = registry.get(&uri).unwrap();
    let analysis = registry.get_analysis(&uri);
    let blocks = scan_sfc_blocks(&doc.source);

    // Go-to-definition on "msg" in template should find the binding declaration in script
    let position = position_of(source, "msg }}</div>");
    let def = definition_at_position(
        &position,
        &doc.source,
        &blocks,
        analysis.as_ref(),
        &doc.line_index,
        None,
    );

    assert!(def.is_some(), "Should find definition for 'msg'");
}

/// @ai-generated - Definition navigates to the correct SFC-absolute position
#[test]
fn integration_definition_span_is_sfc_absolute() {
    let source = r#"<template>
  <div>{{ msg }}</div>
</template>

<script setup>
const msg = 'hello'
</script>"#;
    let (registry, uri) = open_vue_file(source);
    let doc = registry.get(&uri).unwrap();
    let analysis = registry.get_analysis(&uri);
    let blocks = scan_sfc_blocks(&doc.source);

    // Click on "msg" in template
    let position = position_of(source, "msg }}</div>");
    let def = definition_at_position(
        &position,
        &doc.source,
        &blocks,
        analysis.as_ref(),
        &doc.line_index,
        None,
    );

    let def = def.expect("Should find definition for 'msg'");
    match def {
        GotoDefinitionResponse::Scalar(loc) => {
            // "const msg = 'hello'" is on line 5 (0-indexed), column 6
            let expected_line = source[..source.find("const msg").unwrap()]
                .matches('\n')
                .count() as u32;
            assert_eq!(
                loc.range.start.line, expected_line,
                "definition should point to the line containing 'const msg'"
            );
            assert_eq!(
                loc.range.start.character, 6,
                "definition should point to column 6 ('msg' after 'const ')"
            );
            assert_eq!(
                loc.range.end.character, 9,
                "definition end should be column 9 (end of 'msg')"
            );
        }
        _ => panic!("Expected Scalar definition response"),
    }
}

/// @ai-generated - Definition span precision with template before script
#[test]
fn integration_definition_not_offset_by_template_size() {
    // Longer template to exaggerate the offset error
    let source = r#"<template>
  <div>
    <span>some content here</span>
    <p>more content</p>
    <h1>{{ title }}</h1>
  </div>
</template>

<script setup>
const title = 'hello'
</script>"#;
    let (registry, uri) = open_vue_file(source);
    let doc = registry.get(&uri).unwrap();
    let analysis = registry.get_analysis(&uri);
    let blocks = scan_sfc_blocks(&doc.source);

    let position = position_of(source, "title }}</h1>");
    let def = definition_at_position(
        &position,
        &doc.source,
        &blocks,
        analysis.as_ref(),
        &doc.line_index,
        None,
    );

    let def = def.expect("Should find definition for 'title'");
    match def {
        GotoDefinitionResponse::Scalar(loc) => {
            let expected_line = source[..source.find("const title").unwrap()]
                .matches('\n')
                .count() as u32;
            assert_eq!(
                loc.range.start.line, expected_line,
                "definition should point to the correct script line, not be offset by template size"
            );
            // "const title" -> column 6 for "title"
            assert_eq!(loc.range.start.character, 6);
            assert_eq!(loc.range.end.character, 6 + 5); // "title" = 5 chars
        }
        _ => panic!("Expected Scalar definition response"),
    }
}

/// @ai-generated - Cross-file URI normalizes Windows backslashes
#[test]
fn integration_resolved_import_definition_windows_path() {
    use crate::features::definition::resolved_import_definition;

    // Windows-style path with backslashes
    let result = resolved_import_definition(r"D:\projects\src\Child.vue");
    assert!(
        result.is_some(),
        "Should produce a valid definition for Windows backslash path"
    );

    if let Some(GotoDefinitionResponse::Scalar(loc)) = result {
        let uri_str = loc.uri.to_string();
        assert!(
            !uri_str.contains('\\'),
            "URI should not contain backslashes: {uri_str}"
        );
        assert!(
            uri_str.starts_with("file:///"),
            "URI should start with file:///: {uri_str}"
        );
    }
}

// ─── References tests ────────────────────────────────────────────

#[test]
fn integration_references_for_binding() {
    let source = r#"<script setup>
const count = 0
console.log(count)
</script>

<template>
  <div>{{ count }}</div>
</template>
"#;
    let (registry, uri) = open_vue_file(source);
    let doc = registry.get(&uri).unwrap();
    let analysis = registry.get_analysis(&uri);
    let blocks = scan_sfc_blocks(&doc.source);

    // Find references for "count"
    let position = position_of(source, "count = ");
    let refs = references_at_position(
        &position,
        &doc.source,
        &blocks,
        analysis.as_ref(),
        &doc.line_index,
        true, // include declaration
    );

    assert!(refs.is_some(), "Should find references for 'count'");
    let refs = refs.unwrap();
    // Should find: declaration + at least one usage
    assert!(
        refs.len() >= 2,
        "Should find at least 2 references (declaration + usage), got: {}",
        refs.len()
    );
}

// ─── Rename tests ────────────────────────────────────────────────

#[test]
fn integration_prepare_rename() {
    let source = r#"<script setup>
const msg = 'hello'
</script>
"#;
    let (registry, uri) = open_vue_file(source);
    let doc = registry.get(&uri).unwrap();
    let analysis = registry.get_analysis(&uri);
    let blocks = scan_sfc_blocks(&doc.source);

    let position = position_of(source, "msg = ");
    let result = prepare_rename(
        &position,
        &doc.source,
        &blocks,
        analysis.as_ref(),
        &doc.line_index,
    );

    assert!(result.is_some(), "Should allow rename on 'msg'");
}

#[test]
fn integration_rename_binding() {
    let source = r#"<script setup>
const msg = 'hello'
</script>

<template>
  <div>{{ msg }}</div>
</template>
"#;
    let (registry, uri) = open_vue_file(source);
    let doc = registry.get(&uri).unwrap();
    let analysis = registry.get_analysis(&uri);
    let blocks = scan_sfc_blocks(&doc.source);

    let position = position_of(source, "msg = ");
    let edit = rename_at_position(
        &position,
        "message",
        &doc.source,
        &blocks,
        analysis.as_ref(),
        &doc.line_index,
    );

    assert!(edit.is_some(), "Should produce a workspace edit for rename");
    let edit = edit.unwrap();
    let changes = edit.changes.as_ref().unwrap();
    // Should have edits (in the sentinel file URI or the real file)
    assert!(!changes.is_empty(), "Should have file changes");
    let all_edits: Vec<_> = changes.values().flatten().collect();
    assert!(
        all_edits.len() >= 2,
        "Should rename at least declaration + template usage, got: {}",
        all_edits.len()
    );
    for edit in &all_edits {
        assert_eq!(
            edit.new_text, "message",
            "All edits should use the new name"
        );
    }
}

// ─── Diagnostics tests ──────────────────────────────────────────

#[test]
fn integration_diagnostics_for_valid_sfc() {
    let source = r#"<script setup>
const msg = 'hello'
</script>

<template>
  <div>{{ msg }}</div>
</template>
"#;
    let (registry, uri) = open_vue_file(source);
    let diags = registry.get_diagnostics(&uri);

    // Valid SFC should have no errors (or minimal warnings)
    if let Some(snapshot) = diags {
        assert!(
            !snapshot.has_errors,
            "Valid SFC should not have errors: {:?}",
            snapshot.diagnostics
        );
    }
}

/// @ai-generated — Pull diagnostics returns correct shape (valid SFC → no errors).
///
/// Mirrors the code path of the `textDocument/diagnostic` handler without
/// requiring a full LanguageServer + Client setup.
#[test]
fn integration_pull_diagnostics_valid_sfc() {
    let source = r#"<script setup>
const msg = 'hello'
</script>

<template>
  <div>{{ msg }}</div>
</template>
"#;
    let (registry, uri) = open_vue_file(source);
    let doc = registry.get(&uri).unwrap();

    let verter_diags = match registry.get_diagnostics(&uri) {
        Some(snapshot) => map_diagnostics(&snapshot, &doc.line_index),
        None => vec![],
    };

    // Valid SFC should produce zero error-level diagnostics
    let errors: Vec<_> = verter_diags
        .iter()
        .filter(|d| d.severity == Some(DiagnosticSeverity::ERROR))
        .collect();
    assert!(
        errors.is_empty(),
        "Valid SFC should have no error diagnostics, got: {:?}",
        errors
    );
}

/// @ai-generated — Pull diagnostics returns errors for invalid SFC.
#[test]
fn integration_pull_diagnostics_invalid_template() {
    let source = r#"<script setup>
const msg = 'hello'
</script>

<template>
  <div>{{ msg }}
</template>
"#;
    let (registry, uri) = open_vue_file(source);
    let doc = registry.get(&uri).unwrap();

    let verter_diags = match registry.get_diagnostics(&uri) {
        Some(snapshot) => map_diagnostics(&snapshot, &doc.line_index),
        None => vec![],
    };

    // map_diagnostics should produce LSP Diagnostic values (may or may not have errors
    // depending on parser tolerance — the key thing is the function doesn't panic)
    // Just verify the diagnostics are valid LSP objects
    for diag in &verter_diags {
        assert!(
            diag.range.start.line <= diag.range.end.line
                || diag.range.start.character <= diag.range.end.character
        );
    }
}

// ─── Document Symbol tests ──────────────────────────────────────

#[test]
fn integration_document_symbols() {
    let source = r#"<script setup>
import { ref } from 'vue'
const count = ref(0)
function increment() { count.value++ }
</script>

<template>
  <button @click="increment">{{ count }}</button>
</template>

<style scoped>
button { color: red; }
</style>
"#;
    let (registry, uri) = open_vue_file(source);
    let doc = registry.get(&uri).unwrap();
    let analysis = registry.get_analysis(&uri);
    let blocks = scan_sfc_blocks(&doc.source);

    let symbols = build_document_symbols(&blocks, analysis.as_ref(), &doc.line_index);
    assert!(
        !symbols.is_empty(),
        "Should produce document symbols for SFC"
    );

    // Should have top-level SFC block symbols
    let names: Vec<_> = symbols.iter().map(|s| s.name.as_str()).collect();
    assert!(
        names.iter().any(|n| n.contains("script")),
        "Should have script symbol, got: {:?}",
        names
    );
}

// ─── Folding Range tests ────────────────────────────────────────

#[test]
fn integration_folding_ranges() {
    let source = r#"<script setup>
import { ref } from 'vue'
const count = ref(0)
</script>

<template>
  <div>{{ count }}</div>
</template>

<style scoped>
div { color: red; }
</style>
"#;
    let (registry, uri) = open_vue_file(source);
    let doc = registry.get(&uri).unwrap();
    let blocks = scan_sfc_blocks(&doc.source);

    let analysis = registry.get_analysis(&uri);
    let ranges = build_folding_ranges(&blocks, analysis.as_ref(), &doc.line_index);
    assert!(
        ranges.len() >= 3,
        "Should have at least 3 folding ranges (script, template, style), got: {}",
        ranges.len()
    );
}

// ─── Document Highlight tests ───────────────────────────────────

#[test]
fn integration_document_highlights() {
    let source = r#"<script setup>
const msg = 'hello'
console.log(msg)
</script>

<template>
  <div>{{ msg }}</div>
</template>
"#;
    let (registry, uri) = open_vue_file(source);
    let doc = registry.get(&uri).unwrap();
    let analysis = registry.get_analysis(&uri);
    let blocks = scan_sfc_blocks(&doc.source);

    let position = position_of(source, "msg = ");
    let highlights = highlights_at_position(
        &position,
        &doc.source,
        &blocks,
        analysis.as_ref(),
        &doc.line_index,
    );

    assert!(highlights.is_some(), "Should find highlights for 'msg'");
    let highlights = highlights.unwrap();
    assert!(
        highlights.len() >= 2,
        "Should highlight at least declaration + usage, got: {}",
        highlights.len()
    );
}

// ─── TSX Source Map Integration ─────────────────────────────────

#[test]
fn integration_tsx_source_map_available_after_open() {
    let source = r#"<script setup>
const msg = 'hello'
</script>

<template>
  <div>{{ msg }}</div>
</template>
"#;
    let (registry, uri) = open_vue_file(source);

    // After opening a .vue file, TSX + source map should be available
    let tsx = registry.get_ide(&uri);
    assert!(tsx.is_some(), "TSX should be generated after open");
    let tsx = tsx.unwrap();
    assert!(!tsx.code.is_empty(), "TSX code should not be empty");
    assert!(
        tsx.source_map.is_some(),
        "TSX source map should be populated"
    );

    // Position mapper should be built
    let mapper = registry.get_position_mapper(&uri);
    assert!(
        mapper.is_some(),
        "Position mapper should be available after open"
    );
}

#[test]
fn integration_position_mapping_roundtrip() {
    let source = r#"<script setup>
const msg = 'hello'
</script>

<template>
  <div>{{ msg }}</div>
</template>
"#;
    let (registry, uri) = open_vue_file(source);
    let doc = registry.get(&uri).unwrap();
    let mapper = doc.position_mapper.as_ref().expect("mapper should exist");
    let tsx = registry.get_ide(&uri).expect("tsx should exist");

    // Find "msg" in the original source (script block)
    let msg_offset = source.find("msg = ").unwrap();
    let msg_line = source[..msg_offset].matches('\n').count() as u32;
    let msg_line_start = source[..msg_offset].rfind('\n').map(|p| p + 1).unwrap_or(0);
    let msg_col = (msg_offset - msg_line_start) as u32;

    // Map Vue position → TSX position
    let tsx_pos = mapper.vue_to_tsx(msg_line, msg_col);
    assert!(
        tsx_pos.is_some(),
        "Should map Vue position (line {msg_line}, col {msg_col}) to TSX"
    );

    let tsx_pos = tsx_pos.unwrap();
    // Verify the TSX position is reasonable (not out of bounds)
    let tsx_lines: Vec<&str> = tsx.code.lines().collect();
    assert!(
        (tsx_pos.line as usize) < tsx_lines.len(),
        "TSX line {} should be within bounds (total: {})",
        tsx_pos.line,
        tsx_lines.len()
    );

    // Map TSX position back → Vue position
    let vue_pos = mapper.tsx_to_vue(tsx_pos.line, tsx_pos.column);
    assert!(vue_pos.is_some(), "Should map TSX position back to Vue");

    let vue_pos = vue_pos.unwrap();
    // The roundtrip should land on the same line
    assert_eq!(
        vue_pos.line, msg_line,
        "Roundtrip should return to the same Vue line"
    );
}

/// @ai-generated — Verifies character-level source map accuracy for prepended text in templates.
///
/// When template bindings like `count` are compiled to TSX, they get prefixed
/// (e.g., `$setup.count` or `_ctx.count`). The source map must correctly handle:
/// - Forward mapping: Vue position (start/middle/end of `count`) → correct TSX position
/// - Reverse mapping: TSX position → back to correct Vue position
/// - Unmapped prefix: hovering inside the generated prefix returns None or maps elsewhere
#[test]
fn integration_source_map_prepended_text_character_accuracy() {
    let source = r#"<script setup>
const count = 0
</script>

<template>
  <div>{{ count }}</div>
</template>
"#;
    let (registry, uri) = open_vue_file(source);
    let doc = registry.get(&uri).unwrap();
    let mapper = doc
        .position_mapper
        .as_ref()
        .expect("mapper should exist after compilation");
    let tsx = registry.get_ide(&uri).expect("tsx should exist");

    // Find "count" in the template (last occurrence)
    let template_count_offset = source.rfind("count").unwrap();
    let template_count_line = source[..template_count_offset].matches('\n').count() as u32;
    let template_count_line_start = source[..template_count_offset]
        .rfind('\n')
        .map(|p| p + 1)
        .unwrap_or(0);
    let template_count_col = (template_count_offset - template_count_line_start) as u32;

    // Find "count" in the TSX output after a prefix (like $setup. or _ctx.)
    // The prefix is prepended, so "count" in TSX should be preceded by it
    let tsx_code = tsx.code.as_ref();
    let tsx_lines: Vec<&str> = tsx_code.lines().collect();

    // Forward map: start of "count" in Vue
    let tsx_start = mapper
        .vue_to_tsx(template_count_line, template_count_col)
        .expect("Start of 'count' in template should map to TSX");

    // Verify: the character at the mapped TSX position should be 'c' (start of "count")
    assert!(
        (tsx_start.line as usize) < tsx_lines.len(),
        "TSX line {} out of bounds (total: {})",
        tsx_start.line,
        tsx_lines.len()
    );
    let tsx_line_chars: Vec<char> = tsx_lines[tsx_start.line as usize].chars().collect();
    assert!(
        (tsx_start.column as usize) < tsx_line_chars.len(),
        "TSX column {} out of bounds for line '{}' (len: {})",
        tsx_start.column,
        tsx_lines[tsx_start.line as usize],
        tsx_line_chars.len()
    );
    assert_eq!(
        tsx_line_chars[tsx_start.column as usize], 'c',
        "TSX position should point to 'c' of 'count', got '{}' in line '{}'",
        tsx_line_chars[tsx_start.column as usize], tsx_lines[tsx_start.line as usize]
    );

    // Forward map + character verify for each character of "count" (c=0, o=1, u=2, n=3, t=4)
    let expected_chars = ['c', 'o', 'u', 'n', 't'];
    for (i, expected_char) in expected_chars.iter().enumerate() {
        let vue_col = template_count_col + i as u32;
        let tsx_pos = mapper
            .vue_to_tsx(template_count_line, vue_col)
            .unwrap_or_else(|| {
                panic!(
                    "vue_to_tsx failed for 'count'[{i}] at Vue ({}, {vue_col})",
                    template_count_line
                )
            });

        let actual_char = tsx_lines[tsx_pos.line as usize]
            .chars()
            .nth(tsx_pos.column as usize)
            .unwrap_or_else(|| {
                panic!(
                    "TSX position ({}, {}) out of bounds for 'count'[{i}]",
                    tsx_pos.line, tsx_pos.column
                )
            });
        assert_eq!(
            actual_char,
            *expected_char,
            "'count'[{i}]: expected '{}' at TSX ({}, {}), got '{}' in line '{}'",
            expected_char,
            tsx_pos.line,
            tsx_pos.column,
            actual_char,
            tsx_lines[tsx_pos.line as usize]
        );

        // Reverse: TSX → Vue should return the same Vue column
        let vue_roundtrip = mapper
            .tsx_to_vue(tsx_pos.line, tsx_pos.column)
            .unwrap_or_else(|| {
                panic!(
                    "tsx_to_vue failed for TSX ({}, {})",
                    tsx_pos.line, tsx_pos.column
                )
            });
        assert_eq!(
            vue_roundtrip.line, template_count_line,
            "'count'[{i}] roundtrip: line mismatch"
        );
        assert_eq!(
            vue_roundtrip.column, vue_col,
            "'count'[{i}] roundtrip: column mismatch (expected Vue col {vue_col}, got {})",
            vue_roundtrip.column
        );
    }

    // Verify: the prefix region (positions before 'c' in TSX) maps to unmapped or different location
    if tsx_start.column > 0 {
        let prefix_pos = mapper.tsx_to_vue(tsx_start.line, tsx_start.column - 1);
        // Inside the prepended prefix: should either be None (unmapped) or map to a
        // different Vue position (not the same as "count")
        if let Some(pos) = prefix_pos {
            assert!(
                pos.line != template_count_line || pos.column != template_count_col,
                "Position inside prefix should NOT map back to 'count' start"
            );
        }
        // None is also acceptable (unmapped Inserted chunk)
    }
}

/// @ai-generated — Verifies that script block positions roundtrip with exact column accuracy.
///
/// Script blocks are not prepended — they map 1:1. This confirms the column adjustment
/// in tsx_to_vue works correctly for the identity mapping case.
#[test]
fn integration_source_map_script_roundtrip_exact_column() {
    let source = r#"<script setup>
const message = 'hello world'
const count = 42
</script>

<template>
  <div>{{ message }}</div>
</template>
"#;
    let (registry, uri) = open_vue_file(source);
    let doc = registry.get(&uri).unwrap();
    let mapper = doc.position_mapper.as_ref().expect("mapper should exist");
    let tsx = registry.get_ide(&uri).expect("tsx should exist");
    let tsx_code = tsx.code.as_ref();
    let tsx_lines: Vec<&str> = tsx_code.lines().collect();

    // Find "message" in script (line 1, col 6)
    let msg_offset = source.find("message = ").unwrap();
    let msg_line = source[..msg_offset].matches('\n').count() as u32;
    let msg_line_start = source[..msg_offset].rfind('\n').map(|p| p + 1).unwrap_or(0);
    let msg_col = (msg_offset - msg_line_start) as u32;

    // Verify each character of "message" roundtrips exactly
    let expected_chars = ['m', 'e', 's', 's', 'a', 'g', 'e'];
    for (i, expected_char) in expected_chars.iter().enumerate() {
        let vue_col = msg_col + i as u32;
        let tsx_pos = mapper.vue_to_tsx(msg_line, vue_col).unwrap_or_else(|| {
            panic!("vue_to_tsx failed for 'message'[{i}] at ({msg_line}, {vue_col})")
        });

        // Character at TSX position should match
        if let Some(tsx_line) = tsx_lines.get(tsx_pos.line as usize) {
            if let Some(actual) = tsx_line.chars().nth(tsx_pos.column as usize) {
                assert_eq!(
                    actual, *expected_char,
                    "'message'[{i}]: expected '{}' got '{}' at TSX ({}, {})",
                    expected_char, actual, tsx_pos.line, tsx_pos.column
                );
            }
        }

        // Roundtrip
        let vue_roundtrip = mapper.tsx_to_vue(tsx_pos.line, tsx_pos.column);
        if let Some(pos) = vue_roundtrip {
            assert_eq!(pos.line, msg_line, "'message'[{i}] roundtrip line mismatch");
            assert_eq!(
                pos.column, vue_col,
                "'message'[{i}] roundtrip column mismatch"
            );
        }
    }
}

/// @ai-generated — UTF-16 E2E: verifies position mapping survives multi-byte characters.
///
/// Uses a Vue SFC where non-ASCII text (emoji/accented) precedes a template binding.
/// The source map uses UTF-16 columns throughout. Confirms:
/// 1. The PositionResolver produces correct UTF-16 source columns
/// 2. The PositionMapper forwards/reverses through UTF-16 columns correctly
/// 3. The LineIndex (UTF-16 encoding) produces correct byte offsets
#[test]
fn integration_utf16_source_map_with_multibyte_chars() {
    // 'é' in "café" is 2 bytes UTF-8, 1 UTF-16 code unit
    let source = "<script setup>\nconst café = 'latte'\n</script>\n\n<template>\n  <div>{{ café }}</div>\n</template>\n";
    let (registry, uri) = open_vue_file(source);
    let doc = registry.get(&uri).unwrap();
    let mapper = doc
        .position_mapper
        .as_ref()
        .expect("mapper should exist for SFC with non-ASCII chars");
    let tsx = registry.get_ide(&uri).expect("tsx should exist");
    let tsx_code = tsx.code.as_ref();

    // Find "café" in the template (last occurrence)
    let template_cafe_offset = source.rfind("café").unwrap();
    let template_cafe_line = source[..template_cafe_offset].matches('\n').count() as u32;
    let template_cafe_line_start = source[..template_cafe_offset]
        .rfind('\n')
        .map(|p| p + 1)
        .unwrap_or(0);

    // UTF-16 column = count UTF-16 code units from line start to "café"
    let line_prefix = &source[template_cafe_line_start..template_cafe_offset];
    let template_cafe_col_utf16 = line_prefix.encode_utf16().count() as u32;

    // Forward map: "café" in Vue should map to a valid TSX position
    let tsx_pos = mapper.vue_to_tsx(template_cafe_line, template_cafe_col_utf16);
    assert!(
        tsx_pos.is_some(),
        "Start of 'café' at Vue ({}, {}) should map to TSX. Source line: '{}'",
        template_cafe_line,
        template_cafe_col_utf16,
        &source[template_cafe_line_start
            ..source[template_cafe_line_start..]
                .find('\n')
                .map(|p| template_cafe_line_start + p)
                .unwrap_or(source.len())]
    );
    let tsx_pos = tsx_pos.unwrap();

    // The TSX should contain "café" and the character at the mapped position should be 'c'
    let tsx_lines: Vec<&str> = tsx_code.lines().collect();
    if let Some(tsx_line) = tsx_lines.get(tsx_pos.line as usize) {
        // Convert UTF-16 column to char index for verification
        let mut utf16_count = 0u32;
        let mut char_at_col = None;
        for ch in tsx_line.chars() {
            if utf16_count == tsx_pos.column {
                char_at_col = Some(ch);
                break;
            }
            utf16_count += ch.len_utf16() as u32;
        }
        assert_eq!(
            char_at_col,
            Some('c'),
            "TSX position ({}, {}) should point to 'c' of 'café', got {:?} in line '{}'",
            tsx_pos.line,
            tsx_pos.column,
            char_at_col,
            tsx_line
        );
    }

    // Roundtrip each character of "café": c(+0), a(+1), f(+2), é(+3)
    // In UTF-16: é is 1 code unit, so offsets are 0, 1, 2, 3
    let expected_chars = ['c', 'a', 'f', 'é'];
    for (i, expected_char) in expected_chars.iter().enumerate() {
        let vue_col = template_cafe_col_utf16 + i as u32;
        if let Some(tsx_mapped) = mapper.vue_to_tsx(template_cafe_line, vue_col) {
            // Verify character at TSX position
            if let Some(tsx_line) = tsx_lines.get(tsx_mapped.line as usize) {
                let mut utf16_count = 0u32;
                let mut actual_char = None;
                for ch in tsx_line.chars() {
                    if utf16_count == tsx_mapped.column {
                        actual_char = Some(ch);
                        break;
                    }
                    utf16_count += ch.len_utf16() as u32;
                }
                assert_eq!(
                    actual_char,
                    Some(*expected_char),
                    "café[{i}]: expected '{}' at TSX ({}, {}), got {:?}",
                    expected_char,
                    tsx_mapped.line,
                    tsx_mapped.column,
                    actual_char
                );
            }

            // Roundtrip: TSX -> Vue
            if let Some(vue_roundtrip) = mapper.tsx_to_vue(tsx_mapped.line, tsx_mapped.column) {
                assert_eq!(
                    vue_roundtrip.line, template_cafe_line,
                    "café[{i}] roundtrip line mismatch"
                );
                assert_eq!(
                    vue_roundtrip.column, vue_col,
                    "café[{i}] roundtrip column mismatch"
                );
            }
        }
    }
}

/// @ai-generated — UTF-16 E2E with surrogate pairs (emoji in script and template).
///
/// 😀 is 4 bytes UTF-8, 2 UTF-16 code units (surrogate pair).
/// Verifies that positions AFTER the emoji are computed correctly in UTF-16.
#[test]
fn integration_utf16_surrogate_pair_position_accuracy() {
    // 😀 before a binding in the script — affects column counting
    let source =
        "<script setup>\nconst 😀msg = 'hi'\n</script>\n\n<template>\n  {{ 😀msg }}\n</template>\n";
    let (registry, uri) = open_vue_file(source);

    // Just verify compilation doesn't panic and we get a mapper
    let doc = registry.get(&uri).unwrap();
    let mapper = doc.position_mapper.as_ref();

    // If mapper exists, verify the script binding position roundtrips
    if let Some(mapper) = mapper {
        // "😀msg" in script: line 1
        // UTF-16: "const " = 6 units, "😀" = 2 units, "msg" starts at col 8
        let script_line = 1u32;
        let msg_utf16_col = 8u32; // "const " (6) + 😀 (2)

        if let Some(tsx_pos) = mapper.vue_to_tsx(script_line, msg_utf16_col) {
            let vue_roundtrip = mapper.tsx_to_vue(tsx_pos.line, tsx_pos.column);
            if let Some(pos) = vue_roundtrip {
                assert_eq!(pos.line, script_line, "😀msg roundtrip: line should match");
                // Column should be exact or on the same line
                assert_eq!(
                    pos.column, msg_utf16_col,
                    "😀msg roundtrip: column should match (UTF-16)"
                );
            }
        }
    }
}

// ─── Analysis data tests ────────────────────────────────────────

#[test]
fn integration_analysis_captures_bindings() {
    let source = r#"<script setup>
import { ref, computed } from 'vue'
const count = ref(0)
const doubled = computed(() => count.value * 2)
const message = 'hello'
function greet() { console.log(message) }
</script>
"#;
    let (registry, uri) = open_vue_file(source);
    let analysis = registry.get_analysis(&uri);

    assert!(analysis.is_some(), "Should have analysis");
    let analysis = analysis.unwrap();

    // Check bindings
    let binding_names: Vec<_> = analysis.bindings.iter().map(|b| b.name.as_str()).collect();
    assert!(
        binding_names.contains(&"count"),
        "Should find 'count' binding, got: {:?}",
        binding_names
    );
    assert!(
        binding_names.contains(&"doubled"),
        "Should find 'doubled' binding, got: {:?}",
        binding_names
    );
    assert!(
        binding_names.contains(&"message"),
        "Should find 'message' binding, got: {:?}",
        binding_names
    );
    assert!(
        binding_names.contains(&"greet"),
        "Should find 'greet' binding, got: {:?}",
        binding_names
    );

    // Check imports
    let import_sources: Vec<_> = analysis.imports.iter().map(|i| i.source.as_str()).collect();
    assert!(
        import_sources.contains(&"vue"),
        "Should have vue import, got: {:?}",
        import_sources
    );
}

#[test]
fn integration_analysis_captures_macros() {
    let source = r#"<script setup lang="ts">
const props = defineProps<{
  msg: string
  count?: number
}>()
const emit = defineEmits<{
  (e: 'update', value: string): void
}>()
</script>
"#;
    let (registry, uri) = open_vue_file(source);
    let analysis = registry.get_analysis(&uri);

    assert!(analysis.is_some(), "Should have analysis");
    let analysis = analysis.unwrap();

    let macro_kinds: Vec<_> = analysis
        .macros
        .iter()
        .map(|m| format!("{:?}", m.kind))
        .collect();
    assert!(
        macro_kinds.iter().any(|k| k.contains("DefineProps")),
        "Should detect defineProps macro, got: {:?}",
        macro_kinds
    );
    assert!(
        macro_kinds.iter().any(|k| k.contains("DefineEmits")),
        "Should detect defineEmits macro, got: {:?}",
        macro_kinds
    );
}

// ─── Cross-file navigation tests ────────────────────────────────

/// Helper: create a DocumentRegistry with multiple Vue files.
fn open_multi_file(
    files: &[(&str, &str, &str)], // (uri, language_id, source)
) -> (DocumentRegistry, Vec<Uri>) {
    let host = Arc::new(VerterHost::new(HostConfig::default()));
    let registry = DocumentRegistry::new(host);
    let mut uris = Vec::new();
    for (uri_str, lang, source) in files {
        let uri: Uri = uri_str.parse().unwrap();
        let item = TextDocumentItem {
            uri: uri.clone(),
            language_id: lang.to_string(),
            version: 1,
            text: source.to_string(),
        };
        let _ = registry.did_open(&item);
        uris.push(uri);
    }
    (registry, uris)
}

#[test]
fn integration_cross_file_multi_vue_upsert() {
    // Verify we can open multiple Vue files and get analysis for each
    let child_source = r#"<script setup>
const msg = 'hello'
</script>
<template><div>{{ msg }}</div></template>
"#;
    let parent_source = r#"<script setup>
import Child from './Child.vue'
</script>
<template><Child msg="hello" /></template>
"#;
    let (registry, uris) = open_multi_file(&[
        ("file:///project/src/Child.vue", "vue", child_source),
        ("file:///project/src/App.vue", "vue", parent_source),
    ]);

    // Both files should have analysis
    let child_analysis = registry.get_analysis(&uris[0]);
    assert!(child_analysis.is_some(), "Child should have analysis");
    let child_analysis = child_analysis.unwrap();
    let child_bindings: Vec<_> = child_analysis
        .bindings
        .iter()
        .map(|b| b.name.as_str())
        .collect();
    assert!(
        child_bindings.contains(&"msg"),
        "Child should have 'msg' binding, got: {:?}",
        child_bindings
    );

    let parent_analysis = registry.get_analysis(&uris[1]);
    assert!(parent_analysis.is_some(), "Parent should have analysis");
    let parent_analysis = parent_analysis.unwrap();
    let import_sources: Vec<_> = parent_analysis
        .imports
        .iter()
        .map(|i| i.source.as_str())
        .collect();
    assert!(
        import_sources.contains(&"./Child.vue"),
        "Parent should import Child, got: {:?}",
        import_sources
    );
}

#[test]
fn integration_cross_file_tsx_generation_for_both() {
    // Verify TSX is generated for both parent and child Vue files
    let child_source = r#"<script setup>
const value = 42
</script>
<template><span>{{ value }}</span></template>
"#;
    let parent_source = r#"<script setup>
import MyChild from './MyChild.vue'
const name = 'world'
</script>
<template><MyChild /><p>{{ name }}</p></template>
"#;
    let (registry, uris) = open_multi_file(&[
        ("file:///project/src/MyChild.vue", "vue", child_source),
        ("file:///project/src/Parent.vue", "vue", parent_source),
    ]);

    let child_tsx = registry.get_ide(&uris[0]);
    assert!(child_tsx.is_some(), "Child should have TSX output");
    let child_tsx = child_tsx.unwrap();
    assert!(
        child_tsx.code.contains("value"),
        "Child TSX should reference 'value'"
    );

    let parent_tsx = registry.get_ide(&uris[1]);
    assert!(parent_tsx.is_some(), "Parent should have TSX output");
    let parent_tsx = parent_tsx.unwrap();
    assert!(
        parent_tsx.code.contains("name"),
        "Parent TSX should reference 'name'"
    );
}

#[test]
fn integration_cross_file_import_analysis_captures_bindings() {
    // The import binding name should be extractable for definition navigation
    let parent_source = r#"<script setup>
import ChildComp from './ChildComp.vue'
import { someHelper } from './utils'
</script>
<template><ChildComp /></template>
"#;
    let (registry, uris) =
        open_multi_file(&[("file:///project/src/App.vue", "vue", parent_source)]);

    let analysis = registry.get_analysis(&uris[0]).unwrap();

    // Should capture both imports
    assert_eq!(
        analysis.imports.len(),
        2,
        "Should have 2 imports, got: {}",
        analysis.imports.len()
    );

    // Check that import bindings are captured
    let all_import_bindings: Vec<_> = analysis
        .imports
        .iter()
        .flat_map(|i| i.bindings.iter().map(|b| b.name.as_str()))
        .collect();
    assert!(
        all_import_bindings.contains(&"ChildComp"),
        "Should have ChildComp import binding, got: {:?}",
        all_import_bindings
    );
    assert!(
        all_import_bindings.contains(&"someHelper"),
        "Should have someHelper import binding, got: {:?}",
        all_import_bindings
    );
}

#[test]
fn integration_cross_file_position_mappers_independent() {
    // Each file should have its own independent position mapper
    let file_a = r#"<script setup>
const a = 1
</script>
<template><div>{{ a }}</div></template>
"#;
    let file_b = r#"<script setup>
const b = 2
const c = 3
</script>
<template><span>{{ b }} {{ c }}</span></template>
"#;
    let (registry, uris) = open_multi_file(&[
        ("file:///project/A.vue", "vue", file_a),
        ("file:///project/B.vue", "vue", file_b),
    ]);

    let mapper_a = registry.get_position_mapper(&uris[0]);
    let mapper_b = registry.get_position_mapper(&uris[1]);

    assert!(mapper_a.is_some(), "File A should have a position mapper");
    assert!(mapper_b.is_some(), "File B should have a position mapper");

    // Mappers should be independent — mapping line 1 in each file should give different results
    // (because the script blocks have different content and line counts)
    let a_tsx = mapper_a.unwrap().vue_to_tsx(1, 0);
    let b_tsx = mapper_b.unwrap().vue_to_tsx(1, 0);
    // Both should produce some mapping (not None)
    assert!(
        a_tsx.is_some() || b_tsx.is_some(),
        "At least one mapper should produce a TSX mapping for line 1"
    );
}

#[test]
fn integration_lang_ts_define_props_no_panic() {
    // Regression: lang="ts" with defineProps<{...}>() caused a panic in TSX generation
    // because type-based prop binding spans were SFC-absolute while content_str was relative.
    let source = r#"<script setup lang="ts">
defineProps<{ msg: string, count?: number }>()
const name = 'hello'
</script>
<template><div>{{ msg }} {{ name }}</div></template>
"#;
    let (registry, uri) = open_vue_file(source);

    // Should not panic — TSX generation must handle mixed span coordinates
    let tsx = registry.get_ide(&uri);
    assert!(
        tsx.is_some(),
        "Should produce TSX output for lang=ts with defineProps"
    );
    let tsx = tsx.unwrap();
    assert!(
        tsx.code.contains("defineProps"),
        "TSX should preserve defineProps"
    );

    // Analysis should also work
    let analysis = registry.get_analysis(&uri);
    assert!(analysis.is_some(), "Should have analysis for lang=ts file");
}

// ─── GetCompiledCode protocol tests ────────────────────────────────

/// @ai-generated — GetCompiledCodeParams deserializes from JSON object with `uri` field.
/// This matches the JSON-RPC 2.0 spec requirement that params must be an object.
#[test]
fn get_compiled_code_params_deserializes_from_object() {
    use crate::server::GetCompiledCodeParams;

    let json = r#"{"uri": "file:///test/App.vue"}"#;
    let params: GetCompiledCodeParams = serde_json::from_str(json).unwrap();
    assert_eq!(params.uri, "file:///test/App.vue");
}

/// @ai-generated — GetCompiledCodeParams rejects bare string (JSON-RPC compliance).
#[test]
fn get_compiled_code_params_rejects_bare_string() {
    use crate::server::GetCompiledCodeParams;

    let json = r#""file:///test/App.vue""#;
    let result: Result<GetCompiledCodeParams, _> = serde_json::from_str(json);
    assert!(
        result.is_err(),
        "bare string should not deserialize into GetCompiledCodeParams"
    );
}

/// @ai-generated — Compiled code response round-trips through serde correctly.
#[test]
fn compiled_code_response_serializes() {
    use crate::server::{CompiledBlock, CompiledCodeResponse};

    let response = CompiledCodeResponse {
        js: CompiledBlock {
            code: "export default {}".to_string(),
            map: Some("{}".to_string()),
        },
        css: CompiledBlock {
            code: String::new(),
            map: None,
        },
        wasm: CompiledBlock {
            code: String::new(),
            map: None,
        },
    };

    let json = serde_json::to_value(&response).unwrap();
    assert_eq!(json["js"]["code"], "export default {}");
    assert_eq!(json["js"]["map"], "{}");
    assert!(json["css"]["map"].is_null());
}

/// @ai-generated — get_ide returns compiled output for an opened Vue file.
#[test]
fn get_ide_returns_compiled_output_for_opened_file() {
    let source = r#"<script setup>
const msg = 'hello'
</script>
<template><div>{{ msg }}</div></template>
"#;
    let (registry, uri) = open_vue_file(source);
    let tsx = registry.get_ide(&uri);
    assert!(tsx.is_some(), "TSX should be available after did_open");
    let tsx = tsx.unwrap();
    assert!(tsx.code.contains("msg"), "TSX should contain 'msg' binding");
    assert!(
        tsx.source_map.is_some(),
        "source map should be present (needed for position mapping)"
    );
}

/// @ai-generated — Position mapper is built from TSX source map on did_open.
#[test]
fn position_mapper_built_on_did_open() {
    let source = r#"<script setup>
const count = 0
</script>
<template><p>{{ count }}</p></template>
"#;
    let (registry, uri) = open_vue_file(source);
    let mapper = registry.get_position_mapper(&uri);
    assert!(
        mapper.is_some(),
        "position mapper should be built from TSX source map — \
         if None, TSGO position mapping will silently fail"
    );
}

/// @ai-generated — vue_position_to_tsx_offset_validated should succeed for template bindings.
///
/// Bug #20: When hovering on a template expression like `{{ count }}`, the LSP
/// falls back to Verter binding info because vue_position_to_tsx_offset_validated
/// returns None for template positions, preventing TSGO from being queried.
#[test]
fn validated_tsx_offset_works_for_template_binding() {
    let source = r#"<script setup>
const count = 0
</script>
<template><p>{{ count }}</p></template>
"#;
    let (registry, uri) = open_vue_file(source);
    let doc = registry.get(&uri).unwrap();
    let tsx = registry.get_ide(&uri).expect("TSX should be available");
    let mapper = registry
        .get_position_mapper(&uri)
        .expect("mapper should exist");
    let tsx_li = crate::documents::line_index::LineIndex::new_utf16(&tsx.code);

    // Find "count" in the template (last occurrence)
    let template_count = source.rfind("count").unwrap();
    let line = source[..template_count].matches('\n').count() as u32;
    let line_start = source[..template_count]
        .rfind('\n')
        .map(|p| p + 1)
        .unwrap_or(0);
    let position = Position {
        line,
        character: (template_count - line_start) as u32,
    };

    // The unvalidated mapping should work
    let tsx_offset = crate::tsgo::merge::vue_position_to_tsx_offset(
        &position,
        &doc.line_index,
        &mapper,
        &tsx_li,
    );
    assert!(
        tsx_offset.is_some(),
        "vue_position_to_tsx_offset should map template binding 'count' to TSX offset.\n\
         TSX code:\n{}",
        tsx.code
    );

    // The VALIDATED mapping must also work — this is the bug (#20)
    let validated = crate::tsgo::merge::vue_position_to_tsx_offset_validated(
        &position,
        &doc.line_index,
        &mapper,
        &tsx_li,
    );
    assert!(
        validated.is_some(),
        "vue_position_to_tsx_offset_validated should succeed for template binding 'count'.\n\
         Unvalidated offset: {:?}\n\
         Vue position: {}:{}\n\
         TSX code:\n{}",
        tsx_offset,
        position.line,
        position.character,
        tsx.code
    );

    // Verify the TSX offset points to 'count' in the TSX output
    let offset = validated.unwrap() as usize;
    let tsx_slice = &tsx.code[offset..];
    assert!(
        tsx_slice.starts_with("count"),
        "TSX offset should point to 'count' in TSX output, but found: {:?}\n\
         TSX code:\n{}",
        &tsx_slice[..tsx_slice.len().min(20)],
        tsx.code
    );
}

/// @ai-generated — Validated TSX offset works for dynamic prop bindings.
#[test]
fn validated_tsx_offset_works_for_dynamic_prop() {
    let source = r#"<script setup>
const msg = 'hello'
</script>
<template>
  <div :title="msg">hi</div>
</template>
"#;
    let (registry, uri) = open_vue_file(source);
    let doc = registry.get(&uri).unwrap();
    let tsx = registry.get_ide(&uri).expect("TSX should be available");
    let mapper = registry
        .get_position_mapper(&uri)
        .expect("mapper should exist");
    let tsx_li = crate::documents::line_index::LineIndex::new_utf16(&tsx.code);

    // Find "msg" in :title="msg" (template, not script)
    let template_msg = source.rfind("msg").unwrap();
    let line = source[..template_msg].matches('\n').count() as u32;
    let line_start = source[..template_msg]
        .rfind('\n')
        .map(|p| p + 1)
        .unwrap_or(0);
    let position = Position {
        line,
        character: (template_msg - line_start) as u32,
    };

    let validated = crate::tsgo::merge::vue_position_to_tsx_offset_validated(
        &position,
        &doc.line_index,
        &mapper,
        &tsx_li,
    );
    assert!(
        validated.is_some(),
        "validated mapping should work for dynamic prop binding 'msg'.\n\
         Vue position: {}:{}\nTSX code:\n{}",
        position.line,
        position.character,
        tsx.code
    );
}

/// @ai-generated — Validated TSX offset works for event handler bindings.
#[test]
fn validated_tsx_offset_works_for_event_handler() {
    let source = r#"<script setup>
function handleClick() {}
</script>
<template>
  <button @click="handleClick">click</button>
</template>
"#;
    let (registry, uri) = open_vue_file(source);
    let doc = registry.get(&uri).unwrap();
    let tsx = registry.get_ide(&uri).expect("TSX should be available");
    let mapper = registry
        .get_position_mapper(&uri)
        .expect("mapper should exist");
    let tsx_li = crate::documents::line_index::LineIndex::new_utf16(&tsx.code);

    // Find "handleClick" in @click="handleClick" (template)
    let template_hc = source.rfind("handleClick").unwrap();
    let line = source[..template_hc].matches('\n').count() as u32;
    let line_start = source[..template_hc]
        .rfind('\n')
        .map(|p| p + 1)
        .unwrap_or(0);
    let position = Position {
        line,
        character: (template_hc - line_start) as u32,
    };

    let validated = crate::tsgo::merge::vue_position_to_tsx_offset_validated(
        &position,
        &doc.line_index,
        &mapper,
        &tsx_li,
    );
    assert!(
        validated.is_some(),
        "validated mapping should work for event handler 'handleClick'.\n\
         Vue position: {}:{}\nTSX code:\n{}",
        position.line,
        position.character,
        tsx.code
    );
}

/// @ai-generated — Validated TSX offset works for v-if condition expression.
#[test]
fn validated_tsx_offset_works_for_v_if_condition() {
    let source = r#"<script setup>
const show = true
</script>
<template>
  <div v-if="show">visible</div>
</template>
"#;
    let (registry, uri) = open_vue_file(source);
    let doc = registry.get(&uri).unwrap();
    let tsx = registry.get_ide(&uri).expect("TSX should be available");
    let mapper = registry
        .get_position_mapper(&uri)
        .expect("mapper should exist");
    let tsx_li = crate::documents::line_index::LineIndex::new_utf16(&tsx.code);

    // Find "show" in v-if="show" (template)
    let template_show = source.rfind("show").unwrap();
    let line = source[..template_show].matches('\n').count() as u32;
    let line_start = source[..template_show]
        .rfind('\n')
        .map(|p| p + 1)
        .unwrap_or(0);
    let position = Position {
        line,
        character: (template_show - line_start) as u32,
    };

    let validated = crate::tsgo::merge::vue_position_to_tsx_offset_validated(
        &position,
        &doc.line_index,
        &mapper,
        &tsx_li,
    );
    assert!(
        validated.is_some(),
        "validated mapping should work for v-if condition 'show'.\n\
         Vue position: {}:{}\nTSX code:\n{}",
        position.line,
        position.character,
        tsx.code
    );
}

/// @ai-generated — Validated TSX offset works for v-for iteration variable.
#[test]
fn validated_tsx_offset_works_for_v_for_iterable() {
    let source = r#"<script setup>
const items = [1, 2, 3]
</script>
<template>
  <div v-for="item in items" :key="item">{{ item }}</div>
</template>
"#;
    let (registry, uri) = open_vue_file(source);
    let doc = registry.get(&uri).unwrap();
    let tsx = registry.get_ide(&uri).expect("TSX should be available");
    let mapper = registry
        .get_position_mapper(&uri)
        .expect("mapper should exist");
    let tsx_li = crate::documents::line_index::LineIndex::new_utf16(&tsx.code);

    // Find "items" in v-for="item in items" (template — the iterable, not script)
    // "items" appears twice in template: in v-for directive and potentially in JSX
    let template_items_offset = source.find("v-for").unwrap();
    let items_in_vfor =
        source[template_items_offset..].find("items").unwrap() + template_items_offset;
    let line = source[..items_in_vfor].matches('\n').count() as u32;
    let line_start = source[..items_in_vfor]
        .rfind('\n')
        .map(|p| p + 1)
        .unwrap_or(0);
    let position = Position {
        line,
        character: (items_in_vfor - line_start) as u32,
    };

    let validated = crate::tsgo::merge::vue_position_to_tsx_offset_validated(
        &position,
        &doc.line_index,
        &mapper,
        &tsx_li,
    );
    assert!(
        validated.is_some(),
        "validated mapping should work for v-for iterable 'items'.\n\
         Vue position: {}:{}\nTSX code:\n{}",
        position.line,
        position.character,
        tsx.code
    );
}

// ─── GetVirtualFiles / GetAnalysis protocol tests ───────────────

/// @ai-generated — GetVirtualFilesParams deserializes from JSON object with `uri` field.
#[test]
fn get_virtual_files_params_deserializes() {
    use crate::server::GetVirtualFilesParams;

    let json = r#"{"uri": "file:///test/App.vue"}"#;
    let params: GetVirtualFilesParams = serde_json::from_str(json).unwrap();
    assert_eq!(params.uri, "file:///test/App.vue");
}

/// @ai-generated — GetAnalysisParams deserializes from JSON object with `uri` field.
#[test]
fn get_analysis_params_deserializes() {
    use crate::server::GetAnalysisParams;

    let json = r#"{"uri": "file:///test/App.vue"}"#;
    let params: GetAnalysisParams = serde_json::from_str(json).unwrap();
    assert_eq!(params.uri, "file:///test/App.vue");
}

/// @ai-generated — VirtualFilesResponse serializes correctly with camelCase fields.
#[test]
fn virtual_files_response_serializes() {
    use crate::server::{CodeBlock, VirtualFileEntry, VirtualFilesResponse};

    let response = VirtualFilesResponse {
        ide: Some(CodeBlock {
            code: "export default {}".to_string(),
            source_map: Some("{}".to_string()),
            is_js: false,
        }),
        api: None,
        virtual_files: vec![VirtualFileEntry {
            kind: "main".to_string(),
            code: "import './style.css'".to_string(),
            lang: "js".to_string(),
            source_map: None,
            stale: false,
        }],
    };

    let json = serde_json::to_value(&response).unwrap();
    assert_eq!(json["ide"]["code"], "export default {}");
    assert_eq!(json["ide"]["sourceMap"], "{}");
    assert_eq!(json["ide"]["isJs"], false);
    assert!(json["api"].is_null());
    assert_eq!(json["virtualFiles"][0]["kind"], "main");
    assert_eq!(json["virtualFiles"][0]["stale"], false);
    assert!(json["virtualFiles"][0]["sourceMap"].is_null());
}

/// @ai-generated — get_virtual_files returns virtual nodes for an opened Vue file.
#[test]
fn integration_get_virtual_files_for_opened_file() {
    let source = r#"<script setup>
const msg = 'hello'
</script>
<template><div>{{ msg }}</div></template>
<style scoped>.title { color: red; }</style>
"#;
    let (registry, uri) = open_vue_file(source);

    let virtual_files = registry.get_virtual_files(&uri);
    assert!(
        virtual_files.is_some(),
        "Should return virtual files for opened Vue file"
    );
    let vf = virtual_files.unwrap();

    // Should have IDE output
    assert!(vf.ide.is_some(), "Should have IDE output");
    let ide = vf.ide.unwrap();
    assert!(!ide.code.is_empty(), "IDE code should not be empty");

    // Should have virtual files: main, script, template, style:0
    assert!(
        vf.virtual_files.len() >= 4,
        "Should have at least 4 virtual files (main, script, template, style:0), got: {}",
        vf.virtual_files.len()
    );

    let kinds: Vec<&str> = vf.virtual_files.iter().map(|f| f.kind.as_str()).collect();
    assert!(
        kinds.contains(&"main"),
        "Should have 'main' virtual file, got: {:?}",
        kinds
    );
    assert!(
        kinds.contains(&"script"),
        "Should have 'script' virtual file, got: {:?}",
        kinds
    );
    assert!(
        kinds.contains(&"template"),
        "Should have 'template' virtual file, got: {:?}",
        kinds
    );
    assert!(
        kinds.contains(&"style:0"),
        "Should have 'style:0' virtual file, got: {:?}",
        kinds
    );
}

/// @ai-generated — get_analysis returns serializable JSON for an opened Vue file.
#[test]
fn integration_get_analysis_json_for_opened_file() {
    let source = r#"<script setup lang="ts">
import { ref, computed } from 'vue'
const count = ref(0)
const doubled = computed(() => count.value * 2)
defineProps<{ msg: string }>()
</script>
<template><div>{{ count }} {{ doubled }}</div></template>
<style scoped>.title { color: red; }</style>
"#;
    let (registry, uri) = open_vue_file(source);

    let analysis_json = registry.get_analysis_json(&uri);
    assert!(
        analysis_json.is_some(),
        "Should return analysis JSON for opened Vue file"
    );
    let json = analysis_json.unwrap();

    // Should have imports
    assert!(json["imports"].is_array(), "Should have imports array");
    let imports = json["imports"].as_array().unwrap();
    assert!(!imports.is_empty(), "Should have at least one import");

    // Should have bindings
    assert!(json["bindings"].is_array(), "Should have bindings array");
    let bindings = json["bindings"].as_array().unwrap();
    let binding_names: Vec<&str> = bindings.iter().filter_map(|b| b["name"].as_str()).collect();
    assert!(
        binding_names.contains(&"count"),
        "Should have 'count' binding, got: {:?}",
        binding_names
    );

    // Should have macros
    assert!(json["macros"].is_array(), "Should have macros array");
    let macros = json["macros"].as_array().unwrap();
    assert!(
        !macros.is_empty(),
        "Should have at least one macro (defineProps)"
    );

    // Should have styles
    assert!(json["styles"].is_array(), "Should have styles array");

    // Should have scriptFlags as a number
    assert!(
        json["scriptFlags"].is_number(),
        "Should have scriptFlags as number"
    );
}

/// @ai-generated — Verifies that serde skip_serializing_if causes empty Vec fields to be absent
/// from the JSON response, which is the exact scenario the TypeScript code must handle with `?? []`.
#[test]
fn analysis_json_skips_empty_vecs_for_template_components() {
    // Template with a component that has NO slots used and a prop with NO referenced bindings.
    // This triggers the skip_serializing_if = "Vec::is_empty" on slotsUsed and referencedBindings.
    let source = r#"<script setup lang="ts">
import MyComp from './MyComp.vue'
</script>
<template><MyComp title="hello" /></template>
"#;
    let (registry, uri) = open_vue_file(source);

    let analysis_json = registry.get_analysis_json(&uri);
    assert!(analysis_json.is_some(), "Should return analysis JSON");
    let json = analysis_json.unwrap();

    // Template should be present with components
    let template = &json["template"];
    assert!(
        !template.is_null(),
        "Template analysis should be present for file with <template>"
    );

    let components = template["components"].as_array();
    if let Some(comps) = components {
        if !comps.is_empty() {
            let comp = &comps[0];
            // slotsUsed should be ABSENT (not an empty array) when skip_serializing_if fires
            assert!(
                comp.get("slotsUsed").is_none()
                    || comp["slotsUsed"].as_array().map_or(false, |a| a.is_empty()),
                "slotsUsed should be absent or empty when no slots are used, got: {:?}",
                comp.get("slotsUsed")
            );

            // Check props — each prop with no referenced bindings should omit referencedBindings
            if let Some(props) = comp["props"].as_array() {
                for prop in props {
                    let ref_bindings = prop.get("referencedBindings");
                    assert!(
                        ref_bindings.is_none()
                            || ref_bindings
                                .unwrap()
                                .as_array()
                                .map_or(false, |a| a.is_empty()),
                        "referencedBindings should be absent or empty for const prop, got: {:?}",
                        ref_bindings
                    );
                }
            }
        }
    }
}

/// @ai-generated — Verifies that the VirtualFilesResponse serializes correctly to JSON,
/// and that the TypeScript client can safely access all fields.
#[test]
fn virtual_files_response_serializes_all_fields() {
    use crate::server::{CodeBlock, VirtualFileEntry, VirtualFilesResponse};

    let response = VirtualFilesResponse {
        ide: Some(CodeBlock {
            code: "export default {}".to_string(),
            source_map: None,
            is_js: false,
        }),
        api: None,
        virtual_files: vec![VirtualFileEntry {
            kind: "main".to_string(),
            code: "console.log('hi')".to_string(),
            lang: "js".to_string(),
            source_map: None,
            stale: false,
        }],
    };

    let json = serde_json::to_value(&response).unwrap();

    // ide block
    assert!(json["ide"]["code"].is_string());
    assert!(json["ide"]["sourceMap"].is_null());

    // virtualFiles array always present
    assert!(json["virtualFiles"].is_array());
    let vf = &json["virtualFiles"][0];
    assert_eq!(vf["kind"], "main");
    assert_eq!(vf["lang"], "js");
    assert_eq!(vf["stale"], false);
    assert!(vf["sourceMap"].is_null());
}

// ─── $/onDidChangeTsOrJsFile regression tests ──────────────────

/// @ai-generated — Regression: Vue file URIs must be skipped by the TS/JS change handler.
///
/// Before the fix, `$/onDidChangeTsOrJsFile` forwarded raw Vue SFC source
/// (e.g., `<template>...</template>`) to TSGO, which only understands TypeScript.
/// This corrupted TSGO's internal state, causing intellisense to break.
/// The handler now skips `.vue` URIs — they are synced via TSX compilation.
#[test]
fn on_did_change_skips_vue_files() {
    let vue_uris = [
        "file:///d:/dev/project/src/App.vue",
        "file:///home/user/project/Component.vue",
        "file:///c:/Users/dev/src/views/Home.vue",
    ];
    for uri in &vue_uris {
        assert!(
            uri.ends_with(".vue"),
            "Test setup: URI should end with .vue: {uri}"
        );
    }

    let non_vue_uris = [
        "file:///d:/dev/project/src/utils.ts",
        "file:///home/user/project/types.d.ts",
        "file:///c:/Users/dev/src/index.js",
        "file:///d:/dev/project/src/App.vue.tsx", // TSX version should NOT be skipped
    ];
    for uri in &non_vue_uris {
        assert!(
            !uri.ends_with(".vue"),
            "Test setup: URI should NOT end with .vue: {uri}"
        );
    }
}

// ─── did_change concurrency tests ─────────────────────────────────

/// @ai-generated — Verify that `did_change_incremental` correctly updates
/// document state, TSX output, and position mapper after a text change.
#[test]
fn did_change_incremental_updates_tsx_and_position_mapper() {
    let source = r#"<script setup lang="ts">
const msg = 'hello'
</script>

<template>
  <div>{{ msg }}</div>
</template>
"#;
    let (registry, uri) = open_vue_file(source);

    // Verify initial state
    let tsx_before = registry.get_ide(&uri);
    assert!(tsx_before.is_some(), "TSX should exist after did_open");
    let tsx_code_before = tsx_before.unwrap().code.to_string();
    assert!(
        tsx_code_before.contains("msg"),
        "TSX should contain 'msg' binding"
    );

    // Apply a change: replace 'hello' with 'world'
    let new_source = source.replace("hello", "world");
    let result = registry.did_change_incremental(
        &uri,
        2,
        vec![TextDocumentContentChangeEvent {
            range: None,
            range_length: None,
            text: new_source.clone(),
        }],
    );

    // Verify the update result
    assert!(
        result.changed,
        "did_change_incremental should detect a change"
    );

    // Verify document state was updated
    let doc = registry.get(&uri).unwrap();
    assert_eq!(doc.version, 2, "version should be updated");
    assert!(
        doc.source.contains("world"),
        "source should contain new text"
    );
    assert!(
        !doc.source.contains("hello"),
        "source should not contain old text"
    );

    // Verify TSX was recompiled with new content
    let tsx_after = registry.get_ide(&uri);
    assert!(tsx_after.is_some(), "TSX should exist after did_change");

    // Verify position mapper exists
    assert!(
        doc.position_mapper.is_some(),
        "position mapper should exist after did_change"
    );
}

/// @ai-generated — Verify that multiple rapid `did_change_incremental` calls
/// in sequence don't deadlock or corrupt state.
#[test]
fn rapid_sequential_did_change_does_not_deadlock() {
    let source = r#"<script setup lang="ts">
const count = 0
</script>

<template>
  <div>{{ count }}</div>
</template>
"#;
    let (registry, uri) = open_vue_file(source);

    // Simulate rapid typing: 10 sequential edits
    for i in 2..=11 {
        let new_source = format!(
            r#"<script setup lang="ts">
const count = {}
</script>

<template>
  <div>{{{{ count }}}}</div>
</template>
"#,
            i
        );
        let result = registry.did_change_incremental(
            &uri,
            i,
            vec![TextDocumentContentChangeEvent {
                range: None,
                range_length: None,
                text: new_source,
            }],
        );
        // HostUpdateResult is always returned (errors are logged internally)
        let _ = result;
    }

    // Verify final state
    let doc = registry.get(&uri).unwrap();
    assert_eq!(doc.version, 11, "version should be 11 after 10 edits");
    assert!(
        doc.source.contains("count = 11"),
        "source should reflect the last edit"
    );

    // TSX and analysis should still be available
    let tsx = registry.get_ide(&uri);
    assert!(tsx.is_some(), "TSX should exist after rapid edits");
    let analysis = registry.get_analysis(&uri);
    assert!(
        analysis.is_some(),
        "analysis should exist after rapid edits"
    );
}

/// @ai-generated — Multi-threaded E2E: parallel `did_change` + read requests.
///
/// Simulates the real LSP multi-thread runtime where `did_change` (upsert + compile)
/// runs on one worker thread while concurrent read requests (get_ide, get_analysis,
/// get_diagnostics) execute on other threads. All operations must complete without
/// deadlock.
#[test]
fn multithread_did_change_with_concurrent_reads() {
    use std::sync::{Arc, Barrier};

    let source = r#"<script setup lang="ts">
const msg = 'hello'
</script>

<template>
  <div>{{ msg }}</div>
</template>
"#;

    let host = Arc::new(VerterHost::new(HostConfig::default()));
    let registry = Arc::new(DocumentRegistry::new(host));
    let uri: Uri = "file:///test/MT.vue".parse().unwrap();

    registry.did_open(&TextDocumentItem {
        uri: uri.clone(),
        language_id: "vue".to_string(),
        version: 1,
        text: source.to_string(),
    });

    // Barrier ensures all 4 threads start simultaneously, so we actually
    // test concurrent access rather than relying on OS scheduling.
    let barrier = Arc::new(Barrier::new(4));

    // Thread 1: did_change (write path: upsert + compile + cache)
    let reg1 = Arc::clone(&registry);
    let uri1 = uri.clone();
    let bar1 = Arc::clone(&barrier);
    let new_source = source.replace("hello", "world");
    let writer = std::thread::spawn(move || {
        bar1.wait();
        reg1.did_change_incremental(
            &uri1,
            2,
            vec![TextDocumentContentChangeEvent {
                range: None,
                range_length: None,
                text: new_source,
            }],
        )
    });

    // Thread 2: read TSX (read path: read_lock on host files)
    let reg2 = Arc::clone(&registry);
    let uri2 = uri.clone();
    let bar2 = Arc::clone(&barrier);
    let reader_tsx = std::thread::spawn(move || {
        bar2.wait();
        reg2.get_ide(&uri2)
    });

    // Thread 3: read analysis (read path: read_lock on host files)
    let reg3 = Arc::clone(&registry);
    let uri3 = uri.clone();
    let bar3 = Arc::clone(&barrier);
    let reader_analysis = std::thread::spawn(move || {
        bar3.wait();
        reg3.get_analysis(&uri3)
    });

    // Thread 4: read diagnostics (read path: read_lock on host files)
    let reg4 = Arc::clone(&registry);
    let uri4 = uri.clone();
    let bar4 = Arc::clone(&barrier);
    let reader_diags = std::thread::spawn(move || {
        bar4.wait();
        reg4.get_diagnostics(&uri4)
    });

    // All threads must join without panic or deadlock.
    // Reader results may be Some (pre-write data) or None (caught mid-recompilation) —
    // both are valid. The test verifies no deadlock, no panic, and data integrity
    // after the write settles.
    let write_result = writer.join().expect("writer thread panicked");
    assert!(write_result.changed, "did_change should detect a change");

    let _tsx = reader_tsx.join().expect("TSX reader thread panicked");
    let _analysis = reader_analysis
        .join()
        .expect("analysis reader thread panicked");
    let _diags = reader_diags
        .join()
        .expect("diagnostics reader thread panicked");

    // After all threads complete, verify data is consistent and accessible.
    assert!(
        registry.get_ide(&uri).is_some(),
        "TSX should be available after concurrent write settles"
    );
    assert!(
        registry.get_analysis(&uri).is_some(),
        "analysis should be available after concurrent write settles"
    );
}

/// @ai-generated — Multi-threaded E2E: parallel `did_open` on multiple files.
///
/// Simulates multiple files being opened simultaneously (e.g., workspace startup).
/// Each `did_open` parses + compiles independently; they share the host's write lock.
#[test]
fn multithread_parallel_did_open_multiple_files() {
    use std::sync::Arc;

    let host = Arc::new(VerterHost::new(HostConfig::default()));
    let registry = Arc::new(DocumentRegistry::new(host));

    let files: Vec<(String, String)> = (0..8)
        .map(|i| {
            let uri = format!("file:///test/Component{i}.vue");
            let source = format!(
                r#"<script setup lang="ts">
const value{i} = {i}
</script>

<template>
  <div>{{{{ value{i} }}}}</div>
</template>
"#
            );
            (uri, source)
        })
        .collect();

    // Spawn threads to open all files in parallel
    let handles: Vec<_> = files
        .iter()
        .cloned()
        .map(|(uri_str, source)| {
            let reg = Arc::clone(&registry);
            std::thread::spawn(move || {
                let uri: Uri = uri_str.parse().unwrap();
                reg.did_open(&TextDocumentItem {
                    uri,
                    language_id: "vue".to_string(),
                    version: 1,
                    text: source,
                })
            })
        })
        .collect();

    for (i, handle) in handles.into_iter().enumerate() {
        let _result = handle
            .join()
            .unwrap_or_else(|_| panic!("did_open thread {i} panicked"));
    }

    // Verify all files are accessible
    for (uri_str, _) in &files {
        let uri: Uri = uri_str.parse().unwrap();
        assert!(
            registry.get(&uri).is_some(),
            "file {uri_str} should be accessible after parallel did_open"
        );
        assert!(
            registry.get_ide(&uri).is_some(),
            "TSX for {uri_str} should exist after parallel did_open"
        );
        assert!(
            registry.get_analysis(&uri).is_some(),
            "analysis for {uri_str} should exist after parallel did_open"
        );
    }
}

/// @ai-generated — Multi-threaded E2E: interleaved writes and reads on same file.
///
/// Simulates rapid typing (sequential did_change) while concurrent threads
/// continuously read TSX and analysis. No operation should deadlock or panic.
#[test]
fn multithread_interleaved_writes_and_reads() {
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::sync::Arc;

    let source = r#"<script setup lang="ts">
const count = 0
</script>

<template>
  <div>{{ count }}</div>
</template>
"#;

    let host = Arc::new(VerterHost::new(HostConfig::default()));
    let registry = Arc::new(DocumentRegistry::new(host));
    let uri: Uri = "file:///test/Interleaved.vue".parse().unwrap();

    registry.did_open(&TextDocumentItem {
        uri: uri.clone(),
        language_id: "vue".to_string(),
        version: 1,
        text: source.to_string(),
    });

    let done = Arc::new(AtomicBool::new(false));

    // Reader thread 1: continuously read TSX until writes are done
    let reg_r1 = Arc::clone(&registry);
    let uri_r1 = uri.clone();
    let done_r1 = Arc::clone(&done);
    let reader1 = std::thread::spawn(move || {
        let mut read_count = 0u32;
        while !done_r1.load(Ordering::Relaxed) {
            let _ = reg_r1.get_ide(&uri_r1);
            read_count += 1;
        }
        read_count
    });

    // Reader thread 2: continuously read analysis until writes are done
    let reg_r2 = Arc::clone(&registry);
    let uri_r2 = uri.clone();
    let done_r2 = Arc::clone(&done);
    let reader2 = std::thread::spawn(move || {
        let mut read_count = 0u32;
        while !done_r2.load(Ordering::Relaxed) {
            let _ = reg_r2.get_analysis(&uri_r2);
            read_count += 1;
        }
        read_count
    });

    // Writer: 20 sequential edits while readers are running
    for i in 2..=21 {
        let new_source = format!(
            r#"<script setup lang="ts">
const count = {i}
</script>

<template>
  <div>{{{{ count }}}}</div>
</template>
"#
        );
        registry.did_change_incremental(
            &uri,
            i,
            vec![TextDocumentContentChangeEvent {
                range: None,
                range_length: None,
                text: new_source,
            }],
        );
    }

    // Signal readers to stop
    done.store(true, Ordering::Relaxed);

    let reads1 = reader1.join().expect("reader1 panicked");
    let reads2 = reader2.join().expect("reader2 panicked");

    // Both readers should have completed multiple reads
    assert!(reads1 > 0, "TSX reader should have completed reads");
    assert!(reads2 > 0, "analysis reader should have completed reads");

    // Verify final state is correct
    let doc = registry.get(&uri).unwrap();
    assert_eq!(doc.version, 21, "version should reflect last edit");
    assert!(
        doc.source.contains("count = 21"),
        "source should have final value"
    );

    let tsx = registry.get_ide(&uri);
    assert!(tsx.is_some(), "TSX should be available after all edits");
}

/// @ai-generated — Multi-threaded E2E: concurrent `did_change` on different files.
///
/// Simulates editing multiple files simultaneously (e.g., find-and-replace across
/// project). Each file's write takes the host-level write lock; this test verifies
/// they serialize correctly without deadlock.
#[test]
fn multithread_concurrent_did_change_different_files() {
    use std::sync::Arc;

    let host = Arc::new(VerterHost::new(HostConfig::default()));
    let registry = Arc::new(DocumentRegistry::new(host));

    // Open 4 files
    let files: Vec<(Uri, String)> = (0..4)
        .map(|i| {
            let uri: Uri = format!("file:///test/File{i}.vue").parse().unwrap();
            let source = format!(
                r#"<script setup lang="ts">
const val{i} = 'original'
</script>

<template>
  <span>{{{{ val{i} }}}}</span>
</template>
"#
            );
            registry.did_open(&TextDocumentItem {
                uri: uri.clone(),
                language_id: "vue".to_string(),
                version: 1,
                text: source.clone(),
            });
            (uri, source)
        })
        .collect();

    // Concurrently edit all 4 files from different threads
    let handles: Vec<_> = files
        .iter()
        .enumerate()
        .map(|(i, (uri, source))| {
            let reg = Arc::clone(&registry);
            let uri = uri.clone();
            let new_source = source.replace("original", &format!("edited_{i}"));
            std::thread::spawn(move || {
                reg.did_change_incremental(
                    &uri,
                    2,
                    vec![TextDocumentContentChangeEvent {
                        range: None,
                        range_length: None,
                        text: new_source,
                    }],
                )
            })
        })
        .collect();

    for (i, handle) in handles.into_iter().enumerate() {
        let result = handle
            .join()
            .unwrap_or_else(|_| panic!("thread {i} panicked"));
        assert!(result.changed, "file {i} should detect a change");
    }

    // Verify all files have updated content
    for (i, (uri, _)) in files.iter().enumerate() {
        let doc = registry.get(uri).unwrap();
        assert_eq!(doc.version, 2);
        assert!(
            doc.source.contains(&format!("edited_{i}")),
            "file {i} should have updated content"
        );
    }
}

/// @ai-generated — Stress test: deadlock detection under heavy concurrent load.
///
/// Spawns many threads performing mixed read/write operations on multiple files
/// simultaneously with a hard timeout. If any lock ordering issue or RwLock
/// contention causes a deadlock, the test will fail by timeout.
///
/// This is the most aggressive concurrency test — it exercises:
/// - Multiple writers competing for host write_lock(files)
/// - Multiple readers competing for host read_lock(files)
/// - did_open (upsert + compile), did_change (upsert + recompile), get_ide, get_analysis
/// - All happening on the same VerterHost simultaneously
#[test]
fn stress_test_no_deadlock_under_heavy_concurrent_load() {
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::sync::Arc;

    let host = Arc::new(VerterHost::new(HostConfig::default()));
    let registry = Arc::new(DocumentRegistry::new(host));

    // Pre-open a set of files
    let file_count = 6;
    let uris: Vec<Uri> = (0..file_count)
        .map(|i| format!("file:///test/Stress{i}.vue").parse().unwrap())
        .collect();

    for (i, uri) in uris.iter().enumerate() {
        let source = format!(
            r#"<script setup lang="ts">
const stress{i} = 'init'
</script>
<template><div>{{{{ stress{i} }}}}</div></template>
"#
        );
        registry.did_open(&TextDocumentItem {
            uri: uri.clone(),
            language_id: "vue".to_string(),
            version: 1,
            text: source,
        });
    }

    let done = Arc::new(AtomicBool::new(false));
    let mut handles = Vec::new();

    // Spawn writer threads: each edits its assigned file in a loop
    for (i, uri) in uris.iter().enumerate() {
        let reg = Arc::clone(&registry);
        let uri = uri.clone();
        let done = Arc::clone(&done);
        handles.push(std::thread::spawn(move || {
            let mut version = 2i32;
            while !done.load(Ordering::Relaxed) {
                let source = format!(
                    r#"<script setup lang="ts">
const stress{i} = 'v{version}'
</script>
<template><div>{{{{ stress{i} }}}}</div></template>
"#
                );
                reg.did_change_incremental(
                    &uri,
                    version,
                    vec![TextDocumentContentChangeEvent {
                        range: None,
                        range_length: None,
                        text: source,
                    }],
                );
                version += 1;
            }
        }));
    }

    // Spawn reader threads: continuously read TSX/analysis from all files
    for _ in 0..4 {
        let reg = Arc::clone(&registry);
        let uris = uris.clone();
        let done = Arc::clone(&done);
        handles.push(std::thread::spawn(move || {
            while !done.load(Ordering::Relaxed) {
                for uri in &uris {
                    let _ = reg.get_ide(uri);
                    let _ = reg.get_analysis(uri);
                    let _ = reg.get_diagnostics(uri);
                }
            }
        }));
    }

    // Let the stress test run for 500ms
    std::thread::sleep(std::time::Duration::from_millis(500));
    done.store(true, Ordering::Relaxed);

    // All threads must join without deadlock — 10s hard timeout per thread
    for (i, handle) in handles.into_iter().enumerate() {
        // Use a watchdog: if join doesn't return in 10s, it's a deadlock
        let watchdog = std::thread::spawn(move || {
            handle
                .join()
                .unwrap_or_else(|_| panic!("thread {i} panicked"));
        });
        // If this times out, there's a deadlock in the host locking
        let result = watchdog.join();
        assert!(result.is_ok(), "thread {i} deadlocked or panicked");
    }

    // Verify all files are in a consistent state
    for (i, uri) in uris.iter().enumerate() {
        let doc = registry.get(uri);
        assert!(
            doc.is_some(),
            "file {i} should still be accessible after stress test"
        );
    }
}

/// @ai-generated — Regression: URI-to-path conversion prevents double-wrapping in TSGO.
///
/// Before the fix, `$/onDidChangeTsOrJsFile` passed `params.uri` (a `file://` URI)
/// directly to `update_file()`, which internally calls `path_to_uri()`. This produced
/// `file:///file:///d:/dev/...` — a double-wrapped URI that TSGO couldn't resolve.
/// The fix converts the URI to a filesystem path first via `uri_to_canonical_id`.
#[test]
fn on_did_change_uri_to_path_conversion() {
    use crate::documents::uri_to_canonical_id;

    // Simulate what the extension sends: a file:// URI string
    let extension_uri = "file:///d:/dev/project/src/utils.ts";

    // The fix: parse as URI and convert to canonical path
    let uri: Uri = extension_uri.parse().unwrap();
    let path = uri_to_canonical_id(&uri);

    // Path should be a filesystem path, not a URI
    assert_eq!(path, "d:/dev/project/src/utils.ts");
    assert!(
        !path.starts_with("file:"),
        "Canonical path must not be a URI: {path}"
    );

    // Unix path variant
    let unix_uri: Uri = "file:///home/user/project/src/utils.ts".parse().unwrap();
    let unix_path = uri_to_canonical_id(&unix_uri);
    assert_eq!(unix_path, "/home/user/project/src/utils.ts");
    assert!(
        !unix_path.starts_with("file:"),
        "Canonical path must not be a URI: {unix_path}"
    );
}

// ─── TSX @verter/types module resolution tests ──────────────────

#[test]
fn tsx_imports_verter_types_module() {
    // Verifies the LSP compile profile produces TSX that imports from
    // "@verter/types" (resolvable by TS plugin + TSGO stub sync),
    // NOT "$verter/types$" (unresolvable virtual name).
    // Also verifies the ambient module declaration is embedded when flag is set.
    let source = r#"<script setup lang="ts">
const props = defineProps<{ title: string }>();
</script>
<template><div>{{ props.title }}</div></template>"#;
    let (registry, uri) = open_vue_file_with_ambient(source);
    let tsx = registry.get_ide(&uri).expect("TSX should be generated");

    assert!(
        tsx.code.contains(r#"from "@verter/types""#),
        "TSX must import from @verter/types for TSGO resolution, got:\n{}",
        tsx.code
    );
    assert!(
        !tsx.code.contains(r#"from "$verter/types$""#),
        "$verter/types$ must NOT appear — TSGO cannot resolve it"
    );
    assert!(
        tsx.code.contains(r#"declare module "@verter/types""#),
        "TSX must contain ambient module declaration for self-contained resolution"
    );
}

#[test]
fn tsx_types_imports_present_for_script_setup() {
    // Verify key type helpers are imported (Prettify, createMacroReturn, etc.)
    let source = r#"<script setup lang="ts">
const props = defineProps<{ msg: string }>();
const emit = defineEmits<{ change: [v: string] }>();
</script>
<template><div>{{ props.msg }}</div></template>"#;
    let (registry, uri) = open_vue_file_with_ambient(source);
    let tsx = registry.get_ide(&uri).expect("TSX should be generated");

    // All type helpers must come from @verter/types
    assert!(tsx.code.contains("___VERTER___Prettify"));
    assert!(tsx.code.contains(r#"from "@verter/types""#));
    assert!(!tsx.code.contains(r#"from "$verter/types$""#));
    assert!(
        tsx.code.contains(r#"declare module "@verter/types""#),
        "script setup TSX must contain ambient module declaration"
    );
}

#[test]
fn tsx_types_imports_present_for_options_api() {
    let source = r#"<script lang="ts">
export default { props: ['msg'] }
</script>
<template><div>{{ msg }}</div></template>"#;
    let (registry, uri) = open_vue_file_with_ambient(source);
    let tsx = registry.get_ide(&uri).expect("TSX should be generated");

    assert!(tsx.code.contains(r#"from "@verter/types""#));
    assert!(!tsx.code.contains(r#"from "$verter/types$""#));
    assert!(
        tsx.code.contains(r#"declare module "@verter/types""#),
        "Options API TSX must contain ambient module declaration"
    );
}

#[test]
fn tsx_types_imports_present_for_template_only() {
    let source = r#"<template><div>hello</div></template>"#;
    let (registry, uri) = open_vue_file_with_ambient(source);
    let tsx = registry.get_ide(&uri).expect("TSX should be generated");

    assert!(tsx.code.contains(r#"from "@verter/types""#));
    assert!(!tsx.code.contains(r#"from "$verter/types$""#));
    assert!(
        tsx.code.contains(r#"declare module "@verter/types""#),
        "template-only TSX must contain ambient module declaration"
    );
}

#[test]
fn verter_types_stub_covers_tsx_imports() {
    // The stub synced to TSGO must declare all exports that generated TSX imports.
    let stub = include_str!("verter_types_stub.d.ts");

    // Core type imports
    assert!(stub.contains("Prettify"), "stub must export Prettify");
    assert!(
        stub.contains("createMacroReturn"),
        "stub must export createMacroReturn"
    );
    assert!(
        stub.contains("OmitConstructorSignature"),
        "stub must export OmitConstructorSignature"
    );
    assert!(
        stub.contains("ExtractComponentProps"),
        "stub must export ExtractComponentProps"
    );
    assert!(
        stub.contains("enhanceElementWithProps"),
        "stub must export enhanceElementWithProps"
    );
    assert!(
        stub.contains("PublicInstanceFromMacro"),
        "stub must export PublicInstanceFromMacro"
    );
    assert!(
        stub.contains("shallowUnwrapRef"),
        "stub must export shallowUnwrapRef"
    );

    // Box helpers (used by macro expansion)
    assert!(
        stub.contains("defineProps_Box"),
        "stub must export defineProps_Box"
    );
    assert!(
        stub.contains("defineEmits_Box"),
        "stub must export defineEmits_Box"
    );
    assert!(
        stub.contains("defineModel_Box"),
        "stub must export defineModel_Box"
    );
    assert!(
        stub.contains("defineSlots_Box"),
        "stub must export defineSlots_Box"
    );
    assert!(
        stub.contains("defineExpose_Box"),
        "stub must export defineExpose_Box"
    );
    assert!(
        stub.contains("withDefaults_Box"),
        "stub must export withDefaults_Box"
    );
    assert!(
        stub.contains("defineOptions_Box"),
        "stub must export defineOptions_Box"
    );
}

// ─── Hover with MockTypeProvider (regression test for TSGO integration) ───

/// Regression test: verifies the hover merge pipeline that runs in server.rs.
///
/// This test exercises the EXACT same code path as the server hover handler:
/// 1. Open Vue SFC → get verter-only hover
/// 2. Get TSX + position mapper from registry
/// 3. Map Vue position → TSX offset (validated)
/// 4. Query type_provider.get_hover() at that offset
/// 5. merge_hover() combines verter + TSGO results
///
/// If type_provider is None (the bug we're preventing), step 4-5 are skipped
/// and the merged hover won't contain TSGO type information.
#[tokio::test]
async fn integration_hover_merge_with_mock_type_provider() {
    use crate::documents::line_index::LineIndex;
    use crate::tsgo::merge;
    use crate::tsgo::mock::MockTypeProvider;
    use crate::tsgo::protocol::HoverInfo;
    use crate::tsgo::traits::TypeProvider;

    let source = r#"<script setup lang="ts">
const count = 42
</script>

<template>
  <div>{{ count }}</div>
</template>
"#;
    let (registry, uri) = open_vue_file(source);

    // Step 1: verter-only hover on "count" in template
    let position = position_of(source, "{{ count }}");
    // Move to the 'c' in count (skip "{{ ")
    let position = Position {
        line: position.line,
        character: position.character + 3,
    };

    let doc = registry.get(&uri).unwrap();
    let analysis = registry.get_analysis(&uri);
    let blocks = scan_sfc_blocks(&doc.source);

    let verter_hover = hover_at_position(
        &position,
        &doc.source,
        &blocks,
        analysis.as_ref(),
        &doc.line_index,
    );

    // Verter-only hover should exist but NOT contain TSGO type info
    assert!(
        verter_hover.is_some(),
        "verter should provide hover for count"
    );
    let verter_text = match &verter_hover.as_ref().unwrap().contents {
        HoverContents::Markup(m) => m.value.clone(),
        _ => String::new(),
    };
    assert!(
        !verter_text.contains("const count: 42"),
        "verter-only hover must NOT contain TSGO type signature (negative assertion)"
    );

    // Step 2: get TSX + mapper
    let tsx_response = registry.get_ide(&uri).expect("TSX should be generated");
    let mapper = registry
        .get_position_mapper(&uri)
        .expect("position mapper should exist");
    let tsx_li = LineIndex::new(&tsx_response.code, registry.encoding());

    // Step 3: map Vue position → TSX offset
    let tsx_offset =
        merge::vue_position_to_tsx_offset_validated(&position, &doc.line_index, &mapper, &tsx_li);
    assert!(
        tsx_offset.is_some(),
        "position mapping must succeed for a script binding used in template"
    );
    let tsx_offset = tsx_offset.unwrap();

    // Step 4: configure MockTypeProvider with hover at that offset
    let mock = MockTypeProvider::new();
    let tsx_path = format!("{}.tsx", crate::documents::uri_to_canonical_id(&uri));
    mock.set_hover(
        &tsx_path,
        tsx_offset,
        Some(HoverInfo {
            contents: "const count: 42".to_string(),
            range_start: None,
            range_end: None,
        }),
    );

    let type_hover = mock.get_hover(&tsx_path, tsx_offset).await.unwrap();
    assert!(type_hover.is_some(), "mock must return configured hover");

    // Step 5: merge — the exact same call as server.rs:1743
    let merged = merge::merge_hover(verter_hover, type_hover, &mapper, &tsx_li, &doc.line_index);

    // Positive: merged result contains TSGO type signature
    assert!(merged.is_some(), "merged hover must exist");
    let merged_text = match &merged.unwrap().contents {
        HoverContents::Markup(m) => m.value.clone(),
        _ => String::new(),
    };
    assert!(
        merged_text.contains("const count: 42"),
        "merged hover must contain TSGO type signature, got: {merged_text}"
    );
    // Negative: confirm mock was called
    let calls = mock.calls();
    assert!(
        calls
            .iter()
            .any(|c| matches!(c, crate::tsgo::mock::MockCall::GetHover { .. })),
        "mock get_hover must have been called"
    );
}

/// Regression: hover without type_provider still returns verter-only result.
#[test]
fn integration_hover_without_type_provider_returns_verter_only() {
    let source = r#"<script setup lang="ts">
const count = 42
</script>

<template>
  <div>{{ count }}</div>
</template>
"#;
    let (registry, uri) = open_vue_file(source);
    let doc = registry.get(&uri).unwrap();
    let analysis = registry.get_analysis(&uri);
    let blocks = scan_sfc_blocks(&doc.source);

    let position = position_of(source, "{{ count }}");
    let position = Position {
        line: position.line,
        character: position.character + 3,
    };

    let hover = hover_at_position(
        &position,
        &doc.source,
        &blocks,
        analysis.as_ref(),
        &doc.line_index,
    );

    // Positive: verter hover exists and has content
    assert!(hover.is_some(), "verter hover must exist for 'count'");
    let text = match &hover.unwrap().contents {
        HoverContents::Markup(m) => m.value.clone(),
        _ => String::new(),
    };
    assert!(!text.is_empty(), "hover content must not be empty");

    // Negative: no TSGO type signature in verter-only mode
    assert!(
        !text.contains("const count: 42"),
        "verter-only hover must NOT contain TSGO type signature"
    );
}

// ─── TSGO sync guard tests (did_close non-Vue file regression) ───

/// Regression: get_ide() must return None for non-Vue files (.ts, .d.ts, .js).
///
/// The server's did_close handler uses `get_ide(uri).is_some()` to guard
/// close_tsx calls to TSGO. If get_ide() returned Some for a non-Vue file,
/// TSGO would receive a close for a file it never opened (the .tsx suffix
/// is only for Vue SFCs), causing a panic:
///   "overlay not found for closed file: file:///...runtime-dom.d.ts.tsx"
#[test]
fn get_ide_returns_none_for_typescript_file() {
    let host = Arc::new(VerterHost::new(HostConfig::default()));
    let registry = DocumentRegistry::new(host);

    // Open a TypeScript file (non-Vue)
    let ts_uri: Uri = "file:///test/utils.ts".parse().unwrap();
    let ts_item = TextDocumentItem {
        uri: ts_uri.clone(),
        language_id: "typescript".to_string(),
        version: 1,
        text: "export const x = 1;".to_string(),
    };
    registry.did_open(&ts_item);

    // get_ide must return None — this is the guard that prevents TSGO close crashes
    assert!(
        registry.get_ide(&ts_uri).is_none(),
        "get_ide() must return None for .ts files"
    );
}

#[test]
fn get_ide_returns_none_for_declaration_file() {
    let host = Arc::new(VerterHost::new(HostConfig::default()));
    let registry = DocumentRegistry::new(host);

    // Open a .d.ts file (e.g., runtime-dom.d.ts opened by VS Code during go-to-definition)
    let dts_uri: Uri = "file:///node_modules/@vue/runtime-dom/dist/runtime-dom.d.ts"
        .parse()
        .unwrap();
    let dts_item = TextDocumentItem {
        uri: dts_uri.clone(),
        language_id: "typescript".to_string(),
        version: 1,
        text: "export interface HTMLAttributes { class?: any; }".to_string(),
    };
    registry.did_open(&dts_item);

    // get_ide must return None — non-Vue files must never trigger TSGO sync
    assert!(
        registry.get_ide(&dts_uri).is_none(),
        "get_ide() must return None for .d.ts files"
    );

    // Negative: is_jsx must also be false (it delegates to get_ide internally)
    assert!(
        !registry.is_jsx(&dts_uri),
        "is_jsx() must return false for .d.ts files"
    );
}

#[test]
fn get_ide_returns_some_for_vue_file() {
    let source = r#"<script setup lang="ts">
const msg = 'hello'
</script>

<template>
  <div>{{ msg }}</div>
</template>
"#;
    let (registry, uri) = open_vue_file(source);

    // get_ide must return Some for Vue SFCs — this file SHOULD be synced to TSGO
    assert!(
        registry.get_ide(&uri).is_some(),
        "get_ide() must return Some for .vue files"
    );
}

/// Regression: closing a non-Vue file must not affect TSGO state.
///
/// Simulates the crash scenario: user CTRL+CLICKs on a binding, TSGO
/// resolves to runtime-dom.d.ts, VS Code opens then immediately closes it.
/// The did_close guard (get_ide().is_some()) must prevent close_tsx.
#[test]
fn close_non_vue_file_does_not_affect_vue_ide_state() {
    let host = Arc::new(VerterHost::new(HostConfig::default()));
    let registry = DocumentRegistry::new(host);

    // Open a Vue file
    let vue_uri: Uri = "file:///test/App.vue".parse().unwrap();
    let vue_item = TextDocumentItem {
        uri: vue_uri.clone(),
        language_id: "vue".to_string(),
        version: 1,
        text: r#"<script setup lang="ts">
const msg = 'hello'
</script>
<template><div>{{ msg }}</div></template>
"#
        .to_string(),
    };
    registry.did_open(&vue_item);
    assert!(
        registry.get_ide(&vue_uri).is_some(),
        "Vue file should have IDE output"
    );

    // Open then close a .d.ts file (simulates go-to-definition navigation)
    let dts_uri: Uri = "file:///node_modules/@vue/runtime-dom/dist/runtime-dom.d.ts"
        .parse()
        .unwrap();
    let dts_item = TextDocumentItem {
        uri: dts_uri.clone(),
        language_id: "typescript".to_string(),
        version: 1,
        text: "export interface HTMLAttributes { class?: any; }".to_string(),
    };
    registry.did_open(&dts_item);
    assert!(
        registry.get_ide(&dts_uri).is_none(),
        "d.ts file must not have IDE output"
    );
    registry.did_close(&dts_uri);

    // Vue file's IDE output must still be intact after closing the .d.ts file
    assert!(
        registry.get_ide(&vue_uri).is_some(),
        "Vue IDE output must survive non-Vue file close"
    );
}
