//! The one positional model.
//!
//! Every consumer of a signature's parameter shape — comparison,
//! applicability, tuple projection, utility inference — reads it through
//! [`PositionalShape`]. A mode selects the read's purpose; the arity rules
//! themselves are shared, so one signature cannot decide differently per
//! consumer.
//!
//! Rules (the rows of the parameter matrix):
//!
//! * The receiver (`this`) is never a positional slot.
//! * The declared minimum is the LAST required position + 1 (a required
//!   position after an optional one makes the optional one required by
//!   position); required positions after a rest run count too.
//! * The effective minimum additionally relaxes trailing required
//!   positions whose type accepts `void`, unless the read is
//!   void-is-non-optional, and is 0 for an untyped JavaScript signature
//!   unless a strong-arity read is requested.
//! * A rest run is open-ended; a fixed tuple rest is flattened into
//!   ordinary positions before the layout is interned.
//! * Optionality carries an explicit `includes_undefined` fact on the slot
//!   (strict-null behaviour); it is never inferred from a missing span.
//! * Parameter names never take part in type equality.

use super::records::{
    ParameterLayout, ParameterOptionality, ParameterSlot, RestKind, RestSlot,
    SignatureSemanticFlags, SpellingId, TypeToken,
};

/// What a read of the shape is for. All modes share one arity truth.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub enum PositionalMode {
    Comparison,
    Applicability,
    TupleProjection,
    UtilityInference,
}

/// Explicit minimum-arity read flags.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug, Default)]
pub struct MinArityFlags {
    pub void_is_non_optional: bool,
    pub strong_arity_for_untyped_js: bool,
}

impl PositionalMode {
    /// The flags a mode reads with. Deliberately identical for every mode:
    /// a signature has one arity, whichever consumer asks.
    #[must_use]
    pub const fn min_arity_flags(self) -> MinArityFlags {
        MinArityFlags {
            void_is_non_optional: false,
            strong_arity_for_untyped_js: false,
        }
    }
}

/// Type facts the positional model needs but does not own.
pub trait SlotTypeFacts {
    /// Whether the type accepts `void` (is `void` or a union containing it).
    fn accepts_void(&self, ty: TypeToken) -> bool;
}

/// The type at one argument position.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum TypeAt<'a> {
    /// Past the last position of a signature with no rest run.
    Absent,
    One(ParameterSlot),
    /// Inside a rest run: the run element, or (with a tail) the union of the
    /// element and every tail position.
    Run {
        element: TypeToken,
        tail: &'a [ParameterSlot],
    },
    /// Inside an uninstantiated generic rest: the rest type indexed by the
    /// position (`T[index]`).
    GenericRest {
        rest: TypeToken,
        index: usize,
    },
}

/// One element of a projected parameter tuple.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub struct ProjectedElement {
    pub ty: TypeToken,
    pub kind: ProjectedKind,
    pub name: Option<SpellingId>,
}

#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub enum ProjectedKind {
    Required,
    Optional,
    Variadic,
}

/// Result of projecting the parameters from a position on.
#[derive(Clone, PartialEq, Eq, Debug)]
pub enum ProjectedTuple {
    /// Start lies inside/at the rest run: the rest itself (`exact` when the
    /// start is exactly the rest position) or an array of its element.
    Rest {
        ty: TypeToken,
        exact: bool,
    },
    Elements(Vec<ProjectedElement>),
}

/// Borrowed positional view over one signature's layout.
pub struct PositionalShape<'a> {
    layout: &'a ParameterLayout,
    receiver: Option<ParameterSlot>,
    flags: SignatureSemanticFlags,
    facts: &'a dyn SlotTypeFacts,
}

impl<'a> PositionalShape<'a> {
    #[must_use]
    pub fn new(
        layout: &'a ParameterLayout,
        receiver: Option<ParameterSlot>,
        flags: SignatureSemanticFlags,
        facts: &'a dyn SlotTypeFacts,
    ) -> Self {
        Self {
            layout,
            receiver,
            flags,
            facts,
        }
    }

    /// The receiver, if any. Never counted by any arity accessor.
    #[must_use]
    pub fn receiver(&self) -> Option<ParameterSlot> {
        self.receiver
    }

    #[must_use]
    pub fn has_rest(&self) -> bool {
        self.layout.rest.is_some()
    }

