//! Consumer-side mapped-key-domain carrier resolution.
//!
//! The openness walker's mapped-key-domain arm decides whether a mapped type
//! `{ [K in keyof S]: V }` (or `{ [K in S]: V }`) enumerates its keys
//! path-precisely (CLOSED key domain) or stays a shallow carrier-stop (OPEN /
//! undecidable key domain). BEFORE this change the carrier arm
//! (`raise.rs` `TypeOf | ImportType | BareRef`) judged an unresolved
//! `BareRef`/`ImportType` SOURCE by inspecting ONLY its carrier type-arguments
//! — it never resolved the carrier head through the shared dispatch to discover
//! whether the underlying declaration is actually a CLOSED key domain. Two
//! consequences:
//!
//! - a no-arg `BareRef(S)` resolved-CLOSED at the carrier arm but the enumerator
//!   (the deferred-shell evaluator + the key-name enumerator) never unwrapped it,
//!   so `synthesise_mapped_surface` produced nothing — slots vanished;
//! - a `BareRef<Foo<T>>` whose `Foo<T>` has a FIXED key set (T confined to
//!   member VALUE positions) judged OPEN (`any open arg`), carrier-stopping a
//!   closed key domain — the headline silent-slot-loss.
//!
//! These fixtures CONSTRUCT a `Mapped { source, mapper }` directly over a
//! direct-built `BareRef` / `KeyOf(BareRef)` carrier source (or key space) — the
//! same construction style as `carrier_head_resolution_tests` — drive it through
//! the empty-path Shallow `ProjectPath` synthesiser (`synthesise_mapped_surface`,
//! `walk.rs`), and read the enumerated key set off the terminal `Object`. An
//! EMPTY surface is a carrier-stop; a non-empty surface is enumerated. Each
//! positive-capability case is compared against the eager-resolved equivalent
//! (the same source as a pre-resolved `InstantiationRef` / `Ref` body) so the
//! two entry points stay path-independent.
//!
//! NON-BREAKING: no producer is flipped. The new consumer path is exercised
//! ONLY via direct-constructed carriers driven through the dispatch.

use std::sync::Arc;

use super::carrier_head_resolution_tests::{
    bare_ref_carrier, file_scope, host, import_type_carrier, upsert_ts,
};
use super::ProjectSemanticDispatch;
use crate::semantic_query::{
    DeclIdentity, HashValue, MapperKey, MapperKind, NodeScopeId, OptionalityMod, PathSegment,
    PrimitiveKind, ProjectionMode, ProjectionReductionContext, QueryResult, ReadonlyMod,
    SemanticNodeData, SemanticNodeId, SemanticQueryApi, SemanticQueryKey, SemanticQueryOutput,
};

/// Intern an unsubstituted outer `TypeParam` (an unbound generic).
fn outer_type_param(dispatch: &ProjectSemanticDispatch<'_>, name: &str) -> SemanticNodeId {
    dispatch.graph().intern_node(SemanticNodeData::TypeParam {
        decl: DeclIdentity::synthetic(name),
        param_index: 0,
        constraint: None,
        default: None,
        display_name: Arc::from(name),
    })
}

fn primitive(dispatch: &ProjectSemanticDispatch<'_>, kind: PrimitiveKind) -> SemanticNodeId {
    dispatch
        .graph()
        .intern_node(SemanticNodeData::Primitive(kind))
}

fn keyof(dispatch: &ProjectSemanticDispatch<'_>, base: SemanticNodeId) -> SemanticNodeId {
    dispatch
        .graph()
        .intern_node(SemanticNodeData::KeyOf { base })
}

/// A `Computed`-kind `MapperKey` over a fresh binder `K`, the given key space,
/// a closed `string` value body, and no name-remap. (The value body is closed
/// so a carrier-stop, if any, is attributable to the KEY domain — the axis
/// under test.)
fn computed_mapper(
    dispatch: &ProjectSemanticDispatch<'_>,
    key_space: SemanticNodeId,
    value_expr: SemanticNodeId,
) -> MapperKey {
    let k_param = outer_type_param(dispatch, "K");
    MapperKey {
        parameter_node: k_param,
        key_space,
        value_expr,
        optionality: OptionalityMod::Keep,
        readonly: ReadonlyMod::Keep,
        name_remap: None,
        kind: MapperKind::Computed,
    }
}

/// Intern `Mapped { source, mapper }`.
fn mapped(
    dispatch: &ProjectSemanticDispatch<'_>,
    source: SemanticNodeId,
    mapper: MapperKey,
) -> SemanticNodeId {
    dispatch
        .graph()
        .intern_node(SemanticNodeData::Mapped { source, mapper })
}

/// Drive a node through the empty-path Shallow `ProjectPath` synthesiser and
/// return the sorted enumerated member NAMES of the terminal `Object` surface.
/// An empty `Vec` means the surface synthesised no members (a carrier-stop or
/// a genuinely-empty surface).
fn shallow_member_names(
    dispatch: &ProjectSemanticDispatch<'_>,
    node: SemanticNodeId,
) -> Vec<String> {
    let empty_path: Arc<[PathSegment]> = Arc::from(Vec::new().into_boxed_slice());
    let terminal = match dispatch.execute_type_node(SemanticQueryKey::ProjectPath {
        base: node,
        path: empty_path,
        context: ProjectionReductionContext::published(ProjectionMode::Shallow),
    }) {
        QueryResult::Value(SemanticQueryOutput { value: id, .. }) => id,
        QueryResult::Recursive(id) => id,
        other => {
            panic!("Shallow ProjectPath over a mapped subject did not yield a node: {other:?}")
        }
    };
    match dispatch.graph().node_data(terminal).as_deref() {
        Some(SemanticNodeData::Object(view)) => {
            let mut names: Vec<String> = view
                .members
                .iter()
                .map(|m| m.name.as_ref().to_string())
                .collect();
            names.sort();
            names
        }
        // A non-Object terminal (a preserved carrier shell, primitive, etc.)
        // contributes NO enumerated members — equivalent to a carrier-stop for
        // the purpose of these assertions.
        _ => Vec::new(),
    }
}

/// A pre-resolved `InstantiationRef(Foo<args>)` body node (the eager equivalent
/// of a `BareRef(Foo, args)` carrier) for path-independence comparison.
fn instantiation_ref(
    dispatch: &ProjectSemanticDispatch<'_>,
    canonical: &str,
    name: &str,
    args: &[SemanticNodeId],
) -> SemanticNodeId {
    dispatch
        .graph()
        .intern_node(SemanticNodeData::InstantiationRef {
            base: DeclIdentity {
                canonical_id: Arc::from(canonical),
                whole_hash: HashValue::default(),
                decl_name: Arc::from(name),
            },
            args: Arc::from(args.to_vec().into_boxed_slice()),
        })
}

/// Intern a transparent `Alias(target)` node. Each `Alias` over a DISTINCT
/// target hash-conses to a distinct `SemanticNodeId`, so a chain of nested
/// aliases is a chain of distinct nodes — the openness walk decrements its node
/// budget once per hop (`node_openness_uncached` → `budget -= 1`, then the
/// `Alias` arm recurses into `target`).
fn alias_node(dispatch: &ProjectSemanticDispatch<'_>, target: SemanticNodeId) -> SemanticNodeId {
    dispatch
        .graph()
        .intern_node(SemanticNodeData::Alias(target))
}

