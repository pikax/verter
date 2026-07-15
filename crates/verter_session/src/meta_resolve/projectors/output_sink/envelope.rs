//! The output-ENVELOPE assembly half of the terminal
//! [`output_sink`](super) module: the request-local `(effective scope,
//! source identity)` materialization memo, the 11-lane positional
//! materializer, the session-owned resolved type-registry name-overlay
//! finalize, and the [`build_component_meta_output`] envelope builder.
//!
//! A CHILD of the sink module — the capability mint scope
//! (`pub(in crate::meta_resolve::projectors::output_sink)`) covers this
//! descendant, so envelope assembly can mint
//! [`MetaResolveProjectorsOutputCap`] while every non-sink projector module
//! still cannot (`E0624`). Split out of `output_sink.rs` for the production
//! file-size gate; a SANCTIONED co-sink of the cap's mint scope (registered
//! in the mint-scope guard's `SANCTIONED_SINK_MODULES`).

use verter_type_expr::TypeExpr;

use super::{
    materialize_output_source, missing_source_output_type_expr, MetaResolveProjectorsOutputCap,
};
use crate::project_semantic_dispatch::ProjectSemanticDispatch;
use crate::resolver_core::ResolverContext;

/// Request-local output-materialization memo: `(effective scope, source
/// identity)` → materialized value, shared across ALL lanes of one output
/// payload so a repeated source raises once per effective scope. The
/// `materialize_calls` counter observes how many sources actually reached
/// [`materialize_output_source`] (the dedupe-discriminating count).
///
/// The key is FULLY BORROWED from the request-local analysis (`&str`
/// scope + `&SemanticTypeSource`), and lookups go through the single-hash
/// `entry` API: each populated lane slot performs EXACTLY ONE full-source
/// hash traversal and ZERO source clones (a cloned owned key per lookup
/// was O(lanes × source-size) even on hits). The per-slot VALUE clone on
/// a hit is inherent — the positional lanes own their `TypeExpr`s.
struct OutputSourceMemo<'a> {
    memo: std::collections::HashMap<
        (&'a str, &'a verter_type_expr::facts::SemanticTypeSource),
        TypeExpr,
        MemoBuildHasher,
    >,
    materialize_calls: u64,
}

/// The memo map's hasher: `FxHasher` with a test-only hash-op counter, so
/// the request-local-dedupe canary can pin "one full-source hash traversal
/// per populated lane slot" (the hash-work half of the memo contract; the
/// zero-clone half is structural — the borrowed key type has no owned
/// source to clone).
#[derive(Default, Clone)]
struct MemoBuildHasher(rustc_hash::FxBuildHasher);

impl std::hash::BuildHasher for MemoBuildHasher {
    type Hasher = rustc_hash::FxHasher;
    fn build_hasher(&self) -> Self::Hasher {
        #[cfg(test)]
        OUTPUT_MEMO_HASH_OPS_LIVE.with(|ops| ops.set(ops.get() + 1));
        std::hash::BuildHasher::build_hasher(&self.0)
    }
}

#[cfg(test)]
thread_local! {
    /// Live hash-op counter for the CURRENT memo (reset per
    /// [`build_component_meta_output`] envelope build on this thread).
    static OUTPUT_MEMO_HASH_OPS_LIVE: std::cell::Cell<u64> = const { std::cell::Cell::new(0) };
}

#[cfg(test)]
thread_local! {
    /// Test-only observation slot: the number of memo hash operations the
    /// most recent [`build_component_meta_output`] on this thread
    /// performed — one per populated lane slot on the single-hash `entry`
    /// route (a get-then-insert route pays a SECOND full-source hash per
    /// miss and fails the canary).
    pub(crate) static LAST_OUTPUT_MEMO_HASH_OPS: std::cell::Cell<u64> =
        const { std::cell::Cell::new(0) };
}

#[cfg(test)]
thread_local! {
    /// Test-only observation slot: the number of DE-DUPED
    /// [`materialize_output_source`] calls the most recent
    /// [`build_component_meta_output`] on this thread performed. Lets the
    /// request-local-dedupe test assert a repeated source raised once per
    /// effective scope without instrumenting production counters.
    pub(crate) static LAST_OUTPUT_MATERIALIZE_CALLS: std::cell::Cell<u64> =
        const { std::cell::Cell::new(0) };
}