    #[must_use]
    pub fn rest(&self) -> Option<&'a RestSlot> {
        self.layout.rest.as_ref()
    }

    /// Positions a caller can address before the rest run (rest itself is
    /// one more counted position, as in the checker's parameter count).
    #[must_use]
    pub fn parameter_count(&self) -> usize {
        self.layout.parameters.len() + usize::from(self.layout.rest.is_some())
    }

    /// Maximum accepted argument count; `None` when a rest run is open.
    #[must_use]
    pub fn max_arity(&self) -> Option<usize> {
        if self.has_rest() {
            None
        } else {
            Some(self.layout.parameters.len())
        }
    }

    /// The declared minimum from the layout alone.
    #[must_use]
    pub fn declared_minimum(&self) -> usize {
        let params = &self.layout.parameters;
        if let Some(rest) = &self.layout.rest {
            if let Some(last) = rest
                .tail
                .iter()
                .rposition(|s| !s.optionality.declared_optional)
            {
                return params.len() + last + 1;
            }
        }
        params
            .iter()
            .rposition(|s| !s.optionality.declared_optional)
            .map_or(0, |p| p + 1)
    }

    /// Effective minimum under explicit flags.
    #[must_use]
    pub fn effective_minimum_with(&self, flags: MinArityFlags) -> usize {
        if !flags.strong_arity_for_untyped_js
            && self.flags.contains(SignatureSemanticFlags::UNTYPED_JS)
        {
            return 0;
        }
        let mut min = self.declared_minimum();
        if flags.void_is_non_optional {
            return min;
        }
        while min > 0 {
            let accepts = match self.type_at(min - 1) {
                TypeAt::One(slot) => self.facts.accepts_void(slot.ty),
                TypeAt::Run { element, tail } => {
                    self.facts.accepts_void(element)
                        || tail.iter().any(|s| self.facts.accepts_void(s.ty))
                }
                TypeAt::GenericRest { .. } | TypeAt::Absent => false,
            };
            if !accepts {
                break;
            }
            min -= 1;
        }
        min
    }

    /// Effective minimum as read by `mode`.
    #[must_use]
    pub fn effective_minimum(&self, mode: PositionalMode) -> usize {
        self.effective_minimum_with(mode.min_arity_flags())
    }

    /// Whether `argument_count` arguments satisfy the arity.
    #[must_use]
    pub fn accepts_argument_count(&self, argument_count: usize, mode: PositionalMode) -> bool {
        argument_count >= self.effective_minimum(mode)
            && self.max_arity().is_none_or(|max| argument_count <= max)
    }

    /// The type at argument position `pos`.
    #[must_use]
    pub fn type_at(&self, pos: usize) -> TypeAt<'a> {
        if let Some(slot) = self.layout.parameters.get(pos) {
            return TypeAt::One(*slot);
        }
        match &self.layout.rest {
            None => TypeAt::Absent,
            Some(rest) => {
                let index = pos - self.layout.parameters.len();
                match rest.kind {
                    RestKind::GenericTuple => TypeAt::GenericRest {
                        rest: rest.slot.ty,
                        index,
                    },
                    RestKind::Array if rest.tail.is_empty() => TypeAt::One(rest.slot),
                    RestKind::Array => TypeAt::Run {
                        element: rest.slot.ty,
                        tail: &rest.tail,
                    },
                }
            }
        }
    }

    /// Whether position `pos` may be omitted under `mode`.
    #[must_use]
    pub fn is_optional_at(&self, pos: usize, mode: PositionalMode) -> bool {
        pos >= self.effective_minimum(mode)
    }

    /// Project the parameters from `from` on as a tuple (parameter-tuple
    /// utilities and rest-inference read this).
    #[must_use]
    pub fn project_tuple(&self, from: usize, mode: PositionalMode) -> ProjectedTuple {
        let count = self.parameter_count();
        let min = self.effective_minimum(mode);
        if let Some(rest) = &self.layout.rest {
            if from + 1 >= count {
                return ProjectedTuple::Rest {
                    ty: rest.slot.ty,
                    exact: from + 1 == count,
                };
            }
        }
        let mut out = Vec::new();
        let fixed_end = self.layout.parameters.len();
        for pos in from..fixed_end {
            let slot = self.layout.parameters[pos];
            out.push(ProjectedElement {
                ty: slot.ty,
                kind: if pos < min {
                    ProjectedKind::Required
                } else {
                    ProjectedKind::Optional
                },
                name: slot.name,
            });
        }
        if let Some(rest) = &self.layout.rest {
            out.push(ProjectedElement {
                ty: rest.slot.ty,
                kind: ProjectedKind::Variadic,
                name: rest.slot.name,
            });
            for (i, slot) in rest.tail.iter().enumerate() {
                out.push(ProjectedElement {
                    ty: slot.ty,
                    kind: if fixed_end + 1 + i < min {
                        ProjectedKind::Required
                    } else {
                        ProjectedKind::Optional
                    },
                    name: slot.name,
                });
            }
        }
        ProjectedTuple::Elements(out)
    }

    /// Whether a SOURCE signature demands more arguments than a TARGET can
    /// supply (the arity half of signature comparison).
    #[must_use]
    pub fn source_has_more_parameters(
        source: &Self,
        target: &Self,
        strict_arity: bool,
        mode: PositionalMode,
    ) -> bool {
        if target.has_rest() {
            return false;
        }
        let target_count = target.parameter_count();
        if strict_arity {
            source.has_rest() || source.parameter_count() > target_count
        } else {
            source.effective_minimum(mode) > target_count
        }
    }

    /// Positional TYPE equality: tokens, optionality and rest shape. Names
    /// and the receiver's name never participate.
    #[must_use]
    pub fn types_equal(a: &Self, b: &Self) -> bool {
        fn strip(slot: &ParameterSlot) -> (TypeToken, ParameterOptionality) {
            (slot.ty, slot.optionality)
        }
        let same_slots = |x: &[ParameterSlot], y: &[ParameterSlot]| {
            x.len() == y.len() && x.iter().zip(y).all(|(p, q)| strip(p) == strip(q))
        };
        let rest_eq = match (&a.layout.rest, &b.layout.rest) {
            (None, None) => true,
            (Some(x), Some(y)) => {
                x.kind == y.kind && strip(&x.slot) == strip(&y.slot) && same_slots(&x.tail, &y.tail)
            }
            _ => false,
        };
        same_slots(&a.layout.parameters, &b.layout.parameters)
            && rest_eq
            && a.receiver.as_ref().map(strip) == b.receiver.as_ref().map(strip)
    }
}

