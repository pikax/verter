//! U2B.5 guards — the class / namespace / enum / overload key surface.
//!
//! These tests pin the IDENTITY contract of the four new
//! [`SemanticQueryKey`] variants (`ResolveClassSurface`,
//! `ResolveAmbientNamespace`, `ResolveEnum`, `ResolveOverloadSet`), the
//! `SemanticSymbolSpace::Namespace` arm, and the class dual-space routing
//! through the ONE shared engine.
//!
//! Identity is probed BEHAVIORALLY through the family memo: publishing a
//! synthetic candidate under key `a` and then reading
//! `slot_candidate_count_for_tests(b)` is `> 0` iff `a` and `b` project to
//! the SAME `(FamilyKey, ModeSlot)`. This is the same probe
//! `family_slots_multi_candidate.rs` uses, and it discriminates the exact
//! property the brief requires: a warm entry under one identity is
//! returned for another ONLY when they share a slot.

use std::collections::HashSet;
use std::sync::Arc;

use verter_session::for_tests::ReadSetSignature;
use verter_session::semantic_query::{
    AmbientNamespaceContext, ClassSurfaceContext, ClassSurfaceSide, DeclKey, EnumContext,
    OverloadSetContext, PrimitiveKind, ProjectionMode, ProjectionReductionContext, QueryResult,
    ResolvedDeclSlotIdentity, SemanticNodeData, SemanticNodeId, SemanticQueryKey,
    SemanticSymbolSpace, ValueRootKey,
};
use verter_session::{FileKind, HostConfig, UpsertRequest, VerterHost};

fn host() -> VerterHost {
    VerterHost::new_standalone(HostConfig::default())
}

fn upsert(host: &VerterHost, path: &str, source: &str) {
    let _ = host
        .upsert(UpsertRequest {
            canonical_id: Some(path.to_string()),
            input_id: path.to_string(),
            source: Arc::from(source),
            file_kind: FileKind::from_path(path),
            aliases: Vec::new(),
        })
        .expect("upsert succeeds");
}

fn hash16(byte: u8) -> [u8; 16] {
    [byte; 16]
}

fn slot(canonical: &str, name: &str, space: SemanticSymbolSpace) -> ResolvedDeclSlotIdentity {
    ResolvedDeclSlotIdentity {
        defining_canonical: Arc::from(canonical),
        merged_symbol_name: Arc::from(name),
        symbol_space: space,
        project_identity: 0,
        type_env_hash: Default::default(),
        lib_env_hash: Default::default(),
    }
}

fn dummy_node() -> SemanticNodeId {
    SemanticNodeId(1)
}

/// Publish a synthetic candidate under `a`, then return the candidate
/// count `b` projects to. `> 0` ⟺ `a` and `b` share a `(FamilyKey, slot)`.
///
/// A FRESH host per call keeps every pair independent (no cross-pollution
/// from a prior publish). `ProjectionMode::Identity` is used by the
/// callers that build keys here so backfill (which fans broader→narrower
/// slots) never muddies the probe — `Identity` backfills nothing.
fn count_for_b_after_publishing_a(a: &SemanticQueryKey, b: &SemanticQueryKey) -> usize {
    let host = host();
    let graph = host.project_type_store().semantic_graph();
    let node = graph.intern_node(SemanticNodeData::Primitive(PrimitiveKind::Number));
    graph.publish_with_carrier_dispatch_and_generation_for_tests(
        a.clone(),
        QueryResult::Value(node),
        ReadSetSignature::empty(),
        Arc::from(Vec::<Arc<str>>::new().into_boxed_slice()),
        Arc::from(Vec::new().into_boxed_slice()),
        100,
    );
    graph.slot_candidate_count_for_tests(b)
}

/// `a` and `b` are NON-equal keys AND project to DISTINCT `(FamilyKey,
/// slot)` — a warm entry under `a` is unreachable from `b`. Also asserts
/// the positive sanity (`a` reaches its own slot) so the probe is not
/// vacuously passing on a broken publish path.
fn assert_distinct_identity(a: &SemanticQueryKey, b: &SemanticQueryKey) {
    assert_ne!(a, b, "keys must be non-equal");
    assert_eq!(
        count_for_b_after_publishing_a(a, a),
        1,
        "sanity: publishing `a` must reach `a`'s own slot (count 1)"
    );
    assert_eq!(
        count_for_b_after_publishing_a(a, b),
        0,
        "a warm candidate published under `a` must NOT be reachable from \
         `b` — they must project to DISTINCT (FamilyKey, slot)"
    );
}