#[cfg(test)]
thread_local! {
    /// Test-only knob: when `true`, the next [`build_component_meta_output`]
    /// call fails with a typed [`ComponentMetaOutputError`] (consuming the
    /// flag), exercising the fail-closed output path deterministically at
    /// the ENTRY level — where a genuine unraisable source cannot be
    /// injected because the entry builds the analysis internally. Lets the
    /// cache-rail tests assert an output failure suppresses ONLY the
    /// output/encoded-payload admission, never the independently-complete
    /// analysis cache entry.
    ///
    /// [`ComponentMetaOutputError`]: crate::meta_resolve::ComponentMetaOutputError
    pub(crate) static OUTPUT_MATERIALIZE_FORCE_FAIL: std::cell::Cell<bool> = const {
        std::cell::Cell::new(false)
    };
}

/// Test-only CANONICAL-KEYED force-fail knob: when armed with a canonical,
/// the next [`build_component_meta_output`] for EXACTLY that canonical fails
/// with a typed [`ComponentMetaOutputError`] (consuming that canonical's
/// arm). Unlike the thread-local flag above, this is process-global (a
/// `Mutex`) so it reaches output builds running on batch-coordinator POOL
/// WORKER threads and the out-of-crate LSP integration harness; a
/// canonical-keyed SET (not a single slot) so concurrently-running tests
/// arming DIFFERENT canonicals never overwrite each other's arm. Gated to
/// test builds + the `test-support` feature (the LSP dev harness).
///
/// [`ComponentMetaOutputError`]: crate::meta_resolve::ComponentMetaOutputError
#[cfg(any(test, feature = "test-support"))]
pub(crate) static OUTPUT_MATERIALIZE_FORCE_FAIL_FOR: std::sync::Mutex<
    std::collections::BTreeSet<String>,
> = std::sync::Mutex::new(std::collections::BTreeSet::new());

impl<'a> OutputSourceMemo<'a> {
    fn new() -> Self {
        #[cfg(test)]
        OUTPUT_MEMO_HASH_OPS_LIVE.with(|ops| ops.set(0));
        Self {
            memo: std::collections::HashMap::with_hasher(MemoBuildHasher::default()),
            materialize_calls: 0,
        }
    }

    /// Materialize ONE lane slot — the EXHAUSTIVE three-state decision:
    ///
    /// - `Absent(_)` → the centralized schema-absence policy (the typed
    ///   `Unknown` render; honest absence, a valid success);
    /// - `Present(source)` → memoized [`materialize_output_source`]; a
    ///   materialization failure carries the lane + positional indices +
    ///   the failed position (fail-closed — never a silent `Unknown`);
    /// - `Failed(failure)` → an IMMEDIATE typed output error: a REQUIRED
    ///   position whose faithful source could not be constructed FAILS the
    ///   output; it must never render as an `unknown` success.
    #[allow(clippy::too_many_arguments)]
    fn materialize_output_lane_slot(
        &mut self,
        dispatch: &ProjectSemanticDispatch<'_>,
        cap: &MetaResolveProjectorsOutputCap<'_, '_>,
        effective_scope: &'a str,
        lane: crate::meta_resolve::ComponentMetaOutputLane,
        index: usize,
        inner_index: Option<usize>,
        position: &'a verter_type_expr::facts::SourcePosition,
    ) -> Result<TypeExpr, crate::meta_resolve::ComponentMetaOutputError> {
        let source = match position {
            verter_type_expr::facts::SourcePosition::Absent(_) => {
                return Ok(missing_source_output_type_expr());
            }
            verter_type_expr::facts::SourcePosition::Failed(failure) => {
                return Err(crate::meta_resolve::ComponentMetaOutputError {
                    lane,
                    index,
                    inner_index,
                    position: Box::new(position.clone()),
                    failure:
                        crate::meta_resolve::ComponentMetaOutputFailure::RequiredSourceUnavailable {
                            failure: *failure,
                        },
                });
            }
            verter_type_expr::facts::SourcePosition::Present(source) => source,
        };
        // Single-hash borrowed-key route: `entry` hashes the (scope,
        // source) key ONCE per slot; hit and miss both reuse that one
        // traversal, and no owned key is ever built.
        match self.memo.entry((effective_scope, source)) {
            std::collections::hash_map::Entry::Occupied(hit) => Ok(hit.get().clone()),
            std::collections::hash_map::Entry::Vacant(slot) => {
                self.materialize_calls += 1;
                let value = materialize_output_source(dispatch, cap, effective_scope, source)
                    .map_err(|failure| crate::meta_resolve::ComponentMetaOutputError {
                        lane,
                        index,
                        inner_index,
                        position: Box::new(verter_type_expr::facts::SourcePosition::Present(
                            source.clone(),
                        )),
                        failure,
                    })?;
                slot.insert(value.clone());
                Ok(value)
            }
        }
    }
}

