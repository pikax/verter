use std::sync::Arc;

use super::ProjectSemanticDispatch;
use crate::request_context::{current_cold_compute_completeness, ColdComputeCompletenessScope};
use crate::semantic_query::{
    BroadRuntimeClassification, BroadRuntimeKind, DeclIdentity, HashValue, LiteralValue,
    NodeScopeId, PartialReasonSet, PrimitiveKind, QueryError, QueryResult, SemanticNodeData,
    SemanticNodeId, SemanticQueryApi, SemanticQueryKey, SemanticQueryValue, SurfaceMember,
    SurfaceView,
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

fn upsert_vue(host: &VerterHost, id: &str, source: &str) {
    let _ = host
        .upsert(UpsertRequest {
            canonical_id: Some(id.to_owned()),
            input_id: id.to_owned(),
            source: Arc::from(source),
            file_language: FileLanguage::vue(),
            aliases: Vec::new(),
        })
        .expect("fixture must index");
}

fn classify(
    dispatch: &ProjectSemanticDispatch<'_>,
    subject: crate::semantic_query::SemanticNodeId,
) -> BroadRuntimeClassification {
    match dispatch.classify_broad_runtime_transient(subject).result {
        QueryResult::Value(SemanticQueryValue::BroadRuntime(value)) => value,
        other => panic!("expected classifier value, got {other:?}"),
    }
}

fn macro_classifier_key(
    dispatch: &ProjectSemanticDispatch<'_>,
    canonical: &str,
    member: Option<&str>,
) -> SemanticQueryKey {
    let owner = crate::meta_resolve::projectors::build_owner_decl_identity(
        dispatch.ctx,
        canonical,
        verter_type_expr::TopLevelOwnerId::instance(0),
    );
    let root = dispatch
        .broad_runtime_subject_for_macro(&owner, 0)
        .expect("fixture macro ordinal fits canonical locator");
    let subject = member.map_or(root.clone(), |name| root.member(Arc::from(name)));
    dispatch.broad_runtime_key_for(subject)
}

fn classification_from_output(
    output: &super::walk::QueryBuildOutput<SemanticQueryValue>,
) -> &BroadRuntimeClassification {
    match &output.result {
        QueryResult::Value(SemanticQueryValue::BroadRuntime(value)) => value,
        other => panic!("expected broad-runtime value, got {other:?}"),
    }
}

fn execute_classification(
    dispatch: &ProjectSemanticDispatch<'_>,
    key: SemanticQueryKey,
) -> BroadRuntimeClassification {
    match dispatch.execute(key) {
        QueryResult::Value(output) => match output.value {
            SemanticQueryValue::BroadRuntime(value) => value,
            other => panic!("expected broad-runtime value, got {other:?}"),
        },
        other => panic!("expected classifier value, got {other:?}"),
    }
}

fn file_scope(dispatch: &ProjectSemanticDispatch<'_>, canonical: &str) -> NodeScopeId {
    let shallow = dispatch
        .ctx
        .shallow_file_state(canonical)
        .expect("fixture file must be indexed");
    NodeScopeId::File {
        canonical_id: Arc::from(canonical),
        owner: verter_type_expr::TopLevelOwnerId::ordinary_file(),
        whole_hash: shallow.whole_hash,
        local_scope: None,
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

/// Mutation recipe: recurse into an Object member/signature/keyspace after the
/// terminal Object/Function facts are known. The 4,096 deliberately missing
/// member nodes then taint the read Partial (and make traversal scale with the
/// nested surface), failing the constant-boundary classification assertion.
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
    let explosive_members: Vec<_> = (0_u64..4_096)
        .map(|index| SurfaceMember {
            name: Arc::from(format!("nested{index}")),
            value: crate::semantic_query::SemanticNodeId(u64::MAX - index),
            optional: false,
            readonly: false,
            is_method: false,
            visibility: verter_type_expr::MemberVisibility::Public,
            spans: Default::default(),
            declaration_origin: None,
            declared_in_macro_type_arg: crate::semantic_query::MacroOwnBodyStamp::NEUTRAL,
            merge_role: crate::semantic_query::MergeRoleStamp::NEUTRAL,
        })
        .collect();
    let object = graph.intern_node(SemanticNodeData::Object(SurfaceView {
        members: Arc::from(explosive_members.into_boxed_slice()),
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
                owner: verter_type_expr::TopLevelOwnerId::ordinary_file(),
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
                owner: verter_type_expr::TopLevelOwnerId::ordinary_file(),
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

    let first = dispatch.classify_broad_runtime_transient(subject);
    let second = dispatch.classify_broad_runtime_transient(subject);

    for output in [first, second] {
        match output.result {
            QueryResult::Value(value) => assert_eq!(
                value,
                SemanticQueryValue::BroadRuntime(BroadRuntimeClassification::new([
                    BroadRuntimeKind::Boolean,
                ]))
            ),
            other => panic!("expected classifier value, got {other:?}"),
        }
        assert!(
            output.cache_suppress,
            "transient classification is ReturnOnly"
        );
    }
    assert_eq!(
        graph.memo_entry_count(),
        0,
        "rootless classifier result must not enter the family memo"
    );
}

/// Mutation recipe: derive cacheability from observed descendant roots rather
/// than the classifier subject. The global alias below then acquires its
/// child's file root and incorrectly publishes a durable family candidate.
#[test]
fn rootless_classifier_subject_stays_return_only_with_a_rooted_descendant() {
    let host = VerterHost::new_standalone(Default::default());
    upsert_ts(&host, "/rooted-child.ts", "export type Seed = string");
    let dispatch = ProjectSemanticDispatch::new(&host);
    let graph = host.project_type_store().semantic_graph();
    let rooted = graph.intern_node_with_scope(
        SemanticNodeData::Primitive(PrimitiveKind::String),
        file_scope(&dispatch, "/rooted-child.ts"),
    );
    let subject = graph.intern_node(SemanticNodeData::Alias(rooted));
    let memo_entries_before = graph.memo_entry_count();
    let output = dispatch.classify_broad_runtime_transient(subject);

    assert_eq!(
        classification_from_output(&output).kinds(),
        &[BroadRuntimeKind::String]
    );
    assert!(!output.result_is_partial);
    assert!(output.cache_suppress, "the rootless subject is ReturnOnly");
    assert_eq!(graph.memo_entry_count(), memo_entries_before);
}

/// Mutation recipe: remove the empty-complete fallback to explicit Unknown.
/// Intersections filter non-authoritative Unknown arms, leaving an empty kind
/// list that is otherwise indistinguishable from a silently truncated result.
#[test]
fn all_unknown_intersection_is_explicit_complete_unknown() {
    let host = VerterHost::new_standalone(Default::default());
    let graph = host.project_type_store().semantic_graph();
    let unknown = graph.intern_node(SemanticNodeData::Primitive(PrimitiveKind::Unknown));
    let any = graph.intern_node(SemanticNodeData::Primitive(PrimitiveKind::Any));
    let subject = graph.intern_node(SemanticNodeData::Intersection(Arc::from([unknown, any])));
    let dispatch = ProjectSemanticDispatch::new(&host);
    let output = dispatch.classify_broad_runtime_transient(subject);

    assert_eq!(
        classification_from_output(&output).kinds(),
        &[BroadRuntimeKind::Unknown]
    );
    assert!(!output.result_is_partial);
}

#[test]
fn canonical_macro_classifier_values_warm_and_reuse_the_typed_family_entry() {
    let host = VerterHost::new_standalone(Default::default());
    upsert_vue(
        &host,
        "/runtime.vue",
        r#"<script setup lang="ts">defineModel<string | boolean>()</script>"#,
    );
    let dispatch = ProjectSemanticDispatch::new(&host);
    let key = macro_classifier_key(&dispatch, "/runtime.vue", None);

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

/// Mutation recipe: retain the first graph-instance node in the family key or
/// skip the canonical-locator rehydrate on a warm retry. Re-indexing the same
/// owner then either changes key identity or serves the stale String result.
#[test]
fn canonical_locator_rehydrates_after_owner_reindex_without_node_identity() {
    let host = VerterHost::new_standalone(Default::default());
    upsert_vue(
        &host,
        "/reindex.vue",
        r#"<script setup lang="ts">defineModel<string>()</script>"#,
    );

    let first_dispatch = ProjectSemanticDispatch::new(&host);
    let first_key = macro_classifier_key(&first_dispatch, "/reindex.vue", None);
    assert_eq!(
        execute_classification(&first_dispatch, first_key.clone()).kinds(),
        &[BroadRuntimeKind::String]
    );

    upsert_vue(
        &host,
        "/reindex.vue",
        r#"<script setup lang="ts">
type Padding = { before: string; after: boolean }
defineModel<number>()
</script>"#,
    );

    let second_dispatch = ProjectSemanticDispatch::new(&host);
    let second_key = macro_classifier_key(&second_dispatch, "/reindex.vue", None);
    assert_eq!(
        first_key, second_key,
        "content edits and graph-node allocation are validity, never family identity"
    );
    assert_eq!(
        execute_classification(&second_dispatch, second_key.clone()).kinds(),
        &[BroadRuntimeKind::Number],
        "the canonical route must re-source the current payload after invalidation"
    );
    assert_eq!(
        execute_classification(&second_dispatch, second_key).kinds(),
        &[BroadRuntimeKind::Number],
        "the new version may then warm under the same content-free family"
    );
}

/// Mutation recipe: validate a classifier candidate against the base host
/// instead of the caller's session view. The overlay request then warm-serves
/// String, or its Number candidate overwrites/leaks back into the base view.
#[test]
fn canonical_classifier_candidates_are_isolated_by_overlay_read_sets() {
    use crate::resolver_core::SessionResolverContext;
    use crate::session_view::OverlaidView;
    use rustc_hash::FxHashMap;

    let canonical = "/overlay-runtime.vue";
    let host = Arc::new(VerterHost::new_standalone(Default::default()));
    upsert_vue(
        &host,
        canonical,
        r#"<script setup lang="ts">defineModel<string>()</script>"#,
    );
    host.ensure_indexed_ready(canonical)
        .expect("base IndexedReady materialises");

    let base_dispatch = ProjectSemanticDispatch::new(host.as_ref());
    let base_key = macro_classifier_key(&base_dispatch, canonical, None);
    assert_eq!(
        execute_classification(&base_dispatch, base_key.clone()).kinds(),
        &[BroadRuntimeKind::String]
    );

    let overlay_source: Arc<str> =
        Arc::from(r#"<script setup lang="ts">defineModel<number>()</script>"#);
    let mut overlays = FxHashMap::default();
    overlays.insert(canonical.to_owned(), overlay_source);
    let view = OverlaidView::new(Arc::clone(&host), overlays);
    host.materialize_overlay_indexed_ready_with_view(canonical, &view)
        .expect("overlay IndexedReady materialises");
    let store_view = host
        .resolver_store_view_read()
        .into_owned_view()
        .with_session_overlay(&host, &view);
    let overlay_ctx = SessionResolverContext::new(
        &host,
        &view,
        &store_view,
        Arc::new(crate::resolver_core::CanonicalCompletionOverlay::new()),
    );
    let overlay_dispatch = ProjectSemanticDispatch::new(&overlay_ctx);
    let overlay_key = macro_classifier_key(&overlay_dispatch, canonical, None);

    assert_eq!(
        base_key, overlay_key,
        "overlay content is candidate validity, never classifier family identity"
    );
    assert_eq!(
        execute_classification(&overlay_dispatch, overlay_key.clone()).kinds(),
        &[BroadRuntimeKind::Number],
        "the overlay must not warm-hit the base candidate"
    );
    assert_eq!(
        execute_classification(&base_dispatch, base_key.clone()).kinds(),
        &[BroadRuntimeKind::String],
        "the overlay candidate must not leak back into the base view"
    );
    assert_eq!(
        host.project_type_store()
            .semantic_graph()
            .slot_candidate_count_for_tests(&base_key),
        2,
        "base and overlay content versions coexist in one content-free slot"
    );
}

#[test]
fn broad_runtime_contexts_do_not_warm_hit() {
    let host = VerterHost::new_standalone(Default::default());
    upsert_vue(
        &host,
        "/context.vue",
        r#"<script setup lang="ts">defineModel<string>()</script>"#,
    );
    let dispatch = ProjectSemanticDispatch::new(&host);
    let primary = macro_classifier_key(&dispatch, "/context.vue", None);
    let SemanticQueryKey::ClassifyBroadRuntime { subject, context } = primary.clone() else {
        unreachable!("helper returns classifier key");
    };
    let mut alternate_context = context;
    alternate_context.resolve_env_hash[0] ^= u8::MAX;
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
    upsert_vue(
        &host,
        "/partial.vue",
        r#"<script setup lang="ts">defineModel<string>()</script>"#,
    );
    let dispatch = ProjectSemanticDispatch::new(&host);
    let key = macro_classifier_key(&dispatch, "/partial.vue", None);

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

/// Mutation recipe: remove the classifier's `node_data == None` partial fold.
/// The read becomes Complete and the rooted family slot admits, failing both
/// typed-completeness and no-poison assertions below.
#[test]
fn missing_semantic_node_data_is_partial_unknown_and_return_only() {
    let host = VerterHost::new_standalone(Default::default());
    upsert_ts(&host, "/missing-node.ts", "export type Seed = string");
    let dispatch = ProjectSemanticDispatch::new(&host);
    let graph = host.project_type_store().semantic_graph();
    let subject = graph.intern_node_with_scope(
        SemanticNodeData::Alias(SemanticNodeId(u64::MAX)),
        file_scope(&dispatch, "/missing-node.ts"),
    );
    let memo_entries_before = graph.memo_entry_count();

    let completeness_scope = ColdComputeCompletenessScope::enter();
    let output = dispatch.classify_broad_runtime_transient(subject);
    let completeness = current_cold_compute_completeness();
    drop(completeness_scope);

    assert_eq!(
        classification_from_output(&output).kinds(),
        &[BroadRuntimeKind::Unknown]
    );
    assert!(
        output.result_is_partial,
        "missing arena data is not Complete"
    );
    assert!(output.cache_suppress, "a partial result is ReturnOnly");
    assert!(
        completeness
            .reasons()
            .contains(PartialReasonSet::MISSING_SEMANTIC_NODE_DATA),
        "the typed completeness must retain the exact missing-node reason"
    );
    assert_eq!(graph.memo_entry_count(), memo_entries_before);
}

/// Mutation recipe: remove the per-work-item connected-work charge from the
/// runtime classifier. The explosive union completes as `Number` and admits a
/// warm candidate instead of returning typed Partial/ReturnOnly `Unknown`.
#[test]
fn classifier_work_exhaustion_is_partial_unknown_and_never_warms() {
    let host = VerterHost::new_standalone(Default::default());
    upsert_ts(&host, "/work-limit.ts", "export type Seed = string");
    let dispatch = ProjectSemanticDispatch::new(&host);
    let graph = host.project_type_store().semantic_graph();
    let arms: Vec<_> = (0..64)
        .map(|value| {
            graph.intern_node(SemanticNodeData::Literal(LiteralValue::Number(f64::from(
                value,
            ))))
        })
        .collect();
    let subject = graph.intern_node_with_scope(
        SemanticNodeData::Union(Arc::from(arms.into_boxed_slice())),
        file_scope(&dispatch, "/work-limit.ts"),
    );
    let memo_entries_before = graph.memo_entry_count();
    dispatch.set_connected_limits_for_tests(4, 24);

    let completeness_scope = ColdComputeCompletenessScope::enter();
    let output = dispatch.classify_broad_runtime_transient(subject);
    let completeness = current_cold_compute_completeness();
    drop(completeness_scope);

    assert_eq!(
        classification_from_output(&output).kinds(),
        &[BroadRuntimeKind::Unknown],
        "an unfinished classifier must not expose a discovered subset"
    );
    assert!(output.result_is_partial);
    assert!(output.cache_suppress);
    assert!(completeness
        .reasons()
        .contains(PartialReasonSet::PROJECTION_WORK_LIMIT));
    assert_eq!(graph.memo_entry_count(), memo_entries_before);
}

/// Mutation recipe: collapse every `Opaque(QueryError)` to a valid semantic
/// `Unknown`. The non-Miss case becomes Complete and memo-admissible, while the
/// assertions require a typed query-fault Partial and keep honest Miss distinct.
#[test]
fn opaque_query_fault_is_partial_but_honest_miss_is_complete_unknown() {
    let host = VerterHost::new_standalone(Default::default());
    upsert_ts(&host, "/query-fault.ts", "export type Seed = string");
    let dispatch = ProjectSemanticDispatch::new(&host);
    let graph = host.project_type_store().semantic_graph();
    let scope = file_scope(&dispatch, "/query-fault.ts");
    let fault_subject = graph.intern_node_with_scope(
        SemanticNodeData::Opaque(QueryError::Other(Arc::from("fixture fault"))),
        scope.clone(),
    );
    let miss_subject =
        graph.intern_node_with_scope(SemanticNodeData::Opaque(QueryError::Miss), scope);
    let memo_entries_before = graph.memo_entry_count();
    let fault_scope = ColdComputeCompletenessScope::enter();
    let fault = dispatch.classify_broad_runtime_transient(fault_subject);
    let fault_completeness = current_cold_compute_completeness();
    drop(fault_scope);

    assert_eq!(
        classification_from_output(&fault).kinds(),
        &[BroadRuntimeKind::Unknown]
    );
    assert!(fault.result_is_partial);
    assert!(fault.cache_suppress);
    assert!(fault_completeness
        .reasons()
        .contains(PartialReasonSet::SEMANTIC_QUERY_FAULT));
    assert_eq!(graph.memo_entry_count(), memo_entries_before);

    let miss_scope = ColdComputeCompletenessScope::enter();
    let miss = dispatch.classify_broad_runtime_transient(miss_subject);
    let miss_completeness = current_cold_compute_completeness();
    drop(miss_scope);

    assert_eq!(
        classification_from_output(&miss).kinds(),
        &[BroadRuntimeKind::Unknown]
    );
    assert!(!miss.result_is_partial, "honest Miss is semantic Unknown");
    assert!(
        miss.cache_suppress,
        "transient classification is ReturnOnly"
    );
    assert!(!miss_completeness.is_partial());
    assert_eq!(graph.memo_entry_count(), memo_entries_before);
}

/// Mutation recipe: classify an opaque recursive back-edge as ordinary
/// Unknown. The read becomes Complete and publishes, losing both the typed
/// recursion reason and the required cold retry behavior.
#[test]
fn recursive_runtime_carrier_is_typed_partial_and_return_only() {
    let host = VerterHost::new_standalone(Default::default());
    upsert_ts(&host, "/recursive-runtime.ts", "export type Seed = string");
    let dispatch = ProjectSemanticDispatch::new(&host);
    let graph = host.project_type_store().semantic_graph();
    let subject = graph.intern_node_with_scope(
        SemanticNodeData::Opaque(QueryError::RecursiveRef {
            name: Arc::from("RuntimeRecursive"),
        }),
        file_scope(&dispatch, "/recursive-runtime.ts"),
    );
    let memo_entries_before = graph.memo_entry_count();

    let completeness_scope = ColdComputeCompletenessScope::enter();
    let output = dispatch.classify_broad_runtime_transient(subject);
    let completeness = current_cold_compute_completeness();
    drop(completeness_scope);

    assert_eq!(
        classification_from_output(&output).kinds(),
        &[BroadRuntimeKind::Unknown]
    );
    assert!(output.result_is_partial);
    assert!(output.cache_suppress);
    assert!(completeness
        .reasons()
        .contains(PartialReasonSet::SAME_PATH_RECURSION));
    assert_eq!(graph.memo_entry_count(), memo_entries_before);
}

/// Mutation recipe: replace the heap-owned worklist with recursion or add a
/// fixed depth escape. This 4,096-hop terminating alias resolves to String in
/// full; a stack/depth shortcut returns Unknown/Partial or overflows instead.
#[test]
fn finite_deep_alias_chain_is_iterative_and_complete() {
    let host = VerterHost::new_standalone(Default::default());
    let graph = host.project_type_store().semantic_graph();
    let mut subject = graph.intern_node(SemanticNodeData::Primitive(PrimitiveKind::String));
    // Builds the finite alias chain the classifier must resolve iteratively.
    // bounded-loop: fixed 4,096-iteration fixture constructor
    for _ in 0..4_096 {
        subject = graph.intern_node(SemanticNodeData::Alias(subject));
    }
    let dispatch = ProjectSemanticDispatch::new(&host);
    let output = dispatch.classify_broad_runtime_transient(subject);

    assert_eq!(
        classification_from_output(&output).kinds(),
        &[BroadRuntimeKind::String]
    );
    assert!(!output.result_is_partial);
}
