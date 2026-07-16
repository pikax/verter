# Strict props + fallthrough (product contract)

Verter is **strict-first**. This is intentional product design, not a bug vs Volar.

## Vue

| Surface | Meaning |
| --- | --- |
| **Declared props** | `defineProps` / Options `props` only |
| **Accepted call-site attrs** | Declared **plus** computed **fallthrough** (root inheritance) |
| **Unknown prop** | Attr not in the accepted surface → **diagnostic / type error** |

### Why not Volar-loose?

Volar often accepts props that were never declared (loose call-site). Verter does **not**: if it is not declared **and** not proven by fallthrough, it fails.

### Why fallthrough is the differentiator

Verter owns multi-hop **root inheritance** (component → component → native):

- Single native root → intrinsic attrs (`class`, `style`, `data-*`, `aria-*`, listeners) minus declared/consumed
- Single component root → recursive propagation through the child’s accepted surface
- Fragment / unsupported root → **no** inherited surface → extras **fail**
- `inheritAttrs: false` → no automatic inherited surface (extras are a separate policy; fixtures document current IDE acceptance)

So when Verter **allows** `class` on a wrapper that only declares `tone`, that is because fallthrough proved a native leaf accepts it — not because unknown props are ignored.

Public meta fields: `props` (declared) vs `acceptedProps` / `fallthroughSurface` (call-site acceptance). See `/component-meta` → Fallthrough / Root Inheritance.

### E2E

- `parity/vue/fallthrough.test.ts` — deep chain accept + fragment reject
- `parity/shared/strict-props.test.ts` — unknown must fail; fallthrough attrs must pass
- Legacy `attrs-fallthrough.test.ts` on single-project

## Svelte

Svelte is **not** Vue fallthrough. There is no multi-hop “attrs fall through component roots” model like Vue’s `$attrs` + `inheritAttrs`.

| Surface | Meaning |
| --- | --- |
| **Declared props** | `$props()` / export let — **only** these type-check by default |
| **Unknown prop** | Not in `$props` → **type error** (strict, same spirit as Verter Vue) |
| **Rest / spread** | Author **opts in** with `...rest` (and a typed index signature when needed). That is deliberate, not automatic fallthrough. |

So:

- **Same as Verter Vue:** undeclared props fail.
- **Different from Verter Vue:** there is no deep fallthrough engine; “extra” attrs are only legal if the author models rest props (or equivalent).

E2E: `strict-props.test.ts` Svelte arms (`StrictUnknownProp`, `StrictRestOk`).

## One-line summary

**Vue:** strict unknown + **correct** fallthrough acceptance.
**Svelte:** strict unknown + **explicit** rest if you want extras — no Vue-style fallthrough.
