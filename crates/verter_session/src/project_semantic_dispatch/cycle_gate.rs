//! The materialization cycle gate — the SOLE authority for "does this
//! root declaration transitively reach a cycle through a complex helper
//! surface".
//!
//! [`SemanticQueryKey::ClassifyMaterializationCycleGate`] is a sealed
//! semantic query: the key is the env-bearing root slot plus `P`/`R`,
//! the value is the opaque [`MaterializationCycleGateOutcome`]. Only the
//! producer in this module constructs the outcome; consumers read the
//! carried [`MaterializationCycleGateVerdict`] from EITHER arm and never
//! branch on the arm kind.
//!
//! Admission contract: only [`MaterializationCycleGateOutcome::Decided`]
//! admits into the family memo. A `LegacyFallback` always suppresses
//! admission (`cache_suppress`); it marks `result_is_partial` iff any of
//! its reasons is partial (every reason except the hop-limit polarity
//! fallback). The family also carries the live-generation gate, so a
//! bare project-generation bump rejects a warm candidate.

use std::sync::Arc;

use super::walk::QueryBuildOutput;
use super::ProjectSemanticDispatch;
use crate::semantic_query::{
    CacheRead, MaterializationCycleGateFallbackReason, MaterializationCycleGateFallbackReasons,
    MaterializationCycleGateKey, MaterializationCycleGateOutcome, MaterializationCycleGateVerdict,
    QueryError, QueryResult, SemanticQueryKey, SemanticQueryValue,
};

// Counts producer cold builds (test-only) so admission / invalidation
// tests can discriminate a warm serve from a recompute.
#[cfg(test)]
thread_local! {
    static CYCLE_GATE_COMPUTE_COUNTER: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
}

thread_local! {
    /// Set by the producer when a cold build runs on the calling thread
    /// (the synchronous-compute contract). The read path resets it
    /// before dispatch and reads it after: still-`false` means the read
    /// was a warm serve, which the per-request audit payload
    /// (`type_resolution_ref_root_cycle_hits`) attributes.
    static CYCLE_GATE_PRODUCER_RAN: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
}

#[cfg(test)]
pub(crate) fn cycle_gate_compute_counter_for_test() -> usize {
    CYCLE_GATE_COMPUTE_COUNTER.with(|c| c.get())
}

#[cfg(test)]
pub(crate) fn reset_cycle_gate_compute_counter_for_test() {
    CYCLE_GATE_COMPUTE_COUNTER.with(|c| c.set(0));
}

