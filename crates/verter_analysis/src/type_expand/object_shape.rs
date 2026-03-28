//! ObjectShape expansion: extracts a materialized object surface from a `TypeExpr`.

use crate::type_eval::{evaluate_with_lookup, EvalEnv, EvalLookup, NoopEvalLookup};
use crate::type_expr::{LiteralValue, ObjectExpr, ObjectMember, TypeExpr};
use rustc_hash::{FxHashMap, FxHashSet};

use super::normalized::record_partial_markers;
use super::request::{
    ExpandedIndexSignature, ExpandedObjectResult, ExpandedObjectShape, ExpandedProperty,
    ExpansionBudget, ExpansionCompleteness, ExpansionDiagnostic, ExpansionResult,
    ExpansionStopReason,
};

/// Expand a `TypeExpr` into an `ExpandedObjectShape`.
///
/// This is the primary entry point for consumers that need a list of
/// typed members (e.g., `defineProps<T>()`, fallthrough surface).
///
/// The expander handles structural types directly (Object, Intersection,
/// Union) and delegates to `evaluate()` for reference resolution, generic
/// instantiation, and utility type application.
pub fn expand_object_shape(
    expr: &TypeExpr,
    env: &mut EvalEnv,
    budget: &ExpansionBudget,
) -> ExpandedObjectResult {
    let mut lookup = NoopEvalLookup;
    expand_object_shape_with_lookup(expr, env, budget, &mut lookup)
}

pub fn expand_object_shape_with_lookup(
    expr: &TypeExpr,
    env: &mut EvalEnv,
    budget: &ExpansionBudget,
    lookup: &mut dyn EvalLookup,
) -> ExpandedObjectResult {
    env.apply_expansion_budget(budget);

    let mut diagnostics = Vec::new();
    let shape = extract_shape(expr, env, &mut diagnostics, lookup);
    if env.budget_exhausted() {
        diagnostics.push(ExpansionDiagnostic {
            reason: ExpansionStopReason::BudgetExceeded,
            context: "symbolic work limit reached during object-shape expansion".to_string(),
            property_name: None,
        });
    }

    // Scan property types for unexpanded forms (Mapped, Conditional, unresolved Ref, etc.)
    for prop in &shape.properties {
        record_partial_markers(&prop.ty, &mut diagnostics, Some(prop.name.as_str()));
    }

    let completeness = if diagnostics.is_empty() {
        ExpansionCompleteness::Exact
    } else {
        ExpansionCompleteness::Partial
    };

    ExpansionResult {
        value: shape,
        completeness,
        diagnostics,
    }
}

