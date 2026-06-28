# ScopeShadowing per-scope memo — structural-seal deferral (durable debt row)

**Status**: DEFERRED — durable. Component-meta hot-path routing of `ScopeShadowing`
construction through the per-scope memo is currently enforced by a SCANNER-SUPPLEMENT
architecture guard. That scanner-supplement is legitimate ONLY while its STRUCTURAL
replacement (below) is tracked; this row is that tracking. The replacement reads as a
post-Stage-9 resolver-architecture follow-up (assessed for final close-vs-defer at
block-land), and is recorded here regardless so the structural end-state is not lost.

## The invariant being held

Production component-meta hot paths in `crate::meta_resolve::**` must obtain the per-scope
`Arc<ScopeShadowing>` from `ComponentMetaQueryEngine::scope_shadowing_for_scope` (which
builds one shadow set per scope and memoizes it), NOT build a fresh `ScopeShadowing`
directly per published field (an O(fields x scope-names) reclone on the publication hot
path).

Currently enforced by the source guard
`component_meta_hot_paths_obtain_scope_shadowing_from_the_per_scope_memo`
(`crates/verter_session/tests/cases/architecture_guards.rs`), a `syn`/path scanner scoped
to `meta_resolve.rs` + `meta_resolve/**` production, allowlisting EXACTLY the one
genuinely engine-less `None`-arm `ScopeShadowing::from_host_scope` fallback in
`project_expr_class_a_via_dispatch_threaded` (arm-precise). Guard-local SC-first record:
`scanner_invariant = component_meta_hot_path_scope_shadowing_memo_routing`;
`mechanism_ruling = SCF-SCOPE-SHADOWING-MEMO-2026-06-28`; `hardening_rounds = 0` (pre-land,
block-branch soundness corrections are adoption shaping and do NOT increment the counter; it
starts at integration / land).

## Why a scanner-supplement, not a compiler seal (the ruling)

A neutral, unprimed codex Structural-Confinement-First ruling
(`SCF-SCOPE-SHADOWING-MEMO-2026-06-28`) determined that a pure compiler/structural seal is
not expressible within the current owner boundaries:

- `ScopeShadowing` + its `pub(crate)` constructors (`resolver_core::scope_shadowing`), the
  per-scope memo (`resolver_core::component_meta_query_engine`), and the hot paths
  (`meta_resolve`) live in three SIBLING top-level modules.
- The genuinely engine-less `project_semantic_dispatch` lowering layer legitimately builds
  `ScopeShadowing` directly at ten sites (once per lowering op, NOT per field) and is a
  visibility-peer of the hot paths. A `pub(in crate::resolver_core)` seal would admit the
  memo but BREAK those ten builds; any `pub(crate)` escape hatch the dispatch peer can call,
  the hot paths can call too.
- The distinction the invariant needs ("use the memo when an engine is present / not
  per-field") is a runtime cadence/engine-availability distinction, not a module boundary,
  so module visibility, sealed tokens, and type-state cannot express it.

Enforced surface: a literal `ScopeShadowing::<ctor>` path-call (bare, module-qualified, or
fully-qualified) AND the UFCS / qself form `<ScopeShadowing>::<ctor>` — both matched
structurally on the `ScopeShadowing` ident plus a sanctioned constructor segment; inline
`#[cfg(test)]` items are out of scope (the guard is production-only).

Disclosed residual (inherent to any name-based syntactic source scanner; NOT enforced). The
80/20 bound leaves TWO residual classes to the structural end-state rather than chasing
further AST shapes:

1. Identity-laundering forms that do not spell `ScopeShadowing` at the call site — a renamed
   `use ...ScopeShadowing as SS; SS::from_host_scope(...)` import, a `type SS = ScopeShadowing;`
   alias, a function-pointer / value capture or other call-form wrapper INCLUDING a
   parenthesized callee (`let f = ScopeShadowing::from_host_scope; f(...)`,
   `(ScopeShadowing::from_host_scope)(...)`), and macro-expanded construction.
2. Runtime / control-flow multiplicity inside the sanctioned `None` arm — the scanner enforces
   ONE syntactic direct call expression in the allowlisted arm, NOT "at most once at runtime";
   a single sanctioned call inside a loop, closure, or guarded sub-arm builds many times at
   runtime while reading as one syntactic call.

Universal confinement of BOTH classes belongs to the structural end-state below (the
shared-`ResolverContext` per-scope memo, after which the constructors seal and a direct
hot-path build will not compile), NOT this scanner. An observed evasion is a laundering escape
that freezes the scanner per Structural-Confinement-First, at which point the structural
replacement below becomes the required fix.

## The structural end-state that retires the scanner

Introduce ONE shared per-scope shadow memo owned at a request-bound layer that BOTH the
component-meta hot paths AND the `project_semantic_dispatch` lowering layer consult, so
every consumer obtains `ScopeShadowing` through the memo:

- Best owner = the request-bound `ResolverContext` wrappers — NOT `ProjectSemanticDispatch`
  (per-query, immutable) and NOT `ProjectTypeStore` (would need persistent
  invalidation/fact semantics for what is a cheap request-local value). The memo is keyed by
  scope, validated against the same request lifecycle the existing
  `ComponentMetaQueryEngine::scope_payloads` / `scope_shadowings` memos use.
- Migrate all ten `project_semantic_dispatch` direct builds (which also rebuild per
  lowering) onto that shared memo.
- THEN seal the `ScopeShadowing` constructors to `pub(in crate::resolver_core)` (or
  module-private to `scope_shadowing`, with the memo as the sole builder). Construction
  becomes compiler-confined by construction; the source-scanner guard is removed (the
  invariant is then self-bounding — a direct hot-path build will not compile).

This is the Structural-Confinement-First structural end-state for this invariant: it both
removes the per-lowering rebuild cost in the dispatch layer (a perf win beyond the
component-meta hot paths) and replaces the scanner-supplement with a compiler seal.

## Ruling source

- `mechanism_ruling: SCF-SCOPE-SHADOWING-MEMO-2026-06-28` — neutral/unprimed codex
  Structural-Confinement-First architecture ruling: the component-meta hot-path routing
  mechanism is SCANNER-SUPPLEMENTED (route the two component-meta hot sites through the memo
  + a discriminating `meta_resolve`-scoped source guard), and the shared-`ResolverContext`-memo
  migration is the separate structural follow-up scoped out of the regression fix.
- CTO layer-2 cleared the guard 2026-06-28 with the binding tracking requirement that this
  structural replacement be recorded as durable tracked debt.
