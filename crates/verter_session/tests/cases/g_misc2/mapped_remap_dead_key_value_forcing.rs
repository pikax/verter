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
//! Both legs also assert the ANSWER, not just the counter: narrowing
//! must return the same semantic surface/value that whole-surface
//! evaluation produces, and a remap-dropped key must be absent from the
//! published surface. A counter-only assertion would pass for an
//! implementation that skipped the work by skipping the semantics.

#![allow(clippy::too_many_lines)]

use std::sync::Arc;

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
