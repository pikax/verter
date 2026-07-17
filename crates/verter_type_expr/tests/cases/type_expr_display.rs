/**
 * @ai-generated - Behavioral coverage for the stack-safe TypeExpr TypeScript renderer.
 */
use std::sync::Arc;

use verter_type_expr::{
    render_type_expr_display, FunctionExpr, FunctionParam, IndexSignature, LiteralValue,
    MappedModifier, MethodSignature, ObjectExpr, ObjectMember, ObjectProperty, PrimitiveName,
    RecursiveConditionalBranch, RecursiveConditionalFrame, SyntheticCarrierKey,
    SyntheticCarrierSurfaceKind, TupleElement, TypeExpr, TypeExprDisplayError, TypeParam, ValueRef,
};

fn reference(name: &str) -> TypeExpr {
    TypeExpr::Ref {
        name: Arc::from(name),
        type_arguments: Arc::from([]),
    }
}

fn generic_reference(name: &str, arguments: Vec<TypeExpr>) -> TypeExpr {
    TypeExpr::Ref {
        name: Arc::from(name),
        type_arguments: Arc::from(arguments),
    }
}

fn function(
    parameters: Vec<FunctionParam>,
    return_type: TypeExpr,
    type_parameters: Vec<TypeParam>,
) -> FunctionExpr {
    FunctionExpr::synthetic(parameters, Some(Arc::new(return_type)), type_parameters)
}

#[test]
fn renders_every_object_and_function_shape_as_valid_type_syntax() {
    let type_parameter = TypeParam {
        name: "T".into(),
        constraint: Some(Arc::new(reference("Base"))),
        default: Some(Arc::new(TypeExpr::Primitive(PrimitiveName::String))),
    };
    let callable = function(
        vec![FunctionParam::synthetic(
            Some("items".into()),
            TypeExpr::Array {
                element: Arc::new(TypeExpr::TypeParameter(type_parameter.clone())),
                readonly: false,
            },
            false,
            true,
        )],
        generic_reference(
            "Result",
            vec![TypeExpr::TypeParameter(type_parameter.clone())],
        ),
        vec![type_parameter],
    );
    let constructor = function(
        vec![FunctionParam::synthetic(
            Some("value".into()),
            reference("Foo"),
            false,
            false,
        )],
        reference("Instance"),
        Vec::new(),
    );
    let method = function(
        vec![FunctionParam::synthetic(
            Some("value".into()),
            TypeExpr::TypeParameter(TypeParam {
                name: "U".into(),
                constraint: None,
                default: None,
            }),
            true,
            false,
        )],
        generic_reference(
            "Promise",
            vec![TypeExpr::TypeParameter(TypeParam {
                name: "U".into(),
                constraint: None,
                default: None,
            })],
        ),
        vec![TypeParam {
            name: "U".into(),
            constraint: None,
            default: None,
        }],
    );

    let expression = TypeExpr::Object(Arc::new(ObjectExpr {
        properties: vec![
            ObjectMember::Property(ObjectProperty::synthetic_public(
                "dash-key".into(),
                TypeExpr::Array {
                    element: Arc::new(reference("Foo")),
                    readonly: true,
                },
                true,
                true,
            )),
            ObjectMember::IndexSignature(IndexSignature::synthetic(
                "key".into(),
                TypeExpr::Primitive(PrimitiveName::String),
                reference("Bar"),
                true,
            )),
            ObjectMember::CallSignature(callable),
            ObjectMember::ConstructSignature(constructor),
            ObjectMember::Method(MethodSignature::synthetic_public(
                "run".into(),
                method,
                true,
            )),
        ],
    }));

    let rendered = render_type_expr_display(&expression).expect("the complete object must render");
    assert_eq!(
        rendered.text,
        "{ readonly 'dash-key'?: ReadonlyArray<Foo>; readonly [key: string]: Bar; <T extends Base = string>(...items: Array<T>): Result<T>; new (value: Foo): Instance; run?<U>(value?: U): Promise<U> }"
    );
    assert_eq!(
        rendered
            .referenced_type_names
            .iter()
            .map(AsRef::as_ref)
            .collect::<Vec<_>>(),
        vec!["Foo", "Bar", "Base", "Result", "Instance", "Promise"]
    );
}

