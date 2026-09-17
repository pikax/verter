//! Compiler-native type operations.
//!
//! These are operations the CHECKER performs itself — not declarations, and not
//! members of a generated catalog. `await x` computes an awaited type; it does
//! not instantiate a user-visible utility alias that happens to be spelled
//! `Awaited`. Modelling such an operation as a named reference re-admits string
//! identity into semantics and conflates a compiler intrinsic with a userland
//! declaration of the same name, so the operation carries a CLOSED identity
//! instead.
//!
//! Distinct from [`crate::intrinsics`], which is the generated HTML/static
//! intrinsic MEMBER catalog (attributes and event listeners). That module is
//! about catalogued member shapes; this one is about type-level operations the
//! compiler evaluates.
//!
//! The same identity is shared by both representations of an applied operation
//! — the semantic-graph node (`SemanticNodeData::IntrinsicApplication`) and the
//! materialized type expression ([`crate::TypeExpr::IntrinsicApplication`]) —
//! so there is ONE vocabulary and no conversion glue between layers.
//!
//! ## Invariant
//!
//! ```text
//! authored `Awaited<T>`              -> TypeExpr::Ref("Awaited", [T])  (the lib conditional)
//! compiler await over an open binder -> SemanticNodeData::IntrinsicApplication(Awaited, [T])
//! semantic materialisation           -> TypeExpr::IntrinsicApplication(Awaited, [T])
//! ```
//!
//! A `Ref("Awaited")` always means the authored lib alias — a declaration the
//! checker evaluates as its lib CONDITIONAL, even once its identity is proven.
//! An `IntrinsicApplication(Awaited)` means the compiler's own deferred awaited
//! operation (`await x`, an async generator's iteration parameters). The two
//! agree on promises and valid thenables but NOT on malformed ones
//! (`Awaited<{ then(): void }>` is `never`; awaiting that value is `any`),
//! so a resolved lib alias is never re-encoded as the intrinsic, nor the
//! intrinsic as the alias.

use verter_no_storedspan::NoStoredSpan;
use verter_no_typeexpr::NoTypeExpr;

/// A compiler-native type operation, identified by a closed variant rather than
/// by the spelling of any declaration.
///
/// Adding a variant here is a deliberate vocabulary change: it must gain a wire
/// string ([`Self::wire_str`] / [`Self::from_wire_str`]) and a display name
/// ([`Self::display_name`]), both of which are pinned by tests.
#[derive(
    Debug,
    Clone,
    Copy,
    PartialEq,
    Eq,
    Hash,
    PartialOrd,
    Ord,
    serde::Serialize,
    serde::Deserialize,
    NoTypeExpr,
    NoStoredSpan,
)]
#[serde(rename_all = "camelCase")]
pub enum CompilerIntrinsicTypeOp {
    /// The awaited type of an operand.
    ///
    /// The compiler's own awaited operation — what `await x` and an async
    /// generator's iteration parameters perform. NOT the lib `Awaited<T>`
    /// alias, which is a conditional type with different results on
    /// malformed thenables. Deferred (rather than reduced) exactly when the
    /// operand's thenability is not yet decidable — a binder-dependent
    /// operand such as an unconstrained `T`.
    Awaited,
}

impl CompilerIntrinsicTypeOp {
    /// Every operation, in declaration order. Lets exhaustiveness tests iterate
    /// the vocabulary without a wildcard.
    pub const ALL: &'static [CompilerIntrinsicTypeOp] = &[CompilerIntrinsicTypeOp::Awaited];

    /// The exact number of operands the operation takes. An application with
    /// any other count is malformed and is never minted as this operation.
    #[must_use]
    pub const fn arity(self) -> usize {
        match self {
            Self::Awaited => 1,
        }
    }

    /// The operation's rendered name — the spelling a reader expects
    /// (`Awaited<T>`). Display only: identity is the variant, never this string,
    /// and no lookup ever goes the other way from rendered text.
    #[must_use]
    pub const fn display_name(self) -> &'static str {
        match self {
            Self::Awaited => "Awaited",
        }
    }

    /// The stable wire token used by the hand-rolled JSON encoding. Kept
    /// separate from [`Self::display_name`] so a rendering change can never
    /// silently alter the serialized form.
    #[must_use]
    pub const fn wire_str(self) -> &'static str {
        match self {
            Self::Awaited => "awaited",
        }
    }

    /// The FROZEN tag folded into a content-addressed hash stream.
    ///
    /// Deliberately NOT the derived enum discriminant: a derive encodes
    /// DECLARATION ORDER, so reordering or inserting a variant would silently
    /// change every cache key that ever hashed an intrinsic application. This
    /// tag is append-only and independent of both [`Self::display_name`] and
    /// [`Self::wire_str`] — a rendering or wire change must never move it.
    #[must_use]
    pub const fn stable_hash_tag(self) -> u8 {
        match self {
            Self::Awaited => 0,
        }
    }

    /// Parse a wire token. `None` for an unrecognised op, so a payload from a
    /// newer producer fails honestly rather than decoding into the wrong
    /// operation.
    #[must_use]
    pub fn from_wire_str(value: &str) -> Option<Self> {
        match value {
            "awaited" => Some(Self::Awaited),
            _ => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::CompilerIntrinsicTypeOp;

    #[test]
    fn wire_tokens_round_trip_and_are_distinct() {
        let mut seen: Vec<&'static str> = Vec::new();
        for op in CompilerIntrinsicTypeOp::ALL {
            let wire = op.wire_str();
            assert_eq!(
                CompilerIntrinsicTypeOp::from_wire_str(wire),
                Some(*op),
                "`{wire}` must decode back to the op that produced it"
            );
            assert!(
                !seen.contains(&wire),
                "wire token `{wire}` is used by two operations"
            );
            seen.push(wire);
        }
    }

    #[test]
    fn an_unknown_wire_token_fails_rather_than_guessing() {
        assert_eq!(CompilerIntrinsicTypeOp::from_wire_str("uppercase"), None);
        assert_eq!(CompilerIntrinsicTypeOp::from_wire_str(""), None);
    }

    #[test]
    fn arity_is_pinned_per_operation() {
        for op in CompilerIntrinsicTypeOp::ALL {
            let expected = match op {
                CompilerIntrinsicTypeOp::Awaited => 1,
            };
            assert_eq!(op.arity(), expected, "{op:?}");
        }
    }

    #[test]
    fn stable_hash_tags_are_unique_and_independent_of_declaration_order() {
        let mut seen: Vec<u8> = Vec::new();
        for op in CompilerIntrinsicTypeOp::ALL {
            let tag = op.stable_hash_tag();
            assert!(
                !seen.contains(&tag),
                "stable hash tag {tag} is used by two operations — a collision \
                 silently conflates them in every content-addressed key"
            );
            seen.push(tag);
        }
    }

    #[test]
    fn display_name_is_not_the_wire_token() {
        // The two are deliberately separate surfaces: the wire form is stable
        // and lowercase, the display form is the authored-looking spelling.
        assert_eq!(CompilerIntrinsicTypeOp::Awaited.display_name(), "Awaited");
        assert_eq!(CompilerIntrinsicTypeOp::Awaited.wire_str(), "awaited");
    }
}
