//! Cycle-guard normalized type-argument identity.
//!
//! Recursion guards inside the policy walker (e.g. `rewrite_ref`'s active-ref
//! stack) key on `(DeclIdentity, NormalizedTypeArgs)`. Bare-name strings
//! collide across scopes (two `Foo`s in different files) and cannot
//! distinguish `Pick<X, 'a'>` from `Pick<X, 'b'>` inside a generic
//! instantiation chain. The active-refs set on `PolicyCtx` consumes these
//! types; `NormalizedTypeArgs::normalize` is the constructor that the cycle
//! guard uses to discriminate generic instantiations.

use smallvec::SmallVec;
use verter_type_expr::{LiteralValue, TypeExpr};

use crate::semantic_query::DeclIdentity;

use super::core::{DeclLookup, PolicyCtx};

/// Hash of an opaque anonymous shape used to discriminate cycle-guard keys
/// across structurally-identical inline type expressions. Phases that need
/// to distinguish anonymous shapes attach a stable hash; identity equality
/// of the hash is what `NormalizedTypeArg` checks.
pub(crate) type ShapeHash = u64;

/// Hash of a `LiteralValue` used as a cheap discriminator for literal-arg
/// cycle-guard keys (e.g. `Pick<X, 'a'>` vs `Pick<X, 'b'>`).
pub(crate) type LiteralHash = u64;

/// One normalized type argument for the cycle-guard key. The four variants
/// cover every shape the cycle guard observes:
///
/// - `Decl(DeclIdentity)` — a named declaration reference (resolved by the
///   resolver). Two args resolve to the same `DeclIdentity` when their
///   bare-name lookups land on the same declaration.
/// - `Literal(LiteralHash)` — a literal value (`'a'`, `42`, `true`).
///   Different literal values produce different hashes, so `Pick<X, 'a'>`
///   and `Pick<X, 'b'>` produce different `NormalizedTypeArgs`.
/// - `AnonymousShape(ShapeHash)` — an inline anonymous shape that has no
///   declaration identity (e.g. `{ a: string }` passed inline as a type
///   argument). The hash carries enough structural information to
///   distinguish unrelated inline shapes.
/// - `None` — empty / missing argument slot. Reserved for ambient
///   reductions where the caller wants to discriminate between "no arg"
///   and "a real arg that happens to hash to zero".
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub(crate) enum NormalizedTypeArg {
    Decl(DeclIdentity),
    Literal(LiteralHash),
    AnonymousShape(ShapeHash),
    None,
}

/// Normalized form of a type-argument list, used as the second component
/// of cycle-guard keys (the first is the resolved `DeclIdentity` of the
/// declaration being entered).
///
/// Normalization is deterministic: two argument lists that resolve to the
/// same declaration identities and literal values produce the same
/// `NormalizedTypeArgs` value, regardless of the syntactic shape of the
/// original `TypeExpr` arguments. The ordering of arguments IS preserved
/// (positional arguments) — different positions are different keys.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub(crate) struct NormalizedTypeArgs(SmallVec<[NormalizedTypeArg; 4]>);

impl NormalizedTypeArgs {
    /// Build an empty `NormalizedTypeArgs`. Used as the cycle-guard key
    /// for a declaration entered with no type arguments.
    #[must_use]
    pub(crate) fn empty() -> Self {
        Self(SmallVec::new())
    }

    /// Build a `NormalizedTypeArgs` from an iterator of normalized
    /// arguments. Caller is responsible for resolving each input
    /// `TypeExpr` to its `NormalizedTypeArg` (the resolver-aware
    /// constructor lives in the bundle that consumes this type).
    #[allow(dead_code)]
    #[must_use]
    pub(crate) fn from_normalized<I>(args: I) -> Self
    where
        I: IntoIterator<Item = NormalizedTypeArg>,
    {
        Self(args.into_iter().collect())
    }

    /// Resolver-aware constructor — normalizes a slice of `TypeExpr`
    /// arguments into the cycle-guard key form. Each input maps to one
    /// `NormalizedTypeArg`:
    ///
    /// * `Ref { name, args }` resolves through the policy's declaration
    ///   lookup and produces `Decl(DeclIdentity)` keyed on the resolved
    ///   declaration's canonical source. Refs the resolver cannot
    ///   locate fall back to `AnonymousShape(hash)` so unresolved
    ///   references still discriminate by name.
    /// * `Literal(v)` produces `Literal(hash_literal(v))` so
    ///   `Pick<X, 'a'>` and `Pick<X, 'b'>` produce distinct keys.
    /// * `Infer` / unknown-shape arguments produce
    ///   `AnonymousShape(structural_hash)` — a stable hash of the
    ///   `TypeExpr` (via its shared depth-safe iterative `Hash`) so
    ///   structurally-identical inline shapes share an identity but
    ///   distinct shapes do not collide.
    ///
    /// The function is `&mut PolicyCtx` because resolving a `Ref`
    /// argument may consult the same declaration cache the cycle guard
    /// itself uses.
    #[must_use]
    pub(super) fn normalize(args: &[TypeExpr], ctx: &mut PolicyCtx<'_, '_>) -> Self {
        let mut out: SmallVec<[NormalizedTypeArg; 4]> = SmallVec::new();
        for arg in args {
            out.push(normalize_one_arg(arg, ctx));
        }
        Self(out)
    }

    /// Number of arguments (zero for `empty`).
    #[allow(dead_code)]
    #[must_use]
    pub(crate) fn len(&self) -> usize {
        self.0.len()
    }