/// Construct an `import("specifier").qualifier<args>` carrier whose type
/// arguments are the pre-interned `arg_nodes` (so a non-primitive arg — an outer
/// `TypeParam` — can be supplied, unlike the sibling `import_type_carrier` which
/// only takes primitive args). Interned UNDER `owner_scope` so
/// `resolve_import_type_head` can recover the owner canonical from the node
/// scope and resolve the relative specifier.
fn import_type_generic_carrier(
    dispatch: &ProjectSemanticDispatch<'_>,
    specifier: &str,
    qualifier: &[&str],
    arg_nodes: &[SemanticNodeId],
    owner_scope: NodeScopeId,
) -> SemanticNodeId {
    let qualifier: Arc<[Arc<str>]> = Arc::from(
        qualifier
            .iter()
            .map(|s| Arc::<str>::from(*s))
            .collect::<Vec<_>>()
            .into_boxed_slice(),
    );
    dispatch.graph().intern_node_with_scope(
        SemanticNodeData::new_import_type(
            Arc::from(specifier),
            qualifier,
            Arc::from(arg_nodes.to_vec().into_boxed_slice()),
            false,
        ),
        owner_scope,
    )
}

// ── 1. CLOSED no-arg source enumerates == eager ─────────────────────────────
//
// `{ [K in keyof S]: string }` over a no-arg `BareRef(S)` source where
// `type S = { a: string; b: number }` enumerates `{a, b}` path-precisely, EQUAL
// to the eager-resolved equivalent over a pre-resolved `Ref`/decl body. Pre-fix
// the carrier source carrier-stops (or never unwraps) and the surface is empty.
#[test]
fn closed_no_arg_bare_ref_source_enumerates_equals_eager() {
    let host = host();
    upsert_ts(
        &host,
        "/s.ts",
        "export type S = { a: string; b: number };\n",
    );
    let dispatch = ProjectSemanticDispatch::new(&host);
    let scope = file_scope(&dispatch, "/s.ts");
    let string_ty = primitive(&dispatch, PrimitiveKind::String);

    // CARRIER form: `{ [K in keyof BareRef(S)]: string }` — the lowering
    // convention puts the OBJECT carrier in `source` and `keyof source` in
    // `key_space` (see `lower.rs` mapped `KeyOf` arm).
    let carrier_source = bare_ref_carrier(&dispatch, "S", scope.clone(), &[]);
    let keyof_carrier = keyof(&dispatch, carrier_source);
    let mapper_carrier = computed_mapper(&dispatch, keyof_carrier, string_ty);
    let carrier_mapped = mapped(&dispatch, carrier_source, mapper_carrier);
    let carrier_names = shallow_member_names(&dispatch, carrier_mapped);

    assert_eq!(
        carrier_names,
        vec!["a".to_string(), "b".to_string()],
        "`{{ [K in keyof BareRef(S)]: string }}` over a CLOSED no-arg source must enumerate \
         {{a, b}} path-precisely — NOT carrier-stop to an empty surface"
    );

    // EAGER equivalent: the same mapped over the pre-resolved decl body. The
    // resolved-source node enumerates identically, proving path-independence.
    let eager_source = dispatch.resolve_carrier_subject_node(
        bare_ref_carrier(&dispatch, "S", scope.clone(), &[]),
        ProjectionReductionContext::structural_transit(),
    );
    assert_ne!(
        eager_source, carrier_source,
        "the eager source must be the RESOLVED decl node (not the bare carrier) — proving the \
         comparison is against a genuinely resolved equivalent"
    );
    let keyof_eager = keyof(&dispatch, eager_source);
    let mapper_eager = computed_mapper(&dispatch, keyof_eager, string_ty);
    let eager_mapped = mapped(&dispatch, eager_source, mapper_eager);
    let eager_names = shallow_member_names(&dispatch, eager_mapped);
    assert_eq!(
        carrier_names, eager_names,
        "carrier-source enumeration must EQUAL the eager-resolved-source enumeration \
         (path-independence)"
    );
    // NEGATIVE: the open type-parameter source DOES carrier-stop (proves the
    // enumeration is not unconditional).
    let t_param = outer_type_param(&dispatch, "T");
    let keyof_open = keyof(&dispatch, t_param);
    let mapper_open = computed_mapper(&dispatch, keyof_open, string_ty);
    let open_mapped = mapped(&dispatch, t_param, mapper_open);
    assert!(
        shallow_member_names(&dispatch, open_mapped).is_empty(),
        "`{{ [K in keyof T]: string }}` over an unbound outer T must carrier-stop (empty surface)"
    );
}

// ── 2. CLOSED fixed-key generic with value-position-only T enumerates ───────
//
// THE HEADLINE silent-slot-loss fix. `{ [K in keyof BareRef(Foo<T>)]: string }`
// where `interface Foo<T> { label?: string; items?: T }` and `T` is an unbound
// `TypeParam` enumerates `{label, items}` — the KEY domain is CLOSED (T confined
// to the member VALUE `items`). Pre-fix the carrier arm returns OPEN
// (`any open arg = T is open`) → carrier-stop → empty surface.
#[test]
fn closed_fixed_key_generic_value_position_only_t_enumerates() {
    let host = host();
    upsert_ts(
        &host,
        "/foo.ts",
        "export interface Foo<T> { label?: string; items?: T }\n",
    );
    let dispatch = ProjectSemanticDispatch::new(&host);
    let scope = file_scope(&dispatch, "/foo.ts");
    let t_param = outer_type_param(&dispatch, "T");
    let string_ty = primitive(&dispatch, PrimitiveKind::String);

    // CARRIER form: `{ [K in keyof BareRef(Foo)<T>]: string }` — the carrier
    // SOURCE carries the open T arg, but Foo's KEY domain is fixed regardless
    // of T.
    let carrier_source = dispatch.graph().intern_node_with_scope(
        SemanticNodeData::new_bare_ref(
            Arc::from("Foo"),
            scope.clone(),
            Arc::from(vec![t_param].into_boxed_slice()),
        ),
        scope.clone(),
    );
    let keyof_carrier = keyof(&dispatch, carrier_source);
    let mapper = computed_mapper(&dispatch, keyof_carrier, string_ty);
    let mapped_node = mapped(&dispatch, carrier_source, mapper);
    let names = shallow_member_names(&dispatch, mapped_node);

    assert_eq!(
        names,
        vec!["items".to_string(), "label".to_string()],
        "`{{ [K in keyof BareRef(Foo)<T>]: string }}` must enumerate {{label, items}} — Foo's \
         KEY domain is CLOSED (T value-position-only). Pre-fix the carrier arm false-OPENs on \
         the open T arg and carrier-stops (silent slot loss)."
    );

    // Path-independence: the pre-resolved `InstantiationRef(Foo<T>)` source
    // (the existing GREEN behaviour) enumerates the same key set.
    let eager_source = instantiation_ref(&dispatch, "/foo.ts", "Foo", &[t_param]);
    let keyof_eager = keyof(&dispatch, eager_source);
    let mapper_eager = computed_mapper(&dispatch, keyof_eager, string_ty);
    let eager_mapped = mapped(&dispatch, eager_source, mapper_eager);
    let eager_names = shallow_member_names(&dispatch, eager_mapped);
    assert_eq!(
        names, eager_names,
        "carrier-source `BareRef(Foo)<T>` must enumerate the SAME keys as the pre-resolved \
         `InstantiationRef(Foo<T>)` source (path-independence)"
    );
    assert!(!names.is_empty(), "must NOT be a carrier-stop");
}

