//! Member-key indexing of wide declaration headers: interface, type-literal
//! and object-literal member headers keep exact source order and key
//! identity while costing linear key work.

use super::*;
use oxc_allocator::Allocator;
use oxc_span::SourceType;
use verter_parser::oxc_parse::Parser;
use verter_type_expr::TypeAuthoredPropertyKey;

fn index_for(source: &str) -> DeclHeaderIndex {
    let allocator = Allocator::default();
    let ret = Parser::new(&allocator, source, SourceType::ts()).parse();
    assert!(!ret.fatal_error, "fixture must parse");
    build_decl_header_index(&ret.program, source)
}

/// Reference first-wins union: keep a key's first header.
fn reference_first_wins(offered: &[MemberHeader]) -> Vec<MemberHeader> {
    let mut out: Vec<MemberHeader> = Vec::new();
    for header in offered {
        if !out.iter().any(|existing| existing.key == header.key) {
            out.push(header.clone());
        }
    }
    out
}

/// Reference last-wins object order: a repeated key moves to its last
/// occurrence and takes that header.
fn reference_last_wins(offered: &[MemberHeader]) -> Vec<MemberHeader> {
    let mut out: Vec<MemberHeader> = Vec::new();
    for header in offered {
        out.retain(|existing| existing.key != header.key);
        out.push(header.clone());
    }
    out
}

fn wide_interface(width: usize) -> String {
    let members: String = (0..width).map(|i| format!("  m{i}: string;\n")).collect();
    format!("interface Wide {{\n{members}}}\n")
}

fn wide_type_literal(width: usize) -> String {
    let members: String = (0..width).map(|i| format!("  m{i}: string;\n")).collect();
    format!("type Wide = {{\n{members}}};\n")
}

fn wide_object_literal(width: usize) -> String {
    let members: String = (0..width).map(|i| format!("  m{i}: {i},\n")).collect();
    format!("const wide = {{\n{members}}};\n")
}

fn type_members(index: &DeclHeaderIndex) -> &MemberHeaderList {
    &index.type_header("Wide").expect("Wide").member_headers
}

fn object_members(index: &DeclHeaderIndex) -> &MemberHeaderList {
    &index
        .value_header("wide")
        .expect("wide")
        .object_member_headers
}

type Fixture = fn(usize) -> String;
type Members = for<'i> fn(&'i DeclHeaderIndex) -> &'i MemberHeaderList;

#[test]
fn wide_headers_cost_linear_key_work() {
    let widths = [128_u64, 256, 512, 1024];
    let shapes: [(&str, Fixture, Members); 3] = [
        ("interface", wide_interface, type_members),
        ("type literal", wide_type_literal, type_members),
        ("object literal", wide_object_literal, object_members),
    ];
    for (shape, fixture, members_of) in shapes {
        let work: Vec<u64> = widths
            .iter()
            .map(|&width| {
                let index = index_for(&fixture(width as usize));
                let members = members_of(&index);
                assert_eq!(members.len() as u64, width, "{shape}: every member indexed");
                assert!(
                    members.key_probes() <= 2 * width,
                    "{shape} width {width}: {} key operations exceed two per member",
                    members.key_probes()
                );
                members.key_probes()
            })
            .collect();
        for pair in work.windows(2) {
            assert_eq!(
                pair[1],
                2 * pair[0],
                "{shape}: doubling the width doubles key work"
            );
        }
    }
}

fn collect_literal_members(ty: &TSType<'_>, source: &str, out: &mut Vec<MemberHeader>) {
    match ty {
        TSType::TSTypeLiteral(literal) => out.extend(
            literal
                .members
                .iter()
                .filter_map(|sig| interface_member_header(sig, source)),
        ),
        TSType::TSIntersectionType(intersection) => {
            for part in &intersection.types {
                collect_literal_members(part, source, out);
            }
        }
        TSType::TSParenthesizedType(paren) => {
            collect_literal_members(&paren.type_annotation, source, out);
        }
        _ => {}
    }
}

fn object_property_header(prop: &ObjectPropertyKind<'_>, source: &str) -> Option<MemberHeader> {
    let ObjectPropertyKind::ObjectProperty(p) = prop else {
        return None;
    };
    let accessor_or_method = p.method || !matches!(p.kind, oxc_ast::ast::PropertyKind::Init);
    Some(MemberHeader {
        key: lower_property_key(&p.key, source),
        method_kind: accessor_or_method.then_some(match p.kind {
            oxc_ast::ast::PropertyKind::Get => ObjectMethodKind::Get,
            oxc_ast::ast::PropertyKind::Set => ObjectMethodKind::Set,
            oxc_ast::ast::PropertyKind::Init => ObjectMethodKind::Method,
        }),
        has_implementation_body: accessor_or_method,
        optional: false,
        readonly: false,
    })
}

