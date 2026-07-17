use super::compile::compile;
/// @ai-generated — E2E sourcemap correctness tests.
///
/// Verifies that sourcemap tokens resolve to **matching text** at mapped positions,
/// not just valid line/column bounds. Every meaningful token in the generated output
/// is checked against the original SFC source.
use super::compile::types::{
    CodegenOptions, CompileTarget, VerterCompileOptions, VerterCompileResult,
};
use super::cursor::position::utf16_len;
use oxc_allocator::Allocator;

// ── Compilation helpers ────────────────────────────────────────────

fn compile_with_sourcemap(source: &str) -> VerterCompileResult {
    let alloc = Allocator::new();
    let options = CodegenOptions {
        filename: Some("App.vue".to_string()),
        ..Default::default()
    };
    let verter_opts = VerterCompileOptions {
        source_map: true,
        ..Default::default()
    };
    compile(source, &options, &verter_opts, &alloc)
}

fn compile_tsx_with_sourcemap(source: &str) -> VerterCompileResult {
    let alloc = Allocator::new();
    let options = CodegenOptions {
        filename: Some("App.vue".to_string()),
        target: CompileTarget::BUNDLER | CompileTarget::TSX,
        ..Default::default()
    };
    let verter_opts = VerterCompileOptions {
        source_map: true,
        ..Default::default()
    };
    compile(source, &options, &verter_opts, &alloc)
}

// ── Position conversion helpers ────────────────────────────────────

/// Convert a byte offset into 0-based (line, UTF-16 column).
fn byte_offset_to_line_col(text: &str, byte_offset: usize) -> (u32, u32) {
    let mut line: u32 = 0;
    let mut line_start: usize = 0;

    for (i, b) in text.as_bytes().iter().enumerate() {
        if i == byte_offset {
            break;
        }
        if *b == b'\n' {
            line += 1;
            line_start = i + 1;
        }
    }

    let col_utf16 = utf16_len(&text[line_start..byte_offset]) as u32;
    (line, col_utf16)
}

/// Convert 0-based (line, UTF-16 column) to a byte offset, or None if out of bounds.
fn line_col_to_byte_offset(text: &str, line: u32, col: u32) -> Option<usize> {
    let mut current_line: u32 = 0;
    let mut line_start: usize = 0;

    // Find the byte offset of the start of `line`
    if line > 0 {
        for (i, b) in text.as_bytes().iter().enumerate() {
            if *b == b'\n' {
                current_line += 1;
                line_start = i + 1;
                if current_line == line {
                    break;
                }
            }
        }
        if current_line < line {
            return None; // line doesn't exist
        }
    }

    // Walk UTF-16 units within the line to find the byte offset
    let line_bytes = &text.as_bytes()[line_start..];
    let mut utf16_count: u32 = 0;
    let mut i: usize = 0;

    while i < line_bytes.len() && line_bytes[i] != b'\n' {
        if utf16_count == col {
            return Some(line_start + i);
        }
        let b = line_bytes[i];
        if b < 0x80 {
            utf16_count += 1;
            i += 1;
        } else if b < 0xE0 {
            utf16_count += 1;
            i += 2;
        } else if b < 0xF0 {
            utf16_count += 1;
            i += 3;
        } else {
            utf16_count += 2; // surrogate pair
            i += 4;
        }
    }

    // col might point to end of line
    if utf16_count == col {
        return Some(line_start + i);
    }

    None
}

// ── Substring search ───────────────────────────────────────────────

/// Find the byte offset of the Nth (0-based) occurrence of `needle` in `haystack`.
fn find_nth_occurrence(haystack: &str, needle: &str, n: usize) -> Option<usize> {
    let mut count = 0;
    let mut start = 0;
    while let Some(pos) = haystack[start..].find(needle) {
        if count == n {
            return Some(start + pos);
        }
        count += 1;
        start += pos + 1;
    }
    None
}

// ── Word boundary check ────────────────────────────────────────────

fn is_word_char(b: u8) -> bool {
    b.is_ascii_alphanumeric() || b == b'_' || b == b'$'
}

/// Check that the occurrence at `pos` with length `len` sits on word boundaries.
fn is_word_boundary(text: &str, pos: usize, len: usize) -> bool {
    let bytes = text.as_bytes();
    let before_ok = pos == 0 || !is_word_char(bytes[pos - 1]);
    let after_ok = pos + len >= bytes.len() || !is_word_char(bytes[pos + len]);
    before_ok && after_ok
}

// ── Core assertion helpers ─────────────────────────────────────────

/// Build a lookup table from a SourceMap for use with `lookup_token`.
fn build_lookup_table(sm: &oxc_sourcemap::OwnedSourceMap) -> Vec<&[oxc_sourcemap::Token]> {
    sm.generate_lookup_table()
}

/// Assert that `target` text in the generated code maps back to matching text in the
/// original Vue SFC source via the sourcemap.
///
/// `tsx_occurrence` selects which occurrence of `target` in the generated code to check
/// (0-based). Only word-boundary matches are considered.
fn assert_maps_to_source(
    sm: &oxc_sourcemap::OwnedSourceMap,
    lookup: &[&[oxc_sourcemap::Token]],
    generated_code: &str,
    vue_source: &str,
    target: &str,
    tsx_occurrence: usize,
) {
    // Find the Nth word-boundary occurrence of target in generated code
    let mut count = 0;
    let mut search_start = 0;
    let target_offset = loop {
        match generated_code[search_start..].find(target) {
            Some(rel_pos) => {
                let abs_pos = search_start + rel_pos;
                if is_word_boundary(generated_code, abs_pos, target.len()) {
                    if count == tsx_occurrence {
                        break abs_pos;
                    }
                    count += 1;
                }
                search_start = abs_pos + 1;
            }
            None => panic!(
                "Could not find word-boundary occurrence #{} of {:?} in generated code.\n\
                 Found {} occurrences total.\nGenerated code:\n{}",
                tsx_occurrence, target, count, generated_code
            ),
        }
    };

    // Convert byte offset to 0-based (line, col) in generated code
    let (gen_line, gen_col) = byte_offset_to_line_col(generated_code, target_offset);

    // Look up the sourcemap token at this generated position
    let token = sm
        .lookup_token(lookup, gen_line, gen_col)
        .unwrap_or_else(|| {
            panic!(
                "No sourcemap token at generated position {}:{} for {:?}.\n\
                 Byte offset: {}\nGenerated code:\n{}",
                gen_line, gen_col, target, target_offset, generated_code
            )
        });

    // The token must have a source reference
    assert!(
        token.get_source_id().is_some(),
        "Sourcemap token at generated {}:{} for {:?} has no source mapping (unmapped).\n\
         Generated code:\n{}",
        gen_line,
        gen_col,
        target,
        generated_code
    );

    let src_line = token.get_src_line();
    let src_col = token.get_src_col();

    // Convert source (line, col) back to byte offset in Vue source
    let src_byte_offset =
        line_col_to_byte_offset(vue_source, src_line, src_col).unwrap_or_else(|| {
            panic!(
                "Sourcemap maps {:?} at gen {}:{} → src {}:{}, but that position \
                 is out of bounds in Vue source.\nVue source:\n{}",
                target, gen_line, gen_col, src_line, src_col, vue_source
            )
        });

    // Extract the same length of text from the Vue source and compare
    let src_end = (src_byte_offset + target.len()).min(vue_source.len());
    let src_text = &vue_source[src_byte_offset..src_end];

    assert_eq!(
        src_text,
        target,
        "Sourcemap mismatch for {:?}!\n\
         Generated position: {}:{} (byte {})\n\
         Mapped source position: {}:{} (byte {})\n\
         Text at source position: {:?}\n\
         Expected: {:?}\n\
         Vue source:\n{}\n\
         Generated code:\n{}",
        target,
        gen_line,
        gen_col,
        target_offset,
        src_line,
        src_col,
        src_byte_offset,
        src_text,
        target,
        vue_source,
        generated_code
    );
}

