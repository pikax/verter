//! Path-walking helper for [`ProjectSemanticDispatch::build_project_path`]
//! (plan §3 C3). Extracted from the monolithic `project_semantic_dispatch`
//! module in Phase D §5.2 WIP-Split. No semantic changes.

use super::*;
use crate::semantic_query::{DeclIdentity, QueryError, SemanticQueryApi};

/// Alias-cycle tracking identity (plan §3 C3). `(canonical_id, name)`.
type AliasCycleIdentity = (Arc<str>, Arc<str>);

/// Worklist frame for the iterative `walk_path` driver (plan §2
/// "Iterative worklist").
///
/// `Step` advances a path segment linearly until an arm-splitting
/// variant is hit (`Union` / `Intersection`). When that happens, the
/// driver pushes a `Join*` frame **before** the per-arm `Step` frames
/// so LIFO pop order makes the join execute after every arm result has
/// been appended to the `results` stack.
enum WalkFrame {
    Step {
        node: SemanticNodeId,
        path: Arc<[PathSegment]>,
        index: usize,
    },
    JoinUnion {
        arm_count: usize,
    },
    JoinIntersection {
        arm_count: usize,
    },
}

/// Path-walking helper for [`ProjectSemanticDispatch::build_project_path`]
/// (plan §3 C3). One walker per `build_project_path` invocation — carries
/// the caller's requested `mode`, per-hop fence, and the alias-cycle
/// `visited` set that prevents infinite recursion.
///
/// Emits per-hop origin edges (`ProjectMember`, `ProjectIndex`,
/// `AliasResolve`, `ConditionalSelect`) as the walker descends. The
/// caller emits the whole-path `ProjectPath` edge after the walk finishes.
pub(super) struct PathWalker<'a, 'b> {
    dispatch: &'a ProjectSemanticDispatch<'b>,
    mode: ProjectionMode,
    fence: &'a DepSignature,
    /// Alias-cycle detection set (plan §3 C3). Records every
    /// declaration identity the walker has unwrapped on this single
    /// invocation. `SmallVec` because alias chains are overwhelmingly
    /// short; spills to heap only for pathological fixtures.
    visited_aliases: smallvec::SmallVec<[AliasCycleIdentity; 8]>,
    /// Phase D §5.3 WIP-R: per-call cycle-guard over visited
    /// [`SemanticNodeId`]s (plan §2 WalkGuard contract). Replaces the
    /// retired `max_depth = 64` rail — the set grows only on genuine
    /// re-entry, so linear-chain walks cost O(n) set inserts, not O(n^2)
    /// depth checks.
    visited_nodes: rustc_hash::FxHashSet<SemanticNodeId>,
    /// Phase 1B2: per-step intermediate nodes for backfill.
    /// `intermediate_nodes[i]` = node reached after consuming path[..i+1].
    /// `Some(node)` only on linear `Object` member-step transitions.
    /// `None` marks Union/Intersection/Conditional arm-splits — backfill
    /// skips those positions because the per-arm result is not the
    /// canonical answer for `(base, path[..k], mode)`.
    pub(super) intermediate_nodes: Vec<Option<SemanticNodeId>>,
}

impl<'a, 'b> PathWalker<'a, 'b> {
    pub(super) fn new(
        dispatch: &'a ProjectSemanticDispatch<'b>,
        mode: ProjectionMode,
        fence: &'a DepSignature,
    ) -> Self {
        Self {
            dispatch,
            mode,
            fence,
            visited_aliases: smallvec::SmallVec::new(),
            visited_nodes: rustc_hash::FxHashSet::default(),
            intermediate_nodes: Vec::new(),
        }
    }

    fn graph(&self) -> &Arc<SemanticGraphStore> {
        self.dispatch.graph()
    }

    fn opaque_miss(&self) -> SemanticNodeId {
        self.dispatch.opaque(QueryError::Miss)
    }

    /// Extract the declaration identity `(canonical_id, name)` from a
    /// node that carries one. Only `DeclAnchor` does today — Alias and
    /// other structural variants return `None`, which means they do not
    /// participate in cycle detection (they cannot form cycles without
    /// a DeclAnchor sitting between them in the arena).
    fn alias_identity(&self, node: SemanticNodeId) -> Option<AliasCycleIdentity> {
        let data = self.graph().node_data(node)?;
        match &*data {
            SemanticNodeData::Opaque(crate::semantic_query::QueryError::DeclPlaceholder {
                canonical_id,
                name,
                ..
            }) => Some((Arc::clone(canonical_id), Arc::clone(name))),
            _ => None,
        }
    }

