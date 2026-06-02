//! Semantic-IR runtime-constructor reducer for Vue prop types.
//!
//! Maps a semantic [`TypeExpr`] to the set of Vue runtime prop constructor
//! kinds ([`RuntimeCtorKind`]) — the typed-IR analogue of the parser's
//! OXC-AST walker `infer_runtime_type`
//! (`verter_parser::utils::oxc::vue::script::resolve_type::infer`). It drives
//! the runtime `{ type: ... }` value of a `defineProps<T>()`-derived prop
//! declaration on the VDOM/runtime codegen path.
//!
//! This is a pure, non-resolving reduction over the already-lowered typed IR:
//! it walks the [`TypeExpr`] structure and never re-parses source, slices type
//! text, or resolves a `Ref` through the type registry. A `Ref` whose name is
//! not a recognised built-in / utility-type is `Unknown` here exactly as the
//! parser's `infer_type_reference` returns `Unknown` for an unresolved
//! reference — the runtime constructor surface is what TypeScript's *syntactic*
//! shape implies, mirroring Vue's own `resolveType.ts` emission rule.

use verter_compiler::compile::RuntimeCtorKind;
use verter_type_expr::{LiteralValue, PrimitiveName, TypeExpr};

/// Reduce a semantic [`TypeExpr`] to the Vue runtime prop constructor kinds it
/// implies, mirroring the parser's `infer_runtime_type` exactly (which TS type
/// maps to which constructor(s), union dedup + order, and the `Unknown`
/// fallback).
///
/// Returns the constructor kinds in declaration order with duplicates removed
/// for unions (so `string | number` yields `[String, Number]` and
/// `string | string` yields `[String]`). The returned vector is never empty:
/// an un-reducible type yields `[RuntimeCtorKind::Unknown]`, which downstream
/// renders as `null` (matching `format_runtime_types`).
pub fn runtime_constructors_from_type_expr(ty: &TypeExpr) -> Vec<RuntimeCtorKind> {
    match ty {
        // -- Primitives (mirrors infer.rs:38-49) --
        TypeExpr::Primitive(name) => match name {
            PrimitiveName::String => vec![RuntimeCtorKind::String],
            PrimitiveName::Number => vec![RuntimeCtorKind::Number],
            PrimitiveName::Boolean => vec![RuntimeCtorKind::Boolean],
            PrimitiveName::Object => vec![RuntimeCtorKind::Object],
            PrimitiveName::Symbol => vec![RuntimeCtorKind::Symbol],
            PrimitiveName::Null => vec![RuntimeCtorKind::Null],
            // `bigint` => Number (infer.rs:49), same as a bigint literal.
            PrimitiveName::BigInt => vec![RuntimeCtorKind::Number],
            // undefined / void / any / unknown / never => Unknown
            // (infer.rs:44-48).
            PrimitiveName::Undefined
            | PrimitiveName::Void
            | PrimitiveName::Any
            | PrimitiveName::Unknown
            | PrimitiveName::Never => vec![RuntimeCtorKind::Unknown],
        },

        // -- Literals (mirrors infer.rs:159-176) --
        TypeExpr::Literal(lit) => match lit {
            LiteralValue::String(_) => vec![RuntimeCtorKind::String],
            // numeric + bigint literals => Number (infer.rs:162, :164).
            LiteralValue::Number(_) | LiteralValue::BigInt(_) => vec![RuntimeCtorKind::Number],
            LiteralValue::Boolean(_) => vec![RuntimeCtorKind::Boolean],
        },

        // -- Object / interface / type-literal => Object (infer.rs:55) --
        TypeExpr::Object(_) => vec![RuntimeCtorKind::Object],

        // -- Array / tuple => Array (infer.rs:58) --
        TypeExpr::Array { .. } | TypeExpr::Tuple { .. } => vec![RuntimeCtorKind::Array],

        // -- Function => Function (infer.rs:61) --
        TypeExpr::Function(_) => vec![RuntimeCtorKind::Function],

        // -- Parenthesized: transparent (infer.rs:64) --
        TypeExpr::Parenthesized(inner) => runtime_constructors_from_type_expr(inner),

        // -- `readonly T` / standalone rest: the non-`keyof` type operator
        //    recurses into the inner type (infer.rs:140). --
        TypeExpr::Rest(inner) => runtime_constructors_from_type_expr(inner),

        // -- Union: flatten arms + dedup, first-seen order (infer.rs:67-77) --
        TypeExpr::Union(arms) => {
            let mut out: Vec<RuntimeCtorKind> = Vec::new();
            for arm in arms.iter() {
                for ctor in runtime_constructors_from_type_expr(arm) {
                    if !out.contains(&ctor) {
                        out.push(ctor);
                    }
                }
            }
            out
        }

        // -- Intersection: keep non-Unknown deduped arms; empty => Object
        //    (infer.rs:80-95) --
        TypeExpr::Intersection(arms) => {
            let mut out: Vec<RuntimeCtorKind> = Vec::new();
            for arm in arms.iter() {
                for ctor in runtime_constructors_from_type_expr(arm) {
                    if ctor != RuntimeCtorKind::Unknown && !out.contains(&ctor) {
                        out.push(ctor);
                    }
                }
            }
            if out.is_empty() {
                vec![RuntimeCtorKind::Object]
            } else {
                out
            }
        }

        // -- Named reference: built-ins / utility types (infer.rs:98 → :179) --
        TypeExpr::Ref {
            name,
            type_arguments,
        } => runtime_constructors_from_ref(name, type_arguments),

        // -- Conditional: union both branches; empty => Unknown
        //    (infer.rs:101-113) --
        TypeExpr::Conditional {
            true_type,
            false_type,
            ..
        } => {
            let mut out = runtime_constructors_from_type_expr(true_type);
            for ctor in runtime_constructors_from_type_expr(false_type) {
                if !out.contains(&ctor) {
                    out.push(ctor);
                }
            }
            if out.is_empty() {
                vec![RuntimeCtorKind::Unknown]
            } else {
                out
            }
        }

        // -- Mapped type => Object (infer.rs:116) --
        TypeExpr::Mapped { .. } => vec![RuntimeCtorKind::Object],

        // -- Indexed access `T[K]` => Unknown (infer.rs:119) --
        TypeExpr::IndexedAccess { .. } => vec![RuntimeCtorKind::Unknown],

        // -- Template literal => String (infer.rs:122) --
        TypeExpr::TemplateLiteral { .. } => vec![RuntimeCtorKind::String],

        // -- `typeof x`: in a defineProps context, an object shape
        //    (infer.rs:125) --
        TypeExpr::TypeOf(_) => vec![RuntimeCtorKind::Object],

        // -- `keyof T` => string | number | symbol (infer.rs:131-138) --
        TypeExpr::KeyOf(_) => vec![
            RuntimeCtorKind::String,
            RuntimeCtorKind::Number,
            RuntimeCtorKind::Symbol,
        ],

        // -- `infer T` => Unknown (infer.rs:145) --
        TypeExpr::Infer { .. } => vec![RuntimeCtorKind::Unknown],

        // -- A first-class generic type parameter has no resolved runtime
        //    constructor: legacy sees a bare `T` as an unresolved
        //    `TSTypeReference` and returns Unknown (infer.rs:236). --
        TypeExpr::TypeParameter(_) => vec![RuntimeCtorKind::Unknown],

        // -- A recursion placeholder is not reducible to a runtime constructor;
        //    treat it as the unresolved-reference fallback (infer.rs:236). The
        //    legacy OXC walker never minted this node. --
        TypeExpr::RecursiveRef { .. } => vec![RuntimeCtorKind::Unknown],

        // -- Synthetic slot-binding carrier: a binding-object surface, so its
        //    runtime shape is an object. The legacy OXC walker never saw this
        //    Vue-internal carrier; an object surface is the structural truth. --
        TypeExpr::SyntheticSlotBinding(_) => vec![RuntimeCtorKind::Object],

        // -- A type the lowering could not represent => Unknown (the catch-all
        //    at infer.rs:154). --
        TypeExpr::Unknown { .. } => vec![RuntimeCtorKind::Unknown],
    }
}

