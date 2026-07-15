//! Reusable framework IDE sourcemap end-to-end assertion helpers.
//!
//! Cloned from the shape of the compiler's `sourcemap_e2e_tests.rs`
//! token-maps-back assertions, but framework-NEUTRAL: they operate on an
//! `(generated_code, source_map_json)` pair plus the original carrier
//! source, so EVERY carrier vertical (Vue today; Svelte / React / Astro
//! later) re-runs the SAME e2e correctness assertions against its own
//! [`CarrierCompiler::compile_ide`](super::CarrierCompiler::compile_ide)
//! output. A token that maps to mismatched source text is the bug class
//! these helpers catch.
//!
//! Test-only: gated behind `#[cfg(test)]` so the helpers never ship in a
//! release artifact, but `pub` so a later vertical's `#[cfg(test)]`
//! module reaches them.

#![allow(dead_code)]

use super::carrier_compiler::IdeOutput;

/// Parse an [`IdeOutput`]'s `(code, source_map)` into a code string + a
/// parsed `OwnedSourceMap` ready for token lookup.
pub fn parse_ide_output(ide: &IdeOutput) -> (String, oxc_sourcemap::OwnedSourceMap) {
    let sm = oxc_sourcemap::OwnedSourceMap::from_json_string(&ide.source_map)
        .expect("compile_ide must emit a valid source-map JSON string");
    (ide.code.clone(), sm)
}

/// UTF-16 length of a `&str` (the column unit source maps use).
pub fn utf16_len(s: &str) -> usize {
    s.chars().map(char::len_utf16).sum()
}

/// Convert a byte offset into 0-based (line, UTF-16 column).
pub fn byte_offset_to_line_col(text: &str, byte_offset: usize) -> (u32, u32) {
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

/// Convert 0-based (line, UTF-16 column) to a byte offset, or `None` if
/// out of bounds.
pub fn line_col_to_byte_offset(text: &str, line: u32, col: u32) -> Option<usize> {
    let mut current_line: u32 = 0;
    let mut line_start: usize = 0;
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
            return None;
        }
    }
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
            utf16_count += 2;
            i += 4;
        }
    }
    if utf16_count == col {
        return Some(line_start + i);
    }
    None
}

fn is_word_char(b: u8) -> bool {
    b.is_ascii_alphanumeric() || b == b'_' || b == b'$'
}

/// Whether the occurrence at `pos` with length `len` sits on word
/// boundaries.
pub fn is_word_boundary(text: &str, pos: usize, len: usize) -> bool {
    let bytes = text.as_bytes();
    let before_ok = pos == 0 || !is_word_char(bytes[pos - 1]);
    let after_ok = pos + len >= bytes.len() || !is_word_char(bytes[pos + len]);
    before_ok && after_ok
}

/// Build a lookup table for `lookup_token`.
pub fn build_lookup_table(sm: &oxc_sourcemap::OwnedSourceMap) -> Vec<&[oxc_sourcemap::Token]> {
    sm.generate_lookup_table()
}

/// Assert that the `occurrence`-th word-boundary occurrence of `target` in
/// the generated IDE code maps back, via the source map, to matching text
/// in the original carrier `source`.
///
/// The reusable e2e correctness assertion every carrier vertical re-runs:
/// it fails when a token maps to the WRONG source text (the sourcemap-
/// accuracy bug class), not merely when a position is in bounds.
pub fn assert_token_maps_to_source(
    sm: &oxc_sourcemap::OwnedSourceMap,
    lookup: &[&[oxc_sourcemap::Token]],
    generated_code: &str,
    source: &str,
    target: &str,
    occurrence: usize,
) {
    let mut count = 0;
    let mut search_start = 0;
    let target_offset = loop {
        match generated_code[search_start..].find(target) {
            Some(rel_pos) => {
                let abs_pos = search_start + rel_pos;
                if is_word_boundary(generated_code, abs_pos, target.len()) {
                    if count == occurrence {
                        break abs_pos;
                    }
                    count += 1;
                }
                search_start = abs_pos + 1;
            }
            None => panic!(
                "could not find word-boundary occurrence #{occurrence} of {target:?} \
                 in generated code (found {count}). Generated:\n{generated_code}"
            ),
        }
    };

    let (gen_line, gen_col) = byte_offset_to_line_col(generated_code, target_offset);
    let token = sm
        .lookup_token(lookup, gen_line, gen_col)
        .unwrap_or_else(|| {
            panic!("no source-map token at generated {gen_line}:{gen_col} for {target:?}")
        });
    assert!(
        token.get_source_id().is_some(),
        "token at generated {gen_line}:{gen_col} for {target:?} is unmapped"
    );

    let (src_line, src_col) = (token.get_src_line(), token.get_src_col());
    let src_byte_offset = line_col_to_byte_offset(source, src_line, src_col).unwrap_or_else(|| {
        panic!("token maps {target:?} → src {src_line}:{src_col}, out of bounds in carrier source")
    });
    let src_end = (src_byte_offset + target.len()).min(source.len());
    let src_text = &source[src_byte_offset..src_end];
    assert_eq!(
        src_text, target,
        "sourcemap mismatch for {target:?}: gen {gen_line}:{gen_col} → src {src_line}:{src_col} \
         is {src_text:?}, expected {target:?}"
    );
}

