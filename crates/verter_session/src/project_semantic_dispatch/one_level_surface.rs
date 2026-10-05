//! Graph-only one-level surface projection and the macro type-argument
//! surface replay.
//!
//! [`OneLevelSurface`] is the ordered declaration stream of a node's one-level
//! surface (members, call / construct signatures, index signatures) exactly as
//! the shared graph produced it — node ids, graph members and graph index
//! signatures, no spans paired with files, no JSDoc and no display text.
//! Span-rich publication surfaces are projections of it built above the
//! dispatch layer.
//!
//! [`project_one_level_surface`] is the ONE path-precise `Shallow` surface
//! demand shared by every surface consumer; [`project_macro_argument_surface`]
//! is the ONE producer of a Vue macro type argument's one-level surface. Both
//! publication (the framework-surface executor) and the semantic-source raise
//! replay (member-path, callable-occurrence and index-position sources) consume
//! that single producer, so the indexed-access decomposition, macro
//! provenance, root normalization, unresolved-root partiality, projection and
//! incomplete-arm folding are identical by construction — member identity,
//! callable subject identity and index-signature ordinals agree between the
//! publication and its replay.

use std::sync::Arc;

use verter_session_query::analysis::types::AnalyzedMacroKind;

use super::ProjectSemanticDispatch;
use crate::semantic_query::surface_resolution::{
    stable_member_carrier_partiality, stable_query_error_partiality, unresolved_node_partiality,
    NonEmptyReasons, SurfaceResolution,
};
use crate::semantic_query::{
    IndexSignature, PathSegment, ProjectionMode, ProjectionReductionContext, QueryResult,
    SemanticNodeData, SemanticNodeId, SemanticQueryKey, SurfaceEntry, SurfaceMember, SurfaceView,
};
use crate::semantic_query_memo::SemanticGraphStore;

/// A graph-only one-level surface: the resolver-owned ordered declaration
/// stream plus the keyspace / index-signature facts of the surface it was
/// read from.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct OneLevelSurface {
    entries: SurfaceEntries,
    keyspace: Option<SemanticNodeId>,
    has_index_signature: bool,
}

/// Backing of a [`OneLevelSurface`]'s entry stream: a graph view's stream is
/// SHARED (no copy); a surface synthesized here (a presence join, a spread
/// join) OWNS the one vector it was built into.
#[derive(Debug, Clone)]
enum SurfaceEntries {
    Shared(Arc<[SurfaceEntry]>),
    Owned(Vec<SurfaceEntry>),
}

impl SurfaceEntries {
    fn as_slice(&self) -> &[SurfaceEntry] {
        match self {
            Self::Shared(entries) => entries,
            Self::Owned(entries) => entries,
        }
    }
}

impl PartialEq for SurfaceEntries {
    fn eq(&self, other: &Self) -> bool {
        self.as_slice() == other.as_slice()
    }
}

impl Eq for SurfaceEntries {}

impl OneLevelSurface {
    /// The surface that declares nothing.
    pub(crate) fn empty() -> Self {
        Self {
            entries: SurfaceEntries::Owned(Vec::new()),
            keyspace: None,
            has_index_signature: false,
        }
    }

    /// The one-level surface a graph [`SurfaceView`] carries, verbatim.
    pub(crate) fn from_view(view: &SurfaceView) -> Self {
        Self {
            entries: SurfaceEntries::Shared(Arc::clone(&view.entries)),
            keyspace: view.keyspace,
            has_index_signature: view.has_known_index_signature(),
        }
    }

    /// A presence-only surface of POSITIVE named members gathered from an
    /// open carrier. Signatures / index facts are not recovered from open
    /// carriers.
    pub(crate) fn from_presence_members(members: Vec<SurfaceMember>) -> Self {
        Self {
            entries: SurfaceEntries::Owned(members.into_iter().map(SurfaceEntry::Member).collect()),
            keyspace: None,
            has_index_signature: false,
        }
    }

    /// The ordered declaration stream.
    pub(crate) fn entries(&self) -> &[SurfaceEntry] {
        self.entries.as_slice()
    }

    /// Named members in declaration order.
    pub(crate) fn members(&self) -> impl Iterator<Item = &SurfaceMember> {
        self.entries().iter().filter_map(|entry| match entry {
            SurfaceEntry::Member(member) => Some(member),
            _ => None,
        })
    }

