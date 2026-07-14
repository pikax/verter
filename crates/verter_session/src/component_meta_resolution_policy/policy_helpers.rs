//! Shared symbolic-vs-canonical materialization predicate for the
//! component-meta materialize pipeline's bare-ref decision sites: given a
//! target declaration and a `PolicyContext`, it decides whether an imported
//! bare reference MUST materialize canonically or MAY stay symbolic.
//! Centralising the rule keeps every bare-ref decision site on one
//! contract so they cannot drift.
//!
//! # Invariant
//!
//! Workspace-owned direct-member interface/class refs (generic or
//! non-generic) that are not on the recursion/cycle stack and not
//! in a route-preservation context MUST materialize canonically —
//! their cache key `(target_decl_id, normalized_type_args)` is
//! shared across all callers per CLAUDE.md "generic substitutions
//! are part of semantic meaning". The helper centralises the
//! contract so the two callers cannot drift.
//!
//! Symbolic preservation is reserved for:
//!
//! 1. package-backed refs (per `WorkspaceRead::is_package_backed`)
//! 2. explicit shallow-preservation list entries
//! 3. recursion / cycle boundaries (target's
//!    `(DeclId, NormalizedTypeArgs)` already on `active_refs`)
//! 4. lazy-route expression contexts
//! 5. slot-binding indexed-access expressions
//! 6. terminal indexed-access leaves already published
//!
//! Path-substring checks on `node_modules` are BANNED — the
//! helper consumes `WorkspaceRead::is_workspace_owned` /
//! `is_package_backed` exclusively (which route through the
//! resolver's realpath-based classification). The pnpm-symlink
//! and workspace-package-inside-node_modules cases are correctly
//! classified as workspace-owned, NOT package-backed.

use verter_semantic::analysis::type_eval::TypeDeclKind;
use verter_semantic::analysis::type_solver::prepared::PreparedTypeDecl;
use verter_type_expr::facts::PreparedProjectionClassFact;

/// Context bundle for the helper. Exposes the narrow capabilities
/// the predicate needs without leaking `&mut ComponentMetaQueryEngine`
/// or `&dyn ResolverContext` through the public helper signature.
pub(crate) struct PolicyContext<'a> {
    /// Whether `target_canonical_id` is workspace-owned per
    /// `WorkspaceRead::is_workspace_owned` (NOT a path-substring
    /// check on `node_modules`).
    pub is_workspace_owned: &'a dyn Fn(&str) -> bool,
    /// Whether `target_canonical_id` is package-backed per
    /// `WorkspaceRead::is_package_backed` (NOT a path-substring
    /// check on `node_modules`).
    pub is_package_backed: &'a dyn Fn(&str) -> bool,
    /// Caller-provided route-preservation flag. True when the
    /// imported ref is observed inside a lazy-route expression,
    /// a slot-binding indexed-access, or a terminal
    /// indexed-access leaf already published. The caller is
    /// responsible for setting this correctly per their callsite
    /// context.
    pub route_preservation_context: bool,
    /// Caller-provided cycle-active flag. True when the target's
    /// `(DeclId, NormalizedTypeArgs)` identity is already on the
    /// caller's active recursion stack. The caller is responsible
    /// for setting this correctly per their callsite's recursion
    /// guard infrastructure.
    pub cycle_active_for_target: bool,
    /// Caller-provided shallow-preservation list flag. True when
    /// the target's resolved name is on an explicit
    /// shallow-preservation list (e.g., Vue runtime types kept
    /// symbolic by name). The caller is responsible for setting
    /// this correctly per their callsite.
    pub shallow_preserve_list_entry: bool,
}

/// Issue #11 — shared predicate that decides whether
/// an imported bare ref MUST materialize canonically (returns
/// `true`) or MAY stay symbolic (returns `false`).
///
/// - `target_canonical_id`: the target declaration's canonical
///   source id (after import-route resolution).
/// - `prepared_body`: the resolved `PreparedTypeDecl` for the
///   target (or `None` if no prepared decl is available yet —
///   that case treats the body as not direct-member-eligible).
/// - `ctx`: the callsite-provided `PolicyContext` bundle.
///
/// ## Decision flow
///
/// 1. If `ctx.is_package_backed(target_canonical_id)` →
///    return `false` (preserve symbolic, disallowed shape #1).
/// 2. If `ctx.shallow_preserve_list_entry` →
///    return `false` (disallowed shape #2).
/// 3. If `ctx.cycle_active_for_target` →
///    return `false` (disallowed shape #3).
/// 4. If `ctx.route_preservation_context` →
///    return `false` (disallowed shapes #4-#6).
/// 5. If NOT `ctx.is_workspace_owned(target_canonical_id)` →
///    return `false` (the helper only fires for workspace-owned
///    targets; everything else is conservative).
/// 6. If `prepared_body` describes a direct-member interface or
///    class (interface/class kind, OR
///    `PreparedProjectionClass::DirectMembers`, OR a non-empty
///    `member_index`) → return `true` (canonical materialize is
///    required).
/// 7. Otherwise → return `false` (preserve symbolic, conservative
///    default).
///
/// Per §6.5, generic targets are eligible — the cache key
/// `(target_decl_id, normalized_type_args)` is the responsibility
/// of the materialization layer, not this predicate.
#[must_use]
pub(crate) fn imported_ref_must_materialize_canonically(
    target_canonical_id: &str,
    prepared_body: Option<&PreparedTypeDecl>,
    ctx: &PolicyContext<'_>,
) -> bool {
    // 1. Package-backed → preserve symbolic.
    if (ctx.is_package_backed)(target_canonical_id) {
        return false;
    }

    // 2. Explicit shallow-preservation list entry → preserve symbolic.
    if ctx.shallow_preserve_list_entry {
        return false;
    }

    // 3. Recursion/cycle stack → preserve symbolic.
    if ctx.cycle_active_for_target {
        return false;
    }

    // 4-6. Route-preservation context → preserve symbolic.
    if ctx.route_preservation_context {
        return false;
    }

    // 5. Not workspace-owned → preserve symbolic (conservative).
    if !(ctx.is_workspace_owned)(target_canonical_id) {
        return false;
    }

    // 6. Workspace-owned direct-member interface/class →
    //    canonical materialize required.
    let Some(prepared) = prepared_body else {
        return false;
    };

    matches!(prepared.kind, TypeDeclKind::Class)
        || matches!(prepared.kind, TypeDeclKind::Interface)
        || matches!(
            prepared.projection_class,
            PreparedProjectionClassFact::DirectMembers
        )
        || !prepared.member_index.is_empty()
}
