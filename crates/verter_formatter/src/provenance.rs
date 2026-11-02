//! The formatter's provenance identity and carrier contract.
//!
//! Every rendered segment carries an [`AuthoredProvenance`] binding a
//! [`FormatProvenanceId`] to exactly one authored byte range
//! ([`Provenanced<T>`]); a [`FormatProvenanceTable`] owns the identities minted
//! for one source revision and is the only place an identity becomes valid. A
//! producer (authored view, renderer) and a consumer (edit derivation, map
//! join) share exactly these definitions — no bare integer and no separately
//! declared token can stand in for the identity. Those negative contracts run
//! with pinned diagnostics in `tests/compile-fail/` through the workspace
//! compile-contract lane, which owns them:
//!
//! ```compile_fail,E0308
//! use verter_formatter::provenance::FormatProvenanceId;
//! fn consumer(_id: FormatProvenanceId) {}
//! // A bare integer cannot stand in for a provenance identity.
//! consumer(0);
//! ```
//!
//! ```compile_fail,E0308
//! use verter_formatter::provenance::FormatProvenanceId;
//! struct OtherToken(u32);
//! fn consumer(_id: FormatProvenanceId) {}
//! // A separately declared token type cannot stand in either.
//! consumer(OtherToken(0));
//! ```
//!
//! None of these types are wire identities: they never serialize, and an
//! identity is meaningless outside the table (and source revision) that minted
//! it — the identity itself carries its minting revision's scope, so stale
//! provenance cannot join a newer revision even when every ordinal and range
//! coincides and the caller presents the destination's own revision.

use std::any::Any;
use std::fmt;
use std::hash::{Hash, Hasher};
use std::rc::Rc;

use crate::coordinates::AuthoredFormatByteRange;

/// The minting-revision scope, compared exactly: the revision value itself,
/// type-erased so [`FormatProvenanceId`] stays non-generic.
///
/// Equality is the revision's own `Eq` over its own concrete type — never a
/// `Hash` encoding, which lawful implementations may share between unequal
/// revisions — so two scopes are equal exactly when the same revision type
/// holds two equal revision values.
trait ErasedRevision {
    fn as_any(&self) -> &dyn Any;
    fn eq_erased(&self, other: &dyn ErasedRevision) -> bool;
}

/// The one [`ErasedRevision`] storage: the caller's revision value, shared by
/// the minting table and every identity it mints.
#[derive(Clone)]
struct RevisionScope<R> {
    revision: R,
}

impl<R: Eq + 'static> ErasedRevision for RevisionScope<R> {
    fn as_any(&self) -> &dyn Any {
        self
    }

    fn eq_erased(&self, other: &dyn ErasedRevision) -> bool {
        other
            .as_any()
            .downcast_ref::<RevisionScope<R>>()
            .is_some_and(|other| self.revision == other.revision)
    }
}

/// Opaque identity of one source-backed authored unit, valid only inside the
/// [`FormatProvenanceTable`] — one source revision — that minted it.
///
/// An identity is its minting revision's scope plus one table-local ordinal:
/// two tables for the same source revision minting the same view order
/// produce equal identities, while identities minted under different
/// revisions never compare equal, resolve, or validate against each other,
/// however their ordinals and ranges coincide — and however their revisions'
/// `Hash` encodings coincide, because the scope is the revision value
/// compared by `Eq`, not a hash of it.
///
/// There is no public constructor: identities enter circulation exclusively
/// through [`FormatProvenanceTable::bind_authored`], so their assignment is
/// deterministic for a given authored view order and reconstruction assigns
/// the same identities.
#[derive(Clone)]
pub struct FormatProvenanceId {
    scope: Rc<dyn ErasedRevision>,
    ordinal: u32,
}

impl PartialEq for FormatProvenanceId {
    fn eq(&self, other: &Self) -> bool {
        self.ordinal == other.ordinal && self.scope.eq_erased(&*other.scope)
    }
}

impl Eq for FormatProvenanceId {}

/// Equal identities always share an ordinal; the minting scope may differ,
/// which a hash is allowed to collapse. The ordinal alone is therefore a
/// lawful — and the only observable — hash input.
impl Hash for FormatProvenanceId {
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.ordinal.hash(state);
    }
}

/// Prints the ordinal only: the minting scope is the caller's revision value,
/// held for comparison and never serialized, printed or otherwise exposed.
impl fmt::Debug for FormatProvenanceId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("FormatProvenanceId")
            .field("ordinal", &self.ordinal)
            .finish()
    }
}

/// The binding of one provenance identity to exactly one authored byte range.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct AuthoredProvenance {
    id: FormatProvenanceId,
    range: AuthoredFormatByteRange,
}

impl AuthoredProvenance {
    /// Binds `id` to `range`. The binding is only valid where `id` resolves
    /// to `range` in the minting table; see
    /// [`FormatProvenanceTable::validate`].
    #[inline]
    pub const fn new(id: FormatProvenanceId, range: AuthoredFormatByteRange) -> Self {
        Self { id, range }
    }

