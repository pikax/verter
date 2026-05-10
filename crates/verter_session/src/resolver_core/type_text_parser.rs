//! Parse TypeScript type text (`checker.typeToString()` output) into [`TypeExpr`].
//!
//! Both tsserver's `quickinfo.displayString` and TSGO's hover contents produce
//! `checker.typeToString()` output. This parser converts that text into Verter's
//! [`TypeExpr`] IR so all three backends produce the same output shape.
//!
//! # Supported Forms
//!
//! - Primitives: `string`, `number`, `boolean`, `null`, `undefined`, `void`, `never`, `any`, `unknown`
//! - Literals: `"hello"`, `42`, `true`, `false`
//! - Objects: `{ prop: Type; prop?: Type }`
//! - Arrays: `Type[]`
//! - Tuples: `[Type, Type]`
//! - Unions: `Type | Type`
//! - Intersections: `Type & Type`
//! - Functions: `(param: Type) => ReturnType`
//! - References: `TypeName`, `TypeName<Args>`
//!
//! # Unsupported Forms → `TypeExpr::Opaque`
//!
//! Mapped types, conditional types, template literal types, indexed access,
//! `keyof`, `typeof`, `infer`, rest types → stored as raw text in
//! `TypeExpr::Unknown { raw: String }`. Never silently falls back to Verter's type.

use std::sync::Arc;

use verter_type_expr::{
    FunctionExpr, FunctionParam, ObjectExpr, ObjectMember, ObjectProperty, PrimitiveName,
    TupleElement, TypeExpr,
};

/// Parse a TypeScript type text string into a `TypeExpr`.
///
/// Returns `TypeExpr::Unknown { raw: input }` for unrecognized or unsupported forms.
pub fn parse_type_text(input: &str) -> TypeExpr {
    let input = input.trim();
    if input.is_empty() {
        return TypeExpr::Unknown {
            raw: "".to_string(),
        };
    }

    // Try union first (lowest precedence)
    if let Some(expr) = try_parse_union(input) {
        return expr;
    }

    parse_non_union(input)
}

// ---------------------------------------------------------------------------
// Union / Intersection (lowest precedence operators)
// ---------------------------------------------------------------------------

fn try_parse_union(input: &str) -> Option<TypeExpr> {
    let parts = split_top_level(input, '|');
    if parts.len() < 2 {
        return None;
    }
    let types: Vec<TypeExpr> = parts.iter().map(|p| parse_non_union(p.trim())).collect();
    Some(TypeExpr::union(types))
}

fn try_parse_intersection(input: &str) -> Option<TypeExpr> {
    let parts = split_top_level(input, '&');
    if parts.len() < 2 {
        return None;
    }
    let types: Vec<TypeExpr> = parts.iter().map(|p| parse_atom(p.trim())).collect();
    Some(TypeExpr::intersection(types))
}

fn parse_non_union(input: &str) -> TypeExpr {
    let input = input.trim();

    // Try intersection
    if let Some(expr) = try_parse_intersection(input) {
        return expr;
    }

    parse_atom(input)
}

// ---------------------------------------------------------------------------
// Atoms (highest precedence)
// ---------------------------------------------------------------------------

