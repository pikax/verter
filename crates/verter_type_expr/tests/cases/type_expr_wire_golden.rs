//! Byte contract for the `TypeExpr` JSON wire form.
//!
//! The JSON a `TypeExpr` serializes to is read by the FFI, WASM, LSP and
//! session consumers and stored in the oracle snapshots, so its exact bytes
//! are a contract: field names, the sorted key order, which optional fields
//! are written as `null` and which are left out, and how numbers and strings
//! are written. A corpus covering every variant, every member kind and every
//! optional-field case is serialized and compared line by line with the
//! committed golden file.
//!
//! The `serde_json::Value` form ([`TypeExpr::to_json_value`]) must write the
//! same bytes, and must decode back to the node it came from.

use std::sync::Arc;

use verter_type_expr::{
    type_expr_from_json, AuthoredPropertyKey, CanonicalIndexInt, CompilerIntrinsicTypeOp,
    ExcessPropertyOrigin, FunctionExpr, FunctionParam, IndexSignature, LiteralValue,
    MappedModifier, MemberSpans, MemberVisibility, MethodSignature, ObjectExpr, ObjectMember,
    ObjectProperty, OwnedSlice, OwnedStr, PrimitiveName, RecursiveConditionalBranch,
    RecursiveConditionalFrame, SpreadMember, SyntheticCarrierKey, SyntheticCarrierSurfaceKind,
    TopLevelOwnerId, TupleElement, TypeExpr, TypeParam, TypePredicate, TypePredicateSubject,
    UnknownValue, ValueDeclIdentityPart, ValueRef,
};

const GOLDEN: &str = include_str!("type_expr_wire_golden.txt");

fn arc(expr: TypeExpr) -> Arc<TypeExpr> {
    Arc::new(expr)
}

fn string() -> TypeExpr {
    TypeExpr::Primitive(PrimitiveName::String)
}

fn number() -> TypeExpr {
    TypeExpr::Primitive(PrimitiveName::Number)
}

fn empty() -> Arc<[TypeExpr]> {
    Arc::from(Vec::new())
}

fn object(empty_literal: bool, properties: Vec<ObjectMember>) -> TypeExpr {
    TypeExpr::Object(Arc::new(ObjectExpr {
        empty_literal,
        properties,
    }))
}

fn property(key: &str, ty: TypeExpr) -> ObjectProperty {
    ObjectProperty::synthetic_public_key(key.into(), ty, false, false)
}

fn type_param(name: &str, constraint: Option<TypeExpr>, default: Option<TypeExpr>) -> TypeParam {
    TypeParam {
        name: name.into(),
        constraint: constraint.map(arc),
        default: default.map(arc),
        is_const: false,
    }
}

fn function(type_parameters: Vec<TypeParam>, return_type: Option<TypeExpr>) -> FunctionExpr {
    FunctionExpr::synthetic(
        vec![
            FunctionParam::synthetic(Some("a".into()), string(), false, false),
            FunctionParam::synthetic(None, number(), true, false),
            FunctionParam::synthetic(
                Some("rest".into()),
                TypeExpr::Array {
                    element: arc(string()),
                    readonly: true,
                },
                false,
                true,
            ),
        ],
        return_type.map(arc),
        type_parameters,
    )
}

fn predicate(subject: TypePredicateSubject, asserts: bool, ty: Option<TypeExpr>) -> FunctionExpr {
    function(
        Vec::new(),
        Some(TypeExpr::Primitive(PrimitiveName::Boolean)),
    )
    .with_predicate(Some(Arc::new(TypePredicate {
        subject,
        asserts,
        ty: ty.map(arc),
    })))
}

fn unique_symbol_key() -> verter_type_expr::TypeAuthoredPropertyKey {
    AuthoredPropertyKey::UniqueSymbol(ValueDeclIdentityPart {
        canonical_id: OwnedStr::from("/src/keys.ts"),
        owner: TopLevelOwnerId::ordinary_file(),
        symbol: OwnedStr::from("token"),
        member_path: OwnedSlice::from(vec!["inner".to_string()].into_boxed_slice()),
    })
}

