//! Named applications remain semantic carriers at publication reference sites;
//! demanded members and operators read their instantiated structural meaning.
use super::support::*;

const FILE: &str = "/identity/probe.ts";
const SOURCE: &str = include_str!("fixtures/generic_interface_identity.ts");

fn host(strict_null_checks: bool, no_implicit_any: bool) -> Arc<crate::VerterHost> {
    let host = Arc::new(crate::VerterHost::new_standalone_with_tsconfig_projects(
        crate::HostConfig::default(),
        &[(
            "/identity",
            &format!(
                r#"{{ "compilerOptions": {{ "strict": true, "strictNullChecks": {strict_null_checks}, "noImplicitAny": {no_implicit_any} }} }}"#,
            ),
        )],
    ));
    upsert_ts(&host, FILE, SOURCE);
    host
}

fn published(
    host: &crate::VerterHost,
    name: &str,
    argument: PrimitiveName,
    mode: ProjectionMode,
) -> TypeExpr {
    let args = [Arc::new(TypeExpr::Primitive(argument))];
    published_arguments(host, name, &args, mode)
}

fn published_arguments(
    host: &crate::VerterHost,
    name: &str,
    args: &[Arc<TypeExpr>],
    mode: ProjectionMode,
) -> TypeExpr {
    let (outcome, _) = host
        .resolve_named_symbol_wire_with_audit(FILE, name, args, Some(mode))
        .into_parts();
    let root = outcome
        .expect("publication must not fault")
        .expect("named type");
    let bytes = host
        .project_node_to_type_expr_json_bytes(&root)
        .expect("wire publication");
    let expr: TypeExpr = serde_json::from_slice(&bytes).expect("typed wire payload");
    if args.is_empty() {
        let root = host
            .resolve_named_symbol(FILE, name, Some(mode))
            .expect("bare named type");
        let named = host
            .project_node_to_type_expr_json_bytes(&root)
            .expect("named publication");
        let named: TypeExpr = serde_json::from_slice(&named).expect("typed named payload");
        assert_eq!(expr, named, "named and wire publication: {name}");
    }
    expr
}

fn assert_application(expr: &TypeExpr, name: &str, argument: PrimitiveName) {
    assert_application_arguments(expr, name, &[TypeExpr::Primitive(argument)]);
}

fn assert_application_arguments(expr: &TypeExpr, name: &str, expected: &[TypeExpr]) {
    let TypeExpr::Intersection(arms) = expr else {
        panic!("named application must survive the intersection: {expr:?}");
    };
    let application = arms
        .iter()
        .find(|arm| matches!(arm, TypeExpr::Ref { name: found, .. } if found.as_ref() == name))
        .unwrap_or_else(|| panic!("missing {name} application: {expr:?}"));
    let TypeExpr::Ref { type_arguments, .. } = application else {
        unreachable!()
    };
    assert_eq!(type_arguments.as_ref(), expected);
}

#[test]
fn generic_interface_identity_preserves_structural_intersection_reduction() {
    for strict in [false, true] {
        for implicit in [false, true] {
            let host = host(strict, implicit);
            for _ in 0..2 {
                let conflict =
                    published_arguments(&host, "Conflict", &[], ProjectionMode::Expanded);
                assert_primitive(&conflict, PrimitiveName::Never);
                let conditional =
                    published_arguments(&host, "ConflictIsNever", &[], ProjectionMode::Expanded);
                assert_boolean_literal(&conditional, true);
                for (name, non_strict) in [
                    ("NullIntersection", PrimitiveName::Null),
                    ("UndefinedIntersection", PrimitiveName::Undefined),
                ] {
                    let nullish = published_arguments(&host, name, &[], ProjectionMode::Expanded);
                    assert_primitive(
                        &nullish,
                        if strict {
                            PrimitiveName::Never
                        } else {
                            non_strict
                        },
                    );
                }
                for name in ["NullIntersectionIsNever", "UndefinedIntersectionIsNever"] {
                    let conditional =
                        published_arguments(&host, name, &[], ProjectionMode::Expanded);
                    assert_boolean_literal(&conditional, strict);
                }
                let keys = published_arguments(
                    &host,
                    "NullIntersectionKeys",
                    &[],
                    ProjectionMode::Expanded,
                );
                if strict {
                    let TypeExpr::Union(arms) = &keys else {
                        panic!("keyof never: {keys:?}")
                    };
                    assert_eq!(arms.len(), 3);
                    for kind in [
                        PrimitiveName::String,
                        PrimitiveName::Number,
                        PrimitiveName::Symbol,
                    ] {
                        assert!(
                            arms.iter().any(
                                |arm| matches!(arm, TypeExpr::Primitive(found) if *found == kind)
                            ),
                            "missing {kind:?}: {keys:?}"
                        );
                    }
                } else {
                    assert_primitive(&keys, PrimitiveName::Never);
                }
                // Nullability erasure does not make disjoint scalar domains overlap.
                for name in ["NullScalarIntersection", "UndefinedScalarIntersection"] {
                    assert_primitive(
                        &published_arguments(&host, name, &[], ProjectionMode::Expanded),
                        PrimitiveName::Never,
                    );
                }
                let (compatible, _) = evaluate_expr(
                    &host,
                    FILE,
                    "Tag<'a'> & { kind: 'a' }",
                    ProjectionMode::Expanded,
                );
                assert!(!matches!(
                    compatible,
                    TypeExpr::Primitive(PrimitiveName::Never)
                ));
            }
        }
    }
}

