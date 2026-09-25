# STP19 evidence index

Engine pins: `tests/sfc-projection/STP1/products/engine-matrix.json`. Vue pin: `vue@3.6.0-rc.5`.

Selected case IDs: `STP19-explicit`, `STP19-higher-rank`, `STP19-forward`, `STP19-overloads`, `STP19-foreign`, `STP19-erasure`, `STP19-instantiation-alias`.

Commands (run on the candidate; this file is not an execution transcript):

- `node scripts/sfc-projection/verify-node.mjs --node STP19 --engine all --require-all --json`
- `cargo test -p verter_compiler --lib ide::vue_projection::generic_interop`
- `cargo test -p verter_compiler --test main advanced_generic_uses_reads_admitted_carrier_blocks`
- `node --test --test-name-pattern "STP18|STP19" scripts/sfc-projection/verify-node.test.mjs`
- `cargo fmt --all -- --check`
- `cargo clippy --workspace --all-targets -- -D warnings`

Products: `AdvancedGenericUseProjection`, `ForeignComponentContractAdapter`.

Implementation home: `crates/verter_compiler/src/ide/vue_projection/generic_interop.rs`. Dormant consumer: `VueProjectionBackend::advanced_generic_uses`, derived from the same `ProjectionPlan`, attribute-operation and component-use products of the admitted parse plus the carrier's authored `generic` binder. Vue IDE `project_ide`, the `tsc/script` carrier and the Svelte route are unchanged.

Changed symbols: `generic_interop::{ForeignComponentContractAdapter, AdvancedGenericUseProjection, AdvancedGenericUse, UseContractAvailability, project_advanced_generic_uses, USE_COMPONENT, USE_CONTRACT, USE_CONSTRUCTS, USE_CALLS, USE_TOLERANT, USE_FUNCTIONAL, USE_OPEN_ARGS, USE_SCOPE}` (new); `VueProjectionBackend::advanced_generic_uses` (new) plus the product re-exports; `component_use::{USE_PRELUDE, ComponentUseWitness::render, ComponentUseProjection::render_witnesses}`: the construction callee is the adapter and the prelude embeds the adapter declarations; `public_constructor::{render_binder_list, public_binder}`: the binder list renderer and binder parse shared with the use scope (no second binder renderer). `packages/types/src/components/components.ts` needed no change: the rendered prelude declares the adapter next to the witnesses.

Adapter shape: `new (__VerterUseComponent(Comp, __VerterUseConstructor(Comp)))({ ... })` is the intersection of the component's exact contract and an attribute-tolerant signature, so TypeScript's own overload resolution picks the first signature that accepts the use:

