use super::*;
use oxc_allocator::Allocator;

#[test]
fn test_new() {
    let allocator = Allocator::default();
    let ms = CodeTransform::new("Hello World", &allocator);
    assert_eq!(ms.build_string(), "Hello World");
    assert_eq!(ms.original(), "Hello World");
    assert!(!ms.is_modified());
}

#[test]
fn test_empty_string() {
    let allocator = Allocator::default();
    let ms = CodeTransform::new("", &allocator);
    assert_eq!(ms.build_string(), "");
    assert!(!ms.is_modified());
}

#[test]
fn test_append() {
    let allocator = Allocator::default();
    let mut ms = CodeTransform::new("Hello", &allocator);
    ms.append(" World");
    assert_eq!(ms.build_string(), "Hello World");
    assert!(ms.is_modified());
}

#[test]
fn test_prepend() {
    let allocator = Allocator::default();
    let mut ms = CodeTransform::new("World", &allocator);
    ms.prepend("Hello ");
    assert_eq!(ms.build_string(), "Hello World");
    assert!(ms.is_modified());
}

#[test]
fn test_append_multiple() {
    let allocator = Allocator::default();
    let mut ms = CodeTransform::new("Hello", &allocator);
    ms.append(" World");
    ms.append("!");
    assert_eq!(ms.build_string(), "Hello World!");
}

#[test]
fn test_prepend_multiple() {
    let allocator = Allocator::default();
    let mut ms = CodeTransform::new("World", &allocator);
    ms.prepend(" ");
    ms.prepend("Hello");
    assert_eq!(ms.build_string(), "Hello World");
}

#[test]
fn test_overwrite_simple() {
    let allocator = Allocator::default();
    let mut ms = CodeTransform::new("Hello World", &allocator);
    ms.overwrite(6, 11, "Rust");
    assert_eq!(ms.build_string(), "Hello Rust");
    assert!(ms.is_modified());
}

#[test]
fn test_overwrite_beginning() {
    let allocator = Allocator::default();
    let mut ms = CodeTransform::new("Hello World", &allocator);
    ms.overwrite(0, 5, "Hi");
    assert_eq!(ms.build_string(), "Hi World");
}

#[test]
fn test_overwrite_end() {
    let allocator = Allocator::default();
    let mut ms = CodeTransform::new("Hello World", &allocator);
    ms.overwrite(6, 11, "Everyone");
    assert_eq!(ms.build_string(), "Hello Everyone");
}

#[test]
fn test_overwrite_entire() {
    let allocator = Allocator::default();
    let mut ms = CodeTransform::new("Hello World", &allocator);
    ms.overwrite(0, 11, "Goodbye");
    assert_eq!(ms.build_string(), "Goodbye");
}

#[test]
fn test_overwrite_multiple_non_overlapping() {
    let allocator = Allocator::default();
    let mut ms = CodeTransform::new("Hello World Test", &allocator);
    ms.overwrite(0, 5, "Hi");
    ms.overwrite(12, 16, "Case");
    assert_eq!(ms.build_string(), "Hi World Case");
}

#[test]
fn test_overwrite_with_empty() {
    let allocator = Allocator::default();
    let mut ms = CodeTransform::new("Hello World", &allocator);
    ms.overwrite(5, 6, "");
    assert_eq!(ms.build_string(), "HelloWorld");
}

#[test]
fn test_replace_alias() {
    let allocator = Allocator::default();
    let mut ms = CodeTransform::new("Hello World", &allocator);
    ms.replace(6, 11, "Rust");
    assert_eq!(ms.build_string(), "Hello Rust");
}

#[test]
fn test_remove() {
    let allocator = Allocator::default();
    let mut ms = CodeTransform::new("Hello World", &allocator);
    ms.remove(5, 6);
    assert_eq!(ms.build_string(), "HelloWorld");
}

#[test]
fn test_remove_range() {
    let allocator = Allocator::default();
    let mut ms = CodeTransform::new("Hello Beautiful World", &allocator);
    ms.remove(6, 16);
    assert_eq!(ms.build_string(), "Hello World");
}

#[test]
fn test_combined_operations() {
    let allocator = Allocator::default();
    let mut ms = CodeTransform::new("Hello World", &allocator);
    ms.prepend("(");
    ms.append(")");
    ms.overwrite(6, 11, "Rust");
    assert_eq!(ms.build_string(), "(Hello Rust)");
}

#[test]
fn test_chaining() {
    let allocator = Allocator::default();
    let mut ms = CodeTransform::new("Hello World", &allocator);
    ms.prepend("Start: ")
        .overwrite(6, 11, "Rust")
        .append(" - End");
    assert_eq!(ms.build_string(), "Start: Hello Rust - End");
}

#[test]
fn test_multiline_overwrite() {
    let allocator = Allocator::default();
    let mut ms = CodeTransform::new("Line 1\nLine 2\nLine 3", &allocator);
    ms.overwrite(7, 13, "Middle");
    assert_eq!(ms.build_string(), "Line 1\nMiddle\nLine 3");
}

#[test]
fn test_slice() {
    let allocator = Allocator::default();
    let ms = CodeTransform::new("Hello World", &allocator);
    assert_eq!(ms.slice(0, 5), "Hello");
    assert_eq!(ms.slice(6, 11), "World");
}

#[test]
fn test_prepend_left() {
    let allocator = Allocator::default();
    let mut ms = CodeTransform::new("Hello World", &allocator);
    ms.prepend_left(6, "Beautiful ");
    assert_eq!(ms.build_string(), "Hello Beautiful World");
}

#[test]
fn test_append_left() {
    let allocator = Allocator::default();
    let mut ms = CodeTransform::new("Hello World", &allocator);
    ms.append_left(5, ",");
    assert_eq!(ms.build_string(), "Hello, World");
}

#[test]
fn test_from_str() {
    let allocator = Allocator::default();
    let ms = CodeTransform::new("Hello", &allocator);
    assert_eq!(ms.build_string(), "Hello");
}

#[test]
fn test_from_string() {
    let allocator = Allocator::default();
    let hello = String::from("Hello");
    let ms = CodeTransform::new(&hello, &allocator);
    assert_eq!(ms.build_string(), "Hello");
}

#[test]
fn test_display_trait() {
    let allocator = Allocator::default();
    let ms = CodeTransform::new("Hello World", &allocator);
    assert_eq!(format!("{}", ms), "Hello World");
}

