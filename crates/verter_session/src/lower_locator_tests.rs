//! Locator-shape lowering contract — the sealed [`LocatorShapeCtx`] +
//! carrier-only lowering entry, the `lower_locator` provider, and the
//! first-class `SemanticQueryKey::LowerLocator` memoized query.
//!
//! The fixed authored SHAPE of a locator-addressed body lowers exactly once
//! per `(slot, locator, P, R)` family through the shared dispatch:
//!
//! - a cold demand performs ZERO reduction dispatches — conditional / mapped
//!   / `keyof` / indexed-access positions intern DEFERRED carriers, reference
//!   heads carrier-resolve to `DeclRef` / `InstantiationRef` identity, and
//!   declared type parameters stay `TypeParam` shells;
//! - the produced nodes are ROLE-FREE — no `declared_in_macro_type_arg`, no
//!   caller merge role in shape-node identity (caller-relative stamps are
//!   projection-time data, never shape identity);
//! - a warm demand is a memo hit — no second snapshot re-borrow, no
//!   re-lowering;
//! - the macro generic type argument keeps its ONE sanctioned producer
//!   (`macro_type_arg_hot_ref`) — a locator demand for it fails typed.

use std::sync::Arc;

use verter_type_expr::locators::{
    AuthoredAnchor, AuthoredBodyLocator, LocatorSymbolSpace, MacroPayloadLocator,
    MacroPayloadPosition, TypeBodyPathStep, TypeBodySlot,
};
use verter_type_expr::{ObjectExpr, ObjectMember, ObjectProperty, TypeExpr};

use crate::decl_body_memo::LocatorBodyDerefError;
use crate::project_semantic_dispatch::locator_shape::{LocatorBinderFrame, LocatorShapeCtx};
use crate::project_semantic_dispatch::ProjectSemanticDispatch;
use crate::semantic_query::{
    MemberMergeRole, NodeScopeId, ProjectionMode, ProjectionReductionContext, QueryResult,
    SemanticNodeData, SemanticNodeId, SemanticQueryKeyTag,
};
use crate::types::{HostConfig, UpsertRequest};
use crate::{CompileErrorPolicy, FileLanguage, VerterHost};

fn host() -> VerterHost {
    VerterHost::new_standalone(HostConfig {
        dev_mode: false,
        compile_error_policy: CompileErrorPolicy::StrictError,
        ..HostConfig::default()
    })
}

fn upsert_ts(host: &VerterHost, id: &str, source: &str) {
    let _ = host
        .upsert(UpsertRequest {
            canonical_id: None,
            input_id: id.to_string(),
            source: Arc::from(source),
            file_language: FileLanguage::script_ts(),
            aliases: Vec::new(),
        })
        .unwrap();
}

const OWNER_ID: &str = "/w/locator/owner.ts";
const OWNER: &str = "\
export type Base = { x: string; y: number };\n\
export type Wide = { tag: string };\n\
export type Wrapper<T> = { inner: T };\n\
export type Deferred = {\n\
    cond: Wide extends { tag: string } ? 1 : 2;\n\
    keys: keyof Base;\n\
    pick: Base[\"x\"];\n\
    mapped: { [K in keyof Base]: Base[K] };\n\
    plain: Base;\n\
    applied: Wrapper<Base>;\n\
};\n";

/// Whole-decl-body locator for a top-level TYPE symbol.
fn decl_body_locator(canonical: &str, symbol: &str) -> AuthoredBodyLocator {
    AuthoredBodyLocator::DeclBody(TypeBodySlot {
        anchor: AuthoredAnchor {
            canonical_id: Arc::from(canonical),
            symbol: Arc::from(symbol),
            space: LocatorSymbolSpace::Type,
        },
        path: Arc::from(Vec::<TypeBodyPathStep>::new().into_boxed_slice()),
    })
}

fn object_surface(host: &VerterHost, node: SemanticNodeId) -> crate::semantic_query::SurfaceView {
    let graph = host.project_type_store().semantic_graph();
    match graph.node_data(node).as_deref() {
        Some(SemanticNodeData::Object(surface)) => surface.clone(),
        other => panic!("expected an Object surface, got {other:?}"),
    }
}

fn member_value(surface: &crate::semantic_query::SurfaceView, name: &str) -> SemanticNodeId {
    surface
        .members
        .iter()
        .find(|m| m.name.as_ref() == name)
        .unwrap_or_else(|| panic!("member `{name}` must be present"))
        .value
}

