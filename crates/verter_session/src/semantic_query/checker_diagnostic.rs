//! A diagnostic the checker raises for one type operation, and the recovery it
//! continues with — the payload of
//! [`QueryError::CheckerRecovery`](super::QueryError::CheckerRecovery).

use super::PrimitiveKind;

/// A checker diagnostic: its code and the operation that raised it.
///
/// The checker reports these and keeps going, with an answer the checker
/// itself defines — never a gap in this substrate filled with a guess. A
/// type operation continues with the checker's error type, which reads as
/// `any`: that continuation is the diagnostic's [`recovery`](Self::recovery),
/// and it rides `Opaque(QueryError::CheckerRecovery { .. })`, the error type of
/// the §22 lattice, so it dominates a union or an intersection exactly as the
/// checker's error type does. A call no candidate applies to continues with
/// the checker's error-recovery candidate instead
/// (`getCandidateForOverloadFailure`): the call's result carries that
/// candidate and this diagnostic together, so the answer is never a silent
/// success.
///
/// A resource diagnostic ([`CheckerDiagnosticCode::is_resource_limit`]) is
/// reported where one operation exhausts Verter's own allowance for it, and
/// only there: the operation's budget gate
/// ([`checker_policy`](super::checker_policy)) is its sole source. Its
/// recovery is usable but incomplete — a resource partial, never kept —
/// because the allowance, not the types, decided it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct CheckerDiagnostic {
    /// The diagnostic the checker reports.
    pub code: CheckerDiagnosticCode,
    /// The operation that raised it.
    pub operation: CheckerDiagnosticOperation,
}

impl CheckerDiagnostic {
    /// The error type the checker continues with after reporting this
    /// diagnostic, which prints and relates as `any`. `None` for a
    /// call-resolution diagnostic: that call continues with its
    /// error-recovery candidate, which the call's result carries. `None`
    /// for a relation diagnostic too: the relation is false.
    #[must_use]
    pub const fn recovery(self) -> Option<PrimitiveKind> {
        match self.code {
            CheckerDiagnosticCode::ExcessivelyDeepInstantiation
            | CheckerDiagnosticCode::RecursiveFulfillmentCallback
            | CheckerDiagnosticCode::UnionTooComplex
            | CheckerDiagnosticCode::TupleTooLarge => Some(PrimitiveKind::Any),
            CheckerDiagnosticCode::ArgumentNotAssignable
            | CheckerDiagnosticCode::ArgumentCount
            | CheckerDiagnosticCode::ArgumentCountAtLeast
            | CheckerDiagnosticCode::RelationTooComplex => None,
        }
    }
}

/// The checker diagnostics an operation here can raise.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum CheckerDiagnosticCode {
    /// TS2589: an instantiation nested deeper than the checker's
    /// instantiation depth, or a conditional tail run longer than its tail
    /// limit ([`checker_policy`](super::checker_policy)) — including the lib
    /// `Awaited<T>` conditional re-entering an application it is still
    /// evaluating, whose instantiation never terminates — or nested past
    /// Verter's own instantiation budget.
    ExcessivelyDeepInstantiation,
    /// TS2590: a cross product over unions has at least the checker's
    /// product limit of constituents, checked before one is built
    /// ([`checker_policy::cross_product_union`](super::checker_policy::cross_product_union)).
    UnionTooComplex,
    /// TS2799: splicing a tuple into a tuple type reaches the checker's
    /// tuple-size limit. Written in an expression, the checker reports the
    /// same limit as TS2800.
    TupleTooLarge,
    /// TS2859: one relation check recorded as many structured comparisons as
    /// the checker allows it
    /// ([`checker_policy::RelationComplexity`](super::checker_policy::RelationComplexity));
    /// the relation is false.
    RelationTooComplex,
    /// TS1062: the awaited-type relation reached a thenable whose promised
    /// value is a type it is already unwrapping.
    RecursiveFulfillmentCallback,
    /// TS2345: an argument is not assignable to its parameter in a call's
    /// only candidate. This is the applicability failure the checker
    /// reports; when the relation's elaboration reduces to one more
    /// specific message, the checker prints that message's code instead
    /// (a lone missing property is TS2741, several TS2739, an object
    /// literal's unknown property TS2353).
    ArgumentNotAssignable,
    /// TS2554: a call supplies more arguments than its only candidate
    /// accepts, or fewer than it requires where it has no rest parameter.
    ArgumentCount,
    /// TS2555: a call supplies fewer arguments than its only candidate,
    /// which has a rest parameter, requires.
    ArgumentCountAtLeast,
}

