//! Publication-shape improvement comparison.
//!
//! The publication finaliser picks between two candidate published-field shapes.
//! Both the node-domain comparison ([`compare_node_improvement`]) and the
//! `TypeExpr` comparison ([`compare_type_expr_improvement`]) read the SAME
//! publication-scoring facts ([`crate::project_semantic_dispatch::raise::PublicationScore`])
//! and apply the SAME [`publication_score_improves`] formula — there is exactly
//! ONE scoring algebra (the node front rides the shared `SemanticNodeData` fold;
//! the `TypeExpr` front feeds the same per-arm rules), so the two comparisons can
//! never drift. [`node_root_is_explicit_selector_operator`] reads the carrier kind
//! directly and is a separate, non-scoring predicate.

use crate::project_semantic_dispatch::raise::PublicationScore;

/// Whether `candidate` is a strictly BETTER published shape than `current`,
/// scored over the publication-scoring facts. The SINGLE comparison formula both
/// [`compare_node_improvement`] (node front) and [`compare_type_expr_improvement`]
/// (`TypeExpr` front) delegate to:
///
/// 1. a concrete shape beats an exact-`Unknown` root;
/// 2. else: fewer symbolic carriers wins;
/// 3. else: a structural top-level wins over a symbolic one;
/// 4. else (equal carriers): more generic detail wins.
fn publication_score_improves(candidate: &PublicationScore, current: &PublicationScore) -> bool {
    if current.exact_unknown_root && !candidate.exact_unknown_root {
        return true;
    }
    candidate.symbolic_carriers < current.symbolic_carriers
        || (candidate.structural_top_level && !current.structural_top_level)
        || (candidate.symbolic_carriers == current.symbolic_carriers
            && candidate.generic_detail > current.generic_detail)
}

/// Whether `candidate` is a strictly BETTER published shape than `current`,
/// scored on `raise(*)` WITHOUT materialising a `TypeExpr`. Used by the
/// publication finaliser to pick between the field's reduction and the shallow
/// form's reduction in node domain.
///
/// SCORING-INVARIANT SCOPE: the [`publication_score_improves`] ordering is
/// defined over RAISABLE nodes (those whose publication score is `Some`). An
/// UNRAISABLE node scores `None` — it has no shape to publish — and is treated
/// as NOT an improvement (`(None, _) => false`). The symmetric `(Some, None) =>
/// true`: a raisable candidate improves over an unraisable (shapeless) current.
/// Scoring an unraisable node as `PublicationScore::default()` would be WRONG:
/// its zero symbolic carriers would beat any symbolic current
/// (`0 < current.symbolic_carriers`), wrongly preferring the shapeless candidate.
pub(crate) fn compare_node_improvement(
    ctx: &dyn crate::resolver_core::ResolverContext,
    candidate: crate::semantic_query::SemanticNodeId,
    current: crate::semantic_query::SemanticNodeId,
) -> bool {
    let dispatch = crate::project_semantic_dispatch::ProjectSemanticDispatch::new(ctx);
    let candidate_score =
        crate::project_semantic_dispatch::raise::project_node_publication_score_with_dispatch(
            &dispatch, candidate,
        );
    let current_score =
        crate::project_semantic_dispatch::raise::project_node_publication_score_with_dispatch(
            &dispatch, current,
        );
    match (candidate_score, current_score) {
        (Some(candidate), Some(current)) => publication_score_improves(&candidate, &current),
        // A raisable candidate has a publishable shape; an unraisable current
        // has none — the candidate is the improvement.
        (Some(_), None) => true,
        // An unraisable candidate has no shape to publish, so it is NEVER an
        // improvement, regardless of the current's raisability.
        (None, _) => false,
    }
}