/// Materialize ALL 11 component-meta output type lanes against the caller's
/// live view: props, event payloads, slot bindings, models, exposed members,
/// public-instance members, merged type-registry entries, accepted props,
/// accepted event payloads, fallthrough props, fallthrough event payloads.
///
/// ONE dispatch (the caller's) serves the whole payload, with ONE
/// request-local dedupe keyed by `(effective scope, source identity)` shared
/// across every lane, so a repeated source raises once per effective scope.
/// Every lane stays order-aligned 1:1 with its analysis vector — duplicate
/// names are preserved positionally (the dedupe is on the SOURCE, never the
/// name) — and nested lanes (per-slot bindings, per-branch fallthrough rows)
/// mirror the analysis' nested topology positionally.
///
/// The effective scope is PER ROW: the owner canonical for the owner's own
/// lanes, and the row's positional PRODUCING scope
/// (`type_source_scope` / `payload_scope`, threaded through the fallthrough
/// clone boundary and the cross-branch merge in
/// `resolver_core::fallthrough`) for the inherited accepted / fallthrough
/// lanes. Anchor-BEARING positions were additionally normalized to
/// self-anchoring sources at the clone boundary; the positional scope
/// covers the anchor-FREE scope-relative positions (nested bare `Ref`
/// leaves) `absolutized_against` cannot pin — the parent owner is never
/// used blindly as a cross-owner raise scope.
///
/// Per lane slot (the exhaustive three-state decision — see
/// [`OutputSourceMemo::materialize_output_lane_slot`]):
/// - `Absent(_)` → the centralized schema-absence policy
///   ([`missing_source_output_type_expr`]).
/// - `Present(source)` → [`materialize_output_source`]; a failure FAILS the
///   whole output with a typed [`ComponentMetaOutputError`] carrying the
///   lane, the positional indices, and the failed position (fail-closed —
///   never a silent `Unknown`).
/// - `Failed(_)` → an immediate typed [`ComponentMetaOutputError`]: the
///   REQUIRED position's producer-typed failure fails the output.
///
/// [`ComponentMetaOutputError`]: crate::meta_resolve::ComponentMetaOutputError
fn materialize_component_meta_output_types<'a>(
    dispatch: &ProjectSemanticDispatch<'_>,
    cap: &MetaResolveProjectorsOutputCap<'_, '_>,
    scope_canonical_id: &'a str,
    analysis: &'a verter_semantic::analysis::component_meta::ComponentMetaAnalysis,
) -> Result<
    crate::meta_resolve::MaterializedComponentMetaTypes,
    crate::meta_resolve::ComponentMetaOutputError,