    /// Walk `path` starting from `base`, returning the terminal
    /// [`SemanticNodeId`]. Empty path returns `base` verbatim (plan §2
    /// "empty-path projection is the canonical form of whole-surface
    /// expansion"). Path evaluation walks the whole path; per-hop errors
    /// short-circuit via `Opaque(Miss)`.
    ///
    /// Plan §2 "Iterative worklist": the walker maintains an explicit
    /// stack of `WalkFrame`s and a `results` stack. Arm-splitting frames
    /// (`Union` / `Intersection`) push a `JoinUnion` / `JoinIntersection`
    /// frame followed by one `Step` frame per arm; the join combines
    /// the produced arm results per the contributor rule. Bounded by
    /// graph size (finite, interned). No stack recursion on arm descent.
    /// No depth cap.
    pub(super) fn walk(&mut self, base: SemanticNodeId, path: &[PathSegment]) -> SemanticNodeId {
        let initial_path: Arc<[PathSegment]> = Arc::from(path.to_vec().into_boxed_slice());
        let mut frames: Vec<WalkFrame> = vec![WalkFrame::Step {
            node: base,
            path: initial_path,
            index: 0,
        }];
        let mut results: Vec<SemanticNodeId> = Vec::new();

        while let Some(frame) = frames.pop() {
            match frame {
                WalkFrame::Step { node, path, index } => {
                    self.advance_step(node, &path, index, &mut frames, &mut results);
                }
                WalkFrame::JoinUnion { arm_count } => {
                    self.join_union(arm_count, &mut results);
                }
                WalkFrame::JoinIntersection { arm_count } => {
                    self.join_intersection(arm_count, &mut results);
                }
            }
        }

        results.pop().unwrap_or_else(|| self.opaque_miss())
    }