#[test]
fn renders_operators_carriers_and_literals_without_precedence_loss() {
    let mapped = TypeExpr::Mapped {
        parameter: "K".into(),
        source: Arc::new(TypeExpr::KeyOf(Arc::new(reference("Source")))),
        value: Arc::new(TypeExpr::Conditional {
            check: Arc::new(TypeExpr::IndexedAccess {
                object: Arc::new(reference("Source")),
                index: Arc::new(TypeExpr::TypeParameter(TypeParam {
                    name: "K".into(),
                    constraint: None,
                    default: None,
                })),
            }),
            extends: Arc::new(reference("Expected")),
            true_type: Arc::new(TypeExpr::Infer { name: "V".into() }),
            false_type: Arc::new(TypeExpr::Primitive(PrimitiveName::Never)),
        }),
        optional: MappedModifier::Remove,
        readonly: MappedModifier::Add,
        name_type: Some(Arc::new(TypeExpr::TemplateLiteral {
            quasis: vec!["get".into(), "".into()],
            expressions: Arc::from([TypeExpr::TypeParameter(TypeParam {
                name: "K".into(),
                constraint: None,
                default: None,
            })]),
        })),
    };
    let expression = TypeExpr::Union(Arc::from([
        mapped,
        TypeExpr::TypeOf(ValueRef {
            path: vec!["factory".into(), "make".into()],
            type_args: vec![reference("Input")],
        }),
        TypeExpr::ImportType {
            specifier: Arc::from("./module's"),
            qualifier: Arc::from([Arc::from("Nested"), Arc::from("Member")]),
            typeof_query: false,
            type_arguments: Arc::from([reference("Arg")]),
        },
        TypeExpr::RecursiveRef {
            name: Arc::from("Tree"),
            type_arguments: Arc::from([reference("Leaf")]),
            conditional_context: Arc::from([RecursiveConditionalFrame {
                branch: RecursiveConditionalBranch::True,
                decided: true,
                check: Arc::new(reference("Leaf")),
                extends: Arc::new(reference("Node")),
            }]),
        },
        TypeExpr::Tuple {
            readonly: true,
            elements: Arc::from([
                TupleElement {
                    label: Some("head".into()),
                    ty: TypeExpr::Literal(LiteralValue::String("it's".into())),
                    optional: true,
                    rest: false,
                },
                TupleElement {
                    label: Some("tail".into()),
                    ty: TypeExpr::Rest(Arc::new(TypeExpr::Array {
                        element: Arc::new(TypeExpr::Literal(LiteralValue::BigInt("42".into()))),
                        readonly: false,
                    })),
                    optional: false,
                    rest: false,
                },
            ]),
        },
        TypeExpr::Unknown {
            raw: "Custom & Raw".into(),
        },
    ]));

    let rendered = render_type_expr_display(&expression).expect("the complete union must render");
    assert_eq!(
        rendered.text,
        "{ readonly [K in keyof Source as `get${K}`]-?: Source[K] extends Expected ? infer V : never } | typeof factory.make<Input> | import('./module\\'s').Nested.Member<Arg> | Tree<Leaf> | readonly [head?: 'it\\'s', ...tail: Array<42n>] | (Custom & Raw)"
    );
    assert_eq!(
        rendered
            .referenced_type_names
            .iter()
            .map(AsRef::as_ref)
            .collect::<Vec<_>>(),
        vec!["Source", "Expected", "Input", "Arg", "Tree", "Leaf"]
    );
}

#[test]
fn preserves_first_use_reference_order_and_deduplicates_names() {
    let expression = TypeExpr::Intersection(Arc::from([
        reference("Second"),
        generic_reference("First", vec![reference("Second")]),
        TypeExpr::RecursiveRef {
            name: Arc::from("First"),
            type_arguments: Arc::from([]),
            conditional_context: Arc::from([]),
        },
    ]));

    let rendered = render_type_expr_display(&expression).unwrap();
    assert_eq!(
        rendered
            .referenced_type_names
            .iter()
            .map(AsRef::as_ref)
            .collect::<Vec<_>>(),
        vec!["Second", "First"]
    );
}

#[test]
fn emits_only_the_parentheses_required_by_typescript_precedence() {
    let symbolic_member = TypeExpr::IndexedAccess {
        object: Arc::new(reference("RowApi")),
        index: Arc::new(TypeExpr::Literal(LiteralValue::String("name".into()))),
    };
    assert_eq!(
        render_type_expr_display(&symbolic_member).unwrap().text,
        "RowApi['name']"
    );

    let union = TypeExpr::Union(Arc::from([
        TypeExpr::Primitive(PrimitiveName::String),
        TypeExpr::Primitive(PrimitiveName::Number),
    ]));
    assert_eq!(
        render_type_expr_display(&union).unwrap().text,
        "string | number"
    );

    let intersection = TypeExpr::Intersection(Arc::from([
        union,
        TypeExpr::Function(Arc::new(function(
            Vec::new(),
            TypeExpr::Primitive(PrimitiveName::Void),
            Vec::new(),
        ))),
    ]));
    assert_eq!(
        render_type_expr_display(&intersection).unwrap().text,
        "(string | number) & (() => void)"
    );
}

