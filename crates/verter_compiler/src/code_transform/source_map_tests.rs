use super::*;
use oxc_allocator::Allocator;

/// Byte-identical regression guard: the resolver-reuse and reserved-token-vector
/// changes are allocation-only and must not alter emitted source-map bytes.
/// The expected string is the JSON produced for this representative case
/// (overwrite + a mapped insertion across a multi-line source).
#[test]
fn source_map_json_is_byte_identical_for_representative_case() {
    let allocator = Allocator::default();
    let source = "const x = 1;\nconst y = 2;\nconst z = 3;";
    let mut ct = CodeTransform::new(source, &allocator);
    ct.overwrite(6, 7, "foo");
    ct.batch_prepend_left_with_source_map(&[(13, Some((6, 0)), "(mapped) ")]);
    let json = ct.generate_map_json(
        SourceMapOptions::new()
            .with_source("golden.ts")
            .with_file("golden.ts.map"),
    );
    assert_eq!(
        json,
        "{\"version\":3,\"file\":\"golden.ts.map\",\"names\":[],\"sources\":[\"golden.ts\"],\"sourcesContent\":[\"const x = 1;\\nconst y = 2;\\nconst z = 3;\"],\"mappings\":\"AAAA,MAAM,GAAC;AAAD,SACN;AACA\"}"
    );
}

/// One `PositionResolver` is built lazily on first map demand and reused across
/// every subsequent map for the same original source — never rebuilt per map.
/// Discriminates the per-map-rebuild path.
#[test]
fn sourcemap_resolver_is_built_once_and_reused_across_maps() {
    let allocator = Allocator::default();
    let source = "abc\ndef\nghi\njkl";
    let mut ct = CodeTransform::new(source, &allocator);
    ct.overwrite(4, 7, "XYZ");

    // The resolver is lazily built — it must not exist until the first map.
    assert!(
        ct.sourcemap_resolver_for_test().is_none(),
        "resolver must be lazily built on demand, not constructed eagerly"
    );

    let _ = ct.generate_map(SourceMapOptions::new().with_source("a.js"));
    let first = ct
        .sourcemap_resolver_for_test()
        .expect("first map must build and cache the resolver") as *const _;

    let _ = ct.generate_map(SourceMapOptions::new().with_source("a.js"));
    let second = ct
        .sourcemap_resolver_for_test()
        .expect("resolver must remain cached after the second map") as *const _;

    assert!(
        std::ptr::eq(first, second),
        "the SAME PositionResolver must be reused across maps, not rebuilt per map"
    );
}

/// The source-map token buffer is reserved up front to a true upper bound on
/// the emitted token count, so `generate_map` never reallocates it during
/// population. Proven directly against the production map path: the capacity
/// captured at the reservation point must cover the final token count, across
/// inputs whose token totals are dominated by different chunk kinds — including
/// a moved overwrite whose replacement text injects newlines absent from the
/// original source.
///
/// Discriminates three regressions at once: dropping the `Vec::with_capacity`
/// reservation (the captured capacity collapses to 0); an estimate that counts
/// only original-source newlines (it under-reserves the moved-overwrite
/// fixture); and a constant estimate (it cannot cover both fixtures, whose
/// token totals differ).
#[test]
fn sourcemap_token_buffer_reservation_covers_every_emitted_token() {
    let allocator = Allocator::default();

    // Fixture 1 — multi-line source with an overwrite and a mapped insertion:
    // the token total is dominated by Original chunks and original-source
    // newlines.
    {
        let source = "line1\nline2\nline3\nline4\nline5";
        let mut ct = CodeTransform::new(source, &allocator);
        ct.overwrite(6, 11, "XXXXX"); // overwrite "line2"
        ct.batch_prepend_left_with_source_map(&[(18, Some((0, 3)), "(pfx)mapped")]);

        let map = ct.generate_map(SourceMapOptions::new().with_source("t.js"));
        let actual = map.get_tokens().count();
        let reserved = ct.last_reserved_token_capacity_for_test();

        assert!(actual > 0, "representative input must emit tokens");
        assert!(
            reserved >= actual,
            "reserved capacity ({reserved}) must cover the emitted token count \
             ({actual}) so the buffer never reallocates"
        );
    }

    // Fixture 2 — a moved overwrite whose replacement text carries newlines that
    // do NOT exist in the original source. When the overwrite is relocated it
    // becomes a Moved chunk, and each interior newline of its replacement text
    // emits its own token. An estimate that counts only original-source newlines
    // under-reserves here.
    {
        let source = "abcd"; // zero newlines in the original
        let mut ct = CodeTransform::new(source, &allocator);
        // Replace "bc" with text holding ten interior newlines, then relocate it:
        // the overwrite becomes a Moved chunk carrying that replacement text.
        ct.overwrite(1, 3, "L0\nL1\nL2\nL3\nL4\nL5\nL6\nL7\nL8\nL9\nLA");
        ct.move_wrapped(1, 3, 0, "", "");

        let estimate = ct.estimate_sourcemap_token_capacity();
        let chunk_count = ct.chunk_count();
        let original_newlines = source.matches('\n').count();
        // An estimate blind to moved replacement-text newlines would yield this:
        let original_newline_only_estimate = chunk_count + original_newlines + 2;

        let map = ct.generate_map(SourceMapOptions::new().with_source("t.js"));
        let actual = map.get_tokens().count();
        let reserved = ct.last_reserved_token_capacity_for_test();

        // The moved replacement text emits one token per interior newline, so the
        // emitted total outruns the original-newline-only estimate — confirming
        // this fixture genuinely exercises the moved replacement-text term.
        assert!(
            actual > original_newline_only_estimate,
            "moved-overwrite fixture must emit more tokens ({actual}) than an \
             original-newline-only estimate ({original_newline_only_estimate})"
        );
        // The estimate must therefore count those moved replacement-text newlines.
        assert!(
            estimate > original_newline_only_estimate,
            "estimate ({estimate}) must count moved replacement-text newlines, \
             not only original-source newlines ({original_newline_only_estimate})"
        );
        // And the captured reservation must still cover the full emitted count,
        // even with newlines injected by the moved overwrite.
        assert!(
            reserved >= actual,
            "reserved capacity ({reserved}) must cover the emitted token count \
             ({actual}) even when a moved overwrite injects newlines"
        );
    }
}

