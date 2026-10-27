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

/// An overload set's resolution between its stages: its callee's signature
/// list of one kind, read first and then settled one composite candidate at
/// a time.
pub(super) struct OverloadSetBuild {
    callee: SemanticNodeId,
    kind: crate::semantic_query::SignatureKind,
    list: SignatureListRead,
}

/// One shared signature list being read as a continuation: the subject's
/// `SignaturesOfType` read, then the result each composite candidate's node
/// form applies, one at a time, everything settled before kept. The read
/// the in-place reader makes ([`ProjectSemanticDispatch::shared_signature_nodes`]),
/// its reads made the stages' transitions.
///
/// [`ProjectSemanticDispatch::shared_signature_nodes`]: super::ProjectSemanticDispatch::shared_signature_nodes
pub(super) struct SignatureListRead {
    subject: SemanticNodeId,
    kind: crate::semantic_query::SignatureKind,
    /// Whether the candidates are pinned in the order call resolution tries
    /// them rather than in declaration order.
    resolution: bool,
    /// Whether the list is being read again after a kernel-epoch replacement
    /// retired the first read's handles: a second retirement leaves the list
    /// unsettled.
    reread: bool,
    /// The list once read, with the set value whose lease keeps its
    /// handles readable.
    list: Option<(
        crate::signature_kernel::SignatureSetValue,
        super::signature_discovery::PinnedSignatureList,
    )>,
}

/// Where a [`SignatureListRead`] stands after its latest read.
pub(super) enum ListProgress {
    /// The list reads this query next.
    Read(SemanticQueryKey),
    /// Every candidate as its node, in the list's order.
    Nodes(Vec<SemanticNodeId>),
    /// The list did not settle: never a proven-empty one.
    Unsettled,
}

impl SignatureListRead {
    /// A read of `subject`'s signature list of `kind`, and the query it
    /// reads first.
    pub(super) fn begin(
        subject: SemanticNodeId,
        kind: crate::semantic_query::SignatureKind,
        resolution: bool,
    ) -> (Self, SemanticQueryKey) {
        let list = Self {
            subject,
            kind,
            resolution,
            reread: false,
            list: None,
        };
        let read = list.list_key();
        (list, read)
    }

    /// The subject's `SignaturesOfType` read.
    fn list_key(&self) -> SemanticQueryKey {
        SemanticQueryKey::SignaturesOfType {
            subject: self.subject,
            kind: self.kind,
            context: crate::semantic_query::SemanticContextId::production(),
        }
    }
}

/// A callable's apparent type between its stages: the base, the fence its
/// first stage pinned, and where the widening stands.
pub(super) struct ApparentTypeBuild {
    base: SemanticNodeId,
    demand_scope: crate::semantic_query::ApparentDemandScope,
    fence: crate::semantic_query::DepSignature,
    stage: ApparentStage,
}

/// The step of an apparent-type widening a read resumes.
enum ApparentStage {
    /// Classifying the base's anchor: reading the signature list of `kind`
    /// of the object surface `node` the alias walk reached.
    Anchor {
        visited: rustc_hash::FxHashSet<SemanticNodeId>,
        kind: crate::semantic_query::SignatureKind,
        list: SignatureListRead,
    },
    /// Under `strictBindCallApply`, reading the base's call signatures, then
    /// its construct signatures, to choose the strict interface.
    Buckets {
        scope: ApparentScope,
        calls: Option<Vec<SemanticNodeId>>,
        list: SignatureListRead,
    },
    /// Looking up the global interface `name`; `fallback` when the
    /// callable `Function` interface is looked up after it finds none.
    Global {
        scope: ApparentScope,
        name: &'static str,
        fallback: bool,
    },
    /// Reading the interface's instantiation.
    Surface { rootless: bool },
}

/// The canonical an apparent lookup is scoped by, and whether it is a
/// rootless callable's demand canonical.
struct ApparentScope {
    canonical: Arc<str>,
    rootless: bool,
}

/// A class's instance surface between its stages: its declaration slot and
/// type arguments, then the content version its declaring file served,
/// which the surface roots on, while it reads the instantiation.
pub(super) struct ClassInstanceSurfaceBuild {
    decl_slot: crate::semantic_query::ResolvedDeclSlotIdentity,
    type_args: Arc<[SemanticNodeId]>,
    observed: Option<verter_session_query::analysis::types::Hash16>,
}

