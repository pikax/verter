//! @ai-generated - Synthetic mapped-type and template-literal key tests.

use super::oracle;
use super::support::*;
use std::collections::BTreeSet;
use verter_session_oracle_macro::oracle_row;

fn assert_mapped_recursive_container(indexed: bool) {
    let host = make_host_with_footprint();
    let path = "/recursive-containers.ts";
    let index = if indexed { "['a' | 'b']" } else { "" };
    upsert_ts(
        &host,
        path,
        &format!(
            r#"
type List<T> = [T, List<T>];
type Arr<T> = Arr<T>[];
type Linked<T> = {{ v: T; next: Linked<T> }};
type Pair<T> = [T, T[]];
export type TupleProbe = {{ [K in 'a' | 'b']: List<number> }}{index};
export type ArrayProbe = {{ [K in 'a' | 'b']: Arr<number> }}{index};
export type ObjectProbe = {{ [K in 'a' | 'b']: Linked<number> }}{index};
export type FiniteProbe = {{ [K in 'a' | 'b']: Pair<number> }}{index};
"#
        ),
    );
    for _ in 0..2 {
        for name in ["FiniteProbe", "TupleProbe", "ArrayProbe", "ObjectProbe"] {
            let (expr, record) = resolve_expr(&host, path, name, &[], ProjectionMode::Expanded);
            let verter_audit::RequestKindPayload::TypeResolution(payload) = &record.kind_payload
            else {
                panic!("container publication must retain its resolution audit");
            };
            assert!(
                !payload.cache_suppress
                    && !payload.recursion_limit_reached
                    && payload.walker_diagnostics.is_empty(),
                "a finite recursive surface must remain cacheable: {payload:?}"
            );
            let values = if indexed {
                vec![expr]
            } else {
                let props = object_props(&expr);
                assert_eq!(prop_names(&props), vec!["a", "b"]);
                props.into_values().map(|prop| prop.ty).collect()
            };
            for value in values {
                match name {
                    "FiniteProbe" | "TupleProbe" => {
                        let TypeExpr::Tuple { elements, .. } = &value else {
                            panic!("expected a tuple, got {value:?}");
                        };
                        assert_eq!(elements.len(), 2);
                        assert_primitive(&elements[0].ty, PrimitiveName::Number);
                        if name == "TupleProbe" {
                            assert_ref(&elements[1].ty, "List");
                        } else {
                            let TypeExpr::Array { element, .. } = &elements[1].ty else {
                                panic!("the finite pair must retain its array");
                            };
                            assert_primitive(element, PrimitiveName::Number);
                        }
                    }
                    "ArrayProbe" => {
                        let TypeExpr::Array { element, .. } = &value else {
                            panic!("expected an array, got {value:?}");
                        };
                        assert_ref(element, "Arr");
                    }
                    "ObjectProbe" => {
                        let props = object_props(&value);
                        assert_primitive(&props["v"].ty, PrimitiveName::Number);
                        assert_ref(&props["next"].ty, "Linked");
                    }
                    _ => unreachable!(),
                }
            }
        }
    }
}

#[test]
fn mapped_members_keep_recursive_container_edges_addressable() {
    assert_mapped_recursive_container(false);
}

#[test]
fn mapped_indices_keep_recursive_container_edges_addressable() {
    assert_mapped_recursive_container(true);
}

#[test]
fn mapped_signature_slots_reduce_finite_container_helpers() {
    let host = make_host_with_footprint();
    let path = "/finite-signature-containers.ts";
    upsert_ts(
        &host,
        path,
        r#"
type Inner<T extends { a: number }> = { x: T['a'] };
type Items<T extends { a: number }> = T['a'][];
type Pair<T extends { a: number }> = [T['a']];
type Fn<T extends { a: number }> = (x: Inner<T>) => Inner<T>;
export type ObjectProbe = { [K in 'a']: Fn<{ a: 1 }> };
export type ArrayProbe = { [K in 'a']: () => Items<{ a: 1 }> };
export type TupleProbe = { [K in 'a']: () => Pair<{ a: 1 }> };
export type InlineProbe = { [K in 'a']: () => { a: 1 }['a'][] };
"#,
    );
    for _ in 0..2 {
        for name in ["ObjectProbe", "ArrayProbe", "TupleProbe", "InlineProbe"] {
            let (expr, _) = resolve_expr(&host, path, name, &[], ProjectionMode::Expanded);
            let props = object_props(&expr);
            let signature = function_type(&props["a"].ty);
            let returned = signature.return_type.as_deref().expect("declared return");
            match name {
                "ObjectProbe" => {
                    assert_number_literal(&object_props(returned)["x"].ty, 1.0);
                    assert_number_literal(&object_props(&signature.parameters[0].ty)["x"].ty, 1.0);
                }
                "ArrayProbe" | "InlineProbe" => {
                    let TypeExpr::Array { element, .. } = returned else {
                        panic!("expected an array, got {returned:?}");
                    };
                    assert_number_literal(element, 1.0);
                }
                "TupleProbe" => {
                    let TypeExpr::Tuple { elements, .. } = returned else {
                        panic!("expected a tuple, got {returned:?}");
                    };
                    assert_eq!(elements.len(), 1);
                    assert_number_literal(&elements[0].ty, 1.0);
                }
                _ => unreachable!(),
            }
        }
    }
}

#[test]
fn mapped_member_signatures_keep_recursive_aliases_addressable() {
    let host = make_host_with_footprint();
    let path = "/recursive-callables.ts";
    upsert_ts(
        &host,
        path,
        r#"
type Loop<T> = (x: T) => Loop<T>;
type Sink<T> = (x: Sink<T>) => T;
type Grow<T> = (x: T) => Grow<T[]>;
type Left<T> = (x: T) => Right<T>;
type Right<T> = (x: T) => Left<T>;
type Identity<T> = T;
type Field<T> = T extends { a: infer V } ? V : never;
type Finite<T extends { a: number }> = (x: Identity<T['a']>) => () => Identity<T['a']>;
type ConditionalFinite<T extends { a: number }> = (x: Identity<T['a']>) => () => Field<T>;
export type RecursiveReturn = { [K in 'a']: Loop<number> };
export type RecursiveParameter = { [K in 'a']: Sink<number> };
export type GrowingReturn = { [K in 'a']: Grow<number> };
export type MutualReturn = { [K in 'a']: Left<number> };
export type FiniteSlots = { [K in 'a']: Finite<{ a: 1 }> };
export type ConditionalFiniteSlots = { [K in 'a']: ConditionalFinite<{ a: 1 }> };
"#,
    );
    let number = TypeExpr::Primitive(PrimitiveName::Number);
    let array = TypeExpr::Array {
        element: Arc::new(number.clone()),
        readonly: false,
    };
    let nested_array = TypeExpr::Array {
        element: Arc::new(array.clone()),
        readonly: false,
    };
    let assert_callable = |expr: &TypeExpr,
                           name: &str,
                           recursive_return: bool,
                           expected_scalar: &TypeExpr,
                           expected_argument: &TypeExpr| {
        let signature = function_type(expr);
        assert_eq!(signature.parameters.len(), 1);
        let returned = signature.return_type.as_deref().expect("declared return");
        let (recursive, scalar) = if recursive_return {
            (returned, &signature.parameters[0].ty)
        } else {
            (&signature.parameters[0].ty, returned)
        };
        assert_eq!(scalar, expected_scalar);
        assert_ref(recursive, name);
        let arguments = match recursive {
            TypeExpr::Ref { type_arguments, .. }
            | TypeExpr::RecursiveRef { type_arguments, .. } => type_arguments,
            _ => unreachable!("assert_ref checked the carrier"),
        };
        assert_eq!(arguments.as_ref(), std::slice::from_ref(expected_argument));
    };
    for _ in 0..2 {
        for name in ["FiniteSlots", "ConditionalFiniteSlots"] {
            let (finite, _) = resolve_expr(&host, path, name, &[], ProjectionMode::Expanded);
            let props = object_props(&finite);
            let signature = function_type(&props["a"].ty);
            assert_number_literal(&signature.parameters[0].ty, 1.0);
            let nested = function_type(signature.return_type.as_deref().expect("nested callable"));
            assert_number_literal(nested.return_type.as_deref().expect("indexed return"), 1.0);
        }

        for (name, recursive_name, recursive_return, expected_argument, followed_argument) in [
            (
                "RecursiveReturn",
                "Loop",
                true,
                number.clone(),
                number.clone(),
            ),
            (
                "RecursiveParameter",
                "Sink",
                false,
                number.clone(),
                number.clone(),
            ),
            (
                "GrowingReturn",
                "Grow",
                true,
                array.clone(),
                nested_array.clone(),
            ),
        ] {
            let (expr, _) = resolve_expr(&host, path, name, &[], ProjectionMode::Expanded);
            let props = object_props(&expr);
            assert_eq!(prop_names(&props), vec!["a"]);
            assert_callable(
                &props["a"].ty,
                recursive_name,
                recursive_return,
                &number,
                &expected_argument,
            );
            let (followed, _) = resolve_expr(
                &host,
                path,
                recursive_name,
                &[Arc::new(expected_argument.clone())],
                ProjectionMode::Expanded,
            );
            assert_callable(
                &followed,
                recursive_name,
                recursive_return,
                &expected_argument,
                &followed_argument,
            );
        }
        let (mutual, _) = resolve_expr(&host, path, "MutualReturn", &[], ProjectionMode::Expanded);
        let props = object_props(&mutual);
        assert_callable(&props["a"].ty, "Right", true, &number, &number);
        let (followed, _) = resolve_expr(
            &host,
            path,
            "Right",
            &[Arc::new(number.clone())],
            ProjectionMode::Expanded,
        );
        let right = function_type(&followed);
        assert_eq!(right.parameters.len(), 1);
        assert_primitive(&right.parameters[0].ty, PrimitiveName::Number);
        assert_callable(
            right.return_type.as_deref().expect("left callable"),
            "Right",
            true,
            &number,
            &number,
        );
    }
}