/// Reduce a named [`TypeExpr::Ref`] to its runtime constructor kinds, mirroring
/// the parser's `infer_type_reference` (`infer.rs:179`): the recognised
/// built-in JS constructors, the recognised built-in classes (`Date`, `Map`,
/// …), and the TypeScript utility types that imply a concrete runtime shape.
/// Any other name is an unresolved reference and yields `[Unknown]`.
fn runtime_constructors_from_ref(name: &str, type_arguments: &[TypeExpr]) -> Vec<RuntimeCtorKind> {
    match name {
        // Built-in JavaScript constructors (infer.rs:184-190).
        "Array" | "ReadonlyArray" => vec![RuntimeCtorKind::Array],
        "Function" => vec![RuntimeCtorKind::Function],
        "Object" => vec![RuntimeCtorKind::Object],
        "String" => vec![RuntimeCtorKind::String],
        "Number" => vec![RuntimeCtorKind::Number],
        "Boolean" => vec![RuntimeCtorKind::Boolean],
        "Symbol" => vec![RuntimeCtorKind::Symbol],

        // Recognised built-in classes => BuiltIn(name) (infer.rs:193-195).
        "Date" | "RegExp" | "Error" | "Map" | "Set" | "WeakMap" | "WeakSet" | "Promise" => {
            vec![RuntimeCtorKind::BuiltIn(name.to_string())]
        }

        // Object-shaped utility types (infer.rs:198-200).
        "Partial" | "Required" | "Readonly" | "Record" | "Pick" | "Omit" | "InstanceType" => {
            vec![RuntimeCtorKind::Object]
        }
        // Array-shaped utility types (infer.rs:201).
        "Parameters" | "ConstructorParameters" => vec![RuntimeCtorKind::Array],
        // ReturnType is un-inferable without resolution (infer.rs:202).
        "ReturnType" => vec![RuntimeCtorKind::Unknown],
        // String-case utility types (infer.rs:203).
        "Uppercase" | "Lowercase" | "Capitalize" | "Uncapitalize" => {
            vec![RuntimeCtorKind::String]
        }

        // NonNullable<T> infers from T, filtering Null (infer.rs:204-215).
        "NonNullable" => match type_arguments.first() {
            Some(first) => runtime_constructors_from_type_expr(first)
                .into_iter()
                .filter(|ctor| *ctor != RuntimeCtorKind::Null)
                .collect(),
            None => vec![RuntimeCtorKind::Unknown],
        },

        // Extract<T, U> returns U — infer from the 2nd arg (infer.rs:216-224).
        "Extract" => match type_arguments.get(1) {
            Some(second) => runtime_constructors_from_type_expr(second),
            None => vec![RuntimeCtorKind::Unknown],
        },

        // Exclude<T, U> / OmitThisParameter<T> infer from the 1st arg
        // (infer.rs:225-233).
        "Exclude" | "OmitThisParameter" => match type_arguments.first() {
            Some(first) => runtime_constructors_from_type_expr(first),
            None => vec![RuntimeCtorKind::Unknown],
        },

        // Unknown reference — can't resolve without scope (infer.rs:236).
        _ => vec![RuntimeCtorKind::Unknown],
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;
    use verter_type_expr::{FunctionExpr, ObjectExpr, TupleElement, TypeParam};

    fn prim(name: PrimitiveName) -> TypeExpr {
        TypeExpr::Primitive(name)
    }

    // -- Primitives --

    #[test]
    fn string_primitive_maps_to_string() {
        assert_eq!(
            runtime_constructors_from_type_expr(&prim(PrimitiveName::String)),
            vec![RuntimeCtorKind::String]
        );
    }

    #[test]
    fn number_primitive_maps_to_number() {
        assert_eq!(
            runtime_constructors_from_type_expr(&prim(PrimitiveName::Number)),
            vec![RuntimeCtorKind::Number]
        );
    }

    #[test]
    fn boolean_primitive_maps_to_boolean() {
        assert_eq!(
            runtime_constructors_from_type_expr(&prim(PrimitiveName::Boolean)),
            vec![RuntimeCtorKind::Boolean]
        );
    }

    #[test]
    fn symbol_primitive_maps_to_symbol() {
        assert_eq!(
            runtime_constructors_from_type_expr(&prim(PrimitiveName::Symbol)),
            vec![RuntimeCtorKind::Symbol]
        );
    }

    #[test]
    fn object_primitive_maps_to_object() {
        assert_eq!(
            runtime_constructors_from_type_expr(&prim(PrimitiveName::Object)),
            vec![RuntimeCtorKind::Object]
        );
    }

    #[test]
    fn null_primitive_maps_to_null() {
        assert_eq!(
            runtime_constructors_from_type_expr(&prim(PrimitiveName::Null)),
            vec![RuntimeCtorKind::Null]
        );
    }

    #[test]
    fn bigint_primitive_maps_to_number() {
        // Legacy infer.rs:49 — TSBigIntKeyword => Number.
        assert_eq!(
            runtime_constructors_from_type_expr(&prim(PrimitiveName::BigInt)),
            vec![RuntimeCtorKind::Number]
        );
    }

    #[test]
    fn undefined_void_any_unknown_never_map_to_unknown() {
        // Legacy infer.rs:44-48 — each of these => Unknown.
        for name in [
            PrimitiveName::Undefined,
            PrimitiveName::Void,
            PrimitiveName::Any,
            PrimitiveName::Unknown,
            PrimitiveName::Never,
        ] {
            assert_eq!(
                runtime_constructors_from_type_expr(&prim(name)),
                vec![RuntimeCtorKind::Unknown],
                "primitive {name:?} should map to [Unknown]"
            );
        }
    }

    // -- Literals --

    #[test]
    fn string_literal_maps_to_string() {
        assert_eq!(
            runtime_constructors_from_type_expr(&TypeExpr::string_literal("hello")),
            vec![RuntimeCtorKind::String]
        );
    }

    #[test]
    fn number_literal_maps_to_number() {
        assert_eq!(
            runtime_constructors_from_type_expr(&TypeExpr::number_literal(42.0)),
            vec![RuntimeCtorKind::Number]
        );
    }

    #[test]
    fn boolean_literal_maps_to_boolean() {
        assert_eq!(
            runtime_constructors_from_type_expr(&TypeExpr::boolean_literal(true)),
            vec![RuntimeCtorKind::Boolean]
        );
    }

    #[test]
    fn bigint_literal_maps_to_number() {
        // Legacy infer.rs:164 — TSLiteral::BigIntLiteral => Number.
        assert_eq!(
            runtime_constructors_from_type_expr(&TypeExpr::Literal(LiteralValue::BigInt(
                "10n".to_string()
            ))),
            vec![RuntimeCtorKind::Number]
        );
    }

    // -- Union (flatten + dedup + order) --

    #[test]
    fn union_string_number_maps_in_order() {
        let ty = TypeExpr::Union(Arc::from(vec![
            prim(PrimitiveName::String),
            prim(PrimitiveName::Number),
        ]));
        assert_eq!(
            runtime_constructors_from_type_expr(&ty),
            vec![RuntimeCtorKind::String, RuntimeCtorKind::Number]
        );
    }

    #[test]
    fn union_dedupes_repeated_constructor() {
        // string | string => [String] (legacy dedup at infer.rs:71-73).
        let ty = TypeExpr::Union(Arc::from(vec![
            prim(PrimitiveName::String),
            prim(PrimitiveName::String),
        ]));
        assert_eq!(
            runtime_constructors_from_type_expr(&ty),
            vec![RuntimeCtorKind::String]
        );
    }

    #[test]
    fn union_preserves_first_seen_order_on_dedup() {
        // number | string | number => [Number, String] (first-seen order kept).
        let ty = TypeExpr::Union(Arc::from(vec![
            prim(PrimitiveName::Number),
            prim(PrimitiveName::String),
            prim(PrimitiveName::Number),
        ]));
        assert_eq!(
            runtime_constructors_from_type_expr(&ty),
            vec![RuntimeCtorKind::Number, RuntimeCtorKind::String]
        );
    }

    #[test]
    fn nested_union_flattens_and_dedupes() {
        // string | (number | string) => [String, Number] — nested arm flattened,
        // the repeated String deduped, first-seen order preserved.
        let inner = TypeExpr::Union(Arc::from(vec![
            prim(PrimitiveName::Number),
            prim(PrimitiveName::String),
        ]));
        let ty = TypeExpr::Union(Arc::from(vec![prim(PrimitiveName::String), inner]));
        assert_eq!(
            runtime_constructors_from_type_expr(&ty),
            vec![RuntimeCtorKind::String, RuntimeCtorKind::Number]
        );
    }

    // -- Intersection --

    #[test]
    fn intersection_of_objects_maps_to_object() {
        // { } & { } — both arms Object; legacy keeps non-Unknown deduped => [Object].
        let arm = TypeExpr::Object(Arc::new(ObjectExpr { properties: vec![] }));
        let ty = TypeExpr::Intersection(Arc::from(vec![arm.clone(), arm]));
        assert_eq!(
            runtime_constructors_from_type_expr(&ty),
            vec![RuntimeCtorKind::Object]
        );
    }

    #[test]
    fn intersection_filters_unknown_then_falls_back_to_object() {
        // any & any — every arm is Unknown; legacy filters Unknown (infer.rs:85)
        // then, finding the list empty, falls back to [Object] (infer.rs:90-94).
        let ty = TypeExpr::Intersection(Arc::from(vec![
            prim(PrimitiveName::Any),
            prim(PrimitiveName::Any),
        ]));
        assert_eq!(
            runtime_constructors_from_type_expr(&ty),
            vec![RuntimeCtorKind::Object]
        );
    }

    #[test]
    fn intersection_keeps_non_unknown_arms() {
        // string & number — neither is Unknown; legacy keeps both deduped in order.
        let ty = TypeExpr::Intersection(Arc::from(vec![
            prim(PrimitiveName::String),
            prim(PrimitiveName::Number),
        ]));
        assert_eq!(
            runtime_constructors_from_type_expr(&ty),
            vec![RuntimeCtorKind::String, RuntimeCtorKind::Number]
        );
    }

    // -- Array / Tuple --

    #[test]
    fn array_maps_to_array() {
        let ty = TypeExpr::Array {
            element: Arc::new(prim(PrimitiveName::String)),
            readonly: false,
        };
        assert_eq!(
            runtime_constructors_from_type_expr(&ty),
            vec![RuntimeCtorKind::Array]
        );
    }

    #[test]
    fn tuple_maps_to_array() {
        let ty = TypeExpr::Tuple {
            elements: Arc::from(vec![TupleElement {
                label: None,
                ty: prim(PrimitiveName::Number),
                optional: false,
                rest: false,
            }]),
            readonly: false,
        };
        assert_eq!(
            runtime_constructors_from_type_expr(&ty),
            vec![RuntimeCtorKind::Array]
        );
    }

    // -- Function --

    #[test]
    fn function_maps_to_function() {
        let ty = TypeExpr::Function(Arc::new(FunctionExpr::synthetic(vec![], None, vec![])));
        assert_eq!(
            runtime_constructors_from_type_expr(&ty),
            vec![RuntimeCtorKind::Function]
        );
    }

    // -- Object --

    #[test]
    fn object_literal_maps_to_object() {
        let ty = TypeExpr::Object(Arc::new(ObjectExpr { properties: vec![] }));
        assert_eq!(
            runtime_constructors_from_type_expr(&ty),
            vec![RuntimeCtorKind::Object]
        );
    }

    // -- Ref: built-in JS constructors --

    #[test]
    fn ref_array_name_maps_to_array() {
        assert_eq!(
            runtime_constructors_from_type_expr(&TypeExpr::named("Array")),
            vec![RuntimeCtorKind::Array]
        );
        assert_eq!(
            runtime_constructors_from_type_expr(&TypeExpr::named("ReadonlyArray")),
            vec![RuntimeCtorKind::Array]
        );
    }

    #[test]
    fn ref_scalar_constructor_names_map_to_scalars() {
        assert_eq!(
            runtime_constructors_from_type_expr(&TypeExpr::named("Function")),
            vec![RuntimeCtorKind::Function]
        );
        assert_eq!(
            runtime_constructors_from_type_expr(&TypeExpr::named("Object")),
            vec![RuntimeCtorKind::Object]
        );
        assert_eq!(
            runtime_constructors_from_type_expr(&TypeExpr::named("String")),
            vec![RuntimeCtorKind::String]
        );
        assert_eq!(
            runtime_constructors_from_type_expr(&TypeExpr::named("Number")),
            vec![RuntimeCtorKind::Number]
        );
        assert_eq!(
            runtime_constructors_from_type_expr(&TypeExpr::named("Boolean")),
            vec![RuntimeCtorKind::Boolean]
        );
        assert_eq!(
            runtime_constructors_from_type_expr(&TypeExpr::named("Symbol")),
            vec![RuntimeCtorKind::Symbol]
        );
    }

    // -- Ref: recognised built-in classes => BuiltIn(name) --

    #[test]
    fn ref_date_maps_to_builtin_date() {
        assert_eq!(
            runtime_constructors_from_type_expr(&TypeExpr::named("Date")),
            vec![RuntimeCtorKind::BuiltIn("Date".to_string())]
        );
    }

    #[test]
    fn ref_recognised_builtin_classes_carry_their_name() {
        // Legacy infer.rs:193-195 — exactly this name set.
        for name in [
            "Date", "RegExp", "Error", "Map", "Set", "WeakMap", "WeakSet", "Promise",
        ] {
            assert_eq!(
                runtime_constructors_from_type_expr(&TypeExpr::named(name)),
                vec![RuntimeCtorKind::BuiltIn(name.to_string())],
                "builtin {name} should map to BuiltIn({name})"
            );
        }
    }

    // -- Ref: utility types --

    #[test]
    fn ref_object_utility_types_map_to_object() {
        // Legacy infer.rs:198-200.
        for name in [
            "Partial",
            "Required",
            "Readonly",
            "Record",
            "Pick",
            "Omit",
            "InstanceType",
        ] {
            assert_eq!(
                runtime_constructors_from_type_expr(&TypeExpr::named(name)),
                vec![RuntimeCtorKind::Object],
                "utility {name} should map to [Object]"
            );
        }
    }

    #[test]
    fn ref_array_utility_types_map_to_array() {
        // Legacy infer.rs:201.
        for name in ["Parameters", "ConstructorParameters"] {
            assert_eq!(
                runtime_constructors_from_type_expr(&TypeExpr::named(name)),
                vec![RuntimeCtorKind::Array],
                "utility {name} should map to [Array]"
            );
        }
    }

    #[test]
    fn ref_returntype_maps_to_unknown() {
        // Legacy infer.rs:202.
        assert_eq!(
            runtime_constructors_from_type_expr(&TypeExpr::named("ReturnType")),
            vec![RuntimeCtorKind::Unknown]
        );
    }

    #[test]
    fn ref_string_case_utility_types_map_to_string() {
        // Legacy infer.rs:203.
        for name in ["Uppercase", "Lowercase", "Capitalize", "Uncapitalize"] {
            assert_eq!(
                runtime_constructors_from_type_expr(&TypeExpr::named(name)),
                vec![RuntimeCtorKind::String],
                "case-utility {name} should map to [String]"
            );
        }
    }

    #[test]
    fn ref_nonnullable_strips_null_from_arg() {
        // Legacy infer.rs:204-215 — NonNullable<string | null> => [String]
        // (infers from the type arg, filtering Null).
        let ty = TypeExpr::named_with_args(
            "NonNullable",
            vec![TypeExpr::Union(Arc::from(vec![
                prim(PrimitiveName::String),
                prim(PrimitiveName::Null),
            ]))],
        );
        assert_eq!(
            runtime_constructors_from_type_expr(&ty),
            vec![RuntimeCtorKind::String]
        );
    }

    #[test]
    fn ref_nonnullable_without_args_maps_to_unknown() {
        // Legacy infer.rs:214 — no type arg => Unknown.
        assert_eq!(
            runtime_constructors_from_type_expr(&TypeExpr::named("NonNullable")),
            vec![RuntimeCtorKind::Unknown]
        );
    }

    #[test]
    fn ref_extract_infers_from_second_arg() {
        // Legacy infer.rs:216-224 — Extract<T, U> infers from U (the 2nd arg).
        let ty = TypeExpr::named_with_args(
            "Extract",
            vec![prim(PrimitiveName::Boolean), prim(PrimitiveName::Number)],
        );
        assert_eq!(
            runtime_constructors_from_type_expr(&ty),
            vec![RuntimeCtorKind::Number]
        );
    }

    #[test]
    fn ref_extract_without_second_arg_maps_to_unknown() {
        // Legacy infer.rs:223 — missing 2nd arg => Unknown.
        let ty = TypeExpr::named_with_args("Extract", vec![prim(PrimitiveName::Boolean)]);
        assert_eq!(
            runtime_constructors_from_type_expr(&ty),
            vec![RuntimeCtorKind::Unknown]
        );
    }

    #[test]
    fn ref_exclude_infers_from_first_arg() {
        // Legacy infer.rs:225-233 — Exclude<T, U> / OmitThisParameter infer from
        // the first arg.
        for name in ["Exclude", "OmitThisParameter"] {
            let ty = TypeExpr::named_with_args(
                name,
                vec![prim(PrimitiveName::String), prim(PrimitiveName::Null)],
            );
            assert_eq!(
                runtime_constructors_from_type_expr(&ty),
                vec![RuntimeCtorKind::String],
                "{name} should infer from its first type arg"
            );
        }
    }

    #[test]
    fn ref_exclude_without_args_maps_to_unknown() {
        let ty = TypeExpr::named("Exclude");
        assert_eq!(
            runtime_constructors_from_type_expr(&ty),
            vec![RuntimeCtorKind::Unknown]
        );
    }

    #[test]
    fn ref_unrecognised_name_maps_to_unknown() {
        // Legacy infer.rs:236 — an unresolved reference => Unknown.
        assert_eq!(
            runtime_constructors_from_type_expr(&TypeExpr::named("MyCustomType")),
            vec![RuntimeCtorKind::Unknown]
        );
    }

    // -- Operators --

    #[test]
    fn keyof_maps_to_string_number_symbol() {
        // Legacy infer.rs:132-138.
        let ty = TypeExpr::KeyOf(Arc::new(prim(PrimitiveName::Object)));
        assert_eq!(
            runtime_constructors_from_type_expr(&ty),
            vec![
                RuntimeCtorKind::String,
                RuntimeCtorKind::Number,
                RuntimeCtorKind::Symbol,
            ]
        );
    }

    #[test]
    fn typeof_maps_to_object() {
        // Legacy infer.rs:125 — typeof x in defineProps context => Object.
        let ty = TypeExpr::TypeOf(verter_type_expr::ValueRef {
            path: vec!["x".to_string()],
        });
        assert_eq!(
            runtime_constructors_from_type_expr(&ty),
            vec![RuntimeCtorKind::Object]
        );
    }

    #[test]
    fn indexed_access_maps_to_unknown() {
        // Legacy infer.rs:119.
        let ty = TypeExpr::IndexedAccess {
            object: Arc::new(prim(PrimitiveName::Object)),
            index: Arc::new(TypeExpr::string_literal("k")),
        };
        assert_eq!(
            runtime_constructors_from_type_expr(&ty),
            vec![RuntimeCtorKind::Unknown]
        );
    }

    #[test]
    fn conditional_unions_both_branches() {
        // Legacy infer.rs:101-113 — union of true_type and false_type.
        let ty = TypeExpr::Conditional {
            check: Arc::new(prim(PrimitiveName::String)),
            extends: Arc::new(prim(PrimitiveName::String)),
            true_type: Arc::new(prim(PrimitiveName::Number)),
            false_type: Arc::new(prim(PrimitiveName::Boolean)),
        };
        assert_eq!(
            runtime_constructors_from_type_expr(&ty),
            vec![RuntimeCtorKind::Number, RuntimeCtorKind::Boolean]
        );
    }

    #[test]
    fn conditional_dedupes_across_branches() {
        // Both branches String => [String] (dedup at infer.rs:103-107).
        let ty = TypeExpr::Conditional {
            check: Arc::new(prim(PrimitiveName::String)),
            extends: Arc::new(prim(PrimitiveName::String)),
            true_type: Arc::new(prim(PrimitiveName::String)),
            false_type: Arc::new(prim(PrimitiveName::String)),
        };
        assert_eq!(
            runtime_constructors_from_type_expr(&ty),
            vec![RuntimeCtorKind::String]
        );
    }

    #[test]
    fn mapped_maps_to_object() {
        // Legacy infer.rs:116.
        let ty = TypeExpr::Mapped {
            parameter: "K".to_string(),
            source: Arc::new(TypeExpr::KeyOf(Arc::new(prim(PrimitiveName::Object)))),
            value: Arc::new(prim(PrimitiveName::String)),
            optional: verter_type_expr::MappedModifier::None,
            readonly: verter_type_expr::MappedModifier::None,
            name_type: None,
        };
        assert_eq!(
            runtime_constructors_from_type_expr(&ty),
            vec![RuntimeCtorKind::Object]
        );
    }

    #[test]
    fn template_literal_maps_to_string() {
        // Legacy infer.rs:122.
        let ty = TypeExpr::TemplateLiteral {
            quasis: vec!["pre".to_string(), "post".to_string()],
            expressions: Arc::from(vec![prim(PrimitiveName::String)]),
        };
        assert_eq!(
            runtime_constructors_from_type_expr(&ty),
            vec![RuntimeCtorKind::String]
        );
    }

    #[test]
    fn infer_maps_to_unknown() {
        // Legacy infer.rs:145.
        let ty = TypeExpr::Infer {
            name: "T".to_string(),
        };
        assert_eq!(
            runtime_constructors_from_type_expr(&ty),
            vec![RuntimeCtorKind::Unknown]
        );
    }

    // -- Transparent wrappers --

    #[test]
    fn parenthesized_is_transparent() {
        // Legacy infer.rs:64 — recurse into the inner type.
        let ty = TypeExpr::Parenthesized(Arc::new(prim(PrimitiveName::Number)));
        assert_eq!(
            runtime_constructors_from_type_expr(&ty),
            vec![RuntimeCtorKind::Number]
        );
    }

    #[test]
    fn rest_is_transparent() {
        // `readonly T` / standalone rest — legacy's non-Keyof type operator
        // recurses into the inner type (infer.rs:140).
        let ty = TypeExpr::Rest(Arc::new(prim(PrimitiveName::String)));
        assert_eq!(
            runtime_constructors_from_type_expr(&ty),
            vec![RuntimeCtorKind::String]
        );
    }

    // -- Fallbacks for variants with no recognised runtime constructor --

    #[test]
    fn type_parameter_maps_to_unknown() {
        // A bare generic param `T` is, in legacy, an unresolved TSTypeReference
        // => Unknown (infer.rs:236).
        let ty = TypeExpr::TypeParameter(TypeParam {
            name: "T".to_string(),
            constraint: None,
            default: None,
        });
        assert_eq!(
            runtime_constructors_from_type_expr(&ty),
            vec![RuntimeCtorKind::Unknown]
        );
    }

    #[test]
    fn unknown_maps_to_unknown() {
        // Legacy catch-all (infer.rs:154).
        let ty = TypeExpr::Unknown {
            raw: "weird".to_string(),
        };
        assert_eq!(
            runtime_constructors_from_type_expr(&ty),
            vec![RuntimeCtorKind::Unknown]
        );
    }

    #[test]
    fn recursive_ref_maps_to_unknown() {
        // A recursion placeholder is not reducible to a runtime constructor =>
        // Unknown (legacy never minted this node; an unresolved ref => Unknown).
        let ty = TypeExpr::recursive_ref("Self", vec![]);
        assert_eq!(
            runtime_constructors_from_type_expr(&ty),
            vec![RuntimeCtorKind::Unknown]
        );
    }
}