/// Extract an `ExpandedObjectShape` from a `TypeExpr`, handling structural
/// forms directly and delegating sub-expression resolution to `evaluate()`.
fn extract_shape(
    expr: &TypeExpr,
    env: &mut EvalEnv,
    diagnostics: &mut Vec<ExpansionDiagnostic>,
    lookup: &mut dyn EvalLookup,
) -> ExpandedObjectShape {
    match expr {
        // Direct object — extract properties.
        // normalize_members = false: property types are preserved as-is rather than
        // deeply evaluated. This avoids burning expansion budget on complex nested types
        // like VNode (200+ symbols) that appear in slot return types or prop types.
        // The shape captures structural information (names, optionality, readonly);
        // deep type normalization happens separately per-field if the consumer needs it.
        TypeExpr::Object(obj) => object_expr_to_shape(obj, env, diagnostics, false, lookup),

        TypeExpr::Parenthesized(inner) => extract_shape(inner, env, diagnostics, lookup),

        // Intersection — merge shapes from each branch with correct optionality
        TypeExpr::Intersection(types) => {
            let shapes: Vec<ExpandedObjectShape> = types
                .iter()
                .map(|t| extract_shape(t, env, diagnostics, lookup))
                .collect();
            merge_intersection_shapes(shapes)
        }

        // Union — Vue props merge semantics
        TypeExpr::Union(types) => {
            let shapes: Vec<ExpandedObjectShape> = types
                .iter()
                .map(|t| extract_shape(t, env, diagnostics, lookup))
                .collect();
            merge_union_shapes_vue(shapes)
        }

        // Type reference — prefer structural extraction from the declared body
        // when the ref itself is known. This preserves targeted expansion and
        // diagnostics for generic bodies even when evaluation bails out early
        // on opaque args such as missing imported types.
        TypeExpr::Ref {
            name,
            type_arguments,
        } => {
            if type_arguments.is_empty() {
                if let Some(bound) = env.type_bindings.get(name.as_ref()).cloned() {
                    return extract_shape(bound.as_ref(), env, diagnostics, lookup);
                }
            }

            // Handle builtin utility types at the shape level to avoid costly
            // deep evaluation of every property type via evaluate_with_lookup.
            // Instead of evaluating Omit<X, K> → Object → extract_shape,
            // we extract_shape(X) → filter shape properties. This avoids burning
            // expansion budget on property type evaluation (e.g., VNode with 200+ symbols).
            if let Some(shape) =
                try_extract_utility_shape(name, type_arguments, env, diagnostics, lookup)
            {
                return shape;
            }

            let decl = env
                .type_symbols
                .get(name.as_ref())
                .cloned()
                .or_else(|| lookup.resolve_type_decl(name));
            if let Some(decl) = decl {
                if env.active.contains(name.as_ref()) {
                    return ExpandedObjectShape::empty();
                }
                env.active.insert(name.to_string());
                let saved = crate::type_eval::bind_type_parameters(&decl, type_arguments, env);
                let shape = extract_shape(&decl.body, env, diagnostics, lookup);
                crate::type_eval::restore_type_parameters(saved, env);
                env.active.remove(&**name);
                return shape;
            }

            let evaluated = evaluate_with_lookup(expr, env, lookup);
            if env.budget_exhausted() {
                diagnostics.push(ExpansionDiagnostic {
                    reason: ExpansionStopReason::BudgetExceeded,
                    context: "symbolic work limit reached during evaluation".to_string(),
                    property_name: None,
                });
            }
            // If evaluate returned the same Ref (unresolved), emit diagnostic
            // Skip if budget already exhausted (that's the real cause)
            if let TypeExpr::Ref { name, .. } = &evaluated {
                if !env.budget_exhausted() {
                    diagnostics.push(ExpansionDiagnostic {
                        reason: ExpansionStopReason::UnresolvedReference,
                        context: format!("unresolved type reference '{name}'"),
                        property_name: None,
                    });
                }
                return ExpandedObjectShape::empty();
            }
            if let TypeExpr::Object(obj) = &evaluated {
                return object_expr_to_shape(obj, env, diagnostics, false, lookup);
            }
            // Recurse on the evaluated result (may be Object, Intersection, etc.)
            extract_shape(&evaluated, env, diagnostics, lookup)
        }

        // Mapped type that wasn't expanded by evaluate (infinite key space or depth limit)
        TypeExpr::Mapped { source, value, .. } => {
            // First try evaluating the whole mapped type
            let evaluated = evaluate_with_lookup(expr, env, lookup);
            // If it resolved to an Object, extract that
            if let TypeExpr::Object(obj) = &evaluated {
                return object_expr_to_shape(obj, env, diagnostics, false, lookup);
            }
            // Still a Mapped — check why
            if is_infinite_source(source) {
                diagnostics.push(ExpansionDiagnostic {
                    reason: ExpansionStopReason::InfiniteKeySpace,
                    context: "mapped type has infinite key space".to_string(),
                    property_name: None,
                });
                ExpandedObjectShape {
                    properties: Vec::new(),
                    index_signatures: vec![ExpandedIndexSignature {
                        key_type: (**source).clone(),
                        value_type: (**value).clone(),
                        readonly: false,
                    }],
                    call_signatures: Vec::new(),
                }
            } else {
                diagnostics.push(ExpansionDiagnostic {
                    reason: ExpansionStopReason::MappedDepthExceeded,
                    context: "mapped type preserved symbolically".to_string(),
                    property_name: None,
                });
                ExpandedObjectShape::empty()
            }
        }

        // Conditional — resolve if possible, skip if indeterminate
        TypeExpr::Conditional {
            check,
            extends,
            true_type,
            false_type,
        } => {
            let check_eval = evaluate_with_lookup(check, env, lookup);
            let extends_eval = evaluate_with_lookup(extends, env, lookup);
            if crate::type_eval::is_assignable_to(&check_eval, &extends_eval) {
                let evaluated_branch = evaluate_with_lookup(true_type, env, lookup);
                if let TypeExpr::Object(obj) = &evaluated_branch {
                    return object_expr_to_shape(obj, env, diagnostics, false, lookup);
                }
                return extract_shape(&evaluated_branch, env, diagnostics, lookup);
            } else if crate::type_eval::is_definitely_not_assignable(&check_eval, &extends_eval) {
                let evaluated_branch = evaluate_with_lookup(false_type, env, lookup);
                if let TypeExpr::Object(obj) = &evaluated_branch {
                    return object_expr_to_shape(obj, env, diagnostics, false, lookup);
                }
                return extract_shape(&evaluated_branch, env, diagnostics, lookup);
            }
            // Indeterminate — skip, emit diagnostic
            diagnostics.push(ExpansionDiagnostic {
                reason: ExpansionStopReason::IndeterminateConditional,
                context: "conditional type could not be resolved".to_string(),
                property_name: None,
            });
            ExpandedObjectShape::empty()
        }

        // KeyOf, IndexedAccess, TypeOf — try evaluating
        TypeExpr::KeyOf(_) | TypeExpr::IndexedAccess { .. } | TypeExpr::TypeOf(_) => {
            let evaluated = evaluate_with_lookup(expr, env, lookup);
            if matches!(
                &evaluated,
                TypeExpr::KeyOf(_) | TypeExpr::IndexedAccess { .. } | TypeExpr::TypeOf(_)
            ) {
                // Still unresolved
                ExpandedObjectShape::empty()
            } else if let TypeExpr::Object(obj) = &evaluated {
                object_expr_to_shape(obj, env, diagnostics, false, lookup)
            } else {
                extract_shape(&evaluated, env, diagnostics, lookup)
            }
        }

        // All other forms produce no object shape
        _ => ExpandedObjectShape::empty(),
    }
}

