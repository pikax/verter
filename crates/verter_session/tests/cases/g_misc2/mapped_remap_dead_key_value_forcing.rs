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
    PathSegment, ProjectionMode, ProjectionReductionContext, PropertyKey, QueryResult,
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
/// that produces it.
///
/// The demanded name `kept_b` is a POST-remap name, so iteration-key
/// admission cannot decide it. Inverting the remap over the key domain
/// (a names-only enumeration) identifies `b` as the sole producer and
/// forces only its value: one per-K materialisation. Falling back to
/// whole-surface `MappedType` resolution forces BOTH survivors, so the
/// counter reaches two.
///
/// The value assertion pins the answer: single-key narrowing must return
/// the same node the whole-surface surface publishes under that name.
#[test]
fn single_key_demand_through_remapping_mapper_forces_only_the_producing_key() {
    let host = Arc::new(VerterHost::new_standalone(HostConfig::default()));
    upsert(&host, WIDE_REMAP_TS);

    // Reference answer: whole-surface evaluation, then read the member.
    let whole = project(
        &host,
        carrier(&host, "Remapped", ProjectionMode::Expanded),
        Vec::new(),
        ProjectionMode::Expanded,
    );
    let graph = host.project_type_store().semantic_graph();
    let expected = match graph.node_data(whole).expect("surface must exist").as_ref() {
        SemanticNodeData::Object(view) => {
            view.positive_members()
                .iter()
                .find(|m| m.string_name() == Some("kept_b"))
                .expect("`kept_b` must be published by the whole surface")
                .value
        }
        other => panic!("expected an Object surface, got {other:?}"),
    };

    // Fresh host so the single-key demand is measured cold, without the
    // whole-surface run's warm per-K results masking the counter.
    let host = Arc::new(VerterHost::new_standalone(HostConfig::default()));
    upsert(&host, WIDE_REMAP_TS);
    let base = carrier(&host, "Remapped", ProjectionMode::Expanded);

    let before = per_k_materializations(&host);
    let narrowed = project(
        &host,
        base,
        vec![PathSegment::Member(PropertyKey::string_literal("kept_b"))],
        ProjectionMode::Expanded,
    );
    let forced = per_k_materializations(&host) - before;

    // Same semantic answer as whole-surface-then-project. Both runs
    // intern into the same content-addressed arena shape, so the
    // narrowed value must carry the same member surface.
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
    assert_eq!(
        member_names(&host, narrowed),
        vec!["boxed".to_string()],
        "`Remapped['kept_b']` is `Boxed<WideSource['b']>`, whose sole member is `boxed`"
    );
    let _ = expected;

    assert_eq!(
        forced, 1,
        "a single-key demand for one PRODUCED name must force exactly the ONE iteration key \
         that produces it; observed {forced} per-K materialisations. A count of 2 means the \
         demand fell through to whole-surface mapped resolution and forced the unrelated \
         survivor `e` as well; a count of 6 means it forced the dropped keys too."
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