/// A COLD `lower_locator` demand performs ZERO reduction dispatches: the
/// per-request dispatch mask records ONLY the `LowerLocator` tag — never
/// `Instantiate` / `ResolveDecl` / `IndexedAccess` / `Conditional` / `KeyOf`
/// / `MappedType` / `ProjectMember` / `ProjectPath` / `TypeOf` — and every
/// operator position in the produced shape is a DEFERRED carrier, not a
/// reduced result.
#[test]
fn cold_lower_locator_dispatches_zero_reduction_queries() {
    use crate::request_context::{RequestContext, RequestContextGuard};

    let host = host();
    upsert_ts(&host, OWNER_ID, OWNER);

    let ctx = RequestContext::new(1, Arc::from(OWNER_ID), false, None);
    let _guard = RequestContextGuard::install(Arc::clone(&ctx));

    let dispatch = ProjectSemanticDispatch::new(&host);
    let node = match dispatch.lower_locator(decl_body_locator(OWNER_ID, "Deferred")) {
        QueryResult::Value(id) => id,
        other => panic!("cold lower_locator must produce a value, got {other:?}"),
    };

    let tags =
        SemanticQueryKeyTag::decode_dispatch_mask(ctx.type_resolution_dispatched_query_tags_mask());
    assert!(
        tags.contains(&SemanticQueryKeyTag::LowerLocator),
        "the LowerLocator dispatch itself must be recorded; tags = {tags:?}"
    );
    for forbidden in [
        SemanticQueryKeyTag::Instantiate,
        SemanticQueryKeyTag::ResolveDecl,
        SemanticQueryKeyTag::IndexedAccess,
        SemanticQueryKeyTag::Conditional,
        SemanticQueryKeyTag::KeyOf,
        SemanticQueryKeyTag::MappedType,
        SemanticQueryKeyTag::ProjectMember,
        SemanticQueryKeyTag::ProjectPath,
        SemanticQueryKeyTag::TypeOf,
    ] {
        assert!(
            !tags.contains(&forbidden),
            "a cold LowerLocator demand must dispatch ZERO reduction queries; \
             found {forbidden:?} in the per-request dispatch mask: {tags:?}"
        );
    }

    // Output-shape half: every operator position is a DEFERRED carrier.
    let graph = host.project_type_store().semantic_graph();
    let surface = object_surface(&host, node);
    assert!(
        matches!(
            graph.node_data(member_value(&surface, "cond")).as_deref(),
            Some(SemanticNodeData::Conditional { .. })
        ),
        "a conditional position must stay a deferred Conditional carrier"
    );
    assert!(
        matches!(
            graph.node_data(member_value(&surface, "keys")).as_deref(),
            Some(SemanticNodeData::KeyOf { .. })
        ),
        "a keyof position must stay a deferred KeyOf carrier"
    );
    assert!(
        matches!(
            graph.node_data(member_value(&surface, "pick")).as_deref(),
            Some(SemanticNodeData::IndexedAccess { .. })
        ),
        "an indexed-access position must stay a deferred IndexedAccess carrier"
    );
    assert!(
        matches!(
            graph.node_data(member_value(&surface, "mapped")).as_deref(),
            Some(SemanticNodeData::Mapped { .. })
        ),
        "a mapped position must stay a deferred Mapped carrier"
    );
}

/// Reference heads carrier-resolve to IDENTITY: a bare resolvable name
/// lowers to a `DeclRef` carrier and an applied generic reference lowers to
/// an `InstantiationRef` carrier — never an executed / expanded surface.
#[test]
fn locator_shape_reference_heads_resolve_to_identity_carriers() {
    let host = host();
    upsert_ts(&host, OWNER_ID, OWNER);

    let dispatch = ProjectSemanticDispatch::new(&host);
    let node = match dispatch.lower_locator(decl_body_locator(OWNER_ID, "Deferred")) {
        QueryResult::Value(id) => id,
        other => panic!("lower_locator must produce a value, got {other:?}"),
    };

    let graph = host.project_type_store().semantic_graph();
    let surface = object_surface(&host, node);

    match graph.node_data(member_value(&surface, "plain")).as_deref() {
        Some(SemanticNodeData::DeclRef { identity }) => {
            assert_eq!(identity.canonical_id.as_ref(), OWNER_ID);
            assert_eq!(identity.decl_name.as_ref(), "Base");
        }
        other => panic!(
            "a bare resolvable reference head must lower to a DeclRef \
             identity carrier (never an executed surface); got {other:?}"
        ),
    }

    match graph
        .node_data(member_value(&surface, "applied"))
        .as_deref()
    {
        Some(SemanticNodeData::InstantiationRef { base, args }) => {
            assert_eq!(base.decl_name.as_ref(), "Wrapper");
            assert_eq!(args.len(), 1, "the applied argument list is carried");
        }
        other => panic!(
            "an applied reference head must lower to an InstantiationRef \
             identity carrier (never an executed Instantiate); got {other:?}"
        ),
    }
}

