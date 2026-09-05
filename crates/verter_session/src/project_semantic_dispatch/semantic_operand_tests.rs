use std::sync::atomic::Ordering;
use std::sync::Arc;

use static_assertions::assert_not_impl_any;
use verter_type_expr::locators::{
    AugmentationBodyLocator, AuthoredAnchor, AuthoredAugmentationScope, AuthoredBodyLocator,
    JsdocTypedefBodyLocator, LocatorSymbolSpace, MacroPayloadLocator, MacroPayloadPosition,
    TypeBodyPathStep, TypeBodySlot, TypeParamBoundPosition,
};
use verter_type_expr::TopLevelOwnerId;

use crate::locator_identity::{
    semantic_space_for_locator_space, LibEnvHash, LocatorLoweringKey, ParseEnvHash,
    ProjectIdentityDim, ResolveEnvHash, SlotEnvIdentity, TypeEnvHash,
};
use crate::project_semantic_dispatch::raise::{
    dispatch_cold_for, dispatch_warm_for, enable_dispatch_trace_for_test, DISPATCH_TRACE,
};
use crate::request_context::{RequestContext, RequestContextGuard};
use crate::semantic_query::operand::{
    ForcedSemanticOperand, OperandSplitEnv, SemanticOperand, SemanticOperandForceRequest,
    SemanticOperandMintError, SemanticOperandParts,
};
use crate::semantic_query::{
    DeclarationSlotSeed, InstantiateContext, InstantiateKey, MemberMergeRole, PrimitiveKind,
    ProjectionMode, ProjectionReductionContext, QueryError, QueryResult, ReductionDemand,
    SemanticNodeData, SemanticNodeId, SemanticQueryKey, SurfaceProvenanceContext,
    VueHeritagePolicy,
};
use crate::types::{HostConfig, UpsertRequest};
use crate::{CompileErrorPolicy, FileLanguage, VerterHost};

use super::{BuildLocalTaintGuard, ProjectSemanticDispatch, SemanticOperandAuthority};

assert_not_impl_any!(SemanticOperandForceRequest: Clone, Copy);

pub(super) const OWNER: &str = "/w/operand/owner.ts";
pub(super) const OTHER_OWNER: &str = "/w/operand/other.ts";
pub(super) const SOURCE_V1: &str = "\
export type Text = string;\n\
export type Count = number;\n\
export type Box<T> = { value: T };\n\
export type Owned = { a: string };\n";
const SOURCE_V2: &str = "\
export type Text = string;\n\
export type Count = number;\n\
export type Box<T> = { value: T };\n\
export type Owned = { a: string; b: number };\n";

pub(super) fn make_host() -> VerterHost {
    VerterHost::new_standalone(HostConfig {
        dev_mode: false,
        compile_error_policy: CompileErrorPolicy::StrictError,
        ..HostConfig::default()
    })
}

pub(super) fn upsert(host: &VerterHost, source: &str) {
    upsert_at(host, OWNER, source);
}

pub(super) fn upsert_at(host: &VerterHost, canonical: &str, source: &str) {
    let _ = host
        .upsert(UpsertRequest {
            canonical_id: None,
            input_id: canonical.to_string(),
            source: Arc::from(source),
            file_language: FileLanguage::script_ts(),
            aliases: Vec::new(),
        })
        .unwrap();
}

pub(super) fn locator(
    symbol: &str,
    path: impl Into<Arc<[TypeBodyPathStep]>>,
) -> AuthoredBodyLocator {
    locator_at(OWNER, symbol, LocatorSymbolSpace::Type, path)
}

pub(super) fn locator_at(
    canonical: &str,
    symbol: &str,
    space: LocatorSymbolSpace,
    path: impl Into<Arc<[TypeBodyPathStep]>>,
) -> AuthoredBodyLocator {
    AuthoredBodyLocator::DeclBody(TypeBodySlot {
        anchor: AuthoredAnchor {
            canonical_id: Arc::from(canonical),
            owner: TopLevelOwnerId::ordinary_file(),
            symbol: Arc::from(symbol),
            space,
        },
        path: path.into(),
    })
}

#[test]
fn same_shaped_declarations_and_scope_peers_keep_distinct_force_families() {
    let host = make_host();
    upsert(&host, "type A = string; type B = string;");
    upsert_at(&host, OTHER_OWNER, "type A = string;");
    let dispatch = ProjectSemanticDispatch::new(&host);
    let context = ProjectionReductionContext::published(ProjectionMode::Identity);
    let a = mint(&dispatch, whole("A"));
    let b = mint(&dispatch, whole("B"));
    let other_a = mint(
        &dispatch,
        locator_at(
            OTHER_OWNER,
            "A",
            LocatorSymbolSpace::Type,
            Arc::<[TypeBodyPathStep]>::from([]),
        ),
    );
    let a_key = force_key(&dispatch, &a, context);
    let b_key = force_key(&dispatch, &b, context);
    let other_key = force_key(&dispatch, &other_a, context);
    assert_ne!(a_key, b_key, "different binders must not alias");
    assert_ne!(a_key, other_key, "different lexical scopes must not alias");
    let a_forced = force_with_context(&dispatch, &a, context);
    let b_forced = force_with_context(&dispatch, &b, context);
    let other_forced = force_with_context(&dispatch, &other_a, context);
    let a_node = a_forced.node();
    let b_node = b_forced.node();
    let other_node = other_forced.node();
    assert_primitive(&host, a_node, PrimitiveKind::String);
    assert_primitive(&host, b_node, PrimitiveKind::String);
    assert_primitive(&host, other_node, PrimitiveKind::String);
    assert!(a_forced
        .evidence()
        .self_roots()
        .iter()
        .any(|(canonical, _)| canonical.as_ref() == OWNER));
    assert!(!a_forced
        .evidence()
        .self_roots()
        .iter()
        .any(|(canonical, _)| canonical.as_ref() == OTHER_OWNER));
    assert!(other_forced
        .evidence()
        .self_roots()
        .iter()
        .any(|(canonical, _)| canonical.as_ref() == OTHER_OWNER));
    assert!(!other_forced
        .evidence()
        .self_roots()
        .iter()
        .any(|(canonical, _)| canonical.as_ref() == OWNER));
    let graph = host.project_type_store().semantic_graph();
    assert_eq!(graph.slot_candidate_count_for_tests(&a_key), 1);
    assert_eq!(graph.slot_candidate_count_for_tests(&b_key), 1);
    assert_eq!(graph.slot_candidate_count_for_tests(&other_key), 1);
}

#[test]
fn content_free_authored_identity_survives_generation_change() {
    let host = make_host();
    upsert(&host, "type A = string;");
    let dispatch = ProjectSemanticDispatch::new(&host);
    let operand = mint(&dispatch, whole("A"));
    let before = force(&dispatch, &operand, ProjectionMode::Identity);
    host.project_type_store().bump_project_generation();
    let after = force(&dispatch, &operand, ProjectionMode::Identity);
    assert_primitive(&host, before, PrimitiveKind::String);
    assert_primitive(&host, after, PrimitiveKind::String);
}

pub(super) fn whole(symbol: &str) -> AuthoredBodyLocator {
    locator(symbol, Arc::from([]))
}

pub(super) fn member_value(symbol: &str, ordinal: u32) -> AuthoredBodyLocator {
    locator(
        symbol,
        Arc::from([
            TypeBodyPathStep::Member { ordinal },
            TypeBodyPathStep::MemberValue,
        ]),
    )
}

pub(super) fn request(mode: ProjectionMode) -> SemanticOperandForceRequest {
    SemanticOperandForceRequest::new(ProjectionReductionContext::published(mode))
}

pub(super) fn mint(
    dispatch: &ProjectSemanticDispatch<'_>,
    locator: AuthoredBodyLocator,
) -> SemanticOperand {
    dispatch
        .mint_authored_semantic_operand(locator, Arc::from([]))
        .expect("fixture operand must mint")
}

pub(super) fn force(
    dispatch: &ProjectSemanticDispatch<'_>,
    operand: &SemanticOperand,
    mode: ProjectionMode,
) -> SemanticNodeId {
    force_result(dispatch, operand, mode).node()
}

fn force_result(
    dispatch: &ProjectSemanticDispatch<'_>,
    operand: &SemanticOperand,
    mode: ProjectionMode,
) -> ForcedSemanticOperand {
    match dispatch.force_semantic_operand(operand, request(mode)) {
        QueryResult::Value(forced) => forced,
        other => panic!("operand must force, got {other:?}"),
    }
}

fn force_with_context(
    dispatch: &ProjectSemanticDispatch<'_>,
    operand: &SemanticOperand,
    context: ProjectionReductionContext,
) -> ForcedSemanticOperand {
    match dispatch.force_semantic_operand(operand, SemanticOperandForceRequest::new(context)) {
        QueryResult::Value(forced) => forced,
        other => panic!("operand must force, got {other:?}"),
    }
}

pub(super) fn force_key(
    _dispatch: &ProjectSemanticDispatch<'_>,
    operand: &SemanticOperand,
    context: ProjectionReductionContext,
) -> SemanticQueryKey {
    let SemanticOperandParts::Authored(authored) =
        operand.parts(SemanticOperandAuthority::mint_for_forcing_boundary())
    else {
        panic!("fixture must be authored")
    };
    let anchor = crate::semantic_query::operand::authored_anchor(authored.locator());
    let (parse, resolve, type_env, lib_env, project) = authored.split_env().parts();
    let slot = DeclarationSlotSeed::new(
        Arc::clone(&anchor.canonical_id),
        anchor.owner,
        Arc::clone(&anchor.symbol),
        semantic_space_for_locator_space(anchor.space),
    )
    .finalize(SlotEnvIdentity::new(type_env, lib_env, project));
    let lower =
        LocatorLoweringKey::new_unsubstituted(slot, authored.locator().clone(), parse, resolve)
            .expect("fixture locator key");
    let instantiate_context = InstantiateContext::file_backed(
        context,
        resolve.get(),
        parse,
        super::BodySourceWitness::mint_for_dispatch_factory(),
    );
    // Mirrors the production routing: an empty-path type-space DeclBody
    // force converges on the DECLARATION Instantiate family; every nested
    // or alternate locator position keeps authored identity.
    SemanticQueryKey::Instantiate(if authored.addresses_whole_type_declaration() {
        InstantiateKey::new(
            lower.slot().clone(),
            Arc::clone(authored.substitution()),
            instantiate_context,
        )
    } else {
        InstantiateKey::new_authored(
            lower.slot().clone(),
            authored.query_identity(),
            Arc::clone(authored.substitution()),
            instantiate_context,
            SemanticOperandAuthority::mint_for_forcing_boundary(),
        )
    })
}

