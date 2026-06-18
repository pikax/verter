//! `evaluate_deferred_semantic_node` — deferred-shell evaluation
//! fix-point loop ( Change Split + §2 guard contract row for
//! `evaluate_deferred_semantic_node`).
//!
//! Walks `SemanticNodeData` unwrapping `Alias(target)` hops,
//! substituting `Instantiate` shells, and projecting single-segment
//! `IndexedAccess` shells through dispatch re-entry. Returns the
//! caller's current node on cyclic re-entry (fix-point) per
//! Also hosts `normalized_index_key_node` which belongs to the
//! evaluation surface.

use std::cell::Cell;
use std::sync::Arc;

use super::ProjectSemanticDispatch;
use crate::semantic_query::{
    IndexKey, LiteralValue, PathSegment, ProjectionMode, ProjectionReductionContext, QueryError,
    QueryResult, SemanticNodeData, SemanticNodeId, SemanticQueryKey,
};

/// Hard ceiling on recursive `evaluate_deferred_semantic_node_with_context`
/// depth. The evaluator's fix-point walk usually terminates within a few
/// hops (Alias → KeyOf → IndexedAccess → leaf). Pathological mapped-type
/// patterns that combine generic instantiation with nested conditionals
/// over keyspace-derived literals (e.g.
/// `ChatMessagesSlots<T>`'s per-K loop) can fan out unboundedly through
/// the operator re-dispatch chain. The cap prevents the per-K loop from
/// running away even when the substitute hash-cons + evaluate memo
/// cannot collapse the work (every K's substituted body produces a new
/// node id whose recursive evaluator chain re-enters).
///
/// On exhaustion the evaluator returns the current node — a structural-
/// transit carrier-stop equivalent. The downstream materialiser's
/// Opaque-fallback contract keeps the surface addressable by re-dispatch
/// without leaking torn / partial results into shared caches.
///
/// **Sizing rationale.** Default thread stacks on Windows are 1 MB and
/// the evaluator frame is large (~30-50 KB of locals + temporaries with
/// debug info). 256 stack frames at 50 KB = 12 MB, which fits within
/// the workspace-test `RUST_MIN_STACK=134_217_728` (128 MB) budget while
/// staying well under the OS default thread-stack limit's worst case.
/// The ceiling is high enough to never fire on well-formed corpus
/// components (Theme / ChatMessage / Table all run within a few dozen
/// recursive entries); it fires on the pathological tail (e.g.
/// `ChatMessagesSlots<T>`'s thousand-recursive-entry chain) where work
/// would otherwise grow exponentially.
const EVALUATE_DEFERRED_DEPTH_CEILING: u32 = 256;

thread_local! {
    /// Per-thread recursive-depth counter for
    /// `evaluate_deferred_semantic_node_with_context`. The counter is
    /// bumped at the entry of every (potentially recursive) call and
    /// decremented at exit via an RAII guard. When the counter
    /// crosses [`EVALUATE_DEFERRED_DEPTH_CEILING`] the inner call
    /// returns the carrier-stop sentinel (the input `node` itself)
    /// instead of recursing further.
    static EVALUATE_DEFERRED_DEPTH: Cell<u32> = const { Cell::new(0) };
    /// Per-thread sticky truncation flag. Set when the depth guard
    /// fires (any recursive frame on the current thread's evaluator
    /// call chain hit `over_ceiling`). Every publish site consults
    /// this flag and SKIPS publishing into
    /// `evaluate_deferred_memo` when it is set — a parent frame that
    /// consumed a truncated child's input-node carrier must not
    /// publish its derived result as a warm entry, because that
    /// result is budget-tainted (downstream operators reduced
    /// against a carrier-stop, not a fully-resolved sub-evaluation).
    ///
    /// The flag is reset to `false` only when the top-level entry
    /// frame (depth was 0 at enter) exits — every nested frame
    /// preserves the sticky state up the call chain. This routes
    /// budget-exhaustion through `ComputeAdmission::ReturnOnly`
    /// semantics at the evaluator layer: no torn / budget-exhausted
    /// result ever populates the shared memo.
    static EVALUATE_DEFERRED_TRUNCATED: Cell<bool> = const { Cell::new(false) };
}