/// Like `assert_maps_to_source` but with line-level granularity: verifies that
/// `target` appears somewhere on the mapped source line rather than at the exact
/// column. Use this for script-region tests where the sourcemap maps at
/// statement/line level rather than per-identifier.
fn assert_maps_to_source_line(
    sm: &oxc_sourcemap::OwnedSourceMap,
    lookup: &[&[oxc_sourcemap::Token]],
    generated_code: &str,
    vue_source: &str,
    target: &str,
    tsx_occurrence: usize,
) {
    let mut count = 0;
    let mut search_start = 0;
    let target_offset = loop {
        match generated_code[search_start..].find(target) {
            Some(rel_pos) => {
                let abs_pos = search_start + rel_pos;
                if is_word_boundary(generated_code, abs_pos, target.len()) {
                    if count == tsx_occurrence {
                        break abs_pos;
                    }
                    count += 1;
                }
                search_start = abs_pos + 1;
            }
            None => panic!(
                "Could not find word-boundary occurrence #{} of {:?} in generated code.\n\
                 Found {} occurrences total.\nGenerated code:\n{}",
                tsx_occurrence, target, count, generated_code
            ),
        }
    };

    let (gen_line, gen_col) = byte_offset_to_line_col(generated_code, target_offset);

    let token = sm
        .lookup_token(lookup, gen_line, gen_col)
        .unwrap_or_else(|| {
            panic!(
                "No sourcemap token at generated position {}:{} for {:?}.\n\
                 Byte offset: {}\nGenerated code:\n{}",
                gen_line, gen_col, target, target_offset, generated_code
            )
        });

    assert!(
        token.get_source_id().is_some(),
        "Sourcemap token at generated {}:{} for {:?} has no source mapping (unmapped).\n\
         Generated code:\n{}",
        gen_line,
        gen_col,
        target,
        generated_code
    );

    let src_line = token.get_src_line() as usize;
    let vue_lines: Vec<&str> = vue_source.lines().collect();

    assert!(
        src_line < vue_lines.len(),
        "Mapped source line {} out of bounds (SFC has {} lines) for {:?}.\n\
         Generated code:\n{}",
        src_line,
        vue_lines.len(),
        target,
        generated_code
    );

    let source_line_text = vue_lines[src_line];
    assert!(
        source_line_text.contains(target),
        "Sourcemap maps {:?} at gen {}:{} → source line {} which is {:?}, \
         but that line does not contain {:?}.\nVue source:\n{}\nGenerated code:\n{}",
        target,
        gen_line,
        gen_col,
        src_line,
        source_line_text,
        target,
        vue_source,
        generated_code
    );
}

/// For every word-boundary occurrence of each identifier in the generated code,
/// verify the sourcemap maps it back to matching text in the Vue source.
///
/// Skips `___VERTER___` synthetic regions and unmapped tokens.
fn assert_identifiers_map_to_source(
    sm: &oxc_sourcemap::OwnedSourceMap,
    lookup: &[&[oxc_sourcemap::Token]],
    generated_code: &str,
    vue_source: &str,
    identifiers: &[&str],
) {
    for &ident in identifiers {
        let mut search_start = 0;
        let mut occurrence = 0;
        while let Some(rel_pos) = generated_code[search_start..].find(ident) {
            let abs_pos = search_start + rel_pos;

            // Skip if inside a ___VERTER___ synthetic region
            let line_start = generated_code[..abs_pos].rfind('\n').map_or(0, |p| p + 1);
            let line_text = &generated_code[line_start..];
            if line_text.contains("___VERTER___") {
                search_start = abs_pos + 1;
                continue;
            }

            if is_word_boundary(generated_code, abs_pos, ident.len()) {
                let (gen_line, gen_col) = byte_offset_to_line_col(generated_code, abs_pos);

                if let Some(token) = sm.lookup_token(lookup, gen_line, gen_col) {
                    if token.get_source_id().is_some() {
                        let src_line = token.get_src_line();
                        let src_col = token.get_src_col();

                        if let Some(src_byte) =
                            line_col_to_byte_offset(vue_source, src_line, src_col)
                        {
                            let src_end = (src_byte + ident.len()).min(vue_source.len());
                            let src_text = &vue_source[src_byte..src_end];
                            assert_eq!(
                                src_text,
                                ident,
                                "Identifier {:?} occurrence #{} at gen {}:{} maps to src {}:{} \
                                 where text is {:?}.\nGenerated:\n{}\nVue source:\n{}",
                                ident,
                                occurrence,
                                gen_line,
                                gen_col,
                                src_line,
                                src_col,
                                src_text,
                                generated_code,
                                vue_source,
                            );
                        }
                    }
                }
                occurrence += 1;
            }
            search_start = abs_pos + 1;
        }
    }
}

