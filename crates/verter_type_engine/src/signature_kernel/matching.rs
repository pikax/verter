//! Record-level signature matching as owned machines: one signature
//! comparison (the checker's `compareSignaturesIdentical`), the ordered scan
//! of a list for the first candidate identical to a signature, and the
//! composition of a union's or an intersection's candidate lists.
//!
//! A machine never calls its holder. A step answers every question the
//! graph can tell at rest ([`DiscoveryTypes::identical_at_rest`]) and
//! returns the first one it cannot as a [`TypeRequest`], standing where it
//! asked. The holder answers it with the next step's [`TypeAnswer`],
//! wherever it stands: in place, or after waiting on another task. The
//! machine resumes at the position it asked from, every comparison it
//! decided kept, so nothing is compared twice because a question waited.
//!
//! Candidate precedence is authored order and comparisons are decided in
//! order, so an undecided earlier comparison never lets a later candidate
//! win.

use crate::semantic_query::{IncompleteReason, SemanticNodeId};

use super::discovery::{
    combine_union_parameters, composite_candidate, edges_for, is_mixin_constructor, load,
    source_token, undecided, DiscoveryError, DiscoveryTypes, Loaded, MatchOptions, Res,
};
use super::lifetime::EpochBinding;
use super::positional::{PositionalMode, TypeAt};
use super::records::{
    PredicateEffect, SignatureCandidate, SignatureKind, SignatureResultRecipe, TypeToken,
};

/// The result a candidate's recipe denotes: its return type and its
/// predicate effect, the answer to [`TypeRequest::ForcedResult`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ForcedResult {
    pub return_type: TypeToken,
    pub effects: Option<PredicateEffect>,
}

/// What record-level matching asks of the holder of the type graph.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TypeRequest {
    /// Whether `a` and `b` are identical under the binder correspondence
    /// the machine stands in ([`SignatureComparison::correspondence`],
    /// [`SignatureComposition::correspondence`]).
    Identical { a: TypeToken, b: TypeToken },
    /// The result `candidate`'s recipe denotes. Answering it forces a body
    /// recipe; matching asks for it only where its comparison includes
    /// results.
    ForcedResult(SignatureCandidate),
}

/// The holder's answer to the request the previous step returned.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TypeAnswer {
    Identical(bool),
    ForcedResult(ForcedResult),
}

/// Where a matching machine stands after a step: its output, or the
/// question the next step is given the answer to.
#[derive(Debug, PartialEq, Eq)]
pub enum KernelStep<T> {
    Done(T),
    Ask(TypeRequest),
}

/// The answer this step was given to an identity question, else the
/// graph's at rest; `None` is a question for the holder.
fn identity(
    types: &dyn DiscoveryTypes,
    answer: &mut Option<TypeAnswer>,
    a: TypeToken,
    b: TypeToken,
    corr: &[(SemanticNodeId, SemanticNodeId)],
) -> Option<bool> {
    match answer.take() {
        Some(TypeAnswer::Identical(same)) => Some(same),
        Some(TypeAnswer::ForcedResult(_)) => {
            unreachable!("an identity question is answered with an identity")
        }
        None => types.identical_at_rest(a, b, corr),
    }
}

/// A comparison's decision, counted where it is made.
fn decided(types: &dyn DiscoveryTypes, same: bool) -> Res<KernelStep<bool>> {
    types.comparison_decided();
    Ok(KernelStep::Done(same))
}

/// Binder-space equality of two loaded signatures: the same count, and
/// every constraint and default identical under the positional
/// correspondence it builds. Absence is not `unknown`: an omitted default
/// and an authored `unknown` default behave differently for omitted type
/// arguments.
#[derive(Debug, Default)]
struct BinderMatch {
    /// `(source binder, target binder)` in ordinal order, once `paired`.
    corr: Vec<(SemanticNodeId, SemanticNodeId)>,
    /// The binder being compared.
    index: u32,
    /// Its default is compared; `false`: its constraint.
    default: bool,
    paired: bool,
}

impl BinderMatch {
    /// Ready for another pair of signatures, keeping the correspondence's
    /// capacity.
    fn reset(&mut self) {
        self.corr.clear();
        self.index = 0;
        self.default = false;
        self.paired = false;
    }

