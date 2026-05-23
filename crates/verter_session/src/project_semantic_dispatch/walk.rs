//! Path-walking helper for [`ProjectSemanticDispatch::build_project_path`]
//! plus the iterative shallow-mode terminal-surface synthesiser used by
//! `Instantiate { body_mode: ProjectionMode::Shallow }` and by empty-path
//! `ProjectPath` projections in `Shallow` mode. The synthesiser is
//! deliberately non-recursive — it drives a heap-backed worklist so
//! 100-arm intersections / unions cannot overflow the Rust call stack.

use std::sync::atomic::{AtomicUsize, Ordering};

use super::*;
use crate::semantic_query::{DeclIdentity, QueryError, SemanticQueryApi, SurfaceMember};

/// Per-walk maximum frame-stack depth observed by
/// [`expand_empty_path_shallow_terminal_surface`]. Used by
/// [`probe_max_walker_frame_depth`] for the
/// `shallow_walker_stack_depth_bounded_for_100_intersection`
/// regression test that asserts the iterative-worklist invariant
/// (≤ 10 frames for a 100-arm input).
static LAST_SHALLOW_WALKER_MAX_FRAMES: AtomicUsize = AtomicUsize::new(0);

/// Probe the maximum frame-stack depth observed by the most recent
/// shallow-mode terminal-surface walker invocation triggered by
/// dispatching `key`. Drives a fresh dispatch (so the assertion stands
/// regardless of warm-cache state) and returns the depth observed.
///
/// Test-only entry point — the implementation is path-precise:
/// dispatching the same `key` re-runs the walker (subject to memo
/// admission), and the depth atomic is reset per walker invocation
/// before the worklist starts. Concurrent uses of this probe are not
/// thread-safe; call sites are unit tests on a single thread.
#[cfg(test)]
#[doc(hidden)]
#[must_use]
pub fn probe_max_walker_frame_depth(
    dispatch: &ProjectSemanticDispatch<'_>,
    key: &SemanticQueryKey,
) -> usize {
    LAST_SHALLOW_WALKER_MAX_FRAMES.store(0, Ordering::Relaxed);
    let _ = dispatch.execute(key.clone());
    LAST_SHALLOW_WALKER_MAX_FRAMES.load(Ordering::Relaxed)
}

/// Diagnostic emitted by the shallow-mode terminal-surface walker. The
/// memo replays diagnostics on warm reads via `CacheRead.walker_diagnostics`.
/// Variants describe non-fatal observations (cycle short-circuits, open
/// conditionals, pathological-input cap) so consumers can render
/// human-readable messages without re-walking the graph.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum ShallowDiagnostic {
    /// `T & T` — duplicate intersection arm short-circuited so the
    /// walker does not re-enter an arm that contributes nothing new.
    DuplicateArmShortCircuited { node: SemanticNodeId },
    /// True graph cycle detected during shallow-mode walk; the walker
    /// terminates the offending arm without contribution.
    CycleShortCircuited { node: SemanticNodeId },
    /// `Instantiate { ..., body_mode: Navigate }` returned `Recursive`
    /// — the declaration referenced itself transitively. Carries the
    /// declaration identity for diagnostic context.
    CyclicInstantiation { decl: DeclIdentity },
    /// `Instantiate` returned an `Error(QueryError)` with a fatal
    /// query-level failure during shallow-mode synthesis. Carries the
    /// declaration identity and the underlying error so consumers can
    /// distinguish missing declarations from budget exhaustion.
    InstantiationError {
        decl: DeclIdentity,
        error: QueryError,
    },
    /// Open conditional encountered at empty-path Shallow terminal —
    /// no branch was selected, so the walker yields the empty surface
    /// for the conditional's contribution.
    OpenConditional { node: SemanticNodeId },
    /// Pathological input — the walker's visited set exceeded the
    /// 10_000-node cap. `cache_suppress` is set so the result is not
    /// promoted to the memo.
    PathologicalInput { root: SemanticNodeId },
    /// One arm of a Union evaluated to a non-Object surface; the
    /// merged surface drops members for that arm per the union rule
    /// (member surface = members in ALL arms).
    UnionArmEmpty {
        union_node: SemanticNodeId,
        arm_index: usize,
    },
}

/// Build output threaded through `build_project_path` so the dispatch
/// build-closure layer can observe walker diagnostics + the
/// `cache_suppress` aggregation produced by the shallow-mode terminal-
/// surface synthesiser. For non-walker builds (`ResolveDecl`,
/// `Instantiate`, `KeyOf`, etc.), the existing `(QueryResult, DepSignature)`
/// shape coerces via `From` — those builds preserve their tuple
/// signature unchanged.
///
/// `observed_self_roots` carries every `(canonical, observed_hash)`
/// self-root the cold build captured at the value source — the keyed
/// canonical for `ResolveDecl` / `TypeOf` / `Instantiate` /
/// `ResolveMacroPayload`, or the file-derived origin of every input
/// node for the node kinds keyed by interned `SemanticNodeId`s. The
/// shared cold-build helper feeds these to
/// [`crate::semantic_query_memo::semantic_graph_read_set_signature`] so
/// the published memo entry is self-version-rooted: a warm read
/// validates each self-root `FileWholeHash` strictly. The hash is the
/// content version the builder *observed* — never re-read at
/// signature-build time.
///
/// `cache_suppress` (already on this struct) carries the
/// non-cacheable signal: a build that needs a self-root but cannot
/// observe it (an evicted / deleted scope artifact) sets `cache_suppress`
/// so the memo refuses admission while the value still flows to the
/// caller.
/// One deferred prefix-backfill record. Accumulated during a
/// `build_project_path` walk; published into the warm map AFTER the
/// shared cold-build helper finalises the fact tracer so each
/// backfilled entry's carrier holds the same path-precise fact
/// signature as the parent entry. See
/// [`crate::project_semantic_dispatch::build::backfill_prefixes`] for
/// the producer and the shared cold-build helper for the publication
/// point.
#[derive(Debug, Clone)]
pub struct PrefixBackfill {
    pub key: crate::semantic_query::SemanticQueryKey,
    pub node: SemanticNodeId,
}

#[derive(Debug)]
pub struct QueryBuildOutput {
    pub result: QueryResult<SemanticNodeId>,
    pub dep_signature: DepSignature,
    pub walker_diagnostics: Vec<ShallowDiagnostic>,
    pub cache_suppress: bool,
    /// Every `(canonical, observed_hash)` self-root the cold build
    /// captured at the value source — the keyed canonical for
    /// `ResolveDecl` / `TypeOf` / `Instantiate` / `ResolveMacroPayload`,
    /// or the file-derived origin of every input node for the node
    /// kinds keyed by interned `SemanticNodeId`s. The shared cold-build
    /// helper feeds these to
    /// [`crate::semantic_query_memo::semantic_graph_read_set_signature`]
    /// so the published memo entry is self-version-rooted. Empty for
    /// builds whose result is fully structural (no file-derived
    /// dependency).
    ///
    /// A `Vec` (not an inline `SmallVec`) keeps `QueryBuildOutput`
    /// compact on the stack: this struct is moved through every
    /// `build_*` → `execute_cooperative` hop, and a deeply-nested type
    /// resolution carries one per recursion frame.
    pub observed_self_roots: Vec<crate::semantic_query_memo::ObservedGraphSelfRoot>,
    /// The completed self-version-rooted carrier for the published
    /// memo entry — built by the shared cold-build helper from
    /// [`Self::observed_self_roots`] and the traced fact set via
    /// [`crate::semantic_query_memo::semantic_graph_read_set_signature`].
    ///
    /// `None` BEFORE the shared cold-build helper post-processes the
    /// raw build output (the build closures never set it). After
    /// post-processing, **memo admission is decided by
    /// [`Self::cache_suppress`], not by the presence of this carrier**:
    /// a cacheable build carries the self-version-rooted carrier the
    /// memo publishes; a non-cacheable build whose self-rooting failed
    /// (`semantic_graph_read_set_signature` → `None`) still carries a
    /// NON-ADMITTED carrier holding its traced cross-file dep facts so
    /// the cooperative-admission winner can broadcast them to joiners.
    /// Only a tracer-overflow build (no bounded fact list) leaves this
    /// `None`. Whatever carrier is present, `warm_publish_one` / the
    /// in-flight joiner state / prefix-backfill publish or broadcast it
    /// verbatim — they NEVER reconstruct facts from the legacy fence.
    ///
    /// `Box`ed so the in-line `None` case (the raw build-closure
    /// output, on every recursion frame of a deep type resolution)
    /// costs one pointer rather than an inline `ReadSetSignature`.
    pub graph_carrier: Option<Box<crate::fact_signature_helpers::ReadSetSignature>>,
    /// The self-root canonicals recorded on the published
    /// [`crate::semantic_query_memo::MemoEntry`] so a warm read
    /// validates each one's self-root `FileWholeHash` strictly. Derived
    /// by the shared cold-build helper from [`Self::observed_self_roots`].
    /// Empty before post-processing and for structural builds.
    pub self_root_canonicals: Arc<[Arc<str>]>,
    /// Pending prefix-backfill records. The walker pushes one entry
    /// per linear intermediate; the shared cold-build helper publishes
    /// them AFTER the fact tracer finalises so backfilled memo entries
    /// carry the parent's authoritative carrier (the same
    /// self-version-rooted [`Self::graph_carrier`]).
    pub pending_prefix_backfills: Vec<PrefixBackfill>,
}

impl From<(QueryResult<SemanticNodeId>, DepSignature)> for QueryBuildOutput {
    #[inline]
    fn from((result, dep_signature): (QueryResult<SemanticNodeId>, DepSignature)) -> Self {
        Self {
            result,
            dep_signature,
            walker_diagnostics: Vec::new(),
            cache_suppress: false,
            observed_self_roots: Vec::new(),
            graph_carrier: None,
            self_root_canonicals: Arc::from([]),
            pending_prefix_backfills: Vec::new(),
        }
    }
}