/// RAII guard for [`EVALUATE_DEFERRED_DEPTH`]. Increments on `enter`,
/// decrements on `Drop`. The guard's `over_ceiling` flag captures
/// whether the entry exceeded the ceiling — callers consult it to
/// fast-return without recursing.
///
/// The guard's `is_top_level` flag records whether the depth at
/// `enter` was 0 — only the top-level frame may reset the sticky
/// `EVALUATE_DEFERRED_TRUNCATED` flag on `Drop`, so a nested
/// recursive frame's exit never clears a child's truncation signal
/// before the parent has a chance to honour it.
struct DepthGuard {
    over_ceiling: bool,
    is_top_level: bool,
}

impl DepthGuard {
    fn enter() -> Self {
        EVALUATE_DEFERRED_DEPTH.with(|cell| {
            let depth = cell.get();
            cell.set(depth.saturating_add(1));
            let over_ceiling = depth >= EVALUATE_DEFERRED_DEPTH_CEILING;
            if over_ceiling {
                EVALUATE_DEFERRED_TRUNCATED.with(|flag| flag.set(true));
            }
            DepthGuard {
                over_ceiling,
                is_top_level: depth == 0,
            }
        })
    }
}

impl Drop for DepthGuard {
    fn drop(&mut self) {
        EVALUATE_DEFERRED_DEPTH.with(|cell| {
            let depth = cell.get();
            cell.set(depth.saturating_sub(1));
        });
        if self.is_top_level {
            EVALUATE_DEFERRED_TRUNCATED.with(|flag| flag.set(false));
        }
    }
}

/// Returns `true` iff the depth guard has fired anywhere on the
/// current thread's evaluator call chain since the top-level entry.
/// Publish sites consult this flag to gate writes into
/// `evaluate_deferred_memo` — a `true` reading means at least one
/// recursive sub-evaluation returned its input-node carrier-stop,
/// and any derived result the current frame produced is
/// budget-tainted.
fn evaluator_truncated() -> bool {
    EVALUATE_DEFERRED_TRUNCATED.with(|flag| flag.get())
}

/// Entry-scoped outcome of a deferred-shell evaluation: the resolved
/// `node` PLUS whether the evaluation that produced it was PARTIAL — a
/// nested read tripped `BudgetExceeded` / recursion / a fatal walker
/// miss, OR a recursive sub-evaluation was itself partial. The
/// `result_is_partial` bit is the entry-scoped admission authority for
/// `evaluate_deferred_memo`: only a complete result is published, so a
/// budget-tainted result is withheld REGARDLESS of whether a
/// `RequestContext` is installed (the request-global suppress sticky is
/// NOT the authority — see [`ProjectSemanticDispatch::evaluate_deferred_outcome`]).
#[derive(Clone, Copy)]
struct EvaluateDeferredOutcome {
    node: SemanticNodeId,
    result_is_partial: bool,
}

impl EvaluateDeferredOutcome {
    /// A complete (warm-admissible) result.
    fn complete(node: SemanticNodeId) -> Self {
        Self {
            node,
            result_is_partial: false,
        }
    }

    /// A partial (never-published) carrier-stop result.
    fn partial(node: SemanticNodeId) -> Self {
        Self {
            node,
            result_is_partial: true,
        }
    }
}

impl<'a> ProjectSemanticDispatch<'a> {
    pub(super) fn normalized_index_key_node(&self, node: SemanticNodeId) -> IndexKey {
        self.normalized_index_key_node_outcome(node).0
    }

    /// Outcome variant of [`Self::normalized_index_key_node`] threading the
    /// entry-scoped partiality of the index-node evaluation. Resolving the
    /// index expression is a nested deferred call, so a
    /// budget-/recursion-truncated index resolution makes the enclosing
    /// `IndexedAccess` reduction partial — the bool propagates up to the
    /// caller's [`Self::evaluate_deferred_outcome`] admission gate.
    fn normalized_index_key_node_outcome(&self, node: SemanticNodeId) -> (IndexKey, bool) {
        let outcome = self.evaluate_deferred_outcome(
            node,
            ProjectionReductionContext::published(ProjectionMode::Expanded),
        );
        let partial = outcome.result_is_partial;
        let resolved = outcome.node;
        match self.graph().node_data(resolved).as_deref() {
            Some(SemanticNodeData::Literal(LiteralValue::String(text))) => {
                (IndexKey::String(Arc::from(text.as_str())), partial)
            }
            Some(SemanticNodeData::Literal(LiteralValue::Number(number))) => {
                // Bounded integer-convention fold: `IndexKey::Number`
                // admits ONLY literals whose i64 `Display` IS the
                // canonical `js_number_to_string` spelling (the single
                // shared producer predicate —
                // `build::integer_convention_index_key`). Everything
                // else stays `TypeNode` for the walker's G4.5
                // canonical-needle recovery.
                let key = match super::build::integer_convention_index_key(*number) {
                    Some(integer) => IndexKey::Number(integer),
                    None => IndexKey::TypeNode(resolved),
                };
                (key, partial)
            }
            Some(SemanticNodeData::Alias(target)) => {
                let (key, inner_partial) = self.normalized_index_key_node_outcome(*target);
                (key, partial | inner_partial)
            }
            _ => (IndexKey::TypeNode(resolved), partial),
        }
    }

