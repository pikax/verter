# BV1 — landing record

Base `ff2e0217e`. Candidate `943867e12`. Dispatch context: [`context-packet.md`](context-packet.md).

## What shipped

- Seed-goldens oracle repinned `3.6.0-rc.1` → `3.6.0-rc.3`; the tracked divergence ledger
  (`crates/verter_vue_conformance/corpus/known-divergences.json`) regenerated against it and
  then closed to empty — every tracked Vue Vapor/VDOM codegen divergence for the 32-SFC seed
  corpus fixed for real, verified byte-for-byte against the vendored official compiler
  output, not comparator-tuned. Seed conformance suite: 95/95 cells PASS.
- Real corrections landed in `crates/verter_compiler/src/template/code_gen/{vapor,vdom,shared,types.rs}`,
  by area: event-listener modifiers (authored option order, `withModifiers`/`withKeys` import
  wiring, `.prop`/`.attr` DOM-property-vs-attribute routing); `v-text`/`v-html` lowering to
  props, directive-wrap block nesting, inline-handler caching, `NEED_HYDRATION` vs `PROPS`
  patch-flag policy for inline topology; component-resolution helper family
  (`createComponent`/`createDynamicComponent`, Vapor-native built-in names), bracket-access
  tag resolution; static/dynamic text hoisting, whitespace condensing, repeated-`_ctx`-read
  caching within one render effect; `v-for`/`v-slot` destructured-parameter collapse
  (including rest elements and default values) into scope-local bindings matching official's
  accessor-path rewriting; slot forwarding through the non-stable-root `_extend` wrapper form;
  multi-slot insertion-state/nav-chain topology inside a real DOM parent, including routing
  text mixed with structural siblings through the shared nav chain instead of a standalone
  extraction off the wrong node; `<template v-if>`/`<template v-for>` as a transparent
  wrapper with no DOM footprint, matching VDOM; a same-name `v-bind` shorthand fix so an
  authored-but-blank value no longer silently aliases to the shorthand binding.
- Two real, independently-confirmed runtime bugs found and fixed along the way (not merely
  conformance mismatches): a Vapor `<slot :total="count">` dynamic prop silently never
  reaching the slot call; a Vapor `<button>` mixing dynamic text with `<slot>` siblings
  attaching its reactive text update to the wrong DOM node (the header slot's anchor,
  instead of its own position) — a real hydration-correctness defect, not just a byte diff.
- The structural-conformance comparator's own `scope_ordinal` (`verter_vue_conformance`,
  `canon/classify.rs`) fixed to rank by the semantic scope's own `ScopeId` creation order
  instead of a raw AST-node index sensitive to cosmetic paren-node counts — the sole
  remaining entry this was blocking (`script-setup/props-type-withdefaults`) closes with a
  new discriminator recipe proving the fix does not introduce a blind spot for a genuine
  extra scope.
- A ratified BV0→BV1 debt row closed for real: `<template v-if>` wrapping a nested `v-if`
  is now a transparent Vapor codegen wrapper; the previously-`#[ignore]`d runtime-mount
  regression test (`template_v_if_wrapping_inner_v_if_mounts_and_renders_inner_content`,
  `crates/verter_session/src/compile/map_equality_tests/nested_v_for_runtime_proof.rs`)
  passes for real against the pinned with-vapor runtime (`--features bf2-authoritative`).
- A `v-bind` blank-value/same-name-shorthand conflation bug fixed: an explicit, authored,
  empty expression (`:id=""`) no longer silently aliases to the Vue 3.4+ same-name shorthand
  (`:id`); the previously-defined-but-unwired `X_V_BIND_NO_EXPRESSION` diagnostic is now
  emitted, matching official's own recovery behavior.

## Not closed, disclosed and pinned

- `<slot>` element `v-bind` spread and dynamic prop key (`:[key]`) — confirmed genuine data
  loss (the spread silently contributes nothing; the dynamic key becomes a bogus static
  literal prop named `"[key]"`), but closing it needs official's shared `{ $: [...] }`
  merge-array prop form, a materially separate feature from the flat-object form the
  current function builds. Pinned by two named characterization tests
  (`scoped_slot_outlet_spread_is_silently_dropped`,
  `scoped_slot_outlet_dynamic_key_emits_wrong_literal_key`).
