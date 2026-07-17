//! Stack-safe TypeScript display projection for [`TypeExpr`](crate::TypeExpr).
//!
//! This module is deliberately a renderer, not an evaluator. It walks the
//! already-produced typed IR without resolving names or rewriting meaning. The
//! walk uses an explicit heap worklist, so a deep but finite type has no
//! structural-depth cap and never consumes one Rust call frame per type node.

use std::fmt;
use std::fmt::Write as _;
use std::sync::Arc;

use rustc_hash::FxHashSet;

use crate::{
    FunctionExpr, FunctionParam, LiteralValue, MappedModifier, ObjectMember, PrimitiveName,
    TupleElement, TypeExpr, TypeParam,
};

/// A complete TypeScript display projection and the named type references
/// encountered while producing it.
///
/// `referenced_type_names` is de-duplicated in first-use display order. It
/// contains named [`TypeExpr::Ref`] and [`TypeExpr::RecursiveRef`] heads, but
/// deliberately excludes type-parameter declarations/references, value names
/// in `typeof`, and self-contained `import("...")` carriers.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RenderedTypeExpr {
    pub text: String,
    pub referenced_type_names: Vec<Arc<str>>,
}

/// Why a typed expression cannot honestly be represented as TypeScript type
/// syntax.
///
/// Failures stay typed. In particular, internal carriers and malformed source
/// fallbacks are never converted to the keyword `unknown`, because doing so
/// would make an incomplete projection look semantically complete.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TypeExprDisplayError {
    /// A carrier needs an owning semantic projector before it can be displayed.
    InternalCarrier { kind: &'static str },
    /// `Unknown` carried no authored raw syntax to preserve.
    EmptyUnknownSource,
    /// A template-literal type must have exactly one more quasi than expression.
    InvalidTemplateLiteralArity { quasis: usize, expressions: usize },
    /// A raw template quasi would terminate or open an interpolation in the
    /// generated template literal instead of remaining literal text.
    InvalidTemplateLiteralQuasi { index: usize },
    /// A numeric literal cannot be represented by TypeScript numeric syntax.
    NonFiniteNumberLiteral,
    /// The stored bigint magnitude was not a signed base-10 integer.
    InvalidBigIntLiteral,
    /// A reference-like type carried no name.
    EmptyTypeName { kind: &'static str },
    /// A `typeof` value reference carried no path.
    EmptyTypeOfPath,
    /// Type arguments may only apply to a qualified import-type member.
    ImportTypeArgumentsWithoutQualifier,
    /// A dotted reference path contained an empty segment.
    EmptyPathSegment { kind: &'static str, index: usize },
    /// A function or signature has no return type to render.
    MissingFunctionReturnType,
    /// A rest parameter/tuple element cannot also be optional.
    OptionalRest { kind: &'static str },
    /// Both a tuple element and its contained type carried the rest marker.
    DuplicateTupleRestMarker,
    /// A standalone rest marker has no valid TypeScript type-expression syntax.
    StandaloneRestType,
}

impl fmt::Display for TypeExprDisplayError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InternalCarrier { kind } => {
                write!(
                    f,
                    "internal type carrier `{kind}` requires semantic projection"
                )
            }
            Self::EmptyUnknownSource => {
                f.write_str("unrepresentable type carried no authored source syntax")
            }
            Self::InvalidTemplateLiteralArity {
                quasis,
                expressions,
            } => write!(
                f,
                "template-literal type has {quasis} quasis for {expressions} expressions"
            ),
            Self::InvalidTemplateLiteralQuasi { index } => write!(
                f,
                "template-literal quasi at index {index} contains an unescaped delimiter"
            ),
            Self::NonFiniteNumberLiteral => {
                f.write_str("non-finite number is not a TypeScript literal type")
            }
            Self::InvalidBigIntLiteral => {
                f.write_str("bigint literal is not a signed base-10 integer")
            }
            Self::EmptyTypeName { kind } => write!(f, "{kind} carried an empty type name"),
            Self::EmptyTypeOfPath => f.write_str("typeof reference carried an empty value path"),
            Self::ImportTypeArgumentsWithoutQualifier => {
                f.write_str("bare import type cannot carry type arguments")
            }
            Self::EmptyPathSegment { kind, index } => {
                write!(f, "{kind} carried an empty path segment at index {index}")
            }
            Self::MissingFunctionReturnType => f.write_str("function type carried no return type"),
            Self::OptionalRest { kind } => {
                write!(f, "{kind} cannot be both rest and optional")
            }
            Self::DuplicateTupleRestMarker => {
                f.write_str("tuple element carried duplicate rest markers")
            }
            Self::StandaloneRestType => {
                f.write_str("standalone rest marker is not a TypeScript type expression")
            }
        }
    }
}

