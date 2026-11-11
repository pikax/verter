//! A query entry's admission as an owned computation.
//!
//! Before a projection query is claimed, its entry settles the key the
//! family memo identifies it by: sugar canonicalizes (`ProjectMember` and
//! `IndexedAccess` are one-segment `ProjectPath`s), a carrier subject — an
//! unresolved `BareRef` / `ImportType` / `typeof` reference — resolves to
//! the subject it names, and a subject every project shares is read through
//! the apparent wrapper or signatures the demanding project declares. Those
//! normalizations read other queries (the carrier's declaration, its
//! instantiation, its `typeof` value, the wrapper's instantiation) and the
//! resolver's global lookup, under one fact tracer whose facts — the
//! prelude ([`CarrierNormalizationPrelude`]) — join the admitted entry's
//! publication.
//!
//! The admission is written as a continuation ([`QueryEntryBuild`]): each
//! stage either settles the key or names the one query, port or file it
//! reads next, retaining the tracer, the carrier's resolution
//! ([`CarrierRead`]), the observation held around it and everything settled
//! so far. One computation is driven three ways:
//!
//! - the synchronous entry ([`ProjectSemanticDispatch::admit_entry_in_place`])
//!   reads each named query where the entry stands;
//! - a polled root entry (`query_frames`) polls each named query as a
//!   nested root, so a read another task is producing returns `Pending`
//!   with the admission retained;
//! - a frame whose family build needs a projection admits it in the frame
//!   (`query_frames`): each named query becomes the frame's need, and the
//!   admitted canonical key is needed last, its prelude carried to the
//!   frame that produces it.
//!
//! The admitted key is claimed only once its admission settled; no stage
//! runs again, and admission is no memoized query of its own and records no
//! receipt: its reads are charged to the entry that needs them, as the
//! synchronous entry's nested reads are.

use std::sync::Arc;
use std::task::Poll;

use crate::semantic_query::{
    CacheRead, PathSegment, ProjectionMode, ProjectionReductionContext, PropertyKey, QueryError,
    QueryResult, SemanticNodeData, SemanticNodeId, SemanticQueryKey,
};

use super::carrier::{CarrierRead, CarrierResolution, CarrierStep};
use super::flow_return_fact::ObservedRead;
use super::port_continuations::{GlobalLookup, GlobalStep};
use super::{canonicalize_node_list, CarrierNormalizationPrelude, ProjectSemanticDispatch};

/// A node-domain read an admission stage takes.
pub(super) type NodeRead = CacheRead<QueryResult<SemanticNodeId>>;

/// A query entry's admission between two of its stages: the key settled so
/// far, the stage it stands at and what its earlier stages observed.
pub(super) struct QueryEntryBuild {
    key: SemanticQueryKey,
    stage: EntryStage,
    /// The admission's fact tracer, taken while a stage runs.
    tracer: Option<crate::fact_signature_helpers::OwnedStepwiseFactTracer>,
    /// The carrier the admission resolved its subject to, once it did.
    normalized_source: Option<CarrierResolution>,
    /// The typed incompleteness the admission observed.
    partial_reasons: crate::semantic_query::PartialReasonSet,
}

/// Where an admission stands.
enum EntryStage {
    /// Not begun: its first stage normalizes the carrier subject.
    Begun,
    /// The carrier subject's resolution stands at a read, the observation
    /// held around it retained until the resolution settles.
    Carrier {
        read: Box<CarrierRead>,
        observed: ObservedRead<()>,
    },
    /// The apparent wrapper's global lookup or instantiation.
    Wrapper(Box<WrapperRead>),
}

/// An apparent-wrapper read: the subject a shared key reads through the
/// global interface the demanding project declares.
struct WrapperRead {
    kind: WrapperKind,
    name: &'static str,
    args: Vec<SemanticNodeId>,
    lookup: GlobalLookup,
    awaiting: WrapperAwait,
}

/// Which subject the wrapper replaces.
#[derive(Clone, Copy)]
enum WrapperKind {
    /// A path's base whose first step reads its apparent wrapper.
    Path,
    /// A reducing `keyof` of a scope-less array.
    KeyOf,
    /// A signature read's subject taking its signatures from a wrapper.
    Signatures,
}

