//! Guard chains over one union subject, narrowed in work linear in the
//! chain.
//!
//! A chain of guards over one union — `if (x instanceof C0) …; if (x
//! instanceof C1) …`, `"k0" in x`, a user type predicate, a discriminant
//! equality `route.name === "r0"` — decides every guard against the arms
//! the earlier guards left. Deciding each guard by visiting each remaining
//! arm, and building the remaining union on each excluding edge, costs the
//! chain the sum of the arms each guard passes over: quadratic in the
//! chain, however much of the per-arm work is memoized.
//!
//! Instead the union's arms are indexed once per evaluation
//! ([`ArmIndex`]) by the fact each guard family decides an arm by: its
//! class heritage, its own property keys, its string-literal tag. A guard
//! looks up the arms whose verdict its operand can change — and decides
//! them by the family's own per-arm rule — while every other arm takes the
//! verdict the index proves for it without a visit. An excluding edge keeps
//! the arms it did not exclude as a [`FlowArmRemainder`] over the index,
//! built into a union only for a read that needs the type itself.
//!
//! Every fast path answers exactly what the general narrow answers over the
//! remaining arms, and steps aside (`None`) wherever it cannot prove that:
//! the general narrow then runs over the built remainder.

use std::cell::{OnceCell, RefCell};
use std::rc::Rc;
use std::sync::Arc;

use rustc_hash::{FxHashMap, FxHashSet};

use super::super::flow_products::{FlowArmRemainder, FlowNarrowed};
use super::{
    FlowDemandDriver, FlowEvaluator, GuardNarrowing, HeritageReading, InArmPresence,
    InstanceofHeritage, PredicateNarrowConsumption,
};
use crate::semantic_query::{
    FlowReturnDegradation, LiteralValue, PrimitiveKind, SemanticNodeData, SemanticNodeId,
};
use verter_session_query::flow::policy::FlowGap;
use verter_session_query::flow::slice::SliceNarrowSubject;

#[cfg(any(test, feature = "test-support"))]
thread_local! {
    /// Arms this thread indexed for guard narrowing ([`ArmIndex`]): one per
    /// arm per family index prepared; test-only.
    static INDEX_PREPARATION: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
    /// Arms this thread's indexed guards visited: every arm a guard decided
    /// by its family's per-arm rule; test-only.
    static INDEXED_ARM_VISITS: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
}

fn note_index_preparation(arms: usize) {
    #[cfg(any(test, feature = "test-support"))]
    INDEX_PREPARATION.with(|count| count.set(count.get() + arms));
    let _ = arms;
}

fn note_arm_visits(arms: usize) {
    #[cfg(any(test, feature = "test-support"))]
    INDEXED_ARM_VISITS.with(|count| count.set(count.get() + arms));
    let _ = arms;
}

/// Arms this thread indexed for guard narrowing so far (test-only).
#[cfg(any(test, feature = "test-support"))]
pub fn index_preparation_for_tests() -> usize {
    INDEX_PREPARATION.with(std::cell::Cell::get)
}

/// Arms this thread's indexed guards visited so far (test-only).
#[cfg(any(test, feature = "test-support"))]
pub fn indexed_arm_visits_for_tests() -> usize {
    INDEXED_ARM_VISITS.with(std::cell::Cell::get)
}

/// The arms of one union, enumerated once per evaluation, with the
/// per-family indexes guards over it decide by, each prepared on first
/// demand.
pub(super) struct ArmIndex {
    /// The node the arms are enumerated from.
    base: SemanticNodeId,
    /// The arms, in enumeration order ([`FlowEvaluator::enumerated_union_arms_or_self`]).
    arms: Arc<[SemanticNodeId]>,
    /// Every arm's position.
    positions: FxHashMap<SemanticNodeId, u32>,
    /// Whether the enumeration met a carrier it could not resolve.
    gapped: bool,
    /// Whether every subset of the arms builds a union enumerating back to
    /// exactly that subset, which is what lets a remainder stand for the
    /// union the general narrow would build ([`FlowEvaluator::arm_index_is_exact`]).
    exact: OnceCell<bool>,
    heritage: OnceCell<Option<Rc<HeritageArms>>>,
    own_keys: OnceCell<Rc<OwnKeyArms>>,
    tags: RefCell<FxHashMap<Arc<str>, Option<Rc<StringArms>>>>,
    literals: OnceCell<Option<Rc<StringArms>>>,
}

