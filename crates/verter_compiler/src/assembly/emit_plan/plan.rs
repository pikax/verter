use std::sync::atomic::{AtomicU64, Ordering};

use verter_span::Span;

use super::segment::{EmissionSegment, FragmentRef, SlotId};
use super::{ArtifactBoundary, MapMode};
use crate::assembly::fragment::{FragmentDialect, GrammarWitness, SyntacticContract};
use crate::code_transform::{CodeTransform, CodeTransformError};
use crate::emission::{
    EmissionBuilder, EmissionError, EmissionFragment, FragmentSlot, QualifiedSource, SourceKey,
};

/// Identity of one plan, unique in the process. A slot handle carries the
/// token of the plan that reserved it.
#[derive(Debug, Clone, Copy, Eq)]
struct PlanToken(u64);

impl PartialEq for PlanToken {
    fn eq(&self, other: &Self) -> bool {
        self.0 == other.0
    }
}

impl PlanToken {
    fn fresh() -> Self {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        Self(NEXT.fetch_add(1, Ordering::Relaxed))
    }
}

/// Handle of a reserved slot. Bound to the plan that reserved it; neither
/// `Copy` nor `Clone`, and consumed by the fill, so a slot is filled once.
#[derive(Debug)]
pub struct SlotHandle {
    token: PlanToken,
    slot: SlotId,
    contract: SyntacticContract,
}

impl SlotHandle {
    /// The grammar context the slot was reserved for.
    pub fn contract(&self) -> SyntacticContract {
        self.contract
    }
}

/// Typed plan-construction failure. No partial plan exists.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum EmitPlanError {
    /// An authored slice is outside its source or splits a UTF-8 sequence.
    InvalidSourceSlice {
        span: Span,
    },
    /// A slot was reserved and never filled.
    UnfilledSlot,
    /// The handle was reserved by another plan.
    ForeignHandle,
    /// The fragment's grammar context is not the slot's.
    SlotContractMismatch {
        slot: SyntacticContract,
        fragment: SyntacticContract,
    },
    /// A fragment or nested plan was created under the other `MapMode`.
    ModeMismatch,
    /// One source unit and space with two content identities.
    SourceRevisionConflict {
        key: SourceKey,
    },
    /// Output would exceed `u32::MAX` bytes.
    Capacity,
    MapNotCaptured,
    /// A fragment slot that names no fragment of this plan, or one that was
    /// already filled.
    UnknownSlot,
    /// A transform refused to freeze.
    Edit(CodeTransformError),
    /// The plan is not exactly one placed fragment.
    NotOneFragment,
}

impl From<EmissionError> for EmitPlanError {
    fn from(error: EmissionError) -> Self {
        match error {
            EmissionError::InvalidSourceSlice { span } => Self::InvalidSourceSlice { span },
            EmissionError::Capacity => Self::Capacity,
            EmissionError::MapNotCaptured => Self::MapNotCaptured,
            EmissionError::ModeMismatch => Self::ModeMismatch,
            // The plan opens a new fragment whenever the source changes and
            // resolves markers itself, so neither reaches it from a builder.
            EmissionError::ForeignSource | EmissionError::UnknownMarker(_) => Self::UnknownSlot,
        }
    }
}

pub(super) enum SlotBody<'s> {
    Reserved,
    Fragment(EmissionFragment<'s>),
    Plan(EmitPlan<'s>),
    Empty,
}

/// Where a fragment holding a producer-declared slot sits in the plan.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub(super) enum FillHost {
    /// A placed fragment, by index.
    Placed(u32),
    /// A reserved slot filled with a fragment, by slot index.
    Slot(u32),
}

/// A slot a producer declared inside one of the plan's fragments, filled
/// before writing.
pub(super) struct MarkedFill<'s> {
    pub(super) host: FillHost,
    pub(super) piece: u32,
    pub(super) with: EmissionFragment<'s>,
}

