//! The stages of the family builds written as owned continuations
//! (`family_continuation`). Each stage is a method of
//! [`ContinuationStage`], the stage capability: it computes from what the
//! build's earlier stages retained and the answer its last read delivered,
//! and constructs semantic nodes, but holds no dispatcher, so it cannot read
//! another query, read or load a file, join another task's producer or drive
//! an execution. Every dependency a stage has is a [`FamilyStep`] its driver
//! answers.

use std::sync::Arc;

use verter_type_expr::OwnedStr;

use super::build::fence_to_dep_signature;
use super::family_continuation::{ContinuationStage, FamilyBuild, FamilyStep};
use crate::semantic_query::{
    PathSegment, ProjectionMode, QueryError, QueryResult, SemanticNodeId, SemanticQueryKey,
};

/// A macro payload's resolution between its stages: waiting for its owning
/// file's indexed input, or for its payload operation's read with what the
/// owner stage read and the fence it seeded, so that read finishes the
/// resolution without reading the owner again.
pub(super) enum MacroPayloadBuild {
    /// Waiting for the owning file's indexed input.
    Owner(MacroPayloadRequest),
    /// Waiting for the payload operation's read.
    Payload {
        rooting: MacroPayloadRooting,
        local_fence: Vec<crate::semantic_query::DepEntry>,
    },
}

/// What a macro payload's resolution is asked for: its key's operands.
pub(super) struct MacroPayloadRequest {
    owner: crate::semantic_query::ResolvedDeclSlotIdentity,
    macro_index: usize,
    macro_kind: verter_session_query::analysis::types::AnalyzedMacroKind,
    type_args: Arc<[SemanticNodeId]>,
    mode: ProjectionMode,
}

/// An ordered intersection reduction while it reads a nested subgroup's
/// reduction: its input and the operands its steps before that subgroup
/// gave, so the read resumes it at the step after the subgroup.
pub(super) struct ReduceIntersectionBuild {
    input: crate::semantic_query::IntersectionInputRef,
    purpose: crate::semantic_query::IntersectionPurpose,
    context: crate::semantic_query::SemanticContextId,
    /// The next step to take.
    next: usize,
    members: Vec<SemanticNodeId>,
}

/// What a macro payload's output is rooted on: the owning file at the
/// content version its owner stage observed, and its type arguments.
pub(super) struct MacroPayloadRooting {
    owner: OwnedStr,
    is_non_file_owner: bool,
    owner_whole_hash: crate::semantic_query::HashValue,
    stale_real_file_owner: bool,
    type_args: Arc<[SemanticNodeId]>,
}