impl ProjectSemanticDispatch<'_> {
    /// The sealed dispatch API for the materialization cycle gate.
    ///
    /// Returns the family's full [`CacheRead`]: the opaque outcome plus
    /// the family dep signature and the suppress/partial rails. Callers
    /// branch on `outcome.verdict()` (both arms), observe the read via
    /// `observe_component_meta_read_suppress`, and merge
    /// `read.dep_signature` into their local fence.
    pub(crate) fn classify_materialization_cycle_gate(
        &self,
        identity: &crate::semantic_query::DeclIdentity,
    ) -> CacheRead<MaterializationCycleGateOutcome> {
        let key = self.materialization_cycle_gate_key_for(identity);
        self.classify_materialization_cycle_gate_read(key)
    }

    /// The read half of the sealed API: executes `key` through the
    /// family singleflight and narrows the value domain onto
    /// [`MaterializationCycleGateOutcome`]. `pub(super)` so the family
    /// tests can drive constructed keys directly.
    pub(super) fn classify_materialization_cycle_gate_read(
        &self,
        key: SemanticQueryKey,
    ) -> CacheRead<MaterializationCycleGateOutcome> {
        CYCLE_GATE_PRODUCER_RAN.with(|c| c.set(false));
        let read = self.execute_via_cold_build_helper(key);
        if !CYCLE_GATE_PRODUCER_RAN.with(|c| c.get()) {
            // No cold build ran on this thread — the family warm-served
            // (or a cross-thread join landed). Attribute the hit on the
            // active request; cheap when no context is installed.
            if let Some(req_ctx) = crate::request_context::current_request_context() {
                req_ctx.bump_type_resolution_ref_root_cycle_hit();
            }
        }
        let read_partial_reasons = read.partial_reason_classes();
        let (value, defensive) = match read.value {
            QueryResult::Value(SemanticQueryValue::MaterializationCycleGate(outcome)) => {
                (outcome, false)
            }
            // The producer is the sole outcome constructor and always
            // returns one; a non-Value result is a memo-level failure
            // (cancellation / same-path recursion / a poisoned in-flight
            // join). Map it onto the fail-open fallback with the matching
            // reason — never a silent Decided.
            QueryResult::Value(_) | QueryResult::Recursive(_) | QueryResult::Error(_) => {
                let reason = match &read.value {
                    QueryResult::Error(QueryError::Cancelled) => {
                        MaterializationCycleGateFallbackReason::Cancelled
                    }
                    QueryResult::Error(QueryError::UnstableState { .. }) => {
                        MaterializationCycleGateFallbackReason::UnstableGeneration
                    }
                    _ => MaterializationCycleGateFallbackReason::NestedIncompleteObservation,
                };
                (
                    MaterializationCycleGateOutcome::LegacyFallback {
                        verdict: MaterializationCycleGateVerdict::Continue,
                        reasons: MaterializationCycleGateFallbackReasons::new([reason])
                            .expect("single reason is non-empty"),
                    },
                    true,
                )
            }
        };
        // A LegacyFallback never admits (the producer sets the flags;
        // the defensive conversion above forces them). A Decided outcome
        // carries the producer's rails verbatim.
        let is_fallback = !value.is_decided();
        // The defensive conversion is a partial with no class of its own —
        // it rides the anonymous bridge on top of whatever the read named.
        let partial_reasons = read_partial_reasons.union(if defensive {
            crate::semantic_query::PartialReasonSet::PROPAGATED
        } else {
            crate::semantic_query::PartialReasonSet::empty()
        });
        CacheRead {
            value,
            dep_signature: read.dep_signature,
            walker_diagnostics: read.walker_diagnostics,
            cache_suppress: read.cache_suppress || is_fallback || defensive,
            result_is_partial: read.result_is_partial || defensive,
            partial_reasons,
        }
    }

    /// Build the content-free key for one gate root identity: the
    /// env-bearing root slot (`T`/`L`/`J`) plus the defining canonical's
    /// `P` and `R` env dims.
    pub(crate) fn materialization_cycle_gate_key_for(
        &self,
        identity: &crate::semantic_query::DeclIdentity,
    ) -> SemanticQueryKey {
        let host = self.ctx.host_for_fact_tracer_install();
        let env = host.host_view_env_hashes_for(identity.canonical_id.as_ref());
        SemanticQueryKey::ClassifyMaterializationCycleGate(MaterializationCycleGateKey {
            root: self.type_slot_for(
                Arc::clone(&identity.canonical_id),
                identity.owner,
                Arc::clone(&identity.decl_name),
            ),
            parse_env_hash: env.parse_env_hash,
            resolve_env_hash: env.resolve_env_hash,
        })
    }

    /// The family cold-build arm (the
    /// `execute(ClassifyMaterializationCycleGate)` producer). SOLE
    /// constructor of [`MaterializationCycleGateOutcome`].
    ///
    /// An ordered, bounded reachability walk over declaration bodies —
    /// the SAME verdict contract the materializer guards have always
    /// consumed:
    ///
    /// - FIFO declaration queue, 64-dequeue bound.
    /// - `visited` keyed on `DeclIdentity`, inserted on enqueue;
    ///   first-visit-wins path signal (no later upgrade).
    /// - Per hop: empty-args `StructuralTransit`/`Skeleton` `Instantiate`.
    /// - Cycle recognition ONLY for `child == root || child == current`
    ///   (NOT full SCC reachability).
    /// - Hop-limit polarity is the carried path signal (complex → Stop,
    ///   plain → Continue).
    /// - `Opaque(RecursiveRef)` root-name back-edge detection.
    ///
    /// Recoverable incomplete observations (a nested `Recursive`/`Error`
    /// hop read, a partial hop read, a missing body, a scanner fuse, a
    /// missing graph node) do not stop the walk; they demote the final
    /// outcome to `LegacyFallback` — never `Decided`.
    pub(super) fn build_classify_materialization_cycle_gate(
        &self,
        key: &MaterializationCycleGateKey,
    ) -> QueryBuildOutput<SemanticQueryValue> {
        use crate::semantic_query::{
            DeclIdentity, DepVersion, ProjectionMode, ProjectionReductionContext, SemanticNodeId,
        };
        use rustc_hash::FxHashSet;
        use std::collections::VecDeque;

        #[cfg(test)]
        CYCLE_GATE_COMPUTE_COUNTER.with(|c| c.set(c.get() + 1));
        CYCLE_GATE_PRODUCER_RAN.with(|c| c.set(true));

        const MAX_HOPS: usize = 64;

        let mut reasons: Vec<MaterializationCycleGateFallbackReason> = Vec::new();
        let record_reason =
            |reasons: &mut Vec<MaterializationCycleGateFallbackReason>,
             reason: MaterializationCycleGateFallbackReason| {
                if !reasons.contains(&reason) {
                    reasons.push(reason);
                }
            };

        // Re-source the root's observed identity: the slot is
        // content-free (R6), so the `whole_hash` comes from the live
        // indexed view at compute time — the same snapshot the graph's
        // interned `DeclRef` identities were lowered from. An
        // unindexable root yields the default hash; its per-hop
        // `Instantiate` read errors below and demotes the outcome.
        let root = &key.root;
        let root_whole_hash = self
            .ctx
            .ensure_indexed_ready_serve(root.defining_canonical.as_ref())
            .map(|serve| serve.indexed.whole_hash)
            .unwrap_or_default();
        let root_identity = DeclIdentity {
            canonical_id: Arc::clone(&root.defining_canonical),
            owner: root.owner,
            whole_hash: root_whole_hash,
            decl_name: Arc::clone(&root.merged_symbol_name),
        };

        let mut dep_facts: Vec<(Arc<str>, DepVersion)> = self
            .project_generation_signature()
            .iter()
            .cloned()
            .collect();
        let mut observed_self_roots: Vec<crate::semantic_query_memo::ObservedGraphSelfRoot> =
            Vec::new();
        // Record one observed self-root per visited declaration identity
        // (the observed `whole_hash` embedded in the identity, NOT a
        // current-content re-read). The synthetic `__builtin__` carrier
        // (and any other empty canonical) has no file to root against.
        let record_self_root =
            |identity: &DeclIdentity,
             roots: &mut Vec<crate::semantic_query_memo::ObservedGraphSelfRoot>| {
                let canonical = identity.canonical_id.as_ref();
                if canonical.is_empty() || canonical == "__builtin__" {
                    return;
                }
                roots.push((Arc::clone(&identity.canonical_id), identity.whole_hash));
            };

        let graph = self.ctx.project_type_store().semantic_graph();
        let mut visited: FxHashSet<DeclIdentity> = FxHashSet::default();
        let mut queue: VecDeque<(DeclIdentity, bool)> = VecDeque::new();
        visited.insert(root_identity.clone());
        record_self_root(&root_identity, &mut observed_self_roots);
        queue.push_back((root_identity.clone(), false));

        let mut stop = false;
        let mut remaining_hops: usize = MAX_HOPS;
        while let Some((current, path_has_complex_signal)) = queue.pop_front() {
            if remaining_hops == 0 {
                // Hop-limit polarity: fall back to the carried flag
                // rather than blanket-false. Conservative on bounded
                // cyclic chains. NOT a partial result.
                record_reason(
                    &mut reasons,
                    MaterializationCycleGateFallbackReason::HopLimit,
                );
                stop = path_has_complex_signal;
                break;
            }
            remaining_hops -= 1;

            let current_identity = current.clone();

            let hop_key =
                SemanticQueryKey::Instantiate(crate::semantic_query::InstantiateKey::new(
                    self.type_slot_for(
                        Arc::clone(&current.canonical_id),
                        current.owner,
                        Arc::clone(&current.decl_name),
                    ),
                    Arc::from(Vec::<SemanticNodeId>::new().into_boxed_slice()),
                    // Skeleton mode preserves open generics so body lowering
                    // produces TypeParam graph nodes for T-refs (not
                    // Opaque(Miss)). The walk is a structural guard, not a
                    // publication boundary, so keep the Skeleton shape while
                    // using StructuralTransit demand to prevent nested mapped
                    // operators from emitting member publication edges.
                    self.instantiate_context_for(
                        &current.canonical_id,
                        ProjectionReductionContext::structural_transit_with_mode(
                            ProjectionMode::Skeleton,
                        ),
                    ),
                ));
            let read = self.execute_read(hop_key);
            crate::request_context::observe_component_meta_read_suppress(&read);
            crate::component_meta_audit::merge_dep_signature_into_local_fence(
                &mut dep_facts,
                &read.dep_signature,
            );
            if read.result_is_partial {
                record_reason(
                    &mut reasons,
                    MaterializationCycleGateFallbackReason::NestedIncompleteObservation,
                );
            }
            let body_id = match read.value {
                QueryResult::Value(id) => id,
                QueryResult::Recursive(_) => {
                    record_reason(
                        &mut reasons,
                        MaterializationCycleGateFallbackReason::NestedIncompleteObservation,
                    );
                    continue;
                }
                QueryResult::Error(error) => {
                    record_reason(
                        &mut reasons,
                        match &error {
                            QueryError::Cancelled => {
                                MaterializationCycleGateFallbackReason::Cancelled
                            }
                            QueryError::UnstableState { .. } => {
                                MaterializationCycleGateFallbackReason::UnstableGeneration
                            }
                            _ => {
                                MaterializationCycleGateFallbackReason::NestedIncompleteObservation
                            }
                        },
                    );
                    continue;
                }
            };

            let body_has_complex_signal = path_has_complex_signal
                || cycle_gate_has_complex_surface(graph, body_id, 0, &mut reasons);

            // Dispatch's recursive-ref back-edge is published as
            // `Opaque(RecursiveRef { name })` — not a DeclRef — so a
            // pure-graph walk would miss the self-cycle. Detect it
            // explicitly: any `Opaque(RecursiveRef { name })` whose name
            // matches the walk root's decl_name is a back-edge to root.
            if cycle_gate_body_contains_recursive_ref(
                graph,
                body_id,
                &root_identity.decl_name,
                0,
                &mut reasons,
            ) && body_has_complex_signal
            {
                stop = true;
                break;
            }

            let mut child_refs: Vec<(DeclIdentity, bool)> = Vec::new();
            cycle_gate_collect_ref_identities(graph, body_id, &mut child_refs, &mut reasons);

            for (child_identity, ref_has_type_args) in child_refs {
                let cycle_has_complex_signal = body_has_complex_signal || ref_has_type_args;
                // Cycle is reported when:
                //  (a) child == root (transitive cycle back to the walk
                //      root), OR
                //  (b) child == current (intermediate self-reference at
                //      this decl). NOT full SCC reachability.
                if cycle_has_complex_signal
                    && (child_identity == root_identity || child_identity == current_identity)
                {
                    stop = true;
                    break;
                }
                if visited.insert(child_identity.clone()) {
                    record_self_root(&child_identity, &mut observed_self_roots);
                    queue.push_back((child_identity, cycle_has_complex_signal));
                }
            }
            if stop {
                break;
            }
        }

        let verdict = if stop {
            MaterializationCycleGateVerdict::Stop
        } else {
            MaterializationCycleGateVerdict::Continue
        };
        let outcome = match MaterializationCycleGateFallbackReasons::new(reasons.iter().copied()) {
            None => MaterializationCycleGateOutcome::Decided(verdict),
            Some(fallback_reasons) => MaterializationCycleGateOutcome::LegacyFallback {
                verdict,
                reasons: fallback_reasons,
            },
        };

        let mut output: QueryBuildOutput<SemanticQueryValue> = (
            QueryResult::Value(SemanticQueryValue::MaterializationCycleGate(
                outcome.clone(),
            )),
            Arc::from(dep_facts.into_boxed_slice()),
        )
            .into();
        output.observed_self_roots = observed_self_roots;
        // Admission contract: a LegacyFallback never admits
        // (`cache_suppress`); it is partial iff any reason is partial
        // (every reason except HopLimit). The family memo refuses
        // publish on `cache_suppress || result_is_partial`, so only
        // `Decided` warm-serves.
        if let Some(fallback) = outcome.fallback_reasons() {
            output.cache_suppress = true;
            if fallback.any_partial() {
                output.result_is_partial = true;
            }
        }
        output
    }
}