    /// Carry the binder match of `s` against `t` on from where it stands.
    fn step(
        &mut self,
        types: &dyn DiscoveryTypes,
        s: &Loaded<'_>,
        t: &Loaded<'_>,
        answer: &mut Option<TypeAnswer>,
    ) -> Res<KernelStep<bool>> {
        if s.space.binders.len() != t.space.binders.len() {
            return Ok(KernelStep::Done(false));
        }
        if !self.paired {
            let Some(source_binders) = s.binder_nodes() else {
                return undecided();
            };
            let Some(target_binders) = t.binder_nodes() else {
                return undecided();
            };
            self.corr.clear();
            self.corr.extend(
                source_binders
                    .iter()
                    .zip(target_binders.iter())
                    .map(|((_, a), (_, b))| (*a, *b)),
            );
            self.paired = true;
        }
        while let (Some(a), Some(b)) = (
            s.space.binders.get(self.index as usize),
            t.space.binders.get(self.index as usize),
        ) {
            let (x, y) = if self.default {
                (a.default, b.default)
            } else {
                (a.constraint, b.constraint)
            };
            match (x, y) {
                (None, None) => {}
                (Some(x), Some(y)) => {
                    let Some(same) = identity(types, answer, x, y, &self.corr) else {
                        return Ok(KernelStep::Ask(TypeRequest::Identical { a: x, b: y }));
                    };
                    if !same {
                        return Ok(KernelStep::Done(false));
                    }
                }
                _ => return Ok(KernelStep::Done(false)),
            }
            if self.default {
                self.default = false;
                self.index += 1;
            } else {
                self.default = true;
            }
        }
        Ok(KernelStep::Done(true))
    }
}

/// Where a comparison stands: the question it asks next.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ComparisonAt {
    Arity,
    Binders,
    Receiver,
    /// Target position `pos`; `part` 0 is its type or a rest run's element,
    /// `part - 1` indexes the run's tail.
    Position {
        pos: u32,
        part: u32,
    },
    SourceResult,
    TargetResult,
    Results,
}

/// One `compareSignaturesIdentical` of `source` against `target`, between
/// two of its questions: arity (or partial arity), binder constraints and
/// defaults under a positional binder correspondence, the receiver, every
/// target position, and, unless ignored, the results.
#[derive(Debug)]
pub struct SignatureComparison {
    source: SignatureCandidate,
    target: SignatureCandidate,
    options: MatchOptions,
    binders: BinderMatch,
    at: ComparisonAt,
    source_result: Option<ForcedResult>,
    target_result: Option<ForcedResult>,
}

/// What one compared position asks.
enum SlotPair {
    /// The position's shapes differ: no type is compared.
    Differ,
    /// Whether `a` and `b` are identical; `last` when it is the position's
    /// final part.
    Compare {
        a: TypeToken,
        b: TypeToken,
        last: bool,
    },
}

/// Part `part` of one compared position: a run's element, then its tail
/// entries; a generic rest at the same index; or one type each, `any` for a
/// position past a signature's parameters, with optionality that adds
/// `undefined` alike on both sides.
fn slot_pair(types: &dyn DiscoveryTypes, a: TypeAt<'_>, b: TypeAt<'_>, part: u32) -> Res<SlotPair> {
    let any = types
        .any()
        .ok_or(DiscoveryError::Incomplete(IncompleteReason::Budget))?;
    let one = |ty: TypeAt<'_>| match ty {
        TypeAt::Absent => Some((any, false)),
        TypeAt::One(s) => Some((s.ty, s.optionality.includes_undefined)),
        _ => None,
    };
    Ok(match (a, b) {
        (
            TypeAt::Run {
                element: ea,
                tail: ta,
            },
            TypeAt::Run {
                element: eb,
                tail: tb,
            },
        ) => {
            if ta.len() != tb.len() {
                SlotPair::Differ
            } else {
                let part = part as usize;
                let (a, b) = if part == 0 {
                    (ea, eb)
                } else {
                    (ta[part - 1].ty, tb[part - 1].ty)
                };
                SlotPair::Compare {
                    a,
                    b,
                    last: part == ta.len(),
                }
            }
        }
        (
            TypeAt::GenericRest {
                rest: ra,
                index: ia,
            },
            TypeAt::GenericRest {
                rest: rb,
                index: ib,
            },
        ) => {
            if ia == ib {
                SlotPair::Compare {
                    a: ra,
                    b: rb,
                    last: true,
                }
            } else {
                SlotPair::Differ
            }
        }
        (a, b) => match (one(a), one(b)) {
            (Some((ta, ua)), Some((tb, ub))) if ua == ub => SlotPair::Compare {
                a: ta,
                b: tb,
                last: true,
            },
            _ => SlotPair::Differ,
        },
    })
}

impl SignatureComparison {
    /// A comparison of `source` against `target`, before its first question.
    #[must_use]
    pub fn new(
        source: SignatureCandidate,
        target: SignatureCandidate,
        options: MatchOptions,
    ) -> Self {
        Self {
            source,
            target,
            options,
            binders: BinderMatch::default(),
            at: ComparisonAt::Arity,
            source_result: None,
            target_result: None,
        }
    }

    /// Begin this comparison again for another pair, keeping the
    /// correspondence's capacity.
    fn begin(
        &mut self,
        source: SignatureCandidate,
        target: SignatureCandidate,
        options: MatchOptions,
    ) {
        self.source = source;
        self.target = target;
        self.options = options;
        self.binders.reset();
        self.at = ComparisonAt::Arity;
        self.source_result = None;
        self.target_result = None;
    }