impl ArmIndex {
    /// The position of `arm`, when it is one of the indexed arms.
    fn position(&self, arm: SemanticNodeId) -> Option<u32> {
        self.positions.get(&arm).copied()
    }
}

/// A guard subject's current union, read through its index: the arms the
/// standing guards left.
pub(super) struct IndexedArms {
    index: Rc<ArmIndex>,
    remainder: FlowArmRemainder,
}

impl IndexedArms {
    fn holds(&self, position: u32) -> bool {
        self.remainder.holds(position)
    }

    fn arm(&self, position: u32) -> SemanticNodeId {
        self.index.arms[position as usize]
    }

    /// `positions` that remain, sorted and distinct.
    fn remaining_of(&self, mut positions: Vec<u32>) -> Vec<u32> {
        positions.sort_unstable();
        positions.dedup();
        positions.retain(|position| self.holds(*position));
        positions
    }
}

/// A class declaration's identity, as heritage compares it
/// ([`super::super::build::same_class_identity`]).
type ClassKey = (
    verter_type_expr::OwnedStr,
    verter_type_expr::TopLevelOwnerId,
    verter_type_expr::OwnedStr,
);

fn class_key(identity: &crate::semantic_query::DeclIdentity) -> ClassKey {
    (
        identity.canonical_id.clone(),
        identity.owner,
        identity.decl_name.clone(),
    )
}

/// The arms of a union by the class heritage an `instanceof` test decides
/// them by ([`FlowEvaluator::instanceof_heritage_verdict`]).
struct HeritageArms {
    /// Arms whose own heritage chain reaches each declaration: the arms in
    /// that class's family.
    by_ancestor: FxHashMap<ClassKey, Vec<u32>>,
    /// Arms by their own class declaration: the arms a class derived from
    /// it narrows to itself.
    by_identity: FxHashMap<ClassKey, Vec<u32>>,
    /// Arms beyond classification ([`FlowEvaluator::instanceof_arm_is_unclassifiable`]).
    unclassifiable: Vec<u32>,
    /// `null`, `undefined` and `never` arms.
    nullish: Vec<u32>,
    /// Arms whose ancestry is no proof.
    undecided: Vec<u32>,
    /// Each classified arm's own declaration identity and heritage reading
    /// ([`FlowEvaluator::node_heritage_reading`]), read once per
    /// evaluation; `None` for the unclassifiable and nullish arms.
    readings: Vec<Option<(Option<crate::semantic_query::DeclIdentity>, HeritageReading)>>,
    /// Every distinct ancestry observation the arms' heritage reads made,
    /// folded into each frame that decides by this index.
    observed: Vec<crate::semantic_query_memo::ObservedGraphSelfRoot>,
}

impl HeritageArms {
    /// The `instanceof` verdict of the classified arm at `position`
    /// ([`FlowEvaluator::instanceof_heritage_verdict`]), from the reading
    /// the index took.
    fn verdict(
        &self,
        position: u32,
        instance_identity: Option<&crate::semantic_query::DeclIdentity>,
        instance_heritage: &HeritageReading,
    ) -> InstanceofHeritage {
        match &self.readings[position as usize] {
            Some((identity, reading)) => super::instanceof_verdict(
                identity.as_ref(),
                reading,
                instance_identity,
                instance_heritage,
            ),
            None => InstanceofHeritage::Undecidable,
        }
    }
}

/// The arms of a union by their own string property keys.
struct OwnKeyArms {
    /// Plain closed object arms by every key they declare.
    by_key: FxHashMap<Arc<str>, Vec<u32>>,
    /// Each arm's keys, when it is a plain closed object
    /// ([`super::super::ProjectSemanticDispatch::closed_object_own_string_keys`]).
    keys: Vec<Option<FxHashSet<Arc<str>>>>,
    /// Every other arm: its keys are not all known.
    open: Vec<u32>,
}

/// The arms of a union by a distinct string literal each carries: the arm
/// itself, or the arm's member a discriminant test reads.
struct StringArms {
    by_text: FxHashMap<Arc<str>, u32>,
}