#[test]
fn test_source_map_generation() {
    let allocator = Allocator::default();
    let mut ct = CodeTransform::new("Hello World", &allocator);
    ct.overwrite(6, 11, "Rust");

    let options = SourceMapOptions::new()
        .with_source("input.js")
        .with_file("output.js");

    let map = ct.generate_map(options);

    // The source map should be valid
    let sources: Vec<_> = map.get_sources().collect();
    assert_eq!(sources.len(), 1);
    assert_eq!(sources[0], "input.js");
}

#[test]
fn test_source_map_with_content() {
    let allocator = Allocator::default();
    let source = "const x = 1;\nconst y = 2;";
    let mut ct = CodeTransform::new(source, &allocator);
    ct.overwrite(6, 7, "foo");

    let options = SourceMapOptions::new()
        .with_source("test.js")
        .include_content(true);

    let map = ct.generate_map(options);

    // Should include source content
    let content = map.get_source_content(0);
    assert!(content.is_some());
    assert_eq!(content.unwrap(), source);
}

/// Verify PositionResolver-based line/column calculation (0-indexed for source maps)
#[test]
fn test_line_column_calculation_via_resolver() {
    let source = "Hello\nWorld\nTest";
    let resolver = PositionResolver::new(source);

    // PositionResolver returns 1-indexed; source maps use 0-indexed
    let to_0 = |offset: usize| {
        let (line, col, _) = resolver.offset_to_line_col(offset);
        ((line - 1) as u32, (col - 1) as u32)
    };

    assert_eq!(to_0(0), (0, 0)); // H
    assert_eq!(to_0(5), (0, 5)); // \n
    assert_eq!(to_0(6), (1, 0)); // W
    assert_eq!(to_0(12), (2, 0)); // T
}

#[test]
fn test_line_column_edge_cases_via_resolver() {
    // Single line
    let resolver = PositionResolver::new("Hello");
    let (line, col, _) = resolver.offset_to_line_col(0);
    assert_eq!((line - 1, col - 1), (0, 0));
    let (line, col, _) = resolver.offset_to_line_col(4);
    assert_eq!((line - 1, col - 1), (0, 4));

    // Two lines: "abc\ndef"
    let resolver = PositionResolver::new("abc\ndef");
    let (line, col, _) = resolver.offset_to_line_col(0);
    assert_eq!((line - 1, col - 1), (0, 0));
    let (line, col, _) = resolver.offset_to_line_col(3);
    assert_eq!((line - 1, col - 1), (0, 3));
    let (line, col, _) = resolver.offset_to_line_col(4);
    assert_eq!((line - 1, col - 1), (1, 0));
    let (line, col, _) = resolver.offset_to_line_col(6);
    assert_eq!((line - 1, col - 1), (1, 2));

    // "a\nb\nc"
    let resolver = PositionResolver::new("a\nb\nc");
    let (line, col, _) = resolver.offset_to_line_col(2);
    assert_eq!((line - 1, col - 1), (1, 0));
    let (line, col, _) = resolver.offset_to_line_col(4);
    assert_eq!((line - 1, col - 1), (2, 0));
}

// ========================================================================
// TDD: Mapping accuracy tests — verify actual token positions
// ========================================================================

#[test]
fn test_source_map_token_positions_simple_overwrite() {
    let allocator = Allocator::default();
    // Source: "abc\ndef\nghi"
    //          012 3 456 7 890
    // Overwrite "def" (bytes 4-7) with "XYZ"
    // Output: "abc\nXYZ\nghi"
    let source = "abc\ndef\nghi";
    let mut ct = CodeTransform::new(source, &allocator);
    ct.overwrite(4, 7, "XYZ");

    let map = ct.generate_map(SourceMapOptions::new().with_source("test.js"));
    let tokens: Vec<_> = map.get_tokens().collect();

    // Token 0: start of "abc" — gen(0,0) → src(0,0)
    assert_eq!(tokens[0].get_dst_line(), 0);
    assert_eq!(tokens[0].get_dst_col(), 0);
    assert_eq!(tokens[0].get_src_line(), 0);
    assert_eq!(tokens[0].get_src_col(), 0);

    // Find token mapping "XYZ" — should be gen(1,0) → src(1,0)
    let xyz_token = tokens
        .iter()
        .find(|t| t.get_dst_line() == 1 && t.get_dst_col() == 0 && t.get_source_id().is_some())
        .expect("should have a token at generated line 1, col 0");
    assert_eq!(xyz_token.get_src_line(), 1);
    assert_eq!(xyz_token.get_src_col(), 0);

    // Find token mapping "ghi" — should be gen(2,0) → src(2,0)
    let ghi_token = tokens
        .iter()
        .find(|t| t.get_dst_line() == 2 && t.get_dst_col() == 0 && t.get_source_id().is_some())
        .expect("should have a token at generated line 2, col 0");
    assert_eq!(ghi_token.get_src_line(), 2);
    assert_eq!(ghi_token.get_src_col(), 0);
}

