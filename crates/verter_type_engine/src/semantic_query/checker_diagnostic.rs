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
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, verter_retention::heap_size::HeapSize)]
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
            | CheckerDiagnosticCode::TupleTooLarge
            | CheckerDiagnosticCode::ExpressionTupleTooLarge => Some(PrimitiveKind::Any),
            CheckerDiagnosticCode::TypeNotAssignable
            | CheckerDiagnosticCode::NoMatchingOverload
            | CheckerDiagnosticCode::ArgumentNotAssignable
            | CheckerDiagnosticCode::ArgumentCount
            | CheckerDiagnosticCode::ArgumentCountAtLeast
            | CheckerDiagnosticCode::PropertyMissing
            | CheckerDiagnosticCode::PropertiesMissing
            | CheckerDiagnosticCode::ManyPropertiesMissing
            | CheckerDiagnosticCode::NoOverloadExpectsArgumentCount
            | CheckerDiagnosticCode::ExcessProperty
            | CheckerDiagnosticCode::ExcessPropertySuggestion
            | CheckerDiagnosticCode::RelationTooComplex => None,
        }
    }
}

/// The checker diagnostics an operation here can raise.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, verter_retention::heap_size::HeapSize)]
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
    /// TS2799: splicing a tuple into a tuple reaches the checker's
    /// tuple-size limit while a type node is checked
    /// ([`checker_policy::TupleSplicePosition`](super::checker_policy::TupleSplicePosition)).
    TupleTooLarge,
    /// TS2800: the same tuple-size limit reached while an expression is
    /// checked — an array literal spreading tuples as a tuple, or any type
    /// the expression's check instantiates, such as a generic call's
    /// declared return.
    ExpressionTupleTooLarge,
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
    /// TS2741: the elaboration of a relation an object source fails for
    /// lacking exactly one property its object target requires.
    PropertyMissing,
    /// TS2739: as [`Self::PropertyMissing`], lacking two to five.
    PropertiesMissing,
    /// TS2740: as [`Self::PropertyMissing`], lacking more than five; the
    /// message names the first four.
    ManyPropertiesMissing,
    /// TS2575: a call supplies a number of arguments between the fewest
    /// and the most its overloads take, which no overload takes.
    NoOverloadExpectsArgumentCount,
    /// TS2353: an object literal names a property its target does not have.
    ExcessProperty,
    /// TS2561: as [`Self::ExcessProperty`], where the target has a property
    /// whose name is a likely misspelling of it.
    ExcessPropertySuggestion,
}

// Diagnostic codes and message templates, including TS2769 and TS2322:
// TypeScript v5.9.3, src/compiler/diagnosticMessages.json.
// https://github.com/microsoft/TypeScript/blob/v5.9.3/src/compiler/diagnosticMessages.json
impl CheckerDiagnosticCode {
    /// Whether the checker reports this code when a type operation runs out
    /// of an allowance (TS2589, TS2590, TS2799, TS2800, TS2859). Verter reports these
    /// at its own allowance for the operation, as a resource partial, or —
    /// TS2589 only — at a certified divergence, as a complete answer.
    #[must_use]
    pub const fn is_resource_limit(self) -> bool {
        match self {
            Self::ExcessivelyDeepInstantiation
            | Self::UnionTooComplex
            | Self::TupleTooLarge
            | Self::ExpressionTupleTooLarge
            | Self::RelationTooComplex => true,
            Self::CircularTypeAlias
            | Self::NoMatchingOverload
            | Self::TypeNotAssignable
            | Self::RecursiveFulfillmentCallback
            | Self::ArgumentNotAssignable
            | Self::ArgumentCount
            | Self::ArgumentCountAtLeast
            | Self::PropertyMissing
            | Self::PropertiesMissing
            | Self::ManyPropertiesMissing
            | Self::NoOverloadExpectsArgumentCount
            | Self::ExcessProperty
            | Self::ExcessPropertySuggestion => false,
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
            Self::ExpressionTupleTooLarge => 2800,
            Self::RelationTooComplex => 2859,
            Self::RecursiveFulfillmentCallback => 1062,
            Self::ArgumentNotAssignable => 2345,
            Self::ArgumentCount => 2554,
            Self::ArgumentCountAtLeast => 2555,
            Self::PropertyMissing => 2741,
            Self::PropertiesMissing => 2739,
            Self::ManyPropertiesMissing => 2740,
            Self::NoOverloadExpectsArgumentCount => 2575,
            Self::ExcessProperty => 2353,
            Self::ExcessPropertySuggestion => 2561,
        }
    }

