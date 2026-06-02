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
///
/// Carries the COMPLETE one-level surface fact set so the empty-path Shallow
/// projection (`expand_empty_path_shallow_terminal_surface`) reconstructs a
/// full `SurfaceView` rather than dropping signatures (the load-bearing fix
/// for the type-resolution unification): named members PLUS call signatures,
/// construct signatures, index signatures, and the keyspace. Members stay
/// shallow — each `value` is a `SemanticNodeId`, never an expanded body.
#[derive(Debug, Clone, Default)]
pub struct ShallowSurface {
    pub members: Vec<ShallowSurfaceMember>,
    /// Call signatures (`(args): ret`) carried verbatim from the contributing
    /// `SurfaceView`. Each id is a `Function`-shaped node.
    pub call_signatures: Vec<SemanticNodeId>,
    /// Construct signatures (`new (args): ret`) carried verbatim.
    pub construct_signatures: Vec<SemanticNodeId>,
    /// Index signatures (`[k: K]: V`) carried verbatim.
    pub index_signatures: Vec<crate::semantic_query::IndexSignature>,
    /// Keyspace node when the surface is a mapped/keyspace carrier; `None`
    /// for an ordinary object surface.
    pub keyspace: Option<SemanticNodeId>,
}

/// One member contribution while the walker is merging arms. Carries the
/// surface-level optionality / readonly bits so intersection/union merges
/// can implement the TS rules (intersection: required-wins +
/// readonly-OR; union: members in all arms).
///
/// `declared_in_macro_type_arg` propagates the macro own-body bit from
/// `SurfaceMember` through the dispatch walker's intermediate state so
/// `surface_view_from_shallow` can reconstruct an accurate `SurfaceView`.
/// `merge_role` propagates the surface-merge role (OwnBody / Heritage /
/// Authored) so the intersection merge can apply own-body-shadows-heritage
/// ONLY to real interface/class heritage (not authored intersections).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ShallowSurfaceMember {
    pub name: Arc<str>,
    pub value: SemanticNodeId,
    pub optional: bool,
    pub readonly: bool,
    pub is_method: bool,
    /// Declared accessibility, carried verbatim from the source
    /// [`SurfaceMember`] through the walker's intermediate state so the
    /// empty-path Shallow projection round-trip is lossless. A member produced
    /// by an intersection / union / inherited merge carries the MOST-RESTRICTIVE
    /// accessibility across its contributing arms (the shared merge rule): it is
    /// `Public` only when Public in EVERY contributor; a member non-public in
    /// any contributor stays non-public.
    pub visibility: verter_type_expr::MemberVisibility,
    pub declared_in_macro_type_arg: bool,
    pub merge_role: crate::semantic_query::MemberMergeRole,
    /// OXC declaration-site spans, carried verbatim from the source
    /// [`SurfaceMember`] through the walker's intermediate state so the
    /// empty-path Shallow projection round-trip is lossless for member
    /// provenance.
    pub spans: verter_type_expr::MemberSpans,
    /// Canonical declaration file of the source member, carried verbatim from
    /// [`SurfaceMember::declaration_origin`] so the Shallow round-trip pairs
    /// the member's spans with its real declaration file (not its value-node
    /// scope).
    pub declaration_origin: Option<Arc<str>>,
}

impl ShallowSurface {
    /// Empty surface contribution — used when an arm cannot yield any
    /// members (open conditional, non-Object terminal, instantiation
    /// recursion).
    #[inline]
    #[must_use]
    pub fn empty() -> Self {
        Self::default()
    }

