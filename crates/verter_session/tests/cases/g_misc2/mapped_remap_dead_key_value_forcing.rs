//! Dead-key forcing discriminators for mapped types with an `as`-clause
//! key remap.
//!
//! Durable invariant under test: the `as <expr>` remap is a KEY-DOMAIN
//! operator. It reads the mapper's `name_remap` and never its
//! `value_expr`, so it is decidable WITHOUT forcing any key's value.
//! Consequently a mapped-type evaluation must decide each iteration
//! key's produced name FIRST, and force a value operand only for keys
//! that actually reach the produced surface. Two failures this
//! discriminates:
//!
//! 1. **Forcing before the remap decision.** A key the remap DROPS
//!    (`as K extends 'wanted' ? K : never`) is dead; substituting its
//!    binder into the value body, walking the deferred evaluator,
//!    dispatching `Instantiate`, and then throwing the result away is
//!    pure dead-operand work. `mapped_per_k_materializations` counts
//!    every per-K value materialisation, so a wide mapped type with one
//!    surviving key must advance it by exactly one.
//!
//! 2. **Answering a single-key demand by materialising the whole
//!    surface.** `Remapped['kept_b']` asks for ONE produced member. A
//!    remapping mapper cannot use iteration-key membership to admit that
//!    name, but it can invert the remap over the key domain — a
//!    names-only enumeration — and force only the producing key. Falling
//!    back to whole-surface `MappedType` resolution forces every
//!    surviving key's value to answer a one-key question.
//!
//! 3. **Enumerating an OPEN key domain to answer a single-key demand.**
//!    A mapper over an unbound source has no enumerable key set, so a
//!    demanded produced name has no decidable preimage. The demand must
//!    preserve the deferred `Mapped` carrier and force nothing.
//!
//! 4. **Widening the cache-validity rail with a dead key's
//!    dependencies.** A remap-dropped key's value declaration is not a
//!    semantic dependency of the mapped answer, so its facts must not
//!    enter the traced read set — while the demanded key's value facts
//!    and both keys' import-route facts must.
//!
//! 5. **Paying for a demand the key domain already answered.** A
//!    demanded name a CLOSED key domain provably does not produce is
//!    absent, and the key-absent sentinel is the answer whichever route
//!    reaches it. Resolving the whole mapped surface first — forcing
//!    every surviving key's value operand, so the cost scales with the
//!    source's width — only arrives at the same miss. Proven on both the
//!    remapping rail (preimage) and the plain rail (iteration-key
//!    admission).
//!
//! 6. **Continuing to force key values after cancellation.** Cancelled
//!    work is return-only, so both publication rails must stop within
//!    one key of the cancellation rather than running the key domain out
//!    and assembling a surface from degraded per-key results.
//!
//! Every leg also asserts the ANSWER, not just the counter or the read
//! set: narrowing must return the same semantic surface/value that
//! whole-surface evaluation produces, and a remap-dropped key must be
//! absent from the published surface. A counter-only assertion would
//! pass for an implementation that skipped the work by skipping the
//! semantics.

#![allow(clippy::too_many_lines)]

use std::sync::Arc;

use verter_semantic::facts::FactKey;
use verter_session::resolver_core::{FactReadSetFinalise, FactVersionRef};
use verter_session::semantic_query::{
    IndexKey, PathSegment, ProjectionMode, ProjectionReductionContext, PropertyKey, QueryResult,
    SemanticNodeData, SemanticNodeId, SemanticQueryKey, SemanticQueryOutput,
};
use verter_session::{for_tests, HostConfig, UpsertRequest, VerterHost};
use verter_type_expr::TypeExpr;

/// A WIDE source (six keys) behind a `Computed` mapper whose value body
/// is a generic helper instantiation — so every key that reaches the
/// value position costs a real per-K materialisation — and an `as`
/// remap that DROPS all but two keys.
///
/// `Kept` renames its two survivors, so the produced surface names
/// (`kept_b`, `kept_e`) are NOT iteration keys: iteration-key admission
/// cannot answer a demand for them, only the remap preimage can.
const WIDE_REMAP_TS: &str = r#"
export interface WideSource {
  a: string;
  b: number;
  c: boolean;
  d: string[];
  e: number[];
  f: Record<string, string>;
}

export type Boxed<V> = { boxed: V };

export type Kept<K> = K extends 'b' ? 'kept_b' : K extends 'e' ? 'kept_e' : never;

export type Remapped = {
  [K in keyof WideSource as Kept<K>]: Boxed<WideSource[K]>
};
"#;

/// The same six-key shape with the remap written INLINE rather than
/// through a userland helper. Deciding an inline conditional remap needs
/// no nested query, so a cancelled request does not fail the first key
/// closed through the deferred-carrier arm — the key loop is the only
/// thing that can stop it, which is what makes this fixture able to
/// observe the loop's own cancellation checks.
const INLINE_REMAP_TS: &str = r#"
export interface WideSource {
  a: string;
  b: number;
  c: boolean;
  d: string[];
  e: number[];
  f: Record<string, string>;
}

export type Boxed<V> = { boxed: V };

export type Remapped = {
  [K in keyof WideSource as K extends 'b' ? 'kept_b' : K extends 'e' ? 'kept_e' : never]:
    Boxed<WideSource[K]>
};
"#;

fn upsert(host: &Arc<VerterHost>, source: &str) {
    let _ = host.upsert(UpsertRequest {
        canonical_id: Some("/source.ts".to_string()),
        input_id: "/source.ts".to_string(),
        source: Arc::from(source),
        file_language: verter_session::LanguageRegistry::global()
            .classify_static("/source.ts")
            .static_resolution(),
        aliases: Vec::new(),
    });
}

/// Lower `alias_name` to its carrier node under the given mode.
fn carrier(host: &Arc<VerterHost>, alias_name: &str, mode: ProjectionMode) -> SemanticNodeId {
    let expr = TypeExpr::Ref {
        name: Arc::from(alias_name),
        type_arguments: Arc::from(Vec::new().into_boxed_slice()),
    };
    for_tests::dispatch_lower_type_expr_in_scope_with_context_for_tests(
        host,
        "/source.ts",
        &expr,
        ProjectionReductionContext::published(mode),
    )
    .unwrap_or_else(|| panic!("lowering `{alias_name}` must succeed"))
}

fn project(
    host: &Arc<VerterHost>,
    base: SemanticNodeId,
    path: Vec<PathSegment>,
    mode: ProjectionMode,
) -> SemanticNodeId {
    let query = SemanticQueryKey::ProjectPath {
        base,
        path: Arc::from(path.into_boxed_slice()),
        context: ProjectionReductionContext::published(mode),
    };
    match for_tests::dispatch_execute_type_node_for_tests(host, query) {
        QueryResult::Value(SemanticQueryOutput { value: node, .. }) => node,
        other => panic!("ProjectPath must yield a value node, got {other:?}"),
    }
}

fn member_names(host: &Arc<VerterHost>, surface: SemanticNodeId) -> Vec<String> {
    let graph = host.project_type_store().semantic_graph();
    let data = graph
        .node_data(surface)
        .expect("surface node must have semantic data");
    match data.as_ref() {
        SemanticNodeData::Object(view) => view
            .positive_members()
            .iter()
            .filter_map(|m| m.string_name().map(str::to_string))
            .collect(),
        other => panic!("expected an Object surface, got {other:?}"),
    }
}

fn per_k_materializations(host: &Arc<VerterHost>) -> u64 {
    host.project_type_store()
        .semantic_graph()
        .stats_snapshot()
        .mapped_per_k_materializations
}

fn instantiate_count(host: &Arc<VerterHost>) -> u64 {
    host.project_type_store()
        .semantic_graph()
        .stats_snapshot()
        .instantiate_count
}

fn substitute_misses(host: &Arc<VerterHost>) -> u64 {
    host.project_type_store()
        .semantic_graph()
        .stats_snapshot()
        .substitute_memo_misses
}