/// A discriminant test a guard chain can decide by its string-literal
/// tags: strict equality of the tested member with one string literal.
#[derive(Clone, Copy)]
pub(super) struct StringTagTest<'t> {
    pub(super) text: &'t str,
    pub(super) negated: bool,
}

/// The text of a string literal node.
pub(super) fn string_literal_text(
    graph: &super::super::arena_ops::ArenaOps<'_>,
    node: SemanticNodeId,
) -> Option<Arc<str>> {
    match graph.node_data(node).as_deref() {
        Some(SemanticNodeData::Literal(LiteralValue::String(text))) => {
            Some(Arc::from(text.as_str()))
        }
        _ => None,
    }
}

impl<D: FlowDemandDriver> FlowEvaluator<'_, '_, D> {
    /// The index of `node`'s union arms, enumerated once per evaluation.
    fn arm_index(&self, node: SemanticNodeId) -> Rc<ArmIndex> {
        if let Some(index) = self.arm_indexes.borrow().get(&node) {
            return Rc::clone(index);
        }
        let mut arms = Vec::new();
        let mut expanded = FxHashSet::default();
        let mut gapped = false;
        self.collect_enumerated_arms(node, &mut arms, &mut expanded, &mut gapped);
        note_index_preparation(arms.len());
        let mut positions = FxHashMap::default();
        for (position, arm) in arms.iter().enumerate() {
            positions.entry(*arm).or_insert(position as u32);
        }
        let index = Rc::new(ArmIndex {
            base: node,
            arms: Arc::from(arms.into_boxed_slice()),
            positions,
            gapped,
            exact: OnceCell::new(),
            heritage: OnceCell::new(),
            own_keys: OnceCell::new(),
            tags: RefCell::new(FxHashMap::default()),
            literals: OnceCell::new(),
        });
        self.arm_indexes
            .borrow_mut()
            .insert(node, Rc::clone(&index));
        index
    }

    /// Whether a remainder over `index` stands for exactly the union the
    /// general narrow builds from the same arms: the arms are distinct and
    /// resolved, and their union enumerates back to exactly them, so no
    /// arm absorbs, erases or merges with another (an absorbing `any`, a
    /// literal beside its primitive, a nullable arm the frame's null
    /// algebra erases) and every subset of them is its own union's arms.
    fn arm_index_is_exact(&self, index: &ArmIndex) -> bool {
        *index.exact.get_or_init(|| {
            if index.gapped || index.arms.len() < 2 || index.positions.len() != index.arms.len() {
                return false;
            }
            let built = self.union(&index.arms);
            let mut back = Vec::new();
            let mut expanded = FxHashSet::default();
            let mut gapped = false;
            self.collect_enumerated_arms(built, &mut back, &mut expanded, &mut gapped);
            note_index_preparation(back.len());
            !gapped
                && back.len() == index.arms.len()
                && back.iter().all(|arm| index.positions.contains_key(arm))
        })
    }

    /// A guard subject's current union read through its index, when the
    /// subject is a union of at least two arms an index stands for exactly:
    /// a remainder the standing guards left, or the subject's built type.
    pub(super) fn indexed_subject(&mut self, subject: &SliceNarrowSubject) -> Option<IndexedArms> {
        let node = match self.narrowed_read_unbuilt(subject) {
            Some(FlowNarrowed::Remainder(remainder)) => {
                let index = self.arm_index(remainder.base());
                if !Arc::ptr_eq(&index.arms, remainder.arms()) || remainder.remaining() < 2 {
                    return None;
                }
                return Some(IndexedArms { index, remainder });
            }
            Some(FlowNarrowed::Node(node)) => node,
            None => self.subject_current_node(subject)?,
        };
        let index = self.arm_index(node);
        if !self.arm_index_is_exact(&index) {
            return None;
        }
        let remainder = FlowArmRemainder::whole(index.base, Arc::clone(&index.arms));
        Some(IndexedArms { index, remainder })
    }

    /// The fact an excluding edge establishes once `removed` (remaining
    /// positions) are off it: nothing when no arm is, `never` when every
    /// arm is, else the remainder.
    fn excluding_fact(
        &self,
        subject: &SliceNarrowSubject,
        arms: &IndexedArms,
        removed: &[u32],
    ) -> GuardNarrowing {
        if removed.is_empty() {
            return GuardNarrowing::Unchanged;
        }
        if removed.len() == arms.remainder.remaining() {
            return GuardNarrowing::Narrowed(subject.clone(), self.never_node());
        }
        GuardNarrowing::Excluded(subject.clone(), arms.remainder.excluding(removed))
    }

