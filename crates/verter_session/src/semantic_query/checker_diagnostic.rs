//! A diagnostic the checker raises for one type operation, and the recovery it
//! continues with — the payload of
//! [`QueryError::CheckerRecovery`](super::QueryError::CheckerRecovery).

use super::PrimitiveKind;

/// A checker diagnostic: its code and the operation that raised it.
///
/// The checker reports these and keeps going: the operation that raised one
/// continues with the checker's error type, which reads as `any`. That
/// continuation is the diagnostic's [`recovery`](Self::recovery) — an answer
/// the checker itself defines, never a gap in this substrate filled with a
/// guess. It rides `Opaque(QueryError::CheckerRecovery(..))`, the error type of
/// the §22 lattice, so it dominates a union or an intersection exactly as the
/// checker's error type does.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct CheckerDiagnostic {
    /// The diagnostic the checker reports.
    pub code: CheckerDiagnosticCode,
    /// The operation that raised it.
    pub operation: CheckerDiagnosticOperation,
}

impl CheckerDiagnostic {
    /// The type the checker continues with after reporting this diagnostic:
    /// its error type, which prints and relates as `any`.
    #[must_use]
    pub const fn recovery(self) -> PrimitiveKind {
        match self.code {
            CheckerDiagnosticCode::ExcessivelyDeepInstantiation
            | CheckerDiagnosticCode::RecursiveFulfillmentCallback => PrimitiveKind::Any,
        }
    }
}

/// The checker diagnostics an operation here can raise.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum CheckerDiagnosticCode {
    /// TS2589: the lib `Awaited<T>` conditional re-entered an application it
    /// is still evaluating, so its instantiation never terminates.
    ExcessivelyDeepInstantiation,
    /// TS1062: the awaited-type relation reached a thenable whose promised
    /// value is a type it is already unwrapping.
    RecursiveFulfillmentCallback,
}

impl CheckerDiagnosticCode {
    /// The checker's numeric diagnostic code.
    #[must_use]
    pub const fn code(self) -> u32 {
        match self {
            Self::ExcessivelyDeepInstantiation => 2589,
            Self::RecursiveFulfillmentCallback => 1062,
        }
    }

    /// The checker's message text for the code.
    #[must_use]
    pub const fn message(self) -> &'static str {
        match self {
            Self::ExcessivelyDeepInstantiation => {
                "Type instantiation is excessively deep and possibly infinite."
            }
            Self::RecursiveFulfillmentCallback => {
                "Type is referenced directly or indirectly in the fulfillment callback of its \
                 own 'then' method."
            }
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
}