- a typed constructor is passed through whole (every construct overload, every binder: constraints, defaults, `const`, dependent and variadic parameters), with no fixed overload count;
- a constructor whose last construct signature is open `new (...args: any[])` (the Options API `defineComponent`, or a hand-written catch-all) is rebuilt signature by signature in declaration order (`__VerterUseConstructs`): each open signature takes the `$props` its instance publishes instead of accepting anything, and each earlier precise overload stays selectable ahead of it;
- a functional component with one call signature is consumed through it (props parameter, context `slots` / `emit`) by higher-order inference, so a generic functional component keeps its binder; an overloaded functional component is rebuilt signature by signature in declaration order (`__VerterUseCalls`), each rebuilt overload taking exactly its declared props (no attribute index), so an earlier overload never absorbs the prop that selects a later one; generated SFC defaults are never made callable;
- the rebuild walks the signature list from its end with no signature count; a signature with its own type parameters has no identical non-generic copy, so the walk cannot pass it. The walk then keeps every signature it read (that generic signature, at its constraints, through the last), never the component's own type, so an open catch-all is never reopened;
- the tolerant fallback (the last signature's props plus an `unknown` attribute index) accepts undeclared fallthrough attributes; it is never part of a rebuilt overload, and an exact overload always wins over it.

Every use of a parent is placed in one generic function over the parent's authored binder, so a forwarded parameter stays the parent's parameter. A use whose component cannot be named is recorded `Unavailable` with no witness; a component TypeScript types as `any` keeps `any`; an erased generic stays `unknown`.

Deletion population this node: the fixed single-signature construction adapter of the migrated component-use path (the former `__VerterUseConstructor<P, I>(component: abstract new (props: P) => I)` as the sole callee, which kept only the last construct overload and widened Options API components to `any`). The live Vue route keeps its own helpers until atomic activation and is recorded for STP58's retirement: `instantiateComponent` / `componentConstructor` in `crates/verter_compiler/src/ide/script/type_constructs.rs` and `ide/script/comp_emit.rs`, and `instantiateComponent` in `packages/types/src/components/components.ts`.

Known TypeScript limit (open finding F5, raised for an architect ruling): TypeScript's type system reads only the last signature of an overload set and cannot copy a generic signature, so in a rebuilt set the signatures declared ahead of a generic signature are not enumerable, and the generic signature itself is constructed at its constraints (its binder is not inferred per use). A lone generic call signature keeps its binder through higher-order inference; a typed constructor without an open catch-all is passed through whole and keeps every generic overload.

Known failures under F5 (the reviewer's reproductions, open): with `Mix` declared as `(props: { kind: "a"; n: number })`, `<T>(props: { kind: "g"; value: T })`, `(props: { kind: "z"; s: string })`, the use `kind: "a"` fails with TS2322 (`"a"` is not assignable to `"z"`) and `kind: "g"` constructs `value: unknown`; with the pair `<T extends string>(props: { kind: "list"; items: T[] })`, `<T extends string>(props: { kind: "one"; item: T })`, only `"one"` is selectable and `item` is `string`, not `"x"`. A direct call selects every one of these overloads and infers `T`.

Why the direct call is not the projection's fix either (`tests/sfc-projection/evidence/STP19/ts-limits/`, clean on both pinned engines with `tsc -p tests/sfc-projection/evidence/STP19/ts-limits`; each limit is an assertion or an `@ts-expect-error` that fails if a later TypeScript lifts it):

- L1–L3: conditional inference reads the last signature only, erases a generic signature's binder, and an intersection drops only an identical signature copy, so no signature-by-signature rebuild passes a generic call signature;
- L4: value-level higher-order inference keeps a binder for a single signature only;
- L5–L6: `new` on a value with call signatures only resolves every overload but yields `any` with TS7009 under `noImplicitAny`, and a call on a value with construct signatures only is TS2348, so no one application syntax checks both component kinds, and the projection renders from the template without knowing which kind a component is (it answers no types);
- L7: rendering every use as a call and converting constructors instead moves the same loss to constructors (a generic construct overload beside another overload is dropped);
- L8: a direct call of the component's own value (gated so a callable passes through whole) does select every overload and infer `T`, but its result is the component's return type, never the chosen overload's props, slots or emit that the use's observations read, so it cannot be the use's one construction; as an extra validation it renders the authored props twice and reports each argument diagnostic twice;
- `jsx.tsx`: JSX is the one syntax TypeScript resolves against a value's construct signatures, else its call signatures, keeping every overload and binder. A `.tsx` checking module cannot host `lang="ts"` setup code verbatim (`<T>value` assertions), and an element's type is `JSX.Element`, so it gives no specialized instance.

Closing F5 therefore needs a decision beyond this node's surfaces: a type-directed or JSX-based construction for callable components, with its own observation channel, or a ruling that scopes `STP19-overloads` to overload sets without a generic call signature ahead of other signatures.

Not claimed here (owned elsewhere): an overloaded component's undeclared fallthrough attributes are tolerated only by its last signature (excess-key and attribute obligations are STP20's); listener props of a plain generic functional component whose declaration publishes only a context `emit` are not reconstructed from that `emit` (the dependency's declared props are what the adapter reads); event payload overloads (STP22); slot-scope placement of nested uses (STP24); global/async/recursive component resolution (STP32); composition of setup statements into the checking module (STP58).

## Why the negative cases discriminate

Each `reject` row, and each accept row's forbidden twin, is exercised in `protocol.mjs`:

- Rust source patches against `generic_interop.rs` (`assertDirtyTwinsRejected` requires a unique anchor, proves the plant landed, writes it, requires every named discriminator to report `FAILED` in the unit lane and, where named, in the production `--test main` carrier lane, then restores clean):
  - callee reduced to the tolerant single signature — `generic_use_construction_applies_the_exact_contract_first`, `probe_fixtures_are_the_rendered_products`, carrier test
  - open-argument constructors passed through (widened) — `foreign_contract_declarations_keep_published_shapes`, `probe_fixtures_are_the_rendered_products`
  - use scope without the parent binder — `generic_use_scope_carries_the_authored_binder`, `probe_fixtures_are_the_rendered_products`, carrier test
  - a fabricated witness for an unavailable use — `generic_use_availability_keeps_unavailable_uses_unwitnessed`, carrier test
  - the contract's non-constructor result fabricated as `any` — `foreign_contract_declarations_keep_published_shapes`
- TypeScript rewrites of a copy of a pinned probe (`assertTsTwinsRejected`, on every resolved engine, the clean probe first holding the opposite verdict):
  - callee reduced to the tolerant single signature — the first of six overloads (`kind: "circle"`) stops resolving in the positive probe
  - open-argument constructors widened — the Options API `count: '1'` construction error disappears
  - the contract's non-constructor result as `any` — the functional component's `level: 4` error disappears
  - the explicit alias replaced by its base component — the alias's fixed `K = "id"` no longer rejects `field: "label"`
  - the open catch-all rebuilt as the whole contract (`open-catch-all-replaces-earlier-overloads`) — the precise `unit: "celsius"` overload of `Gauge` stops resolving in the positive probe
  - an overloaded callable matched to its last call signature only (`call-overloads-collapse-to-the-last`) — the non-last `mode: "on"` overload of `Toggle` stops resolving
  - every rebuilt call overload given the attribute index (`call-overloads-tolerate-attributes`) — `Menu`'s first overload absorbs `flag`, so the positive probe's `$props.flag` read fails
  - a generic construct signature returning the declared constructor (`generic-construct-overload-reopens-catch-all`) — `Select`'s open catch-all accepts `label: 1` again and the anchored TS2769 disappears
  - a generic call signature discarding the overloads already read (`generic-call-overload-drops-read-overloads`) — `Mix`'s generic `kind: "g"` overload stops resolving
- Anchored negatives (`assertAnchoredNegatives`): every construction violation is exactly one TS2769 at an authored member (for the six-overload use, TypeScript reports its last candidate's failure exactly as for `new Shape(...)` written directly); every violated explicit argument is exactly one TS2344 at the authored argument token.

The tsc probes carry the product's own rendering: `probe_fixtures_are_the_rendered_products` pins the rendered projections inside `positive.ts`, `negative.ts` and `negative-construction.ts`, and `picker_probe_fixture_is_the_rendered_declaration` pins the child component to the public-constructor product. Through both pinned engines the positive probe is diagnostic-free: the forwarded `Picker` slot item is the parent's `T` and its value `T["label"]`; the slot's `map` and the `format` prop keep their own binders per call (hover `number[]`); the explicit alias `Picker<Row, "id">` reached through a renamed re-export and a namespace fixes every channel; Options API, generic setup-function, functional and generic functional dependencies keep their published props, payloads and slots; both the first and fifth of six construct overloads resolve; both precise overloads ahead of `Gauge`'s open catch-all and both non-last `Toggle` call overloads resolve; `Menu`'s later overload is selected by its extra `flag`; `Select`'s generic overload and `Mix`'s generic overload ahead of a non-generic one resolve; the erased item stays exactly `unknown` and the untyped component stays `any`. The negative probe raises only customer TS2322 diagnostics.

## Review findings

Settled in earlier review rounds:

- A trailing `any[]` construct signature replaced every earlier overload — fixed by the signature-by-signature constructor rebuild; discriminated by `open-catch-all-replaces-earlier-overloads`.
- Callable component overloads collapsed to the last call signature — fixed by the signature-by-signature callable rebuild; discriminated by `call-overloads-collapse-to-the-last`.
- Source-map preservation of generic argument tokens was covered only structurally — covered by `generic_use_witness_values_keep_their_authored_origin`.

This round:

- F1, an attribute index on every rebuilt call overload hid a later overload — rebuilt call overloads take exactly their declared props; discriminated by `call-overloads-tolerate-attributes`.
- F4, a generic construct signature reopened the `any[]` catch-all — the walk keeps what it read instead of returning the declared constructor; anchored negative `Select :label="1"` plus `generic-construct-overload-reopens-catch-all`.
- F5, one generic call signature dropped non-generic siblings — overloads read up to the generic signature are now kept (`generic-call-overload-drops-read-overloads`); the overloads declared ahead of a generic signature remain unreachable and a generic call signature stays at its constraints (see the known TypeScript limit and its evidence above); open for an architect ruling.
- F6, this evidence file did not describe the shipped rebuild — updated here.

Raw outcomes: see the CI artifact owner for the candidate run; local qualification is the charter §14 command above.