// ── 2b. CLOSED fixed-key generic IMPORT-TYPE source enumerates ──────────────
//
// The `ImportType` half of the headline fix. The production carrier arm handles
// `ImportType` ALONGSIDE `BareRef` (`raise.rs` `node_openness_uncached`
// `SemanticNodeData::ImportType(_) | SemanticNodeData::BareRef(_)`), so the
// `keyof import("/dep").Foo<T>` source — `interface Foo<T> { label?: string;
// items?: T }` with `T` value-position-only — must resolve its IMPORT head
// through the same dispatch and enumerate `{label, items}`. Pre-fix the carrier
// arm false-OPENs on the open `T` arg and carrier-stops (silent slot loss);
// pre-change (before the arm resolved the head at all) the unresolved
// `ImportType` source carrier-stopped unconditionally. Compared against the
// eager pre-resolved `InstantiationRef(Foo<T>)` for path-independence.
#[test]
fn closed_fixed_key_generic_import_type_source_enumerates() {
    let host = host();
    upsert_ts(
        &host,
        "/dep.ts",
        "export interface Foo<T> { label?: string; items?: T }\n",
    );
    // The OWNER file consuming the import — its scope is what the ImportType
    // carrier is interned under so the relative `./dep` specifier resolves.
    upsert_ts(&host, "/owner.ts", "export const x = 1;\n");
    let dispatch = ProjectSemanticDispatch::new(&host);
    let owner_scope = file_scope(&dispatch, "/owner.ts");
    let t_param = outer_type_param(&dispatch, "T");
    let string_ty = primitive(&dispatch, PrimitiveKind::String);

    // CARRIER form: `{ [K in keyof import("./dep").Foo<T>]: string }` — the
    // ImportType source carries the open `T` arg, but Foo's KEY domain is fixed.
    let carrier_source =
        import_type_generic_carrier(&dispatch, "./dep", &["Foo"], &[t_param], owner_scope);
    let keyof_carrier = keyof(&dispatch, carrier_source);
    let mapper = computed_mapper(&dispatch, keyof_carrier, string_ty);
    let mapped_node = mapped(&dispatch, carrier_source, mapper);
    let names = shallow_member_names(&dispatch, mapped_node);

    assert_eq!(
        names,
        vec!["items".to_string(), "label".to_string()],
        "`{{ [K in keyof import(\"./dep\").Foo<T>]: string }}` must enumerate {{label, items}} — \
         the IMPORT-TYPE head resolves through the same dispatch as a BareRef and Foo's KEY \
         domain is CLOSED (T value-position-only)"
    );

    // Path-independence: the pre-resolved `InstantiationRef(Foo<T>)` (the decl is
    // /dep.ts) enumerates the SAME key set as the import-type carrier.
    let eager_source = instantiation_ref(&dispatch, "/dep.ts", "Foo", &[t_param]);
    let keyof_eager = keyof(&dispatch, eager_source);
    let mapper_eager = computed_mapper(&dispatch, keyof_eager, string_ty);
    let eager_mapped = mapped(&dispatch, eager_source, mapper_eager);
    let eager_names = shallow_member_names(&dispatch, eager_mapped);
    assert_eq!(
        names, eager_names,
        "import-type-source `import(\"./dep\").Foo<T>` must enumerate the SAME keys as the \
         pre-resolved `InstantiationRef(Foo<T>)` source (path-independence)"
    );
    assert!(
        !names.is_empty(),
        "import-type source must NOT carrier-stop"
    );
}

// ── 2c. `{ [K in import("/keys").Keys]: V }` IMPORT-TYPE literal keyspace ────
//
// The `ImportType` analogue of test 7: the KEY SPACE is an `import("./keys").
// Keys` carrier (not via `keyof`) where `Keys = 'a' | 'b'`. After the openness
// gate passes, the keyspace enumerator must resolve the IMPORT-TYPE keyspace
// carrier head to its literal union and produce `{a, b}`.
#[test]
fn literal_union_import_type_keyspace_enumerates() {
    let host = host();
    upsert_ts(&host, "/keys.ts", "export type Keys = 'a' | 'b';\n");
    upsert_ts(&host, "/owner.ts", "export const x = 1;\n");
    let dispatch = ProjectSemanticDispatch::new(&host);
    let owner_scope = file_scope(&dispatch, "/owner.ts");
    let string_ty = primitive(&dispatch, PrimitiveKind::String);

    // The key space is the `import("./keys").Keys` carrier directly (NOT keyof).
    let keyspace = import_type_carrier(&dispatch, "./keys", &["Keys"], &[], false, owner_scope);
    let mapper = computed_mapper(&dispatch, keyspace, string_ty);
    let mapped_node = mapped(&dispatch, keyspace, mapper);
    assert_eq!(
        shallow_member_names(&dispatch, mapped_node),
        vec!["a".to_string(), "b".to_string()],
        "`{{ [K in import(\"./keys\").Keys]: string }}` where `Keys = 'a' | 'b'` must enumerate \
         {{a, b}} — the IMPORT-TYPE keyspace carrier resolves to its literal union"
    );
}

// ── 3. defineSlots-shaped mapped over a CLOSED source == eager slots ─────────
//
// The slots path reads exactly this `synthesise_mapped_surface` surface. A
// `defineSlots<{ [K in keyof S]: (props: S[K]) => any }>()`-shaped mapped over a
// closed `BareRef(S)` source must enumerate the same slot NAMES as the eager
// (pre-resolved-source) equivalent — non-empty, identical. Pre-fix the carrier
// source carrier-stops → `slot_fields()` would see `[]` (slots vanish).
#[test]
fn define_slots_shaped_mapped_over_closed_bare_ref_source_equals_eager() {
    let host = host();
    upsert_ts(
        &host,
        "/slots.ts",
        "export type S = { default: string; header: number };\n",
    );
    let dispatch = ProjectSemanticDispatch::new(&host);
    let scope = file_scope(&dispatch, "/slots.ts");
    // Value body: a closed function-ish surface (closed `any` here — the value
    // axis is not under test, the KEY domain is).
    let any_ty = primitive(&dispatch, PrimitiveKind::Any);

    let carrier_source = bare_ref_carrier(&dispatch, "S", scope.clone(), &[]);
    let keyof_carrier = keyof(&dispatch, carrier_source);
    let mapper = computed_mapper(&dispatch, keyof_carrier, any_ty);
    let carrier_mapped = mapped(&dispatch, carrier_source, mapper);
    let carrier_names = shallow_member_names(&dispatch, carrier_mapped);

    assert_eq!(
        carrier_names,
        vec!["default".to_string(), "header".to_string()],
        "a defineSlots-shaped mapped over a CLOSED `BareRef(S)` source must enumerate the slot \
         names {{default, header}} — NOT vanish to an empty slot surface"
    );

    let eager_source = dispatch.resolve_carrier_subject_node(
        bare_ref_carrier(&dispatch, "S", scope.clone(), &[]),
        ProjectionReductionContext::structural_transit(),
    );
    let keyof_eager = keyof(&dispatch, eager_source);
    let mapper_eager = computed_mapper(&dispatch, keyof_eager, any_ty);
    let eager_mapped = mapped(&dispatch, eager_source, mapper_eager);
    assert_eq!(
        carrier_names,
        shallow_member_names(&dispatch, eager_mapped),
        "carrier slot enumeration must EQUAL the eager-resolved slot enumeration (no silent slot \
         loss)"
    );
    assert!(!carrier_names.is_empty(), "slots must NOT vanish");
}

