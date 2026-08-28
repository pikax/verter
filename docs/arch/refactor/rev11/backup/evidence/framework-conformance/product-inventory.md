# Framework compiler product and route inventory

This is the BF1 starting inventory, not an assertion that current output is correct.

## Product families

| product | framework owner after rescope | profiles | publication rule |
|---|---|---|---|
| parsed SFC identities/diagnostics | B2, framework-local | Vue; Svelte | typed parse result; no runtime artifact on rejection |
| RuntimeClient VDOM | BV1 | Vue dev/prod, map on/off | atomic requested artifact |
| RuntimeClient Vapor | BV1 | Vue dev/prod, map on/off | separate RC family; atomic |
| RuntimeServer SSR | BV1 | Vue dev/prod, map on/off | official SSR topology; not a generic Vapor backend |
| RuntimeClient | BS1 | Svelte runes/legacy where claimed, dev/prod, map on/off | Svelte-native atomic artifact |
| RuntimeServer | BS1 | Svelte runes/legacy where claimed, dev/prod, map on/off | Svelte-native atomic artifact |
| PublicApi | BV1 or BS1 | framework and exact TS domain | established product only |
| Tsc/TSX projection | BV1 or BS1; C3 fulfils imported Vue demands | framework and exact TS domain | established product only |
| declaration | BV1 or BS1 | framework and exact TS domain | established product only |
| diagnostics | syntax B2; semantics framework train | all applicable profiles | in atomic success set or typed non-success, never partial publication |
| source maps | B4 composition; semantic segments from framework train | on/off | only when requested |
| CSS/code-affecting style metadata | framework train; existing CSS owner otherwise retained | applicable SFC modes | no automatic product widening |

## Route reachability at package preparation

| route | current observation | Revision 11 owner |
|---|---|---|
| internal compiler one-shot | present, not the final public direct core | B5 cutover |
| host per-file/virtual-product routes | present and default for several products | later host route owners consume B5/B6 |
| host/NAPI `compile_many` | present, but predates B6's final direct batch | B6 then later NAPI equivalence |
| NAPI `compile_with_audit` | present managed route | later route-owning block |
| WASM compile/audit and virtual products | present | later WASM equivalence |
| direct one-shot final core | absent before B5 | B5 |
| prepared first/repeat final core | absent before B6 | B6 |
| direct batch final core | absent before B6 | B6 |
| project-aware staged compile | existing pieces, final sealed route later | C2/C3 then C4 proof |
| bundler/unplugin managed publication | present in legacy/current form | later route-owning block after C4 core proof |

B3 must enumerate every public/default route and map it to one canonical request.
Transport conversion may not reinterpret semantic defaults. C4 proves only routes
that exist by B6/C3; NAPI, WASM, bundler, and managed publication retain their later
owners.

## Current correctness hazards to probe

- current official pins differ from both required domains;
- current Vue conformance has an RC.1 known-divergence list, inadmissible for a
  supported RC.3 cell;
- current Svelte conformance targets 5.56.3 and contains known-divergence mechanisms;
- Svelte server generation currently has an explicit fail-closed/absent backend;
- current carrier/profile surfaces do not express the complete option inventories;
- legacy routes can produce multiple products without B4's final atomic contract.

These observations authorize BF1 classification and BF3 probes only. They are not
pre-judged backend defects and do not authorize production changes in this package.