#[test]
fn test_source_map_token_positions_with_prepend() {
    let allocator = Allocator::default();
    let source = "abc\ndef";
    let mut ct = CodeTransform::new(source, &allocator);
    ct.prepend("// header\n");

    // Output: "// header\nabc\ndef"
    // The "abc" chunk maps to src(0,0) but should be at gen(1,0)
    let map = ct.generate_map(SourceMapOptions::new().with_source("test.js"));
    let tokens: Vec<_> = map.get_tokens().collect();

    // Find the token for "abc" — generated line should be 1 (after "// header\n")
    let abc_token = tokens
        .iter()
        .find(|t| t.get_src_line() == 0 && t.get_src_col() == 0 && t.get_source_id().is_some())
        .expect("should have a token mapping to src(0,0)");
    assert_eq!(
        abc_token.get_dst_line(),
        1,
        "abc should be on generated line 1 after header"
    );
    assert_eq!(abc_token.get_dst_col(), 0);
}

#[test]
fn test_source_map_token_positions_moved_multiline() {
    let allocator = Allocator::default();
    // Source: "line1\nline2\nline3\nline4"
    //          01234 5 67890 1 23456 7 89012
    // Move "line2\nline3\n" (6-18) to beginning with wrapping
    let source = "line1\nline2\nline3\nline4";
    let mut ct = CodeTransform::new(source, &allocator);
    ct.move_wrapped(6, 18, 0, "/*s*/", "/*e*/");
    // Output: "/*s*/line2\nline3\n/*e*/line1\nline4"

    let map = ct.generate_map(SourceMapOptions::new().with_source("test.js"));
    let tokens: Vec<_> = map.get_tokens().collect();

    // The moved "line2" should map back to original src line 1
    // It's at generated position after "/*s*/" (5 chars), so gen(0, 5)
    let line2_token = tokens
        .iter()
        .find(|t| t.get_src_line() == 1 && t.get_src_col() == 0 && t.get_source_id().is_some())
        .expect("should have a token mapping to src(1,0) for moved line2");
    assert_eq!(
        line2_token.get_dst_line(),
        0,
        "moved line2 should be on generated line 0"
    );
    assert_eq!(
        line2_token.get_dst_col(),
        5,
        "moved line2 should start at column 5 after /*s*/"
    );

    // The moved "line3" should map back to original src line 2
    let line3_token = tokens
        .iter()
        .find(|t| t.get_src_line() == 2 && t.get_src_col() == 0 && t.get_source_id().is_some())
        .expect("should have a token mapping to src(2,0) for moved line3");
    assert_eq!(
        line3_token.get_dst_line(),
        1,
        "moved line3 should be on generated line 1"
    );
}

// ========================================================================
// TDD: UTF-16 column accuracy tests — these should FAIL before the fix
// ========================================================================

/// Source: "abc中文\ndef"
///   bytes: a(0) b(1) c(2) 中(3-5) 文(6-8) \n(9) d(10) e(11) f(12)
///   UTF-16 cols on line 0: a=0, b=1, c=2, 中=3, 文=4, \n=5
/// After overwrite of "中" (bytes 3-6) with "X":
///   Output: "abcX文\ndef"
///   The "文" source col should be 4 (UTF-16), not 6 (bytes)
#[test]
fn test_source_map_utf16_column_for_cjk() {
    let allocator = Allocator::default();
    let source = "abc中文\ndef";
    let mut ct = CodeTransform::new(source, &allocator);
    ct.overwrite(3, 6, "X"); // Replace "中" with "X"

    // Output: "abcX文\ndef"
    assert_eq!(ct.build_string(), "abcX文\ndef");

    let map = ct.generate_map(SourceMapOptions::new().with_source("test.js"));
    let tokens: Vec<_> = map.get_tokens().collect();

    // Token for "abc" — src(0, 0)
    let abc_token = tokens
        .iter()
        .find(|t| t.get_dst_line() == 0 && t.get_dst_col() == 0 && t.get_source_id().is_some())
        .expect("should have token at gen(0,0)");
    assert_eq!(abc_token.get_src_col(), 0);

    // Token for "X" overwrite — should map to src(0, 3) (UTF-16 column of "中")
    // "中" starts at byte 3, which in UTF-16 columns is column 3 (a=0, b=1, c=2, 中=3)
    let x_token = tokens
        .iter()
        .find(|t| t.get_src_line() == 0 && t.get_src_col() == 3 && t.get_source_id().is_some())
        .expect("overwrite of 中 should map to src col 3 (UTF-16)");
    assert_eq!(x_token.get_dst_col(), 3, "generated col for X should be 3");

    // Token for "文" (remaining original) — should map to src(0, 4) in UTF-16
    // "文" starts at byte 6, but UTF-16 column should be 4 (after a, b, c, 中)
    let wen_token = tokens
        .iter()
        .find(|t| t.get_src_line() == 0 && t.get_src_col() == 4 && t.get_source_id().is_some());
    assert!(
        wen_token.is_some(),
        "文 should map to src col 4 (UTF-16), not 6 (bytes). Tokens: {:?}",
        tokens
            .iter()
            .filter(|t| t.get_src_line() == 0)
            .map(|t| (t.get_src_col(), t.get_dst_col()))
            .collect::<Vec<_>>()
    );
}