/// Producer-private scanner: walker-parity check for "complex"
/// cycle-guard surfaces. A body whose top shape is something other than
/// a plain Object / Function / Array / Tuple / Primitive / Literal /
/// TypeParameter / Infer counts as "complex".
///
/// `depth` fuses recursion at 256; on the fuse the scanner returns
/// `false` (a runaway recursion is treated as "not complex" so the walk
/// continues) and records [`MaterializationCycleGateFallbackReason::ScannerLimit`].
/// A missing graph node returns `false` and records
/// [`MaterializationCycleGateFallbackReason::MissingGraphData`].
fn cycle_gate_has_complex_surface(
    graph: &crate::semantic_query_memo::SemanticGraphStore,
    node: crate::semantic_query::SemanticNodeId,
    depth: u32,
    reasons: &mut Vec<MaterializationCycleGateFallbackReason>,
) -> bool {
    use crate::semantic_query::SemanticNodeData;
    if depth > 256 {
        if !reasons.contains(&MaterializationCycleGateFallbackReason::ScannerLimit) {
            reasons.push(MaterializationCycleGateFallbackReason::ScannerLimit);
        }
        return false;
    }
    let Some(data) = graph.node_data(node) else {
        if !reasons.contains(&MaterializationCycleGateFallbackReason::MissingGraphData) {
            reasons.push(MaterializationCycleGateFallbackReason::MissingGraphData);
        }
        return false;
    };
    match data.as_ref() {
        SemanticNodeData::Alias(inner) => {
            cycle_gate_has_complex_surface(graph, *inner, depth + 1, reasons)
        }
        composite @ (SemanticNodeData::Union(_) | SemanticNodeData::Intersection(_)) => {
            let members = composite.composite_members().expect("composite arm");
            members
                .iter()
                .any(|&m| cycle_gate_has_complex_surface(graph, m, depth + 1, reasons))
                || members.iter().any(|&m| {
                    let d = graph.node_data(m);
                    !matches!(d.as_deref(), Some(SemanticNodeData::Object(_)))
                })
        }
        SemanticNodeData::DeclRef { .. }
        | SemanticNodeData::InstantiationRef { .. }
        | SemanticNodeData::IndexedAccess { .. }
        | SemanticNodeData::Conditional { .. }
        | SemanticNodeData::Mapped { .. }
        | SemanticNodeData::KeyOf { .. }
        | SemanticNodeData::TypeOf(_)
        | SemanticNodeData::TypeOfNominal(_)
        | SemanticNodeData::TemplateLiteral { .. } => true,
        _ => false,
    }
}

