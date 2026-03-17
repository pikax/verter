use super::*;

fn parse(source: &str) -> VForParseResult<'static> {
    let allocator = Box::leak(Box::new(Allocator::default()));
    parse_vfor(allocator, source, SourceType::tsx())
}

#[test]
fn test_simple_of() {
    let result = parse("item of items");
    assert!(result.is_ok());
    assert!(result.is_of);
    assert_eq!(result.left_offset, 0);
    assert_eq!(result.right_offset, 8); // "item of " = 8 chars

    // Check left is an identifier
    if let Some(Expression::Identifier(id)) = &result.left {
        assert_eq!(id.name.as_str(), "item");
        // Span should be 0-4 in the left string
        assert_eq!(id.span.start, 0);
        assert_eq!(id.span.end, 4);
    } else {
        panic!("Expected Identifier, got {:?}", result.left);
    }

    // Check right is an identifier with adjusted spans
    if let Some(Expression::Identifier(id)) = &result.right {
        assert_eq!(id.name.as_str(), "items");
        // Spans are now adjusted to reflect original source positions
        assert_eq!(id.span.start, 8); // "item of " = 8 chars
        assert_eq!(id.span.end, 13); // "item of items" = 13 chars
    } else {
        panic!("Expected Identifier expression");
    }
}

#[test]
fn test_simple_in() {
    let result = parse("item in items");
    assert!(result.is_ok());
    assert!(!result.is_of);
    assert_eq!(result.right_offset, 8); // "item in " = 8 chars
}

#[test]
fn test_destructuring_object() {
    let result = parse("{ id, name } of items");
    assert!(result.is_ok());
    assert!(result.is_of);

    // Check left is an object expression
    if let Some(Expression::ObjectExpression(_)) = &result.left {
        // OK
    } else {
        panic!("Expected ObjectExpression, got {:?}", result.left);
    }
}

#[test]
fn test_destructuring_array() {
    let result = parse("[first, second] of items");
    assert!(result.is_ok());
    assert!(result.is_of);

    // Check left is an array expression
    if let Some(Expression::ArrayExpression(_)) = &result.left {
        // OK
    } else {
        panic!("Expected ArrayExpression, got {:?}", result.left);
    }
}

#[test]
fn test_with_parentheses() {
    // Vue's multi-variable syntax - now properly handled!
    let result = parse("(item, index) of items");
    assert!(result.is_ok());
    assert!(result.is_of);

    // Check left is a parenthesized sequence expression
    if let Some(Expression::ParenthesizedExpression(paren)) = &result.left {
        if let Expression::SequenceExpression(seq) = &paren.expression {
            assert_eq!(seq.expressions.len(), 2);
            // First should be "item"
            if let Expression::Identifier(id) = &seq.expressions[0] {
                assert_eq!(id.name.as_str(), "item");
            }
            // Second should be "index"
            if let Expression::Identifier(id) = &seq.expressions[1] {
                assert_eq!(id.name.as_str(), "index");
            }
        } else {
            panic!("Expected SequenceExpression inside parentheses");
        }
    } else {
        panic!("Expected ParenthesizedExpression, got {:?}", result.left);
    }
}

#[test]
fn test_index_key_value() {
    // Vue's multi-variable syntax with three values
    let result = parse("(value, key, index) in obj");
    assert!(result.is_ok());
    assert!(!result.is_of);

    // Check left is a parenthesized sequence expression with 3 items
    if let Some(Expression::ParenthesizedExpression(paren)) = &result.left {
        if let Expression::SequenceExpression(seq) = &paren.expression {
            assert_eq!(seq.expressions.len(), 3);
        } else {
            panic!("Expected SequenceExpression");
        }
    } else {
        panic!("Expected ParenthesizedExpression");
    }
}

#[test]
fn test_member_expression_iterable() {
    let result = parse("item of data.items");
    assert!(result.is_ok());
    assert!(result.is_of);

    // Check right is a member expression
    if let Some(Expression::StaticMemberExpression(_)) = &result.right {
        // OK
    } else {
        panic!("Expected StaticMemberExpression, got {:?}", result.right);
    }
}