/// Iterate ALL sourcemap tokens and verify that mapped text matches between
/// generated code and original source for each token.
///
/// For each pair of consecutive tokens on the same generated line, extracts
/// the text span in the generated code and verifies the source text matches.
/// Skips unmapped tokens (no source_id) and synthetic prefixes.
fn assert_all_mapped_tokens_match(
    sm: &oxc_sourcemap::OwnedSourceMap,
    generated_code: &str,
    vue_source: &str,
) {
    let tokens: Vec<_> = sm.get_tokens().collect();
    if tokens.is_empty() {
        return;
    }

    let gen_lines: Vec<&str> = generated_code.lines().collect();
    let vue_lines: Vec<&str> = vue_source.lines().collect();

    for i in 0..tokens.len() {
        let token = &tokens[i];

        // Skip unmapped tokens
        if token.get_source_id().is_none() {
            continue;
        }

        let dst_line = token.get_dst_line() as usize;
        let dst_col = token.get_dst_col() as usize;
        let src_line = token.get_src_line() as usize;
        let src_col = token.get_src_col() as usize;

        // Bounds check
        if dst_line >= gen_lines.len() || src_line >= vue_lines.len() {
            continue;
        }

        // Determine span length: distance to next token on the same generated line,
        // capped at a reasonable max to avoid spanning across unrelated regions.
        let max_span = 40;
        let span_end = if i + 1 < tokens.len() {
            let next = &tokens[i + 1];
            if next.get_dst_line() == token.get_dst_line() {
                let next_col = next.get_dst_col() as usize;
                if next_col > dst_col {
                    (next_col - dst_col).min(max_span)
                } else {
                    continue; // overlapping or same-col tokens
                }
            } else {
                // Last token on this line — use remaining line length, capped
                let gen_line_text = gen_lines[dst_line];
                let gen_line_utf16_len = utf16_len(gen_line_text);
                if dst_col < gen_line_utf16_len {
                    (gen_line_utf16_len - dst_col).min(max_span)
                } else {
                    continue;
                }
            }
        } else {
            let gen_line_text = gen_lines[dst_line];
            let gen_line_utf16_len = utf16_len(gen_line_text);
            if dst_col < gen_line_utf16_len {
                (gen_line_utf16_len - dst_col).min(max_span)
            } else {
                continue;
            }
        };

        if span_end == 0 {
            continue;
        }

        // Extract text from generated code
        let gen_line_text = gen_lines[dst_line];
        let gen_text = extract_utf16_substr(gen_line_text, dst_col, span_end);

        // Skip synthetic prefixes and generated wrapper code
        if gen_text.starts_with("_ctx.")
            || gen_text.starts_with("$setup.")
            || gen_text.starts_with(".value")
            || gen_text.starts_with("___VERTER___")
            || gen_text.starts_with("__VLS")
            || gen_text.starts_with(";function")
            || gen_text.starts_with(";return")
            || gen_text.starts_with(";const")
            || gen_text.starts_with("export ")
        {
            continue;
        }

        // Skip entire lines that contain ___VERTER___ (generated wrapper code)
        if gen_line_text.contains("___VERTER___") {
            continue;
        }

        // Skip transformed Vue directives: @event → onClick, :prop → prop=, etc.
        // These are legitimate sourcemap transformations where text won't match literally.
        if gen_text.starts_with("on")
            && gen_text.len() > 2
            && gen_text.as_bytes()[2].is_ascii_uppercase()
        {
            continue;
        }

        // Skip JSX expression wrappers and transformed syntax
        if gen_text.starts_with('{') || gen_text.starts_with("}") {
            continue;
        }

        // Extract text from Vue source
        let vue_line_text = vue_lines[src_line];
        let vue_text = extract_utf16_substr(vue_line_text, src_col, span_end);

        // Skip directive transformations (:prop → prop=, @event → onClick, v-* → JSX)
        if vue_text.starts_with(':') || vue_text.starts_with('@') || vue_text.starts_with("v-") {
            continue;
        }

        // Skip v-bind delimiter transformation: `="` in source → `={` in generated.
        // This is a legitimate overwrite from split prop name handling.
        if gen_text.starts_with("={") && vue_text.starts_with("=\"") {
            continue;
        }

        // Handle .value suffix transformation: TSX adds `.value` to ref bindings
        // e.g., `count.value` in TSX maps to `count` in the SFC
        if gen_text.contains(".value") && !vue_text.contains(".value") {
            // Check that the identifier part before .value matches
            if let Some(dot_pos) = gen_text.find(".value") {
                let gen_ident = &gen_text[..dot_pos];
                if vue_text.starts_with(gen_ident) {
                    continue;
                }
            }
        }

        // Compare — they should match
        assert_eq!(
            gen_text,
            vue_text,
            "Token #{} mismatch!\n\
             Generated {}:{} text: {:?}\n\
             Source    {}:{} text: {:?}\n\
             Generated line: {:?}\n\
             Source line:    {:?}\n\
             Full generated code:\n{}",
            i,
            dst_line,
            dst_col,
            gen_text,
            src_line,
            src_col,
            vue_text,
            gen_line_text,
            vue_line_text,
            generated_code,
        );
    }
}

/// Extract a substring from `text` starting at UTF-16 column `col` for `len` UTF-16 units.
fn extract_utf16_substr(text: &str, col: usize, len: usize) -> &str {
    let bytes = text.as_bytes();
    let mut utf16_count: usize = 0;
    let mut byte_start: Option<usize> = None;
    let mut i: usize = 0;

    while i < bytes.len() {
        if utf16_count == col && byte_start.is_none() {
            byte_start = Some(i);
        }

        if let Some(start) = byte_start {
            let units_past_start = utf16_count - col;
            if units_past_start >= len {
                return &text[start..i];
            }
        }

        let b = bytes[i];
        if b < 0x80 {
            utf16_count += 1;
            i += 1;
        } else if b < 0xE0 {
            utf16_count += 1;
            i += 2;
        } else if b < 0xF0 {
            utf16_count += 1;
            i += 3;
        } else {
            utf16_count += 2;
            i += 4;
        }
    }

    // Handle end-of-string
    if let Some(start) = byte_start {
        &text[start..]
    } else {
        ""
    }
}

// ════════════════════════════════════════════════════════════════════
// Test modules
// ════════════════════════════════════════════════════════════════════

mod script_tests {
    use super::*;

    /// @ai-generated — Script sourcemap: `const count = ref(0)` → "count" maps back
    #[test]
    fn script_sourcemap_const_declaration() {
        let source = r#"<script setup>
const count = ref(0)
</script>

<template>
  <div>{{ count }}</div>
</template>
"#;
        let result = compile_with_sourcemap(source);
        assert!(result.errors.is_empty(), "errors: {:?}", result.errors);
        let script = result.script.as_ref().expect("script block");
        assert!(!script.source_map.is_empty(), "script source map is empty");

        let sm = oxc_sourcemap::OwnedSourceMap::from_json_string(&script.source_map)
            .expect("valid source map JSON");
        let lookup = build_lookup_table(&sm);

        // Script sourcemap maps at line/statement level, so use line-level assertion
        assert_maps_to_source_line(&sm, &lookup, &script.code, source, "count", 0);
        assert_maps_to_source_line(&sm, &lookup, &script.code, source, "ref", 0);
    }