/// Node-domain mirror of the publication finaliser's
/// `root_is_explicit_selector_operator`: whether `raise(node)`'s ROOT is an
/// explicit consumer-demand selector (`IndexedAccess` / `keyof` / `typeof` / a
/// `Pick` / `Omit` / `Record` builtin-utility reference). Reads the carrier kind
/// directly (peeling `Alias`); a builtin-utility name is matched on the
/// reference's declaration name.
pub(crate) fn node_root_is_explicit_selector_operator(
    ctx: &dyn crate::resolver_core::ResolverContext,
    node: crate::semantic_query::SemanticNodeId,
) -> bool {
    use crate::semantic_query::SemanticNodeData;
    use verter_semantic::analysis::type_solver::builtin::BuiltinUtility;
    let graph = ctx.project_type_store().semantic_graph();
    let is_selector_util = |name: &str| {
        matches!(
            BuiltinUtility::from_name(name),
            Some(BuiltinUtility::Pick | BuiltinUtility::Omit | BuiltinUtility::Record)
        )
    };
    match graph.node_data(node).as_deref() {
        Some(SemanticNodeData::Alias(inner)) => {
            node_root_is_explicit_selector_operator(ctx, *inner)
        }
        Some(
            SemanticNodeData::IndexedAccess { .. }
            | SemanticNodeData::KeyOf { .. }
            | SemanticNodeData::TypeOf(_),
        ) => true,
        Some(SemanticNodeData::DeclRef { identity }) => {
            is_selector_util(identity.decl_name.as_ref())
        }
        Some(SemanticNodeData::InstantiationRef { base, .. }) => {
            is_selector_util(base.decl_name.as_ref())
        }
        Some(data) if data.bare_ref_head().is_some() => data
            .bare_ref_head()
            .is_some_and(|head| is_selector_util(head.0.as_ref())),
        _ => false,
    }
}

/// Whether `candidate` is a strictly BETTER published shape than `current`,
/// scored over the `TypeExpr` front of the SHARED publication formula. Reproduces
/// the historical symbolic-penalty / structural-top-level / generic-detail
/// comparison EXACTLY (it reads [`PublicationScore`] instead of three standalone
/// walks). Production picks between shapes in node domain
/// ([`compare_node_improvement`]); this `TypeExpr` front is retained as the
/// reference oracle the single-algebra parity differentials assert against
/// ([`compare_node_improvement`] must agree with it over the raised shapes), so
/// it is `#[cfg(test)]`-gated.
#[cfg(test)]
pub(crate) fn compare_type_expr_improvement(
    candidate: &verter_type_expr::TypeExpr,
    current: &verter_type_expr::TypeExpr,
) -> bool {
    let candidate_score =
        crate::project_semantic_dispatch::raise::type_expr_publication_score(candidate);
    let current_score =
        crate::project_semantic_dispatch::raise::type_expr_publication_score(current);
    publication_score_improves(&candidate_score, &current_score)
}

#[cfg(test)]
mod node_scoring_differential_tests {
    //! DIFFERENTIAL EQUIVALENCE: the node-domain scoring comparator equals the
    //! `TypeExpr` comparator (`compare_type_expr_improvement`) on Navigate-lowered
    //! nodes, exercising each scoring clause — current-Unknown, fewer symbolic
    //! carriers, structural-top-level, and a negative — plus the explicit-selector
    //! root predicate over its operator / builtin-utility kinds.

    use std::sync::Arc;

    use verter_type_expr::{PrimitiveName, TypeExpr};

    use super::{
        compare_node_improvement, compare_type_expr_improvement,
        node_root_is_explicit_selector_operator,
    };
    use crate::project_semantic_dispatch::ProjectSemanticDispatch;
    use crate::semantic_query::{ProjectionMode, SemanticNodeId};
    use crate::types::{AnalysisLevel, HostConfig};
    use crate::VerterHost;

    fn build_host() -> VerterHost {
        let ws = Arc::new(verter_workspace::MemoryWorkspace::new(
            verter_workspace::MemoryOptions::default(),
        ));
        ws.inject_file(
            "/src/m.ts".to_string(),
            Arc::from("export interface Foo { a: string; b: number }\n"),
        );
        let host = VerterHost::new(
            HostConfig {
                analysis_level: AnalysisLevel::Full,
                ..HostConfig::default()
            },
            ws,
        );
        assert!(host.ensure_loaded("/src/m.ts"));
        host
    }

    fn lower(host: &VerterHost, expr: &TypeExpr) -> SemanticNodeId {
        ProjectSemanticDispatch::new(host)
            .lower_type_expr_in_scope_with_mode("/src/m.ts", expr, ProjectionMode::Navigate)
            .expect("expr must lower")
    }