impl std::error::Error for TypeExprDisplayError {}

#[derive(Debug, Clone, Copy)]
enum FunctionStyle {
    Arrow,
    ConstructorArrow,
    Signature,
    ConstructSignature,
}

/// Type-operator binding strength, from loosest to tightest.
///
/// The renderer carries the minimum strength required by each parent frame.
/// This keeps the worklist stack-safe while emitting parentheses only where
/// omitting them would change the TypeScript parse tree.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
enum Precedence {
    Lowest,
    ConditionalOrFunction,
    Union,
    Intersection,
    Prefix,
    Postfix,
    Primary,
}

#[derive(Debug)]
enum Frame<'a> {
    Expr(&'a TypeExpr, Precedence),
    ExprBody(&'a TypeExpr),
    ObjectMember(&'a ObjectMember),
    Function(&'a FunctionExpr, FunctionStyle),
    FunctionParam(&'a FunctionParam, usize),
    TupleElement(&'a TupleElement),
    TypeParameterDeclaration(&'a TypeParam),
    GeneratedParameterName(usize),
    Text(&'static str),
    Borrowed(&'a str),
    MemberName(&'a str),
    SingleQuoted(&'a str),
    Number(f64),
    BigInt(&'a str),
}

/// Render a complete type expression as valid, precedence-preserving
/// TypeScript type syntax.
///
/// The implementation has no structural-depth limit and performs no recursive
/// calls. Compound operators are parenthesized or emitted through unambiguous
/// generic spellings (`Array<T>` / `ReadonlyArray<T>`), so callers may embed the
/// returned text in any type position without scanning it to recover precedence.
pub fn render_type_expr_display(
    expression: &TypeExpr,
) -> Result<RenderedTypeExpr, TypeExprDisplayError> {
    let mut text = String::new();
    let mut referenced_type_names = Vec::new();
    let mut seen_references = FxHashSet::default();
    let mut work = vec![Frame::Expr(expression, Precedence::Lowest)];

    while let Some(frame) = work.pop() {
        match frame {
            Frame::Text(value) => text.push_str(value),
            Frame::Borrowed(value) => text.push_str(value),
            Frame::GeneratedParameterName(index) => {
                write!(&mut text, "_arg{index}").expect("writing to String cannot fail");
            }
            Frame::MemberName(name) => push_member_name(&mut text, name),
            Frame::SingleQuoted(value) => push_single_quoted(&mut text, value),
            Frame::Number(value) => {
                if !value.is_finite() {
                    return Err(TypeExprDisplayError::NonFiniteNumberLiteral);
                }
                write!(&mut text, "{value}").expect("writing to String cannot fail");
            }
            Frame::BigInt(value) => {
                if !is_bigint_magnitude(value) {
                    return Err(TypeExprDisplayError::InvalidBigIntLiteral);
                }
                text.push_str(value);
                text.push('n');
            }
            Frame::Expr(expr, minimum) => {
                if expression_precedence(expr) < minimum {
                    work.push(Frame::Text(")"));
                    work.push(Frame::ExprBody(expr));
                    work.push(Frame::Text("("));
                } else {
                    work.push(Frame::ExprBody(expr));
                }
            }
            Frame::ExprBody(expr) => match expr {
                TypeExpr::Primitive(name) => text.push_str(primitive_keyword(*name)),
                TypeExpr::Literal(LiteralValue::String(value)) => {
                    push_single_quoted(&mut text, value)
                }
                TypeExpr::Literal(LiteralValue::Number(value)) => {
                    work.push(Frame::Number(*value));
                }
                TypeExpr::Literal(LiteralValue::Boolean(value)) => {
                    text.push_str(if *value { "true" } else { "false" });
                }
                TypeExpr::Literal(LiteralValue::BigInt(value)) => {
                    work.push(Frame::BigInt(value));
                }
                TypeExpr::Union(types) => {
                    if types.is_empty() {
                        text.push_str("never");
                    } else {
                        push_expr_list(&mut work, types, " | ", Precedence::Union);
                    }
                }
                TypeExpr::Intersection(types) => {
                    if types.is_empty() {
                        text.push_str("unknown");
                    } else {
                        push_expr_list(&mut work, types, " & ", Precedence::Intersection);
                    }
                }
                TypeExpr::Array { element, readonly } => {
                    work.push(Frame::Text(">"));
                    work.push(Frame::Expr(element, Precedence::Lowest));
                    work.push(Frame::Text(if *readonly {
                        "ReadonlyArray<"
                    } else {
                        "Array<"
                    }));
                }
                TypeExpr::Tuple { elements, readonly } => {
                    work.push(Frame::Text("]"));
                    push_tuple_elements(&mut work, elements);
                    work.push(Frame::Text(if *readonly { "readonly [" } else { "[" }));
                }
                TypeExpr::Object(object) => {
                    if object.properties.is_empty() {
                        text.push_str("{}");
                    } else {
                        work.push(Frame::Text(" }"));
                        push_object_members(&mut work, &object.properties);
                        work.push(Frame::Text("{ "));
                    }
                }
                TypeExpr::Function(function) => {
                    work.push(Frame::Function(function, FunctionStyle::Arrow));
                }
                TypeExpr::ConstructorType(function) => {
                    work.push(Frame::Function(function, FunctionStyle::ConstructorArrow));
                }
                TypeExpr::Ref {
                    name,
                    type_arguments,
                } => {
                    ensure_type_name(name, "reference")?;
                    record_reference(name, &mut seen_references, &mut referenced_type_names);
                    push_named_type(&mut work, name, type_arguments);
                }
                TypeExpr::TypeParameter(parameter) => {
                    ensure_type_name(&parameter.name, "type parameter")?;
                    text.push_str(&parameter.name);
                }
                TypeExpr::KeyOf(inner) => {
                    work.push(Frame::Expr(inner, Precedence::Prefix));
                    work.push(Frame::Text("keyof "));
                }
                TypeExpr::TypeOf(value) => {
                    if value.path.is_empty() {
                        return Err(TypeExprDisplayError::EmptyTypeOfPath);
                    }
                    ensure_path(&value.path, "typeof reference")?;
                    if !value.type_args.is_empty() {
                        work.push(Frame::Text(">"));
                        push_expr_list(&mut work, &value.type_args, ", ", Precedence::Lowest);
                        work.push(Frame::Text("<"));
                    }
                    push_string_path(&mut work, &value.path);
                    work.push(Frame::Text("typeof "));
                }
                TypeExpr::IndexedAccess { object, index } => {
                    work.push(Frame::Text("]"));
                    work.push(Frame::Expr(index, Precedence::Lowest));
                    work.push(Frame::Text("["));
                    work.push(Frame::Expr(object, Precedence::Postfix));
                }
                TypeExpr::Conditional {
                    check,
                    extends,
                    true_type,
                    false_type,
                } => {
                    work.push(Frame::Expr(false_type, Precedence::Lowest));
                    work.push(Frame::Text(" : "));
                    work.push(Frame::Expr(true_type, Precedence::Lowest));
                    work.push(Frame::Text(" ? "));
                    work.push(Frame::Expr(extends, Precedence::Union));
                    work.push(Frame::Text(" extends "));
                    work.push(Frame::Expr(check, Precedence::Union));
                }
                TypeExpr::Mapped {
                    parameter,
                    source,
                    value,
                    optional,
                    readonly,
                    name_type,
                } => {
                    ensure_type_name(parameter, "mapped type parameter")?;
                    work.push(Frame::Text(" }"));
                    work.push(Frame::Expr(value, Precedence::Lowest));
                    work.push(Frame::Text(": "));
                    work.push(Frame::Text(mapped_optional_token(*optional)));
                    work.push(Frame::Text("]"));
                    if let Some(name_type) = name_type {
                        work.push(Frame::Expr(name_type, Precedence::Lowest));
                        work.push(Frame::Text(" as "));
                    }
                    work.push(Frame::Expr(source, Precedence::Lowest));
                    work.push(Frame::Text(" in "));
                    work.push(Frame::Borrowed(parameter));
                    work.push(Frame::Text("["));
                    work.push(Frame::Text(mapped_readonly_prefix(*readonly)));
                    work.push(Frame::Text("{ "));
                }
                TypeExpr::TemplateLiteral {
                    quasis,
                    expressions,
                } => {
                    if quasis.len() != expressions.len() + 1 {
                        return Err(TypeExprDisplayError::InvalidTemplateLiteralArity {
                            quasis: quasis.len(),
                            expressions: expressions.len(),
                        });
                    }
                    for (index, quasi) in quasis.iter().enumerate() {
                        if !template_quasi_is_safe(quasi) {
                            return Err(TypeExprDisplayError::InvalidTemplateLiteralQuasi {
                                index,
                            });
                        }
                    }
                    work.push(Frame::Text("`"));
                    for index in (0..expressions.len()).rev() {
                        work.push(Frame::Borrowed(&quasis[index + 1]));
                        work.push(Frame::Text("}"));
                        work.push(Frame::Expr(&expressions[index], Precedence::Lowest));
                        work.push(Frame::Text("${"));
                    }
                    work.push(Frame::Borrowed(&quasis[0]));
                    work.push(Frame::Text("`"));
                }
                TypeExpr::Infer { name } => {
                    ensure_type_name(name, "infer binding")?;
                    work.push(Frame::Borrowed(name));
                    work.push(Frame::Text("infer "));
                }
                TypeExpr::Rest(_) => return Err(TypeExprDisplayError::StandaloneRestType),
                TypeExpr::Parenthesized(inner) => {
                    work.push(Frame::Text(")"));
                    work.push(Frame::Expr(inner, Precedence::Lowest));
                    work.push(Frame::Text("("));
                }
                TypeExpr::RecursiveRef {
                    name,
                    type_arguments,
                    conditional_context: _,
                } => {
                    ensure_type_name(name, "recursive reference")?;
                    record_reference(name, &mut seen_references, &mut referenced_type_names);
                    push_named_type(&mut work, name, type_arguments);
                }
                TypeExpr::SyntheticSlotBinding(_) => {
                    return Err(TypeExprDisplayError::InternalCarrier {
                        kind: "SyntheticSlotBinding",
                    });
                }
                TypeExpr::ImportType {
                    specifier,
                    qualifier,
                    typeof_query,
                    type_arguments,
                } => {
                    if qualifier.is_empty() && !type_arguments.is_empty() {
                        return Err(TypeExprDisplayError::ImportTypeArgumentsWithoutQualifier);
                    }
                    for (index, segment) in qualifier.iter().enumerate() {
                        if segment.is_empty() {
                            return Err(TypeExprDisplayError::EmptyPathSegment {
                                kind: "import type qualifier",
                                index,
                            });
                        }
                    }
                    if !type_arguments.is_empty() {
                        work.push(Frame::Text(">"));
                        push_expr_list(&mut work, type_arguments, ", ", Precedence::Lowest);
                        work.push(Frame::Text("<"));
                    }
                    push_arc_path(&mut work, qualifier);
                    work.push(Frame::Text(")"));
                    work.push(Frame::SingleQuoted(specifier));
                    work.push(Frame::Text("import("));
                    if *typeof_query {
                        work.push(Frame::Text("typeof "));
                    }
                }
                TypeExpr::Unknown { raw } => {
                    if raw.trim().is_empty() {
                        return Err(TypeExprDisplayError::EmptyUnknownSource);
                    }
                    work.push(Frame::Borrowed(raw));
                }
            },
            Frame::ObjectMember(member) => match member {
                ObjectMember::Property(property) => {
                    work.push(Frame::Expr(&property.ty, Precedence::Lowest));
                    work.push(Frame::Text(": "));
                    if property.optional {
                        work.push(Frame::Text("?"));
                    }
                    work.push(Frame::MemberName(&property.name));
                    if property.readonly {
                        work.push(Frame::Text("readonly "));
                    }
                }
                ObjectMember::IndexSignature(index) => {
                    work.push(Frame::Expr(&index.value_type, Precedence::Lowest));
                    work.push(Frame::Text("]: "));
                    work.push(Frame::Expr(&index.key_type, Precedence::Lowest));
                    work.push(Frame::Text(": "));
                    work.push(Frame::Borrowed(if index.key_name.is_empty() {
                        "key"
                    } else {
                        &index.key_name
                    }));
                    work.push(Frame::Text("["));
                    if index.readonly {
                        work.push(Frame::Text("readonly "));
                    }
                }
                ObjectMember::CallSignature(function) => {
                    work.push(Frame::Function(function, FunctionStyle::Signature));
                }
                ObjectMember::ConstructSignature(function) => {
                    work.push(Frame::Function(function, FunctionStyle::ConstructSignature));
                }
                ObjectMember::Method(method) => {
                    work.push(Frame::Function(&method.function, FunctionStyle::Signature));
                    if method.optional {
                        work.push(Frame::Text("?"));
                    }
                    work.push(Frame::MemberName(&method.name));
                }
            },
            Frame::Function(function, style) => {
                let Some(return_type) = function.return_type.as_deref() else {
                    return Err(TypeExprDisplayError::MissingFunctionReturnType);
                };
                work.push(Frame::Expr(return_type, Precedence::Lowest));
                work.push(Frame::Text(
                    if matches!(
                        style,
                        FunctionStyle::Signature | FunctionStyle::ConstructSignature
                    ) {
                        "): "
                    } else {
                        ") => "
                    },
                ));
                push_function_parameters(&mut work, &function.parameters);
                work.push(Frame::Text("("));
                push_type_parameters(&mut work, &function.type_parameters);
                match style {
                    FunctionStyle::Arrow => {}
                    FunctionStyle::ConstructorArrow => work.push(Frame::Text("new ")),
                    FunctionStyle::Signature => {}
                    FunctionStyle::ConstructSignature => work.push(Frame::Text("new ")),
                }
            }
            Frame::FunctionParam(parameter, index) => {
                if parameter.rest && parameter.optional {
                    return Err(TypeExprDisplayError::OptionalRest {
                        kind: "function parameter",
                    });
                }
                work.push(Frame::Expr(&parameter.ty, Precedence::Lowest));
                work.push(Frame::Text(": "));
                if parameter.optional {
                    work.push(Frame::Text("?"));
                }
                if let Some(name) = parameter.name.as_deref().filter(|name| !name.is_empty()) {
                    work.push(Frame::Borrowed(name));
                } else {
                    work.push(Frame::GeneratedParameterName(index));
                }
                if parameter.rest {
                    work.push(Frame::Text("..."));
                }
            }
            Frame::TupleElement(element) => {
                let nested_rest = match &element.ty {
                    TypeExpr::Rest(inner) => Some(inner.as_ref()),
                    _ => None,
                };
                if element.rest && nested_rest.is_some() {
                    return Err(TypeExprDisplayError::DuplicateTupleRestMarker);
                }
                let is_rest = element.rest || nested_rest.is_some();
                if is_rest && element.optional {
                    return Err(TypeExprDisplayError::OptionalRest {
                        kind: "tuple element",
                    });
                }
                if !is_rest && element.label.is_none() && element.optional {
                    work.push(Frame::Text("?"));
                }
                work.push(Frame::Expr(
                    nested_rest.unwrap_or(&element.ty),
                    Precedence::Lowest,
                ));
                if let Some(label) = &element.label {
                    work.push(Frame::Text(": "));
                    if element.optional {
                        work.push(Frame::Text("?"));
                    }
                    work.push(Frame::Borrowed(label));
                }
                if is_rest {
                    work.push(Frame::Text("..."));
                }
            }
            Frame::TypeParameterDeclaration(parameter) => {
                ensure_type_name(&parameter.name, "type parameter declaration")?;
                if let Some(default) = parameter.default.as_deref() {
                    work.push(Frame::Expr(default, Precedence::Lowest));
                    work.push(Frame::Text(" = "));
                }
                if let Some(constraint) = parameter.constraint.as_deref() {
                    work.push(Frame::Expr(constraint, Precedence::Lowest));
                    work.push(Frame::Text(" extends "));
                }
                work.push(Frame::Borrowed(&parameter.name));
            }
        }
    }

    Ok(RenderedTypeExpr {
        text,
        referenced_type_names,
    })
}

fn primitive_keyword(name: PrimitiveName) -> &'static str {
    name.as_str()
}

fn expression_precedence(expression: &TypeExpr) -> Precedence {
    match expression {
        TypeExpr::Conditional { .. } | TypeExpr::Function(_) | TypeExpr::ConstructorType(_) => {
            Precedence::ConditionalOrFunction
        }
        TypeExpr::Union(types) if !types.is_empty() => Precedence::Union,
        TypeExpr::Intersection(types) if !types.is_empty() => Precedence::Intersection,
        TypeExpr::KeyOf(_) | TypeExpr::TypeOf(_) | TypeExpr::Infer { .. } => Precedence::Prefix,
        TypeExpr::IndexedAccess { .. } => Precedence::Postfix,
        // The raw carrier can contain any authored type expression. Treat it
        // as the loosest form so embedding contexts preserve its parse tree.
        TypeExpr::Unknown { .. } => Precedence::Lowest,
        _ => Precedence::Primary,
    }
}

fn ensure_type_name(name: &str, kind: &'static str) -> Result<(), TypeExprDisplayError> {
    if name.is_empty() {
        Err(TypeExprDisplayError::EmptyTypeName { kind })
    } else {
        Ok(())
    }
}

fn ensure_path(path: &[String], kind: &'static str) -> Result<(), TypeExprDisplayError> {
    for (index, segment) in path.iter().enumerate() {
        if segment.is_empty() {
            return Err(TypeExprDisplayError::EmptyPathSegment { kind, index });
        }
    }
    Ok(())
}

fn record_reference(name: &Arc<str>, seen: &mut FxHashSet<Arc<str>>, ordered: &mut Vec<Arc<str>>) {
    if seen.insert(Arc::clone(name)) {
        ordered.push(Arc::clone(name));
    }
}

fn push_named_type<'a>(work: &mut Vec<Frame<'a>>, name: &'a str, args: &'a [TypeExpr]) {
    if !args.is_empty() {
        work.push(Frame::Text(">"));
        push_expr_list(work, args, ", ", Precedence::Lowest);
        work.push(Frame::Text("<"));
    }
    work.push(Frame::Borrowed(name));
}

fn push_expr_list<'a>(
    work: &mut Vec<Frame<'a>>,
    expressions: &'a [TypeExpr],
    separator: &'static str,
    minimum: Precedence,
) {
    for index in (0..expressions.len()).rev() {
        work.push(Frame::Expr(&expressions[index], minimum));
        if index != 0 {
            work.push(Frame::Text(separator));
        }
    }
}

fn push_tuple_elements<'a>(work: &mut Vec<Frame<'a>>, elements: &'a [TupleElement]) {
    for index in (0..elements.len()).rev() {
        work.push(Frame::TupleElement(&elements[index]));
        if index != 0 {
            work.push(Frame::Text(", "));
        }
    }
}

fn push_object_members<'a>(work: &mut Vec<Frame<'a>>, members: &'a [ObjectMember]) {
    for index in (0..members.len()).rev() {
        work.push(Frame::ObjectMember(&members[index]));
        if index != 0 {
            work.push(Frame::Text("; "));
        }
    }
}

fn push_function_parameters<'a>(work: &mut Vec<Frame<'a>>, parameters: &'a [FunctionParam]) {
    for index in (0..parameters.len()).rev() {
        work.push(Frame::FunctionParam(&parameters[index], index));
        if index != 0 {
            work.push(Frame::Text(", "));
        }
    }
}

fn push_type_parameters<'a>(work: &mut Vec<Frame<'a>>, parameters: &'a [TypeParam]) {
    if parameters.is_empty() {
        return;
    }
    work.push(Frame::Text(">"));
    for index in (0..parameters.len()).rev() {
        work.push(Frame::TypeParameterDeclaration(&parameters[index]));
        if index != 0 {
            work.push(Frame::Text(", "));
        }
    }
    work.push(Frame::Text("<"));
}

fn push_string_path<'a>(work: &mut Vec<Frame<'a>>, path: &'a [String]) {
    for index in (0..path.len()).rev() {
        work.push(Frame::Borrowed(&path[index]));
        if index != 0 {
            work.push(Frame::Text("."));
        }
    }
}

fn push_arc_path<'a>(work: &mut Vec<Frame<'a>>, path: &'a [Arc<str>]) {
    for index in (0..path.len()).rev() {
        work.push(Frame::Borrowed(&path[index]));
        work.push(Frame::Text("."));
    }
}

fn mapped_optional_token(modifier: MappedModifier) -> &'static str {
    match modifier {
        MappedModifier::None => "",
        MappedModifier::Add => "?",
        MappedModifier::Remove => "-?",
    }
}

fn mapped_readonly_prefix(modifier: MappedModifier) -> &'static str {
    match modifier {
        MappedModifier::None => "",
        MappedModifier::Add => "readonly ",
        MappedModifier::Remove => "-readonly ",
    }
}