fn with_split_env(operand: &SemanticOperand, split_env: OperandSplitEnv) -> SemanticOperand {
    operand.with_split_env(
        split_env,
        SemanticOperandAuthority::mint_for_forcing_boundary(),
    )
}

fn assert_primitive(host: &VerterHost, node: SemanticNodeId, expected: PrimitiveKind) {
    assert!(
        matches!(
            host.project_type_store().semantic_graph().node_data(node).as_deref(),
            Some(SemanticNodeData::Primitive(actual)) if *actual == expected
        ),
        "expected {expected:?}"
    );
}

pub(super) fn assert_type_param(host: &VerterHost, node: SemanticNodeId) {
    assert!(
        matches!(
            host.project_type_store()
                .semantic_graph()
                .node_data(node)
                .as_deref(),
            Some(SemanticNodeData::TypeParam { .. })
        ),
        "expected a lexically bound type parameter"
    );
}

pub(super) fn assert_infer_ref(host: &VerterHost, node: SemanticNodeId) {
    assert!(
        matches!(
            host.project_type_store()
                .semantic_graph()
                .node_data(node)
                .as_deref(),
            Some(SemanticNodeData::InferRef { .. })
        ),
        "expected a lexically bound infer reference"
    );
}

pub(super) fn assert_operand_error(
    result: QueryResult<ForcedSemanticOperand>,
    expected: QueryError,
) {
    match result {
        QueryResult::Error(actual) => assert_eq!(actual, expected),
        other => panic!("expected {expected:?}, got {other:?}"),
    }
}

fn join_within<T: Send + 'static>(handle: std::thread::JoinHandle<T>, label: &str) -> T {
    let (tx, rx) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        let _ = tx.send(handle.join());
    });
    rx.recv_timeout(std::time::Duration::from_secs(10))
        .unwrap_or_else(|_| panic!("{label} did not finish"))
        .unwrap_or_else(|_| panic!("{label} panicked"))
}

fn locator_key(
    host: &VerterHost,
    dispatch: &ProjectSemanticDispatch<'_>,
    locator: AuthoredBodyLocator,
) -> SemanticQueryKey {
    let anchor = match &locator {
        AuthoredBodyLocator::DeclBody(slot) => &slot.anchor,
        _ => unreachable!(),
    };
    let slot = dispatch.type_slot_for(
        Arc::clone(&anchor.canonical_id),
        anchor.owner,
        Arc::clone(&anchor.symbol),
    );
    let env = host.host_view_env_hashes_for(anchor.canonical_id.as_ref());
    SemanticQueryKey::LowerLocator {
        key: LocatorLoweringKey::new_unsubstituted(
            slot,
            locator,
            ParseEnvHash::from_env_hash(env.parse_env_hash),
            ResolveEnvHash::from_env_hash(env.resolve_env_hash),
        )
        .unwrap(),
    }
}

#[test]
fn every_request_context_axis_changes_the_forced_family_and_unchanged_context_converges() {
    let host = make_host();
    upsert(&host, SOURCE_V1);
    let dispatch = ProjectSemanticDispatch::new(&host);
    let operand = mint(&dispatch, whole("Owned"));
    let base = ProjectionReductionContext::published(ProjectionMode::Shallow);
    let variants = [
        ProjectionReductionContext {
            mode: ProjectionMode::Expanded,
            ..base
        },
        ProjectionReductionContext {
            demand: ReductionDemand::StructuralTransit,
            ..base
        },
        ProjectionReductionContext {
            provenance: SurfaceProvenanceContext::MacroTypeArgOwnBody,
            ..base
        },
        ProjectionReductionContext {
            merge_role: MemberMergeRole::OwnBody,
            ..base
        },
        ProjectionReductionContext {
            vue_heritage_policy: VueHeritagePolicy::SuppressIgnored,
            ..base
        },
    ];
    let base_key = force_key(&dispatch, &operand, base);
    let first = force_with_context(&dispatch, &operand, base);
    let second = force_with_context(&dispatch, &operand, base);
    assert_eq!(first.node(), second.node());
    assert_eq!(dispatch_cold_for(&base_key), 1);
    assert_eq!(dispatch_warm_for(&base_key), 1);
    for variant in variants {
        let variant_key = force_key(&dispatch, &operand, variant);
        assert_ne!(base_key, variant_key);
        let _ = force_with_context(&dispatch, &operand, variant);
        assert_eq!(dispatch_cold_for(&variant_key), 1);
        assert_eq!(
            host.project_type_store()
                .semantic_graph()
                .slot_candidate_count_for_tests(&variant_key),
            1
        );
    }
}

#[test]
fn every_sealed_environment_axis_changes_family_identity_and_wrong_env_refuses_dispatch() {
    let host = make_host();
    upsert(&host, SOURCE_V1);
    let dispatch = ProjectSemanticDispatch::new(&host);
    let operand = mint(&dispatch, whole("Owned"));
    let SemanticOperandParts::Authored(authored) =
        operand.parts(SemanticOperandAuthority::mint_for_forcing_boundary())
    else {
        unreachable!()
    };
    let base = authored.split_env();
    let (parse, resolve, type_env, lib_env, project) = base.parts();
    let variants = [
        OperandSplitEnv::new(
            ParseEnvHash::from_env_hash([9; 16]),
            resolve,
            type_env,
            lib_env,
            project,
            SemanticOperandAuthority::mint_for_forcing_boundary(),
        ),
        OperandSplitEnv::new(
            parse,
            ResolveEnvHash::from_env_hash([9; 16]),
            type_env,
            lib_env,
            project,
            SemanticOperandAuthority::mint_for_forcing_boundary(),
        ),
        OperandSplitEnv::new(
            parse,
            resolve,
            TypeEnvHash::from_env_hash([9; 16]),
            lib_env,
            project,
            SemanticOperandAuthority::mint_for_forcing_boundary(),
        ),
        OperandSplitEnv::new(
            parse,
            resolve,
            type_env,
            LibEnvHash::from_env_hash([9; 16]),
            project,
            SemanticOperandAuthority::mint_for_forcing_boundary(),
        ),
        OperandSplitEnv::new(
            parse,
            resolve,
            type_env,
            lib_env,
            ProjectIdentityDim::from_project_identity(9),
            SemanticOperandAuthority::mint_for_forcing_boundary(),
        ),
    ];
    let context = ProjectionReductionContext::published(ProjectionMode::Identity);
    let base_key = force_key(&dispatch, &operand, context);
    let _ = force_with_context(&dispatch, &operand, context);
    for split_env in variants {
        let variant = with_split_env(&operand, split_env);
        assert_ne!(force_key(&dispatch, &variant, context), base_key);
        assert_operand_error(
            dispatch.force_semantic_operand(&variant, SemanticOperandForceRequest::new(context)),
            QueryError::StaleSemanticOperand,
        );
    }
}

#[test]
fn exact_locator_path_changes_forced_result_and_family() {
    let host = make_host();
    upsert(&host, SOURCE_V1);
    let dispatch = ProjectSemanticDispatch::new(&host);
    let whole_locator = whole("Owned");
    let child_locator = member_value("Owned", 0);
    let whole_operand = mint(&dispatch, whole_locator.clone());
    let child_operand = mint(&dispatch, child_locator.clone());
    assert_ne!(whole_operand, child_operand);

    let whole_node = force(&dispatch, &whole_operand, ProjectionMode::Expanded);
    let child_node = force(&dispatch, &child_operand, ProjectionMode::Expanded);
    assert!(matches!(
        host.project_type_store()
            .semantic_graph()
            .node_data(whole_node)
            .as_deref(),
        Some(SemanticNodeData::Object(_))
    ));
    assert_primitive(&host, child_node, PrimitiveKind::String);
    assert_ne!(
        locator_key(&host, &dispatch, whole_locator),
        locator_key(&host, &dispatch, child_locator)
    );
}

#[test]
fn alternate_locator_arm_is_forced_by_lower_locator() {
    let host = make_host();
    upsert(
        &host,
        "/** @typedef {{ a: number }} FromDoc */\ntype Owned = { a: string };\n",
    );
    let dispatch = ProjectSemanticDispatch::new(&host);
    let locator = AuthoredBodyLocator::JsdocTypedefBody(JsdocTypedefBodyLocator {
        anchor: AuthoredAnchor {
            canonical_id: Arc::from(OWNER),
            owner: TopLevelOwnerId::ordinary_file(),
            symbol: Arc::from("FromDoc"),
            space: LocatorSymbolSpace::Type,
        },
        path: Arc::from([]),
    });
    let node = force(
        &dispatch,
        &mint(&dispatch, locator),
        ProjectionMode::Expanded,
    );
    assert!(matches!(
        host.project_type_store()
            .semantic_graph()
            .node_data(node)
            .as_deref(),
        Some(SemanticNodeData::Object(_))
    ));

    upsert(
        &host,
        "type Aug = string;\ndeclare module \"pkg\" { interface Aug { value: number } }\n",
    );
    let dispatch = ProjectSemanticDispatch::new(&host);
    let augmentation = AuthoredBodyLocator::AugmentationBody(AugmentationBodyLocator {
        anchor: AuthoredAnchor {
            canonical_id: Arc::from(OWNER),
            owner: TopLevelOwnerId::ordinary_file(),
            symbol: Arc::from("Aug"),
            space: LocatorSymbolSpace::Type,
        },
        scope: AuthoredAugmentationScope::Module {
            specifier: Arc::from("pkg"),
        },
        path: Arc::from([
            TypeBodyPathStep::Member { ordinal: 0 },
            TypeBodyPathStep::MemberValue,
        ]),
    });
    assert_primitive(
        &host,
        force(
            &dispatch,
            &mint(&dispatch, augmentation),
            ProjectionMode::Identity,
        ),
        PrimitiveKind::Number,
    );

    upsert(&host, "let { row }: { row: string } = $props();\n");
    let dispatch = ProjectSemanticDispatch::new(&host);
    let macro_annotation = AuthoredBodyLocator::MacroPayload(MacroPayloadLocator {
        anchor: AuthoredAnchor {
            canonical_id: Arc::from(OWNER),
            owner: TopLevelOwnerId::ordinary_file(),
            symbol: Arc::from("default"),
            space: LocatorSymbolSpace::Value,
        },
        macro_index: 0,
        payload: MacroPayloadPosition::TypeAnnotation,
    });
    let node = force(
        &dispatch,
        &mint(&dispatch, macro_annotation),
        ProjectionMode::Expanded,
    );
    assert!(matches!(
        host.project_type_store()
            .semantic_graph()
            .node_data(node)
            .as_deref(),
        Some(SemanticNodeData::Object(_))
    ));
}