#[test]
fn test_complex_transformation() {
    // Simulate a real code transformation scenario
    let source = r#"const x = "old value";
const y = "another value";
console.log(x, y);"#;

    let allocator = Allocator::default();
    let mut ms = CodeTransform::new(source, &allocator);

    // Replace the first string value
    ms.overwrite(10, 21, r#""new value""#);

    // Replace the second string value
    ms.overwrite(33, 48, r#""updated value""#);

    // Add a comment at the beginning
    ms.prepend("// Generated code\n");

    // Add an export at the end
    ms.append("\nexport { x, y };");

    let expected = r#"// Generated code
const x = "new value";
const y = "updated value";
console.log(x, y);
export { x, y };"#;

    assert_eq!(ms.build_string(), expected);
}

#[test]
fn test_unicode_handling() {
    let allocator = Allocator::default();
    let mut ms = CodeTransform::new("Hello 🦀 World", &allocator);
    // Note: emoji is 4 bytes in UTF-8
    ms.overwrite(6, 10, "🎉");
    assert_eq!(ms.build_string(), "Hello 🎉 World");
}

#[test]
fn test_empty_operations() {
    let allocator = Allocator::default();
    let mut ms = CodeTransform::new("Hello", &allocator);
    ms.append("");
    ms.prepend("");
    assert_eq!(ms.build_string(), "Hello");
    assert!(!ms.is_modified());
}

#[test]
fn test_overwrite_invalid_range() {
    let allocator = Allocator::default();
    let mut ms = CodeTransform::new("Hello World", &allocator);
    ms.overwrite(5, 5, "X"); // Empty range
    assert_eq!(ms.build_string(), "Hello World"); // Should be unchanged
}

#[test]
fn test_source_map_options() {
    let options = SourceMapOptions::new()
        .with_source("input.js")
        .with_file("output.js")
        .include_content(true);

    assert_eq!(options.source, Some("input.js"));
    assert_eq!(options.file, Some("output.js"));
    assert!(options.include_content);
}

#[test]
fn test_generate_map_basic() {
    let allocator = Allocator::default();
    let mut ms = CodeTransform::new("Hello World", &allocator);
    ms.overwrite(6, 11, "Rust");

    let options = SourceMapOptions::new()
        .with_source("input.js")
        .with_file("output.js");

    let map = ms.generate_map(options);

    let sources: Vec<_> = map.get_sources().collect();
    assert_eq!(sources.len(), 1);
    assert_eq!(sources[0], "input.js");
}

#[test]
fn test_generate_map_json() {
    let allocator = Allocator::default();
    let ms = CodeTransform::new("test", &allocator);
    let options = SourceMapOptions::new().with_source("test.js");
    let json = ms.generate_map_json(options);

    // Should be valid JSON
    assert!(json.contains("\"sources\""));
    assert!(json.contains("\"mappings\""));
}

// ============================================================================
// Edge Case Tests
// ============================================================================

#[test]
fn test_overlapping_overwrites() {
    let allocator = Allocator::default();
    let mut ct = CodeTransform::new("Hello World Test", &allocator);

    // First overwrite
    ct.overwrite(6, 11, "Rust");
    assert_eq!(ct.build_string(), "Hello Rust Test");

    // Second overwrite overlapping the first
    ct.overwrite(6, 11, "Java");
    assert_eq!(ct.build_string(), "Hello Java Test");
}

#[test]
fn test_multiple_inserts_same_position() {
    let allocator = Allocator::default();
    let mut ct = CodeTransform::new("Hello World", &allocator);

    // Multiple inserts at position 6 - they're inserted in reverse order (LIFO)
    ct.prepend_left(6, "Beautiful ");
    ct.prepend_left(6, "Very ");

    // The second insert comes first because it's inserted at the same position
    assert_eq!(ct.build_string(), "Hello Beautiful Very World");
}

#[test]
fn test_insert_at_position_zero() {
    let allocator = Allocator::default();
    let mut ct = CodeTransform::new("World", &allocator);

    ct.prepend_left(0, "Hello ");
    assert_eq!(ct.build_string(), "Hello World");
}

#[test]
fn test_insert_at_end() {
    let allocator = Allocator::default();
    let mut ct = CodeTransform::new("Hello", &allocator);
    let len = "Hello".len() as u32;

    ct.prepend_left(len, " World");
    assert_eq!(ct.build_string(), "Hello World");
}

#[test]
fn test_remove_everything() {
    let allocator = Allocator::default();
    let mut ct = CodeTransform::new("Hello World", &allocator);

    ct.remove(0, 11);
    assert_eq!(ct.build_string(), "");
}

#[test]
fn test_source_map_after_multiple_edits() {
    let allocator = Allocator::default();
    let source = "line1\nline2\nline3";
    let mut ct = CodeTransform::new(source, &allocator);

    ct.overwrite(0, 5, "LINE1");
    ct.overwrite(6, 11, "LINE2");
    ct.prepend("// Header\n");

    let options = SourceMapOptions::new()
        .with_source("test.js")
        .include_content(true);

    let map = ct.generate_map(options);

    // Verify source map is valid
    let sources: Vec<_> = map.get_sources().collect();
    assert_eq!(sources.len(), 1);

    // Verify original content is preserved
    let content = map.get_source_content(0);
    assert_eq!(content.unwrap(), source);
}

// ============================================================================
// Move Slice Tests
// ============================================================================

#[test]
fn test_move_slice_to_beginning() {
    let allocator = Allocator::default();
    let mut ct = CodeTransform::new("ABCDEF", &allocator);
    ct.move_slice(2, 4, 0); // Move "CD" to the beginning
    assert_eq!(ct.build_string(), "CDABEF");
}

#[test]
fn test_move_slice_to_end() {
    let allocator = Allocator::default();
    let mut ct = CodeTransform::new("ABCDEF", &allocator);
    ct.move_slice(0, 2, 6); // Move "AB" to the end
    assert_eq!(ct.build_string(), "CDEFAB");
}

#[test]
fn test_move_slice_to_middle() {
    let allocator = Allocator::default();
    let mut ct = CodeTransform::new("ABCDEF", &allocator);
    ct.move_slice(4, 6, 2); // Move "EF" to after "AB"
    assert_eq!(ct.build_string(), "ABEFCD");
}

#[test]
fn test_move_slice_same_position() {
    let allocator = Allocator::default();
    let mut ct = CodeTransform::new("ABCDEF", &allocator);
    ct.move_slice(2, 4, 2); // Move "CD" to its own position
    assert_eq!(ct.build_string(), "ABCDEF");
}

#[test]
fn test_move_slice_empty_range() {
    let allocator = Allocator::default();
    let mut ct = CodeTransform::new("ABCDEF", &allocator);
    ct.move_slice(2, 2, 0); // Empty range
    assert_eq!(ct.build_string(), "ABCDEF");
}

#[test]
fn test_move_slice_preserves_source_mapping() {
    let allocator = Allocator::default();
    let source = "Hello World";
    let mut ct = CodeTransform::new(source, &allocator);

    // Move "World" to the beginning
    ct.move_slice(6, 11, 0);
    assert_eq!(ct.build_string(), "WorldHello ");

    // Generate source map and verify it's valid
    let options = SourceMapOptions::new()
        .with_source("test.js")
        .include_content(true);

    let map = ct.generate_map(options);
    let sources: Vec<_> = map.get_sources().collect();
    assert_eq!(sources.len(), 1);
}

#[test]
fn test_move_slice_with_insertions() {
    let allocator = Allocator::default();
    let mut ct = CodeTransform::new("ABCDEFGH", &allocator);

    // Insert "X" at position 4 (between D and E)
    ct.append_left(4, "X");
    assert_eq!(ct.build_string(), "ABCDXEFGH");

    // Now move slice 2-6 (which includes "CD", the inserted "X", and "EF") to position 0
    ct.move_slice(2, 6, 0);

    // Expected: "CDXEF" moves to beginning, leaving "AB" and "GH"
    // Result should be: "CDXEFABGH"
    assert_eq!(ct.build_string(), "CDXEFABGH");
}

#[test]
fn test_move_slice_with_multiple_insertions() {
    let allocator = Allocator::default();
    let mut ct = CodeTransform::new("ABCDEFGH", &allocator);

    // Insert at multiple positions within the range we'll move
    ct.append_left(3, "1");
    ct.append_left(5, "2");
    assert_eq!(ct.build_string(), "ABC1DE2FGH");

    // Move slice 2-6 to end (position 8)
    ct.move_slice(2, 6, 8);

    // "C1DE2F" should move to end
    assert_eq!(ct.build_string(), "ABGHC1DE2F");
}

// ============================================================================
// Move With Prefix/Suffix/Wrapped Tests
// ============================================================================

#[test]
fn test_move_with_prefix() {
    let allocator = Allocator::default();
    let mut ct = CodeTransform::new("ABCDEF", &allocator);
    ct.move_with_prefix(2, 4, 0, ">>"); // Move "CD" to beginning with prefix
    assert_eq!(ct.build_string(), ">>CDABEF");
}

#[test]
fn test_move_with_suffix() {
    let allocator = Allocator::default();
    let mut ct = CodeTransform::new("ABCDEF", &allocator);
    ct.move_with_suffix(2, 4, 0, "<<"); // Move "CD" to beginning with suffix
    assert_eq!(ct.build_string(), "CD<<ABEF");
}

#[test]
fn test_move_wrapped() {
    let allocator = Allocator::default();
    let mut ct = CodeTransform::new("ABCDEF", &allocator);
    ct.move_wrapped(2, 4, 0, "{", "}"); // Move "CD" wrapped with braces
    assert_eq!(ct.build_string(), "{CD}ABEF");
}

#[test]
fn test_move_wrapped_to_end() {
    let allocator = Allocator::default();
    let mut ct = CodeTransform::new("ABCDEF", &allocator);
    ct.move_wrapped(0, 2, 6, "(", ")"); // Move "AB" to end wrapped
    assert_eq!(ct.build_string(), "CDEF(AB)");
}

#[test]
fn test_move_with_prefix_preserves_source_mapping() {
    let allocator = Allocator::default();
    let source = "Hello World";
    let mut ct = CodeTransform::new(source, &allocator);

    // Move "World" to beginning with prefix
    ct.move_with_prefix(6, 11, 0, "// ");
    assert_eq!(ct.build_string(), "// WorldHello ");

    // Generate source map and verify it's valid
    let options = SourceMapOptions::new()
        .with_source("test.js")
        .include_content(true);

    let map = ct.generate_map(options);
    let sources: Vec<_> = map.get_sources().collect();
    assert_eq!(sources.len(), 1);
}

#[test]
fn test_move_wrapped_multiline() {
    let allocator = Allocator::default();
    let mut ct = CodeTransform::new("const x = 1;\nconst y = 2;\nconst z = 3;", &allocator);
    // Move "const y = 2;\n" (positions 13-26) to beginning, wrapped
    ct.move_wrapped(13, 26, 0, "/* moved */\n", "/* end */\n");
    assert_eq!(
        ct.build_string(),
        "/* moved */\nconst y = 2;\n/* end */\nconst x = 1;\nconst z = 3;"
    );
}

#[test]
fn test_move_with_prefix_empty_prefix() {
    let allocator = Allocator::default();
    let mut ct = CodeTransform::new("ABCDEF", &allocator);
    ct.move_with_prefix(2, 4, 0, ""); // Empty prefix should work like regular move
    assert_eq!(ct.build_string(), "CDABEF");
}

#[test]
fn test_move_wrapped_with_insertions() {
    let allocator = Allocator::default();
    let mut ct = CodeTransform::new("ABCDEFGH", &allocator);

    // Insert "X" at position 4
    ct.append_left(4, "X");
    assert_eq!(ct.build_string(), "ABCDXEFGH");

    // Move slice 2-6 with wrapping - insertion should come along
    ct.move_wrapped(2, 6, 0, "[", "]");
    assert_eq!(ct.build_string(), "[CDXEF]ABGH");
}

// ============================================================================
// Overwrite After Move Tests (was_moved behavior)
// ============================================================================

#[test]
fn test_overwrite_after_move_wrapped_does_not_affect_moved_chunk() {
    let allocator = Allocator::default();
    // Simulates Vue SFC compilation: move type params, then overwrite defineProps
    // Source: "const props = defineProps<{title: string}>();"
    //                        ^---------^             ^--^
    //                        to overwrite             to move
    let mut ct = CodeTransform::new("const props = defineProps<{title: string}>();", &allocator);

    // First, move the type params "{title: string}" (positions 26-41) to beginning
    ct.move_wrapped(26, 41, 0, "props:", ",\n");
    assert_eq!(
        ct.build_string(),
        "props:{title: string},\nconst props = defineProps<>();"
    );

    // Then, overwrite "defineProps<" (positions 14-26) with "_props"
    // This should NOT be affected by the moved chunk's original positions
    ct.overwrite(14, 26, "_props");
    assert_eq!(
        ct.build_string(),
        "props:{title: string},\nconst props = _props>();"
    );
}

#[test]
fn test_overwrite_position_not_affected_by_moved_chunks() {
    let allocator = Allocator::default();
    // Test that moved chunks don't trigger incorrect overwrite insertion
    // Source: "AB[content]CD[other]EF"
    //            ^-------^  to move first
    //                     ^------^ to overwrite after
    let mut ct = CodeTransform::new("AB[content]CD[other]EF", &allocator);

    // Move "[content]" (positions 2-11) to the end
    ct.move_slice(2, 11, 22);
    assert_eq!(ct.build_string(), "ABCD[other]EF[content]");

    // Now overwrite "CD" (positions 11-13 in original)
    // The moved chunk's original position (2-11) should not affect this
    ct.overwrite(11, 13, "XX");
    assert_eq!(ct.build_string(), "ABXX[other]EF[content]");
}

#[test]
fn test_move_then_overwrite_within_remaining_content() {
    let allocator = Allocator::default();
    // Move content from middle to beginning, then overwrite something after
    let mut ct = CodeTransform::new("START_MOVE_ME_END_REST", &allocator);

    // Move "MOVE_ME_" (positions 6-14) to position 0
    ct.move_wrapped(6, 14, 0, "[", "]");
    assert_eq!(ct.build_string(), "[MOVE_ME_]START_END_REST");

    // Overwrite "END_" (positions 14-18 in original) with "XXX"
    ct.overwrite(14, 18, "XXX");
    assert_eq!(ct.build_string(), "[MOVE_ME_]START_XXXREST");
}

#[test]
fn test_multiple_moves_then_overwrite() {
    let allocator = Allocator::default();
    let mut ct = CodeTransform::new("A_ONE_B_TWO_C_THREE_D", &allocator);

    // Move "ONE_" to beginning
    ct.move_wrapped(2, 6, 0, "(1:", ")");
    assert_eq!(ct.build_string(), "(1:ONE_)A_B_TWO_C_THREE_D");

    // Move "TWO_" to beginning - moves are appended, so second move comes after first
    ct.move_wrapped(8, 12, 0, "(2:", ")");
    assert_eq!(ct.build_string(), "(1:ONE_)(2:TWO_)A_B_C_THREE_D");

    // Now overwrite "THREE_" (positions 14-20 in original)
    ct.overwrite(14, 20, "3");
    assert_eq!(ct.build_string(), "(1:ONE_)(2:TWO_)A_B_C_3D");
}

#[test]
fn test_overwrite_between_moved_chunks_original_positions() {
    let allocator = Allocator::default();
    // Two chunks moved from positions that surround the overwrite target
    // Move A (positions 0-2), Move C (positions 6-8), Overwrite B (positions 3-5)
    let mut ct = CodeTransform::new("AA_BBB_CC_END", &allocator);

    // Move "AA_" to end
    ct.move_slice(0, 3, 13);
    assert_eq!(ct.build_string(), "BBB_CC_ENDAA_");

    // Move "CC_" to end
    ct.move_slice(7, 10, 13);
    assert_eq!(ct.build_string(), "BBB_ENDAA_CC_");

    // Overwrite "BBB_" (positions 3-7 in original) - should still work correctly
    ct.overwrite(3, 7, "XXX");
    assert_eq!(ct.build_string(), "XXXENDAA_CC_");
}

#[test]
fn test_vue_sfc_style_transformation() {
    let allocator = Allocator::default();
    // Simulate a Vue SFC-style transformation with multiple operations
    // "const props = defineProps<{x: number}>();"
    //  0    6     14          26        37   42
    //                         ^--------^ type params (27-37)
    //               ^---------^ defineProps< (14-26)
    //                                   ^--^ >() to remove (37-40)
    let source = "const props = defineProps<{x: number}>();";
    let mut ct = CodeTransform::new(source, &allocator);

    // 1. Move type params "{x: number}" (positions 26-37) to beginning
    ct.move_wrapped(26, 37, 0, "props:", ",\n");

    // 2. Overwrite "defineProps<" (positions 14-26) with "_props"
    ct.overwrite(14, 26, "_props");

    // 3. Remove ">()" (positions 37-40)
    ct.remove(37, 40);

    let result = ct.build_string();

    // Verify the transformation
    assert!(
        result.contains("props:{x: number}"),
        "Should have moved type params with prefix"
    );
    assert!(
        result.contains("const props = _props"),
        "Should have replaced defineProps< with _props"
    );
    assert!(
        !result.contains("defineProps<"),
        "Should not contain defineProps<"
    );
    assert!(!result.contains(">()"), "Should have removed >()");
}

// ============================================================================
// Bug Fix Tests: Move Edited chunks then prepend_left
// These tests verify the fix for the emits positioning bug where prepend_left
// was inserting content between the prefix and content of move_wrapped calls
// when the moved content was an Edited chunk (from overwrite).
// ============================================================================

#[test]
fn test_move_wrapped_overwritten_chunk_then_prepend_left() {
    // This is the exact bug scenario:
    // 1. Overwrite a span (creates Edited chunk with was_moved: false)
    // 2. Move that span with wrapping to a target position
    // 3. Prepend something at the same target position
    // Expected: prepend should appear AFTER the moved content
    let allocator = Allocator::default();
    let mut ct = CodeTransform::new("START_TYPE_CONTENT_END", &allocator);

    // Overwrite "TYPE_CONTENT" with transformed content (like process_define_emits does)
    ct.overwrite(6, 18, "[\"transformed\"]");
    assert_eq!(ct.build_string(), "START_[\"transformed\"]_END");

    // Move the overwritten span to position 0 with wrapping (like emit_emits_section does)
    ct.move_wrapped(6, 18, 0, "emits:", ",\n");
    assert_eq!(ct.build_string(), "emits:[\"transformed\"],\nSTART__END");

    // Prepend at position 0 (like the setup function declaration)
    ct.prepend_left(0, "setup(){");

    // BUG: Without fix, this would be "emits:setup(){[\"transformed\"]..."
    // EXPECTED: setup should appear AFTER emits
    assert_eq!(
        ct.build_string(),
        "emits:[\"transformed\"],\nsetup(){START__END"
    );
}

#[test]
fn test_multiple_move_wrapped_then_prepend_left_ordering() {
    let allocator = Allocator::default();
    let mut ct = CodeTransform::new("_PROPS_CONTENT__EMITS_CONTENT__BODY", &allocator);

    // Overwrite both content spans
    ct.overwrite(1, 15, "{prop:1}");
    ct.overwrite(16, 30, "[\"emit\"]");

    // Move props with wrapping to position 0
    ct.move_wrapped(1, 15, 0, "props:", ",\n");

    // Move emits with wrapping to position 0
    ct.move_wrapped(16, 30, 0, "emits:", ",\n");

    // Prepend setup at position 0
    ct.prepend_left(0, "setup(){");

    // Expected order: props, emits, setup, body
    // setup should be LAST in the inserted content before the body
    let result = ct.build_string();
    let props_pos = result.find("props:").unwrap();
    let emits_pos = result.find("emits:").unwrap();
    let setup_pos = result.find("setup(){").unwrap();

    assert!(props_pos < emits_pos, "props should come before emits");
    assert!(emits_pos < setup_pos, "emits should come before setup");
}

#[test]
fn test_append_left_prepend_left_ordering_after_move_wrapped() {
    let allocator = Allocator::default();
    let mut ct = CodeTransform::new("_CONTENT_REST", &allocator);

    // Overwrite content (positions 1-9: "CONTENT_")
    ct.overwrite(1, 9, "moved");

    // Move to position 0
    ct.move_wrapped(1, 9, 0, "[", "]");

    // append_left at 0 should go after the moved content
    ct.append_left(0, "APPEND");

    // prepend_left at 0 should also go after the moved content (after the fix)
    ct.prepend_left(0, "PREPEND");

    // Order: moved content, then APPEND, then PREPEND, then original "_REST"
    assert_eq!(ct.build_string(), "[moved]APPENDPREPEND_REST");
}

#[test]
fn test_mixed_operations_same_position() {
    let allocator = Allocator::default();
    let mut ct = CodeTransform::new("A__B__C__D", &allocator);

    // Overwrite B's position
    ct.overwrite(1, 3, "BB");

    // Overwrite C's position
    ct.overwrite(5, 7, "CC");

    // Move B to position 0 with wrapping
    ct.move_wrapped(1, 3, 0, "(b:", ")");

    // Move C to position 0 with wrapping
    ct.move_wrapped(5, 7, 0, "(c:", ")");

    // Append at 0
    ct.append_left(0, "AFTER_MOVES");

    // Prepend at 0
    ct.prepend_left(0, "BEFORE_ORIGINAL");

    let result = ct.build_string();

    // All moves should come first, then append, then prepend, then original content
    assert!(
        result.starts_with("(b:BB)(c:CC)AFTER_MOVESBEFORE_ORIGINALA"),
        "Expected order: moves, append, prepend, original. Got: {}",
        result
    );
}

#[test]
fn test_move_slice_overwritten_chunk_then_prepend_left() {
    // Same as move_wrapped but using move_slice
    let allocator = Allocator::default();
    let mut ct = CodeTransform::new("START_CONTENT_END", &allocator);

    // Overwrite "CONTENT" with transformed content
    ct.overwrite(6, 13, "MOVED");
    assert_eq!(ct.build_string(), "START_MOVED_END");

    // Move the overwritten span to position 0
    ct.move_slice(6, 13, 0);
    assert_eq!(ct.build_string(), "MOVEDSTART__END");

    // Prepend at position 0
    ct.prepend_left(0, "PREFIX:");

    // PREFIX: should appear AFTER the moved content
    assert_eq!(ct.build_string(), "MOVEDPREFIX:START__END");
}

// ============================================================================
// TDD Safety-Net Tests: Insert Position Semantics
// Pin exact behavior before refactoring find_insert_position_for_*
// ============================================================================

#[test]
fn test_prepend_left_split_original_chunk() {
    // prepend_left at a position inside an Original chunk forces a split
    let allocator = Allocator::default();
    let mut ct = CodeTransform::new("ABCDEF", &allocator);
    // Position 3 is inside the single Original chunk [0,6)
    ct.prepend_left(3, "X");
    assert_eq!(ct.build_string(), "ABCXDEF");
    // Should have split into [0,3), Inserted("X"), [3,6)
    assert_eq!(ct.chunk_count(), 3);
}

#[test]
fn test_append_left_split_original_chunk() {
    let allocator = Allocator::default();
    let mut ct = CodeTransform::new("ABCDEF", &allocator);
    ct.append_left(3, "X");
    assert_eq!(ct.build_string(), "ABCXDEF");
    assert_eq!(ct.chunk_count(), 3);
}

#[test]
fn test_prepend_right_after_edited_chunk() {
    // prepend_right when target position corresponds to an Edited chunk
    let allocator = Allocator::default();
    let mut ct = CodeTransform::new("ABCDEF", &allocator);
    ct.overwrite(2, 4, "XX"); // Replace "CD" with "XX"
    ct.prepend_right(4, "Y"); // Insert after position 4 (after "E")
                              // Position 4 maps to "E" in original, prepend_right inserts after it
    assert_eq!(ct.build_string(), "ABXXEFY");
}

#[test]
fn test_append_right_skips_pure_insertions() {
    // append_right at end of source appends in order
    let allocator = Allocator::default();
    let len = "ABCDEF".len() as u32;
    let mut ct = CodeTransform::new("ABCDEF", &allocator);
    ct.append_right(len, "1"); // First insert after end
    ct.append_right(len, "2"); // Second insert after end — goes after "1"
    assert_eq!(ct.build_string(), "ABCDEF12");
}

#[test]
fn test_insert_at_end_of_source() {
    let allocator = Allocator::default();
    let source = "ABCDEF";
    let len = source.len() as u32;
    let mut ct = CodeTransform::new(source, &allocator);
    ct.prepend_left(len, "X");
    ct.append_left(len, "Y");
    assert_eq!(ct.build_string(), "ABCDEFXY");
}

#[test]
fn test_insert_interleaved_prepend_append() {
    // Alternating prepend_left and append_left at the same position
    // This pins the actual interleaving behavior
    let allocator = Allocator::default();
    let mut ct = CodeTransform::new("ABCDEF", &allocator);
    ct.append_left(3, "a1"); // append at pos 3
    ct.prepend_left(3, "p1"); // prepend at pos 3
    ct.append_left(3, "a2"); // append at pos 3
    ct.prepend_left(3, "p2"); // prepend at pos 3
    let result = ct.build_string();
    // Pin the actual output — append stacks FIFO, prepend stacks LIFO
    assert_eq!(&result[..3], "ABC"); // Original before position 3
    assert!(result.ends_with("DEF")); // Original after position 3
                                      // a1 before a2 (FIFO append)
    let a1_pos = result.find("a1").unwrap();
    let a2_pos = result.find("a2").unwrap();
    assert!(a1_pos < a2_pos, "a1 before a2 (FIFO append)");
    // Pin the exact result for regression detection
    assert_eq!(result, "ABCa1p1a2p2DEF");
}

#[test]
fn test_prepend_left_at_overwrite_boundary() {
    // Insert at the start boundary of an overwritten range
    let allocator = Allocator::default();
    let mut ct = CodeTransform::new("ABCDEF", &allocator);
    ct.overwrite(2, 4, "XX");
    ct.prepend_left(2, "Y");
    assert_eq!(ct.build_string(), "ABYXXEF");
}

#[test]
fn test_append_left_at_overwrite_boundary() {
    let allocator = Allocator::default();
    let mut ct = CodeTransform::new("ABCDEF", &allocator);
    ct.overwrite(2, 4, "XX");
    ct.append_left(2, "Y");
    assert_eq!(ct.build_string(), "ABYXXEF");
}

// ============================================================================
// TDD Safety-Net Tests: Move Operation Semantics
// Pin exact behavior before refactoring move_slice/move_wrapped
// ============================================================================

#[test]
fn test_move_wrapped_picks_up_pure_insertion_chunks() {
    // Insertions within moved range should travel with the move
    let allocator = Allocator::default();
    let mut ct = CodeTransform::new("ABCDEFGH", &allocator);
    ct.append_left(4, "X"); // Insert "X" between D and E
    assert_eq!(ct.build_string(), "ABCDXEFGH");
    // Move range [2,6) which includes C, D, inserted X, E, F
    ct.move_wrapped(2, 6, 0, "[", "]");
    let result = ct.build_string();
    assert!(result.contains("X"), "Insertion should travel with move");
    assert!(result.starts_with("["));
}

#[test]
fn test_move_slice_is_equivalent_to_move_wrapped_empty() {
    // move_slice(s,e,t) should produce identical output to move_wrapped(s,e,t,"","")
    let allocator1 = Allocator::default();
    let allocator2 = Allocator::default();
    let source = "ABCDEFGHIJKLMNOP";

    let mut ct1 = CodeTransform::new(source, &allocator1);
    ct1.move_slice(4, 10, 0);

    let mut ct2 = CodeTransform::new(source, &allocator2);
    ct2.move_wrapped(4, 10, 0, "", "");

    assert_eq!(ct1.build_string(), ct2.build_string());
}

#[test]
fn test_move_slice_is_equivalent_to_move_wrapped_empty_with_prior_edits() {
    // Same equivalence test but with prior overwrites
    let allocator1 = Allocator::default();
    let allocator2 = Allocator::default();
    let source = "ABCDEFGHIJKLMNOP";

    let mut ct1 = CodeTransform::new(source, &allocator1);
    ct1.overwrite(5, 8, "XXX");
    ct1.move_slice(4, 10, 0);

    let mut ct2 = CodeTransform::new(source, &allocator2);
    ct2.overwrite(5, 8, "XXX");
    ct2.move_wrapped(4, 10, 0, "", "");

    assert_eq!(ct1.build_string(), ct2.build_string());
}

#[test]
fn test_move_then_overwrite_at_boundary() {
    // Overwrite at the exact start boundary of a moved range
    let allocator = Allocator::default();
    let mut ct = CodeTransform::new("ABCDEFGHIJ", &allocator);
    ct.move_wrapped(4, 7, 0, "(", ")"); // Move "EFG" to start
    assert_eq!(ct.build_string(), "(EFG)ABCDHIJ");
    // Overwrite spanning from D into the (now-empty) moved region
    ct.overwrite(3, 5, "XX");
    // Moved chunks are skipped by overwrite, so this replaces D and the gap
    assert_eq!(ct.build_string(), "(EFG)ABCXXHIJ");
}

#[test]
fn test_move_large_range() {
    // Move 80% of the source to verify correctness at scale
    let allocator = Allocator::default();
    let source = "A".repeat(100);
    let mut ct = CodeTransform::new(&source, &allocator);
    ct.move_wrapped(10, 90, 0, "[", "]");
    let result = ct.build_string();
    // Should have: "[" + 80 A's + "]" + 10 A's (start) + 10 A's (end)
    assert_eq!(result.len(), 102); // 100 original + "[" + "]"
    assert!(result.starts_with("[AAAA"));
    assert!(result.ends_with("AAAA"));
}

#[test]
fn test_move_wrapped_multiple_adjacent_moves() {
    // Multiple adjacent moves shouldn't interfere
    let allocator = Allocator::default();
    let mut ct = CodeTransform::new("AABBCCDDEE", &allocator);
    ct.move_wrapped(0, 2, 10, "(a:", ")"); // Move "AA" to end
    ct.move_wrapped(4, 6, 10, "(c:", ")"); // Move "CC" to end
    let result = ct.build_string();
    assert!(result.contains("(a:AA)"), "First move should be present");
    assert!(result.contains("(c:CC)"), "Second move should be present");
    assert!(result.contains("BB"), "Unmoved content preserved");
    assert!(result.contains("DD"), "Unmoved content preserved");
    assert!(result.contains("EE"), "Unmoved content preserved");
}

// ============================================================================
// TDD Safety-Net Tests: Source Map Semantics
// Pin exact behavior before refactoring source map generation
// ============================================================================

#[test]
fn test_source_map_overwrite_identical_content_is_not_move() {
    // Overwriting with identical content should use overwrite mapping,
    // not move mapping. This tests the fragile string comparison in source_map.rs
    let allocator = Allocator::default();
    let mut ct = CodeTransform::new("Hello World", &allocator);
    ct.overwrite(6, 11, "World"); // Same content as original!
    let options = SourceMapOptions::new()
        .with_source("test.js")
        .include_content(true);
    let map = ct.generate_map(options);
    let sources: Vec<_> = map.get_sources().collect();
    assert_eq!(sources.len(), 1);
    // Map should be valid regardless of the mapping style
    let json = ct.generate_map_json(SourceMapOptions::new().with_source("test.js"));
    assert!(json.contains("\"mappings\""));
}

#[test]
fn test_source_map_moved_chunk_produces_valid_map() {
    // Moved multiline content should produce a valid source map
    let allocator = Allocator::default();
    let source = "line1\nline2\nline3\nline4";
    let mut ct = CodeTransform::new(source, &allocator);
    ct.move_wrapped(6, 17, 0, "/*start*/", "/*end*/"); // Move "line2\nline3"
    let options = SourceMapOptions::new()
        .with_source("test.js")
        .include_content(true);
    let map = ct.generate_map(options);
    let sources: Vec<_> = map.get_sources().collect();
    assert_eq!(sources.len(), 1);
    assert_eq!(map.get_source_content(0).unwrap(), source);
}

#[test]
fn test_source_map_pure_insertion_produces_valid_map() {
    // Inserted content has no source mapping
    let allocator = Allocator::default();
    let mut ct = CodeTransform::new("AB", &allocator);
    ct.prepend_left(1, "INSERTED");
    let options = SourceMapOptions::new()
        .with_source("test.js")
        .include_content(true);
    let map = ct.generate_map(options);
    let sources: Vec<_> = map.get_sources().collect();
    assert_eq!(sources.len(), 1);
}

#[test]
fn test_source_map_complex_scenario() {
    // Realistic scenario: overwrites + moves + insertions
    let allocator = Allocator::default();
    let source = "const x = defineProps<{a: string}>();\nconst y = 1;";
    let mut ct = CodeTransform::new(source, &allocator);
    ct.move_wrapped(22, 33, 0, "props:", ",\n");
    ct.overwrite(10, 22, "_props");
    ct.remove(33, 36);
    ct.prepend("// generated\n");

    let options = SourceMapOptions::new()
        .with_source("test.vue")
        .with_file("test.tsx")
        .include_content(true);
    let map = ct.generate_map(options);
    let sources: Vec<_> = map.get_sources().collect();
    assert_eq!(sources.len(), 1);
    assert_eq!(sources[0], "test.vue");
    assert_eq!(map.get_source_content(0).unwrap(), source);
}

// ============================================================================
// TDD Safety-Net Tests: Batch Operation Semantics
// Pin exact behavior before refactoring
// ============================================================================

#[test]
fn test_batch_overwrite_adjacent_ranges() {
    // Batch overwrites where one ends exactly where another begins
    let allocator = Allocator::default();
    let mut ct = CodeTransform::new("ABCDEFGHIJ", &allocator);
    ct.batch_overwrite(&[(2, 5, "XX"), (5, 8, "YY")]);
    assert_eq!(ct.build_string(), "ABXXYYIJ");
}

#[test]
fn test_batch_prepend_left_multiple_at_same_position() {
    // Multiple batch items targeting the same position
    let allocator = Allocator::default();
    let mut ct = CodeTransform::new("ABCDEF", &allocator);
    ct.batch_prepend_left_static(&[(3, "X"), (3, "Y")]);
    assert_eq!(ct.build_string(), "ABCXYDEF");
}

#[test]
fn test_batch_overwrite_empty_content_removal() {
    // Batch overwrite with empty content acts as removal
    // batch_overwrite skips emitting empty-content chunks for efficiency,
    // which means the original content in those ranges is removed
    let allocator = Allocator::default();
    let mut ct = CodeTransform::new("ABCDEFGHIJ", &allocator);
    ct.batch_overwrite(&[(2, 5, ""), (7, 9, "")]);
    // Removes "CDE" (2-5) and "HI" (7-9)
    assert_eq!(ct.build_string(), "ABFGJ");
}

#[test]
fn test_batch_overwrite_single_item() {
    // Batch with single item should behave like regular overwrite
    let allocator1 = Allocator::default();
    let allocator2 = Allocator::default();
    let source = "ABCDEFGHIJ";

    let mut ct1 = CodeTransform::new(source, &allocator1);
    ct1.overwrite(3, 7, "XX");

    let mut ct2 = CodeTransform::new(source, &allocator2);
    ct2.batch_overwrite(&[(3, 7, "XX")]);

    assert_eq!(ct1.build_string(), ct2.build_string());
}

#[test]
fn test_batch_prepend_left_at_chunk_boundaries() {
    // Batch prepend at position 0 and at source end
    let allocator = Allocator::default();
    let source = "ABCDEF";
    let len = source.len() as u32;
    let mut ct = CodeTransform::new(source, &allocator);
    ct.batch_prepend_left_static(&[(0, "START"), (len, "END")]);
    assert_eq!(ct.build_string(), "STARTABCDEFEND");
}

#[test]
fn test_batch_overwrite_preserves_unaffected_chunks() {
    // Batch overwrite with gaps should preserve content between overwrites
    let allocator = Allocator::default();
    let mut ct = CodeTransform::new("0123456789", &allocator);
    ct.batch_overwrite(&[(1, 3, "A"), (5, 7, "B"), (8, 9, "C")]);
    assert_eq!(ct.build_string(), "0A34B7C9");
}

// ============================================================================
// Size-of assertions — document and verify enum layout
// ============================================================================

#[test]
fn test_chunk_size() {
    use super::chunk::Chunk;
    // 4 explicit variants — largest (Overwritten/Moved) is u32 + u32 + &str = 24 bytes + tag = 32
    assert_eq!(
        std::mem::size_of::<Chunk>(),
        32,
        "Chunk enum size changed unexpectedly — update this test if intentional"
    );
}

// ============================================================================
// Output delta tracking — verifies build_string capacity is accurate
// ============================================================================

#[test]
fn test_output_delta_overwrite() {
    let allocator = Allocator::default();
    let mut ct = CodeTransform::new("Hello World", &allocator);
    // "World" (5 bytes) → "Rust" (4 bytes) = delta -1
    ct.overwrite(6, 11, "Rust");
    assert_eq!(ct.output_delta(), -1);
    let s = ct.build_string();
    assert_eq!(s, "Hello Rust");
    assert_eq!(
        s.len(),
        (ct.original().len() as i64 + ct.output_delta()) as usize
    );
}

#[test]
fn test_output_delta_insertions() {
    let allocator = Allocator::default();
    let mut ct = CodeTransform::new("AB", &allocator);
    ct.prepend_left(1, "x"); // +1
    ct.append_left(1, "y"); // +1
    ct.prepend_right(1, "z"); // +1
    ct.append_right(1, "w"); // +1
    assert_eq!(ct.output_delta(), 4);
    let s = ct.build_string();
    assert_eq!(
        s.len(),
        (ct.original().len() as i64 + ct.output_delta()) as usize
    );
}

#[test]
fn test_output_delta_remove() {
    let allocator = Allocator::default();
    let mut ct = CodeTransform::new("ABCDEF", &allocator);
    ct.remove(2, 4); // remove "CD" = delta -2
    assert_eq!(ct.output_delta(), -2);
    let s = ct.build_string();
    assert_eq!(s, "ABEF");
    assert_eq!(
        s.len(),
        (ct.original().len() as i64 + ct.output_delta()) as usize
    );
}

#[test]
fn test_output_delta_move_wrapped() {
    let allocator = Allocator::default();
    let mut ct = CodeTransform::new("ABCDEF", &allocator);
    ct.move_wrapped(2, 4, 0, "{", "}"); // prefix + suffix = +2
    assert_eq!(ct.output_delta(), 2);
    let s = ct.build_string();
    assert_eq!(s, "{CD}ABEF");
    assert_eq!(
        s.len(),
        (ct.original().len() as i64 + ct.output_delta()) as usize
    );
}

#[test]
fn test_output_delta_batch_overwrite() {
    let allocator = Allocator::default();
    let mut ct = CodeTransform::new("ABCDEFGHIJ", &allocator);
    ct.batch_overwrite(&[(1, 3, "xx"), (5, 7, "yyy")]); // +0 and +1 = +1
    assert_eq!(ct.output_delta(), 1);
    let s = ct.build_string();
    assert_eq!(
        s.len(),
        (ct.original().len() as i64 + ct.output_delta()) as usize
    );
}

#[test]
fn test_output_delta_batch_prepend_left_static() {
    let allocator = Allocator::default();
    let mut ct = CodeTransform::new("ABCDEF", &allocator);
    ct.batch_prepend_left_static(&[(1, "_ctx."), (3, "_ctx.")]); // 5+5 = +10
    assert_eq!(ct.output_delta(), 10);
    let s = ct.build_string();
    assert_eq!(
        s.len(),
        (ct.original().len() as i64 + ct.output_delta()) as usize
    );
}

#[test]
fn test_output_delta_complex_scenario() {
    let allocator = Allocator::default();
    let mut ct = CodeTransform::new("0123456789ABCDEF", &allocator);
    ct.overwrite(2, 4, "XX"); // 0 delta (2 → 2)
    ct.prepend_left(6, "INS"); // +3
    ct.remove(8, 10); // -2
    ct.move_wrapped(12, 14, 0, "<", ">"); // +2
    assert_eq!(ct.output_delta(), 3);
    let s = ct.build_string();
    assert_eq!(
        s.len(),
        (ct.original().len() as i64 + ct.output_delta()) as usize
    );
}

// ========================================================================
// Review findings: nested overwrite no-op delta + batch after prior overwrite
// ========================================================================

/// @ai-generated — Verify output_delta remains accurate after the nested-overwrite
/// no-op path (strict subset of an existing Overwritten chunk is skipped).
/// This exercises the delta reversal at code_transform.rs lines 481-488.
#[test]
fn test_output_delta_after_nested_overwrite_noop() {
    let allocator = Allocator::default();
    // Source: "defineProps<{x: number}>()"
    let source = "defineProps<{x: number}>()";
    let mut ct = CodeTransform::new(source, &allocator);

    // First overwrite: replace "defineProps<{x: number}>" (0..24) with "__props"
    ct.overwrite(0, 24, "__props");
    assert_eq!(ct.build_string(), "__props()");

    // Second overwrite: try to remove the generic "<{x: number}>" (11..24)
    // This is a strict subset of the already-overwritten range — should be a no-op
    ct.overwrite(11, 24, "");
    assert_eq!(ct.build_string(), "__props()");

    // Verify output_delta is accurate (capacity prediction matches actual length)
    let s = ct.build_string();
    assert_eq!(
        s.len(),
        (ct.original().len() as i64 + ct.output_delta()) as usize,
        "output_delta must remain accurate after nested no-op"
    );
}

/// @ai-generated — Verify output_delta after nested no-op with non-empty replacement.
#[test]
fn test_output_delta_after_nested_overwrite_noop_with_content() {
    let allocator = Allocator::default();
    let source = "ABCDEFGHIJ";
    let mut ct = CodeTransform::new(source, &allocator);

    // Overwrite [2,8) with "XY"
    ct.overwrite(2, 8, "XY");
    assert_eq!(ct.build_string(), "ABXYIJ");

    // Try to overwrite [4,6) (strict subset) with "ZZ" — should be no-op
    ct.overwrite(4, 6, "ZZ");
    assert_eq!(ct.build_string(), "ABXYIJ");

    let s = ct.build_string();
    assert_eq!(
        s.len(),
        (ct.original().len() as i64 + ct.output_delta()) as usize,
        "output_delta must remain accurate after nested no-op with content"
    );
}

/// @ai-generated — batch_overwrite after a prior overwrite: the Overwritten chunk
/// should pass through unchanged, and batch items targeting Original ranges
/// around it should work correctly.
#[test]
fn test_batch_overwrite_after_prior_overwrite() {
    let allocator = Allocator::default();
    // Source: "AABBCCDDEE"
    //          0123456789(10)
    let source = "AABBCCDDEE";
    let mut ct = CodeTransform::new(source, &allocator);

    // Prior overwrite: replace "CC" (4..6) with "XX"
    ct.overwrite(4, 6, "XX");
    assert_eq!(ct.build_string(), "AABBXXDDEE");

    // Batch overwrite: replace "BB" (2..4) and "DD" (6..8) — ranges around the prior overwrite
    ct.batch_overwrite(&[(2, 4, "YY"), (6, 8, "ZZ")]);
    assert_eq!(ct.build_string(), "AAYYXXZZEE");

    let s = ct.build_string();
    assert_eq!(
        s.len(),
        (ct.original().len() as i64 + ct.output_delta()) as usize,
        "output_delta must be accurate after batch_overwrite with prior overwrite"
    );
}

/// @ai-generated — batch_overwrite preserves existing Overwritten chunk
/// when batch items are on non-overlapping Original ranges.
#[test]
fn test_batch_overwrite_preserves_prior_overwrite_content() {
    let allocator = Allocator::default();
    let source = "abcdefghij";
    let mut ct = CodeTransform::new(source, &allocator);

    // Overwrite [3,6) with "XYZ"
    ct.overwrite(3, 6, "XYZ");
    assert_eq!(ct.build_string(), "abcXYZghij");

    // Batch: overwrite [0,2) and [8,10)
    ct.batch_overwrite(&[(0, 2, "11"), (8, 10, "22")]);
    assert_eq!(ct.build_string(), "11cXYZgh22");
}

/// @ai-generated — batch_overwrite with a fully contained range: the inner
/// overwrite is a no-op because its source region is already replaced by the
/// outer overwrite. This reproduces the overlap from resolve_whitespace
/// emitting a deletion for whitespace that the parent's tag extension already covers.
#[test]
fn test_batch_overwrite_contained_range_is_noop() {
    let allocator = Allocator::default();
    let mut ct = CodeTransform::new("0123456789", &allocator);
    // Outer: replaces [0,5) with "VNODE", inner: deletes [3,5) — fully contained
    ct.batch_overwrite(&[(0, 5, "VNODE"), (3, 5, "")]);
    assert_eq!(ct.build_string(), "VNODE56789");

    let s = ct.build_string();
    assert_eq!(
        s.len(),
        (ct.original().len() as i64 + ct.output_delta()) as usize,
        "output_delta must be accurate with contained overlap"
    );
}

/// @ai-generated — batch_overwrite with duplicate ranges at the same position:
/// second overwrite is a no-op because the region was already replaced.
/// This reproduces the gap-filling + whitespace-removal duplicate.
#[test]
fn test_batch_overwrite_duplicate_range() {
    let allocator = Allocator::default();
    let mut ct = CodeTransform::new("0123456789", &allocator);
    // Both delete [3,6) — second is redundant
    ct.batch_overwrite(&[(3, 6, ""), (3, 6, "")]);
    assert_eq!(ct.build_string(), "0126789");

    let s = ct.build_string();
    assert_eq!(
        s.len(),
        (ct.original().len() as i64 + ct.output_delta()) as usize,
        "output_delta must be accurate with duplicate ranges"
    );
}

/// @ai-generated — batch_overwrite with trailing contained range: the close
/// tag extension covers trailing whitespace. Inner deletion of the trailing
/// whitespace is a no-op.
#[test]
fn test_batch_overwrite_trailing_contained() {
    let allocator = Allocator::default();
    let mut ct = CodeTransform::new("0123456789", &allocator);
    // First: delete trailing whitespace [7,8), then outer replaces [7,10) with ")"
    ct.batch_overwrite(&[(7, 8, ""), (7, 10, ")")]);
    assert_eq!(ct.build_string(), "0123456)");

    let s = ct.build_string();
    assert_eq!(
        s.len(),
        (ct.original().len() as i64 + ct.output_delta()) as usize,
        "output_delta must be accurate with trailing contained overlap"
    );
}