impl<'s, 'a: 's, C: crate::resolver_core::ResolverCapabilities> ContinuationStage<'s, 'a, C> {
    /// The first stage of a macro payload's resolution: a real-file owner's
    /// indexed input is read next ([`Self::resume_macro_payload_owner`]); a
    /// non-file owner has none, and the resolution goes on at once.
    #[inline(never)]
    pub(super) fn begin_resolve_macro_payload(
        &self,
        owner: &crate::semantic_query::ResolvedDeclSlotIdentity,
        macro_index: usize,
        macro_kind: verter_session_query::analysis::types::AnalyzedMacroKind,
        type_args: &Arc<[SemanticNodeId]>,
        mode: ProjectionMode,
    ) -> FamilyStep {
        self.graph().record_continuation_begun();
        let request = MacroPayloadRequest {
            owner: owner.clone(),
            macro_index,
            macro_kind,
            type_args: Arc::clone(type_args),
            mode,
        };
        if crate::semantic_query::is_non_file_base(owner.defining_canonical.as_ref()) {
            return self.resolve_macro_payload_with_owner(request, None);
        }
        FamilyStep::Load(
            Arc::from(owner.defining_canonical.as_ref()),
            Box::new(FamilyBuild::MacroPayload(MacroPayloadBuild::Owner(request))),
        )
    }

    /// Resume a macro payload's resolution with its owning file's indexed
    /// input (`None` when the live view serves none).
    #[inline(never)]
    pub(super) fn resume_macro_payload_owner(
        &self,
        build: MacroPayloadBuild,
        serve: Option<verter_session_query::inputs::indexed::IndexedInputServe>,
    ) -> FamilyStep {
        let MacroPayloadBuild::Owner(request) = build else {
            unreachable!("a macro payload reads its owner once, first")
        };
        self.resolve_macro_payload_with_owner(request, serve.map(|serve| serve.indexed))
    }

    /// A macro payload's resolution once its owner is read: the fence the
    /// owner seeds, then either the payload's value or the one payload
    /// operation the value is read from — the props intersection, or the
    /// emits / slots / model projection — retained with everything read so
    /// far for [`Self::finish_resolve_macro_payload`].
    #[inline(never)]
    fn resolve_macro_payload_with_owner(
        &self,
        request: MacroPayloadRequest,
        owner_indexed: Option<Arc<verter_session_query::inputs::indexed::IndexedInputRecord>>,
    ) -> FamilyStep {
        use verter_session_query::analysis::types::AnalyzedMacroKind;
        let MacroPayloadRequest {
            owner,
            macro_index,
            macro_kind,
            type_args,
            mode,
        } = request;
        let owner = &owner;
        let type_args = &type_args;

        // R6 / R20: the key is content-free. Re-source the owning
        // SFC's content version from the live indexed view at
        // value-build time so the cached `MemoEntry` self-roots on
        // the current generation's whole_hash. Three classes of
        // non-file owner exist and must NOT invent a synthetic
        // `FileWholeHash` (mirrors the `build_instantiate` non-file
        // base rule):
        //  - the global / structural sentinel (`canonical_id == ""`);
        //  - built-in utility carriers (`canonical_id == "__builtin__"`);
        //  - synthetic test identities (`canonical_id == "<synthetic>"`).
        // These owners root self-version through their `type_args`
        // nodes only (no file fact in the fence or the self-root set).
        // A real-file owner whose `ensure_indexed_ready_serve` returns `None`
        // (the file is unknown to the live view) is a STALE KEY: the
        // build still hands the value to the caller but marks the
        // output non-cacheable (`cache_suppress`) rather than publishing
        // a result self-rooted on the sentinel hash `0`, which could
        // later serve stale.
        let owner_canonical_str = owner.defining_canonical.as_ref();
        let is_non_file_owner = crate::semantic_query::is_non_file_base(owner_canonical_str);
        // A real-file owner unknown to the live view is a stale key —
        // suppress admission so the next caller cold-recomputes.
        let stale_real_file_owner = !is_non_file_owner && owner_indexed.is_none();
        let owner_whole_hash: crate::semantic_query::HashValue = owner_indexed
            .as_ref()
            .map(|indexed| indexed.whole_hash)
            .unwrap_or_default();

        // Seed the local fence with the macro's owning canonical and
        // the re-sourced content version — but ONLY for a real-file
        // owner. A non-file owner has no `FileWholeHash` to root on;
        // recording `(non_file_canonical, 0)` would fabricate a content
        // version for a builtin/structural carrier. The non-file owner
        // roots entirely through its `type_args` (the same rule
        // `build_instantiate` applies to a non-file base).
        let mut local_fence: Vec<crate::semantic_query::DepEntry> = Vec::new();
        let owner_canonical =
            verter_session_query::retention::owned::OwnedStr::new(&owner.defining_canonical);
        if !is_non_file_owner {
            local_fence.push(crate::semantic_query::DepEntry(
                owner_canonical.clone(),
                crate::semantic_query::DepVersion::WholeHash(owner_whole_hash),
            ));
        }
        // Always pin the project-generation so the fence catches
        // workspace-wide changes that could invalidate the lowering
        // basis (mirrors `dep_signature_for` semantics).
        let project_gen = self.project_generation();
        local_fence.push(crate::semantic_query::DepEntry(
            owner_canonical,
            crate::semantic_query::DepVersion::ProjectGeneration(project_gen),
        ));

        let rooting = MacroPayloadRooting {
            owner: owner.defining_canonical.clone(),
            is_non_file_owner,
            owner_whole_hash,
            stale_real_file_owner,
            type_args: Arc::clone(type_args),
        };
        let answered = |rooting, result, local_fence| {
            FamilyStep::Done(Box::new(self.macro_payload_output(
                rooting,
                local_fence,
                result,
                false,
                false,
            )))
        };
        let payload = match macro_kind {
            AnalyzedMacroKind::DefineProps | AnalyzedMacroKind::WithDefaults => {
                // §3.2 sketch: 0 args → Miss; 1 arg → arg directly;
                // ≥2 args → intersection-normalised result.
                if type_args.is_empty() {
                    let miss = QueryResult::Value(self.opaque(QueryError::Miss));
                    return answered(rooting, miss, local_fence);
                }
                if type_args.len() == 1 {
                    return answered(rooting, QueryResult::Value(type_args[0]), local_fence);
                }
                SemanticQueryKey::reduce_intersection_operands(
                    self.graph().store(),
                    Arc::clone(type_args),
                )
            }
            AnalyzedMacroKind::DefineEmits
            | AnalyzedMacroKind::DefineSlots
            | AnalyzedMacroKind::DefineModel => {
                // Read the macro snapshot directly from the live
                // indexed view obtained at function entry.
                // Source-and-consistency: `owner_indexed` came from
                // `ensure_indexed_ready_serve` (the live view), and its
                // `whole_hash` already discriminates content versions
                // through the cached `MemoEntry`'s self-root rail. A
                // missing `IndexedReady` for a snapshot-consuming arm
                // is a stale key — refuse admission so the next
                // caller cold-recomputes. A missing `script_analysis`
                // on a present `IndexedReady` means the SFC carries no
                // macros — return Miss.
                let snapshot = match owner_indexed
                    .as_ref()
                    .and_then(|indexed| indexed.script_analysis.clone())
                {
                    Some(s) => s,
                    None => {
                        let mut out =
                            crate::project_semantic_dispatch::walk::QueryBuildOutput::from((
                                QueryResult::Error(QueryError::Miss),
                                fence_to_dep_signature(local_fence),
                            ));
                        // Only suppress when the owner artifact is
                        // genuinely missing (stale key). A present
                        // `IndexedReady` with no macros yields the
                        // non-suppressing Miss above.
                        if owner_indexed.is_none() {
                            out.cache_suppress = true;
                        }
                        return FamilyStep::Done(Box::new(out));
                    }
                };
                if snapshot.macros.get(macro_index).is_none() {
                    return FamilyStep::Done(Box::new(
                        (
                            QueryResult::Error(QueryError::Miss),
                            fence_to_dep_signature(local_fence),
                        )
                            .into(),
                    ));
                }
                if type_args.is_empty() {
                    let miss = QueryResult::Value(self.opaque(QueryError::Miss));
                    return answered(rooting, miss, local_fence);
                }
                // Project the macro's first type argument through the
                // shared `ProjectPath` dispatcher in the caller's
                // mode. The consumer (component_meta extractor at
                // `verter_semantic/src/analysis/component_meta.rs:2449+`
                // for emits, the slot-bindings walker for slots, and
                // the model-name composer) walks the resulting
                // surface. Mode-aware so Navigate callers don't pay
                // for full member expansion.
                SemanticQueryKey::ProjectPath {
                    base: type_args[0],
                    path: verter_type_expr::OwnedSlice::from(
                        Vec::<PathSegment>::new().into_boxed_slice(),
                    ),
                    context: crate::semantic_query::ProjectionReductionContext::published(mode),
                }
            }
            AnalyzedMacroKind::DefineExpose | AnalyzedMacroKind::DefineOptions => {
                let result = if type_args.is_empty() {
                    QueryResult::Value(self.opaque(QueryError::Miss))
                } else {
                    QueryResult::Value(type_args[0])
                };
                return answered(rooting, result, local_fence);
            }
        };
        FamilyStep::Need(
            payload,
            Box::new(FamilyBuild::MacroPayload(MacroPayloadBuild::Payload {
                rooting,
                local_fence,
            })),
        )
    }

    /// The last stage of a macro payload's resolution: the payload
    /// operation's read folded into what the first stage retained.
    ///
    /// Fold BOTH metadata signals from the nested macro-payload read:
    /// `cache_suppress` (inner-memo non-cacheability) and
    /// `result_is_partial` (budget/walker partial). A bare
    /// `read.value`-only extraction here would drop both via
    /// `QueryBuildOutput::from`'s default-false, letting a budget-tripped
    /// nested read's partial warm the component-meta result.
    #[inline(never)]
    pub(super) fn finish_resolve_macro_payload(
        &self,
        build: MacroPayloadBuild,
        read: crate::semantic_query::CacheRead<QueryResult<SemanticNodeId>>,
    ) -> crate::project_semantic_dispatch::walk::QueryBuildOutput {
        let MacroPayloadBuild::Payload {
            rooting,
            mut local_fence,
        } = build
        else {
            unreachable!("a macro payload finishes after its owner is read")
        };
        crate::fact_signature_helpers::merge_dep_signature_into_local_fence(
            &mut local_fence,
            &read.dep_signature,
        );
        let partial = read.result_is_partial();
        self.macro_payload_output(
            rooting,
            local_fence,
            read.value,
            read.cache_suppress,
            partial,
        )
    }

    /// A macro payload's output: its value under the fence its stages
    /// recorded, rooted on the owning file and on every file its type
    /// arguments were lowered from, tainted by its payload operation's read.
    fn macro_payload_output(
        &self,
        rooting: MacroPayloadRooting,
        local_fence: Vec<crate::semantic_query::DepEntry>,
        result: QueryResult<SemanticNodeId>,
        nested_cache_suppress: bool,
        nested_result_is_partial: bool,
    ) -> crate::project_semantic_dispatch::walk::QueryBuildOutput {
        let MacroPayloadRooting {
            owner,
            is_non_file_owner,
            owner_whole_hash,
            stale_real_file_owner,
            type_args,
        } = rooting;
        // Self-version rooting: the macro payload was resolved from the
        // owning SFC's macro analysis at the re-sourced `owner_whole_hash`
        // AND from the `type_args` nodes — every arm derives its value
        // from `type_args` (returned directly for `DefineProps` /
        // `WithDefaults` 1-arg and `DefineExpose` / `DefineOptions`;
        // `ReduceIntersection`-normalised for the ≥2-arg props arms;
        // `ProjectPath`-projected for `DefineEmits` / `DefineSlots` /
        // `DefineModel`). When a type argument is file-derived from
        // another canonical the result transitively depends on that
        // file's content, so the carrier must self-root on it too.
        // Root the memo entry on the owning canonical AND on each
        // `NodeScopeId::File`-scoped `type_args` node's
        // `(canonical, observed_hash)` self-root so a content edit to the
        // SFC OR to any type argument's originating file misses the warm
        // read. Structural type args (`Global`-scoped primitives) and an
        // empty `type_args` set contribute nothing.
        //
        // Non-file owner fork: when the owner names no file (the
        // structural sentinel `""`, the builtin carrier `"__builtin__"`,
        // or the synthetic test sentinel `"<synthetic>"`), there is no
        // `FileWholeHash` to root on — `owner_whole_hash` is the default
        // sentinel `0`. Skip the file-side self-root in those cases and
        // rely entirely on the `type_args` self-roots (the same rule the
        // `build_instantiate` non-file base path applies).
        let mut observed_self_roots: Vec<crate::semantic_query_memo::ObservedGraphSelfRoot> =
            if is_non_file_owner {
                Vec::new()
            } else {
                vec![(owner, owner_whole_hash)]
            };
        for arg_root in self.observed_self_roots_from_nodes(type_args.iter().copied()) {
            if !observed_self_roots
                .iter()
                .any(|(c, h)| *c == arg_root.0 && *h == arg_root.1)
            {
                observed_self_roots.push(arg_root);
            }
        }
        let mut output = crate::project_semantic_dispatch::walk::QueryBuildOutput::from((
            result,
            fence_to_dep_signature(local_fence),
        ))
        .with_observed_self_roots(observed_self_roots);
        // Fold the nested macro-payload reads' metadata onto the build
        // output so a budget/walker partial in a nested
        // `ReduceIntersection` / `ProjectPath` read taints this macro
        // payload result (and suppresses the component-meta warm gate),
        // while a benign non-cacheable nested read still refuses inner-memo
        // admission without falsely marking this result partial.
        output.cache_suppress |= nested_cache_suppress;
        output.fold_partial(nested_result_is_partial);
        // Stale real-file owner: the key names a file unknown to the
        // live view (`ensure_indexed_ready_serve == None`). The value flows to
        // the caller, but the entry is non-cacheable — never publish a
        // result self-rooted on the sentinel hash `0`.
        if stale_real_file_owner {
            output.cache_suppress = true;
        }
        output
    }

    /// The first stage of an ordered intersection reduction: the operands
    /// up to the first nested subgroup, whose reduction it reads next, or
    /// the reduction itself when no subgroup remains
    /// ([`Self::advance_reduce_intersection`]).
    #[inline(never)]
    pub(super) fn begin_reduce_intersection(
        &self,
        input: crate::semantic_query::IntersectionInputRef,
        purpose: crate::semantic_query::IntersectionPurpose,
        context: crate::semantic_query::SemanticContextId,
    ) -> FamilyStep {
        verter_audit::attribute!(ReduceIntersection);
        self.graph().record_continuation_begun();
        let Some(steps) = input.as_steps() else {
            let members: Arc<[SemanticNodeId]> = Arc::from(input.as_ordered_values());
            return FamilyStep::Done(Box::new(self.build_normalize_composite(
                &members,
                self.reduce_intersection_members(purpose, &members),
            )));
        };
        let members = Vec::with_capacity(steps.len());
        self.advance_reduce_intersection(ReduceIntersectionBuild {
            input,
            purpose,
            context,
            next: 0,
            members,
        })
    }

    /// Resume an intersection reduction with the read of the nested
    /// subgroup it waited on: a reduced subgroup joins the operands, a
    /// failed one contributes none.
    #[inline(never)]
    pub(super) fn resume_reduce_intersection(
        &self,
        mut build: ReduceIntersectionBuild,
        read: crate::semantic_query::CacheRead<QueryResult<SemanticNodeId>>,
    ) -> FamilyStep {
        match read.value {
            QueryResult::Value(node) | QueryResult::Recursive(node) => build.members.push(node),
            QueryResult::Error(_) => {}
        }
        self.advance_reduce_intersection(build)
    }

    /// Take the reduction's operands from its next step on: each value
    /// joins as it is, the first nested subgroup is read next with what was
    /// taken so far retained, and once every step is taken the operands
    /// reduce in their order.
    fn advance_reduce_intersection(&self, mut build: ReduceIntersectionBuild) -> FamilyStep {
        let steps = build
            .input
            .as_steps()
            .expect("a stepped reduction keeps its recipe");
        while let Some(step) = steps.get(build.next) {
            build.next += 1;
            match step {
                crate::semantic_query::IntersectionTerm::Value(id) => build.members.push(*id),
                crate::semantic_query::IntersectionTerm::EvaluateSubgroup {
                    input: nested,
                    purpose,
                } => {
                    let nested = SemanticQueryKey::ReduceIntersection {
                        input: nested.clone(),
                        purpose: *purpose,
                        context: build.context.clone(),
                    };
                    return FamilyStep::Need(
                        nested,
                        Box::new(FamilyBuild::ReduceIntersection(build)),
                    );
                }
            }
        }
        let members: Arc<[SemanticNodeId]> = Arc::from(std::mem::take(&mut build.members));
        FamilyStep::Done(Box::new(self.build_normalize_composite(
            &members,
            self.reduce_intersection_members(build.purpose, &members),
        )))
    }

    /// The canonical intersection of `members` under `purpose`: a derived
    /// intersection's checker reduction, or a written type node's.
    fn reduce_intersection_members(
        &self,
        purpose: crate::semantic_query::IntersectionPurpose,
        members: &[SemanticNodeId],
    ) -> super::canonical_algebra::CanonicalComposite {
        match purpose {
            crate::semantic_query::IntersectionPurpose::CheckerReduction => {
                super::canonical_algebra::intern_ordered_intersection_charged(
                    self.graph(),
                    members,
                    self.construction_meter(),
                )
            }
            crate::semantic_query::IntersectionPurpose::WrittenTypeNode(nullability) => {
                super::canonical_algebra::intern_written_intersection_charged(
                    self.graph(),
                    members,
                    nullability,
                    self.construction_meter(),
                )
            }
        }
    }
}
