# BV1 — Vue compiler-output conformance train

**Status:** PROPOSED / LOCKED. **Class:** Framework subsystem.
**Predecessor:** B4.

## Objective

Deliver Vue compiler correctness on the final B2–B4 substrate for the exact
`vuejs/core v3.6.0-rc.3` domain.

## Owned scope

- Vue-owned semantic model and requested product plans;
- client VDOM and client Vapor generated JavaScript as separate families;
- server/SSR JavaScript and official RC.3 topology;
- script/template assembly; development/production; JavaScript/TypeScript;
- helper/import/call topology, macros, components, directives, events, props,
  attributes, class/style, slots, fragments, Teleport, Suspense, and async setup;
- scoped/slotted metadata, CSS-variable code effects, custom elements, delimiters,
  whitespace/comments, hoisting, handler caching, and binding metadata;
- diagnostics, maps, PublicApi/TSC/declaration/tooling products;
- source-local macro behavior and closed typed demands for imported project data; and
- the complete accepted official-core Vue pack.

BV1 does not implement Vue runtime code, invent `SSR x Vapor`, use an official
compiler in production, or fulfil project-aware imported macro demands. C3 fulfils
those demands without replacing codegen.

`FC-TS-001-LOCAL` is BV1's independently closable partition of `FC-TS-001`. It
proves the source-local PublicApi, TSC/TSX, and declaration cells and proves the
BV1 side of the closed BV1→C3 protocol in `C3.md` with typed deterministic
stubs: every demand kind is planned with the specified identity and order, every
`Success` payload is consumed, and every omitted, `NotFound`, `Stale`, or `Error`
result produces the specified typed non-success without partial publication. It
requires no C3 implementation or live project resolver. `FC-TS-001-PROJECT` is
C3's later end-to-end partition; it combines accepted BV1 codegen with the real
project/type substrate and closes `FC-TS-001` for the jointly owned Vue cells.

## Required exits

`FC-VUE-001`, `FC-HYDRATION-001`, `FC-TS-001-LOCAL`, `FC-ATOMIC-001`,
`FC-ZERO-WORK-001`, and applicable `FC-PERF-001` cells pass. Every BV1-owned or
source-local Vue cell has no blocked official case or semantic known-divergence;
jointly owned project-aware cells remain projection-required until
`FC-TS-001-PROJECT`. Output parses, links to the exact packages, matches protected
structure/topology, executes deterministically, maps accurately, and removes every
corresponding BF3 guard. Vue RC maturity is not Stable.

## Abort/rescope

Stop for a demanded project fact not expressible by the closed typed protocol, an
unlocked compatibility change, an official topology the product model cannot express,
or pressure to share a universal semantic/runtime IR with Svelte.
