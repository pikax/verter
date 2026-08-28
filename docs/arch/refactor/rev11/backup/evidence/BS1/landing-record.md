# BS1 — landing record

Base `6c3939734`. Candidate `fce37476e`. Dispatch context: [`context-packet.md`](context-packet.md).

## What shipped

Seven real Svelte client compiled-output defects fixed, measured against the pinned official
Svelte compiler (`svelte@5.56.8`), in `crates/verter_compiler/src/svelte/runtime/`:

- a top-level function declaration's emitted name lost its source-map provenance; the mapping
  now anchors on the declaration's real parsed name offset instead of a fixed keyword literal,
  degrading to unmapped rather than asserting on `async`/generator forms
- a shorthand attribute binding's reactive property write lost its authored provenance
- a single-name object-shorthand each-item destructure (`{ id }`) bound the whole item object
  under the field's name; now routes through a synthesized item param with a per-field getter,
  matching official. The fix is scoped to the shape it is provably correct for
  (`PatternShape::ShorthandSingleProperty`); a renamed/array/rest single-name decomposition
  fails closed at classification time with a real source span, a legitimate pre-compilation
  capability boundary rather than a miscompile-avoidance withhold
- an each keyed by its own index binding was wrongly treated as keyed
- a member bind rooted at an each item (`bind:value={item.x}`) was wrongly refused the same as
  a bare item bind
- a non-ASCII identifier panicked on a char-boundary slice in the expression rewriter
- a runes component whose each collection subscribed a store wrongly kept the item-immutable
  flag set

## Not closed, disclosed and pinned

- A genuinely multi-name each-item destructure (`{ a, b }`) refuses through a pre-existing,
  unrelated code path carrying a placeholder `Span::new(0, 0)` rather than the pattern's real
  location. Verified pre-existing at base `6c3939734`. Pinned by
  `a_multi_name_each_item_destructure_refuses_with_a_placeholder_span`. Debt row proposed in
  the context packet.
- BS1's charter "Required exits" paragraph (`FC-*` IDs, "every BF3 guard removed") rests on
  unratified AMD-005 text and is not claimed closed by this candidate — see context packet
  scoping section. This candidate closes real, verified Svelte compiled-output defects in the
  charter's "Owned scope" instead.
- Legacy-mode (non-runes) component `bind:` codegen omits the official `$$legacy: true`
  props-object marker entirely. Found while reviewing commit `b78dc9cd8`: its
  `a_member_bind_rooted_at_an_import_is_accepted_for_a_component_bind` test used a fixture with
  no rune calls, so it compiled in legacy mode where official emits an `$$legacy: true` key in
  the child's props object; Verter's actual output omits that key, and the test's loose
  assertions didn't catch it because it wasn't testing for it. Confirmed no production code
  anywhere emits an `$$legacy: true` key into a component-bind props object — a real,
  pre-existing, broader gap in Verter's legacy-mode component-bind codegen, unrelated to the
  member-bind-root axis this block fixes. The test itself was corrected in place (added an
  unused `let { p } = $props();` to force runes mode, sidestepping the gap rather than
  encoding it as passing) rather than left to assert around a known-wrong output. This gap
  needs its own investigation/fix as a separate item.