#[test]
fn generic_interface_identity_completes_substituted_defaults() {
    for strict in [false, true] {
        for implicit in [false, true] {
            let host = host(strict, implicit);
            for _ in 0..2 {
                for name in ["DefaultState", "PartialState", "DefaultNested"] {
                    let expr = published_arguments(&host, name, &[], ProjectionMode::Expanded);
                    match name {
                        "DefaultState" => {
                            assert_application(&expr, "DefaultEmpty", PrimitiveName::String)
                        }
                        "PartialState" => assert_application_arguments(
                            &expr,
                            "Pair",
                            &[
                                TypeExpr::Primitive(PrimitiveName::Number),
                                TypeExpr::Primitive(PrimitiveName::Number),
                            ],
                        ),
                        _ => {
                            let TypeExpr::Tuple { elements, .. } = &expr else {
                                panic!("default tuple: {expr:?}")
                            };
                            assert_application(
                                &elements[0].ty,
                                "DefaultEmpty",
                                PrimitiveName::String,
                            );
                            assert_application_arguments(
                                &object_props(&elements[1].ty)["state"].ty,
                                "Pair",
                                &[
                                    TypeExpr::Primitive(PrimitiveName::Number),
                                    TypeExpr::Primitive(PrimitiveName::Number),
                                ],
                            );
                        }
                    }
                }
            }
            upsert_ts(
                &host,
                FILE,
                &SOURCE
                    .replace("DefaultEmpty<T = string>", "DefaultEmpty<T = number>")
                    .replace("Pair<T, U = T>", "Pair<T, U = string>"),
            );
            for _ in 0..2 {
                assert_application(
                    &published_arguments(&host, "DefaultState", &[], ProjectionMode::Expanded),
                    "DefaultEmpty",
                    PrimitiveName::Number,
                );
                assert_application_arguments(
                    &published_arguments(&host, "PartialState", &[], ProjectionMode::Expanded),
                    "Pair",
                    &[
                        TypeExpr::Primitive(PrimitiveName::Number),
                        TypeExpr::Primitive(PrimitiveName::String),
                    ],
                );
            }
        }
    }
}

#[test]
fn generic_interface_identity_reduces_closed_default_arguments() {
    use verter_type_expr::LiteralValue;
    let key = TypeExpr::Literal(LiteralValue::String("v".into()));
    for strict in [false, true] {
        for implicit in [false, true] {
            let host = host(strict, implicit);
            for _ in 0..2 {
                let expr =
                    published_arguments(&host, "ClosedDefaults", &[], ProjectionMode::Expanded);
                let TypeExpr::Tuple { elements, .. } = &expr else {
                    panic!("default tuple: {expr:?}")
                };
                assert_application_arguments(
                    &elements[0].ty,
                    "KeyEmpty",
                    std::slice::from_ref(&key),
                );
                assert_application_arguments(
                    &object_props(&elements[1].ty)["state"].ty,
                    "KeyEmpty",
                    std::slice::from_ref(&key),
                );
                assert_application(&elements[2].ty, "IndexedEmpty", PrimitiveName::String);
                assert_application_arguments(
                    &published_arguments(&host, "ExplicitKeyState", &[], ProjectionMode::Expanded),
                    "KeyEmpty",
                    std::slice::from_ref(&key),
                );
            }
            upsert_ts(
                &host,
                FILE,
                &SOURCE.replace("Value = { v: string }", "Value = { v: number }"),
            );
            let expr = published_arguments(&host, "ClosedDefaults", &[], ProjectionMode::Expanded);
            let TypeExpr::Tuple { elements, .. } = &expr else {
                panic!("edited default tuple: {expr:?}")
            };
            assert_application(&elements[2].ty, "IndexedEmpty", PrimitiveName::Number);
        }
    }
}