/// An ordered, immutable placement of fragments over qualified sources.
pub struct EmitPlan<'s> {
    pub(super) fragments: Vec<EmissionFragment<'s>>,
    pub(super) segments: Vec<EmissionSegment>,
    pub(super) slots: Vec<SlotBody<'s>>,
    /// Ordered by host, then piece.
    pub(super) marked: Vec<MarkedFill<'s>>,
    /// Every source a fragment of this plan or of a nested plan names, merged
    /// by key in first-use order.
    pub(super) sources: Vec<QualifiedSource<'s>>,
    pub(super) len: u32,
    token: PlanToken,
    pub(super) mode: MapMode,
    /// Present while the whole plan is a producer's own writing: opened by
    /// a witnessed producer, with every slot filled by a witnessed fragment
    /// or plan of the slot's grammar, and nothing else placed.
    pub(super) grammar: Option<GrammarWitness>,
}

impl<'s> EmitPlan<'s> {
    pub fn segments(&self) -> &[EmissionSegment] {
        &self.segments
    }

    /// Exact output length, cached; no walk.
    pub fn len(&self) -> u32 {
        self.len
    }

    pub fn is_empty(&self) -> bool {
        self.len == 0
    }

    pub fn mode(&self) -> MapMode {
        self.mode
    }

    /// The grammar the whole plan is witnessed in, when it is.
    pub fn grammar(&self) -> Option<GrammarWitness> {
        self.grammar
    }

    /// The sources this plan's fragments name.
    pub fn sources(&self) -> &[QualifiedSource<'s>] {
        &self.sources
    }

    /// The plan's fragment, when the plan is exactly one placed fragment
    /// with no slot, boundary or fill. Moves it out; nothing is written.
    ///
    /// # Errors
    ///
    /// [`EmitPlanError::NotOneFragment`] for any other plan.
    pub fn into_single_fragment(mut self) -> Result<EmissionFragment<'s>, EmitPlanError> {
        let single = self.slots.is_empty()
            && self.marked.is_empty()
            && matches!(self.segments.as_slice(), [EmissionSegment::Fragment(_)]);
        match self.fragments.pop() {
            Some(fragment) if single && self.fragments.is_empty() => Ok(fragment),
            _ => Err(EmitPlanError::NotOneFragment),
        }
    }

    /// The last byte the plan writes, fills and nested plans included.
    pub(crate) fn last_byte(&self) -> Option<u8> {
        self.segments
            .iter()
            .rev()
            .find_map(|segment| match *segment {
                EmissionSegment::Fragment(index) => self.fragment_last_byte(
                    &self.fragments[index.0 as usize],
                    FillHost::Placed(index.0),
                ),
                EmissionSegment::Slot(slot) => match &self.slots[slot.0 as usize] {
                    SlotBody::Fragment(fragment) => {
                        self.fragment_last_byte(fragment, FillHost::Slot(slot.0))
                    }
                    SlotBody::Plan(nested) => nested.last_byte(),
                    SlotBody::Empty | SlotBody::Reserved => None,
                },
                EmissionSegment::Boundary(_) => None,
            })
    }

    fn fragment_last_byte(&self, fragment: &EmissionFragment<'s>, host: FillHost) -> Option<u8> {
        fragment
            .pieces()
            .iter()
            .enumerate()
            .rev()
            .find_map(|(index, piece)| {
                match self
                    .marked
                    .iter()
                    .find(|fill| fill.host == host && fill.piece == index as u32)
                {
                    Some(fill) => last_byte_of(&fill.with),
                    None => fragment.piece_text(piece).as_bytes().last().copied(),
                }
            })
    }
}

/// The last byte `fragment` holds.
fn last_byte_of(fragment: &EmissionFragment<'_>) -> Option<u8> {
    fragment
        .pieces()
        .iter()
        .rev()
        .find_map(|piece| fragment.piece_text(piece).as_bytes().last().copied())
}

/// Append-only builder. Nothing appended can be edited afterwards; the only
/// changes to a placed fragment are fills of slots its producer declared.
pub struct EmitPlanBuilder<'s> {
    plan: EmitPlan<'s>,
    /// Text pushed since the last fragment, slot or boundary.
    open: Option<EmissionBuilder<'s>>,
    /// The first refusal of a push that returns no result; `finish` reports
    /// it.
    failure: Option<EmitPlanError>,
}