fn push_member_name(output: &mut String, name: &str) {
    if is_bare_member_name(name) {
        output.push_str(name);
    } else {
        push_single_quoted(output, name);
    }
}

fn is_bare_member_name(name: &str) -> bool {
    if !name.is_empty() && name.bytes().all(|byte| byte.is_ascii_digit()) {
        return true;
    }
    let mut chars = name.chars();
    let Some(first) = chars.next() else {
        return false;
    };
    if !(first.is_alphabetic() || first == '_' || first == '$') {
        return false;
    }
    chars.all(|character| character.is_alphanumeric() || character == '_' || character == '$')
}

fn push_single_quoted(output: &mut String, value: &str) {
    output.push('\'');
    for character in value.chars() {
        match character {
            '\\' => output.push_str("\\\\"),
            '\'' => output.push_str("\\'"),
            '\n' => output.push_str("\\n"),
            '\r' => output.push_str("\\r"),
            '\t' => output.push_str("\\t"),
            '\u{2028}' => output.push_str("\\u{2028}"),
            '\u{2029}' => output.push_str("\\u{2029}"),
            control if control.is_control() => {
                write!(output, "\\u{{{:x}}}", control as u32)
                    .expect("writing to String cannot fail");
            }
            other => output.push(other),
        }
    }
    output.push('\'');
}

fn is_bigint_magnitude(value: &str) -> bool {
    let digits = value.strip_prefix('-').unwrap_or(value);
    !digits.is_empty() && digits.bytes().all(|byte| byte.is_ascii_digit())
}

/// Whether a parser-preserved raw quasi can be copied between template
/// delimiters without changing token boundaries. An odd run of backslashes
/// escapes the following byte; an even run leaves it active.
fn template_quasi_is_safe(value: &str) -> bool {
    let bytes = value.as_bytes();
    let mut index = 0;
    let mut escaped = false;
    while index < bytes.len() {
        match bytes[index] {
            b'\\' => escaped = !escaped,
            b'`' if !escaped => return false,
            b'$' if !escaped && bytes.get(index + 1) == Some(&b'{') => return false,
            _ => escaped = false,
        }
        index += 1;
    }
    !escaped
}
