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
    ForceProjectionSegment, ForcedSemanticOperand, OperandSplitEnv, SemanticOperand,
    SemanticOperandForceProjection, SemanticOperandForceRequest, SemanticOperandMintError,
    SemanticOperandParts,
};
use crate::semantic_query::{
    DeclarationSlotSeed, IndexKey, InstantiateContext, InstantiateKey, MemberMergeRole,
    PathSegment, PrimitiveKind, ProjectionMode, ProjectionPath, ProjectionReductionContext,
    PropertyKey, QueryError, QueryResult, ReductionDemand, SemanticNodeData, SemanticNodeId,
    SemanticQueryKey, SurfaceProvenanceContext, VueHeritagePolicy,
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
    dispatch: &ProjectSemanticDispatch<'_>,
    operand: &SemanticOperand,
    context: ProjectionReductionContext,
) -> SemanticQueryKey {
    force_key_at(
        dispatch,
        operand,
        context,
        SemanticOperandForceProjection::WholeSurface,
    )
}

/// The query key the forcing boundary derives for `operand` at `context`
/// and the given projection precision. Mirrors the production routing in
/// `force_semantic_operand` exactly.
pub(super) fn force_key_at(
    _dispatch: &ProjectSemanticDispatch<'_>,
    operand: &SemanticOperand,
    context: ProjectionReductionContext,
    projection: SemanticOperandForceProjection,
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
    // force AT WHOLE-SURFACE PRECISION converges on the DECLARATION
    // Instantiate family; every nested or alternate locator position, and
    // every selective precision, keeps force-owned authored identity.
    SemanticQueryKey::Instantiate(
        if authored.addresses_whole_type_declaration() && projection.is_whole_surface() {
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
                projection,
                SemanticOperandAuthority::mint_for_forcing_boundary(),
            )
        },
    )
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

/// The request-side spelling of a statically-known derived path. A
/// computed index has no static spelling — it is a sealed operand — so a
/// precision carrying one cannot round-trip through here.
fn demand_from_known_path(path: &Arc<[PathSegment]>) -> Arc<[ForceProjectionSegment]> {
    Arc::from(
        path.iter()
            .map(|segment| match segment {
                PathSegment::Member(key) => ForceProjectionSegment::Member(key.clone()),
                PathSegment::Index(index) => ForceProjectionSegment::Index(
                    index
                        .cloned_known()
                        .expect("a computed index is spelled as a sealed operand, not a path"),
                ),
            })
            .collect::<Vec<_>>()
            .into_boxed_slice(),
    )
}

/// The force request for one precision at one context — the exact
/// vocabulary the force API exposes (no test-only mint).
fn request_at(
    context: ProjectionReductionContext,
    precision: &SemanticOperandForceProjection,
) -> SemanticOperandForceRequest {
    match precision {
        SemanticOperandForceProjection::WholeSurface => SemanticOperandForceRequest::new(context),
        SemanticOperandForceProjection::Path(path) => {
            SemanticOperandForceRequest::projecting(context, demand_from_known_path(path))
        }
        SemanticOperandForceProjection::KeyDomain => {
            SemanticOperandForceRequest::key_domain(context)
        }
    }
}

