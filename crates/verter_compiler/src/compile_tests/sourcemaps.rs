use super::*;

/// @ai-generated - TSX source map should be generated (not empty) for SFCs with template
#[test]
pub(super) fn tsx_source_map_is_generated() {
    let source = r#"<script setup>
const msg = 'hello'
</script>

<template>
  <div>{{ msg }}</div>
</template>
"#;
    let result = compile_tsx(source);
    assert!(result.errors.is_empty(), "errors: {:?}", result.errors);

    let tsx = result.tsx.as_ref().expect("tsx block");
    assert!(
        !tsx.source_map.is_empty(),
        "TSX source map should not be empty"
    );

    // Parse the source map JSON to validate structure
    let sm: serde_json::Value =
        serde_json::from_str(&tsx.source_map).expect("TSX source map should be valid JSON");
    assert_eq!(sm.get("version").and_then(|v| v.as_u64()), Some(3));
    assert!(
        sm.get("mappings")
            .and_then(|v| v.as_str())
            .map(|s| !s.is_empty())
            .unwrap_or(false),
        "Mappings should not be empty"
    );
    assert!(
        sm.get("sources")
            .and_then(|v| v.as_array())
            .map(|a| !a.is_empty())
            .unwrap_or(false),
        "Sources array should not be empty"
    );
}

/// @ai-generated - TSX source map must be independent from force_js mode.
#[test]
pub(super) fn tsx_force_js_toggle_does_not_change_source_map() {
    let source = r#"<script setup lang="ts">
import { ref } from 'vue'
const count: number = 1
const msg: string = 'hello'
</script>
<template>
  <button @click="count++">{{ msg }} {{ count }}</button>
</template>"#;

    let force_js_true = compile_tsx_with_force_js(source, true);
    let force_js_false = compile_tsx_with_force_js(source, false);

    assert!(
        force_js_true.errors.is_empty(),
        "force_js=true compile errors: {:?}",
        force_js_true.errors
    );
    assert!(
        force_js_false.errors.is_empty(),
        "force_js=false compile errors: {:?}",
        force_js_false.errors
    );

    let tsx_true = force_js_true.tsx.expect("tsx block (force_js=true)");
    let tsx_false = force_js_false.tsx.expect("tsx block (force_js=false)");

    assert_eq!(
        tsx_true.source_map, tsx_false.source_map,
        "TSX source map must be identical regardless of force_js"
    );
}

/// @ai-generated — TSX source map: interpolation `{{ msg }}` maps back to Vue source
#[test]
fn tsx_sourcemap_interpolation() {
    let source = r#"<script setup>
const msg = 'hello'
</script>

<template>
  <div>{{ msg }}</div>
</template>
"#;
    let result = compile_tsx_with_source_map(source);
    assert!(result.errors.is_empty(), "errors: {:?}", result.errors);
    let tsx = result.tsx.as_ref().expect("tsx block");
    verify_sourcemap_tokens_in_bounds(source, tsx);
}

/// @ai-generated — TSX source map: component tag maps back
#[test]
fn tsx_sourcemap_component_tag() {
    let source = r#"<script setup>
import MyComponent from './MyComponent.vue'
</script>

<template>
  <MyComponent />
</template>
"#;
    let result = compile_tsx_with_source_map(source);
    assert!(result.errors.is_empty(), "errors: {:?}", result.errors);
    let tsx = result.tsx.as_ref().expect("tsx block");
    verify_sourcemap_tokens_in_bounds(source, tsx);
}

/// @ai-generated — TSX source map: multi-byte characters (CJK, emoji) in script and template
#[test]
fn tsx_sourcemap_multibyte_characters() {
    let source = r#"<script setup>
// 你好世界
const msg = '你好'
</script>

<template>
  <div>{{ msg }} 🎉</div>
</template>
"#;
    let result = compile_tsx_with_source_map(source);
    assert!(result.errors.is_empty(), "errors: {:?}", result.errors);
    let tsx = result.tsx.as_ref().expect("tsx block");
    verify_sourcemap_tokens_in_bounds(source, tsx);
}

/// @ai-generated — TSX source map: template-only SFC
#[test]
fn tsx_sourcemap_template_only() {
    let source = r#"<template>
  <div>hello</div>
</template>
"#;
    let result = compile_tsx_with_source_map(source);
    assert!(result.errors.is_empty(), "errors: {:?}", result.errors);
    let tsx = result.tsx.as_ref().expect("tsx block");
    verify_sourcemap_tokens_in_bounds(source, tsx);
}