#[test]
fn authority_derived_binder_and_substitution_do_not_alias() {
    let host = make_host();
    upsert(&host, SOURCE_V1);
    let dispatch = ProjectSemanticDispatch::new(&host);

    let text_forced = force_result(
        &dispatch,
        &mint(&dispatch, whole("Text")),
        ProjectionMode::Identity,
    );
    let count_forced = force_result(
        &dispatch,
        &mint(&dispatch, whole("Count")),
        ProjectionMode::Identity,
    );
    let text = dispatch.mint_node_semantic_operand(&text_forced).unwrap();
    let count = dispatch.mint_node_semantic_operand(&count_forced).unwrap();

    let text_box = dispatch
        .mint_authored_semantic_operand(member_value("Box", 0), Arc::from([text]))
        .unwrap();
    let count_box = dispatch
        .mint_authored_semantic_operand(member_value("Box", 0), Arc::from([count]))
        .unwrap();
    assert_ne!(text_box, count_box);
    assert_primitive(
        &host,
        force(&dispatch, &text_box, ProjectionMode::Identity),
        PrimitiveKind::String,
    );
    assert_primitive(
        &host,
        force(&dispatch, &count_box, ProjectionMode::Identity),
        PrimitiveKind::Number,
    );
}

#[test]
fn nested_function_mapper_and_infer_binders_are_not_captured_by_outer_substitution() {
    let host = make_host();
    upsert(
        &host,
        "type Number = number;\n\
         type FnBox<T> = { f: <T>() => T; outer: T };\n\
         type MapBox<T> = { [T in 'x']: T };\n\
         type InferBox<T> = T extends infer T ? T : never;\n",
    );
    let dispatch = ProjectSemanticDispatch::new(&host);
    let number_forced = force_result(
        &dispatch,
        &mint(&dispatch, whole("Number")),
        ProjectionMode::Identity,
    );
    let number = dispatch
        .mint_node_semantic_operand(&number_forced)
        .expect("number operand");
    let substituted = |path: Arc<[TypeBodyPathStep]>, symbol: &str| {
        dispatch
            .mint_authored_semantic_operand(locator(symbol, path), Arc::from([number.clone()]))
            .expect("generic operand")
    };

    let inner_function = substituted(
        Arc::from([
            TypeBodyPathStep::Member { ordinal: 0 },
            TypeBodyPathStep::MemberValue,
            TypeBodyPathStep::FunctionReturn,
        ]),
        "FnBox",
    );
    assert_type_param(
        &host,
        force(&dispatch, &inner_function, ProjectionMode::Identity),
    );
    let outer = substituted(
        Arc::from([
            TypeBodyPathStep::Member { ordinal: 1 },
            TypeBodyPathStep::MemberValue,
        ]),
        "FnBox",
    );
    assert_primitive(
        &host,
        force(&dispatch, &outer, ProjectionMode::Identity),
        PrimitiveKind::Number,
    );

    let mapper = substituted(Arc::from([TypeBodyPathStep::MappedValue]), "MapBox");
    assert_type_param(&host, force(&dispatch, &mapper, ProjectionMode::Identity));
    let infer = substituted(Arc::from([TypeBodyPathStep::ConditionalTrue]), "InferBox");
    assert_infer_ref(&host, force(&dispatch, &infer, ProjectionMode::Identity));
}

#[test]
fn defaults_augmentation_and_structural_child_locators_use_canonical_instantiation() {
    let host = make_host();
    upsert(
        &host,
        "type Number = number;\n\
         type Defaulted<T = string> = T;\n\
         type Places<T> = [{ [key: string]: T }, T];\n\
         type Maker<T> = new (value: T) => T;\n\
         declare module 'pkg' { interface Aug<T> { value: T } }\n",
    );
    let dispatch = ProjectSemanticDispatch::new(&host);
    assert_primitive(
        &host,
        force(
            &dispatch,
            &mint(&dispatch, whole("Defaulted")),
            ProjectionMode::Identity,
        ),
        PrimitiveKind::String,
    );
    let number_forced = force_result(
        &dispatch,
        &mint(&dispatch, whole("Number")),
        ProjectionMode::Identity,
    );
    let number = dispatch
        .mint_node_semantic_operand(&number_forced)
        .expect("number operand");
    let instantiate = |body: AuthoredBodyLocator| {
        dispatch
            .mint_authored_semantic_operand(body, Arc::from([number.clone()]))
            .expect("generic operand")
    };

    let augmentation = instantiate(AuthoredBodyLocator::AugmentationBody(
        AugmentationBodyLocator {
            anchor: AuthoredAnchor {
                canonical_id: Arc::from(OWNER),
                owner: TopLevelOwnerId::ordinary_file(),
                symbol: Arc::from("Aug"),
                space: LocatorSymbolSpace::Type,
            },
            scope: AuthoredAugmentationScope::Module {
                specifier: Arc::from("pkg"),
            },
            path: Arc::from([
                TypeBodyPathStep::Member { ordinal: 0 },
                TypeBodyPathStep::MemberValue,
            ]),
        },
    ));
    assert_primitive(
        &host,
        force(&dispatch, &augmentation, ProjectionMode::Identity),
        PrimitiveKind::Number,
    );

    let paths: [Arc<[TypeBodyPathStep]>; 4] = [
        Arc::from(
            [
                TypeBodyPathStep::TupleElement { ordinal: 0 },
                TypeBodyPathStep::Member { ordinal: 0 },
                TypeBodyPathStep::IndexSignatureValue,
            ]
            .as_slice(),
        ),
        Arc::from([TypeBodyPathStep::TupleElement { ordinal: 1 }].as_slice()),
        Arc::from([TypeBodyPathStep::FunctionParam { ordinal: 0 }].as_slice()),
        Arc::from([TypeBodyPathStep::FunctionReturn].as_slice()),
    ];
    for path in paths {
        let symbol = if matches!(path.first(), Some(TypeBodyPathStep::TupleElement { .. })) {
            "Places"
        } else {
            "Maker"
        };
        assert_primitive(
            &host,
            force(
                &dispatch,
                &instantiate(locator(symbol, path)),
                ProjectionMode::Identity,
            ),
            PrimitiveKind::Number,
        );
    }
}

#[test]
fn chained_generic_default_applies_the_earlier_parameters_own_default() {
    let host = make_host();
    upsert(&host, "type Chained<T = string, U = T> = U;\n");
    let dispatch = ProjectSemanticDispatch::new(&host);
    // No substitution args: `U` defaults to `T`, and `T` itself must still
    // resolve through ITS OWN default (`string`) rather than surfacing as an
    // unresolved `T` shell — the default chain is a case of a LATER
    // parameter's default referencing an EARLIER one that the body never
    // names directly.
    assert_primitive(
        &host,
        force(
            &dispatch,
            &mint(&dispatch, whole("Chained")),
            ProjectionMode::Identity,
        ),
        PrimitiveKind::String,
    );
}

#[test]
fn value_space_function_parameter_and_return_operands_are_supported() {
    let host = make_host();
    upsert(
        &host,
        "export function value(input: string): number { return 1; }",
    );
    let dispatch = ProjectSemanticDispatch::new(&host);
    for (step, expected) in [
        (
            TypeBodyPathStep::FunctionParam { ordinal: 0 },
            PrimitiveKind::String,
        ),
        (TypeBodyPathStep::FunctionReturn, PrimitiveKind::Number),
    ] {
        let operand = mint(
            &dispatch,
            locator_at(
                OWNER,
                "value",
                LocatorSymbolSpace::Value,
                Arc::from([TypeBodyPathStep::ValueSignature { ordinal: 0 }, step]),
            ),
        );
        assert_primitive(
            &host,
            force(&dispatch, &operand, ProjectionMode::Identity),
            expected,
        );
    }
}

#[test]
fn value_space_generic_function_signature_binds_its_own_type_parameter() {
    let host = make_host();
    upsert(
        &host,
        "export function generic<T extends string = \"x\">(input: T): T { return input; }",
    );
    let dispatch = ProjectSemanticDispatch::new(&host);
    let param_operand = mint(
        &dispatch,
        locator_at(
            OWNER,
            "generic",
            LocatorSymbolSpace::Value,
            Arc::from([
                TypeBodyPathStep::ValueSignature { ordinal: 0 },
                TypeBodyPathStep::FunctionParam { ordinal: 0 },
            ]),
        ),
    );
    let return_operand = mint(
        &dispatch,
        locator_at(
            OWNER,
            "generic",
            LocatorSymbolSpace::Value,
            Arc::from([
                TypeBodyPathStep::ValueSignature { ordinal: 0 },
                TypeBodyPathStep::FunctionReturn,
            ]),
        ),
    );
    let whole_signature_operand = mint(
        &dispatch,
        locator_at(
            OWNER,
            "generic",
            LocatorSymbolSpace::Value,
            Arc::from([TypeBodyPathStep::ValueSignature { ordinal: 0 }]),
        ),
    );
    let param_node = force(&dispatch, &param_operand, ProjectionMode::Identity);
    let return_node = force(&dispatch, &return_operand, ProjectionMode::Identity);
    // The parameter and return positions both reference the function's OWN
    // `T` — the same interned binder, not two detached shells minted
    // independently per selected position.
    assert_eq!(
        param_node, return_node,
        "the parameter and return positions must bind the SAME function-owned `T`"
    );
    assert_type_param(&host, param_node);
    // The bound CONTENT (`T extends string = "x"`) lives on the whole
    // signature's own `TypeParamDecl` list, keyed by the SAME binder node
    // `param`/`return` resolved to — never on the occurrence node itself
    // (`BinderIdentityMode::FunctionSignature` mints a bound-free binder).
    // Forcing the whole signature and finding that entry proves the
    // function's own constraint/default survive alongside the shared
    // binder, rather than being detached when a sub-position is forced.
    match host
        .project_type_store()
        .semantic_graph()
        .node_data(force(
            &dispatch,
            &whole_signature_operand,
            ProjectionMode::Identity,
        ))
        .as_deref()
    {
        Some(SemanticNodeData::Signature {
            type_parameters, ..
        }) => {
            let decl = type_parameters
                .iter()
                .find(|decl| decl.param == param_node)
                .unwrap_or_else(|| {
                    panic!(
                        "whole-signature type parameters must list the SAME binder node the \
                         parameter/return positions resolved to: {type_parameters:?}"
                    )
                });
            assert!(
                decl.constraint.is_some(),
                "forcing the signature must not lose the function's own `T extends string` \
                 constraint"
            );
            assert!(
                decl.default.is_some(),
                "forcing the signature must not lose the function's own `T = \"x\"` default"
            );
        }
        other => panic!("expected a function signature node, got {other:?}"),
    }
}