fn assert_composed_recursive_mapped_signature(nullable: bool, indexed: bool, conditional: bool) {
    let host = make_host_with_footprint();
    let path = "/composed-recursive-callable.ts";
    let nullish = if nullable { " | null" } else { "" };
    let index = if indexed { "['a' | 'b']" } else { "" };
    let signature = format!("((x: T) => Loop<T>){nullish}");
    let body = if conditional {
        format!("T extends never ? never : {signature}")
    } else {
        signature
    };
    upsert_ts(
        &host,
        path,
        &format!(
            "type Loop<T> = {body};\n\
             export type Probe = {{ [K in 'a' | 'b']: Loop<number> }}{index};"
        ),
    );
    let assert_signature = |value: &TypeExpr| {
        let callable = if nullable {
            let TypeExpr::Union(arms) = value else {
                panic!("expected a nullable callable, got {value:?}");
            };
            assert_eq!(arms.len(), 2);
            assert!(arms.contains(&TypeExpr::Primitive(PrimitiveName::Null)));
            arms.iter()
                .find(|arm| matches!(arm, TypeExpr::Function(_)))
                .expect("the non-null arm must be callable")
        } else {
            value
        };
        let signature = function_type(callable);
        assert_eq!(signature.parameters.len(), 1);
        assert_primitive(&signature.parameters[0].ty, PrimitiveName::Number);
        let returned = signature.return_type.as_deref().expect("declared return");
        assert_ref(returned, "Loop");
        let arguments = match returned {
            TypeExpr::Ref { type_arguments, .. }
            | TypeExpr::RecursiveRef { type_arguments, .. } => type_arguments,
            _ => unreachable!("assert_ref checked the carrier"),
        };
        assert_eq!(
            arguments.as_ref(),
            &[TypeExpr::Primitive(PrimitiveName::Number)]
        );
    };
    let assert_complete = |record: &verter_audit::RequestAuditRecord| {
        let verter_audit::RequestKindPayload::TypeResolution(payload) = &record.kind_payload else {
            panic!("recursive callable publication must retain its resolution audit");
        };
        assert!(
            !payload.cache_suppress
                && !payload.recursion_limit_reached
                && payload.walker_diagnostics.is_empty(),
            "finite recursive callable publication must remain cacheable: {payload:?}"
        );
    };
    for _ in 0..2 {
        let (expr, record) = resolve_expr(&host, path, "Probe", &[], ProjectionMode::Expanded);
        assert_complete(&record);
        if indexed {
            assert_signature(&expr);
        } else {
            let props = object_props(&expr);
            assert_eq!(prop_names(&props), vec!["a", "b"]);
            for prop in props.values() {
                assert_signature(&prop.ty);
            }
        }
        let (followed, record) = resolve_expr(
            &host,
            path,
            "Loop",
            &[Arc::new(TypeExpr::Primitive(PrimitiveName::Number))],
            ProjectionMode::Expanded,
        );
        assert_complete(&record);
        assert_signature(&followed);
    }
}

#[test]
fn mapped_member_union_signatures_keep_recursive_aliases_addressable() {
    assert_composed_recursive_mapped_signature(true, false, false);
}

#[test]
fn mapped_index_signatures_keep_recursive_aliases_addressable() {
    assert_composed_recursive_mapped_signature(false, true, false);
    assert_composed_recursive_mapped_signature(true, true, false);
}

#[test]
fn mapped_signature_helpers_keep_recursive_conditional_callables_addressable() {
    assert_composed_recursive_mapped_signature(true, false, true);
    assert_composed_recursive_mapped_signature(true, true, true);
}

fn mapped_index_wire_answer(
    host: &crate::VerterHost,
    name: &str,
    arguments: &[Arc<TypeExpr>],
) -> TypeExpr {
    let (outcome, _) = host
        .resolve_named_symbol_wire_with_audit(
            "/indices.ts",
            name,
            arguments,
            Some(ProjectionMode::Expanded),
        )
        .into_parts();
    let node = outcome.expect("resolution succeeds").expect("type exists");
    let bytes = host
        .project_node_to_type_expr_json_bytes(&node)
        .unwrap_or_else(|| panic!("{name} must survive JSON projection"));
    serde_json::from_slice(&bytes).expect("valid TypeExpr JSON")
}

#[test]
fn mapped_index_unique_symbol_survives_json_projection() {
    let host = make_host_with_footprint();
    upsert_ts(&host, "/indices.ts", "declare const tag: unique symbol; export type Control = typeof tag; export type Direct = { [K in typeof tag]: K }[typeof tag]; export type Filtered = { [K in typeof tag]: K }[typeof tag & symbol]; export type Deferred<S> = { [K in keyof S]: S[K] }[typeof tag];");
    let free = Arc::new(TypeExpr::TypeParameter(verter_type_expr::TypeParam {
        name: "Free".into(),
        constraint: None,
        default: None,
        is_const: false,
    }));
    for _ in 0..2 {
        let control = mapped_index_wire_answer(&host, "Control", &[]);
        assert!(matches!(control, TypeExpr::TypeOf(_)));
        for name in ["Direct", "Filtered"] {
            assert_eq!(
                mapped_index_wire_answer(&host, name, &[]),
                control,
                "{name}"
            );
        }
        let deferred = mapped_index_wire_answer(&host, "Deferred", &[Arc::clone(&free)]);
        let TypeExpr::IndexedAccess { index, .. } = &deferred else {
            panic!("open mapping must retain the nominal index: {deferred:?}");
        };
        assert_eq!(index.as_ref(), &control);
    }
}