impl WrapperKind {
    /// The context the wrapper interface is instantiated under: the members
    /// and signature carriers a member or signature read takes, or the keys
    /// alone for a `keyof`.
    fn context(self) -> ProjectionReductionContext {
        match self {
            Self::Path | Self::Signatures => {
                ProjectionReductionContext::structural_transit_with_mode(ProjectionMode::Shallow)
            }
            Self::KeyOf => ProjectionReductionContext::structural_transit(),
        }
    }
}

/// What a wrapper read waits on.
enum WrapperAwait {
    /// The global lookup's port.
    Port,
    /// The global lookup's file.
    Source(Arc<str>),
    /// A pending contributor the lookup indexes.
    Index(Arc<str>),
    /// The wrapper interface's instantiation.
    Instantiation,
}

/// The settled outcome of a wrapper read.
enum WrapperOutcome {
    Surface(SemanticNodeId),
    Absent,
    Unsettled,
}

/// What an admission stage is given: nothing on its first stage, else the
/// answer to what its last stage named — produced inside the stage, so a
/// read's delivery is consumed where the admission's tracer and
/// observation stand, as the synchronous entry's nested read is.
pub(super) enum EntryDelivery<'d> {
    Start,
    /// The named query's read; `Pending` while it is still produced.
    Read(&'d mut dyn FnMut() -> Poll<NodeRead>),
    /// The named port, answered in place.
    Port,
    /// The named file: served in place when `serve`, else the load ended
    /// without integrating it.
    Source {
        serve: bool,
    },
    /// The named pending contributor: indexed in place when `materialize`,
    /// else its load already ended.
    Index {
        materialize: bool,
    },
}

/// Where an admission stands after a stage.
pub(super) enum EntryStep {
    /// The settled key and its prelude.
    Admitted(SemanticQueryKey, Box<CarrierNormalizationPrelude>),
    /// The admission reads this query next.
    Read(SemanticQueryKey, Box<QueryEntryBuild>),
    /// The admission reads a resolver port next
    /// ([`QueryEntryBuild::port_reads_file`]).
    Port(Box<QueryEntryBuild>),
    /// The admission reads this file's indexed input next.
    Load(Arc<str>, Box<QueryEntryBuild>),
    /// The admission indexes this pending contributor next.
    Index(Arc<str>, Box<QueryEntryBuild>),
    /// The read the last stage named is still produced: nothing ran.
    Pending(Box<QueryEntryBuild>),
}

// A suspended admission is owned state its frame or root carries to
// whichever thread resumes it, advanced by value one stage at a time.
static_assertions::assert_impl_all!(QueryEntryBuild: Send);
static_assertions::assert_not_impl_any!(QueryEntryBuild: Clone, Copy);

/// How an entry begins its admission.
pub(super) enum EntryBegin {
    /// The entry admits its key through this admission.
    Admits(Box<QueryEntryBuild>),
    /// The entry normalizes nothing: its key is admitted as it is, with no
    /// prelude.
    Plain(SemanticQueryKey),
}

/// What one stage did.
enum StageOutcome {
    Settled,
    Read(SemanticQueryKey),
    Port,
    Load(Arc<str>),
    Index(Arc<str>),
    Pending,
}

impl QueryEntryBuild {
    /// The key as the admission settled it so far.
    pub(super) fn key(&self) -> &SemanticQueryKey {
        &self.key
    }

    /// Whether the query the admission reads next is its carrier's: a read
    /// the carrier's consumers counted as their own dispatch intent.
    pub(super) fn reads_for_carrier(&self) -> bool {
        matches!(self.stage, EntryStage::Carrier { .. })
    }
}

