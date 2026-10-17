//! A diagnostic the checker raises for one type operation, and the recovery it
//! continues with — the payload of
//! [`QueryError::CheckerRecovery`](super::QueryError::CheckerRecovery).

use std::sync::Arc;

use super::{PrimitiveKind, SemanticNodeId};

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
/// reported in exactly two places, both owned by
/// [`checker_policy`](super::checker_policy): where one operation exhausts
/// Verter's own allowance for it — a [`RecoveryBasis::Budget`] recovery,
/// usable but incomplete, never kept, because the allowance rather than the
/// types decided it — and, for TS2589, where a sound proof certifies that
/// the operation can never reach a value — a [`RecoveryBasis::Certified`]
/// recovery, complete and kept like any semantic answer.
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
            | CheckerDiagnosticCode::CircularTypeAlias
            | CheckerDiagnosticCode::RecursiveFulfillmentCallback
            | CheckerDiagnosticCode::UnionTooComplex
            | CheckerDiagnosticCode::TupleTooLarge => Some(PrimitiveKind::Any),
            CheckerDiagnosticCode::TypeNotAssignable
            | CheckerDiagnosticCode::NoMatchingOverload
            | CheckerDiagnosticCode::ArgumentNotAssignable
            | CheckerDiagnosticCode::ArgumentCount
            | CheckerDiagnosticCode::ArgumentCountAtLeast
            | CheckerDiagnosticCode::RelationTooComplex => None,
        }
    }
}

/// The checker diagnostics an operation here can raise.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum CheckerDiagnosticCode {
    /// TS2769: several arity-fitting overloads reject the call.
    NoMatchingOverload,
    /// TS2322: a contextually typed callback's return is not assignable to
    /// the return its contextual signature requires.
    TypeNotAssignable,
    /// TS2589: an instantiation nested deeper than the checker's
    /// instantiation depth, or a conditional tail run longer than its tail
    /// limit ([`checker_policy`](super::checker_policy)) — including the lib
    /// `Awaited<T>` conditional re-entering an application it is still
    /// evaluating, whose instantiation never terminates — or nested past
    /// Verter's own instantiation budget.
    ExcessivelyDeepInstantiation,
    /// TS2456: a type alias whose declared type requires itself — its body
    /// applies the alias, directly or through other aliases, where the
    /// checker instantiates eagerly rather than deferring
    /// ([`checker_policy::circular_type_alias`](super::checker_policy::circular_type_alias)).
    /// A checker error, not a resource limit: the alias is `any`.
    CircularTypeAlias,
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

// Diagnostic codes and message templates, including TS2769 and TS2322:
// TypeScript v5.9.3, src/compiler/diagnosticMessages.json.
// https://github.com/microsoft/TypeScript/blob/v5.9.3/src/compiler/diagnosticMessages.json
impl CheckerDiagnosticCode {
    /// Whether the checker reports this code when a type operation runs out
    /// of an allowance (TS2589, TS2590, TS2799, TS2859). Verter reports these
    /// at its own allowance for the operation, as a resource partial, or —
    /// TS2589 only — at a certified divergence, as a complete answer.
    #[must_use]
    pub const fn is_resource_limit(self) -> bool {
        match self {
            Self::ExcessivelyDeepInstantiation
            | Self::UnionTooComplex
            | Self::TupleTooLarge
            | Self::RelationTooComplex => true,
            Self::CircularTypeAlias
            | Self::NoMatchingOverload
            | Self::TypeNotAssignable
            | Self::RecursiveFulfillmentCallback
            | Self::ArgumentNotAssignable
            | Self::ArgumentCount
            | Self::ArgumentCountAtLeast => false,
        }
    }