> {
    use crate::meta_resolve::ComponentMetaOutputLane as Lane;
    use verter_semantic::analysis::component_meta::FallthroughSurface;

    let mut memo = OutputSourceMemo::new();
    let scope: &'a str = scope_canonical_id;
    let mut lanes = crate::meta_resolve::output::MaterializedComponentMetaTypeLanes::default();

    for (index, prop) in analysis.props.iter().enumerate() {
        lanes.props.push(memo.materialize_output_lane_slot(
            dispatch,
            cap,
            scope,
            Lane::Prop,
            index,
            None,
            &prop.type_source,
        )?);
    }
    for (index, event) in analysis.events.iter().enumerate() {
        lanes.event_payloads.push(memo.materialize_output_lane_slot(
            dispatch,
            cap,
            scope,
            Lane::EventPayload,
            index,
            None,
            &event.payload,
        )?);
    }
    for (index, slot) in analysis.slots.iter().enumerate() {
        let mut bindings = Vec::with_capacity(slot.bindings.len());
        for (inner, binding) in slot.bindings.iter().enumerate() {
            bindings.push(memo.materialize_output_lane_slot(
                dispatch,
                cap,
                scope,
                Lane::SlotBinding,
                index,
                Some(inner),
                &binding.type_source,
            )?);
        }
        lanes.slot_bindings.push(bindings);
    }
    for (index, model) in analysis.models.iter().enumerate() {
        lanes.models.push(memo.materialize_output_lane_slot(
            dispatch,
            cap,
            scope,
            Lane::Model,
            index,
            None,
            &model.type_source,
        )?);
    }
    for (index, exposed) in analysis.exposed.iter().enumerate() {
        lanes.exposed.push(memo.materialize_output_lane_slot(
            dispatch,
            cap,
            scope,
            Lane::Exposed,
            index,
            None,
            &exposed.type_source,
        )?);
    }
    if let Some(public_instance) = analysis.public_instance.as_ref() {
        for (index, member) in public_instance.members.iter().enumerate() {
            lanes
                .public_instance_members
                .push(memo.materialize_output_lane_slot(
                    dispatch,
                    cap,
                    scope,
                    Lane::PublicInstanceMember,
                    index,
                    None,
                    &member.type_source,
                )?);
        }
    }
    for (index, entry) in analysis.type_registry.iter().enumerate() {
        lanes
            .type_registry_entries
            .push(memo.materialize_output_lane_slot(
                dispatch,
                cap,
                scope,
                Lane::TypeRegistryEntry,
                index,
                None,
                &entry.type_source,
            )?);
    }
    for (index, prop) in analysis.accepted_props.iter().enumerate() {
        let effective = effective_output_scope(scope, prop.type_source_scope.as_deref());
        lanes.accepted_props.push(memo.materialize_output_lane_slot(
            dispatch,
            cap,
            effective,
            Lane::AcceptedProp,
            index,
            None,
            &prop.type_source,
        )?);
    }
    for (index, event) in analysis.accepted_events.iter().enumerate() {
        let effective = effective_output_scope(scope, event.payload_scope.as_deref());
        lanes
            .accepted_event_payloads
            .push(memo.materialize_output_lane_slot(
                dispatch,
                cap,
                effective,
                Lane::AcceptedEventPayload,
                index,
                None,
                &event.payload,
            )?);
    }
    if let FallthroughSurface::Branches { branches } = &analysis.fallthrough_surface {
        for (index, branch) in branches.iter().enumerate() {
            let mut props = Vec::with_capacity(branch.props.len());
            for (inner, prop) in branch.props.iter().enumerate() {
                let effective = effective_output_scope(scope, prop.type_source_scope.as_deref());
                props.push(memo.materialize_output_lane_slot(
                    dispatch,
                    cap,
                    effective,
                    Lane::FallthroughProp,
                    index,
                    Some(inner),
                    &prop.type_source,
                )?);
            }
            lanes.fallthrough_props.push(props);
            let mut events = Vec::with_capacity(branch.events.len());
            for (inner, event) in branch.events.iter().enumerate() {
                let effective = effective_output_scope(scope, event.payload_scope.as_deref());
                events.push(memo.materialize_output_lane_slot(
                    dispatch,
                    cap,
                    effective,
                    Lane::FallthroughEventPayload,
                    index,
                    Some(inner),
                    &event.payload,
                )?);
            }
            lanes.fallthrough_event_payloads.push(events);
        }
    }

    #[cfg(test)]
    LAST_OUTPUT_MATERIALIZE_CALLS.with(|calls| calls.set(memo.materialize_calls));
    #[cfg(test)]
    LAST_OUTPUT_MEMO_HASH_OPS
        .with(|ops| ops.set(OUTPUT_MEMO_HASH_OPS_LIVE.with(std::cell::Cell::get)));
    let _ = memo.materialize_calls;

    Ok(crate::meta_resolve::MaterializedComponentMetaTypes::from_lanes(cap, lanes))
}

/// The PER-ROW effective raise scope for an output lane slot: the row's
/// positional PRODUCING scope when it carries one (an inherited source's
/// scope-relative names — nested bare `Ref` leaves included — resolve in
/// the producing file, per the cross-owner effective-scope invariant),
/// else the analysis OWNER canonical. The request-local memo keys on this
/// effective scope, so the same source inherited from two different
/// producers correctly materializes per producer.
fn effective_output_scope<'a>(owner: &'a str, row_scope: Option<&'a str>) -> &'a str {
    row_scope.unwrap_or(owner)
}