    #[test]
    fn compare_node_improvement_matches_type_expr_comparator_per_clause() {
        let host = build_host();
        let foo = || TypeExpr::named("Foo");
        let idx = || TypeExpr::IndexedAccess {
            object: Arc::new(foo()),
            index: Arc::new(TypeExpr::string_literal("a")),
        };
        // A structural array `string[]` (structural top-level, 0 symbolic carriers).
        let array = || TypeExpr::Array {
            element: Arc::new(TypeExpr::Primitive(PrimitiveName::String)),
            readonly: false,
        };

        // (candidate, current) pairs, one per scoring clause + a negative control.
        let pairs: Vec<(TypeExpr, TypeExpr)> = vec![
            // current-Unknown clause: a concrete string beats Unknown.
            (
                TypeExpr::Primitive(PrimitiveName::String),
                TypeExpr::Unknown { raw: String::new() },
            ),
            // fewer-symbolic-carriers clause: a bare `Foo` (1) beats `Pick<Foo,'a'>` (2).
            (
                foo(),
                TypeExpr::named_with_args("Pick", vec![foo(), TypeExpr::string_literal("a")]),
            ),
            // structural-top-level clause: a structural array beats a Ref carrier.
            (array(), foo()),
            // NEGATIVE control: an IndexedAccess does NOT beat a bare string.
            (idx(), TypeExpr::Primitive(PrimitiveName::String)),
        ];

        let mut saw_true = false;
        let mut saw_false = false;
        for (candidate, current) in &pairs {
            let cand_node = lower(&host, candidate);
            let cur_node = lower(&host, current);
            let node_verdict = compare_node_improvement(&host, cand_node, cur_node);
            let expr_verdict = compare_type_expr_improvement(candidate, current);
            assert_eq!(
                node_verdict, expr_verdict,
                "compare_node_improvement must equal compare_type_expr_improvement for \
                 candidate={candidate:?} current={current:?}"
            );
            if node_verdict {
                saw_true = true;
            } else {
                saw_false = true;
            }
        }
        assert!(
            saw_true && saw_false,
            "the differential must exercise BOTH a better and a not-better verdict (genuine reach)"
        );
    }

    #[test]
    fn compare_node_improvement_unraisable_candidate_is_never_an_improvement() {
        // SCORING-INVARIANT (None handling): an UNRAISABLE candidate has no
        // publishable shape (`project_node_publication_score == None`) and must
        // NEVER be scored as an improvement. Scoring it as
        // `PublicationScore::default()` ({symbolic_carriers: 0, …}) makes its zero
        // carriers beat any symbolic current (0 < current.symbolic_carriers),
        // wrongly preferring the shapeless candidate. The invariant is
        // `(None, _) => false`; the symmetric `(Some, None) => true`.
        let host = build_host();
        // A symbolic current: a bare `Foo` Ref raises to one symbolic carrier
        // (`symbolic_carriers == 1`), so the pre-fix default-score path returns
        // `true` here (the bug).
        let symbolic = lower(&host, &TypeExpr::named("Foo"));
        // An UNRAISABLE node: an absent graph id (`u64::MAX`) — its publication
        // score is `None` (no shape to publish).
        let unraisable = SemanticNodeId(u64::MAX);

        // (None, Some): an unraisable candidate is NOT an improvement over a
        // symbolic current. Pre-fix (`unwrap_or_default`) this returned `true`
        // because {0,…} < {1,…}.
        assert!(
            !compare_node_improvement(&host, unraisable, symbolic),
            "an unraisable candidate (publication score None) is never a publication \
             improvement over a symbolic current"
        );
        // (Some, None): a raisable symbolic candidate IS an improvement over an
        // unraisable (shapeless) current.
        assert!(
            compare_node_improvement(&host, symbolic, unraisable),
            "a raisable symbolic candidate improves over an unraisable (shapeless) current"
        );
    }

    #[test]
    fn node_root_explicit_selector_matches_expected_kinds() {
        let host = build_host();
        let foo = || TypeExpr::named("Foo");
        // selectors → true
        for expr in [
            TypeExpr::IndexedAccess {
                object: Arc::new(foo()),
                index: Arc::new(TypeExpr::string_literal("a")),
            },
            TypeExpr::KeyOf(Arc::new(foo())),
            TypeExpr::named_with_args("Pick", vec![foo(), TypeExpr::string_literal("a")]),
            TypeExpr::named_with_args("Omit", vec![foo(), TypeExpr::string_literal("a")]),
        ] {
            let node = lower(&host, &expr);
            assert!(
                node_root_is_explicit_selector_operator(&host, node),
                "{expr:?} is an explicit selector operator at its root"
            );
        }
        // non-selectors → false
        for expr in [foo(), TypeExpr::Primitive(PrimitiveName::String)] {
            let node = lower(&host, &expr);
            assert!(
                !node_root_is_explicit_selector_operator(&host, node),
                "{expr:?} is NOT an explicit selector operator"
            );
        }
    }
}