/// Merged interfaces, intersected type literals and object literals with
/// repeated, numeric-spelled and symbol keys keep the same key identity and
/// order as a per-member scan over the authored members.
#[test]
fn duplicate_numeric_and_symbol_keys_keep_identity_and_order() {
    let source = r#"
declare const sym: unique symbol;
interface Merged {
  a: string;
  1: string;
  "1": number;
  [Symbol.iterator](): void;
  [sym]: string;
  a?: number;
}
interface Merged {
  b: string;
  0x1: boolean;
  [sym]: number;
  a: boolean;
  "2": string;
}
type Lit = { a: string; 1: number } & ({ "1": string; a?: number; [sym]: string } & { 2: string; "2": number });
const obj = {
  a: 1,
  1: 2,
  "1": 3,
  [sym]: 4,
  get b() { return 1; },
  a() {},
  0x1: 5,
  b: 6,
  [Symbol.iterator]() {},
  "a": 7,
};
"#;
    let allocator = Allocator::default();
    let ret = Parser::new(&allocator, source, SourceType::ts()).parse();
    assert!(!ret.fatal_error, "fixture must parse");
    let index = build_decl_header_index(&ret.program, source);

    let mut merged_offered = Vec::new();
    let mut literal_offered = Vec::new();
    let mut object_offered = Vec::new();
    for statement in &ret.program.body {
        match statement {
            Statement::TSInterfaceDeclaration(decl) => merged_offered.extend(
                decl.body
                    .body
                    .iter()
                    .filter_map(|sig| interface_member_header(sig, source)),
            ),
            Statement::TSTypeAliasDeclaration(decl) => {
                collect_literal_members(&decl.type_annotation, source, &mut literal_offered);
            }
            Statement::VariableDeclaration(decl) => {
                for declarator in &decl.declarations {
                    if let Some(Expression::ObjectExpression(obj)) = &declarator.init {
                        object_offered.extend(
                            obj.properties
                                .iter()
                                .filter_map(|prop| object_property_header(prop, source)),
                        );
                    }
                }
            }
            _ => {}
        }
    }

    let merged = &index.type_header("Merged").expect("Merged").member_headers;
    let literal = &index.type_header("Lit").expect("Lit").member_headers;
    let object = &index
        .value_header("obj")
        .expect("obj")
        .object_member_headers;
    assert_eq!(
        merged.as_slice(),
        reference_first_wins(&merged_offered).as_slice()
    );
    assert_eq!(
        literal.as_slice(),
        reference_first_wins(&literal_offered).as_slice()
    );
    assert_eq!(
        object.as_slice(),
        reference_last_wins(&object_offered).as_slice()
    );

    // The fixtures exercise duplicates in every shape.
    assert!(merged.len() < merged_offered.len());
    assert!(literal.len() < literal_offered.len());
    assert!(object.len() < object_offered.len());

    // The merged interface keeps its first `a` header (required) first;
    // the object literal's `a` lands at its last authored position.
    let a = TypeAuthoredPropertyKey::string("a");
    let merged_a = merged.get(&a).expect("merged a");
    assert!(!merged_a.optional);
    assert!(std::ptr::eq(merged_a, &merged[0]));
    assert_eq!(object[object.len() - 1].key, a);
    assert_eq!(object[object.len() - 1].method_kind, None);

    // `1` and `0x1` are one numeric key; `"1"` is a distinct string key.
    let one = TypeAuthoredPropertyKey::Number(
        verter_type_expr::CanonicalIndexInt::from_canonical_i64(1).expect("canonical"),
    );
    let string_one = TypeAuthoredPropertyKey::string("1");
    for list in [merged, object] {
        assert_eq!(list.iter().filter(|m| m.key == one).count(), 1);
        assert_eq!(list.iter().filter(|m| m.key == string_one).count(), 1);
    }
    // The merged interface keeps the first `1` at its authored position, so
    // the later `0x1: boolean` contributor neither replaces nor moves it.
    assert!(std::ptr::eq(
        merged.get(&one).expect("numeric 1"),
        &merged[1]
    ));
    assert!(std::ptr::eq(
        merged.get(&string_one).expect("string 1"),
        &merged[2]
    ));

    // Every key resolves through the index to its ordered member.
    for list in [merged, literal, object] {
        for member in list.iter() {
            assert!(std::ptr::eq(
                list.get(&member.key).expect("indexed"),
                member
            ));
        }
    }
}