#[test]
fn substituted_authored_identity_includes_its_runtime_confinement() {
    let host = make_host();
    upsert(&host, SOURCE_V1);
    let dispatch = ProjectSemanticDispatch::new(&host);
    let forced = force_result(
        &dispatch,
        &mint(&dispatch, whole("Text")),
        ProjectionMode::Identity,
    );
    let argument = dispatch.mint_node_semantic_operand(&forced).unwrap();
    let operand = dispatch
        .mint_authored_semantic_operand(member_value("Box", 0), Arc::from([argument]))
        .unwrap();
    let SemanticOperandParts::Authored(authored) =
        operand.parts(SemanticOperandAuthority::mint_for_forcing_boundary())
    else {
        unreachable!()
    };
    let (store, generation) = authored
        .substitution_runtime()
        .expect("substituted operand must be runtime-confined");
    let other_runtime = operand.with_substitution_runtime(
        Some((store.wrapping_add(1), generation)),
        SemanticOperandAuthority::mint_for_forcing_boundary(),
    );
    assert_ne!(operand, other_runtime);
}

#[test]
fn foreign_substitution_nodes_cannot_be_minted() {
    let host = make_host();
    upsert(&host, SOURCE_V1);
    let dispatch = ProjectSemanticDispatch::new(&host);
    let foreign_host = make_host();
    upsert(&foreign_host, SOURCE_V1);
    let foreign_dispatch = ProjectSemanticDispatch::new(&foreign_host);
    let foreign_forced = force_result(
        &foreign_dispatch,
        &mint(&foreign_dispatch, whole("Text")),
        ProjectionMode::Identity,
    );
    let foreign = foreign_dispatch
        .mint_node_semantic_operand(&foreign_forced)
        .unwrap();
    assert!(matches!(
        dispatch.mint_authored_semantic_operand(member_value("Box", 0), Arc::from([foreign])),
        Err(SemanticOperandMintError::ForeignNode)
    ));
}

#[test]
fn authored_substitution_arguments_are_rejected_as_unbound() {
    // A substitution argument must already be a forced runtime NODE
    // operand — an `Authored` (not-yet-forced) operand supplied instead
    // is a distinct failure from a foreign store/generation node: it was
    // never bound to a concrete node at all.
    let host = make_host();
    upsert(&host, SOURCE_V1);
    let dispatch = ProjectSemanticDispatch::new(&host);
    let unforced = mint(&dispatch, whole("Text"));
    assert!(matches!(
        dispatch.mint_authored_semantic_operand(member_value("Box", 0), Arc::from([unforced])),
        Err(SemanticOperandMintError::UnboundSubstitution)
    ));
}

#[test]
fn node_operand_is_confined_to_store_and_generation() {
    let host = make_host();
    upsert(&host, SOURCE_V1);
    let dispatch = ProjectSemanticDispatch::new(&host);
    let forced = force_result(
        &dispatch,
        &mint(&dispatch, whole("Owned")),
        ProjectionMode::Shallow,
    );
    let operand = dispatch.mint_node_semantic_operand(&forced).unwrap();

    let other = make_host();
    upsert(&other, SOURCE_V1);
    let other_dispatch = ProjectSemanticDispatch::new(&other);
    assert_operand_error(
        other_dispatch.force_semantic_operand(&operand, request(ProjectionMode::Shallow)),
        QueryError::ForeignSemanticOperand,
    );

    host.project_type_store().bump_project_generation();
    assert_operand_error(
        dispatch.force_semantic_operand(&operand, request(ProjectionMode::Shallow)),
        QueryError::StaleSemanticOperand,
    );
}

#[test]
fn mint_node_semantic_operand_rejects_a_foreign_store_and_generation() {
    // Regression coverage for `mint_node_semantic_operand`'s own
    // store/generation confinement check: distinct from
    // `node_operand_is_confined_to_store_and_generation` (which exercises
    // the check inside `force_semantic_operand`), this proves the SEALING
    // boundary itself refuses to mint a runtime-handle operand for a node
    // produced by a different graph/generation, before any force is ever
    // attempted.
    let host = make_host();
    upsert(&host, SOURCE_V1);
    let dispatch = ProjectSemanticDispatch::new(&host);

    let foreign_host = make_host();
    upsert(&foreign_host, SOURCE_V1);
    let foreign_dispatch = ProjectSemanticDispatch::new(&foreign_host);
    let foreign_forced = force_result(
        &foreign_dispatch,
        &mint(&foreign_dispatch, whole("Text")),
        ProjectionMode::Identity,
    );

    assert!(matches!(
        dispatch.mint_node_semantic_operand(&foreign_forced),
        Err(SemanticOperandMintError::ForeignNode)
    ));

    let stale_forced = force_result(
        &dispatch,
        &mint(&dispatch, whole("Text")),
        ProjectionMode::Identity,
    );
    host.project_type_store().bump_project_generation();
    assert!(matches!(
        dispatch.mint_node_semantic_operand(&stale_forced),
        Err(SemanticOperandMintError::ForeignNode)
    ));
}

#[test]
fn node_operand_merges_producer_roots_into_active_candidate() {
    let host = make_host();
    upsert(&host, SOURCE_V1);
    let dispatch = ProjectSemanticDispatch::new(&host);
    let forced = force_result(
        &dispatch,
        &mint(&dispatch, whole("Owned")),
        ProjectionMode::Identity,
    );
    let node = forced.node();
    let operand = dispatch.mint_node_semantic_operand(&forced).unwrap();
    let SemanticOperandParts::Node { evidence, .. } =
        operand.parts(SemanticOperandAuthority::mint_for_forcing_boundary())
    else {
        unreachable!()
    };
    assert!(!evidence.read_set().facts.is_empty());
    assert!(evidence
        .self_roots()
        .iter()
        .any(|(canonical, _)| canonical.as_ref() == OWNER));

    let frame = BuildLocalTaintGuard::push(&dispatch.build_local_taint);
    let projected = force(&dispatch, &operand, ProjectionMode::Expanded);
    let observed = frame.finish();
    assert!(observed
        .observed_self_roots
        .iter()
        .any(|(canonical, _)| canonical.as_ref() == OWNER));
    assert_eq!(projected, node);
    let key = SemanticQueryKey::ProjectPath {
        base: node,
        path: Arc::from([]),
        context: ProjectionReductionContext::published(ProjectionMode::Expanded),
    };
    let graph = host.project_type_store().semantic_graph();
    let candidate_read_set = graph
        .entry_read_set_signature_for_tests(&key)
        .expect("forced projection candidate must publish");
    assert!(evidence
        .read_set()
        .facts
        .iter()
        .all(|fact| candidate_read_set.facts.contains(fact)));
    assert!(graph
        .entry_self_root_canonicals_for_tests(&key)
        .expect("forced projection must retain producer roots")
        .iter()
        .any(|canonical| canonical.as_ref() == OWNER));

    upsert(&host, SOURCE_V2);
    assert_operand_error(
        dispatch.force_semantic_operand(&operand, request(ProjectionMode::Expanded)),
        QueryError::StaleSemanticOperand,
    );
}

#[test]
fn concurrent_identical_forces_join_the_existing_graph_flight() {
    let host = Arc::new(make_host());
    upsert(&host, SOURCE_V1);
    let dispatch = ProjectSemanticDispatch::new(host.as_ref());
    let locator = member_value("Owned", 0);
    let operand = mint(&dispatch, locator.clone());
    let key = locator_key(&host, &dispatch, locator);
    let force_context = ProjectionReductionContext::published(ProjectionMode::Identity);
    let forced_key = force_key(&dispatch, &operand, force_context);
    let (entered_tx, entered_rx) = std::sync::mpsc::channel();
    let (release_tx, release_rx) = std::sync::mpsc::channel();
    let release_rx = parking_lot::Mutex::new(release_rx);
    *host.test_force.semantic_operand_cold_build_seam.0.lock() = Some(Arc::new(move || {
        entered_tx.send(()).expect("winner entry receiver");
        release_rx
            .lock()
            .recv_timeout(std::time::Duration::from_secs(10))
            .expect("winner release");
    }));

    let winner_host = Arc::clone(&host);
    let winner_operand = operand.clone();
    let winner = std::thread::spawn(move || {
        ProjectSemanticDispatch::new(winner_host.as_ref())
            .force_semantic_operand(&winner_operand, request(ProjectionMode::Identity))
    });
    entered_rx
        .recv_timeout(std::time::Duration::from_secs(10))
        .expect("winner must enter the cold build");

    let joiner_host = Arc::clone(&host);
    let joiner_operand = operand.clone();
    let joiner = std::thread::spawn(move || {
        ProjectSemanticDispatch::new(joiner_host.as_ref())
            .force_semantic_operand(&joiner_operand, request(ProjectionMode::Identity))
    });
    let graph = host.project_type_store().semantic_graph();
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
    while graph.test_joiner_on_condvar_count() == 0 && std::time::Instant::now() < deadline {
        std::thread::yield_now();
    }
    assert!(graph.test_joiner_on_condvar_count() > 0);
    release_tx.send(()).expect("release winner");
    let winner = join_within(winner, "operand winner");
    let joiner = join_within(joiner, "operand joiner");
    match (winner, joiner) {
        (QueryResult::Value(winner), QueryResult::Value(joiner)) => {
            assert_eq!(winner.node(), joiner.node())
        }
        other => panic!("both callers must receive the winning value, got {other:?}"),
    }
    // The nested `LowerLocator` step the authored build dispatches en route
    // must dedup...
    assert_eq!(graph.slot_candidate_count_for_tests(&key), 1);
    // ...and so must the authored `Instantiate` family `force_semantic_operand`
    // itself actually publishes — the concurrent joiner's whole point is
    // that TWO callers forcing the SAME operand share ONE winning candidate
    // at the family the force API publishes, not merely at a query nested
    // underneath it.
    assert_eq!(graph.slot_candidate_count_for_tests(&forced_key), 1);
    assert!(graph.stats_snapshot().joined_waits >= 1);
}

#[test]
fn cancellation_before_entry_performs_zero_semantic_work() {
    let host = make_host();
    upsert(&host, SOURCE_V1);
    let dispatch = ProjectSemanticDispatch::new(&host);
    let operand = mint(&dispatch, whole("Owned"));
    let ctx = RequestContext::new(7, Arc::from(OWNER), false, None);
    let _request_guard = RequestContextGuard::install(Arc::clone(&ctx));
    ctx.cancel();
    let _trace = enable_dispatch_trace_for_test();
    let before = host.project_type_store().semantic_graph().node_count();
    assert!(matches!(
        dispatch.force_semantic_operand(&operand, request(ProjectionMode::Expanded)),
        QueryResult::Error(QueryError::Cancelled)
    ));
    assert_eq!(
        before,
        host.project_type_store().semantic_graph().node_count()
    );
    DISPATCH_TRACE.with(|trace| assert!(trace.borrow().is_empty()));
    assert_eq!(ctx.projection_budget.projection_ops_executed_count(), 0);
}