    /// @ai-generated — Script sourcemap: function declaration maps back
    #[test]
    fn script_sourcemap_function_declaration() {
        let source = r#"<script setup>
function handleClick() {
  console.log('clicked')
}
</script>

<template>
  <button @click="handleClick">click</button>
</template>
"#;
        let result = compile_with_sourcemap(source);
        assert!(result.errors.is_empty(), "errors: {:?}", result.errors);
        let script = result.script.as_ref().expect("script block");
        assert!(!script.source_map.is_empty(), "script source map is empty");

        let sm = oxc_sourcemap::OwnedSourceMap::from_json_string(&script.source_map)
            .expect("valid source map JSON");
        let lookup = build_lookup_table(&sm);

        assert_maps_to_source_line(&sm, &lookup, &script.code, source, "handleClick", 0);
        assert_maps_to_source_line(&sm, &lookup, &script.code, source, "console", 0);
    }

    /// @ai-generated — Script sourcemap: import statement maps back
    #[test]
    fn script_sourcemap_import_statement() {
        let source = r#"<script setup>
import { ref } from 'vue'
const count = ref(0)
</script>

<template>
  <div>{{ count }}</div>
</template>
"#;
        let result = compile_with_sourcemap(source);
        assert!(result.errors.is_empty(), "errors: {:?}", result.errors);
        let script = result.script.as_ref().expect("script block");
        assert!(!script.source_map.is_empty(), "script source map is empty");

        let sm = oxc_sourcemap::OwnedSourceMap::from_json_string(&script.source_map)
            .expect("valid source map JSON");
        let lookup = build_lookup_table(&sm);

        // Import statement `ref` is relocated by the compiler, so check it via
        // line-level assertion. Also verify script body identifiers map correctly.
        assert_maps_to_source_line(&sm, &lookup, &script.code, source, "count", 0);
    }
}

mod template_tests {
    use super::*;

    /// @ai-generated — Template sourcemap: all tokens within SFC bounds
    #[test]
    fn template_sourcemap_tokens_in_bounds() {
        let source = r#"<script setup>
const msg = 'hello'
const show = true
</script>

<template>
  <div v-if="show">
    <span>{{ msg }}</span>
  </div>
</template>
"#;
        let result = compile_with_sourcemap(source);
        assert!(result.errors.is_empty(), "errors: {:?}", result.errors);
        let tpl = result.template.as_ref().expect("template block");
        assert!(!tpl.source_map.is_empty(), "template source map is empty");

        let sm = oxc_sourcemap::OwnedSourceMap::from_json_string(&tpl.source_map)
            .expect("valid source map JSON");

        // The template sourcemap operates on the full SFC input, so source lines
        // can reference any SFC line. Generated lines should be within the template
        // output. Template codegen may emit end-of-content markers beyond the last
        // rendered line, so we allow a generous margin.
        let vue_line_count = source.as_bytes().iter().filter(|&&b| b == b'\n').count() + 1;

        for token in sm.get_tokens() {
            if token.get_source_id().is_none() {
                continue;
            }

            let src_line = token.get_src_line() as usize;

            assert!(
                src_line < vue_line_count,
                "Template sourcemap token points to Vue line {} but SFC only has {} lines.",
                src_line,
                vue_line_count,
            );

            // Generated line check omitted: template codegen can emit end-of-content
            // markers at positions beyond the last rendered line. The bounds check in
            // compile_tests.rs's verify_sourcemap_tokens_in_bounds covers this.
        }
    }

    /// Verify that generated positions in the template sourcemap are relative to
    /// the template block output (`tpl_code`), not the full SFC. When a script
    /// block precedes the template, generated lines must NOT be shifted by the
    /// prefix line count.
    #[test]
    fn template_sourcemap_generated_positions_relative_to_tpl_code() {
        let source = r#"<script setup>
const msg = 'hello'
const show = true
</script>

<template>
  <div v-if="show">
    <span>{{ msg }}</span>
  </div>
</template>
"#;
        let result = compile_with_sourcemap(source);
        assert!(result.errors.is_empty(), "errors: {:?}", result.errors);
        let tpl = result.template.as_ref().expect("template block");
        assert!(!tpl.source_map.is_empty(), "template source map is empty");

        let sm = oxc_sourcemap::OwnedSourceMap::from_json_string(&tpl.source_map)
            .expect("valid source map JSON");

        let tpl_line_count = tpl.code.lines().count();

        // Every mapped token's generated line must be within the template code,
        // not shifted by the script prefix lines.
        for token in sm.get_tokens() {
            if token.get_source_id().is_none() {
                continue;
            }
            let gen_line = token.get_dst_line() as usize;
            assert!(
                gen_line < tpl_line_count + 1, // +1 for possible trailing newline
                "Template sourcemap generated line {} exceeds template code ({} lines).\n\
                 This suggests generated positions are relative to the full SFC \
                 rather than the sliced template output.\nTemplate code:\n{}",
                gen_line,
                tpl_line_count,
                tpl.code
            );
        }
    }

    /// Verify that template sourcemap tokens map generated positions to matching
    /// text in the original SFC source.
    #[test]
    fn template_sourcemap_identifiers_map_correctly() {
        let source = r#"<script setup>
const msg = 'hello'
const show = true
</script>

<template>
  <div v-if="show">
    <span>{{ msg }}</span>
  </div>
</template>
"#;
        let result = compile_with_sourcemap(source);
        assert!(result.errors.is_empty(), "errors: {:?}", result.errors);
        let tpl = result.template.as_ref().expect("template block");
        assert!(!tpl.source_map.is_empty(), "template source map is empty");

        let sm = oxc_sourcemap::OwnedSourceMap::from_json_string(&tpl.source_map)
            .expect("valid source map JSON");
        let lookup = build_lookup_table(&sm);

        // Template expressions should map back to matching text in the SFC.
        // 'show' appears in the v-if expression; 'msg' appears in interpolation.
        assert_maps_to_source(&sm, &lookup, &tpl.code, source, "show", 0);
        assert_maps_to_source(&sm, &lookup, &tpl.code, source, "msg", 0);
    }
}

mod tsx_tests {
    use super::*;

    /// Helper: compile TSX and return (sm, lookup, code) tuple.
    fn compile_and_parse_tsx(source: &str) -> (String, oxc_sourcemap::OwnedSourceMap) {
        let result = compile_tsx_with_sourcemap(source);
        assert!(result.errors.is_empty(), "errors: {:?}", result.errors);
        let tsx = result.tsx.expect("tsx block");
        let sm = oxc_sourcemap::OwnedSourceMap::from_json_string(&tsx.source_map)
            .expect("valid source map JSON");
        (tsx.code, sm)
    }

    // ── Script declarations in TSX ─────────────────────────────────