fn class_surface_key(
    canonical: &str,
    name: &str,
    type_args: Arc<[SemanticNodeId]>,
    side: ClassSurfaceSide,
    parse_env: u8,
    resolve_env: u8,
) -> SemanticQueryKey {
    SemanticQueryKey::ResolveClassSurface {
        decl_slot: slot(canonical, name, SemanticSymbolSpace::Type),
        type_args,
        side,
        context: ClassSurfaceContext {
            parse_env_hash: hash16(parse_env),
            resolve_env_hash: hash16(resolve_env),
            // Identity rung → no backfill fan-out muddies the probe.
            mode: ProjectionMode::Identity,
        },
    }
}

fn ambient_namespace_key(
    canonical: &str,
    name: &str,
    parse_env: u8,
    resolve_env: u8,
) -> SemanticQueryKey {
    SemanticQueryKey::ResolveAmbientNamespace {
        namespace_slot: slot(canonical, name, SemanticSymbolSpace::Namespace),
        type_args: Arc::from(Vec::new().into_boxed_slice()),
        context: AmbientNamespaceContext {
            parse_env_hash: hash16(parse_env),
            resolve_env_hash: hash16(resolve_env),
            mode: ProjectionMode::Identity,
        },
    }
}

fn enum_key(canonical: &str, name: &str, resolve_env: u8) -> SemanticQueryKey {
    SemanticQueryKey::ResolveEnum {
        enum_slot: slot(canonical, name, SemanticSymbolSpace::Type),
        context: EnumContext {
            resolve_env_hash: hash16(resolve_env),
        },
    }
}

fn overload_set_key(callee: SemanticNodeId, resolve_env: u8) -> SemanticQueryKey {
    SemanticQueryKey::ResolveOverloadSet {
        callee,
        type_args: Arc::from(Vec::new().into_boxed_slice()),
        context: OverloadSetContext {
            resolve_env_hash: hash16(resolve_env),
        },
    }
}

// ---------------------------------------------------------------------------
// (1) ResolveClassSurface identity covers side + type_args + context.
// ---------------------------------------------------------------------------

#[test]
fn resolve_class_surface_key_covers_side_demand_type_args_and_context() {
    let base = class_surface_key(
        "/c.ts",
        "Foo",
        Arc::from([]),
        ClassSurfaceSide::Instance,
        0,
        0,
    );

    // `side` is a MANDATORY identity discriminator. Instance vs Static of
    // the SAME class + args + context MUST be non-equal AND occupy
    // distinct memo slots. A single-slot impl that ignored `side` would
    // make these SHARE a slot — `assert_distinct_identity` would then see
    // count 1 (not 0) and FAIL. This is the discriminating negative.
    let static_side = class_surface_key(
        "/c.ts",
        "Foo",
        Arc::from([]),
        ClassSurfaceSide::Static,
        0,
        0,
    );
    assert_distinct_identity(&base, &static_side);

    // type_args is part of semantic identity.
    let with_args = class_surface_key(
        "/c.ts",
        "Foo",
        Arc::from(vec![dummy_node()].into_boxed_slice()),
        ClassSurfaceSide::Instance,
        0,
        0,
    );
    assert_distinct_identity(&base, &with_args);

    // A context env-hash difference (resolve_env) is part of identity.
    let other_resolve_env = class_surface_key(
        "/c.ts",
        "Foo",
        Arc::from([]),
        ClassSurfaceSide::Instance,
        0,
        9,
    );
    assert_distinct_identity(&base, &other_resolve_env);

    // A context env-hash difference (parse_env) is part of identity.
    let other_parse_env = class_surface_key(
        "/c.ts",
        "Foo",
        Arc::from([]),
        ClassSurfaceSide::Instance,
        9,
        0,
    );
    assert_distinct_identity(&base, &other_parse_env);
}

// ---------------------------------------------------------------------------
// (2) Per-key identity covers each context's env-hash dimension.
// ---------------------------------------------------------------------------

#[test]
fn resolve_ambient_namespace_key_covers_context() {
    let base = ambient_namespace_key("/n.ts", "N", 0, 0);
    assert_distinct_identity(&base, &ambient_namespace_key("/n.ts", "N", 0, 9));
    assert_distinct_identity(&base, &ambient_namespace_key("/n.ts", "N", 9, 0));
}

#[test]
fn resolve_enum_key_covers_context() {
    let base = enum_key("/e.ts", "E", 0);
    // EnumContext carries ONLY resolve_env_hash (R) — vary it.
    assert_distinct_identity(&base, &enum_key("/e.ts", "E", 9));
}