/// The value node bound to `member` on an evaluated object surface.
fn member_value(host: &Arc<VerterHost>, surface: SemanticNodeId, member: &str) -> SemanticNodeId {
    let graph = host.project_type_store().semantic_graph();
    let data = graph
        .node_data(surface)
        .expect("surface node must have semantic data");
    match data.as_ref() {
        SemanticNodeData::Object(view) => {
            view.positive_members()
                .iter()
                .find(|m| m.string_name() == Some(member))
                .unwrap_or_else(|| panic!("member `{member}` must be published; got {view:?}"))
                .value
        }
        other => panic!("expected an Object surface, got {other:?}"),
    }
}

fn describe(host: &Arc<VerterHost>, node: SemanticNodeId) -> String {
    format!(
        "{:?}",
        host.project_type_store().semantic_graph().node_data(node)
    )
}

/// DISCRIMINATOR (dead-key forcing): a whole-surface evaluation of a
/// six-key mapped type whose `as` remap drops four keys must materialise
/// exactly the two SURVIVING keys' values.
///
/// Deciding the remap AFTER forcing the value — the ordering this test
/// forbids — substitutes and evaluates all six value operands and then
/// discards four, so the counter advances by six. Deciding the remap
/// FIRST advances it by two.
///
/// The surface assertion is the correctness rail: dropping the work must
/// not drop the semantics. Exactly the two renamed members must be
/// published, under their POST-remap names.
#[test]
fn remap_dropped_keys_do_not_force_their_value_operands() {
    let host = Arc::new(VerterHost::new_standalone(HostConfig::default()));
    upsert(&host, WIDE_REMAP_TS);

    let base = carrier(&host, "Remapped", ProjectionMode::Expanded);

    let before = per_k_materializations(&host);
    let surface = project(&host, base, Vec::new(), ProjectionMode::Expanded);
    let forced = per_k_materializations(&host) - before;

    let mut names = member_names(&host, surface);
    names.sort();
    assert_eq!(
        names,
        vec!["kept_b".to_string(), "kept_e".to_string()],
        "the remap keeps exactly `b` and `e`, renamed; four keys must be dropped from the \
         published surface"
    );

    assert_eq!(
        forced, 2,
        "a six-key mapped type whose `as` remap drops four keys must force exactly the two \
         SURVIVING keys' value operands; observed {forced} per-K materialisations. A count of \
         6 means the value operand was forced before the remap decision, so every dropped \
         key's value was substituted, evaluated and then discarded — dead-operand work."
    );
}

/// DISCRIMINATOR (single-key demand through a remapping mapper): a
/// `ProjectPath` for ONE produced name must force only the iteration key
/// that produces it, AND must return the same value the whole surface
/// publishes under that name.
///
/// The demanded name `kept_b` is a POST-remap name, so iteration-key
/// admission cannot decide it. Inverting the remap over the key domain
/// (a names-only enumeration) identifies `b` as the sole producer and
/// forces only its value: one per-K materialisation. Falling back to
/// whole-surface `MappedType` resolution forces BOTH survivors, so the
/// counter reaches two.
///
/// The ANSWER leg is what stops the counter leg from being satisfied by
/// forcing the WRONG single key. `Boxed<WideSource['b']>` and
/// `Boxed<WideSource['e']>` both publish exactly one member named
/// `boxed` and both cost exactly one materialisation — a remap preimage
/// that selected iteration key `e` for the demanded name `kept_b` would
/// satisfy every shape-only assertion while returning `{ boxed: number[] }`
/// instead of `{ boxed: number }`. So the narrowed node is compared
/// against the whole-surface member value node computed on the SAME host
/// (node ids are content-addressed within one arena, and cross-host ids
/// are incomparable), and the `boxed` member's own value is pinned to
/// `number`.
#[test]
fn single_key_demand_through_remapping_mapper_forces_only_the_producing_key() {
    let host = Arc::new(VerterHost::new_standalone(HostConfig::default()));
    upsert(&host, WIDE_REMAP_TS);
    let base = carrier(&host, "Remapped", ProjectionMode::Expanded);

    // Measured window: the single-key demand is cold here — the
    // whole-surface reference is computed AFTER it, on this same host, so
    // no warm per-K result masks the counter and the two nodes remain
    // comparable.
    let before = per_k_materializations(&host);
    let narrowed = project(
        &host,
        base,
        vec![PathSegment::Member(PropertyKey::string_literal("kept_b"))],
        ProjectionMode::Expanded,
    );
    let forced = per_k_materializations(&host) - before;

    let narrowed_data = host
        .project_type_store()
        .semantic_graph()
        .node_data(narrowed)
        .expect("narrowed value must exist");
    assert!(
        !matches!(
            narrowed_data.as_ref(),
            SemanticNodeData::Opaque(_) | SemanticNodeData::Mapped { .. }
        ),
        "single-key narrowing through a remapping mapper must RESOLVE `kept_b`. An opaque miss \
         means the demand stalled; a `Mapped` carrier means the remap failed closed — the \
         userland helper remap `Kept<K>` lowers to an `InstantiationRef`, and leaving that \
         carrier un-instantiated makes a helper-authored remap undecidable where the identical \
         inline conditional decides. Got {:?}",
        narrowed_data.as_ref()
    );
    drop(narrowed_data);

    assert_eq!(
        forced, 1,
        "a single-key demand for one PRODUCED name must force exactly the ONE iteration key \
         that produces it; observed {forced} per-K materialisations. A count of 2 means the \
         demand fell through to whole-surface mapped resolution and forced the unrelated \
         survivor `e` as well; a count of 6 means it forced the dropped keys too."
    );

    // ANSWER leg — the same semantic answer as whole-surface-then-project.
    let whole = project(
        &host,
        carrier(&host, "Remapped", ProjectionMode::Expanded),
        Vec::new(),
        ProjectionMode::Expanded,
    );
    let expected = {
        let graph = host.project_type_store().semantic_graph();
        let data = graph.node_data(whole).expect("surface must exist");
        match data.as_ref() {
            SemanticNodeData::Object(view) => {
                view.positive_members()
                    .iter()
                    .find(|m| m.string_name() == Some("kept_b"))
                    .expect("`kept_b` must be published by the whole surface")
                    .value
            }
            other => panic!("expected an Object surface, got {other:?}"),
        }
    };
    assert_eq!(
        narrowed,
        expected,
        "single-key narrowing must return the SAME node the whole surface publishes under \
         `kept_b`. Interning is content-addressed within one arena, so a differing node is a \
         differing answer — the failure this catches is a remap preimage that selected the \
         wrong producing iteration key: `Boxed<WideSource['e']>` has the same member NAME and \
         the same one-materialisation cost as `Boxed<WideSource['b']>`. narrowed={} expected={}",
        describe(&host, narrowed),
        describe(&host, expected)
    );

    // Independent of node identity: the `boxed` member's own value must
    // address SOURCE KEY `b`, never `e`. The published value is the
    // addressable `WideSource['b']` access, so the producing iteration key
    // is readable directly off it — exactly the fact a wrong-producer
    // regression changes while leaving every member NAME and every counter
    // identical.
    let boxed_value = member_value(&host, narrowed, "boxed");
    let indexed_key = match host
        .project_type_store()
        .semantic_graph()
        .node_data(boxed_value)
        .as_deref()
    {
        Some(SemanticNodeData::IndexedAccess {
            index: IndexKey::String(name),
            ..
        }) => name.to_string(),
        other => panic!(
            "`Remapped['kept_b']`'s `boxed` member is `WideSource['b']`, an addressable \
             indexed access; got {other:?}"
        ),
    };
    assert_eq!(
        indexed_key, "b",
        "the demanded produced name `kept_b` is produced by iteration key `b`, so the value \
         must address `WideSource['b']`. `e` here means the remap preimage selected the wrong \
         producing key — a regression invisible to member names (`boxed` either way) and to \
         the per-K counter (one materialisation either way)."
    );
}

/// `WIDE_REMAP_TS` with ONE dropped key's value type edited (`c: boolean`
/// → `c: symbol`). The edited key is remap-DROPPED, so it contributes
/// nothing to the published surface and its value is never a semantic
/// operand of the mapped result.
const WIDE_REMAP_DEAD_KEY_EDITED_TS: &str = r#"
export interface WideSource {
  a: string;
  b: number;
  c: symbol;
  d: string[];
  e: number[];
  f: Record<string, string>;
}

export type Boxed<V> = { boxed: V };