    /// True when no arguments are present.
    #[allow(dead_code)]
    #[must_use]
    pub(crate) fn is_empty(&self) -> bool {
        self.0.is_empty()
    }

    /// Iterator over the normalized arguments in positional order.
    #[allow(dead_code)]
    pub(crate) fn iter(&self) -> impl Iterator<Item = &NormalizedTypeArg> {
        self.0.iter()
    }
}

impl Default for NormalizedTypeArgs {
    fn default() -> Self {
        Self::empty()
    }
}

/// Peel off `Parenthesized` wrappers iteratively, returning the first
/// non-parenthesised inner type. A deeply-parenthesised argument
/// (`((((T))))`) would otherwise recurse once per layer in
/// [`normalize_one_arg`] and overflow the stack on a pathological depth —
/// the same hazard class the `TypeExpr` iterative `Drop`/`Hash` removed.
fn strip_parentheses(mut arg: &TypeExpr) -> &TypeExpr {
    while let TypeExpr::Parenthesized(inner) = arg {
        arg = inner;
    }
    arg
}

/// Resolve one `TypeExpr` argument into its `NormalizedTypeArg` shape.
fn normalize_one_arg(arg: &TypeExpr, ctx: &mut PolicyCtx<'_, '_>) -> NormalizedTypeArg {
    let arg = strip_parentheses(arg);
    match arg {
        TypeExpr::Ref { name, .. } => {
            // Try to resolve the ref to a declaration identity. When
            // the lookup succeeds the canonical source uniquely keys
            // the argument; otherwise fall back to a structural hash
            // so the cycle guard still discriminates by name.
            if let Some(DeclLookup {
                canonical_source, ..
            }) = ctx.locate_declaration(name.as_ref())
            {
                NormalizedTypeArg::Decl(ctx.decl_identity_for(&canonical_source, name.as_ref()))
            } else {
                NormalizedTypeArg::AnonymousShape(hash_type_expr(arg))
            }
        }
        TypeExpr::Literal(value) => NormalizedTypeArg::Literal(hash_literal(value)),
        TypeExpr::Infer { .. } => NormalizedTypeArg::None,
        _ => NormalizedTypeArg::AnonymousShape(hash_type_expr(arg)),
    }
}

/// Compute a deterministic 64-bit hash of a `TypeExpr`. The structural
/// `Hash` impl on `TypeExpr` is stable across builds because all
/// component hashers are deterministic; it is the shared, depth-safe
/// continuation-frame iterative walker (NOT a recursive derive), so this
/// routes a deeply-nested `TypeExpr` through `Hash` without risking stack
/// overflow. The digest goes through `xxh3` to match the shape used
/// elsewhere in the cycle guard's identity space.
fn hash_type_expr(expr: &TypeExpr) -> ShapeHash {
    use std::hash::Hash;
    let mut hasher = xxhash_rust::xxh3::Xxh3::new();
    let mut bridge = LiteralHashBridge(&mut hasher);
    expr.hash(&mut bridge);
    hasher.digest()
}

/// Stable hash of a `LiteralValue` for use inside `NormalizedTypeArg::Literal`.
/// Mirrors the manual `Hash` impl on `LiteralValue` — same input bytes
/// produce the same digest across builds.
#[must_use]
pub(crate) fn hash_literal(value: &LiteralValue) -> LiteralHash {
    use std::hash::Hash;
    let mut hasher = xxhash_rust::xxh3::Xxh3::new();
    let mut bridge = LiteralHashBridge(&mut hasher);
    value.hash(&mut bridge);
    hasher.digest()
}

struct LiteralHashBridge<'a>(&'a mut xxhash_rust::xxh3::Xxh3);

impl<'a> std::hash::Hasher for LiteralHashBridge<'a> {
    fn finish(&self) -> u64 {
        self.0.digest()
    }

    fn write(&mut self, bytes: &[u8]) {
        self.0.update(bytes);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;

    /// `strip_parentheses` peels a deeply-parenthesised argument
    /// iteratively and MUST NOT overflow the stack on a pathological
    /// depth.
    ///
    /// Discrimination: the former `normalize_one_arg` recursed on the
    /// `Parenthesized` arm (`normalize_one_arg(inner, ctx)`); a chain
    /// this deep overflowed the stack on a default thread stack. The
    /// iterative `while let` peeling returns in O(1) stack.
    ///
    /// (The build/drop of this deep `TypeExpr` is itself depth-safe via
    /// `TypeExpr`'s iterative `Drop`.)
    #[test]
    fn strip_parentheses_handles_deeply_nested_without_stack_overflow() {
        const DEPTH: usize = 200_000;
        let mut arg = TypeExpr::named("Inner");
        for _ in 0..DEPTH {
            arg = TypeExpr::Parenthesized(Arc::new(arg));
        }

        let stripped = strip_parentheses(&arg);
        // The peeled result is the innermost non-parenthesised type.
        assert!(
            matches!(stripped, TypeExpr::Ref { name, .. } if name.as_ref() == "Inner"),
            "strip_parentheses must return the innermost type, got {stripped:?}",
        );
    }

    /// A non-parenthesised argument is returned unchanged.
    #[test]
    fn strip_parentheses_passes_through_non_parenthesised() {
        let arg = TypeExpr::Primitive(verter_type_expr::PrimitiveName::String);
        let stripped = strip_parentheses(&arg);
        assert!(matches!(
            stripped,
            TypeExpr::Primitive(verter_type_expr::PrimitiveName::String)
        ));
    }
}