    /// Call-signature nodes in declaration order.
    pub(crate) fn call_signatures(&self) -> impl Iterator<Item = SemanticNodeId> + '_ {
        self.entries().iter().filter_map(|entry| match entry {
            SurfaceEntry::CallSignature(node) => Some(*node),
            _ => None,
        })
    }

    /// Index signatures in declaration order.
    pub(crate) fn index_signatures(&self) -> impl Iterator<Item = &IndexSignature> {
        self.entries().iter().filter_map(|entry| match entry {
            SurfaceEntry::IndexSignature(signature) => Some(signature),
            _ => None,
        })
    }

    /// Keyspace node, when the surface is a mapped/keyspace carrier.
    pub(crate) fn keyspace(&self) -> Option<SemanticNodeId> {
        self.keyspace
    }

    /// Whether the surface has at least one index signature.
    pub(crate) fn has_index_signature(&self) -> bool {
        self.has_index_signature
    }

    /// Join a spread-program projection into one shallow surface.
    ///
    /// A single closed alternative yields the exact complete surface
    /// (`Resolved`) — the closed witness is the only completeness proof.
    /// Every other shape (open residuals, correlated multi-branch formulas)
    /// joins POSITIVE members only and returns `OpenPresence`: omission from
    /// the joined view is never absence evidence, and the joined view is
    /// publication output that never re-enters binary semantic relation. A
    /// closed witness whose view cannot be built is `Incomplete` — never an
    /// empty success.
    pub(crate) fn from_spread_projection(
        graph: &SemanticGraphStore,
        formula: &crate::semantic_query::ObjectProjectionFormula,
        evidence: &mut super::canonical_algebra::CanonicalEvidence,
    ) -> SurfaceResolution<Self> {
        use crate::semantic_query::{
            AuthoredPropertyKey, ObjectSignatureKind, PositiveKeyPresence, ProjectionEvidence,
            PropertyKey,
        };

        fn union_value(
            graph: &SemanticGraphStore,
            values: Vec<SemanticNodeId>,
            evidence: &mut super::canonical_algebra::CanonicalEvidence,
        ) -> SemanticNodeId {
            // Canonical construction: the joined per-alternative value union
            // routes through the one authority (recursive flatten +
            // structural dedup replace the hand-rolled one-level splice);
            // the evidence threads to the caller's disposition boundary.
            let composite = super::canonical_algebra::intern_ordered_union(
                graph,
                &values,
                verter_session_query::flow::policy::NullabilityPolicy::Strict,
            );
            evidence.absorb(composite.evidence);
            composite.node
        }

        let alternatives = formula.alternatives();
        if let [only] = alternatives {
            if let Some(closed) = only.closed() {
                let Some(view) = closed.to_closed_surface_view() else {
                    return SurfaceResolution::incomplete(NonEmptyReasons::of(
                        crate::semantic_query::PartialReason::SemanticQueryFault,
                    ));
                };
                return SurfaceResolution::resolved(Self::from_view(&view));
            }
        }

        struct MemberJoin {
            key: PropertyKey,
            present_in: usize,
            optional_somewhere: bool,
            values: Vec<SemanticNodeId>,
            readonly_all: bool,
            method_kind: Option<verter_type_expr::ObjectMethodKind>,
            has_implementation_body: bool,
        }

        let alternative_count = alternatives.len();
        let mut joins: Vec<MemberJoin> = Vec::new();
        let mut call_nodes: Vec<SemanticNodeId> = Vec::new();
        let mut construct_nodes: Vec<SemanticNodeId> = Vec::new();
        let mut index_joins: Vec<(usize, SemanticNodeId, Vec<SemanticNodeId>, bool)> = Vec::new();
        for alternative in alternatives {
            alternative.positive().visit(|fact| {
                // JS property identity: dual spellings of one property
                // join into ONE row (matching the walker and macro
                // merges), so a colliding second alternative unions the
                // value instead of publishing a mis-optionaled duplicate.
                let join = match joins
                    .iter_mut()
                    .find(|join| join.key.element_access_collides(fact.key()))
                {
                    Some(join) => join,
                    None => {
                        joins.push(MemberJoin {
                            key: fact.key().clone(),
                            present_in: 0,
                            optional_somewhere: false,
                            values: Vec::new(),
                            readonly_all: true,
                            method_kind: None,
                            has_implementation_body: false,
                        });
                        joins.last_mut().expect("just pushed")
                    }
                };
                join.present_in += 1;
                join.optional_somewhere |= fact.presence() == PositiveKeyPresence::Optional;
                if let ProjectionEvidence::Proven(value) = fact.value() {
                    if !join.values.contains(value) {
                        join.values.push(*value);
                    }
                }
                match fact.facets() {
                    ProjectionEvidence::Proven(facets) => {
                        join.readonly_all &= facets.readonly();
                        if join.method_kind.is_none() {
                            join.method_kind = facets.method_kind();
                            join.has_implementation_body = facets.has_implementation_body();
                        }
                    }
                    ProjectionEvidence::Indeterminate => join.readonly_all = false,
                }
            });
            for signature in alternative.signatures() {
                let bucket = match signature.kind() {
                    ObjectSignatureKind::Call => &mut call_nodes,
                    ObjectSignatureKind::Construct => &mut construct_nodes,
                };
                if !bucket.contains(&signature.node()) {
                    bucket.push(signature.node());
                }
            }
            for index in alternative.indices() {
                let domain = index.domain() as usize;
                let entry = match index_joins.iter_mut().find(|(d, ..)| *d == domain) {
                    Some(entry) => entry,
                    None => {
                        index_joins.push((domain, index.key_type(), Vec::new(), true));
                        index_joins.last_mut().expect("just pushed")
                    }
                };
                if let ProjectionEvidence::Proven(value) = index.value() {
                    if !entry.2.contains(value) {
                        entry.2.push(*value);
                    }
                }
                entry.3 &= matches!(index.readonly(), ProjectionEvidence::Proven(true));
            }
        }

        // ONE entry stream, built in declaration-kind order: members, call
        // signatures, construct signatures, index signatures. The member
        // value unions intern before the index value unions, as before.
        let index_count = index_joins
            .iter()
            .filter(|(.., values, _)| !values.is_empty())
            .count();
        let mut entries: Vec<SurfaceEntry> = Vec::with_capacity(
            joins.len() + call_nodes.len() + construct_nodes.len() + index_count,
        );
        for join in joins {
            // A definitely-present key whose value is Indeterminate in
            // EVERY alternative (an open residual may overwrite it)
            // still publishes its row — with the honest open value,
            // matching the walker's `Opaque(OpenSurface)` convention —
            // never a dropped row.
            let value = if join.values.is_empty() {
                graph.intern_node(SemanticNodeData::Opaque(
                    crate::semantic_query::QueryError::OpenSurface,
                ))
            } else {
                union_value(graph, join.values, evidence)
            };
            entries.push(SurfaceEntry::Member(SurfaceMember {
                key: AuthoredPropertyKey::from_known(join.key),
                value,
                optional: join.optional_somewhere || join.present_in < alternative_count,
                readonly: join.readonly_all,
                method_kind: join.method_kind,
                has_implementation_body: join.has_implementation_body,
                visibility: verter_type_expr::MemberVisibility::Public,
                spans: verter_type_expr::MemberSpans::default(),
                declaration_origin: None,
                declared_in_macro_type_arg: crate::semantic_query::MacroOwnBodyStamp::NEUTRAL,
                merge_role: crate::semantic_query::MergeRoleStamp::NEUTRAL,
                excess_origin: verter_type_expr::ExcessPropertyOrigin::NonLiteral,
            }));
        }
        entries.extend(call_nodes.into_iter().map(SurfaceEntry::CallSignature));
        entries.extend(
            construct_nodes
                .into_iter()
                .map(SurfaceEntry::ConstructSignature),
        );
        for (_, key_type, values, readonly) in index_joins {
            if values.is_empty() {
                continue;
            }
            entries.push(SurfaceEntry::IndexSignature(IndexSignature {
                key_type,
                value_type: union_value(graph, values, evidence),
                readonly,
                spans: verter_type_expr::IndexSignatureSpans::default(),
                declaration_origin: None,
            }));
        }
        SurfaceResolution::open_presence(Self {
            entries: SurfaceEntries::Owned(entries),
            keyspace: None,
            has_index_signature: index_count > 0,
        })
    }
}