impl<'s> EmitPlanBuilder<'s> {
    pub fn new(mode: MapMode) -> Self {
        Self::opened(mode, None)
    }

    /// A plan a producer writes as a whole: its own text, slots of declared
    /// grammar, and the witnessed fragments that fill them.
    pub fn witnessed(mode: MapMode, witness: GrammarWitness) -> Self {
        Self::opened(mode, Some(witness))
    }

    fn opened(mode: MapMode, grammar: Option<GrammarWitness>) -> Self {
        Self {
            plan: EmitPlan {
                fragments: Vec::new(),
                segments: Vec::new(),
                slots: Vec::new(),
                marked: Vec::new(),
                sources: Vec::new(),
                len: 0,
                token: PlanToken::fresh(),
                mode,
                grammar,
            },
            open: None,
            failure: None,
        }
    }

    pub fn mode(&self) -> MapMode {
        self.plan.mode
    }

    /// Authored bytes of `source` carried through unchanged.
    pub fn push_source(
        &mut self,
        source: &QualifiedSource<'s>,
        span: Span,
    ) -> Result<(), EmitPlanError> {
        self.open_for(Some(source))
            .push_source(span)
            .map_err(EmitPlanError::from)
    }

    /// Generated text whose first byte stands for `offset` in `source`.
    pub fn push_anchored(&mut self, text: &str, source: &QualifiedSource<'s>, offset: u32) {
        match self.plan.mode {
            MapMode::NoMap => self.open_for(None).push_unmapped(text),
            MapMode::Map => self.open_for(Some(source)).push_anchored(text, offset),
        }
    }

    pub fn push_unmapped(&mut self, text: &str) {
        self.open_for(None).push_unmapped(text);
    }

    pub fn push_unmapped_fmt(&mut self, args: std::fmt::Arguments<'_>) {
        self.open_for(None).push_unmapped_fmt(args);
    }

    /// A generated buffer, moved in without copying its bytes.
    pub fn push_owned(&mut self, text: String) {
        self.open_for(None)
            .push_owned(text, crate::emission::Provenance::Unmapped);
    }

    pub fn push_boundary(&mut self, boundary: ArtifactBoundary) {
        self.close_open();
        self.plan.segments.push(EmissionSegment::Boundary(boundary));
    }

    /// Place a finished fragment. Its source joins the plan's sources.
    /// A fragment placed this way carries no witness, so the plan stops
    /// being one producer's writing.
    pub fn push_fragment(&mut self, fragment: EmissionFragment<'s>) -> Result<(), EmitPlanError> {
        self.close_open();
        self.plan.grammar = None;
        self.place(fragment)
    }

    /// Freeze `transform` over `source`, its own source text, and place
    /// the fragment. The transform's pieces are handed over; no string is
    /// built on the way.
    pub fn push_transform<M: verter_audit::observe::ObserveExec>(
        &mut self,
        transform: CodeTransform<'s, M>,
        source: QualifiedSource<'s>,
    ) -> Result<(), EmitPlanError> {
        let fragment = transform.freeze(source).map_err(EmitPlanError::Edit)?;
        self.push_fragment(fragment)
    }

    /// Reserve a slot for a fragment in the grammar context `contract`.
    pub fn reserve_slot(&mut self, contract: SyntacticContract) -> SlotHandle {
        self.close_open();
        let slot = SlotId(self.plan.slots.len() as u32);
        self.plan.slots.push(SlotBody::Reserved);
        self.plan.segments.push(EmissionSegment::Slot(slot));
        SlotHandle {
            token: self.plan.token,
            slot,
            contract,
        }
    }

