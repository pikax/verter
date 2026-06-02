//! Reference-set analysis for [`TypeExpr`] — does the expression
//! mention any name in a caller-supplied set?
//!
//! Used by the host-side macro field expander to decide whether a
//! `defineProps`/`defineEmits`/`defineSlots` field's parsed type
//! expression depends on any of its parent shell's type parameters.
//! When the answer is "no", the closure can short-circuit to
//! `ExpansionResult::exact_concrete(parsed)` — no parent projection
//! runs, saving every dispatch the slow path would have triggered.
//!
//! ### Shadowing
//!
//! The walk honours scope shadowing introduced by mapped types
//! (`{ [K in S]: V }` introduces `K` inside `V` and `name_type`) and
//! function-type parameter lists (`<U,V>(x: U) => V` introduces `U`,
//! `V` inside the function signature). When a binding shadows an
//! outer name, the inner subtree masks the outer binding so the
//! masked name does NOT count as a reference inside that subtree.
//! Outside the binding, the outer name is restored.
//!
//! Three public helpers are exposed:
//! - [`type_expr_references_names`] — generic predicate over a name
//!   filter; the most general entry point.
//! - [`function_references_names`] — equivalent walk over a
//!   [`FunctionExpr`] body; useful when the caller has already
//!   stripped a `TypeExpr::Function` wrapper.
//! - [`field_references_type_params`] — convenience wrapper over a
//!   [`TypeParam`] slice (the macro shell's prepared type parameters).
//!
//! All three are `pub` rather than `pub(crate)` because the host
//! crate (`verter_session`) consumes them across the crate boundary.

use verter_type_expr::{FunctionExpr, ObjectMember, TypeExpr, TypeParam};

/// True iff `expr` transitively references any name for which
/// `is_active(name)` returns `true`, after honouring shadowing
/// introduced by mapped types and function-type parameter lists.
///
/// `is_active` is consulted only for names not currently shadowed by
/// an enclosing mapped/function binding — when a name is shadowed,
/// the predicate returns `false` for that name within the shadowed
/// subtree.
pub fn type_expr_references_names(expr: &TypeExpr, is_active: &impl Fn(&str) -> bool) -> bool {
    let mut shadow = ShadowStack::default();
    visit_expr(expr, is_active, &mut shadow)
}

/// True iff the function-type body — parameters, return type, and
/// constraint/default of any nested type-parameter declarations —
/// references any active name. Function-level type parameters mask
/// outer names of the same identifier inside the function's body.
pub fn function_references_names(func: &FunctionExpr, is_active: &impl Fn(&str) -> bool) -> bool {
    let mut shadow = ShadowStack::default();
    visit_function(func, is_active, &mut shadow)
}

/// Convenience wrapper for the macro field fast path: returns `true`
/// iff `expr` references any of the type parameters declared on the
/// macro's parent shell. Names listed in `params` are treated as
/// active; shadowing rules apply.
pub fn field_references_type_params(expr: &TypeExpr, params: &[TypeParam]) -> bool {
    if params.is_empty() {
        return false;
    }
    type_expr_references_names(expr, &|name| params.iter().any(|p| p.name == name))
}

// ---------------------------------------------------------------------------
// Shadowing stack
// ---------------------------------------------------------------------------

/// Append-only stack of shadowed names introduced by enclosing
/// mapped-type / function-type-parameter scopes. A name appears in
/// the stack once per enclosing binding; popping a frame restores
/// the prior binding count for that name.
#[derive(Default)]
struct ShadowStack {
    /// Each entry is the name introduced by an enclosing binding.
    /// Duplicate entries (same name, different scopes) are kept
    /// because each distinct entry records a distinct shadow level.
    frames: Vec<String>,
}

impl ShadowStack {
    fn push(&mut self, name: &str) -> ShadowGuardPos {
        self.frames.push(name.to_string());
        ShadowGuardPos {
            len_before: self.frames.len() - 1,
        }
    }

    fn push_many(&mut self, names: impl IntoIterator<Item = String>) -> ShadowGuardPos {
        let len_before = self.frames.len();
        self.frames.extend(names);
        ShadowGuardPos { len_before }
    }