#[test]
fn force_boundary_and_nested_dispatches_are_budget_charged() {
    let host = make_host();
    upsert(&host, SOURCE_V1);
    let dispatch = ProjectSemanticDispatch::new(&host);
    let operand = mint(&dispatch, whole("Owned"));
    let ctx = RequestContext::with_kind_timing_and_projection_budget(
        8,
        Arc::from(OWNER),
        verter_audit::RequestKind::ComponentMeta,
        false,
        false,
        None,
        100,
    );
    let _guard = RequestContextGuard::install(Arc::clone(&ctx));
    let before = ctx.projection_budget.projection_ops_executed_count();
    let _ = force(&dispatch, &operand, ProjectionMode::Expanded);
    // Two charges minimum: the force's own entry charge plus the nested
    // declaration `Instantiate` cold build's projection charge (a
    // whole-declaration force IS that build — the authored locator walk
    // with its per-step operand charges belongs to nested positions).
    assert!(ctx.projection_budget.projection_ops_executed_count() >= before + 2);
    // The force's one dispatched query (the declaration `Instantiate`) is
    // audit-attributed as a type-resolution hop.
    assert!(ctx.type_resolution_hops.load(Ordering::Relaxed) >= 1);

    drop(_guard);
    let limited_host = make_host();
    upsert(&limited_host, SOURCE_V1);
    let limited_dispatch = ProjectSemanticDispatch::new(&limited_host);
    let locator = whole("Owned");
    let key = locator_key(&limited_host, &limited_dispatch, locator.clone());
    let operand = mint(&limited_dispatch, locator);
    let limited = RequestContext::with_kind_timing_and_projection_budget(
        9,
        Arc::from(OWNER),
        verter_audit::RequestKind::ComponentMeta,
        false,
        false,
        None,
        1,
    );
    let _limited_guard = RequestContextGuard::install(Arc::clone(&limited));
    assert!(matches!(
        limited_dispatch.force_semantic_operand(&operand, request(ProjectionMode::Expanded)),
        QueryResult::Error(QueryError::BudgetExceeded(_))
    ));
    assert_eq!(limited.projection_budget.projection_ops_executed_count(), 2);
    assert_eq!(
        limited_host
            .project_type_store()
            .semantic_graph()
            .slot_candidate_count_for_tests(&key),
        0
    );
}

#[test]
fn binder_scan_charges_once_per_force_not_per_visited_node() {
    // The post-lowering binder-recovery walk is bounded by its visited set,
    // and one force performs exactly one scan. Charging per VISITED node
    // would scale the force's projection charge with the lowered body's
    // WIDTH — a sibling-heavy object could trip the projection budget on
    // scan volume alone, without the force performing any additional
    // semantic work. The two fixtures differ only in member count, so the
    // per-force charge — and every other charge that scales with the
    // force's dispatch structure — must come out equal.
    let spent_on = |source: &str| {
        let host = make_host();
        upsert(&host, source);
        let dispatch = ProjectSemanticDispatch::new(&host);
        let ctx = RequestContext::with_kind_timing_and_projection_budget(
            11,
            Arc::from(OWNER),
            verter_audit::RequestKind::ComponentMeta,
            false,
            false,
            None,
            100_000,
        );
        let _guard = RequestContextGuard::install(Arc::clone(&ctx));
        let before = ctx.projection_budget.projection_ops_executed_count();
        force(
            &dispatch,
            &mint(&dispatch, whole("Probe")),
            ProjectionMode::Identity,
        );
        ctx.projection_budget.projection_ops_executed_count() - before
    };
    let narrow = spent_on("export type Probe = { a: string };\n");
    let wide = spent_on(
        "export type Probe = {\n\
             a: string; b: number; c: boolean; d: bigint; e: symbol; f: \"x\"; g: \"y\";\n\
             h: \"z\"; i: null; j: undefined; k: void; l: never; m: object; n: string;\n\
             o: number; p: boolean; q: bigint; r: symbol;\n\
         };\n",
    );
    assert_eq!(
        narrow, wide,
        "a wider lowered body must not cost more projection charges than a narrow one \
         (narrow spent {narrow}, wide spent {wide})"
    );
}

#[test]
fn cancellation_discovered_inside_the_cold_build_never_warms() {
    let host = make_host();
    upsert(&host, SOURCE_V1);
    let dispatch = ProjectSemanticDispatch::new(&host);
    let locator = whole("Owned");
    let key = locator_key(&host, &dispatch, locator.clone());
    let force_key = force_key(
        &dispatch,
        &mint(&dispatch, locator.clone()),
        ProjectionReductionContext::published(ProjectionMode::Expanded),
    );
    let operand = mint(&dispatch, locator);
    let ctx = RequestContext::new(10, Arc::from(OWNER), false, None);
    let cancel = Arc::clone(&ctx);
    *host.test_force.semantic_operand_cold_build_seam.0.lock() =
        Some(Arc::new(move || cancel.cancel()));
    let _guard = RequestContextGuard::install(ctx);
    assert!(matches!(
        dispatch.force_semantic_operand(&operand, request(ProjectionMode::Expanded)),
        QueryResult::Error(QueryError::Cancelled)
    ));
    // Both the nested `LowerLocator` slot AND the outer `Instantiate`
    // (force) family slot must stay unwarmed — a cancellation surfacing
    // through the force wrapper must not leave the nested candidate
    // family behind as a stray warm entry either.
    assert_eq!(
        host.project_type_store()
            .semantic_graph()
            .slot_candidate_count_for_tests(&key),
        0
    );
    assert_eq!(
        host.project_type_store()
            .semantic_graph()
            .slot_candidate_count_for_tests(&force_key),
        0
    );
}

#[test]
fn partial_and_signature_overflow_discovered_during_force_never_warm() {
    for overflow in [false, true] {
        let host = make_host();
        upsert(&host, SOURCE_V1);
        let dispatch = ProjectSemanticDispatch::new(&host);
        let locator = whole("Owned");
        let operand = mint(&dispatch, locator.clone());
        let key = locator_key(&host, &dispatch, locator.clone());
        let force_key = force_key(
            &dispatch,
            &operand,
            ProjectionReductionContext::published(ProjectionMode::Expanded),
        );
        if overflow {
            host.test_force
                .force_fact_tracer_overflow_observations
                .store(
                    crate::resolver_core::FACT_SIGNATURE_CAP + 1,
                    Ordering::Relaxed,
                );
        } else {
            host.test_force
                .force_result_partial_for_tests
                .store(true, Ordering::Relaxed);
        }
        let result = dispatch.force_semantic_operand(&operand, request(ProjectionMode::Expanded));
        if overflow {
            assert!(matches!(
                result,
                QueryResult::Error(QueryError::SignatureOverflow)
            ));
        } else {
            assert!(matches!(
                result,
                QueryResult::Error(QueryError::IncompleteSemanticOperand { .. })
            ));
        }
        assert_eq!(dispatch_warm_for(&key), 0);
        assert_eq!(
            host.project_type_store()
                .semantic_graph()
                .slot_candidate_count_for_tests(&key),
            0
        );
        // The outer `Instantiate` (force) family slot the force wrapper
        // itself would have published to must ALSO stay at zero
        // candidates — a partial/overflow discovered mid-force must not
        // leave a stray warm `Instantiate` entry even though the nested
        // `LowerLocator` slot above is the one directly poisoned.
        assert_eq!(
            host.project_type_store()
                .semantic_graph()
                .slot_candidate_count_for_tests(&force_key),
            0
        );
    }
}

#[test]
fn complete_cache_suppressed_force_still_returns_its_typed_value() {
    let host = make_host();
    upsert(&host, "type Text = string;");
    let dispatch = ProjectSemanticDispatch::new(&host);
    let operand = mint(&dispatch, whole("Text"));
    let context = ProjectionReductionContext::published(ProjectionMode::Identity);
    let key = force_key(&dispatch, &operand, context);
    host.test_force
        .force_fenced_serve_for_tests
        .store(true, Ordering::Relaxed);
    let forced = force_with_context(&dispatch, &operand, context);
    assert_primitive(&host, forced.node(), PrimitiveKind::String);
    assert_eq!(
        host.project_type_store()
            .semantic_graph()
            .slot_candidate_count_for_tests(&key),
        0,
        "complete fenced value must flow without warming"
    );
}

#[test]
fn cold_force_dereferences_exact_locator_once_and_warm_family_does_not_grow() {
    let host = make_host();
    upsert(&host, SOURCE_V1);
    let dispatch = ProjectSemanticDispatch::new(&host);
    let locator = member_value("Owned", 0);
    let lower_key = locator_key(&host, &dispatch, locator.clone());
    let operand = mint(&dispatch, locator);
    let force_key = force_key(
        &dispatch,
        &operand,
        ProjectionReductionContext::published(ProjectionMode::Identity),
    );
    // bounded-loop: fixed three-call cold/warm/warm regression sequence.
    for _ in 0..3 {
        assert_primitive(
            &host,
            force(&dispatch, &operand, ProjectionMode::Identity),
            PrimitiveKind::String,
        );
    }
    assert_eq!(dispatch_cold_for(&lower_key), 1);
    assert_eq!(dispatch_warm_for(&lower_key), 0);
    assert_eq!(dispatch_cold_for(&force_key), 1);
    assert_eq!(dispatch_warm_for(&force_key), 2);
    assert_eq!(
        host.project_type_store()
            .semantic_graph()
            .slot_candidate_count_for_tests(&force_key),
        1
    );
}