/// The session-owned resolved type-registry name-overlay finalize: fold the
/// resolution's `resolved_type_registry` entries into the analysis'
/// `type_registry` — replace the FIRST same-name entry in place, append a
/// new name at the end (order-preserving). This is a semantic publication
/// decision and runs HERE, in the session owner, BEFORE materialization —
/// never in the wire converter.
fn finalize_resolved_type_registry_overlay(
    registry: &mut Vec<verter_semantic::analysis::component_meta::ResolvedTypeAnalysis>,
    resolved_entries: Vec<verter_semantic::analysis::component_meta::ResolvedTypeAnalysis>,
) {
    for resolved_entry in resolved_entries {
        if let Some(existing) = registry
            .iter_mut()
            .find(|entry| entry.name == resolved_entry.name)
        {
            *existing = resolved_entry;
        } else {
            registry.push(resolved_entry);
        }
    }
}

/// Build the session-owned [`ComponentMetaOutput`] envelope for a final
/// component-meta analysis: apply the session-owned resolved type-registry
/// name-overlay finalize (when a resolution seed is supplied), materialize
/// ALL 11 output type lanes at this terminal sink, and seal them — together
/// with the analysis and the narrowed resolution sidecar — into the
/// request-local envelope the wire converter consumes by value.
///
/// MUST be driven inside the caller's request-bound validated view — after
/// the analysis is extracted, before the request view/dispatch lifetime ends
/// — so the output is materialized against the SAME view the analysis was
/// served under. The envelope is request-local output IR: it never enters
/// `ResolvedComponentMetaState`, `ComponentMetaAnalysis`, or any warm
/// semantic cache.
///
/// [`ComponentMetaOutput`]: crate::meta_resolve::ComponentMetaOutput
pub(crate) fn build_component_meta_output(
    ctx: &dyn ResolverContext,
    scope_canonical_id: &str,
    mut analysis: verter_semantic::analysis::component_meta::ComponentMetaAnalysis,
    resolution: Option<crate::meta_resolve::output::ComponentMetaResolutionSeed>,
) -> Result<crate::meta_resolve::ComponentMetaOutput, crate::meta_resolve::ComponentMetaOutputError>
{
    #[cfg(test)]
    if OUTPUT_MATERIALIZE_FORCE_FAIL.with(|flag| {
        if flag.get() {
            flag.set(false);
            true
        } else {
            false
        }
    }) {
        return Err(forced_output_failure_error());
    }
    #[cfg(any(test, feature = "test-support"))]
    {
        let mut armed = OUTPUT_MATERIALIZE_FORCE_FAIL_FOR.lock().unwrap();
        if armed.remove(scope_canonical_id) {
            return Err(forced_output_failure_error());
        }
    }

    // Session-owned registry overlay finalize BEFORE materialization: the
    // materialized registry lane aligns with the MERGED registry the wire
    // publishes.
    let resolution_output = resolution.map(|seed| {
        finalize_resolved_type_registry_overlay(
            &mut analysis.type_registry,
            seed.resolved_type_registry,
        );
        seed.output
    });

    // ONE dispatch for the whole output payload; the capability mint and
    // every sealed-carrier unwrap live here, in the terminal sink.
    let dispatch = ProjectSemanticDispatch::new(ctx);
    let cap = MetaResolveProjectorsOutputCap::new(&dispatch);
    let types =
        materialize_component_meta_output_types(&dispatch, &cap, scope_canonical_id, &analysis)?;
    Ok(crate::meta_resolve::ComponentMetaOutput::from_parts(
        &cap,
        analysis,
        resolution_output,
        types,
    ))
}

/// The typed error both force-fail knobs return.
#[cfg(any(test, feature = "test-support"))]
fn forced_output_failure_error() -> crate::meta_resolve::ComponentMetaOutputError {
    crate::meta_resolve::ComponentMetaOutputError {
        lane: crate::meta_resolve::ComponentMetaOutputLane::Prop,
        index: 0,
        inner_index: None,
        position: Box::new(verter_type_expr::facts::SourcePosition::Present(
            verter_type_expr::facts::SemanticTypeSource::Closed(
                verter_type_expr::facts::ClosedTypeFact::Leaf(
                    verter_type_expr::facts::LeafTypeFact::Primitive(
                        verter_type_expr::PrimitiveName::Unknown,
                    ),
                ),
            ),
        )),
        failure: crate::meta_resolve::ComponentMetaOutputFailure::UnraisableSource,
    }
}
