# STP20 evidence index

Engine pins: `tests/sfc-projection/STP1/products/engine-matrix.json`. Vue pin: `vue@3.6.0-rc.5` (`@vue/compiler-sfc` and `@vue/runtime-core` boolean casting in `normalizePropsOptions` / `resolvePropValue`).

Selected case IDs: `STP20-default`, `STP20-required`, `STP20-spread-extra`, `STP20-union`, `STP20-optional`, `STP20-overwritten`.

Commands (run on the candidate; this file is not an execution transcript):

- `node scripts/sfc-projection/verify-node.mjs --node STP20 --engine all --require-all --json`
- `cargo test -p verter_compiler --lib ide::vue_projection::props`
- `cargo test -p verter_compiler --lib probe_fixtures_are_the_rendered_products`
- `cargo test -p verter_compiler --test main props_reads_the_caller_contract_from_the_admitted_carrier`
- `cargo fmt --all -- --check`
- `cargo clippy --workspace --all-targets -- -D warnings`

Products: `PropCheckObligation`, `CallerAndSetupPropsContract`, `SpreadCertaintyPolicy`.

Implementation home: `crates/verter_compiler/src/ide/vue_projection/props.rs`. Dormant consumer: `VueProjectionBackend::props`, derived from the admitted parse's attribute operations and public-constructor macro facts plus the script blocks. Vue IDE `project_ide` and the Svelte route are unchanged. STP58 owns Vue atomic activation.

Changed symbols: `props::{CallerAndSetupPropsContract, caller_and_setup_from_blocks, SpreadCertaintyPolicy}` (caller/setup facts now include reactive destructuring defaults, Boolean absence casting, runtime validators and required keys; `witness_types` is the qualification rendering; `caller_and_setup_props` remains the test-only reparse control); `component_use::USE_PRELUDE` (`__VerterUseMemberOpen`, `__VerterUseSupply`, `__VerterUseOpen`, `__VerterUseKnownSpread`, `__VerterUseFallthrough`): a finite spread is checked per prop-union branch, so a key that exists on only one arm is not an excess key, and a spread type is preserved when it is `any`, an index signature, or a union that contains one. Overwritten keys are supplied to that check, matching `{...spread, ...those keys}`. A direct misspelled attribute is rejected; hyphenated attributes, `on*` listeners, and global fallthrough attributes are not.

Caller/setup split, matching Vue 3.6: a static `withDefaults` key and a reactive destructure default (`const { title = "x" } = defineProps<...>()`) are optional for callers and defined in setup. `withDefaults` disables reactive destructure. A runtime `default` defines the setup value; the prop stays required when `required: true`. A non-required Boolean prop (type includes `Boolean`) is optional for callers and defined in setup because absence casts to `false`; an empty string casts to `true` unless `String` precedes `Boolean`. A required Boolean stays required. Validators are recorded and do not change optionality. An open `withDefaults` object proves no key optional, so authored required props stay required.

The positive probe pins `CallerAndSetupPropsContract::witness_types` for `const { title = "fallback" } = defineProps<{ title: string; count: number; flag?: boolean }>()`. `props_reads_the_caller_contract_from_the_admitted_carrier` reads that witness through `VueProjectionBackend::props`.

Deletion population this node: none.

A naked type parameter is checked the way TypeScript checks `{...value, ...overwritten keys}`: it is accepted when those keys cover the required props (optional remainder included), and rejected when a required prop is still missing. It is not collapsed to `never` and not widened to `any`. A constraint that contradicts a required key is rejected. Excess keys that arrive only through the spread are accepted.

Not claimed here (owned elsewhere): live IDE routing (STP58), event payload overloads (STP22), model write/modifier contracts (STP23).

## Why the negative cases discriminate

The negative probe raises TS2345 for the cases below and TS2379 for the explicit `undefined` optional prop (`exactOptionalPropertyTypes`):

- a finite misspelled key (`titel`) is on no prop-union arm
- a finite `title: 1` is rejected when that key is not in the overwritten list
- `{ kind: "number", label }` matches neither arm of `{ kind: "number"; value: number } | { kind: "text"; label: string }`
- `{ kind: "number", value: "x" }` is a mismatched discriminated pair

The positive probe accepts the overwritten `title: 1` (the key is listed), `Record<string, unknown>`, `any`, a union of a finite object with `Record<string, unknown>`, both branch-specific spreads `{ kind: "number", value: 1 }` and `{ kind: "text", label: "ok" }`, a naked `T` spread whose required `title` is overwritten, and direct `id`, `data-test`, and `onClick` fallthrough keys. exactOptionalPropertyTypes stays on: the caller witness omits `title` and `flag` and still requires `count`.
