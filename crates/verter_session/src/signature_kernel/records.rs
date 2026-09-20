//! Signature input shapes, templates, descriptors, candidates, and recipes.
//!
//! Parameter optionality is an explicit field (effective `undefined` inclusion
//! under the active context). It is never reconstructed from a missing span.

#[cfg(target_pointer_width = "64")]
use std::mem::{align_of, size_of};

use crate::semantic_query::{
    OutcomeEvidenceId, ResultEvaluationContextId, SemanticContextId, CONTEXT_FREE_EVALUATION,
    CONTEXT_FREE_EVIDENCE,
};

/// Graph epoch. Zero is never issued.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug, PartialOrd, Ord)]
#[repr(transparent)]
pub struct GraphEpoch(u32);

impl GraphEpoch {
    pub const FIRST: Self = Self(1);

    #[must_use]
    pub const fn from_raw(raw: u32) -> Self {
        Self(raw)
    }

    #[must_use]
    pub const fn as_u32(self) -> u32 {
        self.0
    }

    #[must_use]
    pub const fn is_issued(self) -> bool {
        self.0 != 0
    }
}

/// Packed epoch-qualified intern handle: high 32 bits epoch, low 32 bits index.
#[inline]
pub(super) const fn pack_handle(epoch: GraphEpoch, index: u32) -> u64 {
    ((epoch.as_u32() as u64) << 32) | index as u64
}

#[inline]
pub(super) const fn handle_epoch(raw: u64) -> GraphEpoch {
    GraphEpoch::from_raw((raw >> 32) as u32)
}

#[inline]
pub(super) const fn handle_index(raw: u64) -> u32 {
    raw as u32
}

macro_rules! packed_id {
    ($name:ident) => {
        #[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
        #[repr(transparent)]
        pub struct $name(u64);

        impl $name {
            #[must_use]
            pub const fn from_raw(raw: u64) -> Self {
                Self(raw)
            }

            #[must_use]
            pub const fn as_u64(self) -> u64 {
                self.0
            }

            #[must_use]
            pub const fn epoch(self) -> GraphEpoch {
                handle_epoch(self.0)
            }

            #[must_use]
            pub const fn index(self) -> u32 {
                handle_index(self.0)
            }

            #[must_use]
            pub const fn is_unissued(self) -> bool {
                self.epoch().as_u32() == 0
            }

            #[must_use]
            pub(crate) const fn pack(epoch: GraphEpoch, index: u32) -> Self {
                Self(pack_handle(epoch, index))
            }
        }
    };
}

packed_id!(SignatureInputShapeId);
packed_id!(SignatureTemplateId);
packed_id!(SignatureDescriptorId);
packed_id!(SignatureProvenanceId);
packed_id!(SignatureSetId);
packed_id!(SignatureResultRecipeId);
packed_id!(BinderSpaceId);
packed_id!(DeclarationInstantiationId);
packed_id!(CallSubstitutionId);
packed_id!(ParameterLayoutId);
packed_id!(ParameterSlotId);
packed_id!(BodyLocatorId);
packed_id!(SpellingId);
packed_id!(ConstituentSequenceId);

/// Interned type-shape token. Not a source span, and not a
/// `SemanticNodeId`: the raw value lives in the kernel's own token
/// space and must never be reinterpreted as a semantic node ordinal.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
#[repr(transparent)]
pub struct TypeToken(u64);

impl TypeToken {
    #[must_use]
    pub const fn from_raw(raw: u64) -> Self {
        Self(raw)
    }

    #[must_use]
    pub const fn as_u64(self) -> u64 {
        self.0
    }
}

/// Call vs construct signature kind.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
#[repr(u8)]
pub enum SignatureKind {
    Call = 0,
    Construct = 1,
}

/// Context-independent declared-shape flags (literal specialization, etc.).
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug, Default)]
pub struct SignatureSemanticFlags(u16);

impl SignatureSemanticFlags {
    pub const NONE: Self = Self(0);
    pub const LITERAL_SPECIALIZATION: Self = Self(1 << 0);
    /// Untyped JavaScript signature: every parameter is optional unless a
    /// strong-arity read is requested.
    pub const UNTYPED_JS: Self = Self(1 << 1);

