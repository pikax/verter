//! Whether a read of a value yields a WIDENING literal, as an owned walk
//! that stops at each semantic read it makes.
//!
//! The value's declared type is the fresh literal type a `const` takes from
//! its literal initializer (`const c = 1`), directly or through the values
//! it copies (`const d = c`): the walk follows those copies one declaration
//! at a time, with the set of value roots it already followed, so a copy
//! cycle (`const a = b; const b = a`, the checker's TS7022) reads no
//! literal. A `const` initialized by a call reads as fresh as the call's
//! result is (`const m = f(1)` over `f<T>(x: T): T`): its expression source
//! evaluates under one optional probe, and an indexed initializer advances
//! the shared indexed run ([`FlowReadDriver::advance_indexed_value`]),
//! which stops at the reads it requests — a leaf's `typeof` (`f`, `typeof
//! f`) and a lazy declared return's instantiation. The initializer is read
//! while an expression is checked, as the inline freshness probe reads it:
//! the reads the run requests, their answers and the calls it settles are
//! all made at the expression checking site (a refused tuple splice in a
//! declared return is the expression's TS2800, never the type node's
//! TS2799).
//!
//! [`ProjectSemanticDispatch::begin_value_widening`] starts the walk: each
//! step either answers or holds the walk as a [`ValueWideningRead`] waiting
//! on the read it requested ([`ValueWideningRead::waiting_on`]).
//! [`ProjectSemanticDispatch::resume_value_widening`] answers that read in
//! place, inside the probe's resumed observation, and carries the walk on;
//! [`ProjectSemanticDispatch::value_read_widens`] is that drive in a loop.
//! So a call that is not fresh drops what its reads observed.
//!
//! A holder may not answer the read itself yet: a delivered answer is
//! consumed outside the walk's probe, so a call that is not fresh could
//! not drop what that read observed (an operation-budget partial would
//! stay folded into the enclosing build). Until a delivery carries its own
//! observation into the probe, a holder's delivery
//! ([`ProjectSemanticDispatch::deliver_value_widening`]), and a delivery
//! to one walk of a [`WideningsRead`] list
//! ([`ProjectSemanticDispatch::deliver_value_widenings`]), is refused with
//! [`WideningDeliveryRefusal::NeedsObservedDelivery`], the walk left
//! waiting on its read.
//!
//! Every other read the walk makes is still made in place: the host's
//! indexed, prepared-declaration and route reads; the signature read a
//! namespace import's `default` member makes; an initializer's routed
//! function or flow return; and the call resolution and explicit type
//! arguments of an initializer's calls.
//!
//! This module is a child of the flow evaluator's module only to name its
//! read types and indexed-value run. It adds no stepping or answering of
//! its own: its reads are the indexed run's
//! ([`FlowReadDriver::advance_indexed_value`]), and it answers in place only
//! through the flow evaluator's existing in-place answerer. The flow
//! cutover may relocate those in-place answers like any other flow
//! caller's.

use rustc_hash::FxHashSet;
use verter_session_query::declarations::ValueDeclKind;
use verter_session_query::type_solver::PreparedValueDecl;
use verter_type_expr::facts::DeclaredLiteralFreshness;
use verter_type_expr::OwnedStr;

use super::{
    ExpressionSourceRoute, ExpressionSourceRun, ExpressionSourceState, FlowDemandDriver,
    FlowReadDriver, IndexedValueRun, ProjectSemanticDispatch, ReadDelivery, ReadStep, ReadWait,
};
use crate::semantic_query::{BinderScopeId, ScopeId, ValueRootKey};

/// The value roots, each with its remaining path, a walk has followed.
type FollowedRoots = FxHashSet<(ValueRootKey, Vec<OwnedStr>)>;