#[test]
fn rejects_internal_or_malformed_carriers_instead_of_rendering_unknown() {
    let synthetic = TypeExpr::SyntheticSlotBinding(Arc::new(SyntheticCarrierKey {
        scope_canonical_id: Arc::from("/src/Component.svelte"),
        surface_kind: SyntheticCarrierSurfaceKind::SlotBinding,
        slot_name: Some(Arc::from("default")),
        binding_name: Arc::from("item"),
        value_node: 7,
    }));
    assert_eq!(
        render_type_expr_display(&synthetic),
        Err(TypeExprDisplayError::InternalCarrier {
            kind: "SyntheticSlotBinding"
        })
    );

    assert_eq!(
        render_type_expr_display(&TypeExpr::Unknown { raw: String::new() }),
        Err(TypeExprDisplayError::EmptyUnknownSource)
    );
    assert_eq!(
        render_type_expr_display(&TypeExpr::TemplateLiteral {
            quasis: vec!["only".into()],
            expressions: Arc::from([reference("T")]),
        }),
        Err(TypeExprDisplayError::InvalidTemplateLiteralArity {
            quasis: 1,
            expressions: 1,
        })
    );
    assert_eq!(
        render_type_expr_display(&TypeExpr::Literal(LiteralValue::Number(f64::NAN))),
        Err(TypeExprDisplayError::NonFiniteNumberLiteral)
    );
    assert_eq!(
        render_type_expr_display(&TypeExpr::Rest(Arc::new(reference("T")))),
        Err(TypeExprDisplayError::StandaloneRestType)
    );
    assert_eq!(
        render_type_expr_display(&TypeExpr::TemplateLiteral {
            quasis: vec!["unescaped`delimiter".into()],
            expressions: Arc::from([]),
        }),
        Err(TypeExprDisplayError::InvalidTemplateLiteralQuasi { index: 0 })
    );
    assert_eq!(
        render_type_expr_display(&TypeExpr::ImportType {
            specifier: Arc::from("pkg"),
            qualifier: Arc::from([]),
            typeof_query: false,
            type_arguments: Arc::from([reference("T")]),
        }),
        Err(TypeExprDisplayError::ImportTypeArgumentsWithoutQualifier)
    );
}

#[test]
fn renders_remaining_terminal_and_constructor_forms_without_invalid_empty_operators() {
    let constructor = TypeExpr::ConstructorType(Arc::new(function(
        vec![FunctionParam::synthetic(
            None,
            TypeExpr::Array {
                element: Arc::new(TypeExpr::Primitive(PrimitiveName::String)),
                readonly: false,
            },
            false,
            true,
        )],
        TypeExpr::Primitive(PrimitiveName::Void),
        Vec::new(),
    )));
    let expression = TypeExpr::Intersection(Arc::from([
        constructor,
        TypeExpr::Parenthesized(Arc::new(TypeExpr::Literal(LiteralValue::Boolean(true)))),
        TypeExpr::Literal(LiteralValue::Number(-12.5)),
        TypeExpr::ImportType {
            specifier: Arc::from("pkg"),
            qualifier: Arc::from([]),
            typeof_query: true,
            type_arguments: Arc::from([]),
        },
        TypeExpr::Union(Arc::from([])),
        TypeExpr::Intersection(Arc::from([])),
    ]));

    assert_eq!(
        render_type_expr_display(&expression).unwrap().text,
        "(new (..._arg0: Array<string>) => void) & (true) & -12.5 & typeof import('pkg') & never & unknown"
    );
}

#[test]
fn deep_finite_structure_renders_completely_without_a_depth_cap() {
    const DEPTH: usize = 20_000;
    let mut expression = reference("Leaf");
    for _ in 0..DEPTH {
        expression = TypeExpr::Parenthesized(Arc::new(expression));
    }

    let rendered = render_type_expr_display(&expression).expect("deep finite input is complete");
    assert_eq!(rendered.text.len(), "Leaf".len() + DEPTH * 2);
    assert!(rendered.text.starts_with("(((((((("));
    assert!(rendered.text.ends_with("))))))))"));
    assert!(rendered.text.contains("Leaf"));
    assert!(!rendered.text.contains('\u{2026}'));
}