#[test]
fn mapped_index_broad_domains_read_dictionary_values() {
    let host = make_host_with_footprint();
    upsert_ts(&host, "/indices.ts", "interface S { [key: string]: 1 } interface Named { [key: string]: 1; named: 1 } export type Dictionary = { [K in keyof S & string]: S[K] }[keyof S & string]; export type AllKeys = { [K in keyof S]: S[K] }[keyof S]; export type NamedDictionary = { [K in keyof Named & string]: Named[K] }[keyof Named & string]; export type Strings = { [K in string]: K }[string]; export type Numbers = { [K in number]: 1 }[number]; export type Symbols = { [K in symbol]: 1 }[symbol];");
    for _ in 0..2 {
        assert_primitive(
            &mapped_index_wire_answer(&host, "Strings", &[]),
            PrimitiveName::String,
        );
        for name in [
            "Dictionary",
            "AllKeys",
            "NamedDictionary",
            "Numbers",
            "Symbols",
        ] {
            assert_eq!(
                mapped_index_wire_answer(&host, name, &[]),
                TypeExpr::Literal(verter_type_expr::LiteralValue::Number(1.0)),
                "{name}"
            );
        }
    }
}

#[test]
fn mapped_index_named_dictionary_keeps_signature_values() {
    let host = make_host_with_footprint();
    upsert_ts(&host, "/indices.ts", "interface Strings { [key: string]: 1 | 2; named: 2 } interface Numbers { [key: number]: 1; named: 2 } export type StringValues = { [K in keyof Strings]: Strings[K] }[keyof Strings]; export type NumberValues = { [K in keyof Numbers]: Numbers[K] }[keyof Numbers]; export type Constant = { [K in keyof Strings]: 3 }[keyof Strings];");
    for _ in 0..2 {
        for name in ["StringValues", "NumberValues"] {
            assert_number_literal_union(&mapped_index_wire_answer(&host, name, &[]), &[1.0, 2.0]);
        }
        assert_eq!(
            mapped_index_wire_answer(&host, "Constant", &[]),
            TypeExpr::Literal(verter_type_expr::LiteralValue::Number(3.0))
        );
    }
}

#[test]
fn mapped_index_equal_template_pattern_binds_the_value() {
    let host = make_host_with_footprint();
    upsert_ts(&host, "/indices.ts", "export type Pattern = `a${string}`; export type Constant = { [K in `a${string}`]: 1 }[`a${string}`]; export type Identity = { [K in `a${string}`]: K }[`a${string}`]; export type LiteralControl = { [K in `a${string}`]: 1 }['abc']; export type Aliased = { [K in Pattern]: 1 }[Pattern];");
    for _ in 0..2 {
        for name in ["Constant", "LiteralControl", "Aliased"] {
            assert_eq!(
                mapped_index_wire_answer(&host, name, &[]),
                TypeExpr::Literal(verter_type_expr::LiteralValue::Number(1.0)),
                "{name}"
            );
        }
        assert_eq!(
            mapped_index_wire_answer(&host, "Identity", &[]),
            mapped_index_wire_answer(&host, "Pattern", &[])
        );
    }
}

#[test]
fn mapped_index_assignable_template_pattern_binds_the_value() {
    let host = make_host_with_footprint();
    upsert_ts(&host, "/indices.ts", "export type Numbered = `a${number}`; export type Constant = { [K in `a${string}`]: 1 }[`a${number}`]; export type Identity = { [K in `a${string}`]: K }[`a${number}`]; export type Open<T extends string> = { [K in `a${string}`]: 1 }[`a${T}`];");
    let free = Arc::new(TypeExpr::TypeParameter(verter_type_expr::TypeParam {
        name: "Free".into(),
        constraint: None,
        default: None,
        is_const: false,
    }));
    for _ in 0..2 {
        assert_eq!(
            mapped_index_wire_answer(&host, "Constant", &[]),
            TypeExpr::Literal(verter_type_expr::LiteralValue::Number(1.0))
        );
        assert_eq!(
            mapped_index_wire_answer(&host, "Identity", &[]),
            mapped_index_wire_answer(&host, "Numbered", &[])
        );
        let open = mapped_index_wire_answer(&host, "Open", &[Arc::clone(&free)]);
        assert!(
            matches!(open, TypeExpr::IndexedAccess { .. }),
            "an open interpolant must keep the deferred access: {open:?}"
        );
    }
}

/// `1 | undefined`, in either union order.
fn assert_one_or_undefined(answer: &TypeExpr, label: &str) {
    let TypeExpr::Union(arms) = answer else {
        panic!("{label}: an optional source member must read `1 | undefined`, got {answer:?}");
    };
    assert_eq!(arms.len(), 2, "{label}: {answer:?}");
    assert!(
        arms.contains(&TypeExpr::Literal(verter_type_expr::LiteralValue::Number(
            1.0
        ))) && arms.contains(&TypeExpr::Primitive(PrimitiveName::Undefined)),
        "{label}: {answer:?}"
    );
}

#[test]
fn mapped_index_unique_symbol_key_keeps_source_optionality() {
    let host = make_host_with_footprint();
    upsert_ts(&host, "/indices.ts", "declare const tag: unique symbol; interface Optional { [tag]?: 1 } interface Required { [tag]: 1 } export type OptionalSymbol = { [K in keyof Optional]: 1 }[keyof Optional]; export type RequiredSymbol = { [K in keyof Required]: 1 }[keyof Required];");
    for _ in 0..2 {
        assert_one_or_undefined(
            &mapped_index_wire_answer(&host, "OptionalSymbol", &[]),
            "OptionalSymbol",
        );
        assert_eq!(
            mapped_index_wire_answer(&host, "RequiredSymbol", &[]),
            TypeExpr::Literal(verter_type_expr::LiteralValue::Number(1.0))
        );
    }
}

#[test]
fn mapped_index_optional_source_modifiers_read_in_linear_work() {
    let mut scans = Vec::new();
    for count in [32usize, 64, 128] {
        let host = make_host_with_footprint();
        let members = (0..count)
            .map(|i| format!("k{i}?: {i};"))
            .collect::<String>();
        upsert_ts(
            &host,
            "/indices.ts",
            &format!(
                "interface S {{ {members} }} export type Probe = {{ [K in keyof S]: 1 }}[keyof S];"
            ),
        );
        let before = verter_type_engine::semantic_query::key_scan_comparisons_for_tests();
        let answer = mapped_index_wire_answer(&host, "Probe", &[]);
        scans.push(verter_type_engine::semantic_query::key_scan_comparisons_for_tests() - before);
        assert_one_or_undefined(&answer, &format!("{count} optional keys"));
    }
    // Reading every key's source modifier scans the source surface once:
    // doubling the key count at most doubles the comparisons (a per-key
    // scan quadruples them).
    for pair in scans.windows(2) {
        assert!(
            pair[1] <= 2 * pair[0] + 8,
            "source modifier reads must stay linear in the key count: {scans:?}"
        );
    }
}

#[test]
fn mapped_index_empty_filtered_domains_are_never() {
    let host = make_host_with_footprint();
    upsert_ts(&host, "/indices.ts", "interface Numeric { 0: 1 } interface Empty {} export type NumericOnly = { [K in keyof Numeric & string]: Numeric[K] }[keyof Numeric & string]; export type NoKeys = { [K in keyof Empty & string]: Empty[K] }[keyof Empty & string]; export type Disjoint = { [K in string & number]: K }[string & number]; export type FilteredHop = { [K in keyof Numeric & string]: { value: 1 } }[keyof Numeric & string]['value']; export type NeverHop = { [K in never]: { value: 1 } }[never]['value'];");
    for _ in 0..2 {
        for name in [
            "NumericOnly",
            "NoKeys",
            "Disjoint",
            "FilteredHop",
            "NeverHop",
        ] {
            assert_primitive(
                &mapped_index_wire_answer(&host, name, &[]),
                PrimitiveName::Never,
            );
        }
    }
}