    fn pop_to(&mut self, pos: ShadowGuardPos) {
        self.frames.truncate(pos.len_before);
    }

    fn shadows(&self, name: &str) -> bool {
        self.frames.iter().any(|n| n == name)
    }
}

#[must_use]
struct ShadowGuardPos {
    len_before: usize,
}

// ---------------------------------------------------------------------------
// Visitor — internal recursion over TypeExpr / FunctionExpr.
// ---------------------------------------------------------------------------

fn visit_expr(
    expr: &TypeExpr,
    is_active: &impl Fn(&str) -> bool,
    shadow: &mut ShadowStack,
) -> bool {
    match expr {
        TypeExpr::Primitive(_)
        | TypeExpr::Literal(_)
        | TypeExpr::Unknown { .. }
        | TypeExpr::RecursiveRef { .. }
        | TypeExpr::TypeOf(_)
        | TypeExpr::Infer { .. }
        // Synthetic carriers carry no embedded type-parameter references
        // (their identity is the closed scope/slot/binding tuple).
        | TypeExpr::SyntheticSlotBinding(_) => false,
        TypeExpr::Ref {
            name,
            type_arguments,
        } => {
            let bare = !shadow.shadows(name.as_ref()) && is_active(name.as_ref());
            if bare {
                return true;
            }
            type_arguments
                .iter()
                .any(|arg| visit_expr(arg, is_active, shadow))
        }
        TypeExpr::TypeParameter(param) => {
            // A first-class TypeParameter reference matches when its
            // name is active and not shadowed. The constraint/default
            // sit alongside the param node and do not introduce new
            // shadow scopes themselves — they share the enclosing
            // declaration's scope.
            if !shadow.shadows(param.name.as_str()) && is_active(param.name.as_str()) {
                return true;
            }
            param
                .constraint
                .as_deref()
                .is_some_and(|c| visit_expr(c, is_active, shadow))
                || param
                    .default
                    .as_deref()
                    .is_some_and(|d| visit_expr(d, is_active, shadow))
        }
        TypeExpr::Parenthesized(inner)
        | TypeExpr::Array { element: inner, .. }
        | TypeExpr::KeyOf(inner)
        | TypeExpr::Rest(inner) => visit_expr(inner, is_active, shadow),
        TypeExpr::Tuple { elements, .. } => elements
            .iter()
            .any(|element| visit_expr(&element.ty, is_active, shadow)),
        TypeExpr::Union(types)
        | TypeExpr::Intersection(types)
        | TypeExpr::TemplateLiteral {
            expressions: types, ..
        } => types.iter().any(|ty| visit_expr(ty, is_active, shadow)),
        TypeExpr::Object(object) => object.properties.iter().any(|member| match member {
            ObjectMember::Property(property) => visit_expr(&property.ty, is_active, shadow),
            ObjectMember::IndexSignature(signature) => {
                visit_expr(&signature.key_type, is_active, shadow)
                    || visit_expr(&signature.value_type, is_active, shadow)
            }
            ObjectMember::CallSignature(function) | ObjectMember::ConstructSignature(function) => {
                visit_function(function, is_active, shadow)
            }
            ObjectMember::Method(method) => visit_function(&method.function, is_active, shadow),
        }),
        // A constructor type carries the same `FunctionExpr` payload as a
        // function type — its parameters / return may reference an enclosing
        // type parameter, so it is walked identically.
        TypeExpr::Function(function) | TypeExpr::ConstructorType(function) => {
            visit_function(function, is_active, shadow)
        }
        TypeExpr::IndexedAccess { object, index } => {
            visit_expr(object, is_active, shadow) || visit_expr(index, is_active, shadow)
        }
        TypeExpr::Conditional {
            check,
            extends,
            true_type,
            false_type,
        } => {
            visit_expr(check, is_active, shadow)
                || visit_expr(extends, is_active, shadow)
                || visit_expr(true_type, is_active, shadow)
                || visit_expr(false_type, is_active, shadow)
        }
        TypeExpr::Mapped {
            parameter,
            source,
            value,
            name_type,
            ..
        } => {
            // The mapped-type binder `{ [K in source]: value }`
            // introduces `K` inside `value` and `name_type` but NOT
            // inside `source` itself (the binder reads from `source`).
            if visit_expr(source, is_active, shadow) {
                return true;
            }
            let pos = shadow.push(parameter);
            let hit = visit_expr(value, is_active, shadow)
                || name_type
                    .as_deref()
                    .is_some_and(|nt| visit_expr(nt, is_active, shadow));
            shadow.pop_to(pos);
            hit
        }
    }
}