    /// The code's message with each `{n}` placeholder replaced by
    /// `arguments[n]`, as the checker formats it
    /// (`formatStringFromArgs`).
    #[must_use]
    pub fn format(self, arguments: &[impl AsRef<str>]) -> String {
        format_message(self.message(), arguments)
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
            Self::ExpressionTupleTooLarge => {
                "Expression produces a tuple type that is too large to represent."
            }
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
            Self::PropertyMissing => {
                "Property '{0}' is missing in type '{1}' but required in type '{2}'."
            }
            Self::PropertiesMissing => {
                "Type '{0}' is missing the following properties from type '{1}': {2}"
            }
            Self::ManyPropertiesMissing => {
                "Type '{0}' is missing the following properties from type '{1}': {2}, and {3} \
                 more."
            }
            Self::NoOverloadExpectsArgumentCount => {
                "No overload expects {0} arguments, but overloads do exist that expect either {1} \
                 or {2} arguments."
            }
            Self::ExcessProperty => {
                "Object literal may only specify known properties, and '{0}' does not exist in \
                 type '{1}'."
            }
            Self::ExcessPropertySuggestion => {
                "Object literal may only specify known properties, but '{0}' does not exist in \
                 type '{1}'. Did you mean to write '{2}'?"
            }
        }
    }
}

/// `template` with each `{n}` placeholder replaced by `arguments[n]`, as the
/// checker formats a message (`formatStringFromArgs`).
pub(crate) fn format_message(template: &str, arguments: &[impl AsRef<str>]) -> String {
    let mut message = String::with_capacity(template.len());
    let mut rest = template;
    while let Some(open) = rest.find('{') {
        message.push_str(&rest[..open]);
        let after = &rest[open + 1..];
        let filled = after.find('}').and_then(|close| {
            let index: usize = after[..close].parse().ok()?;
            Some((arguments.get(index)?.as_ref(), close))
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

/// The type operation that raised a [`CheckerDiagnostic`].
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, verter_retention::heap_size::HeapSize,
)]
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
    /// A tuple spread into a tuple, in a type or an expression
    /// (`createNormalizedTupleType`).
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
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, verter_retention::heap_size::HeapSize)]
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
    /// relation that failed (TS2322, TS2345, TS2859). Bound at an argument,
    /// the failure is elaborated as the checker elaborates it
    /// (`checkTypeRelatedToAndOptionallyElaborate`): into the authored
    /// argument's parts, or as one more specific message, with the
    /// relation's message chain below it.
    Types {
        source: SemanticNodeId,
        target: SemanticNodeId,
    },
    /// `{0}` is the arguments the candidates accept — `min`, or `min-max`
    /// when they accept a range — and `{1}` the arguments the call
    /// supplies (TS2554). With no `max` a candidate has a rest parameter
    /// and `{0}` is the fewest required (TS2555).
    ArgumentCount {
        min: u32,
        max: Option<u32>,
        got: u32,
    },
    /// `{0}` is the arguments the call supplies, `{1}` the most any
    /// overload taking fewer requires and `{2}` the fewest any overload
    /// taking more accepts (TS2575).
    ArgumentCountGap { got: u32, below: u32, above: u32 },
}