    /// The binder correspondence an identity question of this comparison
    /// is asked under: `(source binder, target binder)` pairs.
    #[must_use]
    pub fn correspondence(&self) -> &[(SemanticNodeId, SemanticNodeId)] {
        &self.binders.corr
    }

    /// Carry the comparison on from where it stands, given `answer`, the
    /// answer to the request the previous step returned: its decision, or
    /// the next question for the holder. Both candidates' records are read
    /// in `binding`, the epoch they were published in. An `Err` leaves the
    /// comparison dead.
    pub fn step(
        &mut self,
        binding: &EpochBinding,
        types: &dyn DiscoveryTypes,
        mut answer: Option<TypeAnswer>,
    ) -> Res<KernelStep<bool>> {
        if self.at == ComparisonAt::Arity && self.source.signature == self.target.signature {
            return decided(types, true);
        }
        let view = binding.view();
        let s = load(view, self.source)?;
        let t = load(view, self.target)?;
        let sp = s.positional(types);
        let tp = t.positional(types);
        loop {
            match self.at {
                ComparisonAt::Arity => {
                    let mode = PositionalMode::Comparison;
                    let arity_same = sp.parameter_count() == tp.parameter_count()
                        && sp.effective_minimum(mode) == tp.effective_minimum(mode)
                        && sp.has_rest() == tp.has_rest();
                    if !arity_same
                        && !(self.options.partial_match
                            && sp.effective_minimum(mode) <= tp.effective_minimum(mode))
                    {
                        return decided(types, false);
                    }
                    self.at = ComparisonAt::Binders;
                }
                ComparisonAt::Binders => match self.binders.step(types, &s, &t, &mut answer)? {
                    KernelStep::Ask(request) => return Ok(KernelStep::Ask(request)),
                    KernelStep::Done(false) => return decided(types, false),
                    KernelStep::Done(true) => self.at = ComparisonAt::Receiver,
                },
                ComparisonAt::Receiver => {
                    if !self.options.ignore_this_types {
                        match (s.receiver, t.receiver) {
                            (None, None) => {}
                            (Some(a), Some(b)) => {
                                let Some(same) =
                                    identity(types, &mut answer, a.ty, b.ty, &self.binders.corr)
                                else {
                                    return Ok(KernelStep::Ask(TypeRequest::Identical {
                                        a: a.ty,
                                        b: b.ty,
                                    }));
                                };
                                if !same {
                                    return decided(types, false);
                                }
                            }
                            _ => return decided(types, false),
                        }
                    }
                    self.at = ComparisonAt::Position { pos: 0, part: 0 };
                }
                ComparisonAt::Position { pos, part } => {
                    if pos as usize == tp.parameter_count() {
                        let same_body = matches!(
                            (s.recipe, t.recipe),
                            (
                                SignatureResultRecipe::Body { return_obligation_key: a },
                                SignatureResultRecipe::Body { return_obligation_key: b },
                            ) if a == b && self.binders.corr.is_empty()
                        );
                        if self.options.ignore_return_types || same_body {
                            return decided(types, true);
                        }
                        self.at = ComparisonAt::SourceResult;
                        continue;
                    }
                    match slot_pair(
                        types,
                        sp.type_at(pos as usize),
                        tp.type_at(pos as usize),
                        part,
                    )? {
                        SlotPair::Differ => return decided(types, false),
                        SlotPair::Compare { a, b, last } => {
                            let Some(same) = identity(types, &mut answer, a, b, &self.binders.corr)
                            else {
                                return Ok(KernelStep::Ask(TypeRequest::Identical { a, b }));
                            };
                            if !same {
                                return decided(types, false);
                            }
                            self.at = if last {
                                ComparisonAt::Position {
                                    pos: pos + 1,
                                    part: 0,
                                }
                            } else {
                                ComparisonAt::Position {
                                    pos,
                                    part: part + 1,
                                }
                            };
                        }
                    }
                }
                ComparisonAt::SourceResult => match answer.take() {
                    Some(TypeAnswer::ForcedResult(result)) => {
                        self.source_result = Some(result);
                        self.at = ComparisonAt::TargetResult;
                    }
                    Some(TypeAnswer::Identical(_)) => {
                        unreachable!("a result question is answered with a result")
                    }
                    None => return Ok(KernelStep::Ask(TypeRequest::ForcedResult(self.source))),
                },
                ComparisonAt::TargetResult => match answer.take() {
                    Some(TypeAnswer::ForcedResult(result)) => {
                        self.target_result = Some(result);
                        self.at = ComparisonAt::Results;
                    }
                    Some(TypeAnswer::Identical(_)) => {
                        unreachable!("a result question is answered with a result")
                    }
                    None => return Ok(KernelStep::Ask(TypeRequest::ForcedResult(self.target))),
                },
                ComparisonAt::Results => {
                    let (Some(a), Some(b)) = (self.source_result, self.target_result) else {
                        unreachable!("both results are read before they are compared")
                    };
                    // A predicate replaces the return in the comparison, as
                    // the checker's `compareTypePredicatesIdentical` does: two
                    // predicates of the same kind about the same subject with
                    // identical targets, or none on either side and identical
                    // returns.
                    let identical = match (a.effects, b.effects) {
                        (None, None) => {
                            let Some(same) = identity(
                                types,
                                &mut answer,
                                a.return_type,
                                b.return_type,
                                &self.binders.corr,
                            ) else {
                                return Ok(KernelStep::Ask(TypeRequest::Identical {
                                    a: a.return_type,
                                    b: b.return_type,
                                }));
                            };
                            same
                        }
                        (Some(pa), Some(pb)) => {
                            if pa.subject != pb.subject || pa.asserts != pb.asserts {
                                false
                            } else {
                                match (pa.ty, pb.ty) {
                                    (None, None) => true,
                                    (Some(x), Some(y)) => {
                                        let Some(same) =
                                            identity(types, &mut answer, x, y, &self.binders.corr)
                                        else {
                                            return Ok(KernelStep::Ask(TypeRequest::Identical {
                                                a: x,
                                                b: y,
                                            }));
                                        };
                                        same
                                    }
                                    _ => false,
                                }
                            }
                        }
                        _ => false,
                    };
                    return decided(types, identical);
                }
            }
        }
    }
}

