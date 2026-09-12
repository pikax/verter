use super::*;
use crate::semantic_query::{ConditionalPendingSubstitution, DeclIdentity};

fn parameter(store: &SemanticGraphStore, name: &str, ordinal: u16) -> SemanticNodeId {
    store.intern_node(SemanticNodeData::TypeParam {
        decl: DeclIdentity::synthetic("<display>"),
        param_index: ordinal,
        constraint: None,
        default: None,
        display_name: Arc::from(name),
    })
}

fn conditional(
    store: &SemanticGraphStore,
    check: SemanticNodeId,
    extends: SemanticNodeId,
    yes: SemanticNodeId,
    no: SemanticNodeId,
    pairs: &[SubstitutionPair],
) -> SemanticNodeId {
    let pending = pairs.iter().fold(
        ConditionalPendingSubstitution::empty(),
        |frame, &(param, arg)| frame.append_both(param, arg),
    );
    store.intern_node(SemanticNodeData::Conditional {
        check,
        extends,
        true_branch_ref: yes,
        false_branch_ref: no,
        distributive: false,
        pending: (!pending.is_empty()).then(|| Arc::new(pending)),
    })
}

fn render(store: &SemanticGraphStore, node: SemanticNodeId) -> String {
    let count = store.node_count();
    let result = display(
        store,
        &SemanticQueryValue::TypeNode(node),
        DisplayNeeds::default(),
    )
    .0;
    assert_eq!(
        store.node_count(),
        count,
        "display never constructs semantic nodes"
    );
    result
}

#[test]
fn pending_display_applies_only_the_replacements_suffix_to_arguments() {
    let store = SemanticGraphStore::new();
    let check = parameter(&store, "Q", 0);
    let t = parameter(&store, "T", 1);
    let u = parameter(&store, "U", 2);
    let number = store.intern_node(SemanticNodeData::Primitive(PrimitiveKind::Number));
    let string = store.intern_node(SemanticNodeData::Primitive(PrimitiveKind::String));
    let array = store.intern_node(SemanticNodeData::Array {
        element: t,
        readonly: false,
    });
    for (pairs, expected) in [
        (
            vec![(t, u), (u, string)],
            "Q extends number ? string : string",
        ),
        (vec![(u, string), (t, u)], "Q extends number ? U : string"),
        (vec![(t, u), (u, t)], "Q extends number ? T : T"),
        (vec![(t, array)], "Q extends number ? T[] : U"),
    ] {
        let node = conditional(&store, check, number, t, u, &pairs);
        assert_eq!(render(&store, node), expected);
    }
}

#[test]
fn pending_display_uses_replacement_shape_for_parentheses() {
    let store = SemanticGraphStore::new();
    let check = parameter(&store, "Q", 0);
    let t = parameter(&store, "T", 1);
    let number = store.intern_node(SemanticNodeData::Primitive(PrimitiveKind::Number));
    let string = store.intern_node(SemanticNodeData::Primitive(PrimitiveKind::String));
    let union = store.intern_node(SemanticNodeData::Union(
        crate::semantic_query::composite::CompositeList::query_subject(Arc::from([string, number])),
    ));
    let array = store.intern_node(SemanticNodeData::Array {
        element: t,
        readonly: false,
    });
    let node = conditional(&store, check, number, array, number, &[(t, union)]);
    assert_eq!(
        render(&store, node),
        "Q extends number ? (string | number)[] : number"
    );
}

#[test]
fn pending_display_composes_inner_frames_before_outer_frames() {
    let store = SemanticGraphStore::new();
    let check = parameter(&store, "Q", 0);
    let t = parameter(&store, "T", 1);
    let u = parameter(&store, "U", 2);
    let number = store.intern_node(SemanticNodeData::Primitive(PrimitiveKind::Number));
    let string = store.intern_node(SemanticNodeData::Primitive(PrimitiveKind::String));
    let inner = conditional(&store, check, number, t, t, &[(t, u)]);
    let outer = conditional(&store, check, number, inner, number, &[(u, string)]);
    assert_eq!(
        render(&store, outer),
        "Q extends number ? (Q extends number ? string : string) : number"
    );
}