impl CheckerDiagnosticArguments {
    /// Every node the arguments name.
    pub(crate) fn for_each_node(self, mut visit: impl FnMut(SemanticNodeId)) {
        match self {
            Self::None | Self::ArgumentCount { .. } | Self::ArgumentCountGap { .. } => {}
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
    /// The authored call argument at this ordinal. Every argument a spread
    /// expands to is the spread's.
    Argument(u32),
    /// The call's arguments from the authored one at this ordinal on — the
    /// ones its candidates do not accept (`getArgumentArityError`).
    ExcessArguments(u32),
}

/// A REUSABLE diagnostic: what the checker reports, with its typed
/// arguments and the position of its operation it is reported at. A recipe
/// carries no source location, so a cached call result's recipe is valid at
/// every call site that reuses it; binding it to one site makes its
/// [`CheckerDiagnosticOccurrence`]s.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct CheckerDiagnosticRecipe {
    /// The diagnostic the checker reports.
    pub diagnostic: CheckerDiagnostic,
    /// The values its message template is filled with.
    pub arguments: CheckerDiagnosticArguments,
    /// The position of the operation it is reported at.
    pub subject: CheckerDiagnosticSubject,
    /// Whether the diagnostic is the error the last of several overloads
    /// gave, which the checker reports as TS2769 at that error's position,
    /// with `The last overload gave the following error.` and the error
    /// below its own message.
    pub last_overload: bool,
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
            last_overload: false,
        }
    }

    /// The recipe reported as the error the last of several overloads gave
    /// (TS2769).
    #[must_use]
    pub const fn as_last_overload(self) -> Self {
        Self {
            last_overload: true,
            ..self
        }
    }

    /// The recipe's code.
    #[must_use]
    pub const fn code(&self) -> CheckerDiagnosticCode {
        self.diagnostic.code
    }

    /// Bind the recipe to one authored occurrence of its operation: its
    /// subject read off `site` and its arguments printed and elaborated by
    /// `printer` — the occurrences' locations and messages, fixed for good.
    /// A relation failure the checker elaborates into several parts of an
    /// authored argument is several occurrences. `None` when the site does
    /// not record the subject, or the failure runs through a form or a
    /// relation whose elaboration is not modelled: a diagnostic with no
    /// position or message the checker's is never reported.
    pub(crate) fn bind(
        &self,
        site: &CheckerDiagnosticSite,
        printer: &dyn CheckerDiagnosticPrinter,
    ) -> Option<Vec<CheckerDiagnosticOccurrence>> {
        let argument = |ordinal: u32| site.arguments.get(ordinal as usize);
        let count = |n: u32| n.to_string().into_boxed_str();
        if self.last_overload {
            let failures = Self {
                last_overload: false,
                ..*self
            }
            .bind(site, printer)?;
            return Some(
                failures
                    .into_iter()
                    .map(|failure| {
                        let mut chain = Vec::with_capacity(failure.chain.len() + 2);
                        chain.push(CheckerDiagnosticChainLine::new(
                            1,
                            LAST_OVERLOAD_GAVE_THE_FOLLOWING_ERROR.into(),
                        ));
                        chain.push(CheckerDiagnosticChainLine::new(
                            2,
                            failure.message().into_boxed_str(),
                        ));
                        chain.extend(failure.chain.iter().map(|line| {
                            CheckerDiagnosticChainLine::new(
                                line.depth.saturating_add(2),
                                line.message.clone(),
                            )
                        }));
                        CheckerDiagnosticOccurrence::new(
                            failure.start,
                            failure.end,
                            CheckerDiagnostic {
                                code: CheckerDiagnosticCode::NoMatchingOverload,
                                operation: self.diagnostic.operation,
                            },
                            Box::new([]),
                            chain.into_boxed_slice(),
                        )
                    })
                    .collect(),
            );
        }
        let (span, arguments): ((u32, u32), Box<[Box<str>]>) = match self.arguments {
            CheckerDiagnosticArguments::Types { source, target } => {
                if let CheckerDiagnosticSubject::Argument(ordinal) = self.subject {
                    return printer.relation_failure(
                        source,
                        target,
                        self.diagnostic,
                        argument(ordinal)?,
                    );
                }
                let (source, target) = printer.relation_operands(source, target)?;
                (
                    self.subject_span(site)?,
                    Box::new([source.into_boxed_str(), target.into_boxed_str()]),
                )
            }
            CheckerDiagnosticArguments::None => (self.subject_span(site)?, Box::new([])),
            CheckerDiagnosticArguments::ArgumentCount { min, max, got } => {
                let expected = match max {
                    Some(max) if max != min => format!("{min}-{max}"),
                    _ => min.to_string(),
                };
                (
                    self.subject_span(site)?,
                    Box::new([expected.into_boxed_str(), count(got)]),
                )
            }
            CheckerDiagnosticArguments::ArgumentCountGap { got, below, above } => (
                self.subject_span(site)?,
                Box::new([count(got), count(below), count(above)]),
            ),
        };
        Some(vec![CheckerDiagnosticOccurrence::new(
            span.0,
            span.1,
            self.diagnostic,
            arguments,
            Box::new([]),
        )])
    }

    /// The start and end `site` records for the recipe's subject.
    fn subject_span(&self, site: &CheckerDiagnosticSite) -> Option<(u32, u32)> {
        match self.subject {
            CheckerDiagnosticSubject::Operation => site.operation_start.zip(site.operation_end),
            CheckerDiagnosticSubject::Argument(ordinal) => site
                .arguments
                .get(ordinal as usize)
                .map(|argument| (argument.point, argument.end)),
            CheckerDiagnosticSubject::ExcessArguments(ordinal) => {
                let first = site.arguments.get(ordinal as usize)?;
                let last = site.arguments.last()?;
                Some((first.point, last.end))
            }
        }
    }
}