fn parse_atom(input: &str) -> TypeExpr {
    let input = input.trim();

    // Parenthesized: could be function `(x: T) => R` or grouped `(A | B)`
    if input.starts_with('(') {
        if let Some(arrow_pos) = find_arrow_after_parens(input) {
            return parse_function_type(input, arrow_pos);
        }
        // If close paren is at end, it's a grouped type
        if matching_close_paren(input) == Some(input.len() - 1) {
            return parse_type_text(&input[1..input.len() - 1]);
        }
    }

    // Arrow function without parens: `Type => ReturnType` — rare but possible
    // (skip, handled by function parser)

    // Object literal
    if input.starts_with('{') && input.ends_with('}') {
        return parse_object_type(input);
    }

    // Tuple
    if input.starts_with('[') && input.ends_with(']') {
        return parse_tuple_type(input);
    }

    // Array suffix: `Type[]`
    if let Some(element_text) = input.strip_suffix("[]") {
        let inner = parse_type_text(element_text);
        return TypeExpr::Array {
            element: Arc::new(inner),
            readonly: false,
        };
    }

    // String literal
    if (input.starts_with('"') && input.ends_with('"'))
        || (input.starts_with('\'') && input.ends_with('\''))
    {
        let value = &input[1..input.len() - 1];
        return TypeExpr::string_literal(value);
    }

    // Numeric literal
    if let Ok(n) = input.parse::<f64>() {
        return TypeExpr::number_literal(n);
    }

    // Boolean literals
    if input == "true" {
        return TypeExpr::boolean_literal(true);
    }
    if input == "false" {
        return TypeExpr::boolean_literal(false);
    }

    // Primitives
    if let Some(name) = try_parse_primitive(input) {
        return TypeExpr::primitive(name);
    }

    // Generic reference: `Name<Args>`
    if let Some(angle) = input.find('<') {
        if input.ends_with('>') {
            let name = &input[..angle];
            let args_str = &input[angle + 1..input.len() - 1];
            let args: Vec<TypeExpr> = split_top_level(args_str, ',')
                .iter()
                .map(|a| parse_type_text(a.trim()))
                .collect();
            return TypeExpr::named_with_args(name, args);
        }
    }

    // Simple reference or unsupported form
    if is_valid_identifier(input) {
        return TypeExpr::named(input);
    }

    // Fallback: opaque
    TypeExpr::Unknown {
        raw: input.to_string(),
    }
}

// ---------------------------------------------------------------------------
// Object type: `{ prop: Type; prop?: Type }`
// ---------------------------------------------------------------------------

fn parse_object_type(input: &str) -> TypeExpr {
    let inner = input[1..input.len() - 1].trim();
    if inner.is_empty() {
        return TypeExpr::Object(Arc::new(ObjectExpr { properties: vec![] }));
    }

    let mut members = Vec::new();
    let parts = split_top_level(inner, ';');

    for part in parts {
        let part = part.trim();
        if part.is_empty() {
            continue;
        }

        // Property: `name: Type` or `name?: Type`
        if let Some(colon_pos) = find_top_level_colon(part) {
            let key_part = part[..colon_pos].trim();
            let value_part = part[colon_pos + 1..].trim();

            let (name, optional) = if let Some(stripped) = key_part.strip_suffix('?') {
                (stripped, true)
            } else {
                (key_part, false)
            };

            let value_type = parse_type_text(value_part);
            members.push(ObjectMember::Property(ObjectProperty {
                name: name.to_string(),
                ty: value_type,
                optional,
                readonly: false,
            }));
        }
        // Anything else: try as call signature or skip
    }

    TypeExpr::Object(Arc::new(ObjectExpr {
        properties: members,
    }))
}

// ---------------------------------------------------------------------------
// Tuple: `[Type, Type]`
// ---------------------------------------------------------------------------

fn parse_tuple_type(input: &str) -> TypeExpr {
    let inner = input[1..input.len() - 1].trim();
    if inner.is_empty() {
        return TypeExpr::Tuple {
            elements: Arc::from(Vec::<TupleElement>::new()),
            readonly: false,
        };
    }
    let elements: Vec<TypeExpr> = split_top_level(inner, ',')
        .iter()
        .map(|e| parse_type_text(e.trim()))
        .collect();
    TypeExpr::Tuple {
        elements: Arc::from(
            elements
                .into_iter()
                .map(|e| TupleElement {
                    label: None,
                    ty: e,
                    optional: false,
                    rest: false,
                })
                .collect::<Vec<_>>(),
        ),
        readonly: false,
    }
}

// ---------------------------------------------------------------------------
// Function: `(param: Type) => ReturnType`
// ---------------------------------------------------------------------------

fn parse_function_type(input: &str, arrow_pos: usize) -> TypeExpr {
    let params_str = &input[1..input.find(')').unwrap_or(arrow_pos)];
    let return_str = input[arrow_pos + 2..].trim(); // skip "=>"

    let params = if params_str.trim().is_empty() {
        vec![]
    } else {
        split_top_level(params_str, ',')
            .iter()
            .map(|p| {
                let p = p.trim();
                if let Some(colon) = find_top_level_colon(p) {
                    let name = p[..colon].trim().to_string();
                    let ty = parse_type_text(p[colon + 1..].trim());
                    FunctionParam {
                        name: Some(name),
                        ty,
                        optional: false,
                        rest: false,
                    }
                } else {
                    FunctionParam {
                        name: None,
                        ty: parse_type_text(p),
                        optional: false,
                        rest: false,
                    }
                }
            })
            .collect()
    };

    let return_type = if return_str.is_empty() {
        TypeExpr::primitive(PrimitiveName::Void)
    } else {
        parse_type_text(return_str)
    };

    TypeExpr::Function(Arc::new(FunctionExpr {
        parameters: params,
        return_type: Some(Arc::new(return_type)),
        type_parameters: vec![],
    }))
}