#[test]
fn resolve_overload_set_key_covers_context() {
    let base = overload_set_key(dummy_node(), 0);
    // OverloadSetContext carries ONLY resolve_env_hash (R) — vary it.
    assert_distinct_identity(&base, &overload_set_key(dummy_node(), 9));
    // callee is part of identity.
    assert_distinct_identity(&base, &overload_set_key(SemanticNodeId(2), 0));
}

// ---------------------------------------------------------------------------
// (3) `*_do_not_warm_hit` — same site, different env/context ⇒ distinct
//     (FamilyKey, slot), so a warm entry under one context CANNOT be
//     returned for the other. Names match the spec-row cross_context_guard.
// ---------------------------------------------------------------------------

#[test]
fn resolve_class_surface_do_not_warm_hit() {
    // Same class site, different resolve_env (the R21 split-env
    // convention) — must not warm-hit across the env boundary.
    let env_a = class_surface_key(
        "/c.ts",
        "Foo",
        Arc::from([]),
        ClassSurfaceSide::Instance,
        0,
        0,
    );
    let env_b = class_surface_key(
        "/c.ts",
        "Foo",
        Arc::from([]),
        ClassSurfaceSide::Instance,
        0,
        1,
    );
    assert_eq!(
        count_for_b_after_publishing_a(&env_a, &env_b),
        0,
        "ResolveClassSurface must not warm-hit across a resolve_env boundary"
    );
}

#[test]
fn resolve_ambient_namespace_do_not_warm_hit() {
    let env_a = ambient_namespace_key("/n.ts", "N", 0, 0);
    let env_b = ambient_namespace_key("/n.ts", "N", 0, 1);
    assert_eq!(
        count_for_b_after_publishing_a(&env_a, &env_b),
        0,
        "ResolveAmbientNamespace must not warm-hit across a resolve_env boundary"
    );
}

#[test]
fn resolve_enum_do_not_warm_hit() {
    let env_a = enum_key("/e.ts", "E", 0);
    let env_b = enum_key("/e.ts", "E", 1);
    assert_eq!(
        count_for_b_after_publishing_a(&env_a, &env_b),
        0,
        "ResolveEnum must not warm-hit across a resolve_env boundary"
    );
}

#[test]
fn resolve_overload_set_do_not_warm_hit() {
    let env_a = overload_set_key(dummy_node(), 0);
    let env_b = overload_set_key(dummy_node(), 1);
    assert_eq!(
        count_for_b_after_publishing_a(&env_a, &env_b),
        0,
        "ResolveOverloadSet must not warm-hit across a resolve_env boundary"
    );
}

// ---------------------------------------------------------------------------
// (4) SemanticSymbolSpace::Namespace keys distinctly; NO BothTypeValue.
// ---------------------------------------------------------------------------

#[test]
fn semantic_symbol_space_namespace_keys_distinctly() {
    // Three slots for the SAME (canonical, name) under the three symbol
    // spaces are pairwise distinct — a `Namespace` declaration never
    // conflates with the type-space or value-space half.
    let type_slot = slot("/m.ts", "X", SemanticSymbolSpace::Type);
    let value_slot = slot("/m.ts", "X", SemanticSymbolSpace::Value);
    let namespace_slot = slot("/m.ts", "X", SemanticSymbolSpace::Namespace);

    let set: HashSet<ResolvedDeclSlotIdentity> = [
        type_slot.clone(),
        value_slot.clone(),
        namespace_slot.clone(),
    ]
    .into_iter()
    .collect();
    assert_eq!(
        set.len(),
        3,
        "Type / Value / Namespace slots of the same (canonical, name) must \
         be three DISTINCT keys"
    );
    assert_ne!(type_slot, namespace_slot);
    assert_ne!(value_slot, namespace_slot);
    assert_ne!(type_slot, value_slot);

    // NO `BothTypeValue` arm: the enum has EXACTLY Type / Value /
    // Namespace. This exhaustive match fails to compile if a fused
    // `BothTypeValue` arm is ever introduced (the discriminating negative
    // — a class/enum is TWO slots, never one fused arm).
    fn enumerate(space: SemanticSymbolSpace) -> &'static str {
        match space {
            SemanticSymbolSpace::Type => "Type",
            SemanticSymbolSpace::Value => "Value",
            SemanticSymbolSpace::Namespace => "Namespace",
        }
    }
    assert_eq!(enumerate(SemanticSymbolSpace::Type), "Type");
    assert_eq!(enumerate(SemanticSymbolSpace::Value), "Value");
    assert_eq!(enumerate(SemanticSymbolSpace::Namespace), "Namespace");
}