/// Source: "a😀b\ncd"
///   bytes: a(0) 😀(1-4) b(5) \n(6) c(7) d(8)
///   UTF-16 cols on line 0: a=0, 😀=1(+2 units), b=3, \n=4
/// The source column of 'b' should be 3 (UTF-16), not 5 (bytes)
#[test]
fn test_source_map_utf16_column_for_emoji() {
    let allocator = Allocator::default();
    let source = "a😀b\ncd";
    let mut ct = CodeTransform::new(source, &allocator);
    ct.overwrite(5, 6, "B"); // Replace 'b' with 'B'

    // Output: "a😀B\ncd"
    assert_eq!(ct.build_string(), "a😀B\ncd");

    let map = ct.generate_map(SourceMapOptions::new().with_source("test.js"));
    let tokens: Vec<_> = map.get_tokens().collect();

    // Token for overwrite of 'b' → should map to src(0, 3) in UTF-16
    // Byte offset 5 = after a(1 byte) + 😀(4 bytes) = col 3 in UTF-16 (a=0, 😀=1-2, b=3)
    let b_token = tokens
        .iter()
        .find(|t| t.get_src_line() == 0 && t.get_src_col() == 3 && t.get_source_id().is_some());
    assert!(
        b_token.is_some(),
        "b should map to src col 3 (UTF-16), not 5 (bytes). Tokens on line 0: {:?}",
        tokens
            .iter()
            .filter(|t| t.get_src_line() == 0)
            .map(|t| (t.get_src_col(), t.get_dst_col()))
            .collect::<Vec<_>>()
    );
}

/// Overwrite ASCII with CJK content, check generated column of next chunk
#[test]
fn test_source_map_generated_column_utf16() {
    let allocator = Allocator::default();
    let source = "abcdef";
    let mut ct = CodeTransform::new(source, &allocator);
    ct.overwrite(2, 4, "中文"); // Replace "cd" with "中文" (2 chars, 6 bytes)

    // Output: "ab中文ef"
    assert_eq!(ct.build_string(), "ab中文ef");

    let map = ct.generate_map(SourceMapOptions::new().with_source("test.js"));
    let tokens: Vec<_> = map.get_tokens().collect();

    // Token for "ef" — should be at generated col 4 (UTF-16: a=0, b=1, 中=2, 文=3, e=4)
    // NOT at generated col 8 (bytes: a=0, b=1, 中=2-4, 文=5-7, e=8)
    let ef_token = tokens
        .iter()
        .find(|t| t.get_src_line() == 0 && t.get_src_col() == 4 && t.get_source_id().is_some())
        .expect("should have token for original 'ef' at src col 4");
    assert_eq!(
        ef_token.get_dst_col(),
        4,
        "generated col for 'ef' should be 4 (UTF-16), not 8 (bytes)"
    );
}

// ========================================================================
// TDD: Coverage gap tests — verify existing behavior for untested paths
// ========================================================================

#[test]
fn test_source_map_include_content_false() {
    let allocator = Allocator::default();
    let source = "const x = 1;";
    let ct = CodeTransform::new(source, &allocator);

    let map = ct.generate_map(
        SourceMapOptions::new()
            .with_source("test.js")
            .include_content(false),
    );

    let content = map.get_source_content(0);
    assert!(content.is_some(), "source content entry should exist");
    assert_eq!(
        content.unwrap(),
        "",
        "content should be empty string when include_content is false"
    );
}

#[test]
fn test_source_map_no_source_option() {
    let allocator = Allocator::default();
    let source = "abc\ndef";
    let mut ct = CodeTransform::new(source, &allocator);
    ct.overwrite(0, 3, "XYZ");

    let map = ct.generate_map(SourceMapOptions::new()); // No source set

    let tokens: Vec<_> = map.get_tokens().collect();
    // All tokens should have no source_id
    for token in &tokens {
        assert!(
            token.get_source_id().is_none(),
            "token should have no source_id when source option is None"
        );
    }
}

#[test]
fn test_source_map_multiline_overwrite_content() {
    let allocator = Allocator::default();
    let source = "abcdef";
    let mut ct = CodeTransform::new(source, &allocator);
    ct.overwrite(2, 4, "X\nY"); // Replace "cd" with multiline content

    // Output: "abX\nYef"
    assert_eq!(ct.build_string(), "abX\nYef");

    let map = ct.generate_map(SourceMapOptions::new().with_source("test.js"));
    let tokens: Vec<_> = map.get_tokens().collect();

    // Token for "ef" should be on generated line 1 (after the newline in overwrite)
    let ef_token = tokens
        .iter()
        .find(|t| t.get_src_line() == 0 && t.get_src_col() == 4 && t.get_source_id().is_some())
        .expect("should have token for 'ef' at src(0,4)");
    assert_eq!(
        ef_token.get_dst_line(),
        1,
        "ef should be on generated line 1 after multiline overwrite"
    );
    assert_eq!(
        ef_token.get_dst_col(),
        1,
        "ef should be at generated col 1 after 'Y'"
    );
}