/// Project `base` to a pure graph-backed one-level surface.
///
/// This is the ownership boundary shared by every one-level surface consumer
/// (the framework-surface executors, compile-oriented TypeInfo projection and
/// component-meta's native visibility projection). It runs exactly one
/// path-precise `Shallow` demand and performs no source reads, JSDoc
/// hydration, display rendering, or member-body expansion.
///
/// `path` is the path-precise selector applied to `base` BEFORE the one-level
/// surface synthesis. Most callers pass the empty path (the base IS the
/// surface root). A deep indexed-access macro type argument
/// (`defineProps<DeepConfig['ui']['header']>()`) passes a non-empty path: the
/// shared `ProjectPath` walker runs the intermediate hops (`['ui']`) in
/// `Navigate` and the TERMINAL hop (`['header']`) in the caller's mode, so the
/// leaf object's members surface without the intermediate siblings leaking —
/// the path-precise rule.
///
/// `context` is the `ProjectPath` reduction context; `mode` MUST stay
/// `Shallow` so the surface is one-level (member values stay reference-style).
///
/// `walker_diagnostics`, when supplied, receives the shallow walker's
/// side-band diagnostics for this projection (cycle short-circuits,
/// unresolved surface arms, …) — replayed transparently on warm memo reads.
pub(crate) fn project_one_level_surface<C: crate::resolver_core::ResolverCapabilities>(
    ctx: &dyn crate::resolver_core::ResolverContext<C>,
    dispatch: &ProjectSemanticDispatch<'_, C>,
    base: SemanticNodeId,
    path: Arc<[PathSegment]>,
    context: ProjectionReductionContext,
    walker_diagnostics: Option<&mut Vec<super::walk::ShallowDiagnostic>>,
) -> SurfaceResolution<OneLevelSurface> {
    verter_debug_assert_eq!(
        context.mode,
        ProjectionMode::Shallow,
        "project_one_level_surface synthesises a one-level surface; mode must be Shallow"
    );
    // Path-precise `Shallow` projection synthesises the one-level surface
    // (call / construct / index signatures + merged members) without
    // expanding member bodies. An empty `path` projects `base`'s own
    // one-level surface; a non-empty `path` walks the selector hops first
    // (intermediate hops `Navigate`, terminal in the caller's mode) and
    // synthesises the LEAF's surface. This path PRESERVES call / construct
    // signatures, so an emit interface's call signatures survive here (the
    // emit normalizer reads them). `execute_read` (NOT `execute_type_node`)
    // keeps the walker's side-band diagnostics on hand for the sink; it
    // does not record dispatch-intent counters itself, so record them
    // here — this surface synthesis stays visible to the projection-op
    // budget fuse exactly as it was through `execute_type_node`.
    let graph = dispatch.graph();
    // A spread-bearing object is a construction program, not a surface:
    // the empty-path Shallow terminal now projects program roots through
    // the correlated spread query (walker's
    // `program_root_shallow_surface`), but its `SurfaceView` output is
    // closed-by-construction and cannot carry an openness witness. This
    // projection keeps its own spread join so an open / multi-branch
    // formula resolves through the presence-only OPEN arm; only a
    // single closed alternative may claim the complete `Resolved` arm.
    // An UNBOUND generic at the surface ROOT (`<script setup generic="T">
    // defineProps<T>()`) is an OPEN member domain, not an empty one. The
    // shared walker synthesises a CLOSED empty object for a bare
    // `TypeParam` subject, which would make the generic component
    // byte-identical to a props-less one — so the open domain is handled
    // HERE, before the walk: the constraint's closed part is the presence
    // lower bound (`T extends { a: number }` publishes `a`), and an
    // unconstrained parameter publishes the empty presence floor.
    // Complete-as-a-RESULT and warm-capable — never a reason-free empty
    // success, and never a false partial.
    if path.is_empty() {
        if let Some(SemanticNodeData::TypeParam { constraint, .. }) =
            graph.node_data(base).as_deref()
        {
            let constraint = *constraint;
            return match constraint {
                Some(constraint) => project_one_level_surface(
                    ctx,
                    dispatch,
                    constraint,
                    Arc::from(Vec::<PathSegment>::new().into_boxed_slice()),
                    context,
                    walker_diagnostics,
                )
                .into_open_presence(),
                None => SurfaceResolution::open_presence(OneLevelSurface::empty()),
            };
        }
    }
    let spread_base = if path.is_empty()
        && matches!(
            graph.node_data(base).as_deref(),
            Some(SemanticNodeData::ObjectSpreadProgram(_))
        ) {
        Some(base)
    } else {
        None
    };
    let (terminal, read_partiality) = match spread_base {
        Some(base) => (base, None),
        None => {
            let key = SemanticQueryKey::ProjectPath {
                base,
                path,
                context,
            };
            dispatch.record_dispatch_intent_counters(&key);
            let surface_read = dispatch.execute_read(key);
            // Mirror `partial_reason_classes`: a partial read whose
            // producer captured no specific class is a downstream
            // PROPAGATED partial — stated here, at the one conversion
            // site, never normalized inside the claim type.
            let read_partiality = if surface_read.result_is_partial {
                Some(
                    NonEmptyReasons::new(surface_read.partial_reasons).unwrap_or_else(|| {
                        NonEmptyReasons::of(crate::semantic_query::PartialReason::Propagated)
                    }),
                )
            } else {
                None
            };
            if let Some(sink) = walker_diagnostics {
                sink.extend(surface_read.walker_diagnostics.iter().cloned());
            }
            match surface_read.value {
                QueryResult::Value(node) => (node, read_partiality),
                QueryResult::Recursive(node) => (
                    node,
                    Some(match read_partiality {
                        Some(reasons) => reasons
                            .with(crate::semantic_query::PartialReasonSet::SAME_PATH_RECURSION),
                        None => NonEmptyReasons::of(
                            crate::semantic_query::PartialReason::SamePathRecursion,
                        ),
                    }),
                ),
                QueryResult::Error(error) => {
                    return SurfaceResolution::incomplete(NonEmptyReasons::from_query_error(
                        &error,
                    ));
                }
            }
        }
    };

    let node_data = graph.node_data(terminal);
    let resolution = match node_data.as_deref() {
        // A partial terminal read keeps its positive members as a usable
        // subset but is INCOMPLETE with the read's typed reasons:
        // omission is not absence evidence, and the subset never passes
        // as the complete surface.
        Some(SemanticNodeData::Object(view)) => {
            SurfaceResolution::resolved(OneLevelSurface::from_view(view))
        }
        // Open carrier terminal (the walker's open-safe policy returns
        // the compound node when any nested open program contributed):
        // recurse the branches with the shared presence-only read —
        // positive members through the open-presence arm; a branch whose
        // node is an UNRESOLVED carrier makes the join incomplete.
        Some(
            SemanticNodeData::Union(_)
            | SemanticNodeData::Intersection(_)
            | SemanticNodeData::Conditional { .. },
        ) => read_positive_surface_members(ctx, dispatch, terminal)
            .map(OneLevelSurface::from_presence_members)
            .into_open_presence(),
        Some(SemanticNodeData::ObjectSpreadProgram(_)) => {
            let formula = match dispatch.project_object_spread_for_consumer(
                terminal,
                crate::semantic_query::ObjectProjectionSelector::Surface,
                context,
            ) {
                QueryResult::Value(formula) => formula,
                QueryResult::Recursive(_) => {
                    return SurfaceResolution::incomplete(NonEmptyReasons::of(
                        crate::semantic_query::PartialReason::SamePathRecursion,
                    ));
                }
                QueryResult::Error(error) => {
                    return SurfaceResolution::incomplete(NonEmptyReasons::from_query_error(
                        &error,
                    ));
                }
            };
            let mut canonical_evidence = super::canonical_algebra::CanonicalEvidence::default();
            let surface =
                OneLevelSurface::from_spread_projection(graph, &formula, &mut canonical_evidence);
            dispatch.deposit_canonical_evidence(canonical_evidence);
            surface
        }
        // An UNBOUND generic at the surface ROOT (`<script setup
        // generic="T"> defineProps<T>()`) is an OPEN member domain, not an
        // empty one: the constraint's closed part is the presence lower
        // bound (`T extends { a: number }` publishes `a`), and an
        // unconstrained parameter publishes the empty presence floor.
        // Complete-as-a-RESULT and warm-capable — never a reason-free
        // "no such surface" that makes the generic component
        // byte-identical to a props-less one, and never a false partial.
        Some(SemanticNodeData::TypeParam { constraint, .. }) => {
            let constraint = *constraint;
            match constraint {
                Some(constraint) => project_one_level_surface(
                    ctx,
                    dispatch,
                    constraint,
                    Arc::from(Vec::<PathSegment>::new().into_boxed_slice()),
                    context,
                    None,
                )
                .into_open_presence(),
                None => SurfaceResolution::open_presence(OneLevelSurface::empty()),
            }
        }
        // The terminal is an UNRESOLVED carrier (a missed hop's
        // `Opaque(Miss)`, an unresolved import's `BareRef`, a raw
        // fallback, a missing arena node): the resolution could not
        // produce the demanded surface and names why — never an empty
        // success. Any other shape (a primitive / union-free scalar /
        // function) genuinely has no one-level object surface.
        other => match unresolved_node_partiality(other) {
            Some(reasons) => SurfaceResolution::incomplete(reasons),
            None => SurfaceResolution::no_surface(),
        },
    };
    // EVERY arm folds the read's typed partiality into its returned
    // claim, with ONE discrimination on the OPEN arm: the walker's
    // open-program flag rides the read as the class-less `PROPAGATED`
    // bridge — open EVIDENCE, not an operational failure. The
    // `OpenPresence` claim itself carries that openness (omission is
    // not absence evidence), and the read rails still carry the flag
    // to the request scope, so a pure-`PROPAGATED` partial keeps the
    // presence-only claim. Any CLASSED partial (budget / missing
    // dependency / cancellation / recursion / …) demotes the claim on
    // every arm — a producer that observed a classed partial read can
    // never hand onward a reason-free complete/warm claim.
    let read_partiality = match (&resolution, read_partiality) {
        (SurfaceResolution::OpenPresence(_), Some(reasons)) => NonEmptyReasons::new(
            reasons
                .get()
                .without(crate::semantic_query::PartialReasonSet::PROPAGATED),
        ),
        (_, other) => other,
    };
    resolution.with_read_partiality(read_partiality)
}