    #[must_use]
    pub const fn bits(self) -> u16 {
        self.0
    }

    #[must_use]
    pub const fn contains(self, other: Self) -> bool {
        self.0 & other.0 == other.0
    }

    #[must_use]
    pub const fn union(self, other: Self) -> Self {
        Self(self.0 | other.0)
    }
}

/// Effective parameter optionality under the active semantic context.
///
/// `includes_undefined` is the folded `undefined` inclusion. A missing
/// span is not an optionality encoding.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub struct ParameterOptionality {
    pub declared_optional: bool,
    pub includes_undefined: bool,
}

impl ParameterOptionality {
    #[must_use]
    pub const fn required() -> Self {
        Self {
            declared_optional: false,
            includes_undefined: false,
        }
    }

    #[must_use]
    pub const fn optional_with_undefined() -> Self {
        Self {
            declared_optional: true,
            includes_undefined: true,
        }
    }
}

/// One positional or rest slot. `name` is diagnostic/signature-help
/// metadata only: positional type equality never reads it.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub struct ParameterSlot {
    pub ty: TypeToken,
    pub optionality: ParameterOptionality,
    pub name: Option<SpellingId>,
}

impl ParameterSlot {
    #[must_use]
    pub const fn new(ty: TypeToken, optionality: ParameterOptionality) -> Self {
        Self {
            ty,
            optionality,
            name: None,
        }
    }
}

/// Whether a rest slot's type is a resolved array-like element run or a
/// still-uninstantiated type-parameter rest (one open inference position).
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub enum RestKind {
    Array,
    GenericTuple,
}

/// The rest run of a layout. `slot.ty` is the ELEMENT type for an array
/// rest; `tail` holds required/optional positions AFTER the run
/// (`[...string[], number]`). Fixed tuple rests are flattened into
/// `ParameterLayout::parameters` at construction and never appear here.
#[derive(Clone, PartialEq, Eq, Hash, Debug)]
pub struct RestSlot {
    pub slot: ParameterSlot,
    pub kind: RestKind,
    pub tail: Box<[ParameterSlot]>,
}

/// Ordered parameter layout, interned as a whole.
#[derive(Clone, PartialEq, Eq, Hash, Debug)]
pub struct ParameterLayout {
    pub parameters: Box<[ParameterSlot]>,
    pub rest: Option<RestSlot>,
}

/// One binder in a residual or declaration space: constraints and defaults.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub struct BinderDeclaration {
    pub spelling: SpellingId,
    pub constraint: Option<TypeToken>,
    pub default: Option<TypeToken>,
}

/// Explicit binder space. Same spelling in another space is a different binder:
/// `key` is the space identity (declaration-instantiation residual), not the
/// spelling of its binders.
#[derive(Clone, PartialEq, Eq, Hash, Debug)]
pub struct BinderSpace {
    pub key: u64,
    pub binders: Box<[BinderDeclaration]>,
}

/// Closed immutable result recipe. No transaction pointer, inference
/// context, or stack closure.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub enum SignatureResultRecipe {
    Declared {
        return_type: TypeToken,
        predicate_or_assertion: Option<TypeToken>,
    },
    Body {
        return_obligation_key: ReturnObligationKey,
    },
    UnionCommon {
        representative: TypeToken,
        constituents: ConstituentSequenceId,
    },
    UnionSynthesized {
        master: TypeToken,
        constituents: ConstituentSequenceId,
    },
    IntersectionConstruct {
        base_constructor: TypeToken,
        mixins: ConstituentSequenceId,
    },
}

/// Body-obligation identity. A body locator is a semantic dependency:
/// two different bodies never share a recipe merely because their
/// parameter shapes coincide.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub struct ReturnObligationKey {
    pub body_locator: BodyLocatorId,
    pub evaluation: ResultEvaluationContextId,
}

