//! Unit tests for the JSDoc shallow-analysis helpers in [`super`].
//!
//! Extracted to a sibling file (the established `*_tests.rs` pattern) to keep
//! `jsdoc.rs` under the `no_oversize_files` architecture guard's line target.

use super::{extract_jsdoc_near_offset, parse_jsdoc_tag_type_payload};
use verter_type_expr::{PrimitiveName, TypeExpr};

#[test]
fn collect_jsdoc_typedefs_lowers_braced_typedef_to_alias_body() {
    use super::collect_jsdoc_typedefs;
    use oxc_allocator::Allocator;
    use oxc_parser::Parser;
    use oxc_span::SourceType;
    use verter_type_expr::ObjectMember;

    let source = "/** @typedef {{a: number}} Alias */\n";
    let allocator = Allocator::default();
    let ret = Parser::new(&allocator, source, SourceType::ts()).parse();
    let typedefs = collect_jsdoc_typedefs(&ret.program.comments, source);

    assert_eq!(typedefs.len(), 1, "one `@typedef` must be recovered");
    assert_eq!(typedefs[0].name, "Alias");
    // The brace payload `{a: number}` lowers to an object body with a single
    // `a: number` member — the SAME body a TS `type Alias = { a: number }`
    // produces via `lower_ts_type`.
    let TypeExpr::Object(object) = &typedefs[0].body else {
        panic!(
            "typedef body must lower to an object, got {:?}",
            typedefs[0].body
        );
    };
    assert_eq!(object.properties.len(), 1);
    match &object.properties[0] {
        ObjectMember::Property(prop) => {
            assert_eq!(prop.name, "a");
            assert_eq!(prop.ty, TypeExpr::Primitive(PrimitiveName::Number));
        }
        other => panic!("expected `a` property, got {other:?}"),
    }
}

#[test]
fn collect_jsdoc_typedefs_skips_payloadless_typedef() {
    use super::collect_jsdoc_typedefs;
    use oxc_allocator::Allocator;
    use oxc_parser::Parser;
    use oxc_span::SourceType;

    // The `@property`-aggregation form (`@typedef Foo` with no `{T}`) carries
    // no self-contained alias body; it must be skipped, not registered as an
    // empty/`Unknown` alias.
    let source = "/** @typedef Foo\n * @property {number} a\n */\n";
    let allocator = Allocator::default();
    let ret = Parser::new(&allocator, source, SourceType::ts()).parse();
    let typedefs = collect_jsdoc_typedefs(&ret.program.comments, source);
    assert!(
        typedefs.is_empty(),
        "a payload-less `@typedef Foo` must not be registered as an alias; got {typedefs:?}"
    );
}

// The member spans on a JSDoc-`{Type}`-lowered body must be in FILE
// coordinates (sliceable against the original source), NOT the synthetic
// `type __T = <payload>` wrapper's coordinates. typeinfo carries spans, not
// owned strings (owner directive `feedback_typeinfo_spans_not_strings`): any
// consumer that slices a JSDoc-typed member's name / type span must recover
// the correct source token.
//
// The `@typedef` sits at a NON-ZERO file offset (text precedes it) so a
// wrapper-local span and the true file span differ. Pre-fix the body was
// lowered against `type __T = {a: number}` and its spans were left in
// wrapper coordinates: `a`'s name span came out `12..13` and sliced to the
// `\n` / wrapper-prefix region of the source instead of the real `a` token.
// This test FAILS against that tree (it slices the wrong text) and PASSES
// once the producer rebases the spans into file coordinates.
#[test]
fn collect_jsdoc_typedefs_member_spans_are_file_coordinates() {
    use super::collect_jsdoc_typedefs;
    use oxc_allocator::Allocator;
    use oxc_parser::Parser;
    use oxc_span::SourceType;
    use verter_type_expr::ObjectMember;

    // `const x = 1;\n` is 13 bytes, so the JSDoc block — and the `{a: number}`
    // payload inside it — sits well past byte 0. A wrapper-local span (offset
    // by the 11-byte `type __T = ` prefix) would slice the WRONG source text.
    let source = "const x = 1;\n/** @typedef {{a: number}} Alias */\n";
    let allocator = Allocator::default();
    let ret = Parser::new(&allocator, source, SourceType::ts()).parse();
    let typedefs = collect_jsdoc_typedefs(&ret.program.comments, source);

    assert_eq!(typedefs.len(), 1, "one `@typedef` must be recovered");
    let TypeExpr::Object(object) = &typedefs[0].body else {
        panic!(
            "typedef body must lower to an object, got {:?}",
            typedefs[0].body
        );
    };
    let ObjectMember::Property(prop) = &object.properties[0] else {
        panic!("expected `a` property, got {:?}", object.properties[0]);
    };
    assert_eq!(prop.name, "a");

    let name_span = prop
        .spans
        .name
        .expect("the JSDoc-typed member must carry its name span");
    assert_eq!(
        name_span.slice(source),
        "a",
        "the member NAME span must slice the file to the `a` token; a wrapper-local span \
         slices the wrong source text (pre-fix it sliced `{:?}`)",
        source.get(12..13),
    );

    let type_span = prop
        .spans
        .type_annotation
        .expect("the JSDoc-typed member must carry its type-annotation span");
    assert_eq!(
        type_span.slice(source),
        "number",
        "the member TYPE span must slice the file to the `number` token; a wrapper-local span \
         slices the wrong source text",
    );

    // The full member-declaration span must also be file-correct.
    let decl_span = prop
        .spans
        .declaration
        .expect("the JSDoc-typed member must carry its declaration span");
    assert_eq!(
        decl_span.slice(source),
        "a: number",
        "the member DECLARATION span must slice the file to `a: number`",
    );
}