/// A value read's widening walk, held across the read it waits on.
pub(in crate::project_semantic_dispatch) struct ValueWideningRead {
    /// The value roots the walk has followed: a copy cycle reads nothing.
    visited: FollowedRoots,
    cursor: WideningCursor,
    /// The read the walk waits on, answered only where the walk resumes.
    waiting: Option<ReadWait>,
}

impl ValueWideningRead {
    /// The read the walk waits on.
    #[allow(dead_code)] // the step surface a holder of the reads drives
    pub(in crate::project_semantic_dispatch) fn waiting_on(&self) -> &ReadWait {
        self.waiting
            .as_ref()
            .expect("a held widening walk waits on a read")
    }
}

static_assertions::assert_impl_all!(ValueWideningRead: Send);
static_assertions::assert_not_impl_any!(ValueWideningRead: Clone, Copy);

/// Where a widening walk stands.
enum WideningCursor {
    /// The read of `root`'s `path` names the next declaration the walk
    /// reads.
    Follow {
        root: ValueRootKey,
        path: Vec<OwnedStr>,
    },
    /// A `const` initialized by a call: its expression source evaluating,
    /// to tell whether its value is a fresh literal.
    Fresh {
        prepared: verter_retention::heap_size::OwnedValue<PreparedValueDecl>,
        run: ExpressionSourceRun,
    },
}

/// What a widening walk does after a step.
pub(in crate::project_semantic_dispatch) enum WideningStep {
    /// Whether the read yields a widening literal.
    Done(bool),
    /// The walk waits on the read it holds
    /// ([`ValueWideningRead::waiting_on`]).
    Read(Box<ValueWideningRead>),
}

/// Why a widening walk refuses an answer its holder delivers.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(in crate::project_semantic_dispatch) enum WideningDeliveryRefusal {
    /// A delivered answer is consumed outside the walk's freshness probe,
    /// so a call that is not fresh could not drop what that read observed:
    /// a holder's delivery needs an observed delivery, one that carries its
    /// read's observation into the probe.
    NeedsObservedDelivery,
}

/// A holder's delivery a widening walk refused
/// ([`ProjectSemanticDispatch::deliver_value_widening`]).
pub(in crate::project_semantic_dispatch) struct RefusedWideningDelivery {
    pub(in crate::project_semantic_dispatch) refusal: WideningDeliveryRefusal,
    /// The walk, still waiting on its read: resume it in place or abandon
    /// it.
    pub(in crate::project_semantic_dispatch) read: Box<ValueWideningRead>,
}

/// What one declaration of a widening walk decides.
enum WideningHop {
    Answer(bool),
    /// The declaration copies this value.
    Copy(ValueRootKey, Vec<OwnedStr>),
    /// The declaration is a `const` its call initializes, in this scope.
    FreshCall(
        verter_retention::heap_size::OwnedValue<PreparedValueDecl>,
        verter_type_expr::TopLevelOwnerId,
    ),
}

/// The widening walks of several value reads, each waiting on at most one
/// read ([`ProjectSemanticDispatch::begin_value_widenings`]).
pub(in crate::project_semantic_dispatch) struct WideningsRead {
    slots: Vec<WideningSlot>,
}

enum WideningSlot {
    Done(bool),
    Waiting(Box<ValueWideningRead>),
}

impl WideningsRead {
    /// The reads the walks wait on, each with the index of its walk's
    /// path.
    #[allow(dead_code)] // the step surface a holder of the reads drives
    pub(in crate::project_semantic_dispatch) fn requests(
        &self,
    ) -> impl Iterator<Item = (usize, &ReadWait)> + '_ {
        self.slots
            .iter()
            .enumerate()
            .filter_map(|(index, slot)| match slot {
                WideningSlot::Waiting(read) => Some((index, read.waiting_on())),
                WideningSlot::Done(_) => None,
            })
    }

    /// Whether each path's read yields a widening literal, in the order
    /// the paths were given. No walk may wait on a read.
    #[allow(dead_code)] // the step surface a holder of the reads drives
    pub(in crate::project_semantic_dispatch) fn finish(self) -> Vec<bool> {
        self.slots
            .into_iter()
            .map(|slot| match slot {
                WideningSlot::Done(widens) => widens,
                WideningSlot::Waiting(_) => {
                    panic!("a widening list finishes once no walk waits on a read")
                }
            })
            .collect()
    }
}