    /// The checker's numeric diagnostic code.
    #[must_use]
    pub const fn code(self) -> u32 {
        match self {
            Self::NoMatchingOverload => 2769,
            Self::TypeNotAssignable => 2322,
            Self::ExcessivelyDeepInstantiation => 2589,
            Self::CircularTypeAlias => 2456,
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
            Self::NoMatchingOverload => "No overload matches this call.",
            Self::TypeNotAssignable => "Type '{0}' is not assignable to type '{1}'.",
            Self::ExcessivelyDeepInstantiation => {
                "Type instantiation is excessively deep and possibly infinite."
            }
            Self::CircularTypeAlias => "Type alias '{0}' circularly references itself.",
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
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
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
    /// A type alias's declared type (`getDeclaredTypeOfTypeAlias`), whose
    /// body requires the alias itself.
    TypeAliasDeclaration,
}

/// Why a checker recovery is the answer: what the recovery proves.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum RecoveryBasis {
    /// The types decide the diagnostic, independent of any allowance: a
    /// diagnostic the checker derives from the types themselves, or a
    /// certified divergence — a sound proof that the operation can never
    /// reach a value. The recovery is a complete answer and may be kept.
    Certified,
    /// The operation exhausted Verter's own allowance for it. The recovery
    /// is usable but a resource partial: never complete, never kept.
    Budget,
}

/// The typed arguments a [`CheckerDiagnosticRecipe`] fills its code's
/// message template with, in the template's order. Types are graph nodes,
/// printed only when the recipe is bound to an occurrence.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum CheckerDiagnosticArguments {
    /// A message with no placeholder.
    None,
    /// `'{0}'` is the source type and `'{1}'` the target type of the
    /// relation that failed (TS2322, TS2345, TS2859).
    Types {
        source: SemanticNodeId,
        target: SemanticNodeId,
    },
    /// `{0}` is the arguments the only candidate accepts — `min`, or
    /// `min-max` when it accepts a range — and `{1}` the arguments the call
    /// supplies (TS2554). With no `max` the candidate has a rest parameter
    /// and `{0}` is its required count (TS2555).
    ArgumentCount {
        min: u32,
        max: Option<u32>,
        got: u32,
    },
}

impl CheckerDiagnosticArguments {
    /// Every node the arguments name.
    pub(crate) fn for_each_node(self, mut visit: impl FnMut(SemanticNodeId)) {
        match self {
            Self::None | Self::ArgumentCount { .. } => {}
            Self::Types { source, target } => {
                visit(source);
                visit(target);
            }
        }
    }
}

/// Where, relative to the operation that raised it, the checker reports a
/// diagnostic. A recipe names a position of its operation, never a source
/// offset: the same recipe, reused at another call site, is reported at that
/// site's own position ([`CheckerDiagnosticRecipe::bind`]).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum CheckerDiagnosticSubject {
    /// The operation itself; for a call, its callee
    /// (`getDiagnosticSpanForCallNode`).
    Operation,
    /// The call argument at this ordinal.
    Argument(u32),
    /// The call's arguments from this ordinal on — the ones its only
    /// candidate does not accept (`getArgumentArityError`).
    ExcessArguments(u32),
    /// The returned expression of the function argument at this ordinal,
    /// where the checker elaborates an argument failure onto the callback's
    /// return (`elaborateArrowFunction`).
    ArgumentReturn(u32),
}

/// A REUSABLE diagnostic: what the checker reports, with its typed
/// arguments and the position of its operation it is reported at. A recipe
/// carries no source location, so a cached call result's recipe is valid at
/// every call site that reuses it; binding it to one site makes a
/// [`CheckerDiagnosticOccurrence`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct CheckerDiagnosticRecipe {
    /// The diagnostic the checker reports.
    pub diagnostic: CheckerDiagnostic,
    /// The values its message template is filled with.
    pub arguments: CheckerDiagnosticArguments,
    /// The position of the operation it is reported at.
    pub subject: CheckerDiagnosticSubject,
}

impl CheckerDiagnosticRecipe {
    /// A call-resolution recipe.
    #[must_use]
    pub const fn call(
        code: CheckerDiagnosticCode,
        arguments: CheckerDiagnosticArguments,
        subject: CheckerDiagnosticSubject,
    ) -> Self {
        Self {
            diagnostic: CheckerDiagnostic {
                code,
                operation: CheckerDiagnosticOperation::CallResolution,
            },
            arguments,
            subject,
        }
    }

    /// The recipe's code.
    #[must_use]
    pub const fn code(&self) -> CheckerDiagnosticCode {
        self.diagnostic.code
    }

