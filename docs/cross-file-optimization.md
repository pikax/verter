# Cross-File Analysis and Optimization

## Overview

Cross-file prop constness optimization is a whole-program analysis pass that runs after all `.vue` files have been pre-compiled. When **all** parent components pass constant values for a given prop, the child component's compiled output skips dynamic tracking -- no patch flags in VDOM mode, no `renderEffect` wrapping in Vapor mode. This reduces runtime overhead for props that never change.

## How It Works

1. After all files are pre-compiled (`preCompile: true`), `computeCrossFileOptimizations()` builds a **render tree** mapping parent components to their child component usages.
2. For each component used in templates, the optimizer resolves the import to a canonical file ID.
3. Prop constness is aggregated across **all** parent call sites:
   - A prop is `Const` only if **every** parent passes `PropValueConstness::Const` **and** no parent uses `v-bind="spread"`.
   - If **any** parent passes `Dynamic` or `Unknown`, the prop stays `Dynamic`.
   - Root components (no parents in the render tree) conservatively treat all props as `Dynamic`.

## Configuration

Requires both `preCompile` and `crossFileOptimize` to be enabled:

```typescript
import verter from '@verter/unplugin'

export default {
  plugins: [
    verter({
      preCompile: true,           // Required -- all files must be compiled first
      crossFileOptimize: true,    // Enable cross-file optimization
    })
  ]
}
```

## Architecture

```
verter_host::cross_file::CrossFileOptimizer
  render_tree:      HashMap<parent_id, Vec<RenderTreeEdge>>
  prop_constness:   HashMap<child_id, HashMap<prop_name, bool>>
  provide_chains:   HashMap<file_id, HashSet<provide_key>>
```

The optimizer lives in the host crate and operates on the host's compiled file cache. It does not re-parse source files -- it reads prop usage data already extracted during template compilation.

## Import Resolution

When resolving a component import to a canonical file ID, the optimizer tries these strategies in order:

1. **Direct match** against the host's canonical IDs.
2. **Normalized path** -- strip `./` prefix and compare.
3. **Host alias map** -- populated by the LSP or unplugin from tsconfig `paths` or bundler aliases.
4. **Parent file's resolved dependency set** -- suffix matching against known dependencies.
5. **Basename matching** -- compare just the filename portion.
6. **Extension guessing** -- append `.vue` and retry.

## Provide/Inject Validation

As a secondary analysis, the optimizer validates that `inject()` calls have a matching `provide()` call somewhere in the ancestor chain of the render tree. When no provider is found, the optimizer emits an `INJECT_NO_PROVIDER` diagnostic.

## Invalidation

On recomputation (e.g., after a file save), the optimizer diffs the current constness results against the previous ones. Only files whose constness hints actually changed appear in `changedFiles`, and only those files need recompilation. Files with identical hints are left untouched.

## Edge Cases

| Scenario | Behavior |
|----------|----------|
| **Cycles** | Detected via a visited set during traversal. All props involved in cycles are marked `Dynamic`. |
| **Dynamic components** | `<component :is="...">` usages are skipped entirely -- the target cannot be statically resolved. |
| **`v-bind` spread** | A parent using `v-bind="obj"` on a child prevents **all** prop optimizations for that child, since any prop could be passed dynamically through the spread. |
| **External components** | Components from `node_modules` are ignored -- their source is not in the host's file set. |
| **Conditional branches** | Both sides of `v-if`/`v-else` contribute call sites. A prop must be const in **all** branches to qualify. |
| **Root components** | Components with no parent in the render tree conservatively keep all props `Dynamic`, since they may receive props from a router or external caller. |