// ── 4. L1 NEGATIVE — genuinely-open source STILL carrier-stops (leak-proof) ──
//
// (a) `{ [K in keyof T]: V }` over an unbound outer generic T (`BareRef`-less,
// a bare TypeParam) carrier-stops; (b) a `Pick<PropsBase<T>, ...>`-style open
// source (here `keyof BareRef(PropsBase)<T>` where PropsBase has an INDEX
// signature whose key type is T) carrier-stops; (c) the LEAK-PROOF case — an
// open source that ALSO carries an enumerable FIXED key
// (`type Open<T> = { fixed: string } & T`): the `& T` arm opens the key domain,
// so the carrier-stop must hold AND the fixed `fixed` key must NOT leak into the
// surface. (a)/(b) have no fixed member, so a force-open bug could still return
// `[]` and pass vacuously; (c) discriminates a bypass because `fixed` would leak.
// The change must NOT force-open any of these.
#[test]
fn genuinely_open_sources_still_carrier_stop() {
    let host = host();
    // PropsBase has an open key domain: an index signature keyed by an open T
    // leaves the produced key set undecidable.
    upsert_ts(
        &host,
        "/pb.ts",
        "export interface PropsBase<T extends string> { [k: T]: number }\n",
    );
    // Open<T> mixes a CLOSED object arm (the leakable fixed `fixed` key) with an
    // open generic arm `& T` — the intersection's open arm opens the key domain.
    upsert_ts(
        &host,
        "/open.ts",
        "export type Open<T> = { fixed: string } & T;\n",
    );
    let dispatch = ProjectSemanticDispatch::new(&host);
    let scope = file_scope(&dispatch, "/pb.ts");
    let open_scope = file_scope(&dispatch, "/open.ts");
    let t_param = outer_type_param(&dispatch, "T");
    let string_ty = primitive(&dispatch, PrimitiveKind::String);

    // (a) bare outer-generic source.
    let keyof_t = keyof(&dispatch, t_param);
    let mapper_a = computed_mapper(&dispatch, keyof_t, string_ty);
    let mapped_a = mapped(&dispatch, t_param, mapper_a);
    assert!(
        shallow_member_names(&dispatch, mapped_a).is_empty(),
        "`{{ [K in keyof T]: string }}` over an unbound outer T must carrier-stop (empty surface)"
    );

    // (b) open-domain carrier source `keyof BareRef(PropsBase)<T>`.
    let carrier_source = dispatch.graph().intern_node_with_scope(
        SemanticNodeData::new_bare_ref(
            Arc::from("PropsBase"),
            scope.clone(),
            Arc::from(vec![t_param].into_boxed_slice()),
        ),
        scope,
    );
    let keyof_carrier = keyof(&dispatch, carrier_source);
    let mapper_b = computed_mapper(&dispatch, keyof_carrier, string_ty);
    let mapped_b = mapped(&dispatch, carrier_source, mapper_b);
    assert!(
        shallow_member_names(&dispatch, mapped_b).is_empty(),
        "`{{ [K in keyof BareRef(PropsBase)<T>]: string }}` over an OPEN key domain (index \
         signature keyed by open T) must carrier-stop — the change must not force-open it"
    );

    // (c) LEAK-PROOF: `keyof BareRef(Open)<T>` over `Open<T> = { fixed: string }
    // & T`. The `& T` arm opens the key domain, so the surface must be EMPTY —
    // and critically the closed-arm `fixed` key must NOT leak. A bypassed /
    // force-opened gate would resolve `Open<T>`'s source and enumerate `fixed`,
    // making this FAIL.
    let open_source = dispatch.graph().intern_node_with_scope(
        SemanticNodeData::new_bare_ref(
            Arc::from("Open"),
            open_scope.clone(),
            Arc::from(vec![t_param].into_boxed_slice()),
        ),
        open_scope,
    );
    let keyof_open = keyof(&dispatch, open_source);
    let mapper_c = computed_mapper(&dispatch, keyof_open, string_ty);
    let mapped_c = mapped(&dispatch, open_source, mapper_c);
    let open_names = shallow_member_names(&dispatch, mapped_c);
    assert!(
        !open_names.contains(&"fixed".to_string()),
        "the closed-arm key `fixed` must NOT leak into the surface of `{{ [K in keyof Open<T>]: \
         string }}` (Open<T> = {{ fixed: string }} & T) — a leaked `fixed` proves the open-domain \
         gate was bypassed; got {open_names:?}"
    );
    assert!(
        open_names.is_empty(),
        "`{{ [K in keyof BareRef(Open)<T>]: string }}` over an OPEN key domain (the `& T` \
         intersection arm) must carrier-stop to an EMPTY surface; got {open_names:?}"
    );
}

// ── 5a. Resolution MISS source carrier-stops ────────────────────────────────
//
// A `BareRef(Missing)` (no such declaration) source must carrier-stop (empty
// surface) — the resolve-then-classify path falls back to the question-correct
// undecidable answer (OPEN for the key domain) on an unresolved head, NOT
// force-enumerate.
#[test]
fn miss_source_carrier_stops() {
    let host = host();
    upsert_ts(&host, "/rec.ts", "export type Rec = { self: Rec };\n");
    let dispatch = ProjectSemanticDispatch::new(&host);
    let scope = file_scope(&dispatch, "/rec.ts");
    let string_ty = primitive(&dispatch, PrimitiveKind::String);

    let miss_source = bare_ref_carrier(&dispatch, "DoesNotExist", scope, &[]);
    let keyof_miss = keyof(&dispatch, miss_source);
    let mapper_miss = computed_mapper(&dispatch, keyof_miss, string_ty);
    let mapped_miss = mapped(&dispatch, miss_source, mapper_miss);
    assert!(
        shallow_member_names(&dispatch, mapped_miss).is_empty(),
        "a `BareRef(DoesNotExist)` (resolution miss) source must carrier-stop (empty surface), \
         not force-enumerate"
    );
}

