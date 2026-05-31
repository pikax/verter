//! Type-reference name collection for materialised macro shapes.
//!
//! The macro-object materialiser (`produce_macro_object_shapes` and its
//! `synthesize_define_*` / `produce_one_*` / `project_named_ref_*` helper
//! cluster) has been retired — `define_*` macro shapes are produced by the
//! dispatch projectors (`crate::meta_resolve::projectors::define_shapes`),
//! and the `.vue` macro DTO surface is resolved directly through
//! `crate::typeinfo::adapters::vue::surface::vue_macro_dtos_with_ctx`.
//!
//! The single surviving helper collects the set of `Ref` names reachable
//! through a `TypeExpr`, used by the host-side registry-name closure in
//! `component_meta_methods`.

/// Collect every `TypeExpr::Ref` name reachable through `expr` into `out`,
/// recursing through objects, arrays, tuples, unions/intersections,
/// indexed-access, parentheses, and function parameter/return types.
pub(crate) fn collect_type_expr_ref_names(
    expr: &verter_type_expr::TypeExpr,
    out: &mut rustc_hash::FxHashSet<String>,
) {
    use verter_type_expr::{ObjectMember, TypeExpr};
    match expr {
        TypeExpr::Ref {
            name,
            type_arguments,
            ..
        } => {
            out.insert(name.to_string());
            for arg in type_arguments.iter() {
                collect_type_expr_ref_names(arg, out);
            }
        }
        TypeExpr::Object(obj) => {
            for member in &obj.properties {
                match member {
                    ObjectMember::Property(prop) => collect_type_expr_ref_names(&prop.ty, out),
                    ObjectMember::IndexSignature(sig) => {
                        collect_type_expr_ref_names(&sig.key_type, out);
                        collect_type_expr_ref_names(&sig.value_type, out);
                    }
                    ObjectMember::CallSignature(func) | ObjectMember::ConstructSignature(func) => {
                        for param in &func.parameters {
                            collect_type_expr_ref_names(&param.ty, out);
                        }
                        if let Some(ret) = &func.return_type {
                            collect_type_expr_ref_names(ret, out);
                        }
                    }
                    ObjectMember::Method(method) => {
                        for param in &method.function.parameters {
                            collect_type_expr_ref_names(&param.ty, out);
                        }
                        if let Some(ret) = &method.function.return_type {
                            collect_type_expr_ref_names(ret, out);
                        }
                    }
                }
            }
        }
        TypeExpr::Array { element, .. } => collect_type_expr_ref_names(element, out),
        TypeExpr::Tuple { elements, .. } => {
            for el in elements.iter() {
                collect_type_expr_ref_names(&el.ty, out);
            }
        }
        TypeExpr::Union(types) | TypeExpr::Intersection(types) => {
            for ty in types.iter() {
                collect_type_expr_ref_names(ty, out);
            }
        }
        TypeExpr::IndexedAccess { object, index } => {
            collect_type_expr_ref_names(object, out);
            collect_type_expr_ref_names(index, out);
        }
        TypeExpr::Parenthesized(inner) => collect_type_expr_ref_names(inner, out),
        TypeExpr::Function(func) => {
            for param in &func.parameters {
                collect_type_expr_ref_names(&param.ty, out);
            }
            if let Some(ret) = &func.return_type {
                collect_type_expr_ref_names(ret, out);
            }
        }
        _ => {}
    }
}