    /// Fill a reserved slot with a fragment its producer witnessed in the
    /// slot's grammar context.
    pub fn fill_fragment(
        &mut self,
        handle: SlotHandle,
        fragment: EmissionFragment<'s>,
        witness: GrammarWitness,
    ) -> Result<(), EmitPlanError> {
        self.check_handle(&handle)?;
        if witness.contract() != handle.contract {
            return Err(EmitPlanError::SlotContractMismatch {
                slot: handle.contract,
                fragment: witness.contract(),
            });
        }
        self.keep_grammar_if(witness.dialect());
        self.admit_fragment(&fragment)?;
        self.plan.len = add_len(self.plan.len, fragment.len())?;
        self.plan.slots[handle.slot.0 as usize] = SlotBody::Fragment(fragment);
        Ok(())
    }

    /// Fill a reserved slot with a nested plan written in the grammar
    /// context `context`; the plan is written in place.
    pub fn fill_plan(
        &mut self,
        handle: SlotHandle,
        plan: EmitPlan<'s>,
        context: SyntacticContract,
    ) -> Result<(), EmitPlanError> {
        self.check_handle(&handle)?;
        if context != handle.contract {
            return Err(EmitPlanError::SlotContractMismatch {
                slot: handle.contract,
                fragment: context,
            });
        }
        if plan.mode != self.plan.mode {
            return Err(EmitPlanError::ModeMismatch);
        }
        match plan.grammar {
            Some(witness) if witness.contract() == context => {
                self.keep_grammar_if(witness.dialect());
            }
            _ => self.plan.grammar = None,
        }
        let mut sources = self.plan.sources.clone();
        for source in &plan.sources {
            merge_source(&mut sources, source)?;
        }
        let len = add_len(self.plan.len, plan.len)?;
        self.plan.sources = sources;
        self.plan.len = len;
        self.plan.slots[handle.slot.0 as usize] = SlotBody::Plan(plan);
        Ok(())
    }

    /// Leave a reserved slot empty on purpose.
    pub fn fill_empty(&mut self, handle: SlotHandle) -> Result<(), EmitPlanError> {
        self.check_handle(&handle)?;
        self.plan.slots[handle.slot.0 as usize] = SlotBody::Empty;
        Ok(())
    }

    /// Fill a position a producer declared inside one of this plan's
    /// fragments: the marked piece's bytes are replaced by `with`'s. This is
    /// the only way assembly changes bytes inside a fragment, and it happens
    /// before the plan is written. The fill carries no witness, so the plan
    /// stops being one producer's writing.
    pub fn replace_marked(
        &mut self,
        slot: FragmentSlot,
        with: EmissionFragment<'s>,
    ) -> Result<(), EmitPlanError> {
        self.plan.grammar = None;
        self.fill_marked(slot, with)
    }

    /// [`Self::replace_marked`] with a fill its producer witnessed: the plan
    /// stays witnessed when the fill's dialect is the plan's.
    pub fn replace_marked_witnessed(
        &mut self,
        slot: FragmentSlot,
        with: EmissionFragment<'s>,
        witness: GrammarWitness,
    ) -> Result<(), EmitPlanError> {
        self.keep_grammar_if(witness.dialect());
        self.fill_marked(slot, with)
    }

    fn fill_marked(
        &mut self,
        slot: FragmentSlot,
        with: EmissionFragment<'s>,
    ) -> Result<(), EmitPlanError> {
        self.close_open();
        let token = slot.fragment();
        let (host, holder) = match self
            .plan
            .fragments
            .iter()
            .position(|fragment| fragment.token() == token)
        {
            Some(index) => (FillHost::Placed(index as u32), &self.plan.fragments[index]),
            None => self
                .plan
                .slots
                .iter()
                .enumerate()
                .find_map(|(index, body)| match body {
                    SlotBody::Fragment(fragment) if fragment.token() == token => {
                        Some((FillHost::Slot(index as u32), fragment))
                    }
                    _ => None,
                })
                .ok_or(EmitPlanError::UnknownSlot)?,
        };
        let piece = slot.piece();
        let replaced = holder
            .pieces()
            .get(piece as usize)
            .ok_or(EmitPlanError::UnknownSlot)?
            .len();
        if self
            .plan
            .marked
            .iter()
            .any(|fill| fill.host == host && fill.piece == piece)
        {
            return Err(EmitPlanError::UnknownSlot);
        }
        self.admit_fragment(&with)?;
        let len = add_len(self.plan.len - replaced, with.len())?;
        self.plan.len = len;
        self.plan.marked.push(MarkedFill { host, piece, with });
        Ok(())
    }