/// ROLE-FREE shape identity: the SAME authored object body lowered through
/// the OLD reducing path under a macro-own-body context carries the
/// caller-relative stamps (`declared_in_macro_type_arg = true`, an `OwnBody`
/// merge role), while the NEW locator-shape entry produces the neutral
/// stamps — so the two paths intern DISTINCT nodes and the locator shape
/// excludes the caller-relative axes from its identity.
#[test]
fn locator_shape_nodes_exclude_caller_relative_stamps() {
    let host = host();
    upsert_ts(&host, OWNER_ID, OWNER);
    let indexed = host
        .ensure_indexed_ready(OWNER_ID)
        .expect("owner must materialise");

    let dispatch = ProjectSemanticDispatch::new(&host);
    let expr = TypeExpr::Object(Arc::new(ObjectExpr {
        properties: vec![ObjectMember::Property(ObjectProperty::synthetic_public(
            "a".to_string(),
            TypeExpr::Primitive(verter_type_expr::PrimitiveName::String),
            false,
            false,
        ))],
    }));
    let scope = NodeScopeId::File {
        canonical_id: Arc::from(OWNER_ID),
        whole_hash: indexed.whole_hash,
        local_scope: None,
    };

    // OLD reducing path, macro-own-body caller context: stamps the members.
    let env = rustc_hash::FxHashMap::default();
    let name_resolution = rustc_hash::FxHashMap::default();
    let shadowing = crate::resolver_core::scope_shadowing::ScopeShadowing::empty();
    let mut substitutions = Vec::new();
    let stamped = dispatch.shallow_lower_type_expr_with_context(
        &expr,
        &env,
        &scope,
        &name_resolution,
        None,
        &shadowing,
        &mut substitutions,
        ProjectionReductionContext::published_macro_type_arg_body(ProjectionMode::Shallow)
            .with_merge_role(MemberMergeRole::OwnBody),
    );

    // NEW locator-shape entry: role-free.
    let binders: Vec<LocatorBinderFrame> = Vec::new();
    let shape_ctx = LocatorShapeCtx::new(&scope, &binders, None, None);
    let role_free = dispatch.lower_type_expr_for_locator_shape(&expr, &shape_ctx);

    assert_ne!(
        stamped, role_free,
        "the role-stamped node and the role-free locator-shape node must be \
         DISTINCT interned values"
    );

    let stamped_surface = object_surface(&host, stamped);
    assert!(
        stamped_surface.members[0].declared_in_macro_type_arg.get(),
        "control: the OLD path under a macro-own-body context stamps \
         declared_in_macro_type_arg"
    );
    assert_eq!(
        stamped_surface.members[0].merge_role,
        MemberMergeRole::OwnBody,
        "control: the OLD path stamps the caller merge role"
    );

    let role_free_surface = object_surface(&host, role_free);
    assert!(
        !role_free_surface.members[0]
            .declared_in_macro_type_arg
            .get(),
        "the locator-shape entry must NOT stamp declared_in_macro_type_arg"
    );
    assert_eq!(
        role_free_surface.members[0].merge_role,
        MemberMergeRole::Authored,
        "the locator-shape entry must carry the neutral merge role"
    );
}

/// Declared type parameters stay `TypeParam` SHELLS: the generic decl's own
/// parameter position lowers to the binder shell, never a substituted or
/// resolved node.
#[test]
fn locator_shape_keeps_declared_type_parameters_as_shells() {
    let host = host();
    upsert_ts(&host, OWNER_ID, OWNER);

    let dispatch = ProjectSemanticDispatch::new(&host);
    let node = match dispatch.lower_locator(decl_body_locator(OWNER_ID, "Wrapper")) {
        QueryResult::Value(id) => id,
        other => panic!("lower_locator must produce a value, got {other:?}"),
    };

    let graph = host.project_type_store().semantic_graph();
    let surface = object_surface(&host, node);
    match graph.node_data(member_value(&surface, "inner")).as_deref() {
        Some(SemanticNodeData::TypeParam { display_name, .. }) => {
            assert_eq!(display_name.as_ref(), "T");
        }
        other => panic!("a declared type parameter must stay a TypeParam shell, got {other:?}"),
    }
}