#[test]
fn mapped_index_deferred_access_retains_trailing_indices() {
    let host = make_host_with_footprint();
    upsert_ts(&host, "/indices.ts", "type Wrap<T> = { value: T }; export type Probe<S> = { [K in keyof S]: Wrap<S[K]> }[keyof S]['value'];");
    let free = Arc::new(TypeExpr::TypeParameter(verter_type_expr::TypeParam {
        name: "Free".into(),
        constraint: None,
        default: None,
        is_const: false,
    }));
    for _ in 0..2 {
        let answer = mapped_index_wire_answer(&host, "Probe", &[Arc::clone(&free)]);
        let TypeExpr::IndexedAccess { object, index } = &answer else {
            panic!("trailing access must remain: {answer:?}");
        };
        assert_string_literal(index, "value");
        let TypeExpr::IndexedAccess { object, index } = object.as_ref() else {
            panic!("mapped access must remain: {object:?}");
        };
        assert!(matches!(index.as_ref(), TypeExpr::KeyOf(_)));
        let TypeExpr::Mapped { value, .. } = object.as_ref() else {
            panic!("mapping must remain: {object:?}");
        };
        let TypeExpr::Ref {
            name,
            type_arguments,
        } = value.as_ref()
        else {
            panic!("alias application must remain: {value:?}");
        };
        assert_eq!(name.as_ref(), "Wrap");
        assert_eq!(type_arguments.len(), 1);
        assert!(matches!(&type_arguments[0], TypeExpr::IndexedAccess { .. }));
    }
}

#[test]
fn mapped_index_materializes_key_independent_template_unions() {
    let host = make_host_with_footprint();
    upsert_ts(&host, "/constant.ts", "type Label<T> = T extends number ? 'num' : never; export type Probe = { [K in 'a' | 'b']: 'direct' | `nested.${Label<1>}` }['a' | 'b'];");
    let (answer, _) = resolve_expr(
        &host,
        "/constant.ts",
        "Probe",
        &[],
        ProjectionMode::Expanded,
    );
    assert_literal_union(&answer, &["direct", "nested.num"]);
}

#[test]
fn generic_mapped_index_keeps_the_access_and_alias_arguments() {
    let host = make_host_with_footprint();
    upsert_ts(&host, "/generic.ts", "export type Wrap<T> = { value: T }; export type Deferred<S> = { [K in keyof S]: Wrap<S[K]> }[keyof S];");
    let parameter = Arc::new(TypeExpr::TypeParameter(verter_type_expr::TypeParam {
        name: "S".into(),
        constraint: None,
        default: None,
        is_const: false,
    }));
    let (answer, _) = resolve_expr(
        &host,
        "/generic.ts",
        "Deferred",
        &[parameter],
        ProjectionMode::Expanded,
    );
    let TypeExpr::IndexedAccess { object, index } = &answer else {
        panic!("an open mapped index must retain its access, got {answer:?}");
    };
    assert!(
        matches!(index.as_ref(), TypeExpr::KeyOf(_)),
        "the deferred index must retain keyof S: {index:?}"
    );
    let TypeExpr::Mapped { value, .. } = object.as_ref() else {
        panic!("the deferred object must retain its mapping: {object:?}");
    };
    let TypeExpr::Ref {
        name,
        type_arguments,
    } = value.as_ref()
    else {
        panic!("the mapped value must retain its alias application: {value:?}");
    };
    assert_eq!(name.as_ref(), "Wrap");
    assert_eq!(type_arguments.len(), 1);
    assert!(
        matches!(&type_arguments[0], TypeExpr::IndexedAccess { .. }),
        "the alias argument must retain S[K]: {type_arguments:?}"
    );
}

#[test]
fn recursive_mapped_index_publishes_dotted_paths_across_three_modules() {
    assert_recursive_paths_layout(20, 3);
}

#[test]
fn recursive_mapped_index_publishes_dotted_paths_across_five_modules() {
    assert_recursive_paths_layout(100, 5);
}

/// Tripling the entities triples the work: instantiations and fact-signature
/// proof work stay linear in the entity count rather than growing with the
/// pairs of entities each recursive path can reach.
#[test]
fn recursive_mapped_index_path_work_grows_linearly_with_entity_count() {
    let small = assert_recursive_paths_layout(25, 5);
    let large = assert_recursive_paths_layout(75, 5);
    assert!(
        large.instantiations <= small.instantiations * 4,
        "instantiations must grow linearly: {} at 25 entities, {} at 75",
        small.instantiations,
        large.instantiations,
    );
    assert!(
        large.proof_work <= small.proof_work * 4,
        "fact-signature proof work must grow linearly: {} at 25 entities, {} at 75",
        small.proof_work,
        large.proof_work,
    );
}

#[test]
fn recursive_mapped_index_publishes_dotted_paths_in_small_cyclic_layouts() {
    // Two entities per module still cover cyclic imports and recursive path
    // depth under the default request fuse.
    for (count, groups) in [(6, 3), (10, 5)] {
        assert_recursive_paths_layout(count, groups);
    }
}