#[test]
fn single_member_force_never_interns_sibling_members() {
    // A single-member selection whose path crosses NO binder must stay a
    // direct sub-expression lowering: exactly one new node (the `string`
    // primitive). The eager-ancestor-capture regression this guards would
    // instead lower the WHOLE declaration body to answer a single-member
    // request — interning an `Object` shell plus both large sibling members
    // (`b` and `c`, each with five properties) along the way, a node-count
    // delta an order of magnitude larger.
    //
    // The GENERIC leg is the discriminating one. A declaration's own header
    // parameters are NOT a reason to capture the ancestor: the lowering
    // caller builds the header binder frame from the derefed
    // `type_parameters` + `visibility` and lowers the selected
    // sub-expression under it, so `Framed.a` resolves `T` exactly as
    // `Framed`'s whole body would. Treating "the header declares
    // parameters" as an ancestor trigger would put EVERY member selection
    // of EVERY generic declaration on the whole-body route.
    const MEMBERS: &str = "\
    a: string; \
    b: { p: \"b0\"; q: \"b1\"; r: \"b2\"; s: \"b3\"; t: \"b4\" }; \
    c: { p: \"c0\"; q: \"c1\"; r: \"c2\"; s: \"c3\"; t: \"c4\" }; \
";
    for (symbol, header) in [("Plain", ""), ("Framed", "<T>")] {
        let host = make_host();
        upsert(
            &host,
            &format!("export type {symbol}{header} = {{ {MEMBERS} }};\n"),
        );
        let dispatch = ProjectSemanticDispatch::new(&host);
        let graph = || host.project_type_store().semantic_graph().node_count();

        let before = graph();
        assert_primitive(
            &host,
            force(
                &dispatch,
                &mint(&dispatch, member_value(symbol, 0)),
                ProjectionMode::Identity,
            ),
            PrimitiveKind::String,
        );
        let after_first = graph();
        if header.is_empty() {
            assert_eq!(
                after_first - before,
                1,
                "{symbol}: a binder-free selection must intern only the member's \
                 own value type"
            );
        }

        // Selecting the LARGE sibling afterwards must still be cold work:
        // its object plus five distinct string-literal members are six
        // nodes nothing has interned yet. An ancestor-capturing route would
        // have interned all of them while answering `a`, leaving nothing
        // for this second force to add. Counting the second force's delta
        // rather than the first's keeps the bound independent of the
        // per-leg binder-frame overhead (the generic leg additionally
        // interns its header `TypeParam`).
        let _ = force(
            &dispatch,
            &mint(&dispatch, member_value(symbol, 1)),
            ProjectionMode::Identity,
        );
        let after_sibling = graph();
        assert!(
            after_sibling - after_first >= 6,
            "{symbol}: forcing member `a` must not have interned sibling `b` \
             (sibling force added only {} nodes)",
            after_sibling - after_first
        );
    }
}

#[test]
fn content_edit_invalidates_unchanged_locator_identity() {
    let host = make_host();
    upsert(&host, SOURCE_V1);
    let dispatch = ProjectSemanticDispatch::new(&host);
    let locator = whole("Owned");
    let key = locator_key(&host, &dispatch, locator.clone());
    let operand = mint(&dispatch, locator);
    let before = force(&dispatch, &operand, ProjectionMode::Expanded);
    let before_len = match host
        .project_type_store()
        .semantic_graph()
        .node_data(before)
        .as_deref()
    {
        Some(SemanticNodeData::Object(surface)) => surface.positive_members().len(),
        other => panic!("expected object, got {other:?}"),
    };
    assert_eq!(before_len, 1);
    upsert(&host, SOURCE_V2);
    let after = force(&dispatch, &operand, ProjectionMode::Expanded);
    let after_len = match host
        .project_type_store()
        .semantic_graph()
        .node_data(after)
        .as_deref()
    {
        Some(SemanticNodeData::Object(surface)) => surface.positive_members().len(),
        other => panic!("expected object, got {other:?}"),
    };
    assert_eq!(after_len, 2);
    assert_eq!(key, locator_key(&host, &dispatch, whole("Owned")));
    // A whole-declaration force routes through the DECLARATION Instantiate
    // family, whose shell build re-sources the located body through the
    // ordinary declaration machinery — the locator family and the forced
    // family each run exactly ONE cold winner per content version, and the
    // fresh and incremental answers must stay identical.
    let forced_family = force_key(
        &dispatch,
        &operand,
        ProjectionReductionContext::published(ProjectionMode::Expanded),
    );
    assert_eq!(dispatch_cold_for(&key), 2);
    assert_eq!(dispatch_cold_for(&forced_family), 2);

    let incremental = crate::typeinfo::raise::render_node_display_with_ctx(&host, after)
        .expect("incremental result must render");
    let fresh_host = make_host();
    upsert(&fresh_host, SOURCE_V2);
    let fresh_dispatch = ProjectSemanticDispatch::new(&fresh_host);
    let fresh = force(
        &fresh_dispatch,
        &mint(&fresh_dispatch, whole("Owned")),
        ProjectionMode::Expanded,
    );
    let fresh = crate::typeinfo::raise::render_node_display_with_ctx(&fresh_host, fresh)
        .expect("fresh result must render");
    assert_eq!(incremental.text, fresh.text);
    assert_eq!(incremental.degraded, fresh.degraded);
}

#[test]
fn substitution_producer_roots_reach_the_forced_candidate_but_not_its_locator_child() {
    // A forced operand's own candidate must carry the producer roots of the
    // runtime nodes it was substituted with, because those nodes are part of
    // the answer it publishes. The `LowerLocator` build the force triggers
    // underneath is SUBSTITUTION-INDEPENDENT — it lowers the declaration's
    // authored shape and reads nothing from the substituted node's file — so
    // inheriting that root would over-root an unrelated shared entry: every
    // later reader of the same locator shape would then be invalidated by
    // edits to a file its value never depended on.
    let host = make_host();
    upsert(&host, SOURCE_V1);
    upsert_at(&host, OTHER_OWNER, "export type Arg = string;\n");
    let dispatch = ProjectSemanticDispatch::new(&host);

    let argument_forced = force_result(
        &dispatch,
        &mint(
            &dispatch,
            locator_at(OTHER_OWNER, "Arg", LocatorSymbolSpace::Type, Arc::from([])),
        ),
        ProjectionMode::Identity,
    );
    let argument = dispatch
        .mint_node_semantic_operand(&argument_forced)
        .expect("argument operand must seal");
    let SemanticOperandParts::Node { evidence, .. } =
        argument.parts(SemanticOperandAuthority::mint_for_forcing_boundary())
    else {
        unreachable!()
    };
    assert!(
        evidence
            .self_roots()
            .iter()
            .any(|(canonical, _)| canonical.as_ref() == OTHER_OWNER),
        "the argument's producer roots must name its own file"
    );

    // A nested member position of `Box<T>` (the authored-source route a
    // whole-declaration force no longer takes): its `LowerLocator` child
    // stays the substitution-independent leg of the invariant.
    let boxed_locator = member_value("Box", 0);
    let lower_key = locator_key(&host, &dispatch, boxed_locator.clone());
    let operand = dispatch
        .mint_authored_semantic_operand(boxed_locator, Arc::from([argument]))
        .expect("substituted operand must seal");
    let context = ProjectionReductionContext::published(ProjectionMode::Expanded);
    let forced_key = force_key(&dispatch, &operand, context);
    let _ = force_with_context(&dispatch, &operand, context);

    let graph = host.project_type_store().semantic_graph();
    let forced_roots = graph
        .entry_self_root_canonicals_for_tests(&forced_key)
        .expect("the forced operand must publish a candidate");
    assert!(
        forced_roots
            .iter()
            .any(|canonical| canonical.as_ref() == OTHER_OWNER),
        "the forced candidate must retain the substituted node's producer root"
    );
    let lower_roots = graph
        .entry_self_root_canonicals_for_tests(&lower_key)
        .expect("the locator lowering must publish its own candidate");
    assert!(
        !lower_roots
            .iter()
            .any(|canonical| canonical.as_ref() == OTHER_OWNER),
        "the substitution-independent locator child must not inherit the \
         substituted node's producer root, got {lower_roots:?}"
    );
}

#[test]
fn a_forced_operand_returns_its_own_producer_roots_even_on_a_warm_candidate() {
    // The node arm's key is `ProjectPath { base, [], context }` — it does
    // NOT encode the operand's evidence. Two node operands can therefore
    // reach ONE key carrying DIFFERENT producer roots: substituting
    // `Box<T>`'s `value` position with the node produced from another
    // file's `Arg` yields exactly that node back, so the substituted force
    // and the direct `Arg` force publish the same node — the first rooted
    // at BOTH files, the second at `Arg`'s file alone.
    //
    // Only the COLD winner's build observes the injected producer
    // evidence. Forcing the narrower operand first makes the wider one a
    // warm hit, which would otherwise be handed the narrow candidate's
    // roots and silently drop its own owner file — so a
    // `mint -> force -> mint` chain would lose a dependency it genuinely
    // has. Unioning the input operand's evidence into what the force
    // RETURNS is what keeps its own output path-independent.
    let host = make_host();
    upsert(&host, SOURCE_V1);
    upsert_at(&host, OTHER_OWNER, "export type Arg = string;\n");
    let dispatch = ProjectSemanticDispatch::new(&host);

    let argument_forced = force_result(
        &dispatch,
        &mint(
            &dispatch,
            locator_at(OTHER_OWNER, "Arg", LocatorSymbolSpace::Type, Arc::from([])),
        ),
        ProjectionMode::Identity,
    );
    let narrow = dispatch
        .mint_node_semantic_operand(&argument_forced)
        .expect("argument operand must seal");

    // `Box<T> = { value: T }` lives in OWNER; substituting `T` with the
    // OTHER_OWNER node reproduces that node under a force rooted at both.
    let substituted = dispatch
        .mint_authored_semantic_operand(member_value("Box", 0), Arc::from([narrow.clone()]))
        .expect("substituted operand must seal");
    let wide_forced = force_result(&dispatch, &substituted, ProjectionMode::Identity);
    assert_eq!(
        wide_forced.node(),
        argument_forced.node(),
        "the fixture requires the substituted force to reproduce the argument node"
    );
    let wide = dispatch
        .mint_node_semantic_operand(&wide_forced)
        .expect("substituted node operand must seal");

    let names = |forced: &ForcedSemanticOperand, canonical: &str| {
        forced
            .evidence()
            .self_roots()
            .iter()
            .any(|(root, _)| root.as_ref() == canonical)
    };
    assert!(
        names(&wide_forced, OWNER) && names(&wide_forced, OTHER_OWNER),
        "the fixture requires the wider operand to carry both producer roots, got {:?}",
        wide_forced.evidence().self_roots()
    );

    // NARROW first: it wins the cold build for the shared key and roots
    // the candidate at its file alone.
    let context = ProjectionReductionContext::published(ProjectionMode::Expanded);
    let cold = force_with_context(&dispatch, &narrow, context);
    assert!(names(&cold, OTHER_OWNER));

    // WIDE second: a warm hit on that narrower candidate, which must not
    // shrink the roots the operand itself carries.
    let warm = force_with_context(&dispatch, &wide, context);
    assert!(
        names(&warm, OWNER) && names(&warm, OTHER_OWNER),
        "a warm force must return its own operand's producer roots, got {:?}",
        warm.evidence().self_roots()
    );
}