impl QueryBuildOutput {
    /// Attach the cold build's observed self-roots and return `self`.
    ///
    /// Builders that produce a `(QueryResult, DepSignature)` tuple coerce
    /// via the [`From`] impl above (self-roots empty), then call this to
    /// record the `(canonical, observed_hash)` self-root pairs the build
    /// captured at the value source. The shared cold-build helper feeds
    /// these to
    /// [`crate::semantic_query_memo::semantic_graph_read_set_signature`].
    #[inline]
    #[must_use]
    pub fn with_observed_self_roots(
        mut self,
        roots: impl IntoIterator<Item = crate::semantic_query_memo::ObservedGraphSelfRoot>,
    ) -> Self {
        self.observed_self_roots.extend(roots);
        self
    }
}

/// Transient walker-internal surface representation. Not interned in the
/// semantic graph — only the final merged surface is interned via
/// `SemanticNodeData::Object` once synthesis completes.
#[derive(Debug, Clone, Default)]
pub struct ShallowSurface {
    pub members: Vec<ShallowSurfaceMember>,
}

/// One member contribution while the walker is merging arms. Carries the
/// surface-level optionality / readonly bits so intersection/union merges
/// can implement the TS rules (intersection: required-wins +
/// readonly-OR; union: members in all arms).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ShallowSurfaceMember {
    pub name: Arc<str>,
    pub value: SemanticNodeId,
    pub optional: bool,
    pub readonly: bool,
    pub is_method: bool,
}

impl ShallowSurface {
    /// Empty surface contribution — used when an arm cannot yield any
    /// members (open conditional, non-Object terminal, instantiation
    /// recursion).
    #[inline]
    #[must_use]
    pub fn empty() -> Self {
        Self {
            members: Vec::new(),
        }
    }

    /// Build a `ShallowSurface` from a `SurfaceView`. The members are
    /// cloned shallowly — `value` is a `SemanticNodeId` (Copy), so the
    /// clone cost is one Arc bump for `name`.
    #[must_use]
    pub fn from_object(view: &SurfaceView) -> Self {
        Self {
            members: view
                .members
                .iter()
                .map(|m| ShallowSurfaceMember {
                    name: Arc::clone(&m.name),
                    value: m.value,
                    optional: m.optional,
                    readonly: m.readonly,
                    is_method: m.is_method,
                })
                .collect(),
        }
    }
}

/// Returns `true` for `QueryError` variants that must propagate through
/// `cache_suppress` so the memo refuses insertion (a pathological input
/// or a budget breach must not warm the shared cache).
#[inline]
fn is_fatal_query_error(err: &QueryError) -> bool {
    matches!(
        err,
        QueryError::BudgetExceeded(_) | QueryError::UnstableState { .. }
    )
}

/// Alias-cycle tracking identity. `(canonical_id, name)`.
type AliasCycleIdentity = (Arc<str>, Arc<str>);

/// Worklist frame for the iterative `walk_path` driver (
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

/// Literal key used by the Mapped operator-level narrowing path.
///
/// Carries both the literal value and its key-domain kind so the
/// substitution `K = Literal(...)` chooses the correct `LiteralValue`
/// variant (`String` vs `Number`). TypeScript's indexed access
/// `M['1']` (string literal) and `M[1]` (number literal) are
/// semantically distinct keys; reducing both to `String("1")` would
/// silently rewrite any value expression that depends on `K`
/// (identity mapping `K`, `K extends ...`, template literals, …).
///
/// String/number kind also drives the Tier 3 primitive-domain
/// admission check via the segment's `is_string_domain` flag — a
/// numeric segment is admitted only by a `number`-domain key_space
/// and vice versa.
///
/// The `Number` variant stores the literal as `f64` directly so it
/// can be interned into `LiteralValue::Number(f64)` without any
/// further conversion.
///
/// G4.4 convention: every `IndexKey::Number` producer (source
/// lowering at `lower::shallow_lower_type_expr`, node normalisation
/// at `evaluate::normalized_index_key_node`, generic substitution
/// at `substitute::substitute_index_key_with_change_tracking`)
/// stores the truncated integer value of the numeric literal as
/// `i64`. This is the single shared convention across the pipeline
/// — recovery here is a direct `i64 → f64` cast, symmetric with
/// the raise at `raise::raise_index_key_to_type_expr`. Non-integer
/// numeric indices (`Foo[1.5]`) cannot round-trip exactly through
/// `i64` and remain as `IndexKey::TypeNode` references rather than
/// entering this fast path.
///
/// G4.5 completion: the `IndexKey::TypeNode(_)` consumer arm in the
/// Mapped narrowing path inspects the resolved node's data and
/// recovers an f64 `LiteralKey::Number` directly when the node is a
/// `SemanticNodeData::Literal(LiteralValue::Number(f))`. This closes
/// the soundness gap where `{ [K in number]: K }[1.5]` would have
/// fallen back to a deferred Mapped shell instead of substituting
/// `K = 1.5`.
enum LiteralKey {
    String(Arc<str>),
    Number(f64),
}

/// Path-walking helper for [`ProjectSemanticDispatch::build_project_path`]
/// One walker per `build_project_path` invocation — carries
/// the caller's requested `mode`, per-hop fence, and the alias-cycle
/// `visited` set that prevents infinite recursion.
///
/// Emits per-hop origin edges (`ProjectMember`, `ProjectIndex`,
/// `AliasResolve`, `ConditionalSelect`) as the walker descends. The
/// caller emits the whole-path `ProjectPath` edge after the walk finishes.
pub(super) struct PathWalker<'a, 'b> {
    dispatch: &'a ProjectSemanticDispatch<'b>,
    /// The walker carries the full [`ProjectionReductionContext`]
    /// from its constructing caller — the `mode` field is preserved
    /// as a derived accessor ([`Self::mode`]) so the existing call
    /// sites that consult `self.mode` remain unchanged. The demand
    /// axis flows through to per-key Mapped surface synthesis in
    /// `synthesise_mapped_surface` (gated on Published demand per the
    /// boundary constraint that StructuralTransit MUST NOT enumerate
    /// / materialise mapped members — transit is the non-publication
    /// rail).
    context: crate::semantic_query::ProjectionReductionContext,
    fence: &'a DepSignature,
    /// Alias-cycle detection set. Records every
    /// declaration identity the walker has unwrapped on this single
    /// invocation. `SmallVec` because alias chains are overwhelmingly
    /// short; spills to heap only for pathological fixtures.
    visited_aliases: smallvec::SmallVec<[AliasCycleIdentity; 8]>,
    /// Phase D §5.3 WIP-R: per-call cycle-guard over visited
    /// [`SemanticNodeId`]s. Replaces the
    /// retired `max_depth = 64` rail — the set grows only on genuine
    /// re-entry, so linear-chain walks cost O(n) set inserts, not O(n^2)
    /// depth checks.
    visited_nodes: rustc_hash::FxHashSet<SemanticNodeId>,
    /// Per-step intermediate nodes for backfill.
    /// `intermediate_nodes[i]` = node reached after consuming path[..i+1].
    /// `Some(node)` only on linear `Object` member-step transitions.
    /// `None` marks Union/Intersection/Conditional arm-splits — backfill
    /// skips those positions because the per-arm result is not the
    /// canonical answer for `(base, path[..k], mode)`.
    pub(super) intermediate_nodes: Vec<Option<SemanticNodeId>>,
    /// Diagnostics produced by the shallow-mode terminal-surface
    /// synthesiser. Empty for `Identity` / `Navigate` / `Expanded` /
    /// `Skeleton` walks. Drained by `build_project_path` into the
    /// [`QueryBuildOutput`] when the walker finishes.
    pub(super) walker_diagnostics: Vec<ShallowDiagnostic>,
    /// `true` when the walker hit the pathological-input cap or a
    /// nested Instantiate dispatch produced a fatal `QueryError`. The
    /// memo refuses insertion when this is true.
    pub(super) cache_suppress: bool,
}

impl<'a, 'b> PathWalker<'a, 'b> {
    pub(super) fn new(
        dispatch: &'a ProjectSemanticDispatch<'b>,
        context: crate::semantic_query::ProjectionReductionContext,
        fence: &'a DepSignature,
    ) -> Self {
        Self {
            dispatch,
            context,
            fence,
            visited_aliases: smallvec::SmallVec::new(),
            visited_nodes: rustc_hash::FxHashSet::default(),
            intermediate_nodes: Vec::new(),
            walker_diagnostics: Vec::new(),
            cache_suppress: false,
        }
    }