/// A class's static surface between its stages: its declaration slot, type
/// arguments and projection mode, the file versions it roots on so far, and
/// where its composition stands.
pub(super) struct ClassStaticSurfaceBuild {
    decl_slot: crate::semantic_query::ResolvedDeclSlotIdentity,
    type_args: Arc<[SemanticNodeId]>,
    mode: ProjectionMode,
    /// Whether the declaring file served its content version.
    observed: bool,
    observed_self_roots: Vec<crate::semantic_query_memo::ObservedGraphSelfRoot>,
    /// The effective declaring file, when the slot rebased onto one,
    /// served no content version.
    effective_root_missing: bool,
    stage: ClassStaticStage,
}

/// The step of a class's static composition a read resumes.
enum ClassStaticStage {
    /// Reading the declaring file.
    Declaring,
    /// Looking the effective prepared value declaration up.
    Owner,
    /// Reading the effective declaring file the slot rebased onto.
    OwnSource {
        own: ClassStaticOwn,
        prepared: PreparedValue,
    },
    /// Lowering the constructor shape and reading the heritage list.
    Constructor {
        own: ClassStaticOwn,
        prepared: PreparedValue,
    },
    /// Composing the heritage bases in clause order: `current` is the base
    /// being classified, the rest wait in `heritage`; a declared base's
    /// static surface is read before the next base is classified.
    Bases {
        own: ClassStaticOwn,
        own_node: SemanticNodeId,
        heritage: std::vec::IntoIter<super::build::HeritageBase>,
        current: Option<super::build::HeritageBase>,
        surfaces: Vec<SemanticNodeId>,
        partial: bool,
    },
    /// Folding the own surface and the bases' into the composed surface.
    Finish {
        own: ClassStaticOwn,
        own_node: SemanticNodeId,
        surfaces: Vec<SemanticNodeId>,
        partial: bool,
    },
}

type PreparedValue =
    verter_retention::heap_size::OwnedValue<verter_session_query::type_solver::PreparedValueDecl>;

/// The effective declaring identity of a class's static side.
pub(super) struct ClassStaticOwn {
    pub(super) canonical: OwnedStr,
    pub(super) owner: verter_type_expr::TopLevelOwnerId,
    pub(super) symbol: OwnedStr,
}

/// The declaration read a class's static surface stands at, answered in
/// place by its driver
/// ([`ProjectSemanticDispatch::read_class_static_declarations`](super::ProjectSemanticDispatch::read_class_static_declarations)).
pub(super) enum ClassStaticRead<'b> {
    /// The effective prepared value declaration of this slot.
    Owner(&'b crate::semantic_query::ResolvedDeclSlotIdentity),
    /// The constructor shape lowered from `prepared`, and the heritage list.
    Constructor {
        own: &'b ClassStaticOwn,
        prepared: &'b verter_session_query::type_solver::PreparedValueDecl,
        mode: ProjectionMode,
    },
    /// What `base` contributes.
    Base {
        own: &'b ClassStaticOwn,
        base: &'b super::build::HeritageBase,
        mode: ProjectionMode,
    },
    /// The composed surface over `own_node` and the bases' `surfaces`.
    Finish {
        own: &'b ClassStaticOwn,
        own_node: SemanticNodeId,
        surfaces: &'b [SemanticNodeId],
        type_args: &'b Arc<[SemanticNodeId]>,
    },
}

/// The answer to a [`ClassStaticRead`].
pub(super) enum ClassStaticAnswer {
    Owner(Option<super::build::EffectivePreparedValueDecl>),
    Constructor(Option<SemanticNodeId>, Vec<super::build::HeritageBase>),
    Base(super::build::ClassBaseStatic),
    Finish(SemanticNodeId),
}