/// Producer-private scanner: returns `true` when `node`'s shallow
/// surface contains a `SemanticNodeData::Opaque(QueryError::RecursiveRef
/// { name })` matching `target_name`. The dispatch engine collapses
/// self-references into an `Opaque(RecursiveRef)` sentinel rather than a
/// regular DeclRef, so a pure-graph walk would miss the back-edge.
///
/// Depth-fused at 256 on entry (returns `false`, records
/// [`MaterializationCycleGateFallbackReason::ScannerLimit`]); a missing
/// graph node is skipped and records
/// [`MaterializationCycleGateFallbackReason::MissingGraphData`].
fn cycle_gate_body_contains_recursive_ref(
    graph: &crate::semantic_query_memo::SemanticGraphStore,
    node: crate::semantic_query::SemanticNodeId,
    target_name: &Arc<str>,
    depth: u32,
    reasons: &mut Vec<MaterializationCycleGateFallbackReason>,
) -> bool {
    use crate::semantic_query::{QueryError, SemanticNodeData, SemanticNodeId};
    use rustc_hash::FxHashSet;

    if depth > 256 {
        if !reasons.contains(&MaterializationCycleGateFallbackReason::ScannerLimit) {
            reasons.push(MaterializationCycleGateFallbackReason::ScannerLimit);
        }
        return false;
    }

    let mut stack: Vec<SemanticNodeId> = vec![node];
    let mut seen: FxHashSet<SemanticNodeId> = FxHashSet::default();
    while let Some(current) = stack.pop() {
        if !seen.insert(current) {
            continue;
        }
        let Some(data) = graph.node_data(current) else {
            if !reasons.contains(&MaterializationCycleGateFallbackReason::MissingGraphData) {
                reasons.push(MaterializationCycleGateFallbackReason::MissingGraphData);
            }
            continue;
        };
        match data.as_ref() {
            SemanticNodeData::Opaque(QueryError::RecursiveRef { name }) => {
                if name == target_name {
                    return true;
                }
            }
            SemanticNodeData::Alias(inner) => stack.push(*inner),
            composite @ (SemanticNodeData::Union(_) | SemanticNodeData::Intersection(_)) => {
                let members = composite.composite_members().expect("composite arm");
                for &m in members.iter() {
                    stack.push(m);
                }
            }
            SemanticNodeData::Object(surface) => {
                for member in surface.positive_members().iter() {
                    stack.push(member.value);
                }
                for sig in surface.index_signatures.iter() {
                    stack.push(sig.key_type);
                    stack.push(sig.value_type);
                }
                for &call in surface.call_signatures.iter() {
                    stack.push(call);
                }
                for &cons in surface.construct_signatures.iter() {
                    stack.push(cons);
                }
            }
            SemanticNodeData::Array { element, .. } => stack.push(*element),
            SemanticNodeData::Tuple { elements, .. } => {
                for element in elements.iter() {
                    stack.push(element.value);
                }
            }
            SemanticNodeData::IndexedAccess { object, index } => {
                stack.push(*object);
                if let crate::semantic_query::IndexKey::Computed(idx_node) = index {
                    stack.push(*idx_node);
                }
            }
            SemanticNodeData::KeyOf { base } => stack.push(*base),
            SemanticNodeData::Signature {
                params,
                return_type,
                ..
            } => {
                for param in params.iter() {
                    stack.push(param.ty);
                }
                stack.push(*return_type);
            }
            SemanticNodeData::Conditional {
                check,
                extends,
                true_branch_ref,
                false_branch_ref,
                ..
            } => {
                stack.push(*check);
                stack.push(*extends);
                stack.push(*true_branch_ref);
                stack.push(*false_branch_ref);
            }
            SemanticNodeData::Mapped { source, mapper } => {
                stack.push(*source);
                stack.push(mapper.key_space);
                stack.push(mapper.value_expr);
                if let Some(remap) = mapper.name_remap {
                    stack.push(remap);
                }
            }
            SemanticNodeData::TemplateLiteral { expressions, .. } => {
                for &expr in expressions.iter() {
                    stack.push(expr);
                }
            }
            SemanticNodeData::InstantiationRef { args, .. } => {
                for &arg in args.iter() {
                    stack.push(arg);
                }
            }
            // Carrier `type_args` are descended (args-only): a carrier's
            // applied arguments can carry an `Opaque(RecursiveRef)` back-edge.
            // The carrier head is not inspected (head resolution is separate).
            // The nominal terminal carries no args, so it descends nothing.
            SemanticNodeData::BareRef(_)
            | SemanticNodeData::TypeOf(_)
            | SemanticNodeData::TypeOfNominal(_)
            | SemanticNodeData::ImportType(_) => {
                for &arg in data.carrier_type_args().iter() {
                    stack.push(arg);
                }
            }
            _ => {}
        }
    }
    false
}

