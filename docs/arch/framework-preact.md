# Preact delivery and exact-version contract

The Preact delivery contract records two profiles, native and compat, for one
carrier-less JSX adapter. It adds reviewed data and counterexample tables only.
The current repository has no Preact adapter or Preact wire tag; `.tsx` and
`.jsx` remain ordinary scripts. Nothing here claims delivered editor support,
conformance, runtime correctness or a new parser.

The inventory lives in [tests/framework-preact/PRE0](../../tests/framework-preact/PRE0/):

| File                                     | Contract                                                                                    |
| ---------------------------------------- | ------------------------------------------------------------------------------------------- |
| `manifest.json`                          | Four product paths, acceptance/counterexample inventory, and PRE1-ACV validator handoff     |
| `products/preact-version-lock.json`      | Exact reference releases, admission lines, registry sources, integrity and optional signals |
| `products/preact-activation.json`        | FWA1 roles, canonical alias targets, per-file narrowing, switches and zero-work states      |
| `products/preact-capability-matrix.json` | Operation × host × profile cells, sole producer/acceptance, facets, tag and exclusions      |
| `products/preact-host-table.json`        | LSPX11 transport, tsgo operations and typed limitations, joined to each matrix cell         |
| `cases.md`                               | Every planted row and its expected failure reason                                           |

FWA1 reads the release and activation rows as data. REG0 consumes registration
metadata; FCH1 joins the matrix to delivered fixture evidence. PRE1 adds the
executable lock validator and spec, including every planted row, under
PRE1-ACV. REG0's `scripts/run-framework-locks.mjs` discovers that future spec.
There is no validator, spec, schema enforcement or CI gate in this contract.

## Releases and profile identity

| Package           | Exact reference pin | Admitted stable major | Optional |
| ----------------- | ------------------- | --------------------- | -------- |
| `preact`          | 10.29.8             | 10                    | no       |
| `preact`          | 11.0.1              | 11                    | no       |
| `@preact/signals` | 2.11.3              | 2                     | yes      |

