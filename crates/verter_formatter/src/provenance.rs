//! The formatter's provenance identity and carrier contract.
//!
//! Every rendered segment carries an [`AuthoredProvenance`] binding a
//! [`FormatProvenanceId`] to exactly one authored byte range
//! ([`Provenanced<T>`]); a [`FormatProvenanceTable`] owns the identities minted
//! for one source revision and is the only place an identity becomes valid. A
//! producer (authored view, renderer) and a consumer (edit derivation, map
//! join) share exactly these definitions — no bare integer and no separately
//! declared token can stand in for the identity:
//!
//! ```compile_fail
//! use verter_formatter::provenance::FormatProvenanceId;
//! fn consumer(_id: FormatProvenanceId) {}
//! // A bare integer cannot stand in for a provenance identity.
//! consumer(0);
//! ```
//!
//! ```compile_fail
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

use std::collections::hash_map::DefaultHasher;
use std::fmt;
use std::hash::{Hash, Hasher};

use crate::coordinates::AuthoredFormatByteRange;

/// Opaque identity of one source-backed authored unit, valid only inside the
/// [`FormatProvenanceTable`] — one source revision — that minted it.
///
/// An identity is its minting revision's scope plus one table-local ordinal:
/// two tables for the same source revision minting the same view order
/// produce equal identities, while identities minted under different
/// revisions never compare equal, resolve, or validate against each other,
/// however their ordinals and ranges coincide.
///
/// There is no public constructor: identities enter circulation exclusively
/// through [`FormatProvenanceTable::bind_authored`], so their assignment is
/// deterministic for a given authored view order and reconstruction assigns
/// the same identities.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct FormatProvenanceId {
    revision_scope: u128,
    ordinal: u32,
}

/// The binding of one provenance identity to exactly one authored byte range.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
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
    pub const fn id(&self) -> FormatProvenanceId {
        self.id
    }

    /// The bound authored byte range.
    #[inline]
    pub const fn range(&self) -> AuthoredFormatByteRange {
        self.range
    }
}

/// A value carried together with its authored provenance — the sole carrier
/// shape for authored-view nodes and rendered segments.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
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
/// identifier. Reconstructing the same authored view revision — binding the
/// same ranges in the same order — assigns the same identities
/// deterministically; a table for a different revision is a different identity
/// space, and validating against a mismatched revision is rejected.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FormatProvenanceTable<R> {
    source_revision: R,
    /// Scope every identity minted here carries, so a foreign identity can
    /// never be interpreted in this table's numbering space. Derived from
    /// `source_revision` once; equal revisions derive equal scopes.
    revision_scope: u128,
    entries: Vec<AuthoredFormatByteRange>,
}

/// The revision scope one source revision mints all its identities under:
/// two domain-separated SipHash passes over the revision's [`Hash`]
/// encoding, widened to 128 bits.
///
/// Equal revisions always derive the equal scopes that deterministic
/// same-revision reconstruction relies on; distinct revisions would need a
/// 128-bit hash collision to share one. This is a process-local
/// discriminator between identity spaces — never a canonical form, never
/// serialized, never exposed.
fn revision_scope_of<R: Hash>(revision: &R) -> u128 {
    let mut low = DefaultHasher::new();
    let mut high = DefaultHasher::new();
    0u8.hash(&mut low);
    1u8.hash(&mut high);
    revision.hash(&mut low);
    revision.hash(&mut high);
    ((high.finish() as u128) << 64) | (low.finish() as u128)
}

impl<R: Hash> FormatProvenanceTable<R> {
    /// An empty table for exactly `source_revision`.
    #[inline]
    pub fn new(source_revision: R) -> Self {
        Self {
            revision_scope: revision_scope_of(&source_revision),
            source_revision,
            entries: Vec::new(),
        }
    }
}

impl<R> FormatProvenanceTable<R> {
    /// The source revision this table mints and validates identities for.
    #[inline]
    pub fn source_revision(&self) -> &R {
        &self.source_revision
    }

    /// The binding `id` addresses in this table, or `None` when the identity
    /// was minted under another revision's scope or past this table's end:
    /// either way this table never minted it.
    fn entry_for(&self, id: FormatProvenanceId) -> Option<&AuthoredFormatByteRange> {
        if id.revision_scope != self.revision_scope {
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
            revision_scope: self.revision_scope,
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
        match self.entry_for(id) {
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
        self.entry_for(id)
            .copied()
            .ok_or(ProvenanceError::UnknownId { id })
    }

    /// All bindings, in identity (authored view) order.
    pub fn iter(&self) -> impl Iterator<Item = AuthoredProvenance> + '_ {
        self.entries.iter().enumerate().map(|(ordinal, range)| {
            AuthoredProvenance::new(
                FormatProvenanceId {
                    revision_scope: self.revision_scope,
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
}

impl<R: PartialEq> FormatProvenanceTable<R> {
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
        if &self.source_revision != source_revision {
            return Err(ProvenanceError::RevisionMismatch);
        }
        if provenance.id.revision_scope != self.revision_scope {
            // The claimed basis is this table's revision, but the identity
            // was minted under another: a stale binding never joins the
            // current revision, however its ordinal and range align.
            return Err(ProvenanceError::RevisionMismatch);
        }
        match self.entry_for(provenance.id) {
            Some(existing) if *existing == provenance.range => Ok(()),
            Some(existing) => Err(ProvenanceError::MismatchedBinding {
                id: provenance.id,
                table_binding: *existing,
                provenance_range: provenance.range,
            }),
            None => Err(ProvenanceError::UnknownId { id: provenance.id }),
        }
    }
}

/// A provenance identity failure: duplicate or mismatched bindings, unknown
/// identities, or a join across source revisions.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
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
        match *self {
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
