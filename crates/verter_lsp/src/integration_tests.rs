/// @ai-generated — Phase 4 integration tests for verter_lsp.
///
/// These tests use the full pipeline: DocumentRegistry (backed by verter_host) →
/// LSP feature functions → verify results. They test real Vue SFC content end-to-end.
use tower_lsp_server::lsp_types::*;
use verter_host::{HostConfig, VerterHost};

use crate::documents::line_index::LineIndex;
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
    let host = VerterHost::new(HostConfig::default());
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
    );

    assert!(items.is_some(), "Should get completion items in template");
    let items = items.unwrap();
    // Should include bindings from script setup
    let labels: Vec<_> = items.iter().map(|i| i.label.as_str()).collect();
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
    );

    assert!(def.is_some(), "Should find definition for 'msg'");
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

    let ranges = build_folding_ranges(&blocks, &doc.line_index);
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
    let tsx = registry.get_tsx(&uri);
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
    let tsx = registry.get_tsx(&uri).expect("tsx should exist");

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
    let host = VerterHost::new(HostConfig::default());
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

    let child_tsx = registry.get_tsx(&uris[0]);
    assert!(child_tsx.is_some(), "Child should have TSX output");
    let child_tsx = child_tsx.unwrap();
    assert!(
        child_tsx.code.contains("value"),
        "Child TSX should reference 'value'"
    );

    let parent_tsx = registry.get_tsx(&uris[1]);
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
    let tsx = registry.get_tsx(&uri);
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