    pub fn finish(mut self) -> Result<EmitPlan<'s>, EmitPlanError> {
        self.close_open();
        if let Some(failure) = self.failure {
            return Err(failure);
        }
        if self
            .plan
            .slots
            .iter()
            .any(|slot| matches!(slot, SlotBody::Reserved))
        {
            return Err(EmitPlanError::UnfilledSlot);
        }
        self.plan.marked.sort_by_key(|fill| (fill.host, fill.piece));
        Ok(self.plan)
    }

    /// The open builder, reopened when `source` differs from the source its
    /// pieces already name.
    fn open_for(&mut self, source: Option<&QualifiedSource<'s>>) -> &mut EmissionBuilder<'s> {
        let reopen = match (self.open.as_ref(), source) {
            (Some(open), Some(source)) => open
                .source()
                .is_some_and(|current| !current.key.same(&source.key)),
            _ => false,
        };
        if reopen {
            self.close_open();
        }
        let open = match self.open.take() {
            Some(open) => open,
            None => EmissionBuilder::new(self.plan.mode),
        };
        let open = match (open.source(), source) {
            (None, Some(source)) => open.with_source(source.clone()),
            _ => open,
        };
        self.open.insert(open)
    }

    fn close_open(&mut self) {
        let Some(open) = self.open.take() else {
            return;
        };
        if open.is_empty() && open.source().is_none() {
            return;
        }
        let placed = open
            .finish()
            .map_err(EmitPlanError::from)
            .and_then(|fragment| self.place(fragment));
        if let Err(failure) = placed {
            self.failure.get_or_insert(failure);
        }
    }

    fn place(&mut self, fragment: EmissionFragment<'s>) -> Result<(), EmitPlanError> {
        self.admit_fragment(&fragment)?;
        self.plan.len = add_len(self.plan.len, fragment.len())?;
        let index = FragmentRef(self.plan.fragments.len() as u32);
        self.plan.fragments.push(fragment);
        self.plan.segments.push(EmissionSegment::Fragment(index));
        Ok(())
    }

    /// The fragment's policy is the plan's, and its source joins the plan's
    /// sources.
    fn admit_fragment(&mut self, fragment: &EmissionFragment<'s>) -> Result<(), EmitPlanError> {
        if fragment.mode() != self.plan.mode {
            return Err(EmitPlanError::ModeMismatch);
        }
        if let Some(source) = fragment.source() {
            merge_source(&mut self.plan.sources, source)?;
        }
        Ok(())
    }

    /// A witnessed fill keeps the plan witnessed only in the plan's own
    /// dialect.
    fn keep_grammar_if(&mut self, dialect: FragmentDialect) {
        if self
            .plan
            .grammar
            .is_some_and(|grammar| grammar.dialect() != dialect)
        {
            self.plan.grammar = None;
        }
    }

    fn check_handle(&self, handle: &SlotHandle) -> Result<(), EmitPlanError> {
        if handle.token != self.plan.token {
            return Err(EmitPlanError::ForeignHandle);
        }
        Ok(())
    }
}

fn add_len(len: u32, more: u32) -> Result<u32, EmitPlanError> {
    len.checked_add(more).ok_or(EmitPlanError::Capacity)
}

/// Add `source` to `sources` unless a source with its key is there. One
/// unit and space at two revisions is refused.
fn merge_source<'s>(
    sources: &mut Vec<QualifiedSource<'s>>,
    source: &QualifiedSource<'s>,
) -> Result<(), EmitPlanError> {
    for registered in sources.iter() {
        if registered.key.same(&source.key) {
            return Ok(());
        }
        if registered.key.conflicts_with(&source.key) {
            return Err(EmitPlanError::SourceRevisionConflict {
                key: source.key.clone(),
            });
        }
    }
    sources.push(source.clone());
    Ok(())
}

#[cfg(test)]
#[path = "plan_tests.rs"]
mod tests;