    /// The bound identity.
    #[inline]
    pub fn id(&self) -> FormatProvenanceId {
        self.id.clone()
    }

    /// The bound authored byte range.
    #[inline]
    pub const fn range(&self) -> AuthoredFormatByteRange {
        self.range
    }
}

/// A value carried together with its authored provenance — the sole carrier
/// shape for authored-view nodes and rendered segments.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct Provenanced<T> {
    value: T,
    provenance: AuthoredProvenance,
}

impl<T> Provenanced<T> {
    /// Pairs `value` with its authored provenance.
    #[inline]
    pub const fn new(value: T, provenance: AuthoredProvenance) -> Self {
        Self { value, provenance }
    }

    /// The carried value.
    #[inline]
    pub const fn value(&self) -> &T {
        &self.value
    }

    /// The carried provenance binding.
    #[inline]
    pub const fn provenance(&self) -> &AuthoredProvenance {
        &self.provenance
    }

    /// Splits the carrier into its value and provenance binding.
    #[inline]
    pub fn into_parts(self) -> (T, AuthoredProvenance) {
        (self.value, self.provenance)
    }

    /// Transforms the carried value, preserving its provenance binding
    /// unchanged.
    #[inline]
    pub fn map<U>(self, f: impl FnOnce(T) -> U) -> Provenanced<U> {
        Provenanced {
            value: f(self.value),
            provenance: self.provenance,
        }
    }
}

/// The provenance identity table for one source revision: the sole minter and
/// validator of [`FormatProvenanceId`]s for that revision.
///
/// `R` is the caller's stable source-revision identity for the authored text
/// (a content hash, version counter or equivalent), not a formatter-global
/// identifier. It carries its own exact equality (`Eq`) and a `'static`
/// concrete type, because the minting scope is the revision value itself —
/// never a `Hash` encoding of it, which lawful revisions may share. The
/// `'static` and `Eq` bounds are what make an identity space exactly
/// revision-scoped: a table for another revision — or another revision type
/// — never accepts this table's identities, however ordinals, ranges or hash
/// encodings coincide. Reconstructing the same authored view revision —
/// binding the same ranges in the same order — assigns the same identities
/// deterministically.
#[derive(Clone)]
pub struct FormatProvenanceTable<R> {
    /// The revision every identity minted here carries, shared with each
    /// minted identity so a foreign identity can never be interpreted in this
    /// table's numbering space — compared by the revision's own `Eq` over its
    /// own concrete type, never by a `Hash` encoding.
    revision_scope: Rc<RevisionScope<R>>,
    entries: Vec<AuthoredFormatByteRange>,
}