export type Kept<K> = K extends 'b' ? 'kept_b' : K extends 'e' ? 'kept_e' : never;

export type Remapped = {
  [K in keyof WideSource as Kept<K>]: Boxed<WideSource[K]>
};
"#;

/// DISCRIMINATOR (incremental parity after a dead-key edit): editing the
/// value type of a key the remap DROPS must not change the published
/// surface, and recomputation must still perform zero dead-key semantic
/// work.
///
/// The edit lands on the mapped type's own owner file, so strict
/// self-root validation may conservatively reject the warm entry and
/// recompute — that is allowed. What is NOT allowed is the recomputation
/// paying for the dead key: the post-edit run must force the same two
/// surviving keys and no more, and must publish a surface identical to
/// the one a FRESH host computes from the edited source.
///
/// This fails if dead-key work leaks back into recomputation (the
/// counter exceeds two) or if a dropped key's value edit perturbs the
/// published surface (incremental/fresh divergence).
#[test]
fn dead_key_edit_preserves_surface_and_forces_no_dead_key_work_on_recompute() {
    let host = Arc::new(VerterHost::new_standalone(HostConfig::default()));
    upsert(&host, WIDE_REMAP_TS);

    let before_surface = member_names(
        &host,
        project(
            &host,
            carrier(&host, "Remapped", ProjectionMode::Expanded),
            Vec::new(),
            ProjectionMode::Expanded,
        ),
    );

    // Edit the DEAD key `c`'s value type in the same owner file.
    upsert(&host, WIDE_REMAP_DEAD_KEY_EDITED_TS);

    let before = per_k_materializations(&host);
    let after_surface = member_names(
        &host,
        project(
            &host,
            carrier(&host, "Remapped", ProjectionMode::Expanded),
            Vec::new(),
            ProjectionMode::Expanded,
        ),
    );
    let forced = per_k_materializations(&host) - before;

    let mut before_sorted = before_surface;
    before_sorted.sort();
    let mut after_sorted = after_surface;
    after_sorted.sort();
    assert_eq!(
        after_sorted, before_sorted,
        "editing a remap-DROPPED key's value type must not change the published mapped \
         surface: the dropped key contributes no member and its value is not an operand of \
         the result"
    );

    // FRESH parity: a host that never saw the pre-edit source must
    // publish the identical surface.
    let fresh = Arc::new(VerterHost::new_standalone(HostConfig::default()));
    upsert(&fresh, WIDE_REMAP_DEAD_KEY_EDITED_TS);
    let mut fresh_sorted = member_names(
        &fresh,
        project(
            &fresh,
            carrier(&fresh, "Remapped", ProjectionMode::Expanded),
            Vec::new(),
            ProjectionMode::Expanded,
        ),
    );
    fresh_sorted.sort();
    assert_eq!(
        after_sorted, fresh_sorted,
        "incremental and fresh evaluation of the edited source must publish the same surface"
    );

    assert!(
        forced <= 2,
        "recomputation after a dead-key edit must still force at most the two SURVIVING keys' \
         value operands; observed {forced}. More than two means dead-key work leaked back into \
         the recompute path."
    );
}

/// `WIDE_REMAP_TS`'s mapped type re-authored over an UNBOUND source: the
/// iteration domain is `keyof T` for a type parameter with no argument,
/// so the key domain is OPEN.
///
/// Lowered with zero type arguments the alias yields the deferred
/// `Mapped` carrier itself (not a `DeclRef`), which is what puts a
/// single-key demand on the walker's mapped arm with a remapping mapper
/// — the exact entrance the closed-domain narrowing uses.
const OPEN_REMAP_TS: &str = r#"
export type Boxed<V> = { boxed: V };

export type Kept<K> = K extends 'b' ? 'kept_b' : K extends 'e' ? 'kept_e' : never;

export type OpenRemapped<T> = {
  [K in keyof T as Kept<K>]: Boxed<T[K]>
};
"#;

/// DISCRIMINATOR (open key domain at a single-key demand): a
/// `ProjectPath` for one produced name through a remapping mapper whose
/// key domain is OPEN must preserve the deferred `Mapped` carrier and
/// force NOTHING.
///
/// An open domain has no enumerable key set, so no preimage of the
/// demanded produced name exists. The demand must therefore carrier-stop
/// (L1), not guess. Two regressions this catches:
///
/// 1. Enumerating an open domain to answer the narrower demand — any
///    key admitted from a non-finite domain is fabricated, and forcing
///    its value is work for a member that may not exist. The per-K
///    counter must not move at all.
/// 2. Collapsing the un-narrowable demand to a resolved node (an
///    `Opaque` miss or a synthesised member) instead of returning the
///    carrier. The carrier is ADDRESSABLE: a later consumer that binds
///    `T` re-dispatches it. A miss is terminal and wrong.
#[test]
fn open_key_domain_single_key_demand_preserves_the_mapped_carrier() {
    let host = Arc::new(VerterHost::new_standalone(HostConfig::default()));
    upsert(&host, OPEN_REMAP_TS);

    let base = carrier(&host, "OpenRemapped", ProjectionMode::Expanded);
    let graph = host.project_type_store().semantic_graph();
    assert!(
        matches!(
            graph.node_data(base).as_deref(),
            Some(SemanticNodeData::Mapped { .. })
        ),
        "fixture invariant: an unbound-source mapped alias must lower to the deferred `Mapped` \
         carrier, otherwise the demand never reaches the mapped arm and the test is vacuous. \
         Got {:?}",
        graph.node_data(base).as_deref()
    );

    let before = per_k_materializations(&host);
    let projected = project(
        &host,
        base,
        vec![PathSegment::Member(PropertyKey::string_literal("kept_b"))],
        ProjectionMode::Expanded,
    );
    let forced = per_k_materializations(&host) - before;

    assert!(
        matches!(
            host.project_type_store()
                .semantic_graph()
                .node_data(projected)
                .as_deref(),
            Some(SemanticNodeData::Mapped { .. })
        ),
        "a single-key demand through a remapping mapper over an OPEN key domain must preserve \
         the deferred `Mapped` carrier — the domain has no enumerable key set, so the produced \
         name has no decidable preimage and the demand carrier-stops. Got {:?}",
        host.project_type_store()
            .semantic_graph()
            .node_data(projected)
            .as_deref()
    );
    assert_eq!(
        forced, 0,
        "an OPEN key domain must never be enumerated to answer a narrower demand: observed \
         {forced} per-K value materialisations. Any non-zero count means keys were admitted \
         from a non-finite domain and their values forced."
    );
}

const KEPT_VALUE_TS: &str = "export type KeptValue = { inner: number };\n";
const DEAD_VALUE_TS: &str = "export type DeadValue = { inner: string };\n";

/// Owner file whose mapped VALUE projects into the member type
/// (`WideSource[K]['inner']`), so forcing a key's value necessarily
/// enters that key's value declaration — and therefore that file — while
/// leaving a key's value unforced necessarily does not.
const CROSS_FILE_REMAP_OWNER_TS: &str = r#"
import type { KeptValue } from './kept';
import type { DeadValue } from './dead';

export interface WideSource { b: KeptValue; c: DeadValue; }

export type Kept<K> = K extends 'b' ? 'kept_b' : never;

export type Remapped = { [K in keyof WideSource as Kept<K>]: WideSource[K]['inner'] };
"#;

fn upsert_at(host: &Arc<VerterHost>, path: &str, source: &str) {
    let _ = host.upsert(UpsertRequest {
        canonical_id: Some(path.to_string()),
        input_id: path.to_string(),
        source: Arc::from(source),
        file_language: verter_session::LanguageRegistry::global()
            .classify_static(path)
            .static_resolution(),
        aliases: Vec::new(),
    });
}

/// Every fact in `signature` attributed to `canonical`.
fn facts_for(signature: &[FactVersionRef], canonical: &str) -> Vec<FactVersionRef> {
    signature
        .iter()
        .filter(|fact| fact.canonical_id() == Some(canonical))
        .cloned()
        .collect()
}