    /// The fact a selecting edge establishes from the `survivors` it keeps
    /// of the remaining arms.
    fn selecting_fact(
        &self,
        subject: &SliceNarrowSubject,
        arms: &IndexedArms,
        survivors: &[SemanticNodeId],
    ) -> GuardNarrowing {
        if survivors.len() == arms.remainder.remaining() {
            return GuardNarrowing::Unchanged;
        }
        GuardNarrowing::Narrowed(subject.clone(), self.union_or_never(survivors))
    }

    /// The heritage index of `arms`, prepared once per evaluation, its
    /// ancestry observations folded into this frame's self-roots.
    fn heritage_arms(&mut self, index: &ArmIndex) -> Rc<HeritageArms> {
        let heritage = match index.heritage.get() {
            Some(Some(heritage)) => Rc::clone(heritage),
            _ => {
                note_index_preparation(index.arms.len());
                let mut heritage = HeritageArms {
                    by_ancestor: FxHashMap::default(),
                    by_identity: FxHashMap::default(),
                    unclassifiable: Vec::new(),
                    nullish: Vec::new(),
                    undecided: Vec::new(),
                    readings: Vec::with_capacity(index.arms.len()),
                    observed: Vec::new(),
                };
                for (position, arm) in index.arms.iter().enumerate() {
                    let position = position as u32;
                    if self.instanceof_arm_is_unclassifiable(*arm) {
                        heritage.unclassifiable.push(position);
                        heritage.readings.push(None);
                        continue;
                    }
                    if matches!(
                        self.dispatch.arena().node_data(*arm).as_deref(),
                        Some(SemanticNodeData::Primitive(
                            PrimitiveKind::Null | PrimitiveKind::Undefined | PrimitiveKind::Never,
                        ))
                    ) {
                        heritage.nullish.push(position);
                        heritage.readings.push(None);
                        continue;
                    }
                    let reading = self.node_heritage_reading(*arm);
                    match &reading {
                        HeritageReading::Declared(ancestry) => {
                            for ancestor in &ancestry.ancestors {
                                heritage
                                    .by_ancestor
                                    .entry(class_key(ancestor))
                                    .or_default()
                                    .push(position);
                            }
                            if !ancestry.decided {
                                heritage.undecided.push(position);
                            }
                            for observation in ancestry.observed.iter() {
                                if !heritage.observed.contains(observation) {
                                    heritage.observed.push(observation.clone());
                                }
                            }
                        }
                        HeritageReading::Undecided => heritage.undecided.push(position),
                        HeritageReading::ProvedFree => {}
                    }
                    let identity = self.node_class_identity(*arm);
                    if let Some(identity) = &identity {
                        heritage
                            .by_identity
                            .entry(class_key(identity))
                            .or_default()
                            .push(position);
                    }
                    heritage.readings.push(Some((identity, reading)));
                }
                let heritage = Rc::new(heritage);
                let _ = index.heritage.set(Some(Rc::clone(&heritage)));
                heritage
            }
        };
        for observation in &heritage.observed {
            if !self.heritage_self_roots.contains(observation) {
                self.heritage_self_roots.push(observation.clone());
            }
        }
        heritage
    }