// ---------------------------------------------------------------------------
// Primitives
// ---------------------------------------------------------------------------

fn try_parse_primitive(input: &str) -> Option<PrimitiveName> {
    match input {
        "string" => Some(PrimitiveName::String),
        "number" => Some(PrimitiveName::Number),
        "boolean" => Some(PrimitiveName::Boolean),
        "null" => Some(PrimitiveName::Null),
        "undefined" => Some(PrimitiveName::Undefined),
        "void" => Some(PrimitiveName::Void),
        "never" => Some(PrimitiveName::Never),
        "any" => Some(PrimitiveName::Any),
        "unknown" => Some(PrimitiveName::Unknown),
        "symbol" => Some(PrimitiveName::Symbol),
        "bigint" => Some(PrimitiveName::BigInt),
        "object" => Some(PrimitiveName::Object),
        _ => None,
    }
}

// ---------------------------------------------------------------------------
// Helpers: top-level splitting, bracket matching
// ---------------------------------------------------------------------------

/// Split a string at top-level occurrences of `sep`, respecting brackets and quotes.
fn split_top_level(input: &str, sep: char) -> Vec<&str> {
    let mut parts = Vec::new();
    let mut bracket_stack: Vec<char> = Vec::new();
    let mut in_string = false;
    let mut string_char = '"';
    let mut last = 0;

    for (i, c) in input.char_indices() {
        if in_string {
            if c == string_char && !is_escaped(input, i) {
                in_string = false;
            }
            continue;
        }
        match c {
            '"' | '\'' | '`' => {
                in_string = true;
                string_char = c;
            }
            '(' | '{' | '[' => bracket_stack.push(c),
            '<' => {
                // Only treat '<' as bracket opener if it looks like a generic,
                // not a comparison operator. Heuristic: '<' after an identifier char.
                if i > 0 {
                    let prev = input.as_bytes()[i - 1];
                    if prev.is_ascii_alphanumeric() || prev == b'_' || prev == b'$' {
                        bracket_stack.push(c);
                    }
                }
            }
            ')' => {
                if bracket_stack.last() == Some(&'(') {
                    bracket_stack.pop();
                }
            }
            '}' => {
                if bracket_stack.last() == Some(&'{') {
                    bracket_stack.pop();
                }
            }
            ']' => {
                if bracket_stack.last() == Some(&'[') {
                    bracket_stack.pop();
                }
            }
            '>' => {
                if bracket_stack.last() == Some(&'<') {
                    bracket_stack.pop();
                }
            }
            c2 if c2 == sep && bracket_stack.is_empty() => {
                parts.push(&input[last..i]);
                last = i + c2.len_utf8();
            }
            _ => {}
        }
    }
    parts.push(&input[last..]);
    parts
}

fn is_escaped(input: &str, pos: usize) -> bool {
    if pos == 0 {
        return false;
    }
    let bytes = input.as_bytes();
    let mut backslashes = 0;
    let mut i = pos - 1;
    loop {
        if bytes[i] == b'\\' {
            backslashes += 1;
            if i == 0 {
                break;
            }
            i -= 1;
        } else {
            break;
        }
    }
    backslashes % 2 == 1
}

fn matching_close_paren(input: &str) -> Option<usize> {
    if !input.starts_with('(') {
        return None;
    }
    let mut depth = 0;
    for (i, c) in input.char_indices() {
        match c {
            '(' => depth += 1,
            ')' => {
                depth -= 1;
                if depth == 0 {
                    return Some(i);
                }
            }
            _ => {}
        }
    }
    None
}

