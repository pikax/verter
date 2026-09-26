//! A type nested to any depth lowers without a native level per nesting
//! level: [`super::lower_ts_type`] lowers from an explicit stack.

use oxc_allocator::Allocator;
use oxc_ast::ast::Statement;
use oxc_parser::Parser;
use oxc_span::SourceType;
use verter_type_expr::TypeExpr;

use super::lower_ts_type;

/// The stack the parser gets for these fixtures: oxc's parser recurses once
/// per nesting level.
const PARSER_STACK: usize = 256 * 1024 * 1024;

/// The stack the lowering gets: a small fraction of one level per nesting
/// level of the deepest fixture.
const LOWERING_STACK: usize = 256 * 1024;

/// How many times `expr` nests along the one child each fixture nests
/// through, walked iteratively.
fn nesting_depth(expr: &TypeExpr) -> usize {
    let mut depth = 0;
    let mut current = expr;
    loop {
        let next = match current {
            TypeExpr::Ref { type_arguments, .. } if type_arguments.len() == 1 => &type_arguments[0],
            TypeExpr::Parenthesized(inner) | TypeExpr::KeyOf(inner) => inner.as_ref(),
            TypeExpr::Array { element, .. } => element.as_ref(),
            TypeExpr::Tuple { elements, .. } if elements.len() == 1 => &elements[0].ty,
            TypeExpr::Function(function) => match &function.return_type {
                Some(return_type) => return_type.as_ref(),
                None => return depth,
            },
            TypeExpr::Object(object) => match object.properties.first() {
                Some(verter_type_expr::ObjectMember::Property(property)) => &property.ty,
                _ => return depth,
            },
            TypeExpr::Conditional { false_type, .. } => false_type.as_ref(),
            TypeExpr::IndexedAccess { object, .. } => object.as_ref(),
            TypeExpr::Mapped { value, .. } => value.as_ref(),
            _ => return depth,
        };
        depth += 1;
        current = next;
    }
}

/// A parsed annotation lent to the lowering thread.
#[derive(Clone, Copy)]
struct SharedAnnotation<'b, 'a>(&'b oxc_ast::ast::TSType<'a>);

// SAFETY: the lowering thread only reads the syntax tree, and it is a
// scoped thread joined before the parsing thread touches the tree or its
// arena again; nothing reads or writes the tree concurrently with it.
unsafe impl Send for SharedAnnotation<'_, '_> {}

/// `type T = <annotation>;` parsed on a thread with [`PARSER_STACK`], its
/// annotation lowered on a thread with [`LOWERING_STACK`], and the nesting
/// depth of the result.
fn lowered_depth(annotation: String) -> usize {
    std::thread::Builder::new()
        .stack_size(PARSER_STACK)
        .spawn(move || {
            let source = format!("interface Box<T> {{ v: T }}\ntype T = {annotation};\n");
            let allocator = Allocator::default();
            let parsed = Parser::new(&allocator, &source, SourceType::ts()).parse();
            assert!(
                !parsed.panicked && parsed.errors.is_empty(),
                "the fixture parses"
            );
            let Some(Statement::TSTypeAliasDeclaration(alias)) = parsed.program.body.last() else {
                panic!("the fixture ends in a type alias");
            };
            let annotation = SharedAnnotation(&alias.type_annotation);
            let source = source.as_str();
            std::thread::scope(|scope| {
                std::thread::Builder::new()
                    .stack_size(LOWERING_STACK)
                    .spawn_scoped(scope, move || {
                        let annotation = annotation;
                        nesting_depth(&lower_ts_type(annotation.0, source))
                    })
                    .expect("spawn the lowering thread")
                    .join()
                    .expect("the type lowers")
            })
        })
        .expect("spawn the parsing thread")
        .join()
        .expect("the fixture parses and lowers")
}

/// Every form a type nests through lowers 5,000 levels deep on a 256 KiB
/// thread: a named reference's argument, a parenthesised type, an array
/// element, a `keyof` operand, a function type's return, an object type's
/// property, a tuple element, a conditional's false branch, an indexed
/// access's object and a mapped type's value.
#[test]
fn a_type_nested_5000_levels_lowers_on_a_small_stack() {
    let levels = 5000;
    let wrap =
        |open: &str, close: &str| format!("{}1{}", open.repeat(levels), close.repeat(levels));
    let fixtures = [
        ("reference", wrap("Box<", ">")),
        ("parenthesised", wrap("(", ")")),
        ("array", format!("1{}", "[]".repeat(levels))),
        ("keyof", wrap("keyof ", "")),
        ("function", wrap("() => ", "")),
        ("object", wrap("{ v: ", " }")),
        ("tuple", wrap("[", "]")),
        ("conditional", wrap("1 extends 2 ? 3 : ", "")),
        ("indexed access", format!("Box{}", "['v']".repeat(levels))),
        ("mapped", wrap("{ [K in 1]: ", " }")),
    ];
    for (form, annotation) in fixtures {
        assert_eq!(lowered_depth(annotation), levels, "{form}");
    }
}

/// A generic function type rewrites the references to its type parameters
/// from an explicit stack too: over a return nested 5,000 levels deep, and
/// through 1,000 nested generic function types, each binding its own `T`
/// (each one rewrites the functions inside it again, so the work grows with
/// the square of that nesting).
#[test]
fn a_generic_function_type_nested_5000_levels_lowers_on_a_small_stack() {
    let levels = 5000;
    let deep_return = format!("<T>() => {}T{}", "Box<".repeat(levels), ">".repeat(levels));
    assert_eq!(lowered_depth(deep_return), levels + 1);
    let nested = format!("{}T", "<T>() => ".repeat(1000));
    assert_eq!(lowered_depth(nested), 1000);
}