/// The position kind of one tuple element, in the checker's element-flag
/// vocabulary. An array is one [`Self::Rest`] position.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub enum TupleElementKind {
    Required,
    Optional,
    /// `...X[]`: a rest element over an array.
    Rest,
    /// `...T`: a variadic element over a generic.
    Variadic,
}

impl TupleElementKind {
    const fn is_fixed(self) -> bool {
        matches!(self, Self::Required | Self::Optional)
    }
    const fn is_variable(self) -> bool {
        matches!(self, Self::Rest | Self::Variadic)
    }
}

/// Arity facts the plan reads for the generic side's variadic elements; the
/// plan itself never inspects types.
pub trait VariadicArityFacts {
    /// The arity the call's arguments imply for the variadic element at
    /// `index` — a bare rest type parameter's `impliedArity`.
    fn implied_arity(&self, index: usize) -> Option<usize>;
    /// The length of the variadic element's constraint at `index` when that
    /// constraint is a tuple with no rest or variadic element.
    fn fixed_constraint_arity(&self, index: usize) -> Option<usize>;
}

/// One inference a concrete array or tuple makes to a generic tuple. Every
/// index is a position of the side it names: `source` of the concrete
/// side, `target` of the generic side.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub enum TupleInferenceStep {
    /// The source element infers to the target element.
    Element { source: usize, target: usize },
    /// The source's single remaining rest element infers to the target
    /// element: its element type, or an array of it for a variadic target.
    RestElement { source: usize, target: usize },
    /// The source slice `[start, len - end)`, as a mutable tuple keeping
    /// labels, optionality and rest (`sliceTupleType`), infers to the
    /// variadic target element; `speculative` when the target ends in an
    /// optional element (`InferencePriority.SpeculativeTuple`).
    Slice {
        start: usize,
        end: usize,
        target: usize,
        speculative: bool,
    },
    /// The union of the element types of the source slice `[start, len -
    /// end)` infers to the rest target element's element type.
    SliceElementType {
        start: usize,
        end: usize,
        target: usize,
    },
}