fn visit_function(
    func: &FunctionExpr,
    is_active: &impl Fn(&str) -> bool,
    shadow: &mut ShadowStack,
) -> bool {
    // Function-level type parameters introduce a new shadow scope
    // covering the parameters, return type, AND nested constraint /
    // default expressions.
    let pos = shadow.push_many(func.type_parameters.iter().map(|p| p.name.clone()));
    let hit = func.type_parameters.iter().any(|tp| {
        tp.constraint
            .as_deref()
            .is_some_and(|c| visit_expr(c, is_active, shadow))
            || tp
                .default
                .as_deref()
                .is_some_and(|d| visit_expr(d, is_active, shadow))
    }) || func
        .parameters
        .iter()
        .any(|param| visit_expr(&param.ty, is_active, shadow))
        || func
            .return_type
            .as_deref()
            .is_some_and(|rt| visit_expr(rt, is_active, shadow));
    shadow.pop_to(pos);
    hit
}

// ---------------------------------------------------------------------------
// Self-tests — exercises shadowing semantics on the public surface.
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;
    use verter_type_expr::{
        FunctionParam, ObjectExpr, ObjectMember, ObjectProperty, PrimitiveName, TypeExpr,
    };

    fn t_ref(name: &str) -> TypeExpr {
        TypeExpr::Ref {
            name: Arc::from(name),
            type_arguments: Arc::from(Vec::new()),
        }
    }

    fn names_t() -> Vec<TypeParam> {
        vec![TypeParam {
            name: "T".to_string(),
            constraint: None,
            default: None,
        }]
    }

    fn names_t_k() -> Vec<TypeParam> {
        vec![
            TypeParam {
                name: "T".to_string(),
                constraint: None,
                default: None,
            },
            TypeParam {
                name: "K".to_string(),
                constraint: None,
                default: None,
            },
        ]
    }

    #[test]
    fn primitive_field_does_not_reference_any_param() {
        let expr = TypeExpr::Primitive(PrimitiveName::Boolean);
        assert!(!field_references_type_params(&expr, &names_t()));
    }

    #[test]
    fn bare_ref_to_param_is_a_reference() {
        let expr = t_ref("T");
        assert!(field_references_type_params(&expr, &names_t()));
    }

    #[test]
    fn ref_inside_object_property_is_a_reference() {
        let expr = TypeExpr::Object(Arc::new(ObjectExpr {
            properties: vec![ObjectMember::Property(ObjectProperty::synthetic(
                "x".to_string(),
                t_ref("T"),
                false,
                false,
            ))],
        }));
        assert!(field_references_type_params(&expr, &names_t()));
    }

    #[test]
    fn empty_param_list_short_circuits_to_false() {
        let expr = t_ref("T");
        assert!(!field_references_type_params(&expr, &[]));
    }

    // Shadowing — mapped type's parameter masks an outer K.
    #[test]
    fn mapped_type_parameter_masks_outer_k_inside_value() {
        // { [K in U]: K } — outer name set is {K, T}. The inner K is
        // shadowed by the mapped binder; only the mapped source `U`
        // and outer-active `K` outside `value` count. Since `value`
        // is `K` and `K` is shadowed, the result is `false` when only
        // `K` is active (and `U` isn't part of the active set).
        let expr = TypeExpr::Mapped {
            parameter: "K".to_string(),
            source: Arc::new(t_ref("U")),
            value: Arc::new(t_ref("K")),
            optional: verter_type_expr::MappedModifier::None,
            readonly: verter_type_expr::MappedModifier::None,
            name_type: None,
        };
        let active = vec![TypeParam {
            name: "K".to_string(),
            constraint: None,
            default: None,
        }];
        assert!(
            !field_references_type_params(&expr, &active),
            "mapped type's `K` binder must shadow outer `K` inside the value"
        );
    }

    #[test]
    fn mapped_type_source_is_outside_the_inner_shadow_scope() {
        // { [K in T]: number } where `T` is active. The source `T`
        // is NOT shadowed by the binder (binder applies only to
        // value and name_type), so the predicate returns true.
        let expr = TypeExpr::Mapped {
            parameter: "K".to_string(),
            source: Arc::new(t_ref("T")),
            value: Arc::new(TypeExpr::Primitive(PrimitiveName::Number)),
            optional: verter_type_expr::MappedModifier::None,
            readonly: verter_type_expr::MappedModifier::None,
            name_type: None,
        };
        assert!(
            field_references_type_params(&expr, &names_t()),
            "mapped type's source range must NOT be shadowed by the inner binder"
        );
    }

    #[test]
    fn function_type_parameter_masks_outer_t_inside_signature() {
        // <T>(x: T): T — outer `T` is active. The function's own
        // type parameter `T` shadows the outer `T` inside the
        // function's signature. Result: false.
        let func = FunctionExpr::synthetic(
            vec![FunctionParam::synthetic(
                Some("x".to_string()),
                t_ref("T"),
                false,
                false,
            )],
            Some(Arc::new(t_ref("T"))),
            vec![TypeParam {
                name: "T".to_string(),
                constraint: None,
                default: None,
            }],
        );
        assert!(
            !function_references_names(&func, &|name| name == "T"),
            "function-type parameter must shadow outer T"
        );
    }

    #[test]
    fn function_signature_without_shadow_observes_outer_t() {
        // (x: T): T — no own type parameters. Outer T is observed.
        let func = FunctionExpr::synthetic(
            vec![FunctionParam::synthetic(
                Some("x".to_string()),
                t_ref("T"),
                false,
                false,
            )],
            Some(Arc::new(t_ref("T"))),
            Vec::new(),
        );
        assert!(
            function_references_names(&func, &|name| name == "T"),
            "function with no own type parameters must observe outer T"
        );
    }

    #[test]
    fn shadow_stack_restores_after_inner_scope() {
        // Outer: { [K in T]: { [K in keyof Inner]: K } }
        // Outer T active. Inner K is shadowed inside both mappings.
        // Outer T is observed via the outer source.
        let inner = TypeExpr::Mapped {
            parameter: "K".to_string(),
            source: Arc::new(TypeExpr::KeyOf(Arc::new(t_ref("Inner")))),
            value: Arc::new(t_ref("K")),
            optional: verter_type_expr::MappedModifier::None,
            readonly: verter_type_expr::MappedModifier::None,
            name_type: None,
        };
        let outer = TypeExpr::Mapped {
            parameter: "K".to_string(),
            source: Arc::new(t_ref("T")),
            value: Arc::new(inner),
            optional: verter_type_expr::MappedModifier::None,
            readonly: verter_type_expr::MappedModifier::None,
            name_type: None,
        };
        // Active set: {T, K}. Outer T is observed via outer source.
        assert!(field_references_type_params(&outer, &names_t_k()));
    }

    #[test]
    fn conditional_check_observes_param() {
        // T extends string ? 'yes' : 'no' — T is observed in check.
        let expr = TypeExpr::Conditional {
            check: Arc::new(t_ref("T")),
            extends: Arc::new(TypeExpr::Primitive(PrimitiveName::String)),
            true_type: Arc::new(TypeExpr::string_literal("yes")),
            false_type: Arc::new(TypeExpr::string_literal("no")),
        };
        assert!(field_references_type_params(&expr, &names_t()));
    }

    #[test]
    fn type_param_node_carrying_outer_name_is_observed() {
        // A first-class TypeParameter node referencing T is a hit.
        let expr = TypeExpr::TypeParameter(TypeParam {
            name: "T".to_string(),
            constraint: None,
            default: None,
        });
        assert!(field_references_type_params(&expr, &names_t()));
    }
}