impl<R: Eq + 'static> FormatProvenanceTable<R> {
    /// An empty table for exactly `source_revision`.
    #[inline]
    pub fn new(source_revision: R) -> Self {
        Self {
            revision_scope: Rc::new(RevisionScope {
                revision: source_revision,
            }),
            entries: Vec::new(),
        }
    }

    /// The source revision this table mints and validates identities for.
    #[inline]
    pub fn source_revision(&self) -> &R {
        &self.revision_scope.revision
    }

    /// The scope this table mints under, as the erased form identities
    /// carry it.
    fn scope(&self) -> &dyn ErasedRevision {
        &*self.revision_scope
    }

    /// The binding `id` addresses in this table, or `None` when the identity
    /// was minted under another revision's scope or past this table's end:
    /// either way this table never minted it.
    fn entry_for(&self, id: &FormatProvenanceId) -> Option<&AuthoredFormatByteRange> {
        if !id.scope.eq_erased(self.scope()) {
            return None;
        }
        self.entries.get(id.ordinal as usize)
    }

    /// Mints the next identity for `range` in authored view order and records
    /// the binding. Deterministic: replaying the same view order into a fresh
    /// table for the same revision assigns the same identities.
    pub fn bind_authored(&mut self, range: AuthoredFormatByteRange) -> FormatProvenanceId {
        let ordinal = u32::try_from(self.entries.len())
            .expect("provenance table exceeds the u32 identity space");
        self.entries.push(range);
        FormatProvenanceId {
            scope: self.revision_scope.clone(),
            ordinal,
        }
    }

    /// Re-declares a known binding, as a same-revision reconstruction does.
    ///
    /// Re-declaring the same identity with the same range is idempotent; a
    /// duplicate identity bound to a different range is rejected, as is an
    /// identity this table never minted — including one minted under another
    /// revision.
    pub fn declare(
        &mut self,
        id: FormatProvenanceId,
        range: AuthoredFormatByteRange,
    ) -> Result<(), ProvenanceError> {
        match self.entry_for(&id) {
            Some(existing) if *existing == range => Ok(()),
            Some(existing) => Err(ProvenanceError::DuplicateConflictingBinding {
                id,
                existing: *existing,
                requested: range,
            }),
            None => Err(ProvenanceError::UnknownId { id }),
        }
    }

    /// The authored byte range bound to `id` in this table. An identity
    /// minted under another revision is unknown here, whatever its ordinal.
    pub fn resolve(
        &self,
        id: FormatProvenanceId,
    ) -> Result<AuthoredFormatByteRange, ProvenanceError> {
        self.entry_for(&id)
            .copied()
            .ok_or(ProvenanceError::UnknownId { id })
    }

    /// All bindings, in identity (authored view) order.
    pub fn iter(&self) -> impl Iterator<Item = AuthoredProvenance> + '_ {
        self.entries.iter().enumerate().map(|(ordinal, range)| {
            AuthoredProvenance::new(
                FormatProvenanceId {
                    scope: self.revision_scope.clone(),
                    ordinal: ordinal as u32,
                },
                *range,
            )
        })
    }

    /// The number of bound identities.
    #[inline]
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// Whether no identity is bound yet.
    #[inline]
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// Validates a provenance binding for use with this table: the caller's
    /// source revision must equal this table's revision, the identity must
    /// have been minted here — under this table's own revision scope, not
    /// merely under a coinciding ordinal — and the bound range must match
    /// this table's binding exactly. Mixed revisions, stale identities from
    /// other revisions, unknown identities and mismatched bindings are all
    /// rejected — nothing is re-bound approximately.
    pub fn validate(
        &self,
        source_revision: &R,
        provenance: &AuthoredProvenance,
    ) -> Result<(), ProvenanceError> {
        if self.source_revision() != source_revision {
            return Err(ProvenanceError::RevisionMismatch);
        }
        if !provenance.id.scope.eq_erased(self.scope()) {
            // The claimed basis is this table's revision, but the identity
            // was minted under another: a stale binding never joins the
            // current revision, however its ordinal and range align.
            return Err(ProvenanceError::RevisionMismatch);
        }
        match self.entry_for(&provenance.id) {
            Some(existing) if *existing == provenance.range => Ok(()),
            Some(existing) => Err(ProvenanceError::MismatchedBinding {
                id: provenance.id.clone(),
                table_binding: *existing,
                provenance_range: provenance.range,
            }),
            None => Err(ProvenanceError::UnknownId {
                id: provenance.id.clone(),
            }),
        }
    }
}

/// The scope is the revision itself, so comparing revision and entries
/// compares the scopes too.
impl<R: PartialEq> PartialEq for FormatProvenanceTable<R> {
    fn eq(&self, other: &Self) -> bool {
        self.revision_scope.revision == other.revision_scope.revision
            && self.entries == other.entries
    }
}

impl<R: Eq> Eq for FormatProvenanceTable<R> {}

/// The shared scope is the same value as `source_revision` and never printed
/// per identity; the identity's own [`fmt::Debug for FormatProvenanceId`]
/// keeps the ordinal as everything a user-facing message needs.
impl<R: fmt::Debug> fmt::Debug for FormatProvenanceTable<R> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("FormatProvenanceTable")
            .field("source_revision", &self.revision_scope.revision)
            .field("entries", &self.entries)
            .finish()
    }
}

/// A provenance identity failure: duplicate or mismatched bindings, unknown
/// identities, or a join across source revisions.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ProvenanceError {
    /// A known identity was re-declared or presented with a different range
    /// than the one it is bound to.
    DuplicateConflictingBinding {
        /// The contested identity.
        id: FormatProvenanceId,
        /// The range this table already binds the identity to.
        existing: AuthoredFormatByteRange,
        /// The conflicting range.
        requested: AuthoredFormatByteRange,
    },
    /// The identity was never minted by this table.
    UnknownId {
        /// The unrecognized identity.
        id: FormatProvenanceId,
    },
    /// A presented binding does not match the table's binding for that
    /// identity.
    MismatchedBinding {
        /// The contested identity.
        id: FormatProvenanceId,
        /// The range this table binds the identity to.
        table_binding: AuthoredFormatByteRange,
        /// The range the presented provenance carries.
        provenance_range: AuthoredFormatByteRange,
    },
    /// The provenance was minted for a different source revision than the one
    /// it is being validated against.
    RevisionMismatch,
}

impl fmt::Display for ProvenanceError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ProvenanceError::DuplicateConflictingBinding { id, .. } => write!(
                f,
                "provenance identity {id:?} is already bound to a different authored range"
            ),
            ProvenanceError::UnknownId { id } => {
                write!(
                    f,
                    "provenance identity {id:?} was never minted by this table"
                )
            }
            ProvenanceError::MismatchedBinding { id, .. } => write!(
                f,
                "provenance identity {id:?} does not match this table's binding for it"
            ),
            ProvenanceError::RevisionMismatch => {
                write!(
                    f,
                    "provenance identity belongs to a different source revision"
                )
            }
        }
    }
}

impl std::error::Error for ProvenanceError {}
