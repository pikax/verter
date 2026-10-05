//! The sentinel a resolved-import fact records for an unresolved specifier.

/// Sentinel canonical placed on a negative
/// [`FactKey::ResolvedImportClause`] entry so the fact key stays
/// non-`Option` while remaining distinguishable from any real
/// canonical path. The NUL bytes prevent collision with any
/// filesystem path on every platform Verter supports.
pub const UNRESOLVED_SENTINEL: &str = "\0unresolved\0";