impl<'a, C: crate::resolver_core::ResolverCapabilities> ProjectSemanticDispatch<'a, C> {
    /// The key an entry admits `key` as before its subject normalizes:
    /// sugar rewritten to the canonical path, and the operand orders the
    /// family memo identifies converged.
    ///   - `ProjectMember { base, member, mode }` rewrites to
    ///     `ProjectPath { base, path: [Member(member)], mode }` BEFORE the
    ///     key is hashed into the memo, so sugar and canonical share one
    ///     entry and one in-flight wait graph.
    ///   - `IndexedAccess { base, index, mode }` rewrites the same way to
    ///     `ProjectPath { base, path: [Index(index)], mode }`.
    ///   - `ReduceUnion` gets structural member-list canonicalisation so
    ///     `{A, B}` and `{B, A}` converge.
    ///   - Symmetric `Relate` operands get the same ordering as typed
    ///     relation callers before the family memo or wait graph sees them.
    ///
    /// Other variants key off [`SemanticNodeId`]s that are already hashed
    /// verbatim.
    pub(super) fn canonicalize_entry_key(&self, key: SemanticQueryKey) -> SemanticQueryKey {
        match key {
            SemanticQueryKey::Relate {
                mut source,
                mut target,
                relation,
                policy,
                source_freshness,
                inference_context,
                context,
            } => {
                relation.canonicalize_operands(&mut source, &mut target);
                SemanticQueryKey::Relate {
                    source,
                    target,
                    relation,
                    policy,
                    source_freshness,
                    inference_context,
                    context,
                }
            }
            SemanticQueryKey::ProjectMember { base, member, mode } => {
                SemanticQueryKey::ProjectPath {
                    base,
                    path: verter_type_expr::OwnedSlice::from(
                        vec![PathSegment::Member(PropertyKey::identifier(member))]
                            .into_boxed_slice(),
                    ),
                    context: ProjectionReductionContext::published(mode),
                }
            }
            SemanticQueryKey::IndexedAccess { base, index, mode } => {
                SemanticQueryKey::ProjectPath {
                    base,
                    path: verter_type_expr::OwnedSlice::from(
                        vec![PathSegment::Index(index)].into_boxed_slice(),
                    ),
                    context: ProjectionReductionContext::published(mode),
                }
            }
            SemanticQueryKey::ReduceUnion {
                members,
                nullability,
            } => SemanticQueryKey::ReduceUnion {
                members: canonicalize_node_list(self.graph().store(), &members),
                nullability,
            },
            other => other,
        }
    }

    /// Whether an entry of `key` admits it through a retained computation
    /// before its claim: projection sugar, or a projection whose entry
    /// normalizes its subject — a carrier, a scope-less subject read
    /// through its apparent wrapper or its apparent signatures.
    pub(super) fn admits_through_entry(&self, key: &SemanticQueryKey) -> bool {
        matches!(
            key,
            SemanticQueryKey::ProjectMember { .. } | SemanticQueryKey::IndexedAccess { .. }
        ) || self.entry_normalizes_subject(key)
    }

    /// Whether `key`'s entry normalizes its subject under a prelude tracer.
    /// The common case takes this cheap shape probe and allocates nothing.
    fn entry_normalizes_subject(&self, key: &SemanticQueryKey) -> bool {
        self.key_subject_is_carrier(key)
            || self.key_reads_apparent_wrapper(key)
            || self.key_reads_apparent_signatures(key)
    }

    /// The admission of `key`, already canonicalized
    /// ([`Self::canonicalize_entry_key`]), not yet begun, or the key itself
    /// when its entry normalizes nothing, which is admitted as it is with
    /// no prelude.
    pub(super) fn begin_query_entry(&self, key: SemanticQueryKey) -> EntryBegin {
        if !self.entry_normalizes_subject(&key) {
            return EntryBegin::Plain(key);
        }
        let source = crate::fact_signature_helpers::FactTracerBasisSource::from_ctx(self.ctx);
        EntryBegin::Admits(Box::new(QueryEntryBuild {
            key,
            stage: EntryStage::Begun,
            tracer: Some(
                crate::fact_signature_helpers::OwnedStepwiseFactTracer::fanning_out(&source),
            ),
            normalized_source: None,
            partial_reasons: crate::semantic_query::PartialReasonSet::empty(),
        }))
    }

    /// Admit `key` in place: the synchronous driver of its entry's
    /// admission, reading each query it names where the entry stands and
    /// answering each port and file at once. Returns the admitted key and
    /// its prelude.
    pub(super) fn admit_entry_in_place(
        &self,
        key: SemanticQueryKey,
    ) -> (SemanticQueryKey, CarrierNormalizationPrelude) {
        match self.begin_query_entry(key) {
            EntryBegin::Admits(entry) => self.complete_entry_in_place(entry),
            EntryBegin::Plain(key) => (key, CarrierNormalizationPrelude::none()),
        }
    }

