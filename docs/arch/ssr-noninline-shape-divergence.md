# SSR non-inline module shape — ratified interim divergence

Status: **ratified interim divergence** (recorded during the tsc-performance
review integration). Owner: the Vue runtime codegen lane
(`crates/verter_compiler/src/script/process.rs`,
`crates/verter_compiler/src/template/code_gen/binding.rs`,
`crates/verter_compiler/src/template/code_gen/ssr/`).

## The divergence

Official `@vitejs/plugin-vue` + `@vue/compiler-sfc` compile `<script setup>`
SSR in **inline mode**: `setup()` returns the render function, bindings close
over setup scope, and the component object carries `__isScriptSetup: true`.
The dev **non-inline** shape (what official emits when inline is off) is an
8-parameter `ssrRender(_ctx, _push, _parent, _attrs, $props, $setup, $data,
$options)` with `$setup.*` / `$props.*` member routing, plus the
`__isScriptSetup` marker.

Verter's SSR lane is **non-inline** but emits a deliberately different shape:

1. `ssrRender(_ctx, _push, _parent, _attrs)` — 4 parameters;
2. every binding routes through `_ctx.*` (never a free `$setup` identifier,
   never `$setup.*` member routing);
3. the SSR-compiled component does **not** set `__isScriptSetup` (the client
   build DOES set it); `setup` returns a plain object.

## Why not the official 8-param shape (verified evidence)

- Free `$setup` identifiers throw under `@vue/server-renderer` in this
  wrapper shape (`$setup` is only bound when the renderer calls the 8-param
  signature with the instance's `setupState`).
- `hasSetupBinding` in the runtime **skips** `__isScriptSetup`-marked state
  for `_ctx` proxy routing: keeping the marker while routing bindings through
  `_ctx.*` makes every setup binding unreachable on the server. Dropping the
  marker makes the instance proxy expose setup keys via `_ctx.*`, which is
  what the emitted code relies on.

So the three parts of the divergence are mutually consistent: `_ctx.*`
routing requires the marker to be absent, and the 4-param signature is
sufficient because nothing references `$props/$setup/$data/$options`.
Adopting the official 8-param encoding piecemeal (marker back, or `$setup.*`
routing without the renderer contract) breaks at runtime; the correct
alignment is **true-inline SSR** (setup returns the render function), which
is a separate codegen project, not a patch to this lane.

## Compatibility consequences (accepted until inline SSR lands)

- **Tooling identification**: `@vue/test-utils`, devtools, and any tooling
  that identifies `<script setup>` components by `__isScriptSetup` will not
  identify the SSR-compiled module as script-setup. The CLIENT build keeps
  the marker, so browser-side tooling is unaffected.
- **Silent leniency**: options-API / mixin `this.<setupKey>` access **succeeds**
  on the server where official warns/blocks (the proxy exposes setup keys
  without the marker's dev-guard).
- **Future runtime behavior** keyed on the marker diverges on the SSR side
  until alignment.

Guard state: the shape is pinned both directions by
`crates/verter_compiler/src/template/code_gen/ssr/tests.rs` (all-`_ctx.`
routing, no free `$setup`, no `__ssrInlineRender`) and the script-flag tests
(`__isScriptSetup` present client / absent SSR).

## Exit criterion

This divergence is retired by implementing true-inline SSR for
`<script setup>` (setup returns the render function, official inline
semantics), at which point the marker returns and this document is deleted
in the same change.
