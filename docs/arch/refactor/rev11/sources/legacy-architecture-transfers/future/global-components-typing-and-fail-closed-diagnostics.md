# Global-component typing and fail-closed diagnostics

Status: partially resolved. Verter now supplies `@verter/types` virtually when
the project does not install it, and the real-provider harness stages the
fixture's declared framework dependencies hermetically. The setup, unknown-tag,
and custom-element contracts pass on both providers.

The two Options-arm cases remain DEFERRED and are NOT retired. Two separate
things must not be conflated here:

- **The locator is fixed.** `assert_global_component_surface` used to address the
  Options fixture's PascalCase tag with a contiguous byte search, which cannot
  match a tag whose attributes the fixture authors on their own lines; it panicked
  before reaching any provider assertion. It now resolves that element through the
  real Vue parse
  (`RealProviderTestSession::find_template_tag_position`), so the shared body runs
  to its provider assertions in both arms. The fixture is unchanged.
- **The provider gap is not re-measured as OPEN — it is observed CLOSED on a
  small sample, which is not the same as retired.** Both Options cases now
  complete their full bodies under require mode on this tree (see Measurements),
  so no current failure locus is claimed here. The historical
  native-only/publication-ordering symptom diagnosed below is what the deferral
  is about, and it was INTERMITTENT: a handful of green runs cannot distinguish
  "fixed" from "did not reproduce today".

Neither Options case may be credited as passing or retired on the sample
recorded here. Retirement needs the intermittency itself ruled out — repeated
require-mode runs on CI hardware, not one workstation — and a named cause for
why the native-only hover no longer wins.

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

Historically, the test setup materialized only `@verter/types`; the nested
fixture declared Vue in its own `package.json`, but a root
`pnpm install --frozen-lockfile` did not install that nested package.
Consequently `import("vue").GlobalComponents` was an unresolved error type.
TypeScript propagated that error surface as `any` through `keyof` and indexed
access, so the emitted const was `any`, not the registered member type and not
the alias's apparent `unknown` fallback.

The harness now copies declaration-only fixture dependencies from the workspace
installation, while Verter supplies its own types through provider-virtual
fallbacks. A project-installed `@verter/types` package remains authoritative.

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

## Verification

From a fresh checkout, the resolved non-Options matrix is:

```powershell
pnpm install --frozen-lockfile
$env:VERTER_REQUIRE_TSGO=1
$env:VERTER_REQUIRE_TSSERVER=1
cargo test -p verter_lsp --lib global_component_tag_typed_in_setup_arm -- --nocapture
cargo test -p verter_lsp --lib global_component_unknown_tag_fails_closed -- --nocapture
cargo test -p verter_lsp --lib custom_element_tag_stays_fail_open -- --nocapture
```

The full module still includes the Options pair, which stays outside this
resolved matrix. Their shared assertion helper now locates the PascalCase tag
structurally rather than by byte search, so the bodies reach their provider
assertions instead of panicking in the harness, and on the sample below both
bodies complete. The fixture is untouched, and neither variant is credited as
passing on that sample — the symptom this deferral tracks is intermittent.

## Measurements

Historical measurements that motivated the fix:

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

Measured on the tree carrying the structural locator (macOS workstation,
`node_modules` present, tsgo resolved from the workspace install):

```bash
VERTER_REQUIRE_TSGO=1 VERTER_REQUIRE_TSSERVER=1 \
  cargo test -p verter_lsp --lib global_component -- --nocapture --test-threads=1
```

- All EIGHT cases pass, including both Options cases. Every line reports
  `RECEIPT … require_mode=1 status=body-returned`, and no `SKIPPED-WARMUP`
  receipt or `skipping (…)` line appears — under require mode each skip path
  panics, so the bodies ran their assertions against live providers.
- Repetition: three consecutive `VERTER_REQUIRE_TSGO=1` runs of the same filter
  (8/8 each) plus one run with both require flags set (8/8). No failure observed.
- Non-vacuity control for the previously claimed failure locus: planting
  `GlobalCountCompZZZ` into the kebab tail-column hover assertion
  (`crates/verter_lsp/src/real_provider_tests/global_components.rs`) turns
  `global_component_tag_typed_in_options_arm_tsgo` RED, and the panic prints the
  live hover, which does contain the Pascal binding. So that assertion really
  executes and really passes; the earlier "still fails at the kebab tail" claim
  does not hold on this tree. The plant was reverted.

Paired full-suite measurement, same workstation, same session — the whole
`verter_lsp` lib surface run once on the tree WITHOUT the structural locator and
once WITH it, comparing failure SETS rather than counts:

```bash
cargo nextest run -p verter_lsp --lib --no-fail-fast
```

- Without the locator: 2159 tests, 36 failed. Both
  `global_component_tag_typed_in_options_arm_tsgo` and `…_tsserver` are in that
  failure set.
- With the locator: 2173 tests (+14 locator unit tests), 32 failed. Neither
  Options case is in that failure set.

That pairing is what the four isolated repetitions above could not give: the two
cases move from FAIL to PASS across the locator change on one machine in one
session. The rest of the delta is unrelated churn in both directions (three
base-only failures, one head-only), which is exactly why the count is not the
evidence and the set is.

This still does NOT retire the deferral. The symptom recorded above was
intermittent at 1-in-5, so a green sample on one machine is consistent with both
"fixed" and "did not reproduce"; the FAIL→PASS pairing establishes that the
harness locator was a real cause of the failures, not that it was the only one.
What is established is narrower and only that: the Options bodies run to
completion under require mode, they fail without the locator and pass with it in
a paired full-suite run, and no specific current failure site may be asserted.

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
