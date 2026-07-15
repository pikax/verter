//! The matcher's tri-state verdict type — [`MatchCertainty`]
//! (`No`/`Maybe`/`Yes`), its three-valued `and`/`or` folds, and the
//! production [`MatchCertainty::might_match`] projection. Extracted from
//! the selector matcher; see the `matcher` module docs for the
//! fail-open/fail-closed rationale.

/// The matcher's INTERNAL three-valued verdict for one selector⇄template
/// decision — the tri-state behind the official boolean walk.
///
/// - [`Yes`](Self::Yes) — PROVABLY matches: every constraint was verified
///   against static template facts (a decoded static attribute value, a
///   static intrinsic tag, a proven neighborhood hop), or the construct is
///   DEFINITIONAL under the official scoping semantics (a bare `:global`, a
///   pseudo-element, a state pseudo-class, the all-global boundary fallback).
/// - [`No`](Self::No) — PROVABLY does not match (the official `false`).
/// - [`Maybe`](Self::Maybe) — CANNOT be proven either way: the official
///   FAIL-OPEN verdicts that used to collapse to `true` — the
///   `expression_possible_values` UNKNOWN bail, the exponential-combination
///   bail, a spread attribute, a `bind:`/`class:`/`style:` directive value, a
///   whitelisted runtime-toggled attribute (`details[open]`), a
///   `<svelte:element>` dynamic tag, an unevaluated fail-open selector
///   (`:not(...)`, `:nth-*`), the `:is(...)` descendant assumption, an
///   unknown combinator (`||`), a render-tag/component sibling, a
///   PROBABLY-existence sibling hop, and an enumerated possible-value set
///   (possible values are not a proof the matching branch is taken).
///
/// Certainty is computed lazily along the official evaluation order: a branch
/// the official walk short-circuits past is never evaluated, so a skipped
/// upgrade (e.g. an unevaluated all-global fallback behind an already-`Maybe`
/// verdict) conservatively stays `Maybe` — sound, never unsound `Yes`.
///
/// PRODUCTION behavior is untouched: every production consumer projects
/// through [`might_match`](Self::might_match) (`Yes | Maybe ⇒ true`,
/// `No ⇒ false`) — exactly the pre-tri-state boolean, in which `Maybe` WAS
/// `true`. `Maybe` is never treated as `No`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum MatchCertainty {
    /// Provably does not match.
    No,
    /// Cannot be proven either way — the official fail-open "may match
    /// anything" verdicts (kept used/scoped in production, exactly as the
    /// pre-tri-state `true`).
    Maybe,
    /// Provably matches.
    Yes,
}

impl MatchCertainty {
    /// Three-valued AND (= min): a compound/conjunction is only as certain as
    /// its weakest constraint; one proven-`No` constraint disproves it.
    #[must_use]
    pub fn and(self, other: Self) -> Self {
        self.min(other)
    }

    /// Three-valued OR (= max): one proven branch proves the disjunction; a
    /// `Maybe` branch keeps it undecided unless a `Yes` branch exists.
    #[must_use]
    pub fn or(self, other: Self) -> Self {
        self.max(other)
    }

    /// THE production projection: `Yes | Maybe ⇒ true`, `No ⇒ false` —
    /// byte-identical to the pre-tri-state boolean matcher (`Maybe` used to
    /// be `true`). Every used/scoped sink write projects through this.
    #[must_use]
    pub fn might_match(self) -> bool {
        self != Self::No
    }
}