    pub(super) fn evaluate_deferred_semantic_node(&self, node: SemanticNodeId) -> SemanticNodeId {
        // Default to a `Published + Expanded` context. Publication
        // callers (the bounded reducer, mapper value substitution,
        // conditional check evaluation, builtin-utility argument
        // resolution) all need operator dispatches to terminate at
        // their fully-reduced surface. The demand-driven reducer retires the
        // *implicit* Expanded unwrap by exposing
        // [`Self::evaluate_deferred_semantic_node_with_context`] so
        // structural-transit callers (relation engine identity-
        // carrier unwrap and object-vs-record arms) can opt out of
        // publication reduction explicitly.
        self.evaluate_deferred_semantic_node_with_context(
            node,
            ProjectionReductionContext::published(ProjectionMode::Expanded),
        )
    }

    /// Context-explicit variant of
    /// [`Self::evaluate_deferred_semantic_node`]
    /// (demand-driven reducer). The caller supplies the
    /// [`ProjectionReductionContext`] that flows into every operator
    /// re-dispatch (`KeyOf`, `MappedType`, decl-placeholder
    /// `Instantiate`) so a `StructuralTransit` walk does not reify
    /// per-member edges along its evaluation path.
    pub(super) fn evaluate_deferred_semantic_node_with_context(
        &self,
        node: SemanticNodeId,
        reduction_context: ProjectionReductionContext,
    ) -> SemanticNodeId {
        self.evaluate_deferred_outcome(node, reduction_context).node
    }