#[test]
fn test_source_map_with_outro() {
    let allocator = Allocator::default();
    let mut ct = CodeTransform::new("abc", &allocator);
    ct.append("\n// footer");

    // Output: "abc\n// footer"
    assert_eq!(ct.build_string(), "abc\n// footer");

    let map = ct.generate_map(SourceMapOptions::new().with_source("test.js"));
    let tokens: Vec<_> = map.get_tokens().collect();

    // Should have at least 2 tokens: one for "abc" and one for the outro
    assert!(
        tokens.len() >= 2,
        "should have tokens for content and outro"
    );

    // Find the outro token — it should be unmapped (no source_id)
    // The outro starts at the same position as end of content, so look for
    // a token with no source_id
    let has_unmapped = tokens.iter().any(|t| t.get_source_id().is_none());
    assert!(
        has_unmapped,
        "outro should produce an unmapped token (no source_id)"
    );
}

#[test]
fn test_source_map_empty_source() {
    let allocator = Allocator::default();
    let ct = CodeTransform::new("", &allocator);

    let map = ct.generate_map(SourceMapOptions::new().with_source("test.js"));
    // Should produce a valid (empty) source map
    let json = map.to_json_string();
    assert!(json.contains("\"mappings\""));
}

// ========================================================================
// Edge case tests
// ========================================================================

#[test]
fn test_source_map_moved_content_utf16() {
    let allocator = Allocator::default();
    // Source: "abc\n中文def"
    //   Line 0: a(0) b(1) c(2) \n(3)
    //   Line 1: 中(4-6) 文(7-9) d(10) e(11) f(12)
    //   UTF-16 cols on line 1: 中=0, 文=1, d=2, e=3, f=4
    let source = "abc\n中文def";
    let mut ct = CodeTransform::new(source, &allocator);
    // Move "中文def" (bytes 4-13) to the beginning
    ct.move_slice(4, 13, 0);
    // Output: "中文defabc\n"
    assert_eq!(ct.build_string(), "中文defabc\n");

    let map = ct.generate_map(SourceMapOptions::new().with_source("test.js"));
    let tokens: Vec<_> = map.get_tokens().collect();

    // The moved "中文def" should map back to src line 1, col 0
    let moved_token = tokens
        .iter()
        .find(|t| t.get_src_line() == 1 && t.get_src_col() == 0 && t.get_source_id().is_some())
        .expect("moved content should map to src(1, 0)");
    // It's at the start of generated output
    assert_eq!(moved_token.get_dst_line(), 0);
    assert_eq!(moved_token.get_dst_col(), 0);
}

#[test]
fn test_source_map_consecutive_newlines() {
    let allocator = Allocator::default();
    // Source: "a\n\nb\n\nc"  (lines: "a", "", "b", "", "c")
    let source = "a\n\nb\n\nc";
    let ct = CodeTransform::new(source, &allocator);

    let map = ct.generate_map(SourceMapOptions::new().with_source("test.js"));
    let tokens: Vec<_> = map.get_tokens().collect();

    // "a" at src(0,0) → gen(0,0)
    let a_token = tokens
        .iter()
        .find(|t| t.get_src_line() == 0 && t.get_src_col() == 0 && t.get_source_id().is_some())
        .expect("should have token for 'a'");
    assert_eq!(a_token.get_dst_line(), 0);

    // "b" at src(2,0) → gen(2,0)
    let b_token = tokens
        .iter()
        .find(|t| t.get_src_line() == 2 && t.get_src_col() == 0 && t.get_source_id().is_some())
        .expect("should have token for 'b'");
    assert_eq!(b_token.get_dst_line(), 2);

    // "c" at src(4,0) → gen(4,0)
    let c_token = tokens
        .iter()
        .find(|t| t.get_src_line() == 4 && t.get_src_col() == 0 && t.get_source_id().is_some())
        .expect("should have token for 'c'");
    assert_eq!(c_token.get_dst_line(), 4);
}

#[test]
fn test_source_map_trailing_newline() {
    let allocator = Allocator::default();
    // Source: "abc\ndef\n" — last byte is \n
    let source = "abc\ndef\n";
    let ct = CodeTransform::new(source, &allocator);

    let map = ct.generate_map(SourceMapOptions::new().with_source("test.js"));
    let tokens: Vec<_> = map.get_tokens().collect();

    // Should have tokens for "abc" (line 0) and "def" (line 1)
    // but NOT for line 2 (nothing after trailing newline)
    let line2_tokens: Vec<_> = tokens.iter().filter(|t| t.get_dst_line() == 2).collect();
    assert!(
        line2_tokens.is_empty(),
        "should have no tokens on line 2 after trailing newline"
    );
}