    /// Advance a single `Step` frame. Runs the linear per-segment loop
    /// until either:
    /// - The path is exhausted (push terminal result, possibly after
    ///   empty-path expansion for `Expanded` mode).
    /// - An arm-splitting variant is reached (push `Join*` + one `Step`
    ///   per arm, then return).
    /// - An error is encountered (push `Opaque(Miss)`).
    ///
    /// Per-step cycle detection via `visited_nodes`: the set grows on
    /// first visit and catches any genuine re-entry (replaces the
    /// retired `max_depth = 64` rail).
    fn advance_step(
        &mut self,
        start_node: SemanticNodeId,
        path: &Arc<[PathSegment]>,
        start_index: usize,
        frames: &mut Vec<WalkFrame>,
        results: &mut Vec<SemanticNodeId>,
    ) {
        if !self.visited_nodes.insert(start_node) {
            results.push(self.dispatch.opaque(QueryError::AliasCycle {
                chain: Arc::from(Vec::<Arc<str>>::new()),
            }));
            return;
        }
        let mut current = start_node;
        let mut index = start_index;

        // Phase 5g-supplement §5.D.0 r17 — honour
        // `HostConfig::depth_budget` so §5.D.4
        // `no_cache_promotion_for_budget_exceeded_*` tests can
        // construct a constrained host and observe a budget-exceeded
        // sentinel (Recursive).
        //
        // Per §0.6.5 stack-depth discipline this walker is already
        // iterative (frame stack on the heap); the budget check here
        // is a discrimination handle for the §5.D.4 contract, not a
        // stack-safety rail. The budget caps the path-segment count
        // the walker may consume — a path of length > budget short-
        // circuits with a Recursive sentinel before the walker visits
        // the over-budget hop.
        //
        // Convention: budget < `MAX_DEPTH` is interpreted as a
        // strict cap; budget == `MAX_DEPTH` (the default) is the
        // existing behavior. This keeps production hosts on the
        // existing graph-size + cycle-set bound while letting
        // hermetic tests construct a small budget for discrimination.
        let budget = self.dispatch.host.config().depth_budget;
        let cap_active = budget > 0 && budget < crate::component_meta_materialize::MAX_DEPTH;

        while index < path.len() {
            if cap_active && index >= budget {
                results.push(self.dispatch.opaque(QueryError::RecursiveRef {
                    name: Arc::from("depth-budget-exceeded"),
                }));
                return;
            }
            let segment = &path[index];
            let data = match self.graph().node_data(current) {
                Some(d) => d,
                None => {
                    results.push(self.opaque_miss());
                    return;
                }
            };
            match &*data {
                SemanticNodeData::Object(surface) => {
                    let needle = match segment {
                        PathSegment::Member(name) => name.as_ref().to_string(),
                        PathSegment::Index(IndexKey::String(s)) => s.as_ref().to_string(),
                        PathSegment::Index(IndexKey::Number(n)) => n.to_string(),
                        PathSegment::Index(IndexKey::TypeNode(node)) => {
                            match self.dispatch.normalized_index_key_node(*node) {
                                IndexKey::String(text) => text.as_ref().to_string(),
                                IndexKey::Number(number) => number.to_string(),
                                IndexKey::TypeNode(_) => {
                                    results.push(self.opaque_miss());
                                    return;
                                }
                            }
                        }
                    };
                    let member = surface
                        .members
                        .iter()
                        .find(|m| m.name.as_ref() == needle.as_str())
                        .cloned();
                    match member {
                        Some(m) => {
                            let meta = match segment {
                                PathSegment::Member(name) => {
                                    OriginMeta::MemberName(Arc::clone(name))
                                }
                                PathSegment::Index(ix) => OriginMeta::Index(ix.clone()),
                            };
                            let edge_kind = match segment {
                                PathSegment::Member(_) => OriginEdgeKind::ProjectMember,
                                PathSegment::Index(_) => OriginEdgeKind::ProjectIndex,
                            };
                            self.graph().record_origin_edge(
                                m.value,
                                edge_kind,
                                Arc::from(vec![current].into_boxed_slice()),
                                meta,
                                Arc::clone(self.fence),
                            );
                            current = m.value;
                            index += 1;
                            // Phase 1B2: record the linear member-step
                            // intermediate. `intermediate_nodes[i]` is the
                            // node reached after consuming path[..i+1].
                            self.intermediate_nodes.push(Some(current));
                        }
                        None => {
                            results.push(self.opaque_miss());
                            return;
                        }
                    }
                }
                SemanticNodeData::Union(arms) => {
                    // Plan §2 iterative worklist: push a `JoinUnion`
                    // frame then one `Step` frame per arm. Arms inherit
                    // the remaining path starting at `index`. Frames pop
                    // LIFO so the join executes AFTER all arm steps
                    // complete. Union contributor rule: any arm
                    // producing `Opaque(_)` → whole union misses.
                    let arms = arms.clone();
                    let arm_count = arms.len();
                    frames.push(WalkFrame::JoinUnion { arm_count });
                    let remaining_path = path.clone();
                    for arm in arms.iter() {
                        frames.push(WalkFrame::Step {
                            node: *arm,
                            path: Arc::clone(&remaining_path),
                            index,
                        });
                    }
                    // Phase 1B2: arm-split — backfill cannot publish a
                    // single canonical answer for `path[..k]` here.
                    self.intermediate_nodes.push(None);
                    return;
                }
                SemanticNodeData::Intersection(arms) => {
                    // Plan §2 iterative worklist + §3 C3 contributor
                    // rule: push a `JoinIntersection` frame then one
                    // `Step` frame per arm. Opaque arms are dropped at
                    // join time; only non-opaque contributors survive.
                    let arms = arms.clone();
                    let arm_count = arms.len();
                    frames.push(WalkFrame::JoinIntersection { arm_count });
                    let remaining_path = path.clone();
                    for arm in arms.iter() {
                        frames.push(WalkFrame::Step {
                            node: *arm,
                            path: Arc::clone(&remaining_path),
                            index,
                        });
                    }
                    // Phase 1B2: arm-split — backfill cannot publish a
                    // single canonical answer for `path[..k]` here.
                    self.intermediate_nodes.push(None);
                    return;
                }
                SemanticNodeData::Conditional {
                    check,
                    extends,
                    true_branch_ref,
                    false_branch_ref,
                    distributive,
                } => {
                    // Open conditional — distribute the remaining path
                    // into both branches via SemanticQueryApi::execute
                    // (re-entry through dispatch → memo dedup). No
                    // direct recursion into `walk_internal`; the
                    // dispatch re-entry inherits per-path memoisation.
                    let check = *check;
                    let extends = *extends;
                    let true_branch = *true_branch_ref;
                    let false_branch = *false_branch_ref;
                    let distributive = *distributive;
                    let rest_path: Arc<[PathSegment]> =
                        Arc::from(path[index..].to_vec().into_boxed_slice());
                    let true_projection = self.dispatch.execute(SemanticQueryKey::ProjectPath {
                        base: true_branch,
                        path: Arc::clone(&rest_path),
                        mode: self.mode,
                    });
                    let false_projection = self.dispatch.execute(SemanticQueryKey::ProjectPath {
                        base: false_branch,
                        path: rest_path,
                        mode: self.mode,
                    });
                    let true_id = match true_projection {
                        QueryResult::Value(id) => id,
                        _ => self.opaque_miss(),
                    };
                    let false_id = match false_projection {
                        QueryResult::Value(id) => id,
                        _ => self.opaque_miss(),
                    };
                    let wrapper = self.graph().intern_node(SemanticNodeData::Conditional {
                        check,
                        extends,
                        true_branch_ref: true_id,
                        false_branch_ref: false_id,
                        distributive,
                    });
                    self.graph().record_origin_edge(
                        wrapper,
                        OriginEdgeKind::ConditionalSelect,
                        Arc::from(vec![check, extends].into_boxed_slice()),
                        OriginMeta::Branch(BranchSelection::Deferred),
                        Arc::clone(self.fence),
                    );
                    self.graph().record_conditional_deferred();
                    results.push(wrapper);
                    // Phase 1B2: open-conditional arm-split — backfill
                    // cannot publish a single canonical answer for
                    // `path[..k]` here (the wrapper Conditional is the
                    // terminal result for the rest of the path, not an
                    // intermediate hop the prefix peek can reuse).
                    self.intermediate_nodes.push(None);
                    return;
                }
                SemanticNodeData::KeyOf { base } => {
                    let resolved = match self
                        .dispatch
                        .execute(SemanticQueryKey::KeyOf { base: *base })
                    {
                        QueryResult::Value(id) => id,
                        _ => {
                            results.push(self.opaque_miss());
                            return;
                        }
                    };
                    if resolved == current {
                        results.push(current);
                        return;
                    }
                    current = resolved;
                }
                SemanticNodeData::IndexedAccess { object, index: ix } => {
                    let resolved = match self.dispatch.execute(SemanticQueryKey::IndexedAccess {
                        base: *object,
                        index: ix.clone(),
                        mode: self.mode,
                    }) {
                        QueryResult::Value(id) => id,
                        _ => {
                            results.push(self.opaque_miss());
                            return;
                        }
                    };
                    if resolved == current {
                        results.push(current);
                        return;
                    }
                    current = resolved;
                }
                SemanticNodeData::Mapped { source, mapper } => {
                    let resolved = match self.dispatch.execute(SemanticQueryKey::MappedType {
                        source: *source,
                        mapper: mapper.clone(),
                    }) {
                        QueryResult::Value(id) => id,
                        _ => {
                            results.push(self.opaque_miss());
                            return;
                        }
                    };
                    if resolved == current {
                        results.push(current);
                        return;
                    }
                    current = resolved;
                }
                SemanticNodeData::TypeOf {
                    value_root,
                    path: typeof_path,
                } => {
                    let mut resolved = match self.dispatch.execute(SemanticQueryKey::TypeOf {
                        value_root: value_root.clone(),
                    }) {
                        QueryResult::Value(id) => id,
                        _ => {
                            results.push(self.opaque_miss());
                            return;
                        }
                    };
                    if !typeof_path.is_empty() {
                        let projection_path: Arc<[PathSegment]> = Arc::from(
                            typeof_path
                                .iter()
                                .map(|segment| PathSegment::Member(Arc::clone(segment)))
                                .collect::<Vec<_>>()
                                .into_boxed_slice(),
                        );
                        resolved = match self.dispatch.execute(SemanticQueryKey::ProjectPath {
                            base: resolved,
                            path: projection_path,
                            mode: self.mode,
                        }) {
                            QueryResult::Value(id) => id,
                            _ => {
                                results.push(self.opaque_miss());
                                return;
                            }
                        };
                    }
                    if resolved == current {
                        results.push(current);
                        return;
                    }
                    current = resolved;
                }
                SemanticNodeData::Alias(target) => {
                    // Alias unwrap — emit AliasResolve edge and
                    // continue from the target. Cycle detection via
                    // visited_aliases set (plan §3 C3).
                    //
                    // Identity is extracted from the *target* (where the
                    // alias points), not from `current` (the Alias node
                    // itself — which has no DeclAnchor shape). If the
                    // target is a DeclAnchor we've seen before on this
                    // walk, the alias graph contains a cycle through
                    // declaration identity; terminate with
                    // `Opaque(AliasCycle)` instead of looping.
                    let target_id = *target;
                    if let Some(identity) = self.alias_identity(target_id) {
                        if self.visited_aliases.iter().any(|a| a == &identity) {
                            let chain: Arc<[Arc<str>]> = Arc::from(
                                self.visited_aliases
                                    .iter()
                                    .map(|(canonical, name)| {
                                        Arc::<str>::from(format!("{canonical}::{name}"))
                                    })
                                    .chain(std::iter::once(Arc::<str>::from(format!(
                                        "{}::{}",
                                        identity.0, identity.1
                                    ))))
                                    .collect::<Vec<_>>()
                                    .into_boxed_slice(),
                            );
                            results.push(self.dispatch.opaque(QueryError::AliasCycle { chain }));
                            return;
                        }
                        self.visited_aliases.push(identity);
                    }
                    self.graph().record_origin_edge(
                        target_id,
                        OriginEdgeKind::AliasResolve,
                        Arc::from(vec![current].into_boxed_slice()),
                        OriginMeta::None,
                        Arc::clone(self.fence),
                    );
                    current = target_id;
                    // Do not consume a segment; we unwrapped an alias.
                }
                // C16: DeclPlaceholder — expand via Instantiate before
                // continuing the path walk.
                SemanticNodeData::Opaque(QueryError::DeclPlaceholder {
                    canonical_id,
                    name,
                    whole_hash,
                }) => {
                    let identity = DeclIdentity {
                        canonical_id: Arc::clone(canonical_id),
                        whole_hash: *whole_hash,
                        decl_name: Arc::clone(name),
                    };
                    drop(data);
                    let expanded = match self.dispatch.execute(SemanticQueryKey::Instantiate {
                        base: identity,
                        args: Arc::from(Vec::<SemanticNodeId>::new().into_boxed_slice()),
                        body_mode: self.mode,
                    }) {
                        QueryResult::Value(id) => id,
                        QueryResult::Recursive(id) => {
                            results.push(id);
                            return;
                        }
                        QueryResult::Error(_) => {
                            results.push(self.opaque_miss());
                            return;
                        }
                    };
                    if expanded == current {
                        results.push(self.opaque_miss());
                        return;
                    }
                    current = expanded;
                }
                // D26 lazy carriers (plan §3 Step 6.1.A + D41).
                // DeclRef in any mode resolves through ResolveDecl
                // ("aliases follow") — Navigate is lazy at the lowering
                // site but transparent through alias chains during walk.
                SemanticNodeData::DeclRef { identity } => {
                    let scope = ScopeId {
                        canonical_id: Arc::clone(&identity.canonical_id),
                        local_scope: None,
                    };
                    let name = Arc::clone(&identity.decl_name);
                    drop(data);
                    let resolved =
                        match self
                            .dispatch
                            .execute(SemanticQueryKey::ResolveDecl(ResolveDeclKey {
                                scope,
                                name,
                            })) {
                            QueryResult::Value(id) => id,
                            QueryResult::Recursive(id) => {
                                results.push(id);
                                return;
                            }
                            QueryResult::Error(_) => {
                                results.push(self.opaque_miss());
                                return;
                            }
                        };
                    if resolved == current {
                        results.push(current);
                        return;
                    }
                    current = resolved;
                }
                // InstantiationRef differs by mode (D41):
                //   - Navigate: TERMINAL — generic application IS a
                //     structural expansion. Preserve as-is so the
                //     EditorToolbar `items: ArrayOrNested<EditorToolbarItem>`
                //     case keeps the lazy `Ref` shape.
                //   - Expanded: dispatch Instantiate, recurse on result.
                SemanticNodeData::InstantiationRef { base, args } => {
                    if matches!(self.mode, ProjectionMode::Navigate) {
                        results.push(current);
                        return;
                    }
                    let identity = base.clone();
                    let args_clone = Arc::clone(args);
                    drop(data);
                    let resolved = match self.dispatch.execute(SemanticQueryKey::Instantiate {
                        base: identity,
                        args: args_clone,
                        body_mode: self.mode,
                    }) {
                        QueryResult::Value(id) => id,
                        QueryResult::Recursive(id) => {
                            results.push(id);
                            return;
                        }
                        QueryResult::Error(_) => {
                            results.push(self.opaque_miss());
                            return;
                        }
                    };
                    if resolved == current {
                        results.push(current);
                        return;
                    }
                    current = resolved;
                }
                SemanticNodeData::Primitive(_)
                | SemanticNodeData::Literal(_)
                | SemanticNodeData::Opaque(_)
                | SemanticNodeData::VueMacroElements(_)
                | SemanticNodeData::Array { .. }
                | SemanticNodeData::Tuple { .. }
                | SemanticNodeData::TemplateLiteral { .. }
                | SemanticNodeData::TypeParam { .. }
                | SemanticNodeData::Infer { .. }
                | SemanticNodeData::Function { .. } => {
                    // Can't descend further through generic path-walk —
                    // Array indexed-access, Tuple slot projection, and
                    // template-literal relation matching are their own
                    // semantic work (C3 path-walker + D-Cutover). The
                    // shell carriers (B4) exist so the graph publishes
                    // these shapes first-class; deeper projection lands
                    // in later phases. Return Opaque(Miss) for now.
                    results.push(self.opaque_miss());
                    return;
                }
            }
        }
        // D-Cutover §5.8: for `mode: Expanded` with empty path, expand
        // terminal `DeclAnchor` nodes via `Instantiate(anchor, [])` and
        // recurse through Intersection/Union arms so nested
        // `extends`/union-arm refs also surface their body. The
        // pre-§5.8 solver's `solve()` fixed-point iteration expanded
        // non-generic aliases across nested shapes; empty-path
        // projection mirrors that for dispatch-only callers. Shallow
        // and Identity modes retain bare-anchor shapes since their
        // contract promises non-expansion.
        if matches!(self.mode, ProjectionMode::Expanded) {
            current = self.expand_empty_path_terminal(current);
        }
        results.push(current);
    }

