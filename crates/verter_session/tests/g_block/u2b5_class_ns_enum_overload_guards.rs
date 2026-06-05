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
    OverloadSetContext, PrimitiveKind, ProjectionMode, ProjectionReductionContext, QueryError,
    QueryResult, ResolvedDeclSlotIdentity, SemanticNodeData, SemanticNodeId, SemanticQueryKey,
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
    resolve_env: u8,
) -> SemanticQueryKey {
    class_surface_key_with_mode(
        canonical,
        name,
        type_args,
        side,
        resolve_env,
        // Identity rung → no backfill fan-out muddies the probe.
        ProjectionMode::Identity,
    )
}

/// `class_surface_key` with an explicit projection `mode` — used by the
/// mode-axis identity test. The slot's symbol space is `Type`.
fn class_surface_key_with_mode(
    canonical: &str,
    name: &str,
    type_args: Arc<[SemanticNodeId]>,
    side: ClassSurfaceSide,
    resolve_env: u8,
    mode: ProjectionMode,
) -> SemanticQueryKey {
    SemanticQueryKey::ResolveClassSurface {
        decl_slot: slot(canonical, name, SemanticSymbolSpace::Type),
        type_args,
        side,
        context: ClassSurfaceContext {
            resolve_env_hash: hash16(resolve_env),
            mode,
        },
    }
}

fn ambient_namespace_key(canonical: &str, name: &str, resolve_env: u8) -> SemanticQueryKey {
    ambient_namespace_key_with_mode(canonical, name, resolve_env, ProjectionMode::Identity)
}

