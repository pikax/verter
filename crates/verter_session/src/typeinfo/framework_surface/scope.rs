#![deny(missing_docs)]
//! Shared member-value scope derivation for framework-surface producers.
//!
//! The SINGLE owner of the member-value-scope rule consumed by every adapter's
//! surface producer (Vue + Svelte). Housed here — not inside one adapter's
//! resolution leg — so the shared rule has one home and neither `vue_exec` nor
//! `svelte_exec` owns it privately.

use verter_type_expr::TypeExprScope;

use crate::typeinfo::surface::TypeInfoSurfaceMember;
use crate::VerterHost;

/// The scope a surface member's raised `*_expr` should bind to — the member's
/// VALUE-NODE scope (`node_scope(member.value)` → file), falling back to the
/// member's declaration_origin, then `owner_fallback`.
///
/// This is the SINGLE owner of the member-value-scope rule. Every producer that
/// pairs a member-derived `*_expr` with a `*_expr_scope` (the Vue
/// `emits_from_typeinfo_surface` / `props_from_typeinfo_surface` /
/// `slots_from_typeinfo_surface` property-style paths AND the Svelte
/// callback-prop event path) derives the scope HERE so a payload `Ref` (e.g. a
/// callback parameter typed against a same-module `interface Row`) resolves in
/// the file whose OXC parse produced the value expression. A `None` scope paired
/// with a `Some` `*_expr` is a pairing-invariant violation (the consumer cannot
/// resolve the payload's named refs) — see the `surface_member_to_expanded_field`
/// `debug_assert_eq!` pairing guard.
///
/// The value-node scope (NOT the member's declaration_origin) is the file whose
/// OXC parse produced the typed value expression, which is where its nested
/// `Ref`s must resolve. The two files DIVERGE for a generic inherited member;
/// JSDoc deliberately uses the declaration_origin instead (the two axes
/// intentionally use different files).
#[must_use]
pub(crate) fn member_value_expr_scope(
    host: &VerterHost,
    member: &TypeInfoSurfaceMember,
    owner_fallback: &str,
) -> TypeExprScope {
    host.project_type_store()
        .semantic_graph()
        .node_scope(member.value)
        .and_then(|scope| scope.canonical_file())
        .map(|canonical| TypeExprScope::new(canonical.as_ref()))
        .or_else(|| {
            member
                .origin
                .canonical_file
                .as_ref()
                .map(|canonical| TypeExprScope::new(canonical.as_ref()))
        })
        .unwrap_or_else(|| TypeExprScope::new(owner_fallback))
}