    /// `subject instanceof <instance>` over an indexed union. The arms the
    /// tested class's heritage can relate — its family, the classes it
    /// derives from, the instance type itself — and the arms the index
    /// could not classify are decided by the general per-arm rule; every
    /// other arm is PROVED unrelated by heritage. `None` when the tested
    /// class's own heritage is no proof, when the positive edge would read
    /// the union-level fallback (no arm survives by heritage), or when an
    /// arm would leave the edge undecided.
    pub(super) fn narrow_instanceof_indexed(
        &mut self,
        subject: &SliceNarrowSubject,
        arms: &IndexedArms,
        instance: SemanticNodeId,
        instance_identity: Option<&crate::semantic_query::DeclIdentity>,
        instance_heritage: &HeritageReading,
        negated: bool,
    ) -> Option<GuardNarrowing> {
        if !instance_heritage.decided() {
            return None;
        }
        let heritage = self.heritage_arms(&arms.index);
        let mut candidates: Vec<u32> = Vec::new();
        if let Some(identity) = instance_identity {
            if let Some(family) = heritage.by_ancestor.get(&class_key(identity)) {
                candidates.extend_from_slice(family);
            }
        }
        candidates.extend(arms.index.position(instance));
        candidates.extend_from_slice(&heritage.unclassifiable);
        candidates.extend_from_slice(&heritage.undecided);
        if !negated {
            candidates.extend_from_slice(&heritage.nullish);
            if let HeritageReading::Declared(ancestry) = instance_heritage {
                for ancestor in &ancestry.ancestors {
                    if let Some(bases) = heritage.by_identity.get(&class_key(ancestor)) {
                        candidates.extend_from_slice(bases);
                    }
                }
            }
        }
        let candidates = arms.remaining_of(candidates);
        note_arm_visits(candidates.len());
        if negated {
            let mut removed = Vec::new();
            let mut gapped = false;
            for position in candidates {
                let arm = arms.arm(position);
                if self.instanceof_arm_is_unclassifiable(arm) {
                    gapped = true;
                    continue;
                }
                if arm == instance {
                    removed.push(position);
                    continue;
                }
                match heritage.verdict(position, instance_identity, instance_heritage) {
                    InstanceofHeritage::ArmInFamily => removed.push(position),
                    InstanceofHeritage::InstanceDerivesFromArm | InstanceofHeritage::Unrelated => {}
                    InstanceofHeritage::Undecidable => gapped = true,
                }
            }
            if gapped {
                self.record_degradation(FlowReturnDegradation::FlowGap(FlowGap::GuardNarrowing));
            }
            return Some(self.excluding_fact(subject, arms, &removed));
        }
        let mut survivors = Vec::new();
        let mut changed = false;
        for position in candidates {
            let arm = arms.arm(position);
            if self.instanceof_arm_is_unclassifiable(arm) {
                return None;
            }
            if heritage.nullish.binary_search(&position).is_ok() {
                changed = true;
                continue;
            }
            if arm == instance {
                survivors.push(arm);
                continue;
            }
            match heritage.verdict(position, instance_identity, instance_heritage) {
                InstanceofHeritage::ArmInFamily => survivors.push(arm),
                InstanceofHeritage::InstanceDerivesFromArm => {
                    survivors.push(instance);
                    changed = true;
                }
                InstanceofHeritage::Unrelated => {}
                InstanceofHeritage::Undecidable => return None,
            }
        }
        if survivors.is_empty() {
            return None;
        }
        if !changed && survivors.len() == arms.remainder.remaining() {
            return Some(GuardNarrowing::Unchanged);
        }
        Some(GuardNarrowing::Narrowed(
            subject.clone(),
            self.union(&survivors),
        ))
    }

    /// The own-key index of `arms`, prepared once per evaluation.
    fn own_key_arms(&self, index: &ArmIndex) -> Rc<OwnKeyArms> {
        Rc::clone(index.own_keys.get_or_init(|| {
            note_index_preparation(index.arms.len());
            let mut own = OwnKeyArms {
                by_key: FxHashMap::default(),
                keys: Vec::with_capacity(index.arms.len()),
                open: Vec::new(),
            };
            for (position, arm) in index.arms.iter().enumerate() {
                let position = position as u32;
                match self.dispatch.closed_object_own_string_keys(*arm) {
                    Some(keys) => {
                        for key in &keys {
                            own.by_key
                                .entry(Arc::clone(key))
                                .or_default()
                                .push(position);
                        }
                        own.keys.push(Some(keys.into_iter().collect()));
                    }
                    None => {
                        own.keys.push(None);
                        own.open.push(position);
                    }
                }
            }
            Rc::new(own)
        }))
    }