/// Producer-private scanner: collect every reachable `DeclRef` /
/// `InstantiationRef` identity from `node`'s declaration body, paired
/// with whether the reference carries type arguments. Walks THROUGH
/// every shape that could carry a ref — Conditional / Mapped /
/// TemplateLiteral / Object members + index signatures + call /
/// construct / method signatures / Function parameters + return / Tuple
/// elements / IndexedAccess(index + object) / KeyOf / Array / Alias.
/// Aggressive collection — never stops at "complex" body shapes (those
/// are the cycle indicator, not the termination signal).
///
/// A missing graph node is skipped and records
/// [`MaterializationCycleGateFallbackReason::MissingGraphData`].
fn cycle_gate_collect_ref_identities(
    graph: &crate::semantic_query_memo::SemanticGraphStore,
    node: crate::semantic_query::SemanticNodeId,
    out: &mut Vec<(crate::semantic_query::DeclIdentity, bool)>,
    reasons: &mut Vec<MaterializationCycleGateFallbackReason>,
) {
    use crate::semantic_query::{SemanticNodeData, SemanticNodeId};
    use rustc_hash::FxHashSet;

    let mut stack: Vec<SemanticNodeId> = vec![node];
    let mut seen: FxHashSet<SemanticNodeId> = FxHashSet::default();
    while let Some(current) = stack.pop() {
        if !seen.insert(current) {
            continue;
        }
        let Some(data) = graph.node_data(current) else {
            if !reasons.contains(&MaterializationCycleGateFallbackReason::MissingGraphData) {
                reasons.push(MaterializationCycleGateFallbackReason::MissingGraphData);
            }
            continue;
        };
        match data.as_ref() {
            SemanticNodeData::DeclRef { identity } => {
                // Bare DeclRef has no type arguments — false.
                out.push((identity.clone(), false));
            }
            SemanticNodeData::InstantiationRef { base, args } => {
                let ref_has_type_args = !args.is_empty();
                out.push((base.clone(), ref_has_type_args));
                for &arg in args.iter() {
                    stack.push(arg);
                }
            }
            SemanticNodeData::Alias(inner) => stack.push(*inner),
            composite @ (SemanticNodeData::Union(_) | SemanticNodeData::Intersection(_)) => {
                let members = composite.composite_members().expect("composite arm");
                for &m in members.iter() {
                    stack.push(m);
                }
            }
            SemanticNodeData::Object(surface) => {
                // Members hold property/method bodies.
                for member in surface.positive_members().iter() {
                    stack.push(member.value);
                }
                // Index signatures expose key + value types.
                for sig in surface.index_signatures.iter() {
                    stack.push(sig.key_type);
                    stack.push(sig.value_type);
                }
                // Call / construct signatures publish as Function nodes.
                for &call in surface.call_signatures.iter() {
                    stack.push(call);
                }
                for &cons in surface.construct_signatures.iter() {
                    stack.push(cons);
                }
            }
            SemanticNodeData::Array { element, .. } => stack.push(*element),
            SemanticNodeData::Tuple { elements, .. } => {
                for element in elements.iter() {
                    stack.push(element.value);
                }
            }
            SemanticNodeData::IndexedAccess { object, index } => {
                stack.push(*object);
                if let crate::semantic_query::IndexKey::Computed(idx_node) = index {
                    stack.push(*idx_node);
                }
            }
            SemanticNodeData::KeyOf { base } => stack.push(*base),
            SemanticNodeData::Signature {
                params,
                return_type,
                type_parameters,
                ..
            } => {
                for param in params.iter() {
                    stack.push(param.ty);
                }
                stack.push(*return_type);
                for tp in type_parameters.iter() {
                    if let Some(c) = tp.constraint {
                        stack.push(c);
                    }
                    if let Some(d) = tp.default {
                        stack.push(d);
                    }
                }
            }
            SemanticNodeData::Conditional {
                check,
                extends,
                true_branch_ref,
                false_branch_ref,
                ..
            } => {
                stack.push(*check);
                stack.push(*extends);
                stack.push(*true_branch_ref);
                stack.push(*false_branch_ref);
            }
            SemanticNodeData::Mapped { source, mapper } => {
                stack.push(*source);
                stack.push(mapper.key_space);
                stack.push(mapper.value_expr);
                if let Some(remap) = mapper.name_remap {
                    stack.push(remap);
                }
            }
            SemanticNodeData::TemplateLiteral { expressions, .. } => {
                for &expr in expressions.iter() {
                    stack.push(expr);
                }
            }
            // Carrier `type_args` are descended (args-only): a `BareRef` /
            // `TypeOf` / `ImportType` carrier applies its arguments at the
            // reference site, and an arg can carry a `DeclRef` /
            // `InstantiationRef` (a declaration edge). The carrier HEAD is NOT
            // collected here — a `BareRef` / `ImportType` head is unresolved
            // (no decl identity) and a `TypeOf` head is a value root (no decl
            // identity); head resolution is a separate concern.
            SemanticNodeData::BareRef(_)
            | SemanticNodeData::TypeOf(_)
            | SemanticNodeData::TypeOfNominal(_)
            | SemanticNodeData::ImportType(_) => {
                for &arg in data.carrier_type_args().iter() {
                    stack.push(arg);
                }
            }
            _ => {}
        }
    }
}

/// Test-only shim over the producer-private ref collector (carrier-arg
/// descent + missing-data reporting), so scanner-discrimination tests
/// outside this module tree exercise the exact production walk.
#[cfg(test)]
#[doc(hidden)]
pub(crate) fn cycle_gate_collect_ref_identities_for_test(
    graph: &crate::semantic_query_memo::SemanticGraphStore,
    node: crate::semantic_query::SemanticNodeId,
    out: &mut Vec<(crate::semantic_query::DeclIdentity, bool)>,
    reasons: &mut Vec<MaterializationCycleGateFallbackReason>,
) {
    cycle_gate_collect_ref_identities(graph, node, out, reasons);
}

/// Test-only shim over the producer-private recursive-ref detector.
#[cfg(test)]
#[doc(hidden)]
pub(crate) fn cycle_gate_body_contains_recursive_ref_for_test(
    graph: &crate::semantic_query_memo::SemanticGraphStore,
    node: crate::semantic_query::SemanticNodeId,
    target_name: &Arc<str>,
    depth: u32,
    reasons: &mut Vec<MaterializationCycleGateFallbackReason>,
) -> bool {
    cycle_gate_body_contains_recursive_ref(graph, node, target_name, depth, reasons)
}