/// Handle builtin utility types (Omit, Pick, Partial, Required, Readonly) at the
/// shape level without going through `evaluate_with_lookup`. This avoids burning
/// expansion budget on deep property type evaluation (e.g., VNode with 200+ symbols).
///
/// Instead of: evaluate Omit<X,K> → Object → extract_shape
/// We do:      extract_shape(X) → filter/transform shape properties
fn try_extract_utility_shape(
    name: &str,
    type_arguments: &[TypeExpr],
    env: &mut EvalEnv,
    diagnostics: &mut Vec<ExpansionDiagnostic>,
    lookup: &mut dyn EvalLookup,
) -> Option<ExpandedObjectShape> {
    match name {
        "Omit" if type_arguments.len() == 2 => {
            let key_set = extract_key_set_from_type(&type_arguments[1], env, lookup);
            if key_set.is_empty() {
                return None; // fall through to normal evaluation
            }
            let mut shape = extract_shape(&type_arguments[0], env, diagnostics, lookup);
            shape.properties.retain(|p| !key_set.contains(&p.name));
            Some(shape)
        }
        "Pick" if type_arguments.len() == 2 => {
            let key_set = extract_key_set_from_type(&type_arguments[1], env, lookup);
            if key_set.is_empty() {
                return None;
            }
            let mut shape = extract_shape(&type_arguments[0], env, diagnostics, lookup);
            shape.properties.retain(|p| key_set.contains(&p.name));
            shape.index_signatures.clear();
            shape.call_signatures.clear();
            Some(shape)
        }
        "Partial" if type_arguments.len() == 1 => {
            let mut shape = extract_shape(&type_arguments[0], env, diagnostics, lookup);
            for prop in &mut shape.properties {
                prop.optional = true;
            }
            Some(shape)
        }
        "Required" if type_arguments.len() == 1 => {
            let mut shape = extract_shape(&type_arguments[0], env, diagnostics, lookup);
            for prop in &mut shape.properties {
                prop.optional = false;
            }
            Some(shape)
        }
        "Readonly" if type_arguments.len() == 1 => {
            let mut shape = extract_shape(&type_arguments[0], env, diagnostics, lookup);
            for prop in &mut shape.properties {
                prop.readonly = true;
            }
            Some(shape)
        }
        _ => None,
    }
}

/// Extract a set of string literal keys from a type expression.
/// Used for Omit/Pick key extraction at the shape level.
fn extract_key_set_from_type(
    expr: &TypeExpr,
    env: &mut EvalEnv,
    lookup: &mut dyn EvalLookup,
) -> rustc_hash::FxHashSet<String> {
    let mut keys = rustc_hash::FxHashSet::default();
    collect_string_keys(expr, &mut keys, env, lookup);
    keys
}