#[test]
fn pending_display_preserves_exact_infer_scope_in_nested_conditionals() {
    let store = SemanticGraphStore::new();
    let check = parameter(&store, "Q", 0);
    let number = store.intern_node(SemanticNodeData::Primitive(PrimitiveKind::Number));
    let string = store.intern_node(SemanticNodeData::Primitive(PrimitiveKind::String));
    let binder = store.alloc_infer_binder_id();
    let infer = store.intern_node(SemanticNodeData::Infer {
        name: Arc::from("X"),
        binder: binder.clone(),
    });
    let reference = store.intern_node(SemanticNodeData::InferRef {
        name: Arc::from("X"),
        binder,
    });
    let foreign = store.intern_node(SemanticNodeData::Infer {
        name: Arc::from("X"),
        binder: store.alloc_infer_binder_id(),
    });
    for (extends, expected) in [
        (
            infer,
            "Q extends number ? (string extends infer X ? X : string) : number",
        ),
        (
            foreign,
            "Q extends number ? (string extends infer X ? string : string) : number",
        ),
    ] {
        let inner = conditional(&store, reference, extends, reference, reference, &[]);
        let outer = conditional(&store, check, number, inner, number, &[(infer, string)]);
        assert_eq!(render(&store, outer), expected);
    }
}

#[test]
fn pending_display_preserves_infer_scope_declared_inside_a_carrier_argument() {
    // `T extends Foo<infer X> ? X : never` declares `X` in the pattern just
    // as a bare `infer X` does, so an outer substitution over that binder
    // must not reach either the declaration or the true branch. Before the
    // carrier arguments were walked, both collapsed to the outer argument.
    let store = SemanticGraphStore::new();
    let check = parameter(&store, "Q", 0);
    let number = store.intern_node(SemanticNodeData::Primitive(PrimitiveKind::Number));
    let string = store.intern_node(SemanticNodeData::Primitive(PrimitiveKind::String));
    let binder = store.alloc_infer_binder_id();
    let infer = store.intern_node(SemanticNodeData::Infer {
        name: Arc::from("X"),
        binder: binder.clone(),
    });
    let reference = store.intern_node(SemanticNodeData::InferRef {
        name: Arc::from("X"),
        binder,
    });
    for (carrier, expected) in [
        (
            SemanticNodeData::new_bare_ref(
                Arc::from("Foo"),
                crate::semantic_query::NodeScopeId::Global,
                Arc::from(vec![infer].into_boxed_slice()),
            ),
            "Q extends number ? (string extends Foo<infer X> ? X : string) : number",
        ),
        (
            SemanticNodeData::new_import_type(
                Arc::from("m"),
                Arc::from(vec![Arc::<str>::from("G")].into_boxed_slice()),
                Arc::from(vec![infer].into_boxed_slice()),
                false,
            ),
            "Q extends number ? (string extends import(\"m\").G<infer X> ? X : string) : number",
        ),
    ] {
        let extends = store.intern_node(carrier);
        let inner = conditional(&store, reference, extends, reference, reference, &[]);
        let outer = conditional(&store, check, number, inner, number, &[(infer, string)]);
        assert_eq!(render(&store, outer), expected);
    }
}

#[test]
fn pending_display_preserves_a_mapped_binder_and_its_value_scope() {
    let store = SemanticGraphStore::new();
    let check = parameter(&store, "Q", 0);
    let key = parameter(&store, "K", 1);
    let number = store.intern_node(SemanticNodeData::Primitive(PrimitiveKind::Number));
    let string = store.intern_node(SemanticNodeData::Primitive(PrimitiveKind::String));
    let mapped = store.intern_node(SemanticNodeData::Mapped {
        source: key,
        mapper: super::super::MapperKey {
            parameter_node: key,
            key_space: key,
            value_expr: key,
            optionality: OptionalityMod::Keep,
            readonly: ReadonlyMod::Keep,
            name_remap: Some(key),
            kind: super::super::MapperKind::Computed,
        },
    });
    let node = conditional(&store, check, number, mapped, number, &[(key, string)]);
    assert_eq!(
        render(&store, node),
        "Q extends number ? { [K in string as K]: K } : number"
    );
}