    /// Drive a begun admission to its admitted key in place.
    pub(super) fn complete_entry_in_place(
        &self,
        entry: Box<QueryEntryBuild>,
    ) -> (SemanticQueryKey, CarrierNormalizationPrelude) {
        let mut step = self.advance_query_entry(entry, EntryDelivery::Start);
        loop {
            step = match step {
                EntryStep::Admitted(key, prelude) => return (key, *prelude),
                EntryStep::Read(key, entry) => {
                    let mut key = Some(key);
                    let counts = entry.reads_for_carrier();
                    let mut read = || {
                        let key = key.take().expect("a read is made once");
                        Poll::Ready(if counts {
                            self.carrier_read_in_place(key)
                        } else {
                            self.execute_read(key)
                        })
                    };
                    self.advance_query_entry(entry, EntryDelivery::Read(&mut read))
                }
                EntryStep::Port(entry) => self.advance_query_entry(entry, EntryDelivery::Port),
                EntryStep::Load(_, entry) => {
                    self.advance_query_entry(entry, EntryDelivery::Source { serve: true })
                }
                EntryStep::Index(_, entry) => {
                    self.advance_query_entry(entry, EntryDelivery::Index { materialize: true })
                }
                EntryStep::Pending(_) => unreachable!("an in-place read is always ready"),
            };
        }
    }

    /// A carrier's read made in place: its consumer's dispatch intent
    /// counted as the carrier's reads always counted it.
    pub(super) fn carrier_read_in_place(&self, key: SemanticQueryKey) -> NodeRead {
        if !matches!(key, SemanticQueryKey::TypeOf { .. }) {
            self.record_dispatch_intent_counters(&key);
        }
        self.execute_read(key)
    }

    /// Advance `entry` one stage with `delivery`, the answer to what its
    /// last stage named. Each stage runs under the admission's tracer; a
    /// read whose delivery is still pending runs nothing and leaves the
    /// admission where it stood.
    pub(super) fn advance_query_entry(
        &self,
        mut entry: Box<QueryEntryBuild>,
        delivery: EntryDelivery<'_>,
    ) -> EntryStep {
        let source = crate::fact_signature_helpers::FactTracerBasisSource::from_ctx(self.ctx);
        let mut tracer = entry
            .tracer
            .take()
            .expect("an admission holds its tracer between its stages");
        let outcome = {
            let _installed = tracer.install(&source);
            self.entry_stage(&mut entry, delivery)
        };
        match outcome {
            StageOutcome::Settled => {
                let finalise = tracer.finish(&source);
                let QueryEntryBuild {
                    key,
                    normalized_source,
                    partial_reasons,
                    ..
                } = *entry;
                let prelude = self.carrier_prelude(finalise, partial_reasons, normalized_source);
                EntryStep::Admitted(key, Box::new(prelude))
            }
            outcome => {
                entry.tracer = Some(tracer);
                match outcome {
                    StageOutcome::Settled => unreachable!("matched above"),
                    StageOutcome::Read(key) => EntryStep::Read(key, entry),
                    StageOutcome::Port => EntryStep::Port(entry),
                    StageOutcome::Load(file) => EntryStep::Load(file, entry),
                    StageOutcome::Index(file) => EntryStep::Index(file, entry),
                    StageOutcome::Pending => EntryStep::Pending(entry),
                }
            }
        }
    }

    /// Release what an abandoned admission holds: the carrier its
    /// resolution keeps on the dispatcher's normalization record. Its
    /// tracer and held observation are dropped unread.
    pub(super) fn abandon_query_entry(&self, entry: Box<QueryEntryBuild>) {
        if let EntryStage::Carrier { read, .. } = entry.stage {
            self.abandon_carrier_read(*read);
        }
    }