impl<C: crate::resolver_core::ResolverCapabilities> ProjectSemanticDispatch<'_, C> {
    /// Whether a read of the value `path` names, in `canonical`'s `owner`
    /// scope, yields a WIDENING literal: the value's declared type is the
    /// fresh literal type a `const` takes from its literal initializer
    /// (`const c = 1`), directly or through the values it copies
    /// (`const d = c`). The checker widens such a read wherever it widens a
    /// bare literal — a return, a mutable binding, an object or array
    /// member, a call argument. The path resolves as a `typeof` reference to
    /// it does: a namespace member, an import, a global.
    ///
    /// The walk's in-place drive: each read it stops at is answered where
    /// the walk resumes ([`Self::resume_value_widening`]).
    pub fn value_read_widens(
        &self,
        canonical: &str,
        owner: verter_type_expr::TopLevelOwnerId,
        path: &[String],
    ) -> bool {
        let mut step = self.begin_value_widening(canonical, owner, path);
        loop {
            match step {
                WideningStep::Done(widens) => return widens,
                WideningStep::Read(read) => step = self.resume_value_widening(read),
            }
        }
    }

    /// Begin [`Self::value_read_widens`]'s walk for the value `path` names
    /// in `canonical`'s `owner` scope, carried on as far as it goes before
    /// its first read.
    pub(in crate::project_semantic_dispatch) fn begin_value_widening(
        &self,
        canonical: &str,
        owner: verter_type_expr::TopLevelOwnerId,
        path: &[String],
    ) -> WideningStep {
        let Some((root, rest)) = path.split_first() else {
            return WideningStep::Done(false);
        };
        let root = ValueRootKey {
            scope: ScopeId {
                canonical_id: OwnedStr::from(canonical),
                owner,
                local_scope: None,
                binder_scope_id: BinderScopeId::file_scope(owner),
            },
            name: OwnedStr::from(root.as_str()),
        };
        let path = rest
            .iter()
            .map(|segment| OwnedStr::from(segment.as_str()))
            .collect();
        self.advance_value_widening(Box::new(ValueWideningRead {
            visited: FollowedRoots::default(),
            cursor: WideningCursor::Follow { root, path },
            waiting: None,
        }))
    }

    /// Carry a widening walk on, answering the read it waits on in place:
    /// inside its probe's resumed observation, at the expression checking
    /// site the read was requested at.
    pub(in crate::project_semantic_dispatch) fn resume_value_widening(
        &self,
        read: Box<ValueWideningRead>,
    ) -> WideningStep {
        assert!(
            read.waiting.is_some(),
            "a widening walk resumes only from the read it waits on"
        );
        self.advance_value_widening(read)
    }

    /// A holder's answer to the read a widening walk waits on: refused
    /// ([`WideningDeliveryRefusal::NeedsObservedDelivery`]), the walk handed
    /// back still waiting. A delivered answer was consumed outside the
    /// walk's probe, so a call that is not fresh could not drop what that
    /// read observed; [`Self::resume_value_widening`] answers it in place.
    #[allow(dead_code)] // the step surface a holder of the reads drives
    pub(in crate::project_semantic_dispatch) fn deliver_value_widening(
        &self,
        read: Box<ValueWideningRead>,
        _delivery: ReadDelivery,
    ) -> Result<WideningStep, RefusedWideningDelivery> {
        Err(RefusedWideningDelivery {
            refusal: WideningDeliveryRefusal::NeedsObservedDelivery,
            read,
        })
    }

    /// Drop a widening walk its holder no longer needs: what its
    /// initializer's evaluation observed is dropped with it, as a value
    /// that is never selected is.
    #[allow(dead_code)] // the step surface a holder of the reads drives
    pub(in crate::project_semantic_dispatch) fn abandon_value_widening(
        &self,
        read: Box<ValueWideningRead>,
    ) {
        drop(read);
    }

    /// Begin the widening walks of the value reads `paths` name in
    /// `canonical`'s `owner` scope, one per path, each carried on as far
    /// as it goes before its first read.
    #[allow(dead_code)] // the step surface a holder of the reads drives
    pub(in crate::project_semantic_dispatch) fn begin_value_widenings<'p>(
        &self,
        canonical: &str,
        owner: verter_type_expr::TopLevelOwnerId,
        paths: impl IntoIterator<Item = &'p [String]>,
    ) -> WideningsRead {
        WideningsRead {
            slots: paths
                .into_iter()
                .map(|path| WideningSlot::from(self.begin_value_widening(canonical, owner, path)))
                .collect(),
        }
    }

    /// A holder's answer to the read [`WideningsRead::requests`] lists for
    /// path `index`'s walk: refused as [`Self::deliver_value_widening`]
    /// refuses it, the walk left waiting in the list.
    #[allow(dead_code)] // the step surface a holder of the reads drives
    pub(in crate::project_semantic_dispatch) fn deliver_value_widenings(
        &self,
        widenings: &mut WideningsRead,
        index: usize,
        delivery: ReadDelivery,
    ) -> Result<(), WideningDeliveryRefusal> {
        let slot = &mut widenings.slots[index];
        let WideningSlot::Waiting(read) = std::mem::replace(slot, WideningSlot::Done(false)) else {
            panic!("a widening list delivers only a read it requested");
        };
        match self.deliver_value_widening(read, delivery) {
            Ok(step) => {
                *slot = WideningSlot::from(step);
                Ok(())
            }
            Err(RefusedWideningDelivery { refusal, read }) => {
                *slot = WideningSlot::Waiting(read);
                Err(refusal)
            }
        }
    }

    /// Drop widening walks their holder no longer needs
    /// ([`Self::abandon_value_widening`]).
    #[allow(dead_code)] // the step surface a holder of the reads drives
    pub(in crate::project_semantic_dispatch) fn abandon_value_widenings(
        &self,
        widenings: WideningsRead,
    ) {
        for slot in widenings.slots {
            if let WideningSlot::Waiting(read) = slot {
                self.abandon_value_widening(read);
            }
        }
    }

    /// The one step of a widening walk: follow declarations until the walk
    /// answers or its initializer's evaluation waits on a read. The read
    /// the walk waits on, if any, is answered in place where it resumes.
    fn advance_value_widening(&self, mut read: Box<ValueWideningRead>) -> WideningStep {
        loop {
            let ValueWideningRead {
                visited,
                cursor,
                waiting,
            } = &mut *read;
            let hop = match cursor {
                WideningCursor::Follow { root, path } => {
                    assert!(
                        waiting.is_none(),
                        "a walk following a copy waits on no read"
                    );
                    self.declared_literal_widening_hop(root, path, visited)
                }
                WideningCursor::Fresh { prepared, run } => {
                    let source = prepared
                        .type_annotation
                        .expression_source
                        .as_ref()
                        .expect("a call-initialized const evaluates its expression source");
                    match self.advance_expression_source_freshness(source, run, waiting.take()) {
                        ReadStep::Ready(fresh) => return WideningStep::Done(fresh),
                        ReadStep::Wait(wait) => {
                            read.waiting = Some(wait);
                            return WideningStep::Read(read);
                        }
                    }
                }
            };
            read.cursor = match hop {
                WideningHop::Answer(widens) => return WideningStep::Done(widens),
                WideningHop::Copy(root, path) => WideningCursor::Follow { root, path },
                WideningHop::FreshCall(prepared, owner) => WideningCursor::Fresh {
                    prepared,
                    run: ExpressionSourceRun::new(owner),
                },
            };
        }
    }

    /// Whether an expression source's value is a FRESH literal
    /// ([`Self::semantic_expression_source_is_fresh`]), carried on to its
    /// answer or to the next read its indexed run requests. The source
    /// evaluates under one optional probe held across the reads: adopted
    /// when the value is fresh, dropped otherwise. `answer` is the read the
    /// run waits on, answered here in place.
    fn advance_expression_source_freshness(
        &self,
        source: &verter_type_expr::facts::SemanticExpressionSource,
        run: &mut ExpressionSourceRun,
        mut answer: Option<ReadWait>,
    ) -> ReadStep<bool> {
        let scope = match run.observed.take() {
            Some(observed) => observed.resume(self),
            None => self.begin_read_observation(),
        };
        let owner = run.owner;
        let value = loop {
            let indexed = match &mut run.state {
                ExpressionSourceState::Unrouted => {
                    match self.route_semantic_expression_source(source) {
                        ExpressionSourceRoute::Evaluated(value) => break value,
                        ExpressionSourceRoute::Indexed {
                            serve,
                            canonical,
                            expression,
                        } => ExpressionSourceState::Indexed {
                            _serve: serve,
                            canonical,
                            expression,
                            run: IndexedValueRun::new(false),
                            pending_before: self.pending_obligations(),
                        },
                    }
                }
                ExpressionSourceState::Indexed {
                    canonical,
                    expression,
                    run: indexed,
                    pending_before,
                    ..
                } => {
                    let step = {
                        // The initializer is read while its expression is
                        // checked, as the inline probe's indexed run reads
                        // it: the reads the run requests, their in-place
                        // answers and the calls it settles all belong to
                        // the expression site (a refused splice in a
                        // declared return is TS2800, not TS2799).
                        let _expression_site = self.checking_site_scope(
                            crate::semantic_query::checker_policy::TupleSplicePosition::Expression,
                        );
                        let delivery = answer
                            .take()
                            .map(|wait| self.read_in_place(wait, canonical));
                        self.advance_indexed_value(canonical, owner, expression, indexed, delivery)
                    };
                    match step {
                        ReadStep::Wait(wait) => {
                            run.observed = Some(scope.finish(()));
                            return ReadStep::Wait(wait);
                        }
                        ReadStep::Ready(value) => {
                            break self.settle_indexed_value_for_type(*pending_before, value)
                        }
                    }
                }
            };
            run.state = indexed;
        };
        let read = scope.finish(value.filter(|value| value.fresh));
        ReadStep::Ready(
            if read.value().is_some() {
                read.adopt(self).ok().flatten()
            } else {
                read.discard().ok().flatten()
            }
            .is_some(),
        )
    }

    /// What the declaration a widening walk reads next decides, from the
    /// resolved value root `value_root`'s `path`. `visited` holds the
    /// references the walk has already followed.
    fn declared_literal_widening_hop(
        &self,
        value_root: &ValueRootKey,
        path: &[OwnedStr],
        visited: &mut FollowedRoots,
    ) -> WideningHop {
        let Some((identity, path)) = self.value_path_declaration(value_root, path, visited) else {
            return WideningHop::Answer(false);
        };
        // A member of a value (`obj.a`) is not a declaration of its own,
        // except a class's static member (`C.s`) and an enum's member
        // (`E.A`).
        let static_member = match path.as_slice() {
            [] => None,
            [member] => Some(member),
            _ => return WideningHop::Answer(false),
        };
        let Some((declaring_canonical, declaring_owner, _, prepared)) = self
            .effective_prepared_value_decl(
                &identity.canonical_id,
                identity.owner,
                &identity.symbol_name,
            )
        else {
            return WideningHop::Answer(false);
        };
        if let Some(member) = static_member {
            // Every enum member declares the fresh literal type of its
            // value, so a read of one widens as a bare literal does.
            if prepared.kind == ValueDeclKind::Enum {
                return WideningHop::Answer(prepared.enum_members.as_ref().is_some_and(
                    |members| {
                        members
                            .members
                            .iter()
                            .any(|entry| entry.name.as_str() == member.as_ref())
                    },
                ));
            }
            return WideningHop::Answer(matches!(
                &prepared.type_annotation.literal_freshness,
                DeclaredLiteralFreshness::WideningMembers { statics, .. }
                    if statics.iter().any(|widening| widening.as_str() == member.as_ref())
            ));
        }
        let copied_root = match &prepared.type_annotation.literal_freshness {
            // A `const` initialized by a call reads as fresh as the call's
            // result is (`const m = f(1)` over `f<T>(x: T): T`).
            DeclaredLiteralFreshness::Regular if prepared.kind == ValueDeclKind::Const => {
                if prepared.type_annotation.expression_source.is_none() {
                    return WideningHop::Answer(false);
                }
                None
            }
            DeclaredLiteralFreshness::Regular
            | DeclaredLiteralFreshness::WideningMembers { .. }
            | DeclaredLiteralFreshness::WideningNullish => return WideningHop::Answer(false),
            DeclaredLiteralFreshness::Widening => return WideningHop::Answer(true),
            DeclaredLiteralFreshness::Follows(copied) => {
                // A copy written in a namespace body reads that body's own
                // scope first, which holds members no qualified name
                // records; the file scope would name another value.
                if identity.symbol_name.contains('.') {
                    return WideningHop::Answer(false);
                }
                let Some((first, rest)) = copied.split_first() else {
                    return WideningHop::Answer(false);
                };
                Some((
                    ValueRootKey {
                        scope: ScopeId {
                            canonical_id: declaring_canonical,
                            owner: declaring_owner,
                            local_scope: None,
                            binder_scope_id: BinderScopeId::file_scope(declaring_owner),
                        },
                        name: OwnedStr::from(first.as_str()),
                    },
                    rest.iter()
                        .map(|segment| OwnedStr::from(segment.as_str()))
                        .collect(),
                ))
            }
        };
        match copied_root {
            Some((root, rest)) => WideningHop::Copy(root, rest),
            None => WideningHop::FreshCall(prepared, declaring_owner),
        }
    }

    /// The walk of [`Self::value_read_widens`] stepped over the value
    /// `path` names: resumed in place read by read, offered a holder's
    /// delivery at its first read and then resumed in place, and as a
    /// widening list of the path twice offered a holder's delivery for
    /// each read it waits on.
    #[cfg(any(test, feature = "test-support"))]
    #[doc(hidden)]
    pub fn step_value_widening_for_tests(
        &self,
        canonical: &str,
        owner: verter_type_expr::TopLevelOwnerId,
        path: &[String],
    ) -> ValueWideningTrace {
        let mut in_place_reads = Vec::new();
        let mut in_place_sites = Vec::new();
        let mut in_place_recoveries = Vec::new();
        let mut step = self.begin_value_widening(canonical, owner, path);
        let in_place = loop {
            match step {
                WideningStep::Done(widens) => break widens,
                WideningStep::Read(read) => {
                    let wait = read.waiting_on();
                    in_place_reads.push(traced_read(wait));
                    in_place_sites.push(traced_site(wait));
                    in_place_recoveries.push(self.traced_recovery(wait, canonical));
                    step = self.resume_value_widening(read);
                }
            }
        };
        let (holder_delivery, mut step) = match self.begin_value_widening(canonical, owner, path) {
            WideningStep::Done(widens) => (None, WideningStep::Done(widens)),
            WideningStep::Read(read) => {
                match self.deliver_value_widening(read, super::ReadDelivery::None) {
                    Ok(step) => (Some("accepted"), step),
                    Err(RefusedWideningDelivery { refusal, read }) => {
                        (Some(traced_refusal(refusal)), WideningStep::Read(read))
                    }
                }
            }
        };
        let refused_then_in_place = loop {
            match step {
                WideningStep::Done(widens) => break widens,
                WideningStep::Read(read) => step = self.resume_value_widening(read),
            }
        };
        let mut widenings = self.begin_value_widenings(canonical, owner, [path, path]);
        let listed_reads: Vec<_> = widenings
            .requests()
            .map(|(index, wait)| {
                let (key, shape) = traced_read(wait);
                (index, key, shape)
            })
            .collect();
        let listed_deliveries = listed_reads
            .iter()
            .map(|(index, ..)| {
                let delivered =
                    self.deliver_value_widenings(&mut widenings, *index, super::ReadDelivery::None);
                (
                    *index,
                    delivered.map_or_else(traced_refusal, |()| "accepted"),
                )
            })
            .collect();
        let listed_waiting = widenings.requests().count();
        let listed = if listed_waiting == 0 {
            Some(widenings.finish())
        } else {
            self.abandon_value_widenings(widenings);
            None
        };
        let abandoned = self.begin_value_widenings(canonical, owner, [path]);
        let abandoned_waiting = abandoned.requests().count();
        self.abandon_value_widenings(abandoned);
        let value_read_widens = self.value_read_widens(canonical, owner, path);
        // The inline freshness probe the walk replaces, over a `const`
        // whose expression source the path names directly.
        let inline = match path {
            [name] => self
                .effective_prepared_value_decl(canonical, owner, name)
                .and_then(|(_, declaring_owner, _, prepared)| {
                    let source = prepared.type_annotation.expression_source.as_ref()?;
                    let fresh = self.semantic_expression_source_is_fresh(source, declaring_owner);
                    let recovery = self
                        .probe_optional(|| {
                            self.semantic_expression_source_value(source, declaring_owner)
                        })
                        .and_then(|value| self.traced_node_recovery(value.node));
                    Some((fresh, recovery))
                }),
            _ => None,
        };
        ValueWideningTrace {
            value_read_widens,
            inline,
            in_place_sites,
            in_place_recoveries,
            in_place,
            in_place_reads,
            holder_delivery,
            refused_then_in_place,
            listed,
            listed_reads,
            listed_deliveries,
            listed_waiting,
            abandoned_waiting,
        }
    }
}