/// `true` for the facts EVERY file reached through an import route
/// contributes regardless of whether its declarations were ever read:
/// the route target's whole-hash and its syntactic route interface.
/// These are the *import* facts a demanded resolution legitimately
/// traces; anything beyond them means the resolver entered the file to
/// read a declaration.
fn is_import_route_floor_fact(fact: &FactVersionRef) -> bool {
    match fact {
        FactVersionRef::FileWholeHash { .. } => true,
        FactVersionRef::Parse(parse) => {
            matches!(parse.key, FactKey::SyntacticRouteInterface)
        }
        _ => false,
    }
}

/// DISCRIMINATOR (fact-signature membership): the read set of a mapped
/// evaluation contains the demanded key's value-declaration facts and
/// does NOT contain the remap-dropped key's.
///
/// `Remapped` maps `WideSource[K]['inner']`, so producing a key's value
/// requires resolving that key's declared type through its import and
/// reading its `inner` member — an observation attributed to the file
/// that declares it. `b`'s value lives in `/w/kept.ts` and survives the
/// remap; `c`'s lives in `/w/dead.ts` and the remap drops it.
///
/// Both files are reached by the owner's IMPORT ROUTES, so both
/// legitimately contribute the route floor (whole-hash + syntactic route
/// interface) — that leg pins the positive half: demanded import facts
/// DO enter the signature. The discriminating half is what lies beyond
/// the floor: `/w/kept.ts` must contribute more (the resolver entered it
/// to read `KeptValue`), `/w/dead.ts` must contribute nothing more.
///
/// Forcing the value operand before the remap decision resolves
/// `DeadValue['inner']` too, and the dead file's declaration facts land
/// in the read set — widening the cache-validity rail with a dependency
/// the answer does not have.
#[test]
fn demanded_key_value_facts_are_traced_and_dead_key_value_facts_are_not() {
    let host = Arc::new(VerterHost::new_standalone(HostConfig::default()));
    upsert_at(&host, "/w/kept.ts", KEPT_VALUE_TS);
    upsert_at(&host, "/w/dead.ts", DEAD_VALUE_TS);
    upsert_at(&host, "/w/owner.ts", CROSS_FILE_REMAP_OWNER_TS);

    let (surface, finalised) = for_tests::install_fact_tracer_for_tests(&host, || {
        let expr = TypeExpr::Ref {
            name: Arc::from("Remapped"),
            type_arguments: Arc::from(Vec::new().into_boxed_slice()),
        };
        let base = for_tests::dispatch_lower_type_expr_in_scope_with_context_for_tests(
            &host,
            "/w/owner.ts",
            &expr,
            ProjectionReductionContext::published(ProjectionMode::Expanded),
        )
        .expect("lowering `Remapped` must succeed");
        project(&host, base, Vec::new(), ProjectionMode::Expanded)
    });

    // Correctness rail: the surface is the renamed survivor only.
    assert_eq!(
        member_names(&host, surface),
        vec!["kept_b".to_string()],
        "the remap keeps only `b`, renamed to `kept_b`"
    );

    let FactReadSetFinalise::Ok(signature) = finalised else {
        panic!("the traced read set must finalise cacheable; got {finalised:?}");
    };

    // A domain aggregate stands in for an unbounded set of precise
    // facts, so "the dead file contributes nothing beyond the floor"
    // would be an under-approximation rather than a claim. Assert the
    // signature is precise before reading absence from it.
    assert!(
        signature
            .iter()
            .all(|fact| !matches!(fact, FactVersionRef::DomainGeneration(_))),
        "fixture invariant: the read set must stay precise (no collapsed domain aggregate), \
         otherwise an absent per-file fact proves nothing. Got {signature:?}"
    );

    let kept_facts = facts_for(&signature, "/w/kept.ts");
    let dead_facts = facts_for(&signature, "/w/dead.ts");

    // Positive half — both files are reached by an import route, so both
    // contribute the route floor. This is the "demanded import facts
    // enter the signature" leg; it also proves the dead file WAS
    // reachable, so its absence beyond the floor is a decision and not
    // an accident of the fixture.
    assert!(
        dead_facts
            .iter()
            .any(|f| matches!(f, FactVersionRef::FileWholeHash { .. })),
        "fixture invariant: the dropped key's value file must still be import-resolved (its \
         route facts belong in the signature), otherwise the absence assertion below is \
         vacuous. Got {dead_facts:?}"
    );

    // Discriminating half.
    let kept_beyond_floor: Vec<_> = kept_facts
        .iter()
        .filter(|fact| !is_import_route_floor_fact(fact))
        .collect();
    let dead_beyond_floor: Vec<_> = dead_facts
        .iter()
        .filter(|fact| !is_import_route_floor_fact(fact))
        .collect();

    assert!(
        !kept_beyond_floor.is_empty(),
        "the DEMANDED key's value declaration was read, so its file must contribute facts \
         beyond the import-route floor — those are the body facts the cached answer actually \
         depends on. Got only {kept_facts:?}"
    );
    assert!(
        dead_beyond_floor.is_empty(),
        "a remap-DROPPED key's value declaration is not a semantic dependency of the mapped \
         answer, so its file must contribute nothing beyond the import-route floor. Observed \
         {dead_beyond_floor:?}. Forcing the value operand before the remap decision resolves \
         `DeadValue['inner']` and lands its declaration facts in the read set, widening the \
         cache-validity rail with a dependency the answer does not have."
    );
}

/// DISCRIMINATOR (Shallow publication rail): the SAME dead-key ordering
/// must hold on the empty-path Shallow surface synthesiser, not only on
/// the Expanded publication rail.
///
/// The two rails build the mapped surface independently
/// (build_mapped_type for Published(Expanded), the walker's
/// synthesise_mapped_surface for Published(Shallow)), so an ordering fix
/// on one of them is unguarded by every counter assertion that only ever
/// drives Expanded. Published answers are INVARIANT under the reorder —
/// the same two members are published either way — so only a work
/// counter discriminates it.
#[test]
fn shallow_surface_remap_dropped_keys_do_not_force_their_value_operands() {
    let host = Arc::new(VerterHost::new_standalone(HostConfig::default()));
    upsert(&host, WIDE_REMAP_TS);

    let base = carrier(&host, "Remapped", ProjectionMode::Shallow);

    let before = per_k_materializations(&host);
    let surface = project(&host, base, Vec::new(), ProjectionMode::Shallow);
    let forced = per_k_materializations(&host) - before;

    let mut names = member_names(&host, surface);
    names.sort();
    assert_eq!(
        names,
        vec!["kept_b".to_string(), "kept_e".to_string()],
        "the Shallow rail must publish the same two renamed members the Expanded rail does"
    );
    assert_eq!(
        forced, 2,
        "the Shallow surface synthesiser must decide each key's remap BEFORE forcing its \
         value operand, exactly as the Expanded rail does: six keys, four remap-dropped, so \
         exactly two per-K materialisations. Observed {forced}. A count of 6 means the Shallow \
         rail forces every key's value and discards the dropped ones — the dead-operand work \
         the ordering forbids, unguarded because every other counter test drives Expanded."
    );
}

/// A mapped type whose remap drops EVERY key, over a value expression
/// that costs a real instantiation. The produced surface is empty, so
/// the value operand is dead for the whole mapped type — not merely per
/// key.
const ALL_DROPPED_TS: &str = r#"
export type Boxed<V> = { boxed: V };

export type AllDropped = { [K in 'a' | 'b' as never]: Boxed<number> };

export type NoneDropped = { [K in 'a' | 'b']: Boxed<number> };
"#;

/// The same two mapped types over an INERT value expression. Pairing
/// them with `ALL_DROPPED_TS` isolates the cost of the value operand
/// from the fixed cost of resolving the alias and enumerating its key
/// space, which is identical across the two fixtures.
const ALL_DROPPED_INERT_VALUE_TS: &str = r#"
export type Boxed<V> = { boxed: V };

export type AllDropped = { [K in 'a' | 'b' as never]: number };

export type NoneDropped = { [K in 'a' | 'b']: number };
"#;