    /// Combine the top `arm_count` entries from `results` into the
    /// union of the arm projections. Plan §3 C3 union rule: any arm
    /// producing `Opaque(_)` → whole union misses.
    fn join_union(&self, arm_count: usize, results: &mut Vec<SemanticNodeId>) {
        let split = results.len().saturating_sub(arm_count);
        let partials: Vec<SemanticNodeId> = results.drain(split..).collect();
        // If any arm produced Opaque, the whole union collapses.
        if partials.iter().any(|r| {
            matches!(
                self.graph().node_data(*r).as_deref(),
                Some(SemanticNodeData::Opaque(_))
            )
        }) {
            results.push(self.opaque_miss());
            return;
        }
        if partials.is_empty() {
            results.push(self.opaque_miss());
        } else if partials.len() == 1 {
            results.push(partials[0]);
        } else {
            results.push(self.graph().intern_node(SemanticNodeData::Union(Arc::from(
                partials.into_boxed_slice(),
            ))));
        }
    }

    /// Combine the top `arm_count` entries from `results` using the
    /// intersection contributor rule (plan §3 C3): opaque arms drop,
    /// surviving contributors intersect. Zero contributors → `Opaque(Miss)`.
    fn join_intersection(&self, arm_count: usize, results: &mut Vec<SemanticNodeId>) {
        let split = results.len().saturating_sub(arm_count);
        let partials: Vec<SemanticNodeId> = results.drain(split..).collect();
        let contributors: Vec<SemanticNodeId> = partials
            .into_iter()
            .filter(|r| {
                !matches!(
                    self.graph().node_data(*r).as_deref(),
                    Some(SemanticNodeData::Opaque(_))
                )
            })
            .collect();
        if contributors.is_empty() {
            results.push(self.opaque_miss());
        } else if contributors.len() == 1 {
            results.push(contributors[0]);
        } else {
            results.push(
                self.graph()
                    .intern_node(SemanticNodeData::Intersection(Arc::from(
                        contributors.into_boxed_slice(),
                    ))),
            );
        }
    }