    /// Build a `ShallowSurface` from a `SurfaceView`. The members are
    /// cloned shallowly — `value` is a `SemanticNodeId` (Copy), so the
    /// clone cost is one Arc bump for `name`. `declared_in_macro_type_arg`
    /// and `merge_role` propagate from each `SurfaceMember` so the walker's
    /// intermediate state preserves the provenance bit AND the merge role
    /// through intersection / union merges. Call / construct / index
    /// signatures and the keyspace are carried verbatim so the empty-path
    /// Shallow projection no longer drops them.
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
                    // Carry the source member's declared accessibility verbatim.
                    visibility: m.visibility,
                    declared_in_macro_type_arg: m.declared_in_macro_type_arg,
                    merge_role: m.merge_role,
                    // Carry the source member's OXC spans verbatim.
                    spans: m.spans,
                    // Carry the source member's declaration file verbatim.
                    declaration_origin: m.declaration_origin.clone(),
                })
                .collect(),
            call_signatures: view.call_signatures.to_vec(),
            construct_signatures: view.construct_signatures.to_vec(),
            index_signatures: view.index_signatures.to_vec(),
            keyspace: view.keyspace,
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
    /// `true` when this walk projects a NON-EMPTY path (the caller
    /// requested `base[seg..]`), as opposed to an EMPTY whole-surface
    /// projection (`ProjectPath(base, [])`). Set once in [`Self::walk`].
    ///
    /// The path-precision rule distinguishes the two terminal cases: a
    /// non-empty path's terminal value is a PROJECTED segment result, so
    /// under `Expanded` it resolves a carrier (`DeclRef` /
    /// `InstantiationRef`) UNDER the caller's mode. The empty whole-
    /// surface projection KEEPS the carrier-preserving behaviour of
    /// [`Self::expand_empty_path_terminal`] (the slot-binding indexed-
    /// access preservation policy at the empty-path terminal expander).
    original_path_non_empty: bool,
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
            original_path_non_empty: false,
        }
    }

    /// Derived accessor — equivalent to the pre-Commit-2 `self.mode`
    /// field. The full demand axis lives on [`Self::context`].
    #[inline]
    fn mode(&self) -> ProjectionMode {
        self.context.mode
    }

    /// Surface-provenance accessor (codex BINDING design). The macro
    /// type-argument own-body entry context flows from the
    /// `ProjectPath`'s context onto the walker; the `DeclPlaceholder`
    /// expansion below preserves it onto its `Instantiate` dispatch so
    /// the unwrapped declaration's own-body members are stamped
    /// `declared_in_macro_type_arg = true`. Member-value sub-walks
    /// downgrade to structural at the point they re-dispatch.
    #[inline]
    fn provenance(&self) -> crate::semantic_query::SurfaceProvenanceContext {
        self.context.provenance
    }

    /// Effective surface provenance for a carrier-unwrap dispatch in the
    /// shallow-surface worklist (transparent-carrier provenance downgrade).
    ///
    /// When a `Frame::Visit` carries a `provenance_override` (set when the
    /// walker crossed a TRANSPARENT carrier — an identity-utility `Alias`
    /// such as `NoInfer<T>`, or any alias-target indirection — whose own
    /// body has no declared members), that override wins so members reached
    /// THROUGH the carrier are NOT stamped `MacroTypeArgOwnBody`. Otherwise
    /// the walker's constructing context provenance applies.
    #[inline]
    fn effective_provenance(
        &self,
        provenance_override: Option<crate::semantic_query::SurfaceProvenanceContext>,
    ) -> crate::semantic_query::SurfaceProvenanceContext {
        provenance_override.unwrap_or_else(|| self.provenance())
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
        // Record whether this is a non-empty projected path vs an empty
        // whole-surface projection — the terminal expansion of a
        // non-empty path resolves a `DeclRef` / `InstantiationRef`
        // terminal under the caller's mode (path-precision), whereas the
        // empty whole-surface projection stays carrier-preserving.
        self.original_path_non_empty = !path.is_empty();
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
                                PathSegment::Member(name) => OriginMeta::ProjectedMember {
                                    name: Arc::clone(name),
                                    provenance: verter_audit::MemberEdgeProvenance::PathProjection,
                                },
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
                    // Path-precision rule (mirrors the `InstantiationRef`
                    // intermediate-hop demotion above and `evaluate.rs`):
                    // this arm only runs inside `while index < path.len()`,
                    // so a path segment is ALWAYS still pending — the
                    // deferred `T[K]` shell is an INTERMEDIATE hop whose
                    // resolved surface the next segment walks. Re-dispatch
                    // it in `Navigate` so the intermediate stays shallow
                    // (its sibling members are NOT eagerly expanded when
                    // the caller demanded `Expanded`). Only the consumed
                    // TERMINAL segment runs in the caller's mode — that is
                    // handled after the loop by
                    // `resolve_expanded_terminal_carrier`.
                    let resolved = match self.dispatch.execute(SemanticQueryKey::IndexedAccess {
                        base: *object,
                        index: ix.clone(),
                        mode: ProjectionMode::Navigate,
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
                        // Tier 2: **non-emitting** key-domain
                        // membership predicate.
                        //
                        // The enumeration-based admission would call
                        // `key_names_from_keyspace_node` here — which
                        // routes through `evaluate_deferred_semantic_node`
                        // and `key_names_from_base_node`, both
                        // emitting one `ProjectMember` edge per
                        // enumerated key (via the `build_key_of` /
                        // `build_mapped_type` publication loop). That
                        // would emit the entire keyspace just to test
                        // membership of ONE segment.
                        //
                        // The non-emitting predicate decides admission
                        // structurally without `evaluate_deferred_*`
                        // and without `Instantiate` round-trips. When
                        // it cannot decide (`None`), the walker falls
                        // through to Tier 3 (primitive keyspace) or
                        // accepts the unresolved carrier — NEVER
                        // enumerating to prove membership.
                        if let Some(admits) = self
                            .dispatch
                            .keyspace_admits_literal_non_emitting(mapper.key_space, needle)
                        {
                            return admits;
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
                        // Route through the shared
                        // `materialize_selected_key_mapped_value_with_node`
                        // substrate. The node-keyed entry preserves
                        // String / Number literal kind through
                        // substitution (G4 soundness — `M[1]` keeps
                        // `K = Literal::Number(1)`). The helper does
                        // substitute → evaluate → Instantiate →
                        // **trailing Conditional reduction**: the
                        // last step drives the body's `Conditional`
                        // dispatch through the nested-infer arm so
                        // per-key narrowing closes generic-helper
                        // conditionals (`ExtendSlotWithPlan<TPlan, K>`-style)
                        // to the realized `Function` instead of
                        // leaving a Conditional carrier shell. The
                        // Opaque-fallback contract (substituted
                        // carrier on stall) is preserved inside the
                        // helper so the free mapper binder is never
                        // leaked.
                        let value = self
                            .dispatch
                            .materialize_selected_key_mapped_value_with_node(
                                mapper,
                                key_arg,
                                crate::semantic_query::ProjectionReductionContext::published(
                                    self.mode(),
                                ),
                            );
                        // Emit the per-key edge mirroring
                        // `build_mapped_type`'s ProjectMember edge so
                        // downstream origin-graph consumers see the
                        // same per-key contribution.
                        let edge_kind = match next_segment.expect("checked above") {
                            PathSegment::Member(_) => OriginEdgeKind::ProjectMember,
                            PathSegment::Index(_) => OriginEdgeKind::ProjectIndex,
                        };
                        let meta = match next_segment.expect("checked above") {
                            PathSegment::Member(member_name) => OriginMeta::ProjectedMember {
                                name: Arc::clone(member_name),
                                provenance: verter_audit::MemberEdgeProvenance::PathProjection,
                            },
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
                // Intermediate-hop demand demotion.
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
                    whole_hash: _,
                }) => {
                    let base = crate::semantic_query::DeclKey {
                        canonical_id: Arc::clone(canonical_id),
                        decl_name: Arc::clone(name),
                    };
                    drop(data);
                    let expanded = match self.dispatch.execute(SemanticQueryKey::Instantiate {
                        base,
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
                    // Codex Q4 — IA path-precision.
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
                    let identity = base.to_decl_key();
                    let args_clone = Arc::clone(args);
                    drop(data);
                    // Intermediate-hop demand
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
            // Path-precision: when this is a NON-EMPTY projected path,
            // the terminal value is a projected segment result. A
            // terminal `DeclRef` / `InstantiationRef` carrier
            // (e.g. `$props` projected to `DeclRef(Props)`) resolves
            // UNDER the caller's `Expanded` mode so the demanded
            // terminal projection expands. The EMPTY whole-surface
            // projection keeps the carrier-preserving behaviour of
            // `expand_empty_path_terminal` below (the slot-binding
            // indexed-access preservation policy) and is NOT pre-
            // resolved here.
            if self.original_path_non_empty {
                current = self.resolve_expanded_terminal_carrier(current);
            }
            current = self.expand_empty_path_terminal(current);
        } else if matches!(self.mode(), ProjectionMode::Shallow) {
            current = self.expand_empty_path_shallow_terminal_surface(current);
        }
        results.push(current);
    }

    /// Resolve a NON-EMPTY-path terminal carrier under `Expanded`.
    ///
    /// Path-precision rule: the terminal segment of a non-empty path
    /// runs in the caller's mode. When the projected terminal is a
    /// `DeclRef` / `InstantiationRef` carrier and the caller demanded
    /// `Expanded`, resolve the carrier ONE level (declaration resolution
    /// / instantiation under `published(Expanded)`) so the subsequent
    /// `expand_empty_path_terminal` can materialise the Object surface.
    ///
    /// This mirrors the in-loop `DeclRef` / `InstantiationRef` arms
    /// (`advance_step`), which only fire while a segment is still pending
    /// (an INTERMEDIATE hop). The terminal carrier is reached after the
    /// last segment is consumed, so the loop has already exited; this
    /// applies the SAME resolution to the terminal under the caller's
    /// mode. Non-carrier terminals (Object, Union, Intersection, …) are
    /// returned unchanged and flow into `expand_empty_path_terminal`.
    fn resolve_expanded_terminal_carrier(&mut self, node: SemanticNodeId) -> SemanticNodeId {
        let data = match self.graph().node_data(node) {
            Some(data) => data,
            None => return node,
        };
        match &*data {
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
                    QueryResult::Value(id) => id,
                    // A recursive / errored terminal carrier keeps the
                    // carrier (no expansion) — the published surface stays
                    // the bare `DeclRef` per the shallow-by-default rule
                    // when resolution cannot complete.
                    QueryResult::Recursive(_) | QueryResult::Error(_) => node,
                }
            }
            SemanticNodeData::InstantiationRef { base, args } => {
                let identity = base.clone();
                let args_clone = Arc::clone(args);
                drop(data);
                match self.dispatch.execute(SemanticQueryKey::Instantiate {
                    base: identity.to_decl_key(),
                    args: args_clone,
                    context: crate::semantic_query::ProjectionReductionContext::published(
                        self.mode(),
                    ),
                }) {
                    QueryResult::Value(id) => id,
                    QueryResult::Recursive(_) | QueryResult::Error(_) => node,
                }
            }
            _ => {
                drop(data);
                node
            }
        }
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

    /// Iterative empty-path-terminal expander. Union / Intersection
    /// arm descent grows the heap-backed worklist rather than the
    /// Rust call stack, and `DeclAnchor` expansion tail-iterates
    /// through the worklist rather than via a recursive call.
    ///
    /// **Mapped arm.** When the walker runs in
    /// [`ProjectionMode::Expanded`] and a `SemanticNodeData::Mapped`
    /// shell appears at an empty-path terminal, re-enter dispatch via
    /// [`SemanticQueryKey::MappedType`] so the deferred shell is
    /// materialised into its concrete surface rather than being
    /// returned unchanged.
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
                whole_hash: _,
            }) => {
                let identity = crate::semantic_query::DeclKey {
                    canonical_id: Arc::clone(canonical_id),
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
                // Preserve the walker's surface provenance onto the
                // `Instantiate` expansion (codex BINDING design): when
                // the empty-path `ProjectPath` macro-payload surface read
                // entered under `MacroTypeArgOwnBody`, the unwrapped
                // declaration's own-body members must be stamped
                // `declared_in_macro_type_arg = true`. A bare
                // `published(self.mode())` here would drop the provenance
                // and the macro-T-root own-body members would all report
                // `false`.
                let expanded = match self.dispatch.execute(SemanticQueryKey::Instantiate {
                    base: identity,
                    args: Arc::from(Vec::<SemanticNodeId>::new().into_boxed_slice()),
                    context: crate::semantic_query::ProjectionReductionContext::published(
                        self.mode(),
                    )
                    .with_provenance(self.provenance()),
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
            // `DeclRef`/`InstantiationRef`
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
        // to the root slot. No role override at the root — roles are baked at
        // lowering / assigned at heritage-arm descent.
        let mut work: Vec<Frame> = Vec::with_capacity(8);
        work.push(Frame::Visit {
            node,
            target: BufferTarget::Root,
            member_role_override: None,
            heritage_overlay_body: false,
            // Root seed: no transparent-carrier downgrade yet. The walk's
            // constructing context provenance (`self.provenance()`) applies
            // until a transparent carrier is crossed.
            provenance_override: None,
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
                Frame::Visit {
                    node: cur,
                    target,
                    member_role_override,
                    heritage_overlay_body,
                    provenance_override,
                } => {
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
                        member_role_override,
                        heritage_overlay_body,
                        provenance_override,
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
                    member_role_override,
                    heritage_overlay,
                    provenance_override,
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
                            arms: Arc::clone(&arms),
                            arm_index: arm_index + 1,
                            buffer_id,
                            kind,
                            member_role_override,
                            heritage_overlay,
                            provenance_override,
                        });
                    }
                    // Per-arm role override (codex BINDING design): a
                    // heritage-overlay body's REFERENCE-carrier arms are
                    // `extends`/`implements` heritage — visit them with
                    // `Some(Heritage)` so their inherited members (which the
                    // base lowered as its own body) become `Heritage` relative
                    // to the consuming declaration. The own `Object` arm keeps
                    // its lowered `OwnBody` role (no override). Ordinary
                    // intersections / unions just inherit the parent override.
                    let arm_role_override =
                        if heritage_overlay && !arm_is_object_surface(self.graph(), arm) {
                            Some(crate::semantic_query::MemberMergeRole::Heritage)
                        } else {
                            member_role_override
                        };
                    work.push(Frame::Visit {
                        node: arm,
                        target,
                        member_role_override: arm_role_override,
                        heritage_overlay_body: false,
                        // Arm descent inherits the parent's transparent-carrier
                        // downgrade: a union/intersection nested under a
                        // crossed transparent carrier keeps the structural
                        // provenance for its members.
                        provenance_override,
                    });
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
                    // Vue macro object-surface publication enumerates
                    // the UNION of arm members; ordinary `ProjectPath` /
                    // `keyof` uses the TS common-member intersection. The
                    // demand axis on the walker's context selects the rule;
                    // both are cache-keyed in distinct slots
                    // (`MacroSurfaceShallow` vs the `Shallow` publication
                    // slot) so they never collide.
                    let merged = if self.context.is_macro_object_surface() {
                        merge_union_surfaces_for_macro(self.graph(), &arm_surfaces)
                    } else {
                        merge_union_surfaces(self.graph(), &arm_surfaces)
                    };
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
        member_role_override: Option<crate::semantic_query::MemberMergeRole>,
        heritage_overlay_body: bool,
        provenance_override: Option<crate::semantic_query::SurfaceProvenanceContext>,
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
                let mut surface = ShallowSurface::from_object(view);
                drop(data);
                // Apply the heritage role override (codex BINDING design):
                // when this Object was reached through a consuming
                // declaration's `extends`/`implements` heritage carrier, its
                // members (which the base lowered as its OWN body) become
                // `Heritage` relative to the consuming declaration, so the
                // own-body-shadows-heritage merge fires.
                if let Some(role) = member_role_override {
                    for member in &mut surface.members {
                        member.merge_role = role;
                    }
                }
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
                        // Propagate the parent override to ordinary arms; the
                        // heritage-overlay flag (set by the decl-root unwrap
                        // for an interface/class body) makes the per-arm
                        // descent stamp reference arms `Heritage`.
                        member_role_override,
                        heritage_overlay: heritage_overlay_body,
                        provenance_override,
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
                        member_role_override,
                        heritage_overlay: false,
                        provenance_override,
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
                //
                // Preserve the walker's surface provenance (codex BINDING design)
                // (continued): `defineProps<Foo<Bar>>()` makes `Foo`'s
                // OWN-body members macro-T own-body. `Bar` is a generic
                // argument substituted INTO `Foo`'s body and is lowered
                // structurally at the `Ref` arm, so only `Foo`'s own
                // members carry the bit.
                match self.dispatch.execute(SemanticQueryKey::Instantiate {
                    base: identity.to_decl_key(),
                    args: args_clone,
                    context: crate::semantic_query::ProjectionReductionContext::structural_transit_with_mode(
                        ProjectionMode::Navigate,
                    )
                    .with_provenance(self.effective_provenance(provenance_override)),
                }) {
                    QueryResult::Value(body) => {
                        // Continue the walk into the materialised body. If the
                        // instantiated declaration is an interface/class, its
                        // body is an `extends`/`implements` heritage overlay —
                        // mark it so the per-arm descent stamps reference arms
                        // `Heritage`. Propagate any inbound override (this
                        // decl-root may itself be a heritage carrier).
                        let heritage_overlay_body = matches!(
                            self.dispatch.prepared_decl_kind(&identity),
                            Some(
                                verter_semantic::analysis::type_eval::TypeDeclKind::Interface
                                    | verter_semantic::analysis::type_eval::TypeDeclKind::Class
                            )
                        );
                        work.push(Frame::Visit {
                            node: body,
                            target,
                            member_role_override,
                            heritage_overlay_body,
                            provenance_override,
                        });
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
                // Preserve the walker's surface provenance onto the
                // DeclPlaceholder unwrap (codex BINDING design): the
                // mode/demand stay transit-Navigate (carrier-stop
                // semantics for the shallow surface walk), but a
                // `MacroTypeArgOwnBody` walker stamps the unwrapped
                // declaration's OWN-body members `declared_in_macro_type_arg
                // = true`. A bare `structural_transit_with_mode(Navigate)`
                // here drops the provenance and the macro-T-root own-body
                // members all report `false`.
                //
                // Transparent-carrier provenance downgrade: when this
                // DeclPlaceholder was reached THROUGH a transparent carrier
                // (an identity-utility `Alias` such as `NoInfer<Base>`),
                // `provenance_override` is
                // `Some(Structural)` and `effective_provenance` downgrades the
                // unwrap so `Base`'s own-body members are NOT mis-stamped as
                // the macro type argument's own body.
                match self.dispatch.execute(SemanticQueryKey::Instantiate {
                    base: identity.to_decl_key(),
                    args: Arc::from(Vec::<SemanticNodeId>::new().into_boxed_slice()),
                    context: crate::semantic_query::ProjectionReductionContext::structural_transit_with_mode(
                        ProjectionMode::Navigate,
                    )
                    .with_provenance(self.effective_provenance(provenance_override)),
                }) {
                    QueryResult::Value(body) => {
                        // Interface/class declaration body → heritage overlay
                        // (its reference arms are `extends`/`implements`). The
                        // own `Object` arm keeps its lowered `OwnBody` role.
                        let heritage_overlay_body = matches!(
                            self.dispatch.prepared_decl_kind(&identity),
                            Some(
                                verter_semantic::analysis::type_eval::TypeDeclKind::Interface
                                    | verter_semantic::analysis::type_eval::TypeDeclKind::Class
                            )
                        );
                        work.push(Frame::Visit {
                            node: body,
                            target,
                            member_role_override,
                            heritage_overlay_body,
                            provenance_override,
                        });
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
                if is_open && self.context.is_macro_object_surface() {
                    // Open conditional UNDER a Vue macro object surface
                    // (`defineProps<Props<T>>()` where `Props<T>` is an
                    // open conditional). The macro object-surface contract
                    // enumerates the UNION of BOTH branches' members — a
                    // consumer reading the macro surface must see every
                    // member that ANY branch could contribute, each OPTIONAL
                    // (it is present only when the unresolved check selects
                    // that branch). This mirrors the closed-conditional
                    // distribution below, but is taken for the OPEN case
                    // only when the demand axis is a macro object surface;
                    // the ordinary `Published(Shallow)` contract keeps the
                    // empty + `OpenConditional` behaviour (the `else if`
                    // arm). `merge_union_surfaces_for_macro` marks each
                    // member absent from at least one branch as optional, so
                    // the two disjoint branch member sets both surface as
                    // optional automatically.
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
                        member_role_override,
                        heritage_overlay: false,
                        provenance_override,
                    });
                } else if is_open {
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
                        member_role_override,
                        heritage_overlay: false,
                        provenance_override,
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
                // Follow the alias, preserving the role override + heritage
                // flag so an identity-alias wrapper of a heritage carrier /
                // interface body keeps its role classification.
                //
                // TRANSPARENT-carrier provenance downgrade.
                // An `Alias` node is a transparent carrier: it is produced by an
                // identity utility (`NoInfer<T>` interns `Alias(T)`) or by an
                // alias-target indirection. Its own body has NO declared
                // members — the members live on the alias TARGET. A member
                // reached only THROUGH this carrier is therefore NOT the macro
                // type argument's own-body member, so the macro-T own-body
                // provenance must NOT propagate past the alias. Downgrade to
                // `Structural` for the target walk (and keep any already-active
                // downgrade). A DIRECT object-alias macro argument
                // (`type P = { x }`) does NOT reach here as the own-body
                // source: its members are stamped at the
                // `DeclPlaceholder → Instantiate → Object` overlay
                // (`overlay_macro_type_arg_own_body`) before any `Alias` node,
                // so this downgrade does not regress it.
                work.push(Frame::Visit {
                    node: target_id,
                    target,
                    member_role_override,
                    heritage_overlay_body,
                    provenance_override: Some(
                        crate::semantic_query::SurfaceProvenanceContext::Structural,
                    ),
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
                            // Resolve the carrier, preserving the role
                            // override so a heritage `DeclRef` arm keeps its
                            // `Heritage` override flowing into the resolved
                            // declaration's body (cross-file / same-file
                            // heritage members surface as `Heritage`).
                            // Thread any active transparent-carrier
                            // downgrade so a `DeclRef` reached through a
                            // transparent alias keeps the structural provenance.
                            work.push(Frame::Visit {
                                node: resolved,
                                target,
                                member_role_override,
                                heritage_overlay_body,
                                provenance_override,
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
        // Optional source-member surface — populated only when the new
        // `mapped_surface_source_members_for_projection` helper resolves
        // the source carrier to an `Object`. Used by the per-key build
        // loop to:
        //   - feed the Identity-mapper fast path (`mapper.kind ==
        //     MapperKind::Identity` → use `source_member.value`
        //     directly, exactly as `build_mapped_type` does at the
        //     Expanded publication path); and
        //   - inherit `OptionalityMod::Keep` / `ReadonlyMod::Keep`
        //     modifiers from the source member.
        // `None` for sources that did not resolve to an Object (e.g.,
        // the source is itself a Mapped / KeyOf carrier-stop, primitive,
        // etc.).
        let mut source_members: Option<Vec<crate::semantic_query::SurfaceMember>> = None;
        if !collected {
            // The shared enumerator on the SOURCE handles
            // `Opaque(DeclPlaceholder)` / `Object` / `Intersection` /
            // `Union` uniformly. Codex-binding —
            // empty-path Shallow with an imported mapped carrier MUST
            // enumerate just like the Expanded path's MappedType
            // dispatch.
            keys.clear();
            match self.dispatch.key_names_from_base_node(source) {
                Some(enumerated) => keys = enumerated,
                None => {
                    // Transit-Shallow Publication (codex Q1
                    // #2 / Q3): the source is a `DeclRef` /
                    // `InstantiationRef` carrier under
                    // `StructuralTransit(Navigate)` lowering and the
                    // global `key_names_from_base_node` deliberately
                    // does NOT unwrap those (the Identity fast path
                    // depends on that constraint). Fall back to the
                    // synthesise-local
                    // `mapped_surface_source_members_for_projection`
                    // helper which dispatches the source through
                    // `ProjectPath { source, [], Published(Shallow) }`
                    // and reads its terminal `Object` surface. The
                    // helper returns the full `SurfaceMember` list so
                    // the per-key build loop can pick Identity fast
                    // path values + source modifiers when available.
                    match self
                        .dispatch
                        .mapped_surface_source_members_for_projection(source, self.context)
                    {
                        Some(members) => {
                            keys = members.iter().map(|m| Arc::clone(&m.name)).collect();
                            source_members = Some(members);
                        }
                        None => return None,
                    }
                }
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
        // Identity-mapper fast path detection.
        // Transit-Shallow Publication (codex Q1 #3): when
        // `mapper.kind == Identity` (the canonical
        // `[K in keyof T]: T[K]` shape under `Readonly` / `Partial` /
        // `Required`) AND the source enumeration yielded a usable
        // `SurfaceMember` list, the per-key value is the source
        // member's `value` directly — exactly as `build_mapped_type`
        // does at the Expanded publication path. The per-key
        // `materialize_mapped_member_value_for_key` substrate is
        // reserved for the Computed mapper case (e.g.
        // `ExtendSlotWithPlan<TPlan, K>`-style Conditional bodies).
        let value_is_identity = matches!(mapper.kind, crate::semantic_query::MapperKind::Identity);
        // Key-space-independent value hoist — Shallow walker mirror of
        // the `build_mapped_type` hoist. When `mapper.value_expr` does
        // not reference the binder, the per-K substitution is the
        // identity, so the materialised value collapses to a single
        // shared evaluation. Run it ONCE here and reuse for every
        // enumerated key instead of dispatching the
        // selected-key materialiser N times for identical inputs.
        //
        // Identity-mapper fast path (`source_member.value` direct
        // read) is reserved for `MapperKind::Identity`; the hoist
        // only fires on `Computed` mappers, where the per-K
        // substituted body would otherwise diverge by the K argument.
        // Cross-variant `infer`-name matches are treated as references
        // (see `subtree_references_node`'s contract), preventing
        // over-aggressive hoisting on `infer`-bearing value
        // expressions.
        let value_expr_is_k_independent = !value_is_identity
            && !self
                .dispatch
                .subtree_references_node(mapper.value_expr, mapper.parameter_node);
        let shared_value: Option<SemanticNodeId> = if value_expr_is_k_independent {
            Some(
                self.dispatch
                    .materialize_selected_key_mapped_value_k_independent(
                        mapper,
                        materialise_context,
                    ),
            )
        } else {
            None
        };
        let members = keys
            .into_iter()
            .map(|name| {
                let source_member = source_members
                    .as_ref()
                    .and_then(|m| m.iter().find(|sm| sm.name == name));
                // Optionality / readonly resolution per TS semantics:
                //   - Add → always on
                //   - Remove → always off
                //   - Keep → inherit from source member (if known,
                //     else false). Knowing the source member is what
                //     the new source-surface helper unlocks for
                //     DeclRef-source mapped types.
                let optional = match optionality {
                    crate::semantic_query::OptionalityMod::Add => true,
                    crate::semantic_query::OptionalityMod::Remove => false,
                    crate::semantic_query::OptionalityMod::Keep => {
                        source_member.map(|m| m.optional).unwrap_or(false)
                    }
                };
                let readonly = match readonly_mod {
                    crate::semantic_query::ReadonlyMod::Add => true,
                    crate::semantic_query::ReadonlyMod::Remove => false,
                    crate::semantic_query::ReadonlyMod::Keep => {
                        source_member.map(|m| m.readonly).unwrap_or(false)
                    }
                };
                // Identity fast path — use source_member.value
                // verbatim. Computed path — substitute the binder
                // and materialise through the **Selected-Key Transit
                // Realization** substrate. The selected-key helper
                // extends the per-key materialiser with an explicit
                // trailing Conditional reduction: when the mapper
                // body is a Conditional generic helper (e.g.
                // `ExtendSlotWithPlan<TPlan, K>` with a
                // `PricingPlanSlots[K] extends (props: infer P) =>
                // unknown ? ... : ...` body), the per-key value
                // closes to the realized `Function` rather than the
                // Conditional carrier shell. Without this, the
                // graph-native slot-binding consumer's `Function`-arm
                // match would fail at the publication boundary.
                let value = if let (Some(sm), true) = (source_member, value_is_identity) {
                    sm.value
                } else if let Some(shared) = shared_value {
                    shared
                } else {
                    self.dispatch.materialize_selected_key_mapped_value(
                        mapper,
                        name.as_ref(),
                        materialise_context,
                    )
                };
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
                    // Mapped-produced member. The key domain is already
                    // public-only (non-public class members are filtered out of
                    // the keyspace at `source_members_for_published_projection` /
                    // `key_names_step`), so every produced member is public. For
                    // the homomorphic case thread the matched source member's
                    // (public) visibility verbatim so the invariant holds even if
                    // the keyspace gate is bypassed; otherwise `Public` (mirrors
                    // the Expanded path in `build.rs`).
                    visibility: source_member
                        .map_or(verter_type_expr::MemberVisibility::Public, |sm| {
                            sm.visibility
                        }),
                    // Mapped-type synthesis produces a member from a key
                    // domain via `[K in keyof T]: ...`. The produced
                    // member is not literally written in the consuming
                    // macro's T body — it is reached structurally via
                    // the mapper. `false` is the structural truth.
                    declared_in_macro_type_arg: false,
                    // A mapped-produced member is synthesized via the mapper,
                    // never an interface/class heritage overlay — `Authored`
                    // (it never shadows / is shadowed).
                    merge_role: crate::semantic_query::MemberMergeRole::Authored,
                    // Mapped-produced member: synthesized from a key domain,
                    // no single source declaration site — no spans, no file.
                    spans: verter_type_expr::MemberSpans::default(),
                    declaration_origin: None,
                }
            })
            .collect();
        Some(ShallowSurface {
            members,
            call_signatures: Vec::new(),
            construct_signatures: Vec::new(),
            index_signatures: Vec::new(),
            keyspace: None,
        })
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
        /// Surface-merge role override (codex BINDING design for the
        /// type-resolution unification): when `Some(role)`, every member of
        /// the Object surface this node yields is stamped `role`, overriding
        /// the role baked at lowering. Set to `Some(Heritage)` for a
        /// declaration's `extends`/`implements` heritage carrier arm so the
        /// inherited members (which `Base` lowered as its OWN body) become
        /// `Heritage` RELATIVE to the consuming declaration — enabling the
        /// own-body-shadows-heritage merge. Propagates verbatim through
        /// carrier hops (Alias / DeclRef / Instantiate) so a cross-file
        /// heritage carrier chain keeps the override down to the Object.
        member_role_override: Option<crate::semantic_query::MemberMergeRole>,
        /// True when this node is the body of an interface/class declaration
        /// (an `extends`/`implements` heritage overlay). When the node is an
        /// `Intersection`, its REFERENCE-carrier arms are heritage and are
        /// visited with `member_role_override = Some(Heritage)`; the own
        /// `Object` arm keeps its lowered `OwnBody` role. Only meaningful for
        /// the immediate decl-body Intersection; sub-visits default to false.
        heritage_overlay_body: bool,
        /// Surface-provenance override (transparent-carrier downgrade): when
        /// `Some`, the
        /// carrier-unwrap dispatches (Alias-target Instantiate via
        /// DeclPlaceholder / InstantiationRef) below this node use this
        /// provenance INSTEAD of the walker's `self.provenance()`. Set to
        /// `Some(Structural)` when the walker crosses a TRANSPARENT carrier
        /// (an `Alias` produced by an identity utility such as `NoInfer<T>`,
        /// or any alias-target indirection) whose own body has no declared
        /// members: a member reached only THROUGH such a carrier is NOT the
        /// macro type argument's own-body member, so it must not inherit
        /// `MacroTypeArgOwnBody`. Propagates verbatim through subsequent
        /// carrier hops so the downgrade sticks all the way to the Object.
        /// `None` ⇒ use `self.provenance()` (the macro-root / structural
        /// context the walk was constructed under).
        provenance_override: Option<crate::semantic_query::SurfaceProvenanceContext>,
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
        /// Surface-merge role override propagated to each arm's `Visit`
        /// (carrier inheritance — a heritage carrier's arms keep the
        /// override). `None` for ordinary intersections / unions.
        member_role_override: Option<crate::semantic_query::MemberMergeRole>,
        /// True when the parent node is an interface/class heritage-overlay
        /// body: REFERENCE-carrier arms get `Some(Heritage)`, own `Object`
        /// arms keep their lowered role.
        heritage_overlay: bool,
        /// Surface-provenance override (transparent-carrier downgrade)
        /// propagated to each
        /// arm's `Visit`. `None` for ordinary intersections / unions; carries
        /// the transparent-carrier downgrade through arm descents.
        provenance_override: Option<crate::semantic_query::SurfaceProvenanceContext>,
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

/// Whether an intersection arm node is an inline own-body `Object` surface
/// (vs a reference carrier — `DeclRef` / `InstantiationRef` /
/// `DeclPlaceholder` / alias chain to a reference).
///
/// Used by the empty-path Shallow walker to classify a heritage-overlay
/// body's arms: an `Object` arm is the consuming declaration's OWN body
/// (members keep their lowered `OwnBody` role); any other arm is the
/// `extends`/`implements` heritage carrier (its members are overridden to
/// `Heritage`). An `Alias` is followed one hop so an identity-alias wrapper of
/// an own-body object is still classified as own-body.
fn arm_is_object_surface(graph: &SemanticGraphStore, arm: SemanticNodeId) -> bool {
    match graph.node_data(arm) {
        Some(data) => match &*data {
            SemanticNodeData::Object(_) => true,
            SemanticNodeData::Alias(target) => arm_is_object_surface(graph, *target),
            _ => false,
        },
        None => false,
    }
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
    // Aggregate members by name. For each name track the own-body values and
    // the heritage/authored values separately so the P2-1 own-body-shadows-
    // heritage rule can apply ONLY to real interface/class heritage.
    let mut by_name: indexmap::IndexMap<Arc<str>, MergedMemberAccum> = indexmap::IndexMap::new();
    for surface in &live {
        for member in &surface.members {
            let accum = by_name
                .entry(Arc::clone(&member.name))
                .or_insert_with(|| MergedMemberAccum::new(&member.name));
            accum.absorb(member);
        }
    }
    let members: Vec<ShallowSurfaceMember> = by_name
        .into_values()
        .map(|accum| accum.finish(graph))
        .collect();

    // Carry the non-member surface facts through the intersection. Call /
    // construct signatures concatenate across arms (TS `A & B` of two
    // call-signature carriers exposes BOTH overload sets); index signatures
    // concatenate; the keyspace is the first arm's keyspace (an ordinary
    // object intersection carries none). De-dup to keep interned identity
    // stable.
    let mut call_signatures: Vec<SemanticNodeId> = Vec::new();
    let mut construct_signatures: Vec<SemanticNodeId> = Vec::new();
    let mut index_signatures: Vec<crate::semantic_query::IndexSignature> = Vec::new();
    let mut keyspace: Option<SemanticNodeId> = None;
    for surface in &live {
        for sig in &surface.call_signatures {
            if !call_signatures.contains(sig) {
                call_signatures.push(*sig);
            }
        }
        for sig in &surface.construct_signatures {
            if !construct_signatures.contains(sig) {
                construct_signatures.push(*sig);
            }
        }
        for sig in &surface.index_signatures {
            if !index_signatures.contains(sig) {
                index_signatures.push(sig.clone());
            }
        }
        if keyspace.is_none() {
            keyspace = surface.keyspace;
        }
    }

    Some(ShallowSurface {
        members,
        call_signatures,
        construct_signatures,
        index_signatures,
        keyspace,
    })
}

/// Working aggregation for one merged member name during intersection surface
/// synthesis. Separates own-body contributor values from heritage/authored
/// contributor values so the P2-1 own-body-shadows-heritage rule applies ONLY
/// to real interface/class heritage overlays (not authored intersections).
struct MergedMemberAccum {
    name: Arc<str>,
    /// Distinct value nodes from `OwnBody` contributors.
    own_body_values: Vec<SemanticNodeId>,
    /// Distinct value nodes from `Heritage` / `Authored` contributors.
    other_values: Vec<SemanticNodeId>,
    /// True iff at least one contributor was `Heritage` (a real
    /// interface/class `extends`/`implements` overlay).
    saw_heritage: bool,
    /// True iff at least one contributor was `OwnBody`.
    saw_own_body: bool,
    optional: bool,
    readonly: bool,
    is_method: bool,
    declared_in_macro_type_arg: bool,
    /// Spans of the first `OwnBody` contributor — the winning member when the
    /// own-body-shadows-heritage rule fires (or when the result is `OwnBody`).
    own_body_spans: Option<verter_type_expr::MemberSpans>,
    /// Spans of the first contributor of ANY role — used when the result is
    /// not own-body (intersection / heritage / authored), so the surviving
    /// member references its own first declaration site.
    first_spans: Option<verter_type_expr::MemberSpans>,
    /// Declaration file of the first `OwnBody` contributor — paired with
    /// `own_body_spans` so the shadow winner's spans index its own file.
    own_body_origin: Option<Arc<str>>,
    /// Declaration file of the first contributor of ANY role — paired with
    /// `first_spans`.
    first_origin: Option<Arc<str>>,
    /// MOST-RESTRICTIVE visibility across ALL `OwnBody` contributors (the shared
    /// merge rule: `Private` > `Protected` > `Public`), folded over the RAW
    /// contributor stream (NOT deduped value nodes) — two contributors sharing
    /// one value type still both fold in. `None` until the first own-body
    /// contributor is absorbed.
    own_body_visibility_agg: Option<verter_type_expr::MemberVisibility>,
    /// MOST-RESTRICTIVE visibility across ALL `Heritage` / `Authored`
    /// contributors. `None` until the first such contributor is absorbed.
    other_visibility_agg: Option<verter_type_expr::MemberVisibility>,
}

impl MergedMemberAccum {
    fn new(name: &Arc<str>) -> Self {
        Self {
            name: Arc::clone(name),
            own_body_values: Vec::new(),
            other_values: Vec::new(),
            saw_heritage: false,
            saw_own_body: false,
            // `optional` is required-wins (AND across arms); seed `true` so the
            // first absorb sets the truth.
            optional: true,
            readonly: false,
            is_method: false,
            declared_in_macro_type_arg: false,
            own_body_spans: None,
            first_spans: None,
            own_body_origin: None,
            first_origin: None,
            own_body_visibility_agg: None,
            other_visibility_agg: None,
        }
    }

    fn absorb(&mut self, member: &ShallowSurfaceMember) {
        use crate::semantic_query::MemberMergeRole;
        self.optional = self.optional && member.optional;
        self.readonly = self.readonly || member.readonly;
        self.is_method = self.is_method || member.is_method;
        self.declared_in_macro_type_arg =
            self.declared_in_macro_type_arg || member.declared_in_macro_type_arg;
        // Retain a representative span + declaration file per the
        // value-selection rule: the first own-body contributor (the shadow
        // winner) and the first contributor of any role (the
        // intersection/heritage representative). Span and origin are captured
        // together so the surviving member's spans index its own file.
        if self.first_spans.is_none() {
            self.first_spans = Some(member.spans);
            self.first_origin = member.declaration_origin.clone();
        }
        if matches!(member.merge_role, MemberMergeRole::OwnBody) && self.own_body_spans.is_none() {
            self.own_body_spans = Some(member.spans);
            self.own_body_origin = member.declaration_origin.clone();
        }
        // Aggregate visibility to the MOST-RESTRICTIVE contributor per role (the
        // shared merge rule), folded over EVERY contributor (NOT deduped value
        // nodes) — so two contributors sharing one value type still both fold
        // in, and single-vs-multi never depends on value-node identity. The
        // own-body and heritage/authored aggregates are tracked separately so
        // the own-body-shadows-heritage rule can select the matching aggregate
        // in `finish` (in LOCKSTEP with the value selection).
        let fold = |slot: &mut Option<verter_type_expr::MemberVisibility>| {
            *slot = Some(match *slot {
                Some(existing) => existing.most_restrictive(member.visibility),
                None => member.visibility,
            });
        };
        match member.merge_role {
            MemberMergeRole::OwnBody => {
                self.saw_own_body = true;
                fold(&mut self.own_body_visibility_agg);
                if !self.own_body_values.contains(&member.value) {
                    self.own_body_values.push(member.value);
                }
            }
            MemberMergeRole::Heritage => {
                self.saw_heritage = true;
                fold(&mut self.other_visibility_agg);
                if !self.other_values.contains(&member.value) {
                    self.other_values.push(member.value);
                }
            }
            MemberMergeRole::Authored => {
                fold(&mut self.other_visibility_agg);
                if !self.other_values.contains(&member.value) {
                    self.other_values.push(member.value);
                }
            }
        }
    }

    fn finish(self, graph: &SemanticGraphStore) -> ShallowSurfaceMember {
        use crate::semantic_query::MemberMergeRole;
        // P2-1 own-body-shadows-heritage: when the name has an own-body
        // contributor AND a heritage contributor, the derived own-body member
        // SHADOWS the inherited one (`interface Props extends Base { dup }` =>
        // `Props['dup']` is the own `dup`, not `own & heritage`). The heritage
        // values are dropped entirely. Authored intersections never set
        // `saw_heritage`, so `type Props = Base & { dup }` falls through to the
        // intersect branch and keeps `number & string`.
        let (values, role): (Vec<SemanticNodeId>, MemberMergeRole) =
            if self.saw_own_body && self.saw_heritage {
                (self.own_body_values, MemberMergeRole::OwnBody)
            } else {
                // Intersect every distinct contributor value. Own-body values
                // first preserves authored arm order for the common case.
                let mut values = self.own_body_values;
                for v in self.other_values {
                    if !values.contains(&v) {
                        values.push(v);
                    }
                }
                let role = if self.saw_own_body {
                    MemberMergeRole::OwnBody
                } else if self.saw_heritage {
                    MemberMergeRole::Heritage
                } else {
                    MemberMergeRole::Authored
                };
                (values, role)
            };
        let value = match values.as_slice() {
            [single] => *single,
            _ => merge_value_nodes_recursive(graph, &values),
        };
        // Visibility aggregates to the MOST-RESTRICTIVE accessibility across the
        // contributors that ACTUALLY contribute to the surviving member (the
        // shared merge rule: `Private` > `Protected` > `Public`), selected in
        // LOCKSTEP with the value/role selection:
        //
        // - own-body-shadows-heritage (`saw_own_body && saw_heritage`): the
        //   heritage values are dropped, so only own-body contributors
        //   participate — use the own-body aggregate.
        // - otherwise: every contributor participates — fold the own-body and
        //   heritage/authored aggregates together.
        //
        // A merged member is `Public` ONLY when it is Public in EVERY
        // contributing arm; a member non-public in any contributor stays
        // non-public (never synthesized Public). For a SINGLE contributor the
        // aggregate is exactly that contributor's accessibility, so the
        // single-source case is preserved. The RAW contributor counts (not
        // deduped value-node counts) are folded into the aggregate during
        // `absorb`, so two contributors sharing one value type are still
        // aggregated correctly (the deduped-value-node count would have
        // mis-treated them as a single source).
        let visibility = if self.saw_own_body && self.saw_heritage {
            self.own_body_visibility_agg.unwrap_or_default()
        } else {
            match (self.own_body_visibility_agg, self.other_visibility_agg) {
                (Some(own), Some(other)) => own.most_restrictive(other),
                (Some(own), None) => own,
                (None, Some(other)) => other,
                (None, None) => verter_type_expr::MemberVisibility::default(),
            }
        };
        // The surviving member's spans follow the value-selection rule: an
        // own-body result references the own-body declaration site; otherwise
        // the first contributor's site. The declaration file is selected in
        // LOCKSTEP with the spans so the surviving span indexes its own file.
        let (spans, declaration_origin) = if matches!(role, MemberMergeRole::OwnBody) {
            match (self.own_body_spans, self.own_body_origin) {
                (Some(spans), origin) => (spans, origin),
                (None, _) => (self.first_spans.unwrap_or_default(), self.first_origin),
            }
        } else {
            (self.first_spans.unwrap_or_default(), self.first_origin)
        };
        ShallowSurfaceMember {
            name: self.name,
            value,
            optional: self.optional,
            readonly: self.readonly,
            is_method: self.is_method,
            visibility,
            declared_in_macro_type_arg: self.declared_in_macro_type_arg,
            merge_role: role,
            spans,
            declaration_origin,
        }
    }
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

/// Merge per-arm union surfaces under the TS-correct common-member rule,
/// matching the canonical union common-member semantics:
///
/// - A member survives iff it is present (by name) in EVERY resolvable arm.
/// - Its value is the UNION of the per-arm member value nodes (`(A | B)['k']`
///   is `A['k'] | B['k']`).
/// - `optional` iff optional in ANY arm; `readonly` iff readonly in ALL arms.
/// - `is_method` / `declared_in_macro_type_arg` are `false`; the merge role is
///   [`MemberMergeRole::Authored`] — a synthesized common member is reached
///   THROUGH the union, never the macro-T own body or a heritage overlay, so
///   it must not pretend to be own-body / heritage.
/// - A union has no single call/construct/index surface, so the merged surface
///   carries none.
///
/// Returns `Some(empty)` when there are no common members (a disjoint union
/// resolved, it simply has no common members) or when any arm is a non-Object
/// (`None`) surface — a union with an unreadable / non-Object arm has no
/// common Object members. Returns `None` only when the arm vector is empty
/// (defensive).
fn merge_union_surfaces(
    graph: &SemanticGraphStore,
    arm_surfaces: &[Option<ShallowSurface>],
) -> Option<ShallowSurface> {
    use crate::semantic_query::MemberMergeRole;

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
    let mut members: Vec<ShallowSurfaceMember> = Vec::new();
    for first_member in &live[0].members {
        let mut per_arm_values: Vec<SemanticNodeId> = Vec::with_capacity(live.len());
        let mut per_arm_visibilities: Vec<verter_type_expr::MemberVisibility> =
            Vec::with_capacity(live.len());
        let mut optional_in_any = false;
        let mut readonly_in_all = true;
        let mut present_in_all = true;
        for arm in &live {
            match arm.members.iter().find(|m| m.name == first_member.name) {
                Some(arm_member) => {
                    per_arm_values.push(arm_member.value);
                    per_arm_visibilities.push(arm_member.visibility);
                    optional_in_any |= arm_member.optional;
                    readonly_in_all &= arm_member.readonly;
                }
                None => {
                    present_in_all = false;
                    break;
                }
            }
        }
        if !present_in_all {
            continue;
        }
        // Value type = union of the per-arm member values. A single shared
        // value node stays as-is (no singleton union wrapper).
        let value = if per_arm_values.len() == 1 {
            per_arm_values[0]
        } else {
            graph.intern_node(SemanticNodeData::Union(Arc::from(
                per_arm_values.into_boxed_slice(),
            )))
        };
        members.push(ShallowSurfaceMember {
            name: Arc::clone(&first_member.name),
            value,
            optional: optional_in_any,
            readonly: readonly_in_all,
            is_method: false,
            // Union common-member (`(A|B)['k']`): aggregate the MOST-RESTRICTIVE
            // accessibility across the per-arm contributors via the shared fold,
            // so a member non-public in any arm is never synthesized as `Public`
            // (matching the `_for_macro` sibling and TS member-access rules). For
            // a single declaring arm the fold returns that arm's accessibility.
            visibility: verter_type_expr::MemberVisibility::merge_member_visibility(
                per_arm_visibilities,
            ),
            declared_in_macro_type_arg: false,
            merge_role: MemberMergeRole::Authored,
            // Union common-member: the name appears in every arm, so there is
            // no single source declaration site — genuinely synthetic. No spans
            // and no single declaration file (a multi-origin fact).
            spans: verter_type_expr::MemberSpans::default(),
            declaration_origin: None,
        });
    }
    Some(ShallowSurface {
        members,
        call_signatures: Vec::new(),
        construct_signatures: Vec::new(),
        index_signatures: Vec::new(),
        keyspace: None,
    })
}

/// Merge per-arm union surfaces under the **Vue macro object-surface**
/// rule — the UNION of arm members, NOT the TS
/// property-access common-member intersection.
///
/// A prop / slot present in ANY union arm is part of the component macro
/// surface (`defineProps<FixedProps | BubbleProps>()` declares every
/// arm's props). Per the codex BINDING ruling:
///
/// - A member survives iff it is present (by name) in AT LEAST ONE arm.
/// - Its value is the UNION of the per-arm member values for the arms
///   that declare it (a single declaring arm stays as-is).
/// - `optional` iff optional in ANY arm OR ABSENT from any arm (a member
///   not declared by every arm is optional on the merged surface).
/// - `readonly` iff readonly in ALL arms that declare it.
/// - `is_method` / `declared_in_macro_type_arg` are `false`; merge role is
///   [`MemberMergeRole::Authored`] — a member reached THROUGH the union is
///   neither macro-T own-body nor heritage.
/// - A union has no single call/construct/index surface, so the merged
///   surface carries none.
///
/// Returns `Some(empty)` when no arm declares any Object member. Returns
/// `None` only when the arm vector is empty (defensive). Unlike the
/// common-member rule, a non-Object (`None`) arm does NOT collapse the
/// whole surface — the Object arms still contribute their members (a
/// `{ a } | string` macro surface publishes `a`, optional).
fn merge_union_surfaces_for_macro(
    graph: &SemanticGraphStore,
    arm_surfaces: &[Option<ShallowSurface>],
) -> Option<ShallowSurface> {
    use crate::semantic_query::MemberMergeRole;

    if arm_surfaces.is_empty() {
        return None;
    }
    let arm_count = arm_surfaces.len();
    let live: Vec<&ShallowSurface> = arm_surfaces.iter().filter_map(|s| s.as_ref()).collect();
    if live.is_empty() {
        return Some(ShallowSurface::empty());
    }
    // A non-Object arm (`None`) means that arm declares NO members, so
    // every member is effectively absent from it → optional on the union.
    let has_non_object_arm = arm_surfaces.iter().any(|s| s.is_none());

    // Enumerate member names in first-seen order across all arms.
    let mut ordered_names: Vec<Arc<str>> = Vec::new();
    let mut seen: rustc_hash::FxHashSet<Arc<str>> = rustc_hash::FxHashSet::default();
    for arm in &live {
        for member in &arm.members {
            if seen.insert(Arc::clone(&member.name)) {
                ordered_names.push(Arc::clone(&member.name));
            }
        }
    }

    let mut members: Vec<ShallowSurfaceMember> = Vec::with_capacity(ordered_names.len());
    for name in &ordered_names {
        let mut per_arm_values: Vec<SemanticNodeId> = Vec::with_capacity(live.len());
        let mut per_arm_visibilities: Vec<verter_type_expr::MemberVisibility> =
            Vec::with_capacity(live.len());
        let mut optional_in_any = false;
        let mut readonly_in_all = true;
        let mut declaring_arms = 0usize;
        for arm in &live {
            if let Some(arm_member) = arm.members.iter().find(|m| &m.name == name) {
                declaring_arms += 1;
                per_arm_visibilities.push(arm_member.visibility);
                per_arm_values.push(arm_member.value);
                optional_in_any |= arm_member.optional;
                readonly_in_all &= arm_member.readonly;
            }
        }
        // Aggregate the MOST-RESTRICTIVE accessibility across EVERY declaring arm
        // via the shared fold: the merged member is `Public` only when it is
        // Public in EVERY declaring arm; a member non-public in any arm stays
        // non-public (never synthesized Public). For a member declared by exactly
        // one arm the fold returns that arm's accessibility.
        let merged_visibility =
            verter_type_expr::MemberVisibility::merge_member_visibility(per_arm_visibilities);
        // Absent from at least one arm (a live arm without it, or a
        // non-Object arm) ⇒ optional on the merged surface.
        if declaring_arms < arm_count || has_non_object_arm {
            optional_in_any = true;
        }
        let value = if per_arm_values.len() == 1 {
            per_arm_values[0]
        } else {
            graph.intern_node(SemanticNodeData::Union(Arc::from(
                per_arm_values.into_boxed_slice(),
            )))
        };
        members.push(ShallowSurfaceMember {
            name: Arc::clone(name),
            value,
            optional: optional_in_any,
            readonly: readonly_in_all,
            is_method: false,
            // Most-restrictive accessibility across all declaring arms (the
            // shared merge rule): Public only when Public in every declaring
            // arm.
            visibility: merged_visibility,
            declared_in_macro_type_arg: false,
            merge_role: MemberMergeRole::Authored,
            // Union arm-member: reached THROUGH the union, no single source
            // declaration site — genuinely synthetic (multi-origin).
            spans: verter_type_expr::MemberSpans::default(),
            declaration_origin: None,
        });
    }
    Some(ShallowSurface {
        members,
        call_signatures: Vec::new(),
        construct_signatures: Vec::new(),
        index_signatures: Vec::new(),
        keyspace: None,
    })
}

fn surface_view_from_shallow(surface: &ShallowSurface) -> SurfaceView {
    // `declared_in_macro_type_arg` and `merge_role` propagate from each
    // `ShallowSurfaceMember`. The dispatch walker preserves both through
    // intersection / union merges, so round-tripping through
    // `ShallowSurface` is lossless for the member provenance.
    //
    // Call / construct / index signatures and the keyspace are carried
    // verbatim from the `ShallowSurface` — the empty-path Shallow projection
    // no longer drops them (the load-bearing fix for the type-resolution
    // unification). `has_index_signature` is derived from the carried index
    // signatures so it stays consistent with them.
    let members: Vec<SurfaceMember> = surface
        .members
        .iter()
        .map(|m| SurfaceMember {
            name: Arc::clone(&m.name),
            value: m.value,
            optional: m.optional,
            readonly: m.readonly,
            is_method: m.is_method,
            // Carry the walker's preserved declared accessibility back onto the
            // graph member (round-trip through ShallowSurface is lossless).
            visibility: m.visibility,
            declared_in_macro_type_arg: m.declared_in_macro_type_arg,
            merge_role: m.merge_role,
            // Carry the walker's preserved OXC spans back onto the graph member.
            spans: m.spans,
            // Carry the preserved declaration file back onto the graph member.
            declaration_origin: m.declaration_origin.clone(),
        })
        .collect();
    SurfaceView {
        members: Arc::from(members.into_boxed_slice()),
        call_signatures: Arc::from(surface.call_signatures.clone().into_boxed_slice()),
        construct_signatures: Arc::from(surface.construct_signatures.clone().into_boxed_slice()),
        index_signatures: Arc::from(surface.index_signatures.clone().into_boxed_slice()),
        keyspace: surface.keyspace,
        has_index_signature: !surface.index_signatures.is_empty(),
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

/// Worklist frame for the iterative `expand_empty_path_terminal`
/// driver. `Expand` advances one node; `Combine*` rebuilds a
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

#[cfg(test)]
mod m1_merge_visibility_tests {
    //! M1: contributor-aggregation visibility rules for the intersection
    //! (`merge_intersection_surfaces_with_graph`) and union
    //! (`merge_union_surfaces_for_macro`) surface merges.
    //!
    //! A merged member is `Public` ONLY when it is Public in EVERY contributing
    //! arm; a member non-public in any contributor stays non-public (never
    //! synthesized Public — BUG 2). The aggregation folds the MOST-RESTRICTIVE
    //! contributor visibility over the RAW contributor stream, so two
    //! contributors sharing one value type are still aggregated correctly
    //! (BUG 1 — the deduped value-node count would have mis-treated them as a
    //! single source). The result is arm-order INDEPENDENT.

    use std::sync::Arc;

    use verter_type_expr::{MemberSpans, MemberVisibility};

    use super::{
        merge_intersection_surfaces_with_graph, merge_union_surfaces_for_macro, ShallowSurface,
        ShallowSurfaceMember,
    };
    use crate::semantic_query::{MemberMergeRole, PrimitiveKind, SemanticNodeData};
    use crate::semantic_query_memo::SemanticGraphStore;

    fn member(
        graph: &SemanticGraphStore,
        name: &str,
        vis: MemberVisibility,
        role: MemberMergeRole,
        value_prim: PrimitiveKind,
    ) -> ShallowSurfaceMember {
        ShallowSurfaceMember {
            name: Arc::from(name),
            value: graph.intern_node(SemanticNodeData::Primitive(value_prim)),
            optional: false,
            readonly: false,
            is_method: false,
            visibility: vis,
            declared_in_macro_type_arg: false,
            merge_role: role,
            spans: MemberSpans::default(),
            declaration_origin: None,
        }
    }

    fn one_member_surface(m: ShallowSurfaceMember) -> ShallowSurface {
        ShallowSurface {
            members: vec![m],
            call_signatures: Vec::new(),
            construct_signatures: Vec::new(),
            index_signatures: Vec::new(),
            keyspace: None,
        }
    }

    fn merged_member_visibility(surface: &ShallowSurface, name: &str) -> MemberVisibility {
        surface
            .members
            .iter()
            .find(|m| m.name.as_ref() == name)
            .unwrap_or_else(|| panic!("merged surface must contain `{name}`"))
            .visibility
    }

    /// Intersection `A & B`: a member present in both arms, non-public in BOTH,
    /// must stay non-public — aggregated to the most-restrictive — and the
    /// result must be arm-order INDEPENDENT.
    ///
    /// This is the BUG 1 + BUG 2 case: both arms carry the SAME value type
    /// (`number`), so the deduped value-node count is 1; the pre-fix code
    /// therefore treated it as a single contributor and used the FIRST arm's
    /// visibility (arm-order dependent), and a genuine multi-contributor would
    /// have collapsed to Public. The fix aggregates most-restrictive over RAW
    /// contributors.
    #[test]
    fn intersection_same_value_two_contributors_aggregates_most_restrictive() {
        let graph = SemanticGraphStore::new();

        // Arm order [Protected, Private] and [Private, Protected] must both
        // yield Private (most-restrictive), independent of order.
        let prot_then_priv = merge_intersection_surfaces_with_graph(
            &graph,
            &[
                Some(one_member_surface(member(
                    &graph,
                    "x",
                    MemberVisibility::Protected,
                    MemberMergeRole::Authored,
                    PrimitiveKind::Number,
                ))),
                Some(one_member_surface(member(
                    &graph,
                    "x",
                    MemberVisibility::Private,
                    MemberMergeRole::Authored,
                    PrimitiveKind::Number,
                ))),
            ],
        )
        .expect("intersection of two object arms merges");
        assert_eq!(
            merged_member_visibility(&prot_then_priv, "x"),
            MemberVisibility::Private,
            "protected & private (same value) must aggregate to Private",
        );

        let priv_then_prot = merge_intersection_surfaces_with_graph(
            &graph,
            &[
                Some(one_member_surface(member(
                    &graph,
                    "x",
                    MemberVisibility::Private,
                    MemberMergeRole::Authored,
                    PrimitiveKind::Number,
                ))),
                Some(one_member_surface(member(
                    &graph,
                    "x",
                    MemberVisibility::Protected,
                    MemberMergeRole::Authored,
                    PrimitiveKind::Number,
                ))),
            ],
        )
        .expect("intersection merges");
        assert_eq!(
            merged_member_visibility(&priv_then_prot, "x"),
            MemberVisibility::Private,
            "arm order must NOT change the merged visibility (private & protected = Private)",
        );
    }

    /// Intersection: a member public in BOTH arms stays Public; a member public
    /// in one arm and private in the other becomes Private (never Public).
    #[test]
    fn intersection_public_only_when_public_in_all_arms() {
        let graph = SemanticGraphStore::new();

        let both_public = merge_intersection_surfaces_with_graph(
            &graph,
            &[
                Some(one_member_surface(member(
                    &graph,
                    "x",
                    MemberVisibility::Public,
                    MemberMergeRole::Authored,
                    PrimitiveKind::Number,
                ))),
                Some(one_member_surface(member(
                    &graph,
                    "x",
                    MemberVisibility::Public,
                    MemberMergeRole::Authored,
                    PrimitiveKind::String,
                ))),
            ],
        )
        .expect("intersection merges");
        assert_eq!(
            merged_member_visibility(&both_public, "x"),
            MemberVisibility::Public,
            "public in both arms stays Public",
        );

        let one_private = merge_intersection_surfaces_with_graph(
            &graph,
            &[
                Some(one_member_surface(member(
                    &graph,
                    "x",
                    MemberVisibility::Public,
                    MemberMergeRole::Authored,
                    PrimitiveKind::Number,
                ))),
                Some(one_member_surface(member(
                    &graph,
                    "x",
                    MemberVisibility::Private,
                    MemberMergeRole::Authored,
                    PrimitiveKind::String,
                ))),
            ],
        )
        .expect("intersection merges");
        assert_eq!(
            merged_member_visibility(&one_private, "x"),
            MemberVisibility::Private,
            "public-in-one + private-in-other must be Private (never Public)",
        );
    }

    /// Union `A | B`: a member declared in both arms aggregates to the
    /// most-restrictive, arm-order independent. Public only when Public in every
    /// declaring arm.
    #[test]
    fn union_member_in_all_arms_aggregates_most_restrictive() {
        let graph = SemanticGraphStore::new();

        let prot_then_priv = merge_union_surfaces_for_macro(
            &graph,
            &[
                Some(one_member_surface(member(
                    &graph,
                    "x",
                    MemberVisibility::Protected,
                    MemberMergeRole::Authored,
                    PrimitiveKind::Number,
                ))),
                Some(one_member_surface(member(
                    &graph,
                    "x",
                    MemberVisibility::Private,
                    MemberMergeRole::Authored,
                    PrimitiveKind::Number,
                ))),
            ],
        )
        .expect("union merges");
        assert_eq!(
            merged_member_visibility(&prot_then_priv, "x"),
            MemberVisibility::Private,
            "union protected|private must aggregate to Private",
        );

        // Arm-order independent.
        let priv_then_prot = merge_union_surfaces_for_macro(
            &graph,
            &[
                Some(one_member_surface(member(
                    &graph,
                    "x",
                    MemberVisibility::Private,
                    MemberMergeRole::Authored,
                    PrimitiveKind::Number,
                ))),
                Some(one_member_surface(member(
                    &graph,
                    "x",
                    MemberVisibility::Protected,
                    MemberMergeRole::Authored,
                    PrimitiveKind::Number,
                ))),
            ],
        )
        .expect("union merges");
        assert_eq!(
            merged_member_visibility(&priv_then_prot, "x"),
            MemberVisibility::Private,
            "union arm order must NOT change merged visibility",
        );
    }

    /// Union: a member declared in only ONE arm carries that arm's visibility
    /// (the single-source case is preserved by the aggregate).
    #[test]
    fn union_single_arm_member_keeps_its_visibility() {
        let graph = SemanticGraphStore::new();
        let merged = merge_union_surfaces_for_macro(
            &graph,
            &[
                Some(one_member_surface(member(
                    &graph,
                    "only_a",
                    MemberVisibility::Protected,
                    MemberMergeRole::Authored,
                    PrimitiveKind::Number,
                ))),
                Some(one_member_surface(member(
                    &graph,
                    "only_b",
                    MemberVisibility::Public,
                    MemberMergeRole::Authored,
                    PrimitiveKind::String,
                ))),
            ],
        )
        .expect("union merges");
        assert_eq!(
            merged_member_visibility(&merged, "only_a"),
            MemberVisibility::Protected,
            "a member in a single arm keeps that arm's accessibility",
        );
        assert_eq!(
            merged_member_visibility(&merged, "only_b"),
            MemberVisibility::Public,
        );
    }
}