/// Exhaustion withholds the selected value, while its authored mapped access
/// remains intact. The stopped demand is not an evaluated partial value.
#[test]
fn recursive_mapped_index_budget_refusal_does_not_warm() {
    use verter_type_engine::request_context::{RequestContext, RequestContextGuard};
    use verter_type_engine::semantic_query::{
        FactStatus, InstantiateKey, PathSegment, ProjectionReductionContext, QueryError,
        QueryResult, SemanticNodeData, SemanticQueryKey,
    };

    let host = recursive_paths_host(3, 3, 100);
    let _scope = host
        .project_type_store()
        .semantic_graph()
        .enter_node_root_scope();
    let view = crate::typeinfo::current_store_view_for_query(&host).unwrap();
    let host_context = crate::resolver_core::HostResolverContext::from_current(
        &host,
        &view,
        Arc::new(crate::resolver_core::CanonicalCompletionOverlay::new()),
    );
    let dispatch =
        verter_type_engine::project_semantic_dispatch::ProjectSemanticDispatch::new(&host_context);
    let graph = host.project_type_store().semantic_graph();
    // Like the public named-symbol entry, first obtain the declaration shell;
    // the selected index demand below owns reduction of its recursive value.
    let shell = dispatch.execute_read(SemanticQueryKey::Instantiate(InstantiateKey::new(
        dispatch.type_slot_for(
            verter_type_expr::OwnedStr::from("/paths/scenario.ts"),
            verter_type_expr::TopLevelOwnerId::ordinary_file(),
            verter_type_expr::OwnedStr::from("Probe"),
        ),
        Arc::from([]),
        dispatch.instantiate_context_for(
            "/paths/scenario.ts",
            ProjectionReductionContext::published(ProjectionMode::Expanded),
        ),
    )));
    let QueryResult::Value(node) = shell.value().clone() else {
        panic!(
            "the authored access must resolve: {:?}",
            shell.value().clone()
        );
    };
    let data = graph.node_data(node).unwrap();
    let SemanticNodeData::IndexedAccess { object, index } = data.as_ref() else {
        panic!("the mapped index must remain: {data:?}");
    };
    let key = SemanticQueryKey::IndexedAccess {
        base: *object,
        index: index.clone(),
        mode: ProjectionMode::Expanded,
    };
    // IndexedAccess is admitted under this canonical ProjectPath identity.
    let memo_key = SemanticQueryKey::ProjectPath {
        base: *object,
        path: verter_type_expr::OwnedSlice::from([PathSegment::Index(index.clone())]),
        context: ProjectionReductionContext::published(ProjectionMode::Expanded),
    };
    for request in 1..=2 {
        let context = RequestContext::with_kind_timing_and_projection_budget(
            request,
            Arc::from("/paths/scenario.ts"),
            verter_audit::RequestKind::TypeResolution,
            false,
            false,
            None,
            100,
        );
        let _guard = RequestContextGuard::install(Arc::clone(&context));
        let read = dispatch.execute_read(key.clone());
        assert!(context.projection_budget.is_exhausted());
        assert!(read.result_is_partial());
        assert!(read.cache_suppress());
        assert!(matches!(read.fact_status(), FactStatus::Unavailable(_)));
        assert!(
            matches!(
                read.value().clone(),
                QueryResult::Error(QueryError::IncompleteSemanticOperand { .. })
            ),
            "a stopped walk cannot publish an evaluated value: {:?}",
            read.value().clone()
        );
        assert!(!graph.contains_key(&memo_key));
    }
    let bytes = host
        .project_ordinal_to_type_expr_json_bytes_for_test(node)
        .unwrap();
    let answer: TypeExpr = serde_json::from_slice(&bytes).unwrap();
    let TypeExpr::IndexedAccess { object, index } = &answer else {
        panic!("the authored index must survive refusal: {answer:?}");
    };
    let TypeExpr::Mapped { source, value, .. } = object.as_ref() else {
        panic!("the authored mapping must survive refusal: {object:?}");
    };
    assert_eq!(index, source, "the mapped key domain must remain indexed");
    let TypeExpr::Union(arms) = value.as_ref() else {
        panic!("the mapped value must retain its recursive branch: {value:?}");
    };
    let expressions = arms
        .iter()
        .find_map(|arm| match arm {
            TypeExpr::TemplateLiteral { expressions, .. } => Some(expressions),
            _ => None,
        })
        .expect("the dotted-path template must remain");
    let TypeExpr::Ref {
        name,
        type_arguments,
    } = &expressions[1]
    else {
        panic!("the recursive alias must remain: {expressions:?}");
    };
    assert_eq!(name.as_ref(), "Paths");
    assert_eq!(type_arguments.len(), 2);
    assert!(matches!(&type_arguments[0], TypeExpr::IndexedAccess { .. }));
    assert!(matches!(&type_arguments[1], TypeExpr::Tuple { .. }));
    // The public boundary must retain the typed refusal on a repeat request.
    for _ in 0..2 {
        let (outcome, _) = host
            .resolve_named_symbol_wire_with_audit(
                "/paths/scenario.ts",
                "Probe",
                &[],
                Some(ProjectionMode::Expanded),
            )
            .into_parts();
        assert!(
            matches!(
                outcome,
                Err(crate::host_resolve_type_audit::TypeResolutionRequestError::Other(_))
            ),
            "the refused Paths<Api> cannot become a published answer: {outcome:?}"
        );
    }
    // The same demand with enough allowance must recompute and admit its
    // complete result. This also proves the non-admission probe's identity.
    let context = RequestContext::with_kind_timing_and_projection_budget(
        3,
        Arc::from("/paths/scenario.ts"),
        verter_audit::RequestKind::TypeResolution,
        false,
        false,
        None,
        50_000,
    );
    let _guard = RequestContextGuard::install(context);
    let read = dispatch.execute_read(key);
    assert!(!read.result_is_partial() && !read.cache_suppress());
    let QueryResult::Value(value) = read.value().clone() else {
        panic!(
            "the roomy request must complete: {:?}",
            read.value().clone()
        );
    };
    let answer = host.project_node_to_type_expr_for_test(value).unwrap();
    let TypeExpr::Union(paths) = &answer else {
        panic!("the roomy request must publish paths: {answer:?}");
    };
    assert_eq!(paths.len(), 3 * 18);
    assert!(graph.contains_key(&memo_key));
}

/// What one cold evaluation of the mapped index `Paths<Api>` selects over
/// `count` entities admits into the family memo, with the paths it publishes.
fn recursive_paths_admissions(count: usize, groups: usize) -> (u64, usize) {
    use verter_type_engine::request_context::{RequestContext, RequestContextGuard};
    use verter_type_engine::semantic_query::{
        InstantiateKey, ProjectionReductionContext, QueryResult, SemanticNodeData, SemanticQueryKey,
    };

    let host = recursive_paths_host(count, groups, 50_000);
    let view = crate::typeinfo::current_store_view_for_query(&host).unwrap();
    let host_context = crate::resolver_core::HostResolverContext::from_current(
        &host,
        &view,
        Arc::new(crate::resolver_core::CanonicalCompletionOverlay::new()),
    );
    let dispatch =
        verter_type_engine::project_semantic_dispatch::ProjectSemanticDispatch::new(&host_context);
    let graph = host.project_type_store().semantic_graph();
    let shell = dispatch.execute_read(SemanticQueryKey::Instantiate(InstantiateKey::new(
        dispatch.type_slot_for(
            verter_type_expr::OwnedStr::from("/paths/scenario.ts"),
            verter_type_expr::TopLevelOwnerId::ordinary_file(),
            verter_type_expr::OwnedStr::from("Probe"),
        ),
        Arc::from([]),
        dispatch.instantiate_context_for(
            "/paths/scenario.ts",
            ProjectionReductionContext::published(ProjectionMode::Expanded),
        ),
    )));
    let QueryResult::Value(node) = shell.value().clone() else {
        panic!(
            "the authored access must resolve: {:?}",
            shell.value().clone()
        );
    };
    let data = graph.node_data(node).unwrap();
    let SemanticNodeData::IndexedAccess { object, index } = data.as_ref() else {
        panic!("the mapped index must remain: {data:?}");
    };
    let context = RequestContext::with_kind_timing_and_projection_budget(
        1,
        Arc::from("/paths/scenario.ts"),
        verter_audit::RequestKind::TypeResolution,
        false,
        false,
        None,
        50_000,
    );
    let _guard = RequestContextGuard::install(Arc::clone(&context));
    let read = dispatch.execute_read(SemanticQueryKey::IndexedAccess {
        base: *object,
        index: index.clone(),
        mode: ProjectionMode::Expanded,
    });
    assert!(!read.result_is_partial() && !read.cache_suppress());
    let QueryResult::Value(value) = read.value().clone() else {
        panic!("the evaluation must complete: {:?}", read.value().clone());
    };
    let answer = host.project_node_to_type_expr_for_test(value).unwrap();
    let TypeExpr::Union(paths) = &answer else {
        panic!("{count} entities must publish a path union, got {answer:?}");
    };
    (
        context
            .memo_insertions
            .load(std::sync::atomic::Ordering::Relaxed),
        paths.len(),
    )
}

/// A recursive evaluation admits what a later demand can reach — each
/// instantiation and the projections that answer it — while the conditional
/// decisions, template-literal reductions and index projections nested in
/// its evaluation complete for the request alone, so its family-memo
/// admissions stay a small, flat multiple of its entities.
#[test]
fn recursive_paths_admit_their_instantiations_not_their_nested_steps() {
    let sizes = [(20, 3), (40, 5)];
    let measured = sizes.map(|(count, groups)| {
        let (admissions, paths) = recursive_paths_admissions(count, groups);
        assert_eq!(
            paths,
            count * 18,
            "{count} entities publish every dotted path"
        );
        eprintln!("RECURSIVE_PATHS entities={count} admissions={admissions}");
        admissions / count as u64
    });
    // Admitted memo slots: an instantiation fills up to four mode slots. A
    // slot per nested conditional decision, template-literal reduction and
    // index projection as well — about sixty more per entity — lifts every
    // layout far above this line.
    const PER_ENTITY_CEILING: u64 = 100;
    for (per_entity, (count, _)) in measured.iter().zip(sizes) {
        assert!(
            *per_entity <= PER_ENTITY_CEILING,
            "{count} entities admitted {per_entity} entries per entity (ceiling {PER_ENTITY_CEILING}): {measured:?}"
        );
    }
}