#[test]
fn test_function_call_iterable() {
    let result = parse("item of getItems()");
    assert!(result.is_ok());

    // Check right is a call expression
    if let Some(Expression::CallExpression(_)) = &result.right {
        // OK
    } else {
        panic!("Expected CallExpression");
    }
}

#[test]
fn test_empty_input() {
    let result = parse("");
    assert!(!result.is_ok());
    assert!(result.left.is_none());
    assert!(result.right.is_none());
}

#[test]
fn test_missing_separator() {
    let result = parse("item items");
    assert!(!result.is_ok());
    assert!(!result.left_errors.is_empty());
}

#[test]
fn test_typescript_assertion() {
    let result = parse("item of (items as Item[])");
    assert!(result.is_ok());
    assert!(result.is_of);

    // Check right contains type assertion
    if let Some(Expression::ParenthesizedExpression(paren)) = &result.right {
        if let Expression::TSAsExpression(_) = &paren.expression {
            // OK
        } else {
            panic!("Expected TSAsExpression inside parentheses");
        }
    } else {
        panic!("Expected ParenthesizedExpression");
    }
}

#[test]
fn test_span_offset_calculation() {
    // Test that spans correctly reflect original positions
    let result = parse("item of items");
    assert!(result.is_ok());

    // Left side "item" - spans are relative to substring (offset 0)
    if let Some(Expression::Identifier(id)) = &result.left {
        assert_eq!(id.span.start, 0);
        assert_eq!(id.span.end, 4);
        // For absolute position: add left_offset (always 0)
        assert_eq!(id.span.start + result.left_offset, 0);
        assert_eq!(id.span.end + result.left_offset, 4);
    }

    // Right side "items" - spans are now pre-adjusted to original positions
    if let Some(Expression::Identifier(id)) = &result.right {
        // Spans already reflect original source positions
        assert_eq!(id.span.start, 8);
        assert_eq!(id.span.end, 13);
    }
}

#[test]
fn test_complex_destructuring_with_index() {
    let result = parse("({ id, name }, index) of items");
    assert!(result.is_ok());

    // Left should be a parenthesized sequence with object destructuring
    if let Some(Expression::ParenthesizedExpression(paren)) = &result.left {
        if let Expression::SequenceExpression(seq) = &paren.expression {
            assert_eq!(seq.expressions.len(), 2);
            // First should be object expression
            assert!(matches!(
                &seq.expressions[0],
                Expression::ObjectExpression(_)
            ));
            // Second should be identifier "index"
            if let Expression::Identifier(id) = &seq.expressions[1] {
                assert_eq!(id.name.as_str(), "index");
            }
        }
    }
}

#[test]
fn test_array_iterable() {
    let result = parse("item of [1, 2, 3]");
    assert!(result.is_ok());

    // Right should be an array expression
    if let Some(Expression::ArrayExpression(arr)) = &result.right {
        assert_eq!(arr.elements.len(), 3);
    } else {
        panic!("Expected ArrayExpression");
    }
}

#[test]
fn test_range_expression() {
    // Common Vue pattern with computed range
    let result = parse("n of Array(10).keys()");
    assert!(result.is_ok());

    // Right should be a call expression chain
    if let Some(Expression::CallExpression(_)) = &result.right {
        // OK
    } else {
        panic!("Expected CallExpression");
    }
}

#[test]
fn test_object_literal_with_shorthand() {
    // Object literal with shorthand properties on the right side
    // This tests the span adjustment for shorthand object properties
    let result = parse("item of [{ foo }, { bar }]");
    assert!(result.is_ok());

    // Right should be an array of objects
    if let Some(Expression::ArrayExpression(arr)) = &result.right {
        assert_eq!(arr.elements.len(), 2);
    } else {
        panic!("Expected ArrayExpression, got {:?}", result.right);
    }
}