/// DISCRIMINATOR (K-independent shared value): when every key is
/// remap-dropped, the mapped type's shared value operand must not be
/// materialised at all.
///
/// Boxed<number> does not reference the mapper binder, so both rails
/// evaluate it ONCE and reuse it for every key instead of substituting
/// per K. Evaluating it above the key loop makes that single evaluation
/// unconditional — it runs even when the key-domain decision drops every
/// key and publishes an empty surface. The per-K materialisation counter
/// cannot see this: the shared evaluation is precisely the path that
/// bypasses the per-K materialiser.
///
/// The NoneDropped control proves the fixture's value operand really
/// does cost an instantiation, so the zero-delta assertion is about the
/// ordering and not about an inert value expression.
///
/// Driven at the Shallow publication boundary, where the mapper's value
/// expression is still an unresolved carrier when the mapped surface is
/// built, so forcing it is observable. The Expanded publication rail
/// holds the same ordering in the same shape, but a K-INDEPENDENT value
/// operand is by definition substitution-free, so the enclosing
/// declaration-body projection has already realised it before the mapped
/// build runs — there, the shared evaluation is a memo hit either way and
/// no counter can separate the two orderings.
#[test]
fn a_mapped_type_with_every_key_dropped_does_not_force_its_shared_value() {
    // (instantiations, published member names) for `alias` in `source`.
    // The carrier lowering sits OUTSIDE the measured window so the window
    // contains the mapped BUILD, whose key-domain decision is the subject.
    fn measure(source: &str, alias: &str, mode: ProjectionMode) -> (u64, Vec<String>) {
        let host = Arc::new(VerterHost::new_standalone(HostConfig::default()));
        upsert(&host, source);
        let base = carrier(&host, alias, mode);
        let before = instantiate_count(&host);
        let surface = project(&host, base, Vec::new(), mode);
        let cost = instantiate_count(&host) - before;
        let mut names = member_names(&host, surface);
        names.sort();
        (cost, names)
    }

    {
        // Driven at the Shallow publication boundary only — see the scope
        // note on this test.
        let mode = ProjectionMode::Shallow;
        // Control: the SAME mapped shapes over an inert value expression.
        // Subtracting it isolates the cost of the value operand from the
        // fixed cost of resolving the alias and enumerating its key space.
        let (inert_kept, inert_kept_names) =
            measure(ALL_DROPPED_INERT_VALUE_TS, "NoneDropped", mode);
        let (inert_dropped, inert_dropped_names) =
            measure(ALL_DROPPED_INERT_VALUE_TS, "AllDropped", mode);
        let (boxed_kept, boxed_kept_names) = measure(ALL_DROPPED_TS, "NoneDropped", mode);
        let (boxed_dropped, boxed_dropped_names) = measure(ALL_DROPPED_TS, "AllDropped", mode);

        assert_eq!(
            inert_kept_names,
            vec!["a".to_string(), "b".to_string()],
            "fixture invariant ({mode:?}): the surviving-key control keeps both keys"
        );
        assert_eq!(
            boxed_kept_names, inert_kept_names,
            "fixture invariant ({mode:?}): the value expression does not change the key set"
        );
        assert!(
            inert_dropped_names.is_empty() && boxed_dropped_names.is_empty(),
            "a mapped type whose remap drops every key publishes an EMPTY surface ({mode:?}); \
             got inert={inert_dropped_names:?} boxed={boxed_dropped_names:?}"
        );
        assert!(
            boxed_kept > inert_kept,
            "fixture invariant ({mode:?}): the boxed value operand must cost strictly more \
             than the inert one when a key SURVIVES, otherwise the dead-key comparison below \
             is vacuous. Observed inert={inert_kept} boxed={boxed_kept}."
        );

        assert_eq!(
            boxed_dropped, inert_dropped,
            "every key is remap-dropped, so the mapped type's shared value operand is dead for \
             the whole type and must never be evaluated ({mode:?}). Replacing the inert value \
             with one that costs an instantiation must therefore cost nothing: observed \
             inert={inert_dropped} boxed={boxed_dropped}. A difference means the shared \
             K-independent value is evaluated unconditionally, above the key-domain decision \
             that drops every key."
        );
    }
}

/// DISCRIMINATOR (proven-absent produced name): demanding a produced
/// name that a CLOSED key domain provably does not produce must answer
/// the key-absent miss without forcing any surviving key's value.
///
/// Once the remap preimage is empty over a closed domain, absence is
/// PROVEN — nothing further needs deciding. Declining to the
/// whole-surface route instead reaches the same miss only after
/// materialising every surviving key's value, which is the key-scaled
/// work a single-key demand must not do.
///
/// The answer leg pins that the cheaper route did not change the
/// semantics: the outcome must still be the miss sentinel, never a
/// fabricated member and never a published surface.
#[test]
fn a_proven_absent_produced_name_misses_without_forcing_any_value() {
    let host = Arc::new(VerterHost::new_standalone(HostConfig::default()));
    upsert(&host, WIDE_REMAP_TS);
    let base = carrier(&host, "Remapped", ProjectionMode::Expanded);

    let before = per_k_materializations(&host);
    let projected = project(
        &host,
        base,
        vec![PathSegment::Member(PropertyKey::string_literal(
            "never_produced",
        ))],
        ProjectionMode::Expanded,
    );
    let forced = per_k_materializations(&host) - before;

    assert!(
        matches!(
            host.project_type_store()
                .semantic_graph()
                .node_data(projected)
                .as_deref(),
            Some(SemanticNodeData::Opaque(_))
        ),
        "the closed key domain produces only kept_b and kept_e, so never_produced must answer \
         the key-absent sentinel — never a fabricated member value and never a published \
         surface. Got {}",
        describe(&host, projected)
    );
    assert_eq!(
        forced, 0,
        "absence proven from the key domain alone demands NO value operand; observed {forced} \
         per-K materialisations. A count of 2 means the demand declined to whole-surface \
         mapped resolution and forced both surviving keys' values to reach a miss the \
         preimage had already proven."
    );
}

/// A wide source behind an IDENTITY remap. The remap keeps every key
/// under its own name, so the produced key set IS the iteration key set
/// — but name_remap is set, so the demand still enters the remapping
/// preimage path rather than plain iteration-key admission.
const IDENTITY_REMAP_NARROW_TS: &str = r#"
export interface Source { k0: string; k1: string; }

export type Boxed<V> = { boxed: V };

export type Remapped = { [K in keyof Source as K]: Boxed<Source[K]> };
"#;

const IDENTITY_REMAP_WIDE_TS: &str = r#"
export interface Source {
  k0: string; k1: string; k2: string; k3: string; k4: string; k5: string;
  k6: string; k7: string; k8: string; k9: string; k10: string; k11: string;
  k12: string; k13: string; k14: string; k15: string;
}

export type Boxed<V> = { boxed: V };

export type Remapped = { [K in keyof Source as K]: Boxed<Source[K]> };
"#;

/// DISCRIMINATOR (single-key work is independent of source WIDTH): a
/// single-key demand through an identity remap must cost the same
/// whether the source has 2 keys or 16.
///
/// An identity remap is the one remap whose inverse is known
/// structurally — it maps every key to itself — so the preimage of a
/// demanded produced name is decidable by MEMBERSHIP over the enumerated
/// domain. Substituting the binder into name_remap and evaluating it
/// once per domain key instead makes a one-key demand linear in the
/// source's width: the unrelated keys are decided, and paying to decide
/// them is work the demand never asked for.
///
/// Both substitutions and per-K value materialisations are pinned,
/// because a fix that moved the cost from one counter to the other would
/// not be a fix. The answer leg keeps the comparison honest across the
/// two different sources.
#[test]
fn single_key_demand_through_an_identity_remap_is_independent_of_source_width() {
    fn measure(source: &str) -> (u64, u64, Vec<String>) {
        let host = Arc::new(VerterHost::new_standalone(HostConfig::default()));
        upsert(&host, source);
        let base = carrier(&host, "Remapped", ProjectionMode::Expanded);
        let subs_before = substitute_misses(&host);
        let per_k_before = per_k_materializations(&host);
        let narrowed = project(
            &host,
            base,
            vec![PathSegment::Member(PropertyKey::string_literal("k1"))],
            ProjectionMode::Expanded,
        );
        let subs = substitute_misses(&host) - subs_before;
        let per_k = per_k_materializations(&host) - per_k_before;
        (subs, per_k, member_names(&host, narrowed))
    }

    let (narrow_subs, narrow_per_k, narrow_names) = measure(IDENTITY_REMAP_NARROW_TS);
    let (wide_subs, wide_per_k, wide_names) = measure(IDENTITY_REMAP_WIDE_TS);

    assert_eq!(
        narrow_names,
        vec!["boxed".to_string()],
        "fixture invariant: the narrowed value is a one-member box"
    );
    assert_eq!(
        wide_names, narrow_names,
        "widening the source cannot change the answer to a single-key demand"
    );
    assert_eq!(
        wide_per_k, narrow_per_k,
        "a single-key demand materialises only the producing key's value regardless of source \
         width; observed {narrow_per_k} (2 keys) vs {wide_per_k} (16 keys)."
    );
    assert_eq!(
        wide_subs, narrow_subs,
        "an identity remap is invertible structurally, so resolving ONE demanded produced \
         name must not substitute the binder once per source key: observed {narrow_subs} \
         substitutions over a 2-key source vs {wide_subs} over a 16-key source. A difference \
         means the preimage evaluates the remap across the whole domain, making a one-key \
         demand linear in source width."
    );
}