/// A host holding `Paths<Api>` over `count` entities spread across `groups`
/// mutually importing modules, under the request allowance `budget`
/// (`0` keeps the shipped default fuse).
fn recursive_paths_host(count: usize, groups: usize, budget: usize) -> Arc<crate::VerterHost> {
    let mut files = vec![(
        "/paths/utils.ts".to_string(),
        "export interface Meta<T> { tag: T; created: number }\n\
         export type Paths<T, D extends unknown[] = []> = D['length'] extends 3 ? never\n\
         : T extends object ? { [K in keyof T & string]: K | `${K}.${Paths<T[K], [...D, 0]>}` }[keyof T & string] : never;".to_string(),
    )];
    for group in 0..groups {
        let mut source = "import type { Meta } from './utils';\n".to_string();
        for other in 0..groups {
            if other != group {
                let imports = (other..count)
                    .step_by(groups)
                    .map(|i| format!("E{i}"))
                    .collect::<Vec<_>>()
                    .join(", ");
                source.push_str(&format!(
                    "import type {{ {imports} }} from './entities-{other}';\n"
                ));
            }
        }
        for i in (group..count).step_by(groups) {
            source.push_str(&format!("export interface E{i} {{ id: {i}; name: string; next: E{}; owner: E{}; meta: Meta<{i}> }}\n", (i + 1) % count, (i * 7 + 3) % count));
        }
        files.push((format!("/paths/entities-{group}.ts"), source));
    }
    let mut api = String::new();
    for group in 0..groups {
        let imports = (group..count)
            .step_by(groups)
            .map(|i| format!("E{i}"))
            .collect::<Vec<_>>()
            .join(", ");
        api.push_str(&format!(
            "import type {{ {imports} }} from './entities-{group}';\n"
        ));
    }
    api.push_str("export interface Api {\n");
    for i in 0..count {
        api.push_str(&format!("e{i}: E{i};\n"));
    }
    api.push_str("}\n");
    files.push(("/paths/api.ts".to_string(), api));
    files.push(("/paths/scenario.ts".to_string(), "import type { Api } from './api'; import type { Paths } from './utils'; export type Probe = Paths<Api>;".to_string()));
    let borrowed = files
        .iter()
        .map(|(path, source)| (path.as_str(), source.as_str()))
        .collect::<Vec<_>>();
    let host = make_host_with_workspace_files_config(
        &borrowed,
        crate::HostConfig {
            audit_enabled: true,
            projection_op_budget: budget,
            ..crate::HostConfig::default()
        },
    );
    for (path, source) in &borrowed {
        upsert_ts(&host, path, source);
    }
    host
}

/// `Paths<Api>` over `count` entities spread across `groups` mutually
/// importing modules publishes every dotted path to depth 3, as tsc does,
/// with instantiations linear in the entity count.
fn assert_recursive_paths_layout(count: usize, groups: usize) -> RecursivePathsWork {
    // Complete enumeration of the larger closures needs an explicit
    // request allowance; the smallest layout exercises the default fuse.
    let host = recursive_paths_host(count, groups, if count <= 20 { 0 } else { 50_000 });
    let proof_before = verter_session_query::facts::fact_read_set::signature_work_for_tests();
    let (answer, record) = resolve_expr(
        &host,
        "/paths/scenario.ts",
        "Probe",
        &[],
        ProjectionMode::Expanded,
    );
    let stats = host.project_type_store().semantic_graph().stats_snapshot();
    let proof_work =
        verter_session_query::facts::fact_read_set::signature_work_for_tests() - proof_before;
    let verter_audit::RequestKindPayload::TypeResolution(payload) = &record.kind_payload else {
        panic!("recursive paths must retain their resolution audit");
    };
    println!(
        "entities={count} instantiate_count={} projection_ops={} paths_proof_work={proof_work}",
        stats.instantiate_count, payload.projection_ops_executed,
    );
    assert!(
        stats.instantiate_count <= u64::try_from(count * 15 + 20).unwrap(),
        "recursive applications must share already demanded property types: {count} entities used {} instantiations",
        stats.instantiate_count,
    );
    let mut expected = BTreeSet::new();
    for i in 0..count {
        expected.insert(format!("e{i}"));
        for field in ["id", "name", "next", "owner", "meta"] {
            expected.insert(format!("e{i}.{field}"));
        }
        for field in ["next", "owner"] {
            for child in ["id", "name", "next", "owner", "meta"] {
                expected.insert(format!("e{i}.{field}.{child}"));
            }
        }
        for child in ["tag", "created"] {
            expected.insert(format!("e{i}.meta.{child}"));
        }
    }
    let TypeExpr::Union(arms) = &answer else {
        panic!("{count} entities/{groups} modules must publish a path union, got {answer:?}; audit: {:?}", record.kind_payload)
    };
    let actual = arms
        .iter()
        .map(|arm| {
            let TypeExpr::Literal(verter_type_expr::LiteralValue::String(path)) = arm else {
                panic!("path must be a string literal, got {arm:?}")
            };
            path.clone()
        })
        .collect::<BTreeSet<_>>();
    assert_eq!(actual, expected, "{count} entities/{groups} modules");
    RecursivePathsWork {
        instantiations: stats.instantiate_count,
        proof_work,
    }
}

/// The work one `Paths<Api>` resolution spent, read from the engine's counters.
struct RecursivePathsWork {
    instantiations: u64,
    proof_work: usize,
}

#[test]
fn mapped_index_selected_key_does_not_materialize_sibling_values() {
    let host = make_host_with_footprint();
    upsert_ts(&host, "/selected.ts", "type Input = { wanted: 1; a: 2; b: 3; c: 4 }; type Select<S> = { [K in keyof S]: K extends 'wanted' ? 'yes' : 'no' }['wanted']; export type Probe = Select<Input>;");
    let graph = host.project_type_store().semantic_graph();
    let before = graph.stats_snapshot().mapped_per_k_materializations;
    let (answer, _) = resolve_expr(
        &host,
        "/selected.ts",
        "Probe",
        &[],
        ProjectionMode::Expanded,
    );
    assert_string_literal(&answer, "yes");
    assert_eq!(
        graph.stats_snapshot().mapped_per_k_materializations - before,
        1,
        "a selected mapped key must not evaluate sibling values"
    );
}

#[test]
fn unused_mapped_argument_does_not_materialize_values() {
    let host = make_host_with_footprint();
    upsert_ts(
        &host,
        "/unused.ts",
        "type Ignore<T> = 'yes'; export type Probe = Ignore<{ [K in 'a' | 'b']: K }>;",
    );
    let graph = host.project_type_store().semantic_graph();
    let before = graph.stats_snapshot().mapped_per_k_materializations;
    let (answer, _) = resolve_expr(&host, "/unused.ts", "Probe", &[], ProjectionMode::Expanded);
    assert_string_literal(&answer, "yes");
    assert_eq!(
        graph.stats_snapshot().mapped_per_k_materializations - before,
        0,
        "an unused type argument must not evaluate mapped values"
    );
}

#[test]
fn finite_mapped_index_shares_projection_bookkeeping() {
    let entries = [4, 64].map(|count| {
        let host = make_host_with_footprint();
        let names: Vec<_> = (0..count).map(|i| format!("k{i}")).collect();
        let keys = names
            .iter()
            .map(|name| format!("'{name}'"))
            .collect::<Vec<_>>()
            .join(" | ");
        upsert_ts(
            &host,
            "/batch.ts",
            &format!("export type Probe = {{ [K in {keys}]: K }}[{keys}];"),
        );
        let (answer, _) = resolve_expr(&host, "/batch.ts", "Probe", &[], ProjectionMode::Expanded);
        let expected: Vec<_> = names.iter().map(String::as_str).collect();
        assert_literal_union(&answer, &expected);
        host.project_type_store()
            .semantic_graph()
            .memo_entry_count()
    });
    assert!(
        entries[1] <= entries[0] + 4,
        "key selection must share query bookkeeping: {entries:?}"
    );
}