#[test]
fn test_source_map_remove_then_content() {
    let allocator = Allocator::default();
    // Source: "abcdef"
    // Remove "cd" (bytes 2-4), leaving "abef"
    let source = "abcdef";
    let mut ct = CodeTransform::new(source, &allocator);
    ct.overwrite(2, 4, "");

    assert_eq!(ct.build_string(), "abef");

    let map = ct.generate_map(SourceMapOptions::new().with_source("test.js"));
    let tokens: Vec<_> = map.get_tokens().collect();

    // "ab" at src(0,0) → gen(0,0)
    let ab_token = &tokens[0];
    assert_eq!(ab_token.get_src_col(), 0);
    assert_eq!(ab_token.get_dst_col(), 0);

    // "ef" at src(0,4) → gen(0,2) (generated col 2 after "ab")
    let ef_token = tokens
        .iter()
        .find(|t| t.get_src_col() == 4 && t.get_source_id().is_some())
        .expect("should have token for 'ef' at src col 4");
    assert_eq!(
        ef_token.get_dst_col(),
        2,
        "ef should be at generated col 2 after removal of 'cd'"
    );
}

#[test]
fn test_source_map_utf16_on_later_line() {
    let allocator = Allocator::default();
    // Source: "line1\na😀b"
    //   Line 0: l(0) i(1) n(2) e(3) 1(4) \n(5)
    //   Line 1: a(6) 😀(7-10) b(11)
    //   UTF-16 cols on line 1: a=0, 😀=1(+2 units), b=3
    let source = "line1\na😀b";
    let mut ct = CodeTransform::new(source, &allocator);
    ct.overwrite(11, 12, "B"); // Replace 'b' with 'B'

    assert_eq!(ct.build_string(), "line1\na😀B");

    let map = ct.generate_map(SourceMapOptions::new().with_source("test.js"));
    let tokens: Vec<_> = map.get_tokens().collect();

    // Overwrite of 'b' should map to src(1, 3) in UTF-16
    let b_token = tokens
        .iter()
        .find(|t| t.get_src_line() == 1 && t.get_src_col() == 3 && t.get_source_id().is_some());
    assert!(
        b_token.is_some(),
        "b on line 1 should map to src col 3 (UTF-16). Tokens on line 1: {:?}",
        tokens
            .iter()
            .filter(|t| t.get_src_line() == 1)
            .map(|t| (t.get_src_col(), t.get_dst_col()))
            .collect::<Vec<_>>()
    );

    // Generated col should also be 3 (UTF-16: a=0, 😀=1-2, B=3)
    let b_token = b_token.unwrap();
    assert_eq!(b_token.get_dst_col(), 3);
}

#[test]
fn test_source_map_overwrite_with_emoji_content() {
    let allocator = Allocator::default();
    // Source: "abcdef"
    // Overwrite "cd" with "😀" (4 bytes, 2 UTF-16 units)
    let source = "abcdef";
    let mut ct = CodeTransform::new(source, &allocator);
    ct.overwrite(2, 4, "😀");

    // Output: "ab😀ef"
    assert_eq!(ct.build_string(), "ab😀ef");

    let map = ct.generate_map(SourceMapOptions::new().with_source("test.js"));
    let tokens: Vec<_> = map.get_tokens().collect();

    // "ef" at src(0,4) → gen col should be 4 (UTF-16: a=0, b=1, 😀=2-3, e=4)
    // NOT 6 (bytes: a=0, b=1, 😀=2-5, e=6)
    let ef_token = tokens
        .iter()
        .find(|t| t.get_src_col() == 4 && t.get_source_id().is_some())
        .expect("should have token for 'ef' at src col 4");
    assert_eq!(
        ef_token.get_dst_col(),
        4,
        "generated col for 'ef' should be 4 (UTF-16), not 6 (bytes)"
    );
}

#[test]
fn test_source_map_after_moved_utf16_content() {
    let allocator = Allocator::default();
    // Source: "abc中文def"
    //   中(3-5) 文(6-8) — each 3 bytes, 1 UTF-16 unit
    // Move "中文" (bytes 3-9) to beginning
    let source = "abc中文def";
    let mut ct = CodeTransform::new(source, &allocator);
    ct.move_slice(3, 9, 0);

    // Output: "中文abcdef"
    assert_eq!(ct.build_string(), "中文abcdef");

    let map = ct.generate_map(SourceMapOptions::new().with_source("test.js"));
    let tokens: Vec<_> = map.get_tokens().collect();

    // "abc" (original bytes 0-3) should be at generated col 2
    // (after moved "中文" = 2 UTF-16 units), NOT col 6 (bytes)
    let abc_token = tokens
        .iter()
        .find(|t| t.get_src_line() == 0 && t.get_src_col() == 0 && t.get_source_id().is_some())
        .expect("should have token for 'abc' at src(0,0)");
    assert_eq!(
        abc_token.get_dst_col(),
        2,
        "abc should be at generated col 2 after moved 中文 (2 UTF-16 units), not 6 (bytes)"
    );
}

// ========================================================================
// InsertedMapped tests — source-mapped insertions
// ========================================================================