impl CheckerDiagnosticCode {
    /// Whether the checker reports this code when a type operation runs out
    /// of an allowance (TS2589, TS2590, TS2799, TS2859). Verter reports these
    /// at its own allowance for the operation, and the recovery after one is
    /// a resource partial: usable as the checker's recovery, never a
    /// complete answer, never kept.
    #[must_use]
    pub const fn is_resource_limit(self) -> bool {
        match self {
            Self::ExcessivelyDeepInstantiation
            | Self::UnionTooComplex
            | Self::TupleTooLarge
            | Self::RelationTooComplex => true,
            Self::RecursiveFulfillmentCallback
            | Self::ArgumentNotAssignable
            | Self::ArgumentCount
            | Self::ArgumentCountAtLeast => false,
        }
    }

    /// The checker's numeric diagnostic code.
    #[must_use]
    pub const fn code(self) -> u32 {
        match self {
            Self::ExcessivelyDeepInstantiation => 2589,
            Self::UnionTooComplex => 2590,
            Self::TupleTooLarge => 2799,
            Self::RelationTooComplex => 2859,
            Self::RecursiveFulfillmentCallback => 1062,
            Self::ArgumentNotAssignable => 2345,
            Self::ArgumentCount => 2554,
            Self::ArgumentCountAtLeast => 2555,
        }
    }

    /// The checker's message text for the code.
    #[must_use]
    pub const fn message(self) -> &'static str {
        match self {
            Self::ExcessivelyDeepInstantiation => {
                "Type instantiation is excessively deep and possibly infinite."
            }
            Self::UnionTooComplex => {
                "Expression produces a union type that is too complex to represent."
            }
            Self::TupleTooLarge => "Type produces a tuple type that is too large to represent.",
            Self::RelationTooComplex => "Excessive complexity comparing types '{0}' and '{1}'.",
            Self::RecursiveFulfillmentCallback => {
                "Type is referenced directly or indirectly in the fulfillment callback of its \
                 own 'then' method."
            }
            Self::ArgumentNotAssignable => {
                "Argument of type '{0}' is not assignable to parameter of type '{1}'."
            }
            Self::ArgumentCount => "Expected {0} arguments, but got {1}.",
            Self::ArgumentCountAtLeast => "Expected at least {0} arguments, but got {1}.",
        }
    }
}

/// The type operation that raised a [`CheckerDiagnostic`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum CheckerDiagnosticOperation {
    /// The lib `Awaited<T>` conditional over an authored `Awaited<X>`
    /// application.
    LibAwaited,
    /// The runtime awaited-type relation an `await` applies to its operand
    /// ([`SemanticQueryKey::AwaitedNormalize`](super::SemanticQueryKey::AwaitedNormalize)).
    AwaitOperand,
    /// The payload an async function's joined return is wrapped in
    /// ([`SemanticQueryKey::AsyncReturnPayload`](super::SemanticQueryKey::AsyncReturnPayload)).
    AsyncReturnPayload,
    /// The resolution of one call or `new` expression no candidate applies
    /// to ([`SemanticQueryKey::ResolveCall`](super::SemanticQueryKey::ResolveCall)).
    CallResolution,
    /// An intersection distributed over its union constituents
    /// (`getIntersectionType`).
    Intersection,
    /// A template literal type over union spans (`getTemplateLiteralType`).
    TemplateLiteral,
    /// An object spread over union operands (`getSpreadType`).
    ObjectSpread,
    /// A tuple spread into a tuple type (`createNormalizedTupleType`).
    TupleSpread,
    /// One tail run of a conditional alias (`getConditionalType`): its
    /// selected branch applying the alias again.
    ConditionalTail,
    /// One relation check (`checkTypeRelatedTo`).
    Relation,
    /// An instantiation nested past Verter's own instantiation budget,
    /// which lies far past the checker's depth limit.
    InstantiationBudget,
}