// ── 5b. Self-referential BODY has a CLOSED key domain — bounded enumeration ──
//
// `type Rec = { self: Rec }` has a self-referential member VALUE, but its KEY
// domain is CLOSED ({self}) — so `{ [K in keyof Rec]: V }` is a CLOSED
// enumeration of the single key `self`, NOT a carrier-stop. This proves the
// resolve-then-classify path terminates bounded on a self-referential body
// (does not hang / miss) and enumerates the finite key. (The prior name
// `miss_and_recursive_source_carrier_stop` mislabelled this as a carrier-stop.)
#[test]
fn recursive_body_closed_key_domain_enumerates_bounded() {
    let host = host();
    upsert_ts(&host, "/rec.ts", "export type Rec = { self: Rec };\n");
    let dispatch = ProjectSemanticDispatch::new(&host);
    let scope = file_scope(&dispatch, "/rec.ts");
    let string_ty = primitive(&dispatch, PrimitiveKind::String);

    let rec_source = bare_ref_carrier(&dispatch, "Rec", scope, &[]);
    let keyof_rec = keyof(&dispatch, rec_source);
    let mapper_rec = computed_mapper(&dispatch, keyof_rec, string_ty);
    let mapped_rec = mapped(&dispatch, rec_source, mapper_rec);
    assert_eq!(
        shallow_member_names(&dispatch, mapped_rec),
        vec!["self".to_string()],
        "`keyof Rec` for `type Rec = {{ self: Rec }}` has a CLOSED key domain {{self}} — \
         resolution must terminate bounded and enumerate the single key, not hang or miss"
    );
}

// ── 5c. TRUE undecidable self-alias source carrier-stops ────────────────────
//
// A DIRECT self-alias `type Loop = Loop` has NO resolvable body — the key domain
// is genuinely undecidable. `{ [K in keyof Loop]: V }` must carrier-stop (empty
// surface): the carrier-head resolve hits the recursive-ref / miss guard and the
// key-domain question answers OPEN. (Contrast 5b, where the body IS a finite
// object with a self-referential member VALUE.) Run on a worker thread so a
// non-terminating resolver (stack overflow) is caught rather than aborting the
// test process.
#[test]
fn self_alias_loop_source_carrier_stops() {
    let handle = std::thread::Builder::new()
        .stack_size(8 * 1024 * 1024)
        .spawn(|| {
            let host = host();
            upsert_ts(&host, "/loop.ts", "export type Loop = Loop;\n");
            let dispatch = ProjectSemanticDispatch::new(&host);
            let scope = file_scope(&dispatch, "/loop.ts");
            let string_ty = primitive(&dispatch, PrimitiveKind::String);

            let loop_source = bare_ref_carrier(&dispatch, "Loop", scope, &[]);
            let keyof_loop = keyof(&dispatch, loop_source);
            let mapper_loop = computed_mapper(&dispatch, keyof_loop, string_ty);
            let mapped_loop = mapped(&dispatch, loop_source, mapper_loop);
            assert!(
                shallow_member_names(&dispatch, mapped_loop).is_empty(),
                "`{{ [K in keyof Loop]: string }}` for the direct self-alias `type Loop = Loop` \
                 (no resolvable body) must carrier-stop to an EMPTY surface — the undecidable key \
                 domain answers OPEN, not force-enumerate"
            );
        })
        .expect("spawn worker thread");
    handle
        .join()
        .expect("the self-alias key-domain resolution must terminate bounded (no stack overflow)");
}

// ── 6a. Undecidable (outer-generic) KeyDomain source answers OPEN ───────────
//
// The undecidable answer-direction of the openness walk: the KEY-DOMAIN
// question answers OPEN for an undecidable source (here a bare unbound outer
// generic `T`). This is the `node_openness_uncached` `TypeParam ∉ bound_params`
// → open path, the same direction the budget-exhaustion arm produces for the
// KeyDomain question.
#[test]
fn key_domain_undecidable_source_answers_open() {
    let host = host();
    upsert_ts(&host, "/d.ts", "export type D = { a: string };\n");
    let dispatch = ProjectSemanticDispatch::new(&host);
    let scope = file_scope(&dispatch, "/d.ts");
    let string_ty = primitive(&dispatch, PrimitiveKind::String);

    // Control: a shallow closed `keyof BareRef(D)` enumerates (budget intact).
    let source = bare_ref_carrier(&dispatch, "D", scope, &[]);
    let keyof_d = keyof(&dispatch, source);
    let mapper = computed_mapper(&dispatch, keyof_d, string_ty);
    let mapped_node = mapped(&dispatch, source, mapper);
    assert_eq!(
        shallow_member_names(&dispatch, mapped_node),
        vec!["a".to_string()],
        "control: a CLOSED `keyof BareRef(D)` with the budget intact must enumerate {{a}}"
    );

    // The KEY-DOMAIN predicate over a genuinely-open outer generic returns OPEN.
    let t_param = outer_type_param(&dispatch, "T");
    let open_mapper = computed_mapper(&dispatch, t_param, string_ty);
    assert!(
        super::raise::mapped_type_key_domain_is_open_or_unknown(&dispatch, t_param, &open_mapper),
        "the KEY-DOMAIN predicate must answer OPEN for an undecidable (here outer-generic) key \
         domain"
    );
}