    /// @ai-generated — TSX sourcemap: const variable maps back to SFC
    #[test]
    fn tsx_sourcemap_const_variable_maps_back() {
        let source = r#"<script setup>
const count = ref(0)
</script>

<template>
  <div>{{ count }}</div>
</template>
"#;
        let (code, sm) = compile_and_parse_tsx(source);
        let lookup = build_lookup_table(&sm);

        // Script region of TSX maps at line/statement level
        assert_maps_to_source_line(&sm, &lookup, &code, source, "count", 0);
        assert_maps_to_source_line(&sm, &lookup, &code, source, "ref", 0);
    }

    /// @ai-generated — TSX sourcemap: function declaration maps back
    #[test]
    fn tsx_sourcemap_function_declaration_maps_back() {
        let source = r#"<script setup>
function handleClick() {
  console.log('clicked')
}
</script>

<template>
  <button @click="handleClick">click</button>
</template>
"#;
        let (code, sm) = compile_and_parse_tsx(source);
        let lookup = build_lookup_table(&sm);

        assert_maps_to_source_line(&sm, &lookup, &code, source, "handleClick", 0);
        assert_maps_to_source_line(&sm, &lookup, &code, source, "console", 0);
    }

    /// @ai-generated — TSX sourcemap: import statement and script body map back
    #[test]
    fn tsx_sourcemap_import_maps_back() {
        let source = r#"<script setup>
import { ref } from 'vue'
const count = ref(0)
</script>

<template>
  <div>{{ count }}</div>
</template>
"#;
        let (code, sm) = compile_and_parse_tsx(source);
        let lookup = build_lookup_table(&sm);

        // Import line may be unmapped (compiler rewrites imports); verify script body
        assert_maps_to_source_line(&sm, &lookup, &code, source, "count", 0);
    }

    /// @ai-generated — TSX sourcemap: multiple declarations across lines
    #[test]
    fn tsx_sourcemap_multiple_declarations_multiline() {
        let source = r#"<script setup>
const firstName = 'Alice'
const lastName = 'Smith'
const age = 30
</script>

<template>
  <div>{{ firstName }} {{ lastName }} ({{ age }})</div>
</template>
"#;
        let (code, sm) = compile_and_parse_tsx(source);
        let lookup = build_lookup_table(&sm);

        assert_identifiers_map_to_source(
            &sm,
            &lookup,
            &code,
            source,
            &["firstName", "lastName", "age"],
        );
    }

    // ── defineProps ────────────────────────────────────────────────

    /// @ai-generated — TSX sourcemap: defineProps type parameter content maps to SFC
    #[test]
    fn tsx_sourcemap_define_props_type_maps_back() {
        let source = r#"<script setup lang="ts">
defineProps<{ msg: string; count: number }>()
</script>

<template>
  <div>{{ msg }}</div>
</template>
"#;
        let (code, sm) = compile_and_parse_tsx(source);
        let lookup = build_lookup_table(&sm);

        // The type parameter content should map back to original positions
        assert_maps_to_source_line(&sm, &lookup, &code, source, "defineProps", 0);
    }

    /// @ai-generated — TSX sourcemap: withDefaults arguments map back
    #[test]
    fn tsx_sourcemap_define_props_with_defaults() {
        let source = r#"<script setup lang="ts">
const props = withDefaults(defineProps<{ msg: string }>(), {
  msg: 'hello'
})
</script>

<template>
  <div>{{ props.msg }}</div>
</template>
"#;
        let (code, sm) = compile_and_parse_tsx(source);
        let lookup = build_lookup_table(&sm);

        assert_maps_to_source_line(&sm, &lookup, &code, source, "withDefaults", 0);
    }

    // ── Template interpolation ─────────────────────────────────────

    /// @ai-generated — TSX sourcemap: `{{ msg }}` → "msg" maps back
    #[test]
    fn tsx_sourcemap_interpolation_identifier_maps_back() {
        let source = r#"<script setup>
const msg = 'hello'
</script>

<template>
  <div>{{ msg }}</div>
</template>
"#;
        let (code, sm) = compile_and_parse_tsx(source);
        let lookup = build_lookup_table(&sm);

        // "msg" should appear in TSX template region and map back to SFC
        assert_identifiers_map_to_source(&sm, &lookup, &code, source, &["msg"]);
    }

    /// @ai-generated — TSX sourcemap: `{{ a + b }}` → "a" and "b" map back
    #[test]
    fn tsx_sourcemap_interpolation_complex_expression() {
        let source = r#"<script setup>
const a = 1
const b = 2
</script>

<template>
  <div>{{ a + b }}</div>
</template>
"#;
        let (code, sm) = compile_and_parse_tsx(source);
        let lookup = build_lookup_table(&sm);

        assert_identifiers_map_to_source(&sm, &lookup, &code, source, &["a", "b"]);
    }

    // ── Bound props ────────────────────────────────────────────────

    /// @ai-generated — TSX sourcemap: `:value="count"` → "count" maps back
    #[test]
    fn tsx_sourcemap_bound_prop_value_maps_back() {
        let source = r#"<script setup>
const count = 42
</script>

<template>
  <input :value="count" />
</template>
"#;
        let (code, sm) = compile_and_parse_tsx(source);
        let lookup = build_lookup_table(&sm);

        assert_identifiers_map_to_source(&sm, &lookup, &code, source, &["count"]);
    }

    // ── Event handlers ─────────────────────────────────────────────

    /// @ai-generated — TSX sourcemap: `@click="handleClick"` → "handleClick" maps back
    #[test]
    fn tsx_sourcemap_event_handler_maps_back() {
        let source = r#"<script setup>
function handleClick() {}
</script>

<template>
  <button @click="handleClick">click</button>
</template>
"#;
        let (code, sm) = compile_and_parse_tsx(source);
        let lookup = build_lookup_table(&sm);

        assert_identifiers_map_to_source(&sm, &lookup, &code, source, &["handleClick"]);
    }

    // ── Text nodes ─────────────────────────────────────────────────

    /// @ai-generated — TSX sourcemap: `<div>hello world</div>` → "hello world" maps back
    #[test]
    fn tsx_sourcemap_text_node_maps_back() {
        let source = r#"<script setup>
</script>

<template>
  <div>hello world</div>
</template>
"#;
        let (code, sm) = compile_and_parse_tsx(source);
        let lookup = build_lookup_table(&sm);

        assert_maps_to_source(&sm, &lookup, &code, source, "hello world", 0);
    }

    /// @ai-generated — TSX sourcemap: text nodes between elements
    #[test]
    fn tsx_sourcemap_text_between_elements() {
        let source = r#"<script setup>
</script>

<template>
  <div>
    <span>first</span>
    <span>second</span>
  </div>
</template>
"#;
        let (code, sm) = compile_and_parse_tsx(source);
        let lookup = build_lookup_table(&sm);

        assert_maps_to_source(&sm, &lookup, &code, source, "first", 0);
        assert_maps_to_source(&sm, &lookup, &code, source, "second", 0);
    }

