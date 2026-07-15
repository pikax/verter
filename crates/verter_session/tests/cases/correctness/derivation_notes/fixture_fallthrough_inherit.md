# CLAUDE.md §Fallthrough — `inheritAttrs: false` zeros the surface

Source: CLAUDE.md "Fallthrough / Root Inheritance (CRITICAL)" plus
`./.claude/skills/component-meta` "Fallthrough / Root Inheritance
(CRITICAL)" Semantic rules — first bullet:

> `inheritAttrs: false` -- no inherited surface

For the fixture

```ts
defineOptions({ inheritAttrs: false });
defineProps<{ disabled?: boolean }>();
```

with template `<button />`, the resolver:

1. Records `flags.has_inherit_attrs_false = true` from
   `defineOptions({ inheritAttrs: false })`.
2. Sets `fallthrough_surface = FallthroughSurface::None { reason:
   InheritAttrsFalse }` per the host inheritance resolver.
3. Surfaces the declared prop `disabled?: boolean` on `props` with
   `required = false` (the source declared `?`).

The snapshot view's `build_fallthrough_view` projects this as
`Some(FallthroughView { inherit_attrs: false, surface_signature:
"{}" })` — the projection rule documented in `snapshot_view.rs`
emits the fallthrough block exactly when (a) the SFC opted out via
`inheritAttrs: false` or (b) the inherited surface includes a
component-sourced entry. (a) applies here.

Discriminating-test linkage (§0p.A.5):
- `MutationKind::FallthroughInheritFlipped` — flipping
  `fallthrough.inherit_attrs` from `false` to `true` must fail the
  gate. The fixture has `Some(FallthroughView)` so the mutation has
  a live target.

Negative assertion: `events`, `slots`, `models`, and `exposed` are
all empty — the SFC has only `defineOptions` + `defineProps`.