/// A WARM `lower_locator` demand is a memo hit: the same node id returns and
/// NO second snapshot re-borrow / re-lowering happens (the per-host
/// `decl_bodies_lowered` counter stays flat across the warm demand).
#[test]
fn warm_lower_locator_demand_is_a_memo_hit_without_relowering() {
    let host = host();
    upsert_ts(&host, OWNER_ID, OWNER);
    host.provenance().reset();

    let dispatch = ProjectSemanticDispatch::new(&host);
    let locator = decl_body_locator(OWNER_ID, "Base");
    let cold = match dispatch.lower_locator(locator.clone()) {
        QueryResult::Value(id) => id,
        other => panic!("cold lower_locator must produce a value, got {other:?}"),
    };
    let lowered_after_cold = host.provenance().snapshot().decl_bodies_lowered;
    assert!(
        lowered_after_cold > 0,
        "the cold demand must lower the demanded declaration body"
    );

    let warm = match dispatch.lower_locator(locator) {
        QueryResult::Value(id) => id,
        other => panic!("warm lower_locator must produce a value, got {other:?}"),
    };
    assert_eq!(cold, warm, "the warm demand must return the memoized node");
    assert_eq!(
        host.provenance().snapshot().decl_bodies_lowered,
        lowered_after_cold,
        "a warm LowerLocator demand must be a memo hit — no second snapshot \
         re-borrow, no re-lowering"
    );
}

/// The macro generic type argument has exactly ONE sanctioned producer
/// (`macro_type_arg_hot_ref`): a locator deref for the `TypeArgument`
/// payload position fails with the typed routing error — never a second
/// lowering path — and the provider surfaces it as an honest Miss.
#[test]
fn lower_locator_rejects_macro_type_argument_payload() {
    let host = host();
    upsert_ts(&host, OWNER_ID, OWNER);

    let locator = AuthoredBodyLocator::MacroPayload(MacroPayloadLocator {
        anchor: AuthoredAnchor {
            canonical_id: Arc::from(OWNER_ID),
            symbol: Arc::from("default"),
            space: LocatorSymbolSpace::Value,
        },
        macro_index: 0,
        payload: MacroPayloadPosition::TypeArgument,
    });

    let indexed = host
        .ensure_indexed_ready(OWNER_ID)
        .expect("owner must materialise");
    let deref = indexed
        .shallow_state
        .decl_bodies()
        .deref_locator_body(&locator);
    assert_eq!(
        deref.expect_err("a TypeArgument payload deref must fail typed"),
        LocatorBodyDerefError::MacroTypeArgumentHasSoleHotMirrorProducer,
        "the macro generic type argument keeps its sole sanctioned producer"
    );

    let dispatch = ProjectSemanticDispatch::new(&host);
    assert!(
        matches!(
            dispatch.lower_locator(locator),
            QueryResult::Error(crate::semantic_query::QueryError::Miss)
        ),
        "the provider must surface the typed rejection as an honest Miss"
    );
}