/// Read the positive member evidence backing `node`, if `node` resolves to a
/// `SemanticNodeData::Object` shell. An empty result does not prove that an
/// open-spread surface has no additional members, and an UNRESOLVED carrier
/// (an import miss, an opaque shell) is an INCOMPLETE resolution with its
/// typed reason — never a silent zero-member success.
///
/// An `ObjectSpreadProgram` node is the walker's typed open evidence for an
/// open / multi-alternative program root (it never fabricates a closed
/// `Object` for those): publish its POSITIVE member evidence through the
/// correlated spread query — presence only, never completeness, exactly
/// this reader's standing contract.
/// `Union` / `Intersection` / `Conditional` carriers (the walker's typed
/// open evidence for compound roots containing open programs) recurse
/// per-branch with the SAME presence-only rule and merge under the macro
/// enumeration convention (a member present in any branch is published).
pub(crate) fn read_positive_surface_members<C: crate::resolver_core::ResolverCapabilities>(
    ctx: &dyn crate::resolver_core::ResolverContext<C>,
    dispatch: &ProjectSemanticDispatch<'_, C>,
    surface_node: SemanticNodeId,
) -> SurfaceResolution<Vec<SurfaceMember>> {
    /// Join per-arm resolutions under the given member-join rule: the joined
    /// positive members always publish, and ANY incomplete arm makes the
    /// whole join incomplete with the union of the arms' typed reasons — the
    /// joined subset is usable but never passes as the complete evidence.
    fn join_arms<C: crate::resolver_core::ResolverCapabilities>(
        dispatch: &ProjectSemanticDispatch<'_, C>,
        arms: &[SurfaceResolution<Vec<SurfaceMember>>],
        join: impl FnOnce(
            &SemanticGraphStore,
            &[Vec<SurfaceMember>],
            &mut super::canonical_algebra::CanonicalEvidence,
        ) -> Vec<SurfaceMember>,
    ) -> SurfaceResolution<Vec<SurfaceMember>> {
        let mut incomplete = None::<NonEmptyReasons>;
        let mut per_arm: Vec<Vec<SurfaceMember>> = Vec::with_capacity(arms.len());
        for arm in arms {
            match arm {
                SurfaceResolution::Resolved(members) | SurfaceResolution::OpenPresence(members) => {
                    per_arm.push((**members).clone())
                }
                SurfaceResolution::NoSurface(_) => per_arm.push(Vec::new()),
                SurfaceResolution::Incomplete(inc) => {
                    incomplete = Some(match incomplete {
                        Some(acc) => acc.union(inc.non_empty_reasons()),
                        None => inc.non_empty_reasons(),
                    });
                    per_arm.push(Vec::new());
                }
            }
        }
        let mut canonical_evidence = super::canonical_algebra::CanonicalEvidence::default();
        let members = join(dispatch.graph(), &per_arm, &mut canonical_evidence);
        dispatch.deposit_canonical_evidence(canonical_evidence);
        match incomplete {
            Some(reasons) => SurfaceResolution::incomplete_with(reasons, members),
            None => SurfaceResolution::resolved(members),
        }
    }

    match super::node_data_for(dispatch.graph(), surface_node).as_deref() {
        Some(SemanticNodeData::Object(view)) => {
            SurfaceResolution::resolved(view.positive_members().to_vec())
        }
        Some(SemanticNodeData::ObjectSpreadProgram(_)) => {
            let formula = match dispatch.project_object_spread_for_consumer(
                surface_node,
                crate::semantic_query::ObjectProjectionSelector::Surface,
                ProjectionReductionContext::published(ProjectionMode::Shallow),
            ) {
                QueryResult::Value(formula) => formula,
                QueryResult::Recursive(_) => {
                    return SurfaceResolution::incomplete(NonEmptyReasons::of(
                        crate::semantic_query::PartialReason::SamePathRecursion,
                    ));
                }
                // A stable / well-formed open marker contributes no members
                // and stays COMPLETE; only an operational fault is partial.
                QueryResult::Error(error) => {
                    return match stable_query_error_partiality(&error) {
                        Some(reasons) => SurfaceResolution::incomplete(reasons),
                        None => SurfaceResolution::resolved(Vec::new()),
                    };
                }
            };
            let mut canonical_evidence = super::canonical_algebra::CanonicalEvidence::default();
            let members = super::walk::spread_formula_positive_members_for_macro(
                dispatch.graph(),
                &formula,
                &mut canonical_evidence,
            );
            dispatch.deposit_canonical_evidence(canonical_evidence);
            SurfaceResolution::resolved(members)
        }
        Some(SemanticNodeData::Union(arms)) => {
            let arms = arms.members_arc();
            let per_arm: Vec<_> = arms
                .iter()
                .map(|arm| read_positive_surface_members(ctx, dispatch, *arm))
                .collect();
            join_arms(dispatch, &per_arm, |graph, per_arm, evidence| {
                super::walk::presence_union_members(graph, per_arm, evidence)
            })
        }
        // Intersection carriers merge under INTERSECTION rules (required
        // in any declaring arm stays required, same-key collisions
        // intersect the values, readonly in any arm survives) — the union
        // rule would mark intersection members optional and union their
        // values.
        Some(SemanticNodeData::Intersection(arms)) => {
            let arms = arms.members_arc();
            let per_arm: Vec<_> = arms
                .iter()
                .map(|arm| read_positive_surface_members(ctx, dispatch, *arm))
                .collect();
            join_arms(dispatch, &per_arm, |graph, per_arm, evidence| {
                super::walk::presence_intersection_members(graph, per_arm, evidence)
            })
        }
        Some(SemanticNodeData::Conditional {
            true_branch_ref,
            false_branch_ref,
            pending,
            ..
        }) => {
            let true_branch = dispatch.apply_conditional_branch_pending(
                *true_branch_ref,
                pending.as_deref(),
                true,
            );
            let false_branch = dispatch.apply_conditional_branch_pending(
                *false_branch_ref,
                pending.as_deref(),
                false,
            );
            let per_arm = [
                read_positive_surface_members(ctx, dispatch, true_branch),
                read_positive_surface_members(ctx, dispatch, false_branch),
            ];
            join_arms(dispatch, &per_arm, |graph, per_arm, evidence| {
                super::walk::presence_union_members(graph, per_arm, evidence)
            })
        }
        // An OPERATIONALLY unresolved carrier contributes no members AND
        // says so — the typed reason makes `Known | Missing` distinguishable
        // from `Known`. A member-less resolved shape (a primitive / a
        // function) and a STABLE authored-miss carrier (a `BareRef` mirror,
        // the walker's well-formed `OpenSurface` marker) genuinely
        // contribute no positive member evidence — the complete answer.
        other => match stable_member_carrier_partiality(ctx, other) {
            Some(reasons) => SurfaceResolution::incomplete(reasons),
            None => SurfaceResolution::resolved(Vec::new()),
        },
    }
}