/// One concurrent same-operand, same-precision pair: the winner enters the
/// cold build, the joiner parks on the existing in-flight wait, and both
/// observe the winning value with exactly one published candidate per
/// family. Since the precision participates in family identity, each
/// precision gets its own scenario on a fresh host.
fn concurrent_same_force_joins_the_existing_flight(
    host_canonical: &str,
    host_source: &str,
    locator: AuthoredBodyLocator,
    precision: SemanticOperandForceProjection,
) {
    let host = Arc::new(make_host());
    upsert_at(&host, host_canonical, host_source);
    let dispatch = ProjectSemanticDispatch::new(host.as_ref());
    let operand = mint(&dispatch, locator.clone());
    let key = locator_key(&host, &dispatch, locator);
    let force_context = ProjectionReductionContext::published(ProjectionMode::Identity);
    let forced_key = force_key_at(&dispatch, &operand, force_context, precision.clone());
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
    let winner_precision = precision.clone();
    let winner = std::thread::spawn(move || {
        ProjectSemanticDispatch::new(winner_host.as_ref()).force_semantic_operand(
            &winner_operand,
            request_at(force_context, &winner_precision),
        )
    });
    entered_rx
        .recv_timeout(std::time::Duration::from_secs(10))
        .expect("winner must enter the cold build");

    let joiner_host = Arc::clone(&host);
    let joiner_operand = operand.clone();
    let joiner_precision = precision.clone();
    let joiner = std::thread::spawn(move || {
        ProjectSemanticDispatch::new(joiner_host.as_ref()).force_semantic_operand(
            &joiner_operand,
            request_at(force_context, &joiner_precision),
        )
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
fn concurrent_identical_forces_join_the_existing_graph_flight() {
    // Whole-surface precision over a member-value operand (never converges
    // on the declaration family).
    concurrent_same_force_joins_the_existing_flight(
        OWNER,
        SOURCE_V1,
        member_value("Owned", 0),
        SemanticOperandForceProjection::WholeSurface,
    );
    // Selective precisions over the projection fixture: same-path
    // concurrent pairs must join the existing flight per precision, because
    // the precision now participates in family/lane identity.
    for precision in [
        SemanticOperandForceProjection::Path(member_path(&["wanted"])),
        SemanticOperandForceProjection::Path(member_path(&["cold"])),
        SemanticOperandForceProjection::KeyDomain,
    ] {
        concurrent_same_force_joins_the_existing_flight(
            PROJECTION_OWNER,
            PROJECTION_SOURCE,
            deep_locator(),
            precision,
        );
    }
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

// =====================================================================
// Demand-selected force precision.
//
// A force names the requested key, residual path, or key domain BEFORE
// the operand is forced, so the boundary selects the answering query
// family up front instead of expanding a whole base surface and then
// narrowing it. The fixture below is the shape those bounds are read
// against: one small requested member (`wanted.leaf`) beside one
// deliberately large, deeply nested sibling (`cold`) whose
// materialisation is loudly visible in the graph node count.
// =====================================================================

const PROJECTION_OWNER: &str = "/w/operand/projection.ts";
const PROJECTION_SOURCE: &str = "\
export type Deep = { \
wanted: { leaf: string }; \
cold: { p: \"c0\"; q: \"c1\"; r: \"c2\"; s: \"c3\"; t: \"c4\" }; \
};\n";

fn projection_host() -> VerterHost {
    let host = make_host();
    upsert_at(&host, PROJECTION_OWNER, PROJECTION_SOURCE);
    host
}

fn deep_locator() -> AuthoredBodyLocator {
    locator_at(
        PROJECTION_OWNER,
        "Deep",
        LocatorSymbolSpace::Type,
        Arc::from([]),
    )
}

fn member_path(names: &[&str]) -> Arc<[PathSegment]> {
    Arc::from(
        names
            .iter()
            .map(|name| PathSegment::Member(PropertyKey::identifier(Arc::from(*name))))
            .collect::<Vec<_>>()
            .into_boxed_slice(),
    )
}

fn empty_path() -> Arc<[PathSegment]> {
    Arc::from(Vec::<PathSegment>::new().into_boxed_slice())
}

fn candidates(host: &VerterHost, key: &SemanticQueryKey) -> usize {
    host.project_type_store()
        .semantic_graph()
        .slot_candidate_count_for_tests(key)
}

/// The measurement window marker: how many `execute_read` entries have
/// been recorded so far. Everything after it is work ONE force performed.
fn trace_len() -> usize {
    DISPATCH_TRACE.with(|trace| trace.borrow().len())
}

/// Per-query-family dispatch counts recorded after `from`.
///
/// Naming one key's counter proves only that that one key was not asked
/// for. This names EVERY family the measured window entered, so a force
/// that reached sibling work through a different family — a declaration
/// resolution, an instantiation, a conditional or mapped reduction, a
/// `typeof` — cannot pass unobserved.
fn dispatch_classes_since(from: usize) -> std::collections::BTreeMap<&'static str, usize> {
    DISPATCH_TRACE.with(|trace| {
        let trace = trace.borrow();
        let mut counts = std::collections::BTreeMap::<&'static str, usize>::new();
        for entry in trace[from..].iter() {
            *counts.entry(*entry).or_insert(0) += 1;
        }
        counts
    })
}

fn class_count(counts: &std::collections::BTreeMap<&'static str, usize>, class: &str) -> usize {
    counts.get(class).copied().unwrap_or(0)
}

/// Interned semantic nodes — the graph's allocation counter.
fn interned_nodes(host: &VerterHost) -> usize {
    host.project_type_store().semantic_graph().node_count()
}

/// A request context with a generous projection budget, installed so the
/// measured window's substitution calls are countable through
/// `substitute_top_level_calls`.
fn measured_request(id: u64, owner: &str) -> Arc<RequestContext> {
    RequestContext::with_kind_timing_and_projection_budget(
        id,
        Arc::from(owner),
        verter_audit::RequestKind::ComponentMeta,
        false,
        false,
        None,
        4096,
    )
}

/// The operand's own lowered root — the node a whole-surface force
/// projects at the empty path. Reading it through the shared
/// `LowerLocator` query is what lets the assertions below name the exact
/// whole-surface `ProjectPath` key a selective force must never dispatch.
fn lowered_root(
    host: &VerterHost,
    dispatch: &ProjectSemanticDispatch<'_>,
    locator: AuthoredBodyLocator,
) -> SemanticNodeId {
    match dispatch
        .execute_read(locator_key(host, dispatch, locator))
        .value
    {
        QueryResult::Value(node) => node,
        other => panic!("fixture locator must lower, got {other:?}"),
    }
}

fn force_projecting(
    dispatch: &ProjectSemanticDispatch<'_>,
    operand: &SemanticOperand,
    context: ProjectionReductionContext,
    path: Arc<[PathSegment]>,
) -> ForcedSemanticOperand {
    force_demanding(dispatch, operand, context, demand_from_known_path(&path))
}

/// Force at a residual path spelled in the request-side segment
/// vocabulary — the only spelling that can carry a computed index.
fn force_demanding(
    dispatch: &ProjectSemanticDispatch<'_>,
    operand: &SemanticOperand,
    context: ProjectionReductionContext,
    segments: Arc<[ForceProjectionSegment]>,
) -> ForcedSemanticOperand {
    match try_force_demanding(dispatch, operand, context, segments) {
        QueryResult::Value(forced) => forced,
        other => panic!("path force must resolve, got {other:?}"),
    }
}

fn try_force_demanding(
    dispatch: &ProjectSemanticDispatch<'_>,
    operand: &SemanticOperand,
    context: ProjectionReductionContext,
    segments: Arc<[ForceProjectionSegment]>,
) -> QueryResult<ForcedSemanticOperand> {
    dispatch.force_semantic_operand(
        operand,
        SemanticOperandForceRequest::projecting(context, segments),
    )
}

/// A one-segment computed-index demand over the sealed `index` operand.
fn computed_index_demand(index: ForcedSemanticOperand) -> Arc<[ForceProjectionSegment]> {
    Arc::from(vec![ForceProjectionSegment::ComputedIndex(index)].into_boxed_slice())
}

fn force_key_domain(
    dispatch: &ProjectSemanticDispatch<'_>,
    operand: &SemanticOperand,
    context: ProjectionReductionContext,
) -> ForcedSemanticOperand {
    match dispatch.force_semantic_operand(operand, SemanticOperandForceRequest::key_domain(context))
    {
        QueryResult::Value(forced) => forced,
        other => panic!("key-domain force must resolve, got {other:?}"),
    }
}

#[test]
fn a_selective_authored_force_records_its_materialised_point_at_the_residual_path() {
    let host = projection_host();
    let dispatch = ProjectSemanticDispatch::new(&host);
    let _trace = enable_dispatch_trace_for_test();
    let context = ProjectionReductionContext::published(ProjectionMode::Expanded);
    let path = member_path(&["wanted", "leaf"]);

    let operand = mint(&dispatch, deep_locator());
    let key = force_key_at(
        &dispatch,
        &operand,
        context,
        SemanticOperandForceProjection::Path(Arc::clone(&path)),
    );
    let forced = force_projecting(&dispatch, &operand, context, Arc::clone(&path));
    assert_primitive(&host, forced.node(), PrimitiveKind::String);

    // The published candidate's recorded materialised set claims exactly
    // the point the build computed — the residual path at the request's
    // mode. Recording the empty path here would claim "the whole surface
    // at the slot's mode" while the entry holds only the residual-path
    // value, an over-claim any future cross-path satisfaction widening
    // would trust.
    let graph = host.project_type_store().semantic_graph();
    let recorded = graph
        .entry_satisfied_projection_for_tests(&key)
        .expect("the selective force must publish a candidate");
    assert_eq!(
        recorded.points().len(),
        1,
        "a single-terminal selective force records one point, got {:?}",
        recorded.points()
    );
    assert_eq!(
        recorded.points()[0].path(),
        &ProjectionPath::from(Arc::clone(&path)),
        "the recorded materialised point must sit at the residual path, not the \
         empty whole-surface path"
    );

    // Positive control: the whole-surface force of the same operand
    // records the empty path, so the residual path above is load-bearing
    // rather than a constant every force records.
    let whole_key = force_key(&dispatch, &operand, context);
    let _ = force_with_context(&dispatch, &operand, context);
    let whole_recorded = graph
        .entry_satisfied_projection_for_tests(&whole_key)
        .expect("the whole-surface force must publish a candidate");
    assert_eq!(
        whole_recorded.points()[0].path(),
        &ProjectionPath::empty(),
        "the whole-surface force records the empty path"
    );
}

#[test]
fn residual_path_force_answers_the_requested_key_without_requesting_the_whole_base_surface() {
    let host = projection_host();
    let dispatch = ProjectSemanticDispatch::new(&host);
    let _trace = enable_dispatch_trace_for_test();
    let context = ProjectionReductionContext::published(ProjectionMode::Expanded);
    let ctx = measured_request(21, PROJECTION_OWNER);
    let _request = RequestContextGuard::install(Arc::clone(&ctx));

    // COLD, with nothing pre-warmed. Reading the lowered root up front to
    // name the whole-surface key would move the operand's own locator
    // lowering — the step that interns the base body — OUTSIDE the
    // measured window, so anything it spent on the sibling would be
    // invisible. The root is derived AFTER the window closes instead.
    let operand = mint(&dispatch, deep_locator());
    let window = trace_len();
    let nodes_before = interned_nodes(&host);
    let forced = force_projecting(
        &dispatch,
        &operand,
        context,
        member_path(&["wanted", "leaf"]),
    );
    let classes = dispatch_classes_since(window);
    let substitutions = ctx.substitute_top_level_calls.load(Ordering::Relaxed);
    assert_primitive(&host, forced.node(), PrimitiveKind::String);

    // Bounded work by CLASS, not by one named key: the whole cold force
    // is the force's own family entry, ONE dereference of the operand's
    // own locator, and the path projection. Every other family stays at
    // zero — a whole-surface-then-narrow implementation spends its
    // sibling work in exactly the families this asserts are absent.
    assert_eq!(
        class_count(&classes, "Instantiate"),
        1,
        "one force family entry, got {classes:?}"
    );
    assert_eq!(
        class_count(&classes, "LowerLocator"),
        1,
        "exactly one locator dereference — the operand's own body, got {classes:?}"
    );
    assert_eq!(
        class_count(&classes, "ProjectPath"),
        1,
        "one projection at the demanded residual path, got {classes:?}"
    );
    assert_eq!(
        classes.len(),
        3,
        "a two-hop selective force enters no other query family, got {classes:?}"
    );
    assert_eq!(
        substitutions, 0,
        "a non-generic base binds no type parameters, so it substitutes nothing"
    );

    let root = lowered_root(&host, &dispatch, deep_locator());
    let selective = SemanticQueryKey::ProjectPath {
        base: root,
        path: member_path(&["wanted", "leaf"]),
        context,
    };
    // The residual path IS the family the force dispatched...
    assert_eq!(
        dispatch_cold_for(&selective),
        1,
        "the demanded residual path must be the projection the force requests"
    );
    // ...and neither the whole base surface nor the sibling key was ever
    // asked for. A boundary that forces the base first and narrows
    // afterwards trips exactly here.
    for absent in [
        SemanticQueryKey::ProjectPath {
            base: root,
            path: empty_path(),
            context,
        },
        SemanticQueryKey::ProjectPath {
            base: root,
            path: member_path(&["cold"]),
            context,
        },
        SemanticQueryKey::KeyOf {
            base: root,
            context,
        },
    ] {
        assert_eq!(
            dispatch_cold_for(&absent) + dispatch_warm_for(&absent),
            0,
            "a selective force must request neither the whole surface nor a sibling key"
        );
    }

    // An identical warm demand allocates nothing and enters no family
    // beyond its own warm hit.
    let nodes_after = interned_nodes(&host);
    assert!(
        nodes_after > nodes_before,
        "the cold force did intern the base body, so the warm zero below is meaningful"
    );
    let warm_window = trace_len();
    let warm = force_projecting(
        &dispatch,
        &operand,
        context,
        member_path(&["wanted", "leaf"]),
    );
    assert_eq!(warm.node(), forced.node());
    assert_eq!(
        interned_nodes(&host),
        nodes_after,
        "an identical warm demand must allocate no semantic nodes"
    );
    let warm_classes = dispatch_classes_since(warm_window);
    assert_eq!(
        warm_classes.get("Instantiate").copied(),
        Some(1),
        "the warm repeat is one hit on the force's own family, got {warm_classes:?}"
    );
    assert_eq!(
        warm_classes.len(),
        1,
        "a warm repeat must enter no other family, got {warm_classes:?}"
    );
}

#[test]
fn key_domain_force_answers_keyof_without_materialising_member_values() {
    let host = projection_host();
    let dispatch = ProjectSemanticDispatch::new(&host);
    let _trace = enable_dispatch_trace_for_test();
    let context = ProjectionReductionContext::published(ProjectionMode::Expanded);
    let ctx = measured_request(22, PROJECTION_OWNER);
    let _request = RequestContextGuard::install(Arc::clone(&ctx));

    // COLD: the lowered root is derived only AFTER the measured window,
    // so nothing the key-domain force spends is hidden behind a prewarm.
    let operand = mint(&dispatch, deep_locator());
    let window = trace_len();
    let forced = force_key_domain(&dispatch, &operand, context);
    let classes = dispatch_classes_since(window);
    let substitutions = ctx.substitute_top_level_calls.load(Ordering::Relaxed);

    // The key domain is the answer — one key per declared member.
    let keys = key_union_names(&host, forced.node());
    assert_eq!(
        keys,
        vec!["cold".to_string(), "wanted".to_string()],
        "keyof must answer the declared key domain"
    );

    // Bounded work by CLASS: the force's family entry, ONE dereference of
    // the operand's own locator, and the key-domain query. `ProjectPath`
    // is absent entirely — projecting ANY member value to decide the key
    // domain would show up here.
    assert_eq!(
        class_count(&classes, "Instantiate"),
        1,
        "one force family entry, got {classes:?}"
    );
    assert_eq!(
        class_count(&classes, "LowerLocator"),
        1,
        "exactly one locator dereference — the operand's own body, got {classes:?}"
    );
    assert_eq!(
        class_count(&classes, "KeyOf"),
        1,
        "the key-domain family answers it, got {classes:?}"
    );
    assert_eq!(
        classes.len(),
        3,
        "a key-domain force enters no other query family — no member \
         projection, declaration resolution, or reduction, got {classes:?}"
    );
    assert_eq!(
        substitutions, 0,
        "answering a key domain substitutes nothing"
    );

    let root = lowered_root(&host, &dispatch, deep_locator());
    let whole_surface = SemanticQueryKey::ProjectPath {
        base: root,
        path: empty_path(),
        context,
    };
    assert_eq!(
        dispatch_cold_for(&whole_surface) + dispatch_warm_for(&whole_surface),
        0,
        "a key-domain force must not request the base's whole surface"
    );

    // The member VALUES were never projected. A `keyof` that materialised
    // its source's member values would have to demand each member's value
    // surface, and every such demand enters the shared dispatch as a member
    // `ProjectPath` off this exact base.
    for name in ["wanted", "cold"] {
        let member = SemanticQueryKey::ProjectPath {
            base: root,
            path: member_path(&[name]),
            context,
        };
        assert_eq!(
            dispatch_cold_for(&member) + dispatch_warm_for(&member),
            0,
            "keyof forces key-producing structure only - member `{name}` \
             must never have its value projected to answer the key domain"
        );
    }

    // Forcing a member value afterwards is what a value demand looks like,
    // and it DOES project that member, so the zero above is a genuine
    // absence rather than a counter that can never move.
    let _ = force_projecting(&dispatch, &operand, context, member_path(&["cold"]));
    let cold_member = SemanticQueryKey::ProjectPath {
        base: root,
        path: member_path(&["cold"]),
        context,
    };
    assert_eq!(
        dispatch_cold_for(&cold_member),
        1,
        "a member-value demand must project that member"
    );
}

/// Sorted literal key names of a `keyof` result — a single literal or a
/// union of them. Any other shape is a failure, so the assertion cannot
/// pass on a degraded/opaque answer.
fn key_union_names(host: &VerterHost, node: SemanticNodeId) -> Vec<String> {
    let graph = host.project_type_store().semantic_graph();
    let arms: Vec<SemanticNodeId> = match graph.node_data(node).as_deref() {
        Some(SemanticNodeData::Union(arms)) => arms.to_vec(),
        Some(SemanticNodeData::Literal(_)) => vec![node],
        other => panic!("keyof must answer literal keys, got {other:?}"),
    };
    let mut names: Vec<String> = arms
        .iter()
        .map(|arm| match graph.node_data(*arm).as_deref() {
            Some(SemanticNodeData::Literal(crate::semantic_query::LiteralValue::String(name))) => {
                name.to_string()
            }
            other => panic!("keyof arm must be a string literal, got {other:?}"),
        })
        .collect();
    names.sort();
    names
}

#[test]
fn an_empty_residual_path_is_the_whole_surface_precision_and_shares_its_entry() {
    let host = projection_host();
    let dispatch = ProjectSemanticDispatch::new(&host);
    let _trace = enable_dispatch_trace_for_test();
    let context = ProjectionReductionContext::published(ProjectionMode::Identity);

    // "Walk nothing" has exactly ONE spelling. Two would put one value in
    // two family entries: the authored force family carries the precision
    // axis, and only the whole-surface precision converges onto the
    // declaration-source `Instantiate` family the compiler dispatches.
    assert!(
        SemanticOperandForceRequest::projecting(context, demand_from_known_path(&empty_path()))
            .demand()
            .is_whole_surface(),
        "an empty residual path must canonicalise to the whole-surface precision"
    );

    let operand = mint(&dispatch, deep_locator());
    let key = force_key(&dispatch, &operand, context);
    let via_empty_path = force_projecting(&dispatch, &operand, context, empty_path());
    let via_whole_surface = match dispatch
        .force_semantic_operand(&operand, SemanticOperandForceRequest::new(context))
    {
        QueryResult::Value(forced) => forced,
        other => panic!("whole-surface force must resolve, got {other:?}"),
    };

    assert_eq!(
        via_empty_path.node(),
        via_whole_surface.node(),
        "both spellings answer the same surface"
    );
    assert_eq!(dispatch_cold_for(&key), 1, "one cold computation");
    assert_eq!(
        dispatch_warm_for(&key),
        1,
        "the second spelling must warm-hit the first's entry"
    );
    assert_eq!(
        candidates(&host, &key),
        1,
        "an aliasing empty path would publish a second candidate"
    );
}

#[test]
fn distinct_force_precisions_never_alias_and_warm_repeats_add_no_candidates() {
    let host = projection_host();
    let dispatch = ProjectSemanticDispatch::new(&host);
    let _trace = enable_dispatch_trace_for_test();
    let context = ProjectionReductionContext::published(ProjectionMode::Expanded);
    let operand = mint(&dispatch, deep_locator());

    let precisions = [
        SemanticOperandForceProjection::WholeSurface,
        SemanticOperandForceProjection::KeyDomain,
        SemanticOperandForceProjection::Path(member_path(&["wanted"])),
        SemanticOperandForceProjection::Path(member_path(&["cold"])),
    ];
    let mut nodes = Vec::new();
    for precision in &precisions {
        let key = force_key_at(&dispatch, &operand, context, precision.clone());
        let mut forced = Vec::new();
        // bounded-loop: fixed cold/warm pair per precision.
        for _ in 0..2 {
            forced.push(match precision {
                SemanticOperandForceProjection::WholeSurface => match dispatch
                    .force_semantic_operand(&operand, SemanticOperandForceRequest::new(context))
                {
                    QueryResult::Value(value) => value.node(),
                    other => panic!("whole-surface force must resolve, got {other:?}"),
                },
                SemanticOperandForceProjection::KeyDomain => {
                    force_key_domain(&dispatch, &operand, context).node()
                }
                SemanticOperandForceProjection::Path(path) => {
                    force_projecting(&dispatch, &operand, context, Arc::clone(path)).node()
                }
            });
        }
        assert_eq!(
            forced[0], forced[1],
            "{precision:?}: warm repeat must agree"
        );
        assert_eq!(
            dispatch_cold_for(&key),
            1,
            "{precision:?}: exactly one cold computation"
        );
        assert_eq!(
            dispatch_warm_for(&key),
            1,
            "{precision:?}: the repeat must warm-hit"
        );
        assert_eq!(
            candidates(&host, &key),
            1,
            "{precision:?}: a warm repeat must not grow the candidate set"
        );
        nodes.push(forced[0]);
    }

    // Every precision answers a DIFFERENT value, so none may warm-hit
    // another's entry.
    for (i, left) in nodes.iter().enumerate() {
        for (j, right) in nodes.iter().enumerate() {
            if i != j {
                assert_ne!(
                    left, right,
                    "precisions {:?} and {:?} must not share an answer",
                    precisions[i], precisions[j]
                );
            }
        }
    }
}

// =====================================================================
// Composite bases, open shells, and incremental edits under a selective
// force. Each case pushes the demanded residual path THROUGH the shared
// projection families, so the arm/branch rules the walker owns are
// exercised at exactly the precision the force requested.
// =====================================================================

const COMPOSITE_OWNER: &str = "/w/operand/composite.ts";
const COMPOSITE_SOURCE: &str = "\
export type Named = { name: string; extra: { p: \"e0\"; q: \"e1\"; r: \"e2\" } };\n\
export type Tagged = { tag: number; heavy: { h0: \"h0\"; h1: \"h1\"; h2: \"h2\" } };\n\
export type Both = Named & Tagged;\n\
export type Cond<T> = T extends string ? { name: string } : { other: number };\n\
export type Mixed<T> = Named & Cond<T>;\n\
export type Either = { pick: string } | { pick: number };\n\
export type Keys = \"name\" | \"tag\";\n\
export type Flat = { name: string; tag: number; unselected: { p: \"e0\"; q: \"e1\" } };\n\
export type Open<T> = T extends string ? { leaf: \"yes\" } : { leaf: \"no\" };\n";

fn composite_host() -> VerterHost {
    let host = make_host();
    upsert_at(&host, COMPOSITE_OWNER, COMPOSITE_SOURCE);
    host
}

fn composite_locator(symbol: &str) -> AuthoredBodyLocator {
    locator_at(
        COMPOSITE_OWNER,
        symbol,
        LocatorSymbolSpace::Type,
        Arc::from([]),
    )
}

/// The declaration-resolution key the path walker dispatches when it
/// steps a lazy `DeclRef` arm — the intersection's contribution
/// CLASSIFICATION probe, and the only key an arm step actually emits
/// (arms are stepped in-process, never re-dispatched under their own
/// node). Mirrors the walker's construction exactly.
fn arm_resolve_key(host: &VerterHost, arm: SemanticNodeId) -> SemanticQueryKey {
    let graph = host.project_type_store().semantic_graph();
    let data = graph.node_data(arm);
    let Some(SemanticNodeData::DeclRef { identity }) = data.as_deref() else {
        panic!("an intersection arm must lower to a lazy declaration reference")
    };
    SemanticQueryKey::ResolveDecl(crate::semantic_query::ResolveDeclKey {
        scope: crate::semantic_query::ScopeId {
            canonical_id: Arc::clone(&identity.canonical_id),
            owner: identity.owner,
            local_scope: None,
            binder_scope_id: crate::semantic_query::BinderScopeId::file_scope(identity.owner),
        },
        name: Arc::clone(&identity.decl_name),
    })
}

fn primitive_kind(host: &VerterHost, node: SemanticNodeId) -> Option<PrimitiveKind> {
    match host
        .project_type_store()
        .semantic_graph()
        .node_data(node)
        .as_deref()
    {
        Some(SemanticNodeData::Primitive(kind)) => Some(*kind),
        _ => None,
    }
}

#[test]
fn intersection_and_union_bases_carry_the_residual_path_into_their_arms() {
    let host = composite_host();
    let dispatch = ProjectSemanticDispatch::new(&host);
    let _trace = enable_dispatch_trace_for_test();
    let context = ProjectionReductionContext::published(ProjectionMode::Expanded);

    // Intersection: each arm is offered the SAME residual path, and the
    // arm that does not carry the key contributes nothing rather than
    // rewriting the answer to `never`.
    //
    // ONE cold evaluation of ONE `(path segment, context)` pair, measured
    // end to end, is what the per-arm probe bound is stated over.
    let both = mint(&dispatch, composite_locator("Both"));
    let window = trace_len();
    let forced_name = force_projecting(&dispatch, &both, context, member_path(&["name"]));
    let cold_classes = dispatch_classes_since(window);
    assert_eq!(
        primitive_kind(&host, forced_name.node()),
        Some(PrimitiveKind::String),
        "`Both.name` must come from the contributing arm alone"
    );

    // The arms lower as lazy declaration REFERENCES, so the dispatch that
    // tells the walker whether an arm carries the key — the contribution
    // classification probe — is the arm's own `ResolveDecl`. Two arms,
    // two probes: the non-contributing `Tagged` arm is resolved once and
    // then dropped as opaque at the intersection join. Counting a key the
    // arm step never emits (a `ProjectPath` off the arm node: arms are
    // stepped in-process, never re-dispatched) would read zero for every
    // implementation and prove nothing.
    assert_eq!(
        class_count(&cold_classes, "ResolveDecl"),
        2,
        "at most one classification probe per potentially contributing arm, \
         got {cold_classes:?}"
    );
    // The COMPLETE work of that cold evaluation, by class. This is the
    // "zero deep work once non-contribution is proven" bound: exactly ONE
    // `ProjectPath` — the caller's own residual path — so neither arm's
    // members were projected, least of all the non-contributing arm's
    // deliberately value-heavy `heavy`. The rest is the bounded per-arm
    // probe: one `ResolveDecl` and one shallow `Instantiate` per arm, one
    // `LowerLocator` for the intersection's own body plus one per arm
    // declaration, and the force's own family entry.
    assert_eq!(
        cold_classes,
        [
            ("Instantiate", 3),
            ("LowerLocator", 3),
            ("ProjectPath", 1),
            ("ResolveDecl", 2),
        ]
        .into_iter()
        .collect::<std::collections::BTreeMap<&'static str, usize>>(),
        "one cold intersection evaluation must perform exactly the bounded \
         per-arm probe and one projection"
    );

    let both_root = lowered_root(&host, &dispatch, composite_locator("Both"));
    let graph = host.project_type_store().semantic_graph();
    let arms: Vec<SemanticNodeId> = match graph.node_data(both_root).as_deref() {
        Some(SemanticNodeData::Intersection(arms)) => arms.iter().copied().collect(),
        other => panic!("`Both` must lower to an intersection, got {other:?}"),
    };
    // Arms lower as lazy declaration references; name them by identity.
    let decl_named = |node: SemanticNodeId, name: &str| match graph.node_data(node).as_deref() {
        Some(SemanticNodeData::DeclRef { identity }) => identity.decl_name.as_ref() == name,
        _ => false,
    };
    let named_arm = arms
        .iter()
        .copied()
        .find(|arm| decl_named(*arm, "Named"))
        .expect("the contributing arm is the `Named` reference");
    let tagged_arm = arms
        .iter()
        .copied()
        .find(|arm| decl_named(*arm, "Tagged"))
        .expect("the non-contributing arm is the `Tagged` reference");
    // Per-arm, not just in aggregate: each arm was probed exactly once.
    for (arm, label) in [(named_arm, "Named"), (tagged_arm, "Tagged")] {
        let probe = arm_resolve_key(&host, arm);
        assert_eq!(
            dispatch_cold_for(&probe) + dispatch_warm_for(&probe),
            1,
            "arm `{label}` must be classified by exactly one probe per cold evaluation"
        );
    }

    // A DIFFERENT path segment is a different cold evaluation, so it
    // spends its own one probe per arm — and answers from the OTHER arm.
    let tag_window = trace_len();
    let forced_tag = force_projecting(&dispatch, &both, context, member_path(&["tag"]));
    let tag_classes = dispatch_classes_since(tag_window);
    assert_eq!(
        primitive_kind(&host, forced_tag.node()),
        Some(PrimitiveKind::Number),
        "`Both.tag` must come from the contributing arm alone"
    );
    assert_eq!(
        class_count(&tag_classes, "ResolveDecl"),
        2,
        "a second cold evaluation spends its own one probe per arm, got {tag_classes:?}"
    );

    // Root-keyed counters: the intersection root's whole surface and the
    // contributing arm's non-requested `extra` key — both keyed on the
    // ROOT, the base a whole-surface regression at the forcing boundary
    // would use — were never requested.
    for key in [
        SemanticQueryKey::ProjectPath {
            base: both_root,
            path: empty_path(),
            context,
        },
        SemanticQueryKey::ProjectPath {
            base: both_root,
            path: member_path(&["extra"]),
            context,
        },
    ] {
        assert_eq!(
            dispatch_cold_for(&key) + dispatch_warm_for(&key),
            0,
            "answering intersection keys must not request the root's whole \
             surface or a sibling key"
        );
    }

    // An identical warm demand adds zero work: no new cold build, no new
    // classification probe, one warm hit on the force's own family.
    let both_force_key = force_key_at(
        &dispatch,
        &both,
        context,
        SemanticOperandForceProjection::Path(member_path(&["name"])),
    );
    let warm_window = trace_len();
    let warm_again = force_projecting(&dispatch, &both, context, member_path(&["name"]));
    let warm_classes = dispatch_classes_since(warm_window);
    assert_eq!(
        primitive_kind(&host, warm_again.node()),
        Some(PrimitiveKind::String)
    );
    assert_eq!(
        dispatch_cold_for(&both_force_key),
        1,
        "no second cold build"
    );
    assert_eq!(
        dispatch_warm_for(&both_force_key),
        1,
        "the identical repeat must warm-hit"
    );
    assert_eq!(
        class_count(&warm_classes, "ResolveDecl"),
        0,
        "an identical warm demand adds zero classification probes, got {warm_classes:?}"
    );
    for (arm, label) in [(named_arm, "Named"), (tagged_arm, "Tagged")] {
        let probe = arm_resolve_key(&host, arm);
        assert_eq!(
            dispatch_cold_for(&probe) + dispatch_warm_for(&probe),
            2,
            "arm `{label}` keeps one probe per cold evaluation and none for the warm repeat"
        );
    }

    // Zero DEEP work once non-contribution is proven, named on the
    // non-contributing arm's own surface as well as in aggregate above.
    let tagged_surface = lowered_root(&host, &dispatch, composite_locator("Tagged"));
    let heavy_value = match graph.node_data(tagged_surface).as_deref() {
        Some(SemanticNodeData::Object(surface)) => surface
            .positive_members()
            .iter()
            .find(|member| member.string_name() == Some("heavy"))
            .map(|member| member.value)
            .expect("the value-heavy member exists on the non-contributing arm"),
        other => panic!("the non-contributing arm must resolve to an object, got {other:?}"),
    };
    let heavy_keys = [
        SemanticQueryKey::ProjectPath {
            base: tagged_surface,
            path: member_path(&["heavy"]),
            context,
        },
        SemanticQueryKey::ProjectPath {
            base: heavy_value,
            path: empty_path(),
            context,
        },
    ];
    for key in &heavy_keys {
        assert_eq!(
            dispatch_cold_for(key) + dispatch_warm_for(key),
            0,
            "a proven non-contributing arm must never have its member values forced"
        );
    }
    // Positive control: those counters are live — an actual dispatch of
    // the same keys moves them.
    for key in &heavy_keys {
        let _ = dispatch.execute_read(key.clone());
        assert!(
            dispatch_cold_for(key) + dispatch_warm_for(key) > 0,
            "the non-contributing arm's counters must move when the key is dispatched"
        );
    }

    // Union: EVERY arm must contribute, and each receives the same
    // residual path, so the answer is the union of the per-arm results.
    let either = mint(&dispatch, composite_locator("Either"));
    let forced = force_projecting(&dispatch, &either, context, member_path(&["pick"]));
    let graph = host.project_type_store().semantic_graph();
    let mut arms: Vec<PrimitiveKind> = match graph.node_data(forced.node()).as_deref() {
        Some(SemanticNodeData::Union(arms)) => arms
            .iter()
            .filter_map(|arm| primitive_kind(&host, *arm))
            .collect(),
        other => panic!("a union path must answer a union, got {other:?}"),
    };
    arms.sort_by_key(|kind| format!("{kind:?}"));
    assert_eq!(
        arms,
        vec![PrimitiveKind::Number, PrimitiveKind::String],
        "each union arm must contribute its own projection of the residual path"
    );
}

#[test]
fn an_undecidable_intersection_arm_survives_as_an_open_carrier_with_the_residual_path() {
    let host = composite_host();
    let dispatch = ProjectSemanticDispatch::new(&host);
    let _trace = enable_dispatch_trace_for_test();
    let context = ProjectionReductionContext::published(ProjectionMode::Expanded);

    // `Mixed<T> = Named & Cond<T>` forced with `T` UNBOUND: the `Cond<T>`
    // arm's contribution to `name` is undecidable. The projection must
    // preserve an open/partial carrier for that arm — never treat the arm
    // as absent (a plain `string` answer) and never rewrite it to `never`.
    let forced = force_projecting(
        &dispatch,
        &mint(&dispatch, composite_locator("Mixed")),
        context,
        member_path(&["name"]),
    );
    let graph = host.project_type_store().semantic_graph();
    let arms: Vec<SemanticNodeId> = match graph.node_data(forced.node()).as_deref() {
        Some(SemanticNodeData::Intersection(arms)) => arms.iter().copied().collect(),
        other => panic!(
            "an undecidable contribution arm must preserve an open/partial carrier \
             instead of collapsing to the contributing arm alone, got {other:?}"
        ),
    };
    // The proven contributing arm's projection is present...
    assert!(
        arms.iter()
            .any(|arm| primitive_kind(&host, *arm) == Some(PrimitiveKind::String)),
        "the contributing arm's `name` projection must be in the join, got {arms:?}"
    );
    // ...and the undecidable arm survives as an open conditional shell
    // whose branches carry the residual path's own projections — the
    // `true` branch HAS `name`, the `false` branch does not, and neither
    // was selected.
    let shell = arms
        .iter()
        .copied()
        .find(|arm| {
            matches!(
                graph.node_data(*arm).as_deref(),
                Some(SemanticNodeData::Conditional { .. })
            )
        })
        .expect("the undecidable arm must survive as an open conditional carrier");
    match graph.node_data(shell).as_deref() {
        Some(SemanticNodeData::Conditional {
            true_branch_ref,
            false_branch_ref,
            ..
        }) => {
            assert_eq!(
                primitive_kind(&host, *true_branch_ref),
                Some(PrimitiveKind::String),
                "the true branch keeps its own projection of the residual path"
            );
            assert!(
                matches!(
                    graph.node_data(*false_branch_ref).as_deref(),
                    Some(SemanticNodeData::Opaque(_))
                ),
                "the false branch carries no `name` member — its miss stays an opaque \
                 sentinel rather than a fabricated value"
            );
        }
        other => panic!("expected a conditional shell, got {other:?}"),
    }
}

#[test]
fn a_literal_union_index_resolves_its_key_domain_and_distributes_per_key() {
    let host = composite_host();
    let dispatch = ProjectSemanticDispatch::new(&host);
    let _trace = enable_dispatch_trace_for_test();
    let context = ProjectionReductionContext::published(ProjectionMode::Expanded);

    // The index operand is a `"name" | "tag"` union. Its key domain is
    // known before any base member is requested, so the access
    // distributes per literal instead of expanding `Flat`'s surface and
    // filtering afterwards.
    let keys = force_projecting(
        &dispatch,
        &mint(&dispatch, composite_locator("Keys")),
        context,
        empty_path(),
    );
    let forced = force_demanding(
        &dispatch,
        &mint(&dispatch, composite_locator("Flat")),
        context,
        computed_index_demand(keys),
    );

    let graph = host.project_type_store().semantic_graph();
    let mut arms: Vec<PrimitiveKind> = match graph.node_data(forced.node()).as_deref() {
        Some(SemanticNodeData::Union(arms)) => arms
            .iter()
            .filter_map(|arm| primitive_kind(&host, *arm))
            .collect(),
        other => panic!("a literal-union index must distribute, got {other:?}"),
    };
    arms.sort_by_key(|kind| format!("{kind:?}"));
    assert_eq!(
        arms,
        vec![PrimitiveKind::Number, PrimitiveKind::String],
        "`Flat[\"name\" | \"tag\"]` is the union of the two selected members"
    );

    // `Flat` carries a third, value-heavy member the index does NOT
    // select. Key-domain-first distribution projects only the selected
    // literals, so the unselected member's value is never projected and
    // the base's whole surface is never requested; an expand-then-filter
    // implementation would do both, and only these counters tell the two
    // apart.
    let flat_root = lowered_root(&host, &dispatch, composite_locator("Flat"));
    let unselected_member = SemanticQueryKey::ProjectPath {
        base: flat_root,
        path: member_path(&["unselected"]),
        context,
    };
    let whole_surface = SemanticQueryKey::ProjectPath {
        base: flat_root,
        path: empty_path(),
        context,
    };
    let unselected_index = SemanticQueryKey::IndexedAccess {
        base: flat_root,
        index: IndexKey::String(Arc::from("unselected")),
        mode: context.mode,
    };
    for key in [&unselected_member, &whole_surface, &unselected_index] {
        assert_eq!(
            dispatch_cold_for(key) + dispatch_warm_for(key),
            0,
            "an index that selects `name | tag` must never materialise the \
             unselected member or the whole base surface"
        );
    }
    // Positive control: the counters are live — an actual demand for the
    // unselected member moves them.
    let _ = dispatch.execute_read(unselected_member.clone());
    assert_eq!(
        dispatch_cold_for(&unselected_member),
        1,
        "the unselected-member counter must move when the key is actually dispatched"
    );
}

// =====================================================================
// A computed index is an OPERAND, not a bare handle.
//
// `Base[K]` names its key type by graph handle. The base operand's own
// seal proves nothing about that handle: a node minted by another store,
// or by a superseded generation of this one, is just an integer this
// graph would happily read as an unrelated node, and its producer's read
// facts would never reach the forced candidate. So the request spells a
// computed index as a sealed `ForcedSemanticOperand`, and the boundary
// validates it and merges its evidence BEFORE any base work.
// =====================================================================

const INDEX_OWNER: &str = "/w/operand/index_keys.ts";
const INDEX_SOURCE: &str = "export type OtherKeys = \"name\" | \"tag\";\n";

fn index_keys_locator() -> AuthoredBodyLocator {
    locator_at(
        INDEX_OWNER,
        "OtherKeys",
        LocatorSymbolSpace::Type,
        Arc::from([]),
    )
}

/// The forced `"name" | "tag"` key operand of `dispatch`'s own store.
fn key_operand(
    dispatch: &ProjectSemanticDispatch<'_>,
    context: ProjectionReductionContext,
    symbol: &str,
) -> ForcedSemanticOperand {
    force_projecting(
        dispatch,
        &mint(dispatch, composite_locator(symbol)),
        context,
        empty_path(),
    )
}

#[test]
fn a_computed_index_from_a_foreign_store_is_refused_before_the_base_is_touched() {
    let host = composite_host();
    let dispatch = ProjectSemanticDispatch::new(&host);
    let _trace = enable_dispatch_trace_for_test();
    let context = ProjectionReductionContext::published(ProjectionMode::Expanded);

    // A second host over IDENTICAL sources: its graph interns the same
    // shapes, so the foreign `"name" | "tag"` handle is exactly the kind
    // of id this store would otherwise read as one of its own nodes.
    let foreign_host = composite_host();
    let foreign_dispatch = ProjectSemanticDispatch::new(&foreign_host);
    let foreign_keys = key_operand(&foreign_dispatch, context, "Keys");

    let flat_lower = locator_key(&host, &dispatch, composite_locator("Flat"));
    let before = dispatch_cold_for(&flat_lower) + dispatch_warm_for(&flat_lower);
    let flat = mint(&dispatch, composite_locator("Flat"));
    assert_operand_error(
        try_force_demanding(
            &dispatch,
            &flat,
            context,
            computed_index_demand(foreign_keys),
        ),
        QueryError::ForeignSemanticOperand,
    );
    assert_eq!(
        dispatch_cold_for(&flat_lower) + dispatch_warm_for(&flat_lower),
        before,
        "a foreign index must be refused BEFORE the base declaration is dereferenced"
    );

    // Positive control: the same demand shape, with this store's own key
    // operand, resolves — so the refusal is the store check, not an
    // unsupported demand.
    let own_keys = key_operand(&dispatch, context, "Keys");
    let forced = force_demanding(&dispatch, &flat, context, computed_index_demand(own_keys));
    assert!(matches!(
        host.project_type_store()
            .semantic_graph()
            .node_data(forced.node())
            .as_deref(),
        Some(SemanticNodeData::Union(_))
    ));
    assert!(
        dispatch_cold_for(&flat_lower) + dispatch_warm_for(&flat_lower) > before,
        "the accepted force DOES dereference the base, so the zero above is genuine"
    );
}

#[test]
fn a_computed_index_from_a_superseded_generation_is_refused() {
    let host = composite_host();
    let dispatch = ProjectSemanticDispatch::new(&host);
    let _trace = enable_dispatch_trace_for_test();
    let context = ProjectionReductionContext::published(ProjectionMode::Expanded);

    let keys = key_operand(&dispatch, context, "Keys");
    host.project_type_store().bump_project_generation();

    let flat = mint(&dispatch, composite_locator("Flat"));
    assert_operand_error(
        try_force_demanding(&dispatch, &flat, context, computed_index_demand(keys)),
        QueryError::StaleSemanticOperand,
    );

    // Control: the SAME base at the SAME generation, demanded through a
    // statically-known key, still resolves. The refusal is therefore
    // attributable to the stale index operand and not to the base or to
    // the generation bump itself.
    let forced = force_projecting(&dispatch, &flat, context, member_path(&["name"]));
    assert_eq!(
        primitive_kind(&host, forced.node()),
        Some(PrimitiveKind::String)
    );
}

#[test]
fn a_computed_index_operands_producer_roots_reach_the_forced_candidate() {
    let host = composite_host();
    upsert_at(&host, INDEX_OWNER, INDEX_SOURCE);
    let dispatch = ProjectSemanticDispatch::new(&host);
    let _trace = enable_dispatch_trace_for_test();
    let context = ProjectionReductionContext::published(ProjectionMode::Expanded);

    // The key type lives in a DIFFERENT file from the base, so the index
    // producer's roots are distinguishable from the base's own.
    let keys = force_projecting(
        &dispatch,
        &mint(&dispatch, index_keys_locator()),
        context,
        empty_path(),
    );
    assert!(
        keys.evidence()
            .self_roots()
            .iter()
            .any(|(canonical, _)| canonical.as_ref() == INDEX_OWNER),
        "the index operand must carry its own producer roots"
    );
    let index_node = keys.node();

    let flat = mint(&dispatch, composite_locator("Flat"));
    let frame = BuildLocalTaintGuard::push(&dispatch.build_local_taint);
    let forced = force_demanding(&dispatch, &flat, context, computed_index_demand(keys));
    let observed = frame.finish();
    assert!(
        observed
            .observed_self_roots
            .iter()
            .any(|(canonical, _)| canonical.as_ref() == INDEX_OWNER),
        "the index operand's producer roots must root the enclosing candidate, got {:?}",
        observed.observed_self_roots
    );
    assert!(
        forced
            .evidence()
            .read_set()
            .facts
            .iter()
            .any(|fact| fact.canonical_id() == Some(INDEX_OWNER)),
        "the forced result must carry the index producer's read facts"
    );

    // The published candidate retains them, so a later validation of this
    // entry revalidates the index producer's file too.
    let key = force_key_at(
        &dispatch,
        &flat,
        context,
        SemanticOperandForceProjection::Path(Arc::from(
            vec![PathSegment::Index(IndexKey::Computed(index_node))].into_boxed_slice(),
        )),
    );
    assert!(
        host.project_type_store()
            .semantic_graph()
            .entry_self_root_canonicals_for_tests(&key)
            .expect("the computed-index force must publish a candidate")
            .iter()
            .any(|canonical| canonical.as_ref() == INDEX_OWNER),
        "the published candidate must retain the index producer's root"
    );

    // Discriminator: an otherwise identical force whose path names no
    // computed index does NOT observe the index producer's file, so the
    // roots above arrive through the index operand rather than through
    // the base's own reads.
    let frame = BuildLocalTaintGuard::push(&dispatch.build_local_taint);
    let _ = force_projecting(&dispatch, &flat, context, member_path(&["name"]));
    let without_index = frame.finish();
    assert!(
        !without_index
            .observed_self_roots
            .iter()
            .any(|(canonical, _)| canonical.as_ref() == INDEX_OWNER),
        "a force with no computed index must not observe the index producer's file"
    );
}

#[test]
fn an_open_conditional_shell_keeps_the_residual_path_without_selecting_a_branch() {
    let host = composite_host();
    let dispatch = ProjectSemanticDispatch::new(&host);
    let _trace = enable_dispatch_trace_for_test();
    let context = ProjectionReductionContext::published(ProjectionMode::Expanded);

    // `Open<T>` is forced with `T` unbound, so the check cannot be
    // decided. The residual path distributes into BOTH branches and the
    // shell stays addressable — a force must never collapse an open
    // conditional to one branch.
    let forced = force_projecting(
        &dispatch,
        &mint(&dispatch, composite_locator("Open")),
        context,
        member_path(&["leaf"]),
    );
    let graph = host.project_type_store().semantic_graph();
    let (true_branch, false_branch) = match graph.node_data(forced.node()).as_deref() {
        Some(SemanticNodeData::Conditional {
            true_branch_ref,
            false_branch_ref,
            ..
        }) => (*true_branch_ref, *false_branch_ref),
        other => panic!("an undecided conditional must stay a shell, got {other:?}"),
    };
    let literal = |node| match graph.node_data(node).as_deref() {
        Some(SemanticNodeData::Literal(crate::semantic_query::LiteralValue::String(text))) => {
            text.to_string()
        }
        other => panic!("the branch must carry the projected leaf, got {other:?}"),
    };
    assert_eq!(literal(true_branch), "yes");
    assert_eq!(literal(false_branch), "no");
    assert_ne!(
        true_branch, false_branch,
        "both branches keep their own projection of the residual path"
    );
}

#[test]
fn edits_to_the_requested_key_recompute_and_an_unrelated_edit_matches_a_fresh_result() {
    const V1: &str = "\
export type Deep = { wanted: { leaf: string }; cold: { c: \"c0\" } };\n";
    const REQUESTED_EDIT: &str = "\
export type Deep = { wanted: { leaf: number }; cold: { c: \"c0\" } };\n";
    const SIBLING_EDIT: &str = "\
export type Deep = { wanted: { leaf: string }; cold: { c: \"c1\" } };\n";

    let context = ProjectionReductionContext::published(ProjectionMode::Expanded);
    let path = || member_path(&["wanted", "leaf"]);

    let host = make_host();
    upsert_at(&host, PROJECTION_OWNER, V1);
    let dispatch = ProjectSemanticDispatch::new(&host);
    let _trace = enable_dispatch_trace_for_test();
    let forced = force_projecting(&dispatch, &mint(&dispatch, deep_locator()), context, path());
    assert_eq!(
        primitive_kind(&host, forced.node()),
        Some(PrimitiveKind::String)
    );

    // Editing the REQUESTED key must invalidate and recompute.
    upsert_at(&host, PROJECTION_OWNER, REQUESTED_EDIT);
    let dispatch = ProjectSemanticDispatch::new(&host);
    let forced = force_projecting(&dispatch, &mint(&dispatch, deep_locator()), context, path());
    assert_eq!(
        primitive_kind(&host, forced.node()),
        Some(PrimitiveKind::Number),
        "an edit to the requested key must not serve the previous answer"
    );

    // Editing an UNRELATED sibling may conservatively invalidate, but the
    // recomputed answer must equal the one a cold host produces for the
    // same tree — never a stale or divergent result.
    upsert_at(&host, PROJECTION_OWNER, SIBLING_EDIT);
    let dispatch = ProjectSemanticDispatch::new(&host);
    let incremental =
        force_projecting(&dispatch, &mint(&dispatch, deep_locator()), context, path());

    let fresh_host = make_host();
    upsert_at(&fresh_host, PROJECTION_OWNER, SIBLING_EDIT);
    let fresh_dispatch = ProjectSemanticDispatch::new(&fresh_host);
    let fresh = force_projecting(
        &fresh_dispatch,
        &mint(&fresh_dispatch, deep_locator()),
        context,
        path(),
    );
    assert_eq!(
        primitive_kind(&host, incremental.node()),
        primitive_kind(&fresh_host, fresh.node()),
        "an unrelated sibling edit must leave the requested key's answer intact"
    );
    assert_eq!(
        primitive_kind(&host, incremental.node()),
        Some(PrimitiveKind::String)
    );

    // The unrelated-edit recompute performs ZERO sibling body/deep force
    // work: the re-lowered root's `cold` member was never projected, and
    // the whole surface of the re-lowered root was never requested. The
    // recompute shares nothing with the pre-edit node identity, so these
    // counters can only stay zero if the recompute itself stayed
    // selective.
    let recomputed_root = lowered_root(&host, &dispatch, deep_locator());
    let recomputed_whole = SemanticQueryKey::ProjectPath {
        base: recomputed_root,
        path: empty_path(),
        context,
    };
    let sibling_member = SemanticQueryKey::ProjectPath {
        base: recomputed_root,
        path: member_path(&["cold"]),
        context,
    };
    let sibling_deep = SemanticQueryKey::ProjectPath {
        base: recomputed_root,
        path: member_path(&["cold", "c"]),
        context,
    };
    for key in [&recomputed_whole, &sibling_member, &sibling_deep] {
        assert_eq!(
            dispatch_cold_for(key) + dispatch_warm_for(key),
            0,
            "an unrelated sibling edit's recompute must do zero sibling \
             body/deep force work"
        );
    }
    // Positive control: the counters are live for these exact keys.
    let _ = dispatch.execute_read(sibling_member.clone());
    assert_eq!(
        dispatch_cold_for(&sibling_member),
        1,
        "the sibling-member counter must move when the key is actually dispatched"
    );
}

// =====================================================================
// Two-file fixture: the sibling member's type is an import of a module
// that is part of the project but never demanded. A force that touched
// the sibling would have to read that module's facts, so the read set —
// not only dispatch counters — proves the sibling stayed cold.
// =====================================================================

const IMPORT_OWNER: &str = "/w/operand/import_owner.ts";
const IMPORT_DEP: &str = "/w/operand/import_sibling.ts";
const IMPORT_OWNER_SOURCE: &str = "\
import { Cold } from \"./import_sibling\";\n\
export type Deep = { wanted: { leaf: string }; cold: Cold };\n";
const IMPORT_DEP_SOURCE: &str = "\
export type Cold = { d0: \"d0\"; d1: \"d1\"; d2: \"d2\"; d3: \"d3\" };\n";

fn import_host() -> VerterHost {
    let host = make_host();
    upsert_at(&host, IMPORT_OWNER, IMPORT_OWNER_SOURCE);
    upsert_at(&host, IMPORT_DEP, IMPORT_DEP_SOURCE);
    host
}

fn import_deep_locator() -> AuthoredBodyLocator {
    locator_at(
        IMPORT_OWNER,
        "Deep",
        LocatorSymbolSpace::Type,
        Arc::from([]),
    )
}

#[test]
fn a_cold_import_sibling_contributes_no_dispatches_or_facts_to_a_residual_path_force() {
    let host = import_host();
    let dispatch = ProjectSemanticDispatch::new(&host);
    let _trace = enable_dispatch_trace_for_test();
    let context = ProjectionReductionContext::published(ProjectionMode::Expanded);

    let ctx = measured_request(23, IMPORT_OWNER);
    let _request = RequestContextGuard::install(Arc::clone(&ctx));

    // COLD: nothing pre-warmed, so the operand's own locator lowering —
    // where a sibling's declaration would be dereferenced if it were —
    // happens INSIDE the measured window.
    let operand = mint(&dispatch, import_deep_locator());
    let window = trace_len();
    let nodes_before = interned_nodes(&host);
    let forced = force_projecting(
        &dispatch,
        &operand,
        context,
        member_path(&["wanted", "leaf"]),
    );
    let classes = dispatch_classes_since(window);
    let nodes_after_selective = interned_nodes(&host);
    assert_primitive(&host, forced.node(), PrimitiveKind::String);

    // Bounded work by CLASS. `ResolveDecl` at zero is the load-bearing
    // one here: the sibling member's value IS a `DeclRef` carrier, and
    // resolving that carrier — the only way to reach the imported
    // declaration — enters exactly that family.
    assert_eq!(
        class_count(&classes, "Instantiate"),
        1,
        "one force family entry, got {classes:?}"
    );
    assert_eq!(
        class_count(&classes, "LowerLocator"),
        1,
        "exactly one locator dereference — the importing owner's own body, got {classes:?}"
    );
    assert_eq!(
        class_count(&classes, "ProjectPath"),
        1,
        "one projection at the demanded residual path, got {classes:?}"
    );
    assert_eq!(
        classes.len(),
        3,
        "no declaration resolution, instantiation, or reduction may be \
         attributable to the imported sibling, got {classes:?}"
    );
    assert_eq!(
        ctx.substitute_top_level_calls.load(Ordering::Relaxed),
        0,
        "a non-generic base substitutes nothing"
    );

    let root = lowered_root(&host, &dispatch, import_deep_locator());
    let whole_surface = SemanticQueryKey::ProjectPath {
        base: root,
        path: empty_path(),
        context,
    };
    // The whole base surface — the demand that would enumerate the `cold`
    // sibling member — was never requested.
    assert_eq!(
        dispatch_cold_for(&whole_surface) + dispatch_warm_for(&whole_surface),
        0,
        "a selective force must never request the importing base's whole surface"
    );

    // Zero DEEP work attributable to the sibling module. The owner's own
    // lowering observes the import edge's shallow facts (the sibling's
    // whole-hash and parse route — parse/shallow indexing, which the
    // non-enumeration budget explicitly excludes), so attribution is
    // asserted on the deep classes: the sibling declaration was never
    // lowered or instantiated, and the sibling member's value node was
    // never dispatched against.
    let graph = host.project_type_store().semantic_graph();
    let sibling_lower = locator_key(
        &host,
        &dispatch,
        locator_at(IMPORT_DEP, "Cold", LocatorSymbolSpace::Type, Arc::from([])),
    );
    assert_eq!(
        dispatch_cold_for(&sibling_lower) + dispatch_warm_for(&sibling_lower),
        0,
        "the sibling declaration must never be lowered to answer an unrelated key"
    );
    let facts = &forced.evidence().read_set().facts;
    assert!(
        facts.iter().all(|fact| {
            fact.canonical_id() != Some(IMPORT_DEP)
                || matches!(
                    fact,
                    crate::resolver_core::FactVersionRef::FileWholeHash { .. }
                        | crate::resolver_core::FactVersionRef::Parse(_)
                )
        }),
        "only the shallow import-edge facts (whole-hash, parse route) may be \
         attributable to the sibling module, got {facts:?}"
    );
    // The force DID observe the owner's own facts — the absence above is
    // genuine, not an empty read set.
    assert!(
        facts
            .iter()
            .any(|fact| fact.canonical_id() == Some(IMPORT_OWNER)),
        "the force must observe the owner file's facts"
    );

    // The sibling member's value node was never dispatched against.
    let cold_value = match graph.node_data(root).as_deref() {
        Some(SemanticNodeData::Object(surface)) => surface
            .positive_members()
            .iter()
            .find(|member| member.string_name() == Some("cold"))
            .map(|member| member.value)
            .expect("the `cold` sibling member exists"),
        other => panic!("the importing base must lower to an object surface, got {other:?}"),
    };
    let cold_member = SemanticQueryKey::ProjectPath {
        base: cold_value,
        path: empty_path(),
        context,
    };
    assert_eq!(
        dispatch_cold_for(&cold_member) + dispatch_warm_for(&cold_member),
        0,
        "the cold sibling member's value must never be dispatched against"
    );
    // A key-domain force over the SAME importing base decides the key
    // domain without resolving that `DeclRef` either: `keyof Deep` is
    // `"wanted" | "cold"` whatever `Cold` turns out to be.
    let keyof_window = trace_len();
    let nodes_before_keyof = interned_nodes(&host);
    let key_domain = force_key_domain(&dispatch, &operand, context);
    let keyof_classes = dispatch_classes_since(keyof_window);
    assert_eq!(
        key_union_names(&host, key_domain.node()),
        vec!["cold".to_string(), "wanted".to_string()],
        "keyof must answer the declared key domain"
    );
    assert_eq!(
        class_count(&keyof_classes, "ResolveDecl"),
        0,
        "keyof must not resolve an imported member value's declaration, got {keyof_classes:?}"
    );
    assert_eq!(
        dispatch_cold_for(&sibling_lower) + dispatch_warm_for(&sibling_lower),
        0,
        "a key-domain force must not lower the imported member value's declaration"
    );

    // Positive control: demanding the sibling member IS observable — the
    // sibling declaration lowers, so the zeros above are genuine absence
    // rather than dead instrumentation.
    let nodes_before_sibling = interned_nodes(&host);
    let _ = force_projecting(&dispatch, &operand, context, member_path(&["cold"]));
    assert!(
        dispatch_cold_for(&sibling_lower) + dispatch_warm_for(&sibling_lower) > 0,
        "demanding the sibling member must lower the sibling declaration"
    );
    // ALLOCATION bound. The sibling declaration's own body — four string
    // literals plus its object surface — is interned only now. Had the
    // selective and key-domain forces lowered it, this delta would be
    // ~zero because the shapes would already be interned; that is exactly
    // what makes it a bound on sibling-attributable allocation and not
    // merely a "something happened" counter.
    let nodes_after_sibling = interned_nodes(&host);
    assert!(
        nodes_after_sibling - nodes_before_sibling >= 5,
        "the imported sibling's body must be interned only when demanded: \
         selective {} -> {}, keyof {} -> {}, sibling {} -> {}",
        nodes_before,
        nodes_after_selective,
        nodes_before_keyof,
        nodes_before_sibling,
        nodes_before_sibling,
        nodes_after_sibling
    );
    // The value-node counter is live too — a direct dispatch of that exact
    // key records.
    let _ = dispatch.execute_read(cold_member.clone());
    assert_eq!(
        dispatch_cold_for(&cold_member),
        1,
        "the sibling-value counter must move when the key is actually dispatched"
    );
}