#[test]
fn test_inserted_mapped_produces_source_map_token() {
    let allocator = Allocator::default();
    // Source: "abc def ghi"
    //          012345678901
    // Use batch_prepend_left_with_source_map to insert "(def) ? " before "ghi"
    // mapped to source position 4 (where "def" starts).
    let source = "abc def ghi";
    let mut ct = CodeTransform::new(source, &allocator);
    ct.batch_prepend_left_with_source_map(&[(8, Some((4, 0)), "(def) ? ")]);

    // Output: "abc def (def) ? ghi"
    assert_eq!(ct.build_string(), "abc def (def) ? ghi");

    let map = ct.generate_map(SourceMapOptions::new().with_source("test.vue"));
    let tokens: Vec<_> = map.get_tokens().collect();

    // Find the InsertedMapped token — it should map to src(0, 4)
    let mapped_token = tokens
        .iter()
        .find(|t| t.get_src_col() == 4 && t.get_source_id().is_some());
    assert!(
        mapped_token.is_some(),
        "InsertedMapped should produce a token at src col 4. Tokens: {:?}",
        tokens
            .iter()
            .map(|t| (
                t.get_dst_line(),
                t.get_dst_col(),
                t.get_src_line(),
                t.get_src_col(),
                t.get_source_id()
            ))
            .collect::<Vec<_>>()
    );
    // The token should be at generated col 8 (after "abc def ")
    let mapped_token = mapped_token.unwrap();
    assert_eq!(mapped_token.get_dst_col(), 8);
    assert_eq!(mapped_token.get_src_line(), 0);
}

#[test]
fn test_inserted_mapped_none_produces_unmapped_token() {
    let allocator = Allocator::default();
    let source = "abcdef";
    let mut ct = CodeTransform::new(source, &allocator);
    ct.batch_prepend_left_with_source_map(&[(3, None, "XY")]);

    assert_eq!(ct.build_string(), "abcXYdef");

    let map = ct.generate_map(SourceMapOptions::new().with_source("test.vue"));
    let tokens: Vec<_> = map.get_tokens().collect();

    // The XY insertion should be unmapped (source_id = None)
    let xy_token = tokens.iter().find(|t| t.get_dst_col() == 3);
    assert!(xy_token.is_some(), "should have token at gen col 3");
    assert!(
        xy_token.unwrap().get_source_id().is_none(),
        "None source_pos should produce unmapped token"
    );
}

#[test]
fn test_inserted_mapped_multiline_content() {
    let allocator = Allocator::default();
    let source = "abcdef";
    let mut ct = CodeTransform::new(source, &allocator);
    ct.batch_prepend_left_with_source_map(&[(3, Some((0, 0)), "X\nY")]);

    // Output: "abcX\nYdef"
    assert_eq!(ct.build_string(), "abcX\nYdef");

    let map = ct.generate_map(SourceMapOptions::new().with_source("test.vue"));
    let tokens: Vec<_> = map.get_tokens().collect();

    // "def" at src(0,3) should be on generated line 1 after the newline
    let def_token = tokens
        .iter()
        .find(|t| t.get_src_col() == 3 && t.get_source_id().is_some())
        .expect("should have token for 'def'");
    assert_eq!(def_token.get_dst_line(), 1, "def should be on line 1");
    assert_eq!(
        def_token.get_dst_col(),
        1,
        "def should be at col 1 after 'Y'"
    );
}

#[test]
fn test_inserted_mapped_mixed_with_regular() {
    let allocator = Allocator::default();
    let source = "abcdef";
    let mut ct = CodeTransform::new(source, &allocator);
    // Two prepends at position 3: one unmapped, one mapped to source pos 0
    ct.batch_prepend_left_with_source_map(&[(3, None, ", "), (3, Some((0, 0)), "(show) ? ")]);

    assert_eq!(ct.build_string(), "abc, (show) ? def");

    let map = ct.generate_map(SourceMapOptions::new().with_source("test.vue"));
    let tokens: Vec<_> = map.get_tokens().collect();

    // The unmapped ", " should have no source_id
    let comma_token = tokens.iter().find(|t| t.get_dst_col() == 3);
    assert!(comma_token.is_some());
    assert!(comma_token.unwrap().get_source_id().is_none());

    // The mapped "(show) ? " should map to src(0, 0)
    let mapped_token = tokens
        .iter()
        .find(|t| t.get_dst_col() == 5 && t.get_source_id().is_some());
    assert!(
        mapped_token.is_some(),
        "mapped prepend should produce source-mapped token"
    );
    assert_eq!(mapped_token.unwrap().get_src_col(), 0);
}

// ── content_offset tests ────────────────────────────────────