/// One ordered scan of a list for the first candidate identical to a
/// signature: each list candidate is the comparison's source and the
/// signature its target. A scan that finds nothing is ready for another.
#[derive(Debug, Default)]
struct MatchScan {
    /// The list candidate being compared.
    at: u32,
    /// `comparison` stands in the comparison of the candidate at `at`.
    comparing: bool,
    /// The comparison, kept between candidates for its correspondence's
    /// capacity.
    comparison: Option<SignatureComparison>,
}

impl MatchScan {
    /// Carry the scan of `list` for `sig` under `options` on from where it
    /// stands. The caller passes the same list, signature and options until
    /// the scan is done.
    fn step(
        &mut self,
        binding: &EpochBinding,
        types: &dyn DiscoveryTypes,
        list: &[SignatureCandidate],
        sig: SignatureCandidate,
        options: MatchOptions,
        mut answer: Option<TypeAnswer>,
    ) -> Res<KernelStep<Option<SignatureCandidate>>> {
        loop {
            let Some(&candidate) = list.get(self.at as usize) else {
                self.at = 0;
                return Ok(KernelStep::Done(None));
            };
            if !self.comparing {
                match &mut self.comparison {
                    Some(comparison) => comparison.begin(candidate, sig, options),
                    None => {
                        self.comparison = Some(SignatureComparison::new(candidate, sig, options));
                    }
                }
                self.comparing = true;
            }
            let comparison = self
                .comparison
                .as_mut()
                .expect("a scan compares the candidate it began");
            match comparison.step(binding, types, answer.take())? {
                KernelStep::Ask(request) => return Ok(KernelStep::Ask(request)),
                KernelStep::Done(same) => {
                    self.comparing = false;
                    if same {
                        self.at = 0;
                        return Ok(KernelStep::Done(Some(candidate)));
                    }
                    self.at += 1;
                }
            }
        }
    }

    fn correspondence(&self) -> &[(SemanticNodeId, SemanticNodeId)] {
        match &self.comparison {
            Some(comparison) => comparison.correspondence(),
            None => &[],
        }
    }
}

/// A union's or an intersection's candidate lists being composed, between
/// two of the questions its comparisons ask.
#[derive(Debug)]
pub struct SignatureComposition(Composition);

#[derive(Debug)]
enum Composition {
    Union(UnionComposition),
    Intersection(IntersectionComposition),
}

/// Union call or construct signatures over the per-arm candidate lists
/// (arms in `VerterStableV1` order, each list in authored order).
///
/// Phase 1, common matches: a signature from any arm that has a match in
/// every other arm (exact for generic signatures, exact then partial with
/// returns ignored otherwise) becomes one union candidate whose
/// constituents are the matches. Phase 2, restricted synthesis: only when
/// phase 1 found nothing and at most one arm has several signatures, the
/// single signatures combine with each of that arm's signatures. There is
/// never a Cartesian product of overload choices.
#[derive(Debug)]
struct UnionComposition {
    lists: Vec<Vec<SignatureCandidate>>,
    /// The arms with several signatures.
    over_one: Vec<usize>,
    result: Vec<SignatureCandidate>,
    /// The signatures phase 1 represented, in order.
    representatives: Vec<SignatureCandidate>,
    /// The signature phase 1 stands at: `lists[list_index][sig_index]`.
    list_index: u32,
    sig_index: u32,
    stage: UnionStage,
    scan: MatchScan,
    /// The signature's matches so far, `(arm, candidate)`.
    matches: Vec<(u32, SignatureCandidate)>,
    union_restarted_by_mutation: bool,
}