// The OTHER source-anchored entry point: a leading `@type {{a: number}}`
// value annotation (consumed by `extract_jsdoc_type_at_offset`) must also
// produce FILE-coordinate member spans. This caller drives off the block-span
// scanner, so it exercises a different code path than `collect_jsdoc_typedefs`
// above — both must rebase correctly. Pre-fix the `a` name span was
// wrapper-local and sliced the wrong source text.
#[test]
fn extract_jsdoc_type_at_offset_member_spans_are_file_coordinates() {
    use super::extract_jsdoc_type_at_offset;
    use verter_type_expr::ObjectMember;

    // The declaration name token `x` is what the caller anchors on; the
    // leading JSDoc `@type {{a: number}}` block precedes it. Leading text
    // pushes the payload well past byte 0.
    let source = "const PADDING = 0;\n/** @type {{a: number}} */\nconst x = { a: 1 };\n";
    let name_offset = source.find("x =").expect("the `x` binding is present") as u32;

    let ty = extract_jsdoc_type_at_offset(source, name_offset)
        .expect("the `@type {{a: number}}` annotation must lower to a type");
    let TypeExpr::Object(object) = &ty else {
        panic!("`@type {{a: number}}` must lower to an object, got {ty:?}");
    };
    let ObjectMember::Property(prop) = &object.properties[0] else {
        panic!("expected `a` property, got {:?}", object.properties[0]);
    };
    assert_eq!(prop.name, "a");

    let name_span = prop
        .spans
        .name
        .expect("the `@type` member must carry its name span");
    assert_eq!(
        name_span.slice(source),
        "a",
        "the `@type` member NAME span must slice the file to the `a` token",
    );
    let type_span = prop
        .spans
        .type_annotation
        .expect("the `@type` member must carry its type-annotation span");
    assert_eq!(
        type_span.slice(source),
        "number",
        "the `@type` member TYPE span must slice the file to the `number` token",
    );
}