#[test]
fn generic_interface_identity_survives_named_and_wire_publication() {
    for strict in [false, true] {
        for implicit in [false, true] {
            let host = host(strict, implicit);
            for argument in [PrimitiveName::String, PrimitiveName::Number] {
                for mode in [ProjectionMode::Navigate, ProjectionMode::Expanded] {
                    // Two reads exercise the shared query and output caches.
                    for _ in 0..2 {
                        let state = published(&host, "State", argument, mode);
                        assert_application(&state, "Empty", argument);
                        let full = published(&host, "Nonempty", argument, mode);
                        assert_application(&full, "Full", argument);
                        let nested = published(&host, "Nested", argument, mode);
                        let TypeExpr::Tuple { elements, .. } = &nested else {
                            panic!("nested publication must stay a tuple");
                        };
                        assert_application(&elements[0].ty, "Empty", argument);
                        let props = object_props(&elements[1].ty);
                        assert_application(&props["state"].ty, "Empty", argument);
                    }
                }
            }
            for mode in [ProjectionMode::Navigate, ProjectionMode::Expanded] {
                let ordinary = published_arguments(&host, "Ordinary", &[], mode);
                assert_application_arguments(&ordinary, "Plain", &[]);
            }
        }
    }
}

#[test]
fn generic_interface_structural_equivalence_is_independent_of_identity() {
    for strict in [false, true] {
        for implicit in [false, true] {
            let host = host(strict, implicit);
            for argument in [PrimitiveName::String, PrimitiveName::Number] {
                for name in ["Equivalent", "NestedEquivalent"] {
                    let equivalent = published(&host, name, argument, ProjectionMode::Expanded);
                    assert_boolean_literal(&equivalent, true);
                }
            }
        }
    }
}

#[test]
fn generic_interface_identity_distinguishes_contributors_from_structural_members() {
    let host = host(true, true);
    for _ in 0..2 {
        let expr = published(
            &host,
            "ReferencePositions",
            PrimitiveName::String,
            ProjectionMode::Expanded,
        );
        let TypeExpr::Tuple { elements, .. } = &expr else {
            panic!("reference positions: {expr:?}")
        };
        assert_application(&elements[0].ty, "Empty", PrimitiveName::String);
        assert_primitive(
            &object_props(&elements[1].ty)["value"].ty,
            PrimitiveName::String,
        );
        let TypeExpr::Array { element, .. } = &elements[2].ty else {
            panic!("array: {:?}", elements[2].ty)
        };
        // Non-generic element references retain their existing carrier form.
        assert!(
            matches!(element.as_ref(), TypeExpr::Ref { name, type_arguments }
            if name.as_ref() == "Plain" && type_arguments.is_empty())
        );
        let members = object_props(&elements[3].ty);
        assert_application(&members["state"].ty, "Empty", PrimitiveName::String);
        assert_primitive(
            &object_props(&members["full"].ty)["value"].ty,
            PrimitiveName::String,
        );
        let selected = published_arguments(&host, "SelectedFull", &[], ProjectionMode::Expanded);
        let TypeExpr::Ref {
            name,
            type_arguments,
        } = &selected
        else {
            panic!("selected declaration must stay named")
        };
        assert_eq!(name.as_ref(), "Full");
        assert_eq!(
            type_arguments.as_ref(),
            &[TypeExpr::Primitive(PrimitiveName::String)]
        );
    }
}

#[test]
fn generic_interface_identity_preserves_indexed_declaration_values() {
    for strict in [false, true] {
        for implicit in [false, true] {
            let host = host(strict, implicit);
            for _ in 0..2 {
                let selected =
                    published_arguments(&host, "SelectedFull", &[], ProjectionMode::Expanded);
                let TypeExpr::Ref {
                    name,
                    type_arguments,
                } = &selected
                else {
                    panic!("selected declaration must stay named")
                };
                assert_eq!(name.as_ref(), "Full");
                assert_eq!(
                    type_arguments.as_ref(),
                    &[TypeExpr::Primitive(PrimitiveName::String)]
                );
            }
        }
    }
}

#[test]
fn generic_interface_identity_survives_expression_publication() {
    let host = host(true, true);
    let (expr, _) = evaluate_expr(
        &host,
        FILE,
        "Empty<string> & { count: number }",
        ProjectionMode::Expanded,
    );
    assert_application(&expr, "Empty", PrimitiveName::String);
}

