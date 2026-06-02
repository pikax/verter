//! `TypeExpr` reference-name collector (the surviving remnant of the
//! retired macro-shape materialiser).
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
        // A function type and a bare constructor type (`new (...) => R`) carry
        // the same `FunctionExpr` payload; both contribute their parameter and
        // return Refs to the cross-file dependency closure.
        TypeExpr::Function(func) | TypeExpr::ConstructorType(func) => {
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

#[cfg(test)]
mod tests {
    use super::collect_type_expr_ref_names;
    use std::sync::Arc;
    use verter_type_expr::{FunctionExpr, FunctionParam, TypeExpr};

    /// A constructor type's parameter and return Refs must be collected
    /// exactly like a function type's — the registry-name closure in
    /// `component_meta_methods` relies on this to track cross-file
    /// dependencies reachable through a `new (...) => R` shape. Discriminating:
    /// before `ConstructorType` joined the `Function` arm, the value fell to
    /// the wildcard `_ => {}` and BOTH refs were silently dropped.
    #[test]
    fn constructor_type_param_and_return_refs_are_collected() {
        // `new (a: ParamRef) => ReturnRef`
        let ctor = TypeExpr::ConstructorType(Arc::new(FunctionExpr::synthetic(
            vec![FunctionParam::synthetic(
                Some("a".to_string()),
                TypeExpr::named("ParamRef"),
                false,
                false,
            )],
            Some(Arc::new(TypeExpr::named("ReturnRef"))),
            Vec::new(),
        )));
        let mut out = rustc_hash::FxHashSet::default();
        collect_type_expr_ref_names(&ctor, &mut out);
        assert!(
            out.contains("ParamRef"),
            "constructor-type parameter Ref must be collected, got {out:?}",
        );
        assert!(
            out.contains("ReturnRef"),
            "constructor-type return Ref must be collected, got {out:?}",
        );
    }

    /// Negative control: a function type with the same payload collects the
    /// same refs (pins the parity the constructor arm must match).
    #[test]
    fn function_type_param_and_return_refs_are_collected() {
        let function = TypeExpr::Function(Arc::new(FunctionExpr::synthetic(
            vec![FunctionParam::synthetic(
                Some("a".to_string()),
                TypeExpr::named("ParamRef"),
                false,
                false,
            )],
            Some(Arc::new(TypeExpr::named("ReturnRef"))),
            Vec::new(),
        )));
        let mut out = rustc_hash::FxHashSet::default();
        collect_type_expr_ref_names(&function, &mut out);
        assert!(out.contains("ParamRef"));
        assert!(out.contains("ReturnRef"));
    }
}