/// Audit observer that cancels the ACTIVE request the first time a
/// top-level type-parameter substitution is issued, and counts every
/// substitution the run performs.
///
/// The substitution counter is the mapped key loop's per-key forcing
/// signal: every iteration key's remap decision substitutes the binder
/// into the remap expression exactly once before anything else about
/// that key is decided.
const CANCEL_AT_SUBSTITUTION: u64 = 3;

struct CancelOnFirstSubstitution {
    seen: std::sync::atomic::AtomicU64,
    ctx: Arc<verter_session::request_context::RequestContext>,
}

impl verter_audit::AuditObserver for CancelOnFirstSubstitution {
    fn record_event(&self, event: verter_audit::AuditEvent) {
        if matches!(event, verter_audit::AuditEvent::SubstituteTopLevelCall) {
            let seen = self.seen.fetch_add(1, std::sync::atomic::Ordering::SeqCst) + 1;
            if seen == CANCEL_AT_SUBSTITUTION {
                self.ctx.cancel();
            }
        }
    }
}

/// Counts substitutions without cancelling — the control that says how
/// much work an UNINTERRUPTED build performs, so the cancelled run's
/// count is compared against a measured quantity rather than a guess.
struct CountSubstitutions {
    seen: std::sync::atomic::AtomicU64,
}

impl verter_audit::AuditObserver for CountSubstitutions {
    fn record_event(&self, event: verter_audit::AuditEvent) {
        if matches!(event, verter_audit::AuditEvent::SubstituteTopLevelCall) {
            self.seen.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        }
    }
}

fn project_result(
    host: &Arc<VerterHost>,
    base: SemanticNodeId,
    mode: ProjectionMode,
) -> QueryResult<SemanticQueryOutput<SemanticNodeId>> {
    for_tests::dispatch_execute_type_node_for_tests(
        host,
        SemanticQueryKey::ProjectPath {
            base,
            path: Arc::from(Vec::new().into_boxed_slice()),
            context: ProjectionReductionContext::published(mode),
        },
    )
}

/// REGRESSION BOUNDARY (cancelled mapped projection): once the request
/// is cancelled, a mapped projection must stop substituting keys and
/// must not hand back a published surface.
///
/// Two halves, both end-to-end rather than site-specific. WORK: the run
/// stops within one key of the cancelling substitution instead of
/// finishing the six-key loop — measured against an uncancelled control
/// on an identical fresh host, so the bound is compared against observed
/// work rather than a guessed constant. RESULT: a cancelled projection
/// is return-only; a reader must not be handed a complete surface built
/// from a request that was told to stop.
///
/// Scope note, so the evidence is not read as more than it is: the
/// mapped build's own cancellation checks (before key enumeration,
/// before each key's remap substitution, before each key's value force)
/// are NOT what this test isolates. Removing all three leaves it
/// passing, because THIS fixture's remap dispatches a nested query per
/// key and that query refuses a cancelled request on its own, stopping
/// the loop before it can run away. This test pins the contract the
/// chain must keep as a whole, and fails if any link in it stops
/// short-circuiting.
///
/// The loop's OWN checks are load-bearing, and
/// `a_cancelled_mapped_key_loop_stops_forcing_values_on_both_rails`
/// isolates them: over a plain `[K in keyof Src]` mapper the remap
/// decision dispatches nothing, so with those checks removed a request
/// cancelled at its third substitution still forces all six keys'
/// values on both publication rails.
///
/// The inline-remap fixture is deliberate: a userland-helper remap fails
/// the FIRST key closed through the deferred-carrier arm under
/// cancellation, which bounds the loop for a reason unrelated to
/// cancellation and would make the work bound vacuous.
#[test]
fn a_cancelled_mapped_build_stops_substituting_keys() {
    use std::sync::atomic::Ordering;

    // Control: the same build, uninterrupted. Carrier lowering sits
    // outside the observed window on BOTH runs so the counts compare
    // like for like.
    let control_host = Arc::new(VerterHost::new_standalone(HostConfig::default()));
    upsert(&control_host, INLINE_REMAP_TS);
    let control_base = carrier(&control_host, "Remapped", ProjectionMode::Expanded);
    let control_observer = Arc::new(CountSubstitutions {
        seen: std::sync::atomic::AtomicU64::new(0),
    });
    let control_result = {
        let _obs = verter_audit::observer::install_observer(
            Arc::clone(&control_observer) as Arc<dyn verter_audit::AuditObserver>
        );
        project_result(&control_host, control_base, ProjectionMode::Expanded)
    };
    let control_seen = control_observer.seen.load(Ordering::SeqCst);
    assert!(
        matches!(control_result, QueryResult::Value(_)),
        "fixture invariant: the uninterrupted build must succeed, otherwise the cancelled \
         comparison below is not a comparison against a completed loop"
    );
    assert!(
        control_seen >= 6,
        "fixture invariant: the uninterrupted six-key build must issue at least one \
         substitution per iteration key, otherwise the loop running to completion is not \
         observable through this counter. Observed {control_seen}."
    );

    let host = Arc::new(VerterHost::new_standalone(HostConfig::default()));
    upsert(&host, INLINE_REMAP_TS);
    let base = carrier(&host, "Remapped", ProjectionMode::Expanded);

    let ctx = verter_session::request_context::RequestContext::new(
        1,
        Arc::from("/source.ts"),
        false,
        None,
    );
    let observer = Arc::new(CancelOnFirstSubstitution {
        seen: std::sync::atomic::AtomicU64::new(0),
        ctx: Arc::clone(&ctx),
    });
    let result = {
        let _request =
            verter_session::request_context::RequestContextGuard::install(Arc::clone(&ctx));
        // Installed AFTER the request guard so this observer, not the
        // request context's own, owns the substrate slot.
        let _obs = verter_audit::observer::install_observer(
            Arc::clone(&observer) as Arc<dyn verter_audit::AuditObserver>
        );
        project_result(&host, base, ProjectionMode::Expanded)
    };
    let seen = observer.seen.load(Ordering::SeqCst);

    assert!(
        ctx.is_cancelled(),
        "fixture invariant: the observer must actually have cancelled the request, otherwise \
         this test measures an ordinary build"
    );
    assert!(
        seen < control_seen,
        "a request cancelled at its first substitution must perform strictly less \
         substitution work than the uninterrupted build: observed {seen} against a control of \
         {control_seen}. Equal counts mean the mapped key loop ran to completion and only \
         then reported the cancellation — every key after the first was substituted, and \
         possibly forced, for a result that can never be published."
    );
    assert!(
        seen <= CANCEL_AT_SUBSTITUTION + 1,
        "cancellation is checked before key enumeration, before each key's remap \
         substitution, and before each key's value force, so the run must stop within one key \
         of the cancelling substitution. Observed {seen} substitutions (control \
         {control_seen})."
    );
    assert!(
        !matches!(result, QueryResult::Value(_)),
        "a cancelled mapped build is return-only: it must not hand back a complete published \
         surface a later reader could take as the answer. Got {result:?}"
    );
}