/// One unresolvable SURFACE-COMPOSITION reference arm dropped during macro
/// surface synthesis: the arm's head name plus the canonical file whose
/// declaration authored it (the file whose import bindings classify the miss
/// as import-backed vs ambient).
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct UnresolvedSurfaceArm {
    /// The reference's head name as written (`NotFound` in `extends NotFound`).
    pub(crate) name: Arc<str>,
    /// Canonical id of the file whose declaration authored the arm.
    pub(crate) owner_canonical: Arc<str>,
    /// Exact top-level lexical owner that authored the arm.
    pub(crate) owner: verter_type_expr::TopLevelOwnerId,
}

/// One macro's graph-only one-level surface, as the macro surface producer
/// projected it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct MacroArgumentSurface {
    /// The macro's one-level surface (the type argument's members / call
    /// signatures / index signatures, or the analyzer-fact runtime-object
    /// surface).
    pub(crate) surface: OneLevelSurface,
    /// SFC-absolute span of the macro CALL (from the analyzer fact).
    pub(crate) macro_call_span: verter_span::Span,
    /// SURFACE-COMPOSITION reference arms (heritage `extends` parents,
    /// intersection / union arms) the shallow walker dropped as unresolvable
    /// while synthesising `surface` — name-sorted, deduplicated.
    pub(crate) unresolved_surface_arms: Vec<UnresolvedSurfaceArm>,
}