#[test]
fn the_force_evidence_channel_is_scoped_and_retains_nothing() {
    // The force's operand-evidence channel is a SCOPED stack for one
    // in-flight force — never a second in-flight table and never a
    // request-local memo. Two observable consequences: it is empty
    // whenever no force is in flight, and repeating identical forces on
    // one dispatch never accumulates entries in it (a memo would grow).
    let host = make_host();
    upsert(&host, SOURCE_V1);
    let dispatch = ProjectSemanticDispatch::new(&host);
    assert!(dispatch.active_operand_evidence.borrow().is_empty());

    let forced = force_result(
        &dispatch,
        &mint(&dispatch, whole("Owned")),
        ProjectionMode::Identity,
    );
    let node_operand = dispatch
        .mint_node_semantic_operand(&forced)
        .expect("node operand must seal");
    // bounded-loop: four fixed repetitions of one force pair.
    for _ in 0..4 {
        let _ = force(
            &dispatch,
            &mint(&dispatch, whole("Owned")),
            ProjectionMode::Expanded,
        );
        let _ = force(&dispatch, &node_operand, ProjectionMode::Expanded);
        assert!(
            dispatch.active_operand_evidence.borrow().is_empty(),
            "the force evidence channel must unwind to empty after every force"
        );
    }
}

#[test]
fn sealing_refuses_surplus_arguments_and_out_of_range_bound_ordinals() {
    // Two fail-closed admissions at the seal, both reading the anchor
    // declaration's DECLARED header arity.
    //
    // A surplus substitution argument binds no parameter, yet it still
    // enters the family key — admitting it would fragment the cache across
    // distinct keys holding identical values while telling the caller
    // nothing. Supplying FEWER stays legal: the unsupplied parameters fall
    // back to their own declared defaults.
    //
    // A bound locator naming a parameter ordinal the declaration does not
    // declare is the second: it is what makes the authority-derived binder
    // frame load-bearing rather than a classifier, since the refusal reads
    // the frame's ordinal. Deferring it would surface as an anonymous
    // body-deref miss at force time.
    let host = make_host();
    upsert(
        &host,
        "export type Text = string;\n\
         export type Box<T> = { value: T };\n\
         export type Bound<T extends string> = T;\n\
         export type Plain = { a: string };\n",
    );
    let dispatch = ProjectSemanticDispatch::new(&host);
    let argument = dispatch
        .mint_node_semantic_operand(&force_result(
            &dispatch,
            &mint(&dispatch, whole("Text")),
            ProjectionMode::Identity,
        ))
        .expect("argument operand must seal");

    assert_eq!(
        dispatch.mint_authored_semantic_operand(
            whole("Box"),
            Arc::from([argument.clone(), argument.clone()]),
        ),
        Err(SemanticOperandMintError::SubstitutionArity {
            expected: 1,
            actual: 2
        })
    );
    assert_eq!(
        dispatch.mint_authored_semantic_operand(whole("Plain"), Arc::from([argument.clone()])),
        Err(SemanticOperandMintError::SubstitutionArity {
            expected: 0,
            actual: 1
        })
    );
    assert_eq!(
        dispatch.mint_authored_semantic_operand(
            locator(
                "Bound",
                Arc::from([TypeBodyPathStep::TypeParamBound {
                    ordinal: 1,
                    position: TypeParamBoundPosition::Constraint,
                }]),
            ),
            Arc::from([]),
        ),
        Err(SemanticOperandMintError::BoundOrdinalOutOfRange {
            ordinal: 1,
            declared: 1
        })
    );

    // Positive controls: the exact arity seals, an under-supply seals, and
    // the in-range bound frame seals and forces to the declared bound.
    assert!(dispatch
        .mint_authored_semantic_operand(whole("Box"), Arc::from([argument]))
        .is_ok());
    assert!(dispatch
        .mint_authored_semantic_operand(whole("Box"), Arc::from([]))
        .is_ok());
    let in_range = dispatch
        .mint_authored_semantic_operand(
            locator(
                "Bound",
                Arc::from([TypeBodyPathStep::TypeParamBound {
                    ordinal: 0,
                    position: TypeParamBoundPosition::Constraint,
                }]),
            ),
            Arc::from([]),
        )
        .expect("an in-range bound frame must seal");
    assert_primitive(
        &host,
        force(&dispatch, &in_range, ProjectionMode::Identity),
        PrimitiveKind::String,
    );

    // Value-space signatures and augmentation inner decls have a header
    // arity too: surplus arguments still enter InstantiateKey.args even
    // when they bind nothing.
    upsert(
        &host,
        "export type Text = string;\n\
         export function boxed<T>(value: T): T { return value; }\n\
         export function mixed<T>(value: T): T;\n\
         export function mixed(value: string): string;\n\
         export function mixed(value: unknown): unknown { return value; }\n\
         export const marker = { tag: \"m\" };\n\
         export class Holder<T> { value: T; }\n\
         declare module \"pkg\" { export type Box<T> = { value: T } }\n",
    );
    let dispatch = ProjectSemanticDispatch::new(&host);
    let argument = dispatch
        .mint_node_semantic_operand(&force_result(
            &dispatch,
            &mint(&dispatch, whole("Text")),
            ProjectionMode::Identity,
        ))
        .expect("argument operand must seal");
    assert_eq!(
        dispatch.mint_authored_semantic_operand(
            locator_at(
                OWNER,
                "boxed",
                LocatorSymbolSpace::Value,
                Arc::from([TypeBodyPathStep::ValueSignature { ordinal: 0 }]),
            ),
            Arc::from([argument.clone(), argument.clone()]),
        ),
        Err(SemanticOperandMintError::SubstitutionArity {
            expected: 1,
            actual: 2
        })
    );
    assert_eq!(
        dispatch.mint_authored_semantic_operand(
            locator_at(OWNER, "mixed", LocatorSymbolSpace::Value, Arc::from([])),
            Arc::from([argument.clone()]),
        ),
        Err(SemanticOperandMintError::SubstitutionArity {
            expected: 0,
            actual: 1
        })
    );
    // A value-space CONST declares no signature clause and no type-space
    // declaration: the durable rule leaves it at an Exact(0) header, so a
    // surplus argument is refused rather than admitted to fragment the
    // Instantiate family.
    assert_eq!(
        dispatch.mint_authored_semantic_operand(
            locator_at(OWNER, "marker", LocatorSymbolSpace::Value, Arc::from([]),),
            Arc::from([argument.clone()]),
        ),
        Err(SemanticOperandMintError::SubstitutionArity {
            expected: 0,
            actual: 1
        })
    );
    // A CLASS is dual-space: its value position declares no function
    // signature of its own, but its type-space header (`class Holder<T>`)
    // binds, so the arity comes from that prepared header.
    assert_eq!(
        dispatch.mint_authored_semantic_operand(
            locator_at(OWNER, "Holder", LocatorSymbolSpace::Value, Arc::from([]),),
            Arc::from([argument.clone(), argument.clone()]),
        ),
        Err(SemanticOperandMintError::SubstitutionArity {
            expected: 1,
            actual: 2
        })
    );
    assert_eq!(
        dispatch.mint_authored_semantic_operand(
            AuthoredBodyLocator::AugmentationBody(AugmentationBodyLocator {
                anchor: AuthoredAnchor {
                    canonical_id: Arc::from(OWNER),
                    owner: TopLevelOwnerId::ordinary_file(),
                    symbol: Arc::from("Box"),
                    space: LocatorSymbolSpace::Type,
                },
                scope: AuthoredAugmentationScope::Module {
                    specifier: Arc::from("pkg"),
                },
                path: Arc::from([]),
            }),
            Arc::from([argument.clone(), argument.clone()]),
        ),
        Err(SemanticOperandMintError::SubstitutionArity {
            expected: 1,
            actual: 2
        })
    );
    assert!(dispatch
        .mint_authored_semantic_operand(
            locator_at(
                OWNER,
                "boxed",
                LocatorSymbolSpace::Value,
                Arc::from([TypeBodyPathStep::ValueSignature { ordinal: 0 }]),
            ),
            Arc::from([argument.clone()]),
        )
        .is_ok());
    assert!(dispatch
        .mint_authored_semantic_operand(
            locator_at(OWNER, "Holder", LocatorSymbolSpace::Value, Arc::from([])),
            Arc::from([argument]),
        )
        .is_ok());
}

#[test]
fn non_class_value_header_arity_is_not_stolen_by_a_same_named_generic_type() {
    // A value declaration that merely SHARES a name with a generic type
    // declaration declares no header of its own: `function Foo()` has a
    // zero-arity value header even though `interface Foo<T>` exists. A
    // class is the one exception (its value position is the constructor
    // object of a genuinely generic class). Consulting the type-space
    // header for the function/const would seal a surplus argument that
    // binds nothing, fragmenting the value family across distinct
    // InstantiateKey.args holding identical values.
    let host = make_host();
    upsert(
        &host,
        "export type Unit = string;\n\
         export function Foo(): void;\n\
         export interface Foo<T> { x: T }\n\
         export const plain = 1;\n\
         export interface Plain<T> { x: T }\n",
    );
    let dispatch = ProjectSemanticDispatch::new(&host);
    let unit_forced = force_result(
        &dispatch,
        &mint(&dispatch, whole("Unit")),
        ProjectionMode::Identity,
    );
    let unit = dispatch
        .mint_node_semantic_operand(&unit_forced)
        .expect("argument operand must seal");
    for symbol in ["Foo", "plain"] {
        assert_eq!(
            dispatch.mint_authored_semantic_operand(
                locator_at(OWNER, symbol, LocatorSymbolSpace::Value, Arc::from([])),
                Arc::from([unit.clone()]),
            ),
            Err(SemanticOperandMintError::SubstitutionArity {
                expected: 0,
                actual: 1
            }),
            "{symbol}: a non-class value header must stay at its own arity"
        );
        // The same anchor with NO arguments still seals.
        assert!(dispatch
            .mint_authored_semantic_operand(
                locator_at(OWNER, symbol, LocatorSymbolSpace::Value, Arc::from([])),
                Arc::from([]),
            )
            .is_ok());
    }
    // The TYPE-space anchor of the same name keeps the interface's header:
    // one argument seals, two are refused.
    assert!(dispatch
        .mint_authored_semantic_operand(whole("Foo"), Arc::from([unit.clone()]))
        .is_ok());
    assert_eq!(
        dispatch
            .mint_authored_semantic_operand(whole("Foo"), Arc::from([unit.clone(), unit.clone()]),),
        Err(SemanticOperandMintError::SubstitutionArity {
            expected: 1,
            actual: 2
        })
    );
}