// ---------------------------------------------------------------------------
// (5) Class dual-space — instance (TYPE slot) vs static (VALUE slot) route
//     through DISTINCT shared-dispatch paths, with NO query-time OXC.
// ---------------------------------------------------------------------------

#[test]
fn class_dual_space_routes_instance_and_static_through_distinct_shared_paths() {
    let canonical = "/dual/cls.ts";
    let host = host();
    upsert(
        &host,
        canonical,
        "export class Foo {\n  x: number = 1;\n  static y: string = \"a\";\n}\n",
    );

    let decl_slot = slot(canonical, "Foo", SemanticSymbolSpace::Type);
    let ctx = ClassSurfaceContext {
        parse_env_hash: Default::default(),
        resolve_env_hash: Default::default(),
        mode: ProjectionMode::Shallow,
    };

    // Instance side → composes execute(Instantiate { type_slot, Shallow }).
    let instance = run_class_surface(&host, &decl_slot, ClassSurfaceSide::Instance, ctx);
    // Static side → composes execute(TypeOf { value_root_of(value_slot) }).
    let r#static = run_class_surface(&host, &decl_slot, ClassSurfaceSide::Static, ctx);

    // The two sides resolve DIFFERENT halves of the class, so they must
    // produce DIFFERENT result nodes. A single-path impl (both sides
    // routing through one query) would collapse them to the same node and
    // FAIL here.
    assert_ne!(
        instance, r#static,
        "instance (TYPE-space Instantiate) and static (VALUE-space TypeOf) \
         surfaces must resolve to DISTINCT nodes — they route through \
         distinct shared-dispatch paths"
    );

    // Both sides route through the shared memo: re-running the SAME
    // ResolveClassSurface key warm-hits its own admitted slot. This is the
    // structural proof that resolution went through `execute()` (the one
    // shared engine) and admitted a Singleflight result — NOT through a
    // private OXC re-parse / per-surface walker (which would never land in
    // the family memo).
    let graph = host.project_type_store().semantic_graph();
    let instance_key = SemanticQueryKey::ResolveClassSurface {
        decl_slot: decl_slot.clone(),
        type_args: Arc::from(Vec::new().into_boxed_slice()),
        side: ClassSurfaceSide::Instance,
        context: ctx,
    };
    assert!(
        graph.slot_candidate_count_for_tests(&instance_key) > 0,
        "ResolveClassSurface(Instance) must admit into the shared family \
         memo (Singleflight producer) — proving it routed through execute()"
    );

    // And the inner sub-queries the dual-space algorithm composes are
    // themselves warm in the shared memo: Instance warmed the
    // Instantiate(type DeclKey) slot.
    let inner_instantiate = SemanticQueryKey::Instantiate {
        base: DeclKey {
            canonical_id: Arc::from(canonical),
            decl_name: Arc::from("Foo"),
        },
        args: Arc::from(Vec::new().into_boxed_slice()),
        context: ProjectionReductionContext::published(ProjectionMode::Shallow),
    };
    assert!(
        graph.slot_candidate_count_for_tests(&inner_instantiate) > 0,
        "Instance side must have composed execute(Instantiate) — the inner \
         Instantiate(type slot) entry must be warm in the shared memo"
    );

    // Static warmed the TypeOf(value_root) slot.
    let inner_typeof = SemanticQueryKey::TypeOf {
        value_root: ValueRootKey {
            scope: verter_session::semantic_query::ScopeId::file(Arc::from(canonical)),
            name: Arc::from("Foo"),
        },
    };
    assert!(
        graph.slot_candidate_count_for_tests(&inner_typeof) > 0,
        "Static side must have composed execute(TypeOf) — the inner \
         TypeOf(value root) entry must be warm in the shared memo"
    );
}

fn run_class_surface(
    host: &VerterHost,
    decl_slot: &ResolvedDeclSlotIdentity,
    side: ClassSurfaceSide,
    ctx: ClassSurfaceContext,
) -> SemanticNodeId {
    let key = SemanticQueryKey::ResolveClassSurface {
        decl_slot: decl_slot.clone(),
        type_args: Arc::from(Vec::new().into_boxed_slice()),
        side,
        context: ctx,
    };
    match verter_session::for_tests::dispatch_execute_type_node_for_tests(host, key) {
        QueryResult::Value(out) => out.value,
        QueryResult::Recursive(node) => node,
        QueryResult::Error(err) => panic!("ResolveClassSurface({side:?}) errored: {err:?}"),
    }
}
