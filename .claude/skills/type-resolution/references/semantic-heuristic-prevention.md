## Semantic Heuristic Prevention (CRITICAL)

Semantic behavior must be driven by typed facts, explicit projection policy, and complete semantic query identity. Do not encode type meaning, resolver routing, cache validity, or published component-meta shape in local heuristics.

Forbidden patterns:

- Inferring semantic meaning from rendered type strings, raw source slices, identifier suffixes/prefixes, path substrings, display text, or compatibility formatting.
- Choosing between candidate type results with subjective "better shape" scoring when the candidates do not carry exactness/provenance proving which one is authoritative.
- Letting projection mode, substitution environment, conditional context, scope version, workspace/source generation, package-boundary policy, or solver options affect a result without being represented in semantic identity, cached-value validation/read-set metadata, or the projection plan.
- Treating numeric caps, recursion limits, or fanout fuses as normal semantic answers. A fuse may stop work, but must return a structured degraded result such as `BudgetExceeded`, `Unsupported`, or an explicit recursion sentinel, and that result must not be promoted into a warm shared cache entry as if complete.
- Adding compatibility or transition paths that preserve old heuristic behavior. Replace the old path with a typed producer fact, a richer IR variant, an explicit policy enum, or a structured unsupported result.

Allowed optimization heuristics are limited to performance triage that cannot change observable meaning: skipping a cache lookup, choosing an equivalent fast path after tests prove equivalence, or deciding a result is not safe to cache. If an optimization can change which type is returned, which members are visible, how aliases are preserved, or whether a result is considered complete, it is not an optimization heuristic; it is semantic policy and must be modeled explicitly.

Best-architecture target:

- Every semantic query has typed identity plus validation metadata covering all meaning-affecting dimensions: declaration identity, scope/version root, projection mode, type arguments/substitution environment, conditional context, package/workspace policy, solver options. The cache may split these between slot key, per-mode entry, and cached-value fact signature, but no dimension may be implicit.
- Every semantic result carries enough exactness/provenance to distinguish `Exact`, `SurfaceOnly`, `Unsupported`, `BudgetExceeded`, and recursion-sentinel outcomes without re-inspecting text or shape.
- Projection planning is explicit. Callers request identity/navigation/shallow/expanded behavior and package-boundary/depth policy up front; resolvers do not infer these from ad hoc shape checks.
- If the current IR cannot represent a TypeScript construct, extend the IR/schema or return a structured unsupported result with diagnostics. Do not recover meaning by reparsing display text.

Guards: `no_macro_string_heuristics_in_resolver_core`, `no_format_then_reparse`, `no_node_modules_substring_outside_workspace_api` (`crates/verter_session/tests/cases/architecture_guards.rs`) mechanically forbid the text-derived-meaning and path-substring patterns the rule text names. The identity/validation-completeness half of the rule is held by the cache-architecture guards (`/type-cache-architecture`), not by these three.