fn collect_string_keys(
    expr: &TypeExpr,
    keys: &mut rustc_hash::FxHashSet<String>,
    env: &mut EvalEnv,
    lookup: &mut dyn EvalLookup,
) {
    match expr {
        TypeExpr::Literal(LiteralValue::String(s)) => {
            keys.insert(s.clone());
        }
        TypeExpr::Union(types) => {
            for ty in types.iter() {
                collect_string_keys(ty, keys, env, lookup);
            }
        }
        TypeExpr::Ref { .. } => {
            // Evaluate the ref to resolve type aliases like `type Keys = 'a' | 'b'`
            let evaluated = evaluate_with_lookup(expr, env, lookup);
            if &evaluated != expr {
                collect_string_keys(&evaluated, keys, env, lookup);
            }
        }
        TypeExpr::Parenthesized(inner) => {
            collect_string_keys(inner, keys, env, lookup);
        }
        _ => {}
    }
}

/// Convert a `ObjectExpr` (from TypeExpr::Object) to `ExpandedObjectShape`.
fn object_expr_to_shape(
    obj: &ObjectExpr,
    env: &mut EvalEnv,
    diagnostics: &mut Vec<ExpansionDiagnostic>,
    normalize_members: bool,
    lookup: &mut dyn EvalLookup,
) -> ExpandedObjectShape {
    let mut properties = Vec::new();
    let mut index_signatures = Vec::new();
    let mut call_signatures = Vec::new();

    for member in &obj.properties {
        match member {
            ObjectMember::Property(prop) => {
                properties.push(ExpandedProperty {
                    name: prop.name.clone(),
                    ty: normalize_member_type(
                        &prop.ty,
                        env,
                        diagnostics,
                        normalize_members,
                        lookup,
                    ),
                    optional: prop.optional,
                    readonly: prop.readonly,
                });
            }
            ObjectMember::IndexSignature(sig) => {
                index_signatures.push(ExpandedIndexSignature {
                    key_type: normalize_member_type(
                        &sig.key_type,
                        env,
                        diagnostics,
                        normalize_members,
                        lookup,
                    ),
                    value_type: normalize_member_type(
                        &sig.value_type,
                        env,
                        diagnostics,
                        normalize_members,
                        lookup,
                    ),
                    readonly: sig.readonly,
                });
            }
            ObjectMember::CallSignature(sig) | ObjectMember::ConstructSignature(sig) => {
                call_signatures.push(function_expr_to_call_sig(
                    sig,
                    env,
                    diagnostics,
                    normalize_members,
                    lookup,
                ));
            }
            ObjectMember::Method(method) => {
                properties.push(ExpandedProperty {
                    name: method.name.clone(),
                    ty: TypeExpr::Function(std::sync::Arc::new(function_expr_to_type(
                        &method.function,
                        env,
                        diagnostics,
                        normalize_members,
                        lookup,
                    ))),
                    optional: method.optional,
                    readonly: false,
                });
            }
        }
    }

    ExpandedObjectShape {
        properties,
        index_signatures,
        call_signatures,
    }
}

fn normalize_member_type(
    expr: &TypeExpr,
    env: &mut EvalEnv,
    diagnostics: &mut Vec<ExpansionDiagnostic>,
    normalize_members: bool,
    lookup: &mut dyn EvalLookup,
) -> TypeExpr {
    if normalize_members || should_normalize_member_type(expr, env) {
        super::normalized::normalize_expr_with_diagnostics_with_lookup(
            expr,
            env,
            diagnostics,
            lookup,
        )
    } else {
        expr.clone()
    }
}

