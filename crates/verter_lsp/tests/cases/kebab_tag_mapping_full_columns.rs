//! Kebab component-tag rewrite: EVERY letter column of the authored tag name —
//! including the LAST column — must map into the generated TSX.
//!
//! A rewritten kebab tag (`<global-count-comp>` → `<GlobalCountComp>`) used to be
//! ONE whole-name overwritten chunk: the source map emitted a single token at the
//! name start whose reverse run capped at the GENERATED (Pascal) length, so the
//! authored columns past the Pascal length — the tag TAIL (`…om`**`p`** plus the
//! dash savings) — mapped to nothing and hover/definition/rename went dead there.
//! The per-segment rewrite keeps every unchanged byte an `Original` chunk, so the
//! production `PositionMapper` resolves every LETTER column; only the removed `-`
//! separators (deleted bytes with no generated correlate) stay unmapped.
//!
//! Discriminating: against the whole-name-overwrite emission, the last-column
//! probes below return `None` and this test FAILS.

use oxc_allocator::Allocator;
use verter_compiler::compile::{compile, CodegenOptions, CompileTarget, VerterCompileOptions};
use verter_lsp::documents::position_map::PositionMapper;
use verter_span::LspPosition;

/// Compile one SFC through the production IDE/TSX lane and return
/// `(tsx_code, PositionMapper)`.
fn compile_to_mapper(source: &str) -> (String, PositionMapper) {
    let alloc = Allocator::default();
    let options = CodegenOptions {
        filename: Some("App.vue".to_string()),
        target: CompileTarget::BUNDLER | CompileTarget::TSX,
        ..Default::default()
    };
    let verter_opts = VerterCompileOptions {
        source_map: true,
        ..Default::default()
    };
    let result = compile(source, &options, &verter_opts, &alloc);
    assert!(result.errors.is_empty(), "errors: {:?}", result.errors);
    let tsx = result.tsx.as_ref().expect("tsx block");
    let mapper = PositionMapper::from_json(&tsx.source_map).expect("valid TSX source map");
    (tsx.code.clone(), mapper)
}

/// The (line, first-column) of `needle`'s first occurrence in `source`
/// (0-indexed UTF-16-safe for ASCII fixtures).
fn line_col_of(source: &str, needle: &str) -> (u32, u32) {
    let off = source.find(needle).expect("needle present");
    let line = source[..off].matches('\n').count() as u32;
    let col = (off - source[..off].rfind('\n').map(|p| p + 1).unwrap_or(0)) as u32;
    (line, col)
}

/// A GLOBAL kebab tag (no local binding — GlobalComponents fallback const):
/// every letter column of the authored open-tag name maps; only the `-`
/// separators are unmapped; the LAST column (the previous dead-tail probe)
/// maps and lands inside the generated Pascal identifier.
#[test]
fn global_kebab_tag_maps_every_letter_column_including_last() {
    let source = "<template>\n  <global-count-comp :count=\"7\" />\n</template>\n";
    let (tsx, mapper) = compile_to_mapper(source);
    assert!(
        tsx.contains("<GlobalCountComp"),
        "kebab tag must rewrite to the Pascal const: {tsx}"
    );

    let tag_name = "global-count-comp";
    let (line, name_col) = line_col_of(source, tag_name);

    let mut mapped_letters = 0usize;
    for (i, ch) in tag_name.char_indices() {
        let col = name_col + i as u32;
        let mapped = mapper.carrier_to_tsx(LspPosition::new(line, col));
        if ch == '-' {
            assert!(
                mapped.is_none(),
                "the removed `-` separator at col {col} has no generated correlate \
                 and must stay unmapped (fail-closed), got: {mapped:?}"
            );
        } else {
            assert!(
                mapped.is_some(),
                "letter {ch:?} at authored col {col} of the kebab tag must map \
                 into the generated TSX (dead tail-zone regression), tsx:\n{tsx}"
            );
            mapped_letters += 1;
        }
    }
    assert_eq!(
        mapped_letters,
        tag_name.chars().filter(|c| *c != '-').count(),
        "every letter of the tag name must have mapped"
    );

    // The LAST column is the discriminating probe: under the whole-name
    // overwrite emission the reverse run capped at the generated Pascal length
    // (15 < 17), so this column mapped to None.
    let last_col = name_col + (tag_name.len() as u32) - 1;
    let last = mapper
        .carrier_to_tsx(LspPosition::new(line, last_col))
        .expect("LAST authored column of the kebab tag name must map");
    // ... and it must land INSIDE the generated Pascal identifier, not past it.
    let gen_line_start = tsx
        .split('\n')
        .take(last.pos.line as usize)
        .map(|l| l.len() + 1)
        .sum::<usize>();
    let gen_line = &tsx[gen_line_start..];
    let pascal_col = gen_line.find("<GlobalCountComp").expect("tag on line") + 1;
    let pascal_end = pascal_col + "GlobalCountComp".len();
    assert!(
        (last.pos.character as usize) >= pascal_col && (last.pos.character as usize) < pascal_end,
        "last authored column must map inside the generated identifier \
         [{pascal_col}, {pascal_end}), got col {}",
        last.pos.character
    );
}

/// A kebab tag bound to a LOCAL import rewrites through the same per-segment
/// machinery: the letters (including the last column) map; the separators do
/// not.
#[test]
fn local_binding_kebab_tag_maps_every_letter_column() {
    let source = "<script setup lang=\"ts\">\nimport MyLongWidget from './MyLongWidget.vue'\n</script>\n<template>\n  <my-long-widget />\n</template>\n";
    let (tsx, mapper) = compile_to_mapper(source);
    assert!(
        tsx.contains("<MyLongWidget"),
        "kebab tag must rewrite to the local binding: {tsx}"
    );

    let tag_name = "my-long-widget";
    let (line, name_col) = line_col_of(source, tag_name);
    for (i, ch) in tag_name.char_indices() {
        if ch == '-' {
            continue;
        }
        let col = name_col + i as u32;
        assert!(
            mapper.carrier_to_tsx(LspPosition::new(line, col)).is_some(),
            "letter {ch:?} at authored col {col} of the local-binding kebab tag must map"
        );
    }
    let last_col = name_col + (tag_name.len() as u32) - 1;
    assert!(
        mapper
            .carrier_to_tsx(LspPosition::new(line, last_col))
            .is_some(),
        "LAST authored column of the local-binding kebab tag must map"
    );
}

/// The isolated-body path (`<el-button><span>x</span></el-button>`) rewrites the
/// OPEN tag per-segment too: its last column maps.
#[test]
fn kebab_tag_with_isolated_body_maps_open_tag_tail() {
    let source =
        "<template>\n  <global-count-comp><span>x</span></global-count-comp>\n</template>\n";
    let (tsx, mapper) = compile_to_mapper(source);
    assert!(
        tsx.contains("<GlobalCountComp"),
        "kebab open tag must rewrite: {tsx}"
    );
    let tag_name = "global-count-comp";
    let (line, name_col) = line_col_of(source, tag_name);
    let last_col = name_col + (tag_name.len() as u32) - 1;
    assert!(
        mapper
            .carrier_to_tsx(LspPosition::new(line, last_col))
            .is_some(),
        "LAST authored column of the isolated-body kebab open tag must map"
    );
}