    /// Entry-scoped workhorse for the deferred-shell evaluator. Returns the
    /// resolved node PLUS whether THIS evaluation was partial (see
    /// [`EvaluateDeferredOutcome`]).
    ///
    /// The publish gate is ENTRY-scoped: it admits into the shared
    /// `evaluate_deferred_memo` ONLY when the evaluated entry is itself
    /// complete (`!result_is_partial && !evaluator_truncated()`). The
    /// `result_is_partial` accumulator OR-folds every nested
    /// `execute_read`'s `result_is_partial` and every recursive
    /// sub-evaluation's partial flag, so a budget-/recursion-/fatal-tainted
    /// result is withheld REGARDLESS of whether a `RequestContext` is
    /// installed — closing the no-`RequestContext` (`audit Noop`) hole where
    /// the request-global suppress sticky reads `false`. The request sticky
    /// (`current_materialization_cache_suppress`) is NOT the admission
    /// authority here; `observe_component_meta_read_suppress` is retained
    /// PURELY to propagate the same partiality to the request /
    /// cold-compute scope (the component-meta / materialize warm gates).
    fn evaluate_deferred_outcome(
        &self,
        mut node: SemanticNodeId,
        reduction_context: ProjectionReductionContext,
    ) -> EvaluateDeferredOutcome {
        // Hash-cons memo. The store-owned `evaluate_deferred_memo`
        // collapses repeated `(node, context)` evaluations across
        // call sites. The evaluator's fix-point walk is a pure
        // function of `(node, context)` because every recursive call
        // and operator re-dispatch consumes the SAME context and
        // routes through `execute_cooperative` (which is itself
        // content-keyed). The dominant per-K mapped-type loop win:
        // a K-independent subtree like `MessageBase<T>` embedded
        // inside a K-dependent value expression evaluates once per
        // K through the recursive walk pre-memo; post-memo every
        // subsequent visit collapses to one `DashMap::get`.
        let entry_node = node;
        if let Some(cached) = self
            .graph()
            .evaluate_deferred_memo_get(entry_node, reduction_context)
        {
            // A memo hit is COMPLETE by construction: only complete entries
            // are ever admitted by the publish gate below, so a hit carries
            // `result_is_partial = false`.
            return EvaluateDeferredOutcome::complete(cached);
        }
        // Cooperative fail-fast rail: pathological mapped-type per-K
        // loops (e.g. `ChatMessagesSlots<T>` whose per-K body produces
        // a new substituted node id per K, defeating the entry-node
        // memo) can drive `evaluate_deferred_*` into arbitrarily deep
        // recursive operator re-dispatch. The TLS depth guard caps
        // recursion at `EVALUATE_DEFERRED_DEPTH_CEILING`; on
        // exhaustion we return the input node (carrier-stop) WITHOUT
        // publishing into the memo (no torn / budget-exhausted results
        // ever populate shared caches — `ComputeAdmission::ReturnOnly`
        // policy applied at this layer).
        let _depth_guard = DepthGuard::enter();
        if _depth_guard.over_ceiling {
            // Depth-truncated carrier-stop is a partial result.
            return EvaluateDeferredOutcome::partial(entry_node);
        }
        let mut visited = rustc_hash::FxHashSet::default();
        visited.insert(node);
        // Entry-scoped partiality accumulator: OR-folds every nested read's
        // `result_is_partial` and every recursive sub-evaluation's partial
        // flag. This is the admission authority for the shared memo below.
        let mut result_is_partial = false;
        let result = loop {
            let Some(data) = self.graph().node_data(node) else {
                break self.opaque(QueryError::Miss);
            };
            let next = match data.as_ref() {
                SemanticNodeData::Alias(target) => *target,
                SemanticNodeData::KeyOf { base } => {
                    let base_outcome = self.evaluate_deferred_outcome(*base, reduction_context);
                    result_is_partial |= base_outcome.result_is_partial;
                    let read = self.execute_read(SemanticQueryKey::KeyOf {
                        base: base_outcome.node,
                        context: reduction_context,
                    });
                    result_is_partial |= read.result_is_partial;
                    // Two-signal fold: the deferred-shell evaluator returns a
                    // bare node (hash-cons memoised), so it folds a genuinely
                    // incomplete nested read onto the request's sticky partial
                    // flag — the component-meta / materialize warm gates
                    // consult that flag and refuse a partial-tainted result.
                    crate::request_context::observe_component_meta_read_suppress(&read);
                    match read.value {
                        QueryResult::Value(id) => id,
                        _ => break self.opaque(QueryError::Miss),
                    }
                }
                SemanticNodeData::IndexedAccess { object, index } => {
                    // Path-precision rule: the deferred `T[K]` shell is
                    // a single indexed-access hop whose OBJECT is the
                    // intermediate base and whose projection of `K` is
                    // the TERMINAL segment. The object recursion demotes
                    // to `Navigate` so the intermediate surface stays
                    // shallow (its sibling members — `$emit` / `$slots` /
                    // unrelated props — are NOT eagerly expanded when the
                    // caller demanded `Expanded`), while the terminal
                    // single-hop projection runs in the CALLER's mode so
                    // a demanded `Expanded` terminal resolves its carrier.
                    let object_outcome = self.evaluate_deferred_outcome(
                        *object,
                        reduction_context.with_mode(ProjectionMode::Navigate),
                    );
                    result_is_partial |= object_outcome.result_is_partial;
                    let index = match index {
                        IndexKey::String(text) => IndexKey::String(Arc::clone(text)),
                        IndexKey::Number(number) => IndexKey::Number(*number),
                        IndexKey::TypeNode(node) => {
                            let (key, partial) = self.normalized_index_key_node_outcome(*node);
                            result_is_partial |= partial;
                            key
                        }
                    };
                    let read = self.execute_read(SemanticQueryKey::IndexedAccess {
                        base: object_outcome.node,
                        index,
                        mode: reduction_context.mode,
                    });
                    result_is_partial |= read.result_is_partial;
                    crate::request_context::observe_component_meta_read_suppress(&read);
                    match read.value {
                        QueryResult::Value(id) => id,
                        _ => break self.opaque(QueryError::Miss),
                    }
                }
                SemanticNodeData::Mapped { source, mapper } => {
                    let read = self.execute_read(SemanticQueryKey::MappedType {
                        source: *source,
                        mapper: mapper.clone(),
                        context: reduction_context,
                    });
                    result_is_partial |= read.result_is_partial;
                    crate::request_context::observe_component_meta_read_suppress(&read);
                    match read.value {
                        QueryResult::Value(id) => id,
                        _ => break self.opaque(QueryError::Miss),
                    }
                }
                SemanticNodeData::TypeOf(_) => {
                    // `typeof value.path<args>`: resolve the value root through
                    // the single typeof query, PROJECT the carrier's dotted
                    // path, THEN apply the carrier's instantiation `type_args`
                    // to the projected signature (resolve → project → apply,
                    // mirroring the eager lowering order). The enclosing
                    // evaluation's demand rides the key — operator recursion
                    // never widens a deferred typeof carrier past the caller's
                    // mode.
                    let (value_root, path) = data.typeof_head().expect("TypeOf carrier head");
                    // Read the carrier args from the SAME borrow (owned copy so
                    // the borrow is not held across the apply call).
                    let type_args: Vec<SemanticNodeId> = data.carrier_type_args().to_vec();
                    let read = self
                        .execute_read(self.typeof_key_for(value_root.clone(), reduction_context));
                    result_is_partial |= read.result_is_partial;
                    crate::request_context::observe_component_meta_read_suppress(&read);
                    let root = match read.value {
                        QueryResult::Value(id) => id,
                        _ => break self.opaque(QueryError::Miss),
                    };
                    let projected = if path.is_empty() {
                        root
                    } else {
                        let projection_path: Arc<[PathSegment]> = Arc::from(
                            path.iter()
                                .map(|segment| PathSegment::Member(Arc::clone(segment)))
                                .collect::<Vec<_>>()
                                .into_boxed_slice(),
                        );
                        let read = self.execute_read(SemanticQueryKey::ProjectPath {
                            base: root,
                            path: projection_path,
                            context: crate::semantic_query::ProjectionReductionContext::published(
                                ProjectionMode::Navigate,
                            ),
                        });
                        result_is_partial |= read.result_is_partial;
                        crate::request_context::observe_component_meta_read_suppress(&read);
                        match read.value {
                            QueryResult::Value(id) => id,
                            _ => break self.opaque(QueryError::Miss),
                        }
                    };
                    // Instantiation expression (`typeof C.make<string>`): apply
                    // the lowered type arguments to the projected generic
                    // signature. An arity/shape mismatch composes an honest
                    // `Opaque(Miss)` from `apply_typeof_instantiation_args`
                    // AFTER the projection.
                    if type_args.is_empty() {
                        projected
                    } else {
                        self.apply_typeof_instantiation_args(projected, &type_args)
                    }
                }
                SemanticNodeData::Conditional {
                    check,
                    extends,
                    true_branch_ref,
                    false_branch_ref,
                    distributive,
                } => {
                    let read = self.execute_read(SemanticQueryKey::Conditional {
                        check: *check,
                        extends: *extends,
                        true_branch: *true_branch_ref,
                        false_branch: *false_branch_ref,
                        distributive: *distributive,
                    });
                    result_is_partial |= read.result_is_partial;
                    crate::request_context::observe_component_meta_read_suppress(&read);
                    match read.value {
                        QueryResult::Value(id) => id,
                        _ => break self.opaque(QueryError::Miss),
                    }
                }
                SemanticNodeData::TemplateLiteral {
                    quasis,
                    expressions,
                } => {
                    // Route the carrier through the ONE shared
                    // `TemplateLiteralReduce` query producer (no inline fold).
                    // The producer computes the cartesian product over the
                    // finite literal-union choices of every interpolated
                    // expression (`` `cell:${"name"|"count"}` `` ⇒
                    // `"cell:name" | "cell:count"`), folds an all-single-literal
                    // template to one `Literal`, returns `never` for an empty
                    // product, and carrier-stops to this same hash-consed shell
                    // (so `next == node` and the loop terminates) when any
                    // expression is non-finite. A finite-but-over-cap product
                    // carrier-stops with `result_is_partial`, which the
                    // accumulator below folds so the partial is never
                    // warm-admitted. Dispatching here is what makes
                    // `TemplateLiteralReduce` appear in the request trace.
                    let pattern = Arc::clone(quasis);
                    let args = Arc::clone(expressions);
                    drop(data);
                    let read = self.execute_read(SemanticQueryKey::TemplateLiteralReduce {
                        pattern,
                        args,
                        context: self.template_literal_reduce_context(),
                    });
                    result_is_partial |= read.result_is_partial;
                    crate::request_context::observe_component_meta_read_suppress(&read);
                    match read.value {
                        QueryResult::Value(id) => id,
                        _ => break node,
                    }
                }
                SemanticNodeData::Opaque(QueryError::DeclPlaceholder {
                    canonical_id,
                    name,
                    whole_hash: _,
                }) => {
                    let base = self.type_slot_for(Arc::clone(canonical_id), Arc::clone(name));
                    let owner_canonical = Arc::clone(canonical_id);
                    drop(data);
                    // Demand-driven reducer spec: the
                    // declaration-placeholder unwrap inherits the
                    // caller's reduction context. The implicit
                    // `Published + Expanded` unwrap was the path that
                    // re-opened nested `keyof` / `Mapped` reification
                    // during relation-engine binding; the caller's
                    // `StructuralTransit` context now carries through.
                    let read = self.execute_read(SemanticQueryKey::Instantiate {
                        base,
                        args: Arc::from(Vec::<SemanticNodeId>::new().into_boxed_slice()),
                        context: self.instantiate_context_for(&owner_canonical, reduction_context),
                    });
                    result_is_partial |= read.result_is_partial;
                    crate::request_context::observe_component_meta_read_suppress(&read);
                    match read.value {
                        QueryResult::Value(id) => id,
                        _ => break self.opaque(QueryError::Miss),
                    }
                }
                // `DeclRef`/`InstantiationRef`
                // arms are deliberately NOT added to the deferred-
                // shell evaluator. The path-walker (walk.rs) and the
                // keyspace enumerator (enumerate.rs) handle these
                // carriers under explicit demand contexts, but the
                // deferred-shell evaluator is called from intermediate
                // IndexedAccess hops where eagerly resolving a
                // `DeclRef` would over-evaluate symbolic forms that
                // the slot-binding indexed-access preservation policy
                // expects to stay carrier-shaped (e.g.
                // `AppProps['avatar']` must stay
                // `IndexedAccess { object: DeclRef(AppProps), index }`).
                // The brief's conditional "if they are on the
                // macro-shape enumeration path" scopes this symmetry
                // to the enumeration path only.
                _ => break node,
            };
            if next == node {
                break node;
            }
            // Cyclic re-entry detected (self-referential evaluation) — return
            // current node as fix-point per guard contract row.
            if !visited.insert(next) {
                break node;
            }
            node = next;
        };
        // Depth-truncation carrier-stop. If the depth guard fired anywhere on
        // the current call chain (even in a transitive sub-evaluation), every
        // result along the unwinding path is budget-tainted. A parent frame
        // that consumed a truncated child's input-node carrier may have
        // dispatched downstream operators that returned `Opaque(Miss)` or a
        // partial reduction; admitting that derivative into the warm memo
        // would let a stale, budget-exhausted answer survive across queries.
        // Return the ENTRY node (a structural-transit carrier-stop) rather
        // than the partially-reduced `result`, so downstream re-dispatch
        // starts from the same surface the cache would observe on a future
        // cold call (Opaque-fallback contract applied consistently across the
        // call chain), and propagate partial so the caller withholds too.
        if evaluator_truncated() {
            return EvaluateDeferredOutcome::partial(entry_node);
        }
        // Entry-scoped no-poison admission gate (`ComputeAdmission::ReturnOnly`).
        // Publish ONLY when THIS evaluated entry is itself complete — the
        // `result_is_partial` accumulator OR-folded every nested read's
        // `result_is_partial` and every recursive sub-evaluation's partial
        // flag, so a budget-/recursion-/fatal-tainted result is withheld here
        // independent of any `RequestContext`. The request sticky propagation
        // (`observe_component_meta_read_suppress` at the arms above) feeds the
        // request / cold-compute warm gates, but the admission authority for
        // THIS shared memo is the evaluated entry's OWN completeness.
        if !result_is_partial {
            // Publish the entry-node → result mapping. Concurrent
            // publishers for the same `(entry_node, context)` resolve to
            // structurally identical results (the evaluator is pure on
            // those two inputs), so first-writer-wins is correct.
            self.graph()
                .evaluate_deferred_memo_publish(entry_node, reduction_context, result);
        }
        EvaluateDeferredOutcome {
            node: result,
            result_is_partial,
        }
    }
}
