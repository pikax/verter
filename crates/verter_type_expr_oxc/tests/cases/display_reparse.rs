//! The display renderer's text reparses to the tree it rendered.
//!
//! Each row is a type written as TypeScript intends it. The row's tree is
//! that type lowered with every authored parenthesis removed, so the
//! renderer alone decides where parentheses go. Its rendering must parse
//! without a diagnostic and lower back to the same tree: a missing
//! parenthesis regroups an operator, a dropped modifier or a respelled key
//! changes the tree, and either fails the row. Reference names, `keyof`
//! and `typeof` operands are part of the compared tree, so a print that
//! loses an alias, key or value identity fails as well.

use oxc_allocator::Allocator;
use oxc_ast::ast::Statement;
use oxc_parser::Parser;
use oxc_span::SourceType;
use serde_json::Value;

use verter_type_expr::{render_type_expr_display, type_expr_from_json, TypeExpr};
use verter_type_expr_oxc::lower_ts_type;

/// Parse `type __T = <annotation>;` without a diagnostic and lower the
/// annotation.
fn lower_annotation(annotation: &str) -> TypeExpr {
    let source = format!("type __T = {annotation};");
    let allocator = Allocator::default();
    let ret = Parser::new(&allocator, &source, SourceType::ts()).parse();
    assert!(
        !ret.fatal_error && ret.diagnostics.is_empty(),
        "`{annotation}` does not parse: {:?}",
        ret.diagnostics
    );
    let alias = ret
        .program
        .body
        .iter()
        .find_map(|stmt| match stmt {
            Statement::TSTypeAliasDeclaration(alias) if alias.id.name == "__T" => {
                Some(&alias.type_annotation)
            }
            _ => None,
        })
        .expect("the wrapper declares `__T`");
    lower_ts_type(alias, &source)
}

/// `value` with every parenthesized node replaced by its operand.
fn without_parentheses(value: Value) -> Value {
    match value {
        Value::Object(mut map) => {
            if map.get("kind").and_then(Value::as_str) == Some("parenthesized") {
                return without_parentheses(map.remove("inner").expect("a parenthesized operand"));
            }
            Value::Object(
                map.into_iter()
                    .map(|(key, value)| (key, without_parentheses(value)))
                    .collect(),
            )
        }
        Value::Array(items) => Value::Array(items.into_iter().map(without_parentheses).collect()),
        other => other,
    }
}

/// The parenthesis-free tree of `annotation`.
fn intended_tree(annotation: &str) -> Value {
    without_parentheses(lower_annotation(annotation).to_json_value())
}

/// Render the parenthesis-free tree of `annotation`, check that the text
/// reparses to that tree, and return the text.
fn render_and_reparse(annotation: &str) -> String {
    let intended = intended_tree(annotation);
    let tree = type_expr_from_json(&intended).expect("the intended tree decodes");
    assert_eq!(
        tree.to_json_value(),
        intended,
        "`{annotation}`: the decoded tree is the intended one"
    );
    let rendered = render_type_expr_display(&tree)
        .unwrap_or_else(|error| panic!("`{annotation}` renders: {error}"))
        .text;
    assert_eq!(
        intended_tree(&rendered),
        intended,
        "`{annotation}` rendered as `{rendered}`, which parses to another type"
    );
    rendered
}

#[test]
fn operator_precedence_survives_the_print() {
    for annotation in [
        "(A | B) & C",
        "A | (() => void)",
        "A | (new () => B)",
        "A | ((x: B) => (y: C) => D)",
        "((x: unknown) => asserts x is A) | B",
        "keyof (A | B)",
        "keyof typeof value",
        "(keyof T)[K]",
        "(typeof value)[number]",
        "(A | B)[]",
        "(A & B)[]",
        "readonly (keyof T)[]",
        "(() => void) extends T ? 1 : 2",
        "T extends (U extends V ? 1 : 2) ? 3 : 4",
        "T extends [infer U extends string] ? U : never",
        "T extends (infer U extends string) | number ? U : never",
        "T extends A ? (B extends C ? 1 : 2) : 3",
        "`a${A | B}b`",
        "{ [K in keyof T as `get${K & string}`]?: T[K] | undefined }",
        "Alias<keyof Model, typeof value>",
        "[label?: A | B, ...rest: C[]]",
    ] {
        render_and_reparse(annotation);
    }
}

#[test]
fn an_unlabeled_optional_element_parenthesizes_a_loose_operand() {
    for (annotation, spelled) in [
        ("[(A | B)?]", "[(A | B)?]"),
        ("[(() => void)?]", "[(() => void)?]"),
        ("[(keyof T)?]", "[(keyof T)?]"),
        ("[T[K]?, U?]", "[T[K]?, U?]"),
    ] {
        assert_eq!(render_and_reparse(annotation), spelled);
    }
}

#[test]
fn a_const_type_parameter_keeps_its_modifier() {
    assert_eq!(
        render_and_reparse("<const T extends readonly unknown[], U>(...args: T) => [T, U]"),
        "<const T extends ReadonlyArray<unknown>, U>(...args: T) => [T, U]"
    );
}

#[test]
fn a_member_key_keeps_its_string_or_number_spelling() {
    // `keyof` is `'7'` for the string key and `7` for the number key.
    assert_eq!(
        render_and_reparse("{ '7': A } | { 7: B }"),
        "{ '7': A } | { 7: B }"
    );
    assert_eq!(
        render_and_reparse(
            "{ '0123': A; 7: B; '1e3': C; '12345678901234567890': D; '9007199254740993': E }"
        ),
        "{ '0123': A; 7: B; '1e3': C; '12345678901234567890': D; '9007199254740993': E }"
    );
}

#[test]
fn a_number_literal_prints_its_checker_spelling() {
    assert_eq!(
        render_and_reparse("1e+21 | 1e-7 | -5 | 0.5"),
        "1e+21 | 1e-7 | -5 | 0.5"
    );
}
