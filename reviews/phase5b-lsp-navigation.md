# Phase 5b: LSP Navigation & Editing Features Review

## Overall: SOLID Foundation with Critical Gaps in Rename & Extract

Broad feature coverage with bidirectional CSS navigation. Two critical issues: kebab-case component handling and text-based rename.

---

## Critical Issues

### C1. `is_ident_byte` Excludes Hyphen — Breaks Kebab-Case Components
**Files**: definition.rs, references.rs, rename.rs, document_highlight.rs (4 copies)

```rust
fn is_ident_byte(b: u8) -> bool {
    b.is_ascii_alphanumeric() || b == b'_' || b == b'$'
}
```

`word_at_offset` relies on this. Clicking on `<my-component>` extracts only `my` or `component`, not `my-component`. The `to_pascal_case` fallback only helps if the full kebab-case word is extracted first.

**Impact**: Go-to-definition, find-references, rename, and highlighting all broken for kebab-case component tags after the first hyphen. This is a fundamental Vue pattern.

### C2. Rename Uses Text-Based Word Search — Risks Corruption
**File**: rename.rs:155-183

`find_all_word_occurrences(content, &word)` is a simple text search. Renames can hit:
- String literals (`"count"`)
- Comments (`// count items`)
- Object property shorthand keys
- Same-named variables in nested scopes (v-for shadows)

**Impact**: Renaming a binding can corrupt string literals, comments, object property keys, and unrelated variables.

---

## High Issues

### H1. Definition Ignores Template Refs
No handling for `ref="myRef"` in templates. Clicking on ref attribute values does not navigate to `useTemplateRef('myRef')` or `const myRef = ref()` in script.

### H2. Definition Skips Dynamic Classes — But References Handle Them
`find_css_target_in_template` skips `attr.is_dynamic`, but `references.rs` handles dynamic `:class` via `extract_dynamic_class_names_rich`. Asymmetric behavior: "Find References" on dynamic class works, "Go to Definition" does not.

### H3. Extract Component Is a Bare Scaffold
Extracted component is always:
```html
<template>
  {selected_text}
</template>
<script setup lang="ts">
</script>
```
No analysis of referenced bindings, imports, events, props, or slots. Component will almost always have errors.

### H4. Organize Imports Removes Entire Import When Only Some Specifiers Unused
`import { ref, computed } from 'vue'` with only `computed` unused → entire import removed including used `ref`. Comment acknowledges this limitation.

**Impact**: Applying "organize imports" can delete used imports, causing build failures.

### H5. Duplicated Utility Functions Across 4+ Files
`word_at_offset` (4 copies), `is_ident_byte` (4 copies), `find_all_word_occurrences` (3 copies). Fixing C1 requires updating all copies. Any divergence creates subtle behavioral differences.

---

## Medium Issues

### M1. Linked Editing — Kebab/PascalCase Component Tag Conversion
Close tag found by exact string match. If analysis stores PascalCase but source is kebab-case, lookup could fail.

### M2. Call Hierarchy Is Shallow — Same-File Only
`incoming_calls` only searches current file's `template.binding_occurrences`. No cross-file call tracking, no script block call sites, no composable-to-composable chains.

### M3. Formatting Ignores `FormattingOptions` Parameter
`_options` (tab_size, insert_spaces) completely ignored. Always uses `"\n\n"` between blocks. Windows line endings (`\r\n`) not respected.

### M4. CSS Class Search Uses `find()` — Fragile
`attr_text.find(value.as_str())` for `class="btn-btn"` returns first `btn`, potentially wrong position.

### M5. `generate_component_name` Always Returns "ExtractedComponent"
Second extraction from same file silently fails to create file (`ignore_if_exists: true`) but still modifies source.

### M6. Single-Character Identifier False Positives
`find_all_word_occurrences` for `x` matches every standalone `x`. Word boundary checking helps but complex template expressions can still produce spurious results.

### M7. CSS Definition Highlights Entire Attribute Span
Navigating from `.btn` in style to template highlights entire `class="btn primary"` attribute, not just `btn`.

---

## Low Issues

- L1: No UNC/WSL path handling in `resolved_import_definition`
- L2: Organize imports FunctionCall initializer check limited
- L3: No handling of `v-bind()` in CSS or CSS modules class references
- L4: `is_inside_html_comment` uses `rfind`, can be fooled by strings
- L5: Test helper duplication across modules

---

## Strengths

### S1. Clean Sentinel URI Pattern
All features use `SAME_FILE_URI` sentinel; server.rs replaces before returning to client.

### S2. Excellent CSS Bidirectional Navigation
Template class/id ↔ style selector. DOM query selector navigation with CSS rule fallback.

### S3. TypeProvider Integration Architecture
Clean separation of Verter's analysis-based results from TSGO TypeProvider enhancement. Merge functions handle dedup and position mapping.

### S4. Thorough Test Coverage
Comprehensive positive/negative/edge case tests per feature module.

### S5. HTML Comment Awareness
Definition correctly skips navigation from inside HTML comments.

### S6. Path Alias Resolution
Definition supports tsconfig path alias resolution via `resolve_path` callback with import span fallback.

---

## Priority Fixes
1. **C1**: Fix `is_ident_byte` / extract word utilities to shared module
2. **C2**: Replace text-based rename with analysis-span-based rename
3. **H4**: Organize imports — remove only unused specifiers, not whole import
4. **H2**: Add dynamic class support to definition CSS navigation
5. **H3**: Analyze bindings in extract-component to generate props/imports
6. **M3**: Respect formatting options, especially line endings