#[test]
fn mapped_type_without_key_remap_materializes_required_surface() {
    let host = make_host_with_footprint();
    upsert_ts(&host, "/fixtures/mapped-template.ts", MAPPED_TEMPLATE);

    let (expr, record) = resolve_expr(
        &host,
        "/fixtures/mapped-template.ts",
        "PlainSlotMap",
        &[],
        ProjectionMode::Expanded,
    );

    let props = object_props(&expr);
    assert_eq!(prop_names(&props), vec!["empty", "item", "root"]);
    assert!(!props["root"].optional);
    let root = object_props(&props["root"].ty);
    assert_primitive(&root["id"].ty, PrimitiveName::String);
    assert_query_mode(&record, ProjectionModeTag::Expanded);
}

#[test]
#[ignore = "typeinfo does not yet materialize mapped types whose keys are remapped with a template-literal name; keep as the future key-remapping contract"]
fn mapped_type_with_template_literal_key_remap_resolves_remapped_slot() {
    let host = make_host_with_footprint();
    upsert_ts(&host, "/fixtures/mapped-template.ts", MAPPED_TEMPLATE);

    let (expr, record) = resolve_expr(
        &host,
        "/fixtures/mapped-template.ts",
        "RootRemappedSlot",
        &[],
        ProjectionMode::Expanded,
    );

    let slot = function_type(&expr);
    assert_eq!(slot.parameters.len(), 1);
    let payload = object_props(&slot.parameters[0].ty);
    assert_primitive(&payload["id"].ty, PrimitiveName::String);
    assert_query_mode(&record, ProjectionModeTag::Expanded);
}

#[test]
#[ignore = "typeinfo does not yet substitute remapped template-literal mapped keys for non-root members; keep as the future remapped member projection contract"]
fn mapped_type_with_template_literal_key_remap_resolves_item_slot() {
    let host = make_host_with_footprint();
    upsert_ts(&host, "/fixtures/mapped-template.ts", MAPPED_TEMPLATE);

    let (expr, record) = resolve_expr(
        &host,
        "/fixtures/mapped-template.ts",
        "ItemRemappedSlot",
        &[],
        ProjectionMode::Expanded,
    );

    let slot = function_type(&expr);
    assert_eq!(slot.parameters.len(), 1);
    let payload = object_props(&slot.parameters[0].ty);
    assert_primitive(&payload["value"].ty, PrimitiveName::Number);
    assert_query_mode(&record, ProjectionModeTag::Expanded);
}

#[test]
#[ignore = "reducer resolves this correctly (covered by the non-ignored `template_literal_key_reducer_projects_callable_slots` regression); NOT oracle-liftable — the declared source body `StaticTemplateSlots[TemplateLiteralCellName]` is a non-carve-out indexed-access (the index is a template-literal alias `Ref`, not a same-file string-literal chain) the oracle source-side positive allowlist rejects (measured Reject(DeferredConstruct(indexed-access))); the reduced function value is itself admissible. Lift pending an oracle source-walk carve-out for template-literal-keyed indexed access"]
fn template_literal_key_alias_projects_static_template_slot() {
    let host = make_host_with_footprint();
    upsert_ts(&host, "/fixtures/mapped-template.ts", MAPPED_TEMPLATE);

    let (expr, record) = resolve_expr(
        &host,
        "/fixtures/mapped-template.ts",
        "NameCellRenderer",
        &[],
        ProjectionMode::Expanded,
    );

    let renderer = function_type(&expr);
    assert_eq!(renderer.parameters.len(), 1);
    let payload = object_props(&renderer.parameters[0].ty);
    assert_primitive(&payload["value"].ty, PrimitiveName::String);
    assert_string_literal(&payload["column"].ty, "name");
    assert_query_mode(&record, ProjectionModeTag::Expanded);
}

// LIFTED: `RecordTemplateRootSlot = RecordTemplateSlots["slot:root"]` indexes a
// `Record<`slot:${"root"|"item"}`, …>` by the same-file string-literal key
// `slot:root` (the string-literal index-chain source-walk carve-out) and reduces
// to `(payload: { name: "item" | "root" }) => VNode[]`. The lifted body is the
// registry-keyed `oracle::run_row` shared-driver call comparing Verter's
// `Expanded` projection against the checked-in tsgo snapshot.
#[oracle_row]
#[test]
fn record_with_template_literal_key_union_projects_root_slot() {}

#[test]
#[ignore = "reducer resolves this correctly (covered by the non-ignored `template_literal_key_reducer_projects_callable_slots` regression); NOT oracle-liftable — the declared source body indexes `StaticTemplateSlots` by a template-literal union key, a non-carve-out indexed-access the oracle source-side positive allowlist rejects (measured Reject(DeferredConstruct(indexed-access))); the reduced union of function values is itself admissible. Lift pending an oracle source-walk carve-out for template-literal-keyed indexed access"]
fn template_literal_union_key_projects_static_slot_union() {
    let host = make_host_with_footprint();
    upsert_ts(&host, "/fixtures/mapped-template.ts", MAPPED_TEMPLATE);

    let (expr, record) = resolve_expr(
        &host,
        "/fixtures/mapped-template.ts",
        "StaticTemplateSlotUnion",
        &[],
        ProjectionMode::Expanded,
    );

    let TypeExpr::Union(arms) = &expr else {
        panic!("expected union of cell renderers, got {expr:?}");
    };
    assert_eq!(arms.len(), 2);
    assert_query_mode(&record, ProjectionModeTag::Expanded);
}

/// Non-ignored reducer regression for the three callable-result template-literal
/// key rows (`template_literal_key_alias_projects_static_template_slot`,
/// `record_with_template_literal_key_union_projects_root_slot`,
/// `template_literal_union_key_projects_static_slot_union`). All three reduce to
/// admissible function values — none is blocked on a function surface. The
/// `record_*_root_slot` row is now oracle-LIFTED (`#[oracle_row]`): it admits both
/// sides and its `Expanded` projection converges against the checked-in tsgo
/// snapshot. The two `StaticTemplateSlots[…]` alias/union rows stay `#[ignore]`d,
/// blocked by their non-carve-out indexed-access source bodies. The SHARED
/// resolver reduces each one correctly — this test runs the same assertions in the
/// normal suite so the U2.MAPPED_TEMPLATE reducer paths (template-literal alias
/// key, Record template-union keyspace enumeration, union-index distribution) stay
/// guarded.
#[test]
fn template_literal_key_reducer_projects_callable_slots() {
    let host = make_host_with_footprint();
    upsert_ts(&host, "/fixtures/mapped-template.ts", MAPPED_TEMPLATE);

    // (a) template-literal alias key → single static slot.
    let (alias, _) = resolve_expr(
        &host,
        "/fixtures/mapped-template.ts",
        "NameCellRenderer",
        &[],
        ProjectionMode::Expanded,
    );
    let renderer = function_type(&alias);
    let payload = object_props(&renderer.parameters[0].ty);
    assert_primitive(&payload["value"].ty, PrimitiveName::String);
    assert_string_literal(&payload["column"].ty, "name");

    // (b) Record over a template-literal-union keyspace → indexed value slot.
    let (record_slot, _) = resolve_expr(
        &host,
        "/fixtures/mapped-template.ts",
        "RecordTemplateRootSlot",
        &[],
        ProjectionMode::Expanded,
    );
    let slot = function_type(&record_slot);
    let slot_payload = object_props(&slot.parameters[0].ty);
    assert_literal_union(&slot_payload["name"].ty, &["item", "root"]);

    // (c) union template-literal index → union-distributed slot union.
    let (union_slots, _) = resolve_expr(
        &host,
        "/fixtures/mapped-template.ts",
        "StaticTemplateSlotUnion",
        &[],
        ProjectionMode::Expanded,
    );
    let TypeExpr::Union(arms) = &union_slots else {
        panic!("expected union-distributed cell renderers, got {union_slots:?}");
    };
    assert_eq!(arms.len(), 2, "union index must distribute into 2 slots");
    // Discriminate the reduction: BOTH arms must be FUNCTION renderers whose
    // payload object is the expected `{ value; column }` shape. A broken
    // reducer that returned any two-arm union would pass the bare `len == 2`
    // check above, so match each arm by its `column` literal (order-independent)
    // and assert the paired `value` primitive.
    let mut saw_name_arm = false;
    let mut saw_count_arm = false;
    for arm in arms.iter() {
        let renderer = function_type(arm);
        assert_eq!(
            renderer.parameters.len(),
            1,
            "each cell renderer takes one payload param, got {arm:?}"
        );
        let payload = object_props(&renderer.parameters[0].ty);
        assert_eq!(
            prop_names(&payload),
            vec!["column", "value"],
            "payload must carry exactly value + column"
        );
        match string_literal_value(&payload["column"].ty) {
            "name" => {
                assert_primitive(&payload["value"].ty, PrimitiveName::String);
                saw_name_arm = true;
            }
            "count" => {
                assert_primitive(&payload["value"].ty, PrimitiveName::Number);
                saw_count_arm = true;
            }
            other => panic!("unexpected column literal {other:?} in {arm:?}"),
        }
    }
    assert!(
        saw_name_arm && saw_count_arm,
        "must distribute into the `cell:name`(value:string) AND `cell:count`(value:number) renderers"
    );
}