#[derive(Debug)]
enum UnionStage {
    /// Is the signature already represented?
    Represented,
    /// A generic signature's exact match in list `other`.
    Exact { other: u32 },
    /// A non-generic signature's match in list `other`, returns ignored:
    /// exact first, then (`partial`) partial.
    Loose { other: u32, partial: bool },
    /// Restricted synthesis over the master list.
    Synthesis(Box<UnionSynthesis>),
}

/// The restricted synthesis of a union: each signature of the master list
/// combined, list by list, with every other list's first signature.
#[derive(Debug)]
struct UnionSynthesis {
    master: u32,
    /// The master signature being combined.
    master_at: u32,
    /// The list being combined into `acc`.
    other: u32,
    /// The combination so far; `None` before the master signature begins.
    acc: Option<SignatureCandidate>,
    constituents: Vec<(u32, SignatureCandidate)>,
    out: Vec<SignatureCandidate>,
    /// Two generic signatures' binder spaces, matched before they combine.
    binders: BinderMatch,
}

impl UnionComposition {
    fn signature(&self) -> SignatureCandidate {
        self.lists[self.list_index as usize][self.sig_index as usize]
    }

    /// Phase 1 moves to the next signature.
    fn advance(&mut self) {
        self.sig_index += 1;
        self.stage = UnionStage::Represented;
    }

    /// The signature matched in every arm: it represents them, as itself
    /// when it is its only match, else as a composite candidate whose
    /// constituents are the matches.
    fn accept(&mut self, binding: &EpochBinding) -> Res<()> {
        let sig = self.signature();
        self.representatives.push(sig);
        if self.matches.len() == 1 {
            self.result.push(sig);
        } else {
            let view = binding.view();
            let loaded = load(view, sig)?;
            let sequence = edges_for(binding, view, sig, &self.matches)?;
            let representative = source_token(binding, sig)?;
            let candidate = composite_candidate(
                binding,
                view,
                sig,
                view.template(loaded.descriptor.template)?.input_shape,
                SignatureResultRecipe::UnionCommon {
                    representative,
                    constituents: sequence,
                },
                self.result.len() as u32,
            )?;
            self.result.push(candidate);
        }
        self.advance();
        Ok(())
    }

