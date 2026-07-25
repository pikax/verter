//! The opaque [`TypeExpr::Unknown`](crate::TypeExpr::Unknown) payload:
//! [`UnknownValue`] + [`UnknownProvenance`]. Split from the crate root for
//! file-size hygiene (the same pattern as `display` / `type_expr_json`).

use std::hash::{Hash, Hasher};
use std::sync::Arc;

/// Where an [`UnknownValue`] spelling came from. DIAGNOSTIC ONLY: provenance
/// is NEVER part of equality, hashing, the JSON wire shape, or any control
/// decision — two `UnknownValue`s with the same raw text are identical
/// regardless of provenance.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum UnknownProvenance {
    /// Authored syntax the lowering does not represent (unsupported / invalid
    /// TypeScript type syntax preserved as its source text).
    UnsupportedSyntax,
    /// A JSDoc type expression that failed to parse, preserved as its raw
    /// comment text.
    JsdocParseFallback,
    /// A producer that had no output to emit (raw is empty).
    MissingOutput,
    /// An `Unknown` decoded from the JSON wire format (`{"kind":"unknown"}`),
    /// or interned externally as opaque raw text.
    WireOpaque,
    /// A terminal COMPATIBILITY PROJECTION of a typed resolver degradation
    /// (`QueryError`) — the session materialization sidecar keeps the typed
    /// reason; this spelling exists only so wire/display/hash bytes stay
    /// identical to the legacy raw-sentinel encoding.
    CompatibilityProjection,
}

/// The opaque payload of [`TypeExpr::Unknown`](crate::TypeExpr::Unknown):
/// genuinely unrepresentable authored/raw type syntax, carried as its source
/// text plus a diagnostic [`UnknownProvenance`].
///
/// BOTH FIELDS ARE PRIVATE: there is NO public generic `new(raw)` and NO
/// public raw field — construction goes through one of the named provenance
/// constructors, so a bare string can never be smuggled in as an `Unknown`
/// without declaring what it is. Equality and hashing are RAW-ONLY
/// (provenance is diagnostic, never identity, never a control discriminator),
/// preserving the legacy `Unknown { raw: String }` cache/hash behaviour.
#[derive(Debug, Clone)]
pub struct UnknownValue {
    raw: Arc<str>,
    provenance: UnknownProvenance,
}

impl UnknownValue {
    /// Authored syntax the lowering could not represent, preserved verbatim.
    pub fn unsupported_syntax(raw: impl Into<Arc<str>>) -> Self {
        Self {
            raw: raw.into(),
            provenance: UnknownProvenance::UnsupportedSyntax,
        }
    }

    /// A JSDoc type expression that failed to parse, preserved verbatim.
    pub fn jsdoc_parse_fallback(raw: impl Into<Arc<str>>) -> Self {
        Self {
            raw: raw.into(),
            provenance: UnknownProvenance::JsdocParseFallback,
        }
    }

    /// A producer that had no output to emit (empty raw).
    pub fn missing_output() -> Self {
        Self {
            raw: Arc::from(""),
            provenance: UnknownProvenance::MissingOutput,
        }
    }

    /// An `Unknown` decoded from the JSON wire format or interned externally
    /// as opaque raw text.
    pub fn wire_opaque(raw: impl Into<Arc<str>>) -> Self {
        Self {
            raw: raw.into(),
            provenance: UnknownProvenance::WireOpaque,
        }
    }

    /// The terminal COMPATIBILITY PROJECTION of a typed resolver degradation.
    /// Crate-private in spirit — the session materialization sidecar is the
    /// sole producer; hidden from the public API so no caller treats it as a
    /// generic raw escape hatch. (`pub` only because the sidecar lives in the
    /// sibling `verter_session` crate.)
    #[doc(hidden)]
    pub fn compatibility_projection(raw: impl Into<Arc<str>>) -> Self {
        Self {
            raw: raw.into(),
            provenance: UnknownProvenance::CompatibilityProjection,
        }
    }

    /// The carried raw source text.
    pub fn raw(&self) -> &str {
        &self.raw
    }

    /// The diagnostic provenance (NEVER identity, NEVER a control
    /// discriminator).
    pub fn provenance(&self) -> UnknownProvenance {
        self.provenance
    }

    /// `true` when the raw text is empty/whitespace — the display layer keeps
    /// its `EmptyUnknownSource` behaviour on this.
    pub fn is_empty(&self) -> bool {
        self.raw.trim().is_empty()
    }

    /// `true` when the raw text is exactly the `const` assertion syntax the
    /// evaluator recognises (`x as const`) — the typed replacement for a
    /// `raw == "const"` raw-string check.
    pub fn is_const_assertion_syntax(&self) -> bool {
        self.raw.as_ref() == "const"
    }

    /// Consume the value and return the raw text allocation.
    pub fn into_raw(self) -> Arc<str> {
        self.raw
    }
}

/// Equality is RAW-ONLY: provenance is diagnostic, never identity.
impl PartialEq for UnknownValue {
    fn eq(&self, other: &Self) -> bool {
        self.raw == other.raw
    }
}
impl Eq for UnknownValue {}

/// Hashing is RAW-ONLY (byte-identical to the legacy `String` field hash:
/// `str`'s `Hash` stream), preserving every cache key that embedded an
/// `Unknown { raw }`.
impl Hash for UnknownValue {
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.raw.hash(state);
    }
}
