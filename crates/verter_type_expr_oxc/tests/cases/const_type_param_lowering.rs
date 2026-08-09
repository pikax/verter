//! Lowering tests for the `<const T>` type-parameter modifier.
//!
//! The const modifier is PER-PARAMETER identity (`<const T, U>` is valid):
//! the lowering must record each parameter's own `is_const` bit — a
//! session-wide flag, or a modifier dropped at the lowering boundary,
//! cannot produce the mixed `[true, false]` vector these tests pin.
//! Mutation recipe: drop the `p.r#const` read (or the field) — every test
//! here fails.

use oxc_allocator::Allocator;
use oxc_ast::ast::Statement;
use oxc_parser::Parser;
use oxc_span::SourceType;

use verter_type_expr::TypeExpr;
use verter_type_expr_oxc::lower_ts_type;

/// Parse `source` (which MUST declare a `type __T = <annotation>;` alias) and
/// lower the alias's annotation to a [`TypeExpr`].
fn lower_alias(source: &str) -> TypeExpr {
    let allocator = Allocator::default();
    let ret = Parser::new(&allocator, source, SourceType::ts()).parse();
    assert!(!ret.panicked, "OXC parser panicked on `{source}`");

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
        .expect("test wrapper missing `__T` alias");

    lower_ts_type(alias, source)
}

/// The `is_const` vector of a function type's declared type parameters.
fn const_vector(expr: &TypeExpr) -> Vec<bool> {
    let TypeExpr::Function(func) = expr else {
        panic!("expected a lowered function type, got {expr:?}");
    };
    func.type_parameters.iter().map(|p| p.is_const).collect()
}

#[test]
fn const_modifier_lowers_on_the_marked_parameter() {
    let expr = lower_alias("type __T = <const T>(value: T) => T;");
    assert_eq!(
        const_vector(&expr),
        vec![true],
        "`<const T>` must mark exactly that parameter const"
    );
}

#[test]
fn const_modifier_is_per_parameter_not_list_wide() {
    let expr = lower_alias("type __T = <const T, U>(value: T, other: U) => T;");
    assert_eq!(
        const_vector(&expr),
        vec![true, false],
        "`<const T, U>` must parse to per-parameter is_const [true, false] — \
         a session-wide const flag cannot produce the mixed vector"
    );
}

#[test]
fn unmarked_parameter_is_not_const() {
    let expr = lower_alias("type __T = <T>(value: T) => T;");
    assert_eq!(
        const_vector(&expr),
        vec![false],
        "a bare `T` must NOT be marked const"
    );
}