    /// One admission stage, under the admission's tracer.
    fn entry_stage(
        &self,
        entry: &mut QueryEntryBuild,
        delivery: EntryDelivery<'_>,
    ) -> StageOutcome {
        match std::mem::replace(&mut entry.stage, EntryStage::Begun) {
            EntryStage::Begun => {
                self.graph().record_admission_stage();
                // Carrier-subject normalization: when the query SUBJECT is a
                // `BareRef` / `ImportType` / `typeof` carrier, resolve its head
                // through the ONE shared resolver and REWRITE the key to the
                // resolved subject, held in a scoped observation the resolution
                // adopts once it settles.
                let Some((subject, context)) = carrier_subject(self, &entry.key) else {
                    entry.partial_reasons = self.carrier_normalization_partial_reasons(&entry.key);
                    return self.begin_wrapper(entry);
                };
                let scope = self.begin_read_observation();
                match self.begin_carrier_read(subject, context) {
                    CarrierStep::Done(resolved) => {
                        self.carrier_settled(entry, scope.finish(resolved));
                        self.begin_wrapper(entry)
                    }
                    CarrierStep::Read(key, read) => {
                        entry.stage = EntryStage::Carrier {
                            read,
                            observed: scope.finish(()),
                        };
                        StageOutcome::Read(key)
                    }
                }
            }
            EntryStage::Carrier { read, observed } => {
                let EntryDelivery::Read(deliver) = delivery else {
                    unreachable!("a carrier resolution resumes with its read")
                };
                let scope = observed.resume(self);
                let answer = match deliver() {
                    Poll::Ready(answer) => answer,
                    Poll::Pending => {
                        entry.stage = EntryStage::Carrier {
                            read,
                            observed: scope.finish(()),
                        };
                        return StageOutcome::Pending;
                    }
                };
                self.graph().record_admission_stage();
                let answer = self.carrier_resolution_of(answer);
                match self.resume_carrier_read(*read, answer) {
                    CarrierStep::Done(resolved) => {
                        self.carrier_settled(entry, scope.finish(resolved));
                        self.begin_wrapper(entry)
                    }
                    CarrierStep::Read(key, read) => {
                        entry.stage = EntryStage::Carrier {
                            read,
                            observed: scope.finish(()),
                        };
                        StageOutcome::Read(key)
                    }
                }
            }
            EntryStage::Wrapper(wrapper) => self.resume_wrapper(entry, wrapper, delivery),
        }
    }

    /// The carrier subject settled: the key reads the resolved subject, and
    /// the admission records what the resolution established.
    fn carrier_settled(
        &self,
        entry: &mut QueryEntryBuild,
        observed: ObservedRead<CarrierResolution>,
    ) {
        let source = self.adopt_carrier_observation(observed);
        set_carrier_subject(&mut entry.key, source.node);
        entry.partial_reasons = self
            .carrier_normalization_partial_reasons(&entry.key)
            .union(source.status.completeness().reasons());
        entry.normalized_source = Some(source);
    }

    /// Begin the apparent-wrapper stage: the wrapper read of a path whose
    /// first step reads a scope-less subject's wrapper, of a reducing
    /// `keyof` of a scope-less array, or of a signature read's subject
    /// taking its signatures from one — scoped to the demand's project, so
    /// its lookup's facts root the admitted entry too. A project that
    /// declares no such wrapper reads the subject as the answer every such
    /// project shares; with no demand site the key stays as it is, and its
    /// build reads the wrapper for the demand outside the shared memo.
    fn begin_wrapper(&self, entry: &mut QueryEntryBuild) -> StageOutcome {
        let (kind, subject) = match &entry.key {
            SemanticQueryKey::ProjectPath { base, .. }
                if self.key_reads_apparent_wrapper(&entry.key) =>
            {
                (WrapperKind::Path, *base)
            }
            SemanticQueryKey::KeyOf { base, .. } if self.key_reads_apparent_wrapper(&entry.key) => {
                (WrapperKind::KeyOf, *base)
            }
            SemanticQueryKey::SignaturesOfType { subject, .. } => {
                (WrapperKind::Signatures, *subject)
            }
            _ => return self.settle_entry(),
        };
        let Some(((name, args), canonical)) = self
            .apparent_wrapper_of(subject)
            .zip(self.wrapper_demand_canonical())
        else {
            return self.settle_entry();
        };
        if self
            .project_stable_key_for_canonical(canonical.as_ref())
            .is_none()
        {
            return self.settle_entry();
        }
        let (lookup, step) = GlobalLookup::begin(Arc::from(canonical.as_ref()), Arc::from(name));
        let wrapper = Box::new(WrapperRead {
            kind,
            name,
            args,
            lookup,
            awaiting: WrapperAwait::Port,
        });
        self.wrapper_lookup_step(entry, wrapper, step)
    }