/// @ai-generated — TSX source map: v-else-if expression maps correctly.
#[test]
fn tsx_sourcemap_v_else_if_expression_maps_correctly() {
    let source = r#"<script lang="ts" setup>
let isLoggedIn = false;
let hasPermission = false;
</script>

<template>
  <div v-if="isLoggedIn && hasPermission">Full Access</div>
  <div v-else-if="isLoggedIn && !hasPermission">Limited Access</div>
  <div v-else>No Access</div>
</template>
"#;
    let result = compile_tsx_with_source_map(source);
    assert!(result.errors.is_empty(), "errors: {:?}", result.errors);

    let tsx = result.tsx.as_ref().expect("tsx block");
    verify_sourcemap_tokens_in_bounds(source, tsx);

    let sm =
        oxc_sourcemap::SourceMap::from_json_string(&tsx.source_map).expect("valid source map JSON");
    let lookup = sm.generate_lookup_table();

    // Find `isLoggedIn` in the v-else-if expression in TSX
    let elseif_start = tsx.code.find("else if(isLoggedIn").unwrap();
    let tsx_elseif_islogged = tsx.code[elseif_start..].find("isLoggedIn").unwrap() + elseif_start;
    let (tsx_line, tsx_col) = find_line_col_at(&tsx.code, tsx_elseif_islogged);

    // Find `isLoggedIn` in v-else-if attribute in Vue source
    let vue_elseif_attr = source.find(r#"v-else-if="isLoggedIn"#).unwrap() + 11; // skip v-else-if="
    let (vue_line, vue_col) = find_line_col_at(source, vue_elseif_attr);

    let token = sm
        .lookup_token(&lookup, tsx_line, tsx_col)
        .expect("source map token for v-else-if isLoggedIn");
    assert!(
        token.get_source_id().is_some(),
        "v-else-if isLoggedIn should have source mapping"
    );

    let mut mapped_col = token.get_src_col();
    if token.get_dst_line() == tsx_line && tsx_col > token.get_dst_col() {
        mapped_col += tsx_col - token.get_dst_col();
    }
    assert_eq!(
        token.get_src_line(),
        vue_line,
        "v-else-if isLoggedIn: wrong Vue line"
    );
    assert_eq!(mapped_col, vue_col, "v-else-if isLoggedIn: wrong Vue col");
}

// ── Binding occurrence position tests ─────────────────────────────

/// @ai-generated — Binding occurrences have correct span offsets
///
/// Verifies that each RawBindingOccurrence's span.start..span.end maps to the
/// correct binding name in the original source. This catches position bugs in
/// template data extraction.
#[test]
fn binding_occurrence_spans_match_source() {
    let source = r#"<script setup>
const msg = 'hello'
const count = 0
</script>

<template>
  <div>{{ msg }}</div>
  <span>{{ count }}</span>
</template>
"#;
    let result = compile_tsx_with_template_data(source);
    assert!(result.errors.is_empty(), "errors: {:?}", result.errors);

    let tpl_data = result.template_data.as_ref().expect("template data");
    assert!(
        !tpl_data.binding_occurrences.is_empty(),
        "Expected binding occurrences but got none"
    );

    for occ in &tpl_data.binding_occurrences {
        let start = occ.span.start as usize;
        let end = occ.span.end as usize;
        assert!(
            end <= source.len(),
            "Binding '{}' span {}..{} exceeds source length {}",
            occ.name,
            start,
            end,
            source.len()
        );
        let slice = &source[start..end];
        assert_eq!(
            slice, occ.name,
            "Binding occurrence span {}..{} contains '{}' but expected '{}'",
            start, end, slice, occ.name
        );
    }
}

#[test]
fn tsx_destructured_block_has_no_offset_comments() {
    let source = r#"<script setup lang="ts">
import { ref } from 'vue'
const count = ref(0)
const message = ref("hi")
</script>
<template><div>{{ count }} {{ message }}</div></template>"#;
    let result = compile_tsx(source);
    assert!(result.errors.is_empty(), "errors: {:?}", result.errors);
    let tsx = result.tsx.as_ref().expect("tsx block");

    // Negative: NO /*digits,digits*/ offset comments in the output
    assert!(
        !has_offset_comment(&tsx.code),
        "Offset comments /*start,end*/ must NOT appear in TSX output.\nTSX:\n{}",
        tsx.code
    );

    // Positive: boundary markers still present
    assert!(tsx.code.contains("/* verter-destructured-start */"));
    assert!(tsx.code.contains("/* verter-destructured-end */"));

    // Positive: destructured_block metadata is populated
    let meta = tsx
        .destructured_block
        .as_ref()
        .expect("destructured_block metadata should be populated");

    // Should have bindings for count and message
    let names: Vec<&str> = meta.bindings.iter().map(|b| b.name.as_str()).collect();
    assert!(
        names.contains(&"count"),
        "bindings should include 'count', got: {:?}",
        names
    );
    assert!(
        names.contains(&"message"),
        "bindings should include 'message', got: {:?}",
        names
    );

    // Verify source spans point to correct identifiers in SFC
    for binding in &meta.bindings {
        let span_text =
            &source[binding.source_span.start as usize..binding.source_span.end as usize];
        assert_eq!(
            span_text, binding.name,
            "source_span for '{}' should point to identifier in SFC",
            binding.name
        );
    }

    // Block range should bracket the destructured block in TSX
    let start_marker = tsx.code.find("/* verter-destructured-start */").unwrap();
    let end_marker = tsx.code.find("/* verter-destructured-end */").unwrap()
        + "/* verter-destructured-end */".len();
    assert!(
        meta.block_start as usize <= start_marker + "/* verter-destructured-start */".len() + 10,
        "block_start should be near the start marker"
    );
    assert!(
        (meta.block_end as usize) >= end_marker - 10,
        "block_end should be near the end marker"
    );
}

#[test]
fn tsx_destructured_block_meta_non_ascii_spans() {
    // Emoji 😀 is 4 UTF-8 bytes, 2 UTF-16 code units
    let source = "<script setup lang=\"ts\">\nimport { ref } from 'vue'\n// 😀\nconst name = ref('')\n</script>\n<template><div>{{ name }}</div></template>";
    let result = compile_tsx(source);
    assert!(result.errors.is_empty(), "errors: {:?}", result.errors);
    let tsx = result.tsx.as_ref().expect("tsx block");

    // Negative: NO offset comments
    assert!(
        !has_offset_comment(&tsx.code),
        "Offset comments must NOT appear in TSX output.\nTSX:\n{}",
        tsx.code
    );

    // Positive: metadata has the binding with correct SFC byte span
    let meta = tsx
        .destructured_block
        .as_ref()
        .expect("destructured_block metadata");
    let name_binding = meta
        .bindings
        .iter()
        .find(|b| b.name == "name")
        .expect("should have 'name' binding");

    let name_decl_pos = source.find("const name").unwrap();
    let name_ident_start = name_decl_pos + "const ".len();
    let name_ident_end = name_ident_start + "name".len();
    assert_eq!(&source[name_ident_start..name_ident_end], "name");
    assert_eq!(name_binding.source_span.start, name_ident_start as u32);
    assert_eq!(name_binding.source_span.end, name_ident_end as u32);
}

// ── Sourcemap tests for attrs/generic content ───────────────────────────────

#[test]
fn tsx_attrs_content_is_sourcemapped() {
    let source = r#"<script setup lang="ts" attrs="{ class: string }">
const msg = ref('hello')
</script>
<template><div>{{ msg }}</div></template>"#;
    let result = compile_tsx_with_source_map(source);
    assert!(result.errors.is_empty(), "errors: {:?}", result.errors);
    let tsx = result.tsx.as_ref().expect("tsx block");

    let sm =
        oxc_sourcemap::SourceMap::from_json_string(&tsx.source_map).expect("valid source map JSON");
    let lookup = sm.generate_lookup_table();

    // Find "{ class: string }" in the generated TSX output
    let target = "{ class: string }";
    let gen_pos = tsx
        .code
        .find(target)
        .expect("should find attrs content in TSX output");

    // Find the same text in the original SFC source
    let src_pos = source
        .find(target)
        .expect("should find attrs content in SFC source");

    // Look up the sourcemap token at the generated position
    let gen_line = tsx.code[..gen_pos].matches('\n').count() as u32;
    let gen_col = (gen_pos - tsx.code[..gen_pos].rfind('\n').map_or(0, |p| p + 1)) as u32;

    let token = sm
        .lookup_token(&lookup, gen_line, gen_col)
        .expect("should have sourcemap token for attrs content");

    // The token should map back to the original source position
    let src_line = source[..src_pos].matches('\n').count() as u32;
    let src_col = (src_pos - source[..src_pos].rfind('\n').map_or(0, |p| p + 1)) as u32;

    assert_eq!(
        token.get_src_line(),
        src_line,
        "attrs content should map back to SFC source line"
    );
    assert_eq!(
        token.get_src_col(),
        src_col,
        "attrs content should map back to SFC source column"
    );
}