The exact pins and integrity values come from published npm package metadata:
[Preact 10.29.8](https://registry.npmjs.org/preact/10.29.8),
[Preact 11.0.1](https://registry.npmjs.org/preact/11.0.1), and
[signals 2.11.3](https://registry.npmjs.org/@preact%2fsignals/2.11.3).
They are reference artifacts, not changes to repository dependencies. FWA1
reads the actual resolved installed stable version within an admitted major
and carries that exact release. It never substitutes a reference pin for the
installed version. Preact 11 is a second admitted line, not a beta allowance;
all prereleases, canary/nightly, legacy majors and unratified majors are out.

One package contribution selects one Preact release. `preact-native` and
`preact-compat` are distinct profile identities, orthogonal to release, with
the same Preact adapter and tag. Signals contributes its own optional exact
release and activates only through FWA1; its published Preact peer requirement
must also hold. Without admitted signals, no signal facts or signal hover run.

## Activation and ownership

FWA1's `FrameworkActivation` is the sole activation source. Native consumes the
PM-resolved `preact` role. Compat consumes canonical `react`/`react-dom` alias
targets at `preact/compat`, activates Preact, and never activates the React
adapter. This follows the official
[aliasing guidance](https://preactjs.com/guide/v10/getting-started/#aliasing-react-to-preact);
Verter reads captured PM/config facts and does not execute build config.

A file pragma `@jsxImportSource preact` narrows that file to native Preact,
including inside a React package, subject to an admitted resolved release.
Extensions, unresolved import strings and identifier spellings are not
activation evidence. `off` wins and performs zero work. `on` cannot admit an
unsupported release; a manifest-free explicit activation names an admitted
exact release and records explicit provenance. Generated and Vue/Svelte
virtual carriers are not activated.

Shared parsing, PM resolution, TypeInfo, index, maps and edit transactions keep
their existing owners. There is no Preact parser, FileLanguage, attachment
codec, cache or independent resolver. Alias/provider edits invalidate only
affected identities/contributions; PRE1 and PRE2 own fresh/incremental proof.

## Facets and tag

Props come authoritatively from Preact's own types and JSX namespace, including
function, class, generic and compat components. Emits are derived callback
props; Slots are derived from children and render props, each with provenance.
Expose comes from `useImperativeHandle` in `preact/hooks` and compat `forwardRef`
identity. The official [hooks reference](https://preactjs.com/guide/v11/hooks/#useimperativehandle)
describes the imperative handle. Model and Options are UNSUPPORTED, never
empty supported facets. PRE3 owns projection, partiality and typed refusal.

`FRAMEWORK_TAG_PREACT = 6` is the fixed allocation. REG0 pre-allocates the tag;
PRE1 registers the family and owns PRE1-AC1. No proto edit lands here.
`OPEN_CANONICAL` is a structural non-tag and cannot identify Preact.

## Editor delivery

The ten cells cover five operations in both profiles. Every cell names one
producer and one receiving acceptance; fact dependencies do not add producers:

| Operation                          | Producer / acceptance | Required tsgo API operations                                              |
| ---------------------------------- | --------------------- | ------------------------------------------------------------------------- |
| Preact component hover section     | PRE5 / PRE5-AC1       | `getSymbolAtPosition`, `getTypeAtPosition`, `typeToString` (display only) |
| Signal-read hover section          | PRE5 / PRE5-AC2       | `getSymbolAtPosition`                                                     |
| Extra context-provider definitions | PRE5 / PRE5-AC2       | `getSymbolAtPosition`                                                     |
| Verter Preact diagnostics          | PRE8 / PRE8-AC3       | none; canonical facts                                                     |
| Fixes for owned diagnostics        | PRE8 / PRE8-AC3       | none; canonical facts and LSO8 transactions                               |

All cells use `lsp-enhancement` through LSPX11. Its selector covers
`typescriptreact`, `javascriptreact`, `typescript` and `javascript`; activation
still controls each file. `owns(capability, position)` applies the tsgo position
probe, COXD2 ownership and the user's per-capability setting. Unknown keeps
Verter ownership. Inactive and stepped-down cells do zero work. No ordinary
tsgo/TS result is re-provided, and no TS-plugin enhancement cell exists.

If TypeInfo refuses a fact requiring props-member or callback-signature
enumeration, the tsgo API cannot supply it: publish
`Unavailable(tsgoLimitation: operation)` and `$/verter/tsgoLimitation`,
deduplicated by operation/project. Never parse `typeToString` to recover facts
or promote refusal to Complete. The current operation names and display-only
boundary are grounded in `crates/verter_tsgo_api/src/proto/types.rs`; PRE3-AC4,
PRE5-AC4 and LSPX11-AC5 own the receiving proofs.

## Exclusions and evidence

Build, bundle, transforms, HMR setup and presets including
`@preact/preset-vite` are excluded and untouched. The separate upstream build
bridge supplies no editor-delivery proof here. Runtime execution, rendering,
hydration, instrumentation and dev servers are excluded; statically recording
`render`/`hydrate` call identities is allowed without executing them.

Formatting uses the shared TSX/JSX printer; PRE8F owns its corpus proof. Ordinary
language features remain with tsgo/TS. PRE5 registers LSO4 roles but does not
name a Preact-only rename operation, so this lock claims none. Unsupported
facets and every excluded operation retain explicit reasons in the matrix.

PRE0-AC1 through PRE0-AC4 are satisfied here by reviewed products and
`cases.md`, not executable acceptance claims. PRE1-ACV receives all lock
rejection proofs. No existing route is displaced, so the deletion set is empty.
Runtime, cold/warm cache, cancellation and map tests are not applicable to a
data-only change. The ratified CI ownership exception adds only
`tests/framework-preact/PRE0/**/*.json` and `tests/framework-preact/PRE0/*.md`
to `scripts/ci-inert-paths.json` (exported as `CI_INERT_PATHS`) atomically
with this inventory. PRE1-ACV removes these
entries and assigns the consumed inputs and executable specs to their existing
consuming lane when its validator lands. REG0 retains runner discovery.