// A genuine MULTI-LINE / `*`-decorated JSDoc `{Type}` payload has no single
// contiguous source region: `lower_jsdoc_tag_type` reconstructs it (stripping
// the `*` decorations) and lowers it through `parse_jsdoc_tag_type_payload(_,
// None)`, which clears every span. This test characterizes BOTH halves of
// that contract on a real public entry point (`extract_jsdoc_type_at_offset`):
//
//   (a) STRUCTURE preserved — the wrapped object still lowers to the right
//       two members (`a: number`, `b: string`).
//   (b) SPANS absent — every member span is `None` (honest absence, never a
//       wrong wrapper-local offset).
//
// It FAILS if the `None` path regressed (e.g. spans rebased instead of
// cleared → a member span would be `Some`) OR if `clear_spans` stopped
// dropping a member span. The single-line parity test cannot catch this: it
// clears spans AFTER the helper returns, so a broken in-helper `None` clear
// would still look clean there.
#[test]
fn extract_jsdoc_type_multiline_payload_preserves_structure_and_clears_spans() {
    use super::extract_jsdoc_type_at_offset;
    use verter_type_expr::ObjectMember;

    // The `@type` object literal spans three comment lines with leading `*`
    // decorations — there is no contiguous file slice for `{ a: number,
    // b: string }`, so the lowered spans must be cleared.
    let source = "const PADDING = 0;\n\
                  /**\n\
                  \x20* @type {{\n\
                  \x20*   a: number,\n\
                  \x20*   b: string\n\
                  \x20* }}\n\
                  \x20*/\n\
                  const x = { a: 1, b: 'two' };\n";
    let name_offset = source.find("x =").expect("the `x` binding is present") as u32;

    let ty = extract_jsdoc_type_at_offset(source, name_offset)
        .expect("the multi-line `@type` annotation must still lower to a type");

    // (a) Structure preserved: a two-member object.
    let TypeExpr::Object(object) = &ty else {
        panic!("multi-line `@type` object must lower to an object, got {ty:?}");
    };
    assert_eq!(
        object.properties.len(),
        2,
        "the reconstructed object must keep BOTH members; got {:?}",
        object.properties
    );
    let names: Vec<&str> = object
        .properties
        .iter()
        .map(|m| match m {
            ObjectMember::Property(prop) => prop.name.as_str(),
            other => panic!("expected named properties, got {other:?}"),
        })
        .collect();
    assert_eq!(
        names,
        vec!["a", "b"],
        "structure must be preserved across the reconstructed multi-line payload"
    );
    // The member TYPES survive too (structure, not just names).
    for prop in &object.properties {
        let ObjectMember::Property(prop) = prop else {
            unreachable!()
        };
        let expected = match prop.name.as_str() {
            "a" => TypeExpr::Primitive(PrimitiveName::Number),
            "b" => TypeExpr::Primitive(PrimitiveName::String),
            other => panic!("unexpected member {other}"),
        };
        assert_eq!(
            prop.ty, expected,
            "member `{}` must keep its lowered type",
            prop.name
        );
    }

    // (b) Spans absent on EVERY member — honest absence for a payload with no
    // contiguous source region. A surviving `Some(..)` span proves the `None`
    // path stopped clearing (it would slice the wrong source text).
    for prop in &object.properties {
        let ObjectMember::Property(prop) = prop else {
            unreachable!()
        };
        assert_eq!(
            prop.spans.name, None,
            "member `{}` NAME span must be cleared for a multi-line payload; a `Some` proves \
             the `None`/`clear_spans` path regressed",
            prop.name
        );
        assert_eq!(
            prop.spans.type_annotation, None,
            "member `{}` TYPE span must be cleared for a multi-line payload",
            prop.name
        );
        assert_eq!(
            prop.spans.declaration, None,
            "member `{}` DECLARATION span must be cleared for a multi-line payload",
            prop.name
        );
    }
}

// The `@param {T} name` source-anchored extractor must produce FILE-coordinate
// member spans for a SINGLE-LINE payload (which maps linearly onto the file).
// Mirrors the `@type` / `@typedef` `*_member_spans_are_file_coordinates`
// tests but exercises `extract_jsdoc_param_types_at_offset` — a distinct
// public entry. The payload sits at a NON-ZERO file offset, so a wrapper-local
// span (offset by the 11-byte `type __T = ` prefix) would slice the WRONG
// source text. This FAILS against the pre-batch base (wrapper-local spans)
// and PASSES once the producer rebases into file coordinates.
#[test]
fn extract_jsdoc_param_types_member_spans_are_file_coordinates() {
    use super::extract_jsdoc_param_types_at_offset;
    use verter_type_expr::ObjectMember;

    // Leading text + the JSDoc block push the `{a: number}` payload well past
    // byte 0; the function name `fn1` is the anchor the caller resolves on.
    let source =
        "const PADDING = 0;\n/** @param {{a: number}} p the param */\nfunction fn1(p) {}\n";
    let name_offset = source.find("fn1").expect("the `fn1` binding is present") as u32;

    let params = extract_jsdoc_param_types_at_offset(source, name_offset);
    assert_eq!(
        params.len(),
        1,
        "exactly one `@param` type must be recovered"
    );
    assert_eq!(params[0].0, "p", "the param name token must be `p`");

    let TypeExpr::Object(object) = &params[0].1 else {
        panic!(
            "`@param {{a: number}}` must lower to an object, got {:?}",
            params[0].1
        );
    };
    let ObjectMember::Property(prop) = &object.properties[0] else {
        panic!("expected `a` property, got {:?}", object.properties[0]);
    };
    assert_eq!(prop.name, "a");

    let name_span = prop
        .spans
        .name
        .expect("the `@param` member must carry its name span");
    assert_eq!(
        name_span.slice(source),
        "a",
        "the `@param` member NAME span must slice the file to the `a` token; a wrapper-local \
         span slices the wrong source text",
    );
    let type_span = prop
        .spans
        .type_annotation
        .expect("the `@param` member must carry its type-annotation span");
    assert_eq!(
        type_span.slice(source),
        "number",
        "the `@param` member TYPE span must slice the file to the `number` token",
    );
}

