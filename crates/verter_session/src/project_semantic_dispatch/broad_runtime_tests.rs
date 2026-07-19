use std::sync::Arc;

use super::ProjectSemanticDispatch;
use crate::semantic_query::{
    BroadRuntimeClassification, BroadRuntimeKind, DeclIdentity, HashValue, LiteralValue,
    PrimitiveKind, QueryResult, SemanticNodeData, SemanticQueryApi, SemanticQueryKey,
    SemanticQueryValue, SurfaceMember, SurfaceView,
};
use crate::{FileLanguage, UpsertRequest, VerterHost};

fn upsert_ts(host: &VerterHost, id: &str, source: &str) {
    let _ = host
        .upsert(UpsertRequest {
            canonical_id: None,
            input_id: id.to_owned(),
            source: Arc::from(source),
            file_language: FileLanguage::script_ts(),
            aliases: Vec::new(),
        })
        .expect("fixture must index");
}

fn instantiated_alias(
    dispatch: &ProjectSemanticDispatch<'_>,
    canonical: &str,
    name: &str,
) -> crate::semantic_query::SemanticNodeId {
    let slot = dispatch.type_slot_for(Arc::from(canonical), Arc::from(name));
    let key = crate::semantic_query::InstantiateKey::new(
        slot,
        Arc::from([]),
        dispatch.instantiate_context_for(
            canonical,
            crate::semantic_query::ProjectionReductionContext::structural_transit_with_mode(
                crate::semantic_query::ProjectionMode::Navigate,
            ),
        ),
    );
    match dispatch.execute_type_node(SemanticQueryKey::Instantiate(key)) {
        QueryResult::Value(output) => output.value,
        other => panic!("expected instantiated fixture alias, got {other:?}"),
    }
}

fn classify(
    dispatch: &ProjectSemanticDispatch<'_>,
    subject: crate::semantic_query::SemanticNodeId,
) -> BroadRuntimeClassification {
    let result = dispatch.execute(SemanticQueryKey::ClassifyBroadRuntime {
        subject,
        context: dispatch.broad_runtime_context_for(subject),
    });
    match result {
        QueryResult::Value(output) => match output.value {
            SemanticQueryValue::BroadRuntime(value) => value,
            other => panic!("expected broad-runtime value, got {other:?}"),
        },
        other => panic!("expected classifier value, got {other:?}"),
    }
}

#[test]
fn broad_runtime_preserves_union_order_and_first_occurrence_dedup() {
    let host = VerterHost::new_standalone(Default::default());
    let graph = host.project_type_store().semantic_graph();
    let string = graph.intern_node(SemanticNodeData::Primitive(PrimitiveKind::String));
    let number = graph.intern_node(SemanticNodeData::Literal(LiteralValue::Number(42.0)));
    let bigint_literal = graph.intern_node(SemanticNodeData::Literal(LiteralValue::BigInt(
        "1".to_owned(),
    )));
    let unknown = graph.intern_node(SemanticNodeData::Primitive(PrimitiveKind::BigInt));
    let subject = graph.intern_node(SemanticNodeData::Union(Arc::from([
        string,
        number,
        string,
        bigint_literal,
        unknown,
    ])));

    let value = classify(&ProjectSemanticDispatch::new(&host), subject);

    assert_eq!(
        value.kinds(),
        &[
            BroadRuntimeKind::String,
            BroadRuntimeKind::Number,
            BroadRuntimeKind::Unknown,
        ]
    );
}

