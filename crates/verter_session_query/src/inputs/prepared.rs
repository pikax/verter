//! Owned prepared-declaration inputs and preparation outcomes.
//!
//! A host prepares a file's declarations once and hands the engine these
//! owned records: the declaration scope of each lexical owner and the typed
//! outcome of one demanded preparation. The preparation caches and the
//! preparation itself stay with the host.

use std::sync::Arc;

use rustc_hash::{FxHashMap, FxHashSet};
use verter_type_expr::TopLevelOwnerId;

use crate::analysis::types::Hash16;
use crate::type_solver::{PreparedTypeDecl, ResolvedRootIdentity};

/// A typed reason a declaration could not be prepared.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PreparationFailure {
    MissingExternalOwner { local_name: String },
    AuthoredOrdinalOverflow { count: usize },
}

/// Projection-facing prepared-declaration lookup.
///
/// Strict prepared declarations are the only values admitted to the shared
/// cache. When strict preparation fails solely because an exact authored
/// declaration references an unresolved import, projection may consume an
/// ephemeral declaration that retains the known authored shape while carrying
/// typed unresolved-owner debt. The declaration is never written into a slot;
/// the projection must run the normal resolver and classify the result partial
/// only if that exact debt remains unresolved at query exit.
#[derive(Debug, Clone)]
pub enum PreparedTypeDeclResolution {
    Complete(verter_retention::heap_size::OwnedValue<PreparedTypeDecl>),
    AuthoredPartial {
        root_identity: ResolvedRootIdentity,
        declaration: verter_retention::heap_size::OwnedValue<PreparedTypeDecl>,
        failure: PreparationFailure,
    },
    Missing,
    Failed {
        root_identity: ResolvedRootIdentity,
        failure: PreparationFailure,
    },
}

impl PreparedTypeDeclResolution {
    #[must_use]
    pub fn is_missing(&self) -> bool {
        matches!(self, Self::Missing)
    }
}

/// Owned outcome of a source declaration demand.
///
/// A genuine `Ready(None)` (the symbol is not inventoried, is an
/// import-local, or lowered to no decl) is a cacheable absence; a `LeaseMiss`
/// (a broken decl-body lease pin — the demanded body lowering lowered
/// NOTHING) is a TRANSIENT no-warm signal a cache-admitting consumer must NOT
/// persist as absence, so a later demand under a live lease recovers. Never
/// collapse the two at a warm-admission boundary.
pub enum PreparedDeclOutcome<T> {
    Ready(Option<T>),
    LeaseMiss,
    Failed(PreparationFailure),
}

/// Import binding: maps a local import name to its resolved target.
/// Used by the declaration-scope solver host to resolve cross-file references.
#[derive(Debug, Clone, verter_retention::heap_size::HeapSize)]
pub struct ImportBinding {
    pub canonical_id: String,
    pub exported_name: String,
}

/// Script-setup generic type-parameter binding for
/// `<script setup lang="ts" generic="T extends Item = Item">`
/// parameters.
///
/// The binding carries ONLY the parameter name plus its 0-based clause
/// ordinal — the content-free FACT pair. The declaration-site `extends`
/// constraint and `=` default are NEVER stored: at query time the ONE
/// dispatch lowering helper re-borrows the full parameter clause lease-only
/// from the pinned indexed artifact and selects + validates the transient
/// parameter by `(ordinal, name)`. A prepared declaration would be the wrong
/// category for this data — type parameters do not have alias bodies,
/// scope-local name resolution, or the rest of the prepared-decl surface.
///
/// `ordinal` carries the 0-based clause position into the lowered type
/// parameter's index, disambiguating same-name parameters across multiple
/// script-setup declarations within one file.
#[derive(Debug, Clone, verter_no_typeexpr::NoTypeExpr, verter_retention::heap_size::HeapSize)]
pub struct TypeParamBinding {
    pub name: Arc<str>,
    /// 0-based position in the `<script setup generic="T, U, V">` clause,
    /// used as the lowered type parameter's index so multiple script-setup
    /// parameters in one file get distinct identity tuples.
    pub ordinal: u16,
}

/// Owner-exact declaration-scope surfaces of one prepared file.
///
/// The lexical owner is the key of the owner-scope map, so every map here
/// can stay string-keyed without aliasing a same-name binding from another
/// script region.
#[derive(Clone, Default, verter_retention::heap_size::HeapSize)]
pub struct PreparedOwnerScope {
    /// Resolved imports visible in this lexical owner.
    pub import_bindings: FxHashMap<String, ImportBinding>,
    /// Same-file type names visible in this lexical owner.
    pub scope_type_names: FxHashSet<String>,
    /// Same-file value names visible in this lexical owner.
    pub scope_value_names: FxHashSet<String>,
    /// Script-setup generic parameters visible in this lexical owner.
    pub script_setup_type_bindings: FxHashMap<String, TypeParamBinding>,
}

/// Owned declaration-scope record of one prepared file, as the engine reads
/// it: an observation identity, the owning file's content version, and the
/// exact scope of every lexical owner.
#[derive(Clone, verter_retention::heap_size::HeapSize)]
pub struct PreparedInputRecord {
    pub observation_id: u64,
    pub owner_whole_hash: Hash16,
    pub owner_scopes: OwnerScopes,
}

/// The exact declaration scope of every lexical owner of one prepared file:
/// one self-charged cell, shared by the file's bundle and every input record
/// and reader viewing it.
pub type OwnerScopes =
    verter_retention::heap_size::OwnedValue<FxHashMap<TopLevelOwnerId, PreparedOwnerScope>>;

/// A prepared file's input record as the engine holds it: one self-charged
/// cell, so a record a request keeps past its bundle stays charged.
pub type PreparedInputRef = verter_retention::heap_size::OwnedValue<PreparedInputRecord>;

impl PreparedInputRecord {
    /// Exact declaration scope for `owner`. An absent owner has no scope; it
    /// never inherits another owner's.
    pub fn owner_scope(&self, owner: TopLevelOwnerId) -> Option<&PreparedOwnerScope> {
        self.owner_scopes.get(&owner)
    }
}