// The `@returns {T}` source-anchored extractor must likewise produce
// FILE-coordinate member spans. Exercises `extract_jsdoc_return_type_at_offset`
// — another distinct public entry. NON-ZERO file offset; a wrapper-local span
// would slice the wrong source text. FAILS against the pre-batch base, PASSES
// after the rebase.
#[test]
fn extract_jsdoc_return_type_member_spans_are_file_coordinates() {
    use super::extract_jsdoc_return_type_at_offset;
    use verter_type_expr::ObjectMember;

    let source =
        "const PADDING = 0;\n/** @returns {{b: string}} the result */\nfunction fn2() {}\n";
    let name_offset = source.find("fn2").expect("the `fn2` binding is present") as u32;

    let ty = extract_jsdoc_return_type_at_offset(source, name_offset)
        .expect("the `@returns {{b: string}}` annotation must lower to a type");
    let TypeExpr::Object(object) = &ty else {
        panic!("`@returns {{b: string}}` must lower to an object, got {ty:?}");
    };
    let ObjectMember::Property(prop) = &object.properties[0] else {
        panic!("expected `b` property, got {:?}", object.properties[0]);
    };
    assert_eq!(prop.name, "b");

    let name_span = prop
        .spans
        .name
        .expect("the `@returns` member must carry its name span");
    assert_eq!(
        name_span.slice(source),
        "b",
        "the `@returns` member NAME span must slice the file to the `b` token; a wrapper-local \
         span slices the wrong source text",
    );
    let type_span = prop
        .spans
        .type_annotation
        .expect("the `@returns` member must carry its type-annotation span");
    assert_eq!(
        type_span.slice(source),
        "string",
        "the `@returns` member TYPE span must slice the file to the `string` token",
    );
}

#[test]
fn jsdoc_block_spans_extend_tag_text_through_continuation_lines() {
    use super::jsdoc_block_spans_at_offset;
    // `@deprecated` text continues onto a second line; the tag's text span
    // must cover BOTH lines (pre-fix it stopped at line 1). The description
    // span must still stop before the first tag.
    let source = "/**\n * a description line.\n * @deprecated first line of reason\n * second \
                  line of reason\n */\nexport const target = 1;\n";
    let target_start = source.find("target").expect("target name present") as u32;
    let spans = jsdoc_block_spans_at_offset(source, target_start)
        .expect("the JSDoc block governing `target` must produce spans");

    assert_eq!(spans.tags.len(), 1, "exactly one `@deprecated` tag");
    let tag = &spans.tags[0];
    assert_eq!(
        &source[tag.name.start as usize..tag.name.end as usize],
        "deprecated"
    );
    let text_span = tag.text.expect("the `@deprecated` tag carries text");
    let text = &source[text_span.start as usize..text_span.end as usize];
    assert!(
        text.contains("first line of reason"),
        "tag text span must cover the first line; got {text:?}"
    );
    assert!(
        text.contains("second line of reason"),
        "tag text span must cover the CONTINUATION line — a span stopping at line 1 proves the \
         continuation was dropped; got {text:?}"
    );

    // The description span must NOT swallow the tag.
    let desc = spans.description.expect("description span present");
    let desc_text = &source[desc.start as usize..desc.end as usize];
    assert_eq!(desc_text, "a description line.");
}

