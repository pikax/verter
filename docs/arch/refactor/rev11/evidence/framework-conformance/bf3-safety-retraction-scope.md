# BF3 initial safety-retraction scope

> **UNRATIFIED supersession notice — do not implement:** AMD-009 proposes to
> supersede the `BF3-RET-*` production-record scheme below with audit dispositions
> and immediate correction-owner acceptance IDs. The amendment is UNRATIFIED, but
> the settled BF3 consult prohibits implementing this disputed retraction mechanism
> while ratification is pending. The text below remains only as historical scope
> evidence.

BF3 begins from all in-scope Svelte and non-Vue-runtime public/default requests that
currently report success, not from a hand-picked feature list. B3 is not yet available,
so the inventory must include each existing transport spelling that can reach the same
semantic request.

## Minimum probe matrix

| family | profiles/products to enumerate | minimum oracle |
|---|---|---|
| Svelte client | dev/prod; current runes/legacy claims; maps on/off; current option surface | parse/link, Svelte topology, runtime smoke |
| Svelte server | every request currently claiming success | parse/link and render; an existing typed unsupported result is not a success cell |
| PublicApi/TSC/declaration | Vue and Svelte public/default route/profile combinations | exact TypeScript observable fixture |
| diagnostics/maps/CSS | every route that publishes them alongside another product | atomic set, diagnostic/map validity and no unrequested artifact |
| NAPI/WASM/host/bundler | every public/default spelling currently returning success | same semantic probe plus route identity |

Each row expands to exact request JSON/typed construction, route, profile, output
digests, failure stage, and later owner. The first failing minimum probe is sufficient
to retract but not to skip recording other independently reachable cells.

## Retraction records

Use stable `BF3-RET-*` IDs, with `BF3-RET-SVELTE-*` for Svelte runtime records. A record
contains exact domain, request, route, detected typed predicate, proof that detection
precedes publication, typed non-success, complete product-withholding assertion, local
regression, correction owner (`BV1` or `BS1`), and removal acceptance ID. Whole-cell
retraction is mandatory when the safe predicate cannot distinguish the broken subset.

BF3 cannot implement the later correction. A guard that survives its correction
owner's acceptance fails that owner's exit.