    /// Bind the recipe to one authored occurrence: its subject read off
    /// `site` and its type arguments printed by `print` — the occurrence's
    /// location and message, fixed for good. `None` when the site does not
    /// know the subject's position.
    pub(crate) fn bind(
        &self,
        site: &CheckerDiagnosticSite,
        print: impl Fn(SemanticNodeId, SemanticNodeId) -> (String, String),
    ) -> Option<CheckerDiagnosticOccurrence> {
        let start = match self.subject {
            CheckerDiagnosticSubject::Operation => site.operation_start?,
            CheckerDiagnosticSubject::Argument(ordinal)
            | CheckerDiagnosticSubject::ExcessArguments(ordinal) => {
                *site.argument_starts.get(ordinal as usize)?
            }
            CheckerDiagnosticSubject::ArgumentReturn(ordinal) => {
                (*site.argument_return_starts.get(ordinal as usize)?)?
            }
        };
        let arguments: Box<[Box<str>]> = match self.arguments {
            CheckerDiagnosticArguments::None => Box::new([]),
            CheckerDiagnosticArguments::Types { source, target } => {
                let (source, target) = print(source, target);
                Box::new([source.into_boxed_str(), target.into_boxed_str()])
            }
            CheckerDiagnosticArguments::ArgumentCount { min, max, got } => {
                let expected = match max {
                    Some(max) if max != min => format!("{min}-{max}"),
                    _ => min.to_string(),
                };
                Box::new([expected.into_boxed_str(), got.to_string().into_boxed_str()])
            }
        };
        Some(CheckerDiagnosticOccurrence {
            start,
            diagnostic: self.diagnostic,
            arguments,
        })
    }
}

/// The authored positions of one operation's occurrence that a recipe's
/// [`CheckerDiagnosticSubject`] reads: byte offsets into the occurrence's
/// file, each the start of the node the checker reports at.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct CheckerDiagnosticSite {
    /// The operation's own report position (a call's callee), when known.
    pub(crate) operation_start: Option<u32>,
    /// Each call argument's start, in argument order.
    pub(crate) argument_starts: Arc<[u32]>,
    /// Each function argument's returned expression's start, where the
    /// argument is a function returning one expression.
    pub(crate) argument_return_starts: Arc<[Option<u32>]>,
}

/// One diagnostic as the checker reports it at one authored position: the
/// byte offset into its file it starts at, its code, and its message
/// arguments as printed. It names no graph node, so it outlives every store.
/// Occurrences order as the checker sorts its diagnostics
/// (`compareDiagnostics`): by position, then by code, then by message.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct CheckerDiagnosticOccurrence {
    start: u32,
    diagnostic: CheckerDiagnostic,
    arguments: Box<[Box<str>]>,
}

impl Ord for CheckerDiagnosticOccurrence {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        self.start
            .cmp(&other.start)
            .then_with(|| self.code().code().cmp(&other.code().code()))
            .then_with(|| self.arguments.cmp(&other.arguments))
            .then_with(|| self.diagnostic.operation.cmp(&other.diagnostic.operation))
    }
}

impl PartialOrd for CheckerDiagnosticOccurrence {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        Some(self.cmp(other))
    }
}

impl CheckerDiagnosticOccurrence {
    /// The diagnostic reported.
    #[must_use]
    pub const fn diagnostic(&self) -> CheckerDiagnostic {
        self.diagnostic
    }

    /// The diagnostic's code.
    #[must_use]
    pub const fn code(&self) -> CheckerDiagnosticCode {
        self.diagnostic.code
    }

    /// The byte offset into the occurrence's file the diagnostic starts at.
    #[must_use]
    pub const fn start(&self) -> u32 {
        self.start
    }

    /// The printed values of the message's placeholders, in order.
    #[must_use]
    pub fn arguments(&self) -> &[Box<str>] {
        &self.arguments
    }

    /// The full message: the code's template with each `{n}` replaced by
    /// its argument.
    #[must_use]
    pub fn message(&self) -> String {
        let template = self.diagnostic.code.message();
        let mut message = String::with_capacity(template.len());
        let mut rest = template;
        while let Some(open) = rest.find('{') {
            message.push_str(&rest[..open]);
            let after = &rest[open + 1..];
            let filled = after.find('}').and_then(|close| {
                let index: usize = after[..close].parse().ok()?;
                Some((self.arguments.get(index)?, close))
            });
            match filled {
                Some((argument, close)) => {
                    message.push_str(argument);
                    rest = &after[close + 1..];
                }
                None => {
                    message.push('{');
                    rest = after;
                }
            }
        }
        message.push_str(rest);
        message
    }

    /// The heap bytes this occurrence keeps reachable beyond its inline size.
    pub(crate) fn heap_bytes(
        &self,
        aliases: &mut verter_session_query::retention::allocation::AliasSet,
    ) -> usize {
        verter_session_query::retention::owned::Measured::heap_bytes(&self.arguments, aliases)
    }
}