// ── 6b. REAL budget exhaustion drives the `budget==0` arm → OPEN → carrier-stop
//
// This test ACTUALLY exhausts the openness walk's node budget
// (`ENUMERATION_DOMAIN_OPENNESS_NODE_BUDGET = 256`, `raise.rs:3543`) — it does
// not merely assert the undecidable-TypeParam path. Construction: fold a chain
// of transparent `Alias` nodes around a CLOSED `Object` source. Each `Alias`
// hop is a DISTINCT interned `SemanticNodeId` (hash-consing keys on
// `(payload, scope)`, and each alias wraps a distinct target), so the per-node
// memo `(node, position)` never collides; every hop calls
// `node_openness_uncached`, which decrements `self.budget` by 1
// (`raise.rs:4221`) and then recurses into the alias target
// (`SemanticNodeData::Alias(target) => self.node_is_open(ctx, *target)`,
// `raise.rs:4514`).
//
// PROOF the `budget==0` arm is hit: the walk starts at the top alias with
// budget 256 and decrements once per hop. After 256 hops the budget is 0; the
// 257th `node_openness_uncached` call returns at the `if self.budget == 0` arm
// (`raise.rs:4211`) BEFORE reaching the closed `Object` at the bottom. With
// ALIAS_DEPTH = 320 > 256 the walk provably reaches `budget==0` while still
// inside the alias chain, and the `KeyDomain` question's
// `undecidable_is_open()` returns OPEN there.
//
// DISCRIMINATION: a SHALLOW alias chain (depth 4, well under budget) over the
// SAME closed `Object` classifies CLOSED and enumerates `{a}` — so the deep
// chain's OPEN verdict is attributable to BUDGET EXHAUSTION, not to an open
// bottom (the bottom is the identical closed object). If the `budget==0` arm
// answered CLOSED, the deep-chain assertions (a)+(b) below would BOTH FAIL.
#[test]
fn key_domain_budget_exhaustion_carrier_stops() {
    // Depth strictly greater than the 256 node budget so the walk provably
    // reaches `budget==0` before the closed bottom.
    const ALIAS_DEPTH: usize = 320;

    let handle = std::thread::Builder::new()
        .stack_size(16 * 1024 * 1024)
        .spawn(|| {
            let host = host();
            upsert_ts(&host, "/d.ts", "export type D = { a: string };\n");
            let dispatch = ProjectSemanticDispatch::new(&host);
            let scope = file_scope(&dispatch, "/d.ts");
            let string_ty = primitive(&dispatch, PrimitiveKind::String);

            // The closed bottom: `D = { a: string }` resolved to its object body
            // (so the ONLY thing that can open the key domain is budget
            // exhaustion partway down the alias chain — not the bottom).
            let closed_bottom = dispatch.resolve_carrier_subject_node(
                bare_ref_carrier(&dispatch, "D", scope.clone(), &[]),
                ProjectionReductionContext::structural_transit(),
            );

            // CONTROL: a SHALLOW alias chain (depth 4 ≪ 256) over the closed
            // bottom enumerates `{a}` — the walk reaches the closed object well
            // within budget and classifies CLOSED.
            let mut shallow_chain = closed_bottom;
            // bounded-loop: test fixture — fold a fixed depth-4 alias chain.
            for _ in 0..4 {
                shallow_chain = alias_node(&dispatch, shallow_chain);
            }
            let shallow_keyof = keyof(&dispatch, shallow_chain);
            let shallow_mapper = computed_mapper(&dispatch, shallow_keyof, string_ty);
            let shallow_mapped = mapped(&dispatch, shallow_chain, shallow_mapper);
            assert_eq!(
                shallow_member_names(&dispatch, shallow_mapped),
                vec!["a".to_string()],
                "control: a SHALLOW alias chain (depth 4) over a CLOSED object must classify \
                 CLOSED and enumerate {{a}} — the budget is intact, so the bottom is reached"
            );

            // DEEP chain: 320 distinct alias hops > 256 budget. The key-domain
            // walk hits `budget==0` partway down and the KeyDomain question
            // answers OPEN.
            let mut deep_chain = closed_bottom;
            // bounded-loop: test fixture — fold a fixed ALIAS_DEPTH alias chain.
            for _ in 0..ALIAS_DEPTH {
                deep_chain = alias_node(&dispatch, deep_chain);
            }
            let deep_keyof = keyof(&dispatch, deep_chain);
            let deep_mapper = computed_mapper(&dispatch, deep_keyof, string_ty);

            // (a) The key-domain predicate answers OPEN — driven by the
            // `budget==0` arm (the bottom is the SAME closed object the shallow
            // control enumerated, so only budget exhaustion explains the flip).
            assert!(
                super::raise::mapped_type_key_domain_is_open_or_unknown(
                    &dispatch,
                    deep_chain,
                    &deep_mapper,
                ),
                "a {ALIAS_DEPTH}-deep alias chain (> the 256 node budget) over a CLOSED bottom \
                 must classify the KEY domain OPEN via the `budget==0` arm — the shallow control \
                 over the identical bottom is CLOSED, so the verdict is attributable to budget \
                 exhaustion, not an open bottom"
            );

            // (b) The synthesised mapped surface is EMPTY — carrier-stop, NOT
            // force-enumerated / warm-admitted.
            let deep_mapped = mapped(&dispatch, deep_chain, deep_mapper);
            assert!(
                shallow_member_names(&dispatch, deep_mapped).is_empty(),
                "the budget-exhausted mapped source must carrier-stop to an EMPTY surface — not \
                 force-enumerate the (unreached) closed bottom"
            );
        })
        .expect("spawn worker thread");
    handle
        .join()
        .expect("the deep-alias key-domain walk must terminate bounded (no stack overflow)");
}

// ── 7. `{ [K in S]: V }` literal-union keyspace enumerates ───────────────────
//
// `S = 'a' | 'b'` exposed as a `BareRef(S)` KEY SPACE (not via `keyof`). After
// the openness gate passes, the enumerator must resolve the keyspace carrier to
// its literal union and produce keys `{a, b}`.
#[test]
fn literal_union_bare_ref_keyspace_enumerates() {
    let host = host();
    upsert_ts(&host, "/keys.ts", "export type Keys = 'a' | 'b';\n");
    let dispatch = ProjectSemanticDispatch::new(&host);
    let scope = file_scope(&dispatch, "/keys.ts");
    let string_ty = primitive(&dispatch, PrimitiveKind::String);

    // The key space is the `BareRef(Keys)` carrier directly (NOT keyof). The
    // mapped SOURCE is the same carrier (the source is unused for a direct
    // literal keyspace, but must not itself open the domain).
    let keyspace = bare_ref_carrier(&dispatch, "Keys", scope, &[]);
    let mapper = computed_mapper(&dispatch, keyspace, string_ty);
    let mapped_node = mapped(&dispatch, keyspace, mapper);
    assert_eq!(
        shallow_member_names(&dispatch, mapped_node),
        vec!["a".to_string(), "b".to_string()],
        "`{{ [K in BareRef(Keys)]: string }}` where `Keys = 'a' | 'b'` must enumerate {{a, b}} — \
         the keyspace carrier resolves to its literal union"
    );
}

/// A single-member closed `Object` surface `{ <name>: <value> }`.
fn object_one_member(
    dispatch: &ProjectSemanticDispatch<'_>,
    name: &str,
    value: SemanticNodeId,
) -> SemanticNodeId {
    use crate::semantic_query::{IndexSignature, SurfaceMember, SurfaceView};
    let member = SurfaceMember {
        visibility: verter_type_expr::MemberVisibility::Public,
        name: Arc::from(name),
        value,
        optional: false,
        readonly: false,
        is_method: false,
        declared_in_macro_type_arg: false,
        merge_role: crate::semantic_query::MemberMergeRole::Authored,
        spans: Default::default(),
        declaration_origin: None,
    };
    dispatch
        .graph()
        .intern_node(SemanticNodeData::Object(SurfaceView {
            members: Arc::from(vec![member].into_boxed_slice()),
            call_signatures: Arc::from(Vec::<SemanticNodeId>::new().into_boxed_slice()),
            construct_signatures: Arc::from(Vec::<SemanticNodeId>::new().into_boxed_slice()),
            index_signatures: Arc::from(Vec::<IndexSignature>::new().into_boxed_slice()),
            keyspace: None,
            has_index_signature: false,
        }))
}

/// An `Intersection` over the given arms.
fn intersection(dispatch: &ProjectSemanticDispatch<'_>, arms: &[SemanticNodeId]) -> SemanticNodeId {
    dispatch
        .graph()
        .intern_node(SemanticNodeData::Intersection(Arc::from(
            arms.to_vec().into_boxed_slice(),
        )))
}

/// A nullary `Function` `() => <return_type>` (no params, no type parameters).
fn nullary_function(
    dispatch: &ProjectSemanticDispatch<'_>,
    return_type: SemanticNodeId,
) -> SemanticNodeId {
    dispatch.graph().intern_node(SemanticNodeData::Function {
        params: Arc::from(Vec::new().into_boxed_slice()),
        return_type,
        type_parameters: Arc::from(Vec::new().into_boxed_slice()),
        signature_span: None,
        return_type_span: None,
    })
}

/// A `__builtin__` utility instantiation `Utility<args…>` (e.g.
/// `ReturnType<Fn>`), the exact node a builtin-utility reference lowers to.
fn builtin_instantiation(
    dispatch: &ProjectSemanticDispatch<'_>,
    utility: &str,
    args: &[SemanticNodeId],
) -> SemanticNodeId {
    instantiation_ref(dispatch, "__builtin__", utility, args)
}