    /// Iterative empty-path-terminal expander (Path C C9). Replaces the
    /// previous recursive form so Union / Intersection arm descent grows
    /// the heap-backed worklist rather than the Rust call stack.
    /// `DeclAnchor` expansion tail-iterates through the worklist rather
    /// than via a recursive call.
    ///
    /// **Mapped arm (plan §2 Stage 5 Pass C9).** When the walker runs in
    /// [`ProjectionMode::Expanded`] and a `SemanticNodeData::Mapped`
    /// shell appears at an empty-path terminal, re-enter dispatch via
    /// [`SemanticQueryKey::MappedType`] so the deferred shell is
    /// materialised into its concrete surface rather than being returned
    /// unchanged (addresses the pre-§14 Gemini F2 report where
    /// `Expanded` mode left Mapped shells deferred).
    fn expand_empty_path_terminal(&mut self, node: SemanticNodeId) -> SemanticNodeId {
        let mut work: Vec<ExpandFrame> = Vec::new();
        let mut results: Vec<SemanticNodeId> = Vec::new();
        work.push(ExpandFrame::Expand(node));

        while let Some(frame) = work.pop() {
            match frame {
                ExpandFrame::Expand(id) => {
                    self.expand_terminal_step(id, &mut work, &mut results);
                }
                ExpandFrame::CombineIntersection { parent, originals } => {
                    self.combine_expanded_arms(
                        parent,
                        &originals,
                        &mut results,
                        ExpansionCombineKind::Intersection,
                    );
                }
                ExpandFrame::CombineUnion { parent, originals } => {
                    self.combine_expanded_arms(
                        parent,
                        &originals,
                        &mut results,
                        ExpansionCombineKind::Union,
                    );
                }
            }
        }

        results.pop().unwrap_or(node)
    }