    /// Resume a wrapper read with the answer to what it waited on.
    fn resume_wrapper(
        &self,
        entry: &mut QueryEntryBuild,
        mut wrapper: Box<WrapperRead>,
        delivery: EntryDelivery<'_>,
    ) -> StageOutcome {
        let step = match (
            std::mem::replace(&mut wrapper.awaiting, WrapperAwait::Port),
            delivery,
        ) {
            (WrapperAwait::Port, EntryDelivery::Port) => {
                self.graph().record_admission_stage();
                let answer = self.answer_port_in_place(&wrapper.lookup.port());
                wrapper.lookup.resume_port(answer)
            }
            (WrapperAwait::Source(file), EntryDelivery::Source { serve }) => {
                self.graph().record_admission_stage();
                let serve = if serve {
                    self.ctx.ensure_indexed_ready_serve(&file)
                } else {
                    None
                };
                wrapper.lookup.resume_source(serve)
            }
            (WrapperAwait::Index(file), EntryDelivery::Index { materialize }) => {
                self.graph().record_admission_stage();
                if materialize {
                    self.ctx.materialize_pending_contributor(&file);
                }
                wrapper.lookup.resume_source(None)
            }
            (WrapperAwait::Instantiation, EntryDelivery::Read(deliver)) => {
                let read = match deliver() {
                    Poll::Ready(read) => read,
                    Poll::Pending => {
                        wrapper.awaiting = WrapperAwait::Instantiation;
                        entry.stage = EntryStage::Wrapper(wrapper);
                        return StageOutcome::Pending;
                    }
                };
                self.graph().record_admission_stage();
                let outcome = match read.value {
                    QueryResult::Value(surface) => WrapperOutcome::Surface(surface),
                    _ => WrapperOutcome::Unsettled,
                };
                return self.wrapper_settled(entry, wrapper.kind, outcome);
            }
            _ => unreachable!("a wrapper read resumes with what it waited on"),
        };
        self.wrapper_lookup_step(entry, wrapper, step)
    }

    /// Carry a wrapper's global lookup on: its next port or file, or the
    /// declaration it found, whose instantiation is read next.
    fn wrapper_lookup_step(
        &self,
        entry: &mut QueryEntryBuild,
        mut wrapper: Box<WrapperRead>,
        step: GlobalStep,
    ) -> StageOutcome {
        let outcome = match step {
            GlobalStep::Port => {
                wrapper.awaiting = WrapperAwait::Port;
                entry.stage = EntryStage::Wrapper(wrapper);
                return StageOutcome::Port;
            }
            GlobalStep::Load(file) => {
                wrapper.awaiting = WrapperAwait::Source(Arc::clone(&file));
                entry.stage = EntryStage::Wrapper(wrapper);
                return StageOutcome::Load(file);
            }
            GlobalStep::Index(file) => {
                wrapper.awaiting = WrapperAwait::Index(Arc::clone(&file));
                entry.stage = EntryStage::Wrapper(wrapper);
                return StageOutcome::Index(file);
            }
            GlobalStep::Found(None) => WrapperOutcome::Absent,
            GlobalStep::Found(Some(declaration)) => {
                let slot = self.type_slot_for(
                    declaration.canonical_id.clone(),
                    declaration.owner,
                    Arc::<str>::from(wrapper.name),
                );
                let key =
                    SemanticQueryKey::Instantiate(crate::semantic_query::InstantiateKey::new(
                        slot,
                        Arc::from(std::mem::take(&mut wrapper.args).into_boxed_slice()),
                        self.instantiate_context_for(
                            declaration.canonical_id.as_ref(),
                            wrapper.kind.context(),
                        ),
                    ));
                wrapper.awaiting = WrapperAwait::Instantiation;
                entry.stage = EntryStage::Wrapper(wrapper);
                return StageOutcome::Read(key);
            }
        };
        self.wrapper_settled(entry, wrapper.kind, outcome)
    }