#[cfg(any(test, feature = "test-support"))]
impl<C: crate::resolver_core::ResolverCapabilities> ProjectSemanticDispatch<'_, C> {
    /// The checker recovery a whole read answers under the checking site
    /// it was requested at, as the walk's in-place answer reads it.
    fn traced_recovery(
        &self,
        wait: &ReadWait,
        canonical: &str,
    ) -> Option<crate::semantic_query::CheckerDiagnostic> {
        let super::ReadShape::Whole = wait.shape else {
            return None;
        };
        let _site = self.checking_site_scope(wait.checking_site);
        let again = ReadWait {
            key: wait.key.clone(),
            nesting: wait.nesting,
            shape: wait.shape,
            checking_site: wait.checking_site,
            lexical: wait.lexical,
            observed: false,
            origin: wait.origin,
        };
        match self.read_in_place(again, canonical).into_value().value {
            crate::semantic_query::QueryResult::Value(
                crate::semantic_query::SemanticQueryValue::TypeNode(node),
            ) => self.traced_node_recovery(node),
            _ => None,
        }
    }

    /// The checker recovery `node` is, if it is one.
    fn traced_node_recovery(
        &self,
        node: crate::semantic_query::SemanticNodeId,
    ) -> Option<crate::semantic_query::CheckerDiagnostic> {
        match self.graph().node_data(node).as_deref() {
            Some(crate::semantic_query::SemanticNodeData::Opaque(
                crate::semantic_query::QueryError::CheckerRecovery { diagnostic, .. },
            )) => Some(*diagnostic),
            _ => None,
        }
    }
}