    /// Derived accessor — equivalent to the pre-Commit-2 `self.mode`
    /// field. The full demand axis lives on [`Self::context`].
    #[inline]
    fn mode(&self) -> ProjectionMode {
        self.context.mode
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
    /// [`SemanticNodeId`]. Empty path returns `base` verbatim (
    /// "empty-path projection is the canonical form of whole-surface
    /// expansion"). Path evaluation walks the whole path; per-hop errors
    /// short-circuit via `Opaque(Miss)`.
    ///
    /// "Iterative worklist": the walker maintains an explicit
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

        // Supplement §5.D.0 r17 — honour
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
        let budget = self.dispatch.ctx.config().depth_budget;
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
                            // Record the linear member-step
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
                    // Iterative worklist: push a `JoinUnion`
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
                    // Arm-split — backfill cannot publish a
                    // single canonical answer for `path[..k]` here.
                    self.intermediate_nodes.push(None);
                    return;
                }
                SemanticNodeData::Intersection(arms) => {
                    // Iterative worklist + §3 C3 contributor
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
                    // Arm-split — backfill cannot publish a
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
                        context: crate::semantic_query::ProjectionReductionContext::published(
                            self.mode(),
                        ),
                    });
                    let false_projection = self.dispatch.execute(SemanticQueryKey::ProjectPath {
                        base: false_branch,
                        path: rest_path,
                        context: crate::semantic_query::ProjectionReductionContext::published(
                            self.mode(),
                        ),
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
                    // Open-conditional arm-split — backfill
                    // cannot publish a single canonical answer for
                    // `path[..k]` here (the wrapper Conditional is the
                    // terminal result for the rest of the path, not an
                    // intermediate hop the prefix peek can reuse).
                    self.intermediate_nodes.push(None);
                    return;
                }
                SemanticNodeData::KeyOf { base } => {
                    let resolved = match self.dispatch.execute(SemanticQueryKey::KeyOf {
                        base: *base,
                        context: crate::semantic_query::ProjectionReductionContext::published(
                            self.mode(),
                        ),
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
                SemanticNodeData::IndexedAccess { object, index: ix } => {
                    let resolved = match self.dispatch.execute(SemanticQueryKey::IndexedAccess {
                        base: *object,
                        index: ix.clone(),
                        mode: self.mode(),
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
                    // Operator-level Mapped narrowing: when the walker
                    // has a literal key segment and the mapper has no
                    // name remap, substitute K = Literal(name) into
                    // `mapper.value_expr` and evaluate it directly,
                    // rather than dispatching whole-surface MappedType
                    // resolution (which would enumerate every key in
                    // the source surface and substitute value_expr
                    // per-key — path-imprecise: produces sibling key
                    // contributions the caller never requested).
                    //
                    // The narrowing requires:
                    // 1. A remaining segment we can convert to a
                    //    literal name (Member, Index::String,
                    //    Index::Number, or Index::TypeNode that
                    //    normalises to a literal).
                    // 2. `mapper.name_remap.is_none()` — when the
                    //    mapper has `as <expr>`, the iteration key
                    //    is NOT the post-remap surface name; mapping
                    //    back requires the whole-surface enumeration.
                    // 3. Soundness: the literal name MUST be admitted
                    //    by the mapper's key domain. If the source
                    //    surface enumerates and the literal is NOT in
                    //    its member set, OR the key space enumerates
                    //    and the literal is NOT in its enumerated key
                    //    set, the narrowing is unsound — substituting
                    //    a non-admissible key into a non-`T[K]` value
                    //    expression would synthesise a value type
                    //    (e.g. `string`) for a key that the mapped
                    //    surface does NOT contain. Such cases fall
                    //    through to the whole-surface fallback, which
                    //    produces the correct Object miss via the
                    //    bounded enumerated surface.
                    let next_index = index;
                    let next_segment = path.get(next_index);
                    // Carry both the literal *value* (string text or
                    // numeric value) AND its key-domain kind through
                    // narrowing. At substitution time we intern
                    // `LiteralValue::String(...)` vs `LiteralValue::Number(...)`
                    // based on the kind — TypeScript indexed access
                    // `M['1']` (string literal) and `M[1]` (number
                    // literal) are semantically distinct keys, and any
                    // value expression that depends on `K` (identity
                    // mapping `K`, conditional `K extends ...`, etc.)
                    // must substitute the correctly-typed literal.
                    //
                    // The admission tier (Tier 3 primitive-domain
                    // check) reads the same kind discriminator so a
                    // numeric segment is admitted only by a number-
                    // domain key_space and vice versa.
                    let (literal_key, segment_is_string_domain): (Option<LiteralKey>, bool) =
                        match next_segment {
                            Some(PathSegment::Member(name)) => {
                                (Some(LiteralKey::String(Arc::clone(name))), true)
                            }
                            Some(PathSegment::Index(IndexKey::String(s))) => {
                                (Some(LiteralKey::String(Arc::clone(s))), true)
                            }
                            Some(PathSegment::Index(IndexKey::Number(n))) => {
                                // G4.4: integer-convention recovery.
                                //
                                // Every `IndexKey::Number` producer
                                // (`lower::shallow_lower_type_expr`,
                                // `evaluate::normalized_index_key_node`,
                                // `substitute::substitute_index_key_with_change_tracking`)
                                // stores the truncated integer value of
                                // the numeric literal as `i64`. Recover
                                // the f64 via a direct integer→float
                                // cast — symmetric with
                                // `raise::raise_index_key_to_type_expr`'s
                                // `*number as f64` raise.
                                //
                                // Non-integer numeric indices (`Foo[1.5]`)
                                // never reach this arm — the producer-side
                                // `fract() == 0.0` admission guards
                                // route them through `IndexKey::TypeNode`
                                // instead, preserving full f64 precision
                                // via the SemanticNodeId reference.
                                (Some(LiteralKey::Number(*n as f64)), false)
                            }
                            Some(PathSegment::Index(IndexKey::TypeNode(node))) => {
                                match self.dispatch.normalized_index_key_node(*node) {
                                    IndexKey::String(text) => {
                                        (Some(LiteralKey::String(Arc::clone(&text))), true)
                                    }
                                    IndexKey::Number(number) => {
                                        // `normalized_index_key_node`
                                        // stores the truncated integer
                                        // value (`*number as i64` of a
                                        // `LiteralValue::Number` whose
                                        // `fract()` is 0), not the
                                        // bit-pattern. Recover the f64
                                        // by direct integer→float cast.
                                        (Some(LiteralKey::Number(number as f64)), false)
                                    }
                                    IndexKey::TypeNode(resolved) => {
                                        // G4.5: `normalized_index_key_node`
                                        // returns `IndexKey::Number(i64)`
                                        // ONLY when the resolved node is a
                                        // numeric literal whose `fract()`
                                        // is 0 AND it round-trips through
                                        // `i64`. Non-integer literals
                                        // (`Foo[1.5]`) and out-of-i64-range
                                        // numeric literals fall through to
                                        // this `TypeNode(_)` arm, even
                                        // though they ARE concrete numeric
                                        // literals at the graph level.
                                        //
                                        // Recover the f64 literal directly
                                        // from the resolved node's data so
                                        // the Mapped narrowing can perform
                                        // the `K = Literal(Number(f))`
                                        // substitution for primitive
                                        // `number`-domain key spaces
                                        // (`{ [K in number]: K }[1.5]`).
                                        // Mirrors the `LiteralValue::String`
                                        // recovery path implicit in the
                                        // `IndexKey::String` arm above.
                                        match self.graph().node_data(resolved).as_deref() {
                                            Some(SemanticNodeData::Literal(
                                                crate::semantic_query::LiteralValue::Number(n),
                                            )) => (Some(LiteralKey::Number(*n)), false),
                                            Some(SemanticNodeData::Literal(
                                                crate::semantic_query::LiteralValue::String(s),
                                            )) => (
                                                Some(LiteralKey::String(Arc::<str>::from(
                                                    s.as_str(),
                                                ))),
                                                true,
                                            ),
                                            _ => (None, false),
                                        }
                                    }
                                }
                            }
                            None => (None, false),
                        };
                    // Path-precise key-domain admission. Three-tier
                    // check, applied only when we have a literal_name
                    // (without a literal we cannot perform the K =
                    // Literal(name) substitution at all):
                    //
                    //   1. If the source surface is an enumerable
                    //      Object, its member names ARE the iteration
                    //      keys — admit iff the literal is present.
                    //   2. Otherwise, if `key_space` enumerates (string
                    //      literal union, keyof of an Object, …), admit
                    //      iff the literal is in the enumerated set.
                    //   3. Otherwise, if `key_space` is a non-enumerable
                    //      primitive (`string`, `number`, `any`,
                    //      `unknown`, or a union of these), admit iff
                    //      the primitive's domain accepts the segment's
                    //      domain — `Record<string, V>['foo']`,
                    //      `Record<number, V>[1]`, etc. Without this
                    //      tier, the coarse Mapped path re-interns the
                    //      same shell and the walker fails to consume
                    //      the segment, breaking idiomatic indexed
                    //      access into primitive-keyed maps.
                    //
                    // `None` from the chain means the key domain is
                    // genuinely undecidable (e.g. an unresolved generic
                    // bound) — fall back to the coarse path which
                    // produces a deferred shell re-dispatched once the
                    // domain becomes enumerable.
                    let key_admitted: Option<bool> = literal_key.as_ref().map(|key| {
                        // For Tier 1/2 enumerable lookups, both string
                        // and numeric segments compare against textual
                        // member names — TypeScript Object members are
                        // string-keyed at the type level (numeric
                        // indices coerce to their string form, so
                        // `{ '1': X }[1]` matches member `"1"`).
                        //
                        // Numeric-to-string rendering for the integer
                        // fast path (`n.fract() == 0.0`) avoids the
                        // f64 default `Display` impl emitting `"1"` as
                        // `"1"` vs `"1.0"` ambiguity by formatting via
                        // i64 truncation; non-integer numerics fall
                        // back to the default `Display`.
                        let needle_text: String = match key {
                            LiteralKey::String(s) => s.as_ref().to_string(),
                            LiteralKey::Number(n) => {
                                if n.fract() == 0.0
                                    && *n >= i64::MIN as f64
                                    && *n <= i64::MAX as f64
                                {
                                    (*n as i64).to_string()
                                } else {
                                    n.to_string()
                                }
                            }
                        };
                        let needle: &str = needle_text.as_str();
                        // Tier 1: source surface is an enumerable Object.
                        if let Some(SemanticNodeData::Object(view)) =
                            self.graph().node_data(*source).as_deref()
                        {
                            return view.members.iter().any(|m| m.name.as_ref() == needle);
                        }
                        // Tier 2: key_space enumerates.
                        if let Some(names) =
                            self.dispatch.key_names_from_keyspace_node(mapper.key_space)
                        {
                            return names.iter().any(|n| n.as_ref() == needle);
                        }
                        // Tier 3: key_space is a non-enumerable
                        // primitive whose domain admits the segment.
                        self.dispatch.primitive_keyspace_admits_segment(
                            mapper.key_space,
                            segment_is_string_domain,
                        )
                    });
                    let can_narrow = literal_key.is_some()
                        && mapper.name_remap.is_none()
                        && matches!(key_admitted, Some(true));
                    if can_narrow {
                        let key = literal_key.expect("literal_key is_some checked above");
                        // Preserve string-vs-number literal kind through
                        // substitution. `M['1']` and `M[1]` are
                        // semantically distinct keys; the substituted
                        // K must carry the originating segment's kind
                        // so any value expression that depends on K
                        // (identity mapping, `K extends ...`, template
                        // literal positions, …) instantiates with the
                        // correct LiteralValue variant.
                        let key_literal_value = match &key {
                            LiteralKey::String(s) => {
                                crate::semantic_query::LiteralValue::String(s.as_ref().to_string())
                            }
                            // `LiteralKey::Number` already carries the
                            // recovered f64 (construction sites pick
                            // the convention-specific bits→f64 vs
                            // int→f64 conversion). Intern the matching
                            // `LiteralValue::Number` variant — `M[1]`
                            // substitutes K = `Literal::Number(1)`,
                            // NOT `Literal::String("1")`.
                            LiteralKey::Number(n) => {
                                crate::semantic_query::LiteralValue::Number(*n)
                            }
                        };
                        let key_arg = self
                            .graph()
                            .intern_node(SemanticNodeData::Literal(key_literal_value));
                        let substituted = self.dispatch.substitute_semantic_type_param(
                            mapper.value_expr,
                            mapper.parameter_node,
                            key_arg,
                        );
                        // Block 6.i Commit AX (codex Q5): context-
                        // explicit evaluation inherits the path-walker's
                        // `mode` as a `Published(mode)` context.
                        // Mirrors `build_mapped_type`'s substitution
                        // contract — publication walks reify nested
                        // operators; transit walks carrier-stop. The
                        // implicit `Published(Expanded)` default is
                        // replaced so the walker's actual mode flows
                        // through.
                        let evaluated = self.dispatch.evaluate_deferred_semantic_node_with_context(
                            substituted,
                            crate::semantic_query::ProjectionReductionContext::published(
                                self.mode(),
                            ),
                        );
                        // Preserve `build_mapped_type`'s contract:
                        // when evaluation yields Opaque, publish the
                        // un-evaluated substituted node so re-dispatch
                        // can pick up the lazy form once inputs become
                        // enumerable.
                        let value = if matches!(
                            self.graph().node_data(evaluated).as_deref(),
                            Some(SemanticNodeData::Opaque(_))
                        ) {
                            substituted
                        } else {
                            evaluated
                        };
                        // Emit the per-key edge mirroring
                        // `build_mapped_type`'s ProjectMember edge so
                        // downstream origin-graph consumers see the
                        // same per-key contribution.
                        let edge_kind = match next_segment.expect("checked above") {
                            PathSegment::Member(_) => OriginEdgeKind::ProjectMember,
                            PathSegment::Index(_) => OriginEdgeKind::ProjectIndex,
                        };
                        let meta = match next_segment.expect("checked above") {
                            PathSegment::Member(member_name) => {
                                OriginMeta::MemberName(Arc::clone(member_name))
                            }
                            PathSegment::Index(ix) => OriginMeta::Index(ix.clone()),
                        };
                        self.graph().record_origin_edge(
                            value,
                            edge_kind,
                            Arc::from(vec![current, *source, mapper.key_space].into_boxed_slice()),
                            meta,
                            Arc::clone(self.fence),
                        );
                        current = value;
                        index += 1;
                        // Record the per-segment intermediate node —
                        // the linear member-step backfill contract.
                        self.intermediate_nodes.push(Some(current));
                        continue;
                    }
                    // Fallback: whole-surface MappedType resolution.
                    // Used when narrowing is unsafe or not applicable:
                    // - terminal hop with no remaining segment (caller
                    //   requested the whole mapped surface),
                    // - `mapper.name_remap` is set (post-remap surface
                    //   name lookup is not 1:1 to iteration key),
                    // - the next path segment is an unresolvable
                    //   TypeNode index, or
                    // - the literal key is not admitted by the mapper's
                    //   key domain (substituting a non-admissible key
                    //   would forge a value type for a non-existent
                    //   member; the coarse path produces the correct
                    //   Object miss instead).
                    let resolved = match self.dispatch.execute(SemanticQueryKey::MappedType {
                        source: *source,
                        mapper: mapper.clone(),
                        context: crate::semantic_query::ProjectionReductionContext::published(
                            self.mode(),
                        ),
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
                            context: crate::semantic_query::ProjectionReductionContext::published(
                                self.mode(),
                            ),
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
                    // visited_aliases set.
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
                //
                // Block 6.i Commit AX — intermediate-hop demand demotion.
                // We are inside `while index < path.len()`: a path
                // segment is still pending, so this unwrap is an
                // INTERMEDIATE hop in the type-resolution rule
                // "intermediate hops run in Navigate, the terminal hop
                // runs in the caller's mode." Dispatch under
                // `structural_transit()` so the body lowers in
                // transit demand — `keyof` / `Mapped` operators along
                // the decl body publish carriers (no reify) and never
                // reach the publication-edge loops. The terminal
                // expander (`expand_empty_path_terminal`,
                // walk.rs:1403) keeps `published(self.mode)` for
                // empty-path terminal demand.
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
                        context:
                            crate::semantic_query::ProjectionReductionContext::structural_transit(),
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
                // D26 lazy carriers.
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
                    // Block 6.i Commit AX (codex Q4 — IA path-precision).
                    // "Intermediate hops are navigate-only, terminal uses
                    // caller mode." When there is still a `PathSegment`
                    // ahead of us, the InstantiationRef is an
                    // INTERMEDIATE hop in the path walk — we MUST unwrap
                    // it through `Instantiate` so the next segment can
                    // pattern-match on the body's surface. When the
                    // walker reaches a USERLAND InstantiationRef at the
                    // terminal hop under Navigate, it stays terminal
                    // (the codex carrier-preservation contract for
                    // generic-application bodies under shallow-by-
                    // default publication). Builtin utility types (
                    // `Pick`/`Omit`/`Partial`/…, `canonical_id ==
                    // "__builtin__"`) ALWAYS unwrap even under
                    // Navigate per codex Q4 ("the demanded
                    // instantiation is reduced as the terminal").
                    let still_more_path = index < path.len();
                    let is_builtin = base.canonical_id.as_ref() == "__builtin__";
                    if matches!(self.mode(), ProjectionMode::Navigate)
                        && !still_more_path
                        && !is_builtin
                    {
                        results.push(current);
                        return;
                    }
                    let identity = base.clone();
                    let args_clone = Arc::clone(args);
                    drop(data);
                    // Block 6.i Commit AX — intermediate-hop demand
                    // demotion (codex spec). An InstantiationRef
                    // unwrapped with a path segment still pending is
                    // an INTERMEDIATE hop — dispatch under
                    // `structural_transit()` so the body lowers in
                    // transit demand and nested `keyof` / `Mapped`
                    // operators carrier-stop without reifying their
                    // members. The terminal unwrap (no more segments,
                    // walker about to return `current`) keeps the
                    // caller's mode under `published(self.mode)` so a
                    // genuine terminal `Mapped`/`KeyOf` prop still
                    // reduces.
                    //
                    // The previous "upgrade Navigate→Expanded" rule
                    // was wrong on two axes: it only fired for Navigate
                    // callers (Expanded intermediate hops kept
                    // `published(Expanded)` and reified) and it
                    // *upgraded* rather than *demoted* — the root
                    // cause of the ChatMessages `outputSchema|execute`
                    // 62-edge leak in `compute_evaluated_types`.
                    let unwrap_context = if still_more_path {
                        crate::semantic_query::ProjectionReductionContext::structural_transit()
                    } else {
                        crate::semantic_query::ProjectionReductionContext::published(self.mode())
                    };
                    let resolved = match self.dispatch.execute(SemanticQueryKey::Instantiate {
                        base: identity,
                        args: args_clone,
                        context: unwrap_context,
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
                    // semantic work (C3 path-walker + ). The
                    // shell carriers (B4) exist so the graph publishes
                    // these shapes first-class; deeper projection lands
                    // in later phases. Return Opaque(Miss) for now.
                    results.push(self.opaque_miss());
                    return;
                }
            }
        }
        // For `mode: Expanded` with empty path, expand terminal
        // declaration placeholders via `Instantiate(decl, [])` and
        // recurse through Intersection / Union arms so nested
        // `extends` / union-arm refs surface their body.
        //
        // For `mode: Shallow` with empty path, synthesise a one-level
        // merged Object surface from the structural carriers. The
        // walker iterates a heap-backed worklist (no recursion), merges
        // per-arm contributions under TS rules (intersection: union of
        // members + required-wins + readonly-OR; union: intersection of
        // members), and emits diagnostics for cycles / open
        // conditionals / pathological inputs.
        //
        // Identity / Navigate / Skeleton retain their bare carriers —
        // those modes' contracts promise no terminal-surface synthesis.
        if matches!(self.mode(), ProjectionMode::Expanded) {
            current = self.expand_empty_path_terminal(current);
        } else if matches!(self.mode(), ProjectionMode::Shallow) {
            current = self.expand_empty_path_shallow_terminal_surface(current);
        }
        results.push(current);
    }

    /// Combine the top `arm_count` entries from `results` into the
    /// union of the arm projections. C3 union rule: any arm
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
    /// intersection contributor rule: opaque arms drop,
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
    /// **Mapped arm.** When the walker runs in
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
                if self.dispatch.ctx.workspace_is_package_backed(canonical_id) {
                    drop(data);
                    results.push(node);
                    return;
                }
                drop(data);
                let expanded = match self.dispatch.execute(SemanticQueryKey::Instantiate {
                    base: identity,
                    args: Arc::from(Vec::<SemanticNodeId>::new().into_boxed_slice()),
                    context: crate::semantic_query::ProjectionReductionContext::published(
                        self.mode(),
                    ),
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
            // Block 6.i Commit AX — `DeclRef`/`InstantiationRef`
            // arms are deliberately NOT added to
            // `expand_terminal_step`. Eagerly unwrapping these
            // carriers in the empty-path terminal under publication
            // demand collapsed the slot-binding indexed-access
            // preservation policy (the symbolic
            // `IndexedAccess { object: DeclRef(AppProps), index }`
            // form). The carrier-preserving path-walker arms above
            // (`PathWalker::advance_step` at walk.rs:1204-1313)
            // handle DeclRef/InstantiationRef under demand-bounded
            // intermediate-vs-terminal selection; the empty-path
            // terminal expander does not need a duplicate arm.
            SemanticNodeData::Mapped { source, mapper }
                if matches!(self.mode(), ProjectionMode::Expanded) =>
            {
                let source = *source;
                let mapper = mapper.clone();
                drop(data);
                let materialised = match self.dispatch.execute(SemanticQueryKey::MappedType {
                    source,
                    mapper,
                    context: crate::semantic_query::ProjectionReductionContext::published(
                        self.mode(),
                    ),
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
                if materialised == node {
                    results.push(node);
                    return;
                }
                work.push(ExpandFrame::Expand(materialised));
            }
            // Open Conditional at empty-path terminal in
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
            } if matches!(self.mode(), ProjectionMode::Expanded)
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

    // ──────────────────────────────────────────────────────────────────
    //  Shallow-mode terminal-surface synthesis (iterative).
    //
    //  Drives a heap-backed worklist that walks Object / Intersection /
    //  Union / InstantiationRef / Conditional / Mapped / Alias arms and
    //  emits one merged `SemanticNodeData::Object` surface per request.
    //  The synthesiser is intentionally non-recursive: stack depth is
    //  O(1) for inputs of any width because the worklist lives on the
    //  heap. The 100-arm intersection invariant is enforced by
    //  `shallow_walker_stack_depth_bounded_for_100_intersection`.
    //
    //  Pathological-input cap: the walker tracks a `visited` set keyed
    //  by `(node, target_marker)`. When the cap (10_000 entries) fires,
    //  the walker emits `ShallowDiagnostic::PathologicalInput`, sets
    //  `cache_suppress = true`, and stops the worklist. The dispatch
    //  layer threads `cache_suppress` into `QueryBuildOutput` so the
    //  memo refuses warm publish.
    // ──────────────────────────────────────────────────────────────────

    /// Iterative shallow-mode terminal-surface synthesis. Returns the
    /// interned `SemanticNodeData::Object` id whose surface holds the
    /// merged members per the per-arm semantics described above.
    fn expand_empty_path_shallow_terminal_surface(
        &mut self,
        node: SemanticNodeId,
    ) -> SemanticNodeId {
        let span = tracing::debug_span!(
            target: "verter::dispatch::walk",
            "walk_shallow_surface",
            root = ?node,
            mode = "Shallow"
        );
        let _enter = span.enter();

        // Production default. Tests may opt in to a smaller cap via
        // `HostConfig::recursion_budget_overrides.walker_pathological_cap`
        // so the cap-fire path is reachable on hermetic fixtures
        // without requiring a 10_000-node corpus.
        const PATHOLOGICAL_CAP_DEFAULT: usize = 10_000;
        let pathological_cap: usize = self
            .dispatch
            .ctx
            .config()
            .recursion_budget_overrides
            .walker_pathological_cap
            .unwrap_or(PATHOLOGICAL_CAP_DEFAULT);

        // Per-arm buffers: each Intersection / Union allocates a fresh
        // buffer id; arm Visit frames push their result into the
        // corresponding slot via `contribute_surface`. The Flush*
        // frames consume the buffer and push the merged surface up to
        // the parent target.
        let mut intersection_buffers: rustc_hash::FxHashMap<usize, Vec<Option<ShallowSurface>>> =
            rustc_hash::FxHashMap::default();
        let mut union_buffers: rustc_hash::FxHashMap<usize, Vec<Option<ShallowSurface>>> =
            rustc_hash::FxHashMap::default();
        let mut next_buffer_id: usize = 0;

        // Root contribution slot. Holds the synthesised surface once
        // the worklist drains. None until the walker assigns to it.
        let mut root_contribution: Option<ShallowSurface> = None;

        // Cycle / idempotency detection: keyed by the visited node id
        // and the target slot it would contribute to. Repeating the
        // same `(node, target)` pair indicates either a true graph
        // cycle (Foo<T> = { self: Foo<T> }) or an idempotent arm
        // (T & T contributes once). We short-circuit either case with
        // a diagnostic and an empty contribution.
        let mut visited: rustc_hash::FxHashSet<(SemanticNodeId, BufferTarget)> =
            rustc_hash::FxHashSet::default();

        // Frame-depth high-water mark for the probe used by
        // `shallow_walker_stack_depth_bounded_for_100_intersection`.
        // Reset to 0 each call so concurrent-safe single-threaded use
        // observes only the current walk's depth.
        let mut max_depth: usize = 0;
        LAST_SHALLOW_WALKER_MAX_FRAMES.store(0, Ordering::Relaxed);

        // Worklist seeded with a Visit on the input node, contributing
        // to the root slot.
        let mut work: Vec<Frame> = Vec::with_capacity(8);
        work.push(Frame::Visit {
            node,
            target: BufferTarget::Root,
        });

        while let Some(frame) = work.pop() {
            if work.len() + 1 > max_depth {
                max_depth = work.len() + 1;
            }
            // Pathological-input guard.
            if visited.len() >= pathological_cap {
                tracing::warn!(
                    target: "verter::dispatch::walk",
                    root = ?node,
                    visited_count = visited.len(),
                    cap = pathological_cap,
                    "walker_pathological_input_cap"
                );
                self.walker_diagnostics
                    .push(ShallowDiagnostic::PathologicalInput { root: node });
                self.cache_suppress = true;
                break;
            }
            match frame {
                Frame::Visit { node: cur, target } => {
                    tracing::trace!(
                        target: "verter::dispatch::walk",
                        node = ?cur,
                        target = ?target,
                        "walker_visit"
                    );
                    if !visited.insert((cur, target)) {
                        // Same `(node, target)` already visited — a
                        // duplicate arm or a graph cycle. Contribute
                        // empty so the merge does not stall on a self-
                        // reference, and emit a structured diagnostic.
                        let diag = ShallowDiagnostic::DuplicateArmShortCircuited { node: cur };
                        self.walker_diagnostics.push(diag);
                        self.contribute_surface(
                            target,
                            &mut root_contribution,
                            &mut intersection_buffers,
                            &mut union_buffers,
                            None,
                        );
                        continue;
                    }
                    self.visit_shallow_node(
                        cur,
                        target,
                        &mut work,
                        &mut intersection_buffers,
                        &mut union_buffers,
                        &mut next_buffer_id,
                        &mut root_contribution,
                    );
                }
                Frame::VisitArmAt {
                    arms,
                    arm_index,
                    buffer_id,
                    kind,
                } => {
                    if arm_index >= arms.len() {
                        // No more arms — the queued FlushIntersection /
                        // FlushUnion (sitting under this frame in the
                        // worklist) drains the buffer.
                        continue;
                    }
                    let target = match kind {
                        ArmKind::Intersection => BufferTarget::Intersection {
                            buffer_id,
                            arm_index,
                        },
                        ArmKind::Union => BufferTarget::Union {
                            buffer_id,
                            arm_index,
                        },
                    };
                    let arm = arms[arm_index];
                    // Re-queue the iterator for the next arm BEFORE
                    // pushing the Visit so LIFO pop order processes
                    // the current arm first; the next-arm iterator
                    // sits beneath it on the worklist.
                    if arm_index + 1 < arms.len() {
                        work.push(Frame::VisitArmAt {
                            arms,
                            arm_index: arm_index + 1,
                            buffer_id,
                            kind,
                        });
                    }
                    work.push(Frame::Visit { node: arm, target });
                }
                Frame::FlushIntersection {
                    buffer_id,
                    parent_target,
                } => {
                    let arm_surfaces = intersection_buffers.remove(&buffer_id).unwrap_or_default();
                    let merged =
                        merge_intersection_surfaces_with_graph(self.graph(), &arm_surfaces);
                    self.contribute_surface(
                        parent_target,
                        &mut root_contribution,
                        &mut intersection_buffers,
                        &mut union_buffers,
                        merged,
                    );
                }
                Frame::FlushUnion {
                    buffer_id,
                    parent_target,
                    union_node,
                } => {
                    let arm_surfaces = union_buffers.remove(&buffer_id).unwrap_or_default();
                    // Emit a UnionArmEmpty diagnostic per arm that
                    // produced no contribution — informs consumers that
                    // a non-Object arm was encountered.
                    for (arm_index, surf) in arm_surfaces.iter().enumerate() {
                        if surf.is_none() {
                            self.walker_diagnostics
                                .push(ShallowDiagnostic::UnionArmEmpty {
                                    union_node,
                                    arm_index,
                                });
                        }
                    }
                    let merged = merge_union_surfaces(&arm_surfaces);
                    self.contribute_surface(
                        parent_target,
                        &mut root_contribution,
                        &mut intersection_buffers,
                        &mut union_buffers,
                        merged,
                    );
                }
            }
        }

        LAST_SHALLOW_WALKER_MAX_FRAMES.store(max_depth, Ordering::Relaxed);

        // Materialise the root contribution into a SurfaceView and
        // intern it. Empty contribution → empty Object surface.
        let surface_view = match root_contribution {
            Some(surface) => surface_view_from_shallow(&surface),
            None => empty_surface_view(),
        };
        self.graph()
            .intern_node(SemanticNodeData::Object(surface_view))
    }

    /// Visit one node in the shallow-mode worklist. Branches per node
    /// shape and either contributes a surface immediately (Object,
    /// Function, Primitive, Literal, TypeParam, Infer, etc.) or pushes
    /// child Visits + a flush frame onto the worklist (Intersection,
    /// Union, Mapped, InstantiationRef, Conditional, Alias).
    #[allow(clippy::too_many_arguments)]
    fn visit_shallow_node(
        &mut self,
        cur: SemanticNodeId,
        target: BufferTarget,
        work: &mut Vec<Frame>,
        intersection_buffers: &mut rustc_hash::FxHashMap<usize, Vec<Option<ShallowSurface>>>,
        union_buffers: &mut rustc_hash::FxHashMap<usize, Vec<Option<ShallowSurface>>>,
        next_buffer_id: &mut usize,
        root_contribution: &mut Option<ShallowSurface>,
    ) {
        let data = match self.graph().node_data(cur) {
            Some(d) => d,
            None => {
                self.contribute_surface(
                    target,
                    root_contribution,
                    intersection_buffers,
                    union_buffers,
                    None,
                );
                return;
            }
        };
        match &*data {
            SemanticNodeData::Object(view) => {
                let surface = ShallowSurface::from_object(view);
                drop(data);
                self.contribute_surface(
                    target,
                    root_contribution,
                    intersection_buffers,
                    union_buffers,
                    Some(surface),
                );
            }
            SemanticNodeData::Intersection(arms) => {
                let arms = Arc::clone(arms);
                drop(data);
                let buffer_id = *next_buffer_id;
                *next_buffer_id += 1;
                intersection_buffers.insert(buffer_id, vec![None; arms.len()]);
                // Push the flush frame BEFORE the iterator frame so
                // LIFO pop order executes the flush after every arm
                // has contributed.
                work.push(Frame::FlushIntersection {
                    buffer_id,
                    parent_target: target,
                });
                if !arms.is_empty() {
                    work.push(Frame::VisitArmAt {
                        arms,
                        arm_index: 0,
                        buffer_id,
                        kind: ArmKind::Intersection,
                    });
                }
            }
            SemanticNodeData::Union(arms) => {
                let arms = Arc::clone(arms);
                drop(data);
                let buffer_id = *next_buffer_id;
                *next_buffer_id += 1;
                union_buffers.insert(buffer_id, vec![None; arms.len()]);
                work.push(Frame::FlushUnion {
                    buffer_id,
                    parent_target: target,
                    union_node: cur,
                });
                if !arms.is_empty() {
                    work.push(Frame::VisitArmAt {
                        arms,
                        arm_index: 0,
                        buffer_id,
                        kind: ArmKind::Union,
                    });
                }
            }
            SemanticNodeData::InstantiationRef { base, args } => {
                let identity = base.clone();
                let args_clone = Arc::clone(args);
                drop(data);
                // Skeleton mode: unbound TypeParam arguments stay
                // symbolic so Conditional branches don't collapse to
                // `never`. The Skeleton-mode contract mandates
                // dispatch with empty args for shallow-surface
                // synthesis to keep generic helpers' Conditional-arm
                // distribution intact.
                match self.dispatch.execute(SemanticQueryKey::Instantiate {
                    base: identity.clone(),
                    args: args_clone,
                    context: crate::semantic_query::ProjectionReductionContext::published(
                        ProjectionMode::Navigate,
                    ),
                }) {
                    QueryResult::Value(body) => {
                        // Continue the walk into the materialised body.
                        work.push(Frame::Visit { node: body, target });
                    }
                    QueryResult::Recursive(_) => {
                        self.walker_diagnostics
                            .push(ShallowDiagnostic::CyclicInstantiation { decl: identity });
                        self.contribute_surface(
                            target,
                            root_contribution,
                            intersection_buffers,
                            union_buffers,
                            None,
                        );
                    }
                    QueryResult::Error(error) => {
                        if is_fatal_query_error(&error) {
                            self.cache_suppress = true;
                        }
                        self.walker_diagnostics
                            .push(ShallowDiagnostic::InstantiationError {
                                decl: identity,
                                error,
                            });
                        self.contribute_surface(
                            target,
                            root_contribution,
                            intersection_buffers,
                            union_buffers,
                            None,
                        );
                    }
                }
            }
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
                match self.dispatch.execute(SemanticQueryKey::Instantiate {
                    base: identity.clone(),
                    args: Arc::from(Vec::<SemanticNodeId>::new().into_boxed_slice()),
                    context: crate::semantic_query::ProjectionReductionContext::published(
                        ProjectionMode::Navigate,
                    ),
                }) {
                    QueryResult::Value(body) => {
                        work.push(Frame::Visit { node: body, target });
                    }
                    QueryResult::Recursive(_) => {
                        self.walker_diagnostics
                            .push(ShallowDiagnostic::CyclicInstantiation { decl: identity });
                        self.contribute_surface(
                            target,
                            root_contribution,
                            intersection_buffers,
                            union_buffers,
                            None,
                        );
                    }
                    QueryResult::Error(error) => {
                        if is_fatal_query_error(&error) {
                            self.cache_suppress = true;
                        }
                        self.walker_diagnostics
                            .push(ShallowDiagnostic::InstantiationError {
                                decl: identity,
                                error,
                            });
                        self.contribute_surface(
                            target,
                            root_contribution,
                            intersection_buffers,
                            union_buffers,
                            None,
                        );
                    }
                }
            }
            SemanticNodeData::Conditional {
                check,
                true_branch_ref,
                false_branch_ref,
                ..
            } => {
                // Open conditional: check is unbound (TypeParam / Infer).
                // The empty-path Shallow contract for an open conditional
                // is an empty surface — branch selection is impossible
                // until the check resolves.
                //
                // Closed conditional: the check is concrete; ask the
                // relation engine for the branch and walk that. This
                // mirrors the `Conditional` arm handling in the
                // pre-§3.1.3 expand_terminal_step but stays inside the
                // shallow synthesis worklist instead of recursing.
                let check_id = *check;
                let true_branch = *true_branch_ref;
                let false_branch = *false_branch_ref;
                drop(data);
                let check_data = self.graph().node_data(check_id);
                let is_open = matches!(
                    check_data.as_deref(),
                    Some(SemanticNodeData::TypeParam { .. } | SemanticNodeData::Infer { .. })
                );
                drop(check_data);
                if is_open {
                    self.walker_diagnostics
                        .push(ShallowDiagnostic::OpenConditional { node: cur });
                    self.contribute_surface(
                        target,
                        root_contribution,
                        intersection_buffers,
                        union_buffers,
                        Some(ShallowSurface::empty()),
                    );
                } else {
                    // Closed conditional reduces immediately; the
                    // pre-distribution build_conditional already
                    // returned the selected branch as the result, so
                    // hitting a Conditional shell here means the check
                    // is concrete but the relation engine returned
                    // Unknown. Distribute into both branches via Union
                    // using the iterator-frame discipline.
                    let buffer_id = *next_buffer_id;
                    *next_buffer_id += 1;
                    union_buffers.insert(buffer_id, vec![None; 2]);
                    work.push(Frame::FlushUnion {
                        buffer_id,
                        parent_target: target,
                        union_node: cur,
                    });
                    let arms: Arc<[SemanticNodeId]> =
                        Arc::from(vec![true_branch, false_branch].into_boxed_slice());
                    work.push(Frame::VisitArmAt {
                        arms,
                        arm_index: 0,
                        buffer_id,
                        kind: ArmKind::Union,
                    });
                }
            }
            SemanticNodeData::Mapped { source, mapper } => {
                let source = *source;
                let mapper = mapper.clone();
                drop(data);
                // Per-key substitution at the Shallow Mapped surface
                // boundary. `synthesise_mapped_surface` takes the full
                // [`MapperKey`] (not just `value_expr`) so it can
                // substitute the mapper binder with each enumerated
                // key literal and materialise the substituted value
                // just enough to close the selected key — preventing
                // publication of mapped members whose `value` still
                // contains the free mapper binder. Gated on Published
                // demand: StructuralTransit walks return None (transit
                // is the non-publication rail; mapped enumeration is
                // publication work).
                let surface = self.synthesise_mapped_surface(source, &mapper);
                self.contribute_surface(
                    target,
                    root_contribution,
                    intersection_buffers,
                    union_buffers,
                    surface,
                );
            }
            SemanticNodeData::Alias(alias_target) => {
                let target_id = *alias_target;
                drop(data);
                work.push(Frame::Visit {
                    node: target_id,
                    target,
                });
            }
            SemanticNodeData::DeclRef { identity } => {
                let scope = ScopeId {
                    canonical_id: Arc::clone(&identity.canonical_id),
                    local_scope: None,
                };
                let name = Arc::clone(&identity.decl_name);
                drop(data);
                match self
                    .dispatch
                    .execute(SemanticQueryKey::ResolveDecl(ResolveDeclKey {
                        scope,
                        name,
                    })) {
                    QueryResult::Value(resolved) => {
                        if resolved == cur {
                            self.contribute_surface(
                                target,
                                root_contribution,
                                intersection_buffers,
                                union_buffers,
                                None,
                            );
                        } else {
                            work.push(Frame::Visit {
                                node: resolved,
                                target,
                            });
                        }
                    }
                    QueryResult::Recursive(_) | QueryResult::Error(_) => {
                        self.contribute_surface(
                            target,
                            root_contribution,
                            intersection_buffers,
                            union_buffers,
                            None,
                        );
                    }
                }
            }
            // Non-Object terminals contribute nothing to the merged
            // surface — under TS rules a primitive arm in an
            // intersection drops out (the contributor rule), and a
            // primitive arm in a union becomes a `UnionArmEmpty`
            // diagnostic at flush time.
            SemanticNodeData::Primitive(_)
            | SemanticNodeData::Literal(_)
            | SemanticNodeData::Opaque(_)
            | SemanticNodeData::VueMacroElements(_)
            | SemanticNodeData::Array { .. }
            | SemanticNodeData::Tuple { .. }
            | SemanticNodeData::TemplateLiteral { .. }
            | SemanticNodeData::TypeParam { .. }
            | SemanticNodeData::Infer { .. }
            | SemanticNodeData::Function { .. }
            | SemanticNodeData::KeyOf { .. }
            | SemanticNodeData::IndexedAccess { .. }
            | SemanticNodeData::TypeOf { .. } => {
                drop(data);
                self.contribute_surface(
                    target,
                    root_contribution,
                    intersection_buffers,
                    union_buffers,
                    None,
                );
            }
        }
    }

    /// Route a contribution to the appropriate slot. Root contributions
    /// merge with the existing root surface (intersection-style merge);
    /// arm contributions land in the per-buffer arm slot.
    fn contribute_surface(
        &mut self,
        target: BufferTarget,
        root_contribution: &mut Option<ShallowSurface>,
        intersection_buffers: &mut rustc_hash::FxHashMap<usize, Vec<Option<ShallowSurface>>>,
        union_buffers: &mut rustc_hash::FxHashMap<usize, Vec<Option<ShallowSurface>>>,
        contribution: Option<ShallowSurface>,
    ) {
        match target {
            BufferTarget::Root => {
                if let Some(surface) = contribution {
                    *root_contribution = Some(surface);
                }
                // None contribution at root with no prior — leave None;
                // the final surface will be empty Object.
            }
            BufferTarget::Intersection {
                buffer_id,
                arm_index,
            } => {
                if let Some(buf) = intersection_buffers.get_mut(&buffer_id) {
                    if arm_index < buf.len() {
                        buf[arm_index] = contribution;
                    }
                }
            }
            BufferTarget::Union {
                buffer_id,
                arm_index,
            } => {
                if let Some(buf) = union_buffers.get_mut(&buffer_id) {
                    if arm_index < buf.len() {
                        buf[arm_index] = contribution;
                    }
                }
            }
        }
    }

    /// Synthesise a Mapped-shape surface from the dispatched key-space.
    ///
    /// **Per-key Mapped substitution at the Shallow walker.**
    ///
    /// For each string-literal key that the dispatched `KeyOf(source)`
    /// or the direct `key_space` exposes, substitute the mapper binder
    /// (`mapper.parameter_node`) with the key literal in
    /// `mapper.value_expr`, then materialise the substituted node only
    /// enough to close the selected key via the shared
    /// [`ProjectSemanticDispatch::materialize_mapped_member_value_for_key`]
    /// helper. The materialisation runs under
    /// `structural_transit_with_mode(Navigate)`: nested `KeyOf` /
    /// `MappedType` operators carrier-stop via
    /// [`crate::semantic_query::may_reduce_operator`], while
    /// `Conditional` reduction (gated separately by the relation
    /// engine) still fires — turning a substituted
    /// `ExtendSlotWithPlan<TPlan, "badge">` into the `Function` surface
    /// the slot-binding extractor reads. Returns `None` when the
    /// key-space cannot be enumerated (open generic, infinite, etc.)
    /// OR when the caller's demand is `StructuralTransit` (transit is
    /// the non-publication rail; mapped enumeration is publication
    /// work that no transit-mode caller selects through).
    ///
    /// The architectural amendment closes the publication boundary
    /// defect: a prior implementation published mapped members whose
    /// `value` was the raw `mapper.value_expr` — still containing the
    /// free mapper binder `K`. Downstream consumers
    /// (`compute_bindings_via_graph`,
    /// `surface_member_to_expanded_field`, model/reducer paths,
    /// component-meta materialisation) saw `InstantiationRef` shells
    /// where `Function` shapes were expected, and the slot-binding
    /// extractor fell through the `_ => continue` arm. Per-key
    /// substitution at the producer fixes ALL consumer paths
    /// uniformly.
    fn synthesise_mapped_surface(
        &mut self,
        source: SemanticNodeId,
        mapper: &crate::semantic_query::MapperKey,
    ) -> Option<ShallowSurface> {
        // Boundary constraint: per-key Mapped substitution is
        // publication work and MUST only run for `Published` demand.
        // `StructuralTransit(Shallow)` walks carrier-stop here
        // without enumeration; the transit demand is the
        // non-publication rail and no consumer reads the synthesised
        // surface on that path.
        if !crate::semantic_query::may_reduce_operator(self.context) {
            return None;
        }
        // Prefer the explicit `key_space` for fast literal-union
        // collection; fall back to the shared key-name enumerator on
        // the SOURCE so the Shallow walker enumerates member names
        // for mapped types whose source is a deferred shell
        // (`Opaque(DeclPlaceholder)` from an imported interface,
        // `InstantiationRef` from a generic alias body, etc.). The
        // pre-AX walker bailed on these cases via a `collect_literal_keys`-only
        // fallback, which left imported `[K in keyof Foo]?: V` mapped
        // arms unenumerated under empty-path Shallow.
        // `key_names_from_base_node` is the same enumerator
        // `build_mapped_type` uses (`enumerate.rs`); routing the
        // Shallow synthesiser through it keeps the two paths
        // structurally aligned.
        let mut keys: Vec<Arc<str>> = Vec::new();
        let collected = collect_literal_keys(self.graph(), mapper.key_space, &mut keys);
        if !collected {
            // The shared enumerator on the SOURCE handles
            // `Opaque(DeclPlaceholder)` / `Object` / `Intersection` /
            // `Union` uniformly. Block 6.i Commit AX (codex-binding) —
            // empty-path Shallow with an imported mapped carrier MUST
            // enumerate just like the Expanded path's MappedType
            // dispatch.
            keys.clear();
            match self.dispatch.key_names_from_base_node(source) {
                Some(enumerated) => keys = enumerated,
                None => return None,
            }
        }
        if keys.is_empty() {
            return None;
        }
        // Per-key Mapped materialisation context.
        //
        // `structural_transit_with_mode(Navigate)`:
        // - `StructuralTransit` demand → `may_reduce_operator` is
        //   `false`, so nested `KeyOf` / `MappedType` re-dispatches
        //   carrier-stop in the substituted body (no spurious
        //   keyspace-literal anchor emissions).
        // - `Navigate` mode → intermediate hops stay carrier-shaped;
        //   only `Conditional` reduction (gated separately) fires
        //   when the post-substitution check is concrete.
        //
        // The substituted value is materialised "only enough to close
        // the selected key" so `ExtendSlotWithPlan<TPlan, "badge">`
        // reduces to a `Function` node, while unrelated `keyof` /
        // `Mapped` carriers in the body never expand their member
        // surfaces.
        let materialise_context =
            crate::semantic_query::ProjectionReductionContext::structural_transit_with_mode(
                ProjectionMode::Navigate,
            );
        let optionality = mapper.optionality;
        let readonly_mod = mapper.readonly;
        let optional = matches!(optionality, crate::semantic_query::OptionalityMod::Add);
        let readonly = matches!(readonly_mod, crate::semantic_query::ReadonlyMod::Add);
        let members = keys
            .into_iter()
            .map(|name| {
                // Per-key value: substitute the binder and materialise
                // through the shared helper. Falls back to the
                // SUBSTITUTED carrier (binder replaced) on Opaque /
                // Miss — NEVER the raw `mapper.value_expr` which
                // contains the free binder.
                let value = self.dispatch.materialize_mapped_member_value_for_key(
                    mapper,
                    name.as_ref(),
                    materialise_context,
                );
                // Per-key produced name: apply `name_remap` through
                // the same shared helper so `as <expr>` clauses fold
                // identically to the Expanded path.
                let produced_name = self.dispatch.materialize_mapped_member_name_for_key(
                    mapper,
                    &name,
                    materialise_context,
                );
                ShallowSurfaceMember {
                    name: produced_name,
                    value,
                    optional,
                    readonly,
                    is_method: false,
                }
            })
            .collect();
        Some(ShallowSurface { members })
    }
}

// ──────────────────────────────────────────────────────────────────────────
// Shallow-mode terminal-surface synthesiser support types.
// ──────────────────────────────────────────────────────────────────────────

/// Where one Visit's contribution lands. Encodes the surface-merge
/// position so the worklist can route arm results without recursion.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
enum BufferTarget {
    /// Root slot — assigned to `root_contribution`.
    Root,
    /// One arm of an Intersection — `intersection_buffers[buffer_id][arm_index]`.
    Intersection { buffer_id: usize, arm_index: usize },
    /// One arm of a Union — `union_buffers[buffer_id][arm_index]`.
    Union { buffer_id: usize, arm_index: usize },
}

/// Worklist frame for the iterative shallow-mode terminal-surface
/// synthesiser.
///
/// Stack-depth invariant: an N-arm intersection / union pushes
/// **exactly two** frames at the entry hop (`VisitArmAt` + `Flush*`)
/// and the per-arm iteration replaces the `VisitArmAt` frame with the
/// next-arm `VisitArmAt` plus a single `Visit` for the current arm.
/// Stack depth therefore stays O(nesting), not O(arm_count). Nesting
/// itself is bounded by the graph topology; the
/// pathological-input cap (10_000 visited entries) is the safety rail.
#[derive(Debug)]
enum Frame {
    Visit {
        node: SemanticNodeId,
        target: BufferTarget,
    },
    /// Iterator frame for an Intersection / Union arm list. Pops at
    /// each step, pushes a `Visit` for the current arm and a fresh
    /// `VisitArmAt` for `arm_index + 1` (or returns to the queued
    /// `Flush*` frame when `arm_index == arm_count`).
    VisitArmAt {
        arms: Arc<[SemanticNodeId]>,
        arm_index: usize,
        buffer_id: usize,
        kind: ArmKind,
    },
    FlushIntersection {
        buffer_id: usize,
        parent_target: BufferTarget,
    },
    FlushUnion {
        buffer_id: usize,
        parent_target: BufferTarget,
        union_node: SemanticNodeId,
    },
}

/// Discriminant for `VisitArmAt`'s parent kind. Selects which buffer
/// (intersection / union) the arm contribution lands in.
#[derive(Debug, Clone, Copy)]
enum ArmKind {
    Intersection,
    Union,
}

/// Merge per-arm intersection surfaces under TS rules:
/// - members: union across arms; same-named members merge per-rule.
/// - optional: required wins (any required → required).
/// - readonly: OR-merge (any readonly → readonly).
/// - value: when distinct, intern a recursive merged Object surface
///   built from the contributing arms' values (one level deep).
///
/// Empty-arm surfaces are dropped (intersection contributor rule).
/// Returns `None` only when ALL arms are None — caller then treats
/// the result as a deferred / non-Object input.
fn merge_intersection_surfaces_with_graph(
    graph: &SemanticGraphStore,
    arm_surfaces: &[Option<ShallowSurface>],
) -> Option<ShallowSurface> {
    let live: Vec<&ShallowSurface> = arm_surfaces.iter().filter_map(|s| s.as_ref()).collect();
    if live.is_empty() {
        return None;
    }
    if live.len() == 1 {
        return Some(live[0].clone());
    }
    // Aggregate members by name. Track all distinct value ids per
    // member so a later pass can merge them into an Intersection
    // node when they diverge.
    let mut by_name: indexmap::IndexMap<Arc<str>, MergedMember> = indexmap::IndexMap::new();
    for surface in &live {
        for member in &surface.members {
            match by_name.get_mut(&member.name) {
                Some(existing) => {
                    existing.optional = existing.optional && member.optional;
                    existing.readonly = existing.readonly || member.readonly;
                    existing.is_method = existing.is_method || member.is_method;
                    if !existing.values.contains(&member.value) {
                        existing.values.push(member.value);
                    }
                }
                None => {
                    by_name.insert(
                        Arc::clone(&member.name),
                        MergedMember {
                            name: Arc::clone(&member.name),
                            values: vec![member.value],
                            optional: member.optional,
                            readonly: member.readonly,
                            is_method: member.is_method,
                        },
                    );
                }
            }
        }
    }
    let members: Vec<ShallowSurfaceMember> = by_name
        .into_values()
        .map(|m| {
            let value = if m.values.len() == 1 {
                m.values[0]
            } else {
                merge_value_nodes_recursive(graph, &m.values)
            };
            ShallowSurfaceMember {
                name: m.name,
                value,
                optional: m.optional,
                readonly: m.readonly,
                is_method: m.is_method,
            }
        })
        .collect();
    Some(ShallowSurface { members })
}

/// Working aggregation for one merged member during intersection
/// surface synthesis. Tracks all contributing value ids so a follow-up
/// pass can merge them when they diverge across arms.
struct MergedMember {
    name: Arc<str>,
    values: Vec<SemanticNodeId>,
    optional: bool,
    readonly: bool,
    is_method: bool,
}

/// Merge the contributing value ids into a single semantic node. When
/// the values are distinct Object surfaces, build a one-level merged
/// Object surface (analogous to TS `{x: string} & {y: number}` →
/// `{x: string, y: number}`); otherwise intern an Intersection.
fn merge_value_nodes_recursive(
    graph: &SemanticGraphStore,
    values: &[SemanticNodeId],
) -> SemanticNodeId {
    debug_assert!(values.len() >= 2);
    // If every contributing value is an `Object` surface, produce a
    // merged Object directly (one-level deep) so the consumer-visible
    // shape is a unified surface, not an `Intersection` carrier.
    let mut shallow_surfaces: Vec<ShallowSurface> = Vec::with_capacity(values.len());
    let mut all_objects = true;
    for v in values {
        match graph.node_data(*v) {
            Some(data) => match &*data {
                SemanticNodeData::Object(view) => {
                    shallow_surfaces.push(ShallowSurface::from_object(view));
                }
                _ => {
                    all_objects = false;
                    break;
                }
            },
            None => {
                all_objects = false;
                break;
            }
        }
    }
    if all_objects {
        let opt_surfaces: Vec<Option<ShallowSurface>> =
            shallow_surfaces.into_iter().map(Some).collect();
        if let Some(merged) = merge_intersection_surfaces_with_graph(graph, &opt_surfaces) {
            return graph.intern_node(SemanticNodeData::Object(surface_view_from_shallow(&merged)));
        }
    }
    // Fall back: intern an Intersection node so the structural meaning
    // is preserved without forcing the values into an Object shape.
    graph.intern_node(SemanticNodeData::Intersection(Arc::from(
        values.to_vec().into_boxed_slice(),
    )))
}

/// Merge per-arm union surfaces: a member survives iff present in EVERY
/// arm. The merged member's value is the union of the arms' values.
/// Returns the merged surface when at least one member survives;
/// returns `Some(empty)` when no common members exist; returns None
/// only when the arm surfaces vector is empty (defensive).
fn merge_union_surfaces(arm_surfaces: &[Option<ShallowSurface>]) -> Option<ShallowSurface> {
    if arm_surfaces.is_empty() {
        return None;
    }
    // Any None arm means the union has a non-Object arm — there are
    // no common Object members, so the merged surface is empty.
    if arm_surfaces.iter().any(|s| s.is_none()) {
        return Some(ShallowSurface::empty());
    }
    let live: Vec<&ShallowSurface> = arm_surfaces.iter().filter_map(|s| s.as_ref()).collect();
    if live.is_empty() {
        return Some(ShallowSurface::empty());
    }
    let mut common: indexmap::IndexMap<Arc<str>, ShallowSurfaceMember> = indexmap::IndexMap::new();
    for member in &live[0].members {
        let mut present_in_all = true;
        for other in live.iter().skip(1) {
            if !other.members.iter().any(|m| m.name == member.name) {
                present_in_all = false;
                break;
            }
        }
        if present_in_all {
            common.insert(Arc::clone(&member.name), member.clone());
        }
    }
    Some(ShallowSurface {
        members: common.into_values().collect(),
    })
}

/// Lift a `ShallowSurface` into a `SurfaceView` for interning into
/// `SemanticNodeData::Object`. `keyspace` and signatures are empty —
/// the synthesiser produces a one-level merged surface only.
fn surface_view_from_shallow(surface: &ShallowSurface) -> SurfaceView {
    let members: Vec<SurfaceMember> = surface
        .members
        .iter()
        .map(|m| SurfaceMember {
            name: Arc::clone(&m.name),
            value: m.value,
            optional: m.optional,
            readonly: m.readonly,
            is_method: m.is_method,
        })
        .collect();
    SurfaceView {
        members: Arc::from(members.into_boxed_slice()),
        call_signatures: Arc::from(Vec::<SemanticNodeId>::new().into_boxed_slice()),
        construct_signatures: Arc::from(Vec::<SemanticNodeId>::new().into_boxed_slice()),
        index_signatures: Arc::from(
            Vec::<crate::semantic_query::IndexSignature>::new().into_boxed_slice(),
        ),
        keyspace: None,
        has_index_signature: false,
    }
}

/// Empty `SurfaceView` used when the synthesiser has nothing to
/// contribute (e.g., open conditional with no branch chosen).
fn empty_surface_view() -> SurfaceView {
    SurfaceView {
        members: Arc::from(Vec::<SurfaceMember>::new().into_boxed_slice()),
        call_signatures: Arc::from(Vec::<SemanticNodeId>::new().into_boxed_slice()),
        construct_signatures: Arc::from(Vec::<SemanticNodeId>::new().into_boxed_slice()),
        index_signatures: Arc::from(
            Vec::<crate::semantic_query::IndexSignature>::new().into_boxed_slice(),
        ),
        keyspace: None,
        has_index_signature: false,
    }
}

/// Walk a key-space node tree and append every reachable string-literal
/// name into `out`. Returns true if every leaf was a string literal,
/// false if any leaf was non-literal (so the caller knows the keyspace
/// can't be enumerated). Recursive descent into Union arms; flat list
/// for Literal carriers.
fn collect_literal_keys(
    graph: &SemanticGraphStore,
    node: SemanticNodeId,
    out: &mut Vec<Arc<str>>,
) -> bool {
    let data = match graph.node_data(node) {
        Some(d) => d,
        None => return false,
    };
    match &*data {
        SemanticNodeData::Literal(crate::semantic_query::LiteralValue::String(s)) => {
            out.push(Arc::from(s.as_str()));
            true
        }
        SemanticNodeData::Union(arms) => {
            let arms = Arc::clone(arms);
            drop(data);
            for arm in arms.iter() {
                if !collect_literal_keys(graph, *arm, out) {
                    return false;
                }
            }
            true
        }
        _ => false,
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
