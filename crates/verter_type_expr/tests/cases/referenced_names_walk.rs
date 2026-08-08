//! @ai-generated - the referenced-name walk is EXHAUSTIVE and depth-safe.
//!
//! `referenced_names` is the shared authority a consumer uses to decide
//! whether a lowered type binds symbols from the scope it was lowered in.
//! A name reached through ANY nesting must surface, or that consumer
//! silently resolves the name in the wrong scope.

use std::sync::Arc;

use verter_type_expr::{
    referenced_names, FunctionExpr, FunctionParam, ObjectExpr, ObjectMember, ObjectProperty,
    PrimitiveName, ReferencedTypeName, TypeExpr, ValueRef,
};

/// One BARE (unqualified) referenced-name occurrence.
fn bare(head: &str) -> ReferencedTypeName {
    ReferencedTypeName {
        head: head.to_string(),
        qualified: false,
    }
}

/// One QUALIFIED (dotted) referenced-name occurrence.
fn qual(head: &str) -> ReferencedTypeName {
    ReferencedTypeName {
        head: head.to_string(),
        qualified: true,
    }
}

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
        vec![bare("InCheck"), bare("Outer")],
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
        vec![qual("A"), qual("E"), qual("R")],
        "a qualified type reference names its head binding, not the dotted path"
    );
    assert!(
        !type_names.iter().any(|name| name.head.contains('.')),
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
    assert_eq!(type_names, vec![bare("Info"), bare("Rec")]);
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

#[test]
fn referenced_names_marks_qualified_occurrences_per_occurrence() {
    // An answer may reference the SAME head both bare and qualified.
    // The two occurrences carry different meanings — a bare `A` is a
    // Type-meaning lookup, a qualified `A.B` head is a Namespace-meaning
    // one — so a per-NAME bit (or a deduplicated set) loses the
    // distinction and makes one of the two consumers wrong.
    let subject = TypeExpr::union(vec![
        named("A"),
        TypeExpr::Ref {
            name: Arc::from("A.B"),
            type_arguments: Arc::from(Vec::new().into_boxed_slice()),
        },
        named("Plain"),
    ]);

    let mut occurrences = referenced_names(&subject).type_names;
    occurrences.sort();
    assert_eq!(
        occurrences,
        vec![bare("A"), qual("A"), bare("Plain"),],
        "each occurrence carries its own qualified bit"
    );
}

#[test]
fn referenced_names_masks_function_and_constructor_binders_depth_safely() {
    use verter_type_expr::TypeParam;

    // A generic function type's OWN type parameters are binders of the
    // ANSWER, not references into the scope the answer was lowered in. A
    // consumer asking "does this frame bind any name this answer
    // references" must never see them, or a frame with a same-named
    // `class T` spuriously fails closed on `<T>(x: T) => T`.
    let generic = |name: &str, body: TypeExpr| {
        TypeExpr::Function(Arc::new(FunctionExpr::synthetic(
            vec![FunctionParam::synthetic(
                Some("p".to_string()),
                named(name),
                false,
                false,
            )],
            Some(Arc::new(body)),
            vec![TypeParam {
                name: name.to_string(),
                constraint: None,
                default: None,
                is_const: false,
            }],
        )))
    };

    let masked = referenced_names(&generic("T", named("T")));
    assert!(
        masked.type_names.is_empty(),
        "a function's own binder is not a scope reference: {:?}",
        masked.type_names
    );

    // A SIBLING of the binder frame stays visible: the mask is scoped to
    // the function's own subtree, not to the whole walk.
    let sibling = TypeExpr::union(vec![generic("T", named("T")), named("T")]);
    assert_eq!(
        referenced_names(&sibling).type_names,
        vec![bare("T")],
        "the binder frame exits with its subtree"
    );

    // A FREE name inside the binder's subtree still surfaces.
    let free_inside = generic("T", named("Info"));
    assert_eq!(
        referenced_names(&free_inside).type_names,
        vec![bare("Info")],
        "masking removes only the binder's own name"
    );

    // A CONSTRUCTOR type's binders mask identically.
    let ctor_source = generic("C", named("C"));
    let TypeExpr::Function(ref ctor_body) = ctor_source else {
        unreachable!("the helper builds a function type")
    };
    assert!(
        referenced_names(&TypeExpr::ConstructorType(Arc::clone(ctor_body)))
            .type_names
            .is_empty(),
        "a constructor type's own binder is masked too"
    );

    // A METHOD / call signature inside an object carries its own frame,
    // and the object's OTHER members are outside it.
    let object = TypeExpr::Object(Arc::new(ObjectExpr {
        properties: vec![
            ObjectMember::Property(ObjectProperty::synthetic_public_key(
                "sibling".into(),
                named("T"),
                false,
                false,
            )),
            ObjectMember::CallSignature(FunctionExpr::synthetic(
                vec![FunctionParam::synthetic(
                    Some("p".to_string()),
                    named("T"),
                    false,
                    false,
                )],
                None,
                vec![TypeParam {
                    name: "T".to_string(),
                    constraint: None,
                    default: None,
                    is_const: false,
                }],
            )),
        ],
    }));
    assert_eq!(
        referenced_names(&object).type_names,
        vec![bare("T")],
        "a call signature's binder masks only inside the signature"
    );

    // Depth-safe: the enter/exit frames ride the same explicit heap
    // work-stack the rest of the walk uses.
    let mut deep = named("Free");
    for _ in 0..20_000 {
        deep = generic("T", deep);
    }
    assert_eq!(
        referenced_names(&deep).type_names,
        vec![bare("Free")],
        "20k nested binder frames complete without a stack overflow"
    );
}
