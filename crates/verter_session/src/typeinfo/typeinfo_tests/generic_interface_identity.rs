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
    let (outcome, _) = host
        .resolve_named_symbol_wire_with_audit(FILE, name, &args, Some(mode))
        .into_parts();
    let root = outcome
        .expect("publication must not fault")
        .expect("named type");
    let bytes = host
        .project_node_to_type_expr_json_bytes(&root)
        .expect("wire publication");
    serde_json::from_slice(&bytes).expect("typed wire payload")
}

fn assert_application(expr: &TypeExpr, name: &str, argument: PrimitiveName) {
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
    assert_eq!(type_arguments.as_ref(), &[TypeExpr::Primitive(argument)]);
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
            let (ordinary, _) =
                resolve_expr(&host, FILE, "Ordinary", &[], ProjectionMode::Navigate);
            let TypeExpr::Intersection(arms) = &ordinary else {
                panic!("ordinary interface carrier")
            };
            assert!(arms
                .iter()
                .any(|arm| matches!(arm, TypeExpr::Ref { name, .. } if name.as_ref() == "Plain")));
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