fn number_key(value: i64) -> verter_type_expr::TypeAuthoredPropertyKey {
    AuthoredPropertyKey::Number(CanonicalIndexInt::from_canonical_i64(value).expect("canonical"))
}

fn nested_tuple(depth: usize) -> TypeExpr {
    let inner = if depth == 0 {
        string()
    } else {
        nested_tuple(depth - 1)
    };
    TypeExpr::Tuple {
        elements: Arc::from(vec![TupleElement {
            label: Some(format!("e{depth}")),
            ty: inner,
            optional: false,
            rest: false,
        }]),
        readonly: depth.is_multiple_of(2),
    }
}

fn nested_heritage(depth: usize) -> TypeExpr {
    let inner = if depth == 0 {
        string()
    } else {
        nested_heritage(depth - 1)
    };
    let method = MethodSignature::synthetic_public_key(
        format!("m{depth}").as_str().into(),
        function(
            vec![type_param("T", Some(inner.clone()), None)],
            Some(inner),
        ),
        false,
    );
    object(
        false,
        vec![
            ObjectMember::Method(method),
            ObjectMember::Property(property(
                "base",
                TypeExpr::Mapped {
                    parameter: "K".into(),
                    source: arc(TypeExpr::KeyOf(arc(TypeExpr::named("T")))),
                    value: arc(TypeExpr::IndexedAccess {
                        object: arc(TypeExpr::named("T")),
                        index: arc(TypeExpr::named("K")),
                    }),
                    optional: MappedModifier::Remove,
                    readonly: MappedModifier::Add,
                    name_type: None,
                },
            )),
        ],
    )
}

