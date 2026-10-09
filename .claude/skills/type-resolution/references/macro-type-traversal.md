## Macro Type Traversal Rule

When resolving cross-file macro types (`defineProps<T>()`, `defineEmits<T>()`, and other shared host-backed queries), only follow the import graph reachable from the requested type's declaration graph.

**Macro resolution is one shared path — `shared_resolve(type) + normalise`.** Every macro (`defineProps` / `defineEmits` / `defineOptions` / `defineSlots` / `withDefaults`) and every imported `.vue` component surface resolves through exactly TWO steps:

1. **Resolve ONE type via the shared resolver** — the generic-parameter type (`define*<T>()`) OR the object-argument type (`define*({ ... })`). `withDefaults` resolves the props payload type plus the defaults-object type and merges. `.vue`-component imports resolve the imported component's synthesized `$props` / `$emit` / `$slots` / expose surface recursively through the same dispatch (the hardest case — apply EXTRA caution: it is exactly where rule violations cause the worst hangs). Resolution is ALWAYS the shared typed-IR five-mode dispatch — no macro-specific engine, no per-surface walker, no eager element resolver.
2. **Normalize from the stored stream (a thin transform, NOT a resolver)** — props: defaults / optionality / readonly / declaration provenance / `declared_in_macro_type_arg`; emits: walk the canonical `SurfaceEntry` stream once and expand each call-signature or property producer directly into a complete occurrence without a kind-array merge, name join, ordinal join, or post-hoc reorder; a resolved concrete property payload must be tuple/function-shaped, while an incompatible concrete payload closes the macro as `MacroInvalidReason::InvalidEmitsShape` on both Runtime and TSC demands (`{}` remains a valid empty emits surface, and open/opaque payloads retain conservative projection); the call-signature payload strips the leading event-name parameter; slots: function-like members only, first-parameter object becomes bindings, return type preserved; options/expose: pass-through object surface.

A macro/import that resolves its surface through anything other than the shared resolver, or flattens a full surface eagerly before the consumer demands it, is a rule violation — collapse it into `shared_resolve(type) + normalise`.

- There is one shared cross-file type resolver. Consumer-specific ownership rules live in `/component-meta`.
- The resolver has exactly five query modes (see "Query Mode Contract" above):
  - `Identity`: declaration identity and canonical source location only. No body read, no shape materialization.
  - `Navigate`: minimum semantic work needed to continue a requested path. Intermediate hops run in this mode.
  - `Shallow`: one surface level of the requested node without recursive expansion.
  - `Expanded`: recursive materialization of the requested result.
  - `Skeleton`: specialised generic-helper/cycle traversal that keeps unbound type parameters as shells.
- `Skeleton` is a distinct mode, not a synonym for `Navigate`. It is currently scoped to cycle/generic-helper traversal — the materialization cycle gate's per-hop `Instantiate` (`ClassifyMaterializationCycleGate`, see `project_semantic_dispatch::cycle_gate`). New call sites must justify why they need Skeleton semantics instead of `Navigate` / `Shallow`.
- Do not introduce ad hoc navigate/shallow flags; use the canonical modes and the path-precise projection surface.
- Do not walk unrelated imports from the same file.
- Do not treat plain imports as implicit exports.
- Keep direct re-exports (`export { X } from`, `export * from`) as an explicit separate path.
- Parsing a `.ts`/`.js`/declaration file for type resolution must cache discovered symbol name -> canonical location mappings.
- Re-exported names and barrel hops must also be cached once discovered. If traversal follows `export * from './foo'`, cache that result so later lookups do not rescan the same barrel chain.

If a file imports 20 modules but the requested macro type only references `AvatarProps` and `IconProps`, external resolution must only traverse those reachable dependencies.

**TS-first resolution priority:** TypeScript types always take priority over JavaScript files when resolving ambiguous dependency candidates. Verter is a type-strict compiler that relies on TS typing for correctness. JS files should only be used as a last resort when no TS type definition is available. When `DependencyResolution.possible_canonical_ids` contains multiple candidates, use `effective_target()` which selects the single highest-priority candidate: `.d.ts` > `.d.cts` > `.d.mts` > `.ts` > `.tsx` > `.js` > `.jsx` > `.cjs` > `.mjs`. Do not try remaining candidates if the selected one lacks the needed type -- treat as not found.

**Owned resolution is bounded by `workspace_root`:** For owned and project-scoped resolution, `node_modules` and package `#imports` ancestor walks stop at `IdeProjectConfig.workspace_root`. In monorepos, `workspace_root` may be above `project_root` to reach hoisted `node_modules`. In compat `createCheckerByJson()`, `workspace_root == project_root`. Unowned resolution (no owning project) remains unbounded. The boundary is passed via `ancestor_dirs(path, Some(&workspace_root))` and `ancestor_dirs_from_dir(start_dir, Some(&workspace_root))` in `verter_workspace::resolver`.

