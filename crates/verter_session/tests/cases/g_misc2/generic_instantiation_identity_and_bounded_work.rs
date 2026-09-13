//! Exact-identity and bounded-work discriminators for generic
//! instantiation and mapper classification.
//!
//! Durable invariants under test:
//!
//! 1. **Argument binding is positional and exact.** A type argument
//!    binds to the parameter at its declared ordinal, and a declared
//!    default (`B = A`) binds to the NAMED parameter it references —
//!    never to whichever argument happened to be supplied. A binder
//!    that aliases two parameters, or resolves a default against the
//!    wrong ordinal, produces a surface that still type-checks
//!    structurally, so only a per-member value assertion catches it.
//!
//! 2. **A constrained parameter substitutes the exact argument.** Two
//!    instantiations of one constrained generic with different literal
//!    arguments must produce different bodies. Collapsing them to the
//!    constraint (or to each other) is the aliasing failure the
//!    constraint makes easy to hide.
//!
//! 3. **Distinct argument environments keep distinct instantiation
//!    identity, and identical ones join the existing family memo.** A
//!    second demand with the same arguments must not re-instantiate; a
//!    demand with different arguments must.
//!
//! 4. **Repeated warm demands do not grow the memo.** Re-issuing an
//!    already-cached instantiation must add no entries and take no
//!    misses, and one generic body lowers once no matter how many
//!    distinct argument environments instantiate it.
//!
//! 5. **Mapper classification is structural.** Choosing `Identity`
//!    versus `Computed` reads the mapper's carrier shape and binder
//!    identity; it must not materialise the value body to decide, and
//!    must not run the relation engine.
//!
//! 6. **An unused type argument is a dead operand.** `Ignore<A, B>`
//!    never mentions `B`, so the argument bound to `B` contributes
//!    nothing to any surface reachable from the instantiation. Work
//!    must therefore be independent of that argument's STRUCTURE:
//!    replacing a literal with a nested generic instantiation may not
//!    cost additional instantiations or substitutions.
//!
//! 7. **Fresh and incremental answers agree across default and
//!    constraint edits.** Editing a type parameter's declared default
//!    changes what an argument-less position resolves to; editing its
//!    constraint changes nothing, because a constraint bounds an
//!    argument and never substitutes for it. Both must recompute to
//!    exactly what a host that never saw the pre-edit source produces,
//!    compared over RESOLVED values — member names are identical either
//!    way, so a name comparison would see nothing at all.
//!
//! These sit alongside the mapped-side key-domain discriminators in
//! `mapped_remap_dead_key_value_forcing`: that file pins which keys are
//! forced, this one pins that the substitution each forced key applies
//! is the exact one, and that repeating a demand costs nothing.

use std::sync::Arc;

use verter_session::semantic_query::{
    IndexKey, PathSegment, ProjectionMode, ProjectionReductionContext, QueryResult,
    SemanticNodeData, SemanticNodeId, SemanticQueryKey, SemanticQueryOutput,
};
use verter_session::{for_tests, HostConfig, UpsertRequest, VerterHost};
use verter_type_expr::{PrimitiveName, TypeExpr};

/// `Pair` carries a default that references the FIRST parameter by name,
/// so a binder that mixes up ordinals or resolves the default against
/// the supplied argument list rather than the named parameter produces a
/// visibly different surface.
///
/// `IdentityOpen` / `ComputedOpen` are the two mapper classes over an
/// unbound source: `T[K]` is the canonical `Identity` mapper body,
/// `Boxed<T[K]>` is `Computed`. Unbound, they lower to the deferred
/// `Mapped` carrier, so the classified `MapperKind` is readable without
/// any surface materialisation.
const SOURCE_TS: &str = r#"
export interface WideSource { a: string; b: number; c: boolean; }

export type Boxed<V> = { boxed: V };

export type Pair<A, B = A> = { first: A; second: B };

export type Constrained<K extends keyof WideSource> = { picked: WideSource[K] };

export type IdentityOpen<T> = { [K in keyof T]: T[K] };

export type ComputedOpen<T> = { [K in keyof T]: Boxed<T[K]> };

export type Deep<V> = { a: { b: { c: V } } };

export type Ignore<A, B> = { only: A };
"#;