// ── 7c. Value-producing builtin SOURCE over an open generic carrier-stops ────
//
// `{ [K in keyof ReturnType<() => ({ fixed: string } & T)>]: V }` over an
// UNBOUND outer `T`. `ReturnType` is a VALUE-producing builtin — it makes NO
// closed-key claim (`BuiltinUtility::ReturnType.key_domain_argument_positions()`
// is `None`), and the `& T` intersection arm in the function's return makes the
// produced key domain genuinely OPEN. The mapped SOURCE / key-space role MUST
// therefore prove finiteness through the builtin registry and CARRIER-STOP —
// the closed-arm key `fixed` must NOT leak.
//
// DISCRIMINATION (the false-closed defect this guards): under the key-domain
// walk the function ARGUMENT to `ReturnType` is a closed leaf (value surfaces
// are not descended at a KeyDomain position), so `any_open = false`. Before the
// role split the shared mapped-key-domain policy set
// `concrete_no_open_arg_is_closed = true`, so the `InstantiationRef` arm
// returned CLOSED on `!any_open` BEFORE consulting the builtin registry — a
// false-closed key domain that lets the enumerator leak `fixed`. The
// source/key-space role must NOT take that shortcut; it must fall through to the
// registry, where `ReturnType` makes no closed-key claim ⇒ OPEN ⇒ carrier-stop.
// Both the directly-constructed node route AND the prepared-decl TypeExpr route
// (a userland `type Wrapped<T> = ReturnType<…>` reached via a `BareRef`) are
// asserted, since the role split must hold on BOTH closedness routes.
#[test]
fn value_producing_builtin_source_over_open_generic_carrier_stops_no_leak() {
    let host = host();
    // Userland wrapper for the prepared-decl / TypeExpr route.
    upsert_ts(
        &host,
        "/wrap.ts",
        "export type Wrapped<T> = ReturnType<() => { fixed: string } & T>;\n",
    );
    let dispatch = ProjectSemanticDispatch::new(&host);
    let wrap_scope = file_scope(&dispatch, "/wrap.ts");
    let t_param = outer_type_param(&dispatch, "T");
    let string_ty = primitive(&dispatch, PrimitiveKind::String);

    // (a) NODE ROUTE — the source is the directly-constructed
    // `ReturnType<() => ({ fixed: string } & T)>` `InstantiationRef` (the exact
    // node the builtin reference lowers to). This hits the `InstantiationRef`
    // openness arm with `any_open = false` (the open T is buried in the
    // non-descended function return), so it exercises the false-closed shortcut
    // directly.
    let fixed_obj = object_one_member(&dispatch, "fixed", string_ty);
    let ret_intersection = intersection(&dispatch, &[fixed_obj, t_param]);
    let ret_fn = nullary_function(&dispatch, ret_intersection);
    let return_type_source = builtin_instantiation(&dispatch, "ReturnType", &[ret_fn]);
    let keyof_node_source = keyof(&dispatch, return_type_source);
    let mapper_a = computed_mapper(&dispatch, keyof_node_source, string_ty);
    let mapped_a = mapped(&dispatch, return_type_source, mapper_a);
    let node_names = shallow_member_names(&dispatch, mapped_a);
    assert!(
        !node_names.contains(&"fixed".to_string()),
        "the closed-arm key `fixed` must NOT leak from `{{ [K in keyof ReturnType<() => ({{ \
         fixed: string }} & T)>]: V }}` (node route) — a leaked `fixed` proves the \
         source/key-space role took the concrete-no-open-arg shortcut and skipped the builtin \
         registry; got {node_names:?}"
    );
    assert!(
        node_names.is_empty(),
        "a `ReturnType<…>` SOURCE makes no closed-key claim and `& T` opens the key domain — the \
         surface must carrier-stop to EMPTY (node route); got {node_names:?}"
    );

    // (b) PREPARED-DECL / TYPEEXPR ROUTE — the source is a `BareRef(Wrapped)<T>`
    // whose declaration body is `ReturnType<() => { fixed: string } & T>`. The
    // carrier head resolves through the shared dispatch; the prepared-decl
    // closedness route must ALSO keep the `ReturnType<…>` source OPEN.
    let bare_source = dispatch.graph().intern_node_with_scope(
        SemanticNodeData::new_bare_ref(
            Arc::from("Wrapped"),
            wrap_scope.clone(),
            Arc::from(vec![t_param].into_boxed_slice()),
        ),
        wrap_scope,
    );
    let keyof_bare = keyof(&dispatch, bare_source);
    let mapper_b = computed_mapper(&dispatch, keyof_bare, string_ty);
    let mapped_b = mapped(&dispatch, bare_source, mapper_b);
    let decl_names = shallow_member_names(&dispatch, mapped_b);
    assert!(
        !decl_names.contains(&"fixed".to_string()),
        "the closed-arm key `fixed` must NOT leak from a `BareRef(Wrapped)<T>` source whose body \
         is `ReturnType<() => {{ fixed: string }} & T>` (prepared-decl route); got {decl_names:?}"
    );
    assert!(
        decl_names.is_empty(),
        "`{{ [K in keyof BareRef(Wrapped)<T>]: V }}` over `Wrapped<T> = ReturnType<…>` must \
         carrier-stop (prepared-decl route); got {decl_names:?}"
    );
}

// ── 7d. Provably-closed builtin SOURCE still enumerates (role-split must not
//        over-fire) ────────────────────────────────────────────────────────
//
// The companion positive guard to 7c: the source/key-space role proving
// finiteness must NOT start carrier-stopping a builtin that DOES make a
// closed-key claim. `{ [K in keyof Pick<S, 'a'>]: string }` over
// `type S = { a: string; b: number }` must still enumerate `{a}` — `Pick`'s
// output key domain is its selection argument `'a'`, provably closed.
#[test]
fn closed_builtin_source_still_enumerates_under_role_split() {
    let host = host();
    let dispatch = ProjectSemanticDispatch::new(&host);
    let string_ty = primitive(&dispatch, PrimitiveKind::String);
    let number_ty = primitive(&dispatch, PrimitiveKind::Number);

    // `Pick<{ a: string; b: number }, 'a'>` as a directly-constructed
    // `__builtin__` instantiation over a CLOSED `Object` source. `Pick`'s output
    // key domain is its selection argument `'a'` (provably closed), so the
    // source/key-space role must prove finiteness CLOSED and enumerate `{a}`.
    let source_obj = {
        use crate::semantic_query::{IndexSignature, SurfaceMember, SurfaceView};
        let member = |name: &str, value: SemanticNodeId| SurfaceMember {
            visibility: verter_type_expr::MemberVisibility::Public,
            name: Arc::from(name),
            value,
            optional: false,
            readonly: false,
            is_method: false,
            declared_in_macro_type_arg: false,
            merge_role: crate::semantic_query::MemberMergeRole::Authored,
            spans: Default::default(),
            declaration_origin: None,
        };
        dispatch
            .graph()
            .intern_node(SemanticNodeData::Object(SurfaceView {
                members: Arc::from(
                    vec![member("a", string_ty), member("b", number_ty)].into_boxed_slice(),
                ),
                call_signatures: Arc::from(Vec::<SemanticNodeId>::new().into_boxed_slice()),
                construct_signatures: Arc::from(Vec::<SemanticNodeId>::new().into_boxed_slice()),
                index_signatures: Arc::from(Vec::<IndexSignature>::new().into_boxed_slice()),
                keyspace: None,
                has_index_signature: false,
            }))
    };
    let lit_a = dispatch.graph().intern_node(SemanticNodeData::Literal(
        crate::semantic_query::LiteralValue::String("a".to_string()),
    ));
    let pick_source = builtin_instantiation(&dispatch, "Pick", &[source_obj, lit_a]);
    let keyof_pick = keyof(&dispatch, pick_source);
    let mapper = computed_mapper(&dispatch, keyof_pick, string_ty);
    let mapped_node = mapped(&dispatch, pick_source, mapper);
    assert_eq!(
        shallow_member_names(&dispatch, mapped_node),
        vec!["a".to_string()],
        "`{{ [K in keyof Pick<{{a: string; b: number}}, 'a'>]: string }}` must still enumerate \
         {{a}} — Pick makes a closed-key claim (its selection arg), so the role split must NOT \
         carrier-stop it"
    );
}