impl From<WideningStep> for WideningSlot {
    fn from(step: WideningStep) -> Self {
        match step {
            WideningStep::Done(widens) => Self::Done(widens),
            WideningStep::Read(read) => Self::Waiting(read),
        }
    }
}

/// A read's key and the shape of its answer, `"node"`, `"whole"` or
/// `"staged"`.
#[cfg(any(test, feature = "test-support"))]
fn traced_read(wait: &ReadWait) -> (crate::semantic_query::SemanticQueryKey, &'static str) {
    let shape = match wait.shape {
        super::ReadShape::Node => "node",
        super::ReadShape::Whole => "whole",
        super::ReadShape::Staged => "staged",
    };
    ((*wait.key).clone(), shape)
}

/// A refused holder delivery, by its refusal.
#[cfg(any(test, feature = "test-support"))]
fn traced_refusal(refusal: WideningDeliveryRefusal) -> &'static str {
    match refusal {
        WideningDeliveryRefusal::NeedsObservedDelivery => "needs an observed delivery",
    }
}

/// The checking site a read was requested at, `"type"` or `"expression"`.
#[cfg(any(test, feature = "test-support"))]
fn traced_site(wait: &ReadWait) -> &'static str {
    match wait.checking_site {
        crate::semantic_query::checker_policy::TupleSplicePosition::Type => "type",
        crate::semantic_query::checker_policy::TupleSplicePosition::Expression => "expression",
    }
}