#[test]
fn test_object_literal_iterable() {
    // Object expression as the iterable
    let result = parse("item of { a: 1, b: 2 }");
    assert!(result.is_ok());

    // Right should be an object expression
    if let Some(Expression::ObjectExpression(obj)) = &result.right {
        assert_eq!(obj.properties.len(), 2);
    } else {
        panic!("Expected ObjectExpression, got {:?}", result.right);
    }
}

#[test]
fn test_mixed_object_properties() {
    // Object with both shorthand and regular properties
    let result = parse("key of { foo, bar: baz, qux }");
    assert!(result.is_ok());

    if let Some(Expression::ObjectExpression(obj)) = &result.right {
        assert_eq!(obj.properties.len(), 3);
    } else {
        panic!("Expected ObjectExpression, got {:?}", result.right);
    }
}

// ── parse_vfor_sliced ──────────────────────────────────────────

/// @ai-generated - Sliced parse adjusts all spans to be file-relative.
#[test]
fn test_sliced_span_adjustment() {
    let allocator = Box::leak(Box::new(Allocator::default()));
    //               0         1         2         3
    //               0123456789012345678901234567890123
    let input = r#"<div v-for="item of items"></div>"#;
    // "item of items" is at bytes 12..25
    let result = parse_vfor_sliced(allocator, Span::new(12, 25), input, SourceType::tsx());

    assert!(result.is_ok());
    assert!(result.is_of);
    assert_eq!(result.left_offset, 12);
    assert_eq!(result.right_offset, 20); // 12 + 8 ("item of " = 8 chars)

    // Left "item" should be at file position 12..16
    if let Some(Expression::Identifier(id)) = &result.left {
        assert_eq!(id.name.as_str(), "item");
        assert_eq!(id.span.start, 12);
        assert_eq!(id.span.end, 16);
    } else {
        panic!("Expected Identifier");
    }

    // Right "items" should be at file position 20..25
    if let Some(Expression::Identifier(id)) = &result.right {
        assert_eq!(id.name.as_str(), "items");
        assert_eq!(id.span.start, 20);
        assert_eq!(id.span.end, 25);
    } else {
        panic!("Expected Identifier");
    }
}

/// @ai-generated - Sliced parse with offset zero matches raw parse.
#[test]
fn test_sliced_zero_offset_matches_raw() {
    let allocator = Box::leak(Box::new(Allocator::default()));
    let source = "item of items";
    let raw = parse_vfor(allocator, source, SourceType::tsx());
    let sliced = parse_vfor_sliced(
        allocator,
        Span::new(0, source.len() as u32),
        source,
        SourceType::tsx(),
    );

    assert_eq!(raw.is_of, sliced.is_of);
    assert_eq!(raw.left_offset, sliced.left_offset);
    assert_eq!(raw.right_offset, sliced.right_offset);
}

/// @ai-generated - Sliced parse with empty span returns empty result.
#[test]
fn test_sliced_empty_span() {
    let allocator = Box::leak(Box::new(Allocator::default()));
    let result = parse_vfor_sliced(allocator, Span::new(5, 5), "some input", SourceType::tsx());
    assert!(!result.is_ok());
    assert!(result.left.is_none());
    assert!(result.right.is_none());
}

// ── parse_vfor_with_bindings_sliced ────────────────────────────

/// @ai-generated - Bindings sliced adjusts all spans to file-relative.
#[test]
fn test_bindings_sliced_span_adjustment() {
    let allocator = Box::leak(Box::new(Allocator::default()));
    //               0         1         2         3
    //               0123456789012345678901234567890123
    let input = r#"<div v-for="item of items"></div>"#;
    let wb = parse_vfor_with_bindings_sliced(
        allocator,
        Span::new(12, 25),
        input,
        SourceType::tsx(),
        &[],
    );

    assert!(wb.is_ok());

    // Local "item" should be at file position 12..16
    assert_eq!(wb.locals.len(), 1);
    assert_eq!(wb.locals[0].start, 12);
    assert_eq!(wb.locals[0].end, 16);
    assert_eq!(wb.locals[0].slice(input), "item");

    // Reference "items" should be at file position 20..25
    assert_eq!(wb.references.len(), 1);
    assert_eq!(wb.references[0].start, 20);
    assert_eq!(wb.references[0].end, 25);
    assert_eq!(wb.references[0].slice(input), "items");
}