// ── 8. Behavior-preservation: the policy-model refactor is identity for the
//        existing OuterGenericReachability / KeyDomain policies ──────────────
//
// The `mapped_type_is_open_or_unknown` (value-body axis) verdicts on the canon
// closed/open shapes are unchanged by the `outer_generic_only` → `OpenQuestion`
// re-encoding. (The full per-dimension matrix lives in `tests.rs`; this is a
// compact in-module guard that the re-encoding did not flip a verdict.)
#[test]
fn policy_model_refactor_is_behavior_preserving() {
    let host = host();
    upsert_ts(&host, "/bp.ts", "export type S = { a: string };\n");
    let dispatch = ProjectSemanticDispatch::new(&host);
    let scope = file_scope(&dispatch, "/bp.ts");
    let string_ty = primitive(&dispatch, PrimitiveKind::String);
    let t_param = outer_type_param(&dispatch, "T");

    // CLOSED source + CLOSED value: NOT open.
    let closed_source = bare_ref_carrier(&dispatch, "S", scope.clone(), &[]);
    let keyof_closed = keyof(&dispatch, closed_source);
    let closed_mapper = computed_mapper(&dispatch, keyof_closed, string_ty);
    assert!(
        !super::raise::mapped_type_is_open_or_unknown(&dispatch, keyof_closed, &closed_mapper),
        "closed source + closed value body must stay CLOSED under the re-encoded policy"
    );

    // CLOSED source + OPEN value body (reaches outer T): OPEN (value-body axis).
    let open_value_mapper = computed_mapper(&dispatch, keyof_closed, t_param);
    assert!(
        super::raise::mapped_type_is_open_or_unknown(&dispatch, keyof_closed, &open_value_mapper),
        "closed source + value reaching outer T must stay OPEN under the re-encoded policy"
    );

    // OPEN source (bare outer T): OPEN (key-domain axis).
    let open_source_mapper = computed_mapper(&dispatch, t_param, string_ty);
    assert!(
        super::raise::mapped_type_is_open_or_unknown(&dispatch, t_param, &open_source_mapper),
        "open key-domain source must stay OPEN under the re-encoded policy"
    );
}

// ── 9. Carrier-source → conditional recursion TERMINATES bounded (the
//        cycle-guard-threaded closedness route) ─────────────────────────────
//
// The mapped-key-domain SOURCE carrier resolution recurses through the
// prepared-decl closedness route: `BareRef(Rec)<X>` source resolves its head
// to `Rec`'s declaration, whose body is a CONDITIONAL whose `extends` clause
// references `Rec<T>` recursively. The conditional closedness verdict consults
// the SHARED branch-selection oracle (`type_expr_conditional_branch_selection`
// → `ProjectSemanticDispatch::conditional_branch_selection`).
//
// That oracle now runs THROUGH the ACTIVE dispatcher (the cycle-guard
// threading: `&ProjectSemanticDispatch` is passed through
// `prepared_decl_body_is_closed` / `prepared_instantiation_key_domain_is_closed`
// / `key_domain_type_expr_is_closed` / `type_expr_conditional_branch_selection`,
// replacing the former `ProjectSemanticDispatch::new(ctx)` fresh dispatcher), so
// the dispatcher-local `instantiate_active` / `carrier_normalizing` cycle-guard
// state is shared rather than reset on the closedness route. This is a
// soundness-hardening guarantee: the route no longer relies on the
// prepared-decl in-flight `visited` set + the carrier `carrier_normalizing`
// guard being the SOLE bounds (they bound today's reachable shapes, which is
// why this verdict is stable across the change — see the report), but on the
// active dispatcher's guards being correctly threaded so a future shape that
// reaches the oracle's `Instantiate` recursion cannot escape the back-edge.
//
// This test is the TERMINATION + VERDICT regression guard for that route: it
// runs on a worker thread with a bounded stack so any divergence manifests as a
// failed `join` (stack overflow) rather than a whole-suite hang, and asserts
// the question-correct verdict (the recursive conditional is undecidable for
// the key domain ⇒ carrier-stop / empty surface). A regression that breaks
// termination OR flips the verdict fails it.
#[test]
fn bare_ref_decl_conditional_recursion_terminates() {
    let handle = std::thread::Builder::new()
        .stack_size(8 * 1024 * 1024)
        .spawn(|| {
            let host = host();
            // `Rec<T>`'s body is a conditional whose `extends` references
            // `Rec<T>` recursively — the carrier-source closedness route walks
            // it through the shared branch-selection oracle.
            upsert_ts(
                &host,
                "/rec_cond.ts",
                "export type Rec<T> = T extends Rec<T> ? { a: string } : { b: string };\n",
            );
            let dispatch = ProjectSemanticDispatch::new(&host);
            let scope = file_scope(&dispatch, "/rec_cond.ts");
            let string_ty = primitive(&dispatch, PrimitiveKind::String);
            let t_param = outer_type_param(&dispatch, "T");

            // `{ [K in keyof BareRef(Rec)<T>]: string }` — the source carrier
            // resolves `Rec`'s head and recurses into the conditional body.
            let rec_source = dispatch.graph().intern_node_with_scope(
                SemanticNodeData::new_bare_ref(
                    Arc::from("Rec"),
                    scope.clone(),
                    Arc::from(vec![t_param].into_boxed_slice()),
                ),
                scope,
            );
            let keyof_rec = keyof(&dispatch, rec_source);
            let mapper = computed_mapper(&dispatch, keyof_rec, string_ty);
            let mapped_node = mapped(&dispatch, rec_source, mapper);
            // The recursive conditional key domain is undecidable ⇒ carrier-stop.
            let names = shallow_member_names(&dispatch, mapped_node);
            assert!(
                names.is_empty(),
                "`{{ [K in keyof BareRef(Rec)<T>]: string }}` over the self-referential \
                 conditional `Rec<T> = T extends Rec<T> ? … : …` must carrier-stop to an EMPTY \
                 surface (undecidable key domain); got {names:?}"
            );
        })
        .expect("spawn worker thread");
    handle.join().expect(
        "the carrier-source → conditional recursion must TERMINATE bounded (no stack overflow): \
         the closedness route must consult the branch-selection oracle through the ACTIVE \
         dispatcher so the cycle-guard state is shared",
    );
}
