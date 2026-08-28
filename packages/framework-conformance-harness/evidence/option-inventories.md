# Option inventory provenance and rules

The two TSV inventories were resolved from the exact immutable upstream trees, not
from current Verter option structs:

- Vue commit `3adb225775c9b28223a56e07f7a2f874b6fbb138`:
  `packages/compiler-core/src/options.ts`,
  `packages/compiler-sfc/src/{parse,compileScript,compileTemplate,compileStyle}.ts`,
  and `packages/compiler-sfc/src/template/transformAssetUrl.ts`.
- Svelte commit `44a7813730579b94004e182e5a67aab27aa9d2a6`:
  `packages/svelte/types/index.d.ts` (`svelte/compiler.parse`, `CompileOptions`,
  `ModuleCompileOptions`, and `OptimizeOptions`) and
  `packages/svelte/src/compiler/types/template.d.ts` (source-authored
  `<svelte:options>` custom-element fields).

Inherited Vue option keys are listed once for the semantic key and repeated only
where a phase-specific treatment differs (`TransformOptions` hooks and
`CodegenOptions.scopeId`). `compatConfig` enumerates every RC.3 compiler deprecation
key under its ParserOptions row; TransformOptions inherits the same complete refusal.
Nested option objects are expanded when their members independently affect semantics
(`compatConfig`, Vue CSS modules/assets, Svelte compatibility/experimental, and
Svelte's source-authored custom-element descriptor and per-prop settings).

Svelte's compiler-level `customElement` is the exact boolean/callback union in 5.56.8;
the separately inventoried object fields are the official source-authored
`<svelte:options customElement={...}>` descriptor, not configuration-plugin options.
Callback union forms for compiler-level `customElement`, `css`, and `runes` are
host-resolved to their exact primitive value before the canonical request. The
source-authored `extend` callback is unsupported fail-closed; arbitrary callbacks
never become semantic authority inside the compiler core.

The parse API is classified against its established Verter product boundary:
`filename` is unused by the pinned official parser, `modern` changes only the returned
official AST (which is not an established Verter product), and `loose` changes error
recovery and therefore fails closed until that semantic mode is explicitly claimed.

Every row has exactly one classification from the seven-value closed set. `derived`
means B3 computes it solely from canonical fields; `host-resolved` means the host
provides immutable normalized data and B3 validates its compatibility. `external`
does not authorize bundling a preprocessor. `test-only` cannot cross production
request construction. `not applicable` cannot widen the public product set.