/// The chain line TS2769 reports its last overload's error under.
const LAST_OVERLOAD_GAVE_THE_FOLLOWING_ERROR: &str = "The last overload gave the following error.";

/// What binding a recipe reads off the graph: its types as the checker
/// prints them in a message, and a failed relation as the checker reports
/// it.
pub(crate) trait CheckerDiagnosticPrinter {
    /// A failed relation's two types as its message prints them.
    fn relation_operands(
        &self,
        source: SemanticNodeId,
        target: SemanticNodeId,
    ) -> Option<(String, String)>;
    /// The occurrences the checker reports for `source` failing `target`
    /// at the authored expression `site` under `head` — elaborated into the
    /// expression's parts where its form allows, else one diagnostic at the
    /// expression with the relation's message chain
    /// (`checkTypeRelatedToAndOptionallyElaborate`). `None` when the
    /// failure runs through a form or a relation whose elaboration is not
    /// modelled.
    fn relation_failure(
        &self,
        source: SemanticNodeId,
        target: SemanticNodeId,
        head: CheckerDiagnostic,
        site: &verter_type_expr::IndexedReportSite,
    ) -> Option<Vec<CheckerDiagnosticOccurrence>>;
}

/// The authored positions of one operation's occurrence that a recipe's
/// [`CheckerDiagnosticSubject`] reads: byte offsets into the occurrence's
/// file, each the start of the node the checker reports at.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct CheckerDiagnosticSite {
    /// The operation's own report position (a call's callee).
    pub(crate) operation_start: Option<u32>,
    /// The end of the node `operation_start` starts.
    pub(crate) operation_end: Option<u32>,
    /// Each authored call argument's report site, in argument order.
    pub(crate) arguments: Arc<[verter_type_expr::IndexedReportSite]>,
}

/// One line of a diagnostic's message chain below its own message — the
/// relation step that explains it — at its depth in the chain (`1` directly
/// below the diagnostic's message), as `tsc --pretty false` prints it.
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct CheckerDiagnosticChainLine {
    depth: u16,
    message: Box<str>,
}

impl CheckerDiagnosticChainLine {
    pub(crate) fn new(depth: u16, message: Box<str>) -> Self {
        Self { depth, message }
    }

    /// The line's depth below the diagnostic's own message, from `1`.
    #[must_use]
    pub const fn depth(&self) -> u16 {
        self.depth
    }

    /// The line's message.
    #[must_use]
    pub fn message(&self) -> &str {
        &self.message
    }
}