/// Every variant, member kind and optional-field case of the wire form.
fn corpus() -> Vec<(&'static str, TypeExpr)> {
    let mut v: Vec<(&'static str, TypeExpr)> = Vec::new();

    for (label, name) in [
        ("primitive-string", PrimitiveName::String),
        ("primitive-number", PrimitiveName::Number),
        ("primitive-boolean", PrimitiveName::Boolean),
        ("primitive-symbol", PrimitiveName::Symbol),
        ("primitive-bigint", PrimitiveName::BigInt),
        ("primitive-any", PrimitiveName::Any),
        ("primitive-unknown", PrimitiveName::Unknown),
        ("primitive-void", PrimitiveName::Void),
        ("primitive-never", PrimitiveName::Never),
        ("primitive-null", PrimitiveName::Null),
        ("primitive-undefined", PrimitiveName::Undefined),
        ("primitive-object", PrimitiveName::Object),
    ] {
        v.push((label, TypeExpr::Primitive(name)));
    }

    // Literals: string escaping and non-ASCII, and every float edge case.
    v.push((
        "literal-string",
        TypeExpr::Literal(LiteralValue::String("hi".into())),
    ));
    v.push((
        "literal-string-escapes",
        TypeExpr::Literal(LiteralValue::String(
            "quote\" back\\ nl\n tab\t ctl\u{1} uni\u{e9}\u{1f600} sep\u{2028}".into(),
        )),
    ));
    for (label, value) in [
        ("literal-number-int", 42.0),
        ("literal-number-fraction", 1.5),
        ("literal-number-negative", -7.25),
        ("literal-number-neg-zero", -0.0),
        ("literal-number-huge", 1e300),
        ("literal-number-tiny", 5e-324),
        ("literal-number-max-safe", 9_007_199_254_740_993.0),
        ("literal-number-nan", f64::NAN),
        ("literal-number-inf", f64::INFINITY),
        ("literal-number-neg-inf", f64::NEG_INFINITY),
    ] {
        v.push((label, TypeExpr::Literal(LiteralValue::Number(value))));
    }
    v.push((
        "literal-boolean",
        TypeExpr::Literal(LiteralValue::Boolean(false)),
    ));
    v.push((
        "literal-bigint",
        TypeExpr::Literal(LiteralValue::BigInt(
            "123456789012345678901234567890".into(),
        )),
    ));

    v.push(("union-empty", TypeExpr::Union(empty())));
    v.push((
        "union",
        TypeExpr::Union(Arc::from(vec![string(), number()])),
    ));
    v.push((
        "intersection",
        TypeExpr::Intersection(Arc::from(vec![TypeExpr::named("A"), TypeExpr::named("B")])),
    ));
    v.push((
        "array",
        TypeExpr::Array {
            element: arc(string()),
            readonly: false,
        },
    ));
    v.push((
        "array-readonly",
        TypeExpr::Array {
            element: arc(TypeExpr::named("Foo")),
            readonly: true,
        },
    ));
    v.push((
        "tuple",
        TypeExpr::Tuple {
            elements: Arc::from(vec![
                TupleElement {
                    label: Some("a".into()),
                    ty: string(),
                    optional: false,
                    rest: false,
                },
                TupleElement {
                    label: None,
                    ty: number(),
                    optional: true,
                    rest: false,
                },
                TupleElement {
                    label: Some("rest".into()),
                    ty: TypeExpr::Array {
                        element: arc(string()),
                        readonly: false,
                    },
                    optional: false,
                    rest: true,
                },
            ]),
            readonly: true,
        },
    ));
    v.push((
        "tuple-empty",
        TypeExpr::Tuple {
            elements: Arc::from(Vec::new()),
            readonly: false,
        },
    ));

    // Objects: the empty-literal marker, every member kind, every property
    // key kind, and the non-default visibility and excess origin markers.
    v.push(("object-empty", object(false, Vec::new())));
    v.push(("object-empty-literal", object(true, Vec::new())));
    v.push((
        "object-members",
        object(
            false,
            vec![
                ObjectMember::Property(ObjectProperty::synthetic_public_key(
                    "p".into(),
                    string(),
                    true,
                    true,
                )),
                ObjectMember::IndexSignature(IndexSignature::synthetic(
                    "key".into(),
                    string(),
                    number(),
                    true,
                )),
                ObjectMember::CallSignature(function(Vec::new(), Some(string()))),
                ObjectMember::ConstructSignature(function(vec![type_param("T", None, None)], None)),
                ObjectMember::Method(MethodSignature::synthetic_public_key(
                    "m".into(),
                    function(Vec::new(), Some(TypeExpr::Primitive(PrimitiveName::Void))),
                    true,
                )),
                ObjectMember::Spread(SpreadMember::new(TypeExpr::named("Base"))),
            ],
        ),
    ));
    v.push((
        "object-property-keys",
        object(
            false,
            vec![
                ObjectMember::Property(property("plain", string())),
                ObjectMember::Property(property("needs \"quotes\"", string())),
                ObjectMember::Property(ObjectProperty::synthetic_public_key(
                    number_key(0),
                    number(),
                    false,
                    false,
                )),
                ObjectMember::Property(ObjectProperty::synthetic_public_key(
                    number_key(42),
                    number(),
                    false,
                    false,
                )),
                ObjectMember::Property(ObjectProperty::synthetic_public_key(
                    number_key(-3),
                    number(),
                    false,
                    false,
                )),
                ObjectMember::Property(ObjectProperty::synthetic_public_key(
                    unique_symbol_key(),
                    TypeExpr::Primitive(PrimitiveName::Symbol),
                    false,
                    true,
                )),
                ObjectMember::Property(ObjectProperty::synthetic_public_key(
                    AuthoredPropertyKey::Computed(TypeExpr::Literal(LiteralValue::String(
                        "computed".into(),
                    ))),
                    string(),
                    false,
                    false,
                )),
                ObjectMember::Method(MethodSignature::synthetic_public_key(
                    unique_symbol_key(),
                    function(Vec::new(), None),
                    false,
                )),
            ],
        ),
    ));
    let mut markers = Vec::new();
    for visibility in [
        MemberVisibility::Public,
        MemberVisibility::Protected,
        MemberVisibility::Private,
    ] {
        for origin in [
            ExcessPropertyOrigin::NonLiteral,
            ExcessPropertyOrigin::FreshOwn,
            ExcessPropertyOrigin::SpreadTainted,
        ] {
            markers.push(ObjectMember::Property(
                ObjectProperty::with_key_visibility(
                    "p".into(),
                    string(),
                    false,
                    false,
                    visibility,
                    MemberSpans::default(),
                )
                .with_excess_origin(origin),
            ));
            markers.push(ObjectMember::Method(
                MethodSignature::with_key_visibility(
                    "m".into(),
                    function(Vec::new(), None),
                    false,
                    visibility,
                    MemberSpans::default(),
                )
                .with_excess_origin(origin),
            ));
        }
    }
    v.push(("object-member-markers", object(false, markers)));

    // Functions: return present and absent, type parameters with every
    // optional part, predicates over a parameter and over `this`, abstract
    // constructors.
    v.push((
        "function",
        TypeExpr::Function(Arc::new(function(Vec::new(), Some(string())))),
    ));
    v.push((
        "function-no-return",
        TypeExpr::Function(Arc::new(function(Vec::new(), None))),
    ));
    v.push((
        "function-type-parameters",
        TypeExpr::Function(Arc::new(function(
            vec![
                type_param("A", None, None),
                type_param("B", Some(TypeExpr::named("Base")), None),
                type_param("C", None, Some(string())),
                type_param("D", Some(string()), Some(TypeExpr::named("Lit"))),
                TypeParam {
                    name: "E".into(),
                    constraint: None,
                    default: None,
                    is_const: true,
                },
            ],
            Some(TypeExpr::named("A")),
        ))),
    ));
    v.push((
        "function-predicate-parameter",
        TypeExpr::Function(Arc::new(predicate(
            TypePredicateSubject::Parameter(Arc::from("a")),
            false,
            Some(TypeExpr::named("Narrow")),
        ))),
    ));
    v.push((
        "function-asserts-parameter",
        TypeExpr::Function(Arc::new(predicate(
            TypePredicateSubject::Parameter(Arc::from("a")),
            true,
            None,
        ))),
    ));
    v.push((
        "function-predicate-this",
        TypeExpr::Function(Arc::new(predicate(
            TypePredicateSubject::This,
            false,
            Some(TypeExpr::named("Self")),
        ))),
    ));
    v.push((
        "function-asserts-this",
        TypeExpr::Function(Arc::new(predicate(TypePredicateSubject::This, true, None))),
    ));
    v.push((
        "constructor-type",
        TypeExpr::ConstructorType(Arc::new(function(
            vec![type_param("T", None, None)],
            Some(TypeExpr::named("T")),
        ))),
    ));
    v.push((
        "constructor-type-abstract",
        TypeExpr::ConstructorType(Arc::new(
            predicate(TypePredicateSubject::This, true, None).with_abstract(true),
        )),
    ));

    v.push((
        "ref",
        TypeExpr::Ref {
            name: Arc::from("Bare"),
            type_arguments: empty(),
        },
    ));
    v.push((
        "ref-arguments",
        TypeExpr::named_with_args("Record", vec![string(), TypeExpr::named("V")]),
    ));
    v.push((
        "intrinsic-application",
        TypeExpr::IntrinsicApplication {
            op: CompilerIntrinsicTypeOp::Awaited,
            arguments: Arc::from(vec![TypeExpr::named("P")]),
        },
    ));
    v.push((
        "import-type-typeof",
        TypeExpr::import_type("./m", vec![], true, vec![]),
    ));
    v.push((
        "import-type-qualified-generic",
        TypeExpr::import_type(
            "./m",
            vec![Arc::from("A"), Arc::from("Gen")],
            false,
            vec![number()],
        ),
    ));
    v.push((
        "type-parameter",
        TypeExpr::TypeParameter(type_param("U", None, None)),
    ));
    v.push((
        "type-parameter-full",
        TypeExpr::TypeParameter(TypeParam {
            name: "T".into(),
            constraint: Some(arc(TypeExpr::named("Base"))),
            default: Some(arc(string())),
            is_const: true,
        }),
    ));
    v.push(("keyof", TypeExpr::KeyOf(arc(TypeExpr::named("Foo")))));
    v.push((
        "typeof",
        TypeExpr::TypeOf(ValueRef {
            path: vec!["a".into(), "b".into()],
            type_args: vec![],
        }),
    ));
    v.push((
        "typeof-empty-path",
        TypeExpr::TypeOf(ValueRef {
            path: vec![],
            type_args: vec![],
        }),
    ));
    v.push((
        "typeof-instantiation",
        TypeExpr::TypeOf(ValueRef {
            path: vec!["C".into(), "make".into()],
            type_args: vec![string()],
        }),
    ));
    v.push((
        "indexed-access",
        TypeExpr::IndexedAccess {
            object: arc(TypeExpr::named("Foo")),
            index: arc(TypeExpr::Literal(LiteralValue::String("bar".into()))),
        },
    ));
    v.push((
        "conditional",
        TypeExpr::Conditional {
            check: arc(TypeExpr::named("T")),
            extends: arc(TypeExpr::named("U")),
            true_type: arc(string()),
            false_type: arc(number()),
        },
    ));
    for (label, optional, readonly, name_type) in [
        ("mapped", MappedModifier::None, MappedModifier::None, None),
        (
            "mapped-modifiers",
            MappedModifier::Add,
            MappedModifier::Remove,
            None,
        ),
        (
            "mapped-name-type",
            MappedModifier::Remove,
            MappedModifier::Add,
            Some(arc(TypeExpr::TemplateLiteral {
                quasis: vec!["get".into(), "".into()],
                expressions: Arc::from(vec![TypeExpr::named("K")]),
            })),
        ),
    ] {
        v.push((
            label,
            TypeExpr::Mapped {
                parameter: "K".into(),
                source: arc(TypeExpr::KeyOf(arc(TypeExpr::named("T")))),
                value: arc(TypeExpr::IndexedAccess {
                    object: arc(TypeExpr::named("T")),
                    index: arc(TypeExpr::named("K")),
                }),
                optional,
                readonly,
                name_type,
            },
        ));
    }
    v.push((
        "template-literal",
        TypeExpr::TemplateLiteral {
            quasis: vec!["pre-".into(), "-mid-".into(), "-post".into()],
            expressions: Arc::from(vec![TypeExpr::named("A"), number()]),
        },
    ));
    v.push((
        "template-literal-no-expressions",
        TypeExpr::TemplateLiteral {
            quasis: vec!["only".into()],
            expressions: empty(),
        },
    ));
    v.push((
        "infer",
        TypeExpr::Infer {
            name: "R".into(),
            constraint: None,
        },
    ));
    v.push((
        "infer-constraint",
        TypeExpr::Infer {
            name: "R".into(),
            constraint: Some(arc(string())),
        },
    ));
    v.push(("rest", TypeExpr::Rest(arc(TypeExpr::named("R")))));
    v.push((
        "parenthesized",
        TypeExpr::Parenthesized(arc(TypeExpr::Union(Arc::from(vec![
            TypeExpr::named("A"),
            TypeExpr::named("B"),
        ])))),
    ));
    v.push((
        "recursive-ref",
        TypeExpr::RecursiveRef {
            name: Arc::from("Rec"),
            type_arguments: Arc::from(vec![TypeExpr::named("T")]),
            conditional_context: Arc::from(vec![
                RecursiveConditionalFrame {
                    branch: RecursiveConditionalBranch::True,
                    decided: true,
                    check: arc(TypeExpr::named("C")),
                    extends: arc(TypeExpr::named("E")),
                },
                RecursiveConditionalFrame {
                    branch: RecursiveConditionalBranch::False,
                    decided: false,
                    check: arc(TypeExpr::Primitive(PrimitiveName::Never)),
                    extends: arc(TypeExpr::Primitive(PrimitiveName::Unknown)),
                },
            ]),
        },
    ));
    v.push((
        "recursive-ref-empty",
        TypeExpr::RecursiveRef {
            name: Arc::from("Rec"),
            type_arguments: empty(),
            conditional_context: Arc::from(Vec::new()),
        },
    ));
    v.push((
        "synthetic-slot-binding",
        TypeExpr::SyntheticSlotBinding(Arc::new(SyntheticCarrierKey {
            scope_canonical_id: From::from("/owner.vue"),
            surface_kind: SyntheticCarrierSurfaceKind::SlotBinding,
            slot_name: Some(From::from("default")),
            binding_name: From::from("row"),
            value_node: 18_446_744_073_709_551_615,
        })),
    ));
    v.push((
        "synthetic-binding",
        TypeExpr::SyntheticSlotBinding(Arc::new(SyntheticCarrierKey {
            scope_canonical_id: From::from("/owner.vue"),
            surface_kind: SyntheticCarrierSurfaceKind::Binding,
            slot_name: None,
            binding_name: From::from("b"),
            value_node: 0,
        })),
    ));
    v.push((
        "unknown",
        TypeExpr::Unknown(UnknownValue::unsupported_syntax("weird<\"x\">")),
    ));
    v.push((
        "unknown-missing-output",
        TypeExpr::Unknown(UnknownValue::missing_output()),
    ));

    v.push(("nested-tuple", nested_tuple(6)));
    v.push(("nested-heritage", nested_heritage(3)));
    v
}

fn encode(expr: &TypeExpr) -> String {
    serde_json::to_string(expr).expect("a TypeExpr serializes")
}

#[test]
fn every_wire_form_matches_the_golden_bytes() {
    let corpus = corpus();
    let golden: Vec<(&str, &str)> = GOLDEN
        .lines()
        .map(|line| {
            line.split_once('\t')
                .expect("a golden line is `label<TAB>json`")
        })
        .collect();
    let labels: Vec<&str> = corpus.iter().map(|(label, _)| *label).collect();
    let golden_labels: Vec<&str> = golden.iter().map(|(label, _)| *label).collect();
    assert_eq!(
        labels, golden_labels,
        "the corpus and the golden file list the same cases"
    );
    for ((label, expr), (_, expected)) in corpus.iter().zip(&golden) {
        assert_eq!(
            encode(expr),
            *expected,
            "{label}: the wire bytes changed; the golden line is the contract"
        );
    }
}

#[test]
fn the_value_form_writes_the_same_bytes_and_decodes_to_the_same_node() {
    for (label, expr) in corpus() {
        let value = expr.to_json_value();
        assert_eq!(
            serde_json::to_string(&value).expect("a Value serializes"),
            encode(&expr),
            "{label}: the Value form and the streamed form differ"
        );
        assert_eq!(
            serde_json::from_str::<serde_json::Value>(&encode(&expr)).expect("valid JSON"),
            value,
            "{label}: the streamed JSON does not parse to the Value form"
        );
        // A non-finite number has no JSON number and is written as `null`,
        // and the decoder reads `-0.0` as `0`: neither literal decodes back
        // to itself, so they have no round trip to check.
        if matches!(&expr, TypeExpr::Literal(LiteralValue::Number(n))
            if !n.is_finite() || (*n == 0.0 && n.is_sign_negative()))
        {
            continue;
        }
        assert_eq!(
            type_expr_from_json(&value).as_ref(),
            Some(&expr),
            "{label}: the wire form decodes to a different node"
        );
    }
}