- Official `svelte@5.56.8` accepts a BARE-identifier `bind:` rooted at a genuine `$derived(...)`
  rune declarator (`bind:value={derivedThing}`, emitting `$.set(derivedThing, $$value)` —
  Svelte 5's documented "overridable derived" feature). A `{let x = $derived(e)}` TEMPLATE
  DECLARATION TAG (`declaration_tag_lowering.rs::lower_declaration_tag` +
  `state_prep::classify_block_rune_declarator`) reaches this: `is_writable_bind_root`
  (`client_shapes.rs`) now admits `BindingRuntimeKind::Derived` at the bare-Identifier arm, and
  a `let:` component slot-prop — which shares `Derived`'s read shape but not its
  reassignability — now mints the distinct `BindingRuntimeKind::SlotPropDerived` kind at its
  sole minting site (`lower_component.rs::lower_slot_region`), so it stays excluded from that
  admission and keeps refusing (`constant_binding`, oracle-verified) at the bare-Identifier arm
  while remaining admitted at the MEMBER arm (`is_writable_member_bind_extra_root`, which
  admits both `Derived` and `SlotPropDerived`). No new codegen arm was needed — the setter
  already dispatches on `is_signal_kind`, which both `Derived` and `SlotPropDerived` satisfy.
  `a_member_bind_rooted_at_a_declaration_tag_derived_rune_is_accepted` (`client_tests.rs`) now
  asserts the bare-Identifier form positively (`$.get(doubled)` / `$.set(doubled, …)`), proven
  to fail against the pre-fix tree and pass post-fix; the `let:` slot-prop negative control in
  `a_member_bind_rooted_at_a_derived_binding_is_accepted` keeps refusing, re-verified.
  This bare-identifier "overridable-`$derived`" form remains out of scope to fix or even to
  observe for a TOP-LEVEL INSTANCE-SCRIPT `let d = $derived(e)` declarator specifically: that
  form refuses at an earlier, unconditional, unrelated gate
  (`rune_scan.rs::classify_rune_position` — "`$derived` has NO supported position … a
  deferral-ledger follow-up") that this fix does not touch.

## Review arc

Three-mandate review on the initial 7-fix candidate: codex (conformance), grok (architecture,
default-to-BLOCK), Claude subagent (adversarial, isolated worktree). Adversarial: PASS. Codex
and grok both independently found the single-name-destructure fix BLOCKING (conflated binding
name with property key); codex additionally found a debug-only panic risk in the function-decl
mapping fix; grok additionally found an illegitimate zero-span production withhold on the fix's
own emit path. Fix round 1 resolved all three (verified directly against source, not just
review claims). A targeted codex delta-review on just the fix-round diff confirmed both closed
and surfaced the pre-existing multi-name-destructure gap described above, dispositioned as an
added characterization test per the standing bugs-and-types ruling rather than a third fix
round.

## Post-landing correction: component-bind sibling of the member-bind fix

An unprimed adversarial review of this commit found that the member-bind-on-each-item fix above
(`bind:value={item.x}` inside `{#each}`) widened only the DOM-bind writability classifier
(`bind_member_root_is_writable_target` in `client_shapes.rs`); the component-bind projection
(`component_bind_root_is_writable` in `client_component_plan.rs`) still routed its Member arm
through the unwidened `bind_root_is_writable_target`, so the identical case on a custom component
(`<Child bind:value={item.x} />` inside `{#each}`) still refused with
`UnsupportedSvelteRuntimeSurface::Binding`.

Verified against the pinned official `svelte@5.56.8` oracle before fixing: official ACCEPTS
`<Child bind:value={item.x} />` inside `{#each}` (emits `get value() { return item.x; }` /
`set value($$value) { (item.x = $$value); }`) and REFUSES the bare `<Child bind:value={item} />`
with `each_item_invalid_assignment` — the same accept/refuse split as the DOM-bind sibling, so
the widening was correct to extend. Fixed by routing the component-bind Member arm through the
same shared `bind_member_root_is_writable_target` predicate (commit `752a84e3c`), with a
discriminating test (`a_member_bind_rooted_at_an_each_item_is_accepted_for_a_component_bind`)
proven to fail against the pre-fix tree and pass against the post-fix tree. Two pre-existing test
gaps flagged by the same review were closed alongside: a missing positive assertion on the
single-name-destructure getter thunk, and a missing setter-closure assertion on the DOM-bind
sibling test.

Verifying that fix against real compiled output caught a second, narrower defect in the new
test itself: the assertions initially expected the official pretty-printer's parenthesized setter
body (`(item.x = $$value)`); Verter's own compact codegen emits the assignment unparenthesized
(`item.x = $$value`). Corrected in commit `d7f6d3f2e`, re-verified with the same
fail-before/pass-after proof.

`cargo test -p verter_compiler --lib svelte::runtime`: 1901 passed, 0 failed, 5 ignored (net +1
test vs. the original landing figure below).

## Discriminated as pre-existing / environmental, not a regression

- `native_content_handoff::external_template_ide_compile_contains_selected_bytes` — zero diff
  in this candidate against `verter_session` or `framework_common` (confirmed via
  `git diff 6c3939734..block/bs1 --stat -- crates/verter_session/
  crates/verter_compiler/src/framework_common/`, empty). Same already-discriminated failure
  named in B4's and BV1's landing records, reproduced identically on all three gate surfaces.

## Verification

- Canonical Rust gate (`node scripts/gate.mjs --test-threads 8 --memory-limit 18GiB`) ran once,
  at landing readiness, after a fresh worktree's build-prerequisite preflight was satisfied
  (`pnpm install --frozen-lockfile` + `pnpm --filter @verter/language-shared --filter
  @verter/typescript-plugin build`). Terminal three-surface summary: Surface 1 (nextest) 24668
  run / 24667 passed / 1 failed; Surface 2 (direct in-process `verter_session` libtests) 2
  suites clean, 1 with the same already-discriminated failure; Surface 3 (shipped
  `no-debug-assertions` cfg) 8678 run / 8677 passed / 1 failed. The single non-tolerated
  failure named on all three surfaces is the one discriminated above.
- `cargo fmt --all --check`, `cargo clippy --workspace --all-targets -- -D warnings`,
  `cargo clippy --target wasm32-unknown-unknown -p verter_wasm -- -D warnings`,
  `cargo check --workspace --release` — all clean.
- `cargo test -p verter_compiler --lib`: 6205 passed, 0 failed, 6 ignored (floor at dispatch
  start: 5 ignored pre-existing in this crate outside the touched module; +7 un-ignored and
  fixed, +5 new tests added green, +1 new characterization test added ignored — net -6).
- `cargo test -p verter_compiler --lib svelte::runtime`: 1900 passed, 0 failed, 5 ignored.
- No TypeScript/JavaScript source changed; `pnpm test` not required per the program's
  end-of-change rule. No CSS files touched. No type-resolution/type-checking code touched
  (types waived program-wide).