#[test]
fn generic_interface_members_and_closed_operators_still_reduce() {
    let host = host(true, true);
    for (argument, expected) in [
        ("string", PrimitiveName::String),
        ("number", PrimitiveName::Number),
    ] {
        let operators = published(&host, "Operators", expected, ProjectionMode::Expanded);
        let TypeExpr::Tuple { elements, .. } = &operators else {
            panic!("operator tuple")
        };
        assert_application(&elements[0].ty, "Empty", expected);
        let TypeExpr::Intersection(arms) = &elements[0].ty else {
            unreachable!()
        };
        let selected = arms
            .iter()
            .find(|arm| matches!(arm, TypeExpr::Object(_)))
            .expect("closed conditional alias must reduce beside a named interface");
        assert_primitive(&object_props(selected)["selected"].ty, expected);
        assert_primitive(&elements[1].ty, expected);
        for expression in [
            format!("Child<{argument}>['value']"),
            format!("Full<{argument}>['value']"),
        ] {
            let (expr, _) = evaluate_expr(&host, FILE, &expression, ProjectionMode::Expanded);
            assert_primitive(&expr, expected);
        }
        let (keys, _) = evaluate_expr(
            &host,
            FILE,
            &format!("keyof Child<{argument}>"),
            ProjectionMode::Expanded,
        );
        assert_literal_union(&keys, &["own", "value"]);
        let (selected, _) = evaluate_expr(
            &host,
            FILE,
            &format!("Transparent<{argument}>['selected']"),
            ProjectionMode::Expanded,
        );
        assert_primitive(&selected, expected);
        let (transparent, _) = evaluate_expr(
            &host,
            FILE,
            &format!("Transparent<{argument}>"),
            ProjectionMode::Expanded,
        );
        assert_primitive(&object_props(&transparent)["selected"].ty, expected);
        let child = published(&host, "Child", expected, ProjectionMode::Expanded);
        let child_members = object_props(&child);
        assert_primitive(&child_members["value"].ty, expected);
    }
    let (open, _) = resolve_expr(&host, FILE, "State", &[], ProjectionMode::Skeleton);
    let TypeExpr::Intersection(arms) = &open else {
        panic!("open generic carrier")
    };
    assert!(arms.iter().any(|arm| matches!(arm, TypeExpr::Ref { name, type_arguments } if name.as_ref() == "Empty" && !type_arguments.is_empty())));
}

#[test]
fn edited_generic_interface_keeps_fresh_identity_and_denotation() {
    let host = host(true, true);
    assert_application(
        &published(
            &host,
            "State",
            PrimitiveName::String,
            ProjectionMode::Expanded,
        ),
        "Empty",
        PrimitiveName::String,
    );
    let edited = SOURCE.replace("interface Empty<T> {}", "interface Empty<T> { added: T }");
    upsert_ts(&host, FILE, &edited);
    for _ in 0..2 {
        for (argument, primitive) in [
            ("string", PrimitiveName::String),
            ("number", PrimitiveName::Number),
        ] {
            assert_application(
                &published(&host, "State", primitive, ProjectionMode::Expanded),
                "Empty",
                primitive,
            );
            let (value, _) = evaluate_expr(
                &host,
                FILE,
                &format!("State<{argument}>['added']"),
                ProjectionMode::Expanded,
            );
            assert_primitive(&value, primitive);
        }
    }
    // The same symbol now names a transparent alias. Its old interface
    // identity must not survive a warm declaration-kind classification.
    upsert_ts(
        &host,
        FILE,
        &SOURCE.replace("interface Empty<T> {}", "type Empty<T> = {};"),
    );
    for _ in 0..2 {
        let state = published(
            &host,
            "State",
            PrimitiveName::String,
            ProjectionMode::Expanded,
        );
        let props = object_props(&state);
        assert_eq!(prop_names(&props), vec!["count"]);
        assert_primitive(&props["count"].ty, PrimitiveName::Number);
        assert!(!verter_type_expr::render_type_expr_display(&state)
            .unwrap()
            .text
            .contains("Empty"));
    }
}

#[test]
fn shallow_publication_does_not_read_nested_closed_operators() {
    use crate::output_sinks::DispatchOutputTestExt;
    use verter_type_engine::project_semantic_dispatch::ProjectSemanticDispatch;
    use verter_type_engine::semantic_query::ProjectionReductionContext;

    let host = host(true, true);
    let (outcome, _) = host
        .resolve_named_symbol_wire_with_audit(
            FILE,
            "Operators",
            &[Arc::new(TypeExpr::Primitive(PrimitiveName::String))],
            Some(ProjectionMode::Shallow),
        )
        .into_parts();
    let root = outcome.unwrap().unwrap();
    let dispatch = ProjectSemanticDispatch::new(host.as_ref());
    let shallow = dispatch.materialize_reduced_output_type_expr_for_test(
        root.id(),
        ProjectionReductionContext::published(ProjectionMode::Navigate),
    );
    let TypeExpr::Tuple { elements, .. } = &shallow else {
        panic!("shallow tuple")
    };
    assert!(
        matches!(&elements[1].ty, TypeExpr::IndexedAccess { .. }),
        "nested closed access was read without a demand: {shallow:?}"
    );
    let expanded = dispatch.materialize_reduced_output_type_expr_for_test(
        root.id(),
        ProjectionReductionContext::published(ProjectionMode::Expanded),
    );
    let TypeExpr::Tuple { elements, .. } = &expanded else {
        panic!("expanded tuple")
    };
    assert_primitive(&elements[1].ty, PrimitiveName::String);
}
