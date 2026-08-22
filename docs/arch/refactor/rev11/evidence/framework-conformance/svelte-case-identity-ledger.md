# Svelte official-case identity ledger

`svelte-official-cases.tsv`'s `case_id` column is a durable logical identity,
not a hash of the live upstream version pin. This file records the identity
policy and the reviewed transition log for every regeneration that touches
identity — so a routine Svelte patch bump stays mechanical instead of
invalidating all ~590 `evidence_id` cross-references into
`B2-parse-facet-svelte.md` every time.

## Identity contract

`case_id = "SVELTE-" + hash(SVELTE_CASE_ID_SALT + "\0" + source_locator)`,
truncated/upper-cased as before. Two properties by construction:

- **Durable across version bumps.** `SVELTE_CASE_ID_SALT` (defined in
  `generate-official-case-manifests.mjs`) is a FROZEN literal
  (`"svelte-5.56.8"` — the string from the generator's first-ever run). It is
  a namespace constant, not a live version tracker, and must never be updated
  to track `SVELTE_DOMAIN.commit`/`EXPECTED_SVELTE`. A locator's `case_id` is
  therefore a pure function of its own path — unaffected by which upstream
  commit that path currently resolves to.
- **Content-change-tolerant.** The hash input is the `source_locator` only,
  never `source_object` (the git blob/tree hash). A directory that gets an
  upstream bugfix or fixture update at the SAME path keeps the SAME
  `case_id` — it is still the same logical conformance case. `source_object`
  still records the live content hash, so a content change is visible in the
  manifest diff even though the identity does not move.
- **Genuine identity change only on locator change.** A locator that
  disappears from upstream retires its row entirely (no ID is reused). A
  locator that appears for the first time mints a fresh ID from the same
  formula. A rename is exactly "old locator disappears + new locator
  appears" — recorded below as a transition, not silently carried forward.

## What is now mechanical vs. what still needs a human, per bump

**Mechanical (no human decision required):**

- Every unchanged locator's `case_id` — regenerating never perturbs it.
- Every existing `evidence_id`/`B2-parse-facet-svelte.md` cross-reference
  for a locator whose content did not change.
- Detecting which locators are new, removed, or content-changed (this
  ledger's reconciliation is produced by a diff over `source_locator` +
  `source_object` between the previous and regenerated manifest — see
  `reverseEnumerateSvelteRows` / the coverage self-tests in
  `packages/framework-conformance-harness`).

**Still requires a human per bump:**

- Reviewing rows this ledger lists as "content-changed" or "transitioned"
  for silent semantic replacement (a same-path directory that became a
  fundamentally different test) before trusting the carried-forward ID —
  the review recorded below, not a mechanical check.
- Running `verify-b2-parse-facets.mjs` against the new checkout to
  (re-)produce real parse-facet evidence for any NEW B2-owned row, and to
  confirm B2-owned content-changed rows still resolve (idempotent — it
  reprocesses every B2-owned row every run, so a changed verdict is visible
  as a diff in `B2-parse-facet-svelte.md`, not silently accepted).
- Updating the small set of live digest/count citations enumerated in this
  bump's commit (guard test SHA/counts, `performance-gates.toml` cell,
  `coverage.spec.mjs` row-count constants, AMD-010's factual counts).

## Transition log

### Bump: svelte@5.56.8 (`44a7813730579b94004e182e5a67aab27aa9d2a6`) → svelte@5.56.10 (`56a036f4ce873a24ee6631a06d03d372523d7a9b`)

Reconciliation method: regenerated `svelte-official-cases.tsv` against the
freshly pinned 5.56.10 checkout, then diffed every row by `case_id` against
the previously committed manifest.

- **Renamed/removed locators: 0.** No `case_id` present in the old manifest
  is absent from the new one, and no locator moved paths. (The identity
  scheme's durability made this the expected, unremarkable case — there is
  no rename to record.)
- **New locators: 18.** All are genuinely new upstream sample directories
  (net addition, not a renamed pre-existing case); none share a `case_id`
  with any prior row.
  - `css/samples/namespaced-type-selector` (BS1)
  - `parser-modern/samples/css-nth-of-minified` (**B2/BS1** — the one new
    B2-owned row this bump adds; see B2-parse-facet-svelte.md below)
  - `print/samples/{comment-inside-attribute,css-escape-sequences,css-namespaced-type-selector,element-content-wrapping,preserve-whitespace-elements,style-comments}`
    (BF1, `not_applicable` — official AST-printer product, outside Verter's
    compiler product boundary)
  - `runtime-runes/samples/{async-custom-element-attribute,async-each-controlled-empty-pending,class-private-fields-logical-assignment,class-private-state-increment-other-receiver,component-css-props-falsy,derived-destructured-from-derived,derived-forward-ref-leading-comments,effect-teardown-multiple-writes,labeled-statement-derived}`
    (BS1)
  - `server-side-rendering/samples/head-with-multiple-binding-components`
    (BS1)
- **Content-changed, same locator/ID: 11.** Reviewed individually against
  the real diff between the 5.56.8 and 5.56.10 trees (`git diff <old-tree>
  <new-tree>` inside the pinned checkout). Every one is a small, additive
  upstream fixture update tracking real Svelte compiler/parser evolution
  between these two patch releases — none replace the test's subject or
  purpose, so the `case_id` correctly carries forward unchanged.

  | case_id | locator | owner | nature of the diff (reviewed) |
  |---|---|---|---|
  | `SVELTE-646C0AA1B2791C868698` | `migrate/samples/props-export-alias/` | BF1 | `input.svelte`/`output.svelte` both gain a leading `<!-- @component x -->` doc comment — the migrate fixture now also exercises component-doc-comment passthrough alongside the existing `export { klass as class }` → `Props` interface migration; same migration subject, additive coverage |
  | `SVELTE-E5900A338A24CD878E0D` | `parser-legacy/samples/css/` | B2/BS1 | `output.json`'s `Style` node gains `"comments": []` — the parser now always emits a (here empty) `comments` array on `Style` nodes; AST schema field addition, no content/semantic change to the fixture |
  | `SVELTE-947F5A49ACF91F9DE76C` | `parser-legacy/samples/whitespace-after-style-tag/` | B2/BS1 | Same schema addition as `parser-legacy/samples/css/`: `output.json`'s `Style` node gains `"comments": []` |
  | `SVELTE-ED2DD7A86E4D9E723BFB` | `parser-modern/samples/css-nth-syntax/` | B2/BS1 | `output.json`: adds a `comments` array capturing a CSS comment already present in the fixture source — a parser fidelity improvement, not a new test subject |
  | `SVELTE-CB8C9C3614417F2AF1E3` | `parser-modern/samples/css-pseudo-classes/` | B2/BS1 | `output.json`: adds `args` (a `SelectorList`) under `PseudoElementSelector` nodes for `::view-transition-old(x-y)` / `::highlight(...)` — upstream's parser gained functional-pseudo-class-argument parsing between 5.56.8 and 5.56.10; still the same "parse CSS pseudo-classes" case, now exercising a richer construct upstream itself added |
  | `SVELTE-CB4370086413CA801955` | `parser-modern/samples/script-style-no-markup/` | B2/BS1 | `output.json`: the `StyleSheet` node (the `css` block) gains `"comments": []`, and the root now carries a `comments` array with one `Line` comment node capturing the fixture's pre-existing leading `<!-- script and style but no markup -->` HTML comment — the parser started structurally exposing top-level comments it previously discarded from the AST; same "script+style, no markup" subject, richer AST for pre-existing source text |
  | `SVELTE-BAFE48E1057D5E163062` | `parser-modern/samples/semicolon-inside-quotes/` | B2/BS1 | `output.json`: same `comments` array addition as above (`StyleSheet` node `"comments": []` and root-level `"comments": []`) — this fixture has no `instance`/script block at all, so both additions land on the `StyleSheet` (`css`) node and the root; both empty here since the fixture has no comments; schema addition only |
  | `SVELTE-8D17EC339A2A308287F0` | `print/samples/await-block/` | BF1 | `input.svelte`/`output.svelte` each append 5 more `{#await}` block variants (`catch error`-only, `then value`-only, `catch`-with-no-binding, combined `then`/`catch` arms, and `catch`-with-no-binding after a bare `{#await}`) — additive coverage extending the same await-block-printing subject to more of the `{#await}` clause grammar, not a new test |
  | `SVELTE-8C04F2465D2CB6E11401` | `runtime-runes/samples/async-bind-factory-function-remote/` | BS1 | `main.svelte` adds one more factory-returned closure form (`const arrow = () => () => checked`) rendered in a new `{#if true}{arrow()()}{/if}` block; `_config.js`'s expected `ssrHtml`/innerHTML strings grow from five to six `true` tokens to match — additive coverage of one more closure shape for the same "bind through a factory function" subject |
  | `SVELTE-0137D15B6AE9359599D1` | `runtime-runes/samples/event-attribute-spread-update/` | BS1 | `main.svelte` adds a third button whose `onclick` mutates the spread `attrs` object to drop `onclickcapture`; `_config.js` adds a `remove.click()` step and assertions confirming the capture handler is gone while the delegated handler keeps counting — extends the same "reactive event-attribute spread update" subject to also cover removing a handler via the spread object, not a new test |
  | `SVELTE-24DD806C407D446702C7` | `snapshot/samples/dynamic-attributes-casing/` | BS1 | `_expected/client/main.svelte.js`: the generated `template_effect` for the custom element's dynamic `fooBar` attribute changes from a plain-closure call (`() => $.set_custom_element_data(custom_element_1, 'fooBar', y())`) to the derived-args form (`($0) => ..., [() => y()]`) — a compiler codegen-pattern change for template-effect dependency tracking, not a change to what the fixture exercises (still: set a dynamic camelCase attribute on a custom element) |

  Of these, 6 are B2-owned (`parser-legacy` x2, `parser-modern` x4) — the "6
  parser-suite rows this bump touches" referenced in the implementer brief.
  Re-running `verify-b2-parse-facets.mjs` against the 5.56.10 checkout
  reprocesses all 589 pre-existing B2-owned rows (idempotent, from scratch)
  plus the 1 new one; the resulting `B2-parse-facet-svelte.md` is
  byte-identical to the previous file except for the one new
  `### SVELTE-E02C5AD1D1551DEA7C44` section — i.e. Verter's recorded parse
  verdict for all 6 content-changed B2-owned rows is unchanged (`pass`)
  despite the upstream content drift.

- **Net row count:** 3457 → 3475 (+18, 0 removed).

## New B2-owned row: `parser-modern/samples/css-nth-of-minified`

`SVELTE-E02C5AD1D1551DEA7C44` — verified via `verify-b2-parse-facets.mjs`
against the pinned 5.56.10 checkout and the `parse_corpus_probe` binary
(`cargo build -p verter_compiler --bin parse_corpus_probe --features
external-corpus`): classification `pass`, `verdict_hash`
`2c6870b2db80fa41`, single invocation `expected=valid outcome=ok
matches=true`. See `B2-parse-facet-svelte.md#svelte-e02c5ad1d1551dea7c44`.