/// Declared input shape. Hot header of kind and context-independent facts.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub struct SignatureInputShape {
    pub kind: SignatureKind,
    pub binder_declarations: BinderSpaceId,
    pub this_parameter: Option<ParameterSlotId>,
    pub parameter_layout: ParameterLayoutId,
    pub declared_minimum: u16,
    pub signature_semantic_flags: SignatureSemanticFlags,
}

/// Template: one input shape plus one result recipe.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub struct SignatureTemplate {
    pub input_shape: SignatureInputShapeId,
    pub result_recipe: SignatureResultRecipeId,
}

/// Descriptor: a template placed in one declaration-instantiation environment
/// with a residual binder space. Instantiation is descriptor construction;
/// there is no second persistent instantiated-recipe map.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub struct SignatureDescriptor {
    pub template: SignatureTemplateId,
    pub declaration_environment: DeclarationInstantiationId,
    pub residual_binders: BinderSpaceId,
}

/// 16-byte candidate: descriptor handle plus provenance handle.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
#[repr(C)]
pub struct SignatureCandidate {
    pub signature: SignatureDescriptorId,
    pub provenance: SignatureProvenanceId,
}

/// Inline Empty | One | Many. 24 bytes on 64-bit targets.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub enum SignatureSetRef {
    Empty,
    One(SignatureCandidate),
    Many(SignatureSetId),
}

impl SignatureSetRef {
    #[must_use]
    pub const fn empty() -> Self {
        Self::Empty
    }

    #[must_use]
    pub const fn one(candidate: SignatureCandidate) -> Self {
        Self::One(candidate)
    }

    #[must_use]
    pub const fn many(id: SignatureSetId) -> Self {
        Self::Many(id)
    }
}

/// Interned many-set payload.
#[derive(Clone, PartialEq, Eq, Hash, Debug)]
pub struct SignatureSet {
    pub candidates: Box<[SignatureCandidate]>,
}

/// Applied result record: already in call space. Warm reads do not re-apply.
#[derive(Clone, PartialEq, Eq, Hash, Debug)]
pub struct AppliedResult {
    pub descriptor: SignatureDescriptorId,
    pub substitution: CallSubstitutionId,
    pub recipe: SignatureResultRecipeId,
    pub evaluation: ResultEvaluationContextId,
    pub semantic_context: SemanticContextId,
    pub evidence: OutcomeEvidenceId,
}

impl AppliedResult {
    /// Shape-only / context-free *evaluation* (reserved evaluation and
    /// evidence ids). `semantic_context` must be an interned identity —
    /// there is no reserved context sentinel, and id 0 is the first
    /// interned context.
    #[must_use]
    pub fn context_free(
        descriptor: SignatureDescriptorId,
        substitution: CallSubstitutionId,
        recipe: SignatureResultRecipeId,
        semantic_context: SemanticContextId,
    ) -> Self {
        Self {
            descriptor,
            substitution,
            recipe,
            evaluation: CONTEXT_FREE_EVALUATION,
            semantic_context,
            evidence: CONTEXT_FREE_EVIDENCE,
        }
    }
}

pub const LAYOUT_SIGNATURE_CANDIDATE: usize = 16;
pub const LAYOUT_SIGNATURE_SET_REF: usize = 24;
pub const LAYOUT_READY_SET: usize = 32;
pub const LAYOUT_QUERY_OUTCOME_SET: usize = 32;
/// Measured `MemoEntry` size on 64-bit. Pinned by `layouts_are_the_measured_64_bit_sizes`.
pub const LAYOUT_MEMO_ENTRY: usize = 160;

#[cfg(target_pointer_width = "64")]
const _: () = {
    assert!(size_of::<SignatureCandidate>() == LAYOUT_SIGNATURE_CANDIDATE);
    assert!(align_of::<SignatureCandidate>() == 8);
    assert!(size_of::<SignatureSetRef>() == LAYOUT_SIGNATURE_SET_REF);
    assert!(size_of::<crate::semantic_query::Ready<SignatureSetRef>>() == LAYOUT_READY_SET);
    assert!(
        size_of::<crate::semantic_query::QueryOutcome<SignatureSetRef>>()
            == LAYOUT_QUERY_OUTCOME_SET
    );
};