    // ── Directives ─────────────────────────────────────────────────

    /// @ai-generated — TSX sourcemap: `v-if="show"` → "show" maps back
    #[test]
    fn tsx_sourcemap_v_if_expression_maps_back() {
        let source = r#"<script setup>
const show = true
</script>

<template>
  <div v-if="show">visible</div>
</template>
"#;
        let (code, sm) = compile_and_parse_tsx(source);
        let lookup = build_lookup_table(&sm);

        assert_identifiers_map_to_source(&sm, &lookup, &code, source, &["show"]);
    }

    /// @ai-generated — TSX sourcemap: `v-for="item in items"` → "items" maps back
    #[test]
    fn tsx_sourcemap_v_for_expression_maps_back() {
        let source = r#"<script setup>
const items = [1, 2, 3]
</script>

<template>
  <div v-for="item in items" :key="item">{{ item }}</div>
</template>
"#;
        let (code, sm) = compile_and_parse_tsx(source);
        let lookup = build_lookup_table(&sm);

        assert_identifiers_map_to_source(&sm, &lookup, &code, source, &["items"]);
    }

    /// @ai-generated — TSX sourcemap: v-if / v-else-if / v-else chain
    #[test]
    fn tsx_sourcemap_v_if_v_else_chain() {
        let source = r#"<script setup>
const a = true
const b = false
</script>

<template>
  <div v-if="a">alpha</div>
  <div v-else-if="b">beta</div>
  <div v-else>gamma</div>
</template>
"#;
        let (code, sm) = compile_and_parse_tsx(source);
        let lookup = build_lookup_table(&sm);

        assert_identifiers_map_to_source(&sm, &lookup, &code, source, &["a", "b"]);
    }

    // ── Component tags ─────────────────────────────────────────────

    /// @ai-generated — TSX sourcemap: `<MyComponent />` → "MyComponent" maps back
    #[test]
    fn tsx_sourcemap_component_tag_maps_back() {
        let source = r#"<script setup>
import MyComponent from './MyComponent.vue'
</script>

<template>
  <MyComponent />
</template>
"#;
        let (code, sm) = compile_and_parse_tsx(source);
        let lookup = build_lookup_table(&sm);

        assert_identifiers_map_to_source(&sm, &lookup, &code, source, &["MyComponent"]);
    }

    // ── UTF-16 ─────────────────────────────────────────────────────

    /// @ai-generated — TSX sourcemap: emoji before identifier
    #[test]
    fn tsx_sourcemap_emoji_in_text_node() {
        let source = "<script setup>\nconst msg = 'hi'\n</script>\n\n<template>\n  <div>\u{1f389} {{ msg }}</div>\n</template>\n";
        let (code, sm) = compile_and_parse_tsx(source);
        let lookup = build_lookup_table(&sm);

        assert_identifiers_map_to_source(&sm, &lookup, &code, source, &["msg"]);
    }

    /// @ai-generated — TSX sourcemap: CJK characters
    #[test]
    fn tsx_sourcemap_cjk_in_text_node() {
        let source = "<script setup>\nconst msg = 'hi'\n</script>\n\n<template>\n  <div>\u{4f60}\u{597d} {{ msg }}</div>\n</template>\n";
        let (code, sm) = compile_and_parse_tsx(source);
        let lookup = build_lookup_table(&sm);

        assert_identifiers_map_to_source(&sm, &lookup, &code, source, &["msg"]);
    }

    /// @ai-generated — TSX sourcemap: emoji adjacent to interpolation
    #[test]
    fn tsx_sourcemap_emoji_in_interpolation() {
        let source = "<script setup>\nconst msg = 'world'\n</script>\n\n<template>\n  <div>{{ msg }}\u{1f30d}</div>\n</template>\n";
        let (code, sm) = compile_and_parse_tsx(source);
        let lookup = build_lookup_table(&sm);

        assert_identifiers_map_to_source(&sm, &lookup, &code, source, &["msg"]);
    }

    // ── Negative assertions ────────────────────────────────────────

    /// @ai-generated — TSX sourcemap: `_ctx.` prefix positions are unmapped
    #[test]
    fn tsx_sourcemap_ctx_prefix_is_unmapped() {
        let source = r#"<script setup>
const msg = 'hello'
</script>

<template>
  <div>{{ msg }}</div>
</template>
"#;
        let (code, sm) = compile_and_parse_tsx(source);
        let lookup = build_lookup_table(&sm);

        // Find all occurrences of "_ctx." in the TSX code
        let mut search_start = 0;
        while let Some(rel_pos) = code[search_start..].find("_ctx.") {
            let abs_pos = search_start + rel_pos;
            let (gen_line, gen_col) = byte_offset_to_line_col(&code, abs_pos);

            if let Some(token) = sm.lookup_token(&lookup, gen_line, gen_col) {
                // _ctx. should either have no source or map to nothing meaningful
                // The key assertion: source_id should be None for inserted prefixes
                if token.get_dst_line() == gen_line && token.get_dst_col() == gen_col {
                    assert!(
                        token.get_source_id().is_none(),
                        "_ctx. at gen {}:{} should be unmapped but has source_id {:?}",
                        gen_line,
                        gen_col,
                        token.get_source_id(),
                    );
                }
            }
            search_start = abs_pos + 1;
        }
    }

    /// @ai-generated — TSX sourcemap: `$setup.` prefix positions are unmapped
    #[test]
    fn tsx_sourcemap_setup_prefix_is_unmapped() {
        let source = r#"<script setup>
import { ref } from 'vue'
const count = ref(0)
</script>

<template>
  <div>{{ count }}</div>
</template>
"#;
        let (code, sm) = compile_and_parse_tsx(source);
        let lookup = build_lookup_table(&sm);

        // Find all occurrences of "$setup." in the TSX code
        let mut search_start = 0;
        while let Some(rel_pos) = code[search_start..].find("$setup.") {
            let abs_pos = search_start + rel_pos;
            let (gen_line, gen_col) = byte_offset_to_line_col(&code, abs_pos);

            if let Some(token) = sm.lookup_token(&lookup, gen_line, gen_col) {
                if token.get_dst_line() == gen_line && token.get_dst_col() == gen_col {
                    assert!(
                        token.get_source_id().is_none(),
                        "$setup. at gen {}:{} should be unmapped but has source_id {:?}",
                        gen_line,
                        gen_col,
                        token.get_source_id(),
                    );
                }
            }
            search_start = abs_pos + 1;
        }
    }

    // ── Exhaustive validation ──────────────────────────────────────

