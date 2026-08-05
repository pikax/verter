//! @ai-generated - the referenced-name walk is EXHAUSTIVE and depth-safe.
//!
//! `referenced_names` is the shared authority a consumer uses to decide
//! whether a lowered type binds symbols from the scope it was lowered in.
//! A name reached through ANY nesting must surface, or that consumer
//! silently resolves the name in the wrong scope.

use std::sync::Arc;

use verter_type_expr::{
    referenced_names, FunctionExpr, FunctionParam, ObjectExpr, ObjectMember, ObjectProperty,
    PrimitiveName, TypeExpr, ValueRef,
};

fn type_of(root: &str) -> TypeExpr {
    TypeExpr::TypeOf(ValueRef {
        path: vec![root.to_string(), "member".to_string()],
        type_args: Vec::new(),
    })
}

fn named(name: &str) -> TypeExpr {
    TypeExpr::Ref {
        name: Arc::from(name),
        type_arguments: Arc::from(Vec::new().into_boxed_slice()),
    }
}

#[test]
fn referenced_names_reaches_every_nested_carrier() {
    let function = TypeExpr::Function(Arc::new(FunctionExpr::synthetic(
        vec![FunctionParam::synthetic(
            Some("p".to_string()),
            type_of("inParam"),
            false,
            false,
        )],
        Some(Arc::new(type_of("inReturn"))),
        Vec::new(),
    )));
    let object = TypeExpr::Object(Arc::new(ObjectExpr {
        properties: vec![ObjectMember::Property(
            ObjectProperty::synthetic_public_key("k".into(), type_of("inMember"), false, false),
        )],
    }));
    let subject = TypeExpr::union(vec![
        TypeExpr::Array {
            element: Arc::new(type_of("inArray")),
            readonly: false,
        },
        object,
        function,
        TypeExpr::Conditional {
            check: Arc::new(named("InCheck")),
            extends: Arc::new(TypeExpr::Primitive(PrimitiveName::String)),
            true_type: Arc::new(type_of("inTrue")),
            false_type: Arc::new(TypeExpr::Primitive(PrimitiveName::Never)),
        },
        TypeExpr::Ref {
            name: Arc::from("Outer"),
            type_arguments: Arc::from(vec![type_of("inTypeArgument")].into_boxed_slice()),
        },
    ]);

    let names = referenced_names(&subject);
    let mut value_roots = names.value_roots.clone();
    value_roots.sort();
    assert_eq!(
        value_roots,
        vec![
            "inArray".to_string(),
            "inMember".to_string(),
            "inParam".to_string(),
            "inReturn".to_string(),
            "inTrue".to_string(),
            "inTypeArgument".to_string(),
        ],
        "every nested `typeof` root surfaces"
    );
    let mut type_names = names.type_names.clone();
    type_names.sort();
    assert_eq!(
        type_names,
        vec!["InCheck".to_string(), "Outer".to_string()],
        "every named type reference surfaces"
    );
}

#[test]
fn referenced_names_takes_the_path_root_not_the_whole_path() {
    let names = referenced_names(&type_of("root"));
    assert_eq!(names.value_roots, vec!["root".to_string()]);
    assert!(
        names.type_names.is_empty(),
        "a `typeof` path names no type: {:?}",
        names.type_names
    );
}

#[test]
fn referenced_names_takes_the_type_reference_head_not_the_dotted_name() {
    // `E.M` / `A.B.C` resolve their LEFTMOST segment as a binding; the
    // remaining segments are member selections inside whatever that
    // binding denotes. A consumer asking "does this frame bind any name
    // this answer references" compares against binding names, so pushing
    // the dotted string makes every qualified reference miss — the answer
    // silently keeps the scope it was lowered in.
    let dotted = TypeExpr::union(vec![
        TypeExpr::Ref {
            name: Arc::from("E.M"),
            type_arguments: Arc::from(Vec::new().into_boxed_slice()),
        },
        TypeExpr::Ref {
            name: Arc::from("A.B.C"),
            type_arguments: Arc::from(Vec::new().into_boxed_slice()),
        },
        TypeExpr::RecursiveRef {
            name: Arc::from("R.Inner"),
            type_arguments: Arc::from(Vec::new().into_boxed_slice()),
            conditional_context: Arc::from(Vec::new().into_boxed_slice()),
        },
    ]);

    let mut type_names = referenced_names(&dotted).type_names;
    type_names.sort();
    assert_eq!(
        type_names,
        vec!["A".to_string(), "E".to_string(), "R".to_string()],
        "a qualified type reference names its head binding, not the dotted path"
    );
    assert!(
        !type_names.iter().any(|name| name.contains('.')),
        "no dotted name survives into `type_names`: {type_names:?}"
    );
}

#[test]
fn referenced_names_leaves_an_undotted_reference_head_intact() {
    // The head split must not perturb the common case.
    let names = referenced_names(&TypeExpr::union(vec![
        named("Info"),
        TypeExpr::recursive_ref("Rec", Vec::new()),
    ]));
    let mut type_names = names.type_names;
    type_names.sort();
    assert_eq!(type_names, vec!["Info".to_string(), "Rec".to_string()]);
}

#[test]
fn referenced_names_finds_nothing_in_a_name_free_type() {
    let names = referenced_names(&TypeExpr::union(vec![
        TypeExpr::number_literal(1.0),
        TypeExpr::Primitive(PrimitiveName::String),
    ]));
    assert!(names.value_roots.is_empty());
    assert!(names.type_names.is_empty());
}

#[test]
fn referenced_names_is_depth_safe() {
    // The walk runs on an explicit heap work-stack, so a chain far deeper
    // than the thread stack tolerates must still complete.
    let mut deep = type_of("deepRoot");
    for _ in 0..50_000 {
        deep = TypeExpr::Array {
            element: Arc::new(deep),
            readonly: false,
        };
    }
    assert_eq!(
        referenced_names(&deep).value_roots,
        vec!["deepRoot".to_string()]
    );
}