- `v-for`'s object-shorthand default-value destructuring (`{ id = 99 }`) — the v-for LHS
  parses as a plain `Expression`, not a binding pattern, so this shape (valid only in a
  binding-pattern grammar) never reaches codegen as a recognizable AST node; closing it
  needs a parser-mode change with wider blast radius (reference extraction, liveness
  collection) across every consumer of the current `Expression`-shaped v-for AST. Pinned by
  a named characterization test (`v_for_object_shorthand_default_value_stays_disclosed_gap`).

## Review arc

Three-mandate review against the fully-closed-backlog candidate: codex (conformance,
`gpt-5.6-sol`/high), grok (architecture, `grok-4.6`/high, explicit default-to-BLOCK), Claude
subagent (adversarial, isolated worktree, genuine plant→RED→revert→GREEN against three
independent bugs plus the comparator fix). Adversarial: PASS, one non-blocking finding
(discriminator recipe for the ScopeId fix didn't isolate the mechanism — corroborated
independently by codex). Codex: BLOCKING, five findings. Two were concrete and verified —
the `<template v-if>` debt row and the v-bind blank-value conflation — fixed in a follow-up
round along with the discriminator-soundness finding and, going beyond the review's own
ask, the two destructuring/slot-outlet gaps it flagged as suspicious (one turned out to be
a real fixable bug, one a confirmed-but-separate-scope gap, both now honestly disclosed
above rather than silently left as loose ends). Codex's remaining five findings
(`FC-HYDRATION-001`/`FC-TS-001-LOCAL`/`FC-ATOMIC-001`/`FC-ZERO-WORK-001`/`FC-PERF-001` "not
independently evidenced") are recorded in context-packet.md — codex's read-only sandbox
could not execute any test command (`cargo` build-lock permission denial), and its
diff-scoped read does not see predecessor-block evidence (these acceptance IDs originate
from BF3/B4, already ACCEPTED into the ledger). The canonical gate below independently
confirms nothing regressed program-wide; no fresh evidence contradicts the predecessors'
own proof of these criteria.

## Discriminated as pre-existing / environmental, not regressions

- `native_content_handoff::external_template_ide_compile_contains_selected_bytes` — zero
  diff in this candidate against `native_content_handoff.rs` or anything in its call path
  (unrelated to Vue codegen — an external-source IDE-lowering refusal); the same
  already-discriminated failure B4's own landing record lists, reproduced identically on
  all three gate surfaces (nextest, in-process libtest, shipped `no-debug-assertions` cfg).
- `store_view_build_wall_cost_is_flat_across_host_sizes` and
  `resilient_tests::failed_respawn_retries_within_budget_and_recovers` — both
  program-documented known-flaky baseline; did not appear as failures in this run's gate
  invocation.

## Verification

- Canonical Rust gate (`node scripts/gate.mjs --memory-limit 18GiB`) ran once, at landing
  readiness, after a fresh worktree's build-prerequisite preflight was satisfied
  (`pnpm install --frozen-lockfile` + `pnpm --filter @verter/language-shared --filter
  @verter/typescript-plugin build`). Terminal three-surface summary: Surface 1 (nextest,
  process isolation) 24656 run / 24655 passed / 1 failed; Surface 2 (direct in-process
  `verter_session` libtests) 2 suites clean, 1 with the same already-discriminated failure;
  Surface 3 (shipped `no-debug-assertions` cfg, `verter_session`+`verter_scheduler`) 8678
  run / 8677 passed / 1 failed. The single non-tolerated failure named on all three surfaces
  is the one discriminated above.
- `cargo fmt --all --check`, `cargo clippy --workspace --all-targets -- -D warnings`,
  `cargo clippy --target wasm32-unknown-unknown -p verter_wasm -- -D warnings`,
  `cargo check --workspace --release` — all clean.
- `cargo test -p verter_vue_conformance -- --test-threads=1`: 8/8 passed, ledger confirmed
  empty (`{"schema": 1, "cells": []}`).
- `cargo test -p verter_compiler --lib`: 6193 passed, 0 failed, 12 ignored (floor at
  dispatch start: 6165; +28 new tests across the whole dispatch, 0 removed/weakened, ignored
  count unchanged — the 12 are all pre-existing Svelte-scoped, none from this candidate).
- `cargo test -p verter_session --lib --features bf2-authoritative
  compile::map_equality_tests -- --test-threads=1`: 62 passed, 0 failed, 0 ignored —
  includes the newly-un-ignored `<template v-if>` runtime-mount proof passing for real.
- No TypeScript/JavaScript source changed; `pnpm test` not required per the program's
  end-of-change rule for a change confined to Rust crates. No CSS files touched (CSS work
  remains suspended to a later train). No type-resolution/type-checking correctness code
  touched (types waived program-wide).
