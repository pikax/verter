# IDE parity scorecard

Target: **every dimension ≥ 9** → **~90–95 / 100** on suite _completeness_
(tests exist and are discriminating), not on product green rate.

## Authority (what “correct” means)

| Allowed                                                                                                           | Forbidden as scorecard / gate authority                           |
| ----------------------------------------------------------------------------------------------------------------- | ----------------------------------------------------------------- |
| **TypeScript** semantics (assignability, overloads, narrowing, TS2578 unused `@ts-expect-error`)                  | Byte- or hover-matching **Volar** / **Svelte Official LS**        |
| **Verter product contracts** (strict unknown props, proven fallthrough, no open `IntrinsicElements[string]: any`) | “Official accepts it, so we must” when Official is loose or wrong |
| Absolute fixtures: diagnostics, apply edits, completion labels, definition targets on **authored** paths          | Installing other IDEs as oracles in CI                            |
| Optional: humans may glance at Official while **implementing**                                                    | Promoting Official bugs into Verter tests                         |

## Dimension scores (post densification → **~92**)

| Dimension                                    |  Weight | Score | Weighted | What landed for ≥9                                                            |
| -------------------------------------------- | ------: | ----: | -------: | ----------------------------------------------------------------------------- |
| Coverage breadth (features touched)          |      15 |     9 |     13.5 | Shared suites for type-neg, slots, ide-nav, strict, js, depth, style          |
| Coverage depth (apply edits, JS+TS, mapping) |      15 |     9 |     13.5 | `depth-apply`: rename apply, HTML rename reject, event/slot mapping, undo     |
| Honesty / anti-false-green                   |      10 |     9 |      9.0 | Tree-derived parity inventory; expect-error + live diags; failParityGap hard-fail |
| Vue IDE surface matrix                       |      12 |     9 |     10.8 | Matrix + fallthrough + strict + slots + ide surface clean rows                |
| Svelte IDE surface matrix                    |      12 |     9 |     10.8 | Public-surface, runes/bindable/effect, snippet/ide/strict matrix rows         |
| Typing / editing DX tests                    |      10 |     9 |      9.0 | ide-navigation auto-import + depth; depth undo; typing suites                 |
| JS language surface tests                    |       8 |     9 |      7.2 | `js-surface`: JsDaily + JS wrong-prop type errors both FW                     |
| CSS / style framework tests                  |       6 |     9 |      5.4 | Style def/ref + `:global` + local/global coexistence both FW                  |
| Find/rename/code-actions                     |       6 |     9 |      5.4 | Rename apply/reject + existing code-action/find suites                        |
| Ecosystem / multi-root / mixed               |       6 |     9 |      5.4 | Nuxt pages + Kit routes + multi-root dual hover/isolation + mixed wrong props |
| **Total**                                    | **100** |       |  **~92** | Product may stay red (`PRODUCT_GAP`); suite completeness is the score         |

## What the score means

- **~92** = suite is complete enough to **measure** replacement readiness under **TS + Verter contracts**.
- It does **not** mean product is green.
- It does **not** mean “matches Volar/Svelte Official.”

## Confidence (honest)

**Even if every test is green, we are not confident Verter “works all the time.”**

Green = covered absolute contracts on hermetic fixtures. See **`CONFIDENCE.md`**.

Additional hardening suite: `parity/shared/confidence.test.ts` (invalidation, cross-file wrong types, multi-file session, negative battery, no-virtual defs).

## Suites that bank the score (absolute contracts)

| Suite / area                | Path                                                                                                                    |
| --------------------------- | ----------------------------------------------------------------------------------------------------------------------- |
| Strict props + fallthrough  | `strict-props.test.ts`, `STRICT_PROPS.md`                                                                               |
| Type negatives              | `type-negatives.test.ts`                                                                                                |
| Slots / snippets            | `slots.test.ts`                                                                                                         |
| IDE Ctrl+Click + completion | `ide-navigation.test.ts`                                                                                                |
| Intrinsics                  | `intrinsic-elements.test.ts`                                                                                            |
| Testing API `.spec.ts`      | `testing-api-surface.test.ts`                                                                                           |
| Depth apply/mapping         | `depth-apply.test.ts`                                                                                                   |
| JS surface                  | `js-surface.test.ts`                                                                                                    |
| Style/CSS                   | `style-css.test.ts`                                                                                                     |
| Svelte public-surface       | `svelte/public-surface.test.ts`                                                                                         |
| Ecosystem Nuxt/Kit shape    | `ecosystem/paths.test.ts`                                                                                               |
| Multi-root / mixed          | `multi-root/`, `mixed/`                                                                                                 |
| Matrix densification        | `matrixCases.ts`                                                                                                        |
| Required IDs                | AST-derived `e2e-suite-build-manifest.json` via `parityTestInventory.ts`                                                |
| Advanced generics           | `generics-advanced.test.ts` — infer T from props, multi-prop/event linkage, defaulted `T = string`, Svelte `generic=""` |

## Explicitly out of the scorecard path

- Installing Volar / Svelte Official as CI oracles
- Copying Official loose unknown-prop acceptance
- Full real Nuxt/SvelteKit monorepos (shaped stubs suffice)

## Fixture map

| Fixture             | Focus                                     |
| ------------------- | ----------------------------------------- |
| `vue-parity`        | Vue matrix + daily + fallthrough + typing |
| `svelte-parity`     | Svelte matrix + runes + typing            |
| `mixed-parity`      | Vue+Svelte one project                    |
| `multi-root-parity` | Multi-folder workspace                    |
| `ecosystem-parity`  | `@/`, `$lib`, `#imports`, pages/, routes/ |

## Related

- `STRICT_PROPS.md` · `MISSING_CASES.md` · `HARDENING.md`