/// A template whose concatenations the connected-work ledger refuses must
/// CARRIER-STOP to the deferred `TemplateLiteral` shell, and the result must
/// NOT be warm-admitted (it is a non-cacheable budget-tainted partial). The
/// checker has no such limit below its own TS2590 product: the same product
/// under the production ledger enumerates.
#[test]
pub(crate) fn keyspace_budget_exceeded_admits_nothing() {
    use verter_type_engine::semantic_query::{QueryResult, SemanticNodeData};

    let host = make_host_with_footprint();
    let _scope = host
        .project_type_store()
        .semantic_graph()
        .enter_node_root_scope();

    // Two fully-CLOSED finite unions whose 1,600 concatenations exceed a
    // 256-unit work limit. Each union is a wide set of distinct string
    // literals.
    let union_a: Vec<String> = (0..40).map(|i| format!("a{i}")).collect();
    let union_b: Vec<String> = (0..40).map(|i| format!("b{i}")).collect();
    let (read, graph) = template_literal_reduce_read_with_work_limit(
        &host,
        &["", "-", ""],
        &[union_a.clone(), union_b.clone()],
        Some(256),
    );

    // (1) Carrier-stops: the value is the deferred TemplateLiteral SHELL, never
    //     an enumerated 1600-arm literal union.
    let value = match read.value().clone() {
        QueryResult::Value(id) => id,
        other => panic!("expected a value node, got {other:?}"),
    };
    let data = graph.node_data(value).expect("value node must exist");
    assert!(
        matches!(data.as_ref(), SemanticNodeData::TemplateLiteral { .. }),
        "a refused construction must carrier-stop to the TemplateLiteral shell, got {:?}",
        data.as_ref()
    );
    assert!(
        !matches!(data.as_ref(), SemanticNodeData::Union(_)),
        "a refused construction must NOT enumerate a literal union"
    );

    // (2) Not warm-admitted: the refused product is a non-cacheable,
    //     budget-tainted partial.
    assert!(
        read.cache_suppress(),
        "a refused template product must be cache-suppressed (never warm-admitted)"
    );
    assert!(
        read.result_is_partial(),
        "a refused template product must be flagged a budget-tainted partial"
    );

    // Control: the same 1,600-concatenation product under the production
    // ledger fully enumerates and is admissible.
    let (small_read, small_graph) =
        template_literal_reduce_read(&host, &["", "-", ""], &[union_a, union_b]);
    let small_value = match small_read.value().clone() {
        QueryResult::Value(id) => id,
        other => panic!("expected a value node, got {other:?}"),
    };
    let small_data = small_graph.node_data(small_value).expect("node must exist");
    assert!(
        matches!(small_data.as_ref(), SemanticNodeData::Union(_)),
        "the product must fully enumerate to a union, got {:?}",
        small_data.as_ref()
    );
    assert!(
        !small_read.cache_suppress(),
        "the enumerated product is a complete result and must not be suppressed"
    );
    assert!(
        !small_read.result_is_partial(),
        "the enumerated product is complete, not a partial"
    );
}

/// FIX 2 guard — the shared template reducer models TS numeric / boolean /
/// bigint literal interpolation. TS lexes `` `${1 | 2}` `` ⇒ `"1" | "2"`,
/// `` `${true}` `` ⇒ `"true"`, and a bigint literal as its base-10 digits.
/// Discriminates the pre-fix fail-closed code (which admitted ONLY string
/// literals and carrier-stopped these to the deferred shell).
#[test]
pub(crate) fn template_literal_reduce_models_ts_numeric_bigint_lexing() {
    let host = make_host_with_footprint();
    upsert_ts(
        &host,
        "/fixtures/template-lexing.ts",
        "export type NumTpl = `${1 | 2}`;\n\
         export type BoolTpl = `${true}`;\n\
         export type BigTpl = `id-${1n}`;\n",
    );

    // `${1 | 2}` ⇒ "1" | "2" (numeric literal union distributes).
    let (num, _) = resolve_expr(
        &host,
        "/fixtures/template-lexing.ts",
        "NumTpl",
        &[],
        ProjectionMode::Expanded,
    );
    assert_literal_union(&num, &["1", "2"]);

    // `${true}` ⇒ "true" (boolean literal stringifies, single literal folds).
    let (boolean, _) = resolve_expr(
        &host,
        "/fixtures/template-lexing.ts",
        "BoolTpl",
        &[],
        ProjectionMode::Expanded,
    );
    assert_string_literal(&boolean, "true");

    // `id-${1n}` ⇒ "id-1" (bigint literal → base-10 digits, no `n`).
    let (big, _) = resolve_expr(
        &host,
        "/fixtures/template-lexing.ts",
        "BigTpl",
        &[],
        ProjectionMode::Expanded,
    );
    assert_string_literal(&big, "id-1");
}

/// A bigint literal interpolates as TS's `pseudoBigIntToString`: a negated
/// zero is the literal `0n` (`${-0n}` ⇒ `"0"`), a non-zero negative keeps its
/// sign, and a hex literal reads as its base-10 value. `-0n` and `0n` are one
/// literal type, so each is assignable to the other, and a `const` initialised
/// with `-0n` has the literal type `0n`.
#[test]
pub(crate) fn template_literal_bigint_negative_zero_stringifies_unsigned() {
    let host = make_host_with_footprint();
    upsert_ts(
        &host,
        "/fixtures/template-bigint-sign.ts",
        "type __BenchInit = 0;\n\
         type Exclude<T, U> = T extends U ? never : T;\n\
         type Extract<T, U> = T extends U ? T : never;\n\
         export type __Probe = `${0x10n}|${-0n}|${10n}`;\n\
         export type Negative = `${-5n}`;\n\
         export type ZeroIdentity = -0n extends 0n ? (0n extends -0n ? true : false) : false;\n\
         const negativeZero = -0n;\n\
         export type ValueZero = `${typeof negativeZero}`;\n",
    );

    let resolve = |name: &str| {
        resolve_expr(
            &host,
            "/fixtures/template-bigint-sign.ts",
            name,
            &[],
            ProjectionMode::Expanded,
        )
        .0
    };
    assert_string_literal(&resolve("__Probe"), "16|0|10");
    assert_string_literal(&resolve("Negative"), "-5");
    assert_string_literal(&resolve("ValueZero"), "0");
    assert!(
        matches!(
            resolve("ZeroIdentity"),
            TypeExpr::Literal(verter_type_expr::LiteralValue::Boolean(true))
        ),
        "`-0n` and `0n` must be the same literal type"
    );
}