impl ClassStaticSurfaceBuild {
    /// The declaration read the build stands at.
    pub(super) fn declaration_read(&self) -> ClassStaticRead<'_> {
        match &self.stage {
            ClassStaticStage::Owner => ClassStaticRead::Owner(&self.decl_slot),
            ClassStaticStage::Constructor { own, prepared } => ClassStaticRead::Constructor {
                own,
                prepared,
                mode: self.mode,
            },
            ClassStaticStage::Bases {
                own,
                current: Some(base),
                ..
            } => ClassStaticRead::Base {
                own,
                base,
                mode: self.mode,
            },
            ClassStaticStage::Finish {
                own,
                own_node,
                surfaces,
                ..
            } => ClassStaticRead::Finish {
                own,
                own_node: *own_node,
                surfaces,
                type_args: &self.type_args,
            },
            ClassStaticStage::Declaring
            | ClassStaticStage::OwnSource { .. }
            | ClassStaticStage::Bases { current: None, .. } => {
                unreachable!("a class's static surface reads a file or a query here")
            }
        }
    }
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

    /// The first stage of an overload set's resolution: the callee's shared
    /// signature list of `kind` is read next. The candidates ARE that list,
    /// in the order call resolution tries them: the list owns carrier
    /// settlement, alias and constraint hops, apparent globals and the
    /// union and intersection procedures, and the kernel owns the order;
    /// the set chooses nothing.
    #[inline(never)]
    pub(super) fn begin_resolve_overload_set(
        &self,
        callee: SemanticNodeId,
        kind: crate::semantic_query::SignatureKind,
    ) -> FamilyStep {
        self.graph().record_continuation_begun();
        let (list, read) = SignatureListRead::begin(callee, kind, true);
        FamilyStep::Need(
            read,
            Box::new(FamilyBuild::OverloadSet(OverloadSetBuild {
                callee,
                kind,
                list,
            })),
        )
    }

    /// Resume an overload set's resolution with the read it waited on: its
    /// callee's signature list, or the result the composite candidate it
    /// prepared applies. Once every candidate is a node the set is built.
    #[inline(never)]
    pub(super) fn resume_resolve_overload_set(
        &self,
        mut build: OverloadSetBuild,
        read: crate::semantic_query::CacheRead<
            QueryResult<crate::semantic_query::SemanticQueryValue>,
        >,
    ) -> FamilyStep {
        match self.advance_signature_list(&mut build.list, read) {
            ListProgress::Read(key) => {
                FamilyStep::Need(key, Box::new(FamilyBuild::OverloadSet(build)))
            }
            ListProgress::Nodes(nodes) => FamilyStep::Done(Box::new(self.overload_set_output(
                build.callee,
                build.kind,
                nodes,
            ))),
            ListProgress::Unsettled => FamilyStep::Done(Box::new(Self::unsettled_overload_set())),
        }
    }

    /// Carry a signature list on with the read it waited on: its
    /// `SignaturesOfType` read, or the result its prepared composite
    /// candidate applies. The next composite's result is read next, and once
    /// every candidate is a node the list is done. A list whose handles a
    /// kernel-epoch replacement retired is read once more, in the current
    /// epoch; any other failure, or a second retirement, leaves the list
    /// unsettled.
    fn advance_signature_list(
        &self,
        list: &mut SignatureListRead,
        read: crate::semantic_query::CacheRead<
            QueryResult<crate::semantic_query::SemanticQueryValue>,
        >,
    ) -> ListProgress {
        use crate::semantic_query::SemanticQueryValue;
        let settled = match list.list.as_mut() {
            None => match read.value {
                QueryResult::Value(SemanticQueryValue::SignatureSet(value))
                    if read.fact_status.is_complete() =>
                {
                    self.pin_signature_list(list.kind, &value, list.resolution)
                        .map(|pinned| list.list = Some((value, pinned)))
                }
                _ => return ListProgress::Unsettled,
            },
            Some((_, pinned)) => match read.value {
                QueryResult::Value(SemanticQueryValue::SignatureResult(result)) => {
                    self.settle_composite_signature(pinned, result.result)
                }
                _ => return ListProgress::Unsettled,
            },
        };
        let next = settled.and_then(|()| {
            let (_, pinned) = list
                .list
                .as_mut()
                .expect("a list settles its candidates once it is read");
            self.prepare_next_composite(pinned)
        });
        match next {
            Ok(Some(key)) => ListProgress::Read(SemanticQueryKey::ReadSignatureResult(key)),
            Ok(None) => {
                let (_, pinned) = list.list.take().expect("the list was read");
                ListProgress::Nodes(pinned.into_nodes())
            }
            Err(super::signature_discovery::SignatureSetReadFailure::Retired) if !list.reread => {
                list.reread = true;
                list.list = None;
                ListProgress::Read(list.list_key())
            }
            Err(_) => ListProgress::Unsettled,
        }
    }

    /// The first stage of a callable's apparent type: the walk from `base`
    /// through alias hops to the callable that anchors the ambient lookup.
    /// An object surface's first call signature — else its first construct
    /// signature — anchors it, read from the shared signature list
    /// ([`Self::resume_apparent_type`]).
    #[inline(never)]
    pub(super) fn begin_apparent_type(
        &self,
        base: SemanticNodeId,
        context: &crate::semantic_query::ApparentTypeContext,
    ) -> FamilyStep {
        self.graph().record_continuation_begun();
        let build = ApparentTypeBuild {
            base,
            demand_scope: context.demand_scope.clone(),
            fence: self.project_generation_signature(),
            stage: ApparentStage::Surface { rootless: false },
        };
        self.walk_apparent_anchor(build, base, rustc_hash::FxHashSet::default())
    }

    /// Walk from `node` to the callable anchoring the apparent lookup: an
    /// authored signature or deferred callable anchors it in its declaring
    /// file, a signature without an authored occurrence is rootless, and an
    /// object surface's signature list is read next. Anything else — or a
    /// walk that revisits a node — is not callable.
    fn walk_apparent_anchor(
        &self,
        mut build: ApparentTypeBuild,
        mut node: SemanticNodeId,
        mut visited: rustc_hash::FxHashSet<SemanticNodeId>,
    ) -> FamilyStep {
        use crate::semantic_query::SemanticNodeData;
        enum Hop {
            Next(SemanticNodeId),
            Authored(Arc<str>),
            Rootless,
            Object,
            NotCallable,
        }
        let graph = self.graph();
        loop {
            if !visited.insert(node) {
                return FamilyStep::Done(Box::new(self.apparent_miss()));
            }
            let hop = match graph.node_data(node).as_deref() {
                Some(SemanticNodeData::Alias(target)) => Hop::Next(*target),
                Some(SemanticNodeData::Signature { occurrence, .. }) => match occurrence {
                    Some(occurrence) => {
                        Hop::Authored(Arc::from(occurrence.function.anchor.canonical_id.as_ref()))
                    }
                    None => Hop::Rootless,
                },
                Some(SemanticNodeData::DeferredCallable(callable)) => {
                    Hop::Authored(Arc::from(callable.declaring_canonical()))
                }
                Some(SemanticNodeData::Object(_)) => Hop::Object,
                _ => Hop::NotCallable,
            };
            match hop {
                Hop::Next(target) => node = target,
                Hop::Authored(canonical) => {
                    return self.apparent_scoped(
                        build,
                        ApparentScope {
                            canonical,
                            rootless: false,
                        },
                    )
                }
                Hop::Rootless => {
                    let crate::semantic_query::ApparentDemandScope::Rootless { canonical } =
                        &build.demand_scope
                    else {
                        // No demand-scope witness: no project to scope the
                        // ambient lookup by. Fail closed.
                        return FamilyStep::Done(Box::new(self.apparent_miss()));
                    };
                    let canonical = Arc::from(canonical.as_str());
                    return self.apparent_scoped(
                        build,
                        ApparentScope {
                            canonical,
                            rootless: true,
                        },
                    );
                }
                Hop::Object => {
                    let kind = crate::semantic_query::SignatureKind::Call;
                    let (list, read) = SignatureListRead::begin(node, kind, false);
                    build.stage = ApparentStage::Anchor {
                        visited,
                        kind,
                        list,
                    };
                    return FamilyStep::Need(read, Box::new(FamilyBuild::ApparentType(build)));
                }
                Hop::NotCallable => return FamilyStep::Done(Box::new(self.apparent_miss())),
            }
        }
    }

    /// The apparent lookup once its scope is known: under
    /// `strictBindCallApply` the base's call signatures are read next, to
    /// choose between `CallableFunction` and `NewableFunction`; otherwise
    /// the callable `Function` interface is looked up.
    fn apparent_scoped(&self, mut build: ApparentTypeBuild, scope: ApparentScope) -> FamilyStep {
        if self.strict_bind_call_apply(scope.canonical.as_ref()) {
            let (list, read) = SignatureListRead::begin(
                build.base,
                crate::semantic_query::SignatureKind::Call,
                false,
            );
            build.stage = ApparentStage::Buckets {
                scope,
                calls: None,
                list,
            };
            return FamilyStep::Need(read, Box::new(FamilyBuild::ApparentType(build)));
        }
        Self::look_up_apparent_global(
            build,
            scope,
            super::apparent_type::CALLABLE_APPARENT_INTERFACE,
            false,
        )
    }

    /// Look the global interface `name` up next in `scope`'s project.
    fn look_up_apparent_global(
        mut build: ApparentTypeBuild,
        scope: ApparentScope,
        name: &'static str,
        fallback: bool,
    ) -> FamilyStep {
        let demand = Arc::clone(&scope.canonical);
        build.stage = ApparentStage::Global {
            scope,
            name,
            fallback,
        };
        FamilyStep::Global(
            super::family_continuation::GlobalRead { demand, name },
            Box::new(FamilyBuild::ApparentType(build)),
        )
    }

    /// Resume a callable's apparent type with the read it waited on: a
    /// signature list of its anchor walk or of its strict-interface choice,
    /// or the instantiation of the interface it widens to.
    #[inline(never)]
    pub(super) fn resume_apparent_type(
        &self,
        mut build: ApparentTypeBuild,
        read: crate::semantic_query::CacheRead<
            QueryResult<crate::semantic_query::SemanticQueryValue>,
        >,
    ) -> FamilyStep {
        match std::mem::replace(&mut build.stage, ApparentStage::Surface { rootless: false }) {
            ApparentStage::Anchor {
                visited,
                kind,
                mut list,
            } => match self.advance_signature_list(&mut list, read) {
                ListProgress::Read(key) => {
                    build.stage = ApparentStage::Anchor {
                        visited,
                        kind,
                        list,
                    };
                    FamilyStep::Need(key, Box::new(FamilyBuild::ApparentType(build)))
                }
                // An unsettled list leaves the anchor undecided.
                ListProgress::Unsettled => FamilyStep::Done(Box::new(self.apparent_miss())),
                ListProgress::Nodes(nodes) => match nodes.first() {
                    Some(first) => self.walk_apparent_anchor(build, *first, visited),
                    None if kind == crate::semantic_query::SignatureKind::Call => {
                        let kind = crate::semantic_query::SignatureKind::Construct;
                        let (list, read) = SignatureListRead::begin(list.subject, kind, false);
                        build.stage = ApparentStage::Anchor {
                            visited,
                            kind,
                            list,
                        };
                        FamilyStep::Need(read, Box::new(FamilyBuild::ApparentType(build)))
                    }
                    None => FamilyStep::Done(Box::new(self.apparent_miss())),
                },
            },
            ApparentStage::Buckets {
                scope,
                calls,
                mut list,
            } => match (self.advance_signature_list(&mut list, read), calls) {
                (ListProgress::Read(key), calls) => {
                    build.stage = ApparentStage::Buckets { scope, calls, list };
                    FamilyStep::Need(key, Box::new(FamilyBuild::ApparentType(build)))
                }
                // An unsettled bucket chooses no strict interface.
                (ListProgress::Unsettled, _) => Self::look_up_apparent_global(
                    build,
                    scope,
                    super::apparent_type::CALLABLE_APPARENT_INTERFACE,
                    false,
                ),
                (ListProgress::Nodes(calls), None) => {
                    let (list, read) = SignatureListRead::begin(
                        build.base,
                        crate::semantic_query::SignatureKind::Construct,
                        false,
                    );
                    build.stage = ApparentStage::Buckets {
                        scope,
                        calls: Some(calls),
                        list,
                    };
                    FamilyStep::Need(read, Box::new(FamilyBuild::ApparentType(build)))
                }
                (ListProgress::Nodes(constructs), Some(calls)) => {
                    if !calls.is_empty() {
                        Self::look_up_apparent_global(
                            build,
                            scope,
                            super::apparent_type::CALLABLE_STRICT_INTERFACE,
                            true,
                        )
                    } else if !constructs.is_empty() {
                        Self::look_up_apparent_global(
                            build,
                            scope,
                            super::apparent_type::NEWABLE_STRICT_INTERFACE,
                            true,
                        )
                    } else {
                        Self::look_up_apparent_global(
                            build,
                            scope,
                            super::apparent_type::CALLABLE_APPARENT_INTERFACE,
                            false,
                        )
                    }
                }
            },
            ApparentStage::Surface { rootless } => {
                match super::narrow_value_cache_read(read).value {
                    QueryResult::Value(surface) => {
                        let mut output: crate::project_semantic_dispatch::walk::QueryBuildOutput =
                            (QueryResult::Value(surface), build.fence).into();
                        // A rootless apparent value never enters the shared
                        // memo; the value still flows to the caller, and the
                        // suppress taint folds through every enclosing
                        // member/path/call query at the universal read
                        // boundary.
                        output.cache_suppress |= rootless;
                        FamilyStep::Done(Box::new(output))
                    }
                    QueryResult::Recursive(_) | QueryResult::Error(_) => {
                        FamilyStep::Done(Box::new(self.apparent_miss()))
                    }
                }
            }
            ApparentStage::Global { .. } => {
                unreachable!("an apparent type's global lookup resumes with its declaration")
            }
        }
    }

    /// Resume a callable's apparent type with the global declaration its
    /// lookup found: the declaration's instantiation is read next. A strict
    /// interface the project does not declare falls back to `Function`; a
    /// project declaring neither gives no apparent surface.
    #[inline(never)]
    pub(super) fn resume_apparent_global(
        &self,
        mut build: ApparentTypeBuild,
        found: Option<verter_session_query::type_solver::host::ResolvedRootIdentity>,
    ) -> FamilyStep {
        let ApparentStage::Global {
            scope,
            name,
            fallback,
        } = std::mem::replace(&mut build.stage, ApparentStage::Surface { rootless: false })
        else {
            unreachable!("an apparent type resumes its global lookup at that lookup")
        };
        match found {
            Some(declaration) => {
                build.stage = ApparentStage::Surface {
                    rootless: scope.rootless,
                };
                FamilyStep::Need(
                    self.apparent_surface_key(&declaration, name),
                    Box::new(FamilyBuild::ApparentType(build)),
                )
            }
            None if fallback => Self::look_up_apparent_global(
                build,
                scope,
                super::apparent_type::CALLABLE_APPARENT_INTERFACE,
                false,
            ),
            None => FamilyStep::Done(Box::new(self.apparent_miss())),
        }
    }

    /// The first stage of a class's instance surface: its declaring file's
    /// indexed input is read next ([`Self::resume_class_instance_source`]).
    #[inline(never)]
    pub(super) fn begin_class_instance_surface(
        &self,
        decl_slot: &crate::semantic_query::ResolvedDeclSlotIdentity,
        type_args: &Arc<[SemanticNodeId]>,
    ) -> FamilyStep {
        self.graph().record_continuation_begun();
        FamilyStep::Load(
            Arc::from(decl_slot.defining_canonical.as_ref()),
            Box::new(FamilyBuild::ClassInstanceSurface(
                ClassInstanceSurfaceBuild {
                    decl_slot: decl_slot.clone(),
                    type_args: Arc::clone(type_args),
                    observed: None,
                },
            )),
        )
    }

    /// Resume a class's instance surface with its declaring file's indexed
    /// input (`None` when the live view serves none, and the surface is
    /// then never admitted): the TYPE-space half's instantiation under
    /// `Shallow` is read next.
    #[inline(never)]
    pub(super) fn resume_class_instance_source(
        &self,
        mut build: ClassInstanceSurfaceBuild,
        serve: Option<verter_session_query::inputs::indexed::IndexedInputServe>,
    ) -> FamilyStep {
        build.observed = serve.map(|serve| serve.indexed.whole_hash);
        let key = self.class_instance_key(&build.decl_slot, &build.type_args);
        FamilyStep::Need(key, Box::new(FamilyBuild::ClassInstanceSurface(build)))
    }

    /// A class's instance surface from its instantiation's read: the
    /// instantiated node itself (an identity projection), tainted by the
    /// read's partiality and rooted on the declaring file.
    #[inline(never)]
    pub(super) fn finish_class_instance_surface(
        &self,
        build: ClassInstanceSurfaceBuild,
        read: crate::semantic_query::CacheRead<QueryResult<SemanticNodeId>>,
    ) -> crate::project_semantic_dispatch::walk::QueryBuildOutput {
        // Two-signal fold: a budget/recursion/walker-fatal instantiation
        // surfaces as a partial instead of returning through the identity
        // projection as complete.
        let partial = read.result_is_partial();
        let mut output: crate::project_semantic_dispatch::walk::QueryBuildOutput =
            (read.value, self.project_generation_signature()).into();
        output.fold_partial(partial);
        let observed_self_roots = match build.observed {
            Some(whole_hash) => vec![(build.decl_slot.defining_canonical.clone(), whole_hash)],
            // The declaration's live content version could not be observed:
            // the value still flows to the caller, the memo refuses it.
            None => {
                output.cache_suppress = true;
                Vec::new()
            }
        };
        output
            .with_observed_self_roots(observed_self_roots)
            .with_positional_guarantee()
    }

    /// The first stage of a class's static surface: its declaring file's
    /// indexed input is read next ([`Self::resume_class_static_source`]).
    #[inline(never)]
    pub(super) fn begin_class_static_surface(
        &self,
        decl_slot: &crate::semantic_query::ResolvedDeclSlotIdentity,
        type_args: &Arc<[SemanticNodeId]>,
        mode: ProjectionMode,
    ) -> FamilyStep {
        self.graph().record_continuation_begun();
        FamilyStep::Load(
            Arc::from(decl_slot.defining_canonical.as_ref()),
            Box::new(FamilyBuild::ClassStaticSurface(ClassStaticSurfaceBuild {
                decl_slot: decl_slot.clone(),
                type_args: Arc::clone(type_args),
                mode,
                observed: false,
                observed_self_roots: Vec::new(),
                effective_root_missing: false,
                stage: ClassStaticStage::Declaring,
            })),
        )
    }

    /// Resume a class's static surface with the indexed input of the file
    /// it read (`None` when the live view serves none, and the surface is
    /// then never admitted): the declaring file, after which the effective
    /// prepared value declaration is looked up; or the effective declaring
    /// file the slot rebased onto, after which the constructor shape lowers.
    /// The surface roots on each file's served content version.
    #[inline(never)]
    pub(super) fn resume_class_static_source(
        &self,
        mut build: ClassStaticSurfaceBuild,
        serve: Option<verter_session_query::inputs::indexed::IndexedInputServe>,
    ) -> FamilyStep {
        build.stage = match std::mem::replace(&mut build.stage, ClassStaticStage::Declaring) {
            ClassStaticStage::Declaring => {
                if let Some(serve) = serve {
                    build.observed = true;
                    build.observed_self_roots.push((
                        build.decl_slot.defining_canonical.clone(),
                        serve.indexed.whole_hash,
                    ));
                }
                ClassStaticStage::Owner
            }
            ClassStaticStage::OwnSource { own, prepared } => {
                match serve {
                    Some(serve) => build
                        .observed_self_roots
                        .push((own.canonical.clone(), serve.indexed.whole_hash)),
                    // The declaring file's content version could not be
                    // observed: refuse warm admission, which would otherwise
                    // survive a declaring-file edit.
                    None => build.effective_root_missing = true,
                }
                ClassStaticStage::Constructor { own, prepared }
            }
            _ => unreachable!("a class's static surface reads a file only for its declaration"),
        };
        FamilyStep::Declarations(Box::new(FamilyBuild::ClassStaticSurface(build)))
    }

    /// Resume a class's static surface with the answer to the declaration
    /// read it stood at.
    #[inline(never)]
    pub(super) fn resume_class_static_declarations(
        &self,
        mut build: ClassStaticSurfaceBuild,
        answer: ClassStaticAnswer,
    ) -> FamilyStep {
        let stage = std::mem::replace(&mut build.stage, ClassStaticStage::Declaring);
        match (stage, answer) {
            (ClassStaticStage::Owner, ClassStaticAnswer::Owner(None)) => {
                let suppress = !build.observed;
                FamilyStep::Done(Box::new(self.class_static_miss(build, suppress)))
            }
            (ClassStaticStage::Owner, ClassStaticAnswer::Owner(Some(effective))) => {
                let (canonical, owner, symbol, prepared) = effective;
                let rebased = canonical.as_ref() != build.decl_slot.defining_canonical.as_ref();
                let own = ClassStaticOwn {
                    canonical,
                    owner,
                    symbol,
                };
                if rebased {
                    let file = Arc::from(own.canonical.as_ref());
                    build.stage = ClassStaticStage::OwnSource { own, prepared };
                    FamilyStep::Load(file, Box::new(FamilyBuild::ClassStaticSurface(build)))
                } else {
                    build.stage = ClassStaticStage::Constructor { own, prepared };
                    FamilyStep::Declarations(Box::new(FamilyBuild::ClassStaticSurface(build)))
                }
            }
            (ClassStaticStage::Constructor { .. }, ClassStaticAnswer::Constructor(None, _)) => {
                let suppress = !build.observed || build.effective_root_missing;
                FamilyStep::Done(Box::new(self.class_static_miss(build, suppress)))
            }
            (
                ClassStaticStage::Constructor { own, .. },
                ClassStaticAnswer::Constructor(Some(own_node), bases),
            ) => {
                build.stage = ClassStaticStage::Bases {
                    own,
                    own_node,
                    heritage: bases.into_iter(),
                    current: None,
                    surfaces: Vec::new(),
                    partial: false,
                };
                self.advance_class_static_bases(build)
            }
            (mut stage @ ClassStaticStage::Bases { .. }, ClassStaticAnswer::Base(base)) => {
                let ClassStaticStage::Bases {
                    current, surfaces, ..
                } = &mut stage
                else {
                    unreachable!("the stage was just matched")
                };
                *current = None;
                match base {
                    super::build::ClassBaseStatic::Skip => {}
                    super::build::ClassBaseStatic::Surface(node) => surfaces.push(node),
                    super::build::ClassBaseStatic::Read(key) => {
                        build.stage = stage;
                        return FamilyStep::Need(
                            key,
                            Box::new(FamilyBuild::ClassStaticSurface(build)),
                        );
                    }
                }
                build.stage = stage;
                self.advance_class_static_bases(build)
            }
            (ClassStaticStage::Finish { partial, .. }, ClassStaticAnswer::Finish(composed)) => {
                let mut output: crate::project_semantic_dispatch::walk::QueryBuildOutput = (
                    QueryResult::Value(composed),
                    self.project_generation_signature(),
                )
                    .into();
                output.fold_partial(partial);
                if !build.observed || build.effective_root_missing {
                    output.cache_suppress = true;
                }
                output.approximation = Some(if partial {
                    super::projection_fact::ProjectionInterpretation::PresenceOnly
                } else {
                    super::projection_fact::ProjectionInterpretation::Positional
                });
                FamilyStep::Done(Box::new(
                    output.with_observed_self_roots(build.observed_self_roots),
                ))
            }
            _ => unreachable!("a declaration read answers the stage that named it"),
        }
    }

    /// Resume a class's static surface with the read of a declared base's
    /// static surface: a partial read taints the composition, a value joins
    /// the bases' surfaces, and the next base is classified.
    #[inline(never)]
    pub(super) fn resume_class_static_base(
        &self,
        mut build: ClassStaticSurfaceBuild,
        read: crate::semantic_query::CacheRead<QueryResult<SemanticNodeId>>,
    ) -> FamilyStep {
        let ClassStaticStage::Bases {
            surfaces, partial, ..
        } = &mut build.stage
        else {
            unreachable!("a class's static surface reads a query only for a base")
        };
        *partial |= read.result_is_partial();
        if let QueryResult::Value(base_node) = read.value {
            surfaces.push(base_node);
        }
        self.advance_class_static_bases(build)
    }

    /// Classify the next heritage base, or fold the composition once every
    /// base contributed.
    fn advance_class_static_bases(&self, mut build: ClassStaticSurfaceBuild) -> FamilyStep {
        let ClassStaticStage::Bases {
            heritage, current, ..
        } = &mut build.stage
        else {
            unreachable!("only the bases' stage advances through the heritage")
        };
        match heritage.next() {
            Some(base) => *current = Some(base),
            None => {
                let ClassStaticStage::Bases {
                    own,
                    own_node,
                    surfaces,
                    partial,
                    ..
                } = std::mem::replace(&mut build.stage, ClassStaticStage::Declaring)
                else {
                    unreachable!("the stage was just matched")
                };
                build.stage = ClassStaticStage::Finish {
                    own,
                    own_node,
                    surfaces,
                    partial,
                };
            }
        }
        FamilyStep::Declarations(Box::new(FamilyBuild::ClassStaticSurface(build)))
    }

    /// The miss a static surface without an effective declaration or a
    /// constructor shape answers, rooted on the files it read; `suppress`
    /// when one of them served no content version.
    fn class_static_miss(
        &self,
        build: ClassStaticSurfaceBuild,
        suppress: bool,
    ) -> crate::project_semantic_dispatch::walk::QueryBuildOutput {
        let mut output: crate::project_semantic_dispatch::walk::QueryBuildOutput = (
            QueryResult::Error(QueryError::Miss),
            super::empty_signature(),
        )
            .into();
        if suppress {
            output.cache_suppress = true;
        }
        output.with_observed_self_roots(build.observed_self_roots)
    }

    /// The miss an apparent type without a surface answers.
    fn apparent_miss(&self) -> crate::project_semantic_dispatch::walk::QueryBuildOutput {
        (
            QueryResult::Error(QueryError::Miss),
            self.project_generation_signature(),
        )
            .into()
    }

    /// The miss an unsettled candidate list answers: never admitted, so the
    /// next read settles it again.
    fn unsettled_overload_set() -> crate::project_semantic_dispatch::walk::QueryBuildOutput {
        let mut output: crate::project_semantic_dispatch::walk::QueryBuildOutput = (
            QueryResult::Error(QueryError::Miss),
            super::empty_signature(),
        )
            .into();
        output.cache_suppress = true;
        output
    }
}