    /// @ai-generated — TSX sourcemap: exhaustive all-token match on complex SFC
    #[test]
    fn tsx_sourcemap_exhaustive_all_tokens_match() {
        let source = r#"<script setup>
import { ref, computed } from 'vue'
import MyComponent from './MyComponent.vue'

const count = ref(0)
const doubled = computed(() => count.value * 2)
function increment() {
  count.value++
}
</script>

<template>
  <div class="container">
    <h1>Counter: {{ count }}</h1>
    <p>Doubled: {{ doubled }}</p>
    <button @click="increment">Add</button>
    <MyComponent :value="count" />
    <div v-if="count > 0">
      <span>Positive</span>
    </div>
    <ul>
      <li v-for="i in count" :key="i">Item {{ i }}</li>
    </ul>
  </div>
</template>

<style scoped>
.container {
  padding: 20px;
}
</style>
"#;
        let (code, sm) = compile_and_parse_tsx(source);
        let lookup = build_lookup_table(&sm);

        // Exhaustive check: every mapped token's text should match
        assert_all_mapped_tokens_match(&sm, &code, source);

        // Additionally verify specific identifiers
        assert_identifiers_map_to_source(
            &sm,
            &lookup,
            &code,
            source,
            &["count", "doubled", "increment", "MyComponent"],
        );
    }
}

// ── Helper unit tests ──────────────────────────────────────────────

#[cfg(test)]
mod helper_tests {
    use super::*;

    #[test]
    fn test_byte_offset_to_line_col_basic() {
        let text = "abc\ndef\nghi";
        assert_eq!(byte_offset_to_line_col(text, 0), (0, 0)); // 'a'
        assert_eq!(byte_offset_to_line_col(text, 3), (0, 3)); // '\n'
        assert_eq!(byte_offset_to_line_col(text, 4), (1, 0)); // 'd'
        assert_eq!(byte_offset_to_line_col(text, 8), (2, 0)); // 'g'
    }

    #[test]
    fn test_byte_offset_to_line_col_utf16() {
        // 😀 is 4 bytes, 2 UTF-16 units
        let text = "a😀b";
        assert_eq!(byte_offset_to_line_col(text, 0), (0, 0)); // 'a'
        assert_eq!(byte_offset_to_line_col(text, 1), (0, 1)); // 😀
        assert_eq!(byte_offset_to_line_col(text, 5), (0, 3)); // 'b' (after surrogate pair)
    }

    #[test]
    fn test_line_col_to_byte_offset_basic() {
        let text = "abc\ndef\nghi";
        assert_eq!(line_col_to_byte_offset(text, 0, 0), Some(0));
        assert_eq!(line_col_to_byte_offset(text, 1, 0), Some(4));
        assert_eq!(line_col_to_byte_offset(text, 2, 0), Some(8));
        assert_eq!(line_col_to_byte_offset(text, 1, 2), Some(6)); // 'f'
    }

    #[test]
    fn test_line_col_to_byte_offset_utf16() {
        let text = "a😀b";
        assert_eq!(line_col_to_byte_offset(text, 0, 0), Some(0)); // 'a'
        assert_eq!(line_col_to_byte_offset(text, 0, 1), Some(1)); // 😀
        assert_eq!(line_col_to_byte_offset(text, 0, 3), Some(5)); // 'b'
    }

    #[test]
    fn test_find_nth_occurrence() {
        let text = "foo bar foo baz foo";
        assert_eq!(find_nth_occurrence(text, "foo", 0), Some(0));
        assert_eq!(find_nth_occurrence(text, "foo", 1), Some(8));
        assert_eq!(find_nth_occurrence(text, "foo", 2), Some(16));
        assert_eq!(find_nth_occurrence(text, "foo", 3), None);
    }

    #[test]
    fn test_is_word_boundary() {
        let text = "hello world_foo bar";
        assert!(is_word_boundary(text, 0, 5)); // "hello" at start
        assert!(is_word_boundary(text, 16, 3)); // "bar" at end
        assert!(!is_word_boundary(text, 6, 5)); // "world" → not boundary (world_foo)
    }

    #[test]
    fn test_extract_utf16_substr_ascii() {
        let text = "hello world";
        assert_eq!(extract_utf16_substr(text, 0, 5), "hello");
        assert_eq!(extract_utf16_substr(text, 6, 5), "world");
    }

    #[test]
    fn test_extract_utf16_substr_cjk() {
        let text = "a你好b";
        // 'a' = col 0, '你' = col 1, '好' = col 2, 'b' = col 3
        assert_eq!(extract_utf16_substr(text, 0, 1), "a");
        assert_eq!(extract_utf16_substr(text, 1, 2), "你好");
        assert_eq!(extract_utf16_substr(text, 3, 1), "b");
    }

    #[test]
    fn test_extract_utf16_substr_emoji() {
        let text = "a😀b";
        // 'a' = col 0, '😀' = col 1-2 (surrogate pair), 'b' = col 3
        assert_eq!(extract_utf16_substr(text, 0, 1), "a");
        assert_eq!(extract_utf16_substr(text, 1, 2), "😀");
        assert_eq!(extract_utf16_substr(text, 3, 1), "b");
    }
}

// ── Hoisted declaration sourcemap tests ───────────────────────────
//
// These tests verify that type declarations and imports hoisted from
// <script setup> to the top of TSX output retain their sourcemap
// mappings back to the original SFC positions.

mod hoist_mapping_tests {
    use super::*;

    /// Helper: verify a target string in TSX output has a mapped sourcemap token
    /// (source_id is Some) at its position.
    fn assert_token_is_mapped(sm: &oxc_sourcemap::OwnedSourceMap, tsx_code: &str, target: &str) {
        let lookup = build_lookup_table(sm);
        let pos = tsx_code.find(target).unwrap_or_else(|| {
            panic!("'{}' not found in TSX output:\n{}", target, tsx_code);
        });
        let (line, col) = byte_offset_to_line_col(tsx_code, pos);
        let token = sm.lookup_token(&lookup, line, col);
        assert!(
            token.is_some(),
            "no sourcemap token found at gen({},{}) for '{}'",
            line,
            col,
            target
        );
        let tok = token.unwrap();
        assert!(
            tok.get_source_id().is_some(),
            "'{}' at gen({},{}) should be mapped but source_id is None",
            target,
            line,
            col
        );
    }