/// The inferences a concrete `source` (a tuple, or an array when
/// `source_is_tuple` is false) makes to a generic tuple `target` — the
/// checker's tuple arm of `inferFromObjectTypes`. Positions pair from the
/// front over the shared fixed prefix and from the back over the shared
/// fixed suffix; the middle is inferred whole: a lone variadic takes the
/// slice, a lone rest the union of the slice, two variadics split at the
/// first's implied arity, and a variadic beside a rest splits at its fixed
/// constraint arity. A middle the plan cannot split infers nothing.
#[must_use]
pub fn tuple_inference_plan(
    source: &[TupleElementKind],
    source_is_tuple: bool,
    target: &[TupleElementKind],
    facts: &dyn VariadicArityFacts,
) -> Vec<TupleInferenceStep> {
    let source_arity = source.len();
    let target_arity = target.len();
    let mut steps = Vec::new();
    let structure_matches = source_arity == target_arity
        && source
            .iter()
            .zip(target)
            .all(|(s, t)| s.is_variable() == t.is_variable() && (*s == *t || !s.is_variable()));
    if source_is_tuple && structure_matches {
        steps.extend((0..target_arity).map(|i| TupleInferenceStep::Element {
            source: i,
            target: i,
        }));
        return steps;
    }
    let fixed_length =
        |kinds: &[TupleElementKind]| kinds.iter().take_while(|k| k.is_fixed()).count();
    let end_fixed =
        |kinds: &[TupleElementKind]| kinds.iter().rev().take_while(|k| k.is_fixed()).count();
    let target_has_variable = target.iter().any(|k| k.is_variable());
    let start = if source_is_tuple {
        fixed_length(source).min(fixed_length(target))
    } else {
        0
    };
    let end = if source_is_tuple && target_has_variable {
        end_fixed(source).min(end_fixed(target))
    } else {
        0
    };
    steps.extend((0..start).map(|i| TupleInferenceStep::Element {
        source: i,
        target: i,
    }));
    let middle_target_end = target_arity.saturating_sub(end);
    let single_source_rest =
        source_arity.checked_sub(start + end) == Some(1) && source[start] == TupleElementKind::Rest;
    if !source_is_tuple || single_source_rest {
        steps.extend(
            (start..middle_target_end).map(|target| TupleInferenceStep::RestElement {
                source: start,
                target,
            }),
        );
    } else {
        let slice =
            |start: usize, end: usize| (start + end <= source_arity).then_some((start, end));
        match target.get(start..middle_target_end) {
            Some([TupleElementKind::Variadic, TupleElementKind::Variadic]) => {
                if let Some(arity) = facts.implied_arity(start) {
                    let first = (end + source_arity)
                        .checked_sub(arity)
                        .and_then(|first_end| slice(start, first_end));
                    if let (Some((s0, e0)), Some((s1, e1))) = (first, slice(start + arity, end)) {
                        steps.push(TupleInferenceStep::Slice {
                            start: s0,
                            end: e0,
                            target: start,
                            speculative: false,
                        });
                        steps.push(TupleInferenceStep::Slice {
                            start: s1,
                            end: e1,
                            target: start + 1,
                            speculative: false,
                        });
                    }
                }
            }
            Some([TupleElementKind::Variadic, TupleElementKind::Rest]) => {
                if let Some(arity) = facts.fixed_constraint_arity(start) {
                    let first = source_arity
                        .checked_sub(start + arity)
                        .and_then(|first_end| slice(start, first_end));
                    if let Some((s0, e0)) = first {
                        steps.push(TupleInferenceStep::Slice {
                            start: s0,
                            end: e0,
                            target: start,
                            speculative: false,
                        });
                        if let Some((s1, e1)) =
                            slice(start + arity, end).filter(|(s, e)| s + e < source_arity)
                        {
                            steps.push(TupleInferenceStep::SliceElementType {
                                start: s1,
                                end: e1,
                                target: start + 1,
                            });
                        }
                    }
                }
            }
            Some([TupleElementKind::Rest, TupleElementKind::Variadic]) => {
                if let Some(arity) = facts.fixed_constraint_arity(start + 1) {
                    let end_index = source_arity.checked_sub(end_fixed(target));
                    let start_index = end_index.and_then(|index| index.checked_sub(arity));
                    if let (Some(end_index), Some(start_index)) = (end_index, start_index) {
                        if start_index >= start {
                            if let Some((s0, e0)) =
                                slice(start, end + arity).filter(|(s, e)| s + e < source_arity)
                            {
                                steps.push(TupleInferenceStep::SliceElementType {
                                    start: s0,
                                    end: e0,
                                    target: start,
                                });
                            }
                            steps.push(TupleInferenceStep::Slice {
                                start: start_index,
                                end: source_arity - end_index,
                                target: start + 1,
                                speculative: false,
                            });
                        }
                    }
                }
            }
            Some([TupleElementKind::Variadic]) => {
                if let Some((s, e)) = slice(start, end) {
                    steps.push(TupleInferenceStep::Slice {
                        start: s,
                        end: e,
                        target: start,
                        speculative: target.last() == Some(&TupleElementKind::Optional),
                    });
                }
            }
            Some([TupleElementKind::Rest]) => {
                if let Some((s, e)) = slice(start, end).filter(|(s, e)| s + e < source_arity) {
                    steps.push(TupleInferenceStep::SliceElementType {
                        start: s,
                        end: e,
                        target: start,
                    });
                }
            }
            _ => {}
        }
    }
    steps.extend((0..end).map(|i| TupleInferenceStep::Element {
        source: source_arity - i - 1,
        target: target_arity - i - 1,
    }));
    steps
}