/// A broken-lease `LowerLocator` deref is a TRANSIENT ReturnOnly, not a
/// genuine miss: the child build SUPPRESSES admission (`cache_suppress`) so
/// the enclosing `Instantiate` / `LowerLocator` query does NOT warm-publish
/// the derived `Opaque(Miss)` as a false body. The prior fix proved only the
/// `DeclBodyMemo` cell is left uncommitted; this pins the PARENT-query taint
/// end-to-end through `deref_locator_body` → `build_lower_locator`.
///
/// Discrimination (RED against the pre-change tree, GREEN after): pre-change
/// the deref collapsed the lease-miss into `UnknownSymbol` and
/// `build_lower_locator` returned `Error(Miss)` WITHOUT `cache_suppress`, so
/// the `LowerLocator` `CacheRead` reported `cache_suppress == false` — the
/// universal read-boundary fold would then let the enclosing build warm-admit
/// a result embedding the false `Opaque(Miss)`. Post-change the `LeaseMiss`
/// sets `cache_suppress == true`.
#[test]
fn broken_lease_lower_locator_suppresses_parent_admission() {
    use crate::locator_identity::{
        semantic_space_for_locator_space, LocatorLoweringKey, ParseEnvHash, ResolveEnvHash,
    };
    use crate::semantic_query::{QueryError, SemanticQueryKey};

    let host = host();
    upsert_ts(&host, OWNER_ID, OWNER);

    // Materialise the owner's IndexedReady + its DeclBodyMemo. Pin the lease
    // with an UNRELATED body demand (never touches the `Base` LowerLocator
    // memo), then break the memo's worker-retained snapshot so the next
    // `Base` deref lease-misses — the memo still HOLDS its lease, so
    // `ensure_lease` will not re-acquire.
    let indexed = host
        .ensure_indexed_ready(OWNER_ID)
        .expect("owner must materialise");
    let memo = indexed.shallow_state.decl_bodies();
    assert!(
        memo.type_decl("Wide").is_some(),
        "the unrelated demand must pin the lease"
    );
    memo.release_retained_snapshot_for_test();

    // Build the SAME LowerLocator key `lower_locator` builds for `Base`.
    let locator = decl_body_locator(OWNER_ID, "Base");
    let (canonical_id, symbol, space) = match &locator {
        AuthoredBodyLocator::DeclBody(slot) => (
            Arc::clone(&slot.anchor.canonical_id),
            Arc::clone(&slot.anchor.symbol),
            slot.anchor.space,
        ),
        _ => unreachable!("decl_body_locator builds a DeclBody locator"),
    };
    let dispatch = ProjectSemanticDispatch::new(&host);
    let slot = dispatch
        .type_slot_for(Arc::clone(&canonical_id), symbol)
        .with_symbol_space(semantic_space_for_locator_space(space));
    let env = host.host_view_env_hashes_for(canonical_id.as_ref());
    let key = LocatorLoweringKey::new_unsubstituted(
        slot,
        locator,
        ParseEnvHash::from_env_hash(env.parse_env_hash),
        ResolveEnvHash::from_env_hash(env.resolve_env_hash),
    )
    .expect("the locator anchor names its own slot");

    // A COLD LowerLocator demand (the `Base` shape was never lowered) runs
    // `build_lower_locator`, whose deref now lease-misses.
    let read = dispatch.execute_read(SemanticQueryKey::LowerLocator { key });
    assert!(
        matches!(read.value, QueryResult::Error(QueryError::Miss)),
        "a broken-lease deref must fail closed to an Error(Miss), got {:?}",
        read.value
    );
    assert!(
        read.cache_suppress,
        "a broken-lease LowerLocator child must SUPPRESS memo admission so the \
         enclosing Instantiate/LowerLocator query does not warm-publish the \
         derived Opaque(Miss) as a false body; cache_suppress == false means the \
         transient ReturnOnly collapsed into a cacheable UnknownSymbol"
    );
}

/// Reference heads resolve in the AUTHORED declaration's own lexical scope:
/// a namespace member body's bare `Foo` binds the SHADOWING namespace
/// sibling (`NS.Foo`) — the same identity the declaration's
/// `name_resolution` map (and therefore the reducing path) resolves — never
/// the same-named top-level symbol a scope-less lookup would return. The
/// cached `LowerLocator` shape carries the reference IDENTITY, so a
/// wrong-scope resolution here poisons every downstream projection.
#[test]
fn locator_ref_head_resolves_shadowing_namespace_sibling_not_top_level() {
    let host = host();
    upsert_ts(
        &host,
        OWNER_ID,
        "export interface Foo { top: string }\n\
         export namespace NS {\n\
             export interface Foo { nested: number }\n\
             export interface Bar { field: Foo }\n\
         }\n",
    );

    let dispatch = ProjectSemanticDispatch::new(&host);
    let node = match dispatch.lower_locator(decl_body_locator(OWNER_ID, "NS.Bar")) {
        QueryResult::Value(id) => id,
        other => panic!("lower_locator must produce a value, got {other:?}"),
    };
    let graph = host.project_type_store().semantic_graph();
    let surface = object_surface(&host, node);
    match graph.node_data(member_value(&surface, "field")).as_deref() {
        Some(SemanticNodeData::DeclRef { identity }) => {
            assert_eq!(identity.canonical_id.as_ref(), OWNER_ID);
            assert_eq!(
                identity.decl_name.as_ref(),
                "NS.Foo",
                "inside `namespace NS`, the bare `Foo` binds the namespace \
                 sibling (TS namespace scope) — the cached reference identity \
                 must be the SHADOWING `NS.Foo`, not the top-level `Foo`"
            );
        }
        other => panic!("the reference head must stay a DeclRef identity carrier, got {other:?}"),
    }
}