#[test]
fn broad_runtime_classifies_container_callable_and_object_without_member_descent() {
    let host = VerterHost::new_standalone(Default::default());
    let graph = host.project_type_store().semantic_graph();
    let leaf = graph.intern_node(SemanticNodeData::Primitive(PrimitiveKind::String));
    let array = graph.intern_node(SemanticNodeData::Array {
        element: leaf,
        readonly: false,
    });
    let callable = graph.intern_node(SemanticNodeData::Function {
        params: Arc::from([]),
        return_type: leaf,
        type_parameters: Arc::from([]),
        signature_span: None,
        return_type_span: None,
    });
    let object = graph.intern_node(SemanticNodeData::Object(SurfaceView {
        members: Arc::from([SurfaceMember {
            name: Arc::from("value"),
            value: crate::semantic_query::SemanticNodeId(u64::MAX),
            optional: false,
            readonly: false,
            is_method: false,
            visibility: verter_type_expr::MemberVisibility::Public,
            spans: Default::default(),
            declaration_origin: None,
            declared_in_macro_type_arg: crate::semantic_query::MacroOwnBodyStamp::NEUTRAL,
            merge_role: crate::semantic_query::MergeRoleStamp::NEUTRAL,
        }]),
        call_signatures: Arc::from([]),
        construct_signatures: Arc::from([crate::semantic_query::SemanticNodeId(u64::MAX - 1)]),
        index_signatures: Arc::from([]),
        keyspace: Some(array),
        has_index_signature: true,
    }));
    assert_eq!(
        classify(&ProjectSemanticDispatch::new(&host), object).kinds(),
        &[BroadRuntimeKind::Function, BroadRuntimeKind::Object],
        "signature metadata classifies the object without reading member or signature bodies"
    );
    let subject = graph.intern_node(SemanticNodeData::Union(Arc::from([
        array, callable, object,
    ])));

    let value = classify(&ProjectSemanticDispatch::new(&host), subject);

    assert_eq!(
        value.kinds(),
        &[
            BroadRuntimeKind::Array,
            BroadRuntimeKind::Function,
            BroadRuntimeKind::Object,
        ]
    );
}

#[test]
fn broad_runtime_keeps_null_and_unsupported_undefined_distinct() {
    let host = VerterHost::new_standalone(Default::default());
    let graph = host.project_type_store().semantic_graph();
    let null = graph.intern_node(SemanticNodeData::Primitive(PrimitiveKind::Null));
    let undefined = graph.intern_node(SemanticNodeData::Primitive(PrimitiveKind::Undefined));
    let subject = graph.intern_node(SemanticNodeData::Union(Arc::from([null, undefined])));

    assert_eq!(
        classify(&ProjectSemanticDispatch::new(&host), subject).kinds(),
        &[BroadRuntimeKind::Null, BroadRuntimeKind::Unknown]
    );
}

#[test]
fn broad_runtime_recognizes_only_shadow_safe_builtin_nominal_identities() {
    let host = VerterHost::new_standalone(Default::default());
    let graph = host.project_type_store().semantic_graph();
    let builtin = |name: &'static str| {
        graph.intern_node(SemanticNodeData::DeclRef {
            identity: DeclIdentity {
                canonical_id: Arc::from("__builtin__"),
                whole_hash: HashValue::default(),
                decl_name: Arc::from(name),
            },
        })
    };
    for (name, expected) in [
        ("Date", BroadRuntimeKind::Date),
        ("Map", BroadRuntimeKind::Map),
        ("Set", BroadRuntimeKind::Set),
        ("WeakMap", BroadRuntimeKind::WeakMap),
        ("WeakSet", BroadRuntimeKind::WeakSet),
        ("Promise", BroadRuntimeKind::Promise),
        ("Error", BroadRuntimeKind::Error),
    ] {
        assert_eq!(
            classify(&ProjectSemanticDispatch::new(&host), builtin(name)).kinds(),
            &[expected],
            "{name}"
        );
    }
    for name in ["Date", "WeakMap", "WeakSet"] {
        let user_nominal = graph.intern_node(SemanticNodeData::DeclRef {
            identity: DeclIdentity {
                canonical_id: Arc::from(format!("/src/{name}.ts")),
                whole_hash: HashValue::default(),
                decl_name: Arc::from(name),
            },
        });
        assert_eq!(
            classify(&ProjectSemanticDispatch::new(&host), user_nominal).kinds(),
            &[BroadRuntimeKind::Unknown],
            "a user declaration named {name} is not the global nominal"
        );
    }
}