/// Like [`assert_token_maps_to_source`] but with LINE granularity:
/// asserts `target` appears somewhere on the mapped source LINE rather
/// than at the exact column. Use for script-region tokens whose source
/// map maps at statement/line level rather than per-identifier.
pub fn assert_token_maps_to_source_line(
    sm: &oxc_sourcemap::OwnedSourceMap,
    lookup: &[&[oxc_sourcemap::Token]],
    generated_code: &str,
    source: &str,
    target: &str,
    occurrence: usize,
) {
    let mut count = 0;
    let mut search_start = 0;
    let target_offset = loop {
        match generated_code[search_start..].find(target) {
            Some(rel_pos) => {
                let abs_pos = search_start + rel_pos;
                if is_word_boundary(generated_code, abs_pos, target.len()) {
                    if count == occurrence {
                        break abs_pos;
                    }
                    count += 1;
                }
                search_start = abs_pos + 1;
            }
            None => panic!(
                "could not find word-boundary occurrence #{occurrence} of {target:?} \
                 in generated code (found {count})"
            ),
        }
    };
    let (gen_line, gen_col) = byte_offset_to_line_col(generated_code, target_offset);
    let token = sm
        .lookup_token(lookup, gen_line, gen_col)
        .unwrap_or_else(|| {
            panic!("no source-map token at generated {gen_line}:{gen_col} for {target:?}")
        });
    assert!(
        token.get_source_id().is_some(),
        "token at generated {gen_line}:{gen_col} for {target:?} is unmapped"
    );
    let src_line = token.get_src_line() as usize;
    let source_lines: Vec<&str> = source.lines().collect();
    assert!(
        src_line < source_lines.len(),
        "mapped source line {src_line} out of bounds (carrier has {} lines) for {target:?}",
        source_lines.len()
    );
    assert!(
        source_lines[src_line].contains(target),
        "token maps {target:?} → source line {src_line} ({:?}), which does not contain {target:?}",
        source_lines[src_line]
    );
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::framework_common::carrier_compiler::{
        CarrierCompiler, IdeCompileOptions, ParseOptions,
    };
    use crate::framework_common::vue_bridge::VueCarrierCompiler;

    #[test]
    fn position_conversions_round_trip() {
        let text = "abc\ndéf\nghi";
        // The 'g' on line 2, col 0.
        let off = line_col_to_byte_offset(text, 2, 0).unwrap();
        assert_eq!(&text[off..off + 1], "g");
        let (l, c) = byte_offset_to_line_col(text, off);
        assert_eq!((l, c), (2, 0));
    }

    #[test]
    fn helpers_assert_vue_ide_tokens_map_back_to_the_sfc() {
        // The reusable helpers exercised end-to-end against the Vue
        // bridge's IDE output — proving they are live, not a dead clone.
        let compiler = VueCarrierCompiler::default();
        let source =
            "<script setup lang=\"ts\">\nconst myUniqueBinding = 1\n</script>\n<template><div>{{ myUniqueBinding }}</div></template>";
        let artifact = compiler.parse(source, &ParseOptions::default());
        let ide = compiler
            .compile_ide(
                source,
                &artifact,
                &IdeCompileOptions {
                    filename: Some("App.vue".to_string()),
                    ..Default::default()
                },
            )
            .expect("Vue TS SFC compiles to an IDE artifact");
        let (code, sm) = parse_ide_output(&ide);
        let lookup = build_lookup_table(&sm);
        // The script binding maps back to its declaration LINE (script
        // regions map at statement/line granularity).
        assert_token_maps_to_source_line(&sm, &lookup, &code, source, "myUniqueBinding", 0);
    }

    #[test]
    fn column_precise_assertion_catches_a_mismapped_token() {
        // A synthetic source map exercises the COLUMN-precise assertion
        // independent of any framework codegen: a token whose generated
        // position maps to the WRONG source column must be caught.
        //
        // generated `let v = 1;` ; source `const value = 1;`. The token at
        // generated col 4 (`v`) maps to source col 6 (`value`). A correct
        // map (gen col 4 → src col 6) passes for `v`→`value`? No — text
        // differs. We instead build a faithful identity map and assert it
        // passes, then a shifted map and assert it FAILS — proving the
        // helper discriminates on source TEXT, not just bounds.
        let source = "const value = 1;";
        let generated = "const value = 1;";

        // Faithful identity map: gen (0,6) → src (0,6) for `value`.
        let good = make_sourcemap(&[(0, 6, 0, 6)], source, generated);
        let lookup = build_lookup_table(&good);
        assert_token_maps_to_source(&good, &lookup, generated, source, "value", 0);

        // Shifted map: gen (0,6) → src (0,0) (`const`, not `value`).
        let bad = make_sourcemap(&[(0, 6, 0, 0)], source, generated);
        let bad_lookup = build_lookup_table(&bad);
        let result = std::panic::catch_unwind(|| {
            assert_token_maps_to_source(&bad, &bad_lookup, generated, source, "value", 0);
        });
        assert!(
            result.is_err(),
            "a token mapping `value` to the wrong source column must be caught"
        );
    }

    /// Build a minimal `OwnedSourceMap` from `(gen_line, gen_col,
    /// src_line, src_col)` tokens over one source file.
    fn make_sourcemap(
        tokens: &[(u32, u32, u32, u32)],
        source: &str,
        _generated: &str,
    ) -> oxc_sourcemap::OwnedSourceMap {
        let mut builder = oxc_sourcemap::SourceMapBuilder::default();
        let src_id = builder.set_source_and_content("app.vue", source);
        for &(gl, gc, sl, sc) in tokens {
            builder.add_token(gl, gc, sl, sc, Some(src_id), None);
        }
        let json = builder.into_sourcemap().to_json_string();
        oxc_sourcemap::OwnedSourceMap::from_json_string(&json).expect("valid synthetic source map")
    }
}