fn should_normalize_member_type(expr: &TypeExpr, env: &EvalEnv) -> bool {
    match expr {
        TypeExpr::Parenthesized(inner) => should_normalize_member_type(inner, env),
        TypeExpr::Array { element, .. } | TypeExpr::Rest(element) | TypeExpr::KeyOf(element) => {
            should_normalize_member_type(element, env)
        }
        TypeExpr::Tuple { elements, .. } => elements
            .iter()
            .any(|element| should_normalize_member_type(&element.ty, env)),
        TypeExpr::Union(types) | TypeExpr::Intersection(types) => {
            types.iter().any(|ty| should_normalize_member_type(ty, env))
        }
        TypeExpr::Ref {
            name,
            type_arguments,
        } => {
            (type_arguments.is_empty() && env.type_bindings.contains_key(name.as_ref()))
                || (type_arguments.is_empty()
                    && env
                        .type_symbols
                        .get(name.as_ref())
                        .is_some_and(|decl| should_normalize_direct_ref_body(&decl.body, env)))
                || is_builtin_member_utility(name)
                || type_arguments
                    .iter()
                    .any(|arg| should_normalize_member_type(arg, env))
        }
        TypeExpr::Object(obj) => obj.properties.iter().any(|member| match member {
            ObjectMember::Property(prop) => should_normalize_member_type(&prop.ty, env),
            ObjectMember::IndexSignature(sig) => {
                should_normalize_member_type(&sig.key_type, env)
                    || should_normalize_member_type(&sig.value_type, env)
            }
            ObjectMember::CallSignature(func) | ObjectMember::ConstructSignature(func) => {
                should_normalize_function_member(func, env)
            }
            ObjectMember::Method(method) => should_normalize_function_member(&method.function, env),
        }),
        TypeExpr::Function(func) => should_normalize_function_member(func, env),
        TypeExpr::IndexedAccess { object, index } => {
            should_normalize_member_type(object, env)
                || should_normalize_member_type(index, env)
                || should_normalize_indexed_access_member(object, index, env)
        }
        TypeExpr::Conditional {
            check,
            extends,
            true_type,
            false_type,
        } => {
            should_normalize_member_type(check, env)
                || should_normalize_member_type(extends, env)
                || should_normalize_member_type(true_type, env)
                || should_normalize_member_type(false_type, env)
                || matches!(extends.as_ref(), TypeExpr::Infer { .. })
        }
        TypeExpr::Mapped {
            source,
            value,
            name_type,
            ..
        } => {
            should_normalize_member_type(source, env)
                || should_normalize_member_type(value, env)
                || name_type
                    .as_deref()
                    .is_some_and(|ty| should_normalize_member_type(ty, env))
        }
        TypeExpr::TemplateLiteral { expressions, .. } => expressions
            .iter()
            .any(|expr| should_normalize_member_type(expr, env)),
        _ => false,
    }
}

const MEMBER_REF_NORMALIZATION_ALIAS_DEPTH: usize = 4;

fn should_normalize_direct_ref_body(expr: &TypeExpr, env: &EvalEnv) -> bool {
    should_normalize_direct_ref_body_with_depth(expr, env, MEMBER_REF_NORMALIZATION_ALIAS_DEPTH)
}

fn should_normalize_direct_ref_body_with_depth(
    expr: &TypeExpr,
    env: &EvalEnv,
    remaining_depth: usize,
) -> bool {
    match expr {
        TypeExpr::Parenthesized(inner) => {
            should_normalize_direct_ref_body_with_depth(inner, env, remaining_depth)
        }
        TypeExpr::Ref {
            name,
            type_arguments,
        } if type_arguments.is_empty() && remaining_depth > 0 => {
            let next_depth = remaining_depth - 1;
            env.type_bindings.get(name.as_ref()).is_some_and(|bound| {
                should_normalize_direct_ref_body_with_depth(bound, env, next_depth)
            }) || env.type_symbols.get(name.as_ref()).is_some_and(|decl| {
                should_normalize_direct_ref_body_with_depth(&decl.body, env, next_depth)
            })
        }
        TypeExpr::Ref { .. } | TypeExpr::Object(_) | TypeExpr::Function(_) => false,
        TypeExpr::Primitive(_)
        | TypeExpr::Literal(_)
        | TypeExpr::Array { .. }
        | TypeExpr::Tuple { .. }
        | TypeExpr::Union(_)
        | TypeExpr::Intersection(_)
        | TypeExpr::KeyOf(_)
        | TypeExpr::Rest(_)
        | TypeExpr::IndexedAccess { .. }
        | TypeExpr::Conditional { .. }
        | TypeExpr::Mapped { .. }
        | TypeExpr::TemplateLiteral { .. } => true,
        TypeExpr::Unknown { .. }
        | TypeExpr::TypeParameter(_)
        | TypeExpr::TypeOf(_)
        | TypeExpr::Infer { .. } => false,
    }
}

fn should_normalize_indexed_access_member(
    object: &TypeExpr,
    index: &TypeExpr,
    env: &EvalEnv,
) -> bool {
    matches!(
        index,
        TypeExpr::Literal(LiteralValue::String(_)) | TypeExpr::Literal(LiteralValue::Number(_))
    ) && has_direct_structural_index_target(object, env)
}