    fn step(
        &mut self,
        binding: &EpochBinding,
        types: &dyn DiscoveryTypes,
        mut answer: Option<TypeAnswer>,
    ) -> Res<KernelStep<Vec<SignatureCandidate>>> {
        if self.lists.is_empty() || self.lists.iter().any(Vec::is_empty) {
            return Ok(KernelStep::Done(Vec::new()));
        }
        if self.lists.len() == 1 {
            return Ok(KernelStep::Done(std::mem::take(&mut self.lists[0])));
        }
        if !self.union_restarted_by_mutation
            && !self.representatives.is_empty()
            && matches!(answer, Some(TypeAnswer::ForcedResult(_)))
        {
            // MUTATION_RESTART_UNION_AFTER_DELIVERY
            self.union_restarted_by_mutation = true;
            answer = None;
            self.result.clear();
            self.representatives.clear();
            self.list_index = 0;
            self.sig_index = 0;
            self.stage = UnionStage::Represented;
            self.scan = MatchScan::default();
            self.matches.clear();
        }
        loop {
            match self.stage {
                UnionStage::Synthesis(_) => return self.synthesize(binding, types, answer),
                UnionStage::Represented => {
                    if self.list_index as usize == self.lists.len() {
                        if !self.result.is_empty() || self.over_one.len() > 1 {
                            return Ok(KernelStep::Done(std::mem::take(&mut self.result)));
                        }
                        let master = self.over_one.first().copied().unwrap_or(0) as u32;
                        self.stage = UnionStage::Synthesis(Box::new(UnionSynthesis {
                            master,
                            master_at: 0,
                            other: 0,
                            acc: None,
                            constituents: Vec::new(),
                            out: Vec::new(),
                            binders: BinderMatch::default(),
                        }));
                        continue;
                    }
                    if self.sig_index as usize == self.lists[self.list_index as usize].len() {
                        self.list_index += 1;
                        self.sig_index = 0;
                        continue;
                    }
                    let sig = self.signature();
                    match self.scan.step(
                        binding,
                        types,
                        &self.representatives,
                        sig,
                        MatchOptions::EXACT_IGNORING_RETURNS,
                        answer.take(),
                    )? {
                        KernelStep::Ask(request) => return Ok(KernelStep::Ask(request)),
                        KernelStep::Done(Some(_)) => self.advance(),
                        KernelStep::Done(None) => {
                            let generic = !load(binding.view(), sig)?.space.binders.is_empty();
                            self.matches.clear();
                            if !generic {
                                self.stage = UnionStage::Loose {
                                    other: 0,
                                    partial: false,
                                };
                            } else if self.list_index > 0 {
                                // Generic signatures require an exact match,
                                // and only the first list may start one.
                                self.advance();
                            } else {
                                self.matches.push((0, sig));
                                self.stage = UnionStage::Exact { other: 1 };
                            }
                        }
                    }
                }
                UnionStage::Exact { other } => {
                    if other as usize == self.lists.len() {
                        self.accept(binding)?;
                        continue;
                    }
                    let sig = self.signature();
                    match self.scan.step(
                        binding,
                        types,
                        &self.lists[other as usize],
                        sig,
                        MatchOptions::EXACT,
                        answer.take(),
                    )? {
                        KernelStep::Ask(request) => return Ok(KernelStep::Ask(request)),
                        KernelStep::Done(Some(found)) => {
                            self.matches.push((other, found));
                            self.stage = UnionStage::Exact { other: other + 1 };
                        }
                        KernelStep::Done(None) => self.advance(),
                    }
                }
                UnionStage::Loose { other, partial } => {
                    if other as usize == self.lists.len() {
                        self.accept(binding)?;
                        continue;
                    }
                    let sig = self.signature();
                    // Every arm keeps its own edge, even when several arms
                    // settle to the same callable.
                    if other == self.list_index {
                        self.matches.push((other, sig));
                        self.stage = UnionStage::Loose {
                            other: other + 1,
                            partial: false,
                        };
                        continue;
                    }
                    let options = if partial {
                        MatchOptions::PARTIAL_IGNORING_RETURNS
                    } else {
                        MatchOptions::EXACT_IGNORING_RETURNS
                    };
                    match self.scan.step(
                        binding,
                        types,
                        &self.lists[other as usize],
                        sig,
                        options,
                        answer.take(),
                    )? {
                        KernelStep::Ask(request) => return Ok(KernelStep::Ask(request)),
                        KernelStep::Done(Some(found)) => {
                            self.matches.push((other, found));
                            self.stage = UnionStage::Loose {
                                other: other + 1,
                                partial: false,
                            };
                        }
                        KernelStep::Done(None) if !partial => {
                            self.stage = UnionStage::Loose {
                                other,
                                partial: true,
                            };
                        }
                        KernelStep::Done(None) => self.advance(),
                    }
                }
            }
        }
    }

    /// Carry the restricted synthesis on from where it stands
    /// (`combineSignaturesOfUnionMembers` over each master signature).
    fn synthesize(
        &mut self,
        binding: &EpochBinding,
        types: &dyn DiscoveryTypes,
        mut answer: Option<TypeAnswer>,
    ) -> Res<KernelStep<Vec<SignatureCandidate>>> {
        let UnionStage::Synthesis(synthesis) = &mut self.stage else {
            unreachable!("a union synthesizes in its synthesis stage")
        };
        let lists = &self.lists;
        let master = synthesis.master as usize;
        loop {
            let Some(&m) = lists[master].get(synthesis.master_at as usize) else {
                return Ok(KernelStep::Done(std::mem::take(&mut synthesis.out)));
            };
            let acc = match synthesis.acc {
                Some(acc) => acc,
                None => {
                    synthesis.acc = Some(m);
                    synthesis.constituents.clear();
                    synthesis.constituents.push((synthesis.master, m));
                    synthesis.other = 0;
                    synthesis.binders.reset();
                    m
                }
            };
            let other = synthesis.other as usize;
            if other == lists.len() {
                synthesis.finish_master(binding, m)?;
                continue;
            }
            if other == master {
                synthesis.other += 1;
                continue;
            }
            let Some(&right) = lists[other].first() else {
                synthesis.next_master();
                continue;
            };
            let view = binding.view();
            let l = load(view, acc)?;
            let r = load(view, right)?;
            // Both generic: the binder spaces must be identical, and the
            // right side's binders map onto the left's.
            let map_right: Vec<(SemanticNodeId, SemanticNodeId)> =
                if !l.space.binders.is_empty() && !r.space.binders.is_empty() {
                    match synthesis.binders.step(types, &l, &r, &mut answer)? {
                        KernelStep::Ask(request) => return Ok(KernelStep::Ask(request)),
                        KernelStep::Done(false) => {
                            synthesis.next_master();
                            continue;
                        }
                        KernelStep::Done(true) => synthesis
                            .binders
                            .corr
                            .iter()
                            .map(|(a, b)| (*b, *a))
                            .collect(),
                    }
                } else {
                    Vec::new()
                };
            let next = combine_union_parameters(binding, types, &l, &r, acc, &map_right)?;
            synthesis.acc = Some(next);
            synthesis.constituents.push((other as u32, right));
            synthesis.other += 1;
            synthesis.binders.reset();
        }
    }
}