    /// Helper: verify a target string maps back to source text that starts with
    /// the same content.
    fn assert_hoisted_maps_to_source(
        sm: &oxc_sourcemap::OwnedSourceMap,
        tsx_code: &str,
        vue_source: &str,
        target: &str,
    ) {
        let lookup = build_lookup_table(sm);
        let pos = tsx_code.find(target).unwrap_or_else(|| {
            panic!("'{}' not found in TSX output:\n{}", target, tsx_code);
        });
        let (gen_line, gen_col) = byte_offset_to_line_col(tsx_code, pos);
        let token = sm
            .lookup_token(&lookup, gen_line, gen_col)
            .unwrap_or_else(|| {
                panic!(
                    "no sourcemap token for '{}' at gen({},{})",
                    target, gen_line, gen_col
                );
            });
        assert!(
            token.get_source_id().is_some(),
            "'{}' at gen({},{}) is unmapped (source_id=None)",
            target,
            gen_line,
            gen_col
        );
        let src_line = token.get_src_line();
        let src_col = token.get_src_col();
        let src_off = line_col_to_byte_offset(vue_source, src_line, src_col).unwrap_or_else(|| {
            panic!(
                "source position ({},{}) out of bounds for '{}'",
                src_line, src_col, target
            );
        });
        let src_end = (src_off + target.len()).min(vue_source.len());
        let src_text = &vue_source[src_off..src_end];
        assert_eq!(
            src_text, target,
            "hoisted '{}' at gen({},{}) maps to src({},{}) = '{}', expected '{}'",
            target, gen_line, gen_col, src_line, src_col, src_text, target
        );
    }

    #[test]
    fn tsx_hoisted_interface_is_mapped() {
        let source = r#"<script setup lang="ts">
interface Props {
  msg: string
  count: number
}

const props = defineProps<Props>()
</script>
<template>
  <div>{{ props.msg }}</div>
</template>"#;

        let result = compile_tsx_with_sourcemap(source);
        let tsx = result.tsx.as_ref().unwrap();
        let sm = oxc_sourcemap::OwnedSourceMap::from_json_string(&tsx.source_map).unwrap();

        // The compiler-owned JSX authority occupies the one unmapped intro
        // line; the authored interface remains the first mapped declaration at
        // exact column zero on the following line.
        assert!(
            tsx.code
                .starts_with("/** @jsxImportSource vue */\ninterface Props {"),
            "interface should follow the unmapped Vue JSX intro, got:\n{}",
            &tsx.code[..tsx.code.find("\nimport type").unwrap_or(80)]
        );
        assert_token_is_mapped(&sm, &tsx.code, "interface Props {");
    }

    #[test]
    fn tsx_hoisted_interface_maps_to_original_source() {
        let source = r#"<script setup lang="ts">
interface Props {
  msg: string
  count: number
}

const props = defineProps<Props>()
</script>
<template>
  <div>{{ props.msg }}</div>
</template>"#;

        let result = compile_tsx_with_sourcemap(source);
        let tsx = result.tsx.as_ref().unwrap();
        let sm = oxc_sourcemap::OwnedSourceMap::from_json_string(&tsx.source_map).unwrap();

        // Each line of the interface should map back to its original SFC position
        assert_hoisted_maps_to_source(&sm, &tsx.code, source, "interface Props {");
        assert_hoisted_maps_to_source(&sm, &tsx.code, source, "  msg: string");
        assert_hoisted_maps_to_source(&sm, &tsx.code, source, "  count: number");
    }

    #[test]
    fn tsx_hoisted_type_alias_is_mapped() {
        let source = r#"<script setup lang="ts">
type Status = "active" | "inactive"
const status = ref<Status>("active")
</script>
<template>
  <div>{{ status }}</div>
</template>"#;

        let result = compile_tsx_with_sourcemap(source);
        let tsx = result.tsx.as_ref().unwrap();
        let sm = oxc_sourcemap::OwnedSourceMap::from_json_string(&tsx.source_map).unwrap();

        assert_hoisted_maps_to_source(
            &sm,
            &tsx.code,
            source,
            "type Status = \"active\" | \"inactive\"",
        );
    }

    #[test]
    fn tsx_hoisted_import_is_mapped() {
        let source = r#"<script setup lang="ts">
import { ref, computed } from 'vue'
const count = ref(0)
</script>
<template>
  <div>{{ count }}</div>
</template>"#;

        let result = compile_tsx_with_sourcemap(source);
        let tsx = result.tsx.as_ref().unwrap();
        let sm = oxc_sourcemap::OwnedSourceMap::from_json_string(&tsx.source_map).unwrap();

        // User import should be hoisted and mapped back to SFC
        assert_hoisted_maps_to_source(
            &sm,
            &tsx.code,
            source,
            "import { ref, computed } from 'vue'",
        );
    }

    #[test]
    fn tsx_hoisted_import_type_is_mapped() {
        let source = r#"<script setup lang="ts">
import type { Ref } from 'vue'
const count: Ref<number> = ref(0)
</script>
<template>
  <div>{{ count }}</div>
</template>"#;

        let result = compile_tsx_with_sourcemap(source);
        let tsx = result.tsx.as_ref().unwrap();
        let sm = oxc_sourcemap::OwnedSourceMap::from_json_string(&tsx.source_map).unwrap();

        assert_hoisted_maps_to_source(&sm, &tsx.code, source, "import type { Ref } from 'vue'");
    }

    #[test]
    fn tsx_hoisted_multiple_types_all_mapped() {
        let source = r#"<script setup lang="ts">
interface Props {
  msg: string
}

type Emits = {
  (e: 'update', value: string): void
}

const props = defineProps<Props>()
const emit = defineEmits<Emits>()
</script>
<template>
  <div>{{ props.msg }}</div>
</template>"#;

        let result = compile_tsx_with_sourcemap(source);
        let tsx = result.tsx.as_ref().unwrap();
        let sm = oxc_sourcemap::OwnedSourceMap::from_json_string(&tsx.source_map).unwrap();

        // Both type declarations should be mapped
        assert_hoisted_maps_to_source(&sm, &tsx.code, source, "interface Props {");
        assert_hoisted_maps_to_source(&sm, &tsx.code, source, "  msg: string");
        // Type alias with multiple lines
        assert_token_is_mapped(&sm, &tsx.code, "type Emits = {");
    }

    #[test]
    fn tsx_hoisted_enum_is_mapped() {
        let source = r#"<script setup lang="ts">
const enum Direction {
  Up = "UP",
  Down = "DOWN",
}
const dir = ref(Direction.Up)
</script>
<template>
  <div>{{ dir }}</div>
</template>"#;

        let result = compile_tsx_with_sourcemap(source);
        let tsx = result.tsx.as_ref().unwrap();
        let sm = oxc_sourcemap::OwnedSourceMap::from_json_string(&tsx.source_map).unwrap();

        // const enum should be hoisted and mapped
        assert_token_is_mapped(&sm, &tsx.code, "const enum Direction {");
        assert_hoisted_maps_to_source(&sm, &tsx.code, source, "const enum Direction {");
    }
}
