//! Extends-clause `infer` binder-name collection for the structural
//! macro-argument producer: the recursive scan that discovers every
//! `infer X` binder declared in a conditional's `extends` pattern so the
//! seed frames can pre-declare them.

use std::sync::Arc;

use verter_type_expr::{FunctionExpr, ObjectMember, TypeExpr};

/// Collect the `infer` binder names introduced by a conditional's `extends`
/// clause (at any structural depth) into `out`, so the conditional's TRUE
/// branch can bind each to the matching `Infer` carrier. Purely syntactic
/// typed-IR walk — no resolution, allocation-free except the de-duplicated
/// name push.
///
/// This descends EVERY `TypeExpr` child position where an `infer` is
/// syntactically valid inside an `extends` clause, reaching at least the
/// composite coverage of the eager binder
/// `ProjectSemanticDispatch::collect_infer_bindings_into_env` (`Function` /
/// `Object` param/return/member positions) so the dormant carrier binds the
/// same names the eager path would: `Function` / `ConstructorType`
/// (parameters, return, own type-parameter constraint/default), `Object`
/// (property values, index-signature key/value, call/construct/method
/// signatures), `TemplateLiteral` interpolations, `Ref` / `ImportType` type
/// arguments, `TypeOf` instantiation arguments, and `Mapped` source / value /
/// `as`-remap name-type.
///
/// ONE deliberate non-descent: it does NOT recurse into a nested
/// `Conditional`, because an `infer` in an inner conditional's `extends` is
/// scoped to THAT conditional's true branch, not this one's — matching the
/// eager binder, which likewise has no `Conditional` arm.
pub(super) fn collect_extends_infer_binder_names(expr: &TypeExpr, out: &mut Vec<Arc<str>>) {
    match expr {
        TypeExpr::Infer { name } => {
            if !out.iter().any(|n| n.as_ref() == name.as_str()) {
                out.push(Arc::from(name.as_str()));
            }
        }
        TypeExpr::Parenthesized(inner) | TypeExpr::Rest(inner) | TypeExpr::KeyOf(inner) => {
            collect_extends_infer_binder_names(inner, out)
        }
        TypeExpr::Array { element, .. } => collect_extends_infer_binder_names(element, out),
        TypeExpr::Union(arms) | TypeExpr::Intersection(arms) => arms
            .iter()
            .for_each(|a| collect_extends_infer_binder_names(a, out)),
        TypeExpr::Tuple { elements, .. } => elements
            .iter()
            .for_each(|e| collect_extends_infer_binder_names(&e.ty, out)),
        TypeExpr::Ref { type_arguments, .. } | TypeExpr::ImportType { type_arguments, .. } => {
            type_arguments
                .iter()
                .for_each(|a| collect_extends_infer_binder_names(a, out))
        }
        TypeExpr::IndexedAccess { object, index } => {
            collect_extends_infer_binder_names(object, out);
            collect_extends_infer_binder_names(index, out);
        }
        TypeExpr::Function(func) | TypeExpr::ConstructorType(func) => {
            collect_function_infer_binder_names(func, out)
        }
        TypeExpr::Object(obj) => {
            for member in &obj.properties {
                match member {
                    ObjectMember::Property(prop) => {
                        collect_extends_infer_binder_names(&prop.ty, out)
                    }
                    ObjectMember::IndexSignature(sig) => {
                        collect_extends_infer_binder_names(&sig.key_type, out);
                        collect_extends_infer_binder_names(&sig.value_type, out);
                    }
                    ObjectMember::CallSignature(func) | ObjectMember::ConstructSignature(func) => {
                        collect_function_infer_binder_names(func, out)
                    }
                    ObjectMember::Method(method) => {
                        collect_function_infer_binder_names(&method.function, out)
                    }
                    ObjectMember::Spread(spread) => {
                        collect_extends_infer_binder_names(&spread.ty, out)
                    }
                }
            }
        }
        TypeExpr::TemplateLiteral { expressions, .. } => expressions
            .iter()
            .for_each(|e| collect_extends_infer_binder_names(e, out)),
        TypeExpr::TypeOf(value_ref) => value_ref
            .type_args
            .iter()
            .for_each(|a| collect_extends_infer_binder_names(a, out)),
        TypeExpr::Mapped {
            source,
            value,
            name_type,
            ..
        } => {
            collect_extends_infer_binder_names(source, out);
            collect_extends_infer_binder_names(value, out);
            if let Some(name_type) = name_type {
                collect_extends_infer_binder_names(name_type, out);
            }
        }
        _ => {}
    }
}

fn collect_function_infer_binder_names(func: &FunctionExpr, out: &mut Vec<Arc<str>>) {
    for param in &func.parameters {
        collect_extends_infer_binder_names(&param.ty, out);
    }
    if let Some(return_type) = &func.return_type {
        collect_extends_infer_binder_names(return_type, out);
    }
    for tp in &func.type_parameters {
        if let Some(constraint) = &tp.constraint {
            collect_extends_infer_binder_names(constraint, out);
        }
        if let Some(default) = &tp.default {
            collect_extends_infer_binder_names(default, out);
        }
    }
}