fn has_direct_structural_index_target(expr: &TypeExpr, env: &EvalEnv) -> bool {
    has_direct_structural_index_target_with_depth(expr, env, MEMBER_REF_NORMALIZATION_ALIAS_DEPTH)
}

fn has_direct_structural_index_target_with_depth(
    expr: &TypeExpr,
    env: &EvalEnv,
    remaining_depth: usize,
) -> bool {
    match expr {
        TypeExpr::Parenthesized(inner) => {
            has_direct_structural_index_target_with_depth(inner, env, remaining_depth)
        }
        TypeExpr::Object(_) | TypeExpr::Intersection(_) => true,
        TypeExpr::Ref {
            name,
            type_arguments,
        } if type_arguments.is_empty() && remaining_depth > 0 => {
            let next_depth = remaining_depth - 1;
            env.type_bindings.get(name.as_ref()).is_some_and(|bound| {
                has_direct_structural_index_target_with_depth(bound, env, next_depth)
            }) || env.type_symbols.get(name.as_ref()).is_some_and(|decl| {
                has_direct_structural_index_target_with_depth(&decl.body, env, next_depth)
            })
        }
        _ => false,
    }
}

fn should_normalize_function_member(func: &crate::type_expr::FunctionExpr, env: &EvalEnv) -> bool {
    func.parameters
        .iter()
        .any(|param| should_normalize_member_type(&param.ty, env))
        || func
            .return_type
            .as_deref()
            .is_some_and(|ty| should_normalize_member_type(ty, env))
        || func.type_parameters.iter().any(|param| {
            param
                .constraint
                .as_deref()
                .is_some_and(|ty| should_normalize_member_type(ty, env))
                || param
                    .default
                    .as_deref()
                    .is_some_and(|ty| should_normalize_member_type(ty, env))
        })
}

fn is_builtin_member_utility(name: &str) -> bool {
    matches!(
        name,
        "Partial"
            | "Required"
            | "Readonly"
            | "Pick"
            | "Omit"
            | "Record"
            | "Extract"
            | "Exclude"
            | "NonNullable"
            | "ReturnType"
            | "Parameters"
            | "ConstructorParameters"
            | "InstanceType"
            | "Awaited"
    )
}

fn function_expr_to_type(
    func: &crate::type_expr::FunctionExpr,
    env: &mut EvalEnv,
    diagnostics: &mut Vec<ExpansionDiagnostic>,
    normalize_members: bool,
    lookup: &mut dyn EvalLookup,
) -> crate::type_expr::FunctionExpr {
    if normalize_members {
        normalize_function_expr(func, env, diagnostics, lookup)
    } else {
        func.clone()
    }
}

/// Merge intersection of object shapes.
///
/// Optionality: both must be optional for the result to be optional.
/// Readonly: either is readonly makes the result readonly.
fn merge_intersection_shapes(shapes: Vec<ExpandedObjectShape>) -> ExpandedObjectShape {
    let mut merged_props: Vec<ExpandedProperty> = Vec::new();
    let mut merged_index = Vec::new();
    let mut merged_call = Vec::new();

    for shape in shapes {
        for prop in shape.properties {
            if let Some(existing) = merged_props.iter_mut().find(|p| p.name == prop.name) {
                // Intersection: optional only if BOTH are optional
                existing.optional = existing.optional && prop.optional;
                // Intersection: readonly if EITHER is readonly
                existing.readonly = existing.readonly || prop.readonly;
                // Intersection: intersect property types when they differ
                if existing.ty != prop.ty {
                    existing.ty = TypeExpr::intersection(vec![existing.ty.clone(), prop.ty]);
                }
            } else {
                merged_props.push(prop);
            }
        }
        merged_index.extend(shape.index_signatures);
        merged_call.extend(shape.call_signatures);
    }

    ExpandedObjectShape {
        properties: merged_props,
        index_signatures: merged_index,
        call_signatures: merged_call,
    }
}

