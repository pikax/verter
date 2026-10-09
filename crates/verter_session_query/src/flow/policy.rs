//! Flow lowering policy and the gap vocabulary a lowering records where it stops modelling.

use std::sync::Arc;

/// One type-parameter clause enclosing a class expression.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct ClassExpressionClause {
    /// The name the checker prints for the clause's declaration when it
    /// qualifies a reference: `outer` for a function, `Holder.make` for a
    /// class method, `o.m` for a method of an object literal a variable
    /// holds, `arrow` for the arrow a variable holds, `GHolder` for a
    /// class.
    pub container: Arc<str>,
    /// The clause's type parameters, in declaration order.
    pub parameters: Arc<[Arc<str>]>,
}

/// The `null` / `undefined` algebra one union is constructed under —
/// TypeScript's `strictNullChecks`, carried as a construction input
/// rather than read from an ambient setting.
///
/// With `strictNullChecks` off, `null` and `undefined` inhabit every type,
/// so the checker's `getUnionType` never adds a nullable member to a
/// union's type set: `string | null` IS `string`, and a union made only of
/// nullable members is `null` when it names `null`, else `undefined`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum NullabilityPolicy {
    /// `strictNullChecks` on (TypeScript's default): `null` and
    /// `undefined` are ordinary union members.
    Strict,
    /// `strictNullChecks` off: a union erases its `null` / `undefined`
    /// members whenever any other member remains.
    Erased,
}

impl NullabilityPolicy {
    /// The policy a project's effective `strictNullChecks` selects.
    #[must_use]
    pub fn from_strict_null_checks(strict_null_checks: bool) -> Self {
        if strict_null_checks {
            Self::Strict
        } else {
            Self::Erased
        }
    }

    /// Whether `null` / `undefined` are their own types (`strictNullChecks`
    /// on).
    #[must_use]
    pub fn is_strict(self) -> bool {
        matches!(self, Self::Strict)
    }
}

/// The behavioral policy axes of a `FlowReturn` query: the options of the
/// function's OWN project that change what its body infers. The values are
/// also folded into the context's `type_env_hash`; stating them here is
/// what makes the key say which semantics it hashes, and it is the one
/// place the evaluator reads them from.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct FlowReturnPolicy {
    /// `strictNullChecks` of the project owning the function: whether
    /// an optional parameter, an optional member read, an optional chain
    /// or a fall-through adds `undefined`, and which union algebra every
    /// join of the body runs.
    pub nullability: NullabilityPolicy,
    /// `noImplicitAny` of the project owning the function: whether an
    /// unannotated `let` / `var` with no initializer or a bare `null` /
    /// `undefined` one is the checker's AUTO-TYPED variable (its type
    /// follows its assignments) or is declared as its initializer's
    /// widened type (`any` with no initializer).
    pub no_implicit_any: bool,
    /// `useUnknownInCatchVariables` of the project owning the function:
    /// whether an unannotated `catch` variable is `unknown` or `any`.
    pub use_unknown_in_catch_variables: bool,
    /// `noImplicitThis` of the project owning the function: whether an
    /// object literal's method or accessor reads `this` as the literal
    /// (`getContextualThisParameterType`) or, with the option off, as
    /// `any`.
    pub no_implicit_this: bool,
}

impl FlowReturnPolicy {
    /// The flow policy one project's effective compiler options select.
    #[must_use]
    pub fn from_compiler_options(options: &crate::resolution::SemanticCompilerOptions) -> Self {
        Self {
            nullability: NullabilityPolicy::from_strict_null_checks(options.strict_null_checks),
            no_implicit_any: options.no_implicit_any,
            use_unknown_in_catch_variables: options.use_unknown_in_catch_variables,
            no_implicit_this: options.no_implicit_this,
        }
    }
}

/// A detected flow-model gap whose final semantic owner remains external to
/// the flow-return substrate.
#[repr(u8)]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum FlowGap {
    GuardNarrowing,
    NominalRelation,
    ClosureCapture,
    AbruptCompletion,
    UnmodeledExpression,
}