#[test]
fn rootless_classifier_values_are_return_only() {
    let host = VerterHost::new_standalone(Default::default());
    let graph = host.project_type_store().semantic_graph();
    let subject = graph.intern_node(SemanticNodeData::Primitive(PrimitiveKind::Boolean));
    let dispatch = ProjectSemanticDispatch::new(&host);
    let key = SemanticQueryKey::ClassifyBroadRuntime {
        subject,
        context: dispatch.broad_runtime_context_for(subject),
    };

    let first = dispatch.execute(key.clone());
    let second = dispatch.execute(key);

    for result in [first, second] {
        match result {
            QueryResult::Value(output) => assert_eq!(
                output.value,
                SemanticQueryValue::BroadRuntime(BroadRuntimeClassification::new([
                    BroadRuntimeKind::Boolean,
                ]))
            ),
            other => panic!("expected classifier value, got {other:?}"),
        }
    }
    assert_eq!(
        graph.memo_entry_count(),
        0,
        "rootless classifier result must not enter the family memo"
    );
}

#[test]
fn file_rooted_classifier_values_warm_and_reuse_the_typed_family_entry() {
    let host = VerterHost::new_standalone(Default::default());
    upsert_ts(
        &host,
        "/runtime.ts",
        "export type RuntimeValue = string | boolean",
    );
    let dispatch = ProjectSemanticDispatch::new(&host);
    let subject = instantiated_alias(&dispatch, "/runtime.ts", "RuntimeValue");
    let key = SemanticQueryKey::ClassifyBroadRuntime {
        subject,
        context: dispatch.broad_runtime_context_for(subject),
    };

    let first = dispatch.execute(key.clone());
    assert_eq!(
        host.project_type_store()
            .semantic_graph()
            .slot_candidate_count_for_tests(&key),
        1,
        "complete file-rooted classification must warm"
    );
    let second = dispatch.execute(key);

    match (first, second) {
        (QueryResult::Value(first), QueryResult::Value(second)) => {
            assert_eq!(first.value, second.value)
        }
        other => panic!("expected two classifier values, got {other:?}"),
    }
}

#[test]
fn broad_runtime_contexts_do_not_warm_hit() {
    let host = VerterHost::new_standalone(Default::default());
    upsert_ts(&host, "/context.ts", "export type RuntimeValue = string");
    let dispatch = ProjectSemanticDispatch::new(&host);
    let subject = instantiated_alias(&dispatch, "/context.ts", "RuntimeValue");
    let context = dispatch.broad_runtime_context_for(subject);
    let mut alternate_context = context;
    alternate_context.resolve_env_hash[0] ^= u8::MAX;
    let primary = SemanticQueryKey::ClassifyBroadRuntime { subject, context };
    let alternate = SemanticQueryKey::ClassifyBroadRuntime {
        subject,
        context: alternate_context,
    };

    let _ = dispatch.execute(primary.clone());
    let graph = host.project_type_store().semantic_graph();
    assert_eq!(graph.slot_candidate_count_for_tests(&primary), 1);
    assert_eq!(graph.slot_candidate_count_for_tests(&alternate), 0);

    let _ = dispatch.execute(alternate.clone());
    assert_eq!(graph.slot_candidate_count_for_tests(&alternate), 1);
}

#[test]
fn partial_classifier_values_never_admit_and_retry_cold() {
    let host = VerterHost::new_standalone(Default::default());
    upsert_ts(&host, "/partial.ts", "export type RuntimeValue = string");
    let dispatch = ProjectSemanticDispatch::new(&host);
    let subject = instantiated_alias(&dispatch, "/partial.ts", "RuntimeValue");
    let key = SemanticQueryKey::ClassifyBroadRuntime {
        subject,
        context: dispatch.broad_runtime_context_for(subject),
    };

    host.test_force
        .force_result_partial_for_tests
        .store(true, std::sync::atomic::Ordering::Relaxed);
    let _ = dispatch.execute(key.clone());
    let _ = dispatch.execute(key.clone());
    assert_eq!(
        host.project_type_store()
            .semantic_graph()
            .slot_candidate_count_for_tests(&key),
        0,
        "partial classification must never warm"
    );

    host.test_force
        .force_result_partial_for_tests
        .store(false, std::sync::atomic::Ordering::Relaxed);
    let _ = dispatch.execute(key.clone());
    assert_eq!(
        host.project_type_store()
            .semantic_graph()
            .slot_candidate_count_for_tests(&key),
        1,
        "the first complete retry must warm"
    );
}