    /// Expand one node worth of work, pushing either a direct result
    /// onto `results` or sub-expansions + a combine frame onto `work`.
    fn expand_terminal_step(
        &mut self,
        node: SemanticNodeId,
        work: &mut Vec<ExpandFrame>,
        results: &mut Vec<SemanticNodeId>,
    ) {
        let data = match self.graph().node_data(node) {
            Some(data) => data,
            None => {
                results.push(node);
                return;
            }
        };
        match &*data {
            // C16: DeclPlaceholder — expand via Instantiate.
            SemanticNodeData::Opaque(QueryError::DeclPlaceholder {
                canonical_id,
                name,
                whole_hash,
            }) => {
                let identity = DeclIdentity {
                    canonical_id: Arc::clone(canonical_id),
                    whole_hash: *whole_hash,
                    decl_name: Arc::clone(name),
                };
                if let Some(alias_id) = self.alias_identity(node) {
                    if self.visited_aliases.iter().any(|a| a == &alias_id) {
                        drop(data);
                        results.push(node);
                        return;
                    }
                    self.visited_aliases.push(alias_id);
                }
                if canonical_id.contains("/node_modules/")
                    || canonical_id.contains("\\node_modules\\")
                {
                    drop(data);
                    results.push(node);
                    return;
                }
                drop(data);
                let expanded = match self.dispatch.execute(SemanticQueryKey::Instantiate {
                    base: identity,
                    args: Arc::from(Vec::<SemanticNodeId>::new().into_boxed_slice()),
                    body_mode: self.mode,
                }) {
                    QueryResult::Value(id) => id,
                    QueryResult::Recursive(id) => {
                        results.push(id);
                        return;
                    }
                    QueryResult::Error(_) => {
                        results.push(node);
                        return;
                    }
                };
                if expanded == node {
                    results.push(node);
                    return;
                }
                work.push(ExpandFrame::Expand(expanded));
            }
            SemanticNodeData::Mapped { source, mapper }
                if matches!(self.mode, ProjectionMode::Expanded) =>
            {
                let source = *source;
                let mapper = mapper.clone();
                drop(data);
                let materialised = match self
                    .dispatch
                    .execute(SemanticQueryKey::MappedType { source, mapper })
                {
                    QueryResult::Value(id) => id,
                    QueryResult::Recursive(id) => {
                        results.push(id);
                        return;
                    }
                    QueryResult::Error(_) => {
                        results.push(node);
                        return;
                    }
                };
                if materialised == node {
                    results.push(node);
                    return;
                }
                work.push(ExpandFrame::Expand(materialised));
            }
            // Phase 5f §7: open Conditional at empty-path terminal in
            // Expanded mode. Per CLAUDE.md "Macro Type Traversal Rule"
            // — open conditionals distribute the remaining path into
            // both branches; with empty path the "remaining path" is
            // empty so distribution becomes Union(true_branch,
            // false_branch) after each branch is itself expanded.
            //
            // Mirrors the Conditional arm in `advance_step` (lines
            // 280-336) which handles open conditionals at non-empty
            // path positions. Closes the inherited-emits seed
            // (`defineEmits<Mode extends 'editor' ? EditorEmits :
            // ViewerEmits>` with `Mode` unbound) by surfacing both
            // branches' emit shapes through dispatch's
            // `ProjectPath{[],Expanded}` instead of leaving a
            // top-level Conditional shell that the trampoline filter
            // `type_expr_is_expanded_surface` rejects.
            //
            // Distribution-trigger guard: only distribute when the
            // `check` is unbound (TypeParam / Infer). Concrete checks
            // that produced a deferred shell because `relate_nodes`
            // returned `Unknown` (e.g., a structural relation the
            // shallow check can't decide) MUST NOT distribute — the
            // relation may still resolve at a more concrete callsite
            // (after substitutions land), and a premature distribute
            // would falsely Union the false branch into a result that
            // is actually true-branch-only. CLAUDE.md "open
            // conditionals" semantics scope distribution to unbound
            // checks; concrete-check Unknowns are deferred reductions,
            // not open conditionals.
            SemanticNodeData::Conditional {
                check,
                true_branch_ref,
                false_branch_ref,
                ..
            } if matches!(self.mode, ProjectionMode::Expanded)
                && matches!(
                    self.graph().node_data(*check).as_deref(),
                    Some(SemanticNodeData::TypeParam { .. } | SemanticNodeData::Infer { .. })
                ) =>
            {
                let true_branch = *true_branch_ref;
                let false_branch = *false_branch_ref;
                drop(data);
                let arms: Arc<[SemanticNodeId]> =
                    Arc::from(vec![true_branch, false_branch].into_boxed_slice());
                work.push(ExpandFrame::CombineUnion {
                    parent: node,
                    originals: Arc::clone(&arms),
                });
                for arm in arms.iter().rev() {
                    work.push(ExpandFrame::Expand(*arm));
                }
            }
            SemanticNodeData::Intersection(arms) => {
                let arms = Arc::clone(arms);
                drop(data);
                if arms.is_empty() {
                    results.push(node);
                    return;
                }
                work.push(ExpandFrame::CombineIntersection {
                    parent: node,
                    originals: Arc::clone(&arms),
                });
                for arm in arms.iter().rev() {
                    work.push(ExpandFrame::Expand(*arm));
                }
            }
            SemanticNodeData::Union(arms) => {
                let arms = Arc::clone(arms);
                drop(data);
                if arms.is_empty() {
                    results.push(node);
                    return;
                }
                work.push(ExpandFrame::CombineUnion {
                    parent: node,
                    originals: Arc::clone(&arms),
                });
                for arm in arms.iter().rev() {
                    work.push(ExpandFrame::Expand(*arm));
                }
            }
            _ => {
                drop(data);
                results.push(node);
            }
        }
    }

