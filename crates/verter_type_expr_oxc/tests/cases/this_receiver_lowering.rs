//! An authored `this` receiver is preserved by every function-like lowering
//! that can carry one.
//!
//! OXC exposes `this` OUTSIDE `FormalParameters` (`Function::this_param`,
//! `TSFunctionType::this_param`, `TSMethodSignature::this_param`,
//! `TSCallSignatureDeclaration::this_param`), so a lowering that walks only
//! `params.items` silently DISCARDS the receiver and every consumer that reads
//! it — applicability's receiver check, `ThisParameterType`,
//! `OmitThisParameter` — has nothing to read. These tests FAIL against such a
//! tree.
//!
//! The representation is the LEADING parameter named `this`. The negative
//! cases pin that an ordinary function grows no such parameter and that a
//! constructor type (which cannot author a receiver) is untouched.

use oxc_ast::ast::{Statement, TSSignature, TSType};
use oxc_parser::Parser;
use oxc_span::SourceType;

use verter_type_expr::{ObjectMember, PrimitiveName, TypeExpr};
use verter_type_expr_oxc::lower_ts_type;

/// Parse `source` (which MUST declare a `type __T = <annotation>;` alias) and
/// lower the alias's annotation to a [`TypeExpr`].
fn lower_alias(source: &str) -> TypeExpr {
    let allocator = oxc_allocator::Allocator::default();
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

fn function_of(expr: &TypeExpr) -> &verter_type_expr::FunctionExpr {
    match expr {
        TypeExpr::Function(function) | TypeExpr::ConstructorType(function) => function,
        other => panic!("expected a function type, got {other:?}"),
    }
}

#[test]
fn function_type_preserves_the_authored_this_receiver() {
    let lowered = lower_alias("type __T = (this: { v: number }, x: string) => number;");
    let function = function_of(&lowered);
    assert_eq!(
        function.parameters.len(),
        2,
        "the receiver plus the ordinary parameter: {:?}",
        function.parameters
    );
    assert_eq!(function.parameters[0].name.as_deref(), Some("this"));
    assert!(
        function.parameters[0].has_ts_annotation,
        "an authored `this: T` carries its annotation fact"
    );
    let TypeExpr::Object(receiver) = &function.parameters[0].ty else {
        panic!("the receiver type is the authored object literal type");
    };
    assert_eq!(receiver.properties.len(), 1);
    assert_eq!(function.parameters[1].name.as_deref(), Some("x"));
    assert_eq!(
        function.parameters[1].ty,
        TypeExpr::Primitive(PrimitiveName::String)
    );
}

#[test]
fn function_type_without_a_receiver_grows_no_this_parameter() {
    let lowered = lower_alias("type __T = (x: string) => number;");
    let function = function_of(&lowered);
    assert_eq!(function.parameters.len(), 1);
    assert_eq!(function.parameters[0].name.as_deref(), Some("x"));
}

#[test]
fn constructor_type_carries_only_its_ordinary_parameters() {
    let lowered = lower_alias("type __T = new (id: string) => { id: string };");
    let function = function_of(&lowered);
    assert_eq!(function.parameters.len(), 1);
    assert_eq!(function.parameters[0].name.as_deref(), Some("id"));
}

#[test]
fn member_call_and_method_signatures_preserve_the_authored_this_receiver() {
    let lowered = lower_alias(
        "type __T = {\n\
         \x20 (this: void, item: number): string;\n\
         \x20 run(this: { v: number }, factor: number): number;\n\
         };",
    );
    let TypeExpr::Object(object) = &lowered else {
        panic!("expected an object type, got {lowered:?}");
    };
    let ObjectMember::CallSignature(call) = &object.properties[0] else {
        panic!("expected a call signature, got {:?}", object.properties[0]);
    };
    assert_eq!(call.parameters.len(), 2);
    assert_eq!(call.parameters[0].name.as_deref(), Some("this"));
    assert_eq!(
        call.parameters[0].ty,
        TypeExpr::Primitive(PrimitiveName::Void)
    );
    assert_eq!(call.parameters[1].name.as_deref(), Some("item"));

    let ObjectMember::Method(method) = &object.properties[1] else {
        panic!(
            "expected a method signature, got {:?}",
            object.properties[1]
        );
    };
    assert_eq!(method.function.parameters.len(), 2);
    assert_eq!(
        method.function.parameters[0].name.as_deref(),
        Some("this"),
        "a method signature's authored receiver leads its parameter list"
    );
    assert_eq!(
        method.function.parameters[1].name.as_deref(),
        Some("factor")
    );
}

/// A `TSSignature` reached through the type-literal lowering must not be the
/// only path that keeps the receiver: the shared parameter lowering is what
/// carries it, so a nested function-typed member position keeps it too.
#[test]
fn nested_function_typed_member_preserves_the_authored_this_receiver() {
    let source = "type __T = { row: (this: void, item: number) => string };";
    let allocator = oxc_allocator::Allocator::default();
    let ret = Parser::new(&allocator, source, SourceType::ts()).parse();
    assert!(!ret.panicked);
    let has_this_param = ret.program.body.iter().any(|stmt| match stmt {
        Statement::TSTypeAliasDeclaration(alias) => match &alias.type_annotation {
            TSType::TSTypeLiteral(literal) => literal.members.iter().any(|member| match member {
                TSSignature::TSPropertySignature(prop) => prop
                    .type_annotation
                    .as_ref()
                    .is_some_and(|annotation| {
                        matches!(&annotation.type_annotation, TSType::TSFunctionType(function) if function.this_param.is_some())
                    }),
                _ => false,
            }),
            _ => false,
        },
        _ => false,
    });
    assert!(
        has_this_param,
        "the fixture must actually author a nested `this` receiver"
    );

    let lowered = lower_alias(source);
    let TypeExpr::Object(object) = &lowered else {
        panic!("expected an object type, got {lowered:?}");
    };
    let ObjectMember::Property(property) = &object.properties[0] else {
        panic!("expected a property, got {:?}", object.properties[0]);
    };
    let function = function_of(&property.ty);
    assert_eq!(function.parameters.len(), 2);
    assert_eq!(function.parameters[0].name.as_deref(), Some("this"));
}