/// The typed partiality an UNRESOLVABLE macro-type-argument ROOT contributes.
///
/// `None` means the root resolved to a real surface the caller can project.
/// Every `Some` is the resolver's OWN proof that the authored root could not
/// be reached, so the surface the producer would publish is a SUBSET of what
/// the author wrote — the case an empty published surface must never be
/// confused with:
///
/// - an authored reference Navigate preserved as a carrier (`BareRef` /
///   `ImportType`) — the declaration owner did not resolve, which is exactly
///   the imported-dependency class the taxonomy already names;
/// - a body the lowerer kept as raw text (`RawFallback`) — no structural
///   surface was produced;
/// - a typed failure carrier (`Opaque`), which names its own class through the
///   shared query-error classification;
/// - no interned node at all under the resolved id.
fn unresolved_macro_root_partiality(
    base: Option<&SemanticNodeData>,
) -> Option<crate::semantic_query::PartialReasonSet> {
    use crate::semantic_query::PartialReasonSet;

    match base {
        None => Some(PartialReasonSet::MISSING_SEMANTIC_NODE_DATA),
        Some(SemanticNodeData::BareRef(_) | SemanticNodeData::ImportType(_)) => {
            Some(PartialReasonSet::MISSING_DEPENDENCY)
        }
        Some(SemanticNodeData::RawFallback { .. }) => Some(PartialReasonSet::SEMANTIC_QUERY_FAULT),
        Some(SemanticNodeData::Opaque(error)) => {
            Some(super::symbol_identity::query_error_partial_reasons(error))
        }
        Some(_) => None,
    }
}

/// Extract the unresolved SURFACE-COMPOSITION arm facts from a projection's
/// walker diagnostics, name-sorted (then by declaring file) and deduplicated
/// so consumers emit deterministically ordered reports.
fn unresolved_surface_arms_from_diags(
    diags: &[super::walk::ShallowDiagnostic],
) -> Vec<UnresolvedSurfaceArm> {
    let mut arms: Vec<UnresolvedSurfaceArm> = diags
        .iter()
        .filter_map(|diag| match diag {
            super::walk::ShallowDiagnostic::UnresolvedSurfaceArm {
                name,
                owner_canonical,
                owner,
            } => Some(UnresolvedSurfaceArm {
                name: Arc::clone(name),
                owner_canonical: Arc::clone(owner_canonical),
                owner: *owner,
            }),
            _ => None,
        })
        .collect();
    arms.sort_by(|a, b| {
        a.name
            .cmp(&b.name)
            .then_with(|| a.owner_canonical.cmp(&b.owner_canonical))
            .then_with(|| a.owner.cmp(&b.owner))
    });
    arms.dedup();
    arms
}