fn new_host() -> Arc<VerterHost> {
    let host = Arc::new(VerterHost::new_standalone(HostConfig::default()));
    let _ = host.upsert(UpsertRequest {
        canonical_id: Some("/source.ts".to_string()),
        input_id: "/source.ts".to_string(),
        source: Arc::from(SOURCE_TS),
        file_language: verter_session::LanguageRegistry::global()
            .classify_static("/source.ts")
            .static_resolution(),
        aliases: Vec::new(),
    });
    host
}

fn type_ref(name: &str, args: Vec<TypeExpr>) -> TypeExpr {
    TypeExpr::Ref {
        name: Arc::from(name),
        type_arguments: Arc::from(args.into_boxed_slice()),
    }
}

fn lower(host: &Arc<VerterHost>, expr: &TypeExpr, mode: ProjectionMode) -> SemanticNodeId {
    for_tests::dispatch_lower_type_expr_in_scope_with_context_for_tests(
        host,
        "/source.ts",
        expr,
        ProjectionReductionContext::published(mode),
    )
    .unwrap_or_else(|| panic!("lowering `{expr:?}` must succeed"))
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
        QueryResult::Value(SemanticQueryOutput { value, .. }) => value,
        other => panic!("ProjectPath must yield a value node, got {other:?}"),
    }
}