#[test]
fn test_content_offset_shifts_token_within_content() {
    let allocator = Allocator::default();
    // Source: "abc show def"
    //          01234567890
    // Insert "(__props.show) ? " before "def" (position 9), mapped to source 4 (where "show" is)
    // content_offset = 9 (length of "(__props." = 9)
    let source = "abc show def";
    let mut ct = CodeTransform::new(source, &allocator);
    ct.batch_prepend_left_with_source_map(&[(9, Some((4, 9)), "(__props.show) ? ")]);

    assert_eq!(ct.build_string(), "abc show (__props.show) ? def");

    let map = ct.generate_map(SourceMapOptions::new().with_source("test.vue"));
    let tokens: Vec<_> = map.get_tokens().collect();

    // The unmapped prefix "(__props." should have NO source_id
    let prefix_token = tokens
        .iter()
        .find(|t| t.get_dst_col() == 9 && t.get_source_id().is_none());
    assert!(
        prefix_token.is_some(),
        "Unmapped prefix '(__props.' should produce unmapped token. Tokens: {:?}",
        tokens
            .iter()
            .map(|t| (
                t.get_dst_col(),
                t.get_src_col(),
                t.get_source_id().is_some()
            ))
            .collect::<Vec<_>>()
    );

    // The mapped token should be at dst_col 18 (9 + 9) pointing to src_col 4
    let mapped_token = tokens
        .iter()
        .find(|t| t.get_dst_col() == 18 && t.get_source_id().is_some());
    assert!(
        mapped_token.is_some(),
        "Mapped token should be at dst_col 18 (after prefix). Tokens: {:?}",
        tokens
            .iter()
            .map(|t| (
                t.get_dst_col(),
                t.get_src_col(),
                t.get_source_id().is_some()
            ))
            .collect::<Vec<_>>()
    );
    assert_eq!(
        mapped_token.unwrap().get_src_col(),
        4,
        "Mapped token should point to src_col 4 (position of 'show')"
    );
}

#[test]
fn test_content_offset_zero_is_original_behavior() {
    let allocator = Allocator::default();
    let source = "abcdef";
    let mut ct = CodeTransform::new(source, &allocator);
    ct.batch_prepend_left_with_source_map(&[(3, Some((0, 0)), "(show) ? ")]);

    assert_eq!(ct.build_string(), "abc(show) ? def");

    let map = ct.generate_map(SourceMapOptions::new().with_source("test.vue"));
    let tokens: Vec<_> = map.get_tokens().collect();

    // With content_offset = 0, token should be at dst_col 3 (start of content)
    let mapped = tokens
        .iter()
        .find(|t| t.get_dst_col() == 3 && t.get_source_id().is_some());
    assert!(
        mapped.is_some(),
        "With offset 0, token should be at content start. Tokens: {:?}",
        tokens
            .iter()
            .map(|t| (
                t.get_dst_col(),
                t.get_src_col(),
                t.get_source_id().is_some()
            ))
            .collect::<Vec<_>>()
    );
    assert_eq!(mapped.unwrap().get_src_col(), 0);

    // Negative: there should be NO unmapped token at dst_col 3
    // (since offset is 0, the very first token should be mapped)
    let unmapped_at_start = tokens
        .iter()
        .find(|t| t.get_dst_col() == 3 && t.get_source_id().is_none());
    assert!(
        unmapped_at_start.is_none(),
        "With offset 0, there should be no unmapped prefix token at content start"
    );
}

#[test]
fn test_content_offset_binding_prefix_hover_maps_to_identifier() {
    let allocator = Allocator::default();
    // Simulates: v-if="leftArrow" → condition prefix "(__props.leftArrow) ? "
    // Source: `<div v-if="leftArrow">` where "leftArrow" starts at byte 11
    // content_offset = 10 (length of "(__props." = 1 + 8 = 9... wait:
    //   "(" = 1, "__props." = 8, total = 9)
    let source = "<div v-if=\"leftArrow\">content</div>";
    let mut ct = CodeTransform::new(source, &allocator);
    // Insert condition prefix before "<div" (pos 0), mapped to source 11 (where "leftArrow" starts)
    // content_offset = 9: skip "(__props."
    ct.batch_prepend_left_with_source_map(&[(0, Some((11, 9)), "(__props.leftArrow) ? ")]);

    let output = ct.build_string();
    assert!(
        output.starts_with("(__props.leftArrow) ? <div"),
        "got: {}",
        output
    );

    let map = ct.generate_map(SourceMapOptions::new().with_source("test.vue"));
    let tokens: Vec<_> = map.get_tokens().collect();

    // The mapped token should be at dst_col 9 (after "(__props.") pointing to src_col 11
    let mapped = tokens
        .iter()
        .find(|t| t.get_dst_col() == 9 && t.get_source_id().is_some() && t.get_src_col() == 11);
    assert!(
        mapped.is_some(),
        "Mapped token at 'leftArrow' should point to src col 11. Tokens: {:?}",
        tokens
            .iter()
            .map(|t| (
                t.get_dst_col(),
                t.get_src_col(),
                t.get_source_id().is_some()
            ))
            .collect::<Vec<_>>()
    );

    // Negative: no mapped token should exist at dst_col 0 or 1
    // (the "(" and "__props." are unmapped)
    let mapped_at_prefix = tokens
        .iter()
        .find(|t| t.get_dst_col() < 9 && t.get_source_id().is_some());
    assert!(
        mapped_at_prefix.is_none(),
        "No mapped token should exist in the unmapped prefix region (dst_col < 9)"
    );
}

#[test]
fn test_content_offset_clamped_if_exceeds_length() {
    let allocator = Allocator::default();
    let source = "abcdef";
    let mut ct = CodeTransform::new(source, &allocator);
    // content_offset = 100, but content is only 3 bytes
    ct.batch_prepend_left_with_source_map(&[(3, Some((0, 100)), "XYZ")]);

    assert_eq!(ct.build_string(), "abcXYZdef");

    // Should not panic; the entire content becomes unmapped prefix
    let map = ct.generate_map(SourceMapOptions::new().with_source("test.vue"));
    let _tokens: Vec<_> = map.get_tokens().collect();
}