/// A cross-host-stable rendering of one published value node: the
/// information a consumer reads off it, with no semantic-node ids (which
/// are arena-local and therefore incomparable between two hosts).
fn value_shape(host: &Arc<VerterHost>, node: SemanticNodeId) -> String {
    let graph = host.project_type_store().semantic_graph();
    let Some(data) = graph.node_data(node) else {
        return "<absent>".to_string();
    };
    match data.as_ref() {
        SemanticNodeData::Object(view) => {
            let mut inner: Vec<String> = view
                .positive_members()
                .iter()
                .map(|m| {
                    format!(
                        "{}:{}",
                        m.string_name().unwrap_or("<non-string>"),
                        value_shape(host, m.value)
                    )
                })
                .collect();
            inner.sort();
            format!("{{{}}}", inner.join(","))
        }
        SemanticNodeData::IndexedAccess {
            object,
            index: IndexKey::String(name),
        } => {
            // A published member value is shallow by default: it is the
            // ADDRESSABLE `Source['k']` access, which renders the same
            // string whatever `k`'s type is. Resolve it through the shared
            // indexed-access query so the comparison is about the VALUE,
            // not about the carrier that addresses it — otherwise an edit
            // to the demanded key's type is invisible, which is precisely
            // the staleness this parity check exists to catch.
            let object = *object;
            let name = Arc::clone(name);
            drop(data);
            match for_tests::dispatch_execute_type_node_for_tests(
                host,
                SemanticQueryKey::IndexedAccess {
                    base: object,
                    index: IndexKey::String(Arc::clone(&name)),
                    mode: ProjectionMode::Expanded,
                },
            ) {
                QueryResult::Value(SemanticQueryOutput { value, .. }) if value != node => {
                    value_shape(host, value)
                }
                _ => format!("[{name}]"),
            }
        }
        SemanticNodeData::Primitive(kind) => format!("{kind:?}"),
        SemanticNodeData::Literal(literal) => format!("{literal:?}"),
        SemanticNodeData::Opaque(_) => "<opaque>".to_string(),
        SemanticNodeData::Mapped { .. } => "<mapped>".to_string(),
        SemanticNodeData::InstantiationRef { .. } => "<instref>".to_string(),
        _ => "<other>".to_string(),
    }
}

/// The published surface of `base` as sorted
/// `(produced member name, RESOLVED inner value shape)` pairs.
///
/// The inner value is reached by PROJECTING `base[name]['boxed']` rather
/// than by reading the published member node directly. A published mapped
/// member value is shallow by default — `Boxed<WideSource['b']>`
/// publishes the addressable `WideSource['b']` access, which renders the
/// same string whatever `b`'s type is — so a carrier-level comparison
/// would be blind to exactly the edit this parity check exists to catch.
fn surface_shape(host: &Arc<VerterHost>, base: SemanticNodeId) -> Vec<(String, String)> {
    let surface = project(host, base, Vec::new(), ProjectionMode::Expanded);
    let mut names = member_names(host, surface);
    names.sort();
    names
        .into_iter()
        .map(|name| {
            let inner = project(
                host,
                base,
                vec![
                    PathSegment::Member(PropertyKey::string_literal(name.as_str())),
                    PathSegment::Member(PropertyKey::string_literal("boxed")),
                ],
                ProjectionMode::Expanded,
            );
            let shape = value_shape(host, inner);
            (name, shape)
        })
        .collect()
}

/// `WIDE_REMAP_TS` with the DEMANDED key's value type edited
/// (`b: number` -> `b: boolean`). The edit must change the published
/// surface, because `kept_b`'s value is derived from it.
const WIDE_REMAP_DEMANDED_KEY_EDITED_TS: &str = r#"
export interface WideSource {
  a: string;
  b: boolean;
  c: boolean;
  d: string[];
  e: number[];
  f: Record<string, string>;
}

export type Boxed<V> = { boxed: V };

export type Kept<K> = K extends 'b' ? 'kept_b' : K extends 'e' ? 'kept_e' : never;

export type Remapped = {
  [K in keyof WideSource as Kept<K>]: Boxed<WideSource[K]>
};
"#;

/// `WIDE_REMAP_TS` with the REMAP edited: `e` is dropped and `a` is kept
/// under a new produced name. The edit changes the produced key set
/// without touching any value type.
const WIDE_REMAP_REMAP_EDITED_TS: &str = r#"
export interface WideSource {
  a: string;
  b: number;
  c: boolean;
  d: string[];
  e: number[];
  f: Record<string, string>;
}

export type Boxed<V> = { boxed: V };

export type Kept<K> = K extends 'b' ? 'kept_b' : K extends 'a' ? 'kept_a' : never;

export type Remapped = {
  [K in keyof WideSource as Kept<K>]: Boxed<WideSource[K]>
};
"#;

/// DISCRIMINATOR (fresh/incremental parity over VALUES, not just names):
/// after any edit, an incrementally recomputed mapped surface must equal
/// what a host that never saw the pre-edit source computes — member for
/// member, VALUE for value.
///
/// Member names alone cannot carry this. `Boxed<WideSource['b']>` names
/// its member `boxed` whatever `b`'s type is, so editing the demanded
/// key's value type leaves every published NAME identical while changing
/// the answer. A stale warm entry served after that edit is exactly the
/// failure a name comparison cannot see.
///
/// Three edit classes, each a different rail:
/// - a DEAD key's value type: the surface must be UNCHANGED (the dropped
///   key contributes no member and its value is not an operand);
/// - the DEMANDED key's value type: the surface must CHANGE (else a
///   stale value survived the edit);
/// - the REMAP itself: the produced key SET must change.
///
/// Every case additionally requires incremental == fresh, and requires
/// both runs to complete (a `Value` result, never a degraded partial).
#[test]
fn fresh_and_incremental_mapped_surfaces_match_after_dead_demanded_and_remap_edits() {
    // (label, edited source, whether the surface must change)
    let cases: [(&str, &str, bool); 3] = [
        ("dead-key value edit", WIDE_REMAP_DEAD_KEY_EDITED_TS, false),
        (
            "demanded-key value edit",
            WIDE_REMAP_DEMANDED_KEY_EDITED_TS,
            true,
        ),
        ("remap edit", WIDE_REMAP_REMAP_EDITED_TS, true),
    ];

    for (label, edited, must_change) in cases {
        // Incremental: build the pre-edit surface, edit, rebuild.
        let host = Arc::new(VerterHost::new_standalone(HostConfig::default()));
        upsert(&host, WIDE_REMAP_TS);
        let before = surface_shape(&host, carrier(&host, "Remapped", ProjectionMode::Expanded));
        upsert(&host, edited);
        let incremental_base = carrier(&host, "Remapped", ProjectionMode::Expanded);
        assert!(
            matches!(
                project_result(&host, incremental_base, ProjectionMode::Expanded),
                QueryResult::Value(_)
            ),
            "{label}: the incremental recomputation must COMPLETE — a degraded or partial \
             result is not parity with a fresh run, it is an absent answer"
        );
        let incremental = surface_shape(&host, incremental_base);

        // Fresh: a host that never saw the pre-edit source.
        let fresh = Arc::new(VerterHost::new_standalone(HostConfig::default()));
        upsert(&fresh, edited);
        let fresh_base = carrier(&fresh, "Remapped", ProjectionMode::Expanded);
        assert!(
            matches!(
                project_result(&fresh, fresh_base, ProjectionMode::Expanded),
                QueryResult::Value(_)
            ),
            "{label}: the fresh computation must complete"
        );
        let fresh_shape = surface_shape(&fresh, fresh_base);

        assert_eq!(
            incremental, fresh_shape,
            "{label}: the incrementally recomputed surface must equal what a host that never \
             saw the pre-edit source computes — member names AND member values. A difference \
             means a warm entry survived an edit it depends on."
        );

        if must_change {
            assert_ne!(
                incremental, before,
                "{label}: this edit changes the mapped answer, so the recomputed surface must \
                 differ from the pre-edit one. Equality means a stale warm surface was served \
                 for an edit it depends on — and member NAMES alone would not have shown it. before={before:?}"
            );
        } else {
            assert_eq!(
                incremental, before,
                "{label}: a remap-dropped key contributes no member and its value is not an \
                 operand of the mapped answer, so editing it must leave the published surface \
                 identical, values included."
            );
        }
    }
}