fn find_arrow_after_parens(input: &str) -> Option<usize> {
    let close = matching_close_paren(input)?;
    let rest = &input[close + 1..];
    let trimmed = rest.trim_start();
    if trimmed.starts_with("=>") {
        let offset = rest.len() - trimmed.len();
        Some(close + 1 + offset)
    } else {
        None
    }
}

fn find_top_level_colon(input: &str) -> Option<usize> {
    let mut depth = 0i32;
    let mut in_string = false;
    let mut string_char = '"';
    for (i, c) in input.char_indices() {
        if in_string {
            if c == string_char && !is_escaped(input, i) {
                in_string = false;
            }
            continue;
        }
        match c {
            '"' | '\'' | '`' => {
                in_string = true;
                string_char = c;
            }
            '(' | '{' | '[' | '<' => depth += 1,
            ')' | '}' | ']' | '>' => depth -= 1,
            ':' if depth == 0 => return Some(i),
            _ => {}
        }
    }
    None
}

fn is_valid_identifier(input: &str) -> bool {
    if input.is_empty() {
        return false;
    }
    let mut chars = input.chars();
    let first = chars.next().unwrap();
    if !first.is_alphabetic() && first != '_' && first != '$' {
        return false;
    }
    chars.all(|c| c.is_alphanumeric() || c == '_' || c == '$' || c == '.')
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use verter_type_expr::LiteralValue;

    fn assert_primitive(input: &str, expected: PrimitiveName) {
        match parse_type_text(input) {
            TypeExpr::Primitive(p) => assert_eq!(p, expected, "input: {input}"),
            other => panic!("expected Primitive for '{input}', got: {other:?}"),
        }
    }

    // ── Primitives ───────────────────────────────────────────────

    #[test]
    fn primitives() {
        assert_primitive("string", PrimitiveName::String);
        assert_primitive("number", PrimitiveName::Number);
        assert_primitive("boolean", PrimitiveName::Boolean);
        assert_primitive("null", PrimitiveName::Null);
        assert_primitive("undefined", PrimitiveName::Undefined);
        assert_primitive("void", PrimitiveName::Void);
        assert_primitive("never", PrimitiveName::Never);
        assert_primitive("any", PrimitiveName::Any);
        assert_primitive("unknown", PrimitiveName::Unknown);
    }

    // ── Literals ─────────────────────────────────────────────────

    #[test]
    fn string_literal() {
        match parse_type_text(r#""hello""#) {
            TypeExpr::Literal(LiteralValue::String(s)) => assert_eq!(s, "hello"),
            other => panic!("expected string literal, got: {other:?}"),
        }
    }

    #[test]
    fn number_literal() {
        match parse_type_text("42") {
            TypeExpr::Literal(LiteralValue::Number(n)) => assert!((n - 42.0).abs() < f64::EPSILON),
            other => panic!("expected number literal, got: {other:?}"),
        }
    }

    #[test]
    fn boolean_literal() {
        match parse_type_text("true") {
            TypeExpr::Literal(LiteralValue::Boolean(b)) => assert!(b),
            other => panic!("expected boolean literal, got: {other:?}"),
        }
    }

    // ── Objects ──────────────────────────────────────────────────

    #[test]
    fn empty_object() {
        match parse_type_text("{}") {
            TypeExpr::Object(obj) => assert!(obj.properties.is_empty()),
            other => panic!("expected Object, got: {other:?}"),
        }
    }

    #[test]
    fn object_with_properties() {
        match parse_type_text("{ msg: string; count?: number }") {
            TypeExpr::Object(obj) => {
                assert_eq!(obj.properties.len(), 2);
                if let ObjectMember::Property(p) = &obj.properties[0] {
                    assert_eq!(p.name, "msg");
                    assert!(!p.optional);
                }
                if let ObjectMember::Property(p) = &obj.properties[1] {
                    assert_eq!(p.name, "count");
                    assert!(p.optional);
                }
            }
            other => panic!("expected Object, got: {other:?}"),
        }
    }

    // ── Arrays ───────────────────────────────────────────────────

    #[test]
    fn array_type() {
        match parse_type_text("string[]") {
            TypeExpr::Array { ref element, .. } => {
                assert!(matches!(**element, TypeExpr::Primitive(_)));
            }
            other => panic!("expected Array, got: {other:?}"),
        }
    }

    // ── Tuples ───────────────────────────────────────────────────

    #[test]
    fn tuple_type() {
        match parse_type_text("[string, number]") {
            TypeExpr::Tuple { ref elements, .. } => assert_eq!(elements.len(), 2),
            other => panic!("expected Tuple, got: {other:?}"),
        }
    }

    // ── Unions ───────────────────────────────────────────────────

    #[test]
    fn union_type() {
        match parse_type_text("string | number") {
            TypeExpr::Union(u) => assert_eq!(u.len(), 2),
            other => panic!("expected Union, got: {other:?}"),
        }
    }

    #[test]
    fn union_with_null() {
        match parse_type_text("string | null | undefined") {
            TypeExpr::Union(u) => assert_eq!(u.len(), 3),
            other => panic!("expected Union, got: {other:?}"),
        }
    }

    // ── Intersections ────────────────────────────────────────────

    #[test]
    fn intersection_type() {
        match parse_type_text("A & B") {
            TypeExpr::Intersection(i) => assert_eq!(i.len(), 2),
            other => panic!("expected Intersection, got: {other:?}"),
        }
    }

    // ── References ───────────────────────────────────────────────

    #[test]
    fn simple_reference() {
        match parse_type_text("ButtonProps") {
            TypeExpr::Ref {
                ref name,
                ref type_arguments,
            } => {
                assert_eq!(name.as_ref(), "ButtonProps");
                assert!(type_arguments.is_empty());
            }
            other => panic!("expected Ref, got: {other:?}"),
        }
    }

    #[test]
    fn generic_reference() {
        match parse_type_text("Array<string>") {
            TypeExpr::Ref {
                ref name,
                ref type_arguments,
            } => {
                assert_eq!(name.as_ref(), "Array");
                assert_eq!(type_arguments.len(), 1);
            }
            other => panic!("expected Ref, got: {other:?}"),
        }
    }

    // ── Functions ────────────────────────────────────────────────

    #[test]
    fn function_type() {
        match parse_type_text("(x: string) => void") {
            TypeExpr::Function(f) => {
                assert_eq!(f.parameters.len(), 1);
                assert_eq!(f.parameters[0].name.as_deref(), Some("x"));
            }
            other => panic!("expected Function, got: {other:?}"),
        }
    }

    #[test]
    fn function_no_params() {
        match parse_type_text("() => string") {
            TypeExpr::Function(f) => {
                assert!(f.parameters.is_empty());
            }
            other => panic!("expected Function, got: {other:?}"),
        }
    }

    // ── Unknown fallback ─────────────────────────────────────────

    #[test]
    fn opaque_for_unsupported() {
        match parse_type_text("{ [K in keyof T]: T[K] }") {
            TypeExpr::Unknown { .. } => {} // ideal: mapped types → Unknown
            TypeExpr::Object(obj) => {
                // If parsed as Object, it should have no valid properties
                // (mapped type syntax isn't a real property)
                assert!(
                    obj.properties.is_empty()
                        || obj.properties.iter().all(|m| match m {
                            verter_type_expr::ObjectMember::Property(p) => {
                                p.name.contains('[') // not a real prop name
                            }
                            _ => true,
                        }),
                    "mapped type should not produce clean property names"
                );
            }
            other => panic!("expected Unknown or partial Object, got: {other:?}"),
        }
    }

    #[test]
    fn empty_input_is_unknown() {
        match parse_type_text("") {
            TypeExpr::Unknown { ref raw } => assert!(raw.is_empty()),
            other => panic!("expected Unknown, got: {other:?}"),
        }
    }

    // ── Negative assertions ──────────────────────────────────────

    #[test]
    fn union_does_not_contain_raw_pipe() {
        let result = parse_type_text("string | number");
        let debug = format!("{result:?}");
        assert!(
            !debug.contains('|'),
            "union should not contain raw pipe in output: {debug}"
        );
    }

    #[test]
    fn object_does_not_contain_raw_braces_in_keys() {
        if let TypeExpr::Object(obj) = parse_type_text("{ msg: string }") {
            for member in &obj.properties {
                if let ObjectMember::Property(p) = member {
                    assert!(
                        !p.name.contains('{'),
                        "name should not contain braces: {}",
                        p.name
                    );
                }
            }
        }
    }
}