    /// `"key" in subject` over an indexed union, in the checker's
    /// known-key regime. A plain closed object arm that does not declare
    /// the key is PROVED to lack it (`Never`); every other arm is decided by
    /// its own presence verdict ([`Self::arm_in_presence`]). `None` when no
    /// arm proves the key present: the unknown-key regime reads the whole
    /// subject.
    pub(super) fn narrow_in_indexed(
        &mut self,
        key: &Arc<str>,
        subject: &SliceNarrowSubject,
        arms: &IndexedArms,
        negated: bool,
    ) -> Option<GuardNarrowing> {
        let own = self.own_key_arms(&arms.index);
        let mut candidates = own.by_key.get(key.as_ref()).cloned().unwrap_or_default();
        candidates.extend_from_slice(&own.open);
        let candidates = arms.remaining_of(candidates);
        note_arm_visits(candidates.len());
        let presences: Vec<(u32, InArmPresence)> = candidates
            .iter()
            .map(|position| (*position, self.arm_in_presence(arms.arm(*position), key)))
            .collect();
        let key_is_known = presences.iter().any(|(_, presence)| {
            matches!(
                presence,
                InArmPresence::Always | InArmPresence::Optional | InArmPresence::Indexed
            )
        });
        if !key_is_known {
            return None;
        }
        let gapped = presences
            .iter()
            .any(|(_, presence)| matches!(presence, InArmPresence::Unknown));
        let fact = if negated {
            let removed: Vec<u32> = presences
                .iter()
                .filter(|(_, presence)| matches!(presence, InArmPresence::Always))
                .map(|(position, _)| *position)
                .collect();
            self.excluding_fact(subject, arms, &removed)
        } else {
            let survivors: Vec<SemanticNodeId> = presences
                .iter()
                .filter(|(_, presence)| !matches!(presence, InArmPresence::Never))
                .map(|(position, _)| arms.arm(*position))
                .collect();
            self.selecting_fact(subject, arms, &survivors)
        };
        if gapped {
            self.record_degradation(FlowReturnDegradation::FlowGap(FlowGap::GuardNarrowing));
        }
        Some(fact)
    }

    /// A user type predicate's narrow to `target` over an indexed union. A
    /// plain closed object arm lacking a property the target requires is
    /// PROVED not assignable to it
    /// ([`super::super::ProjectSemanticDispatch::assignability_refuting_keys`]);
    /// only the arms declaring every such property, and the arms the index
    /// cannot read, are related through the shared authority. `None` when
    /// the target refutes nothing by its keys, or when no arm survives the
    /// positive edge (the candidate and intersection readings take the
    /// whole subject).
    pub(super) fn narrow_to_predicate_indexed(
        &mut self,
        subject: &SliceNarrowSubject,
        arms: &IndexedArms,
        target: SemanticNodeId,
        negated: bool,
    ) -> Option<(GuardNarrowing, PredicateNarrowConsumption)> {
        let refuting = self
            .dispatch
            .assignability_refuting_keys(target, self.canonical)?;
        let own = self.own_key_arms(&arms.index);
        let holders = |key: &Arc<str>| own.by_key.get(key).map_or(&[][..], Vec::as_slice);
        let rarest = refuting.iter().min_by_key(|key| holders(key).len())?;
        let held = holders(rarest);
        note_arm_visits(held.len());
        let mut candidates: Vec<u32> = held
            .iter()
            .copied()
            .filter(|position| {
                own.keys[*position as usize]
                    .as_ref()
                    .is_some_and(|keys| refuting.iter().all(|key| keys.contains(key)))
            })
            .collect();
        candidates.extend_from_slice(&own.open);
        let candidates = arms.remaining_of(candidates);
        note_arm_visits(candidates.len());
        let mut related = Vec::with_capacity(candidates.len());
        for position in candidates {
            match self.narrowing_assignable(arms.arm(position), target) {
                Some(true) => related.push(position),
                Some(false) => {}
                None => {
                    return Some((
                        GuardNarrowing::Unchanged,
                        PredicateNarrowConsumption::Undecided,
                    ))
                }
            }
        }
        if negated {
            let fact = self.excluding_fact(subject, arms, &related);
            return Some((fact, PredicateNarrowConsumption::Decided));
        }
        if related.is_empty() {
            return None;
        }
        let survivors: Vec<SemanticNodeId> =
            related.iter().map(|position| arms.arm(*position)).collect();
        Some((
            self.selecting_fact(subject, arms, &survivors),
            PredicateNarrowConsumption::Decided,
        ))
    }