impl UnionSynthesis {
    /// The master signature `m` combined with every other list: itself
    /// when nothing combined into it, else a synthesized candidate over
    /// the combination's parameters whose constituents are the combined
    /// signatures.
    fn finish_master(&mut self, binding: &EpochBinding, m: SignatureCandidate) -> Res<()> {
        if self.constituents.len() == 1 {
            self.out.push(m);
        } else {
            let acc = self.acc.expect("a master is finished with its combination");
            let view = binding.view();
            let acc_loaded = load(view, acc)?;
            let sequence = edges_for(binding, view, m, &self.constituents)?;
            let master = source_token(binding, m)?;
            let candidate = composite_candidate(
                binding,
                view,
                m,
                view.template(acc_loaded.descriptor.template)?.input_shape,
                SignatureResultRecipe::UnionSynthesized {
                    master,
                    constituents: sequence,
                },
                self.out.len() as u32,
            )?;
            self.out.push(candidate);
        }
        self.next_master();
        Ok(())
    }

    /// The next master signature; the one being combined contributes
    /// nothing unless it was finished.
    fn next_master(&mut self) {
        self.master_at += 1;
        self.acc = None;
    }
}

/// Intersection signatures for one kind over the per-member candidate lists
/// in authored order.
///
/// Call signatures concatenate, dropping only a signature IDENTICAL to one
/// already present, result included: the checker's `appendSignatures`,
/// which compares with `ignoreReturnTypes: false`. Two signatures that
/// differ only in what they return are different overloads of the
/// intersection and are both kept: `(() => A) & (() => B)` carries both, so
/// conditional inference (`ReturnType`, `InstanceType`), which reads the
/// LAST signature, answers `B`. Signatures identical including the result
/// still collapse onto the first. Union synthesis is the opposite rule: it
/// matches arms by parameters and unions their results.
///
/// Construct signatures follow TypeScript's mixin rule
/// (`resolveIntersectionTypeMembers`): a member that is a mixin constructor
/// (`new (...args: any[]) => X`) contributes NO construct signature of its
/// own; its instance type is mixed into every other member's construct
/// result instead (`IntersectionConstruct`, the results intersected in
/// member order). When every constructor member is a mixin, the first one
/// is not counted as a mixin, so it keeps its signature and the others mix
/// into it: `typeof CtorB & typeof MixA` constructs `B & A` from `CtorB`'s
/// parameters, and two mixins construct from the first one's.
#[derive(Debug)]
struct IntersectionComposition {
    members: Vec<Vec<SignatureCandidate>>,
    mixin_flags: Vec<bool>,
    mixin_count: u32,
    out: Vec<SignatureCandidate>,
    /// The member being appended.
    index: u32,
    /// Its signatures, composed with the mixins, once `mapped_ready`.
    mapped: Vec<SignatureCandidate>,
    mapped_ready: bool,
    /// The signature of `mapped` being appended.
    sig_index: u32,
    scan: MatchScan,
    intersection_restarted_by_mutation: bool,
}

impl IntersectionComposition {
    fn new(
        binding: &EpochBinding,
        types: &dyn DiscoveryTypes,
        kind: SignatureKind,
        members: Vec<Vec<SignatureCandidate>>,
    ) -> Res<Self> {
        let view = binding.view();
        let mut mixin_flags = vec![false; members.len()];
        if kind == SignatureKind::Construct {
            for (flag, list) in mixin_flags.iter_mut().zip(&members) {
                *flag = is_mixin_constructor(view, types, list)?;
            }
            let constructor_count = members.iter().filter(|list| !list.is_empty()).count();
            let mixins = mixin_flags.iter().filter(|f| **f).count();
            if constructor_count > 0 && constructor_count == mixins {
                if let Some(first) = mixin_flags.iter().position(|f| *f) {
                    mixin_flags[first] = false;
                }
            }
        }
        let mixin_count = mixin_flags.iter().filter(|f| **f).count() as u32;
        Ok(Self {
            members,
            mixin_flags,
            mixin_count,
            out: Vec::new(),
            index: 0,
            mapped: Vec::new(),
            mapped_ready: false,
            sig_index: 0,
            scan: MatchScan::default(),
            intersection_restarted_by_mutation: false,
        })
    }