/// Project ONE Vue macro's one-level surface — the single producer shared by
/// the framework-surface publication and the semantic-source raise replay.
///
/// `macro_kind` is the kind the surface is requested for. A runtime-object
/// macro (no type argument) synthesizes its surface from the analyzer's
/// already-typed member facts; `defineModel` publishes the empty surface (its
/// type argument is the model VALUE type); a type-based macro reads the
/// macro argument's ONE mode-neutral mirror handle, decomposes its
/// indexed-access structure, resolves the carrier base one `Navigate` hop,
/// and projects the terminal hop's one-level surface under the macro's
/// provenance. An unresolvable root or SURFACE-COMPOSITION arm records its
/// typed partiality into the active cold-compute completeness scope.
pub(crate) fn project_macro_argument_surface<C: crate::resolver_core::ResolverCapabilities>(
    ctx: &dyn crate::resolver_core::ResolverContext<C>,
    dispatch: &ProjectSemanticDispatch<'_, C>,
    owner_canonical: &Arc<str>,
    macro_index: usize,
    macro_kind: AnalyzedMacroKind,
) -> Option<MacroArgumentSurface> {
    use crate::semantic_query::SurfaceProvenanceContext;

    // Structurally read-only: surface resolution runs inside the
    // DTO producer's traced scope; fenced-ness gates admission there.
    let indexed = ctx
        .ensure_indexed_ready_serve(owner_canonical.as_ref())?
        .indexed;
    let mac = indexed.snapshot.macros.get(macro_index)?;
    if !mac.is_type_based {
        // A runtime-object `defineExpose({...})` / `defineProps({...})`
        // has no type argument to lower, but the analyzer already
        // captured its member names (`mac.expose_fields` /
        // `mac.prop_fields` are populated for BOTH type-based and
        // runtime macro forms). Synthesize a one-level surface directly
        // from those already-typed facts instead of reporting an empty
        // surface for every runtime-declared macro. See
        // `runtime_object_macro_surface` for the structural (not
        // name-string) kind dispatch and the honest-unknown member
        // value policy.
        return runtime_object_macro_surface(ctx, dispatch, owner_canonical, macro_kind, mac);
    }

    // `defineModel` does NOT carry a props OBJECT type argument — its type
    // argument is the model VALUE type (`defineModel<string>()`), which has
    // no one-level member surface. Its props come from the analyzer-
    // synthesized model prop (`AnalyzedMacro.prop_fields`), so the macro
    // surface is the EMPTY object surface.
    if macro_kind == AnalyzedMacroKind::DefineModel {
        return Some(MacroArgumentSurface {
            surface: OneLevelSurface::empty(),
            macro_call_span: mac.span,
            // The empty model surface projects nothing — no arms.
            unresolved_surface_arms: Vec::new(),
        });
    }

    let _ = mac.parsed_type_argument.as_ref()?;

    // Provenance per macro axis. Props request the macro-T own-body
    // provenance on the terminal surface synthesis so the author-declared
    // members are flagged; emits / slots / exposed are structural
    // (`declared_in_macro_type_arg` is a props-axis concern). The terminal
    // `MacroTypeArgOwnBody` synthesis restamps `declared_in_macro_type_arg =
    // true` for EXACTLY the declaration's own-body direct members — it reads
    // the prepared decl's `member_index`, which is populated from direct
    // Object members only and SKIPS heritage `extends` `Ref` arms
    // (`build.rs::overlay_macro_type_arg_own_body`). Heritage-reached members
    // are NOT in `member_index`, so they are left at the structural `false`
    // the empty-path Shallow body lowering assigned. The surface's
    // `merge_role` is independently baked per arm (`Heritage` for
    // `extends`-reached members, `OwnBody` for the declaration's own body).
    // Only `DefineProps` reaches here as a props macro: `WithDefaults` is
    // never `is_type_based` (it bailed at the guard above) and `DefineModel`
    // returned its empty surface above. `DefineProps` requests the macro-T
    // own-body provenance; emits / slots / exposed are structural.
    let terminal_context = match macro_kind {
        AnalyzedMacroKind::DefineProps => ProjectionReductionContext::macro_object_surface(
            ProjectionMode::Shallow,
            SurfaceProvenanceContext::MacroTypeArgOwnBody,
        ),
        _ => ProjectionReductionContext::macro_object_surface(
            ProjectionMode::Shallow,
            SurfaceProvenanceContext::Structural,
        ),
    };

    // Dispatch is bound to the active `ctx`: an overlay session threads its
    // session view through every dispatch-tier read, so the type-argument
    // lowering and cross-file carrier projection below read overlay content.

    // Read the macro arg's mode-neutral mirror handle (the ONE producer),
    // then decompose its indexed-access structure GRAPH-NATIVE. A deep
    // indexed-access type argument (`defineProps<DeepConfig['ui']['header']>()`)
    // lowered to nested `IndexedAccess` carrier shells; this walks those
    // shells into `(base_node, path)` WITHOUT lowering the base a second
    // time — the base node IS a different DEMAND on the same handle. The
    // shared path walker runs intermediate hops in `Navigate` and the
    // TERMINAL hop under `terminal_context` (Shallow). A non-indexed type
    // argument decomposes to `(handle_node, [])`.
    let product = dispatch.macro_type_arg_hot_ref(owner_canonical.as_ref(), macro_index)?;
    let (base_carrier, path) = decompose_indexed_access_chain_node(dispatch, product.hot.node());
    // Resolve the carrier base ONE Navigate hop through the shared dispatch
    // (carrier head resolution — a `BareRef` head routes to its `DeclRef`,
    // a `TypeOf` shell executes its value root, member values stay shallow),
    // reproducing the eager structural-transit-Navigate base lowering. The
    // path-precise `Shallow` projection then synthesises the one-level
    // surface of the terminal hop.
    let base = dispatch.resolve_hot_handle_with_context(
        crate::semantic_query::HotTypeRef::new(base_carrier),
        ProjectionReductionContext::structural_transit_with_mode(ProjectionMode::Navigate),
    );
    // Navigate preserves an authored reference as a carrier when its
    // declaration cannot currently be resolved. That carrier is the
    // resolver-owned proof that the root surface is unavailable, not an
    // authoritative empty slot surface.
    //
    // Dropping the surface here publishes an EMPTY macro-DTO bundle, which
    // is byte-identical to the bundle a component that authored no macro
    // type at all publishes. So the drop RECORDS its typed partiality into
    // the producer's cold-compute completeness scope: the empty bundle is
    // returned to the caller as PARTIAL, is refused surface-store
    // admission, and reaches the published payload as a degraded surface
    // instead of a wrong-complete silence.
    let unresolved_root =
        unresolved_macro_root_partiality(super::node_data_for(dispatch.graph(), base).as_deref());
    if let Some(reasons) = unresolved_root {
        crate::request_context::fold_result_completeness(
            crate::semantic_query::ResultCompleteness::partial(reasons),
        );
        return None;
    }

    // Collect the walker's side-band diagnostics so unresolvable
    // SURFACE-COMPOSITION arms (heritage / intersection / union) the
    // shallow synthesis dropped ride the resolved surface to the
    // compile-facing collector.
    let mut walker_diagnostics = Vec::new();
    // The terminal-hop discharge: a projection that could not produce the
    // demanded surface (a MISSED terminal hop on a deep indexed access, an
    // unresolved carrier mid-path) records its typed partiality — the
    // empty macro-DTO bundle the caller then publishes is PARTIAL, refused
    // surface-store admission, and reaches the payload as a degraded
    // surface instead of a wrong-complete silence. A genuinely non-object
    // terminal stays the complete "no surface" miss.
    let surface = project_one_level_surface(
        ctx,
        dispatch,
        base,
        path,
        terminal_context,
        Some(&mut walker_diagnostics),
    )
    .recorded()?;
    let unresolved_surface_arms = unresolved_surface_arms_from_diags(&walker_diagnostics);
    // An unresolvable SURFACE-COMPOSITION arm (a heritage / intersection /
    // union contributor the shallow synthesis dropped) makes the published
    // METADATA surface partial: the members behind that arm are missing,
    // so the surface must never publish (or warm) as the complete answer.
    // The codegen lanes keep their own diagnostic channel for these arms.
    if !unresolved_surface_arms.is_empty() {
        crate::request_context::fold_result_completeness(
            crate::semantic_query::ResultCompleteness::partial(
                crate::semantic_query::PartialReasonSet::MISSING_DEPENDENCY,
            ),
        );
    }
    Some(MacroArgumentSurface {
        surface,
        macro_call_span: mac.span,
        unresolved_surface_arms,
    })
}