    /// The arms of `index` by the string literal each one's member `name`
    /// reads ([`Self::discriminant_member`]), when every arm's reads one,
    /// each distinct.
    fn tag_arms(&mut self, index: &ArmIndex, name: &Arc<str>) -> Option<Rc<StringArms>> {
        if let Some(tags) = index.tags.borrow().get(name) {
            return tags.clone();
        }
        note_index_preparation(index.arms.len());
        let member: Arc<[Arc<str>]> = Arc::from([Arc::clone(name)]);
        let mut by_text = FxHashMap::default();
        let mut tags = Some(());
        for (position, arm) in index.arms.iter().enumerate() {
            let text = self
                .discriminant_member(*arm, &member)
                .and_then(|tag| string_literal_text(self.dispatch.graph(), tag));
            let distinct = text.is_some_and(|text| by_text.insert(text, position as u32).is_none());
            if !distinct {
                tags = None;
                break;
            }
        }
        let tags = tags.map(|()| Rc::new(StringArms { by_text }));
        index
            .tags
            .borrow_mut()
            .insert(Arc::clone(name), tags.clone());
        tags
    }

    /// The arms of `index` by their own text, when every arm is a distinct
    /// string literal.
    fn literal_arms(&self, index: &ArmIndex) -> Option<Rc<StringArms>> {
        index
            .literals
            .get_or_init(|| {
                note_index_preparation(index.arms.len());
                let graph = self.dispatch.graph();
                let mut by_text = FxHashMap::default();
                for (position, arm) in index.arms.iter().enumerate() {
                    let text = string_literal_text(graph, *arm)?;
                    if by_text.insert(text, position as u32).is_some() {
                        return None;
                    }
                }
                Some(Rc::new(StringArms { by_text }))
            })
            .clone()
    }

    /// A discriminant equality's narrow of the tested member's PARENT over
    /// an indexed union whose arms each tag the member with a distinct
    /// string literal (`narrowTypeByDiscriminant` over strict equality):
    /// the positive edge keeps the arm whose tag is the compared string,
    /// `never` when no remaining arm carries it; the negated edge excludes
    /// that arm. `None` when the arms do not all carry distinct string tags.
    pub(super) fn narrow_parent_by_string_tag(
        &mut self,
        parent: &SliceNarrowSubject,
        arms: &IndexedArms,
        name: &Arc<str>,
        test: StringTagTest<'_>,
    ) -> Option<GuardNarrowing> {
        let tags = self.tag_arms(&arms.index, name)?;
        let hit = tags
            .by_text
            .get(test.text)
            .copied()
            .filter(|position| arms.holds(*position));
        note_arm_visits(usize::from(hit.is_some()));
        Some(match (test.negated, hit) {
            (false, Some(position)) => self.selecting_fact(parent, arms, &[arms.arm(position)]),
            (false, None) => GuardNarrowing::Narrowed(parent.clone(), self.never_node()),
            (true, Some(position)) => self.excluding_fact(parent, arms, &[position]),
            (true, None) => GuardNarrowing::Unchanged,
        })
    }

    /// The tested member's own narrow under a strict string equality, over
    /// an indexed union of distinct string literals: the positive edge
    /// reads the arm spelling the compared string, `never` when no
    /// remaining arm does; the negated edge excludes that arm. `None` when
    /// the arms are not all distinct string literals.
    pub(super) fn narrow_string_literal_arms(
        &mut self,
        subject: &SliceNarrowSubject,
        arms: &IndexedArms,
        test: StringTagTest<'_>,
    ) -> Option<GuardNarrowing> {
        let literals = self.literal_arms(&arms.index)?;
        let hit = literals
            .by_text
            .get(test.text)
            .copied()
            .filter(|position| arms.holds(*position));
        note_arm_visits(usize::from(hit.is_some()));
        Some(match (test.negated, hit) {
            (false, Some(position)) => self.selecting_fact(subject, arms, &[arms.arm(position)]),
            (false, None) => GuardNarrowing::Narrowed(subject.clone(), self.never_node()),
            (true, Some(position)) => self.excluding_fact(subject, arms, &[position]),
            (true, None) => GuardNarrowing::Unchanged,
        })
    }

    /// Whether `subject`'s current union is indexed as distinct string
    /// literals — the leaf a discriminant chain narrows alongside its
    /// parent.
    pub(super) fn indexed_string_literals(
        &mut self,
        subject: &SliceNarrowSubject,
    ) -> Option<IndexedArms> {
        let arms = self.indexed_subject(subject)?;
        self.literal_arms(&arms.index)?;
        Some(arms)
    }
}
