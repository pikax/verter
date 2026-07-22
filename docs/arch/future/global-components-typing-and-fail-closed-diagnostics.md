# Global-component typing and fail-closed diagnostics

Status: deferred by product-owner ruling. This note records the eight
`real_provider_tests::global_components` cases; it does not authorize weakening
or ignoring them.

## Affected tests and observed symptoms

The affected matrix is both providers for each contract:

- `global_component_tag_typed_in_setup_arm_{tsgo,tsserver}`
- `global_component_tag_typed_in_options_arm_{tsgo,tsserver}`
- `global_component_unknown_tag_fails_closed_{tsgo,tsserver}`
- `custom_element_tag_stays_fail_open_{tsgo,tsserver}`

On a fresh root-only install, the registered-tag cases fail with assertions such
as:

````text
setup: tag hover must not degrade to any/unknown, got: ```typescript
const GlobalCountComp: any
```
````

The tsserver path can instead expose only Verter's native tag hover:

```text
options: kebab tag hover must resolve the Pascal component binding, got: **`<global-count-comp>`**
**Props:**
- ✓ `:count` — *const*
```

The negative control fails independently:

```text
unknown tag must carry a fail-closed diagnostic, got: []
```

The custom-element cases are not failures of the configured custom element
itself. Their later, registered kebab-only control loses the provider type:
tsgo reports `const GlobalCountComp: any`; tsserver can return no `:count` hover.

## Mechanism

### 1. Why a registered component degrades to `any`

The compiler collects unresolved component tags in
`crates/verter_compiler/src/ide/script/wrapper.rs:45` and emits a synthetic
fallback const in `emit_global_component_fallbacks` at
`crates/verter_compiler/src/ide/script/wrapper.rs:196`:

```ts
const GlobalCountComp = {} as ___VERTER___GlobalComponentType<'GlobalCountComp'>;
```

The real helper declaration is `packages/types/index.d.ts:9`:

```ts
export type GlobalComponentType<N> =
  N extends keyof import("vue").GlobalComponents
    ? import("vue").GlobalComponents[N]
    : unknown;
```

The test setup at
`crates/verter_lsp/src/real_provider_tests/global_components.rs:32`
materializes only `@verter/types`. The nested fixture declares Vue in its own
`package.json`, but a root `pnpm install --frozen-lockfile` does not install that
nested package. Consequently `import("vue").GlobalComponents` is an unresolved
error type. TypeScript propagates that error surface as `any` through `keyof` and
indexed access, so the emitted const is `any`, not the registered member type and
not the alias's apparent `unknown` fallback.

This is why finding both engine executables does not establish this test's type
environment: the engines run, but the module on which the conditional type is
defined is absent.

### 2. Why the unknown tag emits no fail-closed diagnostic

This is a distinct containment defect downstream of the same missing-module
trigger. The intended unknown-name branch is `unknown`, which makes the emitted
JSX identifier non-callable and produces TS2604. Error-`any` prevents the
conditional from reaching that branch. JSX accepts an `any` tag, so tsgo and
tsserver correctly return no TypeScript diagnostic. The LSP diagnostic merge
does not synthesize TS2604, and must not fabricate one; the result is the observed
empty list at `global_components.rs:291`.

In other words, installing Vue restores the registry lookup, while an explicit
error-`any` containment guard is what would preserve fail-closed behavior when
that lookup cannot be formed. These are separate remedies.

### 3. Why a typed provider result can still disappear

Kebab tags are rewritten to their Pascal fallback binding at
`crates/verter_compiler/src/ide/template/mod.rs:400`. The per-segment mapper at
`crates/verter_compiler/src/ide/template/mod.rs:1753` is intended to retain every
authored letter, including the tail. Hover then queries the provider in
`crates/verter_lsp/src/server/nav_features.rs:308`; if the provider returns no
result or the captured provider surface is no longer valid at
`nav_features.rs:431`, merge returns the already-available native tag hover at
`nav_features.rs:495`.