/// Intern (or reuse the interned) `unknown` primitive node — the honest
/// placeholder value for a runtime-object macro member whose real type is
/// not re-derived by [`runtime_object_macro_surface`]. Content-addressed like
/// every other `intern_node` call: repeated calls across members / requests
/// collapse onto the same node.
fn unknown_member_value_node<C: crate::resolver_core::ResolverCapabilities>(
    dispatch: &ProjectSemanticDispatch<'_, C>,
) -> SemanticNodeId {
    dispatch.graph().intern_node(SemanticNodeData::Primitive(
        crate::semantic_query::PrimitiveKind::Unknown,
    ))
}

fn runtime_object_macro_surface<C: crate::resolver_core::ResolverCapabilities>(
    ctx: &dyn crate::resolver_core::ResolverContext<C>,
    dispatch: &ProjectSemanticDispatch<'_, C>,
    owner_canonical: &Arc<str>,
    macro_kind: AnalyzedMacroKind,
    mac: &verter_session_query::analysis::types::AnalyzedMacro,
) -> Option<MacroArgumentSurface> {
    use crate::semantic_query::{AuthoredPropertyKey, MacroOwnBodyStamp, MergeRoleStamp};

    let declaration_origin = Some(Arc::clone(owner_canonical));
    let mut entries: Vec<SurfaceEntry> = Vec::new();
    match macro_kind {
        AnalyzedMacroKind::DefineExpose => {
            for field in &mac.expose_fields {
                entries.push(SurfaceEntry::Member(SurfaceMember {
                    key: AuthoredPropertyKey::String(Arc::from(field.name.as_str())),
                    // The exposed value's real type is not re-derived
                    // here (see the doc comment above) — `unknown` is
                    // the honest placeholder, never a fabricated shape.
                    value: unknown_member_value_node(dispatch),
                    optional: false,
                    readonly: false,
                    method_kind: None,
                    has_implementation_body: false,
                    visibility: verter_type_expr::MemberVisibility::Public,
                    spans: verter_type_expr::MemberSpans::name_only(
                        field.span.unwrap_or(verter_span::Span::new(0, 0)),
                    ),
                    declaration_origin: declaration_origin.clone(),
                    declared_in_macro_type_arg: MacroOwnBodyStamp::NEUTRAL,
                    merge_role: MergeRoleStamp::NEUTRAL,
                    excess_origin: verter_type_expr::ExcessPropertyOrigin::NonLiteral,
                }));
            }
        }
        AnalyzedMacroKind::DefineProps => {
            for field in &mac.prop_fields {
                entries.push(SurfaceEntry::Member(SurfaceMember {
                    key: AuthoredPropertyKey::String(Arc::from(field.name.as_str())),
                    value: unknown_member_value_node(dispatch),
                    optional: field.is_optional,
                    readonly: false,
                    method_kind: None,
                    has_implementation_body: false,
                    visibility: verter_type_expr::MemberVisibility::Public,
                    spans: verter_type_expr::MemberSpans::name_only(field.span),
                    declaration_origin: declaration_origin.clone(),
                    declared_in_macro_type_arg: MacroOwnBodyStamp::NEUTRAL,
                    merge_role: MergeRoleStamp::NEUTRAL,
                    excess_origin: verter_type_expr::ExcessPropertyOrigin::NonLiteral,
                }));
            }
        }
        // Every other non-type-based macro kind has no synthesized
        // surface here (`DefineEmits`/`DefineSlots` runtime-object forms
        // and the `WithDefaults` outer macro).
        _ => return None,
    }
    if entries.is_empty() {
        // A bare `defineExpose()` / an object-less `defineProps()` call
        // has genuinely nothing to surface — `None` stays correct there,
        // distinct from the "has members but they were dropped" defect.
        return None;
    }

    let base = dispatch
        .graph()
        .intern_node(SemanticNodeData::Object(SurfaceView::from_entries(
            entries, None, false,
        )));

    let surface = project_one_level_surface(
        ctx,
        dispatch,
        base,
        Arc::from(Vec::<PathSegment>::new().into_boxed_slice()),
        ProjectionReductionContext::published(ProjectionMode::Shallow),
        None,
    )
    .recorded()?;
    Some(MacroArgumentSurface {
        surface,
        macro_call_span: mac.span,
        unresolved_surface_arms: Vec::new(),
    })
}

/// Decompose a lowered `IndexedAccess` carrier GRAPH node into
/// `(base_node, path)`.
///
/// The macro hot mirror produces a mode-neutral structural carrier graph for
/// the macro type argument; an indexed-access type argument
/// (`DeepConfig['ui']['header']`) lowers to nested
/// [`SemanticNodeData::IndexedAccess`] shells. This walks those shells —
/// collecting each string-literal / canonical-number index hop into a
/// `ProjectPath` selector — until it reaches the base node, so a deep
/// indexed-access decomposes to `(base, [Index("ui"), Index("header")])`
/// WITHOUT lowering the base a second time (it IS the same handle). A
/// non-indexed carrier decomposes to `(node, [])`.
fn decompose_indexed_access_chain_node<C: crate::resolver_core::ResolverCapabilities>(
    dispatch: &crate::project_semantic_dispatch::ProjectSemanticDispatch<'_, C>,
    node: crate::semantic_query::SemanticNodeId,
) -> (
    crate::semantic_query::SemanticNodeId,
    Arc<[crate::semantic_query::PathSegment]>,
) {
    use crate::semantic_query::{IndexKey, PathSegment, SemanticNodeData};

    // Collect outer→inner, then reverse so the path reads base→terminal.
    let mut rev_path: Vec<PathSegment> = Vec::new();
    let mut current = node;
    while let Some(data) =
        crate::project_semantic_dispatch::node_data_for(dispatch.graph(), current)
    {
        match data.as_ref() {
            SemanticNodeData::IndexedAccess { object, index } => match index {
                IndexKey::String(s) => {
                    rev_path.push(PathSegment::Index(IndexKey::String(Arc::clone(s))));
                    current = *object;
                }
                IndexKey::Number(n) => {
                    rev_path.push(PathSegment::Index(IndexKey::Number(*n)));
                    current = *object;
                }
                // A type-node index is not a path-precise string/number hop —
                // stop and let the dispatch resolve the whole indexed-access.
                IndexKey::UniqueSymbol(_) | IndexKey::Computed(_) => break,
            },
            _ => break,
        }
    }
    rev_path.reverse();
    (current, Arc::from(rev_path.into_boxed_slice()))
}