/// One diagnostic as the checker reports it at one authored position: the
/// byte offset into its file it starts at, its code, its message arguments
/// as printed, and its message chain. It names no graph node, so it outlives
/// every store. Occurrences order as the checker sorts its diagnostics
/// (`compareDiagnostics`): by start, then span length, then code, then
/// message text.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct CheckerDiagnosticOccurrence {
    start: u32,
    end: u32,
    diagnostic: CheckerDiagnostic,
    arguments: Box<[Box<str>]>,
    chain: Box<[CheckerDiagnosticChainLine]>,
}

impl Ord for CheckerDiagnosticOccurrence {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        self.start
            .cmp(&other.start)
            .then_with(|| self.end.cmp(&other.end))
            .then_with(|| self.code().code().cmp(&other.code().code()))
            .then_with(|| self.full_message().cmp(&other.full_message()))
            .then_with(|| self.arguments.cmp(&other.arguments))
            .then_with(|| self.chain.cmp(&other.chain))
            .then_with(|| self.diagnostic.operation.cmp(&other.diagnostic.operation))
    }
}

impl PartialOrd for CheckerDiagnosticOccurrence {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        Some(self.cmp(other))
    }
}

impl CheckerDiagnosticOccurrence {
    pub(crate) fn new(
        start: u32,
        end: u32,
        diagnostic: CheckerDiagnostic,
        arguments: Box<[Box<str>]>,
        chain: Box<[CheckerDiagnosticChainLine]>,
    ) -> Self {
        Self {
            start,
            end,
            diagnostic,
            arguments,
            chain,
        }
    }

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

    /// The diagnostic's own message: the code's template with each `{n}`
    /// replaced by its argument.
    #[must_use]
    pub fn message(&self) -> String {
        self.diagnostic.code.format(&self.arguments)
    }

    /// The message chain below the diagnostic's own message, in order.
    #[must_use]
    pub fn message_chain(&self) -> &[CheckerDiagnosticChainLine] {
        &self.chain
    }

    /// The whole message as `tsc --pretty false` prints it: the
    /// diagnostic's own message, then each chain line on its own line,
    /// indented two spaces per depth.
    #[must_use]
    pub fn full_message(&self) -> String {
        let mut message = self.message();
        for line in self.chain.iter() {
            message.push('\n');
            for _ in 0..line.depth {
                message.push_str("  ");
            }
            message.push_str(&line.message);
        }
        message
    }