#[test]
fn an_unreadable_header_arity_is_a_typed_mint_refusal() {
    // There is no "defer the surplus to force" seal state: a locator whose
    // authored header cannot be read authoritatively is refused AT THE
    // SEAL with a typed error. A value-signature ordinal past the
    // declaration's overload group addresses an authored position that
    // does not exist; an augmentation anchor whose inner declaration (or
    // whole owning bundle) is absent has no binder frame to seal. Both
    // would otherwise mint an operand whose surplus arguments hash into
    // InstantiateKey.args while binding nothing.
    let host = make_host();
    upsert(
        &host,
        "export type Unit = string;\n\
         export function solo(): void;\n\
         declare module \"pkg\" { export type Present<T> = { v: T } }\n",
    );
    let dispatch = ProjectSemanticDispatch::new(&host);
    let unit_forced = force_result(
        &dispatch,
        &mint(&dispatch, whole("Unit")),
        ProjectionMode::Identity,
    );
    let unit = dispatch
        .mint_node_semantic_operand(&unit_forced)
        .expect("argument operand must seal");

    assert_eq!(
        dispatch.mint_authored_semantic_operand(
            locator_at(
                OWNER,
                "solo",
                LocatorSymbolSpace::Value,
                Arc::from([TypeBodyPathStep::ValueSignature { ordinal: 3 }]),
            ),
            Arc::from([]),
        ),
        Err(SemanticOperandMintError::UnresolvedLocatorPath)
    );

    let augmentation = |symbol: &str, canonical: &str| {
        AuthoredBodyLocator::AugmentationBody(AugmentationBodyLocator {
            anchor: AuthoredAnchor {
                canonical_id: Arc::from(canonical),
                owner: TopLevelOwnerId::ordinary_file(),
                symbol: Arc::from(symbol),
                space: LocatorSymbolSpace::Type,
            },
            scope: AuthoredAugmentationScope::Module {
                specifier: Arc::from("pkg"),
            },
            path: Arc::from([]),
        })
    };
    // The inner declaration is absent from the augmentation scope.
    assert_eq!(
        dispatch.mint_authored_semantic_operand(augmentation("Ghost", OWNER), Arc::from([])),
        Err(SemanticOperandMintError::MissingAuthoredDeclaration)
    );
    // The owning canonical has no prepared bundle at all.
    assert_eq!(
        dispatch.mint_authored_semantic_operand(
            augmentation("Present", "/w/operand/nowhere.ts"),
            Arc::from([]),
        ),
        Err(SemanticOperandMintError::MissingAuthoredDeclaration)
    );
    // The PRESENT inner declaration still seals and enforces its arity.
    assert_eq!(
        dispatch.mint_authored_semantic_operand(
            augmentation("Present", OWNER),
            Arc::from([unit.clone(), unit.clone()]),
        ),
        Err(SemanticOperandMintError::SubstitutionArity {
            expected: 1,
            actual: 2
        })
    );
    assert!(dispatch
        .mint_authored_semantic_operand(augmentation("Present", OWNER), Arc::from([unit]))
        .is_ok());
}

#[test]
fn whole_declaration_force_shares_the_declaration_instantiate_memo() {
    // An empty-path type-space force IS the declaration's Instantiate —
    // the query the rest of the compiler already dispatches. Sealing it
    // under a dedicated authored source would fork the family and retain
    // the same (decl, args, context) answer twice; here the ordinary
    // declaration key and the force key must be ONE key, and the ordinary
    // dispatch joins the force's already-warm candidate instead of
    // running a second cold winner.
    let host = make_host();
    upsert(&host, SOURCE_V1);
    let dispatch = ProjectSemanticDispatch::new(&host);
    let text_forced = force_result(
        &dispatch,
        &mint(&dispatch, whole("Text")),
        ProjectionMode::Identity,
    );
    let text = dispatch
        .mint_node_semantic_operand(&text_forced)
        .expect("argument operand must seal");
    let context = ProjectionReductionContext::published(ProjectionMode::Expanded);
    let operand = dispatch
        .mint_authored_semantic_operand(whole("Box"), Arc::from([text.clone()]))
        .expect("substituted operand must seal");
    let _ = force_with_context(&dispatch, &operand, context);

    let ordinary_key = SemanticQueryKey::Instantiate(InstantiateKey::new(
        dispatch.type_slot_for(
            Arc::from(OWNER),
            TopLevelOwnerId::ordinary_file(),
            Arc::from("Box"),
        ),
        Arc::from([text_forced.node()]),
        dispatch.instantiate_context_for(OWNER, context),
    ));
    assert_eq!(
        force_key(&dispatch, &operand, context),
        ordinary_key,
        "the whole-declaration force key must BE the declaration Instantiate key"
    );
    assert_eq!(dispatch_cold_for(&ordinary_key), 1);
    let read = dispatch.execute_read(ordinary_key.clone());
    assert!(
        matches!(read.value, QueryResult::Value(_)),
        "the ordinary declaration dispatch must serve, got {:?}",
        read.value
    );
    assert_eq!(
        dispatch_cold_for(&ordinary_key),
        1,
        "the ordinary declaration dispatch must join the force's candidate, not build a second one"
    );
    assert_eq!(dispatch_warm_for(&ordinary_key), 1);
}

#[test]
fn incomplete_operand_refusals_spell_their_reasons_by_name() {
    // The component-meta-observable projection of a partial force names its
    // reason classes. Rendering the reason set's `Debug` shape instead
    // would leak a bitflag newtype's numeric representation into a string
    // consumers read, and would churn whenever a bit is added or reordered.
    use crate::resolver_core::component_meta_query_engine::semantic_query_error_raw;
    use crate::semantic_query::PartialReasonSet;

    assert_eq!(
        semantic_query_error_raw(&QueryError::IncompleteSemanticOperand {
            reasons: PartialReasonSet::CANCELLED.union(PartialReasonSet::PROPAGATED),
        }),
        "semanticIncompleteOperand(cancelled|propagated)"
    );
    assert_eq!(
        semantic_query_error_raw(&QueryError::IncompleteSemanticOperand {
            reasons: PartialReasonSet::empty(),
        }),
        "semanticIncompleteOperand(none)"
    );
}

#[test]
fn a_warm_force_roots_the_enclosing_candidate_at_its_own_operand_producer() {
    // The node arm's key — `ProjectPath { base, [], context }` — does not
    // encode the operand's evidence, so ONE shared candidate is reachable
    // from operands carrying DIFFERENT producer roots: substituting
    // `Box<T>`'s `value` position with the node produced from another
    // file's `Arg` yields exactly that node back, so the substituted force
    // and the direct `Arg` force reach the same key — the first rooted at
    // BOTH files, the second at `Arg`'s file alone. Only the COLD winner's
    // build observes injected producer evidence, so the shared candidate's
    // own read set is rooted at whichever caller arrived first.
    //
    // That is safe only because the force ALSO merges the operand's
    // evidence into the ENCLOSING in-flight candidate on EVERY force, warm
    // as well as cold. Without the warm-path merge a consumer embedding a
    // warm force result in its own candidate would publish an entry rooted
    // at the cold winner's file and NOT at its own operand's producer — an
    // edit to that producer would then leave the consumer serving a stale
    // surface, a wrong-COMPLETE result rather than a miss.
    //
    // `node_operand_merges_producer_roots_into_active_candidate` covers
    // the COLD leg (its force is the cold winner, so the injected evidence
    // alone would satisfy it). This pins the WARM leg, and the wider
    // operand's leg additionally pins that a warm hit on a narrower
    // candidate does not shrink the enclosing candidate's roots.
    let host = make_host();
    upsert(&host, SOURCE_V1);
    upsert_at(&host, OTHER_OWNER, "export type Arg = string;\n");
    let dispatch = ProjectSemanticDispatch::new(&host);

    let argument_forced = force_result(
        &dispatch,
        &mint(
            &dispatch,
            locator_at(OTHER_OWNER, "Arg", LocatorSymbolSpace::Type, Arc::from([])),
        ),
        ProjectionMode::Identity,
    );
    let narrow = dispatch
        .mint_node_semantic_operand(&argument_forced)
        .expect("argument operand must seal");
    let wide_forced = force_result(
        &dispatch,
        &dispatch
            .mint_authored_semantic_operand(member_value("Box", 0), Arc::from([narrow.clone()]))
            .expect("substituted operand must seal"),
        ProjectionMode::Identity,
    );
    assert_eq!(
        wide_forced.node(),
        argument_forced.node(),
        "the fixture requires the substituted force to reproduce the argument node"
    );
    let wide = dispatch
        .mint_node_semantic_operand(&wide_forced)
        .expect("substituted node operand must seal");

    // NARROW wins the cold build for the shared key and roots the
    // published candidate at OTHER_OWNER alone.
    let context = ProjectionReductionContext::published(ProjectionMode::Expanded);
    let _ = force_with_context(&dispatch, &narrow, context);
    let shared_key = SemanticQueryKey::ProjectPath {
        base: argument_forced.node(),
        path: Arc::from([]),
        context,
    };
    let published_roots = host
        .project_type_store()
        .semantic_graph()
        .entry_self_root_canonicals_for_tests(&shared_key)
        .expect("the cold force must publish a candidate");
    assert!(
        published_roots
            .iter()
            .any(|canonical| canonical.as_ref() == OTHER_OWNER)
            && !published_roots
                .iter()
                .any(|canonical| canonical.as_ref() == OWNER),
        "the fixture requires the cold winner to root the shared candidate at ITS \
         producer alone, got {published_roots:?}"
    );

    // WARM, same producer: the enclosing frame must still be rooted even
    // though this force runs no cold build of its own.
    let frame = BuildLocalTaintGuard::push(&dispatch.build_local_taint);
    let _ = force_with_context(&dispatch, &narrow, context);
    let warm_same = frame.finish();
    assert!(
        warm_same
            .observed_self_roots
            .iter()
            .any(|(canonical, _)| canonical.as_ref() == OTHER_OWNER),
        "a warm force must deposit its operand's roots into the enclosing candidate, got {:?}",
        warm_same.observed_self_roots
    );

    // WARM, the WIDER producer: the shared candidate names OTHER_OWNER
    // only, so the enclosing candidate learns about OWNER solely because
    // the force merges the operand's own evidence on the warm path.
    let frame = BuildLocalTaintGuard::push(&dispatch.build_local_taint);
    let _ = force_with_context(&dispatch, &wide, context);
    let warm_wide = frame.finish();
    for expected in [OWNER, OTHER_OWNER] {
        assert!(
            warm_wide
                .observed_self_roots
                .iter()
                .any(|(canonical, _)| canonical.as_ref() == expected),
            "a warm force must root the enclosing candidate at {expected}, not shrink to the \
             cold winner's roots, got {:?}",
            warm_wide.observed_self_roots
        );
    }
}
