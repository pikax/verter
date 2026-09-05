# CCA1O4 scope disposition: the typed wire-schema extension (compiler.compiler-bridge)

- Status: proposed — awaiting operator/architect ratification
- Date: 2026-09-05
- Disposition: `ADOPT-NOW` for the native wire-schema extension and the
  crates beyond CCA1O4's named surfaces, recorded here because the review
  rounds found the expansion undispositioned, not because a fix owner may
  ratify their own rescope.
- Amends: nothing. No other charter's budget, no DAG edge, no other node's
  ledger row. CCA1O4's own charter gains a pointer section to this record.
- Asks for: one operator decision (see the last section).

## Why this record exists

CCA1O4's charter states "Production surfaces are `packages/unplugin/src/index.ts`
and `packages/unplugin/src/core/compiler.ts`" and excludes "Native/NAPI
signatures, benchmark/tools, TypeScript plugin, WASM, and any profile
deletion". The candidate changes ten production files across
`verter_compiler`, `verter_ffi`, `verter_protocol`, `verter_session`,
`@verter/unplugin` and `@verter/native`, and extends the typed host-request
wire schema with three new fields and two new wire enums. Budgets are
planning references, so the footprint alone is not the issue; the explicit
*exclusion* of native signature work is a scope boundary, and crossing it
requires this record.

## What crossed the boundary, and why the charter's own invariant compels it

The wire extension, all in `crates/verter_protocol/src/types.rs` with its
converter in `crates/verter_ffi/src/convert/input.rs` and the regenerated
byte-pinned mirror `packages/native/host-compile-request.generated.ts`:

- `FfiHostCompileIdentity.ssr_module_id: Option<String>`
- `FfiHostCompileIdentity.hmr_strategy: Option<FfiHmrStrategy>` (new enum)
- `FfiRuntimeProductRequest.style_processing: Option<FfiRuntimeStyleProcessing>`
  (new enum)

Without these three fields the typed route cannot satisfy the charter's own
invariant "Preserve Vue/Svelte bundling, HMR, manifest":

- `hmr_strategy` — the typed request carried no HMR strategy, so a migrated
  Vue carrier lost the `__file` / hot-accept trailer that the legacy
  profile-bearing route emits. Dropping HMR is not a migration.
- `ssr_module_id` — the SSR product's `ssrContext.modules` key derives from
  it; without the field the manifest entry a server build publishes changes
  key.
- `style_processing` — the Vite authored-only cascade (the plugin owns
  preprocessing under Vite's CSS pipeline; the compiler must NOT re-run the
  complete cascade over authored bytes) was inexpressible on the typed
  request. `RuntimeStyleProcessing::default() == Complete`, so an absent
  field keeps the pre-migration behavior and is not a silent flip.

The additions are disciplined: every field is `Option`-typed,
`deny_unknown_fields` is preserved, and the converter had already recorded
this exact addition as the anticipated "wire addition, not a silent default
flip". The generated TypeScript mirror was regenerated through its checked-in
generator, and the byte-pin guard
(`crates/verter_napi/tests/cases/host_compile_request_ts_freshness.rs`)
passes against it.

## The alternative considered

Carrying the wire extension as its own CCA1O2-family node and reverting it
here is the other option the review named. It is not separable in practice:
without the fields the migration fails the charter's preservation invariant,
so reverting them unlands the node's outcome rather than trimming it. The
honest split would sequence "extend the typed wire" BEFORE "migrate the
unplugin consumer"; the candidate instead landed both halves together, which
this record discloses.

## What is asked of the operator

Ratify ONE of:

- **(a) Accept the expansion as scoped here** (recommended). The three
  fields and two enums are the minimum the preservation invariant requires;
  each is additive, optional, and fail-closed. CCA1O4's charter gains the
  pointer section below.
- **(b) Reject.** Then the wire extension reverts, the unplugin migration
  cannot meet its own acceptance, and CCA1O4 needs a rescope that sequences
  a wire-extension node ahead of it.

Until the ruling, this record stands as the disclosure; it is not itself an
approval.