    /// Rebuild an Intersection / Union arm list from the top `originals.len()`
    /// entries on `results`. If every expanded arm is identity to its
    /// original, push the parent id unchanged (avoids spurious
    /// re-interning); otherwise intern the rebuilt compound.
    fn combine_expanded_arms(
        &mut self,
        parent: SemanticNodeId,
        originals: &[SemanticNodeId],
        results: &mut Vec<SemanticNodeId>,
        kind: ExpansionCombineKind,
    ) {
        let n = originals.len();
        let start = results.len().saturating_sub(n);
        let expanded: Vec<SemanticNodeId> = results.drain(start..).collect();
        debug_assert_eq!(
            expanded.len(),
            n,
            "ExpandFrame combine: expected {n} prior arm results, saw {}",
            expanded.len()
        );
        if expanded.iter().zip(originals.iter()).all(|(a, b)| a == b) {
            results.push(parent);
            return;
        }
        let rebuilt = match kind {
            ExpansionCombineKind::Intersection => {
                self.graph()
                    .intern_node(SemanticNodeData::Intersection(Arc::from(
                        expanded.into_boxed_slice(),
                    )))
            }
            ExpansionCombineKind::Union => self.graph().intern_node(SemanticNodeData::Union(
                Arc::from(expanded.into_boxed_slice()),
            )),
        };
        results.push(rebuilt);
    }
}

/// Worklist frame for the iterative `expand_empty_path_terminal` driver
/// (Path C C9). `Expand` advances one node; `Combine*` rebuild a
/// compound from its previously-expanded arms.
enum ExpandFrame {
    Expand(SemanticNodeId),
    CombineIntersection {
        parent: SemanticNodeId,
        originals: Arc<[SemanticNodeId]>,
    },
    CombineUnion {
        parent: SemanticNodeId,
        originals: Arc<[SemanticNodeId]>,
    },
}

/// Tag for [`PathWalker::combine_expanded_arms`] — picks the compound
/// variant to intern when at least one arm changed.
enum ExpansionCombineKind {
    Intersection,
    Union,
}
