# Missing / thin IDE parity cases

Intentional backlog of **discriminating** cases. Prefer absolute contracts
(diagnostics, `@ts-expect-error`, hover needles, apply edits).

**Scorecard authority:** TypeScript + Verter product contracts only. Official
LS (Volar / Svelte) may inform *implementation*, never the gate — they can be
wrong (e.g. loose unknown props). Path to all dimensions ≥9: `SCORECARD.md`.

**All-green ≠ production certainty:** see `CONFIDENCE.md`. Confidence suite:
`parity/shared/confidence.test.ts`.

**Strict props:** see `STRICT_PROPS.md` — Verter Vue is strict-first (unknown fails)
with **correct fallthrough acceptance**; Svelte is strict declared props + optional rest
(no Vue multi-hop fallthrough).

Legend: **done** = suite exists · **thin** = smoke only · **gap** = not covered

## Type surface (props / events / slots / attrs)

| Case | Vue | Svelte | Notes |
| --- | --- | --- | --- |
| Wrong prop type live + `@ts-expect-error` | done | done | `type-negatives` + BadProp |
| Wrong event / callback overload | done | done | emit + native click / onclick |
| Wrong directive / attr types | thin | thin | disabled, v-html, class — expand custom directives |
| **Scoped slots / snippets wrong prop methods** | **done** | **done** | `slots.test.ts` |
| **Unknown slot name / wrong `{@render}` args** | **done** | **done** | SlotWrongNames / SnippetWrongRender |
| Required slot missing | gap | gap | strictSlots / required snippet |
| Slot name completion (`#he…`) | done | done | `ide-navigation` |
| Slot prop member completion | done | done | `ide-navigation` |
| Ctrl+Click event / slot / prop | done | done | `ide-navigation` |
| Event handler local completion | done | done | `ide-navigation` |
| Directive / bind completion | done | thin | `ide-navigation` |
| Auto-import component tag | done | done | `ide-navigation` |
| Auto-import symbol accept | done | thin | Vue computed; Svelte local |
| Conditional / dynamic slot names | gap | gap | |
| Forwarded / nested slots | gap | gap | |
| Generic slot props (`T` from parent) | gap | gap | |

## Macros / framework surface

| Case | Vue | Svelte | Notes |
| --- | --- | --- | --- |
| defineProps / $props hover | thin | thin | |
| defineEmits / callback props | thin | thin | |
| defineModel / bindable | thin | thin | |
| defineExpose public vs testing | thin | n/a | testing-api suite |
| defineOptions / svelte:options | gap | gap | |
| withDefaults | gap | n/a | |
| $state / $derived / $effect | n/a | thin | runes suite |
| $props.id / $bindable edge | n/a | gap | |
| Generic SFC / generic snippet | done | done | `generics-advanced` (script `generic=""`) |
| Infer T from options → value/events | done | done | GenericSelect / GenericField |
| Defaulted generic `T = string` | done | done | GenericDefault |

## IDE features

| Case | Vue | Svelte | Notes |
| --- | --- | --- | --- |
| Rename across script + template/markup | thin | thin | |
| Rename slot prop both sides | gap | gap | |
| Find refs slot prop | gap | gap | |
| Signature help in handlers | thin | thin | lsp-extras |
| Inlay hints | thin | thin | |
| Code action apply (fix type) | thin | thin | |
| Organize imports apply | thin | thin | |
| Format document | thin | thin | |
| Semantic tokens | thin | thin | |
| Document highlights multi-region | thin | thin | |
| Intrinsic element interfaces | done | done | shared suite |
| Intrinsic attr completion after space | gap | gap | |

## Control flow & narrowing

| Case | Vue | Svelte | Notes |
| --- | --- | --- | --- |
| v-if / {#if} narrow hover | thin | thin | |
| v-for / {#each} locals | thin | thin | |
| Discriminated union narrow | gap | gap | |
| {#await} then/catch locals | n/a | thin | |
| {#key} remount | n/a | gap | |

## Style / CSS

| Case | Vue | Svelte | Notes |
| --- | --- | --- | --- |
| Scoped class def/ref | thin | thin | |
| :global isolation | thin | thin | |
| CSS modules | gap | gap | |
| v-bind() in CSS | thin | n/a | |

## Project / ecosystem

| Case | Vue | Svelte | Notes |
| --- | --- | --- | --- |
| Path alias @/ | thin | thin | ecosystem fixture |
| $lib / #imports | thin | thin | stubs only |
| Multi-root isolation | thin | thin | folders + light hover |
| Mixed Vue↔Svelte import | thin | thin | |
| Nuxt-like pages/composables | gap | n/a | layout fixture only |
| SvelteKit routes/$app | n/a | gap | |
| Composite / project references | thin | thin | legacy fixtures |

## JS (not only TS) carriers

| Case | Vue | Svelte | Notes |
| --- | --- | --- | --- |
| JS SFC / .svelte JS hover | thin | thin | |
| JSDoc prop types | gap | gap | |
| // @ts-check in script | gap | gap | |

## Error recovery & DX

| Case | Vue | Svelte | Notes |
| --- | --- | --- | --- |
| Broken script still types earlier bindings | thin | gap | |
| Incremental typing + undo | thin | thin | |
| Keystroke auto-import accept | thin | gap | DX harness |

## Honesty rails

| Case | Status |
| --- | --- |
| No virtual path leak | done (harness) |
| No open IntrinsicElements[string]:any | done |
| Public surface no secret leak | done (Vue); Svelte parallel |
| @ts-expect-error unused fails | done (type-neg + slots) |
| Required matrix ID set | done (all tree-discovered cases required; exact loaded-suite attestation) |

When adding a case: write the **failing** diagnostic / expect-error fixture first, then wire `PRODUCT_GAP` ISSUE ids — do not weaken to name-only hovers.