/// [`ProjectSemanticDispatch::step_value_widening_for_tests`]'s record.
#[cfg(any(test, feature = "test-support"))]
#[doc(hidden)]
pub struct ValueWideningTrace {
    /// [`ProjectSemanticDispatch::value_read_widens`]'s answer.
    pub value_read_widens: bool,
    /// For a path naming a `const` with an expression source: the inline
    /// freshness probe's answer, and the checker recovery its value is.
    pub inline: Option<(bool, Option<crate::semantic_query::CheckerDiagnostic>)>,
    /// The checking site each in-place read was requested at
    /// (`"type"` or `"expression"`).
    pub in_place_sites: Vec<&'static str>,
    /// The checker recovery each in-place whole read answers under that
    /// site; `None` for a node read or an answer that is no recovery.
    pub in_place_recoveries: Vec<Option<crate::semantic_query::CheckerDiagnostic>>,
    /// The walk's answer when each read is answered where it resumes.
    pub in_place: bool,
    /// The reads that walk yielded, in order, each with its answer's
    /// shape (`"node"` or `"whole"`).
    pub in_place_reads: Vec<(crate::semantic_query::SemanticQueryKey, &'static str)>,
    /// What a holder's delivery at the walk's first read met: its refusal
    /// (`"needs an observed delivery"`), or `"accepted"`; `None` when the
    /// walk reads nothing.
    pub holder_delivery: Option<&'static str>,
    /// The answer of that walk, handed back and resumed in place.
    pub refused_then_in_place: bool,
    /// The answers of a widening list over the path twice, when no walk
    /// of it waits on a read.
    pub listed: Option<Vec<bool>>,
    /// The reads that list's walks first wait on, each with its walk's
    /// index.
    pub listed_reads: Vec<(usize, crate::semantic_query::SemanticQueryKey, &'static str)>,
    /// What a holder's delivery to each waiting walk of the list met, as
    /// `holder_delivery`.
    pub listed_deliveries: Vec<(usize, &'static str)>,
    /// How many of the list's walks still wait on a read after those
    /// deliveries.
    pub listed_waiting: usize,
    /// How many reads a list over the path waited on when it was dropped.
    pub abandoned_waiting: usize,
}