/// `ambient_namespace_key` with an explicit projection `mode` — used by the
/// mode-axis identity test.
fn ambient_namespace_key_with_mode(
    canonical: &str,
    name: &str,
    resolve_env: u8,
    mode: ProjectionMode,
) -> SemanticQueryKey {
    SemanticQueryKey::ResolveAmbientNamespace {
        namespace_slot: slot(canonical, name, SemanticSymbolSpace::Namespace),
        type_args: Arc::from(Vec::new().into_boxed_slice()),
        context: AmbientNamespaceContext {
            resolve_env_hash: hash16(resolve_env),
            mode,
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
    let base = class_surface_key("/c.ts", "Foo", Arc::from([]), ClassSurfaceSide::Instance, 0);

    // `side` is a MANDATORY identity discriminator. Instance vs Static of
    // the SAME class + args + context MUST be non-equal AND occupy
    // distinct memo slots. A single-slot impl that ignored `side` would
    // make these SHARE a slot — `assert_distinct_identity` would then see
    // count 1 (not 0) and FAIL. This is the discriminating negative.
    let static_side = class_surface_key("/c.ts", "Foo", Arc::from([]), ClassSurfaceSide::Static, 0);
    assert_distinct_identity(&base, &static_side);

    // type_args is part of semantic identity.
    let with_args = class_surface_key(
        "/c.ts",
        "Foo",
        Arc::from(vec![dummy_node()].into_boxed_slice()),
        ClassSurfaceSide::Instance,
        0,
    );
    assert_distinct_identity(&base, &with_args);

    // A context env-hash difference (resolve_env) is part of identity.
    // `ClassSurfaceContext` carries ONLY `resolve_env_hash` (`R`) + `mode`
    // — there is deliberately NO `parse_env` axis (R21: the composed
    // surface identity-routes `Instantiate` + `TypeOf` and reads no parsed
    // body skeleton, so keying on `parse_env` would be a dead axis).
    let other_resolve_env =
        class_surface_key("/c.ts", "Foo", Arc::from([]), ClassSurfaceSide::Instance, 9);
    assert_distinct_identity(&base, &other_resolve_env);
}

// ---------------------------------------------------------------------------
// (1b) ResolveClassSurface identity is PATH-INDEPENDENT w.r.t. the incoming
//      slot's symbol_space: two keys differing ONLY in `decl_slot.symbol_space`
//      compute the SAME value (side selects the half; the build ignores
//      symbol_space) and so MUST share one (FamilyKey, slot).
// ---------------------------------------------------------------------------

#[test]
fn resolve_class_surface_identity_canonicalizes_decl_slot_symbol_space() {
    // Same class site + side + args + context, but the incoming slot's
    // symbol space differs (Type vs Value vs Namespace). The family key
    // canonicalizes the slot's symbol_space, so all three project to the
    // SAME slot — a warm entry under one is reachable from the others.
    let key_for = |space: SemanticSymbolSpace| SemanticQueryKey::ResolveClassSurface {
        decl_slot: slot("/c.ts", "Foo", space),
        type_args: Arc::from(Vec::new().into_boxed_slice()),
        side: ClassSurfaceSide::Instance,
        context: ClassSurfaceContext {
            resolve_env_hash: hash16(0),
            mode: ProjectionMode::Identity,
        },
    };
    let type_space = key_for(SemanticSymbolSpace::Type);
    let value_space = key_for(SemanticSymbolSpace::Value);
    let namespace_space = key_for(SemanticSymbolSpace::Namespace);

    // The keys are NON-equal (the SemanticQueryKey carries the full slot),
    // but they must project to the SAME (FamilyKey, slot).
    assert_ne!(type_space, value_space);
    assert_ne!(type_space, namespace_space);

    // A candidate published under the Type-space key is reachable from the
    // Value-space and Namespace-space keys (count 1). A non-canonicalizing
    // family key (carrying the raw symbol_space) would FORK the slot and
    // make these counts 0 — that is the discriminating negative.
    assert_eq!(
        count_for_b_after_publishing_a(&type_space, &value_space),
        1,
        "ResolveClassSurface keys differing only in decl_slot.symbol_space \
         (Type vs Value) must share ONE (FamilyKey, slot) — symbol_space \
         must be canonicalized out of the family identity"
    );
    assert_eq!(
        count_for_b_after_publishing_a(&type_space, &namespace_space),
        1,
        "ResolveClassSurface keys differing only in decl_slot.symbol_space \
         (Type vs Namespace) must share ONE (FamilyKey, slot)"
    );
}

// ---------------------------------------------------------------------------
// (1c) ResolveClassSurface / ResolveAmbientNamespace strip `context.mode`
//      into the ModeSlot: two keys differing ONLY in `mode` must map to
//      DISTINCT (FamilyKey, ModeSlot). A ModeSlot-collapsing family_and_slot
//      (always returning ModeSlot::Single) would make them SHARE a slot.
// ---------------------------------------------------------------------------

#[test]
fn resolve_class_surface_identity_covers_mode_axis() {
    // Identity vs Navigate — both backfill nothing onto each other's slot
    // (Navigate backfills only Identity on PUBLISH of Navigate; here we
    // publish Identity, which backfills nothing), so a correct impl gives
    // count 0 while a mode-collapsing impl gives count 1.
    let mode_identity = class_surface_key_with_mode(
        "/c.ts",
        "Foo",
        Arc::from([]),
        ClassSurfaceSide::Instance,
        0,
        ProjectionMode::Identity,
    );
    let mode_navigate = class_surface_key_with_mode(
        "/c.ts",
        "Foo",
        Arc::from([]),
        ClassSurfaceSide::Instance,
        0,
        ProjectionMode::Navigate,
    );
    assert_distinct_identity(&mode_identity, &mode_navigate);
}

#[test]
fn resolve_ambient_namespace_identity_covers_mode_axis() {
    let mode_identity = ambient_namespace_key_with_mode("/n.ts", "N", 0, ProjectionMode::Identity);
    let mode_navigate = ambient_namespace_key_with_mode("/n.ts", "N", 0, ProjectionMode::Navigate);
    assert_distinct_identity(&mode_identity, &mode_navigate);
}

// ---------------------------------------------------------------------------
// (2) Per-key identity covers each context's env-hash dimension.
// ---------------------------------------------------------------------------

#[test]
fn resolve_ambient_namespace_key_covers_context() {
    // `AmbientNamespaceContext` carries ONLY `resolve_env_hash` (`R`) +
    // `mode` — there is deliberately NO `parse_env` axis (R21).
    let base = ambient_namespace_key("/n.ts", "N", 0);
    assert_distinct_identity(&base, &ambient_namespace_key("/n.ts", "N", 9));
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
    let env_a = class_surface_key("/c.ts", "Foo", Arc::from([]), ClassSurfaceSide::Instance, 0);
    let env_b = class_surface_key("/c.ts", "Foo", Arc::from([]), ClassSurfaceSide::Instance, 1);
    assert_eq!(
        count_for_b_after_publishing_a(&env_a, &env_b),
        0,
        "ResolveClassSurface must not warm-hit across a resolve_env boundary"
    );
}

#[test]
fn resolve_ambient_namespace_do_not_warm_hit() {
    let env_a = ambient_namespace_key("/n.ts", "N", 0);
    let env_b = ambient_namespace_key("/n.ts", "N", 1);
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
// (3b) Execute-side NON-PRODUCING behavior. The three non-producing keys'
//      `execute` arms must (a) return `QueryError::Miss` — NOT a Value, NOT a
//      fabricated empty/unknown value — AND (b) admit/cache NOTHING. These
//      dispatch through the REAL `execute()` cooperative path (not a direct
//      publish), so a fake producer returning `OverloadSet([])` (→ narrows to
//      `ValueDomainMismatch`, not `Miss`) or `TypeNode(node)` (→ a `Value`,
//      not `Miss`; also leaves a warm candidate) FAILS both assertions.
// ---------------------------------------------------------------------------

/// Dispatch `key` through the canonical `execute()` path and assert it is a
/// non-producing `Miss` that admitted nothing into the shared memo.
fn assert_execute_is_non_producing_miss(host: &VerterHost, key: SemanticQueryKey) {
    let result = verter_session::for_tests::dispatch_execute_type_node_for_tests(host, key.clone());
    // (a) honest Miss — discriminates a `Value` (TypeNode) producer and a
    // `ValueDomainMismatch` (e.g. an `OverloadSet([])` fake) alike.
    assert!(
        matches!(result, QueryResult::Error(QueryError::Miss)),
        "non-producing execute arm must return Error(Miss), got {result:?}"
    );
    // (b) admitted / cached NOTHING — an `Error` result is never warm-published.
    let graph = host.project_type_store().semantic_graph();
    assert_eq!(
        graph.slot_candidate_count_for_tests(&key),
        0,
        "a non-producing execute arm must admit NOTHING into the shared memo"
    );
}

#[test]
fn resolve_ambient_namespace_execute_is_non_producing_miss() {
    let host = host();
    upsert(
        &host,
        "/n.ts",
        "export namespace N {\n  export const a = 1;\n}\n",
    );
    assert_execute_is_non_producing_miss(&host, ambient_namespace_key("/n.ts", "N", 0));
}

#[test]
fn resolve_enum_execute_is_non_producing_miss() {
    let host = host();
    upsert(&host, "/e.ts", "export enum E {\n  A,\n  B,\n}\n");
    assert_execute_is_non_producing_miss(&host, enum_key("/e.ts", "E", 0));
}

#[test]
fn resolve_overload_set_execute_is_non_producing_miss() {
    let host = host();
    upsert(
        &host,
        "/o.ts",
        "export function f(x: number): void;\nexport function f(x: string): void;\nexport function f(x: unknown): void {}\n",
    );
    assert_execute_is_non_producing_miss(&host, overload_set_key(dummy_node(), 0));
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