/// Merge union of object shapes using Vue props merge semantics.
///
/// - Keys present in ALL variants are required (unless optional in any)
/// - Keys present in only some variants are optional
/// - Property types are unioned across variants
/// - Property order: first appearance across variants
fn merge_union_shapes_vue(shapes: Vec<ExpandedObjectShape>) -> ExpandedObjectShape {
    if shapes.is_empty() {
        return ExpandedObjectShape::empty();
    }
    if shapes.len() == 1 {
        return shapes.into_iter().next().unwrap();
    }

    let variant_count = shapes.len();

    struct PropState {
        present_in: usize,
        optional_in_any: bool,
        types: Vec<TypeExpr>,
        readonly: bool,
    }

    let mut order: Vec<String> = Vec::new();
    let mut states: FxHashMap<String, PropState> = FxHashMap::default();

    for shape in &shapes {
        let mut seen_in_variant: FxHashSet<String> = FxHashSet::default();
        for prop in &shape.properties {
            let state = states.entry(prop.name.clone()).or_insert_with(|| {
                order.push(prop.name.clone());
                PropState {
                    present_in: 0,
                    optional_in_any: false,
                    types: Vec::new(),
                    readonly: true, // AND semantics: start true, any non-readonly makes false
                }
            });

            if seen_in_variant.insert(prop.name.clone()) {
                state.present_in += 1;
            }
            state.optional_in_any |= prop.optional;
            // Union: readonly only if ALL variants mark it readonly
            state.readonly &= prop.readonly;
            if !state.types.iter().any(|t| t == &prop.ty) {
                state.types.push(prop.ty.clone());
            }
        }
    }

    let properties = order
        .into_iter()
        .filter_map(|name| {
            let state = states.remove(&name)?;
            let optional = state.present_in < variant_count || state.optional_in_any;
            let ty = TypeExpr::union(state.types);
            Some(ExpandedProperty {
                name,
                ty,
                optional,
                readonly: state.readonly,
            })
        })
        .collect();

    let mut index_signatures = Vec::new();
    let mut call_signatures = Vec::new();
    for shape in shapes {
        index_signatures.extend(shape.index_signatures);
        call_signatures.extend(shape.call_signatures);
    }

    ExpandedObjectShape {
        properties,
        index_signatures,
        call_signatures,
    }
}

/// Convert a `FunctionExpr` to an `ExpandedCallSignature`.
fn function_expr_to_call_sig(
    sig: &crate::type_expr::FunctionExpr,
    env: &mut EvalEnv,
    diagnostics: &mut Vec<ExpansionDiagnostic>,
    normalize_members: bool,
    lookup: &mut dyn EvalLookup,
) -> super::request::ExpandedCallSignature {
    use crate::type_expr::PrimitiveName;
    let normalized = function_expr_to_type(sig, env, diagnostics, normalize_members, lookup);
    super::request::ExpandedCallSignature {
        parameters: normalized
            .parameters
            .iter()
            .map(|p| super::request::ExpandedParameter {
                name: p.name.clone().unwrap_or_default(),
                ty: p.ty.clone(),
                optional: p.optional,
                rest: p.rest,
            })
            .collect(),
        return_type: normalized
            .return_type
            .as_deref()
            .cloned()
            .unwrap_or(TypeExpr::Primitive(PrimitiveName::Void)),
        type_parameters: normalized.type_parameters.clone(),
    }
}

fn normalize_function_expr(
    sig: &crate::type_expr::FunctionExpr,
    env: &mut EvalEnv,
    diagnostics: &mut Vec<ExpansionDiagnostic>,
    lookup: &mut dyn EvalLookup,
) -> crate::type_expr::FunctionExpr {
    crate::type_expr::FunctionExpr {
        parameters: sig
            .parameters
            .iter()
            .map(|param| crate::type_expr::FunctionParam {
                name: param.name.clone(),
                ty: super::normalized::normalize_expr_with_diagnostics_with_lookup(
                    &param.ty,
                    env,
                    diagnostics,
                    lookup,
                ),
                optional: param.optional,
                rest: param.rest,
            })
            .collect(),
        return_type: sig.return_type.as_ref().map(|ret| {
            std::sync::Arc::new(
                super::normalized::normalize_expr_with_diagnostics_with_lookup(
                    ret,
                    env,
                    diagnostics,
                    lookup,
                ),
            )
        }),
        type_parameters: sig.type_parameters.clone(),
    }
}

/// Check if a mapped type source represents an infinite key space.
fn is_infinite_source(source: &TypeExpr) -> bool {
    matches!(
        source,
        TypeExpr::Primitive(crate::type_expr::PrimitiveName::String)
            | TypeExpr::Primitive(crate::type_expr::PrimitiveName::Number)
            | TypeExpr::Primitive(crate::type_expr::PrimitiveName::Symbol)
    )
}