The global-component test's `hover_with_retry` at
`global_components.rs:53` treats any non-empty hover as ready. A native-only tag
hover is non-empty, so it stops before the provider-specific assertion can
observe a later typed response. This explains the raw-tag symptom and is
separate from the type alias becoming `any`.

## Minimal reproduction

From a fresh checkout:

```powershell
pnpm install --frozen-lockfile
$env:RUST_MIN_STACK=268435456
cargo test -p verter_lsp --lib real_provider_tests::global_components -- --nocapture
```

That is the root-only state in which `@verter/types` is materialized by the test
but the fixture's declared Vue dependency is not installed. Inspect hover on
`<GlobalCountComp>` and diagnostics on `<TotallyUnknownComp>`.

For the dependency control, install the fixture from its own working directory,
then repeat the same Rust command:

```powershell
Push-Location packages/vue-vscode/e2e/fixtures/single-project
npm install --no-package-lock --ignore-scripts
Pop-Location
```

Do not use `npm install --prefix`: it can rewrite the fixture manifest's local
package entry.

## Measurements

- Untouched root-only full-module run: all eight cases failed. Registered-tag
  hovers included `const GlobalCountComp: any`; both unknown-tag cases returned
  `[]` diagnostics.
- After installing the fixture dependency, before the tsserver publication
  ordering fence: 4 passed / 4 failed. All four tsgo cases passed; tsserver
  returned native-only/no-result surfaces.
- After the general tsserver publication fence, with the fixture dependency
  present: 7 passed / 1 failed in one eight-test run. The remaining options/tsgo
  case was intermittent; five isolated repetitions produced 4 passes / 1
  native-only hover failure.

The ordering fence was implemented for the non-global carrier-publication defect;
these improved global results are incidental. No global-component-specific code
was changed.

## Future fix and falsifiable predictions

The future work should land as three independently falsifiable changes:

1. Make the test environment honor the fixture manifest before spawning either
   provider (or materialize a hermetic Vue declaration with the augmentation
   surface). Prediction: on a clean machine, both registered Pascal hovers show
   the component type and both unknown cases reach the `unknown` branch. Removing
   that setup must reproduce `const GlobalCountComp: any`.
2. Add an `IsAny`-style containment guard around the imported
   `GlobalComponents` surface in the owning source
   `packages/types/src/components/components.ts:246`, then regenerate the
   standalone declaration artifacts. Prediction: deliberately making `vue`
   unresolved yields an `unknown` fallback and TS2604 for Pascal-authored unknown
   tags, never `any`/empty diagnostics. Registered tags remain fully typed when
   Vue is present. The kebab/custom-element fail-open arm must remain callable.
3. Make the TSGO carrier publication receipt order the first interactive query,
   or otherwise retry a native-only component tag result until the bounded
   provider readiness outcome is known. Prediction: at least 50 repetitions of
   both setup/options arms contain `GlobalCountComp` at the first and last kebab
   columns, with no native-only result and no new global reload mechanism.

## Blast radius and constraints

- The helper aliases cover every unresolved Vue component tag, including
  auto-imported/global registry members; changing them can affect tag hover,
  props, events, definitions, and diagnostics.
- `GlobalComponentKebabType` in `packages/types/index.d.ts:12` deliberately keeps
  unregistered web components fail-open. Any containment change must preserve
  that behavior while keeping Pascal-authored unknown components fail-closed.
- Publication ordering affects all carrier hover/definition/diagnostic routes,
  not only global components. It must remain project-scoped and bounded; a global
  `reloadProjects` on every edit is not acceptable.
- No part of this diagnosis requires changing `ProjectSemanticDispatch`,
  `SemanticGraphStore`, macro/component-meta resolution, `verter_semantic`, or
  `verter_type_expr`.

This work is deferred solely because the product owner designated global
components as the allowed remaining failure group.