/// Resolve `name<args>` to its fully evaluated surface node.
fn instantiate(host: &Arc<VerterHost>, name: &str, args: Vec<TypeExpr>) -> SemanticNodeId {
    let base = lower(host, &type_ref(name, args), ProjectionMode::Expanded);
    project(host, base, Vec::new(), ProjectionMode::Expanded)
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

fn instantiate_count(host: &Arc<VerterHost>) -> u64 {
    host.project_type_store()
        .semantic_graph()
        .stats_snapshot()
        .instantiate_count
}

/// DISCRIMINATOR (positional binding + named default): `Pair<A, B = A>`
/// must bind by declared ordinal, and its default must resolve to the
/// parameter it names.
///
/// `Pair<string>` leaves `B` to its default, which names `A`, so BOTH
/// members must carry the argument bound to `A`. `Pair<string, number>`
/// supplies both, so the members must differ and must follow the
/// declared order.
///
/// A swapped binder order passes the first case (both members are the
/// same type) and fails the second. A default resolved against the
/// argument LIST rather than the named parameter fails the first —
/// there is no second argument to resolve against. Only the pair of
/// cases together pins both.
#[test]
fn generic_arguments_bind_by_declared_order_and_defaults_follow_the_named_parameter() {
    let host = new_host();

    let defaulted = instantiate(
        &host,
        "Pair",
        vec![TypeExpr::primitive(PrimitiveName::String)],
    );
    let first = member_value(&host, defaulted, "first");
    let second = member_value(&host, defaulted, "second");
    assert_eq!(
        first,
        second,
        "`Pair<string>` leaves `B` to its declared default `= A`, which NAMES the first \
         parameter, so both members must be the argument bound to `A`. first={} second={}",
        describe(&host, first),
        describe(&host, second)
    );
    assert!(
        matches!(
            host.project_type_store()
                .semantic_graph()
                .node_data(first)
                .as_deref(),
            Some(SemanticNodeData::Primitive(_))
        ),
        "the defaulted members must be the SUBSTITUTED argument, not a residual parameter \
         shell or the unresolved default expression. Got {}",
        describe(&host, first)
    );

    let explicit = instantiate(
        &host,
        "Pair",
        vec![
            TypeExpr::primitive(PrimitiveName::String),
            TypeExpr::primitive(PrimitiveName::Number),
        ],
    );
    let explicit_first = member_value(&host, explicit, "first");
    let explicit_second = member_value(&host, explicit, "second");
    assert_ne!(
        explicit_first,
        explicit_second,
        "`Pair<string, number>` supplies distinct arguments, so its members must differ; \
         equal members mean the two parameters alias one binder. first={} second={}",
        describe(&host, explicit_first),
        describe(&host, explicit_second)
    );
    assert_eq!(
        explicit_first,
        first,
        "argument binding is POSITIONAL: `A` takes the first argument in both `Pair<string>` \
         and `Pair<string, number>`, so `first` must be the same node in both. A swapped \
         binder order gives `first` the second argument here. got={} want={}",
        describe(&host, explicit_first),
        describe(&host, first)
    );
}

/// DISCRIMINATOR (constrained parameter substitution): each argument to
/// a constrained generic must substitute exactly, not collapse to the
/// constraint or to a sibling instantiation.
///
/// `Constrained<K extends keyof WideSource>` projects `WideSource[K]`.
/// Three different literal arguments must produce three different
/// bodies. Substituting the CONSTRAINT (`keyof WideSource`) instead of
/// the argument, or memoising the instantiation on the declaration
/// rather than on `(declaration, arguments)`, makes all three identical.
#[test]
fn constrained_parameter_substitutes_the_exact_argument_not_its_constraint() {
    let host = new_host();

    let picked: Vec<(char, SemanticNodeId)> = ['a', 'b', 'c']
        .into_iter()
        .map(|key| {
            let surface = instantiate(
                &host,
                "Constrained",
                vec![TypeExpr::string_literal(key.to_string())],
            );
            (key, member_value(&host, surface, "picked"))
        })
        .collect();

    for (key, node) in &picked {
        let graph = host.project_type_store().semantic_graph();
        let data = graph.node_data(*node);
        let indexed_key = match data.as_deref() {
            Some(SemanticNodeData::IndexedAccess {
                index: verter_session::semantic_query::IndexKey::String(name),
                ..
            }) => name.to_string(),
            other => panic!(
                "`Constrained<'{key}'>['picked']` must carry the substituted index; got {other:?}"
            ),
        };
        assert_eq!(
            indexed_key,
            key.to_string(),
            "the constrained parameter must substitute the EXACT argument into the index \
             position; substituting the constraint `keyof WideSource` or a sibling \
             instantiation's argument gives a different key"
        );
    }

    let distinct: std::collections::BTreeSet<_> = picked.iter().map(|(_, n)| *n).collect();
    assert_eq!(
        distinct.len(),
        3,
        "three different arguments must yield three different bodies; collapsing them means \
         the instantiation memoised on the declaration alone rather than on (declaration, \
         arguments). got {picked:?}"
    );
}

/// DISCRIMINATOR (instantiation identity + memo join): a repeated demand
/// with the SAME arguments must not re-instantiate, and a demand with
/// DIFFERENT arguments must.
///
/// This is the bounded-work contract for generic instantiation: work
/// scales with distinct argument environments, not with demand count. A
/// key that dropped its arguments would make the third case free and
/// wrong (it would return `Pair<string>`'s body); a key that carried
/// something demand-local (a fresh node id, a request identity) would
/// make the second case pay again.
#[test]
fn distinct_argument_environments_instantiate_once_each_and_repeats_join_the_memo() {
    let host = new_host();

    let before_first = instantiate_count(&host);
    let first = instantiate(
        &host,
        "Pair",
        vec![TypeExpr::primitive(PrimitiveName::String)],
    );
    let cold = instantiate_count(&host) - before_first;
    assert!(
        cold >= 1,
        "fixture invariant: the cold demand must actually instantiate `Pair`, otherwise the \
         repeat assertion below is vacuous; observed {cold}"
    );

    let before_repeat = instantiate_count(&host);
    let repeat = instantiate(
        &host,
        "Pair",
        vec![TypeExpr::primitive(PrimitiveName::String)],
    );
    let repeat_cost = instantiate_count(&host) - before_repeat;
    assert_eq!(
        repeat, first,
        "an identical instantiation demand must return the same node"
    );
    assert_eq!(
        repeat_cost, 0,
        "an identical instantiation demand must join the existing family memo and instantiate \
         NOTHING; observed {repeat_cost} further instantiations, so the key carries something \
         demand-local instead of the (declaration slot, arguments) identity"
    );

    let before_other = instantiate_count(&host);
    let other = instantiate(
        &host,
        "Pair",
        vec![TypeExpr::primitive(PrimitiveName::Number)],
    );
    let other_cost = instantiate_count(&host) - before_other;
    assert_ne!(
        other, first,
        "a different argument environment must keep a distinct instantiation identity; \
         aliasing it onto `Pair<string>` returns the wrong body"
    );
    assert!(
        other_cost >= 1,
        "a different argument environment must be instantiated on its own; observed \
         {other_cost}, which means it was served from `Pair<string>`'s candidate"
    );
}

/// DISCRIMINATOR (warm retention): repeatedly re-issuing an already
/// cached instantiation must add no memo entries and take no misses.
///
/// Candidate growth on warm repeats is the retention failure the
/// per-family cap exists to bound: an entry whose discriminant varies
/// per request publishes a fresh candidate every time, evicting live
/// ones and turning a warm read into a cold one at the cap.
#[test]
fn repeated_warm_instantiation_demands_do_not_grow_the_memo() {
    let host = new_host();
    let graph = host.project_type_store().semantic_graph();

    // Warm the demand once so the measured window is purely warm.
    let warmed = instantiate(
        &host,
        "Pair",
        vec![TypeExpr::primitive(PrimitiveName::String)],
    );

    let before = graph.stats_snapshot();
    for _ in 0..5 {
        let again = instantiate(
            &host,
            "Pair",
            vec![TypeExpr::primitive(PrimitiveName::String)],
        );
        assert_eq!(again, warmed, "every warm repeat must return the same node");
    }
    let after = graph.stats_snapshot();

    assert_eq!(
        after.memo_entry_count, before.memo_entry_count,
        "five warm repeats of one instantiation must publish NO new memo entries; entries went \
         {} -> {}",
        before.memo_entry_count, after.memo_entry_count
    );
    assert_eq!(
        after.instantiate_count, before.instantiate_count,
        "five warm repeats must instantiate nothing further"
    );
    assert_eq!(
        after.misses, before.misses,
        "five warm repeats must take no memo misses; a miss means the warm candidate was not \
         satisfied and a fresh one was computed"
    );
    assert!(
        after.hits > before.hits,
        "five warm repeats must register memo hits; zero hits means the repeats never reached \
         the memo and the no-growth assertions above are vacuous"
    );
}

/// DISCRIMINATOR (classifier purity): choosing `MapperKind` must not
/// materialise the mapper's value body or run the relation engine.
///
/// `IdentityOpen` (`T[K]`) and `ComputedOpen` (`Boxed<T[K]>`) differ
/// ONLY in their value expression, so the classifier must distinguish
/// them from carrier shape and binder identity alone. Deciding
/// `Identity` versus `Computed` by materialising the value and
/// inspecting the result — or by asking the relation engine whether the
/// value is the source member type — would move the per-K materialiser
/// and relation counters. Both must stay at zero while the kinds still
/// come out right.
#[test]
fn mapper_kind_classification_materialises_no_value_body() {
    for (alias, expected) in [
        (
            "IdentityOpen",
            verter_session::semantic_query::MapperKind::Identity,
        ),
        (
            "ComputedOpen",
            verter_session::semantic_query::MapperKind::Computed,
        ),
    ] {
        // A fresh host per alias so the counters measure this alias's
        // classification and nothing carried over from the previous one.
        let host = new_host();
        let graph = host.project_type_store().semantic_graph();

        let before = graph.stats_snapshot();
        let node = lower(
            &host,
            &type_ref(alias, Vec::new()),
            ProjectionMode::Expanded,
        );
        let after = graph.stats_snapshot();

        let kind = match graph.node_data(node).as_deref() {
            Some(SemanticNodeData::Mapped { mapper, .. }) => mapper.kind,
            other => panic!(
                "fixture invariant: an unbound mapped alias must lower to the deferred `Mapped` \
                 carrier so its classified kind is readable; got {other:?}"
            ),
        };
        assert_eq!(
            kind, expected,
            "`{alias}` must classify as {expected:?} from its carrier shape"
        );
        assert_eq!(
            after.mapped_per_k_materializations, before.mapped_per_k_materializations,
            "classifying `{alias}` must materialise NO per-key value: the kind is a structural \
             lowering fact, not a property of the evaluated value body"
        );
        assert_eq!(
            after.relation_check_count, before.relation_check_count,
            "classifying `{alias}` must run NO relation check: deciding `Identity` by asking \
             whether the value relates to the source member type is a semantic decision the \
             classifier is not allowed to make"
        );
    }
}

/// DISCRIMINATOR (body lowered once per declaration content): three
/// distinct argument environments of one generic must lower that
/// generic's declaration body EXACTLY once.
///
/// The declaration body is a property of the declaration's content, not
/// of any argument environment: `Pair<string>`, `Pair<number>` and
/// `Pair<boolean>` all instantiate the same lowered body under different
/// substitutions. Re-lowering per environment is the failure this
/// catches — it turns a per-declaration cost into a per-demand one and
/// scales with call sites rather than with content.
///
/// `decl_bodies_lowered` is the lazy body service's own counter, so the
/// assertion reads the exact quantity the invariant names. The three
/// environments must genuinely differ (each is a distinct instantiation
/// identity — pinned by
/// `distinct_argument_environments_instantiate_once_each_and_repeats_join_the_memo`),
/// otherwise a count of one would only mean the demands collapsed.
#[test]
fn one_generic_body_lowers_once_across_distinct_argument_environments() {
    let host = new_host();
    host.provenance().reset();

    let mut surfaces = Vec::new();
    for argument in [
        PrimitiveName::String,
        PrimitiveName::Number,
        PrimitiveName::Boolean,
    ] {
        surfaces.push(instantiate(
            &host,
            "Pair",
            vec![TypeExpr::primitive(argument)],
        ));
    }

    // The three demands must be genuinely distinct, else "lowered once"
    // is a statement about one demand rather than three.
    let distinct: std::collections::BTreeSet<_> = surfaces.iter().copied().collect();
    assert_eq!(
        distinct.len(),
        3,
        "fixture invariant: the three argument environments must produce three distinct \
         instantiations; got {surfaces:?}"
    );

    let lowered = host.provenance().snapshot().decl_bodies_lowered;
    assert_eq!(
        lowered, 1,
        "three distinct argument environments of one generic must lower its declaration body \
         exactly once — the body belongs to the declaration's content, and each environment \
         only re-substitutes it. Observed {lowered} lowerings, i.e. the body was re-lowered per \
         argument environment."
    );
}

/// DISCRIMINATOR (dead generic argument): the argument bound to a type
/// parameter the declaration body never mentions must not be
/// instantiated.
///
/// `Ignore<A, B> = { only: A }` ignores `B` entirely. `Ignore<'x', 'y'>`
/// and `Ignore<'x', Deep<'y'>>` therefore publish the SAME surface — the
/// second merely hands a structurally deeper type to the dead parameter.
/// Any extra instantiation or substitution the second demand performs is
/// work for an operand no reachable surface consumes, and it scales with
/// the dead argument's depth rather than with what was demanded.
///
/// Two hosts so each measurement is cold and attributable; the surface
/// equality leg keeps the counter comparison honest (dropping the work by
/// dropping the semantics would satisfy the counters alone).
#[test]
fn an_unused_generic_argument_is_not_instantiated() {
    fn measure(argument: TypeExpr) -> (u64, u64, Vec<String>) {
        let host = new_host();
        let graph = host.project_type_store().semantic_graph();
        let before = graph.stats_snapshot();
        let surface = instantiate(
            &host,
            "Ignore",
            vec![TypeExpr::string_literal("x".to_string()), argument],
        );
        let after = graph.stats_snapshot();
        let names = match host
            .project_type_store()
            .semantic_graph()
            .node_data(surface)
            .as_deref()
        {
            Some(SemanticNodeData::Object(view)) => view
                .positive_members()
                .iter()
                .filter_map(|m| m.string_name().map(str::to_string))
                .collect(),
            other => panic!("expected an Object surface, got {other:?}"),
        };
        (
            after.instantiate_count - before.instantiate_count,
            after.substitute_memo_misses - before.substitute_memo_misses,
            names,
        )
    }

    let (shallow_inst, shallow_subs, shallow_names) =
        measure(TypeExpr::string_literal("y".to_string()));
    let (deep_inst, deep_subs, deep_names) = measure(TypeExpr::Ref {
        name: Arc::from("Deep"),
        type_arguments: Arc::from(
            vec![TypeExpr::string_literal("y".to_string())].into_boxed_slice(),
        ),
    });

    assert_eq!(
        shallow_names,
        vec!["only".to_string()],
        "fixture invariant: `Ignore` publishes exactly its one `only` member"
    );
    assert_eq!(
        deep_names, shallow_names,
        "a deeper argument bound to the UNUSED parameter cannot change the published surface"
    );
    assert_eq!(
        deep_inst, shallow_inst,
        "the argument bound to the unused parameter `B` is a DEAD operand: no surface          reachable from `Ignore<A, B>` consumes it, so replacing its literal with          `Deep<'y'>` must cost no additional instantiations. Observed {shallow_inst} ->          {deep_inst}; a difference means the reference site instantiated the dead argument          before any parameter usage demanded it, and the cost scales with the dead          argument's depth."
    );
    assert_eq!(
        deep_subs, shallow_subs,
        "likewise for substitutions: the dead argument must not be substituted. Observed          {shallow_subs} -> {deep_subs}."
    );
}

/// `SOURCE_TS` with `Pair`'s DECLARED DEFAULT edited: `B = A` becomes
/// `B = number`. `Pair<string>` supplies no second argument, so the
/// default is the only thing deciding `second` — the published answer
/// must change from `string` to `number`.
const DEFAULT_EDITED_TS: &str = r#"
export interface WideSource { a: string; b: number; c: boolean; }

export type Boxed<V> = { boxed: V };

export type Pair<A, B = number> = { first: A; second: B };

export type Constrained<K extends keyof WideSource> = { picked: WideSource[K] };

export type IdentityOpen<T> = { [K in keyof T]: T[K] };

export type ComputedOpen<T> = { [K in keyof T]: Boxed<T[K]> };

export type Deep<V> = { a: { b: { c: V } } };

export type Ignore<A, B> = { only: A };
"#;

/// `SOURCE_TS` with `Constrained`'s CONSTRAINT EXPRESSION edited:
/// `K extends keyof WideSource` becomes `K extends 'a' | 'b'`. A
/// constraint is a BOUND, not a substitution source, so an argument the
/// new bound still admits must resolve to exactly the same answer.
const CONSTRAINT_EDITED_TS: &str = r#"
export interface WideSource { a: string; b: number; c: boolean; }

export type Boxed<V> = { boxed: V };

export type Pair<A, B = A> = { first: A; second: B };

export type Constrained<K extends 'a' | 'b'> = { picked: WideSource[K] };

export type IdentityOpen<T> = { [K in keyof T]: T[K] };

export type ComputedOpen<T> = { [K in keyof T]: Boxed<T[K]> };

export type Deep<V> = { a: { b: { c: V } } };

export type Ignore<A, B> = { only: A };
"#;

fn upsert_source(host: &Arc<VerterHost>, source: &str) {
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

fn host_with(source: &str) -> Arc<VerterHost> {
    let host = Arc::new(VerterHost::new_standalone(HostConfig::default()));
    upsert_source(&host, source);
    host
}

/// A cross-host-stable rendering of one value node. Semantic node ids are
/// arena-local, so two hosts' answers can only be compared through the
/// information a consumer actually reads off them.
///
/// A member value reached through a constrained parameter is published
/// shallow — `WideSource['a']` is the ADDRESSABLE access, which renders
/// identically whatever `a`'s type is — so it is resolved through the
/// shared indexed-access query. Without that step the comparison would be
/// about the carrier that addresses the value rather than the value, and
/// blind to exactly the staleness this parity check exists to catch.
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

/// Resolve `name<args>` and render its surface cross-host-stably,
/// requiring the projection to COMPLETE. A degraded or partial result is
/// not parity with a fresh run, it is an absent answer.
fn instantiation_shape(host: &Arc<VerterHost>, name: &str, args: Vec<TypeExpr>) -> String {
    let base = lower(host, &type_ref(name, args), ProjectionMode::Expanded);
    let query = SemanticQueryKey::ProjectPath {
        base,
        path: Arc::from(Vec::new().into_boxed_slice()),
        context: ProjectionReductionContext::published(ProjectionMode::Expanded),
    };
    match for_tests::dispatch_execute_type_node_for_tests(host, query) {
        QueryResult::Value(SemanticQueryOutput { value, .. }) => value_shape(host, value),
        other => panic!("the instantiation must complete, got {other:?}"),
    }
}

/// DISCRIMINATOR (fresh/incremental parity across generic DEFAULT and
/// CONSTRAINT edits): after an owner-file edit to a type parameter's
/// declared default or constraint expression, the incrementally
/// recomputed instantiation must equal what a host that never saw the
/// pre-edit source computes — member for member, resolved VALUE for
/// value.
///
/// The mapped-side parity discriminator covers the key-domain edit
/// classes (dead key, demanded key, remap). These are the GENERIC-side
/// classes, and they ride a different rail: a type argument's own
/// lowering decides what the substitution environment carries, and a
/// declared default is lowered at the declaration rather than supplied at
/// the reference site.
///
/// The two cases pin opposite halves of the contract:
///
/// - editing the DEFAULT must CHANGE the answer, because `Pair<string>`
///   supplies no second argument and the default is the only thing
///   deciding `second`. Equality after the edit means a stale warm
///   instantiation was served for an edit it depends on;
/// - editing the CONSTRAINT must NOT change it, because a constraint
///   bounds the argument and never substitutes for it. A change here is
///   the aliasing failure where a constrained parameter collapses to its
///   bound.
///
/// Both require incremental == fresh, over resolved values rather than
/// member names: `Pair` publishes `first`/`second` whatever its default
/// is, so a name comparison would see nothing at all.
#[test]
fn fresh_and_incremental_generic_instantiations_match_after_default_and_constraint_edits() {
    // (label, edited source, alias, arguments, must the answer change)
    let cases: [(&str, &str, &str, Vec<TypeExpr>, bool); 2] = [
        (
            "declared default edit",
            DEFAULT_EDITED_TS,
            "Pair",
            vec![TypeExpr::primitive(PrimitiveName::String)],
            true,
        ),
        (
            "constraint expression edit",
            CONSTRAINT_EDITED_TS,
            "Constrained",
            vec![TypeExpr::string_literal("a")],
            false,
        ),
    ];

    for (label, edited, name, args, must_change) in cases {
        // Incremental: build the pre-edit answer, edit the owner file,
        // rebuild on the same host.
        let host = host_with(SOURCE_TS);
        let before = instantiation_shape(&host, name, args.clone());
        upsert_source(&host, edited);
        let incremental = instantiation_shape(&host, name, args.clone());

        // Fresh: a host that never saw the pre-edit source.
        let fresh = host_with(edited);
        let fresh_shape = instantiation_shape(&fresh, name, args.clone());

        assert_eq!(
            incremental, fresh_shape,
            "{label}: the incrementally recomputed instantiation must equal what a host that \
             never saw the pre-edit source computes. A difference means a warm entry survived \
             an edit it depends on, or that recomputation diverged from a cold build."
        );

        if must_change {
            assert_ne!(
                incremental, before,
                "{label}: this edit changes the instantiated answer, so the recomputed surface \
                 must differ from the pre-edit one (before={before}). Equality means a stale \
                 warm instantiation was served — and member NAMES alone would not have shown \
                 it, since `first`/`second` are published either way."
            );
        } else {
            assert_eq!(
                incremental, before,
                "{label}: a constraint bounds its parameter and never substitutes for it, so an \
                 argument the edited bound still admits must resolve to the same answer. A \
                 change means the parameter collapsed to its constraint."
            );
        }
    }

    // Sibling instantiations must stay distinct across a constraint edit.
    // Collapsing a constrained parameter to its bound would make every
    // admitted argument produce one shared answer — which the per-case
    // comparison above cannot see, because each case fixes one argument.
    let host = host_with(SOURCE_TS);
    let _ = instantiation_shape(&host, "Constrained", vec![TypeExpr::string_literal("a")]);
    let _ = instantiation_shape(&host, "Constrained", vec![TypeExpr::string_literal("b")]);
    upsert_source(&host, CONSTRAINT_EDITED_TS);
    let picked_a = instantiation_shape(&host, "Constrained", vec![TypeExpr::string_literal("a")]);
    let picked_b = instantiation_shape(&host, "Constrained", vec![TypeExpr::string_literal("b")]);
    assert_ne!(
        picked_a, picked_b,
        "after a constraint edit, two arguments the bound admits must still substitute \
         independently: `WideSource['a']` is string and `WideSource['b']` is number. Equal \
         answers mean the instantiations aliased each other or collapsed to the shared bound."
    );
}
