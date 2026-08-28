# Vue known-defect correction deviation memo

## Failed assumption

BF3's charter assumed every BF2-probed successful cell that proves wrong should be
retracted through typed non-success or tracked pending later correction. The
maintainer's direct product decision forecloses both for Vue VDOM, Vapor, and SSR:
production compilation must continue returning success and generated output, with no
retraction, typed non-success, publication guard, known-divergence allowlist, or
temporary tracker/backlog mechanism.

## Measured evidence

BF3's probe against BF2's exact `vuejs/core v3.6.0-rc.3` seed manifest found genuine
confirmed defects, not probe artefacts, including but not limited to:

- Missing `__expose()` emission in every non-inline `<script setup>` cell. Verter
  conflates "an authored `defineExpose` call exists" (`MacroState.has_expose`, set in
  `crates/verter_compiler/src/script/macros.rs:306-329,368-376`, consumed at
  `script/process.rs:441-445,762-768`) with "the generated non-inline setup function
  requires the expose binding." Official Vue rc.3 binds `expose: __expose` when
  `hasDefineExposeCall || !inlineMode` and inserts `__expose();` when
  `!hasDefineExposeCall && !inlineMode`
  (`compiler-sfc.cjs.js:15658,15728`).
- Dropped VDOM `<slot>` fallback static-caching / `CACHED` patch-flag optimization.
  The existing `_cache[N]` machinery in `emit_slot_children_with_cache`
  (`crates/verter_compiler/src/template/code_gen/vdom/slots.rs:1695-1772`) is used
  for component slots and `<template v-slot>` but not for `<slot>` fallback content,
  which instead follows the older `add_children_separators_array` path
  (`slots.rs:209-217`). Slot-context classification at `vdom/mod.rs:855-864` also
  omits slot outlets, and the single-static-text cache-wrapper arm at
  `slots.rs:1741-1750` omits the official `-1 /* CACHED */` argument.
- Additional genuine seed-matrix defects beyond those two named ones: invalid Vapor
  module references/imports producing an undeclared `x0` and an unimported
  `_setText`; incorrect VDOM keyed/stable-fragment topology; incorrect dynamic-props
  membership; setup-return differences; and independent source-map differences.
  These are real backend findings, are not proven to share the two named root causes,
  and must not be silently deferred or recorded as tolerated divergences.
- The reported Svelte-client 112/0 result used `svelte@5.56.3`
  (`scripts/svelte-golden-lib.mjs:32`), not BF2's authoritative `svelte@5.56.8`
  domain
  (`packages/framework-conformance-harness/src/domain-pin.mjs:66-76`). The 12 BF2
  Svelte cells have no genuine candidate comparison yet, so exact-domain Svelte
  client remains a real, open BF3 probe obligation.
- Svelte server already returns a typed `ServerGenerate` non-success at
  `crates/verter_compiler/src/svelte/runtime/client_compile.rs:113`; it is already a
  non-successful cell and needs no new BF3 mechanism.

## Affected invariants

- The conformance-golden rule that an enabled successful cell carries no semantic
  known-divergence remains in force. The conflict is resolved by correction, not by
  suspending that rule.
- BF3's retraction-only remediation model for Vue VDOM/Vapor/SSR is superseded by
  BV0 correction.
- The DAG's single `BF3 -> {B2, B3}` edge is widened so both `BV0` and `BF3` are
  required predecessors of B2 and B3.

## Consequences of not correcting

Without correction, either the maintainer's explicit product decision is violated by
retracting or tracking Vue output that must keep succeeding, or the conformance-golden
no-known-divergence rule is silently violated by leaving confirmed defects in
successful cells with no correction path. Both outcomes are rejected.

## Recommended amendment

AMD-006 adds BV0 as an immediate, bounded Vue-correction block between BF2 and
{B2, B3}; narrows BF3 to Svelte and non-Vue-runtime scope against the exact
`svelte@5.56.8` domain; requires BV1 to preserve every BV0 correction on the final
substrate; and excludes the superseded "track, don't retract" isolated-worktree
record from landing. No replacement tracking artifact is created.