/// A barrel retarget (owner file UNCHANGED) must MISS the warm
/// `LowerLocator` memo: the cold build's ref-head resolution records the
/// barrel/re-export route-chain facts onto the entry's read-set, so editing
/// the barrel to re-export the same name from a DIFFERENT defining file
/// invalidates the cached carrier identity instead of serving the stale
/// `DeclRef` warm (the false-warm class: only the owner's self-root was
/// recorded, and the owner never changed).
#[test]
fn barrel_retarget_misses_warm_locator_shape_without_owner_edit() {
    let host = host();
    upsert_ts(
        &host,
        "/w/locator/dep_a.ts",
        "export type Boxed = { fromA: string };\n",
    );
    upsert_ts(
        &host,
        "/w/locator/dep_b.ts",
        "export type Boxed = { fromB: number };\n",
    );
    upsert_ts(
        &host,
        "/w/locator/barrel.ts",
        "export { Boxed } from './dep_a';\n",
    );
    upsert_ts(
        &host,
        "/w/locator/ref_owner.ts",
        "import { Boxed } from './barrel';\nexport type Holder = { field: Boxed };\n",
    );

    let dispatch = ProjectSemanticDispatch::new(&host);
    let locator = decl_body_locator("/w/locator/ref_owner.ts", "Holder");
    let cold = match dispatch.lower_locator(locator.clone()) {
        QueryResult::Value(id) => id,
        other => panic!("cold lower_locator must produce a value, got {other:?}"),
    };
    let graph = host.project_type_store().semantic_graph();
    let surface = object_surface(&host, cold);
    match graph.node_data(member_value(&surface, "field")).as_deref() {
        Some(SemanticNodeData::DeclRef { identity }) => {
            assert_eq!(
                identity.canonical_id.as_ref(),
                "/w/locator/dep_a.ts",
                "control: the cold ref-head resolves through the barrel to the \
                 ORIGINAL defining file"
            );
        }
        other => panic!("the reference head must be a DeclRef identity carrier, got {other:?}"),
    }

    // Retarget the barrel to a DIFFERENT defining file. The owner file is
    // NOT touched, so its whole-hash self-root still validates — only the
    // recorded route-chain facts can reject the warm entry.
    upsert_ts(
        &host,
        "/w/locator/barrel.ts",
        "export { Boxed } from './dep_b';\n",
    );

    let after = match dispatch.lower_locator(locator) {
        QueryResult::Value(id) => id,
        other => panic!("post-retarget lower_locator must produce a value, got {other:?}"),
    };
    let surface = object_surface(&host, after);
    match graph.node_data(member_value(&surface, "field")).as_deref() {
        Some(SemanticNodeData::DeclRef { identity }) => {
            assert_eq!(
                identity.canonical_id.as_ref(),
                "/w/locator/dep_b.ts",
                "a barrel retarget with the owner unchanged must MISS the warm \
                 locator-shape memo (the read-set carries the route proof) and \
                 re-resolve the reference head to the NEW defining file"
            );
        }
        other => panic!("the reference head must stay a DeclRef identity carrier, got {other:?}"),
    }
}

/// A sub-position locator (a `TypeBodyPathStep` path) derefs and lowers
/// exactly the named authored position — the member's value type — in the
/// authored lexical scope.
#[test]
fn lower_locator_derefs_a_member_value_sub_position() {
    let host = host();
    upsert_ts(&host, OWNER_ID, OWNER);

    let dispatch = ProjectSemanticDispatch::new(&host);
    // `Base` member ordinal 0 is `x: string`.
    let locator = AuthoredBodyLocator::DeclBody(TypeBodySlot {
        anchor: AuthoredAnchor {
            canonical_id: Arc::from(OWNER_ID),
            symbol: Arc::from("Base"),
            space: LocatorSymbolSpace::Type,
        },
        path: Arc::from(
            vec![
                TypeBodyPathStep::Member { ordinal: 0 },
                TypeBodyPathStep::MemberValue,
            ]
            .into_boxed_slice(),
        ),
    });
    let node = match dispatch.lower_locator(locator) {
        QueryResult::Value(id) => id,
        other => panic!("sub-position lower_locator must produce a value, got {other:?}"),
    };
    let graph = host.project_type_store().semantic_graph();
    assert!(
        matches!(
            graph.node_data(node).as_deref(),
            Some(SemanticNodeData::Primitive(
                crate::semantic_query::PrimitiveKind::String
            ))
        ),
        "the `Base` member-0 value position is the authored `string`"
    );
}