    /// Member `index`'s signatures, each composed with every mixin member's
    /// constructor into an `IntersectionConstruct` candidate.
    fn composed(&self, binding: &EpochBinding, index: usize) -> Res<Vec<SignatureCandidate>> {
        let view = binding.view();
        let list = &self.members[index];
        let mut composed = Vec::with_capacity(list.len());
        for &sig in list {
            let mut constituents: Vec<(u32, SignatureCandidate)> = Vec::new();
            for (i, other) in self.members.iter().enumerate() {
                if i == index {
                    constituents.push((i as u32, sig));
                } else if self.mixin_flags[i] {
                    constituents.push((i as u32, other[0]));
                }
            }
            let loaded = load(view, sig)?;
            let sequence = edges_for(binding, view, sig, &constituents)?;
            let base = source_token(binding, sig)?;
            composed.push(composite_candidate(
                binding,
                view,
                sig,
                view.template(loaded.descriptor.template)?.input_shape,
                SignatureResultRecipe::IntersectionConstruct {
                    base_constructor: base,
                    mixins: sequence,
                },
                composed.len() as u32,
            )?);
        }
        Ok(composed)
    }

    fn step(
        &mut self,
        binding: &EpochBinding,
        types: &dyn DiscoveryTypes,
        mut answer: Option<TypeAnswer>,
    ) -> Res<KernelStep<Vec<SignatureCandidate>>> {
        if !self.intersection_restarted_by_mutation
            && self.out.len() >= 2
            && matches!(answer, Some(TypeAnswer::ForcedResult(_)))
        {
            // MUTATION_RESTART_INTERSECTION_AFTER_DELIVERY
            self.intersection_restarted_by_mutation = true;
            answer = None;
            self.out.clear();
            self.index = 0;
            self.mapped_ready = false;
            self.sig_index = 0;
            self.scan = MatchScan::default();
        }
        loop {
            let index = self.index as usize;
            if index == self.members.len() {
                return Ok(KernelStep::Done(std::mem::take(&mut self.out)));
            }
            if !self.mapped_ready {
                if self.members[index].is_empty() || self.mixin_flags[index] {
                    self.index += 1;
                    continue;
                }
                let mapped = if self.mixin_count > 0 {
                    self.composed(binding, index)?
                } else {
                    self.members[index].clone()
                };
                self.mapped = mapped;
                self.sig_index = 0;
                self.mapped_ready = true;
            }
            let Some(&sig) = self.mapped.get(self.sig_index as usize) else {
                self.index += 1;
                self.mapped_ready = false;
                continue;
            };
            match self.scan.step(
                binding,
                types,
                &self.out,
                sig,
                MatchOptions::EXACT,
                answer.take(),
            )? {
                KernelStep::Ask(request) => return Ok(KernelStep::Ask(request)),
                KernelStep::Done(found) => {
                    if found.is_none() {
                        self.out.push(sig);
                    }
                    self.sig_index += 1;
                }
            }
        }
    }
}

impl SignatureComposition {
    /// The union of the per-arm candidate lists `lists`, before its first
    /// comparison.
    #[must_use]
    pub fn union(lists: Vec<Vec<SignatureCandidate>>) -> Self {
        let over_one = lists
            .iter()
            .enumerate()
            .filter(|(_, list)| list.len() > 1)
            .map(|(i, _)| i)
            .collect();
        Self(Composition::Union(UnionComposition {
            lists,
            over_one,
            result: Vec::new(),
            representatives: Vec::new(),
            list_index: 0,
            sig_index: 0,
            stage: UnionStage::Represented,
            scan: MatchScan::default(),
            matches: Vec::new(),
            union_restarted_by_mutation: false,
        }))
    }

    /// The intersection of the per-member candidate lists `members` for
    /// signatures of `kind`, its mixin members read from the records in
    /// `binding`.
    pub fn intersection(
        binding: &EpochBinding,
        types: &dyn DiscoveryTypes,
        kind: SignatureKind,
        members: Vec<Vec<SignatureCandidate>>,
    ) -> Res<Self> {
        Ok(Self(Composition::Intersection(
            IntersectionComposition::new(binding, types, kind, members)?,
        )))
    }

    /// Carry the composition on from where it stands, given `answer`, the
    /// answer to the request the previous step returned: the composed
    /// candidates, or the next question for the holder. Every record is
    /// read and interned in `binding`, the same epoch at every step. An
    /// `Err` leaves the composition dead.
    pub fn step(
        &mut self,
        binding: &EpochBinding,
        types: &dyn DiscoveryTypes,
        answer: Option<TypeAnswer>,
    ) -> Res<KernelStep<Vec<SignatureCandidate>>> {
        match &mut self.0 {
            Composition::Union(union) => union.step(binding, types, answer),
            Composition::Intersection(intersection) => intersection.step(binding, types, answer),
        }
    }

    /// The binder correspondence the identity question the composition
    /// stands at is asked under: the active comparison's, or during a
    /// union's synthesis the binder match's.
    #[must_use]
    pub fn correspondence(&self) -> &[(SemanticNodeId, SemanticNodeId)] {
        match &self.0 {
            Composition::Union(union) => match &union.stage {
                UnionStage::Synthesis(synthesis) => &synthesis.binders.corr,
                _ => union.scan.correspondence(),
            },
            Composition::Intersection(intersection) => intersection.scan.correspondence(),
        }
    }
}
