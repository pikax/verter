# BS1 — Svelte compiler-output conformance train

**Status:** PROPOSED / LOCKED. **Class:** Framework subsystem.
**Predecessor:** B4.

## Objective

Deliver Svelte compiler correctness on the final B2–B4 substrate for exact
`svelte@5.56.8`.

## Owned scope

- Svelte-owned semantic model and product plans;
- client and server JavaScript; development and production;
- runes and legacy behavior only where the capability lock claims them;
- Svelte-native helpers, effects, blocks, events, bindings, actions, transitions,
  animations, components, dynamic elements/components, DOM, and server topology;
- stores, context, slots/children, snippets, boundaries, async behavior, namespaces,
  hydration-compatible output, custom elements, whitespace/comments;
- diagnostics, maps, PublicApi/TSC/declaration/tooling products; and
- the complete accepted official-core Svelte pack.

BS1 does not implement a Svelte runtime, use an official compiler in production, or
automatically widen Verter's product API because the official compiler returns an
extra artifact.

## Required exits

`FC-SVELTE-001`, `FC-HYDRATION-001`, `FC-TS-001`, `FC-ATOMIC-001`,
`FC-ZERO-WORK-001`, and applicable `FC-PERF-001` cells pass. Every supported cell has
no blocked official case or semantic known-divergence. Structural and dependency
proofs demonstrate that no Vue semantic, IR, lowering, helper, hydration, event,
component, or SSR assumption remains anywhere in Svelte implementation paths. Every
corresponding BF3 guard is removed.

## Abort/rescope

Stop for an unlocked compatibility change, an unsupported official mode presented as
success, an output requiring runtime patching, or any design that bases Svelte on Vue.