/// @ai-generated - Bindings sliced with zero offset matches raw.
#[test]
fn test_bindings_sliced_zero_offset() {
    let allocator = Box::leak(Box::new(Allocator::default()));
    let source = "(item, index) of items";
    let raw = parse_vfor_with_bindings(allocator, source, SourceType::tsx(), &[]);
    let sliced = parse_vfor_with_bindings_sliced(
        allocator,
        Span::new(0, source.len() as u32),
        source,
        SourceType::tsx(),
        &[],
    );

    assert_eq!(raw.locals.len(), sliced.locals.len());
    assert_eq!(raw.references.len(), sliced.references.len());

    // Spans should match since offset is 0
    for (r, s) in raw.locals.iter().zip(sliced.locals.iter()) {
        assert_eq!(r.start, s.start);
        assert_eq!(r.end, s.end);
    }
}

/// @ai-generated - Bindings sliced with destructuring and offset.
#[test]
fn test_bindings_sliced_destructuring() {
    let allocator = Box::leak(Box::new(Allocator::default()));
    let input = "prefix { id, name } of data.items suffix";
    //           0123456 = 7 bytes prefix
    let start = 7u32;
    let end = start + "{ id, name } of data.items".len() as u32;
    let wb = parse_vfor_with_bindings_sliced(
        allocator,
        Span::new(start, end),
        input,
        SourceType::tsx(),
        &[],
    );

    assert!(wb.is_ok());
    assert_eq!(wb.locals.len(), 2); // id, name

    // All local spans should be within [start, end)
    for s in &wb.locals {
        assert!(s.start >= start, "Local span start {} < {}", s.start, start);
        assert!(s.end <= end, "Local span end {} > {}", s.end, end);
    }

    // Reference "data" should be within [start, end)
    assert!(!wb.references.is_empty());
    for s in &wb.references {
        assert!(s.start >= start);
        assert!(s.end <= end);
    }
}

/// @ai-generated - Ignored identifiers are excluded from references.
#[test]
fn test_bindings_ignored_identifiers() {
    let allocator = Box::leak(Box::new(Allocator::default()));
    let source = "item of ignoredItems";
    let ignored: Vec<&str> = vec!["ignoredItems"];
    let wb = parse_vfor_with_bindings(allocator, source, SourceType::tsx(), &ignored);

    assert!(wb.is_ok());
    assert!(wb.references.is_empty());
}

/// @ai-generated - References from computed member expressions must be sorted by position.
///
/// Regression test: `collections[activeIndex].data` produces two references
/// (`collections` and `activeIndex`). Since they are collected via FxHashSet,
/// iteration order is non-deterministic. Downstream consumers
/// (`prefix_vfor_references_into`) require ascending order to avoid panics.
#[test]
fn test_bindings_references_sorted_by_position() {
    let allocator = Box::leak(Box::new(Allocator::default()));
    let source = "item in collections[activeIndex].data";
    let wb = parse_vfor_with_bindings(allocator, source, SourceType::tsx(), &[]);

    assert!(wb.is_ok());
    assert_eq!(
        wb.references.len(),
        2,
        "Should have 2 references (collections, activeIndex), got: {:?}",
        wb.references
            .iter()
            .map(|s| s.slice(source))
            .collect::<Vec<_>>()
    );

    // Verify references are sorted by start position
    for pair in wb.references.windows(2) {
        assert!(
            pair[0].start <= pair[1].start,
            "References must be sorted by start position, but got {:?} before {:?} \
                 (names: '{}' before '{}')",
            pair[0],
            pair[1],
            pair[0].slice(source),
            pair[1].slice(source),
        );
    }
}