    /// The wrapper read settled: the key reads its subject through it.
    fn wrapper_settled(
        &self,
        entry: &mut QueryEntryBuild,
        kind: WrapperKind,
        outcome: WrapperOutcome,
    ) -> StageOutcome {
        let replacement = match (kind, outcome) {
            (_, WrapperOutcome::Unsettled) => None,
            (_, WrapperOutcome::Surface(surface)) => Some(surface),
            (WrapperKind::Path | WrapperKind::KeyOf, WrapperOutcome::Absent) => {
                Some(self.opaque(QueryError::Miss))
            }
            // A project declaring no such wrapper gives the subject no
            // apparent signature — the answer `never` has, which every
            // project shares.
            (WrapperKind::Signatures, WrapperOutcome::Absent) => Some(self.graph().intern_node(
                SemanticNodeData::Primitive(crate::semantic_query::PrimitiveKind::Never),
            )),
        };
        if let Some(subject) = replacement {
            match &mut entry.key {
                SemanticQueryKey::ProjectPath { base, .. }
                | SemanticQueryKey::KeyOf { base, .. } => *base = subject,
                SemanticQueryKey::SignaturesOfType { subject: held, .. } => *held = subject,
                _ => unreachable!("only a path, a keyof or a signature read reads a wrapper"),
            }
        }
        self.settle_entry()
    }

    /// The admission's last stage, still under its tracer.
    fn settle_entry(&self) -> StageOutcome {
        // Test-only: force a fenced (ReturnOnly) serve observation onto the
        // prelude tracer so the suppress wiring is exercisable without a
        // superseded-artifact fixture. Zero-cost when unset.
        #[cfg(any(test, feature = "test-support"))]
        if self
            .binding
            .observers
            .forcing
            .carrier_normalization_force_fence_for_tests
            .load(std::sync::atomic::Ordering::Relaxed)
        {
            crate::fact_tracing::note_non_cacheable_read_fan_out(
                verter_session_query::facts::reuse::NonCacheableReadReason::FencedServe,
            );
        }
        StageOutcome::Settled
    }
}

/// The subject `key`'s entry resolves when it is a carrier, and the context
/// the resolution runs under: the base of a path, a member or an index read,
/// of a `keyof`, or the source of a mapped type. A `typeof` carrier base of
/// a NON-EMPTY path is left for the path walker's per-segment `typeof` arm,
/// which runs the path-precise resolve -> project (`Navigate` intermediate
/// hop) -> apply chain; pre-resolving it here would over-resolve the
/// internal carrier path under the caller's outer mode and bypass the
/// walker's intermediate-hop demotion. The EMPTY-path case (a `typeof
/// config` macro payload) has no walker segment to fire the arm, so it MUST
/// resolve here. Every other key carries no subject that could be a carrier:
/// nested carriers inside intersection / union / heritage surfaces re-enter
/// normalization through the shallow-synthesis worklist's re-dispatch.
pub(super) fn carrier_subject<C: crate::resolver_core::ResolverCapabilities>(
    dispatch: &ProjectSemanticDispatch<'_, C>,
    key: &SemanticQueryKey,
) -> Option<(SemanticNodeId, ProjectionReductionContext)> {
    match key {
        SemanticQueryKey::ProjectPath {
            base,
            path,
            context,
        } => (path.is_empty()
            || !dispatch
                .graph()
                .node_data(*base)
                .is_some_and(|data| data.typeof_head().is_some()))
        .then_some((*base, *context)),
        SemanticQueryKey::ProjectMember { base, mode, .. }
        | SemanticQueryKey::IndexedAccess { base, mode, .. } => {
            Some((*base, ProjectionReductionContext::published(*mode)))
        }
        SemanticQueryKey::KeyOf { base, context } => Some((*base, *context)),
        SemanticQueryKey::MappedType {
            source, context, ..
        } => Some((*source, *context)),
        _ => None,
    }
}

/// Make `key` read `subject` in place of the subject [`carrier_subject`]
/// named.
pub(super) fn set_carrier_subject(key: &mut SemanticQueryKey, subject: SemanticNodeId) {
    match key {
        SemanticQueryKey::ProjectPath { base, .. }
        | SemanticQueryKey::ProjectMember { base, .. }
        | SemanticQueryKey::IndexedAccess { base, .. }
        | SemanticQueryKey::KeyOf { base, .. } => *base = subject,
        SemanticQueryKey::MappedType { source, .. } => *source = subject,
        _ => {}
    }
}