    /// The heap bytes this occurrence keeps reachable beyond its inline size.
    pub(crate) fn heap_bytes(
        &self,
        aliases: &mut verter_session_query::retention::allocation::AliasSet,
    ) -> usize {
        use verter_session_query::retention::allocation::heap_block_bytes;
        verter_session_query::retention::owned::Measured::heap_bytes(&self.arguments, aliases)
            + heap_block_bytes(self.chain.len() * std::mem::size_of::<CheckerDiagnosticChainLine>())
            + self
                .chain
                .iter()
                .map(|line| heap_block_bytes(line.message.len()))
                .sum::<usize>()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A printer naming each node by its ordinal and failing every
    /// relation as one TS2345 at the argument's report point, with the
    /// argument's elaborated form, when it has one, as a chain line.
    struct OrdinalPrinter;

    impl CheckerDiagnosticPrinter for OrdinalPrinter {
        fn relation_operands(
            &self,
            source: SemanticNodeId,
            target: SemanticNodeId,
        ) -> Option<(String, String)> {
            Some((format!("T{}", source.0), format!("T{}", target.0)))
        }

        fn relation_failure(
            &self,
            source: SemanticNodeId,
            target: SemanticNodeId,
            head: CheckerDiagnostic,
            site: &verter_type_expr::IndexedReportSite,
        ) -> Option<Vec<CheckerDiagnosticOccurrence>> {
            let (source, target) = self.relation_operands(source, target)?;
            Some(vec![CheckerDiagnosticOccurrence::new(
                site.point,
                site.end,
                head,
                Box::new([source.into_boxed_str(), target.into_boxed_str()]),
                Box::new([CheckerDiagnosticChainLine::new(1, "child".into())]),
            )])
        }
    }

    fn site(operation: u32, arguments: &[u32]) -> CheckerDiagnosticSite {
        CheckerDiagnosticSite {
            operation_start: Some(operation),
            operation_end: Some(operation + 1),
            arguments: arguments
                .iter()
                .map(|&point| verter_type_expr::IndexedReportSite {
                    point,
                    end: point + 1,
                    form: None,
                })
                .collect(),
        }
    }

    /// One reusable recipe, bound at two call sites, reports at each site's
    /// own position: the recipe carries a position of its operation, never
    /// a source offset, so a call result reused at another call is
    /// reported there.
    #[test]
    fn one_recipe_binds_at_each_site_it_is_reused_at() {
        let recipe = CheckerDiagnosticRecipe::call(
            CheckerDiagnosticCode::ArgumentNotAssignable,
            CheckerDiagnosticArguments::Types {
                source: SemanticNodeId(1),
                target: SemanticNodeId(2),
            },
            CheckerDiagnosticSubject::Argument(1),
        );
        let first = recipe.bind(&site(10, &[14, 17]), &OrdinalPrinter).unwrap();
        let second = recipe.bind(&site(40, &[44, 52]), &OrdinalPrinter).unwrap();
        assert_eq!(
            (first[0].start(), second[0].start()),
            (17, 52),
            "each binding reports at its own site's argument"
        );
        assert_eq!(
            first[0].full_message(),
            "Argument of type 'T1' is not assignable to parameter of type 'T2'.\n  child"
        );
        assert_eq!(first[0].full_message(), second[0].full_message());
    }

    /// TS2769 is reported where its last overload's error is, with
    /// `The last overload gave the following error.` and that error — its
    /// own chain one level deeper — below its message.
    #[test]
    fn a_last_overload_error_reports_as_ts2769_at_its_position() {
        let recipe = CheckerDiagnosticRecipe::call(
            CheckerDiagnosticCode::ArgumentNotAssignable,
            CheckerDiagnosticArguments::Types {
                source: SemanticNodeId(1),
                target: SemanticNodeId(2),
            },
            CheckerDiagnosticSubject::Argument(0),
        )
        .as_last_overload();
        let bound = recipe.bind(&site(10, &[14]), &OrdinalPrinter).unwrap();
        assert_eq!(bound[0].start(), 14);
        assert_eq!(bound[0].code(), CheckerDiagnosticCode::NoMatchingOverload);
        assert_eq!(
            bound[0].full_message(),
            "No overload matches this call.\n  The last overload gave the following error.\n    \
             Argument of type 'T1' is not assignable to parameter of type 'T2'.\n      child"
        );
    }

    /// An arity error spans every authored argument from the first excess
    /// one through the last (`getArgumentArityError`).
    #[test]
    fn excess_arguments_span_through_the_last_authored_argument() {
        let recipe = CheckerDiagnosticRecipe::call(
            CheckerDiagnosticCode::ArgumentCount,
            CheckerDiagnosticArguments::ArgumentCount {
                min: 1,
                max: Some(1),
                got: 3,
            },
            CheckerDiagnosticSubject::ExcessArguments(1),
        );
        let bound = recipe
            .bind(&site(10, &[14, 17, 20]), &OrdinalPrinter)
            .unwrap();
        assert_eq!((bound[0].start(), bound[0].end), (17, 21));
    }

    /// A subject the site does not record is not reported anywhere else.
    #[test]
    fn a_subject_the_site_does_not_record_binds_nothing() {
        let recipe = CheckerDiagnosticRecipe::call(
            CheckerDiagnosticCode::ArgumentCount,
            CheckerDiagnosticArguments::ArgumentCount {
                min: 1,
                max: Some(1),
                got: 3,
            },
            CheckerDiagnosticSubject::ExcessArguments(2),
        );
        assert_eq!(recipe.bind(&site(10, &[14, 17]), &OrdinalPrinter), None);
    }
}