/// The same six-key source behind a `Computed` mapper with NO `as`
/// clause. The produced names ARE the iteration keys, so the key domain
/// is closed and a demanded name that is not one of them is provably
/// absent without consulting a single value operand.
const PLAIN_MAPPED_TS: &str = r#"
export interface WideSource {
  a: string;
  b: number;
  c: boolean;
  d: string[];
  e: number[];
  f: Record<string, string>;
}

export type Boxed<V> = { boxed: V };

export type PlainMapped = {
  [K in keyof WideSource]: Boxed<WideSource[K]>
};
"#;

/// DISCRIMINATOR (proven-absent key on a NON-remapping mapper): a
/// demanded literal the CLOSED key domain provably does not produce must
/// miss without forcing any key's value operand.
///
/// This is the same invariant the remapping rail proves in
/// `a_proven_absent_produced_name_misses_without_forcing_any_value`,
/// stated for the plain rail: the mapper's key domain is `keyof
/// WideSource`, the demanded name is not in it, and the answer is the
/// key-absent sentinel whichever route reaches it. Answering through
/// whole-surface `MappedType` resolution forces every SURVIVING key's
/// value — six per-K materialisations here — to produce an `Object` the
/// walker then projects the absent member off of, reaching the identical
/// miss. Work that scales with the source's WIDTH to answer a demand
/// already decided by the key domain is the full-enumeration regression
/// this file exists to forbid.
///
/// The answer leg is what stops a "skip the work by skipping the
/// semantics" implementation from passing: the demand must still produce
/// the key-absent sentinel, and a PRESENT key on the same mapper must
/// still resolve to its real value (otherwise proving absence could be
/// implemented by proving everything absent).
#[test]
fn a_proven_absent_key_on_a_plain_mapper_misses_without_forcing_any_value() {
    let host = Arc::new(VerterHost::new_standalone(HostConfig::default()));
    upsert(&host, PLAIN_MAPPED_TS);
    let base = carrier(&host, "PlainMapped", ProjectionMode::Expanded);

    let before = per_k_materializations(&host);
    let absent = project(
        &host,
        base,
        vec![PathSegment::Member(PropertyKey::string_literal("nope"))],
        ProjectionMode::Expanded,
    );
    let forced = per_k_materializations(&host) - before;

    let absent_data = host
        .project_type_store()
        .semantic_graph()
        .node_data(absent)
        .expect("the absent-key answer must be a real node");
    assert!(
        matches!(absent_data.as_ref(), SemanticNodeData::Opaque(_)),
        "a key the closed domain does not produce must answer with the walker's key-absent \
         sentinel; got {:?}",
        absent_data.as_ref()
    );
    drop(absent_data);

    assert_eq!(
        forced, 0,
        "the key domain `keyof WideSource` proves `nope` absent before any value is demanded, \
         so the miss must cost ZERO per-K value materialisations; observed {forced}. A count \
         equal to the source width (6) means the demand fell through to whole-surface mapped \
         resolution and forced every surviving key's value operand to reach the same miss."
    );

    // The absence proof must be a DECISION about this key, not a blanket
    // refusal: a key the domain DOES produce still resolves to its value.
    let present = project(
        &host,
        base,
        vec![
            PathSegment::Member(PropertyKey::string_literal("b")),
            PathSegment::Member(PropertyKey::string_literal("boxed")),
        ],
        ProjectionMode::Expanded,
    );
    // The published member value is shallow by default — the addressable
    // `WideSource['b']` access — so it is resolved through the shared
    // indexed-access query, exactly as the parity renderer does.
    assert_eq!(
        value_shape(&host, present),
        "Number",
        "`PlainMapped['b']['boxed']` is `WideSource['b']` = number and must still resolve; got \
         {}",
        describe(&host, present)
    );
}

/// REGRESSION BOUNDARY (cancelled mapped key loop, BOTH publication
/// rails): once the request is cancelled, a mapped evaluation must stop
/// forcing key values rather than run its key domain out.
///
/// The `Expanded` build (`build_mapped_type`) and the `Shallow`
/// synthesiser are twins — the same remap-then-value loop over the same
/// enumerated key domain — but they are separate code, so both are
/// driven here.
///
/// The fixture is deliberately a PLAIN `[K in keyof Src]` mapper rather
/// than one of this file's remapping fixtures, and that choice is what
/// makes the test isolate the loops' OWN cancellation checks. A remap
/// written as a conditional dispatches a nested query per key, and that
/// nested query refuses a cancelled request on its own — so a remapping
/// fixture stops whether or not the loop checks anything (see the scope
/// note on `a_cancelled_mapped_build_stops_substituting_keys`, which
/// measures the end-to-end contract on exactly such a fixture). A plain
/// mapper decides `Keep` outright with no dispatch, leaving the loop's
/// own checks as the only thing that can stop it.
///
/// Measured with the per-key checks removed: cancelling at the third
/// substitution still forced all six keys' values on BOTH rails —
/// exactly the uninterrupted control's count.
///
/// Cancelled work is return-only, so neither rail may hand back the
/// surface it could assemble from a stopped request.
#[test]
fn a_cancelled_mapped_key_loop_stops_forcing_values_on_both_rails() {
    use std::sync::atomic::Ordering;

    for mode in [ProjectionMode::Shallow, ProjectionMode::Expanded] {
        // Control: the same evaluation, uninterrupted — the count the
        // cancelled run must come in strictly under.
        let control_host = Arc::new(VerterHost::new_standalone(HostConfig::default()));
        upsert(&control_host, PLAIN_MAPPED_TS);
        let control_base = carrier(&control_host, "PlainMapped", mode);
        let control_before = per_k_materializations(&control_host);
        let control_result = project_result(&control_host, control_base, mode);
        let control_forced = per_k_materializations(&control_host) - control_before;
        assert!(
            matches!(control_result, QueryResult::Value(_)),
            "{mode:?}: fixture invariant — the uninterrupted evaluation must succeed, otherwise \
             the cancelled comparison below is not a comparison against a completed loop"
        );
        assert_eq!(
            control_forced, 6,
            "{mode:?}: fixture invariant — the uninterrupted six-key evaluation must force one \
             value per key, otherwise per-key forcing is not observable through this counter. \
             Observed {control_forced}."
        );

        let host = Arc::new(VerterHost::new_standalone(HostConfig::default()));
        upsert(&host, PLAIN_MAPPED_TS);
        let base = carrier(&host, "PlainMapped", mode);

        let ctx = verter_session::request_context::RequestContext::new(
            1,
            Arc::from("/source.ts"),
            false,
            None,
        );
        let observer = Arc::new(CancelOnFirstSubstitution {
            seen: std::sync::atomic::AtomicU64::new(0),
            ctx: Arc::clone(&ctx),
        });
        let before = per_k_materializations(&host);
        let result = {
            let _request =
                verter_session::request_context::RequestContextGuard::install(Arc::clone(&ctx));
            // Installed AFTER the request guard so this observer, not the
            // request context's own, owns the substrate slot.
            let _obs = verter_audit::observer::install_observer(
                Arc::clone(&observer) as Arc<dyn verter_audit::AuditObserver>
            );
            project_result(&host, base, mode)
        };
        let forced = per_k_materializations(&host) - before;
        let seen = observer.seen.load(Ordering::SeqCst);

        assert!(
            ctx.is_cancelled(),
            "{mode:?}: fixture invariant — the observer must actually have cancelled the \
             request, otherwise this measures an ordinary evaluation"
        );
        assert!(
            forced < control_forced,
            "{mode:?}: a mapped evaluation cancelled partway through its key domain must force \
             strictly fewer key values than the uninterrupted run: observed {forced} against a \
             control of {control_forced}. Equal counts mean the key loop ran to the end of the \
             domain after the request was told to stop."
        );
        assert!(
            forced <= 3,
            "{mode:?}: cancellation is checked before key enumeration, before each key's remap \
             substitution and before each key's value force, so a request cancelled at its \
             third substitution must stop within one key of it. Observed {forced} value \
             forcings after {seen} substitutions (control {control_forced})."
        );
        assert!(
            !matches!(result, QueryResult::Value(_)),
            "{mode:?}: a cancelled mapped evaluation is return-only — the partial member set it \
             assembled is not a complete surface and must never be handed back as one. Got \
             {result:?}"
        );
    }
}