#[test]
fn parse_jsdoc_tag_type_payload_lowers_primitive_keyword() {
    let expr = parse_jsdoc_tag_type_payload("string", None);
    assert_eq!(
        expr,
        TypeExpr::Primitive(PrimitiveName::String),
        "primitive JSDoc payload should lower to the matching TypeExpr primitive"
    );
}

#[test]
fn parse_jsdoc_tag_type_payload_lowers_array_with_element_type() {
    // OXC lowers `Array<number>` to `TypeExpr::Array { element }`,
    // not a `Ref<Array, [number]>`. The lowering is canonical: any
    // `Array<T>` / `T[]` / `ReadonlyArray<T>` collapses into `Array`.
    let expr = parse_jsdoc_tag_type_payload("Array<number>", None);
    match &expr {
        TypeExpr::Array { element, .. } => {
            assert_eq!(&**element, &TypeExpr::Primitive(PrimitiveName::Number));
        }
        other => panic!("expected Array<number>, got {other:?}"),
    }
}

#[test]
fn parse_jsdoc_tag_type_payload_lowers_union() {
    let expr = parse_jsdoc_tag_type_payload("string | number", None);
    match &expr {
        TypeExpr::Union(members) => {
            assert_eq!(members.len(), 2, "union must lower with two members");
            assert!(members
                .iter()
                .any(|m| matches!(m, TypeExpr::Primitive(PrimitiveName::String))));
            assert!(members
                .iter()
                .any(|m| matches!(m, TypeExpr::Primitive(PrimitiveName::Number))));
        }
        other => panic!("expected Union, got {other:?}"),
    }
}

#[test]
fn parse_jsdoc_tag_type_payload_unknown_for_empty_input() {
    let expr = parse_jsdoc_tag_type_payload("", None);
    match &expr {
        TypeExpr::Unknown { raw } => assert_eq!(raw.as_str(), "", "empty input keeps empty raw"),
        other => panic!("expected Unknown for empty payload, got {other:?}"),
    }
}

#[test]
fn extract_jsdoc_near_offset_skips_export_modifier_tokens() {
    let source = r#"
/** Description of the Props interface.
 * @deprecated Use NewProps instead.
 */
export interface Props { a: string }
"#;
    let target_start = source
        .find("interface Props")
        .expect("interface keyword should exist") as u32;

    let (description, tags) = extract_jsdoc_near_offset(source, target_start);

    assert_eq!(
        description.as_deref(),
        Some("Description of the Props interface.")
    );
    assert!(tags.iter().any(|tag| tag.name == "deprecated"));
}

#[test]
fn extract_jsdoc_near_offset_skips_multiple_declaration_modifiers() {
    let source = r#"
/** Description of the Value class. */
export declare abstract class Value {}
"#;
    let target_start = source
        .find("class Value")
        .expect("class keyword should exist") as u32;

    let (description, tags) = extract_jsdoc_near_offset(source, target_start);

    assert_eq!(
        description.as_deref(),
        Some("Description of the Value class.")
    );
    assert!(tags.is_empty());
}

#[test]
fn parse_jsdoc_preserves_newlines_between_description_lines() {
    let raw = r#"/**
     * When type is "single", allows closing content when clicking trigger for an open item.
     * When type is "multiple", this prop has no effect.
     */"#;
    let (description, _) = super::parse_jsdoc(raw);
    assert_eq!(
        description.as_deref(),
        Some("When type is \"single\", allows closing content when clicking trigger for an open item.\nWhen type is \"multiple\", this prop has no effect.")
    );
}

#[test]
fn parse_jsdoc_preserves_paragraph_breaks() {
    let raw = r#"/**
     * The default active value of the item(s).
     *
     * Use when you do not need to control the state of the item(s).
     */"#;
    let (description, _) = super::parse_jsdoc(raw);
    assert_eq!(
        description.as_deref(),
        Some("The default active value of the item(s).\n\nUse when you do not need to control the state of the item(s).")
    );
}

#[test]
fn parse_jsdoc_single_line_unchanged() {
    let raw = "/** Simple description. */";
    let (description, _) = super::parse_jsdoc(raw);
    assert_eq!(description.as_deref(), Some("Simple description."));
}
